//! Doing one job on this machine.
//!
//! The work is read out of git at the commit the job names, never out of
//! the working tree: the tree belongs to whoever is at the keyboard, it
//! may be half-written, and nothing here touches it.
//!
//! What comes back from the agent stays here. The instance is told the
//! type of what happened and one line, which is what the group reads.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use stemma_core::job::{Outcome, Task};
use stemma_core::line::{BlackBox, Line, Siglum};
use stemma_core::proposal::Offer;
use stemma_core::repository::{self, Side};

use crate::agents::{self, Ran};
use crate::api::Handed;
use crate::config::Config;
use crate::home::Home;

/// Which revision of the prompt this build sends.
///
/// It is recorded with every run, because "an agent said so" is not an
/// attribution: the vendor, the model and the words it was given are.
pub const PROMPT_REVISION: &str = "1";

/// Carries out one job and says what came of it.
///
/// Nothing in here fails: every way this can go wrong is one of the
/// outcomes, because a job that ends in nothing at all leaves the person
/// who launched it watching a queue.
pub fn carry_out(home: &Home, config: &Config, handed: &Handed) -> Completed {
    let task: Task = match handed.task.parse() {
        Ok(task) => task,
        Err(problem) => return Completed::only(Outcome::out_of_reach(problem.to_string())),
    };

    let source = match read_source(home, config, handed) {
        Ok(source) => source,
        Err(why) => return Completed::only(Outcome::out_of_reach(why)),
    };

    let line = match Line::read(&source) {
        Ok(line) => line,
        Err(faults) => {
            return Completed::only(Outcome::out_of_reach(format!(
                "`{}` cannot be read at that commit: {}",
                handed.line,
                faults
                    .first()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "it is not a line".to_owned())
            )));
        }
    };

    let other = match &task {
        Task::Reconcile => match handed.against.as_deref() {
            Some(commit) => match read_at(home, config, handed, commit) {
                Ok(other) => Some(other),
                Err(why) => return Completed::only(Outcome::out_of_reach(why)),
            },
            None => {
                return Completed::only(Outcome::out_of_reach(
                    "this job names no second version to reconcile with".to_owned(),
                ));
            }
        },
        _ => None,
    };

    let ran = match agents::run(
        config,
        &handed.agent,
        &prompt(handed, &task, &line, &source, other.as_deref()),
        &agents::output_path(home, &handed.job),
    ) {
        Ok(ran) => ran,
        Err(error) => return Completed::only(Outcome::out_of_reach(format!("{error:#}"))),
    };

    let outcome = became_of(&task, &handed.agent, &ran);
    let proposal = match (&outcome, &ran) {
        (Outcome::Produced { .. }, Ran::Finished(finished)) => {
            proposal(&handed.agent, &finished.output)
        }
        _ => None,
    };

    if matches!(task, Task::Formalize { .. } | Task::Verify { .. }) {
        let checked = proposal
            .as_ref()
            .and_then(|offer| offer.lean_source.as_deref())
            .ok_or_else(|| "the agent offered no complete Lean module".to_owned())
            .and_then(|lean_source| {
                let clone = clone_of(home, config, handed)
                    .ok_or_else(|| "this machine has no matching clone".to_owned())?;
                let manifest = crate::build::read_manifest(&clone).map_err(|e| e.to_string())?;
                let pins = crate::build::group_pins(&clone, &manifest);
                crate::lean::check_offered(
                    &clone,
                    &pins,
                    lean_source,
                    &home
                        .cache_dir()
                        .join("lean")
                        .join(format!("{}.lean", handed.job)),
                )?;
                let declared = crate::lean::declared_statements(lean_source);
                match &task {
                    Task::Formalize { statement } if !declared.contains(statement) => Err(format!(
                        "the compiled module does not declare that it verifies `{statement}`"
                    )),
                    Task::Verify { after } => {
                        let black_box = line
                            .boxes()
                            .into_iter()
                            .find(|black_box| &black_box.after == after)
                            .ok_or_else(|| "the black box is no longer there".to_owned())?;
                        let missing: Vec<String> = black_box
                            .interior
                            .iter()
                            .filter(|statement| !declared.contains(statement))
                            .map(ToString::to_string)
                            .collect();
                        if missing.is_empty() {
                            Ok(())
                        } else {
                            Err(format!(
                                "the compiled module does not verify {}",
                                missing.join(", ")
                            ))
                        }
                    }
                    _ => Ok(()),
                }
            });
        if let Err(said) = checked {
            return Completed::only(Outcome::Broke {
                code: "lean_rejected".to_owned(),
                said,
            });
        }
    }

    let outcome = match (&outcome, &proposal) {
        (Outcome::Produced { .. }, None) => Outcome::Broke {
            code: "malformed_proposal".to_owned(),
            said: "the agent reported an answer without reviewable source and a concrete model"
                .to_owned(),
        },
        _ => outcome,
    };
    Completed { outcome, proposal }
}

/// Everything this machine returns after doing one job.
pub struct Completed {
    /// The typed public outcome.
    pub outcome: Outcome,
    /// The source offered for review, when the run produced one.
    pub proposal: Option<Offer>,
}

impl Completed {
    fn only(outcome: Outcome) -> Self {
        Self {
            outcome,
            proposal: None,
        }
    }
}

const PROPOSAL_START: &str = "stemma: proposal";
const PROPOSAL_END: &str = "stemma: end proposal";
const LEAN_START: &str = "stemma: lean";
const LEAN_END: &str = "stemma: end lean";
const MODEL: &str = "stemma: model ";

fn proposal(agent: &str, output: &str) -> Option<Offer> {
    let start = output.find(PROPOSAL_START)? + PROPOSAL_START.len();
    let rest = &output[start..];
    let end = rest.find(PROPOSAL_END)?;
    let source = rest[..end].trim_matches('\n').to_owned();
    let model = output
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix(MODEL))?
        .trim();
    if source.is_empty() || model.is_empty() {
        return None;
    }
    let vendor = stemma_core::machine::known(agent)?.vendor.to_owned();
    Some(Offer {
        source,
        lean_source: section(output, LEAN_START, LEAN_END),
        vendor,
        model: model.to_owned(),
        prompt_revision: PROMPT_REVISION.to_owned(),
    })
}

fn section(output: &str, start: &str, end: &str) -> Option<String> {
    let rest = &output[output.find(start)? + start.len()..];
    let text = rest[..rest.find(end)?].trim_matches('\n').to_owned();
    (!text.is_empty()).then_some(text)
}

/// What a run of an agent came to.
fn became_of(task: &Task, agent: &str, ran: &Ran) -> Outcome {
    let named = match stemma_core::id::Slug::parse(agent) {
        Ok(agent) => agent,
        Err(problem) => return Outcome::out_of_reach(problem.to_string()),
    };

    if let Some(failure) = agents::trouble(&named, ran) {
        return Outcome::from_failure(&failure);
    }

    let Ran::Finished(finished) = ran else {
        return Outcome::out_of_reach("the agent did not run");
    };

    // Tried and the mathematics did not work is an ordinary outcome of
    // research: the box goes back to its hatch, and nothing is owed.
    match found_nothing(&finished.output) {
        true => Outcome::NothingFound {
            said: nothing_found(task).to_owned(),
        },
        false => Outcome::Produced {
            said: "there is an answer, on the machine that produced it".to_owned(),
        },
    }
}

/// The word an agent ends with when it did not find anything.
const NOTHING: &str = "stemma: nothing";

/// Whether the agent said it found nothing.
fn found_nothing(output: &str) -> bool {
    output
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .any(|line| line.eq_ignore_ascii_case(NOTHING))
}

/// What "it found nothing" means for this task, in the group's words.
fn nothing_found(task: &Task) -> &'static str {
    match task {
        Task::Fill { .. } => "no argument was found",
        // Never "the statement holds". An absence of a refutation is
        // evidence of nothing, and the words have to say so.
        Task::Refute { .. } => "no counterexample was found, which settles nothing",
        Task::Formalize { .. } => "no formalization was found",
        Task::Verify { .. } => "no Lean argument was found",
        Task::Promote { .. } => "no justified promotion was found",
        Task::Explore => "nothing to suggest",
        Task::Reconcile => "no reconciliation was found",
    }
}

/// The source of the line a job is about, at the commit it names.
fn read_source(home: &Home, config: &Config, handed: &Handed) -> Result<String, String> {
    read_at(home, config, handed, &handed.commit)
}

/// The source of the same line at any commit the job names.
fn read_at(home: &Home, config: &Config, handed: &Handed, commit: &str) -> Result<String, String> {
    let clone = clone_of(home, config, handed)
        .ok_or_else(|| format!("this machine has no clone of {}", handed.group))?;

    if !crate::git::holds(&clone, commit) {
        // One fetch, in case the commit landed after the last sync. The
        // tree is left exactly as it was: nothing here resets anything.
        let _ = crate::git::fetch(&clone);
    }

    if !crate::git::holds(&clone, commit) {
        return Err(format!(
            "this machine does not have commit {} of {}",
            short(commit),
            handed.group
        ));
    }

    let inside = format!(
        "{}/{}/{}",
        repository::LINES,
        handed.line,
        repository::ENTRY
    );

    crate::git::show(&clone, commit, &inside)
        .map_err(|_| format!("`{}` is not in that commit", handed.line))
}

/// Where this machine keeps the repository a job is about.
fn clone_of(home: &Home, config: &Config, handed: &Handed) -> Option<PathBuf> {
    let side = match handed.side.parse::<Side>().ok()? {
        Side::Shared => crate::workspace::Side::Shared,
        Side::Private => crate::workspace::Side::Private,
    };

    let workspace = config.workspace_or(home.workspace());
    let clone = config
        .group_directory(&handed.group, &workspace)
        .join(side.directory());

    crate::git::is_clone(&clone).then_some(clone)
}

/// What the agent is asked.
///
/// The rules come first because they are the point: what the line rests
/// on is frozen, the interior of a box is the agent's, and an absence of
/// a refutation is evidence of nothing.
fn prompt(handed: &Handed, task: &Task, line: &Line, source: &str, other: Option<&str>) -> String {
    let mut asked = String::new();

    let _ = writeln!(
        asked,
        "You are working on one line of mathematical work, written in Typst. \
         It belongs to the group `{}`, and this is its state at commit {}.",
        handed.group,
        short(&handed.commit)
    );
    let _ = writeln!(asked);
    let _ = writeln!(
        asked,
        "Three rules hold, whatever you are asked to do:\n\
         \n\
         1. A statement declared `central: true` is what the line rests on. \
            Do not change its text and do not change what it says it uses.\n\
         2. Everything between two central statements is yours to write, \
            repair or rewrite.\n\
         3. Say what you lean on. A statement that uses another declares it, \
            and an argument that quietly assumes something undeclared is not \
            an argument."
    );
    let _ = writeln!(asked);
    let _ = writeln!(asked, "The line as it stands:");
    let _ = writeln!(asked, "----------------------------------------");
    let _ = writeln!(asked, "{}", source.trim());
    let _ = writeln!(asked, "----------------------------------------");
    let _ = writeln!(asked);
    let _ = writeln!(asked, "{}", asked_of(task, line, other));
    let _ = writeln!(asked);
    let _ = writeln!(
        asked,
        "When you have a change to offer, print `stemma: proposal` on its own, \
         then the complete proposed Typst source, then `stemma: end proposal`. \
         If Lean changes, then print `stemma: lean`, the complete `Main.lean` module, \
         and `stemma: end lean`. After it print `stemma: model MODEL-ID` with the concrete model you used, \
         and finally `stemma: produced`. If you found nothing, end with `{NOTHING}`. \
         Finding nothing is an ordinary answer and a useful one."
    );

    asked
}

/// The one thing this job asks for.
fn asked_of(task: &Task, line: &Line, other: Option<&str>) -> String {
    match task {
        Task::Fill { after } => {
            let between = line
                .boxes()
                .into_iter()
                .find(|black_box| &black_box.after == after);

            match between {
                Some(black_box) => format!("Fill the black box {}.", bounded(&black_box)),
                None => "Fill the black box that box refers to, which is no longer there. \
                         Say so rather than guessing."
                    .to_owned(),
            }
        }
        Task::Refute { statement } => format!(
            "Look for a counterexample to `{statement}`. If you do not find one, \
             that settles nothing and you must not say it does: say only that you \
             did not find one."
        ),
        Task::Formalize { statement } => format!(
            "Formalize `{statement}` in Lean. Keep its prose fixed. The Lean module must include \
             the exact marker `-- stemma: verifies {statement}` immediately above the declaration."
        ),
        Task::Verify { after } => {
            let between = line
                .boxes()
                .into_iter()
                .find(|black_box| &black_box.after == after);
            match between {
                Some(black_box) => format!(
                    "Write a Lean argument verifying every statement in the black box {}. \
                     Mark each compiled statement with `-- stemma: verifies SIGLUM`.",
                    bounded(&black_box)
                ),
                None => "The black box to verify is no longer there. Say so rather than guessing."
                    .to_owned(),
            }
        }
        Task::Promote { statement } => format!(
            "Promote `{statement}` out of its black box by declaring it `central: true`. \
             Do not change its claim. Explain why it deserves a stable name; promotion freezes \
             it, splits the surrounding box, and reserves a human correspondence audit."
        ),
        Task::Explore => "Say where this line could go next: what is worth stating, \
                          and why it follows from what is already here."
            .to_owned(),
        Task::Reconcile => format!(
            "Two of this person's machines wrote this line, and the versions have parted. \
             Bring them together into one source that keeps everything either version \
             established and says, for every statement they write differently, the version \
             that is right. Central statements stay exactly as they are in both. You are \
             offering a proposal, not settling it: a person decides.\n\
             \n\
             The other version:\n\
             ----------------------------------------\n\
             {}\n\
             ----------------------------------------",
            other.unwrap_or("it is not here").trim()
        ),
    }
}

/// A box, said by its two ends.
fn bounded(black_box: &BlackBox) -> String {
    let named = |siglum: &Option<Siglum>| match siglum {
        Some(siglum) => format!("`{siglum}`"),
        None => "the start of the line".to_owned(),
    };

    match (&black_box.after, &black_box.before) {
        (after, Some(before)) => format!("between {} and `{before}`", named(after)),
        // The end of a line is open, and that is the ordinary state of
        // work in progress rather than something missing.
        (after, None) => format!("that begins after {} and is still open", named(after)),
    }
}

/// Where the raw output of a job is, for the person whose machine it is.
pub fn output_of(home: &Home, job: &str) -> PathBuf {
    agents::output_path(home, job)
}

/// A commit, short enough to read.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(8)]
}

/// Whether the file holding a job's output is still there.
pub fn output_exists(home: &Home, job: &str) -> bool {
    Path::new(&output_of(home, job)).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Finished;

    const SOURCE: &str = r##"
= Quotient groups

#theorem(id: "thm:first", name: "First", central: true)[y]

#lemma(id: "lem:kernel")[z]

#theorem(id: "thm:second", name: "Second", central: true)[v]
"##;

    fn handed(task: &str) -> Handed {
        Handed {
            job: "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T".to_owned(),
            group: "algebra".to_owned(),
            side: "shared".to_owned(),
            line: "quotient-groups".to_owned(),
            commit: "0123456789abcdef".to_owned(),
            against: None,
            task: task.to_owned(),
            agent: "claude".to_owned(),
            launched_by: "usr:ana".to_owned(),
        }
    }

    fn line() -> Line {
        Line::read(SOURCE).unwrap()
    }

    #[test]
    fn a_box_is_asked_for_by_both_its_ends() {
        let task: Task = "fill@thm:first".parse().unwrap();
        let asked = asked_of(&task, &line(), None);

        assert!(
            asked.contains("between `thm:first` and `thm:second`"),
            "{asked}"
        );

        let first: Task = "fill".parse().unwrap();
        assert!(asked_of(&first, &line(), None).contains("between the start of the line"));
    }

    #[test]
    fn the_prompt_says_what_an_agent_may_not_touch() {
        let task: Task = "fill@thm:first".parse().unwrap();
        let asked = prompt(&handed("fill@thm:first"), &task, &line(), SOURCE, None);

        assert!(asked.contains("Do not change its text"));
        assert!(asked.contains("thm:second"));
        assert!(asked.contains("stemma: nothing"));
    }

    #[test]
    fn an_agent_looking_for_a_counterexample_is_told_what_not_finding_one_means() {
        let task: Task = "refute@thm:first".parse().unwrap();
        let asked = asked_of(&task, &line(), None);

        assert!(asked.contains("settles nothing"));
    }

    #[test]
    fn finding_no_counterexample_is_never_reported_as_the_statement_holding() {
        let task: Task = "refute@thm:first".parse().unwrap();
        let ran = Ran::Finished(Finished {
            status: Some(0),
            output: "I looked and found nothing.\nstemma: nothing\n".to_owned(),
            gave_up: false,
        });

        let outcome = became_of(&task, "claude", &ran);

        assert!(matches!(outcome, Outcome::NothingFound { .. }));
        assert_eq!(
            outcome.said(),
            "no counterexample was found, which settles nothing"
        );
        assert!(outcome.taught_anything());
        assert!(!outcome.is_the_machines());
    }

    #[test]
    fn an_agent_that_said_nothing_in_particular_produced_something_to_look_at() {
        let task: Task = "fill".parse().unwrap();
        let ran = Ran::Finished(Finished {
            status: Some(0),
            output: "Here is an argument.\nstemma: produced\n".to_owned(),
            gave_up: false,
        });

        assert!(matches!(
            became_of(&task, "claude", &ran),
            Outcome::Produced { .. }
        ));
    }

    #[test]
    fn a_proposal_carries_complete_source_and_concrete_attribution() {
        let output = "notes\nstemma: proposal\n= Better line\n#lemma(id: \"lem:a\")[x]\nstemma: end proposal\nstemma: model claude-opus-5\nstemma: produced\n";
        let offered = proposal("claude", output).unwrap();
        assert!(offered.source.starts_with("= Better line"));
        assert_eq!(offered.vendor, "Anthropic");
        assert_eq!(offered.model, "claude-opus-5");
    }

    #[test]
    fn a_missing_agent_never_becomes_a_statement_about_the_mathematics() {
        let task: Task = "fill".parse().unwrap();
        let outcome = became_of(&task, "claude", &Ran::Missing);

        assert!(outcome.is_the_machines());
        assert!(outcome.is_the_owners_to_fix());
        assert!(!outcome.taught_anything());
    }

    #[test]
    fn the_marker_is_only_read_at_the_end() {
        assert!(found_nothing("nothing yet\nstemma: nothing"));
        assert!(found_nothing("stemma: nothing\n\n"));
        assert!(!found_nothing(
            "stemma: nothing\nand then an argument\nand more\nand more"
        ));
        assert!(!found_nothing(
            "an argument that mentions stemma: nothing in passing here"
        ));
    }
}

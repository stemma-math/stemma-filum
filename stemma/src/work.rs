//! Doing one job on this machine.
//!
//! The work is read from the instance at the revision the job names,
//! never out of the files here: those belong to whoever is at the
//! keyboard, they may be half-written, and nothing here touches them.
//!
//! What comes back from the agent stays here. The instance is told the
//! type of what happened and one line, which is what the group reads.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use stemma_core::id::BlockId;
use stemma_core::job::{Outcome, Task};
use stemma_core::line::{BlackBox, Line};
use stemma_core::proposal::{FormalReview, LibraryChange, Offer};
use stemma_core::repository::{self, Side};

use crate::agents::{self, Ran};
use crate::api::Handed;
use crate::config::Config;
use crate::home::Home;
use crate::mirror::{Instance, Mirror, Remote, Which};

/// Which revision of the prompt this build sends.
///
/// It is recorded with every run, because "an agent said so" is not an
/// attribution: the vendor, the model and the words it was given are.
pub const PROMPT_REVISION: &str = "3";

/// Carries out one job and says what came of it.
///
/// Nothing in here fails: every way this can go wrong is one of the
/// outcomes, because a job that ends in nothing at all leaves the person
/// who launched it watching a queue.
pub fn carry_out(home: &Home, config: &Config, instance: &Instance, handed: &Handed) -> Completed {
    let task: Task = match handed.task.parse() {
        Ok(task) => task,
        Err(problem) => return Completed::only(Outcome::out_of_reach(problem.to_string())),
    };

    if task == Task::Build {
        return build(home, instance, handed);
    }

    let source = match read_at(instance, handed, handed.revision) {
        Ok(source) => source,
        Err(why) => return Completed::only(Outcome::out_of_reach(why)),
    };

    let line = match Line::read(&source) {
        Ok(line) => line,
        Err(faults) => {
            return Completed::only(Outcome::out_of_reach(format!(
                "`{}` cannot be read at that revision: {}",
                handed.line,
                faults
                    .first()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "it is not a line".to_owned())
            )));
        }
    };

    let other = match &task {
        Task::Reconcile => match handed.against {
            Some(revision) => match read_at(instance, handed, revision) {
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
    let mut proposal = match (&outcome, &ran) {
        (Outcome::Produced { .. }, Ran::Finished(finished)) => {
            proposal(&handed.agent, &finished.output)
        }
        _ => None,
    };

    let reviews_library = proposal
        .as_ref()
        .is_some_and(|offer| offer.library_change.is_some());
    if reviews_library || needs_lean_check(&task, &outcome) {
        let checked = proposal
            .as_mut()
            .ok_or_else(|| "the agent offered no formal change".to_owned())
            .and_then(|offer| {
                if let Some(change) = &offer.library_change {
                    check_library_offer(change, &task, &line, &handed.line)?;
                    let clone = repository_of(home, handed).ok_or_else(|| {
                        "this machine does not have the line's repository yet".to_owned()
                    })?;
                    let manifest =
                        crate::build::read_manifest(&clone).map_err(|e| e.to_string())?;
                    let pins = crate::build::group_pins(&clone, &manifest);
                    let name = stemma_core::line::Name::parse(&handed.line)
                        .map_err(|error| error.to_string())?;
                    let targets = formal_targets(&task, &line)?;
                    let review = crate::lean::review_change(
                        &clone,
                        &name,
                        &pins,
                        change,
                        &offer.source,
                        &home.cache_dir().join("lean").join(&handed.job),
                        &targets,
                    )
                    .map_err(|error| format!("{error:#}"))?;
                    let proposed_line = Line::read(&offer.source).map_err(|faults| {
                        faults
                            .first()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "the proposed line is unreadable".to_owned())
                    })?;
                    check_review(&review, &targets, &proposed_line)?;
                    offer.formal_review = Some(Box::new(review));
                    return Ok(());
                }
                let lean_source = offer
                    .lean_source
                    .as_deref()
                    .ok_or_else(|| "the agent offered no complete Lean module".to_owned())?;
                let clone = repository_of(home, handed).ok_or_else(|| {
                    "this machine does not have the line's repository yet".to_owned()
                })?;
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
                    Task::Formalize { block } | Task::FollowRoute { block } => {
                        let statement = line
                            .node_at(block)
                            .ok_or_else(|| "the statement block is no longer there".to_owned())?;
                        if declared.contains(&statement.siglum) {
                            Ok(())
                        } else {
                            Err(format!(
                                "the compiled module does not declare that it verifies `{block}`"
                            ))
                        }
                    }
                    Task::Verify { after } => {
                        let black_box = line
                            .boxes()
                            .into_iter()
                            .find(|black_box| &black_box.after == after)
                            .ok_or_else(|| "the black box is no longer there".to_owned())?;
                        let missing: Vec<String> = black_box
                            .interior
                            .iter()
                            .filter_map(|block| line.node_at(block))
                            .filter(|statement| !declared.contains(&statement.siglum))
                            .map(|statement| statement.block.to_string())
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
        (Outcome::Produced { .. }, Some(offer)) => match Line::read(&offer.source) {
            Ok(_) => outcome,
            Err(faults) => {
                return Completed::only(Outcome::Broke {
                    code: "invalid_line".to_owned(),
                    said: format!(
                        "the agent offered something that is not a valid line: {}",
                        faults
                            .first()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "it is not a line".to_owned())
                    ),
                });
            }
        },
        _ => outcome,
    };
    Completed { outcome, proposal }
}

fn needs_lean_check(task: &Task, outcome: &Outcome) -> bool {
    matches!(
        task,
        Task::Formalize { .. } | Task::FollowRoute { .. } | Task::Verify { .. }
    ) && matches!(outcome, Outcome::Produced { .. })
}

fn check_library_offer(
    change: &LibraryChange,
    task: &Task,
    line: &Line,
    line_name: &str,
) -> Result<(), String> {
    if change.files.is_empty() {
        return Err("the agent offered an empty Lean library change".to_owned());
    }
    if let Some(file) = change.files.iter().find(|file| file.before == file.after) {
        return Err(format!("the agent did not change {}", file.path));
    }
    let marked: std::collections::BTreeSet<String> = change
        .files
        .iter()
        .filter_map(|file| file.after.as_deref())
        .flat_map(|source| {
            source
                .split("@[stemma \"")
                .skip(1)
                .filter_map(|rest| rest.split_once('"').map(|(mark, _)| mark.to_owned()))
        })
        .collect();
    let required: Vec<BlockId> = match task {
        Task::Formalize { block } | Task::FollowRoute { block } => vec![*block],
        Task::Verify { after } => {
            line.boxes()
                .into_iter()
                .find(|black_box| &black_box.after == after)
                .ok_or_else(|| "the black box is no longer there".to_owned())?
                .interior
        }
        _ => Vec::new(),
    };
    let missing: Vec<_> = required
        .into_iter()
        .filter(|block| !marked.contains(&format!("{line_name}/{block}")))
        .map(|block| block.to_string())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the Lean library change does not mark {}",
            missing.join(", ")
        ))
    }
}

fn formal_targets(task: &Task, line: &Line) -> Result<Vec<BlockId>, String> {
    match task {
        Task::Formalize { block } | Task::FollowRoute { block } => Ok(vec![*block]),
        Task::Verify { after } => line
            .boxes()
            .into_iter()
            .find(|black_box| &black_box.after == after)
            .map(|black_box| black_box.interior)
            .ok_or_else(|| "the black box is no longer there".to_owned()),
        _ => Ok(Vec::new()),
    }
}

fn check_review(review: &FormalReview, targets: &[BlockId], line: &Line) -> Result<(), String> {
    for block in targets {
        if !review
            .blocks
            .iter()
            .any(|reviewed| &reviewed.check.block == block)
        {
            return Err(format!("Lean produced no review evidence for `{block}`"));
        }
    }
    for reviewed in &review.blocks {
        let central = line
            .node_at(&reviewed.check.block)
            .is_some_and(stemma_core::line::Node::is_central);
        if (central || targets.contains(&reviewed.check.block))
            && reviewed.check.state != stemma_core::model::Verification::Verified
        {
            return Err(if reviewed.check.diagnostic.is_empty() {
                format!("Lean did not verify `{}`", reviewed.check.block)
            } else {
                reviewed.check.diagnostic.clone()
            });
        }
        if central && reviewed.reading.trim().is_empty() {
            return Err(format!(
                "the agent gave no plain-language reading of the formal statement for `{}`",
                reviewed.check.block
            ));
        }
    }
    Ok(())
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
const LIBRARY_START: &str = "stemma: library";
const LIBRARY_END: &str = "stemma: end library";
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
        library_change: section(output, LIBRARY_START, LIBRARY_END)
            .and_then(|change| serde_json::from_str(&change).ok()),
        formal_review: None,
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
        Task::FollowRoute { .. } => "the agent could not formalize the author's route",
        Task::Verify { .. } => "no Lean argument was found",
        Task::Promote { .. } => "no justified promotion was found",
        Task::Prove { .. } => "no proof was found",
        Task::Rewrite { .. } => "no better statement was found",
        Task::Develop { .. } => "nothing was developed",
        Task::Explore => "nothing to suggest",
        Task::Reconcile => "no reconciliation was found",
        Task::Build => "nothing was built",
    }
}

/// The source of the line a job is about, at a revision it names.
fn read_at(instance: &Instance, handed: &Handed, revision: u64) -> Result<String, String> {
    let line = handed
        .line_id
        .as_deref()
        .ok_or_else(|| "this job does not say which line it is for".to_owned())?;

    instance
        .revision(line, Some(revision))
        .map(|found| found.source)
        .map_err(|error| {
            format!(
                "revision {revision} of `{}` cannot be read: {error:#}",
                handed.line
            )
        })
}

/// Where this machine shows the repository a job is about, when it has
/// it: the files beside the lines are what Lean checks against.
fn repository_of(home: &Home, handed: &Handed) -> Option<PathBuf> {
    let side = handed.side.parse::<Side>().ok()?;
    let root = Mirror::at(home).ok()?.repository(&handed.group, side);

    root.join(repository::MANIFEST).is_file().then_some(root)
}

/// Builds the repository a line is in and publishes it, as the instance
/// asked: the machine that wrote the line did not.
fn build(home: &Home, instance: &Instance, handed: &Handed) -> Completed {
    let built = (|| -> anyhow::Result<crate::build::Published> {
        let mirror = Mirror::at(home)?;
        let round = mirror.step(instance, true, &Which::All);
        if let Some(stopped) = round.stopped {
            anyhow::bail!("{stopped}");
        }

        let side: Side = handed.side.parse()?;
        let built = crate::build::read(&mirror, &handed.group, side)?;
        let credentials = crate::credentials::Credentials::read(&home.credentials_file())?;
        let published = crate::build::publish(&built, credentials.as_ref());
        if published == crate::build::Published::Kept {
            mirror.built(&built.nodes)?;
        }

        Ok(published)
    })();

    Completed::only(match built {
        Ok(crate::build::Published::Kept) => Outcome::Produced {
            said: "its pages were built and published".to_owned(),
        },
        Ok(other) => Outcome::out_of_reach(other.to_string()),
        Err(error) => Outcome::out_of_reach(format!("{error:#}")),
    })
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
        "You are working on one line of mathematical work. It belongs to the \
         group `{}`, and this is its state at revision {}.",
        handed.group, handed.revision
    );
    let _ = writeln!(asked);
    let _ = writeln!(asked, "{FORMAT}");
    let _ = writeln!(asked);
    let _ = writeln!(
        asked,
        "Three rules hold, whatever you are asked to do:\n\
         \n\
         1. A definition or theorem with `\"central\": true` is what the line rests on. \
            Do not change its text, its title, its siglum or what it says it uses, \
            and do not delete it. Its proof is yours to write.\n\
         2. Everything between two central statements is yours to write, \
            repair or rewrite.\n\
         3. Say what you lean on. A statement that uses another lists it in `uses` \
            or cites it with `@siglum`, and an argument that quietly assumes \
            something undeclared is not an argument."
    );
    let _ = writeln!(asked);
    let _ = writeln!(asked, "The line as it stands:");
    let _ = writeln!(asked, "----------------------------------------");
    let _ = writeln!(asked, "{}", source.trim());
    let _ = writeln!(asked, "----------------------------------------");
    let _ = writeln!(asked);
    let _ = writeln!(asked, "{}", asked_of(task, line, &handed.line, other));
    let _ = writeln!(asked);
    let _ = writeln!(
        asked,
        "When you have a change to offer, print `stemma: proposal` on its own, \
         then the complete proposed line.json, then `stemma: end proposal`. \
         If Lean changes, then print `stemma: lean`, the complete `Main.lean` module, \
         and `stemma: end lean`. A group-library change is instead printed between \
         `stemma: library` and `stemma: end library` as JSON with a `base` commit, a `files` \
         array of path, before and after, and a `readings` object mapping every central block \
         you mark to your plain-language reading of its Lean-printed statement. After it print \
         `stemma: model MODEL-ID` with the concrete model you used, \
         and finally `stemma: produced`. If you found nothing, end with `{NOTHING}`. \
         Finding nothing is an ordinary answer and a useful one."
    );

    asked
}

/// How a line is written, for an agent that has never seen one.
const FORMAT: &str = "The line is a JSON document, `line.json`, and it is the whole of the \
line: a Typst document is built from it. It has `format` (always 1), `title`, an optional \
`description`, and `blocks`, an ordered list. Every block has an `id` of the form \
`blk:` followed by a ULID, which never changes; a new block needs a new, unique one. \
Each block has a `kind`:\n\
\n\
- `prose`: `content` only. Intuition, intentions, how one idea leads to the next.\n\
- `definition` and `theorem`: `environment` (what it is called on the page: Definition, \
Theorem, Lemma, Proposition, Corollary…), optional `siglum` (`def:name` for a definition, \
`thm:`, `lem:`, `prop:` or `cor:` followed by lowercase letters, digits and hyphens for a \
theorem), optional `title`, `central`, `proof` (whether it carries a proof), `uses` (the \
`id`s of the statements it leans on) and `content`.\n\
- `proof`: `of` (the `id` of the statement it proves) and `content`. A statement with \
`\"proof\": true` has exactly one proof block, possibly empty, anywhere in the line.\n\
\n\
Every `content` is Typst markup: `$x^2$` for mathematics, `@siglum` to cite a statement. \
Brackets in it must be balanced or escaped as `\\[` and `\\]`.";

/// The one thing this job asks for.
fn asked_of(task: &Task, line: &Line, line_name: &str, other: Option<&str>) -> String {
    match task {
        Task::Fill { after } => {
            let between = line
                .boxes()
                .into_iter()
                .find(|black_box| &black_box.after == after);

            match between {
                Some(black_box) => format!("Fill the black box {}.", bounded(line, &black_box)),
                None => "Fill the black box that box refers to, which is no longer there. \
                         Say so rather than guessing."
                    .to_owned(),
            }
        }
        Task::Refute { block } => format!(
            "Look for a counterexample to `{}`. If you do not find one, \
             that settles nothing and you must not say it does: say only that you \
             did not find one.",
            named(line, block)
        ),
        Task::Formalize { block } => format!(
            "Formalize `{}` in Lean. Keep its prose fixed. Mark every declaration that backs it \
             with `@[stemma \"{line_name}/{block}\"]`. If Lean reaches this central result by a \
             different route, revise only non-central blocks so the narrative honestly follows \
             that route, and explain the realignment in your answer.",
            named(line, block)
        ),
        Task::FollowRoute { block } => format!(
            "Your proposed route to `{}` was declined. Keep the author's prose route and every \
             central statement fixed, and try to formalize that route in Lean. Mark the backing \
             declaration with `@[stemma \"{line_name}/{block}\"]`. If the route cannot be made \
             formal, explain exactly where it fails and finish with `stemma: nothing`; do not \
             silently substitute another route.",
            named(line, block)
        ),
        Task::Verify { after } => {
            let between = line
                .boxes()
                .into_iter()
                .find(|black_box| &black_box.after == after);
            match between {
                Some(black_box) => format!(
                    "Write a Lean argument verifying every statement in the black box {}. \
                     Mark each declaration with `@[stemma \"{line_name}/BLOCK-ID\"]`, using the \
                     stable block identifier it establishes.",
                    bounded(line, &black_box),
                ),
                None => "The black box to verify is no longer there. Say so rather than guessing."
                    .to_owned(),
            }
        }
        Task::Promote { block } => format!(
            "Promote `{}` out of its black box by setting `\"central\": true`. \
             Do not change its claim. Explain why it deserves a stable name; promotion freezes \
             it, splits the surrounding box, and reserves a human correspondence audit.",
            named(line, block)
        ),
        Task::Prove { block } => format!(
            "Write the proof of `{}` in its proof block, or finish the one there. \
             Do not change the statement. Add the lemmas the proof needs as new blocks \
             before it, each with its own proof. If you find no proof, say so rather than \
             offering an incomplete one as complete.",
            named(line, block)
        ),
        Task::Rewrite { block } => format!(
            "Rewrite `{}` so that it is correct and precise, keeping what it means. \
             Keep its block identifier. Adjust what uses it only where the new statement \
             requires it, and explain every change of meaning.",
            named(line, block)
        ),
        Task::Develop { block } => format!(
            "The prose block `{block}` describes mathematics informally. Develop it: replace \
             or follow it with the definitions, statements and proofs it describes, so that \
             it bridges what comes before it and what comes after. Keep the intuition as \
             prose where it helps a reader."
        ),
        Task::Explore => "Say where this line could go next: what is worth stating, \
                          and why it follows from what is already here."
            .to_owned(),
        Task::Reconcile => format!(
            "Two of this person's machines wrote this line, and the versions have parted. \
             Merge them into one line that keeps everything either version \
             established and chooses, for every block they write differently, the version \
             that is right. Central statements stay exactly as they are in both. You are \
             offering a proposal, not settling it: a person decides.\n\
             \n\
             The other version:\n\
             ----------------------------------------\n\
             {}\n\
             ----------------------------------------",
            other.unwrap_or("it is missing").trim()
        ),
        Task::Build => "Nothing: a build takes no agent.".to_owned(),
    }
}

/// A box, said by its two ends.
fn bounded(line: &Line, black_box: &BlackBox) -> String {
    let boundary = |block: &Option<BlockId>| match block {
        Some(block) => format!("`{}`", named(line, block)),
        None => "the start of the line".to_owned(),
    };

    match (&black_box.after, &black_box.before) {
        (after, Some(before)) => {
            format!("between {} and `{}`", boundary(after), named(line, before))
        }
        // The end of a line is open, and that is the ordinary state of
        // work in progress rather than something missing.
        (after, None) => format!("that begins after {} and is still open", boundary(after)),
    }
}

fn named(line: &Line, block: &BlockId) -> String {
    line.node_at(block)
        .map(|node| node.siglum.to_string())
        .unwrap_or_else(|| block.to_string())
}

/// Where the raw output of a job is, for the person whose machine it is.
pub fn output_of(home: &Home, job: &str) -> PathBuf {
    agents::output_path(home, job)
}

/// Whether the file holding a job's output is still there.
pub fn output_exists(home: &Home, job: &str) -> bool {
    Path::new(&output_of(home, job)).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formal_work_preserves_agent_failure_and_nothing_found_outcomes() {
        let task = Task::Formalize {
            block: line().node(&"thm:first".parse().unwrap()).unwrap().block,
        };
        assert!(!needs_lean_check(
            &task,
            &Outcome::out_of_reach("agent unavailable")
        ));
        assert!(!needs_lean_check(
            &task,
            &Outcome::NothingFound {
                said: "no proof".to_owned()
            }
        ));
        assert!(needs_lean_check(
            &task,
            &Outcome::Produced {
                said: "proof offered".to_owned()
            }
        ));
    }
    use crate::agents::Finished;

    use stemma_core::document::{Block, Document};

    fn source() -> String {
        let statement = |named: &str, environment: &str, content: &str| {
            Block::theorem(environment, Some(named.parse().unwrap()), content).carrying_proof(false)
        };
        Document::new("Quotient groups", None)
            .with(
                statement("thm:first", "Theorem", "y")
                    .central(true)
                    .titled("First"),
            )
            .with(statement("lem:kernel", "Lemma", "z"))
            .with(
                statement("thm:second", "Theorem", "v")
                    .central(true)
                    .titled("Second"),
            )
            .to_text()
    }

    fn handed(task: &str) -> Handed {
        Handed {
            job: "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T".to_owned(),
            group: "algebra".to_owned(),
            side: "shared".to_owned(),
            line: "quotient-groups".to_owned(),
            line_id: Some("lin:01K5YQ4TQ2VZ8H3M9N0P1R2S3T".to_owned()),
            revision: 3,
            against: None,
            task: task.to_owned(),
            agent: "claude".to_owned(),
            launched_by: "usr:ana".to_owned(),
        }
    }

    fn line() -> Line {
        Line::read(&source()).unwrap()
    }

    fn block(line: &Line, named: &str) -> BlockId {
        line.node(&named.parse().unwrap()).unwrap().block
    }

    #[test]
    fn a_box_is_asked_for_by_both_its_ends() {
        let line = line();
        let task = Task::Fill {
            after: Some(block(&line, "thm:first")),
        };
        let asked = asked_of(&task, &line, "quotient-groups", None);

        assert!(
            asked.contains("between `thm:first` and `thm:second`"),
            "{asked}"
        );

        let first: Task = "fill".parse().unwrap();
        assert!(
            asked_of(&first, &line, "quotient-groups", None)
                .contains("between the start of the line")
        );
    }

    #[test]
    fn the_prompt_says_what_an_agent_may_not_touch() {
        let line = line();
        let task = Task::Fill {
            after: Some(block(&line, "thm:first")),
        };
        let asked = prompt(&handed(&task.to_string()), &task, &line, &source(), None);

        assert!(asked.contains("Do not change its text"));
        assert!(asked.contains("\"kind\""), "the format is described");
        assert!(asked.contains("thm:second"));
        assert!(asked.contains("stemma: nothing"));
    }

    #[test]
    fn a_formal_library_offer_marks_the_block_it_establishes() {
        let line = line();
        let block = block(&line, "thm:first");
        let task = Task::Formalize { block };
        let offered = LibraryChange {
            base: "0123456789012345678901234567890123456789".to_owned(),
            files: vec![stemma_core::proposal::LibraryFileChange {
                path: "Algebra/Quotients.lean".to_owned(),
                before: None,
                after: Some(format!(
                    "@[stemma \"quotient-groups/{block}\"] theorem first : True := by trivial\n"
                )),
            }],
            readings: std::collections::BTreeMap::from([(
                block,
                "The first marked statement holds.".to_owned(),
            )]),
        };

        assert!(check_library_offer(&offered, &task, &line, "quotient-groups").is_ok());
        assert!(check_library_offer(&offered, &task, &line, "another-line").is_err());
    }

    #[test]
    fn an_agent_looking_for_a_counterexample_is_told_what_not_finding_one_means() {
        let line = line();
        let task = Task::Refute {
            block: block(&line, "thm:first"),
        };
        let asked = asked_of(&task, &line, "quotient-groups", None);

        assert!(asked.contains("settles nothing"));
    }

    #[test]
    fn finding_no_counterexample_is_never_reported_as_the_statement_holding() {
        let line = line();
        let task = Task::Refute {
            block: block(&line, "thm:first"),
        };
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
        let written = source();
        let output = format!(
            "notes\nstemma: proposal\n{written}stemma: end proposal\nstemma: model claude-opus-5\nstemma: produced\n"
        );
        let offered = proposal("claude", &output).unwrap();
        assert_eq!(offered.source, written.trim_end());
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

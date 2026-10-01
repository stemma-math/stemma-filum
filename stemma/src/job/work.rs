//! Doing one job on this machine.
//!
//! The work is read from the instance at the revision the job names,
//! never out of the files here: those belong to whoever is at the
//! keyboard, they may be half-written, and nothing here touches them.
//!
//! What comes back from the agent stays here. The instance is told the
//! type of what happened and one line, which is what the group reads.

use std::path::{Path, PathBuf};

use stemma_core::id::BlockId;
use stemma_core::job::{Outcome, Task};
use stemma_core::line::Line;
use stemma_core::proposal::{FormalReview, LibraryChange, LibraryFileChange, Offer, Operation};
use stemma_core::repository::{self, Side};

use crate::api::Handed;
use crate::config::Config;
use crate::home::Home;
use crate::job::agents::{self, Ran};
use crate::mirror::{Instance, Mirror, Remote, Which};

/// Which revision of the prompt this build sends.
///
/// It is recorded with every run, because "an agent said so" is not an
/// attribution: the vendor, the model and the words it was given are.
pub const PROMPT_REVISION: &str = crate::job::workspace::INSTRUCTION_REVISION;

/// Carries out one job and says what came of it.
///
/// Nothing in here fails: every way this can go wrong is one of the
/// outcomes, because a job that ends in nothing at all leaves the person
/// who launched it watching a queue.
pub fn carry_out(home: &Home, config: &Config, instance: &Instance, handed: &Handed) -> Completed {
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    carry_out_controlled(home, config, instance, handed, &cancelled)
}

/// Carries out one job while observing a cancellation flag maintained by the
/// daemon's heartbeat loop.
pub fn carry_out_controlled(
    home: &Home,
    config: &Config,
    instance: &Instance,
    handed: &Handed,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Completed {
    let task: Task = match handed.task.parse() {
        Ok(task) => task,
        Err(problem) => return Completed::only(Outcome::out_of_reach(problem.to_string())),
    };

    if task == Task::Build {
        return build(home, instance, handed, cancelled);
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
                first_fault(&faults, "it is not a line")
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

    let repository = repository_of(home, handed);
    let workspace = match crate::job::workspace::prepare(
        home,
        handed,
        &task,
        &source,
        repository.as_deref(),
        handed.grants.clone(),
    ) {
        Ok(workspace) => workspace,
        Err(error) => return Completed::only(Outcome::out_of_reach(format!("{error:#}"))),
    };
    if let Some(other) = &other {
        let _ = std::fs::write(workspace.root.join("other-line.json"), other);
    }
    let limit = std::time::Duration::from_secs(config.jobs.timeout_minutes.max(1) * 60);
    let ran = match agents::run_in_workspace(
        config,
        &handed.agent,
        "Read AGENTS.md and TASK.md. Work in this directory and answer only through `stemma-job`.",
        &workspace.root,
        &workspace.api_bin,
        &agents::output_path(home, &handed.job),
        limit,
        cancelled,
    ) {
        Ok(ran) => ran,
        Err(error) => {
            return Completed::at(Outcome::out_of_reach(format!("{error:#}")), workspace.root);
        }
    };
    let state = crate::job::answer::read(&workspace.state).ok();
    let was_cancelled = matches!(&ran, Ran::Finished(finished) if finished.cancelled);
    let failure = stemma_core::id::Slug::parse(&handed.agent)
        .ok()
        .and_then(|agent| agents::trouble(&agent, &ran));
    let outcome = if was_cancelled {
        Outcome::Broke {
            code: "cancelled".to_owned(),
            said: "the job was cancelled by its launcher".to_owned(),
        }
    } else if let Some(failure) = failure {
        Outcome::from_failure(&failure)
    } else if let Some(state) = &state
        && state.done
        && let Some(reason) = &state.nothing
    {
        Outcome::NothingFound {
            said: reason.clone(),
        }
    } else if state
        .as_ref()
        .is_some_and(|state| state.done && !state.operations.is_empty())
    {
        Outcome::Produced {
            said: state
                .as_ref()
                .map(|state| state.explanation.clone())
                .unwrap_or_default(),
        }
    } else {
        Outcome::Broke {
            code: "answer_not_submitted".to_owned(),
            said: "the agent stopped without submitting through `stemma-job done`".to_owned(),
        }
    };
    let mut proposal = match (&outcome, state) {
        (Outcome::Produced { .. }, Some(state)) => {
            let source = match stemma_core::proposal::apply_line_operations(
                &source,
                &workspace.grants,
                &state.operations,
            ) {
                Ok(source) => source,
                Err(error) => {
                    return Completed::at(
                        Outcome::Broke {
                            code: "invalid_operations".to_owned(),
                            said: error.to_string(),
                        },
                        workspace.root,
                    );
                }
            };
            if state
                .operations
                .iter()
                .any(|operation| matches!(operation, Operation::WriteLibraryFile { .. }))
                && !workspace.grants.library
            {
                return Completed::at(
                    Outcome::Broke {
                        code: "operation_out_of_scope".to_owned(),
                        said: "the answer contains a library operation this job did not grant"
                            .to_owned(),
                    },
                    workspace.root,
                );
            }
            let library_change = library_change(
                repository.as_deref(),
                &state.operations,
                state.readings.clone(),
            );
            Some(Offer {
                source,
                lean_source: None,
                library_change,
                formal_review: None,
                vendor: stemma_core::machine::known(&handed.agent)
                    .map(|agent| agent.vendor)
                    .unwrap_or("unknown")
                    .to_owned(),
                model: agents::DEFAULT_MODEL.to_owned(),
                prompt_revision: PROMPT_REVISION.to_owned(),
                operations: state.operations,
                explanation: state.explanation,
                effort: None,
            })
        }
        _ => None,
    };

    let reviews_library = proposal
        .as_ref()
        .is_some_and(|offer| offer.library_change.is_some());
    if reviews_library || needs_lean_check(&task, &outcome) {
        let checked = match proposal.as_mut() {
            Some(offer) => check_formal(home, handed, &task, &line, repository.as_deref(), offer),
            None => Err("the agent offered no formal change".to_owned()),
        };
        if let Err(said) = checked {
            return Completed::at(
                Outcome::Broke {
                    code: "lean_rejected".to_owned(),
                    said,
                },
                workspace.root,
            );
        }
    }

    let outcome = match (&outcome, &proposal) {
        (Outcome::Produced { .. }, None) => Outcome::Broke {
            code: "malformed_proposal".to_owned(),
            said: "the agent reported an answer without reviewable source".to_owned(),
        },
        (Outcome::Produced { .. }, Some(offer)) => match Line::read(&offer.source) {
            Ok(_) => outcome,
            Err(faults) => {
                return Completed::at(
                    Outcome::Broke {
                        code: "invalid_line".to_owned(),
                        said: format!(
                            "the agent offered something that is not a valid line: {}",
                            first_fault(&faults, "it is not a line")
                        ),
                    },
                    workspace.root,
                );
            }
        },
        _ => outcome,
    };
    Completed {
        outcome,
        proposal,
        workspace: Some(workspace.root),
    }
}

/// Checks the formal side of an offer before it may travel: a library
/// change is built before and after in Lean and its evidence attached to
/// the offer; a complete module is compiled and must declare what the
/// task asked it to establish.
fn check_formal(
    home: &Home,
    handed: &Handed,
    task: &Task,
    line: &Line,
    repository: Option<&Path>,
    offer: &mut Offer,
) -> Result<(), String> {
    let clone = repository
        .ok_or_else(|| "this machine does not have the line's repository yet".to_owned())?;
    let pins = || -> Result<_, String> {
        let manifest = crate::repository::read_manifest(clone).map_err(|e| e.to_string())?;
        Ok(crate::repository::group_pins(clone, &manifest))
    };

    if let Some(change) = &offer.library_change {
        check_library_offer(change, task, line, &handed.line)?;
        let pins = pins()?;
        let name =
            stemma_core::line::Name::parse(&handed.line).map_err(|error| error.to_string())?;
        let targets = formal_targets(task, line)?;
        let review = crate::lean::review_change(
            clone,
            &name,
            &pins,
            change,
            &offer.source,
            &home.cache_dir().join("lean").join(&handed.job),
            &targets,
        )
        .map_err(|error| format!("{error:#}"))?;
        let proposed_line = Line::read(&offer.source)
            .map_err(|faults| first_fault(&faults, "the proposed line is unreadable"))?;
        check_review(&review, &targets, &proposed_line)?;
        offer.formal_review = Some(Box::new(review));
        return Ok(());
    }

    let lean_source = offer
        .lean_source
        .as_deref()
        .ok_or_else(|| "the agent offered no complete Lean module".to_owned())?;
    crate::lean::check_offered(
        clone,
        &pins()?,
        lean_source,
        &home
            .cache_dir()
            .join("lean")
            .join(format!("{}.lean", handed.job)),
    )?;
    let declared = crate::lean::declared_statements(lean_source);
    let statements = match task {
        Task::Formalize { block } | Task::FollowRoute { block } => vec![
            line.node_at(block)
                .ok_or_else(|| "the statement block is no longer there".to_owned())?,
        ],
        // A box holds prose too; only its statements are declared.
        _ => formal_targets(task, line)?
            .iter()
            .filter_map(|block| line.node_at(block))
            .collect(),
    };
    let missing: Vec<String> = statements
        .into_iter()
        .filter(|statement| !declared.contains(&statement.siglum))
        .map(|statement| statement.block.to_string())
        .collect();

    match (task, missing.is_empty()) {
        (_, true) => Ok(()),
        (Task::Formalize { block } | Task::FollowRoute { block }, false) => Err(format!(
            "the compiled module does not declare that it verifies `{block}`"
        )),
        (_, false) => Err(format!(
            "the compiled module does not verify {}",
            missing.join(", ")
        )),
    }
}

/// The first thing wrong with something that is not a line, in words.
fn first_fault(faults: &[stemma_core::line::Fault], otherwise: &str) -> String {
    faults
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| otherwise.to_owned())
}

fn library_change(
    repository: Option<&Path>,
    operations: &[Operation],
    readings: std::collections::BTreeMap<BlockId, String>,
) -> Option<LibraryChange> {
    let files: Vec<_> = operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::WriteLibraryFile {
                path,
                before,
                after,
            } => Some(LibraryFileChange {
                path: path.clone(),
                before: before.clone(),
                after: after.clone(),
            }),
            _ => None,
        })
        .collect();
    if files.is_empty() {
        return None;
    }
    let base = repository
        .and_then(|repository| {
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repository)
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        })
        .unwrap_or_default();
    Some(LibraryChange {
        base,
        files,
        readings,
    })
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
    let missing: Vec<_> = formal_targets(task, line)?
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
    /// The workspace deliberately left for the machine's owner.
    pub workspace: Option<PathBuf>,
}

impl Completed {
    fn only(outcome: Outcome) -> Self {
        Self {
            outcome,
            proposal: None,
            workspace: None,
        }
    }

    fn at(outcome: Outcome, workspace: PathBuf) -> Self {
        Self {
            outcome,
            proposal: None,
            workspace: Some(workspace),
        }
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
fn build(
    home: &Home,
    instance: &Instance,
    handed: &Handed,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Completed {
    // Each step of a build runs to its end, so a cancellation is heard
    // between them, and nothing is published after one.
    let is_cancelled = || cancelled.load(std::sync::atomic::Ordering::Relaxed);
    let built = (|| -> anyhow::Result<Option<crate::build::Published>> {
        let mirror = Mirror::at(home)?;
        let round = mirror.step(instance, true, &Which::All);
        if let Some(stopped) = round.stopped {
            anyhow::bail!("{stopped}");
        }
        if is_cancelled() {
            return Ok(None);
        }

        let side: Side = handed.side.parse()?;
        let built = crate::build::read(&mirror, &handed.group, side)?;
        if is_cancelled() {
            return Ok(None);
        }
        let credentials = crate::account::credentials::Credentials::read(&home.credentials_file())?;
        let published = crate::build::publish(&built, credentials.as_ref());
        if published == crate::build::Published::Kept {
            mirror.built(&built.nodes)?;
        }

        Ok(Some(published))
    })();

    Completed::only(match built {
        Ok(None) => Outcome::Broke {
            code: "cancelled".to_owned(),
            said: "the job was cancelled by its launcher".to_owned(),
        },
        Ok(Some(crate::build::Published::Kept)) => Outcome::Produced {
            said: "its pages were built and published".to_owned(),
        },
        Ok(Some(other)) => Outcome::out_of_reach(other.to_string()),
        Err(error) => Outcome::out_of_reach(format!("{error:#}")),
    })
}

/// Where the raw output of a job is, for the person whose machine it is.
pub fn output_of(home: &Home, job: &str) -> PathBuf {
    agents::output_path(home, job)
}

/// Whether the file holding a job's output is still there.
pub fn output_exists(home: &Home, job: &str) -> bool {
    output_of(home, job).is_file()
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

    fn line() -> Line {
        Line::read(&source()).unwrap()
    }

    fn block(line: &Line, named: &str) -> BlockId {
        line.node(&named.parse().unwrap()).unwrap().block
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
}

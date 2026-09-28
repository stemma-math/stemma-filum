//! Construction of the inspectable, per-job directory given to an agent.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use stemma_core::document::Document;
use stemma_core::job::Task;
use stemma_core::line::Line;
use stemma_core::proposal::{Grants, LineGrant};

use crate::api::Handed;
use crate::job_api::{self, State};

/// The versioned instruction contract shipped by this build.
pub const INSTRUCTION_REVISION: &str = "workspace-api/1";

/// A prepared job directory and its private API state.
pub struct Workspace {
    /// Root shown to the machine owner and used as the agent's directory.
    pub root: PathBuf,
    /// Private state updated only through `stemma-job`.
    pub state: PathBuf,
    /// Directory prepended to the agent's `PATH`.
    pub api_bin: PathBuf,
    /// Immutable copy used by the daemon to validate the final answer again.
    pub grants: Grants,
}

/// Creates a workspace in the operating system's temporary directory and
/// deliberately leaves it there for its owner to inspect.
pub fn prepare(
    handed: &Handed,
    task: &Task,
    base_source: &str,
    repository: Option<&Path>,
    custom_grants: Option<Grants>,
) -> Result<Workspace> {
    let root = std::env::temp_dir()
        .join("stemma-jobs")
        .join(handed.job.replace(':', "-"));
    if root.exists() {
        bail!("the job workspace already exists at {}", root.display());
    }
    std::fs::create_dir_all(&root)?;
    if let Some(repository) = repository {
        copy_tree(repository, &root)?;
    }
    std::fs::create_dir_all(root.join("scratch"))?;
    std::fs::create_dir_all(root.join(".stemma"))?;
    let group_conventions = std::fs::read_to_string(root.join("AGENTS.md")).ok();
    let mut instructions = include_str!("../instructions/AGENTS.md").to_owned();
    if let Some(conventions) = group_conventions.filter(|text| !text.trim().is_empty()) {
        instructions.push_str("\n## Group conventions\n\n");
        instructions.push_str(conventions.trim());
        instructions.push('\n');
    }
    std::fs::write(root.join("AGENTS.md"), instructions)?;

    let line_at = root
        .join(stemma_core::repository::LINES)
        .join(&handed.line)
        .join(stemma_core::repository::ENTRY);
    if let Some(parent) = line_at.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&line_at, base_source)?;
    std::fs::write(root.join("line.json"), base_source)?;

    let grants = custom_grants.unwrap_or_else(|| grants_for(task, base_source));
    let mut task_file = String::new();
    let _ = writeln!(task_file, "{}", instruction_for(task));
    let _ = writeln!(task_file, "\n## Job\n");
    let _ = writeln!(task_file, "- Group: `{}`", handed.group);
    let _ = writeln!(task_file, "- Line: `{}`", handed.line);
    let _ = writeln!(task_file, "- Revision: `{}`", handed.revision);
    let _ = writeln!(task_file, "- Task: `{task}`");
    if let Some(request) = handed.request.as_deref() {
        let _ = writeln!(task_file, "\n## Request\n\n{request}");
    }
    let _ = writeln!(task_file, "\n## Grants\n");
    let _ = writeln!(
        task_file,
        "```json\n{}\n```",
        serde_json::to_string_pretty(&grants)?
    );
    std::fs::write(root.join("TASK.md"), task_file)?;

    let api_bin = root.join(".stemma/bin");
    std::fs::create_dir_all(&api_bin)?;
    let command = crate::program::command_line()?;
    let linked = api_bin.join(if cfg!(windows) {
        "stemma-job.exe"
    } else {
        "stemma-job"
    });
    link_command(&command, &linked)?;

    let state = root.join(".stemma/job.json");
    job_api::write(
        &state,
        &State {
            line: handed.line.clone(),
            base_source: base_source.to_owned(),
            operations: Vec::new(),
            grants: grants.clone(),
            explanation: String::new(),
            nothing: None,
            done: false,
            workspace: root.clone(),
            library_before: Default::default(),
            readings: Default::default(),
        },
    )?;

    Ok(Workspace {
        root,
        state,
        api_bin,
        grants,
    })
}

fn instruction_for(task: &Task) -> &'static str {
    match task {
        Task::Prove { .. } => include_str!("../instructions/tasks/prove.md"),
        Task::Formalize { .. } => include_str!("../instructions/tasks/formalize.md"),
        Task::Verify { .. } => include_str!("../instructions/tasks/back.md"),
        Task::FollowRoute { .. } => include_str!("../instructions/tasks/realign.md"),
        Task::Refute { .. } => include_str!("../instructions/tasks/refute.md"),
        Task::Rewrite { .. } => include_str!("../instructions/tasks/rewrite.md"),
        Task::Develop { .. } => include_str!("../instructions/tasks/develop.md"),
        Task::Explore => include_str!("../instructions/tasks/explore.md"),
        Task::Reconcile => include_str!("../instructions/tasks/reconcile.md"),
        Task::Custom => include_str!("../instructions/tasks/custom.md"),
        Task::Fill { .. } | Task::Promote { .. } => {
            include_str!("../instructions/tasks/develop.md")
        }
        Task::Build => "# Build\n\nNo agent runs for this task.",
    }
}

fn grants_for(task: &Task, source: &str) -> Grants {
    let Ok(document) = Document::parse(source) else {
        return Grants::default();
    };
    let line = Line::of(&document).ok();
    let ordinary: Vec<_> = document
        .blocks
        .iter()
        .filter(|block| !block.statement().is_some_and(|statement| statement.central))
        .map(|block| *block.id())
        .collect();
    let mut grants = Grants::default();
    let edit_and_remove = |grants: &mut Grants, blocks: &[stemma_core::id::BlockId]| {
        for block in blocks {
            grants.line.push(LineGrant::Edit { block: *block });
            grants.line.push(LineGrant::Remove { block: *block });
        }
    };

    match task {
        Task::Prove { block } => {
            for proof in document.proofs_of(block) {
                grants.line.push(LineGrant::Edit { block: proof.id });
            }
            grants.line.push(LineGrant::Add {
                after: Some(*block),
            });
        }
        Task::Formalize { .. } | Task::FollowRoute { .. } => {
            edit_and_remove(&mut grants, &ordinary);
            grants
                .line
                .extend(document.blocks.iter().map(|block| LineGrant::Add {
                    after: Some(*block.id()),
                }));
            grants.library = true;
        }
        Task::Verify { after } | Task::Fill { after } => {
            let inside = line
                .as_ref()
                .and_then(|line| line.boxes().into_iter().find(|box_| &box_.after == after))
                .map(|box_| box_.interior)
                .unwrap_or_default();
            edit_and_remove(&mut grants, &inside);
            grants.library = matches!(task, Task::Verify { .. });
            grants.line.push(LineGrant::Add { after: *after });
        }
        Task::Refute { block } => grants.line.push(LineGrant::Add {
            after: Some(*block),
        }),
        Task::Promote { block } | Task::Rewrite { block } => {
            grants.line.push(LineGrant::Edit { block: *block });
        }
        Task::Develop { block } => {
            grants.line.push(LineGrant::Edit { block: *block });
            grants.line.push(LineGrant::Add {
                after: Some(*block),
            });
        }
        Task::Explore => {
            grants.add_only = true;
            grants.line.push(LineGrant::Add { after: None });
            grants
                .line
                .extend(document.blocks.iter().map(|block| LineGrant::Add {
                    after: Some(*block.id()),
                }));
        }
        Task::Reconcile => {
            edit_and_remove(&mut grants, &ordinary);
            grants.line.push(LineGrant::Add { after: None });
            grants
                .line
                .extend(document.blocks.iter().map(|block| LineGrant::Add {
                    after: Some(*block.id()),
                }));
        }
        Task::Custom | Task::Build => {}
    }
    grants
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(from).with_context(|| format!("cannot read {}", from.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" || name == "target" {
            continue;
        }
        let source = entry.path();
        let target = to.join(name);
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_tree(&source, &target)?;
        } else if entry.file_type()?.is_file() {
            std::fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn link_command(command: &Path, linked: &Path) -> Result<()> {
    std::os::unix::fs::symlink(command, linked)?;
    Ok(())
}

#[cfg(not(unix))]
fn link_command(command: &Path, linked: &Path) -> Result<()> {
    std::fs::copy(command, linked)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use stemma_core::document::Block;

    #[test]
    fn explore_can_only_add_after_known_positions() {
        let mut document = Document::new("A line", None);
        let block = document.push(Block::prose("Begin"));
        let grants = grants_for(&Task::Explore, &document.to_text());
        assert!(grants.add_only);
        assert!(grants.line.contains(&LineGrant::Add { after: Some(block) }));
        assert!(!grants.library);
    }
}

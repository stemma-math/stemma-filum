//! Construction of the inspectable, per-job directory given to an agent.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use stemma_core::document::Document;
use stemma_core::job::Task;
use stemma_core::line::{Line, Name};
use stemma_core::proposal::{Grants, LineGrant};
use stemma_core::repository;

use crate::api::Handed;
use crate::home::Home;
use crate::job::answer::{self, State};

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

/// Where the workspaces of this machine's jobs are kept: in the client's
/// state directory, which only its owner can enter.
pub fn jobs_dir(home: &Home) -> PathBuf {
    home.state_dir().join("jobs")
}

/// Where the workspace of a job is: one directory per job, in
/// [`jobs_dir`].
pub fn root_of(home: &Home, job: &str) -> PathBuf {
    jobs_dir(home).join(crate::storage::file_name(job))
}

/// Creates a workspace for a job and deliberately leaves it there for its
/// owner to inspect.
pub fn prepare(
    home: &Home,
    handed: &Handed,
    task: &Task,
    base_source: &str,
    repository: Option<&Path>,
    custom_grants: Option<Grants>,
) -> Result<Workspace> {
    // The line's name becomes a directory, so it has to be one.
    let line = Name::parse(&handed.line)
        .map_err(|_| anyhow!("the job names a line that cannot be a directory here"))?;
    let jobs = jobs_dir(home);
    std::fs::create_dir_all(&jobs).with_context(|| format!("cannot create {}", jobs.display()))?;
    // A workspace holds the group's repository and what agents write, so
    // nobody else on the machine may read it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&jobs, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("cannot restrict {}", jobs.display()))?;
    }
    let root = root_of(home, &handed.job);
    match std::fs::create_dir(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!("the job workspace already exists at {}", root.display());
        }
        created => created.with_context(|| format!("cannot create {}", root.display()))?,
    }

    if let Some(repository) = repository {
        copy_tree(repository, &root)?;
    }
    create(&root.join("scratch"))?;
    create(&root.join(".stemma"))?;
    let group_conventions = std::fs::read_to_string(root.join("AGENTS.md")).ok();
    let mut instructions = include_str!("../../instructions/AGENTS.md").to_owned();
    if let Some(conventions) = group_conventions.filter(|text| !text.trim().is_empty()) {
        instructions.push_str("\n## Group conventions\n\n");
        instructions.push_str(conventions.trim());
        instructions.push('\n');
    }
    write(&root.join("AGENTS.md"), &instructions)?;

    let line_at = repository::line_entry(&root, &line);
    if let Some(parent) = line_at.parent() {
        create(parent)?;
    }
    write(&line_at, base_source)?;
    write(&root.join("line.json"), base_source)?;

    let grants = custom_grants.unwrap_or_else(|| grants_for(task, base_source));
    write(&root.join("TASK.md"), &task_file(handed, task, &grants)?)?;

    let api_bin = root.join(".stemma/bin");
    create(&api_bin)?;
    let command = crate::machine::program::command_line()?;
    let linked = api_bin.join(if cfg!(windows) {
        "stemma-job.exe"
    } else {
        "stemma-job"
    });
    link_command(&command, &linked)
        .with_context(|| format!("cannot put `stemma-job` at {}", linked.display()))?;

    let state = root.join(".stemma/job.json");
    answer::write(
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

/// What the agent is asked to do, and with what grants.
fn task_file(handed: &Handed, task: &Task, grants: &Grants) -> Result<String> {
    let mut text = String::new();
    let _ = writeln!(text, "{}", instruction_for(task));
    let _ = writeln!(text, "\n## Job\n");
    let _ = writeln!(text, "- Group: `{}`", handed.group);
    let _ = writeln!(text, "- Line: `{}`", handed.line);
    let _ = writeln!(text, "- Revision: `{}`", handed.revision);
    let _ = writeln!(text, "- Task: `{task}`");
    if let Some(request) = handed.request.as_deref() {
        let _ = writeln!(text, "\n## Request\n\n{request}");
    }
    let _ = writeln!(text, "\n## Grants\n");
    let _ = writeln!(
        text,
        "```json\n{}\n```",
        serde_json::to_string_pretty(grants)?
    );

    Ok(text)
}

/// Makes a directory in the workspace, saying which when it cannot.
fn create(directory: &Path) -> Result<()> {
    std::fs::create_dir_all(directory)
        .with_context(|| format!("cannot create {}", directory.display()))
}

/// Writes a file in the workspace, saying which when it cannot.
fn write(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents).with_context(|| format!("cannot write {}", path.display()))
}

fn instruction_for(task: &Task) -> &'static str {
    match task {
        Task::Prove { .. } => include_str!("../../instructions/tasks/prove.md"),
        Task::Formalize { .. } => include_str!("../../instructions/tasks/formalize.md"),
        Task::Verify { .. } => include_str!("../../instructions/tasks/back.md"),
        Task::FollowRoute { .. } => include_str!("../../instructions/tasks/realign.md"),
        Task::Refute { .. } => include_str!("../../instructions/tasks/refute.md"),
        Task::Rewrite { .. } => include_str!("../../instructions/tasks/rewrite.md"),
        Task::Develop { .. } => include_str!("../../instructions/tasks/develop.md"),
        Task::Explore => include_str!("../../instructions/tasks/explore.md"),
        Task::Reconcile => include_str!("../../instructions/tasks/reconcile.md"),
        Task::Custom => include_str!("../../instructions/tasks/custom.md"),
        Task::Fill { .. } | Task::Promote { .. } => {
            include_str!("../../instructions/tasks/develop.md")
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
        let entry = entry.with_context(|| format!("cannot read {}", from.display()))?;
        let name = entry.file_name();
        if name == ".git" || name == "target" {
            continue;
        }
        let source = entry.path();
        let target = to.join(name);
        let kind = entry
            .file_type()
            .with_context(|| format!("cannot read {}", source.display()))?;
        if kind.is_dir() {
            create(&target)?;
            copy_tree(&source, &target)?;
        } else if kind.is_file() {
            std::fs::copy(&source, &target)
                .with_context(|| format!("cannot copy {}", source.display()))?;
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
    use crate::home::{Environment, HOME_VARIABLE};
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

    #[test]
    fn a_job_is_kept_in_a_directory_of_its_own_and_never_elsewhere() {
        let home = Home::locate(&Environment::from_pairs([(
            HOME_VARIABLE,
            std::env::temp_dir().join("stemma-job-root-test"),
        )]));
        let root = root_of(&home, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T");
        assert!(root.ends_with("jobs/job-01K5YQ4TQ2VZ8H3M9N0P1R2S3T"));
        assert!(root_of(&home, "job:../../elsewhere").starts_with(jobs_dir(&home)));
        assert!(jobs_dir(&home).starts_with(home.state_dir()));
    }

    #[test]
    fn a_job_naming_a_line_that_cannot_be_a_directory_is_refused() {
        let handed: Handed = serde_json::from_value(serde_json::json!({
            "job": "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T",
            "group": "algebra",
            "side": "shared",
            "line": "../../outside",
            "revision": 1,
            "task": "explore",
            "agent": "claude",
            "launched_by": "usr:ana",
        }))
        .unwrap();

        let home = Home::locate(&Environment::from_pairs([(
            HOME_VARIABLE,
            std::env::temp_dir().join("stemma-job-refusal-test"),
        )]));
        let error = prepare(&home, &handed, &Task::Explore, "", None, None)
            .err()
            .unwrap();

        assert!(
            error.to_string().contains("cannot be a directory"),
            "{error}"
        );
    }
}

//! Construction of the inspectable, per-job directory given to an agent.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow, bail};
use stemma_core::document::Document;
use stemma_core::job::Task;
use stemma_core::line::{Line, Name};
use stemma_core::proposal::{Grants, LineGrant};
use stemma_core::repository;

use crate::api::{Handed, Listed};
use crate::home::Home;
use crate::job::answer::{self, State};

/// The versioned instruction contract shipped by this build.
pub const INSTRUCTION_REVISION: &str = "workspace-api/2";

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

/// Creates a workspace for a job and leaves it there for its owner to
/// inspect, until [`tidy`] finds it is no longer needed. A workspace that
/// could not be finished is removed at once, so a full disk is not left
/// fuller.
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

    let prepared = fill(
        handed,
        task,
        base_source,
        repository,
        custom_grants,
        &line,
        root.clone(),
    );
    if prepared.is_err() {
        let _ = std::fs::remove_dir_all(&root);
    }
    prepared
}

/// Puts everything a job needs in its new, empty workspace.
fn fill(
    handed: &Handed,
    task: &Task,
    base_source: &str,
    repository: Option<&Path>,
    custom_grants: Option<Grants>,
    line: &Name,
    root: PathBuf,
) -> Result<Workspace> {
    let grants = custom_grants.unwrap_or_else(|| grants_for(task, base_source));
    if let Some(repository) = repository {
        if grants.library {
            crate::lean::prepare_for_job(repository)?;
        }
        copy_repository(repository, &root)?;
        if grants.library {
            prepare_private_library(repository, &root)?;
        }
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

    let line_at = repository::line_entry(&root, line);
    if let Some(parent) = line_at.parent() {
        create(parent)?;
    }
    write(&line_at, base_source)?;
    write(&root.join("line.json"), base_source)?;

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

/// Lake's private package normally refers to a sibling shared checkout.
/// Job workspaces have no such sibling, so give each one an isolated copy.
fn prepare_private_library(repository: &Path, root: &Path) -> Result<()> {
    let manifest_path = repository.join(repository::MANIFEST);
    if !manifest_path.exists() {
        return Ok(());
    }
    let manifest = crate::repository::read_manifest(repository)?;
    if manifest.side != repository::Side::Private {
        return Ok(());
    }
    let shared = repository
        .parent()
        .context("the private library has no group directory")?
        .join("shared");
    let local_shared = root.join(".lake/workspace-shared");
    create(&local_shared)?;
    copy_repository(&shared, &local_shared)?;

    relocate_private_lakefile(root)?;
    let pins = crate::repository::group_pins(repository, &manifest);
    let lean = pins
        .lean
        .as_deref()
        .context("Lean is not pinned for this group")?;
    let mathlib = pins
        .mathlib
        .as_deref()
        .context("Mathlib is not pinned for this group")?;
    crate::lean::prepare_environment(root, lean, mathlib).map_err(anyhow::Error::msg)
}

fn relocate_private_lakefile(root: &Path) -> Result<()> {
    let lakefile = root.join(repository::LAKEFILE);
    let source = std::fs::read_to_string(&lakefile)?;
    let relocated = source.replace("path = \"../shared\"", "path = \".lake/workspace-shared\"");
    if relocated == source {
        bail!("the private Lake project does not identify its shared library");
    }
    std::fs::write(&lakefile, relocated)?;
    Ok(())
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

/// Copies the group's repository into a workspace, without its history,
/// its build products or this machine's own state. Lake's packages (Mathlib
/// among them, several gigabytes) are shared with the repository rather
/// than copied: a job reads them and has no reason to change them.
fn copy_repository(from: &Path, to: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(from).with_context(|| format!("cannot read {}", from.display()))?
    {
        let entry = entry.with_context(|| format!("cannot read {}", from.display()))?;
        let name = entry.file_name();
        if name == ".git" || name == "target" || name == ".stemma" {
            continue;
        }
        let source = entry.path();
        let target = to.join(&name);
        let kind = entry
            .file_type()
            .with_context(|| format!("cannot read {}", source.display()))?;
        if name == ".lake" && kind.is_dir() {
            create(&target)?;
            copy_lake(&source, &target)?;
        } else if kind.is_dir() {
            create(&target)?;
            copy_tree(&source, &target)?;
        } else if kind.is_file() {
            std::fs::copy(&source, &target)
                .with_context(|| format!("cannot copy {}", source.display()))?;
        }
    }
    Ok(())
}

/// Copies Lake's directory, linking its packages instead of copying them.
fn copy_lake(from: &Path, to: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(from).with_context(|| format!("cannot read {}", from.display()))?
    {
        let entry = entry.with_context(|| format!("cannot read {}", from.display()))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let kind = entry
            .file_type()
            .with_context(|| format!("cannot read {}", source.display()))?;
        if entry.file_name() == "packages" && kind.is_dir() {
            share_directory(&source, &target)?;
        } else if kind.is_dir() {
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
fn share_directory(from: &Path, to: &Path) -> Result<()> {
    std::os::unix::fs::symlink(from, to).with_context(|| format!("cannot link {}", from.display()))
}

#[cfg(not(unix))]
fn share_directory(from: &Path, to: &Path) -> Result<()> {
    create(to)?;
    copy_tree(from, to)
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

/// How long a workspace is kept once its job ended without anything to
/// review, for its owner to look at what happened.
const KEPT_AFTER_ENDING: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a workspace is kept when the instance no longer lists its job.
const KEPT_UNLISTED: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How much room the workspaces of this machine's jobs take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Used {
    /// How many there are.
    pub count: usize,
    /// How many bytes they take, not counting what they share with the
    /// group's repository.
    pub bytes: u64,
}

/// What the workspaces of this machine's jobs take now.
pub fn used(home: &Home) -> Used {
    let mut used = Used::default();
    for (path, _) in workspaces(home) {
        used.count += 1;
        used.bytes += size_of(&path);
    }
    used
}

/// Whether a workspace is still of use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    /// Its job is queued or running: never removed.
    Working,
    /// Worth keeping, unless the workspaces take more than they may.
    Kept,
    /// Of no further use.
    Done,
}

/// Removes the workspaces that are no longer needed, then the oldest of the
/// rest until they fit in `limit` bytes, and says what is left.
///
/// A workspace is kept while its job is queued or running, while its
/// proposal waits for review, for a day after its job ended with nothing
/// to review, and for a week once the instance no longer lists its job.
pub fn tidy(home: &Home, listed: &[Listed], limit: u64) -> Used {
    let now = SystemTime::now();
    let mut kept = Vec::new();
    let mut used = Used::default();

    for (path, touched) in workspaces(home) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let age = now.duration_since(touched).unwrap_or_default();
        let job = listed
            .iter()
            .find(|job| crate::storage::file_name(&job.job) == name);
        match standing(job, age) {
            Standing::Done => {
                let _ = std::fs::remove_dir_all(&path);
            }
            Standing::Working => {
                used.count += 1;
                used.bytes += size_of(&path);
            }
            Standing::Kept => kept.push((path.clone(), touched, size_of(&path))),
        }
    }

    // The oldest go first when there is not room for all of them.
    kept.sort_by_key(|(_, touched, _)| std::cmp::Reverse(*touched));
    for (path, _, bytes) in kept {
        if used.bytes + bytes > limit {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            used.count += 1;
            used.bytes += bytes;
        }
    }

    used
}

/// Where one workspace stands, from what the instance lists about its job
/// and how long ago the workspace was last touched.
fn standing(job: Option<&Listed>, age: Duration) -> Standing {
    let Some(job) = job else {
        return if age < KEPT_UNLISTED {
            Standing::Kept
        } else {
            Standing::Done
        };
    };

    match (job.state.as_str(), job.review.as_deref()) {
        ("queued" | "running", _) => Standing::Working,
        (_, Some("awaiting_review")) => Standing::Kept,
        (_, Some(_)) => Standing::Done,
        (_, None) if age < KEPT_AFTER_ENDING => Standing::Kept,
        (_, None) => Standing::Done,
    }
}

/// Every workspace on this machine, with when it was last touched.
fn workspaces(home: &Home) -> Vec<(PathBuf, SystemTime)> {
    let Ok(entries) = std::fs::read_dir(jobs_dir(home)) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| {
            let path = entry.path();
            let touched = [path.clone(), path.join(".stemma/job.json")]
                .iter()
                .filter_map(|one| one.metadata().and_then(|meta| meta.modified()).ok())
                .max()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (path, touched)
        })
        .collect()
}

/// How many bytes a directory takes, without following links.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().map(|entry| size_of(&entry.path())).sum())
        .unwrap_or(0)
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

    /// A directory of its own for one test, empty.
    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("stemma-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn home_in(root: &Path) -> Home {
        Home::locate(&Environment::from_pairs([(HOME_VARIABLE, root)]))
    }

    fn put(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn listed(job: &str, state: &str, review: Option<&str>) -> Listed {
        serde_json::from_value(serde_json::json!({
            "job": job,
            "group": "algebra",
            "line": "quotients",
            "task": "explore",
            "agent": "claude",
            "state": state,
            "said": "",
            "review": review,
        }))
        .unwrap()
    }

    /// A workspace for `job` holding `bytes` bytes.
    fn workspace_of(home: &Home, job: &str, bytes: usize) -> PathBuf {
        let root = root_of(home, job);
        put(&root.join("line.json"), &"x".repeat(bytes));
        root
    }

    #[test]
    fn a_workspace_shares_lakes_packages_and_leaves_history_and_state_behind() {
        let root = scratch("copy-repository");
        let repository = root.join("repository");
        put(&repository.join("Algebra.lean"), "-- the library");
        put(
            &repository.join(".lake/packages/mathlib/Mathlib.olean"),
            "mathlib",
        );
        put(&repository.join(".lake/build/lib/Algebra.olean"), "built");
        put(&repository.join(".git/HEAD"), "ref: refs/heads/main");
        put(&repository.join(".stemma/check/mathlib-cache"), "stamp");
        let to = root.join("workspace");
        std::fs::create_dir(&to).unwrap();

        copy_repository(&repository, &to).unwrap();

        assert!(to.join("Algebra.lean").is_file());
        assert!(to.join(".lake/build/lib/Algebra.olean").is_file());
        assert!(to.join(".lake/packages/mathlib/Mathlib.olean").is_file());
        #[cfg(unix)]
        assert!(
            std::fs::symlink_metadata(to.join(".lake/packages"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!to.join(".git").exists());
        assert!(!to.join(".stemma").exists());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_private_job_points_lake_to_its_own_shared_copy() {
        let root = scratch("private-lakefile");
        let manifest = stemma_core::repository::Manifest::new(
            "grp:algebra".parse().unwrap(),
            repository::Side::Private,
            "Algebra, private",
        );
        for (path, source) in manifest.lake_files() {
            std::fs::write(root.join(path), source).unwrap();
        }

        relocate_private_lakefile(&root).unwrap();
        let lakefile = std::fs::read_to_string(root.join(repository::LAKEFILE)).unwrap();
        assert!(lakefile.contains("path = \".lake/workspace-shared\""));
        assert!(!lakefile.contains("path = \"../shared\""));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_workspace_that_could_not_be_prepared_is_removed() {
        let root = scratch("unprepared");
        let home = home_in(&root);
        let handed: Handed = serde_json::from_value(serde_json::json!({
            "job": "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T",
            "group": "algebra",
            "side": "shared",
            "line": "quotients",
            "revision": 1,
            "task": "explore",
            "agent": "claude",
            "launched_by": "usr:ana",
        }))
        .unwrap();

        let missing = root.join("no-repository-here");
        assert!(prepare(&home, &handed, &Task::Explore, "", Some(&missing), None).is_err());
        assert!(!root_of(&home, &handed.job).exists());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_workspace_is_kept_while_its_job_may_still_need_it() {
        let root = scratch("tidy-needed");
        let home = home_in(&root);
        let running = workspace_of(&home, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3A", 10);
        let reviewing = workspace_of(&home, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3B", 10);
        let accepted = workspace_of(&home, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3C", 10);
        let ended = workspace_of(&home, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3D", 10);
        let unlisted = workspace_of(&home, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3E", 10);

        let left = tidy(
            &home,
            &[
                listed("job:01K5YQ4TQ2VZ8H3M9N0P1R2S3A", "running", None),
                listed(
                    "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3B",
                    "done",
                    Some("awaiting_review"),
                ),
                listed("job:01K5YQ4TQ2VZ8H3M9N0P1R2S3C", "done", Some("accepted")),
                listed("job:01K5YQ4TQ2VZ8H3M9N0P1R2S3D", "done", None),
            ],
            u64::MAX,
        );

        assert!(running.exists() && reviewing.exists() && ended.exists() && unlisted.exists());
        assert!(!accepted.exists());
        assert_eq!(left, used(&home));
        assert_eq!(left.count, 4);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_oldest_workspaces_go_first_when_they_take_too_much() {
        let root = scratch("tidy-limit");
        let home = home_in(&root);
        let running = workspace_of(&home, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3A", 100);
        let older = workspace_of(&home, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3B", 100);
        let newer = workspace_of(&home, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3C", 100);
        let state = newer.join(".stemma/job.json");
        put(&state, "{}");
        std::fs::File::options()
            .write(true)
            .open(&state)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(60 * 60))
            .unwrap();

        let left = tidy(
            &home,
            &[
                listed("job:01K5YQ4TQ2VZ8H3M9N0P1R2S3A", "running", None),
                listed(
                    "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3B",
                    "done",
                    Some("awaiting_review"),
                ),
                listed(
                    "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3C",
                    "done",
                    Some("awaiting_review"),
                ),
            ],
            250,
        );

        assert!(running.exists() && newer.exists());
        assert!(!older.exists());
        assert_eq!(left.count, 2);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_job_that_ended_keeps_its_workspace_a_day_and_an_unlisted_one_a_week() {
        let hour = Duration::from_secs(60 * 60);
        let ended = listed("job:01K5YQ4TQ2VZ8H3M9N0P1R2S3A", "done", None);
        let withdrawn = listed("job:01K5YQ4TQ2VZ8H3M9N0P1R2S3B", "withdrawn", None);

        assert_eq!(standing(Some(&ended), hour), Standing::Kept);
        assert_eq!(standing(Some(&ended), 25 * hour), Standing::Done);
        assert_eq!(standing(Some(&withdrawn), 25 * hour), Standing::Done);
        assert_eq!(standing(None, 6 * 24 * hour), Standing::Kept);
        assert_eq!(standing(None, 8 * 24 * hour), Standing::Done);
    }
}

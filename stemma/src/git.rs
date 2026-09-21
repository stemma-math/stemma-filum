//! Running git on this machine.
//!
//! The local clone is a supported interface and not an implementation
//! detail: somebody may have edited a file, made a commit by hand, or
//! left work half finished, and all of that has to survive. So nothing
//! here resets, stashes or forces anything. Where the two sides have
//! moved apart, this says so and stops.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The branch a repository's work is on.
pub const BRANCH: &str = "main";

/// The remote a clone came from.
pub const REMOTE: &str = "origin";

/// How far a clone is from what the instance has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Distance {
    /// Commits here that are not there.
    pub ahead: u32,
    /// Commits there that are not here.
    pub behind: u32,
}

impl Distance {
    /// Whether the two sides are the same.
    pub fn is_together(self) -> bool {
        self.ahead == 0 && self.behind == 0
    }

    /// Whether each side has something the other has not.
    ///
    /// This is the one case nothing here can settle: it is the same
    /// person on two machines, and which version stands is theirs to say.
    pub fn has_parted(self) -> bool {
        self.ahead > 0 && self.behind > 0
    }
}

/// What a clone looks like right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Where it is.
    pub path: PathBuf,
    /// Whether there is work in it that is not committed.
    pub dirty: bool,
    /// How far it is from the instance.
    pub distance: Distance,
}

/// Whether there is a clone here.
pub fn is_clone(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Makes a clone, with everything a person needs to use git here by hand.
pub fn clone(url: &str, into: &Path, helper: &str) -> Result<()> {
    let parent = into.parent().context("a clone needs somewhere to go")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("cannot create {}", parent.display()))?;

    run(
        parent,
        &[
            // The credential is never written into the clone. Git asks
            // for it when it needs it, and this program answers.
            "-c",
            &format!("credential.helper={helper}"),
            "clone",
            "--quiet",
            "--",
            url,
            &into.display().to_string(),
        ],
    )?;

    // The same helper is written into the clone, so that a person running
    // git here by hand is not asked for a password they do not have.
    run(into, &["config", "credential.helper", helper])?;

    Ok(())
}

/// Makes sure a clone still knows how to ask for a credential.
///
/// The helper names a program by its path, and paths move: a build
/// directory is emptied, an application is dragged somewhere else. A
/// clone left naming one that is gone sends whoever uses it to a
/// password prompt they have no password for.
pub fn keep_helper(repository: &Path, helper: &str) -> Result<()> {
    // Absent rather than failed: git answers a name it does not have
    // by exiting without success, which here is simply `nothing yet`.
    let known = run(
        repository,
        &["config", "--local", "--get", "credential.helper"],
    )
    .unwrap_or_default();

    if known.trim() == helper {
        return Ok(());
    }

    run(repository, &["config", "credential.helper", helper])?;

    Ok(())
}

/// Asks the instance what it has, without changing anything here.
pub fn fetch(repository: &Path) -> Result<()> {
    run(repository, &["fetch", "--quiet", REMOTE])?;

    Ok(())
}

/// Whether anything in the clone is uncommitted.
pub fn is_dirty(repository: &Path) -> Result<bool> {
    Ok(!run(repository, &["status", "--porcelain"])?
        .trim()
        .is_empty())
}

/// How far the clone is from what was last fetched.
pub fn distance(repository: &Path) -> Result<Distance> {
    let counted = run(
        repository,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{REMOTE}/{BRANCH}...HEAD"),
        ],
    )?;

    let mut numbers = counted.split_whitespace();
    let behind = numbers.next().and_then(|count| count.parse().ok());
    let ahead = numbers.next().and_then(|count| count.parse().ok());

    match (behind, ahead) {
        (Some(behind), Some(ahead)) => Ok(Distance { ahead, behind }),
        _ => bail!("git did not say how far apart the two sides are"),
    }
}

/// Everything a status line needs to say about one clone.
pub fn state(repository: &Path) -> Result<State> {
    Ok(State {
        path: repository.to_owned(),
        dirty: is_dirty(repository)?,
        distance: distance(repository)?,
    })
}

/// Takes what the instance has, and only if nothing here is lost by it.
pub fn fast_forward(repository: &Path) -> Result<()> {
    run(
        repository,
        &[
            "merge",
            "--ff-only",
            "--quiet",
            &format!("{REMOTE}/{BRANCH}"),
        ],
    )?;

    Ok(())
}

/// Sends what is here.
pub fn push(repository: &Path) -> Result<()> {
    run(repository, &["push", "--quiet", REMOTE, BRANCH])?;

    Ok(())
}

/// What the clone's branch points at.
pub fn head(repository: &Path) -> Result<String> {
    Ok(run(repository, &["rev-parse", "HEAD"])?.trim().to_owned())
}

/// Commits one path using the person's ordinary git identity.
pub fn commit_path(repository: &Path, path: &Path, message: &str) -> Result<()> {
    commit_paths(repository, &[path.to_owned()], message)
}

/// Commits several paths atomically using the person's ordinary git identity.
pub fn commit_paths(repository: &Path, paths: &[PathBuf], message: &str) -> Result<()> {
    if paths.is_empty() {
        bail!("no paths were selected to commit");
    }
    let inside = paths
        .iter()
        .map(|path| {
            path.strip_prefix(repository)
                .context("the path is outside the repository")?
                .to_str()
                .context("the path is not text")
        })
        .collect::<Result<Vec<_>>>()?;
    let mut add = vec!["add", "--"];
    add.extend(inside.iter().copied());
    run(repository, &add)?;
    let mut commit = vec!["commit", "-m", message, "--"];
    commit.extend(inside.iter().copied());
    run(repository, &commit)?;
    Ok(())
}

/// Everything uncommitted in the clone, as paths inside it.
///
/// Best effort, and deliberately so: what this is asked is which lines
/// somebody has touched, and a path that cannot be read as one is a
/// path that answers nothing either way. Reading the records with `-z`
/// is what keeps a name with a space in it from arriving in pieces.
pub fn changed_paths(repository: &Path) -> Result<Vec<PathBuf>> {
    let said = run(
        repository,
        &["status", "--porcelain", "-z", "--untracked-files=all"],
    )?;

    Ok(said
        .split('\0')
        .filter(|record| record.len() > 3)
        .map(|record| repository.join(&record[3..]))
        .collect())
}

/// What a file held at one commit.
///
/// Reading the work a job is about goes through git rather than through
/// the working tree: the tree belongs to whoever is at the keyboard, may
/// be dirty, and is never touched by anything that runs here.
pub fn show(repository: &Path, commit: &str, path: &str) -> Result<String> {
    run(repository, &["show", &format!("{commit}:{path}")])
}

/// What the instance's side of the branch points at.
pub fn upstream(repository: &Path) -> Result<String> {
    Ok(
        run(repository, &["rev-parse", &format!("{REMOTE}/{BRANCH}")])?
            .trim()
            .to_owned(),
    )
}

/// The last commit the two sides shared.
pub fn common(repository: &Path) -> Result<String> {
    Ok(run(
        repository,
        &["merge-base", "HEAD", &format!("{REMOTE}/{BRANCH}")],
    )?
    .trim()
    .to_owned())
}

/// Every file one commit holds, as paths inside the repository.
pub fn files(repository: &Path, commit: &str) -> Result<Vec<String>> {
    Ok(run(repository, &["ls-tree", "-r", "--name-only", commit])?
        .lines()
        .map(str::to_owned)
        .collect())
}

/// Starts a merge without committing it, so the result can be looked at.
///
/// Git is left to do the ordinary work: files only one side touched,
/// files nobody touched, and everything that is not mathematics. What
/// it cannot do is decide between two versions of a statement, and this
/// says which files it could not settle.
pub fn merge_without_committing(repository: &Path) -> Result<Vec<String>> {
    let merging = run(
        repository,
        &[
            "merge",
            "--no-commit",
            "--no-ff",
            "--quiet",
            &format!("{REMOTE}/{BRANCH}"),
        ],
    );

    match merging {
        Ok(_) => Ok(Vec::new()),
        // A merge that stops on a conflict is the ordinary case here,
        // not a failure: what it could not settle is the answer.
        Err(error) => {
            let conflicts = unsettled(repository)?;
            if conflicts.is_empty() {
                return Err(error);
            }
            Ok(conflicts)
        }
    }
}

/// The paths a merge in progress could not settle.
pub fn unsettled(repository: &Path) -> Result<Vec<String>> {
    Ok(
        run(repository, &["diff", "--name-only", "--diff-filter=U"])?
            .lines()
            .map(str::to_owned)
            .collect(),
    )
}

/// Puts a merge down and leaves the clone exactly as it was.
pub fn abandon_merge(repository: &Path) -> Result<()> {
    run(repository, &["merge", "--abort"])?;

    Ok(())
}

/// Marks a path as settled.
pub fn settle(repository: &Path, inside: &str) -> Result<()> {
    run(repository, &["add", "--", inside])?;

    Ok(())
}

/// Commits whatever the index holds, which is how a merge is finished.
pub fn commit_index(repository: &Path, message: &str) -> Result<()> {
    run(repository, &["commit", "-m", message])?;

    Ok(())
}

/// Whether a merge is half done here.
pub fn merging(repository: &Path) -> bool {
    repository.join(".git").join("MERGE_HEAD").exists()
}

/// Whether a commit is one this clone holds.
pub fn holds(repository: &Path, commit: &str) -> bool {
    run(
        repository,
        &["cat-file", "-e", &format!("{commit}^{{commit}}")],
    )
    .is_ok()
}

/// The repository a path is inside, if it is inside one.
///
/// Walking up is how git finds a repository, and doing it the same way is
/// what lets somebody run a command from wherever they happen to be.
pub fn containing(start: &Path) -> Option<PathBuf> {
    let mut here = start.to_owned();

    loop {
        if is_clone(&here) {
            return Some(here);
        }

        if !here.pop() {
            return None;
        }
    }
}

/// Runs git, and turns a failure into something a person can read.
fn run(directory: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        // A prompt nobody can see is a program that has hung.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .with_context(|| format!("cannot run git {}", arguments.join(" ")))?;

    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_merge_without_conflicts_is_not_reported_as_success() {
        let root = std::env::temp_dir().join(format!("stemma-merge-error-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        run(&root, &["init", "--quiet"]).unwrap();
        let error = merge_without_committing(&root).unwrap_err();
        assert!(error.to_string().contains("git merge"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_empty_path_selection_cannot_commit_unrelated_staged_work() {
        assert!(commit_paths(Path::new("/nonexistent"), &[], "Do not commit").is_err());
    }

    #[test]
    fn two_sides_that_have_both_moved_are_the_case_nothing_here_settles() {
        assert!(
            Distance {
                ahead: 0,
                behind: 0
            }
            .is_together()
        );
        assert!(
            !Distance {
                ahead: 1,
                behind: 0
            }
            .is_together()
        );
        assert!(
            !Distance {
                ahead: 1,
                behind: 0
            }
            .has_parted()
        );
        assert!(
            !Distance {
                ahead: 0,
                behind: 1
            }
            .has_parted()
        );
        assert!(
            Distance {
                ahead: 1,
                behind: 1
            }
            .has_parted()
        );
    }

    #[test]
    fn a_repository_is_found_by_walking_up_from_wherever_you_are() {
        let root = std::env::temp_dir().join(format!("stemma-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let clone = root.join("stemma/algebra/shared");
        let deep = clone.join("sections/two");
        std::fs::create_dir_all(deep.join(".keep").parent().unwrap()).unwrap();
        std::fs::create_dir_all(clone.join(".git")).unwrap();

        assert_eq!(containing(&deep).as_deref(), Some(clone.as_path()));
        assert_eq!(containing(&clone).as_deref(), Some(clone.as_path()));
        assert_eq!(containing(&root), None);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_clone_is_told_the_helper_it_does_not_have_yet_and_the_one_that_moved() {
        let root = std::env::temp_dir().join(format!("stemma-helper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        run(&root, &["init", "--quiet"]).unwrap();

        let reading = || {
            run(&root, &["config", "--local", "--get", "credential.helper"])
                .unwrap_or_default()
                .trim()
                .to_owned()
        };

        assert_eq!(reading(), "");

        keep_helper(&root, "!'/somewhere/stemma' credential").unwrap();
        assert_eq!(reading(), "!'/somewhere/stemma' credential");

        keep_helper(&root, "!'/elsewhere/stemma' credential").unwrap();
        assert_eq!(reading(), "!'/elsewhere/stemma' credential");

        std::fs::remove_dir_all(&root).unwrap();
    }
}

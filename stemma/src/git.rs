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
            url,
            &into.display().to_string(),
        ],
    )?;

    // The same helper is written into the clone, so that a person running
    // git here by hand is not asked for a password they do not have.
    run(into, &["config", "credential.helper", helper])?;

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
    let inside = path
        .strip_prefix(repository)
        .context("the path is outside the repository")?;
    run(
        repository,
        &[
            "add",
            "--",
            inside.to_str().context("the path is not text")?,
        ],
    )?;
    run(
        repository,
        &[
            "commit",
            "-m",
            message,
            "--",
            inside.to_str().context("the path is not text")?,
        ],
    )?;
    Ok(())
}

/// What a file held at one commit.
///
/// Reading the work a job is about goes through git rather than through
/// the working tree: the tree belongs to whoever is at the keyboard, may
/// be dirty, and is never touched by anything that runs here.
pub fn show(repository: &Path, commit: &str, path: &str) -> Result<String> {
    run(repository, &["show", &format!("{commit}:{path}")])
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
}

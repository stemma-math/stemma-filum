//! Turning what somebody wrote into something the group can read.
//!
//! Writing a line and publishing it are not the same act and do not
//! happen at the same rate. The editor writes the file as often as
//! somebody pauses, because what is on disk must survive the window
//! closing; a commit is a unit of history with an author and a message,
//! and one per keystroke would be a log nobody can read.
//!
//! So the commit happens where the writing ends. That boundary already
//! exists and is called a claim: taking one says this machine is
//! writing the line, and letting it go, leaving it or closing the
//! window says the writing is done.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use stemma_core::line::Name;
use stemma_core::repository;

use crate::git;

/// A line that was written into the clone's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// Which line.
    pub line: Name,
    /// Where the clone is.
    pub clone: PathBuf,
}

impl std::fmt::Display for Recorded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} written", self.line)
    }
}

/// Commits the line a path belongs to, where anything of it changed.
///
/// Nothing to record is not a failure: leaving a line without touching
/// it is the ordinary way to leave a line.
pub fn record(path: &Path) -> Result<Option<Recorded>> {
    let clone = git::containing(path).context("this is not inside a clone")?;
    let line = repository::line_of(&clone, path).context("this is not inside a line")?;

    record_line(&clone, &line)
}

/// Commits one line of one clone, where anything of it changed.
pub fn record_line(clone: &Path, line: &Name) -> Result<Option<Recorded>> {
    let directory = repository::line_directory(clone, line);

    if !changed(clone, &directory)? {
        return Ok(None);
    }

    git::commit_path(clone, &directory, &format!("Write {line}"))?;

    Ok(Some(Recorded {
        line: line.clone(),
        clone: clone.to_owned(),
    }))
}

/// Commits every line of a clone that changed and that nobody is
/// writing here.
///
/// The exception is the point. This runs from synchronising, which the
/// daemon also does, and committing a line somebody has open would
/// publish a half-written sentence they were still thinking about.
pub fn record_loose(clone: &Path, held_here: &[Name]) -> Result<Vec<Recorded>> {
    let mut recorded = Vec::new();

    for line in touched(clone)? {
        if held_here.contains(&line) {
            continue;
        }

        if let Some(one) = record_line(clone, &line)? {
            recorded.push(one);
        }
    }

    Ok(recorded)
}

/// Which lines of the clone have something uncommitted in them.
fn touched(clone: &Path) -> Result<Vec<Name>> {
    let mut found: Vec<Name> = Vec::new();

    for path in git::changed_paths(clone)? {
        let Some(line) = repository::line_of(clone, &path) else {
            continue;
        };

        if !found.contains(&line) {
            found.push(line);
        }
    }

    found.sort_by(|a, b| a.as_str().cmp(b.as_str()));

    Ok(found)
}

/// Whether anything under a directory of the clone is uncommitted.
fn changed(clone: &Path, directory: &Path) -> Result<bool> {
    Ok(git::changed_paths(clone)?
        .iter()
        .any(|path| path.starts_with(directory)))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::process::Command;

    /// A clone with one line in it, committed.
    fn repository(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "stemma-writing-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let git = |arguments: &[&str]| {
            Command::new("git")
                .current_dir(&root)
                .args(arguments)
                .output()
                .unwrap();
        };

        git(&["init", "--quiet"]);
        git(&["config", "user.email", "somebody@example.org"]);
        git(&["config", "user.name", "Somebody"]);

        let line = repository::line_directory(&root, &Name::parse("one").unwrap());
        std::fs::create_dir_all(&line).unwrap();
        std::fs::write(line.join(repository::ENTRY), "= One\n").unwrap();

        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "Start one"]);

        root
    }

    fn entry(clone: &Path, line: &str) -> PathBuf {
        repository::line_entry(clone, &Name::parse(line).unwrap())
    }

    fn commits(clone: &Path) -> usize {
        String::from_utf8(
            Command::new("git")
                .current_dir(clone)
                .args(["log", "--format=%s"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .lines()
        .count()
    }

    #[test]
    fn a_line_that_changed_is_written_into_the_history_once() {
        let clone = repository("changed");
        let path = entry(&clone, "one");

        std::fs::write(&path, "= One\n\nMore.\n").unwrap();

        let recorded = record(&path).unwrap().expect("something to record");

        assert_eq!(recorded.line.as_str(), "one");
        assert_eq!(commits(&clone), 2);
        assert!(!git::is_dirty(&clone).unwrap());

        // Leaving it again records nothing, rather than an empty commit.
        assert_eq!(record(&path).unwrap(), None);
        assert_eq!(commits(&clone), 2);

        std::fs::remove_dir_all(&clone).unwrap();
    }

    #[test]
    fn a_line_somebody_is_writing_here_is_left_alone() {
        let clone = repository("held");
        let one = Name::parse("one").unwrap();

        std::fs::write(entry(&clone, "one"), "= One\n\nStill thinking.\n").unwrap();

        assert_eq!(
            record_loose(&clone, std::slice::from_ref(&one)).unwrap(),
            Vec::new()
        );
        assert!(git::is_dirty(&clone).unwrap());

        let recorded = record_loose(&clone, &[]).unwrap();

        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].line, one);
        assert!(!git::is_dirty(&clone).unwrap());

        std::fs::remove_dir_all(&clone).unwrap();
    }

    #[test]
    fn a_line_nobody_touched_is_not_committed() {
        let clone = repository("untouched");

        assert_eq!(record_loose(&clone, &[]).unwrap(), Vec::new());
        assert_eq!(commits(&clone), 1);

        std::fs::remove_dir_all(&clone).unwrap();
    }
}

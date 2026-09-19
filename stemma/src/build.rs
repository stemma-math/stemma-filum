//! Reading what is written here.
//!
//! Building a line means reading what its source declares — the nodes,
//! which of them the line rests on, and what leans on what. It does not
//! mean typesetting: that needs the pinned compiler, and this has to work
//! on a machine that has not installed one yet.
//!
//! What comes out is keyed by the commit it was read from, so it can be
//! handed to the instance and shown to somebody with no machine running.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use stemma_core::artefact::{LineDigest, Nodes};
use stemma_core::line::{Fault, Line, Name};
use stemma_core::repository::{self, Manifest};
use stemma_core::source;

use crate::config::Config;
use crate::git;
use crate::home::Home;
use crate::pin;
use crate::workspace::Side;

/// What one repository turned out to hold.
#[derive(Debug, Clone)]
pub struct Built {
    /// Where the clone is.
    pub path: PathBuf,
    /// Which of the group's two repositories it is.
    pub side: Side,
    /// What was read, keyed by the commit it was read from.
    pub nodes: Nodes,
    /// What is wrong with it, by line.
    pub faults: Vec<Faulty>,
}

impl Built {
    /// Whether everything in it could be read.
    pub fn is_sound(&self) -> bool {
        self.faults.is_empty()
    }
}

/// A line that could not be read, and why.
#[derive(Debug, Clone)]
pub struct Faulty {
    /// Which line of work.
    pub name: Name,
    /// Where its source is.
    pub source: PathBuf,
    /// What is wrong with it.
    pub faults: Vec<Fault>,
}

/// Reads every repository this machine has, or the one it is inside.
pub fn run(home: &Home, here: &Path, out: &mut impl Write) -> Result<Vec<Built>> {
    let clones = match git::containing(here) {
        Some(clone) => vec![clone],
        None => every_clone(home)?,
    };

    if clones.is_empty() {
        writeln!(
            out,
            "There is nothing here to read. Run `stemma sync` to bring a group down."
        )?;

        return Ok(Vec::new());
    }

    let mut built = Vec::new();

    for clone in clones {
        // One repository nobody can read is not a reason to stop looking
        // at the others, and saying which one it was is the useful part.
        match read(&clone) {
            Ok(one) => {
                report(&one, out)?;
                built.push(one);
            }
            Err(error) => writeln!(out, "{}\n  {error:#}", clone.display())?,
        }
    }

    Ok(built)
}

/// Every clone this machine knows about.
fn every_clone(home: &Home) -> Result<Vec<PathBuf>> {
    let config = Config::read(&home.config_file())?;
    let workspace = config.workspace_or(home.workspace());

    let mut clones = Vec::new();
    let Ok(entries) = std::fs::read_dir(&workspace) else {
        return Ok(clones);
    };

    let mut groups: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    groups.extend(config.groups.values().cloned());
    groups.sort();
    groups.dedup();

    for group in groups {
        for side in Side::both() {
            let path = group.join(side.directory());

            if git::is_clone(&path) {
                clones.push(path);
            }
        }
    }

    Ok(clones)
}

/// Reads one repository.
pub fn read(clone: &Path) -> Result<Built> {
    let manifest = read_manifest(clone)?;
    let commit = git::head(clone).unwrap_or_else(|_| "unknown".to_owned());

    let mut nodes = Nodes::new(manifest.group.clone(), manifest.side, commit);
    nodes.typst = manifest.pins.typst.clone();

    let mut faults = Vec::new();

    for name in lines_in(clone)? {
        let source_path = repository::line_entry(clone, &name);
        let text = std::fs::read_to_string(&source_path)
            .with_context(|| format!("cannot read {}", source_path.display()))?;

        match Line::read(&text) {
            Ok(line) => nodes.lines.push(LineDigest {
                name,
                title: source::title(&text),
                line,
            }),
            Err(found) => faults.push(Faulty {
                name,
                source: source_path,
                faults: found,
            }),
        }
    }

    Ok(Built {
        path: clone.to_owned(),
        side: match manifest.side {
            repository::Side::Shared => Side::Shared,
            repository::Side::Private => Side::Private,
        },
        nodes,
        faults,
    })
}

/// Reads the file at the root that says what the repository is.
pub fn read_manifest(clone: &Path) -> Result<Manifest> {
    let path = clone.join(repository::MANIFEST);
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "{} has no {}, so it is not a Stemma repository",
            clone.display(),
            repository::MANIFEST
        )
    })?;

    Manifest::parse(&text).with_context(|| format!("cannot use {}", path.display()))
}

/// The lines in a repository, in the order their directories sort.
pub fn lines_in(clone: &Path) -> Result<Vec<Name>> {
    let directory = clone.join(repository::LINES);

    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(Vec::new());
    };

    let mut names: Vec<Name> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(repository::ENTRY).is_file())
        .filter_map(|entry| Name::parse(entry.file_name().to_str()?).ok())
        .collect();

    names.sort_by(|one, other| one.as_str().cmp(other.as_str()));

    Ok(names)
}

/// Says what one repository holds, and what is wrong with it.
fn report(built: &Built, out: &mut impl Write) -> Result<()> {
    let where_it_is = built.path.display();
    let lines = built.nodes.lines.len();
    let statements = built.nodes.statement_count();

    writeln!(
        out,
        "{where_it_is}\n  {}, {} at {}",
        count(lines, "line"),
        count(statements, "statement"),
        short(&built.nodes.commit)
    )?;

    for line in &built.nodes.lines {
        let central = line.line.central().len();
        let boxes = line.line.boxes();
        let empty = boxes
            .iter()
            .filter(|black_box| black_box.state == stemma_core::model::BoxState::Empty)
            .count();

        writeln!(
            out,
            "  {:<24} {}, {} the line rests on, {} ({} still empty)",
            line.shown_as(),
            count(line.line.nodes.len(), "statement"),
            central,
            count(boxes.len(), "box"),
            empty
        )?;
    }

    for faulty in &built.faults {
        writeln!(out, "  {} cannot be read:", faulty.name)?;

        for fault in &faulty.faults {
            writeln!(out, "    {}:{}", faulty.source.display(), fault)?;
        }
    }

    match &built.nodes.typst {
        Some(pinned) => writeln!(out, "  {}", pin::report(pinned))?,
        None => writeln!(
            out,
            "  nothing is pinned yet; set `pins.typst` in {} before composing",
            repository::MANIFEST
        )?,
    }

    Ok(())
}

/// A count with the right noun.
fn count(many: usize, noun: &str) -> String {
    match many {
        1 => format!("1 {noun}"),
        many if noun.ends_with('x') => format!("{many} {noun}es"),
        many => format!("{many} {noun}s"),
    }
}

/// A commit, short enough to read.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(8)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_carry_the_right_noun() {
        assert_eq!(count(1, "line"), "1 line");
        assert_eq!(count(3, "line"), "3 lines");
        assert_eq!(count(1, "box"), "1 box");
        assert_eq!(count(2, "box"), "2 boxes");
    }

    #[test]
    fn a_commit_is_shown_short() {
        assert_eq!(short("0123456789abcdef"), "01234567");
        assert_eq!(short("short"), "short");
    }

    #[test]
    fn a_repository_that_is_not_one_says_so() {
        let error = read_manifest(Path::new("/nonexistent/clone")).unwrap_err();

        assert!(error.to_string().contains("not a Stemma repository"));
    }
}

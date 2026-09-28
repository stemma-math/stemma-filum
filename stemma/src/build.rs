//! Reading what is written here, and publishing it.
//!
//! Building a repository means reading what each line declares — the
//! nodes, which of them the line rests on, what leans on what — and what
//! its Lean module establishes. It does not mean typesetting: every
//! revision already carries its page.
//!
//! What comes out names the revision of every line it read, so the web
//! shows it for a line exactly while the line is at that revision. It is
//! built from what the engine keeps, and a line with something not sent
//! yet is read without a revision: it describes nothing the group has.
//!
//! The machine that wrote a line builds its repository when the writing
//! is over; when it cannot, the instance asks another machine for it.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use stemma_core::artefact::{LineDigest, Nodes};
use stemma_core::document::Document;
use stemma_core::line::{Fault, Line, Name};
use stemma_core::repository::{self, Manifest};

use crate::api::Server;
use crate::credentials::Credentials;
use crate::home::Home;
use crate::mirror::{self, Mirror};
use crate::pin;
use crate::workspace::Side;

/// What one repository turned out to hold.
#[derive(Debug, Clone)]
pub struct Built {
    /// Where it is shown on this machine.
    pub path: PathBuf,
    /// Which of the group's two repositories it is.
    pub side: Side,
    /// What was read.
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

/// Builds every repository this machine has, or the one it is inside,
/// and publishes what each holds.
pub fn run(home: &Home, here: &Path, out: &mut impl Write) -> Result<Vec<Built>> {
    let mirror = Mirror::at(home)?;
    let inside = mirror::repository_of(here)
        .and_then(|root| read_manifest(&root).ok())
        .map(|manifest| (manifest.group.slug().as_str().to_owned(), manifest.side));

    let repositories: Vec<(String, repository::Side)> = mirror
        .repositories()?
        .into_iter()
        .filter(|found| inside.as_ref().is_none_or(|inside| inside == found))
        .collect();

    if repositories.is_empty() {
        writeln!(
            out,
            "There are no lines here yet. Run `stemma sync` to get your groups."
        )?;

        return Ok(Vec::new());
    }

    let credentials = Credentials::read(&home.credentials_file())?;
    let mut built = Vec::new();

    for (group, side) in repositories {
        // One repository nobody can read is not a reason to stop looking
        // at the others, and saying which one it was is the useful part.
        match read(&mirror, &group, side) {
            Ok(one) => {
                report(&one, out)?;
                let published = publish(&one, credentials.as_ref());
                if published == Published::Kept {
                    mirror.built(&one.nodes)?;
                }
                writeln!(out, "  {published}")?;
                built.push(one);
            }
            Err(error) => writeln!(out, "{group}/{side}\n  {error:#}")?,
        }
    }

    Ok(built)
}

/// Builds and publishes every repository whose writing is over and has
/// not been built, and says which it published. For the engine, after a
/// round; what fails is tried again after the next.
pub fn publish_due(
    home: &Home,
    mirror: &Mirror,
    released: &[String],
    quiet: std::time::Duration,
) -> Result<Vec<String>> {
    let due = mirror.due_builds(released, quiet)?;
    if due.is_empty() {
        return Ok(Vec::new());
    }

    let credentials = Credentials::read(&home.credentials_file())?;
    let mut published = Vec::new();

    for (group, side) in due {
        let built = read(mirror, &group, side)?;
        match publish(&built, credentials.as_ref()) {
            Published::Kept => {
                mirror.built(&built.nodes)?;
                published.push(format!("{group}/{side}"));
            }
            other => anyhow::bail!("{group}/{side}: {other}"),
        }
    }

    Ok(published)
}

/// Reads one repository, as this machine has it.
pub fn read(mirror: &Mirror, group: &str, side: repository::Side) -> Result<Built> {
    let root = mirror.repository(group, side);
    let manifest = read_manifest(&root)?;
    let pins = group_pins(&root, &manifest);

    let mut nodes = Nodes::new(manifest.group.clone(), manifest.side, String::new());
    nodes.typst = pins.typst.clone();

    let mut records: Vec<_> = mirror
        .records()?
        .into_iter()
        .filter(|record| record.group == group && record.side == side)
        .collect();
    records.sort_by(|one, other| one.name.cmp(&other.name));

    let mut faults = Vec::new();

    for record in records {
        let Ok(name) = Name::parse(&record.name) else {
            continue;
        };
        let read = Document::parse(&record.source)
            .map_err(|unreadable| vec![Fault::from(unreadable)])
            .and_then(|document| Line::of(&document).map(|line| (document, line)));

        match read {
            Ok((document, line)) => {
                let lean = crate::lean::check(&root, &name, &pins)?;
                let sent = !record.pending() && record.there.is_none();
                nodes.lines.push(LineDigest {
                    name,
                    title: Some(document.title).filter(|title| !title.trim().is_empty()),
                    description: document.description,
                    line,
                    lean,
                    revision: sent.then_some(record.base),
                });
            }
            Err(found) => faults.push(Faulty {
                source: mirror.entry(&record),
                name,
                faults: found,
            }),
        }
    }

    nodes.commit = named(&nodes);

    Ok(Built {
        path: root,
        side: match manifest.side {
            repository::Side::Shared => Side::Shared,
            repository::Side::Private => Side::Private,
        },
        nodes,
        faults,
    })
}

/// What an artefact is filed under: what it says, digested, so the same
/// build is the same file however often it is sent.
fn named(nodes: &Nodes) -> String {
    let mut unnamed = nodes.clone();
    unnamed.commit = String::new();

    Sha256::digest(unnamed.to_text().as_bytes())
        .iter()
        .take(20)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What the group has pinned, wherever it is written down.
///
/// The pin belongs to the group, not to a repository: a private line is
/// written against the same environment as the group's own work, or the
/// two would not compose to the same thing. So a private clone with no
/// pin of its own takes the group's, from the shared clone beside it.
pub fn group_pin(clone: &Path, manifest: &Manifest) -> Option<String> {
    group_pins(clone, manifest).typst
}

/// Every compiler pin belonging to the group, inherited by its private side.
pub fn group_pins(clone: &Path, manifest: &Manifest) -> stemma_core::repository::Pins {
    let mut pins = manifest.pins.clone();

    if manifest.side != repository::Side::Private
        || (pins.typst.is_some()
            && pins.lean.is_some()
            && pins.mathlib.is_some()
            && pins.stemma.is_some())
    {
        return pins;
    }

    let Some(parent) = clone.parent() else {
        return pins;
    };
    let beside = parent.join(Side::Shared.directory());
    let Ok(shared) = read_manifest(&beside) else {
        return pins;
    };

    if shared.group == manifest.group {
        pins.typst = pins.typst.or(shared.pins.typst);
        pins.lean = pins.lean.or(shared.pins.lean);
        pins.mathlib = pins.mathlib.or(shared.pins.mathlib);
        pins.stemma = pins.stemma.or(shared.pins.stemma);
        if pins.dependencies.is_empty() {
            pins.dependencies = shared.pins.dependencies;
        }
    }
    pins
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
        "{where_it_is}\n  {}, {}",
        count(lines, "line"),
        count(statements, "statement"),
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

/// What became of handing an artefact to the instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Published {
    /// The instance has it.
    Kept,
    /// This machine is not signed in to anything.
    Unknown,
    /// The instance would not take it.
    Refused(String),
}

impl std::fmt::Display for Published {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kept => f.write_str("published to the server"),
            Self::Unknown => f.write_str("not published: this machine is not signed in"),
            Self::Refused(why) => write!(f, "not published: {why}"),
        }
    }
}

/// Hands what was read to the instance.
pub fn publish(built: &Built, credentials: Option<&Credentials>) -> Published {
    let Some(credentials) = credentials else {
        return Published::Unknown;
    };

    let server = match Server::at(&credentials.server) {
        Ok(server) => server,
        Err(error) => return Published::Refused(format!("{error:#}")),
    };

    let outcome = server.publish(
        &credentials.token,
        built.nodes.group.slug().as_str(),
        built.nodes.side.as_str(),
        stemma_core::artefact::Kind::Nodes.as_str(),
        &built.nodes.commit,
        &built.nodes.to_text(),
    );

    match outcome {
        Ok(()) => Published::Kept,
        Err(error) => Published::Refused(format!("{error:#}")),
    }
}

/// A count with the right noun.
fn count(many: usize, noun: &str) -> String {
    match many {
        1 => format!("1 {noun}"),
        many if noun.ends_with('x') => format!("{many} {noun}es"),
        many => format!("{many} {noun}s"),
    }
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
    fn a_private_clone_takes_the_group_pin_from_the_shared_one_beside_it() {
        use stemma_core::repository::{Manifest, Side as Which};

        let root = std::env::temp_dir().join(format!("stemma-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let group = "grp:algebra".parse().unwrap();
        let shared = root.join("algebra/shared");
        let private = root.join("algebra/private");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::create_dir_all(&private).unwrap();

        let mut theirs = Manifest::new(group, Which::Shared, "Algebra");
        theirs.pins.typst = Some("0.13.1".to_owned());
        std::fs::write(
            shared.join(stemma_core::repository::MANIFEST),
            theirs.to_text(),
        )
        .unwrap();

        let mine = Manifest::new("grp:algebra".parse().unwrap(), Which::Private, "Mine");
        assert_eq!(group_pin(&private, &mine).as_deref(), Some("0.13.1"));

        // A group that has pinned nothing pins nothing either way.
        let others = Manifest::new("grp:topology".parse().unwrap(), Which::Private, "Mine");
        assert_eq!(group_pin(&private, &others), None);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_machine_with_nothing_to_send_to_says_so_rather_than_failing() {
        assert_eq!(
            Published::Unknown.to_string(),
            "not published: this machine is not signed in"
        );
    }

    #[test]
    fn a_repository_that_is_not_one_says_so() {
        let error = read_manifest(Path::new("/nonexistent/clone")).unwrap_err();

        assert!(error.to_string().contains("not a Stemma repository"));
    }
}

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

use anyhow::Result;
use sha2::{Digest, Sha256};
use stemma_core::artefact::{LineDigest, Nodes};
use stemma_core::document::Document;
use stemma_core::line::{Fault, Line, Name};
use stemma_core::repository::{self, Side};

use crate::account::credentials::Credentials;
use crate::api::Server;
use crate::home::Home;
use crate::mirror::Mirror;
use crate::typeset::pin;

pub use crate::repository::{group_pin, group_pins, lines_in, read_manifest};

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
    let inside = crate::repository::repository_of(here)
        .and_then(|root| read_manifest(&root).ok())
        .map(|manifest| (manifest.group.slug().as_str().to_owned(), manifest.side));

    let repositories: Vec<(String, Side)> = mirror
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
///
/// One repository that cannot be built or published does not hold the
/// others back: each is tried, and what failed is the error, once every
/// one has been.
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
    let mut failed = Vec::new();

    for (group, side) in due {
        let outcome = read(mirror, &group, side).and_then(|built| {
            match publish(&built, credentials.as_ref()) {
                Published::Kept => mirror.built(&built.nodes).map(|()| true),
                other => {
                    failed.push(format!("{group}/{side}: {other}"));
                    Ok(false)
                }
            }
        });

        match outcome {
            Ok(true) => published.push(format!("{group}/{side}")),
            Ok(false) => {}
            Err(error) => failed.push(format!("{group}/{side}: {error:#}")),
        }
    }

    match failed.is_empty() {
        true => Ok(published),
        false => anyhow::bail!("{}", failed.join("; ")),
    }
}

/// Reads one repository, as this machine has it.
pub fn read(mirror: &Mirror, group: &str, side: Side) -> Result<Built> {
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
    // Checking the Lean library builds the whole of it, so it is done
    // once for the repository, and only when a line is there to read it.
    let mut library = None;

    for record in records {
        let Ok(name) = Name::parse(&record.name) else {
            continue;
        };
        let read = Document::parse(&record.source)
            .map_err(|unreadable| vec![Fault::from(unreadable)])
            .and_then(|document| Line::of(&document).map(|line| (document, line)));

        match read {
            Ok((document, line)) => {
                let library = match &library {
                    Some(library) => library,
                    None => library.insert(crate::lean::check_library(&root, &pins)?),
                };
                let lean = library.digest(&root, &name);
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
        side: manifest.side,
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
    fn a_machine_with_nothing_to_send_to_says_so_rather_than_failing() {
        assert_eq!(
            Published::Unknown.to_string(),
            "not published: this machine is not signed in"
        );
    }
}

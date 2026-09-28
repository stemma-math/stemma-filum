//! Opening and saving a line's source.

use serde::Serialize;
use stemma_core::artefact::LeanDigest;
use stemma_core::document::Document;
use stemma_core::line::{Fault, Line, Name};

use stemma::mirror::Mirror;

use crate::engine::{self, Nudge};
use crate::workspace::{LineAt, LineEntry};
use crate::{failed, home, said};

/// A line as the editor holds it: its blocks and what can be read out of them.
#[derive(Serialize)]
pub struct OpenLine {
    entry: LineEntry,
    /// The line, when the file on disk is one.
    document: Option<Document>,
    /// The file as it is, for when it is not a line and has to be fixed by hand.
    source: String,
    line: Option<Line>,
    faults: Vec<Problem>,
    lean: Option<LeanDigest>,
}

/// Something wrong with the line, and the block it is about.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Problem {
    /// Which block, counting from one, when it is about one.
    at: Option<usize>,
    message: String,
}

impl OpenLine {
    /// The line as `source` reads, with its last Lean check. A check that
    /// cannot be made is one more thing wrong with the line rather than a
    /// failure: the line was still read, or saved.
    fn of(line: &LineAt, source: String) -> Self {
        let (document, read, mut faults) = analyse(&source);
        let lean = lean_status(line).unwrap_or_else(|error| {
            faults.push(Problem {
                at: None,
                message: failed(error),
            });
            None
        });
        Self {
            entry: line.entry(),
            document,
            source,
            line: read,
            faults,
            lean,
        }
    }
}

/// Reads a line from disk for the editor.
#[tauri::command(async)]
pub fn open_line(path: String) -> Result<OpenLine, String> {
    let line = LineAt::admit(&path)?;
    let source = std::fs::read_to_string(&line.path).map_err(said)?;
    Ok(OpenLine::of(&line, source))
}

/// Saves the line through the engine, which sends it after a pause.
/// Called whenever the writer pauses.
#[tauri::command(async)]
pub fn save_line(path: String, document: Document) -> Result<OpenLine, String> {
    let line = LineAt::admit(&path)?;
    Mirror::at(&home())
        .and_then(|mirror| mirror.save(&line.group, line.side()?, &line.name, &document))
        .map_err(failed)?;
    engine::nudge(Nudge::Saved);
    Ok(OpenLine::of(&line, document.to_text()))
}

/// Reads the line out of its source, collecting every fault.
fn analyse(source: &str) -> (Option<Document>, Option<Line>, Vec<Problem>) {
    let document = match Document::parse(source) {
        Ok(document) => document,
        Err(unreadable) => {
            let problem = Problem {
                at: None,
                message: unreadable.to_string(),
            };
            return (None, None, vec![problem]);
        }
    };

    match Line::of(&document) {
        Ok(line) => (Some(document), Some(line), Vec::new()),
        Err(faults) => (Some(document), None, faults.iter().map(problem).collect()),
    }
}

fn problem(fault: &Fault) -> Problem {
    Problem {
        at: fault.at(),
        message: fault.to_string(),
    }
}

/// The Lean check of this line against the group's pinned toolchain.
fn lean_status(line: &LineAt) -> anyhow::Result<Option<LeanDigest>> {
    let clone = line.clone_root();
    let manifest = stemma::repository::read_manifest(clone)?;
    let pins = stemma::repository::group_pins(clone, &manifest);
    stemma::lean::check(clone, &Name::parse(&line.name)?, &pins)
}

#[cfg(test)]
mod tests {
    use stemma_core::document::{Block, Document};

    use super::analyse;

    #[test]
    fn an_unfinished_line_keeps_its_blocks_and_says_which_one_is_wrong() {
        let unproved = Block::theorem("Theorem", Some("thm:main".parse().unwrap()), "x");
        let source = Document::new("A line", None)
            .with(Block::prose("An idea."))
            .with(unproved)
            .to_text();

        let (document, line, faults) = analyse(&source);

        assert_eq!(document.map(|document| document.blocks.len()), Some(2));
        assert!(line.is_none());
        assert_eq!(faults[0].at, Some(2));
    }

    #[test]
    fn a_valid_line_keeps_its_apparatus() {
        let source = Document::new("A line", None)
            .with(Block::definition(None, "A group."))
            .to_text();

        let (_, line, faults) = analyse(&source);

        assert_eq!(line.map(|line| line.nodes.len()), Some(1));
        assert!(faults.is_empty());
    }

    #[test]
    fn a_file_that_is_not_a_line_is_reported_whole() {
        let (document, _, faults) = analyse("= Hola\n");

        assert!(document.is_none());
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].at, None);
    }
}

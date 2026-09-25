//! Opening, saving and recording a line's source.

use serde::Serialize;
use stemma_core::artefact::LeanDigest;
use stemma_core::document::Document;
use stemma_core::line::{Fault, Line, Name};

use crate::said;
use crate::workspace::{LineAt, LineEntry};

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
    fn of(line: &LineAt, source: String) -> Result<Self, String> {
        let (document, read, faults) = analyse(&source);
        Ok(Self {
            entry: line.entry(),
            document,
            source,
            line: read,
            faults,
            lean: lean_status(line)?,
        })
    }
}

#[tauri::command(async)]
pub fn open_line(path: String) -> Result<OpenLine, String> {
    let line = LineAt::admit(&path)?;
    let source = std::fs::read_to_string(&line.path).map_err(said)?;
    OpenLine::of(&line, source)
}

/// Writes the line to disk. Called whenever the writer pauses.
#[tauri::command(async)]
pub fn save_line(path: String, document: Document) -> Result<OpenLine, String> {
    let line = LineAt::admit(&path)?;
    let source = document.to_text();
    stemma::storage::replace(&line.path, &source).map_err(said)?;
    OpenLine::of(&line, source)
}

/// Commits the line into its clone's history, returning the commit when one
/// was made. Called when a stretch of writing ends, not on every save.
#[tauri::command(async)]
pub fn record_line(path: String) -> Result<Option<String>, String> {
    let line = LineAt::admit(&path)?;
    Ok(stemma::writing::record(&line.path)
        .map_err(said)?
        .map(|recorded| recorded.to_string()))
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

/// The last Lean check of this line against the group's pinned toolchain.
fn lean_status(line: &LineAt) -> Result<Option<LeanDigest>, String> {
    let clone = line.clone_root();
    let manifest = stemma::build::read_manifest(clone).map_err(said)?;
    let pins = stemma::build::group_pins(clone, &manifest);
    stemma::lean::check(clone, &Name::parse(&line.name).map_err(said)?, &pins).map_err(said)
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

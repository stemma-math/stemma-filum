//! Opening, saving and recording a line's source.

use serde::Serialize;
use stemma_core::artefact::LeanDigest;
use stemma_core::line::{BlackBox, Line, Name};
use stemma_core::notation;

use crate::said;
use crate::workspace::{LineAt, LineEntry};

/// A line as the editor holds it: its source and what can be read out of it.
#[derive(Serialize)]
pub struct OpenLine {
    entry: LineEntry,
    source: String,
    line: Option<Line>,
    boxes: Vec<BlackBox>,
    diagnostics: Vec<String>,
    lean: Option<LeanDigest>,
}

impl OpenLine {
    fn of(line: &LineAt, source: String) -> Result<Self, String> {
        let (read, boxes, diagnostics) = analyse(&source);
        Ok(Self {
            entry: line.entry(),
            source,
            line: read,
            boxes,
            diagnostics,
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

/// Writes the source to disk. Called whenever the writer pauses.
#[tauri::command(async)]
pub fn save_line(path: String, source: String) -> Result<OpenLine, String> {
    let line = LineAt::admit(&path)?;
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

/// Prepends the notation import to a line that uses the environments
/// without importing them, and saves it.
#[tauri::command(async)]
pub fn add_notation(path: String) -> Result<OpenLine, String> {
    let line = LineAt::admit(&path)?;
    let source = std::fs::read_to_string(&line.path).map_err(said)?;

    if !notation::missing_from(&source) {
        return OpenLine::of(&line, source);
    }

    let source = format!("{}\n\n{source}", notation::import());
    stemma::storage::replace(&line.path, &source).map_err(said)?;
    OpenLine::of(&line, source)
}

/// Reads the line out of its source, collecting every fault. A missing
/// notation import comes first, since the other faults follow from it.
fn analyse(source: &str) -> (Option<Line>, Vec<BlackBox>, Vec<String>) {
    let missing = notation::missing_from(source)
        .then(notation::how_to_add_it)
        .into_iter();

    match Line::read(source) {
        Ok(line) => {
            let boxes = line.boxes();
            (Some(line), boxes, missing.collect())
        }
        Err(faults) => (
            None,
            Vec::new(),
            missing
                .chain(faults.into_iter().map(|fault| fault.to_string()))
                .collect(),
        ),
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
    use super::analyse;

    fn written(body: &str) -> String {
        format!("{}\n\n{body}", stemma_core::notation::import())
    }

    #[test]
    fn unfinished_source_remains_available_with_diagnostics() {
        let (line, boxes, diagnostics) =
            analyse(&written(r#"#theorem(central: true)[Unfinished.]"#));

        assert!(line.is_none());
        assert!(boxes.is_empty());
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn valid_source_keeps_its_apparatus() {
        let (line, _, diagnostics) = analyse(&written(
            r#"#theorem(id: "thm:main", central: true)[A statement.]"#,
        ));

        assert!(line.is_some());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn a_source_without_the_notation_import_is_told_what_it_lacks() {
        let (_, _, diagnostics) = analyse("= Hola\n\n#question[Adiós.]\n");

        assert!(
            diagnostics
                .iter()
                .any(|said| said.contains("where its environments come from")),
            "nothing said the import was missing: {diagnostics:?}"
        );
    }
}

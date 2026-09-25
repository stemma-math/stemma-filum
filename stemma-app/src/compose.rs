//! Typesetting a line.
//!
//! The line is rendered to Typst and compiled in memory; nothing is
//! written next to it, because the document is built from the line and
//! is not part of it. What Typst says is taken back to the block it is
//! about, so the editor can show it where it was written.

use std::path::Path;

use serde::Serialize;
use stemma_core::document::Document;
use stemma_core::id::BlockId;
use stemma_core::render::{self, Rendered};
use typst::diag::{Severity, SourceDiagnostic};
use typst::syntax::Source;
use typst_as_lib::{TypstAsLibError, TypstEngine, typst_kit_options::TypstKitFontOptions};
use typst_library::layout::PagedDocument;

use crate::said;
use crate::workspace::LineAt;

/// What composing a line gave.
#[derive(Serialize)]
pub struct Preview {
    /// The pages, when it compiled.
    svg: Option<String>,
    /// What Typst said, errors first.
    diagnostics: Vec<Diagnostic>,
}

/// One thing Typst said, placed where it was written.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    /// Whether it stopped the document from compiling.
    error: bool,
    /// The block it is about, when it is about one.
    block: Option<BlockId>,
    /// The line of that block's content, counting from one, when it is in it.
    line: Option<usize>,
    /// What Typst said, with its hints.
    message: String,
}

/// Typesets the editor's line, resolving files relative to the line.
#[tauri::command(async)]
pub fn preview(path: String, document: Document) -> Result<Preview, String> {
    let line = LineAt::admit(&path)?;
    compose(line.directory(), &document)
}

/// Typesets one line to SVG. The notation package is installed first (a
/// no-op when it is already there) so that the rendered import resolves.
fn compose(root: &Path, document: &Document) -> Result<Preview, String> {
    stemma::notation::install().map_err(said)?;

    let rendered = render::typst(document);
    let main = Source::detached(rendered.source.clone());
    let engine = TypstEngine::builder()
        .main_file(main.clone())
        .with_file_system_resolver(root)
        .search_fonts_with(
            TypstKitFontOptions::default()
                .include_system_fonts(true)
                .include_embedded_fonts(true),
        )
        .build();
    let compiled = engine.compile();
    let mut diagnostics: Vec<Diagnostic> = compiled
        .warnings
        .iter()
        .map(|warning| placed(&main, &rendered, warning))
        .collect();

    let svg = match compiled.output {
        Ok(pages) => {
            let pages: PagedDocument = pages;
            Some(
                pages
                    .pages
                    .iter()
                    .map(typst_svg::svg)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
        Err(TypstAsLibError::TypstSource(errors)) => {
            let mut errors: Vec<Diagnostic> = errors
                .iter()
                .map(|error| placed(&main, &rendered, error))
                .collect();
            errors.append(&mut diagnostics);
            diagnostics = errors;
            None
        }
        Err(other) => return Err(other.to_string()),
    };

    Ok(Preview { svg, diagnostics })
}

/// A Typst diagnostic, taken back to the block and line it is about.
fn placed(main: &Source, rendered: &Rendered, diagnostic: &SourceDiagnostic) -> Diagnostic {
    let hints: String = diagnostic
        .hints
        .iter()
        .map(|hint| format!("\n  {hint}"))
        .collect();
    let at = main.range(diagnostic.span).map(|range| range.start);
    let block = at.and_then(|at| rendered.block_at(at));
    let line = match (at, block) {
        (Some(at), Some(block)) if block.content.contains(&at) || at == block.content.end => Some(
            rendered.source[block.content.start..at]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1,
        ),
        _ => None,
    };

    Diagnostic {
        error: diagnostic.severity == Severity::Error,
        block: block.map(|block| block.id),
        line,
        message: format!("{}{hints}", diagnostic.message),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use stemma_core::document::{Block, Document};

    use super::compose;

    #[test]
    fn a_line_with_every_kind_of_block_composes() {
        let group = Block::definition(Some("def:group".parse().unwrap()), "A group is a set.")
            .titled("Group");
        let main = Block::theorem(
            "Lemma",
            Some("lem:one".parse().unwrap()),
            "By @def:group, $1 = 1$.",
        )
        .central(true)
        .using(&group);
        let proof = Block::proof(*main.id(), "");
        let document = Document::new("Things", Some("What a thing is.".to_owned()))
            .with(Block::prose("We begin."))
            .with(group)
            .with(main)
            .with(proof);

        let composed = compose(Path::new("."), &document).expect("a document");

        assert!(composed.svg.is_some_and(|svg| svg.contains("<svg")));
        assert_eq!(composed.diagnostics, Vec::new());
    }

    #[test]
    fn an_error_is_placed_in_the_block_and_line_it_was_written_on() {
        let broken = Block::prose("Fine.\nAlso fine.\n#nonexistent");
        let id = *broken.id();
        let document = Document::new("Things", None)
            .with(Block::prose("Before."))
            .with(broken);

        let composed = compose(Path::new("."), &document).expect("an answer");

        assert!(composed.svg.is_none());
        let first = &composed.diagnostics[0];
        assert!(first.error);
        assert_eq!(first.block, Some(id));
        assert_eq!(first.line, Some(3));
        assert!(first.message.contains("nonexistent"), "{}", first.message);
    }
}

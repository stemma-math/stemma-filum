//! Typesetting a line: the pages the editor previews, and the page every
//! revision carries to the instance.
//!
//! The line is rendered to Typst and compiled in memory; nothing is
//! written next to it, because the document is built from the line and
//! is not part of it. What Typst says is taken back to the block it is
//! about, so whoever wrote it can see where.
//!
//! The instance never compiles anything, so a revision arrives with its
//! page already made, as HTML. A line that does not typeset yet is still
//! a revision somebody wrote, and its page says so rather than being
//! left out: every revision has one.

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use serde::Serialize;
use stemma_core::document::Document;
use stemma_core::id::BlockId;
use stemma_core::render::{self, Rendered};
use stemma_core::repository::Pins;
use typst::diag::{Severity, SourceDiagnostic};
use typst::syntax::Source;
use typst_as_lib::{TypstAsLibError, TypstEngine, typst_kit_options::TypstKitFontOptions};
use typst_html::HtmlDocument;
use typst_library::layout::PagedDocument;

/// The Typst release this build carries, and typesets with.
pub const TYPST: &str = "0.14.0";

/// What composing a line gave, for the editor.
#[derive(Debug, Serialize)]
pub struct Preview {
    /// The pages, when it compiled.
    pub svg: Option<String>,
    /// What Typst said, errors first.
    pub diagnostics: Vec<Diagnostic>,
}

/// One thing Typst said, placed where it was written.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    /// Whether it stopped the document from compiling.
    pub error: bool,
    /// The block it is about, when it is about one.
    pub block: Option<BlockId>,
    /// The line of that block's content, counting from one, when it is in it.
    pub line: Option<usize>,
    /// What Typst said, with its hints.
    pub message: String,
}

/// Whether this build may typeset a group's lines: only with the release
/// the group pinned, when it pinned one.
pub fn may_typeset(pins: &Pins) -> Result<()> {
    match pins.typst.as_deref() {
        Some(pinned) if pinned != TYPST => bail!(
            "typst {pinned} is pinned for this group and this build typesets with typst {TYPST}"
        ),
        _ => Ok(()),
    }
}

/// Typesets a line to SVG pages, resolving files relative to `root`.
pub fn preview(root: &Path, document: &Document) -> Result<Preview> {
    let rendered = render::typst(document);
    let (compiled, main) = compile::<PagedDocument>(root, &rendered)?;

    let (output, mut diagnostics) = placed_all(&main, &rendered, compiled)?;
    let svg = output.map(|pages| {
        pages
            .pages
            .iter()
            .map(typst_svg::svg)
            .collect::<Vec<_>>()
            .join("\n")
    });
    diagnostics.sort_by_key(|diagnostic| !diagnostic.error);

    Ok(Preview { svg, diagnostics })
}

/// The page a revision carries: the line as HTML, or, when it does not
/// typeset yet, a page that says what is in the way.
pub fn page(root: &Path, document: &Document) -> Result<String> {
    let rendered = render::typst(document);
    let (compiled, main) = compile::<HtmlDocument>(root, &rendered)?;
    let (output, diagnostics) = placed_all(&main, &rendered, compiled)?;

    if let Some(html) = output.and_then(|document| typst_html::html(&document).ok()) {
        return Ok(html);
    }

    Ok(unfinished(document, &diagnostics))
}

/// Compiles the rendered line into whichever kind of document is wanted.
fn compile<D: typst::Document>(
    root: &Path,
    rendered: &Rendered,
) -> Result<(typst::diag::Warned<Result<D, TypstAsLibError>>, Source)> {
    crate::typeset::package::install()?;

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

    Ok((engine.compile(), main))
}

/// What compiling gave, with every diagnostic taken back to its block.
fn placed_all<D>(
    main: &Source,
    rendered: &Rendered,
    compiled: typst::diag::Warned<Result<D, TypstAsLibError>>,
) -> Result<(Option<D>, Vec<Diagnostic>)> {
    let mut diagnostics: Vec<Diagnostic> = compiled
        .warnings
        .iter()
        .map(|warning| placed(main, rendered, warning))
        .collect();

    match compiled.output {
        Ok(document) => Ok((Some(document), diagnostics)),
        Err(TypstAsLibError::TypstSource(errors)) => {
            let mut errors: Vec<Diagnostic> = errors
                .iter()
                .map(|error| placed(main, rendered, error))
                .collect();
            errors.append(&mut diagnostics);
            Ok((None, errors))
        }
        Err(other) => Err(anyhow!("{other}")),
    }
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

/// The page of a line that does not typeset yet: its title and what stops it.
fn unfinished(document: &Document, diagnostics: &[Diagnostic]) -> String {
    let errors: String = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.error)
        .map(|diagnostic| format!("<li>{}</li>", escaped(&diagnostic.message)))
        .collect();

    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title></head>\
         <body><main class=\"stemma-line\"><h1>{title}</h1>\
         <p>This revision does not typeset yet.</p><ul>{errors}</ul></main></body></html>\n",
        title = escaped(&document.title),
    )
}

/// Text as it can stand in HTML.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use stemma_core::document::{Block, Document};

    use super::*;

    fn every_kind() -> Document {
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

        Document::new("Things", Some("What a thing is.".to_owned()))
            .with(Block::prose("We begin."))
            .with(group)
            .with(main)
            .with(proof)
    }

    #[test]
    fn a_line_with_every_kind_of_block_composes() {
        let composed = preview(Path::new("."), &every_kind()).expect("a document");

        assert!(composed.svg.is_some_and(|svg| svg.contains("<svg")));
        assert!(
            composed
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.error),
            "{:?}",
            composed.diagnostics
        );
    }

    #[test]
    fn a_line_becomes_a_page_of_html() {
        let document = every_kind();
        let page = page(Path::new("."), &document).expect("a page");

        assert!(page.contains("<html"), "{page}");
        assert!(page.contains("We begin."), "{page}");
        assert!(page.contains("A group is a set."), "{page}");
        assert!(page.contains("Proof."), "{page}");
        assert!(page.contains("<main class=\"stemma-line\""), "{page}");
        for block in &document.blocks {
            assert!(
                page.contains(&format!("<section id=\"{}\"", block.id())),
                "{page}"
            );
        }
        assert!(page.contains("<svg"), "equations use inline SVG: {page}");
        assert!(
            page.contains("<span style=\"display: inline-block\"><svg"),
            "an inline equation stays in its sentence: {page}"
        );
        assert!(page.contains("stemma-line__siglum"), "{page}");
        assert!(page.contains("stemma-line__qed"), "{page}");
        assert!(page.contains("stemma-line__block--central"), "{page}");
        assert!(!page.contains("does not typeset yet"), "{page}");
    }

    #[test]
    fn an_error_is_placed_in_the_block_and_line_it_was_written_on() {
        let broken = Block::prose("Fine.\nAlso fine.\n#nonexistent");
        let id = *broken.id();
        let document = Document::new("Things", None)
            .with(Block::prose("Before."))
            .with(broken);

        let composed = preview(Path::new("."), &document).expect("an answer");

        assert!(composed.svg.is_none());
        let first = &composed.diagnostics[0];
        assert!(first.error);
        assert_eq!(first.block, Some(id));
        assert_eq!(first.line, Some(3));
        assert!(first.message.contains("nonexistent"), "{}", first.message);
    }

    #[test]
    fn a_line_that_does_not_typeset_still_has_a_page_that_says_why() {
        let document = Document::new("Things <b>", None).with(Block::prose("#nonexistent"));

        let page = page(Path::new("."), &document).expect("a page");

        assert!(page.contains("does not typeset yet"));
        assert!(page.contains("nonexistent"));
        assert!(!page.contains("<pre>"));
        assert!(
            page.contains("Things &lt;b&gt;"),
            "the title is text, not markup"
        );
    }

    #[test]
    fn a_group_pinned_to_another_typst_is_not_typeset_here() {
        let mut pins = Pins::default();
        assert!(may_typeset(&pins).is_ok(), "nothing pinned");

        pins.typst = Some(TYPST.to_owned());
        assert!(may_typeset(&pins).is_ok());

        pins.typst = Some("0.11.0".to_owned());
        assert!(may_typeset(&pins).is_err());
    }
}

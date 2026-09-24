//! Typesetting a line and laying out its descent.

use std::path::Path;

use serde::Serialize;
use stemma_core::line::{Line, Siglum};
use typst::diag::SourceDiagnostic;
use typst::syntax::Source;
use typst_as_lib::{TypstAsLibError, TypstEngine, typst_kit_options::TypstKitFontOptions};
use typst_library::layout::PagedDocument;

use crate::said;
use crate::workspace::LineAt;

#[derive(Serialize)]
pub struct Preview {
    svg: String,
    diagnostics: Vec<String>,
}

/// Typesets the editor's text, resolving imports relative to the line.
#[tauri::command(async)]
pub fn preview(path: String, source: String) -> Result<Preview, String> {
    let line = LineAt::admit(&path)?;
    compose(line.directory(), source)
}

/// The descent placement of the editor's text, or `null` when the source
/// does not read as a line. The same placement the instance's pages draw.
#[tauri::command(async)]
pub fn descent(
    source: String,
    verified: Vec<String>,
    bridges: Vec<Vec<String>>,
) -> Result<serde_json::Value, String> {
    let Ok(line) = Line::read(&source) else {
        return Ok(serde_json::Value::Null);
    };

    let verified: Vec<Siglum> = verified.iter().filter_map(|at| at.parse().ok()).collect();
    let spanned: Vec<(Siglum, Siglum)> = bridges
        .iter()
        .filter_map(|pair| match pair.as_slice() {
            [left, right] => Some((left.parse().ok()?, right.parse().ok()?)),
            _ => None,
        })
        .collect();

    serde_json::to_value(stemma_core::descent::of(&line, &verified, &spanned)).map_err(said)
}

/// Typesets one source to SVG. The notation package is installed first
/// (a no-op when it is already there) so that the import resolves.
fn compose(root: &Path, source: String) -> Result<Preview, String> {
    stemma::notation::install().map_err(said)?;

    let main = Source::detached(source);
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
    let diagnostics = compiled
        .warnings
        .iter()
        .map(|warning| complaint(&main, warning))
        .collect();
    let document: PagedDocument = compiled.output.map_err(|error| refused(&main, &error))?;
    let svg = document
        .pages
        .iter()
        .map(typst_svg::svg)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Preview { svg, diagnostics })
}

/// A Typst diagnostic as `line L, column C: message` followed by its hints,
/// instead of a byte span.
fn complaint(main: &Source, diagnostic: &SourceDiagnostic) -> String {
    let at = main
        .range(diagnostic.span)
        .and_then(|range| main.lines().byte_to_line_column(range.start))
        .map(|(line, column)| format!("line {}, column {}: ", line + 1, column + 1))
        .unwrap_or_default();
    let hints: String = diagnostic
        .hints
        .iter()
        .map(|hint| format!("\n  {hint}"))
        .collect();

    format!("{at}{}{hints}", diagnostic.message)
}

/// Why the document did not compile. When the notation import is missing
/// that is said too, since "unknown variable: theorem" does not explain it.
fn refused(main: &Source, error: &TypstAsLibError) -> String {
    let said = match error {
        TypstAsLibError::TypstSource(diagnostics) => diagnostics
            .iter()
            .map(|diagnostic| complaint(main, diagnostic))
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    };

    match stemma_core::notation::missing_from(main.text()) {
        true => format!("{said}\n\n{}", stemma_core::notation::how_to_add_it()),
        false => said,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Source, SourceDiagnostic, complaint, compose};

    #[test]
    fn a_line_written_the_way_the_instance_starts_one_composes() {
        let source = format!(
            "{}\n\n= Things\n\n#question[What is a thing?]\n\n\
             #definition(id: \"def:thing\", name: \"Thing\")[A thing is a thing.]\n\n\
             #theorem(id: \"thm:one\", central: true, uses: \"def:thing\")[Every thing is one.]\n\n\
             #proof(of: \"thm:one\")[Immediate.]\n",
            stemma_core::notation::import()
        );

        let composed = compose(Path::new("."), source).expect("a document");

        assert!(composed.svg.contains("<svg"));
        assert_eq!(composed.diagnostics, Vec::<String>::new());
    }

    #[test]
    fn a_diagnostic_names_its_line_and_column_and_keeps_its_hints() {
        let text = "= Things\n\n#question[What is a thing?]\n";
        let main = Source::detached(text.to_owned());
        let at = text.find("question").unwrap();
        let node = typst::syntax::LinkedNode::new(main.root())
            .leaf_at(at + 1, typst::syntax::Side::Before)
            .unwrap();
        let diagnostic = SourceDiagnostic::error(node.span(), "unknown variable: question")
            .with_hint("nothing here defines it");

        let said = complaint(&main, &diagnostic);

        assert_eq!(
            said,
            "line 3, column 2: unknown variable: question\n  nothing here defines it"
        );
    }
}

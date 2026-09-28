//! Typesetting a line for the editor's preview.
//!
//! The typesetting itself is the client library's, shared with the page
//! every revision carries; this only finds the line the webview means.

use stemma::render::Preview;
use stemma_core::document::Document;

use crate::failed;
use crate::workspace::LineAt;

/// Typesets the editor's line, resolving files relative to the line.
#[tauri::command(async)]
pub fn preview(path: String, document: Document) -> Result<Preview, String> {
    let line = LineAt::admit(&path)?;
    stemma::render::preview(line.directory(), &document).map_err(failed)
}

/// Typesets an HTML page with block addresses for the review overlay.
#[tauri::command(async)]
pub fn review_page(path: String, document: Document) -> Result<String, String> {
    let line = LineAt::admit(&path)?;
    let page = stemma::render::page(line.directory(), &document).map_err(failed)?;
    let style = format!(
        "<style>{}\n{}</style>",
        stemma_identity::TOKENS_CSS,
        include_str!("../../stemma-server/assets/line.css"),
    );
    Ok(page.replacen("</head>", &format!("{style}</head>"), 1))
}

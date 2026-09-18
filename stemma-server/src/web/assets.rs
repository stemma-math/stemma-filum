//! What the pages load besides themselves.
//!
//! Everything is served from the instance's own origin. A stylesheet or a
//! face fetched from somewhere else would tell that somewhere else who is
//! reading a research group's pages.

use std::sync::LazyLock;

use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

/// Where the stylesheet is served.
pub const STYLESHEET_PATH: &str = "/assets/stemma.css";

/// The stylesheet itself.
const STYLESHEET: &str = include_str!("../../assets/stemma.css");

/// What this build's stylesheet is, so a browser can ask whether it has
/// changed instead of fetching it again.
static STYLESHEET_TAG: LazyLock<String> = LazyLock::new(|| tag_of(STYLESHEET));

/// The entity tag of a file this build serves.
fn tag_of(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let short: String = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();

    format!("\"{short}\"")
}

/// Serves the stylesheet.
pub async fn stylesheet(headers: HeaderMap) -> Response {
    serve(
        &headers,
        STYLESHEET,
        "text/css; charset=utf-8",
        &STYLESHEET_TAG,
    )
}

/// Answers with the file, or with "you already have it".
fn serve(headers: &HeaderMap, body: &'static str, content_type: &str, tag: &str) -> Response {
    let known = headers
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == tag));

    let head = [
        (CONTENT_TYPE, content_type.to_owned()),
        (ETAG, tag.to_owned()),
        (
            CACHE_CONTROL,
            "public, max-age=60, must-revalidate".to_owned(),
        ),
    ];

    if known {
        return (StatusCode::NOT_MODIFIED, head).into_response();
    }

    (head, body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tag_changes_with_the_file_and_not_otherwise() {
        assert_eq!(tag_of("body {}"), tag_of("body {}"));
        assert_ne!(tag_of("body {}"), tag_of("body { color: red }"));
        assert!(tag_of("body {}").starts_with('"'));
    }

    #[test]
    fn the_stylesheet_is_built_into_the_binary() {
        assert!(STYLESHEET.contains("--ditto"));
        assert!(!STYLESHEET_TAG.is_empty());
    }
}

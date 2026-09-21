//! What the pages load besides themselves.
//!
//! Everything is served from the instance's own origin. A stylesheet or a
//! face fetched from somewhere else would tell that somewhere else who is
//! reading a research group's pages.

use std::sync::LazyLock;

use axum::extract::Path;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

/// Where the stylesheet is served.
pub const STYLESHEET_PATH: &str = "/assets/stemma.css";

/// Where the faces are served, one path segment below.
pub const FONT_PATH: &str = "/assets/fonts/{file}";

/// Where the shared descent drawing is served.
pub const DESCENT_PATH: &str = "/assets/descent.js";

/// Where the small surface-transition script is served.
pub const SURFACE_PATH: &str = "/assets/surface.js";

/// Where the subtle surface texture is served.
pub const NOISE_PATH: &str = "/assets/noise.svg";

/// The stylesheet itself.
const STYLESHEET: &str = include_str!("../../assets/stemma.css");

/// The drawing, which is the only script the web serves.
///
/// Everything else is rendered here; this draws the one thing a server
/// cannot render as text without lying about it, and the page it is on
/// keeps the list it would otherwise have shown. The application draws
/// its graph with the same file, so that one change to the trace
/// changes both.
const DESCENT: &str = include_str!("../../../assets/descent.js");

/// The front-door transition, kept deliberately independent of page logic.
const SURFACE: &str = include_str!("../../assets/surface.js");

/// The texture laid over blue product surfaces.
const NOISE: &str = include_str!("../../assets/noise.svg");

/// The faces the design names, subset to the Latin these pages set.
///
/// They are built into the binary like the stylesheet, so an instance
/// serves its own typography and a reader is never sent elsewhere for it.
const FONTS: &[(&str, &[u8])] = &[
    (
        "libertinus-serif-400.woff2",
        include_bytes!("../../assets/fonts/libertinus-serif-400.woff2"),
    ),
    (
        "libertinus-serif-400-italic.woff2",
        include_bytes!("../../assets/fonts/libertinus-serif-400-italic.woff2"),
    ),
    (
        "libertinus-serif-700.woff2",
        include_bytes!("../../assets/fonts/libertinus-serif-700.woff2"),
    ),
    (
        "archivo-400.woff2",
        include_bytes!("../../assets/fonts/archivo-400.woff2"),
    ),
    (
        "archivo-400-italic.woff2",
        include_bytes!("../../assets/fonts/archivo-400-italic.woff2"),
    ),
    (
        "archivo-500.woff2",
        include_bytes!("../../assets/fonts/archivo-500.woff2"),
    ),
    (
        "archivo-600.woff2",
        include_bytes!("../../assets/fonts/archivo-600.woff2"),
    ),
    (
        "archivo-narrow-400.woff2",
        include_bytes!("../../assets/fonts/archivo-narrow-400.woff2"),
    ),
    (
        "iosevka-400.woff2",
        include_bytes!("../../assets/fonts/iosevka-400.woff2"),
    ),
];

/// What this build's stylesheet is, so a browser can ask whether it has
/// changed instead of fetching it again.
static STYLESHEET_TAG: LazyLock<String> = LazyLock::new(|| tag_of(STYLESHEET.as_bytes()));

/// The same, for the drawing.
static DESCENT_TAG: LazyLock<String> = LazyLock::new(|| tag_of(DESCENT.as_bytes()));

/// The same, for the surface transition.
static SURFACE_TAG: LazyLock<String> = LazyLock::new(|| tag_of(SURFACE.as_bytes()));

/// The same, for the surface texture.
static NOISE_TAG: LazyLock<String> = LazyLock::new(|| tag_of(NOISE.as_bytes()));

/// The same, for each face.
static FONT_TAGS: LazyLock<Vec<String>> =
    LazyLock::new(|| FONTS.iter().map(|(_, face)| tag_of(face)).collect());

/// The entity tag of a file this build serves.
fn tag_of(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
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
        STYLESHEET.as_bytes(),
        "text/css; charset=utf-8",
        &STYLESHEET_TAG,
    )
}

/// Serves the drawing.
pub async fn descent(headers: HeaderMap) -> Response {
    serve(
        &headers,
        DESCENT.as_bytes(),
        "text/javascript; charset=utf-8",
        &DESCENT_TAG,
    )
}

/// Serves the front-door transition.
pub async fn surface(headers: HeaderMap) -> Response {
    serve(
        &headers,
        SURFACE.as_bytes(),
        "text/javascript; charset=utf-8",
        &SURFACE_TAG,
    )
}

/// Serves the surface texture.
pub async fn noise(headers: HeaderMap) -> Response {
    serve(
        &headers,
        NOISE.as_bytes(),
        "image/svg+xml; charset=utf-8",
        &NOISE_TAG,
    )
}

/// Serves one face.
pub async fn font(Path(file): Path<String>, headers: HeaderMap) -> Response {
    let Some(index) = FONTS.iter().position(|(name, _)| *name == file) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    serve(&headers, FONTS[index].1, "font/woff2", &FONT_TAGS[index])
}

/// Answers with the file, or with "you already have it".
fn serve(headers: &HeaderMap, body: &'static [u8], content_type: &str, tag: &str) -> Response {
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
        assert_eq!(tag_of(b"body {}"), tag_of(b"body {}"));
        assert_ne!(tag_of(b"body {}"), tag_of(b"body { color: red }"));
        assert!(tag_of(b"body {}").starts_with('"'));
    }

    #[test]
    fn the_stylesheet_is_built_into_the_binary() {
        assert!(STYLESHEET.contains("--ditto"));
        assert!(!STYLESHEET_TAG.is_empty());
    }

    #[test]
    fn every_face_the_stylesheet_asks_for_is_one_the_instance_has() {
        for (name, face) in FONTS {
            assert!(
                STYLESHEET.contains(&format!("/assets/fonts/{name}")),
                "{name} is built in and nothing asks for it"
            );
            assert!(face.starts_with(b"wOF2"), "{name} is not a woff2 file");
        }

        for asked in STYLESHEET.split("/assets/fonts/").skip(1) {
            let name = asked.split('"').next().unwrap_or_default();

            assert!(
                FONTS.iter().any(|(built, _)| *built == name),
                "the stylesheet asks for {name} and the instance has no such face"
            );
        }
    }
}

//! What the pages load besides themselves.
//!
//! Everything is served from the instance's own origin, so reading a group's
//! pages never tells a third party who is reading them. Tokens, faces, the
//! mark, the icons and the texture come from `stemma-identity`.

use std::sync::LazyLock;

use axum::extract::Path;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

/// Where the stylesheet is served.
pub const STYLESHEET_PATH: &str = "/assets/stemma.css";

/// Where the faces are served. The stylesheet's `fonts/<file>` URLs resolve
/// here because both live under `/assets/`.
pub const FONT_PATH: &str = "/assets/fonts/{file}";

/// Where the front page's light and its passage to the sign-in are served.
pub const LANDING_PATH: &str = "/assets/landing.js";

/// Where the live mark is served.
pub const MARK_ANIMATED_PATH: &str = "/assets/mark-animated.js";

/// Where the still of the mark is served.
pub const SYMBOL_PATH: &str = "/assets/symbol.webp";

/// Where the signed-in shell's script is served.
pub const SHELL_PATH: &str = "/assets/shell.js";

/// Where the line glyphs are served.
pub const GLYPHS_PATH: &str = "/assets/glyphs.svg";

/// Where the surface texture is served.
pub const NOISE_PATH: &str = "/assets/noise.svg";

/// Where the mark for dark surfaces is served.
pub const MARK_ON_DARK_PATH: &str = "/assets/mark-on-dark.svg";

/// Where the mark for light surfaces is served.
pub const MARK_ON_LIGHT_PATH: &str = "/assets/mark-on-light.svg";

/// Where the web app manifest is served. Browsers look for it, and for the
/// icons it names, at the root of the site.
pub const MANIFEST_PATH: &str = "/site.webmanifest";

/// A file this build serves, with the entity tag that lets a browser ask
/// whether it changed.
struct Asset {
    body: &'static [u8],
    content_type: &'static str,
    tag: String,
}

impl Asset {
    fn new(body: &'static [u8], content_type: &'static str) -> Self {
        Self {
            body,
            content_type,
            tag: tag_of(body),
        }
    }
}

const CSS: &str = "text/css; charset=utf-8";
const JAVASCRIPT: &str = "text/javascript; charset=utf-8";
const SVG: &str = "image/svg+xml; charset=utf-8";
const WOFF2: &str = "font/woff2";
const PNG: &str = "image/png";
const WEBP: &str = "image/webp";
const ICO: &str = "image/x-icon";
const MANIFEST: &str = "application/manifest+json";

/// The identity's faces, tokens and components, then the web interface's own
/// rules.
static STYLESHEET: LazyLock<Asset> = LazyLock::new(|| {
    let joined = [
        stemma_identity::FONTS_CSS,
        stemma_identity::TOKENS_CSS,
        stemma_identity::COMPONENTS_CSS,
        include_str!("../../assets/stemma.css"),
    ]
    .join("\n");

    Asset::new(joined.leak().as_bytes(), CSS)
});

static LANDING: LazyLock<Asset> =
    LazyLock::new(|| Asset::new(include_bytes!("../../assets/landing.js"), JAVASCRIPT));

static MARK_ANIMATED: LazyLock<Asset> =
    LazyLock::new(|| Asset::new(stemma_identity::MARK_ANIMATED_JS.as_bytes(), JAVASCRIPT));

static SYMBOL: LazyLock<Asset> = LazyLock::new(|| Asset::new(stemma_identity::SYMBOL_WEBP, WEBP));

static SHELL: LazyLock<Asset> =
    LazyLock::new(|| Asset::new(include_bytes!("../../assets/shell.js"), JAVASCRIPT));

static GLYPHS: LazyLock<Asset> =
    LazyLock::new(|| Asset::new(stemma_identity::GLYPHS_SVG.as_bytes(), SVG));

static NOISE: LazyLock<Asset> =
    LazyLock::new(|| Asset::new(stemma_identity::NOISE_SVG.as_bytes(), SVG));

static MARK_ON_DARK: LazyLock<Asset> =
    LazyLock::new(|| Asset::new(stemma_identity::MARK_ON_DARK_SVG.as_bytes(), SVG));

static MARK_ON_LIGHT: LazyLock<Asset> =
    LazyLock::new(|| Asset::new(stemma_identity::MARK_ON_LIGHT_SVG.as_bytes(), SVG));

static ICONS: LazyLock<Vec<(&str, Asset)>> = LazyLock::new(|| {
    stemma_identity::WEB_ICONS
        .iter()
        .map(|(name, icon)| {
            let kind = match name.rsplit('.').next() {
                Some("svg") => SVG,
                Some("ico") => ICO,
                _ => PNG,
            };
            (*name, Asset::new(icon, kind))
        })
        .collect()
});

/// What a browser needs to install the site or pin it to a home screen.
static WEB_MANIFEST: LazyLock<Asset> = LazyLock::new(|| {
    let manifest = serde_json::json!({
        "name": "Stemma",
        "icons": [
            { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any maskable" },
            { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any maskable" },
        ],
        "theme_color": stemma_identity::THEME_COLOUR,
        "background_color": stemma_identity::THEME_COLOUR,
    });

    Asset::new(manifest.to_string().leak().as_bytes(), MANIFEST)
});

static FONTS: LazyLock<Vec<(&str, Asset)>> = LazyLock::new(|| {
    stemma_identity::FONTS
        .iter()
        .map(|(name, face)| (*name, Asset::new(face, WOFF2)))
        .collect()
});

/// The entity tag of a file: the first eight bytes of its SHA-256, quoted.
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
    serve(&headers, &STYLESHEET)
}

/// Serves the front page's light.
pub async fn landing(headers: HeaderMap) -> Response {
    serve(&headers, &LANDING)
}

/// Serves the live mark.
pub async fn mark_animated(headers: HeaderMap) -> Response {
    serve(&headers, &MARK_ANIMATED)
}

/// Serves the still of the mark.
pub async fn symbol(headers: HeaderMap) -> Response {
    serve(&headers, &SYMBOL)
}

/// Serves the signed-in shell's script.
pub async fn shell(headers: HeaderMap) -> Response {
    serve(&headers, &SHELL)
}

/// Serves the line glyphs.
pub async fn glyphs(headers: HeaderMap) -> Response {
    serve(&headers, &GLYPHS)
}

/// Serves the surface texture.
pub async fn noise(headers: HeaderMap) -> Response {
    serve(&headers, &NOISE)
}

/// Serves the mark for dark surfaces.
pub async fn mark_on_dark(headers: HeaderMap) -> Response {
    serve(&headers, &MARK_ON_DARK)
}

/// Serves the mark for light surfaces.
pub async fn mark_on_light(headers: HeaderMap) -> Response {
    serve(&headers, &MARK_ON_LIGHT)
}

/// Serves the web app manifest.
pub async fn manifest(headers: HeaderMap) -> Response {
    serve(&headers, &WEB_MANIFEST)
}

/// Serves one of the site's icons, by the name it is served under.
pub fn icon(name: &str, headers: &HeaderMap) -> Response {
    match ICONS.iter().find(|(served, _)| *served == name) {
        Some((_, icon)) => serve(headers, icon),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Serves one face.
pub async fn font(Path(file): Path<String>, headers: HeaderMap) -> Response {
    match FONTS.iter().find(|(name, _)| *name == file) {
        Some((_, face)) => serve(&headers, face),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Answers with the file, or with 304 when the browser already has it.
fn serve(headers: &HeaderMap, asset: &Asset) -> Response {
    let known = headers
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|candidate| candidate.trim() == asset.tag)
        });

    let head = [
        (CONTENT_TYPE, asset.content_type.to_owned()),
        (ETAG, asset.tag.clone()),
        (
            CACHE_CONTROL,
            "public, max-age=60, must-revalidate".to_owned(),
        ),
    ];

    if known {
        return (StatusCode::NOT_MODIFIED, head).into_response();
    }

    (head, asset.body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stylesheet() -> &'static str {
        std::str::from_utf8(STYLESHEET.body).unwrap()
    }

    #[test]
    fn the_tag_changes_with_the_file_and_not_otherwise() {
        assert_eq!(tag_of(b"body {}"), tag_of(b"body {}"));
        assert_ne!(tag_of(b"body {}"), tag_of(b"body { color: red }"));
        assert!(tag_of(b"body {}").starts_with('"'));
    }

    #[test]
    fn the_stylesheet_carries_the_identity() {
        assert!(stylesheet().contains("--accent"));
        assert!(stylesheet().contains("@font-face"));
    }

    #[test]
    fn every_face_the_stylesheet_asks_for_is_one_the_instance_has() {
        for (name, face) in FONTS.iter() {
            assert!(
                stylesheet().contains(&format!("fonts/{name}")),
                "{name} is built in and nothing asks for it"
            );
            assert!(face.body.starts_with(b"wOF2"), "{name} is not a woff2 file");
        }

        for asked in stylesheet().split("url(\"fonts/").skip(1) {
            let name = asked.split('"').next().unwrap_or_default();

            assert!(
                FONTS.iter().any(|(built, _)| *built == name),
                "the stylesheet asks for {name} and the instance has no such face"
            );
        }
    }
}

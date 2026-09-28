//! Stemma's visual identity, embedded for Rust consumers.
//!
//! The files in this crate's directory are the single source of the design
//! tokens, the shared components, the faces, the mark, the icons and the
//! descent drawing:
//!
//! - `tokens/`: `tokens.json`, and `tokens.css` and `tokens.typ` generated
//!   from it by `tokens/build.mjs`;
//! - `css/`: the faces' rules and the shared components;
//! - `fonts/`: the faces, each beside its licence;
//! - `mark/`: the mark, drawn still and live;
//! - `images/`: the line glyphs and the surface texture;
//! - `js/`: the descent drawing;
//! - `icons/`: the application's icons (`app/`) and the site's (`web/`).
//!
//! The server embeds them through the constants here; the desktop
//! application imports the same files through `package.json`, under the
//! flat names its `exports` give them.

/// The faces' `@font-face` rules. URLs are relative (`../fonts/<file>`), so
/// the stylesheet must be served from a directory beside the one that serves
/// [`FONTS`].
pub const FONTS_CSS: &str = include_str!("../css/fonts.css");

/// The design tokens, as custom properties.
pub const TOKENS_CSS: &str = include_str!("../tokens/tokens.css");

/// The shared components' classes, in cascade layers. Needs [`TOKENS_CSS`].
pub const COMPONENTS_CSS: &str = include_str!("../css/components.css");

/// The mark for dark surfaces: the navy sidebars. Its gradients carry ids,
/// so it is shown as an image rather than inlined more than once a page.
pub const MARK_ON_DARK_SVG: &str = include_str!("../mark/on-dark.svg");

/// The mark for light surfaces: sign-in headers, mail, documentation.
pub const MARK_ON_LIGHT_SVG: &str = include_str!("../mark/on-light.svg");

/// The line glyphs, as a sprite of symbols named by id.
pub const GLYPHS_SVG: &str = include_str!("../images/glyphs.svg");

/// The mark drawn live, for the front page alone: a classic script that
/// defines `StemmaLogo`.
pub const MARK_ANIMATED_JS: &str = include_str!("../mark/animated.js");

/// A still of the mark, where it cannot be drawn live.
pub const SYMBOL_WEBP: &[u8] = include_bytes!("../mark/symbol.webp");

/// The surface texture laid over blue product surfaces.
pub const NOISE_SVG: &str = include_str!("../images/noise.svg");

/// The descent drawing: an ES module exporting `draw` and `start`, which
/// also draws every `.descent` placement on the page when loaded.
pub const DESCENT_JS: &str = include_str!("../js/descent.js");

/// The colour a browser paints around the product before it draws: the
/// navy of the front page.
pub const THEME_COLOUR: &str = "#030e29";

/// The files a browser asks a site for to show it in a tab, on a home
/// screen or as an installed app, by the name each is served under.
pub const WEB_ICONS: &[(&str, &[u8])] = &[
    ("favicon.svg", include_bytes!("../icons/web/favicon.svg")),
    ("favicon.ico", include_bytes!("../icons/web/favicon.ico")),
    (
        "apple-touch-icon.png",
        include_bytes!("../icons/web/apple-touch-icon.png"),
    ),
    ("icon-192.png", include_bytes!("../icons/web/icon-192.png")),
    ("icon-512.png", include_bytes!("../icons/web/icon-512.png")),
];

macro_rules! face {
    ($file:literal) => {
        ($file, include_bytes!(concat!("../fonts/", $file)))
    };
}
/// Every face, by the file name `fonts.css` refers to it by.
pub const FONTS: &[(&str, &[u8])] = &[
    face!("libertinus-serif-400.woff2"),
    face!("libertinus-serif-400-italic.woff2"),
    face!("libertinus-serif-700.woff2"),
    face!("instrument-sans-400.woff2"),
    face!("instrument-sans-400-italic.woff2"),
    face!("instrument-sans-500.woff2"),
    face!("instrument-sans-600.woff2"),
    face!("inter-variable.woff2"),
    face!("jetbrains-mono-400.woff2"),
    face!("jetbrains-mono-500.woff2"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_the_stylesheet_names_is_embedded() {
        let named: Vec<&str> = FONTS_CSS
            .split("url(\"../fonts/")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect();

        assert_eq!(named.len(), FONTS.len());
        for file in named {
            assert!(
                FONTS.iter().any(|(name, _)| *name == file),
                "{file} is missing"
            );
        }
    }

    #[test]
    fn the_theme_colour_is_the_front_page_navy() {
        assert!(TOKENS_CSS.contains(&format!("--navy-950: {THEME_COLOUR};")));
    }
}

//! Stemma's visual identity, embedded for Rust consumers.
//!
//! The files in this crate's directory are the single source of the design
//! tokens, the shared components, the faces, the mark and the descent drawing.
//! The tokens are written in `tokens.json`, from which `tokens.css` and
//! `tokens.typ` are generated. The server embeds them through the constants
//! here; the desktop application imports the same files through
//! `package.json`.

/// The faces' `@font-face` rules. URLs are relative (`fonts/<file>`), so the
/// stylesheet must be served from the directory that also serves [`FONTS`].
pub const FONTS_CSS: &str = include_str!("../fonts.css");

/// The design tokens, as custom properties.
pub const TOKENS_CSS: &str = include_str!("../tokens.css");

/// The shared components' classes, in cascade layers. Needs [`TOKENS_CSS`].
pub const COMPONENTS_CSS: &str = include_str!("../components.css");

/// The mark, as inline SVG carrying the `brand-mark` classes.
pub const MARK_SVG: &str = include_str!("../mark.svg");

/// The surface texture laid over blue product surfaces.
pub const NOISE_SVG: &str = include_str!("../noise.svg");

/// The descent drawing: an ES module exporting `draw` and `start`, which
/// also draws every `.descent` placement on the page when loaded.
pub const DESCENT_JS: &str = include_str!("../descent.js");

/// Centre of the mark's 64-unit box, on both axes.
pub const MARK_CENTRE: f64 = 32.0;

/// Radius of the circle the mark's three nodes sit on.
pub const MARK_ORBIT: f64 = 24.0;

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

    /// The centre of every node in the mark, relative to the box's centre.
    fn nodes() -> Vec<(f64, f64)> {
        MARK_SVG
            .split("<circle")
            .skip(1)
            .map(|node| {
                let value = |name: &str| -> f64 {
                    node.split(&format!("{name}=\""))
                        .nth(1)
                        .and_then(|rest| rest.split('"').next())
                        .and_then(|number| number.parse().ok())
                        .unwrap_or_else(|| panic!("a node should carry {name}"))
                };

                (value("cx") - MARK_CENTRE, value("cy") - MARK_CENTRE)
            })
            .collect()
    }

    /// Clockwise from straight up, in degrees.
    fn bearing((x, y): (f64, f64)) -> f64 {
        x.atan2(-y).to_degrees()
    }

    #[test]
    fn the_marks_nodes_all_sit_on_one_orbit() {
        let nodes = nodes();
        assert_eq!(nodes.len(), 3);

        for (x, y) in nodes {
            let orbit = x.hypot(y);
            assert!(
                (orbit - MARK_ORBIT).abs() < 0.01,
                "the node at ({x}, {y}) is {orbit} out, not {MARK_ORBIT}"
            );
        }
    }

    /// Three equal steps around the orbit make the triangle equilateral.
    #[test]
    fn the_marks_nodes_are_a_third_of_a_turn_apart() {
        let [top, left, right] = nodes()[..] else {
            panic!("the mark is three nodes")
        };

        assert!(bearing(top).abs() < 0.01);
        assert!((bearing(left) + 120.0).abs() < 0.01);
        assert!((bearing(right) - 120.0).abs() < 0.01);
    }

    #[test]
    fn every_face_the_stylesheet_names_is_embedded() {
        let named: Vec<&str> = FONTS_CSS
            .split("url(\"fonts/")
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
}

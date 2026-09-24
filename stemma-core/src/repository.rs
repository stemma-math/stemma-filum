//! What a repository holds, and what it says about itself.
//!
//! One file at the root says which group the repository belongs to,
//! which of the two it is, and what the group has pinned. The pin is
//! part of the declaration, not a convenience: two renderings of one line
//! must not differ because two machines happened to have different
//! versions of the compiler, so a surface either respects the pin or
//! refuses to compose.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::id::GroupId;
use crate::line::{BadName, Name};

/// The file at the root of every repository.
pub const MANIFEST: &str = "stemma.toml";

/// The directory lines live in.
pub const LINES: &str = "lines";

/// The file a line is written in.
pub const ENTRY: &str = "main.typ";

/// The Lean module that formalizes one line.
pub const LEAN_ENTRY: &str = "Main.lean";

/// The shape of a repository this build understands.
pub const FORMAT: u32 = 1;

/// Which of a group's two repositories this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// What the whole group has.
    Shared,
    /// What one person has inside one group, and nobody else can clone.
    Private,
}

spelled!(Side, BadSide("`{0}` is not one of a group's repositories") {
    Shared => "shared",
    Private => "private",
});

/// What the group has pinned.
///
/// Absent means nobody has pinned it yet, which is honest for a group
/// that has not written anything: a pin somebody never chose would be a
/// decision made by whichever release happened to create the repository.
///
/// `notation` is the exception, and is set when the repository is made.
/// It is not a tool the group goes and installs but Stemma's own
/// package, and there is no version of it a line could be written
/// without.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pins {
    /// The exact Typst release every surface must use.
    pub typst: Option<String>,
    /// The exact Lean release used to check formalizations.
    pub lean: Option<String>,
    /// The exact Mathlib revision used by the group's Lake project.
    pub mathlib: Option<String>,
    /// The version of the notation package the group's lines import.
    pub notation: Option<String>,
}

/// The file at the root of a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The shape of the repository.
    pub format: u32,
    /// The group it belongs to.
    pub group: GroupId,
    /// Which of the group's two repositories it is.
    pub side: Side,
    /// What it is, in one line, for whoever opens it by hand.
    pub description: Option<String>,
    /// What the group has pinned.
    #[serde(default)]
    pub pins: Pins,
}

impl Manifest {
    /// A manifest for a new repository.
    pub fn new(group: GroupId, side: Side, description: impl Into<String>) -> Self {
        Self {
            format: FORMAT,
            group,
            side,
            description: Some(description.into()),
            pins: Pins {
                notation: Some(crate::notation::VERSION.to_owned()),
                ..Pins::default()
            },
        }
    }

    /// Reads one.
    pub fn parse(text: &str) -> Result<Self, Unreadable> {
        let manifest: Self = toml::from_str(text).map_err(|error| Unreadable::Malformed {
            message: error.message().to_owned(),
        })?;

        if manifest.format != FORMAT {
            return Err(Unreadable::Newer {
                found: manifest.format,
                known: FORMAT,
            });
        }

        Ok(manifest)
    }

    /// Writes one, with the note that says what it is.
    pub fn to_text(&self) -> String {
        let body = toml::to_string_pretty(self).unwrap_or_default();

        format!(
            "# What this repository is. Stemma reads it; you may edit it.\n\
             #\n\
             # `pins` is part of the mathematics, not a convenience: every\n\
             # surface uses exactly what is pinned here, or refuses to\n\
             # compose rather than produce a second rendering of one line.\n\
             \n{body}"
        )
    }
}

/// Why a manifest could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unreadable {
    /// It is not the file it claims to be.
    #[error("{MANIFEST} is not valid: {message}")]
    Malformed {
        /// What the parser said.
        message: String,
    },
    /// It was written by a newer release.
    #[error("this repository is in format {found} and this build knows {known}: update Stemma")]
    Newer {
        /// What the file says.
        found: u32,
        /// What this build understands.
        known: u32,
    },
}

/// Where a line's directory is.
pub fn line_directory(root: &Path, name: &Name) -> PathBuf {
    root.join(LINES).join(name.as_str())
}

/// Where a line's source is.
pub fn line_entry(root: &Path, name: &Name) -> PathBuf {
    line_directory(root, name).join(ENTRY)
}

/// Where a line's Lean formalization is.
pub fn line_lean_entry(root: &Path, name: &Name) -> PathBuf {
    line_directory(root, name).join(LEAN_ENTRY)
}

/// The name of the line a path is inside, if it is inside one.
pub fn line_of(root: &Path, path: &Path) -> Option<Name> {
    let inside = path.strip_prefix(root.join(LINES)).ok()?;
    let first = inside.components().next()?;

    Name::parse(first.as_os_str().to_str()?).ok()
}

/// Checks a name somebody typed for a new line.
pub fn check_line_name(name: &str) -> Result<Name, BadName> {
    Name::parse(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> GroupId {
        "grp:algebra".parse().unwrap()
    }

    #[test]
    fn either_repository_survives_being_written_down() {
        for side in [Side::Shared, Side::Private] {
            assert_eq!(side.to_string().parse(), Ok(side));
        }

        assert!("both".parse::<Side>().is_err());
    }

    #[test]
    fn a_manifest_is_written_and_read_back() {
        let manifest = Manifest::new(group(), Side::Shared, "Algebra, shared");
        let text = manifest.to_text();

        assert_eq!(Manifest::parse(&text).unwrap(), manifest);
        assert!(text.contains("# What this repository is."));
        assert!(text.contains("part of the mathematics"));
    }

    #[test]
    fn a_group_that_has_pinned_something_says_which() {
        let text = r#"
            format = 1
            group = "grp:algebra"
            side = "shared"

            [pins]
            typst = "0.13.1"
        "#;

        let manifest = Manifest::parse(text).unwrap();

        assert_eq!(manifest.pins.typst.as_deref(), Some("0.13.1"));
        assert_eq!(manifest.side, Side::Shared);
    }

    #[test]
    fn the_formal_environment_is_pinned_with_the_document_environment() {
        let text = r#"
            format = 1
            group = "grp:algebra"
            side = "shared"

            [pins]
            typst = "0.13.1"
            lean = "4.24.0"
            mathlib = "v4.24.0"
        "#;
        let manifest = Manifest::parse(text).unwrap();

        assert_eq!(manifest.pins.lean.as_deref(), Some("4.24.0"));
        assert_eq!(manifest.pins.mathlib.as_deref(), Some("v4.24.0"));
    }

    #[test]
    fn a_group_that_has_pinned_nothing_says_that_too() {
        let manifest = Manifest::parse(
            r#"
            format = 1
            group = "grp:algebra"
            side = "private"
            "#,
        )
        .unwrap();

        assert_eq!(manifest.pins.typst, None);
    }

    #[test]
    fn a_repository_from_a_newer_release_is_refused_by_name() {
        let error = Manifest::parse(
            r#"
            format = 99
            group = "grp:algebra"
            side = "shared"
            "#,
        )
        .unwrap_err();

        assert!(matches!(error, Unreadable::Newer { found: 99, .. }));
        assert!(error.to_string().contains("update Stemma"));
    }

    #[test]
    fn a_manifest_that_is_not_one_says_so() {
        assert!(matches!(
            Manifest::parse("this is not toml at all ["),
            Err(Unreadable::Malformed { .. })
        ));
        assert!(matches!(
            Manifest::parse("format = 1"),
            Err(Unreadable::Malformed { .. })
        ));
    }

    #[test]
    fn a_line_lives_where_the_format_says_it_does() {
        let root = Path::new("/home/ana/stemma/algebra/shared");
        let name = Name::parse("quotient-groups").unwrap();

        assert_eq!(
            line_entry(root, &name),
            Path::new("/home/ana/stemma/algebra/shared/lines/quotient-groups/main.typ")
        );
    }

    #[test]
    fn a_path_says_which_line_it_belongs_to() {
        let root = Path::new("/home/ana/stemma/algebra/shared");

        assert_eq!(
            line_of(root, &root.join("lines/quotient-groups/main.typ")),
            Some(Name::parse("quotient-groups").unwrap())
        );
        assert_eq!(line_of(root, &root.join("lines")), None);
        assert_eq!(line_of(root, &root.join("stemma.toml")), None);
        assert_eq!(
            line_of(Path::new("/elsewhere"), &root.join("lines/a")),
            None
        );
    }
}

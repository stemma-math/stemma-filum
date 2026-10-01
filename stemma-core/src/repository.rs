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
use crate::line::Name;

/// The file at the root of every repository.
pub const MANIFEST: &str = "stemma.toml";

/// The directory lines live in.
pub const LINES: &str = "lines";

/// The file a line is written in: its blocks, as JSON.
pub const ENTRY: &str = "line.json";

/// The Lean module that formalizes one line.
pub const LEAN_ENTRY: &str = "Main.lean";

/// Lake's project description at the root of a library mirror.
pub const LAKEFILE: &str = "lakefile.toml";

/// The toolchain selected by the group's Lean pin.
pub const LEAN_TOOLCHAIN: &str = "lean-toolchain";

/// Lean used by newly created groups.
pub const DEFAULT_LEAN: &str = "4.34.1";

/// Mathlib used by newly created groups: the release built with
/// [`DEFAULT_LEAN`], whose build cache that Lean can read.
pub const DEFAULT_MATHLIB: &str = "v4.34.1";
/// An earlier default Mathlib, built with a different Lean than the one
/// pinned beside it.
pub const MISMATCHED_MATHLIB: &str = "v4.34.0";

/// The Stemma Lean package used by newly created groups. This commit is
/// present in the published `main-build`; no `v0.1.0` Git tag exists.
pub const DEFAULT_STEMMA: &str = "cf378e8954b36c1fea6093032692f45cf734ed72";
/// An earlier default that named a release tag which was never published.
pub const UNPUBLISHED_STEMMA: &str = "v0.1.0";

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
/// A new shared repository starts from Stemma's supported defaults. Private
/// repositories omit the Lean pins because they inherit the shared package's
/// exact environment. `package` is Stemma's Typst package version.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pins {
    /// The exact Typst release every surface must use.
    pub typst: Option<String>,
    /// The exact Lean release used to check formalizations.
    pub lean: Option<String>,
    /// The exact Mathlib revision used by the group's Lake project.
    pub mathlib: Option<String>,
    /// The release of the Stemma Lean package providing marks and extraction.
    pub stemma: Option<String>,
    /// Additional Lake packages, including releases of other groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<LakeDependency>,
    /// The version of the Typst package the group's lines are typeset with.
    #[serde(alias = "notation")]
    pub package: Option<String>,
}

/// One additional package pinned into the group's Lake project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LakeDependency {
    /// The package name Lake imports.
    pub name: String,
    /// Its Git repository.
    pub git: String,
    /// The immutable release tag or commit.
    pub rev: String,
    /// A package below the repository root, where needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_dir: Option<String>,
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
        let pins = match side {
            Side::Shared => Pins {
                lean: Some(DEFAULT_LEAN.to_owned()),
                mathlib: Some(DEFAULT_MATHLIB.to_owned()),
                stemma: Some(DEFAULT_STEMMA.to_owned()),
                package: Some(crate::package::VERSION.to_owned()),
                ..Pins::default()
            },
            Side::Private => Pins {
                package: Some(crate::package::VERSION.to_owned()),
                ..Pins::default()
            },
        };
        Self {
            format: FORMAT,
            group,
            side,
            description: Some(description.into()),
            pins,
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

    /// The files that make this repository a Lake package.
    pub fn lake_files(&self) -> Vec<(String, String)> {
        let lean = self.pins.lean.as_deref().unwrap_or(DEFAULT_LEAN);
        let (suffix, library, root) = match self.side {
            Side::Shared => (
                "library",
                "GroupLibrary",
                "import Stemma\n\n-- Formal code is organized by subject modules imported here.\n",
            ),
            Side::Private => (
                "private",
                "PrivateLibrary",
                "import GroupLibrary\n\n-- Private formal code extends the group's shared library.\n",
            ),
        };
        let package = format!(
            "stemma_{}_{suffix}",
            self.group.slug().as_str().replace('-', "_")
        );

        let mut lake = format!(
            "name = {}\nversion = \"0.1.0\"\ndefaultTargets = [{}]\n\n",
            toml_string(&package),
            toml_string(library)
        );
        if let Some(mathlib) = &self.pins.mathlib {
            require(
                &mut lake,
                &[
                    ("name", "mathlib"),
                    (
                        "git",
                        "https://github.com/leanprover-community/mathlib4.git",
                    ),
                    ("rev", mathlib),
                ],
            );
        }
        if let Some(stemma) = &self.pins.stemma {
            require(
                &mut lake,
                &[
                    ("name", "stemma"),
                    ("git", "https://github.com/stemma-math/stemma.git"),
                    ("rev", stemma),
                    ("subDir", "lean"),
                ],
            );
        }
        for dependency in &self.pins.dependencies {
            let mut fields = vec![
                ("name", dependency.name.as_str()),
                ("git", dependency.git.as_str()),
                ("rev", dependency.rev.as_str()),
            ];
            if let Some(sub_dir) = &dependency.sub_dir {
                fields.push(("subDir", sub_dir));
            }
            require(&mut lake, &fields);
        }
        if self.side == Side::Private {
            require(
                &mut lake,
                &[("name", "group_library"), ("path", "../shared")],
            );
        }
        lake.push_str("[[lean_lib]]\nname = ");
        lake.push_str(&toml_string(library));
        lake.push('\n');

        vec![
            (LAKEFILE.to_owned(), lake),
            (
                LEAN_TOOLCHAIN.to_owned(),
                format!("leanprover/lean4:v{lean}\n"),
            ),
            (format!("{library}.lean"), root.to_owned()),
        ]
    }
}

/// Adds one `[[require]]` table to a lakefile, followed by a blank line.
fn require(lake: &mut String, fields: &[(&str, &str)]) {
    lake.push_str("[[require]]\n");
    for (key, value) in fields {
        lake.push_str(key);
        lake.push_str(" = ");
        lake.push_str(&toml_string(value));
        lake.push('\n');
    }
    lake.push('\n');
}

/// A TOML string holding exactly this text.
///
/// Pins are typed by people, so a quote or a newline in one must stay
/// inside its value rather than end it.
fn toml_string(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
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
    fn a_new_group_starts_with_a_complete_pinned_lake_project() {
        let manifest = Manifest::new(group(), Side::Shared, "Algebra, shared");
        assert_eq!(manifest.pins.lean.as_deref(), Some(DEFAULT_LEAN));
        assert_eq!(manifest.pins.mathlib.as_deref(), Some(DEFAULT_MATHLIB));
        assert_eq!(manifest.pins.stemma.as_deref(), Some(DEFAULT_STEMMA));

        let files: std::collections::BTreeMap<_, _> = manifest.lake_files().into_iter().collect();
        assert!(files[LAKEFILE].contains("name = \"mathlib\""));
        assert!(files[LAKEFILE].contains("name = \"stemma\""));
        assert_eq!(
            files[LEAN_TOOLCHAIN],
            format!("leanprover/lean4:v{DEFAULT_LEAN}\n")
        );
    }

    #[test]
    fn a_lake_project_is_written_exactly_as_pinned() {
        let mut manifest = Manifest::new(
            "grp:algebra-two".parse().unwrap(),
            Side::Shared,
            "Algebra, shared",
        );
        manifest.pins.dependencies.push(LakeDependency {
            name: "topology".to_owned(),
            git: "https://example.org/topology.git".to_owned(),
            rev: "v1.0.0".to_owned(),
            sub_dir: Some("lean".to_owned()),
        });
        let files = manifest.lake_files();

        assert_eq!(
            files[0],
            (
                LAKEFILE.to_owned(),
                format!(
                    "name = \"stemma_algebra_two_library\"\n\
                     version = \"0.1.0\"\n\
                     defaultTargets = [\"GroupLibrary\"]\n\
                     \n\
                     [[require]]\n\
                     name = \"mathlib\"\n\
                     git = \"https://github.com/leanprover-community/mathlib4.git\"\n\
                     rev = \"{DEFAULT_MATHLIB}\"\n\
                     \n\
                     [[require]]\n\
                     name = \"stemma\"\n\
                     git = \"https://github.com/stemma-math/stemma.git\"\n\
                     rev = \"{DEFAULT_STEMMA}\"\n\
                     subDir = \"lean\"\n\
                     \n\
                     [[require]]\n\
                     name = \"topology\"\n\
                     git = \"https://example.org/topology.git\"\n\
                     rev = \"v1.0.0\"\n\
                     subDir = \"lean\"\n\
                     \n\
                     [[lean_lib]]\n\
                     name = \"GroupLibrary\"\n"
                )
            )
        );
        assert_eq!(files[2].0, "GroupLibrary.lean");
    }

    #[test]
    fn a_pin_that_needs_escaping_still_makes_a_valid_lakefile() {
        let mut manifest = Manifest::new(group(), Side::Private, "Ana only");
        manifest.pins.dependencies.push(LakeDependency {
            name: "odd".to_owned(),
            git: "https://example.org/\"quoted\"\\path.git".to_owned(),
            rev: "v1\nname = \"injected\"".to_owned(),
            sub_dir: None,
        });
        let files: std::collections::BTreeMap<_, _> = manifest.lake_files().into_iter().collect();
        let lakefile: toml::Table = files[LAKEFILE].parse().expect("a valid lakefile");
        let required = lakefile["require"].as_array().unwrap();

        assert_eq!(required.len(), 2);
        assert_eq!(
            required[0]["git"].as_str(),
            Some("https://example.org/\"quoted\"\\path.git")
        );
        assert_eq!(required[0]["rev"].as_str(), Some("v1\nname = \"injected\""));
        assert_eq!(lakefile["name"].as_str(), Some("stemma_algebra_private"));
    }

    #[test]
    fn a_private_package_depends_on_the_shared_package() {
        let manifest = Manifest::new(group(), Side::Private, "Ana only");
        let files: std::collections::BTreeMap<_, _> = manifest.lake_files().into_iter().collect();

        assert!(files[LAKEFILE].contains("path = \"../shared\""));
        assert!(files["PrivateLibrary.lean"].contains("import GroupLibrary"));
        assert!(
            manifest.pins.lean.is_none(),
            "the shared manifest owns the pin"
        );
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
            Path::new("/home/ana/stemma/algebra/shared/lines/quotient-groups/line.json")
        );
    }

    #[test]
    fn a_path_says_which_line_it_belongs_to() {
        let root = Path::new("/home/ana/stemma/algebra/shared");

        assert_eq!(
            line_of(root, &root.join("lines/quotient-groups/line.json")),
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

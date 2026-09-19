//! What a machine builds out of a repository.
//!
//! Build outputs do not go in git. They are pushed to the instance as
//! artefacts keyed by the commit they were built from, kept outside the
//! repositories and thrown away freely — anything here can be built
//! again from the commit it names.
//!
//! This is what lets the web show a line to somebody with no machine
//! running, without the instance ever compiling anything itself.

use serde::{Deserialize, Serialize};

use crate::id::GroupId;
use crate::line::{Line, Name};
use crate::repository::Side;

/// The shape of an artefact this build understands.
pub const FORMAT: u32 = 1;

/// What kind of thing an artefact is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// What the source declares: the nodes, their centrality and what
    /// they lean on.
    Nodes,
}

impl Kind {
    /// What it is called on the wire and on disk.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nodes => "nodes",
        }
    }

    /// The kind a name names.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "nodes" => Some(Self::Nodes),
            _ => None,
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One line, as it was read out of one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineDigest {
    /// The directory it lives in, which is what it is called.
    pub name: Name,
    /// What its first heading calls it, where it has one.
    pub title: Option<String>,
    /// What it declares.
    pub line: Line,
}

impl LineDigest {
    /// What to show a reader: the title if there is one, the name if not.
    pub fn shown_as(&self) -> &str {
        self.title.as_deref().unwrap_or_else(|| self.name.as_str())
    }
}

/// Everything one repository declares, at one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nodes {
    /// The shape of this artefact.
    pub format: u32,
    /// The group whose repository this is.
    pub group: GroupId,
    /// Which of the group's two repositories it is.
    pub side: Side,
    /// The commit it was built from.
    pub commit: String,
    /// What was pinned when it was built, where anything was.
    pub typst: Option<String>,
    /// The lines, in the order their directories sort.
    pub lines: Vec<LineDigest>,
}

impl Nodes {
    /// Builds one.
    pub fn new(group: GroupId, side: Side, commit: impl Into<String>) -> Self {
        Self {
            format: FORMAT,
            group,
            side,
            commit: commit.into(),
            typst: None,
            lines: Vec::new(),
        }
    }

    /// Reads one, refusing what this build does not understand.
    pub fn parse(text: &str) -> Result<Self, Unreadable> {
        let nodes: Self =
            serde_json::from_str(text).map_err(|error| Unreadable::Malformed(error.to_string()))?;

        if nodes.format != FORMAT {
            return Err(Unreadable::Newer {
                found: nodes.format,
                known: FORMAT,
            });
        }

        Ok(nodes)
    }

    /// Writes one.
    pub fn to_text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// One line of it.
    pub fn line(&self, name: &Name) -> Option<&LineDigest> {
        self.lines.iter().find(|line| &line.name == name)
    }

    /// How many statements the whole repository declares.
    pub fn statement_count(&self) -> usize {
        self.lines.iter().map(|line| line.line.nodes.len()).sum()
    }
}

/// Why an artefact could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unreadable {
    /// It is not what it claims to be.
    #[error("this artefact cannot be read: {0}")]
    Malformed(String),
    /// It was built by a newer release.
    #[error("this artefact is in format {found} and this build knows {known}")]
    Newer {
        /// What the artefact says.
        found: u32,
        /// What this build understands.
        known: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes() -> Nodes {
        let mut nodes = Nodes::new(
            "grp:algebra".parse().unwrap(),
            Side::Shared,
            "0123456789abcdef0123456789abcdef01234567",
        );

        nodes.lines.push(LineDigest {
            name: Name::parse("quotient-groups").unwrap(),
            title: Some("Quotient groups".to_owned()),
            line: Line::read(
                r#"
                #definition(id: "def:quotient")[x]
                #theorem(id: "thm:main", central: true, uses: "def:quotient")[y]
                "#,
            )
            .unwrap(),
        });

        nodes
    }

    #[test]
    fn an_artefact_is_written_and_read_back() {
        let built = nodes();
        let read_back = Nodes::parse(&built.to_text()).unwrap();

        assert_eq!(read_back, built);
        assert_eq!(read_back.statement_count(), 2);
        assert_eq!(
            read_back
                .line(&Name::parse("quotient-groups").unwrap())
                .unwrap()
                .shown_as(),
            "Quotient groups"
        );
    }

    #[test]
    fn a_line_with_no_heading_is_shown_by_its_directory() {
        let digest = LineDigest {
            name: Name::parse("untitled").unwrap(),
            title: None,
            line: Line::read("").unwrap(),
        };

        assert_eq!(digest.shown_as(), "untitled");
    }

    #[test]
    fn an_artefact_from_a_newer_release_is_refused() {
        let text = nodes().to_text().replace("\"format\":1", "\"format\":99");

        assert!(matches!(
            Nodes::parse(&text),
            Err(Unreadable::Newer { found: 99, .. })
        ));
    }

    #[test]
    fn something_that_is_not_an_artefact_is_refused() {
        assert!(matches!(Nodes::parse("{}"), Err(Unreadable::Malformed(_))));
    }

    #[test]
    fn a_kind_is_written_and_read_the_same_way() {
        assert_eq!(Kind::named(Kind::Nodes.as_str()), Some(Kind::Nodes));
        assert_eq!(Kind::named("something-else"), None);
    }
}

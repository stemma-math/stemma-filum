//! A line's source: an ordered list of blocks, kept as JSON.
//!
//! The file is the line. Everything else — the Typst document, the PDF,
//! the graph — is built from it and can be built again. Keeping the
//! source structured rather than as free text means an agent's answer
//! either is a valid line or is not, two versions are compared block by
//! block, and every block has an identity that survives being moved,
//! renamed or rewritten.
//!
//! A block is one of four things:
//!
//! - **prose**: what connects the mathematics — intentions, intuition,
//!   how one idea leads to the next. It declares nothing.
//! - **definition**: something given a meaning. Central unless said
//!   otherwise, because a line's definitions are what it builds on.
//! - **theorem**: a result, under whatever name its author gives the
//!   environment (theorem, lemma, proposition…). Ordinary unless declared
//!   central.
//! - **proof**: the argument for one definition or theorem, which it
//!   names by block identifier. It may stand anywhere in the line.
//!
//! What a block's content says is Typst markup, exactly what goes
//! between the brackets of the environment it becomes.

use serde::{Deserialize, Serialize};

use crate::id::{BlockId, BridgeId, UserId};
use crate::line::{NodeKind, Siglum};
use crate::markup::Scanner;

/// The shape of a line's source this build writes and reads.
pub const FORMAT: u32 = 1;

/// What a definition's environment is called unless its author says otherwise.
pub const DEFINITION: &str = "Definition";

/// What a theorem's environment is called unless its author says otherwise.
pub const THEOREM: &str = "Theorem";

/// A line's source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// The shape of this file.
    pub format: u32,
    /// What the line is called.
    pub title: String,
    /// What the line is about, in a sentence or two, where its author says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What it says, in order.
    #[serde(default)]
    pub blocks: Vec<Block>,
}

impl Document {
    /// A line with a title and nothing written yet.
    pub fn new(title: impl Into<String>, description: Option<String>) -> Self {
        Self {
            format: FORMAT,
            title: title.into(),
            description: description.filter(|said| !said.trim().is_empty()),
            blocks: Vec::new(),
        }
    }

    /// Reads a source, refusing one this build does not understand.
    pub fn parse(text: &str) -> Result<Self, Unreadable> {
        let document: Self = serde_json::from_str(text).map_err(|error| Unreadable::Malformed {
            line: error.line(),
            column: error.column(),
            said: error.to_string(),
        })?;

        if document.format != FORMAT {
            return Err(Unreadable::Format {
                found: document.format,
                known: FORMAT,
            });
        }

        Ok(document)
    }

    /// The source as it is written to disk.
    ///
    /// One field per line and a newline at the end, so that git shows a
    /// change to one block as a change to that block.
    pub fn to_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }

    /// The block with this identifier.
    pub fn block(&self, id: &BlockId) -> Option<&Block> {
        self.blocks.iter().find(|block| block.id() == id)
    }

    /// Where the block with this identifier is, counting from zero.
    pub fn position(&self, id: &BlockId) -> Option<usize> {
        self.blocks.iter().position(|block| block.id() == id)
    }

    /// The same line with a block added at the end.
    pub fn with(mut self, block: Block) -> Self {
        self.blocks.push(block);
        self
    }

    /// Adds a block at the end, answering with its identifier.
    pub fn push(&mut self, block: Block) -> BlockId {
        let id = *block.id();
        self.blocks.push(block);
        id
    }

    /// The proofs written for a block, in order.
    pub fn proofs_of<'a>(&'a self, id: &'a BlockId) -> impl Iterator<Item = &'a Proof> {
        self.blocks.iter().filter_map(move |block| match block {
            Block::Proof(proof) if &proof.of == id => Some(proof),
            _ => None,
        })
    }
}

/// Why a source could not be read at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unreadable {
    /// It is not JSON, or not a line.
    #[error("line {line}, column {column}: {said}")]
    Malformed {
        /// Where it went wrong, counting from one.
        line: usize,
        /// Where on that line, counting from one.
        column: usize,
        /// What went wrong.
        said: String,
    },
    /// It is written in a shape this build does not know.
    #[error("this line is written in format {found} and this build knows {known}")]
    Format {
        /// What the file says.
        found: u32,
        /// What this build writes.
        known: u32,
    },
}

/// One block of a line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", from = "Written")]
pub enum Block {
    /// Prose between the mathematics.
    Prose(Prose),
    /// Something given a meaning.
    Definition(Statement),
    /// A result.
    Theorem(Statement),
    /// The argument for a definition or a theorem.
    Proof(Proof),
}

impl Block {
    /// A new prose block.
    pub fn prose(content: impl Into<String>) -> Self {
        Self::Prose(Prose {
            id: BlockId::generate(),
            content: content.into(),
        })
    }

    /// A new definition, central and carrying no proof, as definitions are
    /// unless their author says otherwise.
    pub fn definition(siglum: Option<Siglum>, content: impl Into<String>) -> Self {
        Self::Definition(Statement::new(DEFINITION, siglum, true, false, content))
    }

    /// A new theorem, ordinary and carrying a proof, as theorems are
    /// unless their author says otherwise.
    ///
    /// The proof is a block of its own, which the caller adds.
    pub fn theorem(
        environment: impl Into<String>,
        siglum: Option<Siglum>,
        content: impl Into<String>,
    ) -> Self {
        Self::Theorem(Statement::new(environment, siglum, false, true, content))
    }

    /// A new proof of the block `of`.
    pub fn proof(of: BlockId, content: impl Into<String>) -> Self {
        Self::Proof(Proof {
            id: BlockId::generate(),
            of,
            content: content.into(),
        })
    }

    /// The same statement, central or not. Nothing else changes.
    pub fn central(mut self, yes: bool) -> Self {
        if let Some(statement) = self.statement_mut() {
            statement.central = yes;
        }
        self
    }

    /// The same statement, carrying a proof or not. Nothing else changes.
    pub fn carrying_proof(mut self, yes: bool) -> Self {
        if let Some(statement) = self.statement_mut() {
            statement.proof = yes;
        }
        self
    }

    /// The same statement, with a title. Nothing else changes.
    pub fn titled(mut self, title: impl Into<String>) -> Self {
        if let Some(statement) = self.statement_mut() {
            statement.title = Some(title.into());
        }
        self
    }

    /// The same statement, declaring that it uses `other`. Nothing else
    /// changes.
    pub fn using(mut self, other: &Block) -> Self {
        let used = *other.id();
        if let Some(statement) = self.statement_mut() {
            statement.uses.push(used);
        }
        self
    }

    /// The statement it is, to change, when it is a definition or a theorem.
    pub fn statement_mut(&mut self) -> Option<&mut Statement> {
        match self {
            Self::Definition(statement) | Self::Theorem(statement) => Some(statement),
            _ => None,
        }
    }

    /// Its identifier.
    pub fn id(&self) -> &BlockId {
        match self {
            Self::Prose(prose) => &prose.id,
            Self::Definition(statement) | Self::Theorem(statement) => &statement.id,
            Self::Proof(proof) => &proof.id,
        }
    }

    /// What it says.
    pub fn content(&self) -> &str {
        match self {
            Self::Prose(prose) => &prose.content,
            Self::Definition(statement) | Self::Theorem(statement) => &statement.content,
            Self::Proof(proof) => &proof.content,
        }
    }

    /// Which of the four it is.
    pub fn kind(&self) -> BlockKind {
        match self {
            Self::Prose(_) => BlockKind::Prose,
            Self::Definition(_) => BlockKind::Definition,
            Self::Theorem(_) => BlockKind::Theorem,
            Self::Proof(_) => BlockKind::Proof,
        }
    }

    /// The statement it is, when it is a definition or a theorem.
    pub fn statement(&self) -> Option<&Statement> {
        match self {
            Self::Definition(statement) | Self::Theorem(statement) => Some(statement),
            _ => None,
        }
    }

    /// The label its Typst document gives it, which `@` and a proof point
    /// at.
    ///
    /// Its siglum when its author named it, so that `@thm:main` in the
    /// text finds it; its block identifier when nobody did.
    pub(crate) fn label(&self) -> String {
        match self
            .statement()
            .and_then(|statement| statement.siglum.as_ref())
        {
            Some(siglum) => siglum.to_string(),
            None => self.id().to_string(),
        }
    }

    /// The siglum it answers to, when it is a definition or a theorem.
    pub fn siglum(&self) -> Option<Siglum> {
        match self {
            Self::Definition(statement) => Some(statement.siglum_as(BlockKind::Definition)),
            Self::Theorem(statement) => Some(statement.siglum_as(BlockKind::Theorem)),
            _ => None,
        }
    }
}

/// Which of the four things a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    /// Prose between the mathematics.
    Prose,
    /// Something given a meaning.
    Definition,
    /// A result.
    Theorem,
    /// The argument for a definition or a theorem.
    Proof,
}

spelled!(BlockKind {
    Prose => "prose",
    Definition => "definition",
    Theorem => "theorem",
    Proof => "proof",
});

/// Prose between the mathematics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prose {
    /// Its identifier.
    pub id: BlockId,
    /// What it says.
    pub content: String,
}

/// A definition or a theorem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    /// Its identifier.
    pub id: BlockId,
    /// What its environment is called on the page: `Theorem`, `Lemma`…
    pub environment: String,
    /// The name other blocks and other lines refer to it by, where it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub siglum: Option<Siglum>,
    /// What it is called in prose: `Bolzano`, `Quotient group`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether the line rests on it.
    ///
    /// A central statement is frozen against agents: its text and what
    /// it uses move only when its author accepts a change.
    pub central: bool,
    /// Whether it carries a proof.
    ///
    /// A theorem always does, even an empty one, unless its author says
    /// there is no intention of proving it here — a result quoted from
    /// elsewhere, say. Being unproved is not a reason to turn it off.
    pub proof: bool,
    /// The statements it leans on, chosen by its author.
    ///
    /// What its content and its proof cite with `@siglum` is added to
    /// these when the line is read, so a reference written in the text
    /// does not have to be declared twice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<BlockId>,
    /// Audited substitution licences it relies on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bridges: Vec<BridgeId>,
    /// The person responsible for downstream decisions, when declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<UserId>,
    /// What it says.
    pub content: String,
}

impl Statement {
    fn new(
        environment: impl Into<String>,
        siglum: Option<Siglum>,
        central: bool,
        proof: bool,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: BlockId::generate(),
            environment: environment.into(),
            siglum,
            title: None,
            central,
            proof,
            uses: Vec::new(),
            bridges: Vec::new(),
            author: None,
            content: content.into(),
        }
    }

    /// The siglum it answers to.
    ///
    /// One its author did not name is called after its block, so every
    /// statement can be pointed at by a job, a Lean marker or the vault.
    pub fn siglum_as(&self, kind: BlockKind) -> Siglum {
        if let Some(siglum) = &self.siglum {
            return siglum.clone();
        }

        let node = match kind {
            BlockKind::Definition => NodeKind::Definition,
            _ => NodeKind::for_environment(&self.environment),
        };
        let name = self.id.ulid().to_string().to_ascii_lowercase();

        Siglum::new(
            node,
            crate::line::Name::parse(&name).expect("a ULID is a valid name"),
        )
    }
}

/// The argument for a definition or a theorem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proof {
    /// Its identifier.
    pub id: BlockId,
    /// The block it proves.
    pub of: BlockId,
    /// What it says. Empty while nobody has written it.
    pub content: String,
}

/// A block as it may be written, before defaults are filled in.
///
/// Definitions and theorems differ in what they are unless their author
/// says otherwise, so the defaults depend on the kind and cannot be
/// written as field attributes.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Written {
    Prose {
        id: BlockId,
        #[serde(default)]
        content: String,
    },
    Definition(WrittenStatement),
    Theorem(WrittenStatement),
    Proof {
        id: BlockId,
        of: BlockId,
        #[serde(default)]
        content: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenStatement {
    id: BlockId,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    siglum: Option<Siglum>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    central: Option<bool>,
    #[serde(default)]
    proof: Option<bool>,
    #[serde(default)]
    uses: Vec<BlockId>,
    #[serde(default)]
    bridges: Vec<BridgeId>,
    #[serde(default)]
    author: Option<UserId>,
    #[serde(default)]
    content: String,
}

impl WrittenStatement {
    fn filled(self, environment: &str, central: bool, proof: bool) -> Statement {
        Statement {
            id: self.id,
            environment: self
                .environment
                .filter(|said| !said.trim().is_empty())
                .unwrap_or_else(|| environment.to_owned()),
            siglum: self.siglum,
            title: self.title.filter(|said| !said.trim().is_empty()),
            central: self.central.unwrap_or(central),
            proof: self.proof.unwrap_or(proof),
            uses: self.uses,
            bridges: self.bridges,
            author: self.author,
            content: self.content,
        }
    }
}

impl From<Written> for Block {
    fn from(written: Written) -> Self {
        match written {
            Written::Prose { id, content } => Self::Prose(Prose { id, content }),
            Written::Definition(statement) => {
                Self::Definition(statement.filled(DEFINITION, true, false))
            }
            Written::Theorem(statement) => Self::Theorem(statement.filled(THEOREM, false, true)),
            Written::Proof { id, of, content } => Self::Proof(Proof { id, of, content }),
        }
    }
}

/// Every label a piece of Typst markup refers to with `@`, in order.
///
/// What is inside raw text or a comment is not a reference, and neither
/// is an escaped `@` or one in the middle of a word, which is an address.
pub fn references(content: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut scanner = Scanner::new(content);

    while !scanner.is_done() {
        if scanner.skip_inert() {
            continue;
        }

        let starts_a_reference = scanner.peek(0) == Some(b'@')
            && !scanner
                .previous()
                .is_some_and(|byte| byte.is_ascii_alphanumeric());
        scanner.at += 1;
        if !starts_a_reference {
            continue;
        }

        let start = scanner.at;
        while scanner
            .peek(0)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || b"_-:.".contains(&byte))
        {
            scanner.at += 1;
        }
        // A sentence may end right after a reference.
        let label = scanner.since(start).trim_end_matches(['.', ':']);
        if !label.is_empty() {
            found.push(label.to_owned());
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> BlockId {
        BlockId::from_ulid(ulid::Ulid(n))
    }

    #[test]
    fn a_new_line_has_a_title_and_nothing_else() {
        let document = Document::new("Quotient groups", Some("  ".to_owned()));

        assert_eq!(document.format, FORMAT);
        assert_eq!(document.description, None);
        assert!(document.blocks.is_empty());
        assert_eq!(
            Document::parse(&document.to_text()).unwrap(),
            document,
            "what is written is what is read"
        );
    }

    #[test]
    fn definitions_and_theorems_default_to_what_they_usually_are() {
        let text = format!(
            r#"{{"format": 1, "title": "T", "blocks": [
                {{"kind": "definition", "id": "{}", "content": "A group."}},
                {{"kind": "theorem", "id": "{}", "content": "It is finite."}}
            ]}}"#,
            id(1),
            id(2)
        );
        let document = Document::parse(&text).unwrap();

        let definition = document.blocks[0].statement().unwrap();
        assert_eq!(definition.environment, DEFINITION);
        assert!(definition.central);
        assert!(!definition.proof);

        let theorem = document.blocks[1].statement().unwrap();
        assert_eq!(theorem.environment, THEOREM);
        assert!(!theorem.central);
        assert!(theorem.proof);
    }

    #[test]
    fn what_is_written_down_is_every_field_so_nothing_depends_on_defaults() {
        let text = format!(
            r#"{{"format": 1, "title": "T", "blocks": [
                {{"kind": "theorem", "id": "{}", "content": "x"}}
            ]}}"#,
            id(1)
        );
        let written = Document::parse(&text).unwrap().to_text();

        assert!(written.contains("\"environment\": \"Theorem\""));
        assert!(written.contains("\"central\": false"));
        assert!(written.contains("\"proof\": true"));
        assert!(!written.contains("uses"), "empty lists are left out");
    }

    #[test]
    fn a_field_nobody_knows_is_refused_rather_than_dropped() {
        let text = format!(
            r#"{{"format": 1, "title": "T", "blocks": [
                {{"kind": "prose", "id": "{}", "content": "x", "centrl": true}}
            ]}}"#,
            id(1)
        );

        assert!(matches!(
            Document::parse(&text),
            Err(Unreadable::Malformed { .. })
        ));
        assert!(matches!(
            Document::parse(r#"{"format": 1, "title": "T", "question": "?"}"#),
            Err(Unreadable::Malformed { .. })
        ));
    }

    #[test]
    fn a_newer_format_is_refused_by_name() {
        assert_eq!(
            Document::parse(r#"{"format": 9, "title": "T"}"#),
            Err(Unreadable::Format { found: 9, known: 1 })
        );
    }

    #[test]
    fn a_statement_nobody_named_is_called_after_its_block() {
        let text = format!(
            r#"{{"format": 1, "title": "T", "blocks": [
                {{"kind": "theorem", "id": "{}", "environment": "Lemma", "content": "x"}},
                {{"kind": "definition", "id": "{}", "siglum": "def:group", "content": "y"}}
            ]}}"#,
            id(1),
            id(2)
        );
        let document = Document::parse(&text).unwrap();

        let unnamed = document.blocks[0].siglum().unwrap();
        assert_eq!(unnamed.kind, NodeKind::Lemma);
        assert_eq!(
            unnamed.name.as_str(),
            id(1).ulid().to_string().to_ascii_lowercase()
        );
        assert_eq!(
            document.blocks[1].siglum().unwrap().to_string(),
            "def:group"
        );
    }

    #[test]
    fn references_are_read_out_of_markup_and_nothing_else() {
        let content = "By @def:group and @thm:main. Mail ana@example.org. \
                       `@lem:raw` // @lem:comment\n\
                       /* @lem:block */ \\@lem:escaped then @lem:last:";

        assert_eq!(references(content), ["def:group", "thm:main", "lem:last"]);
    }

    #[test]
    fn a_reference_inside_a_nested_comment_is_not_one() {
        let content = "/* outer /* inner */ @lem:hidden */ @lem:seen";

        assert_eq!(references(content), ["lem:seen"]);
    }

    #[test]
    fn references_are_read_out_of_text_that_is_not_ascii() {
        let content = "Según @thm:main, y por \\é también @lem:b.";

        assert_eq!(references(content), ["thm:main", "lem:b"]);
    }
}

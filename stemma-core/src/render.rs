//! Writing a line out as a Typst document.
//!
//! The document is built, never edited: the line is its [`Document`], and
//! this is one of the things built from it, like the PDF built from this.
//! The same document always comes out of the same line, so two machines
//! typesetting one commit set the same page.
//!
//! Every block becomes one call to the environment of its kind, with the
//! block's fields as named arguments. The package turns each call into
//! what the page shows and into a `metadata` element carrying the same
//! fields, so what a line declares can be queried out of the document.

use std::ops::Range;

use crate::document::{Block, Document};
use crate::id::BlockId;

/// A line, as a Typst document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The document.
    pub source: String,
    /// Where each block is in it, in the line's order.
    pub blocks: Vec<Placed>,
}

/// Where one block landed in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// The block.
    pub id: BlockId,
    /// Everything its call occupies, in bytes.
    pub whole: Range<usize>,
    /// What its content occupies, in bytes. Its first byte is the first
    /// byte of the block's content.
    pub content: Range<usize>,
}

impl Rendered {
    /// The block whose call covers a byte of the document.
    pub fn block_at(&self, byte: usize) -> Option<&Placed> {
        self.blocks
            .iter()
            .find(|placed| placed.whole.start <= byte && byte < placed.whole.end)
    }

    /// Which line of the document a byte is on, counting from one.
    pub fn line_of(&self, byte: usize) -> usize {
        self.source[..byte.min(self.source.len())]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1
    }
}

/// What the first line of every rendered document says.
pub const PREAMBLE: &str =
    "// Built from the line's line.json by Stemma. Change the line, not this file.";

/// Writes a line as a Typst document.
pub fn typst(document: &Document) -> Rendered {
    let mut source = String::new();
    let mut blocks = Vec::with_capacity(document.blocks.len());

    source.push_str(PREAMBLE);
    source.push('\n');
    source.push_str(&crate::notation::import());
    source.push_str("\n\n#show: stemma.with(\n  title: ");
    source.push_str(&quoted(&document.title));
    source.push_str(",\n");
    if let Some(description) = &document.description {
        source.push_str("  description: ");
        source.push_str(&quoted(description));
        source.push_str(",\n");
    }
    source.push_str(")\n");

    for (at, block) in document.blocks.iter().enumerate() {
        source.push('\n');
        let start = source.len();

        let mut arguments = vec![("id", quoted(&block.id().to_string()))];
        match block {
            Block::Prose(_) => {}
            Block::Definition(statement) | Block::Theorem(statement) => {
                arguments.push(("environment", quoted(&statement.environment)));
                // Only a siglum its author chose is shown: one made up from
                // the block identifier is for machines, not for readers.
                if let Some(siglum) = &statement.siglum {
                    arguments.push(("siglum", quoted(&siglum.to_string())));
                }
                if let Some(title) = &statement.title {
                    arguments.push(("title", quoted(title)));
                }
                arguments.push(("central", statement.central.to_string()));
                arguments.push(("proof", statement.proof.to_string()));
                if !statement.uses.is_empty() {
                    arguments.push(("uses", listed(&statement.uses)));
                }
            }
            Block::Proof(proof) => {
                arguments.push(("of", quoted(&proof.of.to_string())));
                if let Some(proved) = document.block(&proof.of) {
                    arguments.push(("target", quoted(&label_of(proved))));
                }
                // A proof straight after what it proves needs to say no
                // more than that it is one.
                let follows = at > 0 && document.blocks[at - 1].id() == &proof.of;
                arguments.push(("adjacent", follows.to_string()));
            }
        }

        source.push('#');
        source.push_str(block.kind().as_str());
        source.push('(');
        let said: Vec<String> = arguments
            .into_iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect();
        source.push_str(&said.join(", "));
        source.push(')');

        let content = block.content().trim_matches('\n');
        let content_range = if content.trim().is_empty() {
            source.push_str("[]");
            source.len() - 1..source.len() - 1
        } else {
            source.push_str("[\n");
            let begins = source.len();
            source.push_str(content);
            let ends = source.len();
            source.push_str("\n]");
            begins..ends
        };

        if block.statement().is_some() {
            source.push_str(" <");
            source.push_str(&label_of(block));
            source.push('>');
        }

        blocks.push(Placed {
            id: *block.id(),
            whole: start..source.len(),
            content: content_range,
        });
        source.push('\n');
    }

    Rendered { source, blocks }
}

/// A line's source as the Typst document it becomes, or as it is when it
/// is not a line, so that what is wrong with it can still be read.
pub fn readable(source: &str) -> String {
    match Document::parse(source) {
        Ok(document) => typst(&document).source,
        Err(_) => source.to_owned(),
    }
}

/// The label a statement is given, which `@` and its proof point at.
///
/// Its siglum when its author named it, so that `@thm:main` in the text
/// finds it; its block identifier when nobody did.
fn label_of(block: &Block) -> String {
    match block
        .statement()
        .and_then(|statement| statement.siglum.as_ref())
    {
        Some(siglum) => siglum.to_string(),
        None => block.id().to_string(),
    }
}

/// A Typst string holding exactly this text.
fn quoted(text: &str) -> String {
    let mut written = String::with_capacity(text.len() + 2);
    written.push('"');
    for character in text.chars() {
        match character {
            '"' => written.push_str("\\\""),
            '\\' => written.push_str("\\\\"),
            '\n' => written.push_str("\\n"),
            '\r' => written.push_str("\\r"),
            '\t' => written.push_str("\\t"),
            other => written.push(other),
        }
    }
    written.push('"');
    written
}

/// A Typst array of block identifiers.
fn listed(ids: &[BlockId]) -> String {
    let items: Vec<String> = ids.iter().map(|id| quoted(&id.to_string())).collect();
    match items.len() {
        // One item in parentheses is a group, not an array, without its comma.
        1 => format!("({},)", items[0]),
        _ => format!("({})", items.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line::Siglum;

    fn siglum(text: &str) -> Option<Siglum> {
        Some(text.parse().unwrap())
    }

    fn sample() -> Document {
        let mut document = Document::new("Quotient \"groups\"", Some("Why they split.".to_owned()));
        document.push(Block::prose("We begin."));
        let group = document.push(Block::definition(siglum("def:group"), "A group."));
        let mut lemma = Block::theorem("Lemma", None, "By @def:group, it holds.");
        lemma.statement_mut().unwrap().uses.push(group);
        lemma.statement_mut().unwrap().title = Some("Bolzano".to_owned());
        let lemma = document.push(lemma);
        document.push(Block::proof(lemma, ""));
        document
    }

    #[test]
    fn a_line_becomes_one_call_per_block_after_the_import() {
        let document = sample();
        let rendered = typst(&document);

        assert!(rendered.source.starts_with(PREAMBLE));
        assert!(rendered.source.contains(&crate::notation::import()));
        assert!(
            rendered
                .source
                .contains("title: \"Quotient \\\"groups\\\"\",")
        );
        assert!(rendered.source.contains("description: \"Why they split.\""));
        assert_eq!(rendered.blocks.len(), 4);

        for (placed, block) in rendered.blocks.iter().zip(&document.blocks) {
            let call = &rendered.source[placed.whole.clone()];
            assert!(call.starts_with(&format!("#{}(", block.kind())));
            assert!(call.contains(&block.id().to_string()));
        }
    }

    #[test]
    fn a_block_s_content_is_where_the_placement_says_it_is() {
        let document = sample();
        let rendered = typst(&document);

        for (placed, block) in rendered.blocks.iter().zip(&document.blocks) {
            assert_eq!(
                &rendered.source[placed.content.clone()],
                block.content().trim_matches('\n')
            );
        }
    }

    #[test]
    fn a_named_statement_is_labelled_with_its_siglum_and_an_unnamed_one_with_its_block() {
        let document = sample();
        let rendered = typst(&document);
        let lemma = document.blocks[2].id();

        assert!(rendered.source.contains("] <def:group>"));
        assert!(rendered.source.contains(&format!("] <{lemma}>")));
        assert!(
            rendered
                .source
                .contains(&format!("target: \"{lemma}\", adjacent: true)[]"))
        );
    }

    #[test]
    fn what_a_statement_is_is_said_in_full() {
        let rendered = typst(&sample()).source;

        assert!(rendered.contains(
            "environment: \"Definition\", siglum: \"def:group\", central: true, proof: false)"
        ));
        assert!(rendered.contains("environment: \"Lemma\""));
        assert!(rendered.contains("title: \"Bolzano\""));
        assert!(
            rendered.contains("uses: (\"blk:"),
            "one use is still an array"
        );
    }

    #[test]
    fn the_same_line_always_becomes_the_same_document() {
        let document = sample();
        assert_eq!(typst(&document), typst(&document.clone()));
    }

    #[test]
    fn where_a_byte_is_is_answered_by_block_and_by_line() {
        let document = sample();
        let rendered = typst(&document);
        let second = &rendered.blocks[1];

        assert_eq!(
            rendered.block_at(second.content.start).map(|one| one.id),
            Some(*document.blocks[1].id())
        );
        assert_eq!(rendered.block_at(0), None);
        assert_eq!(rendered.line_of(0), 1);
    }
}

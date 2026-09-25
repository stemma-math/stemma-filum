//! Two versions of one line, and the rule for merging them.
//!
//! Only people's machines write mathematics, so the two versions are
//! never two people's: they are one person's, on two of their own
//! machines. Nobody is owed a negotiation, and what is needed is a rule.
//!
//! The rule is that **the line is decided block by block**. A block one
//! side changed and the other left alone is not a conflict and is taken
//! over silently; only a block both sides changed, differently, is put
//! to the person, with the two versions as they wrote them. Everything
//! else — the order of the blocks, the title, the description — follows
//! the machine the decision is being made on, except where only the
//! other machine changed it.
//!
//! Nothing here deletes anything. What it produces is a source, and the
//! version not kept stays in the history it came from.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::document::{Block, Document, Unreadable};
use crate::id::BlockId;

/// Which of the two versions of a block to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    /// The one on the machine the decision is being made on.
    Here,
    /// The one that came from the server.
    There,
}

spelled!(Keep, UnknownKeep("`{0}` is not a version: say `here` or `there`") {
    Here => "here",
    There => "there",
});

/// A block the two versions do not agree on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    /// Which block it is.
    pub block: BlockId,
    /// What to call it: its siglum, or where it is.
    pub label: String,
    /// How it reads on this machine, when it is still there.
    pub here: Option<String>,
    /// How it reads on the server, when it is still there.
    pub there: Option<String>,
}

impl Conflict {
    /// What happened to it, in words.
    pub fn said(&self) -> &'static str {
        match (&self.here, &self.there) {
            (Some(_), Some(_)) => "changed on both machines",
            (Some(_), None) => "changed here and deleted on the server",
            (None, Some(_)) => "deleted here and changed on the server",
            (None, None) => "deleted on both machines",
        }
    }
}

/// Why two versions could not be compared or merged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Trouble {
    /// One of the three versions is not a readable line.
    #[error("the {which} version cannot be read: {problem}")]
    Unreadable {
        /// Which of the three it is.
        which: &'static str,
        /// What is wrong with it.
        problem: Unreadable,
    },
    /// A block both machines changed has no decision.
    #[error("no version of {0} has been chosen")]
    Undecided(String),
    /// Two blocks in one version share an identifier, so neither can be
    /// matched with the other versions.
    #[error("the {which} version has block `{block}` twice")]
    Duplicate {
        /// Which version contains the duplicate.
        which: &'static str,
        /// The identifier they share.
        block: BlockId,
    },
}

/// What the two versions conflict on, in this machine's order.
///
/// Only the blocks both machines changed. A block one of them changed
/// and the other left alone is not a conflict, and presenting it as one
/// would ask a person to decide something they already decided.
pub fn between(base: &str, here: &str, there: &str) -> Result<Vec<Conflict>, Trouble> {
    let versions = Versions::read(base, here, there)?;
    let mut found: Vec<Conflict> = versions
        .ids()
        .into_iter()
        .filter(|id| !versions.settled(id))
        .map(|id| Conflict {
            block: id,
            label: versions.label(&id),
            here: versions.here.get(&id).map(shown),
            there: versions.there.get(&id).map(shown),
        })
        .collect();

    found.sort_by_key(|conflict| {
        (
            versions.here.at.get(&conflict.block).copied(),
            versions.there.at.get(&conflict.block).copied(),
        )
    });

    Ok(found)
}

/// The line as the decisions leave it.
pub fn merge(
    base: &str,
    here: &str,
    there: &str,
    decisions: &BTreeMap<BlockId, Keep>,
) -> Result<String, Trouble> {
    let versions = Versions::read(base, here, there)?;
    let mut kept: BTreeMap<BlockId, Option<&Block>> = BTreeMap::new();

    for id in versions.ids() {
        let (was, mine, theirs) = (
            versions.base.get(&id),
            versions.here.get(&id),
            versions.there.get(&id),
        );

        let chosen = if mine == theirs || was == theirs {
            mine
        } else if was == mine {
            theirs
        } else {
            match decisions.get(&id) {
                Some(Keep::Here) => mine,
                Some(Keep::There) => theirs,
                None => return Err(Trouble::Undecided(versions.label(&id))),
            }
        };
        kept.insert(id, chosen);
    }

    // This machine's order, less what goes, plus what arrives from the
    // server placed after whatever it follows there.
    let mut order: Vec<BlockId> = versions
        .here
        .document
        .blocks
        .iter()
        .map(|block| *block.id())
        .filter(|id| kept.get(id).copied().flatten().is_some())
        .collect();

    let mut previous: Option<BlockId> = None;
    for block in &versions.there.document.blocks {
        let id = *block.id();
        if !order.contains(&id) && kept.get(&id).copied().flatten().is_some() {
            let at = previous
                .and_then(|previous| order.iter().position(|one| one == &previous))
                .map_or(0, |position| position + 1);
            order.insert(at, id);
        }
        if order.contains(&id) {
            previous = Some(id);
        }
    }

    let merged = Document {
        format: versions.here.document.format,
        title: three_way(
            &versions.base.document.title,
            &versions.here.document.title,
            &versions.there.document.title,
        )
        .clone(),
        description: three_way(
            &versions.base.document.description,
            &versions.here.document.description,
            &versions.there.document.description,
        )
        .clone(),
        blocks: order
            .iter()
            .filter_map(|id| kept.get(id).copied().flatten().cloned())
            .collect(),
    };

    Ok(merged.to_text())
}

/// This machine's value, unless only the server changed it.
fn three_way<'a, T: PartialEq>(base: &'a T, here: &'a T, there: &'a T) -> &'a T {
    match here == base {
        true => there,
        false => here,
    }
}

/// A block as a person reads it when choosing between two versions.
fn shown(block: &Block) -> String {
    let mut said = String::new();
    match block {
        Block::Prose(_) => said.push_str("Prose"),
        Block::Definition(statement) | Block::Theorem(statement) => {
            said.push_str(&statement.environment);
            if let Some(title) = &statement.title {
                said.push_str(&format!(" ({title})"));
            }
            if let Some(siglum) = block.siglum() {
                said.push_str(&format!(" [{siglum}]"));
            }
            if statement.central {
                said.push_str(", central");
            }
        }
        Block::Proof(_) => said.push_str("Proof"),
    }
    said.push('\n');
    said.push_str(block.content().trim());
    said
}

/// One version, read and indexed by block.
struct Version {
    document: Document,
    at: BTreeMap<BlockId, usize>,
}

impl Version {
    fn read(which: &'static str, text: &str) -> Result<Self, Trouble> {
        // A line only one side had when they parted is merged against nothing.
        let document = if which == "common" && text.trim().is_empty() {
            Document::new("", None)
        } else {
            Document::parse(text).map_err(|problem| Trouble::Unreadable { which, problem })?
        };

        let mut at = BTreeMap::new();
        for (position, block) in document.blocks.iter().enumerate() {
            if at.insert(*block.id(), position).is_some() {
                return Err(Trouble::Duplicate {
                    which,
                    block: *block.id(),
                });
            }
        }

        Ok(Self { document, at })
    }

    fn get(&self, id: &BlockId) -> Option<&Block> {
        self.at
            .get(id)
            .map(|position| &self.document.blocks[*position])
    }
}

/// The three versions of one line.
struct Versions {
    base: Version,
    here: Version,
    there: Version,
}

impl Versions {
    fn read(base: &str, here: &str, there: &str) -> Result<Self, Trouble> {
        Ok(Self {
            base: Version::read("common", base)?,
            here: Version::read("local", here)?,
            there: Version::read("server", there)?,
        })
    }

    /// Every block any of the three has.
    fn ids(&self) -> BTreeSet<BlockId> {
        [&self.base, &self.here, &self.there]
            .into_iter()
            .flat_map(|version| version.at.keys().copied())
            .collect()
    }

    /// Whether a block needs nobody's decision.
    fn settled(&self, id: &BlockId) -> bool {
        let (was, mine, theirs) = (self.base.get(id), self.here.get(id), self.there.get(id));
        mine == theirs || was == mine || was == theirs
    }

    /// What to call a block: its siglum, the statement a proof is of, or
    /// where it stands.
    fn label(&self, id: &BlockId) -> String {
        let Some((version, block)) = [&self.here, &self.there, &self.base]
            .into_iter()
            .find_map(|version| Some((version, version.get(id)?)))
        else {
            return id.to_string();
        };

        if let Some(siglum) = block.siglum() {
            return siglum.to_string();
        }
        if let Block::Proof(proof) = block
            && let Some(siglum) = version.get(&proof.of).and_then(Block::siglum)
        {
            return format!("the proof of {siglum}");
        }
        format!("{} block {}", block.kind(), version.at[id] + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A definition, a central theorem and its proof.
    fn base() -> Document {
        let mut document = Document::new("Quotient groups", None);
        document.push(Block::definition(
            Some("def:group".parse().unwrap()),
            "A group.",
        ));
        let mut main = Block::theorem(
            "Theorem",
            Some("thm:main".parse().unwrap()),
            "Every finite group is finite.",
        );
        main.statement_mut().unwrap().central = true;
        let main = document.push(main);
        document.push(Block::proof(main, "It is."));
        document
    }

    fn with_proof(document: &Document, content: &str) -> Document {
        let mut changed = document.clone();
        let Block::Proof(proof) = &mut changed.blocks[2] else {
            panic!("the third block is the proof");
        };
        proof.content = content.to_owned();
        changed
    }

    fn proof_id(document: &Document) -> BlockId {
        *document.blocks[2].id()
    }

    #[test]
    fn two_machines_that_wrote_the_same_thing_conflict_on_nothing() {
        let base = base().to_text();
        assert_eq!(between(&base, &base, &base).unwrap(), Vec::new());
    }

    #[test]
    fn a_block_only_one_machine_changed_is_not_put_to_anybody() {
        let original = base();
        let there = with_proof(&original, "Because every finite set is finite.");
        let (base, there) = (original.to_text(), there.to_text());

        assert_eq!(between(&base, &base, &there).unwrap(), Vec::new());

        let merged = merge(&base, &base, &there, &BTreeMap::new()).unwrap();
        assert!(merged.contains("Because every finite set is finite."));
    }

    #[test]
    fn a_block_both_machines_changed_is_put_to_the_person_with_both_versions() {
        let original = base();
        let here = with_proof(&original, "By counting.").to_text();
        let there = with_proof(&original, "By induction.").to_text();

        let conflicts = between(&original.to_text(), &here, &there).unwrap();

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].block, proof_id(&original));
        assert_eq!(conflicts[0].label, "the proof of thm:main");
        assert_eq!(conflicts[0].here.as_deref(), Some("Proof\nBy counting."));
        assert_eq!(conflicts[0].there.as_deref(), Some("Proof\nBy induction."));
        assert_eq!(conflicts[0].said(), "changed on both machines");
    }

    #[test]
    fn nothing_is_merged_until_every_conflict_is_decided() {
        let original = base();
        let here = with_proof(&original, "By counting.").to_text();
        let there = with_proof(&original, "By induction.").to_text();

        assert_eq!(
            merge(&original.to_text(), &here, &there, &BTreeMap::new()),
            Err(Trouble::Undecided("the proof of thm:main".to_owned()))
        );
    }

    #[test]
    fn the_version_the_person_keeps_is_the_one_that_stands() {
        let original = base();
        let here = with_proof(&original, "By counting.").to_text();
        let there = with_proof(&original, "By induction.").to_text();
        let decide = |keep| BTreeMap::from([(proof_id(&original), keep)]);

        let kept = merge(&original.to_text(), &here, &there, &decide(Keep::Here)).unwrap();
        assert!(kept.contains("By counting."));
        assert!(!kept.contains("By induction."));

        let taken = merge(&original.to_text(), &here, &there, &decide(Keep::There)).unwrap();
        assert!(taken.contains("By induction."));
        assert!(!taken.contains("By counting."));
    }

    #[test]
    fn a_block_added_on_the_server_arrives_where_it_was_added() {
        let original = base();
        let mut there = original.clone();
        let mut lemma = Block::theorem("Lemma", Some("lem:step".parse().unwrap()), "A step.");
        lemma.statement_mut().unwrap().proof = false;
        let lemma_id = *lemma.id();
        there.blocks.insert(2, lemma);

        let merged = merge(
            &original.to_text(),
            &original.to_text(),
            &there.to_text(),
            &BTreeMap::new(),
        )
        .unwrap();
        let merged = Document::parse(&merged).unwrap();

        assert_eq!(merged.position(&lemma_id), Some(2));
        assert_eq!(merged.blocks.len(), 4);
        assert!(crate::line::Line::of(&merged).is_ok());
    }

    #[test]
    fn additions_keep_the_servers_order_even_at_the_start() {
        let original = base();
        let mut there = original.clone();
        let first = Block::prose("First.");
        let second = Block::prose("Second.");
        let (first_id, second_id) = (*first.id(), *second.id());
        there.blocks.insert(0, second);
        there.blocks.insert(0, first);

        let merged = merge(
            &original.to_text(),
            &original.to_text(),
            &there.to_text(),
            &BTreeMap::new(),
        )
        .unwrap();
        let merged = Document::parse(&merged).unwrap();

        assert_eq!(merged.position(&first_id), Some(0));
        assert_eq!(merged.position(&second_id), Some(1));
    }

    #[test]
    fn a_block_deleted_on_the_server_goes_from_here_too() {
        let original = base();
        let mut there = original.clone();
        there.blocks.remove(0);

        let merged = merge(
            &original.to_text(),
            &original.to_text(),
            &there.to_text(),
            &BTreeMap::new(),
        )
        .unwrap();

        assert!(!merged.contains("def:group"));
    }

    #[test]
    fn a_block_one_machine_deleted_and_the_other_changed_is_put_to_the_person() {
        let original = base();
        let mut here = original.clone();
        here.blocks.remove(2);
        let there = with_proof(&original, "By induction.");

        let conflicts = between(&original.to_text(), &here.to_text(), &there.to_text()).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].here, None);
        assert_eq!(
            conflicts[0].said(),
            "deleted here and changed on the server"
        );

        let kept = merge(
            &original.to_text(),
            &here.to_text(),
            &there.to_text(),
            &BTreeMap::from([(proof_id(&original), Keep::Here)]),
        )
        .unwrap();
        assert!(!kept.contains("By induction."));
    }

    #[test]
    fn the_order_and_the_title_are_this_machines_unless_only_the_server_changed_them() {
        let original = base();
        let mut here = original.clone();
        here.title = "Quotient groups, at last".to_owned();
        here.blocks.swap(1, 2);
        let mut there = original.clone();
        there.title = "Groups, quotients of".to_owned();
        there.description = Some("Why they split.".to_owned());

        let merged = merge(
            &original.to_text(),
            &here.to_text(),
            &there.to_text(),
            &BTreeMap::new(),
        )
        .unwrap();
        let merged = Document::parse(&merged).unwrap();

        assert_eq!(merged.title, "Quotient groups, at last");
        assert_eq!(merged.description.as_deref(), Some("Why they split."));
        assert_eq!(merged.blocks[1], here.blocks[1]);
    }

    #[test]
    fn a_line_both_machines_started_separately_merges_against_nothing() {
        let here = base();
        let there = base();

        let conflicts = between("", &here.to_text(), &there.to_text()).unwrap();
        assert!(conflicts.is_empty(), "their blocks are different blocks");
    }

    #[test]
    fn a_repeated_block_is_refused_rather_than_silently_overwritten() {
        let original = base();
        let mut there = original.clone();
        there.blocks.push(original.blocks[0].clone());

        assert_eq!(
            merge(
                &original.to_text(),
                &original.to_text(),
                &there.to_text(),
                &BTreeMap::new()
            ),
            Err(Trouble::Duplicate {
                which: "server",
                block: *original.blocks[0].id(),
            })
        );
    }

    #[test]
    fn a_version_that_cannot_be_read_is_named() {
        let base = base().to_text();
        assert!(matches!(
            between(&base, "{", &base),
            Err(Trouble::Unreadable { which: "local", .. })
        ));
    }
}

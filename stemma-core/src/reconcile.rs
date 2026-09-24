//! Two versions of one line, and the rule for bringing them together.
//!
//! Only people's machines write mathematics, so the two versions are
//! never two people's: they are one person's, on two of their own
//! machines. Nobody is owed a negotiation, and what is needed is a rule.
//!
//! The rule is that **the mathematics is decided statement by statement**
//! and everything else in the document follows the machine the decision
//! is being made on. A statement one side changed and the other left
//! alone is not a disagreement and is taken over silently; only a
//! statement both sides wrote is put to the person, with the two versions
//! as they wrote them.
//!
//! Nothing here deletes anything. What it produces is a source, and the
//! version not kept stays in the history it came from.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::line::Siglum;
use crate::source::{self, Problem};

/// Which of the two versions of a statement to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    /// The one on the machine the decision is being made on.
    Here,
    /// The one that came from the instance.
    There,
}

spelled!(Keep, UnknownKeep("`{0}` is not a version: say `here` or `there`") {
    Here => "here",
    There => "there",
});

/// Which part of a statement a piece of source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// What it claims.
    Statement,
    /// The argument for it.
    Proof,
}

/// One decidable piece of a line: a statement, or its proof.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Mark {
    /// The statement it belongs to.
    pub siglum: Siglum,
    /// Whether it is the statement itself or its argument.
    pub part: Part,
}

impl fmt::Display for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.part {
            Part::Statement => write!(f, "{}", self.siglum),
            Part::Proof => write!(f, "the proof of {}", self.siglum),
        }
    }
}

/// A piece of a line, as one version has it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Piece {
    /// Everything it occupies in that version's source.
    span: Range<usize>,
    /// What it says there.
    text: String,
    /// Where it comes in that version.
    order: usize,
}

/// A piece the two versions do not agree on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Divergence {
    /// Which piece it is.
    pub mark: Mark,
    /// How it reads on this machine, when it is still there.
    pub here: Option<String>,
    /// How it reads on the instance, when it is still there.
    pub there: Option<String>,
}

impl Divergence {
    /// What happened to it, in words.
    pub fn said(&self) -> &'static str {
        match (&self.here, &self.there) {
            (Some(_), Some(_)) => "it was written in both places",
            (Some(_), None) => "it was written here and taken out there",
            (None, Some(_)) => "it was taken out here and written there",
            (None, None) => "it is in neither version",
        }
    }
}

/// Why two versions could not be compared or brought together.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Trouble {
    /// One of the three versions is not a readable source.
    #[error("the {which} version cannot be read: {problem}")]
    Unreadable {
        /// Which of the three it is.
        which: &'static str,
        /// What is wrong with it.
        problem: Problem,
    },
    /// A piece both machines wrote has no decision.
    #[error("nobody has said which version of {0} to keep")]
    Undecided(Mark),
    /// Multiple declarations would be collapsed into a single merge decision.
    #[error("the {which} version declares {mark} more than once")]
    Duplicate {
        /// Which version contains the duplicate.
        which: &'static str,
        /// The ambiguous statement or proof.
        mark: Mark,
    },
}

/// What the two versions disagree about.
///
/// Only the pieces both machines wrote. A piece one of them changed and
/// the other left alone is not a disagreement, and presenting it as one
/// would ask a person to decide something they already decided.
pub fn between(base: &str, here: &str, there: &str) -> Result<Vec<Divergence>, Trouble> {
    let (base, here, there) = read_all(base, here, there)?;
    let mut found = Vec::new();

    for mark in marks(&base, &here, &there) {
        let (was, mine, theirs) = (base.get(&mark), here.get(&mark), there.get(&mark));

        if settled(was, mine, theirs) {
            continue;
        }

        found.push(Divergence {
            mark,
            here: mine.map(|piece| piece.text.clone()),
            there: theirs.map(|piece| piece.text.clone()),
        });
    }

    found.sort_by_key(|divergence| {
        (
            here.get(&divergence.mark).map(|piece| piece.order),
            there.get(&divergence.mark).map(|piece| piece.order),
        )
    });

    Ok(found)
}

/// The line as the decisions leave it.
///
/// Everything outside the declarations — headings, prose, the order of
/// the document — is this machine's. That is the rule, and a rule is all
/// this needs: both versions belong to the same person.
pub fn merge(
    base: &str,
    here: &str,
    there: &str,
    decisions: &BTreeMap<Mark, Keep>,
) -> Result<String, Trouble> {
    let (was, mine, theirs) = read_all(base, here, there)?;
    let mut edits = Vec::new();

    for mark in marks(&was, &mine, &theirs) {
        let (before, ours, other) = (was.get(&mark), mine.get(&mark), theirs.get(&mark));

        if settled(before, ours, other) {
            // Either nobody moved it, or only this machine did. What is
            // already in this source is what was decided.
            if text_of(before) == text_of(ours) && text_of(before) != text_of(other) {
                edits.push(take_theirs(&mark, ours, other, &mine, &theirs, here.len()));
            }

            continue;
        }

        match decisions.get(&mark) {
            Some(Keep::Here) => continue,
            Some(Keep::There) => {
                edits.push(take_theirs(&mark, ours, other, &mine, &theirs, here.len()));
            }
            None => return Err(Trouble::Undecided(mark)),
        }
    }

    Ok(apply(here, edits))
}

/// Whether a piece needs nobody's decision.
fn settled(base: Option<&Piece>, here: Option<&Piece>, there: Option<&Piece>) -> bool {
    let (was, mine, theirs) = (text_of(base), text_of(here), text_of(there));

    mine == theirs || was == mine || was == theirs
}

/// The text of a piece that may not be there.
fn text_of(piece: Option<&Piece>) -> Option<&str> {
    piece.map(|piece| piece.text.as_str())
}

/// The edit that puts the instance's version of a piece in place.
struct Edit {
    range: Range<usize>,
    replacement: String,
    order: usize,
}

fn take_theirs(
    mark: &Mark,
    ours: Option<&Piece>,
    other: Option<&Piece>,
    mine: &Version,
    theirs: &Version,
    source_len: usize,
) -> Edit {
    let (range, replacement) = match (ours, other) {
        // It is in both: their words go where ours are.
        (Some(ours), Some(other)) => (ours.span.clone(), other.text.clone()),
        // They took it out.
        (Some(ours), None) => (ours.span.clone(), String::new()),
        // They wrote it, and it goes after whatever it follows there.
        (None, Some(other)) => {
            let at = anchor(mark, mine, theirs, source_len);
            let before_first = mine.values().map(|piece| piece.span.start).min() == Some(at);
            let replacement = if before_first {
                format!("{}\n\n", other.text)
            } else {
                format!("\n\n{}", other.text)
            };
            (at..at, replacement)
        }
        (None, None) => (0..0, String::new()),
    };

    Edit {
        range,
        replacement,
        order: other.map_or(0, |piece| piece.order),
    }
}

/// Where a piece only the instance has belongs in this machine's source.
///
/// After the last thing that comes before it there and exists here, so a
/// statement written between two others arrives between those two.
fn anchor(mark: &Mark, mine: &Version, theirs: &Version, source_len: usize) -> usize {
    let Some(placed) = theirs.get(mark) else {
        return mine
            .values()
            .map(|piece| piece.span.end)
            .max()
            .unwrap_or(source_len);
    };

    theirs
        .iter()
        .filter(|(_, piece)| piece.order < placed.order)
        .filter_map(|(other, _)| mine.get(other))
        .map(|piece| piece.span.end)
        .max()
        .unwrap_or_else(|| {
            // Nothing it follows is here, so it goes before everything.
            mine.values()
                .map(|piece| piece.span.start)
                .min()
                .unwrap_or(source_len)
        })
}

/// Carries out the edits, from the end of the document backwards.
fn apply(source: &str, mut edits: Vec<Edit>) -> String {
    // Replacements precede insertions at the same offset. Insertions sharing
    // an anchor are applied in reverse document order, so their final order
    // is the instance's, independent of the identifiers' alphabetical order.
    edits.sort_by_key(|edit| std::cmp::Reverse((edit.range.start, edit.range.end, edit.order)));

    let mut written = source.to_owned();

    for Edit {
        range, replacement, ..
    } in edits
    {
        let mut range = range;

        // Taking a declaration out takes the blank line it sat on with
        // it, so removing one does not leave a hole in the document.
        if replacement.is_empty() {
            while written[range.end..].starts_with('\n') && range.end < written.len() {
                range.end += 1;
            }
        }

        written.replace_range(range, &replacement);
    }

    written
}

/// Every piece any of the three versions has.
fn marks(base: &Version, here: &Version, there: &Version) -> BTreeSet<Mark> {
    base.keys()
        .chain(here.keys())
        .chain(there.keys())
        .cloned()
        .collect()
}

/// The pieces of one version, by the mark that names them.
type Version = BTreeMap<Mark, Piece>;

/// Reads the three versions, naming whichever of them will not be read.
fn read_all(base: &str, here: &str, there: &str) -> Result<(Version, Version, Version), Trouble> {
    Ok((
        pieces("common", base)?,
        pieces("local", here)?,
        pieces("instance", there)?,
    ))
}

/// The decidable pieces of one version.
fn pieces(which: &'static str, source: &str) -> Result<Version, Trouble> {
    let declarations =
        source::read(source).map_err(|problem| Trouble::Unreadable { which, problem })?;
    let mut found = BTreeMap::new();

    for (order, declaration) in declarations.into_iter().enumerate() {
        let part = match declaration.environment.as_str() {
            "proof" => Part::Proof,
            _ => Part::Statement,
        };
        let named = match part {
            Part::Statement => declaration.text("id"),
            Part::Proof => declaration.text("of"),
        };
        let Some(Ok(siglum)) = named.map(str::parse::<Siglum>) else {
            continue;
        };

        let mark = Mark { siglum, part };
        if found.contains_key(&mark) {
            return Err(Trouble::Duplicate { which, mark });
        }
        found.insert(
            mark,
            Piece {
                text: source[declaration.span.clone()].to_owned(),
                span: declaration.span,
                order,
            },
        );
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "= Quotient groups\n\n\
        #definition(id: \"def:group\", name: \"Group\")[A group.]\n\n\
        #theorem(id: \"thm:main\", central: true, uses: \"def:group\")[Every finite group is finite.]\n\n\
        #proof(of: \"thm:main\")[It is.]\n";

    fn mark(siglum: &str, part: Part) -> Mark {
        Mark {
            siglum: siglum.parse().unwrap(),
            part,
        }
    }

    #[test]
    fn two_machines_that_wrote_the_same_thing_disagree_about_nothing() {
        assert_eq!(between(BASE, BASE, BASE).unwrap(), Vec::new());
    }

    #[test]
    fn a_statement_only_one_machine_touched_is_not_put_to_anybody() {
        let there = BASE.replace("It is.", "Because every finite set is finite.");

        assert_eq!(between(BASE, BASE, &there).unwrap(), Vec::new());

        let merged = merge(BASE, BASE, &there, &BTreeMap::new()).unwrap();

        assert!(merged.contains("Because every finite set is finite."));
        assert!(merged.contains("= Quotient groups"));
    }

    #[test]
    fn a_statement_both_machines_wrote_is_put_to_the_person_with_both_versions() {
        let here = BASE.replace("It is.", "By counting.");
        let there = BASE.replace("It is.", "By induction.");

        let divergences = between(BASE, &here, &there).unwrap();

        assert_eq!(divergences.len(), 1);
        assert_eq!(divergences[0].mark, mark("thm:main", Part::Proof));
        assert!(
            divergences[0]
                .here
                .as_ref()
                .unwrap()
                .contains("By counting.")
        );
        assert!(
            divergences[0]
                .there
                .as_ref()
                .unwrap()
                .contains("By induction.")
        );
        assert_eq!(divergences[0].said(), "it was written in both places");
    }

    #[test]
    fn nothing_is_brought_together_until_every_disagreement_is_decided() {
        let here = BASE.replace("It is.", "By counting.");
        let there = BASE.replace("It is.", "By induction.");

        assert_eq!(
            merge(BASE, &here, &there, &BTreeMap::new()),
            Err(Trouble::Undecided(mark("thm:main", Part::Proof)))
        );
    }

    #[test]
    fn the_version_the_person_keeps_is_the_one_that_stands() {
        let here = BASE.replace("It is.", "By counting.");
        let there = BASE.replace("It is.", "By induction.");
        let decide = |keep| BTreeMap::from([(mark("thm:main", Part::Proof), keep)]);

        let kept = merge(BASE, &here, &there, &decide(Keep::Here)).unwrap();
        assert!(kept.contains("By counting."));
        assert!(!kept.contains("By induction."));

        let taken = merge(BASE, &here, &there, &decide(Keep::There)).unwrap();
        assert!(taken.contains("By induction."));
        assert!(!taken.contains("By counting."));
    }

    #[test]
    fn a_statement_written_only_on_the_instance_arrives_where_it_was_written() {
        let there = BASE.replace(
            "#proof(of: \"thm:main\")",
            "#lemma(id: \"lem:step\")[A step.]\n\n#proof(of: \"thm:main\")",
        );

        let merged = merge(BASE, BASE, &there, &BTreeMap::new()).unwrap();

        assert!(merged.contains("#lemma(id: \"lem:step\")[A step.]"));
        assert!(
            merged.find("lem:step").unwrap() < merged.find("#proof").unwrap(),
            "a lemma written before the proof arrives before it:\n{merged}"
        );
        assert!(crate::line::Line::read(&merged).is_ok());
    }

    #[test]
    fn a_statement_taken_out_on_the_instance_goes_from_here_too() {
        let there = BASE.replace("#proof(of: \"thm:main\")[It is.]\n", "");

        let merged = merge(BASE, BASE, &there, &BTreeMap::new()).unwrap();

        assert!(!merged.contains("#proof"));
        assert!(merged.contains("#theorem"));
        assert!(!merged.contains("\n\n\n"), "a hole was left:\n{merged}");
    }

    #[test]
    fn a_statement_one_machine_took_out_and_the_other_rewrote_is_put_to_the_person() {
        let here = BASE.replace("#proof(of: \"thm:main\")[It is.]\n", "");
        let there = BASE.replace("It is.", "By induction.");

        let divergences = between(BASE, &here, &there).unwrap();

        assert_eq!(divergences.len(), 1);
        assert_eq!(divergences[0].here, None);
        assert_eq!(
            divergences[0].said(),
            "it was taken out here and written there"
        );

        let kept = merge(
            BASE,
            &here,
            &there,
            &BTreeMap::from([(mark("thm:main", Part::Proof), Keep::Here)]),
        )
        .unwrap();

        assert!(!kept.contains("#proof"));
    }

    #[test]
    fn everything_outside_the_statements_is_this_machines() {
        let here = BASE.replace("= Quotient groups", "= Quotient groups, at last");
        let there = BASE.replace("= Quotient groups", "= Groups, quotients of");

        let merged = merge(BASE, &here, &there, &BTreeMap::new()).unwrap();

        assert!(merged.contains("= Quotient groups, at last"));
    }

    #[test]
    fn additions_at_one_anchor_keep_the_instances_document_order() {
        let additions = "#lemma(id: \"lem:z\")[First.]\n\n\
            #lemma(id: \"lem:a\")[Second.]\n\n\
            #proof(of: \"lem:a\")[Third.]";
        for (base, there) in [
            (String::new(), additions.to_owned()),
            (BASE.to_owned(), format!("{BASE}\n{additions}")),
            (BASE.to_owned(), format!("{additions}\n\n{BASE}")),
        ] {
            let merged = merge(&base, &base, &there, &BTreeMap::new()).unwrap();
            assert!(
                merged.contains(additions),
                "wrong insertion order:\n{merged}"
            );
            assert!(crate::line::Line::read(&merged).is_ok());
        }
    }

    #[test]
    fn the_first_statement_arrives_after_the_local_preamble() {
        let base = "#import \"notation.typ\": *\n\n= A new line\n\n#question[Why?]\n";
        let there = format!("{base}\n#theorem(id: \"thm:first\")[An answer.]");
        let here = base.replace("A new line", "The local title");
        let merged = merge(base, &here, &there, &BTreeMap::new()).unwrap();
        assert!(merged.starts_with(&here));
        assert!(merged.ends_with("#theorem(id: \"thm:first\")[An answer.]"));
    }

    #[test]
    fn an_insertion_before_a_replaced_or_removed_first_statement_survives() {
        let base = "#theorem(id: \"thm:main\")[Old.]";
        let added = "#lemma(id: \"lem:first\")[New.]";
        for there in [
            format!("{added}\n\n{}", base.replace("Old.", "Changed.")),
            added.to_owned(),
        ] {
            let merged = merge(base, base, &there, &BTreeMap::new()).unwrap();
            assert_eq!(merged.trim(), there);
        }
    }

    #[test]
    fn duplicate_statements_and_proofs_are_not_silently_overwritten() {
        for (declaration, part) in [
            ("#theorem(id: \"thm:main\")[Duplicated.]", Part::Statement),
            ("#proof(of: \"thm:main\")[Another proof.]", Part::Proof),
        ] {
            let ambiguous = format!("{BASE}\n{declaration}");
            assert_eq!(
                merge(BASE, BASE, &ambiguous, &BTreeMap::new()),
                Err(Trouble::Duplicate {
                    which: "instance",
                    mark: mark("thm:main", part),
                })
            );
        }
    }

    #[test]
    fn a_version_that_cannot_be_read_is_named() {
        assert_eq!(
            between(BASE, "#theorem(id: \"thm:main\"", BASE),
            Err(Trouble::Unreadable {
                which: "local",
                problem: source::read("#theorem(id: \"thm:main\"").unwrap_err(),
            })
        );
    }
}

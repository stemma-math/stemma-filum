//! What changed between two versions of a source.
//!
//! A proposal arrives as a whole line, because that is the only thing an
//! agent can be trusted to produce and the only thing a person can sign.
//! What a person needs to *read*, though, is what moved — so this says
//! it, line by line, and the screens draw it.
//!
//! The two versions are compared as the Typst documents they become,
//! which is how mathematics reads, rather than as the JSON they are kept
//! in. Lines and not characters: a diff that highlights three words
//! inside a rewritten statement invites reading the statement as mostly
//! unchanged.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::line::Line;
use crate::render;

/// The most changed lines per version this compares using a quadratic table.
///
/// Common prefixes and suffixes are always retained. Beyond this limit, the
/// remaining section is shown as one replacement to bound time and memory.
pub const LONGEST: usize = 4_000;

/// What became of one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// It is in both versions.
    Kept,
    /// It is only in the proposed one.
    Added,
    /// It is only in the one that stands.
    Removed,
}

/// One line of the comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    /// What became of it.
    pub change: Change,
    /// What it says.
    pub text: String,
    /// Which line it was, in the version that stands.
    pub before: Option<usize>,
    /// Which line it is, in the proposed version.
    pub after: Option<usize>,
}

impl Row {
    /// Whether this row is part of what a person has to read.
    pub fn moved(&self) -> bool {
        self.change != Change::Kept
    }
}

/// How much moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Extent {
    /// Lines the proposal adds.
    pub added: usize,
    /// Lines it takes out.
    pub removed: usize,
}

impl Extent {
    /// Whether the two versions are the same document.
    pub fn is_nothing(self) -> bool {
        self.added == 0 && self.removed == 0
    }

    /// What it says on screen.
    pub fn said(self) -> String {
        match (self.added, self.removed) {
            (0, 0) => "nothing changed".to_owned(),
            (added, 0) => format!("{} added", counted(added)),
            (0, removed) => format!("{} taken out", counted(removed)),
            (added, removed) => {
                format!("{} added, {} taken out", counted(added), counted(removed))
            }
        }
    }
}

/// A count of lines, with the right noun.
fn counted(count: usize) -> String {
    match count {
        1 => "1 line".to_owned(),
        many => format!("{many} lines"),
    }
}

/// Two versions of a line, line by line of the documents they become.
///
/// A version that is not a readable line is compared as the text it is,
/// so that what is wrong with it can still be seen.
pub fn between(before: &str, after: &str) -> Vec<Row> {
    lines(&render::readable(before), &render::readable(after))
}

/// Two texts, line by line.
pub fn lines(before: &str, after: &str) -> Vec<Row> {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();

    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_middle = &old[prefix..old.len() - suffix];
    let new_middle = &new[prefix..new.len() - suffix];
    let mut common: Vec<_> = (0..prefix).map(|at| (at, at)).collect();
    if old_middle.len() <= LONGEST && new_middle.len() <= LONGEST {
        common.extend(
            longest_common(old_middle, new_middle)
                .into_iter()
                .map(|(a, b)| (a + prefix, b + prefix)),
        );
    }
    common.extend((0..suffix).map(|at| (old.len() - suffix + at, new.len() - suffix + at)));
    let mut rows = Vec::with_capacity(old.len().max(new.len()));
    let (mut at_old, mut at_new) = (0, 0);

    // A last pair just past both ends flushes whatever neither kept.
    for (old_at, new_at) in common.into_iter().chain([(old.len(), new.len())]) {
        rows.extend((at_old..old_at).map(|at| gone(&old, at)));
        rows.extend((at_new..new_at).map(|at| arrived(&new, at)));

        if old_at < old.len() {
            rows.push(Row {
                change: Change::Kept,
                text: old[old_at].to_owned(),
                before: Some(old_at + 1),
                after: Some(new_at + 1),
            });
        }
        at_old = old_at + 1;
        at_new = new_at + 1;
    }

    rows
}

/// How much the two versions differ.
pub fn extent(rows: &[Row]) -> Extent {
    let mut extent = Extent::default();
    for row in rows {
        match row.change {
            Change::Kept => {}
            Change::Added => extent.added += 1,
            Change::Removed => extent.removed += 1,
        }
    }
    extent
}

/// The lines a statement the line rests on occupies in its document.
///
/// What the freeze guard forbids touching, said in the line numbers of
/// [`between`], so a screen can put corner marks around it without
/// reading any mathematics.
pub fn frozen(source: &str) -> BTreeSet<usize> {
    let Ok(document) = Document::parse(source) else {
        return BTreeSet::new();
    };
    if Line::of(&document).is_err() {
        return BTreeSet::new();
    }
    let rendered = render::typst(&document);
    let mut found = BTreeSet::new();

    for (placed, block) in rendered.blocks.iter().zip(&document.blocks) {
        if !block.statement().is_some_and(|statement| statement.central) {
            continue;
        }

        let first = rendered.line_of(placed.whole.start);
        let last = rendered.line_of(placed.whole.end);
        found.extend(first..=last);
    }

    found
}

/// One line as the version that stands has it.
fn gone(old: &[&str], at: usize) -> Row {
    Row {
        change: Change::Removed,
        text: old[at].to_owned(),
        before: Some(at + 1),
        after: None,
    }
}

/// One line as the proposal has it.
fn arrived(new: &[&str], at: usize) -> Row {
    Row {
        change: Change::Added,
        text: new[at].to_owned(),
        before: None,
        after: Some(at + 1),
    }
}

/// The pairs of lines the two versions have in common, in order.
fn longest_common(old: &[&str], new: &[&str]) -> Vec<(usize, usize)> {
    // Lengths cannot exceed LONGEST, so two bytes per cell suffice. A flat
    // table also avoids allocating each row separately.
    debug_assert!(old.len() <= LONGEST && new.len() <= LONGEST);
    let width = new.len() + 1;
    let mut table = vec![0u16; (old.len() + 1) * width];

    for (at_old, line) in old.iter().enumerate().rev() {
        for (at_new, other) in new.iter().enumerate().rev() {
            let at = at_old * width + at_new;
            table[at] = match line == other {
                true => table[at + width + 1] + 1,
                false => table[at + width].max(table[at + 1]),
            };
        }
    }

    let mut common = Vec::new();
    let (mut at_old, mut at_new) = (0, 0);

    while at_old < old.len() && at_new < new.len() {
        if old[at_old] == new[at_new] {
            common.push((at_old, at_new));
            at_old += 1;
            at_new += 1;
        } else if table[(at_old + 1) * width + at_new] >= table[at_old * width + at_new + 1] {
            at_old += 1;
        } else {
            at_new += 1;
        }
    }

    common
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::document::Block;

    const BEFORE: &str = "= Quotient groups\n\n\
        #theorem(id: \"thm:main\", central: true)[Every finite group is finite.]\n\n\
        #proof(of: \"thm:main\")[It is.]\n";

    /// A line with a central theorem, its proof and a lemma after them.
    fn line() -> Document {
        let mut document = Document::new("Quotient groups", None);
        let mut theorem = Block::theorem(
            "Theorem",
            Some("thm:main".parse().unwrap()),
            "Every finite group\nis finite.",
        );
        theorem.statement_mut().unwrap().central = true;
        let theorem = document.push(theorem);
        document.push(Block::proof(theorem, "It is."));
        let mut lemma = Block::theorem("Lemma", Some("lem:step".parse().unwrap()), "A step.");
        lemma.statement_mut().unwrap().proof = false;
        document.push(lemma);
        document
    }

    fn texts(rows: &[Row], change: Change) -> Vec<String> {
        rows.iter()
            .filter(|row| row.change == change)
            .map(|row| row.text.clone())
            .collect()
    }

    #[test]
    fn two_documents_that_are_the_same_moved_nothing() {
        let rows = lines(BEFORE, BEFORE);

        assert!(rows.iter().all(|row| row.change == Change::Kept));
        assert!(extent(&rows).is_nothing());
        assert_eq!(extent(&rows).said(), "nothing changed");
    }

    #[test]
    fn large_unchanged_documents_are_not_reported_as_replacements() {
        let source = "A line.\n".repeat(LONGEST + 1);
        assert!(extent(&lines(&source, &source)).is_nothing());
    }

    #[test]
    fn a_small_edit_in_a_large_document_retains_context_and_line_numbers() {
        let prefix = "Before.\n".repeat(LONGEST);
        let before = format!("{prefix}Old.\nAfter.\n");
        let after = format!("{prefix}New.\nAfter.\n");
        let rows = lines(&before, &after);
        assert_eq!(
            extent(&rows),
            Extent {
                added: 1,
                removed: 1
            }
        );
        assert_eq!(rows[LONGEST].before, Some(LONGEST + 1));
        assert_eq!(rows[LONGEST + 1].after, Some(LONGEST + 1));
        assert_eq!(rows.last().unwrap().change, Change::Kept);
    }

    #[test]
    fn wholesale_fallback_retains_the_common_edges() {
        let before = format!("Heading\n{}End\n", "Old.\n".repeat(LONGEST + 1));
        let after = format!("Heading\n{}End\n", "New.\n".repeat(LONGEST + 1));
        let rows = lines(&before, &after);
        assert_eq!(
            extent(&rows),
            Extent {
                added: LONGEST + 1,
                removed: LONGEST + 1
            }
        );
        assert_eq!(rows.first().unwrap().change, Change::Kept);
        assert_eq!(rows.last().unwrap().change, Change::Kept);
    }

    #[test]
    fn what_moved_is_what_is_shown_as_moved() {
        let after = BEFORE.replace("It is.", "By induction on the order.");
        let rows = lines(BEFORE, &after);

        assert_eq!(
            texts(&rows, Change::Removed),
            ["#proof(of: \"thm:main\")[It is.]".to_owned()]
        );
        assert_eq!(
            texts(&rows, Change::Added),
            ["#proof(of: \"thm:main\")[By induction on the order.]".to_owned()]
        );
        assert_eq!(extent(&rows).said(), "1 line added, 1 line taken out");

        // Everything else is the same document and says so.
        assert_eq!(
            rows.iter().filter(|row| row.change == Change::Kept).count(),
            4
        );
    }

    #[test]
    fn a_line_added_in_the_middle_leaves_the_rest_where_it_was() {
        let after = BEFORE.replace("#proof", "#lemma(id: \"lem:step\")[A step.]\n\n#proof");
        let rows = lines(BEFORE, &after);

        assert_eq!(
            texts(&rows, Change::Added),
            [
                "#lemma(id: \"lem:step\")[A step.]".to_owned(),
                String::new()
            ]
        );
        assert!(texts(&rows, Change::Removed).is_empty());
    }

    #[test]
    fn the_numbers_a_row_carries_are_the_numbers_of_its_own_version() {
        let after = BEFORE.replace("= Quotient groups\n", "= Quotients\n= In general\n");
        let rows = lines(BEFORE, &after);
        let kept = rows
            .iter()
            .find(|row| row.text.starts_with("#theorem"))
            .expect("the statement survived");

        assert_eq!(kept.before, Some(3));
        assert_eq!(kept.after, Some(4));
    }

    #[test]
    fn two_lines_are_compared_as_the_documents_they_become() {
        let before = line();
        let mut after = before.clone();
        let Block::Proof(proof) = &mut after.blocks[1] else {
            panic!("the second block is the proof");
        };
        proof.content = "By induction.".to_owned();
        let rows = between(&before.to_text(), &after.to_text());

        assert_eq!(texts(&rows, Change::Removed), ["It is.".to_owned()]);
        assert_eq!(texts(&rows, Change::Added), ["By induction.".to_owned()]);
        assert!(rows.iter().all(|row| !row.text.contains("\"format\"")));
    }

    #[test]
    fn what_is_not_a_line_is_compared_as_the_text_it_is() {
        let rows = between("not a line", "still not one");

        assert_eq!(texts(&rows, Change::Removed), ["not a line".to_owned()]);
    }

    #[test]
    fn what_the_guard_forbids_touching_is_said_in_line_numbers() {
        let document = line();
        let source = document.to_text();
        let frozen = frozen(&source);
        let rendered = render::typst(&document);
        let theorem = &rendered.blocks[0];
        let proof = &rendered.blocks[1];

        let first = rendered.line_of(theorem.whole.start);
        let last = rendered.line_of(theorem.whole.end);
        assert!(last > first, "a statement written over several lines");
        assert!(
            (first..=last).all(|at| frozen.contains(&at)),
            "frozen whole"
        );
        assert!(
            !frozen.contains(&rendered.line_of(proof.content.start)),
            "its proof is the agent's to write"
        );
        assert!(!frozen.contains(&1));
    }

    #[test]
    fn a_source_that_cannot_be_read_freezes_nothing_rather_than_guessing() {
        assert!(frozen("#theorem(id: \"thm:main\"").is_empty());
    }
}

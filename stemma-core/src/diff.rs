//! What changed between two versions of a source.
//!
//! A proposal arrives as a whole document, because that is the only
//! thing an agent can be trusted to produce and the only thing a person
//! can sign. What a person needs to *read*, though, is what moved — so
//! this says it, line by line, and the screens draw it.
//!
//! Lines and not characters: mathematics written in Typst is written in
//! paragraphs, and a diff that highlights three words inside a rewritten
//! statement invites reading the statement as mostly unchanged.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::line::{Line, Siglum};
use crate::source;

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
            (added, 0) => format!("{} added", lines(added)),
            (0, removed) => format!("{} taken out", lines(removed)),
            (added, removed) => {
                format!("{} added, {} taken out", lines(added), lines(removed))
            }
        }
    }
}

/// A count of lines, with the right noun.
fn lines(count: usize) -> String {
    match count {
        1 => "1 line".to_owned(),
        many => format!("{many} lines"),
    }
}

/// The two versions, line by line.
pub fn between(before: &str, after: &str) -> Vec<Row> {
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
    let mut rows = Vec::new();
    let (mut at_old, mut at_new) = (0, 0);

    for (old_at, new_at) in common {
        while at_old < old_at {
            rows.push(gone(&old, at_old));
            at_old += 1;
        }
        while at_new < new_at {
            rows.push(arrived(&new, at_new));
            at_new += 1;
        }

        rows.push(Row {
            change: Change::Kept,
            text: old[old_at].to_owned(),
            before: Some(old_at + 1),
            after: Some(new_at + 1),
        });
        at_old = old_at + 1;
        at_new = new_at + 1;
    }

    while at_old < old.len() {
        rows.push(gone(&old, at_old));
        at_old += 1;
    }
    while at_new < new.len() {
        rows.push(arrived(&new, at_new));
        at_new += 1;
    }

    rows
}

/// How much the two versions differ.
pub fn extent(rows: &[Row]) -> Extent {
    Extent {
        added: rows
            .iter()
            .filter(|row| row.change == Change::Added)
            .count(),
        removed: rows
            .iter()
            .filter(|row| row.change == Change::Removed)
            .count(),
    }
}

/// The lines a statement the line rests on occupies.
///
/// What the freeze guard forbids touching, said in line numbers, so a
/// screen can put corner marks around it without reading any
/// mathematics.
pub fn frozen(source: &str) -> BTreeSet<usize> {
    let Ok(read) = Line::read(source) else {
        return BTreeSet::new();
    };
    let Ok(declarations) = source::read(source) else {
        return BTreeSet::new();
    };
    let central: Vec<&Siglum> = read
        .central()
        .into_iter()
        .map(|node| &node.siglum)
        .collect();
    let mut found = BTreeSet::new();

    for declaration in declarations {
        let named = declaration
            .text("id")
            .and_then(|id| id.parse::<Siglum>().ok());

        if !named.is_some_and(|siglum| central.contains(&&siglum)) {
            continue;
        }

        let written = &source[declaration.span.clone()];
        let height = written.lines().count().max(1);

        for at in declaration.line..declaration.line + height {
            found.insert(at);
        }
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

    const BEFORE: &str = "= Quotient groups\n\n\
        #theorem(id: \"thm:main\", central: true)[Every finite group is finite.]\n\n\
        #proof(of: \"thm:main\")[It is.]\n";

    fn texts(rows: &[Row], change: Change) -> Vec<String> {
        rows.iter()
            .filter(|row| row.change == change)
            .map(|row| row.text.clone())
            .collect()
    }

    #[test]
    fn two_documents_that_are_the_same_moved_nothing() {
        let rows = between(BEFORE, BEFORE);

        assert!(rows.iter().all(|row| row.change == Change::Kept));
        assert!(extent(&rows).is_nothing());
        assert_eq!(extent(&rows).said(), "nothing changed");
    }

    #[test]
    fn large_unchanged_documents_are_not_reported_as_replacements() {
        let source = "A line.\n".repeat(LONGEST + 1);
        assert!(extent(&between(&source, &source)).is_nothing());
    }

    #[test]
    fn a_small_edit_in_a_large_document_retains_context_and_line_numbers() {
        let prefix = "Before.\n".repeat(LONGEST);
        let before = format!("{prefix}Old.\nAfter.\n");
        let after = format!("{prefix}New.\nAfter.\n");
        let rows = between(&before, &after);
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
        let rows = between(&before, &after);
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
        let rows = between(BEFORE, &after);

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
        let rows = between(BEFORE, &after);

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
        let rows = between(BEFORE, &after);
        let kept = rows
            .iter()
            .find(|row| row.text.starts_with("#theorem"))
            .expect("the statement survived");

        assert_eq!(kept.before, Some(3));
        assert_eq!(kept.after, Some(4));
    }

    #[test]
    fn what_the_guard_forbids_touching_is_said_in_line_numbers() {
        let frozen = frozen(BEFORE);

        assert!(frozen.contains(&3), "the central statement is on line 3");
        assert!(!frozen.contains(&5), "its proof is the agent's to write");
        assert!(!frozen.contains(&1));
    }

    #[test]
    fn a_statement_written_over_several_lines_is_frozen_whole() {
        let source = "= A line\n\n\
            #theorem(\n  id: \"thm:main\",\n  central: true,\n)[\n  Every finite group is finite.\n]\n";
        let frozen = frozen(source);

        assert!(frozen.contains(&3));
        assert!(frozen.contains(&8), "down to its closing bracket");
    }

    #[test]
    fn a_source_that_cannot_be_read_freezes_nothing_rather_than_guessing() {
        assert!(frozen("#theorem(id: \"thm:main\"").is_empty());
    }
}

//! Where each thing sits in a line's descent.
//!
//! A stemma codicum: what a line rests on at the top, what rests on it
//! below, and the black boxes between as bands rather than as their
//! contents. A line with forty lemmas has to stay readable, so the
//! forty are never here — only the definitions, the statements the line
//! rests on, and the boxes.
//!
//! What is computed here is position and nothing else. Nothing in this
//! module knows about pixels, colour or hatching: a drawing decides how
//! wide a level is, and the same placement serves a page, an editor and
//! a printout.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::id::BlockId;
use crate::line::{BlackBox, Line, Node, NodeKind, Siglum};

/// One thing the descent shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "shown", rename_all = "snake_case")]
pub enum Shown {
    /// Something given a meaning.
    Definition {
        /// What it is called.
        siglum: Siglum,
    },
    /// A statement the line rests on.
    Central {
        /// What it is called.
        siglum: Siglum,
        /// Whether Lean compiles an argument for it.
        verified: bool,
    },
    /// What lies between two central statements.
    Box {
        /// Its two ends and how much of it is filled in.
        black_box: BlackBox,
    },
}

impl Shown {
    /// What it is called, when it is called anything.
    pub fn siglum(&self) -> Option<&Siglum> {
        match self {
            Self::Definition { siglum } | Self::Central { siglum, .. } => Some(siglum),
            Self::Box { .. } => None,
        }
    }
}

/// One thing, and where it sits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    /// What is there.
    pub what: Shown,
    /// How far down it is, counting from nothing.
    pub level: usize,
    /// Where it sits on that level, counting from the left.
    pub across: usize,
}

/// What one line between two things means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Descends {
    /// The lower one says it leans on the upper one.
    Uses,
    /// The descent runs through a black box: what the box is for.
    Through,
}

/// One line drawn between two things.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Where it comes from.
    pub from: usize,
    /// Where it goes.
    pub to: usize,
    /// What it means.
    pub means: Descends,
}

/// A proved licence to substitute one definition for another.
///
/// It spans between two definitions on their own level rather than
/// hanging below them, because it is not something either of them rests
/// on. Topology tells a bridge from a theorem that merely relates two
/// definitions; no label has to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// One end.
    pub left: usize,
    /// The other.
    pub right: usize,
}

/// A whole descent, placed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Descent {
    /// Everything shown, in the order the line declares it.
    pub placed: Vec<Placed>,
    /// Every line drawn between two of them.
    pub edges: Vec<Edge>,
    /// Every bridge spanning two definitions.
    pub spans: Vec<Span>,
    /// How many levels deep it goes.
    pub levels: usize,
    /// How many things sit on its widest level.
    pub width: usize,
}

impl Descent {
    /// Whether there is anything to draw.
    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    /// Where one siglum ended up.
    pub fn at(&self, siglum: &Siglum) -> Option<usize> {
        self.placed
            .iter()
            .position(|placed| placed.what.siglum() == Some(siglum))
    }
}

/// Places a line's descent.
///
/// `verified` says which statements Lean compiles an argument for, and
/// `bridges` the pairs of definitions an audited substitution joins.
/// Both are facts from outside the source, and neither changes where
/// anything sits: they change what is drawn there.
pub fn of(line: &Line, verified: &[Siglum], bridges: &[(Siglum, Siglum)]) -> Descent {
    let mut placing = Placing::default();
    let boxes = line.boxes();
    let mut remaining = boxes.iter();
    let mut pending = remaining.next();
    // Where the last central statement was put: the next box hangs below it.
    let mut last_central: Option<usize> = None;

    for node in &line.nodes {
        if node.siglum.kind == NodeKind::Definition {
            let shown = Shown::Definition {
                siglum: node.siglum.clone(),
            };
            placing.put_node(node, shown, placing.below(node));
            continue;
        }

        if !node.is_central() {
            // What is inside a box is not drawn. That is the whole point
            // of a box: the line's shape is the statements it rests on.
            continue;
        }

        // The box before this statement, which the descent runs through.
        let through = pending
            .take()
            .map(|black_box| placing.put_box(black_box, last_central));

        let level = placing
            .below(node)
            .max(through.map_or(0, |(_, level)| level + 1));
        let shown = Shown::Central {
            siglum: node.siglum.clone(),
            verified: verified.contains(&node.siglum),
        };
        let at = placing.put_node(node, shown, level);

        if let Some((box_at, _)) = through {
            placing.join(box_at, at, Descends::Through);
        }

        last_central = Some(at);
        pending = remaining.next();
    }

    // The line's open end: whatever comes after the last central
    // statement is a box with one boundary, which is the ordinary state
    // of work in progress.
    for black_box in pending.into_iter().chain(remaining) {
        placing.put_box(black_box, last_central);
    }

    let Placing {
        mut descent,
        counted,
        ..
    } = placing;

    for (left, right) in bridges {
        if let (Some(left), Some(right)) = (descent.at(left), descent.at(right)) {
            descent.spans.push(Span { left, right });
        }
    }

    descent.width = counted.values().copied().max().unwrap_or(0);

    descent
}

/// A descent while it is being placed, with what placing it needs to
/// remember.
#[derive(Default)]
struct Placing {
    /// What has been placed so far.
    descent: Descent,
    /// How many things each level holds so far.
    counted: BTreeMap<usize, usize>,
    /// The level of every node placed, by siglum.
    level_of: BTreeMap<Siglum, usize>,
    /// The level of every node placed, by the block that declares it.
    level_of_block: BTreeMap<BlockId, usize>,
    /// Where every node was placed.
    place_of: BTreeMap<Siglum, usize>,
}

impl Placing {
    /// Puts one thing at the next free place on a level, answering where.
    fn put(&mut self, what: Shown, level: usize) -> usize {
        let across = self.counted.entry(level).or_default();
        self.descent.placed.push(Placed {
            what,
            level,
            across: *across,
        });
        *across += 1;
        self.descent.levels = self.descent.levels.max(level + 1);

        self.descent.placed.len() - 1
    }

    /// Draws one line between two things already placed.
    fn join(&mut self, from: usize, to: usize, means: Descends) {
        self.descent.edges.push(Edge { from, to, means });
    }

    /// The first level below everything a node leans on.
    fn below(&self, node: &Node) -> usize {
        node.uses
            .iter()
            .filter_map(|used| self.level_of.get(used))
            .map(|level| level + 1)
            .max()
            .unwrap_or(0)
    }

    /// Puts a node on a level and draws the lines down to it from what
    /// it leans on, answering where it was put.
    fn put_node(&mut self, node: &Node, shown: Shown, level: usize) -> usize {
        let at = self.put(shown, level);

        self.level_of.insert(node.siglum.clone(), level);
        self.level_of_block.insert(node.block, level);
        self.place_of.insert(node.siglum.clone(), at);

        for used in &node.uses {
            if let Some(&from) = self.place_of.get(used) {
                self.join(from, at, Descends::Uses);
            }
        }

        at
    }

    /// Puts a box just below the central statement it comes after, and
    /// runs the descent into it from `from`, answering where it was put
    /// and on which level.
    fn put_box(&mut self, black_box: &BlackBox, from: Option<usize>) -> (usize, usize) {
        let level = black_box
            .after
            .as_ref()
            .and_then(|after| self.level_of_block.get(after))
            .map_or(0, |level| level + 1);
        let shown = Shown::Box {
            black_box: black_box.clone(),
        };
        let at = self.put(shown, level);

        if let Some(from) = from {
            self.join(from, at, Descends::Through);
        }

        (at, level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::document::{Block, Document};

    /// A definition, two statements the line rests on, and a lemma
    /// inside the box between them.
    fn source() -> Document {
        let group = Block::definition(Some(siglum("def:group")), "A group.").titled("Group");
        let first = Block::theorem("Theorem", Some(siglum("thm:first")), "First.")
            .central(true)
            .carrying_proof(false)
            .using(&group);
        let step = Block::theorem("Lemma", Some(siglum("lem:step")), "A step.")
            .carrying_proof(false)
            .using(&group);
        let second = Block::theorem("Theorem", Some(siglum("thm:second")), "By @thm:first.")
            .central(true)
            .carrying_proof(false);

        Document::new("Quotient groups", None)
            .with(group)
            .with(first)
            .with(step)
            .with(second)
    }

    fn siglum(raw: &str) -> Siglum {
        raw.parse().unwrap()
    }

    fn descent() -> Descent {
        of(&Line::of(&source()).unwrap(), &[], &[])
    }

    #[test]
    fn each_box_hangs_below_one_statement_and_leads_into_the_next() {
        let descent = descent();
        let first = descent.at(&siglum("thm:first")).unwrap();
        let second = descent.at(&siglum("thm:second")).unwrap();
        let boxes: Vec<usize> = (0..descent.placed.len())
            .filter(|at| matches!(descent.placed[*at].what, Shown::Box { .. }))
            .collect();
        let through: Vec<(usize, usize)> = descent
            .edges
            .iter()
            .filter(|edge| edge.means == Descends::Through)
            .map(|edge| (edge.from, edge.to))
            .collect();

        assert_eq!(
            through,
            [
                (boxes[0], first),
                (first, boxes[1]),
                (boxes[1], second),
                (second, boxes[2]),
            ]
        );
        assert_eq!(
            descent.placed[boxes[1]].level,
            descent.placed[first].level + 1
        );
        assert_eq!(
            descent.placed[boxes[2]].level,
            descent.placed[second].level + 1
        );
    }

    #[test]
    fn what_a_statement_uses_is_drawn_down_to_it() {
        let descent = descent();
        let group = descent.at(&siglum("def:group")).unwrap();
        let first = descent.at(&siglum("thm:first")).unwrap();
        let second = descent.at(&siglum("thm:second")).unwrap();

        assert!(descent.edges.contains(&Edge {
            from: group,
            to: first,
            means: Descends::Uses,
        }));
        assert!(
            descent.edges.contains(&Edge {
                from: first,
                to: second,
                means: Descends::Uses,
            }),
            "a citation in the text is a use too"
        );
        assert_eq!(
            descent.width, 2,
            "the definition shares its level with a box"
        );
    }

    #[test]
    fn the_forty_lemmas_inside_a_box_are_never_drawn() {
        let descent = descent();

        assert_eq!(descent.at(&siglum("lem:step")), None);
        assert!(descent.at(&siglum("def:group")).is_some());
        assert!(descent.at(&siglum("thm:first")).is_some());
    }

    #[test]
    fn what_a_statement_rests_on_is_above_it() {
        let descent = descent();
        let level = |raw: &str| descent.placed[descent.at(&siglum(raw)).unwrap()].level;

        assert_eq!(level("def:group"), 0);
        assert!(level("thm:first") > level("def:group"));
        assert!(level("thm:second") > level("thm:first"));
    }

    #[test]
    fn the_descent_runs_through_the_boxes_between_the_statements() {
        let descent = descent();
        let boxes: Vec<&Placed> = descent
            .placed
            .iter()
            .filter(|placed| matches!(placed.what, Shown::Box { .. }))
            .collect();

        assert_eq!(boxes.len(), 3, "before, between and after");
        assert!(
            descent
                .edges
                .iter()
                .any(|edge| edge.means == Descends::Through)
        );

        let first = descent.at(&siglum("thm:first")).unwrap();
        let second = descent.at(&siglum("thm:second")).unwrap();
        let between = descent.placed[first].level + 1;

        assert!(
            descent
                .placed
                .iter()
                .any(|placed| matches!(placed.what, Shown::Box { .. }) && placed.level == between),
            "the box between them sits between them"
        );
        assert!(descent.placed[second].level > between);
    }

    #[test]
    fn a_verified_statement_says_so_where_it_is_drawn() {
        let descent = of(&Line::of(&source()).unwrap(), &[siglum("thm:first")], &[]);
        let at = descent.at(&siglum("thm:first")).unwrap();

        assert_eq!(
            descent.placed[at].what,
            Shown::Central {
                siglum: siglum("thm:first"),
                verified: true
            }
        );
    }

    #[test]
    fn a_bridge_spans_two_definitions_rather_than_hanging_below_them() {
        let mut source = source();
        source.blocks.insert(
            1,
            Block::definition(Some(siglum("def:quotient")), "A quotient.").titled("Quotient"),
        );
        let line = Line::of(&source).unwrap();
        let descent = of(&line, &[], &[(siglum("def:group"), siglum("def:quotient"))]);

        assert_eq!(descent.spans.len(), 1);

        let span = descent.spans[0];
        assert_eq!(
            descent.placed[span.left].level, descent.placed[span.right].level,
            "a bridge is between equals"
        );
        assert!(
            !descent
                .edges
                .iter()
                .any(|edge| edge.from == span.left && edge.to == span.right)
        );
    }

    #[test]
    fn a_line_that_declares_nothing_has_nothing_to_draw() {
        let descent = of(&Line { nodes: Vec::new() }, &[], &[]);

        assert!(descent.is_empty());
        assert_eq!(descent.levels, 0);
    }
}

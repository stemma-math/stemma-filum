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

use crate::line::{BlackBox, Line, NodeKind, Siglum};

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
    let mut descent = Descent::default();
    let mut level_of: BTreeMap<Siglum, usize> = BTreeMap::new();
    let mut place_of: BTreeMap<Siglum, usize> = BTreeMap::new();
    let mut after_box: Option<(usize, usize)> = None;
    let mut counted: BTreeMap<usize, usize> = BTreeMap::new();
    let boxes = line.boxes();
    let mut remaining = boxes.iter();
    let mut pending = remaining.next();

    let put =
        |descent: &mut Descent, counted: &mut BTreeMap<usize, usize>, what: Shown, level: usize| {
            let across = counted.entry(level).or_default();
            descent.placed.push(Placed {
                what,
                level,
                across: *across,
            });
            *across += 1;
            descent.levels = descent.levels.max(level + 1);

            descent.placed.len() - 1
        };

    for node in &line.nodes {
        let below = |level_of: &BTreeMap<Siglum, usize>| {
            node.uses
                .iter()
                .filter_map(|used| level_of.get(used))
                .map(|level| level + 1)
                .max()
                .unwrap_or(0)
        };

        if node.siglum.kind == NodeKind::Definition {
            let level = below(&level_of);
            let at = put(
                &mut descent,
                &mut counted,
                Shown::Definition {
                    siglum: node.siglum.clone(),
                },
                level,
            );

            level_of.insert(node.siglum.clone(), level);
            place_of.insert(node.siglum.clone(), at);
            draw(&mut descent, node, &place_of);

            continue;
        }

        if !node.is_central() {
            // What is inside a box is not drawn. That is the whole point
            // of a box: the line's shape is the statements it rests on.
            continue;
        }

        // The box before this statement, which the descent runs through.
        let mut through = None;

        if let Some(black_box) = pending.take() {
            let level = black_box
                .after
                .as_ref()
                .and_then(|after| level_of.get(after))
                .map(|level| level + 1)
                .unwrap_or(0);
            let at = put(
                &mut descent,
                &mut counted,
                Shown::Box {
                    black_box: black_box.clone(),
                },
                level,
            );

            if let Some(from) = after_box {
                descent.edges.push(Edge {
                    from: from.0,
                    to: at,
                    means: Descends::Through,
                });
            }

            through = Some((at, level));
        }

        let level = below(&level_of).max(through.map(|(_, level)| level + 1).unwrap_or(0));
        let at = put(
            &mut descent,
            &mut counted,
            Shown::Central {
                siglum: node.siglum.clone(),
                verified: verified.contains(&node.siglum),
            },
            level,
        );

        level_of.insert(node.siglum.clone(), level);
        place_of.insert(node.siglum.clone(), at);
        draw(&mut descent, node, &place_of);

        if let Some((box_at, _)) = through {
            descent.edges.push(Edge {
                from: box_at,
                to: at,
                means: Descends::Through,
            });
        }

        after_box = Some((at, level));
        pending = remaining.next();
    }

    // The line's open end: whatever comes after the last central
    // statement is a box with one boundary, which is the ordinary state
    // of work in progress.
    for black_box in pending.into_iter().chain(remaining) {
        let level = black_box
            .after
            .as_ref()
            .and_then(|after| level_of.get(after))
            .map(|level| level + 1)
            .unwrap_or(0);
        let at = put(
            &mut descent,
            &mut counted,
            Shown::Box {
                black_box: black_box.clone(),
            },
            level,
        );

        if let Some(from) = after_box {
            descent.edges.push(Edge {
                from: from.0,
                to: at,
                means: Descends::Through,
            });
        }
    }

    for (left, right) in bridges {
        if let (Some(left), Some(right)) = (descent.at(left), descent.at(right)) {
            descent.spans.push(Span { left, right });
        }
    }

    descent.width = counted.values().copied().max().unwrap_or(0);

    descent
}

/// Draws the lines from what a node leans on down to the node itself.
fn draw(descent: &mut Descent, node: &crate::line::Node, place_of: &BTreeMap<Siglum, usize>) {
    let to = descent.placed.len() - 1;

    for used in &node.uses {
        if let Some(from) = place_of.get(used) {
            descent.edges.push(Edge {
                from: *from,
                to,
                means: Descends::Uses,
            });
        }
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

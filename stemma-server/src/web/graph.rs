//! The line drawn as a stemma codicum.
//!
//! Definitions, the statements the line rests on, and the closed boxes
//! between them — never the forty lemmas inside a box, because a graph
//! that showed them would be unreadable exactly when it mattered.
//!
//! The placement is worked out here and the drawing happens in the
//! browser, on a canvas, from the placement this page hands it. With
//! nothing running, the same block is the list of what the graph would
//! have shown: the identity survives, and nothing is lost but the
//! picture.

use maud::{Markup, PreEscaped, html};
use stemma_core::artefact::LineDigest;
use stemma_core::descent::{Descent, Shown, of};
use stemma_core::line::Siglum;
use stemma_core::model::{BoxState, Verification};

/// The whole block: the drawing, its placement, and its list.
pub fn drawn(digest: &LineDigest, bridges: &[(Siglum, Siglum)]) -> Markup {
    let descent = of(&digest.line, &verified(digest), bridges);

    if descent.is_empty() {
        return html! {};
    }

    let placement = serde_json::to_string(&descent).unwrap_or_else(|_| "null".to_owned());

    html! {
        figure class="descent" {
            figcaption class="label" { "The descent" }

            canvas class="descent__canvas"
                   role="img"
                   aria-label=(described(&descent)) {}

            script type="application/json" class="descent__placement" {
                (PreEscaped(placement))
            }

            // What the drawing is of, for a reader with nothing running.
            // The script takes it away once it has drawn.
            ol class="descent__list" {
                @for placed in &descent.placed {
                    li {
                        @match &placed.what {
                            Shown::Definition { siglum } => {
                                span class="identifier" { (siglum.to_string()) }
                                " definition"
                            }
                            Shown::Central { siglum, verified } => {
                                span class="identifier" { (siglum.to_string()) }
                                " central"
                                @if *verified { " · Lean verified" }
                            }
                            Shown::Box { black_box } => {
                                "a black box, "
                                (match black_box.state {
                                    BoxState::Empty => "with nothing in it",
                                    BoxState::FilledByHand => "filled by hand",
                                    BoxState::FilledByAgent => "filled by an agent",
                                    BoxState::Verified => "verified",
                                })
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Which of the line's statements Lean compiles an argument for.
fn verified(digest: &LineDigest) -> Vec<Siglum> {
    let Some(lean) = digest.lean.as_ref() else {
        return Vec::new();
    };

    digest
        .line
        .nodes
        .iter()
        .map(|node| node.siglum.clone())
        .filter(|siglum| lean.verification_of(siglum) == Verification::Verified)
        .collect()
}

/// What the drawing is, for somebody who cannot see it.
fn described(descent: &Descent) -> String {
    let definitions = descent
        .placed
        .iter()
        .filter(|placed| matches!(placed.what, Shown::Definition { .. }))
        .count();
    let central = descent
        .placed
        .iter()
        .filter(|placed| matches!(placed.what, Shown::Central { .. }))
        .count();
    let boxes = descent
        .placed
        .iter()
        .filter(|placed| matches!(placed.what, Shown::Box { .. }))
        .count();

    format!(
        "The descent of this line: {definitions} definitions, \
         {central} statements it rests on, and {boxes} black boxes, \
         over {} levels.",
        descent.levels
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use stemma_core::line::{Line, Name};

    use stemma_core::document::{Block, Document};

    /// A definition, two statements the line rests on, and a lemma
    /// inside the box between them.
    fn source() -> Document {
        let statement = |named: &str, environment: &str, content: &str| {
            Block::theorem(environment, Some(named.parse().unwrap()), content).carrying_proof(false)
        };
        let group =
            Block::definition(Some("def:group".parse().unwrap()), "A group.").titled("Group");
        let first = statement("thm:first", "Theorem", "First.")
            .central(true)
            .using(&group);

        Document::new("Quotient groups", None)
            .with(group)
            .with(first)
            .with(statement("lem:step", "Lemma", "A step."))
            .with(statement("thm:second", "Theorem", "By @thm:first.").central(true))
    }

    fn digest(source: &Document) -> LineDigest {
        LineDigest {
            name: Name::parse("quotient-groups").unwrap(),
            title: Some(source.title.clone()),
            description: None,
            line: Line::of(source).unwrap(),
            lean: None,
        }
    }

    #[test]
    fn the_block_carries_both_the_placement_and_the_list_it_falls_back_to() {
        let drawn = drawn(&digest(&source()), &[]).into_string();

        assert!(drawn.contains("descent__canvas"));
        assert!(drawn.contains("descent__placement"));
        assert!(drawn.contains("def:group"));
        assert!(drawn.contains("a black box"));
        let list = &drawn[drawn.find("descent__list").expect("the list")..];
        assert!(
            !list.contains("lem:step"),
            "what is inside a box is not one of the things shown"
        );
    }

    #[test]
    fn a_line_that_declares_nothing_draws_nothing() {
        assert_eq!(
            drawn(&digest(&Document::new("Nothing yet", None)), &[]).into_string(),
            ""
        );
    }

    #[test]
    fn the_drawing_says_what_it_is_for_somebody_who_cannot_see_it() {
        let drawn = drawn(&digest(&source()), &[]).into_string();

        assert!(drawn.contains("1 definitions"));
        assert!(drawn.contains("2 statements it rests on"));
    }
}

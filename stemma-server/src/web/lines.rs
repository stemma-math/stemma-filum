//! Showing a line to somebody whose own machine is switched off.
//!
//! What is drawn comes from an artefact a machine built and handed over,
//! named by the commit it was built from. The instance compiles nothing,
//! so what it can show is what the source declares: the definitions, the
//! statements the line rests on, and the boxes between them.
//!
//! The prose is not here. It arrives with the composed document, which
//! is a different artefact, and a page that showed a name where the
//! mathematics should be would be pretending to be that document.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};
use stemma_core::artefact::{Kind, LineDigest, Nodes, Scope};
use stemma_core::id::{GroupId, Slug};
use stemma_core::line::{Element, Name};
use stemma_core::model::BoxState;
use stemma_core::repository::Side;

use super::layout::{Chrome, page};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::artefacts::Shelf;
use crate::store::artefacts as index;
use crate::store::groups::{self, Group};

/// One line of work.
pub async fn view(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((group, side, line)): Path<(String, String, String)>,
) -> Result<Response, WebError> {
    let (group, scope, side) = admit(&instance, &signed, &group, &side).await?;
    let Ok(name) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };

    let Some(nodes) = read(&instance, &group.id, &scope).await? else {
        return Err(WebError::NotFound);
    };

    let Some(digest) = nodes.line(&name).cloned() else {
        return Err(WebError::NotFound);
    };

    let policy = instance.policy().await?;
    let chrome = Chrome::anonymous(
        digest.shown_as().to_owned(),
        super::front::instance_name(&instance, &policy),
    )
    .signed_in(signed.user.clone(), signed.maintainer);

    Ok(page(&chrome, body(&group, side, &digest, &nodes)).into_response())
}

/// Which repository was asked for, and whether this reader may have it.
async fn admit(
    instance: &Instance,
    signed: &SignedIn,
    group: &str,
    side: &str,
) -> Result<(Group, Scope, Side), WebError> {
    let Ok(slug) = Slug::parse(group) else {
        return Err(WebError::NotFound);
    };

    let side = match side {
        "shared" => Side::Shared,
        "private" => Side::Private,
        _ => return Err(WebError::NotFound),
    };

    let id = GroupId::new(slug);
    let asking = signed.user.id.clone();
    let wanted = id.clone();

    let found = instance
        .read(move |connection| {
            let Some(group) = groups::by_id(connection, &wanted)? else {
                return Ok(None);
            };

            Ok(groups::role_of(connection, &id, &asking)?.map(|_| group))
        })
        .await?;

    // Somebody outside the group is told there is nothing here rather
    // than that there is something they may not see.
    let group = found.ok_or(WebError::NotFound)?;

    Ok((group, Scope::of(side, &signed.user.id), side))
}

/// The most recent thing a machine built for one repository.
pub async fn read(
    instance: &Instance,
    group: &GroupId,
    scope: &Scope,
) -> Result<Option<Nodes>, WebError> {
    let shelf = Shelf::new(instance.settings.paths.artefacts_dir());
    let group = group.clone();
    let scope = scope.clone();

    let built = {
        let group = group.clone();
        let scope = scope.clone();

        instance
            .read(move |connection| index::latest(connection, &group, &scope, Kind::Nodes))
            .await?
    };

    let Some(built) = built else {
        return Ok(None);
    };

    let content = instance
        .on_disk(move || shelf.get(&group, &scope, Kind::Nodes, &built.commit))
        .await?;

    let Some(content) = content else {
        return Ok(None);
    };

    // An artefact this build cannot read is not an error for the reader:
    // it means the machine that built it is running a newer release, and
    // the page says so where the line would have been.
    Ok(Nodes::parse(&String::from_utf8_lossy(&content)).ok())
}

/// The line itself.
fn body(group: &Group, side: Side, digest: &LineDigest, nodes: &Nodes) -> Markup {
    let boxes = digest.line.boxes();
    let central = digest.line.central();

    html! {
        div class="stack" {
            p class="label" {
                a href={ "/g/" (group.slug()) } { (group.name) }
                " · " (side)
            }
            h1 { (digest.shown_as()) }
            p class="quiet identifier" { "lines/" (digest.name) }
        }

        div class="apparatus" {
            div class="apparatus__gutter" {
                span { "the line" }
                span class="identifier" { (short(&nodes.commit)) }
            }

            div class="apparatus__document" {
                @if digest.line.nodes.is_empty() {
                    p class="empty" {
                        "This line declares nothing yet. What appears here is what its "
                        "source says: the definitions, the statements the line rests on, "
                        "and the boxes between them."
                    }
                }

                @for element in digest.line.skeleton() {
                    @match element {
                        Element::Node(siglum) => {
                            @if let Some(node) = digest.line.node(&siglum) {
                                (drawn(node))
                            }
                        }
                        Element::Box(black_box) => (drawn_box(&black_box)),
                    }
                }
            }

            div class="apparatus__register" {
                (register(digest, &central, &boxes, nodes))
            }
        }
    }
}

/// One node, with what is known about it in the margin.
fn drawn(node: &stemma_core::line::Node) -> Markup {
    let central = node.is_central();

    html! {
        div class=@if central { "node node--central" } @else { "node" } {
            div class="node__mark" {
                span class="identifier" { (node.siglum.to_string()) }
                span { (node.siglum.kind) }
                @if central {
                    // Weight and this word are the same fact said twice,
                    // because colour alone must never carry a state.
                    span { "central" }
                }
                @if node.proved {
                    span { "proved here" }
                }
            }

            div class="node__body" {
                p class="node__name" {
                    @match &node.name {
                        Some(name) => (name),
                        None => (node.siglum.to_string()),
                    }
                }

                @if !node.uses.is_empty() {
                    p class="node__uses" {
                        "uses "
                        @for (at, used) in node.uses.iter().enumerate() {
                            @if at > 0 { " · " }
                            (used.to_string())
                        }
                    }
                }
            }
        }
    }
}

/// One box, drawn as the declared absence it is.
fn drawn_box(black_box: &stemma_core::line::BlackBox) -> Markup {
    let filled = black_box.state != BoxState::Empty;

    html! {
        div class=@if filled { "box box--filled" } @else { "box" } {
            div class="node__mark" {
                span { "black box" }
            }

            div class="box__band" {
                @if filled {
                    span class="box__count" { (black_box.interior.len()) }
                    span {
                        @match black_box.interior.len() {
                            1 => "statement between the two it rests between",
                            _ => "statements between the two it rests between",
                        }
                    }
                } @else {
                    // Nothing known and nothing attempted look the same
                    // here, because in both cases nothing is known.
                    span { "nothing here yet" }
                }
            }
        }
    }
}

/// The blocks beside the document.
fn register(
    digest: &LineDigest,
    central: &[&stemma_core::line::Node],
    boxes: &[stemma_core::line::BlackBox],
    nodes: &Nodes,
) -> Markup {
    let empty = boxes
        .iter()
        .filter(|black_box| black_box.state == BoxState::Empty)
        .count();

    html! {
        div class="block" {
            p class="label" { "What the line rests on" }

            @if central.is_empty() {
                p class="empty" { "Nothing is declared central yet." }
            } @else {
                ul class="register" {
                    @for node in central {
                        li {
                            span class="register__name" {
                                @match &node.name {
                                    Some(name) => (name),
                                    None => (node.siglum.to_string()),
                                }
                            }
                            span class="quiet identifier" { (node.siglum.to_string()) }
                        }
                    }
                }
            }
        }

        div class="block" {
            p class="label" { "Boxes" }
            p class="numeric" {
                (boxes.len()) " in all, " (empty) " with nothing in them"
            }
        }

        div class="block" {
            p class="label" { "Built" }
            p class="quiet identifier" { (nodes.commit) }
            @match &nodes.typst {
                Some(pinned) => p class="quiet" { "typst " (pinned) " is pinned" }
                None => p class="quiet" { "nothing is pinned yet" }
            }
            p class="quiet" {
                (digest.line.nodes.len()) " statements, "
                (digest.line.dependencies().len()) " dependencies"
            }
        }
    }
}

/// A commit, short enough to read.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(8)]
}

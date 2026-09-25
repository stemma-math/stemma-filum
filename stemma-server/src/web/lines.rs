//! Showing a line to somebody whose own machine is switched off.
//!
//! What is drawn comes from an artefact a machine built and handed over,
//! named by the commit it was built from. The instance compiles nothing,
//! so what it can show is what the source declares: the definitions, the
//! statements the line rests on, and the boxes between them.
//!
//! The prose is shown as the Typst document the line at the head of the
//! repository becomes, uncompiled. A composed document would be a different
//! artefact, and a page that typeset the mathematics itself would be
//! pretending to be that document.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};
use stemma_core::artefact::{Kind, LineDigest, Nodes, Scope};
use stemma_core::id::{GroupId, Slug};
use stemma_core::job::Task;
use stemma_core::line::{Element, Name};
use stemma_core::model::BoxState;
use stemma_core::repository::Side;

use super::jobs::Offer;
use super::layout::{page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::artefacts::Shelf;
use crate::store::artefacts as index;
use crate::store::claims::{self, Claim};
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

    let written = written(&instance, &group.id, side, &signed, &name).await?;
    let built = read(&instance, &group.id, &scope).await?;
    let digest = match (&written, built.as_ref()) {
        // A build of the commit the repository is at says everything the
        // source does and more.
        (Some(written), Some(nodes)) if nodes.commit == written.commit => nodes
            .line(&name)
            .cloned()
            .unwrap_or_else(|| declared(&name, &written.source)),
        // Nothing has built it yet, which is the ordinary state of a
        // line somebody started a minute ago, or what was built is older
        // than what was pushed since. What the source declares is
        // readable without compiling anything, so the line is here as it
        // stands — with everything a build adds, and only that, missing.
        (Some(written), _) => declared(&name, &written.source),
        (None, built) => built
            .and_then(|nodes| nodes.line(&name))
            .cloned()
            .ok_or(WebError::NotFound)?,
    };
    let nodes = match built {
        Some(nodes) => nodes,
        None => Nodes::new(group.id.clone(), side, String::new()),
    };

    let offers = super::jobs::offers(&instance, &group.id).await?;
    let writing = {
        let owner = signed.user.id.clone();
        let id = group.id.clone();
        let name = name.clone();

        instance
            .read(move |connection| claims::holder(connection, &id, side, name.as_str(), &owner))
            .await?
    };
    let chrome = super::layout::around(&instance, &signed, digest.shown_as().to_owned())
        .await?
        .at(format!("/g/{}", group.slug()));

    let launchers = Anchored {
        signed: &signed,
        group: group.slug().clone(),
        side,
        line: digest.name.clone(),
        commit: nodes.commit.clone(),
        offers,
    };

    Ok(page(
        &chrome,
        body(&Viewing {
            group: &group,
            side,
            digest: &digest,
            nodes: &nodes,
            launchers: &launchers,
            writing: writing.as_ref(),
            signed: &signed,
            written: written.as_ref(),
        }),
    )
    .into_response())
}

/// Every line one repository holds, as its sources declare them.
pub async fn unbuilt_in(
    instance: &Instance,
    group: &GroupId,
    side: Side,
    signed: &SignedIn,
) -> Result<Vec<LineDigest>, WebError> {
    let repositories = instance.repositories();
    let path = match side {
        Side::Shared => repositories.shared(group),
        Side::Private => repositories.personal(group, &signed.user.id),
    };

    let names: Vec<Name> = instance
        .blocking(move || {
            let Some(head) = crate::git::head_commit(&path) else {
                return Ok(Vec::new());
            };

            Ok(crate::git::lines_at(&path, &head)?
                .iter()
                .filter_map(|name| Name::parse(name).ok())
                .collect())
        })
        .await?;

    let mut found = Vec::new();

    for name in names {
        if let Some(digest) = unbuilt(instance, group, side, signed, &name).await? {
            found.push(digest);
        }
    }

    Ok(found)
}

/// A line as its source declares it, for one nothing has built yet.
async fn unbuilt(
    instance: &Instance,
    group: &GroupId,
    side: Side,
    signed: &SignedIn,
    name: &Name,
) -> Result<Option<LineDigest>, WebError> {
    Ok(written(instance, group, side, signed, name)
        .await?
        .map(|written| declared(name, &written.source)))
}

/// A line's source, exactly as it stands at the head of its repository.
struct Written {
    /// The commit it was read from.
    commit: String,
    /// What is in the line's entry file there.
    source: String,
}

/// Reads a line's source at the head of its repository.
async fn written(
    instance: &Instance,
    group: &GroupId,
    side: Side,
    signed: &SignedIn,
    name: &Name,
) -> Result<Option<Written>, WebError> {
    let repositories = instance.repositories();
    let path = match side {
        Side::Shared => repositories.shared(group),
        Side::Private => repositories.personal(group, &signed.user.id),
    };
    let inside = format!(
        "{}/{}/{}",
        stemma_core::repository::LINES,
        name,
        stemma_core::repository::ENTRY
    );

    instance
        .blocking(move || {
            let Some(commit) = crate::git::head_commit(&path) else {
                return Ok(None);
            };
            let Ok(source) = crate::git::show(&path, &commit, &inside) else {
                return Ok(None);
            };

            Ok(Some(Written { commit, source }))
        })
        .await
}

/// What a source declares, with nothing a build would have added.
///
/// The instance compiles nothing and this does not either: reading the
/// declarations is the same work the editor does between keystrokes.
/// What is missing from the answer is everything a build would have
/// added — the Lean result, the composed document — and the page says
/// so where it would have shown them.
fn declared(name: &Name, source: &str) -> LineDigest {
    let document = stemma_core::document::Document::parse(source).ok();

    LineDigest {
        name: name.clone(),
        title: document
            .as_ref()
            .map(|document| document.title.clone())
            .filter(|title| !title.trim().is_empty()),
        description: document
            .as_ref()
            .and_then(|document| document.description.clone()),
        // A source with a fault in it is still a line somebody is
        // writing; what it declares so far is nothing.
        line: document
            .and_then(|document| stemma_core::line::Line::of(&document).ok())
            .unwrap_or(stemma_core::line::Line { nodes: Vec::new() }),
        lean: None,
    }
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
        .blocking(move || shelf.get(&group, &scope, Kind::Nodes, &built.commit))
        .await?;

    let Some(content) = content else {
        return Ok(None);
    };

    // An artefact this build cannot read is not an error for the reader:
    // it means the machine that built it is running a newer release, and
    // the page says so where the line would have been.
    Ok(Nodes::parse(&String::from_utf8_lossy(&content)).ok())
}

/// What a launcher needs, carried down to the thing it is anchored to.
///
/// An agent action is offered where its effect will land, so there is no
/// one screen it belongs on: it travels with the box or the statement.
struct Anchored<'a> {
    signed: &'a SignedIn,
    group: Slug,
    side: Side,
    line: Name,
    commit: String,
    offers: Vec<Offer>,
}

impl Anchored<'_> {
    /// The launcher for one task, here.
    fn at(&self, task: Task) -> Markup {
        super::jobs::launcher(
            self.signed,
            &self.group,
            self.side,
            &self.line,
            &self.commit,
            &task,
            &self.offers,
        )
    }
}

/// Everything one reading of a line needs.
struct Viewing<'a> {
    /// Whose group it is.
    group: &'a Group,
    /// Which of its repositories.
    side: Side,
    /// The line, as the last machine to build it left it.
    digest: &'a LineDigest,
    /// Everything that build read.
    nodes: &'a Nodes,
    /// The launchers anchored to the mathematics.
    launchers: &'a Anchored<'a>,
    /// The machine writing it, if one is.
    writing: Option<&'a Claim>,
    /// Who is reading.
    signed: &'a SignedIn,
    /// What is written, where the repository still has it.
    written: Option<&'a Written>,
}

/// The line itself.
fn body(viewing: &Viewing<'_>) -> Markup {
    let Viewing {
        group,
        side,
        digest,
        nodes,
        launchers,
        writing,
        signed,
        written,
    } = *viewing;
    let boxes = digest.line.boxes();
    let central = digest.line.central();

    html! {
        div class="stack" {
            p class="label" {
                a href={ "/g/" (group.slug()) } { (group.name) }
                " · " (side)
            }
            h1 { (digest.shown_as()) }

            @if let Some(description) = &digest.description {
                p class="serif" { (description) }
            }

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
                        "This line does not contain any declarations yet."
                    }
                }

                @for element in digest.line.skeleton() {
                    @match element {
                        Element::Node(siglum) => {
                            @if let Some(node) = digest.line.node(&siglum) {
                                (drawn(node, digest.lean.as_ref(), launchers))
                            }
                        }
                        Element::Box(black_box) => (drawn_box(&black_box, digest.lean.as_ref(), launchers)),
                    }
                }

                // The line's open end, where a question about where it
                // could go next belongs.
                @if !digest.line.nodes.is_empty() {
                    div class="node" {
                        div class="node__mark" { span { "the end" } }
                        div class="node__body" { (launchers.at(Task::Explore)) }
                    }
                }
            }

            div class="apparatus__register" {
                @if side == Side::Private {
                    div class="block" {
                        p class="label" { "Private" }
                        p {
                            a href={ "/g/" (group.slug()) "/private/" (digest.name) "/share" } {
                                "Share it with the group"
                            }
                        }
                        p class="quiet" {
                            "The current line moves to the shared group repository."
                        }

                        form method="post"
                             action={ "/g/" (group.slug()) "/private/" (digest.name)
                                      "/delete" } {
                            (signature(signed))
                            button class="button button--quiet" type="submit" {
                                "Delete this line"
                            }
                        }
                        p class="quiet" {
                            "The line is removed from your repository, but its history is preserved."
                        }
                    }
                }

                (claim_block(group, side, &digest.name, writing, signed))
                (register(digest, &central, &boxes, nodes))
            }
        }

        // What was written, prose and all, as the Typst it becomes. It has
        // the width of the page because most of it is prose.
        @if let Some(written) = written {
            div class="block source" {
                p class="label" {
                    "Typst source · "
                    span class="identifier" { (short(&written.commit)) }
                }
                pre class="source__text" { (stemma_core::render::readable(&written.source)) }
            }
        }
    }
}

/// One node, with what is known about it in the margin.
fn drawn(
    node: &stemma_core::line::Node,
    lean: Option<&stemma_core::artefact::LeanDigest>,
    launchers: &Anchored<'_>,
) -> Markup {
    let central = node.is_central();
    let verified = lean.is_some_and(|result| {
        result.verification_of(&node.siglum) == stemma_core::model::Verification::Verified
    });

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
                @if verified { span { "Lean verified" } }
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

                // A central statement is frozen against agents, so the
                // one thing that may be asked about it is whether it is
                // false.
                @if central {
                    (launchers.at(Task::Formalize {
                        statement: node.siglum.clone(),
                    }))
                    (launchers.at(Task::Refute {
                        statement: node.siglum.clone(),
                    }))
                } @else if node.siglum.kind.is_statement() {
                    (launchers.at(Task::Promote {
                        statement: node.siglum.clone(),
                    }))
                }
            }
        }
    }
}

/// One box, drawn as the declared absence it is.
fn drawn_box(
    black_box: &stemma_core::line::BlackBox,
    lean: Option<&stemma_core::artefact::LeanDigest>,
    launchers: &Anchored<'_>,
) -> Markup {
    let filled = black_box.state != BoxState::Empty;
    let verified = lean.is_some_and(|result| result.verifies_box(black_box));

    html! {
        div class=@if filled { "box box--filled" } @else { "box" } {
            div class="node__mark" {
                span { "black box" }
                @if verified { span { "Lean verified" } }
            }

            div class="box__band" {
                @if filled && !verified {
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
                    span { "Not completed" }
                }
            }

            div class="box__ask" {
                (launchers.at(Task::Fill {
                    after: black_box.after.clone(),
                }))
                @if filled {
                    (launchers.at(Task::Verify {
                        after: black_box.after.clone(),
                    }))
                }
            }
        }
    }
}

/// Who is writing this line, if anybody is.
///
/// A claim is a condition and not a task: it says what is true now, it
/// lapses on its own, and nothing about it is ever counted. The one
/// action offered is to the person whose machine is holding it.
fn claim_block(
    group: &Group,
    side: Side,
    line: &Name,
    writing: Option<&Claim>,
    signed: &SignedIn,
) -> Markup {
    html! {
        div class="block" {
            p class="label" { "Claim" }

            @match writing {
                None => p class="quiet" { "This line is available to edit." }
                Some(claim) => {
                    p { (claim.label) " is writing it" }
                    p class="quiet" { (claim.held().lapses_in()) }

                    @if claim.asked {
                        p class="quiet" { "Another contributor has requested access." }
                    }

                    @if claim.owner == signed.user.id {
                        form method="post"
                             action={ "/g/" (group.slug()) "/" (side) "/" (line) "/claim/release" } {
                            (signature(signed))
                            button type="submit" { "Release" }
                        }
                    }
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
            p class="label" { "Central statements" }

            @if central.is_empty() {
                p class="empty" { "No central statements have been declared." }
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
                (boxes.len()) " total, " (empty) " incomplete"
            }
        }

        div class="block" {
            p class="label" { "Built" }
            p class="quiet identifier" { (nodes.commit) }
            @match &nodes.typst {
                Some(pinned) => p class="quiet" { "typst " (pinned) " is pinned" }
                None => p class="quiet" { "No Typst version is pinned." }
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

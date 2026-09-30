//! Showing a line to somebody whose own machine is switched off.
//!
//! What is drawn comes from an artefact a machine built and handed over,
//! named by the commit it was built from. The instance compiles nothing,
//! so what it can show is what the source declares: the definitions, the
//! statements the line rests on, and the boxes between them.
//!
//! The current revision carries its typeset HTML page from the machine that
//! wrote it. The server serves that page without compiling the source.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};
use stemma_core::artefact::{Kind, LineDigest, Nodes, Scope};
use stemma_core::id::{GroupId, LineId, Slug};
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
use crate::store::groups::Group;

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
        // A build of the revision the line is at says everything the
        // source does and more.
        (Some(written), Some(nodes))
            if nodes
                .line(&name)
                .is_some_and(|built| built.revision == Some(written.revision)) =>
        {
            nodes
                .line(&name)
                .cloned()
                .unwrap_or_else(|| declared(&name, &written.source))
        }
        // Nothing has built it yet, which is the ordinary state of a
        // line somebody is writing, or what was built is older than what
        // was written since. What the source declares is readable
        // without compiling anything, so the line is here as it stands —
        // with everything a build adds, and only that, missing.
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

    let offers = super::jobs::offers(&instance, &group.id, &signed.user.id).await?;
    let audits = match &written {
        Some(written) => {
            let line = written.line;
            instance
                .read(move |connection| crate::store::formal_audits::for_line(connection, &line))
                .await?
        }
        None => Vec::new(),
    };
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
        revision: written.as_ref().map_or(0, |written| written.revision),
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
            audits: &audits,
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
    let (id, reader) = (group.clone(), signed.user.id.clone());
    let names: Vec<Name> = instance
        .read(move |connection| crate::store::lines::in_group(connection, &id, &reader))
        .await?
        .into_iter()
        .filter(|line| line.side == side)
        .map(|line| line.name)
        .collect();

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

/// A line's source, exactly as it stands at its current revision.
struct Written {
    /// The stable line whose revision this is.
    line: LineId,
    /// The revision it was read from.
    revision: u64,
    /// What the line says there.
    source: String,
}

/// Reads a line's source at its current revision.
async fn written(
    instance: &Instance,
    group: &GroupId,
    side: Side,
    signed: &SignedIn,
    name: &Name,
) -> Result<Option<Written>, WebError> {
    let (group, name, reader) = (group.clone(), name.clone(), signed.user.id.clone());

    instance
        .read(move |connection| {
            let Some(line) =
                crate::store::lines::by_name(connection, &group, side, &name, &reader)?
            else {
                return Ok(None);
            };

            Ok(
                crate::store::revisions::current(connection, &line)?.map(|revision| Written {
                    line: line.id,
                    revision: revision.number,
                    source: revision.source,
                }),
            )
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
        revision: None,
    }
}

/// Which repository was asked for, and whether this reader may have it.
async fn admit(
    instance: &Instance,
    signed: &SignedIn,
    group: &str,
    side: &str,
) -> Result<(Group, Scope, Side), WebError> {
    let side = match side {
        "shared" => Side::Shared,
        "private" => Side::Private,
        _ => return Err(WebError::NotFound),
    };
    let group = super::membership::group_of(instance, group, &signed.user.id).await?;

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
    revision: u64,
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
            self.revision,
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
    /// Human correspondence records for its central formal statements.
    audits: &'a [crate::store::formal_audits::FormalAudit],
}

/// The line itself: its typeset page, then its statements in order beside
/// what is known about the line as a whole.
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
        audits,
    } = *viewing;
    let boxes = digest.line.boxes();
    let central = digest.line.central();
    let checking = written
        .is_some_and(|written| digest.revision != Some(written.revision) || digest.lean.is_none());
    let at = format!("/g/{}/{}/{}", group.slug(), side, digest.name);

    html! {
        header class="page-head" {
            nav class="crumbs" aria-label="Breadcrumb" {
                a href="/#groups" { "Groups" }
                span aria-hidden="true" { "/" }
                a href={ "/g/" (group.slug()) } { (group.name) }
                span aria-hidden="true" { "/" }
                span aria-current="page" { (digest.shown_as()) }
            }
            div class="page-head__text" {
                h1 class="page-title" { (digest.shown_as()) }
                div class="page-meta" {
                    span class="identifier" { "lines/" (digest.name) }
                    @match side {
                        Side::Shared => span class="chip" { (super::layout::glyph("group")) "Shared" },
                        Side::Private => span class="chip" { (super::layout::glyph("lock")) "Private" },
                    }
                    @if writing.is_some() {
                        span class="chip chip--accent" { "Being written" }
                    }
                    @if let Some(written) = written {
                        span class="numeric" { "Revision " (written.revision) }
                    }
                }
                @if let Some(description) = &digest.description {
                    p class="page-subtitle" { (description) }
                }
            }
        }

        // The page, as the machine that wrote this revision typeset it,
        // in a frame of its own: it came from a machine, so nothing in it
        // runs.
        @if written.is_some() {
            section class="block typeset" aria-labelledby="typeset-title" {
                div class="typeset__head" {
                    h2 class="block__title" id="typeset-title" { "The page" }
                    div class="actions" {
                        a class="button button--small" href={ (at) "/line.json" } download="line.json" {
                            (super::layout::glyph("download")) "Download line.json"
                        }
                        a class="button button--small" href={ (at) "/page" } target="_blank" rel="noopener"
                          title="Open the typeset page, then use your browser's print command" {
                            (super::layout::glyph("document")) "Print or save as PDF"
                        }
                    }
                }
                iframe class="typeset__frame" sandbox="" src={ (at) "/page" }
                       title={ "The typeset page of " (digest.shown_as()) } {}
            }
        }

        div class="line-layout" {
            section class="card statements-card" aria-labelledby="statements-title" {
                div class="card__head" {
                    h2 class="card__title" id="statements-title" {
                        "Statements" span class="count" { (digest.line.nodes.len()) }
                    }
                    @if !nodes.commit.is_empty() {
                        span class="quiet identifier" title=(nodes.commit) {
                            "built from " (short(&nodes.commit))
                        }
                    }
                }

                @if digest.line.nodes.is_empty() {
                    div class="card__empty" {
                        p class="empty" { "This line does not contain any declarations yet." }
                    }
                } @else {
                    ol class="statements" {
                        @for element in digest.line.skeleton() {
                            @match element {
                                Element::Node(siglum) => {
                                    @if let Some(node) = digest.line.node(&siglum) {
                                        (drawn(node, digest.lean.as_ref(), audits, checking, launchers))
                                    }
                                }
                                Element::Box(black_box) => (drawn_box(&black_box, digest.lean.as_ref(), checking, launchers)),
                            }
                        }

                        // The line's open end, where a question about where
                        // it could go next belongs.
                        li class="statement statement--end" {
                            div class="statement__head" { span class="label" { "The end of the line" } }
                            div class="statement__actions" { (launchers.at(Task::Explore)) }
                        }
                    }
                }
            }

            aside class="line-layout__rail" aria-label="About the line" {
                @if side == Side::Private {
                    section class="block" aria-labelledby="private-title" {
                        h2 class="block__title" id="private-title" { "Private line" }
                        p class="quiet" {
                            "Only you can see it. Sharing moves it to the group's repository."
                        }
                        div class="actions" {
                            a class="button button--primary" href={ "/g/" (group.slug()) "/private/" (digest.name) "/share" }
                              data-dialog-link {
                                "Share with the group"
                            }
                            a class="button button--danger" href={ "/g/" (group.slug()) "/private/" (digest.name) "/delete" }
                              data-dialog-link {
                                "Delete"
                            }
                        }
                    }
                }

                (claim_block(group, side, &digest.name, writing, signed))
                (register(digest, &central, &boxes, nodes))
            }
        }
    }
}

/// One statement, with what is known about it.
fn drawn(
    node: &stemma_core::line::Node,
    lean: Option<&stemma_core::artefact::LeanDigest>,
    audits: &[crate::store::formal_audits::FormalAudit],
    checking: bool,
    launchers: &Anchored<'_>,
) -> Markup {
    let central = node.is_central();
    let formal =
        lean.and_then(|result| result.blocks.iter().find(|check| check.block == node.block));
    let verified =
        formal.is_some_and(|result| result.state == stemma_core::model::Verification::Verified);
    let audit = audits.iter().find(|audit| audit.block == node.block);
    let audited = audit.is_some_and(|audit| audit.state == "audited");
    let backed = verified && (!central || audited);

    html! {
        li class=@if central { "statement statement--central" } @else { "statement" } {
            div class="statement__head" {
                span class="chip" { (super::profile::capitalised(&node.siglum.kind.to_string())) }
                @if central {
                    // The edge and this word are the same fact said twice,
                    // because colour alone must never carry a state.
                    span class="chip chip--accent" { "Central" }
                }
                @if node.proved {
                    span class="chip" { "Proved here" }
                }
                @if backed { span class="chip chip--success" { "Backed" } }
                @else if verified && central { span class="chip chip--warning" { "Audit missing" } }
                @else { span class="chip chip--dashed" { "Formal work missing" } }
                @if checking { span class="chip chip--dashed" { "Checking" } }
            }

            div class="statement__title" {
                p class="statement__name" {
                    @match &node.name {
                        Some(name) => (name),
                        None => (node.siglum.to_string()),
                    }
                }
                @if node.name.is_some() {
                    span class="identifier statement__siglum" { (node.siglum.to_string()) }
                }
            }

            @if !node.uses.is_empty() {
                p class="statement__uses" {
                    "Uses "
                    @for (at, used) in node.uses.iter().enumerate() {
                        @if at > 0 { " · " }
                        span class="identifier" { (used.to_string()) }
                    }
                }
            }

            @if let Some(formal) = formal {
                div class="formal-review" {
                    p class="label" { "Formal statement · " (formal.state.as_str()) }
                    @for declaration in &formal.declarations {
                        p class="identifier" { (declaration.name) " · " (declaration.module) }
                        pre class="source__text" { (declaration.statement) }
                        @if !declaration.reaches.is_empty() {
                            p class="quiet" { "Reaches " (declaration.reaches.join(", ")) }
                        }
                    }
                    @if formal.declarations.is_empty() {
                        p class="quiet" { (formal.diagnostic) }
                    }
                }
            }
            @if let Some(audit) = audit {
                div class="formal-review" {
                    p class="label" { "Correspondence · " (audit.state.replace('_', " ")) }
                    p { strong { "Agent reading: " } (audit.reading) }
                    @if let Some(reason) = &audit.reason { p class="quiet" { (reason) } }
                }
            }

            div class="statement__actions" {
                (launchers.at(Task::Formalize {
                    block: node.block,
                }))
                // A central statement is frozen against agents; beyond
                // formalizing it, the one attack allowed is refutation.
                @if central && node.siglum.kind.is_statement() {
                    (launchers.at(Task::Refute {
                        block: node.block,
                    }))
                } @else if node.siglum.kind.is_statement() {
                    (launchers.at(Task::Promote {
                        block: node.block,
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
    checking: bool,
    launchers: &Anchored<'_>,
) -> Markup {
    let filled = black_box.state != BoxState::Empty;
    let verified = lean.is_some_and(|result| result.verifies_box(black_box));

    html! {
        li class=@if filled { "statement box box--filled" } @else { "statement box" } {
            div class="statement__head" {
                span class="chip chip--dashed" { "Black box" }
                @if verified { span class="chip chip--success" { "Lean verified" } }
                @if checking { span class="chip chip--dashed" { "Checking" } }
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

            div class="statement__actions" {
                (launchers.at(Task::Fill {
                    after: black_box.after,
                }))
                @if filled {
                    (launchers.at(Task::Verify {
                        after: black_box.after,
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
        section class="block" aria-labelledby="claim-title" {
            h2 class="block__title" id="claim-title" { "Claim" }

            @match writing {
                None => p class="quiet" { "Nobody is writing it. It is available to edit." }
                Some(claim) => {
                    div class="stack stack--tight" {
                        p { (claim.label) " is writing it." }
                        p class="quiet" { (super::profile::capitalised(&claim.held().lapses_in())) "." }
                        @if claim.asked {
                            p class="quiet" { "Another contributor has asked to write it." }
                        }
                    }

                    @if claim.owner == signed.user.id {
                        form method="post"
                             action={ "/g/" (group.slug()) "/" (side) "/" (line) "/claim/release" } {
                            (signature(signed))
                            button class="button" type="submit" { "Release" }
                        }
                    }
                }
            }
        }
    }
}

/// What is known about the line as a whole.
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
        section class="block" aria-labelledby="central-title" {
            h2 class="block__title" id="central-title" {
                "Central statements" span class="count" { (central.len()) }
            }

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

        section class="block" aria-labelledby="facts-title" {
            h2 class="block__title" id="facts-title" { "The line" }
            dl class="facts" {
                dt { "Statements" }
                dd class="numeric" { (digest.line.nodes.len()) }
                dt { "Dependencies" }
                dd class="numeric" { (digest.line.dependencies().len()) }
                dt { "Black boxes" }
                dd class="numeric" {
                    @if boxes.is_empty() { "None" } @else { (boxes.len()) ", " (empty) " not completed" }
                }
                dt { "Built" }
                dd {
                    @if nodes.commit.is_empty() {
                        span class="quiet" { "Not yet" }
                    } @else {
                        span class="identifier" title=(nodes.commit) { (short(&nodes.commit)) }
                    }
                }
                dt { "Typst" }
                dd {
                    @match &nodes.typst {
                        Some(pinned) => span class="identifier" { (pinned) }
                        None => span class="quiet" { "Not pinned" }
                    }
                }
            }
        }
    }
}

/// A commit, short enough to read.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(8)]
}

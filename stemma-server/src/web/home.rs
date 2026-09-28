//! The overview: what claims me.
//!
//! Not a dashboard of everything happening on the instance. What belongs
//! here is what this person is in and what is waiting for them to decide —
//! and a count is only ever on a decision, never on the state of the
//! mathematics.
//!
//! The groups are cards rather than a list because a card can carry what a
//! group is for and how much is in it, and that is the thing somebody is
//! choosing between. Everything deeper is the group's own page.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};
use stemma_core::artefact::{LineDigest, Scope};
use stemma_core::repository::Side;

use super::layout::{glyph, initials, page};
use super::visitor::{SignedIn, Visitor};
use super::{Instance, WebError};
use crate::store::groups::{Membership, Outcome, Request};
use crate::store::machines::Machine;
use crate::store::proposals::{self, Proposal};

/// A group, as the overview shows it.
struct Card {
    /// The group and what the reader may do in it.
    membership: Membership,
    /// Every line the group has shared.
    shared: Vec<LineDigest>,
    /// How many people are in it.
    members: usize,
}

/// The page somebody sees when they are signed in.
pub async fn home(
    State(instance): State<Instance>,
    visitor: Visitor,
) -> Result<Response, WebError> {
    let Some(signed) = visitor.0 else {
        return Ok(axum::response::Redirect::to("/login").into_response());
    };

    let chrome = super::layout::around(&instance, &signed, "Overview")
        .await?
        .at("/");

    let user = signed.user.id.clone();
    let (requests, proposals, obligations) = instance
        .read(move |connection| {
            Ok((
                crate::store::groups::requests_by(connection, &user)?,
                proposals::awaiting(connection, &user)?,
                crate::store::vault::obligations(connection, &user)?,
            ))
        })
        .await?;

    let mut cards = Vec::new();
    for membership in chrome.workspace.groups.clone() {
        let shared = shared_lines(&instance, &membership, &signed).await?;
        let group = membership.group.id.clone();
        let members = instance
            .read(move |connection| crate::store::groups::members(connection, &group))
            .await?
            .len();
        cards.push(Card {
            membership,
            shared,
            members,
        });
    }

    let machines: Vec<Machine> = chrome
        .workspace
        .machines
        .iter()
        .map(|(machine, _)| machine.clone())
        .collect();

    Ok(page(
        &chrome,
        body(
            &signed,
            &chrome.instance,
            &cards,
            &requests,
            &machines,
            &proposals,
            &obligations,
            chrome.workspace.may_ask,
            &chrome.workspace.badges,
        ),
    )
    .into_response())
}

/// What a group has published for everybody in it.
///
/// A line started a minute ago is in the repository and in no build yet,
/// so it is asked for separately: a card that waits for the next build to
/// admit a line makes the screen that created it look broken.
async fn shared_lines(
    instance: &Instance,
    membership: &Membership,
    signed: &SignedIn,
) -> Result<Vec<LineDigest>, WebError> {
    let group = &membership.group.id;
    let scope = Scope::of(Side::Shared, &signed.user.id);

    let mut lines = super::lines::read(instance, group, &scope)
        .await?
        .map(|nodes| nodes.lines)
        .unwrap_or_default();

    for digest in super::lines::unbuilt_in(instance, group, Side::Shared, signed).await? {
        if !lines.iter().any(|line| line.name == digest.name) {
            lines.push(digest);
        }
    }

    Ok(lines)
}

/// What the overview holds.
#[allow(clippy::too_many_arguments)]
fn body(
    signed: &SignedIn,
    instance: &str,
    cards: &[Card],
    requests: &[Request],
    machines: &[Machine],
    proposals: &[Proposal],
    obligations: &[crate::store::vault::Obligation],
    may_ask: bool,
    badges: &std::collections::HashMap<stemma_core::id::GroupId, String>,
) -> Markup {
    // An approved request is a group now, and the group has its own card.
    let requests: Vec<&Request> = requests
        .iter()
        .filter(|request| request.outcome != Some(Outcome::Approved))
        .collect();
    let nothing = cards.is_empty() && requests.is_empty();

    html! {
        header class="page-head" {
            h1 class="page-title" { "Overview" }
            p class="page-subtitle" { "Your groups and machines on " (instance) "." }
        }

        (reviews(proposals))
        (decisions(signed, obligations))

        section class="section" id="groups" aria-labelledby="groups-title" {
            h2 class="section-title" id="groups-title" {
                "Groups" span class="count" { (cards.len()) }
            }

            @if nothing && !may_ask {
                div class="card card--empty empty-card" {
                    p class="empty" {
                        "You have not joined a research group yet. An invitation puts you in one."
                    }
                }
            } @else {
                div class="group-grid" {
                    @for card in cards {
                        (group_card(card, badges.get(&card.membership.group.id).map(String::as_str)))
                    }
                    @for request in &requests {
                        (request_card(request))
                    }
                    @if may_ask {
                        (new_group_card(nothing))
                    }
                }
            }
        }

        section class="section" id="machines" aria-labelledby="machines-title" {
            h2 class="section-title" id="machines-title" {
                "Machines" span class="count" { (machines.len()) }
            }

            @if machines.is_empty() {
                div class="card card--empty empty-card" {
                    p class="empty" {
                        "No machines connected. Sign in on a computer with "
                        code { "stemma login" } " and run " code { "stemma node init" }
                        " to connect it."
                    }
                }
            } @else {
                div class="machine-list" {
                    @for machine in machines {
                        (machine_row(machine))
                    }
                }
            }
        }
    }
}

/// One group, and the way into it: the whole card is the link.
fn group_card(card: &Card, picture: Option<&str>) -> Markup {
    let group = &card.membership.group;
    let members = match card.members {
        1 => "1 member".to_owned(),
        count => format!("{count} members"),
    };
    let shared = match card.shared.len() {
        0 => "Nothing shared yet".to_owned(),
        1 => "1 shared line".to_owned(),
        count => format!("{count} shared lines"),
    };

    html! {
        a class="card group-card" href={ "/g/" (group.slug()) } {
            span class="group-card__top" {
                (super::pictures::badge(&group.id, &group.name, picture, "badge"))
                span class="chip chip--accent" { (super::groups::role_words(card.membership.role)) }
            }
            span class="group-card__name" { (group.name) }
            span class="identifier quiet" { (group.id.to_string()) }

            @if let Some(description) = &group.description {
                span class="group-card__about" { (description) }
            }

            span class="group-card__foot" {
                span { (shared) " · " (members) }
                span class="chevron" { (glyph("chevron-right")) }
            }
        }
    }
}

/// A group somebody asked for and nobody has decided on, or one that was
/// declined, with what the maintainer said.
fn request_card(request: &Request) -> Markup {
    html! {
        div class="card group-card group-card--request" {
            span class="group-card__top" {
                span class="badge badge--muted" aria-hidden="true" { (initials(&request.name)) }
                @match request.outcome {
                    Some(Outcome::Declined) => span class="chip chip--danger" { "Declined" },
                    _ => span class="chip chip--warning" { "Pending" },
                }
            }
            span class="group-card__name" { (request.name) }
            span class="identifier quiet" { "grp:" (request.slug) }
            span class="group-card__about" {
                @match (&request.outcome, &request.note) {
                    (_, Some(note)) => (note),
                    (None, None) => "Waiting for a maintainer.",
                    _ => "A maintainer declined it.",
                }
            }
        }
    }
}

/// The card that makes a group where there is none.
fn new_group_card(alone: bool) -> Markup {
    html! {
        div class=@if alone { "card card--empty request-card request-card--alone" } @else { "card card--empty request-card" } {
            span class="request-card__title" { "Request a research group" }
            p class="quiet" {
                @if alone { "You are not in a research group yet. " }
                "A maintainer decides, and you become its owner."
            }
            a class="button" href="/groups/new" {
                (glyph("add")) "Request a group"
            }
        }
    }
}

/// One machine, and the way to its page.
fn machine_row(machine: &Machine) -> Markup {
    let awake = machine.presence == stemma_core::machine::Presence::Awake;

    html! {
        a class="card machine-row" href={ "/machines/" (machine.id) } {
            span class="machine-row__icon" { (glyph("machine")) }
            span class="machine-row__who" {
                span class="machine-row__name" { (machine.label) }
                span class="identifier quiet" { (machine.id.to_string()) }
            }
            @if awake {
                span class="chip chip--success" { span class="dot" {} "Online" }
            } @else {
                span class="chip" {
                    "Offline"
                    @if let Some(seen) = &machine.last_seen_at {
                        " · last seen " (seen.get(..16).unwrap_or(seen))
                    }
                }
            }
            span class="chevron" { (glyph("chevron-right")) }
        }
    }
}

/// What is waiting for this person to judge.
///
/// A proposal is a machine asking to change mathematics somebody owns, so
/// it belongs beside the groups it would change rather than behind a
/// destination of its own that is empty most days.
fn reviews(proposals: &[Proposal]) -> Markup {
    html! {
        @if !proposals.is_empty() {
            section class="block" id="reviews" {
                h2 class="block__title" { "Proposals awaiting your review" span class="count" { (proposals.len()) } }
                ul class="register" {
                    @for proposal in proposals {
                        li {
                            span class="register__name" {
                                a href={ "/proposals/" (proposal.id) } { (proposal.job.task.asked_for()) }
                            }
                            span class="quiet identifier" { (proposal.job.line) " · " (proposal.model) }
                        }
                    }
                }
            }
        }
    }
}

/// What this person owes a signed answer to.
fn decisions(signed: &SignedIn, obligations: &[crate::store::vault::Obligation]) -> Markup {
    html! {
        @if !obligations.is_empty() {
            section class="block" {
                h2 class="block__title" { "Pending decisions" span class="count" { (obligations.len()) } }
                @for obligation in obligations {
                    form method="post" action={ "/audit-obligations/" (obligation.id) "/decide" } {
                        (super::layout::signature(signed))
                        p { "The audit of bridge " code { (obligation.bridge) } " was withdrawn; "
                            code { (obligation.statement) } " depends on it." }
                        select class="input" name="decision" {
                            option value="stands" { "It stands as it is" }
                            option value="rewrite" { "I will rewrite it" }
                            option value="waiting_for_audit" { "I am waiting for another audit" }
                        }
                        input class="input" name="note" placeholder="Record why" required;
                        button class="button button--primary" { "Record signed decision" }
                    }
                }
            }
        }
    }
}

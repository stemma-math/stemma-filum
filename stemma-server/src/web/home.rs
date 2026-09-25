//! The overview: what claims me.
//!
//! Not a dashboard of everything happening on the instance. What belongs
//! here is what this person is in and what is waiting for them to decide —
//! and a count is only ever on a decision, never on the state of the
//! mathematics.
//!
//! The groups are cards rather than a list because a card can carry what a
//! group is for and what has been shared in it, and that is the thing
//! somebody is choosing between. Everything deeper is the group's own page.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};
use stemma_core::artefact::{LineDigest, Scope};
use stemma_core::repository::Side;

use super::layout::page;
use super::visitor::{SignedIn, Visitor};
use super::{Instance, WebError};
use crate::store::groups::{Membership, Request};
use crate::store::proposals::{self, Proposal};

/// How many of a group's shared lines a card names before it counts.
const ON_A_CARD: usize = 4;

/// A group, as the overview shows it.
struct Card {
    /// The group and what the reader may do in it.
    membership: Membership,
    /// Every line the group has shared.
    shared: Vec<LineDigest>,
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
        cards.push(Card { membership, shared });
    }

    Ok(page(
        &chrome,
        body(
            &signed,
            &cards,
            &requests,
            &proposals,
            &obligations,
            chrome.workspace.may_ask,
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
fn body(
    signed: &SignedIn,
    cards: &[Card],
    requests: &[Request],
    proposals: &[Proposal],
    obligations: &[crate::store::vault::Obligation],
    may_ask: bool,
) -> Markup {
    html! {
        div class="stack" {
            h1 { (signed.user.display_name) }
            p class="quiet identifier" { (signed.user.id.to_string()) }
        }

        div class="block" id="groups" {
            p class="label" { "Groups" }

            @if cards.is_empty() && !may_ask {
                p class="empty" {
                    "You have not joined a research group yet. An invitation puts you in one."
                }
            } @else {
                div class="cards" {
                    @for card in cards {
                        (group_card(card))
                    }
                    @if may_ask {
                        (new_group_card())
                    }
                }
            }
        }

        (reviews(proposals))
        (decisions(signed, obligations))

        @if !requests.is_empty() {
            div class="block" {
                p class="label" { "Your group requests" }
                ul class="register" {
                    @for request in requests {
                        (crate::web::groups::own_request(request))
                    }
                }
            }
        }
    }
}

/// One group, and the way into it.
fn group_card(card: &Card) -> Markup {
    let group = &card.membership.group;
    let slug = group.slug();

    html! {
        a class="card" href={ "/g/" (slug) } {
            div class="card__head" {
                span class="card__name" { (group.name) }
                span class="card__role" { (card.membership.role) }
            }
            p class="quiet identifier" { (group.id.to_string()) }

            @if let Some(description) = &group.description {
                p class="card__about" { (description) }
            }

            div class="card__lines" {
                @if card.shared.is_empty() {
                    p class="empty" { "Nothing shared yet." }
                } @else {
                    p class="label" { "Shared lines · " (card.shared.len()) }
                    ul class="card__register" {
                        @for line in card.shared.iter().take(ON_A_CARD) {
                            li {
                                span class="card__line" { (line.shown_as()) }
                                @if let Some(description) = &line.description {
                                    span class="quiet" { (description) }
                                }
                            }
                        }
                    }
                    @if card.shared.len() > ON_A_CARD {
                        p class="quiet" {
                            (card.shared.len() - ON_A_CARD) " more inside"
                        }
                    }
                }
            }

            span class="card__enter" { "Open group" }
        }
    }
}

/// The card that makes a group where there is none.
fn new_group_card() -> Markup {
    html! {
        a class="card card--new" href="/groups/new" {
            span class="card__plus" aria-hidden="true" { "+" }
            span class="card__name" { "Request a research group" }
            p class="quiet" { "A maintainer decides, and you become its owner." }
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
            div class="block" id="reviews" {
                p class="label" { "Proposals awaiting your review · " (proposals.len()) }
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
            div class="block" {
                p class="label" { "Pending decisions · " (obligations.len()) }
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

//! The personal home: what claims me.
//!
//! Not a dashboard of everything happening on the instance. What belongs
//! here is what this person is in and what is waiting for them to decide —
//! and a count is only ever on a decision, never on the state of the
//! mathematics.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

use super::layout::{Chrome, page};
use super::visitor::{SignedIn, Visitor};
use super::{Instance, WebError};
use crate::store::groups::{self, Membership, Request};
use crate::store::proposals::{self, Proposal};

/// The page somebody sees when they are signed in.
pub async fn home(
    State(instance): State<Instance>,
    visitor: Visitor,
) -> Result<Response, WebError> {
    let Some(signed) = visitor.0 else {
        return Ok(axum::response::Redirect::to("/login").into_response());
    };

    let policy = instance.policy().await?;
    let name = super::front::instance_name(&instance, &policy);

    let user = signed.user.id.clone();
    let (memberships, requests, proposals, obligations) = instance
        .read(move |connection| {
            Ok((
                groups::for_user(connection, &user)?,
                groups::requests_by(connection, &user)?,
                proposals::awaiting(connection, &user)?,
                crate::store::vault::obligations(connection, &user)?,
            ))
        })
        .await?;

    let machines = super::machines::owned_by(&instance, &signed.user.id).await?;

    let chrome = Chrome::anonymous("Home", name)
        .at("/")
        .signed_in(signed.user.clone(), signed.maintainer);

    Ok(page(
        &chrome,
        body(
            &signed,
            &memberships,
            &requests,
            &machines,
            &proposals,
            &obligations,
            policy.group_requests_open,
        ),
    )
    .into_response())
}

/// What the home page holds.
fn body(
    signed: &SignedIn,
    memberships: &[Membership],
    requests: &[Request],
    machines: &[(crate::store::machines::Machine, usize)],
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

            @if memberships.is_empty() {
                p class="empty" {
                    "You have not joined a research group yet. Use an invitation or request a new group."
                }
            } @else {
                ul class="register" {
                    @for membership in memberships {
                        li {
                            span class="register__name" {
                                a href={ "/g/" (membership.group.slug()) } {
                                    (membership.group.name)
                                }
                            }
                            span class="quiet identifier" { (membership.group.id.to_string()) }
                            span class="register__aside" { (membership.role) }
                        }
                    }
                }
            }

            @if may_ask {
                p { a href="/groups/new" { "Request a research group" } }
            }
        }

        (super::machines::summary(machines))

        @if !proposals.is_empty() {
            div class="block" id="reviews" {
                p class="label" { "Proposals awaiting your review" }
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

        @if !obligations.is_empty() {
            div class="block" {
                p class="label" { "Decisions you owe · " (obligations.len()) }
                @for obligation in obligations {
                    form method="post" action={ "/audit-obligations/" (obligation.id) "/decide" } {
                        (super::layout::signature(signed))
                        p { "The audit of bridge " code { (obligation.bridge) } " was withdrawn; "
                            code { (obligation.statement) } " depends on it." }
                        select name="decision" {
                            option value="stands" { "It stands as it is" }
                            option value="rewrite" { "I will rewrite it" }
                            option value="waiting_for_audit" { "I am waiting for another audit" }
                        }
                        input name="note" placeholder="Record why" required;
                        button class="button" { "Record signed decision" }
                    }
                }
            }
        }

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

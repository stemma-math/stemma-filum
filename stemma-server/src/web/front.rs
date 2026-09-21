//! The instance's front door.
//!
//! Not the project's website: those are different artefacts with different
//! owners, and putting marketing copy on a university's server would be
//! the wrong thing in the wrong place. This page says what *this* instance
//! is, who runs it and how to get in. Nothing else.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

use super::layout::{Chrome, page};
use super::visitor::Visitor;
use super::{Instance, WebError};
use crate::store::settings::Policy;
use crate::store::users;

/// What the instance calls itself.
///
/// An instance nobody has named is called by the host it answers on, which
/// is what the people using it call it anyway.
pub fn instance_name(instance: &Instance, policy: &Policy) -> String {
    policy
        .instance_name
        .clone()
        .unwrap_or_else(|| instance.settings.public_url.host().to_owned())
}

/// The front page, whoever is reading.
pub async fn door(
    State(instance): State<Instance>,
    visitor: Visitor,
) -> Result<Response, WebError> {
    if visitor.0.is_some() {
        return super::home::home(State(instance), visitor).await;
    }

    let policy = instance.policy().await?;
    let accounts = instance.read(users::count).await?;
    let name = instance_name(&instance, &policy);

    let chrome = match accounts {
        0 => Chrome::anonymous("Front door", &name).without_ways_in(),
        _ => Chrome::anonymous("Front door", &name).registration(policy.registration_open),
    }
    .landing();

    Ok(page(&chrome, body(&name, &policy, accounts)).into_response())
}

/// The door itself.
fn body(name: &str, policy: &Policy, accounts: i64) -> Markup {
    html! {
        div class="doorway" {
            div class="doorway__constellation" aria-hidden="true" {
                (super::layout::mark())
            }
            div class="doorway__copy" {
                p class="doorway__eyebrow" { "Mathematics collaboration for the AI era" }
                h1 class="doorway__name" { "Stemma" }
                p class="doorway__lead" {
                    "Ideas, people and proofs — connected in one shared research space."
                }
                div class="doorway__actions" {
                    @if accounts == 0 {
                        (fresh())
                    } @else {
                        (way_in(policy))
                    }
                }
                p class="doorway__meta" {
                    span { (name) }
                    @match &policy.operator {
                        Some(operator) => span { "Run by " (operator) "." }
                        None => span { "Independent research infrastructure" }
                    }
                    @if let Some(contact) = &policy.contact {
                        span { (contact) }
                    }
                }
            }
        }
    }
}

/// What a brand new instance says.
///
/// It has no accounts, so offering a log-in form would be offering to sign
/// in as somebody who does not exist. This is the install and check path
/// continuing into the browser.
fn fresh() -> Markup {
    html! {
        div class="block" {
            p class="label" { "Nobody has registered here yet" }
            p {
                "This instance is running and empty. The first account is created the "
                "same way as every other one, and an instance maintainer is then granted "
                "from the machine itself."
            }
            div class="stack stack--tight" {
                p { a class="button button--hero" href="/register" data-surface-exit { "Create the first account" } }
                p class="quiet" {
                    "Then, on the machine: "
                    code { "stemma-server admin grant <account>" }
                }
            }
        }
    }
}

/// The way in, for an instance that has people in it.
fn way_in(policy: &Policy) -> Markup {
    html! {
        div class="block" {
            div class="actions" {
                a class="button button--hero" href="/login" data-surface-exit { "Enter Stemma" }
                @if policy.registration_open {
                    a class="button button--glass" href="/register" data-surface-exit { "Create an account" }
                }
            }

            @if !policy.registration_open {
                p class="quiet" {
                    "Accounts here are created by invitation. Somebody in a group sends "
                    "you a link."
                }
            }
        }
    }
}

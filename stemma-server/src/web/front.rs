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
    let name = instance_name(&instance, &policy);

    let chrome = Chrome::anonymous("Home", &name)
        .registration(policy.registration_open)
        .landing();

    Ok(page(&chrome, body(&name, &policy)).into_response())
}

/// The door itself.
///
/// The same door whatever is behind it. An instance with nobody in it yet
/// is still this instance, and a front page that rearranges itself around
/// how far along its own setup is asks the reader to care about that.
fn body(name: &str, policy: &Policy) -> Markup {
    html! {
        div class="doorway" {
            canvas class="doorway__light" id="doorway-light" aria-hidden="true" {}
            div class="doorway__hero" {
                div class="doorway__stage" {
                    canvas aria-hidden="true" {}
                    img class="doorway__still" src=(super::assets::SYMBOL_PATH) alt="";
                    h1 class="doorway__name" { "Stemma" }
                }
                p class="doorway__lead" {
                    "A collaborative platform for mathematical research in the AI era."
                }
                div class="doorway__actions" {
                    (way_in(policy))
                }
                p class="doorway__meta" {
                    span { (name) }
                    @match &policy.operator {
                        Some(operator) => span { "Managed by " (operator) }
                        None => span { "Independent instance" }
                    }
                    @if let Some(contact) = &policy.contact {
                        span { (contact) }
                    }
                }
            }
        }
    }
}

/// The way in.
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
                    "Registration is by invitation only."
                }
            }
        }
    }
}

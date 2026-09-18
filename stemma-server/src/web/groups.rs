//! Asking for a group.
//!
//! Groups are requested, not created: somebody asks, a maintainer approves,
//! and the person who asked becomes the owner. An instance can be set to
//! let people create them outright, and that is a policy, not a different
//! mechanism.

use axum::extract::State;
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use stemma_core::id::Slug;

use super::layout::{Chrome, page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups::{self, Request};

/// What the request form sends.
#[derive(Debug, Deserialize)]
pub struct Asked {
    /// What the group would be called.
    pub name: String,
    /// The short name its remote and its directory would use.
    pub slug: String,
    /// Why it should exist.
    pub reason: String,
    /// What says the form came from a page we served.
    pub form_token: String,
}

/// The form for asking.
pub async fn request_form(
    State(instance): State<Instance>,
    signed: SignedIn,
) -> Result<Response, WebError> {
    let policy = instance.policy().await?;

    if !policy.group_requests_open {
        return Ok(page(&chrome(&instance, &policy, &signed), closed()).into_response());
    }

    Ok(page(
        &chrome(&instance, &policy, &signed),
        form(&signed, None, None),
    )
    .into_response())
}

/// Records a request.
pub async fn request(
    State(instance): State<Instance>,
    signed: SignedIn,
    axum::Form(asked): axum::Form<Asked>,
) -> Result<Response, WebError> {
    signed.check_form(&asked.form_token)?;

    let policy = instance.policy().await?;

    if !policy.group_requests_open {
        return Ok(page(&chrome(&instance, &policy, &signed), closed()).into_response());
    }

    let refuse = |problem: String, asked: &Asked| {
        (
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            page(
                &chrome(&instance, &policy, &signed),
                form(&signed, Some(&problem), Some(asked)),
            ),
        )
            .into_response()
    };

    let slug = match Slug::parse(asked.slug.trim()) {
        Ok(slug) => slug,
        Err(error) => return Ok(refuse(error.to_string(), &asked)),
    };

    if asked.name.trim().is_empty() {
        return Ok(refuse("The group needs a name.".to_owned(), &asked));
    }
    if asked.reason.trim().is_empty() {
        return Ok(refuse(
            "Say what the group is for. A maintainer reads this.".to_owned(),
            &asked,
        ));
    }

    let taken = {
        let slug = slug.clone();
        !instance
            .read(move |connection| groups::is_free(connection, &slug))
            .await?
    };

    if taken {
        return Ok(refuse(
            format!("`{slug}` is already a group here. Choose another short name."),
            &asked,
        ));
    }

    let user = signed.user.id.clone();
    let name = asked.name.trim().to_owned();
    let reason = asked.reason.trim().to_owned();

    instance
        .read(move |connection| groups::request(connection, &slug, &name, &reason, &user))
        .await?;

    Ok(Redirect::to("/").into_response())
}

/// The shell these pages are drawn in.
fn chrome(
    instance: &Instance,
    policy: &crate::store::settings::Policy,
    signed: &SignedIn,
) -> Chrome {
    Chrome::anonymous(
        "Ask for a group",
        super::front::instance_name(instance, policy),
    )
    .signed_in(signed.user.clone(), signed.maintainer)
}

/// The form itself.
fn form(signed: &SignedIn, problem: Option<&str>, given: Option<&Asked>) -> Markup {
    let value = |pick: fn(&Asked) -> &str| given.map(pick).unwrap_or_default();

    html! {
        div class="sheet--narrow stack" {
            h1 { "Ask for a group" }
            p class="quiet" {
                "A maintainer of this instance reads the request. If they approve it, "
                "the group exists and you are its owner."
            }

            @if let Some(problem) = problem {
                p class="problem" { (problem) }
            }

            form method="post" action="/groups/new" {
                (signature(signed))

                div class="field" {
                    label for="name" { "Name" }
                    input type="text" id="name" name="name" value=(value(|asked| &asked.name))
                          required;
                }

                div class="field" {
                    label for="slug" { "Short name" }
                    input type="text" id="slug" name="slug" value=(value(|asked| &asked.slug))
                          required;
                    span class="field__hint" {
                        "Lowercase letters, digits and hyphens. The group's repositories are "
                        "reached through it, so people will type it and pass it around."
                    }
                }

                div class="field" {
                    label for="reason" { "What is it for?" }
                    textarea id="reason" name="reason" required { (value(|asked| &asked.reason)) }
                }

                button class="button" type="submit" { "Send the request" }
            }
        }
    }
}

/// What an instance that does not take requests says.
fn closed() -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Groups are not requested here" }
            p {
                "On this instance a maintainer creates groups. Ask whoever runs it."
            }
            p { a href="/" { "Back to your home" } }
        }
    }
}

/// How one of somebody's own requests reads on their home page.
pub fn own_request(request: &Request) -> Markup {
    html! {
        li {
            span class="register__name" { (request.name) }
            span class="quiet identifier" { "grp:" (request.slug) }
            span class="register__aside" {
                @match request.outcome {
                    None => "waiting for a maintainer",
                    Some(groups::Outcome::Approved) => "approved",
                    Some(groups::Outcome::Declined) => "declined",
                }
            }
            @if let Some(note) = &request.note {
                p class="quiet" { (note) }
            }
        }
    }
}

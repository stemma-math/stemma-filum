//! Setting a new password from a link.
//!
//! The link is handed out on the machine, or by mail where an instance has
//! it. Either way it is the same link, used once, and using it does not
//! sign anybody in: whoever set the password proves it by logging in with
//! it.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;

use super::layout::{Chrome, page};
use super::{Instance, WebError};
use crate::password;
use crate::store::{resets, sessions, users};

/// What the form sends.
#[derive(Debug, Deserialize)]
pub struct NewPassword {
    /// The password to start using.
    pub password: String,
}

/// The form, if the link still works.
pub async fn form(
    State(instance): State<Instance>,
    Path(secret): Path<String>,
) -> Result<Response, WebError> {
    let policy = instance.policy().await?;
    let name = super::front::instance_name(&instance, &policy);
    let chrome = Chrome::anonymous("Set a password", name).without_ways_in();

    let key = instance.key.clone();
    let held = {
        let secret = secret.clone();
        instance
            .read(move |connection| resets::holder(connection, &key, &secret))
            .await?
    };

    if held.is_none() {
        return Ok((StatusCode::GONE, page(&chrome, spent())).into_response());
    }

    Ok(page(&chrome, body(&secret, None)).into_response())
}

/// Sets the password and sends the person to log in with it.
pub async fn set(
    State(instance): State<Instance>,
    Path(secret): Path<String>,
    axum::Form(form): axum::Form<NewPassword>,
) -> Result<Response, WebError> {
    let policy = instance.policy().await?;
    let name = super::front::instance_name(&instance, &policy);
    let chrome = Chrome::anonymous("Set a password", name).without_ways_in();

    if let Err(weak) = password::check(&form.password) {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            page(&chrome, body(&secret, Some(&weak.to_string()))),
        )
            .into_response());
    }

    let hash = password::hash(&form.password)?;
    let key = instance.key.clone();

    let changed = instance
        .write(move |connection| {
            let transaction = connection.transaction()?;

            let Some(user) = resets::holder(&transaction, &key, &secret)? else {
                return Ok(false);
            };

            if !resets::spend(&transaction, &key, &secret)? {
                return Ok(false);
            }

            users::set_password(&transaction, &user, &hash)?;

            // Anybody else holding a session on this account loses it: the
            // usual reason for setting a new password is that somebody
            // might.
            sessions::end_all(&transaction, &user)?;
            transaction.commit()?;

            Ok(true)
        })
        .await?;

    if !changed {
        return Ok((StatusCode::GONE, page(&chrome, spent())).into_response());
    }

    Ok(Redirect::to("/login").into_response())
}

/// The form itself.
fn body(secret: &str, problem: Option<&str>) -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Set a password" }

            @if let Some(problem) = problem {
                p class="problem" { (problem) }
            }

            form method="post" action={ "/reset/" (secret) } {
                div class="field" {
                    label for="password" { "New password" }
                    input type="password" id="password" name="password"
                          autocomplete="new-password" autofocus required;
                    span class="field__hint" {
                        "At least " (password::MINIMUM_LENGTH) " characters."
                    }
                }

                button class="button" type="submit" { "Set it" }
            }

            p class="quiet" {
                "Every session this account holds ends when the password changes."
            }
        }
    }
}

/// What a link that no longer works says.
fn spent() -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "This link no longer works" }
            p {
                "A reset link works once, and for a couple of hours. Ask whoever runs "
                "this instance for another one."
            }
            p { a href="/" { "Go to the front page" } }
        }
    }
}

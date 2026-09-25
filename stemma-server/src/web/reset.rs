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

/// What somebody who cannot get in sends.
#[derive(Debug, Default, Deserialize)]
pub struct Forgotten {
    /// The address they registered with.
    #[serde(default)]
    pub email: String,
}

/// The way back in, for somebody who cannot get in.
pub async fn asking(State(instance): State<Instance>) -> Result<Response, WebError> {
    let policy = instance.policy().await?;
    let name = super::front::instance_name(&instance, &policy);
    let chrome = Chrome::anonymous("Forgotten password", name);

    Ok(page(&chrome, ask(policy.sends_mail(), None)).into_response())
}

/// Sends a link, and says the same thing either way.
///
/// The answer never depends on whether the address has an account here.
/// Telling somebody which addresses are registered is telling anybody
/// who asks, and an instance that hosts one research group would be
/// telling them who is in it.
pub async fn forgotten(
    State(instance): State<Instance>,
    axum::Form(form): axum::Form<Forgotten>,
) -> Result<Response, WebError> {
    let policy = instance.policy().await?;
    let name = super::front::instance_name(&instance, &policy);
    let chrome = Chrome::anonymous("Forgotten password", name);
    let address = form.email.trim().to_owned();

    if !policy.sends_mail() {
        return Ok(page(&chrome, ask(false, None)).into_response());
    }

    let key = instance.key.clone();
    let asked = address.clone();
    let made = instance
        .read(move |connection| {
            let Some(user) = users::by_email(connection, &asked)? else {
                return Ok(None);
            };

            Ok(Some(resets::create(connection, &key, &user.id)?))
        })
        .await?;

    if let Some(secret) = made {
        let link = instance
            .settings
            .public_url
            .link(&format!("reset/{secret}"));

        instance
            .deliver(
                &address,
                "Setting your password",
                &format!(
                    "Somebody asked for a way to set the password on this account.\n\n\
                     Open this link to set one. It works once, and for a couple of \
                     hours.\n\n{link}\n\n\
                     If it was not you, nothing has happened and you can ignore this."
                ),
            )
            .await;
    }

    Ok(page(&chrome, ask(true, Some(&address))).into_response())
}

/// The form, and what it says once it has been sent.
fn ask(sends_mail: bool, sent_to: Option<&str>) -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Forgotten password" }

            @if !sends_mail {
                p {
                    "Email delivery is not configured. Ask a platform maintainer to create "
                    "a reset link with "
                    code { "stemma-server user reset-password" } "."
                }
            } @else if let Some(address) = sent_to {
                p {
                    "If an account exists for " (address) ", a password reset link has been sent."
                }
                p class="quiet" {
                    "The link can be used once and expires in a few hours."
                }
            } @else {
                p {
                    "Enter your email address to receive a password reset link."
                }

                form method="post" action="/forgotten" class="stack" {
                    div class="field" {
                        label class="field__label" for="email" { "Email address" }
                        input class="input" type="email" id="email" name="email" autofocus required;
                    }

                    button class="button button--primary" type="submit" { "Send me a link" }
                }
            }

            p { a href="/login" { "Back to logging in" } }
        }
    }
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

    let offered = form.password.clone();
    let hash = instance.blocking(move || password::hash(&offered)).await?;
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
                    label class="field__label" for="password" { "New password" }
                    input class="input" type="password" id="password" name="password"
                          autocomplete="new-password" autofocus required;
                    span class="field__hint" {
                        "At least " (password::MINIMUM_LENGTH) " characters."
                    }
                }

                button class="button button--primary" type="submit" { "Set it" }
            }

            p class="quiet" {
                "Changing the password will sign this account out on every device."
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
                "This reset link has expired or has already been used. Request a new one."
            }
            p { a href="/" { "Go to the front page" } }
        }
    }
}

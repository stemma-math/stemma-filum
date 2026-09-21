//! Signing a machine in, from both ends.
//!
//! The machine talks to the two endpoints under `/api/device`; the person
//! approves at `/device` in a browser they are already signed in on. The
//! two halves never talk to each other directly, which is the whole point:
//! the machine may be on the other side of an SSH connection with no
//! browser and no address anybody can reach.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Redirect, Response};
use maud::{Markup, html};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::layout::{Chrome, page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::devices::{self, Collected, Grant};

/// What a machine says when it starts.
#[derive(Debug, Deserialize)]
pub struct Starting {
    /// What the machine calls itself, for the person approving it.
    #[serde(default)]
    pub label: String,
}

/// What the machine is told to do next.
#[derive(Debug, Serialize)]
pub struct Started {
    /// The secret the machine keeps.
    pub device_code: String,
    /// The code the person reads and types.
    pub user_code: String,
    /// Where they go to type it.
    pub verification_uri: String,
    /// The same place, with the code already in it.
    pub verification_uri_complete: String,
    /// How long they have, in seconds.
    pub expires_in: u32,
    /// How often the machine should ask again, in seconds.
    pub interval: u32,
}

/// What a machine sends while it waits.
#[derive(Debug, Deserialize)]
pub struct Collecting {
    /// The secret it was given.
    pub device_code: String,
}

/// What the browser puts in the address bar.
#[derive(Debug, Default, Deserialize)]
pub struct Typed {
    /// The code, where the machine printed a link with it in.
    #[serde(default)]
    pub code: Option<String>,
}

/// What the approval form sends.
#[derive(Debug, Deserialize)]
pub struct Answer {
    /// The code being answered for.
    pub code: String,
    /// Which button was pressed.
    #[serde(default)]
    pub approve: Option<String>,
    /// What says the form came from a page we served.
    pub form_token: String,
}

/// Starts an attempt and tells the machine what to print.
pub async fn start(
    State(instance): State<Instance>,
    Json(starting): Json<Starting>,
) -> Result<Response, WebError> {
    let label = match starting.label.trim() {
        "" => "an unnamed machine".to_owned(),
        given => given.chars().take(60).collect(),
    };

    let key = instance.key.clone();
    let (grant, device_code) = instance
        .read(move |connection| devices::start(connection, &key, &label))
        .await?;

    let user_code = devices::readable(&grant.user_code);
    let verification_uri = instance.settings.public_url.link("device");

    Ok(Json(Started {
        verification_uri_complete: format!("{verification_uri}?code={user_code}"),
        verification_uri,
        device_code,
        user_code,
        expires_in: devices::LIFETIME_MINUTES * 60,
        interval: devices::POLL_SECONDS,
    })
    .into_response())
}

/// Hands the machine its credential, once somebody has approved.
///
/// The shape of the refusals follows the device authorization grant, so a
/// client that already speaks it needs nothing special here.
pub async fn collect(
    State(instance): State<Instance>,
    Json(collecting): Json<Collecting>,
) -> Result<Response, WebError> {
    let key = instance.key.clone();
    let code = collecting.device_code;

    let outcome = instance
        .write(move |connection| devices::collect(connection, &key, &code))
        .await?;

    let refusal = |error: &str, description: &str| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error, "error_description": description })),
        )
            .into_response()
    };

    Ok(match outcome {
        Collected::Waiting => refusal(
            "authorization_pending",
            "nobody has approved this sign-in yet",
        ),
        Collected::Denied => refusal("access_denied", "the sign-in was refused"),
        Collected::Gone => refusal(
            "expired_token",
            "this sign-in is no longer waiting for an answer",
        ),
        Collected::Issued { user, token } => Json(json!({
            "access_token": token,
            "token_type": "bearer",
            "user": user.to_string(),
        }))
        .into_response(),
    })
}

/// The page where somebody approves a machine.
pub async fn form(
    State(instance): State<Instance>,
    signed: SignedIn,
    Query(typed): Query<Typed>,
) -> Result<Response, WebError> {
    let policy = instance.policy().await?;
    let chrome = Chrome::anonymous(
        "Sign a machine in",
        super::front::instance_name(&instance, &policy),
    )
    .signed_in(signed.user.clone(), signed.maintainer);

    let Some(code) = typed.code else {
        return Ok(page(&chrome, ask(&signed, "", None)).into_response());
    };

    match found(&instance, &code).await? {
        Some(grant) => Ok(page(&chrome, confirm(&signed, &grant, &code)).into_response()),
        None => Ok((
            StatusCode::NOT_FOUND,
            page(&chrome, ask(&signed, &code, Some(UNKNOWN))),
        )
            .into_response()),
    }
}

/// Approves or refuses a machine.
pub async fn answer(
    State(instance): State<Instance>,
    signed: SignedIn,
    axum::Form(answer): axum::Form<Answer>,
) -> Result<Response, WebError> {
    signed.check_form(&answer.form_token)?;

    let policy = instance.policy().await?;
    let chrome = Chrome::anonymous(
        "Sign a machine in",
        super::front::instance_name(&instance, &policy),
    )
    .signed_in(signed.user.clone(), signed.maintainer);

    let Some(grant) = found(&instance, &answer.code).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            page(&chrome, ask(&signed, &answer.code, Some(UNKNOWN))),
        )
            .into_response());
    };

    let approving = answer.approve.is_some();
    let user = signed.user.id.clone();
    let id = grant.id.clone();

    let acted = instance
        .read(move |connection| match approving {
            true => devices::approve(connection, &id, &user),
            false => devices::deny(connection, &id),
        })
        .await?;

    if !acted {
        return Ok((
            StatusCode::CONFLICT,
            page(&chrome, ask(&signed, &answer.code, Some(ANSWERED))),
        )
            .into_response());
    }

    Ok(page(&chrome, answered(&grant, approving)).into_response())
}

/// Somebody signed in has no reason to be on this page without a code.
pub async fn redirect() -> Redirect {
    Redirect::to("/device")
}

/// The attempt a code names.
async fn found(instance: &Instance, code: &str) -> Result<Option<Grant>, WebError> {
    let code = code.to_owned();

    instance
        .read(move |connection| devices::by_user_code(connection, &code))
        .await
}

/// What is said about a code that names nothing.
const UNKNOWN: &str =
    "The code is invalid or has expired. Start the sign-in process again on the machine.";

/// What is said about a code somebody already answered.
const ANSWERED: &str = "That sign-in has already been answered.";

/// The form that asks for a code.
fn ask(signed: &SignedIn, code: &str, problem: Option<&str>) -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Sign a machine in" }
            p class="quiet" {
                "Enter the code shown in the desktop app to connect that machine to your account."
            }

            @if let Some(problem) = problem {
                p class="problem" { (problem) }
            }

            form method="get" action="/device" {
                div class="field" {
                    label for="code" { "The code on the machine" }
                    input type="text" id="code" name="code" value=(code)
                          autocomplete="off" autocapitalize="characters" autofocus required;
                }

                button class="button" type="submit" { "Continue" }
            }

            p class="quiet identifier" { (signed.user.id.to_string()) }
        }
    }
}

/// The page that says exactly what is being allowed.
fn confirm(signed: &SignedIn, grant: &Grant, code: &str) -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Let this machine act for you?" }

            div class="block" {
                p class="label" { "The machine says it is" }
                p { (grant.label) }

                p class="label" { "It would act as" }
                p class="identifier" { (signed.user.id.to_string()) }

                p class="quiet" {
                    "This machine will have access to all of your groups until you revoke it."
                }
            }

            form method="post" action="/device" {
                (signature(signed))
                input type="hidden" name="code" value=(code);

                div class="actions" {
                    button class="button" type="submit" name="approve" value="yes" {
                        "Approve"
                    }
                    button class="button button--quiet" type="submit" { "Deny" }
                }
            }
        }
    }
}

/// What is said once it is done.
fn answered(grant: &Grant, approved: bool) -> Markup {
    html! {
        div class="sheet--narrow stack" {
            @if approved {
                h1 { "Done" }
                p { (grant.label) " is now connected to your account." }
                p class="quiet" { "You can return to the desktop app." }
            } @else {
                h1 { "Access denied" }
                p { (grant.label) " was not connected." }
                p class="quiet" {
                    "Access was not granted. You can close this page."
                }
            }

            p { a href="/" { "Back to your home" } }
        }
    }
}

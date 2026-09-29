//! Accepting an invitation.
//!
//! One link does the whole thing. Somebody with no account gets one and
//! their membership in a single step, and somebody who already has an
//! account simply joins. The link is the mechanism; mail, where an
//! instance has it, only delivers what the link already is.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use stemma_core::id::Slug;

use super::layout::{Chrome, page, signature};
use super::visitor::Visitor;
use super::{Instance, WebError};
use crate::password;
use crate::store::groups::{self, Group};
use crate::store::invitations::{self, Invitation, Refusal};
use crate::store::sessions;
use crate::store::users::{self, NotCreated};

/// What the form of somebody who already has an account sends.
#[derive(Debug, Deserialize)]
pub struct Joining {
    /// The CSRF token [`crate::web::layout::signature`] put in the form.
    pub form_token: String,
}

/// What the form of somebody with no account sends.
#[derive(Debug, Deserialize)]
pub struct JoiningAndRegistering {
    /// What to call them on screen.
    pub display_name: String,
    /// The name their identifier is made of.
    pub slug: String,
    /// The password.
    pub password: String,
}

/// What a link offers.
pub async fn offer(
    State(instance): State<Instance>,
    visitor: Visitor,
    Path(secret): Path<String>,
) -> Result<Response, WebError> {
    let (invitation, group) = read(&instance, &secret).await?;
    let policy = instance.policy().await?;
    let name = super::front::instance_name(&instance, &policy);

    let chrome = match &visitor.0 {
        Some(signed) => Chrome::anonymous("An invitation", name)
            .signed_in(signed.user.clone(), signed.maintainer),
        None => Chrome::anonymous("An invitation", name).without_ways_in(),
    };

    let now =
        instance
            .read(|connection| {
                Ok(connection
                    .query_one("SELECT datetime('now')", [], |row| row.get::<_, String>(0))?)
            })
            .await?;

    if let Some(refusal) = invitation.refusal(&now) {
        return Ok((StatusCode::GONE, page(&chrome, refused(&group, refusal))).into_response());
    }

    Ok(page(&chrome, body(&visitor, &invitation, &group, &secret, None)).into_response())
}

/// Joins a group as somebody who already has an account.
pub async fn join(
    State(instance): State<Instance>,
    visitor: Visitor,
    Path(secret): Path<String>,
    axum::Form(form): axum::Form<Joining>,
) -> Result<Response, WebError> {
    let Some(signed) = visitor.0 else {
        return Ok(Redirect::to(&format!("/invite/{secret}")).into_response());
    };

    signed.check_form(&form.form_token)?;

    let (invitation, group) = read(&instance, &secret).await?;
    let user = signed.user.id.clone();
    let role = invitation.role;
    let id = invitation.id;
    let group_id = group.id.clone();

    let joined = instance
        .write(move |connection| {
            let transaction = connection.transaction()?;

            if !invitations::accept(&transaction, &id, &user)? {
                return Ok(false);
            }

            groups::add_member(&transaction, &group_id, &user, role)?;
            transaction.commit()?;

            Ok(true)
        })
        .await?;

    if !joined {
        return Err(used_up());
    }

    let joining = signed.user.clone();
    let joined_group = group.clone();
    instance
        .writing_git(move |repositories| {
            crate::repositories::for_member(repositories, &joined_group, &joining)
        })
        .await?;

    Ok(Redirect::to(&format!("/g/{}", group.slug())).into_response())
}

/// Creates an account and joins the group with it, in one step.
pub async fn join_and_register(
    State(instance): State<Instance>,
    visitor: Visitor,
    Path(secret): Path<String>,
    axum::Form(form): axum::Form<JoiningAndRegistering>,
) -> Result<Response, WebError> {
    if visitor.0.is_some() {
        return Ok(Redirect::to(&format!("/invite/{secret}")).into_response());
    }

    let (invitation, group) = read(&instance, &secret).await?;
    let policy = instance.policy().await?;
    let name = super::front::instance_name(&instance, &policy);
    let chrome = Chrome::anonymous("An invitation", name).without_ways_in();

    let refuse = |problem: String| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            page(
                &chrome,
                body(&Visitor(None), &invitation, &group, &secret, Some(&problem)),
            ),
        )
            .into_response()
    };

    let slug = match Slug::parse(form.slug.trim()) {
        Ok(slug) => slug,
        Err(error) => return Ok(refuse(error.to_string())),
    };

    if let Err(error) = password::check(&form.password) {
        return Ok(refuse(error.to_string()));
    }

    // An invitation carrying an address is for the person at that address,
    // so the account it creates uses it. One with no address asks for one.
    let Some(email) = invitation.email.clone() else {
        return Ok(refuse(
            "This invitation does not name an address, so it cannot create an account. \
             Create one first, then open the link again."
                .to_owned(),
        ));
    };

    let display_name = match form.display_name.trim() {
        "" => slug.as_str().to_owned(),
        given => given.to_owned(),
    };
    let offered = form.password.clone();
    let hash = instance.blocking(move || password::hash(&offered)).await?;
    let role = invitation.role;
    let id = invitation.id;
    let group_id = group.id.clone();

    let outcome = instance
        .write(move |connection| {
            let transaction = connection.transaction()?;

            let user = match users::create(&transaction, &slug, &email, &display_name, &hash)? {
                Ok(user) => user,
                Err(problem) => return Ok(Err(Some(problem))),
            };

            // Somebody else used the link while this form was open; the
            // account goes with the transaction.
            if !invitations::accept(&transaction, &id, &user.id)? {
                return Ok(Err(None));
            }

            groups::add_member(&transaction, &group_id, &user.id, role)?;
            transaction.commit()?;

            Ok(Ok(user))
        })
        .await?;

    let user = match outcome {
        Ok(user) => user,
        Err(Some(NotCreated::SlugTaken(slug))) => {
            return Ok(refuse(format!("`{slug}` is taken. Choose another name.")));
        }
        Err(Some(NotCreated::EmailTaken(_))) => {
            return Ok(refuse(
                "There is already an account for that address. Log in, then open the \
                 link again."
                    .to_owned(),
            ));
        }
        Err(None) => return Err(used_up()),
    };

    let joining = user.clone();
    let joined_group = group.clone();
    instance
        .writing_git(move |repositories| {
            crate::repositories::for_member(repositories, &joined_group, &joining)
        })
        .await?;

    let key = instance.key.clone();
    let id = user.id.clone();
    let (_, cookie) = instance
        .read(move |connection| sessions::start(connection, &key, &id))
        .await?;

    Ok(super::auth::signed_in(
        &instance,
        &cookie,
        &format!("/g/{}", group.slug()),
    ))
}

/// What a link somebody already used says when it is used again.
fn used_up() -> WebError {
    WebError::BadRequest("That invitation cannot be used any more.".to_owned())
}

/// The invitation a link names, and the group it leads into.
async fn read(instance: &Instance, secret: &str) -> Result<(Invitation, Group), WebError> {
    let key = instance.key.clone();
    let secret = secret.to_owned();

    let found = instance
        .read(move |connection| {
            let Some(invitation) = invitations::by_secret(connection, &key, &secret)? else {
                return Ok(None);
            };

            let group = groups::by_id(connection, &invitation.group)?;

            Ok(group.map(|group| (invitation, group)))
        })
        .await?;

    found.ok_or(WebError::NotFound)
}

/// What the link says.
fn body(
    visitor: &Visitor,
    invitation: &Invitation,
    group: &Group,
    secret: &str,
    problem: Option<&str>,
) -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Join " (group.name) }
            p class="quiet" {
                "You have been invited to join as " strong { (invitation.role) } "."
            }
            p class="quiet identifier" { (group.id.to_string()) }

            @if let Some(problem) = problem {
                (super::layout::message(super::layout::Kind::Error, problem))
            }

            @match &visitor.0 {
                Some(signed) => {
                    p {
                        "You are signed in as " span class="identifier" {
                            (signed.user.id.to_string())
                        } "."
                    }
                    form method="post" action={ "/invite/" (secret) "/join" } {
                        (signature(signed))
                        button class="button button--primary" type="submit" { "Join the group" }
                    }
                }
                None => (registration_form(invitation, secret)),
            }
        }
    }
}

/// The form that makes an account and joins in one step.
fn registration_form(invitation: &Invitation, secret: &str) -> Markup {
    html! {
        @match &invitation.email {
            Some(email) => {
                form method="post" action={ "/invite/" (secret) "/register" } {
                    p class="quiet" { "Create an account for " (email) " to accept the invitation." }

                    div class="field" {
                        label class="field__label" for="display_name" { "Your name" }
                        input class="input" type="text" id="display_name" name="display_name"
                              autocomplete="name";
                    }

                    div class="field" {
                        label class="field__label" for="slug" { "Short name" }
                        input class="input input--mono" type="text" id="slug" name="slug" autocomplete="username" required;
                        span class="field__hint" {
                            "Use lowercase letters, numbers and hyphens."
                        }
                    }

                    div class="field" {
                        label class="field__label" for="password" { "Password" }
                        input class="input" type="password" id="password" name="password"
                              autocomplete="new-password" required;
                        span class="field__hint" {
                            "At least " (password::MINIMUM_LENGTH) " characters."
                        }
                    }

                    button class="button button--primary" type="submit" { "Create the account and join" }
                }
            }
            None => {
                p {
                    "Log in to accept this invitation. A personal invitation is required "
                    "to create a new account."
                }
                p {
                    a class="button button--primary" href={ "/login?next=/invite/" (secret) } { "Log in" }
                }
            }
        }
    }
}

/// What a link that no longer works says.
fn refused(group: &Group, refusal: Refusal) -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "This link no longer works" }
            p { (refusal.to_string()) "." }
            p class="quiet" {
                "Ask an administrator of " (group.name) " for a new invitation."
            }
        }
    }
}

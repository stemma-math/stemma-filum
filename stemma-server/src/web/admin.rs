//! The admin area.
//!
//! It keeps the same palette as the rest of the instance: operating a
//! server is not a different product. What it does ask for is the password
//! again, on a plain full-page form rather than a modal, because holding a
//! session is not the same as being at the keyboard.
//!
//! A maintainer approves groups, suspends accounts and edits policy. They
//! cannot read the mathematics of a group they are not in, and nothing
//! here offers to.

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;

use super::auth::Next;
use super::layout::{Chrome, page, signature};
use super::visitor::{Maintainer, SignedIn, safe_destination};
use super::{Instance, WebError};
use crate::password;
use crate::store::groups::{self, Request};
use crate::store::settings::{self, Policy};
use crate::store::users::{self, User};

/// What the re-authentication form sends.
#[derive(Debug, Deserialize)]
pub struct Proof {
    /// The password, typed again.
    pub password: String,
    /// Where the maintainer was going.
    #[serde(default)]
    pub next: Option<String>,
    /// What says the form came from a page we served.
    pub form_token: String,
}

/// What a decision on a group request sends.
#[derive(Debug, Deserialize)]
pub struct Decision {
    /// What the maintainer wants to say about it.
    #[serde(default)]
    pub note: String,
    /// What says the form came from a page we served.
    pub form_token: String,
}

/// What the settings form sends.
#[derive(Debug, Deserialize)]
pub struct InstanceSettings {
    /// What the instance calls itself.
    pub name: String,
    /// Who runs it.
    pub operator: String,
    /// How to reach them.
    pub contact: String,
    /// Whether anyone may create an account.
    #[serde(default)]
    pub registration_open: Option<String>,
    /// Whether account holders may ask for a group.
    #[serde(default)]
    pub group_requests_open: Option<String>,
    /// How many days an invitation lasts.
    pub invitation_ttl_days: String,
    /// What says the form came from a page we served.
    pub form_token: String,
}

/// The form that asks for the password again.
pub async fn authenticate_form(
    State(instance): State<Instance>,
    signed: SignedIn,
    Query(next): Query<Next>,
) -> Result<Response, WebError> {
    let policy = instance.policy().await?;

    Ok(page(
        &chrome(&instance, &policy, &signed, "Confirm it is you"),
        authenticate_body(&signed, next.next.as_deref(), None),
    )
    .into_response())
}

/// Checks the password and lets the maintainer through.
pub async fn authenticate(
    State(instance): State<Instance>,
    signed: SignedIn,
    axum::Form(proof): axum::Form<Proof>,
) -> Result<Response, WebError> {
    signed.check_form(&proof.form_token)?;

    let id = signed.user.id.clone();
    let stored = instance
        .read(move |connection| users::password_hash(connection, &id))
        .await?;

    let correct = stored.is_some_and(|stored| password::verify(&proof.password, &stored));

    if !correct {
        let policy = instance.policy().await?;

        return Ok((
            axum::http::StatusCode::UNAUTHORIZED,
            page(
                &chrome(&instance, &policy, &signed, "Confirm it is you"),
                authenticate_body(
                    &signed,
                    proof.next.as_deref(),
                    Some("That is not the password for this account."),
                ),
            ),
        )
            .into_response());
    }

    let session = signed.session.id;
    instance
        .read(move |connection| crate::store::sessions::mark_reauthenticated(connection, &session))
        .await?;

    let destination = match safe_destination(proof.next.as_deref()).as_str() {
        "/" => "/admin".to_owned(),
        path => path.to_owned(),
    };

    Ok(Redirect::to(&destination).into_response())
}

/// The admin area itself.
pub async fn area(
    State(instance): State<Instance>,
    Maintainer(signed): Maintainer,
) -> Result<Response, WebError> {
    let policy = instance.policy().await?;

    let (requests, accounts, maintainers) = instance
        .read(|connection| {
            let requests = groups::open_requests(connection)?;
            let mut named = Vec::new();

            for request in requests {
                let requester = groups::requester(connection, &request)?;
                named.push((request, requester));
            }

            Ok((
                named,
                users::all(connection)?,
                users::holders(connection, stemma_core::access::Grant::Maintainer)?,
            ))
        })
        .await?;

    let body = html! {
        div class="stack" {
            h1 { "Admin" }
            p class="quiet" {
                "You are operating this instance. Nothing here opens the mathematics of "
                "a group you are not in."
            }
        }

        (requests_block(&signed, &requests))
        (accounts_block(&accounts, &maintainers))
        (settings_block(&signed, &policy))
    };

    Ok(page(
        &chrome(&instance, &policy, &signed, "Admin").at("/admin"),
        body,
    )
    .into_response())
}

/// Approves a group request, which creates the group.
pub async fn approve(
    State(instance): State<Instance>,
    Maintainer(signed): Maintainer,
    Path(id): Path<String>,
    axum::Form(decision): axum::Form<Decision>,
) -> Result<Response, WebError> {
    signed.check_form(&decision.form_token)?;

    let maintainer = signed.user.id.clone();
    let note = trimmed(&decision.note);

    let made = instance
        .write(move |connection| {
            let Some(request) = groups::request_by_id(connection, &id)? else {
                return Ok(None);
            };

            if !request.is_open() {
                return Ok(None);
            }

            let group = groups::approve(connection, &request, &maintainer, note.as_deref())?;
            let owner = users::by_id(connection, &request.requested_by)?;

            Ok(owner.map(|owner| (group, owner)))
        })
        .await?;

    // A group without a repository is a group nobody can work in, so the
    // two arrive together.
    if let Some((group, owner)) = made {
        instance
            .with_repositories(move |repositories| {
                crate::repositories::for_both(repositories, &group, &owner)
            })
            .await?;
    }

    Ok(Redirect::to("/admin").into_response())
}

/// Declines a group request, which keeps it in the record.
pub async fn decline(
    State(instance): State<Instance>,
    Maintainer(signed): Maintainer,
    Path(id): Path<String>,
    axum::Form(decision): axum::Form<Decision>,
) -> Result<Response, WebError> {
    signed.check_form(&decision.form_token)?;

    let maintainer = signed.user.id.clone();
    let note = trimmed(&decision.note);

    instance
        .read(move |connection| {
            let Some(request) = groups::request_by_id(connection, &id)? else {
                return Ok(());
            };

            if request.is_open() {
                groups::decline(connection, &request, &maintainer, note.as_deref())?;
            }

            Ok(())
        })
        .await?;

    Ok(Redirect::to("/admin").into_response())
}

/// Writes the instance's policy.
pub async fn save_settings(
    State(instance): State<Instance>,
    Maintainer(signed): Maintainer,
    axum::Form(form): axum::Form<InstanceSettings>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let by = signed.user.id.clone();
    let days = form
        .invitation_ttl_days
        .trim()
        .parse::<u32>()
        .unwrap_or(crate::config::DEFAULT_INVITATION_TTL_DAYS)
        .clamp(1, 365);

    instance
        .read(move |connection| {
            for (key, value) in [
                (settings::INSTANCE_NAME, trimmed(&form.name)),
                (settings::INSTANCE_OPERATOR, trimmed(&form.operator)),
                (settings::INSTANCE_CONTACT, trimmed(&form.contact)),
            ] {
                match value {
                    Some(value) => settings::set(connection, key, &value, Some(&by))?,
                    None => settings::clear(connection, key)?,
                }
            }

            settings::set(
                connection,
                settings::REGISTRATION_OPEN,
                &form.registration_open.is_some().to_string(),
                Some(&by),
            )?;
            settings::set(
                connection,
                settings::GROUP_REQUESTS_OPEN,
                &form.group_requests_open.is_some().to_string(),
                Some(&by),
            )?;
            settings::set(
                connection,
                settings::INVITATION_TTL_DAYS,
                &days.to_string(),
                Some(&by),
            )
        })
        .await?;

    Ok(Redirect::to("/admin").into_response())
}

/// What somebody typed, or nothing if they typed nothing.
fn trimmed(value: &str) -> Option<String> {
    match value.trim() {
        "" => None,
        given => Some(given.to_owned()),
    }
}

/// The shell these pages are drawn in.
fn chrome(instance: &Instance, policy: &Policy, signed: &SignedIn, title: &str) -> Chrome {
    Chrome::anonymous(title, super::front::instance_name(instance, policy))
        .signed_in(signed.user.clone(), signed.maintainer)
}

/// The form that asks for the password again.
fn authenticate_body(signed: &SignedIn, next: Option<&str>, problem: Option<&str>) -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Confirm it is you" }
            p class="quiet" {
                "The admin area asks for your password again, however long you have "
                "been signed in."
            }

            @if let Some(problem) = problem {
                p class="problem" { (problem) }
            }

            form method="post" action="/admin/authenticate" {
                (signature(signed))
                @if let Some(next) = next {
                    input type="hidden" name="next" value=(next);
                }

                div class="field" {
                    label for="password" { "Password" }
                    input type="password" id="password" name="password"
                          autocomplete="current-password" autofocus required;
                }

                button class="button" type="submit" { "Continue" }
            }
        }
    }
}

/// The group requests waiting for somebody.
fn requests_block(signed: &SignedIn, requests: &[(Request, Option<User>)]) -> Markup {
    html! {
        div class="block" {
            p class="label" { "Group requests · " (requests.len()) }

            @if requests.is_empty() {
                p class="empty" { "Nobody is waiting for an answer." }
            } @else {
                ul class="register" {
                    @for (request, requester) in requests {
                        li {
                            div class="stack stack--tight" {
                                div class="row" {
                                    span class="register__name" { (request.name) }
                                    span class="quiet identifier" { "grp:" (request.slug) }
                                }

                                p class="quiet" {
                                    "Asked by "
                                    @match requester {
                                        Some(user) => (user.display_name),
                                        None => "somebody whose account is gone",
                                    }
                                    " on " span class="numeric" { (request.requested_at) }
                                }

                                p { (request.reason) }

                                form method="post"
                                     action={ "/admin/requests/" (request.id) "/approve" } {
                                    (signature(signed))
                                    div class="field" {
                                        label for={ "note-" (request.id) } { "A note, if you want one" }
                                        input type="text" id={ "note-" (request.id) } name="note";
                                    }
                                    div class="actions" {
                                        button class="button" type="submit" { "Approve" }
                                        button class="button button--quiet" type="submit"
                                               formaction={ "/admin/requests/" (request.id) "/decline" } {
                                            "Decline"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Who has an account here.
fn accounts_block(accounts: &[User], maintainers: &[users::HeldGrant]) -> Markup {
    html! {
        div class="block" {
            p class="label" { "Accounts · " (accounts.len()) }

            div class="scroll" {
                table {
                    thead {
                        tr {
                            th { "Name" }
                            th { "Identifier" }
                            th { "Email" }
                            th { "Since" }
                            th { "Instance" }
                        }
                    }
                    tbody {
                        @for account in accounts {
                            tr {
                                td { (account.display_name) }
                                td class="identifier" { (account.id.to_string()) }
                                td { (account.email) }
                                td class="numeric" { (account.created_at) }
                                td {
                                    @if maintainers.iter().any(|held| held.user == account.id) {
                                        "maintainer"
                                    } @else if !account.is_active() {
                                        "suspended"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            p class="quiet" {
                "A maintainer is granted from the machine: "
                code { "stemma-server admin grant <account>" } "."
            }
        }
    }
}

/// The policy this instance runs under.
fn settings_block(signed: &SignedIn, policy: &Policy) -> Markup {
    html! {
        div class="block" {
            p class="label" { "Settings" }

            form method="post" action="/admin/settings" {
                (signature(signed))

                div class="field" {
                    label for="name" { "What this instance is called" }
                    input type="text" id="name" name="name"
                          value=(policy.instance_name.clone().unwrap_or_default());
                }

                div class="field" {
                    label for="operator" { "Who runs it" }
                    input type="text" id="operator" name="operator"
                          value=(policy.operator.clone().unwrap_or_default());
                }

                div class="field" {
                    label for="contact" { "How to reach them" }
                    input type="text" id="contact" name="contact"
                          value=(policy.contact.clone().unwrap_or_default());
                }

                div class="field" {
                    label {
                        input type="checkbox" name="registration_open" value="yes"
                              checked[policy.registration_open];
                        " Anyone may create an account"
                    }
                    label {
                        input type="checkbox" name="group_requests_open" value="yes"
                              checked[policy.group_requests_open];
                        " Account holders may ask for a group"
                    }
                }

                div class="field" {
                    label for="invitation_ttl_days" { "An invitation lasts, in days" }
                    input type="text" id="invitation_ttl_days" name="invitation_ttl_days"
                          class="numeric" value=(policy.invitation_ttl_days);
                }

                button class="button" type="submit" { "Save" }
            }

            p class="quiet" {
                @if policy.sends_mail() {
                    "Mail goes out through " code { (policy.mail_smtp_url.clone().unwrap_or_default()) } "."
                } @else {
                    "No mail is configured, so invitations and password resets are links "
                    "shown to whoever creates them."
                }
            }
        }
    }
}

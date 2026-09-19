//! Registering, logging in and logging out.
//!
//! The group is never part of the login. A researcher collaborates with
//! several groups and is one person, so the group is a scope chosen after
//! entering rather than a field on this form.

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use stemma_core::id::Slug;

use super::layout::{Chrome, page};
use super::visitor::{SignedIn, Visitor, safe_destination};
use super::{Instance, WebError, cookies};
use crate::password;
use crate::store::sessions::{self, LIFETIME_DAYS};
use crate::store::users::{self, NotCreated};

/// Where to go once this is over.
#[derive(Debug, Default, Deserialize)]
pub struct Next {
    /// The page somebody was trying to reach.
    pub next: Option<String>,
}

/// What the log-in form sends.
#[derive(Debug, Deserialize)]
pub struct Credentials {
    /// A name or an address. People remember one or the other.
    pub login: String,
    /// The password.
    pub password: String,
    /// Where to go afterwards.
    #[serde(default)]
    pub next: Option<String>,
}

/// What the registration form sends.
#[derive(Debug, Deserialize)]
pub struct Registration {
    /// What to call this person on screen.
    pub display_name: String,
    /// The name their identifier is made of.
    pub slug: String,
    /// Where mail to them goes.
    pub email: String,
    /// The password.
    pub password: String,
}

/// The log-in form.
pub async fn login_form(
    State(instance): State<Instance>,
    visitor: Visitor,
    Query(next): Query<Next>,
) -> Result<Response, WebError> {
    if visitor.0.is_some() {
        return Ok(Redirect::to(&safe_destination(next.next.as_deref())).into_response());
    }

    let policy = instance.policy().await?;
    let name = super::front::instance_name(&instance, &policy);
    let chrome = Chrome::anonymous("Log in", name)
        .at("/login")
        .registration(policy.registration_open);

    Ok(page(
        &chrome,
        login_body(next.next.as_deref(), policy.registration_open, None),
    )
    .into_response())
}

/// Checks a password and starts a session.
pub async fn log_in(
    State(instance): State<Instance>,
    axum::Form(credentials): axum::Form<Credentials>,
) -> Result<Response, WebError> {
    let destination = safe_destination(credentials.next.as_deref());
    let login = credentials.login.clone();
    let offered = credentials.password.clone();

    let found = instance
        .read(move |connection| {
            let Some(user) = users::by_login(connection, &login)? else {
                return Ok(None);
            };

            Ok(users::password_hash(connection, &user.id)?.map(|stored| (user, stored)))
        })
        .await?;

    // The same answer whether the account does not exist, the password is
    // wrong or the account is suspended: this form must not be a way of
    // finding out who has an account here.
    let admitted =
        found.filter(|(user, stored)| user.is_active() && password::verify(&offered, stored));

    let Some((user, _)) = admitted else {
        let policy = instance.policy().await?;
        let name = super::front::instance_name(&instance, &policy);
        let chrome = Chrome::anonymous("Log in", name)
            .at("/login")
            .registration(policy.registration_open);

        return Ok((
            axum::http::StatusCode::UNAUTHORIZED,
            page(
                &chrome,
                login_body(
                    credentials.next.as_deref(),
                    policy.registration_open,
                    Some("That name or address and password do not go together."),
                ),
            ),
        )
            .into_response());
    };

    let key = instance.key.clone();
    let id = user.id.clone();
    let (_, cookie) = instance
        .read(move |connection| sessions::start(connection, &key, &id))
        .await?;

    Ok(signed_in(&instance, &cookie, &destination))
}

/// Ends the session this browser holds.
pub async fn log_out(
    State(instance): State<Instance>,
    signed: Option<SignedIn>,
) -> Result<Response, WebError> {
    if let Some(signed) = signed {
        let id = signed.session.id;
        instance
            .read(move |connection| sessions::end(connection, &id))
            .await?;
    }

    let mut headers = HeaderMap::new();
    cookies::attach(
        &mut headers,
        cookies::clear(cookies::SESSION, instance.settings.public_url.is_secure()),
    );

    Ok((headers, Redirect::to("/")).into_response())
}

/// The registration form.
pub async fn register_form(
    State(instance): State<Instance>,
    visitor: Visitor,
) -> Result<Response, WebError> {
    if visitor.0.is_some() {
        return Ok(Redirect::to("/").into_response());
    }

    let policy = instance.policy().await?;
    let accounts = instance.read(users::count).await?;
    let name = super::front::instance_name(&instance, &policy);

    // A closed instance still lets the very first account be made, because
    // otherwise nobody could ever get in and the operator would have to
    // edit the database by hand.
    if !policy.registration_open && accounts > 0 {
        return Ok(page(
            &Chrome::anonymous("Create an account", name).at("/register"),
            closed(),
        )
        .into_response());
    }

    let chrome = Chrome::anonymous("Create an account", name)
        .at("/register")
        .registration(true);

    Ok(page(&chrome, register_body(None, None)).into_response())
}

/// Creates an account and signs the person in.
pub async fn register(
    State(instance): State<Instance>,
    axum::Form(form): axum::Form<Registration>,
) -> Result<Response, WebError> {
    let policy = instance.policy().await?;
    let accounts = instance.read(users::count).await?;
    let name = super::front::instance_name(&instance, &policy);

    if !policy.registration_open && accounts > 0 {
        return Ok(page(
            &Chrome::anonymous("Create an account", name).at("/register"),
            closed(),
        )
        .into_response());
    }

    let refuse = |problem: String, form: &Registration| {
        let chrome = Chrome::anonymous("Create an account", name.clone())
            .at("/register")
            .registration(true);

        (
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            page(&chrome, register_body(Some(&problem), Some(form))),
        )
            .into_response()
    };

    let slug = match Slug::parse(form.slug.trim()) {
        Ok(slug) => slug,
        Err(error) => return Ok(refuse(error.to_string(), &form)),
    };

    if let Err(error) = check_email(form.email.trim()) {
        return Ok(refuse(error, &form));
    }
    if let Err(error) = password::check(&form.password) {
        return Ok(refuse(error.to_string(), &form));
    }

    let display_name = match form.display_name.trim() {
        "" => slug.as_str().to_owned(),
        given => given.to_owned(),
    };

    let hash = password::hash(&form.password)?;
    let email = form.email.trim().to_owned();
    let wanted = slug.clone();

    let created = instance
        .read(move |connection| users::create(connection, &wanted, &email, &display_name, &hash))
        .await?;

    let user = match created {
        Ok(user) => user,
        Err(NotCreated::SlugTaken(slug)) => {
            return Ok(refuse(
                format!("`{slug}` is taken. Choose another name."),
                &form,
            ));
        }
        Err(NotCreated::EmailTaken(_)) => {
            return Ok(refuse(
                "There is already an account for that address. Try logging in.".to_owned(),
                &form,
            ));
        }
    };

    let key = instance.key.clone();
    let id = user.id.clone();
    let (_, cookie) = instance
        .read(move |connection| sessions::start(connection, &key, &id))
        .await?;

    Ok(signed_in(&instance, &cookie, "/"))
}

/// The answer that hands over a session and sends somebody on.
fn signed_in(instance: &Instance, cookie: &str, destination: &str) -> Response {
    let mut headers = HeaderMap::new();
    cookies::attach(
        &mut headers,
        cookies::set(
            cookies::SESSION,
            cookie,
            i64::from(LIFETIME_DAYS) * 24 * 60 * 60,
            instance.settings.public_url.is_secure(),
        ),
    );

    (headers, Redirect::to(destination)).into_response()
}

/// Whether an address could be one.
///
/// The only test that means anything is sending mail to it, and this
/// instance may have no mail at all, so this rejects what is certainly
/// wrong and accepts the rest.
fn check_email(email: &str) -> Result<(), String> {
    let shape = email.split_once('@');

    match shape {
        Some((before, after))
            if !before.is_empty()
                && after.contains('.')
                && !after.starts_with('.')
                && !after.ends_with('.')
                && !email.contains(char::is_whitespace) =>
        {
            Ok(())
        }
        _ => Err(format!("`{email}` does not look like an email address.")),
    }
}

/// The log-in form's body.
fn login_body(next: Option<&str>, registration_open: bool, problem: Option<&str>) -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Log in" }

            @if let Some(problem) = problem {
                p class="problem" { (problem) }
            }

            form method="post" action="/login" {
                @if let Some(next) = next {
                    input type="hidden" name="next" value=(next);
                }

                div class="field" {
                    label for="login" { "Name or email address" }
                    input type="text" id="login" name="login" autocomplete="username"
                          autofocus required;
                }

                div class="field" {
                    label for="password" { "Password" }
                    input type="password" id="password" name="password"
                          autocomplete="current-password" required;
                }

                button class="button" type="submit" { "Log in" }
            }

            p class="quiet" {
                a href="/forgotten" { "Forgotten your password?" }
            }

            @if registration_open {
                p class="quiet" {
                    "No account here yet? " a href="/register" { "Create one" } "."
                }
            }
        }
    }
}

/// The registration form's body.
fn register_body(problem: Option<&str>, given: Option<&Registration>) -> Markup {
    let value = |pick: fn(&Registration) -> &str| given.map(pick).unwrap_or_default().to_owned();

    html! {
        div class="sheet--narrow stack" {
            h1 { "Create an account" }
            p class="quiet" {
                "An account belongs to a person, not to a group. You join groups "
                "afterwards, by invitation."
            }

            @if let Some(problem) = problem {
                p class="problem" { (problem) }
            }

            form method="post" action="/register" {
                div class="field" {
                    label for="display_name" { "Your name" }
                    input type="text" id="display_name" name="display_name"
                          value=(value(|form| &form.display_name)) autocomplete="name";
                }

                div class="field" {
                    label for="slug" { "Short name" }
                    input type="text" id="slug" name="slug" value=(value(|form| &form.slug))
                          autocomplete="username" required;
                    span class="field__hint" {
                        "Lowercase letters, digits and hyphens. Your identifier will be "
                        code { "usr:" } "followed by it, and it is how people refer to you."
                    }
                }

                div class="field" {
                    label for="email" { "Email address" }
                    input type="email" id="email" name="email" value=(value(|form| &form.email))
                          autocomplete="email" required;
                }

                div class="field" {
                    label for="password" { "Password" }
                    input type="password" id="password" name="password"
                          autocomplete="new-password" required;
                    span class="field__hint" {
                        "At least " (password::MINIMUM_LENGTH) " characters. Length is the "
                        "only rule."
                    }
                }

                button class="button" type="submit" { "Create the account" }
            }
        }
    }
}

/// What a closed instance says to somebody who wanted an account.
fn closed() -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Accounts here are by invitation" }
            p {
                "This instance does not take open registrations. Somebody in a group "
                "can send you a link, which creates your account and puts you in their "
                "group in one step."
            }
            p { a href="/login" { "Log in" } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_without_a_host_is_refused() {
        for wrong in [
            "ana",
            "ana@",
            "@example.org",
            "ana@example",
            "a b@example.org",
        ] {
            assert!(check_email(wrong).is_err(), "took `{wrong}`");
        }
    }

    #[test]
    fn an_ordinary_address_is_accepted() {
        for right in ["ana@example.org", "ana.ferrer+stemma@uv.es"] {
            assert!(check_email(right).is_ok(), "refused `{right}`");
        }
    }
}

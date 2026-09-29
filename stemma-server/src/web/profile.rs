//! Somebody's own profile: what they are called, where mail reaches them,
//! their password and their picture.
//!
//! The short name is not here. It is in the account's identifier, and so
//! in every record of what the account has done.

use axum::extract::{Multipart, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;

use super::layout::{page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::password;
use crate::store::pictures;
use crate::store::sessions;
use crate::store::users::{self, NotCreated};

/// Which part of the page a message is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Picture,
    Details,
    Password,
}

/// What was just done, as the page's address says it.
#[derive(Debug, Default, Deserialize)]
pub struct Done {
    /// Which change went through.
    pub done: Option<String>,
}

/// What the details form sends.
#[derive(Debug, Deserialize)]
pub struct Details {
    /// What to be called on screen.
    pub display_name: String,
    /// Where mail should reach them, and what they log in with.
    pub email: String,
    /// The password, needed only to move the account to another address.
    #[serde(default)]
    pub current_password: String,
    /// The CSRF token [`crate::web::layout::signature`] put in the form.
    pub form_token: String,
}

/// What the password form sends.
#[derive(Debug, Deserialize)]
pub struct NewPassword {
    /// The password they have now.
    pub current_password: String,
    /// The one they want.
    pub new_password: String,
    /// The one they want, again.
    pub repeated: String,
    /// The CSRF token [`crate::web::layout::signature`] put in the form.
    pub form_token: String,
}

/// A form that sends nothing but its signature.
#[derive(Debug, Deserialize)]
pub struct Signed {
    /// The CSRF token [`crate::web::layout::signature`] put in the form.
    pub form_token: String,
}

/// The profile.
pub async fn view(
    State(instance): State<Instance>,
    signed: SignedIn,
    Query(done): Query<Done>,
) -> Result<Response, WebError> {
    let said = match done.done.as_deref() {
        Some("picture") => Some((Part::Picture, "Your picture has been changed.")),
        Some("picture-removed") => Some((Part::Picture, "Your picture has been removed.")),
        Some("details") => Some((Part::Details, "Your details have been saved.")),
        Some("password") => Some((
            Part::Password,
            "Your password has been changed, and every other device has been signed out.",
        )),
        _ => None,
    };

    draw(&instance, &signed, StatusCode::OK, said, None, None).await
}

/// Saves the name and the address.
pub async fn details(
    State(instance): State<Instance>,
    signed: SignedIn,
    axum::Form(form): axum::Form<Details>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let refuse = |problem: String| {
        let instance = instance.clone();
        let signed = signed.clone();
        let given = (form.display_name.clone(), form.email.clone());

        async move {
            draw(
                &instance,
                &signed,
                StatusCode::UNPROCESSABLE_ENTITY,
                None,
                Some((Part::Details, problem)),
                Some(given),
            )
            .await
        }
    };

    let display_name = form.display_name.trim().to_owned();
    let email = form.email.trim().to_lowercase();

    if display_name.is_empty() {
        return refuse("Your name cannot be empty.".to_owned()).await;
    }
    if let Err(problem) = crate::address::check(&email) {
        return refuse(problem).await;
    }

    // Moving the account to another address moves the way into it, so
    // it asks for the password first, as a person at an unattended
    // browser would not have it.
    let moving = email != signed.user.email;
    if moving && !confirmed(&instance, &signed, &form.current_password).await? {
        return refuse(
            "Enter your current password to change your email address. The one given is not it."
                .to_owned(),
        )
        .await;
    }

    let id = signed.user.id.clone();
    let saved = instance
        .read(move |connection| {
            if let Err(taken) = users::set_email(connection, &id, &email)? {
                return Ok(Err(taken));
            }
            users::set_display_name(connection, &id, &display_name)?;

            Ok(Ok(()))
        })
        .await?;

    match saved {
        Ok(()) => Ok(Redirect::to("/profile?done=details").into_response()),
        Err(NotCreated::EmailTaken(_) | NotCreated::SlugTaken(_)) => {
            refuse("Another account already uses that email address.".to_owned()).await
        }
    }
}

/// Changes the password, and signs the account out everywhere else.
pub async fn password(
    State(instance): State<Instance>,
    signed: SignedIn,
    axum::Form(form): axum::Form<NewPassword>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let refuse = |problem: &str| {
        draw(
            &instance,
            &signed,
            StatusCode::UNPROCESSABLE_ENTITY,
            None,
            Some((Part::Password, problem.to_owned())),
            None,
        )
    };

    if !confirmed(&instance, &signed, &form.current_password).await? {
        return refuse("Your current password is not the one given.").await;
    }
    if let Err(weak) = password::check(&form.new_password) {
        return refuse(&format!("{}.", capitalised(&weak.to_string()))).await;
    }
    if form.new_password != form.repeated {
        return refuse("The two new passwords are not the same.").await;
    }

    let offered = form.new_password.clone();
    let hash = instance.blocking(move || password::hash(&offered)).await?;
    let id = signed.user.id.clone();
    let here = signed.session.id;

    instance
        .write(move |connection| {
            let transaction = connection.transaction()?;

            users::set_password(&transaction, &id, &hash)?;

            // As a reset does, and for the same reason: the usual reason
            // for a new password is that somebody else might have the old
            // one. The session that changed it stays.
            sessions::end_others(&transaction, &id, &here)?;
            sessions::mark_reauthenticated(&transaction, &here)?;
            transaction.commit()?;

            Ok(())
        })
        .await?;

    Ok(Redirect::to("/profile?done=password").into_response())
}

/// Keeps a new picture.
pub async fn picture(
    State(instance): State<Instance>,
    signed: SignedIn,
    upload: Multipart,
) -> Result<Response, WebError> {
    let (token, bytes) = read_upload(upload).await?;
    signed.check_form(&token)?;

    let id = signed.user.id.clone();
    let kept = instance
        .read(move |connection| pictures::set_user(connection, &id, &bytes))
        .await?;

    match kept {
        Ok(()) => Ok(Redirect::to("/profile?done=picture").into_response()),
        Err(refused) => {
            draw(
                &instance,
                &signed,
                StatusCode::UNPROCESSABLE_ENTITY,
                None,
                Some((Part::Picture, refused.to_string())),
                None,
            )
            .await
        }
    }
}

/// Takes the picture away.
pub async fn remove_picture(
    State(instance): State<Instance>,
    signed: SignedIn,
    axum::Form(form): axum::Form<Signed>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let id = signed.user.id.clone();
    instance
        .read(move |connection| pictures::remove_user(connection, &id))
        .await?;

    Ok(Redirect::to("/profile?done=picture-removed").into_response())
}

/// Reads an upload's signature and its picture.
///
/// Shared by every form that uploads a picture: the token is checked by
/// the caller, which knows whose session it is.
pub(super) async fn read_upload(mut upload: Multipart) -> Result<(String, Vec<u8>), WebError> {
    let unreadable =
        |_| WebError::BadRequest("The upload could not be read. Try once more.".to_owned());

    let mut token = String::new();
    let mut bytes = Vec::new();

    while let Some(field) = upload.next_field().await.map_err(unreadable)? {
        match field.name() {
            Some("form_token") => token = field.text().await.map_err(unreadable)?,
            Some("picture") => bytes = field.bytes().await.map_err(unreadable)?.to_vec(),
            _ => {}
        }
    }

    Ok((token, bytes))
}

/// Whether `offered` is the account's password.
async fn confirmed(
    instance: &Instance,
    signed: &SignedIn,
    offered: &str,
) -> Result<bool, WebError> {
    let id = signed.user.id.clone();
    let stored = instance
        .read(move |connection| users::password_hash(connection, &id))
        .await?;
    let offered = offered.to_owned();

    instance
        .blocking(move || Ok(stored.is_some_and(|stored| password::verify(&offered, &stored))))
        .await
}

/// A sentence starting with a capital.
fn capitalised(sentence: &str) -> String {
    let mut letters = sentence.chars();

    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// Draws the page, with whatever it has to say.
async fn draw(
    instance: &Instance,
    signed: &SignedIn,
    status: StatusCode,
    said: Option<(Part, &str)>,
    problem: Option<(Part, String)>,
    given: Option<(String, String)>,
) -> Result<Response, WebError> {
    let chrome = super::layout::around(instance, signed, "Your profile")
        .await?
        .at("/profile");
    let changed = chrome.workspace.face.clone();

    Ok((
        status,
        page(
            &chrome,
            body(
                signed,
                changed.as_deref(),
                said,
                problem.as_ref(),
                given.as_ref(),
            ),
        ),
    )
        .into_response())
}

/// The page itself.
fn body(
    signed: &SignedIn,
    changed: Option<&str>,
    said: Option<(Part, &str)>,
    problem: Option<&(Part, String)>,
    given: Option<&(String, String)>,
) -> Markup {
    let user = &signed.user;
    let (name, email) = match given {
        Some((name, email)) => (name.as_str(), email.as_str()),
        None => (user.display_name.as_str(), user.email.as_str()),
    };
    let note = |part: Part| {
        html! {
            @if let Some((_, words)) = said.filter(|(about, _)| *about == part) {
                (super::layout::message(super::layout::Kind::Success, words))
            }
            @if let Some((_, words)) = problem.filter(|(about, _)| *about == part) {
                (super::layout::message(super::layout::Kind::Error, words))
            }
        }
    };

    html! {
        header class="page-head" {
            div class="page-head__row" {
                (super::pictures::face(&user.id, &user.display_name, changed, "avatar--large"))
                div class="page-head__text" {
                    h1 class="page-title" { (user.display_name) }
                    div class="page-meta" {
                        span class="identifier" { (user.id.to_string()) }
                        span { (user.email) }
                    }
                }
            }
        }

        div class="settings-page" {
            section class="block" aria-labelledby="picture-title" {
                h2 class="block__title" id="picture-title" { "Picture" }
                (note(Part::Picture))
                (picture_form(
                    "/profile/picture",
                    "/profile/picture/remove",
                    signed,
                    changed.is_some(),
                    "It is shown beside your name to the people in your groups.",
                ))
            }

            section class="block" aria-labelledby="details-title" {
                h2 class="block__title" id="details-title" { "Details" }
                (note(Part::Details))
                form method="post" action="/profile/details" {
                    (signature(signed))

                    div class="field" {
                        label class="field__label" for="display_name" { "Name" }
                        input class="input" type="text" id="display_name" name="display_name"
                              value=(name) autocomplete="name" required;
                    }

                    div class="field" {
                        label class="field__label" for="email" { "Email address" }
                        input class="input" type="email" id="email" name="email"
                              value=(email) autocomplete="email" required;
                        span class="field__hint" { "You can also log in with it." }
                    }

                    div class="field" {
                        label class="field__label" for="details-password" { "Current password" }
                        input class="input" type="password" id="details-password"
                              name="current_password" autocomplete="current-password";
                        span class="field__hint" { "Needed only to change your email address." }
                    }

                    button class="button button--primary" type="submit" { "Save details" }
                }
            }

            section class="block" aria-labelledby="password-title" {
                h2 class="block__title" id="password-title" { "Password" }
                (note(Part::Password))
                form method="post" action="/profile/password" {
                    (signature(signed))

                    div class="field" {
                        label class="field__label" for="current-password" { "Current password" }
                        input class="input" type="password" id="current-password"
                              name="current_password" autocomplete="current-password" required;
                    }

                    div class="field" {
                        label class="field__label" for="new-password" { "New password" }
                        input class="input" type="password" id="new-password" name="new_password"
                              autocomplete="new-password" minlength=(password::MINIMUM_LENGTH) required;
                        span class="field__hint" {
                            "At least " (password::MINIMUM_LENGTH) " characters. "
                            "Every other device signed in to this account will be signed out."
                        }
                    }

                    div class="field" {
                        label class="field__label" for="repeated" { "New password, again" }
                        input class="input" type="password" id="repeated" name="repeated"
                              autocomplete="new-password" required;
                    }

                    button class="button button--primary" type="submit" { "Change password" }
                }
            }
        }
    }
}

/// The two forms a picture is changed with: one to upload, one to remove.
///
/// Shared with a group's page, which says whose picture it is in `hint`.
pub(super) fn picture_form(
    upload_to: &str,
    remove_at: &str,
    signed: &SignedIn,
    has_one: bool,
    hint: &str,
) -> Markup {
    html! {
        form class="picture-form" method="post" action=(upload_to) enctype="multipart/form-data" {
            (signature(signed))
            div class="field" {
                label class="field__label" for="picture" {
                    @if has_one { "Replace the picture" } @else { "Upload a picture" }
                }
                input class="input picture-form__file" type="file" id="picture" name="picture"
                      accept="image/png,image/jpeg,image/webp" required;
                span class="field__hint" {
                    (hint) " A square PNG, JPEG or WebP image of up to 1 MB."
                }
            }
            div class="actions" {
                button class="button button--primary" type="submit" { "Upload" }
                // A form cannot hold another, so the removal is a form of
                // its own that this button belongs to from here.
                @if has_one {
                    button class="button button--danger" type="submit" form="remove-picture" {
                        "Remove the picture"
                    }
                }
            }
        }

        @if has_one {
            form id="remove-picture" method="post" action=(remove_at) hidden {
                (signature(signed))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::pictures::Refused;

    #[test]
    fn a_sentence_is_given_its_capital() {
        assert_eq!(
            capitalised("a password must be at least 10 characters long"),
            "A password must be at least 10 characters long"
        );
        assert_eq!(capitalised(""), "");
    }

    #[test]
    fn every_refusal_says_what_to_do() {
        for refused in [Refused::Empty, Refused::TooLarge, Refused::NotAPicture] {
            assert!(refused.to_string().ends_with('.'), "{refused}");
        }
    }
}

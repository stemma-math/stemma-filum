//! Asking for a group.
//!
//! Groups are requested, not created: somebody asks, a maintainer approves,
//! and the person who asked becomes the owner. An instance can be set to
//! let people create them outright, and that is a policy, not a different
//! mechanism.

use anyhow::Context;
use axum::Json;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use stemma_core::access::GroupRole;
use stemma_core::artefact::{Nodes, Scope};
use stemma_core::id::{InvitationId, Slug};
use stemma_core::repository::Side;

use super::api::Credential;
use super::layout::{Chrome, glyph, page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups::{self, Group, Member};
use crate::store::invitations::{self, Invitation};

/// Changes the exact Lean environment for a group. Only admins and owners
/// may do this; machines hear the resulting library event and rebuild all of
/// the group's work.
pub async fn change_pins(
    State(instance): State<Instance>,
    credential: Credential,
    Path(slug): Path<String>,
    Json(pins): Json<stemma_core::repository::Pins>,
) -> Result<Response, WebError> {
    let slug = Slug::parse(&slug).map_err(|_| WebError::NotFound)?;
    let user = credential.user.id.clone();
    let (group, role) = instance
        .read(move |connection| {
            let group = groups::by_slug(connection, &slug)?.context("group not found")?;
            let role = groups::role_of(connection, &group.id, &user)?.context("not a member")?;
            Ok((group, role))
        })
        .await
        .map_err(|_| WebError::NotFound)?;
    if !role.is_at_least(GroupRole::Admin) {
        return Err(WebError::Forbidden(
            "Only a group admin or owner can change Lean package pins.".to_owned(),
        ));
    }
    validate_pins(&pins)?;

    let id = group.id.clone();
    let name = credential.user.display_name.clone();
    let email = credential.user.email.clone();
    let commit = instance
        .writing_git(move |repositories| {
            crate::git::write_pins(
                repositories,
                &id,
                pins,
                &crate::git::Author {
                    name: &name,
                    email: &email,
                },
            )
        })
        .await?;

    let id = group.id.clone();
    let event = stemma_core::event::Event::Library {
        group: group.slug().as_str().to_owned(),
        side: Side::Shared,
        commit: commit.clone(),
        change: stemma_core::event::LibraryChange::PinsChanged,
    };
    instance
        .write(move |connection| {
            crate::store::events::publish(
                connection,
                crate::store::events::Audience::Group(&id),
                &event,
            )?;
            Ok(())
        })
        .await?;

    Ok(Json(serde_json::json!({ "commit": commit })).into_response())
}

fn validate_pins(pins: &stemma_core::repository::Pins) -> Result<(), WebError> {
    let required = [
        ("Lean", pins.lean.as_deref()),
        ("Mathlib", pins.mathlib.as_deref()),
        ("the Stemma Lean package", pins.stemma.as_deref()),
    ];
    for (name, value) in required {
        if value.is_none_or(|value| value.trim().is_empty() || unsafe_pin(value)) {
            return Err(WebError::BadRequest(format!(
                "{name} needs a safe, exact pin."
            )));
        }
    }
    let mut package_names = std::collections::BTreeSet::from([
        "mathlib".to_owned(),
        "stemma".to_owned(),
        "group_library".to_owned(),
    ]);
    for dependency in &pins.dependencies {
        let valid_name = !dependency.name.is_empty()
            && dependency
                .name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_');
        let valid_git = url::Url::parse(&dependency.git)
            .is_ok_and(|url| matches!(url.scheme(), "https" | "ssh"));
        let valid_subdir = dependency.sub_dir.as_deref().is_none_or(|path| {
            let path = std::path::Path::new(path);
            !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
                && !path.to_string_lossy().contains(['"', '\\'])
        });
        if !valid_name
            || !package_names.insert(dependency.name.clone())
            || !valid_git
            || dependency.rev.trim().is_empty()
            || unsafe_pin(&dependency.rev)
            || !valid_subdir
        {
            return Err(WebError::BadRequest(format!(
                "`{}` is not a safe Lake package pin.",
                dependency.name
            )));
        }
    }
    Ok(())
}

fn unsafe_pin(value: &str) -> bool {
    value.len() > 200
        || value
            .chars()
            .any(|character| character.is_control() || matches!(character, '"' | '\\'))
}

/// What the request form sends.
#[derive(Debug, Deserialize)]
pub struct Asked {
    /// What the group would be called.
    pub name: String,
    /// The short name its remote and its directory would use.
    pub slug: String,
    /// Why it should exist.
    pub reason: String,
    /// The CSRF token [`crate::web::layout::signature`] put in the form.
    pub form_token: String,
}

/// The form for asking.
pub async fn request_form(
    State(instance): State<Instance>,
    signed: SignedIn,
) -> Result<Response, WebError> {
    let policy = instance.policy().await?;
    let chrome = chrome(&instance, &signed).await?;

    if !policy.group_requests_open {
        return Ok(page(&chrome, closed()).into_response());
    }

    Ok(page(&chrome, form(&signed, None, None)).into_response())
}

/// Records a request.
pub async fn request(
    State(instance): State<Instance>,
    signed: SignedIn,
    axum::Form(asked): axum::Form<Asked>,
) -> Result<Response, WebError> {
    signed.check_form(&asked.form_token)?;

    let policy = instance.policy().await?;
    let chrome = chrome(&instance, &signed).await?;

    if !policy.group_requests_open {
        return Ok(page(&chrome, closed()).into_response());
    }

    let refuse = |problem: String, asked: &Asked| {
        (
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            page(&chrome, form(&signed, Some(&problem), Some(asked))),
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
            "Describe the purpose of the research group.".to_owned(),
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
async fn chrome(instance: &Instance, signed: &SignedIn) -> Result<Chrome, WebError> {
    Ok(
        super::layout::around(instance, signed, "Request a research group")
            .await?
            .at("/groups/new"),
    )
}

/// The form itself.
fn form(signed: &SignedIn, problem: Option<&str>, given: Option<&Asked>) -> Markup {
    let value = |pick: fn(&Asked) -> &str| given.map(pick).unwrap_or_default();

    html! {
        div class="sheet--narrow stack" {
            h1 { "Request a research group" }
            p class="quiet" {
                "Submit a group creation request. A platform maintainer will review it."
            }

            @if let Some(problem) = problem {
                p class="problem" { (problem) }
            }

            form method="post" action="/groups/new" {
                (signature(signed))

                div class="field" {
                    label class="field__label" for="name" { "Name" }
                    input class="input" type="text" id="name" name="name" value=(value(|asked| &asked.name))
                          required;
                }

                div class="field" {
                    label class="field__label" for="slug" { "Short name" }
                    input class="input input--mono" type="text" id="slug" name="slug" value=(value(|asked| &asked.slug))
                          required;
                    span class="field__hint" {
                        "Use lowercase letters, numbers and hyphens."
                    }
                }

                div class="field" {
                    label class="field__label" for="reason" { "Purpose" }
                    textarea class="input" id="reason" name="reason" required { (value(|asked| &asked.reason)) }
                }

                button class="button button--primary" type="submit" { "Send the request" }
            }
        }
    }
}

/// What an instance that does not take requests says.
fn closed() -> Markup {
    html! {
        div class="sheet--narrow stack" {
            h1 { "Group requests are closed" }
            p {
                "Contact a platform maintainer to create a research group."
            }
            p { a href="/" { "Back to your home" } }
        }
    }
}

/// What the invitation form sends.
#[derive(Debug, Deserialize)]
pub struct Invite {
    /// Who it is meant for, where that is known.
    #[serde(default)]
    pub email: String,
    /// What they will be able to do in the group.
    pub role: String,
    /// The CSRF token [`crate::web::layout::signature`] put in the form.
    pub form_token: String,
}

/// What a withdrawal sends.
#[derive(Debug, Deserialize)]
pub struct Withdrawal {
    /// The CSRF token [`crate::web::layout::signature`] put in the form.
    pub form_token: String,
}

/// The group's own space.
pub async fn space(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
) -> Result<Response, WebError> {
    let Some(group) = lookup(&instance, &slug).await? else {
        return Err(WebError::NotFound);
    };

    let (role, members, invitations) = read_space(&instance, &group, &signed).await?;

    // Somebody outside the group is told there is nothing here rather than
    // that there is something they may not see: a group's membership is
    // its own business.
    let Some(role) = role else {
        return Err(WebError::NotFound);
    };

    let chrome = super::layout::around(&instance, &signed, group.name.clone())
        .await?
        .at(format!("/g/{}", group.slug()));
    let work = read_work(&instance, &group, &signed).await?;
    let (offers, queue) = read_machines(&instance, &group, &signed).await?;
    let faces = read_faces(&instance, &group, &members).await?;

    Ok(page(
        &chrome,
        space_body(
            &signed,
            &group,
            role,
            &members,
            &faces,
            &invitations,
            &work,
            &offers,
            &queue,
            None,
            None,
        ),
    )
    .into_response())
}

/// Makes an invitation, and shows the link once.
pub async fn invite(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::Form(form): axum::Form<Invite>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let Some(group) = lookup(&instance, &slug).await? else {
        return Err(WebError::NotFound);
    };

    let (role, _, _) = read_space(&instance, &group, &signed).await?;
    let Some(role) = role else {
        return Err(WebError::NotFound);
    };

    if !role.can_invite() {
        return Err(WebError::Forbidden(
            "Only an admin or the owner of this group can invite people.".to_owned(),
        ));
    }

    let offered: GroupRole = form
        .role
        .parse()
        .map_err(|_| WebError::BadRequest("That is not a role in a group.".to_owned()))?;

    // Nobody may hand out more than they hold: an admin cannot make an
    // owner.
    if offered > role {
        return Err(WebError::Forbidden(
            "You cannot invite somebody to more than you hold yourself.".to_owned(),
        ));
    }

    let policy = instance.policy().await?;
    let key = instance.key.clone();
    let id = group.id.clone();
    let by = signed.user.id.clone();
    let email = match form.email.trim() {
        "" => None,
        given => Some(given.to_owned()),
    };
    let days = policy.invitation_ttl_days;

    let (_, secret) = {
        let email = email.clone();

        instance
            .read(move |connection| {
                invitations::create(connection, &key, &id, offered, email.as_deref(), &by, days)
            })
            .await?
    };

    let link = instance
        .settings
        .public_url
        .link(&format!("invite/{secret}"));

    // The link is what the invitation is. Mail carries a copy of it
    // where the instance can send mail, and the page shows it either
    // way, because a link that never arrived is not an invitation.
    let delivered = match &email {
        Some(address) => instance
            .deliver(
                address,
                &format!("{} invites you to {}", signed.user.display_name, group.name),
                &format!(
                    "{} has invited you to join {} on this instance.\n\n\
                         Open this link to accept. It works once, and it expires.\n\n{link}",
                    signed.user.display_name, group.name
                ),
            )
            .await
            .said(address),
        None => None,
    };

    let (_, members, live) = read_space(&instance, &group, &signed).await?;
    let work = read_work(&instance, &group, &signed).await?;
    let (offers, queue) = read_machines(&instance, &group, &signed).await?;
    let faces = read_faces(&instance, &group, &members).await?;

    let chrome = super::layout::around(&instance, &signed, group.name.clone())
        .await?
        .at(format!("/g/{}", group.slug()));

    Ok(page(
        &chrome,
        space_body(
            &signed,
            &group,
            role,
            &members,
            &faces,
            &live,
            &work,
            &offers,
            &queue,
            Some(&link),
            delivered.as_deref(),
        ),
    )
    .into_response())
}

/// Calls an invitation back.
pub async fn withdraw(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((slug, id)): Path<(String, String)>,
    axum::Form(form): axum::Form<Withdrawal>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let Some(group) = lookup(&instance, &slug).await? else {
        return Err(WebError::NotFound);
    };

    let (role, _, _) = read_space(&instance, &group, &signed).await?;

    if !role.is_some_and(GroupRole::can_invite) {
        return Err(WebError::NotFound);
    }

    let invitation: InvitationId = id
        .parse()
        .map_err(|_| WebError::BadRequest("That is not an invitation.".to_owned()))?;
    let by = signed.user.id.clone();
    let group_id = group.id.clone();

    instance
        .read(move |connection| {
            match invitations::by_id(connection, &invitation)? {
                Some(found) if found.group == group_id => {
                    invitations::revoke(connection, &invitation, &by)?;
                }
                _ => {}
            }

            Ok(())
        })
        .await?;

    Ok(Redirect::to(&format!("/g/{slug}")).into_response())
}

/// What the group's details form sends.
#[derive(Debug, Deserialize)]
pub struct Details {
    /// What the group calls itself.
    pub name: String,
    /// What it is for.
    #[serde(default)]
    pub description: String,
    /// The CSRF token [`crate::web::layout::signature`] put in the form.
    pub form_token: String,
}

/// What was just done to the group, as the page's address says it.
#[derive(Debug, Default, Deserialize)]
pub struct Done {
    /// Which change went through.
    pub done: Option<String>,
}

/// The page a group's details are changed on.
pub async fn edit_form(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::extract::Query(done): axum::extract::Query<Done>,
) -> Result<Response, WebError> {
    let group = manageable(&instance, &slug, &signed).await?;
    let said = match done.done.as_deref() {
        Some("details") => Some("The group's details have been saved."),
        Some("picture") => Some("The group's picture has been changed."),
        Some("picture-removed") => Some("The group's picture has been removed."),
        _ => None,
    };

    draw_edit(
        &instance,
        &signed,
        &group,
        axum::http::StatusCode::OK,
        said,
        None,
        None,
    )
    .await
}

/// Saves a group's name and description.
pub async fn edit(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::Form(form): axum::Form<Details>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;
    let group = manageable(&instance, &slug, &signed).await?;

    let name = form.name.trim().to_owned();
    if name.is_empty() {
        return draw_edit(
            &instance,
            &signed,
            &group,
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            None,
            Some("The group needs a name."),
            Some(&form),
        )
        .await;
    }

    let id = group.id.clone();
    let description = form.description.clone();
    instance
        .read(move |connection| {
            groups::rename(connection, &id, &name)?;
            groups::describe(connection, &id, &description)
        })
        .await?;

    Ok(Redirect::to(&format!("/g/{slug}/edit?done=details")).into_response())
}

/// Keeps a new picture for a group.
pub async fn picture(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    upload: axum::extract::Multipart,
) -> Result<Response, WebError> {
    let (token, bytes) = super::profile::read_upload(upload).await?;
    signed.check_form(&token)?;
    let group = manageable(&instance, &slug, &signed).await?;

    let id = group.id.clone();
    let kept = instance
        .read(move |connection| crate::store::pictures::set_group(connection, &id, &bytes))
        .await?;

    match kept {
        Ok(()) => Ok(Redirect::to(&format!("/g/{slug}/edit?done=picture")).into_response()),
        Err(refused) => {
            draw_edit(
                &instance,
                &signed,
                &group,
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                None,
                Some(&refused.to_string()),
                None,
            )
            .await
        }
    }
}

/// Takes a group's picture away.
pub async fn remove_picture(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::Form(form): axum::Form<Withdrawal>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;
    let group = manageable(&instance, &slug, &signed).await?;

    let id = group.id.clone();
    instance
        .read(move |connection| crate::store::pictures::remove_group(connection, &id))
        .await?;

    Ok(Redirect::to(&format!("/g/{slug}/edit?done=picture-removed")).into_response())
}

/// The group, when the reader may manage it.
///
/// Somebody outside it is told there is nothing here, as its page tells
/// them; a member is told why not.
async fn manageable(instance: &Instance, slug: &str, signed: &SignedIn) -> Result<Group, WebError> {
    let Some(group) = lookup(instance, slug).await? else {
        return Err(WebError::NotFound);
    };

    let (role, _, _) = read_space(instance, &group, signed).await?;

    match role {
        None => Err(WebError::NotFound),
        Some(role) if !role.may_manage() => Err(WebError::Forbidden(
            "Only an admin or the owner of this group can change its details.".to_owned(),
        )),
        Some(_) => Ok(group),
    }
}

/// Draws the page a group is changed on.
async fn draw_edit(
    instance: &Instance,
    signed: &SignedIn,
    group: &Group,
    status: axum::http::StatusCode,
    said: Option<&str>,
    problem: Option<&str>,
    given: Option<&Details>,
) -> Result<Response, WebError> {
    let chrome = super::layout::around(instance, signed, format!("Edit {}", group.name))
        .await?
        .at(format!("/g/{}", group.slug()));

    let id = group.id.clone();
    let changed = instance
        .read(move |connection| crate::store::pictures::group_changed(connection, &id))
        .await?;

    Ok((
        status,
        page(
            &chrome,
            edit_body(signed, group, changed.as_deref(), said, problem, given),
        ),
    )
        .into_response())
}

/// The page itself.
fn edit_body(
    signed: &SignedIn,
    group: &Group,
    changed: Option<&str>,
    said: Option<&str>,
    problem: Option<&str>,
    given: Option<&Details>,
) -> Markup {
    let name = given.map_or(group.name.as_str(), |given| given.name.as_str());
    let description = given.map_or(group.description.as_deref().unwrap_or_default(), |given| {
        given.description.as_str()
    });

    html! {
        header class="page-head" {
            nav class="crumbs" aria-label="Breadcrumb" {
                a href="/#groups" { "Groups" }
                span aria-hidden="true" { "/" }
                a href={ "/g/" (group.slug()) } { (group.name) }
                span aria-hidden="true" { "/" }
                span aria-current="page" { "Edit" }
            }
            div class="page-head__row" {
                (super::pictures::badge(&group.id, &group.name, changed, "badge badge--large"))
                div class="page-head__text" {
                    h1 class="page-title" { "Edit the group" }
                    div class="page-meta" {
                        span class="identifier" { (group.id.to_string()) }
                    }
                }
            }
        }

        div class="settings-page" {
            @if let Some(said) = said {
                p class="done" role="status" { (said) }
            }
            @if let Some(problem) = problem {
                p class="problem" role="alert" { (problem) }
            }

            section class="block" aria-labelledby="picture-title" {
                h2 class="block__title" id="picture-title" { "Picture" }
                (super::profile::picture_form(
                    &format!("/g/{}/picture", group.slug()),
                    &format!("/g/{}/picture/remove", group.slug()),
                    signed,
                    changed.is_some(),
                    "It is shown beside the group's name to its members.",
                ))
            }

            section class="block" aria-labelledby="details-title" {
                h2 class="block__title" id="details-title" { "Details" }
                form method="post" action={ "/g/" (group.slug()) "/edit" } {
                    (signature(signed))

                    div class="field" {
                        label class="field__label" for="name" { "Name" }
                        input class="input" type="text" id="name" name="name" value=(name) required;
                    }

                    div class="field" {
                        label class="field__label" for="description" { "Purpose" }
                        textarea class="input" id="description" name="description" rows="3" {
                            (description)
                        }
                        span class="field__hint" { "Shown under the group's name. Leave it empty to show nothing." }
                    }

                    div class="field" {
                        span class="field__label" { "Short name" }
                        span class="identifier" { (group.slug()) }
                        span class="field__hint" {
                            "It cannot be changed: it is in the address of the group's repositories "
                            "on every machine that has them."
                        }
                    }

                    button class="button button--primary" type="submit" { "Save details" }
                }
            }
        }
    }
}

/// The group with this short name.
async fn lookup(instance: &Instance, slug: &str) -> Result<Option<Group>, WebError> {
    let Ok(slug) = Slug::parse(slug) else {
        return Ok(None);
    };

    instance
        .read(move |connection| groups::by_slug(connection, &slug))
        .await
}

/// What the reader may do here, who else is here, and what links are out.
async fn read_space(
    instance: &Instance,
    group: &Group,
    signed: &SignedIn,
) -> Result<(Option<GroupRole>, Vec<Member>, Vec<Invitation>), WebError> {
    let id = group.id.clone();
    let user = signed.user.id.clone();

    instance
        .read(move |connection| {
            let role = groups::role_of(connection, &id, &user)?;
            let members = groups::members(connection, &id)?;
            let live = match role.is_some_and(GroupRole::can_invite) {
                true => invitations::live_for_group(connection, &id)?,
                false => Vec::new(),
            };

            Ok((role, members, live))
        })
        .await
}

/// What has been built for this group, on both sides.
///
/// The shared one is the group's; the private one is the reader's own,
/// and nobody else's is ever read here.
async fn read_work(
    instance: &Instance,
    group: &Group,
    signed: &SignedIn,
) -> Result<Vec<(Side, Nodes)>, WebError> {
    let mut found = Vec::new();

    for side in [Side::Shared, Side::Private] {
        let scope = Scope::of(side, &signed.user.id);
        let mut nodes = super::lines::read(instance, &group.id, &scope)
            .await?
            .unwrap_or_else(|| Nodes::new(group.id.clone(), side, String::new()));

        // A line somebody started a minute ago is in the repository and
        // in no build yet. It belongs in this list from the moment it
        // exists, or the screen that made it appears to have done
        // nothing.
        for digest in super::lines::unbuilt_in(instance, &group.id, side, signed).await? {
            if nodes.line(&digest.name).is_none() {
                nodes.lines.push(digest);
            }
        }

        if !nodes.lines.is_empty() {
            found.push((side, nodes));
        }
    }

    Ok(found)
}

/// The machines this group has, and what they have been asked to do.
///
/// Work on somebody's own repository is listed for them alone, because
/// the repository it is about is one nobody else can clone.
async fn read_machines(
    instance: &Instance,
    group: &Group,
    signed: &SignedIn,
) -> Result<(Vec<super::jobs::Offer>, Vec<crate::store::jobs::Job>), WebError> {
    let offers = super::jobs::offers(instance, &group.id).await?;

    let id = group.id.clone();
    let reader = signed.user.id.clone();
    let queue = instance
        .read(move |connection| crate::store::jobs::for_group(connection, &id, &reader, 20))
        .await?;

    Ok((offers, queue))
}

/// When the group's picture and each member's last changed, for those
/// that have one.
struct Faces {
    /// The group's.
    group: Option<String>,
    /// Each member's, by identifier.
    members: std::collections::HashMap<stemma_core::id::UserId, String>,
}

/// Reads which pictures the group's page shows.
async fn read_faces(
    instance: &Instance,
    group: &Group,
    members: &[Member],
) -> Result<Faces, WebError> {
    let id = group.id.clone();
    let people: Vec<_> = members
        .iter()
        .map(|member| member.user.id.clone())
        .collect();

    instance
        .read(move |connection| {
            let mut members = std::collections::HashMap::new();
            for person in people {
                if let Some(changed) = crate::store::pictures::user_changed(connection, &person)? {
                    members.insert(person, changed);
                }
            }

            Ok(Faces {
                group: crate::store::pictures::group_changed(connection, &id)?,
                members,
            })
        })
        .await
}

/// How a role reads beside a name.
pub fn role_words(role: GroupRole) -> &'static str {
    match role {
        GroupRole::Owner => "Owner",
        GroupRole::Admin => "Admin",
        GroupRole::Member => "Member",
    }
}

/// The group space.
#[allow(clippy::too_many_arguments)]
fn space_body(
    signed: &SignedIn,
    group: &Group,
    role: GroupRole,
    members: &[Member],
    faces: &Faces,
    invitations: &[Invitation],
    work: &[(Side, Nodes)],
    offers: &[super::jobs::Offer],
    queue: &[crate::store::jobs::Job],
    fresh_link: Option<&str>,
    delivered: Option<&str>,
) -> Markup {
    let new_line = format!("/g/{}/lines/new", group.slug());

    html! {
        header class="page-head" {
            nav class="crumbs" aria-label="Breadcrumb" {
                a href="/#groups" { "Groups" }
                span aria-hidden="true" { "/" }
                span aria-current="page" { (group.name) }
            }
            div class="page-head__row" {
                (super::pictures::badge(&group.id, &group.name, faces.group.as_deref(), "badge badge--large"))
                div class="page-head__text" {
                    div class="page-title-row" {
                        h1 class="page-title" { (group.name) }
                        @if role.may_manage() {
                            a class="button button--small" href={ "/g/" (group.slug()) "/edit" } {
                                (glyph("edit")) "Edit"
                            }
                        }
                    }
                    div class="page-meta" {
                        span class="identifier" { (group.id.to_string()) }
                        span class="chip chip--accent" { (role_words(role)) }
                        span {
                            @match members.len() {
                                1 => "1 member",
                                count => { (count) " members" },
                            }
                        }
                    }
                    @if let Some(description) = &group.description {
                        p class="page-subtitle" { (description) }
                    }
                }
                a class="button button--primary button--large" href=(new_line) {
                    (glyph("add")) "New line of work"
                }
            }
        }

        div class="group-layout" {
            div class="group-layout__main" {
                (lines_block(group, work))
                (super::jobs::machine_list(offers))
                (super::jobs::queue(queue))
                @if role.can_invite() {
                    (invitations_block(signed, group, role, invitations, fresh_link, delivered))
                }
            }

            aside class="group-layout__rail" aria-label="About the group" {
                section class="card card--vault vault-card" {
                    div class="vault-card__head" {
                        span class="vault-card__icon" { (glyph("book")) }
                        h2 { "Mathematical base" }
                    }
                    p { "The group vault, shared by every line of work in " (group.name) "." }
                    a class="vault-card__open" href={ "/g/" (group.slug()) "/vault" } {
                        "Open the group vault"
                    }
                }

                section class="card members-card" aria-labelledby="members-title" {
                    div class="card__head" {
                        h2 class="card__title" id="members-title" {
                            "Members" span class="count" { (members.len()) }
                        }
                        @if role.can_invite() {
                            a class="button button--small add-member" href="#invitations" {
                                (glyph("add-person")) "Add"
                            }
                        }
                    }
                    ul class="people" {
                        @for member in members {
                            li class="person" {
                                (super::pictures::face(
                                    &member.user.id,
                                    &member.user.display_name,
                                    faces.members.get(&member.user.id).map(String::as_str),
                                    "",
                                ))
                                span class="person__who" {
                                    span class="person__name" { (member.user.display_name) }
                                    span class="identifier quiet" { (member.user.id.to_string()) }
                                }
                                span class=@if member.role == GroupRole::Owner { "person__role person__role--owner" } @else { "person__role" } {
                                    (role_words(member.role))
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The lines of work, as the last machine to build them left them.
fn lines_block(group: &Group, work: &[(Side, Nodes)]) -> Markup {
    let count: usize = work.iter().map(|(_, nodes)| nodes.lines.len()).sum();

    html! {
        section class="card lines-card" aria-labelledby="lines-title" {
            div class="card__head" {
                h2 class="card__title" id="lines-title" {
                    "Lines of work" span class="count" { (count) }
                }
            }

            @if count == 0 {
                div class="card__empty" {
                    p class="empty" {
                        "No lines of work yet. Start one here, or create one from a connected machine."
                    }
                    a class="button" href={ "/g/" (group.slug()) "/lines/new" } {
                        (glyph("add")) "New line of work"
                    }
                }
            } @else {
                div class="table lines-table" {
                    div class="table__head" aria-hidden="true" {
                        span { "Title" }
                        span { "Summary" }
                        span { "Visibility" }
                        span { "Contents" }
                        span {}
                    }
                    @for (side, nodes) in work {
                        @for line in &nodes.lines {
                            a class="table__row" href={ "/g/" (group.slug()) "/" (side) "/" (line.name) } {
                                span class="lines-table__title" { (line.shown_as()) }
                                span class="lines-table__summary" {
                                    @if let Some(description) = &line.description { (description) }
                                }
                                @match side {
                                    Side::Shared => span class="chip" { (glyph("group")) "Shared" },
                                    Side::Private => span class="chip" { (glyph("lock")) "Private" },
                                }
                                span class="lines-table__contents numeric" {
                                    (line.line.central().len()) " central · "
                                    (line.line.nodes.len()) " in all"
                                }
                                span class="chevron" { (glyph("chevron-right")) }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The invitations out of this group, and the form that makes one.
fn invitations_block(
    signed: &SignedIn,
    group: &Group,
    role: GroupRole,
    invitations: &[Invitation],
    fresh_link: Option<&str>,
    delivered: Option<&str>,
) -> Markup {
    html! {
        section class="block" id="invitations" {
            h2 class="block__title" { "Invitations" }

            @if let Some(link) = fresh_link {
                div class="stack stack--tight" {
                    @match delivered {
                        Some(said) => p { (said) }
                        None => p {
                            "Copy and share this invitation link now. It will not be shown again."
                        }
                    }
                    p class="secret" { (link) }
                }
            }

            @if invitations.is_empty() {
                p class="empty" { "There are no active invitations." }
            } @else {
                ul class="register" {
                    @for invitation in invitations {
                        li {
                            span class="register__name" {
                                @match &invitation.email {
                                    Some(email) => (email),
                                    None => "anybody with the link",
                                }
                            }
                            span class="quiet" { (invitation.role) }
                            span class="register__aside numeric" {
                                "until " (invitation.expires_at)
                            }
                            form method="post"
                                 action={ "/g/" (group.slug()) "/invitations/"
                                          (invitation.id) "/withdraw" } {
                                (signature(signed))
                                button class="button" type="submit" {
                                    "Withdraw"
                                }
                            }
                        }
                    }
                }
            }

            form method="post" action={ "/g/" (group.slug()) "/invitations" } {
                (signature(signed))

                div class="field" {
                    label class="field__label" for="email" { "Email address (optional)" }
                    input class="input" type="email" id="email" name="email";
                    span class="field__hint" {
                        "Used to identify the invitation and send the link when email is configured."
                    }
                }

                div class="field" {
                    label class="field__label" for="role" { "Role" }
                    select class="input" id="role" name="role" {
                        option value="member" { "Member: reads and writes mathematics" }
                        @if role.is_at_least(GroupRole::Admin) {
                            option value="admin" { "Admin: invites members and manages the group" }
                        }
                        @if role.is_at_least(GroupRole::Owner) {
                            option value="owner" { "Owner: can transfer or dissolve the group" }
                        }
                    }
                }

                button class="button button--primary" type="submit" { "Create invitation" }
            }
        }
    }
}

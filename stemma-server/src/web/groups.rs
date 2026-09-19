//! Asking for a group.
//!
//! Groups are requested, not created: somebody asks, a maintainer approves,
//! and the person who asked becomes the owner. An instance can be set to
//! let people create them outright, and that is a policy, not a different
//! mechanism.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use stemma_core::access::GroupRole;
use stemma_core::artefact::{Nodes, Scope};
use stemma_core::id::{InvitationId, Slug};
use stemma_core::repository::Side;

use super::layout::{Chrome, page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups::{self, Group, Member, Request};
use crate::store::invitations::{self, Invitation};

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

/// What the invitation form sends.
#[derive(Debug, Deserialize)]
pub struct Invite {
    /// Who it is meant for, where that is known.
    #[serde(default)]
    pub email: String,
    /// What they will be able to do in the group.
    pub role: String,
    /// What says the form came from a page we served.
    pub form_token: String,
}

/// What a withdrawal sends.
#[derive(Debug, Deserialize)]
pub struct Withdrawal {
    /// What says the form came from a page we served.
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

    let policy = instance.policy().await?;
    let chrome = Chrome::anonymous(
        group.name.clone(),
        super::front::instance_name(&instance, &policy),
    )
    .signed_in(signed.user.clone(), signed.maintainer);
    let work = read_work(&instance, &group, &signed).await?;

    Ok(page(
        &chrome,
        space_body(&signed, &group, role, &members, &invitations, &work, None),
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

    let (_, secret) = instance
        .read(move |connection| {
            invitations::create(connection, &key, &id, offered, email.as_deref(), &by, days)
        })
        .await?;

    let link = instance
        .settings
        .public_url
        .link(&format!("invite/{secret}"));
    let (_, members, live) = read_space(&instance, &group, &signed).await?;
    let work = read_work(&instance, &group, &signed).await?;

    let chrome = Chrome::anonymous(
        group.name.clone(),
        super::front::instance_name(&instance, &policy),
    )
    .signed_in(signed.user.clone(), signed.maintainer);

    Ok(page(
        &chrome,
        space_body(&signed, &group, role, &members, &live, &work, Some(&link)),
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

        if let Some(nodes) = super::lines::read(instance, &group.id, &scope).await? {
            found.push((side, nodes));
        }
    }

    Ok(found)
}

/// The group space.
fn space_body(
    signed: &SignedIn,
    group: &Group,
    role: GroupRole,
    members: &[Member],
    invitations: &[Invitation],
    work: &[(Side, Nodes)],
    fresh_link: Option<&str>,
) -> Markup {
    html! {
        div class="stack" {
            h1 { (group.name) }
            p class="quiet identifier" { (group.id.to_string()) }
        }

        (lines_block(group, work))

        div class="block" {
            p class="label" { "Members · " (members.len()) }
            ul class="register" {
                @for member in members {
                    li {
                        span class="register__name" { (member.user.display_name) }
                        span class="quiet identifier" { (member.user.id.to_string()) }
                        span class="register__aside" { (member.role) }
                    }
                }
            }
        }

        @if role.can_invite() {
            (invitations_block(signed, group, role, invitations, fresh_link))
        }
    }
}

/// The lines of work, as the last machine to build them left them.
fn lines_block(group: &Group, work: &[(Side, Nodes)]) -> Markup {
    let nothing = work.iter().all(|(_, nodes)| nodes.lines.is_empty());

    html! {
        div class="block" {
            p class="label" { "Lines of work" }

            @if nothing {
                p class="empty" {
                    "Nothing has been built for this group yet. A line appears here once "
                    "somebody writes it on their own machine and sends it up: this "
                    "instance compiles nothing itself."
                }
            }

            @for (side, nodes) in work {
                @if !nodes.lines.is_empty() {
                    ul class="register" {
                        @for line in &nodes.lines {
                            li {
                                span class="register__name" {
                                    a href={ "/g/" (group.slug()) "/" (side) "/" (line.name) } {
                                        (line.shown_as())
                                    }
                                }
                                span class="quiet" {
                                    @match side {
                                        Side::Shared => "shared",
                                        // A ruled margin means "not
                                        // shared"; in a list the word
                                        // does the same work.
                                        Side::Private => "yours alone",
                                    }
                                }
                                span class="register__aside numeric" {
                                    (line.line.central().len()) " central · "
                                    (line.line.nodes.len()) " in all"
                                }
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
) -> Markup {
    html! {
        div class="block" {
            p class="label" { "Invitations" }

            @if let Some(link) = fresh_link {
                div class="stack stack--tight" {
                    p { "Send this link to the person you are inviting. It is shown once." }
                    p class="secret" { (link) }
                }
            }

            @if invitations.is_empty() {
                p class="empty" { "No invitation is out at the moment." }
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
                                button class="button button--quiet" type="submit" {
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
                    label for="email" { "Their email address, if you know it" }
                    input type="email" id="email" name="email";
                    span class="field__hint" {
                        "Only used to label the invitation, and to send the link where "
                        "this instance can send mail."
                    }
                }

                div class="field" {
                    label for="role" { "What they will be able to do" }
                    select id="role" name="role" {
                        option value="member" { "Member — reads and writes the mathematics" }
                        @if role.is_at_least(GroupRole::Admin) {
                            option value="admin" { "Admin — also invites and manages the group" }
                        }
                        @if role.is_at_least(GroupRole::Owner) {
                            option value="owner" { "Owner — also transfers or dissolves it" }
                        }
                    }
                }

                button class="button" type="submit" { "Make an invitation" }
            }
        }
    }
}

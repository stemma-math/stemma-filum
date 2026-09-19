//! Machines: registering one, and looking at one.
//!
//! A machine registers once, against the credential it already holds, and
//! then serves whichever groups its owner turns it on for. Registering is
//! a second act after signing in, because a laptop somebody only writes
//! on should not be offered work.
//!
//! What a machine can do is what it reported. Nothing on these pages sets
//! it, and an agent a machine does not have is shown named and greyed
//! rather than dropped: its absence is something its owner can act on.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Json, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use serde_json::json;
use stemma_core::id::{GroupId, JobId, MachineId, Slug};
use stemma_core::machine::{Detected, Presence};

use super::api::Credential;
use super::layout::{Chrome, page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups::{self, Group};
use crate::store::jobs;
use crate::store::machines::{self, Machine};

/// How many of a machine's jobs its panel shows.
const RECENT: u32 = 20;

/// What a machine says when it offers to work.
#[derive(Debug, Default, Deserialize)]
pub struct Registering {
    /// What it calls itself, for its owner to recognise in a list.
    #[serde(default)]
    pub label: String,
    /// What it found when it looked for its agents.
    #[serde(default)]
    pub agents: Vec<Detected>,
    /// The groups it is to serve. Left out, whatever it served stands.
    #[serde(default)]
    pub groups: Option<Vec<String>>,
}

/// What a machine says to stay awake.
#[derive(Debug, Default, Deserialize)]
pub struct Beating {
    /// What it found this time round.
    #[serde(default)]
    pub agents: Vec<Detected>,
}

/// Registers the machine carrying this credential.
pub async fn register(
    State(instance): State<Instance>,
    credential: Credential,
    body: Option<Json<Registering>>,
) -> Result<Response, WebError> {
    let Json(said) = body.unwrap_or_default();

    let owner = credential.user.id.clone();
    let token_id = credential.token_id.clone();
    let label = match said.label.trim() {
        "" => "an unnamed machine".to_owned(),
        given => given.to_owned(),
    };
    let wanted = said.groups.clone();
    let found = said.agents.clone();

    let machine = instance
        .write(move |connection| {
            let machine = machines::register(connection, &owner, &token_id, &label)?;
            machines::report(connection, &machine.id, &found)?;

            if let Some(wanted) = wanted {
                serve_exactly(connection, &machine, &owner, &wanted)?;
            }

            Ok(machine)
        })
        .await?;

    Ok(described(&instance, &machine).await?.into_response())
}

/// Turns a machine on for exactly these groups and off for the rest.
///
/// What the machine asks for is taken as the whole answer, because the
/// thing asking is the thing its owner configured.
fn serve_exactly(
    connection: &rusqlite::Connection,
    machine: &Machine,
    owner: &stemma_core::id::UserId,
    wanted: &[String],
) -> anyhow::Result<()> {
    let mine = groups::for_user(connection, owner)?;

    for membership in &mine {
        let asked = wanted
            .iter()
            .any(|slug| slug.trim() == membership.group.slug().as_str());

        match asked {
            true => machines::enable(connection, &machine.id, &membership.group.id)?,
            false => machines::disable(connection, &machine.id, &membership.group.id)?,
        }
    }

    Ok(())
}

/// Records that a machine is awake, and what it can do right now.
pub async fn beat(
    State(instance): State<Instance>,
    credential: Credential,
    body: Option<Json<Beating>>,
) -> Result<Response, WebError> {
    let Json(said) = body.unwrap_or_default();
    let machine = registered(&instance, &credential).await?;

    let id = machine.id;
    let found = said.agents.clone();

    instance
        .write(move |connection| {
            machines::beat(connection, &id)?;

            if !found.is_empty() {
                machines::report(connection, &id, &found)?;
            }

            Ok(())
        })
        .await?;

    let machine = registered(&instance, &credential).await?;

    Ok(described(&instance, &machine).await?.into_response())
}

/// What the instance thinks of the machine asking.
pub async fn mine(
    State(instance): State<Instance>,
    credential: Credential,
) -> Result<Response, WebError> {
    let machine = registered(&instance, &credential).await?;

    Ok(described(&instance, &machine).await?.into_response())
}

/// The machine behind this credential, if it has registered.
pub async fn registered(instance: &Instance, credential: &Credential) -> Result<Machine, WebError> {
    let token_id = credential.token_id.clone();

    instance
        .read(move |connection| machines::by_token(connection, &token_id))
        .await?
        .filter(Machine::is_live)
        .ok_or_else(|| {
            WebError::Forbidden(
                "This machine is not registered here. Run `stemma node init` on it.".to_owned(),
            )
        })
}

/// One machine, as it is handed back to itself.
async fn described(
    instance: &Instance,
    machine: &Machine,
) -> Result<Json<serde_json::Value>, WebError> {
    let id = machine.id;

    let (groups, agents) = instance
        .read(move |connection| {
            Ok((
                machines::groups(connection, &id)?,
                machines::agents(connection, &id)?,
            ))
        })
        .await?;

    Ok(Json(json!({
        "machine": machine.id.to_string(),
        "label": machine.label,
        "presence": machine.presence,
        "groups": groups
            .iter()
            .map(|group| group.slug().as_str())
            .collect::<Vec<_>>(),
        "agents": agents,
    })))
}

/// What a form on the machine's panel sends.
#[derive(Debug, Deserialize)]
pub struct Change {
    /// The group it is about, where it is about one.
    #[serde(default)]
    pub group: String,
    /// Whether the machine is to serve it.
    #[serde(default)]
    pub serve: String,
    /// What says the form came from a page we served.
    pub form_token: String,
}

/// One machine's panel.
pub async fn panel(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let machine = own(&instance, &signed, &id).await?;
    let owner = signed.user.id.clone();
    let of = machine.id;

    let (agents, serving, memberships, ran) = instance
        .read(move |connection| {
            Ok((
                machines::agents(connection, &of)?,
                machines::groups(connection, &of)?,
                groups::for_user(connection, &owner)?,
                jobs::for_machine(connection, &of, RECENT)?,
            ))
        })
        .await?;

    let policy = instance.policy().await?;
    let chrome = Chrome::anonymous(
        machine.label.clone(),
        super::front::instance_name(&instance, &policy),
    )
    .at("/")
    .signed_in(signed.user.clone(), signed.maintainer);

    let groups: Vec<(Group, bool)> = memberships
        .into_iter()
        .map(|membership| {
            let serving = serving.contains(&membership.group.id);
            (membership.group, serving)
        })
        .collect();

    Ok(page(&chrome, body(&signed, &machine, &agents, &groups, &ran)).into_response())
}

/// Turns a machine on or off for one group.
pub async fn serve(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(id): Path<String>,
    axum::Form(change): axum::Form<Change>,
) -> Result<Response, WebError> {
    signed.check_form(&change.form_token)?;

    let machine = own(&instance, &signed, &id).await?;
    let Ok(slug) = Slug::parse(change.group.trim()) else {
        return Err(WebError::BadRequest("That is not a group.".to_owned()));
    };

    let group = GroupId::new(slug);
    let owner = signed.user.id.clone();
    let of = machine.id;
    let wanted = change.serve == "yes";

    instance
        .write(move |connection| {
            // A machine can only be turned on for a group its owner is
            // in, and that is checked here rather than trusted from the
            // form it came in on.
            if groups::role_of(connection, &group, &owner)?.is_none() {
                return Ok(());
            }

            match wanted {
                true => machines::enable(connection, &of, &group),
                false => machines::disable(connection, &of, &group),
            }
        })
        .await?;

    Ok(Redirect::to(&format!("/machines/{}", machine.id)).into_response())
}

/// Takes a machine back.
pub async fn retire(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(id): Path<String>,
    axum::Form(change): axum::Form<Change>,
) -> Result<Response, WebError> {
    signed.check_form(&change.form_token)?;

    let machine = own(&instance, &signed, &id).await?;
    let of = machine.id;

    instance
        .write(move |connection| machines::retire(connection, &of))
        .await?;

    Ok(Redirect::to("/").into_response())
}

/// Records that the owner has dealt with what stopped a job.
pub async fn clear(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((id, job)): Path<(String, String)>,
    axum::Form(change): axum::Form<Change>,
) -> Result<Response, WebError> {
    signed.check_form(&change.form_token)?;

    let machine = own(&instance, &signed, &id).await?;
    let Ok(job) = job.parse::<JobId>() else {
        return Err(WebError::NotFound);
    };
    let by = signed.user.id.clone();

    instance
        .write(move |connection| jobs::clear(connection, &job, &by))
        .await?;

    Ok(Redirect::to(&format!("/machines/{}", machine.id)).into_response())
}

/// The reader's own machine with this identifier.
///
/// Somebody else's is not theirs to look at, and is answered as nothing
/// rather than as something they may not have: which machines a person
/// has is their business.
async fn own(instance: &Instance, signed: &SignedIn, id: &str) -> Result<Machine, WebError> {
    let Ok(id) = id.parse::<MachineId>() else {
        return Err(WebError::NotFound);
    };

    let owner = signed.user.id.clone();
    let found = instance
        .read(move |connection| machines::by_id(connection, &id))
        .await?;

    found
        .filter(|machine| machine.owner == owner)
        .ok_or(WebError::NotFound)
}

/// The panel itself.
fn body(
    signed: &SignedIn,
    machine: &Machine,
    agents: &[Detected],
    groups: &[(Group, bool)],
    ran: &[jobs::Job],
) -> Markup {
    let owed = ran
        .iter()
        .filter(|job| job.owes_its_owner_a_decision())
        .count();

    html! {
        div class="stack" {
            h1 { (machine.label) }
            p class="quiet identifier" { (machine.id.to_string()) }
            p class="quiet" {
                (machine.presence)
                @if let Some(seen) = &machine.last_seen_at {
                    " · last heard from " (seen)
                }
            }
        }

        div class="block" {
            p class="label" { "Agents" }
            p class="quiet" {
                "What this machine found when it looked. Stemma holds no key for any "
                "of them: they are signed in by you, on this machine."
            }
            (agent_list(agents))
        }

        div class="block" {
            p class="label" { "Groups this machine works for" }
            @if groups.is_empty() {
                p class="empty" { "You are not in any group yet." }
            } @else {
                ul class="register" {
                    @for (group, serving) in groups {
                        li {
                            span class="register__name" { (group.name) }
                            span class="quiet identifier" { (group.id.to_string()) }
                            span class="register__aside" {
                                @match serving {
                                    true => "serving",
                                    false => "not serving",
                                }
                            }
                            form method="post"
                                 action={ "/machines/" (machine.id) "/serve" } {
                                (signature(signed))
                                input type="hidden" name="group" value=(group.slug());
                                input type="hidden" name="serve"
                                      value=@if *serving { "no" } @else { "yes" };
                                button class="button button--quiet" type="submit" {
                                    @match serving {
                                        true => "Stop",
                                        false => "Serve",
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        (job_list(signed, machine, ran, owed))

        div class="block" {
            p class="label" { "Take this machine back" }
            p class="quiet" {
                "Its credential stops working and it is offered no more work. You stay "
                "signed in everywhere else, and what it has already done keeps its name."
            }
            form method="post" action={ "/machines/" (machine.id) "/retire" } {
                (signature(signed))
                button class="button button--quiet" type="submit" { "Take it back" }
            }
        }
    }
}

/// The agents, with the ones that cannot be used named and greyed.
pub fn agent_list(agents: &[Detected]) -> Markup {
    html! {
        ul class="register" {
            @for found in agents {
                @let known = stemma_core::machine::known(found.agent.as_str());

                li class=@if found.availability.is_usable() { "" } @else { "quiet" } {
                    span class="register__name" {
                        @match known {
                            Some(agent) => (agent.called),
                            None => (found.agent.as_str()),
                        }
                    }
                    span class="quiet identifier" { (found.agent.as_str()) }
                    span class="register__aside identifier" {
                        @match (&found.version, found.availability.reason()) {
                            (_, Some(reason)) => (reason),
                            (Some(version), None) => (version),
                            (None, None) => "there",
                        }
                    }
                }
            }
        }
    }
}

/// What this machine has been given, and what it made of it.
fn job_list(signed: &SignedIn, machine: &Machine, ran: &[jobs::Job], owed: usize) -> Markup {
    html! {
        div class="block" {
            p class="label" {
                "Jobs"
                @if owed > 0 {
                    // The one count in the queue: these are waiting for
                    // this machine's owner to fix something.
                    " · " span class="count" { (owed) " to deal with" }
                }
            }

            @if ran.is_empty() {
                p class="empty" { "Nothing has been asked of this machine yet." }
            } @else {
                ul class="register" {
                    @for job in ran {
                        li {
                            span class="register__name" { (job.task.asked_for()) }
                            span class="quiet identifier" {
                                (job.group.slug()) "/" (job.side) "/" (job.line)
                            }
                            span class="register__aside" { (super::jobs::said(job)) }

                            @if job.owes_its_owner_a_decision() {
                                form method="post"
                                     action={ "/machines/" (machine.id) "/jobs/"
                                              (job.id) "/clear" } {
                                    (signature(signed))
                                    button class="button button--quiet" type="submit" {
                                        "I have dealt with it"
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

/// How somebody's machines read on their own home page.
pub fn summary(machines: &[(Machine, usize)]) -> Markup {
    html! {
        div class="block" {
            p class="label" { "Your machines" }

            @if machines.is_empty() {
                p class="empty" {
                    "No machine of yours runs work here yet. Sign one in with "
                    "`stemma login` and offer it with `stemma node init`."
                }
            } @else {
                ul class="register" {
                    @for (machine, owed) in machines {
                        li {
                            span class="register__name" {
                                a href={ "/machines/" (machine.id) } { (machine.label) }
                            }
                            // A sleeping machine is an ordinary state and
                            // not a fault, so it is the word and nothing
                            // else.
                            span class="quiet" { (machine.presence) }
                            span class="register__aside" {
                                @if *owed > 0 {
                                    span class="count" { (owed) " to deal with" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Somebody's machines, each with what it is waiting for them to fix.
pub async fn owned_by(
    instance: &Instance,
    user: &stemma_core::id::UserId,
) -> Result<Vec<(Machine, usize)>, WebError> {
    let user = user.clone();

    instance
        .read(move |connection| {
            let mine = machines::for_user(connection, &user)?;
            let mut with_counts = Vec::new();

            for machine in mine {
                let owed = jobs::for_machine(connection, &machine.id, RECENT)?
                    .iter()
                    .filter(|job| job.owes_its_owner_a_decision())
                    .count();

                with_counts.push((machine, owed));
            }

            Ok(with_counts)
        })
        .await
}

/// Whether any of these machines is there to take work right now.
pub fn any_awake(machines: &[Machine]) -> bool {
    machines
        .iter()
        .any(|machine| machine.presence == Presence::Awake)
}

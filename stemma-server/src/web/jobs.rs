//! The queue, from both ends.
//!
//! Machines ask for work at `/api/jobs`; people launch it from beside the
//! mathematics it is about. Both ends meet in one check: whoever is
//! asking must be in the group, the repository must be one they can
//! reach, and the machine named must be one that serves it.
//!
//! Nothing here interprets an answer. A job comes back with a typed
//! outcome and one line, and that is all the group is ever shown; the
//! output itself never leaves the machine that produced it.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use serde_json::json;
use stemma_core::id::{GroupId, JobId, MachineId, Slug, UserId};
use stemma_core::job::{Choice, JobState, Outcome, Task};
use stemma_core::line::Name;
use stemma_core::machine::{Detected, Presence};
use stemma_core::proposal::{self, Completion};
use stemma_core::repository;

#[derive(Deserialize)]
#[serde(untagged)]
/// A completion report, including the phase-five form without a proposal.
pub enum Report {
    /// Current atomic report.
    Complete(Completion),
    /// Compatible report from a client that predates proposals.
    Outcome(Outcome),
}

impl Report {
    fn complete(self) -> Completion {
        match self {
            Self::Complete(completion) => completion,
            Self::Outcome(outcome) => Completion {
                outcome,
                proposal: None,
            },
        }
    }
}
use stemma_core::repository::Side;

use super::api::Credential;
use super::layout::signature;
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups;
use crate::store::jobs::{self, Job, Launch};
use crate::store::machines::{self, Machine};

/// How many jobs a listing shows.
const RECENT: u32 = 20;

/// What a machine that serves a group can be asked to do.
#[derive(Debug, Clone)]
pub struct Offer {
    /// The machine.
    pub machine: Machine,
    /// What it found when it looked for its agents.
    pub agents: Vec<Detected>,
}

/// The machines serving a group, with what each of them can run.
pub async fn offers(instance: &Instance, group: &GroupId) -> Result<Vec<Offer>, WebError> {
    let group = group.clone();

    instance
        .read(move |connection| {
            let serving = machines::for_group(connection, &group)?;
            let mut offers = Vec::new();

            for machine in serving {
                let agents = machines::agents(connection, &machine.id)?;
                offers.push(Offer { machine, agents });
            }

            Ok(offers)
        })
        .await
}

/// Lists the deliberate machine-and-agent choices for a group client.
pub async fn machine_choices(
    State(instance): State<Instance>,
    credential: Credential,
    Path(group): Path<String>,
) -> Result<Response, WebError> {
    let slug = Slug::parse(&group).map_err(|_| WebError::NotFound)?;
    let group = GroupId::new(slug);
    let reader = credential.user.id;
    let admitted = instance
        .read({
            let group = group.clone();
            move |c| groups::role_of(c, &group, &reader)
        })
        .await?;
    if admitted.is_none() {
        return Err(WebError::NotFound);
    }
    let offers = offers(&instance, &group).await?;
    Ok(
        Json(json!({ "machines": offers.into_iter().map(|offer| json!({
        "machine": offer.machine.id.to_string(), "label": offer.machine.label,
        "presence": offer.machine.presence.as_str(), "agents": offer.agents
    })).collect::<Vec<_>>() }))
        .into_response(),
    )
}

/// Hands the machine asking whatever it may have next.
pub async fn take(
    State(instance): State<Instance>,
    credential: Credential,
) -> Result<Response, WebError> {
    let machine = super::machines::registered(&instance, &credential).await?;
    let id = machine.id;

    let found = instance
        .write(move |connection| {
            machines::beat(connection, &id)?;

            let ready: Vec<Slug> = machines::agents(connection, &id)?
                .into_iter()
                .filter(|agent| agent.availability.is_usable())
                .map(|agent| agent.agent)
                .collect();

            jobs::take_next(connection, &id, &ready)
        })
        .await?;

    let Some(job) = found else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };

    Ok(Json(handed_over(&job)).into_response())
}

/// One job, as the machine that took it is told about it.
fn handed_over(job: &Job) -> serde_json::Value {
    json!({
        "job": job.id.to_string(),
        "group": job.group.slug().as_str(),
        "side": job.side.as_str(),
        "line": job.line.as_str(),
        "commit": job.commit,
        "against": job.against,
        "task": job.task.to_string(),
        "agent": job.agent.as_str(),
        "launched_by": job.launched_by.to_string(),
    })
}

/// Records how a job ended.
pub async fn finish(
    State(instance): State<Instance>,
    credential: Credential,
    Path(id): Path<String>,
    Json(report): Json<Report>,
) -> Result<Response, WebError> {
    let completion = report.complete();
    let machine = super::machines::registered(&instance, &credential).await?;
    let Ok(id) = id.parse::<JobId>() else {
        return Err(WebError::NotFound);
    };

    let found = instance
        .read(move |c| jobs::by_id(c, &id))
        .await?
        .ok_or(WebError::NotFound)?;
    if found.taken_by.as_ref() != Some(&machine.id) || found.state != JobState::Running {
        return Err(WebError::BadRequest(
            "This machine is not running that job.".to_owned(),
        ));
    }
    if let Some(offer) = &completion.proposal {
        let repository = match found.side {
            Side::Shared => instance.repositories().shared(&found.group),
            Side::Private => instance
                .repositories()
                .personal(&found.group, &found.launched_by),
        };
        let inside = format!("{}/{}/{}", repository::LINES, found.line, repository::ENTRY);
        let commit = found.commit.clone();
        let base = instance
            .blocking(move || crate::git::show(&repository, &commit, &inside))
            .await?;
        proposal::preserves_central_statements(&base, &offer.source)
            .map_err(|error| WebError::BadRequest(error.to_string()))?;
    }

    let of = machine.id;
    let recorded = instance
        .write(move |connection| {
            let transaction = connection.transaction()?;
            let recorded = jobs::finish(&transaction, &id, &of, &completion.outcome)?;
            if recorded && let Some(offer) = &completion.proposal {
                crate::store::proposals::create(&transaction, &found, offer)?;
            }
            transaction.commit()?;
            Ok(recorded)
        })
        .await?;

    match recorded {
        true => Ok(Json(json!({ "recorded": true })).into_response()),
        // A job it does not hold is not this machine's to end, and
        // saying so is better than pretending the report landed.
        false => Err(WebError::BadRequest(
            "This machine is not running that job.".to_owned(),
        )),
    }
}

/// What a program on a machine sends to launch a job.
#[derive(Debug, Deserialize)]
pub struct Asking {
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
    /// The commit to work against.
    pub commit: String,
    /// The other version, for a job that brings two of them together.
    #[serde(default)]
    pub against: Option<String>,
    /// What is being asked.
    pub task: String,
    /// Which agent.
    pub agent: String,
    /// Which machine, where one was chosen.
    #[serde(default)]
    pub machine: Option<String>,
}

/// Launches a job from a machine.
pub async fn start(
    State(instance): State<Instance>,
    credential: Credential,
    Json(asking): Json<Asking>,
) -> Result<Response, WebError> {
    let job = admit(&instance, &credential.user.id, &asking).await?;

    Ok(Json(json!({
        "job": job.id.to_string(),
        "state": job.state.as_str(),
    }))
    .into_response())
}

/// What one person has in the queue.
pub async fn listing(
    State(instance): State<Instance>,
    credential: Credential,
) -> Result<Response, WebError> {
    let user = credential.user.id.clone();

    let mine = instance
        .read(move |connection| {
            let mut found = Vec::new();

            for machine in machines::for_user(connection, &user)? {
                found.extend(jobs::for_machine(connection, &machine.id, RECENT)?);
            }

            found.sort_by(|one, other| other.launched_at.cmp(&one.launched_at));
            found.dedup_by(|one, other| one.id == other.id);

            Ok(found)
        })
        .await?;

    let listed: Vec<_> = mine
        .iter()
        .map(|job| {
            let mut described = handed_over(job);
            described["state"] = json!(job.state.as_str());
            described["said"] = json!(said(job));
            described
        })
        .collect();

    Ok(Json(json!({ "jobs": listed })).into_response())
}

/// Checks what was asked and puts it in the queue.
///
/// Everything is checked here and nothing is taken from the form: which
/// group, which repository, which agent and which machine are all
/// answerable from what the instance already knows.
async fn admit(instance: &Instance, by: &UserId, asking: &Asking) -> Result<Job, WebError> {
    let refuse = |what: &str| WebError::BadRequest(what.to_owned());

    let slug = Slug::parse(asking.group.trim()).map_err(|_| WebError::NotFound)?;
    let side: Side = asking
        .side
        .parse()
        .map_err(|_| refuse("That is not one of a group's repositories."))?;
    let line = Name::parse(asking.line.trim()).map_err(|_| WebError::NotFound)?;
    let task: Task = asking
        .task
        .parse()
        .map_err(|_| refuse("That is not something a job can ask for."))?;

    let agent = Slug::parse(asking.agent.trim())
        .ok()
        .filter(|agent| stemma_core::machine::known(agent.as_str()).is_some())
        .ok_or_else(|| refuse("This server cannot run that agent."))?;

    let chosen = match asking
        .machine
        .as_deref()
        .map(str::trim)
        .filter(|given| !given.is_empty())
    {
        Some(given) => Some(
            given
                .parse::<MachineId>()
                .map_err(|_| refuse("That is not a machine."))?,
        ),
        None => None,
    };

    if asking.commit.trim().is_empty() {
        return Err(refuse("A job has to say which commit it is about."));
    }

    let group = GroupId::new(slug);
    let asker = by.clone();
    let commit = asking.commit.trim().to_owned();
    let against = asking
        .against
        .as_deref()
        .map(str::trim)
        .filter(|commit| !commit.is_empty())
        .map(str::to_owned);

    if task == Task::Reconcile && against.is_none() {
        return Err(refuse("A merge job needs the other version's commit."));
    }

    instance
        .write(move |connection| {
            // Somebody outside the group is told there is nothing here,
            // as everywhere else: what a group is working on is its own
            // business.
            if groups::role_of(connection, &group, &asker)?.is_none() {
                anyhow::bail!("nothing here");
            }

            if let Some(machine) = &chosen {
                let serves = machines::for_group(connection, &group)?
                    .iter()
                    .any(|serving| &serving.id == machine);

                if !serves {
                    anyhow::bail!("that machine does not work for this group");
                }

                // A private line exists only on its author's machines,
                // so choosing anybody else's would be choosing a machine
                // that cannot reach the work.
                if side == Side::Private {
                    let owned = machines::by_id(connection, machine)?
                        .is_some_and(|found| found.owner == asker);

                    if !owned {
                        anyhow::bail!("only your own machines can work on your own repository");
                    }
                }
            }

            jobs::launch(
                connection,
                &Launch {
                    group: group.clone(),
                    side,
                    line: line.clone(),
                    commit: commit.clone(),
                    against: against.clone(),
                    task: task.clone(),
                    agent: agent.clone(),
                    choice: match chosen {
                        Some(machine) => Choice::Machine { machine },
                        None => Choice::Anyone,
                    },
                    by: asker.clone(),
                },
            )
        })
        .await
        .map_err(|error| match error {
            WebError::Internal(reason) if reason.to_string() == "nothing here" => {
                WebError::NotFound
            }
            WebError::Internal(reason) => WebError::BadRequest(capitalised(&reason.to_string())),
            other => other,
        })
}

/// A reason, as a sentence.
fn capitalised(reason: &str) -> String {
    let mut characters = reason.chars();

    match characters.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), characters.as_str()),
        None => reason.to_owned(),
    }
}

/// What the launcher beside a line sends.
#[derive(Debug, Deserialize)]
pub struct Launched {
    /// The machine and agent chosen, as one answer.
    pub on: String,
    /// What is being asked.
    pub task: String,
    /// The commit the reader was looking at.
    pub commit: String,
    /// The CSRF token [`crate::web::layout::signature`] put in the form.
    pub form_token: String,
}

/// Launches a job from the line it is about.
pub async fn launch(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((group, side, line)): Path<(String, String, String)>,
    axum::Form(form): axum::Form<Launched>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let (machine, agent) = form
        .on
        .split_once('|')
        .ok_or_else(|| WebError::BadRequest("Choose a machine and an agent.".to_owned()))?;

    admit(
        &instance,
        &signed.user.id,
        &Asking {
            group: group.clone(),
            side: side.clone(),
            line: line.clone(),
            commit: form.commit.clone(),
            against: None,
            task: form.task.clone(),
            agent: agent.to_owned(),
            machine: match machine {
                "anyone" => None,
                given => Some(given.to_owned()),
            },
        },
    )
    .await?;

    Ok(Redirect::to(&format!("/g/{group}/{side}/{line}")).into_response())
}

/// One line about where a job has got to.
pub fn said(job: &Job) -> String {
    match (&job.state, &job.outcome) {
        (JobState::Done, Some(outcome)) => outcome.said().to_owned(),
        (JobState::Done, None) => "it ended".to_owned(),
        (JobState::Running, _) => "running".to_owned(),
        (JobState::Withdrawn, _) => "called back".to_owned(),
        // Waiting is not a warning and carries no badge. A machine that
        // is asleep will take it when it wakes, and that is the whole
        // point of having chosen it.
        (JobState::Queued, _) => match job.choice.is_deliberate() {
            true => "waiting for the machine it was given to".to_owned(),
            false => "waiting for a machine".to_owned(),
        },
    }
}

/// The group's queue.
pub fn queue(jobs: &[Job]) -> Markup {
    html! {
        div class="block" {
            p class="label" { "Jobs" }

            @if jobs.is_empty() {
                p class="empty" {
                    "No jobs have been launched for this group."
                }
            } @else {
                ul class="register" {
                    @for job in jobs {
                        li {
                            span class="register__name" { (job.task.asked_for()) }
                            span class="quiet identifier" {
                                (job.line) " · " (job.agent.as_str())
                            }
                            span class="register__aside" { (said(job)) }
                        }
                    }
                }
            }
        }
    }
}

/// The machines a group has, and what each of them can run.
pub fn machine_list(offers: &[Offer]) -> Markup {
    html! {
        div class="block" {
            p class="label" { "Machines" }

            @if offers.is_empty() {
                p class="empty" {
                    "No machines are available for this group. Run " code { "stemma node init" }
                    " on a connected machine to make it available."
                }
            } @else {
                ul class="register" {
                    @for offer in offers {
                        li {
                            span class="register__name" { (offer.machine.label) }
                            span class="quiet" { (offer.machine.presence) }
                            span class="register__aside identifier" {
                                @let ready: Vec<&str> = offer
                                    .agents
                                    .iter()
                                    .filter(|agent| agent.availability.is_usable())
                                    .map(|agent| agent.agent.as_str())
                                    .collect();

                                @match ready.is_empty() {
                                    true => "no agent on it",
                                    false => (ready.join(" · ")),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The launcher, anchored to the thing the work will land on.
///
/// At rest it is one line of grey grotesk: something that starts work
/// must never be the loudest thing on a page of mathematics. Opened, it
/// is the apparatus again — machines as words with their state, the
/// agents each one actually has nested under it, and the combinations
/// that cannot be chosen greyed with their reason rather than hidden.
pub fn launcher(
    signed: &SignedIn,
    group: &Slug,
    side: Side,
    line: &Name,
    commit: &str,
    task: &Task,
    offers: &[Offer],
) -> Markup {
    html! {
        details class="launcher" {
            summary { (task.asked_for()) }

            div class="launcher__panel" {
                @if offers.is_empty() {
                    p class="empty" {
                        "No machines are available for this group."
                    }
                } @else {
                    form method="post"
                         action={ "/g/" (group) "/" (side) "/" (line) "/jobs" } {
                        (signature(signed))
                        input type="hidden" name="task" value=(task.to_string());
                        input type="hidden" name="commit" value=(commit);

                        @for offer in offers {
                            (choices(
                                &offer.machine.id.to_string(),
                                &offer.machine.label,
                                Some(offer.machine.presence),
                                &offer.agents,
                            ))
                        }

                        (choices("anyone", "Whoever is free", None, &free(offers)))

                        button class="button button--primary" type="submit" { "Ask" }
                    }
                }
            }
        }
    }
}

/// One machine, with the agents it has under it.
fn choices(value: &str, label: &str, presence: Option<Presence>, agents: &[Detected]) -> Markup {
    html! {
        div class="launcher__machine" {
            p class="launcher__name" {
                (label)
                @if let Some(presence) = presence {
                    " " span class="quiet" { (presence) }
                }
            }

            @for agent in agents {
                @let called = stemma_core::machine::known(agent.agent.as_str())
                    .map(|known| known.called)
                    .unwrap_or(agent.agent.as_str());

                @match agent.availability.reason() {
                    // Shown, named and unavailable. Its absence is
                    // something its owner can act on, and hiding it
                    // would teach them nothing.
                    Some(reason) => p class="launcher__agent launcher__agent--out" {
                        span { (called) }
                        span class="quiet identifier" { (reason) }
                    },
                    None => label class="launcher__agent" {
                        input type="radio" name="on"
                              value={ (value) "|" (agent.agent.as_str()) };
                        span { (called) }
                        @if let Some(version) = &agent.version {
                            span class="quiet identifier" { (version) }
                        }
                    },
                }
            }
        }
    }
}

/// What an agent would be, offered to whichever machine is free.
///
/// An agent is on offer if some awake machine has it ready. One that no
/// machine here has is still listed, with the reason, because that is a
/// true thing about this group's machines.
fn free(offers: &[Offer]) -> Vec<Detected> {
    stemma_core::machine::CATALOGUE
        .iter()
        .map(|known| {
            let anywhere = offers
                .iter()
                .filter(|offer| offer.machine.presence == Presence::Awake)
                .flat_map(|offer| offer.agents.iter())
                .find(|agent| agent.agent.as_str() == known.slug && agent.availability.is_usable());

            match anywhere {
                Some(agent) => Detected::ready(agent.agent.clone(), agent.version.clone()),
                None => Detected::missing(known.id()),
            }
        })
        .collect()
}

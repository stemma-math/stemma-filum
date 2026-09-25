//! Receiving an agent's change and putting its human decision on record.
//!
//! One screen, whether the words come from an agent or from a person:
//! only the attribution line changes. What it shows is what *moved*,
//! because a proposal arrives as a whole document and nobody can read a
//! whole document looking for the three lines that are new.
//!
//! Everything the freeze guard forbids touching is marked, so a reader
//! can see at a glance that the statements the line rests on are where
//! they were.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Json, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use serde_json::json;
use stemma_core::diff::{self, Change};
use stemma_core::id::ProposalId;
use stemma_core::repository::{self, Side};

use super::api::Credential;
use super::layout::{page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::proposals;

/// Shows the one review screen used for an agent's proposal.
pub async fn review(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let id: ProposalId = id.parse().map_err(|_| WebError::NotFound)?;
    let proposal = instance
        .read(move |c| proposals::by_id(c, &id))
        .await?
        .ok_or(WebError::NotFound)?;

    if proposal.job.launched_by != signed.user.id {
        return Err(WebError::NotFound);
    }

    let stands = against(&instance, &proposal).await;
    let chrome = super::layout::around(&instance, &signed, "Review a proposal")
        .await?
        .at("/");

    Ok(page(&chrome, body(&signed, &proposal, stands.as_deref())).into_response())
}

/// The source the proposal would replace, as the repository has it.
async fn against(instance: &Instance, proposal: &proposals::Proposal) -> Option<String> {
    let repositories = instance.repositories();
    let path = match proposal.job.side {
        Side::Shared => repositories.shared(&proposal.job.group),
        Side::Private => repositories.personal(&proposal.job.group, &proposal.job.launched_by),
    };
    let inside = format!(
        "{}/{}/{}",
        repository::LINES,
        proposal.job.line,
        repository::ENTRY
    );
    let commit = proposal.job.commit.clone();

    instance
        .blocking(move || Ok(crate::git::show(&path, &commit, &inside).ok()))
        .await
        .ok()
        .flatten()
}

/// The review screen.
fn body(signed: &SignedIn, proposal: &proposals::Proposal, stands: Option<&str>) -> Markup {
    let rows = stands.map(|stands| diff::between(stands, &proposal.source));
    let frozen = diff::frozen(stands.unwrap_or(&proposal.source));

    html! {
        div class="stack" {
            p class="label" {
                a href={ "/g/" (proposal.job.group.slug()) "/" (proposal.job.side)
                         "/" (proposal.job.line) } {
                    (proposal.job.line.as_str())
                }
                " · " (proposal.job.task)
            }
            h1 { "Review the proposed source" }

            // An attribution is a record. It says what happened once and
            // never that anybody endorses it now, so it is set in mono
            // and framed by nothing.
            p class="quiet identifier" {
                (proposal.vendor) " · " (proposal.model)
                " · prompt " (proposal.prompt_revision)
            }
        }

        div class="block" {
            p class="label" {
                "What changed"
                @if let Some(rows) = &rows {
                    " · " (diff::extent(rows).said())
                }
            }

            @match &rows {
                Some(rows) => (drawn(rows, &frozen)),
                None => {
                    p class="quiet" {
                        "The original version is no longer available. The complete proposal is shown instead."
                    }
                    pre class="diff" { (proposal.source) }
                }
            }
        }

        @if let Some(lean) = &proposal.lean_source {
            div class="block" {
                p class="label" { "Lean module" }
                pre class="diff" { (lean) }
            }
        }

        div class="block" {
            p class="label" { "What was checked" }
            ul class="findings" {
                @for finding in findings(proposal, stands) {
                    li { (finding) }
                }
            }
        }

        @if proposal.state == "awaiting_review" {
            form method="post" action={ "/proposals/" (proposal.id) "/decide" } {
                (signature(signed))
                button class="button" name="decision" value="reject" {
                    "Reject"
                }
                button class="button" name="decision" value="ask_again" {
                    "Ask again"
                }
                button class="button button--primary" name="decision" value="accept" { "Accept" }
            }
            p class="quiet" {
                "Accepting records you as the reviewer and preserves the model information. "
                "Apply the proposal to your machine with "
                code { "stemma proposal apply " (proposal.id) }
                ", or from the application."
            }
        } @else {
            p class="quiet" { "This proposal was " (proposal.state) "." }
        }
    }
}

/// The two versions, line by line.
///
/// A statement the guard freezes is framed at the corners of the whole
/// run rather than of each of its lines: it is one statement, not five.
fn drawn(rows: &[diff::Row], frozen: &std::collections::BTreeSet<usize>) -> Markup {
    let held_at = |at: usize| {
        rows.get(at)
            .and_then(|row: &diff::Row| row.before)
            .is_some_and(|line| frozen.contains(&line))
    };

    html! {
        div class="diff" {
            @for (at, row) in rows.iter().enumerate() {
                @let held = held_at(at);

                div class={
                    "diff__row"
                    @match row.change {
                        Change::Added => " diff__row--added",
                        Change::Removed => " diff__row--removed",
                        Change::Kept => "",
                    }
                    @if held { " diff__row--held" }
                    @if held && (at == 0 || !held_at(at - 1)) { " diff__row--held-first" }
                    @if held && !held_at(at + 1) { " diff__row--held-last" }
                } {
                    span class="diff__at" {
                        @match row.before {
                            Some(at) => (at),
                            None => "",
                        }
                    }
                    span class="diff__at" {
                        @match row.after {
                            Some(at) => (at),
                            None => "",
                        }
                    }
                    span class="diff__mark" {
                        @match row.change {
                            Change::Added => "+",
                            Change::Removed => "−",
                            Change::Kept => " ",
                        }
                    }
                    span class="diff__text" { (row.text) }
                }
            }
        }
    }
}

/// What the instance checked before this reached a person.
///
/// Every one of these is something a machine established. None of them
/// is a judgement about the mathematics, and the screen must not read
/// as though the checking were the review.
fn findings(proposal: &proposals::Proposal, stands: Option<&str>) -> Vec<String> {
    let mut found = Vec::new();

    match stemma_core::line::Line::read(&proposal.source) {
        Ok(_) => found.push("the proposed source reads as a line".to_owned()),
        Err(faults) => found.push(format!(
            "the proposed source does not read as a line: {}",
            faults
                .first()
                .map(ToString::to_string)
                .unwrap_or_else(|| "it is not a line".to_owned())
        )),
    }

    if let Some(stands) = stands {
        match stemma_core::proposal::preserves_central_statements(stands, &proposal.source) {
            Ok(()) => found.push(
                "the statements the line rests on, and what they declare they use, \
                 are untouched"
                    .to_owned(),
            ),
            Err(refused) => found.push(refused.to_string()),
        }
    }

    if proposal.lean_source.is_some() {
        found.push("Lean accepted the complete module on the machine that ran it".to_owned());
    }

    found
}

#[derive(Deserialize)]
/// The signed answer from the review form.
pub struct Decision {
    decision: String,
    form_token: String,
}

/// Records the launcher's decision; acceptance signs the words as theirs.
pub async fn decide(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(id): Path<String>,
    axum::Form(form): axum::Form<Decision>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let id: ProposalId = id.parse().map_err(|_| WebError::NotFound)?;

    settle(&instance, &signed.user.id, &id, &form.decision).await?;

    Ok(Redirect::to(&format!("/proposals/{id}")).into_response())
}

/// Records a decision, wherever it was made.
async fn settle(
    instance: &Instance,
    by: &stemma_core::id::UserId,
    id: &ProposalId,
    decision: &str,
) -> Result<(), WebError> {
    let accept = match decision {
        "accept" => true,
        "reject" | "ask_again" => false,
        _ => return Err(WebError::BadRequest("Choose accept or reject.".to_owned())),
    };
    let ask_again = decision == "ask_again";
    let by = by.clone();
    let id = *id;

    let settled = instance
        .write(move |c| {
            let Some(proposal) = proposals::by_id(c, &id)? else {
                return Ok(false);
            };
            if !proposals::decide(c, &id, &by, accept)? {
                return Ok(false);
            }

            // Asking again is not a third state: the proposal is
            // declined and the same question goes back in the queue.
            if ask_again {
                crate::store::jobs::launch(
                    c,
                    &crate::store::jobs::Launch {
                        group: proposal.job.group,
                        side: proposal.job.side,
                        line: proposal.job.line,
                        commit: proposal.job.commit,
                        against: None,
                        task: proposal.job.task,
                        agent: proposal.job.agent,
                        choice: proposal.job.choice,
                        by,
                    },
                )?;
            }

            Ok(true)
        })
        .await?;

    match settled {
        true => Ok(()),
        false => Err(WebError::NotFound),
    }
}

/// What this person still has to review.
pub async fn listing(
    State(instance): State<Instance>,
    credential: Credential,
) -> Result<Response, WebError> {
    let who = credential.user.id.clone();
    let awaiting = instance.read(move |c| proposals::awaiting(c, &who)).await?;

    let listed: Vec<serde_json::Value> = awaiting
        .iter()
        .map(|proposal| {
            json!({
                "proposal": proposal.id.to_string(),
                "group": proposal.job.group.slug().as_str(),
                "side": proposal.job.side.as_str(),
                "line": proposal.job.line.as_str(),
                "task": proposal.job.task.to_string(),
                "agent": proposal.job.agent.as_str(),
                "vendor": proposal.vendor,
                "model": proposal.model,
                "state": proposal.state,
            })
        })
        .collect();

    Ok(Json(json!({ "proposals": listed })).into_response())
}

/// One proposal, with the version it would replace.
pub async fn offered(
    State(instance): State<Instance>,
    credential: Credential,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let id: ProposalId = id.parse().map_err(|_| WebError::NotFound)?;
    let proposal = instance
        .read(move |c| proposals::by_id(c, &id))
        .await?
        .ok_or(WebError::NotFound)?;

    if proposal.job.launched_by != credential.user.id {
        return Err(WebError::NotFound);
    }

    let stands = against(&instance, &proposal).await;

    Ok(Json(json!({
        "proposal": proposal.id.to_string(),
        "group": proposal.job.group.slug().as_str(),
        "side": proposal.job.side.as_str(),
        "line": proposal.job.line.as_str(),
        "commit": proposal.job.commit,
        "task": proposal.job.task.to_string(),
        "agent": proposal.job.agent.as_str(),
        "vendor": proposal.vendor,
        "model": proposal.model,
        "prompt_revision": proposal.prompt_revision,
        "state": proposal.state,
        "stands": stands,
        "source": proposal.source,
        "lean_source": proposal.lean_source,
        "findings": findings(&proposal, stands.as_deref()),
    }))
    .into_response())
}

/// Records a decision made on a machine.
pub async fn api_decide(
    State(instance): State<Instance>,
    credential: Credential,
    Path(id): Path<String>,
    Json(said): Json<Said>,
) -> Result<Response, WebError> {
    let id: ProposalId = id.parse().map_err(|_| WebError::NotFound)?;

    settle(&instance, &credential.user.id, &id, said.decision.trim()).await?;

    Ok(Json(json!({ "proposal": id.to_string(), "decision": said.decision })).into_response())
}

/// What a machine sends to decide a proposal.
#[derive(Debug, Default, Deserialize)]
pub struct Said {
    /// `accept`, `reject` or `ask_again`.
    #[serde(default)]
    pub decision: String,
}

/// Gives accepted source only to the person who signed it, for local application.
pub async fn accepted(
    State(instance): State<Instance>,
    credential: Credential,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let id: ProposalId = id.parse().map_err(|_| WebError::NotFound)?;
    let proposal = instance
        .read(move |c| proposals::by_id(c, &id))
        .await?
        .ok_or(WebError::NotFound)?;

    if proposal.state != "accepted" || proposal.decided_by.as_ref() != Some(&credential.user.id) {
        return Err(WebError::NotFound);
    }

    Ok(Json(json!({
        "source": proposal.source,
        "lean_source": proposal.lean_source,
        "group": proposal.job.group.slug().as_str(),
        "side": proposal.job.side.as_str(),
        "line": proposal.job.line.as_str(),
        "commit": proposal.job.commit,
    }))
    .into_response())
}

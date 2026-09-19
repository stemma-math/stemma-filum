//! Receiving an agent's change and putting its human decision on record.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Json, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use serde_json::json;
use stemma_core::id::ProposalId;

use super::api::Credential;
use super::layout::{Chrome, page, signature};
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
    let policy = instance.policy().await?;
    let chrome = Chrome::anonymous(
        "Review a proposal",
        super::front::instance_name(&instance, &policy),
    )
    .signed_in(signed.user.clone(), signed.maintainer);
    Ok(page(&chrome, body(&signed, &proposal)).into_response())
}

fn body(signed: &SignedIn, proposal: &proposals::Proposal) -> Markup {
    html! {
        div class="stack" {
            p class="label" { "Proposal " (proposal.id) }
            h1 { "Review the proposed source" }
            p class="quiet identifier" { (proposal.vendor) " · " (proposal.model) " · prompt " (proposal.prompt_revision) }
        }
        div class="proposal" {
            pre { (proposal.source) }
            @if let Some(lean) = &proposal.lean_source {
                p class="label" { "Lean module" }
                pre { (lean) }
            }
            ul class="findings" {
                li { "source reads as a line" }
                li { "central statements and their declared dependencies are untouched" }
                @if proposal.lean_source.is_some() { li { "Lean accepted the complete module on the producing machine" } }
            }
        }
        @if proposal.state == "awaiting_review" {
            form method="post" action={ "/proposals/" (proposal.id) "/decide" } {
                (signature(signed))
                button class="button" name="decision" value="reject" { "Reject" }
                button class="button" name="decision" value="ask_again" { "Ask again" }
                button class="button" name="decision" value="accept" { "Accept" }
            }
        } @else { p class="quiet" { "This proposal was " (proposal.state) "." } }
    }
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
    let accept = match form.decision.as_str() {
        "accept" => true,
        "reject" | "ask_again" => false,
        _ => return Err(WebError::BadRequest("Choose accept or reject.".to_owned())),
    };
    let by = signed.user.id.clone();
    let ask_again = form.decision == "ask_again";
    if !instance
        .write(move |c| {
            let Some(proposal) = proposals::by_id(c, &id)? else {
                return Ok(false);
            };
            if !proposals::decide(c, &id, &by, accept)? {
                return Ok(false);
            }
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
        .await?
    {
        return Err(WebError::NotFound);
    }
    Ok(Redirect::to(&format!("/proposals/{id}")).into_response())
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
    Ok(Json(json!({"source": proposal.source, "lean_source": proposal.lean_source, "group": proposal.job.group.slug().as_str(), "side": proposal.job.side.as_str(), "line": proposal.job.line.as_str(), "commit": proposal.job.commit})).into_response())
}

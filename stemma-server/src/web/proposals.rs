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

use anyhow::Context;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Json, Response};
use maud::{Markup, html};
use serde::Deserialize;
use serde_json::json;
use stemma_core::diff::{self, Change};
use stemma_core::id::ProposalId;

use super::api::Credential;
use super::layout::page;
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

    if !may_read(&instance, &proposal, &signed.user.id).await {
        return Err(WebError::NotFound);
    }

    let stands = against(&instance, &proposal).await;
    let chrome = super::layout::around(&instance, &signed, "Review a proposal")
        .await?
        .at("/");

    Ok(page(&chrome, body(&proposal, stands.as_deref())).into_response())
}

async fn may_read(
    instance: &Instance,
    proposal: &proposals::Proposal,
    who: &stemma_core::id::UserId,
) -> bool {
    if proposal.reviewer() == who || proposal.job.launched_by == *who {
        return true;
    }
    if proposal.job.side != stemma_core::repository::Side::Shared {
        return false;
    }
    let group = proposal.job.group.clone();
    let who = who.clone();
    instance
        .read(move |c| crate::store::groups::role_of(c, &group, &who))
        .await
        .ok()
        .flatten()
        .is_some()
}

/// The source the proposal would replace: the revision its job read.
async fn against(instance: &Instance, proposal: &proposals::Proposal) -> Option<String> {
    let line = proposal.job.line_id?;
    let number = proposal.job.revision;

    instance
        .read(move |connection| crate::store::revisions::at(connection, &line, number))
        .await
        .ok()
        .flatten()
        .map(|revision| revision.source)
}

/// The line's latest source, for a review against the version that now stands.
async fn current(instance: &Instance, proposal: &proposals::Proposal) -> Option<String> {
    let line = proposal.job.line_id?;
    instance
        .read(move |c| {
            let Some(started) = crate::store::lines::by_id(c, &line)? else {
                return Ok(None);
            };
            Ok(crate::store::revisions::current(c, &started)?.map(|revision| revision.source))
        })
        .await
        .ok()
        .flatten()
}

/// The review screen.
fn body(proposal: &proposals::Proposal, stands: Option<&str>) -> Markup {
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
                @if let Some(effort) = &proposal.effort { " · " (effort) }
                " · prompt " (proposal.prompt_revision)
            }
            @if !proposal.explanation.is_empty() {
                p { (proposal.explanation) }
            }
        }

        @if !proposal.operations.is_empty() {
            div class="block" {
                p class="label" { "Operations" }
                ul class="findings" {
                    @for operation in &proposal.operations {
                        li { (operation_summary(operation)) }
                    }
                }
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

        @if let Some(change) = &proposal.library_change {
            div class="block" {
                p class="label" { "Lean library changes · base " (change.base) }
                @for file in &change.files {
                    p class="identifier" { (file.path) }
                    (drawn(
                        &diff::between(
                            file.before.as_deref().unwrap_or(""),
                            file.after.as_deref().unwrap_or(""),
                        ),
                        &std::collections::BTreeSet::new(),
                    ))
                }
            }
        }

        @if let Some(review) = &proposal.formal_review {
            (formal_review(review))
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
            a class="button button--primary" href={ "stemma://proposal/" (proposal.id) } {
                "Open in Stemma to decide"
            }
            p class="quiet" {
                "This page is read only. Review and decide in the desktop application."
            }
        } @else {
            p class="quiet" {
                @match (proposal.state.as_str(), proposal.applied_revision) {
                    ("accepted", Some(revision)) => {
                        "Accepted, and in the line since revision " (revision) "."
                    }
                    ("accepted", None) => {
                        "Accepted. One of your machines brings it into the line as soon as it is connected."
                    }
                    ("outdated", _) => {
                        "Accepted, but a block it changes was changed after it was made, "
                        "so it was not brought into the line. Ask for it again against the line as it is."
                    }
                    (state, _) => { "This proposal was " (state.replace('_', " ")) "." }
                }
            }
            @if let Some(by) = &proposal.decided_by {
                p class="quiet identifier" { "Decided by " (by) }
            }
            @if let Some(note) = &proposal.note {
                p { (note) }
            }
        }
    }
}

fn operation_summary(operation: &stemma_core::proposal::Operation) -> String {
    use stemma_core::proposal::Operation;
    match operation {
        Operation::EditBlock { block, .. } => format!("Edit block {block}"),
        Operation::AddBlocks { after, blocks } => format!(
            "Add {} block{} {}",
            blocks.len(),
            if blocks.len() == 1 { "" } else { "s" },
            after.map_or_else(
                || "at the beginning".to_owned(),
                |block| format!("after {block}")
            ),
        ),
        Operation::RemoveBlock { block } => format!("Remove block {block}"),
        Operation::WriteLibraryFile { path, after, .. } => match after {
            Some(_) => format!("Write library file {path}"),
            None => format!("Remove library file {path}"),
        },
    }
}

fn formal_review(review: &stemma_core::proposal::FormalReview) -> Markup {
    html! {
        div class="block" {
            p class="label" { "Formal reach" }
            p {
                @if review.impact.proofs_only {
                    "Proofs only: every printed statement and line link is unchanged."
                } @else {
                    @if !review.impact.statement_closures.is_empty() {
                        "Statement closure changed for "
                        (review.impact.statement_closures.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
                        ". "
                    }
                    @if !review.impact.line_links.is_empty() {
                        "Links of the line changed for "
                        (review.impact.line_links.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
                        "."
                    }
                }
            }
            @for reviewed in &review.blocks {
                div class="formal-review" {
                    p class="label" { (reviewed.check.block) " · " (reviewed.check.state.as_str()) }
                    @for declaration in &reviewed.check.declarations {
                        p class="identifier" { (declaration.name) " · " (declaration.module) }
                        pre class="diff" { (declaration.statement) }
                        @if !declaration.reaches.is_empty() {
                            p class="quiet" { "Reaches " (declaration.reaches.join(", ")) }
                        }
                    }
                    p { strong { "Agent reading: " } (reviewed.reading) }
                }
            }
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
    if let Some(change) = &proposal.library_change {
        found.push(format!(
            "the approved library change is based on {} and touches {} file{}",
            change.base,
            change.files.len(),
            if change.files.len() == 1 { "" } else { "s" }
        ));
    }
    if proposal.formal_review.is_some() {
        found.push(
            "the worker compiled the staged library change and extracted its printed statements"
                .to_owned(),
        );
    }

    found
}

/// Records a decision, wherever it was made.
async fn settle(
    instance: &Instance,
    by: &stemma_core::id::UserId,
    id: &ProposalId,
    decision: &str,
    note: Option<&str>,
) -> Result<(), WebError> {
    let accept = match decision {
        "accept" => true,
        "reject" | "ask_again" => false,
        _ => return Err(WebError::BadRequest("Choose accept or reject.".to_owned())),
    };
    let ask_again = decision == "ask_again";
    let note = note
        .map(str::trim)
        .filter(|note| !note.is_empty())
        .map(str::to_owned);
    let by = by.clone();
    let id = *id;

    let library_commit = if accept {
        let by_for_read = by.clone();
        let proposal = instance
            .read(move |connection| {
                let refused = |reason: &str| WebError::BadRequest(reason.to_owned());
                let proposal = proposals::by_id(connection, &id)?
                    .ok_or_else(|| refused("the proposal does not exist"))?;
                if proposal.state != "awaiting_review" || proposal.reviewer() != &by_for_read {
                    anyhow::bail!(refused("the proposal is not awaiting this person's review"));
                }
                if let Some(line_id) = proposal.job.line_id {
                    let base =
                        crate::store::revisions::at(connection, &line_id, proposal.job.revision)?
                            .context("the proposal's base revision is unavailable")?;
                    let started = crate::store::lines::by_id(connection, &line_id)?
                        .context("the proposal's line is unavailable")?;
                    let current = crate::store::revisions::current(connection, &started)?
                        .context("the current revision is unavailable")?;
                    let clashes = stemma_core::reconcile::between(
                        &base.source,
                        &current.source,
                        &proposal.source,
                    )
                    .map_err(|error| refused(&error.to_string()))?;
                    if !clashes.is_empty() {
                        anyhow::bail!(refused(
                            "This proposal touches blocks changed since it was made. Ask again \
                             on the current line."
                        ));
                    }
                }
                let user = crate::store::users::by_id(connection, proposal.reviewer())?
                    .context("the reviewer does not exist")?;
                Ok((proposal, user))
            })
            .await?;
        match proposal.0.library_change.clone() {
            None => None,
            Some(change) => {
                let group = proposal.0.job.group.clone();
                let owner = proposal.0.job.launched_by.clone();
                let side = proposal.0.job.side;
                let name = proposal.1.display_name.clone();
                let email = proposal.1.email.clone();
                let written = instance
                    .writing_git(move |repositories| {
                        let path = match side {
                            stemma_core::repository::Side::Shared => repositories.shared(&group),
                            stemma_core::repository::Side::Private => {
                                repositories.personal(&group, &owner)
                            }
                        };
                        crate::git::write_approved_library(
                            &path,
                            &change.base,
                            &change.files,
                            &crate::git::Author {
                                name: &name,
                                email: &email,
                            },
                        )
                    })
                    .await?;
                match written {
                    crate::git::LibraryWritten::Taken(commit) => Some(commit),
                    crate::git::LibraryWritten::Conflicted(files) => {
                        return Err(WebError::BadRequest(format!(
                            "The library changed in these files since the agent worked: {}. Ask the agent to bring the changes together.",
                            files.join(", ")
                        )));
                    }
                }
            }
        }
    } else {
        None
    };

    let by_for_write = by.clone();
    let note_for_write = note.clone();
    let decision = decision.to_owned();

    let settled = instance
        .write(move |c| {
            let transaction = c.transaction()?;
            let c = &transaction;
            let Some(proposal) = proposals::by_id(c, &id)? else {
                return Ok(false);
            };
            if !proposals::decide(c, &id, &by_for_write, &decision, note_for_write.as_deref())? {
                return Ok(false);
            }
            if let Some(commit) = &library_commit {
                proposals::library_applied(c, &id, commit)?;
                crate::store::events::publish(
                    c,
                    crate::store::events::Audience::of(
                        proposal.job.side,
                        &proposal.job.group,
                        &proposal.job.launched_by,
                    ),
                    &stemma_core::event::Event::Library {
                        group: proposal.job.group.slug().as_str().to_owned(),
                        side: proposal.job.side,
                        commit: commit.clone(),
                        change: stemma_core::event::LibraryChange::Written,
                    },
                )?;
            }
            if accept {
                for audit in crate::store::formal_audits::accept(c, &proposal, &by_for_write)? {
                    let Some(recorded) = crate::store::formal_audits::for_line(
                        c,
                        &proposal.job.line_id.context("the proposal has no line")?,
                    )?
                    .into_iter()
                    .find(|recorded| recorded.id == audit) else {
                        continue;
                    };
                    crate::store::events::publish(
                        c,
                        crate::store::events::Audience::of(
                            proposal.job.side,
                            &proposal.job.group,
                            &proposal.job.launched_by,
                        ),
                        &stemma_core::event::Event::FormalAudit {
                            group: proposal.job.group.slug().as_str().to_owned(),
                            line: proposal.job.line.as_str().to_owned(),
                            block: recorded.block,
                            state: "audited".to_owned(),
                        },
                    )?;
                }
            }

            // Asking again is not a third state: the proposal is
            // declined and the same question goes back in the queue.
            if ask_again {
                let current_revision = match proposal.job.line_id {
                    Some(id) => {
                        let line = crate::store::lines::by_id(c, &id)?
                            .context("the proposal's line is unavailable")?;
                        if line.is_deleted() {
                            anyhow::bail!("the line was deleted; this task cannot be run again");
                        }
                        line.revision
                    }
                    None => proposal.job.revision,
                };
                let context = format!(
                    "Previous attempt {} was rejected. Its source was:\n{}\nReviewer note: {}",
                    proposal.id,
                    proposal.source,
                    note_for_write.as_deref().unwrap_or("No further note.")
                );
                let request = match proposal.job.request {
                    Some(previous) => Some(format!("{previous}\n\n{context}")),
                    None => Some(context),
                };
                crate::store::jobs::launch(
                    c,
                    &crate::store::jobs::Launch {
                        group: proposal.job.group,
                        side: proposal.job.side,
                        line: proposal.job.line,
                        line_id: proposal.job.line_id,
                        revision: current_revision,
                        against: None,
                        task: match proposal.job.task {
                            stemma_core::job::Task::Formalize { block }
                                if proposal.formal_review.is_some() =>
                            {
                                stemma_core::job::Task::FollowRoute { block }
                            }
                            task => task,
                        },
                        agent: proposal.job.agent,
                        request,
                        grants: proposal.job.grants,
                        choice: proposal.job.choice,
                        by: by_for_write,
                    },
                )?;
            }

            transaction.commit()?;
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
    let (awaiting, recent) = instance
        .read(move |c| Ok((proposals::awaiting(c, &who)?, proposals::recent(c, &who)?)))
        .await?;

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

    let decided: Vec<_> = recent.iter().map(summary).collect();
    Ok(Json(json!({ "proposals": listed, "recent": decided })).into_response())
}

fn summary(proposal: &proposals::Proposal) -> serde_json::Value {
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
        "decision": proposal.decision,
        "note": proposal.note,
    })
}

/// The proposals kept in a line's history, including attribution and decision.
pub async fn history(
    State(instance): State<Instance>,
    credential: Credential,
    Path((group, side, line)): Path<(String, String, String)>,
) -> Result<Response, WebError> {
    let group = format!("grp:{group}")
        .parse()
        .map_err(|_| WebError::NotFound)?;
    let side = side.parse().map_err(|_| WebError::NotFound)?;
    let line = line.parse().map_err(|_| WebError::NotFound)?;
    let who = credential.user.id.clone();
    let entries = instance
        .read(move |c| {
            if crate::store::groups::role_of(c, &group, &who)?.is_none() {
                return Ok(None);
            }
            let Some(line) = crate::store::lines::by_name(c, &group, side, &line, &who)? else {
                return Ok(None);
            };
            Ok(Some(proposals::for_line(c, &line.id)?))
        })
        .await?
        .ok_or(WebError::NotFound)?;
    Ok(
        Json(json!({ "proposals": entries.iter().map(|proposal| json!({
        "proposal": proposal.id.to_string(),
        "task": proposal.job.task.to_string(),
        "agent": proposal.job.agent.as_str(),
        "vendor": proposal.vendor,
        "model": proposal.model,
        "instructions_version": proposal.prompt_revision,
        "explanation": proposal.explanation,
        "state": proposal.state,
        "decision": proposal.decision,
        "note": proposal.note,
        "reviewer": proposal.reviewer().to_string(),
        "decided_by": proposal.decided_by.as_ref().map(ToString::to_string),
        "applied_revision": proposal.applied_revision,
    })).collect::<Vec<_>>() }))
        .into_response(),
    )
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

    if !may_read(&instance, &proposal, &credential.user.id).await {
        return Err(WebError::NotFound);
    }

    let stands = against(&instance, &proposal).await;
    let current = current(&instance, &proposal).await;
    let group = proposal.job.group.clone();
    let members = if proposal.job.launched_by == credential.user.id
        && proposal.job.side == stemma_core::repository::Side::Shared
    {
        instance.read(move |c| crate::store::groups::members(c, &group)).await?
            .into_iter().filter(|member| member.user.suspended_at.is_none())
            .map(|member| json!({ "id": member.user.id.to_string(), "name": member.user.display_name }))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    Ok(Json(json!({
        "proposal": proposal.id.to_string(),
        "group": proposal.job.group.slug().as_str(),
        "side": proposal.job.side.as_str(),
        "line": proposal.job.line.as_str(),
        "line_id": proposal.job.line_id.map(|line| line.to_string()),
        "revision": proposal.job.revision,
        "task": proposal.job.task.to_string(),
        "agent": proposal.job.agent.as_str(),
        "vendor": proposal.vendor,
        "model": proposal.model,
        "prompt_revision": proposal.prompt_revision,
        "operations": proposal.operations,
        "explanation": proposal.explanation,
        "effort": proposal.effort,
        "state": proposal.state,
        "decision": proposal.decision,
        "note": proposal.note,
        "reviewer": proposal.reviewer().to_string(),
        "decided_by": proposal.decided_by.as_ref().map(ToString::to_string),
        "members": members,
        "stands": stands,
        "current": current,
        "source": proposal.source,
        "lean_source": proposal.lean_source,
        "library_change": proposal.library_change,
        "formal_review": proposal.formal_review,
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

    settle(
        &instance,
        &credential.user.id,
        &id,
        said.decision.trim(),
        said.note.as_deref(),
    )
    .await?;

    Ok(Json(json!({ "proposal": id.to_string(), "decision": said.decision })).into_response())
}

/// What a machine sends to decide a proposal.
#[derive(Debug, Default, Deserialize)]
pub struct Said {
    /// `accept`, `reject` or `ask_again`.
    #[serde(default)]
    pub decision: String,
    /// Optional reason or instruction.
    pub note: Option<String>,
}

/// The member chosen for a handoff.
#[derive(Debug, Deserialize)]
pub struct Handoff {
    reviewer: String,
}

/// The launcher assigns a shared proposal to a group member.
pub async fn api_handoff(
    State(instance): State<Instance>,
    credential: Credential,
    Path(id): Path<String>,
    Json(request): Json<Handoff>,
) -> Result<Response, WebError> {
    let id: ProposalId = id.parse().map_err(|_| WebError::NotFound)?;
    let to = request
        .reviewer
        .parse()
        .map_err(|_| WebError::BadRequest("Choose a group member.".to_owned()))?;
    let by = credential.user.id.clone();
    let changed = instance
        .write(move |c| {
            let transaction = c.transaction()?;
            let changed = proposals::handoff(&transaction, &id, &by, &to)?;
            transaction.commit()?;
            Ok(changed)
        })
        .await?;
    if !changed {
        return Err(WebError::NotFound);
    }
    Ok(Json(json!({ "reviewer": request.reviewer })).into_response())
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
        "library_change": proposal.library_change,
        "group": proposal.job.group.slug().as_str(),
        "side": proposal.job.side.as_str(),
        "line": proposal.job.line.as_str(),
        "line_id": proposal.job.line_id.map(|line| line.to_string()),
        "revision": proposal.job.revision,
    }))
    .into_response())
}

/// What this person accepted and no machine of theirs has applied yet,
/// with everything a machine needs to apply it.
pub async fn to_apply(
    State(instance): State<Instance>,
    credential: Credential,
) -> Result<Response, WebError> {
    let who = credential.user.id.clone();
    let waiting = instance
        .read(move |c| proposals::accepted_unapplied(c, &who))
        .await?;

    let listed: Vec<serde_json::Value> = waiting
        .iter()
        .map(|proposal| {
            json!({
                "proposal": proposal.id.to_string(),
                "group": proposal.job.group.slug().as_str(),
                "side": proposal.job.side.as_str(),
                "line": proposal.job.line.as_str(),
                "line_id": proposal.job.line_id.map(|line| line.to_string()),
                "revision": proposal.job.revision,
                "source": proposal.source,
                "lean_source": proposal.lean_source,
            })
        })
        .collect();

    Ok(Json(json!({ "proposals": listed })).into_response())
}

/// What a machine says when it applied a proposal.
#[derive(Debug, Deserialize)]
pub struct Applied {
    /// The revision the line became with it.
    pub revision: u64,
}

/// Records that an accepted proposal was applied, and as which revision.
pub async fn applied(
    State(instance): State<Instance>,
    credential: Credential,
    Path(id): Path<String>,
    Json(applied): Json<Applied>,
) -> Result<Response, WebError> {
    let id: ProposalId = id.parse().map_err(|_| WebError::NotFound)?;
    let who = credential.user.id.clone();

    let recorded = instance
        .read(move |c| proposals::applied(c, &id, &who, applied.revision))
        .await?;

    match recorded {
        true => Ok(Json(json!({ "applied": true })).into_response()),
        false => Err(WebError::NotFound),
    }
}

/// Records that an accepted proposal came too late: a block it changes
/// was changed since the revision it was made from.
pub async fn outdated(
    State(instance): State<Instance>,
    credential: Credential,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let id: ProposalId = id.parse().map_err(|_| WebError::NotFound)?;
    let who = credential.user.id.clone();

    let recorded = instance
        .read(move |c| proposals::outdated(c, &id, &who))
        .await?;

    match recorded {
        true => Ok(Json(json!({ "outdated": true })).into_response()),
        false => Err(WebError::NotFound),
    }
}

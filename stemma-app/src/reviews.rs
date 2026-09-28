//! Proposals waiting for this person's judgement.

use serde::Serialize;
use std::collections::BTreeMap;
use stemma_core::diff::{self, Change};
use stemma_core::document::Document;
use stemma_core::id::BlockId;
use stemma_core::proposal::Verdict;
use stemma_core::reconcile::{self, Keep};

use crate::{Session, carried_out, failed, said};

#[derive(Serialize)]
pub struct Waiting {
    proposal: String,
    group: String,
    side: String,
    line: String,
    task: String,
    agent: String,
    vendor: String,
    model: String,
    state: String,
    decision: Option<String>,
    note: Option<String>,
}

#[derive(Serialize)]
pub struct Reviews {
    waiting: Vec<Waiting>,
    recent: Vec<Waiting>,
}

/// One row of the comparison between the standing source and the proposal.
#[derive(Serialize)]
pub struct Moved {
    change: Change,
    text: String,
    before: Option<usize>,
    after: Option<usize>,
    /// Whether the row belongs to a statement agents may not touch.
    frozen: bool,
}

#[derive(Serialize)]
pub struct Review {
    proposal: String,
    group: String,
    side: String,
    line: String,
    task: String,
    agent: String,
    vendor: String,
    model: String,
    prompt_revision: String,
    operations: Vec<stemma_core::proposal::Operation>,
    explanation: String,
    effort: Option<String>,
    state: String,
    extent: String,
    rows: Vec<Moved>,
    source: String,
    stands: Option<String>,
    current: Option<String>,
    merged: Option<Document>,
    outdated: Vec<String>,
    decision: Option<String>,
    note: Option<String>,
    reviewer: Option<String>,
    decided_by: Option<String>,
    members: Vec<stemma::api::ReviewMember>,
    lean_source: Option<String>,
    formal_review: Option<stemma_core::proposal::FormalReview>,
    library_files: Vec<LibraryFileReview>,
    findings: Vec<String>,
}

#[derive(Serialize)]
struct LibraryFileReview {
    path: String,
    rows: Vec<Moved>,
}

#[tauri::command(async)]
pub fn proposals() -> Result<Reviews, String> {
    let session = Session::open()?;
    let reviews = session.server.reviews(&session.token).map_err(failed)?;
    let rows = |entries: Vec<stemma::api::Waiting>| {
        entries
            .into_iter()
            .map(|one| Waiting {
                proposal: one.proposal,
                group: one.group,
                side: one.side,
                line: one.line,
                task: one.task,
                agent: one.agent,
                vendor: one.vendor,
                model: one.model,
                state: one.state,
                decision: one.decision,
                note: one.note,
            })
            .collect()
    };
    Ok(Reviews {
        waiting: rows(reviews.proposals),
        recent: rows(reviews.recent),
    })
}

/// One proposal, compared row by row against the source that stands.
#[tauri::command(async)]
pub fn proposal(proposal: String, current: Option<String>) -> Result<Review, String> {
    let session = Session::open()?;
    let offered = session
        .server
        .proposal(&session.token, &proposal)
        .map_err(failed)?;

    let stands = offered.stands.clone().unwrap_or_default();
    let mut outdated = Vec::new();
    let current = current.or(offered.current.clone());
    let merged = match current.as_deref() {
        Some(current) if !stands.is_empty() => {
            let conflicts = reconcile::between(&stands, current, &offered.source).map_err(said)?;
            let decisions: BTreeMap<BlockId, Keep> = conflicts
                .iter()
                .map(|one| {
                    outdated.push(one.block.to_string());
                    (one.block, Keep::Here)
                })
                .collect();
            let source =
                reconcile::merge(&stands, current, &offered.source, &decisions).map_err(said)?;
            Some(Document::parse(&source).map_err(said)?)
        }
        _ => Document::parse(&offered.source).ok(),
    };
    let rows = diff::between(&stands, &offered.source);
    let frozen = diff::frozen(&stands);
    let extent = diff::extent(&rows).said();
    let library_files = offered
        .library_change
        .as_ref()
        .map(|change| {
            change
                .files
                .iter()
                .map(|file| LibraryFileReview {
                    path: file.path.clone(),
                    rows: diff::between(
                        file.before.as_deref().unwrap_or(""),
                        file.after.as_deref().unwrap_or(""),
                    )
                    .into_iter()
                    .map(|row| Moved {
                        change: row.change,
                        text: row.text,
                        before: row.before,
                        after: row.after,
                        frozen: false,
                    })
                    .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Review {
        proposal: offered.proposal,
        group: offered.group,
        side: offered.side,
        line: offered.line,
        task: offered.task,
        agent: offered.agent,
        vendor: offered.vendor,
        model: offered.model,
        prompt_revision: offered.prompt_revision,
        operations: offered.operations,
        explanation: offered.explanation,
        effort: offered.effort,
        state: offered.state,
        extent,
        rows: rows
            .into_iter()
            .map(|row| Moved {
                change: row.change,
                frozen: row.before.is_some_and(|at| frozen.contains(&at)),
                text: row.text,
                before: row.before,
                after: row.after,
            })
            .collect(),
        source: offered.source,
        stands: offered.stands,
        current,
        merged,
        outdated,
        decision: offered.decision,
        note: offered.note,
        reviewer: offered.reviewer,
        decided_by: offered.decided_by,
        members: offered.members,
        lean_source: offered.lean_source,
        formal_review: offered.formal_review,
        library_files,
        findings: offered.findings,
    })
}

/// Records the reviewer's decision about a proposal.
#[tauri::command(async)]
pub fn decide_proposal(
    proposal: String,
    decision: Verdict,
    note: Option<String>,
) -> Result<String, String> {
    carried_out(|home, out| {
        stemma::proposals::decide_with_note(home, &proposal, decision, note.as_deref(), out)
    })
}

/// Writes an accepted proposal into this machine's clone.
#[tauri::command(async)]
pub fn apply_proposal(proposal: String) -> Result<String, String> {
    carried_out(|home, out| stemma::proposals::apply(home, &proposal, out))
}

#[tauri::command(async)]
pub fn handoff_proposal(proposal: String, reviewer: String) -> Result<(), String> {
    let session = Session::open()?;
    session
        .server
        .handoff_proposal(&session.token, &proposal, &reviewer)
        .map_err(failed)
}

#[tauri::command(async)]
pub fn line_history(path: String) -> Result<Vec<stemma::api::LineProposal>, String> {
    let line = crate::workspace::LineAt::admit(&path)?;
    let session = Session::open()?;
    session
        .server
        .line_proposals(&session.token, &line.group, &line.side, &line.name)
        .map_err(failed)
}

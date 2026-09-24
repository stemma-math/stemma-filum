//! Proposals waiting for this person's judgement.

use serde::Serialize;
use stemma_core::diff::{self, Change};

use crate::{Session, carried_out, failed};

#[derive(Serialize)]
pub struct Waiting {
    proposal: String,
    group: String,
    line: String,
    task: String,
    agent: String,
    vendor: String,
    model: String,
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
    state: String,
    extent: String,
    rows: Vec<Moved>,
    source: String,
    lean_source: Option<String>,
    findings: Vec<String>,
}

#[tauri::command(async)]
pub fn proposals() -> Result<Vec<Waiting>, String> {
    let session = Session::open()?;
    Ok(session
        .server
        .proposals(&session.token)
        .map_err(failed)?
        .into_iter()
        .map(|waiting| Waiting {
            proposal: waiting.proposal,
            group: waiting.group,
            line: waiting.line,
            task: waiting.task,
            agent: waiting.agent,
            vendor: waiting.vendor,
            model: waiting.model,
        })
        .collect())
}

/// One proposal, compared row by row against the source that stands.
#[tauri::command(async)]
pub fn proposal(proposal: String) -> Result<Review, String> {
    let session = Session::open()?;
    let offered = session
        .server
        .proposal(&session.token, &proposal)
        .map_err(failed)?;

    let stands = offered.stands.clone().unwrap_or_default();
    let rows = diff::between(&stands, &offered.source);
    let frozen = diff::frozen(&stands);
    let extent = diff::extent(&rows).said();

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
        lean_source: offered.lean_source,
        findings: offered.findings,
    })
}

/// Records `accept`, `reject` or `ask_again` for a proposal.
#[tauri::command(async)]
pub fn decide_proposal(proposal: String, decision: String) -> Result<String, String> {
    carried_out(|home, out| stemma::proposals::decide(home, &proposal, &decision, out))
}

/// Writes an accepted proposal into this machine's clone.
#[tauri::command(async)]
pub fn apply_proposal(proposal: String) -> Result<String, String> {
    carried_out(|home, out| stemma::proposals::apply(home, &proposal, out))
}

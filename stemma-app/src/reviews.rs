//! Proposals waiting for this person's judgement.

use serde::Serialize;
use std::collections::BTreeMap;
use stemma_core::diff::{self, Change};
use stemma_core::document::Document;
use stemma_core::id::BlockId;
use stemma_core::proposal::Verdict;
use stemma_core::reconcile::{self, Keep};

use crate::workspace::LineAt;
use crate::{Session, carried_out, failed, said};

/// One proposal in a reviewer's list.
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

/// The proposals waiting for this person, and the ones they decided last.
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

/// One proposal, laid out for review.
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

/// One file of the group's Lean library that a proposal changes.
#[derive(Serialize)]
struct LibraryFileReview {
    path: String,
    rows: Vec<Moved>,
}

impl From<stemma::api::Waiting> for Waiting {
    fn from(one: stemma::api::Waiting) -> Self {
        Self {
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
        }
    }
}

impl Moved {
    fn of(row: diff::Row, frozen: bool) -> Self {
        Self {
            change: row.change,
            text: row.text,
            before: row.before,
            after: row.after,
            frozen,
        }
    }
}

/// The proposals waiting for this person's decision, and the last ones
/// they decided.
#[tauri::command]
pub async fn proposals() -> Result<Reviews, String> {
    crate::blocking(move || -> Result<Reviews, String> {
        let session = Session::open()?;
        let reviews = session.server.reviews(&session.token).map_err(failed)?;
        Ok(Reviews {
            waiting: reviews.proposals.into_iter().map(Waiting::from).collect(),
            recent: reviews.recent.into_iter().map(Waiting::from).collect(),
        })
    })
    .await
}

/// One proposal, compared row by row against the source that stands.
///
/// `current` is the line as the editor has it, when the proposal is
/// reviewed in its line; otherwise the instance's latest revision is used.
#[tauri::command]
pub async fn proposal(proposal: String, current: Option<String>) -> Result<Review, String> {
    crate::blocking(move || -> Result<Review, String> {
        let session = Session::open()?;
        let offered = session
            .server
            .proposal(&session.token, &proposal)
            .map_err(failed)?;

        let stands = offered.stands.clone().unwrap_or_default();
        let current = current.or(offered.current.clone());
        let (merged, outdated) = merged(&stands, current.as_deref(), &offered.source)?;
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
                        .map(|row| Moved::of(row, false))
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
                .map(|row| {
                    let frozen = row.before.is_some_and(|at| frozen.contains(&at));
                    Moved::of(row, frozen)
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
    })
    .await
}

/// The line as it would be with the proposal, and the blocks on which the
/// proposal is outdated.
///
/// The proposal was written against `stands`; when the line is at
/// `current` now, the two are brought together and every block both
/// changed keeps the current version. Without a standing source to
/// compare against, the proposal is taken as it is.
fn merged(
    stands: &str,
    current: Option<&str>,
    offered: &str,
) -> Result<(Option<Document>, Vec<String>), String> {
    let Some(current) = current.filter(|_| !stands.is_empty()) else {
        return Ok((Document::parse(offered).ok(), Vec::new()));
    };

    let conflicts = reconcile::between(stands, current, offered).map_err(failed)?;
    let outdated = conflicts.iter().map(|one| one.block.to_string()).collect();
    let decisions: BTreeMap<BlockId, Keep> = conflicts
        .iter()
        .map(|one| (one.block, Keep::Here))
        .collect();
    let source = reconcile::merge(stands, current, offered, &decisions).map_err(failed)?;
    let document = Document::parse(&source).map_err(said)?;

    Ok((Some(document), outdated))
}

/// Records the reviewer's decision about a proposal.
#[tauri::command]
pub async fn decide_proposal(
    proposal: String,
    decision: Verdict,
    note: Option<String>,
) -> Result<String, String> {
    crate::blocking(move || -> Result<String, String> {
        carried_out(|home, out| {
            stemma::proposals::decide_with_note(home, &proposal, decision, note.as_deref(), out)
        })
    })
    .await
}

/// Hands the review of a proposal to another member of the group.
#[tauri::command]
pub async fn handoff_proposal(proposal: String, reviewer: String) -> Result<(), String> {
    crate::blocking(move || -> Result<(), String> {
        let session = Session::open()?;
        session
            .server
            .handoff_proposal(&session.token, &proposal, &reviewer)
            .map_err(failed)
    })
    .await
}

/// Every proposal made for a line, decided or not.
#[tauri::command]
pub async fn line_history(path: String) -> Result<Vec<stemma::api::LineProposal>, String> {
    crate::blocking(move || -> Result<Vec<stemma::api::LineProposal>, String> {
        let line = LineAt::admit(&path)?;
        let session = Session::open()?;
        session
            .server
            .line_proposals(&session.token, &line.group, &line.side, &line.name)
            .map_err(failed)
    })
    .await
}

#[cfg(test)]
mod tests {
    use stemma_core::document::{Block, Document};

    use super::merged;

    fn line(blocks: &[&str]) -> Document {
        blocks
            .iter()
            .fold(Document::new("A line", None), |document, text| {
                document.with(Block::prose(*text))
            })
    }

    fn rewrite(block: &mut Block, text: &str) {
        let Block::Prose(prose) = block else {
            panic!("only prose is rewritten here");
        };
        prose.content = text.to_owned();
    }

    fn contents(document: &Document) -> Vec<&str> {
        document
            .blocks
            .iter()
            .map(|block| block.content())
            .collect()
    }

    #[test]
    fn a_proposal_on_a_line_that_did_not_move_is_taken_whole() {
        let stands = line(&["One.", "Two."]);
        let mut offered = stands.clone();
        rewrite(&mut offered.blocks[1], "Two, rewritten.");

        let (document, outdated) = merged(
            &stands.to_text(),
            Some(&stands.to_text()),
            &offered.to_text(),
        )
        .unwrap();

        assert_eq!(contents(&document.unwrap()), ["One.", "Two, rewritten."]);
        assert!(outdated.is_empty());
    }

    #[test]
    fn a_block_changed_since_keeps_the_current_version_and_is_outdated() {
        let stands = line(&["One.", "Two."]);
        let mut current = stands.clone();
        rewrite(&mut current.blocks[1], "Two, written here.");
        let mut offered = stands.clone();
        rewrite(&mut offered.blocks[0], "One, rewritten.");
        rewrite(&mut offered.blocks[1], "Two, by the agent.");

        let (document, outdated) = merged(
            &stands.to_text(),
            Some(&current.to_text()),
            &offered.to_text(),
        )
        .unwrap();

        assert_eq!(
            contents(&document.unwrap()),
            ["One, rewritten.", "Two, written here."]
        );
        assert_eq!(outdated, [current.blocks[1].id().to_string()]);
    }

    #[test]
    fn without_a_standing_source_the_proposal_is_read_as_it_is() {
        let offered = line(&["New."]);

        let (document, outdated) = merged("", Some("anything"), &offered.to_text()).unwrap();

        assert_eq!(contents(&document.unwrap()), ["New."]);
        assert!(outdated.is_empty());
    }
}

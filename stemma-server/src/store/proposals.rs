//! Proposed source and the human decision it awaits.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use stemma_core::id::{JobId, ProposalId, UserId};
use stemma_core::proposal::{FormalReview, LibraryChange, Offer, Operation};

use super::jobs::Job;

#[derive(Debug, Clone)]
/// One proposed replacement, joined to the job that produced it.
pub struct Proposal {
    /// Its identifier.
    pub id: ProposalId,
    /// The producing job.
    pub job: Job,
    /// Complete proposed source.
    pub source: String,
    /// Complete proposed Lean module, when the proposal formalizes work.
    pub lean_source: Option<String>,
    /// Per-file change to the Lake package, when one was proposed.
    pub library_change: Option<LibraryChange>,
    /// Lean-produced evidence for reviewing the library change.
    pub formal_review: Option<FormalReview>,
    /// The commit that change became after approval.
    pub library_commit: Option<String>,
    /// Agent vendor.
    pub vendor: String,
    /// Concrete model.
    pub model: String,
    /// Prompt contract revision.
    pub prompt_revision: String,
    /// Narrow operations submitted through the job API.
    pub operations: Vec<Operation>,
    /// The agent's summary for the reviewer.
    pub explanation: String,
    /// Selected reasoning effort.
    pub effort: Option<String>,
    /// Current review state.
    pub state: String,
    /// The person who decided, once decided.
    pub decided_by: Option<UserId>,
    /// The member currently asked to review, or the launcher until handed off.
    pub reviewer: Option<UserId>,
    /// The choice made, including whether a rejection asked for a new attempt.
    pub decision: Option<String>,
    /// The reviewer's optional reason or instruction for the next attempt.
    pub note: Option<String>,
    /// The revision it became on its line, once a machine applied it.
    pub applied_revision: Option<u64>,
}

/// Keeps a validated proposal once per job.
pub fn create(connection: &Connection, job: &Job, offer: &Offer) -> Result<ProposalId> {
    let id = ProposalId::generate();
    let library_change = offer
        .library_change
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let formal_review = offer
        .formal_review
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let operations = serde_json::to_string(&offer.operations)?;
    connection
        .execute(
            "INSERT INTO proposals (id, job_id, source, lean_source, library_change, formal_review, vendor, model, prompt_revision, operations, explanation, effort)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id.to_string(),
                job.id.to_string(),
                offer.source,
                offer.lean_source,
                library_change,
                formal_review,
                offer.vendor,
                offer.model,
                offer.prompt_revision,
                operations,
                offer.explanation,
                offer.effort,
            ],
        )
        .context("cannot keep the proposal")?;
    tell(connection, &id)?;
    Ok(id)
}

/// Says a proposal arrived or was decided, to the person it waits for.
fn tell(connection: &Connection, id: &ProposalId) -> Result<()> {
    let Some(proposal) = by_id(connection, id)? else {
        return Ok(());
    };

    super::events::publish(
        connection,
        super::events::Audience::Person(&proposal.job.group, proposal.reviewer()),
        &stemma_core::event::Event::Proposal {
            group: proposal.job.group.slug().as_str().to_owned(),
            proposal: id.to_string(),
            state: proposal.state.as_str().to_owned(),
        },
    )?;

    Ok(())
}

/// Finds one proposal and its job.
pub fn by_id(connection: &Connection, id: &ProposalId) -> Result<Option<Proposal>> {
    let row = connection
        .query_row(
            "SELECT job_id, source, lean_source, library_change, formal_review, library_commit, vendor, model,
                prompt_revision, operations, explanation, effort, state, decided_by, applied_revision,
                reviewer, decision, note
         FROM proposals WHERE id = ?1",
            params![id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<i64>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, Option<String>>(16)?,
                    row.get::<_, Option<String>>(17)?,
                ))
            },
        )
        .optional()?;
    let Some((
        job,
        source,
        lean_source,
        library_change,
        formal_review,
        library_commit,
        vendor,
        model,
        prompt_revision,
        operations,
        explanation,
        effort,
        state,
        decided_by,
        applied_revision,
        reviewer,
        decision,
        note,
    )) = row
    else {
        return Ok(None);
    };
    let job: JobId = job
        .parse()
        .map_err(|error| anyhow::anyhow!("invalid stored job: {error}"))?;
    let Some(job) = super::jobs::by_id(connection, &job)? else {
        return Ok(None);
    };
    Ok(Some(Proposal {
        id: *id,
        job,
        source,
        lean_source,
        library_change: library_change
            .map(|change| serde_json::from_str(&change))
            .transpose()
            .context("cannot read the proposed library change")?,
        formal_review: formal_review
            .map(|review| serde_json::from_str(&review))
            .transpose()
            .context("cannot read the formal review evidence")?,
        library_commit,
        vendor,
        model,
        prompt_revision,
        operations: serde_json::from_str(&operations)
            .context("cannot read the proposal operations")?,
        explanation,
        effort,
        state,
        decided_by: decided_by.and_then(|id| id.parse().ok()),
        applied_revision: applied_revision.map(|revision| revision.max(0) as u64),
        reviewer: reviewer.and_then(|id| id.parse().ok()),
        decision,
        note,
    }))
}

impl Proposal {
    /// Who is responsible for a pending review.
    pub fn reviewer(&self) -> &UserId {
        self.reviewer.as_ref().unwrap_or(&self.job.launched_by)
    }
}

/// The launcher hands a pending shared-line review to another group member.
pub fn handoff(connection: &Connection, id: &ProposalId, by: &UserId, to: &UserId) -> Result<bool> {
    let Some(proposal) = by_id(connection, id)? else {
        return Ok(false);
    };
    if proposal.state != "awaiting_review"
        || proposal.job.launched_by != *by
        || proposal.job.side != stemma_core::repository::Side::Shared
        || super::groups::role_of(connection, &proposal.job.group, to)?.is_none()
        || super::users::by_id(connection, to)?.is_none_or(|member| member.suspended_at.is_some())
    {
        return Ok(false);
    }
    tell(connection, id)?;
    connection.execute(
        "UPDATE proposals SET reviewer = ?2 WHERE id = ?1 AND state = 'awaiting_review'",
        params![id.to_string(), to.to_string()],
    )?;
    tell(connection, id)?;
    Ok(true)
}

/// Records the library commit made by accepting this proposal.
pub fn library_applied(connection: &Connection, id: &ProposalId, commit: &str) -> Result<()> {
    connection.execute(
        "UPDATE proposals SET library_commit = ?2 WHERE id = ?1 AND library_commit IS NULL",
        params![id.to_string(), commit],
    )?;
    Ok(())
}

/// Records the launcher's first and only decision.
pub fn decide(
    connection: &Connection,
    id: &ProposalId,
    by: &UserId,
    decision: &str,
    note: Option<&str>,
) -> Result<bool> {
    let state = if decision == "accept" {
        "accepted"
    } else {
        "rejected"
    };
    let decided = connection.execute(
        "UPDATE proposals SET state = ?3, decided_by = ?2, decided_at = datetime('now'), decision = ?4, note = ?5
         WHERE id = ?1 AND state = 'awaiting_review'
           AND COALESCE(reviewer, (SELECT launched_by FROM jobs WHERE id = job_id)) = ?2",
        params![id.to_string(), by.to_string(), state, decision, note],
    )? > 0;

    if decided {
        tell(connection, id)?;
    }

    Ok(decided)
}

/// Proposals this person accepted that no machine of theirs has applied
/// yet, oldest first.
pub fn accepted_unapplied(connection: &Connection, by: &UserId) -> Result<Vec<Proposal>> {
    let mut statement = connection.prepare(
        "SELECT id FROM proposals
          WHERE state = 'accepted' AND decided_by = ?1 AND applied_revision IS NULL
          ORDER BY decided_at, id",
    )?;
    let ids = statement
        .query_map(params![by.to_string()], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut found = Vec::new();
    for id in ids {
        if let Some(proposal) = by_id(connection, &id.parse()?)? {
            found.push(proposal);
        }
    }

    Ok(found)
}

/// Notes the revision an accepted proposal became on its line.
pub fn applied(
    connection: &Connection,
    id: &ProposalId,
    by: &UserId,
    revision: u64,
) -> Result<bool> {
    let applied = connection.execute(
        "UPDATE proposals SET applied_revision = ?3
          WHERE id = ?1 AND decided_by = ?2 AND state = 'accepted' AND applied_revision IS NULL",
        params![id.to_string(), by.to_string(), revision as i64],
    )? > 0;

    if applied {
        tell(connection, id)?;
    }

    Ok(applied)
}

/// Notes that an accepted proposal touches blocks that changed since the
/// revision it was made from, so it is not applied.
pub fn outdated(connection: &Connection, id: &ProposalId, by: &UserId) -> Result<bool> {
    let outdated = connection.execute(
        "UPDATE proposals SET state = 'outdated'
          WHERE id = ?1 AND decided_by = ?2 AND state = 'accepted' AND applied_revision IS NULL",
        params![id.to_string(), by.to_string()],
    )? > 0;

    if outdated {
        tell(connection, id)?;
    }

    Ok(outdated)
}

/// Proposals this person still has to review.
pub fn awaiting(connection: &Connection, by: &UserId) -> Result<Vec<Proposal>> {
    let mut statement = connection.prepare(
        "SELECT id FROM proposals WHERE state = 'awaiting_review'
         AND COALESCE(reviewer, (SELECT launched_by FROM jobs WHERE id = job_id)) = ?1 ORDER BY id",
    )?;
    let ids = statement
        .query_map(params![by.to_string()], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter()
        .map(|id| {
            let id = id
                .parse()
                .map_err(|error| anyhow::anyhow!("invalid stored proposal: {error}"))?;
            by_id(connection, &id)?.context("the proposal disappeared")
        })
        .collect()
}

/// Recently decided proposals this person reviewed, newest first.
pub fn recent(connection: &Connection, by: &UserId) -> Result<Vec<Proposal>> {
    let mut statement = connection.prepare(
        "SELECT id FROM proposals WHERE decided_by = ?1 ORDER BY decided_at DESC, id DESC LIMIT 25",
    )?;
    let ids = statement
        .query_map(params![by.to_string()], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter()
        .map(|id| by_id(connection, &id.parse()?)?.context("the proposal disappeared"))
        .collect()
}

/// Every proposal made for a line, including decisions, oldest first.
pub fn for_line(connection: &Connection, line: &stemma_core::id::LineId) -> Result<Vec<Proposal>> {
    let mut statement = connection.prepare(
        "SELECT p.id FROM proposals p JOIN jobs j ON j.id = p.job_id
         WHERE j.line_id = ?1 ORDER BY p.id",
    )?;
    let ids = statement
        .query_map(params![line.to_string()], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter()
        .map(|id| by_id(connection, &id.parse()?)?.context("the proposal disappeared"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups, jobs, test_database, users};
    use stemma_core::access::GroupRole;
    use stemma_core::id::Slug;
    use stemma_core::job::{Choice, Task};
    use stemma_core::line::Name;
    use stemma_core::repository::Side;

    #[test]
    fn accepting_is_a_single_human_decision_and_the_offer_stays_attributed() {
        let mut database = test_database();
        let connection = database.connection_mut();
        let ana = users::create(
            connection,
            &Slug::parse("ana").unwrap(),
            "ana@example.org",
            "Ana",
            "hash",
        )
        .unwrap()
        .unwrap();
        let group = groups::create(
            connection,
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &ana.id,
        )
        .unwrap();
        let job = jobs::launch(
            connection,
            &jobs::Launch {
                group: group.id,
                side: Side::Shared,
                line: Name::parse("quotients").unwrap(),
                line_id: None,
                revision: 1,
                against: None,
                task: Task::Explore,
                agent: Slug::parse("claude").unwrap(),
                model: None,
                effort: None,
                request: None,
                grants: None,
                choice: Choice::Anyone,
                by: ana.id.clone(),
            },
        )
        .unwrap();
        let id = create(
            connection,
            &job,
            &Offer {
                source: "= A proposal".to_owned(),
                lean_source: None,
                library_change: None,
                formal_review: None,
                vendor: "Anthropic".to_owned(),
                model: "claude-opus-5".to_owned(),
                prompt_revision: "1".to_owned(),
                operations: Vec::new(),
                explanation: String::new(),
                effort: None,
            },
        )
        .unwrap();

        assert_eq!(awaiting(connection, &ana.id).unwrap().len(), 1);
        assert!(decide(connection, &id, &ana.id, "accept", None).unwrap());
        assert!(!decide(connection, &id, &ana.id, "reject", Some("too late")).unwrap());
        let accepted = by_id(connection, &id).unwrap().unwrap();
        assert_eq!(accepted.state, "accepted");
        assert_eq!(accepted.model, "claude-opus-5");
        assert_eq!(accepted.decided_by, Some(ana.id));
        assert_eq!(accepted.decision.as_deref(), Some("accept"));
        assert!(accepted.note.is_none());
        assert!(
            awaiting(connection, accepted.decided_by.as_ref().unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_shared_review_can_be_handed_to_a_member_and_keeps_their_note() {
        let mut database = test_database();
        let connection = database.connection_mut();
        let ana = users::create(
            connection,
            &Slug::parse("ana").unwrap(),
            "ana@example.org",
            "Ana",
            "hash",
        )
        .unwrap()
        .unwrap();
        let bob = users::create(
            connection,
            &Slug::parse("bob").unwrap(),
            "bob@example.org",
            "Bob",
            "hash",
        )
        .unwrap()
        .unwrap();
        let outsider = users::create(
            connection,
            &Slug::parse("eve").unwrap(),
            "eve@example.org",
            "Eve",
            "hash",
        )
        .unwrap()
        .unwrap();
        let group = groups::create(
            connection,
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &ana.id,
        )
        .unwrap();
        groups::add_member(connection, &group.id, &bob.id, GroupRole::Member).unwrap();
        let job = jobs::launch(
            connection,
            &jobs::Launch {
                group: group.id,
                side: Side::Shared,
                line: Name::parse("quotients").unwrap(),
                line_id: None,
                revision: 1,
                against: None,
                task: Task::Explore,
                agent: Slug::parse("claude").unwrap(),
                model: None,
                effort: None,
                request: None,
                grants: None,
                choice: Choice::Anyone,
                by: ana.id.clone(),
            },
        )
        .unwrap();
        let id = create(
            connection,
            &job,
            &Offer {
                source: "= A proposal".to_owned(),
                lean_source: None,
                library_change: None,
                formal_review: None,
                vendor: "Anthropic".to_owned(),
                model: "claude-opus-5".to_owned(),
                prompt_revision: "1".to_owned(),
                operations: Vec::new(),
                explanation: "A reason".to_owned(),
                effort: None,
            },
        )
        .unwrap();

        assert!(!handoff(connection, &id, &outsider.id, &bob.id).unwrap());
        assert!(!handoff(connection, &id, &ana.id, &outsider.id).unwrap());
        assert!(handoff(connection, &id, &ana.id, &bob.id).unwrap());
        assert!(awaiting(connection, &ana.id).unwrap().is_empty());
        assert_eq!(awaiting(connection, &bob.id).unwrap().len(), 1);
        assert!(!decide(connection, &id, &ana.id, "accept", None).unwrap());
        assert!(
            decide(
                connection,
                &id,
                &bob.id,
                "ask_again",
                Some("Try the other lemma.")
            )
            .unwrap()
        );
        let decided = by_id(connection, &id).unwrap().unwrap();
        assert_eq!(decided.note.as_deref(), Some("Try the other lemma."));
        assert_eq!(decided.decision.as_deref(), Some("ask_again"));
        assert_eq!(recent(connection, &bob.id).unwrap().len(), 1);
    }
}

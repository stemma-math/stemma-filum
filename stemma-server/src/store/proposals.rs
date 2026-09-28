//! Proposed source and the human decision it awaits.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use stemma_core::id::{JobId, ProposalId, UserId};
use stemma_core::proposal::{FormalReview, LibraryChange, Offer};

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
    /// Current review state.
    pub state: String,
    /// The person who decided, once decided.
    pub decided_by: Option<UserId>,
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
    connection
        .execute(
            "INSERT INTO proposals (id, job_id, source, lean_source, library_change, formal_review, vendor, model, prompt_revision)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id.to_string(),
                job.id.to_string(),
                offer.source,
                offer.lean_source,
                library_change,
                formal_review,
                offer.vendor,
                offer.model,
                offer.prompt_revision
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
        super::events::Audience::Person(&proposal.job.group, &proposal.job.launched_by),
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
                prompt_revision, state, decided_by, applied_revision
         FROM proposals WHERE id = ?1",
            params![id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
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
        state,
        decided_by,
        applied_revision,
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
        state,
        decided_by: decided_by.and_then(|id| id.parse().ok()),
        applied_revision: applied_revision.map(|revision| revision.max(0) as u64),
    }))
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
pub fn decide(connection: &Connection, id: &ProposalId, by: &UserId, accept: bool) -> Result<bool> {
    let state = if accept { "accepted" } else { "rejected" };
    let decided = connection.execute(
        "UPDATE proposals SET state = ?3, decided_by = ?2, decided_at = datetime('now')
         WHERE id = ?1 AND state = 'awaiting_review'
           AND job_id IN (SELECT id FROM jobs WHERE launched_by = ?2)",
        params![id.to_string(), by.to_string(), state],
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
         AND job_id IN (SELECT id FROM jobs WHERE launched_by = ?1) ORDER BY id",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups, jobs, test_database, users};
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
            },
        )
        .unwrap();

        assert_eq!(awaiting(connection, &ana.id).unwrap().len(), 1);
        assert!(decide(connection, &id, &ana.id, true).unwrap());
        assert!(!decide(connection, &id, &ana.id, false).unwrap());
        let accepted = by_id(connection, &id).unwrap().unwrap();
        assert_eq!(accepted.state, "accepted");
        assert_eq!(accepted.model, "claude-opus-5");
        assert_eq!(accepted.decided_by, Some(ana.id));
        assert!(
            awaiting(connection, accepted.decided_by.as_ref().unwrap())
                .unwrap()
                .is_empty()
        );
    }
}

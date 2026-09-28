//! Human correspondence audits for central formal statements.
//!
//! An audit records what one person compared: the exact prose block, Lean's
//! printed declaration data, and the agent's plain-language reading. It is
//! withdrawn automatically when either side of that comparison changes.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use stemma_core::artefact::BlockCheck;
use stemma_core::document::Document;
use stemma_core::id::{AuditId, BlockId, LineId, UserId};

use super::proposals::Proposal;

/// The current correspondence record shown beside a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormalAudit {
    /// Stable audit identifier.
    pub id: AuditId,
    /// Stable block compared.
    pub block: BlockId,
    /// Line revision the reviewer saw.
    pub revision: u64,
    /// Exact serialized prose block the reviewer saw.
    pub block_source: String,
    /// Lean evidence the reviewer saw.
    pub check: BlockCheck,
    /// The proposing agent's prose reading.
    pub reading: String,
    /// `audited` or `audit_withdrawn`.
    pub state: String,
    /// Why an automatic withdrawal happened.
    pub reason: Option<String>,
    /// Person who accepted the correspondence.
    pub audited_by: UserId,
    /// When it was accepted.
    pub audited_at: String,
}

/// Records audits for verified central blocks in an accepted proposal.
pub fn accept(connection: &Connection, proposal: &Proposal, by: &UserId) -> Result<Vec<AuditId>> {
    let Some(line) = proposal.job.line_id else {
        return Ok(Vec::new());
    };
    let Some(review) = &proposal.formal_review else {
        return Ok(Vec::new());
    };
    let document = Document::parse(&proposal.source).context("the audited line is unreadable")?;
    let mut recorded = Vec::new();

    for reviewed in &review.blocks {
        let Some(block) = document.block(&reviewed.check.block) else {
            continue;
        };
        let Some(statement) = block.statement().filter(|statement| statement.central) else {
            continue;
        };
        if reviewed.reading.trim().is_empty() || reviewed.check.declarations.is_empty() {
            continue;
        }

        connection.execute(
            "UPDATE formal_audits SET state = 'audit_withdrawn',
                    withdrawn_reason = 'a newer audit replaced it', withdrawn_at = datetime('now')
              WHERE line_id = ?1 AND block_id = ?2 AND state = 'audited'",
            params![line.to_string(), reviewed.check.block.to_string()],
        )?;
        let id = AuditId::generate();
        connection.execute(
            "INSERT INTO formal_audits
                (id, proposal_id, line_id, block_id, revision, block_source,
                 check_result, reading, audited_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id.to_string(),
                proposal.id.to_string(),
                line.to_string(),
                reviewed.check.block.to_string(),
                proposal.job.revision as i64,
                serde_json::to_string(block)?,
                serde_json::to_string(&reviewed.check)?,
                reviewed.reading.trim(),
                by.to_string(),
            ],
        )?;
        let _ = statement;
        recorded.push(id);
    }
    Ok(recorded)
}

/// Withdraws active audits whose exact prose block changed or disappeared.
pub fn withdraw_changed_prose(
    connection: &Connection,
    line: &LineId,
    source: &str,
) -> Result<Vec<BlockId>> {
    let document = Document::parse(source).context("the written line is unreadable")?;
    let active = active_rows(connection, line)?;
    let mut withdrawn = Vec::new();
    for (id, block, expected) in active {
        let standing = document
            .block(&block)
            .map(serde_json::to_string)
            .transpose()?;
        if standing.as_deref() != Some(expected.as_str()) {
            withdraw(connection, &id, "the block prose changed")?;
            withdrawn.push(block);
        }
    }
    Ok(withdrawn)
}

/// Withdraws an active audit if a published build changed the transitive
/// statement closure or the set of declarations carrying the block mark.
pub fn reconcile_check(connection: &Connection, line: &LineId, check: &BlockCheck) -> Result<bool> {
    let found: Option<(String, String)> = connection
        .query_row(
            "SELECT id, check_result FROM formal_audits
              WHERE line_id = ?1 AND block_id = ?2 AND state = 'audited'
              ORDER BY audited_at DESC, rowid DESC LIMIT 1",
            params![line.to_string(), check.block.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((id, expected)) = found else {
        return Ok(false);
    };
    let expected: BlockCheck =
        serde_json::from_str(&expected).context("cannot read the stored formal audit")?;
    if closure_surface(&expected) == closure_surface(check) {
        return Ok(false);
    }
    withdraw(connection, &id, "the Lean statement closure changed")?;
    Ok(true)
}

/// Current formal correspondence records for a line, newest block first.
pub fn for_line(connection: &Connection, line: &LineId) -> Result<Vec<FormalAudit>> {
    let mut statement = connection.prepare(
        "SELECT id, block_id, revision, block_source, check_result, reading, state,
                withdrawn_reason, audited_by, audited_at
           FROM formal_audits WHERE line_id = ?1
          ORDER BY audited_at DESC, rowid DESC",
    )?;
    let mut seen = BTreeSet::new();
    let rows = statement.query_map([line.to_string()], |row| {
        let id: String = row.get(0)?;
        let block: String = row.get(1)?;
        let check: String = row.get(4)?;
        Ok((
            id,
            block,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            check,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
        ))
    })?;
    let mut audits = Vec::new();
    for row in rows {
        let (id, block, revision, block_source, check, reading, state, reason, by, at) = row?;
        let block: BlockId = block.parse()?;
        if !seen.insert(block) {
            continue;
        }
        audits.push(FormalAudit {
            id: id.parse()?,
            block,
            revision: revision.max(0) as u64,
            block_source,
            check: serde_json::from_str(&check)?,
            reading,
            state,
            reason,
            audited_by: by.parse()?,
            audited_at: at,
        });
    }
    Ok(audits)
}

fn active_rows(connection: &Connection, line: &LineId) -> Result<Vec<(String, BlockId, String)>> {
    let mut statement = connection.prepare(
        "SELECT id, block_id, block_source FROM formal_audits
          WHERE line_id = ?1 AND state = 'audited'",
    )?;
    statement
        .query_map([line.to_string()], |row| {
            let block: String = row.get(1)?;
            let block = block.parse().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok((row.get(0)?, block, row.get(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn withdraw(connection: &Connection, id: &str, reason: &str) -> Result<()> {
    connection.execute(
        "UPDATE formal_audits SET state = 'audit_withdrawn',
                withdrawn_reason = ?2, withdrawn_at = datetime('now')
          WHERE id = ?1 AND state = 'audited'",
        params![id, reason],
    )?;
    Ok(())
}

fn closure_surface(check: &BlockCheck) -> Vec<(&str, &str)> {
    check
        .declarations
        .iter()
        .map(|declaration| {
            (
                declaration.name.as_str(),
                declaration.statement_closure.as_str(),
            )
        })
        .collect()
}

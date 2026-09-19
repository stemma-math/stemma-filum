//! Immutable vault objects and the records and obligations around their audits.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use stemma_core::id::{AuditId, BridgeId, DecisionId, GroupId, ObligationId, UserId, VaultEntryId};
use stemma_core::model::Correspondence;
use stemma_core::vault::{
    AuditAction, Bridge, BridgeKind, BridgeOrigin, DownstreamDecision, Entry,
};

/// A vault entry with its permanent admission record.
#[derive(Debug, Clone)]
pub struct StoredEntry {
    /// Immutable mathematical object.
    pub entry: Entry,
    /// Person who admitted it after validation.
    pub admitted_by: UserId,
    /// When it entered the vault.
    pub admitted_at: String,
}

/// A bridge with its permanent creation record.
#[derive(Debug, Clone)]
pub struct StoredBridge {
    /// Proved bridge object.
    pub bridge: Bridge,
    /// Group whose vault holds it.
    pub group: GroupId,
    /// Person who admitted or requested its derivation.
    pub created_by: UserId,
    /// When it was created.
    pub created_at: String,
}

/// An outstanding downstream decision.
#[derive(Debug, Clone)]
pub struct Obligation {
    /// Stable obligation identity.
    pub id: String,
    /// Group in which the affected work lives.
    pub group: GroupId,
    /// Withdrawn bridge responsible for it.
    pub bridge: BridgeId,
    /// Affected statement siglum.
    pub statement: String,
    /// Person who owes the decision.
    pub assigned_to: UserId,
    /// When it arose.
    pub created_at: String,
}

/// Admits a validated definition once. There is deliberately no update path.
pub fn admit(connection: &Connection, entry: &Entry, by: &UserId) -> Result<()> {
    connection
        .execute(
            "INSERT INTO vault_entries
         (id, group_id, siglum, name, prose, formalization, correspondence, admitted_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.id.to_string(),
                entry.group.to_string(),
                entry.siglum.to_string(),
                entry.name,
                entry.prose,
                entry.formalization,
                correspondence(entry.correspondence),
                by.to_string()
            ],
        )
        .context("cannot admit the definition to the vault")?;
    Ok(())
}

/// Every usable or historically relevant definition in a group.
pub fn entries(connection: &Connection, group: &GroupId) -> Result<Vec<StoredEntry>> {
    let mut query = connection.prepare(
        "SELECT id, siglum, name, prose, formalization, correspondence, admitted_by, admitted_at
         FROM vault_entries WHERE group_id = ?1 ORDER BY name, id",
    )?;
    query
        .query_map([group.to_string()], |row| {
            let correspondence: String = row.get(5)?;
            Ok(StoredEntry {
                entry: Entry {
                    id: id(row.get::<_, String>(0)?)?,
                    group: group.clone(),
                    siglum: id(row.get::<_, String>(1)?)?,
                    name: row.get(2)?,
                    prose: row.get(3)?,
                    formalization: row.get(4)?,
                    correspondence: read_correspondence(&correspondence)?,
                },
                admitted_by: id(row.get::<_, String>(6)?)?,
                admitted_at: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the vault")
}

/// Finds one entry.
pub fn entry(connection: &Connection, target: &VaultEntryId) -> Result<Option<StoredEntry>> {
    let group: Option<String> = connection
        .query_row(
            "SELECT group_id FROM vault_entries WHERE id = ?1",
            [target.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(group) = group else { return Ok(None) };
    let group: GroupId = group.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(entries(connection, &group)?
        .into_iter()
        .find(|stored| stored.entry.id == *target))
}

/// Admits a direct or derived bridge once.
pub fn admit_bridge(
    connection: &Connection,
    group: &GroupId,
    bridge: &Bridge,
    by: &UserId,
) -> Result<()> {
    let (origin, first, second) = match &bridge.origin {
        BridgeOrigin::Direct => ("direct", None, None),
        BridgeOrigin::Derived { parents } => (
            "derived",
            Some(parents[0].to_string()),
            Some(parents[1].to_string()),
        ),
    };
    connection
        .execute(
            "INSERT INTO bridges
         (id, group_id, left_entry, right_entry, kind, theorem, origin, first_parent,
          second_parent, correspondence, created_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                bridge.id.to_string(),
                group.to_string(),
                bridge.left.to_string(),
                bridge.right.to_string(),
                bridge_kind(bridge.kind),
                bridge.theorem,
                origin,
                first,
                second,
                correspondence(bridge.correspondence),
                by.to_string()
            ],
        )
        .context("cannot admit the bridge")?;
    Ok(())
}

/// Every bridge in a group's vault.
pub fn bridges(connection: &Connection, group: &GroupId) -> Result<Vec<StoredBridge>> {
    let mut query = connection.prepare(
        "SELECT id, left_entry, right_entry, kind, theorem, origin, first_parent,
                second_parent, correspondence, created_by, created_at
         FROM bridges WHERE group_id = ?1 ORDER BY created_at, id",
    )?;
    query
        .query_map([group.to_string()], |row| {
            let origin: String = row.get(5)?;
            let first: Option<String> = row.get(6)?;
            let second: Option<String> = row.get(7)?;
            let origin = if origin == "direct" {
                BridgeOrigin::Direct
            } else {
                BridgeOrigin::Derived {
                    parents: [
                        id(first.ok_or_else(|| rusqlite::Error::InvalidQuery)?)?,
                        id(second.ok_or_else(|| rusqlite::Error::InvalidQuery)?)?,
                    ],
                }
            };
            let kind: String = row.get(3)?;
            let state: String = row.get(8)?;
            Ok(StoredBridge {
                bridge: Bridge {
                    id: id(row.get::<_, String>(0)?)?,
                    left: id(row.get::<_, String>(1)?)?,
                    right: id(row.get::<_, String>(2)?)?,
                    kind: read_bridge_kind(&kind)?,
                    theorem: row.get(4)?,
                    origin,
                    correspondence: read_correspondence(&state)?,
                },
                group: group.clone(),
                created_by: id(row.get::<_, String>(9)?)?,
                created_at: row.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list bridges")
}

/// Records an attestation and makes the target currently audited.
pub fn attest(
    connection: &mut Connection,
    target_kind: &str,
    target: &str,
    by: &UserId,
    note: &str,
) -> Result<AuditId> {
    audit(
        connection,
        target_kind,
        target,
        by,
        note,
        AuditAction::Attested,
    )
}

/// Records a withdrawal and propagates it through every derived bridge.
pub fn withdraw(
    connection: &mut Connection,
    target_kind: &str,
    target: &str,
    by: &UserId,
    note: &str,
) -> Result<AuditId> {
    audit(
        connection,
        target_kind,
        target,
        by,
        note,
        AuditAction::Withdrawn,
    )
}

fn audit(
    connection: &mut Connection,
    target_kind: &str,
    target: &str,
    by: &UserId,
    note: &str,
    action: AuditAction,
) -> Result<AuditId> {
    if !matches!(target_kind, "entry" | "bridge") || note.trim().is_empty() {
        bail!("an audit must name a vault object and say what was checked")
    }
    let transaction = connection.transaction()?;
    let table = if target_kind == "entry" {
        "vault_entries"
    } else {
        "bridges"
    };
    let state = match action {
        AuditAction::Attested => "audited",
        AuditAction::Withdrawn => "audit_withdrawn",
    };
    let changed = transaction.execute(
        &format!("UPDATE {table} SET correspondence = ?2 WHERE id = ?1"),
        params![target, state],
    )?;
    if changed == 0 {
        bail!("the audit target does not exist")
    }
    let audit = AuditId::generate();
    let word = match action {
        AuditAction::Attested => "attested",
        AuditAction::Withdrawn => "withdrawn",
    };
    transaction.execute(
        "INSERT INTO audits (id, target_kind, target_id, action, by_user, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            audit.to_string(),
            target_kind,
            target,
            word,
            by.to_string(),
            note.trim()
        ],
    )?;
    if action == AuditAction::Withdrawn && target_kind == "bridge" {
        transaction.execute(
            "WITH RECURSIVE descendants(id) AS (
               SELECT id FROM bridges WHERE first_parent = ?1 OR second_parent = ?1
               UNION SELECT b.id FROM bridges b JOIN descendants d
                 ON b.first_parent = d.id OR b.second_parent = d.id
             ) UPDATE bridges SET correspondence = 'audit_withdrawn'
               WHERE id IN (SELECT id FROM descendants)",
            [target],
        )?;
    }
    transaction.commit()?;
    Ok(audit)
}

/// Assigns one counted downstream decision after a withdrawal.
pub fn assign(
    connection: &Connection,
    group: &GroupId,
    bridge: &BridgeId,
    statement: &str,
    to: &UserId,
) -> Result<String> {
    let id = ObligationId::generate().to_string();
    connection.execute(
        "INSERT OR IGNORE INTO audit_obligations
         (id, group_id, bridge_id, statement, assigned_to) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id,
            group.to_string(),
            bridge.to_string(),
            statement,
            to.to_string()
        ],
    )?;
    Ok(id)
}

/// Outstanding decisions assigned to one person.
pub fn obligations(connection: &Connection, user: &UserId) -> Result<Vec<Obligation>> {
    let mut query = connection.prepare(
        "SELECT id, group_id, bridge_id, statement, assigned_to, created_at
         FROM audit_obligations WHERE assigned_to = ?1 AND decided_at IS NULL ORDER BY created_at",
    )?;
    query
        .query_map([user.to_string()], |row| {
            Ok(Obligation {
                id: row.get(0)?,
                group: id(row.get::<_, String>(1)?)?,
                bridge: id(row.get::<_, String>(2)?)?,
                statement: row.get(3)?,
                assigned_to: id(row.get::<_, String>(4)?)?,
                created_at: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list audit decisions")
}

/// Records the assignee's decision once; the withdrawn condition persists.
pub fn decide(
    connection: &mut Connection,
    obligation: &str,
    by: &UserId,
    decision: DownstreamDecision,
    note: &str,
) -> Result<bool> {
    if note.trim().is_empty() {
        bail!("a signed decision needs a note")
    }
    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE audit_obligations SET decided_at = datetime('now')
         WHERE id = ?1 AND assigned_to = ?2 AND decided_at IS NULL",
        params![obligation, by.to_string()],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    let id = DecisionId::generate();
    transaction.execute(
        "INSERT INTO audit_decisions (id, obligation_id, decision, by_user, note)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.to_string(),
            obligation,
            decision_word(decision),
            by.to_string(),
            note.trim()
        ],
    )?;
    transaction.commit()?;
    Ok(true)
}

fn id<T: std::str::FromStr>(written: String) -> rusqlite::Result<T> {
    written.parse().map_err(|_| rusqlite::Error::InvalidQuery)
}
fn correspondence(value: Correspondence) -> &'static str {
    match value {
        Correspondence::NotAudited => "not_audited",
        Correspondence::Audited => "audited",
        Correspondence::AuditWithdrawn => "audit_withdrawn",
    }
}
fn read_correspondence(value: &str) -> rusqlite::Result<Correspondence> {
    match value {
        "not_audited" => Ok(Correspondence::NotAudited),
        "audited" => Ok(Correspondence::Audited),
        "audit_withdrawn" => Ok(Correspondence::AuditWithdrawn),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}
fn bridge_kind(value: BridgeKind) -> &'static str {
    match value {
        BridgeKind::Predicate => "predicate",
        BridgeKind::Structure => "structure",
    }
}
fn read_bridge_kind(value: &str) -> rusqlite::Result<BridgeKind> {
    match value {
        "predicate" => Ok(BridgeKind::Predicate),
        "structure" => Ok(BridgeKind::Structure),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}
fn decision_word(value: DownstreamDecision) -> &'static str {
    match value {
        DownstreamDecision::Stands => "stands",
        DownstreamDecision::Rewrite => "rewrite",
        DownstreamDecision::WaitingForAudit => "waiting_for_audit",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups, test_database, users};
    use stemma_core::id::Slug;

    fn setting() -> (crate::db::Database, UserId, GroupId) {
        let mut database = test_database();
        let connection = database.connection_mut();
        let user = users::create(
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
            &user.id,
        )
        .unwrap();
        (database, user.id, group.id)
    }

    fn definition(group: &GroupId, siglum: &str, name: &str) -> Entry {
        Entry::admit(
            group.clone(),
            siglum.parse().unwrap(),
            name,
            format!("{name} in prose"),
            format!("def {} := Unit", name.replace(' ', "")),
        )
        .unwrap()
    }

    #[test]
    fn admitted_mathematics_has_no_update_path() {
        let (mut database, user, group) = setting();
        let connection = database.connection_mut();
        let entry = definition(&group, "def:additive", "Additive group");
        admit(connection, &entry, &user).unwrap();

        assert!(admit(connection, &entry, &user).is_err());
        let stored = entries(connection, &group).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].entry.prose, "Additive group in prose");
    }

    #[test]
    fn withdrawing_a_parent_reaches_every_derived_bridge() {
        let (mut database, user, group) = setting();
        let connection = database.connection_mut();
        let one = definition(&group, "def:one", "Additive group");
        let two = definition(&group, "def:two", "Multiplicative group");
        let three = definition(&group, "def:three", "Pointed group");
        for entry in [&one, &two, &three] {
            admit(connection, entry, &user).unwrap();
        }
        let mut first = Bridge::direct(one.id, two.id, BridgeKind::Structure, "first").unwrap();
        let mut second = Bridge::direct(two.id, three.id, BridgeKind::Structure, "second").unwrap();
        first.correspondence = Correspondence::Audited;
        second.correspondence = Correspondence::Audited;
        admit_bridge(connection, &group, &first, &user).unwrap();
        admit_bridge(connection, &group, &second, &user).unwrap();
        let derived = Bridge::compose(&first, &second, "derived").unwrap();
        admit_bridge(connection, &group, &derived, &user).unwrap();

        withdraw(
            connection,
            "bridge",
            &first.id.to_string(),
            &user,
            "The prose was too broad",
        )
        .unwrap();

        let stored = bridges(connection, &group).unwrap();
        assert_eq!(
            stored
                .iter()
                .find(|item| item.bridge.id == derived.id)
                .unwrap()
                .bridge
                .correspondence,
            Correspondence::AuditWithdrawn
        );
    }

    #[test]
    fn a_decision_closes_the_task_and_not_the_withdrawn_fact() {
        let (mut database, user, group) = setting();
        let connection = database.connection_mut();
        let one = definition(&group, "def:one", "Additive group");
        let two = definition(&group, "def:two", "Multiplicative group");
        admit(connection, &one, &user).unwrap();
        admit(connection, &two, &user).unwrap();
        let bridge = Bridge::direct(one.id, two.id, BridgeKind::Structure, "bridge").unwrap();
        admit_bridge(connection, &group, &bridge, &user).unwrap();
        withdraw(
            connection,
            "bridge",
            &bridge.id.to_string(),
            &user,
            "Mismatch",
        )
        .unwrap();
        let obligation = assign(connection, &group, &bridge.id, "thm:main", &user).unwrap();

        assert_eq!(obligations(connection, &user).unwrap().len(), 1);
        assert!(
            decide(
                connection,
                &obligation,
                &user,
                DownstreamDecision::Stands,
                "Checked"
            )
            .unwrap()
        );
        assert!(obligations(connection, &user).unwrap().is_empty());
        assert_eq!(
            bridges(connection, &group).unwrap()[0]
                .bridge
                .correspondence,
            Correspondence::AuditWithdrawn
        );
    }
}

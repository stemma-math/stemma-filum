//! The index over what machines have built.
//!
//! The artefacts are files; this says which ones exist and which is the
//! most recent, so that a page can find what to show without reading a
//! directory. Every row here could be rebuilt by walking the shelf.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::artefact::{Kind, Scope};
use stemma_core::id::{ArtefactId, GroupId, UserId};

use super::id_at;

/// One thing a machine built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// Its identifier.
    pub id: ArtefactId,
    /// Which group's work it is.
    pub group: GroupId,
    /// Which repository it came from.
    pub scope: Scope,
    /// What kind of artefact it is.
    pub kind: Kind,
    /// The commit it was built from.
    pub commit: String,
    /// How large it is.
    pub bytes: i64,
    /// Whose machine built it.
    pub built_by: UserId,
    /// When it arrived.
    pub built_at: String,
}

impl Built {
    /// Reads one out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let scope: String = row.get("scope")?;
        let kind: String = row.get("kind")?;

        Ok(Self {
            id: id_at(row, "id")?,
            group: id_at(row, "group_id")?,
            scope: scope.parse().map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    format!("`{scope}` does not name a repository").into(),
                )
            })?,
            kind: Kind::named(&kind).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    format!("`{kind}` is not a kind of artefact").into(),
                )
            })?,
            commit: row.get("commit_id")?,
            bytes: row.get("bytes")?,
            built_by: id_at(row, "built_by")?,
            built_at: row.get("built_at")?,
        })
    }
}

/// The columns every read asks for.
const COLUMNS: &str = "id, group_id, scope, kind, commit_id, bytes, built_by, built_at";

/// Writes down that something was built.
pub fn record(
    connection: &Connection,
    group: &GroupId,
    scope: &Scope,
    kind: Kind,
    commit: &str,
    bytes: usize,
    by: &UserId,
) -> Result<Built> {
    connection
        .execute(
            "INSERT INTO artefacts (id, group_id, scope, kind, commit_id, bytes, built_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (group_id, scope, kind, commit_id) DO UPDATE
                SET bytes = ?6, built_by = ?7, built_at = datetime('now')",
            params![
                ArtefactId::generate().to_string(),
                group.to_string(),
                scope.to_string(),
                kind.as_str(),
                commit,
                bytes as i64,
                by.to_string()
            ],
        )
        .context("cannot record the artefact")?;

    for_commit(connection, group, scope, kind, commit)?
        .context("the artefact was recorded and cannot be read back")
}

/// The artefact built from one commit, if there is one.
pub fn for_commit(
    connection: &Connection,
    group: &GroupId,
    scope: &Scope,
    kind: Kind,
    commit: &str,
) -> Result<Option<Built>> {
    connection
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM artefacts
                  WHERE group_id = ?1 AND scope = ?2 AND kind = ?3 AND commit_id = ?4"
            ),
            params![group.to_string(), scope.to_string(), kind.as_str(), commit],
            Built::from_row,
        )
        .optional()
        .context("cannot read the artefact")
}

/// The most recent artefact of a kind for one repository.
///
/// Two artefacts recorded in the same second are told apart by the order
/// they arrived in, which is what "most recent" means here.
pub fn latest(
    connection: &Connection,
    group: &GroupId,
    scope: &Scope,
    kind: Kind,
) -> Result<Option<Built>> {
    connection
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM artefacts
                  WHERE group_id = ?1 AND scope = ?2 AND kind = ?3
                  ORDER BY built_at DESC, rowid DESC LIMIT 1"
            ),
            params![group.to_string(), scope.to_string(), kind.as_str()],
            Built::from_row,
        )
        .optional()
        .context("cannot read the artefacts")
}

/// Everything built for one repository, newest first.
pub fn for_scope(connection: &Connection, group: &GroupId, scope: &Scope) -> Result<Vec<Built>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM artefacts
          WHERE group_id = ?1 AND scope = ?2
          ORDER BY built_at DESC, rowid DESC"
    ))?;

    let built = statement
        .query_map(
            params![group.to_string(), scope.to_string()],
            Built::from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the artefacts")?;

    Ok(built)
}

/// Forgets one, for when it has been taken off the shelf.
pub fn forget(connection: &Connection, id: &ArtefactId) -> Result<bool> {
    let removed = connection
        .execute("DELETE FROM artefacts WHERE id = ?1", [id.to_string()])
        .context("cannot forget the artefact")?;

    Ok(removed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups, test_database, users};
    use stemma_core::id::Slug;

    fn set_up(database: &mut crate::db::Database) -> (GroupId, UserId) {
        let ana = users::create(
            database.connection(),
            &Slug::parse("ana").unwrap(),
            "ana@example.org",
            "Ana",
            "hash",
        )
        .unwrap()
        .unwrap();

        let group = groups::create(
            database.connection_mut(),
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &ana.id,
        )
        .unwrap();

        (group.id, ana.id)
    }

    #[test]
    fn the_most_recent_artefact_is_the_one_a_page_finds() {
        let mut database = test_database();
        let (group, ana) = set_up(&mut database);
        let connection = database.connection();

        assert_eq!(
            latest(connection, &group, &Scope::Shared, Kind::Nodes).unwrap(),
            None
        );

        record(
            connection,
            &group,
            &Scope::Shared,
            Kind::Nodes,
            "aaaaaaa",
            10,
            &ana,
        )
        .unwrap();
        let second = record(
            connection,
            &group,
            &Scope::Shared,
            Kind::Nodes,
            "bbbbbbb",
            20,
            &ana,
        )
        .unwrap();

        assert_eq!(
            latest(connection, &group, &Scope::Shared, Kind::Nodes)
                .unwrap()
                .map(|built| built.commit),
            Some(second.commit)
        );
        assert_eq!(
            for_scope(connection, &group, &Scope::Shared).unwrap().len(),
            2
        );
    }

    #[test]
    fn building_one_commit_twice_leaves_one_artefact() {
        let mut database = test_database();
        let (group, ana) = set_up(&mut database);
        let connection = database.connection();

        record(
            connection,
            &group,
            &Scope::Shared,
            Kind::Nodes,
            "aaaaaaa",
            10,
            &ana,
        )
        .unwrap();
        let again = record(
            connection,
            &group,
            &Scope::Shared,
            Kind::Nodes,
            "aaaaaaa",
            30,
            &ana,
        )
        .unwrap();

        assert_eq!(
            for_scope(connection, &group, &Scope::Shared).unwrap().len(),
            1
        );
        assert_eq!(again.bytes, 30);
    }

    #[test]
    fn the_two_repositories_of_a_group_do_not_share_artefacts() {
        let mut database = test_database();
        let (group, ana) = set_up(&mut database);
        let connection = database.connection();
        let mine = Scope::Personal(ana.clone());

        record(
            connection,
            &group,
            &Scope::Shared,
            Kind::Nodes,
            "aaaaaaa",
            10,
            &ana,
        )
        .unwrap();

        assert!(
            latest(connection, &group, &mine, Kind::Nodes)
                .unwrap()
                .is_none()
        );

        record(connection, &group, &mine, Kind::Nodes, "aaaaaaa", 10, &ana).unwrap();

        assert_eq!(for_scope(connection, &group, &mine).unwrap().len(), 1);
        assert_eq!(
            for_scope(connection, &group, &Scope::Shared).unwrap().len(),
            1
        );
    }

    #[test]
    fn an_artefact_that_has_been_thrown_away_is_forgotten() {
        let mut database = test_database();
        let (group, ana) = set_up(&mut database);
        let connection = database.connection();

        let built = record(
            connection,
            &group,
            &Scope::Shared,
            Kind::Nodes,
            "aaaaaaa",
            10,
            &ana,
        )
        .unwrap();

        assert!(forget(connection, &built.id).unwrap());
        assert!(!forget(connection, &built.id).unwrap());
        assert!(
            latest(connection, &group, &Scope::Shared, Kind::Nodes)
                .unwrap()
                .is_none()
        );
    }
}

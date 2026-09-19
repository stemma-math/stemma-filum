//! The lines somebody started, and who writes them.
//!
//! What a line *is* lives in git: its directory, its source, its title
//! and the question it exists to answer. This table holds the two facts
//! git has no place for — who started it, and, once it is shared,
//! whether its author kept the writing to themselves.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::access::WhoWrites;
use stemma_core::id::{GroupId, LineId, UserId};
use stemma_core::line::Name;
use stemma_core::repository::Side;

use super::id_at;

/// One line the instance knows was started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// Its identifier.
    pub id: LineId,
    /// Whose group it is in.
    pub group: GroupId,
    /// Which of the group's repositories it sits in.
    pub side: Side,
    /// What it is called, which is its directory.
    pub name: Name,
    /// Who started it, and answers for it.
    pub author: UserId,
    /// When.
    pub started_at: String,
    /// Who may write it.
    pub writing: WhoWrites,
    /// When it moved to the group, if it has.
    pub shared_at: Option<String>,
}

impl Started {
    /// Reads one out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let unreadable = |what: &str, value: &str| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("`{value}` is not {what}").into(),
            )
        };

        let side: String = row.get("side")?;
        let name: String = row.get("name")?;
        let writing: String = row.get("writing")?;

        Ok(Self {
            id: id_at(row, "id")?,
            group: id_at(row, "group_id")?,
            side: side
                .parse()
                .map_err(|_| unreadable("a repository", &side))?,
            name: Name::parse(&name).map_err(|_| unreadable("a line", &name))?,
            author: id_at(row, "author")?,
            started_at: row.get("started_at")?,
            writing: writing
                .parse()
                .map_err(|_| unreadable("somebody who writes", &writing))?,
            shared_at: row.get("shared_at")?,
        })
    }

    /// Whether this person may write it themselves.
    pub fn admits(&self, who: &UserId) -> bool {
        self.writing.admits(&self.author, who)
    }
}

/// The columns every read asks for.
const COLUMNS: &str = "id, group_id, side, name, author, started_at, writing, shared_at";

/// Records that somebody started a line.
pub fn start(
    connection: &Connection,
    group: &GroupId,
    side: Side,
    name: &Name,
    author: &UserId,
) -> Result<Started> {
    connection
        .execute(
            "INSERT INTO lines (id, group_id, side, name, author)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                LineId::generate().to_string(),
                group.to_string(),
                side.as_str(),
                name.as_str(),
                author.to_string()
            ],
        )
        .context("cannot record the line")?;

    by_name(connection, group, side, name)?.context("the line cannot be read back")
}

/// Records that a line moved to the group, and who writes it there.
pub fn share(
    connection: &Connection,
    group: &GroupId,
    name: &Name,
    author: &UserId,
    writing: WhoWrites,
) -> Result<Started> {
    connection
        .execute(
            "DELETE FROM lines WHERE group_id = ?1 AND side = 'private' AND name = ?2",
            params![group.to_string(), name.as_str()],
        )
        .context("cannot move the line")?;

    connection
        .execute(
            "INSERT INTO lines (id, group_id, side, name, author, writing, shared_at)
             VALUES (?1, ?2, 'shared', ?3, ?4, ?5, datetime('now'))
             ON CONFLICT (group_id, side, name) DO UPDATE
                SET writing = ?5, shared_at = datetime('now')",
            params![
                LineId::generate().to_string(),
                group.to_string(),
                name.as_str(),
                author.to_string(),
                writing.as_str()
            ],
        )
        .context("cannot record that the line is the group's")?;

    by_name(connection, group, Side::Shared, name)?.context("the line cannot be read back")
}

/// One line.
pub fn by_name(
    connection: &Connection,
    group: &GroupId,
    side: Side,
    name: &Name,
) -> Result<Option<Started>> {
    connection
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM lines
                  WHERE group_id = ?1 AND side = ?2 AND name = ?3"
            ),
            params![group.to_string(), side.as_str(), name.as_str()],
            Started::from_row,
        )
        .optional()
        .context("cannot read the line")
}

/// Every line one person started in a group.
pub fn started_by(
    connection: &Connection,
    group: &GroupId,
    author: &UserId,
) -> Result<Vec<Started>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM lines
          WHERE group_id = ?1 AND author = ?2
          ORDER BY started_at, name"
    ))?;

    let found = statement
        .query_map(
            params![group.to_string(), author.to_string()],
            Started::from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the lines")?;

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups, test_database, users};
    use stemma_core::id::Slug;

    fn instance() -> (crate::db::Database, GroupId, UserId) {
        let mut database = test_database();
        let author = users::create(
            database.connection(),
            &Slug::parse("ana").unwrap(),
            "ana@example.org",
            "Ana",
            "hash",
        )
        .unwrap()
        .unwrap()
        .id;
        let group = groups::create(
            database.connection_mut(),
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &author,
        )
        .unwrap()
        .id;

        (database, group, author)
    }

    fn quotients() -> Name {
        Name::parse("quotient-groups").unwrap()
    }

    #[test]
    fn a_line_starts_as_its_authors_own() {
        let (database, group, author) = instance();
        let connection = database.connection();

        let started = start(connection, &group, Side::Private, &quotients(), &author).unwrap();

        assert_eq!(started.side, Side::Private);
        assert_eq!(started.author, author);
        assert_eq!(started.shared_at, None);
        assert_eq!(
            by_name(connection, &group, Side::Shared, &quotients()).unwrap(),
            None
        );
    }

    #[test]
    fn sharing_moves_the_line_and_says_who_writes_it() {
        let (database, group, author) = instance();
        let connection = database.connection();
        let other: UserId = "usr:bruno".parse().unwrap();

        start(connection, &group, Side::Private, &quotients(), &author).unwrap();
        let shared = share(connection, &group, &quotients(), &author, WhoWrites::Author).unwrap();

        assert_eq!(shared.side, Side::Shared);
        assert!(shared.shared_at.is_some());
        assert!(shared.admits(&author));
        assert!(!shared.admits(&other), "everybody else proposes");

        assert_eq!(
            by_name(connection, &group, Side::Private, &quotients()).unwrap(),
            None,
            "a move, not a copy"
        );
    }

    #[test]
    fn a_line_anybody_writes_admits_anybody() {
        let (database, group, author) = instance();
        let connection = database.connection();
        let other: UserId = "usr:bruno".parse().unwrap();

        start(connection, &group, Side::Private, &quotients(), &author).unwrap();
        let shared = share(connection, &group, &quotients(), &author, WhoWrites::Anyone).unwrap();

        assert!(shared.admits(&other));
        assert_eq!(started_by(connection, &group, &author).unwrap().len(), 1);
    }
}

//! The lines of every group, and where each is in its life.
//!
//! A line has one identifier from the moment it is started, through being
//! shared, to being deleted: what it says is in its revisions, and this
//! row holds what revisions have no place for — who started it, who may
//! write it once it is the group's, which revision it is at, and which of
//! those git has been told about.
//!
//! Starting, sharing and deleting are revisions like any other, so a
//! machine that knows which revision it last saw knows whether anything
//! happened to the line since, whatever it was.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::access::WhoWrites;
use stemma_core::event::{Event, LineChange};
use stemma_core::id::{GroupId, LineId, UserId};
use stemma_core::line::Name;
use stemma_core::repository::Side;

use super::events::{self, Audience};
use super::id_at;
use super::revisions::{self, Content, Kind};

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
    /// The revision it is at.
    pub revision: u64,
    /// The last revision git has a commit for.
    pub committed: u64,
    /// When it was deleted, if it was.
    pub deleted_at: Option<String>,
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
        let revision: i64 = row.get("revision")?;
        let committed: i64 = row.get("committed")?;

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
            revision: revision.max(0) as u64,
            committed: committed.max(0) as u64,
            deleted_at: row.get("deleted_at")?,
        })
    }

    /// Whether this person may write it themselves.
    pub fn admits(&self, who: &UserId) -> bool {
        &self.author == who || self.side == Side::Shared && self.writing.admits(&self.author, who)
    }

    /// Whether this person may see it at all.
    pub fn visible_to(&self, who: &UserId) -> bool {
        self.side == Side::Shared || &self.author == who
    }

    /// Whether it has been deleted.
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    /// Who hears about what happens to it.
    pub fn audience(&self) -> Audience<'_> {
        match self.side {
            Side::Shared => Audience::Group(&self.group),
            Side::Private => Audience::Person(&self.group, &self.author),
        }
    }
}

/// The columns every read asks for.
const COLUMNS: &str = "id, group_id, side, name, author, started_at, writing, shared_at, \
                       revision, committed, deleted_at";

/// Starts a line, with its first revision.
///
/// The first revision is what the author asked for and nothing else:
/// its page is written without compiling anything, because the instance
/// never compiles.
pub fn start(
    connection: &mut Connection,
    group: &GroupId,
    side: Side,
    name: &Name,
    author: &UserId,
    content: &Content,
) -> Result<Started> {
    let transaction = connection.transaction()?;
    let id = LineId::generate();

    transaction
        .execute(
            "INSERT INTO lines (id, group_id, side, name, author, shared_at)
             VALUES (?1, ?2, ?3, ?4, ?5, CASE WHEN ?3 = 'shared' THEN datetime('now') END)",
            params![
                id.to_string(),
                group.to_string(),
                side.as_str(),
                name.as_str(),
                author.to_string()
            ],
        )
        .context("cannot record the line")?;

    revisions::add(
        &transaction,
        &id,
        Kind::Started,
        content,
        author,
        None,
        None,
    )?;
    let started = by_id(&transaction, &id)?.context("the line cannot be read back")?;
    tell(&transaction, &started, LineChange::Started)?;
    transaction.commit()?;

    Ok(started)
}

/// Moves a line to the group, and says who writes it there.
///
/// Its content does not change, so the revision that moves it carries
/// the same source and the same page as the one before.
pub fn share(connection: &mut Connection, line: &LineId, writing: WhoWrites) -> Result<Started> {
    let transaction = connection.transaction()?;
    let before = by_id(&transaction, line)?.context("there is no such line")?;
    let current = revisions::at(&transaction, line, before.revision)?
        .context("the line has no revision to share")?;

    transaction
        .execute(
            "UPDATE lines SET side = 'shared', writing = ?2, shared_at = datetime('now')
              WHERE id = ?1",
            params![line.to_string(), writing.as_str()],
        )
        .context("cannot record that the line is the group's")?;

    revisions::add(
        &transaction,
        line,
        Kind::Shared,
        &current.content(),
        &before.author,
        None,
        None,
    )?;

    let shared = by_id(&transaction, line)?.context("the line cannot be read back")?;

    // Whoever could see it before hears it went, and the group hears it
    // arrived: for its author these are the same event.
    tell(&transaction, &shared, LineChange::Shared)?;
    transaction.commit()?;

    Ok(shared)
}

/// Deletes a line, keeping every revision it had.
///
/// The revision that deletes it carries what it last said, so what was
/// deleted can always be read back.
pub fn delete(connection: &mut Connection, line: &LineId, by: &UserId) -> Result<Started> {
    let transaction = connection.transaction()?;
    let before = by_id(&transaction, line)?.context("there is no such line")?;
    let current = revisions::at(&transaction, line, before.revision)?
        .context("the line has no revision to delete")?;

    revisions::add(
        &transaction,
        line,
        Kind::Deleted,
        &current.content(),
        by,
        None,
        None,
    )?;
    transaction
        .execute(
            "UPDATE lines SET deleted_at = datetime('now') WHERE id = ?1",
            [line.to_string()],
        )
        .context("cannot delete the line")?;

    let deleted = by_id(&transaction, line)?.context("the line cannot be read back")?;
    tell(&transaction, &deleted, LineChange::Deleted)?;
    transaction.commit()?;

    Ok(deleted)
}

/// Notes that git has a commit for everything up to a revision.
pub fn committed(connection: &Connection, line: &LineId, revision: u64) -> Result<()> {
    connection
        .execute(
            "UPDATE lines SET committed = max(committed, ?2) WHERE id = ?1",
            params![line.to_string(), revision as i64],
        )
        .context("cannot record the commit")?;

    Ok(())
}

/// Says a line changed, to whoever can see it now.
pub(super) fn tell(connection: &Connection, line: &Started, change: LineChange) -> Result<()> {
    events::publish(
        connection,
        line.audience(),
        &Event::Line {
            group: line.group.slug().as_str().to_owned(),
            side: line.side,
            line: line.name.as_str().to_owned(),
            change,
            revision: Some(line.revision),
        },
    )?;

    Ok(())
}

/// One line, however it stands.
pub fn by_id(connection: &Connection, id: &LineId) -> Result<Option<Started>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM lines WHERE id = ?1"),
            [id.to_string()],
            Started::from_row,
        )
        .optional()
        .context("cannot read the line")
}

/// The line a person means by its place, if it exists and is not deleted.
///
/// A private line is the one this person started: another person's line
/// of the same name is somewhere they cannot see.
pub fn by_name(
    connection: &Connection,
    group: &GroupId,
    side: Side,
    name: &Name,
    author: &UserId,
) -> Result<Option<Started>> {
    connection
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM lines
                  WHERE group_id = ?1 AND side = ?2 AND name = ?3
                    AND (side = 'shared' OR author = ?4)
                    AND deleted_at IS NULL"
            ),
            params![
                group.to_string(),
                side.as_str(),
                name.as_str(),
                author.to_string()
            ],
            Started::from_row,
        )
        .optional()
        .context("cannot read the line")
}

/// Every line one person started in a group, and has not deleted.
pub fn started_by(
    connection: &Connection,
    group: &GroupId,
    author: &UserId,
) -> Result<Vec<Started>> {
    list(
        connection,
        "group_id = ?1 AND author = ?2 AND deleted_at IS NULL",
        params![group.to_string(), author.to_string()],
    )
}

/// Every line a person can see, in every group they are in, deleted ones
/// too: a machine that had one learns it went from this.
pub fn visible_to(connection: &Connection, who: &UserId) -> Result<Vec<Started>> {
    list(
        connection,
        "group_id IN (SELECT group_id FROM memberships WHERE user_id = ?1)
         AND (side = 'shared' OR author = ?1)",
        params![who.to_string()],
    )
}

/// Every line of a group a person can see that is not deleted.
pub fn in_group(connection: &Connection, group: &GroupId, who: &UserId) -> Result<Vec<Started>> {
    list(
        connection,
        "group_id = ?1 AND (side = 'shared' OR author = ?2) AND deleted_at IS NULL",
        params![group.to_string(), who.to_string()],
    )
}

/// Reads several lines.
fn list(
    connection: &Connection,
    condition: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<Started>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM lines WHERE {condition} ORDER BY started_at, name"
    ))?;

    let found = statement
        .query_map(parameters, Started::from_row)?
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

    fn content() -> Content {
        Content {
            source: "{\"title\":\"Quotient groups\"}".to_owned(),
            lean_source: None,
            page: "<h1>Quotient groups</h1>".to_owned(),
        }
    }

    #[test]
    fn a_line_starts_as_its_authors_own_at_its_first_revision() {
        let (mut database, group, author) = instance();
        let connection = database.connection_mut();

        let started = start(
            connection,
            &group,
            Side::Private,
            &quotients(),
            &author,
            &content(),
        )
        .unwrap();

        assert_eq!(started.side, Side::Private);
        assert_eq!(started.author, author);
        assert_eq!(started.shared_at, None);
        assert_eq!(started.revision, 1);
        assert_eq!(
            by_name(connection, &group, Side::Shared, &quotients(), &author).unwrap(),
            None
        );
    }

    #[test]
    fn sharing_moves_the_line_keeps_it_and_says_who_writes_it() {
        let (mut database, group, author) = instance();
        let connection = database.connection_mut();
        let other: UserId = "usr:bruno".parse().unwrap();

        let started = start(
            connection,
            &group,
            Side::Private,
            &quotients(),
            &author,
            &content(),
        )
        .unwrap();
        let shared = share(connection, &started.id, WhoWrites::Author).unwrap();

        assert_eq!(shared.id, started.id, "the same line, moved");
        assert_eq!(shared.side, Side::Shared);
        assert_eq!(shared.revision, 2);
        assert!(shared.shared_at.is_some());
        assert!(shared.admits(&author));
        assert!(!shared.admits(&other), "everybody else proposes");

        let moved = revisions::at(connection, &started.id, 2).unwrap().unwrap();
        assert_eq!(moved.kind, Kind::Shared);
        assert_eq!(
            moved.content(),
            content(),
            "sharing changes nothing it says"
        );

        assert_eq!(
            by_name(connection, &group, Side::Private, &quotients(), &author).unwrap(),
            None,
            "a move, not a copy"
        );
    }

    #[test]
    fn a_line_anybody_writes_admits_anybody() {
        let (mut database, group, author) = instance();
        let connection = database.connection_mut();
        let other: UserId = "usr:bruno".parse().unwrap();

        let started = start(
            connection,
            &group,
            Side::Private,
            &quotients(),
            &author,
            &content(),
        )
        .unwrap();
        let shared = share(connection, &started.id, WhoWrites::Anyone).unwrap();

        assert!(shared.admits(&other));
        assert_eq!(started_by(connection, &group, &author).unwrap().len(), 1);
    }

    #[test]
    fn a_deleted_line_keeps_what_it_said_and_frees_its_name() {
        let (mut database, group, author) = instance();
        let connection = database.connection_mut();

        let started = start(
            connection,
            &group,
            Side::Private,
            &quotients(),
            &author,
            &content(),
        )
        .unwrap();
        let deleted = delete(connection, &started.id, &author).unwrap();

        assert!(deleted.is_deleted());
        assert_eq!(deleted.revision, 2);
        assert_eq!(
            revisions::at(connection, &started.id, 2)
                .unwrap()
                .unwrap()
                .content(),
            content()
        );
        assert_eq!(
            by_name(connection, &group, Side::Private, &quotients(), &author).unwrap(),
            None
        );
        assert_eq!(
            visible_to(connection, &author).unwrap().len(),
            1,
            "still listed as gone"
        );

        let again = start(
            connection,
            &group,
            Side::Private,
            &quotients(),
            &author,
            &content(),
        )
        .unwrap();
        assert_ne!(again.id, started.id, "a new line of the same name");
    }

    #[test]
    fn personal_names_are_independent_and_sharing_moves_only_the_authors_line() {
        let (mut database, group, author) = instance();
        let connection = database.connection_mut();
        let other = users::create(
            connection,
            &Slug::parse("bruno").unwrap(),
            "bruno@example.org",
            "Bruno",
            "hash",
        )
        .unwrap()
        .unwrap()
        .id;
        let mine = start(
            connection,
            &group,
            Side::Private,
            &quotients(),
            &author,
            &content(),
        )
        .unwrap();
        let theirs = start(
            connection,
            &group,
            Side::Private,
            &quotients(),
            &other,
            &content(),
        )
        .unwrap();
        assert!(!theirs.admits(&author));
        assert!(!theirs.visible_to(&author));

        share(connection, &mine.id, WhoWrites::Anyone).unwrap();

        assert!(
            by_name(connection, &group, Side::Private, &quotients(), &author)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            by_name(connection, &group, Side::Private, &quotients(), &other)
                .unwrap()
                .map(|line| line.id),
            Some(theirs.id)
        );
    }
}

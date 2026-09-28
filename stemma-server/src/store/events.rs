//! The notices the instance keeps for the machines watching it.
//!
//! Every change another reader should hear about is written here in the
//! same breath as the change itself, by the function making it, so there
//! is no handler that can make one and forget to say so. Readers ask for
//! what came after the last one they saw and get only what is theirs to
//! see.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use stemma_core::event::Event;
use stemma_core::id::{GroupId, UserId};

/// How long a notice is kept for a reader who was away.
pub const KEPT_DAYS: u32 = 7;

/// The most notices handed over in one read.
pub const PAGE: usize = 500;

/// Who may hear about a change.
#[derive(Debug, Clone, Copy)]
pub enum Audience<'a> {
    /// Everybody in a group.
    Group(&'a GroupId),
    /// One person, about one of their groups.
    Person(&'a GroupId, &'a UserId),
    /// One person, about nothing in particular of any group.
    Alone(&'a UserId),
}

/// One notice, as a reader receives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Its place in the order every reader sees.
    pub id: i64,
    /// What changed.
    pub event: Event,
}

/// Writes a notice down.
pub fn publish(connection: &Connection, audience: Audience<'_>, event: &Event) -> Result<i64> {
    let (group, user) = match audience {
        Audience::Group(group) => (Some(group.to_string()), None),
        Audience::Person(group, user) => (Some(group.to_string()), Some(user.to_string())),
        Audience::Alone(user) => (None, Some(user.to_string())),
    };

    connection
        .execute(
            "INSERT INTO events (kind, group_id, user_id, subject) VALUES (?1, ?2, ?3, ?4)",
            params![
                event.kind(),
                group,
                user,
                serde_json::to_string(event).context("cannot write the event down")?
            ],
        )
        .context("cannot record the event")?;

    Ok(connection.last_insert_rowid())
}

/// What a reader has not seen yet, oldest first, up to [`PAGE`].
///
/// A notice with a person on it is theirs alone; one without is for
/// everybody in its group, as the group is now — somebody who left does
/// not hear about it any more, and somebody who joined hears from then on.
pub fn after(connection: &Connection, since: i64, reader: &UserId) -> Result<Vec<Notice>> {
    let mut statement = connection.prepare(
        "SELECT id, subject FROM events
          WHERE id > ?1
            AND (user_id = ?2
                 OR (user_id IS NULL
                     AND group_id IN (SELECT group_id FROM memberships WHERE user_id = ?2)))
          ORDER BY id
          LIMIT ?3",
    )?;

    let rows = statement
        .query_map(params![since, reader.to_string(), PAGE as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot read the events")?;

    // A notice this build cannot read is somebody else's future: it is
    // skipped rather than stopping every one after it.
    Ok(rows
        .into_iter()
        .filter_map(|(id, subject)| {
            serde_json::from_str(&subject)
                .ok()
                .map(|event| Notice { id, event })
        })
        .collect())
}

/// The last notice written, or nothing when there has never been one.
///
/// Read from the counter rather than from the rows, which may all have
/// been thrown away: the place in the order is never reused.
pub fn latest(connection: &Connection) -> Result<i64> {
    Ok(connection
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'events'",
            [],
            |row| row.get(0),
        )
        .optional()
        .context("cannot read the events")?
        .unwrap_or(0))
}

/// Whether a reader who last saw `since` has missed notices that were
/// already thrown away, and must start over.
pub fn is_too_old(connection: &Connection, since: i64) -> Result<bool> {
    let oldest: Option<i64> = connection
        .query_row("SELECT min(id) FROM events", [], |row| row.get(0))
        .optional()
        .context("cannot read the events")?
        .flatten();

    Ok(match oldest {
        // Something between what they saw and the oldest kept is gone.
        Some(oldest) => since + 1 < oldest,
        // Nothing is kept at all; if anything was ever written after what
        // they saw, it is gone.
        None => since < latest(connection)?,
    })
}

/// Throws away notices older than [`KEPT_DAYS`].
pub fn forget_old(connection: &Connection) -> Result<usize> {
    connection
        .execute(
            "DELETE FROM events WHERE at < datetime('now', ?1)",
            [format!("-{KEPT_DAYS} days")],
        )
        .context("cannot forget the old events")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_database;
    use crate::store::{groups, users};
    use stemma_core::event::{GroupChange, LineChange};
    use stemma_core::id::Slug;
    use stemma_core::repository::Side;

    fn person(connection: &Connection, name: &str) -> UserId {
        users::create(
            connection,
            &Slug::parse(name).unwrap(),
            &format!("{name}@example.org"),
            name,
            "hash",
        )
        .unwrap()
        .unwrap()
        .id
    }

    fn written(line: &str) -> Event {
        Event::Line {
            group: "algebra".to_owned(),
            side: Side::Shared,
            line: line.to_owned(),
            change: LineChange::Written,
            revision: Some(2),
        }
    }

    #[test]
    fn a_group_hears_about_its_own_and_nothing_else() {
        let mut database = test_database();
        let ana = person(database.connection(), "ana");
        let bo = person(database.connection(), "bo");
        let algebra = groups::create(
            database.connection_mut(),
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &ana,
        )
        .unwrap()
        .id;
        let connection = database.connection();

        let first = publish(connection, Audience::Group(&algebra), &written("one")).unwrap();
        publish(
            connection,
            Audience::Person(&algebra, &ana),
            &written("mine"),
        )
        .unwrap();

        // Making the group said so first.
        let heard = after(connection, 0, &ana).unwrap();
        assert_eq!(heard.len(), 3);
        assert_eq!(heard[1].id, first);
        assert!(
            after(connection, 0, &bo).unwrap().is_empty(),
            "bo is not in it"
        );
        assert_eq!(after(connection, first, &ana).unwrap().len(), 1);

        groups::add_member(
            connection,
            &algebra,
            &bo,
            stemma_core::access::GroupRole::Member,
        )
        .unwrap();

        let heard: Vec<Event> = after(connection, 0, &bo)
            .unwrap()
            .into_iter()
            .map(|notice| notice.event)
            .collect();
        assert!(heard.contains(&written("one")), "bo hears the group's");
        assert!(!heard.contains(&written("mine")), "and not ana's own");
    }

    #[test]
    fn somebody_alone_hears_what_is_addressed_to_them() {
        let database = test_database();
        let connection = database.connection();
        let ana = person(connection, "ana");
        let made = Event::Group {
            group: "algebra".to_owned(),
            change: GroupChange::Created,
        };

        publish(connection, Audience::Alone(&ana), &made).unwrap();

        assert_eq!(after(connection, 0, &ana).unwrap()[0].event, made);
    }

    #[test]
    fn a_reader_away_too_long_is_told_to_start_over() {
        let database = test_database();
        let connection = database.connection();
        let ana = person(connection, "ana");

        assert!(
            !is_too_old(connection, 0).unwrap(),
            "nothing was ever missed"
        );

        for line in ["one", "two", "three"] {
            publish(connection, Audience::Alone(&ana), &written(line)).unwrap();
        }
        assert!(!is_too_old(connection, 0).unwrap());

        connection
            .execute(
                "UPDATE events SET at = datetime('now', '-30 days') WHERE id < 3",
                [],
            )
            .unwrap();
        assert_eq!(forget_old(connection).unwrap(), 2);

        assert!(is_too_old(connection, 0).unwrap(), "one and two are gone");
        assert!(!is_too_old(connection, 2).unwrap(), "three is still here");
        assert_eq!(latest(connection).unwrap(), 3);
    }
}

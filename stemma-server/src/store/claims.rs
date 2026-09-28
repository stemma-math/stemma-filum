//! The claims machines hold on lines.
//!
//! A claim says which machine is writing a line right now. It belongs to
//! the machine rather than to its owner: the same person on two of their
//! own devices is exactly the case this exists to settle.
//!
//! Every read here ignores a claim that has lapsed. Nothing sweeps the
//! table on a timer, because a claim that nobody has renewed is already
//! not a claim, and a row that says otherwise would make the instance
//! assert something it does not know.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::claim::{CLAIM_SECONDS, Held};
use stemma_core::event::{ClaimChange, Event};
use stemma_core::id::{ClaimId, GroupId, MachineId, UserId};
use stemma_core::repository::Side;

use super::events::{self, Audience};
use super::id_at;

/// One standing claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// Its identifier.
    pub id: ClaimId,
    /// Which group's line it is on.
    pub group: GroupId,
    /// Which of the group's two repositories.
    pub side: Side,
    /// Which line.
    pub line: String,
    /// The machine that is writing.
    pub machine: MachineId,
    /// What that machine calls itself.
    pub label: String,
    /// Whose machine it is.
    pub owner: UserId,
    /// When it was taken.
    pub taken_at: String,
    /// How many seconds are left before it lapses on its own.
    pub left: u64,
    /// Whether the holder has been asked to hand the line back.
    pub asked: bool,
}

impl Claim {
    /// Reads one out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let side: String = row.get("side")?;
        let left: i64 = row.get("left")?;

        Ok(Self {
            id: id_at(row, "id")?,
            group: id_at(row, "group_id")?,
            side: side.parse().unwrap_or(Side::Shared),
            line: row.get("line")?,
            machine: id_at(row, "machine_id")?,
            label: row.get("label")?,
            owner: id_at(row, "user_id")?,
            taken_at: row.get("taken_at")?,
            left: left.max(0) as u64,
            asked: row.get("asked")?,
        })
    }

    /// The claim as anybody looking at the line sees it.
    pub fn held(&self) -> Held {
        Held {
            machine: self.machine,
            label: self.label.clone(),
            left: self.left,
            asked: self.asked,
        }
    }
}

/// The columns every read asks for.
///
/// How long a claim has left is worked out where the clock is. A claim
/// with nothing left is not returned at all.
fn columns() -> String {
    format!(
        "claims.id, claims.group_id, claims.side, claims.line, claims.machine_id,
         claims.user_id, claims.taken_at, machines.label AS label,
         (claims.asked_at IS NOT NULL) AS asked,
         CAST({CLAIM_SECONDS} - (julianday('now') - julianday(claims.renewed_at)) * 86400
              AS INTEGER) AS left"
    )
}

/// What every read means by a claim that is still standing.
const STANDING: &str = "claims.released_at IS NULL
     AND claims.renewed_at > datetime('now', '-{seconds} seconds')";

/// The condition for a standing claim, with the clock filled in.
fn standing() -> String {
    STANDING.replace("{seconds}", &CLAIM_SECONDS.to_string())
}

/// Takes a line, unless another machine is writing it.
///
/// Taking a line this machine already holds is the same machine saying
/// so again, which renews it.
pub fn take(
    connection: &Connection,
    group: &GroupId,
    side: Side,
    line: &str,
    machine: &MachineId,
    owner: &UserId,
) -> Result<Claim> {
    if let Some(standing) = holder(connection, group, side, line, owner)? {
        if &standing.machine != machine {
            return Ok(standing);
        }

        renew(connection, &standing.id)?;

        return holder(connection, group, side, line, owner)?
            .context("the claim cannot be read back");
    }

    // A claim that lapsed is not a claim. Letting go of it here rather
    // than on a timer keeps the row honest: it says when it ended.
    connection
        .execute(
            "UPDATE claims SET released_at = datetime('now')
              WHERE group_id = ?1 AND side = ?2 AND line = ?3 AND released_at IS NULL
                AND (side = 'shared' OR user_id = ?4)",
            params![group.to_string(), side.as_str(), line, owner.to_string()],
        )
        .context("cannot let the lapsed claim go")?;

    connection
        .execute(
            "INSERT INTO claims (id, group_id, side, line, machine_id, user_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                ClaimId::generate().to_string(),
                group.to_string(),
                side.as_str(),
                line,
                machine.to_string(),
                owner.to_string()
            ],
        )
        .context("cannot take the line")?;

    tell(connection, group, side, line, owner, ClaimChange::Taken)?;

    holder(connection, group, side, line, owner)?.context("the claim cannot be read back")
}

/// Says a claim changed, to whoever can see the line.
fn tell(
    connection: &Connection,
    group: &GroupId,
    side: Side,
    line: &str,
    owner: &UserId,
    change: ClaimChange,
) -> Result<()> {
    let audience = match side {
        Side::Shared => Audience::Group(group),
        Side::Private => Audience::Person(group, owner),
    };

    events::publish(
        connection,
        audience,
        &Event::Claim {
            group: group.slug().as_str().to_owned(),
            side,
            line: line.to_owned(),
            change,
        },
    )?;

    Ok(())
}

/// Says a claim known only by its identifier changed.
fn tell_about(connection: &Connection, claim: &ClaimId, change: ClaimChange) -> Result<()> {
    let found: Option<(String, String, String, String)> = connection
        .query_row(
            "SELECT group_id, side, line, user_id FROM claims WHERE id = ?1",
            [claim.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .context("cannot read the claim")?;

    let Some((group, side, line, owner)) = found else {
        return Ok(());
    };
    let (Ok(group), Ok(side), Ok(owner)) = (group.parse(), side.parse(), owner.parse()) else {
        return Ok(());
    };

    tell(connection, &group, side, &line, &owner, change)
}

/// Says the machine is still writing.
pub fn renew(connection: &Connection, claim: &ClaimId) -> Result<()> {
    connection
        .execute(
            "UPDATE claims SET renewed_at = datetime('now')
              WHERE id = ?1 AND released_at IS NULL",
            params![claim.to_string()],
        )
        .context("cannot renew the claim")?;

    Ok(())
}

/// Lets a line go.
///
/// Anybody may put down a claim held by a machine of theirs, from that
/// machine or from the web: both sides are the same person's.
pub fn release(connection: &Connection, claim: &ClaimId) -> Result<()> {
    let released = connection
        .execute(
            "UPDATE claims SET released_at = datetime('now')
              WHERE id = ?1 AND released_at IS NULL",
            params![claim.to_string()],
        )
        .context("cannot let the line go")?;

    if released > 0 {
        tell_about(connection, claim, ClaimChange::Released)?;
    }

    Ok(())
}

/// Asks the machine holding a line to hand it back.
///
/// Asking twice says nothing new, so the first ask is the one recorded.
pub fn ask(connection: &Connection, claim: &ClaimId, from: Option<&MachineId>) -> Result<()> {
    let asked = connection
        .execute(
            "UPDATE claims
                SET asked_at = datetime('now'), asked_by = ?2
              WHERE id = ?1 AND released_at IS NULL AND asked_at IS NULL",
            params![claim.to_string(), from.map(ToString::to_string)],
        )
        .context("cannot ask for the line")?;

    if asked > 0 {
        tell_about(connection, claim, ClaimChange::Asked)?;
    }

    Ok(())
}

/// The machine writing one line, if one is.
pub fn holder(
    connection: &Connection,
    group: &GroupId,
    side: Side,
    line: &str,
    owner: &UserId,
) -> Result<Option<Claim>> {
    connection
        .query_row(
            &format!(
                "SELECT {} FROM claims JOIN machines ON machines.id = claims.machine_id
                  WHERE claims.group_id = ?1 AND claims.side = ?2 AND claims.line = ?3
                    AND (claims.side = 'shared' OR claims.user_id = ?4)
                    AND {}",
                columns(),
                standing()
            ),
            params![group.to_string(), side.as_str(), line, owner.to_string()],
            Claim::from_row,
        )
        .optional()
        .context("cannot read who is writing the line")
}

/// Every line somebody's machines are writing.
pub fn for_user(connection: &Connection, owner: &UserId) -> Result<Vec<Claim>> {
    list(
        connection,
        "claims.user_id = ?1",
        params![owner.to_string()],
    )
}

/// Every line being written in a group.
pub fn for_group(connection: &Connection, group: &GroupId) -> Result<Vec<Claim>> {
    list(
        connection,
        "claims.group_id = ?1",
        params![group.to_string()],
    )
}

/// Reads several claims.
fn list(
    connection: &Connection,
    condition: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<Claim>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {} FROM claims JOIN machines ON machines.id = claims.machine_id
          WHERE {condition} AND {}
          ORDER BY claims.taken_at, claims.id",
        columns(),
        standing()
    ))?;

    let found = statement
        .query_map(parameters, Claim::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the claims")?;

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups, machines, test_database, users};
    use stemma_core::id::Slug;

    /// An instance with one account, one group and two of its machines.
    fn instance() -> (crate::db::Database, GroupId, UserId, MachineId, MachineId) {
        let mut database = test_database();
        let owner = users::create(
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
            &owner,
        )
        .unwrap()
        .id;
        let laptop = machine(database.connection(), &owner, "tok-laptop", "a laptop");
        let desktop = machine(database.connection(), &owner, "tok-desktop", "a desktop");

        (database, group, owner, laptop, desktop)
    }

    /// One registered machine of somebody's.
    fn machine(connection: &Connection, owner: &UserId, token: &str, label: &str) -> MachineId {
        connection
            .execute(
                "INSERT INTO client_tokens (id, user_id, label, token_hash)
                 VALUES (?1, ?2, ?3, ?1)",
                params![token, owner.to_string(), label],
            )
            .unwrap();

        machines::register(connection, owner, token, label)
            .unwrap()
            .id
    }

    #[test]
    fn a_line_nobody_is_writing_is_taken_by_whoever_asks() {
        let (database, group, owner, laptop, _) = instance();
        let connection = database.connection();

        assert_eq!(
            holder(connection, &group, Side::Shared, "quotient-groups", &owner).unwrap(),
            None
        );

        let claim = take(
            connection,
            &group,
            Side::Shared,
            "quotient-groups",
            &laptop,
            &owner,
        )
        .unwrap();

        assert_eq!(claim.machine, laptop);
        assert!(claim.left > 0);
        assert!(!claim.asked);
    }

    #[test]
    fn a_line_one_machine_is_writing_is_not_handed_to_the_other() {
        let (database, group, owner, laptop, desktop) = instance();
        let connection = database.connection();

        take(connection, &group, Side::Shared, "line", &laptop, &owner).unwrap();
        let refused = take(connection, &group, Side::Shared, "line", &desktop, &owner).unwrap();

        assert_eq!(refused.machine, laptop, "the laptop is still writing it");
    }

    #[test]
    fn taking_a_line_this_machine_already_holds_renews_it() {
        let (database, group, owner, laptop, _) = instance();
        let connection = database.connection();

        let first = take(connection, &group, Side::Shared, "line", &laptop, &owner).unwrap();
        let again = take(connection, &group, Side::Shared, "line", &laptop, &owner).unwrap();

        assert_eq!(first.id, again.id);
    }

    #[test]
    fn a_claim_nobody_renewed_lapses_and_the_line_is_free() {
        let (database, group, owner, laptop, desktop) = instance();
        let connection = database.connection();

        take(connection, &group, Side::Shared, "line", &laptop, &owner).unwrap();
        connection
            .execute(
                "UPDATE claims SET renewed_at = datetime('now', '-1 day')",
                [],
            )
            .unwrap();

        assert_eq!(
            holder(connection, &group, Side::Shared, "line", &owner).unwrap(),
            None
        );

        let taken = take(connection, &group, Side::Shared, "line", &desktop, &owner).unwrap();

        assert_eq!(taken.machine, desktop);
    }

    #[test]
    fn a_line_that_was_asked_for_says_so_until_it_is_let_go() {
        let (database, group, owner, laptop, desktop) = instance();
        let connection = database.connection();

        let claim = take(connection, &group, Side::Shared, "line", &laptop, &owner).unwrap();
        ask(connection, &claim.id, Some(&desktop)).unwrap();

        let asked = holder(connection, &group, Side::Shared, "line", &owner)
            .unwrap()
            .unwrap();
        assert!(asked.asked);

        release(connection, &claim.id).unwrap();

        assert_eq!(
            holder(connection, &group, Side::Shared, "line", &owner).unwrap(),
            None
        );
    }

    #[test]
    fn the_two_sides_of_a_group_are_two_different_lines() {
        let (database, group, owner, laptop, desktop) = instance();
        let connection = database.connection();

        take(connection, &group, Side::Shared, "line", &laptop, &owner).unwrap();
        let private = take(connection, &group, Side::Private, "line", &desktop, &owner).unwrap();

        assert_eq!(private.machine, desktop);
        assert_eq!(for_user(connection, &owner).unwrap().len(), 2);
        assert_eq!(for_group(connection, &group).unwrap().len(), 2);
    }
}

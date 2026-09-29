//! Invitations into a group.
//!
//! An invitation is always a link: bound to one group and one role, single
//! use, expiring and revocable. Mail, where it is configured, delivers that
//! same link, so the path that works without mail is the one every
//! installation exercises rather than a fallback nobody has tried.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::access::GroupRole;
use stemma_core::id::{GroupId, InvitationId, UserId};

use super::{Role, id_at, optional_id_at};
use crate::tokens::Key;

/// An invitation, as the server keeps it.
///
/// The secret itself is not here: it is given to whoever made the
/// invitation, once, and what stays behind is a hash of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitation {
    /// The invitation's identifier.
    pub id: InvitationId,
    /// The group it leads into.
    pub group: GroupId,
    /// The role it carries.
    pub role: GroupRole,
    /// Who it was meant for, where that was known.
    pub email: Option<String>,
    /// Who made it.
    pub created_by: UserId,
    /// When it was made.
    pub created_at: String,
    /// When it stops working.
    pub expires_at: String,
    /// When it was used.
    pub accepted_at: Option<String>,
    /// Who used it.
    pub accepted_by: Option<UserId>,
    /// When it was called back.
    pub revoked_at: Option<String>,
}

impl Invitation {
    /// Reads an invitation out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: id_at(row, "id")?,
            group: id_at(row, "group_id")?,
            role: row.get::<_, Role>("role")?.0,
            email: row.get("email")?,
            created_by: id_at(row, "created_by")?,
            created_at: row.get("created_at")?,
            expires_at: row.get("expires_at")?,
            accepted_at: row.get("accepted_at")?,
            accepted_by: optional_id_at(row, "accepted_by")?,
            revoked_at: row.get("revoked_at")?,
        })
    }

    /// Why this invitation cannot be used, if it cannot.
    pub fn refusal(&self, now: &str) -> Option<Refusal> {
        if self.revoked_at.is_some() {
            Some(Refusal::Revoked)
        } else if self.accepted_at.is_some() {
            Some(Refusal::AlreadyUsed)
        } else if self.expires_at.as_str() <= now {
            Some(Refusal::Expired)
        } else {
            None
        }
    }
}

/// Why a link does not let somebody in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The link was already used, and an invitation is for one person.
    #[error("this invitation has already been used")]
    AlreadyUsed,
    /// Its time ran out.
    #[error("this invitation has expired")]
    Expired,
    /// Somebody in the group called it back.
    #[error("this invitation was withdrawn")]
    Revoked,
}

/// The columns every read asks for.
const COLUMNS: &str = "id, group_id, role, email, created_by, created_at, expires_at,
     accepted_at, accepted_by, revoked_at";

/// Makes an invitation, and returns the secret exactly once.
pub fn create(
    connection: &Connection,
    key: &Key,
    group: &GroupId,
    role: GroupRole,
    email: Option<&str>,
    by: &UserId,
    days: u32,
) -> Result<(Invitation, String)> {
    let id = InvitationId::generate();
    let secret = crate::tokens::secret()?;

    connection
        .execute(
            "INSERT INTO invitations
                 (id, group_id, role, email, token_hash, created_by, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now', ?7))",
            params![
                id.to_string(),
                group.to_string(),
                Role(role),
                email.map(|email| email.trim().to_lowercase()),
                key.fingerprint(&secret),
                by.to_string(),
                format!("+{days} days")
            ],
        )
        .context("cannot create the invitation")?;

    let invitation =
        by_id(connection, &id)?.context("the invitation was created and cannot be read back")?;

    Ok((invitation, secret))
}

/// One invitation.
pub fn by_id(connection: &Connection, id: &InvitationId) -> Result<Option<Invitation>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM invitations WHERE id = ?1"),
            [id.to_string()],
            Invitation::from_row,
        )
        .optional()
        .context("cannot read the invitation")
}

/// The invitation a link names, whether or not it can still be used.
pub fn by_secret(connection: &Connection, key: &Key, secret: &str) -> Result<Option<Invitation>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM invitations WHERE token_hash = ?1"),
            [key.fingerprint(secret)],
            Invitation::from_row,
        )
        .optional()
        .context("cannot read the invitation")
}

/// Every invitation into a group that could still be used, newest first.
pub fn live_for_group(connection: &Connection, group: &GroupId) -> Result<Vec<Invitation>> {
    super::rows(
        connection,
        &format!(
            "SELECT {COLUMNS} FROM invitations
          WHERE group_id = ?1
            AND accepted_at IS NULL AND revoked_at IS NULL
            AND expires_at > datetime('now')
          ORDER BY created_at DESC"
        ),
        [group.to_string()],
        Invitation::from_row,
    )
    .context("cannot list the invitations")
}

/// Uses an invitation up.
///
/// The update carries the conditions again, so two people racing on the
/// same link cannot both get in.
pub fn accept(connection: &Connection, id: &InvitationId, user: &UserId) -> Result<bool> {
    let used = connection
        .execute(
            "UPDATE invitations
                SET accepted_at = datetime('now'), accepted_by = ?2
              WHERE id = ?1
                AND accepted_at IS NULL AND revoked_at IS NULL
                AND expires_at > datetime('now')",
            params![id.to_string(), user.to_string()],
        )
        .context("cannot accept the invitation")?;

    Ok(used > 0)
}

/// Calls an invitation back.
pub fn revoke(connection: &Connection, id: &InvitationId, by: &UserId) -> Result<bool> {
    let revoked = connection
        .execute(
            "UPDATE invitations
                SET revoked_at = datetime('now'), revoked_by = ?2
              WHERE id = ?1 AND accepted_at IS NULL AND revoked_at IS NULL",
            params![id.to_string(), by.to_string()],
        )
        .context("cannot withdraw the invitation")?;

    Ok(revoked > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups, test_database, users};
    use stemma_core::id::Slug;

    fn slug(text: &str) -> Slug {
        Slug::parse(text).unwrap()
    }

    fn account(connection: &Connection, name: &str) -> users::User {
        users::create(
            connection,
            &slug(name),
            &format!("{name}@example.org"),
            name,
            "hash",
        )
        .unwrap()
        .unwrap()
    }

    fn group(database: &mut crate::db::Database, owner: &UserId) -> groups::Group {
        groups::create(
            database.connection_mut(),
            &slug("algebra"),
            "Algebra",
            owner,
        )
        .unwrap()
    }

    #[test]
    fn a_link_leads_into_one_group_with_one_role() {
        let mut database = test_database();
        let key = Key::for_test();
        let ana = account(database.connection(), "ana");
        let group = group(&mut database, &ana.id);

        let (invitation, secret) = create(
            database.connection(),
            &key,
            &group.id,
            GroupRole::Member,
            Some("Enric@example.org"),
            &ana.id,
            14,
        )
        .unwrap();

        assert_eq!(invitation.role, GroupRole::Member);
        assert_eq!(invitation.email.as_deref(), Some("enric@example.org"));
        assert_eq!(
            by_secret(database.connection(), &key, &secret).unwrap(),
            Some(invitation)
        );
        assert_eq!(
            by_secret(database.connection(), &key, "not the secret").unwrap(),
            None
        );
    }

    #[test]
    fn the_secret_is_not_in_the_database() {
        let mut database = test_database();
        let key = Key::for_test();
        let ana = account(database.connection(), "ana");
        let group = group(&mut database, &ana.id);

        let (_, secret) = create(
            database.connection(),
            &key,
            &group.id,
            GroupRole::Member,
            None,
            &ana.id,
            14,
        )
        .unwrap();

        let stored: String = database
            .connection()
            .query_one("SELECT token_hash FROM invitations", [], |row| row.get(0))
            .unwrap();

        assert_ne!(stored, secret);
    }

    #[test]
    fn an_invitation_is_for_one_person_only() {
        let mut database = test_database();
        let key = Key::for_test();
        let ana = account(database.connection(), "ana");
        let enric = account(database.connection(), "enric");
        let group = group(&mut database, &ana.id);

        let (invitation, _) = create(
            database.connection(),
            &key,
            &group.id,
            GroupRole::Member,
            None,
            &ana.id,
            14,
        )
        .unwrap();

        assert!(accept(database.connection(), &invitation.id, &enric.id).unwrap());
        assert!(!accept(database.connection(), &invitation.id, &enric.id).unwrap());

        let used = by_id(database.connection(), &invitation.id)
            .unwrap()
            .unwrap();
        assert_eq!(used.accepted_by, Some(enric.id));
        assert_eq!(
            used.refusal("2026-09-19 00:00:00"),
            Some(Refusal::AlreadyUsed)
        );
    }

    #[test]
    fn a_withdrawn_invitation_stops_working_and_cannot_be_withdrawn_twice() {
        let mut database = test_database();
        let key = Key::for_test();
        let ana = account(database.connection(), "ana");
        let group = group(&mut database, &ana.id);

        let (invitation, _) = create(
            database.connection(),
            &key,
            &group.id,
            GroupRole::Admin,
            None,
            &ana.id,
            14,
        )
        .unwrap();

        assert_eq!(
            live_for_group(database.connection(), &group.id)
                .unwrap()
                .len(),
            1
        );
        assert!(revoke(database.connection(), &invitation.id, &ana.id).unwrap());
        assert!(!revoke(database.connection(), &invitation.id, &ana.id).unwrap());
        assert!(
            live_for_group(database.connection(), &group.id)
                .unwrap()
                .is_empty()
        );
        assert!(!accept(database.connection(), &invitation.id, &ana.id).unwrap());
    }

    #[test]
    fn an_expired_invitation_is_refused_and_says_which_way_it_failed() {
        let mut database = test_database();
        let key = Key::for_test();
        let ana = account(database.connection(), "ana");
        let group = group(&mut database, &ana.id);

        let (invitation, secret) = create(
            database.connection(),
            &key,
            &group.id,
            GroupRole::Member,
            None,
            &ana.id,
            0,
        )
        .unwrap();

        database
            .connection()
            .execute(
                "UPDATE invitations SET expires_at = datetime('now', '-1 day') WHERE id = ?1",
                [invitation.id.to_string()],
            )
            .unwrap();

        let stale = by_secret(database.connection(), &key, &secret)
            .unwrap()
            .unwrap();

        assert_eq!(
            stale.refusal(&now(database.connection())),
            Some(Refusal::Expired)
        );
        assert!(!accept(database.connection(), &invitation.id, &ana.id).unwrap());
    }

    fn now(connection: &Connection) -> String {
        connection
            .query_one("SELECT datetime('now')", [], |row| row.get(0))
            .unwrap()
    }
}

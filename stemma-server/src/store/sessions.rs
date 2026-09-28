//! The hold a browser has on an account.
//!
//! The cookie carries an identifier and a secret; the database keeps the
//! identifier and a keyed hash of the secret. Holding a session is not the
//! same as having just proved who you are, which is why the row also
//! remembers when the password was last typed: the admin area asks about
//! that and not about the session.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::id::{SessionId, UserId};

use super::id_at;
use super::users::{self, User};
use crate::tokens::Key;

/// How long a session lasts without being renewed.
pub const LIFETIME_DAYS: u32 = 30;

/// How recently the password must have been typed for the admin area.
pub const REAUTHENTICATION_MINUTES: u32 = 30;

/// A live session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The session's identifier, which travels in the cookie.
    pub id: SessionId,
    /// Whose it is.
    pub user: UserId,
    /// What the forms of this session must carry back.
    pub csrf_token: String,
    /// When it started.
    pub created_at: String,
    /// When it stops working.
    pub expires_at: String,
    /// When the password was last typed on it.
    pub reauthenticated_at: Option<String>,
}

impl Session {
    /// Reads a session out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: id_at(row, "id")?,
            user: id_at(row, "user_id")?,
            csrf_token: row.get("csrf_token")?,
            created_at: row.get("created_at")?,
            expires_at: row.get("expires_at")?,
            reauthenticated_at: row.get("reauthenticated_at")?,
        })
    }
}

/// What the browser is given, and what it hands back.
///
/// The identifier is in it so that a lookup is one indexed read rather than
/// a scan, and the secret is what makes the identifier worth anything.
pub fn cookie_value(id: &SessionId, secret: &str) -> String {
    format!("{id}.{secret}")
}

/// Splits a cookie back into its two halves.
fn split_cookie(cookie: &str) -> Option<(SessionId, &str)> {
    let (id, secret) = cookie.rsplit_once('.')?;

    Some((id.parse().ok()?, secret))
}

/// Starts a session, and returns the cookie for it.
pub fn start(connection: &Connection, key: &Key, user: &UserId) -> Result<(Session, String)> {
    let id = SessionId::generate();
    let secret = crate::tokens::secret()?;
    let csrf_token = crate::tokens::secret()?;

    connection
        .execute(
            "INSERT INTO sessions
                 (id, user_id, token_hash, csrf_token, expires_at, reauthenticated_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now', ?5), datetime('now'))",
            params![
                id.to_string(),
                user.to_string(),
                key.fingerprint(&secret),
                csrf_token,
                format!("+{LIFETIME_DAYS} days")
            ],
        )
        .context("cannot start the session")?;

    let session = by_id(connection, &id)?.context("the session cannot be read back")?;

    Ok((session, cookie_value(&id, &secret)))
}

/// One session.
pub fn by_id(connection: &Connection, id: &SessionId) -> Result<Option<Session>> {
    connection
        .query_row(
            "SELECT id, user_id, csrf_token, created_at, expires_at, reauthenticated_at
               FROM sessions WHERE id = ?1",
            [id.to_string()],
            Session::from_row,
        )
        .optional()
        .context("cannot read the session")
}

/// Who a cookie belongs to, if it belongs to anybody.
///
/// A cookie for a session that has expired, or whose account is suspended
/// or gone, is nobody's.
pub fn lookup(connection: &Connection, key: &Key, cookie: &str) -> Result<Option<(Session, User)>> {
    let Some((id, secret)) = split_cookie(cookie) else {
        return Ok(None);
    };

    let found: Option<Session> = connection
        .query_row(
            "SELECT id, user_id, csrf_token, created_at, expires_at, reauthenticated_at
               FROM sessions
              WHERE id = ?1 AND token_hash = ?2 AND expires_at > datetime('now')",
            params![id.to_string(), key.fingerprint(secret)],
            Session::from_row,
        )
        .optional()
        .context("cannot read the session")?;

    let Some(session) = found else {
        return Ok(None);
    };

    let Some(user) = users::by_id(connection, &session.user)? else {
        return Ok(None);
    };

    if !user.is_active() {
        return Ok(None);
    }

    Ok(Some((session, user)))
}

/// Notes that the session was used, so it does not look abandoned.
pub fn touch(connection: &Connection, id: &SessionId) -> Result<()> {
    connection
        .execute(
            "UPDATE sessions SET last_seen_at = datetime('now') WHERE id = ?1",
            [id.to_string()],
        )
        .context("cannot record the session's use")?;

    Ok(())
}

/// Notes that the password was typed again on this session.
pub fn mark_reauthenticated(connection: &Connection, id: &SessionId) -> Result<()> {
    connection
        .execute(
            "UPDATE sessions SET reauthenticated_at = datetime('now') WHERE id = ?1",
            [id.to_string()],
        )
        .context("cannot record the authentication")?;

    Ok(())
}

/// Whether the password was typed recently enough for the admin area.
pub fn is_fresh(connection: &Connection, id: &SessionId) -> Result<bool> {
    let fresh: i64 = connection
        .query_one(
            "SELECT count(*) FROM sessions
              WHERE id = ?1
                AND reauthenticated_at IS NOT NULL
                AND reauthenticated_at > datetime('now', ?2)",
            params![
                id.to_string(),
                format!("-{REAUTHENTICATION_MINUTES} minutes")
            ],
            |row| row.get(0),
        )
        .context("cannot read the session")?;

    Ok(fresh > 0)
}

/// Ends one session.
pub fn end(connection: &Connection, id: &SessionId) -> Result<()> {
    connection
        .execute("DELETE FROM sessions WHERE id = ?1", [id.to_string()])
        .context("cannot end the session")?;

    Ok(())
}

/// Ends every session an account holds, wherever it is held.
pub fn end_all(connection: &Connection, user: &UserId) -> Result<usize> {
    connection
        .execute(
            "DELETE FROM sessions WHERE user_id = ?1",
            [user.to_string()],
        )
        .context("cannot end the sessions")
}

/// Ends every session an account holds except the one that asked.
///
/// Changing a password from a session keeps that session: the person who
/// just typed both passwords is the one it belongs to.
pub fn end_others(connection: &Connection, user: &UserId, keep: &SessionId) -> Result<usize> {
    connection
        .execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND id <> ?2",
            params![user.to_string(), keep.to_string()],
        )
        .context("cannot end the other sessions")
}

/// Removes sessions nobody can use any more.
pub fn clear_expired(connection: &Connection) -> Result<usize> {
    connection
        .execute(
            "DELETE FROM sessions WHERE expires_at <= datetime('now')",
            [],
        )
        .context("cannot clear the expired sessions")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_database;
    use stemma_core::id::Slug;

    fn account(connection: &Connection, name: &str) -> User {
        users::create(
            connection,
            &Slug::parse(name).unwrap(),
            &format!("{name}@example.org"),
            name,
            "hash",
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn a_cookie_finds_the_session_it_was_given_for() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection, "ana");

        let (session, cookie) = start(connection, &key, &ana.id).unwrap();
        let (found, user) = lookup(connection, &key, &cookie).unwrap().unwrap();

        assert_eq!(found.id, session.id);
        assert_eq!(user.id, ana.id);
    }

    #[test]
    fn a_cookie_is_worth_nothing_without_the_key_that_made_it() {
        let database = test_database();
        let connection = database.connection();
        let ana = account(connection, "ana");

        let (_, cookie) = start(connection, &Key::for_test(), &ana.id).unwrap();
        let other = Key::from_material(b"another machine's key");

        assert!(lookup(connection, &other, &cookie).unwrap().is_none());
    }

    #[test]
    fn a_tampered_cookie_lets_nobody_in() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection, "ana");

        let (session, cookie) = start(connection, &key, &ana.id).unwrap();

        for spoiled in [
            cookie.replace('.', ""),
            format!("{}.{}", session.id, "0".repeat(64)),
            "nonsense".to_owned(),
            String::new(),
        ] {
            assert!(
                lookup(connection, &key, &spoiled).unwrap().is_none(),
                "`{spoiled}` was accepted"
            );
        }
    }

    #[test]
    fn a_suspended_account_holds_no_session() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection, "ana");
        let (_, cookie) = start(connection, &key, &ana.id).unwrap();

        users::set_suspended(connection, &ana.id, true).unwrap();

        assert!(lookup(connection, &key, &cookie).unwrap().is_none());
    }

    #[test]
    fn an_expired_session_is_nobodys_and_gets_cleared() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection, "ana");
        let (session, cookie) = start(connection, &key, &ana.id).unwrap();

        connection
            .execute(
                "UPDATE sessions SET expires_at = datetime('now', '-1 day') WHERE id = ?1",
                [session.id.to_string()],
            )
            .unwrap();

        assert!(lookup(connection, &key, &cookie).unwrap().is_none());
        assert_eq!(clear_expired(connection).unwrap(), 1);
    }

    #[test]
    fn a_session_starts_fresh_and_goes_stale() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection, "ana");
        let (session, _) = start(connection, &key, &ana.id).unwrap();

        assert!(is_fresh(connection, &session.id).unwrap());

        connection
            .execute(
                "UPDATE sessions SET reauthenticated_at = datetime('now', '-2 hours')
                  WHERE id = ?1",
                [session.id.to_string()],
            )
            .unwrap();

        assert!(!is_fresh(connection, &session.id).unwrap());

        mark_reauthenticated(connection, &session.id).unwrap();
        assert!(is_fresh(connection, &session.id).unwrap());
    }

    #[test]
    fn ending_every_session_signs_a_person_out_everywhere() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection, "ana");

        let (_, laptop) = start(connection, &key, &ana.id).unwrap();
        let (_, desktop) = start(connection, &key, &ana.id).unwrap();

        assert_eq!(end_all(connection, &ana.id).unwrap(), 2);
        assert!(lookup(connection, &key, &laptop).unwrap().is_none());
        assert!(lookup(connection, &key, &desktop).unwrap().is_none());
    }

    #[test]
    fn ending_the_other_sessions_keeps_the_one_that_asked() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection, "ana");
        let bo = account(connection, "bo");

        let (here, kept) = start(connection, &key, &ana.id).unwrap();
        let (_, elsewhere) = start(connection, &key, &ana.id).unwrap();
        let (_, somebody_else) = start(connection, &key, &bo.id).unwrap();

        assert_eq!(end_others(connection, &ana.id, &here.id).unwrap(), 1);
        assert!(lookup(connection, &key, &kept).unwrap().is_some());
        assert!(lookup(connection, &key, &elsewhere).unwrap().is_none());
        assert!(lookup(connection, &key, &somebody_else).unwrap().is_some());
    }

    #[test]
    fn two_sessions_of_one_person_have_different_forms_to_sign() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection, "ana");

        let (one, _) = start(connection, &key, &ana.id).unwrap();
        let (other, _) = start(connection, &key, &ana.id).unwrap();

        assert_ne!(one.csrf_token, other.csrf_token);
    }
}

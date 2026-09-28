//! Signing a machine in.
//!
//! A researcher runs `stemma login` on a laptop or over SSH, the machine
//! shows a short code, and the person types it into a browser they are
//! already signed in on. Nothing has to come back to the machine's own
//! address, which is what makes it work through a terminal on somebody
//! else's computer.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::id::{SessionId, UniqueId, UserId};

use super::optional_id_at;
use crate::tokens::Key;

/// How long a person has to type the code in.
pub const LIFETIME_MINUTES: u32 = 15;

/// How often the machine should ask whether it has been approved.
pub const POLL_SECONDS: u32 = 5;

/// How many characters the code a person reads has.
pub const CODE_LENGTH: usize = 8;

/// Identifiers of an attempt and of the credential it produces.
type GrantId = UniqueId<stemma_core::id::Machine>;
type TokenId = SessionId;

/// One attempt to sign a machine in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// The attempt's identifier.
    pub id: String,
    /// The code the person reads and types.
    pub user_code: String,
    /// What the machine calls itself.
    pub label: String,
    /// When the attempt stops being usable.
    pub expires_at: String,
    /// When somebody approved it.
    pub approved_at: Option<String>,
    /// Who approved it.
    pub approved_by: Option<UserId>,
    /// When somebody refused it.
    pub denied_at: Option<String>,
    /// The credential it produced, once the machine has collected one.
    pub token_id: Option<String>,
}

impl Grant {
    /// Reads an attempt out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            user_code: row.get("user_code")?,
            label: row.get("label")?,
            expires_at: row.get("expires_at")?,
            approved_at: row.get("approved_at")?,
            approved_by: optional_id_at(row, "approved_by")?,
            denied_at: row.get("denied_at")?,
            token_id: row.get("token_id")?,
        })
    }
}

/// How a code a person typed is stored and compared.
///
/// They read it off one screen and type it into another, so the spacing
/// and the case they use are theirs to choose.
pub fn normalise(code: &str) -> String {
    code.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}

/// How a code is shown, which is in two halves so it can be read aloud.
pub fn readable(code: &str) -> String {
    match code.len() {
        length if length > 4 => format!("{}-{}", &code[..4], &code[4..]),
        _ => code.to_owned(),
    }
}

/// Starts an attempt, and returns the secret the machine keeps.
pub fn start(connection: &Connection, key: &Key, label: &str) -> Result<(Grant, String)> {
    let id = GrantId::generate().to_string();
    let device_code = crate::tokens::secret()?;
    let user_code = crate::tokens::readable_code(CODE_LENGTH)?;

    connection
        .execute(
            "INSERT INTO device_grants (id, device_code_hash, user_code, label, expires_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now', ?5))",
            params![
                id,
                key.fingerprint(&device_code),
                user_code,
                label.trim(),
                format!("+{LIFETIME_MINUTES} minutes")
            ],
        )
        .context("cannot start the sign-in")?;

    let grant = by_id(connection, &id)?.context("the sign-in cannot be read back")?;

    Ok((grant, device_code))
}

/// One attempt.
pub fn by_id(connection: &Connection, id: &str) -> Result<Option<Grant>> {
    read(connection, "id = ?1", [id])
}

/// The attempt a person's code names, if it is still live.
pub fn by_user_code(connection: &Connection, code: &str) -> Result<Option<Grant>> {
    read(
        connection,
        "user_code = ?1 AND expires_at > datetime('now')",
        [normalise(code)],
    )
}

/// The attempt a machine's secret names, if it is still live.
pub fn by_device_code(connection: &Connection, key: &Key, secret: &str) -> Result<Option<Grant>> {
    read(
        connection,
        "device_code_hash = ?1 AND expires_at > datetime('now')",
        [key.fingerprint(secret)],
    )
}

/// Reads one attempt.
fn read<P>(connection: &Connection, condition: &str, parameters: P) -> Result<Option<Grant>>
where
    P: rusqlite::Params,
{
    connection
        .query_row(
            &format!(
                "SELECT id, user_code, label, expires_at, approved_at, approved_by,
                        denied_at, token_id
                   FROM device_grants WHERE {condition}"
            ),
            parameters,
            Grant::from_row,
        )
        .optional()
        .context("cannot read the sign-in")
}

/// Records that somebody approved the attempt.
pub fn approve(connection: &Connection, id: &str, user: &UserId) -> Result<bool> {
    let approved = connection
        .execute(
            "UPDATE device_grants
                SET approved_at = datetime('now'), approved_by = ?2
              WHERE id = ?1
                AND approved_at IS NULL AND denied_at IS NULL
                AND expires_at > datetime('now')",
            params![id, user.to_string()],
        )
        .context("cannot approve the sign-in")?;

    Ok(approved > 0)
}

/// Records that somebody refused it.
pub fn deny(connection: &Connection, id: &str) -> Result<bool> {
    let denied = connection
        .execute(
            "UPDATE device_grants
                SET denied_at = datetime('now')
              WHERE id = ?1 AND approved_at IS NULL AND denied_at IS NULL",
            [id],
        )
        .context("cannot refuse the sign-in")?;

    Ok(denied > 0)
}

/// What a machine asking for its credential is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Collected {
    /// Nobody has answered yet.
    Waiting,
    /// Somebody said no.
    Denied,
    /// Nobody answered in time, or there is no such attempt.
    Gone,
    /// Here is the credential, once.
    Issued {
        /// Whose account the machine now acts for.
        user: UserId,
        /// The secret the machine keeps.
        token: String,
    },
}

/// Hands the machine its credential, if there is one to hand it.
///
/// The credential is made at this moment rather than when the person
/// approved, so a secret nobody has collected is never sitting in the
/// database waiting to be read.
pub fn collect(connection: &mut Connection, key: &Key, device_code: &str) -> Result<Collected> {
    let transaction = connection.transaction()?;

    let Some(grant) = by_device_code(&transaction, key, device_code)? else {
        return Ok(Collected::Gone);
    };

    if grant.denied_at.is_some() {
        return Ok(Collected::Denied);
    }

    let (Some(user), true) = (grant.approved_by.clone(), grant.approved_at.is_some()) else {
        return Ok(Collected::Waiting);
    };

    // A machine that asks twice is told nothing the second time: the
    // credential was already handed over, and there is only one.
    if grant.token_id.is_some() {
        return Ok(Collected::Gone);
    }

    let (token_id, token) = issue_token(&transaction, key, &user, &grant.label)?;

    transaction
        .execute(
            "UPDATE device_grants SET token_id = ?2 WHERE id = ?1",
            params![grant.id, token_id],
        )
        .context("cannot record the credential")?;

    transaction.commit()?;

    Ok(Collected::Issued { user, token })
}

/// Makes a credential for a machine.
fn issue_token(
    connection: &Connection,
    key: &Key,
    user: &UserId,
    label: &str,
) -> Result<(String, String)> {
    let id = TokenId::generate().to_string();
    let token = crate::tokens::secret()?;

    connection
        .execute(
            "INSERT INTO client_tokens (id, user_id, label, token_hash)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, user.to_string(), label, key.fingerprint(&token)],
        )
        .context("cannot record the credential")?;

    Ok((id, token))
}

/// Whose account a machine's credential acts for.
pub fn holder(connection: &Connection, key: &Key, token: &str) -> Result<Option<UserId>> {
    Ok(credential(connection, key, token)?.map(|(_, user)| user))
}

/// The credential itself, and whose account it acts for.
///
/// The row's identifier is what a machine is registered against, so a
/// machine keeps its identity across everything except signing in again.
pub fn credential(
    connection: &Connection,
    key: &Key,
    token: &str,
) -> Result<Option<(String, UserId)>> {
    let found: Option<(String, String)> = connection
        .query_row(
            "SELECT id, user_id FROM client_tokens
              WHERE token_hash = ?1 AND revoked_at IS NULL",
            [key.fingerprint(token)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .context("cannot read the credential")?;

    let Some((id, user)) = found else {
        return Ok(None);
    };

    connection
        .execute(
            "UPDATE client_tokens SET last_seen_at = datetime('now') WHERE token_hash = ?1",
            [key.fingerprint(token)],
        )
        .context("cannot record the credential's use")?;

    Ok(user.parse().ok().map(|user| (id, user)))
}

/// Stops a machine's credential working, from wherever it is revoked.
pub fn revoke(connection: &Connection, key: &Key, token: &str) -> Result<bool> {
    let revoked = connection
        .execute(
            "UPDATE client_tokens
                SET revoked_at = datetime('now')
              WHERE token_hash = ?1 AND revoked_at IS NULL",
            [key.fingerprint(token)],
        )
        .context("cannot revoke the credential")?;

    Ok(revoked > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{test_database, users};
    use stemma_core::id::Slug;

    fn account(connection: &Connection) -> UserId {
        users::create(
            connection,
            &Slug::parse("ana").unwrap(),
            "ana@example.org",
            "Ana",
            "hash",
        )
        .unwrap()
        .unwrap()
        .id
    }

    #[test]
    fn a_code_is_read_and_typed_however_somebody_likes() {
        assert_eq!(normalise("abcd-efgh"), "ABCDEFGH");
        assert_eq!(normalise(" ABCD EFGH "), "ABCDEFGH");
        assert_eq!(readable("ABCDEFGH"), "ABCD-EFGH");
    }

    #[test]
    fn a_machine_waits_until_somebody_answers_and_then_gets_one_credential() {
        let mut database = test_database();
        let key = Key::for_test();
        let ana = account(database.connection());

        let (grant, device_code) = start(database.connection(), &key, "ana's laptop").unwrap();

        assert_eq!(
            collect(database.connection_mut(), &key, &device_code).unwrap(),
            Collected::Waiting
        );

        // The person finds their own attempt by the code on the screen.
        let found = by_user_code(database.connection(), &readable(&grant.user_code))
            .unwrap()
            .unwrap();
        assert_eq!(found.id, grant.id);
        assert!(approve(database.connection(), &found.id, &ana).unwrap());

        let Collected::Issued { user, token } =
            collect(database.connection_mut(), &key, &device_code).unwrap()
        else {
            panic!("the machine was not given a credential");
        };

        assert_eq!(user, ana);
        assert_eq!(
            holder(database.connection(), &key, &token).unwrap(),
            Some(ana)
        );

        // Asking again gets nothing: there was one credential and it is out.
        assert_eq!(
            collect(database.connection_mut(), &key, &device_code).unwrap(),
            Collected::Gone
        );
    }

    #[test]
    fn a_refused_attempt_says_so_rather_than_timing_out() {
        let mut database = test_database();
        let key = Key::for_test();
        account(database.connection());

        let (grant, device_code) = start(database.connection(), &key, "a laptop").unwrap();

        assert!(deny(database.connection(), &grant.id).unwrap());
        assert_eq!(
            collect(database.connection_mut(), &key, &device_code).unwrap(),
            Collected::Denied
        );
    }

    #[test]
    fn a_refused_attempt_cannot_then_be_approved() {
        let database = test_database();
        let key = Key::for_test();
        let ana = account(database.connection());

        let (grant, _) = start(database.connection(), &key, "a laptop").unwrap();

        assert!(deny(database.connection(), &grant.id).unwrap());
        assert!(!approve(database.connection(), &grant.id, &ana).unwrap());
    }

    #[test]
    fn an_attempt_nobody_answered_in_time_is_gone() {
        let mut database = test_database();
        let key = Key::for_test();
        let ana = account(database.connection());

        let (grant, device_code) = start(database.connection(), &key, "a laptop").unwrap();

        database
            .connection()
            .execute(
                "UPDATE device_grants SET expires_at = datetime('now', '-1 minute')",
                [],
            )
            .unwrap();

        assert!(
            by_user_code(database.connection(), &grant.user_code)
                .unwrap()
                .is_none()
        );
        assert!(!approve(database.connection(), &grant.id, &ana).unwrap());
        assert_eq!(
            collect(database.connection_mut(), &key, &device_code).unwrap(),
            Collected::Gone
        );
    }

    #[test]
    fn a_revoked_credential_acts_for_nobody() {
        let mut database = test_database();
        let key = Key::for_test();
        let ana = account(database.connection());

        let (grant, device_code) = start(database.connection(), &key, "a laptop").unwrap();
        approve(database.connection(), &grant.id, &ana).unwrap();

        let Collected::Issued { token, .. } =
            collect(database.connection_mut(), &key, &device_code).unwrap()
        else {
            panic!("no credential");
        };

        assert!(revoke(database.connection(), &key, &token).unwrap());
        assert!(!revoke(database.connection(), &key, &token).unwrap());
        assert_eq!(holder(database.connection(), &key, &token).unwrap(), None);
    }
}

//! Getting back into an account.
//!
//! A reset is a link, like an invitation: single use, expiring, and kept
//! only as a hash. Where an instance has no outgoing mail this is the only
//! way back in, so it works the same whether it is printed on the machine
//! or delivered by mail.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use stemma_core::id::{UniqueId, UserId};

use crate::tokens::Key;

/// How long a reset link lasts.
pub const LIFETIME_HOURS: u32 = 2;

/// Kinds of identifier this table uses.
type ResetId = UniqueId<stemma_core::id::Session>;

/// Makes a reset link, and returns the secret once.
pub fn create(connection: &Connection, key: &Key, user: &UserId) -> Result<String> {
    let secret = crate::tokens::secret()?;

    connection
        .execute(
            "INSERT INTO password_resets (id, user_id, token_hash, expires_at)
             VALUES (?1, ?2, ?3, datetime('now', ?4))",
            params![
                ResetId::generate().to_string(),
                user.to_string(),
                key.fingerprint(&secret),
                format!("+{LIFETIME_HOURS} hours")
            ],
        )
        .context("cannot create the reset link")?;

    Ok(secret)
}

/// Whose account a link belongs to, if it still works.
pub fn holder(connection: &Connection, key: &Key, secret: &str) -> Result<Option<UserId>> {
    let found: Option<String> = connection
        .query_row(
            "SELECT user_id FROM password_resets
              WHERE token_hash = ?1 AND used_at IS NULL AND expires_at > datetime('now')",
            [key.fingerprint(secret)],
            |row| row.get(0),
        )
        .optional()
        .context("cannot read the reset link")?;

    Ok(found.and_then(|raw| raw.parse().ok()))
}

/// Uses a link up, and says whether it was still there to use.
pub fn spend(connection: &Connection, key: &Key, secret: &str) -> Result<bool> {
    let spent = connection
        .execute(
            "UPDATE password_resets
                SET used_at = datetime('now')
              WHERE token_hash = ?1 AND used_at IS NULL AND expires_at > datetime('now')",
            [key.fingerprint(secret)],
        )
        .context("cannot use the reset link")?;

    Ok(spent > 0)
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
    fn a_link_names_its_account_once() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection);

        let secret = create(connection, &key, &ana).unwrap();

        assert_eq!(holder(connection, &key, &secret).unwrap(), Some(ana));
        assert!(spend(connection, &key, &secret).unwrap());
        assert!(!spend(connection, &key, &secret).unwrap());
        assert_eq!(holder(connection, &key, &secret).unwrap(), None);
    }

    #[test]
    fn a_link_that_ran_out_of_time_names_nobody() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        let ana = account(connection);

        let secret = create(connection, &key, &ana).unwrap();
        connection
            .execute(
                "UPDATE password_resets SET expires_at = datetime('now', '-1 minute')",
                [],
            )
            .unwrap();

        assert_eq!(holder(connection, &key, &secret).unwrap(), None);
        assert!(!spend(connection, &key, &secret).unwrap());
    }

    #[test]
    fn a_secret_from_somewhere_else_names_nobody() {
        let database = test_database();
        let connection = database.connection();
        let key = Key::for_test();
        account(connection);

        assert_eq!(holder(connection, &key, "not a secret").unwrap(), None);
    }
}

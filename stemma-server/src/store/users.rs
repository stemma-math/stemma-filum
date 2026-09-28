//! Accounts, and the grants that let one operate the instance.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::access::Grant;
use stemma_core::id::{GrantId, Slug, UserId};

use super::id_at;

/// An account on this instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// The account's identifier, `usr:ana`.
    pub id: UserId,
    /// Where mail to this person goes, and what they log in with.
    pub email: String,
    /// What to call them on screen.
    pub display_name: String,
    /// When the account was created.
    pub created_at: String,
    /// When it was suspended, if it was.
    pub suspended_at: Option<String>,
}

impl User {
    /// Whether the account may be used at all.
    pub fn is_active(&self) -> bool {
        self.suspended_at.is_none()
    }

    /// Reads an account out of a row.
    pub(super) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: id_at(row, "id")?,
            email: row.get("email")?,
            display_name: row.get("display_name")?,
            created_at: row.get("created_at")?,
            suspended_at: row.get("suspended_at")?,
        })
    }
}

/// The columns every read of an account asks for.
const COLUMNS: &str = "id, slug, email, display_name, created_at, suspended_at";

/// Why an account could not be created.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotCreated {
    /// Somebody already holds that name.
    #[error("`{0}` is already taken")]
    SlugTaken(Slug),
    /// Somebody already registered with that address.
    #[error("there is already an account for {0}")]
    EmailTaken(String),
}

/// Creates an account.
pub fn create(
    connection: &Connection,
    slug: &Slug,
    email: &str,
    display_name: &str,
    password_hash: &str,
) -> Result<Result<User, NotCreated>> {
    let id = UserId::new(slug.clone());
    let email = email.trim().to_lowercase();

    if by_slug(connection, slug)?.is_some() {
        return Ok(Err(NotCreated::SlugTaken(slug.clone())));
    }
    if by_email(connection, &email)?.is_some() {
        return Ok(Err(NotCreated::EmailTaken(email)));
    }

    connection
        .execute(
            "INSERT INTO users (id, slug, email, display_name, password_hash)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.to_string(),
                slug.as_str(),
                email,
                display_name.trim(),
                password_hash
            ],
        )
        .context("cannot create the account")?;

    by_id(connection, &id)?
        .map(Ok)
        .context("the account was created and cannot be read back")
}

/// The account with this identifier.
pub fn by_id(connection: &Connection, id: &UserId) -> Result<Option<User>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM users WHERE id = ?1"),
            [id.to_string()],
            User::from_row,
        )
        .optional()
        .context("cannot read the account")
}

/// The account with this name.
pub fn by_slug(connection: &Connection, slug: &Slug) -> Result<Option<User>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM users WHERE slug = ?1"),
            [slug.as_str()],
            User::from_row,
        )
        .optional()
        .context("cannot read the account")
}

/// The account registered with this address.
pub fn by_email(connection: &Connection, email: &str) -> Result<Option<User>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM users WHERE email = ?1"),
            [email.trim().to_lowercase()],
            User::from_row,
        )
        .optional()
        .context("cannot read the account")
}

/// The account somebody typed the name or the address of.
///
/// Logging in accepts either, because people remember one or the other and
/// the group is never part of it.
pub fn by_login(connection: &Connection, login: &str) -> Result<Option<User>> {
    let login = login.trim();

    if let Some(user) = by_email(connection, login)? {
        return Ok(Some(user));
    }

    match Slug::parse(&login.to_lowercase()) {
        Ok(slug) => by_slug(connection, &slug),
        Err(_) => Ok(None),
    }
}

/// How many accounts there are.
pub fn count(connection: &Connection) -> Result<i64> {
    connection
        .query_one("SELECT count(*) FROM users", [], |row| row.get(0))
        .context("cannot count the accounts")
}

/// Everybody, oldest first.
pub fn all(connection: &Connection) -> Result<Vec<User>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM users ORDER BY created_at, id"
    ))?;
    let users = statement
        .query_map([], User::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the accounts")?;

    Ok(users)
}

/// The stored hash of an account's password.
pub fn password_hash(connection: &Connection, id: &UserId) -> Result<Option<String>> {
    connection
        .query_row(
            "SELECT password_hash FROM users WHERE id = ?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .context("cannot read the password")
}

/// Replaces an account's password.
pub fn set_password(connection: &Connection, id: &UserId, password_hash: &str) -> Result<()> {
    connection
        .execute(
            "UPDATE users SET password_hash = ?2 WHERE id = ?1",
            params![id.to_string(), password_hash],
        )
        .context("cannot change the password")?;

    Ok(())
}

/// Changes what somebody is called on screen.
pub fn set_display_name(connection: &Connection, id: &UserId, display_name: &str) -> Result<()> {
    connection
        .execute(
            "UPDATE users SET display_name = ?2 WHERE id = ?1",
            params![id.to_string(), display_name.trim()],
        )
        .context("cannot change the name")?;

    Ok(())
}

/// Moves an account to another address, unless somebody else has it.
///
/// The address is also a way to log in, so two accounts can never share
/// one. Keeping one's own address is not a change and not an error.
pub fn set_email(
    connection: &Connection,
    id: &UserId,
    email: &str,
) -> Result<Result<(), NotCreated>> {
    let email = email.trim().to_lowercase();

    if by_email(connection, &email)?.is_some_and(|holder| holder.id != *id) {
        return Ok(Err(NotCreated::EmailTaken(email)));
    }

    connection
        .execute(
            "UPDATE users SET email = ?2 WHERE id = ?1",
            params![id.to_string(), email],
        )
        .context("cannot change the address")?;

    Ok(Ok(()))
}

/// Stops an account being used, or lets it be used again.
pub fn set_suspended(connection: &Connection, id: &UserId, suspended: bool) -> Result<()> {
    let clause = match suspended {
        true => "datetime('now')",
        false => "NULL",
    };

    connection
        .execute(
            &format!("UPDATE users SET suspended_at = {clause} WHERE id = ?1"),
            [id.to_string()],
        )
        .context("cannot change the account")?;

    Ok(())
}

/// One instance-level permission somebody holds, or held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldGrant {
    /// Which permission.
    pub grant: Grant,
    /// Who holds it.
    pub user: UserId,
    /// When it was given.
    pub granted_at: String,
    /// Who gave it, where that was a person and not the machine.
    pub granted_by: Option<UserId>,
}

/// Gives somebody an instance-level permission.
///
/// Doing it twice is not an error and does not make a second record: the
/// question a grant answers is whether they hold it now.
pub fn grant(
    connection: &Connection,
    user: &UserId,
    grant: Grant,
    by: Option<&UserId>,
) -> Result<bool> {
    if holds(connection, user, grant)? {
        return Ok(false);
    }

    connection
        .execute(
            "INSERT INTO instance_grants (id, user_id, kind, granted_by)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                GrantId::generate().to_string(),
                user.to_string(),
                grant.as_str(),
                by.map(ToString::to_string)
            ],
        )
        .context("cannot record the grant")?;

    Ok(true)
}

/// Takes an instance-level permission away, keeping the record that it was
/// once held.
pub fn revoke(
    connection: &Connection,
    user: &UserId,
    grant: Grant,
    by: Option<&UserId>,
) -> Result<bool> {
    let changed = connection
        .execute(
            "UPDATE instance_grants
                SET revoked_at = datetime('now'), revoked_by = ?3
              WHERE user_id = ?1 AND kind = ?2 AND revoked_at IS NULL",
            params![
                user.to_string(),
                grant.as_str(),
                by.map(ToString::to_string)
            ],
        )
        .context("cannot record the revocation")?;

    Ok(changed > 0)
}

/// Whether somebody holds a permission right now.
pub fn holds(connection: &Connection, user: &UserId, grant: Grant) -> Result<bool> {
    let count: i64 = connection
        .query_one(
            "SELECT count(*) FROM instance_grants
              WHERE user_id = ?1 AND kind = ?2 AND revoked_at IS NULL",
            params![user.to_string(), grant.as_str()],
            |row| row.get(0),
        )
        .context("cannot read the grants")?;

    Ok(count > 0)
}

/// Everybody who holds a permission right now.
pub fn holders(connection: &Connection, grant: Grant) -> Result<Vec<HeldGrant>> {
    let mut statement = connection.prepare(
        "SELECT user_id, granted_at, granted_by FROM instance_grants
          WHERE kind = ?1 AND revoked_at IS NULL
          ORDER BY granted_at",
    )?;

    let held = statement
        .query_map([grant.as_str()], |row| {
            Ok(HeldGrant {
                grant,
                user: id_at(row, "user_id")?,
                granted_at: row.get("granted_at")?,
                granted_by: row
                    .get::<_, Option<String>>("granted_by")?
                    .and_then(|raw| raw.parse().ok()),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list who holds the grant")?;

    Ok(held)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_database;

    fn slug(text: &str) -> Slug {
        Slug::parse(text).unwrap()
    }

    fn ana(connection: &Connection) -> User {
        create(
            connection,
            &slug("ana"),
            "Ana@Example.org",
            "Ana Ferrer",
            "hash",
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn an_account_can_be_found_by_name_or_by_address() {
        let database = test_database();
        let connection = database.connection();
        let user = ana(connection);

        assert_eq!(user.id.to_string(), "usr:ana");
        assert_eq!(by_login(connection, "ana").unwrap(), Some(user.clone()));
        assert_eq!(
            by_login(connection, "  ANA@example.org ").unwrap(),
            Some(user)
        );
        assert_eq!(by_login(connection, "nobody").unwrap(), None);
    }

    #[test]
    fn an_address_is_stored_the_way_it_will_be_looked_up() {
        let database = test_database();
        let user = ana(database.connection());

        assert_eq!(user.email, "ana@example.org");
    }

    #[test]
    fn a_name_or_an_address_is_taken_only_once() {
        let database = test_database();
        let connection = database.connection();
        ana(connection);

        assert_eq!(
            create(connection, &slug("ana"), "other@example.org", "Other", "h").unwrap(),
            Err(NotCreated::SlugTaken(slug("ana")))
        );
        assert_eq!(
            create(connection, &slug("ana-f"), "ana@example.org", "Other", "h").unwrap(),
            Err(NotCreated::EmailTaken("ana@example.org".to_owned()))
        );
    }

    #[test]
    fn a_grant_is_given_once_and_leaves_a_record_when_it_goes() {
        let database = test_database();
        let connection = database.connection();
        let user = ana(connection);

        assert!(grant(connection, &user.id, Grant::Maintainer, None).unwrap());
        assert!(!grant(connection, &user.id, Grant::Maintainer, None).unwrap());
        assert!(holds(connection, &user.id, Grant::Maintainer).unwrap());
        assert_eq!(holders(connection, Grant::Maintainer).unwrap().len(), 1);

        assert!(revoke(connection, &user.id, Grant::Maintainer, None).unwrap());
        assert!(!holds(connection, &user.id, Grant::Maintainer).unwrap());
        assert!(holders(connection, Grant::Maintainer).unwrap().is_empty());

        // Revoking what nobody holds changes nothing and is not an error.
        assert!(!revoke(connection, &user.id, Grant::Maintainer, None).unwrap());

        let records: i64 = connection
            .query_one("SELECT count(*) FROM instance_grants", [], |row| row.get(0))
            .unwrap();
        assert_eq!(records, 1, "the revoked grant left the record");
    }

    #[test]
    fn a_name_and_an_address_can_be_changed() {
        let database = test_database();
        let connection = database.connection();
        let user = ana(connection);

        set_display_name(connection, &user.id, "  Ana F. ").unwrap();
        set_email(connection, &user.id, " Ana.F@Example.org")
            .unwrap()
            .unwrap();

        let changed = by_id(connection, &user.id).unwrap().unwrap();
        assert_eq!(changed.display_name, "Ana F.");
        assert_eq!(changed.email, "ana.f@example.org");
        assert_eq!(
            by_login(connection, "ana.f@example.org").unwrap(),
            Some(changed)
        );
        assert_eq!(by_login(connection, "ana@example.org").unwrap(), None);
    }

    #[test]
    fn an_address_somebody_else_has_cannot_be_taken() {
        let database = test_database();
        let connection = database.connection();
        let user = ana(connection);
        create(connection, &slug("bo"), "bo@example.org", "Bo", "h")
            .unwrap()
            .unwrap();

        assert_eq!(
            set_email(connection, &user.id, "BO@example.org").unwrap(),
            Err(NotCreated::EmailTaken("bo@example.org".to_owned()))
        );
        assert_eq!(
            set_email(connection, &user.id, "ana@example.org").unwrap(),
            Ok(()),
            "keeping one's own address is not taking somebody else's"
        );
    }

    #[test]
    fn a_suspended_account_says_so() {
        let database = test_database();
        let connection = database.connection();
        let user = ana(connection);

        assert!(user.is_active());

        set_suspended(connection, &user.id, true).unwrap();
        assert!(!by_id(connection, &user.id).unwrap().unwrap().is_active());

        set_suspended(connection, &user.id, false).unwrap();
        assert!(by_id(connection, &user.id).unwrap().unwrap().is_active());
    }
}

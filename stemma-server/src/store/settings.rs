//! Policy.
//!
//! Infrastructure lives in the configuration file, because the process
//! cannot change how it came up while it is up. Policy lives here: read per
//! request, changed from the admin area, and recorded with who changed it.
//! An operator must be able to fix a wrong SMTP host from the web, without
//! an SSH session.
//!
//! The configuration file may seed these the first time the database is
//! created. After that the database is the answer, and the file's copy is
//! reported as disagreeing rather than quietly obeyed.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use stemma_core::id::UserId;

use crate::config::PolicySeed;

/// What this instance calls itself.
pub const INSTANCE_NAME: &str = "instance.name";

/// Who runs it.
pub const INSTANCE_OPERATOR: &str = "instance.operator";

/// How to reach whoever runs it.
pub const INSTANCE_CONTACT: &str = "instance.contact";

/// Whether anyone may create an account.
pub const REGISTRATION_OPEN: &str = "registration.open";

/// Whether account holders may ask for a group.
pub const GROUP_REQUESTS_OPEN: &str = "registration.group_requests";

/// How many days an invitation lasts.
pub const INVITATION_TTL_DAYS: &str = "invitations.ttl_days";

/// The address outgoing mail comes from.
pub const MAIL_FROM: &str = "mail.from";

/// The SMTP server to hand mail to.
pub const MAIL_SMTP_URL: &str = "mail.smtp_url";

/// The environment variable holding the SMTP password, never the password.
pub const MAIL_PASSWORD_ENV: &str = "mail.password_env";

/// What the instance's policy is right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// What this instance calls itself, where anybody has said.
    pub instance_name: Option<String>,
    /// Who runs it.
    pub operator: Option<String>,
    /// How to reach them.
    pub contact: Option<String>,
    /// Whether anyone may create an account.
    pub registration_open: bool,
    /// Whether account holders may ask for a group.
    pub group_requests_open: bool,
    /// How many days an invitation lasts.
    pub invitation_ttl_days: u32,
    /// Where outgoing mail comes from, when there is any.
    pub mail_from: Option<String>,
    /// The SMTP server, when there is one.
    pub mail_smtp_url: Option<String>,
    /// The environment variable holding the password, never the password.
    pub mail_password_env: Option<String>,
}

impl Policy {
    /// Whether the instance can send mail at all.
    ///
    /// When it cannot, invitations and resets are still links; they are
    /// simply shown to whoever made them instead of being delivered.
    pub fn sends_mail(&self) -> bool {
        self.mail_from.is_some() && self.mail_smtp_url.is_some()
    }
}

/// Reads the whole policy.
pub fn policy(connection: &Connection) -> Result<Policy> {
    Ok(Policy {
        instance_name: get(connection, INSTANCE_NAME)?,
        operator: get(connection, INSTANCE_OPERATOR)?,
        contact: get(connection, INSTANCE_CONTACT)?,
        registration_open: flag(connection, REGISTRATION_OPEN, true)?,
        group_requests_open: flag(connection, GROUP_REQUESTS_OPEN, true)?,
        invitation_ttl_days: number(
            connection,
            INVITATION_TTL_DAYS,
            crate::config::DEFAULT_INVITATION_TTL_DAYS,
        )?,
        mail_from: get(connection, MAIL_FROM)?,
        mail_smtp_url: get(connection, MAIL_SMTP_URL)?,
        mail_password_env: get(connection, MAIL_PASSWORD_ENV)?,
    })
}

/// One setting, as it was written.
pub fn get(connection: &Connection, key: &str) -> Result<Option<String>> {
    connection
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .with_context(|| format!("cannot read the `{key}` setting"))
}

/// One setting as a flag, or `fallback` where nobody has decided.
pub fn flag(connection: &Connection, key: &str, fallback: bool) -> Result<bool> {
    Ok(match get(connection, key)?.as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => fallback,
    })
}

/// One setting as a number, or `fallback` where nobody has decided.
pub fn number(connection: &Connection, key: &str, fallback: u32) -> Result<u32> {
    Ok(get(connection, key)?
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback))
}

/// Writes a setting, recording who changed it.
pub fn set(connection: &Connection, key: &str, value: &str, by: Option<&UserId>) -> Result<()> {
    connection
        .execute(
            "INSERT INTO settings (key, value, updated_by) VALUES (?1, ?2, ?3)
             ON CONFLICT (key) DO UPDATE
                SET value = ?2, updated_at = datetime('now'), updated_by = ?3",
            params![key, value, by.map(ToString::to_string)],
        )
        .with_context(|| format!("cannot write the `{key}` setting"))?;

    Ok(())
}

/// Removes a setting, so that the default answers again.
pub fn clear(connection: &Connection, key: &str) -> Result<()> {
    connection
        .execute("DELETE FROM settings WHERE key = ?1", [key])
        .with_context(|| format!("cannot clear the `{key}` setting"))?;

    Ok(())
}

/// Takes the values the configuration file offers, for the keys nobody has
/// decided yet.
///
/// Seeding never overwrites: the file speaks once, when the database is new,
/// and the database answers from then on.
pub fn seed(connection: &Connection, seed: &PolicySeed) -> Result<Vec<&'static str>> {
    let mut written = Vec::new();

    let offered: [(&'static str, Option<String>); 6] = [
        (
            REGISTRATION_OPEN,
            seed.registration_open.map(|open| open.to_string()),
        ),
        (
            GROUP_REQUESTS_OPEN,
            seed.group_requests_open.map(|open| open.to_string()),
        ),
        (
            INVITATION_TTL_DAYS,
            seed.invitation_ttl_days.map(|days| days.to_string()),
        ),
        (MAIL_FROM, seed.mail.as_ref().map(|mail| mail.from.clone())),
        (
            MAIL_SMTP_URL,
            seed.mail.as_ref().map(|mail| mail.smtp_url.clone()),
        ),
        (
            MAIL_PASSWORD_ENV,
            seed.mail
                .as_ref()
                .and_then(|mail| mail.password_env.clone()),
        ),
    ];

    for (key, value) in offered {
        let (Some(value), None) = (value, get(connection, key)?) else {
            continue;
        };

        set(connection, key, &value, None)?;
        written.push(key);
    }

    Ok(written)
}

/// The settings where the file and the database disagree.
///
/// The database wins; this is what lets `check` say so rather than leaving
/// an operator editing a file that no longer decides anything.
pub fn disagreements(connection: &Connection, seed: &PolicySeed) -> Result<Vec<String>> {
    let mut found = Vec::new();

    let offered: [(&str, Option<String>); 3] = [
        (
            REGISTRATION_OPEN,
            seed.registration_open.map(|open| open.to_string()),
        ),
        (
            GROUP_REQUESTS_OPEN,
            seed.group_requests_open.map(|open| open.to_string()),
        ),
        (
            MAIL_SMTP_URL,
            seed.mail.as_ref().map(|mail| mail.smtp_url.clone()),
        ),
    ];

    for (key, offered) in offered {
        let (Some(offered), Some(stored)) = (offered, get(connection, key)?) else {
            continue;
        };

        if offered != stored {
            found.push(format!(
                "`{key}` is `{stored}` in the database and `{offered}` in the file"
            ));
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MailSeed;
    use crate::store::test_database;

    fn seeded() -> PolicySeed {
        PolicySeed {
            registration_open: Some(false),
            group_requests_open: Some(true),
            invitation_ttl_days: Some(7),
            mail: Some(MailSeed {
                from: "stemma@example.org".to_owned(),
                smtp_url: "smtp://localhost:25".to_owned(),
                password_env: None,
            }),
        }
    }

    #[test]
    fn an_instance_nobody_has_configured_has_a_working_policy() {
        let database = test_database();
        let policy = policy(database.connection()).unwrap();

        assert!(policy.registration_open);
        assert!(policy.group_requests_open);
        assert_eq!(policy.invitation_ttl_days, 14);
        assert!(!policy.sends_mail());
    }

    #[test]
    fn the_file_seeds_the_database_once() {
        let database = test_database();
        let connection = database.connection();

        let written = seed(connection, &seeded()).unwrap();
        assert_eq!(written.len(), 5);

        let seeded_policy = policy(connection).unwrap();
        assert!(!seeded_policy.registration_open);
        assert_eq!(seeded_policy.invitation_ttl_days, 7);
        assert!(seeded_policy.sends_mail());

        // Somebody changes it from the admin area, and the file stops
        // deciding.
        set(connection, REGISTRATION_OPEN, "true", None).unwrap();
        assert!(seed(connection, &seeded()).unwrap().is_empty());
        assert!(policy(connection).unwrap().registration_open);
    }

    #[test]
    fn a_file_that_no_longer_decides_anything_is_reported() {
        let database = test_database();
        let connection = database.connection();

        seed(connection, &seeded()).unwrap();
        assert!(disagreements(connection, &seeded()).unwrap().is_empty());

        set(connection, REGISTRATION_OPEN, "true", None).unwrap();
        let found = disagreements(connection, &seeded()).unwrap();

        assert_eq!(found.len(), 1);
        assert!(found[0].contains("registration.open"));
    }

    #[test]
    fn a_setting_that_makes_no_sense_falls_back_rather_than_failing() {
        let database = test_database();
        let connection = database.connection();

        set(connection, REGISTRATION_OPEN, "perhaps", None).unwrap();
        set(connection, INVITATION_TTL_DAYS, "soon", None).unwrap();

        let policy = policy(connection).unwrap();
        assert!(policy.registration_open);
        assert_eq!(policy.invitation_ttl_days, 14);
    }

    #[test]
    fn half_a_mail_configuration_does_not_count_as_mail() {
        let database = test_database();
        let connection = database.connection();

        set(connection, MAIL_FROM, "stemma@example.org", None).unwrap();

        assert!(!policy(connection).unwrap().sends_mail());
    }
}

//! What an administrator does from the machine.
//!
//! Making the first account, granting somebody the run of the instance
//! and handing out a reset link are done here rather than through the
//! web, because all three are the answer to "nobody can get in".
//!
//! No password is ever typed here. An account is made without one and
//! handed a link that sets it, so nothing anybody has to keep secret
//! passes through a command line or is left in a shell's history.

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use stemma_core::access::Grant;
use stemma_core::id::Slug;

use crate::config::Settings;
use crate::password;
use crate::store::users::NotCreated;
use crate::store::{resets, sessions, users};
use crate::tokens::Key;

/// The account somebody named on the command line.
///
/// A name, an address or a full identifier: whichever the administrator
/// has in front of them.
pub fn find(connection: &Connection, named: &str) -> Result<users::User> {
    let named = named.trim();
    let without_prefix = named.strip_prefix("usr:").unwrap_or(named);

    users::by_login(connection, without_prefix)?
        .with_context(|| format!("there is no account `{named}` on this instance"))
}

/// Makes an account, for somebody who then sets its password.
///
/// This is how the first account on an instance comes to exist. Every
/// other door needs one already: a grant needs somebody to grant to,
/// and an invitation needs somebody to write it. An instance that
/// cannot send mail keeps its registration closed, so without this the
/// only way in would be to open the instance to the world for as long
/// as it takes to register.
pub fn create_account(
    connection: &Connection,
    settings: &Settings,
    name: &str,
    email: &str,
    called: Option<&str>,
) -> Result<String> {
    let key = Key::read(&settings.paths.session_key())?;

    let slug = Slug::parse(name.trim()).context("that is not a name an account can have")?;
    let email = email.trim();

    if let Err(problem) = crate::address::check(email) {
        bail!(problem);
    }

    let display_name = match called.map(str::trim) {
        Some(given) if !given.is_empty() => given.to_owned(),
        _ => slug.as_str().to_owned(),
    };

    let user = match users::create(
        connection,
        &slug,
        email,
        &display_name,
        password::NO_PASSWORD,
    )? {
        Ok(user) => user,
        Err(NotCreated::SlugTaken(slug)) => bail!("`{slug}` is already taken on this instance"),
        Err(NotCreated::EmailTaken(email)) => bail!("there is already an account for {email}"),
    };

    let secret = resets::create(connection, &key, &user.id)?;
    let link = settings.public_url.link(&format!("reset/{secret}"));

    Ok(format!(
        "{} is an account with no password yet.\n\n\
         Give this to {display_name}. It works once, and for {} hours:\n\n  {link}",
        user.id,
        resets::LIFETIME_HOURS
    ))
}

/// Gives somebody the run of the instance.
pub fn grant(connection: &Connection, named: &str) -> Result<String> {
    let user = find(connection, named)?;

    Ok(
        match users::grant(connection, &user.id, Grant::Maintainer, None)? {
            true => format!("{} is now a maintainer of this instance.", user.id),
            false => format!("{} already was a maintainer.", user.id),
        },
    )
}

/// Takes it away again, keeping the record that it was held.
pub fn revoke(connection: &Connection, named: &str) -> Result<String> {
    let user = find(connection, named)?;

    Ok(
        match users::revoke(connection, &user.id, Grant::Maintainer, None)? {
            true => format!("{} is no longer a maintainer.", user.id),
            false => format!("{} was not a maintainer.", user.id),
        },
    )
}

/// Who operates this instance.
pub fn list(connection: &Connection) -> Result<String> {
    let held = users::holders(connection, Grant::Maintainer)?;

    if held.is_empty() {
        return Ok("Nobody maintains this instance yet.".to_owned());
    }

    let mut lines = Vec::new();

    for grant in held {
        let name = users::by_id(connection, &grant.user)?
            .map(|user| user.display_name)
            .unwrap_or_else(|| "an account that is gone".to_owned());

        lines.push(format!(
            "{:<24} {name}  since {}",
            grant.user.to_string(),
            grant.granted_at
        ));
    }

    Ok(lines.join("\n"))
}

/// Hands out a link that lets somebody set a new password.
///
/// Every session the account holds ends with it, because the reason for
/// needing this is usually that somebody else may be holding one.
pub fn reset_password(connection: &Connection, settings: &Settings, named: &str) -> Result<String> {
    let key = Key::read(&settings.paths.session_key())?;
    let user = find(connection, named)?;

    if !user.is_active() {
        bail!("{} is suspended, so a reset link would not work", user.id);
    }

    let secret = resets::create(connection, &key, &user.id)?;
    let ended = sessions::end_all(connection, &user.id)?;

    let link = settings.public_url.link(&format!("reset/{secret}"));
    let sessions = match ended {
        0 => String::new(),
        1 => "\nOne session was ended.".to_owned(),
        many => format!("\n{many} sessions were ended."),
    };

    Ok(format!(
        "Give this to {}. It works once, and for {} hours:\n\n  {link}\n{sessions}",
        user.id,
        resets::LIFETIME_HOURS
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_database;
    use stemma_core::id::Slug;

    fn account(connection: &Connection, name: &str) {
        users::create(
            connection,
            &Slug::parse(name).unwrap(),
            &format!("{name}@example.org"),
            name,
            "hash",
        )
        .unwrap()
        .unwrap();
    }

    #[test]
    fn an_account_is_found_however_it_is_named() {
        let database = test_database();
        let connection = database.connection();
        account(connection, "ana");

        for named in ["ana", "usr:ana", "ana@example.org", "  ana  "] {
            assert_eq!(
                find(connection, named).unwrap().id.to_string(),
                "usr:ana",
                "`{named}` was not found"
            );
        }
    }

    #[test]
    fn naming_nobody_says_so_rather_than_doing_nothing() {
        let database = test_database();
        let error = grant(database.connection(), "nobody").unwrap_err();

        assert!(error.to_string().contains("there is no account"));
    }

    #[test]
    fn granting_twice_is_not_an_error_and_says_what_happened() {
        let database = test_database();
        let connection = database.connection();
        account(connection, "ana");

        assert!(
            grant(connection, "ana")
                .unwrap()
                .contains("is now a maintainer")
        );
        assert!(grant(connection, "ana").unwrap().contains("already was"));
        assert!(list(connection).unwrap().contains("usr:ana"));

        assert!(revoke(connection, "ana").unwrap().contains("no longer"));
        assert!(revoke(connection, "ana").unwrap().contains("was not"));
        assert!(list(connection).unwrap().contains("Nobody maintains"));
    }
}

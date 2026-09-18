//! The few things the server has to ask the operating system.
//!
//! Account lookup and account creation are done by running the platform's
//! own tools rather than by linking to the C library: there is one way an
//! administrator would do it by hand, and this is that way.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The numeric identifier of `name`, or `None` if there is no such account.
pub fn user_id(name: &str) -> Option<u32> {
    let output = Command::new("id").arg("-u").arg(name).output().ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

/// The numeric identifier this process runs as.
pub fn current_user_id() -> Option<u32> {
    let output = Command::new("id").arg("-u").output().ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

/// The primary group of `name`, or `None` if there is no such account.
pub fn group_id(name: &str) -> Option<u32> {
    let output = Command::new("id").arg("-g").arg(name).output().ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

/// Whether this process runs as the superuser.
pub fn is_root() -> bool {
    current_user_id() == Some(0)
}

/// The name of this machine, for messages that must be unambiguous about
/// which machine they are about.
pub fn hostname() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "this machine".to_owned())
}

/// Creates the system account the service runs as.
///
/// It gets no password and no shell: nobody logs in as it, and its home is
/// the state directory so that anything written relative to home lands
/// somewhere the account owns.
pub fn create_system_user(name: &str, home: &Path) -> Result<()> {
    let status = Command::new("useradd")
        .arg("--system")
        .arg("--user-group")
        .arg("--home-dir")
        .arg(home)
        .arg("--no-create-home")
        .arg("--shell")
        .arg("/usr/sbin/nologin")
        .arg(name)
        .status()
        .with_context(|| format!("cannot run useradd to create the `{name}` account"))?;

    if !status.success() {
        bail!("useradd refused to create the `{name}` account");
    }

    Ok(())
}

/// Removes the system account the service ran as.
pub fn delete_system_user(name: &str) -> Result<()> {
    let status = Command::new("userdel")
        .arg(name)
        .status()
        .with_context(|| format!("cannot run userdel to remove the `{name}` account"))?;

    if !status.success() {
        bail!("userdel refused to remove the `{name}` account");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_superuser_is_always_zero() {
        assert_eq!(user_id("root"), Some(0));
    }

    #[test]
    fn an_account_that_does_not_exist_resolves_to_nothing() {
        assert_eq!(user_id("stemma-no-such-account"), None);
    }

    #[test]
    fn this_process_has_an_identity() {
        assert!(current_user_id().is_some());
    }

    #[test]
    fn the_superuser_group_resolves_too() {
        assert!(group_id("root").is_some());
    }

    #[test]
    fn the_machine_always_has_a_name_to_print() {
        assert!(!hostname().is_empty());
    }
}

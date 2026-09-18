//! The few things the server has to ask the operating system.
//!
//! Account lookup and account creation are done by running the platform's
//! own tools rather than by linking to the C library: there is one way an
//! administrator would do it by hand, and this is that way.

use std::process::Command;

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

/// Whether this process runs as the superuser.
pub fn is_root() -> bool {
    current_user_id() == Some(0)
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
}

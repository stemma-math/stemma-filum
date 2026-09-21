//! Passwords.
//!
//! Stored as Argon2id PHC strings, which carry their own parameters, so a
//! hash made by an older release keeps verifying after the cost is raised.

use anyhow::{Context, Result, anyhow};
use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};

/// The shortest password accepted.
///
/// Length is the only rule. Composition rules push people towards
/// `Password1!` and towards writing it down.
pub const MINIMUM_LENGTH: usize = 10;

/// Why a password was not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TooWeak {
    /// Shorter than [`MINIMUM_LENGTH`].
    #[error("a password must be at least {MINIMUM_LENGTH} characters long")]
    TooShort,
    /// Nothing but whitespace.
    #[error("a password must contain something other than spaces")]
    Blank,
}

/// What is stored for an account that has no password yet.
///
/// An account made on the machine has none until somebody follows the
/// link that sets one. This is not a hash and cannot be read as one, so
/// [`verify`] refuses every password against it — which is exactly what
/// an account nobody has claimed should do.
pub const NO_PASSWORD: &str = "!";

/// Checks a password before it is ever hashed.
pub fn check(password: &str) -> Result<(), TooWeak> {
    if password.trim().is_empty() {
        return Err(TooWeak::Blank);
    }
    if password.chars().count() < MINIMUM_LENGTH {
        return Err(TooWeak::TooShort);
    }

    Ok(())
}

/// Hashes a password for storage.
pub fn hash(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow!("cannot hash the password: {error}"))
}

/// Whether `password` is the one behind `stored`.
///
/// A stored hash that cannot be parsed is a failure to verify, never a
/// reason to let somebody in.
pub fn verify(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Hashes a password, refusing one that is too weak.
pub fn accept(password: &str) -> Result<String> {
    check(password).context("that password cannot be used")?;
    hash(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_at_all_verifies_against_an_account_with_no_password() {
        for attempt in ["", "!", "a long enough password"] {
            assert!(!verify(attempt, NO_PASSWORD), "`{attempt}` got in");
        }
    }

    #[test]
    fn a_password_verifies_against_its_own_hash() {
        let stored = accept("correct horse battery").unwrap();

        assert!(verify("correct horse battery", &stored));
        assert!(!verify("correct horse batteries", &stored));
    }

    #[test]
    fn two_hashes_of_one_password_differ() {
        let one = hash("correct horse battery").unwrap();
        let other = hash("correct horse battery").unwrap();

        assert_ne!(one, other, "the salt is not doing its work");
    }

    #[test]
    fn short_and_empty_passwords_are_refused() {
        assert_eq!(check("short"), Err(TooWeak::TooShort));
        assert_eq!(check("          "), Err(TooWeak::Blank));
        assert_eq!(check(""), Err(TooWeak::Blank));
        assert!(check("long enough to pass").is_ok());
    }

    #[test]
    fn a_stored_hash_that_makes_no_sense_lets_nobody_in() {
        assert!(!verify("anything", "not a hash"));
        assert!(!verify("", ""));
    }
}

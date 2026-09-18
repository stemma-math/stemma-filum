//! The secrets that travel in links and cookies.
//!
//! A token is random, given out once, and never stored: what the database
//! keeps is a keyed hash of it. So a copy of the database is not a set of
//! working session cookies and invitation links, because forging one also
//! needs the key that only the machine has.

use anyhow::{Context, Result};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// How many random bytes a token carries.
const TOKEN_BYTES: usize = 32;

/// The alphabet a code read aloud or typed in is made of.
///
/// No `0`, `1`, `I`, `O`, `S` or `5`: somebody is reading this off one
/// screen and typing it into another.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRTUVWXYZ2346789";

/// A fresh secret, as hexadecimal.
pub fn secret() -> Result<String> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).context("cannot read randomness")?;

    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// A short code a person can copy from one screen to another.
pub fn readable_code(length: usize) -> Result<String> {
    let mut bytes = vec![0_u8; length];
    getrandom::fill(&mut bytes).context("cannot read randomness")?;

    Ok(bytes
        .iter()
        .map(|byte| CODE_ALPHABET[*byte as usize % CODE_ALPHABET.len()] as char)
        .collect())
}

/// The key that turns a secret into the hash stored beside it.
#[derive(Clone)]
pub struct Key(Vec<u8>);

impl Key {
    /// Reads the key generated when the instance was installed.
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read the session key at {}", path.display()))?;

        Ok(Self(text.trim().as_bytes().to_vec()))
    }

    /// A key for tests.
    #[cfg(test)]
    pub fn for_test() -> Self {
        Self::for_test_with(b"a key that only this test has")
    }

    /// A particular key for tests, where two of them must differ.
    #[cfg(test)]
    pub fn for_test_with(material: &[u8]) -> Self {
        Self(material.to_vec())
    }

    /// What is stored for `secret`.
    pub fn fingerprint(&self, secret: &str) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.0).expect("this construction accepts any length");
        mac.update(secret.as_bytes());

        mac.finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_secrets_are_never_the_same() {
        assert_ne!(secret().unwrap(), secret().unwrap());
        assert_eq!(secret().unwrap().len(), TOKEN_BYTES * 2);
    }

    #[test]
    fn a_readable_code_avoids_the_letters_that_get_misread() {
        let code = readable_code(200).unwrap();

        assert_eq!(code.len(), 200);
        for character in ['0', '1', 'I', 'O', 'S', '5'] {
            assert!(
                !code.contains(character),
                "`{character}` is in the alphabet"
            );
        }
    }

    #[test]
    fn a_fingerprint_is_the_same_every_time_and_different_per_secret() {
        let key = Key::for_test();

        assert_eq!(key.fingerprint("one"), key.fingerprint("one"));
        assert_ne!(key.fingerprint("one"), key.fingerprint("two"));
    }

    #[test]
    fn a_fingerprint_cannot_be_made_without_the_key() {
        let key = Key::for_test();
        let other = Key::for_test_with(b"a different key");

        assert_ne!(key.fingerprint("one"), other.fingerprint("one"));
    }
}

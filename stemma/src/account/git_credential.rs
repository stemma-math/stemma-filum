//! Answering git when it asks for a credential.
//!
//! Git asks a helper rather than reading a password out of a file, which
//! is what keeps the credential out of every clone's configuration. This
//! program is that helper, and it answers only for the instance this
//! machine is signed in to.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use anyhow::Result;

use crate::account::credentials::Credentials;
use crate::home::Home;

/// The username git is given.
///
/// It decides nothing: the instance looks at the password, which is the
/// credential. A name is sent because git expects a pair.
const USERNAME: &str = "stemma";

/// Answers one question from git.
///
/// Only `get` has anything to say. Git also offers a helper the chance to
/// store or forget a credential, and both are somebody else's business
/// here: the credential comes from `stemma login` and goes with
/// `stemma logout`.
pub fn answer(home: &Home, action: &str, input: impl BufRead, out: &mut impl Write) -> Result<()> {
    if action != "get" {
        return Ok(());
    }

    let asked = read_question(input)?;

    let Some(credentials) = Credentials::read(&home.credentials_file())? else {
        return Ok(());
    };

    let (Some(protocol), Some(host)) = (asked.get("protocol"), asked.get("host")) else {
        return Ok(());
    };

    if !is_our_instance(&credentials.server, protocol, host) {
        return Ok(());
    }

    writeln!(out, "username={USERNAME}")?;
    writeln!(out, "password={}", credentials.token)?;

    Ok(())
}

/// Reads the key and value lines git writes.
fn read_question(input: impl BufRead) -> Result<BTreeMap<String, String>> {
    let mut asked = BTreeMap::new();

    for line in input.lines() {
        let line = line?;

        if line.trim().is_empty() {
            break;
        }

        if let Some((key, value)) = line.split_once('=') {
            asked.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }

    Ok(asked)
}

/// Whether the address git is asking about is the one this machine is
/// signed in to.
///
/// Answering anybody else's question would hand this instance's
/// credential to another server.
fn is_our_instance(server: &str, protocol: &str, host: &str) -> bool {
    let (Ok(ours), Ok(asked)) = (
        url::Url::parse(server),
        url::Url::parse(&format!("{protocol}://{host}")),
    ) else {
        return false;
    };
    if !matches!(ours.scheme(), "http" | "https")
        || !asked.username().is_empty()
        || asked.password().is_some()
        || asked.path() != "/"
        || asked.query().is_some()
        || asked.fragment().is_some()
    {
        return false;
    }
    ours.origin() == asked.origin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_about_our_instance_is_recognised() {
        assert!(is_our_instance(
            "https://stemma.test",
            "https",
            "stemma.test"
        ));
        assert!(is_our_instance(
            "https://stemma.test",
            "https",
            "stemma.test:443"
        ));
        assert!(is_our_instance(
            "http://127.0.0.1:8123",
            "http",
            "127.0.0.1:8123"
        ));
        assert!(is_our_instance(
            "https://example.org/stemma",
            "https",
            "example.org"
        ));
    }

    #[test]
    fn a_question_about_anywhere_else_is_not_answered() {
        for (protocol, host) in [
            ("https", "elsewhere.example"),
            ("http", "stemma.test"),
            ("https", "stemma.test:8443"),
            ("https", "attacker@stemma.test"),
            ("https", "stemma.test/path"),
        ] {
            assert!(
                !is_our_instance("https://stemma.test", protocol, host),
                "answered {protocol}://{host}"
            );
        }
    }

    #[test]
    fn equivalent_hosts_and_ipv6_default_ports_match() {
        assert!(is_our_instance(
            "https://stemma.test",
            "https",
            "STEMMA.TEST:443"
        ));
        assert!(is_our_instance("https://[::1]", "https", "[::1]:443"));
        assert!(!is_our_instance("https://[::1]", "https", "[::1]:8443"));
    }

    #[test]
    fn a_machine_with_no_credential_says_nothing() {
        let home = crate::home::Home::locate(&crate::home::Environment::from_pairs([(
            crate::home::HOME_VARIABLE,
            "/nonexistent/stemma",
        )]));

        let mut out = Vec::new();
        answer(
            &home,
            "get",
            &b"protocol=https\nhost=stemma.test\n"[..],
            &mut out,
        )
        .unwrap();

        assert!(out.is_empty());
    }

    #[test]
    fn git_is_told_nothing_when_it_asks_to_store_or_forget() {
        let home = crate::home::Home::locate(&crate::home::Environment::from_pairs([(
            crate::home::HOME_VARIABLE,
            "/nonexistent/stemma",
        )]));

        for action in ["store", "erase"] {
            let mut out = Vec::new();
            answer(
                &home,
                action,
                &b"protocol=https\nhost=stemma.test\n"[..],
                &mut out,
            )
            .unwrap();

            assert!(out.is_empty(), "said something to `{action}`");
        }
    }

    #[test]
    fn what_git_asked_is_read_up_to_the_blank_line() {
        let asked = read_question(
            &b"protocol=https\nhost=stemma.test\npath=git/algebra/shared.git\n\nrubbish\n"[..],
        )
        .unwrap();

        assert_eq!(asked.get("protocol").map(String::as_str), Some("https"));
        assert_eq!(asked.get("host").map(String::as_str), Some("stemma.test"));
        assert_eq!(asked.len(), 3);
    }
}

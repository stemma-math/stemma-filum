//! Handing a message to somebody else's mail server.
//!
//! An instance that cannot send mail is an ordinary instance: every link
//! it makes is shown to whoever made it, and that is a complete way to
//! run one. So nothing here is ever required, nothing fails because of
//! it, and a message that does not go out is reported to the person who
//! asked for it rather than swallowed.
//!
//! The instance holds no address book. It writes to the address somebody
//! typed, or to the one on the account, and it keeps no record of the
//! message beyond what the link already records.

use anyhow::{Context, Result, bail};
use lettre::message::{Mailbox, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::store::settings::Policy;

/// How long to wait for a mail server before giving up on it.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

/// What an instance needs to send anything at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    /// The address mail comes from.
    from: String,
    /// The server to hand it to.
    smtp_url: String,
    /// The password, where the environment holds one.
    password: Option<String>,
    /// What the instance calls itself, which signs every message.
    instance: String,
}

impl Post {
    /// The way out, if this instance has one.
    ///
    /// The password is read from the environment and never from the
    /// configuration file: a file that carries one turns every backup
    /// into a secret.
    pub fn of(policy: &Policy, instance: &str) -> Option<Self> {
        Some(Self {
            from: policy.mail_from.clone()?,
            smtp_url: policy.mail_smtp_url.clone()?,
            password: policy
                .mail_password_env
                .as_deref()
                .and_then(|name| std::env::var(name).ok())
                .filter(|password| !password.is_empty()),
            instance: instance.to_owned(),
        })
    }

    /// Hands one message over, and waits to hear that it was taken.
    ///
    /// This blocks, so it is called where blocking is allowed.
    pub fn send(&self, to: &str, subject: &str, body: &str) -> Result<()> {
        let from: Mailbox = self
            .from
            .parse()
            .with_context(|| format!("`{}` is not an address mail can come from", self.from))?;
        let to: Mailbox = to
            .parse()
            .with_context(|| format!("`{to}` is not an address"))?;

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(signed(body, &self.instance))
            .context("the message cannot be written")?;

        self.transport()?
            .send(&message)
            .context("the mail server would not take it")?;

        Ok(())
    }

    /// The connection this instance's address describes.
    ///
    /// `smtps` is TLS from the first byte and `smtp` starts in the
    /// clear and turns it on; a server on this machine is the one case
    /// where plain is the ordinary answer, because nothing leaves it.
    fn transport(&self) -> Result<SmtpTransport> {
        let parsed = url::Url::parse(&self.smtp_url)
            .with_context(|| format!("`{}` is not an address", self.smtp_url))?;
        let host = parsed
            .host_str()
            .context("the mail server's address names no host")?;

        let builder = match (parsed.scheme(), here(host)) {
            ("smtps", _) => SmtpTransport::relay(host)?,
            ("smtp", false) => SmtpTransport::starttls_relay(host)?,
            ("smtp", true) => SmtpTransport::builder_dangerous(host),
            (other, _) => bail!("`{other}` is not a way to reach a mail server"),
        };

        let builder = match parsed.port() {
            Some(port) => builder.port(port),
            None => builder,
        };

        let user = match parsed.username() {
            "" => None,
            given => Some(percent(given)),
        };

        let builder = match (user, self.password.clone()) {
            (Some(user), Some(password)) => builder.credentials(Credentials::new(user, password)),
            _ => builder,
        };

        Ok(builder.timeout(Some(PATIENCE)).build())
    }
}

/// Whether the mail server is this machine.
fn here(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// A username as it was written, with its escapes undone.
fn percent(given: &str) -> String {
    let mut written = String::new();
    let mut characters = given.chars();

    while let Some(character) = characters.next() {
        if character != '%' {
            written.push(character);
            continue;
        }

        let hex: String = characters.by_ref().take(2).collect();

        match u8::from_str_radix(&hex, 16) {
            Ok(byte) if hex.len() == 2 => written.push(byte as char),
            _ => {
                written.push('%');
                written.push_str(&hex);
            }
        }
    }

    written
}

/// A message, signed by the instance that sent it.
///
/// Every message says which instance it came from, because somebody who
/// works with three of them has to know which one is asking.
fn signed(body: &str, instance: &str) -> String {
    format!("{}\n\n--\n{instance}\n", body.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(from: Option<&str>, smtp: Option<&str>) -> Policy {
        Policy {
            instance_name: Some("Algebra at the UV".to_owned()),
            operator: None,
            contact: None,
            registration_open: true,
            group_requests_open: true,
            invitation_ttl_days: 14,
            mail_from: from.map(str::to_owned),
            mail_smtp_url: smtp.map(str::to_owned),
            mail_password_env: None,
        }
    }

    #[test]
    fn an_instance_with_no_mail_has_no_way_out() {
        assert_eq!(Post::of(&policy(None, None), "Here"), None);
        assert_eq!(
            Post::of(&policy(Some("stemma@example.org"), None), "Here"),
            None,
            "half a configuration is none"
        );
        assert!(
            Post::of(
                &policy(Some("stemma@example.org"), Some("smtp://localhost:25")),
                "Here"
            )
            .is_some()
        );
    }

    #[test]
    fn how_the_address_is_reached_follows_from_what_it_says() {
        let post = |url: &str| {
            Post::of(&policy(Some("stemma@example.org"), Some(url)), "Here")
                .unwrap()
                .transport()
        };

        assert!(post("smtp://localhost:25").is_ok());
        assert!(post("smtps://mail.example.org:465").is_ok());
        assert!(post("smtp://mail.example.org:587").is_ok());
        assert!(post("carrier-pigeon://example.org").is_err());
        assert!(post("not an address at all").is_err());
    }

    #[test]
    fn every_message_says_which_instance_sent_it() {
        let written = signed("Somebody asked for a link.", "Algebra at the UV");

        assert!(written.starts_with("Somebody asked for a link."));
        assert!(written.trim_end().ends_with("Algebra at the UV"));
    }

    #[test]
    fn a_username_written_with_escapes_is_read_back_whole() {
        assert_eq!(percent("ana%40example.org"), "ana@example.org");
        assert_eq!(percent("plain"), "plain");
        assert_eq!(percent("half%4"), "half%4");
    }
}

//! The address the instance answers on.

use std::fmt;
use std::str::FromStr;

use url::Url;

/// The absolute address every link the server sends is built from.
///
/// Invitations and password resets are links, and they are read somewhere
/// else, days later. A value that only works from the machine itself is
/// discovered in somebody else's inbox, so this is validated at startup and
/// the server refuses to run without it.
#[derive(Debug, Clone)]
pub struct PublicUrl {
    parsed: Url,
    /// The address as it is written into links, without a trailing slash.
    text: String,
}

impl PartialEq for PublicUrl {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for PublicUrl {}

impl PublicUrl {
    /// Validates `input` as the instance's public address.
    pub fn parse(input: &str) -> Result<Self, BadPublicUrl> {
        let reject = |reason: &'static str| BadPublicUrl {
            value: input.to_owned(),
            reason,
        };

        let parsed = Url::parse(input.trim()).map_err(|_| reject("it is not a URL"))?;

        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(reject("it must be an `http` or `https` address"));
        }
        if !parsed.has_host() {
            return Err(reject("it names no host"));
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(reject("it must carry no query string and no fragment"));
        }

        // The trailing slash is dropped so that joining never produces a
        // doubled separator, and so that two spellings of the same address
        // compare equal.
        let text = parsed.as_str().trim_end_matches('/').to_owned();

        Ok(Self { parsed, text })
    }

    /// The address as text, without a trailing slash.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The host the instance answers on.
    pub fn host(&self) -> Option<&str> {
        self.parsed.host_str()
    }

    /// An absolute link to `path`, which is taken relative to the instance.
    pub fn link(&self, path: &str) -> String {
        format!("{}/{}", self.text, path.trim_start_matches('/'))
    }
}

impl fmt::Display for PublicUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A string that cannot be the instance's public address.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{value}` cannot be the public address: {reason}")]
pub struct BadPublicUrl {
    /// What was configured.
    pub value: String,
    /// Why it was rejected.
    pub reason: &'static str,
}

impl FromStr for PublicUrl {
    type Err = BadPublicUrl;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_address_is_accepted() {
        let url = PublicUrl::parse("https://stemma.example.org").unwrap();
        assert_eq!(url.as_str(), "https://stemma.example.org");
    }

    #[test]
    fn a_trailing_slash_does_not_change_the_address() {
        assert_eq!(
            PublicUrl::parse("https://stemma.example.org/").unwrap(),
            PublicUrl::parse("https://stemma.example.org").unwrap()
        );
    }

    #[test]
    fn an_instance_under_a_path_keeps_it() {
        let url = PublicUrl::parse("https://example.org/stemma/").unwrap();
        assert_eq!(
            url.link("/invite/abc"),
            "https://example.org/stemma/invite/abc"
        );
    }

    #[test]
    fn links_never_double_the_separator() {
        let url = PublicUrl::parse("https://stemma.example.org").unwrap();
        assert_eq!(
            url.link("invite/abc"),
            "https://stemma.example.org/invite/abc"
        );
        assert_eq!(
            url.link("/invite/abc"),
            "https://stemma.example.org/invite/abc"
        );
    }

    #[test]
    fn addresses_that_would_break_links_are_rejected() {
        for input in [
            "stemma.example.org",
            "ftp://stemma.example.org",
            "https://stemma.example.org?a=1",
            "https://stemma.example.org#top",
            "/stemma",
            "",
        ] {
            assert!(PublicUrl::parse(input).is_err(), "accepted `{input}`");
        }
    }
}

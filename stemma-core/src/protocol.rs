//! The version the client and the server agree on.
//!
//! Client and server are installed and updated by different people on
//! different machines, so they announce a protocol version on every exchange
//! and refuse to guess when the two do not fit.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The version this build speaks.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::new(0, 5);

/// The header both sides send it in.
pub const PROTOCOL_HEADER: &str = "x-stemma-protocol";

/// A protocol version.
///
/// The major number changes when an exchange that used to work stops working.
/// The minor number changes when something is added, so an older client keeps
/// working against a newer server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Incompatible changes.
    pub major: u16,
    /// Additions.
    pub minor: u16,
}

impl ProtocolVersion {
    /// Builds a version.
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Whether a client speaking this version can talk to `server`.
    ///
    /// The majors must match, and the server must be at least as new as the
    /// client: a client may not use what the server has never heard of.
    pub const fn can_talk_to(self, server: Self) -> bool {
        self.major == server.major && self.minor <= server.minor
    }

    /// Checks compatibility, naming both sides when it fails.
    pub fn check_against(self, server: Self) -> Result<(), ProtocolMismatch> {
        if self.can_talk_to(server) {
            Ok(())
        } else {
            Err(ProtocolMismatch {
                client: self,
                server,
            })
        }
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// A string that does not name a protocol version.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a protocol version: expected two numbers, as in `0.1`")]
pub struct BadProtocolVersion(pub String);

impl FromStr for ProtocolVersion {
    type Err = BadProtocolVersion;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bad = || BadProtocolVersion(input.to_owned());
        let (major, minor) = input.split_once('.').ok_or_else(bad)?;

        Ok(Self::new(
            major.parse().map_err(|_| bad())?,
            minor.parse().map_err(|_| bad())?,
        ))
    }
}

/// Two sides that cannot understand each other.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("this build speaks protocol {client} and the server speaks {server}")]
pub struct ProtocolMismatch {
    /// What the client speaks.
    pub client: ProtocolVersion,
    /// What the server speaks.
    pub server: ProtocolVersion,
}

impl ProtocolMismatch {
    /// Which side is behind, and so which one to update.
    pub fn who_is_behind(&self) -> Behind {
        if self.client.major == self.server.major && self.client.minor > self.server.minor {
            Behind::Server
        } else if self.client.major < self.server.major {
            Behind::Client
        } else {
            Behind::Server
        }
    }
}

/// Which side of a mismatch needs updating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behind {
    /// The client is too old for this server.
    Client,
    /// The server is too old for this client.
    Server,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_talks_to_itself() {
        assert!(PROTOCOL_VERSION.can_talk_to(PROTOCOL_VERSION));
    }

    #[test]
    fn an_older_client_talks_to_a_newer_server() {
        assert!(ProtocolVersion::new(1, 2).can_talk_to(ProtocolVersion::new(1, 7)));
    }

    #[test]
    fn a_newer_client_does_not_talk_to_an_older_server() {
        let mismatch = ProtocolVersion::new(1, 7)
            .check_against(ProtocolVersion::new(1, 2))
            .unwrap_err();
        assert_eq!(mismatch.who_is_behind(), Behind::Server);
    }

    #[test]
    fn different_majors_never_talk() {
        let mismatch = ProtocolVersion::new(1, 0)
            .check_against(ProtocolVersion::new(2, 0))
            .unwrap_err();
        assert_eq!(mismatch.who_is_behind(), Behind::Client);
    }

    #[test]
    fn versions_round_trip_through_their_text() {
        assert_eq!(
            "3.14".parse::<ProtocolVersion>().unwrap(),
            ProtocolVersion::new(3, 14)
        );
        assert_eq!(PROTOCOL_VERSION.to_string().parse(), Ok(PROTOCOL_VERSION));
        assert!("0".parse::<ProtocolVersion>().is_err());
        assert!("0.x".parse::<ProtocolVersion>().is_err());
    }
}

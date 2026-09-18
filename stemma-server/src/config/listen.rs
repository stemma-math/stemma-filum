//! Where the server accepts connections.

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// The address the server binds.
///
/// TLS is terminated by a reverse proxy, so this is a loopback port or a unix
/// socket and never carries a certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listen {
    /// A TCP socket.
    Tcp(SocketAddr),
    /// A unix socket at this path.
    Unix(PathBuf),
}

impl Listen {
    /// The prefix that marks a unix socket in the written form.
    const UNIX_PREFIX: &'static str = "unix:";

    /// The directory that must exist before the socket can be bound, if any.
    pub fn socket_directory(&self) -> Option<&Path> {
        match self {
            Self::Tcp(_) => None,
            Self::Unix(path) => path.parent(),
        }
    }
}

impl fmt::Display for Listen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp(address) => write!(f, "{address}"),
            Self::Unix(path) => write!(f, "{}{}", Self::UNIX_PREFIX, path.display()),
        }
    }
}

/// A string that names no address the server can bind.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not an address to listen on: use `host:port` or `unix:/path/to/socket`")]
pub struct BadListen(pub String);

impl FromStr for Listen {
    type Err = BadListen;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if let Some(path) = input.strip_prefix(Self::UNIX_PREFIX) {
            if path.is_empty() {
                return Err(BadListen(input.to_owned()));
            }
            return Ok(Self::Unix(PathBuf::from(path)));
        }

        input
            .parse()
            .map(Self::Tcp)
            .map_err(|_| BadListen(input.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_and_port_is_a_tcp_socket() {
        let listen: Listen = "127.0.0.1:8080".parse().unwrap();
        assert!(matches!(listen, Listen::Tcp(_)));
        assert_eq!(listen.to_string(), "127.0.0.1:8080");
        assert_eq!(listen.socket_directory(), None);
    }

    #[test]
    fn a_unix_prefix_is_a_unix_socket() {
        let listen: Listen = "unix:/run/stemma/server.sock".parse().unwrap();
        assert_eq!(listen, Listen::Unix("/run/stemma/server.sock".into()));
        assert_eq!(listen.to_string(), "unix:/run/stemma/server.sock");
        assert_eq!(listen.socket_directory(), Some(Path::new("/run/stemma")));
    }

    #[test]
    fn anything_else_is_rejected() {
        for input in ["", "localhost", "unix:", "127.0.0.1"] {
            assert!(input.parse::<Listen>().is_err(), "accepted `{input}`");
        }
    }
}

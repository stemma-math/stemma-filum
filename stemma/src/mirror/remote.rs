//! The instance, as the engine needs it.
//!
//! A handful of questions and one kind of write, behind a trait so the
//! engine can be exercised against an instance kept in memory. An error
//! from any of them means the instance could not be reached; a refusal
//! is an answer and comes back as one.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::api::{About, Listing, Outgoing, Revision, Server, Written};
use crate::home::Home;

/// What the engine asks of an instance.
pub trait Remote {
    /// The groups this machine's owner is in.
    fn groups(&self) -> Result<Vec<String>>;
    /// Every line they can see, and where each is.
    fn lines(&self) -> Result<Vec<Listing>>;
    /// One revision of a line, or the one it is at.
    fn revision(&self, line: &str, number: Option<u64>) -> Result<Revision>;
    /// Writes a line.
    fn write(&self, line: &str, outgoing: &Outgoing) -> Result<Written>;
    /// The files at the root of one of a group's repositories.
    fn root(&self, group: &str, side: &str) -> Result<BTreeMap<String, String>>;
    /// Takes a line for this machine; whether it has it now.
    fn take(&self, about: &About) -> Result<bool>;
    /// Lets a line go.
    fn release(&self, about: &About) -> Result<()>;
}

/// The instance this machine is signed in to.
pub struct Instance {
    server: Server,
    token: String,
}

impl Instance {
    /// The instance, as the credential here names it.
    pub fn connect(home: &Home) -> Result<Self> {
        let (credentials, server) = crate::account::session::connect(home)?;

        Ok(Self {
            server,
            token: credentials.token,
        })
    }

    /// The server, for what the engine does not ask.
    pub fn server(&self) -> &Server {
        &self.server
    }

    /// The credential it is spoken to with.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Remote for Instance {
    fn groups(&self) -> Result<Vec<String>> {
        Ok(self
            .server
            .workspace(&self.token)?
            .groups
            .into_iter()
            .map(|group| group.slug)
            .collect())
    }

    fn lines(&self) -> Result<Vec<Listing>> {
        self.server.lines(&self.token)
    }

    fn revision(&self, line: &str, number: Option<u64>) -> Result<Revision> {
        self.server.revision(&self.token, line, number)
    }

    fn write(&self, line: &str, outgoing: &Outgoing) -> Result<Written> {
        self.server.write_line(&self.token, line, outgoing)
    }

    fn root(&self, group: &str, side: &str) -> Result<BTreeMap<String, String>> {
        self.server.repository_root(&self.token, group, side)
    }

    fn take(&self, about: &About) -> Result<bool> {
        Ok(self.server.take_claim(&self.token, about)?.standing == "writing")
    }

    fn release(&self, about: &About) -> Result<()> {
        self.server.release_claim(&self.token, about).map(|_| ())
    }
}

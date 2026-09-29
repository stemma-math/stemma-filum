//! What an operator wants to know about a running instance.
//!
//! These are facts about the application, not about the unit: how much state
//! there is, which schema it is on, whether mail will go out, how many
//! people have accounts. The one line about systemd is there because the
//! answer to "is it running" belongs next to the rest, not because this is a
//! wrapper over `systemctl`.

use std::fmt;

use crate::config::Settings;
use crate::db::{self, Database};
use crate::install::human_size;
use crate::service;

/// A snapshot of the instance.
#[derive(Debug, Clone)]
pub struct Status {
    /// The build this command came from.
    pub version: &'static str,
    /// The protocol that build speaks.
    pub protocol: String,
    /// The address the instance answers on.
    pub public_url: String,
    /// The address it binds.
    pub listen: String,
    /// Where its state is.
    pub data_dir: String,
    /// How much state there is.
    pub data_size: u64,
    /// The schema the database is on, if it can be read.
    pub schema: Option<u32>,
    /// The schema this build needs.
    pub expected_schema: u32,
    /// Whether outgoing mail is configured.
    pub mail: bool,
    /// How many accounts there are, if the database can be read.
    pub accounts: Option<i64>,
    /// How many groups there are.
    pub groups: Option<i64>,
    /// How many repositories are on disk.
    pub repositories: usize,
    /// What systemd says, where there is a systemd to ask.
    pub service: Option<String>,
}

impl Status {
    /// Gathers everything, tolerating a database that is not there yet.
    pub fn gather(settings: &Settings) -> Self {
        let database = Database::open(&settings.paths.database()).ok();

        Self {
            version: stemma_core::VERSION,
            protocol: stemma_core::PROTOCOL_VERSION.to_string(),
            public_url: settings.public_url.to_string(),
            listen: settings.listen.to_string(),
            data_dir: settings.paths.data_dir().display().to_string(),
            data_size: crate::install::size_of(settings.paths.data_dir()),
            schema: database
                .as_ref()
                .and_then(|database| database.version().ok()),
            expected_schema: db::expected_version(),
            mail: settings.policy_seed.mail.is_some(),
            accounts: database
                .as_ref()
                .and_then(|database| count(database, "users")),
            groups: database
                .as_ref()
                .and_then(|database| count(database, "groups")),
            repositories: count_repositories(settings),
            service: service::is_available().then(service::state).flatten(),
        }
    }
}

/// How many rows a table has.
fn count(database: &Database, table: &str) -> Option<i64> {
    database
        .connection()
        .query_one(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .ok()
}

/// How many repositories are on disk, of both kinds.
fn count_repositories(settings: &Settings) -> usize {
    crate::git::Repositories::new(settings.paths.git_dir())
        .on_disk()
        .len()
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "version    {} (protocol {})",
            self.version, self.protocol
        )?;
        writeln!(f, "answering  {}", self.public_url)?;
        writeln!(f, "listening  {}", self.listen)?;
        writeln!(
            f,
            "state      {} ({})",
            self.data_dir,
            human_size(self.data_size)
        )?;

        match self.schema {
            Some(schema) if schema == self.expected_schema => {
                writeln!(f, "schema     {schema}")?;
            }
            Some(schema) => writeln!(
                f,
                "schema     {schema}, and this build needs {}",
                self.expected_schema
            )?,
            None => writeln!(f, "schema     no database yet")?,
        }

        writeln!(
            f,
            "mail       {}",
            match self.mail {
                true => "configured",
                false => "not configured, so invitations are links only",
            }
        )?;

        match (self.accounts, self.groups) {
            (Some(accounts), Some(groups)) => {
                writeln!(f, "accounts   {accounts}")?;
                writeln!(f, "groups     {groups}")?;
            }
            _ => writeln!(f, "accounts   unknown, the database cannot be read")?,
        }

        writeln!(f, "git        {} repositories", self.repositories)?;

        match &self.service {
            Some(state) => write!(f, "service    {state}"),
            None => write!(f, "service    not run by systemd on this machine"),
        }
    }
}

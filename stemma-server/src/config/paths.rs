//! Every path the server uses.
//!
//! One knob decides them all. Separate settings for the database, the git
//! remotes and the keys would multiply arrangements nobody ever tests, and
//! the directory that has to be backed up would stop being one directory.

use std::path::{Path, PathBuf};

/// The locations under the state directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    data_dir: PathBuf,
}

impl Paths {
    /// Derives every path from the state directory.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// All durable state. This is the directory to back up.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The bare git remotes, one per repository.
    pub fn git_dir(&self) -> PathBuf {
        self.data_dir.join("git")
    }

    /// Build outputs, kept outside git and evictable.
    pub fn artefacts_dir(&self) -> PathBuf {
        self.data_dir.join("artefacts")
    }

    /// Keys generated at first run.
    pub fn keys_dir(&self) -> PathBuf {
        self.data_dir.join("keys")
    }

    /// The key that signs sessions.
    pub fn session_key(&self) -> PathBuf {
        self.keys_dir().join("session.key")
    }

    /// The database holding indexes and operational state.
    pub fn database(&self) -> PathBuf {
        self.data_dir.join("server.db")
    }

    /// Where a backup of the database is written before a migration.
    pub fn database_backup(&self, stamp: &str) -> PathBuf {
        self.data_dir.join(format!("server.db.backup-{stamp}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_sits_under_the_state_directory() {
        let paths = Paths::new("/srv/stemma");

        for path in [
            paths.git_dir(),
            paths.artefacts_dir(),
            paths.keys_dir(),
            paths.session_key(),
            paths.database(),
            paths.database_backup("20260919"),
        ] {
            assert!(
                path.starts_with("/srv/stemma"),
                "{} escaped",
                path.display()
            );
        }
    }

    #[test]
    fn a_second_instance_shares_nothing_with_the_first() {
        let one = Paths::new("/var/lib/stemma");
        let other = Paths::new("/var/lib/stemma-staging");
        assert_ne!(one.database(), other.database());
    }
}

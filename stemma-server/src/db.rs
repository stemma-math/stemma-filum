//! The database.
//!
//! Git is the source of truth. What lives here are indexes that could be
//! rebuilt from it and operational state that was never mathematics: who has
//! an account, which machines are registered, what is queued. Nothing that
//! matters is only in SQL.
//!
//! The schema is a list of migrations applied in order, and the number
//! applied is kept in SQLite's own `user_version`. The server refuses to run
//! against a database that is not at the version this binary was built for,
//! and never migrates by itself: an upgrade must not rewrite a database as a
//! side effect of a restart.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};

/// One step of the schema.
struct Migration {
    /// What the step is about, for the operator watching it run.
    name: &'static str,
    /// The statements to apply.
    sql: &'static str,
}

/// Every step, in the order they are applied. Append only.
const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "identity",
        sql: include_str!("../migrations/0001-identity.sql"),
    },
    Migration {
        name: "groups-and-sessions",
        sql: include_str!("../migrations/0002-groups-and-sessions.sql"),
    },
    Migration {
        name: "devices",
        sql: include_str!("../migrations/0003-devices.sql"),
    },
    Migration {
        name: "artefacts",
        sql: include_str!("../migrations/0004-artefacts.sql"),
    },
    Migration {
        name: "machines-and-jobs",
        sql: include_str!("../migrations/0005-machines-and-jobs.sql"),
    },
    Migration {
        name: "proposals",
        sql: include_str!("../migrations/0006-proposals.sql"),
    },
    Migration {
        name: "lean-proposals",
        sql: include_str!("../migrations/0007-lean-proposals.sql"),
    },
    Migration {
        name: "audited-vault",
        sql: include_str!("../migrations/0008-vault.sql"),
    },
    Migration {
        name: "claims",
        sql: include_str!("../migrations/0009-claims.sql"),
    },
    Migration {
        name: "lines",
        sql: include_str!("../migrations/0010-lines.sql"),
    },
    Migration {
        name: "private-line-scopes",
        sql: include_str!("../migrations/0011-private-line-scopes.sql"),
    },
    Migration {
        name: "group-descriptions",
        sql: include_str!("../migrations/0012-group-descriptions.sql"),
    },
    Migration {
        name: "pictures",
        sql: include_str!("../migrations/0013-pictures.sql"),
    },
    Migration {
        name: "events",
        sql: include_str!("../migrations/0014-events.sql"),
    },
    Migration {
        name: "revisions",
        sql: include_str!("../migrations/0015-revisions.sql"),
    },
    Migration {
        name: "builds",
        sql: include_str!("../migrations/0016-builds.sql"),
    },
    Migration {
        name: "lean-libraries",
        sql: include_str!("../migrations/0017-lean-libraries.sql"),
    },
    Migration {
        name: "formal-reviews",
        sql: include_str!("../migrations/0018-formal-reviews.sql"),
    },
    Migration {
        name: "library-releases",
        sql: include_str!("../migrations/0019-library-releases.sql"),
    },
    Migration {
        name: "scoped-agent-jobs",
        sql: include_str!("../migrations/0020-scoped-agent-jobs.sql"),
    },
    Migration {
        name: "proposal-reviews",
        sql: include_str!("../migrations/0021-proposal-reviews.sql"),
    },
    Migration {
        name: "dismissed-requests",
        sql: include_str!("../migrations/0022-dismissed-requests.sql"),
    },
    Migration {
        name: "agent-defaults",
        sql: include_str!("../migrations/0023-agent-defaults.sql"),
    },
    Migration {
        name: "explicit-machine-sharing",
        sql: include_str!("../migrations/0024-explicit-machine-sharing.sql"),
    },
    Migration {
        name: "user-appearance",
        sql: include_str!("../migrations/0025-user-appearance.sql"),
    },
];

/// The schema version this binary was built for.
pub fn expected_version() -> u32 {
    MIGRATIONS.len() as u32
}

/// An open database.
pub struct Database {
    connection: Connection,
}

impl Database {
    /// Opens the database at `path`, refusing to create one.
    ///
    /// Creating it is `migrate`'s job. A server that creates an empty
    /// database because of a typo in a path is a server that comes up
    /// looking brand new.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            bail!(
                "there is no database at {}: run `stemma-server migrate` to create it",
                path.display()
            );
        }

        Self::open_or_create(path)
    }

    /// Opens the database at `path`, refusing one whose schema this build
    /// does not match.
    pub fn open_current(path: &Path) -> Result<Self> {
        let database = Self::open(path)?;

        if !database.is_current()? {
            bail!(
                "the database is at schema version {} and this build needs {}: run \
                 `stemma-server migrate`",
                database.version()?,
                expected_version()
            );
        }

        Ok(database)
    }

    /// Opens the database at `path`, creating an empty one if there is none.
    pub fn open_or_create(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)
            .with_context(|| format!("cannot open the database at {}", path.display()))?;

        Self::prepare(&connection)?;

        Ok(Self { connection })
    }

    /// An empty database in memory, which is what a test runs against.
    pub fn in_memory() -> Result<Self> {
        let connection =
            Connection::open_in_memory().context("cannot open a database in memory")?;
        Self::prepare(&connection)?;

        Ok(Self { connection })
    }

    /// Settings every connection needs.
    fn prepare(connection: &Connection) -> Result<()> {
        // Foreign keys are off by default in SQLite, which turns every
        // reference in the schema into a comment.
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .context("cannot turn on foreign keys")?;

        // Readers do not block the writer, and a writer that finds the file
        // busy waits instead of failing the request.
        let _: String = connection
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
            .context("cannot put the database in write-ahead logging mode")?;
        connection
            .pragma_update(None, "busy_timeout", 5_000)
            .context("cannot set the busy timeout")?;
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .context("cannot set the synchronous mode")?;

        Ok(())
    }

    /// How many migrations this database has had applied.
    pub fn version(&self) -> Result<u32> {
        self.connection
            .query_one("PRAGMA user_version", [], |row| row.get(0))
            .context("cannot read the schema version")
    }

    /// Whether the schema is the one this binary was built for.
    pub fn is_current(&self) -> Result<bool> {
        Ok(self.version()? == expected_version())
    }

    /// Applies every migration that has not been applied yet.
    ///
    /// Each one runs in its own transaction together with the version bump,
    /// so an interrupted upgrade leaves the database at a version that
    /// really is the version it says.
    pub fn migrate(&mut self) -> Result<Vec<&'static str>> {
        let current = self.version()?;

        if current > expected_version() {
            bail!(
                "the database is at schema version {current}, and this build knows {}; \
                 it was written by a newer release, and downgrading is not supported",
                expected_version()
            );
        }

        let mut applied = Vec::new();

        for (index, migration) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let version = index as u32 + 1;
            let transaction = self
                .connection
                .transaction()
                .context("cannot begin a transaction")?;

            transaction
                .execute_batch(migration.sql)
                .with_context(|| format!("migration {version} ({}) failed", migration.name))?;
            transaction
                .pragma_update(None, "user_version", version)
                .context("cannot record the schema version")?;
            transaction
                .commit()
                .with_context(|| format!("cannot commit migration {version}"))?;

            applied.push(migration.name);
        }

        Ok(applied)
    }

    /// Writes a consistent copy of the database to `path`.
    ///
    /// Downgrades are not supported, so the copy taken before a migration is
    /// the way back. It goes through SQLite rather than through the file
    /// system, because the file on disk is only half the database while
    /// there is a write-ahead log beside it.
    pub fn back_up_to(&self, path: &Path) -> Result<()> {
        self.connection
            .backup(rusqlite::MAIN_DB, path, None)
            .with_context(|| format!("cannot copy the database to {}", path.display()))
    }

    /// The connection, for the queries the rest of the server runs.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// The connection, for the writes that need a transaction of their own.
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

/// Reads the schema version without opening the database for writing.
///
/// A check run by an administrator must not leave journal files owned by
/// root in a directory the service account has to write to.
pub fn version_of(path: &Path) -> Result<u32> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("cannot open the database at {}", path.display()))?;

    connection
        .query_one("PRAGMA user_version", [], |row| row.get(0))
        .context("cannot read the schema version")
}

/// The names of every group, read without opening the database for
/// writing.
pub fn group_slugs(path: &Path) -> Result<Vec<String>> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("cannot open the database at {}", path.display()))?;

    let mut statement = connection.prepare("SELECT slug FROM groups ORDER BY slug")?;
    let slugs = statement
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()
        .context("cannot list the groups")?;

    Ok(slugs)
}

/// Something to name a backup after, so that two of them never collide.
pub fn stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_database_starts_at_nothing_and_migrates_to_the_current_schema() {
        let mut database = Database::in_memory().unwrap();
        assert_eq!(database.version().unwrap(), 0);
        assert!(!database.is_current().unwrap());

        let applied = database.migrate().unwrap();

        assert_eq!(applied.len(), MIGRATIONS.len());
        assert_eq!(database.version().unwrap(), expected_version());
        assert!(database.is_current().unwrap());
    }

    #[test]
    fn private_scope_migration_preserves_existing_lines_and_claims() {
        let mut database = Database::in_memory().unwrap();
        for migration in &MIGRATIONS[..10] {
            database.connection.execute_batch(migration.sql).unwrap();
        }
        database
            .connection
            .pragma_update(None, "user_version", 10)
            .unwrap();
        database
            .connection
            .execute_batch(
                "INSERT INTO users (id, slug, email, display_name, password_hash)
               VALUES ('usr:ana', 'ana', 'ana@example.org', 'Ana', 'hash');
             INSERT INTO groups (id, slug, name) VALUES ('grp:algebra', 'algebra', 'Algebra');
             INSERT INTO client_tokens (id, user_id, label, token_hash)
               VALUES ('tok:old', 'usr:ana', 'Laptop', 'old-hash');
             INSERT INTO machines (id, user_id, token_id, label)
               VALUES ('mch:old', 'usr:ana', 'tok:old', 'Laptop');
             INSERT INTO lines (id, group_id, side, name, author, started_at)
               VALUES ('lin:old', 'grp:algebra', 'private', 'quotients', 'usr:ana', '2026-01-01');
             INSERT INTO claims (id, group_id, side, line, machine_id, user_id)
               VALUES ('clm:old', 'grp:algebra', 'private', 'quotients', 'mch:old', 'usr:ana');",
            )
            .unwrap();
        database.migrate().unwrap();
        let original: (String, String, String) = database
            .connection
            .query_row(
                "SELECT id, author, started_at FROM lines WHERE name = 'quotients'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            original,
            ("lin:old".into(), "usr:ana".into(), "2026-01-01".into())
        );
        let claim: String = database
            .connection
            .query_row(
                "SELECT user_id FROM claims WHERE id = 'clm:old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(claim, "usr:ana");
        assert!(database.is_current().unwrap());
    }

    #[test]
    fn migrating_twice_applies_nothing_the_second_time() {
        let mut database = Database::in_memory().unwrap();
        database.migrate().unwrap();

        assert!(database.migrate().unwrap().is_empty());
    }

    #[test]
    fn a_backup_is_a_database_at_the_same_version() {
        let directory = std::env::temp_dir().join(format!("stemma-backup-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let copy = directory.join("copy.db");

        let mut database = Database::in_memory().unwrap();
        database.migrate().unwrap();
        database.back_up_to(&copy).unwrap();

        assert_eq!(version_of(&copy).unwrap(), expected_version());

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_database_from_a_newer_release_is_refused() {
        let mut database = Database::in_memory().unwrap();
        database
            .connection
            .pragma_update(None, "user_version", expected_version() + 1)
            .unwrap();

        let error = database.migrate().unwrap_err();
        assert!(error.to_string().contains("downgrading is not supported"));
    }

    #[test]
    fn the_schema_refuses_a_grant_to_an_account_that_does_not_exist() {
        let mut database = Database::in_memory().unwrap();
        database.migrate().unwrap();

        let error = database.connection.execute(
            "INSERT INTO instance_grants (id, user_id, kind) VALUES ('gnt:1', 'usr:nobody', 'maintainer')",
            [],
        );

        assert!(error.is_err(), "a grant without an account was accepted");
    }

    #[test]
    fn an_account_can_only_hold_one_live_grant_of_a_kind() {
        let mut database = Database::in_memory().unwrap();
        database.migrate().unwrap();

        database
            .connection
            .execute(
                "INSERT INTO users (id, slug, email, display_name, password_hash)
                 VALUES ('usr:ana', 'ana', 'ana@example.org', 'Ana', 'x')",
                [],
            )
            .unwrap();

        let grant = "INSERT INTO instance_grants (id, user_id, kind)
                     VALUES (?1, 'usr:ana', 'maintainer')";

        database.connection.execute(grant, ["gnt:1"]).unwrap();
        assert!(database.connection.execute(grant, ["gnt:2"]).is_err());

        // Once the first is revoked the same grant can be given again, and
        // both acts stay in the record.
        database
            .connection
            .execute(
                "UPDATE instance_grants SET revoked_at = datetime('now') WHERE id = 'gnt:1'",
                [],
            )
            .unwrap();

        database.connection.execute(grant, ["gnt:2"]).unwrap();

        let rows: u32 = database
            .connection
            .query_one("SELECT count(*) FROM instance_grants", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 2);
    }
}

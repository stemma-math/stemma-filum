//! Reading and writing the database.
//!
//! Everything that touches SQL lives here, so that the web and the command
//! line share one set of rules about what an account is and who may be in a
//! group. The functions take a connection rather than holding one: a
//! transaction is the caller's business, because it is the caller who knows
//! which acts belong together.

pub mod artefacts;
pub mod claims;
pub mod devices;
pub mod groups;
pub mod invitations;
pub mod jobs;
pub mod lines;
pub mod machines;
pub mod proposals;
pub mod resets;
pub mod sessions;
pub mod settings;
pub mod users;
pub mod vault;

use std::str::FromStr;

use rusqlite::Row;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use stemma_core::id::IdKind;

/// Reads a typed identifier out of a column.
///
/// An identifier that does not parse is a database that has been edited by
/// hand into a state the code cannot be asked to interpret.
fn id_at<T>(row: &Row<'_>, column: &str) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let raw: String = row.get(column)?;

    raw.parse().map_err(|error: T::Err| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            error.to_string().into(),
        )
    })
}

/// A newtype so that a group role can be read from and written to SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Role(pub stemma_core::access::GroupRole);

impl FromSql for Role {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        value
            .as_str()?
            .parse()
            .map(Role)
            .map_err(|error| FromSqlError::Other(Box::new(error)))
    }
}

impl rusqlite::ToSql for Role {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.0.as_str().into())
    }
}

/// The identifier of something that is addressed by a slug a person chose.
pub fn named_id<K: IdKind>(slug: &stemma_core::id::Slug) -> stemma_core::id::NamedId<K> {
    stemma_core::id::NamedId::new(slug.clone())
}

/// A database with the schema applied, for tests.
#[cfg(test)]
pub fn test_database() -> crate::db::Database {
    let mut database = crate::db::Database::in_memory().expect("a database in memory");
    database.migrate().expect("the schema applies");

    database
}

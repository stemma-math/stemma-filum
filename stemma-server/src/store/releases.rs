//! Immutable public releases of a group's shared library.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use stemma_core::artefact::FormalDeclaration;
use stemma_core::id::{BlockId, GroupId, UserId};
use stemma_core::line::Name;

/// One central result in generated release documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultPage {
    /// Directory of the line containing the result.
    pub line: Name,
    /// Human title of that line.
    pub line_title: String,
    /// Stable identifier of the central statement.
    pub block: BlockId,
    /// The printed environment name, such as `Theorem`.
    pub environment: String,
    /// Human title of the result, where it has one.
    pub title: Option<String>,
    /// Its prose statement, in the group's source notation.
    pub prose: String,
    /// Lean's printed declarations carrying this result's mark.
    pub declarations: Vec<FormalDeclaration>,
}

impl ResultPage {
    /// What to call the result in a documentation index.
    pub fn name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.environment)
    }
}

/// A published library release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Group that published it.
    pub group: GroupId,
    /// Version chosen by the group.
    pub version: String,
    /// Private shared-library commit selected for publication.
    pub source_commit: String,
    /// Root commit in the public release repository.
    pub release_commit: String,
    /// Generated documentation, restricted to central marked results.
    pub results: Vec<ResultPage>,
    /// Person who published it.
    pub published_by: UserId,
    /// When it was published.
    pub published_at: String,
}

impl Release {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let group: String = row.get("group_id")?;
        let by: String = row.get("published_by")?;
        let results: String = row.get("results")?;
        let unreadable = |column: usize, error: Box<dyn std::error::Error + Send + Sync>| {
            rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, error)
        };

        Ok(Self {
            group: group
                .parse()
                .map_err(|error| unreadable(0, Box::new(error)))?,
            version: row.get("version")?,
            source_commit: row.get("source_commit")?,
            release_commit: row.get("release_commit")?,
            results: serde_json::from_str(&results)
                .map_err(|error| unreadable(4, Box::new(error)))?,
            published_by: by.parse().map_err(|error| unreadable(5, Box::new(error)))?,
            published_at: row.get("published_at")?,
        })
    }
}

const COLUMNS: &str = "group_id, version, source_commit, release_commit, results, \
                       published_by, published_at";

/// Records a release after its public source snapshot exists.
pub fn record(
    connection: &Connection,
    group: &GroupId,
    version: &str,
    source_commit: &str,
    release_commit: &str,
    results: &[ResultPage],
    by: &UserId,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO library_releases
                (group_id, version, source_commit, release_commit, results, published_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                group.to_string(),
                version,
                source_commit,
                release_commit,
                serde_json::to_string(results)?,
                by.to_string(),
            ],
        )
        .context("cannot record the library release")?;
    Ok(())
}

/// One version of a group's library.
pub fn by_version(
    connection: &Connection,
    group: &GroupId,
    version: &str,
) -> Result<Option<Release>> {
    connection
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM library_releases
                  WHERE group_id = ?1 AND version = ?2"
            ),
            params![group.to_string(), version],
            Release::from_row,
        )
        .optional()
        .context("cannot read the library release")
}

/// Every release of a group, newest first.
pub fn for_group(connection: &Connection, group: &GroupId) -> Result<Vec<Release>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM library_releases
          WHERE group_id = ?1 ORDER BY published_at DESC, rowid DESC"
    ))?;
    statement
        .query_map([group.to_string()], Release::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list library releases")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups, test_database, users};
    use stemma_core::id::Slug;

    #[test]
    fn release_documentation_is_immutable_and_read_back() {
        let mut database = test_database();
        let connection = database.connection_mut();
        let user = users::create(
            connection,
            &Slug::parse("ana").unwrap(),
            "ana@example.org",
            "Ana",
            "hash",
        )
        .unwrap()
        .unwrap();
        let group = groups::create(
            connection,
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &user.id,
        )
        .unwrap();
        let result = ResultPage {
            line: Name::parse("quotients").unwrap(),
            line_title: "Quotient groups".to_owned(),
            block: format!("blk:{:026}", 1).parse().unwrap(),
            environment: "Theorem".to_owned(),
            title: Some("First isomorphism theorem".to_owned()),
            prose: "The expected quotient is isomorphic to the image.".to_owned(),
            declarations: Vec::new(),
        };

        record(
            connection,
            &group.id,
            "1.0.0",
            "1111111",
            "2222222",
            std::slice::from_ref(&result),
            &user.id,
        )
        .unwrap();

        let found = by_version(connection, &group.id, "1.0.0").unwrap().unwrap();
        assert_eq!(found.results, [result]);
        assert!(
            record(
                connection,
                &group.id,
                "1.0.0",
                "3333333",
                "4444444",
                &[],
                &user.id,
            )
            .is_err()
        );
    }
}

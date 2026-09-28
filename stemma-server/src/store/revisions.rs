//! The revisions of every line, and the stretches of writing they come in.
//!
//! A write names the revision it was made from and is accepted only if
//! that is still the line's revision, from the machine that holds the
//! line, and only if what it sends is a line at all. Everything a
//! machine can get wrong is refused here, with what it needs to put it
//! right: the revision it missed, or who is writing the line instead.
//!
//! Revisions arrive every few seconds, and git is told once per stretch
//! of writing. A stretch is one machine on one line; it ends when the line
//! is let go, when another machine writes it, or when nothing has been
//! written for a while, and whoever keeps git commits what it came to.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::document::Document;
use stemma_core::event::LineChange;
use stemma_core::id::{LineId, MachineId, UserId};
use stemma_core::repository::Side;

use super::id_at;
use super::lines::{self, Started};

/// How long a stretch of writing may go quiet before it is over.
pub const QUIET_MINUTES: u32 = 10;

/// The largest source, or Lean module, a revision may carry.
pub const LARGEST_SOURCE: usize = 1024 * 1024;

/// The largest page a revision may carry.
pub const LARGEST_PAGE: usize = 8 * 1024 * 1024;

/// What made a revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The line was started.
    Started,
    /// Somebody wrote it.
    Written,
    /// It moved to the group.
    Shared,
    /// It was deleted.
    Deleted,
}

impl Kind {
    /// How it is written down.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Written => "written",
            Self::Shared => "shared",
            Self::Deleted => "deleted",
        }
    }

    fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "started" => Self::Started,
            "written" => Self::Written,
            "shared" => Self::Shared,
            "deleted" => Self::Deleted,
            _ => return None,
        })
    }
}

/// What a revision says: the line, its Lean module, and its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    /// The line, as its `line.json`.
    pub source: String,
    /// The Lean module beside it, where it has one.
    pub lean_source: Option<String>,
    /// What it typesets to, as a page.
    pub page: String,
}

/// One revision of one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// The line.
    pub line: LineId,
    /// Its place in the line's history, from one.
    pub number: u64,
    /// What made it.
    pub kind: Kind,
    /// Which repository the line was in.
    pub side: Side,
    /// The line, as its `line.json`.
    pub source: String,
    /// The Lean module beside it.
    pub lean_source: Option<String>,
    /// What it typesets to.
    pub page: String,
    /// Who made it.
    pub author: UserId,
    /// The machine it was written on, when it was written on one.
    pub machine: Option<MachineId>,
    /// When.
    pub written_at: String,
}

impl Revision {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let unreadable = |what: &str| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("not {what}").into(),
            )
        };
        let kind: String = row.get("kind")?;
        let side: String = row.get("side")?;
        let number: i64 = row.get("number")?;
        let machine: Option<String> = row.get("machine_id")?;

        Ok(Self {
            line: id_at(row, "line_id")?,
            number: number.max(0) as u64,
            kind: Kind::parse(&kind).ok_or_else(|| unreadable("a kind of revision"))?,
            side: side.parse().map_err(|_| unreadable("a repository"))?,
            source: row.get("source")?,
            lean_source: row.get("lean_source")?,
            page: row.get("page")?,
            author: id_at(row, "author")?,
            machine: machine.and_then(|machine| machine.parse().ok()),
            written_at: row.get("written_at")?,
        })
    }

    /// What it says.
    pub fn content(&self) -> Content {
        Content {
            source: self.source.clone(),
            lean_source: self.lean_source.clone(),
            page: self.page.clone(),
        }
    }
}

/// The columns every read asks for.
const COLUMNS: &str = "line_id, number, kind, side, source, lean_source, page, author, \
                       machine_id, written_at";

/// Adds the next revision of a line, and moves the line to it.
///
/// For the modules that make revisions; nothing is checked here.
pub(super) fn add(
    connection: &Connection,
    line: &LineId,
    kind: Kind,
    content: &Content,
    author: &UserId,
    machine: Option<&MachineId>,
    session: Option<&str>,
) -> Result<u64> {
    let (current, side): (i64, String) = connection
        .query_row(
            "SELECT revision, side FROM lines WHERE id = ?1",
            [line.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .context("there is no such line")?;
    let next = current + 1;

    connection
        .execute(
            "INSERT INTO revisions (line_id, number, kind, side, source, lean_source, page,
                                    author, machine_id, session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                line.to_string(),
                next,
                kind.as_str(),
                side,
                content.source,
                content.lean_source,
                content.page,
                author.to_string(),
                machine.map(ToString::to_string),
                session,
            ],
        )
        .context("cannot keep the revision")?;
    connection
        .execute(
            "UPDATE lines SET revision = ?2 WHERE id = ?1",
            params![line.to_string(), next],
        )
        .context("cannot move the line to its revision")?;

    Ok(next as u64)
}

/// One revision of a line.
pub fn at(connection: &Connection, line: &LineId, number: u64) -> Result<Option<Revision>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM revisions WHERE line_id = ?1 AND number = ?2"),
            params![line.to_string(), number as i64],
            Revision::from_row,
        )
        .optional()
        .context("cannot read the revision")
}

/// The revision a line is at.
pub fn current(connection: &Connection, line: &Started) -> Result<Option<Revision>> {
    at(connection, &line.id, line.revision)
}

/// What a machine sends to write a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    /// The revision it was written from.
    pub base: u64,
    /// What it says now.
    pub content: Content,
}

/// Why a write was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The line has been deleted.
    Gone,
    /// The line moved on from the revision the write was made from. What
    /// it is now is here, for the machine to bring its writing up to.
    Stale(Box<Revision>),
    /// The machine does not hold the line; the one that does calls itself
    /// this, when one does.
    NotHeld(Option<String>),
    /// What was sent is not something a line can be.
    Unreadable(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gone => write!(f, "this line has been deleted"),
            Self::Stale(current) => write!(
                f,
                "this line is at revision {} now; bring the writing up to it first",
                current.number
            ),
            Self::NotHeld(Some(label)) => write!(f, "{label} is writing this line"),
            Self::NotHeld(None) => write!(f, "take the line before writing it"),
            Self::Unreadable(why) => write!(f, "{why}"),
        }
    }
}

/// Checks what a machine sent before anything else is asked of it.
pub fn check(content: &Content) -> Result<(), Refused> {
    let too_large = |what: &str| Refused::Unreadable(format!("the {what} is too large"));

    if content.source.len() > LARGEST_SOURCE {
        return Err(too_large("line"));
    }
    if content
        .lean_source
        .as_ref()
        .is_some_and(|lean| lean.len() > LARGEST_SOURCE)
    {
        return Err(too_large("Lean module"));
    }
    if content.page.len() > LARGEST_PAGE {
        return Err(too_large("page"));
    }
    if content.page.trim().is_empty() {
        return Err(Refused::Unreadable(
            "a revision needs the page it typesets to".to_owned(),
        ));
    }
    if let Err(unreadable) = Document::parse(&content.source) {
        return Err(Refused::Unreadable(format!(
            "this is not a line: {unreadable}"
        )));
    }

    Ok(())
}

/// Writes a line, if the write may be taken.
///
/// All of it in one transaction: the revision it was made from, the
/// claim, and the stretch of writing it belongs to are read and changed
/// together, so two machines racing each other get one yes and one no.
pub fn write(
    connection: &mut Connection,
    line: &LineId,
    offered: &Offered,
    machine: &MachineId,
    writer: &UserId,
) -> Result<Result<Started, Refused>> {
    if let Err(refused) = check(&offered.content) {
        return Ok(Err(refused));
    }

    let transaction = connection.transaction()?;
    let Some(found) = lines::by_id(&transaction, line)? else {
        return Ok(Err(Refused::Gone));
    };

    if found.is_deleted() {
        return Ok(Err(Refused::Gone));
    }

    let holder = super::claims::holder(
        &transaction,
        &found.group,
        found.side,
        found.name.as_str(),
        writer,
    )?;
    match holder {
        Some(claim) if &claim.machine == machine => {}
        Some(claim) => return Ok(Err(Refused::NotHeld(Some(claim.label)))),
        None => return Ok(Err(Refused::NotHeld(None))),
    }

    if offered.base != found.revision {
        let current = current(&transaction, &found)?.context("the line has no revision")?;
        return Ok(Err(Refused::Stale(Box::new(current))));
    }

    let session = continue_or_open(&transaction, &found.id, machine, writer)?;
    add(
        &transaction,
        &found.id,
        Kind::Written,
        &offered.content,
        writer,
        Some(machine),
        Some(&session),
    )?;
    let withdrawn = super::formal_audits::withdraw_changed_prose(
        &transaction,
        &found.id,
        &offered.content.source,
    )?;
    for block in withdrawn {
        super::events::publish(
            &transaction,
            match found.side {
                Side::Shared => super::events::Audience::Group(&found.group),
                Side::Private => super::events::Audience::Person(&found.group, writer),
            },
            &stemma_core::event::Event::FormalAudit {
                group: found.group.slug().as_str().to_owned(),
                line: found.name.as_str().to_owned(),
                block,
                state: "audit_withdrawn".to_owned(),
            },
        )?;
    }

    let written = lines::by_id(&transaction, &found.id)?.context("the line cannot be read back")?;
    lines::tell(&transaction, &written, LineChange::Written)?;
    transaction.commit()?;

    Ok(Ok(written))
}

/// The stretch this machine is writing the line in, or a new one.
///
/// Another machine's stretch on the same line is over: its writing ends
/// where this begins, and is committed on its own.
fn continue_or_open(
    connection: &Connection,
    line: &LineId,
    machine: &MachineId,
    writer: &UserId,
) -> Result<String> {
    let open: Option<(String, Option<String>)> = connection
        .query_row(
            "SELECT id, machine_id FROM writing_sessions
              WHERE line_id = ?1 AND closed_at IS NULL",
            [line.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .context("cannot read the stretch of writing")?;

    if let Some((id, holder)) = &open {
        if holder.as_deref() == Some(machine.to_string().as_str()) {
            connection
                .execute(
                    "UPDATE writing_sessions SET last_at = datetime('now') WHERE id = ?1",
                    [id],
                )
                .context("cannot extend the stretch of writing")?;
            return Ok(id.clone());
        }

        close(connection, id)?;
    }

    let id = format!(
        "wrt:{}",
        LineId::generate().to_string().trim_start_matches("lin:")
    );
    connection
        .execute(
            "INSERT INTO writing_sessions (id, line_id, machine_id, user_id) VALUES (?1, ?2, ?3, ?4)",
            params![id, line.to_string(), machine.to_string(), writer.to_string()],
        )
        .context("cannot start a stretch of writing")?;

    Ok(id)
}

/// Ends a stretch of writing. Git is told about it afterwards.
fn close(connection: &Connection, session: &str) -> Result<()> {
    connection
        .execute(
            "UPDATE writing_sessions SET closed_at = datetime('now')
              WHERE id = ?1 AND closed_at IS NULL",
            [session],
        )
        .context("cannot end the stretch of writing")?;

    Ok(())
}

/// Ends whatever stretch of writing a line is in.
pub fn end_writing(connection: &Connection, line: &LineId) -> Result<()> {
    connection
        .execute(
            "UPDATE writing_sessions SET closed_at = datetime('now')
              WHERE line_id = ?1 AND closed_at IS NULL",
            [line.to_string()],
        )
        .context("cannot end the stretch of writing")?;

    Ok(())
}

/// Ends every stretch of writing that has gone quiet.
pub fn end_quiet(connection: &Connection) -> Result<usize> {
    connection
        .execute(
            "UPDATE writing_sessions SET closed_at = datetime('now')
              WHERE closed_at IS NULL AND last_at < datetime('now', ?1)",
            [format!("-{QUIET_MINUTES} minutes")],
        )
        .context("cannot end the quiet stretches of writing")
}

/// A stretch of writing that is over and that git has not been told of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncommitted {
    /// Which stretch.
    pub session: String,
    /// The line it wrote.
    pub line: Started,
    /// Who wrote it.
    pub writer: UserId,
    /// The last revision it wrote, which is what git gets.
    pub last: Revision,
}

/// Every stretch of writing git has yet to be told of, oldest first.
pub fn uncommitted(connection: &Connection) -> Result<Vec<Uncommitted>> {
    let mut statement = connection.prepare(
        "SELECT s.id, s.line_id, s.user_id, max(r.number) AS last
           FROM writing_sessions s JOIN revisions r ON r.session_id = s.id
          WHERE s.closed_at IS NOT NULL AND s.commit_id IS NULL
          GROUP BY s.id
          ORDER BY s.closed_at, s.id",
    )?;
    let found = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                id_at::<LineId>(row, "line_id")?,
                id_at::<UserId>(row, "user_id")?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the stretches of writing")?;

    let mut ready = Vec::new();
    for (session, line, writer, last) in found {
        let (Some(line), Some(last)) = (
            lines::by_id(connection, &line)?,
            at(connection, &line, last.max(0) as u64)?,
        ) else {
            continue;
        };

        ready.push(Uncommitted {
            session,
            line,
            writer,
            last,
        });
    }

    Ok(ready)
}

/// Notes that git was told of a stretch of writing, with the commit it
/// made, or with nothing when there was nothing new to commit.
pub fn committed(
    connection: &Connection,
    session: &str,
    line: &LineId,
    revision: u64,
    commit: Option<&str>,
) -> Result<()> {
    connection
        .execute(
            "UPDATE writing_sessions SET commit_id = ?2 WHERE id = ?1",
            params![session, commit.unwrap_or("")],
        )
        .context("cannot record the commit")?;

    lines::committed(connection, line, revision)
}

/// How long after a stretch of writing ends a machine has to publish
/// what it came to, before another machine is asked to.
pub const BUILD_GRACE_MINUTES: u32 = 5;

/// Notes that what a line came to up to a revision has been built.
pub fn built_up_to(connection: &Connection, line: &LineId, revision: u64) -> Result<usize> {
    connection
        .execute(
            "UPDATE writing_sessions SET built = 1
              WHERE line_id = ?1 AND built = 0
                AND id IN (SELECT session_id FROM revisions
                            WHERE line_id = ?1 AND session_id IS NOT NULL
                            GROUP BY session_id HAVING max(number) <= ?2)",
            params![line.to_string(), revision as i64],
        )
        .context("cannot note what was built")
}

/// A stretch of writing nobody has built, a while after it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unbuilt {
    /// Which stretch.
    pub session: String,
    /// The line it wrote, as it is now.
    pub line: Started,
    /// Who wrote it.
    pub writer: UserId,
}

/// Every stretch of writing that is committed, not built, not yet asked
/// of anybody, and past its grace. A stretch some later writing has
/// overtaken is owed nothing, and is noted as such.
pub fn unbuilt(connection: &Connection) -> Result<Vec<Unbuilt>> {
    let mut statement = connection.prepare(
        "SELECT s.id, s.line_id, s.user_id, max(r.number) AS last
           FROM writing_sessions s JOIN revisions r ON r.session_id = s.id
          WHERE s.built = 0 AND s.build_job IS NULL
            AND s.commit_id IS NOT NULL
            AND s.closed_at < datetime('now', ?1)
          GROUP BY s.id
          ORDER BY s.closed_at, s.id",
    )?;
    let found = statement
        .query_map([format!("-{BUILD_GRACE_MINUTES} minutes")], |row| {
            Ok((
                row.get::<_, String>(0)?,
                id_at::<LineId>(row, "line_id")?,
                id_at::<UserId>(row, "user_id")?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list what is not built")?;

    let mut owed = Vec::new();
    for (session, line, writer, last) in found {
        match lines::by_id(connection, &line)? {
            Some(line) if !line.is_deleted() && line.revision == last.max(0) as u64 => {
                owed.push(Unbuilt {
                    session,
                    line,
                    writer,
                });
            }
            _ => {
                connection
                    .execute(
                        "UPDATE writing_sessions SET built = 1 WHERE id = ?1",
                        [&session],
                    )
                    .context("cannot note what is owed nothing")?;
            }
        }
    }

    Ok(owed)
}

/// Notes which job was asked to build a stretch of writing.
pub fn build_asked(connection: &Connection, session: &str, job: &str) -> Result<()> {
    connection
        .execute(
            "UPDATE writing_sessions SET build_job = ?2 WHERE id = ?1",
            params![session, job],
        )
        .context("cannot note the build that was asked for")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{claims, groups, test_database, users};
    use stemma_core::id::{GroupId, Slug};
    use stemma_core::line::Name;

    struct World {
        database: crate::db::Database,
        group: GroupId,
        ana: UserId,
        laptop: MachineId,
        desktop: MachineId,
        line: LineId,
    }

    fn machine(connection: &Connection, owner: &UserId, token: &str) -> MachineId {
        connection
            .execute(
                "INSERT INTO client_tokens (id, user_id, label, token_hash) VALUES (?1, ?2, ?1, ?1)",
                params![token, owner.to_string()],
            )
            .unwrap();
        crate::store::machines::register(connection, owner, token, token)
            .unwrap()
            .id
    }

    fn line_json(title: &str) -> String {
        Document::new(title, None).to_text()
    }

    fn page(title: &str) -> Content {
        Content {
            source: line_json(title),
            lean_source: None,
            page: format!("<h1>{title}</h1>"),
        }
    }

    fn world() -> World {
        let mut database = test_database();
        let ana = users::create(
            database.connection(),
            &Slug::parse("ana").unwrap(),
            "ana@example.org",
            "Ana",
            "hash",
        )
        .unwrap()
        .unwrap()
        .id;
        let group = groups::create(
            database.connection_mut(),
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &ana,
        )
        .unwrap()
        .id;
        let laptop = machine(database.connection(), &ana, "laptop");
        let desktop = machine(database.connection(), &ana, "desktop");
        let line = lines::start(
            database.connection_mut(),
            &group,
            Side::Shared,
            &Name::parse("quotients").unwrap(),
            &ana,
            &page("Quotients"),
        )
        .unwrap()
        .id;

        World {
            database,
            group,
            ana,
            laptop,
            desktop,
            line,
        }
    }

    fn take(world: &World, machine: &MachineId) {
        claims::take(
            world.database.connection(),
            &world.group,
            Side::Shared,
            "quotients",
            machine,
            &world.ana,
        )
        .unwrap();
    }

    fn written(
        world: &mut World,
        machine: &MachineId,
        base: u64,
        title: &str,
    ) -> Result<Started, Refused> {
        let offered = Offered {
            base,
            content: page(title),
        };
        let (line, ana) = (world.line, world.ana.clone());

        write(
            world.database.connection_mut(),
            &line,
            &offered,
            machine,
            &ana,
        )
        .unwrap()
    }

    #[test]
    fn a_write_from_the_holder_on_the_current_revision_is_the_next_revision() {
        let mut world = world();
        let laptop = world.laptop;
        take(&world, &laptop);

        let line = written(&mut world, &laptop, 1, "Quotient groups").unwrap();

        assert_eq!(line.revision, 2);
        let kept = at(world.database.connection(), &world.line, 2)
            .unwrap()
            .unwrap();
        assert_eq!(kept.kind, Kind::Written);
        assert_eq!(kept.machine, Some(laptop));
        assert_eq!(kept.content(), page("Quotient groups"));
    }

    #[test]
    fn a_write_from_an_old_revision_is_refused_with_the_current_one() {
        let mut world = world();
        let laptop = world.laptop;
        take(&world, &laptop);
        written(&mut world, &laptop, 1, "Two").unwrap();

        match written(&mut world, &laptop, 1, "Also two") {
            Err(Refused::Stale(current)) => {
                assert_eq!(current.number, 2);
                assert_eq!(current.content(), page("Two"));
            }
            other => panic!("not refused as stale: {other:?}"),
        }
    }

    #[test]
    fn only_the_machine_holding_the_line_may_write_it() {
        let mut world = world();
        let (laptop, desktop) = (world.laptop, world.desktop);

        assert_eq!(
            written(&mut world, &laptop, 1, "Two"),
            Err(Refused::NotHeld(None))
        );

        take(&world, &desktop);
        assert_eq!(
            written(&mut world, &laptop, 1, "Two"),
            Err(Refused::NotHeld(Some("desktop".to_owned())))
        );
    }

    #[test]
    fn something_that_is_not_a_line_or_has_no_page_is_refused() {
        let mut world = world();
        let laptop = world.laptop;
        take(&world, &laptop);
        let (line, ana) = (world.line, world.ana.clone());

        for content in [
            Content {
                source: "= Not a line".to_owned(),
                lean_source: None,
                page: "<p>page</p>".to_owned(),
            },
            Content {
                page: " ".to_owned(),
                ..page("Two")
            },
        ] {
            let refused = write(
                world.database.connection_mut(),
                &line,
                &Offered { base: 1, content },
                &laptop,
                &ana,
            )
            .unwrap();
            assert!(
                matches!(refused, Err(Refused::Unreadable(_))),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn a_deleted_line_takes_no_more_writing() {
        let mut world = world();
        let laptop = world.laptop;
        take(&world, &laptop);
        let (line, ana) = (world.line, world.ana.clone());
        lines::delete(world.database.connection_mut(), &line, &ana).unwrap();

        assert_eq!(written(&mut world, &laptop, 2, "Two"), Err(Refused::Gone));
    }

    #[test]
    fn a_stretch_of_writing_ends_when_another_machine_writes_or_it_goes_quiet() {
        let mut world = world();
        let (laptop, desktop) = (world.laptop, world.desktop);
        take(&world, &laptop);
        written(&mut world, &laptop, 1, "Two").unwrap();
        written(&mut world, &laptop, 2, "Three").unwrap();

        assert!(
            uncommitted(world.database.connection()).unwrap().is_empty(),
            "still writing"
        );

        claims::release(
            world.database.connection(),
            &claims::holder(
                world.database.connection(),
                &world.group,
                Side::Shared,
                "quotients",
                &world.ana,
            )
            .unwrap()
            .unwrap()
            .id,
        )
        .unwrap();
        take(&world, &desktop);
        written(&mut world, &desktop, 3, "Four").unwrap();

        let ready = uncommitted(world.database.connection()).unwrap();
        assert_eq!(ready.len(), 1, "the laptop's stretch is over");
        assert_eq!(ready[0].last.number, 3, "and git gets where it ended");

        committed(
            world.database.connection(),
            &ready[0].session,
            &world.line,
            3,
            Some("abc"),
        )
        .unwrap();
        assert!(uncommitted(world.database.connection()).unwrap().is_empty());

        world
            .database
            .connection()
            .execute(
                "UPDATE writing_sessions SET last_at = datetime('now', '-1 hour')",
                [],
            )
            .unwrap();
        assert_eq!(end_quiet(world.database.connection()).unwrap(), 1);
        assert_eq!(
            uncommitted(world.database.connection()).unwrap()[0]
                .last
                .number,
            4
        );
    }

    #[test]
    fn a_stretch_nothing_built_is_owed_a_build_until_something_covers_it() {
        let mut world = world();
        let laptop = world.laptop;
        take(&world, &laptop);
        written(&mut world, &laptop, 1, "Two").unwrap();
        end_writing(world.database.connection(), &world.line).unwrap();
        let ready = uncommitted(world.database.connection()).unwrap();
        committed(
            world.database.connection(),
            &ready[0].session,
            &world.line,
            2,
            Some("abc"),
        )
        .unwrap();

        assert!(
            unbuilt(world.database.connection()).unwrap().is_empty(),
            "still inside its grace"
        );

        let ago = |connection: &Connection| {
            connection
                .execute(
                    "UPDATE writing_sessions SET closed_at = datetime('now', '-1 hour')",
                    [],
                )
                .unwrap();
        };
        ago(world.database.connection());
        let owed = unbuilt(world.database.connection()).unwrap();
        assert_eq!(owed.len(), 1);
        assert_eq!(owed[0].line.revision, 2);

        built_up_to(world.database.connection(), &world.line, 2).unwrap();
        assert!(unbuilt(world.database.connection()).unwrap().is_empty());
    }
}

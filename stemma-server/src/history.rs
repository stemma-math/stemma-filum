//! Telling git what the lines came to.
//!
//! Git is the history, and the history is kept in stretches: one commit
//! for each stretch of somebody writing a line, however many revisions
//! it took. A stretch ends in the database first; this is what then
//! commits it, as the person who wrote it, in the repository the line was
//! in. Whatever fails is tried again the next time, because nothing is
//! marked committed until git has the commit.

use stemma_core::id::LineId;
use stemma_core::repository::Side;

use crate::store::revisions::{self, Uncommitted};
use crate::store::users;
use crate::web::{Instance, WebError};

/// Commits every stretch of writing that is over, and says how many.
pub async fn commit_ended(instance: &Instance) -> Result<usize, WebError> {
    let ready = instance.read(revisions::uncommitted).await?;
    let mut done = 0;

    for one in ready {
        commit(instance, one).await?;
        done += 1;
    }

    Ok(done)
}

/// Ends whatever stretch of writing a line is in and commits it, so git
/// has everything the line says before something else is done to it.
pub async fn settle(instance: &Instance, line: LineId) -> Result<(), WebError> {
    instance
        .read(move |connection| revisions::end_writing(connection, &line))
        .await?;
    commit_ended(instance).await?;

    Ok(())
}

/// Ends the stretches of writing that went quiet, and commits them.
pub async fn tidy(instance: &Instance) -> Result<usize, WebError> {
    instance.read(revisions::end_quiet).await?;
    commit_ended(instance).await
}

/// Commits one stretch.
async fn commit(instance: &Instance, one: Uncommitted) -> Result<(), WebError> {
    let writer = one.writer.clone();
    let Some(user) = instance
        .read(move |connection| users::by_id(connection, &writer))
        .await?
    else {
        return Ok(());
    };

    let line = one.line.clone();
    let last = one.last.clone();
    let committed = instance
        .writing_git(move |repositories| {
            let path = match last.side {
                Side::Shared => repositories.shared(&line.group),
                Side::Private => repositories.personal(&line.group, &line.author),
            };

            crate::git::write_line(
                &path,
                &line.name,
                &last.source,
                last.lean_source.as_deref(),
                &crate::git::Author {
                    name: &user.display_name,
                    email: &user.email,
                },
                &format!("Write {}", line.name),
            )
        })
        .await?;

    let (session, line, number) = (one.session, one.line.id, one.last.number);
    instance
        .read(move |connection| {
            revisions::committed(connection, &session, &line, number, committed.as_deref())
        })
        .await?;

    Ok(())
}

//! Starting a line from the command line, and moving it to the group.
//!
//! The same two acts the web offers, because a machine with no desktop
//! has to be able to do everything: a node over SSH is somebody's
//! working machine too.

use std::io::Write;
use std::path::Path;

use anyhow::Result;

use crate::account::session;
use crate::api::{Sharing, Starting};
use crate::home::Home;
use crate::mirror::{Instance, Mirror, Which};

/// Starts a line, and says where it landed.
pub fn start(
    home: &Home,
    group: &str,
    title: &str,
    description: Option<&str>,
    shared: bool,
    out: &mut impl Write,
) -> Result<()> {
    let (credentials, server) = session::connect(home)?;

    let placed = server.start_line(
        &credentials.token,
        &Starting {
            group: group.to_owned(),
            title: title.to_owned(),
            description: description.map(str::to_owned),
            seen_by: match shared {
                true => "group".to_owned(),
                false => "me".to_owned(),
            },
        },
    )?;

    writeln!(out, "{}/{}/{}", placed.group, placed.side, placed.line)?;
    writeln!(
        out,
        "  {}",
        match placed.side.as_str() {
            "shared" => "the whole group can see it",
            _ => "only you can see it",
        }
    )?;
    bring_here(home, out)
}

/// Takes a line of one's own out of the repository.
pub fn delete(home: &Home, at: &Path, line: Option<&str>, out: &mut impl Write) -> Result<()> {
    let (credentials, server) = session::connect(home)?;
    let (group, _, name) = crate::repository::line_at(at, line)?;

    server.delete_line(&credentials.token, &group, name.as_str())?;

    writeln!(out, "{name}  deleted")?;
    writeln!(out, "  its history is kept")?;

    bring_here(home, out)
}

/// Moves a line to the group.
pub fn share(
    home: &Home,
    at: &Path,
    line: Option<&str>,
    history: bool,
    author_writes: bool,
    out: &mut impl Write,
) -> Result<()> {
    let (credentials, server) = session::connect(home)?;
    let (group, _, name) = crate::repository::line_at(at, line)?;

    let placed = server.share_line(
        &credentials.token,
        &group,
        name.as_str(),
        &Sharing {
            travels: match history {
                true => "everything".to_owned(),
                false => "today".to_owned(),
            },
            writing: match author_writes {
                true => "author".to_owned(),
                false => "anyone".to_owned(),
            },
        },
    )?;

    writeln!(out, "{}/{}/{}", placed.group, placed.side, placed.line)?;
    writeln!(
        out,
        "  shared with the group{}",
        match history {
            true => ", with its full history",
            false => ", current state only",
        }
    )?;
    writeln!(
        out,
        "  {}",
        match author_writes {
            true => "only you can edit it; others send proposals",
            false => "anyone in the group can edit it, one at a time",
        }
    )?;
    bring_here(home, out)
}

/// Brings what the instance just did to this machine, and says what
/// could not be.
fn bring_here(home: &Home, out: &mut impl Write) -> Result<()> {
    let report = Mirror::at(home)?.step(&Instance::connect(home)?, true, &Which::All);

    if let Some(stopped) = &report.stopped {
        writeln!(
            out,
            "  it reaches this machine on the next round: {stopped}"
        )?;
    }
    for aside in &report.aside {
        writeln!(
            out,
            "  a version of {} was kept at {}, because {}",
            aside.line,
            aside.path.display(),
            aside.why
        )?;
    }

    Ok(())
}

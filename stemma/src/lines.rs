//! Starting a line from the command line, and moving it to the group.
//!
//! The same two acts the web offers, because a machine with no desktop
//! has to be able to do everything: a node over SSH is somebody's
//! working machine too.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use crate::api::{Sharing, Starting};
use crate::home::Home;
use crate::session;

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
    writeln!(out, "  run `stemma sync` to get it on this machine")?;

    Ok(())
}

/// Takes a line of one's own out of the repository.
pub fn delete(home: &Home, at: &Path, line: Option<&str>, out: &mut impl Write) -> Result<()> {
    let (credentials, server) = session::connect(home)?;

    let clone = crate::git::containing(at)
        .context("this is not inside a clone: run it from a group's repository")?;
    let manifest = crate::build::read_manifest(&clone)?;
    let name = crate::jobs::line_of(&clone, at, line)?;

    server.delete_line(
        &credentials.token,
        manifest.group.slug().as_str(),
        name.as_str(),
    )?;

    writeln!(out, "{name}  deleted")?;
    writeln!(out, "  its history is kept")?;
    writeln!(out, "  run `stemma sync` to remove it from this machine")?;

    Ok(())
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

    let clone = crate::git::containing(at)
        .context("this is not inside a clone: run it from a group's repository")?;
    let manifest = crate::build::read_manifest(&clone)?;
    let name = crate::jobs::line_of(&clone, at, line)?;

    let placed = server.share_line(
        &credentials.token,
        manifest.group.slug().as_str(),
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
    writeln!(out, "  run `stemma sync` to update this machine")?;

    Ok(())
}

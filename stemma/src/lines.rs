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
    question: &str,
    shared: bool,
    out: &mut impl Write,
) -> Result<()> {
    let (credentials, server) = session::connect(home)?;

    let placed = server.start_line(
        &credentials.token,
        &Starting {
            group: group.to_owned(),
            title: title.to_owned(),
            question: question.to_owned(),
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
    writeln!(out, "  `stemma sync` brings it down to this machine")?;

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

    writeln!(out, "{name}  taken out")?;
    writeln!(
        out,
        "  every version of it stays in the repository's history"
    )?;
    writeln!(out, "  `stemma sync` takes it out of this machine too")?;

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
        "  the group has it{}",
        match history {
            true => ", with everything you wrote getting there",
            false => ", as it stands today",
        }
    )?;
    writeln!(
        out,
        "  {}",
        match author_writes {
            true => "you write it; everybody else proposes",
            false => "anybody in the group writes it, one at a time",
        }
    )?;
    writeln!(out, "  `stemma sync` moves it in this machine's clones too")?;

    Ok(())
}

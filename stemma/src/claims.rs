//! Taking the line this machine is writing, and letting it go.
//!
//! A claim belongs to the machine, so these commands are about this one
//! and no other. Taking a line somebody's other machine is writing does
//! not wrestle it away: it says which machine has it, and offers to ask
//! for it.
//!
//! Nothing here is a warning. A line held elsewhere is an ordinary state
//! of a person who owns two computers, and the claim lapses on its own.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use stemma_core::claim::Held;

use crate::api::{About, Claimed, Server};
use crate::build;
use crate::git;
use crate::home::Home;
use crate::session;

/// Takes the line, if nothing else has it.
pub fn take(home: &Home, at: &Path, line: Option<&str>, out: &mut impl Write) -> Result<()> {
    let (server, token, about) = addressed(home, at, line)?;
    let claimed = server.take_claim(&token, &about)?;

    say(&about, &claimed, out)?;

    if !claimed.may_write() {
        writeln!(
            out,
            "  ask for it with `stemma claim ask`, or wait: a claim lapses on its own"
        )?;
    }

    Ok(())
}

/// Lets the line go, having written what was written.
///
/// In that order: a line let go still holding uncommitted work is work
/// nobody else can see on a line this machine no longer claims. If the
/// writing cannot be recorded it is said and the claim is let go all
/// the same — a claim nobody can release is worse than one released
/// with the work still on the machine, and synchronising will offer to
/// record it again.
pub fn release(home: &Home, at: &Path, line: Option<&str>, out: &mut impl Write) -> Result<()> {
    let (server, token, about) = addressed(home, at, line)?;

    match crate::writing::record(at) {
        Ok(Some(recorded)) => writeln!(out, "{recorded}")?,
        Ok(None) => {}
        Err(error) => writeln!(out, "  it is not in the history yet: {error:#}")?,
    }

    server.release_claim(&token, &about)?;

    writeln!(out, "{}  nobody is writing it now", about.line)?;

    Ok(())
}

/// Asks whichever machine has the line to hand it back.
pub fn ask(home: &Home, at: &Path, line: Option<&str>, out: &mut impl Write) -> Result<()> {
    let (server, token, about) = addressed(home, at, line)?;
    let claimed = server.ask_for_claim(&token, &about)?;

    say(&about, &claimed, out)?;

    Ok(())
}

/// Says which lines are being written, and on what.
pub fn status(home: &Home, out: &mut impl Write) -> Result<()> {
    let (credentials, server) = session::connect(home)?;
    let writing = server.claims(&credentials.token)?;

    if writing.is_empty() {
        writeln!(out, "No line is being written anywhere you work.")?;

        return Ok(());
    }

    for line in writing {
        let here = match line.mine {
            true => " · this machine",
            false => "",
        };

        writeln!(
            out,
            "{}/{}/{}  {}{here}",
            line.group, line.side, line.line, line.label
        )?;
        writeln!(
            out,
            "  {}{}",
            lapsing(line.left),
            match line.asked {
                true => ", and it has been asked for",
                false => "",
            }
        )?;
    }

    Ok(())
}

/// What a claim says, in one or two lines.
fn say(about: &About, claimed: &Claimed, out: &mut impl Write) -> Result<()> {
    writeln!(
        out,
        "{}  {}",
        about.line,
        claimed.said.as_deref().unwrap_or("nobody is writing it")
    )?;

    if let Some(left) = claimed.left {
        writeln!(out, "  {}", lapsing(left))?;
    }

    Ok(())
}

/// How long a claim has left, in the words the rest of the product uses.
fn lapsing(left: u64) -> String {
    Held {
        machine: stemma_core::id::MachineId::generate(),
        label: String::new(),
        left,
        asked: false,
    }
    .lapses_in()
}

/// The instance, the credential, and which line the command is about.
fn addressed(home: &Home, at: &Path, line: Option<&str>) -> Result<(Server, String, About)> {
    let (credentials, server) = session::connect(home)?;

    let clone = git::containing(at)
        .context("this is not inside a clone: run it from a group's repository")?;
    let manifest = build::read_manifest(&clone)?;
    let name = crate::jobs::line_of(&clone, at, line)?;

    Ok((
        server,
        credentials.token,
        About {
            group: manifest.group.slug().as_str().to_owned(),
            side: manifest.side.as_str().to_owned(),
            line: name.as_str().to_owned(),
        },
    ))
}

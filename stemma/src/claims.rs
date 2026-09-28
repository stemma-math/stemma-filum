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

use anyhow::Result;
use stemma_core::claim::Held;

use crate::account::session;
use crate::api::{About, Claimed, Server};
use crate::home::Home;
use crate::mirror::{Instance, Mirror, Which};

/// Takes the line, if nothing else has it.
pub fn take(home: &Home, at: &Path, line: Option<&str>, out: &mut impl Write) -> Result<()> {
    let (server, token, about) = addressed(home, at, line)?;
    let claimed = server.take_claim(&token, &about)?;

    say(&about, &claimed, out)?;

    if !claimed.may_write() {
        writeln!(
            out,
            "  request it with `stemma claim request`, or wait for the claim to expire"
        )?;
    }

    Ok(())
}

/// Lets the line go, having sent what was written.
///
/// In that order: a line let go with writing still here is writing the
/// next machine to take it does not see. If it cannot be sent it is
/// said, and the line is let go all the same — a claim nobody can
/// release is worse than writing that leaves on the next round.
pub fn release(home: &Home, at: &Path, line: Option<&str>, out: &mut impl Write) -> Result<()> {
    let (server, token, about) = addressed(home, at, line)?;

    let waiting =
        send_before_release(home, &about).unwrap_or_else(|error| Some(format!("{error:#}")));
    if let Some(waiting) = waiting {
        writeln!(out, "  not sent yet: {waiting}")?;
    }

    server.release_claim(&token, &about)?;

    writeln!(out, "{}  released", about.line)?;

    Ok(())
}

/// Sends whatever this machine has of a line, before it is let go; why
/// it could not be, when it could not.
pub fn send_before_release(home: &Home, about: &About) -> Result<Option<String>> {
    let mirror = Mirror::at(home)?;
    let side = about.side.parse()?;
    let Some(record) = mirror.find(&about.group, side, &about.line)? else {
        return Ok(None);
    };

    if !record.pending() {
        return Ok(None);
    }

    let report = mirror.send(&Instance::connect(home)?, &Which::Line(record.id));

    Ok(report
        .stopped
        .or_else(|| report.waiting.into_iter().next().map(|waiting| waiting.why)))
}

/// Asks whichever machine has the line to hand it back.
pub fn request(home: &Home, at: &Path, line: Option<&str>, out: &mut impl Write) -> Result<()> {
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
        writeln!(out, "No lines are claimed.")?;

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
                true => ", release requested",
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
        claimed.said.as_deref().unwrap_or("not claimed")
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
    let (group, side, name) = crate::repository::line_at(at, line)?;

    Ok((
        server,
        credentials.token,
        About {
            group,
            side: side.as_str().to_owned(),
            line: name.as_str().to_owned(),
        },
    ))
}

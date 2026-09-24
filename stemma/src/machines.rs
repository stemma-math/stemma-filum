//! What machines a group has, and what each of them can run.
//!
//! The launcher in the application shows this beside the mathematics.
//! Here it is a list, because a person about to write `--machine` has
//! to know what to write and what it would be able to do.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use crate::home::Home;
use crate::session;

/// Lists the machines serving a group.
pub fn list(home: &Home, at: &Path, group: Option<&str>, out: &mut impl Write) -> Result<()> {
    let (credentials, server) = session::connect(home)?;
    let group = match group {
        Some(named) => named.to_owned(),
        None => {
            let clone = crate::git::containing(at).context(
                "say which group with --group, or run this inside one of its repositories",
            )?;

            crate::build::read_manifest(&clone)?
                .group
                .slug()
                .as_str()
                .to_owned()
        }
    };

    let machines = server.machines(&credentials.token, &group)?;

    if machines.is_empty() {
        writeln!(out, "No machine works for {group} yet.")?;

        return Ok(());
    }

    for machine in machines {
        writeln!(
            out,
            "{}  {}  {}",
            machine.machine, machine.label, machine.presence
        )?;

        for agent in machine.agents {
            let called = stemma_core::machine::known(agent.agent.as_str())
                .map(|known| known.called)
                .unwrap_or(agent.agent.as_str());

            writeln!(
                out,
                "  {:<14} {}",
                called,
                // An agent a machine does not have is named with its
                // reason rather than left out: the owner can install it.
                agent.availability.reason().unwrap_or("ready")
            )?;
        }
    }

    Ok(())
}

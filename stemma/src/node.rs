//! Offering this machine as one that runs work.
//!
//! Signing in and offering to work are two acts. A laptop somebody only
//! writes on signs in and never registers; a workstation in a corner
//! does both, over SSH, with no desktop anywhere near it.
//!
//! What this machine can do is found by looking, not by being told. The
//! instance is sent what was found, including the agents that are not
//! here, because an owner can install one and a shorter list would say
//! nothing about why.

use std::io::Write;

use anyhow::{Context, Result};
use stemma_core::machine::{Availability, Detected};

use crate::account::session;
use crate::api::Node;
use crate::config::Config;
use crate::home::Home;

/// Registers this machine, and says what it will work on.
///
/// Given no groups, it offers itself for every group its owner is in:
/// somebody who went to the trouble of registering a machine meant it to
/// be useful, and turning one off later is a click.
pub fn init(home: &Home, groups: &[String], out: &mut impl Write) -> Result<Node> {
    let (credentials, server) = session::connect(home)?;
    let config = Config::read(&home.config_file())?;

    writeln!(out, "Looking for agents on this machine...")?;
    let agents = crate::job::agents::look(home, &config);

    let serving: Vec<String> = match groups.is_empty() {
        false => groups.iter().map(|group| group.trim().to_owned()).collect(),
        true => server
            .workspace(&credentials.token)?
            .groups
            .into_iter()
            .map(|group| group.slug)
            .collect(),
    };

    let node = server
        .register_node(
            &credentials.token,
            &session::machine_name(),
            &session::machine_key(home)?,
            &agents,
            Some(&serving),
        )
        .context("this machine could not be registered")?;

    writeln!(out)?;
    report(&node, &credentials.server, out)?;

    Ok(node)
}

/// Gives this machine the name its owner chose.
pub fn rename(home: &Home, name: &str, out: &mut impl Write) -> Result<Node> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "a machine needs a name");
    let (credentials, server) = session::connect(home)?;
    let node = server
        .rename_node(&credentials.token, name)
        .context("this machine could not be renamed")?;
    writeln!(out, "This machine is now called {}.", node.label)?;

    Ok(node)
}

/// Says what the instance makes of this machine.
pub fn status(home: &Home, out: &mut impl Write) -> Result<bool> {
    let (credentials, server) = session::connect(home)?;

    let Some(node) = server.node(&credentials.token)? else {
        writeln!(
            out,
            "This machine is signed in to {} and is not registered to run jobs.",
            credentials.server
        )?;
        writeln!(out, "Run `stemma node init` to register it.")?;

        return Ok(false);
    };

    report(&node, &credentials.server, out)?;

    Ok(true)
}

/// One machine, as its owner reads it at a terminal.
fn report(node: &Node, server: &str, out: &mut impl Write) -> Result<()> {
    writeln!(out, "{}", node.label)?;
    writeln!(out, "  {} on {}", node.machine, server)?;
    writeln!(out, "  {}", node.presence)?;

    writeln!(
        out,
        "  works for {}",
        match node.groups.is_empty() {
            true => "no groups yet".to_owned(),
            false => node.groups.join(", "),
        }
    )?;

    writeln!(out)?;

    for agent in &node.agents {
        writeln!(out, "  {}", described(agent))?;
    }

    Ok(())
}

/// One agent, in a line.
///
/// Every agent is listed, and the ones that are not here say so. Their
/// absence is something their owner can act on.
fn described(agent: &Detected) -> String {
    let called = stemma_core::machine::known(agent.agent.as_str())
        .map(|known| known.called)
        .unwrap_or_else(|| agent.agent.as_str());

    let said = match (&agent.availability, &agent.version) {
        (Availability::Ready, Some(version)) => version.clone(),
        (Availability::Ready, None) => "here".to_owned(),
        (other, _) => other.reason().unwrap_or("unreachable").to_owned(),
    };

    format!("{:<12} {called:<16} {said}", agent.agent.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use stemma_core::id::Slug;

    #[test]
    fn an_agent_that_is_here_shows_its_version_and_one_that_is_not_says_why() {
        let claude = Slug::parse("claude").unwrap();

        let here = described(&Detected::ready(claude.clone(), Some("1.2.3".to_owned())));
        assert!(here.contains("Claude Code"));
        assert!(here.contains("1.2.3"));

        let missing = described(&Detected::missing(claude));
        assert!(missing.contains("not installed on this machine"));
    }

    #[test]
    fn a_machine_that_is_not_signed_in_is_told_what_to_run() {
        let home = Home::locate(&crate::home::Environment::from_pairs([(
            crate::home::HOME_VARIABLE,
            "/nonexistent/stemma",
        )]));

        let error = status(&home, &mut Vec::new()).unwrap_err();
        assert!(format!("{error:#}").contains("stemma login"));
    }
}

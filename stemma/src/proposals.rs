//! Writing accepted mathematics on its author's machine.

use std::io::Write;

use anyhow::{Context, Result, bail};
use stemma_core::id::ProposalId;
use stemma_core::repository::{self, Side};

use crate::api::Server;
use crate::config::Config;
use crate::home::Home;

/// Fetches an accepted proposal, writes it into the matching clean clone and commits it.
pub fn apply(home: &Home, proposal: &str, out: &mut impl Write) -> Result<()> {
    let proposal: ProposalId = proposal
        .parse()
        .context("that is not a proposal identifier")?;
    let credentials = crate::session::required(home)?;
    let server = Server::at(&credentials.server)?;
    let accepted = server.accepted_proposal(&credentials.token, &proposal.to_string())?;
    let config = Config::read(&home.config_file())?;
    let side: Side = accepted
        .side
        .parse()
        .context("the proposal names no repository")?;
    let clone = config
        .group_directory(&accepted.group, &config.workspace_or(home.workspace()))
        .join(match side {
            Side::Shared => crate::workspace::Side::Shared.directory(),
            Side::Private => crate::workspace::Side::Private.directory(),
        });

    if crate::git::head(&clone)? != accepted.commit {
        bail!(
            "the clone has moved since this proposal was made; review it again against the current line"
        )
    }
    if crate::git::is_dirty(&clone)? {
        bail!("the clone has uncommitted work; keep it before applying the proposal")
    }

    let path = clone
        .join(repository::LINES)
        .join(&accepted.line)
        .join(repository::ENTRY);
    std::fs::write(&path, &accepted.source)
        .with_context(|| format!("cannot write {}", path.display()))?;
    crate::git::commit_path(&clone, &path, &format!("Apply proposal {proposal}"))?;
    writeln!(
        out,
        "Applied {proposal} to {}/{}.",
        accepted.group, accepted.line
    )?;
    Ok(())
}

//! Reviewing what an agent offered, and writing it down once it is
//! yours.
//!
//! The same review as in the web, in a terminal: what moved, what the
//! instance checked, and the three answers. Accepting signs the words
//! as the accepter's own — which is why nothing here accepts anything
//! without being asked to.

use std::io::Write;

use anyhow::{Context, Result, bail};
use stemma_core::id::ProposalId;
use stemma_core::repository::{self, Side};

use crate::api::Server;
use crate::config::Config;
use crate::home::Home;

/// Says what is waiting for this person.
pub fn list(home: &Home, out: &mut impl Write) -> Result<()> {
    let credentials = crate::session::required(home)?;
    let server = Server::at(&credentials.server)?;
    let waiting = server.proposals(&credentials.token)?;

    if waiting.is_empty() {
        writeln!(out, "Nothing is waiting for you to read it.")?;

        return Ok(());
    }

    for proposal in waiting {
        writeln!(out, "{}  {}", proposal.proposal, proposal.line)?;
        writeln!(
            out,
            "  {} · {} · {} {}",
            proposal.task, proposal.agent, proposal.vendor, proposal.model
        )?;
        writeln!(out, "  `stemma proposal show {}`", proposal.proposal)?;
    }

    Ok(())
}

/// Shows one proposal: what moved, and what was checked.
pub fn show(home: &Home, proposal: &str, out: &mut impl Write) -> Result<()> {
    let credentials = crate::session::required(home)?;
    let server = Server::at(&credentials.server)?;
    let offered = server.proposal(&credentials.token, proposal)?;

    writeln!(
        out,
        "{}  {}/{}/{}",
        offered.proposal, offered.group, offered.side, offered.line
    )?;
    writeln!(
        out,
        "  {} · {} · {} {} · prompt {}",
        offered.task, offered.agent, offered.vendor, offered.model, offered.prompt_revision
    )?;
    writeln!(out, "  {}", offered.state)?;
    writeln!(out)?;

    match &offered.stands {
        Some(stands) => {
            let rows = stemma_core::diff::between(stands, &offered.source);
            let frozen = stemma_core::diff::frozen(stands);

            writeln!(out, "{}", stemma_core::diff::extent(&rows).said())?;
            writeln!(out)?;

            for row in rows {
                // A line the guard freezes is marked wherever it is
                // shown, so that "it did not touch it" is visible
                // rather than promised.
                let held = match row.before.is_some_and(|at| frozen.contains(&at)) {
                    true => "|",
                    false => " ",
                };
                let mark = match row.change {
                    stemma_core::diff::Change::Added => "+",
                    stemma_core::diff::Change::Removed => "-",
                    stemma_core::diff::Change::Kept => " ",
                };

                writeln!(out, "{held}{mark} {}", row.text)?;
            }
        }
        None => {
            writeln!(
                out,
                "The version this was written against is not on the instance any more, \
                 so here is the whole of what is proposed."
            )?;
            writeln!(out)?;
            writeln!(out, "{}", offered.source)?;
        }
    }

    if let Some(lean) = &offered.lean_source {
        writeln!(out)?;
        writeln!(out, "Lean module:")?;
        writeln!(out, "{lean}")?;
    }

    writeln!(out)?;
    writeln!(out, "What was checked:")?;

    for finding in &offered.findings {
        writeln!(out, "  {finding}")?;
    }

    if offered.state == "awaiting_review" {
        writeln!(out)?;
        writeln!(
            out,
            "`stemma proposal accept {0}`, `reject {0}` or `ask-again {0}`",
            offered.proposal
        )?;
    }

    Ok(())
}

/// Records a decision. Accepting makes the words the accepter's own.
pub fn decide(home: &Home, proposal: &str, decision: &str, out: &mut impl Write) -> Result<()> {
    let credentials = crate::session::required(home)?;
    let server = Server::at(&credentials.server)?;

    server.decide_proposal(&credentials.token, proposal, decision)?;

    match decision {
        "accept" => {
            writeln!(out, "{proposal}  accepted")?;
            writeln!(
                out,
                "  these are your words now, with the model recorded beside them"
            )?;
            writeln!(out, "  `stemma proposal apply {proposal}` writes them here")?;
        }
        "ask_again" => {
            writeln!(
                out,
                "{proposal}  declined, and the same question is back in the queue"
            )?;
        }
        _ => writeln!(out, "{proposal}  rejected")?,
    }

    Ok(())
}

/// Fetches an accepted proposal, writes it into the matching clean clone and commits it.
pub fn apply(home: &Home, proposal: &str, out: &mut impl Write) -> Result<()> {
    let proposal: ProposalId = proposal
        .parse()
        .context("that is not a proposal identifier")?;
    let credentials = crate::session::required(home)?;
    let server = Server::at(&credentials.server)?;
    let accepted = server.accepted_proposal(&credentials.token, &proposal.to_string())?;
    stemma_core::id::Slug::parse(&accepted.group).context("the proposal names no group")?;
    let line = repository::check_line_name(&accepted.line).context("the proposal names no line")?;
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

    let path = repository::line_entry(&clone, &line);
    std::fs::write(&path, &accepted.source)
        .with_context(|| format!("cannot write {}", path.display()))?;
    let mut paths = vec![path];
    if let Some(lean_source) = accepted.lean_source {
        let lean_path = repository::line_lean_entry(&clone, &line);
        std::fs::write(&lean_path, lean_source)
            .with_context(|| format!("cannot write {}", lean_path.display()))?;
        paths.push(lean_path);
    }
    crate::git::commit_paths(&clone, &paths, &format!("Apply proposal {proposal}"))?;
    writeln!(
        out,
        "Applied {proposal} to {}/{}.",
        accepted.group, accepted.line
    )?;
    Ok(())
}

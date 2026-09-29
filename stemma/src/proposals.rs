//! Reviewing what an agent offered, and writing it down once it is
//! yours.
//!
//! The same review as in the web, in a terminal: what moved, what the
//! instance checked, and the three answers. Accepting signs the words
//! as the accepter's own — which is why nothing here accepts anything
//! without being asked to.

use std::io::Write;
use stemma_core::proposal::Verdict;

use anyhow::{Context, Result};
use stemma_core::id::ProposalId;

use crate::account::session;
use crate::api::AcceptedProposal;
use crate::home::Home;
use crate::mirror::{Applied, Instance, Mirror};

/// Says what is waiting for this person.
pub fn list(home: &Home, out: &mut impl Write) -> Result<()> {
    let (credentials, server) = session::connect(home)?;
    let waiting = server.proposals(&credentials.token)?;

    if waiting.is_empty() {
        writeln!(out, "No proposals waiting for review.")?;

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
    let (credentials, server) = session::connect(home)?;
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
    if let Some(effort) = &offered.effort {
        writeln!(out, "  effort {effort}")?;
    }
    if !offered.explanation.is_empty() {
        writeln!(out, "  {}", offered.explanation)?;
    }
    if !offered.operations.is_empty() {
        writeln!(out, "  {} scoped operation(s)", offered.operations.len())?;
    }
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
                // shown, so that "unchanged" is visible
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
pub fn decide(home: &Home, proposal: &str, verdict: Verdict, out: &mut impl Write) -> Result<()> {
    decide_with_note(home, proposal, verdict, None, out)
}

/// Records the decision and any reviewer note for the next attempt.
pub fn decide_with_note(
    home: &Home,
    proposal: &str,
    verdict: Verdict,
    note: Option<&str>,
    out: &mut impl Write,
) -> Result<()> {
    let (credentials, server) = session::connect(home)?;

    server.decide_proposal_with_note(&credentials.token, proposal, verdict, note)?;

    match verdict {
        Verdict::Accept => {
            writeln!(out, "{proposal}  accepted")?;
            writeln!(
                out,
                "  it is recorded as yours, with the model that proposed it"
            )?;
            apply(home, proposal, out)?;
        }
        Verdict::AskAgain => {
            writeln!(out, "{proposal}  rejected, and the task was queued again")?;
        }
        Verdict::Reject => writeln!(out, "{proposal}  rejected")?,
    }

    Ok(())
}

/// Merges an accepted proposal into its line and writes it as the
/// accepter's revision, or marks it outdated when a block it changes was
/// changed since it was made.
pub fn apply(home: &Home, proposal: &str, out: &mut impl Write) -> Result<()> {
    let proposal: ProposalId = proposal
        .parse()
        .context("that is not a proposal identifier")?;
    let instance = Instance::connect(home)?;
    let accepted = instance
        .server()
        .accepted_proposal(instance.token(), &proposal.to_string())?;

    let said = applied(
        &Mirror::at(home)?,
        &instance,
        &proposal.to_string(),
        &accepted,
    )?;
    writeln!(out, "{proposal}  {said}")?;

    Ok(())
}

/// Applies every proposal this person accepted that no machine of theirs
/// has applied yet: a decision made offline, or on the web, is carried
/// out here. Says what came of each.
pub fn apply_waiting(mirror: &Mirror, instance: &Instance) -> Result<Vec<String>> {
    let waiting = instance.server().accepted_proposals(instance.token())?;
    let mut said = Vec::new();

    for one in waiting {
        let outcome = applied(mirror, instance, &one.proposal, &one.accepted)
            .unwrap_or_else(|error| format!("not applied yet: {error:#}"));
        said.push(format!("{}  {outcome}", one.proposal));
    }

    Ok(said)
}

/// Applies an accepted proposal and tells the instance what came of it,
/// in words.
pub fn applied(
    mirror: &Mirror,
    instance: &Instance,
    proposal: &str,
    accepted: &AcceptedProposal,
) -> Result<String> {
    Ok(match mirror.apply(instance, accepted)? {
        Applied::Written(revision) => {
            instance
                .server()
                .proposal_applied(instance.token(), proposal, revision)?;
            format!("applied to {}/{}", accepted.group, accepted.line)
        }
        Applied::Outdated => {
            instance
                .server()
                .proposal_outdated(instance.token(), proposal)?;
            "outdated: a block it changes was changed since it was made".to_owned()
        }
        Applied::Queued(why) => format!("merged here, not sent yet: {why}"),
    })
}

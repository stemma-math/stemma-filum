//! Changes offered for a person to review.
//!
//! A proposal is not mathematics until somebody accepts it. The guard in
//! this module needs no mathematical judgement: it compares the source at
//! the job's base commit with the source offered by the agent and refuses
//! any change to the text or declared dependencies of a central statement.

use serde::{Deserialize, Serialize};

use crate::actor::{AgentRun, Attribution};
use crate::document::{Block, Document};
use crate::fact::{Fact, FactKind};
use crate::id::{JobId, ProposalId, UserId};
use crate::job::Outcome;
use crate::line::{Line, Siglum};

/// The one atomic report with which a machine completes a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    /// What happened, in the group's typed vocabulary.
    pub outcome: Outcome,
    /// The reviewable change, only when one was produced.
    pub proposal: Option<Offer>,
}

/// What a machine sends when an agent produced a reviewable change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    /// The complete replacement source.
    pub source: String,
    /// The complete Lean module beside the source, when formal work changed.
    #[serde(default)]
    pub lean_source: Option<String>,
    /// Who supplied the agent.
    pub vendor: String,
    /// The concrete model the agent reported using.
    pub model: String,
    /// Which prompt contract produced it.
    pub prompt_revision: String,
}

/// A change awaiting, or having received, a human decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// Its stable identity.
    pub id: ProposalId,
    /// The job that produced it, when an agent did.
    pub job: Option<JobId>,
    /// The complete source it would put in place.
    pub source: String,
    /// The agent run recorded alongside it, when there was one.
    pub proposed_by: Option<AgentRun>,
    /// Where review has got to.
    pub state: ProposalState,
}

/// What a reviewer decides about a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Sign the words as the reviewer's own.
    Accept,
    /// Decline them.
    Reject,
    /// Decline them and queue the same question again.
    AskAgain,
}

spelled!(Verdict, UnknownVerdict("`{0}` is not a decision: use `accept`, `reject` or `ask_again`") {
    Accept => "accept",
    Reject => "reject",
    AskAgain => "ask_again",
});

/// A proposal's human decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProposalState {
    /// Nobody has signed these words yet.
    AwaitingReview,
    /// A person declined them.
    Rejected {
        /// The person who declined it.
        by: UserId,
    },
    /// A person signed them as their own words.
    Accepted {
        /// The human signature, with the proposing run alongside it.
        attribution: Attribution,
    },
}

impl Fact for ProposalState {
    const KIND: FactKind = FactKind::Condition;
}

/// Why an offered source cannot be admitted for review.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardError {
    /// One of the two documents is not a readable line.
    #[error("{which} source is not a valid line: {reason}")]
    InvalidSource {
        /// Which side failed.
        which: &'static str,
        /// Its first useful diagnostic.
        reason: String,
    },
    /// A statement the line rests on disappeared.
    #[error("the proposal removes central statement `{0}`")]
    Removed(Siglum),
    /// It is called something else.
    #[error("the proposal renames central statement `{0}`")]
    Renamed(Siglum),
    /// Its written claim changed.
    #[error("the proposal changes the text of central statement `{0}`")]
    ChangedText(Siglum),
    /// What it declares it leans on changed.
    #[error("the proposal changes what central statement `{0}` uses")]
    ChangedUses(Siglum),
    /// Which audited substitutions it relies on changed.
    #[error("the proposal changes the vault bridges central statement `{0}` uses")]
    ChangedBridges(Siglum),
}

/// Checks that an agent left every existing central statement frozen.
///
/// Statements are matched by block, so a frozen statement cannot be
/// slipped past the guard by renaming it. What it says, what it is
/// called, what it declares it uses and the bridges it relies on are
/// all frozen; its proof is not, and neither is where it stands.
pub fn preserves_central_statements(base: &str, offered: &str) -> Result<(), GuardError> {
    let base = read("base", base)?;
    let offered = read("proposed", offered)?;

    for block in &base.blocks {
        let Some(frozen) = block.statement().filter(|statement| statement.central) else {
            continue;
        };
        let siglum = block.siglum().expect("a statement has a siglum");

        let Some(after) = offered
            .block(&frozen.id)
            .filter(|after| after.kind() == block.kind())
            .and_then(Block::statement)
            .filter(|statement| statement.central)
        else {
            return Err(GuardError::Removed(siglum));
        };

        if after.siglum != frozen.siglum {
            return Err(GuardError::Renamed(siglum));
        }
        if after.content != frozen.content
            || after.environment != frozen.environment
            || after.title != frozen.title
        {
            return Err(GuardError::ChangedText(siglum));
        }
        if after.uses != frozen.uses {
            return Err(GuardError::ChangedUses(siglum));
        }
        if after.bridges != frozen.bridges {
            return Err(GuardError::ChangedBridges(siglum));
        }
    }

    Ok(())
}

/// Reads one side, which has to be a line and not only a document.
fn read(which: &'static str, text: &str) -> Result<Document, GuardError> {
    let invalid = |reason: String| GuardError::InvalidSource { which, reason };
    let document = Document::parse(text).map_err(|unreadable| invalid(unreadable.to_string()))?;

    Line::of(&document).map_err(|faults| {
        invalid(
            faults
                .first()
                .map(ToString::to_string)
                .unwrap_or_else(|| "it is not a line".to_owned()),
        )
    })?;

    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Document {
        let mut document = Document::new("A line", None);
        let group = document.push(Block::definition(
            Some("def:group".parse().unwrap()),
            "A group.",
        ));
        let mut set = Block::definition(Some("def:set".parse().unwrap()), "A set.");
        set.statement_mut().unwrap().central = false;
        let set = document.push(set);
        let mut main = Block::theorem(
            "Theorem",
            Some("thm:main".parse().unwrap()),
            "Every finite group is finite.",
        );
        main.statement_mut().unwrap().central = true;
        main.statement_mut().unwrap().uses.push(group);
        let main = document.push(main);
        document.push(Block::proof(main, "It is."));
        let mut step = Block::theorem(
            "Lemma",
            Some("lem:step".parse().unwrap()),
            "An old argument.",
        );
        step.statement_mut().unwrap().proof = false;
        step.statement_mut().unwrap().uses.push(set);
        document.push(step);
        document
    }

    fn statement(document: &mut Document, at: usize) -> &mut crate::document::Statement {
        document.blocks[at].statement_mut().unwrap()
    }

    fn guarded(original: &Document, offered: &Document) -> Result<(), GuardError> {
        preserves_central_statements(&original.to_text(), &offered.to_text())
    }

    #[test]
    fn an_agent_may_rewrite_the_inside_of_a_box_and_any_proof() {
        let original = base();
        let mut offered = original.clone();
        statement(&mut offered, 4).content = "A much better argument.".to_owned();
        offered.blocks[3] = Block::proof(*offered.blocks[2].id(), "By @lem:step.");
        offered.blocks.swap(0, 1);
        assert_eq!(guarded(&original, &offered), Ok(()));
    }

    #[test]
    fn changing_a_central_claim_is_refused() {
        let original = base();
        let mut offered = original.clone();
        statement(&mut offered, 2).content = "Every group is finite.".to_owned();
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &offered.to_text()),
            Err(GuardError::ChangedText(_))
        ));
    }

    #[test]
    fn a_central_definition_is_frozen_too() {
        let original = base();
        let mut offered = original.clone();
        statement(&mut offered, 0).title = Some("Monoid".to_owned());
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &offered.to_text()),
            Err(GuardError::ChangedText(_))
        ));
    }

    #[test]
    fn changing_a_central_dependency_is_refused() {
        let original = base();
        let mut offered = original.clone();
        let set = *offered.blocks[1].id();
        statement(&mut offered, 2).uses = vec![set];
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &offered.to_text()),
            Err(GuardError::ChangedUses(_))
        ));
    }

    #[test]
    fn removing_or_demoting_or_renaming_a_central_statement_is_refused() {
        let original = base();

        let mut demoted = original.clone();
        statement(&mut demoted, 2).central = false;
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &demoted.to_text()),
            Err(GuardError::Removed(_))
        ));

        let mut renamed = original.clone();
        statement(&mut renamed, 2).siglum = Some("thm:other".parse().unwrap());
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &renamed.to_text()),
            Err(GuardError::Renamed(_))
        ));
    }

    #[test]
    fn an_offer_that_is_not_a_line_is_refused() {
        let original = base();
        let broken = Document {
            blocks: vec![Block::theorem("Theorem", None, "No proof block.")],
            ..original.clone()
        };
        assert!(matches!(
            guarded(&original, &broken),
            Err(GuardError::InvalidSource {
                which: "proposed",
                ..
            })
        ));
    }

    #[test]
    fn a_proposal_state_is_a_condition_not_an_endorsement_record() {
        assert_eq!(ProposalState::KIND, FactKind::Condition);
    }
}

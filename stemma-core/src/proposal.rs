//! Changes offered for a person to review.
//!
//! A proposal is not mathematics until somebody accepts it. The guard in
//! this module needs no mathematical judgement: it compares the source at
//! the job's base commit with the source offered by the agent and refuses
//! any change to the text or declared dependencies of a central statement.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::actor::{AgentRun, Attribution};
use crate::fact::{Fact, FactKind};
use crate::id::{JobId, ProposalId, UserId};
use crate::job::Outcome;
use crate::line::{Line, Siglum};
use crate::source;

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
    /// Its written claim changed.
    #[error("the proposal changes the text of central statement `{0}`")]
    ChangedText(Siglum),
    /// What it declares it leans on changed.
    #[error("the proposal changes what central statement `{0}` uses")]
    ChangedUses(Siglum),
}

/// Checks that an agent left every existing central statement frozen.
pub fn preserves_central_statements(base: &str, offered: &str) -> Result<(), GuardError> {
    let base_line = read_line("base", base)?;
    let offered_line = read_line("proposed", offered)?;
    let base_text = central_text(base, &base_line, "base")?;
    let offered_text = central_text(offered, &offered_line, "proposed")?;

    for central in base_line.central() {
        let Some(after) = offered_line
            .node(&central.siglum)
            .filter(|node| node.is_central())
        else {
            return Err(GuardError::Removed(central.siglum.clone()));
        };

        if base_text.get(&central.siglum) != offered_text.get(&central.siglum) {
            return Err(GuardError::ChangedText(central.siglum.clone()));
        }
        if central.uses != after.uses {
            return Err(GuardError::ChangedUses(central.siglum.clone()));
        }
    }

    Ok(())
}

fn read_line(which: &'static str, text: &str) -> Result<Line, GuardError> {
    Line::read(text).map_err(|faults| GuardError::InvalidSource {
        which,
        reason: faults
            .first()
            .map(ToString::to_string)
            .unwrap_or_else(|| "it is not a line".to_owned()),
    })
}

fn central_text(
    text: &str,
    line: &Line,
    which: &'static str,
) -> Result<BTreeMap<Siglum, String>, GuardError> {
    let declarations = source::read(text).map_err(|problem| GuardError::InvalidSource {
        which,
        reason: problem.to_string(),
    })?;
    let central: Vec<&Siglum> = line
        .central()
        .into_iter()
        .map(|node| &node.siglum)
        .collect();
    let mut found = BTreeMap::new();

    for declaration in declarations {
        let Some(id) = declaration.text("id") else {
            continue;
        };
        let Ok(siglum) = id.parse::<Siglum>() else {
            continue;
        };
        if !central.contains(&&siglum) {
            continue;
        }

        let body = declaration
            .body
            .map(|range| text[range].to_owned())
            .unwrap_or_default();
        found.insert(siglum, body);
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
= A line
#definition(id: "def:group", name: "Group")[A group.]
#definition(id: "def:set", name: "Set")[A set.]
#theorem(id: "thm:main", central: true, uses: "def:group")[Every finite group is finite.]
#lemma(id: "lem:step", uses: "def:group")[An old argument.]
"#;

    #[test]
    fn an_agent_may_rewrite_the_inside_of_a_box() {
        let offered = BASE.replace("An old argument.", "A much better argument.");
        assert_eq!(preserves_central_statements(BASE, &offered), Ok(()));
    }

    #[test]
    fn changing_a_central_claim_is_refused() {
        let offered = BASE.replace("Every finite group is finite.", "Every group is finite.");
        assert!(matches!(
            preserves_central_statements(BASE, &offered),
            Err(GuardError::ChangedText(_))
        ));
    }

    #[test]
    fn changing_a_central_dependency_is_refused() {
        let offered = BASE.replace(
            "uses: \"def:group\")[Every finite",
            "uses: \"def:set\")[Every finite",
        );
        assert!(matches!(
            preserves_central_statements(BASE, &offered),
            Err(GuardError::ChangedUses(_))
        ));
    }

    #[test]
    fn removing_or_demoting_a_central_statement_is_refused() {
        let offered = BASE.replace("central: true, ", "");
        assert!(matches!(
            preserves_central_statements(BASE, &offered),
            Err(GuardError::Removed(_))
        ));
    }

    #[test]
    fn a_proposal_state_is_a_condition_not_an_endorsement_record() {
        assert_eq!(ProposalState::KIND, FactKind::Condition);
    }
}

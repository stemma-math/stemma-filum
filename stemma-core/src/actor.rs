//! Who did something.
//!
//! Every mutation in Stemma is attributed to a concrete actor. An agent is
//! never recorded as "an AI": it is a vendor, a model and the revision of the
//! prompt it was given, run on a named machine by a named person. That person
//! is the one answerable for the change, which is why [`Actor`] can always
//! produce them.

use serde::{Deserialize, Serialize};

use crate::id::{MachineId, UserId};

/// One run of one agent, recorded in enough detail to be reproduced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRun {
    /// Who supplies the agent, as the owner's machine reports it.
    pub vendor: String,
    /// The model the run used.
    pub model: String,
    /// The revision of the prompt Stemma sent.
    pub prompt_revision: String,
    /// The machine the agent ran on.
    pub machine: MachineId,
    /// The person who launched the run and answers for it.
    pub operator: UserId,
}

/// Who performed an act.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "actor", rename_all = "snake_case")]
pub enum Actor {
    /// A person, acting directly.
    Person {
        /// The account that acted.
        user: UserId,
    },
    /// An agent, acting on somebody's behalf.
    Agent {
        /// The run that produced the act.
        run: AgentRun,
    },
}

impl Actor {
    /// The actor for a person acting directly.
    pub fn person(user: UserId) -> Self {
        Self::Person { user }
    }

    /// The actor for one run of an agent.
    pub fn agent(run: AgentRun) -> Self {
        Self::Agent { run }
    }

    /// The person answerable for the act: the actor themselves, or the
    /// operator who launched the agent.
    pub fn responsible(&self) -> &UserId {
        match self {
            Self::Person { user } => user,
            Self::Agent { run } => &run.operator,
        }
    }

    /// The run behind the act, when an agent performed it.
    pub fn run(&self) -> Option<&AgentRun> {
        match self {
            Self::Person { .. } => None,
            Self::Agent { run } => Some(run),
        }
    }
}

/// Whose words a piece of content is.
///
/// Accepting a proposal signs it: the text becomes the accepter's, and the
/// agent that offered it is recorded alongside rather than in place of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribution {
    /// The person the content is attributed to.
    pub author: UserId,
    /// The run that proposed the content, when one did.
    pub proposed_by: Option<AgentRun>,
}

impl Attribution {
    /// Content a person wrote themselves.
    pub fn written_by(author: UserId) -> Self {
        Self {
            author,
            proposed_by: None,
        }
    }

    /// Content an agent proposed and a person accepted.
    pub fn accepted_by(author: UserId, proposed_by: AgentRun) -> Self {
        Self {
            author,
            proposed_by: Some(proposed_by),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> AgentRun {
        AgentRun {
            vendor: "anthropic".to_owned(),
            model: "claude-opus-5".to_owned(),
            prompt_revision: "fill-box/3".to_owned(),
            machine: MachineId::generate(),
            operator: "usr:ana".parse().unwrap(),
        }
    }

    #[test]
    fn an_agent_act_is_answered_for_by_its_operator() {
        let actor = Actor::agent(run());
        assert_eq!(actor.responsible().slug().as_str(), "ana");
    }

    #[test]
    fn accepting_a_proposal_makes_it_the_accepters_words() {
        let ana: UserId = "usr:ana".parse().unwrap();
        let attribution = Attribution::accepted_by(ana.clone(), run());

        assert_eq!(attribution.author, ana);
        assert!(attribution.proposed_by.is_some());
    }
}

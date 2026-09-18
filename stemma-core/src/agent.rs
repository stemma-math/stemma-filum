//! Typed agent failures.
//!
//! A failure says whether the agent was ever able to try. "The agent is not
//! installed here" and "the agent ran and broke" are different facts about
//! different things, and only the second says anything about the job.
//!
//! Agent output carries the owner's account, quota and plan, so it stays on
//! their machine. What reaches the group is the type and a short summary.

use serde::{Deserialize, Serialize};

use crate::id::Slug;

/// Whether the agent got as far as running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attempt {
    /// The agent never ran, so the job learned nothing about the mathematics.
    NotMade,
    /// The agent ran. Whatever happened next is about this job.
    Made,
}

/// Why an agent produced no result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum AgentFailure {
    /// The machine does not have the agent.
    #[error("`{agent}` is not installed on this machine")]
    AgentNotFound {
        /// The agent that was asked for.
        agent: Slug,
    },
    /// The agent is installed but its owner has not signed it in.
    #[error("`{agent}` is not signed in on this machine")]
    AgentNotAuthenticated {
        /// The agent that was asked for.
        agent: Slug,
    },
    /// The vendor refused the request for now.
    #[error("`{agent}` is rate limited on this machine")]
    AgentRateLimited {
        /// The agent that was asked for.
        agent: Slug,
    },
    /// The agent ran and failed.
    #[error("`{agent}` failed: {summary}")]
    AgentError {
        /// The agent that ran.
        agent: Slug,
        /// A short account of the failure, safe to show the group.
        summary: String,
    },
}

impl AgentFailure {
    /// The agent the failure is about.
    pub fn agent(&self) -> &Slug {
        match self {
            Self::AgentNotFound { agent }
            | Self::AgentNotAuthenticated { agent }
            | Self::AgentRateLimited { agent }
            | Self::AgentError { agent, .. } => agent,
        }
    }

    /// Whether the agent ran before failing.
    pub fn attempt(&self) -> Attempt {
        match self {
            Self::AgentNotFound { .. }
            | Self::AgentNotAuthenticated { .. }
            | Self::AgentRateLimited { .. } => Attempt::NotMade,
            Self::AgentError { .. } => Attempt::Made,
        }
    }

    /// The stable code the failure travels under.
    pub fn code(&self) -> &'static str {
        match self {
            Self::AgentNotFound { .. } => "agent_not_found",
            Self::AgentNotAuthenticated { .. } => "agent_not_authenticated",
            Self::AgentRateLimited { .. } => "agent_rate_limited",
            Self::AgentError { .. } => "agent_error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude() -> Slug {
        Slug::parse("claude").unwrap()
    }

    #[test]
    fn a_missing_agent_never_tried() {
        let failure = AgentFailure::AgentNotFound { agent: claude() };
        assert_eq!(failure.attempt(), Attempt::NotMade);
        assert_eq!(failure.code(), "agent_not_found");
    }

    #[test]
    fn an_agent_that_broke_did_try() {
        let failure = AgentFailure::AgentError {
            agent: claude(),
            summary: "the process exited with status 1".to_owned(),
        };
        assert_eq!(failure.attempt(), Attempt::Made);
    }
}

//! Machines, and the agents they turn out to have.
//!
//! A machine registers once against an instance and then serves whichever
//! groups its owner enables — not once per group. What it can do is
//! detected on the machine and reported, never declared from the web: the
//! owner installs an agent and the instance finds out, rather than being
//! told twice and believing the wrong half.
//!
//! An agent a machine does not have is named and shown unavailable rather
//! than hidden, because its absence teaches the owner something about
//! their own machine. That is the general rule: omit an option only when
//! its presence would imply something false.

use serde::{Deserialize, Serialize};

use crate::agent::AgentFailure;
use crate::fact::{Fact, FactKind};
use crate::id::Slug;

/// How often a machine that is awake says so.
pub const BEAT_SECONDS: u64 = 30;

/// How long a machine may be silent before it is taken to be asleep.
///
/// Three missed beats rather than one: a machine is not asleep because a
/// request was slow.
pub const ASLEEP_AFTER_SECONDS: u64 = 3 * BEAT_SECONDS;

/// Whether a machine is there right now.
///
/// A sleeping machine is an ordinary state and not a fault. It is grey and
/// it is the word "asleep", never amber and never a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    /// It has spoken recently enough to be taken for awake.
    Awake,
    /// It has not. Work chosen for it waits until it comes back.
    Asleep,
}

impl Fact for Presence {
    const KIND: FactKind = FactKind::Condition;
}

impl Presence {
    /// What a machine last heard from this long ago is taken to be.
    pub fn after_silence(seconds: u64) -> Self {
        match seconds < ASLEEP_AFTER_SECONDS {
            true => Self::Awake,
            false => Self::Asleep,
        }
    }
}

/// Whether one machine can run one agent, and why not when it cannot.
///
/// This is a state of a machine and never of the mathematics, so it is
/// drawn on the machine: a box three agents could not be reached for looks
/// exactly like a box nobody has attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// The machine has it, and nothing is known to be wrong with it.
    Ready,
    /// The machine does not have it. Its owner can change that.
    #[default]
    NotInstalled,
    /// It is installed, and its owner has not signed it in.
    NotSignedIn,
    /// It is installed and signed in, and the vendor is refusing for now.
    RateLimited,
}

spelled!(Presence { Awake => "awake", Asleep => "asleep" });

impl Fact for Availability {
    const KIND: FactKind = FactKind::Condition;
}

spelled!(Availability, BadAvailability("`{0}` does not say whether an agent can be used") {
    Ready => "ready",
    NotInstalled => "not_installed",
    NotSignedIn => "not_signed_in",
    RateLimited => "rate_limited",
});

impl Availability {
    /// Whether a job may be given to this agent on this machine.
    pub fn is_usable(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Why it cannot be chosen, in the words shown beside it.
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Ready => None,
            Self::NotInstalled => Some("not installed on this machine"),
            Self::NotSignedIn => Some("not signed in on this machine"),
            Self::RateLimited => Some("rate limited on this machine"),
        }
    }

    /// What a failure says about the machine, when it says anything.
    ///
    /// An agent that ran and broke says nothing about the machine: it is
    /// there, it works, and this attempt did not.
    pub fn learned_from(failure: &AgentFailure) -> Option<Self> {
        match failure {
            AgentFailure::AgentNotFound { .. } => Some(Self::NotInstalled),
            AgentFailure::AgentNotAuthenticated { .. } => Some(Self::NotSignedIn),
            AgentFailure::AgentRateLimited { .. } => Some(Self::RateLimited),
            AgentFailure::AgentError { .. } => None,
        }
    }
}

/// An agent Stemma knows how to invoke.
///
/// The list is here rather than on each machine so that both sides name
/// the same agents: a chooser can only show an agent as missing from a
/// machine if it knows the agent exists at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownAgent {
    /// What it is called everywhere in Stemma.
    pub slug: &'static str,
    /// Who supplies it.
    pub vendor: &'static str,
    /// What its makers call it.
    pub called: &'static str,
}

impl KnownAgent {
    /// Its name as an identifier.
    pub fn id(&self) -> Slug {
        Slug::parse(self.slug).expect("every agent in the catalogue is named by a slug")
    }
}

/// Every agent this build can invoke.
///
/// Stemma holds no key for any of them: they are authenticated by their
/// owner on their own machine, and Stemma only runs them and reports what
/// happened in a type the group can read.
pub const CATALOGUE: &[KnownAgent] = &[
    KnownAgent {
        slug: "claude",
        vendor: "Anthropic",
        called: "Claude Code",
    },
    KnownAgent {
        slug: "codex",
        vendor: "OpenAI",
        called: "Codex CLI",
    },
    KnownAgent {
        slug: "gemini",
        vendor: "Google",
        called: "Gemini CLI",
    },
];

/// The agent with this name, if this build knows one.
pub fn known(slug: &str) -> Option<&'static KnownAgent> {
    CATALOGUE.iter().find(|agent| agent.slug == slug)
}

/// What a machine found when it looked for one agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detected {
    /// Which agent.
    pub agent: Slug,
    /// Which version of it, where it could be asked.
    pub version: Option<String>,
    /// Whether it can be used, and why not when it cannot.
    pub availability: Availability,
}

impl Detected {
    /// An agent that is there and works.
    pub fn ready(agent: Slug, version: Option<String>) -> Self {
        Self {
            agent,
            version,
            availability: Availability::Ready,
        }
    }

    /// An agent that is not on the machine at all.
    pub fn missing(agent: Slug) -> Self {
        Self {
            agent,
            version: None,
            availability: Availability::NotInstalled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_in_the_catalogue_is_properly_named() {
        for agent in CATALOGUE {
            assert_eq!(agent.id().as_str(), agent.slug);
            assert!(!agent.vendor.is_empty());
            assert!(!agent.called.is_empty());
        }

        assert!(known("claude").is_some());
        assert!(known("nothing-like-this").is_none());
    }

    #[test]
    fn a_machine_is_asleep_once_it_has_missed_several_beats() {
        assert_eq!(Presence::after_silence(0), Presence::Awake);
        assert_eq!(Presence::after_silence(BEAT_SECONDS + 1), Presence::Awake);
        assert_eq!(
            Presence::after_silence(ASLEEP_AFTER_SECONDS),
            Presence::Asleep
        );
    }

    #[test]
    fn an_agent_that_cannot_be_used_says_why() {
        assert!(Availability::Ready.is_usable());
        assert_eq!(Availability::Ready.reason(), None);

        for unusable in [
            Availability::NotInstalled,
            Availability::NotSignedIn,
            Availability::RateLimited,
        ] {
            assert!(!unusable.is_usable());
            assert!(unusable.reason().is_some(), "{unusable} gave no reason");
            assert_eq!(unusable.to_string().parse(), Ok(unusable));
        }
    }

    #[test]
    fn a_broken_run_says_nothing_about_the_machine() {
        let claude = Slug::parse("claude").unwrap();

        assert_eq!(
            Availability::learned_from(&AgentFailure::AgentNotFound {
                agent: claude.clone()
            }),
            Some(Availability::NotInstalled)
        );
        assert_eq!(
            Availability::learned_from(&AgentFailure::AgentNotAuthenticated {
                agent: claude.clone()
            }),
            Some(Availability::NotSignedIn)
        );
        assert_eq!(
            Availability::learned_from(&AgentFailure::AgentError {
                agent: claude,
                summary: "it exited with status 1".to_owned(),
            }),
            None
        );
    }

    #[test]
    fn what_a_machine_can_do_is_a_condition_and_not_a_task() {
        assert!(!Availability::KIND.is_counted());
        assert!(!Presence::KIND.is_counted());
    }
}

//! The vocabulary of the mathematical model.
//!
//! These are the states a piece of mathematics can be in. They are conditions
//! in the sense of [`crate::fact`]: each says what is true of something right
//! now, none of them is a task, and none of them counts as outstanding work.

use serde::{Deserialize, Serialize};

use crate::fact::{Fact, FactKind};

/// Whether a statement is one of the line's load-bearing results.
///
/// Centrality is declared in the source, as an argument to the mathematical
/// environment, so it is versioned, diffable, readable offline, and something
/// an agent can declare in what it proposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Centrality {
    /// An ordinary statement. It lives inside a black box and an agent may
    /// rewrite it.
    #[default]
    Ordinary,
    /// A statement the line rests on. Its text is frozen against agents, and
    /// it moves by exactly two routes: an agent proves its negation, or an
    /// agent recommends a change and the author accepts.
    Central,
}

impl Centrality {
    /// Whether the statement is central.
    pub fn is_central(self) -> bool {
        matches!(self, Self::Central)
    }
}

/// How much of a black box is filled in.
///
/// A box three agents failed at looks exactly like a box nobody has
/// attempted, because in both cases nothing is known. What tells them apart
/// is the record of attempts kept beside the box, never the box itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoxState {
    /// Nothing inside. Its two boundary statements still hold it in place.
    #[default]
    Empty,
    /// An argument a person wrote.
    FilledByHand,
    /// An argument an agent proposed and a person accepted.
    FilledByAgent,
    /// An argument Lean compiles.
    Verified,
}

impl Fact for BoxState {
    const KIND: FactKind = FactKind::Condition;
}

/// What a proof checker says about a statement.
///
/// `Unverified` is the ordinary state of most mathematics and asks for
/// nothing. An open negation goal is not a pending check and no interface may
/// present it as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// No formal proof has been compiled. This is a fact, not a task.
    #[default]
    Unverified,
    /// A formal proof compiles.
    Verified,
    /// A formal proof of the negation compiles.
    Refuted,
}

impl Fact for Verification {
    const KIND: FactKind = FactKind::Condition;
}

spelled!(Verification {
    Unverified => "unverified",
    Verified => "verified",
    Refuted => "refuted",
});

/// Whether anybody vouches that the formal statement says what the prose
/// says.
///
/// Verification and correspondence are independent. Lean compiling is cheap
/// and mechanical; reading a formalization back into prose is expensive and
/// human, which is why it is reserved for central statements and for the
/// definitions the group builds on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Correspondence {
    /// Nobody has vouched. Nothing is owed, and nothing was lost.
    #[default]
    NotAudited,
    /// Somebody read the formalization against the prose and signed.
    Audited,
    /// Somebody had vouched and no longer does.
    ///
    /// Deliberately not the same state as [`Self::NotAudited`]: the proof
    /// still compiles, but the guarantee that it says what it claims is gone,
    /// and the people whose work leans on it owe a decision.
    AuditWithdrawn,
}

impl Fact for Correspondence {
    const KIND: FactKind = FactKind::Condition;
}

impl Correspondence {
    /// Whether the statement is vouched for right now.
    pub fn is_vouched(self) -> bool {
        matches!(self, Self::Audited)
    }

    /// Whether a guarantee was lost, as opposed to never given.
    pub fn was_lost(self) -> bool {
        matches!(self, Self::AuditWithdrawn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unaudited_and_withdrawn_are_different_states() {
        assert_ne!(Correspondence::NotAudited, Correspondence::AuditWithdrawn);
        assert!(!Correspondence::NotAudited.was_lost());
        assert!(Correspondence::AuditWithdrawn.was_lost());
    }

    #[test]
    fn conditions_are_never_counted_as_work() {
        assert!(!Verification::KIND.is_counted());
        assert!(!Correspondence::KIND.is_counted());
        assert!(!BoxState::KIND.is_counted());
    }

    #[test]
    fn a_statement_is_ordinary_until_it_is_declared_central() {
        assert_eq!(Centrality::default(), Centrality::Ordinary);
        assert!(!Centrality::default().is_central());
    }
}

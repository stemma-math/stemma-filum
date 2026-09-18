//! Records, conditions and obligations.
//!
//! Three different kinds of fact, and no mark carries more than one:
//!
//! - A **record** says what happened and who did it. It is permanent, and it
//!   is never a claim about the present.
//! - A **condition** says what is true of something right now. It lasts
//!   exactly as long as it is true, and it is never a task.
//! - An **obligation** says what a person owes. It is counted, it names that
//!   person, and it clears when they act.
//!
//! Collapsing two of them produces the failures this design is built to
//! avoid: a condition drawn as an obligation becomes a pending badge nobody
//! can ever clear, and a record drawn as a condition makes the system assert
//! now what was only true then.
//!
//! Every type that describes a state implements [`Fact`], so its kind is
//! declared where the type is defined rather than decided by whatever screen
//! happens to draw it.

use serde::{Deserialize, Serialize};

/// Which of the three kinds a fact belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactKind {
    /// What happened, and who did it.
    Record,
    /// What is true right now.
    Condition,
    /// What a person owes.
    Obligation,
}

impl FactKind {
    /// Whether facts of this kind are counted as outstanding work.
    ///
    /// Only obligations are. A condition that will not change on its own is
    /// not a task, however unwelcome it is.
    pub fn is_counted(self) -> bool {
        matches!(self, Self::Obligation)
    }
}

/// A type that describes a state, and says which kind of fact it is.
pub trait Fact {
    /// The kind every value of this type belongs to.
    const KIND: FactKind;
}

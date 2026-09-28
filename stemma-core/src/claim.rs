//! Who is writing a line, and from where.
//!
//! Only people's machines write mathematics, so two versions of a line are
//! never two people: they are one person on two devices. A claim is
//! therefore held by a **machine** and not by an account, which is what
//! reduces every remaining disagreement to somebody against their own
//! laptop — and to a person who owns both sides, any rule at all is
//! acceptable.
//!
//! A claim expires on its own. Somebody who shuts a laptop and goes home
//! has not taken the line with them, and the machine that holds it says so
//! while it is writing rather than being trusted to let go.
//!
//! Being asked for a line back is a condition of the claim and not an
//! obligation: the only person waiting is the one holding it, so it is
//! shown where the claim is shown and counted nowhere.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::fact::{Fact, FactKind};
use crate::id::MachineId;

/// How long a claim outlives the last word written under it.
pub const CLAIM_SECONDS: u64 = 30 * 60;

/// How often the machine that is writing says it still is.
///
/// Six times within the life of a claim, so a laptop that misses a few
/// renewals over a bad connection does not lose the line it is writing.
pub const RENEW_SECONDS: u64 = CLAIM_SECONDS / 6;

/// Whether a claim is still standing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    /// The machine holding it has said so recently enough.
    Held,
    /// It has not, and the line is free again.
    Lapsed,
}

impl Fact for Hold {
    const KIND: FactKind = FactKind::Condition;
}

impl Hold {
    /// What a claim last renewed this long ago has become.
    pub fn after_silence(seconds: u64) -> Self {
        match seconds < CLAIM_SECONDS {
            true => Self::Held,
            false => Self::Lapsed,
        }
    }

    /// Whether the line is still somebody's.
    pub fn stands(self) -> bool {
        matches!(self, Self::Held)
    }
}

/// What one machine may do with one line right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "standing", rename_all = "snake_case")]
pub enum Standing {
    /// Nobody is writing it, so taking it is a formality.
    Open,
    /// This machine holds it and may write.
    Writing,
    /// Another machine holds it. Reading is fine; writing waits, and the
    /// line can be asked for.
    Waiting {
        /// The machine that has it.
        held_by: MachineId,
        /// Whether it has already been asked to hand it back.
        asked: bool,
    },
}

impl Fact for Standing {
    const KIND: FactKind = FactKind::Condition;
}

impl Standing {
    /// What a machine that is looking at a claim stands in relation to it.
    pub fn of(machine: &MachineId, claim: Option<&Held>) -> Self {
        match claim {
            None => Self::Open,
            Some(held) if &held.machine == machine => Self::Writing,
            Some(held) => Self::Waiting {
                held_by: held.machine,
                asked: held.asked,
            },
        }
    }

    /// Whether this machine may write the line.
    pub fn may_write(&self) -> bool {
        matches!(self, Self::Open | Self::Writing)
    }

    /// Whether asking for the line would say anything new.
    pub fn may_ask(&self) -> bool {
        matches!(self, Self::Waiting { asked: false, .. })
    }

    /// What it says on screen.
    pub fn said(&self) -> String {
        match self {
            Self::Open => "nobody is writing this line".to_owned(),
            Self::Writing => "this machine is writing it".to_owned(),
            Self::Waiting {
                held_by,
                asked: false,
            } => {
                format!("{held_by} is writing it")
            }
            Self::Waiting {
                held_by,
                asked: true,
            } => {
                format!("{held_by} is writing it, and has been asked for it")
            }
        }
    }
}

impl fmt::Display for Standing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.said())
    }
}

/// A claim that is standing, as anybody looking at the line sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    /// The machine that is writing.
    pub machine: MachineId,
    /// What that machine calls itself.
    pub label: String,
    /// How many seconds are left before it lapses on its own.
    pub left: u64,
    /// Whether the holder has been asked to hand the line back.
    pub asked: bool,
}

impl Held {
    /// How long is left, in the words a person reads.
    pub fn lapses_in(&self) -> String {
        match self.left {
            0 => "it lapses now".to_owned(),
            1 => "it lapses in 1 second".to_owned(),
            2..=90 => format!("it lapses in {} seconds", self.left),
            left => format!("it lapses in {} minutes", left.div_ceil(60)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(id: &str) -> MachineId {
        format!("mch:{id}").parse().unwrap()
    }

    fn held(by: &MachineId, asked: bool) -> Held {
        Held {
            machine: *by,
            label: "a laptop".to_owned(),
            left: 600,
            asked,
        }
    }

    #[test]
    fn a_claim_nobody_renews_lapses_on_its_own() {
        assert_eq!(Hold::after_silence(0), Hold::Held);
        assert_eq!(Hold::after_silence(RENEW_SECONDS), Hold::Held);
        assert_eq!(Hold::after_silence(CLAIM_SECONDS - 1), Hold::Held);
        assert_eq!(Hold::after_silence(CLAIM_SECONDS), Hold::Lapsed);
        assert!(!Hold::Lapsed.stands());
    }

    #[test]
    fn a_line_nobody_holds_is_open_to_the_machine_looking_at_it() {
        let standing = Standing::of(&machine("01K5YQ4TQ2VZ8H3M9N0P1R2S3T"), None);

        assert_eq!(standing, Standing::Open);
        assert!(standing.may_write());
        assert!(!standing.may_ask());
    }

    #[test]
    fn the_machine_holding_a_line_is_the_one_writing_it() {
        let mine = machine("01K5YQ4TQ2VZ8H3M9N0P1R2S3T");
        let standing = Standing::of(&mine, Some(&held(&mine, false)));

        assert_eq!(standing, Standing::Writing);
        assert!(standing.may_write());
        assert!(standing.said().contains("this machine"));
    }

    #[test]
    fn a_line_another_machine_holds_can_be_read_and_asked_for_once() {
        let mine = machine("01K5YQ4TQ2VZ8H3M9N0P1R2S3T");
        let theirs = machine("01K5YQ4TQ2VZ8H3M9N0P1R2S3V");

        let waiting = Standing::of(&mine, Some(&held(&theirs, false)));
        assert!(!waiting.may_write());
        assert!(waiting.may_ask());

        let asked = Standing::of(&mine, Some(&held(&theirs, true)));
        assert!(!asked.may_ask(), "asking twice says nothing new");
        assert!(asked.said().contains("asked"));
    }

    #[test]
    fn a_claim_says_how_long_it_has_left_in_words() {
        let left = |left| Held {
            machine: machine("01K5YQ4TQ2VZ8H3M9N0P1R2S3T"),
            label: "a laptop".to_owned(),
            left,
            asked: false,
        };

        assert_eq!(left(0).lapses_in(), "it lapses now");
        assert_eq!(left(1).lapses_in(), "it lapses in 1 second");
        assert_eq!(left(90).lapses_in(), "it lapses in 90 seconds");
        assert_eq!(left(91).lapses_in(), "it lapses in 2 minutes");
        assert_eq!(left(45).lapses_in(), "it lapses in 45 seconds");
        assert_eq!(left(600).lapses_in(), "it lapses in 10 minutes");
        assert_eq!(left(601).lapses_in(), "it lapses in 11 minutes");
    }

    #[test]
    fn being_asked_for_a_line_is_a_condition_and_never_a_count() {
        assert_eq!(Standing::KIND, FactKind::Condition);
        assert!(!Standing::KIND.is_counted());
        assert_eq!(Hold::KIND, FactKind::Condition);
    }
}

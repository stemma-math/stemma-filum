//! What changed on the instance, as it tells the machines watching it.
//!
//! An event is a notice and not the news: it says what changed and where,
//! and whoever hears it asks for the thing itself. So it carries names and
//! identifiers, never content, and a client that missed some can always
//! start over by asking for everything.

use serde::{Deserialize, Serialize};

use crate::id::BlockId;
use crate::model::Verification;
use crate::repository::Side;

/// The header a client names the last event it saw in, when it reconnects.
pub const LAST_EVENT_HEADER: &str = "last-event-id";

/// One thing that changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// A line was started, written, shared or deleted.
    Line {
        /// The group's short name.
        group: String,
        /// Which of the group's repositories it is in now.
        side: Side,
        /// The line's name.
        line: String,
        /// What happened to it.
        change: LineChange,
        /// The revision it is at now, where it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<u64>,
    },
    /// A line was taken, let go, or asked for back.
    Claim {
        /// The group's short name.
        group: String,
        /// Which of the group's repositories the line is in.
        side: Side,
        /// The line's name.
        line: String,
        /// What happened to the claim.
        change: ClaimChange,
    },
    /// A group was made, or who is in it changed.
    Group {
        /// The group's short name.
        group: String,
        /// What happened.
        change: GroupChange,
    },
    /// A job was queued, taken, finished or dealt with.
    Job {
        /// The group's short name.
        group: String,
        /// The job's identifier.
        job: String,
        /// The state it is in now.
        state: String,
    },
    /// A proposal arrived or was decided.
    Proposal {
        /// The group's short name.
        group: String,
        /// The proposal's identifier.
        proposal: String,
        /// The state it is in now.
        state: String,
    },
    /// A machine was connected, changed what it offers, or was retired.
    Machine {
        /// The machine's identifier.
        machine: String,
    },
    /// A group's Lake package changed on the instance.
    Library {
        /// The group's short name.
        group: String,
        /// Which package changed.
        side: Side,
        /// The commit it is at.
        commit: String,
        /// What kind of change caused it.
        change: LibraryChange,
    },
    /// A machine published the formal result for one block revision.
    Check {
        /// The group's short name.
        group: String,
        /// Which repository contains the line.
        side: Side,
        /// The line's name.
        line: String,
        /// The line revision this result checks.
        revision: u64,
        /// The stable block checked.
        block: BlockId,
        /// What Lean established.
        state: Verification,
        /// Why neither verification nor refutation was established.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        diagnostic: String,
    },
    /// A central statement's prose/formal correspondence audit changed.
    FormalAudit {
        /// The group's short name.
        group: String,
        /// The line containing the central statement.
        line: String,
        /// The stable block whose dependents should reread their backing.
        block: BlockId,
        /// `audited` or `audit_withdrawn`.
        state: String,
    },
    /// The reader was away too long for the events it missed to be kept:
    /// everything should be asked for again.
    Reset,
}

/// What happened to a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineChange {
    /// It exists now.
    Started,
    /// Somebody wrote it.
    Written,
    /// It moved to the group.
    Shared,
    /// It was taken out.
    Deleted,
}

/// What happened to the claim on a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimChange {
    /// A machine took it.
    Taken,
    /// It was let go.
    Released,
    /// Somebody asked for it back.
    Asked,
}

/// What happened to a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupChange {
    /// It was made.
    Created,
    /// Somebody joined it.
    Joined,
}

/// What changed in a Lake package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryChange {
    /// A person approved an agent's file changes.
    Written,
    /// An admin changed package or toolchain pins.
    PinsChanged,
    /// A private line's formal dependencies moved into the shared package.
    Shared,
}

impl Event {
    /// The word the event is filed under.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Line { .. } => "line",
            Self::Claim { .. } => "claim",
            Self::Group { .. } => "group",
            Self::Job { .. } => "job",
            Self::Proposal { .. } => "proposal",
            Self::Machine { .. } => "machine",
            Self::Library { .. } => "library",
            Self::Check { .. } => "check",
            Self::FormalAudit { .. } => "formal_audit",
            Self::Reset => "reset",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_says_its_kind_beside_what_it_is_about() {
        let written = Event::Line {
            group: "algebra".to_owned(),
            side: Side::Shared,
            line: "quotients".to_owned(),
            change: LineChange::Written,
            revision: Some(4),
        };

        let json = serde_json::to_value(&written).unwrap();

        assert_eq!(json["kind"], "line");
        assert_eq!(json["change"], "written");
        assert_eq!(json["side"], "shared");
        assert_eq!(written.kind(), "line");
        assert_eq!(serde_json::from_value::<Event>(json).unwrap(), written);
    }

    #[test]
    fn a_reset_carries_nothing_but_itself() {
        assert_eq!(
            serde_json::to_string(&Event::Reset).unwrap(),
            r#"{"kind":"reset"}"#
        );
    }
}

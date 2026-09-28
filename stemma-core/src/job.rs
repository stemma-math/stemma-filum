//! Work handed to a machine.
//!
//! A job says what is being asked, where the answer will land, which
//! machine is to do it and with which agent. Machine and agent are a
//! deliberate choice, so a job whose machine is asleep waits for that
//! machine: reassigning it would be overruling the person who chose. Who
//! does not want to choose says so, in as many words, and then anything
//! that is free may take it.
//!
//! How a job ends is typed, and the type carries the one distinction that
//! matters: whether the agent ever got as far as trying. "Nothing was
//! found" is an ordinary outcome of research and belongs to the
//! mathematics; "the agent is not installed here" belongs to the machine
//! and is drawn there, never on the mathematics.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::agent::{AgentFailure, Attempt};
use crate::fact::{Fact, FactKind};
use crate::id::{BlockId, IdError, MachineId};

/// What a job asks an agent to do, and where its answer belongs.
///
/// The two are one thing because an action is offered where its effect
/// will land: filling is asked of a box, refuting of a statement the line
/// rests on, and exploring of the line's open end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "task", rename_all = "snake_case")]
pub enum Task {
    /// Fill the black box that follows a statement — or the line's first
    /// box, which follows none.
    Fill {
        /// The central block the box comes after.
        after: Option<BlockId>,
    },
    /// Look for a counterexample to a statement.
    Refute {
        /// The statement block to attack.
        block: BlockId,
    },
    /// Formalize one statement in Lean.
    Formalize {
        /// The statement block whose prose is to receive a formal counterpart.
        block: BlockId,
    },
    /// Try the author's narrative route after a proposed formal route was declined.
    FollowRoute {
        /// The central statement whose formalization must follow the prose route.
        block: BlockId,
    },
    /// Verify the Lean argument for one black box.
    Verify {
        /// The central block after which the box begins, if any.
        after: Option<BlockId>,
    },
    /// Promote an interior statement so it becomes a frozen box boundary.
    Promote {
        /// The lemma or proposition block to move out of its box.
        block: BlockId,
    },
    /// Write, or finish, the proof of a statement.
    Prove {
        /// The statement block whose proof is to be written.
        block: BlockId,
    },
    /// Rewrite a statement that is not central so that it is correct and
    /// precise, keeping what it means.
    Rewrite {
        /// The statement block to rewrite.
        block: BlockId,
    },
    /// Turn a prose block into the mathematics it describes: definitions,
    /// statements and proofs that bridge what comes before and after it.
    Develop {
        /// The prose block.
        block: BlockId,
    },
    /// Say where the line could go next.
    Explore,
    /// Bring two versions of this line together.
    ///
    /// What comes back is a proposal like any other: an agent may offer
    /// a reconciliation, and a person signs it or does not.
    Reconcile,
    /// Build the line's repository and publish what it holds, for the
    /// web to show. The instance asks for this itself, of any machine of
    /// the group, when the machine that wrote the line did not; no agent
    /// takes part.
    Build,
}

/// What a build job names in place of an agent.
pub const BUILDER: &str = "stemma";

impl Task {
    /// What is being asked, in the words the launcher offers it in.
    pub fn asked_for(&self) -> &'static str {
        match self {
            Self::Fill { .. } => "fill this box",
            Self::Refute { .. } => "look for a counterexample",
            Self::Formalize { .. } => "formalize in Lean",
            Self::FollowRoute { .. } => "follow the author's route in Lean",
            Self::Verify { .. } => "verify this box in Lean",
            Self::Promote { .. } => "promote this statement",
            Self::Prove { .. } => "write the proof",
            Self::Rewrite { .. } => "rewrite this statement rigorously",
            Self::Develop { .. } => "develop this prose into mathematics",
            Self::Explore => "say where the line could go next",
            Self::Reconcile => "bring the two versions together",
            Self::Build => "build its pages",
        }
    }

    /// The statement the work hangs off, when it hangs off one.
    pub fn anchor(&self) -> Option<&BlockId> {
        match self {
            Self::Fill { after } => after.as_ref(),
            Self::Refute { block } => Some(block),
            Self::Formalize { block } => Some(block),
            Self::FollowRoute { block } => Some(block),
            Self::Verify { after } => after.as_ref(),
            Self::Promote { block } => Some(block),
            Self::Prove { block } => Some(block),
            Self::Rewrite { block } => Some(block),
            Self::Develop { .. } | Self::Explore | Self::Reconcile | Self::Build => None,
        }
    }

    /// Whether this asks an agent to touch what the line rests on.
    ///
    /// A central statement is frozen against agents, so refuting one is
    /// the only thing an agent may aim at it.
    pub fn is_aimed_at_a_central_statement(&self) -> bool {
        matches!(
            self,
            Self::Refute { .. } | Self::Formalize { .. } | Self::FollowRoute { .. }
        )
    }
}

impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fill { after: None } => f.write_str("fill"),
            Self::Fill { after: Some(node) } => write!(f, "fill@{node}"),
            Self::Refute { block } => write!(f, "refute@{block}"),
            Self::Formalize { block } => write!(f, "formalize@{block}"),
            Self::FollowRoute { block } => write!(f, "follow@{block}"),
            Self::Verify { after: None } => f.write_str("verify"),
            Self::Verify { after: Some(node) } => write!(f, "verify@{node}"),
            Self::Promote { block } => write!(f, "promote@{block}"),
            Self::Prove { block } => write!(f, "prove@{block}"),
            Self::Rewrite { block } => write!(f, "rewrite@{block}"),
            Self::Develop { block } => write!(f, "develop@{block}"),
            Self::Explore => f.write_str("explore"),
            Self::Reconcile => f.write_str("reconcile"),
            Self::Build => f.write_str("build"),
        }
    }
}

/// Why a string does not name a task.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BadTask {
    /// The verb is not one of ours.
    #[error("`{0}` is not something a job can ask for")]
    Unknown(String),
    /// The verb needs a statement to hang off and has none.
    #[error("`{0}` has to name the statement it is about")]
    Unanchored(String),
    /// What it hangs off is not a block.
    #[error(transparent)]
    Block(#[from] IdError),
}

impl FromStr for Task {
    type Err = BadTask;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        // A block identifier has a colon of its own, so only the first `@`
        // separates the verb from what it hangs off.
        let (verb, anchor) = match input.split_once('@') {
            Some((verb, anchor)) => (verb, Some(anchor)),
            None => (input, None),
        };

        match (verb, anchor) {
            ("fill", after) => Ok(Self::Fill {
                after: after.map(str::parse).transpose()?,
            }),
            ("refute", Some(block)) => Ok(Self::Refute {
                block: block.parse()?,
            }),
            ("refute", None) => Err(BadTask::Unanchored(input.to_owned())),
            ("formalize", Some(block)) => Ok(Self::Formalize {
                block: block.parse()?,
            }),
            ("formalize", None) => Err(BadTask::Unanchored(input.to_owned())),
            ("follow", Some(block)) => Ok(Self::FollowRoute {
                block: block.parse()?,
            }),
            ("follow", None) => Err(BadTask::Unanchored(input.to_owned())),
            ("verify", after) => Ok(Self::Verify {
                after: after.map(str::parse).transpose()?,
            }),
            ("promote", Some(block)) => Ok(Self::Promote {
                block: block.parse()?,
            }),
            ("promote", None) => Err(BadTask::Unanchored(input.to_owned())),
            ("prove", Some(block)) => Ok(Self::Prove {
                block: block.parse()?,
            }),
            ("rewrite", Some(block)) => Ok(Self::Rewrite {
                block: block.parse()?,
            }),
            ("develop", Some(block)) => Ok(Self::Develop {
                block: block.parse()?,
            }),
            ("prove" | "rewrite" | "develop", None) => Err(BadTask::Unanchored(input.to_owned())),
            ("explore", _) => Ok(Self::Explore),
            ("reconcile", _) => Ok(Self::Reconcile),
            ("build", _) => Ok(Self::Build),
            _ => Err(BadTask::Unknown(verb.to_owned())),
        }
    }
}

/// Which machine is to do the work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "choice", rename_all = "snake_case")]
pub enum Choice {
    /// This machine and no other. A sleeping one is a valid answer: the
    /// job waits for it rather than being handed elsewhere.
    Machine {
        /// The machine that was chosen.
        machine: MachineId,
    },
    /// Whoever is free, for somebody who does not want to choose.
    Anyone,
}

impl Choice {
    /// The machine that was chosen, when one was.
    pub fn machine(&self) -> Option<&MachineId> {
        match self {
            Self::Machine { machine } => Some(machine),
            Self::Anyone => None,
        }
    }

    /// Whether this machine may take a job that was chosen this way.
    pub fn admits(&self, machine: &MachineId) -> bool {
        match self {
            Self::Machine { machine: chosen } => chosen == machine,
            Self::Anyone => true,
        }
    }

    /// Whether somebody picked a machine on purpose.
    pub fn is_deliberate(&self) -> bool {
        matches!(self, Self::Machine { .. })
    }
}

/// Where a job has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Nobody has taken it. A job that named a sleeping machine sits here
    /// until that machine comes back, however long that is.
    Queued,
    /// A machine has it.
    Running,
    /// It ended, and its outcome says how.
    Done,
    /// Whoever launched it took it back before anything ran.
    Withdrawn,
}

impl Fact for JobState {
    const KIND: FactKind = FactKind::Condition;
}

impl JobState {
    /// Whether nothing more will happen to it.
    pub fn is_settled(self) -> bool {
        matches!(self, Self::Done | Self::Withdrawn)
    }

    /// Whether a machine asking for work may be given this.
    pub fn may_be_taken(self) -> bool {
        matches!(self, Self::Queued)
    }

    /// Whether it can still be called back.
    ///
    /// Only before it runs: taking a job back from a machine that is part
    /// way through it would be a promise this cannot keep.
    pub fn may_be_withdrawn(self) -> bool {
        matches!(self, Self::Queued)
    }
}

spelled!(JobState, BadJobState("`{0}` is not a state a job can be in") {
    Queued => "queued",
    Running => "running",
    Done => "done",
    Withdrawn => "withdrawn",
});

/// How a job ended.
///
/// A record: it says what happened once, and it is never a claim about
/// now. Raw output is not in here and never travels — it carries the
/// owner's account, quota and plan, so it stays on their machine and the
/// group is given the type and one line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// The agent ran and produced something for a person to look at.
    Produced {
        /// One line about it, safe to show the group.
        said: String,
    },
    /// The agent ran, and the mathematics did not work.
    ///
    /// An ordinary outcome of research. The box goes back to its hatch
    /// and the attempt is recorded beside it; nothing is coloured and
    /// nothing is owed.
    NothingFound {
        /// One line about it, safe to show the group.
        said: String,
    },
    /// The agent never ran.
    ///
    /// A state of the machine rather than of the mathematics, so it is
    /// shown on the machine, where its owner can act on it.
    CouldNotTry {
        /// The stable code of the failure.
        code: String,
        /// One line about it, safe to show the group.
        said: String,
    },
    /// The agent ran and broke.
    Broke {
        /// The stable code of the failure.
        code: String,
        /// One line about it, safe to show the group.
        said: String,
    },
}

impl Fact for Outcome {
    const KIND: FactKind = FactKind::Record;
}

impl Outcome {
    /// What a typed agent failure came to.
    pub fn from_failure(failure: &AgentFailure) -> Self {
        let code = failure.code().to_owned();
        let said = failure.to_string();

        match failure.attempt() {
            Attempt::NotMade => Self::CouldNotTry { code, said },
            Attempt::Made => Self::Broke { code, said },
        }
    }

    /// A machine that could not get to the work at all.
    pub fn out_of_reach(said: impl Into<String>) -> Self {
        Self::CouldNotTry {
            code: "work_out_of_reach".to_owned(),
            said: said.into(),
        }
    }

    /// Whether the agent got as far as running.
    pub fn attempt(&self) -> Attempt {
        match self {
            Self::Produced { .. } | Self::NothingFound { .. } | Self::Broke { .. } => Attempt::Made,
            Self::CouldNotTry { .. } => Attempt::NotMade,
        }
    }

    /// Whether this says something about the machine rather than about
    /// the mathematics.
    ///
    /// Drawn on the machine and never on the node: a box three agents
    /// could not be reached for looks exactly like a box nobody has
    /// attempted.
    pub fn is_the_machines(&self) -> bool {
        self.attempt() == Attempt::NotMade
    }

    /// Whether what stopped it is the machine owner's to fix.
    ///
    /// An agent that is missing, signed out or being refused is theirs:
    /// they own the machine and they own the account. Work the machine
    /// could not reach is also a could-not-try, and it is not theirs —
    /// nothing they install brings down a commit somebody has not
    /// pushed — so it is reported and never counted against them.
    pub fn is_the_owners_to_fix(&self) -> bool {
        matches!(self, Self::CouldNotTry { code, .. } if code.starts_with("agent_"))
    }

    /// Whether anything was learned about the mathematics.
    pub fn taught_anything(&self) -> bool {
        matches!(self, Self::Produced { .. } | Self::NothingFound { .. })
    }

    /// The one line the group is shown.
    pub fn said(&self) -> &str {
        match self {
            Self::Produced { said }
            | Self::NothingFound { said }
            | Self::CouldNotTry { said, .. }
            | Self::Broke { said, .. } => said,
        }
    }

    /// The stable code, where the outcome has one.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Produced { .. } | Self::NothingFound { .. } => None,
            Self::CouldNotTry { code, .. } | Self::Broke { code, .. } => Some(code),
        }
    }

    /// The word it is stored as.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Produced { .. } => "produced",
            Self::NothingFound { .. } => "nothing_found",
            Self::CouldNotTry { .. } => "could_not_try",
            Self::Broke { .. } => "broke",
        }
    }

    /// Puts one back together from the columns it was stored in.
    pub fn rebuild(word: &str, code: Option<String>, said: String) -> Option<Self> {
        Some(match word {
            "produced" => Self::Produced { said },
            "nothing_found" => Self::NothingFound { said },
            "could_not_try" => Self::CouldNotTry {
                code: code.unwrap_or_default(),
                said,
            },
            "broke" => Self::Broke {
                code: code.unwrap_or_default(),
                said,
            },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::Slug;

    fn block() -> BlockId {
        "blk:01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap()
    }

    #[test]
    fn a_task_survives_being_written_down() {
        for task in [
            Task::Fill { after: None },
            Task::Fill {
                after: Some(block()),
            },
            Task::Refute { block: block() },
            Task::Formalize { block: block() },
            Task::FollowRoute { block: block() },
            Task::Verify {
                after: Some(block()),
            },
            Task::Verify { after: None },
            Task::Promote { block: block() },
            Task::Prove { block: block() },
            Task::Rewrite { block: block() },
            Task::Develop {
                block: BlockId::generate(),
            },
            Task::Explore,
        ] {
            assert_eq!(task.to_string().parse(), Ok(task.clone()), "for {task}");
        }

        assert_eq!(Task::Fill { after: None }.to_string(), "fill");
        assert_eq!(
            Task::Refute { block: block() }.to_string(),
            "refute@blk:01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
        assert_eq!(
            Task::Formalize { block: block() }.to_string(),
            "formalize@blk:01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
        assert_eq!(
            Task::FollowRoute { block: block() }.to_string(),
            "follow@blk:01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    #[test]
    fn bringing_two_versions_together_is_about_the_line_and_not_one_statement() {
        let task: Task = "reconcile".parse().unwrap();

        assert_eq!(task, Task::Reconcile);
        assert_eq!(task.to_string(), "reconcile");
        assert_eq!(task.anchor(), None);
        assert!(!task.is_aimed_at_a_central_statement());
    }

    #[test]
    fn a_task_that_needs_a_statement_is_refused_without_one() {
        assert_eq!(
            "refute".parse::<Task>(),
            Err(BadTask::Unanchored("refute".to_owned()))
        );
        assert_eq!(
            "prove".parse::<Task>(),
            Err(BadTask::Unanchored("prove".to_owned()))
        );
        assert_eq!(
            "guess".parse::<Task>(),
            Err(BadTask::Unknown("guess".to_owned()))
        );
        assert!(
            "develop@thm:main".parse::<Task>().is_err(),
            "a block, not a siglum"
        );
        assert!("fill@nothing".parse::<Task>().is_err());
    }

    #[test]
    fn a_chosen_machine_is_the_only_one_that_may_take_the_job() {
        let chosen = MachineId::generate();
        let other = MachineId::generate();
        let choice = Choice::Machine { machine: chosen };

        assert!(choice.admits(&chosen));
        assert!(!choice.admits(&other), "a deliberate choice was overruled");
        assert!(choice.is_deliberate());

        assert!(Choice::Anyone.admits(&other));
        assert!(!Choice::Anyone.is_deliberate());
    }

    #[test]
    fn only_a_job_nobody_has_started_can_be_called_back() {
        assert!(JobState::Queued.may_be_withdrawn());
        assert!(JobState::Queued.may_be_taken());

        for settled in [JobState::Running, JobState::Done, JobState::Withdrawn] {
            assert!(!settled.may_be_withdrawn());
            assert!(!settled.may_be_taken());
        }

        assert!(!JobState::Running.is_settled());
        assert!(JobState::Done.is_settled());
    }

    #[test]
    fn a_state_survives_being_written_down() {
        for state in [
            JobState::Queued,
            JobState::Running,
            JobState::Done,
            JobState::Withdrawn,
        ] {
            assert_eq!(state.to_string().parse(), Ok(state));
        }
    }

    #[test]
    fn a_missing_agent_is_the_machines_business_and_a_failed_proof_is_not() {
        let claude = Slug::parse("claude").unwrap();

        let missing = Outcome::from_failure(&AgentFailure::AgentNotFound {
            agent: claude.clone(),
        });
        assert!(missing.is_the_machines());
        assert!(missing.is_the_owners_to_fix());
        assert!(!missing.taught_anything());
        assert_eq!(missing.code(), Some("agent_not_found"));

        let broke = Outcome::from_failure(&AgentFailure::AgentError {
            agent: claude,
            summary: "it exited with status 1".to_owned(),
        });
        assert!(!broke.is_the_machines());
        assert!(!broke.taught_anything());

        let nothing = Outcome::NothingFound {
            said: "no argument was found".to_owned(),
        };
        assert!(!nothing.is_the_machines());
        assert!(nothing.taught_anything());
    }

    #[test]
    fn work_a_machine_could_not_reach_is_reported_and_not_counted() {
        let unreachable = Outcome::out_of_reach("this machine does not have that commit");

        assert!(unreachable.is_the_machines());
        assert!(
            !unreachable.is_the_owners_to_fix(),
            "somebody was told to fix a machine that is working"
        );
    }

    #[test]
    fn an_outcome_survives_the_columns_it_is_kept_in() {
        for outcome in [
            Outcome::Produced {
                said: "there is something to look at".to_owned(),
            },
            Outcome::NothingFound {
                said: "no argument was found".to_owned(),
            },
            Outcome::out_of_reach("this machine does not have that commit"),
            Outcome::Broke {
                code: "agent_error".to_owned(),
                said: "it exited with status 1".to_owned(),
            },
        ] {
            let rebuilt = Outcome::rebuild(
                outcome.as_str(),
                outcome.code().map(str::to_owned),
                outcome.said().to_owned(),
            );

            assert_eq!(rebuilt, Some(outcome));
        }

        assert_eq!(Outcome::rebuild("nonsense", None, String::new()), None);
    }

    #[test]
    fn how_a_job_ended_is_a_record_and_where_it_is_now_is_a_condition() {
        assert_eq!(Outcome::KIND, FactKind::Record);
        assert_eq!(JobState::KIND, FactKind::Condition);
        assert!(!Outcome::KIND.is_counted());
    }
}

//! What travels between this machine and an instance.
//!
//! Each type here is one side of an exchange in the protocol: what the
//! instance answers, or what this machine sends. The field names are the
//! protocol's, so none of them may change without the instance changing
//! with it.

use serde::{Deserialize, Serialize};

/// What an instance says when a machine asks to sign in.
#[derive(Debug, Clone, Deserialize)]
pub struct SignIn {
    /// The secret this machine keeps.
    pub device_code: String,
    /// The code the person reads and types.
    pub user_code: String,
    /// Where they go to type it.
    pub verification_uri: String,
    /// The same place, with the code already in it.
    pub verification_uri_complete: String,
    /// How long they have, in seconds.
    pub expires_in: u32,
    /// How often to ask again, in seconds.
    pub interval: u32,
}

/// What an instance says when the machine asks whether it may come in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Nobody has answered yet.
    Waiting,
    /// Somebody refused.
    Refused,
    /// Nobody answered in time.
    RanOut,
    /// Here is the credential.
    Admitted {
        /// The credential to keep.
        token: String,
        /// Whose account this machine now acts for.
        user: String,
    },
}

/// One group this machine can work in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GroupRemote {
    /// The group's short name, which is also its directory.
    pub slug: String,
    /// What the group calls itself.
    pub name: String,
    /// What this person may do in it.
    pub role: String,
    /// Where the group's own repository is.
    pub shared: String,
    /// Where this person's repository inside it is.
    pub personal: String,
}

/// What the instance says this machine can work in.
#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    /// Whose account this is.
    pub user: String,
    /// The groups, with their repositories.
    pub groups: Vec<GroupRemote>,
}

/// What the instance knows about this machine as a node.
#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    /// Its identifier here.
    pub machine: String,
    /// What it calls itself.
    pub label: String,
    /// Whether the instance takes it to be there.
    pub presence: stemma_core::machine::Presence,
    /// The groups it serves.
    pub groups: Vec<String>,
    /// What it last reported finding.
    pub agents: Vec<stemma_core::machine::Detected>,
}

/// Accepted source ready to be written by its author's machine.
#[derive(Debug, Clone, Deserialize)]
pub struct AcceptedProposal {
    /// Complete accepted source.
    pub source: String,
    /// Complete accepted Lean module, when formal work changed.
    #[serde(default)]
    pub lean_source: Option<String>,
    /// Per-file change to the group's Lake package, when formal work changed it.
    #[serde(default)]
    pub library_change: Option<stemma_core::proposal::LibraryChange>,
    /// Group whose clone holds it.
    pub group: String,
    /// Shared or private repository.
    pub side: String,
    /// Line directory.
    pub line: String,
    /// The line, by identifier, so it is found after it moved.
    #[serde(default)]
    pub line_id: Option<String>,
    /// The revision the proposal was made against.
    pub revision: u64,
}

/// An accepted proposal no machine has applied yet.
#[derive(Debug, Clone, Deserialize)]
pub struct ToApply {
    /// The proposal's identifier.
    pub proposal: String,
    /// What was accepted.
    #[serde(flatten)]
    pub accepted: AcceptedProposal,
}

/// One machine offered as a deliberate choice for a group.
#[derive(Debug, Clone, Deserialize)]
pub struct OfferedMachine {
    /// Stable machine identifier.
    pub machine: String,
    /// Owner-given label.
    pub label: String,
    /// Awake or asleep.
    pub presence: String,
    /// Every known agent, including unavailable ones with their reason.
    pub agents: Vec<stemma_core::machine::Detected>,
}

/// One immutable definition returned by the group vault.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VaultEntry {
    /// Stable vault identity.
    pub id: String,
    /// Stable source siglum.
    pub siglum: String,
    /// Distinguishing name.
    pub name: String,
    /// Current correspondence condition.
    pub correspondence: String,
}

/// One proved substitution licence returned by the group vault.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VaultBridge {
    /// Stable bridge identity.
    pub id: String,
    /// Definition at one end.
    pub left: String,
    /// Definition at the other end.
    pub right: String,
    /// Predicate or structure bridge.
    pub kind: String,
    /// Current correspondence condition.
    pub correspondence: String,
    /// Whether Stemma composed it on demand.
    pub derived: bool,
}

/// The usable and historical objects in a group's vault.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Vault {
    /// Definitions.
    pub entries: Vec<VaultEntry>,
    /// Proved bridges.
    pub bridges: Vec<VaultBridge>,
}

/// The machines serving a group, as the instance lists them.
#[derive(Deserialize)]
pub(super) struct MachineChoices {
    pub(super) machines: Vec<OfferedMachine>,
}

/// One job, as the machine that took it is told about it.
#[derive(Debug, Clone, Deserialize)]
pub struct Handed {
    /// Its identifier.
    pub job: String,
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
    /// The line, by identifier, so it is found after it moved.
    #[serde(default)]
    pub line_id: Option<String>,
    /// The revision of the line to work against.
    pub revision: u64,
    /// The other revision, when the job is to bring two of them together.
    #[serde(default)]
    pub against: Option<u64>,
    /// What is being asked.
    pub task: String,
    /// Which agent to use.
    pub agent: String,
    /// Concrete model override.
    #[serde(default)]
    pub model: Option<String>,
    /// Reasoning effort override.
    #[serde(default)]
    pub effort: Option<String>,
    /// A person's request for a custom task.
    #[serde(default)]
    pub request: Option<String>,
    /// Explicit capabilities for a custom task.
    #[serde(default)]
    pub grants: Option<stemma_core::proposal::Grants>,
    /// Whether the launcher asked this running job to stop.
    #[serde(default)]
    pub cancel_requested: bool,
    /// Who asked, and answers for it.
    pub launched_by: String,
}

/// One job in a listing.
#[derive(Debug, Clone, Deserialize)]
pub struct Listed {
    /// Its identifier.
    pub job: String,
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    #[serde(default)]
    pub side: String,
    /// Which line of work.
    pub line: String,
    /// What is being asked.
    pub task: String,
    /// Which agent.
    pub agent: String,
    /// Where it has got to.
    pub state: String,
    /// One line about it.
    pub said: String,
    /// How it ended, once it has: `produced`, `nothing_found`,
    /// `could_not_try` or `broke`.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Whether it ended in something the machine's owner has to fix and
    /// has not marked as resolved.
    #[serde(default)]
    pub owed: bool,
    /// Inspectable workspace, once the machine reports it.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Latest progress sentence.
    #[serde(default)]
    pub progress: Option<String>,
    /// Whether cancellation has been requested.
    #[serde(default)]
    pub cancel_requested: bool,
}

/// What the instance answers a running job's progress with.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct JobControl {
    pub(super) cancel_requested: bool,
}

/// What a machine says to launch a job.
#[derive(Debug, Clone, Serialize)]
pub struct Asking {
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
    /// The revision of the line to work against.
    pub revision: u64,
    /// The other revision, for a job that brings two of them together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub against: Option<u64>,
    /// What is being asked.
    pub task: String,
    /// Which agent.
    pub agent: String,
    /// Which machine, where one was chosen.
    pub machine: Option<String>,
    /// Concrete model override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Reasoning effort override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// A person's request for a custom task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    /// Explicit capabilities for a custom task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grants: Option<stemma_core::proposal::Grants>,
}

/// One proposal waiting for a person.
#[derive(Debug, Clone, Deserialize)]
pub struct Waiting {
    /// Its identifier.
    pub proposal: String,
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
    /// What was asked for.
    pub task: String,
    /// Which agent did it.
    pub agent: String,
    /// Who supplied that agent.
    pub vendor: String,
    /// The concrete model.
    pub model: String,
    /// Where review has got to.
    pub state: String,
    /// Decision, when this is a recent review rather than a pending one.
    #[serde(default)]
    pub decision: Option<String>,
    /// The reviewer's reason or instruction.
    #[serde(default)]
    pub note: Option<String>,
}

/// Pending and recently decided proposals for a reviewer.
#[derive(Debug, Clone, Deserialize)]
pub struct Reviews {
    /// Proposals still waiting.
    pub proposals: Vec<Waiting>,
    /// The last decisions made by this person.
    pub recent: Vec<Waiting>,
}

/// One proposal in a line's lasting review history.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LineProposal {
    /// Its identifier.
    pub proposal: String,
    /// The task that produced it.
    pub task: String,
    /// Agent and model attribution.
    pub agent: String,
    /// Vendor attribution.
    pub vendor: String,
    /// Concrete model.
    pub model: String,
    /// Version of the instructions given to the agent.
    pub instructions_version: String,
    /// Agent explanation.
    pub explanation: String,
    /// Review state.
    pub state: String,
    /// The reviewer's choice.
    pub decision: Option<String>,
    /// Reviewer's reason or next-attempt note.
    pub note: Option<String>,
    /// The member the review was assigned to.
    pub reviewer: String,
    /// The member who made the decision.
    pub decided_by: Option<String>,
    /// The line revision it became.
    pub applied_revision: Option<u64>,
}

/// One proposal, with the version it would replace.
#[derive(Debug, Clone, Deserialize)]
pub struct Offered {
    /// Its identifier.
    pub proposal: String,
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
    /// The line revision it was written against.
    pub revision: u64,
    /// What was asked for.
    pub task: String,
    /// Which agent did it.
    pub agent: String,
    /// Who supplied that agent.
    pub vendor: String,
    /// The concrete model.
    pub model: String,
    /// Which prompt contract produced it.
    pub prompt_revision: String,
    /// Narrow operations submitted through the job API.
    #[serde(default)]
    pub operations: Vec<stemma_core::proposal::Operation>,
    /// The agent's reviewer-facing summary.
    #[serde(default)]
    pub explanation: String,
    /// Selected reasoning effort.
    #[serde(default)]
    pub effort: Option<String>,
    /// Where review has got to.
    pub state: String,
    /// The decision if this proposal was reviewed.
    #[serde(default)]
    pub decision: Option<String>,
    /// The reviewer's note, if one was given.
    #[serde(default)]
    pub note: Option<String>,
    /// The member currently responsible for this review.
    #[serde(default)]
    pub reviewer: Option<String>,
    /// The member who made the decision, when decided.
    #[serde(default)]
    pub decided_by: Option<String>,
    /// Members the launcher may hand a shared review to.
    #[serde(default)]
    pub members: Vec<ReviewMember>,
    /// The source it would replace, when the instance still has it.
    pub stands: Option<String>,
    /// The line's latest server revision.
    #[serde(default)]
    pub current: Option<String>,
    /// The source it offers.
    pub source: String,
    /// The Lean module beside it, when formal work changed.
    pub lean_source: Option<String>,
    /// Per-file change to the group's Lake package, when formal work changed it.
    #[serde(default)]
    pub library_change: Option<stemma_core::proposal::LibraryChange>,
    /// Lean-produced evidence for reviewing the library change.
    #[serde(default)]
    pub formal_review: Option<stemma_core::proposal::FormalReview>,
    /// What the instance checked before a person was asked.
    pub findings: Vec<String>,
}

/// A member who may review a shared proposal.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReviewMember {
    /// Their user identifier.
    pub id: String,
    /// Their display name.
    pub name: String,
}

/// What starting a line asks for.
#[derive(Debug, Clone, Serialize)]
pub struct Starting {
    /// The group's short name.
    pub group: String,
    /// What the line is called.
    pub title: String,
    /// What it is about, where its author says.
    pub description: Option<String>,
    /// `me` or `group`.
    pub seen_by: String,
}

/// What moving a line to the group asks for.
#[derive(Debug, Clone, Serialize)]
pub struct Sharing {
    /// `today` or `everything`.
    pub travels: String,
    /// `author` or `anyone`.
    pub writing: String,
}

/// Where a line ended up.
#[derive(Debug, Clone, Deserialize)]
pub struct Placed {
    /// The line's identifier.
    #[serde(default)]
    pub id: Option<String>,
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// What the line is called.
    pub line: String,
    /// The revision it is at.
    #[serde(default)]
    pub revision: u64,
}

/// A line as the instance lists it to a machine.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Listing {
    /// Its identifier, which it keeps for its whole life.
    pub id: String,
    /// The group's short name.
    pub group: String,
    /// Which of the group's repositories it is in.
    pub side: String,
    /// What it is called.
    pub line: String,
    /// Who started it.
    pub author: String,
    /// Who may write it: `author` or `anyone`.
    pub writing: String,
    /// The revision it is at.
    pub revision: u64,
    /// Whether it has been deleted.
    pub deleted: bool,
}

/// One revision of one line, as a machine receives it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Revision {
    /// The line's identifier.
    pub id: String,
    /// The group's short name.
    pub group: String,
    /// Which of the group's repositories the line was in.
    pub side: String,
    /// What the line is called.
    pub line: String,
    /// Which revision.
    pub revision: u64,
    /// What made it: `started`, `written`, `shared` or `deleted`.
    pub kind: String,
    /// The line, as its `line.json`.
    pub source: String,
    /// The Lean module beside it.
    #[serde(default)]
    pub lean_source: Option<String>,
}

/// What a machine sends to write a line.
#[derive(Debug, Clone, Serialize)]
pub struct Outgoing {
    /// The revision it was written from.
    pub base: u64,
    /// The line, as its `line.json`.
    pub source: String,
    /// The Lean module beside it.
    pub lean_source: Option<String>,
    /// What it typesets to.
    pub page: String,
}

/// What the instance made of a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    /// It is this revision now.
    Taken(u64),
    /// The line moved on from the revision it was written from; this is
    /// where it is now.
    Stale(Box<Revision>),
    /// This machine does not hold the line; the one that does calls
    /// itself this, when one does.
    NotHeld(Option<String>),
    /// The line has been deleted.
    Gone,
    /// What was sent is not something a line can be, and why.
    Refused(String),
}

/// Which line a machine is talking to the instance about.
#[derive(Debug, Clone, Serialize)]
pub struct About {
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
}

/// What the instance says about the claim on a line.
#[derive(Debug, Clone, Deserialize)]
pub struct Claimed {
    /// `writing`, `waiting` or `open`.
    pub standing: String,
    /// The machine that has it, when one does.
    #[serde(default)]
    pub machine: Option<String>,
    /// What that machine calls itself.
    #[serde(default)]
    pub label: Option<String>,
    /// How long the claim has left.
    #[serde(default)]
    pub left: Option<u64>,
    /// Whether it has been asked for.
    #[serde(default)]
    pub asked: bool,
    /// The whole of it, in the words the instance uses.
    #[serde(default)]
    pub said: Option<String>,
}

impl Claimed {
    /// Whether this machine may write the line.
    pub fn may_write(&self) -> bool {
        self.standing == "writing" || self.standing == "open"
    }
}

/// One line a machine is writing.
#[derive(Debug, Clone, Deserialize)]
pub struct Writing {
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
    /// What the machine holding it calls itself.
    pub label: String,
    /// Whether the machine holding it is this one.
    pub mine: bool,
    /// How long the claim has left.
    pub left: u64,
    /// Whether it has been asked for.
    pub asked: bool,
}

/// Who a credential acts for.
#[derive(Debug, Clone, Deserialize)]
pub struct Whoami {
    /// The account.
    pub user: String,
    /// What to call them.
    pub display_name: String,
}

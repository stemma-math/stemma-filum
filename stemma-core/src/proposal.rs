//! Changes offered for a person to review.
//!
//! A proposal is not mathematics until somebody accepts it. The guard in
//! this module needs no mathematical judgement: it compares the source at
//! the job's base commit with the source offered by the agent and refuses
//! any change to the text or declared dependencies of a central statement.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::actor::{AgentRun, Attribution};
use crate::artefact::BlockCheck;
use crate::document::{Block, Document};
use crate::fact::{Fact, FactKind};
use crate::id::{BlockId, JobId, ProposalId, UserId};
use crate::job::Outcome;
use crate::line::{Line, Siglum};

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
    /// The complete Lean module beside the source, when formal work changed.
    #[serde(default)]
    pub lean_source: Option<String>,
    /// An approved-only change to the group's Lake package, when formal work moved there.
    #[serde(default)]
    pub library_change: Option<LibraryChange>,
    /// Lean-produced evidence shown beside a proposed library change.
    #[serde(default)]
    pub formal_review: Option<Box<FormalReview>>,
    /// Who supplied the agent.
    pub vendor: String,
    /// The concrete model the agent reported using.
    pub model: String,
    /// Which prompt contract produced it.
    pub prompt_revision: String,
    /// The narrow actions the agent took against the job's base revision.
    #[serde(default)]
    pub operations: Vec<Operation>,
    /// The agent's own summary for the reviewer.
    #[serde(default)]
    pub explanation: String,
    /// The reasoning effort selected for the run, when the vendor supports it.
    #[serde(default)]
    pub effort: Option<String>,
}

/// One reviewable action produced through the job API.
///
/// Line actions name stable blocks and library actions name individual files,
/// so a proposal never needs to claim the whole workspace as its answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Operation {
    /// Replace one existing block, preserving its stable identifier.
    EditBlock {
        /// The block being edited.
        block: BlockId,
        /// Its proposed value.
        with: Block,
    },
    /// Insert blocks after an existing block, or at the beginning.
    AddBlocks {
        /// The block after which they belong. `None` means the beginning.
        after: Option<BlockId>,
        /// The blocks, in order.
        blocks: Vec<Block>,
    },
    /// Remove one block.
    RemoveBlock {
        /// The block being removed.
        block: BlockId,
    },
    /// Replace, create, or remove one file in the group's Lean library.
    WriteLibraryFile {
        /// A relative path below the package root.
        path: String,
        /// Its contents at the job's base, or absence for a new file.
        before: Option<String>,
        /// Its proposed contents, or absence to remove it.
        after: Option<String>,
    },
}

/// The capabilities granted to one job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grants {
    /// Line actions the job API will admit.
    #[serde(default)]
    pub line: Vec<LineGrant>,
    /// Whether files in the Lean library may be changed.
    #[serde(default)]
    pub library: bool,
    /// Whether the task may only add content.
    #[serde(default)]
    pub add_only: bool,
}

/// One line capability granted to a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "grant", rename_all = "snake_case")]
pub enum LineGrant {
    /// Edit this block.
    Edit {
        /// The block that may be edited.
        block: BlockId,
    },
    /// Add blocks after this block, or at the beginning.
    Add {
        /// The position after which blocks may be added.
        after: Option<BlockId>,
    },
    /// Remove this non-central block.
    Remove {
        /// The block that may be removed.
        block: BlockId,
    },
}

impl Grants {
    /// Whether an operation is inside these grants.
    pub fn admits(&self, operation: &Operation) -> bool {
        if self.add_only && !matches!(operation, Operation::AddBlocks { .. }) {
            return false;
        }

        match operation {
            Operation::EditBlock { block, .. } => self.line.iter().any(
                |grant| matches!(grant, LineGrant::Edit { block: allowed } if allowed == block),
            ),
            Operation::AddBlocks { after, .. } => self.line.iter().any(
                |grant| matches!(grant, LineGrant::Add { after: allowed } if allowed == after),
            ),
            Operation::RemoveBlock { block } => self.line.iter().any(
                |grant| matches!(grant, LineGrant::Remove { block: allowed } if allowed == block),
            ),
            Operation::WriteLibraryFile { .. } => self.library && !self.add_only,
        }
    }
}

/// Applies line operations one at a time, validating scope and the line after
/// every action so an agent receives the error while it can still correct it.
pub fn apply_line_operations(
    base: &str,
    grants: &Grants,
    operations: &[Operation],
) -> Result<String, String> {
    let mut document = Document::parse(base).map_err(|error| error.to_string())?;

    for operation in operations {
        if !grants.admits(operation) {
            return Err("that action is outside this job's grants".to_owned());
        }

        match operation {
            Operation::EditBlock { block, with } => {
                if with.id() != block {
                    return Err("an edited block must keep its identifier".to_owned());
                }
                let at = document
                    .position(block)
                    .ok_or_else(|| format!("block `{block}` is not in the line"))?;
                document.blocks[at] = with.clone();
            }
            Operation::AddBlocks { after, blocks } => {
                if blocks.is_empty() {
                    return Err("an add action must contain at least one block".to_owned());
                }
                let mut known: BTreeSet<BlockId> =
                    document.blocks.iter().map(|block| *block.id()).collect();
                for block in blocks {
                    if !known.insert(*block.id()) {
                        return Err(format!("block `{}` already exists", block.id()));
                    }
                }
                let at = match after {
                    Some(block) => document
                        .position(block)
                        .map(|position| position + 1)
                        .ok_or_else(|| format!("block `{block}` is not in the line"))?,
                    None => 0,
                };
                document.blocks.splice(at..at, blocks.iter().cloned());
            }
            Operation::RemoveBlock { block } => {
                let at = document
                    .position(block)
                    .ok_or_else(|| format!("block `{block}` is not in the line"))?;
                if document.blocks[at]
                    .statement()
                    .is_some_and(|statement| statement.central)
                {
                    return Err(format!("central block `{block}` cannot be removed"));
                }
                document.blocks.remove(at);
            }
            Operation::WriteLibraryFile { path, .. } => {
                if !safe_library_path(path) {
                    return Err(format!("`{path}` is not a safe library path"));
                }
                continue;
            }
        }

        let proposed = document.to_text();
        preserves_central_statements(base, &proposed).map_err(|error| error.to_string())?;
        Line::read(&proposed).map_err(|faults| {
            faults
                .first()
                .map(ToString::to_string)
                .unwrap_or_else(|| "the proposed line is invalid".to_owned())
        })?;
    }

    Ok(document.to_text())
}

/// Whether a library path stays below the package root.
pub fn safe_library_path(path: &str) -> bool {
    let path = std::path::Path::new(path);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path.components().all(|part| {
            matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

/// Machine-checked evidence attached to a proposed Lean library change.
///
/// This is reconstructible evidence, not an audit: only the later human
/// decision says that a printed Lean statement means what its prose says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormalReview {
    /// Checks whose observable formal meaning changed, plus the task's target.
    pub blocks: Vec<ReviewedBlock>,
    /// What kind of formal surface the file change reaches.
    pub impact: FormalImpact,
}

/// One block as the worker asks a person to compare it with Lean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewedBlock {
    /// The extractor's complete result for the block after the change.
    pub check: BlockCheck,
    /// The agent's plain-language reading of the printed statement.
    pub reading: String,
}

/// The observable reach of a proposed library change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormalImpact {
    /// The changed files leave every extracted statement and line link intact.
    pub proofs_only: bool,
    /// Blocks whose transitive statement closure changed.
    pub statement_closures: Vec<crate::id::BlockId>,
    /// Blocks whose links to other marked declarations changed.
    pub line_links: Vec<crate::id::BlockId>,
}

impl FormalReview {
    /// Compares extractor output before and after a staged library change.
    ///
    /// Proof terms are deliberately absent from an extractor digest. Thus an
    /// otherwise non-empty change whose statement closures and marked links
    /// are unchanged is exactly a proof-only change at the review boundary.
    pub fn between(
        before: &[BlockCheck],
        after: &[BlockCheck],
        targets: impl IntoIterator<Item = crate::id::BlockId>,
        readings: &BTreeMap<crate::id::BlockId, String>,
    ) -> Self {
        let before: BTreeMap<_, _> = before.iter().map(|check| (check.block, check)).collect();
        let after: BTreeMap<_, _> = after.iter().map(|check| (check.block, check)).collect();
        let targets: BTreeSet<_> = targets.into_iter().collect();
        let blocks: BTreeSet<_> = before.keys().chain(after.keys()).copied().collect();
        let mut statement_closures = Vec::new();
        let mut line_links = Vec::new();
        let mut reviewed = Vec::new();

        for block in blocks {
            let old = before.get(&block).copied();
            let new = after.get(&block).copied();
            if statement_surface(old) != statement_surface(new) {
                statement_closures.push(block);
            }
            if link_surface(old) != link_surface(new) {
                line_links.push(block);
            }
            if let Some(check) = new.filter(|_| old != new || targets.contains(&block)) {
                reviewed.push(ReviewedBlock {
                    check: check.clone(),
                    reading: readings.get(&block).cloned().unwrap_or_default(),
                });
            }
        }

        Self {
            blocks: reviewed,
            impact: FormalImpact {
                proofs_only: statement_closures.is_empty() && line_links.is_empty(),
                statement_closures,
                line_links,
            },
        }
    }
}

fn statement_surface(check: Option<&BlockCheck>) -> Vec<(&str, &str, &str)> {
    check
        .into_iter()
        .flat_map(|check| &check.declarations)
        .map(|declaration| {
            (
                declaration.name.as_str(),
                declaration.statement.as_str(),
                declaration.statement_closure.as_str(),
            )
        })
        .collect()
}

fn link_surface(check: Option<&BlockCheck>) -> Vec<(&str, &[String])> {
    check
        .into_iter()
        .flat_map(|check| &check.declarations)
        .map(|declaration| (declaration.name.as_str(), declaration.reaches.as_slice()))
        .collect()
}

/// A revision-based, per-file change to a Lake package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryChange {
    /// The library commit the agent worked from.
    pub base: String,
    /// Files changed by the agent. Files absent here are untouched.
    pub files: Vec<LibraryFileChange>,
    /// The agent's reading of each marked formal statement, keyed by block.
    #[serde(default)]
    pub readings: BTreeMap<crate::id::BlockId, String>,
}

/// One file in a proposed library change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryFileChange {
    /// Its path below the package root.
    pub path: String,
    /// What it said at [`LibraryChange::base`], or absence for a new file.
    pub before: Option<String>,
    /// What it should say, or absence to remove it.
    pub after: Option<String>,
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

/// What a reviewer decides about a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Sign the words as the reviewer's own.
    Accept,
    /// Decline them.
    Reject,
    /// Decline them and queue the same question again.
    AskAgain,
}

spelled!(Verdict, UnknownVerdict("`{0}` is not a decision: use `accept`, `reject` or `ask_again`") {
    Accept => "accept",
    Reject => "reject",
    AskAgain => "ask_again",
});

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
    /// It is called something else.
    #[error("the proposal renames central statement `{0}`")]
    Renamed(Siglum),
    /// Its written claim changed.
    #[error("the proposal changes the text of central statement `{0}`")]
    ChangedText(Siglum),
    /// What it declares it leans on changed.
    #[error("the proposal changes what central statement `{0}` uses")]
    ChangedUses(Siglum),
    /// Which audited substitutions it relies on changed.
    #[error("the proposal changes the vault bridges central statement `{0}` uses")]
    ChangedBridges(Siglum),
}

/// Checks that an agent left every existing central statement frozen.
///
/// Statements are matched by block, so a frozen statement cannot be
/// slipped past the guard by renaming it. What it says, what it is
/// called, what it declares it uses and the bridges it relies on are
/// all frozen; its proof is not, and neither is where it stands.
pub fn preserves_central_statements(base: &str, offered: &str) -> Result<(), GuardError> {
    let base = read("base", base)?;
    let offered = read("proposed", offered)?;

    for block in &base.blocks {
        let Some(frozen) = block.statement().filter(|statement| statement.central) else {
            continue;
        };
        let siglum = block.siglum().expect("a statement has a siglum");

        let Some(after) = offered
            .block(&frozen.id)
            .filter(|after| after.kind() == block.kind())
            .and_then(Block::statement)
            .filter(|statement| statement.central)
        else {
            return Err(GuardError::Removed(siglum));
        };

        if after.siglum != frozen.siglum {
            return Err(GuardError::Renamed(siglum));
        }
        if after.content != frozen.content
            || after.environment != frozen.environment
            || after.title != frozen.title
        {
            return Err(GuardError::ChangedText(siglum));
        }
        if after.uses != frozen.uses {
            return Err(GuardError::ChangedUses(siglum));
        }
        if after.bridges != frozen.bridges {
            return Err(GuardError::ChangedBridges(siglum));
        }
    }

    Ok(())
}

/// Reads one side, which has to be a line and not only a document.
fn read(which: &'static str, text: &str) -> Result<Document, GuardError> {
    let invalid = |reason: String| GuardError::InvalidSource { which, reason };
    let document = Document::parse(text).map_err(|unreadable| invalid(unreadable.to_string()))?;

    Line::of(&document).map_err(|faults| {
        invalid(
            faults
                .first()
                .map(ToString::to_string)
                .unwrap_or_else(|| "it is not a line".to_owned()),
        )
    })?;

    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artefact::FormalDeclaration;
    use crate::model::Verification;

    fn base() -> Document {
        let mut document = Document::new("A line", None);
        let group = document.push(Block::definition(
            Some("def:group".parse().unwrap()),
            "A group.",
        ));
        let mut set = Block::definition(Some("def:set".parse().unwrap()), "A set.");
        set.statement_mut().unwrap().central = false;
        let set = document.push(set);
        let mut main = Block::theorem(
            "Theorem",
            Some("thm:main".parse().unwrap()),
            "Every finite group is finite.",
        );
        main.statement_mut().unwrap().central = true;
        main.statement_mut().unwrap().uses.push(group);
        let main = document.push(main);
        document.push(Block::proof(main, "It is."));
        let mut step = Block::theorem(
            "Lemma",
            Some("lem:step".parse().unwrap()),
            "An old argument.",
        );
        step.statement_mut().unwrap().proof = false;
        step.statement_mut().unwrap().uses.push(set);
        document.push(step);
        document
    }

    fn checked(block: crate::id::BlockId, closure: &str, reaches: &[&str]) -> BlockCheck {
        BlockCheck {
            block,
            state: Verification::Verified,
            diagnostic: String::new(),
            declarations: vec![FormalDeclaration {
                name: "Library.main".to_owned(),
                module: "Library".to_owned(),
                statement: "True".to_owned(),
                sorry_free: true,
                axioms: Vec::new(),
                reaches: reaches.iter().map(|name| (*name).to_owned()).collect(),
                statement_closure: closure.to_owned(),
            }],
        }
    }

    #[test]
    fn formal_review_separates_proof_changes_from_statement_and_link_changes() {
        let block = crate::id::BlockId::generate();
        let mut proof_before = checked(block, "same", &["Library.lemma"]);
        proof_before.declarations[0].sorry_free = false;
        let proof_after = checked(block, "same", &["Library.lemma"]);
        let proof = FormalReview::between(
            &[proof_before],
            std::slice::from_ref(&proof_after),
            [block],
            &BTreeMap::from([(block, "The proposition is true.".to_owned())]),
        );
        assert!(proof.impact.proofs_only);
        assert_eq!(proof.blocks[0].reading, "The proposition is true.");

        let changed = FormalReview::between(
            std::slice::from_ref(&proof_after),
            &[checked(block, "different", &["Library.other"])],
            [block],
            &BTreeMap::new(),
        );
        assert!(!changed.impact.proofs_only);
        assert_eq!(changed.impact.statement_closures, [block]);
        assert_eq!(changed.impact.line_links, [block]);
    }

    fn statement(document: &mut Document, at: usize) -> &mut crate::document::Statement {
        document.blocks[at].statement_mut().unwrap()
    }

    fn guarded(original: &Document, offered: &Document) -> Result<(), GuardError> {
        preserves_central_statements(&original.to_text(), &offered.to_text())
    }

    #[test]
    fn an_agent_may_rewrite_the_inside_of_a_box_and_any_proof() {
        let original = base();
        let mut offered = original.clone();
        statement(&mut offered, 4).content = "A much better argument.".to_owned();
        offered.blocks[3] = Block::proof(*offered.blocks[2].id(), "By @lem:step.");
        offered.blocks.swap(0, 1);
        assert_eq!(guarded(&original, &offered), Ok(()));
    }

    #[test]
    fn changing_a_central_claim_is_refused() {
        let original = base();
        let mut offered = original.clone();
        statement(&mut offered, 2).content = "Every group is finite.".to_owned();
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &offered.to_text()),
            Err(GuardError::ChangedText(_))
        ));
    }

    #[test]
    fn a_central_definition_is_frozen_too() {
        let original = base();
        let mut offered = original.clone();
        statement(&mut offered, 0).title = Some("Monoid".to_owned());
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &offered.to_text()),
            Err(GuardError::ChangedText(_))
        ));
    }

    #[test]
    fn changing_a_central_dependency_is_refused() {
        let original = base();
        let mut offered = original.clone();
        let set = *offered.blocks[1].id();
        statement(&mut offered, 2).uses = vec![set];
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &offered.to_text()),
            Err(GuardError::ChangedUses(_))
        ));
    }

    #[test]
    fn removing_or_demoting_or_renaming_a_central_statement_is_refused() {
        let original = base();

        let mut demoted = original.clone();
        statement(&mut demoted, 2).central = false;
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &demoted.to_text()),
            Err(GuardError::Removed(_))
        ));

        let mut renamed = original.clone();
        statement(&mut renamed, 2).siglum = Some("thm:other".parse().unwrap());
        assert!(matches!(
            preserves_central_statements(&original.to_text(), &renamed.to_text()),
            Err(GuardError::Renamed(_))
        ));
    }

    #[test]
    fn an_offer_that_is_not_a_line_is_refused() {
        let original = base();
        let broken = Document {
            blocks: vec![Block::theorem("Theorem", None, "No proof block.")],
            ..original.clone()
        };
        assert!(matches!(
            guarded(&original, &broken),
            Err(GuardError::InvalidSource {
                which: "proposed",
                ..
            })
        ));
    }

    #[test]
    fn a_proposal_state_is_a_condition_not_an_endorsement_record() {
        assert_eq!(ProposalState::KIND, FactKind::Condition);
    }

    #[test]
    fn operations_are_checked_against_scope_and_central_statements_as_they_arrive() {
        let original = base();
        let ordinary = *original.blocks[4].id();
        let central = *original.blocks[2].id();
        let mut changed = original.blocks[4].clone();
        changed.statement_mut().unwrap().content = "A corrected argument.".to_owned();
        let grants = Grants {
            line: vec![LineGrant::Edit { block: ordinary }],
            library: false,
            add_only: false,
        };
        let source = apply_line_operations(
            &original.to_text(),
            &grants,
            &[Operation::EditBlock {
                block: ordinary,
                with: changed,
            }],
        )
        .unwrap();
        assert!(source.contains("A corrected argument."));

        let mut changed = original.blocks[2].clone();
        changed.statement_mut().unwrap().content = "A different central claim.".to_owned();
        let central_grant = Grants {
            line: vec![LineGrant::Edit { block: central }],
            ..Grants::default()
        };
        assert!(
            apply_line_operations(
                &original.to_text(),
                &central_grant,
                &[Operation::EditBlock {
                    block: central,
                    with: changed,
                }],
            )
            .is_err()
        );
    }

    #[test]
    fn an_add_only_grant_refuses_every_other_kind_of_action() {
        let original = base();
        let ordinary = *original.blocks[4].id();
        let grants = Grants {
            line: vec![LineGrant::Edit { block: ordinary }],
            add_only: true,
            library: true,
        };
        assert!(!grants.admits(&Operation::EditBlock {
            block: ordinary,
            with: original.blocks[4].clone(),
        }));
        assert!(!grants.admits(&Operation::WriteLibraryFile {
            path: "Stemma/Next.lean".to_owned(),
            before: None,
            after: Some("theorem next : True := by trivial\n".to_owned()),
        }));
    }
}

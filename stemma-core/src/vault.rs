//! Immutable group definitions, proved bridges and correspondence audits.
//!
//! The vault does not make a definition reliable by authority. A definition
//! enters only with a compiled formalization; alternatives are licensed only
//! by an unconditional, compiled bridge; and a human audit records that the
//! formal statement says what its prose says.

use serde::{Deserialize, Serialize};

use crate::fact::{Fact, FactKind};
use crate::id::{AuditId, BridgeId, GroupId, UserId, VaultEntryId};
use crate::line::Siglum;
use crate::model::Correspondence;

/// An immutable definition in one group's vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Stable identity independent of the source line it came from.
    pub id: VaultEntryId,
    /// Group whose mathematical base contains it.
    pub group: GroupId,
    /// Stable source identifier.
    pub siglum: Siglum,
    /// Distinguishing mathematical name.
    pub name: String,
    /// Exact prose admitted by a person.
    pub prose: String,
    /// Exact Lean declaration that compiled when admitted.
    pub formalization: String,
    /// Current correspondence condition.
    pub correspondence: Correspondence,
}

impl Entry {
    /// Builds an admitted definition, refusing a name that distinguishes
    /// nothing. Numbers and possessive author names are not accepted as a
    /// substitute for saying what is mathematically different.
    pub fn admit(
        group: GroupId,
        siglum: Siglum,
        name: impl Into<String>,
        prose: impl Into<String>,
        formalization: impl Into<String>,
    ) -> Result<Self, VaultError> {
        let name = name.into().trim().to_owned();
        let prose = prose.into().trim().to_owned();
        let formalization = formalization.into().trim().to_owned();
        if !distinguishes(&name) {
            return Err(VaultError::UndistinguishedName);
        }
        if prose.is_empty() || formalization.is_empty() {
            return Err(VaultError::IncompleteDefinition);
        }
        Ok(Self {
            id: VaultEntryId::generate(),
            group,
            siglum,
            name,
            prose,
            formalization,
            correspondence: Correspondence::NotAudited,
        })
    }
}

/// What sort of equivalence a bridge proves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeKind {
    /// Logical equivalence between predicates.
    Predicate,
    /// An equivalence preserving the structure in question.
    Structure,
}

/// Why a bridge exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "origin", rename_all = "snake_case")]
pub enum BridgeOrigin {
    /// Somebody supplied and reviewed this proof.
    Direct,
    /// Stemma generated it on demand from two already proved bridges.
    Derived {
        /// The two bridge objects whose proofs were composed.
        parents: [BridgeId; 2],
    },
}

/// A proved, unconditional substitution licence between two definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bridge {
    /// Stable identity: a bridge is an object, never a bare edge.
    pub id: BridgeId,
    /// Definition substituted from.
    pub left: VaultEntryId,
    /// Definition substituted to.
    pub right: VaultEntryId,
    /// The sort of equivalence, which must agree across a composition.
    pub kind: BridgeKind,
    /// Exact compiled Lean theorem.
    pub theorem: String,
    /// Direct or generated on demand.
    pub origin: BridgeOrigin,
    /// Current correspondence condition.
    pub correspondence: Correspondence,
}

impl Bridge {
    /// Admits a direct bridge. Hypotheses are deliberately absent from this
    /// type: a conditional equivalence is an ordinary theorem, not a bridge.
    pub fn direct(
        left: VaultEntryId,
        right: VaultEntryId,
        kind: BridgeKind,
        theorem: impl Into<String>,
    ) -> Result<Self, VaultError> {
        if left == right {
            return Err(VaultError::SameDefinition);
        }
        let theorem = theorem.into().trim().to_owned();
        if theorem.is_empty() {
            return Err(VaultError::MissingProof);
        }
        Ok(Self {
            id: BridgeId::generate(),
            left,
            right,
            kind,
            theorem,
            origin: BridgeOrigin::Direct,
            correspondence: Correspondence::NotAudited,
        })
    }

    /// Composes two compatible bridges on demand. The generated Lean theorem
    /// is still required: graph reachability alone never licenses use.
    pub fn compose(
        first: &Self,
        second: &Self,
        theorem: impl Into<String>,
    ) -> Result<Self, VaultError> {
        if first.kind != second.kind {
            return Err(VaultError::DifferentBridgeKinds);
        }
        if first.right != second.left {
            return Err(VaultError::DisconnectedBridges);
        }
        if first.correspondence != Correspondence::Audited
            || second.correspondence != Correspondence::Audited
        {
            return Err(VaultError::UnauditedParent);
        }
        let theorem = theorem.into().trim().to_owned();
        if theorem.is_empty() {
            return Err(VaultError::MissingProof);
        }
        Ok(Self {
            id: BridgeId::generate(),
            left: first.left,
            right: second.right,
            kind: first.kind,
            theorem,
            origin: BridgeOrigin::Derived {
                parents: [first.id, second.id],
            },
            correspondence: Correspondence::NotAudited,
        })
    }
}

/// A permanent human record about correspondence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Stable record identity.
    pub id: AuditId,
    /// Person who read the formalization back into prose.
    pub by: UserId,
    /// Whether this granted or withdrew the guarantee.
    pub action: AuditAction,
    /// Human account of what was checked or why it was withdrawn.
    pub note: String,
}

impl Fact for AuditRecord {
    const KIND: FactKind = FactKind::Record;
}

/// What an audit record did at the time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// The reader vouched that prose and formalization correspond.
    Attested,
    /// A previous guarantee was taken back.
    Withdrawn,
}

/// A downstream person's signed response to a lost audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownstreamDecision {
    /// The work deliberately stands despite the lost guarantee.
    Stands,
    /// Its author will remove the dependency.
    Rewrite,
    /// Its author is waiting for a new audit.
    WaitingForAudit,
}

impl Fact for DownstreamDecision {
    const KIND: FactKind = FactKind::Record;
}

/// An unanswered request for a concrete person to decide downstream work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditObligation {
    /// Person who owes the answer.
    pub assigned_to: UserId,
    /// Bridge whose withdrawn correspondence reached their work.
    pub bridge: BridgeId,
    /// The affected statement in their source.
    pub statement: Siglum,
}

impl Fact for AuditObligation {
    const KIND: FactKind = FactKind::Obligation;
}

/// Why a vault operation is not admissible.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VaultError {
    /// The proposed name does not explain the mathematical distinction.
    #[error("a vault definition needs a distinguishing mathematical name")]
    UndistinguishedName,
    /// Prose or formal declaration is absent.
    #[error("a vault definition needs both prose and a compiled formalization")]
    IncompleteDefinition,
    /// An object cannot bridge itself.
    #[error("a bridge must relate two different definitions")]
    SameDefinition,
    /// A claimed bridge has no theorem.
    #[error("a bridge needs a compiled Lean theorem")]
    MissingProof,
    /// Predicate and structure equivalences do not compose.
    #[error("bridges of different kinds do not compose")]
    DifferentBridgeKinds,
    /// The middle definition is not shared.
    #[error("the two bridges do not meet at one definition")]
    DisconnectedBridges,
    /// A derived licence cannot rest on an unaudited one.
    #[error("both parent bridges must be audited before they can be composed")]
    UnauditedParent,
}

fn distinguishes(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let numbered = lower
        .split_whitespace()
        .last()
        .is_some_and(|word| word.chars().all(|character| character.is_ascii_digit()));
    !name.is_empty()
        && name.chars().any(char::is_alphabetic)
        && !lower.starts_with("definition")
        && !lower.starts_with("version")
        && !lower.ends_with("'s")
        && !numbered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::Slug;

    fn entry(name: &str) -> Entry {
        Entry::admit(
            GroupId::new(Slug::parse("algebra").unwrap()),
            "def:group".parse().unwrap(),
            name,
            "A group is ...",
            "def Group := ...",
        )
        .unwrap()
    }

    #[test]
    fn admission_needs_prose_formalization_and_a_distinguishing_name() {
        assert!(matches!(
            Entry::admit(
                GroupId::new(Slug::parse("algebra").unwrap()),
                "def:group".parse().unwrap(),
                "Definition 2",
                "A group",
                "def Group := Unit"
            ),
            Err(VaultError::UndistinguishedName)
        ));
        assert!(matches!(
            Entry::admit(
                GroupId::new(Slug::parse("algebra").unwrap()),
                "def:group".parse().unwrap(),
                "Topological group",
                "",
                "def Group := Unit"
            ),
            Err(VaultError::IncompleteDefinition)
        ));
    }

    #[test]
    fn a_bridge_is_an_object_and_never_a_conditional_edge() {
        let left = entry("Additive group");
        let right = entry("Multiplicative group");
        let bridge =
            Bridge::direct(left.id, right.id, BridgeKind::Structure, "theorem bridge").unwrap();

        assert_eq!(bridge.left, left.id);
        assert_eq!(bridge.correspondence, Correspondence::NotAudited);
        assert!(matches!(bridge.origin, BridgeOrigin::Direct));
    }

    #[test]
    fn composition_requires_a_shared_middle_kind_and_two_audits() {
        let one = entry("Additive group");
        let two = entry("Multiplicative group");
        let three = entry("Pointed group");
        let mut first = Bridge::direct(one.id, two.id, BridgeKind::Structure, "first").unwrap();
        let mut second = Bridge::direct(two.id, three.id, BridgeKind::Structure, "second").unwrap();

        assert_eq!(
            Bridge::compose(&first, &second, "composed"),
            Err(VaultError::UnauditedParent)
        );
        first.correspondence = Correspondence::Audited;
        second.correspondence = Correspondence::Audited;
        let composed = Bridge::compose(&first, &second, "composed").unwrap();
        assert!(matches!(composed.origin, BridgeOrigin::Derived { .. }));
    }
}

//! Typed identifiers.
//!
//! Every entity is named by a prefixed identifier, such as `usr:enric` or
//! `job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T`. The prefix is part of the value: it
//! travels through URLs, logs and the wire format, so an identifier pasted
//! anywhere says what kind of thing it names.
//!
//! There are two families. A [`NamedId`] wraps a [`Slug`] chosen by a person
//! and is used where the identifier is read, typed and passed around; group
//! remotes derive their URL from one, which is why slugs are short. A
//! [`UniqueId`] wraps a generated ULID and is used for everything else.
//!
//! Both are generic over a marker type, so a [`UserId`] cannot be passed
//! where a [`GroupId`] is expected even though both are strings underneath.

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ulid::Ulid;

/// The shortest accepted slug.
pub const SLUG_MIN_LEN: usize = 2;

/// The longest accepted slug.
pub const SLUG_MAX_LEN: usize = 32;

/// A human-chosen name, safe to place in a URL, a path or a git remote.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slug(String);

/// Why a string was rejected as a [`Slug`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlugError {
    /// The string is shorter than [`SLUG_MIN_LEN`].
    #[error("a slug must be at least {} characters long", SLUG_MIN_LEN)]
    TooShort,
    /// The string is longer than [`SLUG_MAX_LEN`].
    #[error("a slug must be at most {} characters long", SLUG_MAX_LEN)]
    TooLong,
    /// The first character is not a lowercase letter.
    #[error("a slug must start with a lowercase letter")]
    BadStart,
    /// The last character is neither a lowercase letter nor a digit.
    #[error("a slug must end with a lowercase letter or a digit")]
    BadEnd,
    /// Some character is outside the accepted set.
    #[error("`{0}` is not allowed in a slug: use lowercase letters, digits and hyphens")]
    BadChar(char),
    /// Two hyphens in a row, which reads as a typo and sorts badly.
    #[error("a slug must not contain two hyphens in a row")]
    DoubleHyphen,
}

impl Slug {
    /// Validates `input` and wraps it.
    pub fn parse(input: &str) -> Result<Self, SlugError> {
        if input.len() < SLUG_MIN_LEN {
            return Err(SlugError::TooShort);
        }
        if input.len() > SLUG_MAX_LEN {
            return Err(SlugError::TooLong);
        }

        let mut previous_was_hyphen = false;
        for character in input.chars() {
            match character {
                'a'..='z' | '0'..='9' => previous_was_hyphen = false,
                '-' if previous_was_hyphen => return Err(SlugError::DoubleHyphen),
                '-' => previous_was_hyphen = true,
                other => return Err(SlugError::BadChar(other)),
            }
        }

        // Both ends are checked after the character set, so that an uppercase
        // first letter is reported as the wrong character and not as a bad
        // start: the two mistakes call for different corrections.
        if !input.starts_with(|c: char| c.is_ascii_lowercase()) {
            return Err(SlugError::BadStart);
        }
        if input.ends_with('-') {
            return Err(SlugError::BadEnd);
        }

        Ok(Self(input.to_owned()))
    }

    /// The slug as a string, without any identifier prefix.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Slug {
    type Err = SlugError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

/// The kind of entity an identifier names, and the prefix that announces it.
pub trait IdKind: Clone + Copy + Eq + Ord + std::hash::Hash {
    /// The prefix, without the colon.
    const PREFIX: &'static str;
}

/// Why a string was rejected as an identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    /// The string carries no prefix at all.
    #[error("expected an identifier of the form `{expected}:...`")]
    MissingPrefix {
        /// The prefix the target type requires.
        expected: &'static str,
    },
    /// The string names a different kind of entity.
    #[error("expected a `{expected}:` identifier, found `{found}:`")]
    WrongPrefix {
        /// The prefix the target type requires.
        expected: &'static str,
        /// The prefix the string actually carried.
        found: String,
    },
    /// The part after the prefix is not a valid slug.
    #[error(transparent)]
    Slug(#[from] SlugError),
    /// The part after the prefix is not a valid ULID.
    #[error("`{0}` is not a valid unique identifier")]
    Ulid(String),
}

/// Splits `input` into its prefix and its body, checking the prefix.
fn split_prefix<'a>(input: &'a str, expected: &'static str) -> Result<&'a str, IdError> {
    let (prefix, body) = input
        .split_once(':')
        .ok_or(IdError::MissingPrefix { expected })?;

    if prefix != expected {
        return Err(IdError::WrongPrefix {
            expected,
            found: prefix.to_owned(),
        });
    }

    Ok(body)
}

/// An identifier whose body is a slug a person chose.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamedId<K: IdKind> {
    slug: Slug,
    kind: PhantomData<K>,
}

impl<K: IdKind> NamedId<K> {
    /// Builds an identifier from an already validated slug.
    pub fn new(slug: Slug) -> Self {
        Self {
            slug,
            kind: PhantomData,
        }
    }

    /// Validates `input` as a bare slug, without the prefix.
    pub fn from_slug_str(input: &str) -> Result<Self, IdError> {
        Ok(Self::new(Slug::parse(input)?))
    }

    /// The slug, without the prefix.
    pub fn slug(&self) -> &Slug {
        &self.slug
    }
}

impl<K: IdKind> fmt::Display for NamedId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", K::PREFIX, self.slug)
    }
}

impl<K: IdKind> fmt::Debug for NamedId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl<K: IdKind> FromStr for NamedId<K> {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::from_slug_str(split_prefix(input, K::PREFIX)?)
    }
}

/// An identifier whose body is a ULID the machine generated.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UniqueId<K: IdKind> {
    ulid: Ulid,
    kind: PhantomData<K>,
}

impl<K: IdKind> UniqueId<K> {
    /// Generates a fresh identifier.
    ///
    /// ULIDs sort by creation time, so identifiers of the same kind order
    /// themselves without a separate column.
    pub fn generate() -> Self {
        Self::from_ulid(Ulid::new())
    }

    /// Builds an identifier from an existing ULID.
    pub fn from_ulid(ulid: Ulid) -> Self {
        Self {
            ulid,
            kind: PhantomData,
        }
    }

    /// The ULID, without the prefix.
    pub fn ulid(&self) -> Ulid {
        self.ulid
    }
}

impl<K: IdKind> fmt::Display for UniqueId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", K::PREFIX, self.ulid)
    }
}

impl<K: IdKind> fmt::Debug for UniqueId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl<K: IdKind> FromStr for UniqueId<K> {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let body = split_prefix(input, K::PREFIX)?;
        let ulid = Ulid::from_string(body).map_err(|_| IdError::Ulid(body.to_owned()))?;
        Ok(Self::from_ulid(ulid))
    }
}

/// Serialises an identifier as the single string it is displayed as.
macro_rules! impl_serde_via_string {
    ($type:ident, $expecting:literal) => {
        impl<K: IdKind> Serialize for $type<K> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de, K: IdKind> Deserialize<'de> for $type<K> {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct IdVisitor<K: IdKind>(PhantomData<K>);

                impl<K: IdKind> Visitor<'_> for IdVisitor<K> {
                    type Value = $type<K>;

                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(f, concat!($expecting, " of the form `{}:...`"), K::PREFIX)
                    }

                    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                        value.parse().map_err(de::Error::custom)
                    }
                }

                deserializer.deserialize_str(IdVisitor(PhantomData))
            }
        }
    };
}

impl_serde_via_string!(NamedId, "an identifier");
impl_serde_via_string!(UniqueId, "a unique identifier");

impl Serialize for Slug {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Slug {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(de::Error::custom)
    }
}

/// Declares the marker types and the aliases for every kind of identifier.
macro_rules! id_kinds {
    (
        $(
            $(#[$attribute:meta])*
            $marker:ident : $prefix:literal => $alias:ident as $family:ident
        ),* $(,)?
    ) => {
        $(
            $(#[$attribute])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub struct $marker;

            impl IdKind for $marker {
                const PREFIX: &'static str = $prefix;
            }

            $(#[$attribute])*
            pub type $alias = $family<$marker>;
        )*
    };
}

id_kinds! {
    /// An account on a server, global to it and never scoped to a group.
    User: "usr" => UserId as NamedId,
    /// A research group.
    Group: "grp" => GroupId as NamedId,
    /// A line of work, the atomic unit the platform is built around.
    Line: "lin" => LineId as UniqueId,
    /// A statement, definition or black box inside a line.
    Node: "nod" => NodeId as UniqueId,
    /// One block of a line's source: prose, a statement or a proof.
    Block: "blk" => BlockId as UniqueId,
    /// A single-use link into a group.
    Invitation: "inv" => InvitationId as UniqueId,
    /// Somebody asking for a group to be created.
    GroupRequest: "req" => GroupRequestId as UniqueId,
    /// A browser's hold on an account.
    Session: "ses" => SessionId as UniqueId,
    /// A machine registered against the server to run jobs.
    Machine: "mch" => MachineId as UniqueId,
    /// A unit of work handed to a machine.
    Job: "job" => JobId as UniqueId,
    /// A change offered for review, by a person or by an agent.
    Proposal: "prp" => ProposalId as UniqueId,
    /// One machine's hold on a line while it writes it.
    Claim: "clm" => ClaimId as UniqueId,
    /// A proved finding, such as a contradiction between two statements.
    Finding: "fnd" => FindingId as UniqueId,
    /// An immutable definition admitted to a group's vault.
    VaultEntry: "vlt" => VaultEntryId as UniqueId,
    /// A proved licence to substitute one vault definition for another.
    Bridge: "brg" => BridgeId as UniqueId,
    /// A permanent record of somebody checking prose against formalization.
    Audit: "aud" => AuditId as UniqueId,
    /// A person's answer to a downstream audit obligation.
    Decision: "dcs" => DecisionId as UniqueId,
    /// A counted request for somebody to answer affected downstream work.
    Obligation: "obl" => ObligationId as UniqueId,
    /// A build output stored outside git and keyed by commit.
    Artefact: "art" => ArtefactId as UniqueId,
    /// A revocable instance-level permission held by an account.
    Grant: "gnt" => GrantId as UniqueId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_slugs() {
        for input in ["enric", "ana-b", "grupo-algebra-2", "ab"] {
            assert!(Slug::parse(input).is_ok(), "rejected `{input}`");
        }
    }

    #[test]
    fn rejects_malformed_slugs() {
        let cases = [
            ("a", SlugError::TooShort),
            (&"a".repeat(SLUG_MAX_LEN + 1), SlugError::TooLong),
            ("2fast", SlugError::BadStart),
            ("-lead", SlugError::BadStart),
            ("trail-", SlugError::BadEnd),
            ("Enric", SlugError::BadChar('E')),
            ("with space", SlugError::BadChar(' ')),
            ("double--hyphen", SlugError::DoubleHyphen),
        ];

        for (input, expected) in cases {
            assert_eq!(Slug::parse(input), Err(expected), "for `{input}`");
        }
    }

    #[test]
    fn named_identifiers_round_trip() {
        let id: UserId = "usr:enric".parse().unwrap();
        assert_eq!(id.slug().as_str(), "enric");
        assert_eq!(id.to_string(), "usr:enric");
    }

    #[test]
    fn unique_identifiers_round_trip() {
        let id = JobId::generate();
        let parsed: JobId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
        assert!(id.to_string().starts_with("job:"));
    }

    #[test]
    fn a_prefix_of_another_kind_is_rejected() {
        let error = "grp:algebra".parse::<UserId>().unwrap_err();
        assert_eq!(
            error,
            IdError::WrongPrefix {
                expected: "usr",
                found: "grp".to_owned(),
            }
        );
    }

    #[test]
    fn a_bare_slug_is_not_an_identifier() {
        let error = "enric".parse::<UserId>().unwrap_err();
        assert_eq!(error, IdError::MissingPrefix { expected: "usr" });
    }

    #[test]
    fn a_unique_identifier_rejects_a_body_that_is_not_a_ulid() {
        let error = "job:not-a-ulid".parse::<JobId>().unwrap_err();
        assert_eq!(error, IdError::Ulid("not-a-ulid".to_owned()));
    }
}

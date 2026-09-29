//! Types that are written down as one fixed piece of text.

/// Implements `as_str` and `Display` for a fieldless enum, spelling each
/// variant as given. With an error type and message it also implements
/// `FromStr`, ignoring surrounding whitespace; the message may use `{0}`
/// for the rejected input.
///
/// The spellings are what goes on the wire, in the database and in the
/// interface, so they must match the enum's serde names.
macro_rules! spelled {
    ($type:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        impl $type {
            /// The word it is stored, sent and shown as.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $word),+
                }
            }
        }

        impl std::fmt::Display for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };

    ($type:ident, $error:ident($message:literal) { $($variant:ident => $word:literal),+ $(,)? }) => {
        spelled!($type { $($variant => $word),+ });

        #[doc = concat!("A string that names no [`", stringify!($type), "`].")]
        #[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
        #[error($message)]
        pub struct $error(pub String);

        impl std::str::FromStr for $type {
            type Err = $error;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                match input.trim() {
                    $($word => Ok(Self::$variant),)+
                    other => Err($error(other.to_owned())),
                }
            }
        }
    };
}

/// Implements `Serialize` and `Deserialize` for a type through its
/// `Display` and `FromStr`, so that it travels as the one string it is
/// shown as and nothing that fails to parse is ever read back.
macro_rules! serde_as_text {
    ($type:ty) => {
        impl serde::Serialize for $type {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $type {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = <String as serde::Deserialize>::deserialize(deserializer)?;
                raw.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use crate::access::{Grant, GroupRole, WhoWrites};
    use crate::artefact::Kind;
    use crate::document::BlockKind;
    use crate::job::JobState;
    use crate::machine::{Availability, Presence};
    use crate::model::Verification;
    use crate::proposal::Verdict;
    use crate::reconcile::Keep;
    use crate::repository::Side;
    use crate::vault::{AuditTarget, AuditVerb, BridgeKind};

    /// Checks that every value is spelled as serde names it.
    fn agree<T: Serialize + std::fmt::Display>(values: &[T]) {
        for value in values {
            assert_eq!(
                serde_json::to_value(value).unwrap(),
                value.to_string(),
                "`{value}` is spelled one way and sent another"
            );
        }
    }

    #[test]
    fn every_spelling_is_the_name_it_travels_under() {
        agree(&[GroupRole::Member, GroupRole::Admin, GroupRole::Owner]);
        agree(&[Grant::Maintainer]);
        agree(&[WhoWrites::Author, WhoWrites::Anyone]);
        agree(&[Kind::Nodes]);
        agree(&[
            BlockKind::Prose,
            BlockKind::Definition,
            BlockKind::Theorem,
            BlockKind::Proof,
        ]);
        agree(&[
            JobState::Queued,
            JobState::Running,
            JobState::Done,
            JobState::Withdrawn,
        ]);
        agree(&[Presence::Awake, Presence::Asleep]);
        agree(&[
            Availability::Ready,
            Availability::NotInstalled,
            Availability::NotSignedIn,
            Availability::RateLimited,
        ]);
        agree(&[
            Verification::Unverified,
            Verification::Verified,
            Verification::Refuted,
        ]);
        agree(&[Verdict::Accept, Verdict::Reject, Verdict::AskAgain]);
        agree(&[Keep::Here, Keep::There]);
        agree(&[Side::Shared, Side::Private]);
        agree(&[BridgeKind::Predicate, BridgeKind::Structure]);
        agree(&[AuditTarget::Entry, AuditTarget::Bridge]);
        agree(&[AuditVerb::Attest, AuditVerb::Withdraw]);
    }

    #[test]
    fn a_spelling_is_read_back_whatever_surrounds_it() {
        assert_eq!(" admin\n".parse(), Ok(GroupRole::Admin));
        assert!("Admin".parse::<GroupRole>().is_err(), "spellings are exact");
    }
}

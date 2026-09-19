//! Roles and grants.
//!
//! A role is held inside a group, because a researcher collaborates with
//! several groups and is one person. Nothing at the instance level is a field
//! on a profile: operating the instance is a [`Grant`], which is given, can be
//! revoked, and leaves a history behind.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::id::UserId;

/// What somebody may do inside one group.
///
/// The variants are ordered by authority, so `Owner > Admin > Member`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupRole {
    /// Reads and writes the group's mathematics.
    Member,
    /// Also invites people and manages the group's settings.
    Admin,
    /// Also transfers or dissolves the group. Every group has one.
    Owner,
}

impl GroupRole {
    /// Whether this role carries everything `required` carries.
    pub fn is_at_least(self, required: Self) -> bool {
        self >= required
    }

    /// Whether the role may invite people into the group.
    pub fn can_invite(self) -> bool {
        self.is_at_least(Self::Admin)
    }

    /// The role as it appears on the wire and in the interface.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

impl fmt::Display for GroupRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A string that names no role.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a group role: use `member`, `admin` or `owner`")]
pub struct UnknownGroupRole(pub String);

impl FromStr for GroupRole {
    type Err = UnknownGroupRole;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "member" => Ok(Self::Member),
            "admin" => Ok(Self::Admin),
            "owner" => Ok(Self::Owner),
            other => Err(UnknownGroupRole(other.to_owned())),
        }
    }
}

/// A permission held at the instance level rather than inside a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    /// Approves group requests, suspends accounts and operates the instance.
    ///
    /// It conveys no access to the mathematics of a group the holder does not
    /// belong to, and the areas it opens ask for the password again.
    Maintainer,
}

impl Grant {
    /// The grant as it appears on the wire and in the interface.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Maintainer => "maintainer",
        }
    }
}

impl fmt::Display for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who may write a line the group can see.
///
/// A line somebody shares stays theirs to write unless they say
/// otherwise; the alternative is not a free-for-all either, because a
/// line is written by one machine at a time and the claim says which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhoWrites {
    /// Its author. Everybody else proposes.
    Author,
    /// Anybody in the group, one at a time by claim.
    Anyone,
}

impl WhoWrites {
    /// The word for it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Author => "author",
            Self::Anyone => "anyone",
        }
    }

    /// What it says on screen.
    pub fn said(self) -> &'static str {
        match self {
            Self::Author => "only its author, and everybody else proposes",
            Self::Anyone => "anybody in the group, one at a time",
        }
    }

    /// Whether this person may write the line themselves.
    pub fn admits(self, author: &UserId, who: &UserId) -> bool {
        match self {
            Self::Author => author == who,
            Self::Anyone => true,
        }
    }
}

impl fmt::Display for WhoWrites {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A string that names nobody who could write a line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` does not say who writes a line: use `author` or `anyone`")]
pub struct UnknownWriter(pub String);

impl FromStr for WhoWrites {
    type Err = UnknownWriter;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.trim() {
            "author" => Ok(Self::Author),
            "anyone" => Ok(Self::Anyone),
            other => Err(UnknownWriter(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_order_by_authority() {
        assert!(GroupRole::Owner > GroupRole::Admin);
        assert!(GroupRole::Admin > GroupRole::Member);
        assert!(GroupRole::Owner.is_at_least(GroupRole::Member));
        assert!(!GroupRole::Member.is_at_least(GroupRole::Admin));
    }

    #[test]
    fn only_admins_and_owners_invite() {
        assert!(!GroupRole::Member.can_invite());
        assert!(GroupRole::Admin.can_invite());
        assert!(GroupRole::Owner.can_invite());
    }

    #[test]
    fn a_line_its_author_keeps_admits_only_them() {
        let ana: UserId = "usr:ana".parse().unwrap();
        let bruno: UserId = "usr:bruno".parse().unwrap();

        assert!(WhoWrites::Author.admits(&ana, &ana));
        assert!(!WhoWrites::Author.admits(&ana, &bruno));
        assert!(WhoWrites::Anyone.admits(&ana, &bruno));

        for who in [WhoWrites::Author, WhoWrites::Anyone] {
            assert_eq!(who.as_str().parse::<WhoWrites>().unwrap(), who);
        }
    }

    #[test]
    fn roles_round_trip_through_their_names() {
        for role in [GroupRole::Member, GroupRole::Admin, GroupRole::Owner] {
            assert_eq!(role.as_str().parse::<GroupRole>().unwrap(), role);
        }
    }
}

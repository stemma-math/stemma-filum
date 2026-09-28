//! Roles and grants.
//!
//! A role is held inside a group, because a researcher collaborates with
//! several groups and is one person. Nothing at the instance level is a field
//! on a profile: operating the instance is a [`Grant`], which is given, can be
//! revoked, and leaves a history behind.

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

    /// Whether the role may change the group's details: its name, what it
    /// is for and its picture.
    pub fn may_manage(self) -> bool {
        self.is_at_least(Self::Admin)
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

spelled!(GroupRole, UnknownGroupRole("`{0}` is not a group role: use `member`, `admin` or `owner`") {
    Member => "member",
    Admin => "admin",
    Owner => "owner",
});

spelled!(Grant { Maintainer => "maintainer" });

spelled!(WhoWrites, UnknownWriter("`{0}` does not say who writes a line: use `author` or `anyone`") {
    Author => "author",
    Anyone => "anyone",
});

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
    fn only_admins_and_owners_change_a_group() {
        assert!(!GroupRole::Member.may_manage());
        assert!(GroupRole::Admin.may_manage());
        assert!(GroupRole::Owner.may_manage());
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

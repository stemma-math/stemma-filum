//! Making the repositories a group and its members need.
//!
//! A group that exists without a repository is a group nobody can work
//! in, so the two are made together. Joining one is the same: the person
//! gets their own repository inside that group at the moment they get the
//! membership, rather than the first time they need it.

use anyhow::Result;
use stemma_core::repository::{Manifest, Side};

use crate::git::{self, Repositories};
use crate::store::groups::Group;
use crate::store::users::User;

/// Makes what a group needs.
pub fn for_group(repositories: &Repositories, group: &Group) -> Result<bool> {
    git::ensure(
        &repositories.shared(&group.id),
        &Manifest::new(
            group.id.clone(),
            Side::Shared,
            format!("{} — the group's own repository", group.name),
        ),
    )
}

/// Makes what one member of a group needs.
pub fn for_member(repositories: &Repositories, group: &Group, user: &User) -> Result<bool> {
    git::ensure(
        &repositories.personal(&group.id, &user.id),
        &Manifest::new(
            group.id.clone(),
            Side::Private,
            format!("{} — {} only", group.name, user.display_name),
        ),
    )
}

/// Makes everything a group and one of its members need.
pub fn for_both(repositories: &Repositories, group: &Group, user: &User) -> Result<()> {
    for_group(repositories, group)?;
    for_member(repositories, group, user)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups, test_database, users};
    use stemma_core::id::Slug;

    #[test]
    fn a_group_and_its_owner_both_get_one() {
        let root = std::env::temp_dir().join(format!("stemma-provision-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let repositories = Repositories::new(&root);
        let mut database = test_database();

        let ana = users::create(
            database.connection(),
            &Slug::parse("ana").unwrap(),
            "ana@example.org",
            "Ana",
            "hash",
        )
        .unwrap()
        .unwrap();

        let group = groups::create(
            database.connection_mut(),
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &ana.id,
        )
        .unwrap();

        for_both(&repositories, &group, &ana).unwrap();

        assert!(git::exists(&repositories.shared(&group.id)));
        assert!(git::exists(&repositories.personal(&group.id, &ana.id)));

        // Doing it again is not an error and makes nothing new.
        assert!(!for_group(&repositories, &group).unwrap());
        assert!(!for_member(&repositories, &group, &ana).unwrap());

        std::fs::remove_dir_all(&root).unwrap();
    }
}

//! Whether somebody is in the group an address names.
//!
//! Most of what the instance serves belongs to one group, and the first
//! thing every such page and endpoint asks is the same: which group the
//! address means, and whether the one asking is in it. Somebody outside
//! it is told there is nothing there, exactly as for a group that does
//! not exist: which groups an instance holds, and who is in them, is the
//! groups' own business.

use stemma_core::access::GroupRole;
use stemma_core::id::{Slug, UserId};

use super::{Instance, WebError};
use crate::store::groups::{self, Group};

/// The group a short name names, and the role `who` holds in it.
pub async fn membership(
    instance: &Instance,
    slug: &str,
    who: &UserId,
) -> Result<(Group, GroupRole), WebError> {
    let slug = Slug::parse(slug.trim()).map_err(|_| WebError::NotFound)?;
    let who = who.clone();

    instance
        .read(move |connection| {
            let Some(group) = groups::by_slug(connection, &slug)? else {
                return Ok(None);
            };

            Ok(groups::role_of(connection, &group.id, &who)?.map(|role| (group, role)))
        })
        .await?
        .ok_or(WebError::NotFound)
}

/// The group a short name names, when `who` is in it.
pub async fn group_of(instance: &Instance, slug: &str, who: &UserId) -> Result<Group, WebError> {
    Ok(membership(instance, slug, who).await?.0)
}

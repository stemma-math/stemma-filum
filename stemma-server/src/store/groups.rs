//! Groups, who is in them, and the requests that create them.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::access::GroupRole;
use stemma_core::event::{Event, GroupChange};
use stemma_core::id::{GroupId, GroupRequestId, Slug, UserId};

use super::events::{self, Audience};
use super::{Role, id_at};
use crate::store::users::{self, User};

/// A research group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The group's identifier, `grp:algebra`.
    pub id: GroupId,
    /// What the group calls itself.
    pub name: String,
    /// What the group is for, when it has said.
    pub description: Option<String>,
    /// When it was created.
    pub created_at: String,
    /// When it was archived, if it was.
    pub archived_at: Option<String>,
}

impl Group {
    /// The short name the remote URL and the workspace directory use.
    pub fn slug(&self) -> &Slug {
        self.id.slug()
    }

    /// Reads a group out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: id_at(row, "id")?,
            name: row.get("name")?,
            description: row.get("description")?,
            created_at: row.get("created_at")?,
            archived_at: row.get("archived_at")?,
        })
    }
}

/// A group somebody belongs to, and what they may do in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    /// The group.
    pub group: Group,
    /// The role held in it.
    pub role: GroupRole,
}

/// Somebody in a group, and what they may do in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The person.
    pub user: User,
    /// The role they hold.
    pub role: GroupRole,
    /// When they joined.
    pub joined_at: String,
}

/// The columns every read of a group asks for.
const COLUMNS: &str = "id, slug, name, description, created_at, archived_at";

/// Creates a group with its first owner.
///
/// Both happen together or neither does: a group without an owner is a
/// group nobody can administer.
pub fn create(
    connection: &mut Connection,
    slug: &Slug,
    name: &str,
    owner: &UserId,
) -> Result<Group> {
    let transaction = connection.transaction()?;
    let group = found(&transaction, slug, name, owner)?;
    transaction.commit()?;

    Ok(group)
}

/// Writes a group and its first owner down, inside the caller's
/// transaction.
fn found(connection: &Connection, slug: &Slug, name: &str, owner: &UserId) -> Result<Group> {
    let id = GroupId::new(slug.clone());

    connection
        .execute(
            "INSERT INTO groups (id, slug, name) VALUES (?1, ?2, ?3)",
            params![id.to_string(), slug.as_str(), name.trim()],
        )
        .context("cannot create the group")?;

    connection
        .execute(
            "INSERT INTO memberships (group_id, user_id, role) VALUES (?1, ?2, ?3)",
            params![id.to_string(), owner.to_string(), Role(GroupRole::Owner)],
        )
        .context("cannot make the requester the owner")?;

    tell(connection, &id, GroupChange::Created)?;

    by_id(connection, &id)?.context("the group was created and cannot be read back")
}

/// Says what a group is for.
///
/// An empty description is no description: a group that says nothing about
/// itself should show nothing, not an empty line where a sentence goes.
pub fn describe(connection: &Connection, id: &GroupId, description: &str) -> Result<()> {
    let said = description.trim();

    connection
        .execute(
            "UPDATE groups SET description = ?2 WHERE id = ?1",
            params![id.to_string(), (!said.is_empty()).then_some(said)],
        )
        .context("cannot describe the group")?;

    Ok(())
}

/// Changes what a group calls itself.
///
/// The short name stays: it is in every remote and every directory the
/// group's repositories were cloned into.
pub fn rename(connection: &Connection, id: &GroupId, name: &str) -> Result<()> {
    connection
        .execute(
            "UPDATE groups SET name = ?2 WHERE id = ?1",
            params![id.to_string(), name.trim()],
        )
        .context("cannot rename the group")?;

    Ok(())
}

/// Whether a name is still free.
pub fn is_free(connection: &Connection, slug: &Slug) -> Result<bool> {
    Ok(by_slug(connection, slug)?.is_none())
}

/// The group with this identifier.
pub fn by_id(connection: &Connection, id: &GroupId) -> Result<Option<Group>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM groups WHERE id = ?1"),
            [id.to_string()],
            Group::from_row,
        )
        .optional()
        .context("cannot read the group")
}

/// The group with this name.
pub fn by_slug(connection: &Connection, slug: &Slug) -> Result<Option<Group>> {
    by_id(connection, &GroupId::new(slug.clone()))
}

/// Every group, oldest first.
pub fn all(connection: &Connection) -> Result<Vec<Group>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM groups ORDER BY created_at, id"
    ))?;
    let groups = statement
        .query_map([], Group::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the groups")?;

    Ok(groups)
}

/// The groups somebody belongs to.
pub fn for_user(connection: &Connection, user: &UserId) -> Result<Vec<Membership>> {
    let mut statement = connection.prepare(
        "SELECT g.id, g.slug, g.name, g.description, g.created_at, g.archived_at, m.role
           FROM memberships m JOIN groups g ON g.id = m.group_id
          WHERE m.user_id = ?1
          ORDER BY g.name",
    )?;

    let memberships = statement
        .query_map([user.to_string()], |row| {
            Ok(Membership {
                group: Group::from_row(row)?,
                role: row.get::<_, Role>("role")?.0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the groups")?;

    Ok(memberships)
}

/// What somebody may do in a group, if anything.
pub fn role_of(
    connection: &Connection,
    group: &GroupId,
    user: &UserId,
) -> Result<Option<GroupRole>> {
    let role: Option<Role> = connection
        .query_row(
            "SELECT role FROM memberships WHERE group_id = ?1 AND user_id = ?2",
            params![group.to_string(), user.to_string()],
            |row| row.get(0),
        )
        .optional()
        .context("cannot read the membership")?;

    Ok(role.map(|role| role.0))
}

/// Everybody in a group, those with the most authority first.
pub fn members(connection: &Connection, group: &GroupId) -> Result<Vec<Member>> {
    // Roles are stored as words, and the words do not sort by authority.
    let mut statement = connection.prepare(
        "SELECT u.id, u.slug, u.email, u.display_name, u.created_at, u.suspended_at,
                m.role, m.joined_at
           FROM memberships m JOIN users u ON u.id = m.user_id
          WHERE m.group_id = ?1
          ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END,
                   u.display_name",
    )?;

    let members = statement
        .query_map([group.to_string()], |row| {
            Ok(Member {
                user: User::from_row(row)?,
                role: row.get::<_, Role>("role")?.0,
                joined_at: row.get("joined_at")?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the members")?;

    Ok(members)
}

/// Puts somebody in a group, or leaves their role alone if they are in it.
pub fn add_member(
    connection: &Connection,
    group: &GroupId,
    user: &UserId,
    role: GroupRole,
) -> Result<bool> {
    let added = connection
        .execute(
            "INSERT OR IGNORE INTO memberships (group_id, user_id, role) VALUES (?1, ?2, ?3)",
            params![group.to_string(), user.to_string(), Role(role)],
        )
        .context("cannot add the member")?;

    if added > 0 {
        tell(connection, group, GroupChange::Joined)?;
    }

    Ok(added > 0)
}

/// Says a group changed, to everybody in it now.
fn tell(connection: &Connection, group: &GroupId, change: GroupChange) -> Result<()> {
    events::publish(
        connection,
        Audience::Group(group),
        &Event::Group {
            group: group.slug().as_str().to_owned(),
            change,
        },
    )?;

    Ok(())
}

/// What became of a request for a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A maintainer said yes, and the group exists.
    Approved,
    /// A maintainer said no.
    Declined,
}

impl Outcome {
    /// How the outcome is written down.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Declined => "declined",
        }
    }
}

/// Somebody asking for a group to exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The request's identifier.
    pub id: String,
    /// The name the group would have.
    pub slug: Slug,
    /// What the group would be called.
    pub name: String,
    /// Why it should exist.
    pub reason: String,
    /// Who asked.
    pub requested_by: UserId,
    /// When they asked.
    pub requested_at: String,
    /// What was decided, if anything has been.
    pub outcome: Option<Outcome>,
    /// When it was decided.
    pub decided_at: Option<String>,
    /// Who decided.
    pub decided_by: Option<UserId>,
    /// What they said about it.
    pub note: Option<String>,
}

impl Request {
    /// Whether the request is still waiting for somebody.
    pub fn is_open(&self) -> bool {
        self.outcome.is_none()
    }

    /// Reads a request out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            slug: id_at::<GroupId>(row, "group_id")?.slug().clone(),
            name: row.get("name")?,
            reason: row.get("reason")?,
            requested_by: id_at(row, "requested_by")?,
            requested_at: row.get("requested_at")?,
            outcome: match row.get::<_, Option<String>>("outcome")?.as_deref() {
                Some("approved") => Some(Outcome::Approved),
                Some("declined") => Some(Outcome::Declined),
                _ => None,
            },
            decided_at: row.get("decided_at")?,
            decided_by: row
                .get::<_, Option<String>>("decided_by")?
                .and_then(|raw| raw.parse().ok()),
            note: row.get("note")?,
        })
    }
}

/// The columns a request is read through.
///
/// The slug is read back as a full identifier so that the same parsing rule
/// applies to it as to everything else.
const REQUEST_COLUMNS: &str = "id, 'grp:' || slug AS group_id, name, reason, requested_by,
     requested_at, outcome, decided_at, decided_by, note";

/// Records somebody asking for a group.
pub fn request(
    connection: &Connection,
    slug: &Slug,
    name: &str,
    reason: &str,
    by: &UserId,
) -> Result<Request> {
    let id = GroupRequestId::generate().to_string();

    connection
        .execute(
            "INSERT INTO group_requests (id, slug, name, reason, requested_by)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                slug.as_str(),
                name.trim(),
                reason.trim(),
                by.to_string()
            ],
        )
        .context("cannot record the request")?;

    request_by_id(connection, &id)?.context("the request was recorded and cannot be read back")
}

/// One request.
pub fn request_by_id(connection: &Connection, id: &str) -> Result<Option<Request>> {
    connection
        .query_row(
            &format!("SELECT {REQUEST_COLUMNS} FROM group_requests WHERE id = ?1"),
            [id],
            Request::from_row,
        )
        .optional()
        .context("cannot read the request")
}

/// Every request nobody has answered yet, oldest first.
pub fn open_requests(connection: &Connection) -> Result<Vec<Request>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {REQUEST_COLUMNS} FROM group_requests
          WHERE outcome IS NULL ORDER BY requested_at"
    ))?;

    let requests = statement
        .query_map([], Request::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the requests")?;

    Ok(requests)
}

/// Somebody's own requests, newest first.
pub fn requests_by(connection: &Connection, user: &UserId) -> Result<Vec<Request>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {REQUEST_COLUMNS} FROM group_requests
          WHERE requested_by = ?1 ORDER BY requested_at DESC"
    ))?;

    let requests = statement
        .query_map([user.to_string()], Request::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the requests")?;

    Ok(requests)
}

/// Approves a request, creating the group and making the requester its
/// owner.
///
/// The decision and the group are written together, so a request is never
/// approved without its group nor a group made from a request that had
/// already been answered.
pub fn approve(
    connection: &mut Connection,
    request: &Request,
    maintainer: &UserId,
    note: Option<&str>,
) -> Result<Group> {
    let transaction = connection.transaction()?;

    if !decide(
        &transaction,
        &request.id,
        Outcome::Approved,
        maintainer,
        note,
    )? {
        bail!("the request has already been answered");
    }

    let group = found(
        &transaction,
        &request.slug,
        &request.name,
        &request.requested_by,
    )?;
    describe(&transaction, &group.id, &request.reason)?;
    let group = by_id(&transaction, &group.id)?
        .context("the group was described and cannot be read back")?;
    transaction.commit()?;

    Ok(group)
}

/// Declines a request, saying who decided and why. Says whether it was
/// still open to decline.
pub fn decline(
    connection: &Connection,
    request: &Request,
    maintainer: &UserId,
    note: Option<&str>,
) -> Result<bool> {
    decide(connection, &request.id, Outcome::Declined, maintainer, note)
}

/// Writes the decision down, unless one already was.
fn decide(
    connection: &Connection,
    id: &str,
    outcome: Outcome,
    maintainer: &UserId,
    note: Option<&str>,
) -> Result<bool> {
    let decided = connection
        .execute(
            "UPDATE group_requests
                SET outcome = ?2, decided_at = datetime('now'), decided_by = ?3, note = ?4
              WHERE id = ?1 AND outcome IS NULL",
            params![id, outcome.as_str(), maintainer.to_string(), note],
        )
        .context("cannot record the decision")?;

    Ok(decided > 0)
}

/// Somebody's account, for the screens that show who asked for what.
pub fn requester(connection: &Connection, request: &Request) -> Result<Option<User>> {
    users::by_id(connection, &request.requested_by)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_database;

    fn slug(text: &str) -> Slug {
        Slug::parse(text).unwrap()
    }

    fn account(connection: &Connection, name: &str) -> User {
        users::create(
            connection,
            &slug(name),
            &format!("{name}@example.org"),
            name,
            "hash",
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn a_group_is_created_with_its_owner() {
        let mut database = test_database();
        let ana = account(database.connection(), "ana");

        let group = create(
            database.connection_mut(),
            &slug("algebra"),
            "Algebra",
            &ana.id,
        )
        .unwrap();

        assert_eq!(group.id.to_string(), "grp:algebra");
        assert_eq!(
            role_of(database.connection(), &group.id, &ana.id).unwrap(),
            Some(GroupRole::Owner)
        );
        assert_eq!(
            for_user(database.connection(), &ana.id).unwrap(),
            vec![Membership {
                group,
                role: GroupRole::Owner
            }]
        );
    }

    #[test]
    fn somebody_outside_a_group_holds_no_role_in_it() {
        let mut database = test_database();
        let ana = account(database.connection(), "ana");
        let enric = account(database.connection(), "enric");
        let group = create(
            database.connection_mut(),
            &slug("algebra"),
            "Algebra",
            &ana.id,
        )
        .unwrap();

        assert_eq!(
            role_of(database.connection(), &group.id, &enric.id).unwrap(),
            None
        );
        assert!(
            for_user(database.connection(), &enric.id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_approved_request_becomes_a_group_owned_by_whoever_asked() {
        let mut database = test_database();
        let ana = account(database.connection(), "ana");
        let root = account(database.connection(), "root-user");

        let asked = request(
            database.connection(),
            &slug("algebra"),
            "Algebra",
            "We are six people working on it.",
            &ana.id,
        )
        .unwrap();

        assert!(asked.is_open());
        assert_eq!(open_requests(database.connection()).unwrap().len(), 1);

        let group = approve(database.connection_mut(), &asked, &root.id, None).unwrap();

        assert_eq!(
            role_of(database.connection(), &group.id, &ana.id).unwrap(),
            Some(GroupRole::Owner)
        );
        assert!(open_requests(database.connection()).unwrap().is_empty());

        let answered = request_by_id(database.connection(), &asked.id)
            .unwrap()
            .unwrap();
        assert_eq!(answered.outcome, Some(Outcome::Approved));
        assert_eq!(answered.decided_by, Some(root.id));
    }

    #[test]
    fn a_declined_request_stays_in_the_record_with_its_reason() {
        let mut database = test_database();
        let ana = account(database.connection(), "ana");
        let root = account(database.connection(), "root-user");

        let asked = request(
            database.connection(),
            &slug("algebra"),
            "Algebra",
            "Because.",
            &ana.id,
        )
        .unwrap();

        decline(
            database.connection(),
            &asked,
            &root.id,
            Some("There is already a group for this."),
        )
        .unwrap();

        let answered = request_by_id(database.connection(), &asked.id)
            .unwrap()
            .unwrap();

        assert_eq!(answered.outcome, Some(Outcome::Declined));
        assert_eq!(
            answered.note.as_deref(),
            Some("There is already a group for this.")
        );
        assert!(
            by_slug(database.connection(), &slug("algebra"))
                .unwrap()
                .is_none()
        );
        let _ = &mut database;
    }

    #[test]
    fn members_are_listed_with_the_most_authority_first() {
        let mut database = test_database();
        let ana = account(database.connection(), "ana");
        let enric = account(database.connection(), "enric");
        let group = create(
            database.connection_mut(),
            &slug("algebra"),
            "Algebra",
            &ana.id,
        )
        .unwrap();

        assert!(
            add_member(
                database.connection(),
                &group.id,
                &enric.id,
                GroupRole::Member
            )
            .unwrap()
        );
        assert!(
            !add_member(
                database.connection(),
                &group.id,
                &enric.id,
                GroupRole::Admin
            )
            .unwrap()
        );

        let members = members(database.connection(), &group.id).unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].role, GroupRole::Owner);
        assert_eq!(members[1].role, GroupRole::Member);
    }

    #[test]
    fn an_admin_is_listed_before_a_member_and_after_the_owner() {
        let mut database = test_database();
        let ana = account(database.connection(), "ana");
        let bo = account(database.connection(), "bo");
        let cai = account(database.connection(), "cai");
        let group = create(
            database.connection_mut(),
            &slug("algebra"),
            "Algebra",
            &ana.id,
        )
        .unwrap();

        // Named so that the names alone would put the member first.
        add_member(database.connection(), &group.id, &bo.id, GroupRole::Member).unwrap();
        add_member(database.connection(), &group.id, &cai.id, GroupRole::Admin).unwrap();

        let roles: Vec<_> = members(database.connection(), &group.id)
            .unwrap()
            .into_iter()
            .map(|member| member.role)
            .collect();
        assert_eq!(
            roles,
            vec![GroupRole::Owner, GroupRole::Admin, GroupRole::Member]
        );
    }

    #[test]
    fn a_request_is_answered_once() {
        let mut database = test_database();
        let ana = account(database.connection(), "ana");
        let root = account(database.connection(), "root-user");
        let asked = request(
            database.connection(),
            &slug("algebra"),
            "Algebra",
            "Rings.",
            &ana.id,
        )
        .unwrap();

        assert!(decline(database.connection(), &asked, &root.id, None).unwrap());
        assert!(!decline(database.connection(), &asked, &root.id, None).unwrap());
        assert!(approve(database.connection_mut(), &asked, &root.id, None).is_err());
        assert!(
            by_slug(database.connection(), &slug("algebra"))
                .unwrap()
                .is_none(),
            "a declined request made a group"
        );
    }

    #[test]
    fn a_request_for_a_name_taken_since_leaves_the_request_open() {
        let mut database = test_database();
        let ana = account(database.connection(), "ana");
        let bo = account(database.connection(), "bo");
        let root = account(database.connection(), "root-user");
        let first = request(
            database.connection(),
            &slug("algebra"),
            "A",
            "One.",
            &ana.id,
        )
        .unwrap();
        let second = request(database.connection(), &slug("algebra"), "B", "Two.", &bo.id).unwrap();

        approve(database.connection_mut(), &first, &root.id, None).unwrap();
        assert!(approve(database.connection_mut(), &second, &root.id, None).is_err());

        let second = request_by_id(database.connection(), &second.id)
            .unwrap()
            .unwrap();
        assert!(second.is_open(), "the failed approval was recorded");
    }

    #[test]
    fn a_renamed_group_keeps_its_short_name() {
        let mut database = test_database();
        let ana = account(database.connection(), "ana");
        let group = create(
            database.connection_mut(),
            &slug("algebra"),
            "Algebra",
            &ana.id,
        )
        .unwrap();

        rename(database.connection(), &group.id, "  Commutative algebra ").unwrap();

        let renamed = by_slug(database.connection(), &slug("algebra"))
            .unwrap()
            .unwrap();
        assert_eq!(renamed.name, "Commutative algebra");
        assert_eq!(renamed.id, group.id);
    }
}

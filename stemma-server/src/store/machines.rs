//! The machines registered against this instance.
//!
//! A machine is its credential: it registers once with the one it
//! collected from `stemma login`, and taking the machine back takes that
//! credential with it, without signing its owner out of anything else.
//!
//! What a machine can do is what it reported finding, never what somebody
//! typed into a form. An agent it does not have is kept here too, with
//! the reason, because the owner can install it and an absence with a
//! reason is worth more than a shorter list.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use stemma_core::id::{GroupId, MachineId, Slug, UserId};
use stemma_core::machine::{ASLEEP_AFTER_SECONDS, Availability, Detected, Presence};

use super::id_at;

/// One machine, as the instance knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    /// Its identifier.
    pub id: MachineId,
    /// Whose machine it is.
    pub owner: UserId,
    /// What it calls itself, which is what its owner reads in a list.
    pub label: String,
    /// When it first registered.
    pub registered_at: String,
    /// When it last said it was there.
    pub last_seen_at: Option<String>,
    /// Whether it is there now.
    pub presence: Presence,
    /// When its owner took it back, if they have.
    pub retired_at: Option<String>,
}

impl Machine {
    /// Reads one out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let awake: bool = row.get("awake")?;

        Ok(Self {
            id: id_at(row, "id")?,
            owner: id_at(row, "user_id")?,
            label: row.get("label")?,
            registered_at: row.get("registered_at")?,
            last_seen_at: row.get("last_seen_at")?,
            presence: match awake {
                true => Presence::Awake,
                false => Presence::Asleep,
            },
            retired_at: row.get("retired_at")?,
        })
    }

    /// Whether it is still somebody's machine.
    pub fn is_live(&self) -> bool {
        self.retired_at.is_none()
    }
}

/// The columns every read asks for.
///
/// Whether a machine is awake is worked out where the clock is, rather
/// than by parsing a timestamp back out afterwards.
fn columns() -> String {
    format!(
        "id, user_id, label, registered_at, last_seen_at, retired_at,
         (last_seen_at IS NOT NULL
          AND last_seen_at > datetime('now', '-{ASLEEP_AFTER_SECONDS} seconds')) AS awake"
    )
}

/// Registers a machine against the credential it is carrying.
///
/// Registering twice with the same credential is the same machine saying
/// so again: it keeps its identifier, and anything that named it still
/// names it.
pub fn register(
    connection: &Connection,
    owner: &UserId,
    token_id: &str,
    label: &str,
) -> Result<Machine> {
    connection
        .execute(
            "INSERT INTO machines (id, user_id, token_id, label, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT (token_id) DO UPDATE
                SET label = ?4, last_seen_at = datetime('now'), retired_at = NULL",
            params![
                MachineId::generate().to_string(),
                owner.to_string(),
                token_id,
                label.trim()
            ],
        )
        .context("cannot register the machine")?;

    let machine = by_token(connection, token_id)?.context("the machine cannot be read back")?;
    tell(connection, &machine.id, None)?;

    Ok(machine)
}

/// Says a machine changed, to its owner and to the groups it serves, who
/// see what it offers. `also` is a group it has just stopped serving.
fn tell(connection: &Connection, machine: &MachineId, also: Option<&GroupId>) -> Result<()> {
    let Some(found) = by_id(connection, machine)? else {
        return Ok(());
    };
    let event = stemma_core::event::Event::Machine {
        machine: machine.to_string(),
    };

    super::events::publish(
        connection,
        super::events::Audience::Alone(&found.owner),
        &event,
    )?;
    for group in groups(connection, machine)?.iter().chain(also) {
        super::events::publish(connection, super::events::Audience::Group(group), &event)?;
    }

    Ok(())
}

/// The machine holding this credential, if one does.
pub fn by_token(connection: &Connection, token_id: &str) -> Result<Option<Machine>> {
    read(connection, "token_id = ?1", params![token_id])
}

/// One machine.
pub fn by_id(connection: &Connection, id: &MachineId) -> Result<Option<Machine>> {
    read(connection, "id = ?1", params![id.to_string()])
}

/// Reads one machine.
fn read(
    connection: &Connection,
    condition: &str,
    parameters: impl rusqlite::Params,
) -> Result<Option<Machine>> {
    connection
        .query_row(
            &format!("SELECT {} FROM machines WHERE {condition}", columns()),
            parameters,
            Machine::from_row,
        )
        .optional()
        .context("cannot read the machine")
}

/// Somebody's machines, the ones they have not taken back.
pub fn for_user(connection: &Connection, owner: &UserId) -> Result<Vec<Machine>> {
    list(
        connection,
        "user_id = ?1 AND retired_at IS NULL",
        params![owner.to_string()],
    )
}

/// The machines that serve a group.
///
/// Both halves have to hold: the owner turned the machine on for this
/// group, and the owner is still in it. Losing a membership stops the
/// machines with it, which is the point of checking here rather than at
/// the moment somebody enabled it.
pub fn for_group(connection: &Connection, group: &GroupId) -> Result<Vec<Machine>> {
    list(
        connection,
        "retired_at IS NULL
         AND id IN (SELECT machine_id FROM machine_groups WHERE group_id = ?1)
         AND user_id IN (SELECT user_id FROM memberships WHERE group_id = ?1)",
        params![group.to_string()],
    )
}

/// Reads several machines.
fn list(
    connection: &Connection,
    condition: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<Machine>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {} FROM machines WHERE {condition} ORDER BY registered_at, id",
        columns()
    ))?;

    let found = statement
        .query_map(parameters, Machine::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the machines")?;

    Ok(found)
}

/// Records that a machine is there.
pub fn beat(connection: &Connection, machine: &MachineId) -> Result<bool> {
    let seen = connection
        .execute(
            "UPDATE machines SET last_seen_at = datetime('now')
              WHERE id = ?1 AND retired_at IS NULL",
            params![machine.to_string()],
        )
        .context("cannot record that the machine is there")?;

    Ok(seen > 0)
}

/// Writes down what a machine found when it looked for its agents.
///
/// What it reports replaces what it reported before. An agent it stopped
/// mentioning is one this build of the client no longer knows about, and
/// keeping the old row would have the instance claiming something about
/// a machine that has not said it.
pub fn report(connection: &mut Connection, machine: &MachineId, found: &[Detected]) -> Result<()> {
    let transaction = connection.transaction()?;

    transaction
        .execute(
            "DELETE FROM machine_agents WHERE machine_id = ?1",
            params![machine.to_string()],
        )
        .context("cannot clear what the machine reported before")?;

    for agent in found {
        transaction
            .execute(
                "INSERT INTO machine_agents (machine_id, agent, version, availability, models, efforts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    machine.to_string(),
                    agent.agent.as_str(),
                    agent.version,
                    agent.availability.as_str(),
                    serde_json::to_string(&agent.models)?,
                    serde_json::to_string(&agent.efforts)?,
                ],
            )
            .context("cannot record what the machine found")?;
    }

    tell(&transaction, machine, None)?;
    transaction.commit()?;

    Ok(())
}

/// What one machine can do.
///
/// Every agent this build knows about is in the answer, whether or not
/// the machine has it: an option that is missing teaches nothing, and an
/// option shown with its reason teaches the owner about their machine.
pub fn agents(connection: &Connection, machine: &MachineId) -> Result<Vec<Detected>> {
    let mut statement = connection
        .prepare("SELECT agent, version, availability, models, efforts FROM machine_agents WHERE machine_id = ?1")?;

    let reported: Vec<(String, Option<String>, String, String, String)> = statement
        .query_map(params![machine.to_string()], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot read what the machine reported")?;

    let found = stemma_core::machine::CATALOGUE
        .iter()
        .map(|known| {
            let said = reported
                .iter()
                .find(|(agent, _, _, _, _)| agent == known.slug);

            match said {
                Some((_, version, availability, models, efforts)) => Detected {
                    agent: known.id(),
                    version: version.clone(),
                    availability: availability.parse().unwrap_or_default(),
                    models: serde_json::from_str(models).unwrap_or_default(),
                    efforts: serde_json::from_str(efforts).unwrap_or_default(),
                },
                // A machine running an older client says nothing about an
                // agent it has never heard of, and cannot run it either.
                None => Detected::missing(known.id()),
            }
        })
        .collect();

    Ok(found)
}

/// Whether a machine can run an agent right now.
pub fn can_run(connection: &Connection, machine: &MachineId, agent: &Slug) -> Result<bool> {
    let availability: Option<String> = connection
        .query_row(
            "SELECT availability FROM machine_agents WHERE machine_id = ?1 AND agent = ?2",
            params![machine.to_string(), agent.as_str()],
            |row| row.get(0),
        )
        .optional()
        .context("cannot read what the machine can run")?;

    Ok(availability
        .and_then(|word| word.parse::<Availability>().ok())
        .is_some_and(Availability::is_usable))
}

/// Turns a machine on for a group.
pub fn enable(connection: &Connection, machine: &MachineId, group: &GroupId) -> Result<()> {
    connection
        .execute(
            "INSERT INTO machine_groups (machine_id, group_id) VALUES (?1, ?2)
             ON CONFLICT (machine_id, group_id) DO NOTHING",
            params![machine.to_string(), group.to_string()],
        )
        .context("cannot turn the machine on for the group")?;

    tell(connection, machine, None)?;

    Ok(())
}

/// Turns it off again.
pub fn disable(connection: &Connection, machine: &MachineId, group: &GroupId) -> Result<()> {
    connection
        .execute(
            "DELETE FROM machine_groups WHERE machine_id = ?1 AND group_id = ?2",
            params![machine.to_string(), group.to_string()],
        )
        .context("cannot turn the machine off for the group")?;

    tell(connection, machine, Some(group))?;

    Ok(())
}

/// The groups a machine serves.
pub fn groups(connection: &Connection, machine: &MachineId) -> Result<Vec<GroupId>> {
    let mut statement = connection.prepare(
        "SELECT group_id FROM machine_groups
          WHERE machine_id = ?1
            AND group_id IN (SELECT group_id FROM memberships
                              WHERE user_id = (SELECT user_id FROM machines WHERE id = ?1))
          ORDER BY group_id",
    )?;

    let found = statement
        .query_map(params![machine.to_string()], |row| id_at(row, "group_id"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("cannot list the machine's groups")?;

    Ok(found)
}

/// Takes a machine back, and its credential with it.
///
/// The row stays: work it did is attributed to it, and an attribution
/// that cannot be resolved is worse than a machine in a list marked as
/// gone.
pub fn retire(connection: &Connection, machine: &MachineId) -> Result<bool> {
    let retired = connection
        .execute(
            "UPDATE machines SET retired_at = datetime('now')
              WHERE id = ?1 AND retired_at IS NULL",
            params![machine.to_string()],
        )
        .context("cannot take the machine back")?;

    if retired > 0 {
        connection
            .execute(
                "UPDATE client_tokens SET revoked_at = datetime('now')
                  WHERE id = (SELECT token_id FROM machines WHERE id = ?1)
                    AND revoked_at IS NULL",
                params![machine.to_string()],
            )
            .context("cannot take the machine's credential back")?;
        tell(connection, machine, None)?;
    }

    Ok(retired > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups as group_store, test_database, users};

    /// An account with a credential, as `stemma login` leaves one.
    fn account(connection: &Connection, name: &str) -> (UserId, String) {
        let user = users::create(
            connection,
            &Slug::parse(name).unwrap(),
            &format!("{name}@example.org"),
            name,
            "hash",
        )
        .unwrap()
        .unwrap()
        .id;

        let token = format!("ses:{name}");
        connection
            .execute(
                "INSERT INTO client_tokens (id, user_id, label, token_hash)
                 VALUES (?1, ?2, 'a machine', ?1)",
                params![token, user.to_string()],
            )
            .unwrap();

        (user, token)
    }

    fn group(connection: &mut Connection, slug: &str, owner: &UserId) -> GroupId {
        group_store::create(connection, &Slug::parse(slug).unwrap(), slug, owner)
            .unwrap()
            .id
    }

    #[test]
    fn a_machine_registers_once_and_keeps_its_name() {
        let database = test_database();
        let connection = database.connection();
        let (ana, token) = account(connection, "ana");

        let first = register(connection, &ana, &token, "ana's laptop").unwrap();
        assert_eq!(first.presence, Presence::Awake);
        assert!(first.is_live());

        let again = register(connection, &ana, &token, "the laptop").unwrap();
        assert_eq!(
            again.id, first.id,
            "registering again made a second machine"
        );
        assert_eq!(again.label, "the laptop");

        assert_eq!(for_user(connection, &ana).unwrap(), vec![again]);
    }

    #[test]
    fn a_machine_that_has_not_spoken_for_a_while_is_asleep() {
        let database = test_database();
        let connection = database.connection();
        let (ana, token) = account(connection, "ana");
        let machine = register(connection, &ana, &token, "a laptop").unwrap();

        connection
            .execute(
                "UPDATE machines SET last_seen_at = datetime('now', '-1 hour')",
                [],
            )
            .unwrap();

        assert_eq!(
            by_id(connection, &machine.id).unwrap().unwrap().presence,
            Presence::Asleep
        );

        assert!(beat(connection, &machine.id).unwrap());
        assert_eq!(
            by_id(connection, &machine.id).unwrap().unwrap().presence,
            Presence::Awake
        );
    }

    #[test]
    fn every_agent_is_listed_even_the_ones_the_machine_does_not_have() {
        let mut database = test_database();
        let (ana, token) = account(database.connection(), "ana");
        let machine = register(database.connection(), &ana, &token, "a laptop").unwrap();

        let claude = Slug::parse("claude").unwrap();
        report(
            database.connection_mut(),
            &machine.id,
            &[Detected::ready(claude.clone(), Some("1.2.3".to_owned()))],
        )
        .unwrap();

        let found = agents(database.connection(), &machine.id).unwrap();

        assert_eq!(found.len(), stemma_core::machine::CATALOGUE.len());
        assert!(
            found.iter().any(|agent| {
                agent.agent == claude && agent.availability == Availability::Ready
            })
        );
        assert!(
            found
                .iter()
                .any(|agent| agent.availability == Availability::NotInstalled),
            "an agent nobody reported was quietly dropped"
        );

        assert!(can_run(database.connection(), &machine.id, &claude).unwrap());
        assert!(
            !can_run(
                database.connection(),
                &machine.id,
                &Slug::parse("codex").unwrap()
            )
            .unwrap()
        );
    }

    #[test]
    fn what_a_machine_reports_replaces_what_it_reported_before() {
        let mut database = test_database();
        let (ana, token) = account(database.connection(), "ana");
        let machine = register(database.connection(), &ana, &token, "a laptop").unwrap();
        let claude = Slug::parse("claude").unwrap();

        report(
            database.connection_mut(),
            &machine.id,
            &[Detected::ready(claude.clone(), None)],
        )
        .unwrap();
        report(
            database.connection_mut(),
            &machine.id,
            &[Detected::missing(claude.clone())],
        )
        .unwrap();

        assert!(!can_run(database.connection(), &machine.id, &claude).unwrap());
    }

    #[test]
    fn a_machine_serves_a_group_only_while_its_owner_is_in_it() {
        let mut database = test_database();
        let (ana, token) = account(database.connection(), "ana");
        let algebra = group(database.connection_mut(), "algebra", &ana);
        let connection = database.connection();
        let machine = register(connection, &ana, &token, "a laptop").unwrap();

        assert!(for_group(connection, &algebra).unwrap().is_empty());

        enable(connection, &machine.id, &algebra).unwrap();
        assert_eq!(
            for_group(connection, &algebra).unwrap(),
            vec![machine.clone()]
        );
        assert_eq!(
            groups(connection, &machine.id).unwrap(),
            vec![algebra.clone()]
        );

        connection
            .execute(
                "DELETE FROM memberships WHERE user_id = ?1",
                params![ana.to_string()],
            )
            .unwrap();

        assert!(
            for_group(connection, &algebra).unwrap().is_empty(),
            "a machine went on serving a group its owner had left"
        );
        assert!(groups(connection, &machine.id).unwrap().is_empty());

        disable(connection, &machine.id, &algebra).unwrap();
    }

    #[test]
    fn taking_a_machine_back_takes_its_credential_with_it() {
        let database = test_database();
        let connection = database.connection();
        let (ana, token) = account(connection, "ana");
        let machine = register(connection, &ana, &token, "a laptop").unwrap();

        assert!(retire(connection, &machine.id).unwrap());
        assert!(!retire(connection, &machine.id).unwrap());

        let revoked: Option<String> = connection
            .query_row(
                "SELECT revoked_at FROM client_tokens WHERE id = ?1",
                params![token],
                |row| row.get(0),
            )
            .unwrap();
        assert!(revoked.is_some(), "the credential still works");

        assert!(for_user(connection, &ana).unwrap().is_empty());
        assert!(!by_id(connection, &machine.id).unwrap().unwrap().is_live());
    }
}

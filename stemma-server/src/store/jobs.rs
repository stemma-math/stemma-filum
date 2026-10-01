//! The queue.
//!
//! The instance computes nothing. It keeps what was asked for, hands it
//! to the machine it was meant for, and records the typed outcome that
//! comes back. Raw output never arrives here at all.
//!
//! Two rules do most of the work. A job that named a machine goes to that
//! machine and to no other, asleep or not, because choosing was
//! deliberate and reassigning it would overrule the person who chose. A
//! job on somebody's own repository goes only to their own machines,
//! because that is where that repository exists.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use stemma_core::event::Event;
use stemma_core::id::{GroupId, JobId, LineId, MachineId, Slug, UserId};
use stemma_core::job::{Choice, JobState, Outcome, Task};
use stemma_core::line::Name;
use stemma_core::machine::ASLEEP_AFTER_SECONDS;
use stemma_core::proposal::Grants;
use stemma_core::repository::Side;

use super::{id_at, optional_id_at};

/// One unit of work, as the instance holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// Its identifier.
    pub id: JobId,
    /// Whose mathematics it is about.
    pub group: GroupId,
    /// Which of the group's two repositories.
    pub side: Side,
    /// Which line of work.
    pub line: Name,
    /// The line, by identifier, where the job knows it.
    pub line_id: Option<LineId>,
    /// The revision of the line it was launched against.
    pub revision: u64,
    /// The other revision, when the job is to bring two of them together.
    pub against: Option<u64>,
    /// What it asks for, and where the answer lands.
    pub task: Task,
    /// Which agent was chosen.
    pub agent: Slug,
    /// A person's request for a custom task.
    pub request: Option<String>,
    /// Explicit grants for a custom task.
    pub grants: Option<Grants>,
    /// Which machine was chosen, or nobody in particular.
    pub choice: Choice,
    /// Who launched it and answers for it.
    pub launched_by: UserId,
    /// When.
    pub launched_at: String,
    /// Where it has got to.
    pub state: JobState,
    /// The machine that took it.
    pub taken_by: Option<MachineId>,
    /// When it was taken.
    pub taken_at: Option<String>,
    /// When it ended.
    pub finished_at: Option<String>,
    /// How it ended.
    pub outcome: Option<Outcome>,
    /// When the machine's owner dealt with what stopped it.
    pub cleared_at: Option<String>,
    /// Inspectable workspace reported by the machine.
    pub workspace: Option<String>,
    /// Most recent progress sentence.
    pub progress: Option<String>,
    /// Whether the launcher asked a running job to stop.
    pub cancel_requested_at: Option<String>,
}

impl Job {
    /// Reads one out of a row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let unreadable = |what: &str, value: &str| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("`{value}` is not {what}").into(),
            )
        };

        let side: String = row.get("side")?;
        let line: String = row.get("line")?;
        let task: String = row.get("task")?;
        let agent: String = row.get("agent")?;
        let state: String = row.get("state")?;
        let outcome: Option<String> = row.get("outcome")?;
        let chosen: Option<String> = row.get("chosen_machine")?;

        Ok(Self {
            id: id_at(row, "id")?,
            group: id_at(row, "group_id")?,
            side: side
                .parse()
                .map_err(|_| unreadable("a repository", &side))?,
            line: Name::parse(&line).map_err(|_| unreadable("a line", &line))?,
            line_id: optional_id_at(row, "line_id")?,
            revision: revision_at(row, "revision")?.unwrap_or_default(),
            against: revision_at(row, "against")?,
            task: task.parse().map_err(|_| unreadable("a task", &task))?,
            agent: Slug::parse(&agent).map_err(|_| unreadable("an agent", &agent))?,
            request: row.get("request")?,
            grants: row
                .get::<_, Option<String>>("grants")?
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            choice: match chosen {
                Some(machine) => Choice::Machine {
                    machine: machine
                        .parse()
                        .map_err(|_| unreadable("a machine", &machine))?,
                },
                None => Choice::Anyone,
            },
            launched_by: id_at(row, "launched_by")?,
            launched_at: row.get("launched_at")?,
            state: state.parse().map_err(|_| unreadable("a state", &state))?,
            taken_by: optional_id_at(row, "taken_by")?,
            taken_at: row.get("taken_at")?,
            finished_at: row.get("finished_at")?,
            outcome: match outcome {
                Some(word) => Some(
                    Outcome::rebuild(
                        &word,
                        row.get("code")?,
                        row.get::<_, Option<String>>("reason")?.unwrap_or_default(),
                    )
                    .ok_or_else(|| unreadable("an outcome", &word))?,
                ),
                None => None,
            },
            cleared_at: row.get("cleared_at")?,
            workspace: row.get("workspace")?,
            progress: row.get("progress")?,
            cancel_requested_at: row.get("cancel_requested_at")?,
        })
    }

    /// Whether this job is still waiting for the machine somebody chose.
    pub fn waits_for_a_machine(&self) -> bool {
        self.state == JobState::Queued && self.choice.is_deliberate()
    }

    /// Whether it ended in something its machine's owner has to fix.
    pub fn owes_its_owner_a_decision(&self) -> bool {
        self.cleared_at.is_none()
            && self
                .outcome
                .as_ref()
                .is_some_and(Outcome::is_the_owners_to_fix)
    }
}

/// A revision, which the table keeps as text.
fn revision_at(row: &Row<'_>, column: &str) -> rusqlite::Result<Option<u64>> {
    Ok(row
        .get::<_, Option<String>>(column)?
        .and_then(|raw| raw.trim().parse().ok()))
}

/// The columns every read asks for.
const COLUMNS: &str = "id, group_id, side, line, line_id, revision, against, task, agent,
                       chosen_machine, launched_by, launched_at, state, taken_by, taken_at,
                       finished_at, outcome, code, reason, cleared_at, request,
                       grants, workspace, progress, cancel_requested_at";

/// What somebody asked for.
#[derive(Debug, Clone)]
pub struct Launch {
    /// Whose mathematics it is about.
    pub group: GroupId,
    /// Which of the group's two repositories.
    pub side: Side,
    /// Which line of work.
    pub line: Name,
    /// The line, by identifier.
    pub line_id: Option<LineId>,
    /// The revision to work against.
    pub revision: u64,
    /// The other revision, for a job that brings two of them together.
    pub against: Option<u64>,
    /// What is being asked.
    pub task: Task,
    /// Which agent.
    pub agent: Slug,
    /// A person's request for a custom task.
    pub request: Option<String>,
    /// Capabilities chosen for a custom task.
    pub grants: Option<Grants>,
    /// Which machine, or nobody in particular.
    pub choice: Choice,
    /// Who is asking.
    pub by: UserId,
}

/// Puts a job in the queue.
pub fn launch(connection: &Connection, asked: &Launch) -> Result<Job> {
    let id = JobId::generate();

    connection
        .execute(
            "INSERT INTO jobs (id, group_id, side, line, revision, against, task, agent,
                               chosen_machine, launched_by, state, line_id, request, grants)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'queued', ?11, ?12, ?13)",
            params![
                id.to_string(),
                asked.group.to_string(),
                asked.side.as_str(),
                asked.line.as_str(),
                asked.revision.to_string(),
                asked.against.map(|against| against.to_string()),
                asked.task.to_string(),
                asked.agent.as_str(),
                asked.choice.machine().map(MachineId::to_string),
                asked.by.to_string(),
                asked.line_id.map(|line| line.to_string()),
                asked.request,
                asked
                    .grants
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
            ],
        )
        .context("cannot add the job to the queue")?;

    tell(connection, &id)?;

    by_id(connection, &id)?.context("the job cannot be read back")
}

/// Says a job changed, to whoever can see its line.
fn tell(connection: &Connection, id: &JobId) -> Result<()> {
    let Some(job) = by_id(connection, id)? else {
        return Ok(());
    };

    super::events::publish(
        connection,
        super::events::Audience::of(job.side, &job.group, &job.launched_by),
        &Event::Job {
            group: job.group.slug().as_str().to_owned(),
            job: job.id.to_string(),
            state: job.state.to_string(),
        },
    )?;

    Ok(())
}

/// One job.
pub fn by_id(connection: &Connection, id: &JobId) -> Result<Option<Job>> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM jobs WHERE id = ?1"),
            params![id.to_string()],
            Job::from_row,
        )
        .optional()
        .context("cannot read the job")
}

/// Hands a machine the next job it may have, and marks it taken.
///
/// A job that named this machine is handed over whatever the machine can
/// run: if the agent turns out not to be there, that is exactly the
/// typed failure its owner needs to see, and it belongs on the machine.
/// A job that named nobody goes only where the agent is ready, because
/// there is no one to tell.
pub fn take_next(
    connection: &mut Connection,
    machine: &MachineId,
    ready: &[Slug],
) -> Result<Option<Job>> {
    let transaction = connection.transaction()?;

    let mut parameters: Vec<String> = vec![machine.to_string()];
    let anyone = match ready.is_empty() {
        true => "0".to_owned(),
        false => {
            let places: Vec<String> = ready
                .iter()
                .enumerate()
                .map(|(at, agent)| {
                    parameters.push(agent.as_str().to_owned());
                    format!("?{}", at + 2)
                })
                .collect();

            format!(
                "chosen_machine IS NULL AND agent IN ({})",
                places.join(", ")
            )
        }
    };

    let found = transaction
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM jobs
                  WHERE state = 'queued'
                    AND (chosen_machine = ?1 OR ({anyone})
                         OR (chosen_machine IS NULL AND task = 'build'))
                    AND group_id IN (SELECT group_id FROM machine_groups
                                      WHERE machine_id = ?1)
                    AND (task = 'build' OR launched_by = (SELECT user_id FROM machines WHERE id = ?1)
                         OR EXISTS (SELECT 1 FROM machine_groups
                                    WHERE machine_id = ?1 AND group_id = jobs.group_id AND shared = 1))
                    AND group_id IN (SELECT group_id FROM memberships
                                      WHERE user_id = (SELECT user_id FROM machines
                                                        WHERE id = ?1))
                    AND (side = 'shared'
                         OR launched_by = (SELECT user_id FROM machines WHERE id = ?1))
                  ORDER BY launched_at, rowid
                  LIMIT 1"
            ),
            params_from_iter(parameters.iter()),
            Job::from_row,
        )
        .optional()
        .context("cannot look for work")?;

    let Some(job) = found else {
        return Ok(None);
    };

    let taken = transaction
        .execute(
            "UPDATE jobs SET state = 'running', taken_by = ?2, taken_at = datetime('now')
              WHERE id = ?1 AND state = 'queued'",
            params![job.id.to_string(), machine.to_string()],
        )
        .context("cannot mark the job as taken")?;

    if taken == 0 {
        return Ok(None);
    }

    tell(&transaction, &job.id)?;
    let job = by_id(&transaction, &job.id)?;
    transaction.commit()?;

    Ok(job)
}

/// Records how a job ended.
///
/// Only the machine that took it may say, and only once: a second report
/// is a machine that lost the answer to the first and is guessing.
pub fn finish(
    connection: &Connection,
    id: &JobId,
    machine: &MachineId,
    outcome: &Outcome,
) -> Result<bool> {
    let recorded = connection
        .execute(
            "UPDATE jobs
                SET state = 'done', finished_at = datetime('now'),
                    outcome = ?3, code = ?4, reason = ?5
              WHERE id = ?1 AND taken_by = ?2 AND state = 'running'",
            params![
                id.to_string(),
                machine.to_string(),
                outcome.as_str(),
                outcome.code(),
                outcome.said(),
            ],
        )
        .context("cannot record how the job ended")?;

    if recorded > 0 {
        tell(connection, id)?;
    }

    Ok(recorded > 0)
}

/// Calls a job back before anything has run it.
pub fn withdraw(connection: &Connection, id: &JobId, by: &UserId) -> Result<bool> {
    let withdrawn = connection
        .execute(
            "UPDATE jobs SET state = 'withdrawn', finished_at = datetime('now')
              WHERE id = ?1 AND launched_by = ?2 AND state = 'queued'",
            params![id.to_string(), by.to_string()],
        )
        .context("cannot call the job back")?;

    if withdrawn > 0 {
        tell(connection, id)?;
    }

    Ok(withdrawn > 0)
}

/// Asks a running job to stop, or withdraws it before it starts.
///
/// A machine that is away never hears the request, so a running job
/// whose machine is away ends here and now, as cancelled. Asking again
/// later ends it the same way once its machine has gone away.
pub fn cancel(connection: &Connection, id: &JobId, by: &UserId) -> Result<bool> {
    let requested = connection.execute(
        "UPDATE jobs
            SET state = CASE WHEN state = 'queued' THEN 'withdrawn' ELSE state END,
                finished_at = CASE WHEN state = 'queued' THEN datetime('now') ELSE finished_at END,
                cancel_requested_at = datetime('now')
          WHERE id = ?1 AND launched_by = ?2 AND state IN ('queued', 'running')
            AND cancel_requested_at IS NULL",
        params![id.to_string(), by.to_string()],
    )? > 0;
    let ended = connection.execute(
        &format!(
            "UPDATE jobs
                SET state = 'done', finished_at = datetime('now'),
                    outcome = 'broke', code = 'cancelled',
                    reason = 'the job was cancelled while the machine running it was away'
              WHERE id = ?1 AND launched_by = ?2 AND state = 'running'
                AND cancel_requested_at IS NOT NULL
                AND NOT EXISTS (
                    SELECT 1 FROM machines
                     WHERE machines.id = jobs.taken_by
                       AND last_seen_at > datetime('now', '-{ASLEEP_AFTER_SECONDS} seconds'))"
        ),
        params![id.to_string(), by.to_string()],
    )? > 0;
    if requested || ended {
        tell(connection, id)?;
    }
    Ok(requested || ended)
}

/// Records an inspectable workspace and progress from the machine holding the job.
pub fn progress(
    connection: &Connection,
    id: &JobId,
    machine: &MachineId,
    workspace: Option<&str>,
    progress: &str,
) -> Result<bool> {
    let changed = connection.execute(
        "UPDATE jobs SET workspace = COALESCE(?3, workspace), progress = ?4
          WHERE id = ?1 AND taken_by = ?2 AND state = 'running'",
        params![id.to_string(), machine.to_string(), workspace, progress],
    )? > 0;
    if changed {
        tell(connection, id)?;
    }
    Ok(changed)
}

/// Records that a machine's owner has dealt with what stopped a job.
///
/// The outcome stays in the record, saying what happened then. What
/// clears is the decision it asked of them, which is the only part that
/// was ever counted; a job that asks none has nothing to clear, so a
/// failure it comes to later is still counted.
pub fn clear(connection: &Connection, id: &JobId, by: &UserId) -> Result<bool> {
    let cleared = connection
        .execute(
            "UPDATE jobs SET cleared_at = datetime('now')
              WHERE id = ?1 AND cleared_at IS NULL
                AND outcome = 'could_not_try' AND code GLOB 'agent_*'
                AND taken_by IN (SELECT id FROM machines WHERE user_id = ?2)",
            params![id.to_string(), by.to_string()],
        )
        .context("cannot record the decision")?;

    if cleared > 0 {
        tell(connection, id)?;
    }

    Ok(cleared > 0)
}

/// How many jobs are waiting for this person to fix one of their machines.
///
/// The only count in the queue, because it is the only thing in it that
/// somebody owes. A job that is queued, running, or that tried and found
/// nothing is never counted.
pub fn owed_by(connection: &Connection, user: &UserId) -> Result<i64> {
    connection
        .query_one(
            "SELECT count(*) FROM jobs
              WHERE outcome = 'could_not_try' AND code GLOB 'agent_*'
                AND cleared_at IS NULL
                AND taken_by IN (SELECT id FROM machines WHERE user_id = ?1)",
            params![user.to_string()],
            |row| row.get(0),
        )
        .context("cannot count what the machines owe")
}

/// The jobs in a group, as one member may see them.
///
/// Work on somebody's own repository is theirs: it is listed for them and
/// for nobody else, because the repository it is about is one nobody else
/// can clone.
pub fn for_group(
    connection: &Connection,
    group: &GroupId,
    reader: &UserId,
    most: u32,
) -> Result<Vec<Job>> {
    list(
        connection,
        "group_id = ?1 AND (side = 'shared' OR launched_by = ?2)",
        params![group.to_string(), reader.to_string(), most],
    )
}

/// The jobs one machine has taken.
pub fn for_machine(connection: &Connection, machine: &MachineId, most: u32) -> Result<Vec<Job>> {
    list(
        connection,
        "taken_by = ?1 OR chosen_machine = ?1",
        params![machine.to_string(), most],
    )
}

/// Reads several jobs, newest first.
fn list(
    connection: &Connection,
    condition: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<Job>> {
    super::rows(
        connection,
        &format!(
            "SELECT {COLUMNS} FROM jobs WHERE {condition}
          ORDER BY launched_at DESC, rowid DESC LIMIT ?{}",
            count_of(condition) + 1
        ),
        parameters,
        Job::from_row,
    )
    .context("cannot list the jobs")
}

/// How many placeholders a condition uses.
fn count_of(condition: &str) -> usize {
    (1..=9)
        .filter(|at| condition.contains(&format!("?{at}")))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{groups as group_store, machines, test_database, users};
    use stemma_core::access::GroupRole;
    use stemma_core::machine::Detected;

    /// Somebody with a machine that is registered and awake.
    struct Person {
        user: UserId,
        machine: MachineId,
    }

    fn person(connection: &Connection, name: &str) -> Person {
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

        let machine = machines::register(connection, &user, &token, &format!("{name}'s laptop"))
            .unwrap()
            .id;

        Person { user, machine }
    }

    fn claude() -> Slug {
        Slug::parse("claude").unwrap()
    }

    fn asked(group: &GroupId, by: &UserId, choice: Choice) -> Launch {
        Launch {
            group: group.clone(),
            side: Side::Shared,
            line: Name::parse("quotients").unwrap(),
            line_id: None,
            revision: 1,
            against: None,
            task: Task::Fill { after: None },
            agent: claude(),
            request: None,
            grants: None,
            choice,
            by: by.clone(),
        }
    }

    /// One group, one member with one ready machine serving it.
    fn ready(connection: &mut Connection) -> (GroupId, Person) {
        let ana = person(connection, "ana");
        let algebra = group_store::create(
            connection,
            &Slug::parse("algebra").unwrap(),
            "Algebra",
            &ana.user,
        )
        .unwrap()
        .id;

        machines::enable(connection, &ana.machine, &algebra).unwrap();
        machines::report(
            connection,
            &ana.machine,
            &[Detected::ready(claude(), Some("1.0".to_owned()))],
        )
        .unwrap();

        (algebra, ana)
    }

    #[test]
    fn a_job_is_taken_by_a_machine_that_serves_the_group() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        let job = launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();
        assert_eq!(job.state, JobState::Queued);

        let taken = take_next(database.connection_mut(), &ana.machine, &[claude()])
            .unwrap()
            .expect("the machine was given nothing");

        assert_eq!(taken.id, job.id);
        assert_eq!(taken.state, JobState::Running);
        assert_eq!(taken.taken_by, Some(ana.machine));

        // And there is nothing left to take.
        assert!(
            take_next(
                database.connection_mut(),
                &taken.taken_by.unwrap(),
                &[claude()]
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn another_members_job_is_taken_only_after_explicit_sharing() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());
        let enric = person(database.connection(), "enric");
        group_store::add_member(
            database.connection(),
            &algebra,
            &enric.user,
            GroupRole::Member,
        )
        .unwrap();
        let job = launch(
            database.connection(),
            &asked(
                &algebra,
                &enric.user,
                Choice::Machine {
                    machine: ana.machine,
                },
            ),
        )
        .unwrap();

        assert!(
            take_next(database.connection_mut(), &ana.machine, &[claude()])
                .unwrap()
                .is_none()
        );
        machines::share(database.connection(), &ana.machine, &algebra, true).unwrap();
        assert_eq!(
            take_next(database.connection_mut(), &ana.machine, &[claude()])
                .unwrap()
                .unwrap()
                .id,
            job.id
        );
    }

    #[test]
    fn a_launcher_can_withdraw_queued_work_and_request_running_work_to_stop() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        let queued = launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();
        assert!(cancel(database.connection(), &queued.id, &ana.user).unwrap());
        assert_eq!(
            by_id(database.connection(), &queued.id)
                .unwrap()
                .unwrap()
                .state,
            JobState::Withdrawn
        );

        let running = launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();
        take_next(database.connection_mut(), &ana.machine, &[claude()]).unwrap();
        assert!(cancel(database.connection(), &running.id, &ana.user).unwrap());
        let running = by_id(database.connection(), &running.id).unwrap().unwrap();
        assert_eq!(running.state, JobState::Running);
        assert!(running.cancel_requested_at.is_some());
    }

    #[test]
    fn cancelling_work_whose_machine_is_away_ends_it_at_once() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        let running = launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();
        take_next(database.connection_mut(), &ana.machine, &[claude()]).unwrap();
        assert!(cancel(database.connection(), &running.id, &ana.user).unwrap());
        assert!(!cancel(database.connection(), &running.id, &ana.user).unwrap());

        database
            .connection()
            .execute(
                "UPDATE machines SET last_seen_at = datetime('now', '-1 hour')",
                [],
            )
            .unwrap();
        assert!(cancel(database.connection(), &running.id, &ana.user).unwrap());
        let ended = by_id(database.connection(), &running.id).unwrap().unwrap();
        assert_eq!(ended.state, JobState::Done);
        assert_eq!(
            ended.outcome.as_ref().and_then(Outcome::code),
            Some("cancelled")
        );
    }

    #[test]
    fn a_job_that_named_a_machine_goes_to_that_machine_and_waits_for_it() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        let enric = person(database.connection(), "enric");
        group_store::add_member(
            database.connection(),
            &algebra,
            &enric.user,
            GroupRole::Member,
        )
        .unwrap();
        machines::enable(database.connection(), &enric.machine, &algebra).unwrap();
        machines::report(
            database.connection_mut(),
            &enric.machine,
            &[Detected::ready(claude(), None)],
        )
        .unwrap();

        launch(
            database.connection(),
            &asked(
                &algebra,
                &ana.user,
                Choice::Machine {
                    machine: ana.machine,
                },
            ),
        )
        .unwrap();

        // Somebody else's machine is free and could run it, and does not.
        assert!(
            take_next(database.connection_mut(), &enric.machine, &[claude()])
                .unwrap()
                .is_none(),
            "a deliberate choice of machine was overruled"
        );

        assert!(
            take_next(database.connection_mut(), &ana.machine, &[claude()])
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_machine_that_was_chosen_takes_the_job_even_without_the_agent() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        // The agent it was asked for is not one this machine has.
        machines::report(
            database.connection_mut(),
            &ana.machine,
            &[Detected::missing(claude())],
        )
        .unwrap();

        launch(
            database.connection(),
            &asked(
                &algebra,
                &ana.user,
                Choice::Machine {
                    machine: ana.machine,
                },
            ),
        )
        .unwrap();

        // It takes it, so that the owner is told what is missing rather
        // than watching a job sit in a queue for no stated reason.
        assert!(
            take_next(database.connection_mut(), &ana.machine, &[])
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_job_nobody_chose_a_machine_for_waits_for_one_that_has_the_agent() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();

        assert!(
            take_next(database.connection_mut(), &ana.machine, &[])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn work_on_somebodys_own_repository_goes_only_to_their_own_machines() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        let enric = person(database.connection(), "enric");
        group_store::add_member(
            database.connection(),
            &algebra,
            &enric.user,
            GroupRole::Member,
        )
        .unwrap();
        machines::enable(database.connection(), &enric.machine, &algebra).unwrap();
        machines::report(
            database.connection_mut(),
            &enric.machine,
            &[Detected::ready(claude(), None)],
        )
        .unwrap();

        let mut mine = asked(&algebra, &ana.user, Choice::Anyone);
        mine.side = Side::Private;
        launch(database.connection(), &mine).unwrap();

        assert!(
            take_next(database.connection_mut(), &enric.machine, &[claude()])
                .unwrap()
                .is_none(),
            "somebody else's machine was handed a private line"
        );
        assert!(
            take_next(database.connection_mut(), &ana.machine, &[claude()])
                .unwrap()
                .is_some()
        );

        // And it is listed for its author and for nobody else.
        assert_eq!(
            for_group(database.connection(), &algebra, &ana.user, 20)
                .unwrap()
                .len(),
            1
        );
        assert!(
            for_group(database.connection(), &algebra, &enric.user, 20)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn only_the_machine_that_took_a_job_may_say_how_it_ended() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        let job = launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();
        take_next(database.connection_mut(), &ana.machine, &[claude()]).unwrap();

        let outcome = Outcome::Produced {
            said: "there is something to look at".to_owned(),
        };
        let somebody_else = MachineId::generate();

        assert!(!finish(database.connection(), &job.id, &somebody_else, &outcome).unwrap());
        assert!(finish(database.connection(), &job.id, &ana.machine, &outcome).unwrap());
        assert!(
            !finish(database.connection(), &job.id, &ana.machine, &outcome).unwrap(),
            "a job ended twice"
        );

        let done = by_id(database.connection(), &job.id).unwrap().unwrap();
        assert_eq!(done.state, JobState::Done);
        assert_eq!(done.outcome, Some(outcome));
    }

    #[test]
    fn what_stopped_a_job_is_counted_for_the_owner_until_they_deal_with_it() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        let job = launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();
        take_next(database.connection_mut(), &ana.machine, &[claude()]).unwrap();

        assert_eq!(owed_by(database.connection(), &ana.user).unwrap(), 0);

        finish(
            database.connection(),
            &job.id,
            &ana.machine,
            &Outcome::from_failure(&stemma_core::agent::AgentFailure::AgentNotFound {
                agent: claude(),
            }),
        )
        .unwrap();

        assert_eq!(owed_by(database.connection(), &ana.user).unwrap(), 1);
        assert!(
            by_id(database.connection(), &job.id)
                .unwrap()
                .unwrap()
                .owes_its_owner_a_decision()
        );

        assert!(clear(database.connection(), &job.id, &ana.user).unwrap());
        assert_eq!(owed_by(database.connection(), &ana.user).unwrap(), 0);

        // The outcome stays: what cleared was the decision, not the fact.
        let dealt_with = by_id(database.connection(), &job.id).unwrap().unwrap();
        assert!(dealt_with.outcome.unwrap().is_the_machines());
    }

    #[test]
    fn nothing_found_is_an_ordinary_outcome_and_is_never_counted() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        let job = launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();
        take_next(database.connection_mut(), &ana.machine, &[claude()]).unwrap();
        finish(
            database.connection(),
            &job.id,
            &ana.machine,
            &Outcome::NothingFound {
                said: "no argument was found".to_owned(),
            },
        )
        .unwrap();

        assert_eq!(owed_by(database.connection(), &ana.user).unwrap(), 0);
    }

    #[test]
    fn work_the_machine_could_not_reach_is_nobodys_to_fix() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        let job = launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();
        take_next(database.connection_mut(), &ana.machine, &[claude()]).unwrap();
        finish(
            database.connection(),
            &job.id,
            &ana.machine,
            &Outcome::out_of_reach("this machine does not have that commit"),
        )
        .unwrap();

        // The machine is working. Telling its owner to fix it would send
        // them looking for something that is not wrong.
        assert_eq!(owed_by(database.connection(), &ana.user).unwrap(), 0);
        assert!(
            !by_id(database.connection(), &job.id)
                .unwrap()
                .unwrap()
                .owes_its_owner_a_decision()
        );
    }

    #[test]
    fn a_job_can_be_called_back_only_before_it_runs_and_only_by_its_author() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());
        let enric = person(database.connection(), "enric");

        let job = launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();

        assert!(!withdraw(database.connection(), &job.id, &enric.user).unwrap());
        assert!(withdraw(database.connection(), &job.id, &ana.user).unwrap());
        assert_eq!(
            by_id(database.connection(), &job.id)
                .unwrap()
                .unwrap()
                .state,
            JobState::Withdrawn
        );

        assert!(
            take_next(database.connection_mut(), &ana.machine, &[claude()])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_machine_that_no_longer_serves_the_group_is_given_nothing() {
        let mut database = test_database();
        let (algebra, ana) = ready(database.connection_mut());

        launch(
            database.connection(),
            &asked(&algebra, &ana.user, Choice::Anyone),
        )
        .unwrap();
        machines::disable(database.connection(), &ana.machine, &algebra).unwrap();

        assert!(
            take_next(database.connection_mut(), &ana.machine, &[claude()])
                .unwrap()
                .is_none()
        );
    }
}

//! This machine as a worker: its agents and jobs, and offering it to
//! groups.

use serde::Serialize;
use stemma_core::id::BlockId;
use stemma_core::proposal::{Grants, LineGrant};

use crate::workspace::LineAt;
use crate::{Session, carried_out, failed, home};

/// One agent a machine offers, and whether it can be used.
#[derive(Serialize)]
pub struct Agent {
    name: String,
    called: String,
    availability: String,
    reason: Option<String>,
}

impl Agent {
    fn of(agent: &stemma_core::machine::Detected) -> Self {
        let name = agent.agent.as_str();
        Self {
            called: stemma_core::machine::known(name)
                .map_or(name, |known| known.called)
                .to_owned(),
            name: name.to_owned(),
            availability: agent.availability.to_string(),
            reason: agent.availability.reason().map(str::to_owned),
        }
    }
}

/// One job this machine was given.
#[derive(Serialize)]
pub struct Job {
    id: String,
    group: String,
    side: String,
    line: String,
    state: String,
    outcome: Option<String>,
    owed: bool,
    task: String,
    agent: String,
    said: String,
    raw_output: bool,
    workspace: Option<String>,
    progress: Option<String>,
    cancel_requested: bool,
}

/// This machine as a worker, for the settings.
#[derive(Serialize)]
pub struct MachinePanel {
    label: String,
    presence: String,
    agents: Vec<Agent>,
    jobs: Vec<Job>,
    workspaces: Workspaces,
    /// Whether the background service runs jobs while the app is closed.
    while_closed: bool,
}

/// How much room the workspaces of this machine's jobs take.
#[derive(Serialize)]
pub struct Workspaces {
    count: usize,
    bytes: u64,
    /// How many bytes they may take before the oldest are removed.
    limit: u64,
}

/// A machine a group can send work to.
#[derive(Serialize)]
pub struct Offer {
    machine: String,
    label: String,
    presence: String,
    agents: Vec<Agent>,
}

/// This machine as the instance sees it, with the jobs it was given;
/// nothing when it has not offered to run jobs.
#[tauri::command]
pub async fn machine() -> Result<Option<MachinePanel>, String> {
    crate::blocking(move || -> Result<Option<MachinePanel>, String> {
        let home = home();
        let session = Session::open()?;
        let Some(node) = session.server.node(&session.token).map_err(failed)? else {
            return Ok(None);
        };
        let jobs = session
            .server
            .jobs(&session.token)
            .map_err(failed)?
            .into_iter()
            .map(|job| Job {
                raw_output: stemma::job::work::output_exists(&home, &job.job),
                id: job.job,
                group: job.group,
                side: job.side,
                line: job.line,
                state: job.state,
                outcome: job.outcome,
                owed: job.owed,
                task: job.task,
                agent: job.agent,
                said: job.said,
                workspace: job.workspace,
                progress: job.progress,
                cancel_requested: job.cancel_requested,
            })
            .collect();

        let used = stemma::job::workspace::used(&home);
        let config = stemma::config::Config::read(&home.config_file()).unwrap_or_default();
        let limit = config.jobs.workspace_limit();

        Ok(Some(MachinePanel {
            label: node.label,
            presence: node.presence.to_string(),
            agents: node.agents.iter().map(Agent::of).collect(),
            jobs,
            workspaces: Workspaces {
                count: used.count,
                bytes: used.bytes,
                limit,
            },
            while_closed: config.jobs.while_closed,
        }))
    })
    .await
}

/// Gives this machine the name its owner chose, and says the name kept.
#[tauri::command]
pub async fn rename_machine(label: String) -> Result<String, String> {
    crate::blocking(move || -> Result<String, String> {
        let session = Session::open()?;
        let node = session
            .server
            .rename_node(&session.token, label.trim())
            .map_err(failed)?;
        Ok(node.label)
    })
    .await
}

/// Says whether the background service keeps running jobs while the app
/// is closed. Turned on, a service the app's quitting stopped starts again
/// now rather than at the next launch.
#[tauri::command]
pub async fn set_while_closed(while_closed: bool) -> Result<(), String> {
    crate::blocking(move || -> Result<(), String> {
        let home = home();
        let path = home.config_file();
        let mut config = stemma::config::Config::read(&path).map_err(failed)?;
        config.jobs.while_closed = while_closed;
        config.write(&path).map_err(failed)?;

        let service = stemma::machine::service::service(&home);
        if while_closed && service == stemma::machine::service::Service::Stopped {
            stemma::machine::service::start(&home).map_err(failed)?;
        }
        Ok(())
    })
    .await
}

/// The raw agent output kept on this machine for one job.
#[tauri::command]
pub async fn job_output(job: String) -> Result<String, String> {
    crate::blocking(move || -> Result<String, String> {
        carried_out(|home, out| stemma::job::launch::output(home, &job, out))
    })
    .await
}

/// Marks what stopped a job as resolved, once its owner has fixed it.
#[tauri::command]
pub async fn clear_job(job: String) -> Result<(), String> {
    crate::blocking(move || -> Result<(), String> {
        let session = Session::open()?;
        session
            .server
            .clear_job(&session.token, &job)
            .map_err(failed)
    })
    .await
}

/// Cancels a queued job or asks its running agent to stop.
#[tauri::command]
pub async fn cancel_job(job: String) -> Result<(), String> {
    crate::blocking(move || -> Result<(), String> {
        let session = Session::open()?;
        session
            .server
            .cancel_job(&session.token, &job)
            .map_err(failed)
    })
    .await
}

/// The machines that work for `group`, and what each can run.
#[tauri::command]
pub async fn offers(group: String) -> Result<Vec<Offer>, String> {
    crate::blocking(move || -> Result<Vec<Offer>, String> {
        let session = Session::open()?;
        Ok(session
            .server
            .machines(&session.token, &group)
            .map_err(failed)?
            .into_iter()
            .map(|machine| Offer {
                agents: machine.agents.iter().map(Agent::of).collect(),
                machine: machine.machine,
                label: machine.label,
                presence: machine.presence,
            })
            .collect())
    })
    .await
}

/// Asks `machine` to run `task` with `agent` on the line.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn launch(
    path: String,
    task: String,
    agent: String,
    here: bool,
    machine: Option<String>,
    request: Option<String>,
    grant_blocks: Vec<String>,
    grant_library: bool,
) -> Result<String, String> {
    crate::blocking(move || -> Result<String, String> {
        let line = LineAt::admit(&path)?;
        let wanted = stemma::job::launch::Wanted {
            line: None,
            task,
            agent,
            machine,
            here,
            grants: request
                .is_some()
                .then(|| grants(grant_blocks, grant_library))
                .transpose()?,
            request,
        };
        stemma::job::launch::run(&home(), line.directory(), &wanted, &mut Vec::new())
            .map_err(failed)
    })
    .await
}

/// What a custom request may change: each block granted may be edited,
/// removed or followed by new blocks, and the Lean library when allowed.
fn grants(blocks: Vec<String>, library: bool) -> Result<Grants, String> {
    let mut line = Vec::new();
    for raw in blocks {
        let block: BlockId = raw
            .parse()
            .map_err(|error| format!("invalid block grant: {error}"))?;
        line.extend([
            LineGrant::Edit { block },
            LineGrant::Remove { block },
            LineGrant::Add { after: Some(block) },
        ]);
    }
    Ok(Grants {
        line,
        library,
        add_only: false,
    })
}

/// Offers this machine to the instance for work in `groups`, and says in
/// one sentence what came of it.
#[tauri::command]
pub async fn node_init(groups: Vec<String>) -> Result<String, String> {
    crate::blocking(move || -> Result<String, String> {
        let node = stemma::node::init(&home(), &groups, &mut std::io::sink()).map_err(failed)?;
        Ok(registered(&node))
    })
    .await
}

/// What registering came to, as a person reads it in a notice: which
/// groups the machine works for, and which agents it can run.
fn registered(node: &stemma::api::Node) -> String {
    let groups = match node.groups.as_slice() {
        [] => format!("{} works for no group yet.", node.label),
        groups => format!("{} works for {}.", node.label, listed(groups)),
    };
    let ready: Vec<String> = node
        .agents
        .iter()
        .filter(|agent| agent.availability.is_usable())
        .map(|agent| {
            stemma_core::machine::known(agent.agent.as_str())
                .map_or(agent.agent.as_str(), |known| known.called)
                .to_owned()
        })
        .collect();

    match ready.as_slice() {
        [] => format!("{groups} No agent is ready on it."),
        ready => format!("{groups} Ready: {}.", listed(ready)),
    }
}

/// Names in a sentence: `a`, `a and b`, `a, b and c`.
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn registering_is_said_in_one_sentence() {
        use stemma_core::machine::{Availability, Detected};

        let node = stemma::api::Node {
            machine: "mch:01M3VW1C32DP2Y3KAPA310B7B1".to_owned(),
            label: "Marios-MBP".to_owned(),
            presence: stemma_core::machine::Presence::Awake,
            groups: vec!["prova".to_owned(), "sample".to_owned()],
            agents: vec![
                Detected {
                    agent: "claude".parse().unwrap(),
                    version: Some("2.1.283".to_owned()),
                    availability: Availability::Ready,
                },
                Detected {
                    agent: "gemini".parse().unwrap(),
                    version: None,
                    availability: Availability::NotInstalled,
                },
            ],
        };

        assert_eq!(
            super::registered(&node),
            "Marios-MBP works for prova and sample. Ready: Claude Code."
        );
    }

    use stemma_core::proposal::LineGrant;

    use super::grants;

    #[test]
    fn a_granted_block_may_be_edited_removed_and_followed() {
        let block = "blk:01K5YQ4TQ2VZ8H3M9N0P1R2S3T";
        let granted = grants(vec![block.to_owned()], true).unwrap();
        let block = block.parse().unwrap();

        assert_eq!(
            granted.line,
            [
                LineGrant::Edit { block },
                LineGrant::Remove { block },
                LineGrant::Add { after: Some(block) },
            ]
        );
        assert!(granted.library);
        assert!(!granted.add_only);
    }

    #[test]
    fn a_grant_that_is_not_a_block_is_refused() {
        assert!(grants(vec!["thm:main".to_owned()], false).is_err());
    }
}

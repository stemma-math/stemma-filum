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
        let limit = stemma::config::Config::read(&home.config_file())
            .unwrap_or_default()
            .jobs
            .workspace_limit();

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
        }))
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

/// Offers this machine to the instance for work in `groups`.
#[tauri::command]
pub async fn node_init(groups: Vec<String>) -> Result<String, String> {
    crate::blocking(move || -> Result<String, String> {
        carried_out(|home, out| stemma::node::init(home, &groups, out).map(|_| ()))
    })
    .await
}

#[cfg(test)]
mod tests {
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

//! This machine as a worker: its agents and jobs, and offering it to
//! groups.

use serde::Serialize;

use crate::workspace::LineAt;
use crate::{Session, carried_out, failed, home, said};

#[derive(Serialize)]
pub struct Agent {
    name: String,
    called: String,
    availability: String,
    reason: Option<String>,
    models: Vec<String>,
    efforts: Vec<String>,
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
            models: agent.models.clone(),
            efforts: agent.efforts.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct Job {
    id: String,
    state: String,
    task: String,
    agent: String,
    said: String,
    raw_output: bool,
    workspace: Option<String>,
    progress: Option<String>,
    cancel_requested: bool,
}

#[derive(Serialize)]
pub struct MachinePanel {
    label: String,
    presence: String,
    agents: Vec<Agent>,
    jobs: Vec<Job>,
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
#[tauri::command(async)]
pub fn machine() -> Result<Option<MachinePanel>, String> {
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
            raw_output: stemma::work::output_exists(&home, &job.job),
            id: job.job,
            state: job.state,
            task: job.task,
            agent: job.agent,
            said: job.said,
            workspace: job.workspace,
            progress: job.progress,
            cancel_requested: job.cancel_requested,
        })
        .collect();

    Ok(Some(MachinePanel {
        label: node.label,
        presence: node.presence.to_string(),
        agents: node.agents.iter().map(Agent::of).collect(),
        jobs,
    }))
}

/// The raw agent output kept on this machine for one job.
#[tauri::command(async)]
pub fn job_output(job: String) -> Result<String, String> {
    carried_out(|home, out| stemma::jobs::output(home, &job, out))
}

/// Cancels a queued job or asks its running agent to stop.
#[tauri::command(async)]
pub fn cancel_job(job: String) -> Result<(), String> {
    let session = Session::open()?;
    session
        .server
        .cancel_job(&session.token, &job)
        .map_err(said)
}

/// The machines that work for `group`, and what each can run.
#[tauri::command(async)]
pub fn offers(group: String) -> Result<Vec<Offer>, String> {
    let session = Session::open()?;
    Ok(session
        .server
        .machines(&session.token, &group)
        .map_err(said)?
        .into_iter()
        .map(|machine| Offer {
            agents: machine.agents.iter().map(Agent::of).collect(),
            machine: machine.machine,
            label: machine.label,
            presence: machine.presence,
        })
        .collect())
}

/// Asks `machine` to run `task` with `agent` on the line.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn launch(
    path: String,
    task: String,
    agent: String,
    here: bool,
    machine: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    request: Option<String>,
    grant_blocks: Vec<String>,
    grant_library: bool,
) -> Result<String, String> {
    let line = LineAt::admit(&path)?;
    let wanted = stemma::jobs::Wanted {
        line: None,
        task,
        agent,
        machine,
        here,
        model,
        effort,
        request: request.clone(),
        grants: request
            .map(|_| {
                let mut line = Vec::new();
                for raw in grant_blocks {
                    let block = raw
                        .parse()
                        .map_err(|error| format!("invalid block grant: {error}"))?;
                    line.push(stemma_core::proposal::LineGrant::Edit { block });
                    line.push(stemma_core::proposal::LineGrant::Remove { block });
                    line.push(stemma_core::proposal::LineGrant::Add { after: Some(block) });
                }
                Ok::<_, String>(stemma_core::proposal::Grants {
                    line,
                    library: grant_library,
                    add_only: false,
                })
            })
            .transpose()?,
    };
    stemma::jobs::run(&home(), line.directory(), &wanted, &mut Vec::new()).map_err(failed)
}

/// Offers this machine to the instance for work in `groups`.
#[tauri::command(async)]
pub fn node_init(groups: Vec<String>) -> Result<String, String> {
    carried_out(|home, out| stemma::node::init(home, &groups, out).map(|_| ()))
}

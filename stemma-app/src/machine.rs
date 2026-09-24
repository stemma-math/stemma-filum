//! This machine as a worker: its agents and jobs, offering it to groups,
//! and installing the command line and daemon it carries.

use std::path::PathBuf;

use serde::Serialize;

use crate::workspace::LineAt;
use crate::{Session, carried_out, home, said};

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

#[derive(Serialize)]
pub struct Job {
    id: String,
    state: String,
    task: String,
    agent: String,
    said: String,
    raw_output: bool,
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

/// This machine as the instance sees it, with the jobs it was given.
#[tauri::command(async)]
pub fn machine() -> Result<MachinePanel, String> {
    let home = home();
    let session = Session::open()?;
    let node = session
        .server
        .node(&session.token)
        .map_err(said)?
        .ok_or_else(|| "run `stemma node init` to offer this machine".to_owned())?;
    let jobs = session
        .server
        .jobs(&session.token)
        .map_err(said)?
        .into_iter()
        .map(|job| Job {
            raw_output: stemma::work::output_exists(&home, &job.job),
            id: job.job,
            state: job.state,
            task: job.task,
            agent: job.agent,
            said: job.said,
        })
        .collect();

    Ok(MachinePanel {
        label: node.label,
        presence: node.presence.to_string(),
        agents: node.agents.iter().map(Agent::of).collect(),
        jobs,
    })
}

/// The raw agent output kept on this machine for one job.
#[tauri::command(async)]
pub fn job_output(job: String) -> Result<String, String> {
    carried_out(|home, out| stemma::jobs::output(home, &job, out))
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
pub fn launch(
    path: String,
    task: String,
    agent: String,
    here: bool,
    machine: Option<String>,
) -> Result<String, String> {
    let line = LineAt::admit(&path)?;
    let wanted = stemma::jobs::Wanted {
        line: None,
        task,
        agent,
        machine,
        here,
    };
    stemma::jobs::run(&home(), line.directory(), &wanted, &mut Vec::new()).map_err(said)
}

/// Offers this machine to the instance for work in `groups`.
#[tauri::command(async)]
pub fn node_init(groups: Vec<String>) -> Result<String, String> {
    carried_out(|home, out| stemma::node::init(home, &groups, out).map(|_| ()))
}

/// Registers the daemon with the system's user service manager.
#[tauri::command(async)]
pub fn daemon_install() -> Result<String, String> {
    carried_out(stemma::daemon::install)
}

/// Links the `stemma` binary bundled with the application into
/// `~/.local/bin`, which needs no administrator rights.
#[tauri::command(async)]
pub fn install_cli() -> Result<String, String> {
    let carried = stemma::program::carried().ok_or_else(|| {
        "this build does not carry the command line: build the application with `just app`"
            .to_owned()
    })?;

    let home = std::env::var("HOME").map_err(|_| "there is no home directory".to_owned())?;
    let directory = PathBuf::from(home).join(".local").join("bin");
    let placed = directory.join(stemma::program::NAME);

    std::fs::create_dir_all(&directory).map_err(said)?;
    if std::fs::symlink_metadata(&placed).is_ok() {
        std::fs::remove_file(&placed).map_err(said)?;
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(&carried, &placed).map_err(said)?;
    #[cfg(not(unix))]
    std::fs::copy(&carried, &placed).map_err(said)?;

    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|entry| entry == directory));

    Ok(if on_path {
        format!("`stemma` is at {}, and on your path.", placed.display())
    } else {
        format!(
            "`stemma` is at {}. Add that directory to your path: \
             echo 'export PATH=\"$HOME/.local/bin:$PATH\"' >> ~/.zshrc",
            placed.display()
        )
    })
}

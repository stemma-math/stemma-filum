//! Setting this machine up from the application, repairing it at each
//! launch, and removing Stemma from it.
//!
//! The checklist is one survey of five items, each done, needing
//! attention, or not set up, and one command per item that does it. The
//! steps of removal are separate commands, so the window can ask its
//! questions between them.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use stemma::machine::daemon::Started;
use stemma::machine::service::Service;
use stemma::machine::setup::{Link, Repaired, Shell};
use stemma_core::machine::Availability;
use tauri::{AppHandle, Emitter, Runtime};

use crate::{Session, failed, home, said};

/// The name the window listens for once repairing at launch is done.
pub const REPAIRED_EVENT: &str = "stemma:repaired";

/// What repairing at launch could not fix, once it has run.
static REPAIRED: Mutex<Option<Repaired>> = Mutex::new(None);

/// What trying each agent out found, this session.
static TRIED: Mutex<BTreeMap<String, Tried>> = Mutex::new(BTreeMap::new());

/// Where to read how to install Lean.
const LEAN_INSTRUCTIONS: &str = "https://lean-lang.org/lean4/doc/quickstart.html";

/// Where one item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Done,
    Attention,
    NotSetUp,
    /// It could not be checked, as when the server is out of reach.
    Unknown,
}

/// One item of the checklist.
#[derive(Serialize)]
pub struct Item {
    state: State,
    /// Why, when it is not done.
    detail: Option<String>,
}

impl Item {
    fn done() -> Self {
        Self {
            state: State::Done,
            detail: None,
        }
    }

    fn not_set_up() -> Self {
        Self {
            state: State::NotSetUp,
            detail: None,
        }
    }

    fn because(state: State, detail: impl Into<String>) -> Self {
        Self {
            state,
            detail: Some(detail.into()),
        }
    }
}

/// The command line: its link, and whether a terminal finds it.
#[derive(Serialize)]
pub struct CommandLine {
    #[serde(flatten)]
    item: Item,
    link: Option<String>,
    /// When a terminal would not find it: what to add, and where.
    path_line: Option<PathLine>,
}

#[derive(Serialize)]
pub struct PathLine {
    shell: String,
    profile: String,
    line: String,
}

/// Which groups this machine works for.
#[derive(Serialize)]
pub struct Groups {
    #[serde(flatten)]
    item: Item,
    serving: Vec<String>,
}

/// One agent.
#[derive(Serialize)]
pub struct Agent {
    name: String,
    called: String,
    #[serde(flatten)]
    item: Item,
    version: Option<String>,
    instructions: String,
}

/// Lean, through elan.
#[derive(Serialize)]
pub struct Lean {
    #[serde(flatten)]
    item: Item,
    instructions: String,
}

/// The whole checklist.
#[derive(Serialize)]
pub struct Survey {
    service: Item,
    command_line: CommandLine,
    groups: Groups,
    agents: Vec<Agent>,
    lean: Lean,
}

/// What trying an agent out found.
#[derive(Debug, Clone, Serialize)]
pub struct Tried {
    availability: Availability,
    said: String,
}

/// Repairs what moving or updating the application broke, and tells the
/// window when it is done. Runs once, as the application starts.
pub fn repair_at_launch<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();

    std::thread::Builder::new()
        .name("stemma-repair".to_owned())
        .spawn(move || {
            let repaired = match stemma::machine::program::carried() {
                Some(carried) => stemma::machine::setup::repair(&home(), &carried),
                None => Repaired::default(),
            };
            if let Ok(mut kept) = REPAIRED.lock() {
                *kept = Some(repaired);
            }
            let _ = app.emit(REPAIRED_EVENT, ());
        })
        .expect("a thread to repair this machine on");
}

/// What repairing at launch could not fix, so far.
fn repaired() -> Repaired {
    REPAIRED
        .lock()
        .ok()
        .and_then(|kept| kept.clone())
        .unwrap_or_default()
}

/// Where every item stands.
#[tauri::command(async)]
pub fn setup_survey() -> Result<Survey, String> {
    let home = home();
    let repaired = repaired();
    let config = stemma::config::Config::read(&home.config_file()).map_err(failed)?;

    // What the server knows about this machine: which groups it works
    // for, and what jobs taught it about its agents.
    let node =
        Session::open().and_then(|session| session.server.node(&session.token).map_err(failed));

    let groups = match &node {
        Ok(Some(node)) if !node.groups.is_empty() => Groups {
            item: Item::done(),
            serving: node.groups.clone(),
        },
        Ok(_) => Groups {
            item: Item::not_set_up(),
            serving: Vec::new(),
        },
        Err(problem) => Groups {
            item: Item::because(State::Unknown, problem.clone()),
            serving: Vec::new(),
        },
    };

    let learned: BTreeMap<String, Availability> = match &node {
        Ok(Some(node)) => node
            .agents
            .iter()
            .map(|agent| (agent.agent.as_str().to_owned(), agent.availability))
            .collect(),
        _ => BTreeMap::new(),
    };
    let tried = TRIED.lock().map(|tried| tried.clone()).unwrap_or_default();
    let agents = stemma::job::agents::look(&home, &config)
        .into_iter()
        .map(|found| {
            let name = found.agent.as_str().to_owned();
            let known = stemma_core::machine::known(&name);
            let availability = match (found.availability, tried.get(&name), learned.get(&name)) {
                (Availability::NotInstalled, _, _) => Availability::NotInstalled,
                (_, Some(tried), _) => tried.availability,
                (_, None, Some(learned)) => *learned,
                (found, None, None) => found,
            };
            let item = match availability {
                Availability::Ready => Item::done(),
                Availability::NotInstalled => Item::not_set_up(),
                other => Item::because(
                    State::Attention,
                    tried
                        .get(&name)
                        .map(|tried| tried.said.clone())
                        .or_else(|| other.reason().map(str::to_owned))
                        .unwrap_or_default(),
                ),
            };

            Agent {
                called: known.map_or(name.as_str(), |known| known.called).to_owned(),
                instructions: known.map_or("", |known| known.instructions).to_owned(),
                version: found.version,
                item,
                name,
            }
        })
        .collect();

    Ok(Survey {
        service: service(&home, &repaired),
        command_line: command_line(&repaired),
        groups,
        agents,
        lean: Lean {
            item: match stemma::lean::elan() {
                Some(_) => Item::done(),
                None => Item::not_set_up(),
            },
            instructions: LEAN_INSTRUCTIONS.to_owned(),
        },
    })
}

/// The background service's item.
fn service(home: &stemma::home::Home, repaired: &Repaired) -> Item {
    let standing = match stemma::machine::setup::service_standing(home) {
        Ok(standing) => standing,
        Err(error) => return Item::because(State::Attention, failed(error)),
    };
    if standing == Service::NotInstalled {
        return Item::not_set_up();
    }
    if let Some(problem) = &repaired.service {
        return Item::because(State::Attention, problem.clone());
    }

    let program = stemma::machine::setup::stable_program().ok();
    let written = stemma::machine::service::service_file(home)
        .ok()
        .flatten()
        .and_then(|path| std::fs::read_to_string(path).ok());
    let wanted = program
        .as_ref()
        .and_then(|program| stemma::machine::service::service_text(home, program));
    if written.is_none() || written != wanted {
        return Item::because(State::Attention, "It runs a different copy of Stemma.");
    }

    match standing {
        Service::Running(pid) => {
            let current = program
                .and_then(|program| program.canonicalize().ok())
                .zip(Started::read(home))
                .is_some_and(|(program, started)| started.is_current(pid, &program));
            match current {
                true => Item::done(),
                false => Item::because(
                    State::Attention,
                    "An earlier version of the service is running.",
                ),
            }
        }
        _ => Item::because(State::Attention, "It is installed and not running."),
    }
}

/// The command line's item.
fn command_line(repaired: &Repaired) -> CommandLine {
    let plain = |item| CommandLine {
        item,
        link: stemma::machine::setup::link_path()
            .ok()
            .map(|path| path.display().to_string()),
        path_line: None,
    };

    let Some(carried) = stemma::machine::program::carried() else {
        return plain(Item::because(
            State::Attention,
            "This build of the application does not carry the command line.",
        ));
    };
    if let Some(problem) = &repaired.link {
        return plain(Item::because(State::Attention, problem.clone()));
    }

    match stemma::machine::setup::link() {
        Err(error) => plain(Item::because(State::Attention, failed(error))),
        Ok(Link::Missing) => plain(Item::not_set_up()),
        Ok(Link::Other) => plain(Item::because(
            State::Attention,
            "Another program is where the command line goes.",
        )),
        Ok(Link::To(target)) if !stemma::machine::setup::links_to(&target, &carried) => plain(
            Item::because(State::Attention, "It leads to another copy of Stemma."),
        ),
        Ok(Link::To(_)) => match Shell::login() {
            Some(shell) if !shell.finds_commands() => CommandLine {
                path_line: shell.profile().ok().map(|profile| PathLine {
                    shell: shell.name().to_owned(),
                    profile: profile.display().to_string(),
                    line: shell.line().to_owned(),
                }),
                ..plain(Item::because(
                    State::Attention,
                    "A terminal would not find it: ~/.local/bin is not on your path.",
                ))
            },
            _ => plain(Item::done()),
        },
    }
}

/// Installs the background service, and the command line's link it runs.
#[tauri::command(async)]
pub fn setup_service() -> Result<String, String> {
    let home = home();

    // The service runs the link rather than the bundle, which moves.
    if let Some(carried) = stemma::machine::program::carried()
        && stemma::machine::setup::link().map_err(failed)? == Link::Missing
    {
        stemma::machine::setup::link_to(&carried).map_err(failed)?;
    }
    let program = stemma::machine::setup::stable_program().map_err(failed)?;
    stemma::machine::service::put_in_place(&home, &program).map_err(failed)?;
    forget_repair(|repaired| repaired.service = None);

    Ok("The background service is running and starts with this machine.".to_owned())
}

/// Links the command line the application carries into `~/.local/bin`.
#[tauri::command(async)]
pub fn setup_link() -> Result<String, String> {
    let carried = stemma::machine::program::carried().ok_or_else(|| {
        "This build of the application does not carry the command line.".to_owned()
    })?;
    let placed = stemma::machine::setup::link_to(&carried).map_err(failed)?;
    forget_repair(|repaired| repaired.link = None);

    Ok(format!("The stemma command is at {}.", placed.display()))
}

/// Adds the line that puts `~/.local/bin` on the path to the profile of
/// the shell this account logs in with.
#[tauri::command(async)]
pub fn setup_path() -> Result<String, String> {
    let shell =
        Shell::login().ok_or_else(|| "Your login shell is not one Stemma knows.".to_owned())?;
    let profile = shell.add_line().map_err(failed)?;

    Ok(format!(
        "Added to {}. Terminals opened from now on find the stemma command.",
        profile.display()
    ))
}

/// Asks an agent for one word, to find out whether it is installed and
/// signed in.
#[tauri::command(async)]
pub fn try_agent(agent: String) -> Result<Tried, String> {
    let home = home();
    let config = stemma::config::Config::read(&home.config_file()).map_err(failed)?;
    let slug = stemma_core::id::Slug::parse(&agent).map_err(said)?;
    let ran = stemma::job::agents::try_out(&home, &config, &agent).map_err(failed)?;

    let tried = match stemma::job::agents::trouble(&slug, &ran) {
        None => Tried {
            availability: Availability::Ready,
            said: "It answered.".to_owned(),
        },
        Some(failure) => Tried {
            availability: Availability::learned_from(&failure).unwrap_or(Availability::Ready),
            said: failure.to_string(),
        },
    };
    if let Ok(mut all) = TRIED.lock() {
        all.insert(agent, tried.clone());
    }

    Ok(tried)
}

/// Clears what repairing at launch reported, once the owner fixed it.
fn forget_repair(fixed: impl FnOnce(&mut Repaired)) {
    if let Ok(mut kept) = REPAIRED.lock()
        && let Some(repaired) = kept.as_mut()
    {
        fixed(repaired);
    }
}

/// Sends what can be sent, and answers the lines still not on the
/// server, as `group/side/name`.
#[tauri::command(async)]
pub fn removal_send() -> Vec<String> {
    stemma::machine::uninstall::send(&home())
        .into_iter()
        .map(|line| format!("{}/{}/{}", line.group, line.side.as_str(), line.name))
        .collect()
}

/// Stops and removes the service, the link and the profile line, takes
/// the machine back on the server and signs out.
#[tauri::command(async)]
pub fn removal_leave() -> Result<Vec<String>, String> {
    let home = home();
    let mut done = stemma::machine::uninstall::take_down(&home).map_err(failed)?;
    done.extend(stemma::machine::uninstall::leave(&home).map_err(failed)?);

    Ok(done)
}

/// What Stemma keeps on this machine.
#[tauri::command(async)]
pub fn removal_data() -> Vec<String> {
    stemma::machine::uninstall::local_data(&home())
        .into_iter()
        .map(|path| path.display().to_string())
        .collect()
}

/// Deletes what Stemma keeps on this machine.
#[tauri::command(async)]
pub fn removal_delete_data() -> Result<(), String> {
    stemma::machine::uninstall::delete_data(&home()).map_err(failed)
}

/// The application bundle, when the application runs from one.
fn bundle() -> Option<PathBuf> {
    stemma::machine::uninstall::bundle_of(&std::env::current_exe().ok()?)
}

/// Whether the application can be moved to the Trash.
#[tauri::command(async)]
pub fn can_trash() -> bool {
    cfg!(target_os = "macos") && bundle().is_some()
}

/// Moves the application to the Trash, and quits.
#[tauri::command(async)]
pub fn trash_and_quit<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let bundle =
        bundle().ok_or_else(|| "The application is not running from a bundle.".to_owned())?;
    let script = format!(
        "tell application \"Finder\" to delete (POSIX file \"{}\")",
        bundle
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    );
    let ran = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(said)?;
    if !ran.status.success() {
        return Err(format!(
            "Stemma could not be moved to the Trash: {}",
            String::from_utf8_lossy(&ran.stderr).trim()
        ));
    }

    app.exit(0);
    Ok(())
}

/// Quits the application.
#[tauri::command(async)]
pub fn quit<R: Runtime>(app: AppHandle<R>) {
    app.exit(0);
}

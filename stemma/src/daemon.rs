//! The loop that takes work, and keeps lines in step with the window shut.
//!
//! It says it is there, asks for a job, does it, reports the type of
//! what happened, and asks again: the instance keeps the queue, this
//! machine does the work, and the two never swap jobs. Beside it runs the
//! engine the application runs, which works while the application is
//! closed, so what was written still leaves.
//!
//! A headless machine has to be able to do all of this, so the daemon is
//! a command like any other and can be put under whatever supervises the
//! machine it runs on.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use stemma_core::job::Outcome;
use stemma_core::machine::{BEAT_SECONDS, Detected};

use crate::api::Server;
use crate::config::Config;
use crate::credentials::Credentials;
use crate::home::Home;
use crate::session;
use crate::work;

/// How often to look for agents again.
///
/// Somebody installs one while the daemon is running, and the instance
/// should find out without being restarted.
const LOOK_EVERY: Duration = Duration::from_secs(10 * 60);

/// How long to wait after failing to reach the instance.
const AFTER_TROUBLE: Duration = Duration::from_secs(30);

/// Runs the loop.
///
/// It keeps this machine's lines in step while the application is closed
/// — the same engine, one at a time — and, on a machine registered to run
/// jobs, takes them. With `once`, it does one round and takes at most one
/// job and returns, which is what a machine woken by something else wants
/// and what a test can watch.
pub fn run(home: &Home, once: bool, out: &mut impl Write) -> Result<()> {
    let (credentials, server) = session::connect(home)?;
    let config = Config::read(&home.config_file())?;
    let node = server.node(&credentials.token)?.is_some();

    if once {
        if !crate::mirror::Store::at(home.state_dir()).window_open() {
            let report = crate::mirror::Mirror::at(home)?.step(
                &crate::mirror::Instance::connect(home)?,
                true,
                &crate::mirror::Which::All,
            );
            crate::mirror::tell(&report, out)?;
        }
        if !node {
            return Ok(());
        }
    }

    let woken = match once {
        true => None,
        false => Some(listen(home)),
    };

    if !node {
        writeln!(
            out,
            "Keeping lines in step with {} as {}. This machine is not registered to run \
             jobs: `stemma node init` offers it.",
            credentials.server, credentials.user
        )?;
        out.flush()?;

        // Nothing else to do: the engine runs until the program is stopped.
        loop {
            std::thread::park();
        }
    }

    let mut agents = crate::agents::look(home, &config);
    let mut looked = Instant::now();

    writeln!(
        out,
        "Running jobs from {} as {}.",
        credentials.server, credentials.user
    )?;
    out.flush()?;

    loop {
        if looked.elapsed() >= LOOK_EVERY {
            agents = crate::agents::look(home, &config);
            looked = Instant::now();
        }

        match server.beat(&credentials.token, &agents) {
            Ok(_) => {}
            Err(error) if once => return Err(error),
            Err(error) => {
                writeln!(out, "{}: {error:#}", credentials.server)?;
                out.flush()?;
                std::thread::sleep(AFTER_TROUBLE);

                continue;
            }
        }

        let taken = match server.take_job(&credentials.token) {
            Ok(taken) => taken,
            Err(error) if once => return Err(error),
            Err(error) => {
                writeln!(out, "{}: {error:#}", credentials.server)?;
                out.flush()?;
                std::thread::sleep(AFTER_TROUBLE);

                continue;
            }
        };

        let Some(handed) = taken else {
            if once {
                writeln!(out, "No jobs waiting.")?;
                return Ok(());
            }

            // Until the next beat is due, or until the instance says
            // there is work, whichever is first.
            match &woken {
                Some(woken) => {
                    let _ = woken.recv_timeout(Duration::from_secs(BEAT_SECONDS));
                    while woken.try_recv().is_ok() {}
                }
                None => std::thread::sleep(Duration::from_secs(BEAT_SECONDS)),
            }
            continue;
        };

        do_one(
            home,
            &config,
            &server,
            &credentials,
            &handed,
            &mut agents,
            out,
        )?;

        if once {
            return Ok(());
        }
    }
}

/// Listens to the instance on a thread of its own, passing everything
/// to the engine, which runs on another, and ringing whenever a job may
/// be waiting: one was queued, a machine changed, or the stream opened
/// after being away.
fn listen(home: &Home) -> std::sync::mpsc::Receiver<()> {
    use crate::events::Heard;
    use crate::mirror::engine::{self, Nudge, Role};
    use stemma_core::event::Event;

    let (ring, woken) = std::sync::mpsc::channel();
    let (nudges, heard) = engine::channel();

    let keeping = home.clone();
    std::thread::spawn(move || {
        engine::run(&keeping, Role::Background, &heard, |_| {});
    });

    let home = home.clone();
    std::thread::spawn(move || {
        let never = std::sync::atomic::AtomicBool::new(false);

        crate::events::follow(&home, &never, |heard| {
            let work = match &heard {
                Heard::Connected => {
                    let _ = nudges.send(Nudge::Connected);
                    true
                }
                Heard::Changed(event) => {
                    let _ = nudges.send(Nudge::Heard(event.clone()));
                    match event {
                        Event::Job { state, .. } => state == "queued",
                        Event::Machine { .. } | Event::Reset => true,
                        _ => false,
                    }
                }
                Heard::Lost(_) => false,
            };

            if work {
                let _ = ring.send(());
            }
        });
    });

    woken
}

/// Does one job and reports what came of it.
#[allow(clippy::too_many_arguments)]
fn do_one(
    home: &Home,
    config: &Config,
    server: &Server,
    credentials: &Credentials,
    handed: &crate::api::Handed,
    agents: &mut [Detected],
    out: &mut impl Write,
) -> Result<()> {
    writeln!(
        out,
        "{} {} · {} on {}/{}",
        handed.job, handed.task, handed.agent, handed.group, handed.side
    )?;
    out.flush()?;

    let workspace = std::env::temp_dir()
        .join("stemma-jobs")
        .join(handed.job.replace(':', "-"));
    let workspace_text = workspace.display().to_string();
    let cancelled = std::sync::atomic::AtomicBool::new(handed.cancel_requested);
    if server.job_progress(
        &credentials.token,
        &handed.job,
        Some(&workspace_text),
        "preparing the workspace",
    )? {
        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    let completed = std::thread::scope(|scope| -> Result<work::Completed> {
        let (finished, waiting) = std::sync::mpsc::sync_channel(1);
        let cancelled_for_worker = &cancelled;
        scope.spawn(move || {
            let completed = match crate::mirror::Instance::connect(home) {
                Ok(instance) => work::carry_out_controlled(
                    home,
                    config,
                    &instance,
                    handed,
                    cancelled_for_worker,
                ),
                Err(error) => work::Completed {
                    outcome: stemma_core::job::Outcome::out_of_reach(format!("{error:#}")),
                    proposal: None,
                    workspace: None,
                },
            };
            let _ = finished.send(completed);
        });
        wait_with_heartbeat(waiting, Duration::from_secs(BEAT_SECONDS), || {
            // Keep working through temporary network failures.
            if let Err(error) = server.beat(&credentials.token, agents) {
                writeln!(out, "{}: {error:#}", credentials.server)?;
                out.flush()?;
            }
            match server.job_progress(
                &credentials.token,
                &handed.job,
                Some(&workspace_text),
                "the agent is working",
            ) {
                Ok(true) => cancelled.store(true, std::sync::atomic::Ordering::Relaxed),
                Ok(false) => {}
                Err(error) => {
                    writeln!(out, "{}: {error:#}", credentials.server)?;
                    out.flush()?;
                }
            }
            Ok(())
        })
    })?;
    let outcome = &completed.outcome;

    writeln!(out, "  {}", outcome.said())?;
    if let Some(workspace) = &completed.workspace {
        writeln!(out, "  workspace at {}", workspace.display())?;
    }

    // Raw output carries this machine's account, quota and plan, so it
    // is named here and goes nowhere.
    if work::output_exists(home, &handed.job) {
        writeln!(
            out,
            "  it is at {}",
            work::output_of(home, &handed.job).display()
        )?;
    }
    out.flush()?;

    // What the job taught about this machine is worth reporting at
    // once, so the instance stops handing it work it cannot do.
    remember(agents, outcome, &handed.agent);

    server
        .finish_job(
            &credentials.token,
            &handed.job,
            &stemma_core::proposal::Completion {
                outcome: completed.outcome.clone(),
                proposal: completed.proposal.clone(),
            },
        )
        .with_context(|| {
            format!(
                "cannot tell {} how {} ended",
                credentials.server, handed.job
            )
        })?;

    if completed.proposal.is_some() {
        writeln!(out, "  a proposal is waiting for review")?;
    }

    Ok(())
}

/// Waits for work while keeping its machine visible to the instance.
fn wait_with_heartbeat<T>(
    waiting: std::sync::mpsc::Receiver<T>,
    interval: Duration,
    mut heartbeat: impl FnMut() -> Result<()>,
) -> Result<T> {
    loop {
        match waiting.recv_timeout(interval) {
            Ok(completed) => return Ok(completed),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => heartbeat()?,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                bail!("the job worker stopped without reporting an outcome");
            }
        }
    }
}

/// Writes what a job taught about an agent into what this machine
/// reports.
fn remember(agents: &mut [Detected], outcome: &Outcome, agent: &str) {
    let Some(code) = outcome.code() else {
        return;
    };

    let availability = match code {
        "agent_not_found" => stemma_core::machine::Availability::NotInstalled,
        "agent_not_authenticated" => stemma_core::machine::Availability::NotSignedIn,
        "agent_rate_limited" => stemma_core::machine::Availability::RateLimited,
        _ => return,
    };

    if let Some(found) = agents
        .iter_mut()
        .find(|found| found.agent.as_str() == agent)
    {
        found.availability = availability;
    }
}

/// What supervises this machine, where anything does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supervisor {
    /// A Linux machine running systemd, with a user manager.
    Systemd,
    /// A Mac, with launchd.
    Launchd,
    /// Something else, which this cannot write a unit for.
    Unknown,
}

impl Supervisor {
    /// What is here.
    pub fn here() -> Self {
        if Path::new("/run/systemd/system").is_dir() {
            return Self::Systemd;
        }
        if cfg!(target_os = "macos") {
            return Self::Launchd;
        }

        Self::Unknown
    }
}

/// What the unit is called.
pub const UNIT_NAME: &str = "stemma.service";

/// What the launch agent is called.
pub const AGENT_NAME: &str = "org.stemma.daemon.plist";

/// Puts the daemon under whatever supervises this machine.
pub fn install(home: &Home, out: &mut impl Write) -> Result<()> {
    // What is supervised is the command line, not whatever asked for it
    // to be installed. Asked from the application, `current_exe` would
    // put the application itself in the plist, and every login would
    // open a window.
    let executable = crate::program::command_line()?;
    let moved = std::env::var(crate::home::HOME_VARIABLE).ok();

    match Supervisor::here() {
        Supervisor::Systemd => {
            let path = systemd_path()?;
            write(&path, &unit(&executable, moved.as_deref()))?;

            writeln!(out, "Wrote {}.", path.display())?;
            enable(
                out,
                "systemctl",
                &[
                    &["--user", "daemon-reload"],
                    &["--user", "enable", "--now", UNIT_NAME],
                ],
            )?;
        }
        Supervisor::Launchd => {
            let path = launchd_path(home)?;
            write(&path, &plist(&executable, home, moved.as_deref()))?;

            writeln!(out, "Wrote {}.", path.display())?;

            let uid = current_uid().unwrap_or_default();
            enable(
                out,
                "launchctl",
                &[&[
                    "bootstrap",
                    &format!("gui/{uid}"),
                    &path.display().to_string(),
                ]],
            )?;
        }
        Supervisor::Unknown => {
            writeln!(
                out,
                "This machine is run by neither systemd nor launchd, so there is \
                 nothing here to install into."
            )?;
            writeln!(out, "Run `stemma daemon run` under a service manager.")?;
        }
    }

    Ok(())
}

/// Runs the command that turns the service on, and says so either way.
///
/// Failing to enable it is not failing to install it: the file is
/// written, and somebody can run one command.
fn enable(out: &mut impl Write, program: &str, runs: &[&[&str]]) -> Result<()> {
    for arguments in runs {
        let ran = Command::new(program).args(*arguments).output();

        let happy = ran
            .as_ref()
            .map(|output| output.status.success())
            .unwrap_or(false);

        if !happy {
            writeln!(out, "The service is installed but not started. Run:")?;
            writeln!(out, "  {program} {}", arguments.join(" "))?;

            return Ok(());
        }
    }

    writeln!(
        out,
        "The daemon is running and will start with this machine."
    )?;

    Ok(())
}

/// Where a systemd user unit goes.
fn systemd_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("this account has no home directory")?;

    Ok(Path::new(&home)
        .join(".config/systemd/user")
        .join(UNIT_NAME))
}

/// Where a launch agent goes.
fn launchd_path(_home: &Home) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("this account has no home directory")?;

    Ok(Path::new(&home)
        .join("Library/LaunchAgents")
        .join(AGENT_NAME))
}

/// Writes a file, making the directory above it.
fn write(path: &Path, content: &str) -> Result<()> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
    }

    std::fs::write(path, content).with_context(|| format!("cannot write {}", path.display()))
}

/// This account's number, for the one command that wants it.
fn current_uid() -> Option<String> {
    let output = Command::new("id").arg("-u").output().ok()?;

    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The systemd user unit.
fn unit(executable: &Path, moved: Option<&str>) -> String {
    let moved = match moved {
        // A machine keeping its Stemma somewhere else has to keep it
        // there when systemd starts it, or the daemon comes up as
        // somebody else.
        Some(root) => format!("Environment={}={root}\n", crate::home::HOME_VARIABLE),
        None => String::new(),
    };
    let path = format!("Environment=PATH={}\n", programs_path());

    format!(
        "[Unit]\n\
         Description=Stemma daemon\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart={} daemon run\n\
         {path}\
         {moved}\
         Restart=on-failure\n\
         RestartSec=10s\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        executable.display()
    )
}

/// The path the daemon is given to find programs with.
///
/// A service is not started from anybody's shell, so it inherits a path
/// of four system directories and nothing the person has installed. An
/// agent sitting in `/opt/homebrew/bin` is then not merely unusable but
/// reported to the group as not installed, which is a lie about their
/// machine. So the places agents are actually installed into travel
/// with the service, and the agents themselves get a path they can run
/// the tools they need from.
fn programs_path() -> String {
    crate::agents::where_programs_are()
        .iter()
        .map(|place| place.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// The launch agent.
fn plist(executable: &Path, home: &Home, moved: Option<&str>) -> String {
    let log = home.state_dir().join("daemon.log");
    let moved = match moved {
        Some(root) => format!(
            "<key>{}</key><string>{root}</string>",
            crate::home::HOME_VARIABLE
        ),
        None => String::new(),
    };
    let environment = format!(
        "    <key>EnvironmentVariables</key>\n    \
         <dict><key>PATH</key><string>{}</string>{moved}</dict>\n",
        programs_path()
    );

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n    \
         <key>Label</key><string>org.stemma.daemon</string>\n    \
         <key>ProgramArguments</key>\n    \
         <array><string>{}</string><string>daemon</string><string>run</string></array>\n    \
         <key>RunAtLoad</key><true/>\n    \
         <key>KeepAlive</key><true/>\n    \
         <key>StandardOutPath</key><string>{}</string>\n    \
         <key>StandardErrorPath</key><string>{}</string>\n\
         {environment}\
         </dict>\n\
         </plist>\n",
        executable.display(),
        log.display(),
        log.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_for_a_job_keeps_sending_heartbeats() {
        let (finished, waiting) = std::sync::mpsc::channel();
        let mut beats = 0;
        let result = wait_with_heartbeat(waiting, Duration::from_millis(1), || {
            beats += 1;
            if beats == 2 {
                finished.send("completed").unwrap();
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(result, "completed");
        assert_eq!(beats, 2);
    }

    #[test]
    fn a_lost_worker_is_reported_instead_of_waiting_forever() {
        let (finished, waiting) = std::sync::mpsc::channel::<()>();
        drop(finished);
        assert!(wait_with_heartbeat(waiting, Duration::from_secs(30), || Ok(())).is_err());
    }
    use stemma_core::id::Slug;
    use stemma_core::machine::Availability;

    fn home() -> Home {
        Home::locate(&crate::home::Environment::from_pairs([(
            crate::home::HOME_VARIABLE,
            "/tmp/stemma-daemon-test",
        )]))
    }

    #[test]
    fn the_unit_runs_the_daemon_and_comes_back_after_a_failure() {
        let written = unit(Path::new("/usr/local/bin/stemma"), None);

        assert!(written.contains("ExecStart=/usr/local/bin/stemma daemon run"));
        assert!(written.contains("Restart=on-failure"));
        assert!(written.contains("WantedBy=default.target"));
        assert!(!written.contains("STEMMA_HOME"));
    }

    #[test]
    fn a_machine_that_keeps_stemma_elsewhere_keeps_it_there_when_it_is_started() {
        let written = unit(Path::new("/usr/local/bin/stemma"), Some("/data/stemma"));
        assert!(written.contains("Environment=STEMMA_HOME=/data/stemma"));

        let agent = plist(
            Path::new("/usr/local/bin/stemma"),
            &home(),
            Some("/data/stemma"),
        );
        assert!(agent.contains("<key>STEMMA_HOME</key><string>/data/stemma</string>"));
    }

    #[test]
    fn the_service_carries_a_path_an_agent_can_be_found_on() {
        let agent = plist(Path::new("/usr/local/bin/stemma"), &home(), None);
        let service = unit(Path::new("/usr/local/bin/stemma"), None);

        // What the system would otherwise give it, and where nothing is.
        for written in [&agent, &service] {
            assert!(written.contains("PATH"), "the service carries no path");
            assert!(
                written.contains("/opt/homebrew/bin") || written.contains("/usr/local/bin"),
                "the path holds nowhere an agent is installed"
            );
        }
    }

    #[test]
    fn the_launch_agent_names_the_program_and_keeps_it_running() {
        let written = plist(Path::new("/usr/local/bin/stemma"), &home(), None);

        assert!(written.contains("<string>/usr/local/bin/stemma</string>"));
        assert!(written.contains("<key>KeepAlive</key><true/>"));
        assert!(written.contains("daemon.log"));
    }

    #[test]
    fn a_job_that_could_not_try_changes_what_this_machine_says_it_can_do() {
        let claude = Slug::parse("claude").unwrap();
        let mut agents = vec![Detected::ready(claude.clone(), Some("1.0".to_owned()))];

        remember(
            &mut agents,
            &Outcome::from_failure(&stemma_core::agent::AgentFailure::AgentNotAuthenticated {
                agent: claude.clone(),
            }),
            "claude",
        );

        assert_eq!(agents[0].availability, Availability::NotSignedIn);

        // An agent that ran and broke says nothing about the machine.
        remember(
            &mut agents,
            &Outcome::Produced {
                said: "there is an answer".to_owned(),
            },
            "claude",
        );

        assert_eq!(agents[0].availability, Availability::NotSignedIn);
    }
}

//! The loop that takes work.
//!
//! It says it is there, asks for a job, does it, reports the type of
//! what happened, and asks again. Nothing else: the instance keeps the
//! queue, this machine does the work, and the two never swap jobs.
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
/// With `once`, it takes at most one job and returns, which is what a
/// machine woken by something else wants and what a test can watch.
pub fn run(home: &Home, once: bool, out: &mut impl Write) -> Result<()> {
    let credentials = session::required(home)?;
    let server = Server::at(&credentials.server)?;
    let config = Config::read(&home.config_file())?;

    if server.node(&credentials.token)?.is_none() {
        bail!("this machine is not offered for work: run `stemma node init`");
    }

    let mut agents = crate::agents::look(home, &config);
    let mut looked = Instant::now();

    writeln!(
        out,
        "Taking work from {} as {}.",
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
                writeln!(out, "Nothing is waiting.")?;
                return Ok(());
            }

            std::thread::sleep(Duration::from_secs(BEAT_SECONDS));
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

    let completed = work::carry_out(home, config, handed);
    let outcome = &completed.outcome;

    writeln!(out, "  {}", outcome.said())?;

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
    let executable = std::env::current_exe().context("cannot find this program")?;
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
            writeln!(
                out,
                "Run `stemma daemon run` under whatever supervises this machine."
            )?;
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
            writeln!(out, "It is written and not started. Run:")?;
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

    format!(
        "[Unit]\n\
         Description=Stemma daemon\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart={} daemon run\n\
         {moved}\
         Restart=on-failure\n\
         RestartSec=10s\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        executable.display()
    )
}

/// The launch agent.
fn plist(executable: &Path, home: &Home, moved: Option<&str>) -> String {
    let log = home.state_dir().join("daemon.log");
    let environment = match moved {
        Some(root) => format!(
            "    <key>EnvironmentVariables</key>\n    \
             <dict><key>{}</key><string>{root}</string></dict>\n",
            crate::home::HOME_VARIABLE
        ),
        None => String::new(),
    };

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

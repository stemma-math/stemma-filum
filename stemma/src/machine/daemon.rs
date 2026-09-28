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
//! machine it runs on; [`super::service`] is how it is put there.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use stemma_core::job::Outcome;
use stemma_core::machine::{BEAT_SECONDS, Detected};

use crate::account::credentials::Credentials;
use crate::account::session;
use crate::api::{Handed, Server};
use crate::config::Config;
use crate::home::Home;
use crate::job::work;
use crate::mirror::{self, Instance, Mirror, Store, Which};

/// How often to look for agents again.
///
/// Somebody installs one while the daemon is running, and the instance
/// should find out without being restarted.
const LOOK_EVERY: Duration = Duration::from_secs(10 * 60);

/// How long to wait after failing to reach the instance.
const AFTER_TROUBLE: Duration = Duration::from_secs(30);

/// How many times a running daemon tries to say how a job ended before
/// it gives up on saying so.
///
/// The outcome is the only thing the instance hears of a job, so a
/// network that is away for a moment must not lose it.
const FINISH_ATTEMPTS: u32 = 10;

/// Runs the loop.
///
/// It keeps this machine's lines in step while the application is closed
/// — the same engine, one at a time — and, on a machine registered to run
/// jobs, takes them. With `once`, it does one round and takes at most one
/// job and returns, which is what a machine woken by something else wants
/// and what a test can watch.
pub fn run(home: &Home, once: bool, out: &mut impl Write) -> Result<()> {
    if !once {
        // Said before anything can fail, so a daemon that keeps being
        // restarted still says which build it is.
        if let Err(error) = Started::record(home) {
            writeln!(out, "{error:#}")?;
        }
    }

    let (credentials, server) = session::connect(home)?;
    let config = Config::read(&home.config_file())?;
    let node = server.node(&credentials.token)?.is_some();

    if once {
        if !Store::at(home.state_dir()).window_open() {
            let report = Mirror::at(home)?.step(&Instance::connect(home)?, true, &Which::All);
            mirror::tell(&report, out)?;
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

    let mut agents = crate::job::agents::look(home, &config);
    let mut looked = Instant::now();

    writeln!(
        out,
        "Running jobs from {} as {}.",
        credentials.server, credentials.user
    )?;
    out.flush()?;

    loop {
        if looked.elapsed() >= LOOK_EVERY {
            agents = crate::job::agents::look(home, &config);
            looked = Instant::now();
        }

        let taken = server
            .beat(&credentials.token, &agents)
            .and_then(|_| server.take_job(&credentials.token));
        let taken = match taken {
            Ok(taken) => taken,
            Err(error) if once => return Err(error),
            Err(error) => {
                say_trouble(out, &credentials, &error)?;
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

        let job = Job {
            home,
            config: &config,
            server: &server,
            credentials: &credentials,
            handed: &handed,
        };
        let done = job.carry_out(&mut agents, !once, out);

        match done {
            Ok(()) => {}
            Err(error) if once => return Err(error),
            // One job that could not be reported is not a reason to stop
            // taking the next one.
            Err(error) => say_trouble(out, &credentials, &error)?,
        }

        if once {
            return Ok(());
        }
    }
}

/// Says what went wrong talking to the instance, on the daemon's output.
fn say_trouble(
    out: &mut impl Write,
    credentials: &Credentials,
    error: &anyhow::Error,
) -> Result<()> {
    writeln!(out, "{}: {error:#}", credentials.server)?;
    out.flush()?;

    Ok(())
}

/// Listens to the instance on a thread of its own, passing everything
/// to the engine, which runs on another, and ringing whenever a job may
/// be waiting: one was queued, a machine changed, or the stream opened
/// after being away.
fn listen(home: &Home) -> Receiver<()> {
    use crate::api::events::Heard;
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
        let never = AtomicBool::new(false);

        crate::api::events::follow(&home, &never, |heard| {
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

/// One job this machine took, and what it needs to do it.
struct Job<'a> {
    home: &'a Home,
    config: &'a Config,
    server: &'a Server,
    credentials: &'a Credentials,
    handed: &'a Handed,
}

impl Job<'_> {
    /// Does the job and reports what came of it.
    ///
    /// Reporting progress is worth trying and not worth failing over;
    /// reporting how it ended is the one thing that has to arrive, and a
    /// `patient` daemon tries again for a while when the instance is
    /// away.
    fn carry_out(
        &self,
        agents: &mut [Detected],
        patient: bool,
        out: &mut impl Write,
    ) -> Result<()> {
        let Job {
            home,
            config,
            handed,
            ..
        } = *self;

        writeln!(
            out,
            "{} {} · {} on {}/{}",
            handed.job, handed.task, handed.agent, handed.group, handed.side
        )?;
        out.flush()?;

        let workspace = crate::job::workspace::root_of(home, &handed.job);
        let workspace_text = workspace.display().to_string();
        let cancelled = AtomicBool::new(handed.cancel_requested);
        self.progress(&workspace_text, "preparing the workspace", &cancelled, out)?;

        let completed = std::thread::scope(|scope| -> Result<work::Completed> {
            let (finished, waiting) = std::sync::mpsc::sync_channel(1);
            let cancelled = &cancelled;
            scope.spawn(move || {
                let completed = match Instance::connect(home) {
                    Ok(instance) => {
                        work::carry_out_controlled(home, config, &instance, handed, cancelled)
                    }
                    Err(error) => work::Completed {
                        outcome: Outcome::out_of_reach(format!("{error:#}")),
                        proposal: None,
                        workspace: None,
                    },
                };
                let _ = finished.send(completed);
            });
            wait_with_heartbeat(waiting, Duration::from_secs(BEAT_SECONDS), || {
                // Keep working through temporary network failures.
                if let Err(error) = self.server.beat(&self.credentials.token, agents) {
                    say_trouble(out, self.credentials, &error)?;
                }
                self.progress(&workspace_text, "the agent is working", cancelled, out)
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

        self.finish(&completed, patient, out)?;

        if completed.proposal.is_some() {
            writeln!(out, "  a proposal is waiting for review")?;
        }

        Ok(())
    }

    /// Tells the instance how far the job has got, and notes when its
    /// launcher asked for it to stop. Not reaching the instance is said
    /// and passed over: the job goes on either way.
    fn progress(
        &self,
        workspace: &str,
        progress: &str,
        cancelled: &AtomicBool,
        out: &mut impl Write,
    ) -> Result<()> {
        match self.server.job_progress(
            &self.credentials.token,
            &self.handed.job,
            Some(workspace),
            progress,
        ) {
            Ok(true) => cancelled.store(true, Ordering::Relaxed),
            Ok(false) => {}
            Err(error) => say_trouble(out, self.credentials, &error)?,
        }

        Ok(())
    }

    /// Tells the instance how the job ended.
    fn finish(
        &self,
        completed: &work::Completed,
        patient: bool,
        out: &mut impl Write,
    ) -> Result<()> {
        let completion = stemma_core::proposal::Completion {
            outcome: completed.outcome.clone(),
            proposal: completed.proposal.clone(),
        };
        let attempts = if patient { FINISH_ATTEMPTS } else { 1 };
        let mut tried = 0;

        loop {
            tried += 1;
            match self
                .server
                .finish_job(&self.credentials.token, &self.handed.job, &completion)
            {
                Ok(()) => return Ok(()),
                Err(error) if tried < attempts => {
                    say_trouble(out, self.credentials, &error)?;
                    std::thread::sleep(AFTER_TROUBLE);
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "cannot tell {} how {} ended",
                            self.credentials.server, self.handed.job
                        )
                    });
                }
            }
        }
    }
}

/// Waits for work while keeping its machine visible to the instance.
fn wait_with_heartbeat<T>(
    waiting: Receiver<T>,
    interval: Duration,
    mut heartbeat: impl FnMut() -> Result<()>,
) -> Result<T> {
    loop {
        match waiting.recv_timeout(interval) {
            Ok(completed) => return Ok(completed),
            Err(RecvTimeoutError::Timeout) => heartbeat()?,
            Err(RecvTimeoutError::Disconnected) => {
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

/// What the daemon writes about itself as it starts.
///
/// The application reads it to tell whether the daemon running is the
/// one it carries: after an update, the old one keeps running until
/// something restarts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Started {
    /// Its process.
    pub pid: u32,
    /// The client version it was built as.
    pub version: String,
    /// When it started, in seconds since the epoch.
    pub at: u64,
}

impl Started {
    fn path(home: &Home) -> PathBuf {
        home.state_dir().join("daemon.toml")
    }

    /// What the daemon last wrote, if anything.
    pub fn read(home: &Home) -> Option<Self> {
        toml::from_str(&std::fs::read_to_string(Self::path(home)).ok()?).ok()
    }

    /// Writes that this process is the daemon.
    fn record(home: &Home) -> Result<()> {
        let started = Self {
            pid: std::process::id(),
            version: VERSION.to_owned(),
            at: seconds(SystemTime::now()),
        };
        let text = toml::to_string(&started).context("cannot describe the daemon")?;

        crate::storage::replace(&Self::path(home), &text)
    }

    /// Whether this is the daemon `program` starts, running now as
    /// `pid`: the same version, the same process, and started after
    /// the program was last replaced.
    pub fn is_current(&self, pid: Option<u32>, program: &Path) -> bool {
        let replaced = std::fs::metadata(program)
            .and_then(|about| about.modified())
            .map(seconds)
            .unwrap_or(0);

        self.version == VERSION && pid.is_none_or(|pid| pid == self.pid) && self.at >= replaced
    }
}

/// The version of this client.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A time as whole seconds since the epoch.
fn seconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use stemma_core::id::Slug;
    use stemma_core::machine::Availability;

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

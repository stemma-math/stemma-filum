//! The agents this machine turns out to have.
//!
//! Stemma holds no key for any of them. They are installed and signed in
//! by their owner, on their own machine, and all this does is find them,
//! run them, and turn what happened into a type the group can read.
//!
//! What comes out of an agent carries the owner's account, their quota
//! and their plan, so it is written to a file here and never sent
//! anywhere. What travels is the type and one line.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use stemma_core::agent::AgentFailure;
use stemma_core::id::Slug;
use stemma_core::machine::{Availability, Detected};

use crate::config::Config;
use crate::home::Home;

/// How long to wait for an agent to say which version it is.
const VERSION_LIMIT: Duration = Duration::from_secs(10);

/// How long an agent may work before this gives up on it.
const RUN_LIMIT: Duration = Duration::from_secs(20 * 60);

/// How often to look at whether the agent has finished.
const POLL: Duration = Duration::from_millis(50);

/// How this build invokes one of the agents in the catalogue.
struct Invocation {
    /// Which agent.
    slug: &'static str,
    /// What its program is called.
    program: &'static str,
    /// What makes it print its version.
    version: &'static [&'static str],
    /// What makes it answer one prompt and stop. The prompt is the last
    /// argument.
    run: &'static [&'static str],
}

/// How each agent is invoked.
///
/// These are the vendors' own commands, and they change. Anything here
/// can be overridden per machine in the configuration file, which is
/// also how an agent installed somewhere unusual is found.
const INVOCATIONS: &[Invocation] = &[
    Invocation {
        slug: "claude",
        program: "claude",
        version: &["--version"],
        run: &["-p"],
    },
    Invocation {
        slug: "codex",
        program: "codex",
        version: &["--version"],
        run: &["exec"],
    },
    Invocation {
        slug: "gemini",
        program: "gemini",
        version: &["--version"],
        run: &["-p"],
    },
];

/// How this build invokes one agent.
fn invocation(slug: &str) -> Option<&'static Invocation> {
    INVOCATIONS.iter().find(|known| known.slug == slug)
}

/// The program an agent is run as, if this machine has one.
pub fn program(config: &Config, slug: &str) -> Option<PathBuf> {
    if let Some(given) = config
        .agents
        .get(slug)
        .and_then(|agent| agent.command.as_ref())
    {
        let given = PathBuf::from(given);

        return runnable(&given).then_some(given);
    }

    on_path(invocation(slug)?.program)
}

/// Looks for every agent this build knows about.
///
/// Every one of them is in the answer, including the ones that are not
/// here: an agent somebody could install is worth naming, and a shorter
/// list would say nothing about why.
pub fn look(home: &Home, config: &Config) -> Vec<Detected> {
    stemma_core::machine::CATALOGUE
        .iter()
        .map(|known| {
            let name = known.id();

            match program(config, known.slug) {
                Some(program) => {
                    let mut detected =
                        Detected::ready(name, version_of(home, &program, known.slug));
                    if let Some(configured) = config.agents.get(known.slug) {
                        detected.models.clone_from(&configured.models);
                        detected.efforts.clone_from(&configured.efforts);
                    }
                    detected
                }
                None => Detected::missing(name),
            }
        })
        .collect()
}

/// What version an agent says it is, where it will say.
fn version_of(home: &Home, program: &Path, slug: &str) -> Option<String> {
    let arguments = invocation(slug)?.version;
    let mut command = Command::new(program);
    command.args(arguments);

    let finished = wait_for(
        &mut command,
        &scratch(home).ok()?.join(format!("{slug}.version")),
        VERSION_LIMIT,
    )
    .ok()?;

    // An agent that is installed and will not say which version it is
    // is still installed, which is the part that matters here.
    finished
        .output
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
}

/// What became of running an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    /// This machine does not have it.
    Missing,
    /// It ran, one way or another.
    Finished(Finished),
}

/// One run of a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// What it exited with, where it exited on its own.
    pub status: Option<i32>,
    /// Everything it said, which stays on this machine.
    pub output: String,
    /// Whether it was still going when the time ran out.
    pub gave_up: bool,
    /// Whether the launcher cancelled the run.
    pub cancelled: bool,
}

impl Finished {
    /// Whether it ended the way a program that worked ends.
    pub fn is_clean(&self) -> bool {
        !self.gave_up && !self.cancelled && self.status == Some(0)
    }
}

/// Runs an agent against one prompt, keeping what it says here.
pub fn run(config: &Config, slug: &str, prompt: &str, into: &Path) -> Result<Ran> {
    let Some(program) = program(config, slug) else {
        return Ok(Ran::Missing);
    };
    let Some(invocation) = invocation(slug) else {
        return Ok(Ran::Missing);
    };

    let mut command = Command::new(program);
    command.args(invocation.run).arg(prompt);

    Ok(Ran::Finished(wait_for(&mut command, into, RUN_LIMIT)?))
}

/// How long trying an agent out may take.
const TRY_LIMIT: Duration = Duration::from_secs(2 * 60);

/// Asks an agent for one word, to find out whether it is installed and
/// signed in. What it says stays in this machine's cache.
pub fn try_out(home: &Home, config: &Config, slug: &str) -> Result<Ran> {
    let Some(program) = program(config, slug) else {
        return Ok(Ran::Missing);
    };
    let Some(invocation) = invocation(slug) else {
        return Ok(Ran::Missing);
    };

    let directory = scratch(home)?.join("tried");
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;

    let mut command = Command::new(program);
    command
        .current_dir(&directory)
        .args(invocation.run)
        .arg("Answer with the single word: ready");

    Ok(Ran::Finished(wait_for(
        &mut command,
        &directory.join(format!("{slug}.log")),
        TRY_LIMIT,
    )?))
}

/// Runs an agent inside one prepared workspace with its vendor's sandbox
/// enabled and the job API ahead of the ordinary `PATH`.
#[allow(clippy::too_many_arguments)]
pub fn run_in_workspace(
    config: &Config,
    slug: &str,
    prompt: &str,
    workspace: &Path,
    api_bin: &Path,
    into: &Path,
    model: &str,
    effort: Option<&str>,
    limit: Duration,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Ran> {
    let Some(program) = program(config, slug) else {
        return Ok(Ran::Missing);
    };
    let mut command = Command::new(program);
    match slug {
        "codex" => {
            command.args([
                "exec",
                "--sandbox",
                "workspace-write",
                "--config",
                "sandbox_workspace_write.network_access=true",
            ]);
            if model != "default" {
                command.args(["--model", model]);
            }
            if let Some(effort) = effort {
                command.arg("--config");
                command.arg(format!("model_reasoning_effort=\"{effort}\""));
            }
            command.arg(prompt);
        }
        "claude" => {
            command.args(["-p", "--permission-mode", "acceptEdits"]);
            if model != "default" {
                command.args(["--model", model]);
            }
            command.arg(prompt);
        }
        "gemini" => {
            command.arg("--sandbox");
            if model != "default" {
                command.args(["--model", model]);
            }
            command.arg("-p");
            command.arg(prompt);
        }
        _ => return Ok(Ran::Missing),
    }
    let mut paths = vec![api_bin.to_owned()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    command
        .current_dir(workspace)
        .env("PATH", std::env::join_paths(paths)?)
        .env(
            crate::job::answer::STATE_VARIABLE,
            workspace.join(".stemma/job.json"),
        );

    Ok(Ran::Finished(wait_for_controlled(
        &mut command,
        into,
        limit,
        cancelled,
    )?))
}

/// Runs a program, writing what it says to a file, and gives up on it
/// after a while.
///
/// Everything goes to the file rather than to a pipe this has to drain:
/// a program that fills a pipe nobody is reading stops, and a daemon
/// that stops waiting for a pipe is a daemon that never comes back.
fn wait_for(command: &mut Command, into: &Path, limit: Duration) -> Result<Finished> {
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    wait_for_controlled(command, into, limit, &cancelled)
}

fn wait_for_controlled(
    command: &mut Command,
    into: &Path,
    limit: Duration,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Finished> {
    if let Some(directory) = into.parent() {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
    }

    let file = File::create(into).with_context(|| format!("cannot write {}", into.display()))?;
    let errors = file
        .try_clone()
        .with_context(|| format!("cannot write {}", into.display()))?;

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(errors))
        .spawn()
        .with_context(|| format!("cannot run {:?}", command.get_program()))?;

    let deadline = Instant::now() + limit;
    let mut gave_up = false;
    let mut was_cancelled = false;

    let status = loop {
        match child.try_wait().context("cannot wait for the agent")? {
            Some(status) => break status.code(),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                gave_up = true;

                break None;
            }
            None if cancelled.load(std::sync::atomic::Ordering::Relaxed) => {
                let _ = child.kill();
                let _ = child.wait();
                was_cancelled = true;
                break None;
            }
            None => std::thread::sleep(POLL),
        }
    };

    Ok(Finished {
        status,
        output: std::fs::read_to_string(into).unwrap_or_default(),
        gave_up,
        cancelled: was_cancelled,
    })
}

/// Words that mean the owner has not signed this agent in.
///
/// These are the vendors' own words and they change. Getting one wrong
/// costs a failure reported as the agent breaking rather than as the
/// machine needing attention, which is a worse message and not a wrong
/// one.
const NOT_SIGNED_IN: &[&str] = &[
    "not logged in",
    "please log in",
    "please sign in",
    "not authenticated",
    "unauthorized",
    "authentication required",
    "invalid api key",
];

/// Words that mean the vendor is refusing for now.
const RATE_LIMITED: &[&str] = &[
    "rate limit",
    "rate-limit",
    "too many requests",
    "quota",
    "usage limit",
];

/// What went wrong, in the one vocabulary the group is shown.
pub fn trouble(agent: &Slug, ran: &Ran) -> Option<AgentFailure> {
    let finished = match ran {
        Ran::Missing => {
            return Some(AgentFailure::AgentNotFound {
                agent: agent.clone(),
            });
        }
        Ran::Finished(finished) => finished,
    };

    if finished.is_clean() {
        return None;
    }

    let said = finished.output.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|word| said.contains(word));

    if any(NOT_SIGNED_IN) {
        return Some(AgentFailure::AgentNotAuthenticated {
            agent: agent.clone(),
        });
    }
    if any(RATE_LIMITED) {
        return Some(AgentFailure::AgentRateLimited {
            agent: agent.clone(),
        });
    }

    Some(AgentFailure::AgentError {
        agent: agent.clone(),
        summary: match (finished.gave_up, finished.status) {
            (true, _) => format!(
                "it was still running after {} minutes",
                RUN_LIMIT.as_secs() / 60
            ),
            (false, Some(status)) => format!("it exited with status {status}"),
            (false, None) => "it was stopped".to_owned(),
        },
    })
}

/// What a run of an agent says about the machine it ran on.
pub fn learned(ran: &Ran, agent: &Slug) -> Option<(Slug, Availability)> {
    let failure = trouble(agent, ran)?;

    Availability::learned_from(&failure).map(|availability| (agent.clone(), availability))
}

/// Where the scratch files an agent leaves behind go.
fn scratch(home: &Home) -> Result<PathBuf> {
    let directory = home.cache_dir().join("agents");

    std::fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;

    Ok(directory)
}

/// Where the raw output of one job is kept.
///
/// In the cache, because it can be thrown away, and on this machine,
/// because it is the owner's and nobody else's.
pub fn output_path(home: &Home, job: &str) -> PathBuf {
    home.cache_dir()
        .join("jobs")
        .join(format!("{}.log", job.replace(':', "-")))
}

/// The first place this machine keeps programs that has one of this
/// name.
pub fn on_path(program: &str) -> Option<PathBuf> {
    where_programs_are()
        .into_iter()
        .map(|directory| directory.join(program))
        .find(|candidate| runnable(candidate))
}

/// Everywhere a program of an agent's might be.
///
/// The path first, because somebody who put one somewhere meant it.
/// Then the places installers use, because the path is not the one this
/// looks at: a daemon is started by the system, and the system starts
/// it with a path of four directories that no agent has ever been
/// installed into. Without these, an agent sitting right there is
/// reported to the group as not installed.
pub fn where_programs_are() -> Vec<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .and_then(|path| path.to_str().map(str::to_owned))
        .unwrap_or_default();

    beside(&on_path, dirs::home_dir())
}

/// The path, and then the places installers write into beside a home.
///
/// The home is given rather than read so that this can be tried: what
/// it does is only true of a machine somebody has installed things on,
/// and a test has to be able to be that machine.
fn beside(on_path: &str, home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut places: Vec<PathBuf> = on_path
        .split(':')
        .filter(|directory| !directory.is_empty())
        .map(PathBuf::from)
        .collect();

    for known in installed_into(home) {
        if !places.contains(&known) {
            places.push(known);
        }
    }

    places
}

/// The directories the vendors' installers write into.
fn installed_into(home: Option<PathBuf>) -> Vec<PathBuf> {
    let beside = |inside: &str| home.as_ref().map(|home| home.join(inside));

    // The per-user profile of a machine managed by Nix is named after
    // the person, and there is no variable that holds it.
    let profile = home
        .as_ref()
        .and_then(|home| home.file_name())
        .map(|user| Path::new("/etc/profiles/per-user").join(user).join("bin"));

    [
        beside(".local/bin"),
        beside(".claude/local"),
        beside(".bun/bin"),
        beside(".volta/bin"),
        beside(".npm-global/bin"),
        beside(".nix-profile/bin"),
        beside(".cargo/bin"),
        beside(".elan/bin"),
        profile,
        Some(PathBuf::from("/opt/homebrew/bin")),
        Some(PathBuf::from("/usr/local/bin")),
        Some(PathBuf::from("/run/current-system/sw/bin")),
        Some(PathBuf::from("/nix/var/nix/profiles/default/bin")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Whether a path is something this machine can run.
fn runnable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };

    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "stemma-agents-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        root
    }

    /// A program that stands in for an agent.
    fn stub(root: &Path, name: &str, script: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        path
    }

    fn claude() -> Slug {
        Slug::parse("claude").unwrap()
    }

    #[test]
    fn an_agent_is_found_where_it_was_installed_and_not_only_on_the_path() {
        let machine = somewhere("installed");
        let installed = machine.join(".local/bin");
        std::fs::create_dir_all(&installed).unwrap();
        stub(&installed, "claude", "echo 'claude 1.2.3'");

        // What a service is started with on macOS, and what made an
        // agent sitting right there be reported as not installed.
        let places = beside("/usr/bin:/bin:/usr/sbin:/sbin", Some(machine.clone()));

        assert!(
            places
                .iter()
                .map(|place| place.join("claude"))
                .any(|candidate| runnable(&candidate)),
            "an installed agent was not found: {places:?}"
        );

        std::fs::remove_dir_all(&machine).unwrap();
    }

    #[test]
    fn where_a_program_might_be_keeps_the_path_ahead_of_the_guesses() {
        let places = beside("/somewhere/of/my/own", Some(PathBuf::from("/home/ana")));

        assert_eq!(places[0], PathBuf::from("/somewhere/of/my/own"));
        assert!(places.contains(&PathBuf::from("/home/ana/.local/bin")));
        assert!(places.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn every_agent_in_the_catalogue_can_be_invoked() {
        for known in stemma_core::machine::CATALOGUE {
            assert!(
                invocation(known.slug).is_some(),
                "`{}` is offered and cannot be run",
                known.slug
            );
        }
    }

    #[test]
    fn an_agent_this_machine_does_not_have_is_named_and_reported_missing() {
        let root = somewhere("missing");
        let home = crate::home::Home::locate(&crate::home::Environment::from_pairs([(
            crate::home::HOME_VARIABLE,
            root.display().to_string(),
        )]));

        let mut config = Config::default();
        config.agents.insert(
            "claude".to_owned(),
            crate::config::AgentConfig {
                command: Some(root.join("nothing-here").display().to_string()),
                ..Default::default()
            },
        );

        let found = look(&home, &config);
        let claude = found
            .iter()
            .find(|agent| agent.agent.as_str() == "claude")
            .expect("claude is in the catalogue");

        assert_eq!(claude.availability, Availability::NotInstalled);
        assert_eq!(found.len(), stemma_core::machine::CATALOGUE.len());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_agent_that_is_there_is_found_with_its_version() {
        let root = somewhere("found");
        let home = crate::home::Home::locate(&crate::home::Environment::from_pairs([(
            crate::home::HOME_VARIABLE,
            root.display().to_string(),
        )]));

        let program = stub(&root, "claude", "echo 'claude 1.2.3'");
        let mut config = Config::default();
        config.agents.insert(
            "claude".to_owned(),
            crate::config::AgentConfig {
                command: Some(program.display().to_string()),
                ..Default::default()
            },
        );

        let found = look(&home, &config);
        let claude = found
            .iter()
            .find(|agent| agent.agent.as_str() == "claude")
            .unwrap();

        assert_eq!(claude.availability, Availability::Ready);
        assert_eq!(claude.version.as_deref(), Some("claude 1.2.3"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn what_an_agent_says_stays_in_a_file_on_this_machine() {
        let root = somewhere("output");
        let program = stub(&root, "claude", "echo 'the account is ana@example.org'");

        let mut config = Config::default();
        config.agents.insert(
            "claude".to_owned(),
            crate::config::AgentConfig {
                command: Some(program.display().to_string()),
                ..Default::default()
            },
        );

        let kept = root.join("jobs/one.log");
        let ran = run(&config, "claude", "do something", &kept).unwrap();

        let Ran::Finished(finished) = &ran else {
            panic!("the agent did not run");
        };

        assert!(finished.is_clean());
        assert!(finished.output.contains("ana@example.org"));
        assert!(kept.is_file(), "nothing was kept on the machine");
        assert_eq!(trouble(&claude(), &ran), None);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_agent_nobody_signed_in_is_the_machines_business_and_not_the_jobs() {
        let root = somewhere("signed-out");
        let program = stub(&root, "claude", "echo 'Please log in first.' >&2; exit 1");

        let mut config = Config::default();
        config.agents.insert(
            "claude".to_owned(),
            crate::config::AgentConfig {
                command: Some(program.display().to_string()),
                ..Default::default()
            },
        );

        let ran = run(&config, "claude", "do something", &root.join("one.log")).unwrap();
        let failure = trouble(&claude(), &ran).expect("a failure");

        assert_eq!(failure.code(), "agent_not_authenticated");
        assert_eq!(
            learned(&ran, &claude()).map(|(_, availability)| availability),
            Some(Availability::NotSignedIn)
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_agent_that_broke_says_nothing_about_the_machine() {
        let root = somewhere("broken");
        let program = stub(&root, "claude", "echo 'something went wrong' >&2; exit 3");

        let mut config = Config::default();
        config.agents.insert(
            "claude".to_owned(),
            crate::config::AgentConfig {
                command: Some(program.display().to_string()),
                ..Default::default()
            },
        );

        let ran = run(&config, "claude", "do something", &root.join("one.log")).unwrap();
        let failure = trouble(&claude(), &ran).expect("a failure");

        assert_eq!(failure.code(), "agent_error");
        assert_eq!(
            failure.to_string(),
            "`claude` failed: it exited with status 3"
        );
        assert_eq!(learned(&ran, &claude()), None);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_agent_that_is_not_there_never_tried() {
        let ran = Ran::Missing;
        let failure = trouble(&claude(), &ran).unwrap();

        assert_eq!(failure.code(), "agent_not_found");
        assert_eq!(
            failure.attempt(),
            stemma_core::agent::Attempt::NotMade,
            "a missing agent must never look like a failed proof"
        );
    }
}

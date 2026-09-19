//! Asking a machine for something, from the command line.
//!
//! The launcher in the app is anchored to the mathematics, which is
//! where an action belongs when there is a screen. Here there is no
//! screen, so the anchor is where the command is run from: inside a
//! clone, inside a line, against the commit that is checked out.
//!
//! The choice of machine and agent stays deliberate either way. Asking
//! for nobody in particular is something a person says in as many
//! words.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use stemma_core::job::Task;
use stemma_core::line::Name;
use stemma_core::repository;

use crate::api::{Asking, Server};
use crate::build;
use crate::credentials::Credentials;
use crate::git;
use crate::home::Home;
use crate::session;

/// What a person asked for at the terminal.
#[derive(Debug, Clone)]
pub struct Wanted {
    /// The line of work, where they named one.
    pub line: Option<String>,
    /// What is being asked.
    pub task: String,
    /// Which agent.
    pub agent: String,
    /// Which machine, where they chose one.
    pub machine: Option<String>,
    /// Whether they chose this machine.
    pub here: bool,
}

/// Puts a job in the queue.
pub fn run(home: &Home, at: &Path, wanted: &Wanted, out: &mut impl Write) -> Result<String> {
    let credentials = session::required(home)?;
    let server = Server::at(&credentials.server)?;

    let clone = git::containing(at)
        .context("this is not inside a clone: run it from a group's repository")?;
    let manifest = build::read_manifest(&clone)?;
    let commit = git::head(&clone)?;

    let task: Task = wanted.task.parse().context("that is not a task")?;
    let line = line_of(&clone, at, wanted.line.as_deref())?;

    ensure_reachable(&clone, &commit)?;

    let machine = match (&wanted.machine, wanted.here) {
        (Some(_), true) => bail!("choose one machine, not two"),
        (Some(named), false) => Some(named.clone()),
        (None, true) => Some(this_machine(&server, &credentials)?),
        (None, false) => None,
    };

    let job = server.launch_job(
        &credentials.token,
        &Asking {
            group: manifest.group.slug().as_str().to_owned(),
            side: manifest.side.as_str().to_owned(),
            line: line.as_str().to_owned(),
            commit: commit.clone(),
            task: task.to_string(),
            agent: wanted.agent.clone(),
            machine: machine.clone(),
        },
    )?;

    writeln!(out, "{job}")?;
    writeln!(
        out,
        "  {} · {} on {} at {}",
        task.asked_for(),
        wanted.agent,
        line,
        short(&commit)
    )?;
    writeln!(
        out,
        "  {}",
        match &machine {
            // A machine that is asleep is a valid answer: the job waits
            // for it, because choosing it was the point.
            Some(machine) => format!("waiting for {machine}"),
            None => "waiting for whichever machine is free".to_owned(),
        }
    )?;

    Ok(job)
}

/// Says what is in the queue for this person's machines.
pub fn list(home: &Home, out: &mut impl Write) -> Result<()> {
    let credentials = session::required(home)?;
    let server = Server::at(&credentials.server)?;
    let jobs = server.jobs(&credentials.token)?;

    if jobs.is_empty() {
        writeln!(out, "Nothing has been asked of your machines.")?;
        return Ok(());
    }

    for job in jobs {
        writeln!(out, "{}  {:<10} {}", job.job, job.state, job.said)?;
        writeln!(
            out,
            "  {} · {} on {}/{}",
            job.task, job.agent, job.group, job.line
        )?;
    }

    Ok(())
}

/// The line a command is about.
///
/// Run from inside one, it is that one. Run from anywhere else in the
/// clone, it has to be named: guessing which of a group's lines somebody
/// meant is the kind of help that starts work on the wrong thing.
fn line_of(clone: &Path, at: &Path, named: Option<&str>) -> Result<Name> {
    if let Some(named) = named {
        return Name::parse(named.trim()).context("that is not the name of a line");
    }

    if let Some(found) = here(clone, at) {
        return Ok(found);
    }

    bail!(
        "say which line this is about, with --line: {}",
        available(clone)
    )
}

/// The line the current directory is inside, if it is inside one.
fn here(clone: &Path, at: &Path) -> Option<Name> {
    let inside = at.canonicalize().ok()?;
    let clone = clone.canonicalize().ok()?;
    let mut parts = inside.strip_prefix(&clone).ok()?.components();

    let lines: PathBuf = parts.next()?.as_os_str().into();
    if lines != Path::new(repository::LINES) {
        return None;
    }

    Name::parse(parts.next()?.as_os_str().to_str()?).ok()
}

/// The lines there are, for a message that helps rather than scolds.
fn available(clone: &Path) -> String {
    let names: Vec<String> = build::lines_in(clone)
        .unwrap_or_default()
        .iter()
        .map(|name| name.as_str().to_owned())
        .collect();

    match names.is_empty() {
        true => "this repository has no lines yet".to_owned(),
        false => names.join(", "),
    }
}

/// Refuses to launch work against a commit nobody else can get to.
///
/// A machine that takes the job fetches it from the instance, so a
/// commit that has not been pushed is work nothing can reach — and
/// finding that out as a failed job hours later is the worst way to
/// learn it.
fn ensure_reachable(clone: &Path, commit: &str) -> Result<()> {
    if git::is_dirty(clone).unwrap_or(false) {
        bail!(
            "there is uncommitted work here, so {} is not what you are looking at",
            short(commit)
        );
    }

    if git::distance(clone)
        .map(|far| far.ahead > 0)
        .unwrap_or(false)
    {
        bail!("this commit is not on the instance yet: push it first");
    }

    Ok(())
}

/// This machine's identifier on the instance.
fn this_machine(server: &Server, credentials: &Credentials) -> Result<String> {
    let node = server
        .node(&credentials.token)?
        .context("this machine is not offered for work: run `stemma node init`")?;

    Ok(node.machine)
}

/// A commit, short enough to read.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(8)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "stemma-jobs-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        root
    }

    #[test]
    fn a_command_run_inside_a_line_is_about_that_line() {
        let clone = somewhere("inside");
        let inside = clone.join("lines/quotient-groups");
        std::fs::create_dir_all(inside.join("figures")).unwrap();

        assert_eq!(
            here(&clone, &inside).map(|name| name.as_str().to_owned()),
            Some("quotient-groups".to_owned())
        );
        assert_eq!(
            here(&clone, &inside.join("figures")).map(|name| name.as_str().to_owned()),
            Some("quotient-groups".to_owned()),
            "somewhere under a line is still that line"
        );
        assert_eq!(here(&clone, &clone), None);

        std::fs::remove_dir_all(&clone).unwrap();
    }

    #[test]
    fn a_command_run_anywhere_else_asks_which_line_and_says_what_there_is() {
        let clone = somewhere("outside");
        std::fs::create_dir_all(clone.join("lines/quotient-groups")).unwrap();
        std::fs::write(clone.join("lines/quotient-groups/main.typ"), "").unwrap();

        let error = line_of(&clone, &clone, None).unwrap_err();

        assert!(error.to_string().contains("--line"));
        assert!(error.to_string().contains("quotient-groups"));

        std::fs::remove_dir_all(&clone).unwrap();
    }

    #[test]
    fn a_line_named_by_hand_is_taken_as_named() {
        let clone = somewhere("named");

        assert_eq!(
            line_of(&clone, &clone, Some(" quotient-groups "))
                .unwrap()
                .as_str(),
            "quotient-groups"
        );
        assert!(line_of(&clone, &clone, Some("Not A Line")).is_err());
    }
}

//! Asking a machine for something, from the command line.
//!
//! The launcher in the app is anchored to the mathematics, which is
//! where an action belongs when there is a screen. Here there is no
//! screen, so the anchor is where the command is run from: inside a
//! line, against the revision the instance has of it.
//!
//! The choice of machine and agent stays deliberate either way. Asking
//! for nobody in particular is something a person says in as many
//! words.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use stemma_core::id::JobId;
use stemma_core::job::Task;
use stemma_core::line::Name;
use stemma_core::repository::Side;

use crate::account::credentials::Credentials;
use crate::account::session;
use crate::api::{Asking, Server};
use crate::home::Home;
use crate::job::work;
use crate::mirror::{Instance, Mirror, Which};
use crate::remedy::Remedy;

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
    /// A person's request for a custom task.
    pub request: Option<String>,
    /// Explicit capabilities for a custom task.
    pub grants: Option<stemma_core::proposal::Grants>,
}

/// Puts a job in the queue, against the revision of the line the
/// instance has. What is here and not sent yet is sent first, so the
/// job works on what the person sees.
pub fn run(home: &Home, at: &Path, wanted: &Wanted, out: &mut impl Write) -> Result<String> {
    let (credentials, server) = session::connect(home)?;
    let (group, side, line) = crate::repository::line_at(at, wanted.line.as_deref())?;
    let task: Task = wanted.task.parse().context("that is not a task")?;
    let revision = sent_revision(home, &group, side, &line)?;

    let machine = match (&wanted.machine, wanted.here) {
        (Some(_), true) => bail!("choose one machine, not two"),
        (Some(named), false) => Some(named.clone()),
        (None, true) => Some(this_machine(&server, &credentials)?),
        (None, false) => None,
    };

    let job = server.launch_job(
        &credentials.token,
        &Asking {
            group,
            side: side.as_str().to_owned(),
            line: line.as_str().to_owned(),
            revision,
            against: None,
            task: task.to_string(),
            agent: wanted.agent.clone(),
            machine: machine.clone(),
            request: wanted.request.clone(),
            grants: wanted.grants.clone(),
        },
    )?;

    writeln!(out, "{job}")?;
    writeln!(
        out,
        "  {} · {} on {} at revision {revision}",
        task.asked_for(),
        wanted.agent,
        line,
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
    let (credentials, server) = session::connect(home)?;
    let jobs = server.jobs(&credentials.token)?;

    if jobs.is_empty() {
        writeln!(out, "No jobs.")?;
        return Ok(());
    }

    for job in jobs {
        writeln!(out, "{}  {:<10} {}", job.job, job.state, job.said)?;
        writeln!(
            out,
            "  {} · {} on {}/{}",
            job.task, job.agent, job.group, job.line
        )?;

        if work::output_exists(home, &job.job) {
            writeln!(
                out,
                "  raw output on this machine · `stemma job output {}`",
                job.job
            )?;
        }
        if let Some(progress) = &job.progress {
            writeln!(out, "  {progress}")?;
        }
        if let Some(workspace) = &job.workspace {
            writeln!(out, "  workspace · {workspace}")?;
        }
    }

    Ok(())
}

/// Cancels a queued job or asks the machine running it to stop.
pub fn cancel(home: &Home, job: &str, out: &mut impl Write) -> Result<()> {
    let job: JobId = job.parse().context("that is not a job identifier")?;
    let (credentials, server) = session::connect(home)?;
    server.cancel_job(&credentials.token, &job.to_string())?;
    writeln!(out, "{job}  cancellation requested")?;
    Ok(())
}

/// Prints the raw output one of this machine's agents left behind.
///
/// The identifier is parsed before it becomes a path. Besides producing a
/// useful error for a mistyped id, that makes this local inspection command
/// incapable of naming an arbitrary file.
pub fn output(home: &Home, job: &str, out: &mut impl Write) -> Result<()> {
    let job: JobId = job.parse().context("that is not a job identifier")?;
    let path = work::output_of(home, &job.to_string());
    let raw = std::fs::read(&path)
        .with_context(|| format!("this machine has no raw output for {job}"))?;

    out.write_all(&raw)?;

    Ok(())
}

/// The revision a line is at on the instance, once what is here has
/// been sent: a job works on what the instance has, and work against
/// something only this machine has is work nothing else can reach.
fn sent_revision(home: &Home, group: &str, side: Side, line: &Name) -> Result<u64> {
    let mirror = Mirror::at(home)?;
    let record = mirror
        .find(group, side, line.as_str())?
        .ok_or(Remedy::Sync)
        .context("this line is not on this machine yet")?;

    if record.there.is_some() {
        return Err(Remedy::Decide).context("this line needs a decision first");
    }
    if !record.pending() {
        return Ok(record.base);
    }

    let report = mirror.send(&Instance::connect(home)?, &Which::Line(record.id.clone()));
    match report.revisions.get(&record.id) {
        Some(revision) => Ok(*revision),
        None => bail!(
            "what is here has not been sent yet, so no other machine could work on it: {}",
            report
                .stopped
                .or_else(|| report.waiting.into_iter().next().map(|waiting| waiting.why))
                .unwrap_or_else(|| "it is waiting to be sent".to_owned())
        ),
    }
}

/// This machine's identifier on the instance.
pub(crate) fn this_machine(server: &Server, credentials: &Credentials) -> Result<String> {
    let node = server
        .node(&credentials.token)?
        .ok_or(Remedy::SetUpMachine)
        .context("this machine is not registered to run jobs")?;

    Ok(node.machine)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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

    fn home(root: &Path) -> Home {
        Home::locate(&crate::home::Environment::from_pairs([(
            crate::home::HOME_VARIABLE,
            root.to_string_lossy().as_ref(),
        )]))
    }

    #[test]
    fn raw_output_can_only_be_read_by_its_job_identifier() {
        let root = somewhere("raw-output");
        let home = home(&root);
        let job = "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T";
        let path = work::output_of(&home, job);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"the agent's private account detail\n").unwrap();

        let mut shown = Vec::new();
        output(&home, job, &mut shown).unwrap();
        assert_eq!(shown, b"the agent's private account detail\n");

        let error = output(&home, "../../credentials", &mut Vec::new()).unwrap_err();
        assert!(error.to_string().contains("not a job identifier"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn absent_raw_output_says_that_it_is_absent_on_this_machine() {
        let root = somewhere("no-raw-output");
        let error = output(
            &home(&root),
            "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T",
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("this machine has no raw output"));
    }
}

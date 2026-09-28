//! The `stemma` command: the client researchers install on their own
//! machine. Everything it does lives in the library; this only dispatches.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use stemma::cli::{
    ClaimCommand, Cli, Command, ConflictCommand, DaemonCommand, JobCommand, LineCommand,
    NodeCommand, ProposalCommand, VaultCommand,
};
use stemma::credentials::Credentials;
use stemma::home::{Environment, Home};
use stemma::{
    build, claims, conflicts, daemon, export, git_credential, jobs, lines, machines, mirror, node,
    proposals, session, vault,
};
use stemma_core::proposal::Verdict;

fn main() -> ExitCode {
    if std::env::args_os()
        .next()
        .and_then(|path| PathBuf::from(path).file_stem().map(|name| name.to_owned()))
        .is_some_and(|name| name == "stemma-job")
    {
        return match stemma::job_api::run_from_environment() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("stemma-job: {error:#}");
                ExitCode::FAILURE
            }
        };
    }

    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        // A reader closing the pipe (as `head` does) is not a failure.
        Err(error) if is_broken_pipe(&error) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("stemma: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

/// The directory the command was run in, which names the line or group
/// when no flag does.
fn here() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| ".".into())
}

/// Carries out one command. Returns whether the answer was a success, for
/// the commands whose output is a yes-or-no check.
fn run(cli: Cli) -> Result<bool> {
    let home = &Home::locate(&Environment::from_process());
    let out = &mut std::io::stdout().lock();

    match cli.command {
        Command::Login { server } => {
            session::log_in(home, &server, out)?;
        }
        Command::Whoami => return session::whoami(home, out),
        Command::Logout => session::log_out(home, out)?,
        Command::Sync => {
            mirror::run(home, out)?;
        }
        Command::Status => {
            mirror::status(home, out)?;
        }
        Command::Build => {
            build::run(home, &here(), out)?;
        }
        Command::Where => where_things_are(home, out)?,
        Command::Export {
            group,
            private,
            into,
        } => {
            export::run(home, &group, private, into.as_deref(), out)?;
        }
        Command::Machines { group } => machines::list(home, &here(), group.as_deref(), out)?,

        Command::Line { command } => match command {
            LineCommand::New {
                group,
                title,
                description,
                shared,
            } => lines::start(home, &group, &title, description.as_deref(), shared, out)?,
            LineCommand::Delete { line } => lines::delete(home, &here(), line.as_deref(), out)?,
            LineCommand::Share {
                line,
                history,
                mine_to_write,
            } => lines::share(home, &here(), line.as_deref(), history, mine_to_write, out)?,
        },

        Command::Claim { command } => match command {
            ClaimCommand::Take { line } => claims::take(home, &here(), line.as_deref(), out)?,
            ClaimCommand::Request { line } => claims::request(home, &here(), line.as_deref(), out)?,
            ClaimCommand::Release { line } => claims::release(home, &here(), line.as_deref(), out)?,
            ClaimCommand::Status => claims::status(home, out)?,
        },

        Command::Conflict { command } => match command {
            ConflictCommand::List => conflicts::list(home, &here(), out)?,
            ConflictCommand::Resolve { keep } => conflicts::resolve(home, &here(), keep, out)?,
        },

        Command::Node { command } => match command {
            NodeCommand::Init { groups } => {
                node::init(home, &groups, out)?;
            }
            NodeCommand::Status => return node::status(home, out),
        },

        Command::Daemon { command } => match command {
            DaemonCommand::Run { once } => daemon::run(home, once, out)?,
            DaemonCommand::Install => daemon::install(home, out)?,
        },

        Command::Job { command } => match command {
            JobCommand::Run(asked) => {
                let grants = asked
                    .request
                    .as_ref()
                    .map(|_| stemma_core::proposal::Grants {
                        line: asked
                            .grant_blocks
                            .iter()
                            .flat_map(|block| {
                                [
                                    stemma_core::proposal::LineGrant::Edit { block: *block },
                                    stemma_core::proposal::LineGrant::Remove { block: *block },
                                    stemma_core::proposal::LineGrant::Add {
                                        after: Some(*block),
                                    },
                                ]
                            })
                            .collect(),
                        library: asked.grant_library,
                        add_only: false,
                    });
                let wanted = jobs::Wanted {
                    line: asked.line,
                    task: if asked.request.is_some() {
                        "custom".to_owned()
                    } else {
                        asked.task
                    },
                    agent: asked.agent,
                    machine: asked.machine,
                    here: asked.here,
                    model: asked.model,
                    effort: asked.effort,
                    request: asked.request,
                    grants,
                };
                jobs::run(home, &here(), &wanted, out)?;
            }
            JobCommand::List => jobs::list(home, out)?,
            JobCommand::Output { job } => jobs::output(home, &job, out)?,
            JobCommand::Cancel { job } => jobs::cancel(home, &job, out)?,
        },

        Command::Proposal { command } => match command {
            ProposalCommand::List => proposals::list(home, out)?,
            ProposalCommand::Show { proposal } => proposals::show(home, &proposal, out)?,
            ProposalCommand::Accept { proposal } => {
                proposals::decide(home, &proposal, Verdict::Accept, out)?
            }
            ProposalCommand::Reject { proposal, reason } => proposals::decide_with_note(
                home,
                &proposal,
                Verdict::Reject,
                reason.as_deref(),
                out,
            )?,
            ProposalCommand::AskAgain { proposal, note } => proposals::decide_with_note(
                home,
                &proposal,
                Verdict::AskAgain,
                note.as_deref(),
                out,
            )?,
            ProposalCommand::Apply { proposal } => proposals::apply(home, &proposal, out)?,
        },

        Command::Vault { command } => match command {
            VaultCommand::List { group } => vault::list(home, &group, out)?,
            VaultCommand::Admit {
                group,
                siglum,
                name,
                prose,
                lean,
            } => vault::admit(home, &group, &siglum, &name, &prose, &lean, out)?,
            VaultCommand::Bridge {
                group,
                left,
                right,
                kind,
                lean,
            } => vault::bridge(home, &group, &left, &right, kind, &lean, out)?,
            VaultCommand::Audit {
                group,
                target_kind,
                target,
                action,
                note,
            } => vault::audit(home, &group, target_kind, &target, action, &note, out)?,
        },

        Command::Credential { action } => {
            git_credential::answer(home, &action, std::io::stdin().lock(), out)?
        }
    }

    Ok(true)
}

/// Prints where this machine keeps its configuration, state and work.
fn where_things_are(home: &Home, out: &mut impl Write) -> Result<()> {
    let signed_in = Credentials::read(&home.credentials_file())?;

    writeln!(out, "config     {}", home.config_file().display())?;
    writeln!(out, "state      {}", home.state_dir().display())?;
    writeln!(out, "cache      {}", home.cache_dir().display())?;
    writeln!(out, "workspace  {}", home.workspace().display())?;
    writeln!(
        out,
        "server     {}",
        signed_in
            .as_ref()
            .map_or("not signed in", |credentials| credentials.server.as_str())
    )?;

    Ok(())
}

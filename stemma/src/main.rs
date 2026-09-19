//! The `stemma` command: the client researchers install on their own
//! machine.

use std::io::Write;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use stemma::cli::{
    Cli, Command, DaemonCommand, JobCommand, NodeCommand, ProposalCommand, VaultCommand,
};
use stemma::credentials::Credentials;
use stemma::home::{Environment, Home};
use stemma::session;

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        // Somebody piping this into `head` closes the output, and that is
        // not a failure of the command they ran.
        Err(error) if is_broken_pipe(&error) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("stemma: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Whether what went wrong is that nobody is reading any more.
fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

/// Carries out one command.
fn run(cli: Cli) -> Result<ExitCode> {
    let home = Home::locate(&Environment::from_process());
    let mut out = std::io::stdout().lock();

    match cli.command {
        Command::Login { server } => {
            session::log_in(&home, &server, &mut out)?;

            Ok(ExitCode::SUCCESS)
        }

        Command::Whoami => Ok(match session::whoami(&home, &mut out)? {
            true => ExitCode::SUCCESS,
            false => ExitCode::FAILURE,
        }),

        Command::Logout => {
            session::log_out(&home, &mut out)?;

            Ok(ExitCode::SUCCESS)
        }

        Command::Sync => {
            stemma::sync::run(&home, &mut out)?;

            Ok(ExitCode::SUCCESS)
        }

        Command::Build => {
            let here = std::env::current_dir().unwrap_or_else(|_| ".".into());
            stemma::build::run(&home, &here, &mut out)?;

            Ok(ExitCode::SUCCESS)
        }

        Command::Node { command } => match command {
            NodeCommand::Init { groups } => {
                stemma::node::init(&home, &groups, &mut out)?;

                Ok(ExitCode::SUCCESS)
            }
            NodeCommand::Status => Ok(match stemma::node::status(&home, &mut out)? {
                true => ExitCode::SUCCESS,
                false => ExitCode::FAILURE,
            }),
        },

        Command::Daemon { command } => match command {
            DaemonCommand::Run { once } => {
                stemma::daemon::run(&home, once, &mut out)?;

                Ok(ExitCode::SUCCESS)
            }
            DaemonCommand::Install => {
                stemma::daemon::install(&home, &mut out)?;

                Ok(ExitCode::SUCCESS)
            }
        },

        Command::Job { command } => match command {
            JobCommand::Run(asked) => {
                let here = std::env::current_dir().unwrap_or_else(|_| ".".into());

                stemma::jobs::run(
                    &home,
                    &here,
                    &stemma::jobs::Wanted {
                        line: asked.line,
                        task: asked.task,
                        agent: asked.agent,
                        machine: asked.machine,
                        here: asked.here,
                    },
                    &mut out,
                )?;

                Ok(ExitCode::SUCCESS)
            }
            JobCommand::List => {
                stemma::jobs::list(&home, &mut out)?;

                Ok(ExitCode::SUCCESS)
            }
            JobCommand::Output { job } => {
                stemma::jobs::output(&home, &job, &mut out)?;

                Ok(ExitCode::SUCCESS)
            }
        },

        Command::Proposal { command } => match command {
            ProposalCommand::Apply { proposal } => {
                stemma::proposals::apply(&home, &proposal, &mut out)?;
                Ok(ExitCode::SUCCESS)
            }
        },

        Command::Vault { command } => {
            match command {
                VaultCommand::List { group } => stemma::vault::list(&home, &group, &mut out)?,
                VaultCommand::Admit {
                    group,
                    siglum,
                    name,
                    prose,
                    lean,
                } => stemma::vault::admit(&home, &group, &siglum, &name, &prose, &lean, &mut out)?,
                VaultCommand::Bridge {
                    group,
                    left,
                    right,
                    kind,
                    lean,
                } => stemma::vault::bridge(&home, &group, &left, &right, &kind, &lean, &mut out)?,
                VaultCommand::Audit {
                    group,
                    target_kind,
                    target,
                    action,
                    note,
                } => stemma::vault::audit(
                    &home,
                    &group,
                    &target_kind,
                    &target,
                    &action,
                    &note,
                    &mut out,
                )?,
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Status => {
            stemma::sync::status(&home, &mut out)?;

            Ok(ExitCode::SUCCESS)
        }

        Command::Credential { action } => {
            let input = std::io::stdin().lock();
            stemma::credential::answer(&home, &action, input, &mut out)?;

            Ok(ExitCode::SUCCESS)
        }

        Command::Where => {
            let signed_in = Credentials::read(&home.credentials_file())?;

            writeln!(out, "config     {}", home.config_file().display())?;
            writeln!(out, "state      {}", home.state_dir().display())?;
            writeln!(out, "cache      {}", home.cache_dir().display())?;
            writeln!(out, "workspace  {}", home.workspace().display())?;
            writeln!(
                out,
                "instance   {}",
                match &signed_in {
                    Some(credentials) => credentials.server.as_str(),
                    None => "not signed in",
                }
            )?;

            Ok(ExitCode::SUCCESS)
        }
    }
}

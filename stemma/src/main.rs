//! The `stemma` command: the client researchers install on their own
//! machine.

use std::io::Write;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use stemma::cli::{Cli, Command};
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

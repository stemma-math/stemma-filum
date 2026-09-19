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
        Err(error) => {
            eprintln!("stemma: {error:#}");
            ExitCode::FAILURE
        }
    }
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

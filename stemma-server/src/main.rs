//! The `stemma-server` command: the program that runs an instance.

mod check;
mod cli;
mod config;
mod system;

use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use crate::check::Report;
use crate::cli::{Cli, Command};
use crate::config::{Environment, Overrides, Settings};

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("stemma-server: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Carries out one command.
fn run(cli: Cli) -> Result<ExitCode> {
    let environment = Environment::from_process();
    let config_path = Settings::config_path(cli.config.as_deref(), &environment);

    match cli.command {
        Command::Check {
            settings,
            user,
            strict,
        } => {
            let report = check(&config_path, &environment, &settings.overrides(), &user);

            println!("{report}");

            Ok(match report.allows_startup(strict) {
                true => ExitCode::SUCCESS,
                false => ExitCode::FAILURE,
            })
        }
    }
}

/// Reads the configuration and inspects the instance it describes.
fn check(
    config_path: &Path,
    environment: &Environment,
    overrides: &Overrides,
    service_user: &str,
) -> Report {
    match Settings::load(config_path, environment, overrides) {
        Ok(settings) => check::inspect(&settings, service_user),
        Err(error) => check::from_config_error(&error),
    }
}

//! The `stemma-server` command: the program that runs an instance.

mod check;
mod cli;
mod config;
mod install;
mod service;
mod system;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::check::Report;
use crate::cli::{Cli, Command};
use crate::config::{ConfigFile, Environment, Overrides, Settings};
use crate::install::{Journal, Options, RemovalOptions};

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
        Command::Install {
            data_dir,
            user,
            no_service,
            dry_run,
        } => {
            let options = Options {
                env_path: config::env_path_for(&config_path),
                data_dir: state_directory(&config_path, &environment, data_dir)?,
                config_path,
                user,
                service: !no_service,
                dry_run,
            };

            let journal = install::install(&options)?;
            report_journal(&journal);
            announce_next_steps(&options);

            Ok(ExitCode::SUCCESS)
        }

        Command::Uninstall {
            data_dir,
            user,
            purge,
            yes,
            dry_run,
        } => {
            let options = RemovalOptions {
                env_path: config::env_path_for(&config_path),
                data_dir: state_directory(&config_path, &environment, data_dir)?,
                config_path,
                user,
                purge,
                dry_run,
            };

            if options.purge && !options.dry_run && !confirm_purge(&options, yes)? {
                println!("Nothing was removed.");
                return Ok(ExitCode::SUCCESS);
            }

            let journal = install::uninstall(&options)?;
            report_journal(&journal);

            if !options.purge {
                announce_what_remains(&options);
            }

            Ok(ExitCode::SUCCESS)
        }

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

/// Where state goes, according to the flag, the environment and whatever
/// configuration is already on the machine.
fn state_directory(
    config_path: &Path,
    environment: &Environment,
    flag: Option<PathBuf>,
) -> Result<PathBuf> {
    let file = ConfigFile::read_if_present(config_path)?.unwrap_or_default();
    let overrides = Overrides {
        data_dir: flag,
        ..Overrides::default()
    };

    Ok(Settings::resolve_data_dir(&file, environment, &overrides)?)
}

/// Prints what a run of the installer did.
fn report_journal(journal: &Journal) {
    if !journal.steps.is_empty() {
        println!("{journal}");
    }
}

/// Says what an operator has to do before the instance will answer.
fn announce_next_steps(options: &Options) {
    println!();
    println!("The instance is installed and is not running.");
    println!(
        "  1. Edit {} — `public_url` is required.",
        options.config_path.display()
    );
    println!("  2. Run `stemma-server check`.");

    if options.service && service::is_available() {
        println!("  3. Run `systemctl start {}`.", service::UNIT_NAME);
    } else {
        println!("  3. Run `stemma-server serve` under whatever supervises it.");
    }
}

/// Says where everything the uninstall left behind is.
fn announce_what_remains(options: &RemovalOptions) {
    println!();
    println!("The software is gone. What the instance wrote is not:");
    println!("  {}", options.config_path.display());
    println!(
        "  {} ({})",
        options.data_dir.display(),
        install::human_size(install::size_of(&options.data_dir))
    );
    println!("  the `{}` account", options.user);
    println!();
    println!("Run the same command with --purge to erase them.");
}

/// Asks for a purge to be confirmed, by naming the machine it will empty.
fn confirm_purge(options: &RemovalOptions, already_answered: bool) -> Result<bool> {
    let hostname = system::hostname();
    let size = install::human_size(install::size_of(&options.data_dir));

    println!("This erases, on {hostname}:");
    println!("  {}", options.config_path.display());
    println!("  {}", options.env_path.display());
    println!("  {} ({size} of mathematics)", options.data_dir.display());
    println!("  the `{}` account", options.user);
    println!();
    println!(
        "Back it up first:  tar czf stemma-backup.tar.gz {}",
        options.data_dir.display()
    );
    println!();

    if already_answered {
        return Ok(true);
    }

    if !std::io::stdin().is_terminal() {
        bail!("a purge asks for confirmation, so pass --yes to answer it in advance");
    }

    print!("Type the name of this machine ({hostname}) to confirm: ");
    std::io::stdout().flush().ok();

    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("cannot read the confirmation")?;

    Ok(answer.trim() == hostname)
}

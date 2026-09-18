//! The command line surface.
//!
//! Installing, configuring and bootstrapping an instance are separate
//! commands rather than one wizard. Each can be run alone, re-run safely, and
//! driven by a provisioning tool, and every question a wizard would ask has a
//! flag and an environment variable.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::config::Overrides;

/// The program's arguments.
#[derive(Debug, Parser)]
#[command(
    name = "stemma-server",
    version,
    about = "Runs a Stemma instance.",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// The configuration file to read.
    #[arg(long, short, global = true, value_name = "PATH", env = CONFIG_ENV)]
    pub config: Option<PathBuf>,

    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The commands an operator runs.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Puts an instance on this machine, without starting it.
    Install {
        /// The directory holding all durable state.
        #[arg(long, value_name = "PATH")]
        data_dir: Option<PathBuf>,

        /// The system user the service runs as.
        #[arg(long, value_name = "NAME", default_value = crate::config::DEFAULT_SYSTEM_USER)]
        user: String,

        /// Do everything except write and enable the systemd unit.
        #[arg(long)]
        no_service: bool,

        /// Report what would happen and change nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Takes the software off this machine, leaving its data behind.
    Uninstall {
        /// The directory holding all durable state.
        #[arg(long, value_name = "PATH")]
        data_dir: Option<PathBuf>,

        /// The system user the service ran as.
        #[arg(long, value_name = "NAME", default_value = crate::config::DEFAULT_SYSTEM_USER)]
        user: String,

        /// Also erase the configuration, the state and the system user.
        #[arg(long)]
        purge: bool,

        /// Answer the confirmation a purge asks for.
        #[arg(long)]
        yes: bool,

        /// Report what would happen and change nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Reports whether the instance could start right now, and why not.
    Check {
        /// Settings given on the command line.
        #[command(flatten)]
        settings: SettingsArgs,

        /// The system user the service runs as.
        #[arg(long, value_name = "NAME", default_value = crate::config::DEFAULT_SYSTEM_USER)]
        user: String,

        /// Treat warnings as failures, for a deployment pipeline.
        #[arg(long)]
        strict: bool,
    },
}

/// The settings that can be given on the command line.
///
/// These beat the environment, which beats the configuration file.
#[derive(Debug, Args)]
pub struct SettingsArgs {
    /// The absolute address the instance answers on.
    #[arg(long, value_name = "URL")]
    pub public_url: Option<String>,

    /// The address to bind, as `host:port` or `unix:/path/to/socket`.
    #[arg(long, value_name = "ADDRESS")]
    pub listen: Option<String>,

    /// The directory holding all durable state.
    #[arg(long, value_name = "PATH")]
    pub data_dir: Option<PathBuf>,
}

impl SettingsArgs {
    /// The settings as an override layer.
    pub fn overrides(&self) -> Overrides {
        Overrides {
            public_url: self.public_url.clone(),
            listen: self.listen.clone(),
            data_dir: self.data_dir.clone(),
        }
    }
}

/// The variable that names the configuration file.
///
/// It is written out rather than built from [`ENV_PREFIX`], because the
/// attribute that declares it takes a literal. A test keeps the two in step.
pub const CONFIG_ENV: &str = "STEMMA_SERVER_CONFIG";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ENV_PREFIX;

    fn parse(arguments: &[&str]) -> Cli {
        Cli::try_parse_from(arguments).expect("the arguments should parse")
    }

    #[test]
    fn the_config_variable_uses_the_shared_prefix() {
        assert_eq!(CONFIG_ENV, format!("{ENV_PREFIX}CONFIG"));
    }

    #[test]
    fn check_runs_with_no_arguments_at_all() {
        let cli = parse(&["stemma-server", "check"]);
        assert!(cli.config.is_none());

        let Command::Check { strict, user, .. } = cli.command else {
            panic!("expected the check command");
        };
        assert!(!strict);
        assert_eq!(user, crate::config::DEFAULT_SYSTEM_USER);
    }

    #[test]
    fn every_setting_has_a_flag() {
        let cli = parse(&[
            "stemma-server",
            "--config",
            "/srv/stemma.toml",
            "check",
            "--public-url",
            "https://stemma.example.org",
            "--listen",
            "unix:/run/stemma/server.sock",
            "--data-dir",
            "/srv/stemma",
            "--strict",
        ]);

        assert_eq!(
            cli.config.as_deref(),
            Some(std::path::Path::new("/srv/stemma.toml"))
        );

        let Command::Check {
            settings, strict, ..
        } = cli.command
        else {
            panic!("expected the check command");
        };
        assert!(strict);

        let overrides = settings.overrides();
        assert_eq!(
            overrides.public_url.as_deref(),
            Some("https://stemma.example.org")
        );
        assert_eq!(
            overrides.listen.as_deref(),
            Some("unix:/run/stemma/server.sock")
        );
        assert_eq!(
            overrides.data_dir.as_deref(),
            Some(std::path::Path::new("/srv/stemma"))
        );
    }

    #[test]
    fn installing_takes_the_defaults_of_the_layout() {
        let Command::Install {
            data_dir,
            user,
            no_service,
            dry_run,
        } = parse(&["stemma-server", "install"]).command
        else {
            panic!("expected the install command");
        };

        assert!(data_dir.is_none());
        assert_eq!(user, crate::config::DEFAULT_SYSTEM_USER);
        assert!(!no_service);
        assert!(!dry_run);
    }

    #[test]
    fn a_purge_can_be_answered_in_advance() {
        let Command::Uninstall { purge, yes, .. } =
            parse(&["stemma-server", "uninstall", "--purge", "--yes"]).command
        else {
            panic!("expected the uninstall command");
        };

        assert!(purge && yes);
    }

    #[test]
    fn the_definition_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}

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

        /// Ask the configuration questions once the files are in place.
        #[arg(long)]
        interactive: bool,

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

    /// Asks the questions the configuration file cannot answer by itself.
    Setup {
        /// The directory holding all durable state.
        #[arg(long, value_name = "PATH")]
        data_dir: Option<PathBuf>,
    },

    /// Runs the server in the foreground.
    Serve {
        /// Settings given on the command line.
        #[command(flatten)]
        settings: SettingsArgs,

        /// The system user the service runs as.
        #[arg(long, value_name = "NAME", default_value = crate::config::DEFAULT_SYSTEM_USER)]
        user: String,
    },

    /// Instance maintainers.
    Admin {
        /// What to do about them.
        #[command(subcommand)]
        action: AdminAction,

        /// Settings given on the command line.
        #[command(flatten)]
        settings: SettingsArgs,
    },

    /// Accounts, from the machine.
    User {
        /// What to do about one.
        #[command(subcommand)]
        action: UserAction,

        /// Settings given on the command line.
        #[command(flatten)]
        settings: SettingsArgs,
    },

    /// Reports what this instance is and what is in it.
    Status {
        /// Settings given on the command line.
        #[command(flatten)]
        settings: SettingsArgs,
    },

    /// Copies everything the instance could not be rebuilt without.
    Backup {
        /// Settings given on the command line.
        #[command(flatten)]
        settings: SettingsArgs,

        /// The directory to write the archive into.
        #[arg(long, value_name = "PATH", default_value = ".")]
        to: PathBuf,

        /// Leave only this many archives in that directory.
        #[arg(long, value_name = "COUNT")]
        keep: Option<usize>,

        /// Carry the build outputs too, which the instance can make again.
        #[arg(long)]
        with_artefacts: bool,
    },

    /// Brings the database up to the schema this build needs.
    Migrate {
        /// Settings given on the command line.
        #[command(flatten)]
        settings: SettingsArgs,
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

/// What can be done about instance maintainers.
///
/// A grant takes an account that already exists, so the way to the first
/// maintainer is to register through the web like anybody else and then run
/// `grant` here. That keeps passwords out of the command line.
#[derive(Debug, Subcommand)]
pub enum AdminAction {
    /// Gives an account the run of the instance.
    Grant {
        /// A name, an address or a full identifier.
        account: String,
    },

    /// Takes it away, keeping the record that it was held.
    Revoke {
        /// A name, an address or a full identifier.
        account: String,
    },

    /// Says who operates this instance.
    List,
}

/// What can be done about an account from the machine.
#[derive(Debug, Subcommand)]
pub enum UserAction {
    /// Makes an account and prints a link for setting its password.
    Create {
        /// The name it is known by here.
        name: String,

        /// Where to reach whoever holds it.
        #[arg(long, value_name = "ADDRESS")]
        email: String,

        /// What to call them, if not the name itself.
        #[arg(long, value_name = "NAME")]
        called: Option<String>,
    },

    /// Prints a single-use link for setting a new password.
    ResetPassword {
        /// A name, an address or a full identifier.
        account: String,
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
            interactive,
            no_service,
            dry_run,
        } = parse(&["stemma-server", "install"]).command
        else {
            panic!("expected the install command");
        };

        assert!(data_dir.is_none());
        assert_eq!(user, crate::config::DEFAULT_SYSTEM_USER);
        assert!(!interactive);
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
    fn a_backup_goes_to_the_current_directory_unless_told_otherwise() {
        let Command::Backup {
            to,
            keep,
            with_artefacts,
            ..
        } = parse(&["stemma-server", "backup"]).command
        else {
            panic!("expected the backup command");
        };

        assert_eq!(to, PathBuf::from("."));
        assert!(keep.is_none());
        assert!(!with_artefacts);
    }

    #[test]
    fn a_backup_can_be_told_where_to_go_and_what_to_leave() {
        let Command::Backup {
            to,
            keep,
            with_artefacts,
            ..
        } = parse(&[
            "stemma-server",
            "backup",
            "--to",
            "/srv/backups",
            "--keep",
            "7",
            "--with-artefacts",
        ])
        .command
        else {
            panic!("expected the backup command");
        };

        assert_eq!(to, PathBuf::from("/srv/backups"));
        assert_eq!(keep, Some(7));
        assert!(with_artefacts);
    }

    #[test]
    fn an_account_is_made_with_a_name_and_an_address() {
        let Command::User {
            action:
                UserAction::Create {
                    name,
                    email,
                    called,
                },
            ..
        } = parse(&[
            "stemma-server",
            "user",
            "create",
            "ana",
            "--email",
            "ana@uv.es",
        ])
        .command
        else {
            panic!("expected the create command");
        };

        assert_eq!(name, "ana");
        assert_eq!(email, "ana@uv.es");
        assert!(called.is_none());
    }

    #[test]
    fn the_definition_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}

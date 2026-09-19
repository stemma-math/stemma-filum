//! The command line surface.
//!
//! The command line stays complete on its own: a machine with no desktop —
//! a node over SSH, a container — has to be able to do everything, or the
//! product has been split in two.

use clap::{Parser, Subcommand};

/// The program's arguments.
#[derive(Debug, Parser)]
#[command(
    name = "stemma",
    version,
    about = "Work with a Stemma instance from this machine.",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// What a researcher runs.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Signs this machine in to an instance.
    Login {
        /// The instance's address, such as `stemma.example.org`.
        server: String,
    },

    /// Says who this machine acts for.
    Whoami,

    /// Gives the credential back and forgets it.
    Logout,

    /// Brings every group this machine can work in into step.
    Sync,

    /// Says what this machine has, without asking the instance.
    Status,

    /// Says where this machine keeps its things.
    Where,

    /// Answers git when it asks for a credential.
    #[command(hide = true)]
    Credential {
        /// What git wants: `get`, `store` or `erase`.
        action: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_in_needs_an_instance_to_sign_in_to() {
        assert!(Cli::try_parse_from(["stemma", "login"]).is_err());

        let Command::Login { server } = Cli::try_parse_from(["stemma", "login", "uv.es"])
            .unwrap()
            .command
        else {
            panic!("expected the login command");
        };

        assert_eq!(server, "uv.es");
    }

    #[test]
    fn the_definition_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}

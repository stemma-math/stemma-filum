//! The command line surface.
//!
//! The command line stays complete on its own: a machine with no desktop —
//! a node over SSH, a container — has to be able to do everything, or the
//! product has been split in two.

use clap::{Args, Parser, Subcommand};

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

    /// Reads what the sources here declare.
    Build,

    /// Offers this machine, or says what the instance makes of it.
    Node {
        /// What to do with it.
        #[command(subcommand)]
        command: NodeCommand,
    },

    /// Runs the loop that takes work, or installs it.
    Daemon {
        /// What to do with it.
        #[command(subcommand)]
        command: DaemonCommand,
    },

    /// Asks a machine for something.
    Job {
        /// What to do.
        #[command(subcommand)]
        command: JobCommand,
    },

    /// Applies source already accepted in the review screen.
    Proposal {
        /// What to do with it.
        #[command(subcommand)]
        command: ProposalCommand,
    },

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

/// What can be done with a reviewed proposal.
#[derive(Debug, Subcommand)]
pub enum ProposalCommand {
    /// Writes and commits an accepted proposal on this machine.
    Apply {
        /// The accepted proposal to write.
        proposal: String,
    },
}

/// What can be done with this machine as a node.
#[derive(Debug, Subcommand)]
pub enum NodeCommand {
    /// Offers this machine to the instance as one that runs work.
    Init {
        /// A group to work for. Given none, it works for every group
        /// this account is in.
        #[arg(long = "group", value_name = "SLUG")]
        groups: Vec<String>,
    },

    /// Says what the instance makes of this machine.
    Status,
}

/// What can be done with the daemon.
#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Takes work until it is stopped.
    Run {
        /// Take at most one job and stop.
        #[arg(long)]
        once: bool,
    },

    /// Puts it under whatever supervises this machine.
    Install,
}

/// What can be done with jobs.
#[derive(Debug, Subcommand)]
pub enum JobCommand {
    /// Asks for something to be done.
    Run(JobRun),

    /// Says what has been asked of this account's machines.
    List,

    /// Prints the raw output kept on this machine for one job.
    Output {
        /// The job whose local output to print.
        job: String,
    },
}

/// What a job asks for, and of whom.
#[derive(Debug, Args)]
pub struct JobRun {
    /// What to ask for: `fill`, `refute@thm:main`, `formalize@thm:main`,
    /// `verify@thm:main` or `explore`.
    #[arg(long)]
    pub task: String,

    /// Which agent is to do it.
    #[arg(long)]
    pub agent: String,

    /// Which line of work, when it is not the one this is run inside.
    #[arg(long)]
    pub line: Option<String>,

    /// Which machine is to do it. A sleeping one waits, rather than
    /// being passed over.
    #[arg(long, conflicts_with = "here")]
    pub machine: Option<String>,

    /// This machine.
    #[arg(long)]
    pub here: bool,
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
    fn a_job_says_what_it_asks_for_and_of_which_agent() {
        assert!(Cli::try_parse_from(["stemma", "job", "run", "--task", "fill"]).is_err());

        let Command::Job {
            command: JobCommand::Run(asked),
        } = Cli::try_parse_from([
            "stemma",
            "job",
            "run",
            "--task",
            "refute@thm:main",
            "--agent",
            "claude",
            "--here",
        ])
        .unwrap()
        .command
        else {
            panic!("expected a job to run");
        };

        assert_eq!(asked.task, "refute@thm:main");
        assert_eq!(asked.agent, "claude");
        assert!(asked.here);
        assert_eq!(asked.machine, None);
    }

    #[test]
    fn one_machine_is_chosen_and_not_two() {
        assert!(
            Cli::try_parse_from([
                "stemma",
                "job",
                "run",
                "--task",
                "fill",
                "--agent",
                "claude",
                "--here",
                "--machine",
                "mch:1",
            ])
            .is_err()
        );
    }

    #[test]
    fn raw_output_names_the_job_that_produced_it() {
        let Command::Job {
            command: JobCommand::Output { job },
        } = Cli::try_parse_from(["stemma", "job", "output", "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T"])
            .unwrap()
            .command
        else {
            panic!("expected a job output command");
        };

        assert_eq!(job, "job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T");
    }

    #[test]
    fn a_node_works_for_every_group_unless_it_is_told_otherwise() {
        let Command::Node {
            command: NodeCommand::Init { groups },
        } = Cli::try_parse_from(["stemma", "node", "init"])
            .unwrap()
            .command
        else {
            panic!("expected a node to register");
        };

        assert!(groups.is_empty());

        let Command::Node {
            command: NodeCommand::Init { groups },
        } = Cli::try_parse_from(["stemma", "node", "init", "--group", "algebra"])
            .unwrap()
            .command
        else {
            panic!("expected a node to register");
        };

        assert_eq!(groups, ["algebra"]);
    }

    #[test]
    fn the_definition_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}

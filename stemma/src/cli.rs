//! The command line surface.
//!
//! The command line stays complete on its own: a machine with no desktop —
//! a node over SSH, a container — has to be able to do everything, or the
//! product has been split in two.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use stemma_core::reconcile::Keep;
use stemma_core::vault::{AuditTarget, AuditVerb, BridgeKind};

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

    /// Starts a line, or moves one to the group.
    Line {
        /// What to do with it.
        #[command(subcommand)]
        command: LineCommand,
    },

    /// Says what machines a group has, and what each of them can run.
    Machines {
        /// Which group, when it is not the one this is run inside.
        #[arg(long)]
        group: Option<String>,
    },

    /// Says who is writing a line, and takes it or lets it go.
    Claim {
        /// What to do with it.
        #[command(subcommand)]
        command: ClaimCommand,
    },

    /// Works with two versions of a line that have parted.
    Conflict {
        /// What to do about them.
        #[command(subcommand)]
        command: ConflictCommand,
    },

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

    /// Works with a group's immutable definitions and correspondence audits.
    Vault {
        /// What to do in it.
        #[command(subcommand)]
        command: VaultCommand,
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

/// What can be done with a proposal.
#[derive(Debug, Subcommand)]
pub enum ProposalCommand {
    /// Says what is waiting for you to read it.
    List,

    /// Shows what one proposal would change, and what was checked.
    Show {
        /// The proposal to read.
        proposal: String,
    },

    /// Signs the words as yours.
    Accept {
        /// The proposal to accept.
        proposal: String,
    },

    /// Declines them.
    Reject {
        /// The proposal to reject.
        proposal: String,
    },

    /// Declines them and puts the same question back in the queue.
    AskAgain {
        /// The proposal to decline.
        proposal: String,
    },

    /// Writes and commits an accepted proposal on this machine.
    Apply {
        /// The accepted proposal to write.
        proposal: String,
    },
}

/// What can be done with a group's vault without a graphical interface.
#[derive(Debug, Subcommand)]
pub enum VaultCommand {
    /// Lists definitions and proved bridges.
    List {
        /// Group slug.
        group: String,
    },
    /// Admits a definition whose complete Lean module compiles locally.
    Admit {
        /// Group slug.
        group: String,
        /// Definition siglum in the published source.
        #[arg(long)]
        siglum: String,
        /// Distinguishing mathematical name.
        #[arg(long)]
        name: String,
        /// File holding the exact prose.
        #[arg(long)]
        prose: PathBuf,
        /// Complete Lean module to compile and admit.
        #[arg(long)]
        lean: PathBuf,
    },
    /// Admits an unconditional bridge whose complete Lean module compiles locally.
    Bridge {
        /// Group slug.
        group: String,
        /// Vault definition at one end.
        #[arg(long)]
        left: String,
        /// Vault definition at the other end.
        #[arg(long)]
        right: String,
        /// The equivalence proved: `predicate` or `structure`.
        #[arg(long)]
        kind: BridgeKind,
        /// Complete Lean module containing the unconditional theorem.
        #[arg(long)]
        lean: PathBuf,
    },
    /// Attests or withdraws correspondence.
    Audit {
        /// Group slug.
        group: String,
        /// What is audited: `entry` or `bridge`.
        #[arg(long)]
        target_kind: AuditTarget,
        /// Stable vault object identifier.
        #[arg(long)]
        target: String,
        /// `attest` or `withdraw`.
        #[arg(long)]
        action: AuditVerb,
        /// What was checked or why the guarantee is withdrawn.
        #[arg(long)]
        note: String,
    },
}

/// What can be done with a line itself.
#[derive(Debug, Subcommand)]
pub enum LineCommand {
    /// Starts one.
    New {
        /// Which group it belongs to.
        #[arg(long)]
        group: String,

        /// What it is called.
        #[arg(long)]
        title: String,

        /// What it is about, in a sentence or two.
        #[arg(long)]
        description: Option<String>,

        /// Let the whole group see it. Left out, it is yours alone.
        #[arg(long)]
        shared: bool,
    },

    /// Takes one of yours out of the repository.
    Delete {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,
    },

    /// Moves one of yours to the group.
    Share {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,

        /// Send everything you wrote getting here, not only today's state.
        #[arg(long)]
        history: bool,

        /// Keep the writing to yourself; everybody else proposes.
        #[arg(long)]
        mine_to_write: bool,
    },
}

/// What can be done with the claim on a line.
#[derive(Debug, Subcommand)]
pub enum ClaimCommand {
    /// Takes the line for this machine.
    Take {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,
    },

    /// Asks whichever machine has the line to hand it back.
    Ask {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,
    },

    /// Lets the line go.
    Release {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,
    },

    /// Says which lines are being written, and on what.
    Status,
}

/// What can be done about two versions that have parted.
#[derive(Debug, Subcommand)]
pub enum ConflictCommand {
    /// Says what the two versions disagree about.
    List,

    /// Brings them together, keeping one side wherever they disagree.
    Resolve {
        /// Which version stands: `here` or `there`.
        #[arg(long)]
        keep: Keep,
    },

    /// Asks an agent to reconcile them, which comes back as a proposal.
    Reconcile {
        /// Which agent is to do it.
        #[arg(long)]
        agent: String,
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
    fn a_new_line_says_what_it_is_called_and_who_sees_it() {
        assert!(
            Cli::try_parse_from(["stemma", "line", "new", "--group", "algebra"]).is_err(),
            "a line needs a title"
        );

        let Command::Line {
            command:
                LineCommand::New {
                    group,
                    title,
                    description,
                    shared,
                },
        } = Cli::try_parse_from([
            "stemma",
            "line",
            "new",
            "--group",
            "algebra",
            "--title",
            "Quotient groups",
            "--description",
            "Does it split?",
        ])
        .unwrap()
        .command
        else {
            panic!("expected a line to start");
        };

        assert_eq!(group, "algebra");
        assert_eq!(title, "Quotient groups");
        assert_eq!(description.as_deref(), Some("Does it split?"));
        assert!(!shared, "a line is its author's until they say otherwise");
    }

    #[test]
    fn a_proposal_is_read_before_it_is_signed() {
        let Command::Proposal {
            command: ProposalCommand::Show { proposal },
        } = Cli::try_parse_from([
            "stemma",
            "proposal",
            "show",
            "prp:01K5YQ4TQ2VZ8H3M9N0P1R2S3T",
        ])
        .unwrap()
        .command
        else {
            panic!("expected a proposal to read");
        };

        assert_eq!(proposal, "prp:01K5YQ4TQ2VZ8H3M9N0P1R2S3T");

        for answer in ["accept", "reject", "ask-again"] {
            assert!(
                Cli::try_parse_from([
                    "stemma",
                    "proposal",
                    answer,
                    "prp:01K5YQ4TQ2VZ8H3M9N0P1R2S3T"
                ])
                .is_ok(),
                "`{answer}` is one of the three answers"
            );
        }

        assert!(
            Cli::try_parse_from(["stemma", "proposal", "accept"]).is_err(),
            "accepting says which words are being signed"
        );
    }

    #[test]
    fn the_machines_of_a_group_can_be_asked_for_from_anywhere() {
        let Command::Machines { group } =
            Cli::try_parse_from(["stemma", "machines", "--group", "algebra"])
                .unwrap()
                .command
        else {
            panic!("expected a listing of machines");
        };

        assert_eq!(group.as_deref(), Some("algebra"));

        let Command::Machines { group } =
            Cli::try_parse_from(["stemma", "machines"]).unwrap().command
        else {
            panic!("expected a listing of machines");
        };

        assert_eq!(group, None, "inside a clone, it is that group");
    }

    #[test]
    fn the_definition_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}

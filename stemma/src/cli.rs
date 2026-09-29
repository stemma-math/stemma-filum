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
    version = stemma_core::VERSION,
    about = "Work with a Stemma server from this machine.",
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
    /// Signs this machine in to a server.
    Login {
        /// The server's address, such as `stemma.example.org`.
        server: String,
    },

    /// Shows the signed-in account.
    Whoami,

    /// Signs this machine out.
    Logout,

    /// Takes in what changed on the server and sends what is here, now.
    Sync,

    /// Checks the lines here and publishes what they declare.
    Build,

    /// Creates, deletes or shares lines.
    Line {
        /// What to do with it.
        #[command(subcommand)]
        command: LineCommand,
    },

    /// Lists a group's machines and their agents.
    Machines {
        /// Which group, when it is not the one this is run inside.
        #[arg(long)]
        group: Option<String>,
    },

    /// Shows, takes or releases the claim on a line.
    Claim {
        /// What to do with it.
        #[command(subcommand)]
        command: ClaimCommand,
    },

    /// Decides between versions of a block changed here and elsewhere.
    Conflict {
        /// What to do about them.
        #[command(subcommand)]
        command: ConflictCommand,
    },

    /// Registers this machine to run jobs, or shows its status.
    Node {
        /// What to do with it.
        #[command(subcommand)]
        command: NodeCommand,
    },

    /// Runs or installs the background service.
    Daemon {
        /// What to do with it.
        #[command(subcommand)]
        command: DaemonCommand,
    },

    /// Starts and lists agent jobs.
    Job {
        /// What to do.
        #[command(subcommand)]
        command: JobCommand,
    },

    /// Reviews and applies agent proposals.
    Proposal {
        /// What to do with it.
        #[command(subcommand)]
        command: ProposalCommand,
    },

    /// Manages a group's verified definitions and audits.
    Vault {
        /// What to do in it.
        #[command(subcommand)]
        command: VaultCommand,
    },

    /// Says which lines here are not sent yet or need a decision, without
    /// contacting the server.
    Status,

    /// Makes a read-only git clone of one of a group's repositories.
    Export {
        /// The group's short name.
        group: String,
        /// Your own repository in the group rather than the shared one.
        #[arg(long)]
        private: bool,
        /// Where the clone goes; a new directory beside you otherwise.
        into: Option<PathBuf>,
    },

    /// Shows where this machine keeps its files.
    Where,

    /// Removes Stemma from this machine: the service, the command line's
    /// link, the machine on the server and the credential, and what it
    /// keeps here if you say so.
    Uninstall {
        /// Remove it even if some lines are not on the server yet.
        #[arg(long)]
        discard_unsent: bool,
        /// Delete what Stemma keeps here without asking.
        #[arg(long, conflicts_with = "keep_data")]
        delete_data: bool,
        /// Keep what Stemma keeps here without asking.
        #[arg(long)]
        keep_data: bool,
    },

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
    /// Lists proposals waiting for review.
    List,

    /// Shows a proposal's changes and checks.
    Show {
        /// The proposal to read.
        proposal: String,
    },

    /// Accepts a proposal.
    Accept {
        /// The proposal to accept.
        proposal: String,
    },

    /// Rejects a proposal.
    Reject {
        /// The proposal to reject.
        proposal: String,
        /// Optional reason kept with the decision.
        #[arg(long)]
        reason: Option<String>,
    },

    /// Rejects a proposal and runs the same task again.
    AskAgain {
        /// The proposal to decline.
        proposal: String,
        /// Instruction for the next attempt.
        #[arg(long)]
        note: Option<String>,
    },

    /// Applies an accepted proposal on this machine.
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
    /// Creates a line.
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

        /// Share it with the group. Without this, it is private.
        #[arg(long)]
        shared: bool,
    },

    /// Deletes one of your private lines. Its history is kept.
    Delete {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,
    },

    /// Shares one of your private lines with the group.
    Share {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,

        /// Share the full history, not only the current state.
        #[arg(long)]
        history: bool,

        /// Only you can edit it; others send proposals.
        #[arg(long)]
        mine_to_write: bool,
    },
}

/// What can be done with the claim on a line.
#[derive(Debug, Subcommand)]
pub enum ClaimCommand {
    /// Claims the line for this machine.
    Take {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,
    },

    /// Asks the machine holding the claim to release it.
    Request {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,
    },

    /// Releases the claim.
    Release {
        /// Which line, when it is not the one this is run inside.
        #[arg(long)]
        line: Option<String>,
    },

    /// Lists claimed lines and where.
    Status,
}

/// What can be done about two versions that have parted.
#[derive(Debug, Subcommand)]
pub enum ConflictCommand {
    /// Lists the conflicting blocks.
    List,

    /// Settles them, keeping one side wherever they disagree.
    Resolve {
        /// Which version stands: `here` or `there`.
        #[arg(long)]
        keep: Keep,
    },
}

/// What can be done with this machine as a node.
#[derive(Debug, Subcommand)]
pub enum NodeCommand {
    /// Registers this machine to run jobs.
    Init {
        /// A group to work for. Given none, it works for every group
        /// this account is in.
        #[arg(long = "group", value_name = "SLUG")]
        groups: Vec<String>,
    },

    /// Shows this machine as the server sees it.
    Status,
}

/// What can be done with the daemon.
#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Runs jobs until stopped.
    Run {
        /// Take at most one job and stop.
        #[arg(long)]
        once: bool,
    },

    /// Installs it as a user service.
    Install,
}

/// What can be done with jobs.
#[derive(Debug, Subcommand)]
pub enum JobCommand {
    /// Starts a job.
    Run(JobRun),

    /// Lists this account's jobs.
    List,

    /// Prints the raw output kept on this machine for one job.
    Output {
        /// The job whose local output to print.
        job: String,
    },

    /// Cancels a queued job or asks a running one to stop.
    Cancel {
        /// The job to cancel.
        job: String,
    },
}

/// What a job asks for, and of whom.
#[derive(Debug, Args)]
pub struct JobRun {
    /// The task: `refute@BLOCK`, `formalize@BLOCK`, `back@BLOCK`,
    /// `realign@BLOCK`, `prove@BLOCK`, `rewrite@BLOCK`,
    /// `develop@BLOCK`, `explore`, `reconcile`, or `custom`.
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

    /// Concrete model offered by the chosen machine.
    #[arg(long)]
    pub model: Option<String>,

    /// Reasoning effort offered by the chosen machine.
    #[arg(long)]
    pub effort: Option<String>,

    /// A custom request. This selects the `custom` task.
    #[arg(long)]
    pub request: Option<String>,

    /// A block the custom request may edit, remove, or add after.
    #[arg(long = "grant-block", value_name = "BLOCK")]
    pub grant_blocks: Vec<stemma_core::id::BlockId>,

    /// Allow a custom request to change the Lean library.
    #[arg(long, requires = "request")]
    pub grant_library: bool,
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

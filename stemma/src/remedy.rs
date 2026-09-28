//! What somebody can do about an error.
//!
//! The command line names a command to run; the application offers a
//! button that does the same. So an error that has a remedy carries it
//! as its root cause, and each surface words it its own way: the command
//! line prints the whole chain, which ends in the command, and the
//! application finds the remedy in the chain and shows its button.

use std::fmt;

/// Something to do that clears an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remedy {
    /// Sign this machine in again.
    SignIn,
    /// Offer this machine to run jobs.
    SetUpMachine,
    /// Send and receive lines now.
    Sync,
    /// Decide between the versions of a block.
    Decide,
}

impl Remedy {
    /// Its name, as the application refers to it.
    pub fn slug(self) -> &'static str {
        match self {
            Self::SignIn => "sign-in",
            Self::SetUpMachine => "set-up",
            Self::Sync => "sync",
            Self::Decide => "decide",
        }
    }

    /// The remedy an error carries, if it carries one.
    pub fn of(error: &anyhow::Error) -> Option<Self> {
        error
            .chain()
            .find_map(|cause| cause.downcast_ref::<Self>().copied())
    }
}

/// The command line's wording: the command to run.
impl fmt::Display for Remedy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SignIn => "run `stemma login`",
            Self::SetUpMachine => "run `stemma node init`",
            Self::Sync => "run `stemma sync`",
            Self::Decide => "`stemma conflict list` shows which blocks",
        })
    }
}

impl std::error::Error for Remedy {}

#[cfg(test)]
mod tests {
    use anyhow::Context;

    use super::*;

    #[test]
    fn the_command_line_reads_the_command_and_the_application_finds_the_remedy() {
        let error = Err::<(), _>(Remedy::Sync)
            .context("this line is not on this machine yet")
            .unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "this line is not on this machine yet: run `stemma sync`"
        );
        assert_eq!(Remedy::of(&error), Some(Remedy::Sync));
        assert_eq!(Remedy::of(&anyhow::anyhow!("nothing to do")), None);
    }
}

//! What this machine has, against what the group pinned.
//!
//! The pin is part of the declaration: two renderings of one line must
//! not differ because two people happened to install different
//! compilers. So the answer here is one of three — the right one, the
//! wrong one, or none — and composing is only allowed by the first.

use std::process::Command;

/// What this machine can do about a pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// The pinned release is the one installed.
    Matches(String),
    /// Something else is installed.
    Differs {
        /// What the group pinned.
        pinned: String,
        /// What is here.
        installed: String,
    },
    /// Nothing is installed.
    Missing {
        /// What the group pinned.
        pinned: String,
    },
}

impl Standing {
    /// Whether this machine may compose the group's documents.
    pub fn may_compose(&self) -> bool {
        matches!(self, Self::Matches(_))
    }
}

impl std::fmt::Display for Standing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Matches(version) => write!(f, "typst {version} is pinned, and installed"),
            Self::Differs { pinned, installed } => write!(
                f,
                "typst {pinned} is pinned and typst {installed} is installed, \
                 so nothing here may be composed"
            ),
            Self::Missing { pinned } => write!(
                f,
                "typst {pinned} is pinned and none is installed, \
                 so nothing here may be composed"
            ),
        }
    }
}

/// How this machine stands against a pin.
pub fn standing(pinned: &str) -> Standing {
    match installed() {
        Some(installed) if installed == pinned => Standing::Matches(installed),
        Some(installed) => Standing::Differs {
            pinned: pinned.to_owned(),
            installed,
        },
        None => Standing::Missing {
            pinned: pinned.to_owned(),
        },
    }
}

/// The same thing, as one line.
pub fn report(pinned: &str) -> String {
    standing(pinned).to_string()
}

/// Which Typst is on this machine, if any.
pub fn installed() -> Option<String> {
    let output = Command::new("typst").arg("--version").output().ok()?;

    if !output.status.success() {
        return None;
    }

    version_in(&String::from_utf8_lossy(&output.stdout))
}

/// The version out of what `typst --version` says.
fn version_in(said: &str) -> Option<String> {
    said.split_whitespace()
        .find(|word| {
            word.chars()
                .next()
                .is_some_and(|first| first.is_ascii_digit())
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_out_of_what_typst_says() {
        assert_eq!(
            version_in("typst 0.13.1 (abcdef)").as_deref(),
            Some("0.13.1")
        );
        assert_eq!(version_in("typst 0.14.0\n").as_deref(), Some("0.14.0"));
        assert_eq!(version_in("something else entirely"), None);
        assert_eq!(version_in(""), None);
    }

    #[test]
    fn only_the_pinned_release_may_compose() {
        assert!(Standing::Matches("0.13.1".to_owned()).may_compose());
        assert!(
            !Standing::Differs {
                pinned: "0.13.1".to_owned(),
                installed: "0.14.0".to_owned(),
            }
            .may_compose()
        );
        assert!(
            !Standing::Missing {
                pinned: "0.13.1".to_owned()
            }
            .may_compose()
        );
    }

    #[test]
    fn what_is_wrong_names_both_versions() {
        let said = Standing::Differs {
            pinned: "0.13.1".to_owned(),
            installed: "0.14.0".to_owned(),
        }
        .to_string();

        assert!(said.contains("0.13.1"));
        assert!(said.contains("0.14.0"));
        assert!(said.contains("may be composed"));
    }
}

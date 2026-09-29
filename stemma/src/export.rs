//! Taking a repository's whole history away.
//!
//! Lines are written through the instance, and nothing here needs git to
//! work. But the history is the group's, and anybody who can read a
//! repository can walk away with it: this makes a git clone of it, which
//! can be fetched again later and never pushed.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::account::session;
use crate::git;
use crate::machine::program;

/// Clones one of a group's repositories into a directory, read-only, and
/// says where it went.
pub fn run(
    home: &crate::home::Home,
    group: &str,
    private: bool,
    into: Option<&Path>,
    out: &mut impl Write,
) -> Result<PathBuf> {
    let (credentials, server) = session::connect(home)?;
    let listing = server.workspace(&credentials.token)?;
    let found = listing
        .groups
        .iter()
        .find(|one| one.slug == group)
        .with_context(|| format!("you are not in a group called {group}"))?;
    stemma_core::id::Slug::parse(&found.slug).context("the server named the group oddly")?;

    let (url, side) = match private {
        true => (&found.personal, "private"),
        false => (&found.shared, "shared"),
    };
    let target = match into {
        Some(into) => into.to_owned(),
        None => std::env::current_dir()?.join(format!("{group}-{side}")),
    };

    if target.exists() {
        bail!("{} is already there", target.display());
    }

    git::clone(url, &target, &helper()?)?;

    writeln!(out, "{}", target.display())?;
    writeln!(out, "  the {side} history of {group}, to keep")?;
    writeln!(
        out,
        "  `git fetch` there brings it up to date; nothing can be pushed back"
    )?;

    Ok(target)
}

/// How git is told to ask the command line for a credential.
///
/// Not whatever is running: the application is not a credential helper,
/// and git running it would open a window and then wait for an answer
/// that a window has no way to give.
fn helper() -> Result<String> {
    // Git reads a helper that is not an absolute path as a name to look
    // for under `git-credential-`, and a leading `!` as a shell command.
    // The second is the one that survives a path with a space in it.
    let program = program::command_line()?;

    Ok(format!("!{} credential", quoted(&program)))
}

/// One path, as a shell word, whatever is in it.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_with_a_space_in_it_is_still_one_word_to_a_shell() {
        assert_eq!(
            quoted(Path::new(
                "/Applications/Stemma 2.app/Contents/MacOS/stemma"
            )),
            "'/Applications/Stemma 2.app/Contents/MacOS/stemma'"
        );
    }
}

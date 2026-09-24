//! Finding the command line on this machine.
//!
//! Two programs are built from this workspace, and one carries the
//! other: the application ships `stemma` beside its own binary. So
//! wherever something has to name the command line to somebody else —
//! to git, as the helper that answers for a credential, or to the
//! supervisor, as the thing to run at login — `current_exe` is the
//! wrong answer half the time. Inside the application it names the
//! application, and naming the application means opening another
//! window instead of running a command.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// What the command line is called wherever it is installed.
pub const NAME: &str = "stemma";

/// Where the command line is on this machine.
///
/// The answer is always a full path. A bare name is no use to either of
/// the things that ask: git reads `stemma` as a helper to look for
/// under `git-credential-`, and a supervisor has no path at all.
pub fn command_line() -> Result<PathBuf> {
    let running = std::env::current_exe().context("cannot find this program")?;

    itself_or_carried(&running)
        .or_else(on_the_path)
        .with_context(|| {
            format!(
                "cannot find the `{NAME}` command line. The application carries \
                 one beside it, and there is none beside {}, nor one on the PATH.",
                running.display()
            )
        })
}

/// The command line the application carries beside its own binary, when
/// the running program has one there.
pub fn carried() -> Option<PathBuf> {
    let running = std::env::current_exe().ok()?;
    let beside = running.parent()?.join(NAME);
    is_a_program(&beside).then_some(beside)
}

/// The command line as reached from the program that is running.
///
/// Either this is it, or this is the application and it is beside it:
/// `target/release` while the workspace is being built, and
/// `Contents/MacOS` once the application is a bundle.
fn itself_or_carried(running: &Path) -> Option<PathBuf> {
    if running.file_name() == Some(OsStr::new(NAME)) {
        return Some(running.to_owned());
    }

    running
        .parent()
        .map(|beside| beside.join(NAME))
        .filter(|carried| is_a_program(carried))
}

/// Whatever is installed, for anything that is neither of the two.
fn on_the_path() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(NAME))
        .find(|candidate| is_a_program(candidate))
}

/// Whether a path is something that could be run.
#[cfg(unix)]
fn is_a_program(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .is_ok_and(|about| about.is_file() && about.permissions().mode() & 0o111 != 0)
}

/// Whether a path is something that could be run.
#[cfg(not(unix))]
fn is_a_program(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("stemma-{name}-{}", std::process::id()));

        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        root
    }

    fn a_program(path: &Path) {
        std::fs::write(path, "#!/bin/sh\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn the_command_line_run_as_itself_is_the_answer() {
        let running = somewhere("program-itself").join(NAME);

        assert_eq!(itself_or_carried(&running), Some(running));
    }

    #[test]
    fn the_application_answers_with_the_one_it_carries() {
        let root = somewhere("program-carried");
        let carried = root.join(NAME);
        a_program(&carried);

        assert_eq!(itself_or_carried(&root.join("stemma-app")), Some(carried));
    }

    #[test]
    fn something_that_only_looks_like_a_program_is_not_one() {
        let root = somewhere("program-placeholder");
        std::fs::write(root.join(NAME), "this build carries no command line\n").unwrap();

        assert_eq!(itself_or_carried(&root.join("stemma-app")), None);
    }
}

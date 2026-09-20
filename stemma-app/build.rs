//! Builds the native Tauri resources.
//!
//! The application carries the command line with it, so installing one
//! installs both. The binary is expected beside this crate, named for
//! the target it was built for, which is what Tauri calls a sidecar.

use std::path::{Path, PathBuf};

fn main() {
    carry_the_command_line();

    tauri_build::build()
}

/// Puts `stemma` where the bundle will pick it up.
///
/// A release build has one; an ordinary `cargo build` of this crate may
/// not, and that must not stop it. Then a placeholder stands in, it is
/// not executable, and the window says this build carries no command
/// line rather than installing something that would not run.
fn carry_the_command_line() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    let beside =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("binaries");
    let carried = beside.join(format!("stemma-{target}"));

    println!("cargo:rerun-if-changed={}", carried.display());

    if std::fs::create_dir_all(&beside).is_err() {
        return;
    }

    if let Some(built) = built_command_line() {
        let _ = std::fs::copy(built, &carried);

        return;
    }

    if !carried.exists() {
        let _ = std::fs::write(
            &carried,
            "This build carries no command line. `just app` builds one.\n",
        );
    }
}

/// The `stemma` binary this workspace has already built, if it has one.
fn built_command_line() -> Option<PathBuf> {
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_owned());
    let workspace = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?)
        .parent()?
        .to_owned();

    [profile.as_str(), "release", "debug"]
        .iter()
        .map(|profile| workspace.join("target").join(profile).join("stemma"))
        .find(|path| is_a_program(path))
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

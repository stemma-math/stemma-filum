//! Running git on this machine.
//!
//! Nothing here needs git to write mathematics: lines go through the
//! instance a revision at a time. Git is only how somebody takes a
//! repository's history away, as a clone they can fetch into and never
//! push from.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Makes a clone, with everything a person needs to use git here by hand.
pub fn clone(url: &str, into: &Path, helper: &str) -> Result<()> {
    let parent = into.parent().context("a clone needs somewhere to go")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("cannot create {}", parent.display()))?;

    run(
        parent,
        &[
            // The credential is never written into the clone. Git asks
            // for it when it needs it, and this program answers.
            "-c",
            &format!("credential.helper={helper}"),
            "clone",
            "--quiet",
            "--",
            url,
            &into.display().to_string(),
        ],
    )?;

    // The same helper is written into the clone, so that a person running
    // git here by hand is not asked for a password they do not have.
    run(into, &["config", "credential.helper", helper])?;

    Ok(())
}

/// Runs git, and turns a failure into something a person can read.
fn run(directory: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        // A prompt nobody can see is a program that has hung.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .with_context(|| format!("cannot run git {}", arguments.join(" ")))?;

    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_says_which_git_command_failed() {
        let root = std::env::temp_dir().join(format!("stemma-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let error = run(&root, &["rev-parse", "HEAD"]).unwrap_err();
        assert!(error.to_string().contains("git rev-parse HEAD failed"));

        std::fs::remove_dir_all(&root).unwrap();
    }
}

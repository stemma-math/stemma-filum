//! Checking a line against the group's pinned Lean environment.
//!
//! The server never runs Lean. A person's machine checks the module beside
//! the Typst source and publishes only this reconstructible digest. Compiler
//! output remains local, just like an agent's raw output.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use stemma_core::artefact::LeanDigest;
use stemma_core::line::{Name, Siglum};
use stemma_core::repository::{self, Pins};

const MARKER: &str = "-- stemma: verifies ";

/// Checks a line when it has a Lean module.
pub fn check(clone: &Path, line: &Name, pins: &Pins) -> Result<Option<LeanDigest>> {
    let module = repository::line_lean_entry(clone, line);
    if !module.is_file() {
        return Ok(None);
    }

    let Some(lean) = pins.lean.as_deref() else {
        return Ok(Some(refused("Lean is not pinned for this group", pins)));
    };
    let Some(mathlib) = pins.mathlib.as_deref() else {
        return Ok(Some(refused("Mathlib is not pinned for this group", pins)));
    };

    let installed = installed_version(clone);
    if installed.as_deref() != Some(lean) {
        let diagnostic = match installed {
            Some(found) => format!("Lean {lean} is pinned but Lean {found} is installed"),
            None => format!("Lean {lean} is pinned but is not available through Lake"),
        };
        return Ok(Some(LeanDigest {
            lean: lean.to_owned(),
            mathlib: mathlib.to_owned(),
            compiled: false,
            statements: Vec::new(),
            diagnostic,
        }));
    }

    if let Some(found) = mathlib_revision(clone)?
        && found != mathlib
    {
        return Ok(Some(LeanDigest {
            lean: lean.to_owned(),
            mathlib: mathlib.to_owned(),
            compiled: false,
            statements: Vec::new(),
            diagnostic: format!("Mathlib {mathlib} is pinned but the Lake manifest has {found}"),
        }));
    }

    let output = Command::new("lake")
        .current_dir(clone)
        .args(["env", "lean"])
        .arg(&module)
        .output()
        .with_context(|| format!("cannot run Lean for {}", module.display()))?;
    let compiled = output.status.success();
    let diagnostic = if compiled {
        String::new()
    } else {
        concise(&String::from_utf8_lossy(&output.stderr))
    };
    let source = std::fs::read_to_string(&module)
        .with_context(|| format!("cannot read {}", module.display()))?;

    Ok(Some(LeanDigest {
        lean: lean.to_owned(),
        mathlib: mathlib.to_owned(),
        compiled,
        statements: if compiled {
            markers(&source)
        } else {
            Vec::new()
        },
        diagnostic,
    }))
}

/// Checks a proposed complete module before it is allowed to travel.
pub fn check_offered(
    clone: &Path,
    pins: &Pins,
    source: &str,
    scratch: &Path,
) -> Result<(), String> {
    let lean = pins
        .lean
        .as_deref()
        .ok_or("Lean is not pinned for this group")?;
    let mathlib = pins
        .mathlib
        .as_deref()
        .ok_or("Mathlib is not pinned for this group")?;
    match installed_version(clone) {
        Some(found) if found == lean => {}
        Some(found) => {
            return Err(format!(
                "Lean {lean} is pinned but Lean {found} is installed"
            ));
        }
        None => {
            return Err(format!(
                "Lean {lean} is pinned but is not available through Lake"
            ));
        }
    }
    match mathlib_revision(clone).map_err(|error| error.to_string())? {
        Some(found) if found != mathlib => {
            return Err(format!(
                "Mathlib {mathlib} is pinned but the Lake manifest has {found}"
            ));
        }
        _ => {}
    }
    if let Some(parent) = scratch.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(scratch, source).map_err(|error| error.to_string())?;
    let output = Command::new("lake")
        .current_dir(clone)
        .args(["env", "lean"])
        .arg(scratch)
        .output()
        .map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(scratch);
    if output.status.success() {
        Ok(())
    } else {
        Err(concise(&String::from_utf8_lossy(&output.stderr)))
    }
}

fn refused(reason: &str, pins: &Pins) -> LeanDigest {
    LeanDigest {
        lean: pins.lean.clone().unwrap_or_default(),
        mathlib: pins.mathlib.clone().unwrap_or_default(),
        compiled: false,
        statements: Vec::new(),
        diagnostic: reason.to_owned(),
    }
}

fn installed_version(clone: &Path) -> Option<String> {
    let output = Command::new("lake")
        .current_dir(clone)
        .args(["env", "lean", "--version"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| version_in(&output.stdout))?
}

fn version_in(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output)
        .split_whitespace()
        .map(|word| word.strip_prefix('v').unwrap_or(word))
        .find(|word| word.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|word| word.trim_end_matches(',').to_owned())
}

fn mathlib_revision(clone: &Path) -> Result<Option<String>> {
    let path = clone.join("lake-manifest.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let manifest: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(manifest["packages"]
        .as_array()
        .and_then(|packages| packages.iter().find(|package| package["name"] == "mathlib"))
        .and_then(|package| package["rev"].as_str())
        .map(str::to_owned))
}

fn markers(source: &str) -> Vec<Siglum> {
    let mut found: Vec<Siglum> = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix(MARKER))
        .filter_map(|written| written.trim().parse().ok())
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Stable Stemma identifiers a Lean module says its declarations establish.
pub fn declared_statements(source: &str) -> Vec<Siglum> {
    markers(source)
}

fn concise(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Lean rejected the module")
        .chars()
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_module_names_the_statements_it_verifies() {
        let found = markers(
            "-- stemma: verifies thm:main\n theorem main : True := by trivial\n\
             -- stemma: verifies thm:main\n-- stemma: verifies lem:step",
        );

        assert_eq!(
            found.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["thm:main", "lem:step"]
        );
    }

    #[test]
    fn the_lean_release_is_read_from_its_version_line() {
        assert_eq!(
            version_in(b"Lean (version 4.24.0, arm64-apple-darwin)"),
            Some("4.24.0".to_owned())
        );
    }
}

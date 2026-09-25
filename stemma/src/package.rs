//! The line package, and putting it where Typst will look for it.
//!
//! The document built from a line begins with `#import
//! "@stemma/line:…"`, and Typst resolves that by reading its own
//! directory of local packages — the same one the `typst` command reads,
//! and not one of Stemma's. So the package is installed there: the
//! document stays an ordinary Typst file that somebody can compose by
//! hand, with nothing of Stemma running.
//!
//! It travels inside this program, so a machine that has the command
//! line has the package, and installing it is a copy and not a
//! download.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use stemma_core::package::{NAME, NAMESPACE, VERSION};

/// The package's own manifest, as Typst reads it.
const MANIFEST: &str = include_str!("../../typst/typst.toml");

/// What the environments are.
const LIBRARY: &str = include_str!("../../typst/lib.typ");

/// Where this version of the package goes.
pub fn directory() -> Result<PathBuf> {
    let data = dirs::data_dir().context("this machine has nowhere to keep Typst's packages")?;

    Ok(data
        .join("typst")
        .join("packages")
        .join(NAMESPACE)
        .join(NAME)
        .join(VERSION))
}

/// Puts it where Typst will find it, and says where that was.
///
/// Doing nothing when it is already there is what makes this safe to
/// call before every composition: a version is written once, and a new
/// version lands beside the old rather than over it, so a line pinned
/// to either still composes.
pub fn install() -> Result<PathBuf> {
    let directory = directory()?;

    if is_written(&directory) {
        return Ok(directory);
    }

    std::fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;

    for (name, contents) in [("typst.toml", MANIFEST), ("lib.typ", LIBRARY)] {
        let path = directory.join(name);

        std::fs::write(&path, contents)
            .with_context(|| format!("cannot write {}", path.display()))?;
    }

    Ok(directory)
}

/// Whether what is there is what this build carries.
fn is_written(directory: &Path) -> bool {
    let same = |name: &str, contents: &str| {
        std::fs::read_to_string(directory.join(name)).is_ok_and(|found| found == contents)
    };

    same("typst.toml", MANIFEST) && same("lib.typ", LIBRARY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_package_carried_is_the_one_the_import_line_names() {
        assert!(MANIFEST.contains(&format!("name = \"{NAME}\"")));
        assert!(MANIFEST.contains(&format!("version = \"{VERSION}\"")));
        assert!(MANIFEST.contains("entrypoint = \"lib.typ\""));
    }

    #[test]
    fn every_function_a_rendered_line_calls_is_defined() {
        for function in stemma_core::package::FUNCTIONS {
            assert!(
                LIBRARY.contains(&format!("#let {function}(")),
                "the package defines no `{function}`"
            );
        }
    }

    #[test]
    fn it_is_installed_where_typst_looks_and_not_where_stemma_does() {
        let directory = directory().expect("a directory");

        assert!(directory.ends_with(format!("typst/packages/{NAMESPACE}/{NAME}/{VERSION}")));
    }
}

//! Replacing local state without exposing partial writes or credentials.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// Writes a complete private file beside its destination, then replaces it.
pub fn replace(path: &Path, contents: &str) -> Result<()> {
    let directory = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;

    let (temporary, mut file) = loop {
        let candidate = directory.join(format!(
            ".stemma-{}-{}.tmp",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => break (Temporary(candidate), file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("cannot write {}", path.display()));
            }
        }
    };

    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("cannot write {}", path.display()))?;
    drop(file);
    std::fs::rename(&temporary.0, path)
        .with_context(|| format!("cannot replace {}", path.display()))
}

/// An identifier the instance handed over, as one file name.
///
/// Identifiers are a prefix, a colon and letters, and the colon is no
/// business of a file system's. Anything else that is not a letter, a
/// digit, `-` or `_` is replaced the same way, so that an identifier can
/// never name a directory above the one it is put in.
pub fn file_name(id: &str) -> String {
    id.chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            _ => '-',
        })
        .collect()
}

/// A temporary file that is removed unless it was renamed into place.
struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_is_complete_and_failed_publication_cleans_up() {
        let root = std::env::temp_dir().join(format!(
            "stemma-storage-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("source.typ");
        replace(&path, "A longer original document.").unwrap();
        replace(&path, "Short.").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Short.");
        let directory = root.join("directory");
        std::fs::create_dir(&directory).unwrap();
        assert!(replace(&directory, "Cannot replace a directory.").is_err());
        assert!(directory.is_dir());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_identifier_is_one_file_name_and_never_a_path() {
        assert_eq!(
            file_name("job:01K5YQ4TQ2VZ8H3M9N0P1R2S3T"),
            "job-01K5YQ4TQ2VZ8H3M9N0P1R2S3T"
        );
        assert_eq!(file_name("lin:../../credentials"), "lin-------credentials");
        assert!(!file_name("a/b\\c").contains(['/', '\\']));
    }
}

//! The credential this machine holds, and the instance it is for.
//!
//! Configuration and credential are kept apart. The address of the
//! instance is something a person may want to read and edit; the token is
//! something only this account should be able to read at all.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// What is kept after signing in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    /// The instance this credential is for.
    pub server: String,
    /// Whose account it acts for.
    pub user: String,
    /// The credential itself.
    pub token: String,
}

impl Credentials {
    /// Reads them, or reports that this machine has none.
    pub fn read(path: &Path) -> Result<Option<Self>> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", path.display()));
            }
        };

        toml::from_str(&text)
            .map(Some)
            .with_context(|| format!("{} is not something this build wrote", path.display()))
    }

    /// Writes them where only this account can read them.
    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory)
                .with_context(|| format!("cannot create {}", directory.display()))?;
        }

        let text = toml::to_string_pretty(self).context("cannot write the credential")?;

        // Written and then narrowed in one step, so there is never a
        // moment when the file exists and anybody can read it.
        write_private(path, &text)
    }

    /// Forgets them.
    pub fn forget(path: &Path) -> Result<bool> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error).with_context(|| format!("cannot remove {}", path.display())),
        }
    }
}

/// Writes a file only this account may read.
fn write_private(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options
        .open(path)
        .with_context(|| format!("cannot write {}", path.display()))?;

    file.write_all(contents.as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("stemma-{name}-{}", std::process::id()))
    }

    #[test]
    fn a_machine_with_no_credential_says_so_rather_than_failing() {
        assert_eq!(
            Credentials::read(Path::new("/nonexistent/credentials.toml")).unwrap(),
            None
        );
    }

    #[test]
    fn what_is_written_is_what_is_read_back() {
        let root = somewhere("credentials");
        let path = root.join("credentials.toml");

        let credentials = Credentials {
            server: "https://stemma.test".to_owned(),
            user: "usr:ana".to_owned(),
            token: "a secret".to_owned(),
        };

        credentials.write(&path).unwrap();
        assert_eq!(Credentials::read(&path).unwrap(), Some(credentials));

        assert!(Credentials::forget(&path).unwrap());
        assert!(!Credentials::forget(&path).unwrap());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn nobody_else_on_the_machine_can_read_it() {
        use std::os::unix::fs::PermissionsExt;

        let root = somewhere("credentials-mode");
        let path = root.join("credentials.toml");

        Credentials {
            server: "https://stemma.test".to_owned(),
            user: "usr:ana".to_owned(),
            token: "a secret".to_owned(),
        }
        .write(&path)
        .unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        std::fs::remove_dir_all(&root).unwrap();
    }
}

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
        let text = toml::to_string_pretty(self).context("cannot write the credential")?;
        crate::storage::replace(path, &text)
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

        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, "old credential").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

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

    #[cfg(unix)]
    #[test]
    fn replacing_a_credential_does_not_follow_a_symbolic_link() {
        let root = somewhere("credentials-symlink");
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("other-file");
        let path = root.join("credentials.toml");
        std::fs::write(&target, "keep this").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();

        Credentials {
            server: "https://stemma.test".to_owned(),
            user: "usr:ana".to_owned(),
            token: "a secret".to_owned(),
        }
        .write(&path)
        .unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep this");
        assert!(
            !std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
}

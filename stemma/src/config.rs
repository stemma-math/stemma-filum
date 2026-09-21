//! What this machine has been told to do.
//!
//! The workspace is where clones land by default, and the map from a
//! group to a path is what lets one live somewhere else: on another disk,
//! beside a paper, wherever its owner wants it. Commands look a group up
//! here before assuming the default.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The configuration file, as written.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Where clones land, when a group does not say otherwise.
    pub workspace: Option<PathBuf>,
    /// Where a particular group's clones are.
    #[serde(default)]
    pub groups: BTreeMap<String, PathBuf>,
    /// What to run for an agent this machine keeps somewhere unusual.
    #[serde(default)]
    pub agents: BTreeMap<String, AgentConfig>,
}

/// How this machine invokes one agent, where the ordinary way is wrong.
///
/// Detection is still detection: this says where to look, and whether
/// anything is there is answered by looking.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    /// The program to run.
    pub command: Option<String>,
}

impl Config {
    /// Reads it, or reports that there is none yet.
    pub fn read(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", path.display()));
            }
        };

        toml::from_str(&text)
            .with_context(|| format!("{} is not valid configuration", path.display()))
    }

    /// Writes it.
    pub fn write(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("cannot write the configuration")?;
        crate::storage::replace(path, &text)
    }

    /// Where clones land.
    pub fn workspace_or(&self, fallback: &Path) -> PathBuf {
        self.workspace
            .clone()
            .unwrap_or_else(|| fallback.to_owned())
    }

    /// Where a group's clones are, which is the default unless it was
    /// given a place of its own.
    pub fn group_directory(&self, group: &str, workspace: &Path) -> PathBuf {
        self.groups
            .get(group)
            .cloned()
            .unwrap_or_else(|| crate::workspace::group_directory(workspace, group))
    }

    /// Remembers that a group lives somewhere other than the default.
    pub fn put_group(&mut self, group: &str, directory: &Path, workspace: &Path) {
        if directory == crate::workspace::group_directory(workspace, group) {
            self.groups.remove(group);
        } else {
            self.groups.insert(group.to_owned(), directory.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_with_no_configuration_still_knows_where_things_go() {
        let config = Config::read(Path::new("/nonexistent/config.toml")).unwrap();

        assert_eq!(
            config.group_directory("algebra", Path::new("/home/ana/stemma")),
            Path::new("/home/ana/stemma/algebra")
        );
    }

    #[test]
    fn a_group_can_live_somewhere_else() {
        let workspace = Path::new("/home/ana/stemma");
        let mut config = Config::default();

        config.put_group("algebra", Path::new("/data/algebra"), workspace);
        assert_eq!(
            config.group_directory("algebra", workspace),
            Path::new("/data/algebra")
        );

        // Putting it back where it would have gone stops it being worth
        // recording.
        config.put_group("algebra", Path::new("/home/ana/stemma/algebra"), workspace);
        assert!(config.groups.is_empty());
    }

    #[test]
    fn what_is_written_is_what_is_read_back() {
        let root = std::env::temp_dir().join(format!("stemma-config-{}", std::process::id()));
        let path = root.join("config.toml");

        let mut config = Config {
            workspace: Some(PathBuf::from("/data/stemma")),
            ..Config::default()
        };
        config.put_group(
            "algebra",
            Path::new("/data/algebra"),
            Path::new("/data/stemma"),
        );

        config.write(&path).unwrap();
        assert_eq!(Config::read(&path).unwrap(), config);

        std::fs::remove_dir_all(&root).unwrap();
    }
}

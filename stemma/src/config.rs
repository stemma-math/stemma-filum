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
    /// Defaults for agent jobs on this machine.
    #[serde(default)]
    pub jobs: JobConfig,
}

/// How this machine invokes one agent, where the ordinary way is wrong.
///
/// Detection is still detection: this says where to look, and whether
/// anything is there is answered by looking.
///
/// Anything else in it is ignored rather than refused: earlier builds read
/// a model and an effort here, and a machine configured for them still
/// runs. Agents now run with their own defaults.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    /// The program to run.
    pub command: Option<String>,
}

/// Machine-wide job settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobConfig {
    /// Maximum running time for an agent job.
    pub timeout_minutes: u64,
    /// How many gigabytes the workspaces of finished jobs may take
    /// together before the oldest are removed.
    pub workspaces_gigabytes: u64,
    /// Whether the background service keeps running jobs, and lines in
    /// step, while the application is closed. When not, it stops with the
    /// application and starts again when it opens.
    pub while_closed: bool,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self {
            timeout_minutes: 20,
            workspaces_gigabytes: 10,
            while_closed: true,
        }
    }
}

impl JobConfig {
    /// How many bytes the workspaces may take together.
    pub fn workspace_limit(&self) -> u64 {
        self.workspaces_gigabytes.saturating_mul(1_000_000_000)
    }
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
            .unwrap_or_else(|| crate::repository::group_directory(workspace, group))
    }

    /// Remembers that a group lives somewhere other than the default.
    pub fn put_group(&mut self, group: &str, directory: &Path, workspace: &Path) {
        if directory == crate::repository::group_directory(workspace, group) {
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

    #[test]
    fn a_model_and_an_effort_configured_by_an_earlier_build_are_ignored() {
        let config: Config = toml::from_str(
            r#"
            [agents.codex]
            command = "/opt/codex"
            models = ["gpt-5-codex"]
            efforts = ["medium", "high"]
            model = "gpt-5-codex"
            effort = "medium"

            [agents.codex.tasks.prove]
            effort = "high"
            "#,
        )
        .unwrap();

        assert_eq!(
            config.agents["codex"].command.as_deref(),
            Some("/opt/codex")
        );
        assert_eq!(config.jobs, JobConfig::default());
    }
}

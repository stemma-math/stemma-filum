//! Where the client keeps its things.
//!
//! The ordinary places: configuration under `~/.config`, state under
//! `~/.local/state`, discardable work under `~/.cache`. `STEMMA_HOME`
//! moves all three at once, which is what makes a second account on one
//! machine, or a test, possible without moving anything else.
//!
//! The workspace is deliberately somewhere visible. Repositories are the
//! researcher's, not the program's, and hiding them under a dot directory
//! would say otherwise.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The variable that moves everything.
pub const HOME_VARIABLE: &str = "STEMMA_HOME";

/// The environment the client was started in.
#[derive(Debug, Default, Clone)]
pub struct Environment {
    values: BTreeMap<String, String>,
}

impl Environment {
    /// The variables this process was started with.
    pub fn from_process() -> Self {
        Self::from_pairs(std::env::vars_os())
    }

    /// A set built from arbitrary pairs.
    pub fn from_pairs<K, V>(pairs: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<OsString>,
        V: Into<OsString>,
    {
        Self {
            values: pairs
                .into_iter()
                .filter_map(|(key, value)| {
                    Some((
                        key.into().into_string().ok()?,
                        value.into().into_string().ok()?,
                    ))
                })
                .collect(),
        }
    }

    /// One variable, ignoring an empty one.
    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }

    /// One variable as a path.
    fn path(&self, name: &str) -> Option<PathBuf> {
        self.get(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }
}

/// Every directory the client uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    config: PathBuf,
    state: PathBuf,
    cache: PathBuf,
    workspace: PathBuf,
}

impl Home {
    /// Works out where everything goes.
    pub fn locate(environment: &Environment) -> Self {
        // One variable moves the lot, so an instance kept apart from
        // another one stays apart in every directory rather than in some.
        if let Some(root) = environment.path(HOME_VARIABLE) {
            return Self {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
                workspace: root.join("workspace"),
            };
        }

        let home = environment
            .path("HOME")
            .unwrap_or_else(|| PathBuf::from("."));
        let under = |variable: &str, fallback: &str| {
            environment
                .path(variable)
                .unwrap_or_else(|| home.join(fallback))
                .join("stemma")
        };

        Self {
            config: under("XDG_CONFIG_HOME", ".config"),
            state: under("XDG_STATE_HOME", ".local/state"),
            cache: under("XDG_CACHE_HOME", ".cache"),
            workspace: home.join("stemma"),
        }
    }

    /// Where the configuration file is.
    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    /// Where the credential is kept.
    pub fn credentials_file(&self) -> PathBuf {
        self.state.join("credentials.toml")
    }

    /// The directory holding configuration.
    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    /// The directory holding state.
    pub fn state_dir(&self) -> &Path {
        &self.state
    }

    /// The directory holding what can be thrown away.
    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    /// Where group repositories land by default.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_of(pairs: &[(&str, &str)]) -> Home {
        Home::locate(&Environment::from_pairs(pairs.iter().copied()))
    }

    #[test]
    fn everything_sits_in_the_ordinary_places() {
        let home = home_of(&[("HOME", "/home/ana")]);

        assert_eq!(
            home.config_file(),
            Path::new("/home/ana/.config/stemma/config.toml")
        );
        assert_eq!(
            home.credentials_file(),
            Path::new("/home/ana/.local/state/stemma/credentials.toml")
        );
        assert_eq!(home.cache_dir(), Path::new("/home/ana/.cache/stemma"));
    }

    #[test]
    fn the_workspace_is_somewhere_a_person_can_see_it() {
        assert_eq!(
            home_of(&[("HOME", "/home/ana")]).workspace(),
            Path::new("/home/ana/stemma")
        );
    }

    #[test]
    fn the_usual_variables_are_obeyed() {
        let home = home_of(&[
            ("HOME", "/home/ana"),
            ("XDG_CONFIG_HOME", "/elsewhere/config"),
            ("XDG_STATE_HOME", "/elsewhere/state"),
        ]);

        assert_eq!(home.config_dir(), Path::new("/elsewhere/config/stemma"));
        assert_eq!(home.state_dir(), Path::new("/elsewhere/state/stemma"));
        assert_eq!(home.cache_dir(), Path::new("/home/ana/.cache/stemma"));
    }

    #[test]
    fn one_variable_moves_all_of_it() {
        let home = home_of(&[
            ("HOME", "/home/ana"),
            ("XDG_CONFIG_HOME", "/elsewhere/config"),
            (HOME_VARIABLE, "/tmp/second-account"),
        ]);

        assert_eq!(home.config_dir(), Path::new("/tmp/second-account/config"));
        assert_eq!(home.state_dir(), Path::new("/tmp/second-account/state"));
        assert_eq!(home.cache_dir(), Path::new("/tmp/second-account/cache"));
        assert_eq!(home.workspace(), Path::new("/tmp/second-account/workspace"));
    }

    #[test]
    fn a_relative_setting_is_ignored_rather_than_obeyed() {
        // Where the program was started is nowhere to keep somebody's work.
        let home = home_of(&[("HOME", "/home/ana"), (HOME_VARIABLE, "somewhere")]);

        assert_eq!(home.config_dir(), Path::new("/home/ana/.config/stemma"));
    }
}

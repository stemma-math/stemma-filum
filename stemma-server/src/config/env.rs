//! The environment the server was started in.
//!
//! Every setting has an environment variable, so an instance can be
//! provisioned without anybody answering a question at a terminal. Reading
//! them through this type rather than through
//! [`std::env`](mod@std::env) keeps resolution testable.

use std::collections::BTreeMap;
use std::ffi::OsString;

/// The prefix every variable shares.
pub const PREFIX: &str = "STEMMA_SERVER_";

/// A set of environment variables.
#[derive(Debug, Default, Clone)]
pub struct Environment {
    values: BTreeMap<String, String>,
}

impl Environment {
    /// The variables this process was started with.
    pub fn from_process() -> Self {
        Self::from_pairs(std::env::vars_os())
    }

    /// A set built from arbitrary pairs, for tests and for provisioning.
    pub fn from_pairs<K, V>(pairs: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<OsString>,
        V: Into<OsString>,
    {
        let values = pairs
            .into_iter()
            .filter_map(|(key, value)| {
                let key = key.into().into_string().ok()?;
                let value = value.into().into_string().ok()?;
                key.starts_with(PREFIX).then_some((key, value))
            })
            .collect();

        Self { values }
    }

    /// The value of `STEMMA_SERVER_<suffix>`, ignoring an empty one.
    ///
    /// An empty variable means "unset" because that is what a provisioning
    /// tool writes when it has nothing to say, and because the alternative is
    /// an instance whose public address is the empty string.
    pub fn get(&self, suffix: &str) -> Option<&str> {
        self.values
            .get(&format!("{PREFIX}{suffix}"))
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_prefixed_variables_are_kept() {
        let environment = Environment::from_pairs([
            ("STEMMA_SERVER_LISTEN", "127.0.0.1:9000"),
            ("HOME", "/root"),
        ]);

        assert_eq!(environment.get("LISTEN"), Some("127.0.0.1:9000"));
        assert_eq!(environment.get("HOME"), None);
    }

    #[test]
    fn an_empty_variable_counts_as_unset() {
        let environment = Environment::from_pairs([("STEMMA_SERVER_PUBLIC_URL", "   ")]);
        assert_eq!(environment.get("PUBLIC_URL"), None);
    }
}

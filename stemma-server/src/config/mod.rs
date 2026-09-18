//! Reading the instance's configuration.
//!
//! Three sources, in order: a flag beats an environment variable, which beats
//! the file, which beats the built-in default. Every setting has all three
//! forms, so an instance can be provisioned entirely without a terminal.
//!
//! What lives here is infrastructure: how the process comes up, which it
//! cannot change while it is up. Policy — whether registration is open, how
//! long an invitation lasts, where mail goes — is answered by the database
//! and edited from the admin area. The file may seed those values the first
//! time the database is created; after that the database is authoritative.

mod env;
mod listen;
mod paths;
mod public_url;

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use self::env::{Environment, PREFIX as ENV_PREFIX};
pub use self::listen::{BadListen, Listen};
pub use self::paths::Paths;
pub use self::public_url::{BadPublicUrl, PublicUrl};

/// Where the configuration file lives unless told otherwise.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/stemma/server.toml";

/// Where the file holding secrets lives unless told otherwise.
pub const DEFAULT_ENV_PATH: &str = "/etc/stemma/server.env";

/// Where durable state lives unless told otherwise.
pub const DEFAULT_DATA_DIR: &str = "/var/lib/stemma";

/// The address bound unless told otherwise.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";

/// The system user the service runs as unless told otherwise.
pub const DEFAULT_SYSTEM_USER: &str = "stemma";

/// How long an invitation lasts when nobody has chosen.
pub const DEFAULT_INVITATION_TTL_DAYS: u32 = 14;

/// The configuration file, as written.
///
/// Unknown keys are refused rather than ignored: a misspelled setting that is
/// silently dropped is an instance running on defaults its operator believes
/// they changed.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    /// The absolute address the instance answers on.
    pub public_url: Option<String>,
    /// The address to bind.
    pub listen: Option<String>,
    /// The state directory.
    pub data_dir: Option<PathBuf>,
    /// Who may create accounts and request groups.
    #[serde(default)]
    pub registration: RegistrationTable,
    /// How long invitations last.
    #[serde(default)]
    pub invitations: InvitationsTable,
    /// Outgoing mail. Absent means link-only mode.
    pub mail: Option<MailTable>,
}

/// The `[registration]` table.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationTable {
    /// Whether anyone may create an account.
    pub open: Option<bool>,
    /// Whether account holders may request a group.
    pub group_requests: Option<bool>,
}

/// The `[invitations]` table.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvitationsTable {
    /// How many days an invitation stays usable.
    pub ttl_days: Option<u32>,
}

/// The `[mail]` table.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailTable {
    /// The address outgoing mail comes from.
    pub from: Option<String>,
    /// The SMTP server to hand mail to.
    pub smtp_url: Option<String>,
    /// The name of the environment variable holding the SMTP password.
    pub password_env: Option<String>,
}

impl ConfigFile {
    /// Reads and parses the file at `path`.
    pub fn read(path: &Path) -> Result<Self, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ConfigError::Missing {
                    path: path.to_owned(),
                });
            }
            Err(source) => {
                return Err(ConfigError::Unreadable {
                    path: path.to_owned(),
                    source,
                });
            }
        };

        toml::from_str(&text).map_err(|error| ConfigError::Malformed {
            path: path.to_owned(),
            message: error.message().to_owned(),
        })
    }
}

/// Values given on the command line, which beat everything else.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    /// The public address.
    pub public_url: Option<String>,
    /// The address to bind.
    pub listen: Option<String>,
    /// The state directory.
    pub data_dir: Option<PathBuf>,
}

/// Policy the file offers the database the first time it is created.
///
/// Every field is optional because an absent one means "whatever the
/// database already decided", and because after the first run the database
/// is the answer to all of them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PolicySeed {
    /// Whether anyone may create an account.
    pub registration_open: Option<bool>,
    /// Whether account holders may request a group.
    pub group_requests_open: Option<bool>,
    /// How many days an invitation stays usable.
    pub invitation_ttl_days: Option<u32>,
    /// Outgoing mail, when it is configured at all.
    pub mail: Option<MailSeed>,
}

/// Outgoing mail, as far as the file and the environment describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailSeed {
    /// The address outgoing mail comes from.
    pub from: String,
    /// The SMTP server to hand mail to.
    pub smtp_url: String,
    /// The environment variable holding the password, never the password.
    pub password_env: Option<String>,
}

/// The infrastructure settings, resolved and validated.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The absolute address every link is built from.
    pub public_url: PublicUrl,
    /// The address the server binds.
    pub listen: Listen,
    /// Every path the server uses.
    pub paths: Paths,
    /// What the file offers the database on first run.
    pub policy_seed: PolicySeed,
    /// The file these came from.
    pub source: PathBuf,
}

impl Settings {
    /// Where the configuration file is, according to flag, environment and
    /// default in that order.
    pub fn config_path(flag: Option<&Path>, environment: &Environment) -> PathBuf {
        flag.map(Path::to_owned)
            .or_else(|| environment.get("CONFIG").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH))
    }

    /// Reads the file at `path` and resolves it against the environment and
    /// the command line.
    pub fn load(
        path: &Path,
        environment: &Environment,
        overrides: &Overrides,
    ) -> Result<Self, ConfigError> {
        Self::resolve(ConfigFile::read(path)?, path, environment, overrides)
    }

    /// Resolves an already parsed file.
    pub fn resolve(
        file: ConfigFile,
        source: &Path,
        environment: &Environment,
        overrides: &Overrides,
    ) -> Result<Self, ConfigError> {
        let policy_seed = PolicySeed::resolve(&file, environment)?;

        let public_url = overrides
            .public_url
            .clone()
            .or_else(|| environment.get("PUBLIC_URL").map(str::to_owned))
            .or(file.public_url)
            .ok_or(ConfigError::NoPublicUrl)?;

        let listen = overrides
            .listen
            .clone()
            .or_else(|| environment.get("LISTEN").map(str::to_owned))
            .or(file.listen)
            .unwrap_or_else(|| DEFAULT_LISTEN.to_owned());

        let data_dir = overrides
            .data_dir
            .clone()
            .or_else(|| environment.get("DATA_DIR").map(PathBuf::from))
            .or(file.data_dir)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));

        if !data_dir.is_absolute() {
            return Err(ConfigError::RelativeDataDir(data_dir));
        }

        Ok(Self {
            public_url: PublicUrl::parse(&public_url)?,
            listen: listen.parse()?,
            paths: Paths::new(data_dir),
            policy_seed,
            source: source.to_owned(),
        })
    }
}

impl PolicySeed {
    /// Collects what the file and the environment say about policy.
    fn resolve(file: &ConfigFile, environment: &Environment) -> Result<Self, ConfigError> {
        let mail_table = file.mail.clone().unwrap_or_default();

        let from = environment
            .get("MAIL_FROM")
            .map(str::to_owned)
            .or(mail_table.from);
        let smtp_url = environment
            .get("MAIL_SMTP_URL")
            .map(str::to_owned)
            .or(mail_table.smtp_url);
        let password_env = environment
            .get("MAIL_PASSWORD_ENV")
            .map(str::to_owned)
            .or(mail_table.password_env);

        let mail = match (from, smtp_url) {
            (Some(from), Some(smtp_url)) => Some(MailSeed {
                from,
                smtp_url,
                password_env,
            }),
            (None, None) => None,
            (None, Some(_)) => return Err(ConfigError::IncompleteMail { missing: "from" }),
            (Some(_), None) => {
                return Err(ConfigError::IncompleteMail {
                    missing: "smtp_url",
                });
            }
        };

        Ok(Self {
            registration_open: read_bool(environment, "REGISTRATION_OPEN")?
                .or(file.registration.open),
            group_requests_open: read_bool(environment, "REGISTRATION_GROUP_REQUESTS")?
                .or(file.registration.group_requests),
            invitation_ttl_days: read_number(environment, "INVITATIONS_TTL_DAYS")?
                .or(file.invitations.ttl_days),
            mail,
        })
    }
}

/// Reads a boolean setting from the environment.
fn read_bool(environment: &Environment, suffix: &str) -> Result<Option<bool>, ConfigError> {
    match environment.get(suffix) {
        None => Ok(None),
        Some("true" | "yes" | "1") => Ok(Some(true)),
        Some("false" | "no" | "0") => Ok(Some(false)),
        Some(value) => Err(ConfigError::BadValue {
            setting: format!("{ENV_PREFIX}{suffix}"),
            expected: "`true` or `false`",
            value: value.to_owned(),
        }),
    }
}

/// Reads a whole-number setting from the environment.
fn read_number(environment: &Environment, suffix: &str) -> Result<Option<u32>, ConfigError> {
    match environment.get(suffix) {
        None => Ok(None),
        Some(value) => value.parse().map(Some).map_err(|_| ConfigError::BadValue {
            setting: format!("{ENV_PREFIX}{suffix}"),
            expected: "a whole number of days",
            value: value.to_owned(),
        }),
    }
}

/// Why the configuration could not be used.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// There is no file where one was expected.
    #[error("there is no configuration file at {}", .path.display())]
    Missing {
        /// Where the file was looked for.
        path: PathBuf,
    },
    /// The file is there but cannot be read.
    #[error("cannot read {}: {source}", .path.display())]
    Unreadable {
        /// The file that could not be read.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// The file is not the configuration it claims to be.
    #[error("{} is not valid configuration: {message}", .path.display())]
    Malformed {
        /// The file that could not be parsed.
        path: PathBuf,
        /// What the parser said.
        message: String,
    },
    /// No public address anywhere.
    #[error(
        "the public address is not set: put `public_url` in the configuration file, \
         or set {ENV_PREFIX}PUBLIC_URL"
    )]
    NoPublicUrl,
    /// The public address cannot build links.
    #[error(transparent)]
    PublicUrl(#[from] BadPublicUrl),
    /// The listen address cannot be bound.
    #[error(transparent)]
    Listen(#[from] BadListen),
    /// The state directory is relative, so it would depend on where the
    /// process happened to be started.
    #[error("the state directory must be an absolute path, and `{}` is not", .0.display())]
    RelativeDataDir(PathBuf),
    /// A setting was given in a form it does not take.
    #[error("{setting} must be {expected}, and `{value}` is not")]
    BadValue {
        /// The setting that was wrong.
        setting: String,
        /// What it accepts.
        expected: &'static str,
        /// What was given.
        value: String,
    },
    /// Half a mail configuration, which would fail at the worst moment.
    #[error("outgoing mail needs both `from` and `smtp_url`, and `{missing}` is missing")]
    IncompleteMail {
        /// The half that is absent.
        missing: &'static str,
    },
}

/// The commented file a fresh installation starts from.
pub fn skeleton() -> String {
    format!(
        r#"# Stemma server configuration.
#
# This file holds infrastructure: how the process comes up. Everything else —
# registration, invitation lifetime, outgoing mail — is edited from the admin
# area once the instance is running. The values below seed those settings the
# first time the database is created.
#
# Every setting can also be given as an environment variable
# ({ENV_PREFIX}PUBLIC_URL, {ENV_PREFIX}LISTEN, and so on), which wins over
# this file. Secrets belong in {DEFAULT_ENV_PATH}, never here: this file
# should stay safe to paste into a bug report.

# The absolute address the instance answers on. Required: every invitation and
# password reset link is built from it.
public_url = "https://stemma.example.org"

# The address to bind. TLS belongs to the reverse proxy in front.
listen = "{DEFAULT_LISTEN}"          # or "unix:/run/stemma/server.sock"

# All durable state. This is the directory to back up.
data_dir = "{DEFAULT_DATA_DIR}"

[registration]
open = true            # anyone may create an account
group_requests = true  # account holders may request a group

[invitations]
ttl_days = {DEFAULT_INVITATION_TTL_DAYS}

# Outgoing mail. With this table absent the instance runs in link-only mode:
# invitations and password resets still work, as links shown to whoever
# created them.
#
# [mail]
# from = "stemma@example.org"
# smtp_url = "smtp://localhost:25"
# password_env = "STEMMA_SMTP_PASSWORD"
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> ConfigFile {
        toml::from_str(text).expect("the text should parse")
    }

    fn resolve(file: ConfigFile) -> Result<Settings, ConfigError> {
        Settings::resolve(
            file,
            Path::new(DEFAULT_CONFIG_PATH),
            &Environment::default(),
            &Overrides::default(),
        )
    }

    #[test]
    fn the_skeleton_is_valid_configuration() {
        let settings = resolve(parse(&skeleton())).unwrap();

        assert_eq!(settings.public_url.as_str(), "https://stemma.example.org");
        assert_eq!(settings.listen.to_string(), DEFAULT_LISTEN);
        assert_eq!(settings.paths.data_dir(), Path::new(DEFAULT_DATA_DIR));
        assert_eq!(settings.policy_seed.registration_open, Some(true));
        assert_eq!(
            settings.policy_seed.invitation_ttl_days,
            Some(DEFAULT_INVITATION_TTL_DAYS)
        );
        assert_eq!(settings.policy_seed.mail, None);
    }

    #[test]
    fn a_misspelled_setting_is_refused() {
        let error = toml::from_str::<ConfigFile>("publik_url = \"https://example.org\"");
        assert!(error.is_err());
    }

    #[test]
    fn the_environment_beats_the_file_and_a_flag_beats_both() {
        let file = parse(r#"public_url = "https://from-file.example.org""#);
        let environment =
            Environment::from_pairs([("STEMMA_SERVER_PUBLIC_URL", "https://from-env.example.org")]);

        let from_environment = Settings::resolve(
            file.clone(),
            Path::new(DEFAULT_CONFIG_PATH),
            &environment,
            &Overrides::default(),
        )
        .unwrap();
        assert_eq!(
            from_environment.public_url.as_str(),
            "https://from-env.example.org"
        );

        let overrides = Overrides {
            public_url: Some("https://from-flag.example.org".to_owned()),
            ..Overrides::default()
        };
        let from_flag = Settings::resolve(
            file,
            Path::new(DEFAULT_CONFIG_PATH),
            &environment,
            &overrides,
        )
        .unwrap();
        assert_eq!(
            from_flag.public_url.as_str(),
            "https://from-flag.example.org"
        );
    }

    #[test]
    fn an_instance_can_be_configured_from_the_environment_alone() {
        let environment = Environment::from_pairs([
            ("STEMMA_SERVER_PUBLIC_URL", "https://stemma.example.org"),
            ("STEMMA_SERVER_DATA_DIR", "/srv/stemma"),
            ("STEMMA_SERVER_MAIL_FROM", "stemma@example.org"),
            ("STEMMA_SERVER_MAIL_SMTP_URL", "smtp://localhost:25"),
        ]);

        let settings = Settings::resolve(
            ConfigFile::default(),
            Path::new(DEFAULT_CONFIG_PATH),
            &environment,
            &Overrides::default(),
        )
        .unwrap();

        assert_eq!(
            settings.paths.database(),
            Path::new("/srv/stemma/server.db")
        );
        assert_eq!(
            settings.policy_seed.mail.unwrap().from,
            "stemma@example.org"
        );
    }

    #[test]
    fn an_instance_without_a_public_address_does_not_resolve() {
        assert!(matches!(
            resolve(ConfigFile::default()),
            Err(ConfigError::NoPublicUrl)
        ));
    }

    #[test]
    fn half_a_mail_configuration_is_refused() {
        let file = parse(
            r#"
            public_url = "https://stemma.example.org"

            [mail]
            smtp_url = "smtp://localhost:25"
            "#,
        );

        assert!(matches!(
            resolve(file),
            Err(ConfigError::IncompleteMail { missing: "from" })
        ));
    }

    #[test]
    fn no_mail_table_is_link_only_mode_and_not_an_error() {
        let file = parse(r#"public_url = "https://stemma.example.org""#);
        assert_eq!(resolve(file).unwrap().policy_seed.mail, None);
    }

    #[test]
    fn a_relative_state_directory_is_refused() {
        let file = parse(
            r#"
            public_url = "https://stemma.example.org"
            data_dir = "stemma-data"
            "#,
        );

        assert!(matches!(
            resolve(file),
            Err(ConfigError::RelativeDataDir(_))
        ));
    }

    #[test]
    fn a_boolean_given_as_prose_is_refused_by_name() {
        let environment = Environment::from_pairs([
            ("STEMMA_SERVER_PUBLIC_URL", "https://stemma.example.org"),
            ("STEMMA_SERVER_REGISTRATION_OPEN", "perhaps"),
        ]);

        let error = Settings::resolve(
            ConfigFile::default(),
            Path::new(DEFAULT_CONFIG_PATH),
            &environment,
            &Overrides::default(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("STEMMA_SERVER_REGISTRATION_OPEN")
        );
    }

    #[test]
    fn a_missing_file_says_where_it_looked() {
        let error = ConfigFile::read(Path::new("/nonexistent/stemma/server.toml")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("/nonexistent/stemma/server.toml")
        );
    }
}

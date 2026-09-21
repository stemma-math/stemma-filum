//! Would the instance start right now, and if not, why.
//!
//! One report answers that question, whether an operator asked for it or
//! `serve` asked on their behalf at startup. Findings come in two classes,
//! and the distinction is what makes the startup check safe to run every
//! time: a fatal finding stops the server, a warning is reported and the
//! server carries on.
//!
//! Mail is deliberately never fatal. A provider having a bad afternoon must
//! not be the reason a research group cannot reach its own server, and the
//! path that works without mail is the one every instance already uses.

use std::fmt;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use crate::config::{ConfigError, Listen, MailSeed, Settings};
use crate::db;
use crate::system;

/// How long to wait for an SMTP host to answer before giving up on it.
const SMTP_TIMEOUT: Duration = Duration::from_secs(3);

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Reported, and the instance still starts.
    Warning,
    /// The instance refuses to start.
    Fatal,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Warning => "warning",
            Self::Fatal => "fatal",
        })
    }
}

/// One thing that is wrong, or worth knowing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// How much it matters.
    pub severity: Severity,
    /// What is wrong.
    pub message: String,
    /// What to do about it.
    pub remedy: Option<String>,
}

impl Finding {
    /// A finding that stops the server.
    pub fn fatal(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Fatal,
            message: message.into(),
            remedy: None,
        }
    }

    /// A finding the operator should know about.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            remedy: None,
        }
    }

    /// Says what to do about the finding.
    pub fn remedy(mut self, remedy: impl Into<String>) -> Self {
        self.remedy = Some(remedy.into());
        self
    }
}

/// Everything one run of the check found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// The findings, in the order they were made.
    pub findings: Vec<Finding>,
}

impl Report {
    /// A report holding a single finding.
    pub fn of(finding: Finding) -> Self {
        Self {
            findings: vec![finding],
        }
    }

    /// Adds a finding.
    pub fn push(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    /// How many findings of `severity` there are.
    pub fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    }

    /// Whether the instance may start.
    ///
    /// Under `strict` a warning is enough to stop it, which is what a
    /// deployment pipeline wants and what a running instance does not.
    pub fn allows_startup(&self, strict: bool) -> bool {
        self.count(Severity::Fatal) == 0 && !(strict && self.count(Severity::Warning) > 0)
    }

    /// One line saying how it went.
    pub fn summary(&self) -> String {
        let fatal = self.count(Severity::Fatal);
        let warnings = self.count(Severity::Warning);

        match (fatal, warnings) {
            (0, 0) => "Everything the server needs is in place.".to_owned(),
            (0, warnings) => format!(
                "{}, and nothing that stops the server.",
                plural(warnings, "warning")
            ),
            (fatal, 0) => format!(
                "{} to fix before the server will start.",
                plural(fatal, "problem")
            ),
            (fatal, warnings) => format!(
                "{} to fix before the server will start, and {}.",
                plural(fatal, "problem"),
                plural(warnings, "warning")
            ),
        }
    }
}

impl Report {
    /// The findings with the ones that stop the server first, each class
    /// still in the order it was found.
    pub fn ordered(&self) -> Vec<&Finding> {
        let mut ordered: Vec<&Finding> = self.findings.iter().collect();
        ordered.sort_by_key(|finding| std::cmp::Reverse(finding.severity));
        ordered
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for finding in self.ordered() {
            writeln!(f, "{:<8} {}", finding.severity.to_string(), finding.message)?;

            if let Some(remedy) = &finding.remedy {
                writeln!(f, "{:<8} {remedy}", "")?;
            }
        }

        write!(f, "{}", self.summary())
    }
}

/// Renders a count with its noun.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Turns a configuration that could not be read into a report.
///
/// Nothing else can be inspected without it, so this is always the whole
/// report rather than one finding among many.
pub fn from_config_error(error: &ConfigError) -> Report {
    let remedy = match error {
        ConfigError::Missing { .. } => {
            Some("run `stemma-server install` to write one, or pass --config".to_owned())
        }
        ConfigError::NoPublicUrl => {
            Some("every invitation and reset link is built from it".to_owned())
        }
        _ => None,
    };

    let mut finding = Finding::fatal(error.to_string());
    finding.remedy = remedy;
    Report::of(finding)
}

/// Inspects an instance whose configuration did resolve.
pub fn inspect(settings: &Settings, service_user: &str) -> Report {
    let mut report = Report::default();

    inspect_config_file(&mut report, &settings.source);
    inspect_state_directory(&mut report, settings, service_user);
    inspect_keys(&mut report, settings);
    inspect_database(&mut report, settings);
    inspect_repositories(&mut report, settings);
    inspect_listen(&mut report, &settings.listen);
    inspect_mail(&mut report, settings.policy_seed.mail.as_ref());

    report
}

/// The configuration file itself.
fn inspect_config_file(report: &mut Report, path: &Path) {
    if let Some(mode) = mode_of(path)
        && mode & 0o007 != 0
    {
        report.push(
            Finding::warning(format!(
                "{} can be read by anyone on this machine",
                path.display()
            ))
            .remedy(format!("chmod 0640 {}", path.display())),
        );
    }
}

/// The directory holding everything that must survive.
fn inspect_state_directory(report: &mut Report, settings: &Settings, service_user: &str) {
    let data_dir = settings.paths.data_dir();

    let metadata = match std::fs::metadata(data_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            report.push(
                Finding::fatal(format!(
                    "the state directory {} does not exist",
                    data_dir.display()
                ))
                .remedy("run `stemma-server install` to create it".to_owned()),
            );
            return;
        }
        Err(error) => {
            report.push(Finding::fatal(format!(
                "cannot look at the state directory {}: {error}",
                data_dir.display()
            )));
            return;
        }
    };

    if !metadata.is_dir() {
        report.push(Finding::fatal(format!(
            "{} is not a directory",
            data_dir.display()
        )));
        return;
    }

    inspect_mode(report, data_dir, 0o750, Severity::Fatal);
    inspect_owner(report, data_dir, service_user);
    inspect_writability(report, data_dir);
}

/// The keys generated at first run.
fn inspect_keys(report: &mut Report, settings: &Settings) {
    let keys_dir = settings.paths.keys_dir();
    let session_key = settings.paths.session_key();

    if keys_dir.exists() {
        inspect_mode(report, &keys_dir, 0o700, Severity::Fatal);
    }

    match std::fs::File::open(&session_key) {
        Ok(_) => inspect_mode(report, &session_key, 0o600, Severity::Fatal),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => report.push(
            Finding::fatal(format!(
                "the session signing key {} is missing",
                session_key.display()
            ))
            .remedy("run `stemma-server install` to generate it".to_owned()),
        ),
        Err(error) => report.push(Finding::fatal(format!(
            "cannot read the session signing key {}: {error}",
            session_key.display()
        ))),
    }
}

/// The database, which has to be at the version this binary knows.
fn inspect_database(report: &mut Report, settings: &Settings) {
    let path = settings.paths.database();
    let expected = db::expected_version();

    if !path.exists() {
        report.push(
            Finding::fatal(format!("there is no database at {}", path.display()))
                .remedy("run `stemma-server migrate` to create it".to_owned()),
        );
        return;
    }

    match db::version_of(&path) {
        Err(error) => report.push(Finding::fatal(format!("{error:#}"))),
        Ok(version) if version == expected => {}
        Ok(version) if version < expected => report.push(
            Finding::fatal(format!(
                "the database is at schema version {version}, and this build needs {expected}"
            ))
            .remedy("run `stemma-server migrate`".to_owned()),
        ),
        Ok(version) => report.push(
            Finding::fatal(format!(
                "the database is at schema version {version}, which is newer than the {expected}                  this build knows"
            ))
            .remedy("this machine is running an older release than the database expects".to_owned()),
        ),
    }
}

/// The repositories, against the groups the database knows about.
///
/// Git is the source of truth and the database is an index over it, so
/// the two drifting apart is worth saying out loud — and it is the kind
/// of thing only a check notices, because neither side fails on its own.
fn inspect_repositories(report: &mut Report, settings: &Settings) {
    let database = settings.paths.database();

    if !database.exists() {
        return;
    }

    let Ok(known) = db::group_slugs(&database) else {
        return;
    };

    let repositories = crate::git::Repositories::new(settings.paths.git_dir());
    let Ok(on_disk) = repositories.groups_on_disk() else {
        return;
    };

    for missing in known.iter().filter(|slug| !on_disk.contains(slug)) {
        report.push(
            Finding::warning(format!(
                "the group `{missing}` has no repository in {}",
                repositories.root().display()
            ))
            .remedy("nobody in it can clone anything until there is one".to_owned()),
        );
    }

    for stray in on_disk.iter().filter(|slug| !known.contains(slug)) {
        report.push(
            Finding::warning(format!(
                "{} holds repositories for `{stray}`, which is not a group here",
                repositories.root().display()
            ))
            .remedy("nothing was deleted; they are simply not reachable".to_owned()),
        );
    }
}

/// The address the server would bind.
fn inspect_listen(report: &mut Report, listen: &Listen) {
    match listen {
        Listen::Tcp(address) => match TcpListener::bind(address) {
            Ok(listener) => drop(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => report.push(
                Finding::fatal(format!("something is already listening on {address}"))
                    .remedy("stop it, or choose another address with `listen`".to_owned()),
            ),
            Err(error) => report.push(Finding::fatal(format!("cannot bind {address}: {error}"))),
        },
        Listen::Unix(path) => {
            if let Some(directory) = listen.socket_directory()
                && !directory.exists()
            {
                // A directory directly under /run is made by whatever
                // starts the process and is gone again when it stops,
                // so its absence says the instance is not running. That
                // is the ordinary state of an instance somebody is
                // about to start, and no reason to refuse to.
                let made_at_startup = directory.parent() == Some(Path::new("/run"));
                let said = format!(
                    "the socket directory {} does not exist, so {} cannot be bound",
                    directory.display(),
                    path.display()
                );

                report.push(
                    match made_at_startup {
                        true => Finding::warning(said),
                        false => Finding::fatal(said),
                    }
                    .remedy(
                        "systemd creates it from `RuntimeDirectory=` when the service \
                         starts"
                            .to_owned(),
                    ),
                );
            }
        }
    }
}

/// Outgoing mail, which is never a reason to refuse to start.
fn inspect_mail(report: &mut Report, mail: Option<&MailSeed>) {
    let Some(mail) = mail else {
        report.push(
            Finding::warning(
                "outgoing mail is not configured, so the instance runs in link-only mode",
            )
            .remedy(
                "invitations and password resets are shown as links to whoever creates them"
                    .to_owned(),
            ),
        );
        return;
    };

    match smtp_address(&mail.smtp_url) {
        None => report.push(Finding::warning(format!(
            "`{}` does not name an SMTP host and port",
            mail.smtp_url
        ))),
        Some(address) => {
            if TcpStream::connect_timeout(&address, SMTP_TIMEOUT).is_err() {
                report.push(
                    Finding::warning(format!("the SMTP host at {address} does not answer"))
                        .remedy("the instance starts anyway, and falls back to links".to_owned()),
                );
            }
        }
    }
}

/// Resolves an SMTP address out of its URL.
fn smtp_address(smtp_url: &str) -> Option<SocketAddr> {
    let parsed = url::Url::parse(smtp_url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port().unwrap_or(match parsed.scheme() {
        "smtps" => 465,
        _ => 25,
    });

    (host, port).to_socket_addrs().ok()?.next()
}

/// Reports permissions looser than the layout allows.
fn inspect_mode(report: &mut Report, path: &Path, allowed: u32, severity: Severity) {
    let Some(mode) = mode_of(path) else {
        return;
    };

    let extra = mode & !allowed;
    if extra == 0 {
        return;
    }

    let message = format!(
        "{} is mode {mode:04o}, which is looser than the {allowed:04o} it should be",
        path.display()
    );
    let finding = match severity {
        Severity::Fatal => Finding::fatal(message),
        Severity::Warning => Finding::warning(message),
    };

    report.push(finding.remedy(format!("chmod {allowed:04o} {}", path.display())));
}

/// Reports a state directory the service user does not own.
fn inspect_owner(report: &mut Report, path: &Path, service_user: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let (Ok(metadata), Some(expected)) =
            (std::fs::metadata(path), system::user_id(service_user))
        else {
            return;
        };

        if metadata.uid() != expected {
            report.push(
                Finding::fatal(format!(
                    "{} is owned by user {}, but the service runs as `{service_user}` (user {expected})",
                    path.display(),
                    metadata.uid()
                ))
                .remedy(format!("chown -R {service_user} {}", path.display())),
            );
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (report, path, service_user);
    }
}

/// Reports a state directory this process cannot write to.
fn inspect_writability(report: &mut Report, path: &Path) {
    let probe = path.join(".stemma-write-probe");

    let attempt = std::fs::File::create(&probe).and_then(|mut file| file.write_all(b"stemma"));
    let _ = std::fs::remove_file(&probe);

    if let Err(error) = attempt {
        report.push(Finding::fatal(format!(
            "cannot write inside the state directory {}: {error}",
            path.display()
        )));
    }
}

/// The permission bits of `path`, on platforms that have them.
fn mode_of(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        Some(std::fs::metadata(path).ok()?.permissions().mode() & 0o7777)
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_socket_directory_systemd_makes_is_not_a_reason_to_refuse_to_start() {
        let mut report = Report::default();
        inspect_listen(
            &mut report,
            &"unix:/run/stemma/server.sock".parse().unwrap(),
        );

        assert!(report.allows_startup(false), "{report}");
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.message.contains("/run/stemma"))
        );
    }

    #[test]
    fn a_socket_directory_nobody_will_make_is() {
        let mut report = Report::default();
        inspect_listen(
            &mut report,
            &"unix:/nowhere/at/all/server.sock".parse().unwrap(),
        );

        assert!(!report.allows_startup(false));
    }

    #[test]
    fn a_clean_report_lets_the_server_start() {
        let report = Report::default();
        assert!(report.allows_startup(false));
        assert!(report.allows_startup(true));
        assert_eq!(report.summary(), "Everything the server needs is in place.");
    }

    #[test]
    fn a_warning_stops_only_a_strict_run() {
        let report = Report::of(Finding::warning("mail is not configured"));
        assert!(report.allows_startup(false));
        assert!(!report.allows_startup(true));
    }

    #[test]
    fn a_fatal_finding_always_stops_the_server() {
        let report = Report::of(Finding::fatal("the state directory does not exist"));
        assert!(!report.allows_startup(false));
        assert!(!report.allows_startup(true));
    }

    #[test]
    fn the_summary_counts_both_classes() {
        let mut report = Report::of(Finding::fatal("one"));
        report.push(Finding::warning("two"));
        report.push(Finding::warning("three"));

        assert_eq!(
            report.summary(),
            "1 problem to fix before the server will start, and 2 warnings."
        );
    }

    #[test]
    fn a_rendered_report_shows_the_remedy_under_the_finding() {
        let report = Report::of(Finding::fatal("the key is missing").remedy("run install"));
        let rendered = report.to_string();

        assert!(rendered.contains("fatal    the key is missing"));
        assert!(rendered.contains("         run install"));
    }

    #[test]
    fn what_stops_the_server_is_reported_first() {
        let mut report = Report::of(Finding::warning("first, but only a warning"));
        report.push(Finding::fatal("second, and fatal"));
        report.push(Finding::warning("third"));

        let messages: Vec<&str> = report
            .ordered()
            .iter()
            .map(|finding| finding.message.as_str())
            .collect();

        assert_eq!(
            messages,
            ["second, and fatal", "first, but only a warning", "third",]
        );
    }

    #[test]
    fn an_smtp_url_without_a_port_takes_the_usual_one() {
        assert_eq!(smtp_address("smtp://127.0.0.1").unwrap().port(), 25);
        assert_eq!(smtp_address("smtps://127.0.0.1").unwrap().port(), 465);
        assert_eq!(smtp_address("smtp://127.0.0.1:2525").unwrap().port(), 2525);
        assert_eq!(smtp_address("not a url"), None);
    }
}

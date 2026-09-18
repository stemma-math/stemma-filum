//! Putting an instance on a machine, and taking it off again.
//!
//! Every step is separately satisfiable and separately reported, so running
//! the installer twice changes nothing and says so. Nothing here starts the
//! service: an instance that is installed but not yet configured should not
//! be answering requests.
//!
//! Removal never destroys data by accident. Taking the software off the
//! machine and erasing what a research group wrote are different acts, and
//! only the second one has to be asked for.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{self, Paths};
use crate::service::{self, UnitSettings};
use crate::system;

/// How many bytes of randomness the session signing key holds.
const SESSION_KEY_BYTES: usize = 32;

/// What an installation was asked to do.
#[derive(Debug, Clone)]
pub struct Options {
    /// The configuration file to create or leave alone.
    pub config_path: PathBuf,
    /// The file holding secrets.
    pub env_path: PathBuf,
    /// The state directory.
    pub data_dir: PathBuf,
    /// The account the service will run as.
    pub user: String,
    /// Whether to write and enable the systemd unit.
    pub service: bool,
    /// Report what would happen and change nothing.
    pub dry_run: bool,
}

/// What a removal was asked to do.
#[derive(Debug, Clone)]
pub struct RemovalOptions {
    /// The configuration file.
    pub config_path: PathBuf,
    /// The file holding secrets.
    pub env_path: PathBuf,
    /// The state directory.
    pub data_dir: PathBuf,
    /// The account the service ran as.
    pub user: String,
    /// Also erase configuration, state and the account.
    pub purge: bool,
    /// Report what would happen and change nothing.
    pub dry_run: bool,
}

/// What happened to one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The step did something.
    Done(String),
    /// The step found its work already done.
    Already(String),
    /// The step did not apply to this machine.
    Skipped(String),
    /// The step would have done something, on a real run.
    Planned(String),
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Done(what) => write!(f, "done     {what}"),
            Self::Already(what) => write!(f, "already  {what}"),
            Self::Skipped(what) => write!(f, "skipped  {what}"),
            Self::Planned(what) => write!(f, "would    {what}"),
        }
    }
}

/// Everything one run did, in order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Journal {
    /// The steps, in the order they were taken.
    pub steps: Vec<Outcome>,
}

impl Journal {
    /// Records a step.
    fn push(&mut self, outcome: Outcome) {
        self.steps.push(outcome);
    }

    /// Whether anything about the machine actually changed.
    #[cfg(test)]
    pub fn changed_anything(&self) -> bool {
        self.steps
            .iter()
            .any(|step| matches!(step, Outcome::Done(_)))
    }
}

impl fmt::Display for Journal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, step) in self.steps.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{step}")?;
        }

        Ok(())
    }
}

/// Puts an instance on this machine.
pub fn install(options: &Options) -> Result<Journal> {
    require_privilege(options.dry_run, &[&options.config_path, &options.data_dir])?;

    let paths = Paths::new(&options.data_dir);
    let mut journal = Journal::default();

    let account = ensure_account(options, &mut journal)?;
    ensure_directories(options, &paths, account, &mut journal)?;
    ensure_config(options, account, &mut journal)?;
    ensure_session_key(options, &paths, account, &mut journal)?;
    ensure_service(options, &mut journal)?;

    Ok(journal)
}

/// Takes the software off this machine, and its data too if asked.
pub fn uninstall(options: &RemovalOptions) -> Result<Journal> {
    require_privilege(options.dry_run, &[&options.config_path, &options.data_dir])?;

    let mut journal = Journal::default();
    remove_service(options, &mut journal)?;

    if !options.purge {
        return Ok(journal);
    }

    for path in [
        options.config_path.clone(),
        options.env_path.clone(),
        options.data_dir.clone(),
    ] {
        remove_path(&path, options.dry_run, &mut journal)?;
    }

    remove_account(options, &mut journal)?;

    Ok(journal)
}

/// Refuses to start work that will fail halfway through.
///
/// The standard layout belongs to root, so an ordinary installation needs
/// root. A container or a development machine puts everything somewhere the
/// running account already owns, and there root would be beside the point.
fn require_privilege(dry_run: bool, targets: &[&Path]) -> Result<()> {
    if dry_run || system::is_root() {
        return Ok(());
    }

    for target in targets {
        if !can_write_to(target) {
            bail!(
                "{} is not yours to write, so this command has to run as root",
                target.display()
            );
        }
    }

    Ok(())
}

/// Whether this process could create or replace `path`.
fn can_write_to(path: &Path) -> bool {
    let mut candidate = path;

    loop {
        if candidate.exists() {
            let probe = if candidate.is_dir() {
                candidate.join(".stemma-write-probe")
            } else {
                return fs::OpenOptions::new().append(true).open(candidate).is_ok();
            };

            let wrote = fs::File::create(&probe).is_ok();
            let _ = fs::remove_file(&probe);
            return wrote;
        }

        match candidate.parent() {
            Some(parent) => candidate = parent,
            None => return false,
        }
    }
}

/// The account the service runs as.
#[derive(Debug, Clone, Copy)]
struct Account {
    user: u32,
    group: u32,
}

/// Creates the service account if it is not there already.
fn ensure_account(options: &Options, journal: &mut Journal) -> Result<Option<Account>> {
    let name = &options.user;

    if let (Some(user), Some(group)) = (system::user_id(name), system::group_id(name)) {
        journal.push(Outcome::Already(format!("the `{name}` account exists")));
        return Ok(Some(Account { user, group }));
    }

    if options.dry_run {
        journal.push(Outcome::Planned(format!("create the `{name}` account")));
        return Ok(None);
    }

    if !system::is_root() {
        journal.push(Outcome::Skipped(format!(
            "the `{name}` account, because this installation is not a root one"
        )));
        return Ok(None);
    }

    if !cfg!(target_os = "linux") {
        bail!(
            "creating a system account is only supported on Linux; \
             create `{name}` by hand and run this again"
        );
    }

    system::create_system_user(name, &options.data_dir)?;
    journal.push(Outcome::Done(format!("created the `{name}` account")));

    let (Some(user), Some(group)) = (system::user_id(name), system::group_id(name)) else {
        bail!("the `{name}` account was created but cannot be looked up");
    };

    Ok(Some(Account { user, group }))
}

/// Creates the directories of the layout, with the permissions of the layout.
fn ensure_directories(
    options: &Options,
    paths: &Paths,
    account: Option<Account>,
    journal: &mut Journal,
) -> Result<()> {
    let config_dir = options
        .config_path
        .parent()
        .unwrap_or(Path::new("/"))
        .to_owned();

    // The configuration directory belongs to root, and the service account
    // only gets to read it. Everything under the state directory belongs to
    // the service account, which writes there constantly.
    let wanted: [(PathBuf, u32, Option<u32>); 5] = [
        (config_dir, 0o750, Some(0)),
        (paths.data_dir().to_owned(), 0o750, None),
        (paths.git_dir(), 0o750, None),
        (paths.artefacts_dir(), 0o750, None),
        (paths.keys_dir(), 0o700, None),
    ];

    for (path, mode, owner) in wanted {
        // Ownership is only ever set by an installation that has the
        // privilege to set it. Elsewhere the directories simply belong to
        // whoever ran the command, which is also who will run the server.
        let owner = system::is_root()
            .then(|| owner.or(account.map(|account| account.user)))
            .flatten();
        let group = system::is_root()
            .then(|| account.map(|account| account.group))
            .flatten();

        if path.is_dir() {
            journal.push(Outcome::Already(format!("{} is there", path.display())));
        } else if options.dry_run {
            journal.push(Outcome::Planned(format!(
                "create {} with mode {mode:04o}",
                path.display()
            )));
            continue;
        } else {
            fs::create_dir_all(&path)
                .with_context(|| format!("cannot create {}", path.display()))?;
            journal.push(Outcome::Done(format!(
                "created {} with mode {mode:04o}",
                path.display()
            )));
        }

        if !options.dry_run {
            set_mode(&path, mode)?;
            set_owner(&path, owner, group)?;
        }
    }

    Ok(())
}

/// Writes the commented configuration file, and never overwrites one.
fn ensure_config(options: &Options, account: Option<Account>, journal: &mut Journal) -> Result<()> {
    let path = &options.config_path;

    if path.exists() {
        journal.push(Outcome::Already(format!(
            "{} is there, and was left alone",
            path.display()
        )));
        return Ok(());
    }

    if options.dry_run {
        journal.push(Outcome::Planned(format!(
            "write a commented {}",
            path.display()
        )));
        return Ok(());
    }

    fs::write(path, config::skeleton(&options.data_dir))
        .with_context(|| format!("cannot write {}", path.display()))?;
    set_mode(path, 0o640)?;

    if system::is_root() {
        set_owner(path, Some(0), account.map(|account| account.group))?;
    }

    journal.push(Outcome::Done(format!(
        "wrote a commented {}",
        path.display()
    )));

    Ok(())
}

/// Generates the key that signs sessions, once.
fn ensure_session_key(
    options: &Options,
    paths: &Paths,
    account: Option<Account>,
    journal: &mut Journal,
) -> Result<()> {
    let path = paths.session_key();

    if path.exists() {
        journal.push(Outcome::Already(
            "the session signing key is there".to_owned(),
        ));
        return Ok(());
    }

    if options.dry_run {
        journal.push(Outcome::Planned(format!(
            "generate the session signing key at {}",
            path.display()
        )));
        return Ok(());
    }

    let mut bytes = [0_u8; SESSION_KEY_BYTES];
    getrandom::fill(&mut bytes).context("cannot read randomness to generate the session key")?;

    write_private(&path, &hex(&bytes))?;

    if system::is_root() {
        set_owner(
            &path,
            account.map(|account| account.user),
            account.map(|account| account.group),
        )?;
    }

    journal.push(Outcome::Done(format!(
        "generated the session signing key at {}",
        path.display()
    )));

    Ok(())
}

/// Writes and enables the unit, where there is a systemd to enable it with.
fn ensure_service(options: &Options, journal: &mut Journal) -> Result<()> {
    if !options.service {
        journal.push(Outcome::Skipped(
            "the systemd unit, because --no-service was given".to_owned(),
        ));
        return Ok(());
    }

    if !service::is_available() {
        journal.push(Outcome::Skipped(
            "the systemd unit, because this machine is not run by systemd".to_owned(),
        ));
        return Ok(());
    }

    let unit = UnitSettings {
        executable: std::env::current_exe().context("cannot tell where this binary lives")?,
        config_path: options.config_path.clone(),
        env_path: options.env_path.clone(),
        user: options.user.clone(),
        data_dir: options.data_dir.clone(),
    };
    let path = unit.unit_path();

    if options.dry_run {
        journal.push(Outcome::Planned(format!(
            "write {} and enable it",
            path.display()
        )));
        return Ok(());
    }

    fs::write(&path, unit.render()).with_context(|| format!("cannot write {}", path.display()))?;
    set_mode(&path, 0o644)?;
    service::reload()?;
    service::enable()?;

    journal.push(Outcome::Done(format!(
        "wrote {} and enabled it",
        path.display()
    )));

    Ok(())
}

/// Stops the service and removes its unit.
fn remove_service(options: &RemovalOptions, journal: &mut Journal) -> Result<()> {
    let path = Path::new(service::UNIT_DIRECTORY).join(service::UNIT_NAME);

    if !path.exists() {
        journal.push(Outcome::Already(format!(
            "{} is not on this machine",
            path.display()
        )));
        return Ok(());
    }

    if options.dry_run {
        journal.push(Outcome::Planned(format!(
            "stop the service and remove {}",
            path.display()
        )));
        return Ok(());
    }

    service::stop()?;
    service::disable()?;
    fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
    service::reload()?;

    journal.push(Outcome::Done(format!(
        "stopped the service and removed {}",
        path.display()
    )));

    Ok(())
}

/// Erases one path of the layout.
fn remove_path(path: &Path, dry_run: bool, journal: &mut Journal) -> Result<()> {
    if !path.exists() {
        journal.push(Outcome::Already(format!(
            "{} is not on this machine",
            path.display()
        )));
        return Ok(());
    }

    if dry_run {
        journal.push(Outcome::Planned(format!("erase {}", path.display())));
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .with_context(|| format!("cannot erase {}", path.display()))?;

    journal.push(Outcome::Done(format!("erased {}", path.display())));

    Ok(())
}

/// Removes the service account.
fn remove_account(options: &RemovalOptions, journal: &mut Journal) -> Result<()> {
    let name = &options.user;

    if system::user_id(name).is_none() {
        journal.push(Outcome::Already(format!("there is no `{name}` account")));
        return Ok(());
    }

    if options.dry_run {
        journal.push(Outcome::Planned(format!("remove the `{name}` account")));
        return Ok(());
    }

    system::delete_system_user(name)?;
    journal.push(Outcome::Done(format!("removed the `{name}` account")));

    Ok(())
}

/// How much there is under `path`, for a message that has to be honest about
/// what is about to be destroyed.
pub fn size_of(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => size_of(&entry.path()),
            Ok(_) => entry.metadata().map(|metadata| metadata.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// A size a person can read.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];

    let mut size = bytes as f64;
    let mut unit = 0;

    while size >= 1000.0 && unit < UNITS.len() - 1 {
        size /= 1000.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Writes a file only this account may read.
fn write_private(path: &Path, contents: &str) -> Result<()> {
    let mut file = private_file(path)?;

    file.write_all(contents.as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))
}

/// Opens a new file with the narrowest permissions the platform offers.
fn private_file(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    options
        .open(path)
        .with_context(|| format!("cannot create {}", path.display()))
}

/// Sets permissions, where the platform has them.
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("cannot set the permissions of {}", path.display()))?;
    }

    #[cfg(not(unix))]
    let _ = (path, mode);

    Ok(())
}

/// Sets ownership, where the platform has it.
fn set_owner(path: &Path, user: Option<u32>, group: Option<u32>) -> Result<()> {
    #[cfg(unix)]
    {
        if user.is_some() || group.is_some() {
            std::os::unix::fs::chown(path, user, group)
                .with_context(|| format!("cannot set the owner of {}", path.display()))?;
        }
    }

    #[cfg(not(unix))]
    let _ = (path, user, group);

    Ok(())
}

/// The bytes as hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dry_run_changes_nothing_and_says_what_it_would_do() {
        let root = std::env::temp_dir().join(format!("stemma-install-{}", std::process::id()));
        let options = Options {
            config_path: root.join("etc/server.toml"),
            env_path: root.join("etc/server.env"),
            data_dir: root.join("lib"),
            user: "stemma-no-such-account".to_owned(),
            service: true,
            dry_run: true,
        };

        let journal = install(&options).unwrap();

        assert!(!journal.changed_anything());
        assert!(!root.exists());
        assert!(journal.steps.iter().any(|step| matches!(
            step,
            Outcome::Planned(what) if what.contains("account")
        )));
    }

    #[test]
    fn an_installation_into_a_directory_of_ones_own_needs_no_privilege() {
        let root = std::env::temp_dir().join(format!("stemma-rootless-{}", std::process::id()));
        let options = Options {
            config_path: root.join("etc/server.toml"),
            env_path: root.join("etc/server.env"),
            data_dir: root.join("lib"),
            user: "stemma-no-such-account".to_owned(),
            service: false,
            dry_run: false,
        };

        let journal = install(&options).expect("a rootless installation should be allowed");

        assert!(journal.changed_anything());
        assert!(options.config_path.is_file());
        assert!(root.join("lib/git").is_dir());
        assert!(root.join("lib/keys/session.key").is_file());

        // Running it again finds everything in place and changes nothing.
        let second = install(&options).unwrap();
        assert!(!second.changed_anything());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sizes_are_reported_in_units_a_person_reads() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1_500), "1.5 kB");
        assert_eq!(human_size(2_400_000_000), "2.4 GB");
    }

    #[test]
    fn a_key_is_written_as_hexadecimal() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}

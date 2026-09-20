//! Taking a copy an instance can be put back from.
//!
//! Two of the things under the state directory cannot be copied by the
//! file system alone while the instance is running. The database is half
//! in a write-ahead log, so it is copied through SQLite; a repository
//! being pushed to receives its objects before the reference that names
//! them, so it is copied through git. What comes out is laid out exactly
//! like the directory it came from, which is what makes putting it back
//! an ordinary thing to do rather than a procedure.
//!
//! What is left out is what the instance can make again: the build
//! outputs, and the copies of the database a migration leaves behind.
//! Everything else the state directory holds is carried whether this
//! module knows what it is or not, because a copy that quietly omits
//! whatever was added since it was written is a copy nobody can trust.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config::{Paths, Settings, env_path_for};
use crate::db::{self, Database};
use crate::git;
use crate::install::human_size;

/// What every archive is called, before the moment it was taken.
const PREFIX: &str = "stemma-backup-";

/// And after it.
const SUFFIX: &str = ".tar.gz";

/// What a migration calls the copy it leaves behind.
const MIGRATION_COPY: &str = "server.db.backup-";

/// What a copy was asked to be.
#[derive(Debug, Clone)]
pub struct Options {
    /// The directory to write the archive into.
    pub to: PathBuf,
    /// How many archives to leave in that directory, if it is to be kept
    /// tidy at all.
    pub keep: Option<usize>,
    /// Whether to carry the build outputs too.
    pub artefacts: bool,
}

/// What one copy holds, written beside it.
///
/// An archive nobody can inspect without unpacking it is an archive
/// nobody checks. This says what is inside, what was deliberately left
/// out, and what has to be true of the machine it goes back onto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The name the archive carries, without its suffix.
    pub name: String,
    /// When it was taken, in seconds since the epoch.
    pub taken_at: String,
    /// The build that took it.
    pub version: &'static str,
    /// The protocol that build speaks.
    pub protocol: String,
    /// The schema the database was on, if there was a database.
    pub schema: Option<u32>,
    /// The state directory it came from.
    pub data_dir: PathBuf,
    /// The repositories it carries, named as they sit under `git`.
    pub repositories: Vec<String>,
    /// The configuration files it carries.
    pub configuration: Vec<String>,
    /// Anything else under the state directory that came along.
    pub also: Vec<String>,
    /// Whether the build outputs came too.
    pub artefacts: bool,
    /// What was left behind, and why, in words.
    pub left_out: Vec<String>,
}

impl Manifest {
    /// The file that sits at the top of the archive.
    pub fn render(&self) -> String {
        let data_dir = self.data_dir.display();
        let mut text = format!(
            "# A Stemma backup, taken by version {version} (protocol {protocol}).\n\
             #\n\
             # To put it back:\n\
             #\n\
             #   systemctl stop stemma-server\n\
             #   tar xzf {name}{SUFFIX}\n\
             #   cp -a {name}/state/. {data_dir}/\n\
             #   stemma-server migrate\n\
             #   systemctl start stemma-server\n\
             #\n\
             # The configuration beside the state is not put back by that, on\n\
             # purpose: it describes a machine, and the machine being restored\n\
             # onto may already have its own.\n\
             \n",
            version = self.version,
            protocol = self.protocol,
            name = self.name,
        );

        text.push_str(&format!("taken_at = {}\n", self.taken_at));
        text.push_str(&format!("version = {}\n", quoted(self.version)));
        text.push_str(&format!("protocol = {}\n", quoted(&self.protocol)));

        match self.schema {
            Some(schema) => text.push_str(&format!("schema = {schema}\n")),
            None => text.push_str("schema = false  # there was no database yet\n"),
        }

        text.push_str(&format!(
            "data_dir = {}\n",
            quoted(&self.data_dir.display().to_string())
        ));
        text.push_str(&format!("artefacts = {}\n", self.artefacts));
        text.push_str(&format!("repositories = {}\n", listed(&self.repositories)));
        text.push_str(&format!(
            "configuration = {}\n",
            listed(&self.configuration)
        ));
        text.push_str(&format!("also = {}\n", listed(&self.also)));
        text.push_str(&format!("left_out = {}\n", listed(&self.left_out)));

        text
    }

    /// What the archive holds, in one line.
    fn holds(&self) -> String {
        let mut parts = vec![match self.schema {
            Some(schema) => format!("the database (schema {schema})"),
            None => "no database, because there is none yet".to_owned(),
        }];

        parts.push(match self.repositories.len() {
            1 => "1 repository".to_owned(),
            other => format!("{other} repositories"),
        });

        if !self.configuration.is_empty() {
            parts.push("the configuration".to_owned());
        }

        if self.artefacts {
            parts.push("the build outputs".to_owned());
        }

        parts.join(", ")
    }
}

/// One copy, and what taking it did.
#[derive(Debug, Clone)]
pub struct Backup {
    /// Where the archive is.
    pub path: PathBuf,
    /// How big it is.
    pub size: u64,
    /// What is in it.
    pub manifest: Manifest,
    /// The older archives that were removed to make room.
    pub pruned: Vec<String>,
}

impl fmt::Display for Backup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut lines = vec![
            format!(
                "wrote    {} ({})",
                self.path.display(),
                human_size(self.size)
            ),
            format!("holds    {}", self.manifest.holds()),
        ];

        for reason in &self.manifest.left_out {
            lines.push(format!("left     {reason}"));
        }

        for name in &self.pruned {
            lines.push(format!("removed  {name}"));
        }

        write!(f, "{}", lines.join("\n"))
    }
}

/// Takes one copy.
pub fn take(settings: &Settings, config_path: &Path, options: &Options) -> Result<Backup> {
    let taken_at = db::stamp();
    let name = format!("{PREFIX}{taken_at}");
    let archive = options.to.join(format!("{name}{SUFFIX}"));

    std::fs::create_dir_all(&options.to)
        .with_context(|| format!("cannot create {}", options.to.display()))?;

    if archive.exists() {
        bail!(
            "{} is already there: a second copy within the same second would \
             overwrite the first",
            archive.display()
        );
    }

    let staging = Staging::at(options.to.join(&name))?;
    let manifest = assemble(
        settings,
        config_path,
        options,
        &staging.path,
        name,
        taken_at,
    )?;

    std::fs::write(staging.path.join("manifest.toml"), manifest.render())
        .context("cannot write the manifest")?;

    compress(&options.to, &manifest.name, &archive)?;
    keep_private(&archive)?;

    let pruned = match options.keep {
        Some(keep) => prune(&options.to, keep)?,
        None => Vec::new(),
    };

    let size = std::fs::metadata(&archive)
        .map(|about| about.len())
        .unwrap_or(0);

    Ok(Backup {
        path: archive,
        size,
        manifest,
        pruned,
    })
}

/// Puts everything worth keeping under one directory.
fn assemble(
    settings: &Settings,
    config_path: &Path,
    options: &Options,
    staging: &Path,
    name: String,
    taken_at: String,
) -> Result<Manifest> {
    let paths = &settings.paths;
    let state = staging.join("state");

    std::fs::create_dir_all(&state).context("cannot make room for the copy")?;

    let mut left_out = Vec::new();

    let schema = copy_database(paths, &state)?;
    let repositories = copy_repositories(paths, &state)?;

    if paths.keys_dir().is_dir() {
        copy_tree(&paths.keys_dir(), &state.join("keys"))?;
    }

    if paths.artefacts_dir().is_dir() {
        if options.artefacts {
            copy_tree(&paths.artefacts_dir(), &state.join("artefacts"))?;
        } else {
            left_out.push("the build outputs, which the instance makes again".to_owned());
        }
    }

    let also = copy_the_rest(paths, &state, &mut left_out)?;
    let configuration = copy_configuration(config_path, &staging.join("config"))?;

    Ok(Manifest {
        name,
        taken_at,
        version: env!("CARGO_PKG_VERSION"),
        protocol: stemma_core::PROTOCOL_VERSION.to_string(),
        schema,
        data_dir: paths.data_dir().to_owned(),
        repositories,
        configuration,
        also,
        artefacts: options.artefacts,
        left_out,
    })
}

/// Copies the database through SQLite, which is the only thing that can.
///
/// The connection is an ordinary one, and an ordinary one can write: the
/// copy is read through SQLite's own machinery, and that machinery needs
/// the shared index a write-ahead log is read with.
fn copy_database(paths: &Paths, state: &Path) -> Result<Option<u32>> {
    let path = paths.database();

    if !path.exists() {
        return Ok(None);
    }

    let database = Database::open(&path)?;
    database.back_up_to(&state.join("server.db"))?;

    Ok(database.version().ok())
}

/// Copies every repository, asking git for each one.
fn copy_repositories(paths: &Paths, state: &Path) -> Result<Vec<String>> {
    let repositories = git::Repositories::new(paths.git_dir());
    let mut carried = Vec::new();

    for path in repositories.on_disk() {
        let inside = path
            .strip_prefix(repositories.root())
            .unwrap_or(path.as_path());

        git::mirror(&path, &state.join("git").join(inside))?;
        carried.push(inside.display().to_string());
    }

    Ok(carried)
}

/// Copies whatever else the state directory holds.
///
/// The write-ahead log and its index are the one thing skipped without
/// being replaced: the copy of the database already holds what is in
/// them, and a stale log beside a fresh database is worse than neither.
fn copy_the_rest(paths: &Paths, state: &Path, left_out: &mut Vec<String>) -> Result<Vec<String>> {
    let known = [
        "server.db",
        "server.db-wal",
        "server.db-shm",
        "git",
        "keys",
        "artefacts",
    ];

    let Ok(entries) = std::fs::read_dir(paths.data_dir()) else {
        return Ok(Vec::new());
    };

    let mut carried = Vec::new();
    let mut migration_copies = 0;

    for entry in entries.filter_map(Result::ok) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };

        if known.contains(&name.as_str()) {
            continue;
        }

        if name.starts_with(MIGRATION_COPY) {
            migration_copies += 1;
            continue;
        }

        let from = entry.path();
        let to = state.join(&name);

        match from.is_dir() {
            true => copy_tree(&from, &to)?,
            false => copy_file(&from, &to)?,
        }

        carried.push(name);
    }

    if migration_copies > 0 {
        left_out.push(format!(
            "{migration_copies} copies a migration left behind, which are older \
             than this one"
        ));
    }

    carried.sort();

    Ok(carried)
}

/// Copies the configuration, which lives outside the state directory.
fn copy_configuration(config_path: &Path, into: &Path) -> Result<Vec<String>> {
    let mut carried = Vec::new();

    for path in [config_path.to_owned(), env_path_for(config_path)] {
        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        std::fs::create_dir_all(into)
            .with_context(|| format!("cannot create {}", into.display()))?;
        copy_file(&path, &into.join(name))?;
        carried.push(name.to_owned());
    }

    Ok(carried)
}

/// Copies a directory and everything under it.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("cannot create {}", to.display()))?;

    let entries =
        std::fs::read_dir(from).with_context(|| format!("cannot read {}", from.display()))?;

    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {}", from.display()))?;
        let source = entry.path();
        let destination = to.join(entry.file_name());

        match source.is_dir() {
            true => copy_tree(&source, &destination)?,
            false => copy_file(&source, &destination)?,
        }
    }

    Ok(())
}

/// Copies one file, keeping who may read it.
fn copy_file(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to)
        .with_context(|| format!("cannot copy {} to {}", from.display(), to.display()))?;

    Ok(())
}

/// Turns the assembled directory into one file.
fn compress(inside: &Path, name: &str, archive: &Path) -> Result<()> {
    let status = Command::new("tar")
        .arg("-czf")
        .arg(archive)
        .arg("-C")
        .arg(inside)
        .arg(name)
        .status()
        .context("cannot run tar")?;

    if !status.success() {
        bail!("tar could not write {}", archive.display());
    }

    Ok(())
}

/// Leaves only the newest `keep` archives in a directory.
///
/// A name carries the second it was taken in, so the order they sort in
/// is the order they happened in. A directory asked to keep none still
/// keeps the one just taken: the flag says how many to hold on to, and
/// none is not an answer to that.
fn prune(inside: &Path, keep: usize) -> Result<Vec<String>> {
    let keep = keep.max(1);

    let Ok(entries) = std::fs::read_dir(inside) else {
        return Ok(Vec::new());
    };

    let mut archives: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(PREFIX) && name.ends_with(SUFFIX))
        .collect();

    archives.sort();

    let how_many = archives.len().saturating_sub(keep);
    let mut removed = Vec::new();

    for name in archives.into_iter().take(how_many) {
        std::fs::remove_file(inside.join(&name))
            .with_context(|| format!("cannot remove {name}"))?;
        removed.push(name);
    }

    Ok(removed)
}

/// Shuts everybody else out of the archive.
///
/// It carries the key that signs every session, so it is as much a
/// secret as the state directory it came from.
#[cfg(unix)]
fn keep_private(archive: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(archive, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("cannot set the permissions of {}", archive.display()))
}

/// Shuts everybody else out of the archive.
#[cfg(not(unix))]
fn keep_private(_archive: &Path) -> Result<()> {
    Ok(())
}

/// A directory that is assembled, archived, and then gone.
///
/// It goes whether the copy finished or not: half an archive lying
/// beside the finished ones is a thing somebody restores from one day.
struct Staging {
    /// Where it is.
    path: PathBuf,
}

impl Staging {
    /// Makes one.
    fn at(path: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&path)
            .with_context(|| format!("cannot create {}", path.display()))?;

        Ok(Self { path })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// One string, as the manifest spells it.
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A list of them.
fn listed(values: &[String]) -> String {
    let inside: Vec<String> = values.iter().map(|value| quoted(value)).collect();

    format!("[{}]", inside.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_says_how_to_put_the_copy_back() {
        let manifest = Manifest {
            name: "stemma-backup-1000".to_owned(),
            taken_at: "1000".to_owned(),
            version: "0.0.0",
            protocol: "1".to_owned(),
            schema: Some(10),
            data_dir: PathBuf::from("/var/lib/stemma"),
            repositories: vec!["cosme/shared.git".to_owned()],
            configuration: vec!["server.toml".to_owned()],
            also: Vec::new(),
            artefacts: false,
            left_out: vec!["the build outputs".to_owned()],
        };

        let text = manifest.render();

        assert!(text.contains("tar xzf stemma-backup-1000.tar.gz"));
        assert!(text.contains("cp -a stemma-backup-1000/state/. /var/lib/stemma/"));
        assert!(text.contains("schema = 10"));
        assert!(text.contains("repositories = [\"cosme/shared.git\"]"));
        assert!(text.contains("left_out = [\"the build outputs\"]"));
    }

    #[test]
    fn a_manifest_admits_an_instance_that_has_never_run() {
        let manifest = Manifest {
            name: "stemma-backup-1000".to_owned(),
            taken_at: "1000".to_owned(),
            version: "0.0.0",
            protocol: "1".to_owned(),
            schema: None,
            data_dir: PathBuf::from("/var/lib/stemma"),
            repositories: Vec::new(),
            configuration: Vec::new(),
            also: Vec::new(),
            artefacts: false,
            left_out: Vec::new(),
        };

        assert!(manifest.render().contains("schema = false"));
        assert!(manifest.holds().contains("no database"));
    }

    #[test]
    fn a_quoted_path_survives_a_quotation_mark() {
        assert_eq!(quoted("a \"name\""), "\"a \\\"name\\\"\"");
        assert_eq!(
            listed(&["one".to_owned(), "two".to_owned()]),
            "[\"one\", \"two\"]"
        );
    }
}

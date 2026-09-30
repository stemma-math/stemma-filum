//! What the engine keeps of every line, apart from the files people see.
//!
//! One record per line, under the client's state, named by the line's
//! identifier so a line that moves is still the same record. A record
//! holds two versions of the line: the revision it was last in step with,
//! which is what a merge starts from, and what this machine has, which is
//! what is shown and what is sent.
//!
//! Records are changed under a lock held across processes, because the
//! application, the daemon and a command at a terminal may all be
//! running, and the lock is never held while the instance is being asked
//! something: a slow network must not keep the editor from saving.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use stemma_core::repository::Side;

/// One line, as this machine keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The line's identifier.
    pub id: String,
    /// The group's short name.
    pub group: String,
    /// Which of the group's repositories it is in.
    pub side: Side,
    /// What it is called.
    pub name: String,
    /// Who started it.
    pub author: String,
    /// Who may write it: `author` or `anyone`.
    pub writing: String,
    /// The revision this machine was last in step with.
    pub base: u64,
    /// The line at that revision.
    pub base_source: String,
    /// Its Lean module at that revision.
    pub base_lean: Option<String>,
    /// The line as this machine has it.
    pub source: String,
    /// Its Lean module as this machine has it.
    pub lean_source: Option<String>,
    /// When it was last saved here, in milliseconds since the epoch.
    #[serde(default)]
    pub saved_at: Option<u64>,
    /// A revision the instance has that changed some block this machine
    /// changed too, differently: the line needs a decision before what
    /// is here can be sent.
    #[serde(default)]
    pub there: Option<There>,
    /// Why what is here could not be sent, the last time it was tried.
    #[serde(default)]
    pub trouble: Option<String>,
    /// When this machine last sent a revision nothing has been built from
    /// yet, in milliseconds since the epoch.
    #[serde(default)]
    pub unbuilt: Option<u64>,
}

/// A revision on the instance that this machine's changes part from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct There {
    /// Which revision.
    pub revision: u64,
    /// The line at it.
    pub source: String,
    /// Its Lean module at it.
    pub lean_source: Option<String>,
}

impl Record {
    /// Whether this machine has something the instance has not.
    pub fn pending(&self) -> bool {
        self.source != self.base_source || self.lean_source != self.base_lean
    }

    /// The group, repository and name, as people read them.
    pub fn label(&self) -> String {
        format!("{}/{}/{}", self.group, self.side, self.name)
    }

    /// Whether it is the line with this name in this repository.
    pub fn is(&self, group: &str, side: Side, name: &str) -> bool {
        self.group == group && self.side == side && self.name == name
    }
}

/// Where the records are.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

/// The lock on the records, for as long as it is held.
pub struct Locked {
    _file: File,
}

impl Store {
    /// The records under a state directory.
    pub fn at(state: &Path) -> Self {
        Self {
            root: state.join("mirror"),
        }
    }

    /// Where versions changed by hand are kept.
    pub fn aside(&self) -> PathBuf {
        self.root.join("aside")
    }

    fn lines(&self) -> PathBuf {
        self.root.join("lines")
    }

    fn file(&self, id: &str) -> PathBuf {
        self.lines()
            .join(format!("{}.json", crate::storage::file_name(id)))
    }

    /// Waits for, and takes, the lock on the records.
    pub fn lock(&self) -> Result<Locked> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("cannot create {}", self.root.display()))?;
        let path = self.root.join("lock");
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("cannot open {}", path.display()))?;
        file.lock()
            .with_context(|| format!("cannot lock {}", path.display()))?;

        Ok(Locked { _file: file })
    }

    /// Keeps the machine for the application, for as long as what this
    /// returns is held: the daemon's engine leaves the work to it.
    pub fn hold_window(&self) -> Result<Locked> {
        let file = self.window_file()?;
        file.try_lock()
            .map_err(|_| anyhow::anyhow!("another window already keeps this machine in step"))?;

        Ok(Locked { _file: file })
    }

    /// Whether the application is keeping the machine in step.
    pub fn window_open(&self) -> bool {
        let Ok(file) = self.window_file() else {
            return false;
        };

        match file.try_lock() {
            Ok(()) => {
                let _ = file.unlock();
                false
            }
            Err(_) => true,
        }
    }

    fn window_file(&self) -> Result<File> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("cannot create {}", self.root.display()))?;
        let path = self.root.join("window");

        File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("cannot open {}", path.display()))
    }

    /// Every record.
    pub fn all(&self) -> Result<Vec<Record>> {
        let entries = match std::fs::read_dir(self.lines()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).context("cannot read what this machine keeps"),
        };

        let mut found = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                found.push(read(&path)?);
            }
        }

        found.sort_by_key(Record::label);
        Ok(found)
    }

    /// One record, by the line's identifier.
    pub fn get(&self, id: &str) -> Result<Option<Record>> {
        let path = self.file(id);

        match std::fs::read_to_string(&path) {
            Ok(text) => parse(&path, &text).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    /// The record of the line with this name in this repository.
    pub fn named(&self, group: &str, side: Side, name: &str) -> Result<Option<Record>> {
        Ok(self
            .all()?
            .into_iter()
            .find(|record| record.is(group, side, name)))
    }

    /// Keeps a record.
    pub fn put(&self, record: &Record) -> Result<()> {
        let text = serde_json::to_string_pretty(record).context("cannot write the record")?;
        crate::storage::replace(&self.file(&record.id), &text)
    }

    /// The groups the instance last said this person belongs to; none
    /// before it has said.
    pub fn groups(&self) -> Result<Vec<String>> {
        let path = self.root.join("groups.json");

        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("{} is not a list of groups", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    /// Remembers the groups the instance lists.
    pub fn put_groups(&self, groups: &[String]) -> Result<()> {
        let text = serde_json::to_string_pretty(groups).context("cannot write the groups")?;
        crate::storage::replace(&self.root.join("groups.json"), &text)
    }

    /// Forgets a record.
    pub fn remove(&self, id: &str) -> Result<()> {
        match std::fs::remove_file(self.file(id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("cannot forget the line"),
        }
    }
}

fn read(path: &Path) -> Result<Record> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;

    parse(path, &text)
}

fn parse(path: &Path, text: &str) -> Result<Record> {
    serde_json::from_str(text).with_context(|| format!("{} is not a record", path.display()))
}

/// Now, in milliseconds since the epoch.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

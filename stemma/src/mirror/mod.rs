//! Keeping this machine's lines in step with the instance, a revision at
//! a time.
//!
//! The instance owns every line's history. What is here is a mirror of
//! the lines this person can see: for each, the revision it was last in
//! step with and what this machine has made of it since. The engine keeps
//! that in its own store and writes the files people see from it, so the
//! files are a view and never what is sent: a file changed by hand is
//! put back from the store, with the changed version kept aside.
//!
//! Writing leaves on its own. The editor saves through the engine, and a
//! line with something new is sent a couple of seconds after the last
//! save, typeset here with the group's pinned Typst, against the revision
//! it was written from. Offline, it waits, and leaves when the instance
//! can be reached again.
//!
//! When the instance has moved on meanwhile, the line is merged block by
//! block: a block changed on one side only is taken from that side, and
//! only a block changed on both, differently, waits for a person to say
//! which to keep. Nothing is ever dropped: what a machine had of a line
//! deleted or hidden meanwhile is kept aside, and said.

pub mod engine;
mod remote;
mod store;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use stemma_core::document::Document;
use stemma_core::id::{BlockId, Slug};
use stemma_core::line::Name;
use stemma_core::reconcile::{self, Conflict, Keep};
use stemma_core::repository::{self, Side};

use crate::api::{About, AcceptedProposal, Listing, Outgoing, Revision, Written};
use crate::config::Config;
use crate::home::Home;

pub use remote::{Instance, Remote};
pub use store::Store;
use store::now;
pub use store::{Record, There};

/// How long after the last save a line is sent.
pub const PAUSE: Duration = Duration::from_secs(2);

/// How a line is typeset to the page every revision carries.
pub type Typeset = fn(&Path, &Document) -> Result<String>;

/// This machine's lines, as the engine keeps them and shows them.
pub struct Mirror {
    store: Store,
    config: Config,
    workspace: PathBuf,
    typeset: Typeset,
}

/// Which lines to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Which {
    /// Every line with something new.
    All,
    /// Those last saved at least this long ago.
    Paused(Duration),
    /// This one, by identifier.
    Line(String),
}

impl Which {
    fn admits(&self, record: &Record, now: u64) -> bool {
        match self {
            Self::All => true,
            Self::Paused(pause) => record
                .saved_at
                .is_none_or(|saved| now.saturating_sub(saved) >= pause.as_millis() as u64),
            Self::Line(id) => &record.id == id,
        }
    }
}

/// What a round with the instance did.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Lines that arrived here, or changed here because the instance had
    /// something newer.
    pub arrived: Vec<String>,
    /// Lines that were sent.
    pub sent: Vec<String>,
    /// Lines both sides changed, merged block by block on their own.
    pub merged: Vec<String>,
    /// Lines that need a decision before they can be sent.
    pub deciding: Vec<String>,
    /// Lines that are no longer here.
    pub gone: Vec<String>,
    /// Versions kept aside rather than lost.
    pub aside: Vec<Aside>,
    /// Lines that could not be sent, and why.
    pub waiting: Vec<Waiting>,
    /// What stopped the round before it was done, when something did:
    /// most often, that the instance could not be reached.
    pub stopped: Option<String>,
    /// The revision each line sent became, by identifier.
    #[serde(skip)]
    pub revisions: BTreeMap<String, u64>,
}

impl Report {
    /// Whether any line's files changed here.
    pub fn touched_files(&self) -> bool {
        !(self.arrived.is_empty() && self.merged.is_empty() && self.gone.is_empty())
    }
}

/// A version of a line kept aside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Aside {
    /// Which line it is a version of.
    pub line: String,
    /// Where it is now.
    pub path: PathBuf,
    /// Why it was put there.
    pub why: String,
}

/// A line that could not be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Waiting {
    /// Which line.
    pub line: String,
    /// Why, in words.
    pub why: String,
}

/// Where one line stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Standing {
    /// Its identifier.
    pub id: String,
    /// The group's short name.
    pub group: String,
    /// Which of the group's repositories.
    pub side: Side,
    /// What it is called.
    pub name: String,
    /// Where it stands.
    pub state: State,
    /// Why it was not sent, the last time that was tried.
    pub trouble: Option<String>,
}

/// Where a line stands, as people are told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The instance has everything this machine has.
    Sent,
    /// This machine has something the instance has not yet.
    NotSent,
    /// The instance changed a block this machine changed too, and
    /// somebody has to say which to keep.
    NeedsDecision,
}

impl State {
    /// In words.
    pub fn said(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::NotSent => "not sent yet",
            Self::NeedsDecision => "needs a decision",
        }
    }
}

/// What applying an accepted proposal came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// It is in this revision of the line.
    Written(u64),
    /// A block it changes changed since it was made.
    Outdated,
    /// It is merged here and waits to be sent, for this reason.
    Queued(String),
}

/// What happened when a line was brought up to a newer revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Brought {
    /// It was already there.
    Nothing,
    /// Nothing was here, so it took the revision as it is.
    Took,
    /// Both sides changed it, and the blocks merged on their own.
    Merged,
    /// Both changed a block, differently.
    Parted,
}

impl Mirror {
    /// This machine's mirror.
    pub fn at(home: &Home) -> Result<Self> {
        let config = Config::read(&home.config_file())?;
        let workspace = config.workspace_or(home.workspace());

        Ok(Self {
            store: Store::at(home.state_dir()),
            config,
            workspace,
            typeset: crate::typeset::render::page,
        })
    }

    /// Typesets with something other than Typst, as tests do.
    pub fn typesetting_with(mut self, typeset: Typeset) -> Self {
        self.typeset = typeset;
        self
    }

    /// Where one of a group's repositories is shown.
    pub fn repository(&self, group: &str, side: Side) -> PathBuf {
        self.config
            .group_directory(group, &self.workspace)
            .join(side.as_str())
    }

    /// Where a line's `line.json` is shown.
    pub fn entry(&self, record: &Record) -> PathBuf {
        self.line_directory(record).join(repository::ENTRY)
    }

    fn line_directory(&self, record: &Record) -> PathBuf {
        self.repository(&record.group, record.side)
            .join(repository::LINES)
            .join(&record.name)
    }

    fn lean_entry(&self, record: &Record) -> PathBuf {
        self.line_directory(record).join(repository::LEAN_ENTRY)
    }

    // ---- On this machine alone ----

    /// Every line here, and where each stands.
    pub fn standing(&self) -> Result<Vec<Standing>> {
        Ok(self.store.all()?.iter().map(standing).collect())
    }

    /// Every line here, as the engine keeps it.
    pub fn records(&self) -> Result<Vec<Record>> {
        self.store.all()
    }

    /// Every repository this machine has a line in.
    pub fn repositories(&self) -> Result<Vec<(String, Side)>> {
        let mut found: Vec<(String, Side)> = self
            .store
            .all()?
            .into_iter()
            .map(|record| (record.group, record.side))
            .collect();
        found.sort_by(|one, other| (&one.0, one.1.as_str()).cmp(&(&other.0, other.1.as_str())));
        found.dedup();

        Ok(found)
    }

    /// The repositories whose writing here is over and was not built: a
    /// line of theirs was sent and then let go, or nothing was sent for
    /// `quiet`. `released` names the lines just let go, by identifier.
    pub fn due_builds(&self, released: &[String], quiet: Duration) -> Result<Vec<(String, Side)>> {
        let now = now();
        let mut due: Vec<(String, Side)> = Vec::new();

        for record in self.store.all()? {
            let Some(sent) = record.unbuilt else {
                continue;
            };
            let over = released.contains(&record.id)
                || now.saturating_sub(sent) >= quiet.as_millis() as u64;
            let place = (record.group.clone(), record.side);

            if over && !due.contains(&place) {
                due.push(place);
            }
        }

        Ok(due)
    }

    /// Notes that what was built covers every line it names at the
    /// revision this machine is at.
    pub fn built(&self, nodes: &stemma_core::artefact::Nodes) -> Result<()> {
        let _locked = self.store.lock()?;
        let group = nodes.group.slug().as_str();

        for mut record in self.store.all()? {
            let covered = record.group == group
                && record.side == nodes.side
                && nodes.lines.iter().any(|line| {
                    line.name.as_str() == record.name && line.revision == Some(record.base)
                });

            if covered && record.unbuilt.is_some() {
                record.unbuilt = None;
                self.store.put(&record)?;
            }
        }

        Ok(())
    }

    /// Marks every line affected by a library change for an immediate rebuild.
    /// A shared package is a dependency of both sides; a private package affects
    /// only its owner's private lines.
    pub fn rebuild(&self, group: &str, side: Side) -> Result<()> {
        let _locked = self.store.lock()?;
        for mut record in self.store.all()? {
            let affected = record.group == group && (record.side == side || side == Side::Shared);
            if affected {
                record.unbuilt = Some(0);
                self.store.put(&record)?;
            }
        }
        Ok(())
    }

    /// The record of one line.
    pub fn find(&self, group: &str, side: Side, name: &str) -> Result<Option<Record>> {
        self.store.named(group, side, name)
    }

    /// Saves what the editor has of a line. It is sent after a pause.
    pub fn save(&self, group: &str, side: Side, name: &str, document: &Document) -> Result<()> {
        let _locked = self.store.lock()?;
        let mut record = self
            .store
            .named(group, side, name)?
            .context("this line is not on this machine yet")?;
        let before = record.clone();

        record.source = document.to_text();
        record.saved_at = Some(now());

        let mut report = Report::default();
        self.show(Some(&before), &record, &mut report)?;
        self.store.put(&record)
    }

    /// The blocks a line's two versions disagree on, when it needs a
    /// decision.
    pub fn conflicts(&self, group: &str, side: Side, name: &str) -> Result<Vec<Conflict>> {
        let Some(record) = self.store.named(group, side, name)? else {
            return Ok(Vec::new());
        };
        let Some(there) = &record.there else {
            return Ok(Vec::new());
        };

        reconcile::between(&record.base_source, &record.source, &there.source)
            .map_err(|trouble| anyhow!("{trouble}"))
    }

    /// Settles a line that needs a decision, keeping for each block the
    /// version chosen. It is sent as soon as it can be.
    pub fn decide(
        &self,
        group: &str,
        side: Side,
        name: &str,
        decisions: &BTreeMap<BlockId, Keep>,
    ) -> Result<()> {
        let _locked = self.store.lock()?;
        let mut record = self
            .store
            .named(group, side, name)?
            .context("this line is not on this machine")?;
        let there = record
            .there
            .clone()
            .context("this line does not need a decision")?;
        let before = record.clone();

        record.source = reconcile::merge(
            &record.base_source,
            &record.source,
            &there.source,
            decisions,
        )
        .map_err(|trouble| anyhow!("{trouble}"))?;
        record.lean_source = match record.lean_source == record.base_lean {
            true => there.lean_source.clone(),
            false => record.lean_source,
        };
        record.base = there.revision;
        record.base_source = there.source;
        record.base_lean = there.lean_source;
        record.there = None;
        record.saved_at = None;

        let mut report = Report::default();
        self.show(Some(&before), &record, &mut report)?;
        self.store.put(&record)
    }

    /// Puts back every file that was changed by hand, keeping what it
    /// said aside, and writes every file that is missing.
    pub fn tidy(&self, report: &mut Report) -> Result<()> {
        let _locked = self.store.lock()?;

        for record in self.store.all()? {
            self.show(Some(&record), &record, report)?;
        }

        Ok(())
    }

    // ---- With the instance ----

    /// A whole round: the files here put right, what changed on the
    /// instance taken in, and what is here sent. `full` also brings the
    /// files at the root of every repository, which change seldom.
    ///
    /// It never fails: what stopped it is in the report, and whatever it
    /// had done before that is done.
    pub fn step(&self, remote: &dyn Remote, full: bool, sending: &Which) -> Report {
        let mut report = Report::default();

        if let Err(error) = self.step_into(remote, full, sending, &mut report) {
            report.stopped = Some(format!("{error:#}"));
        }

        report
    }

    fn step_into(
        &self,
        remote: &dyn Remote,
        full: bool,
        sending: &Which,
        report: &mut Report,
    ) -> Result<()> {
        self.tidy(report)?;

        if full {
            self.roots(remote, &remote.groups()?)?;
        }

        self.take_in(remote, report)?;
        self.send_into(remote, sending, report)
    }

    /// Sends the lines asked for, and nothing else.
    pub fn send(&self, remote: &dyn Remote, which: &Which) -> Report {
        let mut report = Report::default();

        if let Err(error) = self.send_into(remote, which, &mut report) {
            report.stopped = Some(format!("{error:#}"));
        }

        report
    }

    /// Merges an accepted proposal into the line and sends it.
    ///
    /// The proposal is merged block by block into what this machine has,
    /// from the revision it was made against: a block only the proposal
    /// changed is taken, and one this line changed since as well makes
    /// the proposal outdated.
    pub fn apply(&self, remote: &dyn Remote, accepted: &AcceptedProposal) -> Result<Applied> {
        let id = accepted
            .line_id
            .clone()
            .context("the proposal does not say which line it is for")?;

        let mut report = Report::default();
        self.take_in(remote, &mut report)?;
        let base = remote.revision(&id, Some(accepted.revision))?;

        {
            let _locked = self.store.lock()?;
            let mut record = self
                .store
                .get(&id)?
                .context("the line is not on this machine")?;

            let conflicts = reconcile::between(&base.source, &record.source, &accepted.source)
                .map_err(|trouble| anyhow!("{trouble}"))?;
            let lean_clash = record.lean_source != base.lean_source
                && accepted.lean_source != base.lean_source
                && record.lean_source != accepted.lean_source;

            if !conflicts.is_empty() || lean_clash {
                return Ok(Applied::Outdated);
            }

            let before = record.clone();
            record.source = reconcile::merge(
                &base.source,
                &record.source,
                &accepted.source,
                &BTreeMap::new(),
            )
            .map_err(|trouble| anyhow!("{trouble}"))?;
            if accepted.lean_source != base.lean_source {
                record.lean_source = accepted.lean_source.clone();
            }
            record.saved_at = None;

            self.show(Some(&before), &record, &mut report)?;
            self.store.put(&record)?;
        }

        self.send_into(remote, &Which::Line(id.clone()), &mut report)?;

        let record = self
            .store
            .get(&id)?
            .context("the line is not on this machine")?;

        Ok(match (report.revisions.get(&id), record.pending()) {
            (Some(revision), _) => Applied::Written(*revision),
            (None, false) => Applied::Written(record.base),
            (None, true) => Applied::Queued(
                report
                    .waiting
                    .first()
                    .map(|waiting| waiting.why.clone())
                    .or(record.trouble)
                    .unwrap_or_else(|| "it has not been sent yet".to_owned()),
            ),
        })
    }

    /// Writes the files at the root of every repository of these groups.
    fn roots(&self, remote: &dyn Remote, groups: &[String]) -> Result<()> {
        for group in groups {
            if Slug::parse(group).is_err() {
                continue;
            }

            for side in [Side::Shared, Side::Private] {
                // A repository with nothing in it yet has nothing at its
                // root either; that is not the instance being away.
                let Ok(files) = remote.root(group, side.as_str()) else {
                    continue;
                };
                let root = self.repository(group, side);

                for (path, content) in files {
                    let Some(inside) = root_path(&path) else {
                        continue;
                    };
                    let at = root.join(inside);

                    if std::fs::read_to_string(&at).ok().as_deref() != Some(content.as_str()) {
                        crate::storage::replace(&at, &content)?;
                    }
                    let mut permissions = std::fs::metadata(&at)?.permissions();
                    permissions.set_readonly(true);
                    std::fs::set_permissions(&at, permissions)?;
                }
            }
        }

        Ok(())
    }

    /// Takes in whatever changed on the instance.
    fn take_in(&self, remote: &dyn Remote, report: &mut Report) -> Result<()> {
        let listed = remote.lines()?;
        let known = self.store.all()?;

        for listing in &listed {
            let Some(side) = placed(listing) else {
                continue;
            };
            let record = known.iter().find(|record| record.id == listing.id);

            if listing.deleted {
                if record.is_some() {
                    self.forget(&listing.id, "it was deleted", report)?;
                }
                continue;
            }

            let Some(record) = record else {
                let revision = remote.revision(&listing.id, None)?;
                self.arrive(listing, side, &revision, report)?;
                continue;
            };

            let known_there = record
                .there
                .as_ref()
                .is_some_and(|there| there.revision >= listing.revision);
            let behind = listing.revision > record.base && !known_there;
            let moved = !record.is(&listing.group, side, &listing.line);
            let described = record.author != listing.author || record.writing != listing.writing;

            if behind || moved || described {
                let revision = match behind {
                    true => Some(remote.revision(&listing.id, None)?),
                    false => None,
                };
                self.update(listing, side, revision.as_ref(), report)?;
            }
        }

        for record in &known {
            if !listed.iter().any(|listing| listing.id == record.id) {
                self.forget(&record.id, "it is no longer shown to you", report)?;
            }
        }

        Ok(())
    }

    /// A line this machine has not had before.
    fn arrive(
        &self,
        listing: &Listing,
        side: Side,
        revision: &Revision,
        report: &mut Report,
    ) -> Result<()> {
        let _locked = self.store.lock()?;

        if self.store.get(&listing.id)?.is_some() {
            return Ok(());
        }

        let record = Record {
            id: listing.id.clone(),
            group: listing.group.clone(),
            side,
            name: listing.line.clone(),
            author: listing.author.clone(),
            writing: listing.writing.clone(),
            base: revision.revision,
            base_source: revision.source.clone(),
            base_lean: revision.lean_source.clone(),
            source: revision.source.clone(),
            lean_source: revision.lean_source.clone(),
            saved_at: None,
            there: None,
            trouble: None,
            unbuilt: None,
        };

        self.show(None, &record, report)?;
        self.store.put(&record)?;
        report.arrived.push(record.label());

        Ok(())
    }

    /// A line the instance has moved, renamed, redescribed or written.
    fn update(
        &self,
        listing: &Listing,
        side: Side,
        revision: Option<&Revision>,
        report: &mut Report,
    ) -> Result<()> {
        let _locked = self.store.lock()?;
        let Some(mut record) = self.store.get(&listing.id)? else {
            return Ok(());
        };
        let before = record.clone();

        record.group = listing.group.clone();
        record.side = side;
        record.name = listing.line.clone();
        record.author = listing.author.clone();
        record.writing = listing.writing.clone();

        let brought = match revision {
            Some(revision) => bring_up(&mut record, revision),
            None => Brought::Nothing,
        };

        self.show(Some(&before), &record, report)?;
        self.store.put(&record)?;

        let label = record.label();
        match brought {
            Brought::Took => report.arrived.push(label),
            Brought::Merged => report.merged.push(label),
            Brought::Parted => report.deciding.push(label),
            Brought::Nothing if !before.is(&record.group, record.side, &record.name) => {
                report.arrived.push(label);
            }
            Brought::Nothing => {}
        }

        Ok(())
    }

    /// Takes a line off this machine, keeping aside whatever of it the
    /// instance does not have.
    fn forget(&self, id: &str, why: &str, report: &mut Report) -> Result<()> {
        let _locked = self.store.lock()?;
        let Some(record) = self.store.get(id)? else {
            return Ok(());
        };

        self.keep_touched(&record, report)?;

        if record.pending() {
            let aside = self.put_aside(
                &record,
                &[
                    (repository::ENTRY, Some(record.source.as_str())),
                    (
                        repository::LEAN_ENTRY,
                        record
                            .lean_source
                            .as_deref()
                            .filter(|_| record.lean_source != record.base_lean),
                    ),
                ],
            )?;
            report.aside.push(Aside {
                line: record.label(),
                path: aside,
                why: format!("it had changes that were not sent, and {why}"),
            });
        }

        remove_file(&self.entry(&record))?;
        remove_file(&self.lean_entry(&record))?;
        let _ = std::fs::remove_dir(self.line_directory(&record));
        self.store.remove(id)?;
        report.gone.push(record.label());

        Ok(())
    }

    /// Sends every line asked for that has something new and needs no
    /// decision. Not reaching the instance stops it.
    fn send_into(&self, remote: &dyn Remote, which: &Which, report: &mut Report) -> Result<()> {
        let now = now();

        for record in self.store.all()? {
            if record.pending() && record.there.is_none() && which.admits(&record, now) {
                self.send_one(remote, &record, true, report)?;
            }
        }

        Ok(())
    }

    fn send_one(
        &self,
        remote: &dyn Remote,
        record: &Record,
        again: bool,
        report: &mut Report,
    ) -> Result<()> {
        let outgoing = match self.prepared(record) {
            Ok(outgoing) => outgoing,
            Err(error) => return self.wait(record, format!("{error:#}"), report),
        };
        let about = About {
            group: record.group.clone(),
            side: record.side.as_str().to_owned(),
            line: record.name.clone(),
        };

        // A line nobody holds is taken for as long as it takes to send
        // it: this is what lets writing done offline leave by itself.
        let mut taken = false;
        let answer = loop {
            match remote.write(&record.id, &outgoing)? {
                Written::NotHeld(None) if !taken && remote.take(&about)? => taken = true,
                answer => break answer,
            }
        };
        if taken {
            let _ = remote.release(&about);
        }

        match answer {
            Written::Taken(revision) => {
                let _locked = self.store.lock()?;
                if let Some(mut current) = self.store.get(&record.id)?
                    && current.base == record.base
                {
                    current.base = revision;
                    current.base_source = outgoing.source;
                    current.base_lean = outgoing.lean_source;
                    current.trouble = None;
                    current.unbuilt = Some(now());
                    self.store.put(&current)?;
                }
                report.sent.push(record.label());
                report.revisions.insert(record.id.clone(), revision);
            }
            Written::Stale(current) => {
                let listing = Listing {
                    id: record.id.clone(),
                    group: current.group.clone(),
                    side: current.side.clone(),
                    line: current.line.clone(),
                    author: record.author.clone(),
                    writing: record.writing.clone(),
                    revision: current.revision,
                    deleted: false,
                };
                let Some(side) = placed(&listing) else {
                    return self.wait(
                        record,
                        "the server named this line in a way it cannot be kept".to_owned(),
                        report,
                    );
                };
                self.update(&listing, side, Some(&current), report)?;

                if again
                    && let Some(merged) = self.store.get(&record.id)?
                    && merged.pending()
                    && merged.there.is_none()
                {
                    self.send_one(remote, &merged, false, report)?;
                }
            }
            Written::NotHeld(Some(holder)) => {
                self.wait(record, format!("{holder} is writing this line"), report)?;
            }
            Written::NotHeld(None) => {
                self.wait(
                    record,
                    "the line could not be taken to send it".to_owned(),
                    report,
                )?;
            }
            Written::Gone => self.forget(&record.id, "it was deleted", report)?,
            Written::Refused(why) => self.wait(record, why, report)?,
        }

        Ok(())
    }

    /// What a line is sent as: the line, and the page it typesets to.
    fn prepared(&self, record: &Record) -> Result<Outgoing> {
        let root = self.repository(&record.group, record.side);
        let pins = crate::repository::read_manifest(&root)
            .map(|manifest| crate::repository::group_pins(&root, &manifest))
            .unwrap_or_default();
        crate::typeset::render::may_typeset(&pins)?;

        let document = Document::parse(&record.source).context("what is here is not a line")?;
        let page = (self.typeset)(&root, &document)?;

        Ok(Outgoing {
            base: record.base,
            source: record.source.clone(),
            lean_source: record.lean_source.clone(),
            page,
        })
    }

    /// Notes why a line was not sent.
    fn wait(&self, record: &Record, why: String, report: &mut Report) -> Result<()> {
        let _locked = self.store.lock()?;
        if let Some(mut current) = self.store.get(&record.id)? {
            current.trouble = Some(why.clone());
            self.store.put(&current)?;
        }
        report.waiting.push(Waiting {
            line: record.label(),
            why,
        });

        Ok(())
    }

    // ---- The files people see ----

    /// Writes a line's files as `after` has them, where `before` is what
    /// the store had: a file that says something else was changed by
    /// hand, and is kept aside before it is put back.
    fn show(&self, before: Option<&Record>, after: &Record, report: &mut Report) -> Result<()> {
        match before {
            Some(before) => {
                self.keep_touched(before, report)?;

                if self.line_directory(before) != self.line_directory(after) {
                    remove_file(&self.entry(before))?;
                    remove_file(&self.lean_entry(before))?;
                    let _ = std::fs::remove_dir(self.line_directory(before));
                }
            }
            None => self.keep_touched(after, report)?,
        }

        write_if_changed(&self.entry(after), Some(&after.source))?;
        write_if_changed(&self.lean_entry(after), after.lean_source.as_deref())
    }

    /// Keeps aside whatever of a line's files says something other than
    /// the record.
    fn keep_touched(&self, record: &Record, report: &mut Report) -> Result<()> {
        let entry = touched(&self.entry(record), Some(&record.source));
        let lean = touched(&self.lean_entry(record), record.lean_source.as_deref());

        if entry.is_none() && lean.is_none() {
            return Ok(());
        }

        let path = self.put_aside(
            record,
            &[
                (repository::ENTRY, entry.as_deref()),
                (repository::LEAN_ENTRY, lean.as_deref()),
            ],
        )?;
        report.aside.push(Aside {
            line: record.label(),
            path,
            why: "it was changed outside Stemma".to_owned(),
        });

        Ok(())
    }

    /// Keeps some files of a line aside, and says where.
    fn put_aside(&self, record: &Record, files: &[(&str, Option<&str>)]) -> Result<PathBuf> {
        let directory = self.store.aside().join(format!(
            "{}-{}-{}-{}",
            now(),
            record.group,
            record.side,
            record.name
        ));

        for (name, content) in files {
            if let Some(content) = content {
                crate::storage::replace(&directory.join(name), content)?;
            }
        }

        Ok(directory)
    }
}

/// Brings a line up to a newer revision, merging what is here into it.
fn bring_up(record: &mut Record, current: &Revision) -> Brought {
    if current.revision <= record.base {
        return Brought::Nothing;
    }

    let there = There {
        revision: current.revision,
        source: current.source.clone(),
        lean_source: current.lean_source.clone(),
    };

    if !record.pending() {
        record.source = there.source.clone();
        record.lean_source = there.lean_source.clone();
        take_base(record, there);
        return Brought::Took;
    }

    let lean = if record.lean_source == record.base_lean {
        Some(there.lean_source.clone())
    } else if there.lean_source == record.base_lean || there.lean_source == record.lean_source {
        Some(record.lean_source.clone())
    } else {
        None
    };
    let merged = reconcile::between(&record.base_source, &record.source, &there.source)
        .ok()
        .filter(Vec::is_empty)
        .and_then(|_| {
            reconcile::merge(
                &record.base_source,
                &record.source,
                &there.source,
                &BTreeMap::new(),
            )
            .ok()
        });

    match (merged, lean) {
        (Some(source), Some(lean)) => {
            record.source = source;
            record.lean_source = lean;
            take_base(record, there);
            match record.pending() {
                true => Brought::Merged,
                false => Brought::Took,
            }
        }
        _ => {
            record.there = Some(there);
            Brought::Parted
        }
    }
}

/// Makes a revision the one a line is in step with.
fn take_base(record: &mut Record, there: There) {
    record.base = there.revision;
    record.base_source = there.source;
    record.base_lean = there.lean_source;
    record.there = None;
    record.trouble = None;
}

fn standing(record: &Record) -> Standing {
    Standing {
        id: record.id.clone(),
        group: record.group.clone(),
        side: record.side,
        name: record.name.clone(),
        state: if record.there.is_some() {
            State::NeedsDecision
        } else if record.pending() {
            State::NotSent
        } else {
            State::Sent
        },
        trouble: record.trouble.clone(),
    }
}

/// The repository a listed line is in, when its names are ones a path
/// can be made of.
fn placed(listing: &Listing) -> Option<Side> {
    Slug::parse(&listing.group).ok()?;
    Name::parse(&listing.line).ok()?;
    listing.side.parse().ok()
}

/// Where a file the instance keeps at a repository's root goes, when it
/// stays inside the repository and outside its lines.
fn root_path(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    let mut parts = path.components();
    let first = parts.next()?;

    let plain = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    let reserved = first.as_os_str() == repository::LINES || first.as_os_str() == ".git";

    (plain && !reserved).then(|| path.to_owned())
}

/// What a file says when it is not what it should: `None` when it says
/// what it should, or is absent and should be.
fn touched(path: &Path, expected: Option<&str>) -> Option<String> {
    let found = std::fs::read_to_string(path).ok();

    match (found, expected) {
        (None, _) => None,
        (Some(found), Some(expected)) if found == expected => None,
        (Some(found), _) => Some(found),
    }
}

fn write_if_changed(path: &Path, content: Option<&str>) -> Result<()> {
    match content {
        Some(content) => {
            if std::fs::read_to_string(path).ok().as_deref() != Some(content) {
                crate::storage::replace(path, content)?;
            }
            Ok(())
        }
        None => remove_file(path),
    }
}

fn remove_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("cannot remove {}", path.display())),
    }
}

// ---- The command line ----

/// Takes in what changed and sends what is here, now, and says so.
pub fn run(home: &Home, out: &mut impl Write) -> Result<Report> {
    let mirror = Mirror::at(home)?;
    let instance = Instance::connect(home)?;
    let report = mirror.step(&instance, true, &Which::All);

    tell(&report, out)?;

    Ok(report)
}

/// Says where every line stands, without asking the instance.
pub fn status(home: &Home, out: &mut impl Write) -> Result<()> {
    let standing = Mirror::at(home)?.standing()?;

    if standing.is_empty() {
        writeln!(out, "No lines on this machine yet. Run `stemma sync`.")?;
        return Ok(());
    }

    let waiting: Vec<&Standing> = standing
        .iter()
        .filter(|line| line.state != State::Sent)
        .collect();

    if waiting.is_empty() {
        writeln!(out, "Every line here has been sent.")?;
        return Ok(());
    }

    for line in waiting {
        writeln!(
            out,
            "{}/{}/{}  {}",
            line.group,
            line.side,
            line.name,
            line.state.said()
        )?;
        if let Some(trouble) = &line.trouble {
            writeln!(out, "  {trouble}")?;
        }
    }

    Ok(())
}

/// Says what a round did, a line at a time.
pub fn tell(report: &Report, out: &mut impl Write) -> Result<()> {
    for line in &report.arrived {
        writeln!(out, "{line}  updated here")?;
    }
    for line in &report.merged {
        writeln!(out, "{line}  merged with changes made elsewhere")?;
    }
    for line in &report.sent {
        writeln!(out, "{line}  sent")?;
    }
    for line in &report.gone {
        writeln!(out, "{line}  removed from this machine")?;
    }
    for line in &report.deciding {
        writeln!(
            out,
            "{line}  needs a decision: `stemma conflict list` shows which blocks"
        )?;
    }
    for waiting in &report.waiting {
        writeln!(out, "{}  not sent yet: {}", waiting.line, waiting.why)?;
    }
    for aside in &report.aside {
        writeln!(
            out,
            "{}  a version was kept at {}, because {}",
            aside.line,
            aside.path.display(),
            aside.why
        )?;
    }

    match &report.stopped {
        Some(why) => writeln!(out, "Stopped before it was done: {why}")?,
        None if report == &Report::default() => writeln!(out, "Everything is up to date.")?,
        None => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests;

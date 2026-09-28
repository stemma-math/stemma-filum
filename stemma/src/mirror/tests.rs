//! The engine against an instance kept in memory.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use stemma_core::document::{Block, Document};
use stemma_core::reconcile::Keep;
use stemma_core::repository::{self, Side};

use super::*;
use crate::api::{About, Listing, Outgoing, Revision, Written};
use crate::home::{Environment, HOME_VARIABLE, Home};

/// One line, as the pretend instance keeps it.
struct Kept {
    id: String,
    side: Side,
    name: String,
    revisions: Vec<(String, Option<String>)>,
    deleted: bool,
    /// Which machine holds it: `here`, some other label, or nobody.
    holder: Option<String>,
}

/// An instance that lives in memory.
#[derive(Default)]
struct Pretend {
    lines: RefCell<Vec<Kept>>,
    away: Cell<bool>,
    pages: RefCell<Vec<String>>,
    released: Cell<usize>,
}

impl Pretend {
    fn start(&self, id: &str, side: Side, name: &str, document: &Document) {
        self.lines.borrow_mut().push(Kept {
            id: id.to_owned(),
            side,
            name: name.to_owned(),
            revisions: vec![(document.to_text(), None)],
            deleted: false,
            holder: Some("here".to_owned()),
        });
    }

    /// Another machine writes the line.
    fn written_elsewhere(&self, id: &str, document: &Document) {
        let mut lines = self.lines.borrow_mut();
        let line = lines.iter_mut().find(|line| line.id == id).unwrap();
        line.revisions.push((document.to_text(), None));
    }

    fn with<T>(&self, id: &str, change: impl FnOnce(&mut Kept) -> T) -> T {
        let mut lines = self.lines.borrow_mut();
        change(lines.iter_mut().find(|line| line.id == id).unwrap())
    }

    fn current(&self, id: &str) -> (u64, String) {
        self.with(id, |line| {
            (
                line.revisions.len() as u64,
                line.revisions.last().unwrap().0.clone(),
            )
        })
    }

    fn reachable(&self) -> Result<()> {
        match self.away.get() {
            true => bail!("cannot reach the instance"),
            false => Ok(()),
        }
    }

    fn revision_of(line: &Kept, number: u64) -> Revision {
        let (source, lean_source) = line.revisions[number as usize - 1].clone();

        Revision {
            id: line.id.clone(),
            group: "algebra".to_owned(),
            side: line.side.as_str().to_owned(),
            line: line.name.clone(),
            revision: number,
            kind: "written".to_owned(),
            source,
            lean_source,
        }
    }
}

impl Remote for Pretend {
    fn groups(&self) -> Result<Vec<String>> {
        self.reachable()?;
        Ok(vec!["algebra".to_owned()])
    }

    fn lines(&self) -> Result<Vec<Listing>> {
        self.reachable()?;
        Ok(self
            .lines
            .borrow()
            .iter()
            .map(|line| Listing {
                id: line.id.clone(),
                group: "algebra".to_owned(),
                side: line.side.as_str().to_owned(),
                line: line.name.clone(),
                author: "usr:ana".to_owned(),
                writing: "anyone".to_owned(),
                revision: line.revisions.len() as u64,
                deleted: line.deleted,
            })
            .collect())
    }

    fn revision(&self, id: &str, number: Option<u64>) -> Result<Revision> {
        self.reachable()?;
        let lines = self.lines.borrow();
        let line = lines.iter().find(|line| line.id == id).unwrap();

        Ok(Self::revision_of(
            line,
            number.unwrap_or(line.revisions.len() as u64),
        ))
    }

    fn write(&self, id: &str, outgoing: &Outgoing) -> Result<Written> {
        self.reachable()?;
        let mut lines = self.lines.borrow_mut();
        let line = lines.iter_mut().find(|line| line.id == id).unwrap();

        if line.deleted {
            return Ok(Written::Gone);
        }
        match line.holder.as_deref() {
            Some("here") => {}
            Some(other) => return Ok(Written::NotHeld(Some(other.to_owned()))),
            None => return Ok(Written::NotHeld(None)),
        }
        let at = line.revisions.len() as u64;
        if outgoing.base != at {
            return Ok(Written::Stale(Box::new(Self::revision_of(line, at))));
        }

        line.revisions
            .push((outgoing.source.clone(), outgoing.lean_source.clone()));
        self.pages.borrow_mut().push(outgoing.page.clone());

        Ok(Written::Taken(at + 1))
    }

    fn root(&self, _group: &str, side: &str) -> Result<BTreeMap<String, String>> {
        self.reachable()?;
        Ok(BTreeMap::from([
            (
                repository::MANIFEST.to_owned(),
                format!("# the {side} manifest\n"),
            ),
            ("../outside".to_owned(), "no".to_owned()),
            ("lines/sneaky/line.json".to_owned(), "no".to_owned()),
        ]))
    }

    fn take(&self, about: &About) -> Result<bool> {
        self.reachable()?;
        let mut lines = self.lines.borrow_mut();
        let line = lines
            .iter_mut()
            .find(|line| line.name == about.line)
            .unwrap();

        if line.holder.is_some() {
            return Ok(false);
        }
        line.holder = Some("here".to_owned());
        Ok(true)
    }

    fn release(&self, about: &About) -> Result<()> {
        self.reachable()?;
        let mut lines = self.lines.borrow_mut();
        let line = lines
            .iter_mut()
            .find(|line| line.name == about.line)
            .unwrap();
        line.holder = None;
        self.released.set(self.released.get() + 1);
        Ok(())
    }
}

fn typeset(_root: &Path, document: &Document) -> Result<String> {
    Ok(format!("<h1>{}</h1>", document.title))
}

struct World {
    root: PathBuf,
    mirror: Mirror,
    instance: Pretend,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn world(name: &str) -> World {
    let root = std::env::temp_dir().join(format!(
        "stemma-mirror-{name}-{}-{}",
        std::process::id(),
        now()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let home = Home::locate(&Environment::from_pairs([(HOME_VARIABLE, &root)]));

    World {
        mirror: Mirror::at(&home).unwrap().typesetting_with(typeset),
        instance: Pretend::default(),
        root,
    }
}

impl World {
    fn step(&self) -> Report {
        self.mirror.step(&self.instance, true, &Which::All)
    }

    fn file(&self, side: Side, name: &str) -> PathBuf {
        self.root
            .join("workspace/algebra")
            .join(side.as_str())
            .join("lines")
            .join(name)
            .join(repository::ENTRY)
    }

    fn shown(&self, side: Side, name: &str) -> Option<String> {
        std::fs::read_to_string(self.file(side, name)).ok()
    }

    fn save(&self, document: &Document) {
        self.mirror
            .save("algebra", Side::Shared, "quotients", document)
            .unwrap();
    }

    fn state(&self) -> State {
        self.mirror.standing().unwrap()[0].state
    }
}

/// A line of three paragraphs.
fn three() -> Document {
    Document::new("Quotients", None)
        .with(Block::prose("One."))
        .with(Block::prose("Two."))
        .with(Block::prose("Three."))
}

/// The same line, with one paragraph saying something else.
fn saying(document: &Document, at: usize, content: &str) -> Document {
    let mut changed = document.clone();
    if let Block::Prose(prose) = &mut changed.blocks[at] {
        prose.content = content.to_owned();
    }
    changed
}

fn started(name: &str) -> (World, Document) {
    let world = world(name);
    let document = three();
    world
        .instance
        .start("lin:one", Side::Shared, "quotients", &document);
    world.step();
    (world, document)
}

#[test]
fn a_line_arrives_as_its_file_beside_the_repository_root() {
    let (world, document) = started("arrives");

    assert_eq!(
        world.shown(Side::Shared, "quotients"),
        Some(document.to_text())
    );
    assert_eq!(world.state(), State::Sent);

    let root = world.root.join("workspace/algebra/shared");
    assert_eq!(
        std::fs::read_to_string(root.join(repository::MANIFEST)).unwrap(),
        "# the shared manifest\n"
    );
    assert!(!world.root.join("workspace/algebra/outside").exists());
    assert!(
        !root.join("lines/sneaky").exists(),
        "lines are not root files"
    );
}

#[test]
fn what_is_saved_waits_for_a_pause_and_then_leaves_with_its_page() {
    let (world, document) = started("saved");
    let changed = saying(&document, 1, "Two, better.");

    world.save(&changed);

    assert_eq!(
        world.shown(Side::Shared, "quotients"),
        Some(changed.to_text())
    );
    assert_eq!(world.state(), State::NotSent);

    let report = world.mirror.step(
        &world.instance,
        false,
        &Which::Paused(Duration::from_secs(60)),
    );
    assert!(report.sent.is_empty(), "still inside the pause");

    let report = world
        .mirror
        .send(&world.instance, &Which::Paused(Duration::ZERO));
    assert_eq!(report.sent, ["algebra/shared/quotients"]);
    assert_eq!(report.revisions["lin:one"], 2);
    assert_eq!(world.instance.current("lin:one"), (2, changed.to_text()));
    assert_eq!(*world.instance.pages.borrow(), ["<h1>Quotients</h1>"]);
    assert_eq!(world.state(), State::Sent);
}

#[test]
fn a_line_written_elsewhere_is_taken_as_it_is_when_nothing_here_changed() {
    let (world, document) = started("taken");
    let elsewhere = saying(&document, 0, "One, from the desktop.");
    world.instance.written_elsewhere("lin:one", &elsewhere);

    let report = world.step();

    assert_eq!(report.arrived, ["algebra/shared/quotients"]);
    assert_eq!(
        world.shown(Side::Shared, "quotients"),
        Some(elsewhere.to_text())
    );
}

#[test]
fn blocks_changed_on_one_side_each_merge_on_their_own_and_are_sent() {
    let (world, document) = started("merged");
    world
        .instance
        .written_elsewhere("lin:one", &saying(&document, 0, "One, from the desktop."));
    world.save(&saying(&document, 2, "Three, from the laptop."));

    let report = world.step();

    assert_eq!(report.merged, ["algebra/shared/quotients"]);
    assert_eq!(report.sent, ["algebra/shared/quotients"]);
    let both = saying(
        &saying(&document, 0, "One, from the desktop."),
        2,
        "Three, from the laptop.",
    );
    assert_eq!(world.instance.current("lin:one"), (3, both.to_text()));
    assert_eq!(world.shown(Side::Shared, "quotients"), Some(both.to_text()));
}

#[test]
fn a_refused_write_is_merged_and_sent_again() {
    let (world, document) = started("stale");
    world.save(&saying(&document, 2, "Three, from the laptop."));
    // The instance moved after this machine last looked.
    world
        .instance
        .written_elsewhere("lin:one", &saying(&document, 0, "One, from the desktop."));

    let report = world.mirror.send(&world.instance, &Which::All);

    assert_eq!(report.merged, ["algebra/shared/quotients"]);
    assert_eq!(report.sent, ["algebra/shared/quotients"]);
    assert_eq!(world.instance.current("lin:one").0, 3);
    assert_eq!(world.state(), State::Sent);
}

#[test]
fn a_block_changed_on_both_sides_waits_for_a_decision_and_then_leaves() {
    let (world, document) = started("parted");
    world
        .instance
        .written_elsewhere("lin:one", &saying(&document, 1, "Two, from the desktop."));
    world.save(&saying(&document, 1, "Two, from the laptop."));

    let report = world.step();

    assert_eq!(report.deciding, ["algebra/shared/quotients"]);
    assert!(report.sent.is_empty());
    assert_eq!(world.state(), State::NeedsDecision);

    let conflicts = world
        .mirror
        .conflicts("algebra", Side::Shared, "quotients")
        .unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].block, *document.blocks[1].id());

    world
        .mirror
        .decide(
            "algebra",
            Side::Shared,
            "quotients",
            &BTreeMap::from([(conflicts[0].block, Keep::Here)]),
        )
        .unwrap();
    let report = world.step();

    assert_eq!(report.sent, ["algebra/shared/quotients"]);
    assert_eq!(
        world.instance.current("lin:one"),
        (3, saying(&document, 1, "Two, from the laptop.").to_text())
    );
}

#[test]
fn a_file_changed_by_hand_is_put_back_and_its_version_kept_aside() {
    let (world, document) = started("by-hand");
    std::fs::write(world.file(Side::Shared, "quotients"), "edited by hand").unwrap();

    let report = world.step();

    assert_eq!(
        world.shown(Side::Shared, "quotients"),
        Some(document.to_text())
    );
    assert_eq!(report.aside.len(), 1);
    assert_eq!(
        std::fs::read_to_string(report.aside[0].path.join(repository::ENTRY)).unwrap(),
        "edited by hand"
    );
    assert!(
        report.sent.is_empty(),
        "what was edited by hand is not sent"
    );
}

#[test]
fn a_line_deleted_elsewhere_goes_with_what_was_not_sent_kept_aside() {
    let (world, document) = started("deleted");
    world.save(&saying(&document, 0, "Unsent."));
    world.instance.with("lin:one", |line| line.deleted = true);

    let report = world.step();

    assert_eq!(report.gone, ["algebra/shared/quotients"]);
    assert_eq!(world.shown(Side::Shared, "quotients"), None);
    assert!(world.mirror.standing().unwrap().is_empty());
    assert_eq!(report.aside.len(), 1);
    assert!(
        std::fs::read_to_string(report.aside[0].path.join(repository::ENTRY))
            .unwrap()
            .contains("Unsent.")
    );
}

#[test]
fn offline_the_line_waits_and_leaves_when_the_instance_is_back() {
    let (world, document) = started("offline");
    let changed = saying(&document, 0, "Written on a train.");
    world.instance.away.set(true);
    world.save(&changed);

    let report = world.step();

    assert!(report.stopped.is_some());
    assert_eq!(world.state(), State::NotSent);

    world.instance.away.set(false);
    let report = world.step();

    assert_eq!(report.stopped, None);
    assert_eq!(report.sent, ["algebra/shared/quotients"]);
    assert_eq!(world.instance.current("lin:one"), (2, changed.to_text()));
}

#[test]
fn a_line_nobody_holds_is_taken_to_send_it_and_let_go_again() {
    let (world, document) = started("unheld");
    world.instance.with("lin:one", |line| line.holder = None);
    world.save(&saying(&document, 0, "Written after the claim lapsed."));

    let report = world.step();

    assert_eq!(report.sent, ["algebra/shared/quotients"]);
    assert_eq!(world.instance.released.get(), 1);
    assert_eq!(
        world.instance.with("lin:one", |line| line.holder.clone()),
        None
    );
}

#[test]
fn a_line_another_machine_holds_waits_and_says_so() {
    let (world, document) = started("held");
    world
        .instance
        .with("lin:one", |line| line.holder = Some("desktop".to_owned()));
    world.save(&saying(&document, 0, "Not yet."));

    let report = world.step();

    assert_eq!(report.waiting[0].why, "desktop is writing this line");
    let standing = &world.mirror.standing().unwrap()[0];
    assert_eq!(standing.state, State::NotSent);
    assert_eq!(
        standing.trouble.as_deref(),
        Some("desktop is writing this line")
    );
}

#[test]
fn a_line_moved_to_the_group_moves_here_too() {
    let world = world("moved");
    let document = three();
    world
        .instance
        .start("lin:one", Side::Private, "quotients", &document);
    world.step();
    assert!(world.shown(Side::Private, "quotients").is_some());

    world.instance.with("lin:one", |line| {
        line.side = Side::Shared;
        line.revisions.push(line.revisions[0].clone());
    });
    world.step();

    assert_eq!(world.shown(Side::Private, "quotients"), None);
    assert_eq!(
        world.shown(Side::Shared, "quotients"),
        Some(document.to_text())
    );
}

fn accepted(document: &Document, revision: u64) -> AcceptedProposal {
    AcceptedProposal {
        source: document.to_text(),
        lean_source: None,
        group: "algebra".to_owned(),
        side: "shared".to_owned(),
        line: "quotients".to_owned(),
        line_id: Some("lin:one".to_owned()),
        revision,
    }
}

#[test]
fn an_accepted_proposal_is_merged_into_what_is_here_and_written() {
    let (world, document) = started("proposal");
    world.save(&saying(&document, 2, "Three, still being written."));

    let applied = world
        .mirror
        .apply(
            &world.instance,
            &accepted(&saying(&document, 0, "One, as proposed."), 1),
        )
        .unwrap();

    assert_eq!(applied, Applied::Written(2));
    assert_eq!(
        world.instance.current("lin:one").1,
        saying(
            &saying(&document, 0, "One, as proposed."),
            2,
            "Three, still being written."
        )
        .to_text()
    );
}

#[test]
fn a_proposal_touching_a_block_changed_since_is_outdated() {
    let (world, document) = started("outdated");
    world
        .instance
        .written_elsewhere("lin:one", &saying(&document, 0, "One, rewritten."));

    let applied = world
        .mirror
        .apply(
            &world.instance,
            &accepted(&saying(&document, 0, "One, as proposed."), 1),
        )
        .unwrap();

    assert_eq!(applied, Applied::Outdated);
    assert_eq!(
        world.instance.current("lin:one").0,
        2,
        "nothing was written"
    );
}

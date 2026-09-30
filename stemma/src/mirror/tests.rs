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
    /// Writes answered so far.
    writes: Cell<usize>,
    /// The write, counting from one, that finds the instance gone.
    lost_at_write: Cell<Option<usize>>,
    /// Groups listed besides `algebra`, and whether `algebra` is not.
    other_groups: RefCell<Vec<String>>,
    left_algebra: Cell<bool>,
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
        let mut groups = self.other_groups.borrow().clone();
        if !self.left_algebra.get() {
            groups.insert(0, "algebra".to_owned());
        }
        Ok(groups)
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
        self.writes.set(self.writes.get() + 1);
        if self.lost_at_write.get() == Some(self.writes.get()) {
            bail!("the connection dropped");
        }
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
            (repository::MANIFEST.to_owned(), manifest(side)),
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

fn manifest(side: &str) -> String {
    repository::Manifest::new(
        "grp:algebra".parse().unwrap(),
        side.parse().unwrap(),
        "Algebra",
    )
    .to_text()
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
        manifest("shared")
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
        library_change: None,
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

#[test]
fn a_build_names_the_revision_of_every_line_the_group_has() {
    let (world, _) = started("build");
    world
        .instance
        .start("lin:two", Side::Shared, "groups", &three());
    world.step();
    world
        .mirror
        .save(
            "algebra",
            Side::Shared,
            "groups",
            &saying(&three(), 0, "Unsent."),
        )
        .unwrap();

    let built = crate::build::read(&world.mirror, "algebra", Side::Shared).unwrap();
    let revision = |name: &str| {
        built
            .nodes
            .lines
            .iter()
            .find(|line| line.name.as_str() == name)
            .unwrap()
            .revision
    };

    assert_eq!(revision("quotients"), Some(1));
    assert_eq!(
        revision("groups"),
        None,
        "what is not sent is nobody's revision"
    );
    assert!(stemma_core::artefact::is_commit(&built.nodes.commit));
    assert_eq!(
        crate::build::read(&world.mirror, "algebra", Side::Shared)
            .unwrap()
            .nodes
            .commit,
        built.nodes.commit,
        "the same build is filed under the same name"
    );
}

#[test]
fn writing_is_built_once_it_is_let_go_or_goes_quiet() {
    let (world, document) = started("due");
    world.save(&saying(&document, 0, "Sent."));
    world.step();

    assert!(
        world
            .mirror
            .due_builds(&[], Duration::from_secs(600))
            .unwrap()
            .is_empty(),
        "still being written"
    );
    assert_eq!(
        world
            .mirror
            .due_builds(&["lin:one".to_owned()], Duration::from_secs(600))
            .unwrap(),
        [("algebra".to_owned(), Side::Shared)]
    );
    assert_eq!(
        world.mirror.due_builds(&[], Duration::ZERO).unwrap().len(),
        1,
        "or it went quiet"
    );

    let built = crate::build::read(&world.mirror, "algebra", Side::Shared).unwrap();
    world.mirror.built(&built.nodes).unwrap();

    assert!(
        world
            .mirror
            .due_builds(&[], Duration::ZERO)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_window_keeps_the_machine_while_it_is_open() {
    let world = world("window");
    let store = Store::at(&world.root.join("state"));

    assert!(!store.window_open());
    let held = store.hold_window().unwrap();
    assert!(store.window_open());
    assert!(store.hold_window().is_err(), "one window at a time");
    drop(held);
    assert!(!store.window_open());
}

#[test]
fn a_line_taken_to_send_it_is_let_go_even_when_sending_fails() {
    let (world, document) = started("taken-and-lost");
    world.instance.with("lin:one", |line| line.holder = None);
    world.save(&saying(&document, 0, "Written after the claim lapsed."));
    // The first write finds nobody holding the line; the second, made
    // once this machine has taken it, never arrives.
    world.instance.lost_at_write.set(Some(2));

    let report = world.step();

    assert!(report.stopped.is_some());
    assert_eq!(world.instance.released.get(), 1, "the line was not let go");
    assert_eq!(
        world.instance.with("lin:one", |line| line.holder.clone()),
        None
    );
    assert_eq!(world.state(), State::NotSent);
}

#[test]
fn two_versions_kept_aside_at_once_are_both_kept() {
    let (world, document) = started("aside-twice");
    world.save(&saying(&document, 0, "Not sent."));
    std::fs::write(world.file(Side::Shared, "quotients"), "edited by hand").unwrap();
    world.instance.with("lin:one", |line| line.deleted = true);

    // Sending, unlike a whole round, does not put the files right first,
    // so the line goes with a file changed by hand and changes not sent.
    let report = world.mirror.send(&world.instance, &Which::All);

    assert_eq!(report.gone, ["algebra/shared/quotients"]);
    assert_eq!(report.aside.len(), 2);
    assert_ne!(report.aside[0].path, report.aside[1].path);
    assert_eq!(
        std::fs::read_to_string(report.aside[0].path.join(repository::ENTRY)).unwrap(),
        "edited by hand"
    );
    assert!(
        std::fs::read_to_string(report.aside[1].path.join(repository::ENTRY))
            .unwrap()
            .contains("Not sent.")
    );
}

#[test]
fn a_record_is_kept_under_its_identifier_and_never_above_the_store() {
    let world = world("record-names");
    let store = Store::at(&world.root.join("state"));
    let mut record = world_record("lin:../../escaped");

    store.put(&record).unwrap();

    assert!(!world.root.join("escaped.json").exists());
    assert_eq!(
        store.get("lin:../../escaped").unwrap(),
        Some(record.clone())
    );
    assert_eq!(store.get("lin:elsewhere").unwrap(), None);

    record.id = "lin:one".to_owned();
    store.put(&record).unwrap();
    assert!(world.root.join("state/mirror/lines/lin-one.json").is_file());
}

fn world_record(id: &str) -> Record {
    let source = three().to_text();

    Record {
        id: id.to_owned(),
        group: "algebra".to_owned(),
        side: Side::Shared,
        name: "quotients".to_owned(),
        author: "usr:ana".to_owned(),
        writing: "anyone".to_owned(),
        base: 1,
        base_source: source.clone(),
        base_lean: None,
        source,
        lean_source: None,
        saved_at: None,
        there: None,
        trouble: None,
        unbuilt: None,
    }
}

#[test]
fn the_daemons_engine_waits_quietly_while_the_window_keeps_the_machine() {
    let world = world("background-waits");
    let home = Home::locate(&Environment::from_pairs([(HOME_VARIABLE, &world.root)]));
    let _window = Store::at(home.state_dir()).hold_window().unwrap();
    let (nudges, heard) = super::engine::channel();
    let (finished, done) = std::sync::mpsc::channel();

    let running = home.clone();
    std::thread::spawn(move || {
        let mut rounds = 0;
        super::engine::run(&running, super::engine::Role::Background, &heard, |_| {
            rounds += 1;
        });
        let _ = finished.send(rounds);
    });

    nudges.send(super::engine::Nudge::Now).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    drop(nudges);

    // An engine that only ever looked at the window again would never
    // notice that nobody can nudge it any more.
    let rounds = done
        .recv_timeout(Duration::from_secs(10))
        .expect("the engine is still going with nobody left to nudge it");
    assert_eq!(rounds, 0, "the engine worked while the window was open");
}

#[test]
fn only_the_groups_the_instance_lists_are_the_machines() {
    let (world, _) = started("listed-groups");
    world
        .instance
        .other_groups
        .borrow_mut()
        .push("topology".to_owned());
    world.step();

    // A directory left from an earlier instance at the same address.
    let stray = world.root.join("workspace/geometry/shared");
    std::fs::create_dir_all(stray.join("lines/circles")).unwrap();
    std::fs::write(stray.join(repository::MANIFEST), manifest("shared")).unwrap();
    std::fs::write(stray.join("lines/circles/line.json"), "{}").unwrap();
    // And something that is not a repository Stemma wrote.
    let notes = world.root.join("workspace/notes");
    std::fs::create_dir_all(&notes).unwrap();

    assert_eq!(world.mirror.groups().unwrap(), ["algebra", "topology"]);

    world.step();

    assert!(!world.root.join("workspace/geometry").exists());
    assert!(notes.exists());
    assert!(world.root.join("workspace/topology/shared").exists());
    assert_eq!(world.mirror.groups().unwrap(), ["algebra", "topology"]);
}

#[test]
fn a_group_left_goes_once_its_lines_have_gone() {
    let (world, document) = started("left-group");
    world.save(&saying(&document, 0, "Not sent."));
    world.instance.away.set(true);
    world.mirror.step(&world.instance, false, &Which::All);
    world.instance.away.set(false);

    world.instance.left_algebra.set(true);
    world.instance.lines.borrow_mut().clear();
    let report = world.step();

    assert!(!world.root.join("workspace/algebra").exists());
    assert!(world.mirror.groups().unwrap().is_empty());
    // What was not sent was kept aside before the group went.
    assert!(!report.aside.is_empty());
}

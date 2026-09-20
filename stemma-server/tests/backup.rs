//! Taking a copy of an instance, and looking inside it.
//!
//! These build a state directory with real repositories in it and run
//! the real `git` and `tar`, because what is being tested is whether
//! what comes out can be put back, and only those two can say.

use std::path::{Path, PathBuf};
use std::process::Command;

use stemma_core::id::{GroupId, Slug};
use stemma_core::repository::{Manifest, Side};
use stemma_server::backup::{self, Options};
use stemma_server::config::{Listen, Paths, PolicySeed, PublicUrl, Settings};
use stemma_server::db::{self, Database};
use stemma_server::git::{self, Repositories};

/// An instance's files, without an instance running on them.
struct Instance {
    root: PathBuf,
    settings: Settings,
    config_path: PathBuf,
}

impl Instance {
    /// Lays out a state directory with a database, a repository, a key
    /// and a build output in it.
    fn at(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "stemma-backup-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let state = root.join("state");
        let paths = Paths::new(&state);
        std::fs::create_dir_all(&state).expect("somewhere to put the instance");

        let mut database = Database::open_or_create(&paths.database()).expect("a database on disk");
        database.migrate().expect("the schema applies");
        drop(database);

        let group = GroupId::new(Slug::parse("cosme").expect("a name"));
        let repositories = Repositories::new(paths.git_dir());
        git::ensure(
            &repositories.shared(&group),
            &Manifest::new(group.clone(), Side::Shared, "the group's own"),
        )
        .expect("a repository");

        std::fs::create_dir_all(paths.keys_dir()).expect("a place for the keys");
        std::fs::write(paths.session_key(), "a key").expect("a key");

        std::fs::create_dir_all(paths.artefacts_dir()).expect("a place for the outputs");
        std::fs::write(paths.artefacts_dir().join("one.pdf"), "a build").expect("a build");

        std::fs::write(paths.database_backup("1"), "an older copy").expect("an older copy");

        let config_path = root.join("server.toml");
        std::fs::write(&config_path, "public_url = \"https://stemma.test\"\n")
            .expect("a configuration");

        let settings = Settings {
            public_url: PublicUrl::parse("https://stemma.test").expect("an address"),
            listen: "127.0.0.1:0".parse::<Listen>().expect("an address"),
            paths,
            policy_seed: PolicySeed::default(),
            source: config_path.clone(),
        };

        Self {
            root,
            settings,
            config_path,
        }
    }

    /// Where copies are written.
    fn backups(&self) -> PathBuf {
        self.root.join("backups")
    }

    /// Takes one.
    fn take(&self, options: Options) -> backup::Backup {
        backup::take(&self.settings, &self.config_path, &options).expect("a copy")
    }

    /// The copy it was asked for, unpacked.
    fn unpacked(&self, taken: &backup::Backup) -> PathBuf {
        let into = self
            .root
            .join(format!("unpacked-{}", taken.manifest.taken_at));
        std::fs::create_dir_all(&into).expect("somewhere to unpack it");

        let status = Command::new("tar")
            .arg("-xzf")
            .arg(&taken.path)
            .arg("-C")
            .arg(&into)
            .status()
            .expect("tar runs");
        assert!(status.success(), "tar could not read the archive");

        into.join(&taken.manifest.name)
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// What a copy is asked for when nothing particular is being tested.
fn plainly(to: PathBuf) -> Options {
    Options {
        to,
        keep: None,
        artefacts: false,
    }
}

#[test]
fn a_copy_holds_everything_the_instance_could_not_be_rebuilt_without() {
    let instance = Instance::at("whole");
    let taken = instance.take(plainly(instance.backups()));
    let inside = instance.unpacked(&taken);

    let database = inside.join("state/server.db");
    assert!(database.is_file(), "the database is not in the copy");
    assert_eq!(
        db::version_of(&database).expect("a readable database"),
        db::expected_version()
    );

    let repository = inside.join("state/git/cosme/shared.git");
    assert!(
        git::exists(&repository),
        "the repository is not in the copy"
    );
    assert_eq!(
        git::head_commit(&repository),
        git::head_commit(&instance.settings.paths.git_dir().join("cosme/shared.git")),
        "the copy of the repository is at a different commit"
    );

    assert_eq!(
        std::fs::read_to_string(inside.join("state/keys/session.key")).ok(),
        Some("a key".to_owned())
    );
    assert!(
        inside.join("config/server.toml").is_file(),
        "the configuration is not in the copy"
    );

    assert_eq!(taken.manifest.repositories, vec!["cosme/shared.git"]);
    assert_eq!(taken.manifest.schema, Some(db::expected_version()));
}

#[test]
fn what_the_instance_can_make_again_stays_behind() {
    let instance = Instance::at("evictable");

    // Held open for the length of the copy, so that the write-ahead log
    // and its index are on disk while it is taken, exactly as they are
    // on a machine that is answering requests.
    let running = Database::open(&instance.settings.paths.database()).expect("a connection");

    let taken = instance.take(plainly(instance.backups()));
    let inside = instance.unpacked(&taken);

    assert!(
        instance
            .settings
            .paths
            .data_dir()
            .join("server.db-wal")
            .is_file(),
        "the instance was not writing ahead, so this proves nothing"
    );
    drop(running);

    assert!(
        !inside.join("state/artefacts").exists(),
        "the build outputs came along uninvited"
    );
    assert!(
        !inside.join("state/server.db.backup-1").exists(),
        "an older copy of the database came along"
    );
    assert!(
        !inside.join("state/server.db-wal").exists(),
        "the write-ahead log came along beside a copy that already holds it"
    );

    assert!(
        taken
            .manifest
            .left_out
            .iter()
            .any(|reason| reason.contains("build outputs")),
        "the copy does not admit what it left: {:?}",
        taken.manifest.left_out
    );
}

#[test]
fn the_build_outputs_come_when_they_are_asked_for() {
    let instance = Instance::at("artefacts");
    let taken = instance.take(Options {
        artefacts: true,
        ..plainly(instance.backups())
    });
    let inside = instance.unpacked(&taken);

    assert_eq!(
        std::fs::read_to_string(inside.join("state/artefacts/one.pdf")).ok(),
        Some("a build".to_owned())
    );
    assert!(taken.manifest.artefacts);
}

#[test]
fn whatever_else_the_state_directory_holds_comes_too() {
    let instance = Instance::at("unknown");
    let state = instance.settings.paths.data_dir();
    std::fs::write(state.join("something-new"), "written by a later release")
        .expect("something new");

    let taken = instance.take(plainly(instance.backups()));
    let inside = instance.unpacked(&taken);

    assert_eq!(
        std::fs::read_to_string(inside.join("state/something-new")).ok(),
        Some("written by a later release".to_owned())
    );
    assert_eq!(taken.manifest.also, vec!["something-new"]);
}

#[test]
fn a_copied_repository_forgets_the_machine_it_came_from() {
    let instance = Instance::at("origin");
    let taken = instance.take(plainly(instance.backups()));
    let inside = instance.unpacked(&taken);

    let told = Command::new("git")
        .arg("-C")
        .arg(inside.join("state/git/cosme/shared.git"))
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .expect("git runs");

    assert!(
        !told.status.success(),
        "the copy still points at where it was taken from: {}",
        String::from_utf8_lossy(&told.stdout)
    );
}

#[test]
fn keeping_a_few_removes_the_oldest() {
    let instance = Instance::at("keep");
    let backups = instance.backups();
    std::fs::create_dir_all(&backups).expect("somewhere to keep them");

    for stamp in ["0000000001", "0000000002", "0000000003"] {
        std::fs::write(
            backups.join(format!("stemma-backup-{stamp}.tar.gz")),
            "an older copy",
        )
        .expect("an older copy");
    }

    let taken = instance.take(Options {
        keep: Some(2),
        ..plainly(backups.clone())
    });

    assert_eq!(
        taken.pruned,
        vec![
            "stemma-backup-0000000001.tar.gz",
            "stemma-backup-0000000002.tar.gz"
        ]
    );
    assert!(archives(&backups).contains(&"stemma-backup-0000000003.tar.gz".to_owned()));
    assert!(taken.path.is_file(), "the copy just taken was removed");
    assert_eq!(archives(&backups).len(), 2);
}

#[test]
fn a_directory_asked_to_keep_nothing_still_keeps_the_copy_just_taken() {
    let instance = Instance::at("keep-none");
    let backups = instance.backups();

    let taken = instance.take(Options {
        keep: Some(0),
        ..plainly(backups.clone())
    });

    assert!(taken.path.is_file());
    assert_eq!(archives(&backups).len(), 1);
}

#[test]
fn nothing_is_left_lying_beside_the_archive() {
    let instance = Instance::at("tidy");
    let backups = instance.backups();
    let taken = instance.take(plainly(backups.clone()));

    let left: Vec<String> = std::fs::read_dir(&backups)
        .expect("the directory the copy went into")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();

    assert_eq!(
        left,
        vec![
            taken
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("a name")
                .to_owned()
        ]
    );
}

#[cfg(unix)]
#[test]
fn the_archive_is_nobody_elses_business() {
    use std::os::unix::fs::PermissionsExt;

    let instance = Instance::at("private");
    let taken = instance.take(plainly(instance.backups()));

    let mode = std::fs::metadata(&taken.path)
        .expect("the archive")
        .permissions()
        .mode();

    assert_eq!(mode & 0o077, 0, "the archive carries the session key");
}

/// The archives in a directory, by name.
fn archives(inside: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(inside)
        .expect("the directory the copies go into")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with(".tar.gz"))
        .collect();

    found.sort();

    found
}

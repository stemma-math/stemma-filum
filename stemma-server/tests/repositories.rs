//! Cloning and pushing with git itself.
//!
//! These drive the real `git` binary against a listening instance,
//! because the thing being tested is whether git is happy — and only git
//! can say. They ask for a runtime with more than one thread, since
//! waiting for git to finish would otherwise stop the instance answering
//! it.

use std::path::{Path, PathBuf};
use std::process::Command;

use stemma_core::access::GroupRole;
use stemma_core::id::Slug;
use stemma_server::config::{Listen, Paths, PolicySeed, PublicUrl, Settings};
use stemma_server::db::Database;
use stemma_server::store::{devices, groups, users};
use stemma_server::tokens::Key;
use stemma_server::web::{self, Instance};
use stemma_server::{git, repositories};

/// An instance listening on a port, with its repositories on disk.
struct Listening {
    root: PathBuf,
    address: String,
    instance: Instance,
}

impl Listening {
    /// Starts one.
    async fn start(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("stemma-repositories-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a directory to work in");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port");
        let port = listener.local_addr().expect("an address").port();
        let address = format!("http://127.0.0.1:{port}");

        let mut database = Database::in_memory().expect("a database");
        database.migrate().expect("the schema applies");

        let settings = Settings {
            public_url: PublicUrl::parse(&address).expect("an address"),
            listen: "127.0.0.1:0".parse::<Listen>().expect("an address"),
            paths: Paths::new(&root),
            policy_seed: PolicySeed::default(),
            source: root.join("server.toml"),
        };

        let instance = Instance::new(settings, database, Key::from_material(b"a test key"));
        let app = web::router().with_state(instance.clone());

        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self {
            root,
            address,
            instance,
        }
    }

    /// Makes an account, a group it owns, and the repositories for both.
    async fn group_with_owner(&self, owner: &str, group: &str) {
        let owner = owner.to_owned();
        let group = group.to_owned();
        let root = self.root.clone();

        self.instance
            .write(move |connection| {
                let user = users::create(
                    connection,
                    &Slug::parse(&owner).unwrap(),
                    &format!("{owner}@example.org"),
                    &owner,
                    "hash",
                )?
                .expect("a new account");

                let group = groups::create(
                    connection,
                    &Slug::parse(&group).unwrap(),
                    "A group",
                    &user.id,
                )?;

                repositories::for_both(&git::Repositories::new(root.join("git")), &group, &user)?;

                Ok(())
            })
            .await
            .expect("a group with its repositories");
    }

    /// A credential for an account, made the way a signed-in machine gets
    /// one.
    async fn credential(&self, account: &str) -> String {
        let account = account.to_owned();

        self.instance
            .write(move |connection| {
                let key = Key::from_material(b"a test key");
                let user = users::by_login(connection, &account)?.expect("the account");
                let (grant, device_code) = devices::start(connection, &key, "a test machine")?;

                devices::approve(connection, &grant.id, &user.id)?;

                match devices::collect(connection, &key, &device_code)? {
                    devices::Collected::Issued { token, .. } => Ok(token),
                    other => panic!("no credential: {other:?}"),
                }
            })
            .await
            .expect("a credential")
    }

    /// Puts somebody in a group, with their own repository.
    async fn add_member(&self, account: &str, group: &str, role: GroupRole) {
        let account = account.to_owned();
        let group = group.to_owned();
        let root = self.root.clone();

        self.instance
            .write(move |connection| {
                let user = users::create(
                    connection,
                    &Slug::parse(&account).unwrap(),
                    &format!("{account}@example.org"),
                    &account,
                    "hash",
                )?
                .expect("a new account");

                let group =
                    groups::by_slug(connection, &Slug::parse(&group).unwrap())?.expect("the group");

                groups::add_member(connection, &group.id, &user.id, role)?;
                repositories::for_member(&git::Repositories::new(root.join("git")), &group, &user)?;

                Ok(())
            })
            .await
            .expect("a member");
    }

    /// The address of one of this instance's repositories.
    fn repository(&self, path: &str) -> String {
        format!("{}/{path}", self.address)
    }

    /// Where a test can put a working copy.
    fn workspace(&self, name: &str) -> PathBuf {
        self.root.join("work").join(name)
    }

    /// Takes everything down.
    fn finish(self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// What running git said.
struct Ran {
    worked: bool,
    said: String,
}

/// Runs git, carrying a credential the way our client does.
fn git_with(token: &str, directory: &Path, arguments: &[&str]) -> Ran {
    std::fs::create_dir_all(directory).expect("somewhere to run");

    let output = Command::new("git")
        .args([
            "-c",
            &format!("http.extraHeader=Authorization: Bearer {token}"),
        ])
        .args(arguments)
        .current_dir(directory)
        // Without this, a refused credential turns into a prompt nobody
        // is there to answer.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Ana")
        .env("GIT_AUTHOR_EMAIL", "ana@example.org")
        .env("GIT_COMMITTER_NAME", "Ana")
        .env("GIT_COMMITTER_EMAIL", "ana@example.org")
        .output()
        .expect("git runs");

    Ran {
        worked: output.status.success(),
        said: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_clones_the_group_and_pushes_back_to_it() {
    let listening = Listening::start("shared").await;
    listening.group_with_owner("ana", "algebra").await;
    let token = listening.credential("ana").await;

    let work = listening.workspace("ana");
    let url = listening.repository("git/algebra/shared.git");

    let cloned = git_with(&token, &work, &["clone", &url, "shared"]);
    assert!(cloned.worked, "the clone failed: {}", cloned.said);
    assert!(
        work.join("shared/stemma.toml").is_file(),
        "nothing was cloned"
    );

    // Writing, committing and pushing, as a person would.
    std::fs::write(work.join("shared/line.typ"), "= A line of work\n").unwrap();

    let clone = work.join("shared");
    for arguments in [
        vec!["add", "line.typ"],
        vec!["commit", "-q", "-m", "Start a line"],
        vec!["push", "-q", "origin", "main"],
    ] {
        let ran = git_with(&token, &clone, &arguments);
        assert!(ran.worked, "git {arguments:?} failed: {}", ran.said);
    }

    // Somebody else in the group gets what was pushed.
    listening
        .add_member("enric", "algebra", GroupRole::Member)
        .await;
    let other = listening.credential("enric").await;
    let elsewhere = listening.workspace("enric");

    let cloned = git_with(&other, &elsewhere, &["clone", &url, "shared"]);
    assert!(cloned.worked, "the second clone failed: {}", cloned.said);
    assert!(
        elsewhere.join("shared/line.typ").is_file(),
        "the push did not arrive"
    );

    listening.finish();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_personal_repository_is_one_persons_and_nobody_elses() {
    let listening = Listening::start("personal").await;
    listening.group_with_owner("ana", "algebra").await;
    listening
        .add_member("enric", "algebra", GroupRole::Admin)
        .await;

    let ana = listening.credential("ana").await;
    let enric = listening.credential("enric").await;
    let url = listening.repository("git/algebra/personal/ana.git");

    let hers = git_with(&ana, &listening.workspace("ana"), &["clone", &url, "mine"]);
    assert!(hers.worked, "she could not clone her own: {}", hers.said);

    // An admin of the group, which is as much authority as the group has.
    let theirs = git_with(
        &enric,
        &listening.workspace("enric"),
        &["clone", &url, "hers"],
    );
    assert!(!theirs.worked, "somebody else cloned a private repository");

    listening.finish();
}

#[tokio::test(flavor = "multi_thread")]
async fn somebody_outside_the_group_is_told_there_is_nothing_there() {
    let listening = Listening::start("outsider").await;
    listening.group_with_owner("ana", "algebra").await;

    let outsider = listening
        .instance
        .write(|connection| {
            users::create(
                connection,
                &Slug::parse("mallory").unwrap(),
                "mallory@example.org",
                "Mallory",
                "hash",
            )?
            .expect("a new account");

            Ok(())
        })
        .await;
    assert!(outsider.is_ok());

    let token = listening.credential("mallory").await;
    let url = listening.repository("git/algebra/shared.git");

    let refused = git_with(
        &token,
        &listening.workspace("mallory"),
        &["clone", &url, "shared"],
    );

    assert!(!refused.worked, "an outsider cloned a group's repository");
    assert!(
        refused.said.contains("not found") || refused.said.contains("404"),
        "the refusal said more than it should: {}",
        refused.said
    );

    listening.finish();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_machine_with_no_credential_is_asked_for_one() {
    let listening = Listening::start("anonymous").await;
    listening.group_with_owner("ana", "algebra").await;

    let url = listening.repository("git/algebra/shared.git");
    let refused = git_with(
        "",
        &listening.workspace("nobody"),
        &["clone", &url, "shared"],
    );

    assert!(
        !refused.worked,
        "a repository was served to nobody in particular"
    );

    // Git answers a request for a credential by looking for one, and
    // there is none here to find.
    assert!(
        refused.said.contains("could not read Username") || refused.said.contains("Authentication"),
        "git was not asked for a credential: {}",
        refused.said
    );

    listening.finish();
}

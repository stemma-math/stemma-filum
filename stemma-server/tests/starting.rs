//! Starting a line in the web, and moving it to the group.

mod support;

use axum::http::StatusCode;
use stemma_core::access::GroupRole;
use stemma_core::document::{Block, Document};
use stemma_core::id::Slug;
use stemma_core::line::Name;
use stemma_core::repository::{self, Side};
use stemma_server::store::{groups, users};
use support::{Harness, Page};

/// The password every account in these tests uses.
const PASSWORD: &str = "a long enough password";

/// The hidden field a form has to send back.
fn form_token(page: &Page) -> String {
    let marker = "name=\"form_token\" value=\"";
    let start = page.body.find(marker).expect("a form token") + marker.len();
    let rest = &page.body[start..];

    rest[..rest.find('"').expect("the end of the token")].to_owned()
}

/// Registers somebody and leaves them signed in.
async fn register(harness: &mut Harness, name: &str) {
    harness
        .post(
            "/register",
            &[
                ("display_name", name),
                ("slug", name),
                ("email", &format!("{name}@example.org")),
                ("password", PASSWORD),
            ],
        )
        .await;
}

/// Signs somebody in again.
async fn sign_in(harness: &mut Harness, name: &str) {
    harness
        .post("/login", &[("login", name), ("password", PASSWORD)])
        .await;
}

/// Makes a group owned by somebody, with its repositories on disk.
async fn own_a_group(harness: &mut Harness, owner: &str) {
    let owner = owner.to_owned();

    harness
        .instance
        .write(move |connection| {
            let user = users::by_login(connection, &owner)?.expect("the account");
            groups::create(
                connection,
                &Slug::parse("algebra").unwrap(),
                "Algebra",
                &user.id,
            )?;

            Ok(())
        })
        .await
        .unwrap();

    make_repositories(harness, "ana").await;
}

/// Puts somebody in the group, with a repository of their own.
async fn add_member(harness: &mut Harness, name: &str) {
    let who = name.to_owned();

    harness
        .instance
        .write(move |connection| {
            let user = users::by_login(connection, &who)?.expect("the account");
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");

            groups::add_member(connection, &group.id, &user.id, GroupRole::Member)?;

            Ok(())
        })
        .await
        .unwrap();

    make_repositories(harness, name).await;
}

/// The repositories a group and one of its members need.
async fn make_repositories(harness: &mut Harness, who: &str) {
    let who = who.to_owned();
    let repositories = harness.instance.repositories();

    harness
        .instance
        .read(move |connection| {
            let user = users::by_login(connection, &who)?.expect("the account");
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");

            stemma_server::git::ensure(
                &repositories.shared(&group.id),
                &repository::Manifest::new(group.id.clone(), Side::Shared, "Algebra"),
            )?;
            stemma_server::git::ensure(
                &repositories.personal(&group.id, &user.id),
                &repository::Manifest::new(group.id.clone(), Side::Private, "Ana"),
            )?;

            Ok(())
        })
        .await
        .unwrap();
}

/// Starts a line through the form.
async fn start(harness: &mut Harness, title: &str, description: &str, seen_by: &str) -> Page {
    let form = harness.get("/g/algebra/lines/new").await;
    let token = form_token(&form);

    harness
        .post(
            "/g/algebra/lines",
            &[
                ("form_token", &token),
                ("title", title),
                ("description", description),
                ("seen_by", seen_by),
            ],
        )
        .await
}

/// What a repository holds for a line right now.
async fn source(harness: &Harness, who: Option<&str>, line: &str) -> Option<String> {
    let who = who.map(str::to_owned);
    let line = line.to_owned();
    let repositories = harness.instance.repositories();

    harness
        .instance
        .read(move |connection| {
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");
            let path = match &who {
                Some(who) => {
                    let user = users::by_login(connection, who)?.expect("the account");

                    repositories.personal(&group.id, &user.id)
                }
                None => repositories.shared(&group.id),
            };
            let Some(head) = stemma_server::git::head_commit(&path) else {
                return Ok(None);
            };
            let inside = format!("{}/{}/{}", repository::LINES, line, repository::ENTRY);

            Ok(stemma_server::git::show(&path, &head, &inside).ok())
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn a_line_is_created_private_with_its_title_and_description() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let form = harness.get("/g/algebra/lines/new").await;
    assert!(form.says("Only me"));
    assert!(form.says("The whole group"));

    let made = start(
        &mut harness,
        "Quotient groups",
        "Does every finite group split?",
        "me",
    )
    .await;

    assert_eq!(made.status, StatusCode::SEE_OTHER);
    assert_eq!(
        made.location.as_deref(),
        Some("/g/algebra/private/quotient-groups")
    );

    let written = source(&harness, Some("ana"), "quotient-groups")
        .await
        .expect("the line was written");

    let written = Document::parse(&written).expect("a line");
    assert_eq!(written.title, "Quotient groups");
    assert_eq!(
        written.description.as_deref(),
        Some("Does every finite group split?")
    );
    assert!(written.blocks.is_empty());

    // Private is physical: it is in her repository and in nobody else's.
    assert_eq!(source(&harness, None, "quotient-groups").await, None);
}

#[tokio::test]
async fn a_line_needs_a_title_and_not_a_description() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let refused = start(&mut harness, "   ", "About something", "me").await;
    assert_eq!(refused.status, StatusCode::OK);
    assert!(refused.says("A line needs a title."));

    let made = start(&mut harness, "Quotient groups", "   ", "me").await;
    assert_eq!(made.status, StatusCode::SEE_OTHER);
    let written = source(&harness, Some("ana"), "quotient-groups")
        .await
        .expect("the line was written");
    assert_eq!(Document::parse(&written).unwrap().description, None);
}

#[tokio::test]
async fn sharing_moves_the_line_out_of_the_authors_repository() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    start(&mut harness, "Quotient groups", "Does it split?", "me").await;

    let form = harness
        .get("/g/algebra/private/quotient-groups/share")
        .await;

    assert!(form.says("Today's state only"));
    assert!(form.says("Everything from the beginning"));
    assert!(form.says("Who can write"));

    let moved = harness
        .post(
            "/g/algebra/private/quotient-groups/share",
            &[
                ("form_token", &form_token(&form)),
                ("travels", "today"),
                ("writing", "author"),
            ],
        )
        .await;

    assert_eq!(moved.status, StatusCode::SEE_OTHER);
    assert_eq!(
        moved.location.as_deref(),
        Some("/g/algebra/shared/quotient-groups")
    );

    assert!(source(&harness, None, "quotient-groups").await.is_some());
    assert_eq!(
        source(&harness, Some("ana"), "quotient-groups").await,
        None,
        "a move, and not a copy"
    );
}

#[tokio::test]
async fn nothing_shared_may_lean_on_something_only_its_author_can_see() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    start(&mut harness, "Notation", "What do we write?", "me").await;
    start(&mut harness, "Quotient groups", "Does it split?", "me").await;

    // She reaches into her other line, which the group cannot see.
    let group = harness.instance.repositories();
    harness
        .instance
        .read(move |connection| {
            let user = users::by_login(connection, "ana")?.expect("the account");
            let found =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");
            let mine = group.personal(&found.id, &user.id);
            let leaning = Document::new("Quotient groups", None)
                .with(Block::prose("#import \"../notation/main.typ\": *"))
                .to_text();

            stemma_server::git::start_line(
                &mine,
                &Name::parse("quotient-groups").unwrap(),
                &leaning,
                &stemma_server::git::Author {
                    name: "Ana",
                    email: "ana@example.org",
                },
            )?;

            Ok(())
        })
        .await
        .unwrap();

    let form = harness
        .get("/g/algebra/private/quotient-groups/share")
        .await;

    assert!(form.says("depends on private work"));
    assert!(form.says("../notation/main.typ"));

    let refused = harness
        .post(
            "/g/algebra/private/quotient-groups/share",
            &[
                ("form_token", &form_token(&form)),
                ("travels", "today"),
                ("writing", "anyone"),
            ],
        )
        .await;

    assert_eq!(refused.status, StatusCode::OK);
    assert!(refused.says("cannot depend on private work"));
    assert_eq!(source(&harness, None, "quotient-groups").await, None);
}

#[tokio::test]
async fn a_line_its_author_kept_is_not_taken_by_somebody_elses_machine() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    start(&mut harness, "Quotient groups", "Does it split?", "me").await;

    let form = harness
        .get("/g/algebra/private/quotient-groups/share")
        .await;
    harness
        .post(
            "/g/algebra/private/quotient-groups/share",
            &[
                ("form_token", &form_token(&form)),
                ("travels", "today"),
                ("writing", "author"),
            ],
        )
        .await;

    register(&mut harness, "bruno").await;
    add_member(&mut harness, "bruno").await;

    let token = harness.credential("bruno").await;
    harness
        .post_json_with_token(
            "/api/machines",
            &token,
            r#"{"label":"bruno's laptop","groups":["algebra"],"agents":[]}"#,
        )
        .await;

    let refused = harness
        .post_json_with_token(
            "/api/claims",
            &token,
            r#"{"group":"algebra","side":"shared","line":"quotient-groups"}"#,
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert!(refused.says("others send proposals"));

    // And her own machine still writes it.
    sign_in(&mut harness, "ana").await;
    let hers = harness.credential("ana").await;
    harness
        .post_json_with_token(
            "/api/machines",
            &hers,
            r#"{"label":"ana's laptop","groups":["algebra"],"agents":[]}"#,
        )
        .await;

    let taken = harness
        .post_json_with_token(
            "/api/claims",
            &hers,
            r#"{"group":"algebra","side":"shared","line":"quotient-groups"}"#,
        )
        .await;

    assert_eq!(taken.status, StatusCode::OK);
}

#[tokio::test]
async fn somebody_outside_the_group_starts_nothing_in_it() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    register(&mut harness, "bruno").await;

    let asked = harness.get("/g/algebra/lines/new").await;

    assert_eq!(asked.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_machine_with_no_desktop_starts_and_shares_a_line_too() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let token = harness.credential("ana").await;
    let started = harness
        .post_json_with_token(
            "/api/lines",
            &token,
            r#"{"group":"algebra","title":"Quotient groups",
                "description":"Does it split?","seen_by":"me"}"#,
        )
        .await;

    assert_eq!(started.status, StatusCode::OK);
    assert!(started.says("quotient-groups"));
    assert!(started.says("private"));

    let shared = harness
        .post_json_with_token(
            "/api/lines/algebra/quotient-groups/share",
            &token,
            r#"{"travels":"everything","writing":"anyone"}"#,
        )
        .await;

    assert_eq!(shared.status, StatusCode::OK);
    assert!(source(&harness, None, "quotient-groups").await.is_some());
    assert_eq!(source(&harness, Some("ana"), "quotient-groups").await, None);
}

#[tokio::test]
async fn the_description_is_what_a_listing_shows_when_a_title_is_not_enough() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    start(
        &mut harness,
        "Quotient groups",
        "Does every finite group split?",
        "group",
    )
    .await;

    // A machine builds it and hands over what it read.
    let built = source(&harness, None, "quotient-groups")
        .await
        .expect("the line");

    publish(&mut harness, &built).await;

    let space = harness.get("/g/algebra").await;
    assert!(space.says("Does every finite group split?"));

    let line = harness.get("/g/algebra/shared/quotient-groups").await;
    assert!(line.says("Does every finite group split?"));
}

/// Puts the line on the instance, as the machine that built it would.
async fn publish(harness: &mut Harness, built: &str) {
    use stemma_core::artefact::{Kind, LineDigest, Nodes, Scope};
    use stemma_server::artefacts::Shelf;
    use stemma_server::store::artefacts as index;

    let built = built.to_owned();
    let shelf = Shelf::new(harness.instance.settings.paths.artefacts_dir());

    harness
        .instance
        .write(move |connection| {
            let user = users::by_login(connection, "ana")?.expect("the account");
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");
            let scope = Scope::of(Side::Shared, &user.id);
            let commit = "0123456789abcdef0123456789abcdef01234567";

            let mut nodes = Nodes::new(group.id.clone(), Side::Shared, commit.to_owned());
            let document = Document::parse(&built).expect("a line");
            nodes.lines.push(LineDigest {
                name: Name::parse("quotient-groups").unwrap(),
                title: Some(document.title.clone()),
                description: document.description.clone(),
                line: stemma_core::line::Line::of(&document).expect("a line that reads"),
                lean: None,
            });

            let text = nodes.to_text();
            shelf.put(&group.id, &scope, Kind::Nodes, commit, text.as_bytes())?;
            index::record(
                connection,
                &group.id,
                &scope,
                Kind::Nodes,
                commit,
                text.len(),
                &user.id,
            )?;

            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_line_of_your_own_can_be_taken_out_and_its_history_stays() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    start(&mut harness, "Quotient groups", "Does it split?", "me").await;

    let line = harness.get("/g/algebra/private/quotient-groups").await;
    assert!(line.says("Delete this line"));

    let gone = harness
        .post(
            "/g/algebra/private/quotient-groups/delete",
            &[("form_token", &form_token(&line))],
        )
        .await;

    assert_eq!(gone.status, StatusCode::SEE_OTHER);
    assert_eq!(source(&harness, Some("ana"), "quotient-groups").await, None);

    // The repository still holds every version of it.
    let kept = harness
        .instance
        .read({
            let repositories = harness.instance.repositories();

            move |connection| {
                let user = users::by_login(connection, "ana")?.expect("the account");
                let group = groups::by_slug(connection, &Slug::parse("algebra").unwrap())?
                    .expect("the group");
                let path = repositories.personal(&group.id, &user.id);
                let head = stemma_server::git::head_commit(&path).expect("a branch");
                let before = format!("{head}~1");

                Ok(stemma_server::git::show(
                    &path,
                    &before,
                    &format!(
                        "{}/quotient-groups/{}",
                        repository::LINES,
                        repository::ENTRY
                    ),
                )
                .is_ok())
            }
        })
        .await
        .unwrap();

    assert!(kept, "the commit before still has it");
}

#[tokio::test]
async fn nobody_takes_out_a_line_the_group_has() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    start(&mut harness, "Quotient groups", "Does it split?", "group").await;

    let token = harness.credential("ana").await;
    let refused = harness
        .post_json_with_token("/api/lines/algebra/quotient-groups/delete", &token, "{}")
        .await;

    assert_eq!(refused.status, StatusCode::NOT_FOUND);
    assert!(source(&harness, None, "quotient-groups").await.is_some());
}

#[tokio::test]
async fn a_line_can_be_read_the_moment_it_exists_and_says_what_is_missing() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    start(
        &mut harness,
        "Quotient groups",
        "Does every finite group split?",
        "me",
    )
    .await;

    // Nothing has built it. It is still there, and the group space
    // lists it.
    let space = harness.get("/g/algebra").await;
    assert!(space.says("Quotient groups"));

    let line = harness.get("/g/algebra/private/quotient-groups").await;
    assert!(line.status.is_success());
    assert!(line.says("Does every finite group split?"));
    assert!(line.says("This line does not contain any declarations yet"));
}

/// Writes a line on a machine and pushes it, as synchronising does.
async fn write(harness: &Harness, who: Option<&str>, line: &str, text: &str) {
    let who = who.map(str::to_owned);
    let repositories = harness.instance.repositories();

    let bare = harness
        .instance
        .read(move |connection| {
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");

            Ok(match &who {
                Some(who) => {
                    let user = users::by_login(connection, who)?.expect("the account");

                    repositories.personal(&group.id, &user.id)
                }
                None => repositories.shared(&group.id),
            })
        })
        .await
        .unwrap();

    let work = bare.with_extension("work");
    let _ = std::fs::remove_dir_all(&work);

    let git = |at: &std::path::Path, arguments: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(at)
            .args(arguments)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Ana")
            .env("GIT_AUTHOR_EMAIL", "ana@example.org")
            .env("GIT_COMMITTER_NAME", "Ana")
            .env("GIT_COMMITTER_EMAIL", "ana@example.org")
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };

    git(
        bare.parent().unwrap(),
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    std::fs::write(
        work.join(repository::LINES)
            .join(line)
            .join(repository::ENTRY),
        text,
    )
    .unwrap();
    git(&work, &["commit", "-q", "-am", &format!("Write {line}")]);
    git(&work, &["push", "-q", "origin", "main"]);

    std::fs::remove_dir_all(&work).unwrap();
}

#[tokio::test]
async fn what_a_machine_pushed_is_shown_as_it_was_written() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    start(&mut harness, "Quotient groups", "Does it split?", "me").await;

    let started = source(&harness, Some("ana"), "quotient-groups")
        .await
        .expect("the line");
    write(
        &harness,
        Some("ana"),
        "quotient-groups",
        &Document::parse(&started)
            .unwrap()
            .with(Block::prose("It does, and here is why."))
            .to_text(),
    )
    .await;

    let line = harness.get("/g/algebra/private/quotient-groups").await;
    assert!(line.says("It does, and here is why."));
}

#[tokio::test]
async fn a_build_older_than_what_was_pushed_does_not_hide_it() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    start(&mut harness, "Quotient groups", "Does it split?", "group").await;

    // A machine built the line while it was still empty.
    let started = source(&harness, None, "quotient-groups")
        .await
        .expect("the line");
    publish(&mut harness, &started).await;

    write(
        &harness,
        None,
        "quotient-groups",
        &Document::parse(&started)
            .unwrap()
            .with(
                Block::theorem("Theorem", Some("thm:split".parse().unwrap()), "It splits.")
                    .titled("Splitting")
                    .central(true)
                    .carrying_proof(false),
            )
            .to_text(),
    )
    .await;

    let line = harness.get("/g/algebra/shared/quotient-groups").await;
    assert!(!line.says("This line does not contain any declarations yet"));
    assert!(line.says("thm:split"));
    assert!(line.says("It splits."));
}

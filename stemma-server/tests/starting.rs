//! Starting a line in the web, and moving it to the group.

mod support;

use axum::http::StatusCode;
use stemma_core::access::GroupRole;
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
async fn start(harness: &mut Harness, title: &str, question: &str, seen_by: &str) -> Page {
    let form = harness.get("/g/algebra/lines/new").await;
    let token = form_token(&form);

    harness
        .post(
            "/g/algebra/lines",
            &[
                ("form_token", &token),
                ("title", title),
                ("question", question),
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
async fn a_line_is_born_private_with_the_question_it_exists_to_answer() {
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

    assert!(written.contains("= Quotient groups"));
    assert!(written.contains("#question[Does every finite group split?]"));
    assert_eq!(
        stemma_core::source::question(&written).as_deref(),
        Some("Does every finite group split?")
    );

    // Private is physical: it is in her repository and in nobody else's.
    assert_eq!(source(&harness, None, "quotient-groups").await, None);
}

#[tokio::test]
async fn a_line_without_its_question_is_a_folder_and_is_refused() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let refused = start(&mut harness, "Quotient groups", "   ", "me").await;

    assert_eq!(refused.status, StatusCode::OK);
    assert!(refused.says("not a folder"));
    assert_eq!(source(&harness, Some("ana"), "quotient-groups").await, None);
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
            let leaning = "= Quotient groups\n\n\
                           #question[Does it split?]\n\n\
                           #import \"../notation/main.typ\": *\n";

            stemma_server::git::start_line(
                &mine,
                &Name::parse("quotient-groups").unwrap(),
                leaning,
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

    assert!(form.says("leans on work nobody else can see"));
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
    assert!(refused.says("may lean on something only you can"));
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
    assert!(refused.says("everybody else proposes"));

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
                "question":"Does it split?","seen_by":"me"}"#,
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

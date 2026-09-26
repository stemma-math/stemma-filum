//! Showing a line that somebody else's machine built.

mod support;

use axum::http::StatusCode;
use stemma_core::access::GroupRole;
use stemma_core::artefact::{Kind, Nodes, Scope};
use stemma_core::document::Document;
use stemma_core::id::Slug;
use stemma_core::repository::Side;
use stemma_server::artefacts::Shelf;
use stemma_server::store::{artefacts as index, groups, users};
use support::Harness;

/// The password every account in these tests uses.
const PASSWORD: &str = "a long enough password";

/// A line with two central statements and work between them.
fn source() -> Document {
    let quotient = support::statement("def:quotient", "Quotient group");
    let kernel = support::statement("lem:kernel", "Kernel").using(&quotient);
    let image = support::statement("lem:image", "Image");
    let first = support::statement("thm:first", "First isomorphism theorem")
        .central(true)
        .using(&quotient);
    let second = support::statement("thm:second", "Correspondence")
        .central(true)
        .using(&kernel)
        .using(&image);

    Document::new("Quotient groups", None)
        .with(quotient)
        .with(first)
        .with(kernel)
        .with(image)
        .with(second)
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

/// Makes a group owned by somebody.
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
}

/// Puts an artefact on the instance, as a machine would have.
async fn publish(harness: &mut Harness, side: Side, who: &str, commit: &str) {
    let who = who.to_owned();
    let commit = commit.to_owned();
    let shelf = Shelf::new(harness.instance.settings.paths.artefacts_dir());

    harness
        .instance
        .write(move |connection| {
            let user = users::by_login(connection, &who)?.expect("the account");
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");
            let scope = Scope::of(side, &user.id);

            let mut nodes = Nodes::new(group.id.clone(), side, commit.clone());
            nodes.lines.push(support::digest(&source()));

            let text = nodes.to_text();
            shelf.put(&group.id, &scope, Kind::Nodes, &commit, text.as_bytes())?;
            index::record(
                connection,
                &group.id,
                &scope,
                Kind::Nodes,
                &commit,
                text.len(),
                &user.id,
            )?;

            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_group_with_nothing_built_says_where_a_line_would_come_from() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let page = harness.get("/g/algebra").await;

    assert!(page.says("No lines of work yet."));
    assert!(page.says("create one from a connected machine"));
}

#[tokio::test]
async fn a_line_appears_once_a_machine_has_sent_what_it_read() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, Side::Shared, "ana", "0123456789abcdef").await;

    let space = harness.get("/g/algebra").await;
    assert!(space.says("Quotient groups"));
    assert!(space.says("2 central"));

    let line = harness.get("/g/algebra/shared/quotient-groups").await;

    assert_eq!(line.status, StatusCode::OK);
    assert!(line.says("First isomorphism theorem"));
    assert!(line.says("thm:first"));
    assert!(
        line.says("node--central"),
        "what the line rests on is drawn as such"
    );
    assert!(line.says("uses def:quotient"));
}

#[tokio::test]
async fn a_box_between_two_central_statements_is_drawn_as_one() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, Side::Shared, "ana", "0123456789abcdef").await;

    let line = harness.get("/g/algebra/shared/quotient-groups").await;

    assert!(line.says("black box"));
    assert!(line.says("Not completed"), "an empty box says so plainly");
    assert!(line.says("statements between the two it rests between"));
    assert!(line.says("3 total, 2 incomplete"));

    // A gap is hatched, never coloured: it is a declared absence, not a
    // failure and not an alarm.
    assert!(!line.says("--refuted"));
}

#[tokio::test]
async fn the_lemmas_inside_a_box_are_not_drawn_beside_the_statements() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, Side::Shared, "ana", "0123456789abcdef").await;

    let line = harness.get("/g/algebra/shared/quotient-groups").await;
    let body = &line.body;

    // They are named where they are used, and counted in the box, but
    // the line stays readable: forty lemmas must not become forty rows.
    assert!(body.contains("uses lem:kernel"));
    assert!(!body.contains("class=\"node__name\">lem:kernel"));
}

#[tokio::test]
async fn one_persons_own_work_is_theirs_and_not_the_groups() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, Side::Private, "ana", "0123456789abcdef").await;

    let hers = harness.get("/g/algebra/private/quotient-groups").await;
    assert_eq!(hers.status, StatusCode::OK);
    assert!(hers.says("Quotient groups"));

    // Somebody else in the group reaches for the same address and finds
    // their own private side, which holds nothing.
    harness.post("/logout", &[]).await;
    register(&mut harness, "enric").await;

    harness
        .instance
        .write(|connection| {
            let enric = users::by_login(connection, "enric")?.expect("the account");
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");

            groups::add_member(connection, &group.id, &enric.id, GroupRole::Member)?;

            Ok(())
        })
        .await
        .unwrap();

    let theirs = harness.get("/g/algebra/private/quotient-groups").await;
    assert_eq!(theirs.status, StatusCode::NOT_FOUND);

    let space = harness.get("/g/algebra").await;
    assert!(
        !space.says("Quotient groups"),
        "somebody else's private work is not listed"
    );
}

#[tokio::test]
async fn a_line_in_a_group_somebody_is_not_in_is_not_there_for_them() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, Side::Shared, "ana", "0123456789abcdef").await;

    harness.post("/logout", &[]).await;
    register(&mut harness, "mallory").await;

    let page = harness.get("/g/algebra/shared/quotient-groups").await;

    assert_eq!(page.status, StatusCode::NOT_FOUND);
    assert!(!page.says("First isomorphism"));
}

#[tokio::test]
async fn a_line_nobody_has_written_is_not_there() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, Side::Shared, "ana", "0123456789abcdef").await;

    assert_eq!(
        harness.get("/g/algebra/shared/no-such-line").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        harness
            .get("/g/algebra/sideways/quotient-groups")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

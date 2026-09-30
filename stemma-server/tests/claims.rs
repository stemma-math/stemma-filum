//! Who is writing a line, and from which machine.

mod support;

use axum::http::StatusCode;
use serde_json::Value;
use stemma_core::access::GroupRole;
use stemma_core::artefact::{Kind, Nodes, Scope};
use stemma_core::document::Document;
use stemma_core::id::Slug;
use stemma_core::repository::Side;
use stemma_server::artefacts::Shelf;
use stemma_server::store::{artefacts as index, groups, users};
use support::{Harness, Page};

/// The password every account in these tests uses.
const PASSWORD: &str = "a long enough password";

/// The commit the published line was built from.
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// A line with one central statement.
fn source() -> Document {
    Document::new("Quotient groups", None)
        .with(support::statement("thm:first", "First isomorphism theorem").central(true))
}

/// What the line the tests are about is called.
const LINE: &str = "quotient-groups";

/// What an answer said, as JSON.
fn parsed(page: &Page) -> Value {
    serde_json::from_str(&page.body).expect("an answer in JSON")
}

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

/// Makes the group and publishes one line in it.
async fn a_group_with_a_line(harness: &mut Harness, owner: &str) {
    let owner = owner.to_owned();
    let shelf = Shelf::new(harness.instance.settings.paths.artefacts_dir());

    harness
        .instance
        .write(move |connection| {
            let user = users::by_login(connection, &owner)?.expect("the account");
            let group = groups::create(
                connection,
                &Slug::parse("algebra").unwrap(),
                "Algebra",
                &user.id,
            )?;
            let scope = Scope::of(Side::Shared, &user.id);

            let mut nodes = Nodes::new(group.id.clone(), Side::Shared, COMMIT.to_owned());
            nodes.lines.push(support::digest(&source()));

            let text = nodes.to_text();
            shelf.put(&group.id, &scope, Kind::Nodes, COMMIT, text.as_bytes())?;
            index::record(
                connection,
                &group.id,
                &scope,
                Kind::Nodes,
                COMMIT,
                text.len(),
                &user.id,
            )?;

            Ok(())
        })
        .await
        .unwrap();
}

/// Puts somebody in the group.
async fn add_member(harness: &mut Harness, name: &str) {
    let name = name.to_owned();

    harness
        .instance
        .write(move |connection| {
            let user = users::by_login(connection, &name)?.expect("the account");
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");

            groups::add_member(connection, &group.id, &user.id, GroupRole::Member)?;

            Ok(())
        })
        .await
        .unwrap();
}

/// Registers one machine for an account and answers with its credential.
async fn a_machine(harness: &mut Harness, who: &str, label: &str) -> String {
    let token = harness.credential(who).await;
    let registered = harness
        .post_json_with_token(
            "/api/machines",
            &token,
            &format!(r#"{{"label":"{label}","groups":["algebra"],"agents":[]}}"#),
        )
        .await;

    assert_eq!(registered.status, StatusCode::OK);

    token
}

/// What a machine sends to say which line it means.
fn about() -> String {
    format!(r#"{{"group":"algebra","side":"shared","line":"{LINE}"}}"#)
}

/// Signs in as somebody, taking over the harness's cookie.
async fn sign_in(harness: &mut Harness, name: &str) {
    harness
        .post("/login", &[("login", name), ("password", PASSWORD)])
        .await;
}

#[tokio::test]
async fn the_machine_that_takes_a_line_is_the_one_writing_it() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;

    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    let taken = harness
        .post_json_with_token("/api/claims", &laptop, &about())
        .await;

    assert_eq!(taken.status, StatusCode::OK);
    assert_eq!(parsed(&taken)["standing"].as_str(), Some("writing"));

    let line = harness.get(&format!("/g/algebra/shared/{LINE}")).await;

    assert!(line.says("ana's laptop is writing it"));
    assert!(line.says("It lapses in"));
}

#[tokio::test]
async fn a_line_one_machine_is_writing_is_not_taken_by_the_other() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;

    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    let desktop = a_machine(&mut harness, "ana", "ana's desktop").await;

    harness
        .post_json_with_token("/api/claims", &laptop, &about())
        .await;
    let refused = harness
        .post_json_with_token("/api/claims", &desktop, &about())
        .await;

    assert_eq!(parsed(&refused)["standing"].as_str(), Some("waiting"));
    assert_eq!(parsed(&refused)["label"].as_str(), Some("ana's laptop"));

    // Asking for it back says so on the claim, and says it once.
    let asked = harness
        .post_json_with_token("/api/claims/ask", &desktop, &about())
        .await;

    assert_eq!(asked.status, StatusCode::OK);
    assert_eq!(parsed(&asked)["asked"].as_bool(), Some(true));

    let line = harness.get(&format!("/g/algebra/shared/{LINE}")).await;
    assert!(line.says("Another contributor has asked to write it."));
}

#[tokio::test]
async fn letting_a_line_go_leaves_it_open_to_the_next_machine() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;

    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    let desktop = a_machine(&mut harness, "ana", "ana's desktop").await;

    harness
        .post_json_with_token("/api/claims", &laptop, &about())
        .await;
    harness
        .post_json_with_token("/api/claims/release", &laptop, &about())
        .await;

    let taken = harness
        .post_json_with_token("/api/claims", &desktop, &about())
        .await;

    assert_eq!(parsed(&taken)["standing"].as_str(), Some("writing"));
}

#[tokio::test]
async fn renewal_does_not_take_a_released_line_again() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;

    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    harness
        .post_json_with_token("/api/claims", &laptop, &about())
        .await;
    harness
        .post_json_with_token("/api/claims/release", &laptop, &about())
        .await;

    let renewed = harness
        .post_json_with_token("/api/claims/renew", &laptop, &about())
        .await;
    assert_eq!(renewed.status, StatusCode::NOT_FOUND);

    let claims = parsed(&harness.get_with_token("/api/claims", &laptop).await);
    assert_eq!(claims["claims"].as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn the_owner_can_hand_back_a_line_their_own_machine_is_holding() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;

    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    harness
        .post_json_with_token("/api/claims", &laptop, &about())
        .await;

    let line = harness.get(&format!("/g/algebra/shared/{LINE}")).await;
    let token = form_token(&line);

    let handed = harness
        .post(
            &format!("/g/algebra/shared/{LINE}/claim/release"),
            &[("form_token", &token)],
        )
        .await;

    assert_eq!(handed.status, StatusCode::SEE_OTHER);

    let after = harness.get(&format!("/g/algebra/shared/{LINE}")).await;
    assert!(after.says("It is available to edit."));
}

#[tokio::test]
async fn somebody_elses_line_is_not_theirs_to_hand_back() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;

    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    harness
        .post_json_with_token("/api/claims", &laptop, &about())
        .await;

    register(&mut harness, "bruno").await;
    add_member(&mut harness, "bruno").await;
    sign_in(&mut harness, "bruno").await;

    let line = harness.get(&format!("/g/algebra/shared/{LINE}")).await;

    assert!(line.says("ana's laptop is writing it"));
    assert!(
        !line.says(">Release<"),
        "somebody else's machine is not theirs to put down"
    );

    let refused = harness
        .post(
            &format!("/g/algebra/shared/{LINE}/claim/release"),
            &[("form_token", &form_token(&line))],
        )
        .await;

    assert_eq!(refused.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_machine_outside_the_group_cannot_take_one_of_its_lines() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;

    register(&mut harness, "bruno").await;
    let token = harness.credential("bruno").await;
    harness
        .post_json_with_token(
            "/api/machines",
            &token,
            r#"{"label":"bruno's laptop","agents":[]}"#,
        )
        .await;

    let refused = harness
        .post_json_with_token("/api/claims", &token, &about())
        .await;

    assert_eq!(refused.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_machine_is_told_every_line_being_written_where_it_works() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;

    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    let desktop = a_machine(&mut harness, "ana", "ana's desktop").await;

    harness
        .post_json_with_token("/api/claims", &laptop, &about())
        .await;

    let listed = parsed(&harness.get_with_token("/api/claims", &desktop).await);

    assert_eq!(listed["claims"][0]["line"].as_str(), Some(LINE));
    assert_eq!(listed["claims"][0]["mine"].as_bool(), Some(false));
    assert_eq!(listed["claims"][0]["label"].as_str(), Some("ana's laptop"));
}

#[tokio::test]
async fn handing_back_a_claim_requires_the_sessions_form_token() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;
    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    harness
        .post_json_with_token("/api/claims", &laptop, &about())
        .await;

    let refused = harness
        .post(
            &format!("/g/algebra/shared/{LINE}/claim/release"),
            &[("form_token", "wrong-token")],
        )
        .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    let listed = parsed(&harness.get_with_token("/api/claims", &laptop).await);
    assert_eq!(listed["claims"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn private_claims_are_visible_only_to_their_owner() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;
    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    let private = about().replace("shared", "private");
    assert_eq!(
        harness
            .post_json_with_token("/api/claims", &laptop, &private)
            .await
            .status,
        StatusCode::OK
    );

    register(&mut harness, "bruno").await;
    add_member(&mut harness, "bruno").await;
    let other = a_machine(&mut harness, "bruno", "bruno's laptop").await;
    for endpoint in ["/api/claims/renew", "/api/claims/ask"] {
        assert_eq!(
            harness
                .post_json_with_token(endpoint, &other, &private)
                .await
                .status,
            StatusCode::NOT_FOUND
        );
    }
    let listed = parsed(&harness.get_with_token("/api/claims", &other).await);
    assert!(listed["claims"].as_array().unwrap().is_empty());
    let own = parsed(&harness.get_with_token("/api/claims", &laptop).await);
    assert_eq!(own["claims"].as_array().unwrap().len(), 1);
    let taken = harness
        .post_json_with_token("/api/claims", &other, &private)
        .await;
    assert_eq!(taken.status, StatusCode::OK);
    assert_eq!(parsed(&taken)["standing"], "writing");
    harness
        .post_json_with_token("/api/claims/release", &other, &private)
        .await;
    let own = parsed(&harness.get_with_token("/api/claims", &laptop).await);
    assert_eq!(own["claims"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn renewing_does_not_reveal_a_claim_outside_the_group() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    a_group_with_a_line(&mut harness, "ana").await;
    let laptop = a_machine(&mut harness, "ana", "ana's laptop").await;
    harness
        .post_json_with_token("/api/claims", &laptop, &about())
        .await;
    register(&mut harness, "bruno").await;
    let other = harness.credential("bruno").await;
    harness
        .post_json_with_token(
            "/api/machines",
            &other,
            r#"{"label":"bruno's laptop","agents":[]}"#,
        )
        .await;
    assert_eq!(
        harness
            .post_json_with_token("/api/claims/renew", &other, &about())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

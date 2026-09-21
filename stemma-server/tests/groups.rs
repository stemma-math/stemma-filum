//! Asking for a group, and a maintainer answering.

mod support;

use axum::http::StatusCode;
use stemma_core::access::Grant;
use stemma_server::store::{groups, settings, users};
use support::{Harness, Page};

/// The password every account in these tests uses.
const PASSWORD: &str = "a long enough password";

/// Registers somebody and leaves them signed in.
async fn register(harness: &mut Harness, name: &str) -> Page {
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
        .await
}

/// The hidden field a form has to send back.
fn form_token(page: &Page) -> String {
    let marker = "name=\"form_token\" value=\"";
    let start = page.body.find(marker).expect("a form token") + marker.len();
    let rest = &page.body[start..];

    rest[..rest.find('"').expect("the end of the token")].to_owned()
}

/// Makes the signed-in account a maintainer and proves who they are.
async fn become_maintainer(harness: &mut Harness, name: &str) {
    let name = name.to_owned();
    harness
        .instance
        .write(move |connection| {
            let user = users::by_login(connection, &name)?.expect("the account");
            users::grant(connection, &user.id, Grant::Maintainer, None)?;

            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn somebody_asks_for_a_group_and_a_maintainer_makes_it() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;

    let form = harness.get("/groups/new").await;
    assert!(form.says("Request a research group"));

    let asked = harness
        .post(
            "/groups/new",
            &[
                ("name", "Algebra"),
                ("slug", "algebra"),
                ("reason", "Six of us work on it."),
                ("form_token", &form_token(&form)),
            ],
        )
        .await;

    assert_eq!(asked.status, StatusCode::SEE_OTHER);

    let home = harness.get("/").await;
    assert!(home.says("Your group requests"));
    assert!(home.says("waiting for a maintainer"));

    // A second person, who operates the instance, answers it.
    harness.post("/logout", &[]).await;
    register(&mut harness, "enric").await;
    become_maintainer(&mut harness, "enric").await;

    let admin = harness.get("/admin").await;
    assert!(admin.says("Group requests · 1"));
    assert!(admin.says("Six of us work on it."));

    let id = harness
        .instance
        .read(|connection| Ok(groups::open_requests(connection)?[0].id.clone()))
        .await
        .unwrap();

    let approved = harness
        .post(
            &format!("/admin/requests/{id}/approve"),
            &[("note", ""), ("form_token", &form_token(&admin))],
        )
        .await;

    assert_eq!(approved.status, StatusCode::SEE_OTHER);
    assert!(
        harness
            .get("/admin")
            .await
            .says("There are no pending group requests")
    );

    // The group belongs to whoever asked for it, not to whoever approved.
    harness.post("/logout", &[]).await;
    harness
        .post("/login", &[("login", "ana"), ("password", PASSWORD)])
        .await;

    let home = harness.get("/").await;
    assert!(home.says("Algebra"));
    assert!(home.says("grp:algebra"));
    assert!(home.says("owner"));
}

#[tokio::test]
async fn a_declined_request_says_why_and_creates_nothing() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;

    let form = harness.get("/groups/new").await;
    harness
        .post(
            "/groups/new",
            &[
                ("name", "Algebra"),
                ("slug", "algebra"),
                ("reason", "Because."),
                ("form_token", &form_token(&form)),
            ],
        )
        .await;

    harness.post("/logout", &[]).await;
    register(&mut harness, "enric").await;
    become_maintainer(&mut harness, "enric").await;

    let admin = harness.get("/admin").await;
    let id = harness
        .instance
        .read(|connection| Ok(groups::open_requests(connection)?[0].id.clone()))
        .await
        .unwrap();

    harness
        .post(
            &format!("/admin/requests/{id}/decline"),
            &[
                ("note", "There is already a group for this."),
                ("form_token", &form_token(&admin)),
            ],
        )
        .await;

    harness.post("/logout", &[]).await;
    harness
        .post("/login", &[("login", "ana"), ("password", PASSWORD)])
        .await;

    let home = harness.get("/").await;
    assert!(home.says("declined"));
    assert!(home.says("There is already a group for this."));

    let made = harness.instance.read(groups::all).await.unwrap();
    assert!(made.is_empty(), "a declined request made a group");
}

#[tokio::test]
async fn a_name_another_group_holds_is_refused_before_anybody_reads_the_request() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;

    harness
        .instance
        .write(|connection| {
            let ana = users::by_login(connection, "ana")?.expect("the account");
            groups::create(
                connection,
                &stemma_core::id::Slug::parse("algebra").unwrap(),
                "Algebra",
                &ana.id,
            )?;

            Ok(())
        })
        .await
        .unwrap();

    let form = harness.get("/groups/new").await;
    let asked = harness
        .post(
            "/groups/new",
            &[
                ("name", "Algebra again"),
                ("slug", "algebra"),
                ("reason", "Because."),
                ("form_token", &form_token(&form)),
            ],
        )
        .await;

    assert_eq!(asked.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(asked.says("is already a group here"));
}

#[tokio::test]
async fn the_admin_area_is_not_open_to_somebody_who_does_not_operate_the_instance() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;

    let page = harness.get("/admin").await;

    assert_eq!(page.status, StatusCode::FORBIDDEN);
    assert!(page.says("for the people who operate this instance"));
}

#[tokio::test]
async fn the_admin_area_asks_for_the_password_again_when_the_session_is_old() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    become_maintainer(&mut harness, "ana").await;

    harness
        .instance
        .write(|connection| {
            connection.execute(
                "UPDATE sessions SET reauthenticated_at = datetime('now', '-2 hours')",
                [],
            )?;

            Ok(())
        })
        .await
        .unwrap();

    let sent_away = harness.get("/admin").await;
    assert_eq!(sent_away.status, StatusCode::SEE_OTHER);
    assert_eq!(
        sent_away.location.as_deref(),
        Some("/admin/authenticate?next=/admin")
    );

    let form = harness.get("/admin/authenticate?next=/admin").await;
    assert!(form.says("Confirm it is you"));

    let wrong = harness
        .post(
            "/admin/authenticate",
            &[
                ("password", "not the password"),
                ("next", "/admin"),
                ("form_token", &form_token(&form)),
            ],
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);

    let right = harness
        .post(
            "/admin/authenticate",
            &[
                ("password", PASSWORD),
                ("next", "/admin"),
                ("form_token", &form_token(&form)),
            ],
        )
        .await;

    assert_eq!(right.location.as_deref(), Some("/admin"));
    assert!(harness.get("/admin").await.says("Group requests"));
}

#[tokio::test]
async fn a_form_from_somewhere_else_is_refused() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;

    let refused = harness
        .post(
            "/groups/new",
            &[
                ("name", "Algebra"),
                ("slug", "algebra"),
                ("reason", "Because."),
                ("form_token", "a token from another page"),
            ],
        )
        .await;

    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert!(refused.says("out of date"));
}

#[tokio::test]
async fn a_maintainer_names_the_instance_and_the_front_door_says_so() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    become_maintainer(&mut harness, "ana").await;

    let admin = harness.get("/admin").await;

    harness
        .post(
            "/admin/settings",
            &[
                ("name", "Àlgebra UV"),
                ("operator", "Departament d'Àlgebra"),
                ("contact", "algebra@uv.es"),
                ("group_requests_open", "yes"),
                ("invitation_ttl_days", "7"),
                ("form_token", &form_token(&admin)),
            ],
        )
        .await;

    let policy = harness.instance.read(settings::policy).await.unwrap();

    assert_eq!(policy.instance_name.as_deref(), Some("Àlgebra UV"));
    assert!(!policy.registration_open, "an unticked box means no");
    assert!(policy.group_requests_open);
    assert_eq!(policy.invitation_ttl_days, 7);

    harness.post("/logout", &[]).await;
    let door = harness.get("/").await;

    assert!(door.says("Àlgebra UV"));
    assert!(door.says("Managed by Departament d'Àlgebra"));
    assert!(door.says("algebra@uv.es"));
}

#[tokio::test]
async fn an_instance_that_does_not_take_requests_says_so_rather_than_hiding_the_page() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;

    harness
        .instance
        .write(|connection| settings::set(connection, settings::GROUP_REQUESTS_OPEN, "false", None))
        .await
        .unwrap();

    let page = harness.get("/groups/new").await;

    assert_eq!(page.status, StatusCode::OK);
    assert!(page.says("Group requests are closed"));
    assert!(!harness.get("/").await.says("Request a research group"));
}

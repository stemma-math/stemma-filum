//! Getting into a group by way of a link.

mod support;

use axum::http::StatusCode;
use stemma_core::access::GroupRole;
use stemma_core::id::Slug;
use stemma_server::store::{groups, invitations, users};
use support::{Harness, Page};

/// The password every account in these tests uses.
const PASSWORD: &str = "a long enough password";

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

/// The hidden field a form has to send back.
fn form_token(page: &Page) -> String {
    let marker = "name=\"form_token\" value=\"";
    let start = page.body.find(marker).expect("a form token") + marker.len();
    let rest = &page.body[start..];

    rest[..rest.find('"').expect("the end of the token")].to_owned()
}

/// The link out of a page that shows one.
fn link(page: &Page) -> String {
    let marker = "<p class=\"secret\">";
    let start = page.body.find(marker).expect("a link") + marker.len();
    let rest = &page.body[start..];

    rest[..rest.find('<').expect("the end of the link")].to_owned()
}

/// The path part of a link, which is what a test can ask for.
fn path_of(link: &str) -> String {
    link.trim_start_matches("https://stemma.test").to_owned()
}

/// Puts the signed-in account in a group as its owner.
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

#[tokio::test]
async fn a_link_creates_the_account_and_the_membership_in_one_step() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let space = harness.get("/g/algebra").await;
    let asking = harness.get("/g/algebra/invitations/new").await;
    assert!(space.says(r#"Members<span class="count">1</span>"#));
    assert!(space.says("There are no active invitations."));

    let made = harness
        .post(
            "/g/algebra/invitations",
            &[
                ("email", "enric@example.org"),
                ("role", "member"),
                ("form_token", &form_token(&asking)),
            ],
        )
        .await;

    assert!(made.says("It will not be shown again."));
    let invitation = path_of(&link(&made));
    assert!(invitation.starts_with("/invite/"));

    // Somebody with no account at all opens it.
    harness.post("/logout", &[]).await;

    let offered = harness.get(&invitation).await;
    assert!(offered.says("Join Algebra"));
    assert!(offered.says("enric@example.org"));

    let joined = harness
        .post(
            &format!("{invitation}/register"),
            &[
                ("display_name", "Enric Cosme"),
                ("slug", "enric"),
                ("password", PASSWORD),
            ],
        )
        .await;

    assert_eq!(joined.status, StatusCode::SEE_OTHER);
    assert_eq!(joined.location.as_deref(), Some("/g/algebra"));

    let space = harness.get("/g/algebra").await;
    assert!(space.says(r#"Members<span class="count">2</span>"#));
    assert!(space.says("usr:enric"));

    // The account it created is the one the invitation named.
    let enric = harness
        .instance
        .read(|connection| users::by_login(connection, "enric"))
        .await
        .unwrap()
        .expect("the account");
    assert_eq!(enric.email, "enric@example.org");
}

#[tokio::test]
async fn an_invitation_is_for_one_person_and_then_it_is_spent() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let asking = harness.get("/g/algebra/invitations/new").await;
    let made = harness
        .post(
            "/g/algebra/invitations",
            &[
                ("email", "enric@example.org"),
                ("role", "member"),
                ("form_token", &form_token(&asking)),
            ],
        )
        .await;
    let invitation = path_of(&link(&made));

    harness.post("/logout", &[]).await;
    harness
        .post(
            &format!("{invitation}/register"),
            &[
                ("display_name", "Enric"),
                ("slug", "enric"),
                ("password", PASSWORD),
            ],
        )
        .await;
    harness.post("/logout", &[]).await;

    let spent = harness.get(&invitation).await;

    assert_eq!(spent.status, StatusCode::GONE);
    assert!(spent.says("This link no longer works"));
    assert!(spent.says("already been used"));
}

#[tokio::test]
async fn somebody_who_already_has_an_account_simply_joins() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let asking = harness.get("/g/algebra/invitations/new").await;
    let made = harness
        .post(
            "/g/algebra/invitations",
            &[
                ("email", ""),
                ("role", "admin"),
                ("form_token", &form_token(&asking)),
            ],
        )
        .await;
    let invitation = path_of(&link(&made));

    harness.post("/logout", &[]).await;
    register(&mut harness, "enric").await;

    let offered = harness.get(&invitation).await;
    assert!(offered.says("You are signed in as"));

    let joined = harness
        .post(
            &format!("{invitation}/join"),
            &[("form_token", &form_token(&offered))],
        )
        .await;

    assert_eq!(joined.location.as_deref(), Some("/g/algebra"));

    let role = harness
        .instance
        .read(|connection| {
            let enric = users::by_login(connection, "enric")?.expect("the account");
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");

            groups::role_of(connection, &group.id, &enric.id)
        })
        .await
        .unwrap();

    assert_eq!(role, Some(GroupRole::Admin));
}

#[tokio::test]
async fn a_link_with_no_address_cannot_make_an_account_and_says_so() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let asking = harness.get("/g/algebra/invitations/new").await;
    let made = harness
        .post(
            "/g/algebra/invitations",
            &[
                ("email", ""),
                ("role", "member"),
                ("form_token", &form_token(&asking)),
            ],
        )
        .await;
    let invitation = path_of(&link(&made));

    harness.post("/logout", &[]).await;
    let offered = harness.get(&invitation).await;

    assert!(offered.says("Log in to accept this invitation"));
    assert!(!offered.says("Create the account and join"));
}

#[tokio::test]
async fn a_withdrawn_invitation_stops_working() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let asking = harness.get("/g/algebra/invitations/new").await;
    let made = harness
        .post(
            "/g/algebra/invitations",
            &[
                ("email", "enric@example.org"),
                ("role", "member"),
                ("form_token", &form_token(&asking)),
            ],
        )
        .await;
    let invitation = path_of(&link(&made));

    let id = harness
        .instance
        .read(|connection| {
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");

            Ok(invitations::live_for_group(connection, &group.id)?[0].id)
        })
        .await
        .unwrap();

    let space = harness.get("/g/algebra").await;
    harness
        .post(
            &format!("/g/algebra/invitations/{id}/withdraw"),
            &[("form_token", &form_token(&space))],
        )
        .await;

    harness.post("/logout", &[]).await;
    let withdrawn = harness.get(&invitation).await;

    assert_eq!(withdrawn.status, StatusCode::GONE);
    assert!(withdrawn.says("was withdrawn"));
}

#[tokio::test]
async fn nobody_may_invite_somebody_to_more_than_they_hold() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    // An ordinary member is in the group and cannot invite at all.
    register(&mut harness, "enric").await;
    harness
        .instance
        .write(|connection| {
            let enric = users::by_login(connection, "enric")?.expect("the account");
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");

            groups::add_member(connection, &group.id, &enric.id, GroupRole::Admin)?;

            Ok(())
        })
        .await
        .unwrap();

    let asking = harness.get("/g/algebra/invitations/new").await;
    assert!(asking.says("Admin: invites members"));
    assert!(!asking.says("Owner: can transfer"));

    let refused = harness
        .post(
            "/g/algebra/invitations",
            &[
                ("email", ""),
                ("role", "owner"),
                ("form_token", &form_token(&asking)),
            ],
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert!(refused.says("more than you hold"));
}

#[tokio::test]
async fn a_member_sees_the_group_without_the_invitations() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
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

    let space = harness.get("/g/algebra").await;

    assert!(space.says(r#"Members<span class="count">2</span>"#));
    assert!(!space.says("/invitations/new"));

    let asking = harness.get("/g/algebra/invitations/new").await;
    assert_eq!(asking.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_group_somebody_is_not_in_is_not_there_for_them() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    harness.post("/logout", &[]).await;
    register(&mut harness, "enric").await;

    let page = harness.get("/g/algebra").await;

    assert_eq!(page.status, StatusCode::NOT_FOUND);
    assert!(!page.says("Algebra"));
}

#[tokio::test]
async fn a_link_that_names_nothing_is_nothing() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;

    let page = harness.get("/invite/0123456789abcdef").await;

    assert_eq!(page.status, StatusCode::NOT_FOUND);
}

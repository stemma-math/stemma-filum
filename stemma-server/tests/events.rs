//! Machines hearing what changed, without asking.

mod support;

use serde_json::Value;
use stemma_core::access::GroupRole;
use stemma_core::id::Slug;
use stemma_core::repository::Side;
use stemma_server::store::{claims, groups, users};
use support::Harness;

/// The password every account in these tests uses.
const PASSWORD: &str = "a long enough password";

/// Registers somebody.
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
    harness.cookie = None;
}

/// Makes the group, owned by `owner`.
async fn a_group(harness: &Harness, owner: &str) {
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
        .expect("the group");
}

/// Somebody joins the group.
async fn joins(harness: &Harness, who: &str) {
    let who = who.to_owned();

    harness
        .instance
        .read(move |connection| {
            let user = users::by_login(connection, &who)?.expect("the account");
            groups::add_member(
                connection,
                &"grp:algebra".parse().unwrap(),
                &user.id,
                GroupRole::Member,
            )?;
            Ok(())
        })
        .await
        .expect("a member");
}

/// What one notice says.
fn said(data: &str) -> Value {
    serde_json::from_str(data).expect("a notice in JSON")
}

#[tokio::test]
async fn a_machine_hears_it_is_listening_and_from_where() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    let token = harness.credential("ana").await;

    let heard = harness.listen(&token, None, 1).await;

    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].1, "{}", "the first thing is that it is ready");
    assert!(heard[0].0.is_some(), "and where it is, to come back to");
}

#[tokio::test]
async fn a_machine_that_comes_back_hears_what_it_missed_and_only_its_own() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    register(&mut harness, "bo").await;
    register(&mut harness, "cai").await;
    let ana = harness.credential("ana").await;
    let cai = harness.credential("cai").await;

    let before = harness.listen(&ana, None, 1).await[0].0.expect("a place");

    a_group(&harness, "ana").await;
    joins(&harness, "bo").await;

    // Bo's machine takes a line in the group while ana's is away.
    let bo = harness.credential("bo").await;
    let registered = harness
        .post_json_with_token("/api/machines", &bo, r#"{"label": "Bo's laptop"}"#)
        .await;
    let machine: stemma_core::id::MachineId = said(&registered.body)["machine"]
        .as_str()
        .expect("the machine")
        .parse()
        .expect("an identifier");
    harness
        .instance
        .read(move |connection| {
            let bo = users::by_login(connection, "bo")?.expect("the account");
            claims::take(
                connection,
                &"grp:algebra".parse().unwrap(),
                Side::Shared,
                "quotients",
                &machine,
                &bo.id,
            )?;
            Ok(())
        })
        .await
        .expect("a claim");

    let heard = harness.listen(&ana, Some(before), 4).await;
    let kinds: Vec<(String, String)> = heard
        .iter()
        .skip(1)
        .map(|(_, data)| {
            let notice = said(data);
            (
                notice["kind"].as_str().unwrap_or_default().to_owned(),
                notice["change"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();

    assert_eq!(
        kinds,
        [
            ("group".to_owned(), "created".to_owned()),
            ("group".to_owned(), "joined".to_owned()),
            ("claim".to_owned(), "taken".to_owned()),
        ]
    );
    let ids: Vec<i64> = heard.iter().skip(1).filter_map(|(id, _)| *id).collect();
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "in order: {ids:?}"
    );

    // Cai is in no group, and hears nothing of it.
    let heard = harness.listen(&cai, Some(before), 2).await;
    assert_eq!(heard.len(), 1, "only that it is listening: {heard:?}");
}

#[tokio::test]
async fn a_machine_away_too_long_is_told_to_start_over() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    let ana = harness.credential("ana").await;
    a_group(&harness, "ana").await;
    a_group_again(&harness).await;

    harness
        .instance
        .read(|connection| {
            connection.execute("UPDATE events SET at = datetime('now', '-30 days')", [])?;
            stemma_server::store::events::forget_old(connection)?;
            Ok(())
        })
        .await
        .expect("the old events forgotten");

    let heard = harness.listen(&ana, Some(0), 2).await;

    assert_eq!(heard.len(), 2);
    assert_eq!(said(&heard[1].1)["kind"], "reset");
}

/// Something else happens in the group, so there is more than one event.
async fn a_group_again(harness: &Harness) {
    harness
        .instance
        .read(|connection| {
            let ana = users::by_login(connection, "ana")?.expect("the account");
            groups::describe(connection, &"grp:algebra".parse().unwrap(), "Rings.")?;
            groups::add_member(
                connection,
                &"grp:algebra".parse().unwrap(),
                &ana.id,
                GroupRole::Owner,
            )?;
            Ok(())
        })
        .await
        .expect("more");
}

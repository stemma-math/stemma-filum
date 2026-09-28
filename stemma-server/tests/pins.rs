//! Group administrators choosing the reproducible Lean environment.

mod support;

use axum::http::StatusCode;
use stemma_core::access::GroupRole;
use stemma_core::id::Slug;
use stemma_core::repository::{LEAN_TOOLCHAIN, Manifest};
use stemma_server::git::{self, Repositories};
use stemma_server::repositories;
use stemma_server::store::{groups, users};
use support::Harness;

async fn group_with_member(harness: &Harness) {
    let root = harness.root.join("git");
    harness
        .instance
        .write(move |connection| {
            let owner = users::create(
                connection,
                &Slug::parse("ana").unwrap(),
                "ana@example.org",
                "Ana",
                "hash",
            )?
            .expect("a new owner");
            let member = users::create(
                connection,
                &Slug::parse("bo").unwrap(),
                "bo@example.org",
                "Bo",
                "hash",
            )?
            .expect("a new member");
            let group = groups::create(
                connection,
                &Slug::parse("algebra").unwrap(),
                "Algebra",
                &owner.id,
            )?;
            groups::add_member(connection, &group.id, &member.id, GroupRole::Member)?;
            let repositories = Repositories::new(root);
            repositories::for_both(&repositories, &group, &owner)?;
            repositories::for_member(&repositories, &group, &member)?;
            Ok(())
        })
        .await
        .unwrap();
}

const PINS: &str = r#"{
    "typst": "0.14.0",
    "lean": "4.35.0",
    "mathlib": "v4.35.0",
    "stemma": "v0.2.0",
    "dependencies": [{
        "name": "geometry",
        "git": "https://example.org/geometry.git",
        "rev": "v1.2.3",
        "sub_dir": "lean"
    }]
}"#;

#[tokio::test]
async fn only_an_admin_changes_pins_and_every_private_toolchain_follows() {
    let mut harness = Harness::new();
    group_with_member(&harness).await;
    let owner = harness.credential("ana").await;
    let member = harness.credential("bo").await;
    let before = harness.listen(&owner, None, 1).await[0].0.unwrap();

    let changed = harness
        .post_json_with_token("/api/groups/algebra/pins", &owner, PINS)
        .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);

    let repositories = Repositories::new(harness.root.join("git"));
    let group = "grp:algebra".parse().unwrap();
    let shared = repositories.shared(&group);
    let head = git::head_commit(&shared).unwrap();
    let manifest = Manifest::parse(&git::show(&shared, &head, "stemma.toml").unwrap()).unwrap();
    assert_eq!(manifest.pins.lean.as_deref(), Some("4.35.0"));
    assert_eq!(manifest.pins.package.as_deref(), Some("0.1.0"));
    assert_eq!(manifest.pins.dependencies[0].name, "geometry");
    assert!(
        git::show(&shared, &head, "lakefile.toml")
            .unwrap()
            .contains("https://example.org/geometry.git")
    );

    for user in ["usr:ana", "usr:bo"] {
        let private = repositories.personal(&group, &user.parse().unwrap());
        let head = git::head_commit(&private).unwrap();
        assert_eq!(
            git::show(&private, &head, LEAN_TOOLCHAIN).unwrap(),
            "leanprover/lean4:v4.35.0\n"
        );
    }

    let heard = harness.listen(&owner, Some(before), 2).await;
    assert!(heard[1].1.contains(r#""kind":"library""#));
    assert!(heard[1].1.contains(r#""change":"pins_changed""#));

    let head = git::head_commit(&shared).unwrap();
    let refused = harness
        .post_json_with_token("/api/groups/algebra/pins", &member, PINS)
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(git::head_commit(&shared).as_deref(), Some(head.as_str()));
}

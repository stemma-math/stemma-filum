//! Registering a machine, and the queue it takes work from.

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

/// The commit everything here is about.
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// A line with two central statements and a box between them.
fn source() -> Document {
    Document::new("Quotient groups", None)
        .with(support::statement("def:quotient", "Quotient group"))
        .with(support::statement("thm:first", "First isomorphism theorem").central(true))
        .with(support::statement("lem:kernel", "Kernel"))
        .with(support::statement("thm:second", "Correspondence").central(true))
}

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

/// Puts a line on the instance, as a machine that built it would have.
async fn publish(harness: &mut Harness, who: &str) {
    let who = who.to_owned();
    let shelf = Shelf::new(harness.instance.settings.paths.artefacts_dir());

    harness
        .instance
        .write(move |connection| {
            let user = users::by_login(connection, &who)?.expect("the account");
            let group =
                groups::by_slug(connection, &Slug::parse("algebra").unwrap())?.expect("the group");
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

/// Registers a machine with one agent on it.
async fn offer_a_machine(harness: &mut Harness, token: &str, label: &str) -> String {
    let registered = harness
        .post_json_with_token(
            "/api/machines",
            token,
            &format!(
                r#"{{"label":"{label}",
                     "groups":["algebra"],
                     "agents":[{{"agent":"claude","version":"1.2.3",
                                 "availability":"ready"}},
                               {{"agent":"codex","version":null,
                                 "availability":"not_installed"}}]}}"#
            ),
        )
        .await;

    assert_eq!(registered.status, StatusCode::OK);
    parsed(&registered)["machine"]
        .as_str()
        .expect("a machine")
        .to_owned()
}

#[tokio::test]
async fn signing_a_machine_in_does_not_offer_it_for_work() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let token = harness.credential("ana").await;

    // The credential works, and the machine is still not a node.
    assert_eq!(
        harness.get_with_token("/api/me", &token).await.status,
        StatusCode::OK
    );

    let asked = harness.post_with_token("/api/jobs/take", &token).await;
    assert_eq!(asked.status, StatusCode::FORBIDDEN);
    assert!(asked.says("stemma node init"));
}

#[tokio::test]
async fn a_program_is_refused_in_a_line_and_a_person_in_a_page() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let token = harness.credential("ana").await;
    let refused = harness.get_with_token("/api/machines", &token).await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(
        parsed(&refused)["error"].as_str(),
        Some("This machine is not registered here. Run `stemma node init` on it.")
    );
    assert!(!refused.body.contains('<'));

    // The same refusal, asked for by somebody reading, is still a page.
    let page = harness.get("/machines/nobody").await;
    assert!(page.body.contains("<!DOCTYPE html>") || page.body.contains("<html"));
}

#[tokio::test]
async fn a_machine_registers_once_and_says_what_it_found() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let token = harness.credential("ana").await;
    let id = offer_a_machine(&mut harness, &token, "ana's laptop").await;

    let described = parsed(&harness.get_with_token("/api/machines", &token).await);

    assert_eq!(described["machine"].as_str(), Some(id.as_str()));
    assert_eq!(described["presence"].as_str(), Some("awake"));
    assert_eq!(described["groups"][0].as_str(), Some("algebra"));

    // Every agent is in the answer, including the ones it does not have.
    let agents = described["agents"].as_array().expect("the agents");
    assert_eq!(agents.len(), stemma_core::machine::CATALOGUE.len());
    assert!(
        agents
            .iter()
            .any(|agent| { agent["agent"] == "claude" && agent["availability"] == "ready" })
    );
    assert!(
        agents.iter().any(|agent| {
            agent["agent"] == "gemini" && agent["availability"] == "not_installed"
        })
    );

    // Registering again is the same machine saying so again.
    let again = offer_a_machine(&mut harness, &token, "the laptop").await;
    assert_eq!(again, id);
}

#[tokio::test]
async fn a_group_member_can_list_deliberate_machine_choices() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let token = harness.credential("ana").await;
    let machine = offer_a_machine(&mut harness, &token, "ana's laptop").await;
    let choices = parsed(
        &harness
            .get_with_token("/api/groups/algebra/machines", &token)
            .await,
    );

    assert_eq!(choices["machines"][0]["machine"], machine);
    assert_eq!(choices["machines"][0]["label"], "ana's laptop");
    assert_eq!(choices["machines"][0]["presence"], "awake");
    assert_eq!(choices["machines"][0]["agents"][0]["agent"], "claude");
    assert_eq!(
        choices["machines"][0]["agents"][1]["availability"],
        "not_installed"
    );
}

#[tokio::test]
async fn a_job_launched_from_a_line_is_taken_by_the_machine_that_was_chosen() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, "ana").await;

    let token = harness.credential("ana").await;
    let machine = offer_a_machine(&mut harness, &token, "ana's laptop").await;

    let line = harness.get("/g/algebra/shared/quotient-groups").await;
    assert!(line.says("fill this box"), "no launcher on the box");
    assert!(line.says("look for a counterexample"));
    assert!(line.says("say where the line could go next"));
    assert!(line.says("Claude Code"));

    // An agent the machine does not have is named and shown out of
    // reach, rather than being left off the list.
    assert!(line.says("not installed on this machine"));
    assert!(line.says("launcher__agent--out"));

    let launched = harness
        .post(
            "/g/algebra/shared/quotient-groups/jobs",
            &[
                ("on", &format!("{machine}|claude")),
                ("task", "fill@thm:first"),
                ("commit", COMMIT),
                ("form_token", &form_token(&line)),
            ],
        )
        .await;

    assert_eq!(launched.status, StatusCode::SEE_OTHER);

    let space = harness.get("/g/algebra").await;
    assert!(space.says("ana's laptop"));
    assert!(space.says("fill this box"));
    assert!(space.says("waiting for the machine it was given to"));

    // The machine asks for work and is given it.
    let taken = harness.post_with_token("/api/jobs/take", &token).await;
    assert_eq!(taken.status, StatusCode::OK);

    let handed = parsed(&taken);
    assert_eq!(handed["task"].as_str(), Some("fill@thm:first"));
    assert_eq!(handed["agent"].as_str(), Some("claude"));
    assert_eq!(handed["commit"].as_str(), Some(COMMIT));
    assert_eq!(handed["line"].as_str(), Some("quotient-groups"));

    // And there is nothing else waiting.
    assert_eq!(
        harness
            .post_with_token("/api/jobs/take", &token)
            .await
            .status,
        StatusCode::NO_CONTENT
    );

    let job = handed["job"].as_str().expect("a job");
    let finished = harness
        .post_json_with_token(
            &format!("/api/jobs/{job}/finish"),
            &token,
            r#"{"outcome":"nothing_found","said":"no argument was found"}"#,
        )
        .await;

    assert_eq!(finished.status, StatusCode::OK);

    let space = harness.get("/g/algebra").await;
    assert!(space.says("no argument was found"));
    // Tried and the mathematics did not work is an ordinary outcome: it
    // asks nothing of anybody and is counted nowhere.
    assert!(!space.says("to deal with"));
}

#[tokio::test]
async fn a_machine_that_could_not_try_says_so_on_the_machine() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, "ana").await;

    let token = harness.credential("ana").await;
    let machine = offer_a_machine(&mut harness, &token, "ana's laptop").await;

    let line = harness.get("/g/algebra/shared/quotient-groups").await;
    harness
        .post(
            "/g/algebra/shared/quotient-groups/jobs",
            &[
                ("on", &format!("{machine}|claude")),
                ("task", "fill"),
                ("commit", COMMIT),
                ("form_token", &form_token(&line)),
            ],
        )
        .await;

    let job = parsed(&harness.post_with_token("/api/jobs/take", &token).await)["job"]
        .as_str()
        .expect("a job")
        .to_owned();

    harness
        .post_json_with_token(
            &format!("/api/jobs/{job}/finish"),
            &token,
            r#"{"outcome":"could_not_try","code":"agent_not_authenticated",
                "said":"`claude` is not signed in on this machine"}"#,
        )
        .await;

    // It is counted for the machine's owner, where the account and the
    // plan already live, and nowhere else.
    let home = harness.get("/").await;
    assert!(home.says("1 to deal with"));

    let panel = harness.get(&format!("/machines/{machine}")).await;
    assert_eq!(panel.status, StatusCode::OK);
    assert!(panel.says("is not signed in on this machine"));
    assert!(panel.says("Mark as resolved"));

    // The group is told the typed reason, and the decision stays on the
    // machine: the count travels with its owner in their own bar, and the
    // thing to do about it is offered nowhere else.
    let space = harness.get("/g/algebra").await;
    assert!(space.says("is not signed in on this machine"));
    assert!(!space.says("Mark as resolved"));

    harness
        .post(
            &format!("/machines/{machine}/jobs/{job}/clear"),
            &[("form_token", &form_token(&panel))],
        )
        .await;

    // The decision cleared the count; the outcome stays in the record.
    let home = harness.get("/").await;
    assert!(!home.says("to deal with"));

    let panel = harness.get(&format!("/machines/{machine}")).await;
    assert!(panel.says("is not signed in on this machine"));
}

#[tokio::test]
async fn somebody_elses_machine_is_never_handed_a_job_that_named_mine() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, "ana").await;

    let hers = harness.credential("ana").await;
    let her_machine = offer_a_machine(&mut harness, &hers, "ana's laptop").await;

    harness.post("/logout", &[]).await;
    register(&mut harness, "enric").await;
    add_member(&mut harness, "enric").await;

    let his = harness.credential("enric").await;
    offer_a_machine(&mut harness, &his, "the workstation").await;

    // Enric launches the work on her machine on purpose.
    let line = harness.get("/g/algebra/shared/quotient-groups").await;
    harness
        .post(
            "/g/algebra/shared/quotient-groups/jobs",
            &[
                ("on", &format!("{her_machine}|claude")),
                ("task", "refute@thm:first"),
                ("commit", COMMIT),
                ("form_token", &form_token(&line)),
            ],
        )
        .await;

    // His own machine is free and could run it, and is given nothing.
    assert_eq!(
        harness.post_with_token("/api/jobs/take", &his).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        harness
            .post_with_token("/api/jobs/take", &hers)
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_machine_nobody_chose_takes_work_only_where_the_agent_is_there() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;
    publish(&mut harness, "ana").await;

    let token = harness.credential("ana").await;
    offer_a_machine(&mut harness, &token, "ana's laptop").await;

    let line = harness.get("/g/algebra/shared/quotient-groups").await;
    let ask = |agent: &str, form: &str| {
        (
            format!("anyone|{agent}"),
            "explore".to_owned(),
            form.to_owned(),
        )
    };

    let (on, task, form) = ask("codex", &form_token(&line));
    let refused = harness
        .post(
            "/g/algebra/shared/quotient-groups/jobs",
            &[
                ("on", &on),
                ("task", &task),
                ("commit", COMMIT),
                ("form_token", &form),
            ],
        )
        .await;

    // Asking for an agent nothing here has is allowed: the job waits.
    assert_eq!(refused.status, StatusCode::SEE_OTHER);
    assert_eq!(
        harness
            .post_with_token("/api/jobs/take", &token)
            .await
            .status,
        StatusCode::NO_CONTENT,
        "a machine was handed work for an agent it does not have"
    );

    let line = harness.get("/g/algebra/shared/quotient-groups").await;
    let (on, task, form) = ask("claude", &form_token(&line));
    harness
        .post(
            "/g/algebra/shared/quotient-groups/jobs",
            &[
                ("on", &on),
                ("task", &task),
                ("commit", COMMIT),
                ("form_token", &form),
            ],
        )
        .await;

    assert_eq!(
        harness
            .post_with_token("/api/jobs/take", &token)
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn taking_a_machine_back_stops_its_credential_and_nothing_else() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let token = harness.credential("ana").await;
    let machine = offer_a_machine(&mut harness, &token, "ana's laptop").await;

    let panel = harness.get(&format!("/machines/{machine}")).await;
    harness
        .post(
            &format!("/machines/{machine}/retire"),
            &[("form_token", &form_token(&panel))],
        )
        .await;

    // The machine's credential is gone.
    assert_eq!(
        harness.get_with_token("/api/me", &token).await.status,
        StatusCode::FORBIDDEN
    );

    // Its owner is still signed in in the browser.
    let home = harness.get("/").await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(!home.says("ana's laptop"));
}

#[tokio::test]
async fn one_persons_machines_are_their_own_business() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;
    own_a_group(&mut harness, "ana").await;

    let token = harness.credential("ana").await;
    let machine = offer_a_machine(&mut harness, &token, "ana's laptop").await;

    harness.post("/logout", &[]).await;
    register(&mut harness, "enric").await;

    let panel = harness.get(&format!("/machines/{machine}")).await;
    assert_eq!(panel.status, StatusCode::NOT_FOUND);
}

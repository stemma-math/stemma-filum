//! The one screen where a person signs, or does not sign, an agent's words.

mod support;

use axum::http::StatusCode;
use serde_json::Value;
use stemma_core::id::Slug;
use stemma_core::job::{Choice, Task};
use stemma_core::line::Name;
use stemma_core::proposal::Offer;
use stemma_core::repository::{self, Side};
use stemma_server::store::{groups, jobs, proposals, users};
use support::{Harness, Page};

/// The password every account in these tests uses.
const PASSWORD: &str = "a long enough password";

/// What a line says before the agent touches it.
const STANDS: &str = "= Quotient groups\n\n\
    #question[Does it split?]\n\n\
    #theorem(id: \"thm:main\", central: true)[Every finite group is finite.]\n\n\
    #proof(of: \"thm:main\")[It is.]\n";

/// What the agent offers instead.
const OFFERED: &str = "= Quotient groups\n\n\
    #question[Does it split?]\n\n\
    #theorem(id: \"thm:main\", central: true)[Every finite group is finite.]\n\n\
    #proof(of: \"thm:main\")[By induction on the order of the group.]\n";

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

/// An account, a group with repositories, and a line with a proposal
/// waiting on it.
async fn awaiting(harness: &mut Harness) -> String {
    harness
        .post(
            "/register",
            &[
                ("display_name", "Ana"),
                ("slug", "ana"),
                ("email", "ana@example.org"),
                ("password", PASSWORD),
            ],
        )
        .await;

    let repositories = harness.instance.repositories();

    harness
        .instance
        .write(move |connection| {
            let user = users::by_login(connection, "ana")?.expect("the account");
            let group = groups::create(
                connection,
                &Slug::parse("algebra").unwrap(),
                "Algebra",
                &user.id,
            )?;
            let mine = repositories.personal(&group.id, &user.id);

            stemma_server::git::ensure(
                &mine,
                &repository::Manifest::new(group.id.clone(), Side::Private, "Ana"),
            )?;
            stemma_server::git::start_line(
                &mine,
                &Name::parse("quotient-groups").unwrap(),
                STANDS,
                &stemma_server::git::Author {
                    name: "Ana",
                    email: "ana@example.org",
                },
            )?;

            let commit = stemma_server::git::head_commit(&mine).expect("a branch");
            let job = jobs::launch(
                connection,
                &jobs::Launch {
                    group: group.id.clone(),
                    side: Side::Private,
                    line: Name::parse("quotient-groups").unwrap(),
                    commit,
                    against: None,
                    task: Task::Fill {
                        after: Some("thm:main".parse().unwrap()),
                    },
                    agent: Slug::parse("claude").unwrap(),
                    choice: Choice::Anyone,
                    by: user.id.clone(),
                },
            )?;

            let proposal = proposals::create(
                connection,
                &job,
                &Offer {
                    source: OFFERED.to_owned(),
                    lean_source: None,
                    vendor: "Anthropic".to_owned(),
                    model: "claude-opus-4".to_owned(),
                    prompt_revision: "1".to_owned(),
                },
            )?;

            Ok(proposal.to_string())
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn the_review_screen_shows_what_moved_and_what_may_not_be_touched() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;

    let screen = harness.get(&format!("/proposals/{proposal}")).await;

    assert!(screen.status.is_success());
    assert!(screen.says("1 line added, 1 line taken out"));
    assert!(screen.says("diff__row--added"));
    assert!(screen.says("By induction on the order of the group."));
    assert!(
        screen.says("diff__row--frozen"),
        "the statement the line rests on is marked where it is"
    );

    // The attribution is a record, and it is there in full.
    assert!(screen.says("Anthropic"));
    assert!(screen.says("claude-opus-4"));
    assert!(screen.says("prompt 1"));

    // And what was checked is checked, not asserted.
    assert!(screen.says("the proposed source reads as a line"));
    assert!(screen.says("are untouched"));
}

#[tokio::test]
async fn a_proposal_that_changes_what_the_line_rests_on_says_so() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;
    let moved = OFFERED.replace("Every finite group is finite.", "Every group is finite.");

    harness
        .instance
        .write(move |connection| {
            connection.execute("UPDATE proposals SET source = ?1", rusqlite::params![moved])?;

            Ok(())
        })
        .await
        .unwrap();

    let screen = harness.get(&format!("/proposals/{proposal}")).await;

    assert!(screen.says("changes the text of central statement"));
}

#[tokio::test]
async fn a_machine_reads_and_decides_the_same_proposal() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;
    let token = harness.credential("ana").await;

    let listed = parsed(&harness.get_with_token("/api/proposals", &token).await);
    assert_eq!(
        listed["proposals"][0]["proposal"].as_str(),
        Some(proposal.as_str())
    );
    assert_eq!(
        listed["proposals"][0]["line"].as_str(),
        Some("quotient-groups")
    );

    let offered = parsed(
        &harness
            .get_with_token(&format!("/api/proposals/{proposal}/review"), &token)
            .await,
    );

    assert_eq!(offered["stands"].as_str(), Some(STANDS));
    assert_eq!(offered["source"].as_str(), Some(OFFERED));
    assert_eq!(offered["model"].as_str(), Some("claude-opus-4"));
    assert!(
        offered["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding
                .as_str()
                .is_some_and(|said| said.contains("untouched")))
    );

    let decided = harness
        .post_json_with_token(
            &format!("/api/proposals/{proposal}/decide"),
            &token,
            r#"{"decision":"accept"}"#,
        )
        .await;

    assert_eq!(decided.status, StatusCode::OK);

    // Accepted, so the machine may now write it down.
    let accepted = parsed(
        &harness
            .get_with_token(&format!("/api/proposals/{proposal}"), &token)
            .await,
    );

    assert_eq!(accepted["source"].as_str(), Some(OFFERED));
}

#[tokio::test]
async fn nobody_else_reads_or_decides_somebody_elses_proposal() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;

    harness
        .post(
            "/register",
            &[
                ("display_name", "Bruno"),
                ("slug", "bruno"),
                ("email", "bruno@example.org"),
                ("password", PASSWORD),
            ],
        )
        .await;

    let screen = harness.get(&format!("/proposals/{proposal}")).await;
    assert_eq!(screen.status, StatusCode::NOT_FOUND);

    let token = harness.credential("bruno").await;
    let asked = harness
        .get_with_token(&format!("/api/proposals/{proposal}/review"), &token)
        .await;

    assert_eq!(asked.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn asking_again_declines_this_one_and_puts_the_question_back() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;

    let screen = harness.get(&format!("/proposals/{proposal}")).await;
    harness
        .post(
            &format!("/proposals/{proposal}/decide"),
            &[
                ("form_token", &form_token(&screen)),
                ("decision", "ask_again"),
            ],
        )
        .await;

    let after = harness.get(&format!("/proposals/{proposal}")).await;
    assert!(after.says("This proposal was rejected."));

    let queued =
        harness
            .instance
            .read(|connection| {
                Ok(connection
                    .query_row("SELECT count(*) FROM jobs", [], |row| row.get::<_, i64>(0))?)
            })
            .await
            .unwrap();

    assert_eq!(queued, 2, "the same question is in the queue again");
}

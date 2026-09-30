//! Proposals are read on the web and decided by a signed-in machine.

mod support;

use axum::http::StatusCode;
use serde_json::Value;
use stemma_core::artefact::{BlockCheck, FormalDeclaration};
use stemma_core::document::{Block, Document};
use stemma_core::id::{BlockId, Slug};
use stemma_core::job::{Choice, Task};
use stemma_core::line::Name;
use stemma_core::model::Verification;
use stemma_core::proposal::{
    FormalImpact, FormalReview, LibraryChange, LibraryFileChange, Offer, ReviewedBlock,
};
use stemma_core::repository::{self, Side};
use stemma_server::store::{groups, jobs, proposals, users};
use support::{Harness, Page};

/// The password every account in these tests uses.
const PASSWORD: &str = "a long enough password";

/// What a line says before the agent touches it. Its blocks have fixed
/// identifiers, so the agent's version is the same blocks changed.
fn stands() -> String {
    let mut theorem = Block::theorem(
        "Theorem",
        Some("thm:main".parse().unwrap()),
        "Every finite group is finite.",
    )
    .central(true);
    let id: BlockId = format!("blk:{:026}", 1).parse().unwrap();
    theorem.statement_mut().unwrap().id = id;
    let Block::Proof(mut proof) = Block::proof(id, "It is.") else {
        unreachable!()
    };
    proof.id = format!("blk:{:026}", 2).parse().unwrap();

    Document::new("Quotient groups", Some("Does it split?".to_owned()))
        .with(theorem)
        .with(Block::Proof(proof))
        .to_text()
}

/// What the agent offers instead.
fn proposed() -> String {
    stands().replace("It is.", "By induction on the order of the group.")
}

/// What an answer said, as JSON.
fn parsed(page: &Page) -> Value {
    serde_json::from_str(&page.body).expect("an answer in JSON")
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
                &stands(),
                &stemma_server::git::Author {
                    name: "Ana",
                    email: "ana@example.org",
                },
            )?;

            let line = stemma_server::store::lines::start(
                connection,
                &group.id,
                Side::Private,
                &Name::parse("quotient-groups").unwrap(),
                &user.id,
                &stemma_server::store::revisions::Content {
                    source: stands(),
                    lean_source: None,
                    page: "<p>The page.</p>".to_owned(),
                },
            )?;
            let job = jobs::launch(
                connection,
                &jobs::Launch {
                    group: group.id.clone(),
                    side: Side::Private,
                    line: Name::parse("quotient-groups").unwrap(),
                    line_id: Some(line.id),
                    revision: line.revision,
                    against: None,
                    task: Task::Fill { after: None },
                    agent: Slug::parse("claude").unwrap(),
                    request: None,
                    grants: None,
                    choice: Choice::Anyone,
                    by: user.id.clone(),
                },
            )?;

            let proposal = proposals::create(
                connection,
                &job,
                &Offer {
                    source: proposed(),
                    lean_source: None,
                    library_change: None,
                    formal_review: None,
                    vendor: "Anthropic".to_owned(),
                    model: "claude-opus-4".to_owned(),
                    prompt_revision: "1".to_owned(),
                    operations: Vec::new(),
                    explanation: String::new(),
                    effort: None,
                },
            )?;

            Ok(proposal.to_string())
        })
        .await
        .unwrap()
}

fn main_block() -> BlockId {
    format!("blk:{:026}", 1).parse().unwrap()
}

fn checked(closure: &str) -> BlockCheck {
    BlockCheck {
        block: main_block(),
        state: Verification::Verified,
        diagnostic: String::new(),
        declarations: vec![FormalDeclaration {
            name: "Algebra.main".to_owned(),
            module: "Algebra.Quotients".to_owned(),
            statement: "∀ G, Finite G → Finite G".to_owned(),
            sorry_free: true,
            axioms: Vec::new(),
            reaches: vec!["Algebra.helper".to_owned()],
            statement_closure: closure.to_owned(),
        }],
    }
}

async fn with_formal_review(harness: &Harness, proposal: &str) {
    let proposal = proposal.to_owned();
    harness
        .instance
        .write(move |connection| {
            let review = FormalReview {
                blocks: vec![ReviewedBlock {
                    check: checked("closure-one"),
                    reading: "Every finite group is finite.".to_owned(),
                }],
                impact: FormalImpact {
                    proofs_only: false,
                    statement_closures: vec![main_block()],
                    line_links: vec![main_block()],
                },
            };
            connection.execute(
                "UPDATE proposals SET formal_review = ?2 WHERE id = ?1",
                rusqlite::params![proposal, serde_json::to_string(&review)?],
            )?;
            connection.execute(
                "UPDATE jobs SET task = ?1 WHERE id = (SELECT job_id FROM proposals WHERE id = ?2)",
                rusqlite::params![format!("formalize@{}", main_block()), proposal],
            )?;
            Ok(())
        })
        .await
        .unwrap();
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
        screen.says("diff__row--held-first"),
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
async fn a_library_change_is_shown_file_by_file_before_a_decision() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;
    let id = proposal.clone();
    harness
        .instance
        .write(move |connection| {
            let change = LibraryChange {
                base: "0123456789012345678901234567890123456789".to_owned(),
                files: vec![LibraryFileChange {
                    path: "Algebra/Quotients.lean".to_owned(),
                    before: Some("theorem old : True := by trivial\n".to_owned()),
                    after: Some("theorem new : True := by trivial\n".to_owned()),
                }],
                readings: Default::default(),
            };
            connection.execute(
                "UPDATE proposals SET library_change = ?2 WHERE id = ?1",
                rusqlite::params![id, serde_json::to_string(&change)?],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let screen = harness.get(&format!("/proposals/{proposal}")).await;
    assert!(screen.says("Lean library changes"));
    assert!(screen.says("Algebra/Quotients.lean"));
    assert!(screen.says("theorem old"));
    assert!(screen.says("theorem new"));
}

#[tokio::test]
async fn formal_review_shows_reach_and_acceptance_records_a_withdrawable_audit() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;
    with_formal_review(&harness, &proposal).await;

    let screen = harness.get(&format!("/proposals/{proposal}")).await;
    assert!(screen.says("Formal reach"));
    assert!(screen.says("∀ G, Finite G → Finite G"));
    assert!(screen.says("Agent reading:"));
    assert!(screen.says("Links of the line changed"));

    let token = harness.credential("ana").await;
    let decided = harness
        .post_json_with_token(
            &format!("/api/proposals/{proposal}/decide"),
            &token,
            r#"{"decision":"accept"}"#,
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK);

    let (line, audit) = harness
        .instance
        .read(|connection| {
            let line: String =
                connection.query_row("SELECT line_id FROM formal_audits LIMIT 1", [], |row| {
                    row.get(0)
                })?;
            let line = line.parse()?;
            let audit = stemma_server::store::formal_audits::for_line(connection, &line)?
                .pop()
                .expect("the accepted audit");
            Ok((line, audit))
        })
        .await
        .unwrap();
    assert_eq!(audit.state, "audited");
    assert_eq!(audit.reading, "Every finite group is finite.");

    harness
        .instance
        .read(move |connection| {
            assert!(stemma_server::store::formal_audits::reconcile_check(
                connection,
                &line,
                &checked("closure-two"),
            )?);
            Ok(())
        })
        .await
        .unwrap();

    let withdrawn = harness
        .instance
        .read(|connection| {
            Ok(connection.query_row(
                "SELECT state, withdrawn_reason FROM formal_audits LIMIT 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(withdrawn.0, "audit_withdrawn");
    assert!(withdrawn.1.contains("statement closure"));
}

#[tokio::test]
async fn changing_a_central_blocks_prose_withdraws_its_formal_audit() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;
    with_formal_review(&harness, &proposal).await;
    let token = harness.credential("ana").await;
    harness
        .post_json_with_token(
            &format!("/api/proposals/{proposal}/decide"),
            &token,
            r#"{"decision":"accept"}"#,
        )
        .await;

    harness
        .instance
        .read(|connection| {
            let line: String =
                connection.query_row("SELECT line_id FROM formal_audits LIMIT 1", [], |row| {
                    row.get(0)
                })?;
            let changed = stands().replace(
                "Every finite group is finite.",
                "Every finite group remains finite.",
            );
            let withdrawn = stemma_server::store::formal_audits::withdraw_changed_prose(
                connection,
                &line.parse()?,
                &changed,
            )?;
            assert_eq!(withdrawn, [main_block()]);
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn asking_again_after_a_formal_realignment_asks_for_the_authors_route() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;
    with_formal_review(&harness, &proposal).await;

    let screen = harness.get(&format!("/proposals/{proposal}")).await;
    assert!(screen.says("Open in Stemma to decide"));
    let token = harness.credential("ana").await;
    let answer = harness
        .post_json_with_token(
            &format!("/api/proposals/{proposal}/decide"),
            &token,
            r#"{"decision":"ask_again","note":"Follow the author's route."}"#,
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK);

    let task = harness
        .instance
        .read(|connection| {
            Ok(connection.query_row(
                "SELECT task FROM jobs ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(task, format!("realign@{}", main_block()));
}

#[tokio::test]
async fn a_proposal_that_changes_what_the_line_rests_on_says_so() {
    let mut harness = Harness::new();
    let proposal = awaiting(&mut harness).await;
    let moved = proposed().replace("Every finite group is finite.", "Every group is finite.");

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

    assert_eq!(offered["stands"].as_str(), Some(stands().as_str()));
    assert_eq!(offered["source"].as_str(), Some(proposed().as_str()));
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

    assert_eq!(accepted["source"].as_str(), Some(proposed().as_str()));
    let recent = parsed(&harness.get_with_token("/api/proposals", &token).await);
    assert_eq!(recent["proposals"].as_array().unwrap().len(), 0);
    assert_eq!(
        recent["recent"][0]["proposal"].as_str(),
        Some(proposal.as_str())
    );
    let history = parsed(
        &harness
            .get_with_token(
                "/api/proposals/history/algebra/private/quotient-groups",
                &token,
            )
            .await,
    );
    assert_eq!(
        history["proposals"][0]["decided_by"].as_str(),
        Some("usr:ana")
    );
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
    assert!(screen.says("Open in Stemma to decide"));
    let web_decision = harness
        .post(
            &format!("/proposals/{proposal}/decide"),
            &[("decision", "accept")],
        )
        .await;
    assert_eq!(web_decision.status, StatusCode::METHOD_NOT_ALLOWED);
    let token = harness.credential("ana").await;
    let answer = harness
        .post_json_with_token(
            &format!("/api/proposals/{proposal}/decide"),
            &token,
            r#"{"decision":"ask_again","note":"Try a shorter argument."}"#,
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK);

    let after = harness.get(&format!("/proposals/{proposal}")).await;
    assert!(after.says("This proposal was rejected."));
    assert!(after.says("Try a shorter argument."));

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
    let request = harness
        .instance
        .read(|connection| {
            Ok(connection.query_row(
                "SELECT request FROM jobs ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )?)
        })
        .await
        .unwrap();
    assert!(request.contains("Try a shorter argument."));
    assert!(request.contains(&proposed()));
}

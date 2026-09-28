//! Publishing immutable source and permanent documentation pages.

mod support;

use axum::http::StatusCode;
use stemma_core::artefact::{BlockCheck, FormalDeclaration};
use stemma_core::document::{Block, Document};
use stemma_core::id::{BlockId, Slug};
use stemma_core::job::{Choice, Task};
use stemma_core::line::Name;
use stemma_core::model::Verification;
use stemma_core::proposal::{FormalImpact, FormalReview, Offer, ReviewedBlock};
use stemma_core::repository::{self, Side};
use stemma_server::git::{self, Author, LibraryWritten};
use stemma_server::store::{formal_audits, groups, jobs, lines, proposals, users};
use support::{Harness, Page};

const PASSWORD: &str = "a long enough password";

fn central_block() -> BlockId {
    format!("blk:{:026}", 1).parse().unwrap()
}

fn source() -> String {
    let mut theorem = Block::theorem(
        "Theorem",
        Some("thm:main".parse().unwrap()),
        "Every finite group is finite.",
    )
    .central(true)
    .titled("Finiteness theorem");
    theorem.statement_mut().unwrap().id = central_block();
    Document::new(
        "Quotient groups",
        Some("A line about quotients.".to_owned()),
    )
    .with(theorem)
    .to_text()
}

fn form_token(page: &Page) -> String {
    let marker = "name=\"form_token\" value=\"";
    let start = page.body.find(marker).expect("a form token") + marker.len();
    let rest = &page.body[start..];
    rest[..rest.find('"').expect("the end of the token")].to_owned()
}

async fn group_ready_to_release(harness: &mut Harness, audited: bool) {
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
            let user = users::by_login(connection, "ana")?.expect("the owner");
            let group = groups::create(
                connection,
                &Slug::parse("algebra").unwrap(),
                "Algebra",
                &user.id,
            )?;
            let shared = repositories.shared(&group.id);
            git::ensure(
                &shared,
                &repository::Manifest::new(group.id.clone(), Side::Shared, "Algebra"),
            )?;
            let author = Author {
                name: "Ana",
                email: "ana@example.org",
            };
            let name = Name::parse("quotient-groups").unwrap();
            git::start_line(&shared, &name, &source(), &author)?;
            let base = git::head_commit(&shared).unwrap();
            let helper = "import Stemma\n\nprivate theorem hiddenHelper : True := by trivial\n\n@[stemma \"quotient-groups/blk:00000000000000000000000001\"]\ntheorem Algebra.main : True := by trivial\n";
            assert!(matches!(
                git::write_library(
                    &shared,
                    &base,
                    &[stemma_core::proposal::LibraryFileChange {
                        path: "Algebra/Quotients.lean".to_owned(),
                        before: None,
                        after: Some(helper.to_owned()),
                    }],
                    &author,
                    "Add the formalization",
                )?,
                LibraryWritten::Taken(_)
            ));

            let line = lines::start(
                connection,
                &group.id,
                Side::Shared,
                &name,
                &user.id,
                &stemma_server::store::revisions::Content {
                    source: source(),
                    lean_source: None,
                    page: "<p>The theorem.</p>".to_owned(),
                },
            )?;
            if audited {
                let job = jobs::launch(
                    connection,
                    &jobs::Launch {
                        group: group.id.clone(),
                        side: Side::Shared,
                        line: name,
                        line_id: Some(line.id),
                        revision: line.revision,
                        against: None,
                        task: Task::Formalize {
                            block: central_block(),
                        },
                        agent: Slug::parse("codex").unwrap(),
                        model: None,
                        effort: None,
                        request: None,
                        grants: None,
                        choice: Choice::Anyone,
                        by: user.id.clone(),
                    },
                )?;
                let check = BlockCheck {
                    block: central_block(),
                    state: Verification::Verified,
                    diagnostic: String::new(),
                    declarations: vec![FormalDeclaration {
                        name: "Algebra.main".to_owned(),
                        module: "Algebra.Quotients".to_owned(),
                        statement: "Algebra.main : True".to_owned(),
                        sorry_free: true,
                        axioms: Vec::new(),
                        reaches: vec!["Algebra.hiddenHelper".to_owned()],
                        statement_closure: "closure".to_owned(),
                    }],
                };
                let proposal = proposals::create(
                    connection,
                    &job,
                    &Offer {
                        source: source(),
                        lean_source: None,
                        library_change: None,
                        formal_review: Some(Box::new(FormalReview {
                            blocks: vec![ReviewedBlock {
                                check,
                                reading: "Every finite group is finite.".to_owned(),
                            }],
                            impact: FormalImpact {
                                proofs_only: false,
                                statement_closures: vec![central_block()],
                                line_links: Vec::new(),
                            },
                        })),
                        vendor: "OpenAI".to_owned(),
                        model: "codex".to_owned(),
                        prompt_revision: "1".to_owned(),
                        operations: Vec::new(),
                        explanation: String::new(),
                        effort: None,
                    },
                )?;
                let proposal = proposals::by_id(connection, &proposal)?.unwrap();
                formal_audits::accept(connection, &proposal, &user.id)?;
            }
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn an_audited_library_gets_public_documentation_and_permanent_result_pages() {
    let mut harness = Harness::new();
    group_ready_to_release(&mut harness, true).await;
    let form = harness.get("/g/algebra/releases").await;
    let published = harness
        .post(
            "/g/algebra/releases",
            &[("version", "1.0.0"), ("form_token", &form_token(&form))],
        )
        .await;
    assert_eq!(published.status, StatusCode::SEE_OTHER);
    assert_eq!(
        published.location.as_deref(),
        Some("/releases/algebra/1.0.0")
    );

    harness.post("/logout", &[]).await;
    let docs = harness.get("/releases/algebra/1.0.0").await;
    assert!(docs.status.is_success());
    assert!(docs.says("Finiteness theorem"));
    assert!(docs.says("rev = &quot;v1.0.0&quot;"));
    assert!(
        !docs.says("hiddenHelper"),
        "helpers leaked into documentation"
    );

    let result = harness
        .get(&format!(
            "/releases/algebra/1.0.0/quotient-groups/{}",
            central_block()
        ))
        .await;
    assert!(result.says("Every finite group is finite."));
    assert!(result.says("Algebra.main : True"));
    assert!(result.says("Permanent result id"));
    assert!(
        !result.says("hiddenHelper"),
        "helpers leaked into a result page"
    );

    let source = harness
        .get("/releases/algebra/1.0.0/source/Algebra/Quotients.lean")
        .await;
    assert!(
        source.says("hiddenHelper"),
        "the helper was removed from source"
    );
}

#[tokio::test]
async fn a_central_result_without_a_current_formal_audit_blocks_publication() {
    let mut harness = Harness::new();
    group_ready_to_release(&mut harness, false).await;
    let form = harness.get("/g/algebra/releases").await;
    let refused = harness
        .post(
            "/g/algebra/releases",
            &[("version", "1.0.0"), ("form_token", &form_token(&form))],
        )
        .await;

    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert!(refused.says("Every central result must have a current"));
    assert_eq!(
        harness.get("/releases/algebra/1.0.0").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn an_audit_of_different_prose_cannot_document_the_released_source() {
    let mut harness = Harness::new();
    group_ready_to_release(&mut harness, true).await;
    harness
        .instance
        .read(|connection| {
            connection.execute("UPDATE formal_audits SET block_source = '{}'", [])?;
            Ok(())
        })
        .await
        .unwrap();
    let form = harness.get("/g/algebra/releases").await;
    let refused = harness
        .post(
            "/g/algebra/releases",
            &[("version", "1.0.0"), ("form_token", &form_token(&form))],
        )
        .await;

    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert!(refused.says("Every central result must have a current"));
}

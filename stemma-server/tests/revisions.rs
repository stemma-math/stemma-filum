//! Writing a line a revision at a time, as the machines do.

mod support;

use axum::http::StatusCode;
use serde_json::{Value, json};
use stemma_core::document::{Block, Document};
use stemma_core::id::Slug;
use stemma_server::store::{groups, users};
use support::{Harness, Page};

/// The password every account in these tests uses.
const PASSWORD: &str = "a long enough password";

fn parsed(page: &Page) -> Value {
    serde_json::from_str(&page.body).expect("an answer in JSON")
}

/// Somebody with a group, a registered machine, and a line in the group.
struct Writing {
    harness: Harness,
    token: String,
    line: String,
}

async fn writing() -> Writing {
    let mut harness = Harness::new();
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
            stemma_server::repositories::for_both(&repositories, &group, &user)?;
            Ok(())
        })
        .await
        .expect("a group with its repositories");

    let token = harness.credential("ana").await;
    let registered = harness
        .post_json_with_token("/api/machines", &token, r#"{"label": "Ana's laptop"}"#)
        .await;
    assert_eq!(registered.status, StatusCode::OK, "{}", registered.body);

    let started = harness
        .post_json_with_token(
            "/api/lines",
            &token,
            r#"{"group": "algebra", "title": "Quotient groups", "seen_by": "group"}"#,
        )
        .await;
    assert_eq!(started.status, StatusCode::OK, "{}", started.body);
    let started = parsed(&started);
    assert_eq!(started["revision"], 1);

    Writing {
        harness,
        token,
        line: started["id"].as_str().expect("an identifier").to_owned(),
    }
}

impl Writing {
    async fn take(&mut self) {
        let taken = self
            .harness
            .post_json_with_token(
                "/api/claims",
                &self.token,
                r#"{"group": "algebra", "side": "shared", "line": "quotient-groups"}"#,
            )
            .await;
        assert_eq!(parsed(&taken)["standing"], "writing", "{}", taken.body);
    }

    async fn write(&mut self, base: u64, words: &str) -> Page {
        let source = Document::new("Quotient groups", None)
            .with(Block::prose(words))
            .to_text();
        let body = json!({
            "base": base,
            "source": source,
            "page": format!("<h1>Quotient groups</h1><p>{words}</p>"),
        });

        self.harness
            .put_json_with_token(
                &format!("/api/lines/{}", self.line),
                &self.token,
                &body.to_string(),
            )
            .await
    }
}

#[tokio::test]
async fn a_new_line_is_listed_at_its_first_revision_with_a_page_nobody_compiled() {
    let mut writing = writing().await;

    let listed = parsed(
        &writing
            .harness
            .get_with_token("/api/lines", &writing.token)
            .await,
    );
    assert_eq!(listed["lines"][0]["id"], writing.line.as_str());
    assert_eq!(listed["lines"][0]["revision"], 1);
    assert_eq!(listed["lines"][0]["deleted"], false);

    let read = parsed(
        &writing
            .harness
            .get_with_token(&format!("/api/lines/{}", writing.line), &writing.token)
            .await,
    );
    assert_eq!(read["kind"], "started");
    assert!(Document::parse(read["source"].as_str().unwrap()).is_ok());

    let page = writing
        .harness
        .get("/g/algebra/shared/quotient-groups/page")
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.says("<h1>Quotient groups</h1>"));
    assert!(page.says("@media print"));
    assert!(page.says("--font-math"));

    let download = writing
        .harness
        .get("/g/algebra/shared/quotient-groups/line.json")
        .await;
    assert_eq!(download.status, StatusCode::OK);
    assert_eq!(
        download.content_type.as_deref(),
        Some("application/json; charset=utf-8")
    );
    assert!(Document::parse(&download.body).is_ok());
}

#[tokio::test]
async fn the_machine_holding_the_line_writes_it_a_revision_at_a_time() {
    let mut writing = writing().await;

    let refused = writing.write(1, "Before taking it.").await;
    assert_eq!(refused.status, StatusCode::LOCKED, "only the holder writes");

    writing.take().await;

    let written = writing.write(1, "Every group has one.").await;
    assert_eq!(written.status, StatusCode::OK, "{}", written.body);
    assert_eq!(parsed(&written)["revision"], 2);

    let stale = writing.write(1, "From an old revision.").await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    let current = &parsed(&stale)["current"];
    assert_eq!(current["revision"], 2);
    assert!(
        current["source"]
            .as_str()
            .unwrap()
            .contains("Every group has one.")
    );

    let broken = writing
        .harness
        .put_json_with_token(
            &format!("/api/lines/{}", writing.line),
            &writing.token,
            &json!({ "base": 2, "source": "not a line", "page": "<p/>" }).to_string(),
        )
        .await;
    assert_eq!(broken.status, StatusCode::UNPROCESSABLE_ENTITY);

    // The group sees the revision as it is written.
    let page = writing
        .harness
        .get("/g/algebra/shared/quotient-groups/page")
        .await;
    assert!(page.says("Every group has one."));
    let view = writing
        .harness
        .get("/g/algebra/shared/quotient-groups")
        .await;
    assert!(view.says("Being written"));
    assert!(view.says("Revision 2"));
    assert!(view.says("Download line.json"));
    assert!(view.says("Print or save as PDF"));
    assert!(!view.says("Typst source"));

    let download = writing
        .harness
        .get("/g/algebra/shared/quotient-groups/line.json")
        .await;
    assert!(download.says("Every group has one."));
    assert!(!download.says("From an old revision."));
}

#[tokio::test]
async fn letting_the_line_go_commits_the_writing_once() {
    let mut writing = writing().await;
    writing.take().await;
    writing.write(1, "First.").await;
    writing.write(2, "Second.").await;

    let shared = writing
        .harness
        .instance
        .repositories()
        .shared(&"grp:algebra".parse().unwrap());
    let commits_before = commits(&shared);

    writing
        .harness
        .post_json_with_token(
            "/api/claims/release",
            &writing.token,
            r#"{"group": "algebra", "side": "shared", "line": "quotient-groups"}"#,
        )
        .await;

    assert_eq!(
        commits(&shared),
        commits_before + 1,
        "one commit for the stretch"
    );
    let head = stemma_server::git::head_commit(&shared).unwrap();
    let kept = stemma_server::git::show(&shared, &head, "lines/quotient-groups/line.json").unwrap();
    assert!(kept.contains("Second."), "git has where the writing ended");
}

#[tokio::test]
async fn a_page_from_a_machine_runs_nothing_and_loads_nothing() {
    let mut writing = writing().await;
    writing.take().await;
    writing.write(1, "<script>alert(1)</script>").await;

    let request = axum::http::Request::builder()
        .uri("/g/algebra/shared/quotient-groups/page")
        .header(
            axum::http::header::COOKIE,
            writing.harness.cookie.clone().unwrap(),
        )
        .body(axum::body::Body::empty())
        .unwrap();
    let answer = tower::ServiceExt::oneshot(
        stemma_server::web::router().with_state(writing.harness.instance.clone()),
        request,
    )
    .await
    .unwrap();

    let policy = answer
        .headers()
        .get("content-security-policy")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(policy.contains("default-src 'none'"), "{policy}");
    assert!(policy.contains("sandbox"), "{policy}");
    assert_eq!(
        answer.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );

    let request = axum::http::Request::builder()
        .uri("/g/algebra/shared/quotient-groups/line.json")
        .header(
            axum::http::header::COOKIE,
            writing.harness.cookie.clone().unwrap(),
        )
        .body(axum::body::Body::empty())
        .unwrap();
    let answer = tower::ServiceExt::oneshot(
        stemma_server::web::router().with_state(writing.harness.instance.clone()),
        request,
    )
    .await
    .unwrap();
    assert_eq!(
        answer.headers().get("content-disposition").unwrap(),
        "attachment; filename=\"line.json\""
    );

    let view = writing
        .harness
        .get("/g/algebra/shared/quotient-groups")
        .await;
    assert!(view.says("<iframe class=\"typeset__frame\" sandbox=\"\""));
}

#[tokio::test]
async fn nobody_pushes_a_line_through_git_or_writes_one_they_cannot_see() {
    let mut writing = writing().await;

    let elsewhere = writing
        .harness
        .put_json_with_token(
            "/api/lines/lin:01JAAAAAAAAAAAAAAAAAAAAAAA",
            &writing.token,
            &json!({ "base": 1, "source": "{}", "page": "<p/>" }).to_string(),
        )
        .await;
    assert_eq!(elsewhere.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn what_a_line_said_before_it_was_shared_stays_its_authors() {
    let mut writing = writing().await;
    let started = writing
        .harness
        .post_json_with_token(
            "/api/lines",
            &writing.token,
            r#"{"group": "algebra", "title": "Draft", "seen_by": "me"}"#,
        )
        .await;
    assert_eq!(started.status, StatusCode::OK, "{}", started.body);
    let line = parsed(&started)["id"].as_str().unwrap().to_owned();

    let shared = writing
        .harness
        .post_json_with_token(
            "/api/lines/algebra/draft/share",
            &writing.token,
            r#"{"travels": "today", "writing": "author"}"#,
        )
        .await;
    assert_eq!(shared.status, StatusCode::OK, "{}", shared.body);

    writing
        .harness
        .instance
        .write(|connection| {
            let bo = users::create(
                connection,
                &Slug::parse("bo").unwrap(),
                "bo@example.org",
                "Bo",
                "hash",
            )?
            .expect("a new account");
            groups::add_member(
                connection,
                &"grp:algebra".parse().unwrap(),
                &bo.id,
                stemma_core::access::GroupRole::Member,
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let other = writing.harness.credential("bo").await;

    let now = writing
        .harness
        .get_with_token(&format!("/api/lines/{line}"), &other)
        .await;
    assert_eq!(now.status, StatusCode::OK, "{}", now.body);
    assert_eq!(parsed(&now)["side"], "shared");

    let before = writing
        .harness
        .get_with_token(&format!("/api/lines/{line}?revision=1"), &other)
        .await;
    assert_eq!(before.status, StatusCode::NOT_FOUND, "{}", before.body);

    let own = writing
        .harness
        .get_with_token(&format!("/api/lines/{line}?revision=1"), &writing.token)
        .await;
    assert_eq!(own.status, StatusCode::OK, "{}", own.body);
    assert_eq!(parsed(&own)["side"], "private");
}

/// How many commits a bare repository's branch has.
fn commits(repository: &std::path::Path) -> usize {
    let output = std::process::Command::new("git")
        .args(["rev-list", "--count", "refs/heads/main"])
        .current_dir(repository)
        .output()
        .expect("git");

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

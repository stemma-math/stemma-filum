//! Signing a machine in without a browser on it.

mod support;

use axum::http::StatusCode;
use serde_json::Value;
use stemma_server::store::devices;
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

/// What an answer said, as JSON.
fn parsed(page: &Page) -> Value {
    serde_json::from_str(&page.body).expect("an answer in JSON")
}

#[tokio::test]
async fn a_machine_waits_for_a_person_and_then_holds_a_credential() {
    let mut harness = Harness::new();

    // On the machine, which has no browser.
    let started = parsed(
        &harness
            .post_json("/api/device/start", r#"{"label":"ana's laptop"}"#)
            .await,
    );

    let device_code = started["device_code"].as_str().expect("a device code");
    let user_code = started["user_code"].as_str().expect("a code to read");

    assert_eq!(user_code.len(), 9, "four, a hyphen and four");
    assert_eq!(
        started["verification_uri"].as_str(),
        Some("https://stemma.test/device")
    );
    assert_eq!(started["interval"].as_u64(), Some(5));

    let waiting = harness
        .post_json(
            "/api/device/collect",
            &format!(r#"{{"device_code":"{device_code}"}}"#),
        )
        .await;

    assert_eq!(waiting.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        parsed(&waiting)["error"].as_str(),
        Some("authorization_pending")
    );

    // In a browser, where somebody is already signed in.
    register(&mut harness, "ana").await;

    let asked = harness.get(&format!("/device?code={user_code}")).await;
    assert!(asked.says("Let this machine act for you?"));
    assert!(asked.says("laptop"), "the page does not name the machine");
    assert!(asked.says("usr:ana"));

    let answered = harness
        .post(
            "/device",
            &[
                ("code", user_code),
                ("approve", "yes"),
                ("form_token", &form_token(&asked)),
            ],
        )
        .await;

    assert!(answered.says("can now act for you"));

    // Back on the machine.
    let collected = harness
        .post_json(
            "/api/device/collect",
            &format!(r#"{{"device_code":"{device_code}"}}"#),
        )
        .await;

    assert_eq!(collected.status, StatusCode::OK);
    let answer = parsed(&collected);
    assert_eq!(answer["user"].as_str(), Some("usr:ana"));

    let token = answer["access_token"].as_str().expect("a credential");
    let holder = harness
        .instance
        .read({
            let token = token.to_owned();
            move |connection| {
                devices::holder(
                    connection,
                    &stemma_server::tokens::Key::from_material(b"a test key"),
                    &token,
                )
            }
        })
        .await
        .unwrap();

    assert_eq!(holder.map(|id| id.to_string()).as_deref(), Some("usr:ana"));

    // The credential is handed over once.
    let again = harness
        .post_json(
            "/api/device/collect",
            &format!(r#"{{"device_code":"{device_code}"}}"#),
        )
        .await;
    assert_eq!(parsed(&again)["error"].as_str(), Some("expired_token"));
}

#[tokio::test]
async fn a_refused_machine_is_told_so_rather_than_left_waiting() {
    let mut harness = Harness::new();
    let started = parsed(
        &harness
            .post_json("/api/device/start", r#"{"label":"somebody else's laptop"}"#)
            .await,
    );
    let device_code = started["device_code"].as_str().unwrap().to_owned();
    let user_code = started["user_code"].as_str().unwrap().to_owned();

    register(&mut harness, "ana").await;
    let asked = harness.get(&format!("/device?code={user_code}")).await;

    let refused = harness
        .post(
            "/device",
            &[("code", &user_code), ("form_token", &form_token(&asked))],
        )
        .await;

    assert!(refused.says("was not let in"));

    let told = harness
        .post_json(
            "/api/device/collect",
            &format!(r#"{{"device_code":"{device_code}"}}"#),
        )
        .await;

    assert_eq!(parsed(&told)["error"].as_str(), Some("access_denied"));
}

#[tokio::test]
async fn a_code_that_names_nothing_says_what_to_do_about_it() {
    let mut harness = Harness::new();
    register(&mut harness, "ana").await;

    let page = harness.get("/device?code=ABCD-EFGH").await;

    assert_eq!(page.status, StatusCode::NOT_FOUND);
    assert!(page.says("No machine is waiting with that code"));
}

#[tokio::test]
async fn nobody_approves_a_machine_without_being_signed_in() {
    let mut harness = Harness::new();
    let started = parsed(
        &harness
            .post_json("/api/device/start", r#"{"label":"a laptop"}"#)
            .await,
    );
    let user_code = started["user_code"].as_str().unwrap().to_owned();

    let page = harness.get(&format!("/device?code={user_code}")).await;

    assert_eq!(page.status, StatusCode::SEE_OTHER);
    assert!(
        page.location
            .as_deref()
            .unwrap()
            .starts_with("/login?next=/device")
    );
}

#[tokio::test]
async fn a_machine_that_says_nothing_about_itself_is_still_named_on_the_page() {
    let mut harness = Harness::new();
    let started = parsed(&harness.post_json("/api/device/start", "{}").await);
    let user_code = started["user_code"].as_str().unwrap().to_owned();

    register(&mut harness, "ana").await;
    let asked = harness.get(&format!("/device?code={user_code}")).await;

    assert!(asked.says("an unnamed machine"));
}

#[tokio::test]
async fn a_code_can_be_typed_however_it_is_read() {
    let mut harness = Harness::new();
    let started = parsed(
        &harness
            .post_json("/api/device/start", r#"{"label":"a laptop"}"#)
            .await,
    );
    let user_code = started["user_code"].as_str().unwrap().to_owned();
    let typed = user_code.replace('-', "").to_lowercase();

    register(&mut harness, "ana").await;

    assert!(
        harness
            .get(&format!("/device?code={typed}"))
            .await
            .says("Let this machine act for you?")
    );
}

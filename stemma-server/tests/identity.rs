//! Getting into an instance, from an empty one to a signed-in account.

mod support;

use axum::http::StatusCode;
use stemma_server::store::{settings, users};
use support::Harness;

/// The fields a registration form sends.
fn registration(name: &str) -> Vec<(&'static str, String)> {
    vec![
        ("display_name", "Ana Ferrer".to_owned()),
        ("slug", name.to_owned()),
        ("email", format!("{name}@example.org")),
        ("password", "a long enough password".to_owned()),
    ]
}

/// The same fields, borrowed the way the harness wants them.
fn fields<'a>(given: &'a [(&'static str, String)]) -> Vec<(&'a str, &'a str)> {
    given
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect()
}

#[tokio::test]
async fn a_fresh_instance_says_it_is_empty_instead_of_offering_a_log_in() {
    let mut harness = Harness::new();
    let page = harness.get("/").await;

    assert_eq!(page.status, StatusCode::OK);
    assert!(page.says("Nobody has registered here yet"));
    assert!(page.says("Create the first account"));
    assert!(!page.says("Log in</a>"));
}

#[tokio::test]
async fn an_instance_is_called_by_its_host_until_somebody_names_it() {
    let mut harness = Harness::at("https://algebra.uv.es");
    assert!(harness.get("/").await.says("algebra.uv.es"));

    harness
        .instance
        .write(|connection| {
            settings::set(connection, settings::INSTANCE_NAME, "Àlgebra UV", None)?;
            settings::set(connection, settings::INSTANCE_OPERATOR, "Enric Cosme", None)
        })
        .await
        .unwrap();

    let page = harness.get("/").await;
    assert!(page.says("Àlgebra UV"));
    assert!(page.says("Run by Enric Cosme."));
}

#[tokio::test]
async fn registering_creates_the_account_and_signs_the_person_in() {
    let mut harness = Harness::new();
    let form = registration("ana");

    let page = harness.post("/register", &fields(&form)).await;

    assert_eq!(page.status, StatusCode::SEE_OTHER);
    assert_eq!(page.location.as_deref(), Some("/"));
    assert!(
        page.set_cookie
            .is_some_and(|cookie| cookie.contains("HttpOnly"))
    );

    let home = harness.get("/").await;
    assert!(home.says("Ana Ferrer"));
    assert!(home.says("usr:ana"));
    assert!(home.says("You are not in any group yet"));
}

#[tokio::test]
async fn an_instance_over_plain_http_does_not_claim_its_cookie_is_encrypted() {
    let mut harness = Harness::at("http://localhost:8080");
    let form = registration("ana");

    let page = harness.post("/register", &fields(&form)).await;
    let cookie = page.set_cookie.expect("a session cookie");

    assert!(!cookie.contains("Secure"));
    assert!(cookie.contains("HttpOnly"));
}

#[tokio::test]
async fn a_name_or_an_address_is_taken_only_once_and_the_form_says_which() {
    let mut harness = Harness::new();
    harness
        .post("/register", &fields(&registration("ana")))
        .await;
    harness.post("/logout", &[]).await;

    let same_name = harness
        .post("/register", &fields(&registration("ana")))
        .await;
    assert_eq!(same_name.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(same_name.says("is taken"));

    let mut other = registration("ana-f");
    other[2].1 = "ana@example.org".to_owned();
    let same_address = harness.post("/register", &fields(&other)).await;
    assert!(same_address.says("already an account for that address"));
}

#[tokio::test]
async fn what_cannot_be_an_identifier_is_refused_with_the_reason() {
    let mut harness = Harness::new();

    let mut wrong = registration("Ana Ferrer");
    let page = harness.post("/register", &fields(&wrong)).await;
    assert!(page.says("not allowed in a slug"));

    wrong = registration("ana");
    wrong[3].1 = "short".to_owned();
    let page = harness.post("/register", &fields(&wrong)).await;
    assert!(page.says("at least 10 characters"));

    wrong = registration("ana");
    wrong[2].1 = "not-an-address".to_owned();
    let page = harness.post("/register", &fields(&wrong)).await;
    assert!(page.says("does not look like an email address"));
}

#[tokio::test]
async fn logging_in_needs_the_password_and_says_nothing_about_who_exists() {
    let mut harness = Harness::new();
    harness
        .post("/register", &fields(&registration("ana")))
        .await;
    harness.post("/logout", &[]).await;

    let wrong_password = harness
        .post(
            "/login",
            &[("login", "ana"), ("password", "not the password")],
        )
        .await;
    let no_such_account = harness
        .post(
            "/login",
            &[("login", "nobody"), ("password", "not the password")],
        )
        .await;

    assert_eq!(wrong_password.status, StatusCode::UNAUTHORIZED);
    assert_eq!(no_such_account.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_password.body, no_such_account.body);

    let admitted = harness
        .post(
            "/login",
            &[
                ("login", "ana@example.org"),
                ("password", "a long enough password"),
            ],
        )
        .await;

    assert_eq!(admitted.status, StatusCode::SEE_OTHER);
    assert!(harness.get("/").await.says("usr:ana"));
}

#[tokio::test]
async fn a_suspended_account_cannot_log_in_or_stay_in() {
    let mut harness = Harness::new();
    harness
        .post("/register", &fields(&registration("ana")))
        .await;

    harness
        .instance
        .write(|connection| {
            let ana = users::by_login(connection, "ana")?.expect("the account");
            users::set_suspended(connection, &ana.id, true)
        })
        .await
        .unwrap();

    // The session it already held is worth nothing.
    let page = harness.get("/").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.says("Log in"));

    let refused = harness
        .post(
            "/login",
            &[("login", "ana"), ("password", "a long enough password")],
        )
        .await;

    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logging_out_takes_the_cookie_back() {
    let mut harness = Harness::new();
    harness
        .post("/register", &fields(&registration("ana")))
        .await;

    let page = harness.post("/logout", &[]).await;

    assert_eq!(page.location.as_deref(), Some("/"));
    assert!(
        page.set_cookie
            .is_some_and(|cookie| cookie.contains("Max-Age=0"))
    );
    assert!(harness.get("/").await.says("Log in"));
}

#[tokio::test]
async fn a_page_that_needs_an_account_sends_the_reader_to_the_form_and_back() {
    let mut harness = Harness::new();
    harness
        .post("/register", &fields(&registration("ana")))
        .await;
    harness.post("/logout", &[]).await;

    let page = harness.get("/login?next=/admin").await;
    assert!(page.says("value=\"/admin\""));

    let admitted = harness
        .post(
            "/login",
            &[
                ("login", "ana"),
                ("password", "a long enough password"),
                ("next", "/admin"),
            ],
        )
        .await;

    assert_eq!(admitted.location.as_deref(), Some("/admin"));
}

#[tokio::test]
async fn a_closed_instance_still_lets_the_first_account_in() {
    let mut harness = Harness::new();
    harness
        .instance
        .write(|connection| settings::set(connection, settings::REGISTRATION_OPEN, "false", None))
        .await
        .unwrap();

    let first = harness
        .post("/register", &fields(&registration("ana")))
        .await;
    assert_eq!(first.status, StatusCode::SEE_OTHER);

    harness.post("/logout", &[]).await;

    let second = harness
        .post("/register", &fields(&registration("enric")))
        .await;
    assert!(second.says("Accounts here are by invitation"));
    assert!(
        harness
            .get("/register")
            .await
            .says("Accounts here are by invitation")
    );
}

#[tokio::test]
async fn the_front_door_of_a_closed_instance_says_how_people_get_in() {
    let mut harness = Harness::new();
    harness
        .post("/register", &fields(&registration("ana")))
        .await;
    harness.post("/logout", &[]).await;

    harness
        .instance
        .write(|connection| settings::set(connection, settings::REGISTRATION_OPEN, "false", None))
        .await
        .unwrap();

    let page = harness.get("/").await;

    assert!(page.says("Accounts here are created by invitation"));
    assert!(!page.says("/register"));
}

#[tokio::test]
async fn there_is_nothing_at_an_address_with_nothing_at_it() {
    let mut harness = Harness::new();
    let page = harness.get("/nowhere").await;

    assert_eq!(page.status, StatusCode::NOT_FOUND);
    assert!(page.says("There is nothing at this address."));
}

#[tokio::test]
async fn the_stylesheet_is_served_from_this_instance_and_can_be_cached() {
    let mut harness = Harness::new();
    let page = harness.get("/assets/stemma.css").await;

    assert_eq!(page.status, StatusCode::OK);
    assert!(page.says("--ditto"));

    let front = harness.get("/").await;
    assert!(front.says("href=\"/assets/stemma.css\""));
    assert!(
        !front.body.contains("//fonts."),
        "the page reaches off the instance"
    );
}

#[tokio::test]
async fn a_reset_link_sets_a_password_once_and_ends_every_session() {
    let mut harness = Harness::new();
    harness
        .post("/register", &fields(&registration("ana")))
        .await;

    let secret = harness
        .instance
        .read(|connection| {
            let ana = users::by_login(connection, "ana")?.expect("the account");
            stemma_server::store::resets::create(
                connection,
                &stemma_server::tokens::Key::from_material(b"a test key"),
                &ana.id,
            )
        })
        .await
        .unwrap();

    let form = harness.get(&format!("/reset/{secret}")).await;
    assert!(form.says("Set a password"));

    let too_short = harness
        .post(&format!("/reset/{secret}"), &[("password", "short")])
        .await;
    assert_eq!(too_short.status, StatusCode::UNPROCESSABLE_ENTITY);

    let set = harness
        .post(
            &format!("/reset/{secret}"),
            &[("password", "another long password")],
        )
        .await;
    assert_eq!(set.location.as_deref(), Some("/login"));

    // The session it held before is gone, and the link is spent.
    assert!(harness.get("/").await.says("Log in"));
    assert_eq!(
        harness.get(&format!("/reset/{secret}")).await.status,
        StatusCode::GONE
    );

    let admitted = harness
        .post(
            "/login",
            &[("login", "ana"), ("password", "another long password")],
        )
        .await;
    assert_eq!(admitted.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn an_instance_that_cannot_send_mail_says_so_rather_than_pretending() {
    let mut harness = Harness::new();

    let login = harness.get("/login").await;
    assert!(login.says("Forgotten your password?"));

    let asked = harness.get("/forgotten").await;
    assert!(asked.says("does not send mail"));
    assert!(asked.says("stemma-server admin reset"));

    // And it does not quietly claim to have sent anything.
    let sent = harness
        .post("/forgotten", &[("email", "ana@example.org")])
        .await;

    assert!(sent.says("does not send mail"));
    assert!(!sent.says("on its way"));
}

#[tokio::test]
async fn asking_for_a_link_says_the_same_thing_whoever_asks() {
    let mut harness = Harness::new();

    harness
        .instance
        .write(|connection| {
            stemma_server::store::settings::set(
                connection,
                stemma_server::store::settings::MAIL_FROM,
                "stemma@example.org",
                None,
            )?;
            stemma_server::store::settings::set(
                connection,
                stemma_server::store::settings::MAIL_SMTP_URL,
                "smtp://127.0.0.1:1",
                None,
            )?;

            Ok(())
        })
        .await
        .unwrap();

    harness
        .post(
            "/register",
            &[
                ("display_name", "Ana"),
                ("slug", "ana"),
                ("email", "ana@example.org"),
                ("password", "a long enough password"),
            ],
        )
        .await;

    let known = harness
        .post("/forgotten", &[("email", "ana@example.org")])
        .await;
    let unknown = harness
        .post("/forgotten", &[("email", "nobody@example.org")])
        .await;

    assert!(known.says("on its way"));
    assert!(unknown.says("on its way"));
    assert_eq!(
        known.body.replace("ana@", "nobody@"),
        unknown.body,
        "the answer must not say which addresses have accounts"
    );
}

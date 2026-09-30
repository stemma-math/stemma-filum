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
async fn the_front_door_is_the_same_door_however_empty_the_instance_is() {
    let mut harness = Harness::new();
    let page = harness.get("/").await;

    assert_eq!(page.status, StatusCode::OK);
    assert!(page.says("Enter Stemma"));
    assert!(page.says("Create an account"));
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
    assert!(page.says("Managed by Enric Cosme"));
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
    // No groups yet, so the only card on the overview is the one that
    // makes one.
    assert!(home.says("Request a research group"));
}

#[tokio::test]
async fn appearance_chosen_on_web_is_shared_with_the_machine_and_back() {
    let mut harness = Harness::new();
    harness
        .post("/register", &fields(&registration("ana")))
        .await;
    let session = harness.cookie.clone();

    let saved = harness
        .post(
            "/appearance",
            &[("appearance", "light"), ("back", "/profile")],
        )
        .await;
    assert_eq!(saved.status, StatusCode::SEE_OTHER);
    assert_eq!(saved.location.as_deref(), Some("/profile"));
    harness.cookie = session;
    let profile = harness.get("/profile").await;
    assert!(profile.says("data-theme=\"light\""));

    let token = harness.credential("ana").await;
    let account = harness.get_with_token("/api/me", &token).await;
    assert!(account.says("\"appearance\":\"light\""));

    let changed = harness
        .post_json_with_token("/api/appearance", &token, "{\"appearance\":\"dark\"}")
        .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert!(harness.get("/profile").await.says("data-theme=\"dark\""));

    let invalid = harness
        .post_json_with_token("/api/appearance", &token, "{\"appearance\":\"blue\"}")
        .await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    assert!(harness.get("/profile").await.says("data-theme=\"dark\""));
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
    assert!(wrong_password.says("The login details are incorrect."));
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

    assert!(page.says("Registration is by invitation only"));
    assert!(!page.says("/register"));
}

#[tokio::test]
async fn there_is_nothing_at_an_address_with_nothing_at_it() {
    let mut harness = Harness::new();
    let page = harness.get("/nowhere").await;

    assert_eq!(page.status, StatusCode::NOT_FOUND);
    assert!(page.says("The requested page could not be found."));
}

#[tokio::test]
async fn the_stylesheet_is_served_from_this_instance_and_can_be_cached() {
    let mut harness = Harness::new();
    let page = harness.get("/assets/css/stemma.css").await;

    assert_eq!(page.status, StatusCode::OK);
    assert!(page.says("--accent"));

    let front = harness.get("/").await;
    assert!(front.says("href=\"/assets/css/stemma.css\""));
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
async fn the_first_account_is_made_on_the_machine_and_sets_its_own_password() {
    let mut harness = Harness::new();

    // The key the machine reads is the one the instance already holds:
    // a link made on the command line has to be worth something to the
    // process that answers it.
    std::fs::create_dir_all(harness.instance.settings.paths.keys_dir())
        .expect("somewhere for the key");
    std::fs::write(harness.instance.settings.paths.session_key(), "a test key").expect("a key");

    let settings = harness.instance.settings.clone();
    let said = harness
        .instance
        .read(move |connection| {
            stemma_server::admin::create_account(
                connection,
                &settings,
                "cosme",
                "cosme@uv.es",
                Some("Enric Cosme"),
            )
        })
        .await
        .expect("an account");

    assert!(said.contains("usr:cosme"));
    assert!(said.contains("Enric Cosme"));

    let secret = said
        .rsplit("/reset/")
        .next()
        .expect("a link")
        .trim()
        .to_owned();

    // Until that link is followed the account cannot be logged into at
    // all, however plausible the guess.
    let guessed = harness
        .post("/login", &[("login", "cosme"), ("password", "cosme@uv.es")])
        .await;
    assert_eq!(guessed.status, StatusCode::UNAUTHORIZED);

    let set = harness
        .post(
            &format!("/reset/{secret}"),
            &[("password", "a long enough password")],
        )
        .await;
    assert_eq!(set.location.as_deref(), Some("/login"));

    let admitted = harness
        .post(
            "/login",
            &[("login", "cosme"), ("password", "a long enough password")],
        )
        .await;
    assert_eq!(admitted.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn an_account_made_on_the_machine_is_refused_twice() {
    let harness = Harness::new();

    std::fs::create_dir_all(harness.instance.settings.paths.keys_dir())
        .expect("somewhere for the key");
    std::fs::write(harness.instance.settings.paths.session_key(), "a test key").expect("a key");

    let make = |name: &'static str, email: &'static str| {
        let settings = harness.instance.settings.clone();
        let instance = harness.instance.clone();

        async move {
            instance
                .read(move |connection| {
                    stemma_server::admin::create_account(connection, &settings, name, email, None)
                })
                .await
        }
    };

    make("cosme", "cosme@uv.es").await.expect("an account");

    let again = make("cosme", "somebody-else@uv.es").await;
    assert!(
        format!("{:#}", again.unwrap_err()).contains("already taken"),
        "a name was handed out twice"
    );

    let same_address = make("mario", "cosme@uv.es").await;
    assert!(
        format!("{:#}", same_address.unwrap_err()).contains("already an account"),
        "an address was handed out twice"
    );
}

#[tokio::test]
async fn an_instance_that_cannot_send_mail_says_so_rather_than_pretending() {
    let mut harness = Harness::new();

    let login = harness.get("/login").await;
    assert!(login.says("Forgotten your password?"));

    let asked = harness.get("/forgotten").await;
    assert!(asked.says("Email delivery is not configured"));
    assert!(asked.says("stemma-server user reset-password"));

    // And it does not quietly claim to have sent anything.
    let sent = harness
        .post("/forgotten", &[("email", "ana@example.org")])
        .await;

    assert!(sent.says("Email delivery is not configured"));
    assert!(!sent.says("has been sent"));
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

    assert!(known.says("a password reset link has been sent"));
    assert!(unknown.says("a password reset link has been sent"));
    assert_eq!(
        known.body.replace("ana@", "nobody@"),
        unknown.body,
        "the answer must not say which addresses have accounts"
    );
}

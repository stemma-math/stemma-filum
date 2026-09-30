//! An instance to make requests against.
//!
//! Shared by every test file, so each one uses a part of it.

#![allow(dead_code)]

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderValue, Request, Response, StatusCode, header};
use stemma_server::config::{Listen, Paths, PolicySeed, PublicUrl, Settings};
use stemma_server::db::Database;
use stemma_server::tokens::Key;
use stemma_server::web::{self, Instance};
use tower::ServiceExt;

/// A whole instance, in memory, with the schema applied.
pub struct Harness {
    /// What the routes run against.
    pub instance: Instance,
    /// The cookie the last answer handed out, if it handed one out.
    pub cookie: Option<String>,
    /// Where this instance's files are, for as long as the test runs.
    pub root: PathBuf,
}

impl Harness {
    /// A fresh instance nobody has used.
    pub fn new() -> Self {
        Self::at("https://stemma.test")
    }

    /// A fresh instance answering on a particular address.
    pub fn at(public_url: &str) -> Self {
        let mut database = Database::in_memory().expect("a database in memory");
        database.migrate().expect("the schema applies");

        let root = std::env::temp_dir().join(format!(
            "stemma-harness-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("somewhere to keep the instance's files");

        let settings = Settings {
            public_url: PublicUrl::parse(public_url).expect("a valid address"),
            listen: "127.0.0.1:0".parse::<Listen>().expect("a valid address"),
            paths: Paths::new(&root),
            policy_seed: PolicySeed::default(),
            source: root.join("server.toml"),
        };

        Self {
            instance: Instance::new(settings, database, Key::from_material(b"a test key")),
            cookie: None,
            root,
        }
    }

    /// The routes, as the server assembles them.
    fn app(&self) -> Router {
        web::router().with_state(self.instance.clone())
    }

    /// Asks for a page, carrying whatever cookie was last handed out.
    pub async fn get(&mut self, path: &str) -> Page {
        let request = self
            .request("GET", path)
            .body(Body::empty())
            .expect("a request");

        self.send(request).await
    }

    /// Sends a form, carrying whatever cookie was last handed out.
    pub async fn post(&mut self, path: &str, fields: &[(&str, &str)]) -> Page {
        let body = fields
            .iter()
            .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
            .collect::<Vec<_>>()
            .join("&");

        let request = self
            .request("POST", path)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .expect("a request");

        self.send(request).await
    }

    /// Sends what a program on a machine sends, which is JSON.
    pub async fn post_json(&mut self, path: &str, body: &str) -> Page {
        let request = self
            .request("POST", path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .expect("a request");

        self.send(request).await
    }

    /// Sends JSON as a machine would, carrying a credential.
    pub async fn post_json_with_token(&mut self, path: &str, token: &str, body: &str) -> Page {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .expect("a request");

        self.send(request).await
    }

    /// Replaces something as a machine would, carrying a credential.
    pub async fn put_json_with_token(&mut self, path: &str, token: &str, body: &str) -> Page {
        let request = Request::builder()
            .method("PUT")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .expect("a request");

        self.send(request).await
    }

    /// A credential for an account, made the way a signed-in machine
    /// collects one.
    pub async fn credential(&self, account: &str) -> String {
        let account = account.to_owned();

        self.instance
            .write(move |connection| {
                let key = Key::from_material(b"a test key");
                let user = stemma_server::store::users::by_login(connection, &account)?
                    .expect("the account");
                let (grant, device_code) =
                    stemma_server::store::devices::start(connection, &key, "a test machine")?;

                stemma_server::store::devices::approve(connection, &grant.id, &user.id)?;

                match stemma_server::store::devices::collect(connection, &key, &device_code)? {
                    stemma_server::store::devices::Collected::Issued { token, .. } => Ok(token),
                    other => panic!("no credential: {other:?}"),
                }
            })
            .await
            .expect("a credential")
    }

    /// Asks for something as a machine would, carrying a credential.
    pub async fn get_with_token(&mut self, path: &str, token: &str) -> Page {
        let request = Request::builder()
            .method("GET")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("a request");

        self.send(request).await
    }

    /// Sends something as a machine would, carrying a credential.
    pub async fn post_with_token(&mut self, path: &str, token: &str) -> Page {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("a request");

        self.send(request).await
    }

    /// Listens to the event stream as a machine would, and returns the
    /// first `wanted` events it says, each as its `id` and its `data`.
    ///
    /// A stream does not end by itself, so this stops at what it wanted,
    /// or after a few seconds of hearing nothing more.
    pub async fn listen(
        &self,
        token: &str,
        last_seen: Option<i64>,
        wanted: usize,
    ) -> Vec<(Option<i64>, String)> {
        use futures_util::StreamExt;

        let mut request = Request::builder()
            .method("GET")
            .uri("/api/events")
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(last_seen) = last_seen {
            request = request.header("last-event-id", last_seen.to_string());
        }

        let response = self
            .app()
            .oneshot(request.body(Body::empty()).expect("a request"))
            .await
            .expect("an answer");
        assert_eq!(response.status(), StatusCode::OK, "the stream was refused");

        let mut body = response.into_body().into_data_stream();
        let mut text = String::new();
        let mut heard = Vec::new();

        while heard.len() < wanted {
            let chunk = tokio::time::timeout(std::time::Duration::from_secs(3), body.next()).await;
            let Ok(Some(Ok(chunk))) = chunk else { break };
            text.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(end) = text.find("\n\n") {
                let frame: String = text.drain(..end + 2).collect();
                let id = frame
                    .lines()
                    .find_map(|line| line.strip_prefix("id: "))
                    .and_then(|id| id.parse().ok());
                if let Some(data) = frame.lines().find_map(|line| line.strip_prefix("data: ")) {
                    heard.push((id, data.to_owned()));
                }
            }
        }

        heard.truncate(wanted);
        heard
    }

    /// Starts a request with the cookie this harness holds.
    fn request(&self, method: &str, path: &str) -> axum::http::request::Builder {
        let request = Request::builder().method(method).uri(path);

        match &self.cookie {
            Some(cookie) => request.header(header::COOKIE, cookie),
            None => request,
        }
    }

    /// Runs one request and remembers any cookie that comes back.
    async fn send(&mut self, request: Request<Body>) -> Page {
        let response = self.app().oneshot(request).await.expect("an answer");
        let page = Page::read(response).await;

        if let Some(handed_out) = &page.set_cookie {
            self.cookie = match handed_out.contains("Max-Age=0") {
                true => None,
                false => handed_out.split(';').next().map(str::to_owned),
            };
        }

        page
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One answer.
pub struct Page {
    /// What it came back with.
    pub status: StatusCode,
    /// Where it sends the reader, if it sends them anywhere.
    pub location: Option<String>,
    /// The cookie it handed out, if any.
    pub set_cookie: Option<String>,
    /// What kind of thing it says it is.
    pub content_type: Option<String>,
    /// Whether a response may be served without asking the instance again.
    pub cache_control: Option<String>,
    /// What it says.
    pub body: String,
}

impl Page {
    /// Reads an answer.
    async fn read(response: Response<Body>) -> Self {
        let status = response.status();
        let header = |name: header::HeaderName| {
            response
                .headers()
                .get(name)
                .and_then(|value: &HeaderValue| value.to_str().ok())
                .map(str::to_owned)
        };

        let location = header(header::LOCATION);
        let set_cookie = header(header::SET_COOKIE);
        let content_type = header(header::CONTENT_TYPE);
        let cache_control = header(header::CACHE_CONTROL);

        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a body");

        Self {
            status,
            location,
            set_cookie,
            content_type,
            cache_control,
            body: String::from_utf8_lossy(&body).into_owned(),
        }
    }

    /// Whether the page says this.
    pub fn says(&self, words: &str) -> bool {
        self.body.contains(words)
    }
}

/// Escapes a form field.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "+".to_owned(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// A statement with a name and no proof to write, as these tests need them.
pub fn statement(siglum: &str, title: &str) -> stemma_core::document::Block {
    let siglum: stemma_core::line::Siglum = siglum.parse().expect("a siglum");
    match siglum.kind {
        stemma_core::line::NodeKind::Definition => {
            stemma_core::document::Block::definition(Some(siglum), "x").titled(title)
        }
        kind => stemma_core::document::Block::theorem(kind.word(), Some(siglum), "y")
            .titled(title)
            .carrying_proof(false),
    }
}

/// A line's digest, as a machine that read it would publish it.
pub fn digest(document: &stemma_core::document::Document) -> stemma_core::artefact::LineDigest {
    stemma_core::artefact::LineDigest {
        name: stemma_core::line::Name::parse("quotient-groups").unwrap(),
        title: Some(document.title.clone()),
        description: document.description.clone(),
        line: stemma_core::line::Line::of(document).expect("a line that reads"),
        lean: None,
        revision: None,
    }
}

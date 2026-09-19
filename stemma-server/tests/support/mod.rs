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

        let settings = Settings {
            public_url: PublicUrl::parse(public_url).expect("a valid address"),
            listen: "127.0.0.1:0".parse::<Listen>().expect("a valid address"),
            paths: Paths::new("/tmp/stemma-test"),
            policy_seed: PolicySeed::default(),
            source: PathBuf::from("/tmp/stemma-test/server.toml"),
        };

        Self {
            instance: Instance::new(settings, database, Key::from_material(b"a test key")),
            cookie: None,
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

/// One answer.
pub struct Page {
    /// What it came back with.
    pub status: StatusCode,
    /// Where it sends the reader, if it sends them anywhere.
    pub location: Option<String>,
    /// The cookie it handed out, if any.
    pub set_cookie: Option<String>,
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

        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a body");

        Self {
            status,
            location,
            set_cookie,
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

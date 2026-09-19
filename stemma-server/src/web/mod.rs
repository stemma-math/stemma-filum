//! The web the instance serves.
//!
//! Every page is rendered here, on the server, and the identity survives
//! with JavaScript switched off: hairlines, paper levels and hatches are
//! all CSS, and there is nothing else to lose.

pub mod admin;
pub mod api;
pub mod artefacts;
pub mod assets;
pub mod auth;
pub mod cookies;
pub mod device;
pub mod front;
pub mod groups;
pub mod home;
pub mod invite;
pub mod jobs;
pub mod layout;
pub mod lines;
pub mod machines;
pub mod proposals;
pub mod reset;
pub mod transport;
pub mod vault;
pub mod visitor;

use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use rusqlite::Connection;

use crate::config::Settings;
use crate::db::Database;
use crate::store::settings::Policy;
use crate::tokens::Key;

/// Everything a page needs to answer a request.
#[derive(Clone)]
pub struct Instance {
    /// How this process came up.
    pub settings: Arc<Settings>,
    /// The database, which one thread uses at a time.
    pub database: Arc<Mutex<Database>>,
    /// The key that makes a cookie or a link worth something.
    pub key: Key,
}

impl Instance {
    /// Builds the state the web runs on.
    pub fn new(settings: Settings, database: Database, key: Key) -> Self {
        Self {
            settings: Arc::new(settings),
            database: Arc::new(Mutex::new(database)),
            key,
        }
    }

    /// Runs a read against the database.
    ///
    /// SQLite is not asynchronous, so the work goes to a thread that is
    /// allowed to block rather than holding up the runtime.
    pub async fn read<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let database = self.database.clone();

        let outcome = tokio::task::spawn_blocking(move || {
            let database = database.lock().map_err(|_| {
                anyhow::anyhow!("the database is unusable because a request panicked holding it")
            })?;

            work(database.connection())
        })
        .await
        .map_err(|error| WebError::Internal(anyhow::anyhow!("the work did not finish: {error}")))?;

        outcome.map_err(WebError::Internal)
    }

    /// Runs a write against the database, where the work needs a
    /// transaction of its own.
    pub async fn write<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let database = self.database.clone();

        let outcome = tokio::task::spawn_blocking(move || {
            let mut database = database.lock().map_err(|_| {
                anyhow::anyhow!("the database is unusable because a request panicked holding it")
            })?;

            work(database.connection_mut())
        })
        .await
        .map_err(|error| WebError::Internal(anyhow::anyhow!("the work did not finish: {error}")))?;

        outcome.map_err(WebError::Internal)
    }

    /// Runs work against the repositories.
    ///
    /// Git is a program, not a library here, so this also goes to a thread
    /// that is allowed to block.
    pub async fn with_repositories<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce(&crate::git::Repositories) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let repositories = self.repositories();

        tokio::task::spawn_blocking(move || work(&repositories))
            .await
            .map_err(|error| {
                WebError::Internal(anyhow::anyhow!("the work did not finish: {error}"))
            })?
            .map_err(WebError::Internal)
    }

    /// Runs work that touches the disk.
    ///
    /// Files are not asynchronous either, so this goes to a thread that
    /// is allowed to wait for them.
    pub async fn on_disk<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|error| {
                WebError::Internal(anyhow::anyhow!("the work did not finish: {error}"))
            })?
            .map_err(WebError::Internal)
    }

    /// Where this instance keeps its repositories.
    pub fn repositories(&self) -> crate::git::Repositories {
        crate::git::Repositories::new(self.settings.paths.git_dir())
    }

    /// What the instance's policy says right now.
    pub async fn policy(&self) -> Result<Policy, WebError> {
        self.read(crate::store::settings::policy).await
    }
}

/// Every page the instance serves.
///
/// The state is left to the caller to attach, so that the routes that are
/// not pages can be added beside these first.
pub fn router() -> Router<Instance> {
    Router::new()
        .route("/", get(front::door))
        .route("/login", get(auth::login_form).post(auth::log_in))
        .route("/register", get(auth::register_form).post(auth::register))
        .route("/logout", post(auth::log_out))
        .route(
            "/groups/new",
            get(groups::request_form).post(groups::request),
        )
        .route("/g/{slug}", get(groups::space))
        .route("/g/{slug}/vault", get(vault::page_view))
        .route("/g/{slug}/vault/entries", post(vault::admit))
        .route("/g/{slug}/vault/bridges", post(vault::bridge))
        .route("/g/{slug}/vault/audits", post(vault::audit))
        .route("/g/{slug}/vault/obligations", post(vault::assign))
        .route("/audit-obligations/{id}/decide", post(vault::decide))
        .route("/g/{slug}/{side}/{line}", get(lines::view))
        .route("/g/{slug}/{side}/{line}/jobs", post(jobs::launch))
        .route("/g/{slug}/invitations", post(groups::invite))
        .route(
            "/g/{slug}/invitations/{id}/withdraw",
            post(groups::withdraw),
        )
        .route("/machines/{id}", get(machines::panel))
        .route("/machines/{id}/serve", post(machines::serve))
        .route("/machines/{id}/retire", post(machines::retire))
        .route("/machines/{id}/jobs/{job}/clear", post(machines::clear))
        .route("/invite/{secret}", get(invite::offer))
        .route("/invite/{secret}/join", post(invite::join))
        .route("/invite/{secret}/register", post(invite::join_and_register))
        .route("/reset/{secret}", get(reset::form).post(reset::set))
        .route("/device", get(device::form).post(device::answer))
        .route(
            "/git/{group}/{*rest}",
            get(transport::serve).post(transport::serve),
        )
        .route("/api/me", get(api::me))
        .route("/api/workspace", get(api::workspace))
        .route(
            "/api/groups/{group}/{side}/artefacts/{kind}/{commit}",
            get(artefacts::send).put(artefacts::receive),
        )
        .route(
            "/api/machines",
            get(machines::mine).post(machines::register),
        )
        .route("/api/machines/beat", post(machines::beat))
        .route("/api/jobs", get(jobs::listing).post(jobs::start))
        .route("/api/jobs/take", post(jobs::take))
        .route("/api/groups/{group}/machines", get(jobs::machine_choices))
        .route("/api/jobs/{id}/finish", post(jobs::finish))
        .route("/api/proposals/{id}", get(proposals::accepted))
        .route("/proposals/{id}", get(proposals::review))
        .route("/proposals/{id}/decide", post(proposals::decide))
        .route("/api/logout", post(api::sign_out))
        .route("/api/device/start", post(device::start))
        .route("/api/device/collect", post(device::collect))
        .route("/admin", get(admin::area))
        .route(
            "/admin/authenticate",
            get(admin::authenticate_form).post(admin::authenticate),
        )
        .route("/admin/settings", post(admin::save_settings))
        .route("/admin/requests/{id}/approve", post(admin::approve))
        .route("/admin/requests/{id}/decline", post(admin::decline))
        .route(assets::STYLESHEET_PATH, get(assets::stylesheet))
        .fallback(get(not_found))
}

/// What is served when there is nothing at an address.
async fn not_found() -> WebError {
    WebError::NotFound
}

/// Why a page could not be served.
#[derive(Debug, thiserror::Error)]
pub enum WebError {
    /// There is nothing at this address.
    #[error("there is nothing here")]
    NotFound,
    /// There is something here, and it is not this visitor's.
    #[error("{0}")]
    Forbidden(String),
    /// What was sent cannot be acted on.
    #[error("{0}")]
    BadRequest(String),
    /// The server could not do its part.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl WebError {
    /// The status the answer carries.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// What the page says.
    ///
    /// An internal failure is logged in full and described in one line: the
    /// visitor cannot act on a stack of contexts, and some of them name
    /// paths on the machine.
    pub fn explain(&self) -> String {
        match self {
            Self::NotFound => "There is nothing at this address.".to_owned(),
            Self::Forbidden(reason) | Self::BadRequest(reason) => reason.clone(),
            Self::Internal(_) => {
                "Something in the server went wrong. It has been logged.".to_owned()
            }
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        if let Self::Internal(error) = &self {
            tracing::error!("{error:#}");
        }

        (self.status(), layout::failure(&self)).into_response()
    }
}

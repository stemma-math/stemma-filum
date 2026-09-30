//! The web the instance serves.
//!
//! Every page is rendered here, on the server, and the identity survives
//! with JavaScript switched off: hairlines, paper levels and hatches are
//! all CSS, and there is nothing else to lose.

pub mod admin;
pub mod api;
pub mod appearance;
pub mod artefacts;
pub mod assets;
pub mod auth;
pub mod claims;
pub mod cookies;
pub mod device;
pub mod events;
pub mod front;
pub mod groups;
pub mod home;
pub mod invite;
pub mod jobs;
pub mod layout;
pub mod lines;
pub mod machines;
pub mod membership;
pub mod pictures;
pub mod profile;
pub mod proposals;
pub mod releases;
pub mod reset;
pub mod revisions;
pub mod starting;
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
    /// Rung whenever the database may have changed, so that whoever is
    /// waiting for events looks again instead of on a timer.
    pub changed: Arc<tokio::sync::Notify>,
    /// Held while anything writes to the repositories, so two writers
    /// never race for the same branch.
    pub git: Arc<Mutex<()>>,
}

impl Instance {
    /// Builds the state the web runs on.
    pub fn new(settings: Settings, database: Database, key: Key) -> Self {
        Self {
            settings: Arc::new(settings),
            database: Arc::new(Mutex::new(database)),
            key,
            changed: Arc::new(tokio::sync::Notify::new()),
            git: Arc::new(Mutex::new(())),
        }
    }

    /// Runs a read against the database on the blocking pool, since SQLite
    /// calls block.
    pub async fn read<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let database = self.database.clone();
        self.blocking(move || work(lock(&database)?.connection()))
            .await
    }

    /// Runs a write that needs a transaction of its own.
    pub async fn write<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let database = self.database.clone();
        self.blocking(move || work(lock(&database)?.connection_mut()))
            .await
    }

    /// Runs work against the repositories, which shells out to git.
    pub async fn with_repositories<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce(&crate::git::Repositories) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let repositories = self.repositories();
        self.blocking(move || work(&repositories)).await
    }

    /// Reads without saying anything may have changed.
    ///
    /// For whoever is watching for changes: their reading is not one, and
    /// ringing for it would wake every other watcher to read in turn.
    pub async fn peek<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let database = self.database.clone();

        tokio::task::spawn_blocking(move || work(lock(&database)?.connection()))
            .await
            .map_err(|error| {
                WebError::Internal(anyhow::anyhow!("the work did not finish: {error}"))
            })?
            .map_err(WebError::from)
    }

    /// Runs synchronous I/O or expensive computation on the blocking pool.
    pub async fn blocking<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let done = tokio::task::spawn_blocking(work)
            .await
            .map_err(|error| {
                WebError::Internal(anyhow::anyhow!("the work did not finish: {error}"))
            })?
            .map_err(WebError::from);

        // Most work reads and changes nothing, and a stream woken for
        // nothing reads once and goes back to sleep: cheaper than
        // knowing which work wrote.
        self.changed.notify_waiters();

        done
    }

    /// Runs work that writes to the repositories, one writer at a time.
    pub async fn writing_git<T, F>(&self, work: F) -> Result<T, WebError>
    where
        F: FnOnce(&crate::git::Repositories) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let repositories = self.repositories();
        let git = self.git.clone();

        self.blocking(move || {
            let _held = git
                .lock()
                .map_err(|_| anyhow::anyhow!("a writer to the repositories failed"))?;

            work(&repositories)
        })
        .await
    }

    /// Where this instance keeps its repositories.
    pub fn repositories(&self) -> crate::git::Repositories {
        crate::git::Repositories::new(self.settings.paths.git_dir())
    }

    /// What the instance's policy says right now.
    pub async fn policy(&self) -> Result<Policy, WebError> {
        self.read(crate::store::settings::policy).await
    }

    /// Hands one message to the instance's mail server.
    ///
    /// Nothing depends on this working. An instance with no way out is
    /// an ordinary instance, and a message that does not go is said so
    /// to the person who asked for it, with the link still in front of
    /// them.
    pub async fn deliver(&self, to: &str, subject: &str, body: &str) -> Delivered {
        let policy = match self.policy().await {
            Ok(policy) => policy,
            Err(_) => return Delivered::NoWayOut,
        };
        let named = crate::web::front::instance_name(self, &policy);
        let Some(post) = crate::mail::Post::of(&policy, &named) else {
            return Delivered::NoWayOut;
        };

        let (to, subject, body) = (to.to_owned(), subject.to_owned(), body.to_owned());
        let sent = tokio::task::spawn_blocking(move || post.send(&to, &subject, &body)).await;

        match sent {
            Ok(Ok(())) => Delivered::Sent,
            Ok(Err(why)) => {
                tracing::warn!("a message did not go out: {why:#}");

                Delivered::Refused(why.to_string())
            }
            Err(error) => {
                tracing::warn!("a message did not go out: {error}");

                Delivered::Refused("the instance could not finish sending it".to_owned())
            }
        }
    }
}

/// Locks the database, failing if a panic poisoned it.
fn lock(database: &Mutex<Database>) -> Result<std::sync::MutexGuard<'_, Database>> {
    database.lock().map_err(|_| {
        anyhow::anyhow!("the database is unusable because a request panicked holding it")
    })
}

/// Everything the instance serves over HTTP.
///
/// The state is attached by the caller, so routes that are not part of the
/// web can be added beside these first.
pub fn router() -> Router<Instance> {
    Router::new()
        .merge(pages())
        .merge(api())
        .merge(git())
        .merge(assets())
        .fallback(get(not_found))
        .layer(axum::middleware::from_fn(in_the_form_it_was_asked_for))
        .layer(axum::middleware::from_fn(appearance::remember))
}

/// Server-rendered pages and the forms they post.
fn pages() -> Router<Instance> {
    Router::new()
        .route("/", get(front::door))
        .route("/login", get(auth::login_form).post(auth::log_in))
        .route("/register", get(auth::register_form).post(auth::register))
        .route("/logout", post(auth::log_out))
        .route("/appearance", post(appearance::choose))
        .route("/forgotten", get(reset::asking).post(reset::forgotten))
        .route("/reset/{secret}", get(reset::form).post(reset::set))
        .route("/device", get(device::form).post(device::answer))
        .route("/invite/{secret}", get(invite::offer))
        .route("/invite/{secret}/join", post(invite::join))
        .route("/invite/{secret}/register", post(invite::join_and_register))
        .route(
            "/groups/new",
            get(groups::request_form).post(groups::request),
        )
        .route("/profile", get(profile::view))
        .route("/profile/details", post(profile::details))
        .route("/profile/password", post(profile::password))
        .route("/profile/picture", post(profile::picture))
        .route("/profile/picture/remove", post(profile::remove_picture))
        .route("/pictures/users/{slug}", get(pictures::user))
        .route("/pictures/groups/{slug}", get(pictures::group))
        .route("/releases/{slug}", get(releases::public_group))
        .route("/releases/{slug}/{version}", get(releases::documentation))
        .route(
            "/releases/{slug}/{version}/source/{*path}",
            get(releases::source),
        )
        .route(
            "/releases/{slug}/{version}/{line}/{block}",
            get(releases::result),
        )
        .route("/g/{slug}", get(groups::space))
        .route(
            "/g/{slug}/releases",
            get(releases::manage).post(releases::publish),
        )
        .route("/g/{slug}/edit", get(groups::edit_form).post(groups::edit))
        .route("/g/{slug}/picture", post(groups::picture))
        .route("/g/{slug}/picture/remove", post(groups::remove_picture))
        .route("/g/{slug}/invitations", post(groups::invite))
        .route(
            "/g/{slug}/invitations/{id}/withdraw",
            post(groups::withdraw),
        )
        .route("/g/{slug}/lines/new", get(starting::form))
        .route("/g/{slug}/lines", post(starting::start))
        .route(
            "/g/{slug}/private/{line}/share",
            get(starting::share_form).post(starting::share),
        )
        .route("/g/{slug}/private/{line}/delete", post(starting::remove))
        .route("/g/{slug}/vault", get(vault::page_view))
        .route("/g/{slug}/vault/entries", post(vault::admit))
        .route("/g/{slug}/vault/bridges", post(vault::bridge))
        .route("/g/{slug}/vault/audits", post(vault::audit))
        .route("/g/{slug}/vault/obligations", post(vault::assign))
        .route("/audit-obligations/{id}/decide", post(vault::decide))
        .route("/g/{slug}/{side}/{line}", get(lines::view))
        .route("/g/{slug}/{side}/{line}/page", get(revisions::page))
        .route("/g/{slug}/{side}/{line}/line.json", get(revisions::source))
        .route("/g/{slug}/{side}/{line}/jobs", post(jobs::launch))
        .route(
            "/g/{slug}/{side}/{line}/claim/release",
            post(claims::hand_back),
        )
        .route("/proposals/{id}", get(proposals::review))
        .route("/machines/{id}", get(machines::panel))
        .route("/machines/{id}/serve", post(machines::serve))
        .route("/machines/{id}/retire", post(machines::retire))
        .route("/machines/{id}/jobs/{job}/clear", post(machines::clear))
        .route("/admin", get(admin::area))
        .route(
            "/admin/authenticate",
            get(admin::authenticate_form).post(admin::authenticate),
        )
        .route("/admin/settings", post(admin::save_settings))
        .route("/admin/requests/{id}/approve", post(admin::approve))
        .route("/admin/requests/{id}/decline", post(admin::decline))
}

/// The JSON API the client and the daemon speak.
fn api() -> Router<Instance> {
    Router::new()
        .route("/api/me", get(api::me))
        .route("/api/logout", post(api::sign_out))
        .route("/api/workspace", get(api::workspace))
        .route("/api/events", get(events::stream))
        .route("/api/device/start", post(device::start))
        .route("/api/device/collect", post(device::collect))
        .route(
            "/api/lines",
            get(revisions::listing).post(starting::api_start),
        )
        .route(
            "/api/lines/{id}",
            get(revisions::read)
                .put(revisions::write)
                .layer(axum::extract::DefaultBodyLimit::max(
                    revisions::LARGEST_WRITE,
                )),
        )
        .route("/api/repositories/{group}/{side}", get(revisions::root))
        .route("/api/lines/{slug}/{line}/share", post(starting::api_share))
        .route(
            "/api/lines/{slug}/{line}/delete",
            post(starting::api_remove),
        )
        .route("/api/claims", get(claims::listing).post(claims::take))
        .route("/api/claims/renew", post(claims::renew))
        .route("/api/claims/release", post(claims::release))
        .route("/api/claims/ask", post(claims::ask))
        .route(
            "/api/machines",
            get(machines::mine).post(machines::register),
        )
        .route("/api/machines/beat", post(machines::beat))
        .route("/api/machines/retire", post(machines::api_retire))
        .route("/api/jobs", get(jobs::listing).post(jobs::start))
        .route("/api/jobs/take", post(jobs::take))
        .route("/api/jobs/{id}/finish", post(jobs::finish))
        .route("/api/jobs/{id}/progress", post(jobs::progress))
        .route("/api/jobs/{id}/cancel", post(jobs::cancel))
        .route("/api/jobs/{id}/clear", post(jobs::clear))
        .route("/api/proposals", get(proposals::listing))
        .route(
            "/api/proposals/history/{group}/{side}/{line}",
            get(proposals::history),
        )
        .route("/api/proposals/accepted", get(proposals::to_apply))
        .route("/api/proposals/{id}", get(proposals::accepted))
        .route("/api/proposals/{id}/applied", post(proposals::applied))
        .route("/api/proposals/{id}/outdated", post(proposals::outdated))
        .route("/api/proposals/{id}/review", get(proposals::offered))
        .route("/api/proposals/{id}/decide", post(proposals::api_decide))
        .route("/api/proposals/{id}/handoff", post(proposals::api_handoff))
        .route("/api/groups/{group}/machines", get(jobs::machine_choices))
        .route("/api/groups/{group}/pins", post(groups::change_pins))
        .route("/api/groups/{group}/vault", get(vault::api_view))
        .route("/api/groups/{group}/vault/entries", post(vault::api_admit))
        .route("/api/groups/{group}/vault/bridges", post(vault::api_bridge))
        .route("/api/groups/{group}/vault/audits", post(vault::api_audit))
        .route(
            "/api/groups/{group}/{side}/artefacts/{kind}/{commit}",
            get(artefacts::send).put(artefacts::receive),
        )
}

/// Git's smart HTTP transport for the group repositories.
fn git() -> Router<Instance> {
    Router::new().route(
        "/git/{group}/{*rest}",
        get(transport::serve).post(transport::serve),
    )
}

/// Stylesheet, faces, scripts, textures, the mark and the icons.
fn assets() -> Router<Instance> {
    let router = Router::new()
        .route(assets::STYLESHEET_PATH, get(assets::stylesheet))
        .route(assets::LANDING_PATH, get(assets::landing))
        .route(assets::MARK_ANIMATED_PATH, get(assets::mark_animated))
        .route(assets::SYMBOL_PATH, get(assets::symbol))
        .route(assets::SHELL_PATH, get(assets::shell))
        .route(assets::GLYPHS_PATH, get(assets::glyphs))
        .route(assets::NOISE_PATH, get(assets::noise))
        .route(assets::MARK_ON_DARK_PATH, get(assets::mark_on_dark))
        .route(assets::MARK_ON_LIGHT_PATH, get(assets::mark_on_light))
        .route(assets::MANIFEST_PATH, get(assets::manifest))
        .route(assets::FONT_PATH, get(assets::font));

    stemma_identity::WEB_ICONS
        .iter()
        .fold(router, |router, (name, _)| {
            router.route(
                &format!("/{name}"),
                get(move |headers: axum::http::HeaderMap| async move {
                    assets::icon(name, &headers)
                }),
            )
        })
}

/// What became of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivered {
    /// The mail server took it.
    Sent,
    /// This instance has no mail server, which is not a fault.
    NoWayOut,
    /// It has one, and it would not take the message.
    Refused(String),
}

impl Delivered {
    /// What to tell whoever asked for it, when there is anything to tell.
    pub fn said(&self, to: &str) -> Option<String> {
        match self {
            Self::Sent => Some(format!("It has been sent to {to}.")),
            Self::NoWayOut => None,
            Self::Refused(why) => Some(format!(
                "It could not be sent to {to} ({why}), so the link below is the only copy."
            )),
        }
    }
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
    Internal(anyhow::Error),
}

/// What a piece of work that failed comes to.
///
/// Work fails with an [`anyhow::Error`]. A refusal it raised on purpose,
/// with `anyhow::bail!(WebError::…)`, travels inside that error and comes
/// back out as itself, so a check made in the same breath as the work it
/// guards can still answer as a missing page or a bad request; anything
/// else is the server failing to do its part.
impl From<anyhow::Error> for WebError {
    fn from(error: anyhow::Error) -> Self {
        match error.downcast::<Self>() {
            Ok(refusal) => refusal,
            Err(error) => Self::Internal(error),
        }
    }
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
            Self::NotFound => "The requested page could not be found.".to_owned(),
            Self::Forbidden(reason) | Self::BadRequest(reason) => reason.clone(),
            Self::Internal(_) => "Something went wrong. Please try again later.".to_owned(),
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        if let Self::Internal(error) = &self {
            tracing::error!("{error:#}");
        }

        let mut response = (self.status(), layout::failure(&self)).into_response();
        response.extensions_mut().insert(Refusal(self.explain()));

        response
    }
}

/// What a refusal said, carried beside the page that says it.
///
/// The sentence is the same for everybody; only its wrapping differs. So
/// it travels on the response, and whoever is about to send it decides
/// whether it goes out as a page or as a line.
#[derive(Debug, Clone)]
pub struct Refusal(pub String);

/// Gives a program the sentence and a person the page.
///
/// Routes under `/api` are read by programs, and a program handed a
/// document ends up showing somebody the navigation bar of an error
/// page. The same refusal, as one field, is what it can use.
pub async fn in_the_form_it_was_asked_for(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let a_program_asked = request.uri().path().starts_with("/api/");
    let response = next.run(request).await;

    let Some(Refusal(said)) = response.extensions().get::<Refusal>().cloned() else {
        return response;
    };

    match a_program_asked {
        true => (
            response.status(),
            axum::Json(serde_json::json!({ "error": said })),
        )
            .into_response(),
        false => response,
    }
}

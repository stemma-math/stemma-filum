//! Running the server.
//!
//! `serve` runs in the foreground. It does not fork, write a pid file or put
//! itself in the background: whatever supervises it — systemd on a host, the
//! runtime in a container — needs a process it can watch, and a program that
//! disappears into the background is a program its supervisor has lost. For
//! the same reason the log goes to standard output, where the journal picks
//! it up, and never to a file somebody would have to rotate.
//!
//! Before binding anything it runs the same check an operator runs by hand,
//! and refuses to start on a fatal finding.

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use serde_json::json;
use stemma_core::{PROTOCOL_HEADER, PROTOCOL_VERSION};

use crate::check;
use crate::config::{Listen, Settings};
use crate::db::{self, Database};
use crate::tokens::Key;
use crate::web::{self, Instance};

/// Runs the server until it is asked to stop.
pub fn serve(settings: Settings, service_user: &str) -> Result<()> {
    start_logging();

    let report = check::inspect(&settings, service_user);

    if !report.allows_startup(false) {
        eprintln!("{report}");
        bail!("refusing to start");
    }

    for finding in report.ordered() {
        tracing::warn!("{}", finding.message);
    }

    let database = Database::open_current(&settings.paths.database())?;

    let key = Key::read(&settings.paths.session_key())?;
    let instance = Instance::new(settings, database, key);

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the runtime")?
        .block_on(run(instance))
}

/// Sends the log where the supervisor is already looking.
fn start_logging() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .with_target(false)
        .init();
}

/// Binds the address and serves until interrupted.
async fn run(instance: Instance) -> Result<()> {
    let listen = instance.settings.listen.clone();
    let public_url = instance.settings.public_url.clone();
    tokio::spawn(maintain(instance.clone()));
    let router = router(instance);

    tracing::info!("listening on {listen}, answering as {public_url}");

    match listen {
        Listen::Tcp(address) => {
            let listener = tokio::net::TcpListener::bind(address)
                .await
                .with_context(|| format!("cannot bind {address}"))?;

            axum::serve(listener, router)
                .with_graceful_shutdown(interrupted())
                .await
                .context("the server stopped unexpectedly")
        }
        Listen::Unix(path) => {
            // A socket left behind by a process that was killed would stop
            // this one from binding, and it names nothing that is listening.
            let _ = std::fs::remove_file(&path);

            let listener = tokio::net::UnixListener::bind(&path)
                .with_context(|| format!("cannot bind {}", path.display()))?;

            open_to_the_group(&path)?;

            axum::serve(listener, router)
                .with_graceful_shutdown(interrupted())
                .await
                .context("the server stopped unexpectedly")
        }
    }
}

/// Lets the reverse proxy reach the socket, and nobody else.
/// Housekeeping nobody asks for, on a timer, for as long as the server
/// runs. A round that fails is logged and the next one tries again.
async fn maintain(instance: Instance) {
    let mut every = tokio::time::interval(std::time::Duration::from_secs(30));
    let mut rounds: u64 = 0;

    loop {
        every.tick().await;
        rounds += 1;

        // Writing that went quiet is committed within a minute of the
        // ten it is allowed.
        if let Err(error) = crate::history::tidy(&instance).await {
            tracing::warn!("cannot commit the writing that went quiet: {error}");
        }

        match crate::history::ask_for_builds(&instance).await {
            Ok(0) => {}
            Ok(asked) => tracing::info!("asked for {asked} builds nobody published"),
            Err(error) => tracing::warn!("cannot ask for builds: {error}"),
        }

        if rounds % 120 == 1 {
            match instance.read(crate::store::events::forget_old).await {
                Ok(0) => {}
                Ok(forgotten) => tracing::info!("forgot {forgotten} old events"),
                Err(error) => tracing::warn!("cannot forget old events: {error}"),
            }
        }
    }
}

fn open_to_the_group(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))
            .with_context(|| format!("cannot set the permissions of {}", path.display()))?;
    }

    #[cfg(not(unix))]
    let _ = path;

    Ok(())
}

/// Resolves when the supervisor asks the server to stop.
async fn interrupted() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::error!("cannot listen for a stop signal: {error}");
            return;
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }

    tracing::info!("stopping");
}

/// What the server answers.
fn router(instance: Instance) -> Router {
    web::router()
        .route("/health", get(health))
        .route("/api/version", get(version))
        .layer(axum::middleware::from_fn(announce_protocol))
        .with_state(instance)
}
/// Every answer says which protocol it speaks, so a client never has to ask
/// separately before it can tell whether it is talking to something it
/// understands.
async fn announce_protocol(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;

    if let Ok(value) = HeaderValue::from_str(&PROTOCOL_VERSION.to_string()) {
        response.headers_mut().insert(PROTOCOL_HEADER, value);
    }

    response
}

/// Whether the instance is answering, for whatever watches it.
async fn health(State(instance): State<Instance>) -> impl IntoResponse {
    let schema = instance
        .database
        .lock()
        .ok()
        .and_then(|database| database.version().ok());

    let healthy = schema == Some(db::expected_version());

    let status = if healthy {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "healthy": healthy,
            "schema": schema,
        })),
    )
}

/// What this instance is and what it speaks.
async fn version(State(instance): State<Instance>) -> impl IntoResponse {
    Json(json!({
        "name": "stemma-server",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": PROTOCOL_VERSION.to_string(),
        "public_url": instance.settings.public_url.as_str(),
    }))
}

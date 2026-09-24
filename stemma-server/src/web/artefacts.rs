//! Taking in what machines build.
//!
//! The instance compiles nothing. What it does is keep what somebody
//! else's machine built, named by the commit it came from, so that a page
//! can show a line to a reader whose own machine is switched off.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;
use stemma_core::artefact::{self, Kind, Scope};
use stemma_core::id::{GroupId, Slug};
use stemma_core::repository::Side;

use super::api::Credential;
use super::{Instance, WebError};
use crate::artefacts::{LARGEST, Shelf};
use crate::store::{artefacts as index, groups};

/// What a machine sends, and where it goes.
type Address = Path<(String, String, String, String)>;

/// Takes one artefact in.
pub async fn receive(
    State(instance): State<Instance>,
    machine: Credential,
    Path((group, side, kind, commit)): Address,
    body: Body,
) -> Result<Response, WebError> {
    let (group, scope, kind) = admit(&instance, &machine, &group, &side, &kind, &commit).await?;

    let content = axum::body::to_bytes(body, LARGEST)
        .await
        .map_err(|_| WebError::BadRequest("that artefact is too large".to_owned()))?;

    // What arrives is read before it is kept: an artefact nothing can
    // parse would be a page that fails when somebody opens it, days
    // later, with nothing to say about why.
    let text = std::str::from_utf8(&content)
        .map_err(|_| WebError::BadRequest("that artefact is not text".to_owned()))?;
    let nodes =
        artefact::Nodes::parse(text).map_err(|error| WebError::BadRequest(error.to_string()))?;

    if nodes.group != group {
        return Err(WebError::BadRequest(format!(
            "that artefact says it belongs to {}, and it was sent to {group}",
            nodes.group
        )));
    }
    if nodes.commit != commit {
        return Err(WebError::BadRequest(format!(
            "that artefact says it was built from {}, and it was sent as {commit}",
            nodes.commit
        )));
    }

    let shelf = Shelf::new(instance.settings.paths.artefacts_dir());
    let bytes = content.len();

    {
        let group = group.clone();
        let scope = scope.clone();
        let commit = commit.clone();

        instance
            .blocking(move || {
                shelf
                    .put(&group, &scope, kind, &commit, &content)
                    .map(|_| ())
            })
            .await?;
    }

    let by = machine.user.id.clone();
    let recorded = instance
        .read(move |connection| {
            index::record(connection, &group, &scope, kind, &commit, bytes, &by)
        })
        .await?;

    tracing::info!(
        "kept {} for {} at {} ({bytes} bytes)",
        recorded.kind,
        recorded.scope,
        recorded.commit
    );

    Ok((
        StatusCode::CREATED,
        Json(json!({ "kept": true, "bytes": bytes })),
    )
        .into_response())
}

/// Hands one back.
pub async fn send(
    State(instance): State<Instance>,
    machine: Credential,
    Path((group, side, kind, commit)): Address,
) -> Result<Response, WebError> {
    let (group, scope, kind) = admit(&instance, &machine, &group, &side, &kind, &commit).await?;

    let shelf = Shelf::new(instance.settings.paths.artefacts_dir());
    let found = instance
        .blocking(move || shelf.get(&group, &scope, kind, &commit))
        .await?;

    match found {
        Some(content) => Ok((
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            content,
        )
            .into_response()),
        None => Err(WebError::NotFound),
    }
}

/// Works out what was asked for, and whether this machine may have it.
async fn admit(
    instance: &Instance,
    machine: &Credential,
    group: &str,
    side: &str,
    kind: &str,
    commit: &str,
) -> Result<(GroupId, Scope, Kind), WebError> {
    let Ok(slug) = Slug::parse(group) else {
        return Err(WebError::NotFound);
    };
    let Some(kind) = Kind::named(kind) else {
        return Err(WebError::NotFound);
    };

    let side = match side {
        "shared" => Side::Shared,
        "private" => Side::Private,
        _ => return Err(WebError::NotFound),
    };

    if !artefact::is_commit(commit) {
        return Err(WebError::BadRequest(
            "that is not a commit this instance can name a file after".to_owned(),
        ));
    }

    let group = GroupId::new(slug);

    // Both sides need membership: a private repository only exists
    // inside a group somebody belongs to. What the side decides is whose
    // shelf it is, and a machine can only ever reach its owner's.
    let id = group.clone();
    let asking = machine.user.id.clone();
    let role = instance
        .read(move |connection| groups::role_of(connection, &id, &asking))
        .await?;

    if role.is_none() {
        return Err(WebError::NotFound);
    }

    Ok((group, Scope::of(side, &machine.user.id), kind))
}

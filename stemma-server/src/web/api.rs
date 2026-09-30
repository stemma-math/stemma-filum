//! What a program on somebody's machine talks to.
//!
//! A machine does not hold a browser session: it holds a credential of its
//! own, which can be taken back without signing its owner out of anything
//! else. It carries it the ordinary way, as a bearer token.

use axum::extract::{FromRequestParts, State};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;
use stemma_core::PROTOCOL_VERSION;

use super::{Instance, WebError, transport};
use crate::store::devices;
use crate::store::users::{self, User};

/// A credential a program on somebody's machine is carrying.
///
/// Whether that machine has also registered as one that runs jobs is a
/// separate question: signing in and offering to work are two acts, and
/// a laptop somebody only writes on does neither the second.
#[derive(Debug, Clone)]
pub struct Credential {
    /// The account it acts for.
    pub user: User,
    /// The row it was issued as, which is what a machine is registered
    /// against.
    pub token_id: String,
    /// The credential itself, so it can be given back.
    pub token: String,
}

impl FromRequestParts<Instance> for Credential {
    type Rejection = WebError;

    async fn from_request_parts(
        parts: &mut Parts,
        instance: &Instance,
    ) -> Result<Self, Self::Rejection> {
        let refuse =
            || WebError::Forbidden("This needs a credential from `stemma login`.".to_owned());

        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or_else(refuse)?
            .to_owned();

        let key = instance.key.clone();
        let carried = token.clone();

        let found = instance
            .read(move |connection| {
                let Some((token_id, id)) = devices::credential(connection, &key, &carried)? else {
                    return Ok(None);
                };

                Ok(users::by_id(connection, &id)?.map(|user| (token_id, user)))
            })
            .await?;

        match found.filter(|(_, user)| user.is_active()) {
            Some((token_id, user)) => Ok(Self {
                user,
                token_id,
                token,
            }),
            None => Err(refuse()),
        }
    }
}

/// Who this credential acts for.
pub async fn me(
    State(instance): State<Instance>,
    machine: Credential,
) -> Result<Response, WebError> {
    let user = machine.user.id.clone();
    let appearance = instance
        .read(move |connection| users::appearance(connection, &user))
        .await?;
    Ok(Json(json!({
        "user": machine.user.id.to_string(),
        "display_name": machine.user.display_name,
        "protocol": PROTOCOL_VERSION.to_string(),
        "appearance": appearance,
    }))
    .into_response())
}

/// The groups this credential can work in, and where their repositories
/// are.
///
/// The client asks for this rather than being configured with it: what a
/// person is in changes when somebody invites them, and a list of remotes
/// kept by hand goes stale the moment it matters.
pub async fn workspace(
    State(instance): State<Instance>,
    machine: Credential,
) -> Result<Response, WebError> {
    let public_url = instance.settings.public_url.clone();
    let user = machine.user.id.clone();

    let memberships = instance
        .read(move |connection| crate::store::groups::for_user(connection, &user))
        .await?;

    let groups: Vec<_> = memberships
        .iter()
        .map(|membership| {
            let slug = membership.group.slug();

            json!({
                "id": membership.group.id.to_string(),
                "slug": slug.as_str(),
                "name": membership.group.name,
                "role": membership.role.as_str(),
                "shared": public_url.link(&transport::shared_path(slug)),
                "personal": public_url
                    .link(&transport::personal_path(slug, &machine.user.id)),
            })
        })
        .collect();

    Ok(Json(json!({
        "user": machine.user.id.to_string(),
        "groups": groups,
    }))
    .into_response())
}

/// Gives a credential back.
pub async fn sign_out(
    State(instance): State<Instance>,
    machine: Credential,
) -> Result<Response, WebError> {
    let key = instance.key.clone();
    let token = machine.token.clone();

    instance
        .read(move |connection| devices::revoke(connection, &key, &token))
        .await?;

    Ok(Json(json!({ "revoked": true })).into_response())
}

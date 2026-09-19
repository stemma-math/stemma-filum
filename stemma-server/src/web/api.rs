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

use super::{Instance, WebError};
use crate::store::devices;
use crate::store::users::{self, User};

/// A machine acting for somebody.
#[derive(Debug, Clone)]
pub struct Machine {
    /// The account it acts for.
    pub user: User,
    /// The credential it carried, so it can be given back.
    pub token: String,
}

impl FromRequestParts<Instance> for Machine {
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
                let Some(id) = devices::holder(connection, &key, &carried)? else {
                    return Ok(None);
                };

                users::by_id(connection, &id)
            })
            .await?;

        match found.filter(User::is_active) {
            Some(user) => Ok(Self { user, token }),
            None => Err(refuse()),
        }
    }
}

/// Who this credential acts for.
pub async fn me(machine: Machine) -> Response {
    Json(json!({
        "user": machine.user.id.to_string(),
        "display_name": machine.user.display_name,
        "protocol": PROTOCOL_VERSION.to_string(),
    }))
    .into_response()
}

/// Gives a credential back.
pub async fn sign_out(
    State(instance): State<Instance>,
    machine: Machine,
) -> Result<Response, WebError> {
    let key = instance.key.clone();
    let token = machine.token.clone();

    instance
        .read(move |connection| devices::revoke(connection, &key, &token))
        .await?;

    Ok(Json(json!({ "revoked": true })).into_response())
}

//! Serving pictures, and drawing somebody by theirs or by their initials.
//!
//! A picture is only served to somebody signed in, and a group's only to
//! its members, as the group's page is. Its address carries when it last
//! changed, so a browser may keep it for as long as it likes: a new
//! picture is a new address.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};
use stemma_core::id::{GroupId, Slug, UserId};

use super::layout::initials;
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups;
use crate::store::pictures::{self, Picture};

/// Where somebody's picture is, as of when it changed.
pub fn user_address(user: &UserId, changed: &str) -> String {
    format!("/pictures/users/{}?v={}", user.slug(), version(changed))
}

/// Where a group's picture is, as of when it changed.
pub fn group_address(group: &GroupId, changed: &str) -> String {
    format!("/pictures/groups/{}?v={}", group.slug(), version(changed))
}

/// A change time as a query value: its digits.
fn version(changed: &str) -> String {
    changed.chars().filter(char::is_ascii_digit).collect()
}

/// A person: their picture where they have one, their initials where not.
pub fn face(user: &UserId, name: &str, changed: Option<&str>, class: &str) -> Markup {
    html! {
        span class={ "avatar " (class) } aria-hidden="true" {
            @match changed {
                Some(changed) => img src=(user_address(user, changed)) alt="";
                None => (initials(name)),
            }
        }
    }
}

/// A group: its picture where it has one, its initials where not.
pub fn badge(group: &GroupId, name: &str, changed: Option<&str>, class: &str) -> Markup {
    html! {
        span class=(class) aria-hidden="true" {
            @match changed {
                Some(changed) => img src=(group_address(group, changed)) alt="";
                None => (initials(name)),
            }
        }
    }
}

/// Somebody's picture.
pub async fn user(
    State(instance): State<Instance>,
    _signed: SignedIn,
    Path(slug): Path<String>,
) -> Result<Response, WebError> {
    let slug = Slug::parse(&slug).map_err(|_| WebError::NotFound)?;
    let user = UserId::new(slug);

    let found = instance
        .read(move |connection| pictures::of_user(connection, &user))
        .await?;

    found.map(send).ok_or(WebError::NotFound)
}

/// A group's picture, for somebody in the group.
pub async fn group(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
) -> Result<Response, WebError> {
    let slug = Slug::parse(&slug).map_err(|_| WebError::NotFound)?;
    let group = GroupId::new(slug);
    let reader = signed.user.id;

    let found = instance
        .read(move |connection| {
            // Somebody outside the group is told there is nothing here, as
            // the group's page tells them.
            if groups::role_of(connection, &group, &reader)?.is_none() {
                return Ok(None);
            }

            pictures::of_group(connection, &group)
        })
        .await?;

    found.map(send).ok_or(WebError::NotFound)
}

/// The picture, said to be exactly what its bytes are, and nothing a
/// browser could be talked into running.
fn send(picture: Picture) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(picture.media_type),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );

    (headers, picture.bytes).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_address_changes_when_the_picture_does() {
        let ana: UserId = "usr:ana".parse().unwrap();

        assert_eq!(
            user_address(&ana, "2026-09-28 06:49:23"),
            "/pictures/users/ana?v=20260928064923"
        );
        assert_ne!(
            user_address(&ana, "2026-09-28 06:49:23"),
            user_address(&ana, "2026-09-28 06:50:00")
        );
    }

    #[test]
    fn somebody_without_a_picture_is_their_initials() {
        let ana: UserId = "usr:ana".parse().unwrap();

        let drawn = face(&ana, "Ana Ferrer", None, "").into_string();
        assert!(drawn.contains(">AF<"), "{drawn}");
        assert!(!drawn.contains("<img"));

        let drawn = face(&ana, "Ana Ferrer", Some("2026-09-28 06:49:23"), "").into_string();
        assert!(
            drawn.contains("<img src=\"/pictures/users/ana?v=20260928064923\""),
            "{drawn}"
        );
    }
}

//! Who is reading.
//!
//! A page asks for [`Visitor`] when it can be read by anybody and for
//! [`SignedIn`] when it cannot. Asking for the second is what redirects
//! somebody to the log-in form, so no page has to remember to check.

use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect, Response};
use stemma_core::access::Grant;

use super::{Instance, WebError, cookies};
use crate::store::sessions::{self, Session};
use crate::store::users::{self, User};

/// Somebody with a session.
#[derive(Debug, Clone)]
pub struct SignedIn {
    /// The session their browser holds.
    pub session: Session,
    /// The account it belongs to.
    pub user: User,
    /// Whether they operate this instance.
    pub maintainer: bool,
}

impl SignedIn {
    /// Checks that a form came from a page this session was served.
    ///
    /// Without it, another site could have somebody's browser post here
    /// while carrying their cookie.
    pub fn check_form(&self, submitted: &str) -> Result<(), WebError> {
        match crate::tokens::same_secret(&self.session.csrf_token, submitted) {
            true => Ok(()),
            false => Err(WebError::BadRequest(
                "That form is out of date. Open the page again and try once more.".to_owned(),
            )),
        }
    }
}

/// Whoever is reading, signed in or not.
#[derive(Debug, Clone, Default)]
pub struct Visitor(pub Option<SignedIn>);

impl Visitor {
    /// The account, where there is one.
    pub fn user(&self) -> Option<&User> {
        self.0.as_ref().map(|signed| &signed.user)
    }

    /// Whether the reader operates this instance.
    pub fn is_maintainer(&self) -> bool {
        self.0.as_ref().is_some_and(|signed| signed.maintainer)
    }
}

impl FromRequestParts<Instance> for Visitor {
    type Rejection = WebError;

    async fn from_request_parts(
        parts: &mut Parts,
        instance: &Instance,
    ) -> Result<Self, Self::Rejection> {
        let Some(cookie) = cookies::read(&parts.headers, cookies::SESSION).map(str::to_owned)
        else {
            return Ok(Self(None));
        };

        let key = instance.key.clone();

        let found = instance
            .read(move |connection| {
                let Some((session, user)) = sessions::lookup(connection, &key, &cookie)? else {
                    return Ok(None);
                };

                sessions::touch(connection, &session.id)?;
                let maintainer = users::holds(connection, &user.id, Grant::Maintainer)?;

                Ok(Some(SignedIn {
                    session,
                    user,
                    maintainer,
                }))
            })
            .await?;

        Ok(Self(found))
    }
}

impl FromRequestParts<Instance> for SignedIn {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        instance: &Instance,
    ) -> Result<Self, Self::Rejection> {
        let visitor = Visitor::from_request_parts(parts, instance)
            .await
            .map_err(IntoResponse::into_response)?;

        visitor.0.ok_or_else(|| {
            let wanted = parts.uri.path();
            let destination = match wanted {
                "/" => "/login".to_owned(),
                path => format!("/login?next={}", encode(path)),
            };

            Redirect::to(&destination).into_response()
        })
    }
}

impl OptionalFromRequestParts<Instance> for SignedIn {
    type Rejection = WebError;

    async fn from_request_parts(
        parts: &mut Parts,
        instance: &Instance,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(Visitor::from_request_parts(parts, instance).await?.0)
    }
}

/// Somebody who operates this instance, and has just proved who they are.
///
/// Holding a session is not the same as having typed the password
/// recently, and the second is what the admin area asks for: a laptop left
/// open is a session, not a person.
#[derive(Debug, Clone)]
pub struct Maintainer(pub SignedIn);

impl FromRequestParts<Instance> for Maintainer {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        instance: &Instance,
    ) -> Result<Self, Self::Rejection> {
        let signed =
            <SignedIn as FromRequestParts<Instance>>::from_request_parts(parts, instance).await?;

        if !signed.maintainer {
            return Err(WebError::Forbidden(
                "The admin area is for the people who operate this instance.".to_owned(),
            )
            .into_response());
        }

        let session = signed.session.id;
        let fresh = instance
            .read(move |connection| sessions::is_fresh(connection, &session))
            .await
            .map_err(IntoResponse::into_response)?;

        if !fresh {
            let destination = format!("/admin/authenticate?next={}", encode(parts.uri.path()));
            return Err(Redirect::to(&destination).into_response());
        }

        Ok(Self(signed))
    }
}

/// Escapes a path so it survives being a query parameter.
fn encode(path: &str) -> String {
    path.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Where to go after signing in.
///
/// Only a path on this instance is accepted: a `next` that names another
/// host is how a log-in form is turned into a way of sending people
/// somewhere they did not mean to go.
pub fn safe_destination(next: Option<&str>) -> String {
    next.filter(|path| {
        path.starts_with('/')
            && !path.starts_with("//")
            && !path.contains('\\')
            && !path.chars().any(char::is_control)
    })
    .unwrap_or("/")
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_on_this_instance_is_where_we_go_back_to() {
        assert_eq!(safe_destination(Some("/admin")), "/admin");
        assert_eq!(safe_destination(Some("/g/algebra")), "/g/algebra");
    }

    #[test]
    fn a_destination_somewhere_else_is_refused() {
        for elsewhere in [
            "//example.org",
            "/\\example.org",
            "/\t/example.org",
            "/\n/example.org",
            "/admin\r\nLocation: https://example.org",
            "/admin\0",
            "https://example.org",
            "javascript:alert(1)",
            "admin",
            "",
        ] {
            assert_eq!(safe_destination(Some(elsewhere)), "/", "took `{elsewhere}`");
        }

        assert_eq!(safe_destination(None), "/");
    }

    #[test]
    fn a_path_survives_being_a_query_parameter() {
        assert_eq!(encode("/g/algebra"), "/g/algebra");
        assert_eq!(encode("/g/a b"), "/g/a%20b");
        assert_eq!(encode("/?x=1"), "/%3Fx%3D1");
    }
}

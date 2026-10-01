//! Lines, a revision at a time, for the machines that keep them.
//!
//! A machine asks which lines it can see and at which revision each is,
//! fetches the ones that moved, and writes back a revision at a time
//! against the one it started from. Lines are named by identifier here,
//! not by where they are, so a line that moved to the group while a
//! machine was away is still the same line to it.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;
use stemma_core::id::{GroupId, LineId, Slug};
use stemma_core::line::Name;
use stemma_core::repository::Side;

use super::api::Credential;
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups;
use crate::store::lines::{self, Started};
use crate::store::revisions::{self, Content, Offered, Refused, Revision};

/// The largest write a machine may send: the page, the line and its
/// Lean module, with room for what surrounds them.
pub const LARGEST_WRITE: usize =
    revisions::LARGEST_PAGE + 2 * revisions::LARGEST_SOURCE + 64 * 1024;

/// Every line this machine's owner can see, and the revision each is at.
pub async fn listing(
    State(instance): State<Instance>,
    credential: Credential,
) -> Result<Response, WebError> {
    let reader = credential.user.id.clone();
    let found = instance
        .read(move |connection| lines::visible_to(connection, &reader))
        .await?;

    let listed: Vec<serde_json::Value> = found.iter().map(described).collect();

    Ok(Json(json!({ "lines": listed })).into_response())
}

/// Which revision a machine wants, when it wants an older one.
#[derive(Debug, Default, Deserialize)]
pub struct Which {
    /// The revision; the current one when left out.
    pub revision: Option<u64>,
}

/// One revision of one line.
///
/// A line that was its author's before it was shared keeps the revisions
/// it had then, and those stay its author's: sharing hands the group the
/// line as it stands, and whatever history travelled with it did so by
/// the author's choice, not to everybody who can ask for a number.
pub async fn read(
    State(instance): State<Instance>,
    credential: Credential,
    Path(id): Path<String>,
    Query(which): Query<Which>,
) -> Result<Response, WebError> {
    let line = seen(&instance, &credential, &id).await?;
    let number = which.revision.unwrap_or(line.revision);
    let at = line.id;

    let revision = instance
        .read(move |connection| revisions::at(connection, &at, number))
        .await?
        .filter(|revision| revision.side == Side::Shared || line.author == credential.user.id)
        .ok_or(WebError::NotFound)?;

    Ok(Json(revision_json(&line, &revision)).into_response())
}

/// What a machine sends to write a line.
#[derive(Debug, Deserialize)]
pub struct Written {
    /// The revision it wrote from.
    pub base: u64,
    /// The line, as its `line.json`.
    pub source: String,
    /// The Lean module beside it.
    #[serde(default)]
    pub lean_source: Option<String>,
    /// What it typesets to.
    pub page: String,
}

/// Writes a line.
pub async fn write(
    State(instance): State<Instance>,
    credential: Credential,
    Path(id): Path<String>,
    Json(written): Json<Written>,
) -> Result<Response, WebError> {
    let machine = super::machines::registered(&instance, &credential).await?;
    let line = seen(&instance, &credential, &id).await?;

    if !line.admits(&credential.user.id) {
        return Err(WebError::Forbidden(
            "Only its author can edit this line; others send proposals.".to_owned(),
        ));
    }

    let offered = Offered {
        base: written.base,
        content: Content {
            source: written.source,
            lean_source: written.lean_source,
            page: written.page,
        },
    };
    let writer = credential.user.id.clone();
    let at = line.id;

    let answer = instance
        .write(move |connection| revisions::write(connection, &at, &offered, &machine.id, &writer))
        .await?;

    Ok(match answer {
        Ok(written) => Json(json!({ "revision": written.revision })).into_response(),
        Err(refused) => refusal(&line, refused),
    })
}

/// A refused write, with what the machine needs to put it right.
fn refusal(line: &Started, refused: Refused) -> Response {
    let said = refused.to_string();

    match refused {
        Refused::Stale(current) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": said,
                "current": revision_json(line, &current),
            })),
        )
            .into_response(),
        Refused::NotHeld(holder) => (
            StatusCode::LOCKED,
            Json(json!({ "error": said, "held_by": holder })),
        )
            .into_response(),
        Refused::Gone => (StatusCode::GONE, Json(json!({ "error": said }))).into_response(),
        Refused::Unreadable(_) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": said })),
        )
            .into_response(),
    }
}

/// The files at the root of one of a group's repositories: the manifest
/// and whatever the group keeps there to check its lines with. A machine
/// puts them beside its lines, as they are.
pub async fn root(
    State(instance): State<Instance>,
    credential: Credential,
    Path((group, side)): Path<(String, String)>,
) -> Result<Response, WebError> {
    let group = super::membership::group_of(&instance, &group, &credential.user.id)
        .await?
        .id;
    let side: Side = side.parse().map_err(|_| WebError::NotFound)?;
    let path = match side {
        Side::Shared => instance.repositories().shared(&group),
        Side::Private => instance
            .repositories()
            .personal(&group, &credential.user.id),
    };

    let found = instance
        .blocking(move || crate::git::root_files(&path))
        .await?;

    let files: serde_json::Map<String, serde_json::Value> = found
        .into_iter()
        .map(|(name, content)| (name, serde_json::Value::String(content)))
        .collect();

    Ok(Json(json!({ "files": files })).into_response())
}

/// The current revision's line.json, downloaded exactly as the machine wrote it.
pub async fn source(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((slug, side, name)): Path<(String, String, String)>,
) -> Result<Response, WebError> {
    let revision = current(&instance, &signed, &slug, &side, &name).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"line.json\""),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-cache"),
    );
    Ok((headers, revision.source).into_response())
}

/// The page a line typesets to, for the web to show inside a frame.
///
/// The page came from a machine, so it is shown as it would be shown
/// from anywhere else: nothing in it runs, it loads nothing from
/// anywhere, and it can be shown by this instance's pages alone.
pub async fn page(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((slug, side, name)): Path<(String, String, String)>,
) -> Result<Response, WebError> {
    let revision = current(&instance, &signed, &slug, &side, &name).await?;
    let reader = signed.user.id.clone();
    let saved = instance
        .read(move |connection| crate::store::users::appearance(connection, &reader))
        .await?;
    let theme = saved
        .as_deref()
        .map(super::appearance::Appearance::parse)
        .unwrap_or_else(super::appearance::chosen)
        .theme();
    Ok((
        framed_page_headers(),
        themed(&styled_page(&revision.page), theme),
    )
        .into_response())
}

/// A page in the appearance its reader chose, where they chose one;
/// otherwise it follows their system, as the page around it does.
fn themed(page: &str, theme: Option<&str>) -> String {
    match (theme, page.find("<html")) {
        (Some(theme), Some(at)) => {
            let at = at + "<html".len();
            format!("{} data-theme=\"{theme}\"{}", &page[..at], &page[at..])
        }
        _ => page.to_owned(),
    }
}

/// Reads exactly the current revision after checking the reader's membership.
async fn current(
    instance: &Instance,
    signed: &SignedIn,
    slug: &str,
    side: &str,
    name: &str,
) -> Result<Revision, WebError> {
    let group = GroupId::new(Slug::parse(slug).map_err(|_| WebError::NotFound)?);
    let side: Side = side.parse().map_err(|_| WebError::NotFound)?;
    let name = Name::parse(name).map_err(|_| WebError::NotFound)?;
    let reader = signed.user.id.clone();

    instance
        .read(move |connection| {
            if groups::role_of(connection, &group, &reader)?.is_none() {
                return Ok(None);
            }
            let Some(line) = lines::by_name(connection, &group, side, &name, &reader)? else {
                return Ok(None);
            };

            revisions::current(connection, &line)
        })
        .await?
        .ok_or(WebError::NotFound)
}

/// Adds the identity's colours and the line's print rules to the page the
/// machine sent. Its content is served unchanged and is never compiled here.
fn styled_page(page: &str) -> String {
    let style = format!(
        "<style>\n{}\n{}\n</style>",
        stemma_identity::TOKENS_CSS,
        include_str!("../../assets/line.css")
    );
    if let Some(at) = page.find("</head>") {
        let mut styled = String::with_capacity(page.len() + style.len());
        styled.push_str(&page[..at]);
        styled.push_str(&style);
        styled.push_str(&page[at..]);
        styled
    } else {
        format!("{style}{page}")
    }
}

/// What a page from a machine is served with.
pub fn framed_page_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; \
             sandbox; frame-ancestors 'self'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-cache"),
    );
    headers
}

/// A line as it is listed.
fn described(line: &Started) -> serde_json::Value {
    json!({
        "id": line.id.to_string(),
        "group": line.group.slug().as_str(),
        "side": line.side.as_str(),
        "line": line.name.as_str(),
        "author": line.author.to_string(),
        "writing": line.writing.as_str(),
        "revision": line.revision,
        "deleted": line.is_deleted(),
    })
}

/// A revision as a machine receives it.
fn revision_json(line: &Started, revision: &Revision) -> serde_json::Value {
    json!({
        "id": line.id.to_string(),
        "group": line.group.slug().as_str(),
        "side": revision.side.as_str(),
        "line": line.name.as_str(),
        "revision": revision.number,
        "kind": revision.kind.as_str(),
        "source": revision.source,
        "lean_source": revision.lean_source,
        "author": revision.author.to_string(),
        "written_at": revision.written_at,
    })
}

/// The line a machine names, when its owner can see it.
///
/// A line somebody cannot see is not there, as it is not on the web.
async fn seen(instance: &Instance, credential: &Credential, id: &str) -> Result<Started, WebError> {
    let id: LineId = id.parse().map_err(|_| WebError::NotFound)?;
    let reader = credential.user.id.clone();

    instance
        .read(move |connection| {
            let Some(line) = lines::by_id(connection, &id)? else {
                return Ok(None);
            };
            let member = groups::role_of(connection, &line.group, &reader)?.is_some();

            Ok((member && line.visible_to(&reader)).then_some(line))
        })
        .await?
        .ok_or(WebError::NotFound)
}

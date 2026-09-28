//! Serving git itself.
//!
//! Clients are git, not this program, so the instance speaks git's own
//! transport rather than inventing one: `git http-backend` does the
//! protocol and this module does what it cannot — deciding who is asking
//! and which repository they may have.
//!
//! What a credential opens is narrow on purpose. A group's shared
//! repository is open to its members; a personal repository is open to
//! exactly one person, and not to the group's owner, not to a maintainer
//! of the instance, and not to anybody who knows the address. That is what
//! makes a private line private: it is not a flag on a row, it is a
//! repository nobody else can clone.
//!
//! Nothing is pushed. Lines are written a revision at a time through the
//! instance, which checks each one, and a push would go around that; so
//! git here only ever reads, and a clone is a way to take the history
//! away, never a way to put anything back.

use std::process::Stdio;

use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::{Path as UrlPath, RawQuery, State};
use axum::http::header::{
    AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue,
    WWW_AUTHENTICATE,
};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use stemma_core::id::{Slug, UserId};
use tokio::io::AsyncWriteExt;

use super::{Instance, WebError};
use crate::git::Kind;
use crate::store::{devices, groups, users};

/// The most a single push may carry.
const MOST_A_PUSH_CARRIES: usize = 256 * 1024 * 1024;

/// Answers a request from git.
pub async fn serve(
    State(instance): State<Instance>,
    method: Method,
    UrlPath((group, rest)): UrlPath<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, WebError> {
    let query = query.unwrap_or_default();

    if !is_git_talking(&method) {
        return Err(WebError::NotFound);
    }

    let Ok(group_slug) = Slug::parse(&group) else {
        return Err(WebError::NotFound);
    };

    let Some((repository, path)) = split(&rest) else {
        return Err(WebError::NotFound);
    };

    if is_a_push(path, &query) {
        return Err(WebError::Forbidden(
            "These repositories are read-only: lines are written through Stemma.".to_owned(),
        ));
    }

    let Some((kind, on_disk)) = instance.repositories().resolve(&group_slug, repository) else {
        return Err(WebError::NotFound);
    };

    // Release snapshots are deliberately public. Working repositories still
    // do not confirm their existence until a credential proves membership.
    let asking = who_is_asking(&instance, &headers).await?;
    if kind != Kind::Releases {
        let Some(asking) = asking.as_ref() else {
            return Ok(ask_for_a_credential());
        };
        if !may(&instance, &group_slug, kind, repository, asking).await? {
            return Err(WebError::NotFound);
        }
    }

    if !crate::git::exists(&on_disk) {
        return Err(WebError::NotFound);
    }

    let body = axum::body::to_bytes(body, MOST_A_PUSH_CARRIES)
        .await
        .map_err(|_| WebError::BadRequest("that push is too large".to_owned()))?;

    let remote_user = asking
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "public".to_owned());
    run_backend(
        &instance,
        &method,
        &format!("/{group}/{repository}/{path}"),
        &query,
        &headers,
        &remote_user,
        body.to_vec(),
    )
    .await
    .map_err(WebError::Internal)
}

/// Whether git is trying to send something rather than to read.
fn is_a_push(path: &str, query: &str) -> bool {
    path == "git-receive-pack"
        || query
            .split('&')
            .any(|pair| pair == "service=git-receive-pack")
}

/// Whether this is a shape git uses at all.
fn is_git_talking(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::POST)
}

/// Splits what was asked for into a repository and a path inside it.
fn split(rest: &str) -> Option<(&str, &str)> {
    match rest.split_once(".git/") {
        Some((repository, path))
            if !path.is_empty()
                && !path.contains('\\')
                && !path.chars().any(char::is_control)
                && path.split('/').all(|part| !matches!(part, "" | "." | "..")) =>
        {
            Some((&rest[..repository.len() + 4], path))
        }
        _ => None,
    }
}

/// Whose credential the request carried, if it carried one that works.
async fn who_is_asking(
    instance: &Instance,
    headers: &HeaderMap,
) -> Result<Option<UserId>, WebError> {
    let Some(token) = credential(headers) else {
        return Ok(None);
    };

    let key = instance.key.clone();

    let holder = instance
        .read(move |connection| {
            let Some(id) = devices::holder(connection, &key, &token)? else {
                return Ok(None);
            };

            // A suspended account keeps its repositories and loses its
            // way in.
            Ok(users::by_id(connection, &id)?
                .filter(users::User::is_active)
                .map(|user| user.id))
        })
        .await?;

    Ok(holder)
}

/// The credential a request carried, in either of the two ways git sends
/// one.
fn credential(headers: &HeaderMap) -> Option<String> {
    let offered = headers.get(AUTHORIZATION)?.to_str().ok()?;

    if let Some(token) = offered.strip_prefix("Bearer ") {
        return Some(token.trim().to_owned()).filter(|token| !token.is_empty());
    }

    // Git asks a credential helper for a username and a password. The
    // password is the credential; the username is whatever the helper
    // said, and is not what decides anything.
    let encoded = offered.strip_prefix("Basic ")?.trim();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let pair = String::from_utf8(decoded).ok()?;

    pair.split_once(':')
        .map(|(_, password)| password.to_owned())
        .filter(|password| !password.is_empty())
}

/// Whether this account may have this repository.
async fn may(
    instance: &Instance,
    group: &Slug,
    kind: Kind,
    repository: &str,
    asking: &UserId,
) -> Result<bool, WebError> {
    let group = stemma_core::id::GroupId::new(group.clone());
    let asking = asking.clone();

    match kind {
        Kind::Shared => {
            let role = instance
                .read(move |connection| groups::role_of(connection, &group, &asking))
                .await?;

            Ok(role.is_some())
        }
        Kind::Personal => {
            // The repository's own name says whose it is, and only they
            // may have it. Being in the group is not enough, and neither
            // is running the instance.
            let whose = repository
                .strip_prefix("personal/")
                .and_then(|name| name.strip_suffix(".git"))
                .and_then(|name| Slug::parse(name).ok())
                .map(UserId::new);

            Ok(whose.as_ref() == Some(&asking))
        }
        Kind::Releases => Ok(true),
    }
}

/// Asks git for a credential, in the way git knows how to answer.
fn ask_for_a_credential() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"stemma\""),
        )],
        "This repository needs a credential from `stemma login`.\n",
    )
        .into_response()
}

/// Hands the request to git and streams back what it says.
async fn run_backend(
    instance: &Instance,
    method: &Method,
    path_info: &str,
    query: &str,
    headers: &HeaderMap,
    asking: &str,
    body: Vec<u8>,
) -> Result<Response> {
    let root = instance.repositories().root().to_owned();

    let mut command = tokio::process::Command::new("git");
    command
        .arg("http-backend")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GIT_PROJECT_ROOT", &root)
        // This instance decided who may have what before git was asked,
        // so git is told not to ask the question again by looking for a
        // file in the repository.
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("REQUEST_METHOD", method.as_str())
        .env("PATH_INFO", path_info)
        .env("QUERY_STRING", query)
        .env("REMOTE_USER", asking)
        .env("CONTENT_LENGTH", body.len().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    forward(&mut command, headers, &CONTENT_TYPE, "CONTENT_TYPE");
    // Git compresses a request once it passes a kilobyte, which a fetch
    // from a clone with any history of its own soon does, and the backend
    // only inflates what it is told was deflated.
    forward(
        &mut command,
        headers,
        &CONTENT_ENCODING,
        "HTTP_CONTENT_ENCODING",
    );
    forward(
        &mut command,
        headers,
        &HeaderName::from_static("git-protocol"),
        "GIT_PROTOCOL",
    );

    let mut child = command.spawn().context("cannot run git http-backend")?;

    let mut stdin = child.stdin.take().context("git took no input")?;
    tokio::spawn(async move {
        let _ = stdin.write_all(&body).await;
        let _ = stdin.shutdown().await;
    });

    let stdout = child.stdout.take().context("git said nothing")?;
    let stderr = child.stderr.take();

    // The process outlives this function: the answer is still being read
    // out of its output while the request handler has already returned,
    // and dropping the handle here would kill it mid-sentence.
    tokio::spawn(async move {
        if let Some(mut stderr) = stderr {
            let mut said = Vec::new();
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut said).await;

            let said = String::from_utf8_lossy(&said);
            if !said.trim().is_empty() {
                tracing::warn!("git http-backend: {}", said.trim());
            }
        }

        let _ = child.wait().await;
    });
    let (head, rest) = read_cgi_head(stdout).await?;

    // The answer can be a whole repository, so it is streamed rather than
    // gathered up first.
    let stream = tokio_util::io::ReaderStream::new(rest);
    let mut response = Response::new(Body::from_stream(stream));

    *response.status_mut() = head.status;
    for (name, value) in head.headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            response.headers_mut().append(name, value);
        }
    }

    Ok(response)
}

/// Passes one header through to git, under the name git looks for.
fn forward(
    command: &mut tokio::process::Command,
    headers: &HeaderMap,
    header: &HeaderName,
    variable: &str,
) {
    if let Some(value) = headers.get(header).and_then(|value| value.to_str().ok()) {
        command.env(variable, value);
    }
}

/// What git said before the body.
struct Head {
    status: StatusCode,
    headers: Vec<(String, String)>,
}

/// Reads the headers git wrote, leaving the body where it is.
async fn read_cgi_head<R>(stdout: R) -> Result<(Head, tokio::io::BufReader<R>)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    const MAX_HEAD_BYTES: usize = 64 * 1024;
    let mut stdout = tokio::io::BufReader::new(stdout);

    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];

    // Buffer the pipe reads while retaining any body bytes read ahead.
    // A broken backend must not produce an empty success or unbounded head.
    while !buffer.ends_with(b"\r\n\r\n") && !buffer.ends_with(b"\n\n") {
        anyhow::ensure!(
            buffer.len() < MAX_HEAD_BYTES,
            "git returned oversized CGI headers"
        );
        match stdout.read(&mut byte).await? {
            0 => anyhow::bail!("git output ended before its CGI headers were complete"),
            _ => buffer.push(byte[0]),
        }
    }

    let text = String::from_utf8(buffer).context("git returned non-UTF-8 CGI headers")?;
    let mut status = StatusCode::OK;
    let mut headers = Vec::new();

    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (name, value) = line
            .split_once(':')
            .context("git returned a malformed CGI header")?;
        let value = value.trim().to_owned();

        if name.eq_ignore_ascii_case("status") {
            status = value
                .split_whitespace()
                .next()
                .and_then(|code| code.parse().ok())
                .and_then(|code: u16| StatusCode::from_u16(code).ok())
                .context("git returned an invalid CGI status")?;
        } else {
            headers.push((name.to_owned(), value));
        }
    }

    Ok((Head { status, headers }, stdout))
}

/// Where a repository is, as a client is told to reach it.
pub fn url(public_url: &crate::config::PublicUrl, group: &Slug, repository: &str) -> String {
    public_url.link(&format!("git/{group}/{repository}"))
}

/// The path of a group's shared repository.
pub fn shared_path(group: &Slug) -> String {
    format!("git/{group}/shared.git")
}

/// The path of somebody's own repository inside a group.
pub fn personal_path(group: &Slug, user: &UserId) -> String {
    format!("git/{group}/personal/{}.git", user.slug())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cgi_headers_preserve_status_and_body() {
        use tokio::io::AsyncReadExt;

        for separator in ["\r\n", "\n"] {
            let output = format!(
                "Status: 403 Forbidden{separator}Content-Type: text/plain{separator}{separator}body\n"
            );
            let (head, mut rest) = read_cgi_head(output.as_bytes()).await.unwrap();
            assert_eq!(head.status, StatusCode::FORBIDDEN);
            assert_eq!(
                head.headers,
                vec![("Content-Type".into(), "text/plain".into())]
            );
            let mut body = String::new();
            rest.read_to_string(&mut body).await.unwrap();
            assert_eq!(body, "body\n");
        }
    }

    #[tokio::test]
    async fn broken_cgi_output_is_not_a_successful_response() {
        for output in [
            "",
            "Content-Type: text/plain\r\n",
            "not a header\n\n",
            "Status: nonsense\n\n",
        ] {
            assert!(
                read_cgi_head(output.as_bytes()).await.is_err(),
                "accepted {output:?}"
            );
        }
        let oversized = vec![b'a'; 64 * 1024 + 1];
        assert!(read_cgi_head(oversized.as_slice()).await.is_err());
    }

    fn basic(user: &str, password: &str) -> HeaderMap {
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Basic {encoded}")).unwrap(),
        );

        headers
    }

    #[test]
    fn nothing_is_ever_pushed() {
        assert!(is_a_push("git-receive-pack", ""));
        assert!(is_a_push("info/refs", "service=git-receive-pack"));
        assert!(!is_a_push("info/refs", "service=git-upload-pack"));
        assert!(!is_a_push("git-upload-pack", ""));
    }

    #[test]
    fn a_request_is_split_into_a_repository_and_a_path_inside_it() {
        assert_eq!(
            split("shared.git/info/refs"),
            Some(("shared.git", "info/refs"))
        );
        assert_eq!(
            split("personal/ana.git/git-upload-pack"),
            Some(("personal/ana.git", "git-upload-pack"))
        );
    }

    #[test]
    fn what_is_not_a_repository_path_is_refused() {
        for asked in [
            "shared.git",
            "",
            "info/refs",
            "shared.git/",
            "shared/info",
            "shared.git/../personal/ana.git/info/refs",
            "shared.git/info/./refs",
            "shared.git//info/refs",
            "shared.git/..\\personal\\ana.git/info/refs",
        ] {
            assert_eq!(split(asked), None, "took `{asked}`");
        }
    }

    #[test]
    fn only_the_two_methods_git_uses_are_answered() {
        assert!(is_git_talking(&Method::GET));
        assert!(is_git_talking(&Method::POST));
        assert!(!is_git_talking(&Method::DELETE));
        assert!(!is_git_talking(&Method::PUT));
    }

    #[test]
    fn a_credential_is_read_from_either_way_git_sends_one() {
        let mut bearer = HeaderMap::new();
        bearer.insert(AUTHORIZATION, HeaderValue::from_static("Bearer a-secret"));

        assert_eq!(credential(&bearer).as_deref(), Some("a-secret"));
        assert_eq!(
            credential(&basic("stemma", "a-secret")).as_deref(),
            Some("a-secret")
        );
        assert_eq!(
            credential(&basic("", "a-secret")).as_deref(),
            Some("a-secret")
        );
    }

    #[test]
    fn a_request_with_nothing_to_show_carries_no_credential() {
        let mut nonsense = HeaderMap::new();
        nonsense.insert(AUTHORIZATION, HeaderValue::from_static("Basic not-base64"));

        assert_eq!(credential(&HeaderMap::new()), None);
        assert_eq!(credential(&nonsense), None);
        assert_eq!(credential(&basic("stemma", "")), None);
    }

    #[test]
    fn a_client_is_told_where_its_repositories_are() {
        let public = crate::config::PublicUrl::parse("https://stemma.test").unwrap();
        let algebra = Slug::parse("algebra").unwrap();
        let ana: UserId = "usr:ana".parse().unwrap();

        assert_eq!(
            url(&public, &algebra, "shared.git"),
            "https://stemma.test/git/algebra/shared.git"
        );
        assert_eq!(shared_path(&algebra), "git/algebra/shared.git");
        assert_eq!(
            personal_path(&algebra, &ana),
            "git/algebra/personal/ana.git"
        );
    }
}

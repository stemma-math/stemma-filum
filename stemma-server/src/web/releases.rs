//! Publishing a reviewed shared library and serving its public documentation.

use std::collections::HashMap;
use std::path::{Component, Path as FilePath};

use anyhow::{Context, Result, bail};
use axum::extract::{Path, State};
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use stemma_core::access::GroupRole;
use stemma_core::document::Document;
use stemma_core::id::{BlockId, Slug};
use stemma_core::line::Name;
use stemma_core::model::Verification;
use stemma_core::repository::{self, Side};

use super::layout::{Chrome, page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::git::Author;
use crate::store::groups::{self, Group};
use crate::store::releases::{self, Release, ResultPage};

/// What the publication form sends.
#[derive(Debug, Deserialize)]
pub struct Publish {
    /// The CSRF token [`signature`] put in the form.
    pub form_token: String,
}

/// The member-only release management page.
pub async fn manage(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
) -> Result<Response, WebError> {
    let (group, role) = super::membership::membership(&instance, &slug, &signed.user.id).await?;
    let id = group.id.clone();
    let releases = instance
        .read(move |connection| releases::for_group(connection, &id))
        .await?;
    let chrome = super::layout::around(&instance, &signed, "Library releases")
        .await?
        .at(format!("/g/{}/releases", group.slug()));

    Ok(page(&chrome, manage_body(&signed, &group, role, &releases)).into_response())
}

/// Publishes the shared repository's current commit.
pub async fn publish(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::Form(form): axum::Form<Publish>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;
    let (group, role) = super::membership::membership(&instance, &slug, &signed.user.id).await?;
    if !role.is_at_least(GroupRole::Admin) {
        return Err(WebError::Forbidden(
            "Only an admin or the owner of this group can publish a release.".to_owned(),
        ));
    }
    let group_id = group.id.clone();
    let (source_commit, documents) = instance
        .with_repositories(move |repositories| {
            let shared = repositories.shared(&group_id);
            let commit = crate::git::head_commit(&shared)
                .context("the shared library has no source to release")?;
            let mut documents = Vec::new();
            for line in crate::git::lines_at(&shared, &commit)? {
                let name = Name::parse(&line).context("the shared library has an invalid line")?;
                let source = crate::git::show(
                    &shared,
                    &commit,
                    &format!("{}/{}/{}", repository::LINES, name, repository::ENTRY),
                )?;
                documents.push((name, Document::parse(&source)?));
            }
            Ok((commit, documents))
        })
        .await?;

    let group_id = group.id.clone();
    let reader = signed.user.id.clone();
    let results = instance
        .read(move |connection| documented(connection, &group_id, &reader, documents))
        .await?
        .map_err(WebError::BadRequest)?;

    let group_id = group.id.clone();
    let (published_on, existing): (String, Vec<Release>) = instance
        .read(move |connection| {
            let date =
                connection.query_row("SELECT strftime('%Y-%m-%d', 'now')", [], |row| row.get(0))?;
            Ok((date, releases::for_group(connection, &group_id)?))
        })
        .await?;
    if let Some(release) = existing.iter().find(|release| {
        release.source_commit == source_commit
            && release.version.starts_with(&format!("{published_on}-"))
    }) {
        return Err(WebError::BadRequest(format!(
            "Commit {source_commit} was already released today as {}.",
            release.version
        )));
    }
    let version = generated_version(
        &published_on,
        &source_commit,
        &existing
            .iter()
            .map(|release| release.version.as_str())
            .collect::<Vec<_>>(),
    )?;

    let group_id = group.id.clone();
    let source = source_commit.clone();
    let release_version = version.clone();
    let author_name = signed.user.display_name.clone();
    let author_email = signed.user.email.clone();
    let release_commit = instance
        .writing_git(move |repositories| {
            crate::git::publish_release(
                &repositories.shared(&group_id),
                &repositories.releases(&group_id),
                &source,
                &release_version,
                &Author {
                    name: &author_name,
                    email: &author_email,
                },
            )
        })
        .await?;

    let group_id = group.id.clone();
    let by = signed.user.id.clone();
    let saved = instance
        .write({
            let version = version.clone();
            let source_commit = source_commit.clone();
            let release_commit = release_commit.clone();
            let results = results.clone();
            move |connection| {
                releases::record(
                    connection,
                    &group_id,
                    &version,
                    &source_commit,
                    &release_commit,
                    &results,
                    &by,
                )
            }
        })
        .await;
    if let Err(error) = saved {
        let group_id = group.id.clone();
        let cleanup_version = version.clone();
        let _ = instance
            .writing_git(move |repositories| {
                crate::git::remove_release(&repositories.releases(&group_id), &cleanup_version)
            })
            .await;
        return Err(error);
    }

    Ok(Redirect::to(&format!("/releases/{}/{version}", group.slug())).into_response())
}

/// All public releases from one group.
pub async fn public_group(
    State(instance): State<Instance>,
    Path(slug): Path<String>,
) -> Result<Response, WebError> {
    let Some(group) = public_group_by_slug(&instance, &slug).await? else {
        return Err(WebError::NotFound);
    };
    let id = group.id.clone();
    let releases = instance
        .read(move |connection| releases::for_group(connection, &id))
        .await?;
    if releases.is_empty() {
        return Err(WebError::NotFound);
    }
    let chrome = public_chrome(&instance, &format!("{} releases", group.name)).await?;

    Ok(page(&chrome, public_group_body(&group, &releases)).into_response())
}

/// Generated documentation for one immutable version.
pub async fn documentation(
    State(instance): State<Instance>,
    Path((slug, version)): Path<(String, String)>,
) -> Result<Response, WebError> {
    let Some((group, release)) = public_release(&instance, &slug, &version).await? else {
        return Err(WebError::NotFound);
    };
    let chrome = public_chrome(
        &instance,
        &format!("{} {} documentation", group.name, release.version),
    )
    .await?;
    let git = instance
        .settings
        .public_url
        .link(&format!("git/{}/releases.git", group.slug()));

    Ok(page(&chrome, documentation_body(&group, &release, &git)).into_response())
}

/// A permanent citation page for one central result.
pub async fn result(
    State(instance): State<Instance>,
    Path((slug, version, line, block)): Path<(String, String, String, String)>,
) -> Result<Response, WebError> {
    let Some((group, release)) = public_release(&instance, &slug, &version).await? else {
        return Err(WebError::NotFound);
    };
    let Ok(line) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };
    let Ok(block) = block.parse::<BlockId>() else {
        return Err(WebError::NotFound);
    };
    let Some(result) = release
        .results
        .iter()
        .find(|result| result.line == line && result.block == block)
    else {
        return Err(WebError::NotFound);
    };
    let chrome = public_chrome(&instance, result.name()).await?;

    Ok(page(&chrome, result_body(&group, &release, result)).into_response())
}

/// One text file from a released source tree.
pub async fn source(
    State(instance): State<Instance>,
    Path((slug, version, path)): Path<(String, String, String)>,
) -> Result<Response, WebError> {
    let Some((group, release)) = public_release(&instance, &slug, &version).await? else {
        return Err(WebError::NotFound);
    };
    if !safe_source_path(&path) {
        return Err(WebError::NotFound);
    }
    let group_id = group.id.clone();
    let release_commit = release.release_commit.clone();
    let content = instance
        .with_repositories(move |repositories| {
            crate::git::show(&repositories.releases(&group_id), &release_commit, &path)
        })
        .await
        .map_err(|_| WebError::NotFound)?;

    Ok((
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )],
        content,
    )
        .into_response())
}

fn documented(
    connection: &rusqlite::Connection,
    group: &stemma_core::id::GroupId,
    reader: &stemma_core::id::UserId,
    documents: Vec<(Name, Document)>,
) -> Result<Result<Vec<ResultPage>, String>> {
    let mut pages = Vec::new();
    let mut missing = Vec::new();

    for (name, document) in documents {
        let Some(line) =
            crate::store::lines::by_name(connection, group, Side::Shared, &name, reader)?
        else {
            bail!("the shared source contains an unindexed line `{name}`");
        };
        let audits: HashMap<_, _> = crate::store::formal_audits::for_line(connection, &line.id)?
            .into_iter()
            .map(|audit| (audit.block, audit))
            .collect();

        for block in &document.blocks {
            let Some(statement) = block.statement().filter(|statement| statement.central) else {
                continue;
            };
            let block_source = serde_json::to_string(block)?;
            let Some(audit) = audits.get(block.id()).filter(|audit| {
                audit.state == "audited"
                    && audit.block_source == block_source
                    && audit.check.state == Verification::Verified
                    && !audit.check.declarations.is_empty()
                    && audit
                        .check
                        .declarations
                        .iter()
                        .all(|declaration| declaration.sorry_free)
            }) else {
                missing.push(format!("{} / {}", document.title, block.id()));
                continue;
            };
            pages.push(ResultPage {
                line: name.clone(),
                line_title: document.title.clone(),
                block: *block.id(),
                environment: statement.environment.clone(),
                title: statement.title.clone(),
                prose: statement.content.clone(),
                declarations: audit.check.declarations.clone(),
            });
        }
    }

    if !missing.is_empty() {
        return Ok(Err(format!(
            "Every central result must have a current, sorry-free formal audit before release. Missing: {}.",
            missing.join(", ")
        )));
    }
    if pages.is_empty() {
        return Ok(Err(
            "There are no audited central results to publish in this library.".to_owned(),
        ));
    }
    Ok(Ok(pages))
}

fn generated_version(date: &str, source_commit: &str, used: &[&str]) -> Result<String> {
    if source_commit.len() < 12 || !source_commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("the shared library has an invalid source commit");
    }
    for length in 12..=source_commit.len() {
        let version = format!("{date}-{}", &source_commit[..length]);
        if !used.contains(&version.as_str()) {
            return Ok(version);
        }
    }
    bail!("the shared library's source commit has no unused release name")
}

fn safe_source_path(path: &str) -> bool {
    !path.is_empty()
        && FilePath::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

async fn public_group_by_slug(instance: &Instance, slug: &str) -> Result<Option<Group>, WebError> {
    let Ok(slug) = Slug::parse(slug) else {
        return Ok(None);
    };
    instance
        .read(move |connection| groups::by_slug(connection, &slug))
        .await
}

async fn public_release(
    instance: &Instance,
    slug: &str,
    version: &str,
) -> Result<Option<(Group, Release)>, WebError> {
    let Some(group) = public_group_by_slug(instance, slug).await? else {
        return Ok(None);
    };
    let id = group.id.clone();
    let version = version.to_owned();
    let release = instance
        .read(move |connection| releases::by_version(connection, &id, &version))
        .await?;
    Ok(release.map(|release| (group, release)))
}

async fn public_chrome(instance: &Instance, title: &str) -> Result<Chrome, WebError> {
    let policy = instance.policy().await?;
    Ok(Chrome::anonymous(
        title,
        super::front::instance_name(instance, &policy),
    ))
}

fn manage_body(signed: &SignedIn, group: &Group, role: GroupRole, releases: &[Release]) -> Markup {
    html! {
        nav class="crumbs" aria-label="Breadcrumb" {
            a href={ "/g/" (group.slug()) } { (group.name) }
            span aria-hidden="true" { "/" }
            span aria-current="page" { "Library releases" }
        }
        header class="page-head" {
            h1 class="page-title" { "Library releases" }
            p class="page-subtitle" {
                "A release is the only part of this library visible outside the group. "
                "Its source, documentation and citation pages never change."
            }
        }
        @if role.is_at_least(GroupRole::Admin) {
            section class="block" {
                h2 class="block__title" { "Publish the current shared library" }
                p {
                    "Every central result must have a current formal audit and a sorry-free Lean declaration."
                }
                form method="post" action={ "/g/" (group.slug()) "/releases" } class="stack" {
                    (signature(signed))
                    p class="field__hint" {
                        "The version is assigned when published: UTC date and the shared source commit. "
                        "Releases are permanent and cannot be replaced."
                    }
                    button class="button button--primary" type="submit" { "Publish release" }
                }
            }
        }
        (release_list(group, releases))
    }
}

fn release_list(group: &Group, releases: &[Release]) -> Markup {
    html! {
        section class="block" {
            h2 class="block__title" { "Published versions" }
            @if releases.is_empty() {
                p class="empty" { "No release has been published yet." }
            } @else {
                ul class="stack" {
                    @for release in releases {
                        li {
                            a href={ "/releases/" (group.slug()) "/" (release.version) } {
                                "Version " (release.version)
                            }
                            span class="quiet" {
                                " · " (release.results.len()) " central results · " (release.published_at)
                            }
                        }
                    }
                }
            }
        }
    }
}

fn public_group_body(group: &Group, releases: &[Release]) -> Markup {
    html! {
        header class="page-head" {
            h1 class="page-title" { (group.name) " library" }
            @if let Some(description) = &group.description {
                p class="page-subtitle" { (description) }
            }
        }
        (release_list(group, releases))
    }
}

fn documentation_body(group: &Group, release: &Release, git: &str) -> Markup {
    let package = format!("stemma_{}_library", group.slug().as_str().replace('-', "_"));
    html! {
        nav class="crumbs" aria-label="Breadcrumb" {
            a href={ "/releases/" (group.slug()) } { (group.name) }
            span aria-hidden="true" { "/" }
            span aria-current="page" { "Version " (release.version) }
        }
        header class="page-head" {
            h1 class="page-title" { (group.name) " " (release.version) }
            p class="page-subtitle" {
                (release.results.len()) " audited central results, published " (release.published_at) "."
            }
        }
        section class="block" {
            h2 class="block__title" { "Use this release" }
            pre { code {
                "[[require]]\nname = \"" (package) "\"\ngit = \"" (git)
                "\"\nrev = \"v" (release.version) "\""
            } }
            p { "Git repository: " code { (git) } }
            p {
                a href={ "/releases/" (group.slug()) "/" (release.version) "/source/stemma.toml" } {
                    "View the released source manifest"
                }
            }
        }
        section class="block" {
            h2 class="block__title" { "Documentation" }
            ul class="stack" {
                @for result in &release.results {
                    li {
                        a href={ "/releases/" (group.slug()) "/" (release.version) "/" (result.line) "/" (result.block) } {
                            (result.name())
                        }
                        span class="quiet" { " · " (result.line_title) }
                    }
                }
            }
        }
    }
}

fn result_body(group: &Group, release: &Release, result: &ResultPage) -> Markup {
    html! {
        nav class="crumbs" aria-label="Breadcrumb" {
            a href={ "/releases/" (group.slug()) "/" (release.version) } {
                (group.name) " " (release.version)
            }
            span aria-hidden="true" { "/" }
            span aria-current="page" { (result.name()) }
        }
        header class="page-head" {
            p class="label" { (result.environment) }
            h1 class="page-title" { (result.name()) }
            p class="page-subtitle" { (result.line_title) }
        }
        section class="block" {
            h2 class="block__title" { "Prose statement" }
            p { (result.prose) }
        }
        section class="block" {
            h2 class="block__title" { "Lean statement" }
            @for declaration in &result.declarations {
                div class="stack" {
                    p class="identifier" { (declaration.name) }
                    pre { code { (declaration.statement) } }
                    p {
                        a href={ "/releases/" (group.slug()) "/" (release.version) "/source/"
                            (declaration.module.replace('.', "/")) ".lean" } {
                            "View " (declaration.module) " in the released source"
                        }
                    }
                }
            }
        }
        footer class="quiet" {
            "Permanent result id " (result.block) " · release commit " (release.release_commit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_versions_name_the_date_and_source_commit() {
        assert_eq!(
            generated_version(
                "2026-09-30",
                "0123456789abcdef0123456789abcdef01234567",
                &[]
            )
            .unwrap(),
            "2026-09-30-0123456789ab"
        );
        assert_eq!(
            generated_version(
                "2026-09-30",
                "0123456789abcdef0123456789abcdef01234567",
                &["2026-09-30-0123456789ab"],
            )
            .unwrap(),
            "2026-09-30-0123456789abc"
        );
        assert!(generated_version("2026-09-30", "not-a-commit", &[]).is_err());
    }

    #[test]
    fn source_paths_cannot_leave_the_release_tree() {
        assert!(safe_source_path("Algebra/Quotients.lean"));
        assert!(!safe_source_path("../shared.git/config"));
        assert!(!safe_source_path("Algebra/../stemma.toml"));
        assert!(!safe_source_path(""));
    }
}

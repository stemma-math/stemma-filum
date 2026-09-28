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
    /// Version chosen by the group, without the Git tag's `v` prefix.
    pub version: String,
    /// The CSRF token [`signature`] put in the form.
    pub form_token: String,
}

/// The member-only release management page.
pub async fn manage(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
) -> Result<Response, WebError> {
    let (group, role) = member_group(&instance, &signed, &slug).await?;
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
    let (group, role) = member_group(&instance, &signed, &slug).await?;
    if !role.is_at_least(GroupRole::Admin) {
        return Err(WebError::Forbidden(
            "Only an admin or the owner of this group can publish a release.".to_owned(),
        ));
    }
    let version = normalized_version(&form.version)?;
    let id = group.id.clone();
    let exists = instance
        .read({
            let version = version.clone();
            move |connection| Ok(releases::by_version(connection, &id, &version)?.is_some())
        })
        .await?;
    if exists {
        return Err(WebError::BadRequest(format!(
            "Version {version} has already been published and cannot be replaced."
        )));
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

fn normalized_version(given: &str) -> Result<String, WebError> {
    let version = given.trim();
    let valid = !version.is_empty()
        && version.len() <= 64
        && version.as_bytes()[0].is_ascii_digit()
        && version.as_bytes()[version.len() - 1].is_ascii_alphanumeric()
        && !version.contains("..")
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'));
    if !valid {
        return Err(WebError::BadRequest(
            "Use a version such as 1.0.0 or 2.1.0-rc.1, without a leading v.".to_owned(),
        ));
    }
    Ok(version.to_owned())
}

fn safe_source_path(path: &str) -> bool {
    !path.is_empty()
        && FilePath::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

async fn member_group(
    instance: &Instance,
    signed: &SignedIn,
    slug: &str,
) -> Result<(Group, GroupRole), WebError> {
    let slug = Slug::parse(slug).map_err(|_| WebError::NotFound)?;
    let user = signed.user.id.clone();
    let found = instance
        .read(move |connection| {
            let Some(group) = groups::by_slug(connection, &slug)? else {
                return Ok(None);
            };
            let role = groups::role_of(connection, &group.id, &user)?;
            Ok(role.map(|role| (group, role)))
        })
        .await?;
    found.ok_or(WebError::NotFound)
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
        div class="stack" {
            header class="page-head" {
                h1 class="page-title" { "Library releases" }
                p class="page-subtitle" {
                    "A release is the only part of this library visible outside the group. "
                    "Its source, documentation and citation pages never change."
                }
            }
            @if role.is_at_least(GroupRole::Admin) {
                section class="card" {
                    h2 { "Publish the current shared library" }
                    p {
                        "Every central result must have a current formal audit and a sorry-free Lean declaration."
                    }
                    form method="post" action={ "/g/" (group.slug()) "/releases" } class="stack" {
                        (signature(signed))
                        div class="field" {
                            label class="field__label" for="version" { "Version" }
                            input class="input" id="version" name="version" placeholder="1.0.0" required;
                            span class="field__hint" { "Versions are permanent and cannot be replaced." }
                        }
                        button class="button button--primary" type="submit" { "Publish release" }
                    }
                }
            }
            (release_list(group, releases))
        }
    }
}

fn release_list(group: &Group, releases: &[Release]) -> Markup {
    html! {
        section class="card" {
            h2 { "Published versions" }
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
        div class="stack" {
            header class="page-head" {
                h1 class="page-title" { (group.name) " library" }
                @if let Some(description) = &group.description {
                    p class="page-subtitle" { (description) }
                }
            }
            (release_list(group, releases))
        }
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
        div class="stack" {
            header class="page-head" {
                h1 class="page-title" { (group.name) " " (release.version) }
                p class="page-subtitle" {
                    (release.results.len()) " audited central results, published " (release.published_at) "."
                }
            }
            section class="card" {
                h2 { "Use this release" }
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
            section class="card" {
                h2 { "Documentation" }
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
        article class="stack" {
            header class="page-head" {
                p class="label" { (result.environment) }
                h1 class="page-title" { (result.name()) }
                p class="page-subtitle" { (result.line_title) }
            }
            section class="card" {
                h2 { "Prose statement" }
                p { (result.prose) }
            }
            section class="card" {
                h2 { "Lean statement" }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_safe_git_tags_and_paths() {
        for good in ["1.0.0", "2.1.0-rc.1", "2026.09+final"] {
            assert_eq!(normalized_version(good).unwrap(), good);
        }
        for bad in ["", "v1.0.0", "../main", "1/2", ".1", "1."] {
            assert!(normalized_version(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn source_paths_cannot_leave_the_release_tree() {
        assert!(safe_source_path("Algebra/Quotients.lean"));
        assert!(!safe_source_path("../shared.git/config"));
        assert!(!safe_source_path("Algebra/../stemma.toml"));
        assert!(!safe_source_path(""));
    }
}

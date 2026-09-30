//! Starting a line, and moving it to the group.
//!
//! A line is born in the web because it is the one act that needs no
//! mathematics: three fields, and a directory in a repository. What the
//! form collects is written into the source and committed with the
//! person as its author, so a clone of the repository is the whole
//! line and nothing about it is kept where they cannot see it.
//!
//! Sharing is a **move**: the group receives the line, the author keeps
//! the history that got them there, and nothing is deleted. One rule is
//! checked mechanically before it happens — nothing the group can see
//! may lean on something only its author can.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use stemma_core::access::WhoWrites;
use stemma_core::document::Document;
use stemma_core::id::GroupId;
use stemma_core::line::{Line, Name, NodeKind};
use stemma_core::render;
use stemma_core::repository::Side;

use super::layout::{page, signature};
use super::membership::group_of;
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups::Group;
use crate::store::lines;
use crate::store::revisions::Content;

/// What the form that starts a line sends.
#[derive(Debug, Default, Deserialize)]
pub struct Starting {
    /// What the line is called.
    #[serde(default)]
    pub title: String,
    /// What it is about. Empty when its author says nothing.
    #[serde(default)]
    pub description: String,
    /// `me` or `group`.
    #[serde(default)]
    pub seen_by: String,
    /// The form's own token.
    #[serde(default)]
    pub form_token: String,
}

/// What the form that shares a line sends.
#[derive(Debug, Default, Deserialize)]
pub struct Sharing {
    /// `today` or `everything`.
    #[serde(default)]
    pub travels: String,
    /// `author` or `anyone`.
    #[serde(default)]
    pub writing: String,
    /// The form's own token.
    #[serde(default)]
    pub form_token: String,
}

/// The three fields a line needs.
pub async fn form(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
) -> Result<Response, WebError> {
    let group = group_of(&instance, &slug, &signed.user.id).await?;
    let chrome = super::layout::around(&instance, &signed, "New line")
        .await?
        .at(format!("/g/{}", group.slug()));

    Ok(page(&chrome, asked(&signed, &group, None, None)).into_response())
}

/// Starts it.
pub async fn start(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::Form(form): axum::Form<Starting>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let group = group_of(&instance, &slug, &signed.user.id).await?;
    let side = match form.seen_by.trim() {
        "group" => Side::Shared,
        _ => Side::Private,
    };

    match born(
        &instance,
        &signed.user,
        &group,
        form.title.trim(),
        form.description.trim(),
        side,
    )
    .await
    {
        Ok(started) => Ok(
            Redirect::to(&format!("/g/{}/{}/{}", group.slug(), side, started.name)).into_response(),
        ),
        Err(WebError::BadRequest(problem)) => {
            let chrome = super::layout::around(&instance, &signed, "New line")
                .await?
                .at(format!("/g/{}", group.slug()));

            Ok(page(&chrome, asked(&signed, &group, Some(&problem), Some(&form))).into_response())
        }
        Err(other) => Err(other),
    }
}

/// Starts a line, wherever the asking came from.
///
/// Its first revision is what the form said, and its page is written
/// here from those words: nothing is compiled, because nothing needs to
/// be for a title and a sentence.
async fn born(
    instance: &Instance,
    who: &crate::store::users::User,
    group: &Group,
    title: &str,
    description: &str,
    side: Side,
) -> Result<lines::Started, WebError> {
    let refuse = |what: &str| WebError::BadRequest(what.to_owned());

    if title.is_empty() {
        return Err(refuse("A line needs a title."));
    }

    let Some(name) = Name::from_title(title) else {
        return Err(refuse(
            "That title has nothing a directory can be named after.",
        ));
    };

    let taken = {
        let (id, name, author) = (group.id.clone(), name.clone(), who.id.clone());

        instance
            .read(move |connection| {
                Ok(lines::by_name(connection, &id, side, &name, &author)?.is_some())
            })
            .await?
    };

    if taken {
        return Err(refuse("There is already a line by that name here."));
    }

    let description = (!description.is_empty()).then(|| description.to_owned());
    let content = Content {
        source: Document::new(title, description.clone()).to_text(),
        lean_source: None,
        page: first_page(title, description.as_deref()),
    };

    {
        let (group, name, source) = (group.id.clone(), name.clone(), content.source.clone());
        let (author, author_name, author_email) =
            (who.id.clone(), who.display_name.clone(), who.email.clone());

        instance
            .writing_git(move |repositories| {
                let path = match side {
                    Side::Shared => repositories.shared(&group),
                    Side::Private => repositories.personal(&group, &author),
                };

                if crate::git::holds_line(&path, &name) {
                    anyhow::bail!(WebError::BadRequest(
                        "There is already a line by that name here.".to_owned()
                    ));
                }

                crate::git::start_line(
                    &path,
                    &name,
                    &source,
                    &crate::git::Author {
                        name: &author_name,
                        email: &author_email,
                    },
                )?;

                Ok(())
            })
            .await?;
    }

    let (id, named, author) = (group.id.clone(), name.clone(), who.id.clone());

    instance
        .write(move |connection| {
            let started = lines::start(connection, &id, side, &named, &author, &content)?;
            lines::committed(connection, &started.id, started.revision)?;

            Ok(started)
        })
        .await
}

/// The page a line has before anybody has typeset it: its title and what
/// it is about, as the form said them.
pub fn first_page(title: &str, description: Option<&str>) -> String {
    html! {
        (maud::DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                title { (title) }
            }
            body {
                main class="stemma-line" {
                    h1 { (title) }
                    @if let Some(description) = description {
                        p class="stemma-line__description" { (description) }
                    }
                }
            }
        }
    }
    .into_string()
}

/// The two questions and the one warning.
pub async fn share_form(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((slug, line)): Path<(String, String)>,
) -> Result<Response, WebError> {
    let group = group_of(&instance, &slug, &signed.user.id).await?;
    let Ok(name) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };

    let source = mine(&instance, &signed.user, &group.id, &name).await?;
    let chrome = super::layout::around(&instance, &signed, format!("Share {name}"))
        .await?
        .at(format!("/g/{}", group.slug()));

    let held = held_back(&instance, &group.id, &source).await?;

    Ok(page(
        &chrome,
        offered(&signed, &group, &name, &source, &held, None),
    )
    .into_response())
}

/// Moves it.
pub async fn share(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((slug, line)): Path<(String, String)>,
    axum::Form(form): axum::Form<Sharing>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let group = group_of(&instance, &slug, &signed.user.id).await?;
    let Ok(name) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };

    match moved(
        &instance,
        &signed.user,
        &group,
        &name,
        form.travels.trim() == "everything",
        form.writing.trim().parse().unwrap_or(WhoWrites::Anyone),
    )
    .await
    {
        Ok(_) => Ok(Redirect::to(&format!("/g/{}/shared/{}", group.slug(), name)).into_response()),
        Err(WebError::BadRequest(problem)) => {
            let source = mine(&instance, &signed.user, &group.id, &name).await?;
            let held = held_back(&instance, &group.id, &source).await?;
            let chrome = super::layout::around(&instance, &signed, format!("Share {name}"))
                .await?
                .at(format!("/g/{}", group.slug()));

            Ok(page(
                &chrome,
                offered(&signed, &group, &name, &source, &held, Some(&problem)),
            )
            .into_response())
        }
        Err(other) => Err(other),
    }
}

/// Moves a line to the group, wherever the asking came from.
async fn moved(
    instance: &Instance,
    who: &crate::store::users::User,
    group: &Group,
    name: &Name,
    history: bool,
    writing: WhoWrites,
) -> Result<lines::Started, WebError> {
    let line = own_line(instance, who, &group.id, name).await?;

    // Git has to have everything the line says before it moves.
    crate::history::settle(instance, line.id).await?;

    let source = mine(instance, who, &group.id, name).await?;
    let held = held_back(instance, &group.id, &source).await?;

    if !held.is_empty() {
        return Err(WebError::BadRequest(
            "A shared line cannot depend on private work.".to_owned(),
        ));
    }

    let clash = {
        let (id, name, author) = (group.id.clone(), name.clone(), who.id.clone());

        instance
            .read(move |connection| {
                Ok(lines::by_name(connection, &id, Side::Shared, &name, &author)?.is_some())
            })
            .await?
    };

    if clash {
        return Err(WebError::BadRequest(
            "The group already has a line by that name. Rename yours before sharing it.".to_owned(),
        ));
    }

    let (group_id, author, moving) = (group.id.clone(), who.id.clone(), name.clone());
    let (author_name, author_email) = (who.display_name.clone(), who.email.clone());

    let library_commit = instance
        .writing_git(move |repositories| {
            let ours = repositories.shared(&group_id);
            let theirs = repositories.personal(&group_id, &author);

            // The group receives it first, so nothing can be lost
            // between the two commits.
            crate::git::bring_line(&ours, &theirs, &moving, history)?;
            let author = crate::git::Author {
                name: &author_name,
                email: &author_email,
            };
            let moved = crate::git::bring_line_library(&ours, &theirs, &moving, &author)?;
            crate::git::drop_line(
                &theirs,
                &moving,
                &author,
                &format!("Move {moving} to the group"),
            )?;

            Ok((!moved.is_empty())
                .then(|| crate::git::head_commit(&ours))
                .flatten())
        })
        .await?;

    instance
        .write(move |connection| {
            let shared = lines::share(connection, &line.id, writing)?;
            lines::committed(connection, &shared.id, shared.revision)?;
            if let Some(commit) = &library_commit {
                crate::store::events::publish(
                    connection,
                    crate::store::events::Audience::Group(&shared.group),
                    &stemma_core::event::Event::Library {
                        group: shared.group.slug().as_str().to_owned(),
                        side: Side::Shared,
                        commit: commit.clone(),
                        change: stemma_core::event::LibraryChange::Shared,
                    },
                )?;
            }

            Ok(shared)
        })
        .await
}

/// A line of this person's own, in their own repository.
async fn own_line(
    instance: &Instance,
    who: &crate::store::users::User,
    group: &GroupId,
    name: &Name,
) -> Result<lines::Started, WebError> {
    let (group, name, author) = (group.clone(), name.clone(), who.id.clone());

    instance
        .read(move |connection| lines::by_name(connection, &group, Side::Private, &name, &author))
        .await?
        .ok_or(WebError::NotFound)
}

/// Asks before a line of one's own is taken out of the repository.
pub async fn remove_form(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((slug, line)): Path<(String, String)>,
) -> Result<Response, WebError> {
    let group = group_of(&instance, &slug, &signed.user.id).await?;
    let Ok(name) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };
    own_line(&instance, &signed.user, &group.id, &name).await?;

    let chrome = super::layout::around(&instance, &signed, format!("Delete {name}"))
        .await?
        .at(format!("/g/{}", group.slug()));
    let back = format!("/g/{}/private/{}", group.slug(), name);

    Ok(page(
        &chrome,
        super::layout::dialog(
            "Delete this line?",
            &back,
            html! {
                p class="dialog__text" {
                    "It is removed from your repository. Its history is kept."
                }
                form method="post" action={ "/g/" (group.slug()) "/private/" (name) "/delete" } {
                    (signature(&signed))
                    (super::layout::dialog_actions(&back, "Delete line", true))
                }
            },
        ),
    )
    .into_response())
}

/// Takes a line of one's own out of the repository.
pub async fn remove(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((slug, line)): Path<(String, String)>,
    axum::Form(form): axum::Form<Removing>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;

    let group = group_of(&instance, &slug, &signed.user.id).await?;
    let Ok(name) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };

    gone(&instance, &signed.user, &group, &name).await?;

    Ok(Redirect::to(&format!("/g/{}", group.slug())).into_response())
}

/// The same, from a machine.
pub async fn api_remove(
    State(instance): State<Instance>,
    credential: super::api::Credential,
    Path((slug, line)): Path<(String, String)>,
) -> Result<Response, WebError> {
    let group = group_of(&instance, &slug, &credential.user.id).await?;
    let Ok(name) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };

    let deleted = gone(&instance, &credential.user, &group, &name).await?;

    let mut said = placed(&deleted);
    said["gone"] = serde_json::Value::Bool(true);

    Ok(axum::Json(said).into_response())
}

/// What the form that takes a line out sends.
#[derive(Debug, Default, Deserialize)]
pub struct Removing {
    /// The form's own token.
    #[serde(default)]
    pub form_token: String,
}

/// Takes the line out of its author's repository.
///
/// Only out of their own: a line the group has is the group's
/// mathematics, and nothing about it is one person's to remove. Even
/// here nothing is destroyed — the line's revisions stay, and so does
/// every commit that ever held it.
async fn gone(
    instance: &Instance,
    who: &crate::store::users::User,
    group: &Group,
    name: &Name,
) -> Result<lines::Started, WebError> {
    let line = own_line(instance, who, &group.id, name).await?;

    crate::history::settle(instance, line.id).await?;

    let (group_id, author, named) = (group.id.clone(), who.id.clone(), name.clone());
    let (author_name, author_email) = (who.display_name.clone(), who.email.clone());

    instance
        .writing_git(move |repositories| {
            let path = repositories.personal(&group_id, &author);

            if crate::git::holds_line(&path, &named) {
                crate::git::drop_line(
                    &path,
                    &named,
                    &crate::git::Author {
                        name: &author_name,
                        email: &author_email,
                    },
                    &format!("Delete {named}"),
                )?;
            }

            Ok(())
        })
        .await?;

    let by = who.id.clone();
    instance
        .write(move |connection| {
            let deleted = lines::delete(connection, &line.id, &by)?;
            lines::committed(connection, &deleted.id, deleted.revision)?;

            Ok(deleted)
        })
        .await
}

/// The line's source as it is now, in this person's own repository.
async fn mine(
    instance: &Instance,
    who: &crate::store::users::User,
    group: &GroupId,
    name: &Name,
) -> Result<String, WebError> {
    let line = own_line(instance, who, group, name).await?;

    instance
        .read(move |connection| crate::store::revisions::current(connection, &line))
        .await?
        .map(|revision| revision.source)
        .ok_or(WebError::NotFound)
}

/// What the line leans on that the group cannot see.
///
/// A path that stays inside the line travels with it. One that leaves
/// it names another line, and if that line is still its author's alone
/// the group would receive mathematics resting on nothing.
async fn held_back(
    instance: &Instance,
    group: &GroupId,
    source: &str,
) -> Result<Vec<String>, WebError> {
    let imports = match Document::parse(source) {
        Ok(document) => stemma_core::source::imports(&render::typst(&document).source),
        Err(_) => stemma_core::source::imports(source),
    };

    if imports.iter().all(|path| !path.starts_with("..")) {
        return Ok(Vec::new());
    }

    let shared = instance.repositories().shared(group);
    let known: Vec<String> = instance
        .blocking(move || {
            let Some(head) = crate::git::head_commit(&shared) else {
                return Ok(Vec::new());
            };

            crate::git::lines_at(&shared, &head)
        })
        .await?;

    Ok(imports
        .into_iter()
        .filter(|path| path.starts_with(".."))
        .filter(|path| {
            let named = path
                .trim_start_matches("../")
                .split('/')
                .next()
                .unwrap_or_default()
                .to_owned();

            !known.contains(&named)
        })
        .collect())
}

/// The form itself.
fn asked(
    signed: &SignedIn,
    group: &Group,
    problem: Option<&str>,
    given: Option<&Starting>,
) -> Markup {
    let title = given.map(|given| given.title.clone()).unwrap_or_default();
    let description = given
        .map(|given| given.description.clone())
        .unwrap_or_default();
    let shared = given.is_some_and(|given| given.seen_by.trim() == "group");
    let back = format!("/g/{}", group.slug());

    super::layout::dialog(
        "New line of work",
        &back,
        html! {
            p class="dialog__text" { "In " (group.name) "." }

            @if let Some(problem) = problem {
                (super::layout::message(super::layout::Kind::Error, problem))
            }

            form method="post" action={ "/g/" (group.slug()) "/lines" } {
                (signature(signed))

                div class="field" {
                    label class="field__label" for="title" { "Title" }
                    input class="input" type="text" id="title" name="title" value=(title) autofocus required;
                }

                div class="field" {
                    label class="field__label" for="description" { "Description" }
                    textarea class="input" id="description" name="description" rows="3" { (description) }
                    span class="field__hint" {
                        "Optional. What this line is about, in a sentence or two."
                    }
                }

                fieldset class="fieldset" {
                    legend { "Visibility" }

                    label class="choice" {
                        input type="radio" name="seen_by" value="me" checked[!shared];
                        span { "Only me" }
                    }
                    label class="choice" {
                        input type="radio" name="seen_by" value="group" checked[shared];
                        span { "The whole group" }
                    }

                    p class="field__hint" {
                        "Private lines stay in your repository until you share them with the group."
                    }
                }

                (super::layout::dialog_actions(&back, "Create line", false))
            }
        },
    )
}

/// The sharing screen: two questions and one warning.
fn offered(
    signed: &SignedIn,
    group: &Group,
    name: &Name,
    source: &str,
    held: &[String],
    problem: Option<&str>,
) -> Markup {
    let travelling: Vec<String> = Line::read(source)
        .map(|line| {
            line.nodes
                .iter()
                .filter(|node| node.siglum.kind == NodeKind::Definition)
                .map(|node| match &node.name {
                    Some(called) => format!("{} ({})", called, node.siglum),
                    None => node.siglum.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    let back = format!("/g/{}/private/{}", group.slug(), name);

    super::layout::dialog(
        "Share this line",
        &back,
        html! {
            p class="dialog__text" {
                "It moves to the group's shared repository, where every member of "
                (group.name) " can read it."
            }

            @if let Some(problem) = problem {
                (super::layout::message(super::layout::Kind::Error, problem))
            }

            @if !held.is_empty() {
                (super::layout::message(
                    super::layout::Kind::Warning,
                    "This line depends on private work. Share these lines first, or remove the references to them.",
                ))
                ul class="dialog__list" {
                    @for path in held {
                        li { span class="identifier" { (path) } }
                    }
                }
            }

            form method="post"
                 action={ "/g/" (group.slug()) "/private/" (name) "/share" } {
                (signature(signed))

                fieldset class="fieldset" {
                    legend { "History to share" }

                    label class="choice" {
                        input type="radio" name="travels" value="today" checked;
                        span { "Today's state only" }
                    }
                    label class="choice" {
                        input type="radio" name="travels" value="everything";
                        span { "Everything from the beginning" }
                    }
                }

                fieldset class="fieldset" {
                    legend { "Who can write" }

                    label class="choice" {
                        input type="radio" name="writing" value="author" checked;
                        span { "Only me; others send proposals" }
                    }
                    label class="choice" {
                        input type="radio" name="writing" value="anyone";
                        span { "Anyone in the group, one at a time" }
                    }
                }

                div class="field" {
                    span class="field__label" { "What travels with the line" }
                    @if travelling.is_empty() {
                        span class="field__hint" { "No definitions of your own." }
                    } @else {
                        ul class="dialog__list" {
                            @for definition in &travelling {
                                li { (definition) }
                            }
                        }
                        span class="field__hint" {
                            "These definitions move with the line. Sharing does not add them to the vault."
                        }
                    }
                }

                div class="dialog__actions" {
                    a class="button" href=(back) data-dialog-close { "Cancel" }
                    button class="button button--primary" type="submit" disabled[!held.is_empty()] {
                        "Share"
                    }
                }
            }
        },
    )
}

/// Starts a line from a machine.
pub async fn api_start(
    State(instance): State<Instance>,
    credential: super::api::Credential,
    axum::Json(asking): axum::Json<AskingToStart>,
) -> Result<Response, WebError> {
    let group = group_of(&instance, &asking.group, &credential.user.id).await?;
    let side = match asking.seen_by.trim() {
        "group" => Side::Shared,
        _ => Side::Private,
    };

    let started = born(
        &instance,
        &credential.user,
        &group,
        asking.title.trim(),
        asking.description.as_deref().unwrap_or_default().trim(),
        side,
    )
    .await?;

    Ok(axum::Json(placed(&started)).into_response())
}

/// Moves a line to the group from a machine.
pub async fn api_share(
    State(instance): State<Instance>,
    credential: super::api::Credential,
    Path((slug, line)): Path<(String, String)>,
    axum::Json(asking): axum::Json<AskingToShare>,
) -> Result<Response, WebError> {
    let group = group_of(&instance, &slug, &credential.user.id).await?;
    let Ok(name) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };

    let shared = moved(
        &instance,
        &credential.user,
        &group,
        &name,
        asking.travels.trim() == "everything",
        asking.writing.trim().parse().unwrap_or(WhoWrites::Anyone),
    )
    .await?;

    Ok(axum::Json(placed(&shared)).into_response())
}

/// Where a line is now, as a machine is told after changing it.
fn placed(line: &lines::Started) -> serde_json::Value {
    serde_json::json!({
        "id": line.id.to_string(),
        "group": line.group.slug().as_str(),
        "side": line.side.as_str(),
        "line": line.name.as_str(),
        "revision": line.revision,
    })
}

/// What a machine sends to start a line.
#[derive(Debug, Default, Deserialize)]
pub struct AskingToStart {
    /// The group's short name.
    #[serde(default)]
    pub group: String,
    /// What the line is called.
    #[serde(default)]
    pub title: String,
    /// What it is about, where its author says.
    #[serde(default)]
    pub description: Option<String>,
    /// `me` or `group`.
    #[serde(default)]
    pub seen_by: String,
}

/// What a machine sends to move a line to the group.
#[derive(Debug, Default, Deserialize)]
pub struct AskingToShare {
    /// `today` or `everything`.
    #[serde(default)]
    pub travels: String,
    /// `author` or `anyone`.
    #[serde(default)]
    pub writing: String,
}

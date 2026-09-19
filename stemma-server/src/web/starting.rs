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
use stemma_core::id::{GroupId, Slug};
use stemma_core::line::{Line, Name, NodeKind};
use stemma_core::repository::{self, Side};

use super::layout::{Chrome, page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::groups::{self, Group};
use crate::store::lines;

/// What the form that starts a line sends.
#[derive(Debug, Default, Deserialize)]
pub struct Starting {
    /// What the line is called.
    #[serde(default)]
    pub title: String,
    /// What it exists to answer.
    #[serde(default)]
    pub question: String,
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
    let group = admit(&instance, &signed, &slug).await?;
    let policy = instance.policy().await?;
    let chrome = Chrome::anonymous(
        "New line".to_owned(),
        super::front::instance_name(&instance, &policy),
    )
    .signed_in(signed.user.clone(), signed.maintainer);

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

    let group = admit(&instance, &signed, &slug).await?;
    let side = match form.seen_by.trim() {
        "group" => Side::Shared,
        _ => Side::Private,
    };

    match born(
        &instance,
        &signed.user,
        &group,
        form.title.trim(),
        form.question.trim(),
        side,
    )
    .await
    {
        Ok(name) => {
            Ok(Redirect::to(&format!("/g/{}/{}/{}", group.slug(), side, name)).into_response())
        }
        Err(WebError::BadRequest(problem)) => {
            let policy = instance.policy().await?;
            let chrome = Chrome::anonymous(
                "New line".to_owned(),
                super::front::instance_name(&instance, &policy),
            )
            .signed_in(signed.user.clone(), signed.maintainer);

            Ok(page(&chrome, asked(&signed, &group, Some(&problem), Some(&form))).into_response())
        }
        Err(other) => Err(other),
    }
}

/// Starts a line, wherever the asking came from.
async fn born(
    instance: &Instance,
    who: &crate::store::users::User,
    group: &Group,
    title: &str,
    question: &str,
    side: Side,
) -> Result<Name, WebError> {
    let refuse = |what: &str| WebError::BadRequest(what.to_owned());

    if title.is_empty() {
        return Err(refuse("A line needs a title."));
    }
    if question.is_empty() {
        return Err(refuse(
            "A line needs the question it exists to answer. That is what makes it a line \
             and not a folder.",
        ));
    }

    let Some(name) = Name::from_title(title) else {
        return Err(refuse(
            "That title has nothing a directory can be named after.",
        ));
    };

    let path = match side {
        Side::Shared => instance.repositories().shared(&group.id),
        Side::Private => instance.repositories().personal(&group.id, &who.id),
    };

    if crate::git::holds_line(&path, &name) {
        return Err(refuse("There is already a line by that name here."));
    }

    {
        let path = path.clone();
        let name = name.clone();
        let source = skeleton(title, question);
        let author_name = who.display_name.clone();
        let author_email = who.email.clone();

        instance
            .on_disk(move || {
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

    {
        let id = group.id.clone();
        let named = name.clone();
        let who = who.id.clone();

        instance
            .write(move |connection| {
                match side {
                    Side::Shared => {
                        lines::share(connection, &id, &named, &who, WhoWrites::Anyone)?;
                    }
                    Side::Private => {
                        lines::start(connection, &id, side, &named, &who)?;
                    }
                }

                Ok(())
            })
            .await?;
    }

    Ok(name)
}

/// What a line's source says before anybody has written mathematics.
fn skeleton(title: &str, question: &str) -> String {
    format!("= {title}\n\n#question[{question}]\n")
}

/// The two questions and the one warning.
pub async fn share_form(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((slug, line)): Path<(String, String)>,
) -> Result<Response, WebError> {
    let group = admit(&instance, &signed, &slug).await?;
    let Ok(name) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };

    let source = mine(&instance, &signed.user, &group.id, &name).await?;
    let policy = instance.policy().await?;
    let chrome = Chrome::anonymous(
        format!("Share {name}"),
        super::front::instance_name(&instance, &policy),
    )
    .signed_in(signed.user.clone(), signed.maintainer);

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

    let group = admit(&instance, &signed, &slug).await?;
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
        Ok(()) => Ok(Redirect::to(&format!("/g/{}/shared/{}", group.slug(), name)).into_response()),
        Err(WebError::BadRequest(problem)) => {
            let source = mine(&instance, &signed.user, &group.id, &name).await?;
            let held = held_back(&instance, &group.id, &source).await?;
            let policy = instance.policy().await?;
            let chrome = Chrome::anonymous(
                format!("Share {name}"),
                super::front::instance_name(&instance, &policy),
            )
            .signed_in(signed.user.clone(), signed.maintainer);

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
) -> Result<(), WebError> {
    let source = mine(instance, who, &group.id, name).await?;
    let held = held_back(instance, &group.id, &source).await?;

    if !held.is_empty() {
        return Err(WebError::BadRequest(
            "Nothing the group can see may lean on something only you can.".to_owned(),
        ));
    }

    let repositories = instance.repositories();
    let ours = repositories.shared(&group.id);
    let theirs = repositories.personal(&group.id, &who.id);
    let moving = name.clone();
    let author_name = who.display_name.clone();
    let author_email = who.email.clone();

    instance
        .on_disk(move || {
            // The group receives it first, so nothing can be lost
            // between the two commits.
            crate::git::bring_line(&ours, &theirs, &moving, history)?;
            crate::git::drop_line(
                &theirs,
                &moving,
                &crate::git::Author {
                    name: &author_name,
                    email: &author_email,
                },
            )?;

            Ok(())
        })
        .await?;

    let id = group.id.clone();
    let named = name.clone();
    let author = who.id.clone();

    instance
        .write(move |connection| {
            lines::share(connection, &id, &named, &author, writing)?;

            Ok(())
        })
        .await?;

    Ok(())
}

/// The line's source in this person's own repository.
async fn mine(
    instance: &Instance,
    who: &crate::store::users::User,
    group: &GroupId,
    name: &Name,
) -> Result<String, WebError> {
    let path = instance.repositories().personal(group, &who.id);
    let inside = format!("{}/{}/{}", repository::LINES, name, repository::ENTRY);

    instance
        .on_disk(move || {
            let head =
                crate::git::head_commit(&path).ok_or_else(|| anyhow::anyhow!("nothing here"))?;

            crate::git::show(&path, &head, &inside)
        })
        .await
        .map_err(|_| WebError::NotFound)
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
    let imports = stemma_core::source::imports(source);

    if imports.iter().all(|path| !path.starts_with("..")) {
        return Ok(Vec::new());
    }

    let shared = instance.repositories().shared(group);
    let known: Vec<String> = instance
        .on_disk(move || {
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
    let question = given
        .map(|given| given.question.clone())
        .unwrap_or_default();
    let shared = given.is_some_and(|given| given.seen_by.trim() == "group");

    html! {
        div class="sheet--narrow stack" {
            p class="label" {
                a href={ "/g/" (group.slug()) } { (group.name) }
            }
            h1 { "New line" }

            @if let Some(problem) = problem {
                p class="problem" { (problem) }
            }

            form method="post" action={ "/g/" (group.slug()) "/lines" } class="stack" {
                (signature(signed))

                label class="field" {
                    span class="label" { "Title" }
                    input type="text" name="title" value=(title) autofocus required;
                }

                label class="field" {
                    span class="label" { "The question" }
                    textarea name="question" rows="3" required { (question) }
                    span class="field__hint" {
                        "What this line exists to answer. It is what a listing shows "
                        "when the title is not enough."
                    }
                }

                fieldset {
                    legend class="label" { "Who can see it" }

                    label class="choice" {
                        input type="radio" name="seen_by" value="me" checked[!shared];
                        span { "Only me" }
                    }
                    label class="choice" {
                        input type="radio" name="seen_by" value="group" checked[shared];
                        span { "The whole group" }
                    }

                    p class="field__hint" {
                        "A line only you can see sits in your own repository, which "
                        "nobody else can clone. You can move it to the group later."
                    }
                }

                button class="button" type="submit" { "Start the line" }
            }
        }
    }
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

    html! {
        div class="sheet--narrow stack" {
            p class="label" {
                a href={ "/g/" (group.slug()) } { (group.name) }
                " · " a href={ "/g/" (group.slug()) "/private/" (name) } { (name.as_str()) }
            }
            h1 { "Share this line" }
            p class="quiet" {
                "Sharing is a move. The group receives the line; what you wrote "
                "getting there stays with you."
            }

            @if let Some(problem) = problem {
                p class="problem" { (problem) }
            }

            @if !held.is_empty() {
                div class="block" {
                    p class="label" { "This line leans on work nobody else can see" }
                    ul class="register" {
                        @for path in held {
                            li { span class="identifier" { (path) } }
                        }
                    }
                    p class="quiet" {
                        "Share what it leans on first, or take the reference out."
                    }
                }
            }

            form method="post"
                 action={ "/g/" (group.slug()) "/private/" (name) "/share" }
                 class="stack" {
                (signature(signed))

                fieldset {
                    legend class="label" { "What the group sees of what you have written" }

                    label class="choice" {
                        input type="radio" name="travels" value="today" checked;
                        span { "Today's state only" }
                    }
                    label class="choice" {
                        input type="radio" name="travels" value="everything";
                        span { "Everything from the beginning" }
                    }
                }

                fieldset {
                    legend class="label" { "Who can write" }

                    label class="choice" {
                        input type="radio" name="writing" value="author" checked;
                        span { "Only me. Everybody else proposes" }
                    }
                    label class="choice" {
                        input type="radio" name="writing" value="anyone";
                        span { "Anybody in the group, one at a time" }
                    }
                }

                div class="block" {
                    p class="label" { "What travels with the line" }

                    @if travelling.is_empty() {
                        p class="quiet" { "No definitions of your own." }
                    } @else {
                        ul class="register" {
                            @for definition in &travelling {
                                li { span class="register__name" { (definition) } }
                            }
                        }
                        p class="quiet" {
                            "These move with the line as ordinary definitions. None of "
                            "them enters the vault by being shared: that is a separate, "
                            "deliberate act."
                        }
                    }
                }

                button class="button" type="submit" disabled[!held.is_empty()] {
                    "Move it to the group"
                }
            }
        }
    }
}

/// Starts a line from a machine.
pub async fn api_start(
    State(instance): State<Instance>,
    credential: super::api::Credential,
    axum::Json(asking): axum::Json<AskingToStart>,
) -> Result<Response, WebError> {
    let group = belongs(&instance, &credential.user, &asking.group).await?;
    let side = match asking.seen_by.trim() {
        "group" => Side::Shared,
        _ => Side::Private,
    };

    let name = born(
        &instance,
        &credential.user,
        &group,
        asking.title.trim(),
        asking.question.trim(),
        side,
    )
    .await?;

    Ok(axum::Json(serde_json::json!({
        "group": group.slug().as_str(),
        "side": side.as_str(),
        "line": name.as_str(),
    }))
    .into_response())
}

/// Moves a line to the group from a machine.
pub async fn api_share(
    State(instance): State<Instance>,
    credential: super::api::Credential,
    Path((slug, line)): Path<(String, String)>,
    axum::Json(asking): axum::Json<AskingToShare>,
) -> Result<Response, WebError> {
    let group = belongs(&instance, &credential.user, &slug).await?;
    let Ok(name) = Name::parse(&line) else {
        return Err(WebError::NotFound);
    };

    moved(
        &instance,
        &credential.user,
        &group,
        &name,
        asking.travels.trim() == "everything",
        asking.writing.trim().parse().unwrap_or(WhoWrites::Anyone),
    )
    .await?;

    Ok(axum::Json(serde_json::json!({
        "group": group.slug().as_str(),
        "side": "shared",
        "line": name.as_str(),
    }))
    .into_response())
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
    /// What it exists to answer.
    #[serde(default)]
    pub question: String,
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

/// The group, if this account is in it.
async fn belongs(
    instance: &Instance,
    who: &crate::store::users::User,
    slug: &str,
) -> Result<Group, WebError> {
    let Ok(slug) = Slug::parse(slug.trim()) else {
        return Err(WebError::NotFound);
    };
    let who = who.id.clone();

    let found = instance
        .read(move |connection| {
            let Some(group) = groups::by_slug(connection, &slug)? else {
                return Ok(None);
            };

            match groups::role_of(connection, &group.id, &who)?.is_some() {
                true => Ok(Some(group)),
                false => Ok(None),
            }
        })
        .await?;

    found.ok_or(WebError::NotFound)
}

/// The group, if this reader is in it.
async fn admit(instance: &Instance, signed: &SignedIn, slug: &str) -> Result<Group, WebError> {
    belongs(instance, &signed.user, slug).await
}

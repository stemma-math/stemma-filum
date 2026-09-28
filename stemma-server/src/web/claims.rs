//! Taking a line, letting it go, and asking for it back.
//!
//! A claim is held by a machine, so every one of these is answered to a
//! machine's credential rather than to a session — except letting go,
//! which a person may also do from the web, because both machines are
//! theirs and a laptop left shut somewhere else should not keep them out
//! of their own work.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Json, Redirect, Response};
use serde::Deserialize;
use serde_json::json;
use stemma_core::claim::Standing;
use stemma_core::id::{GroupId, Slug};
use stemma_core::line::Name;
use stemma_core::repository::Side;

use super::api::Credential;
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::claims::{self, Claim};
use crate::store::groups;

/// Which line a machine is talking about.
#[derive(Debug, Deserialize)]
pub struct About {
    /// The group's short name.
    pub group: String,
    /// Which of its repositories.
    pub side: String,
    /// Which line of work.
    pub line: String,
}

impl About {
    /// The line it names, when the names are ones this instance uses.
    fn read(&self) -> Result<(GroupId, Side, Name), WebError> {
        let slug = Slug::parse(self.group.trim()).map_err(|_| WebError::NotFound)?;
        let side: Side = self.side.parse().map_err(|_| {
            WebError::BadRequest("That is not one of a group's repositories.".to_owned())
        })?;
        let line = Name::parse(self.line.trim()).map_err(|_| WebError::NotFound)?;

        Ok((GroupId::new(slug), side, line))
    }
}

/// Takes a line for the machine asking, unless another one has it.
pub async fn take(
    State(instance): State<Instance>,
    credential: Credential,
    Json(about): Json<About>,
) -> Result<Response, WebError> {
    let machine = super::machines::registered(&instance, &credential).await?;
    let (group, side, line) = about.read()?;
    let owner = machine.owner.clone();
    let id = machine.id;

    let claim = instance
        .write(move |connection| {
            in_the_group(connection, &group, &owner)?;

            // A shared line its author kept is theirs to write, and
            // everybody else proposes. Nothing about that is a warning:
            // the machine simply is not the one writing it.
            if let Some(started) =
                crate::store::lines::by_name(connection, &group, side, &line, &owner)?
                && (side == Side::Private && started.author != owner || !started.admits(&owner))
            {
                anyhow::bail!("only its author can edit this line; others send proposals")
            }

            if let Some(claim) = claims::holder(connection, &group, side, line.as_str(), &owner)? {
                visible(&claim, &owner)?;
            }
            claims::take(connection, &group, side, line.as_str(), &id, &owner)
        })
        .await
        .map_err(refused)?;

    Ok(Json(standing(&claim, &machine.id)).into_response())
}

/// Says the machine is still writing.
pub async fn renew(
    State(instance): State<Instance>,
    credential: Credential,
    Json(about): Json<About>,
) -> Result<Response, WebError> {
    let machine = super::machines::registered(&instance, &credential).await?;
    let (group, side, line) = about.read()?;
    let id = machine.id;
    let owner = machine.owner.clone();

    let claim = instance
        .write(move |connection| {
            in_the_group(connection, &group, &owner)?;
            let Some(claim) = claims::holder(connection, &group, side, line.as_str(), &owner)?
            else {
                anyhow::bail!("nothing here");
            };

            visible(&claim, &owner)?;
            if claim.machine == id {
                claims::renew(connection, &claim.id)?;
            }

            claims::holder(connection, &group, side, line.as_str(), &owner)?.context_or_nothing()
        })
        .await
        .map_err(nothing_here)?;

    Ok(Json(standing(&claim, &machine.id)).into_response())
}

/// Lets a line go, from the machine holding it.
pub async fn release(
    State(instance): State<Instance>,
    credential: Credential,
    Json(about): Json<About>,
) -> Result<Response, WebError> {
    let machine = super::machines::registered(&instance, &credential).await?;
    let (group, side, line) = about.read()?;
    let id = machine.id;
    let owner = machine.owner.clone();

    let released = instance
        .write(move |connection| {
            if let Some(claim) = claims::holder(connection, &group, side, line.as_str(), &owner)?
                && claim.machine == id
            {
                claims::release(connection, &claim.id)?;
            }

            crate::store::lines::by_name(connection, &group, side, &line, &owner)
        })
        .await?;

    // Letting go ends the writing, and git gets it now rather than when
    // the stretch would have gone quiet.
    if let Some(line) = released {
        crate::history::settle(&instance, line.id).await?;
    }

    Ok(Json(json!({ "standing": "open" })).into_response())
}

/// Asks the machine writing a line to hand it back.
pub async fn ask(
    State(instance): State<Instance>,
    credential: Credential,
    Json(about): Json<About>,
) -> Result<Response, WebError> {
    let machine = super::machines::registered(&instance, &credential).await?;
    let (group, side, line) = about.read()?;
    let id = machine.id;
    let owner = machine.owner.clone();

    let claim = instance
        .write(move |connection| {
            in_the_group(connection, &group, &owner)?;

            let Some(claim) = claims::holder(connection, &group, side, line.as_str(), &owner)?
            else {
                anyhow::bail!("nothing here");
            };

            visible(&claim, &owner)?;
            claims::ask(connection, &claim.id, Some(&id))?;

            claims::holder(connection, &group, side, line.as_str(), &owner)?.context_or_nothing()
        })
        .await
        .map_err(nothing_here)?;

    Ok(Json(standing(&claim, &machine.id)).into_response())
}

/// Every line being written in the groups this machine's owner is in.
pub async fn listing(
    State(instance): State<Instance>,
    credential: Credential,
) -> Result<Response, WebError> {
    let machine = super::machines::registered(&instance, &credential).await?;
    let owner = machine.owner.clone();
    let id = machine.id;

    let held = instance
        .read(move |connection| {
            let mut found = Vec::new();

            for membership in groups::for_user(connection, &owner)? {
                found.extend(
                    claims::for_group(connection, &membership.group.id)?
                        .into_iter()
                        .filter(|claim| claim.side == Side::Shared || claim.owner == owner),
                );
            }

            Ok(found)
        })
        .await?;

    let listed: Vec<serde_json::Value> = held
        .iter()
        .map(|claim| {
            json!({
                "group": claim.group.slug().as_str(),
                "side": claim.side.as_str(),
                "line": claim.line,
                "machine": claim.machine.to_string(),
                "label": claim.label,
                "mine": claim.machine == id,
                "left": claim.left,
                "asked": claim.asked,
            })
        })
        .collect();

    Ok(Json(json!({ "claims": listed })).into_response())
}

/// Proof that the release came from the signed-in session's form.
#[derive(Debug, Deserialize)]
pub struct ReleaseForm {
    /// The session's CSRF token.
    pub form_token: String,
}

/// Hands a line back from the web.
///
/// Only the person whose machine is holding it: the claim belongs to a
/// device, but every device belongs to somebody.
pub async fn hand_back(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path((group, side, line)): Path<(String, String, String)>,
    axum::Form(form): axum::Form<ReleaseForm>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;
    let about = About {
        group: group.clone(),
        side: side.clone(),
        line: line.clone(),
    };
    let (id, chosen, name) = about.read()?;
    let who = signed.user.id.clone();

    let released = instance
        .write(move |connection| {
            let Some(claim) = claims::holder(connection, &id, chosen, name.as_str(), &who)? else {
                return Ok(None);
            };

            if claim.owner != who {
                anyhow::bail!("nothing here");
            }

            claims::release(connection, &claim.id)?;
            crate::store::lines::by_name(connection, &id, chosen, &name, &who)
        })
        .await
        .map_err(nothing_here)?;

    if let Some(line) = released {
        crate::history::settle(&instance, line.id).await?;
    }

    Ok(Redirect::to(&format!("/g/{group}/{side}/{line}")).into_response())
}

/// What a machine is told about a claim it asked about.
fn standing(claim: &Claim, asking: &stemma_core::id::MachineId) -> serde_json::Value {
    let held = claim.held();
    let standing = Standing::of(asking, Some(&held));

    json!({
        "standing": match standing {
            Standing::Writing => "writing",
            Standing::Waiting { .. } => "waiting",
            Standing::Open => "open",
        },
        "machine": claim.machine.to_string(),
        "label": claim.label,
        "left": claim.left,
        "asked": claim.asked,
        "said": standing.said(),
    })
}

/// A personal claim reveals activity only to its owner.
fn visible(claim: &Claim, who: &stemma_core::id::UserId) -> anyhow::Result<()> {
    if claim.side == Side::Private && &claim.owner != who {
        anyhow::bail!("nothing here");
    }
    Ok(())
}

/// Refuses somebody who is not in the group, the way every page does.
fn in_the_group(
    connection: &rusqlite::Connection,
    group: &GroupId,
    who: &stemma_core::id::UserId,
) -> anyhow::Result<()> {
    match groups::role_of(connection, group, who)?.is_some() {
        true => Ok(()),
        false => anyhow::bail!("nothing here"),
    }
}

/// Turns the one refusal these handlers raise into a missing page.
fn nothing_here(error: WebError) -> WebError {
    match error {
        WebError::Internal(reason) if reason.to_string() == "nothing here" => WebError::NotFound,
        other => other,
    }
}

/// The same, plus the one refusal that is worth saying out loud.
fn refused(error: WebError) -> WebError {
    match error {
        WebError::Internal(reason)
            if reason.to_string().starts_with("only its author can edit") =>
        {
            WebError::Forbidden(capitalised(&reason.to_string()))
        }
        other => nothing_here(other),
    }
}

/// A reason, as a sentence.
fn capitalised(reason: &str) -> String {
    let mut characters = reason.chars();

    match characters.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), characters.as_str()),
        None => reason.to_owned(),
    }
}

/// A claim that has to be there by the time it is read back.
trait Reread {
    /// The claim, or the one refusal these handlers raise.
    fn context_or_nothing(self) -> anyhow::Result<Claim>;
}

impl Reread for Option<Claim> {
    fn context_or_nothing(self) -> anyhow::Result<Claim> {
        self.ok_or_else(|| anyhow::anyhow!("nothing here"))
    }
}

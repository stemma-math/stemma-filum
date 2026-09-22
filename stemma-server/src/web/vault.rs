//! The group's immutable definitions, proved bridges and audit records.

use anyhow::Context;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Json, Redirect, Response};
use maud::{Markup, html};
use serde::Deserialize;
use serde_json::json;
use stemma_core::artefact::Scope;
use stemma_core::id::{BridgeId, Slug, UserId, VaultEntryId};
use stemma_core::line::{NodeKind, Siglum};
use stemma_core::model::Correspondence;
use stemma_core::vault::{Bridge, BridgeKind, DownstreamDecision, Entry};

use super::api::Credential;
use super::layout::{page, signature};
use super::visitor::SignedIn;
use super::{Instance, WebError};
use crate::store::{groups, vault};

#[derive(Deserialize)]
/// A request to admit a compiled definition.
pub struct Admission {
    siglum: String,
    name: String,
    prose: String,
    formalization: String,
    form_token: String,
}

#[derive(Deserialize)]
/// A direct bridge or an on-demand composition request.
pub struct Bridging {
    left: String,
    right: String,
    kind: String,
    theorem: String,
    #[serde(default)]
    first_parent: String,
    #[serde(default)]
    second_parent: String,
    form_token: String,
}

#[derive(Deserialize)]
/// A signed correspondence audit action.
pub struct Auditing {
    target_kind: String,
    target: String,
    action: String,
    note: String,
    form_token: String,
}

#[derive(Deserialize)]
/// One affected statement assigned to its author.
pub struct Assignment {
    bridge: String,
    statement: String,
    assigned_to: String,
    form_token: String,
}

#[derive(Deserialize)]
/// The answer to one downstream obligation.
pub struct Deciding {
    decision: String,
    note: String,
    form_token: String,
}

#[derive(Deserialize)]
/// Definition already checked by the local client and ready for admission.
pub struct ApiAdmission {
    /// Stable source siglum.
    pub siglum: String,
    /// Distinguishing name.
    pub name: String,
    /// Exact prose.
    pub prose: String,
    /// Exact compiled Lean declaration.
    pub formalization: String,
}

#[derive(Deserialize)]
/// A compiled direct bridge submitted by a client.
pub struct ApiBridge {
    /// Left vault definition.
    pub left: String,
    /// Right vault definition.
    pub right: String,
    /// Predicate or structure equivalence.
    pub kind: String,
    /// Exact compiled unconditional theorem.
    pub theorem: String,
}

#[derive(Deserialize)]
/// A signed audit submitted by a client.
pub struct ApiAudit {
    /// Entry or bridge.
    pub target_kind: String,
    /// Stable object identifier.
    pub target: String,
    /// Attest or withdraw.
    pub action: String,
    /// What was checked or why it was withdrawn.
    pub note: String,
}

/// Lists vault objects for a non-browser client.
pub async fn api_view(
    State(instance): State<Instance>,
    credential: Credential,
    Path(slug): Path<String>,
) -> Result<Response, WebError> {
    let group = credential_group(&instance, &credential, &slug).await?;
    let id = group.id;
    let (entries, bridges) = instance
        .read(move |connection| {
            Ok((
                vault::entries(connection, &id)?,
                vault::bridges(connection, &id)?,
            ))
        })
        .await?;
    Ok(Json(json!({
        "entries": entries.into_iter().map(|stored| json!({
            "id": stored.entry.id, "siglum": stored.entry.siglum, "name": stored.entry.name,
            "prose": stored.entry.prose, "formalization": stored.entry.formalization,
            "correspondence": correspondence_word(stored.entry.correspondence),
            "admitted_by": stored.admitted_by, "admitted_at": stored.admitted_at
        })).collect::<Vec<_>>(),
        "bridges": bridges.into_iter().map(|stored| json!({
            "id": stored.bridge.id, "left": stored.bridge.left, "right": stored.bridge.right,
            "kind": format!("{:?}", stored.bridge.kind).to_ascii_lowercase(),
            "correspondence": correspondence_word(stored.bridge.correspondence),
            "derived": matches!(stored.bridge.origin, stemma_core::vault::BridgeOrigin::Derived { .. })
        })).collect::<Vec<_>>()
    })).into_response())
}

/// Admits a locally verified definition for a CLI or desktop client.
pub async fn api_admit(
    State(instance): State<Instance>,
    credential: Credential,
    Path(slug): Path<String>,
    Json(form): Json<ApiAdmission>,
) -> Result<Response, WebError> {
    let group = credential_group(&instance, &credential, &slug).await?;
    let siglum: Siglum = form
        .siglum
        .parse()
        .map_err(|_| WebError::BadRequest("That is not a siglum.".to_owned()))?;
    if siglum.kind != NodeKind::Definition {
        return Err(WebError::BadRequest(
            "Only a definition enters the vault.".to_owned(),
        ));
    }
    require_published_verification(&instance, &group.id, &siglum).await?;
    let entry = Entry::admit(group.id, siglum, form.name, form.prose, form.formalization)
        .map_err(|error| WebError::BadRequest(error.to_string()))?;
    let id = entry.id;
    let by = credential.user.id;
    instance
        .read(move |connection| vault::admit(connection, &entry, &by))
        .await?;
    Ok(Json(json!({"entry": id})).into_response())
}

/// Admits a locally compiled direct bridge for a client.
pub async fn api_bridge(
    State(instance): State<Instance>,
    credential: Credential,
    Path(slug): Path<String>,
    Json(form): Json<ApiBridge>,
) -> Result<Response, WebError> {
    let group = credential_group(&instance, &credential, &slug).await?;
    let left: VaultEntryId = form
        .left
        .parse()
        .map_err(|_| WebError::BadRequest("That is not a vault entry.".to_owned()))?;
    let right: VaultEntryId = form
        .right
        .parse()
        .map_err(|_| WebError::BadRequest("That is not a vault entry.".to_owned()))?;
    let kind = match form.kind.as_str() {
        "predicate" => BridgeKind::Predicate,
        "structure" => BridgeKind::Structure,
        _ => {
            return Err(WebError::BadRequest(
                "That is not a bridge kind.".to_owned(),
            ));
        }
    };
    let bridge = Bridge::direct(left, right, kind, form.theorem)
        .map_err(|error| WebError::BadRequest(error.to_string()))?;
    let id = bridge.id;
    let group_id = group.id;
    let by = credential.user.id;
    instance
        .read(move |connection| {
            let both_here = [bridge.left, bridge.right].into_iter().all(|id| {
                vault::entry(connection, &id)
                    .ok()
                    .flatten()
                    .is_some_and(|entry| entry.entry.group == group_id)
            });
            if !both_here {
                anyhow::bail!("both definitions must be in this group's vault")
            }
            vault::admit_bridge(connection, &group_id, &bridge, &by)
        })
        .await?;
    Ok(Json(json!({"bridge": id})).into_response())
}

/// Records a correspondence action for a client.
pub async fn api_audit(
    State(instance): State<Instance>,
    credential: Credential,
    Path(slug): Path<String>,
    Json(form): Json<ApiAudit>,
) -> Result<Response, WebError> {
    let group = credential_group(&instance, &credential, &slug).await?;
    let group_id = group.id;
    let by = credential.user.id;
    let id = instance
        .write(move |connection| {
            let belongs = match form.target_kind.as_str() {
                "entry" => form
                    .target
                    .parse::<VaultEntryId>()
                    .ok()
                    .and_then(|id| vault::entry(connection, &id).ok().flatten())
                    .is_some_and(|entry| entry.entry.group == group_id),
                "bridge" => form.target.parse::<BridgeId>().ok().is_some_and(|id| {
                    vault::bridges(connection, &group_id)
                        .unwrap_or_default()
                        .iter()
                        .any(|bridge| bridge.bridge.id == id)
                }),
                _ => false,
            };
            if !belongs {
                anyhow::bail!("the audit target is not in this group's vault")
            }
            match form.action.as_str() {
                "attest" => {
                    vault::attest(connection, &form.target_kind, &form.target, &by, &form.note)
                }
                "withdraw" => {
                    vault::withdraw(connection, &form.target_kind, &form.target, &by, &form.note)
                }
                _ => anyhow::bail!("that is not an audit action"),
            }
        })
        .await?;
    Ok(Json(json!({"audit": id})).into_response())
}

/// Shows the vault as objects rather than unexplained graph edges.
pub async fn page_view(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
) -> Result<Response, WebError> {
    let group = member_group(&instance, &signed, &slug).await?;
    let id = group.id.clone();
    let (entries, bridges) = instance
        .read(move |connection| {
            Ok((
                vault::entries(connection, &id)?,
                vault::bridges(connection, &id)?,
            ))
        })
        .await?;
    let chrome = super::layout::around(&instance, &signed, format!("{} vault", group.name))
        .await?
        .at(format!("/g/{}", group.slug()));
    Ok(page(
        &chrome,
        body(&signed, &group.name, &slug, &entries, &bridges),
    )
    .into_response())
}

/// Admits a definition only when the latest shared artefact says Lean verified it.
pub async fn admit(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::Form(form): axum::Form<Admission>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;
    let group = member_group(&instance, &signed, &slug).await?;
    let siglum: Siglum = form
        .siglum
        .parse()
        .map_err(|_| WebError::BadRequest("That is not a siglum.".to_owned()))?;
    if siglum.kind != NodeKind::Definition {
        return Err(WebError::BadRequest(
            "Only a definition enters the vault.".to_owned(),
        ));
    }
    let nodes = super::lines::read(&instance, &group.id, &Scope::Shared)
        .await?
        .ok_or_else(|| {
            WebError::BadRequest("Build and publish the shared repository first.".to_owned())
        })?;
    let verified = nodes.lines.iter().any(|line| {
        line.line.node(&siglum).is_some()
            && line.lean.as_ref().is_some_and(|lean| {
                lean.verification_of(&siglum) == stemma_core::model::Verification::Verified
            })
    });
    if !verified {
        return Err(WebError::BadRequest(
            "The latest shared build does not contain a Lean-verified definition with that siglum."
                .to_owned(),
        ));
    }
    let entry = Entry::admit(group.id, siglum, form.name, form.prose, form.formalization)
        .map_err(|error| WebError::BadRequest(error.to_string()))?;
    let by = signed.user.id;
    instance
        .read(move |connection| vault::admit(connection, &entry, &by))
        .await?;
    Ok(Redirect::to(&format!("/g/{slug}/vault")).into_response())
}

/// Admits a direct bridge, or composes two audited parents on demand.
pub async fn bridge(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::Form(form): axum::Form<Bridging>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;
    let group = member_group(&instance, &signed, &slug).await?;
    let left: VaultEntryId = form
        .left
        .parse()
        .map_err(|_| WebError::BadRequest("That is not a vault entry.".to_owned()))?;
    let right: VaultEntryId = form
        .right
        .parse()
        .map_err(|_| WebError::BadRequest("That is not a vault entry.".to_owned()))?;
    let kind = match form.kind.as_str() {
        "predicate" => BridgeKind::Predicate,
        "structure" => BridgeKind::Structure,
        _ => {
            return Err(WebError::BadRequest(
                "That is not a bridge kind.".to_owned(),
            ));
        }
    };
    let group_id = group.id;
    let by = signed.user.id;
    instance
        .read(move |connection| {
            let bridge = if form.first_parent.trim().is_empty() {
                Bridge::direct(left, right, kind, form.theorem)?
            } else {
                let first: BridgeId = form
                    .first_parent
                    .parse()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let second: BridgeId = form
                    .second_parent
                    .parse()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let existing = vault::bridges(connection, &group_id)?;
                let first = existing
                    .iter()
                    .find(|item| item.bridge.id == first)
                    .context("the first parent is not in this vault")?;
                let second = existing
                    .iter()
                    .find(|item| item.bridge.id == second)
                    .context("the second parent is not in this vault")?;
                Bridge::compose(&first.bridge, &second.bridge, form.theorem)?
            };
            let both_here = [bridge.left, bridge.right].into_iter().all(|entry| {
                vault::entry(connection, &entry)
                    .ok()
                    .flatten()
                    .is_some_and(|entry| entry.entry.group == group_id)
            });
            if !both_here {
                anyhow::bail!("both definitions must be in this group's vault")
            }
            vault::admit_bridge(connection, &group_id, &bridge, &by)
        })
        .await?;
    Ok(Redirect::to(&format!("/g/{slug}/vault")).into_response())
}

/// Attests or withdraws correspondence while preserving the record.
pub async fn audit(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::Form(form): axum::Form<Auditing>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;
    let group = member_group(&instance, &signed, &slug).await?;
    let group_id = group.id.clone();
    let shared = if form.action == "withdraw" && form.target_kind == "bridge" {
        super::lines::read(&instance, &group_id, &Scope::Shared).await?
    } else {
        None
    };
    let by = signed.user.id;
    instance
        .write(move |connection| {
            let belongs = match form.target_kind.as_str() {
                "entry" => form
                    .target
                    .parse::<VaultEntryId>()
                    .ok()
                    .and_then(|id| vault::entry(connection, &id).ok().flatten())
                    .is_some_and(|entry| entry.entry.group == group_id),
                "bridge" => form.target.parse::<BridgeId>().ok().is_some_and(|id| {
                    vault::bridges(connection, &group_id)
                        .unwrap_or_default()
                        .iter()
                        .any(|bridge| bridge.bridge.id == id)
                }),
                _ => false,
            };
            if !belongs {
                anyhow::bail!("the audit target is not in this group's vault")
            }
            match form.action.as_str() {
                "attest" => {
                    vault::attest(connection, &form.target_kind, &form.target, &by, &form.note)
                        .map(|_| ())
                }
                "withdraw" => {
                    vault::withdraw(connection, &form.target_kind, &form.target, &by, &form.note)?;
                    if form.target_kind == "bridge" {
                        let target: BridgeId =
                            form.target.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
                        let bridges = vault::bridges(connection, &group_id)?;
                        let mut affected = vec![target];
                        loop {
                            let before = affected.len();
                            for stored in &bridges {
                                if let stemma_core::vault::BridgeOrigin::Derived { parents } =
                                    stored.bridge.origin
                                    && parents.iter().any(|parent| affected.contains(parent))
                                    && !affected.contains(&stored.bridge.id)
                                {
                                    affected.push(stored.bridge.id);
                                }
                            }
                            if affected.len() == before {
                                break;
                            }
                        }
                        let members = groups::members(connection, &group_id)?;
                        let owner = members
                            .iter()
                            .find(|member| member.role == stemma_core::access::GroupRole::Owner)
                            .context("the group has no owner")?
                            .user
                            .id
                            .clone();
                        if let Some(nodes) = &shared {
                            for node in nodes.lines.iter().flat_map(|line| &line.line.nodes) {
                                for bridge in node
                                    .bridges
                                    .iter()
                                    .filter(|bridge| affected.contains(bridge))
                                {
                                    let assigned = node
                                        .author
                                        .as_ref()
                                        .filter(|author| {
                                            members.iter().any(|member| &member.user.id == *author)
                                        })
                                        .unwrap_or(&owner);
                                    vault::assign(
                                        connection,
                                        &group_id,
                                        bridge,
                                        &node.siglum.to_string(),
                                        assigned,
                                    )?;
                                }
                            }
                        }
                    }
                    Ok(())
                }
                _ => anyhow::bail!("that is not an audit action"),
            }
        })
        .await?;
    Ok(Redirect::to(&format!("/g/{slug}/vault")).into_response())
}

/// Assigns a concrete downstream decision after a bridge audit was withdrawn.
pub async fn assign(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(slug): Path<String>,
    axum::Form(form): axum::Form<Assignment>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;
    let group = member_group(&instance, &signed, &slug).await?;
    let bridge: BridgeId = form
        .bridge
        .parse()
        .map_err(|_| WebError::BadRequest("That is not a bridge.".to_owned()))?;
    let assigned_to: UserId = form
        .assigned_to
        .parse()
        .map_err(|_| WebError::BadRequest("That is not a user.".to_owned()))?;
    let group_id = group.id;
    instance
        .read(move |connection| {
            if groups::role_of(connection, &group_id, &assigned_to)?.is_none() {
                anyhow::bail!("that person is not in the group")
            }
            let withdrawn = vault::bridges(connection, &group_id)?
                .into_iter()
                .any(|item| {
                    item.bridge.id == bridge
                        && item.bridge.correspondence == Correspondence::AuditWithdrawn
                });
            if !withdrawn {
                anyhow::bail!("only a withdrawn bridge creates this obligation")
            }
            vault::assign(
                connection,
                &group_id,
                &bridge,
                &form.statement,
                &assigned_to,
            )
            .map(|_| ())
        })
        .await?;
    Ok(Redirect::to(&format!("/g/{slug}/vault")).into_response())
}

/// Records the assigned person's answer and closes only the obligation.
pub async fn decide(
    State(instance): State<Instance>,
    signed: SignedIn,
    Path(id): Path<String>,
    axum::Form(form): axum::Form<Deciding>,
) -> Result<Response, WebError> {
    signed.check_form(&form.form_token)?;
    let decision = match form.decision.as_str() {
        "stands" => DownstreamDecision::Stands,
        "rewrite" => DownstreamDecision::Rewrite,
        "waiting_for_audit" => DownstreamDecision::WaitingForAudit,
        _ => {
            return Err(WebError::BadRequest(
                "That is not a downstream decision.".to_owned(),
            ));
        }
    };
    let by = signed.user.id;
    let recorded = instance
        .write(move |connection| vault::decide(connection, &id, &by, decision, &form.note))
        .await?;
    if !recorded {
        return Err(WebError::NotFound);
    }
    Ok(Redirect::to("/").into_response())
}

async fn member_group(
    instance: &Instance,
    signed: &SignedIn,
    slug: &str,
) -> Result<groups::Group, WebError> {
    let slug = Slug::parse(slug).map_err(|_| WebError::NotFound)?;
    let user = signed.user.id.clone();
    instance
        .read(move |connection| {
            let group = groups::by_slug(connection, &slug)?.context("group not found")?;
            if groups::role_of(connection, &group.id, &user)?.is_none() {
                anyhow::bail!("not a member")
            }
            Ok(group)
        })
        .await
        .map_err(|_| WebError::NotFound)
}

async fn credential_group(
    instance: &Instance,
    credential: &Credential,
    slug: &str,
) -> Result<groups::Group, WebError> {
    let slug = Slug::parse(slug).map_err(|_| WebError::NotFound)?;
    let user = credential.user.id.clone();
    instance
        .read(move |connection| {
            let group = groups::by_slug(connection, &slug)?.context("group not found")?;
            if groups::role_of(connection, &group.id, &user)?.is_none() {
                anyhow::bail!("not a member")
            }
            Ok(group)
        })
        .await
        .map_err(|_| WebError::NotFound)
}

async fn require_published_verification(
    instance: &Instance,
    group: &stemma_core::id::GroupId,
    siglum: &Siglum,
) -> Result<(), WebError> {
    let nodes = super::lines::read(instance, group, &Scope::Shared)
        .await?
        .ok_or_else(|| {
            WebError::BadRequest("Build and publish the shared repository first.".to_owned())
        })?;
    let verified = nodes.lines.iter().any(|line| {
        line.line.node(siglum).is_some()
            && line.lean.as_ref().is_some_and(|lean| {
                lean.verification_of(siglum) == stemma_core::model::Verification::Verified
            })
    });
    if verified {
        Ok(())
    } else {
        Err(WebError::BadRequest(
            "The latest shared build does not contain a Lean-verified definition with that siglum."
                .to_owned(),
        ))
    }
}

fn correspondence_word(value: Correspondence) -> &'static str {
    match value {
        Correspondence::NotAudited => "not_audited",
        Correspondence::Audited => "audited",
        Correspondence::AuditWithdrawn => "audit_withdrawn",
    }
}

fn body(
    signed: &SignedIn,
    group: &str,
    slug: &str,
    entries: &[vault::StoredEntry],
    bridges: &[vault::StoredBridge],
) -> Markup {
    html! {
        div class="stack" { p class="label" { a href={ "/g/" (slug) } { (group) } } h1 { "Vault" }
            p class="quiet" { "Definitions are immutable. Every line between them is a proved object with its own audit state." } }
        div class="block" { p class="label" { "Definitions · " (entries.len()) }
            ul class="register" { @for stored in entries { li { span class="register__name" { (stored.entry.name) }
                span class="quiet identifier" { (stored.entry.id) " · " (stored.entry.siglum) }
                span class="register__aside" { (format!("{:?}", stored.entry.correspondence)) } } } }
        }
        div class="block" { p class="label" { "Bridges · " (bridges.len()) }
            ul class="register" { @for stored in bridges { li { span class="register__name" { (stored.bridge.left) " ↔ " (stored.bridge.right) }
                span class="quiet identifier" { (stored.bridge.id) " · " (format!("{:?}", stored.bridge.kind)) }
                span class="register__aside" { (format!("{:?}", stored.bridge.correspondence)) } } } }
        }
        details { summary { "Admit a verified definition" } form method="post" action={ "/g/" (slug) "/vault/entries" } {
            (signature(signed)) input name="siglum" placeholder="def:topological-group" required; input name="name" placeholder="Topological group" required;
            textarea name="prose" placeholder="Exact prose" required {} textarea name="formalization" placeholder="Exact compiled Lean declaration" required {}
            button class="button" { "Admit definition" }
        } }
        details { summary { "Admit or derive a bridge" } form method="post" action={ "/g/" (slug) "/vault/bridges" } {
            (signature(signed)) input name="left" placeholder="vlt:…" required; input name="right" placeholder="vlt:…" required;
            select name="kind" { option value="predicate" { "predicate" } option value="structure" { "structure" } }
            textarea name="theorem" placeholder="Compiled unconditional Lean theorem" required {}
            input name="first_parent" placeholder="First audited parent (for derived bridge)"; input name="second_parent" placeholder="Second audited parent";
            button class="button" { "Admit bridge" }
        } }
        details { summary { "Record an audit" } form method="post" action={ "/g/" (slug) "/vault/audits" } {
            (signature(signed)) select name="target_kind" { option value="entry" { "definition" } option value="bridge" { "bridge" } }
            input name="target" placeholder="vlt:… or brg:…" required; select name="action" { option value="attest" { "attest" } option value="withdraw" { "withdraw" } }
            textarea name="note" placeholder="What was checked, or why the guarantee is withdrawn" required {} button class="button" { "Record signed audit" }
        } }
        @if bridges.iter().any(|stored| stored.bridge.correspondence == Correspondence::AuditWithdrawn) {
            details { summary { "Assign an affected downstream decision" }
                form method="post" action={ "/g/" (slug) "/vault/obligations" } {
                    (signature(signed)) input name="bridge" placeholder="Withdrawn brg:…" required;
                    input name="statement" placeholder="thm:affected" required;
                    input name="assigned_to" placeholder="usr:author" required;
                    button class="button" { "Assign decision" }
                }
            }
        }
    }
}

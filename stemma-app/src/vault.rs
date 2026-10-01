//! A group's vault of admitted definitions and bridges.

use stemma_core::vault::{AuditTarget, AuditVerb, BridgeKind};

use crate::{Session, carried_out, failed};

/// What `group` has admitted, and the bridges between it.
#[tauri::command]
pub async fn vault(group: String) -> Result<stemma::api::Vault, String> {
    crate::blocking(move || -> Result<stemma::api::Vault, String> {
        let session = Session::open()?;
        session.server.vault(&session.token, &group).map_err(failed)
    })
    .await
}

/// Admits a definition whose Lean module compiles on this machine.
#[tauri::command]
pub async fn vault_admit(
    group: String,
    siglum: String,
    name: String,
    prose: String,
    lean: String,
) -> Result<String, String> {
    crate::blocking(move || -> Result<String, String> {
        carried_out(|home, out| {
            stemma::vault::admit_text(home, &group, &siglum, &name, &prose, &lean, out)
        })
    })
    .await
}

/// Admits a proved licence to substitute one definition for another.
#[tauri::command]
pub async fn vault_bridge(
    group: String,
    left: String,
    right: String,
    kind: BridgeKind,
    lean: String,
) -> Result<String, String> {
    crate::blocking(move || -> Result<String, String> {
        carried_out(|home, out| {
            stemma::vault::bridge_text(home, &group, &left, &right, kind, &lean, out)
        })
    })
    .await
}

/// Attests a correspondence, or withdraws one.
#[tauri::command]
pub async fn vault_audit(
    group: String,
    target_kind: AuditTarget,
    target: String,
    action: AuditVerb,
    note: String,
) -> Result<String, String> {
    crate::blocking(move || -> Result<String, String> {
        carried_out(|home, out| {
            stemma::vault::audit(home, &group, target_kind, &target, action, &note, out)
        })
    })
    .await
}

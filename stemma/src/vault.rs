//! The complete command-line surface for the group vault.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use stemma_core::vault::{AuditTarget, AuditVerb, BridgeKind};

use crate::build;
use crate::config::Config;
use crate::home::Home;
use crate::session;

/// Lists definitions and bridges, including withdrawn objects as history.
pub fn list(home: &Home, group: &str, out: &mut impl Write) -> Result<()> {
    let (credentials, server) = session::connect(home)?;
    let vault = server.vault(&credentials.token, group)?;
    writeln!(out, "Definitions")?;
    for entry in vault.entries {
        writeln!(
            out,
            "  {}  {} · {} · {}",
            entry.id, entry.name, entry.siglum, entry.correspondence
        )?;
    }
    writeln!(out, "Bridges")?;
    for bridge in vault.bridges {
        writeln!(
            out,
            "  {}  {} ↔ {} · {} · {}{}",
            bridge.id,
            bridge.left,
            bridge.right,
            bridge.kind,
            bridge.correspondence,
            if bridge.derived { " · derived" } else { "" }
        )?;
    }
    Ok(())
}

/// Compiles and admits one definition.
pub fn admit(
    home: &Home,
    group: &str,
    siglum: &str,
    name: &str,
    prose: &Path,
    lean: &Path,
    out: &mut impl Write,
) -> Result<()> {
    let prose = std::fs::read_to_string(prose).context("cannot read the prose")?;
    let module =
        std::fs::read_to_string(lean).with_context(|| format!("cannot read {}", lean.display()))?;

    admit_text(home, group, siglum, name, &prose, &module, out)
}

/// The same, from text somebody typed rather than from files.
pub fn admit_text(
    home: &Home,
    group: &str,
    siglum: &str,
    name: &str,
    prose: &str,
    lean: &str,
    out: &mut impl Write,
) -> Result<()> {
    let module = checked(home, group, lean)?;
    let (credentials, server) = session::connect(home)?;
    let id = server.admit_vault_entry(
        &credentials.token, group,
        &serde_json::json!({"siglum": siglum, "name": name, "prose": prose, "formalization": module}),
    )?;
    writeln!(out, "Admitted {id} to {group}.")?;
    Ok(())
}

/// Compiles and admits one direct unconditional bridge.
pub fn bridge(
    home: &Home,
    group: &str,
    left: &str,
    right: &str,
    kind: BridgeKind,
    lean: &Path,
    out: &mut impl Write,
) -> Result<()> {
    let module =
        std::fs::read_to_string(lean).with_context(|| format!("cannot read {}", lean.display()))?;

    bridge_text(home, group, left, right, kind, &module, out)
}

/// The same, from text somebody typed rather than from a file.
pub fn bridge_text(
    home: &Home,
    group: &str,
    left: &str,
    right: &str,
    kind: BridgeKind,
    lean: &str,
    out: &mut impl Write,
) -> Result<()> {
    let module = checked(home, group, lean)?;
    let (credentials, server) = session::connect(home)?;
    let id = server.admit_bridge(
        &credentials.token,
        group,
        &serde_json::json!({"left": left, "right": right, "kind": kind, "theorem": module}),
    )?;
    writeln!(out, "Admitted {id} to {group}.")?;
    Ok(())
}

/// Records an attestation or withdrawal under the signed-in person's identity.
pub fn audit(
    home: &Home,
    group: &str,
    target_kind: AuditTarget,
    target: &str,
    action: AuditVerb,
    note: &str,
    out: &mut impl Write,
) -> Result<()> {
    let (credentials, server) = session::connect(home)?;
    let id = server.audit_vault(
        &credentials.token, group,
        &serde_json::json!({"target_kind": target_kind, "target": target, "action": action, "note": note}),
    )?;
    writeln!(out, "Recorded {id}.")?;
    Ok(())
}

fn checked(home: &Home, group: &str, module: &str) -> Result<String> {
    let module = module.to_owned();
    let config = Config::read(&home.config_file())?;
    let clone = config
        .group_directory(group, &config.workspace_or(home.workspace()))
        .join(crate::workspace::Side::Shared.directory());
    let manifest = build::read_manifest(&clone)?;
    let pins = build::group_pins(&clone, &manifest);
    let scratch: PathBuf = home.cache_dir().join("lean").join("vault-check.lean");
    crate::lean::check_offered(&clone, &pins, &module, &scratch).map_err(anyhow::Error::msg)?;

    Ok(module)
}

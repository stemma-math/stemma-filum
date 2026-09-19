//! The complete command-line surface for the group vault.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::api::Server;
use crate::build;
use crate::config::Config;
use crate::home::Home;
use crate::session;

/// Lists definitions and bridges, including withdrawn objects as history.
pub fn list(home: &Home, group: &str, out: &mut impl Write) -> Result<()> {
    let credentials = session::required(home)?;
    let vault = Server::at(&credentials.server)?.vault(&credentials.token, group)?;
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
    let module = checked_module(home, group, lean)?;
    let credentials = session::required(home)?;
    let id = Server::at(&credentials.server)?.admit_vault_entry(
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
    kind: &str,
    lean: &Path,
    out: &mut impl Write,
) -> Result<()> {
    let module = checked_module(home, group, lean)?;
    let credentials = session::required(home)?;
    let id = Server::at(&credentials.server)?.admit_bridge(
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
    target_kind: &str,
    target: &str,
    action: &str,
    note: &str,
    out: &mut impl Write,
) -> Result<()> {
    let credentials = session::required(home)?;
    let id = Server::at(&credentials.server)?.audit_vault(
        &credentials.token, group,
        &serde_json::json!({"target_kind": target_kind, "target": target, "action": action, "note": note}),
    )?;
    writeln!(out, "Recorded {id}.")?;
    Ok(())
}

fn checked_module(home: &Home, group: &str, path: &Path) -> Result<String> {
    let module =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
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

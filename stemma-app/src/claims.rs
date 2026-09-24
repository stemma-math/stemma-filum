//! Who writes a line, and reconciling two versions of it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use stemma::api::{About, Claimed, Server};
use stemma_core::line::Siglum;
use stemma_core::reconcile::{Keep, Mark, Part};

use crate::workspace::LineAt;
use crate::{Session, said};

#[derive(Serialize)]
pub struct Claim {
    standing: String,
    label: Option<String>,
    left: Option<u64>,
    asked: bool,
    said: String,
}

/// Takes the line for this machine; renews the claim when already held.
#[tauri::command(async)]
pub fn claim(path: String) -> Result<Claim, String> {
    with_claim(&path, Server::take_claim)
}

/// Records what was written, then releases the claim. The release goes
/// ahead even if recording fails: a claim that cannot be released is worse
/// than one released with work still uncommitted.
#[tauri::command(async)]
pub fn let_go(path: String) -> Result<Claim, String> {
    if let Ok(line) = LineAt::admit(&path) {
        let _ = stemma::writing::record(&line.path);
    }
    with_claim(&path, Server::release_claim)
}

/// Asks the machine holding the line to hand it back.
#[tauri::command(async)]
pub fn ask_for_line(path: String) -> Result<Claim, String> {
    with_claim(&path, Server::ask_for_claim)
}

fn with_claim(
    path: &str,
    speak: impl Fn(&Server, &str, &About) -> anyhow::Result<Claimed>,
) -> Result<Claim, String> {
    let line = LineAt::admit(path)?;
    let session = Session::open()?;
    let about = About {
        group: line.group,
        side: line.side,
        line: line.name,
    };
    let claimed = speak(&session.server, &session.token, &about).map_err(said)?;

    Ok(Claim {
        said: claimed
            .said
            .unwrap_or_else(|| "nobody is writing this line".to_owned()),
        standing: claimed.standing,
        label: claimed.label,
        left: claimed.left,
        asked: claimed.asked,
    })
}

/// One statement or proof on which the two versions differ.
#[derive(Serialize)]
pub struct Disagreement {
    siglum: String,
    part: Part,
    mark: String,
    here: Option<String>,
    there: Option<String>,
    said: String,
}

/// Which version to keep for one disagreement.
#[derive(Deserialize)]
pub struct Decision {
    siglum: Siglum,
    part: Part,
    keep: Keep,
}

/// What the local and remote versions of this line disagree about.
#[tauri::command(async)]
pub fn parted(path: String) -> Result<Vec<Disagreement>, String> {
    let line = LineAt::admit(&path)?;
    let clone = line.clone_root();
    let _ = stemma::git::fetch(clone);

    let found = stemma::conflicts::between(clone).map_err(said)?;
    let Some(parted) = found.into_iter().find(|one| one.line.as_str() == line.name) else {
        return Ok(Vec::new());
    };

    Ok(parted
        .divergences
        .into_iter()
        .map(|divergence| Disagreement {
            siglum: divergence.mark.siglum.to_string(),
            part: divergence.mark.part,
            mark: divergence.mark.to_string(),
            said: divergence.said().to_owned(),
            here: divergence.here,
            there: divergence.there,
        })
        .collect())
}

/// Writes the decisions as one merge commit.
#[tauri::command(async)]
pub fn settle(path: String, decisions: Vec<Decision>) -> Result<Vec<String>, String> {
    let line = LineAt::admit(&path)?;
    let decided: BTreeMap<Mark, Keep> = decisions
        .into_iter()
        .map(|decision| {
            let mark = Mark {
                siglum: decision.siglum,
                part: decision.part,
            };
            (mark, decision.keep)
        })
        .collect();

    let settled = stemma::conflicts::settle(
        line.clone_root(),
        &BTreeMap::from([(line.name.clone(), decided)]),
    )
    .map_err(said)?;

    Ok(settled.iter().map(ToString::to_string).collect())
}

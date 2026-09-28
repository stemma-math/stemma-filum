//! Who writes a line, and deciding between two versions of its blocks.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use stemma::api::{About, Claimed, Server};
use stemma_core::id::BlockId;
use stemma_core::reconcile::Keep;

use stemma::mirror::Mirror;

use crate::engine::{self, Nudge};
use crate::workspace::LineAt;
use crate::{Session, failed, home, said};

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

/// Sends what was written, then releases the claim. The release goes
/// ahead even if sending fails: a claim that cannot be released is worse
/// than one released with writing that leaves on the next round.
#[tauri::command(async)]
pub fn release(path: String) -> Result<Claim, String> {
    if let Ok(line) = LineAt::admit(&path) {
        let about = About {
            group: line.group,
            side: line.side,
            line: line.name,
        };
        let _ = stemma::claims::send_before_release(&home(), &about);
    }
    with_claim(&path, Server::release_claim)
}

/// Who is writing the line, without taking or renewing anything: for
/// looking again when the instance says the claim changed.
#[tauri::command(async)]
pub fn claim_status(path: String) -> Result<Claim, String> {
    let line = LineAt::admit(&path)?;
    let session = Session::open()?;
    let found = session
        .server
        .claims(&session.token)
        .map_err(said)?
        .into_iter()
        .find(|held| held.group == line.group && held.side == line.side && held.line == line.name);

    Ok(match found {
        None => Claim {
            standing: "open".to_owned(),
            label: None,
            left: None,
            asked: false,
            said: "nobody is writing this line".to_owned(),
        },
        Some(held) => Claim {
            standing: if held.mine { "writing" } else { "waiting" }.to_owned(),
            said: match (held.mine, held.asked) {
                (true, _) => "this machine is writing it".to_owned(),
                (false, false) => format!("{} is writing it", held.label),
                (false, true) => format!("{} is writing it, and has been asked for it", held.label),
            },
            label: Some(held.label),
            left: Some(held.left),
            asked: held.asked,
        },
    })
}

/// Asks the machine holding the claim to release it.
#[tauri::command(async)]
pub fn request_claim(path: String) -> Result<Claim, String> {
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

/// One block on which the two versions conflict.
#[derive(Serialize)]
pub struct Conflict {
    block: BlockId,
    label: String,
    here: Option<String>,
    there: Option<String>,
    said: String,
}

/// Which version to keep for one conflict.
#[derive(Deserialize)]
pub struct Decision {
    block: BlockId,
    keep: Keep,
}

/// The blocks this machine and the instance both changed in this line,
/// when it needs a decision.
#[tauri::command(async)]
pub fn parted(path: String) -> Result<Vec<Conflict>, String> {
    let line = LineAt::admit(&path)?;
    let conflicts = Mirror::at(&home())
        .and_then(|mirror| mirror.conflicts(&line.group, line.side()?, &line.name))
        .map_err(failed)?;

    Ok(conflicts
        .into_iter()
        .map(|conflict| Conflict {
            said: conflict.said().to_owned(),
            block: conflict.block,
            label: conflict.label,
            here: conflict.here,
            there: conflict.there,
        })
        .collect())
}

/// Settles the line with the version chosen for each block; the engine
/// sends it at once.
#[tauri::command(async)]
pub fn settle(path: String, decisions: Vec<Decision>) -> Result<Vec<String>, String> {
    let line = LineAt::admit(&path)?;
    let decided: BTreeMap<BlockId, Keep> = decisions
        .into_iter()
        .map(|decision| (decision.block, decision.keep))
        .collect();

    Mirror::at(&home())
        .and_then(|mirror| mirror.decide(&line.group, line.side()?, &line.name, &decided))
        .map_err(failed)?;
    engine::nudge(Nudge::Now);

    Ok(vec![line.name])
}

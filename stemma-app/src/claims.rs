//! Who writes a line, and deciding between two versions of its blocks.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use stemma::api::{About, Claimed, Server, Writing};
use stemma_core::id::BlockId;
use stemma_core::reconcile::Keep;

use stemma::mirror::Mirror;

use crate::engine::{self, Nudge};
use crate::workspace::LineAt;
use crate::{Session, failed, home};

/// Who writes a line, as the window shows it.
#[derive(Serialize)]
pub struct Claim {
    /// `writing`, `waiting` or `open`.
    standing: String,
    /// What the machine holding it calls itself, when one does.
    label: Option<String>,
    /// How long the claim has left, in seconds.
    left: Option<u64>,
    /// Whether it has been asked for.
    asked: bool,
    /// The whole of it, in words.
    said: String,
}

impl Claim {
    /// A claim as the instance described it. When the instance gave no
    /// words for it, they are said from where it stands.
    fn new(
        standing: String,
        label: Option<String>,
        left: Option<u64>,
        asked: bool,
        said: Option<String>,
    ) -> Self {
        let said = said.unwrap_or_else(|| words(&standing, label.as_deref(), asked));
        Self {
            standing,
            label,
            left,
            asked,
            said,
        }
    }
}

impl From<Claimed> for Claim {
    fn from(claimed: Claimed) -> Self {
        Self::new(
            claimed.standing,
            claimed.label,
            claimed.left,
            claimed.asked,
            claimed.said,
        )
    }
}

impl From<Option<Writing>> for Claim {
    fn from(held: Option<Writing>) -> Self {
        match held {
            None => Self::new("open".to_owned(), None, None, false, None),
            Some(held) => Self::new(
                if held.mine { "writing" } else { "waiting" }.to_owned(),
                Some(held.label),
                Some(held.left),
                held.asked,
                None,
            ),
        }
    }
}

/// Where a claim stands, in words.
fn words(standing: &str, label: Option<&str>, asked: bool) -> String {
    let holder = label.unwrap_or("another machine");
    match standing {
        "writing" => "this machine is writing it".to_owned(),
        "waiting" if asked => format!("{holder} is writing it, and has been asked for it"),
        "waiting" => format!("{holder} is writing it"),
        _ => "nobody is writing this line".to_owned(),
    }
}

/// Takes the line for this machine; renews the claim when already held.
#[tauri::command]
pub async fn claim(path: String) -> Result<Claim, String> {
    crate::blocking(move || -> Result<Claim, String> { with_claim(&path, Server::take_claim) })
        .await
}

/// Extends an existing claim without taking an unclaimed line.
#[tauri::command]
pub async fn renew_claim(path: String) -> Result<Claim, String> {
    crate::blocking(move || -> Result<Claim, String> { with_claim(&path, Server::renew_claim) })
        .await
}

/// Sends what was written, then releases the claim. The release goes
/// ahead even if sending fails: a claim that cannot be released is worse
/// than one released with writing that leaves on the next round.
#[tauri::command]
pub async fn release(path: String) -> Result<Claim, String> {
    crate::blocking(move || -> Result<Claim, String> {
        if let Ok(line) = LineAt::admit(&path) {
            let _ = stemma::claims::send_before_release(&home(), &line.about());
        }
        with_claim(&path, Server::release_claim)
    })
    .await
}

/// Who is writing the line, without taking or renewing anything: for
/// looking again when the instance says the claim changed.
#[tauri::command]
pub async fn claim_status(path: String) -> Result<Claim, String> {
    crate::blocking(move || -> Result<Claim, String> {
        let line = LineAt::admit(&path)?;
        let session = Session::open()?;
        let held = session
            .server
            .claims(&session.token)
            .map_err(failed)?
            .into_iter()
            .find(|held| {
                held.group == line.group && held.side == line.side && held.line == line.name
            });

        Ok(Claim::from(held))
    })
    .await
}

/// Asks the machine holding the claim to release it.
#[tauri::command]
pub async fn request_claim(path: String) -> Result<Claim, String> {
    crate::blocking(move || -> Result<Claim, String> { with_claim(&path, Server::ask_for_claim) })
        .await
}

fn with_claim(
    path: &str,
    speak: impl Fn(&Server, &str, &About) -> anyhow::Result<Claimed>,
) -> Result<Claim, String> {
    let line = LineAt::admit(path)?;
    let session = Session::open()?;
    let claimed = speak(&session.server, &session.token, &line.about()).map_err(failed)?;

    Ok(Claim::from(claimed))
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
#[tauri::command]
pub async fn parted(path: String) -> Result<Vec<Conflict>, String> {
    crate::blocking(move || -> Result<Vec<Conflict>, String> {
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
    })
    .await
}

/// Settles the line with the version chosen for each block; the engine
/// sends it at once.
#[tauri::command]
pub async fn settle(path: String, decisions: Vec<Decision>) -> Result<Vec<String>, String> {
    crate::blocking(move || -> Result<Vec<String>, String> {
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
    })
    .await
}

#[cfg(test)]
mod tests {
    use stemma::api::Writing;

    use super::Claim;

    fn held(mine: bool, asked: bool) -> Option<Writing> {
        Some(Writing {
            group: "algebra".to_owned(),
            side: "shared".to_owned(),
            line: "quotients".to_owned(),
            label: "Office iMac".to_owned(),
            mine,
            left: 240,
            asked,
        })
    }

    #[test]
    fn a_claim_says_who_is_writing() {
        let said = |held| Claim::from(held).said;

        assert_eq!(said(None), "nobody is writing this line");
        assert_eq!(said(held(true, false)), "this machine is writing it");
        assert_eq!(said(held(false, false)), "Office iMac is writing it");
        assert_eq!(
            said(held(false, true)),
            "Office iMac is writing it, and has been asked for it"
        );
    }

    #[test]
    fn a_claim_this_machine_holds_is_one_it_writes() {
        let claim = Claim::from(held(true, false));

        assert_eq!(claim.standing, "writing");
        assert_eq!(claim.left, Some(240));
    }
}

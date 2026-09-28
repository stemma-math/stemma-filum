//! Lines changed both here and elsewhere, and the person who wrote both.
//!
//! The engine merges everything that needs nobody: a block changed on one
//! side only is taken from that side. What is left is a block both sides
//! changed, differently, and only a person can say which to keep. The
//! version not kept stays in the line's history on the instance.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use stemma_core::id::BlockId;
use stemma_core::reconcile::Keep;

use crate::home::Home;
use crate::mirror::{self, Mirror, Standing, State};

/// Lists the blocks that need a decision, in the line the path is inside
/// or in every line.
pub fn list(home: &Home, at: &Path, out: &mut impl Write) -> Result<()> {
    let mirror = Mirror::at(home)?;
    let deciding = deciding(&mirror, at)?;

    if deciding.is_empty() {
        writeln!(out, "No line here needs a decision.")?;
        return Ok(());
    }

    for line in &deciding {
        writeln!(out, "{}/{}/{}", line.group, line.side, line.name)?;

        for conflict in mirror.conflicts(&line.group, line.side, &line.name)? {
            writeln!(out, "  {}  {}", conflict.label, conflict.said())?;
            writeln!(out, "    here   {}", shown(conflict.here.as_deref()))?;
            writeln!(out, "    there  {}", shown(conflict.there.as_deref()))?;
        }
    }

    writeln!(out)?;
    writeln!(
        out,
        "Keep one side wherever they disagree with `stemma conflict resolve --keep here|there`."
    )?;

    Ok(())
}

/// Settles the lines that need a decision, keeping one side wherever
/// they disagree. They are sent on the next round.
pub fn resolve(home: &Home, at: &Path, keep: Keep, out: &mut impl Write) -> Result<()> {
    let mirror = Mirror::at(home)?;
    let deciding = deciding(&mirror, at)?;

    if deciding.is_empty() {
        writeln!(out, "No line here needs a decision.")?;
        return Ok(());
    }

    for line in &deciding {
        let decisions: BTreeMap<BlockId, Keep> = mirror
            .conflicts(&line.group, line.side, &line.name)?
            .into_iter()
            .map(|conflict| (conflict.block, keep))
            .collect();

        mirror.decide(&line.group, line.side, &line.name, &decisions)?;
        writeln!(out, "{}/{}/{}  settled", line.group, line.side, line.name)?;
    }

    writeln!(out, "Run `stemma sync` to send them now.")?;

    Ok(())
}

/// The lines needing a decision: the one the path is inside, when it is
/// inside one, or all of them.
fn deciding(mirror: &Mirror, at: &Path) -> Result<Vec<Standing>> {
    let only = mirror::line_at(at, None).ok();

    Ok(mirror
        .standing()?
        .into_iter()
        .filter(|line| line.state == State::NeedsDecision)
        .filter(|line| {
            only.as_ref().is_none_or(|(group, side, name)| {
                &line.group == group && &line.side == side && line.name == name.as_str()
            })
        })
        .collect())
}

/// A version of a block, on one line of a terminal.
fn shown(version: Option<&str>) -> String {
    match version {
        None => "(deleted)".to_owned(),
        Some(text) => {
            let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
            match flat.chars().count() > 72 {
                true => format!("{}…", flat.chars().take(71).collect::<String>()),
                false => flat,
            }
        }
    }
}

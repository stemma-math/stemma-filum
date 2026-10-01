//! The file-backed API exposed to an agent inside one job workspace.
//!
//! The daemon creates the state file and grants. The `stemma-job` command
//! only appends actions that validate against both, so the daemon never has
//! to interpret free-form agent output or read arbitrary workspace changes.
//!
//! An agent may run several `stemma-job` commands at once, so every one
//! that changes the state holds a lock beside it from reading it to
//! writing it back: two answers given together are both kept.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use stemma_core::document::Block;
use stemma_core::id::BlockId;
use stemma_core::proposal::{Grants, Operation, apply_line_operations, safe_library_path};

/// The environment variable through which the private job API is located.
pub const STATE_VARIABLE: &str = "STEMMA_JOB_STATE";

/// Persistent state shared by the daemon and the command in the workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    /// Stable line name used in Lean marks.
    pub line: String,
    /// The source at the job's base revision.
    pub base_source: String,
    /// The operations accepted so far.
    #[serde(default)]
    pub operations: Vec<Operation>,
    /// What this job may do.
    pub grants: Grants,
    /// The explanation shown to the reviewer.
    #[serde(default)]
    pub explanation: String,
    /// A reason the task found nothing.
    #[serde(default)]
    pub nothing: Option<String>,
    /// Whether the agent submitted the answer.
    #[serde(default)]
    pub done: bool,
    /// Root of the workspace, used only by `check`.
    pub workspace: PathBuf,
    /// Original library contents captured lazily before the first write.
    #[serde(default)]
    pub library_before: BTreeMap<String, Option<String>>,
    /// Plain-language readings attached to marked declarations.
    #[serde(default)]
    pub readings: BTreeMap<BlockId, String>,
}

impl State {
    /// The proposed line after all accepted operations.
    pub fn source(&self) -> Result<String> {
        apply_line_operations(&self.base_source, &self.grants, &self.operations)
            .map_err(anyhow::Error::msg)
    }
}

/// The command available only inside an agent workspace.
#[derive(Debug, Parser)]
#[command(
    name = "stemma-job",
    about = "Answer the current Stemma job through its scoped API."
)]
pub struct JobApi {
    #[command(subcommand)]
    command: JobCommand,
}

#[derive(Debug, Subcommand)]
enum JobCommand {
    /// Replace one granted block with a JSON block read from a file.
    Edit {
        block: BlockId,
        #[arg(value_name = "JSON_FILE")]
        from: PathBuf,
    },
    /// Add one JSON block, or an array of blocks, at a granted position.
    Add {
        #[arg(long)]
        after: Option<BlockId>,
        #[arg(value_name = "JSON_FILE")]
        from: PathBuf,
    },
    /// Remove one granted non-central block.
    Remove { block: BlockId },
    /// Propose the contents of one granted Lean library file.
    Write {
        path: String,
        #[arg(value_name = "FILE")]
        from: PathBuf,
    },
    /// Propose removing one granted Lean library file.
    RemoveFile { path: String },
    /// Record the reading of a declaration marked for one line block.
    Mark { block: BlockId, reading: String },
    /// Validate the proposed line and build the staged Lean package.
    Check,
    /// Record the summary a reviewer should see.
    Explain { summary: String },
    /// Record that the task found nothing, with a useful reason.
    Nothing { reason: String },
    /// Submit the operations, explanation, or nothing result.
    Done,
    /// Show the grants and the answer accumulated so far.
    Status,
}

/// Runs `stemma-job` from the current process image.
pub fn run_from_environment() -> Result<()> {
    let api = JobApi::parse();
    let path = std::env::var_os(STATE_VARIABLE)
        .map(PathBuf::from)
        .context("this command is only available inside a Stemma job workspace")?;
    let mut changed = !matches!(api.command, JobCommand::Check | JobCommand::Status);
    let _locked = match changed {
        true => Some(lock(&path)?),
        false => None,
    };
    let mut state = read(&path)?;

    match api.command {
        JobCommand::Edit { block, from } => {
            let with = block_from(&from)?;
            append(&mut state, Operation::EditBlock { block, with })?;
            println!("accepted edit of {block}");
        }
        JobCommand::Add { after, from } => {
            let blocks = blocks_from(&from)?;
            append(&mut state, Operation::AddBlocks { after, blocks })?;
            println!("accepted added blocks");
        }
        JobCommand::Remove { block } => {
            append(&mut state, Operation::RemoveBlock { block })?;
            println!("accepted removal of {block}");
        }
        JobCommand::Write { path: file, from } => {
            let after = std::fs::read_to_string(&from)
                .with_context(|| format!("cannot read {}", from.display()))?;
            library(&mut state, file, Some(after))?;
            println!("accepted library write");
        }
        JobCommand::RemoveFile { path: file } => {
            library(&mut state, file, None)?;
            println!("accepted library removal");
        }
        JobCommand::Mark { block, reading } => {
            if !state.grants.library {
                bail!("this job may not write Lean marks");
            }
            if reading.trim().is_empty() {
                bail!("a mark needs a plain-language reading");
            }
            let marker = format!("@[stemma \"{}/{}\"]", state.line, block);
            let present = state.operations.iter().any(|operation| {
                matches!(operation, Operation::WriteLibraryFile { after: Some(source), .. } if source.contains(&marker))
            });
            if !present {
                bail!("write a library file containing `{marker}` before recording its reading");
            }
            state.readings.insert(block, reading.trim().to_owned());
            println!("recorded reading for {block}");
        }
        JobCommand::Check => {
            check(&state)?;
            println!("the proposed line and library passed their checks");
            changed = false;
        }
        JobCommand::Explain { summary } => {
            if summary.trim().is_empty() {
                bail!("an explanation cannot be empty");
            }
            state.explanation = summary.trim().to_owned();
            state.nothing = None;
            println!("recorded explanation");
        }
        JobCommand::Nothing { reason } => {
            if reason.trim().is_empty() {
                bail!("a nothing result needs a reason");
            }
            if !state.operations.is_empty() {
                bail!("a job with proposed operations cannot also report nothing");
            }
            state.nothing = Some(reason.trim().to_owned());
            println!("recorded nothing result");
        }
        JobCommand::Done => {
            if state.operations.is_empty() && state.nothing.is_none() {
                bail!("there is no answer to submit");
            }
            if !state.operations.is_empty() && state.explanation.trim().is_empty() {
                bail!("explain the proposed operations before submitting them");
            }
            check(&state)?;
            state.done = true;
            println!("submitted job answer");
        }
        JobCommand::Status => {
            serde_json::to_writer_pretty(std::io::stdout().lock(), &state)?;
            println!();
            changed = false;
        }
    }

    if changed {
        write(&path, &state)?;
    }
    Ok(())
}

/// Reads a job API state written by the daemon or a completed command.
pub fn read(path: &Path) -> Result<State> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read the job state at {}", path.display()))?;
    serde_json::from_str(&text).context("the job state is invalid")
}

/// Writes a state atomically, so an interrupted command leaves the last
/// complete answer for the daemon to inspect.
pub fn write(path: &Path, state: &State) -> Result<()> {
    let mut text = serde_json::to_string_pretty(state).context("cannot write the job state")?;
    text.push('\n');

    crate::storage::replace(path, &text)
}

/// Waits for, and takes, the lock on a job's state, held for as long as
/// what this returns is.
fn lock(state: &Path) -> Result<File> {
    let path = state.with_extension("lock");
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    file.lock()
        .with_context(|| format!("cannot lock {}", path.display()))?;

    Ok(file)
}

fn block_from(path: &Path) -> Result<Block> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).context("the file does not contain one valid block")
}

fn blocks_from(path: &Path) -> Result<Vec<Block>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    if let Ok(blocks) = serde_json::from_str::<Vec<Block>>(&text) {
        return Ok(blocks);
    }
    Ok(vec![serde_json::from_str(&text).context(
        "the file contains neither a block nor an array of blocks",
    )?])
}

fn append(state: &mut State, operation: Operation) -> Result<()> {
    if state.done {
        bail!("this answer has already been submitted");
    }
    let mut proposed = state.operations.clone();
    proposed.push(operation.clone());
    apply_line_operations(&state.base_source, &state.grants, &proposed)
        .map_err(anyhow::Error::msg)?;
    state.operations.push(operation);
    state.nothing = None;
    Ok(())
}

fn library(state: &mut State, path: String, after: Option<String>) -> Result<()> {
    if !safe_library_path(&path) {
        bail!("`{path}` is not a safe library path");
    }
    if state.done {
        bail!("this answer has already been submitted");
    }
    let absolute = state.workspace.join(&path);
    let before = state
        .library_before
        .entry(path.clone())
        .or_insert_with(|| std::fs::read_to_string(&absolute).ok())
        .clone();
    let operation = Operation::WriteLibraryFile {
        path: path.clone(),
        before,
        after: after.clone(),
    };
    if !state.grants.admits(&operation) {
        bail!("that action is outside this job's grants");
    }
    if let Some(parent) = absolute.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let present = after.is_some();
    match after {
        Some(contents) => std::fs::write(&absolute, contents)?,
        None if absolute.exists() => std::fs::remove_file(&absolute)?,
        None => {}
    }
    if path.ends_with(".lean") {
        crate::lean::update_root_import(&state.workspace, &path, present)?;
    }
    state.operations.retain(
        |existing| !matches!(existing, Operation::WriteLibraryFile { path: old, .. } if old == &path),
    );
    state.operations.push(operation);
    state.nothing = None;
    Ok(())
}

fn check(state: &State) -> Result<()> {
    state.source()?;
    if state
        .operations
        .iter()
        .any(|operation| matches!(operation, Operation::WriteLibraryFile { .. }))
    {
        for block in state.readings.keys() {
            let marker = format!("@[stemma \"{}/{}\"]", state.line, block);
            if !state.operations.iter().any(|operation| {
                matches!(operation, Operation::WriteLibraryFile { after: Some(source), .. } if source.contains(&marker))
            }) {
                bail!("no proposed library file contains `{marker}`");
            }
        }
        let status = crate::lean::tool("lake")
            .arg("build")
            .current_dir(&state.workspace)
            .status()
            .context("cannot run `lake build`")?;
        if !status.success() {
            bail!("`lake build` rejected the proposed library");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use stemma_core::document::Document;
    use stemma_core::proposal::LineGrant;
    use stemma_core::repository::{Manifest, Side};

    #[test]
    fn writing_a_module_updates_the_staged_root_without_granting_root_edits() {
        let root = std::env::temp_dir().join(format!("stemma-answer-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let manifest = Manifest::new("grp:algebra".parse().unwrap(), Side::Shared, "Algebra");
        std::fs::write(
            root.join(stemma_core::repository::MANIFEST),
            manifest.to_text(),
        )
        .unwrap();
        std::fs::write(root.join("GroupLibrary.lean"), "import Stemma\n").unwrap();
        let mut state = State {
            line: "rings".to_owned(),
            base_source: Document::new("Rings", None).to_text(),
            operations: Vec::new(),
            grants: Grants {
                library: true,
                ..Grants::default()
            },
            explanation: String::new(),
            nothing: None,
            done: false,
            workspace: root.clone(),
            library_before: BTreeMap::new(),
            readings: BTreeMap::new(),
        };

        library(
            &mut state,
            "GroupLibrary/Rings.lean".to_owned(),
            Some("def ring := 1\n".to_owned()),
        )
        .unwrap();
        let imports = std::fs::read_to_string(root.join("GroupLibrary.lean")).unwrap();
        assert!(imports.contains("import GroupLibrary.Rings\n"));
        assert!(
            library(
                &mut state,
                "GroupLibrary.lean".to_owned(),
                Some("-- bad".to_owned())
            )
            .is_err()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_edit_is_rejected_before_it_enters_state_when_it_is_not_granted() {
        let mut document = Document::new("A line", None);
        let block = document.push(Block::prose("Before"));
        let mut state = State {
            line: "a-line".to_owned(),
            base_source: document.to_text(),
            operations: Vec::new(),
            grants: Grants::default(),
            explanation: String::new(),
            nothing: None,
            done: false,
            workspace: PathBuf::from("/tmp/nowhere"),
            library_before: BTreeMap::new(),
            readings: BTreeMap::new(),
        };
        let mut changed = document.blocks[0].clone();
        if let Block::Prose(prose) = &mut changed {
            prose.content = "After".to_owned();
        }
        assert!(
            append(
                &mut state,
                Operation::EditBlock {
                    block,
                    with: changed
                }
            )
            .is_err()
        );
        assert!(state.operations.is_empty());

        state.grants.line.push(LineGrant::Edit { block });
        let mut changed = document.blocks[0].clone();
        if let Block::Prose(prose) = &mut changed {
            prose.content = "After".to_owned();
        }
        append(
            &mut state,
            Operation::EditBlock {
                block,
                with: changed,
            },
        )
        .unwrap();
        assert_eq!(state.operations.len(), 1);
    }
}

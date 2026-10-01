//! Checking a line against the group's pinned Lean environment.
//!
//! The server never runs Lean. A person's machine checks the module beside
//! the Typst source and publishes only this reconstructible digest. Compiler
//! output remains local, just like an agent's raw output.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;
use stemma_core::artefact::{BlockCheck, FormalDeclaration, LeanDigest};
use stemma_core::id::BlockId;
use stemma_core::line::{Name, Siglum};
use stemma_core::model::Verification;
use stemma_core::proposal::{FormalReview, LibraryChange};
use stemma_core::repository::{self, Pins};

const MARKER: &str = "-- stemma: verifies ";

/// Checks a line when its repository has a Lean library.
pub fn check(clone: &Path, line: &Name, pins: &Pins) -> Result<Option<LeanDigest>> {
    Ok(check_library(clone, pins)?.digest(clone, line))
}

/// What checking a repository's Lean library came to, ready to be read
/// for any of its lines.
///
/// Building the library and extracting its marks is the same work for
/// every line in the repository, so it is done once and each line reads
/// its own blocks out of the result.
#[derive(Debug)]
pub struct Library {
    pins: Pins,
    state: LibraryState,
}

#[derive(Debug)]
enum LibraryState {
    /// The repository has no Lean library.
    Absent,
    /// The library could not be checked, and why.
    Refused(String),
    /// It was built and its marks extracted.
    Extracted {
        lean: String,
        mathlib: String,
        report: ExtractorReport,
    },
}

/// Checks a line like [`check`], reusing this process's last check of the
/// library while nothing it was built from has changed.
///
/// An editor checks a line every time its writer pauses, and building and
/// extracting a library against Mathlib takes far longer than a pause. A
/// line's own `line.json` is read afresh every time, so what changes there
/// is always seen; a check that was refused is never reused, so installing
/// what was missing is seen at the next pause.
pub fn check_reusing(clone: &Path, line: &Name, pins: &Pins) -> Result<Option<LeanDigest>> {
    use std::collections::HashMap;
    use std::sync::{Arc, LazyLock, Mutex};

    /// A library check, by repository, with what it was made from.
    type Checked = HashMap<PathBuf, (String, Arc<Library>)>;
    static CHECKED: LazyLock<Mutex<Checked>> = LazyLock::new(Mutex::default);

    // Lines of one repository are often opened one after another, and
    // each asks for the same check. The later ones wait for the first and
    // read what it found, instead of each starting Lake over again.
    static CHECKING: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
        LazyLock::new(Mutex::default);
    let turn = CHECKING
        .lock()
        .map(|mut checking| Arc::clone(checking.entry(clone.to_owned()).or_default()))
        .unwrap_or_default();
    let _turn = turn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let fingerprint = library_fingerprint(clone, pins)?;
    let reused = CHECKED
        .lock()
        .ok()
        .and_then(|checked| checked.get(clone).cloned())
        .filter(|(built_from, _)| *built_from == fingerprint)
        .map(|(_, library)| library);
    let library = match reused {
        Some(library) => library,
        None => {
            let library = Arc::new(check_library(clone, pins)?);
            if let Ok(mut checked) = CHECKED.lock() {
                match library.state {
                    LibraryState::Extracted { .. } => {
                        checked.insert(clone.to_owned(), (fingerprint, Arc::clone(&library)));
                    }
                    _ => {
                        checked.remove(clone);
                    }
                }
            }
            library
        }
    };

    Ok(library.digest(clone, line))
}

/// What a library check was made from: the pins, and the size and time of
/// every Lean source and Lake file, as a digest.
fn library_fingerprint(clone: &Path, pins: &Pins) -> Result<String> {
    use sha2::{Digest, Sha256};

    let mut files = library_sources(clone)?;
    files.extend(
        [
            "lean-toolchain",
            repository::LAKEFILE,
            "lakefile.lean",
            "lake-manifest.json",
        ]
        .map(PathBuf::from),
    );
    files.sort();

    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(pins)?);
    for file in files {
        digest.update(file.to_string_lossy().as_bytes());
        if let Ok(about) = std::fs::metadata(clone.join(&file)) {
            digest.update(about.len().to_le_bytes());
            let modified = about
                .modified()
                .ok()
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .unwrap_or_default();
            digest.update(modified.as_nanos().to_le_bytes());
        }
    }

    Ok(hex(&digest.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Prepares, builds and reads the Lean library of a repository against
/// the group's pinned environment.
pub fn check_library(clone: &Path, pins: &Pins) -> Result<Library> {
    let state = library_state(clone, pins)?;

    Ok(Library {
        pins: pins.clone(),
        state,
    })
}

fn library_state(clone: &Path, pins: &Pins) -> Result<LibraryState> {
    let refused = |reason: &str| Ok(LibraryState::Refused(reason.to_owned()));

    if !has_lean_sources(clone) {
        return Ok(LibraryState::Absent);
    }

    let Some(lean) = pins.lean.as_deref() else {
        return refused("Lean is not pinned for this group");
    };
    let Some(mathlib) = pins.mathlib.as_deref() else {
        return refused("Mathlib is not pinned for this group");
    };

    if elan().is_none() {
        return refused(
            "Lean checking needs elan. Install it from https://lean-lang.org/lean4/doc/quickstart.html, then sync again",
        );
    }
    if let Err(reason) = prepare_environment(clone, lean, mathlib) {
        return refused(&reason);
    }

    let installed = installed_version(clone);
    if installed.as_deref() != Some(lean) {
        return refused(&match installed {
            Some(found) => format!("Lean {lean} is pinned but Lean {found} is installed"),
            None => format!("Lean {lean} is pinned but is not available through Lake"),
        });
    }

    if let Err(reason) = check_mathlib(clone, mathlib) {
        return refused(&reason);
    }
    let modules = modules_in(clone)?;
    if modules.is_empty() {
        return Ok(LibraryState::Absent);
    }
    let output = tool("lake")
        .current_dir(clone)
        .arg("build")
        .output()
        .with_context(|| format!("cannot build the Lean library in {}", clone.display()))?;
    if !output.status.success() {
        return refused(&concise(&String::from_utf8_lossy(&output.stderr)));
    }

    Ok(LibraryState::Extracted {
        lean: lean.to_owned(),
        mathlib: mathlib.to_owned(),
        report: extract(clone, &modules)?,
    })
}

impl Library {
    /// What the library establishes for one line, when there is a
    /// library at all.
    pub fn digest(&self, clone: &Path, line: &Name) -> Option<LeanDigest> {
        let (lean, mathlib, report) = match &self.state {
            LibraryState::Absent => return None,
            LibraryState::Refused(reason) => return Some(refused(clone, line, reason, &self.pins)),
            LibraryState::Extracted {
                lean,
                mathlib,
                report,
            } => (lean, mathlib, report),
        };

        let prefix = format!("{}/", line.as_str());
        let mut blocks: Vec<BlockCheck> = report
            .marks
            .iter()
            .filter_map(|mark| {
                let block = mark.block.strip_prefix(&prefix)?.parse::<BlockId>().ok()?;
                Some(classify(block, mark.declarations.clone()))
            })
            .collect();

        for block in statements_of(clone, line) {
            if !blocks.iter().any(|result| result.block == block) {
                blocks.push(BlockCheck {
                    block,
                    state: Verification::Unverified,
                    diagnostic: "no Lean declaration is marked for this block".to_owned(),
                    declarations: Vec::new(),
                });
            }
        }
        blocks.sort_by_key(|result| result.block);

        Some(LeanDigest {
            lean: lean.clone(),
            mathlib: mathlib.clone(),
            compiled: true,
            blocks,
            diagnostic: broken_for(report, &prefix, &blocks_of(clone, line)),
        })
    }
}

/// A line's document as its repository has it, if it can be read.
fn document_of(clone: &Path, line: &Name) -> Option<stemma_core::document::Document> {
    let source = std::fs::read_to_string(repository::line_entry(clone, line)).ok()?;
    stemma_core::document::Document::parse(&source).ok()
}

/// The statement blocks of a line as its repository has it.
fn statements_of(clone: &Path, line: &Name) -> Vec<BlockId> {
    document_of(clone, line)
        .map(|document| {
            document
                .blocks
                .iter()
                .filter(|block| block.statement().is_some())
                .map(|block| *block.id())
                .collect()
        })
        .unwrap_or_default()
}

/// Every block of a line as its repository has it.
fn blocks_of(clone: &Path, line: &Name) -> Vec<BlockId> {
    document_of(clone, line)
        .map(|document| document.blocks.iter().map(|block| *block.id()).collect())
        .unwrap_or_default()
}

/// Checks an agent's library change in detached worktrees and returns the
/// before/after evidence a reviewer needs. The person's working tree is never
/// touched, and the temporary trees are removed even when Lean rejects one.
pub fn review_change(
    clone: &Path,
    line: &Name,
    pins: &Pins,
    change: &LibraryChange,
    proposed_source: &str,
    scratch: &Path,
    targets: &[BlockId],
) -> Result<FormalReview> {
    let base = scratch.join("base");
    let proposed = scratch.join("proposed");
    std::fs::create_dir_all(scratch)
        .with_context(|| format!("cannot create {}", scratch.display()))?;

    crate::git::detached_worktree(clone, &change.base, &base)?;
    let checked = (|| {
        crate::git::detached_worktree(clone, &change.base, &proposed)?;
        apply_change(&proposed, change)?;
        let line_entry = repository::line_entry(&proposed, line);
        std::fs::write(&line_entry, proposed_source)
            .with_context(|| format!("cannot stage {}", line_entry.display()))?;
        let before = check(&base, line, pins)?.unwrap_or_else(|| empty_digest(pins));
        let after = check(&proposed, line, pins)?.unwrap_or_else(|| empty_digest(pins));
        if !after.compiled {
            anyhow::bail!(after.diagnostic.clone());
        }
        Ok(FormalReview::between(
            &before.blocks,
            &after.blocks,
            targets.iter().copied(),
            &change.readings,
        ))
    })();

    crate::git::remove_worktree(clone, &proposed);
    crate::git::remove_worktree(clone, &base);
    let _ = std::fs::remove_dir_all(scratch);
    checked
}

fn empty_digest(pins: &Pins) -> LeanDigest {
    LeanDigest {
        lean: pins.lean.clone().unwrap_or_default(),
        mathlib: pins.mathlib.clone().unwrap_or_default(),
        compiled: true,
        blocks: Vec::new(),
        diagnostic: String::new(),
    }
}

fn apply_change(worktree: &Path, change: &LibraryChange) -> Result<()> {
    for change in &change.files {
        let relative = safe_library_path(&change.path)?;
        let path = worktree.join(&relative);
        let before = std::fs::read_to_string(&path).ok();
        if before != change.before {
            anyhow::bail!(
                "the proposed change does not match its base for {}",
                change.path
            );
        }
        match &change.after {
            Some(source) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, source)?;
            }
            None if path.exists() => std::fs::remove_file(&path)?,
            None => {}
        }
    }
    update_root_imports(worktree, change)
}

fn safe_library_path(written: &str) -> Result<PathBuf> {
    let path = Path::new(written);
    let reserved = path.starts_with(repository::LINES)
        || matches!(
            written,
            repository::MANIFEST
                | repository::LAKEFILE
                | repository::LEAN_TOOLCHAIN
                | "GroupLibrary.lean"
                | "PrivateLibrary.lean"
        );
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.components().any(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some(".git" | ".lake" | ".stemma" | "lines")
            )
        })
        || reserved
    {
        anyhow::bail!("`{written}` is not an agent-writable Lean library path");
    }
    Ok(path.to_owned())
}

fn update_root_imports(worktree: &Path, change: &LibraryChange) -> Result<()> {
    update_root_imports_from(
        worktree,
        change
            .files
            .iter()
            .map(|file| (file.path.as_str(), file.after.is_some())),
    )
}

/// Keeps a staged module reachable from Lake's root target. The root is
/// generated here because an agent has no grant to edit it.
pub(crate) fn update_root_import(worktree: &Path, path: &str, present: bool) -> Result<()> {
    update_root_imports_from(worktree, std::iter::once((path, present)))
}

fn update_root_imports_from<'a>(
    worktree: &Path,
    files: impl Iterator<Item = (&'a str, bool)>,
) -> Result<()> {
    let manifest = crate::repository::read_manifest(worktree)?;
    let root = match manifest.side {
        repository::Side::Shared => "GroupLibrary.lean",
        repository::Side::Private => "PrivateLibrary.lean",
    };
    let root_path = worktree.join(root);
    let before = std::fs::read_to_string(&root_path).unwrap_or_default();
    let mut imports: BTreeSet<String> = before
        .lines()
        .filter_map(|line| line.trim().strip_prefix("import ").map(str::to_owned))
        .collect();
    imports.insert(match manifest.side {
        repository::Side::Shared => "Stemma".to_owned(),
        repository::Side::Private => "GroupLibrary".to_owned(),
    });
    for (path, present) in files.filter(|(path, _)| path.ends_with(".lean")) {
        let module = path.trim_end_matches(".lean").replace(['/', '\\'], ".");
        if present {
            imports.insert(module);
        } else {
            imports.remove(&module);
        }
    }
    let source = imports
        .into_iter()
        .map(|module| format!("import {module}\n"))
        .collect::<String>();
    std::fs::write(root_path, source)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtractorReport {
    marks: Vec<ExtractedMark>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExtractedMark {
    block: String,
    declarations: Vec<ExtractedDeclaration>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtractedDeclaration {
    name: String,
    module: String,
    statement: String,
    sorry_free: bool,
    axioms: Vec<String>,
    reaches: Vec<String>,
    statement_closure: String,
}

impl From<ExtractedDeclaration> for FormalDeclaration {
    fn from(found: ExtractedDeclaration) -> Self {
        Self {
            name: found.name,
            module: found.module,
            statement: found.statement,
            sorry_free: found.sorry_free,
            axioms: found.axioms,
            reaches: found.reaches,
            statement_closure: found.statement_closure,
        }
    }
}

const ALLOWED_AXIOMS: [&str; 3] = ["propext", "Classical.choice", "Quot.sound"];

fn classify(block: BlockId, declarations: Vec<ExtractedDeclaration>) -> BlockCheck {
    let sound = |declaration: &ExtractedDeclaration| {
        declaration.sorry_free
            && declaration
                .axioms
                .iter()
                .all(|axiom| ALLOWED_AXIOMS.contains(&axiom.as_str()))
    };
    let verified: Vec<_> = declarations
        .iter()
        .filter(|declaration| sound(declaration))
        .collect();
    let refuted = verified
        .iter()
        .any(|declaration| is_negation(&declaration.statement));
    let state = if refuted {
        Verification::Refuted
    } else if !verified.is_empty() {
        Verification::Verified
    } else {
        Verification::Unverified
    };
    let diagnostic = if state != Verification::Unverified {
        String::new()
    } else if declarations
        .iter()
        .any(|declaration| !declaration.sorry_free)
    {
        "the marked declaration depends on `sorry`".to_owned()
    } else {
        let disallowed: BTreeSet<_> = declarations
            .iter()
            .flat_map(|declaration| &declaration.axioms)
            .filter(|axiom| !ALLOWED_AXIOMS.contains(&axiom.as_str()))
            .cloned()
            .collect();
        match disallowed.is_empty() {
            true => "no verified declaration is marked for this block".to_owned(),
            false => format!(
                "the marked declaration uses disallowed axioms: {}",
                disallowed.into_iter().collect::<Vec<_>>().join(", ")
            ),
        }
    };

    BlockCheck {
        block,
        state,
        diagnostic,
        declarations: declarations.into_iter().map(Into::into).collect(),
    }
}

fn is_negation(statement: &str) -> bool {
    let statement = statement.trim_start_matches(|character: char| {
        character.is_whitespace() || matches!(character, '(' | '{' | '[')
    });
    statement.starts_with("Not ") || statement.starts_with('¬')
}

/// Runs the extractor over the library's modules.
///
/// Which marks name a block that does not exist is decided per line, from
/// the line as it is when read ([`broken_for`]), so the extractor is given
/// no blocks and its report depends on the Lean sources alone.
fn extract(clone: &Path, modules: &[String]) -> Result<ExtractorReport> {
    let directory = clone.join(".stemma/check");
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;
    let list = directory.join("blocks.json");
    std::fs::write(&list, b"[]").with_context(|| format!("cannot write {}", list.display()))?;
    let mut command = tool("lake");
    command
        .current_dir(clone)
        .args(["exe", "stemma-extract", "--", "--blocks"])
        .arg(&list)
        .args(modules);
    let output = command
        .output()
        .with_context(|| "cannot run the Stemma Lean extractor")?;
    if !output.status.success() {
        anyhow::bail!(concise(&String::from_utf8_lossy(&output.stderr)));
    }
    serde_json::from_slice(&output.stdout)
        .context("the Stemma Lean extractor returned unreadable JSON")
}

/// The marks naming `prefix`'s line that name none of its `blocks`.
fn broken_for(report: &ExtractorReport, prefix: &str, blocks: &[BlockId]) -> String {
    let broken: Vec<_> = report
        .marks
        .iter()
        .map(|mark| mark.block.as_str())
        .filter(|mark| {
            mark.strip_prefix(prefix).is_some_and(|block| {
                !block
                    .parse::<BlockId>()
                    .is_ok_and(|block| blocks.contains(&block))
            })
        })
        .map(str::to_owned)
        .collect();
    match broken.is_empty() {
        true => String::new(),
        false => format!("unknown block marks: {}", broken.join(", ")),
    }
}

fn has_lean_sources(clone: &Path) -> bool {
    modules_in(clone).is_ok_and(|modules| !modules.is_empty())
}

fn modules_in(clone: &Path) -> Result<Vec<String>> {
    let mut found: Vec<String> = library_sources(clone)?
        .iter()
        .map(|relative| {
            let mut module = relative
                .with_extension("")
                .to_string_lossy()
                .replace(['/', '\\'], ".");
            if module.ends_with(".Main") {
                module.truncate(module.len() - ".Main".len());
            }
            module
        })
        .collect();
    found.sort();
    found.dedup();
    Ok(found)
}

/// The library's Lean files, relative to the repository: every `.lean`
/// file but the Lake configuration, outside the lines, Lake's own
/// directory, Stemma's scratch directory and git's.
fn library_sources(clone: &Path) -> Result<Vec<PathBuf>> {
    fn visit(root: &Path, at: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
        for entry in
            std::fs::read_dir(at).with_context(|| format!("cannot read {}", at.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap_or(&path);
            if [".lake", ".stemma", ".git", repository::LINES]
                .iter()
                .any(|skipped| relative.starts_with(skipped))
            {
                continue;
            }
            if entry.file_type()?.is_dir() {
                visit(root, &path, found)?;
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("lean")
                && path.file_name().and_then(|name| name.to_str()) != Some("lakefile.lean")
            {
                found.push(relative.to_owned());
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    visit(clone, clone, &mut found)?;
    Ok(found)
}

/// Where elan is on this machine, if it is anywhere a program would be.
///
/// The one answer to whether Lean can be used here: the setup checklist and
/// the checking of lines both ask this.
pub fn elan() -> Option<std::path::PathBuf> {
    crate::job::agents::on_path("elan")
}

/// A Lean tool (`lake`, `lean`, `elan`), found and run where elan installs
/// it even when this process was started with a short path, as the
/// application and the background service are.
pub(crate) fn tool(program: &str) -> Command {
    let found = crate::job::agents::on_path(program);
    let mut command = Command::new(found.as_deref().unwrap_or(Path::new(program)));
    if let Ok(path) = std::env::join_paths(crate::job::agents::where_programs_are()) {
        command.env("PATH", path);
    }
    command
}

pub(crate) fn prepare_environment(clone: &Path, lean: &str, mathlib: &str) -> Result<(), String> {
    set_toolchain(clone, lean).map_err(|error| format!("{error:#}"))?;
    let directory = clone.join(".stemma/check");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;

    // The application and the daemon both prepare the same repository,
    // and two `lake update`s at once fetch Mathlib twice into one place.
    // Whoever comes second waits, then finds the work done.
    let lock = directory.join("lock");
    let _locked = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock)
        .and_then(|file| file.lock().map(|()| file))
        .map_err(|error| format!("cannot lock {}: {error}", lock.display()))?;

    // `lake update` resolves the packages again and may move any that are
    // not pinned, so it runs only when what it resolves from changed.
    let resolved_from = resolution_inputs(clone);
    let resolved = directory.join("lake-update");
    let manifest = clone.join("lake-manifest.json");
    if !manifest.exists()
        || std::fs::read_to_string(&resolved).ok().as_deref() != Some(resolved_from.as_str())
    {
        let update = tool("lake")
            .current_dir(clone)
            .arg("update")
            .output()
            .map_err(|error| format!("cannot prepare the pinned Lake packages: {error}"))?;
        if !update.status.success() {
            return Err(concise(&String::from_utf8_lossy(&update.stderr)));
        }
        std::fs::write(&resolved, &resolved_from).map_err(|error| error.to_string())?;
    }

    let stamp = directory.join("mathlib-cache");
    if std::fs::read_to_string(&stamp).ok().as_deref() == Some(mathlib) {
        return Ok(());
    }
    let cache = tool("lake")
        .current_dir(clone)
        .args(["exe", "cache", "get"])
        .output()
        .map_err(|error| format!("cannot fetch Mathlib's build cache: {error}"))?;
    if !cache.status.success() {
        return Err(format!(
            "Mathlib's build cache could not be fetched: {}",
            concise(&String::from_utf8_lossy(&cache.stderr))
        ));
    }
    std::fs::write(stamp, mathlib).map_err(|error| error.to_string())?;
    Ok(())
}

/// Resolves and caches the pinned packages before a sandboxed agent gets a
/// copy of this repository. The agent cannot update the shared Lake cache.
pub(crate) fn prepare_for_job(clone: &Path) -> Result<()> {
    if !clone.join(repository::MANIFEST).exists() || !has_lean_sources(clone) {
        return Ok(());
    }
    let manifest = crate::repository::read_manifest(clone)?;
    if manifest.side == repository::Side::Private {
        let shared = clone
            .parent()
            .context("the private library has no group directory")?
            .join("shared");
        prepare_for_job(&shared)?;
    }
    let pins = crate::repository::group_pins(clone, &manifest);
    let lean = pins
        .lean
        .as_deref()
        .context("Lean is not pinned for this group")?;
    let mathlib = pins
        .mathlib
        .as_deref()
        .context("Mathlib is not pinned for this group")?;
    prepare_environment(clone, lean, mathlib).map_err(anyhow::Error::msg)
}

/// What `lake update` resolves the packages from: the toolchain and the
/// Lake configuration, as a digest.
fn resolution_inputs(clone: &Path) -> String {
    use sha2::{Digest, Sha256};

    let mut digest = Sha256::new();
    for file in ["lean-toolchain", repository::LAKEFILE, "lakefile.lean"] {
        digest.update(file.as_bytes());
        digest.update(std::fs::read(clone.join(file)).unwrap_or_default());
    }
    hex(&digest.finalize())
}

fn set_toolchain(clone: &Path, lean: &str) -> Result<()> {
    let path = clone.join("lean-toolchain");
    let expected = format!("leanprover/lean4:v{lean}\n");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(expected.as_str()) {
        std::fs::write(&path, expected)
            .with_context(|| format!("cannot set {}", path.display()))?;
    }
    Ok(())
}

/// Checks a proposed complete module before it is allowed to travel.
pub fn check_offered(
    clone: &Path,
    pins: &Pins,
    source: &str,
    scratch: &Path,
) -> Result<(), String> {
    let lean = pins
        .lean
        .as_deref()
        .ok_or("Lean is not pinned for this group")?;
    let mathlib = pins
        .mathlib
        .as_deref()
        .ok_or("Mathlib is not pinned for this group")?;
    match installed_version(clone) {
        Some(found) if found == lean => {}
        Some(found) => {
            return Err(format!(
                "Lean {lean} is pinned but Lean {found} is installed"
            ));
        }
        None => {
            return Err(format!(
                "Lean {lean} is pinned but is not available through Lake"
            ));
        }
    }
    check_mathlib(clone, mathlib)?;
    if let Some(parent) = scratch.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(scratch, source).map_err(|error| error.to_string())?;
    let output = tool("lake")
        .current_dir(clone)
        .args(["env", "lean"])
        .arg(scratch)
        .output()
        .map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(scratch);
    if output.status.success() {
        Ok(())
    } else {
        Err(concise(&String::from_utf8_lossy(&output.stderr)))
    }
}

fn refused(clone: &Path, line: &Name, reason: &str, pins: &Pins) -> LeanDigest {
    LeanDigest {
        lean: pins.lean.clone().unwrap_or_default(),
        mathlib: pins.mathlib.clone().unwrap_or_default(),
        compiled: false,
        blocks: unchecked_blocks(clone, line, reason),
        diagnostic: reason.to_owned(),
    }
}

fn unchecked_blocks(clone: &Path, line: &Name, diagnostic: &str) -> Vec<BlockCheck> {
    statements_of(clone, line)
        .into_iter()
        .map(|block| BlockCheck {
            block,
            state: Verification::Unverified,
            diagnostic: diagnostic.to_owned(),
            declarations: Vec::new(),
        })
        .collect()
}

fn installed_version(clone: &Path) -> Option<String> {
    let output = tool("lake")
        .current_dir(clone)
        .args(["env", "lean", "--version"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| version_in(&output.stdout))?
}

fn version_in(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output)
        .split_whitespace()
        .map(|word| word.strip_prefix('v').unwrap_or(word))
        .find(|word| word.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|word| word.trim_end_matches(',').to_owned())
}

fn mathlib_revision(clone: &Path) -> Result<Option<String>> {
    let path = clone.join("lake-manifest.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("cannot read {}", path.display())),
    };
    let manifest: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(manifest["packages"]
        .as_array()
        .and_then(|packages| packages.iter().find(|package| package["name"] == "mathlib"))
        .and_then(|package| package["rev"].as_str())
        .map(str::to_owned))
}

fn check_mathlib(clone: &Path, pinned: &str) -> Result<(), String> {
    match mathlib_revision(clone).map_err(|error| format!("{error:#}"))? {
        Some(found) if found == pinned => Ok(()),
        Some(_) if declares_mathlib_pin(clone, pinned) => Ok(()),
        Some(found) => Err(format!(
            "Mathlib {pinned} is pinned but the Lake manifest has {found}"
        )),
        None => Err(format!(
            "Mathlib {pinned} is pinned but the Lake manifest does not identify its revision"
        )),
    }
}

fn declares_mathlib_pin(clone: &Path, pinned: &str) -> bool {
    let Ok(lakefile) = std::fs::read_to_string(clone.join(repository::LAKEFILE)) else {
        return false;
    };
    let wanted = format!("rev = \"{pinned}\"");
    lakefile
        .split("[[require]]")
        .any(|package| package.contains("name = \"mathlib\"") && package.contains(&wanted))
}

fn markers(source: &str) -> Vec<Siglum> {
    let mut found: Vec<Siglum> = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix(MARKER))
        .filter_map(|written| written.trim().parse().ok())
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Stable Stemma identifiers a Lean module says its declarations establish.
pub fn declared_statements(source: &str) -> Vec<Siglum> {
    markers(source)
}

fn concise(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("info:"))
        .unwrap_or("Lean could not complete the check")
        .chars()
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_informational_lake_line_does_not_hide_the_failure() {
        assert_eq!(
            concise(
                "info: no previous manifest, creating one from scratch\nerror: package unavailable"
            ),
            "error: package unavailable"
        );
        assert_eq!(
            concise("info: no previous manifest, creating one from scratch"),
            "Lean could not complete the check"
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("stemma-lean-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn lean_tools_run_with_elans_directory_on_their_path() {
        let command = tool("lake");
        let path = command
            .get_envs()
            .find(|(name, _)| *name == "PATH")
            .and_then(|(_, value)| value)
            .expect("a path for the tool");
        let places: Vec<PathBuf> = std::env::split_paths(path).collect();

        assert_eq!(places, crate::job::agents::where_programs_are());
        if let Some(home) = dirs::home_dir() {
            assert!(places.contains(&home.join(".elan/bin")));
        }
    }

    #[test]
    fn a_mark_is_broken_only_when_its_line_has_no_such_block() {
        let kept: BlockId = "blk:01K5YQ4TQ2VZ8H3M9N0P1R2S3T".parse().unwrap();
        let report = ExtractorReport {
            marks: [
                format!("rings/{kept}"),
                "rings/blk:01K5YQ4TQ2VZ8H3M9N0P1R2S3V".to_owned(),
                "fields/blk:01K5YQ4TQ2VZ8H3M9N0P1R2S3W".to_owned(),
            ]
            .into_iter()
            .map(|block| ExtractedMark {
                block,
                declarations: Vec::new(),
            })
            .collect(),
        };

        assert_eq!(
            broken_for(&report, "rings/", &[kept]),
            "unknown block marks: rings/blk:01K5YQ4TQ2VZ8H3M9N0P1R2S3V"
        );
        assert_eq!(
            broken_for(&report, "fields/", &[]),
            "unknown block marks: fields/blk:01K5YQ4TQ2VZ8H3M9N0P1R2S3W"
        );
        assert_eq!(broken_for(&report, "groups/", &[]), "");
    }

    #[test]
    fn a_library_check_is_reused_until_a_lean_file_changes() {
        let root = scratch("fingerprint");
        let pins = Pins::default();
        std::fs::write(root.join("Algebra.lean"), "theorem a : True := trivial").unwrap();
        std::fs::create_dir_all(root.join("lines/rings")).unwrap();
        let first = library_fingerprint(&root, &pins).unwrap();

        // Writing the line is not writing the library.
        std::fs::write(root.join("lines/rings/line.json"), "{}").unwrap();
        assert_eq!(library_fingerprint(&root, &pins).unwrap(), first);

        std::fs::write(root.join("Algebra.lean"), "theorem a : 1 = 1 := rfl").unwrap();
        assert_ne!(library_fingerprint(&root, &pins).unwrap(), first);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn lake_resolves_again_only_when_its_configuration_changes() {
        let root = scratch("resolution");
        std::fs::write(root.join("lean-toolchain"), "leanprover/lean4:v4.34.1\n").unwrap();
        std::fs::write(root.join(repository::LAKEFILE), "name = \"algebra\"\n").unwrap();
        let first = resolution_inputs(&root);

        std::fs::write(root.join("Algebra.lean"), "theorem a : True := trivial").unwrap();
        assert_eq!(resolution_inputs(&root), first);

        std::fs::write(root.join("lean-toolchain"), "leanprover/lean4:v4.35.0\n").unwrap();
        assert_ne!(resolution_inputs(&root), first);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_unrecorded_mathlib_revision_cannot_satisfy_a_pin() {
        assert!(check_mathlib(Path::new("/nonexistent/stemma"), "pinned").is_err());
        let root = std::env::temp_dir().join(format!("stemma-mathlib-pin-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let manifest = root.join("lake-manifest.json");
        std::fs::write(&manifest, r#"{"packages": []}"#).unwrap();
        assert!(check_mathlib(&root, "pinned").is_err());
        std::fs::write(
            &manifest,
            r#"{"packages": [{"name":"mathlib","rev":"pinned"}]}"#,
        )
        .unwrap();
        assert!(check_mathlib(&root, "pinned").is_ok());
        assert!(check_mathlib(&root, "another").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_clean_module_names_the_statements_it_verifies() {
        let found = markers(
            "-- stemma: verifies thm:main\n theorem main : True := by trivial\n\
             -- stemma: verifies thm:main\n-- stemma: verifies lem:step",
        );

        assert_eq!(
            found.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["thm:main", "lem:step"]
        );
    }

    #[test]
    fn the_lean_release_is_read_from_its_version_line() {
        assert_eq!(
            version_in(b"Lean (version 4.24.0, arm64-apple-darwin)"),
            Some("4.24.0".to_owned())
        );
    }

    fn declaration(sorry_free: bool, axioms: &[&str], statement: &str) -> ExtractedDeclaration {
        ExtractedDeclaration {
            name: "Algebra.main".to_owned(),
            module: "Algebra".to_owned(),
            statement: statement.to_owned(),
            sorry_free,
            axioms: axioms.iter().map(|axiom| (*axiom).to_owned()).collect(),
            reaches: Vec::new(),
            statement_closure: "sha256:abc".to_owned(),
        }
    }

    #[test]
    fn only_sorry_free_declarations_using_the_allowed_axioms_verify_a_block() {
        let block = "blk:01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap();
        assert_eq!(
            classify(
                block,
                vec![declaration(true, &["Classical.choice"], "True")]
            )
            .state,
            Verification::Verified
        );
        let sorry = classify(block, vec![declaration(false, &[], "True")]);
        assert_eq!(sorry.state, Verification::Unverified);
        assert!(sorry.diagnostic.contains("sorry"));
        let axiom = classify(block, vec![declaration(true, &["unsafeAxiom"], "True")]);
        assert_eq!(axiom.state, Verification::Unverified);
        assert!(axiom.diagnostic.contains("unsafeAxiom"));
    }

    #[test]
    fn a_verified_negation_refutes_its_block() {
        let block = "blk:01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap();
        let result = classify(block, vec![declaration(true, &[], "Not (1 = 2)")]);
        assert_eq!(result.state, Verification::Refuted);
    }
}

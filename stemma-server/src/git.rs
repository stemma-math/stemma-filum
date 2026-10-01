//! The repositories an instance keeps.
//!
//! Git is the source of truth, so these are the durable part of an
//! instance and the database beside them is an index. They are bare: the
//! server holds no working tree, because nothing here writes mathematics.
//!
//! Every group has one shared repository, and every person has one of
//! their own **per group** — the environment a line is written against is
//! pinned per group, so a private line has to live under that pin. That is
//! also what makes privacy physical rather than a property of a row: a
//! private line sits in a repository nobody else can clone.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use stemma_core::id::{GroupId, Slug, UserId};
use stemma_core::line::Name;
use stemma_core::proposal::LibraryFileChange;
use stemma_core::repository::{self, Manifest};

/// What applying an approved library change came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryWritten {
    /// It became this commit.
    Taken(String),
    /// These files changed both in the proposal and since its base.
    Conflicted(Vec<String>),
}

/// Who the instance is, when it is the one making a commit.
const COMMITTER: (&str, &str) = ("Stemma", "stemma@localhost");

/// Which kind of repository a path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The group's own, which every member clones.
    Shared,
    /// One person's, inside one group, which only they can clone.
    Personal,
    /// Immutable snapshots the group deliberately made public.
    Releases,
}

impl Kind {
    /// What this kind is called in a path and in a message.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Personal => "personal",
            Self::Releases => "releases",
        }
    }
}

/// Where an instance keeps its repositories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repositories {
    root: PathBuf,
}

impl Repositories {
    /// The repositories under this directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory holding every repository.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The group's shared repository.
    pub fn shared(&self, group: &GroupId) -> PathBuf {
        self.root.join(group.slug().as_str()).join("shared.git")
    }

    /// One person's repository inside one group.
    pub fn personal(&self, group: &GroupId, user: &UserId) -> PathBuf {
        self.root
            .join(group.slug().as_str())
            .join("personal")
            .join(format!("{}.git", user.slug()))
    }

    /// The public snapshots a group has released.
    pub fn releases(&self, group: &GroupId) -> PathBuf {
        self.root.join(group.slug().as_str()).join("releases.git")
    }

    /// Every private package belonging to one group, in stable order.
    pub fn personal_for_group(&self, group: &GroupId) -> Vec<PathBuf> {
        let directory = self.root.join(group.slug().as_str()).join("personal");
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut found: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| exists(path))
            .collect();
        found.sort();
        found
    }

    /// The path a request names, if it names one of ours.
    ///
    /// Everything reachable is spelled out here rather than assembled from
    /// what was asked for, so no request can walk out of the root.
    pub fn resolve(&self, group: &Slug, rest: &str) -> Option<(Kind, PathBuf)> {
        let group = GroupId::new(group.clone());

        if rest == "shared.git" {
            return Some((Kind::Shared, self.shared(&group)));
        }

        if rest == "releases.git" {
            return Some((Kind::Releases, self.releases(&group)));
        }

        let personal = rest
            .strip_prefix("personal/")?
            .strip_suffix(".git")
            .and_then(|name| Slug::parse(name).ok())?;

        Some((
            Kind::Personal,
            self.personal(&group, &UserId::new(personal)),
        ))
    }

    /// The groups that have a directory here.
    ///
    /// Used to notice a directory with no group and a group with no
    /// directory, which is the kind of drift only a check catches.
    pub fn groups_on_disk(&self) -> Result<Vec<String>> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Ok(Vec::new());
        };

        let mut found: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();

        found.sort();

        Ok(found)
    }

    /// Every repository on disk, in a settled order.
    ///
    /// The layout is walked rather than asked of the database, because
    /// what has to be copied or counted is what is there and not what a
    /// row says should be.
    pub fn on_disk(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();

        for group in self.groups_on_disk().unwrap_or_default() {
            let directory = self.root.join(&group);
            let shared = directory.join("shared.git");

            if exists(&shared) {
                found.push(shared);
            }

            let releases = directory.join("releases.git");
            if exists(&releases) {
                found.push(releases);
            }

            let Ok(entries) = std::fs::read_dir(directory.join("personal")) else {
                continue;
            };

            let mut personal: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| exists(path))
                .collect();

            personal.sort();
            found.extend(personal);
        }

        found
    }
}

/// Copies one shared-library tree into a new, history-free public release.
///
/// Each version is a root commit. Consequently a public tag reveals exactly
/// that release's source and no unpublished commits that led to it.
pub fn publish_release(
    shared: &Path,
    releases: &Path,
    source_commit: &str,
    version: &str,
    author: &Author<'_>,
) -> Result<String> {
    if !exists(releases) {
        if let Some(parent) = releases.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        run(
            Path::new("."),
            &["init", "--quiet", "--bare", &releases.display().to_string()],
        )?;
    }

    let tag = format!("refs/tags/v{version}");
    if run(releases, &["show-ref", "--verify", "--quiet", &tag]).is_ok() {
        bail!("version {version} has already been released");
    }

    let temporary = format!("refs/stemma/publishing/{}", uuid());
    run(
        releases,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            &shared.display().to_string(),
            &format!("+{source_commit}:{temporary}"),
        ],
    )
    .context("cannot copy the shared source into the release repository")?;

    let published = (|| {
        let tree = run(releases, &["rev-parse", &format!("{temporary}^{{tree}}")])?;
        let commit = run_with(
            releases,
            &["commit-tree", &tree, "-m", &format!("Release v{version}")],
            None,
            &author.environment(),
        )?;
        run(releases, &["update-ref", &tag, &commit])?;
        Ok(commit)
    })();

    let _ = run(releases, &["update-ref", "-d", &temporary]);
    match published {
        Ok(commit) => {
            if let Err(error) = run(releases, &["gc", "--quiet", "--prune=now"]) {
                let _ = run(releases, &["update-ref", "-d", &tag]);
                return Err(error).context("cannot seal the release source");
            }
            Ok(commit)
        }
        Err(error) => Err(error),
    }
}

/// Removes a release ref while compensating for a failed database record.
pub fn remove_release(path: &Path, version: &str) -> Result<()> {
    run(
        path,
        &["update-ref", "-d", &format!("refs/tags/v{version}")],
    )?;
    Ok(())
}

/// Makes a bare repository, unless it is already there.
///
/// Returns whether it made one, so that the caller can say what happened
/// rather than guess.
pub fn ensure(path: &Path, manifest: &Manifest) -> Result<bool> {
    if path.join("HEAD").is_file() {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }

    run(
        Path::new("."),
        &[
            "init",
            "--quiet",
            "--bare",
            "--initial-branch=main",
            &path.display().to_string(),
        ],
    )?;

    // A repository with no branch clones with a warning and leaves the
    // person wondering what went wrong, so it starts with one commit that
    // says what it is.
    seed(path, manifest)?;

    Ok(true)
}

/// Writes the first commit, without ever making a working tree.
fn seed(path: &Path, manifest: &Manifest) -> Result<()> {
    let mut files = vec![(repository::MANIFEST.to_owned(), manifest.to_text())];
    files.extend(manifest.lake_files());
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut entries = String::new();
    for (name, contents) in files {
        let blob = run_with_input(path, &["hash-object", "-w", "--stdin"], &contents)?;
        entries.push_str(&format!("100644 blob {blob}\t{name}\n"));
    }
    let tree = run_with_input(path, &["mktree"], &entries)?;
    let commit = run(
        path,
        &[
            "-c",
            &format!("user.name={}", COMMITTER.0),
            "-c",
            &format!("user.email={}", COMMITTER.1),
            "commit-tree",
            &tree,
            "-m",
            "Make the repository",
        ],
    )?;

    // The empty old value says the branch must not exist yet, so a
    // repository somebody else seeded meanwhile is never overwritten.
    run(path, &["update-ref", "refs/heads/main", &commit, ""])?;

    Ok(())
}

/// Copies a repository, by asking git for it.
///
/// A push writes its objects before the reference that names them, so a
/// copy taken file by file can end up holding a reference to an object
/// it did not get. Git hands over a set that hangs together or nothing
/// at all, which is the only kind of copy worth keeping.
pub fn mirror(from: &Path, to: &Path) -> Result<()> {
    let parent = to.parent().filter(|parent| !parent.as_os_str().is_empty());

    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }

    // A backup may be started by root and then handed to the service
    // account. Its inherited directory can consequently be somewhere the
    // service account cannot traverse. The destination's parent was made by
    // this process and is always a safe place from which to start git.
    run(
        parent.unwrap_or(Path::new(".")),
        &[
            "clone",
            "--quiet",
            "--mirror",
            "--no-hardlinks",
            &from.display().to_string(),
            &to.display().to_string(),
        ],
    )?;

    // A clone remembers where it was made from. A copy that is put back
    // on a machine has no business fetching from the one it was taken
    // off, so the remote goes with the rest of the journey.
    run(to, &["remote", "remove", "origin"])?;

    Ok(())
}

/// Whoever a commit is by.
///
/// The instance commits, and the person authors. Git keeps the two
/// apart and so does this: every mutation is attributed to a concrete
/// person, and the machinery that carried it out is not that person.
#[derive(Debug, Clone, Copy)]
pub struct Author<'a> {
    /// What they are called.
    pub name: &'a str,
    /// Where they are reachable.
    pub email: &'a str,
}

impl Author<'_> {
    /// What git is told about them.
    fn environment(&self) -> Vec<(&'static str, String)> {
        vec![
            ("GIT_AUTHOR_NAME", self.name.to_owned()),
            ("GIT_AUTHOR_EMAIL", self.email.to_owned()),
            ("GIT_COMMITTER_NAME", COMMITTER.0.to_owned()),
            ("GIT_COMMITTER_EMAIL", COMMITTER.1.to_owned()),
        ]
    }
}

/// Starts a line in a bare repository, with nothing in it but itself.
///
/// The source is the author's from the first commit: what the form
/// collected is written into the file rather than kept in a row
/// beside it, so a clone of this repository is the whole line.
pub fn start_line(path: &Path, name: &Name, source: &str, author: &Author<'_>) -> Result<String> {
    let inside = format!("{}/{}/{}", repository::LINES, name, repository::ENTRY);

    write_commit(path, author, &format!("Start {name}"), |index| {
        let blob = run_with_input(path, &["hash-object", "-w", "--stdin"], source)?;

        index_git(
            path,
            index,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{blob},{inside}"),
            ],
        )?;

        Ok(())
    })
}

/// The most a root file may weigh to be handed to a machine.
const LARGEST_ROOT_FILE: u64 = 256 * 1024;

/// The most root files handed over.
const MOST_ROOT_FILES: usize = 200;

/// Every text file of a repository outside its lines, as it is now: the
/// manifest and whatever the group keeps beside it. A file too large to
/// be configuration, or not text, is left out.
pub fn root_files(path: &Path) -> Result<Vec<(String, String)>> {
    let Some(head) = head_commit(path) else {
        return Ok(Vec::new());
    };

    let listed = run(path, &["ls-tree", "-r", "-l", "-z", &head])?;
    let mut found = Vec::new();

    for entry in listed.split('\0').filter(|entry| !entry.is_empty()) {
        let Some((about, file)) = entry.split_once('\t') else {
            continue;
        };
        let mut parts = about.split_whitespace();
        let (Some(_mode), Some(kind), Some(_object), Some(size)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };

        if kind != "blob"
            || file.starts_with(&format!("{}/", repository::LINES))
            || size
                .parse::<u64>()
                .map_or(true, |size| size > LARGEST_ROOT_FILE)
        {
            continue;
        }

        if let Ok(content) = show(path, &head, file) {
            found.push((file.to_owned(), content));
        }

        if found.len() >= MOST_ROOT_FILES {
            break;
        }
    }

    Ok(found)
}

/// Whether a repository already has a line by this name.
pub fn holds_line(path: &Path, name: &Name) -> bool {
    let Some(head) = head_commit(path) else {
        return false;
    };

    run(
        path,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{head}:{}/{name}", repository::LINES),
        ],
    )
    .is_ok()
}

/// Every line a repository holds at a commit.
pub fn lines_at(path: &Path, commit: &str) -> Result<Vec<String>> {
    let listed = run(
        path,
        &[
            "ls-tree",
            "--name-only",
            &format!("{commit}:{}", repository::LINES),
        ],
    )
    .unwrap_or_default();

    Ok(listed.lines().map(str::to_owned).collect())
}

/// Moves a line from one repository into another.
///
/// `history` decides what the other side receives: the state of the
/// day, or every commit that ever touched the line. Neither carries
/// anything else the author wrote — their false starts elsewhere stay
/// where they are.
pub fn bring_line(to: &Path, from: &Path, name: &Name, history: bool) -> Result<()> {
    let kept = format!("refs/stemma/moving/{name}");

    // The objects have to be here before anything can be built from
    // them, and a ref keeps them from being collected meanwhile.
    run(
        to,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            &from.display().to_string(),
            &format!("+refs/heads/main:{kept}"),
        ],
    )?;

    let inside = format!("{}/{}", repository::LINES, name);
    let commits: Vec<String> = match history {
        true => run(to, &["rev-list", "--reverse", &kept, "--", &inside])?
            .lines()
            .map(str::to_owned)
            .collect(),
        false => vec![run(to, &["rev-parse", &kept])?],
    };

    for commit in &commits {
        let Ok(tree) = run(to, &["rev-parse", &format!("{commit}:{inside}")]) else {
            continue;
        };
        let author = author_of(to, commit)?;
        let message = run(to, &["log", "-1", "--format=%B", commit])?;
        let when = run(to, &["log", "-1", "--format=%aI", commit])?;

        write_commit_at(
            to,
            &Author {
                name: &author.0,
                email: &author.1,
            },
            Some(&when),
            message.trim(),
            |index| {
                remove_prefix(to, index, &inside)?;
                graft(to, index, &inside, &tree)
            },
        )?;
    }

    run(to, &["update-ref", "-d", &kept])?;

    Ok(())
}

/// Moves the private modules carrying a line's marks, and their local imports,
/// into the shared package. A module carrying marks for another private line
/// is refused rather than leaking that line while moving this one.
pub fn bring_line_library(
    to: &Path,
    from: &Path,
    line: &Name,
    author: &Author<'_>,
) -> Result<Vec<String>> {
    let private_head = head_commit(from).context("the private library has no branch")?;
    let shared_head = head_commit(to).context("the shared library has no branch")?;
    let sources: std::collections::BTreeMap<String, String> = root_files(from)?
        .into_iter()
        .filter(|(path, _)| path.ends_with(".lean"))
        .collect();
    let prefix = format!("{}/", line.as_str());
    let mut selected: std::collections::BTreeSet<String> = sources
        .iter()
        .filter(|(_, source)| {
            marks_in(source)
                .iter()
                .any(|mark| mark.starts_with(&prefix))
        })
        .map(|(path, _)| path.clone())
        .collect();
    if selected.is_empty() {
        return Ok(Vec::new());
    }

    let by_module: std::collections::BTreeMap<String, String> = sources
        .keys()
        .map(|path| (module_of(path), path.clone()))
        .collect();
    let mut pending: Vec<String> = selected.iter().cloned().collect();
    while let Some(path) = pending.pop() {
        let Some(source) = sources.get(&path) else {
            continue;
        };
        for imported in imports_in(source) {
            if let Some(dependency) = by_module.get(&imported)
                && selected.insert(dependency.clone())
            {
                pending.push(dependency.clone());
            }
        }
    }

    for path in &selected {
        let foreign: Vec<_> = marks_in(&sources[path])
            .into_iter()
            .filter(|mark| !mark.starts_with(&prefix))
            .collect();
        if !foreign.is_empty() {
            bail!(
                "{} also carries marks for private work ({}); split the module before sharing",
                path,
                foreign.join(", ")
            );
        }
    }

    let mut into_shared = Vec::new();
    let mut out_of_private = Vec::new();
    for path in &selected {
        let source = sources[path].clone();
        let before = content_at(to, &shared_head, path)?;
        if before.as_ref().is_some_and(|current| current != &source) {
            bail!("the shared library already has a different `{path}`");
        }
        into_shared.push(LibraryFileChange {
            path: path.clone(),
            before: before.clone(),
            after: Some(source.clone()),
        });
        out_of_private.push(LibraryFileChange {
            path: path.clone(),
            before: Some(source),
            after: None,
        });
    }

    let root = "GroupLibrary.lean";
    let before = content_at(to, &shared_head, root)?.unwrap_or_default();
    let mut imports: std::collections::BTreeSet<String> = before
        .lines()
        .filter_map(|line| line.trim().strip_prefix("import ").map(str::to_owned))
        .collect();
    imports.extend(
        selected
            .iter()
            .filter(|path| path.as_str() != root)
            .map(|path| module_of(path)),
    );
    let after = imports
        .into_iter()
        .map(|module| format!("import {module}\n"))
        .collect::<String>();
    if after != before {
        into_shared.push(LibraryFileChange {
            path: root.to_owned(),
            before: Some(before),
            after: Some(after),
        });
    }

    match write_library(
        to,
        &shared_head,
        &into_shared,
        author,
        &format!("Share {} formalization", line),
    )? {
        LibraryWritten::Taken(_) => {}
        LibraryWritten::Conflicted(files) => {
            bail!(
                "the shared library changed concurrently in {}",
                files.join(", ")
            )
        }
    }
    match write_library(
        from,
        &private_head,
        &out_of_private,
        author,
        &format!("Move {} formalization to the group", line),
    )? {
        LibraryWritten::Taken(_) => {}
        LibraryWritten::Conflicted(files) => {
            bail!(
                "the private library changed concurrently in {}",
                files.join(", ")
            )
        }
    }

    Ok(selected.into_iter().collect())
}

fn marks_in(source: &str) -> Vec<String> {
    source
        .split("@[stemma \"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(mark, _)| mark.to_owned()))
        .collect()
}

fn imports_in(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("import "))
        .flat_map(str::split_whitespace)
        .map(str::to_owned)
        .collect()
}

fn module_of(path: &str) -> String {
    path.trim_end_matches(".lean").replace(['/', '\\'], ".")
}

/// Takes a line out of a repository, saying why in `message`.
pub fn drop_line(path: &Path, name: &Name, author: &Author<'_>, message: &str) -> Result<String> {
    let inside = format!("{}/{}", repository::LINES, name);

    write_commit(path, author, message, |index| {
        remove_prefix(path, index, &inside)
    })
}

/// Commits what a line says now: its source, and its Lean module or the
/// absence of one. Nothing is committed when git already has exactly
/// that, and then there is no commit to return.
pub fn write_line(
    path: &Path,
    name: &Name,
    source: &str,
    lean: Option<&str>,
    author: &Author<'_>,
    message: &str,
) -> Result<Option<String>> {
    let entry = format!("{}/{}/{}", repository::LINES, name, repository::ENTRY);
    let module = format!("{}/{}/{}", repository::LINES, name, repository::LEAN_ENTRY);

    let head = head_commit(path).context("the repository has no branch yet")?;
    let before = run(path, &["rev-parse", &format!("{head}^{{tree}}")])?;
    let index = path.join(format!("stemma-index-{}", uuid()));
    let _ = std::fs::remove_file(&index);

    let built = (|| {
        index_git(path, &index, &["read-tree", &head])?;

        let blob = run_with_input(path, &["hash-object", "-w", "--stdin"], source)?;
        index_git(
            path,
            &index,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{blob},{entry}"),
            ],
        )?;

        match lean {
            Some(lean) => {
                let blob = run_with_input(path, &["hash-object", "-w", "--stdin"], lean)?;
                index_git(
                    path,
                    &index,
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("100644,{blob},{module}"),
                    ],
                )?;
            }
            None => {
                index_git(path, &index, &["update-index", "--force-remove", &module])?;
            }
        }

        let tree = index_git(path, &index, &["write-tree"])?;
        if tree == before {
            return Ok(None);
        }

        let commit = run_with(
            path,
            &["commit-tree", &tree, "-p", &head, "-m", message],
            None,
            &author.environment(),
        )?;
        run(path, &["update-ref", "refs/heads/main", &commit, &head])?;

        Ok(Some(commit))
    })();

    let _ = std::fs::remove_file(&index);
    let _ = std::fs::remove_dir_all(index.with_extension("worktree"));

    built
}

/// Applies an approved agent change to a library, merging concurrent work per file.
///
/// Files untouched by the proposal always come from the current head. A proposed
/// file is accepted when the current head still agrees with its base; when both
/// changed it differently, the complete proposal waits for another review.
pub fn write_library(
    path: &Path,
    base: &str,
    changes: &[LibraryFileChange],
    author: &Author<'_>,
    message: &str,
) -> Result<LibraryWritten> {
    let head = head_commit(path).context("the repository has no branch yet")?;
    // The base arrives from a machine. Only an object name is ever handed
    // to git, never something it could read as an option.
    anyhow::ensure!(
        is_object_name(base),
        "the library change names no commit in this repository"
    );
    run(
        path,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    )
    .context("the library change names no commit in this repository")?;

    let mut conflicts = Vec::new();
    let mut changes_tree = false;
    for change in changes {
        check_library_path(&change.path)?;
        let at_base = content_at(path, base, &change.path)?;
        if at_base != change.before {
            anyhow::bail!(
                "the approved change does not match its base for {}",
                change.path
            );
        }
        let current = content_at(path, &head, &change.path)?;
        if current != change.before && current != change.after && change.after != change.before {
            conflicts.push(change.path.clone());
        }
        if change.after != change.before && current != change.after {
            changes_tree = true;
        }
    }
    if !conflicts.is_empty() {
        conflicts.sort();
        return Ok(LibraryWritten::Conflicted(conflicts));
    }
    if !changes_tree {
        return Ok(LibraryWritten::Taken(head));
    }

    let commit = write_commit(path, author, message, |index| {
        for change in changes {
            let current = content_at(path, &head, &change.path)?;
            if change.after == change.before || current == change.after {
                continue;
            }
            match &change.after {
                Some(contents) => {
                    let blob = run_with_input(path, &["hash-object", "-w", "--stdin"], contents)?;
                    index_git(
                        path,
                        index,
                        &[
                            "update-index",
                            "--add",
                            "--cacheinfo",
                            &format!("100644,{blob},{}", change.path),
                        ],
                    )?;
                }
                None => {
                    index_git(
                        path,
                        index,
                        &["update-index", "--force-remove", &change.path],
                    )?;
                }
            }
        }
        Ok(())
    })?;

    Ok(LibraryWritten::Taken(commit))
}

/// Applies a person's approved agent proposal, keeping package configuration
/// under the separate admin-only pin operation.
pub fn write_approved_library(
    path: &Path,
    base: &str,
    changes: &[LibraryFileChange],
    author: &Author<'_>,
) -> Result<LibraryWritten> {
    if changes.is_empty() {
        bail!("the approved library change contains no files");
    }
    for change in changes {
        if change.before == change.after {
            bail!("the approved change does not change {}", change.path);
        }
        if matches!(
            change.path.as_str(),
            repository::MANIFEST
                | repository::LAKEFILE
                | repository::LEAN_TOOLCHAIN
                | "GroupLibrary.lean"
                | "PrivateLibrary.lean"
        ) {
            bail!("the server manages `{}`", change.path);
        }
    }
    let written = write_library(
        path,
        base,
        changes,
        author,
        "Apply approved Lean library change",
    )?;
    if matches!(written, LibraryWritten::Conflicted(_)) {
        return Ok(written);
    }

    let head = head_commit(path).context("the repository has no branch yet")?;
    let manifest = content_at(path, &head, repository::MANIFEST)?
        .context("the library has no Stemma manifest")?;
    let manifest = Manifest::parse(&manifest)?;
    let root = match manifest.side {
        repository::Side::Shared => "GroupLibrary.lean",
        repository::Side::Private => "PrivateLibrary.lean",
    };
    let before = content_at(path, &head, root)?.unwrap_or_default();
    let mut imports: std::collections::BTreeSet<String> = before
        .lines()
        .filter_map(|line| line.trim().strip_prefix("import ").map(str::to_owned))
        .collect();
    imports.insert(match manifest.side {
        repository::Side::Shared => "Stemma".to_owned(),
        repository::Side::Private => "GroupLibrary".to_owned(),
    });
    for change in changes
        .iter()
        .filter(|change| change.path.ends_with(".lean"))
    {
        let module = module_of(&change.path);
        match &change.after {
            Some(_) => {
                imports.insert(module);
            }
            None => {
                imports.remove(&module);
            }
        }
    }
    let after = imports
        .into_iter()
        .map(|module| format!("import {module}\n"))
        .collect::<String>();
    write_library(
        path,
        &head,
        &[LibraryFileChange {
            path: root.to_owned(),
            before: Some(before),
            after: Some(after),
        }],
        author,
        "Update Lean library imports",
    )
}

/// Changes a group's pins and regenerates the Lake configuration mirrored by
/// every member package. This is an administrative operation, separate from
/// agent-authored library files.
pub fn write_pins(
    repositories: &Repositories,
    group: &GroupId,
    mut pins: stemma_core::repository::Pins,
    author: &Author<'_>,
) -> Result<String> {
    let shared = repositories.shared(group);
    let head = head_commit(&shared).context("the shared library has no branch")?;
    let written = show(&shared, &head, repository::MANIFEST)?;
    let mut manifest = Manifest::parse(&written)?;
    pins.package = manifest.pins.package.clone();
    manifest.pins = pins;
    let generated: std::collections::BTreeMap<_, _> = manifest
        .lake_files()
        .into_iter()
        .filter(|(path, _)| path == repository::LAKEFILE || path == repository::LEAN_TOOLCHAIN)
        .collect();
    let mut changes = vec![LibraryFileChange {
        path: repository::MANIFEST.to_owned(),
        before: Some(written),
        after: Some(manifest.to_text()),
    }];
    for (path, after) in generated {
        changes.push(LibraryFileChange {
            before: content_at(&shared, &head, &path)?,
            path,
            after: Some(after),
        });
    }
    let commit = match write_library(&shared, &head, &changes, author, "Change Lean package pins")?
    {
        LibraryWritten::Taken(commit) => commit,
        LibraryWritten::Conflicted(files) => {
            bail!(
                "the shared library changed concurrently in {}",
                files.join(", ")
            )
        }
    };

    let toolchain = manifest
        .lake_files()
        .into_iter()
        .find(|(path, _)| path == repository::LEAN_TOOLCHAIN)
        .map(|(_, content)| content)
        .context("the group has no Lean toolchain")?;
    for private in repositories.personal_for_group(group) {
        let Some(private_head) = head_commit(&private) else {
            continue;
        };
        let change = LibraryFileChange {
            path: repository::LEAN_TOOLCHAIN.to_owned(),
            before: content_at(&private, &private_head, repository::LEAN_TOOLCHAIN)?,
            after: Some(toolchain.clone()),
        };
        match write_library(
            &private,
            &private_head,
            &[change],
            author,
            "Follow the group's Lean pin",
        )? {
            LibraryWritten::Taken(_) => {}
            LibraryWritten::Conflicted(files) => {
                bail!(
                    "a private library changed concurrently in {}",
                    files.join(", ")
                )
            }
        }
    }

    Ok(commit)
}

/// Repairs pins an earlier default left in an existing group: the
/// Stemma tag that was never published, which becomes a commit already
/// in the published main build, and a Mathlib built with another Lean
/// than the default one pinned beside it, which becomes the release
/// built with that Lean.
pub fn upgrade_legacy_pins(repositories: &Repositories, group: &GroupId) -> Result<Option<String>> {
    let shared = repositories.shared(group);
    if !exists(&shared) {
        return Ok(None);
    }
    let head = head_commit(&shared).context("the shared library has no branch")?;
    let written = show(&shared, &head, repository::MANIFEST)?;
    let mut pins = Manifest::parse(&written)?.pins;
    let unpublished = pins.stemma.as_deref() == Some(repository::UNPUBLISHED_STEMMA);
    let mismatched = pins.lean.as_deref() == Some(repository::DEFAULT_LEAN)
        && pins.mathlib.as_deref() == Some(repository::MISMATCHED_MATHLIB);
    if !unpublished && !mismatched {
        return Ok(None);
    }
    if unpublished {
        pins.stemma = Some(repository::DEFAULT_STEMMA.to_owned());
    }
    if mismatched {
        pins.mathlib = Some(repository::DEFAULT_MATHLIB.to_owned());
    }
    let author = Author {
        name: COMMITTER.0,
        email: COMMITTER.1,
    };
    write_pins(repositories, group, pins, &author).map(Some)
}

/// Whether `name` is spelled like a full object name, in SHA-1 or SHA-256.
fn is_object_name(name: &str) -> bool {
    matches!(name.len(), 40 | 64) && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// A file as it stood at a commit, or nothing when it was not there.
fn content_at(path: &Path, commit: &str, file: &str) -> Result<Option<String>> {
    match whole(path, &["show", &format!("{commit}:{file}")]) {
        Ok(content) => Ok(Some(content)),
        Err(_) => Ok(None),
    }
}

fn check_library_path(path: &str) -> Result<()> {
    let path = Path::new(path);
    let plain = !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
    let reserved = path.starts_with(repository::LINES) || path.starts_with(".git");
    if !plain || reserved {
        bail!("`{}` is not a library file", path.display());
    }
    Ok(())
}

/// Who wrote a commit: their name and their address.
fn author_of(path: &Path, commit: &str) -> Result<(String, String)> {
    let said = run(path, &["log", "-1", "--format=%an%n%ae", commit])?;
    let mut parts = said.lines();

    Ok((
        parts.next().unwrap_or_default().to_owned(),
        parts.next().unwrap_or_default().to_owned(),
    ))
}

/// Puts a whole tree under a path in the index being built.
///
/// Entry by entry rather than with `read-tree --prefix`, which wants a
/// working tree to check against and there is none here: the instance
/// keeps bare repositories and never checks anything out.
fn graft(path: &Path, index: &Path, inside: &str, tree: &str) -> Result<()> {
    let listed = run(path, &["ls-tree", "-r", "-z", tree])?;

    for entry in listed.split('\0').filter(|entry| !entry.is_empty()) {
        let (about, file) = entry
            .split_once('\t')
            .context("git described a tree in a way this does not understand")?;
        let mut parts = about.split_whitespace();
        let mode = parts.next().unwrap_or("100644");
        let object = parts.nth(1).context("git named no object")?;

        index_git(
            path,
            index,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("{mode},{object},{inside}/{file}"),
            ],
        )?;
    }

    Ok(())
}

/// Takes everything under a path out of the index being built.
fn remove_prefix(path: &Path, index: &Path, inside: &str) -> Result<()> {
    let listed = index_git(path, index, &["ls-files", "--", &format!("{inside}/*")])?;

    for file in listed.lines() {
        index_git(path, index, &["update-index", "--force-remove", file])?;
    }

    Ok(())
}

/// Builds a commit on the branch out of an index nobody else can see.
fn write_commit(
    path: &Path,
    author: &Author<'_>,
    message: &str,
    build: impl FnOnce(&Path) -> Result<()>,
) -> Result<String> {
    write_commit_at(path, author, None, message, build)
}

/// The same, dated as the commit it is replaying was dated.
fn write_commit_at(
    path: &Path,
    author: &Author<'_>,
    when: Option<&str>,
    message: &str,
    build: impl FnOnce(&Path) -> Result<()>,
) -> Result<String> {
    let head = head_commit(path).context("the repository has no branch yet")?;
    let index = path.join(format!("stemma-index-{}", uuid()));
    let _ = std::fs::remove_file(&index);

    let built = (|| {
        index_git(path, &index, &["read-tree", &head])?;
        build(&index)?;

        let tree = index_git(path, &index, &["write-tree"])?;
        let mut environment = author.environment();

        if let Some(when) = when {
            environment.push(("GIT_AUTHOR_DATE", when.to_owned()));
        }

        let commit = run_with(
            path,
            &["commit-tree", &tree, "-p", &head, "-m", message],
            None,
            &environment,
        )?;

        run(path, &["update-ref", "refs/heads/main", &commit, &head])?;

        Ok(commit)
    })();

    let _ = std::fs::remove_file(&index);
    let _ = std::fs::remove_dir_all(index.with_extension("worktree"));

    built
}

/// Runs git against an index of our own rather than the repository's.
///
/// Some of these ask for a working tree even though they touch only the
/// index, so they are given an empty directory to be satisfied by.
/// Nothing is ever written into it: these repositories are bare, and
/// the instance checks nothing out.
fn index_git(path: &Path, index: &Path, arguments: &[&str]) -> Result<String> {
    let empty = index.with_extension("worktree");
    std::fs::create_dir_all(&empty).with_context(|| format!("cannot make {}", empty.display()))?;

    let mut with_worktree = vec!["-c", "core.bare=false"];
    with_worktree.extend_from_slice(arguments);

    run_with(
        path,
        &with_worktree,
        None,
        &[
            ("GIT_INDEX_FILE", index.display().to_string()),
            ("GIT_WORK_TREE", empty.display().to_string()),
        ],
    )
}

/// Something short and unrepeated, for a file only this call uses.
///
/// The counter keeps two calls in the same process apart even when the
/// clock has not moved between them.
fn uuid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static CALLS: AtomicU64 = AtomicU64::new(0);

    format!(
        "{}-{:?}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default(),
        CALLS.fetch_add(1, Ordering::Relaxed)
    )
}

/// Whether a path holds a repository this instance made.
pub fn exists(path: &Path) -> bool {
    path.join("HEAD").is_file()
}

/// What a repository's branch points at, if it has one.
pub fn head_commit(path: &Path) -> Option<String> {
    run(path, &["rev-parse", "refs/heads/main"]).ok()
}

/// Reads one file exactly as it stood at a commit in a bare repository.
///
/// Exactly: the trailing newline of a text file is part of the file,
/// and a comparison that lost it would report a change nobody made.
pub fn show(path: &Path, commit: &str, inside: &str) -> Result<String> {
    whole(path, &["show", &format!("{commit}:{inside}")])
}

/// Runs git inside a repository.
fn run(repository: &Path, arguments: &[&str]) -> Result<String> {
    run_with(repository, arguments, None, &[])
}

/// Runs git inside a repository, handing it something on its input.
fn run_with_input(repository: &Path, arguments: &[&str], input: &str) -> Result<String> {
    run_with(repository, arguments, Some(input), &[])
}

/// Runs git and hands back exactly what it printed.
fn whole(repository: &Path, arguments: &[&str]) -> Result<String> {
    said(repository, arguments, None, &[])
}

/// Runs git, and turns a failure into something an operator can read.
///
/// What git prints is trimmed, because almost everything it is asked
/// here is one identifier on a line of its own. Reading a file is the
/// exception and goes through [`whole`].
fn run_with(
    repository: &Path,
    arguments: &[&str],
    input: Option<&str>,
    environment: &[(&str, String)],
) -> Result<String> {
    Ok(said(repository, arguments, input, environment)?
        .trim()
        .to_owned())
}

/// The whole of what git printed, or what went wrong.
fn said(
    repository: &Path,
    arguments: &[&str],
    input: Option<&str>,
    environment: &[(&str, String)],
) -> Result<String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut command = Command::new("git");
    command
        .args(arguments)
        .current_dir(repository)
        .envs(environment.iter().map(|(name, value)| (*name, value)))
        .stdin(match input {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .with_context(|| format!("cannot run git {}", arguments.join(" ")))?;

    if let Some(input) = input {
        child
            .stdin
            .take()
            .context("git took no input")?
            .write_all(input.as_bytes())
            .context("cannot hand git its input")?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("git {} did not finish", arguments.join(" ")))?;

    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "stemma-git-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        root
    }

    fn group() -> GroupId {
        "grp:algebra".parse().unwrap()
    }

    fn user() -> UserId {
        "usr:ana".parse().unwrap()
    }

    #[test]
    fn a_group_and_a_person_get_different_repositories() {
        let repositories = Repositories::new("/var/lib/stemma/git");

        assert_eq!(
            repositories.shared(&group()),
            Path::new("/var/lib/stemma/git/algebra/shared.git")
        );
        assert_eq!(
            repositories.personal(&group(), &user()),
            Path::new("/var/lib/stemma/git/algebra/personal/ana.git")
        );
        assert_eq!(
            repositories.releases(&group()),
            Path::new("/var/lib/stemma/git/algebra/releases.git")
        );
    }

    #[test]
    fn only_an_object_name_is_taken_as_a_base() {
        assert!(is_object_name(&"a".repeat(40)));
        assert!(is_object_name(&"0".repeat(64)));

        for refused in [
            "--output=/tmp/x",
            "HEAD",
            "main",
            &"a".repeat(39),
            &"g".repeat(40),
            "",
        ] {
            assert!(!is_object_name(refused), "took `{refused}`");
        }
    }

    #[test]
    fn two_scratch_names_made_at_once_differ() {
        assert_ne!(uuid(), uuid());
    }

    #[test]
    fn only_the_paths_this_instance_makes_can_be_reached() {
        let repositories = Repositories::new("/var/lib/stemma/git");
        let algebra = Slug::parse("algebra").unwrap();

        assert_eq!(
            repositories
                .resolve(&algebra, "shared.git")
                .map(|(kind, _)| kind),
            Some(Kind::Shared)
        );
        assert_eq!(
            repositories
                .resolve(&algebra, "personal/ana.git")
                .map(|(kind, _)| kind),
            Some(Kind::Personal)
        );
        assert_eq!(
            repositories
                .resolve(&algebra, "releases.git")
                .map(|(kind, _)| kind),
            Some(Kind::Releases)
        );

        for asked in [
            "../../etc/passwd",
            "personal/../../shared.git",
            "personal/../ana.git",
            "shared",
            "personal/ana",
            "personal/Ana.git",
            "",
        ] {
            assert!(
                repositories.resolve(&algebra, asked).is_none(),
                "reached `{asked}`"
            );
        }
    }

    fn ana() -> Author<'static> {
        Author {
            name: "Ana",
            email: "ana@example.org",
        }
    }

    fn quotients() -> Name {
        Name::parse("quotient-groups").unwrap()
    }

    /// A source with a question and one statement in it.
    fn source(description: &str) -> String {
        stemma_core::document::Document::new("Quotient groups", Some(description.to_owned()))
            .to_text()
    }

    #[test]
    fn a_line_starts_with_the_person_as_its_author_and_the_instance_as_its_committer() {
        let root = somewhere("started");
        let path = Repositories::new(&root).personal(&group(), &user());
        ensure(&path, &manifest()).unwrap();

        let commit = start_line(&path, &quotients(), &source("Does it split?"), &ana()).unwrap();

        assert!(holds_line(&path, &quotients()));
        assert_eq!(head_commit(&path).as_deref(), Some(commit.as_str()));

        let who = run(&path, &["log", "-1", "--format=%an <%ae> / %cn", &commit]).unwrap();
        assert_eq!(who, "Ana <ana@example.org> / Stemma");

        let written = show(
            &path,
            &commit,
            &format!(
                "{}/{}/{}",
                repository::LINES,
                quotients(),
                repository::ENTRY
            ),
        )
        .unwrap();
        assert!(written.contains("Does it split?"));

        // Everything that was already there is still there.
        assert!(show(&path, &commit, repository::MANIFEST).is_ok());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sharing_moves_the_line_and_leaves_the_rest_where_it_was() {
        let root = somewhere("moved");
        let repositories = Repositories::new(&root);
        let mine = repositories.personal(&group(), &user());
        let ours = repositories.shared(&group());
        ensure(&mine, &manifest()).unwrap();
        ensure(&ours, &manifest()).unwrap();

        start_line(&mine, &quotients(), &source("First thought"), &ana()).unwrap();
        let other = Name::parse("another-line").unwrap();
        start_line(&mine, &other, &source("Still mine"), &ana()).unwrap();
        start_line(&mine, &quotients(), &source("A better question"), &ana()).unwrap();

        bring_line(&ours, &mine, &quotients(), false).unwrap();
        drop_line(&mine, &quotients(), &ana(), "Move it").unwrap();

        assert!(holds_line(&ours, &quotients()));
        assert!(!holds_line(&mine, &quotients()), "a move, not a copy");
        assert!(holds_line(&mine, &other), "the rest is untouched");

        let head = head_commit(&ours).unwrap();
        let written = show(
            &ours,
            &head,
            &format!(
                "{}/{}/{}",
                repository::LINES,
                quotients(),
                repository::ENTRY
            ),
        )
        .unwrap();

        assert!(written.contains("A better question"));
        assert!(
            !written.contains("First thought"),
            "the state of the day, and nothing before it"
        );
        assert_eq!(
            run(&ours, &["rev-list", "--count", "HEAD"]).unwrap(),
            "2",
            "the group receives one commit, not somebody's whole history"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_line_can_travel_with_everything_from_the_beginning() {
        let root = somewhere("history");
        let repositories = Repositories::new(&root);
        let mine = repositories.personal(&group(), &user());
        let ours = repositories.shared(&group());
        ensure(&mine, &manifest()).unwrap();
        ensure(&ours, &manifest()).unwrap();

        start_line(&mine, &quotients(), &source("First thought"), &ana()).unwrap();
        start_line(&mine, &quotients(), &source("A better question"), &ana()).unwrap();

        bring_line(&ours, &mine, &quotients(), true).unwrap();

        assert_eq!(
            run(&ours, &["rev-list", "--count", "HEAD"]).unwrap(),
            "3",
            "the first commit of the repository, and one for each of hers"
        );

        let said = run(&ours, &["log", "--format=%an", "-2"]).unwrap();
        assert!(
            said.lines().all(|who| who == "Ana"),
            "her commits stay hers: {said}"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    fn manifest() -> Manifest {
        Manifest::new(
            group(),
            stemma_core::repository::Side::Shared,
            "Algebra, shared",
        )
    }

    #[test]
    fn an_existing_group_with_the_unpublished_default_gets_a_real_pin() {
        let root = somewhere("old-stemma-pin");
        let repositories = Repositories::new(&root);
        let mut old = manifest();
        old.pins.stemma = Some(repository::UNPUBLISHED_STEMMA.to_owned());
        let path = repositories.shared(&group());
        ensure(&path, &old).unwrap();

        let commit = upgrade_legacy_pins(&repositories, &group()).unwrap();
        assert!(commit.is_some());
        assert!(
            upgrade_legacy_pins(&repositories, &group())
                .unwrap()
                .is_none()
        );
        let head = head_commit(&path).unwrap();
        let current = Manifest::parse(&show(&path, &head, repository::MANIFEST).unwrap()).unwrap();
        assert_eq!(
            current.pins.stemma.as_deref(),
            Some(repository::DEFAULT_STEMMA)
        );
        assert!(
            show(&path, &head, repository::LAKEFILE)
                .unwrap()
                .contains(repository::DEFAULT_STEMMA)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_existing_group_with_a_mismatched_mathlib_gets_the_one_built_with_its_lean() {
        let root = somewhere("old-mathlib-pin");
        let repositories = Repositories::new(&root);
        let mut old = manifest();
        old.pins.mathlib = Some(repository::MISMATCHED_MATHLIB.to_owned());
        let path = repositories.shared(&group());
        ensure(&path, &old).unwrap();

        assert!(
            upgrade_legacy_pins(&repositories, &group())
                .unwrap()
                .is_some()
        );
        let head = head_commit(&path).unwrap();
        let current = Manifest::parse(&show(&path, &head, repository::MANIFEST).unwrap()).unwrap();
        assert_eq!(
            current.pins.mathlib.as_deref(),
            Some(repository::DEFAULT_MATHLIB)
        );
        assert!(
            show(&path, &head, repository::LAKEFILE)
                .unwrap()
                .contains(&format!("rev = \"{}\"", repository::DEFAULT_MATHLIB))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_new_repository_has_a_branch_to_clone() {
        let root = somewhere("seeded");
        let repositories = Repositories::new(&root);
        let path = repositories.shared(&group());

        assert!(ensure(&path, &manifest()).unwrap());
        assert!(exists(&path));
        assert!(head_commit(&path).is_some());

        // Making it again finds it and changes nothing.
        let before = head_commit(&path);
        assert!(!ensure(&path, &manifest()).unwrap());
        assert_eq!(head_commit(&path), before);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_new_repository_says_which_group_and_which_side_it_is() {
        let root = somewhere("manifest");
        let repositories = Repositories::new(&root);
        let path = repositories.shared(&group());

        ensure(&path, &manifest()).unwrap();

        let written = run(
            &path,
            &["show", &format!("refs/heads/main:{}", repository::MANIFEST)],
        )
        .unwrap();
        let read_back = Manifest::parse(&written).unwrap();

        assert_eq!(read_back.group, group());
        assert_eq!(read_back.side, stemma_core::repository::Side::Shared);
        assert_eq!(read_back.pins.typst, None, "nobody has pinned anything yet");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn what_is_on_disk_can_be_listed_against_what_is_in_the_database() {
        let root = somewhere("listing");
        let repositories = Repositories::new(&root);

        assert!(repositories.groups_on_disk().unwrap().is_empty());

        ensure(&repositories.shared(&group()), &manifest()).unwrap();
        ensure(
            &repositories.shared(&"grp:topology".parse().unwrap()),
            &Manifest::new(
                "grp:topology".parse().unwrap(),
                stemma_core::repository::Side::Shared,
                "Topology",
            ),
        )
        .unwrap();

        assert_eq!(
            repositories.groups_on_disk().unwrap(),
            ["algebra", "topology"]
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn approved_changes_on_different_files_merge_from_one_base() {
        let root = somewhere("library-merge");
        let path = Repositories::new(&root).shared(&group());
        ensure(&path, &manifest()).unwrap();
        let base = head_commit(&path).unwrap();
        let author = Author {
            name: "Ana",
            email: "ana@example.org",
        };

        for (file, contents) in [
            ("Algebra/Groups.lean", "def G := True\n"),
            ("Algebra/Rings.lean", "def R := True\n"),
        ] {
            assert!(matches!(
                write_approved_library(
                    &path,
                    &base,
                    &[LibraryFileChange {
                        path: file.to_owned(),
                        before: None,
                        after: Some(contents.to_owned()),
                    }],
                    &author,
                )
                .unwrap(),
                LibraryWritten::Taken(_)
            ));
        }

        let head = head_commit(&path).unwrap();
        assert_eq!(
            show(&path, &head, "Algebra/Groups.lean").unwrap(),
            "def G := True\n"
        );
        assert_eq!(
            show(&path, &head, "Algebra/Rings.lean").unwrap(),
            "def R := True\n"
        );
        let imports = show(&path, &head, "GroupLibrary.lean").unwrap();
        assert!(imports.contains("import Algebra.Groups\n"));
        assert!(imports.contains("import Algebra.Rings\n"));
        let repeated = write_approved_library(
            &path,
            &base,
            &[LibraryFileChange {
                path: "Algebra/Groups.lean".to_owned(),
                before: None,
                after: Some("def G := True\n".to_owned()),
            }],
            &author,
        )
        .unwrap();
        assert_eq!(repeated, LibraryWritten::Taken(head.clone()));
        assert_eq!(head_commit(&path).as_deref(), Some(head.as_str()));

        assert!(
            write_approved_library(
                &path,
                &head,
                &[LibraryFileChange {
                    path: repository::LEAN_TOOLCHAIN.to_owned(),
                    before: content_at(&path, &head, repository::LEAN_TOOLCHAIN).unwrap(),
                    after: Some("leanprover/lean4:v4.35.0\n".to_owned()),
                }],
                &author,
            )
            .is_err(),
            "ordinary approved work cannot change admin-owned pins"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sharing_moves_marked_modules_and_their_local_dependencies() {
        let root = somewhere("library-share");
        let repositories = Repositories::new(&root);
        let shared = repositories.shared(&group());
        let private = repositories.personal(&group(), &user());
        ensure(&shared, &manifest()).unwrap();
        ensure(
            &private,
            &Manifest::new(group(), repository::Side::Private, "Ana only"),
        )
        .unwrap();
        let base = head_commit(&private).unwrap();
        let author = Author {
            name: "Ana",
            email: "ana@example.org",
        };
        let marked = "import Algebra.Helper\n@[stemma \"quotient-groups/blk:01ARZ3NDEKTSV4RRFFQ69G5FAV\"] theorem main : True := by trivial\n";
        write_approved_library(
            &private,
            &base,
            &[
                LibraryFileChange {
                    path: "Algebra/Quotients.lean".to_owned(),
                    before: None,
                    after: Some(marked.to_owned()),
                },
                LibraryFileChange {
                    path: "Algebra/Helper.lean".to_owned(),
                    before: None,
                    after: Some("def helper := True\n".to_owned()),
                },
            ],
            &author,
        )
        .unwrap();

        let moved = bring_line_library(&shared, &private, &quotients(), &author).unwrap();
        assert_eq!(moved, ["Algebra/Helper.lean", "Algebra/Quotients.lean"]);
        let shared_head = head_commit(&shared).unwrap();
        assert!(show(&shared, &shared_head, "Algebra/Quotients.lean").is_ok());
        assert!(show(&shared, &shared_head, "Algebra/Helper.lean").is_ok());
        let private_head = head_commit(&private).unwrap();
        assert!(show(&private, &private_head, "Algebra/Quotients.lean").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

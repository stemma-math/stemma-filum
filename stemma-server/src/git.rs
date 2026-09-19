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
use stemma_core::repository::{self, Manifest};

/// Who the instance is, when it is the one making a commit.
const COMMITTER: (&str, &str) = ("Stemma", "stemma@localhost");

/// Which kind of repository a path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The group's own, which every member clones.
    Shared,
    /// One person's, inside one group, which only they can clone.
    Personal,
}

impl Kind {
    /// What this kind is called in a path and in a message.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Personal => "personal",
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

    /// The path a request names, if it names one of ours.
    ///
    /// Everything reachable is spelled out here rather than assembled from
    /// what was asked for, so no request can walk out of the root.
    pub fn resolve(&self, group: &Slug, rest: &str) -> Option<(Kind, PathBuf)> {
        let group = GroupId::new(group.clone());

        if rest == "shared.git" {
            return Some((Kind::Shared, self.shared(&group)));
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
    let blob = run_with_input(path, &["hash-object", "-w", "--stdin"], &manifest.to_text())?;
    let tree = run_with_input(
        path,
        &["mktree"],
        &format!("100644 blob {blob}\t{}\n", repository::MANIFEST),
    )?;
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

    run(path, &["update-ref", "refs/heads/main", &commit])?;

    Ok(())
}

/// Whether a path holds a repository this instance made.
pub fn exists(path: &Path) -> bool {
    path.join("HEAD").is_file()
}

/// What a repository's branch points at, if it has one.
pub fn head_commit(path: &Path) -> Option<String> {
    run(path, &["rev-parse", "refs/heads/main"]).ok()
}

/// Runs git inside a repository.
fn run(repository: &Path, arguments: &[&str]) -> Result<String> {
    run_with(repository, arguments, None)
}

/// Runs git inside a repository, handing it something on its input.
fn run_with_input(repository: &Path, arguments: &[&str], input: &str) -> Result<String> {
    run_with(repository, arguments, Some(input))
}

/// Runs git, and turns a failure into something an operator can read.
fn run_with(repository: &Path, arguments: &[&str], input: Option<&str>) -> Result<String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut command = Command::new("git");
    command
        .args(arguments)
        .current_dir(repository)
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

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
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

    fn manifest() -> Manifest {
        Manifest::new(
            group(),
            stemma_core::repository::Side::Shared,
            "Algebra, shared",
        )
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
}

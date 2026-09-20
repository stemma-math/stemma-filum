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

/// Takes a line out of a repository.
pub fn drop_line(path: &Path, name: &Name, author: &Author<'_>) -> Result<String> {
    let inside = format!("{}/{}", repository::LINES, name);

    write_commit(
        path,
        author,
        &format!("Move {name} to the group"),
        |index| remove_prefix(path, index, &inside),
    )
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
fn uuid() -> String {
    format!(
        "{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
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
    fn source(question: &str) -> String {
        format!(
            "= Quotient groups\n\n#question[{question}]\n\n             #theorem(id: \"thm:one\", central: true)[It splits.]\n"
        )
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
        drop_line(&mine, &quotients(), &ana()).unwrap();

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

//! Two versions of one line, and the person who wrote both.
//!
//! This never runs on its own and never resets anything. It reads the
//! two versions out of git, says which statements they disagree about,
//! and writes what the person decided as an ordinary merge commit — so
//! the version not kept is still in the history, and still theirs.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use stemma_core::line::Name;
use stemma_core::reconcile::{self, Divergence, Keep, Mark};
use stemma_core::repository;

use crate::git;
use crate::home::Home;

/// One line whose two versions have parted.
#[derive(Debug, Clone)]
pub struct Parted {
    /// Which line it is.
    pub line: Name,
    /// What both machines started from.
    pub base: String,
    /// What this machine has.
    pub here: String,
    /// What the instance has.
    pub there: String,
    /// What the two of them disagree about.
    pub divergences: Vec<Divergence>,
}

impl Parted {
    /// Where the line's source is inside the repository.
    fn inside(&self) -> String {
        format!("{}/{}/{}", repository::LINES, self.line, repository::ENTRY)
    }
}

/// Says what the two versions disagree about.
pub fn list(home: &Home, at: &Path, out: &mut impl Write) -> Result<()> {
    let clone = clone_at(home, at)?;
    git::fetch(&clone)?;

    let distance = git::distance(&clone)?;

    if !distance.has_parted() {
        writeln!(
            out,
            "This machine and the instance have not parted. `stemma sync` is enough."
        )?;

        return Ok(());
    }

    let parted = between(&clone)?;

    if parted.iter().all(|line| line.divergences.is_empty()) {
        writeln!(
            out,
            "Both sides moved, and they wrote about different things. \
             `stemma conflict resolve --keep here` brings them together."
        )?;

        return Ok(());
    }

    for line in &parted {
        if line.divergences.is_empty() {
            continue;
        }

        writeln!(out, "{}", line.line)?;

        for divergence in &line.divergences {
            writeln!(out, "  {}  {}", divergence.mark, divergence.said())?;
            writeln!(out, "    here   {}", shown(divergence.here.as_deref()))?;
            writeln!(out, "    there  {}", shown(divergence.there.as_deref()))?;
        }
    }

    writeln!(out)?;
    writeln!(
        out,
        "Keep one side with `stemma conflict resolve --keep here|there`, \
         or ask an agent to reconcile them with `stemma conflict reconcile --agent AGENT`."
    )?;

    Ok(())
}

/// Brings the two versions together, keeping one side wherever they
/// disagree.
pub fn resolve(home: &Home, at: &Path, keep: Keep, out: &mut impl Write) -> Result<()> {
    let clone = clone_at(home, at)?;
    git::fetch(&clone)?;

    let parted = between(&clone)?;
    let decisions: BTreeMap<String, BTreeMap<Mark, Keep>> = parted
        .iter()
        .map(|line| {
            let decided = line
                .divergences
                .iter()
                .map(|divergence| (divergence.mark.clone(), keep))
                .collect();

            (line.line.as_str().to_owned(), decided)
        })
        .collect();

    let settled = settle(&clone, &decisions)?;

    for line in &settled {
        writeln!(out, "{line}  brought together")?;
    }

    writeln!(
        out,
        "The version not kept is still in this clone's history, \
         and `stemma sync` sends the result."
    )?;

    Ok(())
}

/// Asks an agent for a reconciliation, which comes back as a proposal.
///
/// It runs here, on this machine, because this is the only machine that
/// has both versions: the local one was never sent anywhere.
pub fn reconcile(home: &Home, at: &Path, agent: &str, out: &mut impl Write) -> Result<()> {
    let credentials = crate::session::required(home)?;
    let server = crate::api::Server::at(&credentials.server)?;
    let clone = clone_at(home, at)?;
    git::fetch(&clone)?;

    let manifest = crate::build::read_manifest(&clone)?;
    let parted = between(&clone)?;
    let Some(line) = parted
        .iter()
        .find(|line| !line.divergences.is_empty())
        .or_else(|| parted.first())
    else {
        bail!("nothing here has parted")
    };

    let job = server.launch_job(
        &credentials.token,
        &crate::api::Asking {
            group: manifest.group.slug().as_str().to_owned(),
            side: manifest.side.as_str().to_owned(),
            line: line.line.as_str().to_owned(),
            commit: git::head(&clone)?,
            against: Some(git::upstream(&clone)?),
            task: "reconcile".to_owned(),
            agent: agent.to_owned(),
            machine: Some(crate::jobs::this_machine(&server, &credentials)?),
        },
    )?;

    writeln!(out, "{job}")?;
    writeln!(
        out,
        "  bring the two versions together · {} on {}",
        agent, line.line
    )?;
    writeln!(
        out,
        "  it runs here, because this is the machine that has both versions"
    )?;
    writeln!(
        out,
        "  what comes back is a proposal; accepting it makes the words yours, \
         and `stemma conflict resolve --keep here` then settles the line"
    )?;

    Ok(())
}

/// Every line whose two versions have parted.
pub fn between(clone: &Path) -> Result<Vec<Parted>> {
    let base = git::common(clone)?;
    let here = git::head(clone)?;
    let there = git::upstream(clone)?;

    let mut found = Vec::new();

    for name in lines_in(clone, &here, &there)? {
        let inside = format!("{}/{}/{}", repository::LINES, name, repository::ENTRY);
        let at = |commit: &str| git::show(clone, commit, &inside).unwrap_or_default();

        let (ours, theirs) = (at(&here), at(&there));

        // A line only one side has is not a disagreement: git can add or
        // remove a file without anybody deciding anything.
        if ours == theirs || ours.is_empty() || theirs.is_empty() {
            continue;
        }

        let was = at(&base);
        let divergences = reconcile::between(&was, &ours, &theirs)
            .with_context(|| format!("`{name}` cannot be compared"))?;

        found.push(Parted {
            line: name,
            base: was,
            here: ours,
            there: theirs,
            divergences,
        });
    }

    Ok(found)
}

/// Writes the decisions as one merge commit.
///
/// Git does the ordinary work — files one side touched, files that are
/// not mathematics — and every line source is written from the
/// decisions instead, because a statement that both machines wrote must
/// never be blended by a text merge.
pub fn settle(
    clone: &Path,
    decisions: &BTreeMap<String, BTreeMap<Mark, Keep>>,
) -> Result<Vec<Name>> {
    if git::is_dirty(clone)? {
        bail!("there is uncommitted work here: keep it first, and nothing will be lost")
    }
    if git::merging(clone) {
        bail!("a merge is already half done here: finish it with git, or `git merge --abort`")
    }

    let parted = between(clone)?;
    let mut merged: Vec<(Parted, String)> = Vec::new();

    for line in parted {
        let decided = decisions
            .get(line.line.as_str())
            .cloned()
            .unwrap_or_default();

        let source = reconcile::merge(&line.base, &line.here, &line.there, &decided)
            .with_context(|| format!("`{}` cannot be brought together", line.line))?;

        merged.push((line, source));
    }

    let unsettled = git::merge_without_committing(clone)?;
    let ours: Vec<String> = merged.iter().map(|(line, _)| line.inside()).collect();
    let left: Vec<String> = unsettled
        .into_iter()
        .filter(|path| !ours.contains(path))
        .collect();

    if !left.is_empty() {
        git::abandon_merge(clone)?;

        bail!(
            "these are not mathematics and this cannot speak for them: {}",
            left.join(", ")
        )
    }

    let mut written = Vec::new();

    for (line, source) in &merged {
        let path: PathBuf = clone.join(line.inside());

        std::fs::write(&path, source)
            .with_context(|| format!("cannot write {}", path.display()))?;
        git::settle(clone, &line.inside())?;
        written.push(line.line.clone());
    }

    git::commit_index(clone, &message(&written))?;

    Ok(written)
}

/// What the merge commit says it did.
fn message(lines: &[Name]) -> String {
    match lines.split_first() {
        None => "Bring this machine and the instance together".to_owned(),
        Some((first, [])) => format!("Bring two versions of {first} together"),
        Some(_) => format!(
            "Bring two versions of {} together",
            lines
                .iter()
                .map(Name::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Every line either side has.
fn lines_in(clone: &Path, here: &str, there: &str) -> Result<Vec<Name>> {
    let mut names: Vec<Name> = Vec::new();

    for commit in [here, there] {
        for path in git::files(clone, commit)? {
            let Some(rest) = path.strip_prefix(&format!("{}/", repository::LINES)) else {
                continue;
            };
            let Some((name, entry)) = rest.split_once('/') else {
                continue;
            };
            if entry != repository::ENTRY {
                continue;
            }
            let Ok(name) = Name::parse(name) else {
                continue;
            };

            if !names.contains(&name) {
                names.push(name);
            }
        }
    }

    names.sort_by(|left, right| left.as_str().cmp(right.as_str()));

    Ok(names)
}

/// The clone a command is about.
fn clone_at(home: &Home, at: &Path) -> Result<PathBuf> {
    let _ = home;

    git::containing(at).context("this is not inside a clone: run it from a group's repository")
}

/// One version of a statement, short enough to read in a terminal.
fn shown(version: Option<&str>) -> String {
    let Some(version) = version else {
        return "it is not there".to_owned();
    };

    let one_line: String = version.split_whitespace().collect::<Vec<_>>().join(" ");

    match one_line.char_indices().nth(140) {
        Some((at, _)) => format!("{}…", &one_line[..at]),
        None => one_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A line with one statement and a proof, which is what the two
    /// machines below both rewrite.
    const SOURCE: &str = "= Quotient groups\n\n        #theorem(id: \"thm:main\", central: true)[Every finite group is finite.]\n\n        #proof(of: \"thm:main\")[It is.]\n";

    /// Somewhere to put three repositories.
    fn scratch(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("stemma-conflict-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        root
    }

    /// Runs git, and says so loudly when it will not.
    fn git(at: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(at)
            .env("GIT_AUTHOR_NAME", "Ana")
            .env("GIT_AUTHOR_EMAIL", "ana@example.org")
            .env("GIT_COMMITTER_NAME", "Ana")
            .env("GIT_COMMITTER_EMAIL", "ana@example.org")
            .output()
            .expect("git");

        assert!(
            output.status.success(),
            "git {}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Writes the line's source in a clone.
    fn write(clone: &Path, source: &str) {
        let path = clone.join("lines/quotient-groups/main.typ");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }

    /// An instance with the line on it, and two machines that have it.
    fn two_machines(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = scratch(name);
        let instance = root.join("instance.git");

        git(
            &root,
            &["init", "--bare", "--initial-branch=main", "instance.git"],
        );

        let first = root.join("laptop");
        git(
            &root,
            &["clone", "--quiet", instance.to_str().unwrap(), "laptop"],
        );
        write(&first, SOURCE);
        git(&first, &["add", "."]);
        git(&first, &["commit", "--quiet", "-m", "A line"]);
        git(&first, &["push", "--quiet", "origin", "main"]);

        let second = root.join("desktop");
        git(
            &root,
            &["clone", "--quiet", instance.to_str().unwrap(), "desktop"],
        );

        (root, first, second)
    }

    #[test]
    fn a_statement_both_machines_wrote_is_a_disagreement_and_the_rest_is_not() {
        let (root, laptop, desktop) = two_machines("parted");

        write(&desktop, &SOURCE.replace("It is.", "By induction."));
        git(&desktop, &["commit", "--quiet", "-am", "By induction"]);
        git(&desktop, &["push", "--quiet", "origin", "main"]);

        write(&laptop, &SOURCE.replace("It is.", "By counting."));
        git(&laptop, &["commit", "--quiet", "-am", "By counting"]);
        git(&laptop, &["fetch", "--quiet", "origin"]);

        let parted = between(&laptop).unwrap();

        assert_eq!(parted.len(), 1);
        assert_eq!(parted[0].divergences.len(), 1);
        assert_eq!(
            parted[0].divergences[0].mark.to_string(),
            "the proof of thm:main"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_version_kept_is_written_and_the_other_stays_in_the_history() {
        let (root, laptop, desktop) = two_machines("settled");

        write(&desktop, &SOURCE.replace("It is.", "By induction."));
        git(&desktop, &["commit", "--quiet", "-am", "By induction"]);
        git(&desktop, &["push", "--quiet", "origin", "main"]);

        write(&laptop, &SOURCE.replace("It is.", "By counting."));
        git(&laptop, &["commit", "--quiet", "-am", "By counting"]);
        git(&laptop, &["fetch", "--quiet", "origin"]);

        let parted = between(&laptop).unwrap();
        let decisions = BTreeMap::from([(
            "quotient-groups".to_owned(),
            parted[0]
                .divergences
                .iter()
                .map(|divergence| (divergence.mark.clone(), Keep::There))
                .collect(),
        )]);

        let settled = settle(&laptop, &decisions).unwrap();

        assert_eq!(settled.len(), 1);

        let written =
            std::fs::read_to_string(laptop.join("lines/quotient-groups/main.typ")).unwrap();
        assert!(written.contains("By induction."));
        assert!(!written.contains("By counting."));

        // Both versions are still reachable: the merge has two parents,
        // and the one not kept is one of them.
        let parents = git(&laptop, &["rev-list", "--parents", "-n", "1", "HEAD"]);
        assert_eq!(parents.split_whitespace().count(), 3);
        assert!(git(&laptop, &["log", "--oneline"]).contains("By counting"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn nothing_is_written_while_a_statement_is_undecided() {
        let (root, laptop, desktop) = two_machines("undecided");

        write(&desktop, &SOURCE.replace("It is.", "By induction."));
        git(&desktop, &["commit", "--quiet", "-am", "By induction"]);
        git(&desktop, &["push", "--quiet", "origin", "main"]);

        write(&laptop, &SOURCE.replace("It is.", "By counting."));
        git(&laptop, &["commit", "--quiet", "-am", "By counting"]);
        git(&laptop, &["fetch", "--quiet", "origin"]);

        let refused = settle(&laptop, &BTreeMap::new()).unwrap_err();

        assert!(format!("{refused:#}").contains("thm:main"));
        assert!(!git::merging(&laptop), "a half-done merge was left behind");
        assert!(
            std::fs::read_to_string(laptop.join("lines/quotient-groups/main.typ"))
                .unwrap()
                .contains("By counting.")
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn work_that_is_not_committed_stops_this_before_it_starts() {
        let (root, laptop, desktop) = two_machines("dirty");

        write(&desktop, &SOURCE.replace("It is.", "By induction."));
        git(&desktop, &["commit", "--quiet", "-am", "By induction"]);
        git(&desktop, &["push", "--quiet", "origin", "main"]);

        write(&laptop, &SOURCE.replace("It is.", "By counting."));
        git(&laptop, &["commit", "--quiet", "-am", "By counting"]);
        git(&laptop, &["fetch", "--quiet", "origin"]);
        write(&laptop, &SOURCE.replace("It is.", "Half a thought"));

        let refused = settle(&laptop, &BTreeMap::new()).unwrap_err();

        assert!(format!("{refused:#}").contains("uncommitted"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn what_only_one_machine_touched_needs_nobody_to_decide_anything() {
        let (root, laptop, desktop) = two_machines("apart");

        write(
            &desktop,
            &SOURCE.replace("#proof", "#lemma(id: \"lem:step\")[A step.]\n\n#proof"),
        );
        git(&desktop, &["commit", "--quiet", "-am", "A step"]);
        git(&desktop, &["push", "--quiet", "origin", "main"]);

        write(&laptop, &SOURCE.replace("= Quotient groups", "= Quotients"));
        git(&laptop, &["commit", "--quiet", "-am", "A better title"]);
        git(&laptop, &["fetch", "--quiet", "origin"]);

        let parted = between(&laptop).unwrap();
        assert!(parted[0].divergences.is_empty());

        settle(&laptop, &BTreeMap::new()).unwrap();

        let written =
            std::fs::read_to_string(laptop.join("lines/quotient-groups/main.typ")).unwrap();

        assert!(written.contains("lem:step"), "what they wrote came over");
        assert!(
            written.contains("= Quotients"),
            "this machine keeps its prose"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }
}

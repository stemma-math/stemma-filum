//! Keeping this machine and the instance in step.
//!
//! What is here is a clone, and a clone is somebody's working copy: it
//! may be dirty, it may hold commits they made by hand, and none of that
//! is this program's to undo. So synchronising takes what the instance
//! has when nothing here is lost by it, sends what is here when the
//! instance has nothing newer, and where both have moved it says so and
//! leaves the two versions alone.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use stemma_core::line::Name;

use crate::api::{GroupRemote, Server};
use crate::config::Config;
use crate::credentials::Credentials;
use crate::git;
use crate::home::Home;
use crate::program;
use crate::workspace::Side;

/// What happened to one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It was not here, and now it is.
    Cloned,
    /// Both sides already had the same thing.
    Together,
    /// What the instance had came here.
    Took(u32),
    /// What was here went to the instance.
    Sent(u32),
    /// Both, in that order.
    TookAndSent(u32, u32),
    /// There is something here the instance has not, and nothing was
    /// done about it.
    ToSend(u32),
    /// There is something on the instance this machine has not, and
    /// nothing was done about it.
    ToTake(u32),
    /// Each side has something the other has not.
    Parted(u32, u32),
    /// Something stopped it, and the rest carried on.
    Stopped(String),
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cloned => write!(f, "cloned"),
            Self::Together => write!(f, "up to date"),
            Self::Took(commits) => write!(f, "took {}", commits_of(*commits)),
            Self::Sent(commits) => write!(f, "sent {}", commits_of(*commits)),
            Self::TookAndSent(took, sent) => {
                write!(f, "took {}, sent {}", commits_of(*took), commits_of(*sent))
            }
            Self::ToSend(commits) => write!(f, "{} to send", commits_of(*commits)),
            Self::ToTake(commits) => write!(f, "{} to take", commits_of(*commits)),
            Self::Parted(here, there) => write!(
                f,
                "this machine and the instance have both moved: {} here, {} there",
                commits_of(*here),
                commits_of(*there)
            ),
            Self::Stopped(reason) => write!(f, "{reason}"),
        }
    }
}

/// A count of commits, with the right noun.
fn commits_of(count: u32) -> String {
    match count {
        1 => "1 commit".to_owned(),
        many => format!("{many} commits"),
    }
}

/// One repository, after it was looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Which group it belongs to.
    pub group: String,
    /// Which of the group's two repositories it is.
    pub side: Side,
    /// Where it is on this machine.
    pub path: PathBuf,
    /// What happened.
    pub outcome: Outcome,
    /// Whether there is uncommitted work in it.
    pub dirty: bool,
    /// The lines that were committed before any of it, because nobody
    /// was writing them and they would otherwise have gone nowhere.
    pub recorded: Vec<String>,
}

/// Brings every group this machine can work in into step.
pub fn run(home: &Home, out: &mut impl Write) -> Result<Vec<Report>> {
    let credentials = crate::session::required(home)?;
    let server = Server::at(&credentials.server)?;
    let listing = server.workspace(&credentials.token)?;

    // Validate the complete listing before using remote names as local paths.
    for group in &listing.groups {
        stemma_core::id::Slug::parse(&group.slug)
            .context("the instance returned an invalid group name")?;
    }

    let mut config = Config::read(&home.config_file())?;
    let workspace = config.workspace_or(home.workspace());
    let mut reports = Vec::new();

    if listing.groups.is_empty() {
        writeln!(
            out,
            "You are not in any group yet, so there is nothing to bring down."
        )?;

        return Ok(reports);
    }

    // Which lines are being written on this machine right now. What is
    // uncommitted in one of those is somebody mid-sentence, and it is
    // committed when they finish rather than from under them.
    let held = held_here(&server, &credentials.token);

    for group in &listing.groups {
        let directory = config.group_directory(&group.slug, &workspace);

        for side in Side::both() {
            let report = one(group, side, &directory, &held);
            writeln!(out, "{}", line(&report))?;
            reports.push(report);
        }
    }

    // Where both sides moved, nothing here settles it and nothing here
    // hides it either: the resolver is a command away, and it speaks
    // about statements rather than about commits.
    if reports
        .iter()
        .any(|report| matches!(report.outcome, Outcome::Parted(_, _)))
    {
        writeln!(
            out,
            "\nWhere both sides moved, `stemma conflict list` says which statements \
             the two versions disagree about."
        )?;
    }

    // Recording where each group went means a group that was moved keeps
    // being found there next time.
    for group in &listing.groups {
        let directory = config.group_directory(&group.slug, &workspace);
        config.put_group(&group.slug, &directory, &workspace);
    }

    config.write(&home.config_file())?;

    Ok(reports)
}

/// Says what this machine has, without touching the instance.
pub fn status(home: &Home, out: &mut impl Write) -> Result<Vec<Report>> {
    let credentials = Credentials::read(&home.credentials_file())?;
    let config = Config::read(&home.config_file())?;
    let workspace = config.workspace_or(home.workspace());

    let mut reports = Vec::new();

    for (group, directory) in known_groups(&config, &workspace) {
        for side in Side::both() {
            let path = directory.join(side.directory());

            if !git::is_clone(&path) {
                continue;
            }

            let outcome = match git::state(&path) {
                Ok(state) => (state.dirty, describe(state.distance)),
                Err(error) => (false, Outcome::Stopped(format!("{error:#}"))),
            };

            let report = Report {
                group: group.clone(),
                side,
                path,
                dirty: outcome.0,
                outcome: outcome.1,
                recorded: Vec::new(),
            };

            writeln!(out, "{}", line(&report))?;
            reports.push(report);
        }
    }

    if reports.is_empty() {
        match credentials {
            Some(_) => writeln!(out, "Nothing has been brought down yet. Run `stemma sync`.")?,
            None => writeln!(out, "This machine is not signed in. Run `stemma login`.")?,
        }
    }

    Ok(reports)
}

/// Every group this machine has heard of, and where it is.
fn known_groups(config: &Config, workspace: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = config
        .groups
        .iter()
        .map(|(group, directory)| (group.clone(), directory.clone()))
        .collect();

    if let Ok(entries) = std::fs::read_dir(workspace) {
        for entry in entries.filter_map(Result::ok) {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };

            if entry.path().is_dir() && !found.iter().any(|(group, _)| group == &name) {
                found.push((name, entry.path()));
            }
        }
    }

    found.sort();
    found
}

/// Brings one repository into step.
fn one(group: &GroupRemote, side: Side, directory: &Path, held: &Held) -> Report {
    let path = directory.join(side.directory());
    let url = match side {
        Side::Shared => &group.shared,
        Side::Private => &group.personal,
    };

    // Before anything is counted or sent: work written and never
    // committed is invisible to everything below, and would sit here
    // while this said the machine was up to date.
    let recorded = record_loose(&path, group, side, held);

    let outcome = match work(url, &path) {
        Ok(outcome) => outcome,
        Err(error) => Outcome::Stopped(format!("{error:#}")),
    };

    let dirty = git::is_dirty(&path).unwrap_or(false);

    Report {
        group: group.slug.clone(),
        side,
        path,
        outcome,
        dirty,
        recorded,
    }
}

/// The lines this machine is writing, as the instance has them.
///
/// An instance that cannot be asked means nothing is treated as being
/// written, and the safe reading of that is the opposite: nothing is
/// committed from under anybody. So it is every line, and a machine
/// that cannot reach its instance commits nothing by itself.
fn held_here(server: &Server, token: &str) -> Held {
    match server.claims(token) {
        Ok(claims) => Held::These(
            claims
                .into_iter()
                .filter(|claim| claim.mine)
                .map(|claim| (claim.group, claim.side, claim.line))
                .collect(),
        ),
        Err(_) => Held::Unknown,
    }
}

/// What this machine knows about which of its lines are being written.
enum Held {
    /// The instance said, and these are they.
    These(Vec<(String, String, String)>),
    /// The instance could not be asked.
    Unknown,
}

/// Commits work nobody is writing, and says which lines those were.
fn record_loose(clone: &Path, group: &GroupRemote, side: Side, held: &Held) -> Vec<String> {
    let Held::These(held) = held else {
        return Vec::new();
    };

    if !git::is_clone(clone) {
        return Vec::new();
    }

    let here: Vec<Name> = held
        .iter()
        .filter(|(slug, which, _)| slug == &group.slug && which == side.directory())
        .filter_map(|(_, _, line)| Name::parse(line).ok())
        .collect();

    crate::writing::record_loose(clone, &here)
        .unwrap_or_default()
        .into_iter()
        .map(|recorded| recorded.line.as_str().to_owned())
        .collect()
}

/// The work itself.
fn work(url: &str, path: &Path) -> Result<Outcome> {
    let helper = helper()?;

    if !git::is_clone(path) {
        git::clone(url, path, &helper)?;
        return Ok(Outcome::Cloned);
    }

    git::keep_helper(path, &helper)?;
    git::fetch(path)?;

    let distance = git::distance(path)?;

    if distance.has_parted() {
        return Ok(Outcome::Parted(distance.ahead, distance.behind));
    }

    let took = distance.behind;
    let sent = distance.ahead;

    if took > 0 {
        git::fast_forward(path)?;
    }
    if sent > 0 {
        git::push(path)?;
    }

    Ok(match (took, sent) {
        (0, 0) => Outcome::Together,
        (took, 0) => Outcome::Took(took),
        (0, sent) => Outcome::Sent(sent),
        (took, sent) => Outcome::TookAndSent(took, sent),
    })
}

/// What a distance means when nothing was done about it.
///
/// Reading and acting are different, and so are their words: a commit
/// that is still here is one *to send*, not one that was sent.
fn describe(distance: git::Distance) -> Outcome {
    match (distance.ahead, distance.behind) {
        (0, 0) => Outcome::Together,
        (ahead, 0) => Outcome::ToSend(ahead),
        (0, behind) => Outcome::ToTake(behind),
        (ahead, behind) => Outcome::Parted(ahead, behind),
    }
}

/// How git is told to ask the command line for a credential.
///
/// Not whatever is running: the application is not a credential
/// helper, and git running it would open a window and then wait for an
/// answer that a window has no way to give.
fn helper() -> Result<String> {
    // Git reads a helper that is not an absolute path as a name to look
    // for under `git-credential-`, and a leading `!` as a shell command.
    // The second is the one that survives a path with a space in it,
    // which the application's own path may well have.
    let program = program::command_line()?;

    Ok(format!("!{} credential", quoted(&program)))
}

/// One path, as a shell word, whatever is in it.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

/// One line of what happened.
fn line(report: &Report) -> String {
    let dirty = match report.dirty {
        true => "  (uncommitted work here)",
        false => "",
    };

    format!(
        "{:<20} {:<8} {}{dirty}",
        report.group, report.side, report.outcome
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_happened_is_said_in_words_and_counted() {
        assert_eq!(Outcome::Cloned.to_string(), "cloned");
        assert_eq!(Outcome::Together.to_string(), "up to date");
        assert_eq!(Outcome::Took(1).to_string(), "took 1 commit");
        assert_eq!(Outcome::Sent(3).to_string(), "sent 3 commits");
        assert_eq!(
            Outcome::TookAndSent(1, 2).to_string(),
            "took 1 commit, sent 2 commits"
        );
    }

    #[test]
    fn two_sides_that_have_both_moved_are_described_and_not_settled() {
        let said = Outcome::Parted(2, 3).to_string();

        assert!(said.contains("both moved"));
        assert!(said.contains("2 commits here"));
        assert!(said.contains("3 commits there"));
    }

    #[test]
    fn a_path_with_a_space_in_it_is_still_one_word_to_a_shell() {
        assert_eq!(
            quoted(Path::new(
                "/Applications/Stemma 2.app/Contents/MacOS/stemma"
            )),
            "'/Applications/Stemma 2.app/Contents/MacOS/stemma'"
        );
    }

    #[test]
    fn reading_a_distance_does_not_claim_anything_was_done_about_it() {
        let apart = |ahead, behind| describe(git::Distance { ahead, behind });

        assert_eq!(apart(0, 0), Outcome::Together);
        assert_eq!(apart(2, 0), Outcome::ToSend(2));
        assert_eq!(apart(0, 2), Outcome::ToTake(2));
        assert_eq!(apart(1, 1), Outcome::Parted(1, 1));

        assert_eq!(Outcome::ToSend(1).to_string(), "1 commit to send");
        assert_eq!(Outcome::ToTake(2).to_string(), "2 commits to take");
    }

    #[test]
    fn a_line_says_the_group_the_side_and_what_happened() {
        let report = Report {
            group: "algebra".to_owned(),
            side: Side::Private,
            path: PathBuf::from("/home/ana/stemma/algebra/private"),
            outcome: Outcome::Cloned,
            dirty: true,
            recorded: Vec::new(),
        };

        let line = line(&report);

        assert!(line.starts_with("algebra"));
        assert!(line.contains("private"));
        assert!(line.contains("cloned"));
        assert!(line.contains("uncommitted work here"));
    }
}

//! The lines and groups on this machine, and changes to which lines exist.

use std::path::{Path, PathBuf};

use serde::Serialize;
use stemma::api::About;
use stemma::mirror::{Instance, Mirror, State, Which};
use stemma_core::document::Document;
use stemma_core::line::Name;
use stemma_core::repository::{self, Side};

use crate::engine::{self, Nudge};
use crate::{Session, carried_out, failed, home, said};

/// A line's `line.json`, checked to lie inside this machine's workspace.
///
/// The path is `<workspace>/<group>/<side>/lines/<name>/line.json`, and every
/// command that takes a path from the webview goes through [`LineAt::admit`].
pub struct LineAt {
    pub path: PathBuf,
    pub group: String,
    pub side: String,
    pub name: String,
}

impl LineAt {
    /// Canonicalises `given` and admits it only if it has the exact shape of
    /// a line inside the workspace.
    pub fn admit(given: &str) -> Result<Self, String> {
        let path = Path::new(given).canonicalize().map_err(said)?;
        let workspace = workspace()?.canonicalize().map_err(said)?;
        if !is_line_path(&workspace, &path) {
            return Err("that is not a line in this machine's workspace".to_owned());
        }
        Self::at(path)
    }

    fn at(path: PathBuf) -> Result<Self, String> {
        let component = |depth: usize, what: &str| {
            path.ancestors()
                .nth(depth)
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .ok_or_else(|| format!("the line has no {what}"))
        };
        Ok(Self {
            name: component(1, "name")?,
            side: component(3, "repository")?,
            group: component(4, "group")?,
            path,
        })
    }

    /// Which of the group's repositories it is in.
    pub fn side(&self) -> anyhow::Result<Side> {
        Ok(self.side.parse()?)
    }

    /// The line's own directory.
    pub fn directory(&self) -> &Path {
        self.path
            .parent()
            .expect("an admitted line has a directory")
    }

    /// The repository the line lives in.
    pub fn clone_root(&self) -> &Path {
        self.path
            .ancestors()
            .nth(3)
            .expect("an admitted line is inside a repository")
    }

    /// Which line it is, as the instance names it.
    pub fn about(&self) -> About {
        About {
            group: self.group.clone(),
            side: self.side.clone(),
            line: self.name.clone(),
        }
    }

    /// What the webview is told about which line this is.
    pub fn entry(&self) -> LineEntry {
        LineEntry {
            group: self.group.clone(),
            side: self.side.clone(),
            name: self.name.clone(),
            path: self.path.display().to_string(),
        }
    }
}

/// Only the exact workspace layout is admitted, after resolving symlinks.
fn is_line_path(workspace: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(workspace) else {
        return false;
    };
    let parts: Vec<_> = relative.iter().filter_map(|part| part.to_str()).collect();
    matches!(parts.as_slice(), [group, "private" | "shared", lines, name, entry]
        if !group.is_empty() && *lines == repository::LINES
            && Name::parse(name).is_ok() && *entry == repository::ENTRY)
}

/// Where this machine keeps its groups.
fn workspace() -> Result<PathBuf, String> {
    let home = home();
    let config = stemma::config::Config::read(&home.config_file()).map_err(failed)?;
    Ok(config.workspace_or(home.workspace()))
}

#[derive(Serialize)]
pub struct LineEntry {
    group: String,
    side: String,
    name: String,
    path: String,
}

/// One line as this machine has it.
#[derive(Serialize)]
pub struct Known {
    group: String,
    side: String,
    name: String,
    title: Option<String>,
    description: Option<String>,
    path: String,
    /// `sent`, `not_sent` or `needs_decision`.
    state: State,
}

/// Where every line that is not simply sent stands, and whether the
/// instance could be reached the last time it was tried.
#[tauri::command(async)]
pub fn standing() -> engine::Said {
    engine::last()
}

/// Sends every line with something new, now, rather than after the
/// pause: for when the window is about to close.
#[tauri::command(async)]
pub fn send_now() -> Result<(), String> {
    let home = home();
    let mirror = Mirror::at(&home).map_err(failed)?;
    let instance = Instance::connect(&home).map_err(failed)?;
    let report = mirror.send(&instance, &Which::All);
    engine::nudge(Nudge::Now);

    match report.stopped {
        Some(stopped) => Err(stopped),
        None => Ok(()),
    }
}

/// The groups on this machine, sorted.
#[tauri::command(async)]
pub fn groups() -> Result<Vec<String>, String> {
    let home = home();
    let config = stemma::config::Config::read(&home.config_file()).map_err(failed)?;
    let mut found: Vec<String> = config.groups.keys().cloned().collect();

    if let Ok(entries) = std::fs::read_dir(config.workspace_or(home.workspace())) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            // A hidden directory, such as `.git`, is never a group.
            if !name.starts_with('.') && entry.path().is_dir() && !found.contains(&name) {
                found.push(name);
            }
        }
    }

    found.sort();
    Ok(found)
}

/// The groups whose details this person may change, as the instance says.
///
/// Asked of the server rather than read from the machine, because a role
/// changes there and nowhere else.
#[tauri::command(async)]
pub fn managed_groups() -> Result<Vec<String>, String> {
    let session = Session::open()?;
    let workspace = session.server.workspace(&session.token).map_err(failed)?;

    Ok(workspace
        .groups
        .into_iter()
        .filter(|group| manages(&group.role))
        .map(|group| group.slug)
        .collect())
}

/// Whether a role, as the instance spells it, may change a group's details.
fn manages(role: &str) -> bool {
    role.parse::<stemma_core::access::GroupRole>()
        .is_ok_and(stemma_core::access::GroupRole::may_manage)
}

/// Every line on this machine, with the title and description its source
/// declares and where it stands.
#[tauri::command(async)]
pub fn lines() -> Result<Vec<Known>, String> {
    let mirror = Mirror::at(&home()).map_err(failed)?;
    let records = mirror.records().map_err(failed)?;
    let standing = mirror.standing().map_err(failed)?;

    let mut found: Vec<Known> = records
        .iter()
        .map(|record| {
            let document = Document::parse(&record.source).ok();
            Known {
                group: record.group.clone(),
                side: record.side.as_str().to_owned(),
                name: record.name.clone(),
                title: document
                    .as_ref()
                    .map(|document| document.title.clone())
                    .filter(|title| !title.trim().is_empty()),
                description: document.and_then(|document| document.description),
                path: mirror.entry(record).display().to_string(),
                state: standing
                    .iter()
                    .find(|line| line.id == record.id)
                    .map_or(State::Sent, |line| line.state),
            }
        })
        .collect();

    found.sort_by(|a, b| (&a.group, &a.side, &a.name).cmp(&(&b.group, &b.side, &b.name)));
    Ok(found)
}

#[tauri::command(async)]
pub fn new_line(
    group: String,
    title: String,
    description: Option<String>,
    shared: bool,
) -> Result<String, String> {
    carried_out(|home, out| {
        stemma::lines::start(home, &group, &title, description.as_deref(), shared, out)
    })
}

/// Removes one of this person's private lines; its history is kept.
#[tauri::command(async)]
pub fn delete_line(path: String) -> Result<String, String> {
    let line = LineAt::admit(&path)?;
    carried_out(|home, out| stemma::lines::delete(home, line.directory(), None, out))
}

/// Moves a private line into the group's shared repository.
#[tauri::command(async)]
pub fn share_line(path: String, history: bool, mine_to_write: bool) -> Result<String, String> {
    let line = LineAt::admit(&path)?;
    carried_out(|home, out| {
        stemma::lines::share(home, line.directory(), None, history, mine_to_write, out)
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{LineAt, is_line_path, manages};

    #[test]
    fn only_actual_workspace_lines_are_admitted() {
        let workspace = Path::new("/workspace");
        assert!(is_line_path(
            workspace,
            Path::new("/workspace/algebra/private/lines/quotients/line.json")
        ));
        for path in [
            "/workspace/line.json",
            "/workspace/algebra/private/other/quotients/line.json",
            "/workspace/algebra/arbitrary/lines/quotients/line.json",
            "/workspace/algebra/private/lines/quotients/nested/line.json",
            "/elsewhere/algebra/private/lines/quotients/line.json",
        ] {
            assert!(!is_line_path(workspace, Path::new(path)), "{path}");
        }
    }

    #[test]
    fn a_line_identity_comes_from_its_repository_path() {
        let line = LineAt::at(PathBuf::from(
            "/workspace/algebra/private/lines/quotients/line.json",
        ))
        .unwrap();

        assert_eq!(
            (line.group.as_str(), line.side.as_str(), line.name.as_str()),
            ("algebra", "private", "quotients")
        );
        assert_eq!(
            line.directory(),
            Path::new("/workspace/algebra/private/lines/quotients")
        );
        assert_eq!(line.clone_root(), Path::new("/workspace/algebra/private"));
    }

    #[test]
    fn only_admins_and_owners_manage_a_group() {
        assert!(manages("owner"));
        assert!(manages("admin"));
        assert!(!manages("member"));
        assert!(!manages("anything else"));
    }
}

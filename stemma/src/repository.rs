//! Where a person's repositories live on their own machine, and finding
//! one from a path.
//!
//! Somewhere visible, one directory per group, and inside each of those
//! the two repositories that group gives them: what the group shares, and
//! what is theirs alone. The workspace is where new repositories land,
//! not a prison — the configuration maps a group to wherever its
//! directory actually is, and commands find their repository by walking
//! up to its manifest.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use stemma_core::line::Name;
use stemma_core::repository::{self, Manifest, Pins, Side};

/// Where a group's repositories go inside a workspace.
pub fn group_directory(workspace: &Path, group: &str) -> PathBuf {
    workspace.join(group)
}

/// Reads the file at the root that says what the repository is.
pub fn read_manifest(clone: &Path) -> Result<Manifest> {
    let path = clone.join(repository::MANIFEST);
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "{} has no {}, so it is not a Stemma repository",
            clone.display(),
            repository::MANIFEST
        )
    })?;

    Manifest::parse(&text).with_context(|| format!("cannot use {}", path.display()))
}

/// What the group has pinned for Typst, wherever it is written down.
///
/// See [`group_pins`].
pub fn group_pin(clone: &Path, manifest: &Manifest) -> Option<String> {
    group_pins(clone, manifest).typst
}

/// Every compiler pin belonging to the group.
///
/// The pins belong to the group, not to a repository: a private line is
/// written against the same environment as the group's own work, or the
/// two would not compose to the same thing. So a private repository with
/// no pin of its own takes the group's, from the shared one beside it.
pub fn group_pins(clone: &Path, manifest: &Manifest) -> Pins {
    let mut pins = manifest.pins.clone();

    if manifest.side != Side::Private
        || (pins.typst.is_some()
            && pins.lean.is_some()
            && pins.mathlib.is_some()
            && pins.stemma.is_some())
    {
        return pins;
    }

    let Some(parent) = clone.parent() else {
        return pins;
    };
    let Ok(shared) = read_manifest(&parent.join(Side::Shared.as_str())) else {
        return pins;
    };

    if shared.group == manifest.group {
        pins.typst = pins.typst.or(shared.pins.typst);
        pins.lean = pins.lean.or(shared.pins.lean);
        pins.mathlib = pins.mathlib.or(shared.pins.mathlib);
        pins.stemma = pins.stemma.or(shared.pins.stemma);
        if pins.dependencies.is_empty() {
            pins.dependencies = shared.pins.dependencies;
        }
    }
    pins
}

/// The lines in a repository, in the order their directories sort.
pub fn lines_in(clone: &Path) -> Result<Vec<Name>> {
    let directory = clone.join(repository::LINES);

    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(Vec::new());
    };

    let mut names: Vec<Name> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(repository::ENTRY).is_file())
        .filter_map(|entry| Name::parse(entry.file_name().to_str()?).ok())
        .collect();

    names.sort_by(|one, other| one.as_str().cmp(other.as_str()));

    Ok(names)
}

/// The repository a path is inside, found by its manifest.
pub fn repository_of(path: &Path) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;

    path.ancestors()
        .find(|directory| directory.join(repository::MANIFEST).is_file())
        .map(Path::to_path_buf)
}

/// The group, repository and line a command is about: the line named,
/// or the one the path is inside.
pub fn line_at(at: &Path, named: Option<&str>) -> Result<(String, Side, Name)> {
    let root = repository_of(at)
        .context("this is not inside a group's repository: run it from one of its lines")?;
    let manifest = read_manifest(&root)?;
    let name = line_of(&root, at, named)?;

    Ok((
        manifest.group.slug().as_str().to_owned(),
        manifest.side,
        name,
    ))
}

/// The line a command is about.
///
/// Run from inside one, it is that one. Run from anywhere else in the
/// repository, it has to be named: guessing which of a group's lines
/// somebody meant is the kind of help that starts work on the wrong
/// thing.
pub fn line_of(clone: &Path, at: &Path, named: Option<&str>) -> Result<Name> {
    if let Some(named) = named {
        return Name::parse(named.trim()).context("that is not the name of a line");
    }

    if let Some(found) = inside(clone, at) {
        return Ok(found);
    }

    bail!(
        "say which line this is about, with --line: {}",
        available(clone)
    )
}

/// The line a path is inside, if it is inside one.
fn inside(clone: &Path, at: &Path) -> Option<Name> {
    let inside = at.canonicalize().ok()?;
    let clone = clone.canonicalize().ok()?;
    let mut parts = inside.strip_prefix(&clone).ok()?.components();

    if parts.next()?.as_os_str() != repository::LINES {
        return None;
    }

    Name::parse(parts.next()?.as_os_str().to_str()?).ok()
}

/// The lines there are, for a message that helps rather than scolds.
fn available(clone: &Path) -> String {
    let found = lines_in(clone).unwrap_or_default();
    let names: Vec<&str> = found.iter().map(Name::as_str).collect();

    match names.is_empty() {
        true => "this repository has no lines yet".to_owned(),
        false => names.join(", "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "stemma-repository-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        root
    }

    #[test]
    fn a_group_gets_a_directory_of_its_own_in_the_workspace() {
        assert_eq!(
            group_directory(Path::new("/home/ana/stemma"), "algebra"),
            Path::new("/home/ana/stemma/algebra")
        );
    }

    #[test]
    fn a_command_run_inside_a_line_is_about_that_line() {
        let clone = somewhere("inside");
        let line = clone.join("lines/quotient-groups");
        std::fs::create_dir_all(line.join("figures")).unwrap();

        assert_eq!(
            inside(&clone, &line).map(|name| name.as_str().to_owned()),
            Some("quotient-groups".to_owned())
        );
        assert_eq!(
            inside(&clone, &line.join("figures")).map(|name| name.as_str().to_owned()),
            Some("quotient-groups".to_owned()),
            "somewhere under a line is still that line"
        );
        assert_eq!(inside(&clone, &clone), None);

        std::fs::remove_dir_all(&clone).unwrap();
    }

    #[test]
    fn a_command_run_anywhere_else_asks_which_line_and_says_what_there_is() {
        let clone = somewhere("outside");
        std::fs::create_dir_all(clone.join("lines/quotient-groups")).unwrap();
        std::fs::write(clone.join("lines/quotient-groups/line.json"), "").unwrap();

        let error = line_of(&clone, &clone, None).unwrap_err();

        assert!(error.to_string().contains("--line"));
        assert!(error.to_string().contains("quotient-groups"));

        std::fs::remove_dir_all(&clone).unwrap();
    }

    #[test]
    fn a_line_named_by_hand_is_taken_as_named() {
        let clone = somewhere("named");

        assert_eq!(
            line_of(&clone, &clone, Some(" quotient-groups "))
                .unwrap()
                .as_str(),
            "quotient-groups"
        );
        assert!(line_of(&clone, &clone, Some("Not A Line")).is_err());
    }

    #[test]
    fn a_private_repository_takes_the_group_pin_from_the_shared_one_beside_it() {
        let root = somewhere("pin");

        let group = "grp:algebra".parse().unwrap();
        let shared = root.join("algebra/shared");
        let private = root.join("algebra/private");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::create_dir_all(&private).unwrap();

        let mut theirs = Manifest::new(group, Side::Shared, "Algebra");
        theirs.pins.typst = Some("0.13.1".to_owned());
        std::fs::write(shared.join(repository::MANIFEST), theirs.to_text()).unwrap();

        let mine = Manifest::new("grp:algebra".parse().unwrap(), Side::Private, "Mine");
        assert_eq!(group_pin(&private, &mine).as_deref(), Some("0.13.1"));

        // A group that has pinned nothing pins nothing either way.
        let others = Manifest::new("grp:topology".parse().unwrap(), Side::Private, "Mine");
        assert_eq!(group_pin(&private, &others), None);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_repository_that_is_not_one_says_so() {
        let error = read_manifest(Path::new("/nonexistent/clone")).unwrap_err();

        assert!(error.to_string().contains("not a Stemma repository"));
    }
}

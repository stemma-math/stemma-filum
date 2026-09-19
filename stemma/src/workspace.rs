//! Where a person's repositories live on their own machine.
//!
//! Somewhere visible, one directory per group, and inside each of those
//! the two repositories that group gives them: what the group shares, and
//! what is theirs alone. The workspace is where new clones land, not a
//! prison — the configuration maps a group to wherever its clone actually
//! is, and commands find their repository by walking up.

use std::path::{Path, PathBuf};

/// What a group's directory holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// What the whole group has.
    Shared,
    /// What is this person's alone, inside this group.
    Private,
}

impl Side {
    /// The directory it lands in.
    pub fn directory(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            // Not "personal": the word on the directory should say what
            // it means for the person reading it, which is that nobody
            // else can see inside.
            Self::Private => "private",
        }
    }

    /// Both of them, in the order they are shown.
    pub fn both() -> [Self; 2] {
        [Self::Shared, Self::Private]
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.directory())
    }
}

/// Where a group's clones go.
pub fn group_directory(workspace: &Path, group: &str) -> PathBuf {
    workspace.join(group)
}

/// Where one of a group's two clones goes.
pub fn clone_directory(workspace: &Path, group: &str, side: Side) -> PathBuf {
    group_directory(workspace, group).join(side.directory())
}

/// Which group and side a path belongs to, if it belongs to one.
pub fn locate(workspace: &Path, path: &Path) -> Option<(String, Side)> {
    let inside = path.strip_prefix(workspace).ok()?;
    let mut parts = inside.components();

    let group = parts.next()?.as_os_str().to_str()?.to_owned();
    let side = match parts.next()?.as_os_str().to_str()? {
        "shared" => Side::Shared,
        "private" => Side::Private,
        _ => return None,
    };

    Some((group, side))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_gets_a_directory_with_both_of_its_repositories_in_it() {
        let workspace = Path::new("/home/ana/stemma");

        assert_eq!(
            clone_directory(workspace, "algebra", Side::Shared),
            Path::new("/home/ana/stemma/algebra/shared")
        );
        assert_eq!(
            clone_directory(workspace, "algebra", Side::Private),
            Path::new("/home/ana/stemma/algebra/private")
        );
    }

    #[test]
    fn a_path_says_which_group_and_which_side_it_is() {
        let workspace = Path::new("/home/ana/stemma");

        assert_eq!(
            locate(workspace, Path::new("/home/ana/stemma/algebra/shared")),
            Some(("algebra".to_owned(), Side::Shared))
        );
        assert_eq!(
            locate(
                workspace,
                Path::new("/home/ana/stemma/algebra/private/sections/two")
            ),
            Some(("algebra".to_owned(), Side::Private))
        );
    }

    #[test]
    fn a_path_outside_the_workspace_belongs_to_no_group() {
        let workspace = Path::new("/home/ana/stemma");

        assert_eq!(locate(workspace, Path::new("/home/ana/elsewhere")), None);
        assert_eq!(locate(workspace, Path::new("/home/ana/stemma")), None);
        assert_eq!(
            locate(workspace, Path::new("/home/ana/stemma/algebra/notes")),
            None
        );
    }
}

//! Where build outputs are kept.
//!
//! Outside the repositories, named by the commit they were built from,
//! and thrown away freely: everything here can be built again by whoever
//! has the commit. Git stays the source of truth, and this is a shelf.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use stemma_core::artefact::{Kind, Scope};
use stemma_core::id::GroupId;

/// The most an artefact may weigh.
pub const LARGEST: usize = 32 * 1024 * 1024;

/// Where an instance keeps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    root: PathBuf,
}

impl Shelf {
    /// The shelf under this directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Where one artefact is.
    pub fn path(&self, group: &GroupId, scope: &Scope, kind: Kind, commit: &str) -> PathBuf {
        self.root
            .join(group.slug().as_str())
            .join(scope.to_string())
            .join(kind.as_str())
            .join(format!("{commit}.json"))
    }

    /// Puts one on the shelf, replacing what was there for that commit.
    pub fn put(
        &self,
        group: &GroupId,
        scope: &Scope,
        kind: Kind,
        commit: &str,
        content: &[u8],
    ) -> Result<PathBuf> {
        let path = self.path(group, scope, kind, commit);
        let directory = path.parent().context("an artefact needs somewhere to go")?;

        std::fs::create_dir_all(directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
        std::fs::write(&path, content)
            .with_context(|| format!("cannot write {}", path.display()))?;

        Ok(path)
    }

    /// Reads one off the shelf.
    pub fn get(
        &self,
        group: &GroupId,
        scope: &Scope,
        kind: Kind,
        commit: &str,
    ) -> Result<Option<Vec<u8>>> {
        let path = self.path(group, scope, kind, commit);

        match std::fs::read(&path) {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    /// Takes one off the shelf.
    ///
    /// Nothing is lost by it: whoever has the commit can build it again.
    pub fn evict(&self, group: &GroupId, scope: &Scope, kind: Kind, commit: &str) -> Result<bool> {
        let path = self.path(group, scope, kind, commit);

        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error).with_context(|| format!("cannot remove {}", path.display())),
        }
    }

    /// The directory everything sits under.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> GroupId {
        "grp:algebra".parse().unwrap()
    }

    #[test]
    fn an_artefact_is_named_by_what_it_was_built_from() {
        let shelf = Shelf::new("/var/lib/stemma/artefacts");
        let ana = "usr:ana".parse().unwrap();

        assert_eq!(
            shelf.path(&group(), &Scope::Shared, Kind::Nodes, "abc1234"),
            Path::new("/var/lib/stemma/artefacts/algebra/shared/nodes/abc1234.json")
        );
        assert_eq!(
            shelf.path(&group(), &Scope::Personal(ana), Kind::Nodes, "abc1234"),
            Path::new("/var/lib/stemma/artefacts/algebra/personal/ana/nodes/abc1234.json")
        );
    }

    #[test]
    fn what_is_put_on_the_shelf_is_what_comes_off_it() {
        let root = std::env::temp_dir().join(format!("stemma-shelf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let shelf = Shelf::new(&root);

        assert_eq!(
            shelf
                .get(&group(), &Scope::Shared, Kind::Nodes, "abc1234")
                .unwrap(),
            None
        );

        shelf
            .put(&group(), &Scope::Shared, Kind::Nodes, "abc1234", b"{}")
            .unwrap();

        assert_eq!(
            shelf
                .get(&group(), &Scope::Shared, Kind::Nodes, "abc1234")
                .unwrap(),
            Some(b"{}".to_vec())
        );

        // Building the same commit again replaces what was there.
        shelf
            .put(
                &group(),
                &Scope::Shared,
                Kind::Nodes,
                "abc1234",
                b"{\"a\":1}",
            )
            .unwrap();
        assert_eq!(
            shelf
                .get(&group(), &Scope::Shared, Kind::Nodes, "abc1234")
                .unwrap(),
            Some(b"{\"a\":1}".to_vec())
        );

        assert!(
            shelf
                .evict(&group(), &Scope::Shared, Kind::Nodes, "abc1234")
                .unwrap()
        );
        assert!(
            !shelf
                .evict(&group(), &Scope::Shared, Kind::Nodes, "abc1234")
                .unwrap()
        );

        std::fs::remove_dir_all(&root).unwrap();
    }
}

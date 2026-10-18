//! The substrate: node files, headers, bodies, links, ids, the closed kind set, the index, search, the refusal. The
//! graph, the lock, the proposed tree and the commit land with their milestones.

mod body;
mod closed;
mod file;
mod header;
mod id;
mod index;
mod kind;
mod refusal;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub use body::{Body, Check, Section};
pub use closed::OutsideSet;
pub use file::File;
pub use header::{Header, LINKS, RESERVED, Unresolved};
pub use id::Id;
pub use index::{Filter, Hit, Index, Row};
pub use kind::Kind;
pub use refusal::{Code, Refusal};
// The macro's expansion names serde through this crate, so a domain crate needs no serde dependency of its own.
#[doc(hidden)]
pub use serde;

/// A vault root: the directory that holds `projects/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vault {
    /// The root directory.
    pub root: PathBuf,
}

impl Vault {
    /// Finds the vault that holds `from`: the nearest ancestor (or `from` itself) with a `projects/` directory.
    ///
    /// Refuses `no_vault` when no ancestor qualifies. `BP_VAULT` overrides the search.
    pub fn discover(from: &Path) -> Result<Self, Refusal> {
        if let Some(root) = std::env::var_os("BP_VAULT") {
            let root = PathBuf::from(root);
            if root.join("projects").is_dir() {
                return Ok(Self { root });
            }
            return Err(Refusal::new(
                Code::NoVault,
                root.display().to_string(),
                "BP_VAULT names a directory that holds projects/",
                "unset BP_VAULT, or point it at the vault root",
            ));
        }
        for dir in from.ancestors() {
            if dir.join("projects").is_dir() {
                return Ok(Self { root: dir.to_path_buf() });
            }
        }
        Err(Refusal::new(
            Code::NoVault,
            from.display().to_string(),
            "a verb runs inside a vault: a directory that holds projects/",
            "cd into the vault, or set BP_VAULT",
        ))
    }

    /// Reads and parses one file by id. Refuses `no_node` when no file has that id.
    pub fn read(&self, id: &Id) -> Result<File, Refusal> {
        let bytes = std::fs::read(id.path(&self.root)).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Refusal::new(
                Code::NoNode,
                id.as_str(),
                "an id names a markdown file under the vault root",
                format!("bp search {}", id.as_str().rsplit('/').next().unwrap_or_default()),
            ),
            _ => Refusal::failed(id.as_str(), e.to_string()),
        })?;
        File::from_bytes(id.clone(), &bytes)
    }

    /// Every markdown file under `projects/` and `shared/`, sorted by id. Dot directories (`.bp`, `.git`) are skipped.
    /// Refuses `schema_invalid` on a markdown file whose path is not an id: it would be neither read nor counted.
    pub fn ids(&self) -> Result<Vec<Id>, Refusal> {
        let mut ids = Vec::new();
        for top in ["projects", "shared"] {
            let dir = self.root.join(top);
            if dir.is_dir() {
                collect(&self.root, &dir, &mut ids)?;
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// The project `path` sits under, when it is inside `projects/<project>/`.
    pub fn project_of(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.root).ok()?;
        let mut parts = rel.components();
        match (parts.next(), parts.next()) {
            (Some(p), Some(name)) if p.as_os_str() == "projects" => {
                Some(name.as_os_str().to_string_lossy().into_owned())
            }
            _ => None,
        }
    }
}

fn collect(root: &Path, dir: &Path, ids: &mut Vec<Id>) -> Result<(), Refusal> {
    let io = |e: std::io::Error| Refusal::failed(dir.display().to_string(), e.to_string());
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        let hidden = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, ids)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            let id = Id::of_path(root, &path).ok_or_else(|| {
                let rel = path.strip_prefix(root).unwrap_or(&path).display().to_string();
                Refusal::schema(rel, "the path is not an id: projects/<project>/<kind>/<slug> or shared/<kind>/<slug>")
            })?;
            ids.push(id);
        }
    }
    Ok(())
}

/// The result of `list --count-only`.
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    /// Every markdown file scanned.
    pub files: usize,
    /// Files without a header.
    pub no_header: usize,
    /// Nodes per kind.
    pub nodes: BTreeMap<String, usize>,
    /// Records per kind.
    pub records: BTreeMap<String, usize>,
    /// Fenced `check` blocks across every body.
    pub check_blocks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_the_nearest_ancestor_with_projects() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("vault");
        std::fs::create_dir_all(root.join("projects/bp/issues")).unwrap();
        let v = Vault::discover(&root.join("projects/bp/issues")).unwrap();
        assert_eq!(v.root, root);
        assert_eq!(v.project_of(&root.join("projects/bp/issues")), Some("bp".into()));
        assert_eq!(v.project_of(&root), None);
    }

    #[test]
    fn refuses_outside_a_vault() {
        let dir = tempfile::tempdir().unwrap();
        let err = Vault::discover(dir.path()).unwrap_err();
        assert_eq!(err.code, Code::NoVault);
        assert_eq!(err.exit, 2);
    }
}

//! The substrate: node files, headers, bodies, links, ids, the closed kind set, the index, search, the refusal. The
//! graph, the lock, the proposed tree and the commit land with their milestones.

mod body;
mod closed;
mod file;
pub mod git;
mod header;
mod id;
mod index;
mod kind;
mod refusal;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub use body::{Body, Check, Finding, FindingKind, Section};
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
    /// Finds the vault that holds `from`: the nearest ancestor (or `from` itself) with `projects/` and `shared/`.
    ///
    /// Refuses `no_vault` when no ancestor qualifies. `BP_VAULT` overrides the search.
    pub fn discover(from: &Path) -> Result<Self, Refusal> {
        if let Some(root) = std::env::var_os("BP_VAULT") {
            // The canonical root, so a working directory under it matches however BP_VAULT spells the path.
            let root = PathBuf::from(&root).canonicalize().unwrap_or_else(|_| PathBuf::from(root));
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
        // Both top directories: a home directory with a `projects/` of code checkouts is not a vault.
        for dir in from.ancestors() {
            if dir.join("projects").is_dir() && dir.join("shared").is_dir() {
                return Ok(Self { root: dir.to_path_buf() });
            }
        }
        Err(Refusal::new(
            Code::NoVault,
            from.display().to_string(),
            "a verb runs inside a vault: a directory that holds projects/ and shared/",
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

    /// Reads one file as it was at a commit, branch or blueprint tag. Refuses `unknown_value` when git does not know
    /// the ref and `no_node` when the file did not exist there. It branches on git's exit codes, never its text, so
    /// the locale cannot change the refusal; `--end-of-options` keeps a ref that starts with `-` from being an option.
    pub fn read_at(&self, id: &Id, at: &str) -> Result<File, Refusal> {
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&self.root)
                .env("LC_ALL", "C")
                .args(args)
                .output()
                .map_err(|e| Refusal::failed("git", format!("{e}; install git, or drop --at")))
        };
        let unknown = || {
            Refusal::new(
                Code::UnknownValue,
                format!("--at {at}"),
                "--at names a commit, branch or tag of the vault repository",
                format!("git -C {} tag --list 'blueprint/*'", self.root.display()),
            )
        };
        // An empty ref would read git's staging area, which is no commit.
        if at.is_empty() {
            return Err(unknown());
        }
        let commit = git(&["rev-parse", "--verify", "--quiet", "--end-of-options", &format!("{at}^{{commit}}")])?;
        match commit.status.code() {
            Some(0) => {}
            Some(1) => return Err(unknown()),
            _ => {
                return Err(Refusal::failed(
                    format!("--at {at}"),
                    format!("{} is not a git repository git can read; drop --at", self.root.display()),
                ));
            }
        }
        let sha = String::from_utf8_lossy(&commit.stdout).trim().to_string();
        let blob = git(&["cat-file", "blob", &format!("{sha}:./{id}.md")])?;
        if !blob.status.success() {
            return Err(Refusal::new(
                Code::NoNode,
                format!("{id}@{at}"),
                "an id names a markdown file under the vault root at that commit",
                format!("git -C {} log --oneline -- {id}.md", self.root.display()),
            ));
        }
        File::from_bytes(id.clone(), &blob.stdout)
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

    /// The project for a working directory: the one whose directory holds it, else the one whose `repository` is
    /// the directory's remote (15, `where`). Refuses `schema_invalid` when two projects name that repository.
    pub fn project_for(&self, cwd: &Path) -> Result<Option<String>, Refusal> {
        if let Some(p) = self.project_of(cwd) {
            return Ok(Some(p));
        }
        let Some(remote) = git::Repo::find(cwd).and_then(|r| r.remote()) else { return Ok(None) };
        let Ok(entries) = std::fs::read_dir(self.root.join("projects")) else { return Ok(None) };
        let mut names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        let matching: Vec<String> = names
            .into_iter()
            .filter(|name| {
                let Ok(file) = self.read(&Id::from_trusted(format!("projects/{name}/project"))) else { return false };
                let repository =
                    file.header.as_ref().and_then(|h| h.fields.get("repository")?.as_str().map(str::to_string));
                repository.is_some_and(|r| git::same_repository(&r, &remote))
            })
            .collect();
        match &matching[..] {
            [] => Ok(None),
            [one] => Ok(Some(one.clone())),
            _ => Err(Refusal::new(
                Code::SchemaInvalid,
                remote,
                "one project names a repository (12)",
                format!("set a distinct `repository` on one of: {}", matching.join(", ")),
            )),
        }
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
    fn discovers_the_nearest_ancestor_with_projects_and_shared() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("vault");
        std::fs::create_dir_all(root.join("projects/bp/issues")).unwrap();
        std::fs::create_dir_all(root.join("shared")).unwrap();
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

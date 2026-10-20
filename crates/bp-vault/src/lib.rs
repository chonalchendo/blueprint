//! The substrate: node files, headers, bodies, links, ids, the closed kind set, the index, search, the refusal, and
//! the write path: the lock, the proposed tree and the commit.

mod body;
mod closed;
mod emit;
mod file;
mod fingerprint;
pub mod git;
mod header;
mod id;
mod index;
mod kind;
mod propose;
mod refusal;
mod rename;
mod walk;
mod write;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub use body::{Body, Check, Finding, FindingKind, Section};
pub use closed::OutsideSet;
pub use emit::set_field;
pub use file::{Broken, DeltaState, File, header_end, normalise, opens_with_header, written};
pub use header::{Header, LINKS, RESERVED, Unresolved};
pub use id::{Id, SLUG_MAX, is_slug, slug};
pub use index::{Fault, Filter, Hit, Index, Reading, Record, Row};
pub use kind::{DesignLevel, Kind};
pub use refusal::{Code, Refusal};
pub use rename::retarget;
pub use walk::{Fill, Gap, ISSUE_CONTEXT, Reached, Requires, fill};
pub use write::{Held, Moves, Writer, Written};
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
        Self::search(from)
    }

    /// The ancestor search alone, so a test reads the same rule whatever `BP_VAULT` its runner set.
    fn search(from: &Path) -> Result<Self, Refusal> {
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
        File::from_bytes(id.clone(), self.text(id)?.as_bytes())
    }

    /// The `schema_invalid` refusal, with the by-hand fix, when the file at `id` is there but does not parse. A verb
    /// that finds no node at `id` asks before it refuses, so an agent is not sent to search for a file that exists
    /// (25, issue 8). It reads the file, so it runs only on a refusal path.
    pub fn unparsed(&self, id: &Id) -> Option<Refusal> {
        self.read(id).err().filter(|e| e.code == Code::SchemaInvalid)
    }

    /// One file's text as it is on disk, for a writer that changes some lines and keeps the rest. Refuses `no_node`
    /// when no file has that id, and `schema_invalid` when the file is not UTF-8.
    pub fn text(&self, id: &Id) -> Result<String, Refusal> {
        let bytes = std::fs::read(id.path(&self.root)).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Refusal::new(
                Code::NoNode,
                id.as_str(),
                "an id names a markdown file under the vault root",
                format!("bp search {}", id.as_str().rsplit('/').next().unwrap_or_default()),
            ),
            _ => Refusal::failed(id.as_str(), e.to_string()),
        })?;
        String::from_utf8(bytes).map_err(|e| Refusal::schema(id.as_str(), &Broken::unread(format!("not UTF-8: {e}"))))
    }

    /// Reads one file as it was at a commit, branch or blueprint tag. Refuses `unknown_value` when git does not know
    /// the ref and `no_node` when the file did not exist there. It branches on git's exit codes, never its text, so
    /// the locale cannot change the refusal; `--end-of-options` keeps a ref that starts with `-` from being an option.
    /// It parses the past file under today's schema: a header an older schema allowed refuses (no migration pre-v0.1).
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

    /// The commit time of the last commit that touched one file, Unix seconds: its processing time (13). `None`
    /// when git cannot say, as for a file never committed or a vault outside git.
    pub fn last_commit(&self, id: &Id) -> Option<i64> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .env("LC_ALL", "C")
            .args(["log", "-1", "--format=%ct", "--", &format!("./{id}.md")])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    /// Every markdown file under `projects/` and `shared/`: the ids, sorted, and the paths that are not ids, without
    /// `.md` (a stray `notes.md`), which the index holds as broken files. Dot directories (`.bp`, `.git`) are skipped.
    pub fn ids(&self) -> Result<(Vec<Id>, Vec<String>), Refusal> {
        let (mut ids, mut strays) = (Vec::new(), Vec::new());
        for top in ["projects", "shared"] {
            let dir = self.root.join(top);
            if dir.is_dir() {
                collect(&self.root, &dir, &mut ids, &mut strays)?;
            }
        }
        ids.sort();
        Ok((ids, strays))
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

fn collect(root: &Path, dir: &Path, ids: &mut Vec<Id>, strays: &mut Vec<String>) -> Result<(), Refusal> {
    let io = |e: std::io::Error| Refusal::failed(dir.display().to_string(), e.to_string());
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        let hidden = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, ids, strays)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            if let Some(id) = Id::of_path(root, &path) {
                ids.push(id);
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path).with_extension("");
                strays.push(rel.to_string_lossy().replace('\\', "/"));
            }
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
    /// Every file that does not parse, by path: what an agent hands a human to repair (25, issue 8).
    pub broken: Vec<BrokenFile>,
}

/// One file that does not parse.
#[derive(Debug, Serialize)]
pub struct BrokenFile {
    /// The path under the vault root, with `.md`.
    pub path: String,
    /// The parse error.
    pub detail: String,
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
        let v = Vault::search(&root.join("projects/bp/issues")).unwrap();
        assert_eq!(v.root, root);
        assert_eq!(v.project_of(&root.join("projects/bp/issues")), Some("bp".into()));
        assert_eq!(v.project_of(&root), None);
    }

    #[test]
    fn refuses_outside_a_vault() {
        let dir = tempfile::tempdir().unwrap();
        let err = Vault::search(dir.path()).unwrap_err();
        assert_eq!(err.code, Code::NoVault);
        assert_eq!(err.exit, 2);
    }
}

//! The substrate: node files, headers, bodies, links, ids, the graph, the index, search, the lock, the proposed tree,
//! one commit. This milestone holds the vault root, the envelope and the refusal; the rest lands issue by issue.

mod body;
mod file;
mod header;
mod id;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub use body::{Body, Check, Section};
pub use file::File;
pub use header::{Header, LINKS};
pub use id::Id;

/// The contract version every `--json` result carries. A breaking change to a verb's result bumps it.
pub const CONTRACT: u32 = 1;

/// A refusal is one envelope on stdout and exit code 2. The code is closed; the fix is the command that resolves it.
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{code}: {rule}")]
pub struct Refusal {
    /// The closed refusal code, for example `no_vault`.
    pub code: &'static str,
    /// The id or path the rule refused.
    pub subject: String,
    /// The rule, in words, as the design states it.
    pub rule: &'static str,
    /// The command that resolves the refusal.
    pub fix: String,
    /// Always 2.
    pub exit: u8,
}

impl Refusal {
    /// Builds a refusal with exit code 2.
    pub fn new(code: &'static str, subject: impl Into<String>, rule: &'static str, fix: impl Into<String>) -> Self {
        Self { code, subject: subject.into(), rule, fix: fix.into(), exit: 2 }
    }
}

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
                "no_vault",
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
            "no_vault",
            from.display().to_string(),
            "a verb runs inside a vault: a directory that holds projects/",
            "cd into the vault, or set BP_VAULT",
        ))
    }

    /// Reads and parses one file by id.
    pub fn read(&self, id: &Id) -> Result<File, Refusal> {
        File::read(&self.root, id)
    }

    /// Every markdown file under `projects/` and `shared/`, sorted by id. Dot directories (`.bp`, `.git`) are skipped.
    pub fn ids(&self) -> std::io::Result<Vec<Id>> {
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

    /// Reads and parses every file. The first bad header refuses the load.
    pub fn load(&self) -> Result<Loaded, Refusal> {
        let ids = self.ids().map_err(|e| {
            Refusal::new("no_vault", self.root.display().to_string(), "the vault root is readable", e.to_string())
        })?;
        let mut loaded = Loaded::default();
        for id in ids {
            let file = self.read(&id)?;
            if file.header.is_none() {
                loaded.assets.push(file.id);
            } else if file.id.is_record() {
                loaded.records.push(file);
            } else {
                loaded.nodes.push(file);
            }
        }
        Ok(loaded)
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

fn collect(root: &Path, dir: &Path, ids: &mut Vec<Id>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let hidden = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, ids)?;
        } else if let Some(id) = Id::of_path(root, &path) {
            ids.push(id);
        }
    }
    Ok(())
}

/// Every file of the vault, split by class.
#[derive(Debug, Default)]
pub struct Loaded {
    /// Headed files at node paths.
    pub nodes: Vec<File>,
    /// Headed files at record paths.
    pub records: Vec<File>,
    /// Files without a header.
    pub assets: Vec<Id>,
}

impl Loaded {
    /// The census: files, nodes per kind, records per kind. The converter's report must agree with it.
    pub fn counts(&self) -> Counts {
        let mut counts =
            Counts { files: self.nodes.len() + self.records.len() + self.assets.len(), ..Counts::default() };
        counts.no_header = self.assets.len();
        for h in self.nodes.iter().filter_map(|f| f.header.as_ref()) {
            *counts.nodes.entry(h.kind.clone()).or_default() += 1;
        }
        for h in self.records.iter().filter_map(|f| f.header.as_ref()) {
            *counts.records.entry(h.kind.clone()).or_default() += 1;
        }
        counts
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, id: &str, text: &str) {
        let path = Id::new(id).path(root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn loads_nodes_records_and_assets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "projects/bp/issues/0001.a", "---\nkind: issue\ntitle: A\nstatus: open\n---\n\nLead.\n");
        write(root, "projects/bp/issues/0001.a/claims/s1", "---\nkind: claim\nunder: projects/bp/issues/0001.a\n---\n");
        write(root, "shared/learnings/l", "---\nkind: learning\ntitle: L\nstatus: draft\n---\n");
        write(root, "shared/assets/glossary", "# Glossary\n");
        write(root, ".bp/notes", "---\nkind: issue\n---\n");
        let v = Vault { root: root.to_path_buf() };
        let loaded = v.load().unwrap();
        let counts = loaded.counts();
        assert_eq!(counts.files, 4);
        assert_eq!(counts.no_header, 1);
        assert_eq!(counts.nodes["issue"], 1);
        assert_eq!(counts.nodes["learning"], 1);
        assert_eq!(counts.records["claim"], 1);
        assert_eq!(v.read(&Id::new("projects/bp/issues/0001.a")).unwrap().body.lead, "Lead.");
        assert_eq!(v.read(&Id::new("projects/bp/issues/none")).unwrap_err().code, "no_node");
    }

    #[test]
    fn a_bad_header_refuses_the_load() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "projects/bp/issues/x", "---\ntitle: no kind\n---\n");
        let e = Vault { root: dir.path().to_path_buf() }.load().unwrap_err();
        assert_eq!(e.code, "bad_header");
        assert_eq!(e.subject, "projects/bp/issues/x");
    }

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
        assert_eq!(err.code, "no_vault");
        assert_eq!(err.exit, 2);
    }
}

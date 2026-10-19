//! The fingerprint: whether the index is fresh, cheaply. Hashing every file costs about 240 ms against a call budget
//! of a few. The fingerprint holds the schema, the git head, the time and size of git's own index (checkout, reset,
//! stash and add rewrite it), and the time of every kind directory and design subdirectory (a node file added or
//! removed). The tool is the single writer and commits every write, so a tool write always moves the head. Two
//! changes stay unseen until a commit: an in-place edit, and a record added or removed by hand under a node. The
//! census (`list --count-only`) hashes every file whatever the fingerprint says, so it is the read that reconciles
//! them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::index::schema;

/// The schema, the git head and the modification time of `projects/`, `shared/`, every project and every kind directory.
pub(crate) fn fingerprint(root: &Path) -> String {
    let mut dirs = vec![root.join("projects"), root.join("shared")];
    if let Ok(projects) = std::fs::read_dir(root.join("projects")) {
        dirs.extend(projects.flatten().map(|e| e.path()));
    }
    let mut kinds = Vec::new();
    for d in &dirs[1..] {
        if let Ok(entries) = std::fs::read_dir(d) {
            kinds.extend(entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()));
        }
    }
    let designs: Vec<_> = kinds
        .iter()
        .filter(|p| p.ends_with("designs"))
        .flat_map(|d| [d.join("containers"), d.join("components")])
        .collect();
    dirs.extend(kinds);
    dirs.extend(designs);
    stamp(root, dirs.iter().map(PathBuf::as_path))
}

/// Whether the stored fingerprint holds, read again over the directories it names without listing any. A directory
/// added or removed moves its parent's time, and every parent is in the stored list.
pub(crate) fn still(root: &Path, stored: &str) -> bool {
    let prefix = root.display().to_string();
    let dirs =
        stored.lines().filter(|l| l.starts_with(&prefix)).filter_map(|l| l.rsplit_once('=')).map(|(d, _)| Path::new(d));
    stamp(root, dirs) == stored
}

fn stamp<'a>(root: &Path, dirs: impl Iterator<Item = &'a Path>) -> String {
    // The schema leads, so a binary with another schema never takes this index as fresh.
    let repo = crate::git::Repo::find(root);
    let head = repo.as_ref().and_then(crate::git::Repo::head).unwrap_or_default();
    let mut print = format!("schema={}\n{head}", schema());
    if let Some(m) = repo.and_then(|r| std::fs::metadata(r.git.join("index")).ok()) {
        let nanos = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos());
        let _ = write!(print, "\nindex={}:{}", nanos.unwrap_or_default(), m.len());
    }
    for d in dirs {
        if let Ok(t) = std::fs::metadata(d).and_then(|m| m.modified()) {
            let nanos = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
            let _ = write!(print, "\n{}={nanos}", d.display());
        }
    }
    print
}

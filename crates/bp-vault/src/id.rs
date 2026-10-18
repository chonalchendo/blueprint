//! A node's key is its path id: `projects/<project>/<kind>/<slug>` or `shared/<kind>/<slug>`. A record's key is its
//! parent's id plus `/<kind>s/<stamp>`. The id is the file path under the vault root without `.md` (13, rule 1).

use std::path::{Path, PathBuf};

use serde::Serialize;

/// A path id. Always relative to the vault root, always without `.md`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    /// Builds an id from its text. Trims a trailing `.md` and surrounding whitespace; strips `[[` and `]]`.
    pub fn new(text: &str) -> Self {
        let t = text.trim().trim_start_matches("[[").trim_end_matches("]]").trim();
        Self(t.strip_suffix(".md").unwrap_or(t).to_string())
    }

    /// The id of the file at `path` under `root`, when it is a markdown file inside the root.
    pub fn of_path(root: &Path, path: &Path) -> Option<Self> {
        let rel = path.strip_prefix(root).ok()?;
        let s = rel.to_str()?;
        let s = s.strip_suffix(".md")?;
        Some(Self(s.replace('\\', "/")))
    }

    /// The file this id names.
    pub fn path(&self, root: &Path) -> PathBuf {
        root.join(format!("{}.md", self.0))
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The project segment, for `projects/<project>/...`.
    pub fn project(&self) -> Option<&str> {
        let mut it = self.0.split('/');
        match (it.next(), it.next()) {
            (Some("projects"), Some(p)) => Some(p),
            _ => None,
        }
    }

    /// The kind directory the id sits in: the segment before the slug.
    pub fn kind_dir(&self) -> Option<&str> {
        let mut parts: Vec<&str> = self.0.split('/').collect();
        parts.pop()?;
        parts.last().copied()
    }

    /// A record id has the shape `<parent>/<kind>s/<stamp>`, where the parent is itself a node id: four segments for
    /// a shared node, five for a project node, one more for a design in a subdirectory. A node id has two or three
    /// segments under its root, four for `designs/containers/<slug>` and `designs/components/<slug>`.
    pub fn is_record(&self) -> bool {
        let parts: Vec<&str> = self.0.split('/').collect();
        let node_len = match parts.first().copied() {
            Some("projects") => {
                if parts.get(2).copied() == Some("designs")
                    && matches!(parts.get(3).copied(), Some("containers" | "components"))
                {
                    5
                } else {
                    4
                }
            }
            Some("shared") => 3,
            _ => return false,
        };
        parts.len() == node_len + 2
    }

    /// The parent node of a record id.
    pub fn parent(&self) -> Option<Self> {
        if !self.is_record() {
            return None;
        }
        let mut parts: Vec<&str> = self.0.split('/').collect();
        parts.truncate(parts.len() - 2);
        Some(Self(parts.join("/")))
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_from_text_paths_and_links() {
        assert_eq!(Id::new("[[projects/bp/issues/0411.x]]").as_str(), "projects/bp/issues/0411.x");
        assert_eq!(Id::new(" projects/bp/designs/system.md ").as_str(), "projects/bp/designs/system");
        let root = Path::new("/v");
        let id = Id::of_path(root, Path::new("/v/shared/learnings/a-b.md")).unwrap();
        assert_eq!(id.as_str(), "shared/learnings/a-b");
        assert_eq!(id.path(root), PathBuf::from("/v/shared/learnings/a-b.md"));
        assert_eq!(Id::of_path(root, Path::new("/v/README.txt")), None);
    }

    #[test]
    fn nodes_records_and_parents() {
        let issue = Id::new("projects/bp/issues/0411.x");
        assert!(!issue.is_record());
        assert_eq!(issue.project(), Some("bp"));
        assert_eq!(issue.kind_dir(), Some("issues"));
        let claim = Id::new("projects/bp/issues/0411.x/claims/abc");
        assert!(claim.is_record());
        assert_eq!(claim.parent(), Some(issue));
        let design = Id::new("projects/bp/designs/components/walk");
        assert!(!design.is_record());
        assert!(Id::new("projects/bp/designs/components/walk/runs/abc").is_record());
        assert!(!Id::new("projects/bp/designs/system").is_record());
        assert!(Id::new("shared/learnings/a/runs/abc").is_record());
        assert!(!Id::new("shared/learnings/a").is_record());
    }
}

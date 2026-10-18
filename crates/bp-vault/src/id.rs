//! A path id: the file path under the vault root without `.md`, `projects/<project>/...` or `shared/...` (13, rule 1).

use std::path::{Path, PathBuf};

use serde::Serialize;

/// A path id. Always relative to the vault root, always without `.md`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    /// Parses an id from a link or an argument: `[[`, `]]`, a `|label`, a `#section` and `.md` are stripped. Refuses
    /// anything outside `projects/` and `shared/`, an empty segment, `.` or `..`, so an id never leaves the vault.
    pub fn parse(text: &str) -> Result<Self, String> {
        let t = text.trim().trim_start_matches("[[").trim_end_matches("]]");
        let t = t.split(['|', '#']).next().unwrap_or_default().trim();
        let t = t.strip_suffix(".md").unwrap_or(t);
        let mut parts = t.split('/');
        let root = parts.next().unwrap_or_default();
        let ok = matches!(root, "projects" | "shared")
            && t.split('/').count() >= 3
            && parts.all(|s| !s.is_empty() && s != "." && s != ".." && !s.contains('\\'));
        if ok { Ok(Self(t.to_string())) } else { Err(format!("{text:?} is not a path id under projects/ or shared/")) }
    }

    /// An id built from parts already known to be an id.
    pub(crate) fn from_trusted(text: String) -> Self {
        Self(text)
    }

    /// The id of a markdown file under `root`; `None` for anything else.
    pub fn of_path(root: &Path, path: &Path) -> Option<Self> {
        let rel = path.strip_prefix(root).ok()?.to_str()?.strip_suffix(".md")?;
        Self::parse(&rel.replace('\\', "/")).ok()
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
        self.0.strip_prefix("projects/")?.split('/').next()
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
    fn parses_links_and_arguments() {
        assert_eq!(Id::parse("[[projects/bp/issues/0411.x|the issue]]").unwrap().as_str(), "projects/bp/issues/0411.x");
        assert_eq!(
            Id::parse(" projects/bp/designs/system.md#Context ").unwrap().as_str(),
            "projects/bp/designs/system"
        );
        assert_eq!(Id::parse("shared/learnings/a").unwrap().project(), None);
        assert_eq!(Id::parse("projects/bp/project").unwrap().project(), Some("bp"));
    }

    #[test]
    fn refuses_what_is_not_a_vault_id() {
        for bad in [
            "/etc/passwd",
            "milestone.anvil.x",
            "[[milestone.anvil.x]]",
            "projects/../x/y",
            "projects//x",
            "shared/x",
            "projects/bp/./x",
            "",
        ] {
            assert!(Id::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ids_of_files() {
        let root = Path::new("/v");
        let id = Id::of_path(root, Path::new("/v/shared/learnings/a-b.md")).unwrap();
        assert_eq!(id.path(root), PathBuf::from("/v/shared/learnings/a-b.md"));
        assert_eq!(Id::of_path(root, Path::new("/v/README.txt")), None);
        assert_eq!(Id::of_path(root, Path::new("/elsewhere/projects/a/b.md")), None);
    }
}

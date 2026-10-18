//! What the tool reads from git without a subprocess: the worktree top, the commit `HEAD` names and the `origin`
//! remote. A read verb runs on every agent turn, and a `git` process costs more than the whole call budget (22).

use std::path::{Path, PathBuf};

/// The git repository that holds a directory.
pub struct Repo {
    /// The worktree top: the directory that holds `.git`.
    pub top: PathBuf,
    /// The worktree's git directory; a linked worktree has its own.
    git: PathBuf,
    /// The directory that holds refs, packed refs and config, shared by every worktree.
    common: PathBuf,
}

impl Repo {
    /// The repository whose worktree holds `from`, if any.
    pub fn find(from: &Path) -> Option<Self> {
        let top = from.ancestors().find(|d| d.join(".git").exists())?.to_path_buf();
        let dot = top.join(".git");
        let git = match std::fs::read_to_string(&dot) {
            Ok(text) => top.join(text.strip_prefix("gitdir:")?.trim()),
            Err(_) => dot,
        };
        let common =
            std::fs::read_to_string(git.join("commondir")).map_or_else(|_| git.clone(), |c| git.join(c.trim()));
        Some(Self { top, git, common })
    }

    /// The index file: checkout, stash, reset and add all rewrite it.
    pub fn index(&self) -> PathBuf {
        self.git.join("index")
    }

    /// The commit `HEAD` names.
    pub fn head(&self) -> Option<String> {
        let head = std::fs::read_to_string(self.git.join("HEAD")).ok()?;
        let Some(name) = head.trim().strip_prefix("ref: ") else { return Some(head.trim().to_string()) };
        if let Ok(sha) = std::fs::read_to_string(self.common.join(name)) {
            return Some(sha.trim().to_string());
        }
        let packed = std::fs::read_to_string(self.common.join("packed-refs")).ok()?;
        packed.lines().find_map(|l| l.strip_suffix(name).map(|s| s.trim().to_string()))
    }

    /// The `origin` remote's url, as written in the config.
    pub fn origin(&self) -> Option<String> {
        let config = std::fs::read_to_string(self.common.join("config")).ok()?;
        let mut in_origin = false;
        for line in config.lines().map(str::trim) {
            if line.starts_with('[') {
                in_origin = line == "[remote \"origin\"]";
            } else if in_origin
                && let Some((key, value)) = line.split_once('=')
                && key.trim() == "url"
            {
                return Some(value.trim().to_string());
            }
        }
        None
    }
}

/// A repository url as host and path, so the ssh, scp and https forms of one repository compare equal.
pub fn same_repository(a: &str, b: &str) -> bool {
    fn key(url: &str) -> String {
        let url = url.trim();
        let rest = url.split_once("://").map_or_else(|| url.replacen(':', "/", 1), |(_, r)| r.to_string());
        let rest = rest.rsplit_once('@').map_or(rest.as_str(), |(_, r)| r);
        let rest = rest.trim_end_matches('/');
        rest.strip_suffix(".git").unwrap_or(rest).to_ascii_lowercase()
    }
    key(a) == key(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_repository_in_three_forms() {
        let https = "https://github.com/chonalchendo/blueprint";
        assert!(same_repository(https, "git@github.com:chonalchendo/blueprint.git"));
        assert!(same_repository(https, "ssh://git@github.com/chonalchendo/blueprint.git"));
        assert!(same_repository(https, "https://github.com/chonalchendo/blueprint/"));
        assert!(!same_repository(https, "https://github.com/chonalchendo/blueprint-x"));
    }

    #[test]
    fn reads_top_head_and_origin_from_a_linked_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main/.git");
        std::fs::create_dir_all(main.join("worktrees/w/")).unwrap();
        std::fs::create_dir_all(main.join("refs/heads")).unwrap();
        std::fs::write(main.join("refs/heads/b"), "abc\n").unwrap();
        std::fs::write(main.join("config"), "[core]\n\turl = no\n[remote \"origin\"]\n\turl = git@h:o/r.git\n")
            .unwrap();
        std::fs::write(main.join("worktrees/w/HEAD"), "ref: refs/heads/b\n").unwrap();
        std::fs::write(main.join("worktrees/w/commondir"), "../..\n").unwrap();
        let wt = dir.path().join("w");
        std::fs::create_dir_all(wt.join("sub")).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", main.join("worktrees/w").display())).unwrap();
        let repo = Repo::find(&wt.join("sub")).unwrap();
        assert_eq!(repo.top, wt);
        assert_eq!(repo.head().as_deref(), Some("abc"));
        assert_eq!(repo.origin().as_deref(), Some("git@h:o/r.git"));
    }
}

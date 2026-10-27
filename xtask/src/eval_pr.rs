//! The pull request of a review or respond trial (30, design change 1): the commits the clone's own fixtures name,
//! and a mutant planted in the head.

use std::path::Path;

use anyhow::Result;

use crate::eval_sandbox::git;

/// The commits that the fixtures under `xtask/evals/` at `at` name on their `base:`, `head:` and `stack:` lines. From
/// #60 on, xtask's `the_shipped_fixtures_match_the_history_they_name` asserts that they are in the clone, and an issue's
/// Check that runs xtask's tests runs it. A fixture's head can be a branch commit that the fetched heads do not reach,
/// as `pr-20`'s is, so the clone fetches them too (review of PR 63, major 1).
pub(crate) fn named_commits(code: &Path, at: &str) -> Result<Vec<String>> {
    let files = git(code, &["ls-tree", "-r", "--name-only", at, "--", "xtask/evals"])?;
    let mut shas = std::collections::BTreeSet::new();
    for file in files.lines().filter(|f| Path::new(f).extension().is_some_and(|e| e == "md")) {
        let text = git(code, &["show", &format!("{at}:{file}")])?;
        for line in text.split("\n---\n").next().unwrap_or_default().lines() {
            let value = ["base:", "head:", "stack:"].iter().find_map(|k| line.strip_prefix(k));
            shas.extend(value.and_then(|v| v.split_whitespace().next()).map(str::to_string));
        }
    }
    Ok(shas.into_iter().collect())
}

/// Applies a mutant's diff to the head commit in `dir` and amends that commit, message and author kept. The pull
/// request is then one commit, as the landed one was, so its history does not point at the defect. The committer is
/// `bp-eval`, because the sandbox may have no git identity.
pub(crate) fn plant(dir: &Path, diff: &Path) -> Result<()> {
    git(dir, &["apply", &diff.display().to_string()])?;
    let me = ["-c", "user.name=bp-eval", "-c", "user.email=bp-eval@bp.invalid"];
    git(dir, &[&me[..], &["commit", "-q", "--no-verify", "-a", "--amend", "--no-edit"]].concat())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_planted_head_differs_from_the_landed_head_by_the_mutant_alone() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("r");
        let g = |args: &[&str]| {
            let me = ["-c", "user.name=t", "-c", "user.email=t@t.invalid"];
            git(&repo, &[&me[..], args].concat()).unwrap()
        };
        std::fs::create_dir(&repo).unwrap();
        g(&["init", "-q"]);
        std::fs::write(repo.join("a.rs"), "fn a() -> bool {\n    1 > 0\n}\n").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "base"]);
        let base = g(&["rev-parse", "HEAD"]);
        std::fs::write(repo.join("a.rs"), "fn a() -> bool {\n    2 > 0\n}\n\nfn b() {}\n").unwrap();
        g(&["commit", "-qam", "the landed change (#9)"]);
        let head = g(&["rev-parse", "HEAD"]);
        std::fs::write(repo.join("a.rs"), "fn a() -> bool {\n    2 >= 0\n}\n\nfn b() {}\n").unwrap();
        let diff = format!("{}\n", g(&["diff"]));
        g(&["checkout", "-q", "."]);
        let file = dir.path().join("mutant.diff");
        std::fs::write(&file, &diff).unwrap();

        plant(&repo, &file).unwrap();
        assert_eq!(format!("{}\n", g(&["diff", &head, "HEAD"])), diff, "the mutant and nothing else");
        assert_eq!(g(&["rev-parse", "HEAD^"]), base, "one commit on the base, as landed");
        assert_eq!(g(&["log", "-1", "--format=%s %an"]), "the landed change (#9) t", "message and author kept");
        assert!(g(&["status", "--porcelain"]).is_empty());
    }

    #[test]
    fn the_clone_fetches_the_commits_its_own_fixtures_name() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, clone) = (dir.path().join("r"), dir.path().join("c"));
        let g = |at: &Path, args: &[&str]| {
            let me = ["-c", "user.name=t", "-c", "user.email=t@t.invalid"];
            git(at, &[&me[..], args].concat()).unwrap()
        };
        std::fs::create_dir(&repo).unwrap();
        g(&repo, &["init", "-q", "-b", "master"]);
        std::fs::write(repo.join("a.rs"), "fn a() {}\n").unwrap();
        g(&repo, &["add", "-A"]);
        g(&repo, &["commit", "-qm", "base"]);
        let base = g(&repo, &["rev-parse", "HEAD"]);
        // A branch commit that master never reaches, as `pr-20`'s head is.
        g(&repo, &["checkout", "-q", "-b", "side"]);
        std::fs::write(repo.join("b.rs"), "fn b() {}\n").unwrap();
        g(&repo, &["add", "-A"]);
        g(&repo, &["commit", "-qm", "side"]);
        let side = g(&repo, &["rev-parse", "HEAD"]);
        g(&repo, &["checkout", "-q", "master"]);
        g(&repo, &["branch", "-q", "-D", "side"]);
        let fixture = format!(
            "kind: mutant\nbase: {base}\nhead: {side}\nmutant_diff: |\n    head: {base}x\ntruth:\n---\nhead: no\n"
        );
        std::fs::create_dir_all(repo.join("xtask/evals/bp-review")).unwrap();
        std::fs::write(repo.join("xtask/evals/bp-review/pr-1.md"), fixture).unwrap();
        g(&repo, &["add", "-A"]);
        g(&repo, &["commit", "-qm", "head"]);
        let head = g(&repo, &["rev-parse", "HEAD"]);

        std::fs::create_dir(&clone).unwrap();
        g(&clone, &["init", "-q"]);
        g(&clone, &["fetch", "-q", &repo.display().to_string(), &head]);
        let named = named_commits(&clone, &head).unwrap();
        assert_eq!(
            named,
            {
                let mut v = vec![base.clone(), side.clone()];
                v.sort();
                v
            },
            "the keys only, not the diff or the body"
        );
        assert!(
            git(&clone, &["cat-file", "-e", &format!("{side}^{{commit}}")]).is_err(),
            "the head's fetch leaves it out"
        );
        let refs: Vec<&str> = named.iter().map(String::as_str).collect();
        g(&clone, &[&["fetch", "-q", &repo.display().to_string()][..], &refs].concat());
        assert!(git(&clone, &["cat-file", "-e", &format!("{side}^{{commit}}")]).is_ok(), "an unreachable sha fetches");
    }
}

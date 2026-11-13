//! The commit a build of `bp` is from, when Blueprint's default branch holds it, and the git files whose change can
//! move that answer. `build.rs` includes this file, so a unit test runs the same code on repositories of its own and
//! never reads the refs of the clone it is built in (review of #99, row 45).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The commit of the clone at `dir` when `origin/master` holds it, else `None`: outside a clone (a source archive), or
/// for a commit of a worktree or a pull request branch. The workflow refuses a pin off the default branch, and a squash
/// merge never takes a branch commit there. The check reads local refs only, with no network.
///
/// The paths are what cargo watches: `HEAD`, `packed-refs`, and the ref `HEAD` names and `origin/master`, each as its
/// loose file or, once `git pack-refs` has packed it, its nearest directory under `refs/`, which changes when git
/// writes the ref loose again (review of #99, row 39). Only existing paths are named: cargo reruns a script that
/// watches a missing one on every build.
pub fn built_from(dir: &Path) -> (Option<String>, Vec<PathBuf>) {
    let git = |args: &[&str]| {
        let out = Command::new("git").current_dir(dir).env("GIT_NO_REPLACE_OBJECTS", "1").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let mut watched = Vec::new();
    let names = ["HEAD", "packed-refs", "refs/remotes/origin/master"].map(str::to_string);
    for name in names.into_iter().chain(git(&["symbolic-ref", "-q", "HEAD"])) {
        let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", &name]) else {
            return (None, watched);
        };
        let under_refs = if name.starts_with("refs/") { name.matches('/').count() } else { 0 };
        if let Some(near) = Path::new(&path).ancestors().take(under_refs + 1).find(|a| a.exists()) {
            watched.push(near.to_path_buf());
        }
    }
    let on = git(&["merge-base", "--is-ancestor", "HEAD", "refs/remotes/origin/master"]).is_some();
    (on.then(|| git(&["rev-parse", "--verify", "-q", "HEAD^{commit}"])).flatten(), watched)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::built_from;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("SSH_AUTH_SOCK")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn a_build_pins_only_a_commit_on_origins_default_branch_and_watches_each_ref() {
        let t = tempfile::tempdir().unwrap();
        git(t.path(), &["init", "-q", "--bare", "-b", "master", "origin.git"]);
        git(t.path(), &["init", "-q", "-b", "master", "code"]);
        let code = t.path().join("code");
        git(&code, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&code, &["remote", "add", "origin", &t.path().join("origin.git").display().to_string()]);
        git(&code, &["push", "-q", "-u", "origin", "master"]);
        let head = git(&code, &["rev-parse", "HEAD"]);
        let refs = PathBuf::from(git(&code, &["rev-parse", "--path-format=absolute", "--git-common-dir"])).join("refs");
        // Each ref is watched by its loose file, or by a directory under `refs/` that holds where it would be.
        let covered =
            |watched: &[PathBuf], r: &str| watched.iter().any(|p| p.starts_with(&refs) && refs.join(r).starts_with(p));

        let (sha, watched) = built_from(&code);
        assert_eq!(sha.as_deref(), Some(head.as_str()), "origin/master holds HEAD");
        assert!(watched.contains(&refs.join("heads/master")) && watched.contains(&refs.join("remotes/origin/master")));
        assert!(watched.iter().any(|p| p.ends_with(".git/HEAD")), "{watched:?}");

        // A commit that origin/master lacks, as on a worktree's or a pull request's branch: no pin.
        git(&code, &["checkout", "-q", "-b", "bp/x"]);
        git(&code, &["commit", "-q", "--allow-empty", "-m", "two"]);
        let (sha, watched) = built_from(&code);
        assert_eq!(sha, None);
        assert!(watched.contains(&refs.join("heads/bp/x")), "the ref HEAD names: {watched:?}");

        // After `git pack-refs` no ref has a file of its own: packed-refs and the directories stand in.
        git(&code, &["checkout", "-q", "master"]);
        git(&code, &["pack-refs", "--all"]);
        assert!(!refs.join("heads/master").exists() && !refs.join("remotes/origin/master").exists());
        let (sha, watched) = built_from(&code);
        assert_eq!(sha.as_deref(), Some(head.as_str()));
        assert!(watched.iter().all(|p| p.exists()), "only existing paths: {watched:?}");
        assert!(watched.iter().any(|p| p.ends_with(".git/packed-refs")), "{watched:?}");
        assert!(covered(&watched, "heads/master") && covered(&watched, "remotes/origin/master"), "{watched:?}");

        // Outside any clone: no pin, and nothing to watch.
        let plain = tempfile::tempdir().unwrap();
        assert_eq!(built_from(plain.path()), (None, Vec::new()));
    }
}

//! The forge check `bp-gate` (37, design change 5): the workflow `install` writes into the clone. Unlike the bundle,
//! it is a tracked file: it acts only once it is committed to the default branch, and that commit is high.

/// Where the workflow lives, from the clone's top.
pub const WORKFLOW: &str = ".github/workflows/bp-gate.yml";

/// The workflow for `project`, the slug a signed approval names. Blueprint's own repository builds `bp` from the base;
/// another builds it from this package's repository.
pub fn workflow(project: &str) -> String {
    include_str!("bp-gate.yml").replace("@project@", project).replace("@source@", env!("CARGO_PKG_REPOSITORY"))
}

#[cfg(test)]
mod tests {
    use yaml_rust2::{Yaml, YamlLoader};

    use super::{WORKFLOW, workflow};

    fn key<'a>(y: &'a Yaml, k: &str) -> &'a Yaml {
        &y[k]
    }

    fn keys(y: &Yaml) -> Vec<&str> {
        y.as_hash().into_iter().flatten().filter_map(|(k, _)| k.as_str()).collect()
    }

    #[test]
    fn gate_workflow_runs_from_base_and_requires_signature_for_high() {
        let top = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(top.path().join(".git/info")).unwrap();
        crate::install(top.path(), crate::Target::Claude, "bp").unwrap();
        let text = std::fs::read_to_string(top.path().join(WORKFLOW)).unwrap();
        assert_eq!(text, workflow("bp"));
        let exclude = std::fs::read_to_string(top.path().join(".git/info/exclude")).unwrap();
        assert!(!exclude.contains(WORKFLOW), "the workflow is tracked, so it can be committed: {exclude}");
        let doc = &YamlLoader::load_from_str(&text).unwrap()[0];
        // Only `pull_request_target`: the run reads this file from the default branch, never the pull request's.
        assert_eq!(keys(key(doc, "on")), ["pull_request_target"]);
        let types: Vec<&str> =
            doc["on"]["pull_request_target"]["types"].as_vec().unwrap().iter().filter_map(Yaml::as_str).collect();
        assert_eq!(types, ["opened", "synchronize", "reopened", "edited"]);
        // Read-only, at the top and with no job override; no secret beyond the run's own token.
        assert_eq!(keys(&doc["permissions"]), ["contents", "pull-requests"]);
        assert!(keys(&doc["permissions"]).iter().all(|k| doc["permissions"][*k].as_str() == Some("read")));
        assert_eq!(keys(&doc["jobs"]), ["bp-gate"]);
        let job = &doc["jobs"]["bp-gate"];
        assert_eq!(job["name"].as_str(), Some("bp-gate"), "branch protection requires this name");
        assert!(job["permissions"].is_badvalue(), "no job widens the permissions");
        assert!(!text.contains("secrets."), "{text}");
        assert_eq!(job["env"]["GH_TOKEN"].as_str(), Some("${{ github.token }}"));
        // The base is the commit the run starts from; the head is named once, as data.
        assert_eq!(job["env"]["BP_BASE"].as_str(), Some("${{ github.sha }}"));
        assert_eq!(job["env"]["BP_HEAD"].as_str(), Some("${{ github.event.pull_request.head.sha }}"));
        assert_eq!(text.matches("github.event.pull_request.head").count(), 1, "only BP_HEAD names the head");
        assert_eq!(job["env"]["BP_PROJECT"].as_str(), Some("bp"));
        let steps = job["steps"].as_vec().unwrap();
        let uses: Vec<&str> = steps.iter().filter_map(|s| s["uses"].as_str()).collect();
        assert_eq!(uses, ["actions/checkout@v4", "actions/cache@v4"], "no other action runs");
        let checkout = &steps[0]["with"];
        assert_eq!(checkout["ref"].as_str(), Some("${{ github.sha }}"), "the one checkout is the base");
        assert!(steps[1]["with"]["key"].as_str().unwrap().contains("hashFiles('base/Cargo.lock')"));
        // Each line of a script that names the head fetches it or passes it to `bp gate`: none checks it out or runs
        // it, and the build reads the base or Blueprint's source.
        let runs: Vec<&str> = steps.iter().filter_map(|s| s["run"].as_str()).collect();
        let named: Vec<&str> = runs.iter().flat_map(|r| r.lines()).filter(|l| l.contains("BP_HEAD")).collect();
        assert_eq!(
            named,
            [
                "git -C base fetch -q --no-tags origin \"$BP_HEAD\"",
                "\"$GITHUB_WORKSPACE/bp-target/release/bp\" gate --base \"$BP_BASE\" --head \"$BP_HEAD\" --pr \"$BP_PR\" \\",
            ]
        );
        for word in ["checkout", "switch", "worktree", "reset", "restore", "merge", "pull "] {
            for line in runs.iter().flat_map(|r| r.lines()).filter(|l| l.contains(word)) {
                assert!(line.contains("bp-source"), "{word}: only Blueprint's source is checked out: {line}");
            }
        }
        let build = runs.iter().find(|r| r.contains("cargo build")).unwrap();
        assert!(build.contains("src=base") && build.contains("--locked"), "{build}");
        // For high, `bp gate` reads the comments, and a signed one is the only way to pass.
        let gate = runs.last().unwrap();
        assert!(gate.contains("--project \"$BP_PROJECT\" --comments \"$RUNNER_TEMP/comments.json\""), "{gate}");
        assert!(runs.iter().any(|r| r.starts_with("gh api --paginate \"repos/$GH_REPO/issues/$BP_PR/comments\"")));
        // This repository runs the same file, for its own project.
        let ours = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.github/workflows/bp-gate.yml"));
        assert_eq!(ours.unwrap(), text, "the committed workflow is the one install writes for project bp");
    }
}

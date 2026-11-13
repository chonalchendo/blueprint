//! The forge check `bp-gate` (37, design change 5): the workflow `install` writes into the clone, and the pin of the
//! Blueprint commit it builds `bp` from, in `.bp/risk.yml`. Unlike the bundle, both are tracked files: they act only
//! once they are committed to the default branch, and that commit is high.

/// Where the workflow lives, from the clone's top.
pub const WORKFLOW: &str = ".github/workflows/bp-gate.yml";

/// The risk file, as `bp_change::risk::RISK_FILE` names it; this crate does not depend on that one.
pub const RISK_FILE: &str = ".bp/risk.yml";

/// The commit this binary is built from, or `""` for a build outside a clone. `install` pins the gate to it by default.
pub const BUILT_FROM: &str = env!("BP_BUILT_FROM");

/// The workflow for `project`, the slug a signed approval names. Every repository, Blueprint's too, builds `bp` from
/// this package's repository at the pin.
pub fn workflow(project: &str) -> String {
    include_str!("bp-gate.yml").replace("@project@", project).replace("@source@", env!("CARGO_PKG_REPOSITORY"))
}

/// The risk file's text with the line `gate: <sha>`: the one line that starts `gate:` is replaced, or the line is
/// appended. The other lines stay as the human wrote them.
pub(crate) fn pinned(text: Option<&str>, sha: &str) -> String {
    let line = format!("gate: {sha}");
    let Some(text) = text.filter(|t| !t.is_empty()) else { return format!("{line}\n") };
    if text.lines().any(|l| l.starts_with("gate:")) {
        return text
            .lines()
            .map(|l| if l.starts_with("gate:") { format!("{line}\n") } else { format!("{l}\n") })
            .collect();
    }
    let sep = if text.ends_with('\n') { "" } else { "\n" };
    format!("{text}{sep}{line}\n")
}

#[cfg(test)]
mod tests {
    use yaml_rust2::{Yaml, YamlLoader};

    use super::{RISK_FILE, WORKFLOW, pinned, workflow};

    const PIN: &str = "0123456789abcdef0123456789abcdef01234567";

    /// `actions/checkout` v4.4.0, by commit: a moved tag does not change the code that runs.
    const CHECKOUT: &str = "actions/checkout@11d5960a326750d5838078e36cf38b85af677262";

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
        crate::install(top.path(), crate::Target::Claude, "bp", PIN).unwrap();
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
        // The base is the commit the run starts from; the head is named once, as data.
        assert_eq!(job["env"]["BP_BASE"].as_str(), Some("${{ github.sha }}"));
        assert_eq!(job["env"]["BP_HEAD"].as_str(), Some("${{ github.event.pull_request.head.sha }}"));
        // A signature names the base branch, so a retarget reruns this check and needs a new signature.
        assert_eq!(job["env"]["BP_BASE_REF"].as_str(), Some("${{ github.event.pull_request.base.ref }}"));
        assert_eq!(text.matches("github.event.pull_request.head").count(), 1, "only BP_HEAD names the head");
        assert_eq!(job["env"]["BP_PROJECT"].as_str(), Some("bp"));
        assert_eq!(job["env"]["BP_SOURCE"].as_str(), Some(env!("CARGO_PKG_REPOSITORY")));
        let steps = job["steps"].as_vec().unwrap();
        // One action, pinned by commit, and no cache: a cache is not signed, and unsigned runs write it.
        let uses: Vec<&str> = steps.iter().filter_map(|s| s["uses"].as_str()).collect();
        assert_eq!(uses, [CHECKOUT], "no other action runs");
        let code = text.lines().map(str::trim).filter(|l| !l.starts_with('#'));
        assert!(code.into_iter().all(|l| !l.to_lowercase().contains("cache")), "{text}");
        let checkout = &steps[0]["with"];
        assert_eq!(checkout["ref"].as_str(), Some("${{ github.sha }}"), "the one checkout is the base");
        assert_eq!(checkout["persist-credentials"].as_bool(), Some(false), "the checkout keeps no token");
        // The token is a step's, for the two steps that call the forge: the build never holds it.
        assert!(job["env"]["GH_TOKEN"].is_badvalue());
        let token: Vec<&str> =
            steps.iter().filter(|s| !s["env"]["GH_TOKEN"].is_badvalue()).filter_map(|s| s["name"].as_str()).collect();
        assert_eq!(token, ["fetch the head as data", "read the comments"]);
        assert!(steps.iter().all(
            |s| s["env"]["GH_TOKEN"].is_badvalue() || s["env"]["GH_TOKEN"].as_str() == Some("${{ github.token }}")
        ));
        let runs: Vec<&str> = steps.iter().filter_map(|s| s["run"].as_str()).collect();
        let lines: Vec<&str> = runs.iter().flat_map(|r| r.lines()).map(str::trim).collect();
        // The pin is read at the base, the default branch's commit; then `bp` is built from Blueprint at the pin.
        let at = |needle: &str| lines.iter().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle}"));
        assert_eq!(lines[at("BP_GATE=")], r#"echo "BP_GATE=$gate" >> "$GITHUB_ENV""#);
        assert_eq!(lines.iter().filter(|l| l.contains("risk.yml")).count(), 2, "one read of the pin, one message");
        assert_eq!(
            lines[at("risk.yml")],
            r"gate=$(sed -n -E 's/^gate: ([0-9a-f]{40})$/\1/p' base/.bp/risk.yml)",
            "the pin is the base's"
        );
        assert_eq!(lines[at("-ne 40")], r#"if [ "${#gate}" -ne 40 ]; then"#, "no pin, no gate");
        let build = runs.iter().find(|r| r.contains("cargo build")).unwrap();
        assert_eq!(
            build.lines().collect::<Vec<_>>(),
            [
                "git init -q bp-source",
                r#"git -C bp-source fetch -q --depth 1 --no-tags "$BP_SOURCE" "$BP_GATE""#,
                r#"git -C bp-source checkout -q --detach "$BP_GATE""#,
                "cd bp-source",
                "cargo build --locked --release -p bp-tool",
            ],
            "the build reads Blueprint at the pin, never a path of this repository's tree"
        );
        assert!(at("cargo build") < at("BP_HEAD"), "the build ends before the head is fetched");
        // Each line of a script that names the head fetches it or passes it to `bp gate`: none checks it out or runs
        // it.
        let named: Vec<&str> = lines.iter().copied().filter(|l| l.contains("BP_HEAD")).collect();
        assert_eq!(
            named,
            [
                r#"git -C base -c "http.extraheader=AUTHORIZATION: basic $auth" fetch -q --no-tags origin "$BP_HEAD""#,
                r#"--head "$BP_HEAD" --pr "$BP_PR" --project "$BP_PROJECT" --comments "$RUNNER_TEMP/comments.json""#,
            ]
        );
        for word in ["checkout", "switch", "worktree", "reset", "restore", "merge", "pull "] {
            for line in lines.iter().filter(|l| l.contains(word)) {
                assert!(line.contains("bp-source"), "{word}: only Blueprint's source is checked out: {line}");
            }
        }
        // For high, `bp gate` reads the comments, and a signed one is the only way to pass.
        let gate = runs.last().unwrap();
        assert!(
            gate.starts_with(r#""$GITHUB_WORKSPACE/bp-source/target/release/bp" gate --base "$BP_BASE""#),
            "{gate}"
        );
        assert!(gate.contains(r#"--base-ref "$BP_BASE_REF""#), "{gate}");
        assert!(runs.iter().any(|r| r.starts_with("gh api --paginate \"repos/$GH_REPO/issues/$BP_PR/comments\"")));
        // This repository runs the same file, for its own project.
        let ours = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.github/workflows/bp-gate.yml"));
        assert_eq!(ours.unwrap(), text, "the committed workflow is the one install writes for project bp");
    }

    #[test]
    fn install_pins_the_gate_in_the_risk_file_and_keeps_the_rest() {
        let top = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(top.path().join(".git/info")).unwrap();
        let t = top.path();
        let risk = || std::fs::read_to_string(t.join(RISK_FILE)).unwrap();
        crate::install(t, crate::Target::Claude, "bp", PIN).unwrap();
        assert_eq!(risk(), format!("gate: {PIN}\n"), "a clone with no risk file gets one");
        let mine = "high: [infra/]\n# the human's key\nsigner: ssh-ed25519 AAAA me\n";
        std::fs::write(t.join(RISK_FILE), mine).unwrap();
        let (wrote, v) = crate::install(t, crate::Target::Claude, "bp", PIN).unwrap();
        assert!(wrote && v["gate"] == PIN, "{v}");
        assert_eq!(risk(), format!("{mine}gate: {PIN}\n"), "the human's lines stay");
        assert!(!crate::install(t, crate::Target::Claude, "bp", PIN).unwrap().0, "a repeat writes nothing");
        let next = "fedcba9876543210fedcba9876543210fedcba98";
        assert!(crate::install(t, crate::Target::Claude, "bp", next).unwrap().0);
        assert_eq!(risk(), format!("{mine}gate: {next}\n"), "a new pin replaces the old line");
        let exclude = std::fs::read_to_string(t.join(".git/info/exclude")).unwrap();
        assert!(!exclude.contains(RISK_FILE), "the pin is tracked, so it can be committed: {exclude}");
    }

    #[test]
    fn pinned_writes_one_gate_line() {
        assert_eq!(pinned(None, PIN), format!("gate: {PIN}\n"));
        assert_eq!(pinned(Some(""), PIN), format!("gate: {PIN}\n"));
        assert_eq!(
            pinned(Some("low: [docs/]"), PIN),
            format!("low: [docs/]\ngate: {PIN}\n"),
            "a last line with no end"
        );
        assert_eq!(pinned(Some("gate: 'x'\nlow: []\n"), PIN), format!("gate: {PIN}\nlow: []\n"), "the line in place");
    }
}

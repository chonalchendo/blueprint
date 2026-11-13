//! The forge check `bp-gate` (37, design change 5): the workflow `install` writes into the clone, and the pin of the
//! Blueprint commit it builds `bp` from, in `.bp/risk.yml`. Unlike the bundle, both are tracked files: they act only
//! once they are committed to the default branch, and that commit is high.

use std::path::Path;

use bp_vault::{Code, Refusal};

/// Where the workflow lives, from the clone's top.
pub const WORKFLOW: &str = ".github/workflows/bp-gate.yml";

/// The risk file, as `bp_change::risk::RISK_FILE` names it; this crate does not depend on that one.
pub const RISK_FILE: &str = ".bp/risk.yml";

/// The commit this binary is built from when Blueprint's default branch holds it, else `""`. `install` pins the gate to
/// it by default.
pub const BUILT_FROM: &str = env!("BP_BUILT_FROM");

/// The workflow for `project`, the slug a signed approval names. Every repository, Blueprint's too, builds `bp` from
/// this package's repository at the pin.
pub fn workflow(project: &str) -> String {
    include_str!("bp-gate.yml").replace("@project@", project).replace("@source@", env!("CARGO_PKG_REPOSITORY"))
}

/// The pin `install` writes as `gate:` in the risk file. A pin change is high and needs the human's signature, so
/// only the human's own `--gate` replaces a committed pin.
#[derive(Debug, Clone, Copy)]
pub enum Pin<'a> {
    /// `--gate <sha>`: written, and it replaces the pin the file holds.
    Given(&'a str),
    /// The commit this binary is built from: written only into a file that holds no `gate` key.
    Built(&'a str),
}

/// What `install` writes of the two tracked files, and the pin in force after it. [`plan`] reads and checks both
/// before the bundle writes anything, so a refusal leaves the clone as it was (review of #99, row 36).
pub(crate) struct Tracked {
    writes: Vec<(&'static str, String)>,
    pub(crate) gate: Option<String>,
}

impl Tracked {
    /// Writes each changed file whole, by a rename: an install cut short leaves the old file, never a short one that
    /// still parses (review of #99, row 37). `true` when it wrote one.
    pub(crate) fn write(&self, top: &Path) -> Result<bool, Refusal> {
        for (rel, text) in &self.writes {
            let path = top.join(rel);
            let dir = path.parent().expect("a tracked file has a parent");
            let tmp = dir.join(format!(".{}.{}", path.file_name().expect("a file").display(), std::process::id()));
            let failed =
                |e: std::io::Error| Refusal::failed(path.display().to_string(), format!("{e}; fix it, then rerun"));
            std::fs::create_dir_all(dir).map_err(failed)?;
            std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                failed(e)
            })?;
        }
        Ok(!self.writes.is_empty())
    }
}

/// The workflow for `project` and the risk file with `pin`, as [`Pin`]'s rule writes it, each checked: inside the top,
/// under a writable directory, and with a pin the workflow reads as `bp gate` does.
pub(crate) fn plan(top: &Path, project: &str, pin: Option<Pin<'_>>) -> Result<Tracked, Refusal> {
    for rel in [WORKFLOW, RISK_FILE] {
        if !crate::inside(top, rel) {
            return Err(Refusal::new(
                Code::BundleInvalid,
                rel,
                "install writes only inside the clone's top: no absolute path, no `..`, no symlink out (17)",
                format!("{rel} is not the install's to touch at {}; remove the link, then rerun", top.display()),
            ));
        }
    }
    let read = |rel: &str| {
        crate::read(&top.join(rel)).map_err(|e| Refusal::failed(e.subject, format!("{}; fix {rel}, then rerun", e.fix)))
    };
    let risk = read(RISK_FILE)?;
    let new_risk = pin.and_then(|p| written(risk.as_deref(), p));
    let gate = match new_risk.as_deref().or(risk.as_deref()) {
        Some(text) => in_force(text).map_err(|why| {
            Refusal::new(
                Code::RiskFileInvalid,
                RISK_FILE,
                "the forge check reads its pin from the one line `gate: <40 lower-case hex digits>` of .bp/risk.yml \
                 (37, design change 5)",
                why,
            )
        })?,
        None => None,
    };
    let mut writes = Vec::new();
    let flow = workflow(project);
    if read(WORKFLOW)?.as_deref() != Some(flow.as_str()) {
        writes.push((WORKFLOW, flow));
    }
    if let Some(new) = new_risk.filter(|new| risk.as_deref() != Some(new.as_str())) {
        writes.push((RISK_FILE, new));
    }
    for (rel, _) in &writes {
        let path = top.join(rel);
        let near = path.ancestors().skip(1).find(|a| a.exists()).expect("the top exists");
        let why = if !near.is_dir() {
            "is not a directory"
        } else if near.metadata().is_ok_and(|m| m.permissions().readonly()) {
            "is read-only"
        } else {
            continue;
        };
        return Err(Refusal::failed(
            near.display().to_string(),
            format!("{} {why}, so install cannot write {rel}; fix it, then rerun", near.display()),
        ));
    }
    Ok(Tracked { writes, gate })
}

/// The risk file's text after the install writes `pin`; `None` when the file stays as it is. A file that does not
/// parse holds a pin for this rule: the install cannot tell, and [`in_force`] refuses that file anyway.
pub(crate) fn written(text: Option<&str>, pin: Pin<'_>) -> Option<String> {
    let holds = || {
        let docs = text.map(yaml_rust2::YamlLoader::load_from_str);
        match docs {
            None => false,
            Some(Ok(docs)) => docs.first().is_some_and(|d| !d["gate"].is_badvalue()),
            Some(Err(_)) => true,
        }
    };
    match pin {
        Pin::Given(sha) => Some(pinned(text, sha)),
        Pin::Built(sha) if !holds() => Some(pinned(text, sha)),
        Pin::Built(_) => None,
    }
}

/// The pin in force, by the workflow's own rule: the one line `gate: <40 lower-case hex digits>`, split at `\n` only,
/// as `sed` reads it. `Err` names the line when the file is not YAML, or its `gate` key is in another form (quoted,
/// with a comment, twice, or with a CR line end): the workflow would read no pin, or `bp gate` another (review of #99,
/// row 38).
pub(crate) fn in_force(text: &str) -> Result<Option<String>, String> {
    let line = |at: Option<usize>| at.map_or_else(String::new, |i| format!("line {} of {RISK_FILE}: ", i + 1));
    let gate_line = text.split('\n').position(|l| l.trim_start_matches([' ', '"', '\'']).starts_with("gate"));
    let fix = "; write the pin as the one line `gate: <40 lower-case hex digits>` with LF line ends, then rerun";
    let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| format!("{RISK_FILE} is not YAML: {e}{fix}"))?;
    let hex = |s: &&str| s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let read: Vec<&str> = text.split('\n').filter_map(|l| l.strip_prefix("gate: ")).filter(hex).collect();
    let key = docs.first().map(|d| &d["gate"]).filter(|g| !g.is_badvalue());
    match (key, &read[..]) {
        (None, []) => Ok(None),
        (Some(k), [one]) if k.as_str() == Some(one) => Ok(Some((*one).to_string())),
        _ => Err(format!("{}the workflow reads no one pin here{fix}", line(gate_line))),
    }
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
    use bp_vault::Code;
    use serde_json::json;
    use yaml_rust2::{Yaml, YamlLoader};

    use super::{Pin, RISK_FILE, WORKFLOW, in_force, pinned, workflow};

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
        crate::install(top.path(), crate::Target::Claude, "bp", Some(Pin::Built(PIN))).unwrap();
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
        // A pass belongs to the commit, so a high head passes onto the default branch only.
        assert_eq!(job["env"]["BP_DEFAULT_REF"].as_str(), Some("${{ github.event.repository.default_branch }}"));
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
            r#"gate=$(git -C base show "$BP_BASE:.bp/risk.yml" | sed -n -E 's/^gate: ([0-9a-f]{40})$/\1/p')"#,
            "the pin is the base commit's, never the work tree's"
        );
        assert_eq!(lines[at("-ne 40")], r#"if [ "${#gate}" -ne 40 ]; then"#, "no pin, no gate");
        let build = runs.iter().find(|r| r.contains("cargo build")).unwrap();
        assert_eq!(
            build.lines().collect::<Vec<_>>(),
            [
                "git init -q bp-source",
                r#"git -C bp-source fetch -q --no-tags "$BP_SOURCE" HEAD"#,
                r#"if ! git -C bp-source merge-base --is-ancestor "$BP_GATE" FETCH_HEAD; then"#,
                r#"  echo "::error::the pin $BP_GATE is not on the default branch of $BP_SOURCE""#,
                "  exit 1",
                "fi",
                r#"git -C bp-source checkout -q --detach "$BP_GATE""#,
                "cd bp-source",
                "cargo build --locked --release -p bp-tool",
            ],
            "the build reads Blueprint at the pin, on its default branch, never a path of this repository's tree"
        );
        assert!(at("cargo build") < at("BP_HEAD"), "the build ends before the head is fetched");
        // Each line of a script that names the head fetches it or passes it to `bp gate`: none checks it out or runs
        // it.
        let named: Vec<&str> = lines.iter().copied().filter(|l| l.contains("BP_HEAD")).collect();
        assert_eq!(
            named,
            [
                r#"git -C base -c "http.extraheader=AUTHORIZATION: basic $auth" fetch -q --no-tags origin "$BP_HEAD""#,
                r#"--default-ref "$BP_DEFAULT_REF" --head "$BP_HEAD" --pr "$BP_PR" --project "$BP_PROJECT" --comments "$RUNNER_TEMP/comments.json""#,
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
        assert!(gate.contains(r#"--base-ref "$BP_BASE_REF" \"#), "{gate}");
        assert!(runs.iter().any(|r| r.starts_with("gh api --paginate \"repos/$GH_REPO/issues/$BP_PR/comments\"")));
        // This repository runs the same file, for its own project.
        let ours = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.github/workflows/bp-gate.yml"));
        assert_eq!(ours.unwrap(), text, "the committed workflow is the one install writes for project bp");
        // CI runs the head's code, so its token reads only: else that code writes statuses, checks and runs.
        let ci = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.github/workflows/ci.yml"));
        let ci = &YamlLoader::load_from_str(&ci.unwrap()).unwrap()[0];
        assert_eq!(keys(&ci["permissions"]), ["contents"]);
        assert_eq!(ci["permissions"]["contents"].as_str(), Some("read"));
        assert!(ci["jobs"].as_hash().unwrap().values().all(|j| j["permissions"].is_badvalue()), "no job widens it");
    }

    #[test]
    fn gate_workflow_reads_the_whole_base_and_ends_a_stalled_run() {
        // Review of #99, rows 35 and 42: keys that change no text a reader checks, but every run.
        let doc = &YamlLoader::load_from_str(&workflow("bp")).unwrap()[0];
        let job = &doc["jobs"]["bp-gate"];
        let steps = job["steps"].as_vec().unwrap();
        let checkout = &steps[0]["with"];
        // The whole history: a shallow base has no merge base, and every pull request would read high.
        assert_eq!(checkout["fetch-depth"].as_i64(), Some(0), "the test merge needs the base's history");
        assert_eq!(checkout["path"].as_str(), Some("base"), "the base sits beside bp-source, not around it");
        let gate_step = steps.iter().find(|s| s["name"].as_str() == Some("bp gate")).unwrap();
        assert_eq!(gate_step["working-directory"].as_str(), Some("base"), "bp gate runs in the base's clone");
        // A stalled network step ends the run, and a newer push ends the older run of the same pull request.
        assert_eq!(job["timeout-minutes"].as_i64(), Some(30));
        assert_eq!(job["concurrency"]["group"].as_str(), Some("bp-gate-${{ github.event.pull_request.number }}"));
        assert_eq!(job["concurrency"]["cancel-in-progress"].as_bool(), Some(true));
    }

    #[test]
    fn install_pins_the_gate_in_the_risk_file_and_keeps_the_rest() {
        let top = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(top.path().join(".git/info")).unwrap();
        let t = top.path();
        let risk = || std::fs::read_to_string(t.join(RISK_FILE)).unwrap();
        let run = |pin| crate::install(t, crate::Target::Claude, "bp", pin).unwrap();
        let (_, v) = run(Some(Pin::Built(PIN)));
        assert_eq!((risk(), &v["gate"]), (format!("gate: {PIN}\n"), &json!(PIN)), "a clone with no risk file gets one");
        let mine = "high: [infra/]\n# the human's key\nsigner: ssh-ed25519 AAAA me\n";
        std::fs::write(t.join(RISK_FILE), mine).unwrap();
        let (wrote, v) = run(Some(Pin::Built(PIN)));
        assert!(wrote && v["gate"] == PIN, "{v}");
        assert_eq!(risk(), format!("{mine}gate: {PIN}\n"), "the human's lines stay");
        assert!(!run(Some(Pin::Built(PIN))).0, "a repeat writes nothing");
        // A committed pin stays: only the human's `--gate` moves it, since a pin change is high.
        let next = "fedcba9876543210fedcba9876543210fedcba98";
        let (wrote, v) = run(Some(Pin::Built(next)));
        assert!(!wrote && v["gate"] == PIN, "the result names the pin in force: {v}");
        assert_eq!(risk(), format!("{mine}gate: {PIN}\n"));
        let (wrote, v) = run(Some(Pin::Given(next)));
        assert!(wrote && v["gate"] == next, "{v}");
        assert_eq!(risk(), format!("{mine}gate: {next}\n"), "--gate replaces the old line");
        let exclude = std::fs::read_to_string(t.join(".git/info/exclude")).unwrap();
        assert!(!exclude.contains(RISK_FILE), "the pin is tracked, so it can be committed: {exclude}");
        // A `gate` key in a form the workflow cannot read, or a file that does not parse, refuses: nothing is written.
        for held in ["\"gate\": 'x'\n", "gate: [\n", &format!("gate: {PIN} # pin\n"), &format!("gate: {PIN}\r\n")] {
            std::fs::write(t.join(RISK_FILE), held).unwrap();
            std::fs::remove_file(t.join(WORKFLOW)).ok();
            for pin in [Some(Pin::Built(PIN)), None] {
                let r = crate::install(t, crate::Target::Claude, "bp", pin).unwrap_err();
                assert_eq!((r.code, r.subject.as_str()), (Code::RiskFileInvalid, RISK_FILE), "{held:?}: {r:?}");
                // The gate line, or for a file that is not YAML the parser's own line and column.
                assert!(r.fix.starts_with("line 1 of .bp/risk.yml: ") || r.fix.contains(" line 2 column 1"), "{r:?}");
                assert!(risk() == held && !t.join(WORKFLOW).exists(), "{held:?}: nothing is written");
            }
        }
        // `--gate` over a quoted pin adds a second `gate` key, which refuses too.
        std::fs::write(t.join(RISK_FILE), format!("low: []\n'gate': {PIN}\n")).unwrap();
        let r = crate::install(t, crate::Target::Claude, "bp", Some(Pin::Given(next))).unwrap_err();
        assert!(
            r.code == Code::RiskFileInvalid && r.fix.contains("duplicated key in mapping at byte 63 line 3"),
            "{r:?}"
        );
        // A plain-`gate:` line that does not parse is the one `--gate` mends.
        std::fs::write(t.join(RISK_FILE), "gate: [\n").unwrap();
        let (_, v) = run(Some(Pin::Given(next)));
        assert!(v["gate"] == next && risk() == format!("gate: {next}\n"), "{v}");
        // With no known commit, the pin stays as it is: the check fails closed until the human writes one.
        std::fs::write(t.join(RISK_FILE), format!("gate: {next}\n")).unwrap();
        let (_, v) = run(None);
        assert!(v["gate"] == next && risk() == format!("gate: {next}\n"), "{v}");
        std::fs::remove_file(t.join(RISK_FILE)).unwrap();
        let (_, v) = run(None);
        assert!(!t.join(RISK_FILE).exists() && v["gate"].is_null(), "no risk file is made with no pin");
    }

    #[test]
    fn in_force_reads_the_one_pin_line_the_workflow_reads() {
        // The workflow's own line, run by `sh` over each file: the install's reader and the workflow's agree on every
        // pin they read, and the install refuses each file where the workflow reads none but YAML reads a `gate`.
        let flow = workflow("bp");
        let line = flow.lines().find(|l| l.contains("risk.yml\" | sed")).unwrap();
        let sed = &line[line.find("sed -n").unwrap()..line.rfind(')').unwrap()];
        let dir = tempfile::tempdir().unwrap();
        let workflow_reads = |text: &str| {
            std::fs::write(dir.path().join("risk.yml"), text).unwrap();
            let out = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("gate=$({sed} < risk.yml); [ \"${{#gate}}\" -eq 40 ] && printf %s \"$gate\""))
                .current_dir(dir.path())
                .output()
                .unwrap();
            Some(String::from_utf8(out.stdout).unwrap()).filter(|g| !g.is_empty())
        };
        let ok = |text: &str| in_force(text).unwrap();
        for (text, pin) in [
            (format!("low: []\ngate: {PIN}\n"), Some(PIN)),
            (format!("gate: {PIN}"), Some(PIN)),
            ("low: [docs/]\n".into(), None),
            (String::new(), None),
            ("# gate: none yet\n".into(), None),
        ] {
            assert_eq!(ok(&text).as_deref(), pin, "{text:?}");
            assert_eq!(workflow_reads(&text).as_deref(), pin, "the workflow reads the same: {text:?}");
        }
        for (text, why) in [
            (format!("gate: '{PIN}'\n"), "quoted"),
            (format!("\"gate\": {PIN}\n"), "a quoted key"),
            (format!("gate: {PIN} # pin\n"), "a comment"),
            (format!("gate: {PIN}\ngate: {PIN}\n"), "twice"),
            (format!("low: []\r\ngate: {PIN}\r\n"), "CRLF"),
            ("gate: abc\n".into(), "short"),
            ("gate:\n  - a\n".into(), "a list"),
            (format!("high:\n\t- : [\ngate: {PIN}\n"), "not YAML"),
            (format!("a: 1\n---\ngate: {PIN}\n"), "in a second document"),
        ] {
            let e = in_force(&text).unwrap_err();
            assert!(e.contains("write the pin as the one line"), "{why}: {e}");
            // Each refused file is one where the workflow and a YAML reader disagree, or YAML reads nothing at all.
            if let Ok(docs) = YamlLoader::load_from_str(&text) {
                let yaml = docs.first().map(|d| &d["gate"]).filter(|g| !g.is_badvalue()).map(|g| format!("{g:?}"));
                let sed = workflow_reads(&text).map(|g| format!("{:?}", Yaml::String(g)));
                assert_ne!(yaml, sed, "{why}");
            }
        }
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

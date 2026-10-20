//! `install` and `check` on a directory that stands in for a clone's top: the writes, the repeat, and each drift the
//! manifest catches (17, check 7). Nothing here runs a process or reads the home directory.

use std::path::Path;

use bp_install::{Target, check, install};
use bp_vault::Code;

fn top() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    dir
}

fn read(top: &Path, rel: &str) -> String {
    std::fs::read_to_string(top.join(rel)).unwrap()
}

fn write(top: &Path, rel: &str, text: &str) {
    std::fs::write(top.join(rel), text).unwrap();
}

/// The subject `check` refuses, or `ok`.
fn checked(top: &Path) -> String {
    match check(top) {
        Ok(()) => "ok".into(),
        Err(r) => {
            assert_eq!((r.code, r.exit), (Code::BundleMismatch, 2), "{r:?}");
            assert!(r.fix.contains("bp install --target"), "{r:?}");
            r.subject
        }
    }
}

#[test]
fn installs_keeps_the_users_lines_and_a_repeat_writes_nothing() {
    let dir = top();
    let t = dir.path();
    write(t, "CLAUDE.md", "# Mine\n\nKeep this line.\n");
    std::fs::create_dir_all(t.join(".claude")).unwrap();
    let mine = r#"{"model": "x", "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "mine.sh"}]}]},
        "permissions": {"allow": ["Bash(ls *)", "Bash(export BP_ROLE=old)"]}}"#;
    write(t, ".claude/settings.local.json", mine);
    assert_eq!(checked(t), "ok", "a clone with no bp- entry is not checked");
    let (wrote, v) = install(t, Target::Claude).unwrap();
    assert!(wrote);
    assert_eq!(v["target"], "claude");
    for entry in ["AGENTS.md#bp", "CLAUDE.md#bp", "settings.local.json#hooks"] {
        assert!(v["files"][entry].is_string(), "{entry}: {v}");
    }
    assert_eq!(
        read(t, "CLAUDE.md"),
        format!("# Mine\n\nKeep this line.\n\n{IMPORT}"),
        "the user's lines, then the import"
    );
    assert!(read(t, "AGENTS.md").starts_with("<!-- bp:begin -->\n## "), "the section lives in AGENTS.md");
    let settings: serde_json::Value = serde_json::from_str(&read(t, ".claude/settings.local.json")).unwrap();
    assert_eq!(settings["model"], "x");
    let pre = settings["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre[0]["hooks"][0]["command"], "mine.sh", "the user's entry stays first");
    assert!(pre[1]["hooks"][0]["command"].as_str().unwrap().ends_with("/.claude/hooks/bp-guard.sh"));
    // Each of Blueprint's hooks blocks when its script cannot start; the user's entry is not touched.
    assert!(pre[0]["hooks"][0].get("onFailure").is_none());
    let groups = settings["hooks"].as_object().unwrap().values().flat_map(|g| g.as_array().unwrap());
    let ours: Vec<&serde_json::Value> = groups.map(|g| &g["hooks"][0]).filter(|h| h["command"] != "mine.sh").collect();
    assert_eq!(ours.len(), 4);
    assert!(ours.iter().all(|h| h["onFailure"] == "block"), "{ours:?}");
    let allow = &settings["permissions"]["allow"];
    assert_eq!(allow, &serde_json::json!(["Bash(ls *)", "Bash(export BP_ROLE=*)"]), "an older rule of ours goes");
    let manifest: serde_json::Value = serde_json::from_str(&read(t, ".claude/skills/bundle.json")).unwrap();
    assert_eq!((manifest["tool_version"].as_str(), &manifest["files"]), (Some(bp_install::VERSION), &v["files"]));
    let exclude = read(t, ".git/info/exclude");
    assert!(exclude.lines().any(|l| l == "/.claude/settings.local.json"), "{exclude}");
    assert!(!exclude.lines().any(|l| l == "/CLAUDE.md"), "the user's file stays tracked");
    assert!(exclude.lines().any(|l| l == "/AGENTS.md"), "an AGENTS.md the install created is its own");
    let guard = std::fs::metadata(t.join(".claude/hooks/bp-guard.sh")).unwrap();
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&guard.permissions()) & 0o111, 0o111, "run by path");
    assert_eq!(checked(t), "ok");
    let (wrote, again) = install(t, Target::Claude).unwrap();
    assert!(!wrote, "a repeat exits 3");
    assert_eq!(again, v);
    assert_eq!(read(t, ".git/info/exclude"), exclude, "a repeat adds no exclude line");
}

/// The section the claude target adds to a Claude Code file, so the harness reads `AGENTS.md` (27, design change 1).
const IMPORT: &str = "<!-- bp:begin -->\n@AGENTS.md\n<!-- bp:end -->\n";

/// One edit to an installed clone.
type Edit<'a> = &'a dyn Fn(&Path);

#[test]
fn check_catches_each_drift_and_install_repairs_it() {
    let dir = top();
    let t = dir.path();
    write(t, "CLAUDE.md", "# Mine\n");
    install(t, Target::Claude).unwrap();
    let (skill, settings, manifest) =
        (".claude/skills/bp-complete/SKILL.md", ".claude/settings.local.json", ".claude/skills/bundle.json");
    let tamper = |rel: &'static str| {
        move |t: &Path| write(t, rel, &read(t, rel).replace("<!-- bp:end -->", "x\n<!-- bp:end -->"))
    };
    let (agents, claude) = (tamper("AGENTS.md"), tamper("CLAUDE.md"));
    let edits: [(Edit, &str); 8] = [
        (&|t| write(t, skill, &(read(t, skill) + "\n")), skill),
        (&agents, "AGENTS.md#bp"),
        (&claude, "CLAUDE.md#bp"),
        (&|t| write(t, settings, &read(t, settings).replace("SessionEnd", "Gone")), "settings.local.json#hooks"),
        (&|t| write(t, settings, &read(t, settings).replace("BP_ROLE=*", "BP_ROLE=x")), "settings.local.json#hooks"),
        (&|t| write(t, manifest, &read(t, manifest).replace(bp_install::VERSION, "0.0.0-quokka")), manifest),
        (&|t| std::fs::remove_file(t.join(manifest)).unwrap(), ".claude/skills"),
        // A bundle that an older binary of the same version wrote: the skill and its manifest entry agree, and the
        // binary ships other bytes.
        (
            &|t| {
                write(t, skill, "older");
                let older = blake3::hash(b"older").to_hex().to_string();
                let mut m: serde_json::Value = serde_json::from_str(&read(t, manifest)).unwrap();
                m["files"][skill] = older.into();
                write(t, manifest, &m.to_string());
            },
            skill,
        ),
    ];
    for (edit, subject) in edits {
        edit(t);
        assert_eq!(checked(t), subject);
        assert!(install(t, Target::Claude).unwrap().0, "{subject}");
        assert_eq!(checked(t), "ok", "{subject}");
    }
}

#[test]
fn a_reinstall_removes_what_the_bundle_no_longer_holds() {
    let dir = top();
    let t = dir.path();
    install(t, Target::Claude).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&read(t, ".claude/skills/bundle.json")).unwrap();
    manifest["files"][".claude/skills/bp-old/SKILL.md"] = "0".into();
    write(t, ".claude/skills/bundle.json", &manifest.to_string());
    std::fs::create_dir_all(t.join(".claude/skills/bp-old")).unwrap();
    write(t, ".claude/skills/bp-old/SKILL.md", "old");
    assert!(install(t, Target::Claude).unwrap().0);
    assert!(!t.join(".claude/skills/bp-old").exists());
    assert_eq!(checked(t), "ok");
}

/// Each old manifest entry that leads out of the top: absolute, `..`, and through a symlinked directory.
#[test]
fn a_reinstall_removes_nothing_outside_its_directories() {
    let out = tempfile::tempdir().unwrap();
    let victim = out.path().join("victim.txt");
    let entries = [
        victim.display().to_string(),
        format!("{}{}", "../".repeat(40), victim.display().to_string().trim_start_matches('/')),
        ".claude/skills/bp-link/victim.txt".to_string(),
    ];
    for entry in entries {
        std::fs::write(&victim, "mine").unwrap();
        let dir = top();
        let t = dir.path();
        install(t, Target::Claude).unwrap();
        std::os::unix::fs::symlink(out.path(), t.join(".claude/skills/bp-link")).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_str(&read(t, ".claude/skills/bundle.json")).unwrap();
        manifest["files"][&entry] = "0".into();
        write(t, ".claude/skills/bundle.json", &manifest.to_string());
        let r = install(t, Target::Claude).unwrap_err();
        assert_eq!((r.code, r.exit, r.subject.as_str()), (Code::BundleInvalid, 2, entry.as_str()), "{r:?}");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "mine", "{entry}");
    }
    // Inside the top, but outside the target's directories: the user's own file.
    let dir = top();
    let t = dir.path();
    install(t, Target::Claude).unwrap();
    write(t, "mine.txt", "mine");
    let mut manifest: serde_json::Value = serde_json::from_str(&read(t, ".claude/skills/bundle.json")).unwrap();
    manifest["files"]["mine.txt"] = "0".into();
    write(t, ".claude/skills/bundle.json", &manifest.to_string());
    assert_eq!(install(t, Target::Claude).unwrap_err().subject, "mine.txt");
    assert_eq!(read(t, "mine.txt"), "mine");
}

#[test]
fn install_never_writes_through_a_symlink_out_of_the_top() {
    let out = tempfile::tempdir().unwrap();
    let dir = top();
    let t = dir.path();
    std::os::unix::fs::symlink(out.path(), t.join(".agents")).unwrap();
    let r = install(t, Target::Codex).unwrap_err();
    assert_eq!(r.code, Code::BundleInvalid, "{r:?}");
    assert!(r.subject.starts_with(".agents/"), "{r:?}");
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
    assert!(!t.join("AGENTS.md").exists(), "a refusal writes nothing");
}

#[test]
fn unpaired_markers_stop_the_install() {
    for text in ["# Mine\n<!-- bp:begin -->\nno end\n", "<!-- bp:end -->\n<!-- bp:begin -->\n", "<!-- bp:end -->\n"] {
        let dir = top();
        let t = dir.path();
        write(t, "CLAUDE.md", text);
        let r = install(t, Target::Claude).unwrap_err();
        assert_eq!(r.subject, "CLAUDE.md", "{text:?}");
        assert_eq!(read(t, "CLAUDE.md"), text);
    }
}

#[test]
fn codex_writes_its_own_paths_and_creates_an_excluded_agents_file() {
    let dir = top();
    let t = dir.path();
    let (_, v) = install(t, Target::Codex).unwrap();
    assert!(v["files"]["AGENTS.md#bp"].is_string() && v["files"]["hooks.json#hooks"].is_string(), "{v}");
    assert!(t.join(".agents/skills/bp-complete/SKILL.md").is_file());
    let hooks = read(t, ".codex/hooks.json");
    assert!(
        hooks.contains("--git-common-dir)/..\\\"/.codex/hooks/bp-start.sh"),
        "the main top, from any worktree: {hooks}"
    );
    assert!(!hooks.contains("onFailure") && !hooks.contains("permissions"), "both are Claude Code's: {hooks}");
    assert!(read(t, "AGENTS.md").starts_with("<!-- bp:begin -->\n"));
    assert!(read(t, ".git/info/exclude").lines().any(|l| l == "/AGENTS.md"), "a file the install creates is its own");
    assert_eq!(checked(t), "ok");
}

/// The files a clone holds before the install, path and text.
type Files<'a> = &'a [(&'a str, &'a str)];

/// Claude Code skips `AGENTS.md` when one of its own files exists, so the first of them imports it; with none, the
/// install creates no `CLAUDE.md` (27, design change 1).
#[test]
fn the_claude_target_imports_agents_md_into_the_first_claude_file() {
    let cases: [(Files, Option<&str>); 4] = [
        (&[], None),
        (&[(".claude/CLAUDE.md", "Mine.\n"), ("CLAUDE.local.md", "Mine.\n")], Some(".claude/CLAUDE.md")),
        (&[("CLAUDE.local.md", "Mine.")], Some("CLAUDE.local.md")),
        (&[("CLAUDE.md", "# Mine\n\n@AGENTS.md\n")], None),
    ];
    for (held, want) in cases {
        let dir = top();
        let t = dir.path();
        std::fs::create_dir_all(t.join(".claude")).unwrap();
        for (rel, text) in held {
            write(t, rel, text);
        }
        let (wrote, v) = install(t, Target::Claude).unwrap();
        assert!(wrote);
        let imports: Vec<String> = v["files"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| k.ends_with("#bp") && *k != "AGENTS.md#bp")
            .cloned()
            .collect();
        assert_eq!(imports, want.map(|w| format!("{w}#bp")).into_iter().collect::<Vec<_>>(), "{held:?}");
        for (rel, text) in held {
            let now = read(t, rel);
            if Some(*rel) == want {
                let sep = if text.ends_with('\n') { "\n" } else { "\n\n" };
                assert_eq!(now, format!("{text}{sep}{IMPORT}"), "{rel}");
            } else {
                assert_eq!(&now, text, "{rel} is not touched");
            }
        }
        assert_eq!(
            t.join("CLAUDE.md").exists(),
            held.iter().any(|(r, _)| *r == "CLAUDE.md"),
            "no CLAUDE.md is created"
        );
        assert_eq!(checked(t), "ok");
        assert!(!install(t, Target::Claude).unwrap().0, "a repeat exits 3: {held:?}");
    }
    // A Claude Code file added after the install hides the section, so the walk refuses until a reinstall imports it.
    let dir = top();
    let t = dir.path();
    install(t, Target::Claude).unwrap();
    write(t, "CLAUDE.md", "# Late\n");
    assert_eq!(checked(t), "CLAUDE.md#bp");
    assert!(install(t, Target::Claude).unwrap().0);
    assert_eq!(read(t, "CLAUDE.md"), format!("# Late\n\n{IMPORT}"));
    assert_eq!(checked(t), "ok");
}

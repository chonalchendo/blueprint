//! `bp list` intents and `--ready` through the binary, with a forge snapshot and a fixed clock.

use std::path::Path;

use assert_cmd::Command;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn issue(status: &str, depends: &str) -> String {
    format!("---\nkind: issue\ntitle: T\nstatus: {status}\ndepends: [{depends}]\n---\n\nBody.\n")
}

fn vault() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    std::fs::create_dir(r.join("shared")).unwrap();
    write(r, "projects/bp/issues/0001.ready.md", &issue("open", ""));
    write(r, "projects/bp/issues/0002.waits.md", &issue("open", "\"projects/bp/issues/0003.held\""));
    write(r, "projects/bp/issues/0003.held.md", &issue("in-progress", ""));
    write(
        r,
        "projects/bp/issues/0003.held/claims/s1.md",
        "---\nkind: claim\nunder: projects/bp/issues/0003.held\nfrom: \"2026-10-17T00:00:00Z\"\n---\n",
    );
    write(r, "projects/bp/issues/0004.done.md", &issue("resolved", ""));
    // A released claim no longer holds the issue; an escalated release blocks it (13, 21).
    let claim = |under: &str, reason: &str| {
        format!(
            "---\nkind: claim\nunder: {under}\nfrom: 2026-10-16T00:00:00Z\n---\n\n## Released\n\nto: 2026-10-16T01:00:00Z\nreason: {reason}\n"
        )
    };
    write(r, "projects/bp/issues/0005.dropped.md", &issue("open", ""));
    write(r, "projects/bp/issues/0005.dropped/claims/c.md", &claim("projects/bp/issues/0005.dropped", "dropped"));
    write(r, "projects/bp/issues/0006.escalated.md", &issue("open", ""));
    write(r, "projects/bp/issues/0006.escalated/claims/c.md", &claim("projects/bp/issues/0006.escalated", "escalated"));
    // A re-review whose head sorts first, an approval of an older head, and two checks at one head.
    let pr7 = "projects/bp/issues/0007.reviewed";
    write(r, &format!("{pr7}.md"), &issue("in-progress", ""));
    for head in ["aaaa", "ffff"] {
        write(r, &format!("{pr7}/reviews/{head}.md"), &format!("---\nkind: review\nunder: {pr7}\nhead: {head}\n---\n"));
    }
    let pr8 = "projects/bp/issues/0008.twochecks";
    write(r, &format!("{pr8}.md"), &issue("in-progress", ""));
    write(r, &format!("{pr8}/reviews/bbbb.md"), &format!("---\nkind: review\nunder: {pr8}\nhead: bbbb\n---\n"));
    for (name, at, verdict) in [("direct-1", "06", "fail"), ("direct-2", "07", "pass")] {
        write(
            r,
            &format!("{pr8}/runs/{name}-bbbb-1.md"),
            &format!(
                "---\nkind: run\nunder: {pr8}\ncheck: {pr8}/{name}\ncommit: bbbb\nattempt: 1\nat: 2026-10-17T{at}:00:00Z\nverdict: {verdict}\n---\n"
            ),
        );
    }
    // Needs landing: the reviewer's approval, green, and a passing run of the one `error` block's text at the head;
    // no review record, as `land` reads none. The twin's approval is the author's, which counts for nothing.
    write(r, "projects/bp/project.md", "---\nkind: project\ntitle: Bp\nstatus: active\nreviewer: r\n---\n");
    for slug in ["0009.lands", "0010.twin"] {
        let id = format!("projects/bp/issues/{slug}");
        let block = "## Check\n\n```check shell {name=hello expect=0}\ntrue\n```\n";
        let hash = bp_check::hash(bp_check::named(&bp_vault::Body::parse(block))[0]);
        write(r, &format!("{id}.md"), &format!("{}\n{block}", issue("in-progress", "")));
        if slug == "0010.twin" {
            write(r, &format!("{id}/reviews/cccc.md"), &format!("---\nkind: review\nunder: {id}\nhead: cccc\n---\n"));
        }
        write(
            r,
            &format!("{id}/runs/hello-cccc-1.md"),
            &format!(
                "---\nkind: run\nunder: {id}\ncheck: hello\ncheck_hash: {hash}\ncommit: cccc\nattempt: 1\nverdict: pass\n---\n"
            ),
        );
    }
    write(r, "projects/bp/milestones/m.md", "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n");
    let green = r#""statusCheckRollup":[{"conclusion":"SUCCESS"}]"#;
    let old_approval = r#""reviews":[{"state":"APPROVED","commit":{"oid":"old0"}}]"#;
    write(
        r,
        "forge/open-prs.json",
        &format!(
            r#"[{{"number":9,"headRefName":"bp/0003.held","headRefOid":"h"}},
{{"number":10,"headRefName":"bp/0007.reviewed","headRefOid":"aaaa",{green},{old_approval}}},
{{"number":11,"headRefName":"bp/0008.twochecks","headRefOid":"bbbb",{green},"reviews":[{{"state":"APPROVED","commit":{{"oid":"bbbb"}}}}]}},
{{"number":12,"headRefName":"bp/0009.lands","headRefOid":"cccc",{green},"reviews":[{{"state":"APPROVED","author":{{"login":"r"}},"commit":{{"oid":"cccc"}}}}]}},
{{"number":13,"headRefName":"bp/0010.twin","headRefOid":"cccc",{green},"reviews":[{{"state":"APPROVED","author":{{"login":"a"}},"commit":{{"oid":"cccc"}}}}]}}]"#
        ),
    );
    d
}

fn list(root: &Path, args: &[&str], forge: bool) -> serde_json::Value {
    let mut cmd = Command::cargo_bin("bp").unwrap();
    cmd.current_dir(root).env_remove("BP_VAULT").env("BP_NOW", "2026-10-18T00:00:00Z").env_remove("BP_FORGE_SNAPSHOT");
    if forge {
        cmd.env("BP_FORGE_SNAPSHOT", root.join("forge"));
    }
    let out = cmd.arg("list").args(args).arg("--json").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn intent<'a>(v: &'a serde_json::Value, id: &str) -> (&'a str, &'a str) {
    let row = v["rows"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap();
    (row["intent"].as_str().unwrap_or("-"), row["note"].as_str().unwrap_or("-"))
}

#[test]
fn every_live_row_carries_its_intent() {
    let d = vault();
    let v = list(d.path(), &["issue"], false);
    assert_eq!(v["forge"], "not read");
    assert_eq!(intent(&v, "projects/bp/issues/0001.ready"), ("ready", ""));
    assert_eq!(intent(&v, "projects/bp/issues/0002.waits"), ("waiting", "waits for projects/bp/issues/0003.held"));
    assert_eq!(intent(&v, "projects/bp/issues/0003.held"), ("working", "stale"));
    assert_eq!(intent(&v, "projects/bp/issues/0004.done"), ("-", "-"));
    assert_eq!(intent(&v, "projects/bp/issues/0005.dropped"), ("ready", ""));
    assert_eq!(intent(&v, "projects/bp/issues/0006.escalated"), ("blocked", "escalated"));
    let v = list(d.path(), &["milestone"], false);
    assert_eq!(intent(&v, "projects/bp/milestones/m"), ("needs-milestone-approval", ""));
}

#[test]
fn the_forge_snapshot_moves_the_review_path_and_unblocks_dependents() {
    let d = vault();
    let v = list(d.path(), &["issue"], true);
    assert_eq!(v["forge"], "snapshot");
    assert_eq!(intent(&v, "projects/bp/issues/0003.held"), ("needs-review", ""));
    assert_eq!(
        intent(&v, "projects/bp/issues/0002.waits"),
        ("ready", ""),
        "an open pull request on the target is enough"
    );
    assert_eq!(
        intent(&v, "projects/bp/issues/0007.reviewed"),
        ("needs-approval", ""),
        "an old approval needs renewing"
    );
    assert_eq!(intent(&v, "projects/bp/issues/0008.twochecks"), ("blocked", "run failed at head"));
    assert_eq!(intent(&v, "projects/bp/issues/0009.lands"), ("needs-landing", ""), "land's own predicate");
    assert_eq!(intent(&v, "projects/bp/issues/0010.twin"), ("needs-approval", ""), "only the reviewer's counts");
    // The vault's own snapshot reads the same, with its age; a project with none reads `not read`.
    let own = d.path().join(".bp/forge/bp");
    std::fs::create_dir_all(&own).unwrap();
    std::fs::copy(d.path().join("forge/open-prs.json"), own.join("open-prs.json")).unwrap();
    let v = list(d.path(), &["issue"], false);
    assert!(v["forge"] == "snapshot" && v["forge_age"].is_i64(), "{v}");
    assert_eq!(intent(&v, "projects/bp/issues/0003.held"), ("needs-review", ""));
    let v = list(d.path(), &["issue", "--project", "other"], false);
    assert!(v["forge"] == "not read" && v.get("forge_age").is_none(), "{v}");
    // A bad file of a project out of view breaks no list: only the projects in view are read.
    std::fs::create_dir_all(d.path().join(".bp/forge/zzz")).unwrap();
    std::fs::write(d.path().join(".bp/forge/zzz/open-prs.json"), "garbage").unwrap();
    let v = list(d.path(), &["issue", "--project", "bp"], false);
    assert!(v["forge"] == "snapshot" && v["forge_age"].is_i64(), "{v}");
    let v = list(d.path(), &["issue"], false);
    assert!(v["forge"] == "snapshot" && v["forge_age"].is_i64(), "with no --project, the rows' projects: {v}");
}

#[test]
fn ready_filters_before_the_limit() {
    let d = vault();
    let v = list(d.path(), &["issue", "--ready"], false);
    let ids: Vec<&str> = v["rows"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["projects/bp/issues/0001.ready", "projects/bp/issues/0005.dropped"]);
    assert_eq!((v["total"].as_u64(), v["truncated"].as_bool()), (Some(2), Some(false)));
    let v = list(d.path(), &["issue", "--ready", "--limit", "0"], true);
    assert_eq!((v["total"].as_u64(), v["truncated"].as_bool()), (Some(3), Some(true)));
}

fn bp(root: &Path, envs: &[(&str, &Path)], args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let mut cmd = Command::cargo_bin("bp").unwrap();
    cmd.current_dir(root).env_remove("BP_VAULT").env_remove("BP_NOW").env_remove("BP_FORGE_SNAPSHOT");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.args(args).arg("--json").output().unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap())
}

#[test]
fn ready_lists_issues_only() {
    let d = vault();
    let (code, v) = bp(d.path(), &[], &["list", "milestone", "--ready"]);
    assert_eq!((code, v["code"].as_str()), (Some(4), Some("usage")));
}

#[test]
fn a_bad_clock_is_usage_and_a_bad_snapshot_names_its_file() {
    let d = vault();
    let (code, v) = bp(d.path(), &[("BP_NOW", Path::new("yesterday"))], &["list", "issue"]);
    assert_eq!((code, v["code"].as_str()), (Some(4), Some("usage")));
    let none = d.path().join("none");
    let (code, v) = bp(d.path(), &[("BP_FORGE_SNAPSHOT", &none)], &["list", "issue"]);
    assert_eq!((code, v["code"].as_str()), (Some(1), Some("could_not_run")));
    assert!(v["subject"].as_str().unwrap().ends_with("open-prs.json"), "{v}");
    let (code, _) = bp(d.path(), &[("BP_FORGE_SNAPSHOT", &none)], &["list", "milestone"]);
    assert_eq!(code, Some(0), "a milestone list never reads the forge");
}

#[test]
fn a_walk_reads_needs_derivation_inside_its_one_read() {
    // Needs derivation lists the milestone's issues inside the walk's read, so the list opens no second transaction.
    let d = vault();
    let r = d.path();
    let block = "## Finish line\n\n```check shell {name=open expect=0}\ntrue\n```\n";
    write(
        r,
        "projects/bp/milestones/a.md",
        &format!("---\nkind: milestone\ntitle: A\nstatus: approved\n---\n\n{block}"),
    );
    write(r, "projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n\nBody.\n");
    let under = "---\nkind: issue\ntitle: U\nstatus: open\nunder: projects/bp/milestones/a\ncites: [projects/bp/designs/system]\n---\n\nBody.\n";
    write(r, "projects/bp/issues/0011.under.md", under);
    let forge = r.join("forge");
    let (code, v) =
        bp(r, &[("BP_FORGE_SNAPSHOT", &forge)], &["walk", "projects/bp/issues/0011.under", "--for", "complete"]);
    assert_eq!(code, Some(0), "{v}");
    let a = v["intents"].as_array().unwrap().iter().find(|i| i["id"] == "projects/bp/milestones/a").unwrap();
    assert_eq!(a["intent"], "needs-derivation", "{v}");
}

#[test]
fn needs_shaping_is_one_row_per_project_on_its_root_design() {
    let d = vault();
    let r = d.path();
    let design = |status: &str| format!("---\nkind: design\ntitle: D\nstatus: {status}\n---\n\nBody.\n");
    write(r, "projects/q/project.md", "---\nkind: project\ntitle: Q\nstatus: active\n---\n");
    write(r, "projects/q/designs/product.md", &design("active"));
    write(r, "projects/q/designs/components/x.md", &design("active"));
    let answer = "---\nkind: concern-answer\nunder: projects/q/project\nconcern: c\nstate: open\ndecided_at: 2026-10-19T10:00:00Z\n---\n";
    write(r, "projects/q/concerns/c/2026-10-19T10-00-00Z.md", answer);
    let v = list(r, &["design", "--project", "q"], false);
    assert_eq!(
        intent(&v, "projects/q/designs/product"),
        ("needs-shaping", "c open; bp walk projects/q/designs/product --for shape")
    );
    assert_eq!(intent(&v, "projects/q/designs/components/x"), ("-", "-"), "one node per project");
    // A system design is the root once it exists, and an inactive root reads nothing.
    write(r, "projects/q/designs/system.md", &design("draft"));
    let v = list(r, &["design", "--project", "q"], false);
    assert_eq!(intent(&v, "projects/q/designs/product"), ("-", "-"));
    assert_eq!(intent(&v, "projects/q/designs/system"), ("-", "-"));
    let v = list(r, &["concern-answer", "--status", "open"], false);
    assert_eq!(v["rows"][0]["status"], "open", "{v}");
}

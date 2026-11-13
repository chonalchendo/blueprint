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
    // A converted review reads round 1 and `conformance`; a medium head needs the failure-paths lens too (37).
    let lens = |under: &str, head: &str| {
        format!("---\nkind: review\nunder: {under}\nhead: {head}\nlens: failure-paths\n---\n")
    };
    write(r, &format!("{pr7}/reviews/aaaa-2.md"), &lens(pr7, "aaaa"));
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
        let hash = bp_vault::Body::parse(block).named_checks()[0].hash();
        write(r, &format!("{id}.md"), &format!("{}\n{block}", issue("in-progress", "")));
        if slug == "0010.twin" {
            write(r, &format!("{id}/reviews/cccc.md"), &format!("---\nkind: review\nunder: {id}\nhead: cccc\n---\n"));
            write(r, &format!("{id}/reviews/cccc-2.md"), &lens(&id, "cccc"));
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
    let lenses = "round 1: missing lenses: conformance, failure-paths";
    assert_eq!(intent(&v, "projects/bp/issues/0003.held"), ("needs-review", lenses));
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
    assert_eq!(intent(&v, "projects/bp/issues/0003.held"), ("needs-review", lenses));
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
    assert_eq!((a.get("model"), a.get("effort")), (None, None), "the session derives on its own model: {v}");
    let own = v["intents"].as_array().unwrap().iter().find(|i| i["id"] == "projects/bp/issues/0011.under").unwrap();
    assert_eq!(
        (own["intent"].as_str(), own["model"].as_str(), own["effort"].as_str()),
        (Some("ready"), Some("sonnet"), Some("high"))
    );
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

#[test]
fn a_designed_concern_needs_shaping_until_a_milestone_stamped_after_it() {
    let d = vault();
    let r = d.path();
    write(r, "projects/q/project.md", "---\nkind: project\ntitle: Q\nstatus: active\n---\n");
    write(r, "projects/q/designs/product.md", "---\nkind: design\ntitle: D\nstatus: active\n---\n\nBody.\n");
    let answer = "---\nkind: concern-answer\nunder: projects/q/project\nconcern: c\nstate: designed\n\
                  design: projects/q/designs/product\ndecided_at: 2026-10-19T10:00:00Z\n---\n";
    write(r, "projects/q/concerns/c/2026-10-19T10-00-00Z.md", answer);
    write(r, "projects/q/milestones/m.md", "---\nkind: milestone\ntitle: M\nstatus: done\n---\n\nGoal.\n");
    let version = |n: u32, m: &str, at: &str| {
        let text = format!(
            "---\nkind: blueprint-version\nunder: projects/q/project\nmilestone: projects/q/milestones/{m}\nn: {n}\n\
             tag: blueprint/q/{n}\naccepted_at: {at}\nmanifest: {{}}\n---\n"
        );
        write(r, &format!("projects/q/versions/{n}.md"), &text);
    };
    version(1, "m", "2026-10-18T10:00:00Z");
    let v = list(r, &["design", "--project", "q"], false);
    let note = "c designed; bp walk projects/q/designs/product --for shape";
    assert_eq!(intent(&v, "projects/q/designs/product"), ("needs-shaping", note), "a stamp before the answer");
    // Only a `done` milestone's stamp proves it.
    write(r, "projects/q/milestones/n.md", "---\nkind: milestone\ntitle: N\nstatus: abandoned\n---\n\nGoal.\n");
    version(2, "n", "2026-10-20T10:00:00Z");
    let v = list(r, &["design", "--project", "q"], false);
    assert_eq!(intent(&v, "projects/q/designs/product"), ("needs-shaping", note), "a stamp of a milestone not done");
    version(3, "m", "2026-10-20T10:00:00Z");
    let v = list(r, &["design", "--project", "q"], false);
    assert_eq!(intent(&v, "projects/q/designs/product"), ("-", "-"), "a stamp after the answer proves it");
    // A `decided_at` that is not an instant falls back to the record's stamp.
    let dated = "---\nkind: concern-answer\nunder: projects/q/project\nconcern: e\nstate: designed\n\
                 design: projects/q/designs/product\ndecided_at: 2026-10-30\n---\n";
    write(r, "projects/q/concerns/e/2026-10-30T09-00-00Z.md", dated);
    let v = list(r, &["design", "--project", "q"], false);
    let note = "e designed; bp walk projects/q/designs/product --for shape";
    assert_eq!(intent(&v, "projects/q/designs/product"), ("needs-shaping", note), "a date before any later stamp");
    version(4, "m", "2026-10-30T10:00:00Z");
    let v = list(r, &["design", "--project", "q"], false);
    assert_eq!(intent(&v, "projects/q/designs/product"), ("-", "-"), "a stamp after the record's own stamp");
}

/// The issue's `risk` label and a pending delta it caused reach needs review: each reads high, so round one asks for
/// the tests-and-checks lens (37, design changes 1 and 8; review of #98, finding 11 d and e).
#[test]
fn the_label_and_a_pending_delta_reach_the_lenses() {
    let d = vault();
    let r = d.path();
    let labelled = "projects/bp/issues/0011.labelled";
    let caused = "projects/bp/issues/0012.caused";
    write(r, &format!("{labelled}.md"), "---\nkind: issue\ntitle: L\nstatus: in-progress\nrisk: high\n---\n\nBody.\n");
    write(r, &format!("{caused}.md"), &issue("in-progress", ""));
    write(r, "projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n\n## One\n\nOld.\n");
    let delta =
        format!("---\nkind: delta\nunder: projects/bp/designs/system\ncause: {caused}\n---\n\n## One\n\nNew.\n");
    write(r, "projects/bp/designs/system/deltas/d.md", &delta);
    let prs = r.join("forge/open-prs.json");
    let mut open: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&prs).unwrap()).unwrap();
    for (n, slug) in [(14, "0011.labelled"), (15, "0012.caused")] {
        open.push(serde_json::json!({ "number": n, "headRefName": format!("bp/{slug}"), "headRefOid": "dddd" }));
    }
    std::fs::write(&prs, serde_json::to_string(&open).unwrap()).unwrap();
    let v = list(r, &["issue"], true);
    let note = "round 1: missing lenses: conformance, failure-paths, tests-and-checks";
    assert_eq!(intent(&v, labelled), ("needs-review", note), "the label");
    assert_eq!(intent(&v, caused), ("needs-review", note), "the delta");
    assert_eq!(intent(&v, "projects/bp/issues/0003.held").1, "round 1: missing lenses: conformance, failure-paths");
}

/// A row that leads to a dispatch carries the model and effort of 36, section 3; one that leads to none carries
/// neither (37, design change 9).
#[test]
fn list_and_walk_rows_carry_model_and_effort() {
    let d = vault();
    let r = d.path();
    let labelled = |slug: &str, label: &str| {
        let text = format!("---\nkind: issue\ntitle: L\nstatus: open\n{label}\n---\n\nBody.\n");
        write(r, &format!("projects/bp/issues/{slug}.md"), &text);
    };
    labelled("0021.low", "risk: low");
    labelled("0022.high", "risk: high");
    labelled("0023.hard", "complex: true");
    let v = list(r, &["issue"], false);
    let route = |id: &str| {
        let row = v["rows"].as_array().unwrap().iter().find(|row| row["id"] == id).unwrap();
        (row["model"].as_str().unwrap_or("-"), row["effort"].as_str().unwrap_or("-"))
    };
    assert_eq!(route("projects/bp/issues/0001.ready"), ("sonnet", "high"), "no label reads medium");
    assert_eq!(route("projects/bp/issues/0021.low"), ("sonnet", "medium"));
    assert_eq!(route("projects/bp/issues/0022.high"), ("opus", "high"));
    assert_eq!(route("projects/bp/issues/0023.hard"), ("fable", "high"), "complex");
    assert_eq!(route("projects/bp/issues/0002.waits"), ("-", "-"), "a waiting issue dispatches no one");
    assert_eq!(route("projects/bp/issues/0003.held"), ("-", "-"));
}

/// A ready issue routes by the level the review path reads: a pending delta it caused raises it with no pull request
/// open. A label that `set` would refuse reads as the costly value, and the note names it (21, rule 1; 36, section 3).
#[test]
fn a_ready_issue_routes_by_its_delta_and_reads_a_bad_label_as_the_costly_one() {
    let d = vault();
    let r = d.path();
    let labelled = |slug: &str, label: &str| {
        let text = format!("---\nkind: issue\ntitle: L\nstatus: open\n{label}\n---\n\nBody.\n");
        write(r, &format!("projects/bp/issues/{slug}.md"), &text);
    };
    labelled("0031.caused", "risk: medium");
    labelled("0032.shout", "risk: High");
    labelled("0033.list", "risk: [high]");
    labelled("0034.word", "complex: yes");
    labelled("0035.null", "complex: ~");
    labelled("0036.fine", "complex: false");
    write(r, "projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n\n## One\n\nOld.\n");
    let delta = "---\nkind: delta\nunder: projects/bp/designs/system\ncause: projects/bp/issues/0031.caused\n---\n\n## One\n\nNew.\n";
    write(r, "projects/bp/designs/system/deltas/d.md", delta);
    let v = list(r, &["issue"], false);
    let row = |id: &str| v["rows"].as_array().unwrap().iter().find(|row| row["id"] == id).unwrap().clone();
    let route = |slug: &str| {
        let row = row(&format!("projects/bp/issues/{slug}"));
        (row["model"].as_str().unwrap().to_string(), row["effort"].as_str().unwrap().to_string())
    };
    let is = |m: &str, e: &str| (m.to_string(), e.to_string());
    assert_eq!(route("0031.caused"), is("opus", "high"), "a pending delta the issue caused");
    assert_eq!(route("0032.shout"), is("opus", "high"), "risk High reads high");
    assert_eq!(route("0033.list"), is("opus", "high"));
    assert_eq!(route("0034.word"), is("fable", "high"), "complex yes reads true");
    assert_eq!(route("0035.null"), is("fable", "high"));
    assert_eq!(route("0036.fine"), is("sonnet", "high"));
    let note = |slug: &str| row(&format!("projects/bp/issues/{slug}"))["note"].as_str().unwrap_or("").to_string();
    assert!(note("0032.shout").contains("risk \"High\" is not low, medium or high: read as high"), "{v}");
    assert!(note("0034.word").contains("complex \"yes\" is not true or false: read as true"), "{v}");
    assert_eq!(note("0036.fine"), "");
}

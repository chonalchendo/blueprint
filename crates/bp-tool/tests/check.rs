//! `bp check` through the binary: a vault under git and a separate code repository, as the agent runs it. One block
//! per verdict and attribute, one commit of every run record, the next attempt, and no lock while a block runs.

use std::path::Path;

mod common;

use common::{bp, bp_env, commits, git};

const I: &str = "projects/bp/issues/0001.i";
const J: &str = "projects/bp/issues/0002.j";

const BLOCKS: &str = "~~~check shell {name=p expect=0}
true
~~~
~~~check shell {name=f expect=0}
false
~~~
~~~check shell {name=two expect=2}
exit 2
~~~
~~~check scenario {name=s measure=count max=3}
echo 2
~~~
~~~check scenario {name=sf measure=count max=3}
echo 5
~~~
~~~check shell {name=w severity=warn expect=0}
false
~~~
~~~check shell {name=t expect=0 timeout=1}
sleep 30
~~~
~~~check shell {name=q expect=fail until=2099-01-01}
false
~~~
~~~check shell {name=lapsed expect=fail until=2026-10-19}
false
~~~
~~~check shell {name=pl expect=0 when=post-land}
true
~~~
~~~check deps {name=d}
bp-tool -> bp-vault
~~~
~~~check shell {name=bad expect=maybe}
true
~~~
";

/// The vault, whose issue holds the blocks plus one that proves where a block runs, and the code repository.
fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let place =
        "~~~check shell {name=place expect=0}\ntest -f marker.txt\ntest -f \"$BP_VAULT/projects/bp/project.md\"\n~~~\n";
    let issue = format!("---\nkind: issue\ntitle: I\nstatus: open\n---\n\n## Check\n\n{BLOCKS}{place}");
    let vault = common::vault(&[
        ("projects/bp/project.md", "---\nkind: project\ntitle: bp\nstatus: active\n---\n"),
        ("projects/bp/issues/0001.i.md", &issue),
        ("projects/bp/issues/0002.j.md", "---\nkind: issue\ntitle: J\nstatus: open\n---\n"),
        ("shared/threads/t.md", "---\nkind: thread\ntitle: T\nstatus: open\n---\n"),
    ]);
    (vault, common::vault(&[("marker.txt", "here\n")]))
}

fn check(vault: &Path, code: &Path, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let v = vault.to_str().unwrap();
    bp_env(code, &[("BP_VAULT", v)], &[&["check"], args].concat())
}

/// A run record's header, through `show`.
fn show(vault: &Path, id: &str) -> serde_json::Value {
    let (exit, v) = bp(vault, &["show", id]);
    assert_eq!(exit, Some(0), "{v}");
    v
}

#[test]
fn each_block_gets_its_verdict_in_one_commit() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let sha = git(code, &["rev-parse", "HEAD"]).trim().to_string();
    let before = commits(vault);
    let (exit, v) = check(vault, code, &[I]);
    assert_eq!(exit, Some(2), "{v}");
    assert_eq!(v["code"], "check_failed");
    assert_eq!(v["subject"], format!("{I}/runs/f-{sha}-1"), "the first failed run");
    for (name, verdict) in [
        ("p", "pass"),
        ("f", "fail"),
        ("two", "pass"),
        ("s", "pass"),
        ("sf", "fail"),
        ("w", "fail"),
        ("t", "timed-out"),
        ("q", "pass"),
        ("lapsed", "fail"),
        ("pl", "skipped"),
        ("d", "skipped"),
        ("bad", "could-not-run"),
        ("place", "pass"),
    ] {
        let run = show(vault, &format!("{I}/runs/{name}-{sha}-1"));
        assert_eq!(run["verdict"], verdict, "{name}: {run}");
    }
    assert_eq!(commits(vault).parse::<u32>().unwrap(), before.parse::<u32>().unwrap() + 1, "one commit");
    let message = git(vault, &["log", "-1", "--format=%B"]);
    assert!(message.contains("Bp-Verb: check\n") && message.contains("Bp-Role: human"), "{message}");
    assert!(git(vault, &["status", "--porcelain"]).is_empty());

    let run = show(vault, &format!("{I}/runs/f-{sha}-1"));
    assert_eq!((run["kind"].as_str(), run["check"].as_str()), (Some("run"), Some("f")));
    assert_eq!((run["commit"].as_str(), run["attempt"].as_u64()), (Some(sha.as_str()), Some(1)));
    assert_eq!(
        (run["at"].as_str(), run["ended"].as_str()),
        (Some("2026-10-19T08:00:00Z"), Some("2026-10-19T08:00:00Z"))
    );
    for hash in ["check_hash", "evidence"] {
        assert_eq!(run[hash].as_str().map(str::len), Some(64), "{hash}: {run}");
    }
    assert!(show(vault, &format!("{I}/runs/d-{sha}-1")).get("evidence").is_none(), "nothing ran");
    let body = std::fs::read_to_string(vault.join(format!("{I}/runs/sf-{sha}-1.md"))).unwrap();
    assert!(body.contains("measure 5 over max 3") && body.contains("\n```\n5\n```\n"), "{body}");
}

#[test]
fn a_rerun_takes_the_next_attempt_and_a_warn_failure_alone_exits_0() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let sha = git(code, &["rev-parse", "HEAD"]).trim().to_string();
    for attempt in 1..=2 {
        let (exit, v) = check(vault, code, &[I, "--name", "w"]);
        assert_eq!(exit, Some(0), "{v}");
        let run = &v["runs"][0];
        assert_eq!((run["name"].as_str(), run["verdict"].as_str()), (Some("w"), Some("fail")), "{v}");
        assert_eq!(run["id"], format!("{I}/runs/w-{sha}-{attempt}"));
        assert_eq!(v["commit"].as_str().unwrap(), git(vault, &["rev-parse", "HEAD"]).trim());
    }
    assert_eq!(show(vault, &format!("{I}/runs/w-{sha}-2"))["attempt"], 2);
}

#[test]
fn a_block_may_write_the_vault_while_it_runs() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let bp = assert_cmd::cargo::cargo_bin("bp");
    let block =
        format!("~~~check shell {{name=writes expect=0}}\n{} set {J} severity high --json\n~~~\n", bp.display());
    let text = format!("---\nkind: issue\ntitle: J\nstatus: open\n---\n\n## Check\n\n{block}");
    std::fs::write(vault.join(format!("{J}.md")), text).unwrap();
    git(vault, &["commit", "-qam", "block"]);
    let before: u32 = commits(vault).parse().unwrap();
    let (exit, v) = check(vault, code, &[J]);
    assert_eq!(exit, Some(0), "{v}");
    assert_eq!(v["runs"][0]["verdict"], "pass", "{v}");
    assert_eq!(commits(vault).parse::<u32>().unwrap(), before + 2, "the block's write, then the run record");
}

#[test]
fn check_refuses_what_it_cannot_run() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let before = commits(vault);
    let (exit, v) = check(vault, code, &[I, "--name", "nope"]);
    assert_eq!((exit, v["code"].as_str()), (Some(4), Some("usage")), "{v}");
    let (exit, v) = check(vault, code, &["shared/threads/t"]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{v}");
    let (exit, v) = check(vault, code, &[J]);
    assert_eq!((exit, v["runs"].as_array().map(Vec::len)), (Some(0), Some(0)), "no block, no write: {v}");
    let elsewhere = tempfile::tempdir().unwrap();
    let (exit, v) = check(vault, elsewhere.path(), &[I]);
    assert_eq!((exit, v["code"].as_str()), (Some(1), Some("could_not_run")), "no code repository: {v}");
    assert_eq!(commits(vault), before);
}

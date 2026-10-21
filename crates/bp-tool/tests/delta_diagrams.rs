//! A delta carries its diagrams (28, design change 7) through the binary: `record delta --diagram` and
//! `--remove-diagram`, the `nodes` check on the proposed body and specs together, `pr` listing each diagram, and
//! `stamp` writing, rendering and removing them, with the manifest pinning each spec and a drill page drawn again.

use std::path::Path;

mod common;

use common::{bp, bp_env, git};
use serde_json::{Value, json};

const M: &str = "projects/bp/milestones/m";
const ISSUE: &str = "projects/bp/issues/i";
const SYSTEM: &str = "projects/bp/designs/system";
const FRONT: &str = "projects/bp/designs/containers/front";

/// A container view of the front: a glossary term and a noun of the front's body.
const KEEP: &str = "type: architecture\nlevel: container\ntitle: Front\nlede: Two boxes.\nnodes:\n  - {id: h, term: \
                    harness, at: [0, 0]}\n  - {id: v, noun: Vault, at: [1, 0]}\n";
const OLD: &str = "type: architecture\nlevel: container\ntitle: Old\nlede: One box.\nnodes:\n  - {id: h, term: harness, at: [0, 0]}\n";
/// A component view inside the front, naming a noun only the proposed body has.
const NEW: &str = "type: architecture\nlevel: component\ntitle: Index\nlede: One box.\nnodes:\n  - {id: i, noun: Index, at: [0, 0]}\n";
/// The system's container view: its `Front` box drills to the front's component views.
const SYS: &str = "type: architecture\nlevel: container\ntitle: System\nlede: One box.\nnodes:\n  - {id: f, noun: Front, at: [0, 0]}\n";

fn design(title: &str, under: &str, body: &str) -> String {
    format!("---\nkind: design\ntitle: {title}\nstatus: active\nunder: {under}\n---\n\n{body}")
}

/// A project with a glossary, an active system design with a container view, an active front design with two views
/// and a page each, a planned milestone with one passing block, an open issue under it with one block, and `extra`.
fn vault(extra: &[(&str, &str)]) -> tempfile::TempDir {
    let milestone = "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n\n## Finish line\n\n\
                     ~~~check shell {name=p expect=0}\ntrue\n~~~\n";
    let issue = "---\nkind: issue\ntitle: I\nstatus: open\nunder: projects/bp/milestones/m\n---\n\nBody.\n\n## Check\n\n\
                 ```check shell {name=hello expect=0}\ntrue\n```\n";
    let (keep, old, sys) =
        (format!("{FRONT}/diagrams/keep"), format!("{FRONT}/diagrams/old"), format!("{SYSTEM}/diagrams/containers"));
    let files = [
        ("projects/bp/designs/glossary.md".to_string(), design("G", SYSTEM, "- `harness` — runs the agents.\n")),
        (format!("{SYSTEM}.md"), "---\nkind: design\ntitle: S\nstatus: active\n---\n\n## Front\n\nThe front.\n".into()),
        (format!("{FRONT}.md"), design("Front", SYSTEM, "The front.\n\n## Vault\n\nThe files.\n")),
        (format!("{keep}.yaml"), KEEP.into()),
        (format!("{keep}.html"), "<p>keep</p>\n".into()),
        (format!("{old}.yaml"), OLD.into()),
        (format!("{old}.html"), "<p>old</p>\n".into()),
        (format!("{sys}.yaml"), SYS.into()),
        (format!("{sys}.html"), "<p>system</p>\n".into()),
        (format!("{M}.md"), milestone.into()),
        (format!("{ISSUE}.md"), issue.into()),
    ];
    let mut all: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    all.push(("projects/bp/project.md", "---\nkind: project\ntitle: bp\nstatus: active\n---\n"));
    all.push(("shared/.keep", ""));
    all.extend(extra);
    common::vault(&all)
}

/// Writes each (name, text) into `dir` and returns their paths.
fn files(dir: &Path, texts: &[(&str, &str)]) -> Vec<String> {
    texts
        .iter()
        .map(|(name, text)| {
            std::fs::write(dir.join(name), text).unwrap();
            dir.join(name).display().to_string()
        })
        .collect()
}

/// `bp record delta <FRONT> --field milestone=M` with the body and `extra` flags.
fn record(root: &Path, body: &str, extra: &[&str]) -> (Option<i32>, Value) {
    let ms = format!("milestone={M}");
    let cause = format!("cause={ISSUE}");
    let args = ["record", "delta", FRONT, "--field", &ms, "--field", &cause, "--field", "change=views", "--body-file"];
    bp(root, &[&args[..], &[body], extra].concat())
}

const BODY: &str = "The front.\n\n## Vault\n\nThe files.\n\n## Index\n\nThe index.\n";

#[test]
fn a_delta_carries_its_diagrams_through_record_and_stamp() {
    let v = vault(&[]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    let tmp = tempfile::tempdir().unwrap();
    let dropped = BODY.replace("## Vault\n\nThe files.\n\n", "");
    let unknown = NEW.replace("noun: Index", "term: no-such-term");
    let [body, dropped, plain, new, keep, unknown] = &files(
        tmp.path(),
        &[
            ("body.md", BODY),
            ("dropped.md", &dropped),
            ("plain.md", "The front.\n\n## Vault\n\nThe files.\n"),
            ("new.yaml", NEW),
            ("keep.yaml", KEEP),
            ("u.yaml", unknown.as_str()),
        ],
    )[..] else {
        unreachable!()
    };
    let before = common::commits(root);
    let new_flag = format!("new={new}");

    // The body drops `Vault`, which the kept view names: the nodes check refuses before anything is written.
    let (exit, v) = record(root, dropped, &["--diagram", &new_flag]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("diagram_invalid")), "{v}");
    assert_eq!(v["subject"], format!("{FRONT}/diagrams/keep.yaml#v"), "{v}");
    assert_eq!(v["findings"][0]["rule"], "nodes/resolve", "{v}");
    // A body without `Index` leaves the new view's box unnamed; a term outside the glossary names nothing either.
    let (exit, v) = record(root, plain, &["--diagram", &new_flag]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("diagram_invalid")), "{v}");
    assert_eq!(v["subject"], format!("{new}#i"), "{v}");
    let (_, v) = record(root, body, &["--diagram", &format!("new={unknown}")]);
    assert_eq!(v["code"], "diagram_invalid", "{v}");
    for (flags, why) in [
        (vec!["--diagram", "new"], "no file"),
        (vec!["--diagram", &new_flag, "--remove-diagram", "new"], "one name twice"),
        (vec!["--remove-diagram", "Bad.name"], "not a name"),
    ] {
        assert_eq!(record(root, body, &flags).1["code"], "usage", "{why}");
    }
    let (exit, v) =
        bp(root, &["record", "review", ISSUE, "--field", &format!("head={}", "a".repeat(40)), "--diagram", &new_flag]);
    assert_eq!((exit, v["code"].as_str()), (Some(4), Some("usage")), "only a delta carries diagrams: {v}");
    assert_eq!(common::commits(root), before, "no refusal writes");

    let (exit, v) =
        record(root, body, &["--diagram", &new_flag, "--diagram", &format!("keep={keep}"), "--remove-diagram", "old"]);
    assert_eq!(exit, Some(0), "{v}");
    let delta = v["id"].as_str().unwrap().to_string();
    assert_eq!(std::fs::read_to_string(root.join(&delta).join("new.yaml")).unwrap(), NEW, "the spec in the delta");
    let shown = bp(root, &["show", &delta]).1;
    assert_eq!((&shown["diagrams"], &shown["remove_diagrams"]), (&json!(["keep", "new"]), &json!(["old"])), "{shown}");
    // The base: the body after the header, then each spec's name and text, each part after its length.
    let text = std::fs::read_to_string(root.join(format!("{FRONT}.md"))).unwrap();
    let mut base = blake3::Hasher::new();
    for part in [&text[text.find("\n---\n").unwrap() + 5..], "keep", KEEP, "old", OLD] {
        base.update(&(part.len() as u64).to_le_bytes()).update(part.as_bytes());
    }
    assert_eq!(shown["base"], base.finalize().to_hex().to_string(), "the base covers the body and the specs");
    let diff = std::fs::read_to_string(root.join(&delta).join("keep.diff.html")).unwrap();
    assert!(diff.contains("<script>"), "the replaced view's diff page");
    assert!(!root.join(&delta).join("new.diff.html").exists(), "an added view has no diff page");
    let files = git(root, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files.lines().count(), 4, "the record, its two specs and a diff page, one commit: {files}");
    assert!(git(root, &["status", "--porcelain"]).is_empty());
    assert!(!root.join(FRONT).join("diagrams/new.yaml").exists(), "the design is untouched until the stamp");

    assert_eq!(bp(root, &["transition", ISSUE, "abandoned"]).0, Some(0));
    assert_eq!(bp(root, &["transition", M, "approved"]).0, Some(0));
    let (exit, v) = bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", M]);
    assert_eq!(exit, Some(0), "{v}");
    let at = |p: &str| std::fs::read_to_string(root.join(FRONT).join("diagrams").join(p)).ok();
    assert_eq!(at("new.yaml").as_deref(), Some(NEW));
    assert!(at("new.html").unwrap().contains("data-noun=\"Index\""), "rendered beside its spec");
    assert!(at("keep.html").unwrap().contains("data-id=\"v\""), "the replaced view rendered again");
    assert_eq!((at("old.yaml"), at("old.html")), (None, None), "the removed view goes with its page");
    let system = std::fs::read_to_string(root.join(SYSTEM).join("diagrams/containers.html")).unwrap();
    assert!(system.contains("\"children\":1"), "the system's page counts the new component view");
    let paths: Vec<&str> = v["diagrams"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    for p in ["new.yaml", "new.html", "keep.html", "old.yaml", "old.html"] {
        assert!(paths.contains(&format!("{FRONT}/diagrams/{p}").as_str()), "{p}: {paths:?}");
    }
    let manifest = &bp(root, &["show", "projects/bp/versions/1"]).1["manifest"];
    let spec = format!("{FRONT}/diagrams/new.yaml");
    assert_eq!(manifest[&spec], blake3::hash(NEW.as_bytes()).to_hex().to_string(), "the manifest pins it: {manifest}");
    assert!(manifest.get(format!("{FRONT}/diagrams/old.yaml")).is_none(), "{manifest}");
    assert_eq!(bp(root, &["show", FRONT]).1["body"], BODY.trim_end());
    assert!(git(root, &["status", "--porcelain"]).is_empty());
}

/// `pr` lists each diagram the delta changes, in its body and its result. A fake `gh` answers `api user`, an empty
/// `pr list` and `pr create`, keeping the body it was given.
#[test]
fn pr_lists_each_diagram_a_delta_changes() {
    let t = tempfile::tempdir().unwrap();
    let t = t.path();
    let origin = t.join("origin.git");
    git(t, &["init", "-q", "--bare", "-b", "master", "origin.git"]);
    git(t, &["init", "-q", "-b", "master", "code"]);
    let code = t.join("code");
    git(&code, &["config", "user.name", "t"]);
    git(&code, &["config", "user.email", "t@t"]);
    git(&code, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&code, &["remote", "add", "origin", &origin.display().to_string()]);
    git(&code, &["push", "-q", "-u", "origin", "master"]);
    git(&code, &["remote", "set-head", "origin", "master"]);
    std::fs::create_dir(t.join("bin")).unwrap();
    let fake = format!(
        "#!/bin/sh\ncase \"$1 $2\" in\n  \"api user\") echo '{{\"login\":\"agent\"}}' ;;\n  \"pr list\") echo '[]' ;;\n  \
         \"pr create\") shift 9; printf '%s' \"$1\" > '{t}/body'; echo 'https://forge.invalid/o/r/pull/1' ;;\n  *) exit 9 ;;\nesac\n",
        t = t.display()
    );
    std::fs::write(t.join("bin/gh"), fake).unwrap();
    std::fs::set_permissions(t.join("bin/gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let project = format!("---\nkind: project\ntitle: bp\nstatus: active\nrepository: {}\n---\n", origin.display());
    let v = vault(&[]);
    let root = v.path();
    std::fs::write(root.join("projects/bp/project.md"), project).unwrap();
    git(root, &["commit", "-qam", "repository"]);
    assert_eq!(bp(root, &["transition", M, "approved"]).0, Some(0));
    let tmp = tempfile::tempdir().unwrap();
    let [body, new, keep] = &files(tmp.path(), &[("body.md", BODY), ("new.yaml", NEW), ("keep.yaml", KEEP)])[..] else {
        unreachable!()
    };
    let flags = ["--diagram", &format!("new={new}"), "--diagram", &format!("keep={keep}"), "--remove-diagram", "old"];
    let (exit, d) = record(root, body, &flags);
    assert_eq!(exit, Some(0), "{d}");
    let delta = d["id"].as_str().unwrap();

    let path = format!("{}:{}", t.join("bin").display(), std::env::var("PATH").unwrap());
    let env = [("BP_VAULT", root.to_str().unwrap()), ("PATH", path.as_str())];
    let (exit, out) = bp_env(&code, &env, &["cut", ISSUE]);
    assert_eq!(exit, Some(0), "{out}");
    let (exit, out) = bp_env(&code, &env, &["pr", ISSUE]);
    assert_eq!(exit, Some(0), "{out}");
    let want = json!([
        { "delta": delta, "name": "keep", "change": "replaced", "diff": format!("{delta}/keep.diff.html") },
        { "delta": delta, "name": "new", "change": "added" },
        { "delta": delta, "name": "old", "change": "removed" },
    ]);
    assert_eq!(out["diagrams"], want, "{out}");
    let body = std::fs::read_to_string(t.join("body")).unwrap();
    let listed = format!(
        "- {delta}\n  - diagram `keep`: replaced; diff page `{delta}/keep.diff.html`\n  - diagram `new`: added\n  - \
         diagram `old`: removed\n"
    );
    assert!(body.contains(&listed), "{body}");
}

#[test]
fn a_term_the_milestone_adds_to_the_glossary_resolves() {
    let v = vault(&[]);
    let root = v.path();
    let tmp = tempfile::tempdir().unwrap();
    let spec = NEW.replace("noun: Index", "term: index");
    let glossary = "- `harness` — runs the agents.\n- `index` — the derived store.\n";
    let [body, spec, glossary] = &files(tmp.path(), &[("body.md", BODY), ("s.yaml", &spec), ("g.md", glossary)])[..]
    else {
        unreachable!()
    };
    let flag = format!("new={spec}");
    assert_eq!(record(root, body, &["--diagram", &flag]).1["code"], "diagram_invalid", "no such term yet");
    let ms = format!("milestone={M}");
    let g = ["record", "delta", "projects/bp/designs/glossary", "--field", &ms, "--body-file", glossary];
    assert_eq!(bp(root, &g).0, Some(0));
    let (exit, v) = record(root, body, &["--diagram", &flag]);
    assert_eq!(exit, Some(0), "the glossary's delta under the same milestone names it: {v}");
}

#[test]
fn a_dirty_spec_or_an_unknown_removal_refuses() {
    let v = vault(&[]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    let tmp = tempfile::tempdir().unwrap();
    let [body] = &files(tmp.path(), &[("body.md", BODY)])[..] else { unreachable!() };
    let (exit, v) = record(root, body, &["--remove-diagram", "nosuch"]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("unknown_value")), "{v}");
    // A spec read from the working tree would reach the base and the check without git holding it.
    let keep = root.join(FRONT).join("diagrams/keep.yaml");
    std::fs::write(&keep, KEEP.replace("Two boxes.", "Dirty.")).unwrap();
    assert_eq!(record(root, body, &[]).1["code"], "path_dirty");
    git(root, &["checkout", "--", "."]);
    assert_eq!(record(root, body, &[]).0, Some(0));
    assert_eq!(bp(root, &["transition", ISSUE, "abandoned"]).0, Some(0));
    assert_eq!(bp(root, &["transition", M, "approved"]).0, Some(0));
    // The stamp reads every spec of the project, so a dirty one anywhere refuses before the blocks run.
    std::fs::write(root.join(SYSTEM).join("diagrams/containers.yaml"), SYS.replace("One box.", "Dirty box.")).unwrap();
    let (exit, v) = bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", M]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("path_dirty")), "{v}");
    assert!(!root.join(format!("{M}/runs")).exists(), "no block ran");
}

#[test]
fn a_glossary_delta_keeps_every_term_a_spec_uses() {
    const DRAFT: &str = "projects/bp/designs/containers/draft";
    let draft = "---\nkind: design\ntitle: Draft\nstatus: draft\nunder: projects/bp/designs/system\n---\n\nDraft.\n";
    let v = vault(&[(&format!("{DRAFT}.md"), draft)]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    let both = "- `harness` — runs the agents.\n- `spare` — unused.\n";
    std::fs::write(root.join("projects/bp/designs/glossary.md"), design("G", SYSTEM, both)).unwrap();
    git(root, &["commit", "-qam", "spare"]);
    let tmp = tempfile::tempdir().unwrap();
    let spare = OLD.replace("term: harness", "term: spare");
    let [dropped, kept, spec] = &files(
        tmp.path(),
        &[("dropped.md", "- `spare` — unused.\n"), ("kept.md", "- `harness` — runs the agents.\n"), ("s.yaml", &spare)],
    )[..] else {
        unreachable!()
    };
    let glossary = |body: &str| {
        let ms = format!("milestone={M}");
        bp(root, &["record", "delta", "projects/bp/designs/glossary", "--field", &ms, "--body-file", body])
    };
    // Dropping `harness` orphans both views of the front: the refusal names each.
    let (exit, v) = glossary(dropped);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("diagram_invalid")), "{v}");
    assert_eq!(v["subject"], format!("{FRONT}/diagrams/keep.yaml#h"), "{v}");
    for spec in ["keep", "old"] {
        assert!(v["fix"].as_str().unwrap().contains(&format!("{FRONT}/diagrams/{spec}.yaml")), "{v}");
    }
    assert!(!v["fix"].as_str().unwrap().contains("edit "), "only bp writes a vault file: {v}");
    // `spare` is in no spec when the delta drops it; a draft's view takes it up before the stamp.
    assert_eq!(glossary(kept).0, Some(0));
    assert_eq!(bp(root, &["diagram", DRAFT, "spare", "--spec-file", spec]).0, Some(0));
    assert_eq!(bp(root, &["transition", ISSUE, "abandoned"]).0, Some(0));
    assert_eq!(bp(root, &["transition", M, "approved"]).0, Some(0));
    let (exit, v) = bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", M]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("diagram_invalid")), "{v}");
    assert_eq!(v["subject"], format!("{DRAFT}/diagrams/spare.yaml#h"), "{v}");
}

//! The editorial delta (28, design change 9) through the binary: `record delta --editorial` needs no milestone and
//! refuses `not_editorial` on a change to a check block, a heading, a link, a glossary term, a noun, or any part of a
//! spec but its text and layout; `bp stamp <delta>` is the human's, writes the delta alone and stamps no version.

mod common;

use std::path::Path;

use common::{bp, bp_env, commits, git};
use serde_json::Value;

const S: &str = "projects/bp/designs/system";
const A: &str = "projects/bp/milestones/a";

const BODY: &str = "Lead words. See [[shared/threads/t]].\n\n## Store\n\nThe store keeps files.\n\n### Detail\n\n\
                    | Noun | Means |\n|---|---|\n| Index | the rows |\n\n## Checks\n\n\
                    ~~~check shell {name=p expect=0}\ntrue\n~~~\n";
const SPEC: &str = "type: architecture\nlevel: container\ntitle: One\nlede: Two boxes.\nnodes:\n  - {id: s, noun: Store, \
                    at: [0, 0]}\n  - {id: i, noun: Index, at: [1, 0]}\nedges:\n  - {from: s, to: i, label: reads}\n";

const ER: &str = "type: er\nlevel: conceptual\ntitle: One\nlede: Two entities.\nnodes:\n  - {id: s, noun: Store, at: [0, 0]}\n  \
                  - {id: i, noun: Index, at: [1, 0]}\nedges:\n  - {from: s, to: i, label: holds, card: [\"1\", \"*\"]}\n";
const SEQ: &str = "type: sequence\ntitle: One\nlede: A call.\nparties:\n  - {id: s, noun: Store}\n  - {id: i, noun: Index}\n\
                   steps:\n  - {id: read, from: s, to: i, say: read}\n  - {id: ack, from: i, to: s, say: ack}\n";
const GLOSSARY: &str = "projects/bp/designs/glossary";

fn vault() -> tempfile::TempDir {
    vault_with(SPEC)
}

/// A project, the active design [`S`] with [`BODY`] and the view `one` as `spec`, the glossary, two threads and the
/// planned milestone [`A`].
fn vault_with(spec: &str) -> tempfile::TempDir {
    let (s, one, page, a) =
        (format!("{S}.md"), format!("{S}/diagrams/one.yaml"), format!("{S}/diagrams/one.html"), format!("{A}.md"));
    let design = format!("---\nkind: design\ntitle: S\nstatus: active\n---\n\n{BODY}");
    let glossary = "---\nkind: design\ntitle: G\nstatus: active\n---\n\nTerms.\n\n- `index` — the rows\n";
    let milestone = "---\nkind: milestone\ntitle: A\nstatus: planned\n---\n\n## Finish line\n\n\
                     ~~~check shell {name=p expect=0}\ntrue\n~~~\n";
    common::vault(&[
        ("projects/bp/project.md", "---\nkind: project\ntitle: bp\nstatus: active\n---\n"),
        (&s, &design),
        (&one, spec),
        (&page, "<p>one</p>\n"),
        (&format!("{GLOSSARY}.md"), glossary),
        (&a, milestone),
        ("shared/threads/t.md", "---\nkind: thread\ntitle: T\nstatus: open\n---\n"),
        ("shared/threads/u.md", "---\nkind: thread\ntitle: U\nstatus: open\n---\n"),
    ])
}

/// `bp record delta <S> --editorial` with `body` and, when given, the spec `one`.
fn record(root: &Path, body: &str, spec: Option<&str>, extra: &[&str]) -> (Option<i32>, Value) {
    record_on(root, S, body, spec, extra)
}

fn record_on(root: &Path, design: &str, body: &str, spec: Option<&str>, extra: &[&str]) -> (Option<i32>, Value) {
    let file = root.join(".bp/body.md");
    std::fs::write(&file, body).unwrap();
    let mut args = vec!["record", "delta", design, "--editorial", "--body-file", file.to_str().unwrap()];
    let flag;
    if let Some(spec) = spec {
        std::fs::write(root.join(".bp/one.yaml"), spec).unwrap();
        flag = format!("one={}", root.join(".bp/one.yaml").display());
        args.extend(["--diagram", &flag]);
    }
    args.extend(extra);
    bp(root, &args)
}

/// Asserts the record refuses `not_editorial` with `subject` and commits nothing.
fn refuses(body: &str, spec: Option<&str>, subject: &str, names: &str) {
    refuses_on(&vault(), S, body, spec, subject, names);
}

/// [`refuses`] on `design` of the vault `v`.
fn refuses_on(v: &tempfile::TempDir, design: &str, body: &str, spec: Option<&str>, subject: &str, names: &str) {
    let root = v.path();
    let before = commits(root);
    let (exit, r) = record_on(root, design, body, spec, &[]);
    assert_eq!(
        (exit, r["code"].as_str(), r["subject"].as_str()),
        (Some(2), Some("not_editorial"), Some(subject)),
        "{r}"
    );
    assert!(r["fix"].as_str().unwrap().contains(names), "{r}");
    assert!(r["fix"].as_str().unwrap().contains(&format!("bp record delta {design} --field milestone=")), "{r}");
    assert_eq!(commits(root), before, "nothing is committed");
}

#[test]
fn refuses_not_editorial_check_block() {
    refuses(&BODY.replace("true\n~~~", "false\n~~~"), None, S, "changes check block `p`");
}

#[test]
fn refuses_not_editorial_heading() {
    refuses(&BODY.replace("### Detail", "### Details"), None, S, "drops heading `### Detail`");
    refuses(&BODY.replace("## Checks", "## Notes\n\nMore.\n\n## Checks"), None, S, "adds heading `## Notes`");
}

#[test]
fn refuses_not_editorial_link() {
    refuses(&BODY.replace("threads/t]]", "threads/u]]"), None, S, "drops link `shared/threads/t`");
    refuses(&BODY.replace("Lead words.", "Lead words, [[shared/threads/u]]."), None, S, "adds link `shared/threads/u`");
}

#[test]
fn refuses_not_editorial_diagram_ids() {
    let one = format!("{S}/diagrams/one.yaml");
    refuses(
        BODY,
        Some(&SPEC.replace("id: i,", "id: ix,").replace("to: i,", "to: ix,")),
        &one,
        "changes the id of node `i`",
    );
}

#[test]
fn refuses_not_editorial_diagram_edges() {
    let one = format!("{S}/diagrams/one.yaml");
    refuses(BODY, Some(&SPEC.replace("from: s, to: i", "from: i, to: s")), &one, "changes the id of edge `s--i`");
}

#[test]
fn refuses_not_editorial_diagram_terms() {
    let one = format!("{S}/diagrams/one.yaml");
    refuses(BODY, Some(&SPEC.replace("noun: Index", "noun: Detail")), &one, "changes the noun of node `i`");
}

#[test]
fn refuses_not_editorial_a_new_diagram() {
    let v = vault();
    let root = v.path();
    std::fs::write(root.join(".bp/two.yaml"), SPEC).unwrap();
    let flag = format!("two={}", root.join(".bp/two.yaml").display());
    let (exit, r) = record(root, BODY, None, &["--diagram", &flag]);
    let two = format!("{S}/diagrams/two.yaml");
    assert_eq!(
        (exit, r["code"].as_str(), r["subject"].as_str()),
        (Some(2), Some("not_editorial"), Some(two.as_str())),
        "{r}"
    );
}

/// [`refuses`] on the spec `one`, whose base is `base`.
fn refuses_spec(base: &str, spec: &str, names: &str) {
    refuses_on(&vault_with(base), S, BODY, Some(spec), &format!("{S}/diagrams/one.yaml"), names);
}

#[test]
fn refuses_not_editorial_diagram_role() {
    refuses_spec(SPEC, &SPEC.replace("noun: Store,", "noun: Store, role: external,"), "changes the role of node `s`");
}

#[test]
fn refuses_not_editorial_diagram_group() {
    let group = "groups:\n  - {id: g, label: Trust boundary, members: [s, i]}\nedges:";
    refuses_spec(SPEC, &SPEC.replace("edges:", group), "adds group `g`");
}

#[test]
fn refuses_not_editorial_diagram_step_order() {
    let swapped = SEQ
        .replace("{id: read, from: s, to: i, say: read}", "TMP")
        .replace("{id: ack, from: i, to: s, say: ack}", "{id: read, from: s, to: i, say: read}");
    let swapped = swapped.replace("TMP", "{id: ack, from: i, to: s, say: ack}");
    refuses_spec(SEQ, &swapped, "changes the id of step `read`");
}

#[test]
fn refuses_not_editorial_diagram_dashed() {
    refuses_spec(
        SPEC,
        &SPEC.replace("label: reads}", "label: reads, style: dashed}"),
        "changes the dashed of edge `s--i`",
    );
}

#[test]
fn refuses_not_editorial_diagram_card() {
    refuses_spec(ER, &ER.replace("[\"1\", \"*\"]", "[\"*\", \"1\"]"), "changes the card of edge `s--i`");
}

#[test]
fn refuses_not_editorial_diagram_focus() {
    refuses_spec(
        SPEC,
        &SPEC.replace("lede: Two boxes.\n", "lede: Two boxes.\nfocus: [i]\n"),
        "changes the spec's focus",
    );
}

#[test]
fn refuses_not_editorial_diagram_level() {
    refuses_spec(SPEC, &SPEC.replace("level: container", "level: component"), "changes the spec's level");
}

#[test]
fn refuses_not_editorial_diagram_type() {
    refuses_spec(
        SPEC,
        &SPEC.replace("type: architecture\nlevel: container\n", "type: state\n"),
        "changes the spec's type",
    );
}

#[test]
fn refuses_not_editorial_a_glossary_term() {
    let v = vault();
    let body = "Terms.\n\n- `index` — the rows\n";
    refuses_on(&v, GLOSSARY, &body.replace("`index`", "`catalogue`"), None, GLOSSARY, "drops glossary term `index`");
    refuses_on(&v, GLOSSARY, &format!("{body}- `widget` — a part\n"), None, GLOSSARY, "adds glossary term `widget`");
}

#[test]
fn refuses_not_editorial_a_noun() {
    let body = BODY.replace("| Index | the rows |", "| Index | the rows |\n| Extra | more |");
    refuses(&body, None, S, "adds noun `extra`");
}

/// Wording, a table cell, a node label and a node's cell change: the delta records with no milestone, and the
/// human's stamp writes it alone, with no finish line, version or tag.
#[test]
fn wording_labels_and_cells_record_and_stamp() {
    let v = vault();
    let root = v.path();
    let body = BODY.replace("keeps files", "keeps the files").replace("the rows", "the index rows");
    let spec = SPEC.replace("noun: Store,", "noun: Store, label: File store,").replace("at: [1, 0]", "at: [2, 0]");
    let (exit, r) = record(root, &body, Some(&spec), &["--field", "change=wording"]);
    assert_eq!((exit, &r["code"]), (Some(0), &Value::Null), "{r}");
    let delta = r["id"].as_str().unwrap().to_string();
    let header = bp(root, &["show", &delta]).1;
    assert_eq!(
        (&header["editorial"], &header["milestone"], &header["state"]),
        (&Value::Bool(true), &Value::Null, &"proposed".into())
    );

    // A second editorial delta on the design waits for the human to accept the first.
    let (exit, r) = record(root, &body.replace("Lead words", "Lead text"), None, &[]);
    assert_eq!((exit, r["code"].as_str()), (Some(2), Some("delta_pending")), "{r}");
    assert!(r["fix"].as_str().unwrap().contains(&format!("bp stamp {delta}")), "{r}");

    let (exit, r) = bp_env(root, &[("BP_ROLE", "session")], &["stamp", &delta]);
    assert_eq!((exit, r["code"].as_str()), (Some(2), Some("role_forbidden")), "accepting is the human's act: {r}");
    let (exit, r) = bp(root, &["stamp", &delta]);
    assert_eq!((exit, r["state"].as_str()), (Some(0), Some("accepted")), "{r}");
    assert_eq!(bp(root, &["show", S]).1["body"], body.trim());
    assert_eq!(std::fs::read_to_string(root.join(format!("{S}/diagrams/one.yaml"))).unwrap(), spec);
    assert!(std::fs::read_to_string(root.join(format!("{S}/diagrams/one.html"))).unwrap().contains("File store"));
    assert_eq!(bp(root, &["show", &delta]).1["state"], "accepted");
    let log = git(root, &["log", "-1", "--format=%B"]);
    assert!(log.contains("Bp-Verb: stamp") && log.contains(&format!("Bp-Delta: {delta}")), "{log}");
    assert!(git(root, &["tag"]).trim().is_empty(), "no version tag");
    assert!(!root.join("projects/bp/versions").exists(), "no version record");
    assert_eq!(bp(root, &["show", A]).1["status"], "planned", "no milestone moves");
    assert_eq!(bp(root, &["stamp", &delta]).0, Some(3), "a repeat is a no-op");
}

#[test]
fn an_editorial_stamp_refuses_base_moved_and_path_dirty() {
    let v = vault();
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    let (exit, r) = record(root, &BODY.replace("keeps files", "keeps the files"), None, &[]);
    assert_eq!(exit, Some(0), "{r}");
    let delta = r["id"].as_str().unwrap().to_string();

    std::fs::write(root.join(format!("{S}.md")), "dirty\n").unwrap();
    let (exit, r) = bp(root, &["stamp", &delta]);
    assert_eq!((exit, r["code"].as_str()), (Some(2), Some("path_dirty")), "{r}");
    git(root, &["checkout", "--", "."]);
    // A dirty spec is `path_dirty`, not a moved base: the check runs before the base is hashed.
    std::fs::write(root.join(format!("{S}/diagrams/one.yaml")), SPEC.replace("reads", "reads dirty")).unwrap();
    let (exit, r) = bp(root, &["stamp", &delta]);
    assert_eq!((exit, r["code"].as_str()), (Some(2), Some("path_dirty")), "{r}");
    git(root, &["checkout", "--", "."]);

    // A milestone's delta lands first; the editorial one was proposed on the design before it.
    assert_eq!(bp(root, &["transition", A, "approved"]).0, Some(0));
    let file = root.join(".bp/a.md");
    std::fs::write(&file, BODY.replace("Lead words.", "Lead words, now more.")).unwrap();
    let ms = format!("milestone={A}");
    assert_eq!(bp(root, &["record", "delta", S, "--field", &ms, "--body-file", file.to_str().unwrap()]).0, Some(0));
    assert_eq!(bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", A]).0, Some(0));
    let before = commits(root);
    let (exit, r) = bp(root, &["stamp", &delta]);
    assert_eq!(
        (exit, r["code"].as_str(), r["subject"].as_str()),
        (Some(2), Some("base_moved"), Some(delta.as_str())),
        "{r}"
    );
    assert!(r["fix"].as_str().unwrap().contains(&format!("bp record delta {S} --editorial --body-file <f>")), "{r}");
    assert_eq!(commits(root), before);
}

#[test]
fn the_flag_and_the_milestone_exclude_each_other() {
    let v = vault();
    let root = v.path();
    let ms = format!("milestone={A}");
    let (exit, r) = record(root, BODY, None, &["--field", &ms]);
    assert_eq!((exit, r["code"].as_str()), (Some(4), Some("usage")), "{r}");
    let file = root.join(".bp/body.md");
    let (exit, r) = bp(root, &["record", "delta", S, "--body-file", file.to_str().unwrap()]);
    assert_eq!((exit, r["code"].as_str()), (Some(2), Some("schema_invalid")), "{r}");
    assert!(r["fix"].as_str().unwrap().contains("--editorial"), "{r}");

    // A milestone's delta is accepted by its milestone's stamp, never alone.
    let (_, r) = bp(root, &["record", "delta", S, "--field", &ms, "--body-file", file.to_str().unwrap()]);
    let (exit, r) = bp(root, &["stamp", r["id"].as_str().unwrap()]);
    assert_eq!(
        (exit, r["code"].as_str(), r["fix"].as_str()),
        (Some(2), Some("illegal_transition"), Some(format!("bp stamp {A}").as_str())),
        "{r}"
    );
}

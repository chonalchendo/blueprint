//! The write path (12): one writer at a time, a dirty target refused, a failed or killed writer's files back to
//! their pre-images, one commit with its trailers, the finding rule of 25 (design change 1), and a header writer
//! whose output parses back to what it read.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use bp_vault::{Code, File, Id, Index, Vault};
use serde_json::Value;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

fn issue(severity: &str, under: &str) -> String {
    format!("---\nkind: issue\ntitle: T\nstatus: open\nunder: {under}\nseverity: {severity}\n---\n\nBody.\n")
}

/// A vault under git with one milestone and two issues, one commit, its own identity.
fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (id, text) in [
        ("projects/bp/milestones/m", "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n".to_string()),
        ("projects/bp/issues/a", issue("low", "projects/bp/milestones/m")),
        ("projects/bp/issues/b", issue("low", "'[[milestone.gone]]'")),
    ] {
        let path = Id::parse(id).unwrap().path(root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::create_dir_all(root.join("shared")).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@t"]);
    Index::open(&Vault { root: root.to_path_buf() }).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "one"]);
    let v = Vault { root: root.to_path_buf() };
    (dir, v)
}

fn change(id: &str, text: String) -> BTreeMap<Id, String> {
    BTreeMap::from([(Id::parse(id).unwrap(), text)])
}

#[test]
fn a_second_writer_waits_then_refuses_vault_locked() {
    let (_d, v) = vault();
    let (held, release) = (std::sync::Barrier::new(2), std::sync::Barrier::new(2));
    std::thread::scope(|s| {
        s.spawn(|| {
            let _w = v.writer().unwrap();
            held.wait();
            release.wait();
        });
        held.wait();
        let start = std::time::Instant::now();
        let e = v.writer().err().unwrap();
        assert_eq!((e.code, e.exit), (Code::VaultLocked, 2));
        assert!(start.elapsed() >= std::time::Duration::from_secs(5), "it waits 5 s first");
        release.wait();
    });
    assert!(v.writer().is_ok(), "the lock goes with its holder");
}

#[test]
fn one_commit_with_trailers_and_a_fresh_index() {
    let (_d, v) = vault();
    let id = "projects/bp/issues/a";
    let text = bp_vault::set_field(&v.text(&Id::parse(id).unwrap()).unwrap(), "severity", &"high".into());
    let w = v.writer().unwrap().commit("set", "worker", "set a severity", &change(id, text)).unwrap();
    assert_eq!(w.commit, git(&v.root, &["rev-parse", "HEAD"]).trim());
    let message = git(&v.root, &["log", "-1", "--format=%B"]);
    assert!(message.lines().any(|l| l == "Bp-Verb: set") && message.lines().any(|l| l == "Bp-Role: worker"));
    assert_eq!(git(&v.root, &["diff", "--numstat", "HEAD~1", "HEAD"]), "1\t1\tprojects/bp/issues/a.md\n");
    assert_eq!(git(&v.root, &["status", "--porcelain"]), "");
    let warm = Index::open(&v).unwrap().header(id).unwrap().unwrap();
    assert_eq!(warm["severity"], "high");
    std::fs::remove_file(v.root.join(".bp/index.db")).unwrap();
    assert_eq!(Index::open(&v).unwrap().header(id).unwrap().unwrap(), warm, "the index equals a rebuild");
}

#[test]
fn a_write_refuses_only_the_findings_it_adds() {
    let (_d, v) = vault();
    // b's `under` was broken before this write, so its label change commits and lists the finding.
    let b = "projects/bp/issues/b";
    let text = bp_vault::set_field(&v.text(&Id::parse(b).unwrap()).unwrap(), "severity", &"high".into());
    let w = v.writer().unwrap().commit("set", "human", "set b", &change(b, text)).unwrap();
    assert_eq!(w.findings.len(), 1);
    assert_eq!((w.findings[0].code, w.findings[0].field.as_str()), ("unresolved_link", "under"));
    // A new broken link refuses, and leaves the head and the working tree as they were.
    let head = git(&v.root, &["rev-parse", "HEAD"]);
    let e = v
        .writer()
        .unwrap()
        .commit("set", "human", "set a", &change("projects/bp/issues/a", issue("low", "projects/bp/milestones/nope")))
        .err()
        .unwrap();
    assert_eq!(e.code, Code::UnresolvedLink);
    assert_eq!(git(&v.root, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&v.root, &["status", "--porcelain"]), "");
    // A link to a file the same write adds resolves against the proposed tree.
    let mut both = change("projects/bp/issues/a", issue("low", "projects/bp/milestones/n"));
    both.insert(
        Id::parse("projects/bp/milestones/n").unwrap(),
        "---\nkind: milestone\ntitle: N\nstatus: planned\n---\n".into(),
    );
    assert!(v.writer().unwrap().commit("create", "human", "a under n", &both).unwrap().findings.is_empty());
}

/// A pre-commit hook for the vault's repository.
fn hook(root: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(".git/hooks/pre-commit");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn severity(v: &Vault, id: &str, value: &str) -> BTreeMap<Id, String> {
    change(id, bp_vault::set_field(&v.text(&Id::parse(id).unwrap()).unwrap(), "severity", &value.into()))
}

#[test]
fn a_failed_commit_restores_the_pre_images_and_keeps_every_other_edit() {
    let (_d, v) = vault();
    hook(&v.root, "exit 1");
    let a = Id::parse("projects/bp/issues/a").unwrap();
    let before = v.text(&a).unwrap();
    // A human's edit in a file the write does not name.
    let b = Id::parse("projects/bp/issues/b").unwrap().path(&v.root);
    std::fs::write(&b, std::fs::read_to_string(&b).unwrap() + "HUMAN WORK IN PROGRESS\n").unwrap();
    let head = git(&v.root, &["rev-parse", "HEAD"]);
    let mut changes = severity(&v, a.as_str(), "critical");
    changes.insert(
        Id::parse("projects/bp/milestones/n").unwrap(),
        "---\nkind: milestone\ntitle: N\nstatus: planned\n---\n".into(),
    );
    let e = v.writer().unwrap().commit("set", "human", "set a", &changes).err().unwrap();
    assert_eq!((e.code, e.subject.as_str()), (Code::CouldNotRun, "git commit"));
    assert_eq!(v.text(&a).unwrap(), before, "the target is its pre-image");
    assert!(!v.root.join("projects/bp/milestones/n.md").exists(), "a file the write made is gone");
    assert!(std::fs::read_to_string(&b).unwrap().ends_with("HUMAN WORK IN PROGRESS\n"));
    assert_eq!(git(&v.root, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&v.root, &["status", "--porcelain"]), " M projects/bp/issues/b.md\n", "nothing staged");
    assert!(!v.root.join(".bp/pending").exists());
}

#[test]
fn a_target_with_uncommitted_edits_refuses_path_dirty() {
    let (_d, v) = vault();
    let a = "projects/bp/issues/a";
    let path = Id::parse(a).unwrap().path(&v.root);
    let edited = v.text(&Id::parse(a).unwrap()).unwrap() + "note\n";
    std::fs::write(&path, &edited).unwrap();
    let new = Id::parse("projects/bp/milestones/n").unwrap().path(&v.root);
    let head = git(&v.root, &["rev-parse", "HEAD"]);
    for staged in [false, true] {
        if staged {
            git(&v.root, &["add", "--", "projects/bp/issues/a.md"]);
        }
        let e = v.writer().unwrap().commit("set", "human", "set a", &severity(&v, a, "high")).err().unwrap();
        assert_eq!((e.code, e.exit, e.subject.as_str()), (Code::PathDirty, 2, "projects/bp/issues/a.md"));
        assert_eq!(e.fix, "commit or stash projects/bp/issues/a.md, then rerun");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited, "the human's edit stays");
    }
    // A file the write would make already sits on disk, untracked.
    let mine = "---\nkind: milestone\ntitle: Mine\nstatus: planned\n---\n";
    std::fs::write(&new, mine).unwrap();
    let e = v
        .writer()
        .unwrap()
        .commit("create", "human", "n", &change("projects/bp/milestones/n", "x".into()))
        .err()
        .unwrap();
    assert_eq!((e.code, e.subject.as_str()), (Code::PathDirty, "projects/bp/milestones/n.md"));
    assert_eq!(std::fs::read_to_string(&new).unwrap(), mine);
    assert_eq!(git(&v.root, &["rev-parse", "HEAD"]), head);
}

#[test]
fn a_path_is_literal_not_a_glob() {
    let (_d, v) = vault();
    for name in ["a[1]", "a1"] {
        std::fs::write(v.root.join(format!("projects/bp/issues/{name}.md")), issue("low", "projects/bp/milestones/m"))
            .unwrap();
    }
    git(&v.root, &["add", "-A"]);
    git(&v.root, &["commit", "-qm", "two"]);
    let a1 = v.root.join("projects/bp/issues/a1.md");
    std::fs::write(&a1, std::fs::read_to_string(&a1).unwrap() + "note\n").unwrap();
    v.writer().unwrap().commit("set", "human", "set a[1]", &severity(&v, "projects/bp/issues/a[1]", "high")).unwrap();
    assert_eq!(git(&v.root, &["show", "--name-only", "--format=", "HEAD"]), "projects/bp/issues/a[1].md\n");
    assert_eq!(git(&v.root, &["status", "--porcelain"]), " M projects/bp/issues/a1.md\n");
}

#[test]
fn a_vault_without_a_commit_refuses_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = Id::parse("projects/bp/issues/a").unwrap().path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, issue("low", "projects/bp/milestones/m")).unwrap();
    let v = Vault { root: dir.path().to_path_buf() };
    let e = v.writer().err().unwrap();
    assert_eq!((e.code, e.subject), (Code::CouldNotRun, dir.path().display().to_string()));
    assert!(!dir.path().join(".bp").exists(), "nothing touched the disk");
    git(dir.path(), &["init", "-q"]);
    assert_eq!(v.writer().err().unwrap().code, Code::CouldNotRun, "a repository with no commit has no HEAD");
}

/// Not a test on its own: the writer `a_killed_writer_...` runs as a child process and kills inside its commit.
#[test]
fn child_writer() {
    let Some(root) = std::env::var_os("BP_TEST_CHILD_WRITER") else { return };
    let v = Vault { root: root.into() };
    let _ = v.writer().unwrap().commit("set", "human", "set a", &severity(&v, "projects/bp/issues/a", "critical"));
}

#[test]
fn a_killed_writer_is_undone_and_the_next_write_commits_only_its_own_file() {
    use std::os::unix::process::CommandExt;
    let (_d, v) = vault();
    let a = Id::parse("projects/bp/issues/a").unwrap();
    let before = v.text(&a).unwrap();
    // The commit is the widest window: git holds its own `index.lock` while the hook runs.
    hook(&v.root, "touch .bp/hooked; sleep 30");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_writer", "--nocapture"])
        .env("BP_TEST_CHILD_WRITER", &v.root)
        .process_group(0)
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while !v.root.join(".bp/hooked").exists() {
        assert!(start.elapsed() < std::time::Duration::from_secs(30), "the hook never ran");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let group = format!("-{}", child.id());
    assert!(Command::new("kill").args(["-KILL", "--", &group]).status().unwrap().success());
    child.wait().unwrap();
    std::fs::remove_file(v.root.join(".git/hooks/pre-commit")).unwrap();
    assert!(v.root.join(".bp/pending").exists(), "the dead writer left its journal");
    let lock = v.root.join(".git/index.lock");
    if lock.exists() {
        // Only a human can tell a dead git from a live one, so the tool refuses and keeps the journal.
        let e = v.writer().err().unwrap();
        assert_eq!((e.code, e.subject.as_str()), (Code::CouldNotRun, lock.to_str().unwrap()));
        assert!(e.fix.contains(&format!("remove {}", lock.display())), "{}", e.fix);
        assert!(v.root.join(".bp/pending").exists());
        std::fs::remove_file(&lock).unwrap();
    }
    v.writer().unwrap().commit("set", "human", "set b", &severity(&v, "projects/bp/issues/b", "medium")).unwrap();
    assert_eq!(git(&v.root, &["show", "--name-only", "--format=", "HEAD"]), "projects/bp/issues/b.md\n");
    assert_eq!(git(&v.root, &["status", "--porcelain"]), "");
    assert_eq!(v.text(&a).unwrap(), before);
    assert!(!v.root.join(".bp/pending").exists());
}

/// The labels `set` writes (13): the line writer's whole domain.
const LABELS: [&str; 9] =
    ["title", "tags", "applies", "class", "repository", "forge", "cue", "severity", "walkthrough"];

/// The header text and the body of a file as `File` reads them: the body starts after the first line that is
/// exactly `---` after the opening one.
fn split(text: &str) -> (&str, &str) {
    let rest = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = rest.strip_prefix("---\r\n").or_else(|| rest.strip_prefix("---\n")).unwrap();
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']) == "---" {
            return (&rest[..at], &rest[at + line.len()..]);
        }
        at += line.len();
    }
    panic!("a parsed header closes");
}

/// Every node of `$BP_VAULT`, when it is set: each label it holds, set through the line writer to its own value
/// and to a new one, changes that field alone, in one line. Skipped otherwise.
#[test]
fn every_label_of_the_vault_takes_the_line_writer() {
    let Some(root) = std::env::var_os("BP_VAULT") else { return };
    let v = Vault { root: root.into() };
    let mut writes = 0;
    for id in v.ids().unwrap() {
        let text = v.text(&id).unwrap();
        let Some(header) = File::from_bytes(id.clone(), text.as_bytes()).unwrap().header else { continue };
        if header.kind.is_record() {
            continue;
        }
        let (_, body) = split(&text);
        for key in LABELS {
            let own =
                if key == "title" { header.title.clone().map(Value::from) } else { header.fields.get(key).cloned() };
            let Some(own) = own else { continue };
            let new = match &own {
                Value::String(s) => Value::from(format!("{s} x")),
                Value::Bool(b) => Value::from(!b),
                _ => serde_json::json!(["x/new-tag"]),
            };
            for value in [own, new] {
                let out = bp_vault::set_field(&text, key, &value);
                let (yaml, after) = split(&out);
                let got = bp_vault::Header::parse(yaml, id.as_str()).unwrap();
                let mut want = header.clone();
                if key == "title" {
                    want.title = value.as_str().map(String::from);
                } else {
                    want.fields.insert(key.into(), value.clone());
                }
                assert_eq!(got, want, "{id} {key}");
                assert_eq!(after, body, "{id} {key}: the body is untouched");
                let (old, new): (Vec<&str>, Vec<&str>) = (text.lines().collect(), out.lines().collect());
                assert_eq!(old.len(), new.len(), "{id} {key}");
                let changed = old.iter().zip(&new).filter(|(a, b)| a != b).count();
                assert!(changed == 1 || (changed == 0 && want == header), "{id} {key}: {changed} lines");
                writes += 1;
            }
        }
    }
    assert!(writes > 0);
}

/// Every file of `$BP_VAULT`, when it is set: its header parses, emits and parses again to the same header, and the
/// body is untouched. Skipped otherwise.
#[test]
fn every_header_of_the_vault_round_trips() {
    let Some(root) = std::env::var_os("BP_VAULT") else { return };
    let v = Vault { root: root.into() };
    let mut headers = 0;
    for id in v.ids().unwrap() {
        let text = v.text(&id).unwrap();
        let first = File::from_bytes(id.clone(), text.as_bytes()).unwrap();
        let Some(header) = &first.header else { continue };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text).replace("\r\n", "\n");
        // The body starts after the first line that is exactly `---`, as `File` reads it.
        let rest = text.strip_prefix("---\n").unwrap();
        let mut at = 0;
        let mut body = "";
        for line in rest.split_inclusive('\n') {
            at += line.len();
            if line.trim_end_matches('\n') == "---" {
                body = &rest[at..];
                break;
            }
        }
        let again = File::from_bytes(id.clone(), format!("---\n{}---\n{body}", header.emit()).as_bytes()).unwrap();
        assert_eq!(again.header.as_ref(), Some(header), "{id}:\n{}", header.emit());
        assert_eq!(again.body, first.body, "{id}");
        headers += 1;
    }
    assert!(headers > 0);
}

//! The write path (12): one writer at a time, a dead writer's leftovers discarded, one commit with its trailers,
//! the finding rule of 25 (design change 1), and a header writer whose output parses back to what it read.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use bp_vault::{Code, File, Id, Index, Vault};

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

#[test]
fn a_dead_writer_is_undone_and_the_next_write_commits_only_its_own_file() {
    let (_d, v) = vault();
    let a = Id::parse("projects/bp/issues/a").unwrap();
    let original = v.text(&a).unwrap();
    // What a writer killed between its rename and its commit leaves: the journal of `Writer::commit`, a temporary
    // file, a renamed file that is staged, and a new file.
    std::fs::write(v.root.join(".bp/pending"), "projects/bp/issues/a.md\nprojects/bp/issues/new.md\n").unwrap();
    std::fs::write(v.root.join("projects/bp/issues/.new.md.bp-tmp"), "half").unwrap();
    std::fs::write(a.path(&v.root), issue("critical", "projects/bp/milestones/m")).unwrap();
    std::fs::write(v.root.join("projects/bp/issues/new.md"), issue("low", "projects/bp/milestones/m")).unwrap();
    git(&v.root, &["add", "projects/bp/issues/a.md"]);
    let b = "projects/bp/issues/b";
    let text = bp_vault::set_field(&v.text(&Id::parse(b).unwrap()).unwrap(), "severity", &"medium".into());
    v.writer().unwrap().commit("set", "human", "set b", &change(b, text)).unwrap();
    assert_eq!(git(&v.root, &["show", "--name-only", "--format=", "HEAD"]), "projects/bp/issues/b.md\n");
    assert_eq!(git(&v.root, &["status", "--porcelain"]), "");
    assert_eq!(v.text(&a).unwrap(), original);
    assert!(!v.root.join(".bp/pending").exists());
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

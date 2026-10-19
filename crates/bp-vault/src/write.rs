//! The write path (12, Write path). A writer holds `.bp/lock`, an OS file lock that dies with its process. It builds
//! the proposed tree, the changed files in memory, and refuses only a finding the proposed tree holds that the current
//! tree does not (25, design change 1): the converted vault holds hundreds of broken links no write made, and refusing
//! on them would refuse every write. Each file goes through a temporary file and a rename, then one commit of the
//! changed paths with the trailers, then the index takes the changed files and the new fingerprint, so the next read
//! rehashes nothing.

use std::collections::BTreeMap;
use std::fs::TryLockError;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::fingerprint::fingerprint;
use crate::index::{bp_dir, failed, insert, remove};
use crate::{Code, File, Id, Index, Refusal, Vault};

/// How long a second writer waits for the lock (12, An error case).
const WAIT: Duration = Duration::from_secs(5);

/// The journal under `.bp/`: the paths a writer is about to put on disk, one a line, written before the first and
/// removed after the commit. A writer that dies between the two leaves it for the next writer to undo.
const PENDING: &str = "pending";

/// The one writer of a vault, holding `.bp/lock` until dropped.
pub struct Writer<'v> {
    vault: &'v Vault,
    index: Index,
    _lock: std::fs::File,
}

/// A finding a written file holds: a link that resolves to nothing (`unresolved_link`), or a body finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Held {
    /// `unresolved_link`, or the body finding's kind.
    pub code: &'static str,
    /// The file.
    pub id: String,
    /// The header field, or `body`.
    pub field: String,
    /// The link text, or the body finding's detail.
    pub text: String,
}

/// A committed write.
#[derive(Debug)]
pub struct Written {
    /// The full sha of the commit.
    pub commit: String,
    /// Every finding the written files hold. None is new, except a body finding, which never refuses.
    pub findings: Vec<Held>,
}

impl Vault {
    /// Takes `.bp/lock`, waiting up to 5 s, then refuses `vault_locked`. Before anything is read, the files a writer
    /// that died before its commit left on disk go back to the head.
    pub fn writer(&self) -> Result<Writer<'_>, Refusal> {
        let path = bp_dir(&self.root)?.join("lock");
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| failed(&path, &e))?;
        let start = Instant::now();
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if start.elapsed() < WAIT => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(Refusal::new(
                        Code::VaultLocked,
                        path.display().to_string(),
                        "one writer at a time: a second waits 5 s for .bp/lock (12, Write path)",
                        "rerun the same command once the other write has finished",
                    ));
                }
                Err(TryLockError::Error(e)) => return Err(failed(&path, &e)),
            }
        }
        discard(&self.root)?;
        Ok(Writer { index: Index::open(self)?, vault: self, _lock: lock })
    }
}

impl Writer<'_> {
    /// Validates `changes` as the proposed tree, writes them, and commits them alone with `Bp-Verb` and `Bp-Role`.
    /// Every change differs from the file on disk: a verb answers a repeat with exit 3 before it gets here. A
    /// refusal or a failure leaves the head and the working tree as they were.
    pub fn commit(
        mut self,
        verb: &str,
        role: &str,
        subject: &str,
        changes: &BTreeMap<Id, String>,
    ) -> Result<Written, Refusal> {
        let (files, findings) = self.index.propose(self.vault, changes)?;
        let root = &self.vault.root;
        let paths: Vec<String> = changes.keys().map(|id| format!("{id}.md")).collect();
        let journal = root.join(".bp").join(PENDING);
        std::fs::write(&journal, paths.join("\n") + "\n").map_err(|e| failed(&journal, &e))?;
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        let trailers = format!("Bp-Verb: {verb}\nBp-Role: {role}");
        let done = changes
            .iter()
            .try_for_each(|(id, text)| put(&id.path(root), text))
            .and_then(|()| git(root, &[&["add", "--"], &paths[..]].concat()))
            .and_then(|_| git(root, &[&["commit", "-q", "-m", subject, "-m", &trailers, "--"], &paths[..]].concat()));
        if let Err(e) = done {
            // The refusal the caller sees is the failure, not a second one from the undo.
            let _ = discard(root);
            return Err(e);
        }
        std::fs::remove_file(&journal).map_err(|e| failed(&journal, &e))?;
        let commit = crate::git::Repo::find(root).and_then(|r| r.head()).unwrap_or_default();
        // The head moved, so an update that fails here costs the next read one refresh and loses nothing.
        let _ = self.index.apply(root, &files);
        Ok(Written { commit, findings })
    }
}

impl Index {
    /// Validates the proposed tree: the current tree with `changes` written over it. Each changed file parses, and
    /// holds no refusing finding that its current version does not. Returns the parsed files and every finding they
    /// hold. Links resolve against the proposed tree: a file the same write adds is there.
    pub fn propose(&self, vault: &Vault, changes: &BTreeMap<Id, String>) -> Result<(Vec<File>, Vec<Held>), Refusal> {
        let mut files = Vec::with_capacity(changes.len());
        let mut all = Vec::new();
        for (id, text) in changes {
            let file = File::from_bytes(id.clone(), text.as_bytes())?;
            let before = match vault.read(id) {
                Ok(old) => self.held(&old, None)?,
                Err(e) if e.code == Code::NoNode => Vec::new(),
                Err(e) => return Err(e),
            };
            let now = self.held(&file, Some(changes))?;
            for new in now.iter().filter(|h| !before.contains(h)) {
                if new.code == Code::UnresolvedLink.as_str() {
                    return Err(Refusal::new(
                        Code::UnresolvedLink,
                        id.as_str(),
                        "a write adds no link that resolves to nothing in the proposed tree (12, Write path)",
                        format!(
                            "`{}` names `{}`; point it at an id that exists: bp search {}",
                            new.field, new.text, new.text
                        ),
                    ));
                }
                if new.code == Code::CheckUnnamed.as_str() {
                    return Err(Refusal::new(
                        Code::CheckUnnamed,
                        id.as_str(),
                        "every check block has a name (15, the check grammar)",
                        format!("add name=<slug> to the block `{}`", new.text),
                    ));
                }
            }
            all.extend(now);
            files.push(file);
        }
        Ok((files, all))
    }

    /// The findings one file holds against the current tree, or against the proposed tree when `proposed` is given.
    fn held(&self, file: &File, proposed: Option<&BTreeMap<Id, String>>) -> Result<Vec<Held>, Refusal> {
        let resolves = |target: &Id| -> Result<bool, Refusal> {
            if proposed.is_some_and(|p| p.contains_key(target)) {
                return Ok(true);
            }
            self.conn
                .prepare_cached(
                    "select exists (select 1 from files where id = ?1) or exists (select 1 from aliases where alias = ?1)",
                )
                .and_then(|mut st| st.query_row([target.as_str()], |r| r.get(0)))
                .map_err(|e| failed(Path::new(".bp/index.db"), &e))
        };
        let id = file.id.as_str();
        let held = |code: &'static str, field: &str, text: &str| Held {
            code,
            id: id.into(),
            field: field.into(),
            text: text.into(),
        };
        let link = Code::UnresolvedLink.as_str();
        let mut out = Vec::new();
        if let Some(h) = &file.header {
            out.extend(h.unresolved.iter().map(|u| held(link, &u.field, &u.text)));
            let ids = h.links.iter().flat_map(|(k, ids)| ids.iter().map(move |i| (k.as_str(), i)));
            for (field, target) in h.under.iter().map(|u| ("under", u)).chain(ids) {
                if !resolves(target)? {
                    out.push(held(link, field, target.as_str()));
                }
            }
        }
        out.extend(file.body.unresolved.iter().map(|t| held(link, "body", t)));
        for target in &file.body.links {
            if !resolves(target)? {
                out.push(held(link, "body", target.as_str()));
            }
        }
        out.extend(file.body.findings.iter().map(|f| held(f.kind.as_str(), "body", &f.detail)));
        Ok(out)
    }

    /// Takes the committed files and the fingerprint after the commit, in one transaction.
    fn apply(&mut self, root: &Path, files: &[File]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        for f in files {
            remove(&tx, f.id.as_str())?;
            insert(&tx, f)?;
        }
        crate::walk::rebuild_graph(&tx)?;
        tx.execute("insert or replace into meta values ('fingerprint', ?1)", [fingerprint(root)])?;
        tx.commit()
    }
}

/// The temporary file beside `path`: a dot name, so no reader takes it for a node.
fn temp(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.bp-tmp"))
}

/// Writes one file through a temporary file and a rename, so a reader sees the old file or the new one.
fn put(path: &Path, text: &str) -> Result<(), Refusal> {
    let io = |e: std::io::Error| failed(path, &e);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = temp(path);
    std::fs::write(&tmp, text).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

/// Undoes what a writer that died before its commit left, as its journal names it: the temporary files go, a path
/// the head holds returns to the head in the working tree and git's index, and any other path goes. A human's other
/// files are never touched.
fn discard(root: &Path) -> Result<(), Refusal> {
    let journal = root.join(".bp").join(PENDING);
    let Ok(text) = std::fs::read_to_string(&journal) else { return Ok(()) };
    let paths: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    // A journal cut short names nothing; git given no path would act on the whole tree.
    if paths.is_empty() {
        return std::fs::remove_file(&journal).map_err(|e| failed(&journal, &e));
    }
    for p in &paths {
        let _ = std::fs::remove_file(temp(&root.join(p)));
    }
    let listed = git(root, &[&["ls-tree", "-z", "--name-only", "HEAD", "--"], &paths[..]].concat())?;
    let tracked: Vec<&str> = listed.split('\0').filter(|p| !p.is_empty()).collect();
    git(root, &[&["reset", "-q", "--"], &paths[..]].concat())?;
    if !tracked.is_empty() {
        git(root, &[&["checkout", "-q", "HEAD", "--"], &tracked[..]].concat())?;
    }
    for p in paths.iter().filter(|p| !tracked.contains(p)) {
        let _ = std::fs::remove_file(root.join(p));
    }
    std::fs::remove_file(&journal).map_err(|e| failed(&journal, &e))
}

/// Runs git in the vault and returns its stdout. It branches on the exit code alone; `LC_ALL=C` keeps the text in
/// a refusal readable whatever the locale.
fn git(root: &Path, args: &[&str]) -> Result<String, Refusal> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .map_err(|e| Refusal::failed("git", format!("{e}; install git")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    Err(Refusal::failed(
        format!("git {}", args[0]),
        format!(
            "git exited {}: {}; see git -C {} status",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim(),
            root.display()
        ),
    ))
}

//! The write path (12, Write path). A writer holds `.bp/lock`, an OS file lock that dies with its process. A target
//! path with uncommitted edits refuses `path_dirty` first, so the current tree is HEAD and a write never takes or
//! loses a human's work. The writer builds the proposed tree, the changed files in memory, and refuses only a finding
//! the proposed tree holds that the current tree does not (25, design change 1): the converted vault holds hundreds
//! of broken links no write made, and refusing on them would refuse every write. Before the first file is written,
//! the journal `.bp/pending` holds each path and its pre-image. Each file goes through a temporary file and a
//! rename, then one commit of the changed paths with the trailers, then the index takes the changed files and the new
//! fingerprint, so the next read rehashes nothing. On a failure, or in the next writer after one died, the journal's
//! pre-images go back. git's own `index.lock` is never deleted.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::TryLockError;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::index::failed;
use crate::{Code, Id, Index, Refusal, Vault};

/// How long a second writer waits for the lock (12, An error case).
const WAIT: Duration = Duration::from_secs(5);

/// The journal under `.bp/`: one line of JSON, the head and each path with its pre-image's length, then the
/// pre-images end to end. It is renamed into place whole, so it is there, complete, or not there.
const PENDING: &str = "pending";

/// The one writer of a vault, holding `.bp/lock` until dropped.
pub struct Writer<'v> {
    vault: &'v Vault,
    index: Index,
    /// The head and the paths `clean` found clean, so `commit` asks git once. A cell, so a verb can clean while it
    /// reads through [`Writer::index`].
    clean: Cell<Option<(String, Vec<String>)>>,
    /// Trailer lines after `Bp-Verb`, `Bp-Role` and `Bp-Session`.
    trailers: Vec<String>,
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

/// What a write moves besides the markdown it writes.
#[derive(Debug, Default)]
pub struct Moves {
    /// Each new id and the id it had, which the write removes.
    pub ids: BTreeMap<Id, Id>,
    /// Each file that is not markdown, as its old and new paths under the root.
    pub files: Vec<(String, String)>,
}

impl Moves {
    /// `paths` as git takes them, sorted: a path under a moved node's old directory goes as that directory, and one
    /// under its new directory too when `new` is set. A rename moves every file under the old directory, so the
    /// directory names the same files, and git matches one pathspec, not one per file (25, issue 9: a 95-path
    /// rename). The new directory stands for its files only once the rename is admitted: before, `status` would
    /// report a directory HEAD holds as dirty files, which 12 refuses as taken.
    pub fn pathspecs(&self, paths: &[impl AsRef<str>], new: bool) -> Vec<String> {
        let dirs: BTreeSet<&str> = self
            .ids
            .iter()
            .flat_map(|(to, from)| [Some(from.as_str()), new.then_some(to.as_str())])
            .flatten()
            .collect();
        let specs: BTreeSet<&str> = paths
            .iter()
            .map(AsRef::as_ref)
            .map(|p| p.match_indices('/').map(|(i, _)| &p[..i]).find(|d| dirs.contains(d)).unwrap_or(p))
            .collect();
        specs.into_iter().map(str::to_string).collect()
    }
}

/// The journal's first line: the head the write started from, and each path with its pre-image's length, or none
/// for a path the write creates.
#[derive(Serialize, Deserialize)]
struct Journal {
    head: String,
    paths: Vec<(String, Option<usize>)>,
}

impl Vault {
    /// Refuses a vault with no git commit, then takes `.bp/lock`, waiting up to 5 s, then refuses `vault_locked`.
    /// Before anything is read, the files a writer that died before its commit left go back to their pre-images.
    pub fn writer(&self) -> Result<Writer<'_>, Refusal> {
        // A write ends in a commit; without a head, nothing may touch the disk, `.bp/` included.
        if crate::git::Repo::find(&self.root).and_then(|r| r.head()).is_none() {
            let root = self.root.display();
            return Err(Refusal::failed(
                root.to_string(),
                format!(
                    "a write commits to the vault's git repository, which has no commit; git -C {root} init, commit once, then rerun"
                ),
            ));
        }
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
        restore(&self.root)?;
        Ok(Writer { index: Index::open(self)?, vault: self, clean: Cell::new(None), trailers: Vec::new(), _lock: lock })
    }

    /// Refuses `path_dirty` when a file of `ids` differs from HEAD in the working tree or git's index, or is on disk
    /// though HEAD lacks it. It takes no lock, so a dry run refuses what the write would (`write_results.dry_run`).
    pub fn clean<'i>(&self, ids: impl IntoIterator<Item = &'i Id>) -> Result<(), Refusal> {
        clean(&self.root, ids).map(drop)
    }

    /// [`Vault::clean`] over paths relative to the root, for a rename's dry run.
    pub fn clean_paths(&self, paths: &[String]) -> Result<(), Refusal> {
        head_if_clean(&self.root, &paths.iter().map(String::as_str).collect::<Vec<_>>()).map(drop)
    }
}

impl Writer<'_> {
    /// The index, opened under the lock. A verb reads through it rather than opening a second, so it reads the
    /// index no other write can move before this one commits, and opens it once.
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// `Vault::clean`, under the lock; `commit` reuses the head it read.
    pub fn clean<'i>(&self, ids: impl IntoIterator<Item = &'i Id>) -> Result<(), Refusal> {
        self.clean.set(Some(clean(&self.vault.root, ids)?));
        Ok(())
    }

    /// [`Writer::clean`] over paths relative to the root: a node's directory holds files that are not markdown.
    pub fn clean_paths(&self, paths: Vec<String>) -> Result<(), Refusal> {
        let head = head_if_clean(&self.vault.root, &paths.iter().map(String::as_str).collect::<Vec<_>>())?;
        self.clean.set(Some((head, paths)));
        Ok(())
    }

    /// Adds a trailer line to the commit, after `Bp-Verb` and `Bp-Role`: `stamp`'s `Bp-Version` and each `Bp-Delta`
    /// (12, Write path). The caller keeps it to one line, so no text of it reads as a second trailer.
    pub fn trailer(&mut self, line: String) {
        self.trailers.push(line);
    }

    /// Validates `changes` as the proposed tree, writes them, and commits them alone with `Bp-Verb` and `Bp-Role`.
    /// Every change differs from the file on disk: a verb answers a repeat with exit 3 before it gets here. A
    /// refusal or a failure leaves the head and the working tree as they were.
    pub fn commit(
        self,
        verb: &str,
        role: &str,
        subject: &str,
        changes: &BTreeMap<Id, String>,
    ) -> Result<Written, Refusal> {
        self.commit_moving(verb, role, subject, changes, &Moves::default())
    }

    /// [`Writer::commit`] for a write that also moves files (12, A rename): each old id of `moves` goes, and each
    /// file that is not markdown is copied to its new path and removed from its old one, in the same commit.
    pub fn commit_moving(
        mut self,
        verb: &str,
        role: &str,
        subject: &str,
        changes: &BTreeMap<Id, String>,
        moves: &Moves,
    ) -> Result<Written, Refusal> {
        let root = &self.vault.root;
        let gone: Vec<String> = moves.ids.values().map(|id| format!("{id}.md")).collect();
        let mut paths: Vec<String> = changes.keys().map(|id| format!("{id}.md")).collect();
        paths.extend(gone.iter().cloned());
        paths.extend(moves.files.iter().flat_map(|(from, to)| [from.clone(), to.clone()]));
        let checked = moves.pathspecs(&paths, false);
        let head = match self.clean.take() {
            Some((head, clean)) if checked.iter().all(|p| clean.contains(p)) => head,
            _ => head_if_clean(root, &checked.iter().map(String::as_str).collect::<Vec<_>>())?,
        };
        let specs = moves.pathspecs(&paths, true);
        let specs: Vec<&str> = specs.iter().map(String::as_str).collect();
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        let (files, findings) = self.index.propose_moving(self.vault, changes, &moves.ids)?;
        journal(root, head, &paths)?;
        let mut trailers = format!("Bp-Verb: {verb}\nBp-Role: {role}");
        // The session-end hook finds the session's last node by this trailer (12). A value with a space or a line
        // end could forge a second trailer, so it is not written.
        if let Some(s) = std::env::var("BP_SESSION").ok().filter(|s| !s.is_empty() && !s.contains(char::is_whitespace))
        {
            trailers = format!("{trailers}\nBp-Session: {s}");
        }
        for line in &self.trailers {
            trailers = format!("{trailers}\n{line}");
        }
        // `commit -- <paths>` stages a tracked path itself, a removed one too; only a path HEAD lacks needs `add`,
        // so a label write spawns git twice, as a plain one-file commit does.
        let mut new: Vec<&str> =
            paths.iter().zip(changes.keys()).filter(|(_, id)| !id.path(root).exists()).map(|(p, _)| *p).collect();
        new.extend(moves.files.iter().map(|(_, to)| to.as_str()));
        let new = moves.pathspecs(&new, true);
        let new: Vec<&str> = new.iter().map(String::as_str).collect();
        let done = changes
            .iter()
            .try_for_each(|(id, text)| put(&id.path(root), text.as_bytes(), false))
            .and_then(|()| {
                moves.files.iter().try_for_each(|(from, to)| {
                    let bytes = std::fs::read(root.join(from)).map_err(|e| failed(&root.join(from), &e))?;
                    put(&root.join(to), &bytes, false)
                })
            })
            .and_then(|()| {
                gone.iter()
                    .chain(moves.files.iter().map(|(from, _)| from))
                    .try_for_each(|p| std::fs::remove_file(root.join(p)).map_err(|e| failed(&root.join(p), &e)))
            })
            .and_then(
                |()| if new.is_empty() { Ok(String::new()) } else { git(root, &[&["add", "--"], &new[..]].concat()) },
            )
            .and_then(|_| git(root, &[&["commit", "-q", "-m", subject, "-m", &trailers, "--"], &specs[..]].concat()));
        if let Err(e) = done {
            // The caller sees the failure, not a second refusal from the undo. An undo that fails keeps the journal,
            // and the next writer tries again.
            let _ = restore(root);
            return Err(e);
        }
        let journal = root.join(".bp").join(PENDING);
        std::fs::remove_file(&journal).map_err(|e| failed(&journal, &e))?;
        // The ref files name the new head; reading them spares a git process.
        let commit = crate::git::Repo::find(root).and_then(|r| r.head()).ok_or_else(|| {
            Refusal::failed(root.display().to_string(), "git committed but its head cannot be read; rerun bp where")
        })?;
        // git keeps no directory, so the emptied ones go too, before the fingerprint reads their parents' times.
        for p in gone.iter().chain(moves.files.iter().map(|(from, _)| from)) {
            prune(root, p);
        }
        // The head moved, so an update that fails here costs the next read one refresh and loses nothing.
        let _ = self.index.apply(root, &files, &moves.ids);
        Ok(Written { commit, findings })
    }
}

/// `.bp/`, made on first use with a `.gitignore` of its own.
pub(crate) fn bp_dir(root: &Path) -> Result<PathBuf, Refusal> {
    let dir = root.join(".bp");
    std::fs::create_dir_all(&dir).map_err(|e| failed(&dir, &e))?;
    // `.bp/` holds derived state only; ignoring it here keeps the vault's own .gitignore untouched.
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n").map_err(|e| failed(&ignore, &e))?;
    }
    Ok(dir)
}

/// The head and the paths of `ids`, once none holds an uncommitted edit.
fn clean<'i>(root: &Path, ids: impl IntoIterator<Item = &'i Id>) -> Result<(String, Vec<String>), Refusal> {
    let paths: Vec<String> = ids.into_iter().map(|id| format!("{id}.md")).collect();
    let head = head_if_clean(root, &paths.iter().map(String::as_str).collect::<Vec<_>>())?;
    Ok((head, paths))
}

/// The head's sha, once no path of `paths` holds an uncommitted edit: staged, unstaged, untracked or ignored.
fn head_if_clean(root: &Path, paths: &[&str]) -> Result<String, Refusal> {
    let status = ["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all", "--ignored=matching", "--"];
    let out = git(root, &[&status[..], paths].concat())?;
    let mut head = String::new();
    for entry in out.split('\0') {
        if let Some(oid) = entry.strip_prefix("# branch.oid ") {
            oid.clone_into(&mut head);
        }
        // The fields before the path: an ordinary change has 8, a rename 9, a conflict 10; untracked and ignored, 1.
        let before = match entry.as_bytes().first() {
            Some(b'1') => 8,
            Some(b'2') => 9,
            Some(b'u') => 10,
            Some(b'?' | b'!') => 1,
            _ => continue,
        };
        let path = entry.splitn(before + 1, ' ').nth(before).unwrap_or(entry);
        return Err(Refusal::new(
            Code::PathDirty,
            path,
            "a write starts from the committed file (12)",
            format!("commit or stash {path}, then rerun"),
        ));
    }
    Ok(head)
}

/// Writes the journal for a write from `head` to `paths`, synced, before the first file is written.
fn journal(root: &Path, head: String, paths: &[&str]) -> Result<(), Refusal> {
    let mut entries = Vec::with_capacity(paths.len());
    let mut images = Vec::new();
    for p in paths {
        let len = match std::fs::read(root.join(p)) {
            Ok(bytes) => {
                images.extend_from_slice(&bytes);
                Some(bytes.len())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(failed(&root.join(p), &e)),
        };
        entries.push(((*p).to_string(), len));
    }
    let mut text = serde_json::to_vec(&Journal { head, paths: entries }).expect("a plain struct serialises");
    text.push(b'\n');
    text.extend(images);
    put(&root.join(".bp").join(PENDING), &text, true)
}

/// Undoes a write that did not commit, as its journal names it: each path gets its pre-image back, or goes when it
/// had none, and leaves git's index. A journal whose head is no longer HEAD belongs to a write that committed, or
/// to a head a human moved since; then nothing is undone. A human's other files are never touched.
fn restore(root: &Path) -> Result<(), Refusal> {
    let path = root.join(".bp").join(PENDING);
    let text = match std::fs::read(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(failed(&path, &e)),
    };
    let (line, mut images) = text.split_at(text.iter().position(|b| *b == b'\n').expect("a journal has its line"));
    let journal: Journal = serde_json::from_slice(line).expect("a journal is renamed into place whole");
    images = &images[1..];
    if git(root, &["rev-parse", "HEAD"])?.trim() == journal.head {
        for (p, len) in &journal.paths {
            let target = root.join(p);
            let gone = |r: std::io::Result<()>| match r {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(failed(&target, &e)),
                _ => Ok(()),
            };
            gone(std::fs::remove_file(temp(&target)))?;
            if let Some(n) = len {
                let (image, rest) = images.split_at(*n);
                put(&target, image, false)?;
                images = rest;
            } else {
                gone(std::fs::remove_file(&target))?;
                // A crashed rename made the new directories; they go with the files, as after a commit.
                prune(root, p);
            }
        }
        let paths: Vec<&str> = journal.paths.iter().map(|(p, _)| p.as_str()).collect();
        git(root, &[&["reset", "-q", "--"], &paths[..]].concat())?;
    }
    std::fs::remove_file(&path).map_err(|e| failed(&path, &e))
}

/// Removes each directory above `path`, under the root, that is now empty: git keeps no directory.
fn prune(root: &Path, path: &str) {
    let mut dir = root.join(path);
    while dir.pop() && dir != *root && std::fs::remove_dir(&dir).is_ok() {}
}

/// The temporary file beside `path`: a dot name, so no reader takes it for a node.
fn temp(path: &Path) -> PathBuf {
    let name = path.file_name().expect("a written path names a file").to_string_lossy();
    path.with_file_name(format!(".{name}.bp-tmp"))
}

/// Writes one file through a temporary file and a rename, so a reader sees the old file or the new one. Only the
/// journal is synced: it holds every pre-image, so after a power loss the next writer restores the data files from
/// it, and an unsynced data file costs nothing. On macOS a sync is a full flush, the largest part of a write.
fn put(path: &Path, bytes: &[u8], sync: bool) -> Result<(), Refusal> {
    let io = |e: std::io::Error| failed(path, &e);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = temp(path);
    let mut file = std::fs::File::create(&tmp).map_err(io)?;
    file.write_all(bytes).and_then(|()| if sync { file.sync_all() } else { Ok(()) }).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

/// Runs git in the vault and returns its stdout. It branches on the exit code alone; `LC_ALL=C` keeps the text in
/// a refusal readable whatever the locale, and literal pathspecs keep `a[1].md` from naming `a1.md`. Without
/// `--no-optional-locks`, `status` rewrites git's index after a commit, which moves the fingerprint, so the next
/// call after a refused or no-op write would rescan every file; `add` and `commit` still lock it.
fn git(root: &Path, args: &[&str]) -> Result<String, Refusal> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("--no-optional-locks")
        .env("LC_ALL", "C")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .args(args)
        .output()
        .map_err(|e| Refusal::failed("git", format!("{e}; install git")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    // A git process holds git's lock, or one was killed holding it; only a human can tell which.
    if stderr.contains("index.lock")
        && let Some(lock) = crate::git::Repo::find(root).map(|r| r.git.join("index.lock")).filter(|l| l.exists())
    {
        let lock = lock.display();
        return Err(Refusal::failed(
            lock.to_string(),
            format!("a git process holds it, or one died; remove {lock} if no git process runs, then rerun"),
        ));
    }
    Err(Refusal::failed(
        format!("git {}", args[0]),
        format!(
            "git exited {}: {}; see git -C {} status",
            out.status.code().unwrap_or(-1),
            stderr.trim(),
            root.display()
        ),
    ))
}

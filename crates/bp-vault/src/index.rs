//! The index: `.bp/index.db`, derived from the files and holding nothing else (12). It refreshes at open when the
//! vault's fingerprint moved: every file is hashed, and only files whose content hash changed are parsed again.
//!
//! The fingerprint is the git head plus the modification time of every kind directory. The tool is the single
//! writer and commits every write, so the head moves on every tool write; a kind directory's time moves when a
//! file is added or removed outside the tool. An uncommitted in-place edit is seen at the next commit.

use std::fmt::Write as _;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

use crate::{Counts, File, Id, Refusal, Vault};

/// Bump on any change to the tables or to what the parser puts in them; a mismatch rebuilds from nothing.
const SCHEMA: i64 = 1;

const TABLES: &str = "
create table meta (key text primary key, value text not null);
create table files (id text primary key, hash text not null, class text not null);
create table nodes (rowid integer primary key, id text unique not null, kind text not null, title text,
  status text, project text, under text, header text not null);
create index nodes_kind on nodes (kind, project, status);
create table records (id text primary key, kind text not null, under text, header text not null);
create index records_under on records (under, kind);
create table links (src text not null, type text not null, dst text not null);
create index links_src on links (src);
create index links_dst on links (dst);
create virtual table fts using fts5 (id, title, body, tokenize = 'unicode61');
";

/// An open, fresh index.
pub struct Index {
    conn: Connection,
}

/// One row of `list`.
#[derive(Debug, Serialize)]
pub struct Row {
    /// The path id.
    pub id: String,
    /// The kind.
    pub kind: String,
    /// The title; records have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The status; records have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub under: Option<String>,
}

/// One row of `search`.
#[derive(Debug, Serialize)]
pub struct Hit {
    /// The path id.
    pub id: String,
    /// The kind.
    pub kind: String,
    /// The title.
    pub title: Option<String>,
    /// BM25, higher is better.
    pub rank: f64,
    /// Matching text with the terms in brackets.
    pub snippet: String,
}

/// The filters of `list`.
#[derive(Debug, Default)]
pub struct Filter<'a> {
    /// The kind; required.
    pub kind: &'a str,
    /// Exact status.
    pub status: Option<&'a str>,
    /// The project segment of the id.
    pub project: Option<&'a str>,
    /// The parent id.
    pub under: Option<&'a str>,
    /// At most this many rows.
    pub limit: usize,
}

impl Index {
    /// Opens `.bp/index.db` under the vault, creating or refreshing it first when the fingerprint moved.
    pub fn open(vault: &Vault) -> Result<Self, Refusal> {
        let dir = vault.root.join(".bp");
        std::fs::create_dir_all(&dir).map_err(|e| failed(&dir, &e))?;
        // `.bp/` holds derived state only; ignoring it here keeps the vault's own .gitignore untouched.
        let ignore = dir.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n").map_err(|e| failed(&ignore, &e))?;
        }
        let path = dir.join("index.db");
        let conn = Connection::open(&path).map_err(|e| failed(&path, &e))?;
        conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(|e| failed(&path, &e))?;
        let mut index = Self { conn };
        let print = fingerprint(&vault.root);
        if index.meta("fingerprint").as_deref() != Some(print.as_str()) {
            // A bad header keeps its own refusal; anything else is the index failing.
            index.refresh(vault, &print).map_err(|e| match e.downcast::<Refusal>() {
                Ok(refusal) => *refusal,
                Err(e) => failed(&path, &e),
            })?;
        }
        Ok(index)
    }

    fn meta(&self, key: &str) -> Option<String> {
        self.conn.query_row("select value from meta where key = ?1", [key], |r| r.get(0)).ok()
    }

    fn refresh(&mut self, vault: &Vault, print: &str) -> Result<(), Box<dyn std::error::Error>> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let schema: Option<String> =
            tx.query_row("select value from meta where key = 'schema'", [], |r| r.get(0)).optional().unwrap_or(None);
        if schema != Some(SCHEMA.to_string()) {
            let names: Vec<String> = tx
                .prepare("select name from sqlite_master where type in ('table', 'view') and name not like 'fts_%'")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            for name in names {
                tx.execute_batch(&format!("drop table if exists \"{name}\""))?;
            }
            tx.execute_batch(TABLES)?;
            tx.execute("insert into meta values ('schema', ?1)", [SCHEMA.to_string()])?;
        }
        let mut known: std::collections::HashMap<String, String> = tx
            .prepare("select id, hash from files")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for id in vault.ids()? {
            let bytes = std::fs::read(id.path(&vault.root))?;
            let hash = blake3::hash(&bytes).to_hex().to_string();
            if known.remove(id.as_str()).as_deref() == Some(hash.as_str()) {
                continue;
            }
            remove(&tx, id.as_str())?;
            let mut file = File::parse(id, &String::from_utf8_lossy(&bytes))?;
            file.hash = hash;
            insert(&tx, &file)?;
        }
        for gone in known.keys() {
            remove(&tx, gone)?;
        }
        tx.execute("insert or replace into meta values ('fingerprint', ?1)", [print])?;
        tx.commit()?;
        Ok(())
    }

    /// The census, from the index.
    pub fn counts(&self) -> Result<Counts, Refusal> {
        let run = || -> rusqlite::Result<Counts> {
            let mut c = Counts {
                files: self.conn.query_row("select count(*) from files", [], count)?,
                no_header: self.conn.query_row("select count(*) from files where class = 'asset'", [], count)?,
                ..Counts::default()
            };
            for (table, into) in [("nodes", &mut c.nodes), ("records", &mut c.records)] {
                let mut st = self.conn.prepare(&format!("select kind, count(*) from {table} group by kind"))?;
                for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, count_at(r, 1)?)))? {
                    let (k, n) = row?;
                    into.insert(k, n);
                }
            }
            Ok(c)
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// Rows of one kind, ordered by id, at most `limit`, and the total before the limit.
    pub fn list(&self, f: &Filter) -> Result<(Vec<Row>, usize), Refusal> {
        let run = || -> rusqlite::Result<(Vec<Row>, usize)> {
            let is_record: bool =
                self.conn.query_row("select exists (select 1 from records where kind = ?1)", [f.kind], |r| r.get(0))?;
            let (from, cols) = if is_record {
                ("records", "id, kind, null, null, under")
            } else {
                ("nodes", "id, kind, title, status, under")
            };
            // Records carry no status, so a status filter on a record kind matches nothing.
            let status = if is_record { "?2 is null" } else { "(?2 is null or status = ?2)" };
            let filter = format!(
                "where kind = ?1 and {status} and (?3 is null or id like 'projects/' || ?3 || '/%') and (?4 is null or under = ?4)"
            );
            let under = f.under.map(|u| Id::new(u).as_str().to_string());
            let args = params![f.kind, f.status, f.project, under];
            let total = self.conn.query_row(&format!("select count(*) from {from} {filter}"), args, count)?;
            let mut st =
                self.conn.prepare(&format!("select {cols} from {from} {filter} order by id limit {}", f.limit))?;
            let rows = st
                .query_map(args, |r| {
                    Ok(Row { id: r.get(0)?, kind: r.get(1)?, title: r.get(2)?, status: r.get(3)?, under: r.get(4)? })
                })?
                .collect::<rusqlite::Result<_>>()?;
            Ok((rows, total))
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// BM25 over ids, titles and bodies of nodes. Every term must match; when none does, any term may.
    pub fn search(
        &self,
        terms: &[String],
        kind: Option<&str>,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Hit>, Refusal> {
        // Quoting each term makes it a literal: FTS5 operators and punctuation in agent input cannot break the query.
        let quoted: Vec<String> = terms
            .iter()
            .flat_map(|t| t.split_whitespace())
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect();
        let run = |query: &str| -> rusqlite::Result<Vec<Hit>> {
            let mut st = self.conn.prepare(
                "select n.id, n.kind, n.title, -bm25(fts, 10.0, 5.0, 1.0) as r, snippet(fts, 2, '[', ']', '…', 16)
                 from fts join nodes n on n.rowid = fts.rowid
                 where fts match ?1 and (?2 is null or n.kind = ?2) and (?3 is null or n.id like 'projects/' || ?3 || '/%')
                 order by r desc limit ?4",
            )?;
            st.query_map(params![query, kind, project, i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
                Ok(Hit { id: r.get(0)?, kind: r.get(1)?, title: r.get(2)?, rank: r.get(3)?, snippet: r.get(4)? })
            })?
            .collect()
        };
        let all = run(&quoted.join(" ")).map_err(|e| failed(Path::new(".bp/index.db"), &e))?;
        if !all.is_empty() || quoted.len() < 2 {
            return Ok(all);
        }
        run(&quoted.join(" OR ")).map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }
}

/// `SQLite` counts are `i64`; a count is never negative.
fn count_at(r: &rusqlite::Row, i: usize) -> rusqlite::Result<usize> {
    Ok(usize::try_from(r.get::<_, i64>(i)?).unwrap_or_default())
}

fn count(r: &rusqlite::Row) -> rusqlite::Result<usize> {
    count_at(r, 0)
}

fn insert(tx: &rusqlite::Transaction, file: &File) -> rusqlite::Result<()> {
    let id = file.id.as_str();
    let Some(header) = &file.header else {
        tx.execute("insert into files values (?1, ?2, 'asset')", params![id, file.hash])?;
        return Ok(());
    };
    let json = Value::Object(header.to_json()).to_string();
    let under = header.under.as_ref().map(Id::as_str);
    if file.id.is_record() {
        tx.execute("insert into files values (?1, ?2, 'record')", params![id, file.hash])?;
        tx.execute("insert into records values (?1, ?2, ?3, ?4)", params![id, header.kind, under, json])?;
    } else {
        tx.execute("insert into files values (?1, ?2, 'node')", params![id, file.hash])?;
        tx.execute(
            "insert into nodes (id, kind, title, status, project, under, header) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, header.kind, header.title, header.status, file.id.project(), under, json],
        )?;
        tx.execute(
            "insert into fts (rowid, id, title, body) values (last_insert_rowid(), ?1, ?2, ?3)",
            params![id, header.title, file.body.text],
        )?;
    }
    let mut st = tx.prepare_cached("insert into links values (?1, ?2, ?3)")?;
    if let Some(u) = under {
        st.execute(params![id, "under", u])?;
    }
    for (kind, dsts) in &header.links {
        for d in dsts {
            st.execute(params![id, kind, d.as_str()])?;
        }
    }
    for d in &file.body.links {
        st.execute(params![id, "body", d.as_str()])?;
    }
    Ok(())
}

fn remove(tx: &rusqlite::Transaction, id: &str) -> rusqlite::Result<()> {
    tx.execute("delete from fts where rowid = (select rowid from nodes where id = ?1)", [id])?;
    for table in ["files", "nodes", "records"] {
        tx.execute(&format!("delete from {table} where id = ?1"), [id])?;
    }
    tx.execute("delete from links where src = ?1", [id])?;
    Ok(())
}

/// The git head and the modification time of `projects/`, `shared/`, every project and every kind directory.
fn fingerprint(root: &Path) -> String {
    let mut print = head(root).unwrap_or_default();
    let mut dirs = vec![root.join("projects"), root.join("shared")];
    if let Ok(projects) = std::fs::read_dir(root.join("projects")) {
        dirs.extend(projects.flatten().map(|e| e.path()));
    }
    let mut kinds = Vec::new();
    for d in &dirs[1..] {
        if let Ok(entries) = std::fs::read_dir(d) {
            kinds.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    dirs.extend(kinds);
    for d in dirs {
        if let Ok(t) = std::fs::metadata(&d).and_then(|m| m.modified()) {
            let nanos = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
            let _ = write!(print, "\n{}={nanos}", d.display());
        }
    }
    print
}

/// The commit `HEAD` names, read from `.git` without a subprocess. `None` outside a git repository.
fn head(root: &Path) -> Option<String> {
    let mut git = root.join(".git");
    if git.is_file() {
        let text = std::fs::read_to_string(&git).ok()?;
        git = root.join(text.strip_prefix("gitdir:")?.trim());
    }
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let Some(name) = head.trim().strip_prefix("ref: ") else { return Some(head.trim().to_string()) };
    // A linked worktree keeps its refs in the common directory.
    let common = std::fs::read_to_string(git.join("commondir")).map_or_else(|_| git.clone(), |c| git.join(c.trim()));
    if let Ok(sha) = std::fs::read_to_string(common.join(name)) {
        return Some(sha.trim().to_string());
    }
    let packed = std::fs::read_to_string(common.join("packed-refs")).ok()?;
    packed.lines().find_map(|l| l.strip_suffix(name).map(|s| s.trim().to_string()))
}

fn failed(subject: &Path, e: &dyn std::fmt::Display) -> Refusal {
    Refusal::failed("index_failed", subject.display().to_string(), format!("{e}; delete .bp/index.db and rerun"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, id: &str, text: &str) {
        let path = Id::new(id).path(root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn issue(title: &str, status: &str) -> String {
        format!(
            "---\nkind: issue\ntitle: {title}\nstatus: {status}\nunder: projects/bp/milestones/m\ncites: [\"projects/bp/designs/system\"]\n---\n\nThe walk keeps to its budget [[shared/learnings/l]].\n"
        )
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "projects/bp/issues/0001.a", &issue("Walk budget", "open"));
        write(root, "projects/bp/issues/0002.b", &issue("Other", "resolved"));
        write(root, "projects/bp/issues/0001.a/claims/s1", "---\nkind: claim\nunder: projects/bp/issues/0001.a\n---\n");
        write(root, "projects/ex/issues/0001.c", &issue("Elsewhere", "open"));
        write(root, "shared/assets/glossary", "# Glossary\n");
        let v = Vault { root: root.to_path_buf() };
        (dir, v)
    }

    fn all(kind: &str) -> Filter<'_> {
        Filter { kind, limit: 50, ..Filter::default() }
    }

    #[test]
    fn lists_filters_and_truncates() {
        let (_d, v) = vault();
        let index = Index::open(&v).unwrap();
        let (rows, total) = index.list(&all("issue")).unwrap();
        assert_eq!((rows.len(), total), (3, 3));
        let (rows, _) = index.list(&Filter { status: Some("open"), project: Some("bp"), ..all("issue") }).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title.as_deref(), Some("Walk budget"));
        let (rows, total) = index.list(&Filter { limit: 1, ..all("issue") }).unwrap();
        assert_eq!((rows.len(), total), (1, 3));
        let (rows, _) = index.list(&Filter { under: Some("[[projects/bp/issues/0001.a]]"), ..all("claim") }).unwrap();
        assert_eq!(rows[0].under.as_deref(), Some("projects/bp/issues/0001.a"));
        assert!(index.list(&Filter { status: Some("open"), ..all("claim") }).unwrap().0.is_empty());
        let c = index.counts().unwrap();
        assert_eq!((c.files, c.no_header, c.nodes["issue"], c.records["claim"]), (5, 1, 3, 1));
        assert!(v.root.join(".bp/.gitignore").is_file());
    }

    #[test]
    fn searches_with_literal_terms_and_falls_back_to_any() {
        let (_d, v) = vault();
        let index = Index::open(&v).unwrap();
        let hits = index.search(&["walk budget".into()], None, Some("bp"), 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, "projects/bp/issues/0001.a", "the title match ranks first");
        assert!(hits[0].snippet.contains("[walk]"));
        assert_eq!(index.search(&["walk nothingmatches".into()], None, None, 10).unwrap().len(), 3);
        assert!(index.search(&["\"(AND".into()], None, None, 10).unwrap().is_empty());
    }

    #[test]
    fn refreshes_when_a_file_comes_goes_or_the_head_moves() {
        let (_d, v) = vault();
        let count = |v: &Vault| Index::open(v).unwrap().list(&all("issue")).unwrap().1;
        assert_eq!(count(&v), 3);
        write(&v.root, "projects/bp/issues/0003.d", &issue("New", "open"));
        assert_eq!(count(&v), 4);
        std::fs::remove_file(Id::new("projects/bp/issues/0002.b").path(&v.root)).unwrap();
        assert_eq!(count(&v), 3);
        // An in-place edit is seen when the head moves, as every tool write commits.
        std::fs::create_dir_all(v.root.join(".git")).unwrap();
        std::fs::write(v.root.join(".git/HEAD"), "aaaa\n").unwrap();
        Index::open(&v).unwrap();
        write(&v.root, "projects/bp/issues/0001.a", &issue("Walk budget", "resolved"));
        std::fs::write(v.root.join(".git/HEAD"), "bbbb\n").unwrap();
        let index = Index::open(&v).unwrap();
        let (rows, _) = index.list(&Filter { status: Some("resolved"), ..all("issue") }).unwrap();
        assert_eq!(rows[0].id, "projects/bp/issues/0001.a");
    }

    #[test]
    fn a_bad_header_keeps_its_refusal() {
        let (_d, v) = vault();
        write(&v.root, "projects/bp/issues/0009.bad", "---\ntitle: no kind\n---\n");
        let e = Index::open(&v).err().unwrap();
        assert_eq!((e.code, e.exit), ("bad_header", 2));
    }

    #[test]
    fn reads_the_head_through_a_ref_and_packed_refs() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/master\n").unwrap();
        std::fs::write(git.join("packed-refs"), "# pack-refs\ncafe refs/heads/master\n").unwrap();
        assert_eq!(head(dir.path()).as_deref(), Some("cafe"));
        std::fs::write(git.join("refs/heads/master"), "beef\n").unwrap();
        assert_eq!(head(dir.path()).as_deref(), Some("beef"));
        assert_eq!(head(&dir.path().join("none")), None);
    }
}

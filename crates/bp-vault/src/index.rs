//! The index: `.bp/index.db`, derived from the files and holding nothing else (12). It refreshes at open when the
//! vault's fingerprint moved: every file is hashed, and only files whose content hash changed are parsed again.
//!
//! The fingerprint is cheap on purpose; hashing every file costs about 240 ms against a call budget of a few. It
//! holds the git head, the time and size of git's own index (checkout, reset, stash and add rewrite it), and the
//! time of every kind directory and design subdirectory (a node file added or removed). The tool is the single
//! writer and commits every write, so a tool write always moves the head. Two changes stay unseen until a commit:
//! an in-place edit, and a record added or removed by hand under a node. The census (`list --count-only`) hashes
//! every file whatever the fingerprint says, so it is the read that reconciles them.

use std::fmt::Write as _;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

use crate::{Counts, File, Kind, Refusal, Vault};

/// Bump on any change to what the parser puts in the tables; a mismatch rebuilds from nothing. A change to the
/// tables themselves rebuilds without a bump, because the schema key carries their hash: two branches that both
/// bump to the same number still differ.
const SCHEMA: i64 = 3;

fn schema() -> String {
    format!("{SCHEMA}-{}", &blake3::hash(TABLES.as_bytes()).to_hex()[..12])
}

const TABLES: &str = "
create table meta (key text primary key, value text not null);
create table files (id text primary key, hash text not null, class text not null);
create table nodes (rowid integer primary key, id text unique not null, kind text not null, title text,
  status text, project text, under text, header text not null);
create index nodes_kind on nodes (kind, project, status);
create table records (id text primary key, kind text not null, project text, under text, header text not null);
create index records_under on records (under, kind);
create table links (src text not null, type text not null, dst text not null);
create table unresolved (src text not null, field text not null, text text not null);
create index unresolved_src on unresolved (src);
create table aliases (alias text not null, id text not null);
create index aliases_alias on aliases (alias);
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
    /// Matching text from the best column, the terms marked `«` and `»`.
    pub snippet: String,
}

/// The filters of `list`.
#[derive(Debug)]
pub struct Filter<'a> {
    /// The kind.
    pub kind: crate::Kind,
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
            index.refresh_or_fail(vault, &print, &path)?;
        }
        Ok(index)
    }

    /// Hashes every file and parses the changed ones, whatever the fingerprint says: the census's read.
    pub fn reconcile(&mut self, vault: &Vault) -> Result<(), Refusal> {
        let path = vault.root.join(".bp/index.db");
        let print = fingerprint(&vault.root);
        self.conn.execute("delete from meta where key = 'fingerprint'", []).map_err(|e| failed(&path, &e))?;
        self.refresh_or_fail(vault, &print, &path)
    }

    fn refresh_or_fail(&mut self, vault: &Vault, print: &str, path: &Path) -> Result<(), Refusal> {
        // A bad header keeps its own refusal; anything else is the index failing.
        self.refresh(vault, print).map_err(|e| match e.downcast::<Refusal>() {
            Ok(refusal) => *refusal,
            Err(e) => failed(path, &e),
        })
    }

    fn meta(&self, key: &str) -> Option<String> {
        self.conn.query_row("select value from meta where key = ?1", [key], |r| r.get(0)).ok()
    }

    fn refresh(&mut self, vault: &Vault, print: &str) -> Result<(), Box<dyn std::error::Error>> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // A process that waited on the lock finds the refresh another one just committed.
        let current: Option<String> = tx
            .query_row("select value from meta where key = 'fingerprint'", [], |r| r.get(0))
            .optional()
            .unwrap_or(None);
        if current.as_deref() == Some(print) {
            return Ok(());
        }
        let schema: Option<String> =
            tx.query_row("select value from meta where key = 'schema'", [], |r| r.get(0)).optional().unwrap_or(None);
        if schema != Some(self::schema()) {
            let names: Vec<String> = tx
                .prepare("select name from sqlite_master where type in ('table', 'view') and name not like 'fts_%'")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            for name in names {
                tx.execute_batch(&format!("drop table if exists \"{name}\""))?;
            }
            tx.execute_batch(TABLES)?;
            tx.execute("insert into meta values ('schema', ?1)", [self::schema()])?;
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
            insert(&tx, &File::from_bytes(id, &bytes)?)?;
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

    /// Nodes per kind and status, for the census's status check.
    pub fn statuses(&self) -> Result<Vec<(String, Option<String>, usize)>, Refusal> {
        let run = || -> rusqlite::Result<Vec<(String, Option<String>, usize)>> {
            let mut st = self.conn.prepare("select kind, status, count(*) from nodes group by kind, status")?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, count_at(r, 2)?)))?.collect()
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// Links that resolve to no file and no alias (contract code `unresolved_link`): targets that are not path ids,
    /// and path ids that name nothing.
    pub fn unresolved(&self) -> Result<usize, Refusal> {
        let sql = "select (select count(*) from unresolved) + (select count(*) from links l where not exists \
                   (select 1 from files f where f.id = l.dst) and not exists (select 1 from aliases a where a.alias = l.dst))";
        self.conn.query_row(sql, [], count).map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// Rows of one kind, ordered by id, at most `limit`, and the total before the limit. One read transaction, so
    /// the total and the rows come from the same refresh.
    pub fn list(&self, f: &Filter) -> Result<(Vec<Row>, usize), Refusal> {
        let run = || -> rusqlite::Result<(Vec<Row>, usize)> {
            let tx = self.conn.unchecked_transaction()?;
            let is_record = f.kind.is_record();
            let (from, cols) = if is_record {
                ("records", "id, kind, null, null, under")
            } else {
                ("nodes", "id, kind, title, status, under")
            };
            // Records carry no status, so a status filter on a record kind matches nothing.
            let status = if is_record { "?2 is null" } else { "(?2 is null or status = ?2)" };
            let filter =
                format!("where kind = ?1 and {status} and (?3 is null or project = ?3) and (?4 is null or under = ?4)");
            let under = f.under;
            let limit = i64::try_from(f.limit).unwrap_or(i64::MAX);
            let total = tx.query_row(
                &format!("select count(*) from {from} {filter}"),
                params![f.kind.as_str(), f.status, f.project, under],
                count,
            )?;
            let mut st = tx.prepare(&format!("select {cols} from {from} {filter} order by id limit ?5"))?;
            let rows = st
                .query_map(params![f.kind.as_str(), f.status, f.project, under, limit], |r| {
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
        kind: Option<Kind>,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Hit>, Refusal> {
        let quoted: Vec<String> = tokens(terms).map(|t| format!("\"{t}\"")).collect();
        if quoted.is_empty() {
            return Err(Refusal::usage("a search with no words", "bp search <word> [<word>...]"));
        }
        let run = |query: &str| -> rusqlite::Result<Vec<Hit>> {
            // Rank and limit first: SQLite evaluates a select list for every row it sorts, and a snippet is the
            // costly part, so only the rows kept get one.
            let mut st = self.conn.prepare_cached(
                "with top as (
                   select fts.rowid as id, -bm25(fts, 10.0, 5.0, 1.0) as r from fts join nodes n on n.rowid = fts.rowid
                   where fts match ?1 and (?2 is null or n.kind = ?2) and (?3 is null or n.project = ?3)
                   order by r desc limit ?4)
                 select n.id, n.kind, n.title, top.r, snippet(fts, -1, '«', '»', '…', 16)
                 from top join fts on fts.rowid = top.id join nodes n on n.rowid = top.id
                 where fts match ?1 order by top.r desc",
            )?;
            st.query_map(
                params![query, kind.map(Kind::as_str), project, i64::try_from(limit).unwrap_or(i64::MAX)],
                |r| Ok(Hit { id: r.get(0)?, kind: r.get(1)?, title: r.get(2)?, rank: r.get(3)?, snippet: r.get(4)? }),
            )?
            .collect()
        };
        let all = run(&quoted.join(" ")).map_err(|e| failed(Path::new(".bp/index.db"), &e))?;
        if !all.is_empty() || quoted.len() < 2 {
            return Ok(all);
        }
        run(&quoted.join(" OR ")).map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }
}

/// rusqlite reads no `usize`; a `count(*)` is never negative.
fn count_at(r: &rusqlite::Row, i: usize) -> rusqlite::Result<usize> {
    Ok(usize::try_from(r.get::<_, i64>(i)?).expect("a count is never negative"))
}

fn count(r: &rusqlite::Row) -> rusqlite::Result<usize> {
    count_at(r, 0)
}

/// The words of the terms as FTS5 sees them: runs of letters, digits and underscores. Quoting each one makes it a
/// literal, so operators and punctuation in agent input cannot break the query.
fn tokens(terms: &[String]) -> impl Iterator<Item = &str> {
    terms.iter().flat_map(|t| t.split(|c: char| !(c.is_alphanumeric() || c == '_'))).filter(|w| !w.is_empty())
}

fn insert(tx: &rusqlite::Transaction, file: &File) -> rusqlite::Result<()> {
    let id = file.id.as_str();
    let Some(header) = &file.header else {
        tx.execute("insert into files values (?1, ?2, 'asset')", params![id, file.hash])?;
        return Ok(());
    };
    let json = Value::Object(header.to_json()).to_string();
    let under = header.under.as_ref().map(crate::Id::as_str);
    let kind = header.kind.as_str();
    if header.kind.is_record() {
        tx.execute("insert into files values (?1, ?2, 'record')", params![id, file.hash])?;
        tx.execute(
            "insert into records values (?1, ?2, ?3, ?4, ?5)",
            params![id, kind, file.id.project(), under, json],
        )?;
    } else {
        tx.execute("insert into files values (?1, ?2, 'node')", params![id, file.hash])?;
        tx.execute(
            "insert into nodes (id, kind, title, status, project, under, header) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, kind, header.title, header.status, file.id.project(), under, json],
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
    let mut st = tx.prepare_cached("insert into aliases values (?1, ?2)")?;
    for a in &header.aliases {
        st.execute(params![a, id])?;
    }
    let mut st = tx.prepare_cached("insert into unresolved values (?1, ?2, ?3)")?;
    for u in &header.unresolved {
        st.execute(params![id, u.field, u.text])?;
    }
    for text in &file.body.unresolved {
        st.execute(params![id, "body", text])?;
    }
    Ok(())
}

fn remove(tx: &rusqlite::Transaction, id: &str) -> rusqlite::Result<()> {
    tx.execute("delete from fts where rowid = (select rowid from nodes where id = ?1)", [id])?;
    for table in ["files", "nodes", "records"] {
        tx.execute(&format!("delete from {table} where id = ?1"), [id])?;
    }
    tx.execute("delete from links where src = ?1", [id])?;
    tx.execute("delete from unresolved where src = ?1", [id])?;
    tx.execute("delete from aliases where id = ?1", [id])?;
    Ok(())
}

/// The schema, the git head and the modification time of `projects/`, `shared/`, every project and every kind directory.
fn fingerprint(root: &Path) -> String {
    // The schema leads, so a binary with another schema never takes this index as fresh.
    let mut print = format!("schema={}\n{}", schema(), head(root).unwrap_or_default());
    if let Ok(m) = std::fs::metadata(git_dir(root).join("index")) {
        let nanos = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos());
        let _ = write!(print, "\nindex={}:{}", nanos.unwrap_or_default(), m.len());
    }
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
    let designs: Vec<_> = kinds
        .iter()
        .filter(|p| p.ends_with("designs"))
        .flat_map(|d| [d.join("containers"), d.join("components")])
        .collect();
    dirs.extend(kinds);
    dirs.extend(designs);
    for d in dirs {
        if let Ok(t) = std::fs::metadata(&d).and_then(|m| m.modified()) {
            let nanos = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
            let _ = write!(print, "\n{}={nanos}", d.display());
        }
    }
    print
}

/// The git directory: `.git`, or the directory a linked worktree's `.git` file names.
fn git_dir(root: &Path) -> std::path::PathBuf {
    let git = root.join(".git");
    match std::fs::read_to_string(&git) {
        Ok(text) => text.strip_prefix("gitdir:").map_or(git.clone(), |d| root.join(d.trim())),
        Err(_) => git,
    }
}

/// The commit `HEAD` names, read from `.git` without a subprocess. `None` outside a git repository.
fn head(root: &Path) -> Option<String> {
    let git = git_dir(root);
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
    Refusal::failed(subject.display().to_string(), format!("{e}; delete .bp/index.db and rerun"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, id: &str, text: &str) {
        let path = crate::Id::parse(id).unwrap().path(root);
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

    fn all(kind: &str) -> Filter<'static> {
        Filter { kind: kind.parse().unwrap(), status: None, project: None, under: None, limit: 50 }
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
        let (rows, _) = index.list(&Filter { under: Some("projects/bp/issues/0001.a"), ..all("claim") }).unwrap();
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
        assert_eq!(hits[0].snippet, "«Walk» «budget»", "the best column is the title");
        let hyphen = index.search(&["walk-budget".into()], None, Some("bp"), 10).unwrap();
        assert_eq!(hyphen.len(), 2, "punctuation splits words");
        assert_eq!(index.search(&[" ".into()], None, None, 10).err().map(|e| e.exit), Some(4));
        assert_eq!(index.search(&["walk nothingmatches".into()], None, None, 10).unwrap().len(), 3);
        assert!(index.search(&["\"(AND".into()], None, None, 10).unwrap().is_empty());
    }

    #[test]
    fn an_index_from_another_schema_is_rebuilt() {
        let (_d, v) = vault();
        drop(Index::open(&v).unwrap());
        let db = Connection::open(v.root.join(".bp/index.db")).unwrap();
        db.execute_batch("update meta set value = '2' where key = 'schema'; update meta set value = 'no schema line' where key = 'fingerprint'; drop table aliases;").unwrap();
        drop(db);
        let index = Index::open(&v).unwrap();
        assert!(index.unresolved().is_ok(), "the aliases table is back");
        assert_eq!(index.list(&all("issue")).unwrap().1, 3);
    }

    #[test]
    fn refreshes_when_a_file_comes_goes_or_the_head_moves() {
        let (_d, v) = vault();
        let count = |v: &Vault| Index::open(v).unwrap().list(&all("issue")).unwrap().1;
        assert_eq!(count(&v), 3);
        write(&v.root, "projects/bp/issues/0003.d", &issue("New", "open"));
        assert_eq!(count(&v), 4);
        std::fs::remove_file(crate::Id::parse("projects/bp/issues/0002.b").unwrap().path(&v.root)).unwrap();
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
    fn records_and_design_subdirectories_are_seen() {
        let (_d, v) = vault();
        let claims = |v: &Vault| Index::open(v).unwrap().list(&all("claim")).unwrap().1;
        assert_eq!(claims(&v), 1);
        std::fs::remove_file(crate::Id::parse("projects/bp/issues/0001.a/claims/s1").unwrap().path(&v.root)).unwrap();
        assert_eq!(claims(&v), 1, "a record removed by hand stays until a commit or the census");
        let mut index = Index::open(&v).unwrap();
        index.reconcile(&v).unwrap();
        assert_eq!(index.list(&all("claim")).unwrap().1, 0, "the census reconciles");
        write(&v.root, "projects/bp/designs/components/walk", "---\nkind: design\ntitle: Walk\nstatus: active\n---\n");
        assert_eq!(Index::open(&v).unwrap().list(&all("design")).unwrap().1, 1);
    }

    #[test]
    fn project_matches_exactly() {
        let (_d, v) = vault();
        let index = Index::open(&v).unwrap();
        for p in ["b_", "BP", "%"] {
            assert_eq!(index.list(&Filter { project: Some(p), ..all("issue") }).unwrap().1, 0, "{p}");
        }
    }

    /// 12, check 5: a refreshed index equals one rebuilt from nothing.
    #[test]
    fn the_index_is_derived() {
        let (_d, v) = vault();
        Index::open(&v).unwrap();
        write(&v.root, "projects/bp/issues/0003.d", &issue("New", "open"));
        std::fs::remove_file(crate::Id::parse("projects/bp/issues/0002.b").unwrap().path(&v.root)).unwrap();
        let refreshed = dump(&Index::open(&v).unwrap());
        std::fs::remove_file(v.root.join(".bp/index.db")).unwrap();
        assert_eq!(refreshed, dump(&Index::open(&v).unwrap()));
    }

    fn dump(index: &Index) -> Vec<String> {
        let tables = [
            ("files", "id, hash, class"),
            ("nodes", "id, kind, title, status, project, under, header"),
            ("records", "id, kind, project, under, header"),
            ("links", "src, type, dst"),
            ("fts", "id, title, body"),
        ];
        let mut out = Vec::new();
        for (table, cols) in tables {
            let mut st = index.conn.prepare(&format!("select {cols} from {table} order by 1, 2, 3")).unwrap();
            let n = st.column_count();
            let rows = st
                .query_map([], |r| (0..n).map(|i| r.get::<_, Option<String>>(i)).collect::<rusqlite::Result<Vec<_>>>())
                .unwrap();
            out.extend(rows.map(|r| format!("{table} {:?}", r.unwrap())));
        }
        out
    }

    #[test]
    fn a_bad_header_keeps_its_refusal() {
        let (_d, v) = vault();
        write(&v.root, "projects/bp/issues/0009.bad", "---\ntitle: no kind\n---\n");
        let e = Index::open(&v).err().unwrap();
        assert_eq!((e.code, e.exit), (crate::Code::SchemaInvalid, 2));
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

//! The index: `.bp/index.db`, derived from the files and holding nothing else (12). It refreshes at open when the
//! vault's fingerprint moved (see the fingerprint module): every file is hashed, and only files whose content hash
//! changed are parsed again.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::fingerprint::{fingerprint, still};
use crate::{Broken, BrokenFile, Code, Counts, File, FindingKind, Id, Kind, Refusal, Vault};

mod insert;
mod integrity;
use insert::broken;
pub(crate) use insert::{remove, replace};
pub use integrity::Fault;

/// Bump on any change to what the parser puts in the tables; a mismatch rebuilds from nothing. A change to the
/// tables themselves rebuilds without a bump, because the schema key carries their hash: two branches that both
/// bump to the same number still differ.
const SCHEMA: i64 = 8;

/// What is wrong with a markdown file whose path is not an id.
const STRAY: &str = "the path is not an id: projects/<project>/<kind>/<slug> or shared/<kind>/<slug>";

pub(crate) fn schema() -> String {
    format!("{SCHEMA}-{}", &blake3::hash(TABLES.as_bytes()).to_hex()[..12])
}

const TABLES: &str = "
create table meta (key text primary key, value text not null);
create table files (id text primary key, hash text not null, class text not null);
create table nodes (rowid integer primary key, id text unique not null, kind text not null, title text,
  status text, project text, under text, header text not null, tokens integer not null, design_kind text, scope text);
create index nodes_kind on nodes (kind, project, status);
create table records (id text primary key, kind text not null, project text, under text, header text not null,
  body text not null);
create index records_under on records (under, kind);
create index records_check on records (kind, project, json_extract(header, '$.check_kind'));
create table links (src text not null, type text not null, dst text not null);
create table unresolved (src text not null, field text not null, text text not null);
create index unresolved_src on unresolved (src);
create table markdown (src text not null, target text not null);
create index markdown_src on markdown (src);
create table aliases (alias text not null, id text not null);
create index aliases_alias on aliases (alias);
create index aliases_id on aliases (id);
create table checks (src text not null);
create index checks_src on checks (src);
create table findings (src text not null, kind text not null, detail text not null);
create index findings_src on findings (src);
create index links_src on links (src, type, dst);
create index links_dst on links (dst, type, src);
create table graph (csr blob not null);
create virtual table fts using fts5 (id, title, body, tokenize = 'unicode61');
";

/// An open, fresh index.
pub struct Index {
    pub(crate) conn: Connection,
}

/// One read transaction, held until dropped: every statement of a multi-statement read sees the same index, even
/// while another process refreshes it.
pub struct Reading<'a> {
    _tx: rusqlite::Transaction<'a>,
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
    /// A design's kind, from its path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_kind: Option<String>,
    /// A design's scope, from its path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
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

/// One record as `Index::records` reads it.
#[derive(Debug)]
pub struct Record {
    /// The header, as `show` prints it.
    pub header: Map<String, Value>,
    /// The body: what the record's appends wrote.
    pub body: String,
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
        let path = crate::write::bp_dir(&vault.root)?.join("index.db");
        let conn = Connection::open(&path).map_err(|e| failed(&path, &e))?;
        conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(|e| failed(&path, &e))?;
        let mut index = Self { conn };
        if !index.meta("fingerprint").is_some_and(|stored| still(&vault.root, &stored)) {
            index.refresh_or_fail(vault, &fingerprint(&vault.root), &path)?;
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
        // A refusal from the walk of the files keeps its code; anything else is the index failing.
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
        let rebuilt = schema != Some(self::schema());
        if rebuilt {
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
        let mut changed = rebuilt;
        let (ids, strays) = vault.ids()?;
        let files =
            ids.into_iter().map(|id| (id.as_str().to_string(), Some(id))).chain(strays.into_iter().map(|s| (s, None)));
        for (key, id) in files {
            let bytes = std::fs::read(vault.root.join(format!("{key}.md")))?;
            let hash = blake3::hash(&bytes).to_hex().to_string();
            if known.remove(&key).as_deref() == Some(hash.as_str()) {
                continue;
            }
            changed = true;
            // A stray has no id, so nothing parses it. The index holds it under its path as a broken file, which the
            // census names and which blocks no other file (25, issue 8).
            match id.ok_or_else(|| Broken::unread(STRAY)).and_then(|id| File::parse(id, &bytes)) {
                Ok(file) => replace(&tx, &file)?,
                Err(e) => broken(&tx, &key, &hash, &e.why)?,
            }
        }
        for gone in known.keys() {
            changed = true;
            remove(&tx, gone, false)?;
        }
        if changed {
            crate::walk::rebuild_graph(&tx)?;
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
                check_blocks: self.conn.query_row("select count(*) from checks", [], count)?,
                ..Counts::default()
            };
            let mut st = self.conn.prepare("select src || '.md', detail from findings where kind = ?1 order by 1")?;
            c.broken = st
                .query_map([Code::SchemaInvalid.as_str()], |r| Ok(BrokenFile { path: r.get(0)?, detail: r.get(1)? }))?
                .collect::<rusqlite::Result<_>>()?;
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

    /// Findings per kind, every kind present, zero or not: the body findings, and `schema_invalid` for each file that
    /// does not parse.
    ///
    /// # Panics
    ///
    /// Never: the index stores only kinds of the closed set.
    pub fn findings(&self) -> Result<BTreeMap<&'static str, usize>, Refusal> {
        let kinds = FindingKind::ALL.iter().map(|k| k.as_str()).chain([Code::SchemaInvalid.as_str()]);
        let mut counts: BTreeMap<&'static str, usize> = kinds.map(|k| (k, 0)).collect();
        let run = || -> rusqlite::Result<Vec<(String, usize)>> {
            let mut st = self.conn.prepare("select kind, count(*) from findings group by kind")?;
            st.query_map([], |r| Ok((r.get(0)?, count_at(r, 1)?)))?.collect()
        };
        for (kind, n) in run().map_err(|e| failed(Path::new(".bp/index.db"), &e))? {
            let (&kind, _) = counts.get_key_value(kind.as_str()).expect("the index stores closed finding kinds");
            counts.insert(kind, n);
        }
        Ok(counts)
    }

    /// Links that resolve to no file and no alias (contract code `unresolved_link`): targets that are not path ids,
    /// and path ids that name nothing or a file that does not parse.
    pub fn unresolved(&self) -> Result<usize, Refusal> {
        let sql = "select (select count(*) from unresolved) + (select count(*) from links l where not exists \
                   (select 1 from files f where f.id = l.dst and f.class != 'broken') \
                   and not exists (select 1 from aliases a where a.alias = l.dst))";
        self.conn.query_row(sql, [], count).map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// Rows of one kind, ordered by id, at most `limit`, and the total before the limit. One read transaction, so
    /// the total and the rows come from the same refresh.
    pub fn list(&self, f: &Filter) -> Result<(Vec<Row>, usize), Refusal> {
        let run = || -> rusqlite::Result<(Vec<Row>, usize)> {
            let tx = self.conn.unchecked_transaction()?;
            let is_record = f.kind.is_record();
            let (from, cols) = if is_record {
                ("records", "id, kind, null, null, under, null, null")
            } else {
                ("nodes", "id, kind, title, status, under, design_kind, scope")
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
                    Ok(Row {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        title: r.get(2)?,
                        status: r.get(3)?,
                        under: r.get(4)?,
                        design_kind: r.get(5)?,
                        scope: r.get(6)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            Ok((rows, total))
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// Starts one read transaction; see [`Reading`].
    pub fn reading(&self) -> Result<Reading<'_>, Refusal> {
        let tx = self.conn.unchecked_transaction().map_err(|e| failed(Path::new(".bp/index.db"), &e))?;
        Ok(Reading { _tx: tx })
    }

    /// A node's body cost in tokens; `None` when no node has the id. Zero tokens is an empty body.
    pub fn tokens_of(&self, id: &str) -> Result<Option<usize>, Refusal> {
        self.conn
            .query_row("select tokens from nodes where id = ?1", [id], |r| count_at(r, 0))
            .optional()
            .map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// What holds `id` up to case: a file, else a node with it as a live alias, and whether it is the alias. Two ids
    /// that differ only by case are one id, because the default macOS file system cannot hold both (13, rule 1).
    pub fn holder(&self, id: &Id) -> Result<Option<(Id, bool)>, Refusal> {
        let row: Option<(String, bool)> = self
            .conn
            .query_row(
                "select id, alias from (select id, 0 as alias from files where id = ?1 collate nocase \
                 union all select id, 1 from aliases where alias = ?1 collate nocase) order by alias, id limit 1",
                [id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| failed(Path::new(".bp/index.db"), &e))?;
        Ok(row.map(|(id, alias)| (Id::from_trusted(id), alias)))
    }

    /// The header of one node or record as JSON, when the id is in the index.
    pub fn header(&self, id: &str) -> Result<Option<Map<String, Value>>, Refusal> {
        let text: Option<String> = self
            .conn
            .query_row(
                "select header from nodes where id = ?1 union all select header from records where id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| failed(Path::new(".bp/index.db"), &e))?;
        Ok(text.map(|t| object(&t)))
    }

    /// The header and body of every record of one kind under a node, in time order: by the kind's time field, then
    /// by the `-N` a second record with one key took, as a number (13, rule 1). A record's body holds what its
    /// appends wrote, such as a claim's `## Released` section (13).
    pub fn records(&self, under: &str, kind: Kind) -> Result<Vec<Record>, Refusal> {
        let run = || -> rusqlite::Result<Vec<(String, String, String)>> {
            let mut st =
                self.conn.prepare_cached("select id, header, body from records where under = ?1 and kind = ?2")?;
            st.query_map([under, kind.as_str()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
        };
        let rows = run().map_err(|e| failed(Path::new(".bp/index.db"), &e))?;
        // Every instant the vault writes is UTC with `Z`, so text order is time order.
        let time = |r: &Record| kind.time_field().and_then(|f| r.header.get(f)?.as_str().map(str::to_string));
        let mut rows: Vec<_> = rows
            .into_iter()
            .map(|(id, h, body)| {
                let r = Record { header: object(&h), body };
                let (stem, n) = second(&id);
                ((time(&r), stem.to_string(), n), r)
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(rows.into_iter().map(|(_, r)| r).collect())
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
/// A record id's key without its `-N`, and N: 1 for the first record with the key.
fn second(id: &str) -> (&str, u64) {
    let key = id.rsplit('/').next().unwrap_or(id);
    match key.rsplit_once('-') {
        Some((stem, n)) if !n.starts_with('0') && n.len() < 6 => n.parse().map_or((key, 1), |n| (stem, n)),
        _ => (key, 1),
    }
}

pub(crate) fn count_at(r: &rusqlite::Row, i: usize) -> rusqlite::Result<usize> {
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

/// A stored header: the index wrote it from a JSON object, so it reads back as one.
fn object(text: &str) -> Map<String, Value> {
    serde_json::from_str(text).expect("the index stores headers as JSON objects")
}

pub(crate) fn failed(subject: &Path, e: &dyn std::fmt::Display) -> Refusal {
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
    fn records_read_in_time_order_and_record_fields_are_edges() {
        let (_d, v) = vault();
        let l = "shared/learnings/l";
        write(&v.root, l, "---\nkind: learning\ntitle: L\nstatus: draft\n---\n");
        let evidence = |at: &str, note: &str| {
            format!(
                "---\nkind: evidence\nunder: {l}\nat: {at}\noutcome: helped\nissue: projects/bp/issues/gone\nnote: {note}\n---\n\n~~~check shell\nx\n~~~\n"
            )
        };
        let z = "2026-10-19T10-00-00Z";
        for n in 1..=10 {
            let id = if n == 1 { format!("{l}/evidence/{z}") } else { format!("{l}/evidence/{z}-{n}") };
            write(&v.root, &id, &evidence("2026-10-19T10:00:00Z", &n.to_string()));
        }
        write(&v.root, &format!("{l}/evidence/2026-10-19T11-00-00Z"), &evidence("2026-10-19T09:00:00Z", "0"));
        let index = Index::open(&v).unwrap();
        let notes: Vec<i64> =
            index.records(l, Kind::Evidence).unwrap().iter().map(|r| r.header["note"].as_i64().unwrap()).collect();
        assert_eq!(notes, (0..=10).collect::<Vec<_>>());
        let edges: i64 = index
            .conn
            .query_row("select count(*) from links where type = 'issue' and dst = 'projects/bp/issues/gone'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(edges, 11, "a record's `→` field is an edge");
        assert_eq!(index.counts().unwrap().check_blocks, 0, "a record's check block is prose");
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
        let (rows, total) = Index::open(&v).unwrap().list(&all("design")).unwrap();
        assert_eq!(total, 1);
        assert_eq!((rows[0].design_kind.as_deref(), rows[0].scope.as_deref()), (Some("component"), Some("component")));
        // A new project moves `projects/`, which every fingerprint names, so the fast path sees it too.
        write(&v.root, "projects/zz/issues/0001.z", &issue("New project", "open"));
        assert_eq!(Index::open(&v).unwrap().list(&all("issue")).unwrap().1, 4);
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
        let body = "---\nkind: issue\ntitle: E\nstatus: open\naliases: [old.e]\ncites: ['[[x.y]]']\n---\n\n[d](/projects/bp/d.html)\n\n## A\n\n```check shell {name=one}\ntrue\n```\n\n## A\n";
        write(&v.root, "projects/bp/issues/0004.e", body);
        std::fs::remove_file(crate::Id::parse("projects/bp/issues/0002.b").unwrap().path(&v.root)).unwrap();
        let refreshed = dump(&Index::open(&v).unwrap());
        std::fs::remove_file(v.root.join(".bp/index.db")).unwrap();
        assert_eq!(refreshed, dump(&Index::open(&v).unwrap()));
    }

    /// 25, issue 9: a write updates the walk's graph in place, or not at all, and the index still equals a rebuild.
    /// Each step names the way the update must go, so a change that rebuilds on every write fails; the stored graph
    /// must equal one rebuilt over the same rows, byte for byte.
    #[test]
    fn a_write_keeps_the_graph_equal_to_a_rebuild() {
        use crate::walk::Updated::{Grown, Kept, Rebuilt};
        let (_d, v) = vault();
        let mut index = Index::open(&v).unwrap();
        let learning = |aliases: &str| format!("---\nkind: learning\ntitle: L\nstatus: draft\n{aliases}---\n");
        let (issue_a, issue_b, issue_d, l) = (
            "projects/bp/issues/0001.a",
            "projects/bp/issues/0002.b",
            "projects/bp/issues/0003.d",
            "shared/learnings/l",
        );
        let to_m = |text: String| text.replace("[[shared/learnings/l]]", "[[shared/learnings/m]]");
        let steps = [
            // A label: no link moves, so the graph stays.
            (vec![(issue_a, issue("Walk budget", "resolved"))], None, Kept),
            // An inbox entry with no link: one new row, which the walk does not enter.
            (
                vec![("shared/inbox/2026-10-20-note", "---\nkind: inbox\ntitle: Note\nstatus: raw\n---\n".into())],
                None,
                Grown,
            ),
            // A walkable node that no link names: one new row the walk enters.
            (vec![("shared/learnings/k", learning(""))], None, Grown),
            // A node the issues' bodies already name: new arcs in, so a rebuild.
            (vec![(l, learning(""))], None, Rebuilt),
            // A body link that goes.
            (vec![(issue_b, issue("Other", "resolved").replace("[[shared/learnings/l]]", ""))], None, Rebuilt),
            // A new node with links out.
            (vec![(issue_d, issue("New", "open"))], None, Rebuilt),
            // A rename that rewrites every link to the node: it keeps its row, so no arc moves.
            (
                vec![
                    ("shared/learnings/m", learning("aliases: [\"l\"]\n")),
                    (issue_a, to_m(issue("Walk budget", "resolved"))),
                    (issue_d, to_m(issue("New", "open"))),
                    ("projects/ex/issues/0001.c", to_m(issue("Elsewhere", "open"))),
                ],
                Some(("shared/learnings/m", l)),
                Kept,
            ),
        ];
        for (files, moved, way) in steps {
            let moved: BTreeMap<crate::Id, crate::Id> =
                moved.iter().map(|(n, o)| (crate::Id::parse(n).unwrap(), crate::Id::parse(o).unwrap())).collect();
            for old in moved.values() {
                std::fs::remove_file(old.path(&v.root)).unwrap();
            }
            let files: Vec<File> = files
                .iter()
                .map(|(id, text)| {
                    write(&v.root, id, text);
                    File::parse(crate::Id::parse(id).unwrap(), text.as_bytes()).unwrap()
                })
                .collect();
            let step = files[0].id.to_string();
            assert_eq!(index.apply(&v.root, &files, &moved).unwrap(), way, "{step}");
            let csr = || -> Vec<u8> { index.conn.query_row("select csr from graph", [], |r| r.get(0)).unwrap() };
            let stored = csr();
            crate::walk::rebuild_graph(&index.conn).unwrap();
            assert_eq!(stored, csr(), "{step}: the graph equals one rebuilt over the same rows");
            let written = dump(&index);
            drop(index);
            std::fs::remove_file(v.root.join(".bp/index.db")).unwrap();
            index = Index::open(&v).unwrap();
            assert_eq!(written, dump(&index), "{step}: the index equals a rebuild from nothing");
        }
    }

    fn dump(index: &Index) -> Vec<String> {
        let tables = [
            ("files", "id, hash, class"),
            ("nodes", "id, kind, title, status, project, under, header, tokens, design_kind, scope"),
            ("records", "id, kind, project, under, header, body"),
            ("links", "src, type, dst"),
            ("fts", "id, title, body"),
            ("unresolved", "src, field, text"),
            ("markdown", "src, target"),
            ("aliases", "alias, id"),
            ("checks", "src"),
            ("findings", "src, kind, detail"),
        ];
        let mut out = Vec::new();
        for (table, cols) in tables {
            let mut st = index.conn.prepare(&format!("select {cols} from {table} order by {cols}")).unwrap();
            let n = st.column_count();
            let rows = st
                .query_map([], |r| {
                    (0..n).map(|i| r.get::<_, rusqlite::types::Value>(i)).collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap();
            out.extend(rows.map(|r| format!("{table} {:?}", r.unwrap())));
        }
        out.extend(crate::walk::graph_rows(&index.conn));
        out
    }

    /// 25, issue 8: a file that does not parse is one finding and no node, a link to it resolves to nothing, and a
    /// refresh equals a rebuild both when the file breaks and when a human repairs it.
    #[test]
    fn a_file_that_does_not_parse_is_a_finding() {
        let (_d, v) = vault();
        let b = "projects/bp/issues/0002.b";
        let cites_b = format!("---\nkind: issue\ntitle: C\nstatus: open\ncites: [\"{b}\"]\n---\n");
        write(&v.root, "projects/bp/issues/0003.c", &cites_b);
        let whole = Index::open(&v).unwrap().unresolved().unwrap();
        // An in-place edit: the census's reconcile reads it.
        write(&v.root, b, "---\nkind: issue\ntitle: B\ntitle: again\nstatus: open\n---\n");
        // A markdown file whose path is not an id is held the same way, under its path.
        let stray = v.root.join("shared/notes.md");
        std::fs::write(&stray, "# notes\n").unwrap();
        let mut index = Index::open(&v).unwrap();
        index.reconcile(&v).unwrap();
        assert_eq!(index.findings().unwrap()["schema_invalid"], 2);
        let c = index.counts().unwrap();
        let broken: Vec<(&str, &str)> = c.broken.iter().map(|f| (f.path.as_str(), f.detail.as_str())).collect();
        assert_eq!(
            broken,
            [(&*format!("{b}.md"), "the key `title` appears twice at line 4 of the file"), ("shared/notes.md", STRAY)]
        );
        assert_eq!((c.files, c.nodes["issue"]), (7, 3), "the files are counted, their nodes are not");
        assert!(index.header(b).unwrap().is_none());
        assert!(index.list(&all("issue")).unwrap().0.iter().all(|r| r.id != b));
        assert!(index.search(&["Other".into()], None, None, 10).unwrap().is_empty());
        // B's own three broken links go with its node; C's cite of B is now a fourth kind of nothing.
        assert_eq!(index.unresolved().unwrap(), whole - 3 + 1);
        let refreshed = dump(&index);
        std::fs::remove_file(v.root.join(".bp/index.db")).unwrap();
        assert_eq!(refreshed, dump(&Index::open(&v).unwrap()));
        write(&v.root, b, &issue("Other", "resolved"));
        std::fs::remove_file(&stray).unwrap();
        let mut index = Index::open(&v).unwrap();
        index.reconcile(&v).unwrap();
        assert_eq!((index.findings().unwrap()["schema_invalid"], index.unresolved().unwrap()), (0, whole));
        assert!(index.header(b).unwrap().is_some());
        let refreshed = dump(&index);
        std::fs::remove_file(v.root.join(".bp/index.db")).unwrap();
        assert_eq!(refreshed, dump(&Index::open(&v).unwrap()));
    }

    #[test]
    fn reads_the_head_through_a_ref_and_packed_refs() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/master\n").unwrap();
        std::fs::write(git.join("packed-refs"), "# pack-refs\ncafe refs/heads/master\n").unwrap();
        assert_eq!(crate::git::Repo::find(dir.path()).unwrap().head().as_deref(), Some("cafe"));
        std::fs::write(git.join("refs/heads/master"), "beef\n").unwrap();
        assert_eq!(crate::git::Repo::find(dir.path()).unwrap().head().as_deref(), Some("beef"));
    }
}

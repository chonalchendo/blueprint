//! Writing one file into the index and taking it out again: the two steps of a refresh.

use rusqlite::params;
use serde_json::Value;

use crate::{Code, File};

/// Puts a changed file's rows in place of its old ones. A node keeps its rowid across an edit. A node that is new, or
/// back after a delete or a break, takes a rowid a rebuild would not give it: the rows equal a rebuild's, and a walk's
/// ranks keep their order but may differ in the last float bits (12, check 5).
pub(crate) fn replace(tx: &rusqlite::Transaction, file: &File) -> rusqlite::Result<()> {
    remove(tx, file.id.as_str(), file.header.as_ref().is_some_and(|h| !h.kind.is_record()))?;
    insert(tx, file)
}

fn insert(tx: &rusqlite::Transaction, file: &File) -> rusqlite::Result<()> {
    let id = file.id.as_str();
    let Some(header) = &file.header else {
        tx.execute("insert into files values (?1, ?2, 'asset')", params![id, file.hash])?;
        return Ok(());
    };
    let json = Value::Object(file.header_json().expect("a file with a header")).to_string();
    let under = header.under.as_ref().map(crate::Id::as_str);
    let kind = header.kind.as_str();
    if header.kind.is_record() {
        tx.execute("insert into files values (?1, ?2, 'record')", params![id, file.hash])?;
        tx.execute(
            "insert into records values (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, kind, file.id.project(), under, json, file.body.text],
        )?;
    } else {
        tx.execute("insert into files values (?1, ?2, 'node')", params![id, file.hash])?;
        let level = crate::DesignLevel::of(&file.id);
        tx.execute(
            "insert into nodes (id, kind, title, status, project, under, header, tokens, design_kind, scope)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) on conflict (id) do update set kind = excluded.kind,
             title = excluded.title, status = excluded.status, project = excluded.project, under = excluded.under,
             header = excluded.header, tokens = excluded.tokens, design_kind = excluded.design_kind,
             scope = excluded.scope",
            params![
                id,
                kind,
                header.title,
                header.status,
                file.id.project(),
                under,
                json,
                body_tokens(&file.body.text),
                level.map(|l| l.design_kind),
                level.map(|l| l.scope)
            ],
        )?;
        tx.execute(
            "insert into fts (rowid, id, title, body) values ((select rowid from nodes where id = ?1), ?1, ?2, ?3)",
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
    let dir = id.rsplit_once('/').expect("an id has three segments").0;
    for a in &header.aliases {
        // An alias is a past path id (13, rule 1); `rename` keeps only the old slug, in the node's own directory.
        if a.contains('/') {
            st.execute(params![a, id])?;
        } else {
            st.execute(params![format!("{dir}/{a}"), id])?;
        }
    }
    let mut st = tx.prepare_cached("insert into unresolved values (?1, ?2, ?3)")?;
    for u in &header.unresolved {
        st.execute(params![id, u.field, u.text])?;
    }
    for text in &file.body.unresolved {
        st.execute(params![id, "body", text])?;
    }
    let mut st = tx.prepare_cached("insert into markdown values (?1, ?2)")?;
    for target in &file.body.markdown {
        st.execute([id, target])?;
    }
    let mut st = tx.prepare_cached("insert into checks values (?1)")?;
    for _ in &file.body.checks {
        st.execute([id])?;
    }
    let mut st = tx.prepare_cached("insert into findings values (?1, ?2, ?3)")?;
    for f in &file.body.findings {
        st.execute([id, f.kind.as_str(), &f.detail])?;
    }
    Ok(())
}

/// Puts a file that does not parse in place of its old rows: a `files` row and one `schema_invalid` finding with
/// the parse error, and no node. One hand-made mistake then blocks no read and no write but its own (25, issue 8);
/// a link to the file resolves to nothing, as to a missing node.
pub(crate) fn broken(tx: &rusqlite::Transaction, id: &str, hash: &str, why: &str) -> rusqlite::Result<()> {
    remove(tx, id, false)?;
    tx.execute("insert into files values (?1, ?2, 'broken')", params![id, hash])?;
    tx.execute("insert into findings values (?1, ?2, ?3)", params![id, Code::SchemaInvalid.as_str(), why])?;
    Ok(())
}

/// Deletes a file's rows; the node row stays when `keep_node`, for `replace` to update.
pub(crate) fn remove(tx: &rusqlite::Transaction, id: &str, keep_node: bool) -> rusqlite::Result<()> {
    tx.execute("delete from fts where rowid = (select rowid from nodes where id = ?1)", [id])?;
    let tables: &[&str] = if keep_node { &["files", "records"] } else { &["files", "nodes", "records"] };
    for table in tables {
        tx.execute(&format!("delete from {table} where id = ?1"), [id])?;
    }
    tx.execute("delete from links where src = ?1", [id])?;
    for table in ["unresolved", "markdown", "checks", "findings"] {
        tx.execute(&format!("delete from {table} where src = ?1"), [id])?;
    }
    tx.execute("delete from aliases where id = ?1", [id])?;
    Ok(())
}

/// A body's cost in the walk's budget: four characters a token over the trimmed body, rounded up, so an empty
/// body costs nothing and any other at least one.
fn body_tokens(body: &str) -> i64 {
    i64::try_from(body.trim().len().div_ceil(4)).expect("a body is far below i64::MAX bytes")
}

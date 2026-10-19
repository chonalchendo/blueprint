//! The proposed tree (25, design change 1): the current tree with a write's changes over it, the findings it holds,
//! and the index update after the commit.

use std::collections::BTreeMap;
use std::path::Path;

use crate::fingerprint::fingerprint;
use crate::index::{failed, replace};
use crate::write::Held;
use crate::{Code, File, Id, Index, Refusal, Vault};

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
    pub(crate) fn apply(&mut self, root: &Path, files: &[File]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        for f in files {
            replace(&tx, f)?;
        }
        crate::walk::rebuild_graph(&tx)?;
        tx.execute("insert or replace into meta values ('fingerprint', ?1)", [fingerprint(root)])?;
        tx.commit()
    }
}

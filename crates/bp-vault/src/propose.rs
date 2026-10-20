//! The proposed tree (25, design change 1): the current tree with a write's changes over it, the findings it holds,
//! and the index update after the commit.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::OptionalExtension;

use crate::fingerprint::fingerprint;
use crate::index::{failed, remove, replace};
use crate::write::Held;
use crate::{Code, File, Id, Index, Refusal, Vault};

impl Index {
    /// Validates the proposed tree: the current tree with `changes` written over it. Each changed file parses, and
    /// holds no refusing finding that its current version does not. Returns the parsed files and every finding they
    /// hold. Links resolve against the proposed tree: a file the same write adds is there.
    pub fn propose(&self, vault: &Vault, changes: &BTreeMap<Id, String>) -> Result<(Vec<File>, Vec<Held>), Refusal> {
        self.propose_moving(vault, changes, &BTreeMap::new())
    }

    /// [`Index::propose`] for a write that moves files: `moved` maps a new id to the id it had, and the file is held
    /// to that old version, so a moved node's broken links stay findings it already held.
    pub fn propose_moving(
        &self,
        vault: &Vault,
        changes: &BTreeMap<Id, String>,
        moved: &BTreeMap<Id, Id>,
    ) -> Result<(Vec<File>, Vec<Held>), Refusal> {
        let mut files = Vec::with_capacity(changes.len());
        let mut all = Vec::new();
        for (id, text) in changes {
            let file = File::from_bytes(id.clone(), text.as_bytes())?;
            let mut before = match vault.read(moved.get(id).unwrap_or(id)) {
                Ok(old) => self.held(&old, None)?,
                Err(e) if e.code == Code::NoNode => Vec::new(),
                Err(e) => return Err(e),
            };
            for h in &mut before {
                id.as_str().clone_into(&mut h.id);
            }
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
                        "every check block has a name, and a name is a slug (15, the check grammar; 13)",
                        format!(
                            "give the block `{}` a name=<slug>: lowercase ASCII letters, digits, `-` and `.`, at \
                             most 80 characters",
                            new.text
                        ),
                    ));
                }
                if let Some(refusal) = check_rule(id, new) {
                    return Err(refusal);
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
        out.extend(file.body.empty_checks.iter().map(|s| held(Code::CheckEmpty.as_str(), "body", s)));
        let names: Vec<&String> = file.body.checks.iter().filter_map(|c| c.attrs.get("name")).collect();
        for (i, name) in names.iter().enumerate() {
            // A shared name is one finding, at its second block. Case is ignored: the run files of `a` and `A` are
            // one file on the default macOS file system.
            if names[..i].iter().filter(|n| n.eq_ignore_ascii_case(name)).count() == 1 {
                out.push(held(Code::CheckDuplicate.as_str(), "body", name));
            }
        }
        Ok(out)
    }

    /// The node whose live alias is `id`, when one is: a rename keeps the old id resolving, so no new node takes it
    /// (13, rule 1).
    pub fn aliased(&self, id: &Id) -> Result<Option<Id>, Refusal> {
        let holder: Option<String> = self
            .conn
            .query_row("select id from aliases where alias = ?1 order by id limit 1", [id.as_str()], |r| r.get(0))
            .optional()
            .map_err(|e| failed(Path::new(".bp/index.db"), &e))?;
        Ok(holder.map(Id::from_trusted))
    }

    /// Takes the committed files, drops the ids the commit removed, and stores the fingerprint, in one transaction.
    pub(crate) fn apply<'i>(
        &mut self,
        root: &Path,
        files: &[File],
        gone: impl IntoIterator<Item = &'i Id>,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        for id in gone {
            remove(&tx, id.as_str(), false)?;
        }
        for f in files {
            replace(&tx, f)?;
        }
        crate::walk::rebuild_graph(&tx)?;
        tx.execute("insert or replace into meta values ('fingerprint', ?1)", [fingerprint(root)])?;
        tx.commit()
    }
}

/// The refusal for a check finding a write adds: a check section with no block, or two blocks that share a name.
fn check_rule(id: &Id, new: &Held) -> Option<Refusal> {
    let (code, rule, fix) = match new.code.parse() {
        Ok(Code::CheckEmpty) => (
            Code::CheckEmpty,
            "a check section, and each of its subsections, holds a check block (15, the check grammar)",
            format!("add a fenced check block under `{}`, or move its prose to another section", new.text),
        ),
        Ok(Code::CheckDuplicate) => (
            Code::CheckDuplicate,
            "the check blocks of one owner have distinct names (25, design change 2)",
            format!("rename one of the blocks named `{}`", new.text),
        ),
        _ => return None,
    };
    Some(Refusal::new(code, id.as_str(), rule, fix))
}

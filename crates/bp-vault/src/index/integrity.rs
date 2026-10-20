//! The project's integrity rules (25, issue 11) as reads of the index, and the integrity runs that `blocked` reads
//! (21). Only the project's own files count: a shared node is out of every project's scope.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use rusqlite::params;
use serde_json::{Map, Value};

use super::{failed, object};
use crate::{Code, Index, Refusal, Vault};

/// One way a file of the project breaks a vault rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// The rule, as the run's output names it: `broken`, `link`, `issue`, `level` or `case`.
    pub rule: &'static str,
    /// The node's id; for a file that does not parse, or a path no file holds, the path.
    pub id: String,
    /// What is wrong, in one line.
    pub detail: String,
    /// The id is a file the index parsed, so a run record may name it as an edge without adding a broken link.
    pub node: bool,
}

impl Index {
    /// Every fault of `project`, rule by rule (25, issue 11), each rule's in id order.
    pub fn integrity(&self, vault: &Vault, project: &str) -> Result<Vec<Fault>, Refusal> {
        let prefix = format!("projects/{project}/");
        let mut out = Vec::new();
        self.broken_files(&prefix, &mut out)?;
        self.links_to_nothing(&vault.root, project, &mut out)?;
        self.issues_without_context(project, &mut out)?;
        self.designs_off_the_spine(project, &mut out)?;
        self.case_twins(&vault.root, &prefix, &mut out)?;
        out.dedup();
        Ok(out)
    }

    /// The integrity runs of `id`'s project, in no order, when any run names `id`: the headers of the run records
    /// whose check is of kind `integrity`, each with its `id`. None when no run names it, since then none blocks it;
    /// that probe is one index lookup, so a walk over many issues pays nothing for the rule (24, call budget).
    pub fn integrity_runs(&self, id: &str) -> Result<Vec<Map<String, Value>>, Refusal> {
        let run = || -> rusqlite::Result<Vec<Map<String, Value>>> {
            let named: bool = self
                .conn
                .prepare_cached("select exists (select 1 from links where dst = ?1 and type = 'names')")?
                .query_row([id], |r| r.get(0))?;
            if !named {
                return Ok(Vec::new());
            }
            let mut st = self.conn.prepare_cached(
                "select id, header from records where kind = 'run' and project = ?1 \
                 and json_extract(header, '$.check_kind') = 'integrity'",
            )?;
            st.query_map([crate::Id::from_trusted(id.to_string()).project()], |r| {
                let mut header = object(&r.get::<_, String>(1)?);
                header.insert("id".into(), Value::String(r.get(0)?));
                Ok(header)
            })?
            .collect()
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    /// Rule 1: a file that does not parse (the census's `broken`).
    fn broken_files(&self, prefix: &str, out: &mut Vec<Fault>) -> Result<(), Refusal> {
        let sql = "select src, detail from findings where kind = ?1 and substr(src, 1, length(?2)) = ?2 order by src";
        for (src, detail) in self.pairs(sql, params![Code::SchemaInvalid.as_str(), prefix])? {
            out.push(Fault { rule: "broken", id: format!("{src}.md"), detail, node: false });
        }
        Ok(())
    }

    /// Rule 2: a header link, body wikilink or root-relative markdown link from a node of the project that resolves
    /// to nothing. A markdown link names an asset more often than a node, so it resolves against the files too.
    fn links_to_nothing(&self, root: &Path, project: &str, out: &mut Vec<Fault>) -> Result<(), Refusal> {
        let mut found: Vec<(String, String)> = Vec::new();
        let sql = "select u.src, u.field || ' ' || u.text || ': not an id' from unresolved u \
                   join nodes n on n.id = u.src where n.project = ?1";
        found.extend(self.pairs(sql, [project])?);
        let sql = "select l.src, l.type || ' ' || l.dst || ': resolves to nothing' from links l \
                   join nodes n on n.id = l.src where n.project = ?1 \
                   and not exists (select 1 from files f where f.id = l.dst and f.class != 'broken') \
                   and not exists (select 1 from aliases a where a.alias = l.dst)";
        found.extend(self.pairs(sql, [project])?);
        let sql = "select m.src, m.target from markdown m join nodes n on n.id = m.src where n.project = ?1";
        for (src, target) in self.pairs(sql, [project])? {
            let resolves = match target.strip_suffix(".md") {
                Some(id) => self.known(id)?,
                None => self.known(&target)? || root.join(&target).exists(),
            };
            if !resolves {
                found.push((src, format!("markdown /{target}: resolves to nothing")));
            }
        }
        found.sort();
        found.dedup();
        out.extend(found.into_iter().map(|(id, detail)| Fault { rule: "link", id, detail, node: true }));
        Ok(())
    }

    /// Rule 3 (21, rule 3): an open or in-progress issue whose `under` is no milestone that exists, or that cites no
    /// design that exists. A walk for `complete` would refuse it.
    fn issues_without_context(&self, project: &str, out: &mut Vec<Fault>) -> Result<(), Refusal> {
        // The issue, its `under`, and whether that is a milestone and whether it cites a design.
        type Row = (String, Option<String>, bool, bool);
        let run = || -> rusqlite::Result<Vec<Row>> {
            let mut st = self.conn.prepare(
                "select i.id, i.under,
                   exists (select 1 from nodes m where m.id = i.under and m.kind = 'milestone')
                   or exists (select 1 from aliases a join nodes m on m.id = a.id
                              where a.alias = i.under and m.kind = 'milestone'),
                   exists (select 1 from links l join nodes d on d.id = l.dst
                           where l.src = i.id and l.type = 'cites' and d.kind = 'design')
                   or exists (select 1 from links l join aliases a on a.alias = l.dst join nodes d on d.id = a.id
                              where l.src = i.id and l.type = 'cites' and d.kind = 'design')
                 from nodes i where i.kind = 'issue' and i.project = ?1 and i.status in ('open', 'in-progress')
                 order by i.id",
            )?;
            st.query_map([project], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect()
        };
        for (id, under, milestone, design) in run().map_err(|e| failed(Path::new(".bp/index.db"), &e))? {
            if !milestone {
                let detail = under.map_or_else(
                    || "no `under` milestone".to_string(),
                    |u| format!("`under` {u} is no milestone that exists"),
                );
                out.push(Fault { rule: "issue", id: id.clone(), detail, node: true });
            }
            if !design {
                out.push(Fault { rule: "issue", id, detail: "cites no design that exists".into(), node: true });
            }
        }
        Ok(())
    }

    /// Rule 4 (13, Design: the spine): a container design sits under a system design; a component under a
    /// container or a system design.
    fn designs_off_the_spine(&self, project: &str, out: &mut Vec<Fault>) -> Result<(), Refusal> {
        // The design, its level, its `under` and that parent's level.
        type Row = (String, String, Option<String>, Option<String>);
        let run = || -> rusqlite::Result<Vec<Row>> {
            let mut st = self.conn.prepare(
                "select d.id, d.design_kind, d.under,
                   coalesce((select p.design_kind from nodes p where p.id = d.under and p.kind = 'design'),
                            (select p.design_kind from aliases a join nodes p on p.id = a.id
                             where a.alias = d.under and p.kind = 'design'))
                 from nodes d where d.kind = 'design' and d.project = ?1
                 and d.design_kind in ('container', 'component') order by d.id",
            )?;
            st.query_map([project], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect()
        };
        for (id, level, under, parent) in run().map_err(|e| failed(Path::new(".bp/index.db"), &e))? {
            let (fits, want) = match level.as_str() {
                "container" => (parent.as_deref() == Some("system"), "a system design"),
                _ => (matches!(parent.as_deref(), Some("container" | "system")), "a container or a system design"),
            };
            if !fits {
                let parent = match (under, parent) {
                    (None, _) => "no `under`".to_string(),
                    (Some(u), Some(p)) => format!("`under` {u} is a {p} design"),
                    (Some(u), None) => format!("`under` {u} is no design"),
                };
                let detail = format!("{parent}; a {level} sits under {want}");
                out.push(Fault { rule: "level", id, detail, node: true });
            }
        }
        Ok(())
    }

    /// Rule 5 (13, rule 1): two paths of the project that differ only by case. The default macOS file system holds
    /// only one of them, so git's tree is read beside the index's files.
    fn case_twins(&self, root: &Path, prefix: &str, out: &mut Vec<Fault>) -> Result<(), Refusal> {
        let sql = "select id || '.md', class from files where substr(id, 1, length(?1)) = ?1";
        let files: BTreeMap<String, String> = self.pairs(sql, [prefix])?.into_iter().collect();
        let mut paths: BTreeSet<String> = files.keys().cloned().collect();
        paths.extend(tracked(root, prefix));
        out.extend(twins(&paths).into_iter().map(|(path, others)| {
            let node = files.get(&path).is_some_and(|class| class != "broken");
            let id = path.strip_suffix(".md").map_or_else(|| path.clone(), str::to_string);
            Fault { rule: "case", id, detail: format!("differs only by case from {}", others.join(", ")), node }
        }));
        Ok(())
    }

    /// A file the index parsed, or a live alias, has this id: a link to it resolves.
    fn known(&self, id: &str) -> Result<bool, Refusal> {
        self.conn
            .prepare_cached(
                "select exists (select 1 from files where id = ?1 and class != 'broken') \
                 or exists (select 1 from aliases where alias = ?1)",
            )
            .and_then(|mut st| st.query_row([id], |r| r.get(0)))
            .map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }

    fn pairs(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<(String, String)>, Refusal> {
        let run = || -> rusqlite::Result<Vec<(String, String)>> {
            let mut st = self.conn.prepare(sql)?;
            st.query_map(p, |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }
}

/// The paths git tracks under `prefix`; none when the vault is not a git repository.
fn tracked(root: &Path, prefix: &str) -> Vec<String> {
    let out = Command::new("git").arg("-C").arg(root).args(["ls-files", "-z", "--", prefix]).output();
    let Some(out) = out.ok().filter(|o| o.status.success()) else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout).split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect()
}

/// Each path that shares its lowercase form with another, with those others.
fn twins(paths: &BTreeSet<String>) -> Vec<(String, Vec<String>)> {
    let mut groups: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for p in paths {
        groups.entry(p.to_lowercase()).or_default().push(p);
    }
    let mut out = Vec::new();
    for group in groups.values().filter(|g| g.len() > 1) {
        for p in group {
            let others = group.iter().filter(|o| *o != p).map(|o| (*o).clone()).collect();
            out.push(((*p).clone(), others));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Index;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn node(kind: &str, extra: &str) -> String {
        format!("---\nkind: {kind}\ntitle: T\nstatus: open\n{extra}---\n\nBody.\n")
    }

    /// A clean project `bp` and a project `ex` that breaks every rule, so a fault of `bp` would be out of scope.
    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        for p in ["bp", "ex"] {
            write(r, &format!("projects/{p}/project.md"), &node("project", ""));
            write(r, &format!("projects/{p}/designs/system.md"), &node("design", ""));
            write(r, &format!("projects/{p}/milestones/m.md"), &node("milestone", ""));
            let cites = format!("under: projects/{p}/milestones/m\ncites: [projects/{p}/designs/system]\n");
            write(r, &format!("projects/{p}/issues/ok.md"), &node("issue", &cites));
            let under = format!("under: projects/{p}/designs/system\n");
            write(r, &format!("projects/{p}/designs/containers/c.md"), &node("design", &under));
        }
        write(r, "projects/bp/designs/system/diagrams/d.html", "<p/>\n");
        write(r, "shared/threads/t.md", &node("thread", "cites: [shared/nowhere/x]\n"));
        let v = Vault { root: r.to_path_buf() };
        (dir, v)
    }

    fn faults(v: &Vault, project: &str) -> Vec<(&'static str, String, bool)> {
        let index = Index::open(v).unwrap();
        index.integrity(v, project).unwrap().into_iter().map(|f| (f.rule, f.id, f.node)).collect()
    }

    #[test]
    fn a_clean_project_has_no_fault_and_shared_nodes_are_out_of_scope() {
        let (_d, v) = vault();
        assert_eq!(faults(&v, "bp"), []);
    }

    #[test]
    fn rule_1_a_file_that_does_not_parse() {
        let (_d, v) = vault();
        write(&v.root, "projects/ex/issues/bad.md", "---\nkind: [\n---\n");
        write(&v.root, "projects/ex/Notes.md", "# stray\n");
        let want =
            [("broken", "projects/ex/Notes.md".into(), false), ("broken", "projects/ex/issues/bad.md".into(), false)];
        assert_eq!(faults(&v, "ex"), want);
    }

    #[test]
    fn rule_2_a_link_that_resolves_to_nothing() {
        let (_d, v) = vault();
        let links = "under: projects/ex/milestones/m\ncites: [projects/ex/designs/system, projects/ex/designs/gone, '[[old]]']\n";
        let body = "[[projects/ex/issues/nope]] [[projects/ex/issues/ok]] [a](/projects/ex/designs/system/diagrams/d.html) \
                    [b](/projects/bp/designs/system/diagrams/d.html#x) [c](/projects/ex/issues/ok.md) \
                    [d](/projects/ex/issues/missing.md) `[e](/projects/ex/in/code)`\n";
        let text = node("issue", links).replace("Body.\n", body);
        write(&v.root, "projects/ex/issues/links.md", &text);
        let index = Index::open(&v).unwrap();
        let details: Vec<String> = index.integrity(&v, "ex").unwrap().into_iter().map(|f| f.detail).collect();
        assert_eq!(
            details,
            [
                "body projects/ex/issues/nope: resolves to nothing",
                "cites [[old]]: not an id",
                "cites projects/ex/designs/gone: resolves to nothing",
                "markdown /projects/ex/designs/system/diagrams/d.html: resolves to nothing",
                "markdown /projects/ex/issues/missing.md: resolves to nothing",
            ]
        );
    }

    #[test]
    fn rule_3_a_live_issue_without_its_milestone_or_a_cited_design() {
        let (_d, v) = vault();
        write(&v.root, "projects/ex/issues/bare.md", &node("issue", ""));
        let wrong = "under: projects/ex/designs/system\ncites: [projects/ex/milestones/m]\n";
        write(&v.root, "projects/ex/issues/wrong.md", &node("issue", wrong));
        let done = "under: projects/ex/designs/system\n";
        write(&v.root, "projects/ex/issues/done.md", &node("issue", done).replace("status: open", "status: resolved"));
        let aliased = "under: projects/ex/milestones/old-m\ncites: [projects/ex/designs/system]\n";
        write(&v.root, "projects/ex/issues/aliased.md", &node("issue", aliased));
        write(&v.root, "projects/ex/milestones/m.md", &node("milestone", "aliases: [old-m]\n"));
        let index = Index::open(&v).unwrap();
        let found: Vec<(String, String)> = index
            .integrity(&v, "ex")
            .unwrap()
            .into_iter()
            .filter(|f| f.rule == "issue")
            .map(|f| (f.id, f.detail))
            .collect();
        let want = [
            ("projects/ex/issues/bare", "no `under` milestone"),
            ("projects/ex/issues/bare", "cites no design that exists"),
            ("projects/ex/issues/wrong", "`under` projects/ex/designs/system is no milestone that exists"),
            ("projects/ex/issues/wrong", "cites no design that exists"),
        ];
        assert_eq!(found, want.map(|(a, b)| (a.to_string(), b.to_string())), "a resolved issue and an alias pass");
    }

    #[test]
    fn rule_4_a_design_off_the_spine() {
        let (_d, v) = vault();
        let under = |u: &str| node("design", &format!("under: {u}\n"));
        write(&v.root, "projects/ex/designs/containers/bad.md", &under("projects/ex/project"));
        write(&v.root, "projects/ex/designs/containers/orphan.md", &node("design", ""));
        write(&v.root, "projects/ex/designs/components/ok-c.md", &under("projects/ex/designs/containers/c"));
        write(&v.root, "projects/ex/designs/components/ok-s.md", &under("projects/ex/designs/system"));
        write(&v.root, "projects/ex/designs/components/bad.md", &under("projects/ex/designs/components/ok-c"));
        write(&v.root, "projects/ex/designs/data-model.md", &under("projects/ex/project"));
        let index = Index::open(&v).unwrap();
        let found: Vec<String> =
            index.integrity(&v, "ex").unwrap().into_iter().filter(|f| f.rule == "level").map(|f| f.id).collect();
        assert_eq!(
            found,
            [
                "projects/ex/designs/components/bad",
                "projects/ex/designs/containers/bad",
                "projects/ex/designs/containers/orphan"
            ]
        );
    }

    #[test]
    fn rule_5_paths_that_differ_only_by_case() {
        let paths: BTreeSet<String> = [
            "projects/ex/issues/a.md",
            "projects/ex/issues/A.md",
            "projects/ex/issues/b.md",
            "projects/ex/Issues/b.md",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let found = twins(&paths);
        let ids: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            ids,
            [
                "projects/ex/issues/A.md",
                "projects/ex/issues/a.md",
                "projects/ex/Issues/b.md",
                "projects/ex/issues/b.md"
            ]
        );
        assert_eq!(found[0].1, ["projects/ex/issues/a.md"]);
        // git holds both twins on any file system; the default macOS one holds only one of them on disk.
        let (_d, v) = vault();
        let git = |args: &[&str]| {
            let out = Command::new("git").arg("-C").arg(&v.root).args(args).output().unwrap();
            assert!(out.status.success(), "git {args:?}");
            String::from_utf8(out.stdout).unwrap()
        };
        git(&["init", "-q"]);
        let blob = git(&["hash-object", "-w", "projects/ex/issues/ok.md"]);
        git(&["add", "projects/ex/issues/ok.md"]);
        git(&["update-index", "--add", "--cacheinfo", &format!("100644,{},projects/ex/issues/OK.md", blob.trim())]);
        let want =
            [("case", "projects/ex/issues/OK".to_string(), false), ("case", "projects/ex/issues/ok".to_string(), true)];
        assert_eq!(faults(&v, "ex"), want, "only the parsed twin can be a run's edge");
    }
}

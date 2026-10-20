//! The project's integrity rules (25, issue 11) as reads of the index, and the integrity runs that `blocked` reads
//! (21). Only the project's own files count: a shared node is out of every project's scope.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value};

use super::{failed, object};
use crate::{Code, ISSUE_CONTEXT, Index, Refusal, Vault};

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
    /// Every fault of `project`, rule by rule (25, issue 11), each rule's in id order; then the faults with no node,
    /// last, so a run's output tail keeps them: no record names them. `Err` when git cannot list the tracked files.
    pub fn integrity(&self, vault: &Vault, project: &str) -> Result<Vec<Fault>, Refusal> {
        let prefix = format!("projects/{project}/");
        let tracked = tracked(&vault.root)?;
        let mut out = Vec::new();
        self.broken_files(&prefix, &mut out)?;
        self.links_to_nothing(&tracked, project, &mut out)?;
        self.issues_without_context(project, &mut out)?;
        self.designs_off_the_spine(project, &mut out)?;
        self.case_twins(&tracked, &prefix, &mut out)?;
        out.dedup();
        out.sort_by_key(|f| !f.node);
        Ok(out)
    }

    /// Every integrity run of `project`: its header with `id` and `under`, in no order. One lookup through the
    /// index on kind, project and check kind, so a call reads it once, not once per issue (24, call budget).
    pub fn integrity_runs(&self, project: &str) -> Result<Vec<Map<String, Value>>, Refusal> {
        let run = || -> rusqlite::Result<Vec<Map<String, Value>>> {
            let mut st = self.conn.prepare_cached(
                "select id, under, header from records where kind = 'run' and project = ?1 \
                 and json_extract(header, '$.check_kind') = 'integrity'",
            )?;
            st.query_map([project], |r| {
                let mut header = object(&r.get::<_, String>(2)?);
                header.insert("id".into(), Value::String(r.get(0)?));
                header.insert("under".into(), r.get::<_, Option<String>>(1)?.map_or(Value::Null, Value::String));
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
    /// to nothing. A markdown link names an asset more often than a node, so it resolves against the tracked files
    /// too: exactly, so the verdict is the same on every file system and at the run's commit.
    fn links_to_nothing(&self, tracked: &BTreeSet<String>, project: &str, out: &mut Vec<Fault>) -> Result<(), Refusal> {
        let mut found: Vec<(String, String)> = Vec::new();
        let sql = "select u.src, u.field || ' ' || u.text || ': not an id' from unresolved u \
                   join nodes n on n.id = u.src where n.project = ?1";
        found.extend(self.pairs(sql, [project])?);
        let sql = "select l.src, l.type || ' ' || l.dst from links l join nodes n on n.id = l.src where n.project = ?1 \
                   and not exists (select 1 from files f where f.id = l.dst and f.class != 'broken') \
                   and not exists (select 1 from aliases a where a.alias = l.dst)";
        for (src, link) in self.pairs(sql, [project])? {
            let dst = link.split_once(' ').map_or("", |(_, d)| d);
            found.push((src, format!("{link}: {}", self.miss(dst, tracked)?)));
        }
        let sql = "select m.src, m.target from markdown m join nodes n on n.id = m.src where n.project = ?1";
        for (src, target) in self.pairs(sql, [project])? {
            let why = if target.split('/').any(|s| s == "..") {
                Some("a `..` segment leaves the path the link names".to_string())
            } else {
                let resolves = match target.strip_suffix(".md") {
                    Some(id) => self.known(id)?,
                    None => self.known(&target)? || tracked.contains(&target),
                };
                if resolves { None } else { Some(self.miss(target.strip_suffix(".md").unwrap_or(&target), tracked)?) }
            };
            if let Some(why) = why {
                found.push((src, format!("markdown /{target}: {why}")));
            }
        }
        found.sort();
        found.dedup();
        out.extend(found.into_iter().map(|(id, detail)| Fault { rule: "link", id, detail, node: true }));
        Ok(())
    }

    /// Rule 3 (21, rule 3): an open or in-progress issue that a walk for `complete` refuses, by the walk's own
    /// predicate: its `under` milestone or a cited design is absent or empty, or it cites none.
    fn issues_without_context(&self, project: &str, out: &mut Vec<Fault>) -> Result<(), Refusal> {
        let run = || -> rusqlite::Result<Vec<String>> {
            let mut st = self.conn.prepare(
                "select id from nodes where kind = 'issue' and project = ?1 and status in ('open', 'in-progress') \
                 order by id",
            )?;
            st.query_map([project], |r| r.get(0))?.collect()
        };
        for id in run().map_err(|e| failed(Path::new(".bp/index.db"), &e))? {
            for req in ISSUE_CONTEXT {
                if let Some(gap) = self.context_gap(&id, req)? {
                    out.push(Fault { rule: "issue", id: id.clone(), detail: gap.detail(req), node: true });
                }
            }
        }
        Ok(())
    }

    /// Rule 4 (13, Design: the spine): a product sits under the project; a system under a product; a data model, a
    /// glossary and a container under a system; a component under a container or a system; each parent in the
    /// design's own project.
    fn designs_off_the_spine(&self, project: &str, out: &mut Vec<Fault>) -> Result<(), Refusal> {
        // The design, its level, its `under`, and that parent's level (a design's, else its kind) and project.
        type Row = (String, String, Option<String>, Option<String>, Option<String>);
        let run = || -> rusqlite::Result<Vec<Row>> {
            let mut st = self.conn.prepare(
                "select d.id, d.design_kind, d.under, coalesce(p.design_kind, p.kind), p.project from nodes d
                 left join nodes p on p.id = coalesce((select n.id from nodes n where n.id = d.under),
                                                      (select a.id from aliases a where a.alias = d.under))
                 where d.kind = 'design' and d.project = ?1 order by d.id",
            )?;
            st.query_map([project], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect()
        };
        for (id, level, under, parent, home) in run().map_err(|e| failed(Path::new(".bp/index.db"), &e))? {
            let (want, said): (&[&str], &str) = match level.as_str() {
                "product" => (&["project"], "the project"),
                "system" => (&["product"], "a product design"),
                "component" => (&["container", "system"], "a container or a system design"),
                _ => (&["system"], "a system design"),
            };
            let wrong = match (under, parent, home) {
                (None, ..) => "no `under`".to_string(),
                (Some(u), None, _) => format!("`under` {u} is no node"),
                (Some(u), Some(_), h) if h.as_deref() != Some(project) => {
                    format!("`under` {u} is outside projects/{project}")
                }
                (Some(_), Some(p), _) if want.contains(&p.as_str()) => continue,
                (Some(u), Some(p), _) => format!("`under` {u} is a {p}"),
            };
            let detail = format!("{wrong}; a {level} design sits under {said} of its project");
            out.push(Fault { rule: "level", id, detail, node: true });
        }
        Ok(())
    }

    /// Rule 5 (13, rule 1): two paths of the project that differ only by case. The default macOS file system holds
    /// only one of them, so git's tree is read beside the index's files; the project's prefix matches in any case,
    /// so a twin of the project's own directory counts.
    fn case_twins(&self, tracked: &BTreeSet<String>, prefix: &str, out: &mut Vec<Fault>) -> Result<(), Refusal> {
        let sql = "select id || '.md', class from files where lower(substr(id, 1, length(?1))) = lower(?1)";
        let files: BTreeMap<String, String> = self.pairs(sql, [prefix])?.into_iter().collect();
        let mut paths: BTreeSet<String> = files.keys().cloned().collect();
        let ours = |p: &&String| p.get(..prefix.len()).is_some_and(|h| h.eq_ignore_ascii_case(prefix));
        paths.extend(tracked.iter().filter(ours).cloned());
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

    /// Why a link to `id` resolves to nothing: a file, an alias or a tracked path that differs only by case names
    /// the fix, as `show` would resolve it (13, rule 1); else nothing holds it.
    fn miss(&self, id: &str, tracked: &BTreeSet<String>) -> Result<String, Refusal> {
        let held: Option<String> = self
            .conn
            .prepare_cached(
                "select id from files where id = ?1 collate nocase and class != 'broken' \
                 union all select alias from aliases where alias = ?1 collate nocase limit 1",
            )
            .and_then(|mut st| st.query_row([id], |r| r.get(0)).optional())
            .map_err(|e| failed(Path::new(".bp/index.db"), &e))?;
        let held = held.or_else(|| tracked.iter().find(|p| p.eq_ignore_ascii_case(id)).cloned());
        Ok(held.map_or_else(|| "resolves to nothing".into(), |h| format!("differs only by case from {h}")))
    }

    fn pairs(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<(String, String)>, Refusal> {
        let run = || -> rusqlite::Result<Vec<(String, String)>> {
            let mut st = self.conn.prepare(sql)?;
            st.query_map(p, |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
        };
        run().map_err(|e| failed(Path::new(".bp/index.db"), &e))
    }
}

/// Every path git tracks in the vault. A run names the vault's head with a clean project tree, so these are the
/// files the run's commit holds.
fn tracked(root: &Path) -> Result<BTreeSet<String>, Refusal> {
    let out = Command::new("git").arg("-C").arg(root).args(["ls-files", "-z"]).output();
    match out {
        Ok(o) if o.status.success() => {
            Ok(String::from_utf8_lossy(&o.stdout).split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect())
        }
        Ok(o) => Err(Refusal::failed("git ls-files", String::from_utf8_lossy(&o.stderr).trim())),
        Err(e) => Err(Refusal::failed("git ls-files", format!("git did not start: {e}"))),
    }
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
    use crate::{Gap, Index};

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn node(kind: &str, extra: &str) -> String {
        format!("---\nkind: {kind}\ntitle: T\nstatus: open\n{extra}---\n\nBody.\n")
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git").arg("-C").arg(root).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    /// A clean project `bp` and a project `ex` that breaks every rule, so a fault of `bp` would be out of scope.
    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        for p in ["bp", "ex"] {
            write(r, &format!("projects/{p}/project.md"), &node("project", ""));
            write(
                r,
                &format!("projects/{p}/designs/product.md"),
                &node("design", &format!("under: projects/{p}/project\n")),
            );
            let under = format!("under: projects/{p}/designs/product\n");
            write(r, &format!("projects/{p}/designs/system.md"), &node("design", &under));
            write(r, &format!("projects/{p}/milestones/m.md"), &node("milestone", ""));
            let cites = format!("under: projects/{p}/milestones/m\ncites: [projects/{p}/designs/system]\n");
            write(r, &format!("projects/{p}/issues/ok.md"), &node("issue", &cites));
            let under = format!("under: projects/{p}/designs/system\n");
            write(r, &format!("projects/{p}/designs/containers/c.md"), &node("design", &under));
        }
        write(r, "projects/bp/designs/system/diagrams/d.html", "<p/>\n");
        write(r, "shared/threads/t.md", &node("thread", "cites: [shared/nowhere/x]\n"));
        git(r, &["init", "-q"]);
        let v = Vault { root: r.to_path_buf() };
        (dir, v)
    }

    /// The faults of `project`, every file on disk tracked first: integrity reads the tracked set.
    fn all(v: &Vault, project: &str) -> Vec<Fault> {
        git(&v.root, &["add", "-A"]);
        Index::open(v).unwrap().integrity(v, project).unwrap()
    }

    fn faults(v: &Vault, project: &str) -> Vec<(&'static str, String, bool)> {
        all(v, project).into_iter().map(|f| (f.rule, f.id, f.node)).collect()
    }

    fn details(v: &Vault, project: &str, rule: &str) -> Vec<(String, String)> {
        all(v, project).into_iter().filter(|f| f.rule == rule).map(|f| (f.id, f.detail)).collect()
    }

    #[test]
    fn a_clean_project_has_no_fault_and_shared_nodes_are_out_of_scope() {
        let (_d, v) = vault();
        assert_eq!(faults(&v, "bp"), []);
    }

    #[test]
    fn rule_1_a_file_that_does_not_parse_and_faults_with_no_node_come_last() {
        let (_d, v) = vault();
        write(&v.root, "projects/ex/issues/bad.md", "---\nkind: [\n---\n");
        write(&v.root, "projects/ex/Notes.md", "# stray\n");
        write(&v.root, "projects/ex/issues/zz.md", &node("issue", ""));
        let found = faults(&v, "ex");
        let tail: Vec<_> = found.iter().skip_while(|f| f.2).cloned().collect();
        let want =
            [("broken", "projects/ex/Notes.md".into(), false), ("broken", "projects/ex/issues/bad.md".into(), false)];
        assert_eq!(tail, want, "the output's tail keeps what no record names: {found:?}");
        assert!(found.len() > 2 && found[0].2, "{found:?}");
    }

    #[test]
    fn rule_2_a_link_that_resolves_to_nothing() {
        let (_d, v) = vault();
        let links = "under: projects/ex/milestones/m\ncites: [projects/ex/designs/system, projects/ex/designs/gone, '[[old]]']\n";
        let body = "[[projects/ex/issues/nope]] [[projects/ex/issues/ok]] [[projects/ex/designs/SYSTEM]] \
                    [a](/projects/ex/designs/system/diagrams/d.html) [b](/projects/bp/designs/system/diagrams/d.html#x) \
                    [B](/projects/bp/designs/system/diagrams/D.html) [c](/projects/ex/issues/ok.md) \
                    [d](/projects/ex/issues/missing.md) [t](/projects/../../etc/hosts) [u](/projects/ex/untracked.html) \
                    `[e](/projects/ex/in/code)`\n";
        write(&v.root, "projects/ex/issues/links.md", &node("issue", links).replace("Body.\n", body));
        git(&v.root, &["add", "-A"]);
        // On disk but not in git: the run's commit would not hold it.
        write(&v.root, "projects/ex/untracked.html", "<p/>\n");
        let index = Index::open(&v).unwrap();
        let details: Vec<String> = index.integrity(&v, "ex").unwrap().into_iter().map(|f| f.detail).collect();
        assert_eq!(
            details,
            [
                "body projects/ex/designs/SYSTEM: differs only by case from projects/ex/designs/system",
                "body projects/ex/issues/nope: resolves to nothing",
                "cites [[old]]: not an id",
                "cites projects/ex/designs/gone: resolves to nothing",
                "markdown /projects/../../etc/hosts: a `..` segment leaves the path the link names",
                "markdown /projects/bp/designs/system/diagrams/D.html: differs only by case from projects/bp/designs/system/diagrams/d.html",
                "markdown /projects/ex/designs/system/diagrams/d.html: resolves to nothing",
                "markdown /projects/ex/issues/missing.md: resolves to nothing",
                "markdown /projects/ex/untracked.html: resolves to nothing",
            ]
        );
    }

    #[test]
    fn rule_3_is_the_walks_predicate_with_aliases_and_empty_targets() {
        let (_d, v) = vault();
        write(&v.root, "projects/ex/issues/bare.md", &node("issue", ""));
        let wrong = "under: projects/ex/designs/system\ncites: [projects/ex/milestones/m]\n";
        write(&v.root, "projects/ex/issues/wrong.md", &node("issue", wrong));
        let done = "under: projects/ex/designs/system\n";
        write(&v.root, "projects/ex/issues/done.md", &node("issue", done).replace("status: open", "status: resolved"));
        let aliased = "under: projects/ex/milestones/old-m\ncites: [projects/ex/designs/containers/old-c]\n";
        write(&v.root, "projects/ex/issues/aliased.md", &node("issue", aliased));
        write(&v.root, "projects/ex/milestones/m.md", &node("milestone", "aliases: [old-m]\n"));
        let c = "under: projects/ex/designs/system\naliases: [old-c]\n";
        write(&v.root, "projects/ex/designs/containers/c.md", &node("design", c));
        let empty = "under: projects/ex/milestones/m\ncites: [projects/ex/designs/containers/e]\n";
        write(&v.root, "projects/ex/issues/empty.md", &node("issue", empty));
        let e = node("design", "under: projects/ex/designs/system\n").replace("Body.\n", "");
        write(&v.root, "projects/ex/designs/containers/e.md", &e);
        let one = "under: projects/ex/milestones/m\ncites: [projects/ex/designs/system, projects/ex/designs/components/gone]\n";
        write(&v.root, "projects/ex/issues/one.md", &node("issue", one));
        let want = [
            ("projects/ex/issues/bare", "no milestone under `under`"),
            ("projects/ex/issues/bare", "no design under `cites`"),
            ("projects/ex/issues/empty", "the design projects/ex/designs/containers/e is empty"),
            ("projects/ex/issues/one", "the design projects/ex/designs/components/gone is absent"),
            ("projects/ex/issues/wrong", "no milestone under `under`"),
            ("projects/ex/issues/wrong", "no design under `cites`"),
        ];
        let found = details(&v, "ex", "issue");
        assert_eq!(
            found,
            want.map(|(a, b)| (a.to_string(), b.to_string())),
            "an alias and a resolved issue pass; each cited design must hold"
        );
        let index = Index::open(&v).unwrap();
        let gap = index.context_gap("projects/ex/issues/empty", ISSUE_CONTEXT[1]).unwrap();
        assert_eq!(gap, Some(Gap::Empty("projects/ex/designs/containers/e".into())), "the walk reads the same gap");
    }

    #[test]
    fn rule_4_a_design_off_the_spine() {
        let (_d, v) = vault();
        let under = |u: &str| node("design", &format!("under: {u}\n"));
        write(&v.root, "projects/ex/designs/containers/bad.md", &under("projects/ex/project"));
        write(&v.root, "projects/ex/designs/containers/orphan.md", &node("design", ""));
        write(&v.root, "projects/ex/designs/containers/other.md", &under("projects/bp/designs/system"));
        write(&v.root, "projects/ex/designs/components/ok-c.md", &under("projects/ex/designs/containers/c"));
        write(&v.root, "projects/ex/designs/components/ok-s.md", &under("projects/ex/designs/system"));
        write(&v.root, "projects/ex/designs/components/bad.md", &under("projects/ex/designs/components/ok-c"));
        write(&v.root, "projects/ex/designs/data-model.md", &under("projects/ex/project"));
        write(&v.root, "projects/ex/designs/glossary.md", &under("projects/ex/designs/system"));
        write(&v.root, "projects/ex/designs/system.md", &under("projects/ex/designs/nowhere"));
        let found = details(&v, "ex", "level");
        let want = [
            (
                "components/bad",
                "`under` projects/ex/designs/components/ok-c is a component; a component design sits under a container or a system design of its project",
            ),
            (
                "containers/bad",
                "`under` projects/ex/project is a project; a container design sits under a system design of its project",
            ),
            ("containers/orphan", "no `under`; a container design sits under a system design of its project"),
            (
                "containers/other",
                "`under` projects/bp/designs/system is outside projects/ex; a container design sits under a system design of its project",
            ),
            (
                "data-model",
                "`under` projects/ex/project is a project; a data-model design sits under a system design of its project",
            ),
            (
                "system",
                "`under` projects/ex/designs/nowhere is no node; a system design sits under a product design of its project",
            ),
        ];
        assert_eq!(found, want.map(|(a, b)| (format!("projects/ex/designs/{a}"), b.to_string())));
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
        // git holds both twins on any file system; the default macOS one holds only one of them on disk. A twin of
        // the project's own directory counts too.
        let (_d, v) = vault();
        git(&v.root, &["add", "-A"]);
        let blob = git(&v.root, &["hash-object", "-w", "projects/ex/issues/ok.md"]);
        for twin in ["projects/ex/issues/OK.md", "projects/Ex/issues/ok.md"] {
            git(&v.root, &["update-index", "--add", "--cacheinfo", &format!("100644,{},{twin}", blob.trim())]);
        }
        let found = Index::open(&v).unwrap().integrity(&v, "ex").unwrap();
        let found: Vec<_> = found.into_iter().map(|f| (f.rule, f.id, f.node)).collect();
        let want = [
            ("case", "projects/ex/issues/ok".to_string(), true),
            ("case", "projects/Ex/issues/ok".to_string(), false),
            ("case", "projects/ex/issues/OK".to_string(), false),
        ];
        assert_eq!(found, want, "only the parsed twin can be a run's edge, and it leads");
    }

    #[test]
    fn a_vault_git_cannot_read_could_not_run() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault { root: dir.path().to_path_buf() };
        write(&v.root, "projects/ex/project.md", &node("project", ""));
        let e = Index::open(&v).unwrap().integrity(&v, "ex").unwrap_err();
        assert_eq!(e.code, Code::CouldNotRun, "not a vault with no twins: {e:?}");
    }
}

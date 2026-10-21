//! `bp diagram` (28, design changes 5 and 13). `bp diagram <design> <name> --spec-file <f>` checks, renders and
//! writes the spec and its page into the design in one vault commit; `--remove` removes both. On a design past
//! `draft` it refuses `design_not_delta`, as `set --body-file` does. `bp diagram --spec-file <f> --out <path>
//! [--base <spec>]` is the preview: it renders a spec, or the diff of two, outside the vault and writes nothing in
//! it. Either form refuses a broken hard rule with `diagram_invalid`, whose envelope adds every finding, each naming
//! its rule and element; a warning does not refuse.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bp_check::nodes::{Names, is_name, specs, terms};
use bp_design::DesignStatus;
use bp_diagram::{Finding, Link, Severity};
use bp_vault::{Code, Id, Kind, Moves, Refusal, SLUG_MAX, Vault};
use clap::Args;
use serde::Serialize;
use serde_json::{Value, json};

use crate::{Wrote, emit, finish, subject};

/// `bp diagram <design> <name> (--spec-file <f> | --remove)` or `bp diagram --spec-file <f> --out <path> [--base
/// <spec>]`.
#[derive(Args)]
pub struct Diagram {
    /// The design that holds the diagram, for example `projects/bp/designs/system`. Without it, the verb previews.
    #[arg(requires = "name")]
    pub design: Option<String>,
    /// The diagram's name: the spec is `<design>/diagrams/<name>.yaml`, its page `<name>.html` beside it.
    pub name: Option<String>,
    /// The diagram spec: YAML with `type`, `title`, `lede`, `nodes` and `edges` (13, Diagram).
    #[arg(long, required_unless_present = "remove")]
    pub spec_file: Option<PathBuf>,
    /// The preview's page: a path outside the vault, so the agent and the human look first.
    #[arg(long, required_unless_present = "design", conflicts_with = "design")]
    pub out: Option<PathBuf>,
    /// A base spec: the preview shows the change from it, with `added`, `removed`, `changed` and `moved` marks.
    #[arg(long, requires = "out")]
    pub base: Option<PathBuf>,
    /// Remove the diagram's spec and page from the design.
    #[arg(long, requires = "design", conflicts_with = "spec_file")]
    pub remove: bool,
}

/// `diagram_invalid`: the five fields of every refusal, then every finding.
#[derive(Serialize)]
struct Invalid {
    #[serde(flatten)]
    refusal: Refusal,
    findings: Vec<Value>,
}

/// A refusal, and for `diagram_invalid` every finding.
pub(crate) struct Refused(pub(crate) Refusal, pub(crate) Option<Vec<Value>>);

impl From<Refusal> for Refused {
    fn from(r: Refusal) -> Self {
        Self(r, None)
    }
}

pub(crate) fn rows(findings: &[Finding]) -> Vec<Value> {
    findings
        .iter()
        .map(|f| json!({ "rule": f.rule, "id": f.id, "severity": f.severity.as_str(), "detail": f.detail }))
        .collect()
}

/// `diagram_invalid` for the first error of `findings`; `then` is the command that tries again.
pub(crate) fn invalid(spec: &Path, findings: &[Finding], then: &str) -> Refused {
    let first = findings.iter().find(|f| f.severity == Severity::Error).unwrap_or(&findings[0]);
    let refusal = Refusal::new(
        Code::DiagramInvalid,
        format!("{}#{}", spec.display(), first.id),
        "a diagram spec breaks no hard rule (28, design change 4)",
        format!("edit {}: {} `{}`: {}; then {then}", spec.display(), first.rule, first.id, first.detail),
    );
    Refused(refusal, Some(rows(findings)))
}

pub(crate) fn read(cwd: &Path, path: &Path) -> Result<String, Refusal> {
    std::fs::read_to_string(cwd.join(path))
        .map_err(|e| Refusal::failed(path.display().to_string(), format!("{e}; name a readable spec file")))
}

pub(crate) fn run(cwd: &Path, d: &Diagram, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    let done = match (&d.design, &d.name) {
        (Some(design), Some(name)) => write(cwd, d, design, name),
        _ => preview(cwd, d).map(Wrote::Changed),
    };
    answer(done, json, out)
}

/// The exit of a write that may refuse `diagram_invalid`, with every finding: `diagram` and `record delta`.
pub(crate) fn answer(done: Result<Wrote, Refused>, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    match done {
        Ok(wrote) => crate::wrote(Ok(wrote), json, out),
        Err(Refused(refusal, None)) => finish::<Value>(Err(refusal), json, out),
        Err(Refused(refusal, Some(findings))) => {
            let exit = refusal.exit;
            emit(out, &Invalid { refusal, findings }, json);
            ExitCode::from(exit)
        }
    }
}

/// The vault-writing form: the spec and its page into `<design>/diagrams/`, or both out with `--remove`, in one
/// commit with `Bp-Verb: diagram`. A repeat that changes no file exits 3.
fn write(cwd: &Path, d: &Diagram, design: &str, name: &str) -> Result<Wrote, Refused> {
    let spec = d.spec_file.as_deref().map(|f| read(cwd, f).map(|text| (f, text))).transpose()?;
    if !is_name(name) {
        return Err(
            Refusal::usage(name, "name the diagram in lowercase letters, digits and `-`, such as containers").into()
        );
    }
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    let writer = vault.writer()?;
    let id = subject(writer.index(), design)?;
    let header = writer.index().header(id.as_str())?;
    let field = |k: &str| header.as_ref().and_then(|h| h.get(k)?.as_str().map(str::to_string));
    match field("kind") {
        None => {
            return Err(Refusal::new(
                Code::NoNode,
                id.as_str(),
                "a diagram belongs to a design that exists (13, Diagram)",
                format!("name a design: bp list design --project {}", id.project().unwrap_or("<project>")),
            )
            .into());
        }
        Some(k) if k.parse::<Kind>().ok() != Some(Kind::Design) => {
            return Err(Refusal::new(
                Code::SchemaInvalid,
                id.as_str(),
                "a diagram belongs to a design (13, Diagram)",
                format!("a diagram belongs to a design; {id} is kind {k}"),
            )
            .into());
        }
        Some(_) => {}
    }
    let preview = format!("bp diagram --spec-file <f> --out /tmp/{name}.html");
    if field("status").as_deref() != Some(DesignStatus::Draft.as_str()) {
        return Err(Refusal::new(
            Code::DesignNotDelta,
            id.as_str(),
            "a design past draft changes only through a delta record; its diagrams change with it (13, Design history)",
            format!(
                "bp record delta {id} --field milestone=<milestone> --body-file <f> --diagram {name}=<f>; or --editorial \
                 when only wording and layout change; {preview} renders it first"
            ),
        )
        .into());
    }
    let project = id.project().expect("a design sits in a project");
    let paths = [format!("{id}/diagrams/{name}.yaml"), format!("{id}/diagrams/{name}.html")];
    // The names come from the design's body and the glossary as committed, so the `nodes` check, which runs on the
    // head, reads the same ones.
    let read_too = [format!("{id}.md"), format!("projects/{project}/designs/glossary.md")];
    writer.clean_paths(paths.iter().chain(&read_too).cloned().collect())?;
    let on_disk = |p: &str| std::fs::read_to_string(vault.root.join(p)).ok();
    let (files, findings) = match spec {
        None => (paths.iter().filter(|p| on_disk(p).is_some()).map(|p| (p.clone(), None)).collect::<Vec<_>>(), vec![]),
        Some((file, spec)) => {
            let names = Names::of(&vault, &id, &vault.read(&id)?.body.text);
            let then = format!("bp diagram {id} {name} --spec-file {} --json", file.display());
            let specs_of = |c: &Id| specs(&vault, c).0.into_iter().map(|(_, t)| t).collect();
            let render = page(&spec, project, &names, &specs_of).map_err(|f| invalid(file, &f, &then))?;
            let texts = [spec, render.html];
            let files = paths
                .iter()
                .zip(texts)
                .filter(|(p, t)| on_disk(p).as_ref() != Some(t))
                .map(|(p, t)| (p.clone(), Some(t)))
                .collect();
            (files, render.findings)
        }
    };
    let mut result = json!({ "id": id, "name": name });
    if files.is_empty() {
        return Ok(Wrote::Already(result));
    }
    let key = if d.remove { "removed" } else { "written" };
    let changed: Vec<String> = files.iter().map(|(p, _)| p.clone()).collect();
    let moves = Moves { writes: files, ..Moves::default() };
    let written =
        writer.commit_moving("diagram", role.as_str(), &format!("diagram {id} {name}"), &BTreeMap::new(), &moves)?;
    result[key] = changed.into();
    result["commit"] = written.commit.into();
    result["findings"] = rows(&findings).into();
    Ok(Wrote::Changed(result))
}

/// The page of `spec`, a diagram of a design in `project` whose links `names` resolves, or every finding. The drill
/// badge's count (28, design change 12): a node of a container-level diagram drills to the component-level diagrams
/// of the container design its link names (`m5-interactive-architecture.md` §3.3), whose spec texts `specs_of` gives.
/// The page keeps the count it was rendered with.
pub(crate) fn page(
    spec: &str,
    project: &str,
    names: &Names,
    specs_of: &dyn Fn(&Id) -> Vec<String>,
) -> Result<bp_diagram::Render, Vec<Finding>> {
    let level = level_of(spec);
    let children = |link: Link<'_>| {
        let (Link::Term(text) | Link::Noun(text)) = link;
        let container = Id::parse(&format!("projects/{project}/designs/containers/{}", bp_vault::slug(text, SLUG_MAX)));
        match (level.as_deref(), container) {
            (Some("container"), Ok(c)) => {
                specs_of(&c).iter().filter(|t| level_of(t).as_deref() == Some("component")).count()
            }
            _ => 0,
        }
    };
    bp_diagram::render_with(spec, None, Some(&|l| names.resolves(l)), &children).map_err(|mut f| {
        names.explain(&mut f);
        f
    })
}

/// A spec's level, read without resolving its links; none when it has none or does not parse.
pub(crate) fn level_of(spec: &str) -> Option<String> {
    bp_diagram::outline(spec, &|_| true).ok().and_then(|(level, _)| level)
}

/// The slugs the vault's glossaries define, over every project: the preview names no design. None when no project
/// has a glossary, so there is nothing to check a term against.
fn glossary(vault: &Vault) -> Option<BTreeSet<String>> {
    let projects = std::fs::read_dir(vault.root.join("projects")).ok()?;
    let texts: Vec<String> =
        projects.filter_map(|p| std::fs::read_to_string(p.ok()?.path().join("designs/glossary.md")).ok()).collect();
    (!texts.is_empty()).then(|| texts.iter().flat_map(|t| terms(t).map(str::to_string)).collect())
}

fn warn(detail: &str) -> Finding {
    Finding { rule: "nodes/resolve", id: "spec".into(), severity: Severity::Warn, detail: detail.into() }
}

/// An `--out` inside the vault: the preview writes nothing there (28, design change 5), and a file the tool did not
/// commit would hide from the guard hook.
fn inside(vault: &Vault, cwd: &Path, path: &Path) -> bool {
    let full = cwd.join(path);
    let dir = full.parent().and_then(|p| p.canonicalize().ok());
    dir.is_some_and(|d| d.starts_with(&vault.root))
}

/// The preview form: the page at `--out`, outside the vault.
fn preview(cwd: &Path, d: &Diagram) -> Result<Value, Refused> {
    let (Some(file), Some(out)) = (&d.spec_file, &d.out) else {
        unreachable!("clap requires --spec-file and --out without a design")
    };
    let vault = Vault::discover(cwd).ok();
    if let Some(v) = vault.as_ref().filter(|v| inside(v, cwd, out)) {
        let name = out.file_name().map_or_else(|| "diagram.html".into(), |n| n.to_string_lossy().into_owned());
        return Err(Refusal::new(
            Code::Usage,
            out.display().to_string(),
            "the preview renders outside the vault and writes nothing in it (28, design change 5)",
            format!(
                "pass --out a path outside {}, such as /tmp/{name}; bp diagram <design> <name> writes into a design",
                v.root.display()
            ),
        )
        .into());
    }
    let spec = read(cwd, file)?;
    let base = d.base.as_ref().map(|b| read(cwd, b)).transpose()?;
    // A term resolves against the vault's glossaries. A noun resolves against its home design, which only the
    // vault-writing form has, so the preview names it once as unchecked.
    let terms = vault.as_ref().and_then(glossary);
    let noun_seen = Cell::new(false);
    let resolve = |link: Link<'_>| match link {
        Link::Term(t) => terms.as_ref().is_none_or(|known| known.contains(t)),
        Link::Noun(_) => {
            noun_seen.set(true);
            true
        }
    };
    let mut unchecked = Vec::new();
    match (&vault, &terms) {
        (None, _) => unchecked.push(warn("no vault: terms and nouns were not checked; set BP_VAULT to check terms")),
        (Some(_), None) => unchecked.push(warn("the vault holds no glossary design, so terms were not checked")),
        _ => {}
    }
    let then = format!("bp diagram --spec-file {} --out {} --json", file.display(), out.display());
    let mut render =
        bp_diagram::render(&spec, base.as_deref(), Some(&resolve)).map_err(|f| invalid(file, &f, &then))?;
    if vault.is_some() && noun_seen.get() {
        unchecked.push(warn("nouns resolve against their home design, which bp diagram <design> <name> names"));
    }
    render.findings.extend(unchecked);
    std::fs::write(cwd.join(out), &render.html)
        .map_err(|e| Refusal::failed(out.display().to_string(), format!("{e}; name a writable path")))?;
    Ok(json!({ "out": out.display().to_string(), "bytes": render.html.len(), "findings": rows(&render.findings) }))
}

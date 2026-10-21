//! `bp diagram --spec-file <f> --out <path> [--base <spec>]`: the preview form (28, design changes 5 and 13). It
//! checks and renders a spec, or the diff of two, to a file outside the vault and writes nothing to the vault. Its
//! result lists the warnings; an error refuses with `diagram_invalid`, and the envelope adds every finding, each
//! naming its rule and element. The vault-writing form is issue 2's.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bp_diagram::{Finding, Link, Severity};
use bp_vault::{Code, Refusal, Vault};
use clap::Args;
use serde::Serialize;
use serde_json::{Value, json};

use crate::{emit, finish};

/// `bp diagram --spec-file <f> --out <path> [--base <spec>]`.
#[derive(Args)]
pub struct Diagram {
    /// The diagram spec: YAML with `type`, `title`, `lede`, `nodes` and `edges` (13, Diagram).
    #[arg(long)]
    pub spec_file: PathBuf,
    /// Where the HTML goes: a path outside the vault, so the agent and the human look first.
    #[arg(long)]
    pub out: PathBuf,
    /// A base spec: the render shows the change from it, with `added`, `removed`, `changed` and `moved` marks.
    #[arg(long)]
    pub base: Option<PathBuf>,
}

/// `diagram_invalid`: the five fields of every refusal, then every finding.
#[derive(Serialize)]
struct Invalid {
    #[serde(flatten)]
    refusal: Refusal,
    findings: Vec<Value>,
}

fn rows(findings: &[bp_diagram::Finding]) -> Vec<Value> {
    findings
        .iter()
        .map(|f| json!({ "rule": f.rule, "id": f.id, "severity": f.severity.as_str(), "detail": f.detail }))
        .collect()
}

fn read(cwd: &Path, path: &Path) -> Result<String, Refusal> {
    std::fs::read_to_string(cwd.join(path))
        .map_err(|e| Refusal::failed(path.display().to_string(), format!("{e}; name a readable spec file")))
}

/// The slugs the vault's glossaries define: each `projects/<p>/designs/glossary.md` lists a term as `` - `slug` — ``
/// (03's form). None when no project has a glossary, so there is nothing to check a term against.
fn glossary(vault: &Vault) -> Option<BTreeSet<String>> {
    let projects = std::fs::read_dir(vault.root.join("projects")).ok()?;
    let texts: Vec<String> =
        projects.filter_map(|p| std::fs::read_to_string(p.ok()?.path().join("designs/glossary.md")).ok()).collect();
    let slug = |line: &str| Some(line.trim_start().strip_prefix("- `")?.split_once('`')?.0.to_string());
    (!texts.is_empty()).then(|| texts.iter().flat_map(|t| t.lines().filter_map(slug)).collect())
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

pub(crate) fn run(cwd: &Path, d: &Diagram, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    let vault = Vault::discover(cwd).ok();
    if let Some(v) = vault.as_ref().filter(|v| inside(v, cwd, &d.out)) {
        let name = d.out.file_name().map_or_else(|| "diagram.html".into(), |n| n.to_string_lossy().into_owned());
        let refusal = Refusal::new(
            Code::Usage,
            d.out.display().to_string(),
            "the preview renders outside the vault and writes nothing in it (28, design change 5)",
            format!(
                "pass --out a path outside {}, such as /tmp/{name}; bp diagram <design> <name> writes into a design",
                v.root.display()
            ),
        );
        return finish::<Value>(Err(refusal), json, out);
    }
    let texts = read(cwd, &d.spec_file).and_then(|s| Ok((s, d.base.as_ref().map(|b| read(cwd, b)).transpose()?)));
    let (spec, base) = match texts {
        Ok(t) => t,
        Err(refusal) => return finish::<Value>(Err(refusal), json, out),
    };
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
    let render = match bp_diagram::render(&spec, base.as_deref(), Some(&resolve)) {
        Ok(mut r) => {
            if vault.is_some() && noun_seen.get() {
                unchecked.push(warn("nouns resolve against their home design, which bp diagram <design> <name> names"));
            }
            r.findings.extend(unchecked);
            r
        }
        Err(findings) => {
            let first = findings.iter().find(|f| f.severity == bp_diagram::Severity::Error).unwrap_or(&findings[0]);
            let refusal = Refusal::new(
                Code::DiagramInvalid,
                format!("{}#{}", d.spec_file.display(), first.id),
                "a diagram spec breaks no hard rule (28, design change 4)",
                format!(
                    "edit {}: {} `{}`: {}; then bp diagram --spec-file {} --out {} --json",
                    d.spec_file.display(),
                    first.rule,
                    first.id,
                    first.detail,
                    d.spec_file.display(),
                    d.out.display()
                ),
            );
            let exit = refusal.exit;
            emit(out, &Invalid { refusal, findings: rows(&findings) }, json);
            return ExitCode::from(exit);
        }
    };
    if let Err(e) = std::fs::write(cwd.join(&d.out), &render.html) {
        return finish::<Value>(
            Err(Refusal::failed(d.out.display().to_string(), format!("{e}; name a writable path"))),
            json,
            out,
        );
    }
    let result =
        json!({ "out": d.out.display().to_string(), "bytes": render.html.len(), "findings": rows(&render.findings) });
    finish(Ok(result), json, out)
}

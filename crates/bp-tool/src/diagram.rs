//! `bp diagram --spec-file <f> --out <path> [--base <spec>]`: the preview form (28, design changes 5 and 13). It
//! checks and renders a spec, or the diff of two, to a file outside the vault and writes nothing to the vault. Its
//! result lists the warnings; an error refuses with `diagram_invalid`, and the envelope adds every finding, each
//! naming its rule and element. The vault-writing form is issue 2's.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bp_vault::{Code, Refusal};
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

pub(crate) fn run(cwd: &Path, d: &Diagram, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    let texts = read(cwd, &d.spec_file).and_then(|s| Ok((s, d.base.as_ref().map(|b| read(cwd, b)).transpose()?)));
    let (spec, base) = match texts {
        Ok(t) => t,
        Err(refusal) => return finish::<Value>(Err(refusal), json, out),
    };
    let render = match bp_diagram::render(&spec, base.as_deref(), None) {
        Ok(r) => r,
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

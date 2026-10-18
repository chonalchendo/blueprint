//! `cargo xtask <check>`. Today: `deps`, the crate rule. Exit 0 when the rule holds, 2 with one line per violation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use serde_json::Value;

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    if task != "deps" {
        eprintln!("usage: cargo xtask deps");
        return ExitCode::from(4);
    }
    let result = deps(Path::new("."));
    match result {
        Ok(violations) if violations.is_empty() => ExitCode::SUCCESS,
        Ok(violations) => {
            for v in violations {
                eprintln!("{v}");
            }
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("xtask: {e:#}");
            ExitCode::from(1)
        }
    }
}

/// Runs the crate rule over the workspace at `root`. Returns the violations, one line each.
fn deps(root: &Path) -> Result<Vec<String>> {
    let rule = Rule::parse(&std::fs::read_to_string(root.join("crate-rule.txt")).context("read crate-rule.txt")?)?;
    let out = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .context("run cargo metadata")?;
    if !out.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).context("parse cargo metadata")?;
    Ok(rule.check(&meta))
}

#[derive(Debug, Default)]
struct Rule {
    /// crate -> (permitted workspace dependency -> permitted features)
    edges: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
    /// external dependency -> the one crate that may depend on it
    only: BTreeMap<String, String>,
}

impl Rule {
    fn parse(text: &str) -> Result<Self> {
        let mut rule = Self::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            if let Some(rest) = line.strip_prefix("only ") {
                let (krate, dep) = rest.split_once(':').context("`only <crate>: <dep>`")?;
                rule.only.insert(dep.trim().to_string(), krate.trim().to_string());
                continue;
            }
            let (krate, deps) = line.split_once("->").with_context(|| format!("`<crate> -> <deps>`: {line}"))?;
            let mut permitted = BTreeMap::new();
            for dep in deps.split_whitespace() {
                let (name, features) = match dep.split_once('[') {
                    Some((n, f)) => (n, f.trim_end_matches(']').split(',').map(str::to_string).collect()),
                    None => (dep, BTreeSet::new()),
                };
                permitted.insert(name.to_string(), features);
            }
            rule.edges.insert(krate.trim().to_string(), permitted);
        }
        Ok(rule)
    }

    fn check(&self, meta: &Value) -> Vec<String> {
        let packages = meta["packages"].as_array().cloned().unwrap_or_default();
        let members: BTreeSet<String> =
            packages.iter().filter_map(|p| p["name"].as_str().map(str::to_string)).collect();
        let mut violations = Vec::new();
        for p in &packages {
            let name = p["name"].as_str().unwrap_or_default();
            let Some(permitted) = self.edges.get(name) else {
                violations.push(format!("crate not in the rule: {name}"));
                continue;
            };
            for d in p["dependencies"].as_array().into_iter().flatten() {
                if d["kind"].as_str().is_some_and(|k| k == "dev") {
                    continue;
                }
                let dep = d["name"].as_str().unwrap_or_default();
                if members.contains(dep) {
                    match permitted.get(dep) {
                        None => violations.push(format!("edge outside the rule: {name} -> {dep}")),
                        Some(features) => {
                            for f in d["features"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                                if !features.contains(f) {
                                    violations.push(format!("feature outside the rule: {name} -> {dep}[{f}]"));
                                }
                            }
                        }
                    }
                } else if let Some(owner) = self.only.get(dep)
                    && owner != name
                {
                    violations.push(format!("external dependency reserved for {owner}: {name} -> {dep}"));
                }
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
    }

    /// Copies the manifests of the workspace into a temporary directory, so a test can break one edge.
    fn copy_workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = workspace_root();
        for rel in ["Cargo.toml", "crate-rule.txt", "rust-toolchain.toml"] {
            std::fs::copy(root.join(rel), dir.path().join(rel)).unwrap();
        }
        for member in ["crates", "xtask"] {
            copy_manifests(&root.join(member), &dir.path().join(member));
        }
        dir
    }

    fn copy_manifests(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let path = entry.path();
            let dest = to.join(entry.file_name());
            if path.is_dir() && entry.file_name() != "target" {
                copy_manifests(&path, &dest);
            } else if path.file_name().is_some_and(|f| f == "Cargo.toml") {
                std::fs::copy(&path, &dest).unwrap();
            } else if path.extension().is_some_and(|e| e == "rs") {
                // cargo metadata needs the targets to exist; their content does not matter.
                std::fs::write(&dest, "").unwrap();
            }
        }
    }

    /// Adds a line under `[dependencies]`: appending would land in whatever table ends the manifest.
    fn insert_dependency(dir: &Path, rel: &str, line: &str) {
        let p = dir.join(rel);
        let s = std::fs::read_to_string(&p).unwrap().replace("[dependencies]\n", &format!("[dependencies]\n{line}\n"));
        std::fs::write(&p, s).unwrap();
    }

    #[test]
    fn the_workspace_holds_the_rule() {
        let violations = deps(&workspace_root()).unwrap();
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn deps_refuses_an_edge_outside_the_rule() {
        let dir = copy_workspace();
        insert_dependency(dir.path(), "crates/bp-view/Cargo.toml", "bp-check.workspace = true");
        let violations = deps(dir.path()).unwrap();
        assert_eq!(violations, vec!["edge outside the rule: bp-view -> bp-check".to_string()]);
    }

    #[test]
    fn deps_refuses_the_write_feature_outside_bp_tool() {
        let dir = copy_workspace();
        let p = dir.path().join("crates/bp-view/Cargo.toml");
        let s = std::fs::read_to_string(&p)
            .unwrap()
            .replace("bp-forge.workspace = true", "bp-forge = { workspace = true, features = [\"write\"] }");
        std::fs::write(&p, s).unwrap();
        let violations = deps(dir.path()).unwrap();
        assert_eq!(violations, vec!["feature outside the rule: bp-view -> bp-forge[write]".to_string()]);
    }

    #[test]
    fn deps_refuses_a_reserved_external_dependency() {
        let dir = copy_workspace();
        insert_dependency(dir.path(), "crates/bp-vault/Cargo.toml", "tokio = \"1\"");
        let violations = deps(dir.path()).unwrap();
        assert_eq!(violations, vec!["external dependency reserved for bp-view: bp-vault -> tokio".to_string()]);
    }
}

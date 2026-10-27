//! `cargo xtask <check>`. `deps`, the crate rule: exit 0 when the rule holds, 2 with one line per violation.
//! `bench`, the call budget: prints the largest ratio last. `scratch`, a writable copy of `$BP_VAULT`: prints its
//! directory. `lines`, the file cap: exit 2 with one line per non-test file over 500 lines. `forge <project>`, a fake
//! forge for a repo block: prints its directory. `gh`, the fake `gh` that directory's shim runs. `eval`, the skill eval and its grader
//! (`eval.rs`). `mutants`, the planted-defect fixtures of the review eval (`mutants.rs`). `plants`, the planted-fact
//! fixtures of the research eval (`plants.rs`).
//! `sandbox [claim]`, 27's prelude as `export` lines. `stage`, a staged copy (32, design change 11): `VAULT=`, `FORGE=`
//! and `CLONE=` lines.

mod bench;
mod eval;
mod eval_agent;
mod eval_arms;
mod eval_end;
mod eval_fixture;
mod eval_grade;
mod eval_kills;
mod eval_plant;
mod eval_report;
mod eval_result;
mod eval_sandbox;
mod eval_score;
mod eval_trace;
mod forge;
mod gh;
mod lines;
mod mutants;
mod mutants_walk;
mod plants;
mod plants_count;
mod plants_flip;
mod plants_hop;
mod plants_judge;
mod plants_leak;
mod plants_pool;
mod plants_text;
mod plants_verify;
mod sandbox;
mod scratch;
mod stage;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use serde_json::Value;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("deps") => {}
        Some("lines") => {
            return report(lines::run(Path::new(".")));
        }
        Some("bench") => {
            return match bench::run(&args[1..]) {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("xtask: {e:#}");
                    ExitCode::from(1)
                }
            };
        }
        Some("gh") => return gh::run(&args[1..]),
        Some(verb @ ("eval" | "mutants" | "plants")) => {
            let done = match verb {
                "eval" => eval::run(&args[1..]),
                "mutants" => mutants::run(&args[1..]),
                _ => plants::run(&args[1..]),
            };
            return match done {
                Err(e) if e.is::<eval::Usage>() => {
                    eprintln!("xtask: {e:#}");
                    ExitCode::from(4)
                }
                done => report(done),
            };
        }
        Some("sandbox") => {
            let claim = match &args[1..] {
                [] => false,
                [one] if one == "claim" => true,
                _ => {
                    eprintln!("usage: cargo xtask sandbox [claim]");
                    return ExitCode::from(4);
                }
            };
            return lines(sandbox::run(claim));
        }
        Some("stage") => return lines(stage::run()),
        Some(verb @ ("scratch" | "forge")) => {
            let made = if verb == "scratch" { scratch::run() } else { forge::run(&args[1..]) };
            return match made {
                Ok(dir) => {
                    println!("{}", dir.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("xtask: {e:#}");
                    ExitCode::from(1)
                }
            };
        }
        _ => {
            eprintln!(
                "usage: cargo xtask deps | cargo xtask lines | cargo xtask bench <runs> <plain> <label=command>... | cargo xtask scratch | cargo xtask forge <project> | cargo xtask eval <skill> --trials N | cargo xtask eval --grade [--check] <skill> | cargo xtask mutants --pr <n> --count <k> --seed <s> | cargo xtask plants --list|--verify|--from ... | cargo xtask sandbox [claim] | cargo xtask stage"
            );
            return ExitCode::from(4);
        }
    }
    report(deps(Path::new(".")))
}

/// Exit 0 with no violations, 2 with one line each on stderr, 1 when the check could not run.
/// Prints the lines a block evaluates, else the error and exit 1.
fn lines(made: Result<String>) -> ExitCode {
    match made {
        Ok(lines) => {
            print!("{lines}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("xtask: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn report(result: Result<Vec<String>>) -> ExitCode {
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
    // `$CARGO` is the cargo that runs this alias, with its toolchain; a bare `cargo` may be another one.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .output()
        .context("run cargo metadata")?;
    if !out.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).context("parse cargo metadata")?;
    Ok(rule.check(&meta, root))
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

    fn check(&self, meta: &Value, root: &Path) -> Vec<String> {
        let graph = Graph::of(meta);
        let mut violations = Vec::new();
        let rows: BTreeSet<&String> = self.edges.keys().chain(self.only.values()).collect();
        for krate in rows {
            if !graph.members.contains_key(krate.as_str()) {
                violations.push(format!("rule row with no package: {krate}"));
            }
        }
        for (name, p) in &graph.members {
            let Some(permitted) = self.edges.get(*name) else {
                violations.push(format!("crate not in the rule: {name}"));
                continue;
            };
            for d in p["dependencies"].as_array().into_iter().flatten() {
                let dep = d["name"].as_str().unwrap_or_default();
                if d["kind"].as_str().is_some_and(|k| k == "dev") || !graph.members.contains_key(dep) {
                    continue;
                }
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
            }
            // A `[features]` entry `<dep>/<feature>` or `<dep>?/<feature>` switches the feature on as surely as
            // the dependency line does.
            for enables in p["features"].as_object().into_iter().flat_map(|m| m.values()) {
                for e in enables.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    let Some((dep, f)) = e.split_once('/') else { continue };
                    let dep = dep.trim_end_matches('?');
                    if let Some(features) = permitted.get(dep)
                        && !features.contains(f)
                    {
                        violations.push(format!("feature outside the rule: {name} -> {dep}[{f}]"));
                    }
                }
            }
            // A reserved crate counts wherever it enters: directly, or through an external dependency. An entry
            // through another member is that member's violation, or an edge outside the rule. cargo's resolve
            // unions every member's features, so a path it shows is confirmed against the member's own build.
            for (dep, owner) in &self.only {
                if owner == name {
                    continue;
                }
                for direct in graph.external_deps(name) {
                    let lead = format!("external dependency reserved for {owner}: {name} -> {dep}");
                    if direct == dep {
                        violations.push(lead);
                    } else if graph.reaches(direct, dep) && builds_with(root, name, dep) {
                        violations.push(format!("{lead} through {direct}"));
                    }
                }
            }
        }
        violations
    }
}

/// Whether `member`'s own build for the host, with its own features, compiles `dep` as a normal dependency.
fn builds_with(root: &Path, member: &str, dep: &str) -> bool {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    std::process::Command::new(cargo)
        .args(["tree", "--quiet", "-p", member, "-e", "normal", "-i", dep, "--prefix", "none"])
        .current_dir(root)
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}

/// The resolved dependency graph by package name, normal edges only: dev and build dependencies put nothing in the
/// shipped binary.
struct Graph<'a> {
    members: BTreeMap<&'a str, &'a Value>,
    edges: BTreeMap<&'a str, BTreeSet<&'a str>>,
}

impl<'a> Graph<'a> {
    fn of(meta: &'a Value) -> Self {
        let packages = meta["packages"].as_array().map(Vec::as_slice).unwrap_or_default();
        let name_of: BTreeMap<&str, &str> =
            packages.iter().filter_map(|p| Some((p["id"].as_str()?, p["name"].as_str()?))).collect();
        let member_ids: BTreeSet<&str> =
            meta["workspace_members"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let members = packages
            .iter()
            .filter(|p| p["id"].as_str().is_some_and(|id| member_ids.contains(id)))
            .filter_map(|p| Some((p["name"].as_str()?, p)))
            .collect();
        let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for node in meta["resolve"]["nodes"].as_array().into_iter().flatten() {
            let Some(from) = node["id"].as_str().and_then(|id| name_of.get(id)) else { continue };
            for d in node["deps"].as_array().into_iter().flatten() {
                let normal = d["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null());
                if let Some(to) = d["pkg"].as_str().and_then(|id| name_of.get(id))
                    && normal
                {
                    edges.entry(from).or_default().insert(to);
                }
            }
        }
        Self { members, edges }
    }

    /// A member's direct dependencies that are not members.
    fn external_deps(&self, member: &str) -> impl Iterator<Item = &'a str> + '_ {
        self.edges.get(member).into_iter().flatten().copied().filter(|d| !self.members.contains_key(d))
    }

    /// Whether `to` is in the normal-dependency closure of `from`.
    fn reaches(&self, from: &str, to: &str) -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from];
        while let Some(at) = stack.pop() {
            for next in self.edges.get(at).into_iter().flatten() {
                if *next == to {
                    return true;
                }
                if seen.insert(*next) {
                    stack.push(next);
                }
            }
        }
        false
    }
}

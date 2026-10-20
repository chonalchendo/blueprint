//! The bundle text against the built verbs (27, design change 5): every file of `bundle/`, or of `BP_BUNDLE`. A
//! body template is read by the contract rules only (design change 7). `resolves` gathers every failure and fails
//! once, so each planted fault of the `skill-contract` block names its own file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bp_operation::{Action, READ_VERBS, Role};
use bp_vault::Code;
use clap::{CommandFactory, Parser};

/// The five slots of a dispatch template, in 17's order (Interfaces 3).
const SLOTS: [&str; 5] = ["Node:", "Skill:", "Budget:", "Forbidden:", "Stop:"];

/// A skill body's cap in tokens, counted as bytes / 4 (17, Skill set).
const TOKENS: usize = 5_000;

fn root() -> PathBuf {
    std::env::var_os("BP_BUNDLE")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bundle"), PathBuf::from)
}

/// Every file under `dir`, as a path relative to `base` with `/`, and its text. A dot file is the host's.
fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk(base, &path, out);
        } else {
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            out.insert(rel, String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned());
        }
    }
}

fn ext(path: &str, want: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case(want))
}

/// The verbs the binary parses; clap's own `help` is not one.
fn verbs() -> BTreeSet<String> {
    bp_tool::Cli::command().get_subcommands().map(|c| c.get_name().to_string()).filter(|v| v != "help").collect()
}

/// The names a body may backtick: each code, and each field a result or a record can carry. No list of result
/// fields exists apart from the code that builds them, so the lexicon is each snake-case string literal and field
/// name in the workspace's sources: a name in none of them is a typo or a field that no verb returns.
fn lexicon() -> BTreeSet<String> {
    let mut sources = BTreeMap::new();
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    walk(&crates, &crates, &mut sources);
    let mut names: BTreeSet<String> = Code::ALL.iter().map(|c| c.as_str().to_string()).collect();
    for text in sources.iter().filter(|(p, _)| p.contains("/src/") && ext(p, "rs")).map(|(_, t)| t) {
        names.extend(text.split('"').skip(1).step_by(2).filter(|s| snake(s)).map(str::to_string));
        for line in text.lines() {
            let field = line.trim_start().trim_start_matches("pub(crate) ").trim_start_matches("pub ");
            if let Some((name, _)) = field.split_once(": ").filter(|(n, _)| snake(n)) {
                names.insert(name.to_string());
            }
        }
    }
    names
}

/// A snake-case name: lower-case words joined by `_`, at least two.
fn snake(s: &str) -> bool {
    s.contains('_')
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && !s.ends_with('_')
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A shell line's words, with `'` and `"` quoting as the shell reads them.
fn words(line: &str) -> Vec<String> {
    let (mut out, mut word, mut quote, mut open) = (Vec::new(), String::new(), None, false);
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => (quote, open) = (Some(c), true),
            (None, c) if c.is_whitespace() => {
                if open || !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
                open = false;
            }
            (_, c) => word.push(c),
        }
    }
    if open || !word.is_empty() {
        out.push(word);
    }
    out
}

/// The contract rules, on any markdown file: each `bp …` span or fenced line parses in the real clap parser, each
/// backticked snake-case name is a code or a field, and each `exit <n>` is 0 to 4 (15, the exit table).
fn contract(path: &str, text: &str, names: &BTreeSet<String>, errors: &mut Vec<String>) {
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let spans: Vec<&str> = if fenced { vec![line.trim()] } else { line.split('`').skip(1).step_by(2).collect() };
        for span in spans {
            if span.starts_with("bp ")
                && let Err(e) = bp_tool::Cli::try_parse_from(words(span))
            {
                errors.push(format!("{path}: `{span}` does not parse: {}", e.kind()));
            } else if snake(span) && !names.contains(span) {
                errors.push(format!("{path}: `{span}` is no code and no result field"));
            }
        }
        let lower = line.to_lowercase();
        for (at, _) in lower.match_indices("exit ") {
            let n: String = lower[at + 5..].chars().take_while(char::is_ascii_digit).collect();
            if n.parse::<u32>().is_ok_and(|n| n > 4) {
                errors.push(format!("{path}: exit {n} is outside the contract's 0 to 4"));
            }
        }
    }
}

/// A frontmatter's `key: value` lines; a `- item` line extends the key above it, as `a, b`.
fn front(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(yaml) = text.strip_prefix("---\n").and_then(|r| r.split_once("\n---").map(|(y, _)| y)) else {
        return map;
    };
    let mut last = String::new();
    for line in yaml.lines() {
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            let v: &mut String = map.entry(last.clone()).or_default();
            *v = if v.is_empty() { item.trim().to_string() } else { format!("{v}, {}", item.trim()) };
        } else if let Some((k, v)) = line.split_once(':') {
            last = k.trim().to_string();
            map.insert(last.clone(), v.trim().to_string());
        }
    }
    map
}

fn list(value: &str) -> BTreeSet<String> {
    value
        .split([',', ' '])
        .map(|w| w.trim_matches(|c: char| c == '`' || c == '.'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// A skill: its name is its directory, it fits the cap, its `Stop:` line calls its action's stop, and each
/// reference sits one level under `references/` and is named in the body.
fn skill(dir: &str, files: &BTreeMap<String, String>, errors: &mut Vec<String>) {
    let path = format!("skills/{dir}/SKILL.md");
    let Some(text) = files.get(&path) else { return errors.push(format!("skills/{dir}: no SKILL.md")) };
    let f = front(text);
    if f.get("name").map(String::as_str) != Some(dir) || f.get("description").is_none_or(String::is_empty) {
        errors.push(format!("{path}: the name is {dir} and the description says when it fires"));
    }
    if text.len() / 4 >= TOKENS {
        errors.push(format!("{path}: {} tokens; the body is under {TOKENS}", text.len() / 4));
    }
    let action = dir.strip_prefix("bp-").unwrap_or(dir);
    if action.parse::<Action>().is_err() {
        errors.push(format!("{path}: bp-<action>, and {action} is no action of 21"));
    }
    let stop = format!("--for {action} --done");
    if !text.lines().any(|l| l.starts_with("Stop:") && l.contains("`bp walk ") && l.contains(&stop)) {
        errors.push(format!("{path}: a Stop: line calls `bp walk <id> {stop}`"));
    }
    for rest in files.keys().filter_map(|p| p.strip_prefix(&format!("skills/{dir}/"))) {
        match rest.split_once('/') {
            _ if rest == "SKILL.md" => {}
            Some(("references", file)) if !file.contains('/') => {
                if !text.contains(&format!("references/{file}")) {
                    errors.push(format!("skills/{dir}/{rest}: the body names each reference"));
                }
            }
            Some(("scripts", _)) => {}
            _ => errors.push(format!("skills/{dir}/{rest}: references/ is one level deep, and nothing else ships")),
        }
    }
}

/// A role: `bp-<role>` of a role an agent runs, its preloads in the bundle, and its stop as the last paragraph.
fn role(stem: &str, text: &str, skills: &BTreeSet<&str>, errors: &mut Vec<String>) -> BTreeSet<String> {
    let path = format!("roles/{stem}.md");
    let f = front(text);
    let named = stem.strip_prefix("bp-").and_then(|r| r.parse::<Role>().ok()).filter(|r| *r != Role::Human);
    if named.is_none() || f.get("name").map(String::as_str) != Some(stem) {
        errors.push(format!("{path}: the name is bp-<role> of a dispatched role, and equals the file name"));
    }
    let preloads = f.get("skills").map(|s| list(s)).unwrap_or_default();
    for s in preloads.iter().filter(|s| !skills.contains(s.as_str())) {
        errors.push(format!("{path}: preloads {s}, which the bundle lacks"));
    }
    let last = text.trim_end().rsplit("\n\n").next().unwrap_or_default();
    if !last.starts_with("Stop") {
        errors.push(format!("{path}: the last paragraph is the stop condition, `Stop: …`"));
    }
    preloads
}

/// A dispatch template: the five slots, a skill its role preloads, its action's budget, and the complement of its
/// role's verbs as the forbidden list (21, rule 6).
fn template(path: &str, text: &str, preloads: Option<&BTreeSet<String>>, errors: &mut Vec<String>) {
    let (role, action) = path.strip_prefix("roles/").and_then(|p| p.strip_suffix(".md")?.split_once('/')).unwrap();
    let slot = |s: &str| -> Vec<&str> { text.lines().filter_map(|l| l.strip_prefix(s)).map(str::trim).collect() };
    for s in SLOTS {
        if slot(s).len() != 1 {
            errors.push(format!("{path}: one `{s}` line"));
        }
    }
    let skill = slot("Skill:").first().copied().unwrap_or_default();
    if !skill.is_empty() && !preloads.is_some_and(|p| p.contains(skill)) {
        errors.push(format!("{path}: Skill: {skill}, which roles/{role}.md does not preload"));
    }
    let budget = slot("Budget:").first().map(|b| b.replace(',', "")).unwrap_or_default();
    let want = action.parse::<Action>().map(Action::budget);
    if want.is_err() || budget.split_whitespace().next().and_then(|b| b.parse().ok()) != want.ok() {
        errors.push(format!("{path}: Budget: the {action} row's budget (21, rule 6)"));
    }
    let Some(own) = role.strip_prefix("bp-").and_then(|r| r.parse::<Role>().ok()).and_then(Role::verbs) else {
        return errors.push(format!("{path}: {role} is no dispatched role"));
    };
    let forbidden: BTreeSet<String> =
        verbs().into_iter().filter(|v| !READ_VERBS.contains(&v.as_str()) && !own.contains(&v.as_str())).collect();
    if slot("Forbidden:").first().is_some_and(|f| list(f) != forbidden) {
        let want: Vec<_> = forbidden.into_iter().collect();
        errors.push(format!("{path}: Forbidden: {}", want.join(", ")));
    }
}

#[test]
fn resolves() {
    let base = root();
    let mut files = BTreeMap::new();
    walk(&base, &base, &mut files);
    let names = lexicon();
    let mut errors = Vec::new();
    for (path, text) in files.iter().filter(|(p, _)| ext(p, "md")) {
        contract(path, text, &names, &mut errors);
    }
    let skills: BTreeSet<&str> =
        files.keys().filter_map(|p| p.strip_prefix("skills/")?.split_once('/').map(|(d, _)| d)).collect();
    for dir in &skills {
        skill(dir, &files, &mut errors);
    }
    let mut preloads = BTreeMap::new();
    for (stem, text) in files.iter().filter_map(|(p, t)| Some((p.strip_prefix("roles/")?.strip_suffix(".md")?, t))) {
        if !stem.contains('/') {
            preloads.insert(stem.to_string(), role(stem, text, &skills, &mut errors));
        }
    }
    for (path, text) in &files {
        let parts: Vec<&str> = path.split('/').collect();
        match parts.as_slice() {
            ["roles", r, _] if ext(path, "md") => template(path, text, preloads.get(*r), &mut errors),
            ["skills", ..] | ["roles" | "hooks" | "templates", _] | ["always-on.md"] => {}
            _ => errors.push(format!("{path}: no bundle part holds this file (17)")),
        }
    }
    assert!(errors.is_empty(), "{} bundle rule(s) fail:\n{}", errors.len(), errors.join("\n"));
}

/// The binary's verbs, flags and codes are in `analysis/contract.json`, at `BP_CONTRACT`. CI cannot read the design
/// repository, so a block runs it.
#[test]
#[ignore = "reads analysis/contract.json from the design repository; the skill-contract block runs it"]
fn contract_parity() {
    let path = std::env::var_os("BP_CONTRACT").expect("BP_CONTRACT names analysis/contract.json");
    let contract: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut missing = Vec::new();
    let mut cli = bp_tool::Cli::command();
    cli.build();
    for verb in cli.get_subcommands().filter(|c| c.get_name() != "help") {
        let flags = &contract["verbs"][verb.get_name()]["flags"];
        if flags.is_null() {
            missing.push(format!("verb {}", verb.get_name()));
            continue;
        }
        for long in verb.get_arguments().filter_map(clap::Arg::get_long).filter(|l| !["help", "version"].contains(l)) {
            if !flags.as_array().unwrap().iter().any(|f| f == &format!("--{long}")) {
                missing.push(format!("{} --{long}", verb.get_name()));
            }
        }
    }
    missing.extend(Code::ALL.iter().map(|c| c.as_str()).filter(|c| contract["codes"][c].is_null()).map(String::from));
    assert!(missing.is_empty(), "absent from contract.json: {}", missing.join(", "));
}

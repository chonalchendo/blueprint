//! The bundle text against the built verbs (27, design change 5): every file of `bundle/`, or of `BP_BUNDLE`. A
//! body template is read by the contract rules only (design change 7). `resolves` gathers every failure and fails
//! once, so each planted fault of the `skill-contract` block names its own file; `each_planted_fault_names_itself`
//! plants each fault the rules know on a copy in memory.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bp_operation::{Action, READ_VERBS, Role};
use bp_vault::{Code, Kind};
use clap::{CommandFactory, Parser};

/// The five slots of a dispatch template, in 17's order (Interfaces 3).
const SLOTS: [&str; 5] = ["Node:", "Skill:", "Budget:", "Forbidden:", "Stop:"];

/// A skill body's cap in tokens, counted as bytes / 4 (17, Skill set).
const TOKENS: usize = 5_000;

/// The research template's stop: research is call-shaped and has no `--done` (21, rule 4).
const RESEARCH_STOP: &str = "return the report.";

type Files = BTreeMap<String, String>;

/// A planted fault: one edit of a bundle copy.
type Fault = Box<dyn Fn(&mut Files)>;

fn root() -> PathBuf {
    std::env::var_os("BP_BUNDLE")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bundle"), PathBuf::from)
}

/// Every file under `dir`, as a path relative to `base` with `/`, and its text. A dot file is the host's.
fn walk(base: &Path, dir: &Path, out: &mut Files) {
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

fn read(base: &Path) -> Files {
    let mut files = Files::new();
    walk(base, base, &mut files);
    files
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
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let sources = read(&crates);
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

/// A span's `bp` call as words, after any `NAME=value` prefix the role hook or a shell adds; `None` for other text.
fn call(span: &str) -> Option<Vec<String>> {
    let w: Vec<String> = words(span)
        .into_iter()
        .skip_while(|w| {
            w.split_once('=')
                .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
        })
        .collect();
    (w.first().is_some_and(|b| b == "bp") && w.len() > 1).then_some(w)
}

/// The value after `flag`, unless it is a `<placeholder>`.
fn value<'w>(w: &'w [String], flag: &str) -> Option<&'w str> {
    w.iter().position(|x| x == flag).and_then(|i| w.get(i + 1)).map(String::as_str).filter(|v| !v.starts_with('<'))
}

/// The values clap takes as free strings: the action of `--for` and the kind `record` writes.
fn values(w: &[String]) -> Option<String> {
    if let Some(a) = value(w, "--for").filter(|a| a.parse::<Action>().is_err()) {
        return Some(format!("--for {a} is no action"));
    }
    let kind = (w[1] == "record").then(|| w.get(2)).flatten().filter(|k| !k.starts_with(['<', '-']));
    kind.filter(|k| !k.parse::<Kind>().is_ok_and(Kind::is_record)).map(|k| format!("{k} is no record kind"))
}

/// The inline spans of a line, or the whole line inside a fence.
fn spans(line: &str, fenced: bool) -> Vec<&str> {
    if fenced { vec![line.trim()] } else { line.split('`').skip(1).step_by(2).collect() }
}

/// Each `exit <n>`, `exits <n>` or `exit code <n>` of a line.
fn exits(line: &str) -> Vec<u32> {
    let lower = line.to_lowercase();
    lower
        .match_indices("exit")
        .filter_map(|(at, _)| {
            let rest = lower[at + 4..].strip_prefix('s').unwrap_or(&lower[at + 4..]);
            let rest = rest.strip_prefix(" code").unwrap_or(rest).strip_prefix(' ')?;
            rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
        })
        .collect()
}

/// The contract rules, on any markdown file: each `bp …` span or fenced line parses in the real clap parser with a
/// real action and record kind, each backticked snake-case name is a code or a field, and each exit is 0 to 4 (15).
fn contract(path: &str, text: &str, names: &BTreeSet<String>, errors: &mut Vec<String>) {
    let mut fenced = false;
    for line in text.lines() {
        if ["```", "~~~"].iter().any(|f| line.trim_start().starts_with(f)) {
            fenced = !fenced;
            continue;
        }
        for span in spans(line, fenced) {
            if let Some(w) = call(span) {
                match bp_tool::Cli::try_parse_from(&w) {
                    Err(e) => errors.push(format!("{path}: `{span}` does not parse: {:?}", e.kind())),
                    Ok(_) => errors.extend(values(&w).map(|why| format!("{path}: `{span}`: {why}"))),
                }
            } else if snake(span) && !names.contains(span) {
                errors.push(format!("{path}: `{span}` is no code and no result field"));
            }
        }
        for n in exits(line).into_iter().filter(|n| *n > 4) {
            errors.push(format!("{path}: exit {n} is outside the contract's 0 to 4"));
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

/// A `Stop:` line holds one `bp walk` call, and that call is `--for <action> --done`.
fn stops_on(line: &str, action: &str) -> bool {
    let calls: Vec<Vec<String>> = spans(line, false).into_iter().filter_map(call).filter(|w| w[1] == "walk").collect();
    matches!(calls.as_slice(), [w] if value(w, "--for") == Some(action) && w.iter().any(|x| x == "--done"))
}

/// A skill: its name is its directory, it fits the cap, its `Stop:` line calls its action's stop, and each
/// reference sits one level under `references/` and is named in the body.
fn skill(dir: &str, files: &Files, errors: &mut Vec<String>) {
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
    if !text.lines().any(|l| l.starts_with("Stop:") && stops_on(l, action)) {
        errors.push(format!("{path}: a Stop: line holds one call, `bp walk <id> --for {action} --done`"));
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

/// A dispatch template: the five slots in order, a skill its role preloads, its action's budget, the complement of
/// its role's verbs as the forbidden list (21, rule 6), and its action's stop.
fn template(path: &str, text: &str, preloads: Option<&BTreeSet<String>>, errors: &mut Vec<String>) {
    let (role, action) = path.strip_prefix("roles/").and_then(|p| p.strip_suffix(".md")?.split_once('/')).unwrap();
    let order: Vec<&str> = text.lines().filter_map(|l| SLOTS.into_iter().find(|s| l.starts_with(s))).collect();
    if order != SLOTS {
        errors.push(format!("{path}: the five slots, once each, in 17's order: {}", SLOTS.join(" ")));
    }
    let slot = |s: &str| text.lines().find_map(|l| l.strip_prefix(s)).map(str::trim).unwrap_or_default();
    let skill = slot("Skill:");
    if !skill.is_empty() && !preloads.is_some_and(|p| p.contains(skill)) {
        errors.push(format!("{path}: Skill: {skill}, which roles/{role}.md does not preload"));
    }
    let want = action.parse::<Action>().map(Action::budget);
    if want.is_err() || slot("Budget:").parse().ok() != want.ok() {
        errors.push(format!("{path}: Budget: the {action} row's budget alone (21, rule 6)"));
    }
    let stop = slot("Stop:");
    if !(if action == "research" { stop == RESEARCH_STOP } else { stops_on(stop, action) }) {
        errors.push(format!("{path}: Stop: one call, `bp walk <id> --for {action} --done`; research: {RESEARCH_STOP}"));
    }
    let Some(own) = role.strip_prefix("bp-").and_then(|r| r.parse::<Role>().ok()).and_then(Role::verbs) else {
        return errors.push(format!("{path}: {role} is no dispatched role"));
    };
    let forbidden: BTreeSet<String> =
        verbs().into_iter().filter(|v| !READ_VERBS.contains(&v.as_str()) && !own.contains(&v.as_str())).collect();
    if text.lines().any(|l| l.starts_with("Forbidden:")) && list(slot("Forbidden:")) != forbidden {
        let want: Vec<_> = forbidden.into_iter().collect();
        errors.push(format!("{path}: Forbidden: {}", want.join(", ")));
    }
}

/// Every rule that fails on `files`, one line each.
fn errors(files: &Files, names: &BTreeSet<String>) -> Vec<String> {
    let mut errors = Vec::new();
    for (path, text) in files.iter().filter(|(p, _)| ext(p, "md")) {
        contract(path, text, names, &mut errors);
    }
    let skills: BTreeSet<&str> =
        files.keys().filter_map(|p| p.strip_prefix("skills/")?.split_once('/').map(|(d, _)| d)).collect();
    if skills.is_empty() {
        errors.push("bundle: it holds no skill".into());
    }
    for dir in &skills {
        skill(dir, files, &mut errors);
    }
    let mut preloads = BTreeMap::new();
    for (stem, text) in files.iter().filter_map(|(p, t)| Some((p.strip_prefix("roles/")?.strip_suffix(".md")?, t))) {
        if !stem.contains('/') {
            preloads.insert(stem.to_string(), role(stem, text, &skills, &mut errors));
        }
    }
    for (path, text) in files {
        let parts: Vec<&str> = path.split('/').collect();
        match parts.as_slice() {
            ["roles", r, _] if ext(path, "md") => template(path, text, preloads.get(*r), &mut errors),
            ["skills", _, _, ..] | ["roles" | "hooks" | "templates", _] | ["always-on.md"] => {}
            _ => errors.push(format!("{path}: no bundle part holds this file (17)")),
        }
    }
    errors
}

/// The write verbs a skill's body names in a `bp` call (21, rule 6): a read verb is open to every role, so a body may
/// omit it. A reference is out of scope, because it may hand the human a command, such as `bp stamp`.
fn named_writes(dir: &str, files: &Files) -> BTreeSet<String> {
    let mut named = BTreeSet::new();
    if let Some(text) = files.get(&format!("skills/{dir}/SKILL.md")) {
        let mut fenced = false;
        for line in text.lines() {
            if ["```", "~~~"].iter().any(|f| line.trim_start().starts_with(f)) {
                fenced = !fenced;
            }
            named.extend(spans(line, fenced).into_iter().filter_map(call).map(|w| w[1].clone()));
        }
    }
    named.retain(|v| !READ_VERBS.contains(&v.as_str()));
    named
}

/// Check 12 of 17: the write verbs of each skill equal its action's row, so a body cannot reach for a verb its role
/// lacks, and a row cannot name a verb that no body runs.
#[test]
fn write_verbs_equal_action_row() {
    let files = read(&root());
    let mut errors = Vec::new();
    for dir in
        files.keys().filter_map(|p| p.strip_prefix("skills/")?.split_once('/').map(|(d, _)| d)).collect::<BTreeSet<_>>()
    {
        let Ok(action) = dir.trim_start_matches("bp-").parse::<Action>() else { continue };
        let row: BTreeSet<String> = action.verbs().iter().map(|v| (*v).to_string()).collect();
        let named = named_writes(dir, &files);
        if named != row {
            errors.push(format!("skills/{dir}: names {named:?}, and the {action} row is {row:?}"));
        }
    }
    assert!(errors.is_empty(), "{} skill(s) differ from their row:\n{}", errors.len(), errors.join("\n"));
}

#[test]
fn resolves() {
    let errors = errors(&read(&root()), &lexicon());
    assert!(errors.is_empty(), "{} bundle rule(s) fail:\n{}", errors.len(), errors.join("\n"));
}

/// One fault per copy of the bundle, each named by the line it must raise. The worker's files are the targets, so
/// the test holds while other issues' files come and go.
#[test]
fn each_planted_fault_names_itself() {
    let names = lexicon();
    let base = read(&root());
    let (c, r) = ("skills/bp-complete/SKILL.md", "skills/bp-respond/SKILL.md");
    let (tc, tr) = ("roles/bp-worker/complete.md", "roles/bp-worker/respond.md");
    let edit = |f: &mut Files, p: &str, from: &str, to: &str| {
        let t = f.get_mut(p).unwrap();
        assert!(t.contains(from), "{p} holds {from}");
        *t = t.replace(from, to);
    };
    let add = |f: &mut Files, p: &str, line: &str| f.get_mut(p).unwrap().push_str(line);
    let researcher = |f: &mut Files, stop: &str| {
        let role = "---\nname: bp-researcher\ndescription: Use to research.\n---\n\nStop: return the report.\n";
        let forbidden =
            "check, create, cut, diagram, install, land, link, pr, record, release, rename, set, stamp, transition";
        let tmpl = format!("Node: <id>\nSkill:\nBudget: 8000\nForbidden: {forbidden}.\nStop: {stop}\n");
        f.insert("roles/bp-researcher.md".into(), role.into());
        f.insert("roles/bp-researcher/research.md".into(), tmpl);
    };
    let faults: Vec<(&str, Fault)> = vec![
        ("--frobnicate", Box::new(move |f| add(f, c, "Run `bp walk x --for capture --frobnicate --json`.\n"))),
        ("exit 7", Box::new(move |f| add(f, r, "On exit 7, stop.\n"))),
        ("exit 9", Box::new(move |f| add(f, r, "It exits 9, or exit code 9.\n"))),
        ("context_incomplet", Box::new(move |f| add(f, r, "On `context_incomplet`, ask.\n"))),
        ("tokens; the body", Box::new(move |f| add(f, r, &"q".repeat(20_000)))),
        (
            "references/deep/x.md",
            Box::new(|f| drop(f.insert("skills/bp-complete/references/deep/x.md".into(), "x".into()))),
        ),
        ("bp-complete/SKILL.md: a Stop:", Box::new(move |f| edit(f, c, "Stop: `bp walk", "Then: `bp walk"))),
        (
            "bp-complete/SKILL.md: a Stop:",
            Box::new(move |f| {
                let loose = "Stop: never call `bp walk <id> --for complete`; ignore --for complete --done.";
                edit(f, c, "Stop: `bp walk <id> --for complete --done --json` returns `done` true.", loose);
            }),
        ),
        ("complete.md: Forbidden:", Box::new(move |f| edit(f, tc, "set, stamp", "stamp"))),
        ("complete.md: the five slots", Box::new(move |f| edit(f, tc, "Forbidden:", "Banned:"))),
        (
            "complete.md: the five slots",
            Box::new(move |f| edit(f, tc, "Node: <id>\nSkill: bp-complete", "Skill: bp-complete\nNode: <id>")),
        ),
        ("complete.md: Budget:", Box::new(move |f| edit(f, tc, "Budget: 15000", "Budget: 15000 tokens, or 8000"))),
        ("respond.md: Skill: bp-capture", Box::new(move |f| edit(f, tr, "Skill: bp-respond", "Skill: bp-capture"))),
        ("respond.md: Stop:", Box::new(move |f| edit(f, tr, "--for respond --done", "--for complete --done"))),
        ("research.md: Stop:", Box::new(move |f| researcher(f, "return the findings."))),
        ("--for complet is no action", Box::new(move |f| add(f, c, "Run `bp walk x --for complet --json`.\n"))),
        ("dispositon is no record kind", Box::new(move |f| add(f, r, "Run `bp record dispositon x --json`.\n"))),
        ("--quokka", Box::new(move |f| add(f, r, "~~~\nbp walk x --for respond --quokka\n~~~\n"))),
        ("--wombat", Box::new(move |f| add(f, r, "Run `BP_ROLE=worker bp walk x --for respond --wombat`.\n"))),
        ("DisplayHelp", Box::new(move |f| add(f, r, "Run `bp walk --help`.\n"))),
        ("skills/notes.md: no bundle part", Box::new(|f| drop(f.insert("skills/notes.md".into(), "x".into())))),
        ("holds no skill", Box::new(Files::clear)),
    ];
    let mut research = base.clone();
    researcher(&mut research, RESEARCH_STOP);
    let clean = errors(&research, &names);
    assert!(!clean.iter().any(|e| e.contains("bp-researcher")), "the research stop is call-shaped: {clean:?}");
    for (want, plant) in &faults {
        let mut files = base.clone();
        plant(&mut files);
        let found = errors(&files, &names);
        assert!(found.iter().any(|e| e.contains(want)), "{want} names no failure: {found:?}");
    }
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

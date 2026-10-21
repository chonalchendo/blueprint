//! The `nodes` kind (12, check 3; 28, design change 6): each node of each diagram spec under the owner design names a
//! slug of its project's glossary, or a noun the design names in a heading or a table's first column. A `start`,
//! `end`, `choice` or `decision` node may name neither (28, design change 1). Like
//! `integrity`, it reads vault state in process, so its run names the vault's head. `bp diagram` resolves a node
//! with the same [`Names`] before it writes a spec.

use std::collections::BTreeSet;

use bp_diagram::{Finding, Link};
use bp_vault::{Id, Kind, Vault};

use crate::Verdict;
use crate::integrity::{Ran, evidence, plural};

/// What a diagram node of one design may name.
pub struct Names {
    /// None when the project has no glossary design: then no term resolves.
    terms: Option<BTreeSet<String>>,
    nouns: BTreeSet<String>,
    project: String,
}

impl Names {
    /// The names of `design`, whose body is `body`: its project's glossary slugs and its own nouns. A project with no
    /// glossary design gives no term, so every `term` fails until the glossary exists.
    ///
    /// # Panics
    ///
    /// Never: a design's id names its project.
    pub fn of(vault: &Vault, design: &Id, body: &str) -> Self {
        let project = design.project().expect("a design sits in a project");
        let glossary = std::fs::read_to_string(vault.root.join(format!("projects/{project}/designs/glossary.md")));
        Self::with(glossary.ok().as_deref(), design, body)
    }

    /// [`Names::of`] with the glossary's text given: a delta's diagrams resolve against the glossary its milestone
    /// proposes (28, design change 7).
    ///
    /// # Panics
    ///
    /// Never: a design's id names its project.
    pub fn with(glossary: Option<&str>, design: &Id, body: &str) -> Self {
        Self {
            terms: glossary.map(|g| terms(g).map(str::to_string).collect()),
            nouns: nouns(body),
            project: design.project().expect("a design sits in a project").to_string(),
        }
    }

    /// Adds to each term finding why no term resolves when the project has no glossary design, and how to make it.
    pub fn explain(&self, findings: &mut [Finding]) {
        if self.terms.is_some() {
            return;
        }
        let p = &self.project;
        let why = format!(
            "projects/{p}/designs/glossary does not exist; create it: bp create design --project {p} --title Glossary \
             --slug glossary --under projects/{p}/designs/system --body-file <f>"
        );
        for f in findings.iter_mut().filter(|f| f.rule == "nodes/resolve" && f.detail.starts_with("term `")) {
            f.detail = format!("{}: {why}", f.detail);
        }
    }

    /// Whether `link` resolves: a term exactly, a noun ignoring case and markdown marks.
    pub fn resolves(&self, link: Link<'_>) -> bool {
        match link {
            Link::Term(t) => self.terms.as_ref().is_some_and(|known| known.contains(t)),
            Link::Noun(n) => self.nouns.contains(&key(n)),
        }
    }
}

/// The slugs a glossary defines, in 03's form: one `` - `slug` — `` line per term.
pub fn terms(glossary: &str) -> impl Iterator<Item = &str> {
    glossary.lines().filter_map(|line| Some(line.trim_start().strip_prefix("- `")?.split_once('`')?.0))
}

/// Whether `name` names a diagram: a slug with no dot, so `<name>.yaml` never reads as a converted
/// `<name>.nodes.yaml`.
pub fn is_name(name: &str) -> bool {
    bp_vault::is_slug(name) && !name.contains('.')
}

/// Each diagram spec under `design`, as its name and text, by name: `<design>/diagrams/<name>.yaml`. Then every other
/// entry there that is not `<name>.html` nor a converted `<name>.nodes.yaml`, which the spec replaces: a file `bp`
/// cannot have written, so the check fails on it rather than skip a spec it would not read.
pub fn specs(vault: &Vault, design: &Id) -> (Vec<(String, String)>, Vec<String>) {
    let Ok(dir) = std::fs::read_dir(vault.root.join(design.as_str()).join("diagrams")) else {
        return (Vec::new(), Vec::new());
    };
    let (mut specs, mut strays) = (Vec::new(), Vec::new());
    for path in dir.filter_map(|e| Some(e.ok()?.path())) {
        let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let spec = file.strip_suffix(".yaml").filter(|n| is_name(n) && path.is_file());
        match spec.map(|n| (n, std::fs::read_to_string(&path))) {
            Some((name, Ok(text))) => specs.push((name.to_string(), text)),
            None if file.strip_suffix(".html").is_some_and(is_name) || file.ends_with(".nodes.yaml") => {}
            _ => strays.push(file),
        }
    }
    specs.sort();
    strays.sort();
    (specs, strays)
}

/// Runs the rule over the owner's specs. `Err` is why it could not run.
pub(crate) fn run(vault: &Vault, owner: &Id) -> Result<Ran, String> {
    let file = vault.read(owner).map_err(|r| format!("{}: {}", r.rule, r.fix))?;
    if file.header.as_ref().map(|h| h.kind) != Some(Kind::Design) {
        return Err(format!("check nodes reads a design's diagrams, and {owner} is no design"));
    }
    let commit = bp_vault::git::Repo::find(&vault.root)
        .and_then(|r| r.head())
        .ok_or_else(|| format!("the vault at {} has no commit for the run to name", vault.root.display()))?;
    let names = Names::of(vault, owner, &file.body.text);
    let (specs, strays) = specs(vault, owner);
    let mut lines: Vec<String> = strays
        .iter()
        .map(|f| {
            format!("nodes/name: diagrams/{f}: not a diagram spec; bp diagram writes only <name>.yaml and <name>.html")
        })
        .collect();
    let mut bad = strays.len();
    for (name, text) in &specs {
        let mut findings = bp_diagram::outline(text, &|l| names.resolves(l)).map_or_else(|f| f, |(_, f)| f);
        names.explain(&mut findings);
        bad += usize::from(!findings.is_empty());
        lines.extend(findings.iter().map(|f| format!("{}: {name}#{}: {}", f.rule, f.id, f.detail)));
    }
    let (verdict, note) = match (specs.len(), lines.len()) {
        (0, 0) => (Verdict::Pass, format!("{owner} has no diagram spec")),
        (n, 0) => (Verdict::Pass, format!("every node of {} of {owner} resolves", plural(n, "diagram spec"))),
        (n, k) => {
            let files = plural(n + strays.len(), "diagram file");
            (Verdict::Fail, format!("{} in {bad} of {files} of {owner}", plural(k, "finding")))
        }
    };
    let (tail, dropped, evidence) = evidence(&lines);
    Ok(Ran { verdict, note, commit, tail, dropped, evidence, names: Vec::new() })
}

/// A noun as it compares: without markdown marks or surrounding space, in lower case.
fn key(text: &str) -> String {
    text.replace(['*', '`'], "").trim().to_lowercase()
}

/// A heading without its leading number: `3.`, `3.1` or `3.1.`, then a space.
fn unnumbered(heading: &str) -> &str {
    let text = heading.trim_start();
    let end = text.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(text.len());
    let (number, rest) = text.split_at(end);
    let numbered = number.starts_with(|c: char| c.is_ascii_digit()) && !number.contains("..");
    if numbered && rest.starts_with(' ') { rest } else { text }
}

/// The nouns a design body names: each ATX heading, and the first cell of each table row below the header row.
/// Fenced code names none, so a check block's comment is no heading.
pub fn nouns(body: &str) -> BTreeSet<String> {
    let lines: Vec<&str> = body.lines().map(str::trim_start).collect();
    let rule = |l: &str| l.starts_with('|') && l.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '));
    let mut out = BTreeSet::new();
    let mut fence: Option<String> = None;
    for (i, line) in lines.iter().enumerate() {
        let run: String = line.chars().take_while(|c| matches!(c, '`' | '~')).collect();
        if run.len() >= 3 && run.chars().all(|c| c == run.chars().next().unwrap_or('`')) {
            // A fence closes on a run of its own mark at least as long as the one that opened it.
            match &fence {
                None => fence = Some(run),
                Some(open) if run.starts_with(open.as_str()) && line.trim_end() == run => fence = None,
                Some(_) => {}
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
            out.insert(key(unnumbered(line[hashes..].trim_end_matches('#'))));
        } else if line.starts_with('|') && !rule(line) && !lines.get(i + 1).is_some_and(|n| rule(n)) {
            out.extend(line[1..].split('|').next().map(key).filter(|k| !k.is_empty()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_resolve_nouns_from_headings_and_first_cells() {
        let body = "Lead.\n\n## The **Tool**\n\n| Container | Role |\n|---|---|\n| `Human view` | reads |\n| Vault | \
                    files |\n\n```check shell {name=x}\n# Not a heading\n| Not | a row |\n```\n\n~~~~\n```\n# Inside\n~~~~\n";
        let got = nouns(body);
        let want: BTreeSet<String> = ["the tool", "human view", "vault"].into_iter().map(String::from).collect();
        assert_eq!(got, want, "the header row, code and the rule row name nothing");
        let glossary = Some(terms("# Glossary\n\n- `vault` — the files.\n").map(String::from).collect());
        let names = Names { terms: glossary, nouns: got, project: "bp".into() };
        assert!(names.resolves(Link::Term("vault")) && !names.resolves(Link::Term("Vault")), "a term is a slug");
        assert!(names.resolves(Link::Noun("Human View")) && names.resolves(Link::Noun("**Vault**")));
        assert!(!names.resolves(Link::Noun("Container")) && !names.resolves(Link::Noun("tool")));
    }

    #[test]
    fn nodes_drop_a_heading_number() {
        let got =
            nouns("## 3. Vault\n\n### 3.1 Index\n\n### 3.1. Write path\n\n## 2026 to 2027\n\n## 3..4 Odd\n\n## 7.x\n");
        let want: BTreeSet<String> =
            ["vault", "index", "write path", "to 2027", "3..4 odd", "7.x"].into_iter().map(String::from).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn nodes_say_when_the_glossary_is_missing() {
        let names = Names { terms: None, nouns: BTreeSet::new(), project: "bp".into() };
        assert!(!names.resolves(Link::Term("vault")));
        let spec = "type: architecture\nlevel: container\ntitle: T\nlede: L.\nnodes:\n  - {id: v, term: vault, at: [0, 0]}\n  - {id: n, noun: N, at: [1, 0]}\n";
        let (_, mut findings) = bp_diagram::outline(spec, &|l| names.resolves(l)).unwrap();
        names.explain(&mut findings);
        assert!(findings[0].detail.contains("projects/bp/designs/glossary does not exist; create it"), "{findings:?}");
        assert!(!findings[1].detail.contains("glossary"), "a noun finding stays: {findings:?}");
    }
}

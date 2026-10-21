//! The `nodes` kind (12, check 3; 28, design change 6): each node of each diagram spec under the owner design names a
//! slug of its project's glossary, or a noun the design names in a heading or a table's first column. Like
//! `integrity`, it reads vault state in process, so its run names the vault's head. `bp diagram` resolves a node
//! with the same [`Names`] before it writes a spec.

use std::collections::BTreeSet;

use bp_diagram::Link;
use bp_vault::{Id, Kind, Vault};

use crate::Verdict;
use crate::integrity::{Ran, evidence, plural};

/// What a diagram node of one design may name.
pub struct Names {
    terms: BTreeSet<String>,
    nouns: BTreeSet<String>,
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
        Self {
            terms: glossary.map(|g| terms(&g).map(str::to_string).collect()).unwrap_or_default(),
            nouns: nouns(body),
        }
    }

    /// Whether `link` resolves: a term exactly, a noun ignoring case and markdown marks.
    pub fn resolves(&self, link: Link<'_>) -> bool {
        match link {
            Link::Term(t) => self.terms.contains(t),
            Link::Noun(n) => self.nouns.contains(&key(n)),
        }
    }
}

/// The slugs a glossary defines, in 03's form: one `` - `slug` — `` line per term.
pub fn terms(glossary: &str) -> impl Iterator<Item = &str> {
    glossary.lines().filter_map(|line| Some(line.trim_start().strip_prefix("- `")?.split_once('`')?.0))
}

/// Each diagram spec under `design`, as its name and text, by name: `<design>/diagrams/<name>.yaml`. A name with a dot
/// is no spec: `bp diagram` writes none, and a converted `<name>.nodes.yaml` is the node list the spec replaces.
pub fn specs(vault: &Vault, design: &Id) -> Vec<(String, String)> {
    let Ok(dir) = std::fs::read_dir(vault.root.join(design.as_str()).join("diagrams")) else { return Vec::new() };
    let mut out: Vec<(String, String)> = dir
        .filter_map(|e| {
            let path = e.ok()?.path();
            let name = path.file_name()?.to_str()?.strip_suffix(".yaml")?;
            if name.contains('.') {
                return None;
            }
            Some((name.to_string(), std::fs::read_to_string(&path).ok()?))
        })
        .collect();
    out.sort();
    out
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
    let specs = specs(vault, owner);
    let mut lines = Vec::new();
    let mut bad = 0;
    for (name, text) in &specs {
        let findings = bp_diagram::outline(text, &|l| names.resolves(l)).map_or_else(|f| f, |(_, f)| f);
        bad += usize::from(!findings.is_empty());
        lines.extend(findings.iter().map(|f| format!("{}: {name}#{}: {}", f.rule, f.id, f.detail)));
    }
    let (verdict, note) = match (specs.len(), lines.len()) {
        (0, _) => (Verdict::Pass, format!("{owner} has no diagram spec")),
        (n, 0) => (Verdict::Pass, format!("every node of {} of {owner} resolves", plural(n, "diagram spec"))),
        (n, k) => {
            (Verdict::Fail, format!("{} in {bad} of {} of {owner}", plural(k, "finding"), plural(n, "diagram spec")))
        }
    };
    let (tail, dropped, evidence) = evidence(&lines);
    Ok(Ran { verdict, note, commit, tail, dropped, evidence, names: Vec::new() })
}

/// A noun as it compares: without markdown marks or surrounding space, in lower case.
fn key(text: &str) -> String {
    text.replace(['*', '`'], "").trim().to_lowercase()
}

/// The nouns a design body names: each ATX heading, and the first cell of each table row below the header row.
/// Fenced code names none, so a check block's comment is no heading.
fn nouns(body: &str) -> BTreeSet<String> {
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
            out.insert(key(line[hashes..].trim_end_matches('#')));
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
        let names =
            Names { terms: terms("# Glossary\n\n- `vault` — the files.\n").map(String::from).collect(), nouns: got };
        assert!(names.resolves(Link::Term("vault")) && !names.resolves(Link::Term("Vault")), "a term is a slug");
        assert!(names.resolves(Link::Noun("Human View")) && names.resolves(Link::Noun("**Vault**")));
        assert!(!names.resolves(Link::Noun("Container")) && !names.resolves(Link::Noun("tool")));
    }
}

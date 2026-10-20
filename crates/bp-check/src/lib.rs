//! Check runner (15; 25, design changes 4 and 5): runs an owner's check blocks from the code repository, judges each
//! against its attributes, and writes one run record per block in one commit through the vault's write path. No lock
//! is held while a block runs, so a block may call `bp`; the lock comes after, for the commit alone.

mod judge;
mod process;

use std::collections::BTreeMap;
use std::path::Path;

use bp_vault::{Check, Code, Header, Held, Id, Kind, Refusal, Vault};
use serde_json::Value;

use crate::judge::Plan;
use crate::process::Ran;

bp_vault::closed_enum! {
    /// A run's verdict (13, Run). A failure to run differs from a violation (15, E6).
    pub enum Verdict {
        Pass = "pass",
        Fail = "fail",
        CouldNotRun = "could-not-run",
        Skipped = "skipped",
        TimedOut = "timed-out",
    }
}

/// The runs one `check` wrote.
#[derive(Debug)]
pub struct Checked {
    /// The vault commit that holds the run records; none when the owner has no named block.
    pub commit: Option<String>,
    /// One row per block, in body order.
    pub runs: Vec<Run>,
    /// The findings the run records hold.
    pub findings: Vec<Held>,
}

/// One run record, as the result lists it.
#[derive(Debug)]
pub struct Run {
    /// The block's name.
    pub name: String,
    /// The verdict.
    pub verdict: Verdict,
    /// The run record's id.
    pub id: Id,
    /// Why the verdict, in one line.
    pub note: String,
    /// The block is `error` severity: its failure fails the verb.
    pub error: bool,
}

impl Checked {
    /// The first `error` block that neither passed nor was skipped (25, design change 4).
    pub fn failed(&self) -> Option<&Run> {
        self.runs.iter().find(|r| r.error && !matches!(r.verdict, Verdict::Pass | Verdict::Skipped))
    }
}

/// A block that ran, before its record has a key.
struct Done<'b> {
    check: &'b Check,
    at: String,
    ended: String,
    verdict: Verdict,
    note: String,
    ran: Option<Ran>,
}

/// Runs the named blocks of `owner`, or the one `only` names, from the git repository that holds `repo`, and commits
/// one run record per block. `clock` gives the instant, `YYYY-MM-DDTHH:MM:SSZ`; its date decides `until`.
///
/// # Panics
///
/// Never: a named block's name is a slug, and an owner id, a slug and a sha make a run id.
pub fn check(
    vault: &Vault,
    owner: &Id,
    repo: &Path,
    only: Option<&str>,
    role: &str,
    clock: &dyn Fn() -> String,
) -> Result<Checked, Refusal> {
    let file = vault.read(owner)?;
    if !file.header.as_ref().is_some_and(|h| Kind::CHECK_OWNERS.contains(&h.kind)) {
        return Err(Refusal::new(
            Code::SchemaInvalid,
            owner.as_str(),
            "a check block belongs to a design, an issue or a milestone; elsewhere it is prose (13, rule 1)",
            format!("check the owner of the block: bp show {} --no-body", owner.as_str()),
        ));
    }
    let code = bp_vault::git::Repo::find(repo).and_then(|r| Some((r.head()?, r.top))).ok_or_else(|| {
        Refusal::failed(
            repo.display().to_string(),
            "a run records the commit it ran at: run bp check from the code repository, which needs a commit",
        )
    })?;
    let (commit, top) = code;
    // A block without a slug name has no run key; the census already counts it as `check_unnamed`.
    let named: Vec<&Check> =
        file.body.checks.iter().filter(|c| c.attrs.get("name").is_some_and(|n| bp_vault::is_slug(n))).collect();
    let blocks: Vec<&Check> = match only {
        Some(n) => named.iter().copied().filter(|c| c.attrs["name"] == n).collect(),
        None => named.clone(),
    };
    if let Some(n) = only
        && blocks.is_empty()
    {
        let known: Vec<&str> = named.iter().map(|c| c.attrs["name"].as_str()).collect();
        let fix = if known.is_empty() {
            format!("{owner} has no named check block")
        } else {
            format!("name one of the blocks of {owner}: {}", known.join(", "))
        };
        return Err(Refusal::usage(format!("--name {n}"), fix));
    }
    if blocks.is_empty() {
        return Ok(Checked { commit: None, runs: Vec::new(), findings: Vec::new() });
    }
    let today = clock()[..10].to_string();
    let done: Vec<Done> = blocks
        .into_iter()
        .map(|check| {
            let at = clock();
            let (verdict, note, ran) = match judge::plan(check, &today) {
                Plan::Skip(why) => (Verdict::Skipped, why, None),
                Plan::Broken(why) => (Verdict::CouldNotRun, why, None),
                Plan::Run(spec) => {
                    let dir = spec.cwd.as_ref().map_or_else(|| top.clone(), |d| top.join(d));
                    let ran = process::run(&check.text, &dir, &vault.root, spec.timeout);
                    let (verdict, note) = judge::judge(&spec.test, &ran.end);
                    (verdict, note, Some(ran))
                }
            };
            Done { check, at, ended: clock(), verdict, note, ran }
        })
        .collect();
    let writer = vault.writer()?;
    let mut changes = BTreeMap::new();
    let mut runs = Vec::with_capacity(done.len());
    for d in &done {
        let name = d.check.attrs["name"].as_str();
        // The first free attempt, read under the lock: a run is never overwritten (15, the run record).
        let mut attempt = 1_u32;
        let id = loop {
            let id = Id::parse(&format!("{owner}/runs/{name}-{commit}-{attempt}")).expect("a slug key makes an id");
            if !changes.contains_key(&id) && !id.path(&vault.root).exists() {
                break id;
            }
            attempt += 1;
        };
        changes.insert(id.clone(), record(owner, &commit, attempt, d));
        let error = judge::is_error(d.check);
        runs.push(Run { name: name.to_string(), verdict: d.verdict, id, note: d.note.clone(), error });
    }
    let written = writer.commit("check", role, &format!("check {owner}"), &changes)?;
    Ok(Checked { commit: Some(written.commit), runs, findings: written.findings })
}

/// The run record's text (13, Run; 25, design change 5): the converted fields, then `ended`, the block's hash and
/// the output's; the body is the verdict's reason and the output's last lines.
fn record(owner: &Id, commit: &str, attempt: u32, d: &Done) -> String {
    let attrs: Vec<String> = d.check.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let block = format!("check {} {{{}}}\n{}", d.check.runner, attrs.join(" "), d.check.text);
    let mut fields: BTreeMap<String, Value> = BTreeMap::from([
        ("check".into(), d.check.attrs["name"].as_str().into()),
        ("commit".into(), commit.into()),
        ("attempt".into(), attempt.into()),
        ("at".into(), d.at.as_str().into()),
        ("ended".into(), d.ended.as_str().into()),
        ("verdict".into(), d.verdict.as_str().into()),
        ("check_hash".into(), blake3::hash(block.as_bytes()).to_hex().as_str().into()),
    ]);
    if let Some(ran) = &d.ran {
        fields.insert("evidence".into(), ran.evidence.as_str().into());
    }
    let header = Header {
        kind: Kind::Run,
        title: None,
        status: None,
        under: Some(owner.clone()),
        aliases: Vec::new(),
        links: BTreeMap::new(),
        unresolved: Vec::new(),
        fields,
    };
    format!("---\n{}---\n\n{}", header.emit(), body(&d.note, d.ran.as_ref()))
}

/// The reason, then the output's tail in a fence longer than any backtick run it holds, so no output line reads as
/// markdown: a heading, a fence or a wikilink in it stays text.
fn body(note: &str, ran: Option<&Ran>) -> String {
    let Some(ran) = ran.filter(|r| !r.tail.is_empty()) else { return format!("{note}\n") };
    let run = ran.tail.iter().flat_map(|l| l.split(|c| c != '`')).map(str::len).max().unwrap_or(0);
    let fence = "`".repeat((run + 1).max(3));
    let mut parts = vec![note.to_string()];
    if ran.dropped > 0 {
        parts.push(format!("The last {} of {} lines of output.", ran.tail.len(), ran.tail.len() + ran.dropped));
    }
    parts.push(format!("{fence}\n{}\n{fence}", ran.tail.join("\n")));
    parts.join("\n\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_stays_text_in_the_body() {
        let ran = Ran {
            end: process::End::TimedOut(std::time::Duration::from_secs(1)),
            tail: vec!["## heading".into(), "````".into(), "[[shared/nowhere]]".into()],
            dropped: 2,
            evidence: String::new(),
        };
        let text = body("why", Some(&ran));
        assert!(text.starts_with("why\n\nThe last 3 of 5 lines of output.\n\n`````\n"), "{text}");
        let parsed = bp_vault::Body::parse(&text);
        assert!(parsed.sections.is_empty() && parsed.links.is_empty() && parsed.unresolved.is_empty(), "{parsed:?}");
        assert!(parsed.findings.is_empty(), "{:?}", parsed.findings);
        assert_eq!(body("skipped", None), "skipped\n");
    }
}

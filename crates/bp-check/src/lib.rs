//! Check runner (15; 25, design changes 4 and 5): runs an owner's check blocks from the code repository, judges each
//! against its attributes, and gives one run record per block for the vault's write path. It splits in two (26,
//! design change 6): [`run`] holds no lock while a block runs, so a block may call `bp`; [`Ran::records`] gives the
//! record texts under the lock, which a verb commits with its own changes. `check` commits them alone.

mod integrity;
mod judge;
mod process;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use bp_vault::{Check, Code, Header, Held, Id, Kind, Refusal, Vault};
use serde_json::Value;

use crate::judge::Plan;
pub use crate::judge::is_error;

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

/// How long the commit waits for `.bp/lock`, in rounds of the writer's own 5 s wait.
const LOCK_WAIT: Duration = Duration::from_secs(60);

/// The runs one `check` wrote.
#[derive(Debug)]
pub struct Checked {
    /// The vault commit that holds the run records.
    pub commit: String,
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
    /// The first failed run.
    pub fn failed(&self) -> Option<&Run> {
        self.runs.iter().find(|r| r.failed())
    }
}

impl Run {
    /// An `error` block that neither passed nor was skipped: it fails the verb (25, design change 4).
    pub fn failed(&self) -> bool {
        self.error && !matches!(self.verdict, Verdict::Pass | Verdict::Skipped)
    }
}

/// A block that ran, before its record has a key.
struct Done {
    check: Check,
    at: String,
    ended: String,
    verdict: Verdict,
    note: String,
    /// The last lines of output and the count of lines before them.
    tail: Vec<String>,
    dropped: usize,
    evidence: Option<String>,
    /// The commit the run names when it is not the code's: an integrity run's vault head.
    commit: Option<String>,
    /// The nodes an integrity run failed.
    names: Vec<Id>,
}

/// The blocks one call ran, with no lock held, before any record is written. A verb that runs blocks takes the lock
/// after, and commits the records with its own changes (26, design change 6).
pub struct Ran {
    owner: Id,
    /// The code commit the blocks ran at.
    commit: String,
    done: Vec<Done>,
}

/// The block's `check_hash` (13, Run): blake3 of its fence line and text. A run counts for a block only while the
/// hashes match, so a block edited after its run needs a new one (26, design change 4).
pub fn hash(check: &Check) -> String {
    let attrs: Vec<String> = check.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let block = format!("check {} {{{}}}\n{}", check.runner, attrs.join(" "), check.text);
    blake3::hash(block.as_bytes()).to_hex().to_string()
}

/// The blocks of a body that have a run key: a slug `name`. The census counts the rest as `check_unnamed`.
pub fn named(body: &bp_vault::Body) -> Vec<&Check> {
    body.checks.iter().filter(|c| c.attrs.get("name").is_some_and(|n| bp_vault::is_slug(n))).collect()
}

/// Runs the named blocks of `owner`, or the one `only` names, from the git repository that holds `repo`, and commits
/// one run record per block; none when the owner has no named block. `clock` gives the instant,
/// `YYYY-MM-DDTHH:MM:SSZ`; its date decides `until`.
pub fn check(
    vault: &Vault,
    owner: &Id,
    repo: &Path,
    only: Option<&str>,
    role: &str,
    clock: &dyn Fn() -> String,
) -> Result<Option<Checked>, Refusal> {
    let Some(ran) = run(vault, owner, repo, only, &[], clock)? else { return Ok(None) };
    let writer = ran.writer(vault, &format!("bp check {owner}"))?;
    let (changes, runs) = ran.records(vault);
    let written = writer.commit("check", role, &format!("check {owner}"), &changes)?;
    Ok(Some(Checked { commit: written.commit, runs, findings: written.findings }))
}

/// Runs the named blocks of `owner` but those `skip` names, or the one `only` names, from the git repository that
/// holds `repo`, holding no lock, so a block may call `bp`. `None` when no block is left to run.
///
/// # Panics
///
/// Never: a named block's name is a slug.
pub fn run(
    vault: &Vault,
    owner: &Id,
    repo: &Path,
    only: Option<&str>,
    skip: &[&str],
    clock: &dyn Fn() -> String,
) -> Result<Option<Ran>, Refusal> {
    let file = vault.read(owner)?;
    if !file.header.as_ref().is_some_and(|h| Kind::CHECK_OWNERS.contains(&h.kind)) {
        return Err(Refusal::new(
            Code::SchemaInvalid,
            owner.as_str(),
            "a check block belongs to a design, an issue or a milestone; elsewhere it is prose (13, rule 1)",
            format!("check the owner of the block: bp show {} --no-body", owner.as_str()),
        ));
    }
    let named = named(&file.body);
    let blocks: Vec<&Check> = match only {
        Some(n) => named.iter().copied().filter(|c| c.attrs["name"] == n).collect(),
        None => named.iter().copied().filter(|c| !skip.contains(&c.attrs["name"].as_str())).collect(),
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
        return Ok(None);
    }
    let (commit, top) = code_tree(vault, repo)?;
    if blocks.iter().any(|c| c.runner == "integrity") {
        integrity::clean(vault, owner)?;
    }
    let today = clock()[..10].to_string();
    let done: Vec<Done> = blocks
        .into_iter()
        .map(|check| {
            let at = clock();
            let done = |verdict, note| Done {
                check: check.clone(),
                at: at.clone(),
                ended: String::new(),
                verdict,
                note,
                tail: Vec::new(),
                dropped: 0,
                evidence: None,
                commit: None,
                names: Vec::new(),
            };
            let mut d = match judge::plan(check, &today) {
                Plan::Skip(why) => done(Verdict::Skipped, why),
                Plan::Broken(why) => done(Verdict::CouldNotRun, why),
                Plan::Run(spec) => {
                    let dir = spec.cwd.as_ref().map_or_else(|| top.clone(), |d| top.join(d));
                    let ran = process::run(&check.text, &dir, &vault.root, spec.timeout);
                    let (verdict, note) = judge::judge(&spec.test, &ran.end);
                    Done { tail: ran.tail, dropped: ran.dropped, evidence: ran.evidence, ..done(verdict, note) }
                }
                Plan::Integrity => match integrity::run(vault, owner) {
                    Ok(ran) => Done {
                        tail: ran.tail,
                        dropped: ran.dropped,
                        evidence: Some(ran.evidence),
                        commit: Some(ran.commit),
                        names: ran.names,
                        ..done(ran.verdict, ran.note)
                    },
                    Err(why) => done(Verdict::CouldNotRun, why),
                },
            };
            d.ended = clock();
            d
        })
        .collect();
    Ok(Some(Ran { owner: owner.clone(), commit, done }))
}

impl Ran {
    /// The code commit the blocks ran at.
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// The writer, waited for up to [`LOCK_WAIT`]: the verdicts cost the whole run, so it waits longer than a plain
    /// write. If the lock never frees, the refusal's fix carries every verdict and `then`, the command to rerun, so
    /// the run is not lost with the commit.
    pub fn writer<'v>(&self, vault: &'v Vault, then: &str) -> Result<bp_vault::Writer<'v>, Refusal> {
        let start = Instant::now();
        loop {
            match vault.writer() {
                Err(r) if r.code == Code::VaultLocked && start.elapsed() < LOCK_WAIT => {}
                Err(mut r) if r.code == Code::VaultLocked => {
                    let verdicts: Vec<String> =
                        self.done.iter().map(|d| format!("{} {}", d.check.attrs["name"], d.verdict)).collect();
                    r.fix = format!(
                        "the lock stayed held for {} s, so no run was recorded; the verdicts were: {}; rerun {then} once the other write has finished",
                        LOCK_WAIT.as_secs(),
                        verdicts.join(", ")
                    );
                    return Err(r);
                }
                other => return other,
            }
        }
    }

    /// Each run record's id and text, and its row in body order. Each takes the first free attempt, so the caller
    /// holds the writer: a run is never overwritten (15, the run record).
    ///
    /// # Panics
    ///
    /// Never: an owner id, a slug and a sha make a run id.
    pub fn records(&self, vault: &Vault) -> (BTreeMap<Id, String>, Vec<Run>) {
        let mut changes = BTreeMap::new();
        let mut runs = Vec::with_capacity(self.done.len());
        for d in &self.done {
            let name = d.check.attrs["name"].as_str();
            let commit = d.commit.as_deref().unwrap_or(&self.commit);
            let owner = &self.owner;
            let mut attempt = 1_u32;
            let id = loop {
                let id = Id::parse(&format!("{owner}/runs/{name}-{commit}-{attempt}")).expect("a slug key makes an id");
                if !changes.contains_key(&id) && !id.path(&vault.root).exists() {
                    break id;
                }
                attempt += 1;
            };
            changes.insert(id.clone(), record(owner, commit, attempt, d));
            let error = judge::is_error(&d.check);
            runs.push(Run { name: name.to_string(), verdict: d.verdict, id, note: d.note.clone(), error });
        }
        (changes, runs)
    }
}

/// The code repository's HEAD and top. A run names the commit its blocks ran at, so the tree must be that commit:
/// clean, untracked files included, and not the vault itself (15, the check grammar).
fn code_tree(vault: &Vault, repo: &Path) -> Result<(String, std::path::PathBuf), Refusal> {
    let here = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    if here.starts_with(&vault.root) {
        return Err(Refusal::usage(
            here.display().to_string(),
            format!("run bp check from the code repository, not from inside the vault at {}", vault.root.display()),
        ));
    }
    let (commit, top) = bp_vault::git::Repo::find(repo).and_then(|r| Some((r.head()?, r.top))).ok_or_else(|| {
        Refusal::failed(
            repo.display().to_string(),
            "a run records the commit it ran at: run bp check from the code repository, which needs a commit",
        )
    })?;
    let status = Command::new("git").arg("-C").arg(&top).args(["status", "--porcelain"]).output();
    match status {
        Ok(out) if out.status.success() && out.stdout.is_empty() => Ok((commit, top)),
        Ok(out) if out.status.success() => {
            let first = String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or_default().to_string();
            Err(Refusal::new(
                Code::PathDirty,
                top.display().to_string(),
                "a run records the commit its blocks ran at, so the code tree is clean (15, the check grammar)",
                format!("commit or stash the changes in {} (first: {}), then rerun", top.display(), first.trim()),
            ))
        }
        Ok(out) => Err(Refusal::failed(top.display().to_string(), String::from_utf8_lossy(&out.stderr).trim())),
        Err(e) => Err(Refusal::failed(top.display().to_string(), format!("git did not start: {e}"))),
    }
}

/// The run record's text (13, Run; 25, design change 5): the converted fields, then `ended`, the block's hash and
/// the output's; the body is the verdict's reason and the output's last lines.
fn record(owner: &Id, commit: &str, attempt: u32, d: &Done) -> String {
    let mut fields: BTreeMap<String, Value> = BTreeMap::from([
        ("check".into(), d.check.attrs["name"].as_str().into()),
        // The block's kind, so a reader finds the integrity runs without parsing bodies (21, blocked).
        ("check_kind".into(), d.check.runner.as_str().into()),
        ("commit".into(), commit.into()),
        ("attempt".into(), attempt.into()),
        ("at".into(), d.at.as_str().into()),
        ("ended".into(), d.ended.as_str().into()),
        ("verdict".into(), d.verdict.as_str().into()),
        ("severity".into(), if judge::is_error(&d.check) { "error" } else { "warn" }.into()),
        ("check_hash".into(), hash(&d.check).into()),
    ]);
    if let Some(evidence) = &d.evidence {
        fields.insert("evidence".into(), evidence.as_str().into());
    }
    let mut links = BTreeMap::new();
    if !d.names.is_empty() {
        links.insert("names".to_string(), d.names.clone());
    }
    let header = Header {
        kind: Kind::Run,
        title: None,
        status: None,
        under: Some(owner.clone()),
        aliases: Vec::new(),
        links,
        unresolved: Vec::new(),
        fields,
    };
    format!("---\n{}---\n\n{}", header.emit(), body(&d.note, &d.tail, d.dropped))
}

/// The reason as the lead, then the output's tail under `## Output`, so `show --no-body` keeps only the reason. The
/// tail sits in a fence longer than any backtick run it holds, so no output line reads as markdown: a heading, a
/// fence or a wikilink in it stays text.
fn body(note: &str, tail: &[String], dropped: usize) -> String {
    if tail.is_empty() {
        return format!("{note}\n");
    }
    let run = tail.iter().flat_map(|l| l.split(|c| c != '`')).map(str::len).max().unwrap_or(0);
    let fence = "`".repeat((run + 1).max(3));
    let mut parts = vec![note.to_string(), "## Output".to_string()];
    if dropped > 0 {
        parts.push(format!("The last {} of {} lines of output.", tail.len(), tail.len() + dropped));
    }
    parts.push(format!("{fence}\n{}\n{fence}", tail.join("\n")));
    parts.join("\n\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_stays_text_in_the_body() {
        let tail = ["## heading".to_string(), "````".into(), "[[shared/nowhere]]".into()];
        let text = body("why", &tail, 2);
        assert!(text.starts_with("why\n\n## Output\n\nThe last 3 of 5 lines of output.\n\n`````\n"), "{text}");
        let parsed = bp_vault::Body::parse(&text);
        assert_eq!(parsed.lead, "why");
        let headings: Vec<&str> = parsed.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["Output"], "the heading in the output stays text");
        assert!(parsed.links.is_empty() && parsed.unresolved.is_empty(), "{parsed:?}");
        assert!(parsed.findings.is_empty(), "{:?}", parsed.findings);
        assert_eq!(body("skipped", &[], 0), "skipped\n");
    }
}

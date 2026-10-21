//! The `integrity` kind (25, issue 11): the owner's project against the vault rules, in process over the index. Its
//! run names the vault's head, not the code's, because integrity is vault state; and it names each failing node, so
//! `blocked` reads the run instead of the rules (21).

use std::collections::BTreeSet;
use std::process::Command;

use bp_vault::{Code, Fault, Id, Index, Refusal, Vault};

use crate::Verdict;
use crate::process::Tail;

/// One in-process run, integrity or nodes, before its record has a key.
pub(crate) struct Ran {
    pub(crate) verdict: Verdict,
    pub(crate) note: String,
    /// The vault's head when the index was read.
    pub(crate) commit: String,
    /// One line per finding, within the run record's tail limits, and the lines before them.
    pub(crate) tail: Vec<String>,
    pub(crate) dropped: usize,
    /// blake3 of every line, then the NUL byte that separates an empty stderr (13, Run).
    pub(crate) evidence: String,
    /// The failing nodes, sorted: only parsed files, since a run that names a broken one adds a broken link.
    pub(crate) names: Vec<Id>,
}

/// Refuses `path_dirty` when the owner's project holds a change git has not committed, untracked files included:
/// the run names the vault's head, so that commit must hold every fault the run names (25, issue 11).
pub(crate) fn clean(vault: &Vault, owner: &Id) -> Result<(), Refusal> {
    let project = owner.project().expect("a check owner is a project's design, issue or milestone");
    let dir = format!("projects/{project}/");
    // `--no-optional-locks`: a plain `status` rewrites git's index, which moves the vault's fingerprint (12, Index).
    let out = Command::new("git")
        .arg("-C")
        .arg(&vault.root)
        .args(["--no-optional-locks", "status", "--porcelain", "--", &dir])
        .output();
    match out {
        Ok(o) if o.status.success() && o.stdout.is_empty() => Ok(()),
        Ok(o) if o.status.success() => {
            let first = String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or_default().trim().to_string();
            Err(Refusal::new(
                Code::PathDirty,
                dir.clone(),
                "an in-process run names the vault's head, so the project's files are committed (15, the check grammar)",
                format!("commit or restore the changes under {dir} (first: {first}), then rerun bp check {owner}"),
            ))
        }
        Ok(o) => Err(Refusal::failed(dir, String::from_utf8_lossy(&o.stderr).trim())),
        Err(e) => Err(Refusal::failed(dir, format!("git did not start: {e}"))),
    }
}

/// Runs the rules over `owner`'s project. `Err` is why it could not run.
pub(crate) fn run(vault: &Vault, owner: &Id) -> Result<Ran, String> {
    let project = owner.project().expect("a check owner is a project's design, issue or milestone");
    let commit = bp_vault::git::Repo::find(&vault.root)
        .and_then(|r| r.head())
        .ok_or_else(|| format!("the vault at {} has no commit for the run to name", vault.root.display()))?;
    let faults =
        Index::open(vault).and_then(|i| i.integrity(vault, project)).map_err(|r| format!("{}: {}", r.rule, r.fix))?;
    Ok(judge(project, commit, &faults))
}

fn judge(project: &str, commit: String, faults: &[Fault]) -> Ran {
    let names: BTreeSet<Id> = faults.iter().filter(|f| f.node).filter_map(|f| Id::parse(&f.id).ok()).collect();
    let lines: Vec<String> = faults.iter().map(|f| format!("{}: {}: {}", f.rule, f.id, f.detail)).collect();
    let (tail, dropped, evidence) = evidence(&lines);
    let (verdict, note) = if faults.is_empty() {
        (Verdict::Pass, format!("no file of projects/{project} breaks a vault rule"))
    } else {
        let nodes = faults.iter().map(|f| f.id.as_str()).collect::<BTreeSet<_>>().len();
        (
            Verdict::Fail,
            format!("{} on {} of projects/{project}", plural(faults.len(), "finding"), plural(nodes, "file")),
        )
    };
    Ran { verdict, note, commit, tail, dropped, evidence, names: names.into_iter().collect() }
}

/// `n` of `one`, with its plural.
pub(crate) fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// An in-process run's output, one line per finding, as a run record keeps it: the tail, the count of lines before
/// it, and the evidence digest, blake3 of every line, then the NUL byte that separates an empty stderr (13, Run).
pub(crate) fn evidence(lines: &[String]) -> (Vec<String>, usize, String) {
    let mut hasher = blake3::Hasher::new();
    let mut tail = Tail::default();
    for line in lines {
        // A parse error may span lines; the output keeps one line per finding.
        let line: String = line.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
        tail.push(line);
    }
    hasher.update(&[0]);
    let (tail, dropped) = tail.drain();
    (tail, dropped, hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fault(rule: &'static str, id: &str, detail: &str, node: bool) -> Fault {
        Fault { rule, id: id.into(), detail: detail.into(), node }
    }

    #[test]
    fn any_finding_fails_and_names_each_parsed_node_once() {
        let faults = [
            fault("broken", "projects/bp/issues/x.md", "line 1\nline 2", false),
            fault("issue", "projects/bp/issues/b", "cites no design that exists", true),
            fault("issue", "projects/bp/issues/a", "no `under` milestone", true),
            fault("issue", "projects/bp/issues/a", "cites no design that exists", true),
        ];
        let ran = judge("bp", "abc".into(), &faults);
        assert_eq!(ran.verdict, Verdict::Fail);
        assert_eq!(ran.note, "4 findings on 3 files of projects/bp");
        let names: Vec<&str> = ran.names.iter().map(Id::as_str).collect();
        assert_eq!(names, ["projects/bp/issues/a", "projects/bp/issues/b"], "sorted, once, no broken file");
        assert_eq!(ran.tail[0], "broken: projects/bp/issues/x.md: line 1 line 2", "one line per finding");
        assert_eq!((ran.tail.len(), ran.dropped), (4, 0));
        let mut want = blake3::Hasher::new();
        want.update(format!("{}\n\0", ran.tail.join("\n")).as_bytes());
        assert_eq!(ran.evidence, want.finalize().to_hex().to_string());
        let clean = judge("bp", "abc".into(), &[]);
        assert_eq!((clean.verdict, clean.names.len(), clean.tail.len()), (Verdict::Pass, 0, 0));
        assert_eq!(judge("bp", "abc".into(), &faults[1..2]).note, "1 finding on 1 file of projects/bp");
    }

    #[test]
    fn the_output_keeps_the_tail_and_the_names_keep_every_node() {
        let faults: Vec<Fault> = (0..150)
            .map(|i| fault("issue", &format!("projects/bp/issues/i{i:03}"), "no `under` milestone", true))
            .collect();
        let ran = judge("bp", "abc".into(), &faults);
        assert_eq!((ran.tail.len(), ran.dropped, ran.names.len()), (crate::process::TAIL, 50, 150));
    }
}

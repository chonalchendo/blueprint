//! `bp risk <issue>`: the risk level in force and the rules that set it (37, design changes 1 and 2). The level is the
//! highest of the issue's `risk` label (absent reads `medium`), the level the claim's diff computes, each review that
//! holds a `risk` row as first committed, and a pending delta the issue caused; then the round caps (design change 6).
//! The diff is what a head lands: its test merge into the sha that origin's server names for the base branch the pull
//! request targets, diffed from that sha. The head is the higher of the local branch and the sha origin names for it;
//! each sha is fetched. The rules are `.bp/risk.yml` on that base commit, so a branch that edits them is judged by the
//! base's rules, and its edit is high. The answer names each head it judged and the base. It writes nothing to the
//! vault and calls no `gh`.

use std::collections::BTreeSet;
use std::path::Path;

use bp_change::intents::Base;
use bp_change::risk::{self, Computed, Config, Level, Pattern, RISK_FILE};
use bp_change::rounds;
use bp_read::inputs::{field, held};
use bp_read::snapshot::clone_of;
use bp_vault::{Code, Id, Index, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::claim::{branch, default_branch, git};
use crate::subject;

/// `bp risk <issue>`.
#[derive(Args)]
pub struct Risk {
    /// The issue whose risk level prints.
    pub issue: String,
}

/// The computed level, the base commit whose rules judged it, and each head it judged. `unmerged` when the level is
/// that of a head with no test merge: the next step is then a rebase.
pub(crate) struct Judged {
    pub(crate) computed: Computed,
    pub(crate) base: String,
    pub(crate) heads: Vec<String>,
    pub(crate) unmerged: bool,
}

pub(crate) fn risk(cwd: &Path, r: &Risk) -> Result<Value, Refusal> {
    let vault = Vault::discover(cwd)?;
    let index = Index::open(&vault)?;
    let id = subject(&index, &r.issue)?;
    let Some(header) = index.header(id.as_str())? else {
        return Err(vault.unparsed(&id).unwrap_or_else(|| {
            Refusal::new(Code::NoNode, id.as_str(), "bp risk reads one issue (37, design change 1)", "bp list issue")
        }));
    };
    if header.get("kind").and_then(Value::as_str) != Some("issue") {
        // `cut` refuses a node of another kind so too: only an issue moves, and only an issue has a level.
        return Err(Refusal::new(
            Code::IllegalTransition,
            id.as_str(),
            "bp risk reads one issue (37, design change 1)",
            "bp list issue",
        ));
    }
    let declared = match header.get("risk") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str().and_then(|s| s.parse().ok()).ok_or_else(|| {
            Refusal::new(
                Code::UnknownValue,
                format!("{id} risk {v}"),
                "a label with a closed set takes one of its values (13)",
                format!("bp set {id} risk <low|medium|high>"),
            )
        })?),
    };
    // A pending delta is vault data: it raises the level with or without a clone in view (37, design change 1).
    let delta = !bp_read::carried_deltas(&index, &id)?.is_empty();
    // Each review counts as its first commit wrote it, so an edit cannot drop a raise or an error row (37, design
    // change 2).
    let reviews = bp_read::inputs::reviews(&vault, &index, id.as_str())?;
    let raised_by: Vec<&str> = reviews.iter().filter(|r| r.raises()).map(|r| r.id.as_str()).collect();
    let judged = judged(cwd, &index, &id)?;
    let computed = judged.as_ref().map(|j| j.computed.level);
    let base = risk::level(declared.unwrap_or(Level::Medium), computed, raised_by.len() + usize::from(delta));
    let (level, capped) = rounds::capped(base, &reviews);
    let mut reasons = Vec::new();
    match declared {
        Some(d) if d == level => reasons.push(format!("the issue declares {d}")),
        None if level == Level::Medium => reasons.push("the issue declares no risk, which reads medium".into()),
        _ => {}
    }
    if let Some(j) = judged.as_ref().filter(|j| j.computed.level == level) {
        reasons.extend(j.computed.reasons.iter().cloned());
    }
    if level == Level::High {
        if delta {
            reasons.push("the issue caused a pending delta, and a design change is high".into());
        }
        reasons.extend(raised_by.iter().map(|r| format!("review {r} holds a risk row")));
    }
    if let Some((round, from)) = capped {
        reasons.push(format!("round cap: the review of round {round} holds an error row at or past the cap of {from}"));
    }
    Ok(json!({
        "level": level,
        "declared": declared.unwrap_or(Level::Medium),
        "computed": computed,
        "head": judged.as_ref().map_or(Vec::new(), |j| j.heads.clone()),
        "base": judged.as_ref().map(|j| j.base.clone()),
        "raised_by": raised_by,
        "reasons": reasons,
    }))
}

/// The level of the held claim's diff; `None` with no held claim, or when the working directory is in no clone of
/// the project's repository. A gate reads `None` as not computed, never as low. The heads are the higher of the local
/// branch and the one origin names.
fn judged(cwd: &Path, index: &Index, id: &Id) -> Result<Option<Judged>, Refusal> {
    let Some(claim) = held(index, id)? else { return Ok(None) };
    let top = match clone_of(cwd, index, id.project().unwrap_or_default()) {
        Ok(top) => top,
        Err(e) if e.code == Code::NoClone => return Ok(None),
        Err(e) => return Err(e),
    };
    // The base branch the pull request targets, as `land` finds it: a layer over an open dependency targets its branch.
    let target = match bp_read::inputs::base(index, &claim)? {
        Base::Branch(b) => b,
        Base::Unknown => default_branch(&top)?,
        Base::Unmerged(_) => field(&claim, "base_branch").unwrap_or_default(),
    };
    let name = branch(id);
    let local = git(&top, &["rev-parse", "--verify", "-q", &format!("refs/heads/{name}^{{commit}}")]).ok();
    let mut heads: Vec<String> = local.into_iter().chain(at_origin(&top, &name)?).collect();
    heads.dedup();
    if heads.is_empty() {
        return Err(Refusal::failed(name, format!("the clone holds no branch of the claim; bp cut {id} puts it back")));
    }
    judge(&top, &target, heads).map(Some)
}

/// The level of each head's diff against the base that origin names for `target`, by the rules on that base. `pr`
/// calls it with the head it pushes.
pub(crate) fn judge(top: &Path, target: &str, heads: Vec<String>) -> Result<Judged, Refusal> {
    let Some(base) = at_origin(top, target)? else {
        return Err(Refusal::failed(format!("origin/{target}"), "origin holds no base branch of the claim"));
    };
    let config = rules(top, &base, &format!("origin/{target}"), &heads)?;
    judge_by(top, base, heads, &config)
}

/// The level of what each head lands on `base`, by `config`; the highest wins, and a tie keeps the first head's
/// reasons. `bp gate` calls it with the base the forge names and that base's rules. What lands is the test merge of
/// the head into the base, diffed from the base: `base...head` reads from one merge base, which a commit date picks
/// among several, and it shows a path as the head's start held it, before a rename on the base (review rows 10, 11).
/// A conflict, or a test merge that fails, reads high: what lands is then not known.
pub(crate) fn judge_by(top: &Path, base: String, heads: Vec<String>, config: &Config) -> Result<Judged, Refusal> {
    let mut best: Option<(Computed, bool)> = None;
    for head in &heads {
        let landed = landed(top, &base, head)?;
        let unmerged = landed.is_err();
        let c = match landed {
            Ok(tree) => {
                // Neither `.gitmodules` nor an attribute of the clone hides or shrinks a change: a gitlink always
                // prints, no driver or text conversion runs, and a size git does not count reads high.
                let args = [
                    "-c",
                    "core.quotePath=false",
                    "-c",
                    "core.attributesFile=/dev/null",
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--text",
                    "--ignore-submodules=none",
                    "--raw",
                    "--numstat",
                    "-z",
                    "--find-renames",
                    &base,
                    &tree,
                ];
                risk::computed(&risk::changes(&git(top, &args)?), config)
            }
            // What lands is not known, so the security lens joins round one as for a high path.
            Err(why) => Computed { level: Level::High, reasons: vec![format!("{head} {why}")], security: true },
        };
        if best.as_ref().is_none_or(|(b, _)| c.level > b.level) {
            best = Some((c, unmerged));
        }
    }
    let (computed, unmerged) = best.expect("one head at least");
    Ok(Judged { computed, base, heads, unmerged })
}

/// The tree of the test merge of `head` into `base`, or why there is none: a conflict, or git's own words.
fn landed(top: &Path, base: &str, head: &str) -> Result<Result<String, String>, Refusal> {
    let out = bp_read::git::command(top)
        .args(["merge-tree", "--write-tree", base, head])
        .output()
        .map_err(|e| Refusal::failed("git", format!("{e}; install git")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(match out.status.code() {
        Some(0) => Ok(stdout.lines().next().unwrap_or_default().to_string()),
        Some(1) => Err(format!("conflicts with {base}, so what lands is not known")),
        _ => Err(format!("has no test merge with {base}: {}", String::from_utf8_lossy(&out.stderr).trim())),
    })
}

/// The sha origin's server names for `refs/heads/<branch>`, fetched; `None` when it holds no such branch. A failed
/// call refuses: old refs would judge another head by stale rules. Nothing of the clone's own names the sha: a
/// `remote.origin.fetch` refspec, `git update-ref` or `git remote set-head` moves each local ref with no push.
pub(crate) fn at_origin(top: &Path, branch: &str) -> Result<Option<String>, Refusal> {
    let want = format!("refs/heads/{branch}");
    let listed = git(top, &["ls-remote", "origin", &want])?;
    let sha = listed.lines().find_map(|l| l.split_once('\t').filter(|(_, r)| *r == want).map(|(sha, _)| sha));
    let Some(sha) = sha.filter(|s| s.len() >= 40 && s.bytes().all(|b| b.is_ascii_hexdigit())) else { return Ok(None) };
    git(top, &["fetch", "-q", "origin", sha])
        .map_err(|e| Refusal::failed(e.subject, format!("git cannot fetch {sha}, origin's {branch}: {}", e.fix)))?;
    Ok(Some(sha.to_string()))
}

/// `.bp/risk.yml` on the base commit; the default when it holds none. An exact pattern that names a directory of the
/// base tree or of a head's, in any ASCII case, matches no path under it, so it refuses as a parse failure does.
/// `place` names the base in a refusal: `origin/<branch>`, or the sha `bp gate` was given.
pub(crate) fn rules(top: &Path, base: &str, place: &str, heads: &[String]) -> Result<Config, Refusal> {
    let at = format!("{base}:{RISK_FILE}");
    let invalid = |why: String| {
        let branch = place.strip_prefix("origin/").unwrap_or(place);
        Refusal::new(
            Code::RiskFileInvalid,
            format!("{place}:{RISK_FILE}"),
            "the risk file holds `high` and `low` lists of path patterns, `limits`, `signer` and `gate` (37, design \
             changes 1 and 5)",
            format!("{why}; edit {RISK_FILE} on {branch} by hand, then git commit it"),
        )
    };
    // A base the clone lacks, or a git that cannot run, is no missing risk file: the default rules would turn a clone
    // fault into a lower level, and `bp gate` would send the human to `install` (review of #99, row 33). git words a
    // missing commit and a missing path alike, so the commit is read first.
    git(top, &["cat-file", "-e", &format!("{base}^{{commit}}")]).map_err(|e| {
        Refusal::failed(place, format!("{}; git fetch origin {base} brings the base commit, then rerun", e.fix))
    })?;
    match git(top, &["cat-file", "-e", &at]) {
        Ok(_) => {}
        Err(e) if e.fix.contains("does not exist in") => return Ok(Config::default()),
        Err(e) => return Err(e),
    }
    let config = Config::parse(&git(top, &["show", &at])?).map_err(invalid)?;
    let mut dirs = BTreeSet::new();
    for tree in std::iter::once(base).chain(heads.iter().map(String::as_str)) {
        let listed = git(top, &["ls-tree", "-r", "-d", "-z", "--name-only", tree])?;
        dirs.extend(listed.split('\0').map(str::to_ascii_lowercase));
    }
    let exact = config.high.iter().chain(&config.low).filter_map(|p| match p {
        Pattern::Exact(e) => Some(e.as_str()),
        _ => None,
    });
    if let Some(path) = exact.into_iter().find(|p| dirs.contains(&p.to_ascii_lowercase())) {
        return Err(invalid(format!("the pattern {path:?} names a directory, so it matches no path: write {path}/")));
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use bp_change::risk::{Config, Level};

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("SSH_AUTH_SOCK")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn a_head_with_no_test_merge_reads_high_and_calls_the_security_lens() {
        // An orphan head shares no history with the base: `git merge-tree` exits 128, which is no conflict.
        let t = tempfile::tempdir().unwrap();
        let top = t.path();
        git(top, &["init", "-q", "-b", "master"]);
        std::fs::create_dir_all(top.join("docs")).unwrap();
        std::fs::write(top.join("docs/a.md"), "a\n").unwrap();
        git(top, &["add", "-A"]);
        git(top, &["commit", "-qm", "base"]);
        let base = git(top, &["rev-parse", "HEAD"]);
        git(top, &["checkout", "-q", "--orphan", "other"]);
        std::fs::write(top.join("docs/a.md"), "b\n").unwrap();
        git(top, &["add", "-A"]);
        git(top, &["commit", "-qm", "orphan"]);
        let head = git(top, &["rev-parse", "HEAD"]);
        let config = Config::parse("low: [docs/]\n").unwrap();
        let judged = super::judge_by(top, base.clone(), vec![head.clone()], &config).unwrap();
        let c = &judged.computed;
        assert_eq!((c.level, c.security, judged.unmerged), (Level::High, true, true), "{:?}", c.reasons);
        assert!(c.reasons[0].starts_with(&format!("{head} has no test merge with {base}: ")), "{:?}", c.reasons);
        // The same change from the base merges: low, and no rebase is the next step.
        git(top, &["checkout", "-q", "-b", "child", &base]);
        std::fs::write(top.join("docs/a.md"), "b\n").unwrap();
        git(top, &["commit", "-qam", "child"]);
        let judged = super::judge_by(top, base, vec![git(top, &["rev-parse", "HEAD"])], &config).unwrap();
        assert_eq!((judged.computed.level, judged.unmerged), (Level::Low, false));
    }
}

//! `bp gate --base <sha> --base-ref <branch> --default-ref <branch> --head <sha> --pr <n> --project <name>`: the forge
//! check `bp-gate` (37, design change 5). It runs in a plain clone with no vault, on the forge, from the base: the
//! workflow checks out the base, fetches the head as data, and never runs code from it. The level is that of what the
//! head lands on the base, by the one level function, with the rules and the signer of `.bp/risk.yml` at the base,
//! never the head's copy. A high head passes only on a pull request into the default branch, with a comment that
//! carries the human's valid signature of the approval of this project, this pull request, its base branch and this
//! head.

use std::io::Read;
use std::path::{Path, PathBuf};

use bp_change::risk::{Level, RISK_FILE};
use bp_vault::{Code, Id, Kind, Refusal};
use clap::Args;
use serde_json::{Value, json};

use crate::risk::{judge_by, rules};
use crate::signed::{Act, Block, blocks, cap, is_branch, is_sha, public_key, verify};

/// `bp gate --base <sha> --base-ref <branch> --default-ref <branch> --head <sha> --pr <n> --project <name> [--comments <path>]`.
#[derive(Args)]
pub struct Gate {
    /// The commit whose rules judge the head: the default branch's head that the check runs from.
    #[arg(long)]
    pub base: String,
    /// The branch the pull request merges into, as the forge names it. A signature names it, so a retarget needs a
    /// new one.
    #[arg(long)]
    pub base_ref: String,
    /// The repository's default branch, as the forge names it. A check passes a commit, not a pull request, and a
    /// commit that passes onto a stacked branch can then be pushed to this one, so a high head passes onto it only.
    #[arg(long)]
    pub default_ref: String,
    /// The pull request's head commit, fetched as data.
    #[arg(long)]
    pub head: String,
    /// The pull request number on the forge.
    #[arg(long)]
    pub pr: u64,
    /// The project slug that a signed approval names, as `bp install` writes it into the workflow.
    #[arg(long)]
    pub project: String,
    /// The pull request's comments, as `gh api` prints them: a JSON array of comment objects or bodies, or several
    /// arrays, one per page. `-` reads stdin. Read only for a high head.
    #[arg(long)]
    pub comments: Option<PathBuf>,
}

pub(crate) fn gate(cwd: &Path, g: &Gate) -> Result<Value, Refusal> {
    for (flag, sha) in [("--base", &g.base), ("--head", &g.head)] {
        if !is_sha(sha) {
            return Err(Refusal::usage(
                format!("{flag} {sha}"),
                format!("{flag} takes a full commit sha: bp gate --help"),
            ));
        }
    }
    for (flag, branch) in [("--base-ref", &g.base_ref), ("--default-ref", &g.default_ref)] {
        if !is_branch(branch) {
            return Err(Refusal::usage(format!("{flag} {branch:?}"), format!("{flag} takes a branch name")));
        }
    }
    let top = bp_vault::git::Repo::find(cwd)
        .ok_or_else(|| Refusal::failed(cwd.display().to_string(), "bp gate runs in a clone of the repository"))?
        .top;
    let config = rules(&top, &g.base, &g.base, std::slice::from_ref(&g.head))?;
    // The workflow built this binary from the pin; a base with no pin has no gate, so it fails closed.
    if config.gate.is_none() {
        return Err(Refusal::new(
            Code::RiskFileInvalid,
            format!("{}:{RISK_FILE}", g.base),
            "the forge check builds bp from the Blueprint commit that `gate` in .bp/risk.yml on the default branch \
             names (37, design change 5)",
            "bp install writes the line `gate: <sha>` into .bp/risk.yml; commit it to the default branch from a shell",
        ));
    }
    let judged = judge_by(&top, g.base.clone(), vec![g.head.clone()], &config)?;
    let level = judged.computed.level;
    let result = |approval: &str| {
        json!({
            "level": level,
            "base": g.base,
            "head": g.head,
            "pull_request": g.pr,
            "reasons": judged.computed.reasons,
            "approval": approval,
        })
    };
    if level < Level::High {
        return Ok(result("not_needed"));
    }
    // Each refusal names the level and its reasons: only a pass prints them (review of #99, row 31).
    let why = format!("; the head reads {}: {}", level.as_str(), judged.computed.reasons.join("; "));
    let with_why = |mut r: Refusal| {
        r.fix.push_str(&why);
        r
    };
    if g.base_ref != g.default_ref {
        return Err(with_why(Refusal::new(
            Code::ApprovalMissing,
            format!("pull request {} at {}", g.pr, g.head),
            "a high pull request passes the forge check only into the default branch: a pass belongs to the commit, \
             and the commit could then be pushed to the default branch (37, design change 5)",
            format!(
                "retarget pull request {} from {} to {} once the lower layers land, then the human runs bp approve \
                 <issue> --post from a shell and reruns the bp-gate check",
                g.pr, g.base_ref, g.default_ref
            ),
        )));
    }
    let Some(signer) = config.signer.as_deref().and_then(public_key) else {
        return Err(with_why(Refusal::new(
            Code::SignerMissing,
            format!("{}:{RISK_FILE}", g.base),
            "a high pull request passes the forge check only with a signature by the key that .bp/risk.yml on the \
             base names (37, design change 5)",
            "add the line `signer: <the .pub file's type and key>` to .bp/risk.yml on the default branch, from a shell",
        )));
    };
    // A head with no test merge reads high because what lands is not known; the forge cannot merge it either, so the
    // next step is a rebase, not a signature.
    let next = if judged.unmerged {
        format!("rebase the head onto {} and push it; the push reruns the bp-gate check", g.default_ref)
    } else {
        format!(
            "the human runs bp approve <issue> --post from a shell, at head {}, then reruns the bp-gate check",
            g.head
        )
    };
    let bodies = match &g.comments {
        Some(path) => bodies(path)?,
        None => Vec::new(),
    };
    signed(&bodies.iter().flat_map(|b| blocks(b)).collect::<Vec<_>>(), g, &signer, next).map_err(with_why)?;
    Ok(result("signed"))
}

/// `Ok` when one block is the approval of this project, pull request, base branch and head, and its signature by `signer`
/// verifies against the message rebuilt from those values. Else `signature_invalid` when a block names this head and
/// does not verify, or `approval_missing`, which names the heads the pull request's blocks sign. `fix` is the next step.
fn signed(blocks: &[Block], g: &Gate, signer: &str, fix: String) -> Result<(), Refusal> {
    let pr = g.pr.to_string();
    let ours: Vec<&Block> = blocks
        .iter()
        .filter(|b| b.get("act") == Some("approve") && b.get("project") == Some(&g.project))
        .filter(|b| b.get("pull_request") == Some(&pr))
        .collect();
    let at_head: Vec<&&Block> = ours.iter().filter(|b| b.get("head") == Some(&g.head)).collect();
    for b in &at_head {
        // The node is the one field the gate cannot know: it must be an issue of this project, and it is signed.
        let Some(node) = b.get("node").and_then(|n| Id::parse(n).ok()) else { continue };
        if !Kind::Issue.fits(&node) || node.project() != Some(g.project.as_str()) {
            continue;
        }
        let act = Act { verb: "approve", node: &node, pull_request: g.pr, base: &g.base_ref, head: &g.head, more: &[] };
        let message = act.message();
        if let Some(sig) = &b.signature
            && verify(&message, sig, signer)?
        {
            return Ok(());
        }
    }
    if !at_head.is_empty() {
        return Err(Refusal::new(
            Code::SignatureInvalid,
            format!("pull request {} at {}", g.pr, g.head),
            "an approval counts only with a signature, by the signer that .bp/risk.yml on the base names, of this \
             project, pull request, base branch and head (37, design change 5)",
            fix,
        ));
    }
    let heads: Vec<&str> = ours.iter().filter_map(|b| b.get("head")).collect();
    let stale = if heads.is_empty() { String::new() } else { format!("; the comments sign {}", heads.join(", ")) };
    Err(Refusal::new(
        Code::ApprovalMissing,
        format!("pull request {} at {}", g.pr, g.head),
        "a high pull request passes the forge check only with the human's signed approval of its head (37, design \
         change 5)",
        format!("{fix}{stale}"),
    ))
}

/// The comment bodies of `gh api` output: one or more JSON values, each an array of comment objects with a `body`, or
/// of strings. `gh api --paginate` prints one array per page.
fn bodies(path: &Path) -> Result<Vec<String>, Refusal> {
    let subject = path.display().to_string();
    // The workflow writes the file, so a bad one is a failed or cut-short read of the forge: the rerun reads it again.
    let failed = |why: String| {
        let fix = format!("{why}; rerun the bp-gate check, whose step `read the comments` writes {subject}");
        Refusal::failed(&subject, fix)
    };
    let mut text = String::new();
    let read = if path == Path::new("-") {
        std::io::stdin().read_to_string(&mut text).map(drop)
    } else {
        std::fs::read_to_string(path).map(|t| text = t)
    };
    read.map_err(|e| failed(e.to_string()))?;
    let mut out = Vec::new();
    for value in serde_json::Deserializer::from_str(&text).into_iter::<Value>() {
        let value = value.map_err(|e| failed(format!("the comments are not JSON: {e}")))?;
        let items = match value {
            Value::Array(items) => items,
            other => vec![other],
        };
        for item in items {
            match item.get("body").unwrap_or(&item) {
                Value::String(body) => out.push(body.clone()),
                Value::Null => {}
                // One cut of the value: a whole page would put every comment in the envelope.
                other => return Err(failed(format!("an item holds no comment body: {}", cap(&other.to_string())))),
            }
        }
    }
    Ok(out)
}

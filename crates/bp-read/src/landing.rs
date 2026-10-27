//! `land`'s refusals (26, design change 4; 32, design change 5): the first unmet gate, a missing pull request, an
//! unset reviewer and an unclaimed issue, each as the envelope `land` prints. `land` and the Change page build them
//! here, so the page names the code and the fix `land` would give at the same head (21, rule 1).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use bp_change::intents::Unmet;
use bp_forge::PullRequest;
use bp_vault::{Code, Id, Refusal};

/// The claim's branch: `<project>/<slug>` (26, design change 2).
pub fn branch(id: &Id) -> String {
    format!("{}/{}", id.project().unwrap_or_default(), id.as_str().rsplit('/').next().unwrap_or_default())
}

/// The fix for an issue at `status` with no active claim: the verb that moves it to where its next verb starts.
pub fn unclaimed(id: &Id, status: Option<&str>) -> String {
    match status {
        Some("open") => format!("bp cut {id}, then rerun"),
        Some("abandoned") => format!("bp transition {id} open, then bp cut {id}"),
        Some("in-progress") => format!("{id} is in-progress with no active claim (21, check 2); no verb moves it yet"),
        Some(s) => format!("an issue at {s} is not claimed again; bp show {id}"),
        None => format!("cut claims an issue; bp show {id}"),
    }
}

/// `illegal_transition`: `pr` and `land` act on an issue under an active claim.
pub fn unheld(id: &Id, status: Option<&str>) -> Refusal {
    Refusal::new(
        Code::IllegalTransition,
        id.as_str(),
        "pr and land act on an issue under an active claim; cut makes it (21, claim)",
        unclaimed(id, status),
    )
}

/// `reviewer_invalid`: the project names no reviewer, or the token's own login (`why`).
pub fn reviewer_invalid(id: &Id, why: &str) -> Refusal {
    let project = id.project().unwrap_or_default();
    Refusal::new(
        Code::ReviewerInvalid,
        format!("projects/{project}/project"),
        "land counts only the reviewer identity's approval, a login other than the token's (26, design change 3)",
        format!(
            "{why}; the human sets the second identity's forge login: bp set projects/{project}/project reviewer <login>"
        ),
    )
}

/// `review_missing`: the claim's branch has no open or merged pull request.
pub fn no_pull_request(id: &Id) -> Refusal {
    Refusal::new(
        Code::ReviewMissing,
        id.as_str(),
        "land merges an open pull request the reviewer identity approved (15, Controls 1)",
        format!("{} has no open pull request; bp pr {id}, then the reviewer approves it", branch(id)),
    )
}

/// The fix of any refusal on a merged pull request: no approval or run can follow a merge, so the human judges a
/// change that skipped a gate.
pub fn after_merge(number: u64, id: &Id) -> String {
    format!(
        "pull request {number} is merged, so nothing can meet this now and nothing is recorded; the human reviews the \
         merge, then bp release {id} --reason lost"
    )
}

/// The first unmet part of needs landing as its refusal (26, design change 4). `clone` is the clone's top and the
/// claim's `base`, the commit its diff starts at: with both, a missing approval names `diff_changed` when the
/// reviewer's latest approval saw another diff.
pub fn refusal(unmet: &Unmet, id: &Id, pr: &PullRequest, reviewer: &str, clone: Option<(&Path, &str)>) -> Refusal {
    let (n, head) = (pr.number, &pr.head_ref_oid);
    match unmet {
        Unmet::Unmerged(below) => Refusal::new(
            Code::BaseUnmerged,
            id.as_str(),
            "a layer lands after the layer it was cut from (15, a dependency chain to landed)",
            format!("bp land {below} first: it moves pull request {n} onto its base; then bp land {id}"),
        ),
        Unmet::Mismatch(base) => Refusal::new(
            Code::BaseMismatch,
            id.as_str(),
            "land merges into the claim's base branch only (26, design change 4)",
            format!(
                "pull request {n} targets {}; bp pr {id} retargets it to {base}, then bp land {id}",
                pr.base_ref_name.as_deref().unwrap_or_default()
            ),
        ),
        Unmet::Review => unapproved(id, pr, reviewer, clone),
        Unmet::Checks => Refusal::new(
            Code::ChecksPending,
            id.as_str(),
            "the forge check at the head is green (15, Controls 3)",
            if pr.has_failed() {
                format!("a check of pull request {n} failed at {head}; fix it, commit it, then bp pr {id}")
            } else {
                format!("wait for the checks of pull request {n} at {head} to pass, then bp land {id}")
            },
        ),
        Unmet::Failed(run) => Refusal::new(
            Code::CheckFailed,
            run.id.as_str(),
            "every error-severity check block passes at the head (26, design change 4)",
            format!(
                "{} did not pass at {head}; read it with bp show {}, fix the cause, commit it, then bp pr {id}",
                run.check, run.id
            ),
        ),
        Unmet::Evidence(block) => Refusal::new(
            Code::EvidenceStale,
            id.as_str(),
            "land needs a passing run of each error block's current text at the head (26, design change 4)",
            format!("{block} has no run of its current text at {head}; bp pr {id}, then bp land {id}"),
        ),
    }
}

/// No approval at the head: `review_missing`, or `diff_changed` when the reviewer's latest approval, at an earlier
/// head, saw another diff. The comparison only names the refusal; the forge merges an approval of the latest push
/// alone (15, Controls 1).
fn unapproved(id: &Id, pr: &PullRequest, reviewer: &str, clone: Option<(&Path, &str)>) -> Refusal {
    let (n, head) = (pr.number, &pr.head_ref_oid);
    let rule = "land counts only the reviewer identity's approving review at the head (15, Controls 1)";
    let again = format!("the human approves pull request {n} at {head} as {reviewer}, then bp land {id}");
    let Some(&earlier) = pr.approved_heads(reviewer).last() else {
        return Refusal::new(Code::ReviewMissing, id.as_str(), rule, again);
    };
    match clone.and_then(|(top, from)| diff_unchanged(top, from, earlier, pr)) {
        Some(false) => Refusal::new(
            Code::DiffChanged,
            id.as_str(),
            "the diff changed since the reviewer identity's latest approval (15, Controls 1)",
            format!("{reviewer} approved {earlier}, and the diff at {head} differs; {again}"),
        ),
        Some(true) => {
            Refusal::new(Code::ReviewMissing, id.as_str(), rule, format!("diff unchanged since {earlier}; {again}"))
        }
        None => Refusal::new(
            Code::ReviewMissing,
            id.as_str(),
            rule,
            format!("{reviewer} approved {earlier}, which this clone cannot compare; {again}"),
        ),
    }
}

/// Whether the diff the reviewer approved at `reviewed`, against the claim's base `from`, is the head's against the
/// base now, by `git patch-id --verbatim`, whitespace kept: `rm -rf /x/cache` and `rm -rf /x/ cache` differ. `None`
/// when the clone at `top` lacks a commit or a diff is empty. It only names the refusal: no earlier approval counts.
pub fn diff_unchanged(top: &Path, from: &str, reviewed: &str, p: &PullRequest) -> Option<bool> {
    let base = p.base_ref_name.as_ref()?;
    let id = |range: String| -> Option<String> {
        let diff = Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["diff", "--no-ext-diff", "--no-color", "--end-of-options", &range])
            .output()
            .ok()?;
        if !diff.status.success() || diff.stdout.is_empty() {
            return None;
        }
        let mut child = Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["patch-id", "--verbatim"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(&diff.stdout).ok()?;
        let out = child.wait_with_output().ok()?;
        String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_string)
    };
    let reviewed = id(format!("{from}...{reviewed}"))?;
    Some(reviewed == id(format!("refs/remotes/origin/{base}...{}", p.head_ref_oid))?)
}

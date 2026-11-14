//! `land`'s refusals (26, design change 4; 32, design change 5; 37, design change 3): the first unmet gate, the check
//! set included, a missing pull request and an unclaimed issue, each as the envelope `land` prints. `land` and the
//! Change page build them here, so the page names the code and the fix `land` would give at the same head (21, rule
//! 1).

use bp_change::intents::Unmet;
use bp_change::rounds;
use bp_forge::PullRequest;
use bp_vault::{Code, Id, Refusal};

/// The claim's branch: `<project>/<slug>` (26, design change 2).
pub fn branch(id: &Id) -> String {
    format!("{}/{}", id.project().unwrap_or_default(), id.as_str().rsplit('/').next().unwrap_or_default())
}

/// The fix for an issue at `status` with no active claim: the verb that moves it to where its next verb starts.
pub fn unclaimed(id: &Id, status: Option<&str>) -> String {
    match status {
        Some("open") => format!("dispatch a worker to run `bp cut {id}`, then rerun"),
        Some("abandoned") => format!("bp transition {id} open, then dispatch a worker to run `bp cut {id}`"),
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

/// `review_missing`: the claim's branch has no open or merged pull request.
pub fn no_pull_request(id: &Id) -> Refusal {
    Refusal::new(
        Code::ReviewMissing,
        id.as_str(),
        "land merges an open pull request whose head the review read (37, design change 3)",
        format!("{} has no open pull request; dispatch a worker to run `bp pr {id}`; then its review", branch(id)),
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

/// The first unmet part of needs landing as its refusal (26, design change 4; 37, design change 3).
pub fn refusal(unmet: &Unmet, id: &Id, pr: &PullRequest) -> Refusal {
    let (n, head) = (pr.number, &pr.head_ref_oid);
    // The session lands, and `pr` is the worker's verb, so a fix that needs it names the dispatch (review of #100, C3).
    let pr_step = format!("dispatch a worker to run `bp pr {id}`");
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
                "pull request {n} targets {}; {pr_step}, which retargets it to {base}; then bp land {id}",
                pr.base_ref_name.as_deref().unwrap_or_default()
            ),
        ),
        Unmet::Checks => Refusal::new(
            Code::ChecksPending,
            id.as_str(),
            "the forge check at the head is green (15, Controls 3)",
            if pr.has_failed() {
                format!(
                    "a check of pull request {n} failed at {head}; dispatch a worker to fix it, commit it and run \
                     `bp pr {id}`"
                )
            } else {
                format!("wait for the checks of pull request {n} at {head} to pass, then bp land {id}")
            },
        ),
        Unmet::Failed(run) => Refusal::new(
            Code::CheckFailed,
            run.id.as_str(),
            "every error-severity check block passes at the head (26, design change 4)",
            format!(
                "{} did not pass at {head}; read it with bp show {}; dispatch a worker to fix the cause, commit it and \
                 run `bp pr {id}`",
                run.check, run.id
            ),
        ),
        Unmet::Evidence(block) => Refusal::new(
            Code::EvidenceStale,
            id.as_str(),
            "land needs a passing run of each error block's current text at the head (26, design change 4)",
            format!("{block} has no run of its current text at {head}; {pr_step}; then bp land {id}"),
        ),
        Unmet::Lenses { round, missing } => {
            let lenses: Vec<&str> = missing.iter().map(|l| l.as_str()).collect();
            Refusal::new(
                Code::ReviewMissing,
                id.as_str(),
                "land needs a review of each lens of the head's round at the level in force (37, design changes 3 \
                 and 6)",
                format!(
                    "round {round} at {head}: missing lenses: {}; dispatch bp-reviewer for each, record each review, \
                     then bp land {id}",
                    lenses.join(", ")
                ),
            )
        }
        Unmet::Unanswered(open) => Refusal::new(
            Code::FindingsOpen,
            id.as_str(),
            "land needs a disposition of each finding of each review, at any head (37, design change 3)",
            format!(
                "no disposition: {}; dispatch bp-worker with its respond template, then bp land {id}",
                rounds::named(open, head)
            ),
        ),
        Unmet::Holding(rows) => Refusal::new(
            Code::FindingsOpen,
            id.as_str(),
            "an error row holds the land until a verify review at a later head does not report it again (37, design \
             change 6)",
            format!(
                "{} hold the land; dispatch a worker to fix each and run `bp pr {id}`, then record its verify review; \
                 a decline or a round cap goes to the human",
                rounds::cited(rows)
            ),
        ),
        Unmet::Approval => Refusal::new(
            Code::ApprovalMissing,
            id.as_str(),
            "a high pull request lands only with the human's signed approval of its head (37, design change 3)",
            format!("the human runs bp approve {id} from a shell, at head {head}; then bp land {id}"),
        ),
        // The reviews read the head, so a block changed since then is one they have not seen.
        Unmet::ChecksChanged => Refusal::new(
            Code::ChecksChanged,
            id.as_str(),
            "land merges a head that names the error blocks the pull request carries now (27, design change 8)",
            format!(
                "an error block of {id}, its milestone or a carried delta changed after {head} was pushed; {pr_step}, \
                 which pushes a commit that names them; then its verify review, then bp land {id}"
            ),
        ),
    }
}

//! The master safety net (37, design change 11). A failed check at the base branch's head, on a commit that the forge
//! merged as a pull request that a land record of the project names, makes the project `master-red`. While a red
//! holds, `master-red` is the first intent of each live issue of the project but its revert issue, and `land` refuses
//! every pull request of it but the exact revert. The revert, an end record that `land` writes when git shows the red
//! reverted, or the human's signed clear ends the red. Pure: the caller reads the records, the forge and the trees.

/// A master-red record, reduced: the red commit, the pull request whose merge made it and the checks that failed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Red {
    /// The record's id.
    pub id: String,
    /// The red commit: the base branch's head when `land` read the failed checks.
    pub sha: String,
    /// The pull request whose merge is the red commit.
    pub pull_request: u64,
    /// The base branch the red commit heads.
    pub base: String,
    /// The names of the checks that failed there. A clear signs them, so a new failure at the sha needs a new clear.
    pub checks: Vec<String>,
    /// The issue whose land merged the red commit.
    pub issue: String,
    /// The red commit is itself the land of a revert: no revert of it skips review, so only the human's clear ends it.
    pub revert: bool,
    /// The live issue whose label `reverts` names the red, which the intents read; `land` judges the revert from git.
    pub revert_issue: Option<String>,
}

/// The red that holds: the earliest of `reds`, in record order, that `ended` does not end. A revert of its sha, or the
/// human's clear of it, ends one; the caller decides which records count for each reader.
pub fn red_in_force(reds: &[Red], ended: impl Fn(&Red) -> bool) -> Option<&Red> {
    reds.iter().find(|r| !ended(r))
}

/// What `land` read of a pull request and the base branch while a red holds.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    /// The base branch's head now, as `git ls-remote` names it.
    pub tip: &'a str,
    /// The commit of the base branch that the forge holds the pull request against (`baseRefOid`).
    pub base: &'a str,
    /// The pull request's head descends from the red commit.
    pub descends: bool,
    /// The tree of the pull request's head.
    pub tree: &'a str,
    /// The tree of the red commit's first parent: the base branch before the merge.
    pub before: &'a str,
    /// The forge shows a failed check at `tip` now, and the forge merged `tip` as a pull request that a land record of
    /// the project names: the red is not a record alone.
    pub failing: bool,
}

/// Whether a revert of `red` can skip its review at all: the red still heads the base branch, still fails on the
/// forge, and is no revert itself. Else only the human's clear ends it (37, design change 11).
pub fn revertible(red: &Red, c: &Candidate) -> bool {
    c.failing && c.tip == red.sha && !red.revert
}

/// The pull request is the exact revert of `red`: [`revertible`], based on the red commit, and its head tree is the
/// tree before the merge, byte for byte (git names a tree by its content). A revert restores a tree that the base
/// branch already held, so it needs no review. A master-red record alone, which an agent can write, only refuses: it
/// never lets a pull request skip its review. A revert of a revert would put the red merge back, so it never skips.
pub fn reverts(red: &Red, c: &Candidate) -> bool {
    revertible(red, c) && c.base == red.sha && c.descends && !c.tree.is_empty() && c.tree == c.before
}

/// `land`'s rule while the project may be red (37, design change 11): `Ok(None)` with no red, so the pull request
/// meets every gate; `Ok(Some(red))` for the exact revert of the red, which meets every gate but the review gates;
/// else `Err(red)`, which `land` refuses `master_red`.
pub fn landable<'a>(red: Option<&'a Red>, c: &Candidate) -> Result<Option<&'a Red>, &'a Red> {
    match red {
        None => Ok(None),
        Some(r) if reverts(r, c) => Ok(Some(r)),
        Some(r) => Err(r),
    }
}

/// The note of the `master-red` intent: the red, and the one act that ends it in its case. A red that is itself a
/// revert ends at the human's clear only. Else its revert issue, when one names it, lands with no review; with none,
/// the session creates one.
pub fn note(red: &Red) -> String {
    let project = red.id.split('/').nth(1).unwrap_or_default();
    let short = &red.sha[..red.sha.len().min(12)];
    let checks = if red.checks.is_empty() { "a check".to_string() } else { red.checks.join(", ") };
    let red_at =
        format!("master red at {short}: {checks} failed after pull request {} of {}", red.pull_request, red.issue);
    let clear = format!("the human runs bp clear {project}");
    if red.revert {
        return format!("{red_at}, itself a revert; no revert lands with no review, so only {clear}");
    }
    match &red.revert_issue {
        Some(issue) => format!("{red_at}; its revert issue {issue} lands with no review; or {clear}"),
        None => format!(
            "{red_at}; no revert issue names it: create one with --field reverts={}, then a worker lands its exact revert \
             with no review; or {clear}",
            red.sha
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Intent, Issue, PullRequest, issue, landing};
    use super::*;
    use crate::risk::Level;

    const RED: &str = "1111111111111111111111111111111111111111";
    const NEXT: &str = "2222222222222222222222222222222222222222";
    const BEFORE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn red() -> Red {
        Red {
            id: "projects/bp/reds/r".into(),
            sha: RED.into(),
            pull_request: 7,
            base: "master".into(),
            issue: "projects/bp/issues/x".into(),
            ..Red::default()
        }
    }

    fn revert() -> Candidate<'static> {
        Candidate { tip: RED, base: RED, descends: true, tree: BEFORE, before: BEFORE, failing: true }
    }

    #[test]
    fn master_red_refuses_land_except_exact_revert() {
        let r = red();
        assert_eq!(landable(None, &revert()), Ok(None), "no red: every pull request meets the usual gates");
        assert_eq!(landable(Some(&r), &revert()), Ok(Some(&r)), "the exact revert lands");
        // A tree that differs by one byte is another tree object: git names a tree by its content.
        let byte = Candidate { tree: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab", ..revert() };
        let other = [
            ("a tree that differs by one byte", byte),
            ("a pull request against another base", Candidate { base: NEXT, ..revert() }),
            ("a base branch that moved past the red", Candidate { tip: NEXT, ..revert() }),
            ("a head that does not descend from the red", Candidate { descends: false, ..revert() }),
            ("no tree read", Candidate { tree: "", before: "", ..revert() }),
            ("a red the forge no longer shows: a record alone", Candidate { failing: false, ..revert() }),
        ];
        for (why, c) in other {
            assert_eq!(landable(Some(&r), &c), Err(&r), "{why}");
        }
        // A red that is itself the land of a revert takes no revert exception: its revert would put the red merge
        // back with no review, and the loop would not stop. Only the human's clear ends it.
        let looped = Red { revert: true, ..red() };
        assert_eq!(landable(Some(&looped), &revert()), Err(&looped), "the revert of a revert");
        assert!(!revertible(&looped, &revert()) && revertible(&r, &revert()));
        assert!(note(&looped).contains("only the human runs bp clear bp"), "{}", note(&looped));

        // A red ends at a revert or a clear; the next one, in record order, then holds.
        let later = Red { id: "projects/bp/reds/s".into(), sha: NEXT.into(), ..red() };
        let reds = [r.clone(), later.clone()];
        assert_eq!(red_in_force(&reds, |_| false), Some(&r), "the earliest holds");
        assert_eq!(red_in_force(&reds, |x| x.sha == RED), Some(&later));
        assert_eq!(red_in_force(&reds, |_| true), None);

        // While red, every live issue of the project reads master-red first, before blocked and ready: an issue whose
        // run failed at its head, which reads blocked without the red, reads master-red. It routes nothing: it waits.
        let pr = PullRequest { head: NEXT.into(), base: Some("master".into()), green: true, failed: false };
        let failed = super::super::Run { commit: NEXT.into(), error: true, failed: true, ..Default::default() };
        let blocked =
            Issue { status: "in-progress".into(), pr: Some(pr.clone()), runs: vec![failed], ..Issue::default() };
        assert_eq!(issue(&blocked, 0).intent, Some(Intent::Blocked), "no red: blocked");
        let i = Issue { red: Some(r.clone()), ..blocked.clone() };
        let v = issue(&i, 0);
        assert_eq!((v.intent, v.route), (Some(Intent::MasterRed), None));
        assert!(v.note.contains(RED) && v.note.contains("bp clear bp"), "{}", v.note);
        assert!(v.note.contains("--field reverts="), "no revert issue yet: the note says how to make one: {}", v.note);
        let named = Red { revert_issue: Some("projects/bp/issues/undo".into()), ..r.clone() };
        assert!(note(&named).contains("its revert issue projects/bp/issues/undo"), "{}", note(&named));
        // The revert issue reads its own intents, routed by its own level: ready, so the session dispatches its build.
        let undo =
            Issue { status: "open".into(), red: Some(named.clone()), revert: Some(RED.into()), ..Issue::default() };
        let v = issue(&undo, 0);
        assert_eq!(v.intent, Some(Intent::Ready), "{}", v.note);
        assert!(v.route.is_some(), "the revert issue's build is routed by its level");

        // The revert meets no review gate: a high head with no review lands once its forge checks are green.
        let high = Issue { declared: Some(Level::High), pr: Some(pr.clone()), ..Issue::default() };
        assert!(landing(&high, &pr).is_err(), "a high head with no review does not land");
        let undone = Issue { revert: Some(RED.into()), ..high.clone() };
        assert_eq!(landing(&undone, &pr), Ok(()), "its revert does");
        let red_checks = PullRequest { green: false, failed: true, ..pr.clone() };
        assert!(landing(&undone, &red_checks).is_err(), "the forge checks still hold at the revert's head");
    }
}

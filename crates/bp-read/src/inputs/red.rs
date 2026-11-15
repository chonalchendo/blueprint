//! A project's master-red records as the intents read them (37, design change 11): each red, and the one that holds
//! while no clear record names it and no land record names its revert. The intents verify no signature and read no
//! forge, so a hand-written clear changes the intent only: `land` verifies the clear and judges the revert from git.

use bp_change::intents::{self, Red};
use bp_vault::{Filter, Index, Kind, Record, Refusal};
use serde_json::Value;

use super::text;

/// A master-red record, reduced; `None` when it names no full sha, which no `land` writes.
pub fn red_of(r: &Record) -> Option<Red> {
    let sha = text(&r.header, "red").filter(|s| s.len() >= 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let checks = r.header.get("checks").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
    Some(Red {
        id: r.id.clone(),
        sha,
        pull_request: r.header.get("pull_request").and_then(Value::as_u64).unwrap_or_default(),
        base: text(&r.header, "base").unwrap_or_default(),
        checks: checks.map(str::to_string).collect(),
    })
}

/// The master-red records of `project`, in record order.
pub fn reds(index: &Index, project: &str) -> Result<Vec<Red>, Refusal> {
    Ok(index.records(&format!("projects/{project}/project"), Kind::MasterRed)?.iter().filter_map(red_of).collect())
}

/// The red of `project` that the intents read as holding: the earliest whose sha no clear record and no land record's
/// `reverts` line names. The claims are read only while a red has no clear record, which is rare.
pub fn red(index: &Index, project: &str) -> Result<Option<Red>, Refusal> {
    let reds = reds(index, project)?;
    let clears = index.records(&format!("projects/{project}/project"), Kind::Clear)?;
    let cleared = |r: &Red| clears.iter().any(|c| text(&c.header, "red").as_deref() == Some(r.sha.as_str()));
    if intents::red_in_force(&reds, cleared).is_none() {
        return Ok(None);
    }
    let reverted = reverted(index, project)?;
    Ok(intents::red_in_force(&reds, |r| cleared(r) || reverted.contains(&r.sha)).cloned())
}

/// The red shas that a land record of `project` names in its `reverts` line: a claim released `landed` by the revert.
pub fn reverted(index: &Index, project: &str) -> Result<Vec<String>, Refusal> {
    let f = Filter { kind: Kind::Claim, status: None, project: Some(project), under: None, limit: crate::ALL };
    let mut issues: Vec<String> = index.list(&f)?.0.into_iter().filter_map(|row| row.under).collect();
    issues.dedup();
    let mut out = Vec::new();
    for issue in issues {
        for claim in index.records(&issue, Kind::Claim)? {
            let release = intents::release(&claim.body);
            if release.get("reason").map(String::as_str) == Some("landed") {
                out.extend(release.get("reverts").cloned());
            }
        }
    }
    Ok(out)
}

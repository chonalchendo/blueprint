//! Composes the `Intents` function's inputs from the index and the forge snapshot. The function itself is pure
//! and lives in `bp-change`; this is the IO around it (23, rule 2).

use bp_change::intents::{self, Claim, Dependency, Issue, PullRequest, Verdict};
use bp_forge::Snapshot;
use bp_vault::{Index, Kind, Refusal};
use serde_json::{Map, Value};

/// The clock and the forge one call reads.
pub struct World {
    /// Unix seconds. `BP_NOW` fixes it for parity runs and replays, which must see the clock the reference saw.
    pub now: i64,
    /// `None` when the forge was not read.
    pub forge: Option<Snapshot>,
}

impl World {
    /// Reads `BP_NOW` and `BP_FORGE_SNAPSHOT`. The live forge read lands with the repo verbs.
    pub fn read() -> Result<Self, Refusal> {
        let now = match std::env::var("BP_NOW") {
            Ok(s) => intents::timestamp(&s).ok_or_else(|| {
                Refusal::failed(format!("BP_NOW={s}"), "set BP_NOW to YYYY-MM-DDTHH:MM:SSZ, or unset it")
            })?,
            Err(_) => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
        };
        let forge = match std::env::var_os("BP_FORGE_SNAPSHOT") {
            Some(dir) => Some(Snapshot::from_dir(std::path::Path::new(&dir)).map_err(|e| {
                Refusal::failed(
                    format!("BP_FORGE_SNAPSHOT={}", dir.to_string_lossy()),
                    format!("{e}; point BP_FORGE_SNAPSHOT at a gh JSON directory"),
                )
            })?),
            None => None,
        };
        Ok(Self { now, forge })
    }

    fn pr(&self, id: &str) -> Option<PullRequest> {
        let pr = self.forge.as_ref()?.open_for(id.rsplit('/').next()?)?;
        Some(PullRequest { head: pr.head_ref_oid.clone(), green: pr.is_green(), approved: pr.is_approved() })
    }
}

/// The intent of one issue or milestone row.
pub fn of(index: &Index, world: &World, kind: Kind, id: &str, status: Option<&str>) -> Result<Verdict, Refusal> {
    let status = status.unwrap_or_default();
    if kind == Kind::Milestone {
        return Ok(intents::milestone(status));
    }
    if !intents::is_live_issue(status) {
        return Ok(intents::issue(&Issue { status: status.into(), ..Issue::default() }, world.now));
    }
    let header = index.header(id)?.unwrap_or_default();
    let mut depends = Vec::new();
    for d in header.get("depends").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let target = index.header(d)?;
        depends.push(Dependency {
            id: d.to_string(),
            resolved: target.as_ref().and_then(|h| h.get("status")).and_then(Value::as_str) == Some("resolved"),
            open_pr: world.pr(d).is_some(),
            exists: target.is_some(),
        });
    }
    let text = |m: &Map<String, Value>, k: &str| m.get(k).and_then(Value::as_str).map(str::to_string);
    let claims = index
        .records(id, Kind::Claim)?
        .iter()
        .map(|c| {
            let release = intents::release(&c.body);
            Claim {
                at: text(&c.header, "from").and_then(|t| intents::timestamp(&t)),
                released: release.contains_key("to"),
                escalated: release.get("reason").map(String::as_str) == Some("escalated"),
            }
        })
        .collect();
    let runs = index
        .records(id, Kind::Run)?
        .iter()
        .map(|r| intents::Run {
            at: text(&r.header, "at").and_then(|t| intents::timestamp(&t)),
            commit: text(&r.header, "commit").unwrap_or_default(),
            passed: text(&r.header, "verdict").as_deref() == Some("pass"),
        })
        .collect();
    let review_heads = index.records(id, Kind::Review)?.iter().filter_map(|r| text(&r.header, "head")).collect();
    let issue = Issue { status: status.into(), claims, runs, review_heads, depends, pr: world.pr(id) };
    Ok(intents::issue(&issue, world.now))
}

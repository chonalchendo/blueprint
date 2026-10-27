//! The stack of a Change page (18, Change 2): the layers from each claim's `base_issue`, in land order, and the live
//! issues that wait on the change outside the stack.

use std::collections::BTreeSet;

use bp_change::IssueStatus;
use bp_change::intents::Intent;
use bp_vault::{Id, Index, Kind, Record, Refusal};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use crate::Intents;
use crate::inputs::{field, held, status, text};

/// One layer of the stack.
#[derive(Serialize, TS)]
pub struct Layer {
    /// The issue's id.
    pub id: String,
    /// Its place in land order, from 1 at the bottom.
    pub layer: usize,
    /// "landed", or its intent and note in words.
    pub state: String,
    /// It is the issue the page shows.
    pub current: bool,
}

/// An intent and its note in words: "waiting: waits for 0412 to land".
pub(crate) fn verdict_words(intent: Option<Intent>, note: &str) -> String {
    let intent =
        intent.and_then(|i| Some(serde_json::to_value(i).ok()?.as_str()?.replace('-', " "))).unwrap_or_default();
    match (intent.is_empty(), note.is_empty()) {
        (true, _) => "no intent".to_string(),
        (false, true) => intent,
        (false, false) => format!("{intent}: {note}"),
    }
}

/// The stack of `id` from each claim's `base_issue` (18, Change 2), in land order, and the issues that wait on it
/// outside the stack. Below: the claim's `base_issue`, then that issue's held claim's, or its landed one's once it
/// resolved. Above: each issue whose held claim names a layer as its `base_issue`. Each walk stops at
/// [`STACK_DEPTH`](bp_change::intents::STACK_DEPTH) steps, the deepest stack `cut` makes.
pub(crate) fn stack(
    index: &Index,
    intents: &Intents,
    id: &Id,
    claim: Option<&Record>,
    words: &str,
) -> Result<(Vec<Layer>, Vec<String>), Refusal> {
    let depth = bp_change::intents::STACK_DEPTH;
    let world = intents.world.as_ref().expect("an issue's project was handed in");
    let state = |n: &str| -> Result<String, Refusal> {
        let s = status(index, n)?;
        if s.as_deref() == Some(IssueStatus::Resolved.as_str()) {
            return Ok("landed".into());
        }
        // A cut layer with no pull request stays in the stack: `land` restacks it (18, Change 2).
        if world.pr(n).is_none() {
            return Ok("no pull request".into());
        }
        let v = intents.row(Kind::Issue, n, s.as_deref())?;
        Ok(verdict_words(v.intent, &v.note))
    };
    let mut below = Vec::new();
    let mut on = claim.and_then(|c| field(c, "base_issue"));
    for _ in 0..depth {
        let Some(next) = on.take() else { break };
        let Ok(n) = Id::parse(&next) else { break };
        let c = match held(index, &n)? {
            Some(h) => Some(h),
            None => landed(index, &n)?,
        };
        on = c.and_then(|c| field(&c, "base_issue"));
        below.push((next.clone(), state(&next)?));
    }
    let mut above = Vec::new();
    let mut level = vec![id.clone()];
    for _ in 0..depth {
        let mut next = Vec::new();
        for l in &level {
            for n in index.inbound(l)? {
                let on = held(index, &n)?.and_then(|h| field(&h, "base_issue"));
                if on.as_deref() == Some(l.as_str()) {
                    above.push((n.to_string(), state(n.as_str())?));
                    next.push(n);
                }
            }
        }
        level = next;
    }
    let mut layers: Vec<(String, String, bool)> = below.into_iter().rev().map(|(i, s)| (i, s, false)).collect();
    if claim.is_some() {
        layers.push((id.to_string(), words.to_string(), true));
    }
    layers.extend(above.into_iter().map(|(i, s)| (i, s, false)));
    let stacked: BTreeSet<&str> = layers.iter().map(|(i, ..)| i.as_str()).collect();
    let mut waits = Vec::new();
    for n in index.inbound(id)? {
        let Some(h) = index.header(n.as_str())? else { continue };
        let depends = h.get("depends").and_then(Value::as_array).is_some_and(|d| d.iter().any(|t| t == id.as_str()));
        let live = text(&h, "status").is_some_and(|s| bp_change::intents::is_live_issue(&s));
        if depends && live && world.pr(n.as_str()).is_none() && !stacked.contains(n.as_str()) {
            waits.push(n.to_string());
        }
    }
    let stack = layers
        .into_iter()
        .enumerate()
        .map(|(i, (id, state, current))| Layer { id, layer: i + 1, state, current })
        .collect();
    Ok((stack, waits))
}

/// The claim `issue` landed under: the latest released `landed`.
fn landed(index: &Index, issue: &Id) -> Result<Option<Record>, Refusal> {
    let claims = index.records(issue.as_str(), Kind::Claim)?;
    Ok(claims
        .into_iter()
        .rev()
        .find(|c| bp_change::intents::release(&c.body).get("reason").map(String::as_str) == Some("landed")))
}

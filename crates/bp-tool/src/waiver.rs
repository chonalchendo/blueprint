//! Waivers (13, Waiver; 27, design change 10): which are open, the `--waive` values a stamp takes, and the records it
//! writes. `stamp` calls it; 21's re-asked intent reads [`open`] too, so the record reduction and the passing-run test
//! live here, not in the verb.

use std::collections::BTreeMap;

use bp_change::intents;
use bp_vault::{Check, Code, Filter, Id, Index, Kind, Refusal, Vault};
use serde_json::Value;

use crate::stamp::Stamp;

/// An open waiver: its id, its milestone, its block's name and the block as the finish line holds it now; `None`
/// when the block is gone, which [`refuse`] refuses.
#[derive(PartialEq)]
pub(crate) struct Reask {
    pub(crate) waiver: Id,
    pub(crate) milestone: Id,
    pub(crate) name: String,
    pub(crate) check: Option<Check>,
}

impl Reask {
    /// The `--waive` value that waives it again.
    pub(crate) fn key(&self) -> String {
        format!("{}/{}", self.milestone, self.name)
    }
}

/// The flags' refusals, before the vault is read: one `--reason` per `--waive`, each one line, each block once.
pub(crate) fn usage(s: &Stamp) -> Result<(), Refusal> {
    if s.waive.len() != s.reason.len() {
        return Err(Refusal::usage("--waive", "give each --waive <name> its own --reason <why>, in the same order"));
    }
    if s.reason.iter().any(|r| r.trim().is_empty() || r.contains(['\n', '\r'])) {
        return Err(Refusal::usage("--reason", "give each reason as one line of text"));
    }
    if let Some(w) = s.waive.iter().enumerate().find_map(|(i, w)| s.waive[..i].contains(w).then_some(w)) {
        return Err(Refusal::usage(format!("--waive {w}"), "waive each block once"));
    }
    Ok(())
}

/// The project's open waivers, by 21's re-asked predicate: a passing run of the block follows the waiver when git
/// last touched the run, not the waiver.
pub(crate) fn open(vault: &Vault, index: &Index, project: &str, now: i64) -> Result<Vec<Reask>, Refusal> {
    let all =
        Filter { kind: Kind::Milestone, status: None, project: Some(project), under: None, limit: usize::MAX >> 1 };
    let (mut waivers, mut runs) = (Vec::new(), BTreeMap::new());
    for m in index.list(&all)?.0 {
        let records = index.records(&m.id, Kind::Waiver)?;
        if records.is_empty() {
            continue;
        }
        for r in records {
            let key = r.id.rsplit('/').next().unwrap_or(&r.id);
            let field = |k: &str| r.header.get(k).and_then(Value::as_str).filter(|t| !t.is_empty());
            // A block name may end in `-<n>`, so only `check` splits the key `<check>-<n>`; 13's rule 1 on the bare
            // key would misread `walk-8` as the eighth waiver of `walk`. A waiver written by hand without it keys
            // its own block.
            let block = field("check").unwrap_or(key);
            let seq = key.strip_prefix(block).and_then(|n| n.strip_prefix('-')).and_then(|n| n.parse().ok());
            // A `valid_until` that is not an instant reads as past, so the stamp asks again.
            let valid_until = field("valid_until").map(|t| intents::timestamp(t).unwrap_or(i64::MIN));
            waivers.push(intents::Waiver {
                id: r.id.clone(),
                milestone: m.id.clone(),
                block: block.into(),
                seq: seq.unwrap_or(1),
                valid_until,
            });
        }
        runs.insert(m.id.clone(), index.records(&m.id, Kind::Run)?);
    }
    let passed_after = |w: &intents::Waiver| {
        let waiver = Id::parse(&w.id).expect("the index holds ids");
        let passed = runs[&w.milestone].iter().filter(|r| {
            let field = |k: &str| r.header.get(k).and_then(Value::as_str);
            field("check") == Some(w.block.as_str()) && field("verdict") == Some("pass")
        });
        // The latest first: it is the one that follows, if any does.
        passed.rev().any(|r| vault.touched_after(&Id::parse(&r.id).expect("the index holds ids"), &waiver))
    };
    let mut open = Vec::new();
    for w in intents::reasked(&waivers, now, passed_after) {
        let milestone = Id::parse(&w.milestone).expect("the index holds ids");
        let body = vault.read(&milestone)?.body;
        let check = bp_check::named(&body).into_iter().find(|c| c.attrs["name"] == w.block).cloned();
        let waiver = Id::parse(&w.id).expect("the index holds ids");
        open.push(Reask { waiver, milestone, name: w.block.clone(), check });
    }
    Ok(open)
}

/// The refusals of a stamp of `id` over its waivers. Each `--waive` names a block of `finish`, the milestone's finish
/// line, that can fail, or an open waiver; and each open waiver not waived again names a block its milestone holds.
pub(crate) fn refuse(id: &Id, finish: &[Check], open: &[Reask], waive: &[String]) -> Result<(), Refusal> {
    let names: Vec<&str> = finish.iter().map(|c| c.attrs["name"].as_str()).collect();
    // A block name is a slug, so a `/` names an open waiver, split at the last one (27, design change 10).
    let known = |w: &str| match w.rsplit_once('/') {
        Some((m, n)) => open.iter().any(|r| r.milestone.as_str() == m && r.name == n),
        None => names.contains(&w),
    };
    if let Some(w) = waive.iter().find(|w| !known(w)) {
        let open: Vec<String> = open.iter().map(Reask::key).collect();
        return Err(Refusal::new(
            Code::UnknownValue,
            format!("waive {w}"),
            "a waiver names a block of the milestone's finish line, or an open waiver of the project (13, Waiver)",
            format!(
                "waive one of the blocks of {id}: {}; or an open waiver: {}",
                names.join(", "),
                if open.is_empty() { "none".to_string() } else { open.join(", ") }
            ),
        ));
    }
    // A block that records `skipped` at every stamp never fails one, and no run of it ever closes its waiver.
    let skips = finish.iter().find_map(|c| {
        let name = c.attrs["name"].as_str();
        bp_check::skips(c).filter(|_| waive.iter().any(|w| w == name)).map(|why| (name, why))
    });
    if let Some((name, why)) = skips {
        return Err(Refusal::new(
            Code::UnknownValue,
            format!("waive {name}"),
            "a waiver names a block that can fail the stamp; a skipped block cannot, and its waiver would be asked \
             again at every stamp (27, design change 10)",
            format!(
                "{name} records skipped ({why}), so it needs no waiver; drop --waive {name} and rerun bp stamp {id}"
            ),
        ));
    }
    if let Some(r) = open.iter().find(|r| r.check.is_none() && !waive.contains(&r.key())) {
        return Err(Refusal::new(
            Code::UnknownValue,
            r.waiver.as_str(),
            "an open waiver's check names a block of its milestone's finish line, which stamp runs again (13, Waiver; \
             21, re-asked)",
            format!(
                "{} holds no block {}: restore it in that finish line by hand and git commit it, or waive it again: \
                 bp stamp {id} --waive {} --reason <why>",
                r.milestone,
                r.name,
                r.key()
            ),
        ));
    }
    Ok(())
}

/// Puts a waiver per `--waive` into `changes`, decided at `at`. Returns the waivers.
pub(crate) fn write(vault: &Vault, id: &Id, s: &Stamp, at: &str, changes: &mut BTreeMap<Id, String>) -> Vec<Id> {
    let mut waivers = Vec::new();
    for (name, reason) in s.waive.iter().zip(&s.reason) {
        // A waiver waived again is the next of its block, under its own milestone (13, Waiver).
        let (under, name) = match name.rsplit_once('/') {
            Some((m, n)) => (Id::parse(m).expect("the gate matched an open waiver's milestone"), n),
            None => (id.clone(), name.as_str()),
        };
        // `valid_until` stays empty, so the next stamp of the project asks again (13, Waiver).
        let waiver = crate::record::free(&vault.root, &format!("{under}/waivers/{name}"));
        let fields = [("check", name.into()), ("reason", reason.as_str().into()), ("decided_at", at.into())];
        changes.insert(waiver.clone(), crate::stamp::record(Kind::Waiver, &under, &[], fields));
        waivers.push(waiver);
    }
    waivers
}

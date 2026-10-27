//! Waivers (13, Waiver; 27, design change 10): the `--waive` values a stamp takes, and the records it writes. Which
//! waivers are open is `bp_read::inputs::open_waivers`, which 21's re-asked intent reads too.

use std::collections::BTreeMap;

use bp_read::inputs::Reask;
use bp_vault::{Check, Code, Id, Kind, Refusal, Vault};

use crate::stamp::Stamp;

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

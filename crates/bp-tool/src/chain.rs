//! A chain of deltas (34, design change 4; 13, Delta): a second `record delta` on a design under the same milestone,
//! with another `cause`, builds on the pending one and names it in `--field chains`. Its `base` is the hash of the
//! design as that delta leaves it, and its body keeps every section the chain added or changed. `stamp` walks the
//! chain in order and checks each base against the body the link before wrote, so a link that never saw a change
//! refuses `base_moved` (28, design change 8).

use std::collections::BTreeMap;

use bp_check::nodes::specs;
use bp_vault::{Body, Code, DeltaState, Id, Index, Kind, Refusal, Vault};
use serde_json::{Map, Value};

use crate::delta::{Diagrams, Specs, carried, hash, moved};

/// A proposed delta as the index holds it: its design, its id, its body (the proposed design body), its header and
/// its diagrams.
#[derive(PartialEq)]
pub(crate) struct Proposed {
    pub(crate) design: Id,
    pub(crate) delta: Id,
    pub(crate) body: String,
    pub(crate) header: Map<String, Value>,
    pub(crate) diagrams: Diagrams,
}

/// The proposed deltas of `design` under `milestone`, or its editorial one, in chain order (34, design change 4): the
/// one that chains on none first, then the one that chains on it, and so on. A delta whose `chains` names no
/// proposed delta comes next in time order, so `walk` reads its base as moved.
pub(crate) fn chain(
    vault: &Vault,
    index: &Index,
    design: &Id,
    milestone: Option<&str>,
) -> Result<Vec<Proposed>, Refusal> {
    let mut rest = Vec::new();
    for d in index.records(design.as_str(), Kind::Delta)? {
        if d.header.get("milestone").and_then(Value::as_str) == milestone
            && DeltaState::of(&Body::parse(&d.body)) == DeltaState::Proposed
        {
            let delta = Id::parse(&d.id).expect("the index holds ids");
            let diagrams = carried(vault, &delta, &d.header)?;
            rest.push(Proposed { design: design.clone(), delta, body: d.body, header: d.header, diagrams });
        }
    }
    let mut chain: Vec<Proposed> = Vec::with_capacity(rest.len());
    while !rest.is_empty() {
        let tip = chain.last().map(|l| l.delta.as_str());
        let next = rest.iter().position(|p| p.header.get("chains").and_then(Value::as_str) == tip).unwrap_or(0);
        chain.push(rest.remove(next));
    }
    Ok(chain)
}

/// Puts the diagrams of `link` over `specs`, and where each put spec lives over `homes`.
fn put(specs: &mut Specs, homes: &mut BTreeMap<String, String>, link: &Proposed) {
    for (name, spec) in &link.diagrams {
        if let Some(s) = spec {
            specs.insert(name.clone(), s.text.clone());
            homes.insert(name.clone(), s.file.display().to_string());
        } else {
            specs.remove(name);
            homes.remove(name);
        }
    }
}

/// The specs of `design` and where each lives, as committed.
fn committed(vault: &Vault, design: &Id) -> (Specs, BTreeMap<String, String>) {
    let specs: Specs = specs(vault, design).0.into_iter().collect();
    let homes = specs.keys().map(|n| (n.clone(), format!("{design}/diagrams/{n}.yaml"))).collect();
    (specs, homes)
}

/// The specs of `design` as the links before `delta` leave them, which `delta` adds to, replaces or removes: the
/// committed specs when `header`, the delta's, names no `chains`.
pub(crate) fn before(
    vault: &Vault,
    index: &Index,
    design: &Id,
    delta: &Id,
    header: &Map<String, Value>,
) -> Result<Specs, Refusal> {
    let (mut specs, mut homes) = committed(vault, design);
    if header.get("chains").is_some() {
        let chain = chain(vault, index, design, header.get("milestone").and_then(Value::as_str))?;
        chain.iter().take_while(|l| l.delta != *delta).for_each(|l| put(&mut specs, &mut homes, l));
    }
    Ok(specs)
}

/// What [`walk`] found: the hash of the design as the links before the first moved one leave it, its specs and where
/// each lives, and that link with its `base_moved` refusal. With no moved link, what a delta on the tip builds on.
pub(crate) struct Walked {
    pub(crate) base: String,
    pub(crate) specs: Specs,
    pub(crate) homes: BTreeMap<String, String>,
    pub(crate) moved: Option<(usize, Refusal)>,
}

/// Checks the base of each link of `chain`, a chain of `design`, against the hash of the design as the links before
/// it leave it: the first against the design, each next against the body the one before writes and the specs the
/// chain puts so far (28, design change 8; 34, design change 4).
pub(crate) fn walk(vault: &Vault, design: &Id, chain: &[Proposed], milestone: Option<&str>) -> Result<Walked, Refusal> {
    let text = vault.text(design)?;
    let (mut specs, mut homes) = committed(vault, design);
    let mut base = hash(&text[bp_vault::header_end(&text).expect("a design has a header")..], &specs);
    for (i, link) in chain.iter().enumerate() {
        let on = i.checked_sub(1).map(|j| &chain[j].delta);
        if let Err(r) = moved(vault, design, &link.delta, &link.header, milestone, &base, on) {
            return if r.code == Code::BaseMoved {
                Ok(Walked { base, specs, homes, moved: Some((i, r)) })
            } else {
                Err(r)
            };
        }
        put(&mut specs, &mut homes, link);
        let written = crate::set::rebody(&text, &link.body).unwrap_or_else(|| text.clone());
        base = hash(&written[bp_vault::header_end(&written).expect("a design has a header")..], &specs);
    }
    Ok(Walked { base, specs, homes, moved: None })
}

/// The `cause` of a pending delta; none for design work.
fn cause_of(p: &Proposed) -> Option<&str> {
    p.header.get("cause").and_then(Value::as_str)
}

/// Where a new delta goes: the hash and specs it builds on and where each spec lives, the delta it chains on, and the
/// stale delta it rejects.
pub(crate) struct Onto {
    pub(crate) base: String,
    pub(crate) specs: Specs,
    pub(crate) homes: BTreeMap<String, String>,
    pub(crate) chains: Option<Id>,
    pub(crate) stale: Option<Id>,
}

/// A new delta of `design`: its `milestone` (none for an editorial delta), its `cause`, the `chains` its caller named
/// and its body.
pub(crate) struct New<'a> {
    pub(crate) milestone: Option<&'a str>,
    pub(crate) cause: Option<&'a str>,
    pub(crate) given: Option<&'a str>,
    pub(crate) body: &'a str,
}

/// Where a new delta of `design` goes (13, Delta; 34, design change 4). With no delta pending, on the design. With a
/// chain pending under its milestone and no link of its cause, on the chain's tip. A first moved link of its cause
/// can never be accepted: the new delta replaces it on the links before it and closes it as rejected. Any other
/// pending case refuses `delta_pending`; an editorial delta never chains. A delta that goes on a link must name it in
/// `chains` and keep what the chain wrote ([`kept`]).
pub(crate) fn onto(vault: &Vault, index: &Index, design: &Id, new: &New) -> Result<Onto, Refusal> {
    let chain = chain(vault, index, design, new.milestone)?;
    let Walked { base, specs, homes, moved } = walk(vault, design, &chain, new.milestone)?;
    let (on, stale) = match moved {
        None if (new.milestone.is_some() || chain.is_empty()) && !chain.iter().any(|p| cause_of(p) == new.cause) => {
            (chain.last(), None)
        }
        Some((i, _)) if cause_of(&chain[i]) == new.cause => (chain[..i].last(), Some(chain[i].delta.clone())),
        moved => return Err(pending(design, new.milestone, &chain, moved)),
    };
    let tip = on.map(|p| p.delta.as_str());
    if new.given != tip {
        let fix = match tip {
            Some(tip) => format!(
                "{tip} pends under the milestone with another cause; read the tip: bp show {tip} --json; write the \
                 body on top of it; rerun with --field chains={tip}"
            ),
            None => format!("no delta of {design} pends for this one to chain on; rerun without --field chains"),
        };
        return Err(Refusal::new(
            Code::DeltaPending,
            format!("{design} {}", new.milestone.unwrap_or("editorial")),
            "a delta builds on the latest pending delta of its design and milestone and names it in `chains`, so a \
             chain never forks (34, design change 4)",
            fix,
        ));
    }
    if let Some(on) = on {
        kept(vault, design, on, new)?;
    }
    Ok(Onto { base, specs, homes, chains: on.map(|p| p.delta.clone()), stale })
}

/// The `delta_pending` refusal of a new delta that may not go on `chain`: an editorial delta pends, a delta of its
/// cause pends and has not moved, or a link of another cause moved, whose owner proposes it again first.
fn pending(design: &Id, milestone: Option<&str>, chain: &[Proposed], moved: Option<(usize, Refusal)>) -> Refusal {
    let fix = match moved {
        None if milestone.is_none() => format!(
            "the human accepts the pending editorial delta first: bp stamp {}; then propose this one",
            chain[0].delta
        ),
        None => format!(
            "the pending delta carries the change until the milestone is stamped: bp list delta --under {design}; \
             read the chain's tip: bp show {} --json",
            chain[chain.len() - 1].delta
        ),
        Some((i, r)) => format!(
            "{} can never be stamped, and its cause is {}, not this one; its owner proposes it again: {}",
            chain[i].delta,
            cause_of(&chain[i]).unwrap_or("design work"),
            r.fix
        ),
    };
    Refusal::new(
        Code::DeltaPending,
        format!("{design} {}", milestone.unwrap_or("editorial")),
        "a design carries one chain of proposed deltas per milestone, one delta per cause, and one editorial delta \
         (13, Delta)",
        fix,
    )
}

/// Refuses `chain_reverts` when the new delta's body undoes a `## ` section that `on`, the link it goes on, holds
/// against the committed design: one the chain added that the body drops, or one the chain changed that the body
/// puts back as committed. The stamp writes the tip's body in whole, so such a body would drop the earlier change
/// without a word (34, design change 4).
fn kept(vault: &Vault, design: &Id, on: &Proposed, new: &New) -> Result<(), Refusal> {
    let committed = vault.read(design)?.body;
    let (chain, body) = (Body::parse(&on.body), Body::parse(new.body));
    let text = |b: &Body, h: &str| b.sections_named(h).first().map(|s| s.text.clone());
    for section in &chain.sections {
        let (was, now) = (text(&committed, &section.heading), text(&body, &section.heading));
        let what = match (was, now) {
            (None, None) => "adds the section and this body drops it",
            (Some(was), Some(now)) if was != section.text && now == was => {
                "changes the section and this body puts back the committed text"
            }
            _ => continue,
        };
        let (delta, heading) = (&on.delta, &section.heading);
        let mut again = vec![format!("bp record delta {design} {}", crate::delta::carrier(new.milestone))];
        again.extend(new.cause.map(|c| format!("--field cause={c}")));
        again.push(format!("--field chains={delta} --body-file <f>"));
        return Err(Refusal::new(
            Code::ChainReverts,
            format!("{delta}#{heading}"),
            "a delta on a pending chain keeps every section the chain added or changed, as the stamp writes the tip's \
             body in whole (34, design change 4)",
            format!(
                "the chain up to {delta} {what}: `## {heading}`; read the tip: bp show {delta} --json; write the body \
                 on top of it; rerun {}",
                again.join(" ")
            ),
        ));
    }
    Ok(())
}

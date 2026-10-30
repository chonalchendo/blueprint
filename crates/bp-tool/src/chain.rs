//! A chain of deltas (34, design change 4; 13, Delta): a second `record delta` on a design under the same milestone,
//! with another `cause`, builds on the pending one. Its `base` is the hash of the design as that delta leaves it, and
//! its `chains` names it. `stamp` walks the chain in order and checks each base against the body the link before
//! wrote, so a link that never saw a change refuses `base_moved` (28, design change 8).

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

/// What [`walk`] found: the hash and the specs of the design as the links before the first moved one leave it, and
/// that link with its `base_moved` refusal. With no moved link, the state a delta on the chain's tip builds on.
pub(crate) struct Walked {
    pub(crate) base: String,
    pub(crate) specs: Specs,
    pub(crate) moved: Option<(usize, Refusal)>,
}

/// Checks the base of each link of `chain`, a chain of `design`, against the hash of the design as the links before
/// it leave it: the first against the design, each next against the body the one before writes and the specs the
/// chain puts so far (28, design change 8; 34, design change 4).
pub(crate) fn walk(vault: &Vault, design: &Id, chain: &[Proposed], milestone: Option<&str>) -> Result<Walked, Refusal> {
    let text = vault.text(design)?;
    let mut specs: Specs = specs(vault, design).0.into_iter().collect();
    let mut base = hash(&text[bp_vault::header_end(&text).expect("a design has a header")..], &specs);
    for (i, link) in chain.iter().enumerate() {
        if let Err(r) = moved(vault, design, &link.delta, &link.header, milestone, &base) {
            return if r.code == Code::BaseMoved { Ok(Walked { base, specs, moved: Some((i, r)) }) } else { Err(r) };
        }
        for (name, spec) in &link.diagrams {
            match spec {
                Some(s) => specs.insert(name.clone(), s.text.clone()),
                None => specs.remove(name),
            };
        }
        let written = crate::set::rebody(&text, &link.body).unwrap_or_else(|| text.clone());
        base = hash(&written[bp_vault::header_end(&written).expect("a design has a header")..], &specs);
    }
    Ok(Walked { base, specs, moved: None })
}

/// The `cause` of a pending delta; none for design work.
fn cause_of(p: &Proposed) -> Option<&str> {
    p.header.get("cause").and_then(Value::as_str)
}

/// Where a new delta goes: the hash and specs it builds on, the delta it chains on, and the stale delta it rejects.
pub(crate) struct Onto {
    pub(crate) base: String,
    pub(crate) specs: Specs,
    pub(crate) chains: Option<Id>,
    pub(crate) stale: Option<Id>,
}

/// Where a new delta of `design` under `milestone` goes (13, Delta; 34, design change 4). With no delta pending, on
/// the design. With a chain pending and no link of the new delta's `cause`, on the chain's tip. A first moved link of
/// that cause can never be accepted: the new delta replaces it on the links before it and closes it as rejected. Any
/// other pending case refuses `delta_pending`; an editorial delta never chains. `given` is a `chains` the caller
/// named: it must be the tip, so a chain never forks.
pub(crate) fn onto(
    vault: &Vault,
    index: &Index,
    design: &Id,
    milestone: Option<&str>,
    cause: Option<&str>,
    given: Option<&str>,
) -> Result<Onto, Refusal> {
    let chain = chain(vault, index, design, milestone)?;
    let Walked { base, specs, moved } = walk(vault, design, &chain, milestone)?;
    let fix = match moved {
        None if (milestone.is_some() || chain.is_empty()) && !chain.iter().any(|p| cause_of(p) == cause) => {
            let chains = chain.last().map(|p| p.delta.clone());
            return forks(design, milestone, given, Onto { base, specs, chains, stale: None });
        }
        None if milestone.is_none() => {
            let delta = &chain[0].delta;
            format!("the human accepts the pending editorial delta first: bp stamp {delta}; then propose this one")
        }
        None => format!(
            "the pending delta carries the change until the milestone is stamped: bp list delta --under {design}"
        ),
        Some((i, _)) if cause_of(&chain[i]) == cause => {
            let chains = chain[..i].last().map(|p| p.delta.clone());
            let stale = Some(chain[i].delta.clone());
            return forks(design, milestone, given, Onto { base, specs, chains, stale });
        }
        Some((i, r)) => format!(
            "{} can never be stamped, and its cause is {}, not this one; its owner proposes it again: {}",
            chain[i].delta,
            cause_of(&chain[i]).unwrap_or("design work"),
            r.fix
        ),
    };
    Err(Refusal::new(
        Code::DeltaPending,
        format!("{design} {}", milestone.unwrap_or("editorial")),
        "a design carries one chain of proposed deltas per milestone, one delta per cause, and one editorial delta \
         (13, Delta)",
        fix,
    ))
}

/// `onto`, unless the caller named a `chains` other than the delta it chains on: two deltas on one link would fork
/// the chain (34, design change 4).
fn forks(design: &Id, milestone: Option<&str>, given: Option<&str>, onto: Onto) -> Result<Onto, Refusal> {
    let tip = onto.chains.as_ref().map(Id::as_str);
    if given.is_none() || given == tip {
        return Ok(onto);
    }
    let fix = match tip {
        Some(tip) => format!(
            "write your body on top of the latest delta's body (bp show {tip} --json), then rerun bp record delta \
             {design} without --field chains; the tool chains it on {tip}"
        ),
        None => format!("no delta of {design} pends for this one to chain on; rerun without --field chains"),
    };
    Err(Refusal::new(
        Code::DeltaPending,
        format!("{design} {}", milestone.unwrap_or("editorial")),
        "a delta chains on the latest pending delta of its design and milestone, so a chain never forks (34, design \
         change 4)",
        fix,
    ))
}

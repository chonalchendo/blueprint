//! The check set a pull request carries (27, design change 8): the `error` blocks of the issue, its milestone and each
//! proposed delta it caused, now and at the claim. The head names the set by a `Bp-Checks` trailer. `pr` pushes a
//! commit that names a changed set, and `land` and the Change page test that the head names the set now (21, rule 1).

use std::collections::BTreeMap;
use std::path::Path;

use bp_vault::{Body, Code, Id, Index, Kind, Record, Refusal, Vault};

use crate::git::git;

/// Each named block's hash, and whether it is an error block, by name.
pub type Blocks = BTreeMap<String, (String, bool)>;

/// One owner of blocks: the issue, its milestone, or a carried delta.
pub struct Owner {
    /// The node or record that holds the blocks now.
    pub id: Id,
    /// Where its claim-time blocks come from, under the id it had then: a delta's is its design's.
    pub source: Id,
    /// Its blocks at the claim's commit.
    pub then: Blocks,
    /// Its blocks now.
    pub now: Blocks,
}

/// The check set of an issue under a claim, now and at the claim.
pub struct CheckSet {
    /// The commit that added the issue's earliest claim: "since the claim".
    pub at: String,
    /// The proposed deltas the issue caused, each with the design it changes.
    pub deltas: Vec<(Id, Record)>,
    /// The issue, its milestone, then each delta.
    pub owners: Vec<Owner>,
    /// The set hash now.
    pub set: String,
    /// The set hash at the claim.
    pub claimed: String,
}

/// Reads the check set of `issue`. "Since the claim" is the commit that added the issue's earliest claim, under any id
/// the issue had, so neither `release` and `cut` again nor a `rename` moves the baseline past a change. Each owner is
/// read at that commit under the id it had there. A delta's blocks are compared with its design's then, as its body
/// is the proposed design body.
///
/// # Panics
///
/// When the issue has no claim: `pr`, `land` and the Change page read it under one.
pub fn check_set(vault: &Vault, index: &Index, issue: &Id) -> Result<CheckSet, Refusal> {
    let claims = index.records(issue.as_str(), Kind::Claim)?;
    let first = &claims.first().expect("the check set is read under a claim").id;
    // A claim's path is its issue's id and a suffix a rename keeps. `--follow` is no help: it takes a sibling claim
    // for a copy and follows it.
    let suffix = &first[issue.as_str().len()..];
    let ids = std::iter::once(issue.clone()).chain(index.aliases_of(issue)?);
    let paths: Vec<String> = ids.map(|id| format!("./{id}{suffix}.md")).collect();
    let args = ["log", "--diff-filter=A", "--format=%H", "--"].into_iter().chain(paths.iter().map(String::as_str));
    let at = git(&vault.root, &args.collect::<Vec<_>>())?.lines().last().unwrap_or_default().to_string();
    let file = vault.read(issue)?;
    let deltas = crate::carried_deltas(index, issue)?;
    // Each owner: its id, the file its claim-time text comes from, and its blocks now.
    let mut now = vec![(issue.clone(), issue.clone(), hashes(&file.body))];
    if let Some(m) = file.header.and_then(|h| h.under) {
        let blocks = hashes(&vault.read(&m)?.body);
        now.push((m.clone(), m, blocks));
    }
    for (design, d) in &deltas {
        let delta = Id::parse(&d.id).expect("the index holds ids");
        now.push((delta, design.clone(), hashes(&Body::parse(&d.body))));
    }
    let (mut owners, mut set, mut claimed) = (Vec::new(), Vec::new(), Vec::new());
    for (id, source, now) in now {
        let (source, then) = then(vault, index, source, &at)?;
        let errors = |b: &Blocks| -> Vec<String> { b.values().filter(|(_, e)| *e).map(|(h, _)| h.clone()).collect() };
        set.extend(errors(&now));
        claimed.extend(errors(&then));
        owners.push(Owner { id, source, then, now });
    }
    Ok(CheckSet {
        at,
        deltas,
        owners,
        set: bp_vault::set_hash(set.iter().map(String::as_str)),
        claimed: bp_vault::set_hash(claimed.iter().map(String::as_str)),
    })
}

impl CheckSet {
    /// Whether `commit` in `repo` names the check set now: its `Bp-Checks` trailer, else the set at the claim, which
    /// holds until a `pr` pushes a trailer.
    pub fn named_by(&self, repo: &Path, commit: &str) -> Result<bool, Refusal> {
        let named = git(repo, &["log", "-1", "--format=%(trailers:key=Bp-Checks,valueonly)", commit])?;
        Ok(named.lines().last().map_or(self.claimed.as_str(), str::trim) == self.set)
    }
}

/// The blocks of `id` at commit `at`, under the id it had there: today's, or a past one its aliases keep. A file the
/// commit lacks had no blocks then. One today's schema cannot parse counts as none too: its blocks read as added,
/// which the reviewer sees, where a refusal would stop every later `pr`.
fn then(vault: &Vault, index: &Index, id: Id, at: &str) -> Result<(Id, Blocks), Refusal> {
    for past in std::iter::once(id.clone()).chain(index.aliases_of(&id)?) {
        match vault.read_at(&past, at) {
            Ok(f) => return Ok((past, hashes(&f.body))),
            Err(r) if r.code == Code::SchemaInvalid => return Ok((past, BTreeMap::new())),
            Err(r) if r.code == Code::NoNode => {}
            Err(r) => return Err(r),
        }
    }
    Ok((id, BTreeMap::new()))
}

/// The named blocks of `body`.
fn hashes(body: &Body) -> Blocks {
    body.named_checks().into_iter().map(|c| (c.attrs["name"].clone(), (c.hash(), c.is_error()))).collect()
}

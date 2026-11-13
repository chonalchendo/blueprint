//! The dependency stack an issue is cut on (15, a dependency chain to landed): the claim's base, the targets, and the
//! test that `cut` and ready share.

/// The branch a held claim's pull request lands on (15, a dependency chain to landed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Base {
    /// Not recorded: a converted claim names no base branch. `land` tests origin's default branch; a read without the
    /// clone cannot, so needs landing holds and the Change page withholds the command.
    #[default]
    Unknown,
    /// This branch.
    Branch(String),
    /// The claim was cut from this dependency's pull request head, and the dependency is not resolved yet.
    Unmerged(String),
}

/// A `depends` target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependency {
    /// The target id.
    pub id: String,
    /// The target's status; `None` when the target is not in the vault.
    pub status: Option<String>,
    /// The target holds an open pull request.
    pub open_pr: bool,
    /// The stack a dependant would sit on: the target, then each held claim's unresolved `base_issue` below it, at
    /// most [`STACK_DEPTH`]. Read only for an unresolved target with an open pull request.
    pub layers: Vec<String>,
}

/// A stack is at most three layers deep; the cap is a choice, not a measurement (15, a dependency chain to landed).
pub const STACK_DEPTH: usize = 3;

/// Why an issue cannot be cut on its unresolved dependencies (15, a dependency chain to landed).
#[derive(Debug, PartialEq, Eq)]
pub enum Unstacked<'a> {
    /// These hold no open pull request to base on (`dependency_unpushed`).
    Unpushed(Vec<&'a str>),
    /// No one stack holds them all (`dependency_split`).
    Split(Vec<&'a str>),
    /// The new layer would sit on these, the top first, past the cap (`stack_too_deep`).
    Deep(&'a [String]),
}

/// The dependency an issue is cut on: the unresolved one whose stack holds every other; `None` when each is resolved.
/// `cut` refuses on the error and ready waits on it, so the two never disagree (21, rule 1).
pub fn stack(depends: &[Dependency]) -> Result<Option<&Dependency>, Unstacked<'_>> {
    let below: Vec<&Dependency> = depends.iter().filter(|d| d.status.as_deref() != Some("resolved")).collect();
    let unpushed: Vec<&str> = below.iter().filter(|d| !d.open_pr).map(|d| d.id.as_str()).collect();
    if !unpushed.is_empty() {
        return Err(Unstacked::Unpushed(unpushed));
    }
    if below.is_empty() {
        return Ok(None);
    }
    let Some(&top) = below.iter().find(|d| below.iter().all(|o| d.layers.contains(&o.id))) else {
        return Err(Unstacked::Split(below.iter().map(|d| d.id.as_str()).collect()));
    };
    if top.layers.len() >= STACK_DEPTH {
        return Err(Unstacked::Deep(&top.layers));
    }
    Ok(Some(top))
}

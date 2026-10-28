//! The read composition (32, design change 1): the clock and the forge snapshots a read sees, the `Intents`
//! composition over the index, the record reductions the pure intents read, and the typed objects of `list`, `show`
//! and `search`, and the page objects the view serves beside them. `bp-tool` prints them; `bp-view` serves them. It writes nothing but the index refresh.

mod asof;
mod change;
pub mod checkset;
pub mod clock;
mod design;
mod diagram;
mod diff;
pub mod git;
pub mod inputs;
mod intents;
pub mod landing;
mod milestone;
mod moving;
mod objects;
mod pages;
pub mod snapshot;
mod stack;
mod version;
mod world;

pub use asof::{BodyDiff, SectionLines};
use bp_vault::{Code, Id, Index, Refusal};
pub use change::{
    Change, ChangeDelta, ChangePullRequest, ChangeRefusal, DIFF_CAP, Diff, DiffFile, Layer, carried_deltas, change,
};
pub use design::{Design, DesignDelta, Diagram, Heading, design};
pub use diagram::diagram;
pub use intents::Intents;
pub use milestone::{Concern, FinishBlock, LastRun, Milestone, milestone};
pub use moving::{Moving, MovingGroup, MovingRow, moving};
pub use objects::{Census, Forge, List, ListRow, Search, Show, census, list, search, show, status_in_set};
pub use pages::{Home, HomeDesign, HomeMilestone, ProjectRow, Projects, home, projects};
pub use world::{World, split};

/// No limit on a `list` read: every row.
pub const ALL: usize = usize::MAX >> 1;

/// An id argument. Anything that is not a path id under the vault names no node.
pub fn node_id(text: &str) -> Result<Id, Refusal> {
    Id::parse(text).map_err(|why| {
        Refusal::new(
            Code::NoNode,
            text,
            "an id is a path under projects/ or shared/, without .md",
            format!("{why}; bp search {}", text.rsplit('/').next().unwrap_or(text).replace(['-', '.', '_'], " ")),
        )
    })
}

/// A verb's subject: the id a node or record holds today, through the index. Case is ignored, and a renamed node's
/// past id names the node (13, rule 1). An id nothing holds stays as asked, and the verb refuses it `no_node`.
pub fn subject(index: &Index, text: &str) -> Result<Id, Refusal> {
    let id = node_id(text)?;
    Ok(index.holder(&id)?.map_or(id, |(held, _)| held))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bp_vault::FindingKind;
    use ts_rs::TS;

    use super::*;

    /// Writes `web/src/types/` from the objects and every type they hold. The `check` job's last step after
    /// `cargo test --workspace` fails on any change to the directory, a new file included, so a type changed without
    /// its export fails there (22). The directory is generated whole, so a type removed leaves no file.
    #[test]
    fn export_types() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/types");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("the types directory is writable");
        }
        // JSON numbers parse as `number`; a forge age never nears 2^53 seconds.
        let cfg = ts_rs::Config::new().with_out_dir(&dir).with_large_int("number");
        List::export_all(&cfg).expect("List exports");
        Show::export_all(&cfg).expect("Show exports");
        Search::export_all(&cfg).expect("Search exports");
        Census::export_all(&cfg).expect("Census exports");
        Home::export_all(&cfg).expect("Home exports");
        Projects::export_all(&cfg).expect("Projects exports");
        Moving::export_all(&cfg).expect("Moving exports");
        Change::export_all(&cfg).expect("Change exports");
        Milestone::export_all(&cfg).expect("Milestone exports");
        Design::export_all(&cfg).expect("Design exports");
        // The closed enum's `Serialize` writes each text; its `TS` derives the variant in snake case. They agree.
        let ts = FindingKind::inline(&cfg);
        for k in FindingKind::ALL {
            assert!(ts.contains(&format!("\"{k}\"")), "{k} is not in {ts}");
        }
    }
}

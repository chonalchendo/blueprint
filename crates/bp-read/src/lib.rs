//! The read composition (32, design change 1): the clock and the forge snapshots a read sees, the `Intents`
//! composition over the index, the record reductions the pure intents read, and the typed objects of `list`, `show`
//! and `search`. `bp-tool` prints them; `bp-view` serves them. It writes nothing but the index refresh.

pub mod clock;
pub mod inputs;
mod intents;
mod objects;
mod world;

pub use intents::Intents;
pub use objects::{Census, Forge, List, ListRow, Search, Show, census, list, search, show, status_in_set};
pub use world::{World, split};

/// No limit on a `list` read: every row.
pub const ALL: usize = usize::MAX >> 1;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bp_vault::FindingKind;
    use ts_rs::TS;

    use super::*;

    /// Writes `web/src/types/` from the objects and every type they hold. CI diffs the directory, so a type changed
    /// without its export fails there (22). The directory is generated whole, so a type removed leaves no file.
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
        // The closed enum's `Serialize` writes each text; its `TS` derives the variant in snake case. They agree.
        let ts = FindingKind::inline(&cfg);
        for k in FindingKind::ALL {
            assert!(ts.contains(&format!("\"{k}\"")), "{k} is not in {ts}");
        }
    }
}

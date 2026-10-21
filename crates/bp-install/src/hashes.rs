//! The installed files' hashes, cached by each file's stat in the clone's git directory. `check` runs before every
//! walk, and reading and hashing each installed file cost a quarter of a walk (27, issue 13); a stat costs a tenth of
//! a read. A hit needs the device, inode, size, and the modification and change times to the nanosecond to match. A
//! write sets the change time, but a coarse clock (whole seconds on HFS+, one tick on Linux) can give two writes one
//! time. So a file changed within a second of the check is not cached: a later write then carries a later time, and a
//! changed file is hashed again and still refuses `bundle_mismatch` (27, design change 2).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The cache of one clone: an entry's stat and hash, by manifest entry.
pub(crate) struct Hashes {
    path: PathBuf,
    held: BTreeMap<String, (String, String)>,
    changed: bool,
    /// When the check began, in nanoseconds since the epoch, less one second: the widest clock step a stat shows.
    /// A file changed after it is not cached, as a second write within that step would keep the stat the cache holds.
    settled: i128,
}

impl Hashes {
    /// The cache of the clone at `top`; empty when absent or unreadable, so the check hashes every file.
    pub(crate) fn load(top: &Path) -> Self {
        let path = top.join(".git/bp-bundle-hashes");
        let held = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let mut f = l.split('\t');
                Some((f.next()?.to_string(), (f.next()?.to_string(), f.next()?.to_string())))
            })
            .collect();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos().cast_signed());
        Self { path, held, changed: false, settled: now - 1_000_000_000 }
    }

    /// The hash `hash` gives `entry` from the bytes of `file`, from the cache when the file's stat is the one cached;
    /// `None` when the file is absent or `hash` gives none. The stat is read before the bytes, so a write between
    /// the two moves the stat and the next check hashes again.
    pub(crate) fn of(
        &mut self,
        entry: &str,
        file: &Path,
        hash: impl FnOnce(&[u8]) -> Option<String>,
    ) -> Option<String> {
        let m = std::fs::metadata(file).ok()?;
        let stat = format!(
            "{}:{}:{}:{}.{}:{}.{}",
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec()
        );
        if let Some((held, h)) = self.held.get(entry)
            && *held == stat
        {
            return Some(h.clone());
        }
        let h = hash(&std::fs::read(file).ok()?)?;
        let ns = |s: i64, n: i64| i128::from(s) * 1_000_000_000 + i128::from(n);
        if ns(m.ctime(), m.ctime_nsec()) < self.settled && ns(m.mtime(), m.mtime_nsec()) < self.settled {
            self.held.insert(entry.to_string(), (stat, h.clone()));
            self.changed = true;
        }
        Some(h)
    }

    /// Drops `entry`'s cached hash, so the next [`Hashes::of`] reads the file: a damaged cache line (a hand edit, a
    /// torn write) would otherwise refuse each walk until the file's stat moved.
    pub(crate) fn forget(&mut self, entry: &str) {
        self.changed |= self.held.remove(entry).is_some();
    }

    /// Writes the cache when the check hashed a file again. A failed write costs the next check its hits, no more;
    /// the rename keeps a parallel walk from reading half a file.
    pub(crate) fn save(&self) {
        if !self.changed {
            return;
        }
        let mut text = String::new();
        for (e, (s, h)) in &self.held {
            let _ = writeln!(text, "{e}\t{s}\t{h}");
        }
        let tmp = self.path.with_extension(std::process::id().to_string());
        if std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &self.path)).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

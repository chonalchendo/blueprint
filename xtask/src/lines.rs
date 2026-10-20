//! `cargo xtask lines`, the file cap of 22: no non-test source file holds more than 500 lines. A file's test module
//! starts at its first `#[cfg(test)]`; files under a `tests/` directory are tests whole.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const CAP: usize = 500;

/// One line per file over the cap, as `<path>: <lines> non-test lines (cap 500)`.
pub fn run(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for dir in ["crates", "xtask"] {
        sources(&root.join(dir), &mut files)?;
    }
    files.sort();
    let mut over = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let lines = text.lines().take_while(|l| l.trim_start() != "#[cfg(test)]").count();
        if lines > CAP {
            let shown = path.strip_prefix(root).unwrap_or(&path).display();
            over.push(format!("{shown}: {lines} non-test lines (cap {CAP})"));
        }
    }
    Ok(over)
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if path.is_dir() {
            if !matches!(name, "tests" | "target") {
                sources(&path, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

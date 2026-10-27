//! `cargo xtask lines`, the file cap of 22: no non-test source file holds more than 500 lines. A file's test code
//! starts at the first `#[cfg(test)]` that opens a `mod`; a `#[cfg(test)]` helper above it still counts. Files under a
//! `tests/` directory are tests whole, and `target/` is build output. `build.rs`, `benches/` and `examples/` count.

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
        let lines = non_test(&text);
        if lines > CAP {
            let shown = path.strip_prefix(root).expect("every path is under the root it was joined to").display();
            over.push(format!("{shown}: {lines} non-test lines (cap {CAP})"));
        }
    }
    Ok(over)
}

/// The lines before the test module: the first `#[cfg(test)]` whose next non-blank line opens a `mod`.
pub(crate) fn non_test(text: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    (0..lines.len())
        .find(|&i| {
            lines[i].trim() == "#[cfg(test)]"
                && lines[i + 1..]
                    .iter()
                    .map(|l| l.trim())
                    .find(|l| !l.is_empty())
                    .is_some_and(|l| l.starts_with("mod "))
        })
        .unwrap_or(lines.len())
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            if !path.file_name().is_some_and(|n| n == "tests" || n == "target") {
                sources(&path, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> String {
        "fn a() {}\n".repeat(n)
    }

    #[test]
    fn the_test_module_ends_the_count_and_a_test_helper_does_not() {
        let text = format!(
            "{}#[cfg(test)]\nuse std::fmt;\n{}\n#[cfg(test)]\n\nmod tests {{\n{}}}\n",
            lines(100),
            lines(400),
            lines(900)
        );
        assert_eq!(non_test(&text), 503);
        assert_eq!(non_test(&lines(7)), 7);
    }

    #[test]
    fn a_file_over_the_cap_is_named_and_tests_dirs_are_skipped() {
        let root = std::env::temp_dir().join(format!("xtask-lines-{}", std::process::id()));
        let src = root.join("crates/a/src");
        std::fs::create_dir_all(root.join("crates/a/tests")).unwrap();
        std::fs::create_dir_all(root.join("xtask")).unwrap();
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("ok.rs"), lines(500)).unwrap();
        std::fs::write(src.join("big.rs"), lines(501)).unwrap();
        std::fs::write(root.join("crates/a/tests/long.rs"), lines(900)).unwrap();
        let over = run(&root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(over, ["crates/a/src/big.rs: 501 non-test lines (cap 500)"]);
    }
}

//! Embeds `bundle/` (23): every file under it, by its path relative to `bundle/`, in path order. Cargo reruns this
//! when any file under the directory changes, so the binary never ships a stale bundle.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    // `cargo xtask eval` builds the arms that lack the skill, or hold filler for it, from a copy of the bundle (29,
    // design change 1). The manifest check forbids editing an installed file, so the arm needs its own binary.
    println!("cargo:rerun-if-env-changed=BP_BUNDLE_DIR");
    let root = std::env::var_os("BP_BUNDLE_DIR")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bundle"), PathBuf::from);
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    let mut out = String::from("&[\n");
    for path in &files {
        let rel = path.strip_prefix(&root).expect("walk stays under the root").to_string_lossy().replace('\\', "/");
        let abs = path.canonicalize().expect("a walked file exists");
        writeln!(out, "    ({rel:?}, include_bytes!({:?})),", abs.display().to_string())
            .expect("a String takes writes");
    }
    out.push(']');
    let dest = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("bundle.rs");
    std::fs::write(dest, out).expect("OUT_DIR is writable");
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("bundle/ sits at the workspace root").flatten() {
        let path = entry.path();
        // A dot file is the host's (`.DS_Store`), not the bundle's.
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk(&path, files);
        } else {
            files.push(path);
        }
    }
}

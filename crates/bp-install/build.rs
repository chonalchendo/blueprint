//! Embeds `bundle/` (23): every file under it, by its path relative to `bundle/`, in path order. Cargo reruns this
//! when any file under the directory changes, so the binary never ships a stale bundle. It also names the commit the
//! build is from, as `BP_BUILT_FROM`: `install` pins the forge check's build of `bp` to it (37, design change 5).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    println!("cargo:rustc-env=BP_BUILT_FROM={}", built_from().unwrap_or_default());
}

/// The commit of the clone this crate sits in, or `None` outside a clone (a source archive). Cargo reruns this when
/// `HEAD` or its branch moves. Only existing files are watched: cargo reruns a script that watches a missing one on
/// every build.
fn built_from() -> Option<String> {
    let git = |args: &[&str]| {
        let out = Command::new("git").current_dir(env!("CARGO_MANIFEST_DIR")).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let watched =
        ["HEAD".to_string(), "packed-refs".to_string()].into_iter().chain(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in watched {
        let path = git(&["rev-parse", "--path-format=absolute", "--git-path", &name])?;
        if Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    git(&["rev-parse", "--verify", "-q", "HEAD^{commit}"])
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

//! Helpers of the eval integration tests: a repository to clone, a vault to copy, and git without a global identity.

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The `bp` that the xtask tests drive, or `None` (the test then passes without running) when it is not built.
pub fn bp() -> Option<PathBuf> {
    let bp = Path::new(env!("CARGO_BIN_EXE_xtask")).with_file_name("bp");
    if !bp.is_file() {
        eprintln!("skipped: {} is not built (cargo build -p bp-tool)", bp.display());
        return None;
    }
    Some(bp)
}

/// A vault with a project and its system design: the least that a sandbox needs.
pub fn vault(dir: &Path) {
    let project = "---\nkind: project\ntitle: Bp\nstatus: active\nrepository: https://github.invalid/o/r\n---\n";
    write(&dir.join("projects/bp/project.md"), project);
    write(&dir.join("projects/bp/designs/system.md"), "---\nkind: design\ntitle: S\nstatus: active\n---\n\nBody.\n");
    write(&dir.join("shared/.gitkeep"), "");
}

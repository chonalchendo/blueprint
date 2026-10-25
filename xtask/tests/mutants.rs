//! `cargo xtask mutants` without `cargo-mutants` (30, Decided 3): it refuses with the install line before it reads
//! anything else. An empty `PATH` and no `$CARGO` stand for a host without the tool.

use std::path::Path;
use std::process::Command;

#[test]
fn mutants_refuses_with_the_install_line_when_cargo_mutants_is_absent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["mutants", "--pr", "62", "--count", "4", "--seed", "7"])
        .current_dir(root)
        .env("PATH", "")
        .env_remove("CARGO")
        .env_remove("BP_REDESIGN")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("is not installed: cargo install --locked cargo-mutants@27.1.0"), "{stderr}");
}

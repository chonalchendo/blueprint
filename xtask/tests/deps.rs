//! `cargo xtask deps` through the binary, on a copy of the workspace manifests with one edge broken: exit 2 and
//! one stderr line per violation.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// Copies the manifests and the lock file into a temporary directory, so a test can break one edge.
fn copy_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = workspace_root();
    for rel in ["Cargo.toml", "Cargo.lock", "crate-rule.txt", "rust-toolchain.toml"] {
        std::fs::copy(root.join(rel), dir.path().join(rel)).unwrap();
    }
    for member in ["crates", "xtask"] {
        copy_manifests(&root.join(member), &dir.path().join(member));
    }
    dir
}

fn copy_manifests(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        let dest = to.join(entry.file_name());
        if path.is_dir() && entry.file_name() != "target" {
            copy_manifests(&path, &dest);
        } else if path.file_name().is_some_and(|f| f == "Cargo.toml") {
            std::fs::copy(&path, &dest).unwrap();
        } else if path.extension().is_some_and(|e| e == "rs") {
            // cargo metadata needs the targets to exist; their content does not matter.
            std::fs::write(&dest, "").unwrap();
        }
    }
}

fn edit(dir: &Path, rel: &str, from: &str, to: &str) {
    let p = dir.join(rel);
    let s = std::fs::read_to_string(&p).unwrap();
    assert!(s.contains(from), "{rel} holds no {from:?}");
    std::fs::write(&p, s.replacen(from, to, 1)).unwrap();
}

/// Runs the binary in `dir`: the exit code and the stderr lines.
fn deps(dir: &Path) -> (Option<i32>, Vec<String>) {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("deps")
        .env("CARGO_NET_OFFLINE", "true")
        .current_dir(dir)
        .output()
        .unwrap();
    (out.status.code(), String::from_utf8_lossy(&out.stderr).lines().map(str::to_string).collect())
}

#[test]
fn the_workspace_holds_the_rule() {
    assert_eq!(deps(&workspace_root()), (Some(0), vec![]));
}

#[test]
fn an_edge_outside_the_rule() {
    let dir = copy_workspace();
    edit(dir.path(), "crates/bp-view/Cargo.toml", "[dependencies]\n", "[dependencies]\nbp-check.workspace = true\n");
    assert_eq!(deps(dir.path()), (Some(2), vec!["edge outside the rule: bp-view -> bp-check".into()]));
}

#[test]
fn the_write_feature_on_a_dependency_line() {
    let dir = copy_workspace();
    let line = "bp-forge = { workspace = true, features = [\"write\"] }";
    edit(dir.path(), "crates/bp-view/Cargo.toml", "bp-forge.workspace = true", line);
    assert_eq!(deps(dir.path()), (Some(2), vec!["feature outside the rule: bp-view -> bp-forge[write]".into()]));
}

#[test]
fn the_write_feature_through_a_features_table() {
    let dir = copy_workspace();
    let p = dir.path().join("crates/bp-view/Cargo.toml");
    let s = std::fs::read_to_string(&p).unwrap() + "\n[features]\nlive = [\"bp-forge/write\"]\n";
    std::fs::write(&p, s).unwrap();
    assert_eq!(deps(dir.path()), (Some(2), vec!["feature outside the rule: bp-view -> bp-forge[write]".into()]));
}

#[test]
fn a_reserved_dependency_directly_and_through_another_crate() {
    let dir = copy_workspace();
    edit(dir.path(), "crates/bp-vault/Cargo.toml", "[dependencies]\n", "[dependencies]\nmemchr = \"2\"\n");
    edit(dir.path(), "crate-rule.txt", "only bp-view: tokio", "only bp-view: memchr\nonly bp-view: libsqlite3-sys");
    let (code, lines) = deps(dir.path());
    assert_eq!(code, Some(2));
    assert!(lines.contains(&"external dependency reserved for bp-view: bp-vault -> memchr".into()), "{lines:?}");
    let through = "external dependency reserved for bp-view: bp-vault -> libsqlite3-sys through rusqlite";
    assert!(lines.contains(&through.into()), "{lines:?}");
}

#[test]
fn a_rule_row_with_no_package() {
    let dir = copy_workspace();
    edit(dir.path(), "crate-rule.txt", "xtask        ->", "xtask        ->\nbp-gone      -> bp-vault");
    assert_eq!(deps(dir.path()), (Some(2), vec!["rule row with no package: bp-gone".into()]));
}

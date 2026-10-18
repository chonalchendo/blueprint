//! `bp where` through the binary: the envelope, the project, the refusal and its exit code.

use assert_cmd::Command;

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects/bp/issues")).unwrap();
    dir
}

#[test]
fn where_inside_a_project() {
    let dir = vault();
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(dir.path().join("projects/bp/issues"))
        .env_remove("BP_VAULT")
        .args(["where", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["project"], "bp");
    assert_eq!(v["contract"], 1);
    // The process reports its canonical working directory; macOS puts temp dirs behind a symlink.
    let reported = std::fs::canonicalize(v["vault"].as_str().unwrap()).unwrap();
    assert_eq!(reported, std::fs::canonicalize(dir.path()).unwrap());
}

#[test]
fn where_refuses_outside_a_vault_with_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(dir.path())
        .env_remove("BP_VAULT")
        .args(["where", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["code"], "no_vault");
    assert!(v["fix"].as_str().unwrap().contains("BP_VAULT"));
}

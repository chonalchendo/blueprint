//! `bp where` through the binary: the envelope, the project, the refusal and its exit code.

use assert_cmd::Command;

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects/bp/issues")).unwrap();
    std::fs::create_dir_all(dir.path().join("shared")).unwrap();
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
    assert_eq!(v["contract_version"], 1);
    // The process reports its canonical working directory; macOS puts temp dirs behind a symlink.
    let reported = std::fs::canonicalize(v["vault"].as_str().unwrap()).unwrap();
    assert_eq!(reported, std::fs::canonicalize(dir.path()).unwrap());
}

#[test]
fn where_refuses_outside_a_vault_with_exit_2() {
    // A `projects/` of code checkouts alone, as in a home directory, is not a vault.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects/code")).unwrap();
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

#[test]
fn where_from_a_code_repository_finds_the_project_by_its_remote() {
    let vault = vault();
    std::fs::write(
        vault.path().join("projects/bp/project.md"),
        "---\nkind: project\ntitle: Blueprint\nstatus: active\nrepository: https://github.com/o/blueprint\n---\n",
    )
    .unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".git")).unwrap();
    std::fs::write(repo.path().join(".git/config"), "[remote \"origin\"]\n\turl = git@github.com:o/blueprint.git\n")
        .unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(repo.path().join("src"))
        .env("BP_VAULT", vault.path())
        .args(["where", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["project"], "bp");
    let worktree = std::fs::canonicalize(v["worktree"].as_str().unwrap()).unwrap();
    assert_eq!(worktree, std::fs::canonicalize(repo.path()).unwrap());
}

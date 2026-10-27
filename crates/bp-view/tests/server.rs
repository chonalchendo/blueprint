//! The server end to end: started on port 0, read through its printed url, stopped by SIGTERM.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};

const REPOSITORY: &str = "https://github.invalid/o/r";

/// A vault of one project with a design and a ready issue, in one commit.
fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        (
            "projects/bp/project.md",
            format!("---\nkind: project\ntitle: Blueprint\nstatus: active\nrepository: {REPOSITORY}\n---\n"),
        ),
        (
            "projects/bp/designs/system.md",
            "---\nkind: design\ntitle: System\nstatus: active\n---\n\n## TL;DR\n\nThe vault.\n".into(),
        ),
        (
            "projects/bp/issues/0001.a.md",
            "---\nkind: issue\ntitle: A\nstatus: open\nunder: projects/bp/designs/system\n---\n\n## Problem\n\nNone.\n"
                .into(),
        ),
        ("shared/assets/glossary.md", "# Glossary\n".into()),
        (".bp/.gitignore", "*\n".into()),
    ];
    for (rel, text) in &files {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["-c", "user.name=t", "-c", "user.email=t@t", "add", "-A"]);
    git(dir.path(), &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "one"]);
    dir
}

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success(), "git {args:?}");
}

/// A running `bp-view`, stopped by SIGTERM when dropped.
struct Server {
    child: Child,
    url: String,
}

impl Server {
    fn start(vault: &Path, args: &[&str], path: Option<&Path>) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_bp-view"));
        cmd.arg("--vault").arg(vault).args(args).stdout(Stdio::piped());
        // The view takes `--vault` over `BP_VAULT`, so a stray one in the runner's environment changes nothing.
        cmd.env("BP_VAULT", "/nonexistent").env_remove("BP_NOW").env_remove("BP_FORGE_SNAPSHOT");
        if let Some(dir) = path {
            cmd.env("PATH", format!("{}:{}", dir.display(), std::env::var("PATH").unwrap()));
        }
        let mut child = cmd.spawn().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut line).unwrap();
        let printed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(printed.as_object().unwrap().len(), 1, "the line holds the url alone: {line}");
        let url = printed["url"].as_str().unwrap().to_string();
        assert!(url.starts_with("http://127.0.0.1:") && url.ends_with('/'), "{url}");
        Self { child, url }
    }

    /// GET `path`: the status and the body.
    fn get(&self, path: &str) -> (u16, String) {
        let host = self.url.trim_start_matches("http://").trim_end_matches('/');
        let mut stream = TcpStream::connect(host).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        (head.split(' ').nth(1).unwrap().parse().unwrap(), body.to_string())
    }

    fn json(&self, path: &str) -> (u16, serde_json::Value) {
        let (status, body) = self.get(path);
        (status, serde_json::from_str(&body).unwrap_or_else(|e| panic!("{path}: {e}: {body}")))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = Command::new("kill").arg(self.child.id().to_string()).status();
        let _ = self.child.wait();
    }
}

#[test]
fn serves_the_page_objects_and_logs_the_page_routes() {
    let dir = vault();
    let server = Server::start(dir.path(), &["--forge", "off"], None);
    assert_eq!(server.json("/api/health"), (200, serde_json::json!({ "ok": true })));
    let (status, projects) = server.json("/api/projects");
    assert_eq!((status, projects["rows"][0]["id"].as_str()), (200, Some("projects/bp/project")));
    let (status, home) = server.json("/api/bp");
    assert_eq!(status, 200, "{home}");
    assert_eq!(home["nodes"], serde_json::json!({ "design": 1, "issue": 1, "project": 1 }));
    assert_eq!(home["intents"], serde_json::json!({ "ready": 1 }));
    assert_eq!(home["designs"][0]["id"], "projects/bp/designs/system");
    assert_eq!(home["forge"], "not read", "no snapshot exists");
    let (status, node) = server.json("/api/node/projects/bp/issues/0001.a");
    assert_eq!((status, node["title"].as_str()), (200, Some("A")));
    let (status, found) = server.json("/api/search?q=vault");
    assert_eq!((status, found["rows"][0]["id"].as_str()), (200, Some("projects/bp/designs/system")));
    for missing in ["/api/node/projects/bp/issues/nope", "/api/nope", "/api/bp/moving"] {
        let (status, refusal) = server.json(missing);
        assert_eq!((status, refusal["code"].as_str()), (404, Some("no_node")), "{missing}");
    }
    let (status, page) = server.get("/bp/moving");
    assert!(status == 200 && page.contains("id=\"root\""), "a page route serves the client: {page}");

    let log = std::fs::read_to_string(dir.path().join(".bp/use.log")).unwrap();
    let routes: Vec<&str> = log.lines().map(|l| l.split_once(' ').unwrap().1).collect();
    assert_eq!(routes, ["/ -", "/bp -", "/node/projects/bp/issues/0001.a projects/bp/issues/0001.a"], "{log}");
    assert!(log.lines().all(|l| l.as_bytes()[19] == b'Z'), "{log}");
}

#[test]
fn forge_off_never_calls_gh_and_a_failed_read_turns_it_off() {
    let dir = vault();
    let clone = tempfile::tempdir().unwrap();
    git(clone.path(), &["init", "-q"]);
    git(clone.path(), &["remote", "add", "origin", REPOSITORY]);
    let bin = tempfile::tempdir().unwrap();
    let calls = bin.path().join("calls");
    let gh = bin.path().join("gh");
    std::fs::write(&gh, format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 1\n", calls.display())).unwrap();
    std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let clone_arg = clone.path().to_str().unwrap();

    let off = Server::start(dir.path(), &["--forge", "off", "--clone", clone_arg], Some(bin.path()));
    assert_eq!(off.json("/api/bp").0, 200);
    drop(off);
    assert!(!calls.exists(), "--forge off called gh");

    let on = Server::start(dir.path(), &["--clone", clone_arg], Some(bin.path()));
    let (status, home) = on.json("/api/bp");
    assert_eq!((status, home["forge"].as_str()), (200, Some("not read")), "a failed read leaves the snapshot: {home}");
    assert_eq!(on.json("/api/bp").0, 200);
    let made = std::fs::read_to_string(&calls).unwrap();
    assert_eq!(made.lines().count(), 1, "one failed read turns the forge off: {made}");
}

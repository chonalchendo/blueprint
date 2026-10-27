//! The server end to end: started on port 0, read through its printed url, stopped by SIGTERM.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};

const REPOSITORY: &str = "https://github.invalid/o/r";

/// A vault of one project with a design and its diagram, a planned milestone and a ready issue, in one commit.
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
        ("projects/bp/designs/system/diagrams/context.html", "<svg></svg>\n".into()),
        ("projects/bp/milestones/m.md", "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n".into()),
        // A second project of the same repository, and one of a repository the test clone does not hold.
        (
            "projects/two/project.md",
            format!("---\nkind: project\ntitle: Two\nstatus: active\nrepository: {REPOSITORY}\n---\n"),
        ),
        (
            "projects/other/project.md",
            "---\nkind: project\ntitle: Other\nstatus: active\nrepository: https://github.invalid/o/other\n---\n"
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
        self.request("GET", path, &self.host())
    }

    fn host(&self) -> String {
        self.url.trim_start_matches("http://").trim_end_matches('/').to_string()
    }

    /// `method` `path` with `Host: host`: the status and the body.
    fn request(&self, method: &str, path: &str, host: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(self.host()).unwrap();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
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
    let intents = serde_json::json!({ "ready": 1, "needs-milestone-approval": 1 });
    assert_eq!(projects["rows"][0]["intents"], intents, "the milestone's intent counts beside the issue's");
    assert_eq!(projects["rows"][0].get("version"), None, "no version record yet");
    let (status, home) = server.json("/api/bp");
    assert_eq!(status, 200, "{home}");
    assert_eq!(home["nodes"], serde_json::json!({ "design": 1, "issue": 1, "milestone": 1, "project": 1 }));
    assert_eq!(home["intents"], intents);
    assert_eq!(home["designs"][0]["id"], "projects/bp/designs/system");
    assert_eq!((home["designs"][0]["diagrams"].as_u64(), home["designs"][0].get("accepted")), (Some(1), None));
    assert_eq!(home["milestones"][0]["id"], "projects/bp/milestones/m");
    assert_eq!(home["milestones"][0]["issues"], serde_json::json!({}), "the issue sits under the design");
    assert_eq!(home["forge"], "not read", "no snapshot exists");
    let (status, node) = server.json("/api/node/projects/bp/issues/0001.a");
    assert_eq!((status, node["title"].as_str()), (200, Some("A")));
    let (status, found) = server.json("/api/search?q=vault");
    assert_eq!((status, found["rows"][0]["id"].as_str()), (200, Some("projects/bp/designs/system")));
    let (status, moving) = server.json("/api/bp/moving");
    assert_eq!((status, moving["project"].as_str(), moving["forge"].as_str()), (200, Some("bp"), Some("not read")));
    let groups: Vec<&str> = moving["groups"].as_array().unwrap().iter().map(|g| g["group"].as_str().unwrap()).collect();
    assert_eq!(groups, ["needs-landing", "needs-acceptance", "blocked", "ready", "working"]);
    let ready = &moving["groups"][3]["rows"][0];
    assert_eq!(ready["command"], "bp cut projects/bp/issues/0001.a");
    assert_eq!((ready["head"].as_str(), ready.get("wait")), (Some("forge not read"), None));
    assert_eq!(moving["groups"][1]["rows"][0]["command"], "bp transition projects/bp/milestones/m approved");
    for missing in ["/api/node/projects/bp/issues/nope", "/api/nope", "/api/nope/moving", "/api/bp/moving/x"] {
        let (status, refusal) = server.json(missing);
        assert_eq!((status, refusal["code"].as_str()), (404, Some("no_node")), "{missing}");
    }
    for missing in ["/api", "/api/"] {
        let (status, refusal) = server.json(missing);
        assert_eq!((status, refusal["code"].as_str()), (404, Some("no_node")), "{missing}");
    }
    let (status, page) = server.get("/bp/moving");
    assert!(status == 200 && page.contains("id=\"root\""), "a page route serves the client: {page}");
    assert_eq!(server.get("/assets/missing.js").0, 404, "an asset the build lacks is no page");

    // The log holds the id served, whatever form of it the URL held; a HEAD is no page view.
    for alias in
        ["%5B%5Bprojects/bp/issues/0001.a%5D%5D", "projects/bp/issues/0001.a.md", "projects/bp/issues/0001.A%7Cx"]
    {
        assert_eq!(server.get(&format!("/api/node/{alias}")).0, 200, "{alias}");
    }
    assert_eq!(server.request("HEAD", "/api/bp", &server.host()).0, 200);
    // A space or a control character, once decoded, would split or forge a log line.
    for bad in [
        "projects/bp/issues/0001.a%7Cx%20y",
        "projects/bp/issues/0001.a%7C%0A2026-01-01T00:00:00Z%20/f%20-",
        "%20projects/bp/project",
        "projects/bp/project%00",
    ] {
        let (status, refusal) = server.json(&format!("/api/node/{bad}"));
        assert_eq!((status, refusal["code"].as_str()), (400, Some("usage")), "{bad}");
    }

    let log = std::fs::read_to_string(dir.path().join(".bp/use.log")).unwrap();
    let routes: Vec<&str> = log.lines().map(|l| l.split_once(' ').unwrap().1).collect();
    let node = "/node/projects/bp/issues/0001.a projects/bp/issues/0001.a";
    assert_eq!(routes, ["/ -", "/bp -", node, "/bp/moving -", node, node, node], "{log}");
    assert!(log.lines().all(|l| l.as_bytes()[19] == b'Z'), "{log}");
}

#[test]
fn only_the_views_own_host_is_served() {
    let dir = vault();
    let server = Server::start(dir.path(), &["--forge", "off"], None);
    let port = server.host().rsplit_once(':').unwrap().1.to_string();
    assert_eq!(server.request("GET", "/api/health", &format!("localhost:{port}")).0, 200);
    for host in ["attacker.example", "attacker.example:80", &format!("attacker.example:{port}"), "127.0.0.1:1"] {
        for path in ["/api/bp", "/", "/bp/moving"] {
            let (status, body) = server.request("GET", path, host);
            let refusal: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(status, 403, "{host} {path}");
            assert_eq!(
                (refusal["code"].as_str(), refusal["fix"].as_str()),
                (Some("usage"), Some("open the view at its printed url"))
            );
        }
    }
    assert!(!dir.path().join(".bp/use.log").exists(), "a refused request logs nothing");
}

#[test]
fn forge_off_never_calls_gh_and_a_gh_failure_turns_off_its_project_alone() {
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
    let made = || std::fs::read_to_string(&calls).map_or(0, |t| t.lines().count());

    let off = Server::start(dir.path(), &["--forge", "off", "--clone", clone_arg], Some(bin.path()));
    assert_eq!(off.json("/api/bp").0, 200);
    drop(off);
    assert_eq!(made(), 0, "--forge off called gh");

    let on = Server::start(dir.path(), &["--clone", clone_arg], Some(bin.path()));
    assert_eq!(on.json("/api/nope").0, 404);
    assert_eq!(made(), 0, "a project the vault does not hold reads no forge");
    assert_eq!(on.json("/api/other").0, 200);
    assert_eq!(made(), 0, "a project the clone does not hold reads no forge");
    let (status, home) = on.json("/api/bp");
    assert_eq!((status, home["forge"].as_str()), (200, Some("not read")), "a failed read leaves the snapshot: {home}");
    assert_eq!(made(), 1, "neither the 404 nor the other project turned bp's reads off");
    assert_eq!(on.json("/api/two").0, 200);
    assert_eq!(made(), 2, "bp's failure leaves two's reads on");
    assert_eq!(on.json("/api/bp").0, 200);
    assert_eq!(made(), 2, "a failed read is not retried");
}

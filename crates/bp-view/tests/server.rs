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
        // A page under `diagrams/` is vault content: this one tries to run in the view's origin.
        (
            "projects/bp/designs/system/diagrams/hostile.html",
            "<svg onload=\"fetch('/api/projects')\"><script>document.title='ran'</script><g onclick='x()'/></svg>\n"
                .into(),
        ),
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
        let (status, _, body) = self.exchange("GET", path, &self.host());
        (status, body)
    }

    fn host(&self) -> String {
        self.url.trim_start_matches("http://").trim_end_matches('/').to_string()
    }

    /// `method` `path` with `Host: host`: the status and the body.
    fn request(&self, method: &str, path: &str, host: &str) -> (u16, String) {
        let (status, _, body) = self.exchange(method, path, host);
        (status, body)
    }

    /// `method` `path` with `Host: host`: the status, the head with its header names in lower case, and the body.
    fn exchange(&self, method: &str, path: &str, host: &str) -> (u16, String, String) {
        let mut stream = TcpStream::connect(self.host()).unwrap();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        let lower = head
            .lines()
            .map(|l| l.split_once(':').map_or(l.to_string(), |(k, v)| format!("{}:{v}", k.to_ascii_lowercase())))
            .collect::<Vec<_>>()
            .join("\n");
        (head.split(' ').nth(1).unwrap().parse().unwrap(), lower, body.to_string())
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
    assert!(home["designs"][0]["diagrams"] == 2 && home["designs"][0]["version"] == "no version", "{home}");
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
    // An unclaimed issue's Change page ends in the refusal `land` gives, and its diff text is no page view.
    let (status, change) = server.json("/api/bp/change/projects/bp/issues/0001.a");
    assert_eq!(
        (status, change["refusal"]["code"].as_str(), change.get("command")),
        (200, Some("illegal_transition"), None)
    );
    assert_eq!((change["gates"].as_array().map(Vec::len), change["head"].as_str()), (Some(0), Some("forge not read")));
    assert_eq!(server.json("/api/bp/change/projects/bp/issues/0001.a?diff=1").0, 200);
    // The Milestone and Design pages are logged; a diagram's SVG is a part of the Design page, not a view.
    let (status, m) = server.json("/api/bp/milestone/projects/bp/milestones/m");
    assert_eq!((status, m["intent"].as_str(), m["title"].as_str()), (200, Some("needs-milestone-approval"), Some("M")));
    // The page's one chip is the milestone row's own Moving command (32, design change 8).
    assert_eq!(m["command"], moving["groups"][1]["rows"][0]["command"], "{m}");
    assert_eq!(m["rows_not_in_vault"].as_array().map(Vec::len), Some(4), "{m}");
    let (status, d) = server.json("/api/bp/design/projects/bp/designs/system");
    assert_eq!((status, d["version"].as_str(), d["level"].as_str()), (200, Some("no version"), Some("system")), "{d}");
    assert_eq!(
        (d["diagrams"][0]["name"].as_str(), d["outline"][0]["anchor"].as_str()),
        (Some("context"), Some("tl-dr"))
    );
    assert_eq!(server.get("/api/bp/design/projects/bp/designs/system/diagram/context"), (200, "<svg></svg>".into()));
    // A browser that opens the route runs nothing: the policy forbids it, and no script or handler is left to run.
    let (status, head, svg) =
        server.exchange("GET", "/api/bp/design/projects/bp/designs/system/diagram/hostile", &server.host());
    assert_eq!((status, svg.as_str()), (200, "<svg><g/></svg>"), "{head}");
    let policy = "content-security-policy: sandbox; default-src 'none'; style-src 'unsafe-inline'; img-src data:; \
                  font-src data:";
    assert!(head.lines().any(|l| l == policy), "{head}");
    assert!(head.lines().any(|l| l == "x-content-type-options: nosniff"), "{head}");
    for missing in [
        "/api/node/projects/bp/issues/nope",
        "/api/nope",
        "/api/nope/moving",
        "/api/bp/moving/x",
        "/api/bp/change/projects/bp/designs/system",
        "/api/two/change/projects/bp/issues/0001.a",
        "/api/bp/milestone/projects/bp/milestones/nope",
        "/api/bp/milestone/projects/bp/issues/0001.a",
        "/api/bp/design/projects/bp/issues/0001.a",
        "/api/two/design/projects/bp/designs/system",
        "/api/bp/design/projects/bp/designs/system/diagram/nope",
    ] {
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
    let change = "/bp/change/projects/bp/issues/0001.a projects/bp/issues/0001.a";
    let milestone = "/bp/milestone/projects/bp/milestones/m projects/bp/milestones/m";
    let design = "/bp/design/projects/bp/designs/system projects/bp/designs/system";
    assert_eq!(routes, ["/ -", "/bp -", node, "/bp/moving -", change, milestone, design, node, node, node], "{log}");
    assert!(log.lines().all(|l| l.as_bytes()[19] == b'Z'), "{log}");
}

/// A body or a diagram the client's sanitiser misses still fetches nothing from another host: the page's policy, on
/// the root and on every page route, has the browser refuse it (32, design change 10).
#[test]
fn every_page_route_holds_a_policy_that_fetches_from_its_own_origin_only() {
    let dir = vault();
    let server = Server::start(dir.path(), &["--forge", "off"], None);
    let policy = "content-security-policy: default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                  img-src 'self' data:; font-src 'self' data:; connect-src 'self'; base-uri 'none'; \
                  form-action 'none'; frame-ancestors 'none'";
    for page in ["/", "/bp", "/bp/moving", "/bp/design/projects/bp/designs/system?at=blueprint/bp/1"] {
        let (status, head, _) = server.exchange("GET", page, &server.host());
        assert!(status == 200 && head.lines().any(|l| l == policy), "{page}: {head}");
    }
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
fn a_tag_with_slashes_in_the_query_serves_the_body_at_the_tag() {
    let dir = vault();
    let g = |args: &[&str]| git(dir.path(), &[&["-c", "user.name=t", "-c", "user.email=t@t"][..], args].concat());
    g(&["tag", "blueprint/bp/1"]);
    let path = dir.path().join("projects/bp/designs/system.md");
    std::fs::write(&path, std::fs::read_to_string(&path).unwrap().replace("The vault.", "The vault.\n\nA probe line."))
        .unwrap();
    g(&["commit", "-qam", "probe"]);
    let server = Server::start(dir.path(), &["--forge", "off"], None);
    let design = "/api/bp/design/projects/bp/designs/system";
    let d = server.json(design).1;
    assert_eq!(
        (d["version"].as_str(), d["version_tag"].as_str(), d["since"].as_str()),
        (Some("changed since blueprint/bp/1"), Some("blueprint/bp/1"), Some("blueprint/bp/1")),
        "the tag alone names the version, and the banner links the diff since it"
    );
    // Home, its design row and Projects read the same version (32, design change 9).
    let (home, projects) = (server.json("/api/bp").1, server.json("/api/projects").1);
    assert_eq!(
        (home["version"].as_str(), home["designs"][0]["version"].as_str(), projects["rows"][0]["version"].as_str()),
        (Some("blueprint/bp/1"), Some("changed since blueprint/bp/1"), Some("blueprint/bp/1"))
    );
    // The query's `/` arrives as written or escaped; either names the tag.
    for at in ["blueprint/bp/1", "blueprint%2Fbp%2F1"] {
        let (status, d) = server.json(&format!("{design}?at={at}"));
        assert_eq!(
            (status, d["version"].as_str(), d["body"].as_str()),
            (200, Some("at blueprint/bp/1"), Some("## TL;DR\n\nThe vault.")),
            "{at}"
        );
        assert_eq!(d["diff"]["sections"], serde_json::json!(["TL;DR"]), "{at}");
        assert_eq!(d["diff"]["added"], serde_json::json!([{ "section": "TL;DR", "lines": ["A probe line."] }]), "{at}");
    }
    let (status, refusal) = server.json(&format!("{design}?at=blueprint/bp/9"));
    assert_eq!((status, refusal["code"].as_str()), (404, Some("no_tag")));
    assert_eq!(refusal["fix"], "git tag -l 'blueprint/bp/*' lists the versions");
    let log = std::fs::read_to_string(dir.path().join(".bp/use.log")).unwrap();
    let routes: Vec<&str> = log.lines().map(|l| l.split_once(' ').unwrap().1).collect();
    assert_eq!(
        routes,
        ["/bp/design/projects/bp/designs/system projects/bp/designs/system", "/bp -", "/ -"]
            .into_iter()
            .chain(["/bp/design/projects/bp/designs/system projects/bp/designs/system"; 2])
            .collect::<Vec<_>>(),
        "a tag is the same page"
    );
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

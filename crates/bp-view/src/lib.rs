//! The human view's server (18, Server; 32, design changes 2 to 4 and 10): one process on a loopback port that serves
//! the client and a JSON API of the verbs' own objects. Each request opens the vault as a verb does, so the index
//! refresh applies and no watcher runs. It writes under `.bp/` only: the index, the forge snapshot and the use log.

mod client;
pub mod route;
pub mod uselog;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bp_vault::{Code, Filter, Index, Kind, Refusal, Vault};
use serde_json::Value;

pub use route::Route;

/// How often the view reads the forge for one project, at most (18, Forge reader: a choice).
const FORGE_EVERY: Duration = Duration::from_secs(60);

/// What every request shares: the vault, and the forge's state.
pub struct View {
    vault: Vault,
    /// The clone whose `gh` reads the forge.
    clone: PathBuf,
    /// The forge is off: `--forge off`, or a read failed, after which the view never calls `gh` again.
    forge_off: AtomicBool,
    /// When the view last read each project's forge.
    read_at: Mutex<HashMap<String, Instant>>,
}

impl View {
    /// The view of the vault at `root`, reading the forge from `clone` unless `forge` is false. `--vault` names the
    /// vault outright, so a `BP_VAULT` in the environment cannot send the view to another.
    pub fn new(root: &std::path::Path, clone: PathBuf, forge: bool) -> Result<Self, Refusal> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if !root.join("projects").is_dir() || !root.join("shared").is_dir() {
            return Err(Refusal::new(
                Code::NoVault,
                root.display().to_string(),
                "a vault is a directory that holds projects/ and shared/",
                "bp-view --vault <the vault root>",
            ));
        }
        Ok(Self { vault: Vault { root }, clone, forge_off: AtomicBool::new(!forge), read_at: Mutex::default() })
    }

    /// The page object of `route`, as the verbs print it: keys sorted at every level. A page route is logged.
    pub fn page(&self, route: &Route) -> Result<Value, Refusal> {
        // Health reads nothing, so it answers while the index refreshes.
        if *route == Route::Health {
            return Ok(serde_json::json!({ "ok": true }));
        }
        let (vault, index) = (&self.vault, Index::open(&self.vault)?);
        let object = match route {
            Route::Health => unreachable!("answered above"),
            Route::Projects => {
                let filter =
                    Filter { kind: Kind::Project, status: None, project: None, under: None, limit: bp_read::ALL };
                to_value(&bp_read::list(vault, &index, &filter, false)?)
            }
            Route::Home(project) => {
                self.refresh(&index, project);
                to_value(&bp_read::home(vault, &index, project)?)
            }
            Route::Node(id) => {
                let id = bp_read::subject(&index, id)?;
                to_value(&bp_read::show(vault, Some(&index), &id, None, None, false)?)
            }
            Route::Search(q) => {
                let terms: Vec<String> = q.split_whitespace().map(str::to_string).collect();
                if terms.is_empty() {
                    return Err(Refusal::usage("q", "/api/search?q=<terms>"));
                }
                to_value(&bp_read::search(&index, &terms, None, None, SEARCH_LIMIT)?)
            }
        };
        if let Some((page, id)) = route.page() {
            let line = uselog::line(bp_read::clock::now()?, &page, id);
            if let Err(e) = uselog::append(&vault.root, &line) {
                eprintln!("use log not written: {e}");
            }
        }
        Ok(object)
    }

    /// Rewrites `project`'s forge snapshot from the clone, at most once a minute, unless the forge is off. A failed
    /// read turns the forge off and the pages read the snapshot that exists (32, design change 4).
    fn refresh(&self, index: &Index, project: &str) {
        if self.forge_off.load(Ordering::Relaxed) {
            return;
        }
        {
            let mut read_at = self.read_at.lock().expect("no request panics while it holds the lock");
            if read_at.get(project).is_some_and(|at| at.elapsed() < FORGE_EVERY) {
                return;
            }
            read_at.insert(project.to_string(), Instant::now());
        }
        if let Err(r) = bp_read::snapshot::refresh(&self.vault, index, &self.clone, project) {
            self.forge_off.store(true, Ordering::Relaxed);
            eprintln!("forge off: {}: {}: {}", r.code, r.subject, r.fix);
        }
    }
}

/// The rows a search returns, as `bp search` does by default.
const SEARCH_LIMIT: usize = 10;

/// An object through a JSON map, so its keys come out sorted, as the verbs print it.
fn to_value(object: &impl serde::Serialize) -> Value {
    serde_json::to_value(object).expect("a plain struct serialises")
}

/// The server's routes: `/api/` for the page objects, and every other path to the client.
pub fn router(view: Arc<View>) -> Router {
    Router::new().route("/api/{*path}", get(api)).fallback(client::serve).with_state(view)
}

async fn api(
    State(view): State<Arc<View>>,
    Path(path): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(route) = Route::parse(&path, query.get("q").map(String::as_str)) else {
        return refused(&Refusal::new(
            Code::NoNode,
            format!("/api/{path}"),
            "an API route is /api/projects, /api/<project>, /api/node/<id> or /api/search?q=<terms>",
            "curl <url>api/projects",
        ));
    };
    match tokio::task::spawn_blocking(move || view.page(&route)).await {
        Ok(Ok(object)) => json(StatusCode::OK, &object),
        Ok(Err(refusal)) => refused(&refusal),
        Err(e) => refused(&Refusal::failed(format!("/api/{path}"), e.to_string())),
    }
}

/// The refusal envelope, under the status that names it: 404 for an id or route that names nothing.
fn refused(refusal: &Refusal) -> Response {
    let status = match refusal.code {
        Code::NoNode | Code::NoSection => StatusCode::NOT_FOUND,
        Code::Usage => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    json(status, &to_value(refusal))
}

fn json(status: StatusCode, value: &Value) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], value.to_string()).into_response()
}

/// Binds `127.0.0.1:<port>` (0 takes a free port), prints `{"url":"http://127.0.0.1:<port>/"}` once it serves, and
/// serves until SIGINT or SIGTERM.
pub async fn serve(view: View, port: u16) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let url = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
    println!("{}", serde_json::json!({ "url": url }));
    axum::serve(listener, router(Arc::new(view))).with_graceful_shutdown(stop()).await
}

/// Resolves on SIGINT or SIGTERM.
async fn stop() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("a process can listen for SIGTERM");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

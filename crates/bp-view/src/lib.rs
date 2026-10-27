//! The human view's server (18, Server; 32, design changes 2 to 4 and 10): one process on a loopback port that serves
//! the client and a JSON API of the verbs' own objects. Each request opens the vault as a verb does, so the index
//! refresh applies and no watcher runs. It writes under `.bp/` only: the index, the forge snapshot and the use log.

mod client;
mod forge;
pub mod route;
pub mod uselog;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query, Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bp_vault::{Code, Filter, Index, Kind, Refusal, Vault};
use serde_json::Value;

pub use route::Route;

/// What every request shares: the vault, and the forge's state.
pub struct View {
    vault: Vault,
    forge: forge::Forge,
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
        Ok(Self { vault: Vault { root }, forge: forge::Forge::new(clone, forge) })
    }

    /// The page object of `route`, as the verbs print it: keys sorted at every level. When `log` is set, a page
    /// object is logged under the page path the browser shows and the id served, never the text the URL held.
    pub fn page(&self, route: &Route, log: bool) -> Result<Value, Refusal> {
        // Health reads nothing, so it answers while the index refreshes.
        if *route == Route::Health {
            return Ok(serde_json::json!({ "ok": true }));
        }
        let (vault, index) = (&self.vault, Index::open(&self.vault)?);
        let (object, page) = match route {
            Route::Health => unreachable!("answered above"),
            Route::Projects => {
                let filter =
                    Filter { kind: Kind::Project, status: None, project: None, under: None, limit: bp_read::ALL };
                (to_value(&bp_read::list(vault, &index, &filter, false)?), Some(("/".to_string(), "-".to_string())))
            }
            Route::Home(project) => {
                // A project the vault does not hold refuses below, and reads no forge.
                if index.header(&format!("projects/{project}/project"))?.is_some() {
                    self.forge.refresh(vault, &index, project);
                }
                (to_value(&bp_read::home(vault, &index, project)?), Some((format!("/{project}"), "-".to_string())))
            }
            Route::Node(id) => {
                let id = bp_read::subject(&index, id)?;
                let object = to_value(&bp_read::show(vault, Some(&index), &id, None, None, false)?);
                (object, Some((format!("/node/{id}"), id.to_string())))
            }
            Route::Search(q) => {
                let terms: Vec<String> = q.split_whitespace().map(str::to_string).collect();
                if terms.is_empty() {
                    return Err(Refusal::usage("q", "/api/search?q=<terms>"));
                }
                (to_value(&bp_read::search(&index, &terms, None, None, SEARCH_LIMIT)?), None)
            }
        };
        if let Some((path, id)) = page.filter(|_| log) {
            let line = uselog::line(bp_read::clock::now()?, &path, &id);
            if let Err(e) = uselog::append(&vault.root, &line) {
                eprintln!("use log not written: {e}");
            }
        }
        Ok(object)
    }
}

/// The rows a search returns, as `bp search` does by default.
const SEARCH_LIMIT: usize = 10;

/// An object through a JSON map, so its keys come out sorted, as the verbs print it.
fn to_value(object: &impl serde::Serialize) -> Value {
    serde_json::to_value(object).expect("a plain struct serialises")
}

/// The server's routes: `/api/` for the page objects, and every other path to the client. A request whose `Host` is
/// not the loopback address of `port` is refused, so a page served from another name (DNS rebinding) reads nothing.
pub fn router(view: Arc<View>, port: u16) -> Router {
    let hosts: Arc<[String; 2]> = Arc::new([format!("127.0.0.1:{port}"), format!("localhost:{port}")]);
    Router::new()
        .route("/api", get(|| async { no_route("") }))
        .route("/api/", get(|| async { no_route("") }))
        .route("/api/{*path}", get(api))
        .fallback(client::serve)
        .with_state(view)
        .layer(from_fn_with_state(hosts, loopback_only))
}

async fn loopback_only(State(hosts): State<Arc<[String; 2]>>, request: Request, next: Next) -> Response {
    let host = request.headers().get(header::HOST).and_then(|h| h.to_str().ok());
    if host.is_some_and(|h| hosts.iter().any(|a| a == h)) {
        return next.run(request).await;
    }
    let refusal = Refusal::usage(format!("Host: {}", host.unwrap_or_default()), "open the view at its printed url");
    json(StatusCode::FORBIDDEN, &to_value(&refusal))
}

/// `path` arrives percent-decoded. A `GET` that serves a page object is logged; a `HEAD` is not.
async fn api(
    State(view): State<Arc<View>>,
    method: Method,
    Path(path): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    // No id holds a space or a control character; one in the URL would split or forge a use log line.
    if path.contains(|c: char| c.is_whitespace() || c.is_control()) {
        return refused(&Refusal::usage(format!("/api/{path}"), "an API path holds no space or control character"));
    }
    let Some(route) = Route::parse(&path, query.get("q").map(String::as_str)) else {
        return no_route(&path);
    };
    let log = method == Method::GET;
    match tokio::task::spawn_blocking(move || view.page(&route, log)).await {
        Ok(Ok(object)) => json(StatusCode::OK, &object),
        Ok(Err(refusal)) => refused(&refusal),
        Err(e) => refused(&Refusal::failed(format!("/api/{path}"), e.to_string())),
    }
}

fn no_route(path: &str) -> Response {
    refused(&Refusal::new(
        Code::NoNode,
        format!("/api/{path}"),
        "an API route is /api/projects, /api/<project>, /api/node/<id> or /api/search?q=<terms>",
        "curl <url>api/projects",
    ))
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
    let port = listener.local_addr()?.port();
    println!("{}", serde_json::json!({ "url": format!("http://127.0.0.1:{port}/") }));
    axum::serve(listener, router(Arc::new(view), port)).with_graceful_shutdown(stop()).await
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

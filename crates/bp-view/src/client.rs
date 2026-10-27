//! The client (32, design change 3): the Vite build in `web/dist`, inside the release binary and read from disk in
//! a debug build. Every path that is not a file of the build is a page route, so it gets `index.html` and the client
//! routes it. Until the client lands (32, issue 2) the build is absent and a placeholder stands in.

use axum::http::{Uri, header};
use axum::response::{IntoResponse, Response};

#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
#[allow_missing = true]
struct Dist;

/// The page a build without the client serves: the client's mount point, so a block can tell the page came.
const PLACEHOLDER: &str = "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>bp-view</title></head>\n<body><div id=\"root\"></div><p>bp-view: the client is not built into this binary.</p></body></html>\n";

/// The file `uri` names in the build, else `index.html`, else the placeholder.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let file = Dist::get(path)
        .filter(|_| !path.is_empty())
        .map(|f| (f, path))
        .or_else(|| Dist::get("index.html").map(|f| (f, "index.html")));
    match file {
        Some((f, name)) => ([(header::CONTENT_TYPE, content_type(name))], f.data.into_owned()).into_response(),
        None => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], PLACEHOLDER).into_response(),
    }
}

/// The media type of a build file, by its extension: the few kinds a Vite build writes.
fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map_or("", |(_, ext)| ext) {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

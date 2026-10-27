//! The client (32, design changes 3 and 12): the Vite build in `web/dist`, inside the release binary and read from
//! disk in a debug build. A path that is not a file of the build is a page route, which gets `index.html` for the
//! client to route, or an asset the build lacks, which gets 404. A build made before `npm run build` holds no client,
//! and a placeholder page stands in.

use std::borrow::Cow;

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

/// The build, relative to this crate's manifest directory. `rust-embed` expands no `$VAR` without its
/// `interpolate-folder-path` feature, and with `allow_missing` a folder it cannot find embeds nothing, silently; so
/// the path is relative, and a test holds it to `web/dist`. `build.rs` rebuilds the crate on a change under it.
#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Dist;

/// The page a build without the client serves: the client's mount point, so a block can tell the page came.
const PLACEHOLDER: &str = concat!(
    "<!doctype html>\n",
    "<html lang=\"en\"><head><meta charset=\"utf-8\"><title>bp-view</title></head>\n",
    "<body><div id=\"root\"></div><p>bp-view: the client is not built into this binary.</p></body></html>\n",
);

/// The file `uri` names in the build; else `index.html` for a page route; else 404 for an asset the build lacks.
pub async fn serve(uri: Uri) -> Response {
    match pick(uri.path(), |name| Dist::get(name).map(|f| f.data)) {
        Some((kind, body)) => ([(header::CONTENT_TYPE, kind)], body.into_owned()).into_response(),
        None => (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], "no such asset\n")
            .into_response(),
    }
}

/// The media type and the bytes for `path`, from the build that `get` reads: the file itself; else, for a page
/// route, `index.html`, or the placeholder in a build without the client. `None` is an asset the build lacks, so a
/// stale script tag gets 404, not a page that parses as no script.
fn pick(path: &str, get: impl Fn(&str) -> Option<Cow<'static, [u8]>>) -> Option<(&'static str, Cow<'static, [u8]>)> {
    let path = path.trim_start_matches('/');
    if let Some(file) = Some(path).filter(|p| !p.is_empty()).and_then(&get) {
        return Some((content_type(path), file));
    }
    if is_asset(path) {
        return None;
    }
    Some(match get("index.html") {
        Some(f) => (content_type("index.html"), f),
        None => ("text/html; charset=utf-8", Cow::Borrowed(PLACEHOLDER.as_bytes())),
    })
}

/// A path names an asset when it sits under `assets/`, where Vite writes, or ends in an extension the build writes.
/// Any other dot is a page route's: an id such as `projects/bp/issues/0411.x` holds one.
fn is_asset(path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("assets/") || last.rsplit_once('.').is_some_and(|(_, ext)| content_type_of(ext).is_some())
}

/// The media type of a build file, by its extension: the few kinds a Vite build writes.
fn content_type(name: &str) -> &'static str {
    name.rsplit_once('.').and_then(|(_, ext)| content_type_of(ext)).unwrap_or("application/octet-stream")
}

fn content_type_of(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "map" | "txt" => "text/plain; charset=utf-8",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The derive's folder.
    const FOLDER: &str = "../../web/dist";

    /// A build of `files`, as `Dist::get` reads one.
    fn build(files: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<Cow<'static, [u8]>> {
        |name| files.iter().find(|(n, _)| *n == name).map(|(_, text)| Cow::Borrowed(text.as_bytes()))
    }

    #[test]
    fn a_page_route_gets_the_page_and_a_missing_asset_404() {
        let empty = build(&[]);
        for path in ["/", "/bp/moving", "/node/projects/bp/issues/0411.x"] {
            assert_eq!(pick(path, &empty), Some(("text/html; charset=utf-8", Cow::Borrowed(PLACEHOLDER.as_bytes()))));
        }
        let client = build(&[("index.html", "<script type=\"module\">"), ("assets/a.js", "let a")]);
        for path in ["/", "/bp/moving", "/node/projects/bp/issues/0411.x", "/node/shared/conventions/convention.prose"]
        {
            let (kind, body) = pick(path, &client).expect(path);
            assert_eq!((kind, &*body), ("text/html; charset=utf-8", &b"<script type=\"module\">"[..]), "{path}");
        }
        assert_eq!(
            pick("/assets/a.js", &client),
            Some(("text/javascript; charset=utf-8", Cow::Borrowed(&b"let a"[..])))
        );
        for path in ["/assets/nope.js", "/assets/x", "/favicon.ico", "/bp/x.css"] {
            assert_eq!(pick(path, &client), None, "{path}");
            assert_eq!(pick(path, &empty), None, "{path}");
        }
    }

    /// The derive names `FOLDER` with no `$`, so `rust-embed` resolves it from the manifest directory to the
    /// workspace's `web/dist`; and a debug build reads that folder, so a file there is served.
    #[test]
    fn the_build_folder_is_the_workspaces_web_dist() {
        assert!(!FOLDER.contains('$'), "{FOLDER}");
        assert!(include_str!("client.rs").contains(&format!("#[folder = \"{FOLDER}\"]")), "the derive names FOLDER");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FOLDER);
        assert!(dir.parent().unwrap().join("src/types").is_dir(), "{} is not under web/", dir.display());
        if dir.join("index.html").is_file() {
            assert!(Dist::get("index.html").is_some(), "web/dist/index.html exists and the build does not hold it");
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The files the server hands out: read-only, and only these paths (as editor/serve.mjs served them with --game):
//!
//! - the game: `/` and `/index.html` (the page; `/?mp` is multiplayer), the dev sandbox's link page `/dev.html` (with
//!   its `/devlink.js`) and tuning panel (`/tune.js`, `/tune.css`), its lobby and touch-control scripts and
//!   styles, both browser builds (`/pkg/`, `/pkg-webgpu/`) and `/assets/`, except the personal source images that
//!   .gitignore keeps out of the repository, and the licences page `/THIRD_PARTY_LICENSES.html`;
//! - the map editor: `/editor/` with its scripts, styles and fonts, the shared map rules and the loose objects'
//!   shapes it draws (`/tools/network/map.mjs`, `/tools/network/presets.mjs`), the maps, and at `/assets/maps/` the
//!   list of maps as JSON ({file, name} each).
//!
//! A path part may not start with a dot, so nothing above the repository can be named; files are streamed with the
//! content type of their extension, `cache-control: no-cache` (a browser asks again, and is told 304 when its copy is
//! current: an ETag, and Last-Modified) and `x-content-type-options: nosniff`.
//!
//! A file may have precompressed copies beside it, `NAME.br` and `NAME.gz` (tools/build-web.sh writes them for the
//! browser builds). A browser that accepts that encoding is sent the copy, with `content-encoding` and
//! `x-decoded-length` (the file's own length, for the page's progress bar), but only while the copy is at least as new
//! as the file: a build that replaces the file and not its copies is sent as it is.
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use regex::Regex;
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;
use tower::ServiceExt;
use tower_http::services::ServeFile;

// (Word characters as JavaScript's \w: ASCII letters, digits and the underscore.)
static ALLOWED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^/editor/(index\.html|[0-9A-Za-z_-]+\.(js|css))$",
        r"^/editor/fonts/[0-9A-Za-z_-]+\.(woff2|txt)$",
        r"^/tools/network/(map|presets)\.mjs$",
        r"^/assets/maps/[0-9A-Za-z_-]+\.json$",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("a valid pattern"))
    .collect()
});
static GAME: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^/(index|dev)\.html$",
        // The licences of everything in the game and where its source is (tools/build-licenses.sh), which the page and the menu link to.
        r"^/THIRD_PARTY_LICENSES\.html$",
        r"^/(lobby|touch|tune)\.(js|css)$",
        r"^/devlink\.js$",
        r"^/(pkg|pkg-webgpu|assets)(/[0-9A-Za-z_][0-9A-Za-z_.-]*)+$",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("a valid pattern"))
    .collect()
});
/// The content type of a file, by its extension.
pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "wasm" => "application/wasm",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "png" => "image/png",
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "wgsl" | "md" | "txt" => "text/plain; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}
fn plain(status: StatusCode, text: &'static str, head: bool) -> Response<Body> {
    let mut response = Response::new(if head { Body::empty() } else { Body::from(text) });
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}
/// The path of a request as the page named it: dot segments resolved (as a browser's URL parser resolves them, `%2e`
/// included), then percent-decoded. None for a path that cannot be decoded (a stray `%`, bytes that are not UTF-8).
pub fn decode(raw: &str) -> Option<String> {
    let mut parts: Vec<&str> = vec![];
    let segments: Vec<&str> = raw.split('/').skip(1).collect();
    for (i, segment) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match segment.to_ascii_lowercase().as_str() {
            "." | "%2e" => {
                if last {
                    parts.push("");
                }
            }
            ".." | ".%2e" | "%2e." | "%2e%2e" => {
                parts.pop();
                if last {
                    parts.push("");
                }
            }
            _ => parts.push(segment),
        }
    }
    let path = format!("/{}", parts.join("/"));
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
/// The list of maps the editor's Open dialog shows: every map file with the name stored inside it.
async fn listing(root: &Path, head: bool) -> Response<Body> {
    let mut files = vec![];
    if let Ok(mut dir) = tokio::fs::read_dir(root.join("assets/maps")).await {
        while let Ok(Some(entry)) = dir.next_entry().await {
            let file = entry.file_name().to_string_lossy().to_string();
            if file.strip_suffix(".json").is_some_and(crate::lobby::map_name) {
                files.push(file);
            }
        }
    }
    files.sort();
    let mut maps = vec![];
    for file in files {
        let Ok(text) = tokio::fs::read_to_string(root.join("assets/maps").join(&file)).await else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let name = match value.get("name") {
            None | Some(serde_json::Value::Null) => file.clone(),
            Some(serde_json::Value::String(name)) => name.clone(),
            Some(other) => other.to_string(),
        };
        maps.push(serde_json::json!({"file": file, "name": name}));
    }
    let text = serde_json::to_string(&maps).unwrap_or_else(|_| "[]".into());
    let mut response = Response::new(if head { Body::empty() } else { Body::from(text) });
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}
/// Answers a request for a file.
pub async fn serve(root: &Path, method: &Method, raw_path: &str, headers: &HeaderMap) -> Response<Body> {
    let head = method == Method::HEAD;
    if method != Method::GET && !head {
        let mut response = plain(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed\n", false);
        response.headers_mut().insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }
    let Some(mut path) = decode(raw_path) else {
        return plain(StatusCode::BAD_REQUEST, "Bad request\n", head);
    };
    if path == "/editor" {
        let mut response = plain(StatusCode::FOUND, "", head);
        response.headers_mut().insert(header::LOCATION, HeaderValue::from_static("/editor/"));
        return response;
    }
    // The game is the site's front page: http://HOST:PORT/?mp plays online.
    if path == "/" {
        path = "/index.html".into();
    }
    if path == "/editor/" {
        path = "/editor/index.html".into();
    }
    if path == "/assets/maps/" {
        return listing(root, head).await;
    }
    let allowed = ALLOWED.iter().any(|pattern| pattern.is_match(&path));
    let game = GAME.iter().any(|pattern| pattern.is_match(&path));
    if !allowed && !game {
        return plain(StatusCode::NOT_FOUND, "Not found\n", head);
    }
    let file = root.join(&path[1..]);
    let Some(info) = tokio::fs::metadata(&file).await.ok().filter(|info| info.is_file()) else {
        return plain(StatusCode::NOT_FOUND, "Not found\n", head);
    };
    let mime = content_type(&path).parse().unwrap_or(mime_guess_fallback());
    let (sent, sent_info, encoding) = match compressed(&file, &info, headers).await {
        Some((copy, copy_info, encoding)) => (copy, copy_info, Some(encoding)),
        None => (file, info.clone(), None),
    };
    let tag = etag(&sent_info, encoding);
    let answer = |mut response: Response<Body>| {
        let out = response.headers_mut();
        out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        out.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        out.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
        if let Ok(value) = HeaderValue::from_str(&tag) {
            out.insert(header::ETAG, value);
        }
        if let Some(encoding) = encoding.filter(|_| response.status() != StatusCode::NOT_MODIFIED) {
            let out = response.headers_mut();
            out.insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
            out.insert("x-decoded-length", HeaderValue::from(info.len()));
        }
        response
    };
    // A browser whose copy is current is answered 304: by its ETag (If-None-Match), or else by its date.
    let none_match = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok());
    if let Some(list) = none_match
        && list.split(',').map(|t| t.trim().trim_start_matches("W/")).any(|t| t == tag || t == "*")
    {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        return answer(response);
    }
    // Streamed, and ranges are honoured.
    let mut request = Request::builder().method(method.clone()).uri("/").body(Body::empty()).expect("a valid request");
    for name in [header::IF_MODIFIED_SINCE, header::IF_UNMODIFIED_SINCE, header::RANGE, header::IF_RANGE] {
        // (If-None-Match, when sent, decides alone.)
        if name == header::IF_MODIFIED_SINCE && none_match.is_some() {
            continue;
        }
        if let Some(value) = headers.get(&name) {
            request.headers_mut().insert(name, value.clone());
        }
    }
    let Ok(response) = ServeFile::new_with_mime(&sent, &mime).oneshot(request).await;
    let response = response.map(Body::new);
    if response.status() == StatusCode::NOT_FOUND {
        return plain(StatusCode::NOT_FOUND, "Not found\n", head);
    }
    answer(response)
}
/// Whether a request's Accept-Encoding takes `coding` (not when it gives it q=0).
fn accepts(headers: &HeaderMap, coding: &str) -> bool {
    headers.get_all(header::ACCEPT_ENCODING).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(',')).any(|item| {
        let mut parts = item.split(';');
        let name = parts.next().unwrap_or("").trim();
        let q = parts.find_map(|p| p.trim().strip_prefix("q=")).and_then(|q| q.trim().parse::<f32>().ok()).unwrap_or(1.);
        name.eq_ignore_ascii_case(coding) && q > 0.
    })
}
/// The precompressed copy of `file` to send instead (its path, its metadata and the encoding): brotli before gzip,
/// each only when the request takes it and the copy is not older than the file. Never for a range.
async fn compressed(file: &Path, info: &Metadata, headers: &HeaderMap) -> Option<(PathBuf, Metadata, &'static str)> {
    if headers.contains_key(header::RANGE) {
        return None;
    }
    for (encoding, extension) in [("br", "br"), ("gzip", "gz")] {
        if !accepts(headers, encoding) {
            continue;
        }
        let mut name = file.as_os_str().to_owned();
        name.push(".");
        name.push(extension);
        let copy = PathBuf::from(name);
        if let Ok(copy_info) = tokio::fs::metadata(&copy).await
            && copy_info.is_file()
            && copy_info.modified().ok() >= info.modified().ok()
        {
            return Some((copy, copy_info, encoding));
        }
    }
    None
}
/// A file's ETag: its length and modification time, and the encoding it is sent in.
fn etag(info: &Metadata, encoding: Option<&str>) -> String {
    let time = info.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    format!("\"{:x}-{:x}{}\"", info.len(), time, encoding.map_or(String::new(), |e| format!("-{e}")))
}
fn mime_guess_fallback() -> mime::Mime {
    mime::APPLICATION_OCTET_STREAM
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_are_resolved_and_decoded_as_a_browser_names_them() {
        assert_eq!(decode("/assets/maps/x.json").as_deref(), Some("/assets/maps/x.json"));
        assert_eq!(decode("/assets/../index.html").as_deref(), Some("/index.html"));
        assert_eq!(decode("/assets/%2e%2E/index.html").as_deref(), Some("/index.html"));
        assert_eq!(decode("/../../etc/passwd").as_deref(), Some("/etc/passwd"));
        assert_eq!(decode("/pkg/a%20b.js").as_deref(), Some("/pkg/a b.js"));
        assert_eq!(decode("/pkg/%zz"), None);
        assert_eq!(decode("/pkg/%ff"), None);
        assert_eq!(decode("/editor/.").as_deref(), Some("/editor/"));
        // Decoded slashes and dots never get past the patterns: every part must start with a word character.
        let path = decode("/assets/..%2F..%2Fetc%2Fpasswd").unwrap();
        assert!(!GAME.iter().any(|p| p.is_match(&path)) && !ALLOWED.iter().any(|p| p.is_match(&path)), "{path}");
    }
    #[test]
    fn the_game_pages_and_scripts_are_served_and_nothing_beside_them() {
        let game = |path: &str| GAME.iter().any(|p| p.is_match(path));
        for path in ["/index.html", "/dev.html", "/devlink.js", "/tune.js", "/tune.css", "/lobby.js", "/touch.css", "/THIRD_PARTY_LICENSES.html"] {
            assert!(game(path), "{path}");
        }
        for path in ["/README.md", "/devlink.css", "/other.html", "/tune.mjs", "/Cargo.toml", "/THIRD_PARTY_NOTICES.md", "/THIRD_PARTY_LICENSES.htm", "/x/THIRD_PARTY_LICENSES.html"] {
            assert!(!game(path), "{path}");
        }
    }
    #[tokio::test]
    async fn the_licences_page_is_served_from_the_root_as_html() {
        let root = std::env::temp_dir().join(format!("hotdog-licences-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("THIRD_PARTY_LICENSES.html"), b"<!doctype html><title>Licences</title>").unwrap();
        let response = serve(&root, &Method::GET, "/THIRD_PARTY_LICENSES.html", &HeaderMap::new()).await;
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(parts.headers.get(header::CONTENT_TYPE).unwrap(), "text/html; charset=utf-8");
        assert_eq!(&axum::body::to_bytes(body, 1 << 20).await.unwrap()[..], b"<!doctype html><title>Licences</title>");
        // Beside it, only what the page lists: the notes and the licence text are not served from the root.
        std::fs::write(root.join("THIRD_PARTY_NOTICES.md"), b"notes").unwrap();
        let response = serve(&root, &Method::GET, "/THIRD_PARTY_NOTICES.md", &HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(&root);
    }
    #[tokio::test]
    async fn a_precompressed_copy_goes_to_a_browser_that_takes_it_while_it_is_as_new_as_the_file() {
        let root = std::env::temp_dir().join(format!("hotdog-statics-{}", std::process::id()));
        let pkg = root.join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("game.wasm"), b"the whole module").unwrap();
        std::fs::write(pkg.join("game.wasm.br"), b"br").unwrap();
        std::fs::write(pkg.join("game.wasm.gz"), b"gz").unwrap();
        let ask = |pairs: &[(header::HeaderName, &str)]| {
            let mut headers = HeaderMap::new();
            for (name, value) in pairs {
                headers.insert(name.clone(), value.parse().unwrap());
            }
            headers
        };
        let get = async |headers: HeaderMap| {
            let response = serve(&root, &Method::GET, "/pkg/game.wasm", &headers).await;
            let (parts, body) = response.into_parts();
            let text = |name: header::HeaderName| parts.headers.get(name).map(|v| v.to_str().unwrap().to_string());
            let seen = (parts.status.as_u16(), text(header::CONTENT_ENCODING), text("x-decoded-length".parse().unwrap()), text(header::ETAG), text(header::CONTENT_TYPE));
            (seen, axum::body::to_bytes(body, 1 << 20).await.unwrap().to_vec())
        };
        let chrome = (header::ACCEPT_ENCODING, "gzip, deflate, br, zstd");
        let ((status, encoding, decoded, tag, kind), body) = get(ask(&[chrome.clone()])).await;
        assert_eq!((status, encoding.as_deref(), decoded.as_deref(), kind.as_deref(), &body[..]), (200, Some("br"), Some("16"), Some("application/wasm"), &b"br"[..]));
        let ((_, encoding, ..), body) = get(ask(&[(header::ACCEPT_ENCODING, "gzip, deflate")])).await;
        assert_eq!((encoding.as_deref(), &body[..]), (Some("gzip"), &b"gz"[..]));
        let ((_, encoding, decoded, ..), body) = get(ask(&[(header::ACCEPT_ENCODING, "br;q=0, gzip;q=0")])).await;
        assert_eq!((encoding, decoded, &body[..]), (None, None, &b"the whole module"[..]));
        // A range is of the file itself.
        let ((status, encoding, ..), body) = get(ask(&[chrome.clone(), (header::RANGE, "bytes=0-2")])).await;
        assert_eq!((status, encoding, &body[..]), (206, None, &b"the"[..]));
        // A browser whose copy is current is told so, by its tag.
        let ((status, encoding, _, again, _), body) = get(ask(&[chrome.clone(), (header::IF_NONE_MATCH, tag.as_deref().unwrap())])).await;
        assert_eq!((status, encoding, again, body.len()), (304, None, tag.clone(), 0));
        // A new build of the file without new copies: the file itself, with a new tag.
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(pkg.join("game.wasm"), b"the new module").unwrap();
        let ((status, encoding, _, fresh, _), body) = get(ask(&[chrome.clone(), (header::IF_NONE_MATCH, tag.as_deref().unwrap())])).await;
        assert_eq!((status, encoding, &body[..]), (200, None, &b"the new module"[..]));
        assert_ne!(fresh, tag);
        std::fs::remove_dir_all(&root).unwrap();
    }
}

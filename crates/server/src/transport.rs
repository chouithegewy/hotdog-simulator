// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! One port for everything: HTTP for the files (statics.rs) and the status (`/status`), and WebSocket for the game
//! (`/ws`, or any path but the lobby's) and each page's lobby (`/lobby`). Each socket gets a task of its own that
//! reads what the client sends and writes what the game loop has for it; the game loop (hub.rs) never waits for one.
//!
//! What a socket may do, as the Node server allowed: messages of at most 16 KiB, at most 180 a second (more ends it),
//! its first message within five seconds (hub.rs), and at most 1 MiB waiting to be written to it (outbox.rs). Text
//! must be JSON (anything else ends it); binary is a game socket's commands (hub.rs reads them; the lobby's socket may
//! not send any).
//!
//! Keepalive: every socket is sent a WebSocket ping (a control frame, below the messages above: it is never one of them,
//! and a recording never holds it) every `PING` seconds (--ping), however quiet it is, so that a reverse proxy's idle
//! timeout (nginx's default 60 s) does not cut a lobby that is only sitting there. Browsers and WebSocket libraries
//! answer pings by themselves. A socket that has said nothing at all (a pong or any message) since `MISSED` pings in a row
//! is a dead connection and is let go, as one that breaks the limits above is: its task ends and the game loop is told it
//! has closed.
//!
//! With the server's --lag and --jitter every message is held back on its way (in and out, in order), as a slow
//! network would: for trying the netcode.
use crate::hub::Event;
use crate::outbox::{Out, Outbox, SocketOut};
use crate::statics;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderValue, Response, header};
use axum::response::IntoResponse;
use axum::routing::get;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

/// The largest message a socket may send.
pub const MESSAGE: usize = 16 * 1024;
/// The most messages a socket may send in a second.
pub const RATE: u32 = 180;
/// How often a socket is pinged (--ping): well inside a proxy's idle timeout (nginx 60 s), so that two pings fit in it.
pub const PING: Duration = Duration::from_secs(20);
/// A socket that has answered none of this many pings in a row, and sent nothing, is let go.
pub const MISSED: u32 = 3;

pub struct App {
    pub root: PathBuf,
    pub events: mpsc::UnboundedSender<Event>,
    next: AtomicU64,
    /// Added to every message's way, each way: half the round trip, and the jitter on it either side.
    pub lag: Duration,
    pub jitter: Duration,
    /// The replays folder (replay.rs), listed at /replays and served at /replays/<id>; None: none.
    pub replays: Option<PathBuf>,
    /// How often each socket is pinged (`PING`, --ping); zero: never.
    pub ping: Duration,
}
impl App {
    pub fn new(root: PathBuf, events: mpsc::UnboundedSender<Event>) -> Arc<App> {
        App::slowed(root, events, Duration::ZERO, Duration::ZERO)
    }
    /// An app whose sockets add `lag` to the round trip, give or take `jitter` (see --lag).
    pub fn slowed(root: PathBuf, events: mpsc::UnboundedSender<Event>, lag: Duration, jitter: Duration) -> Arc<App> {
        Arc::new(App { root, events, next: AtomicU64::new(1), lag: lag / 2, jitter: jitter / 2, replays: None, ping: PING })
    }
    /// The app (just made: nothing else holds it yet) with its replays folder.
    pub fn with_replays(app: Arc<App>, replays: Option<PathBuf>) -> Arc<App> {
        match Arc::try_unwrap(app) {
            Ok(app) => Arc::new(App { replays, ..app }),
            Err(app) => app,
        }
    }
    /// The app (just made, as above) whose sockets are pinged every `ping`.
    pub fn with_ping(app: Arc<App>, ping: Duration) -> Arc<App> {
        match Arc::try_unwrap(app) {
            Ok(app) => Arc::new(App { ping, ..app }),
            Err(app) => app,
        }
    }
    /// When a message that starts on its way now arrives: half the round trip, give or take the jitter, but never
    /// before the one before it (`last`).
    fn due(&self, last: &mut Instant) -> Instant {
        let spread = self.jitter.as_secs_f64() * (rand::random::<f64>() * 2. - 1.);
        let delay = Duration::from_secs_f64((self.lag.as_secs_f64() + spread).max(0.));
        *last = (*last).max(Instant::now() + delay);
        *last
    }
    fn slow(&self) -> bool {
        !self.lag.is_zero() || !self.jitter.is_zero()
    }
}
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/replays", get(replays))
        .route("/replays/{id}", get(replay_file))
        .fallback(any)
        .with_state(app)
}
/// The replays there are, newest first (replay.rs `list`).
async fn replays(State(app): State<Arc<App>>) -> Response<Body> {
    let dir = app.replays.clone();
    let list = tokio::task::spawn_blocking(move || dir.map(|dir| crate::replay::list(&dir)).unwrap_or_default()).await.unwrap_or_default();
    let mut response = axum::Json(serde_json::json!({"replays": list})).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}
/// One replay's file (a recording still being written too: it plays up to where it has got).
async fn replay_file(State(app): State<Arc<App>>, axum::extract::Path(id): axum::extract::Path<String>) -> Response<Body> {
    let file = app.replays.as_deref().and_then(|dir| crate::replay::path(dir, &id));
    let bytes = match file {
        Some(path) => tokio::fs::read(&path).await.ok(),
        None => None,
    };
    let Some(bytes) = bytes else {
        return (axum::http::StatusCode::NOT_FOUND, "No such replay").into_response();
    };
    let id = id.trim_end_matches(".replay").to_string();
    let mut response = Response::new(Body::from(bytes));
    let h = response.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{id}.replay\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    response
}
/// The status JSON (see hub.rs `status`).
async fn status(State(app): State<Arc<App>>) -> Response<Body> {
    let (reply, answer) = oneshot::channel();
    let _ = app.events.send(Event::Status { reply });
    let value = answer.await.unwrap_or_default();
    let mut response = axum::Json(value).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    // (Any page may read it: the dev page on one server shows the others' status.)
    response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response
}
/// Everything else: a WebSocket (the lobby's at a path ending in /lobby, the game's at any other), or a file.
async fn any(State(app): State<Arc<App>>, request: Request) -> Response<Body> {
    let (mut parts, _) = request.into_parts();
    let upgrade = parts.headers.get(header::UPGRADE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if upgrade {
        let page = parts.uri.path().trim_end_matches('/').ends_with("/lobby");
        return match WebSocketUpgrade::from_request_parts(&mut parts, &app).await {
            Ok(ws) => ws.max_message_size(MESSAGE).max_frame_size(MESSAGE).on_upgrade(move |socket| connection(app, socket, page)).into_response(),
            Err(rejection) => rejection.into_response(),
        };
    }
    statics::serve(&app.root, &parts.method, parts.uri.path(), &parts.headers).await
}
/// Writes one message the game loop sent, unless the socket is cut off first. Returns whether the socket goes on.
async fn write(socket: &mut WebSocket, out: &SocketOut, message: Out) -> bool {
    let (message, size) = match message {
        Out::Text(text) => {
            let size = text.len();
            (Message::Text(text), size)
        }
        Out::Binary(bytes) => {
            let size = bytes.len();
            (Message::Binary(bytes), size)
        }
        Out::End => {
            // Closed by the server after its last word: the close is sent, and the client given a moment to answer it.
            let _ = tokio::time::timeout(Duration::from_secs(1), socket.send(Message::Close(None))).await;
            let _ = tokio::time::timeout(Duration::from_millis(500), async { while let Some(Ok(_)) = socket.recv().await {} }).await;
            return false;
        }
    };
    tokio::select! {
        biased;
        _ = out.kill.notified() => false,
        written = socket.send(message) => {
            out.queued.fetch_sub(size, Ordering::Relaxed);
            written.is_ok()
        }
    }
}
/// What a socket received, checked: an event for the game loop, or None for something that ends the socket.
fn read(peer: u64, page: bool, message: Message) -> Option<Option<Event>> {
    match message {
        Message::Text(text) => {
            let message = serde_json::from_str(text.as_str()).ok()?;
            Some(Some(Event::Message { peer, message }))
        }
        Message::Binary(bytes) if !page => Some(Some(Event::Binary { peer, bytes: bytes.to_vec() })),
        Message::Ping(_) | Message::Pong(_) => Some(None),
        Message::Binary(_) | Message::Close(_) => None,
    }
}
/// One socket, from its opening to its end.
async fn connection(app: Arc<App>, mut socket: WebSocket, page: bool) {
    let peer = app.next.fetch_add(1, Ordering::Relaxed);
    let (outbox, out, mut outgoing) = Outbox::socket();
    if app.events.send(Event::Open { peer, page, outbox }).is_err() {
        return;
    }
    let (mut window, mut count) = (std::time::Instant::now(), 0u32);
    // With --lag: what is on its way out and in, each with when it arrives.
    let (mut leaving, mut arriving) = (VecDeque::<(Instant, Out)>::new(), VecDeque::<(Instant, Event)>::new());
    let (mut last_out, mut last_in) = (Instant::now(), Instant::now());
    let far = Instant::now() + Duration::from_secs(86400 * 365);
    // The keepalive: when the next ping is due, and how many have gone out since the socket last said anything.
    let ping = (!app.ping.is_zero()).then_some(app.ping);
    let mut ping_due = ping.map_or(far, |every| Instant::now() + every);
    let mut unanswered = 0u32;
    'socket: loop {
        let (out_due, in_due) = (leaving.front().map_or(far, |(due, _)| *due), arriving.front().map_or(far, |(due, _)| *due));
        tokio::select! {
            biased;
            _ = out.kill.notified() => break,
            _ = tokio::time::sleep_until(out_due), if !leaving.is_empty() => {
                while leaving.front().is_some_and(|(due, _)| *due <= Instant::now()) {
                    let (_, message) = leaving.pop_front().unwrap();
                    if !write(&mut socket, &out, message).await {
                        break 'socket;
                    }
                }
            }
            _ = tokio::time::sleep_until(in_due), if !arriving.is_empty() => {
                while arriving.front().is_some_and(|(due, _)| *due <= Instant::now()) {
                    let (_, event) = arriving.pop_front().unwrap();
                    if app.events.send(event).is_err() {
                        break 'socket;
                    }
                }
            }
            _ = tokio::time::sleep_until(ping_due), if ping.is_some() => {
                let every = app.ping;
                ping_due = Instant::now() + every;
                if unanswered >= MISSED {
                    break;
                }
                unanswered += 1;
                // (A ping that cannot even be written in a whole interval is as good as unanswered.)
                let sent = tokio::select! {
                    biased;
                    _ = out.kill.notified() => break,
                    sent = tokio::time::timeout(every, socket.send(Message::Ping(Bytes::new()))) => sent,
                };
                if !matches!(sent, Ok(Ok(()))) {
                    break;
                }
            }
            sent = outgoing.recv() => match sent {
                Some(message) if app.slow() => leaving.push_back((app.due(&mut last_out), message)),
                Some(message) => {
                    if !write(&mut socket, &out, message).await {
                        break;
                    }
                }
                None => break,
            },
            received = socket.recv() => {
                let Some(Ok(message)) = received else {
                    break;
                };
                // (Anything at all shows the socket is there: a pong, or a message from one that is playing.)
                unanswered = 0;
                if !matches!(message, Message::Ping(_) | Message::Pong(_)) {
                    if window.elapsed() > Duration::from_secs(1) {
                        (window, count) = (std::time::Instant::now(), 0);
                    }
                    count += 1;
                    if count > RATE {
                        break;
                    }
                }
                match read(peer, page, message) {
                    None => break,
                    Some(None) => {}
                    Some(Some(event)) if app.slow() => arriving.push_back((app.due(&mut last_in), event)),
                    Some(Some(event)) => {
                        if app.events.send(event).is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }
    let _ = app.events.send(Event::Closed { peer });
}

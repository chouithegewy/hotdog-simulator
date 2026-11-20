// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The keepalive (transport.rs): every socket is pinged at an interval (--ping), a socket that answers stays, and one
//! that has answered none of three pings in a row (and sent nothing) is let go. The pings here are short (a fifth of a
//! second or so, where the server's own are twenty), and the checks are loose about when, exact about what.
//!
//! One socket is a raw TCP stream that speaks WebSocket by hand, so that it can read the server's frames as they are
//! (the ping is a control frame) and not answer them; a WebSocket library, as every browser, answers by itself.
use futures_util::{SinkExt, StreamExt};
use hotdog_server::config;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
/// The ping interval most of these start the server with, in seconds, and the same as a duration.
const EVERY: f64 = 0.25;
fn every() -> Duration {
    Duration::from_secs_f64(EVERY)
}

/// A server on a free port of this machine, started with `args` (as on the command line).
async fn serve(args: &[&str]) -> SocketAddr {
    // (No replays: these tests would otherwise record into the repository's replays folder.)
    let mut all: Vec<String> = ["--port", "0", "--root", ROOT, "--replays-keep", "0"].iter().map(|s| s.to_string()).collect();
    all.extend(args.iter().map(|s| s.to_string()));
    let config = config::parse(&all).expect("a valid command line");
    hotdog_server::start(config, false).await.expect("the server starts").addr
}
/// Waits up to `seconds` for `found` to give something.
async fn wait_for<T>(what: &str, seconds: u64, mut found: impl FnMut() -> Option<T>) -> T {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        if let Some(value) = found() {
            return value;
        }
        assert!(tokio::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}
/// The numbers /status says (a plain HTTP request).
async fn status(addr: SocketAddr) -> Value {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"GET /status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut response = vec![];
    stream.read_to_end(&mut response).await.unwrap();
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").expect("a response");
    serde_json::from_slice(&response[split + 4..]).unwrap()
}

/// A WebSocket spoken by hand over TCP: what the server sends is read frame by frame, and nothing is ever answered
/// unless the test says so. Its two halves can be used apart (`split`).
struct Raw {
    reader: Reader,
    writer: Writer,
}
struct Reader(OwnedReadHalf);
struct Writer(OwnedWriteHalf);
/// What one frame was: its opcode (1 text, 2 binary, 8 close, 9 ping, 10 pong) and its payload.
type Frame = (u8, Vec<u8>);
impl Raw {
    async fn open(addr: SocketAddr, path: &str) -> Raw {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        // The response head, a byte at a time so that no frame behind it is taken with it.
        let mut head = vec![];
        while !head.ends_with(b"\r\n\r\n") {
            head.push(stream.read_u8().await.expect("the upgrade is answered"));
        }
        assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
        let (read, write) = stream.into_split();
        Raw { reader: Reader(read), writer: Writer(write) }
    }
    fn split(self) -> (Reader, Writer) {
        (self.reader, self.writer)
    }
    async fn send(&mut self, opcode: u8, payload: &[u8]) {
        self.writer.send(opcode, payload).await;
    }
    async fn text(&mut self, value: Value) {
        self.writer.send(1, value.to_string().as_bytes()).await;
    }
    async fn read(&mut self) -> Option<Frame> {
        self.reader.read().await
    }
}
impl Writer {
    /// Sends one frame (masked, as a client must; its payload under 126 bytes).
    async fn send(&mut self, opcode: u8, payload: &[u8]) {
        assert!(payload.len() < 126);
        let mask = [0x12u8, 0x34, 0x56, 0x78];
        let mut frame = vec![0x80 | opcode, 0x80 | payload.len() as u8];
        frame.extend(mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.0.write_all(&frame).await.unwrap();
    }
}
impl Reader {
    /// The next frame the server sends; None once the connection is over (closed, or cut off). Not to be cancelled
    /// half way (a timeout round it loses what it has read).
    async fn read(&mut self) -> Option<Frame> {
        let mut head = [0u8; 2];
        self.0.read_exact(&mut head).await.ok()?;
        assert_eq!(head[1] & 0x80, 0, "a server's frames are not masked");
        let length = match head[1] & 0x7f {
            126 => self.0.read_u16().await.ok()? as usize,
            127 => self.0.read_u64().await.ok()? as usize,
            n => n as usize,
        };
        let mut payload = vec![0u8; length];
        self.0.read_exact(&mut payload).await.ok()?;
        Some((head[0] & 0x0f, payload))
    }
}

/// A WebSocket client of a library (as a browser is): it answers a ping without being asked. It keeps the messages it is
/// sent, counts the pings it saw, and says whether the connection has ended.
#[derive(Clone)]
struct Client {
    inbox: Arc<Mutex<Vec<Value>>>,
    pings: Arc<AtomicUsize>,
    closed: Arc<AtomicBool>,
    out: mpsc::UnboundedSender<Message>,
}
impl Client {
    async fn open(addr: SocketAddr, path: &str, first: Value) -> Client {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}{path}")).await.expect("the socket opens");
        let (mut write, mut read) = socket.split();
        let (inbox, pings, closed) = (Arc::new(Mutex::new(vec![])), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
        let (out, mut outgoing) = mpsc::unbounded_channel::<Message>();
        let (keep, seen, done) = (inbox.clone(), pings.clone(), closed.clone());
        tokio::spawn(async move {
            while let Some(Ok(message)) = read.next().await {
                match message {
                    Message::Text(text) => keep.lock().unwrap().push(serde_json::from_str(text.as_str()).unwrap()),
                    Message::Binary(bytes) => keep.lock().unwrap().push(json!({"type": "frame", "bytes": bytes.len()})),
                    Message::Ping(_) => {
                        seen.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            done.store(true, Ordering::Relaxed);
        });
        tokio::spawn(async move {
            while let Some(message) = outgoing.recv().await {
                if write.send(message).await.is_err() {
                    break;
                }
            }
        });
        let client = Client { inbox, pings, closed, out };
        client.send(first);
        client
    }
    fn send(&self, message: Value) {
        let _ = self.out.send(Message::Text(message.to_string().into()));
    }
    fn messages(&self) -> Vec<Value> {
        self.inbox.lock().unwrap().clone()
    }
    fn find(&self, kind: &str) -> Option<Value> {
        self.messages().into_iter().find(|m| m["type"] == kind)
    }
    fn snapshots(&self) -> Vec<Value> {
        self.messages().into_iter().filter(|m| m["type"] == "snapshot").collect()
    }
    fn pings(&self) -> usize {
        self.pings.load(Ordering::Relaxed)
    }
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }
    async fn welcome(&self) -> Value {
        wait_for("no welcome", 6, || self.find("welcome")).await
    }
}

#[test]
fn the_ping_is_twenty_seconds_by_default_and_the_command_line_sets_it_or_turns_it_off() {
    let parse = |extra: &[&str]| {
        let mut args: Vec<String> = vec!["--root".into(), ROOT.into()];
        args.extend(extra.iter().map(|s| s.to_string()));
        config::parse(&args)
    };
    assert_eq!(parse(&[]).unwrap().net.ping, Duration::from_secs(20));
    assert_eq!(parse(&["--ping", "7.5"]).unwrap().net.ping, Duration::from_millis(7500));
    assert_eq!(parse(&["--ping", "0"]).unwrap().net.ping, Duration::ZERO);
    for wrong in ["soon", "-1", "901", ""] {
        let error = parse(&["--ping", wrong]).unwrap_err();
        assert!(error.contains("--ping needs a number of seconds from 0 to 900"), "{wrong}: {error}");
    }
    assert!(parse(&["--ping"]).unwrap_err().contains("--ping"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_socket_is_pinged_at_the_interval_and_what_it_is_sent_besides_is_only_the_messages() {
    let addr = serve(&["--ping", &EVERY.to_string()]).await;
    let mut lobby = Raw::open(addr, "/lobby").await;
    lobby.text(json!({"type": "hello", "name": "Idle"})).await;
    // It says its welcome, and then nothing but pings, one every interval: each is answered (a pong), so that it stays.
    let (mut pings, mut others) = (vec![], vec![]);
    while pings.len() < 5 {
        let (opcode, payload) = tokio::time::timeout(Duration::from_secs(10), lobby.read()).await.expect("a frame in ten seconds").expect("the socket stays open while it answers");
        match opcode {
            9 => {
                assert!(payload.is_empty(), "a ping carries nothing");
                pings.push(Instant::now());
                lobby.send(10, &payload).await;
            }
            _ => others.push((opcode, payload)),
        }
    }
    // Every other frame is a text message, as before: the welcome first.
    assert!(others.iter().all(|(opcode, _)| *opcode == 1), "{:?}", others.iter().map(|(opcode, _)| opcode).collect::<Vec<_>>());
    let first: Value = serde_json::from_slice(&others[0].1).unwrap();
    assert_eq!(first["type"], "welcome");
    // The pings are an interval apart: never early, and not much later.
    for pair in pings.windows(2) {
        let gap = (pair[1] - pair[0]).as_secs_f64();
        assert!(gap >= EVERY * 0.9 && gap < EVERY * 8., "two pings {gap} s apart, every {EVERY} s");
    }
    assert_eq!(status(addr).await["lobby"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_socket_that_answers_stays_however_long_it_sits_idle() {
    let addr = serve(&["--ping", &EVERY.to_string()]).await;
    // Both kinds of socket, answering by themselves as a browser does, through eight times the window that cuts one off.
    let lobby = Client::open(addr, "/lobby", json!({"type": "hello", "name": "Sitter"})).await;
    lobby.welcome().await;
    let game = Client::open(addr, "/ws", json!({"type": "join", "version": 12, "json": true})).await;
    game.welcome().await;
    tokio::time::sleep(every() * (hotdog_server::transport::MISSED * 8)).await;
    for (what, client) in [("lobby", &lobby), ("game", &game)] {
        assert!(!client.is_closed(), "the {what} socket was cut off though it answers");
        assert!(client.pings() >= 12, "the {what} socket saw {} pings in {} intervals", client.pings(), hotdog_server::transport::MISSED * 8);
    }
    let now = status(addr).await;
    // (A game socket that joins with no token is a member of the lobby as well: two members, one hotdog played.)
    assert_eq!((now["lobby"].clone(), now["humans"].clone()), (json!(2), json!(1)), "both are still there: {now}");
    // (Nothing but the messages reached either: the pings are not among them.)
    assert!(lobby.messages().iter().chain(game.messages().iter()).all(|m| m["type"].is_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_socket_that_never_answers_is_let_go_after_three_missed_pings() {
    let addr = serve(&["--ping", &EVERY.to_string()]).await;
    for path in ["/lobby", "/ws"] {
        let mut socket = Raw::open(addr, path).await;
        let hello = if path == "/lobby" { json!({"type": "hello", "name": "Gone"}) } else { json!({"type": "join", "version": 12, "name": "Gone"}) };
        socket.text(hello).await;
        let started = Instant::now();
        // It reads everything and answers nothing, as a client whose connection has died would: three pings, then the end.
        let mut pings = 0;
        let ended = tokio::time::timeout(Duration::from_secs(20), async {
            while let Some((opcode, _)) = socket.read().await {
                pings += (opcode == 9) as u32;
            }
        })
        .await;
        assert!(ended.is_ok(), "{path}: the socket that answers nothing was not cut off");
        assert_eq!(pings, hotdog_server::transport::MISSED, "{path}: pings before it was let go");
        // After the third ping has had its interval, and not before.
        let took = started.elapsed().as_secs_f64();
        assert!(took >= EVERY * 3.8, "{path}: let go after {took} s, which is before {} intervals", hotdog_server::transport::MISSED + 1);
    }
    // Both have left: nobody is in the lobby or the game.
    let mut left = false;
    for _ in 0..100 {
        let now = status(addr).await;
        if now["lobby"] == 0 && now["humans"] == 0 && now["players"] == 0 {
            left = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(left, "the sockets that were let go are still counted");
}

#[tokio::test(flavor = "multi_thread")]
async fn anything_a_socket_says_counts_as_an_answer_and_a_server_told_not_to_ping_sends_none() {
    let addr = serve(&["--ping", &EVERY.to_string()]).await;
    // A socket that reads the pings but answers none, and only sends a message of its own now and then (here a pong
    // nobody asked for, which the protocol allows), is as alive as one that answers.
    let mut chatty = Raw::open(addr, "/lobby").await;
    chatty.text(json!({"type": "hello", "name": "Chatty"})).await;
    let (mut reader, mut writer) = chatty.split();
    let pings = Arc::new(AtomicUsize::new(0));
    let (count, ended) = (pings.clone(), Arc::new(AtomicBool::new(false)));
    let over = ended.clone();
    tokio::spawn(async move {
        while let Some((opcode, _)) = reader.read().await {
            count.fetch_add((opcode == 9) as usize, Ordering::Relaxed);
        }
        over.store(true, Ordering::Relaxed);
    });
    for _ in 0..(hotdog_server::transport::MISSED * 4 * 2) {
        tokio::time::sleep(every() / 2).await;
        writer.send(10, b"").await;
    }
    assert!(!ended.load(Ordering::Relaxed), "the socket that kept sending was let go");
    assert!(pings.load(Ordering::Relaxed) >= 6, "{} pings", pings.load(Ordering::Relaxed));
    // --ping 0: none at all, and a socket that says nothing stays.
    let quiet = serve(&["--ping", "0"]).await;
    let mut silent = Raw::open(quiet, "/lobby").await;
    silent.text(json!({"type": "hello", "name": "Quiet"})).await;
    let (mut reader, _writer) = silent.split();
    let seen = Arc::new(Mutex::new(vec![]));
    let keep = seen.clone();
    let reading = tokio::spawn(async move {
        while let Some((opcode, _)) = reader.read().await {
            keep.lock().unwrap().push(opcode);
        }
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!reading.is_finished(), "a socket that is not pinged was let go");
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty() && seen.iter().all(|opcode| *opcode == 1), "frames of a server told to send no pings: {seen:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_in_progress_is_unaffected_by_the_pings() {
    let addr = serve(&["--ping", "0.2", "--countdown", "0.2", "--opening", "0.3"]).await;
    // A page: its lobby socket, then its game's socket with the token the lobby gave it.
    let lobby = Client::open(addr, "/lobby", json!({"type": "hello", "name": "Player"})).await;
    let welcome = lobby.welcome().await;
    let game = Client::open(addr, "/ws", json!({"type": "join", "version": 12, "token": welcome["token"], "json": true})).await;
    let me = game.welcome().await["id"].as_u64().unwrap();
    game.send(json!({"type": "ready", "ready": true}));
    wait_for("the round did not begin", 8, || game.snapshots().into_iter().find(|s| s["stage"] == "Warmup")).await;
    let ammo = |game: &Client| game.snapshots().last().and_then(|s| s["players"].as_array()?.iter().find(|p| p["id"] == me)?["ammo"][1].as_i64());
    let before = (game.snapshots().len(), ammo(&game).unwrap());
    // Two seconds of play: shots every 120 ms, which the replicated game shows, through ten pings of each socket.
    let started = Instant::now();
    let mut seq = 1;
    while started.elapsed() < Duration::from_secs(2) {
        game.send(json!({"type": "input", "commands": [{"seq": seq, "weapon": 1, "fire": true}]}));
        seq += 1;
        tokio::time::sleep(Duration::from_millis(120)).await;
        game.send(json!({"type": "input", "commands": [{"seq": seq, "weapon": 1}]}));
        seq += 1;
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    let after = (game.snapshots().len(), ammo(&game).unwrap());
    assert!(after.0 > before.0 + 30, "frames stopped coming: {} then {}", before.0, after.0);
    assert!(after.1 < before.1, "the shots did not reach the game: ammo {} then {}", before.1, after.1);
    assert!(!game.is_closed() && !lobby.is_closed(), "a socket in a game was cut off");
    assert!(game.pings() >= 8 && lobby.pings() >= 8, "pings seen: game {}, lobby {}", game.pings(), lobby.pings());
    // Every message is JSON with a type, as ever: the pings are not among them.
    assert!(game.messages().iter().chain(lobby.messages().iter()).all(|m| m["type"].is_string()));
}

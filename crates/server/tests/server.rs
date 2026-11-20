// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The server over real sockets, as clients use it: each test starts one on a free port and drives it with WebSocket
//! clients (pages' lobby sockets, games' sockets, the native client's) and plain HTTP. These were the Node server's
//! transport.test.mjs and lobby.test.mjs socket tests, and the rules of its page server, editor/serve.mjs. Most game
//! sockets here ask for the JSON debug encoding (whole frames as JSON, commands as JSON); one decodes the binary frames
//! as the game does.
use futures_util::{SinkExt, StreamExt};
use hotdog_server::config;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// A server on a free port of this machine, started with `args` (as on the command line).
async fn serve(args: &[&str]) -> SocketAddr {
    // (No replays: these tests would otherwise record into the repository's replays folder.)
    let mut all: Vec<String> = ["--port", "0", "--root", ROOT, "--replays-keep", "0"].iter().map(|s| s.to_string()).collect();
    all.extend(args.iter().map(|s| s.to_string()));
    let config = config::parse(&all).expect("a valid command line");
    hotdog_server::start(config, false).await.expect("the server starts").addr
}
/// Waits up to six seconds for `found` to give something.
async fn wait_for<T>(what: &str, mut found: impl FnMut() -> Option<T>) -> T {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    loop {
        if let Some(value) = found() {
            return value;
        }
        assert!(tokio::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}
/// A WebSocket client that keeps everything it is sent.
#[derive(Clone)]
struct Client {
    inbox: Arc<Mutex<Vec<Value>>>,
    closed: Arc<AtomicBool>,
    out: mpsc::UnboundedSender<Message>,
}
impl Client {
    async fn open(addr: SocketAddr, path: &str, first: Value) -> Client {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}{path}")).await.expect("the socket opens");
        let (mut write, mut read) = socket.split();
        let (inbox, closed) = (Arc::new(Mutex::new(vec![])), Arc::new(AtomicBool::new(false)));
        let (out, mut outgoing) = mpsc::unbounded_channel::<Message>();
        let (keep, done) = (inbox.clone(), closed.clone());
        tokio::spawn(async move {
            while let Some(Ok(message)) = read.next().await {
                match message {
                    Message::Text(text) => keep.lock().unwrap().push(serde_json::from_str(text.as_str()).unwrap()),
                    // A binary frame, in its place among the rest.
                    Message::Binary(bytes) => keep.lock().unwrap().push(json!({"type": "frame", "bytes": bytes.to_vec()})),
                    _ => {}
                }
            }
            done.store(true, Ordering::Relaxed);
        });
        tokio::spawn(async move {
            while let Some(message) = outgoing.recv().await {
                let close = matches!(message, Message::Close(_));
                if write.send(message).await.is_err() || close {
                    break;
                }
            }
        });
        let client = Client { inbox, closed, out };
        client.send(first);
        client
    }
    fn send(&self, message: Value) {
        let _ = self.out.send(Message::Text(message.to_string().into()));
    }
    fn raw(&self, message: Message) {
        let _ = self.out.send(message);
    }
    fn close(&self) {
        let _ = self.out.send(Message::Close(None));
    }
    fn messages(&self) -> Vec<Value> {
        self.inbox.lock().unwrap().clone()
    }
    fn last(&self, kind: &str) -> Option<Value> {
        self.messages().into_iter().rev().find(|m| m["type"] == kind)
    }
    fn find(&self, kind: &str) -> Option<Value> {
        self.messages().into_iter().find(|m| m["type"] == kind)
    }
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }
    async fn welcome(&self) -> Value {
        wait_for("no welcome", || self.find("welcome")).await
    }
    async fn error(&self) -> String {
        wait_for("no error", || self.find("error")).await["message"].as_str().unwrap().to_string()
    }
}
/// A browser: its page's lobby socket, then its game's socket with the token the lobby gave it.
struct Browser {
    lobby: Client,
    game: Client,
    member: u64,
    player: u64,
}
async fn browser(addr: SocketAddr, hello: Value) -> Browser {
    let mut hello = hello;
    hello["type"] = json!("hello");
    let lobby = Client::open(addr, "/lobby", hello).await;
    let welcome = lobby.welcome().await;
    let game = Client::open(addr, "/ws", json!({"type": "join", "version": 12, "token": welcome["token"], "json": true})).await;
    let seat = game.welcome().await;
    Browser { lobby, game, member: welcome["id"].as_u64().unwrap(), player: seat["id"].as_u64().unwrap() }
}
/// A plain HTTP request; the status, the headers (lower-case names) and the body.
async fn http(addr: SocketAddr, method: &str, path: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut response = vec![];
    stream.read_to_end(&mut response).await.unwrap();
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").expect("a response");
    let head = String::from_utf8_lossy(&response[..split]).to_string();
    let mut lines = head.lines();
    let status = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    let headers = lines.filter_map(|l| l.split_once(": ")).map(|(k, v)| (k.to_ascii_lowercase(), v.to_string())).collect();
    (status, headers, response[split + 4..].to_vec())
}
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}
async fn status(addr: SocketAddr) -> Value {
    let (code, _, body) = http(addr, "GET", "/status").await;
    assert_eq!(code, 200);
    serde_json::from_slice(&body).unwrap()
}
fn snapshots(client: &Client) -> Vec<Value> {
    client.messages().into_iter().filter(|m| m["type"] == "snapshot").collect()
}
fn player(snapshot: &Value, id: u64) -> &Value {
    snapshot["players"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap()
}
/// A JSON `input` message (the debug encoding) with one command, numbered `seq`, that says `what`.
fn command(seq: u64, what: Value) -> Value {
    let mut c = what;
    c["seq"] = json!(seq);
    json!({"type": "input", "commands": [c]})
}
/// A binary frame's bytes, from where a client keeps it.
fn bytes(frame: &Value) -> Vec<u8> {
    frame["bytes"].as_array().unwrap().iter().map(|b| b.as_u64().unwrap() as u8).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_native_style_client_and_a_browser_share_a_live_authoritative_round() {
    let addr = serve(&["--countdown", "0.2", "--opening", "0.3"]).await;
    // A client without a page (the native one) joins with the name and colour it asks for; the browser game here has no
    // page either and is given a name of its own.
    let native = Client::open(addr, "/ws", json!({"type": "join", "version": 12, "name": "Native Nell", "color": "green", "json": true})).await;
    let browser = Client::open(addr, "/", json!({"type": "join", "version": 12, "json": true})).await;
    let (a, b) = (native.welcome().await, browser.welcome().await);
    assert_ne!(a["id"], b["id"]);
    // The welcome names the protocol and carries the map the server is running, in canonical form: by default the bundled one.
    assert_eq!(json!([a["version"], a["tick"], a["send"]]), json!([12, 60, 30]));
    assert_eq!(a["map"].as_str().unwrap(), std::fs::read_to_string(format!("{ROOT}/assets/maps/counter-siege.json")).unwrap());
    assert_eq!(b["map"], a["map"]);
    let (nell, other) = (a["id"].as_u64().unwrap(), b["id"].as_u64().unwrap());
    // The first snapshots carry the map's loose objects: the bundled map's bowl, where the map puts it, on both sockets.
    for client in [&native, &browser] {
        let first = wait_for("loose objects were not sent", || snapshots(client).into_iter().find(|s| !s["props"].as_array().unwrap().is_empty())).await;
        let props = first["props"].as_array().unwrap();
        assert_eq!(props.iter().map(|q| q["id"].clone()).collect::<Vec<_>>(), [json!(0)]);
        let p = &props[0]["p"];
        assert!((p[0].as_f64().unwrap() - 1.2).abs() < 0.01 && (p[2].as_f64().unwrap() - 2.75).abs() < 0.01 && props[0]["q"].as_array().unwrap().len() == 4);
        assert!(first["knocks"].is_array());
    }
    wait_for("players were not replicated", || snapshots(&native).iter().any(|s| s["players"].as_array().unwrap().len() == 5).then_some(())).await;
    // Rounds start from the lobby: nothing happens on the counter until both have said they are ready.
    let waiting = wait_for("both players were not seated", || snapshots(&browser).into_iter().rev().find(|s| s["players"].as_array().unwrap().iter().filter(|p| p["human"] == true).count() == 2)).await;
    assert_eq!((waiting["stage"].clone(), waiting["countdown"].clone()), (json!("Lobby"), Value::Null));
    assert_eq!(player(&waiting, nell)["profile"], json!({"name": "Native Nell", "color": "green"}));
    assert_eq!(player(&waiting, other)["name"], "HOTDOG");
    native.send(command(1, json!({"weapon": 1, "fire": true})));
    native.send(json!({"type": "ready", "ready": true}));
    wait_for("the native client was not ready", || snapshots(&browser).iter().any(|s| player(s, nell)["ready"] == true).then_some(())).await;
    assert!(snapshots(&browser).iter().all(|s| s["stage"] == "Lobby"), "one of two ready does not start the round");
    browser.send(json!({"type": "ready", "ready": true}));
    wait_for("the fridge did not open", || (snapshots(&native).iter().any(|s| s["stage"] == "Opening") && snapshots(&browser).iter().any(|s| s["stage"] == "Opening")).then_some(())).await;
    let begun = wait_for("the round did not begin", || snapshots(&native).into_iter().find(|s| s["stage"] == "Warmup")).await;
    assert_eq!(player(&begun, nell)["ammo"][1], 12, "a shot asked for in the lobby is not fired");
    assert_eq!(player(&begun, nell)["name"], "Native Nell");
    native.send(command(2, json!({"weapon": 1, "fire": true})));
    let replicated = wait_for("the native client's shot did not reach the browser", || snapshots(&browser).into_iter().find(|s| player(s, nell)["ammo"][1] == 11)).await;
    assert!(replicated["shots"].as_array().unwrap().iter().any(|shot| shot["owner"] == nell));
    // Every hotdog carries its aim's pitch and its melee swings; firing along [0, 0, -1] is level.
    for p in replicated["players"].as_array().unwrap() {
        assert!(p["pitch"].is_number() && p["swings"].is_u64());
    }
    assert_eq!(player(&replicated, nell)["pitch"], 0.0);
    // Every snapshot carries the emergences under way (none during the warm-up) beside the zombies.
    assert_eq!((replicated["emergences"].clone(), replicated["enemies"].clone()), (json!([]), json!([])));
    browser.send(command(1, json!({"weapon": 1, "fire": true})));
    wait_for("the browser's shot did not reach the native client", || snapshots(&native).iter().any(|s| player(s, other)["ammo"][1] == 11).then_some(())).await;
    browser.close();
    wait_for("the player who left was not replaced by a bot", || snapshots(&native).iter().rev().take(3).any(|s| s["players"].as_array().unwrap().len() == 5 && player(s, other)["bot"] == true).then_some(())).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_map_from_the_editor_is_played_and_every_client_is_told_which_and_an_old_client_is_refused() {
    let file = format!("{ROOT}/assets/maps/counter-siege-v2.json");
    let addr = serve(&["--map", &file]).await;
    let text = std::fs::read_to_string(&file).unwrap();
    // A client built for the previous handshake is turned away rather than shown the wrong arena.
    let old = Client::open(addr, "/ws", json!({"type": "join", "version": 11})).await;
    assert!(old.error().await.contains("Protocol version 12"));
    wait_for("the old client's socket was not closed", || old.is_closed().then_some(())).await;
    let current = Client::open(addr, "/ws", json!({"type": "join", "version": 12, "json": true})).await;
    let welcome = current.welcome().await;
    assert_eq!(welcome["map"].as_str().unwrap(), text, "the canonical text");
    let snapshot = wait_for("no snapshot", || snapshots(&current).into_iter().find(|s| s["players"].as_array().unwrap().len() == 5)).await;
    let map: Value = serde_json::from_str(&text).unwrap();
    let pickups: Vec<Value> = snapshot["pickups"].as_array().unwrap().iter().map(|i| json!([i["p"][0], i["p"][2], i["weapon"], i["ammo"]])).collect();
    // (Where the map's pickups are and what they hold; what each gives is the game's rules', whatever the map says.)
    let wanted: Vec<Value> = map["pickups"].as_array().unwrap().iter().map(|i| json!([i["p"][0], i["p"][1], i["weapon"], kitchen_sim::magazine::stock(i["weapon"].as_u64().unwrap() as usize).0])).collect();
    assert_eq!(pickups.len(), wanted.len());
    for (got, want) in pickups.iter().zip(&wanted) {
        for k in 0..2 {
            assert!((got[k].as_f64().unwrap() - want[k].as_f64().unwrap()).abs() < 1e-6, "{got} {want}");
        }
        assert_eq!((got[2].clone(), got[3].clone()), (want[2].clone(), want[3].clone()));
    }
    assert_eq!(status(addr).await["map"], "Counter Siege");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_lobby_starts_on_what_the_command_line_says_a_map_of_assets_maps_by_name_a_mode_or_random() {
    for (args, map, choice) in [
        (vec![], "Counter Siege", ["counter-siege", "siege"]),
        (vec!["--map", "horde-night"], "Horde Night", ["horde-night", "siege"]),
        (vec!["--mode", "forts"], "Countertop Fort", ["pillow-fort", "forts"]),
        (vec!["--map", "random"], "Counter Siege", ["random", "siege"]),
        (vec!["--mode", "forts", "--map", "random"], "Countertop Fort", ["random", "forts"]),
        // The mode is the game's: a siege on a Forts map, Free For All on the bundled map.
        (vec!["--mode", "siege", "--map", "pillow-fort"], "Countertop Fort", ["pillow-fort", "siege"]),
        (vec!["--mode", "ffa"], "Counter Siege", ["counter-siege", "ffa"]),
    ] {
        let addr = serve(&args).await;
        let now = status(addr).await;
        assert_eq!(
            json!([now["map"], now["choice"]["name"], now["choice"]["mode"], now["leader"], now["protocol"], now["players"], now["stage"]]),
            json!([map, choice[0], choice[1], null, 12, 0, "Lobby"]),
            "{args:?}"
        );
    }
}

#[test]
fn an_invalid_or_missing_map_or_option_stops_the_server_with_the_reason() {
    let folder = std::env::temp_dir().join(format!("hotdog-map-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let broken = folder.join("broken.json");
    std::fs::write(&broken, r#"{"format":"hotdog-map","version":1,"bounds":[0,0,2,1],"starts":[{"p":[0.2,0.2]}],"spawns":[]}"#).unwrap();
    let absent = folder.join("absent.json");
    for (args, reason) in [
        (vec!["--map", broken.to_str().unwrap()], "at least one spawn"),
        (vec!["--map", absent.to_str().unwrap()], "No such file"),
        (vec!["--map"], "needs a map file"),
        (vec!["--mode", "forts", "--map", "horde-night"], "Horde Night has no Forts layout (two bases, a field and a bacon spot), so it cannot be played as Forts"),
        (vec!["--mode", "ffa", "--map", "kung-fu-dojo"], "Kung Fu Dojo is a Kung Fu map, so it cannot be played as Free For All"),
        (vec!["--mode", "ctf"], "--mode needs siege or forts"),
        (vec!["--map", "no-such-map"], "No such file"),
        (vec!["--build", "0"], "--build needs a number of seconds from 1 to 900"),
        (vec!["--stale", "1000"], "--stale needs a number of seconds from 1 to 900"),
        (vec!["--ws-port", "9001"], "one port"),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_hotdog-server")).args(["--port", "0", "--root", ROOT]).args(&args).output().unwrap();
        let text = String::from_utf8_lossy(&output.stderr).to_string() + &String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {text}");
        assert!(text.contains(reason), "{args:?}: {text}");
        assert!(!text.contains("Hotdog Simulator at"), "{args:?} did not start");
    }
    let _ = std::fs::remove_dir_all(folder);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_routes_lobby_to_the_lobby_and_ties_each_pages_token_to_its_hotdog() {
    let addr = serve(&["--countdown", "0.3", "--opening", "0.4"]).await;
    let a = browser(addr, json!({"name": "Ann", "color": "blue"})).await;
    let b = browser(addr, json!({"name": "Ben", "color": "green"})).await;
    let c = browser(addr, json!({"name": "ann", "color": "nope"})).await;
    let roster = wait_for("the roster did not list three seated players", || {
        c.lobby.last("roster").filter(|r| r["members"].as_array().unwrap().len() == 3 && r["members"].as_array().unwrap().iter().all(|m| m["player"].is_u64()))
    })
    .await;
    let members: Vec<Value> = roster["members"].as_array().unwrap().iter().map(|m| json!([m["id"], m["name"], m["player"]])).collect();
    assert_eq!(members, [json!([a.member, "Ann", a.player]), json!([b.member, "Ben", b.player]), json!([c.member, "ann 2", c.player])]);
    assert_eq!(roster["stage"], "Lobby");
    let snap = wait_for("no snapshot", || a.game.last("snapshot")).await;
    assert_eq!(snap["stage"], "Lobby");
    let humans: Vec<Value> = snap["players"].as_array().unwrap().iter().filter(|p| p["human"] == true).map(|p| json!([p["id"], p["name"], p["color"]])).collect();
    assert_eq!(humans, [json!([a.player, "Ann", "blue"]), json!([b.player, "Ben", "green"]), json!([c.player, "ann 2", roster["members"][2]["color"]])]);
    assert!(["red", "blue", "green", "yellow", "black"].contains(&roster["members"][2]["color"].as_str().unwrap()));
    // A token that is unknown, or already playing, gets no hotdog.
    for token in [json!("made-up"), Value::Null, json!(roster["members"][0]["id"])] {
        let stray = Client::open(addr, "/ws", json!({"type": "join", "version": 12, "token": token})).await;
        assert!(stray.error().await.contains("reload"));
    }
    // Chat, and signalling to one member only.
    b.lobby.send(json!({"type": "chat", "text": "anyone\u{0} awake?"}));
    wait_for("the chat did not reach every page", || [&a, &b, &c].iter().all(|x| x.lobby.last("chat").is_some_and(|m| m["text"] == "anyone awake?")).then_some(())).await;
    a.lobby.send(json!({"type": "signal", "to": c.member, "data": {"description": {"type": "offer", "sdp": "v=0"}}}));
    wait_for("the offer was not relayed", || c.lobby.last("signal").filter(|m| m["from"] == a.member)).await;
    assert_eq!(b.lobby.last("signal"), None);
    // Lobby messages on a game socket, and game messages on a lobby socket, do nothing.
    a.game.send(json!({"type": "chat", "text": "wrong socket"}));
    a.lobby.send(json!({"type": "input", "fire": true}));
    a.lobby.send(json!({"type": "join", "version": 12}));
    // A fourth and fifth are admitted; a sixth is refused on both sockets.
    let d = browser(addr, json!({"name": "Dee"})).await;
    let e = Client::open(addr, "/ws", json!({"type": "join", "version": 12, "name": "Native", "color": "red", "json": true})).await;
    e.welcome().await;
    let full = Client::open(addr, "/lobby", json!({"type": "hello", "name": "Six"})).await;
    assert!(full.error().await.contains("Kitchen full"));
    let packed = Client::open(addr, "/ws", json!({"type": "join", "version": 12})).await;
    assert!(packed.error().await.contains("Kitchen full"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(a.lobby.messages().iter().all(|m| m["text"] != "wrong socket"));
    // Everyone ready: countdown, the fridge opens, the round begins for all, under their own names.
    for x in [&a, &b, &c, &d] {
        x.lobby.send(json!({"type": "ready", "ready": true}));
    }
    e.send(json!({"type": "ready", "ready": true}));
    wait_for("the fridge did not open", || a.lobby.last("roster").filter(|r| r["stage"] == "Opening")).await;
    let round = wait_for("the round did not begin", || c.game.last("snapshot").filter(|s| s["stage"] == "Warmup")).await;
    let names: Vec<Value> = round["players"].as_array().unwrap().iter().map(|p| p["name"].clone()).collect();
    assert_eq!(names, [json!("Ann"), json!("Ben"), json!("ann 2"), json!("Dee"), json!("Native")]);
    assert!(round["players"].as_array().unwrap().iter().all(|p| p["human"] == true && p["bot"] == false));
    // Two leave; their places are bots again and the lobby says who went.
    d.lobby.close();
    d.game.close();
    e.close();
    wait_for("the leavers stayed in the roster", || a.lobby.last("roster").filter(|r| r["members"].as_array().unwrap().len() == 3)).await;
    let said: Vec<Value> = a.lobby.messages().into_iter().filter(|m| m["type"] == "system").map(|m| m["text"].clone()).collect();
    assert!(said.contains(&json!("Dee left")) && said.contains(&json!("Native left")));
    wait_for("the leavers were not replaced by bots", || a.game.last("snapshot").filter(|s| s["players"].as_array().unwrap().iter().filter(|p| p["bot"] == true).count() == 2)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_lets_the_leader_choose_tells_every_client_the_new_map_before_the_round_on_it_and_says_what_is_next() {
    let addr = serve(&["--countdown", "0.3", "--opening", "0.6"]).await;
    let a = browser(addr, json!({"name": "Ann"})).await;
    let b = browser(addr, json!({"name": "Ben", "team": "ketchup"})).await;
    // A client without a page, and one that decodes the binary frames as the game does (src/network.rs), acknowledging
    // each.
    let native = Client::open(addr, "/ws", json!({"type": "join", "version": 12, "name": "Native", "json": true})).await;
    native.welcome().await;
    let raw = Client::open(addr, "/ws", json!({"type": "join", "version": 12, "name": "Raw"})).await;
    raw.welcome().await;
    let decoded: Arc<Mutex<Vec<(u32, u32)>>> = Arc::new(Mutex::new(vec![]));
    {
        let (raw, decoded) = (raw.clone(), decoded.clone());
        tokio::spawn(async move {
            let (mut states, mut read): (Vec<(u32, kitchen_sim::net::State)>, usize) = (vec![], 0);
            loop {
                let messages = raw.messages();
                for m in &messages[read..] {
                    if m["type"] == "map" {
                        states.clear();
                    }
                    if m["type"] != "frame" {
                        continue;
                    }
                    let bytes = bytes(m);
                    let (tick, base) = kitchen_sim::net::peek_frame(&bytes).unwrap();
                    let state = states.iter().find(|(t, _)| *t == base).map(|(_, s)| s);
                    assert!(base == 0 || state.is_some(), "a frame against {base}, which this client never had");
                    let (header, state, _) = kitchen_sim::net::decode_frame(&bytes, state).unwrap();
                    assert_eq!(state.checksum(), header.checksum);
                    states.push((tick, state));
                    decoded.lock().unwrap().push((tick, base));
                    raw.raw(Message::Binary(kitchen_sim::net::Batch { ack: tick, resync: false, time: 0, commands: vec![] }.encode().into()));
                }
                read = messages.len();
                if raw.is_closed() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
    }
    wait_for("the binary client's frames were never deltas", || decoded.lock().unwrap().iter().any(|(_, base)| *base > 0).then_some(())).await;
    wait_for("Ann does not lead", || b.lobby.last("roster").filter(|r| r["leader"] == a.member)).await;
    let now = status(addr).await;
    assert_eq!(json!([now["map"], now["mode"], now["choice"]["name"], now["leader"], now["maps"], now["protocol"]]), json!(["Counter Siege", "siege", "counter-siege", "Ann", 14, 12]));
    assert_eq!(wait_for("no snapshot", || a.game.last("snapshot")).await["leader"], a.player);
    // Ben may not choose; Ann chooses Forts on Moonlit Fort, and everyone sees it.
    b.lobby.send(json!({"type": "choose", "mode": "forts"}));
    wait_for("Ben was not refused", || b.lobby.last("notice").filter(|n| n["text"] == "Only the leader, Ann, can choose the game.")).await;
    a.lobby.send(json!({"type": "choose", "map": "../moonlit-fort"}));
    wait_for("a path was not refused", || a.lobby.last("notice").filter(|n| n["text"] == "That is not the name of a map.")).await;
    a.lobby.send(json!({"type": "choose", "map": "moonlit-fort"}));
    let chosen = wait_for("the choice did not reach Ben", || b.lobby.last("roster").filter(|r| r["choice"]["name"] == "moonlit-fort")).await;
    assert_eq!(chosen["mode"], "forts");
    let sides: Vec<Value> = chosen["members"].as_array().unwrap().iter().map(|m| json!([m["name"], m["side"]])).collect();
    assert_eq!(sides, [json!(["Ann", 0]), json!(["Ben", 1]), json!(["Native", 0]), json!(["Raw", 1])]);
    let now = status(addr).await;
    assert_eq!(json!([now["map"], now["choice"]["name"], now["choice"]["mode"]]), json!(["Counter Siege", "moonlit-fort", "forts"]));
    let told = wait_for("the native client was not told the choice", || native.last("snapshot").filter(|s| s["choice"]["name"] == "moonlit-fort")).await;
    assert_eq!(told["choice"], json!({"name": "moonlit-fort", "title": "Moonlit Fort", "mode": "forts", "theme": "night"}));
    // Everyone ready: the fridge opens, and before the first snapshot of the new round every game socket is told the map
    // and its hotdog there.
    for x in [&a, &b] {
        x.lobby.send(json!({"type": "ready", "ready": true}));
    }
    native.send(json!({"type": "ready", "ready": true}));
    raw.send(json!({"type": "ready", "ready": true}));
    let moonlit = std::fs::read_to_string(format!("{ROOT}/assets/maps/moonlit-fort.json")).unwrap();
    for client in [&a.game, &b.game, &native] {
        let k = wait_for("no map message", || client.messages().iter().position(|m| m["type"] == "map")).await;
        let next = wait_for("no snapshot after the map", || client.messages()[k + 1..].iter().find(|m| m["type"] == "snapshot").cloned()).await;
        let messages = client.messages();
        let message = &messages[k];
        assert_eq!(message["map"].as_str().unwrap(), moonlit);
        assert!(next["players"].as_array().unwrap().iter().any(|p| p["id"] == message["id"] && p["human"] == true));
        assert_eq!(next["players"].as_array().unwrap().len(), 6);
        assert_eq!(next["stage"], "Opening");
        assert!(messages[..k].iter().all(|m| m["type"] != "snapshot" || m["players"].as_array().unwrap().len() == 5));
    }
    // The binary client: told the map too, and its first frame of the new game is whole (what it had of the old one
    // is no use), then deltas again.
    let k = wait_for("no map message for the binary client", || raw.messages().iter().position(|m| m["type"] == "map")).await;
    let next = wait_for("no frame after the map", || raw.messages()[k + 1..].iter().find(|m| m["type"] == "frame").cloned()).await;
    let (tick, base) = kitchen_sim::net::peek_frame(&bytes(&next)).unwrap();
    assert_eq!(base, 0, "the frame of {tick} after the map is whole");
    wait_for("no delta on the new map", || decoded.lock().unwrap().iter().any(|(t, base)| *t > tick && *base >= tick).then_some(())).await;
    let build = wait_for("the Forts round did not begin", || a.game.last("snapshot").filter(|s| s["stage"] == "Build")).await;
    assert!(build["forts"].is_object());
    let now = status(addr).await;
    assert_eq!(json!([now["map"], now["mode"], now["stage"]]), json!(["Moonlit Fort", "forts", "Build"]));
    // Inputs reach the new game: Ann's hotdog there is the one the map message named.
    let mine = a.game.find("map").unwrap()["id"].as_u64().unwrap();
    a.game.send(command(1, json!({"yaw": 1.25})));
    wait_for("the command did not reach the new game", || b.game.last("snapshot").filter(|s| (player(s, mine)["yaw"].as_f64().unwrap() - 1.25).abs() < 1e-3)).await;
    raw.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_socket_that_floods_sends_too_much_or_speaks_another_language_is_cut_off() {
    let addr = serve(&[]).await;
    // More than 180 messages in a second.
    let flood = Client::open(addr, "/ws", json!({"type": "join", "version": 12})).await;
    flood.welcome().await;
    for _ in 0..200 {
        flood.send(json!({"type": "input", "yaw": 0}));
    }
    wait_for("a flood was not cut off", || flood.is_closed().then_some(())).await;
    // A message of more than 16 KiB.
    let big = Client::open(addr, "/lobby", json!({"type": "hello", "name": "Big"})).await;
    big.welcome().await;
    big.send(json!({"type": "chat", "text": "x".repeat(20000)}));
    wait_for("an oversized message was not cut off", || big.is_closed().then_some(())).await;
    // Something that is not JSON, and a binary message that is not a batch of commands.
    let garbled = Client::open(addr, "/ws", json!({"type": "join", "version": 12})).await;
    garbled.welcome().await;
    garbled.raw(Message::Text("not json".into()));
    wait_for("text that is not JSON was not cut off", || garbled.is_closed().then_some(())).await;
    let binary = Client::open(addr, "/ws", json!({"type": "join", "version": 12})).await;
    binary.welcome().await;
    binary.raw(Message::Binary(vec![1, 2, 3].into()));
    wait_for("a binary message was not cut off", || binary.is_closed().then_some(())).await;
    // A lobby socket's first word must be its hello.
    let rude = Client::open(addr, "/lobby", json!({"type": "chat", "text": "hi"})).await;
    wait_for("a lobby socket that did not say hello was not cut off", || rude.is_closed().then_some(())).await;
    // Everyone who was cut off has left: nobody is in the lobby or the game.
    let mut left = false;
    for _ in 0..100 {
        let now = status(addr).await;
        if now["lobby"] == 0 && now["humans"] == 0 && now["players"] == 0 {
            left = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(left, "the cut-off sockets were not let go");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_socket_that_never_says_who_it_is_is_cut_off_after_five_seconds() {
    let addr = serve(&[]).await;
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await.unwrap();
    let (_write, mut read) = socket.split();
    let started = std::time::Instant::now();
    let ended = tokio::time::timeout(Duration::from_secs(8), async { while let Some(Ok(_)) = read.next().await {} }).await;
    assert!(ended.is_ok(), "the silent socket was not cut off");
    let took = started.elapsed().as_secs_f64();
    assert!((4.8..7.).contains(&took), "cut off after {took} s");
}

#[tokio::test(flavor = "multi_thread")]
async fn files_are_served_read_only_from_the_allow_list_with_their_content_types_and_never_from_outside_it() {
    let addr = serve(&[]).await;
    let get = |path: &'static str| http(addr, "GET", path);
    // The game is the front page; the editor is at /editor/.
    let (code, headers, body) = get("/").await;
    assert_eq!((code, header(&headers, "content-type")), (200, Some("text/html; charset=utf-8")));
    assert_eq!(body, std::fs::read(format!("{ROOT}/index.html")).unwrap());
    assert_eq!(header(&headers, "x-content-type-options"), Some("nosniff"));
    assert_eq!(header(&headers, "cache-control"), Some("no-cache"));
    let (code, headers, _) = get("/editor").await;
    assert_eq!((code, header(&headers, "location")), (302, Some("/editor/")));
    let (code, _, body) = get("/editor/").await;
    assert_eq!(code, 200);
    assert_eq!(body, std::fs::read(format!("{ROOT}/editor/index.html")).unwrap());
    for (path, kind) in [
        ("/index.html", "text/html; charset=utf-8"),
        ("/lobby.js", "text/javascript; charset=utf-8"),
        ("/lobby.css", "text/css; charset=utf-8"),
        ("/touch.js", "text/javascript; charset=utf-8"),
        ("/editor/app.js", "text/javascript; charset=utf-8"),
        ("/editor/editor.css", "text/css; charset=utf-8"),
        ("/tools/network/map.mjs", "text/javascript; charset=utf-8"),
        ("/assets/maps/pillow-fort.json", "application/json; charset=utf-8"),
        ("/assets/models/hotdog-player.glb", "model/gltf-binary"),
    ] {
        let (code, headers, body) = http(addr, "GET", path).await;
        assert_eq!((code, header(&headers, "content-type")), (200, Some(kind)), "{path}");
        assert_eq!(body, std::fs::read(format!("{ROOT}{path}")).unwrap(), "{path}");
        assert_eq!(header(&headers, "content-length").map(|n| n.parse::<usize>().unwrap()), Some(body.len()), "{path}");
    }
    // Both browser builds, streamed with their length.
    for path in ["/pkg/hotdog_simulator_bg.wasm", "/pkg-webgpu/hotdog_simulator_bg.wasm"] {
        let size = std::fs::metadata(format!("{ROOT}{path}")).map(|m| m.len()).unwrap_or(0);
        if size == 0 {
            continue;
        }
        let (code, headers, body) = http(addr, "HEAD", path).await;
        assert_eq!((code, header(&headers, "content-type"), header(&headers, "content-length")), (200, Some("application/wasm"), Some(size.to_string().as_str())), "{path}");
        assert!(body.is_empty(), "HEAD has no body");
    }
    // The list of maps the editor's Open dialog shows.
    let (code, headers, body) = get("/assets/maps/").await;
    assert_eq!((code, header(&headers, "content-type")), (200, Some("application/json; charset=utf-8")));
    let list: Vec<Value> = serde_json::from_slice(&body).unwrap();
    let mut files: Vec<String> = std::fs::read_dir(format!("{ROOT}/assets/maps")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).filter(|f| f.ends_with(".json")).collect();
    files.sort();
    assert_eq!(list.iter().map(|e| e["file"].as_str().unwrap().to_string()).collect::<Vec<_>>(), files);
    assert!(list.contains(&json!({"file": "pillow-fort.json", "name": "Countertop Fort"})));
    // Nothing else: not the source, not the server, not a missing file, not dot files, not outside the repository.
    for path in [
        "/README.md",
        "/Cargo.toml",
        "/src/main.rs",
        "/tools/network/map.test.mjs",
        "/editor/model.test.mjs",
        "/crates/server/src/main.rs",
        "/tools/hotdog-server.service",
        "/assets/missing.png",
        "/.gitignore",
        "/assets/.hidden",
        "/assets/../Cargo.toml",
        "/assets/%2e%2e/Cargo.toml",
        "/assets/..%2fCargo.toml",
        "/pkg/../../../etc/passwd",
        "/editor/fonts/../../Cargo.toml",
        "/assets/maps/missing.json",
        "/pkg/",
    ] {
        let (code, _, body) = http(addr, "GET", path).await;
        assert_eq!((code, String::from_utf8_lossy(&body).to_string()), (404, "Not found\n".to_string()), "{path}");
    }
    let (code, _, _) = get("/assets/%zz").await;
    assert_eq!(code, 400);
    let (code, headers, _) = http(addr, "POST", "/index.html").await;
    assert_eq!((code, header(&headers, "allow")), (405, Some("GET, HEAD")));
    let (code, headers, body) = get("/status").await;
    assert_eq!((code, header(&headers, "content-type")), (200, Some("application/json")));
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["game"], "hotdog-simulator");
}

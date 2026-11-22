// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The connection to the server (protocol 12, kitchen_sim::net): the server owns the round; this client predicts its
//! own hotdog and draws everything else a little in the past (arena/online.rs). Both targets speak the same
//! WebSocket, the server's `/ws` (crates/server): a browser through the page's WebSocket, natively through tungstenite.
//!
//! - **Joining** (text): `{type:'join',version:12}` (with `sandbox` when the address asks for the dev sandbox:
//!   runtime.rs) with, in a browser, the `token` the page's lobby was given
//!   (lobby.js: window.hotdogLobby.token, a promise the game waits for), which ties this hotdog to that page's name,
//!   colour, chat and voice; or, without a page, the `name`, `color` and `team` asked for. The welcome says which
//!   hotdog this client plays, the map, and how many steps and frames a second the server makes.
//! - **Frames** (binary): each is decoded against the state of a frame this client decoded before (the one its header
//!   names), checked against its checksum and acknowledged in the next batch; one that cannot be decoded or does not
//!   check out makes this client ask for a whole frame. The decoded frames go to the lobby and the arena.
//! - **Commands** (binary): every frame of the game, the commands made since the last (arena/online.rs) go out, with
//!   the two before them again and the newest frame decoded (`Batch`); with no command to send, the ack alone.
//! - **The card table** (text, Fully Loaded: cards.rs): the server's `{type:'cards',table}` is kept for the table's
//!   view (`Seen`), and this player's flips, stops and picks (`Wishes`) go as `{type:'cards',action,seat?}`.
//!
//! - **The electric griddle** (text, griddle.rs): the server's `{type:'griddle',griddles}` sets the griddles' states, and a press of use at
//!   a dial goes as `{type:'griddle'}`.
//!
//! `receive` runs before the arena's tick and `send` after it.
use bevy::prelude::*;
use kitchen_sim::net::{self, Batch, Command, Frame, State};
use std::collections::{HashMap, VecDeque};

/// How many decoded frames' states are kept for the server to encode against (two seconds of frames at 30 a second).
const KEPT: usize = 64;
/// How many commands already sent go again with the new ones.
const AGAIN: usize = 2;
/// What came from the server.
pub enum Incoming {
    /// JSON: the welcome, a new map, an error (or, with the debug encoding, a frame).
    Text(String),
    /// A frame.
    Binary(Vec<u8>),
}
/// What goes to the server.
pub enum Outgoing {
    Text(String),
    Binary(Vec<u8>),
}
#[derive(Resource, Default)]
pub struct Network {
    started: bool,
    round: u64,
    remotes: HashMap<u32, (Entity, bool)>,
    /// Each other hotdog's colour and how loudly its player speaks, from the newest frame.
    looks: HashMap<u32, (String, f32, bool)>,
    pub status: String,
    /// The states of the frames decoded lately, by step; the newest step decoded and the newest acknowledged; whether
    /// to ask for a whole frame, and when that was last asked.
    states: VecDeque<(u32, State)>,
    pub newest: u32,
    told: u32,
    resync: bool,
    asked: f64,
    /// The commands sent lately (the newest few go again with each batch), and when this client connected.
    recent: VecDeque<Command>,
    since: f64,
    /// The sandbox's tuning panel's changes waiting to go to the server (tune.rs), as kitchen_sim::tune's text.
    pub tunes: Vec<String>,
    /// A replay being watched (replay/): what the server said, read in place of a socket's (no socket is opened).
    pub replay: Option<VecDeque<Incoming>>,
    /// For the checks: frames decoded, frames that could not be, and whole frames.
    pub frames: u64,
    pub broken: u64,
    pub whole: u64,
    #[cfg(not(target_arch = "wasm32"))]
    outgoing: Option<std::sync::mpsc::Sender<Outgoing>>,
    #[cfg(not(target_arch = "wasm32"))]
    incoming: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Incoming>>>,
}
// The page's side: the socket, what it has received (text as it is, frames as bytes), and how many bytes it has
// received and sent (`window.hotdogNet`, for the checks). `net_read` hands everything over as one buffer of
// [kind (0 text, 1 binary), length (4 bytes, little-endian), bytes]...; a game that stops reading (a hidden tab) has
// its oldest frames dropped, never a text message.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
let socket=null,queue=[],frames=0;const net=window.hotdogNet={rx:0,tx:0,frames:0,opened:0};
export function net_start(url,join){const lobby=window.hotdogLobby;if(lobby&&lobby.token&&typeof lobby.token.then==='function')lobby.token.then(token=>net_open(url,token,join),()=>net_open(url,null,join));else net_open(url,null,join)}
function net_open(url,token,join){try{socket=new WebSocket(url);socket.binaryType='arraybuffer';socket.onopen=()=>{net.opened=performance.now();let m;try{m=JSON.parse(join)}catch{m={type:'join',version:12}}if(token)m.token=token;send(JSON.stringify(m))};
socket.onmessage=e=>{if(typeof e.data==='string'){net.rx+=e.data.length;queue.push(e.data)}else{net.rx+=e.data.byteLength;net.frames++;queue.push(new Uint8Array(e.data));if(++frames>240){const i=queue.findIndex(m=>typeof m!=='string');queue.splice(i,1);frames--}}};
socket.onerror=()=>queue.push(JSON.stringify({type:'error',message:'Could not connect to multiplayer server'}));socket.onclose=()=>queue.push(JSON.stringify({type:'error',message:'Multiplayer disconnected'}));}catch(e){queue.push(JSON.stringify({type:'error',message:String(e)}))}}
function send(data){if(socket&&socket.readyState===1){socket.send(data);net.tx+=typeof data==='string'?data.length:data.byteLength}}
export function net_send(s){send(s)}
export function net_send_bytes(b){send(b.slice())}
export function net_read(){const parts=queue.splice(0);frames=0;const encoder=new TextEncoder();const items=parts.map(m=>typeof m==='string'?[0,encoder.encode(m)]:[1,m]);let n=0;for(const [,b] of items)n+=5+b.length;
const out=new Uint8Array(n),view=new DataView(out.buffer);let at=0;for(const [k,b] of items){out[at]=k;view.setUint32(at+1,b.length,true);out.set(b,at+5);at+=5+b.length}return out}
")]
extern "C" {
    fn net_start(url: &str, join: &str);
    fn net_send(s: &str);
    fn net_send_bytes(b: &[u8]);
    fn net_read() -> Vec<u8>;
}
impl Network {
    fn start(&mut self, address: &str, join: String, now: f64) {
        self.started = true;
        self.since = now;
        // (A replay has no server: its stream is handed over in `replay`.)
        if self.replay.is_some() {
            self.status = "Replay".into();
            return;
        }
        self.status = "Connecting…".into();
        #[cfg(target_arch = "wasm32")]
        net_start(address, &join);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = std::sync::mpsc::channel::<Outgoing>();
            let (messages, received) = std::sync::mpsc::channel();
            let address = address.to_owned();
            self.outgoing = Some(tx);
            self.incoming = Some(std::sync::Mutex::new(received));
            std::thread::spawn(move || connect(address, join, rx, messages));
        }
    }
    fn send(&self, message: Outgoing) {
        #[cfg(target_arch = "wasm32")]
        match message {
            Outgoing::Text(s) => net_send(&s),
            Outgoing::Binary(b) => net_send_bytes(&b),
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(tx) = &self.outgoing {
            let _ = tx.send(message);
        }
    }
    fn read(&self) -> Vec<Incoming> {
        #[cfg(target_arch = "wasm32")]
        {
            let bytes = net_read();
            let (mut out, mut at) = (vec![], 0);
            while at + 5 <= bytes.len() {
                let n = u32::from_le_bytes([bytes[at + 1], bytes[at + 2], bytes[at + 3], bytes[at + 4]]) as usize;
                let Some(body) = bytes.get(at + 5..at + 5 + n) else {
                    break;
                };
                out.push(if bytes[at] == 0 { Incoming::Text(String::from_utf8_lossy(body).into_owned()) } else { Incoming::Binary(body.to_vec()) });
                at += 5 + n;
            }
            out
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.incoming.as_ref().map(|rx| rx.lock().unwrap().try_iter().take(240).collect()).unwrap_or_default()
        }
    }
    /// Forgets the frames decoded (a new map: the next frame is whole).
    fn forget(&mut self) {
        self.states.clear();
        (self.newest, self.told) = (0, 0);
    }
    /// A replay's seek (replay/): the frames decoded are forgotten, and the other hotdogs drawn again from the next.
    pub fn rewind(&mut self) {
        self.forget();
        self.round = u64::MAX;
    }
    /// Decodes a frame against the state of the one its header names, and checks it. None for one that cannot be
    /// decoded (this client then asks for a whole frame) or that is older than the newest.
    pub fn decode(&mut self, bytes: &[u8]) -> Option<Frame> {
        let Ok((tick, base)) = net::peek_frame(bytes) else {
            self.broken += 1;
            return None;
        };
        if tick <= self.newest {
            return None;
        }
        let state = match base {
            0 => None,
            base => match self.states.iter().find(|(t, _)| *t == base) {
                Some((_, state)) => Some(state),
                None => {
                    (self.resync, self.broken) = (true, self.broken + 1);
                    return None;
                }
            },
        };
        let Ok((header, state, events)) = net::decode_frame(bytes, state) else {
            (self.resync, self.broken) = (true, self.broken + 1);
            return None;
        };
        if state.checksum() != header.checksum {
            (self.resync, self.broken) = (true, self.broken + 1);
            return None;
        }
        if base == 0 {
            (self.resync, self.whole) = (false, self.whole + 1);
        }
        let mut frame = Frame::from_state(&state, header.elapsed);
        (frame.tick, frame.ack, frame.mine) = (tick, header.ack, header.mine);
        (frame.shots, frame.splats, frame.knocks) = (events.shots, events.splats, events.knocks);
        self.states.push_back((tick, state));
        while self.states.len() > KEPT {
            self.states.pop_front();
        }
        self.newest = tick;
        self.frames += 1;
        Some(frame)
    }
    /// The batches that send `commands` (new since the last), each repeating the few sent before it, with the newest
    /// frame decoded; with no command, the ack alone when there is a new one (or a whole frame to ask for).
    pub fn batches(&mut self, commands: Vec<Command>, now: f64) -> Vec<Batch> {
        let resync = self.resync && now - self.asked > 0.25;
        if resync {
            self.asked = now;
        }
        let time = ((now - self.since) * 1000.).max(0.) as u32;
        let mut out = vec![];
        for chunk in commands.chunks(net::MAX_COMMANDS - AGAIN) {
            let again = self.recent.len().min(AGAIN);
            let mut list: Vec<Command> = self.recent.iter().skip(self.recent.len() - again).copied().collect();
            list.extend_from_slice(chunk);
            self.recent.extend(chunk.iter().copied());
            while self.recent.len() > AGAIN {
                self.recent.pop_front();
            }
            out.push(Batch { ack: self.newest, resync: resync && out.is_empty(), time, commands: list });
        }
        if out.is_empty() && (self.newest != self.told || resync) {
            out.push(Batch { ack: self.newest, resync, time, commands: vec![] });
        }
        if !out.is_empty() {
            self.told = self.newest;
        }
        out
    }
}
/// Whether the client waits for a replay's file: connected to a replay (`runtime::REPLAY`) whose stream has not been
/// handed over yet.
pub fn waiting_for_replay(connect: &str, stream: bool) -> bool {
    connect == crate::runtime::REPLAY && !stream
}
/// Before the arena's tick: whatever the server has said since the last frame.
pub fn receive(world: &mut World) {
    let options = world.resource::<crate::runtime::Options>().clone();
    if options.connect.is_empty() {
        return;
    }
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    world.resource_scope(|world, mut net: Mut<Network>| {
        // A replay's stream is handed over once its file has come (in a browser some time after the first frame): until
        // then there is nothing to read, and no server to open a socket to (`replay:` is no address).
        if waiting_for_replay(&options.connect, net.replay.is_some()) {
            return;
        }
        if !net.started {
            // A client without a page says here what it wants to be called and painted; the server checks it.
            let mut join = serde_json::json!({"type": "join", "version": net::PROTOCOL});
            for (key, value) in [("name", &options.name), ("color", &options.color), ("team", &options.team)] {
                if !value.is_empty() {
                    join[key] = serde_json::Value::String(value.clone());
                }
            }
            // The dev sandbox the address asks for (a --dev server sets it up for the first to join; the frames say
            // what the game is played by).
            if let Some(asked) = options.sandbox() {
                join["sandbox"] = asked;
            }
            net.start(&options.connect, join.to_string(), now);
        }
        let messages = match net.replay.as_mut() {
            Some(stream) => stream.drain(..).collect(),
            None => net.read(),
        };
        for message in messages {
            let frame = match message {
                Incoming::Binary(bytes) => net.decode(&bytes),
                Incoming::Text(raw) => {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
                        continue;
                    };
                    match v["type"].as_str().unwrap_or("") {
                        "welcome" => {
                            {
                                let mut g = world.resource_mut::<crate::arena::Game>();
                                g.network = true;
                                g.network_id = v["id"].as_u64().unwrap_or(0) as u32;
                                g.online.hz = v["tick"].as_f64().unwrap_or(net::TICK_HZ as f64) as f32;
                                g.online.send_hz = v["send"].as_f64().unwrap_or(net::SEND_HZ as f64) as f32;
                                net.status = "Connected".into();
                                info!("Multiplayer connected as hotdog {} ({} steps and {} frames a second)", g.network_id, g.online.hz, g.online.send_hz);
                            }
                            net.forget();
                            // The server says which map it is running; the arena is rebuilt from it.
                            match kitchen_sim::map::Map::parse(v["map"].as_str().unwrap_or("")) {
                                Ok(map) => {
                                    info!("Server map: {}", map.name);
                                    crate::level::notice("");
                                    crate::arena::load_map(world, map);
                                }
                                Err(error) => {
                                    net.status = format!("The server's map could not be read: {error}");
                                    warn!("{}", net.status);
                                    crate::level::notice(&net.status);
                                }
                            }
                            None
                        }
                        // The lobby's leader chose another map and the round starting now is played on it: the server
                        // has built a new simulation and says which hotdog this player is there. The arena is built
                        // again, nothing of the last round is kept, and the next frame is whole.
                        "map" => {
                            match kitchen_sim::map::Map::parse(v["map"].as_str().unwrap_or("")) {
                                Ok(map) => {
                                    info!("The next round is on {}", map.name);
                                    crate::level::notice("");
                                    net.round = 0;
                                    net.forget();
                                    for (_, (entity, _)) in net.remotes.drain() {
                                        world.entity_mut(entity).despawn();
                                    }
                                    crate::arena::switch_map(world, map, v["id"].as_u64().unwrap_or(0) as u32);
                                }
                                Err(error) => {
                                    net.status = format!("The server's new map could not be read: {error}");
                                    warn!("{}", net.status);
                                    crate::level::notice(&net.status);
                                }
                            }
                            None
                        }
                        // The tuning (kitchen_sim::tune) the server plays by, and whether the sandbox's panel may
                        // change it.
                        "tuning" => {
                            let mut g = world.resource_mut::<crate::arena::Game>();
                            g.tuning = kitchen_sim::tune::Tuning::parse(v["text"].as_str().unwrap_or(""));
                            g.tune_editable = v["editable"].as_bool().unwrap_or(false);
                            None
                        }
                        // Fully Loaded (cards.rs): the card table as it stands, on joining and whenever it changes. Whether
                        // there is a table to show it at is for the frames' rules to say.
                        "cards" => {
                            if let (Ok(view), Some(mut seen)) = (serde_json::from_value::<kitchen_sim::cards::View>(v["table"].clone()), world.get_resource_mut::<crate::cards::Seen>()) {
                                seen.hear(view);
                            }
                            None
                        }
                        // Kung Fu: the fight as the server has it (kitchen_sim::kungfu::View): on joining and whenever it
                        // changes. The HUD, the animation and the sounds read it from `Game::kungfu`.
                        "kungfu" => {
                            crate::fight::hear(&mut world.resource_mut::<crate::arena::Game>(), &v);
                            None
                        }
                        // Titan: the titan as the server has it (kitchen_sim::titan::View), read from `Game::titan` (titan/).
                        "titan" => {
                            crate::titan::hear(&mut world.resource_mut::<crate::arena::Game>(), &v);
                            None
                        }
                        // Every hotdog's reserve and the reload under way (reload.rs, kitchen_sim::magazine), which the
                        // frames do not carry: on joining and whenever any of it changes.
                        "ammo" => {
                            crate::reload::hear(&mut world.resource_mut::<crate::arena::Game>(), &v);
                            None
                        }
                        // The electric griddles' states (griddle.rs): on joining, whenever one is flipped, and every second.
                        "griddle" => {
                            crate::griddle::hear(world, &v);
                            None
                        }
                        "error" => {
                            net.status = v["message"].as_str().unwrap_or("Disconnected").into();
                            warn!("{}", net.status);
                            None
                        }
                        // The debug encoding: a whole frame as JSON.
                        "snapshot" => serde_json::from_value::<Frame>(v).ok(),
                        _ => None,
                    }
                }
            };
            if let Some(frame) = frame {
                take(world, &mut net, &frame, &options);
            }
        }
    });
}
/// A frame decoded: the lobby and the arena take it, and which other hotdogs there are to draw is brought up to date.
fn take(world: &mut World, net: &mut Network, frame: &Frame, options: &crate::runtime::Options) {
    crate::lobby::observe(world, frame);
    crate::arena::apply_frame(world, frame);
    // In the lobby nobody is on the counter: the players are drawn in the fridge (lobby.rs).
    let holding = world.resource::<crate::lobby::Lobby>().holding;
    let id = world.resource::<crate::arena::Game>().network_id;
    let round = frame.round as u64;
    if round != net.round || holding {
        net.round = round;
        for (_, (entity, _)) in net.remotes.drain() {
            world.entity_mut(entity).despawn();
        }
    }
    if holding {
        return;
    }
    let present: Vec<u32> = frame.players.iter().map(|p| p.player.id).collect();
    let gone: Vec<u32> = net.remotes.keys().filter(|id| !present.contains(id)).copied().collect();
    for id in gone {
        if let Some((entity, _)) = net.remotes.remove(&id) {
            world.entity_mut(entity).despawn();
        }
    }
    net.looks.clear();
    for seat in &frame.players {
        let p = &seat.player;
        if p.id == id && !options.demo {
            continue;
        }
        net.looks.insert(p.id, (seat.color.clone(), if seat.human { seat.talk } else { 0. }, p.consumed));
        if net.remotes.get(&p.id).is_some_and(|entry| entry.1 != p.infected) {
            let (entity, _) = net.remotes.remove(&p.id).unwrap();
            world.entity_mut(entity).despawn();
        }
        if !net.remotes.contains_key(&p.id) {
            let entity = crate::arena::remote_hotdog(world, Vec3::from_array(p.p), p.infected, p.id);
            net.remotes.insert(p.id, (entity, p.infected));
        }
    }
}
/// After the arena's tick: the other hotdogs where the timeline draws them, the shots it passed, the ready wishes, and
/// this frame's commands and ack to the server.
pub fn send(world: &mut World) {
    let options = world.resource::<crate::runtime::Options>().clone();
    if options.connect.is_empty() {
        return;
    }
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    world.resource_scope(|world, mut net: Mut<Network>| {
        // The other hotdogs, each frame where the timeline has them (arena/online.rs put them in the roster).
        let (roster, stage_over, table, reloads, kungfu, titan) = {
            let g = world.resource::<crate::arena::Game>();
            let reloads: std::collections::HashMap<u32, f32> = g.roster.iter().map(|p| (p.id, crate::reload::progress(g, p.id))).collect();
            (g.roster.clone(), matches!(g.phase_name(), "RoundOver" | "Rest"), crate::cards::at_table(g), reloads, g.kung_fu(), crate::titan::titan_id(g))
        };
        for p in &roster {
            let Some(&(entity, _)) = net.remotes.get(&p.id) else {
                continue;
            };
            let (color, talk, consumed) = net.looks.get(&p.id).cloned().unwrap_or_default();
            // (The titan's hotdog is drawn as the titan: titan/.)
            let shown = if consumed || titan == Some(p.id) { Visibility::Hidden } else { Visibility::Visible };
            if world.get::<Visibility>(entity) != Some(&shown) {
                world.entity_mut(entity).insert(shown);
            }
            let cues = crate::player_model::Cues {
                alive: p.alive,
                cheer: p.alive && stage_over,
                prone: p.prone,
                crouch: p.crouch,
                stim: p.stim,
                weapon: p.weapon,
                carry: p.carry >= 0,
                pitch: p.pitch,
                swings: p.swings,
                aimed: p.aimed,
                airborne: !p.grounded,
                launched: p.launched,
                velocity: Vec3::from_array(p.v),
                reload: reloads.get(&p.id).copied().unwrap_or(0.),
                kungfu,
            };
            // At the card table a hotdog holds no gun, turns to whoever's go it is and cheers only its own luck (cards.rs).
            let (yaw, cues) = crate::cards::seated(world, table, p.id, p.yaw, cues);
            let point = Vec3::from_array(p.p);
            if let Some(mut tr) = world.get_mut::<Transform>(entity) {
                tr.translation = point + if p.alive { Vec3::ZERO } else { Vec3::Y * 0.025 };
                tr.rotation = Quat::from_rotation_y(yaw) * if p.alive { Quat::IDENTITY } else { Quat::from_rotation_z(1.57) };
            }
            crate::player_model::animate(world, entity, now as f32, cues);
            // Its ring in the colour its player chose (a team's in Forts stays), and its mouth moving while that player speaks.
            crate::player_model::paint(world, entity, &color);
            crate::player_model::speak(world, entity, talk, now as f32);
        }
        // The shots the timeline passed: guns kick the hotdog that fired; only beams (the turret) leave a trail,
        // because rounds arrive as projectiles.
        let (shots, me) = {
            let mut g = world.resource_mut::<crate::arena::Game>();
            (std::mem::take(&mut g.online.shots), g.network_id)
        };
        for shot in shots {
            if let Some((entity, _)) = net.remotes.get(&shot.owner) {
                crate::player_model::kick(world, *entity, shot.weapon);
            }
            if shot.owner != me {
                crate::arena::remote_shot(world, Vec3::from_array(shot.a), Vec3::from_array(shot.b), shot.projectile == 0, shot.weapon);
            }
        }
        // Enter in the lobby (see lobby.rs): ready, or not.
        if let Some(ready) = world.resource_mut::<crate::lobby::Lobby>().take_wish() {
            net.send(Outgoing::Text(serde_json::json!({"type": "ready", "ready": ready}).to_string()));
        }
        let (network, ready, commands) = {
            let mut g = world.resource_mut::<crate::arena::Game>();
            (g.network, g.pending_ready, std::mem::take(&mut g.online.unsent))
        };
        // The sandbox's tuning panel's changes (tune.rs).
        for text in std::mem::take(&mut net.tunes) {
            net.send(Outgoing::Text(serde_json::json!({"type": "tune", "text": text}).to_string()));
        }
        // Fully Loaded (cards.rs): this player's flips, stops and picks at the card table.
        let wishes = world.get_resource_mut::<crate::cards::Wishes>().map(|mut wishes| std::mem::take(&mut wishes.0)).unwrap_or_default();
        for command in wishes {
            net.send(Outgoing::Text(crate::cards::wish(command).to_string()));
        }
        // E at a griddle's dial (griddle.rs): the server flips it if this player's hotdog is in reach of the dial, facing it.
        for _ in 0..crate::griddle::take_presses(world) {
            net.send(Outgoing::Text(serde_json::json!({"type": "griddle"}).to_string()));
        }
        // R in a Forts build phase: ready for the forcefield to drop, or not.
        if let Some(ready) = ready {
            net.send(Outgoing::Text(serde_json::json!({"type": "ready", "ready": ready}).to_string()));
        }
        if network {
            for batch in net.batches(commands, now) {
                net.send(Outgoing::Binary(batch.encode()));
            }
            // This player's own view for the match's recording (replay/pov.rs): a text of its own, which a server that
            // does not know it passes over. Only once the server has welcomed this client (before, a text is its join).
            let views = world.get_resource_mut::<crate::replay::pov::Pov>().map(|mut p| std::mem::take(&mut p.outbox)).unwrap_or_default();
            if net.newest > 0 && net.replay.is_none() {
                for text in views {
                    net.send(Outgoing::Text(text));
                }
            }
        }
    });
}
/// The native client's connection, on a thread of its own: the server's game socket over WebSocket (tungstenite), the
/// join first, then whatever the game sends (`outgoing`), and everything the server says passed back (`incoming`).
/// `address` is `--connect`'s HOST:PORT (the socket is ws://HOST:PORT/ws) or a ws:// or wss:// address. When the connection
/// ends, an error message says why.
#[cfg(not(target_arch = "wasm32"))]
fn connect(address: String, join: String, outgoing: std::sync::mpsc::Receiver<Outgoing>, incoming: std::sync::mpsc::Sender<Incoming>) {
    use std::time::Duration;
    use tungstenite::{Error, Message, stream::MaybeTlsStream};
    let url = if address.contains("://") { address } else { format!("ws://{address}/ws") };
    let result = (|| -> Result<(), Error> {
        // (A wss:// address is TLS: rustls, told once which cryptography it uses.)
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (mut socket, _) = tungstenite::connect(url.as_str())?;
        let stream = match socket.get_mut() {
            MaybeTlsStream::Plain(stream) => Some(stream),
            MaybeTlsStream::Rustls(tls) => Some(tls.get_mut()),
            _ => None,
        };
        if let Some(stream) = stream {
            stream.set_nodelay(true)?;
            // Reads wait a moment at most, so that what the game sends goes out promptly.
            stream.set_read_timeout(Some(Duration::from_millis(2)))?;
        }
        socket.send(Message::text(join))?;
        loop {
            while let Ok(message) = outgoing.try_recv() {
                socket.send(match message {
                    Outgoing::Text(s) => Message::text(s),
                    Outgoing::Binary(b) => Message::binary(b),
                })?;
            }
            let message = match socket.read() {
                Ok(Message::Text(text)) => Incoming::Text(text.as_str().to_string()),
                Ok(Message::Binary(bytes)) => Incoming::Binary(bytes.to_vec()),
                Ok(Message::Close(_)) => break,
                Ok(_) => continue,
                Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                Err(Error::ConnectionClosed | Error::AlreadyClosed) => break,
                Err(e) => return Err(e),
            };
            if incoming.send(message).is_err() {
                return Ok(());
            }
        }
        Ok(())
    })();
    let message = match result {
        Ok(()) => "Disconnected".into(),
        Err(e) => format!("Connection failed: {e}"),
    };
    let _ = incoming.send(Incoming::Text(serde_json::json!({"type": "error", "message": message}).to_string()));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use tungstenite::Message;
    fn text(message: Incoming) -> String {
        match message {
            Incoming::Text(s) => s,
            Incoming::Binary(_) => panic!("a binary message"),
        }
    }
    /// A replay's viewer opens no socket while its file is on its way (in a browser `replay:` is no WebSocket address:
    /// the page logged an error for it), and reads the stream once it is there; a server's address is never held up.
    #[test]
    fn a_replay_waits_for_its_file_and_never_opens_a_socket() {
        assert!(waiting_for_replay(crate::runtime::REPLAY, false));
        assert!(!waiting_for_replay(crate::runtime::REPLAY, true));
        assert!(!waiting_for_replay("ws://127.0.0.1:8137/ws", false));
        assert!(!waiting_for_replay("", false));
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.insert_resource(crate::runtime::Options { connect: crate::runtime::REPLAY.into(), ..Default::default() });
        app.init_resource::<Network>().init_resource::<Time<Real>>();
        app.world_mut().run_system_once(receive).unwrap();
        let net = app.world().resource::<Network>();
        assert!(!net.started && net.outgoing.is_none(), "no socket before the file");
    }
    /// The native client joins over the server's WebSocket at /ws, hears what it is sent (text and frames), and sends
    /// what the game gives it (text and commands); when the server closes the socket, it says so.
    #[test]
    fn the_native_client_joins_over_the_servers_websocket_and_trades_messages() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut path = String::new();
            let mut socket = tungstenite::accept_hdr(stream, |request: &tungstenite::handshake::server::Request, response| {
                path = request.uri().path().to_string();
                Ok(response)
            })
            .unwrap();
            let join = socket.read().unwrap().into_text().unwrap().to_string();
            socket.send(Message::text(r#"{"type":"welcome","id":3,"version":12,"map":"{}"}"#)).unwrap();
            socket.send(Message::binary(vec![2u8, 7, 0])).unwrap();
            let commands = socket.read().unwrap().into_data().to_vec();
            socket.close(None).unwrap();
            while socket.read().is_ok() {}
            (path, join, commands)
        });
        let (to_server, outgoing) = mpsc::channel();
        let (incoming, from_server) = mpsc::channel();
        let client = std::thread::spawn(move || connect(format!("127.0.0.1:{port}"), r#"{"type":"join","version":12,"name":"Nell"}"#.into(), outgoing, incoming));
        let welcome = text(from_server.recv_timeout(Duration::from_secs(5)).unwrap());
        assert!(welcome.contains(r#""type":"welcome""#), "{welcome}");
        assert!(matches!(from_server.recv_timeout(Duration::from_secs(5)).unwrap(), Incoming::Binary(b) if b == [2, 7, 0]));
        let batch = Batch { ack: 7, resync: false, time: 5, commands: vec![Command::new(1, Vec2::new(0., -1.), 0.5, 0., net::button::JUMP, 1, None)] };
        to_server.send(Outgoing::Binary(batch.encode())).unwrap();
        let (path, join, commands) = server.join().unwrap();
        assert_eq!((path.as_str(), join.as_str()), ("/ws", r#"{"type":"join","version":12,"name":"Nell"}"#));
        assert_eq!(Batch::decode(&commands).unwrap(), batch);
        let farewell = text(from_server.recv_timeout(Duration::from_secs(5)).unwrap());
        assert!(farewell.contains("Disconnected"), "{farewell}");
        client.join().unwrap();
        // Nobody there: the reason is passed on.
        let (_, outgoing) = mpsc::channel();
        let (incoming, from_server) = mpsc::channel();
        connect(format!("127.0.0.1:{port}"), "{}".into(), outgoing, incoming);
        assert!(text(from_server.recv().unwrap()).contains("Connection failed"));
    }
    /// The server pings every socket now and then (crates/server, transport.rs: a WebSocket control frame, below the
    /// messages, so that a proxy's idle timeout does not cut a quiet lobby, and a socket that answers none is let go):
    /// the native client's library answers by itself, with the ping's own payload, and the ping reaches the game as
    /// nothing at all, so no recording can hold it.
    #[test]
    fn the_native_client_answers_the_servers_pings_and_the_game_never_hears_of_them() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            // (A client that does not answer fails the test in five seconds, not never.)
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            socket.read().unwrap();
            socket.send(Message::Ping(b"are you there".to_vec().into())).unwrap();
            socket.send(Message::text(r#"{"type":"welcome","id":3,"version":12,"map":"{}"}"#)).unwrap();
            socket.send(Message::Ping(Default::default())).unwrap();
            socket.send(Message::binary(vec![2u8, 7, 0])).unwrap();
            let mut pongs = vec![];
            while pongs.len() < 2 {
                if let Message::Pong(payload) = socket.read().unwrap() {
                    pongs.push(payload.to_vec());
                }
            }
            socket.close(None).unwrap();
            while socket.read().is_ok() {}
            pongs
        });
        let (_to_server, outgoing) = mpsc::channel();
        let (incoming, from_server) = mpsc::channel();
        let client = std::thread::spawn(move || connect(format!("127.0.0.1:{port}"), r#"{"type":"join","version":12}"#.into(), outgoing, incoming));
        let heard: Vec<Incoming> = from_server.iter().collect();
        client.join().unwrap();
        assert_eq!(server.join().unwrap(), [b"are you there".to_vec(), vec![]], "each ping is answered with its payload");
        // All the game hears: the welcome, the frame, and the farewell when the server closes. No ping, no pong.
        assert_eq!(heard.len(), 3);
        assert!(matches!(&heard[0], Incoming::Text(t) if t.contains(r#""type":"welcome""#)));
        assert!(matches!(&heard[1], Incoming::Binary(b) if *b == [2, 7, 0]));
        assert!(matches!(&heard[2], Incoming::Text(t) if t.contains("Disconnected")));
    }
    /// Frames are decoded against the state the header names; one against a state this client has not got, or whose
    /// checksum does not match, is dropped and a whole frame asked for; each batch acknowledges the newest frame and
    /// repeats the two commands before its new ones.
    #[test]
    fn frames_decode_against_what_was_acknowledged_and_a_broken_one_asks_for_a_whole_frame() {
        let mut feast = kitchen_sim::feast::Feast::new(true);
        let mut net = Network::default();
        let state = |feast: &kitchen_sim::feast::Feast| {
            let frame = Frame { players: feast.players.iter().map(|p| net::Seat { player: p.clone(), ..Default::default() }).collect(), enemies: feast.enemies.clone(), elapsed: feast.elapsed, ..Default::default() };
            frame.state()
        };
        let header = |tick: u32, base: u32, s: &State| net::Header { tick, base, ack: tick / 2, checksum: s.checksum(), elapsed: 1., mine: None };
        let s1 = state(&feast);
        let whole = net::encode_frame(&header(10, 0, &s1), None, &s1, &net::Events::default());
        let f = net.decode(&whole).unwrap();
        assert_eq!((f.tick, f.ack, net.newest, net.whole), (10, 5, 10, 1));
        feast.tick(0.5);
        let s2 = state(&feast);
        let delta = net::encode_frame(&header(12, 10, &s2), Some(&s1), &s2, &net::Events::default());
        assert_eq!(net.decode(&delta).unwrap().players.len(), 5);
        assert!(net.decode(&delta).is_none(), "an old frame again is passed over");
        // Against a frame it never had: dropped, and a whole frame asked for.
        let lost = net::encode_frame(&header(14, 11, &s2), Some(&s1), &s2, &net::Events::default());
        assert!(net.decode(&lost).is_none() && net.resync);
        let commands: Vec<Command> = (1..=4).map(|k| Command::new(k, Vec2::ZERO, 0., 0., 0, 1, None)).collect();
        let batches = net.batches(commands[..3].to_vec(), 1.);
        assert_eq!((batches.len(), batches[0].ack, batches[0].resync, batches[0].commands.len()), (1, 12, true, 3));
        let batches = net.batches(commands[3..].to_vec(), 1.1);
        assert_eq!(batches[0].commands.iter().map(|c| c.seq).collect::<Vec<_>>(), [2, 3, 4], "the two before go again");
        assert!(!batches[0].resync, "asked again only after a quarter of a second");
        assert!(net.batches(vec![], 1.2).is_empty(), "nothing new: nothing sent");
        // A checksum that does not match: dropped too.
        let mut bad = net::encode_frame(&header(16, 12, &s2), Some(&s2), &s2, &net::Events::default());
        let at = bad.len() - 7 - 4;
        bad[at] ^= 1;
        let _ = net.decode(&bad);
        assert_eq!(net.newest, 12);
    }

    /// The game as this module meets it online: the arena (arena.rs `test_app`) with the card table's resources and
    /// the lobby, playing hotdog 2, and a connection whose far ends the test holds: what the server says, and what the
    /// game sends it.
    pub(crate) fn connected() -> (App, mpsc::Sender<Incoming>, mpsc::Receiver<Outgoing>) {
        let mut app = crate::arena::test_app("");
        app.add_plugins(crate::cards::CardsPlugin);
        app.world_mut().resource_mut::<crate::runtime::Options>().connect = "127.0.0.1:1".into();
        app.world_mut().insert_resource(crate::lobby::Lobby::default());
        let (server, incoming) = mpsc::channel();
        let (outgoing, sent) = mpsc::channel();
        app.world_mut().insert_resource(Network { started: true, incoming: Some(std::sync::Mutex::new(incoming)), outgoing: Some(outgoing), ..Default::default() });
        {
            let mut g = app.world_mut().resource_mut::<crate::arena::Game>();
            (g.network, g.network_id) = (true, 2);
        }
        (app, server, sent)
    }
    /// A frame of a round played by `rules` (the debug encoding's whole frame as JSON; the binary one says the same).
    fn snapshot(round: u32, rules: &str, tick: u32) -> Incoming {
        let mut feast = kitchen_sim::feast::Feast::with_map(kitchen_sim::map::Map::bundled(), false);
        feast.set_rules(kitchen_sim::rules::Rules::parse(rules));
        let players = feast.players.iter().map(|p| net::Seat { player: p.clone(), human: p.id == 2, ..Default::default() }).collect();
        let frame = Frame { kind: "snapshot".into(), tick, round, rules: rules.into(), stage: "Warmup".into(), players, ..Default::default() };
        Incoming::Text(serde_json::to_string(&frame).unwrap())
    }
    pub(crate) fn told(view: &kitchen_sim::cards::View) -> Incoming {
        Incoming::Text(serde_json::json!({"type": "cards", "table": view}).to_string())
    }
    /// What the server has said so far, then a frame of the game.
    pub(crate) fn hear(app: &mut App, server: &mpsc::Sender<Incoming>, said: Vec<Incoming>) {
        for message in said {
            server.send(message).unwrap();
        }
        receive(app.world_mut());
        app.update();
    }
    fn seen(app: &App) -> (Option<kitchen_sim::cards::View>, Option<usize>) {
        let seen = app.world().resource::<crate::cards::Seen>();
        (seen.view.clone(), seen.me)
    }

    /// The server's `{type:'cards',table}` is what the table looks like, while the frames' rules say the game is Fully
    /// Loaded: one heard before the frame that says so (on joining, or a new game's on an arena built afresh) waits for
    /// it; the newest replaces it; one that cannot be read is passed over; and once the rules say otherwise there is no
    /// table, whatever is heard.
    #[test]
    fn a_cards_message_fills_seen() {
        let (mut app, server, _) = connected();
        let mut table = kitchen_sim::cards::Table::new(5, 7);
        table.tick(5.);
        let first = table.view();
        hear(&mut app, &server, vec![told(&first)]);
        assert_eq!(seen(&app), (None, None), "no frame has said there is a table yet");
        hear(&mut app, &server, vec![snapshot(1, "mode=cards", 1)]);
        assert_eq!(seen(&app), (Some(first.clone()), Some(1)), "hotdog 2 sits at the second seat");
        // The table moves on (the hotdog whose go it is stops): the newest is seen. Then something that is no table
        // changes nothing.
        table.command(1, kitchen_sim::cards::Command::Stop).unwrap();
        let later = table.view();
        assert!(later.version > first.version);
        hear(&mut app, &server, vec![told(&later), snapshot(1, "mode=cards", 2)]);
        assert_eq!(seen(&app).0, Some(later.clone()));
        hear(&mut app, &server, vec![Incoming::Text(r#"{"type":"cards","table":{"seats":3}}"#.into()), Incoming::Text(r#"{"type":"cards"}"#.into())]);
        assert_eq!(seen(&app).0, Some(later.clone()));
        // The next game, on an arena built afresh (a `map` message rebuilds it, rules and all, before the new table and
        // the frame that says what is played): the new table is shown once that frame has come.
        app.world_mut().resource_mut::<crate::arena::Game>().feast = kitchen_sim::feast::Feast::with_map(kitchen_sim::map::Map::bundled(), false);
        let mut again = kitchen_sim::cards::Table::new(5, 8);
        again.tick(5.);
        hear(&mut app, &server, vec![told(&again.view())]);
        assert_eq!(seen(&app), (None, None));
        hear(&mut app, &server, vec![snapshot(2, "mode=cards", 3)]);
        assert_eq!(seen(&app).0, Some(again.view()));
        // A siege: no table, and none for a table heard during it.
        hear(&mut app, &server, vec![snapshot(3, "", 4)]);
        assert_eq!(seen(&app), (None, None));
        hear(&mut app, &server, vec![told(&later)]);
        assert_eq!(seen(&app), (None, None));
    }

    /// Online at the card table the other hotdogs hold no gun (cards.rs `seated`), and back in a siege they hold
    /// theirs again.
    #[test]
    fn at_the_card_table_the_others_hold_no_gun() {
        let (mut app, server, _) = connected();
        app.add_systems(Update, crate::arena::tick);
        let mut table = kitchen_sim::cards::Table::new(5, 7);
        table.tick(5.);
        let weapons = |app: &mut App| {
            let world = app.world_mut();
            world.query::<&crate::player_model::Gait>().iter(world).map(|gait| gait.motion().weapon).collect::<Vec<_>>()
        };
        let frame = |app: &mut App, said: Vec<Incoming>| {
            app.world_mut().resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::from_millis(50));
            hear(app, &server, said);
            send(app.world_mut());
        };
        frame(&mut app, vec![told(&table.view()), snapshot(1, "mode=cards", 1)]);
        for tick in 2..8 {
            frame(&mut app, vec![snapshot(1, "mode=cards", tick)]);
        }
        assert!(crate::cards::at_table(app.world().resource::<crate::arena::Game>()));
        let held = weapons(&mut app);
        assert!(held.len() == 4 && held.iter().all(|w| *w == 0), "{held:?}");
        for tick in 8..14 {
            frame(&mut app, vec![snapshot(2, "", tick)]);
        }
        let held = weapons(&mut app);
        assert!(held.len() == 4 && held.iter().all(|w| *w > 0), "{held:?}");
    }

    /// The server's `{type:'ammo'}` is where this player's reserve comes from (a frame carries only the magazine): the
    /// display shows both; R shows the reload at once and asks for it in the next command (`button::RELOAD`); another
    /// hotdog's reload, heard, is what its third-person reload is posed from.
    #[test]
    fn the_servers_reserves_reach_the_display_and_r_goes_out_as_a_reload_in_the_next_command() {
        use kitchen_sim::feast::RIFLE;
        let (mut app, server, _sent) = connected();
        app.add_systems(Update, crate::arena::tick);
        let mut feast = kitchen_sim::feast::Feast::with_map(kitchen_sim::map::Map::bundled(), false);
        feast.players[1].ammo[RIFLE] = 20;
        let players = feast.players.iter().map(|p| net::Seat { player: p.clone(), human: p.id == 2, ..Default::default() }).collect();
        let frame = Frame { kind: "snapshot".into(), tick: 1, round: 1, stage: "Warmup".into(), elapsed: 2., players, ..Default::default() };
        let ammo = serde_json::json!({"type": "ammo", "hotdogs": [{"id": 2, "reserve": [0, 48, 0, 0, 0, 0, 100, 0], "gun": 0, "at": 0, "of": 0}, {"id": 3, "reserve": [0, 0, 0, 0, 0, 0, 0, 0], "gun": 6, "at": 1.5, "of": 1.8}]});
        hear(&mut app, &server, vec![Incoming::Text(serde_json::to_string(&frame).unwrap()), Incoming::Text(ammo.to_string())]);
        let g = app.world().resource::<crate::arena::Game>();
        assert_eq!(g.mag.reserve, [0, 48, 0, 0, 0, 0, 100, 0]);
        let arms = crate::hud::Show::of(g, &crate::hud::Context::default()).arms.unwrap();
        assert_eq!(arms.rounds, Some(crate::hud::Rounds::Count { now: 20, full: 60, reserve: 100 }));
        let other = crate::reload::progress(g, 3);
        assert!(other > 0.2 && other < 0.4, "hotdog 3 is a quarter through its reload: {other}");
        // R: shown at once, and sent.
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyR);
        crate::arena::test_frame(&mut app, 0.05);
        let g = app.world().resource::<crate::arena::Game>();
        assert!(g.mag.reload.is_some_and(|r| r.gun == RIFLE), "foreseen");
        assert_eq!(g.online.unsent.iter().filter(|c| c.reloads()).count(), 1, "the next command asks for it, once");
        assert!(g.online.unsent.iter().all(|c| c.pose() == kitchen_sim::feast::build::Pose::default()), "and is no pose");
    }

    /// This player's wishes go to the server as `{type:'cards',action,seat?}`, the seat only with a target, once each.
    #[test]
    fn wishes_go_out_as_json() {
        use kitchen_sim::cards::Command::{Flip, Stop, Target};
        let (mut app, _server, sent) = connected();
        app.world_mut().resource_mut::<crate::cards::Wishes>().0.extend([Flip, Stop, Target(3)]);
        send(app.world_mut());
        let texts: Vec<serde_json::Value> = sent
            .try_iter()
            .map(|message| match message {
                Outgoing::Text(s) => serde_json::from_str(&s).unwrap(),
                Outgoing::Binary(_) => panic!("a binary message"),
            })
            .collect();
        assert_eq!(
            texts,
            [
                serde_json::json!({"type": "cards", "action": "flip"}),
                serde_json::json!({"type": "cards", "action": "stop"}),
                serde_json::json!({"type": "cards", "action": "target", "seat": 3}),
            ]
        );
        assert!(app.world().resource::<crate::cards::Wishes>().0.is_empty());
        send(app.world_mut());
        assert!(sent.try_iter().next().is_none(), "each goes once");
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The game loop: one task owns the lobby, its game and every socket's place in them, so nothing is shared and
//! nothing is locked. Sockets (transport.rs) hand it what they receive as events; it answers through each socket's
//! outbox.
//!
//! **Time.** The loop steps the game at a fixed rate (`Net::tick`, 60 a second) by a monotonic clock: each step is
//! due at a moment of its own, and a loop that wakes late takes every step it owes at once, so a slow step never slows
//! the game. It owes at most CATCH_UP steps; further behind than that (a machine stalled for seconds), it lets the
//! time go and says so. Steps are numbered from the server's start (`tick`); every `Net::every()`th step it sends each
//! game socket a frame, and ticks the lobby.
//!
//! **Frames.** Each game socket is sent its own frame (kitchen_sim::net): the shared state of the game, encoded against
//! the last frame that client acknowledged (`Stream`), with the last of its commands applied and its own body; a
//! client that has acknowledged nothing yet, has fallen too far behind or asked for it is sent a whole one, and so is
//! every client after a new map. The state is made once a frame for everyone; only the encoding is per socket. A
//! socket that asked for JSON is sent the frame whole, as JSON.
//!
//! **The card table.** Fully Loaded (kitchen_sim::cards) travels as JSON text beside the frames, which are as they
//! always were: every game socket is sent `{type:'cards',table:View}` when it sits down while there is a table and
//! whenever the table's `version` has moved since the last it was sent (checked every frame step; the clock running
//! does not move it). A game socket sends `{type:'cards',action:'flip'|'stop'|'target',seat?}`: the seat that acts is
//! always its own hotdog's place among the players (never one it names; `seat` is only the hotdog `target` picks), and
//! the game refuses whatever is not that seat's go (game.rs `cards`). A message that is none of these is passed over.
//!
//! **The griddle.** A map with an electric griddle (kitchen_sim::griddle) has its state travel as JSON text beside the frames too:
//! every game socket is sent `{type:'griddle',griddles:[State]}` when it sits down, whenever a griddle is flipped or a new round
//! begins, and every `griddle::SYNC` seconds (a client carries each temperature on between by the same law). A game socket sends
//! `{type:'griddle'}` for use pressed: the game flips the griddle in reach of that socket's own hotdog, if there is one.
//!
//! **Kung Fu.** The fight (kitchen_sim::kungfu `View`: every fighter's style, move, when it began, its block, stun, chi
//! and guard, and the events of the last half second) travels as JSON text beside the frames too: every game socket is
//! sent `{type:'kungfu', ...View}` when it sits down and at every frame step at which the view has changed since it was
//! last sent (a move running on does not change it: the client works out the time into it from `started`).
//!
//! **Reserves and reloads.** A frame carries each gun's magazine as it always has; each hotdog's reserve and the reload
//! under way (kitchen_sim::magazine `Stock`) travel as JSON text beside the frames: every game socket is sent
//! `{type:'ammo',hotdogs:[Stock]}` when it sits down and at every frame step at which any of it has changed (a reload
//! running on does not change it: `at` and `of` say when it began and how long it takes). Not in Kung Fu or at the card
//! table, which have no guns. A reload is asked for in a command (`button::RELOAD`).
//!
//! A socket is a game socket (the game, in a browser or native: join, then commands) or a page's lobby socket (hello,
//! then the lobby's messages: names, colours, ready, team, the leader's choice, chat, voice signalling).
use crate::config::Net;
use crate::lobby::{self, CAPACITY, Lobby};
use crate::outbox::Outbox;
use crate::protocol::{self, PROTOCOL, is, truthy};
use kitchen_sim::cards;
use kitchen_sim::griddle;
use kitchen_sim::kungfu;
use kitchen_sim::net::{self as wire, Batch, Events, Frame, Header, Mine, State};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

/// A socket that has not joined (or said hello) within HANDSHAKE is cut off.
pub const HANDSHAKE: Duration = Duration::from_secs(5);
/// The most steps the loop takes at once to catch up.
pub const CATCH_UP: u32 = 12;
/// How many frames sent to a socket are kept to encode against: a client whose newest ack is older than that (two
/// seconds of frames, at 30 a second) is sent a whole frame.
pub const KEPT: usize = 64;

/// What the sockets tell the game loop.
pub enum Event {
    /// A socket has opened: a page's lobby socket, or a game socket.
    Open { peer: u64, page: bool, outbox: Outbox },
    Message { peer: u64, message: Value },
    /// A binary message: from a game socket, a batch of commands.
    Binary { peer: u64, bytes: Vec<u8> },
    Closed { peer: u64 },
    /// The status JSON, for /status.
    Status { reply: oneshot::Sender<Value> },
}
/// What a game socket has been sent of the game: the states of the frames it was sent lately, by step, the newest
/// of them it has acknowledged, whether it asked for a whole frame, the newest shot, splat and knock it has been
/// told of and in which game and round they were numbered, and how many bytes and frames it has been sent.
#[derive(Default)]
struct Stream {
    json: bool,
    sent: VecDeque<(u32, Arc<State>)>,
    acked: u32,
    resync: bool,
    told: [u32; 3],
    era: (u64, u32),
    bytes: u64,
    frames: u64,
    whole: u64,
}
impl Stream {
    /// Everything a new map makes old.
    fn restart(&mut self) {
        (self.sent, self.acked, self.resync, self.told) = (VecDeque::new(), 0, false, [0; 3]);
    }
    /// The frame to encode against: the newest the client has acknowledged, while it is kept.
    fn base(&self) -> Option<&(u32, Arc<State>)> {
        if self.resync || self.acked == 0 {
            return None;
        }
        self.sent.iter().find(|(tick, _)| *tick == self.acked)
    }
    /// The client acknowledges the frame of step `tick`: one it was sent and that is still kept.
    fn ack(&mut self, tick: u32) {
        if tick > self.acked && self.sent.iter().any(|(t, _)| *t == tick) {
            self.acked = tick;
        }
    }
    /// After a frame of step `tick`: it is kept, and the frames older than the acknowledged one are dropped.
    fn keep(&mut self, tick: u32, state: Arc<State>) {
        self.sent.push_back((tick, state));
        while self.sent.len() > 1 && self.sent.front().is_some_and(|(t, _)| *t < self.acked) {
            self.sent.pop_front();
        }
        while self.sent.len() > KEPT {
            self.sent.pop_front();
        }
    }
}
struct Peer {
    page: bool,
    outbox: Outbox,
    /// The hotdog a game socket plays, and the member a socket belongs to.
    player: Option<u32>,
    member: Option<u32>,
    opened: Instant,
    stream: Stream,
}
/// What the loop has cost since it last said (`--stats`).
#[derive(Default)]
struct Stats {
    steps: u64,
    step: Duration,
    worst: Duration,
    sends: u64,
    send: Duration,
    behind: u64,
    since: Option<Instant>,
}
pub struct Hub {
    pub lobby: Lobby,
    peers: BTreeMap<u64, Peer>,
    pub net: Net,
    /// The steps taken since the server started (the step a frame is taken at), and the games built (a new map's
    /// numbers its shots afresh).
    pub tick: u32,
    game: u64,
    /// The version of the card table (kitchen_sim::cards) every game socket has been sent last; None before the first
    /// (and after a new game is built, whose table's versions count afresh).
    cards_sent: Option<u64>,
    /// What every game socket has been told of the griddles last: the round and how many times each was flipped in it, and the
    /// step it was told at; None before the first (and when the map has none).
    griddles_sent: Option<((u32, Vec<u32>), u32)>,
    /// The fight (kitchen_sim::kungfu) as every game socket was last told it, its clock left out; None before the first.
    kungfu_sent: Option<String>,
    /// The hotdogs' reserves and reloads (kitchen_sim::magazine) as every game socket was last told them; None before
    /// the first.
    ammo_sent: Option<String>,
    stats: Stats,
    /// Every match recorded, from a spectator's view (replay.rs): off unless the server gives it a folder.
    pub replays: crate::replay::Replays,
}
impl Hub {
    pub fn new(lobby: Lobby, net: Net) -> Hub {
        Hub { lobby, peers: BTreeMap::new(), net, tick: 0, game: 0, cards_sent: None, griddles_sent: None, kungfu_sent: None, ammo_sent: None, stats: Stats::default(), replays: crate::replay::Replays::off() }
    }
    /// The number of the game being played (a new map's game is a new one).
    pub fn game(&self) -> u64 {
        self.game
    }
    /// Runs the loop until every sender of events has gone.
    pub async fn run(mut self, mut events: mpsc::UnboundedReceiver<Event>) {
        let step = Duration::from_secs_f64(1. / self.net.tick as f64);
        let mut next = Instant::now() + step;
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Some(event) => self.event(event),
                    None => break,
                },
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(next)) => {
                    let (steps, due, lost) = owed(next, Instant::now(), step);
                    next = due;
                    let mut send = false;
                    for _ in 0..steps {
                        send |= self.advance();
                    }
                    if let Some(lost) = lost {
                        eprintln!("The game loop fell {} ms behind and let that time go: the game slowed down", lost.as_millis());
                        self.stats.behind += 1;
                    }
                    if send {
                        self.send();
                    }
                    self.report();
                }
            }
        }
    }
    pub fn event(&mut self, event: Event) {
        match event {
            Event::Open { peer, page, outbox } => {
                let stream = Stream { json: self.net.json, ..Default::default() };
                self.peers.insert(peer, Peer { page, outbox, player: None, member: None, opened: Instant::now(), stream });
            }
            Event::Message { peer, message } => self.receive(peer, &message),
            Event::Binary { peer, bytes } => self.binary(peer, &bytes),
            Event::Closed { peer } => self.closed(peer),
            Event::Status { reply } => {
                let _ = reply.send(self.status());
            }
        }
    }
    /// One message from a socket.
    fn receive(&mut self, id: u64, message: &Value) {
        let Some(peer) = self.peers.get(&id) else {
            return;
        };
        let outbox = peer.outbox.clone();
        if peer.page {
            match peer.member {
                Some(member) => self.lobby.handle(member, message),
                None if !lobby::hello(message) => outbox.destroy(),
                None => match self.lobby.enter(outbox.clone(), message) {
                    Some(member) => self.peers.get_mut(&id).unwrap().member = Some(member),
                    None => outbox.end(),
                },
            }
            return;
        }
        let Some(player) = peer.player else {
            // (A card command or a use at the griddle before the join is nobody's: passed over, as one that is wrong is. The
            // handshake's time limit still cuts the socket off if it never joins.)
            if is(message, "cards") || is(message, "griddle") {
                return;
            }
            return self.join(id, message);
        };
        if is(message, "ready") {
            if let Some(member) = peer.member {
                self.lobby.ready(member, message.get("ready") == Some(&Value::Bool(true)));
            }
        } else if is(message, "tune") {
            self.tune(id, message);
        } else if is(message, "cards") {
            self.cards(player, message);
        } else if is(message, "griddle") {
            // Use, at a griddle: the game says whether one flips; if so every socket is told at once.
            if self.lobby.game.use_griddle(player) {
                self.tell_griddles();
            }
        } else if is(message, "pov") {
            // The player's own view, for the recording (replay.rs): checked, kept or refused, never played.
            self.replays.pov(self.game, self.tick, self.net.tick, player, message);
        } else if is(message, "input") {
            // The debug encoding's commands (protocol.rs), and its ack.
            let commands = protocol::commands(message);
            self.lobby.game.commands(player, &commands);
            if let Some(ack) = protocol::integer(&message["ack"]) {
                self.peers.get_mut(&id).unwrap().stream.ack(ack as u32);
            }
        }
    }
    /// Whether the tuning may be changed from a game socket: in the sandbox, on a --dev server.
    fn tunable(&self) -> bool {
        self.lobby.dev() && self.lobby.game.rules().sandbox
    }
    /// The tuning as a game socket is told it: what differs from the constants, and whether it may change it.
    fn tuning(&self) -> String {
        protocol::encode(&json!({"type": "tuning", "text": self.lobby.game.feast.tune.text(), "editable": self.tunable()}))
    }
    /// A game socket's `tune` (`{type:'tune',text}`, kitchen_sim::tune: `mustard.speed=1.1`, `reset sniper`,
    /// `reset`): taken in the sandbox on a --dev server, and every game socket is told the tuning now; otherwise only
    /// the sender is told the tuning as it stands.
    fn tune(&mut self, id: u64, message: &Value) {
        let text = message.get("text").and_then(Value::as_str).unwrap_or("");
        if self.tunable() && text.len() <= 4000 {
            self.lobby.game.feast.tune.apply(text);
            let told = self.tuning();
            for peer in self.peers.values().filter(|peer| peer.player.is_some()) {
                peer.outbox.send_text(told.clone());
            }
            self.replays.text(self.game, self.tick, &told);
        } else if let Some(peer) = self.peers.get(&id) {
            peer.outbox.send_text(self.tuning());
        }
    }
    /// A game socket's `cards` (`{type:'cards',action:'flip'|'stop'|'target',seat?}`) from the hotdog `player`: the seat
    /// that acts is that hotdog's own place among the players, whatever the message says; `seat` is only the hotdog a
    /// `target` picks, a whole number below the number of seats. Anything else (another action, a `seat` that is
    /// missing, not whole or out of range) is passed over; so is what the game refuses (not that seat's go, no table).
    fn cards(&mut self, player: u32, message: &Value) {
        let game = &mut self.lobby.game;
        let Some(seat) = game.seat(player) else {
            return;
        };
        let command = match message.get("action").and_then(Value::as_str) {
            Some("flip") => cards::Command::Flip,
            Some("stop") => cards::Command::Stop,
            Some("target") => {
                let picked = message.get("seat").and_then(protocol::integer).filter(|n| *n >= 0. && *n < game.seats() as f64);
                let Some(picked) = picked else {
                    return;
                };
                cards::Command::Target(picked as usize)
            }
            _ => return,
        };
        game.cards(seat, command);
    }
    /// Every frame step: the card table to every game socket if it has changed since it was last sent. A game with no
    /// table has nothing to send.
    fn tell_cards(&mut self) {
        let Some(table) = self.lobby.game.feast.cards.as_ref() else {
            self.cards_sent = None;
            return;
        };
        if self.cards_sent == Some(table.version()) {
            return;
        }
        self.cards_sent = Some(table.version());
        let told = table_text(table);
        for peer in self.peers.values().filter(|peer| peer.player.is_some()) {
            peer.outbox.send_text(told.clone());
        }
        self.replays.text(self.game, self.tick, &told);
    }
    /// Every frame step: the griddles' states to every game socket when one has been flipped or a round has begun since they were
    /// last sent, and every SYNC seconds otherwise. A map without a griddle has nothing to send.
    fn tell_griddles(&mut self) {
        let game = &self.lobby.game;
        if game.feast.griddles.is_empty() {
            self.griddles_sent = None;
            return;
        }
        let key = (game.feast.round, game.feast.griddles.iter().map(|g| g.flips).collect::<Vec<_>>());
        let every = (griddle::SYNC * self.net.tick as f32).max(1.) as u32;
        if self.griddles_sent.as_ref().is_some_and(|(sent, at)| *sent == key && self.tick.wrapping_sub(*at) < every) {
            return;
        }
        self.griddles_sent = Some((key, self.tick));
        let told = griddle_text(&game.feast.griddle_states());
        for peer in self.peers.values().filter(|peer| peer.player.is_some()) {
            peer.outbox.send_text(told.clone());
        }
        self.replays.text(self.game, self.tick, &told);
    }
    /// Every frame step: the fight (Kung Fu) to every game socket if it has changed since it was last sent. A game that is
    /// not Kung Fu has nothing to send.
    fn tell_kungfu(&mut self) {
        let Some(view) = self.lobby.game.feast.kungfu_view() else {
            self.kungfu_sent = None;
            return;
        };
        let key = serde_json::to_string(&kungfu::View { elapsed: 0., ..view.clone() }).unwrap_or_default();
        if self.kungfu_sent.as_ref() == Some(&key) {
            return;
        }
        self.kungfu_sent = Some(key);
        let told = kungfu_text(&view);
        for peer in self.peers.values().filter(|peer| peer.player.is_some()) {
            peer.outbox.send_text(told.clone());
        }
        self.replays.text(self.game, self.tick, &told);
    }
    /// Every frame step: every hotdog's reserves and the reload under way (kitchen_sim::magazine `Stock`), what a frame
    /// does not carry, to every game socket when any of it has changed since it was last sent. Kung Fu and the card table
    /// have no guns, and nothing to send.
    fn tell_ammo(&mut self) {
        let feast = &self.lobby.game.feast;
        if feast.kungfu.is_some() || feast.cards.is_some() || feast.players.is_empty() {
            self.ammo_sent = None;
            return;
        }
        let told = ammo_text(&feast.stocks());
        if self.ammo_sent.as_ref() == Some(&told) {
            return;
        }
        self.ammo_sent = Some(told.clone());
        for peer in self.peers.values().filter(|peer| peer.player.is_some()) {
            peer.outbox.send_text(told.clone());
        }
        self.replays.text(self.game, self.tick, &told);
    }
    /// A binary message: from a game socket that has joined, a batch of commands; anything else ends the socket.
    fn binary(&mut self, id: u64, bytes: &[u8]) {
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        };
        let (Some(player), false) = (peer.player, peer.page) else {
            return peer.outbox.destroy();
        };
        let Ok(batch) = Batch::decode(bytes) else {
            return peer.outbox.destroy();
        };
        peer.stream.ack(batch.ack);
        if batch.resync {
            // Whole frames until the client acknowledges one: what it had may be no use.
            (peer.stream.resync, peer.stream.acked) = (true, 0);
        }
        self.lobby.game.commands(player, &batch.commands);
    }
    /// A game socket's first message: the join handshake. The page's lobby socket was given a token and the game
    /// hands it over here, so the hotdog is that member's; a client without a page joins as a guest, with the name
    /// and colours it asks for. The welcome says which hotdog it plays, on which map, and how often the game is
    /// stepped and sent; a whole frame follows at once.
    fn join(&mut self, id: u64, message: &Value) {
        let outbox = self.peers[&id].outbox.clone();
        let refuse = |text: &str| {
            outbox.send_text(protocol::error(text));
            outbox.end();
        };
        if !is(message, "join") || message.get("version").and_then(Value::as_f64) != Some(PROTOCOL as f64) {
            return refuse(protocol::MISMATCH);
        }
        let member = match message.get("token") {
            None => self.lobby.guest(message),
            Some(token) => self.lobby.claim(token),
        };
        let Some(member) = member else {
            return refuse(if message.get("token").is_none() { protocol::FULL } else { protocol::UNKNOWN_PAGE });
        };
        // The dev sandbox (--dev): the first to join a game nobody plays may ask for it, and everyone after plays it.
        if let Some(asked) = message.get("sandbox").filter(|asked| asked.is_object())
            && self.lobby.dev()
            && self.lobby.game.players() == 0
            && self.lobby.sandbox(asked)
        {
            self.game += 1;
            self.cards_sent = None;
            self.griddles_sent = None;
            self.kungfu_sent = None;
            self.ammo_sent = None;
            println!("Sandbox: {} on {}", self.lobby.game.rules().text(), lobby::played(&self.lobby.game));
        }
        let Some(player) = self.lobby.game.join() else {
            if !self.lobby.member(member).is_some_and(|m| m.voice) {
                self.lobby.remove(member);
            }
            return refuse(protocol::FULL);
        };
        let peer = self.peers.get_mut(&id).unwrap();
        (peer.member, peer.player) = (Some(member), Some(player));
        peer.stream.json |= truthy(&message["json"]);
        self.lobby.seat(member, player);
        outbox.send_text(protocol::welcome(player, &self.lobby.game.text, self.net.tick, self.net.send));
        // The tuning, when it is not the constants' or may be changed (the sandbox).
        if self.tunable() || !self.lobby.game.feast.tune.text().is_empty() {
            outbox.send_text(self.tuning());
        }
        // The card table, as it stands, when there is one.
        if let Some(table) = self.lobby.game.feast.cards.as_ref() {
            outbox.send_text(table_text(table));
        }
        // The griddles, as they stand, when there are any.
        if !self.lobby.game.feast.griddles.is_empty() {
            outbox.send_text(griddle_text(&self.lobby.game.feast.griddle_states()));
        }
        // The fight, as it stands, in Kung Fu.
        if let Some(view) = self.lobby.game.feast.kungfu_view() {
            outbox.send_text(kungfu_text(&view));
        }
        // The reserves and reloads, as they stand, where there are guns.
        let feast = &self.lobby.game.feast;
        if feast.kungfu.is_none() && feast.cards.is_none() && !feast.players.is_empty() {
            outbox.send_text(ammo_text(&feast.stocks()));
        }
        let frame = self.lobby.frame();
        let state = Arc::new(frame.state());
        let (tick, game, checksum) = (self.tick, self.game, state.checksum());
        if let Some(peer) = self.peers.get_mut(&id) {
            send_frame(peer, &self.lobby, &frame, &state, checksum, tick, game);
        }
    }
    fn closed(&mut self, id: u64) {
        let Some(peer) = self.peers.remove(&id) else {
            return;
        };
        if peer.page {
            if let Some(member) = peer.member {
                self.lobby.leave(member);
            }
        } else if let Some(player) = peer.player {
            self.replays.forget(player);
            self.lobby.game.leave(player);
            if let Some(member) = peer.member {
                self.lobby.unseat(member);
            }
            // The sandbox lasts while somebody plays it.
            if self.lobby.game.rules().sandbox && self.lobby.game.players() == 0 {
                self.lobby.unsandbox();
                self.game += 1;
                self.cards_sent = None;
                self.griddles_sent = None;
                self.kungfu_sent = None;
                self.ammo_sent = None;
                println!("Sandbox over: back to {}", lobby::played(&self.lobby.game));
            }
        }
    }
    /// One step: the commands due and the game (`Game::tick`); every `Net::every()`th step the lobby too. A round
    /// starting on another map is told to every game socket at once: the map, and the hotdog it now plays there; what
    /// each was sent of the old map is forgotten, so its next frame is whole. Returns whether a frame is due.
    pub fn advance(&mut self) -> bool {
        let started = Instant::now();
        self.lobby.game.tick(1. / self.net.tick as f32);
        self.tick += 1;
        let due = self.tick % self.net.every() == 0;
        if due && let Some(seats) = self.lobby.tick() {
            self.game += 1;
            self.cards_sent = None;
            self.griddles_sent = None;
            self.kungfu_sent = None;
            self.ammo_sent = None;
            let told = (!self.lobby.game.feast.tune.text().is_empty()).then(|| self.tuning());
            let game = &self.lobby.game;
            println!("Next round: {}", lobby::played(game));
            for peer in self.peers.values_mut().filter(|peer| peer.player.is_some()) {
                peer.stream.restart();
                match peer.member.and_then(|member| seats.get(&member)) {
                    Some(&player) => {
                        peer.player = Some(player);
                        peer.outbox.send_text(protocol::map(player, &game.text));
                        if let Some(told) = &told {
                            peer.outbox.send_text(told.clone());
                        }
                    }
                    None => {
                        peer.player = None;
                        peer.outbox.send_text(protocol::error(protocol::NO_PLACE));
                        peer.outbox.end();
                    }
                }
            }
        }
        if due {
            self.tell_cards();
            self.tell_griddles();
            self.tell_kungfu();
            self.tell_ammo();
        }
        let spent = started.elapsed();
        self.stats.steps += 1;
        self.stats.step += spent;
        self.stats.worst = self.stats.worst.max(spent);
        due
    }
    /// A frame to every game socket: the state made once, encoded for each against what it acknowledged. A socket
    /// that never said who it is goes.
    pub fn send(&mut self) {
        let started = Instant::now();
        let frame = self.lobby.frame();
        let state = Arc::new(frame.state());
        let (tick, game, checksum) = (self.tick, self.game, state.checksum());
        for peer in self.peers.values_mut().filter(|peer| peer.player.is_some()) {
            send_frame(peer, &self.lobby, &frame, &state, checksum, tick, game);
        }
        // The replay: the same shared frame, for a spectator (replay.rs), while anybody plays.
        let playing = self.peers.values().filter(|peer| peer.player.is_some()).count();
        self.replays.frame(&self.lobby, game, tick, self.net.tick, playing, &frame, &state);
        for peer in self.peers.values().filter(|peer| peer.player.is_none() && peer.member.is_none() && peer.opened.elapsed() > HANDSHAKE) {
            peer.outbox.destroy();
        }
        self.stats.sends += 1;
        self.stats.send += started.elapsed();
    }
    /// Every `--stats` seconds: the cost of a step and of a round of frames, how many bytes each game socket was sent
    /// a second, and the server's resident memory.
    fn report(&mut self) {
        let Some(every) = self.net.stats else {
            return;
        };
        let since = *self.stats.since.get_or_insert_with(Instant::now);
        let wall = since.elapsed();
        if wall < every {
            return;
        }
        let s = &self.stats;
        let ms = |d: Duration, n: u64| d.as_secs_f64() * 1000. / n.max(1) as f64;
        let streams: Vec<&Stream> = self.peers.values().filter(|p| p.player.is_some()).map(|p| &p.stream).collect();
        let rates: Vec<String> = streams.iter().map(|s| format!("{:.1}", s.bytes as f64 / wall.as_secs_f64() / 1024.)).collect();
        let frames: u64 = streams.iter().map(|s| s.frames).sum();
        let whole: u64 = streams.iter().map(|s| s.whole).sum();
        let bytes: u64 = streams.iter().map(|s| s.bytes).sum();
        println!(
            "stats: {} steps, {:.3} ms a step (worst {:.3}), {:.3} ms a round of frames, {} late; {} game sockets, KB/s each [{}], {:.0} bytes a frame, {} whole of {}; {} enemies; rss {:.1} MB",
            s.steps,
            ms(s.step, s.steps),
            s.worst.as_secs_f64() * 1000.,
            ms(s.send, s.sends),
            s.behind,
            streams.len(),
            rates.join(" "),
            bytes as f64 / frames.max(1) as f64,
            whole,
            frames,
            self.lobby.game.feast.enemies.len(),
            resident() / 1048576.,
        );
        self.stats = Stats { since: Some(Instant::now()), ..Stats::default() };
        for peer in self.peers.values_mut() {
            (peer.stream.bytes, peer.stream.frames, peer.stream.whole) = (0, 0, 0);
        }
    }
    /// What is being played now (`map`, `mode`, `stage`), what the next round plays (`choice`) and who chooses it
    /// (`leader`, a name), how many maps there are, the players, the protocol and the rates.
    pub fn status(&mut self) -> Value {
        let leader = self.lobby.lead().and_then(|id| self.lobby.member(id)).map(|m| m.name.clone());
        let game = &self.lobby.game;
        json!({
            "game": "hotdog-simulator",
            "players": game.players(),
            "humans": self.peers.values().filter(|peer| peer.player.is_some()).count(),
            "lobby": self.lobby.members.len(),
            "stage": game.stage(),
            "capacity": CAPACITY,
            "protocol": PROTOCOL,
            "tick": self.net.tick,
            "send": self.net.send,
            "step": self.tick,
            "map": game.map.name,
            "mode": game.feast.mode().name(),
            "rules": game.rules().text(),
            "dev": self.lobby.dev(),
            "choice": self.lobby.choice.shown(),
            "leader": leader,
            "maps": self.lobby.maps.len(),
        })
    }
}
/// The card table as game sockets are told it: `{type:'cards',table}` with the table's view (kitchen_sim::cards).
fn table_text(table: &cards::Table) -> String {
    #[derive(Serialize)]
    struct Told<'a> {
        r#type: &'static str,
        table: &'a cards::View,
    }
    protocol::encode(&Told { r#type: "cards", table: &table.view() })
}
/// The griddles as game sockets are told them: `{type:'griddle',griddles}` with each one's state (kitchen_sim::griddle).
fn griddle_text(states: &[griddle::State]) -> String {
    #[derive(Serialize)]
    struct Told<'a> {
        r#type: &'static str,
        griddles: &'a [griddle::State],
    }
    protocol::encode(&Told { r#type: "griddle", griddles: states })
}
/// The fight as game sockets are told it: `{type:'kungfu', ...View}` (kitchen_sim::kungfu).
pub fn kungfu_text(view: &kungfu::View) -> String {
    #[derive(Serialize)]
    struct Told<'a> {
        r#type: &'static str,
        #[serde(flatten)]
        view: &'a kungfu::View,
    }
    protocol::encode(&Told { r#type: "kungfu", view })
}
/// The hotdogs' reserves and reloads as game sockets are told them: `{type:'ammo',hotdogs}` with each one's
/// kitchen_sim::magazine `Stock` (its id, its reserve by gun, and the reload under way: `gun`, 0 for none, `at` the round's
/// clock it began, `of` how long it takes).
pub fn ammo_text(stocks: &[kitchen_sim::magazine::Stock]) -> String {
    #[derive(Serialize)]
    struct Told<'a> {
        r#type: &'static str,
        hotdogs: &'a [kitchen_sim::magazine::Stock],
    }
    protocol::encode(&Told { r#type: "ammo", hotdogs: stocks })
}
/// One game socket's frame: the shared `frame` (and its `state`) with this client's ack and body, encoded against
/// what it acknowledged (or as JSON), with the shots, splats and knocks it has not been told of yet.
fn send_frame(peer: &mut Peer, lobby: &Lobby, frame: &Frame, state: &Arc<State>, checksum: u32, tick: u32, game: u64) {
    let Some(player) = peer.player else {
        return;
    };
    let g = &lobby.game;
    let ack = g.applied(player);
    let mine = g.player(player).filter(|p| p.alive).and(g.feast.body(player)).map(Mine::of);
    let stream = &mut peer.stream;
    if stream.json {
        let mut value = serde_json::to_value(frame).unwrap_or_default();
        (value["tick"], value["ack"]) = (json!(tick), json!(ack));
        if let Some(mine) = mine {
            value["mine"] = json!(mine);
        }
        let text = protocol::encode(&value);
        (stream.bytes, stream.frames, stream.whole) = (stream.bytes + text.len() as u64, stream.frames + 1, stream.whole + 1);
        peer.outbox.send_text(text);
        return;
    }
    if stream.era != (game, frame.round) {
        (stream.era, stream.told) = ((game, frame.round), [0; 3]);
    }
    let events = Events {
        shots: frame.shots.iter().filter(|s| s.id > stream.told[0]).cloned().collect(),
        splats: frame.splats.iter().filter(|s| s.id > stream.told[1]).cloned().collect(),
        knocks: frame.knocks.iter().filter(|k| k.id > stream.told[2]).cloned().collect(),
    };
    stream.told = [
        events.shots.iter().map(|s| s.id).fold(stream.told[0], u32::max),
        events.splats.iter().map(|s| s.id).fold(stream.told[1], u32::max),
        events.knocks.iter().map(|k| k.id).fold(stream.told[2], u32::max),
    ];
    let base = stream.base().cloned();
    let header = Header { tick, base: base.as_ref().map_or(0, |(t, _)| *t), ack, checksum, elapsed: frame.elapsed, mine };
    let bytes = wire::encode_frame(&header, base.as_ref().map(|(_, s)| &**s), state, &events);
    stream.bytes += bytes.len() as u64;
    stream.frames += 1;
    if base.is_none() {
        stream.whole += 1;
        stream.resync = false;
    }
    stream.keep(tick, state.clone());
    peer.outbox.send_binary(bytes);
}
/// The steps owed at `now` when the next was due at `next`, each `step` long: every one that has come due (CATCH_UP at
/// most), when the next is due after them, and the time let go when more than CATCH_UP were owed (the next is then
/// due a step from now).
pub fn owed(next: Instant, now: Instant, step: Duration) -> (u32, Instant, Option<Duration>) {
    let (mut steps, mut next) = (0, next);
    while next <= now && steps < CATCH_UP {
        next += step;
        steps += 1;
    }
    if next <= now {
        return (steps, now + step, Some(now - next));
    }
    (steps, next, None)
}
/// The server's resident memory in bytes (Linux; 0 elsewhere).
pub fn resident() -> f64 {
    std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split_whitespace().nth(1)?.parse::<f64>().ok()).map_or(0., |pages| pages * 4096.)
}

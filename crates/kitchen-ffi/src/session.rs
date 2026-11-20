// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The game behind a handle.
//!
//! - **Offline** a `Feast` is stepped here: the local player's command is applied as the server applies a person's
//!   (crates/server game.rs `Game::command`, ported as `apply_command`), then the round steps (bots, physics, rules).
//! - **Online** the `Feast` is this client's copy of the server's round, built from the welcome's map and kept from the
//!   frames: decoded against the states kept (a port of src/network.rs `Network::decode` and `batches`), mirrored into
//!   the Feast as src/arena.rs `apply_frame` does, and this client's own hotdog predicted ahead of the server (a port of
//!   src/arena/online.rs `Predictor`, `settle` and `mirror`) with kitchen-sim's own `Feast::predict` and `rehearse`.
//!   Those three pieces of glue live in the Bevy client, not the simulation; the report lists what to move into
//!   kitchen-sim so that both clients share them.
use crate::events::{self, Watch};
use kitchen_sim::feast::{self, Stage};
use kitchen_sim::kungfu;
use kitchen_sim::rules::Mode;
use kitchen_sim::map::Prop;
use kitchen_sim::net;
use crate::*;
use glam::{Vec2, Vec3};
use kitchen_sim::actor::{self, Actor};
use kitchen_sim::cards;
use kitchen_sim::feast::{Feast, Player};
use kitchen_sim::griddle::{self, Plate};
use kitchen_sim::kungfu::View;
use kitchen_sim::map::Map;
use kitchen_sim::net::{Batch, Command, Frame, Mine, State, button};
use kitchen_sim::props::{PropState, State as Held};
use kitchen_sim::rules::Rules;
use kitchen_sim::tune::Tuning;
use std::collections::VecDeque;

/// How many decoded frames' states are kept for the server to encode against (network.rs KEPT), and how many
/// commands already sent go again with the new ones (AGAIN).
const KEPT: usize = 64;
const AGAIN: usize = 2;
/// The most commands one step makes (online.rs MOST); a correction further than SNAP is taken at once, nearer it is
/// eased away at EASE a second; a prediction within AGREE of the server's word needs no correcting.
const MOST: usize = 15;
const SNAP: f32 = 0.25;
const EASE: f32 = 12.;
const AGREE: f64 = 0.0015;
/// The most events kept waiting.
pub(crate) const QUEUE: usize = 4096;

/// The local player's input as last set: what is held, and the edges (and pickup) latched for the next command.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub stick: Vec2,
    pub yaw: f32,
    pub pitch: f32,
    pub held: u16,
    pub latch: u16,
    pub weapon: u8,
    pub pickup: Option<usize>,
}

pub struct Game {
    /// Offline: the round. Online: this client's copy of it (None until the welcome).
    pub feast: Option<Feast>,
    /// The local player's hotdog.
    pub me: u32,
    /// Bumped whenever the map changes.
    pub generation: u32,
    pub input: Input,
    /// The last command's number.
    pub seq: u32,
    /// Offline: who a bot plays, as asked (`ks_game_set_bot`): a new round sets its hotdogs out afresh, and these again.
    pub bots: Vec<(u32, bool)>,
    /// Kung Fu: the fight (offline after every step; online from `{type:'kungfu'}`).
    pub kungfu: Option<View>,
    /// Online, Fully Loaded: the table as the server last told it.
    pub cards: Option<cards::View>,
    pub events: VecDeque<KsEvent>,
    pub watch: Watch,
    pub online: Option<Box<Online>>,
    /// The rules as text (`KsMatch::rules` points into it).
    pub rules_text: String,
    /// Text handed out by `ks_cards_json` and `ks_map_json`.
    pub cache: String,
}

/// An online game's connection and prediction.
pub struct Online {
    pub welcomed: bool,
    pub hz: f32,
    pub send_hz: f32,
    /// network.rs: the decoded frames' states by step, the newest decoded and the newest acknowledged, whether to ask
    /// for a whole frame and when that was last asked; the commands sent lately; this client's clock (seconds).
    states: VecDeque<(u32, State)>,
    newest: u32,
    told: u32,
    resync: bool,
    asked: f64,
    recent: VecDeque<Command>,
    clock: f64,
    pub frames: u64,
    pub broken: u64,
    pub whole: u64,
    /// What waits to be sent (kind, bytes), and the bytes of the last message handed out.
    outbox: VecDeque<(u32, Vec<u8>)>,
    sent: Vec<u8>,
    /// online.rs: commands made and not yet sent, time not yet made into a command.
    unsent: Vec<Command>,
    acc: f32,
    /// The prediction: on, the commands sent and not yet applied each with the body after it, this client's copy of
    /// the bodies, where the predicted body stood after the last two steps, the correction being eased away, the stim
    /// just slapped on.
    pub predict: bool,
    pending: VecDeque<(Command, Actor)>,
    ghosts: actor::World,
    prev: Vec3,
    cur: Vec3,
    offset: Vec3,
    stim: f32,
    pub corrections: u32,
    pub snaps: u32,
    /// The newest frame (its seats, the lobby's countdown, Forts' state), the last command the server applied, the round.
    pub latest: Option<Frame>,
    pub ack: u32,
    round: u32,
    /// The server's last error.
    pub error: String,
    tuning: Tuning,
    /// The tuning as the server wrote it, and whether the sandbox's panel may change it.
    tuning_text: String,
    tuning_editable: bool,
    /// The hotdogs' reserves and reloads as the server last told them (`{type:'ammo'}`): a frame does not carry them,
    /// so they are put back into the copy of the round after each.
    stocks: Vec<kitchen_sim::magazine::Stock>,
}
impl Online {
    fn new(join: String) -> Online {
        Online {
            welcomed: false,
            hz: net::TICK_HZ as f32,
            send_hz: net::SEND_HZ as f32,
            states: VecDeque::new(),
            newest: 0,
            told: 0,
            resync: false,
            asked: -1.,
            recent: VecDeque::new(),
            clock: 0.,
            frames: 0,
            broken: 0,
            whole: 0,
            outbox: VecDeque::from([(KS_MESSAGE_TEXT, join.into_bytes())]),
            stocks: vec![],
            sent: vec![],
            unsent: vec![],
            acc: 0.,
            predict: true,
            pending: VecDeque::new(),
            ghosts: actor::World::default(),
            prev: Vec3::ZERO,
            cur: Vec3::ZERO,
            offset: Vec3::ZERO,
            stim: 0.,
            corrections: 0,
            snaps: 0,
            latest: None,
            ack: 0,
            round: 0,
            error: String::new(),
            tuning: Tuning::default(),
            tuning_text: String::new(),
            tuning_editable: false,
        }
    }
    fn text(&mut self, value: serde_json::Value) {
        self.outbox.push_back((KS_MESSAGE_TEXT, value.to_string().into_bytes()));
    }
    /// Forgets the frames decoded (a new map: the next frame is whole) and the prediction.
    fn forget(&mut self) {
        self.states.clear();
        (self.newest, self.told) = (0, 0);
        self.reset_prediction();
        self.latest = None;
        self.round = 0;
    }
    fn reset_prediction(&mut self) {
        self.pending.clear();
        self.ghosts.actors.clear();
        (self.acc, self.offset, self.stim) = (0., Vec3::ZERO, 0.);
    }
    /// network.rs `decode`: a frame against the state of the one its header names, checked. Ok(None) for one older than
    /// the newest.
    fn decode(&mut self, bytes: &[u8]) -> Res<Option<Frame>> {
        let (tick, base) = match net::peek_frame(bytes) {
            Ok(v) => v,
            Err(e) => {
                self.broken += 1;
                return fail(KsStatus::KS_ERR_MALFORMED, format!("not a frame: {}", e.0));
            }
        };
        if tick <= self.newest {
            return Ok(None);
        }
        let at = match base {
            0 => None,
            base => match self.states.iter().position(|(t, _)| *t == base) {
                Some(i) => Some(i),
                None => {
                    (self.resync, self.broken) = (true, self.broken + 1);
                    return fail(KsStatus::KS_ERR_MALFORMED, format!("frame {tick} is against frame {base}, which this client has not got: a whole frame is asked for"));
                }
            },
        };
        let decoded = net::decode_frame(bytes, at.map(|i| &self.states[i].1));
        let (header, state, events) = match decoded {
            Ok(v) => v,
            Err(e) => {
                (self.resync, self.broken) = (true, self.broken + 1);
                return fail(KsStatus::KS_ERR_MALFORMED, format!("frame {tick} could not be decoded: {}", e.0));
            }
        };
        if state.checksum() != header.checksum {
            (self.resync, self.broken) = (true, self.broken + 1);
            return fail(KsStatus::KS_ERR_MALFORMED, format!("frame {tick} does not check out: a whole frame is asked for"));
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
        Ok(Some(frame))
    }
    /// network.rs `batches`: the batches that send `commands`, each repeating the few sent before it, with the newest
    /// frame decoded; with no command, the ack alone when there is a new one (or a whole frame to ask for).
    fn batches(&mut self, commands: Vec<Command>) -> Vec<Batch> {
        let now = self.clock;
        let resync = self.resync && now - self.asked > 0.25;
        if resync {
            self.asked = now;
        }
        let time = (now * 1000.).max(0.) as u32;
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
    /// Where this client's own hotdog is drawn: between its last two predicted steps, less what is left of a correction.
    fn shown(&self, me: u32) -> Option<Vec3> {
        self.ghosts.get(me)?;
        let step = 1. / self.hz;
        Some(self.prev.lerp(self.cur, (self.acc / step).clamp(0., 1.)) + self.offset)
    }
}

/// What a predicted command's walk depends on beyond the command (online.rs `Context`, without Kung Fu's foresight).
#[derive(Clone, Copy)]
struct Ctx {
    me: u32,
    dt: f32,
    stim: bool,
    burden: f32,
}

impl Game {
    fn blank(feast: Option<Feast>, me: u32, online: Option<Box<Online>>) -> Game {
        Game {
            feast,
            me,
            generation: 1,
            input: Input::default(),
            seq: 0,
            bots: vec![],
            kungfu: None,
            cards: None,
            events: VecDeque::new(),
            watch: Watch::default(),
            online,
            rules_text: String::new(),
            cache: String::new(),
        }
    }
    /// An offline game: the map, played by the rules, its draws from `seed`.
    pub fn offline(map: Map, rules: &str, seed: u64, flags: u32) -> Game {
        let mut feast = Feast::with_map(map, false);
        feast.card_seed = seed;
        feast.follow = flags & KS_GAME_FOLLOW != 0;
        feast.set_rules(Rules::parse(rules));
        let mut g = Game::blank(Some(feast), 1, None);
        if flags & KS_GAME_ALL_BOTS != 0 {
            g.bots.push((1, true));
        }
        g.apply_bots();
        g.watch.round = g.feast.as_ref().map_or(0, |f| f.round);
        g.refresh();
        g
    }
    /// An online game waiting for its welcome; `join` is the first message to send.
    pub fn online(join: String) -> Game {
        Game::blank(None, 0, Some(Box::new(Online::new(join))))
    }
    pub fn feast(&self) -> Res<&Feast> {
        match &self.feast {
            Some(f) => Ok(f),
            None => fail(KsStatus::KS_ERR_NOT_READY, "no map yet: the server's welcome has not been received"),
        }
    }
    fn online_mut(&mut self) -> Res<&mut Online> {
        match self.online.as_deref_mut() {
            Some(o) => Ok(o),
            None => fail(KsStatus::KS_ERR_WRONG_MODE, "an online call on an offline game"),
        }
    }
    fn offline_feast(&mut self) -> Res<&mut Feast> {
        if self.online.is_some() {
            return fail(KsStatus::KS_ERR_WRONG_MODE, "an offline call on an online game");
        }
        self.feast.as_mut().ok_or_else(|| Fail { status: KsStatus::KS_ERR_NOT_READY, message: "no round".into() })
    }
    /// Whether the round is Kung Fu (its GRAB is the block, a level).
    fn kungfu_on(&self) -> bool {
        self.kungfu.is_some() || self.feast.as_ref().is_some_and(|f| f.kungfu.is_some() || f.rules.kungfu || f.level.map.mode == kungfu::MODE)
    }
    fn edges(&self) -> u16 {
        if self.kungfu_on() { button::EDGES & !button::GRAB } else { button::EDGES }
    }
    /// After anything changed the round: the rules' text, the fight (offline), the events.
    fn refresh(&mut self) {
        let Some(f) = self.feast.as_ref() else {
            return;
        };
        self.rules_text = f.rules.text();
        if self.online.is_none() {
            self.kungfu = f.kungfu_view();
            events::offline(self);
        }
    }
    fn apply_bots(&mut self) {
        if let Some(f) = self.feast.as_mut() {
            for (id, bot) in &self.bots {
                if let Some(p) = f.player_mut(*id) {
                    p.bot = *bot;
                }
            }
        }
    }

    // ----- Input and steps

    pub fn set_command(&mut self, c: &KsCommand) -> Res {
        if ![c.stick_x, c.stick_y, c.yaw, c.pitch].iter().all(|v| v.is_finite()) {
            return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, "the stick, yaw and pitch must be finite numbers");
        }
        if c.weapon >= KS_WEAPONS {
            return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, format!("weapon {} (0 to keep, 1..7)", c.weapon));
        }
        if c.pickup < -1 || c.pickup > u16::MAX as i32 {
            return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, format!("pickup {} (-1 for none)", c.pickup));
        }
        if let Some(f) = self.feast.as_ref().filter(|f| c.pickup >= 0 && c.pickup as usize >= f.pickups.len().max(f.level.map.pickups.len())) {
            return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, format!("pickup {}: the map has {}", c.pickup, f.level.map.pickups.len()));
        }
        let buttons = (c.buttons & KS_BUTTON_ALL) as u16;
        let edges = self.edges();
        let i = &mut self.input;
        i.held = buttons & !edges;
        i.latch |= buttons & edges;
        (i.stick, i.yaw, i.pitch, i.weapon) = (Vec2::new(c.stick_x, c.stick_y), c.yaw, c.pitch, c.weapon as u8);
        if c.pickup >= 0 {
            i.pickup = Some(c.pickup as usize);
        }
        Ok(())
    }
    /// The next command: what is held, and the edges and pickup latched since the last.
    fn take_command(input: &mut Input, seq: &mut u32) -> Command {
        *seq += 1;
        let buttons = input.held | std::mem::take(&mut input.latch);
        Command::new(*seq, input.stick, input.yaw, input.pitch, buttons, input.weapon, input.pickup.take())
    }
    pub fn step(&mut self, dt: f32) -> Res {
        if self.online.is_some() {
            return self.step_online(dt);
        }
        let me = self.me;
        let c = Game::take_command(&mut self.input, &mut self.seq);
        let f = self.feast.as_mut().ok_or_else(|| Fail { status: KsStatus::KS_ERR_NOT_READY, message: "no round".into() })?;
        let round = f.round;
        if f.player(me).is_some_and(|p| !p.bot) {
            apply_command(f, me, &c, dt);
        }
        f.tick(dt);
        if f.round != round {
            self.apply_bots();
        }
        self.refresh();
        Ok(())
    }
    pub fn restart(&mut self) -> Res {
        self.offline_feast()?.reset();
        self.apply_bots();
        self.refresh();
        Ok(())
    }
    pub fn set_bot(&mut self, id: u32, bot: bool) -> Res {
        let f = self.offline_feast()?;
        if f.player(id).is_none() {
            return fail(KsStatus::KS_ERR_NOT_FOUND, format!("no hotdog {id}"));
        }
        self.bots.retain(|(i, _)| *i != id);
        self.bots.push((id, bot));
        self.apply_bots();
        Ok(())
    }
    pub fn use_griddle(&mut self) -> Res {
        let me = self.me;
        if let Some(o) = self.online.as_deref_mut() {
            o.text(serde_json::json!({"type": "griddle"}));
            return Ok(());
        }
        let flipped = self.offline_feast()?.use_griddle(me);
        self.refresh();
        if flipped { Ok(()) } else { fail(KsStatus::KS_ERR_REFUSED, "no griddle's dial in reach, facing it (or it flipped a moment ago)") }
    }
    pub fn ready(&mut self, flag: bool) -> Res {
        let me = self.me;
        if let Some(o) = self.online.as_deref_mut() {
            o.text(serde_json::json!({"type": "ready", "ready": flag}));
            return Ok(());
        }
        if self.offline_feast()?.ready(me, flag) { Ok(()) } else { fail(KsStatus::KS_ERR_REFUSED, "not a Forts build phase, or the hotdog is dead or a bot") }
    }
    pub fn cards_command(&mut self, action: u32, seat: u32) -> Res {
        let command = match action {
            KS_CARDS_FLIP => cards::Command::Flip,
            KS_CARDS_STOP => cards::Command::Stop,
            KS_CARDS_TARGET => cards::Command::Target(seat as usize),
            _ => return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, format!("cards action {action}")),
        };
        let me = self.me;
        if let Some(o) = self.online.as_deref_mut() {
            let name = ["flip", "stop", "target"][action as usize];
            let mut m = serde_json::json!({"type": "cards", "action": name});
            if action == KS_CARDS_TARGET {
                m["seat"] = seat.into();
            }
            o.text(m);
            return Ok(());
        }
        let taken = self.offline_feast()?.cards_command(me, command);
        self.refresh();
        if taken { Ok(()) } else { fail(KsStatus::KS_ERR_REFUSED, "no card table, or not this seat's go") }
    }
    pub fn set_tuning(&mut self, text: &str) -> Res {
        if let Some(o) = self.online.as_deref_mut() {
            o.text(serde_json::json!({"type": "tune", "text": text}));
            return Ok(());
        }
        self.offline_feast()?.tune = Tuning::parse(text);
        Ok(())
    }

    // ----- Online

    pub fn set_prediction(&mut self, on: bool) -> Res {
        let o = self.online_mut()?;
        o.predict = on;
        if !on {
            o.reset_prediction();
        }
        Ok(())
    }
    pub fn send_text(&mut self, text: &str) -> Res {
        let o = self.online_mut()?;
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(v) if v.is_object() => {
                o.outbox.push_back((KS_MESSAGE_TEXT, text.as_bytes().to_vec()));
                Ok(())
            }
            _ => fail(KsStatus::KS_ERR_INVALID_ARGUMENT, "the message must be a JSON object"),
        }
    }
    pub fn poll(&mut self) -> Res<KsMessage> {
        let o = self.online_mut()?;
        Ok(match o.outbox.pop_front() {
            Some((kind, bytes)) => {
                o.sent = bytes;
                KsMessage { kind, data: KsStr { ptr: o.sent.as_ptr(), len: o.sent.len() } }
            }
            None => KsMessage { kind: KS_MESSAGE_NONE, data: KsStr::EMPTY },
        })
    }
    fn step_online(&mut self, dt: f32) -> Res {
        let kungfu = self.kungfu_on();
        let me = self.me;
        let Game { feast, online, input, seq, .. } = self;
        let o = online.as_deref_mut().expect("online");
        o.clock += dt as f64;
        let Some(feast) = feast.as_mut().filter(|_| o.welcomed) else {
            return Ok(());
        };
        let step = 1. / o.hz.max(1.);
        // (A stim just slapped on walks the prediction faster until a frame says so: online.rs `play`.)
        o.stim = (o.stim - dt).max(0.);
        if !kungfu && input.latch & button::STIM != 0 && feast.player(me).is_some_and(|p| p.stim_wait <= 0.) {
            o.stim = feast.tune.stim_time;
        }
        o.acc = (o.acc + dt).min(MOST as f32 * step);
        let predicting = o.predict && !kungfu;
        let ctx = Ctx { me, dt: step, stim: feast.player(me).is_some_and(|p| p.stim > 0.) || o.stim > 0., burden: feast.burden(me) };
        while o.acc >= step {
            o.acc -= step;
            let c = Game::take_command(input, seq);
            o.unsent.push(c);
            if let Some(at) = predicting.then(|| predict_step(feast, o, &c, ctx)).flatten() {
                (o.prev, o.cur) = (o.cur, at);
            }
        }
        o.offset *= (-dt * EASE).exp();
        if o.offset.length() < 1e-4 {
            o.offset = Vec3::ZERO;
        }
        let commands = std::mem::take(&mut o.unsent);
        for batch in o.batches(commands) {
            o.outbox.push_back((KS_MESSAGE_BINARY, batch.encode()));
        }
        Ok(())
    }
    pub fn receive_binary(&mut self, bytes: &[u8]) -> Res {
        let o = self.online_mut()?;
        if !o.welcomed {
            return fail(KsStatus::KS_ERR_NOT_READY, "a frame before the welcome");
        }
        if let Some(frame) = o.decode(bytes)? {
            self.take_frame(frame);
        }
        Ok(())
    }
    pub fn receive_text(&mut self, text: &str) -> Res {
        self.online_mut()?;
        let v: serde_json::Value = serde_json::from_str(text).or_else(|e| fail(KsStatus::KS_ERR_MALFORMED, format!("not JSON: {e}")))?;
        if !v.is_object() {
            return fail(KsStatus::KS_ERR_MALFORMED, "not a JSON object");
        }
        let kind = v["type"].as_str().unwrap_or("").to_string();
        match kind.as_str() {
            "welcome" | "map" => {
                let map = match Map::parse(v["map"].as_str().unwrap_or("")) {
                    Ok(map) => map,
                    Err(e) => {
                        let o = self.online_mut()?;
                        o.error = format!("The server's map could not be read: {}", e.0);
                        return fail(KsStatus::KS_ERR_MALFORMED, o.error.clone());
                    }
                };
                let id = v["id"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
                let o = self.online_mut()?;
                if kind == "welcome" {
                    o.welcomed = true;
                    o.hz = v["tick"].as_f64().filter(|n| n.is_finite() && *n >= 1.).unwrap_or(net::TICK_HZ as f64) as f32;
                    o.send_hz = v["send"].as_f64().filter(|n| n.is_finite() && *n >= 1.).unwrap_or(net::SEND_HZ as f64) as f32;
                    o.error.clear();
                }
                o.forget();
                let mut feast = Feast::with_map(map, false);
                feast.tune = o.tuning.clone();
                self.feast = Some(feast);
                (self.me, self.kungfu, self.cards) = (id, None, None);
                // (What was pressed before there was a round to press it in is let go.)
                (self.input.latch, self.input.pickup) = (0, None);
                self.generation += 1;
                self.watch = Watch::default();
                self.rules_text.clear();
                events::push(&mut self.events, KsEvent { kind: KS_EVENT_MAP, value: self.generation as i32, by: id, ..Default::default() });
            }
            "tuning" => {
                let text = v["text"].as_str().unwrap_or("").to_string();
                let tuning = Tuning::parse(&text);
                if let Some(f) = self.feast.as_mut() {
                    f.tune = tuning.clone();
                }
                let o = self.online_mut()?;
                (o.tuning, o.tuning_text, o.tuning_editable) = (tuning, text, v["editable"].as_bool().unwrap_or(false));
            }
            "cards" => {
                if let Ok(view) = serde_json::from_value::<cards::View>(v["table"].clone()) {
                    self.cards = Some(view);
                }
            }
            "kungfu" => {
                if let Ok(view) = serde_json::from_value::<View>(v) {
                    self.kungfu = Some(view);
                    events::kungfu(self);
                }
            }
            // The hotdogs' reserves and reloads, which frames do not carry (kitchen_sim::magazine).
            "ammo" => {
                if let Ok(stocks) = serde_json::from_value::<Vec<kitchen_sim::magazine::Stock>>(v["hotdogs"].clone()) {
                    if let Some(f) = self.feast.as_mut() {
                        f.restock(&stocks);
                    }
                    self.online_mut()?.stocks = stocks;
                }
            }
            "griddle" => {
                if let (Ok(states), Some(f)) = (serde_json::from_value::<Vec<griddle::State>>(v["griddles"].clone()), self.feast.as_mut()) {
                    f.hear_griddles(&states);
                    events::griddles(self);
                }
            }
            "error" => {
                self.online_mut()?.error = v["message"].as_str().unwrap_or("Disconnected").to_string();
            }
            // The debug encoding: a whole frame as JSON.
            "snapshot" => {
                if self.feast.is_none() {
                    return fail(KsStatus::KS_ERR_NOT_READY, "a frame before the welcome");
                }
                let frame = serde_json::from_value::<Frame>(v).or_else(|e| fail(KsStatus::KS_ERR_MALFORMED, format!("a snapshot that does not read: {e}")))?;
                self.take_frame(frame);
            }
            _ => {}
        }
        Ok(())
    }
    /// A frame from the server, taken in (arena.rs `apply_frame`, loose.rs `apply`, online.rs `mirror` and `settle`).
    fn take_frame(&mut self, frame: Frame) {
        let me = self.me;
        let Game { feast: Some(feast), online: Some(o), .. } = self else {
            return;
        };
        let o = &mut **o;
        // What the server plays by, which this copy plays by too; a new round (Forts: the forcefield up again and
        // everything where the map has it).
        if frame.rules != feast.rules.text() {
            feast.set_rules(Rules::parse(&frame.rules));
        }
        if frame.round != o.round {
            if feast.fort.is_some() && o.round != 0 {
                feast.reset();
            }
            o.round = frame.round;
            o.reset_prediction();
        }
        feast.round = frame.round;
        feast.elapsed = frame.elapsed;
        feast.ends = frame.ends;
        feast.wave_spawned = frame.wave_spawned;
        feast.wave_kills = frame.wave_kills;
        feast.friendly_fire_events = frame.friendly_fire_events;
        feast.famine_at = frame.famine_at;
        feast.warmup = frame.warmup;
        feast.winner = frame.winner;
        feast.alert = frame.alert.clone();
        if let Some(stage) = stage_named(&frame.stage) {
            feast.stage = stage;
        }
        if frame.forts.as_ref().is_some_and(|fort| !fort.field) {
            feast.lower_field();
        }
        // (The bacon where the frame has it: its carrier walks slower, `Feast::burden`, and this copy predicts so.)
        if let (Some(fort), Some(state)) = (feast.fort.as_mut(), frame.forts.as_ref()) {
            fort.bacon = state.bacon.clone();
        }
        feast.pests = frame.pests.clone();
        feast.emergences = frame.emergences.clone();
        feast.enemies = frame.enemies.clone();
        if frame.turrets.len() == feast.turret_yaw.len() {
            feast.turret_yaw = frame.turrets.clone();
        }
        feast.players = frame.players.iter().map(|seat| seat.player.clone()).collect();
        // (The reserves and reloads the frame does not carry.)
        feast.restock(&o.stocks);
        for entry in &frame.props {
            if entry.id >= feast.props.len() {
                continue;
            }
            let mine = feast.props.get_state(entry.id);
            let norm = entry.q.iter().map(|x| x * x).sum::<f64>().sqrt();
            let rotation = if norm > 0.5 { entry.q.map(|x| x / norm) } else { mine.rotation };
            let holder = feast.players.iter().find(|p| p.carry == entry.id as i32).map_or(-1, |p| p.id as i32);
            let theirs = PropState { id: entry.id, shape: mine.shape, state: Held::from_number(entry.state), holder, position: entry.p, rotation, velocity: entry.v, angular_velocity: entry.w };
            if mine.state != theirs.state || mine.position != theirs.position || mine.rotation != theirs.rotation || mine.velocity != theirs.velocity || mine.angular_velocity != theirs.angular_velocity || mine.holder != holder {
                feast.props.set_state(entry.id, &theirs);
            }
        }
        if frame.drapes != feast.drapes {
            feast.set_drapes(frame.drapes.clone());
        }
        for p in &frame.pickups {
            if let Some(existing) = feast.pickups.get_mut(p.id) {
                *existing = p.clone();
            }
        }
        feast.projectiles = frame.projectiles.clone();
        feast.shots = frame.shots.clone();
        feast.splats = frame.splats.clone();
        feast.knocks = frame.knocks.clone();
        // A round that is not Kung Fu forgets the fight.
        let fu = feast.rules.kungfu || feast.level.map.mode == kungfu::MODE;
        // This client's own body: the others where the server has them, its own checked against the server's word.
        mirror(feast, o, me);
        o.ack = frame.ack;
        let alive = feast.player(me).is_some_and(|p| p.alive);
        if o.predict && !fu {
            let ctx = Ctx { me, dt: 1. / o.hz.max(1.), stim: feast.player(me).is_some_and(|p| p.stim > 0.) || o.stim > 0., burden: feast.burden(me) };
            settle(feast, o, frame.ack, frame.mine.filter(|_| alive), ctx);
        }
        o.latest = Some(frame);
        if !fu {
            self.kungfu = None;
        }
        self.rules_text = self.feast.as_ref().map(|f| f.rules.text()).unwrap_or_default();
        events::online(self);
    }

    // ----- Reading

    pub fn match_state(&self) -> Res<KsMatch> {
        let f = self.feast()?;
        let latest = self.online.as_ref().and_then(|o| o.latest.as_ref());
        let stage = latest.and_then(|fr| net::STAGES.iter().position(|s| *s == fr.stage)).map_or(stage_index(f.stage), |i| i as u32);
        // (The mode the round is played in, the rules' or the map's own: a Forts map may be played as a siege.)
        let mode = match f.mode() {
            Mode::Cards => KS_MODE_CARDS,
            Mode::KungFu => KS_MODE_KUNGFU,
            Mode::Ffa => KS_MODE_FFA,
            Mode::Forts => KS_MODE_FORTS,
            Mode::Titan => KS_MODE_TITAN,
            Mode::Siege if f.level.map.mode == kungfu::MODE => KS_MODE_KUNGFU,
            Mode::Siege => KS_MODE_SIEGE,
        };
        let has_forts = match &self.online {
            Some(o) => o.latest.as_ref().is_some_and(|fr| fr.forts.is_some()),
            None => f.fort.is_some(),
        };
        let has_cards = match &self.online {
            Some(_) => self.cards.is_some() && f.rules.cards,
            None => f.cards.is_some(),
        };
        Ok(KsMatch {
            mode,
            sandbox: f.rules.sandbox as u32,
            stage,
            round: f.round,
            elapsed: f.elapsed,
            warmup: f.warmup,
            famine_at: f.famine_at,
            ends: f.ends,
            time_left: f.time_left().unwrap_or(-1.),
            goal: if f.rules.ends() { f.rules.goal() } else { 0 },
            winner: f.winner.unwrap_or(0),
            wave_spawned: f.wave_spawned,
            wave_kills: f.wave_kills,
            wave_size: f.wave(),
            friendly_fire_events: f.friendly_fire_events,
            countdown: latest.and_then(|fr| fr.countdown).unwrap_or(-1.),
            opening: latest.map_or(0., |fr| fr.opening),
            opening_for: latest.map_or(0., |fr| fr.opening_for),
            local_player: self.me,
            players: f.players.len() as u32,
            has_forts: has_forts as u32,
            has_kungfu: self.kungfu.is_some() as u32,
            has_cards: has_cards as u32,
            map_generation: self.generation,
            alert: KsStr::of(&f.alert),
            rules: KsStr::of(&self.rules_text),
        })
    }
    pub fn players(&self) -> Res<Vec<KsPlayer>> {
        let f = self.feast()?;
        let latest = self.online.as_ref().and_then(|o| o.latest.as_ref());
        let shown = self.online.as_ref().filter(|o| o.predict && !self.kungfu_on()).and_then(|o| o.shown(self.me));
        Ok(f.players.iter().map(|p| player(p, self.me, latest.and_then(|fr| fr.players.iter().find(|s| s.player.id == p.id)), self.online.is_some(), shown)).collect())
    }
    pub fn enemies(&self) -> Res<Vec<KsEnemy>> {
        Ok(self.feast()?.enemies.iter().map(|e| KsEnemy { id: e.id, p: e.p, yaw: e.yaw, hp: e.hp, born: e.born, forming: e.forming }).collect())
    }
    pub fn projectiles(&self) -> Res<Vec<KsProjectile>> {
        Ok(self
            .feast()?
            .projectiles
            .iter()
            .map(|q| KsProjectile { id: q.id, owner: q.owner, weapon: q.weapon as u32, hostile: q.hostile as u32, p: q.p, v: q.v, travelled: q.travelled, born: q.born, from: q.from, launch: q.launch })
            .collect())
    }
    pub fn props(&self) -> Res<Vec<KsProp>> {
        let f = self.feast()?;
        let w = &f.props;
        let f3 = |v: [f64; 3]| v.map(|x| x as f32);
        Ok((0..w.len())
            .map(|id| {
                let b = w.body(id);
                KsProp { id: id as u32, kind: b.shape as u32, state: w.state(id) as u32, holder: b.holder, p: f3(w.position(id)), q: w.rotation(id).map(|x| x as f32), v: f3(w.velocity(id)), w: f3(w.angular_velocity(id)) }
            })
            .collect())
    }
    pub fn pickups(&self) -> Res<Vec<KsPickup>> {
        let f = self.feast()?;
        Ok(f.pickups.iter().map(|p| KsPickup { id: p.id as u32, weapon: p.weapon as u32, ammo: p.ammo, available: (p.ready <= f.elapsed) as u32, p: p.p, ready: p.ready, respawn: p.respawn }).collect())
    }
    pub fn turrets(&self) -> Res<Vec<KsTurret>> {
        let f = self.feast()?;
        let posts: Vec<([f64; 2], f64)> = f.level.map.props.iter().filter_map(|p| if let Prop::Turret { p, base, .. } = p { Some((*p, *base)) } else { None }).collect();
        Ok(posts
            .iter()
            .enumerate()
            .map(|(i, (p, base))| {
                let beam = if self.online.is_none() { f.turrets.get(i).copied() } else { None };
                KsTurret {
                    post: p.map(|v| v as f32),
                    base: *base as f32,
                    yaw: f.turret_yaw.get(i).copied().unwrap_or(0.),
                    origin: beam.map_or([0.; 3], |b| b.0.to_array()),
                    direction: beam.map_or([0.; 3], |b| b.1.to_array()),
                }
            })
            .collect())
    }
    pub fn swarms(&self) -> Res<Vec<KsSwarm>> {
        Ok(self.feast()?.pests.iter().map(|s| KsSwarm { id: s.id, p: s.p, hp: s.hp, born: s.born, empty: s.empty }).collect())
    }
    pub fn puddles(&self) -> Res<Vec<KsPuddle>> {
        Ok(self
            .feast()?
            .emergences
            .iter()
            .flat_map(|e| {
                e.puddles.iter().map(move |p| KsPuddle {
                    emergence: e.id,
                    started: e.started,
                    ends: e.ends,
                    spawn: p.spawn as u32,
                    p: p.p,
                    source: p.source,
                    arrive: p.arrive,
                    until: p.until,
                    count: p.count,
                })
            })
            .collect())
    }
    pub fn drapes(&self) -> Res<Vec<KsDrape>> {
        Ok(self.feast()?.drapes.iter().map(|d| KsDrape { id: d.id, napkin: d.napkin as u32, props: d.props.len() as u32, rect: d.rect.map(|v| v as f32), top: d.top as f32 }).collect())
    }
    pub fn griddles(&self) -> Res<Vec<KsGriddle>> {
        let f = self.feast()?;
        Ok(f.griddles
            .iter()
            .filter_map(|g| {
                let plate = Plate::of(f.level.map.blocks.get(g.block)?);
                Some(KsGriddle { block: g.block as u32, on: g.on as u32, temp: g.temp, burn_rate: griddle::burn_rate(g.temp), by: g.by, rect: plate.rect, top: plate.top, dial: plate.dial, front: plate.front })
            })
            .collect())
    }
    pub fn forts(&self) -> Res<KsForts> {
        let f = self.feast()?;
        let (state, reason) = match &self.online {
            Some(o) => match o.latest.as_ref().and_then(|fr| fr.forts.as_ref()) {
                Some(s) => (s.clone(), KsStr::of(&s.reason)),
                None => return fail(KsStatus::KS_ERR_NOT_FOUND, "not a Forts game"),
            },
            None => match (f.fort_state(), f.fort.as_ref()) {
                (Some(s), Some(fort)) => (s, KsStr::of(&fort.reason)),
                _ => return fail(KsStatus::KS_ERR_NOT_FOUND, "not a Forts game"),
            },
        };
        let zones = f.level.map.fortress();
        let r4 = |r: [f64; 4]| r.map(|v| v as f32);
        Ok(KsForts {
            scores: state.scores,
            rounds: state.rounds,
            match_no: state.match_no,
            build: state.build,
            build_ends: state.ends,
            field: state.field as u32,
            bacon_state: state.bacon.state as u32,
            bacon_holder: state.bacon.holder,
            bacon_p: state.bacon.p,
            bacon_idle: state.bacon.idle,
            stale: state.stale,
            alarms: state.alarms,
            winner: state.winner.unwrap_or(-2),
            champion: state.champion.map_or(-1, |c| c as i32),
            reason,
            axis: zones.as_ref().map_or(0, |z| z.axis as u32),
            mid: zones.as_ref().map_or(0., |z| z.mid as f32),
            field_rect: zones.as_ref().map_or([0.; 4], |z| r4(z.field)),
            bases: zones.as_ref().map_or([[0.; 4]; 2], |z| [r4(z.bases[0]), r4(z.bases[1])]),
            sides: zones.as_ref().map_or([0; 2], |z| z.sides),
            bacon_spot: zones.as_ref().map_or([0.; 2], |z| z.bacon.map(|v| v as f32)),
        })
    }
    pub fn cards_json(&mut self) -> Res<KsStr> {
        let view = match &self.online {
            Some(_) => self.cards.clone(),
            None => self.feast()?.cards.as_ref().map(|t| t.view()),
        };
        let Some(view) = view else {
            return fail(KsStatus::KS_ERR_NOT_FOUND, "no card table");
        };
        self.cache = serde_json::to_string(&view).unwrap_or_default();
        Ok(KsStr::of(&self.cache))
    }
    /// The round's clock Kung Fu is read at.
    fn fu_clock(&self) -> f32 {
        let elapsed = self.feast.as_ref().map_or(0., |f| f.elapsed);
        self.kungfu.as_ref().map_or(elapsed, |v| v.elapsed.max(elapsed))
    }
    pub fn kungfu_state(&self) -> Res<KsKungfu> {
        let view = self.kungfu.as_ref().ok_or_else(|| Fail { status: KsStatus::KS_ERR_NOT_FOUND, message: "not Kung Fu".into() })?;
        Ok(KsKungfu { clock: self.fu_clock(), goal: view.goal, ends: view.ends, fighters: view.fighters.len() as u32 })
    }
    pub fn fighters(&self) -> Res<Vec<KsFighter>> {
        let view = self.kungfu.as_ref().ok_or_else(|| Fail { status: KsStatus::KS_ERR_NOT_FOUND, message: "not Kung Fu".into() })?;
        let clock = self.fu_clock();
        let alive = |id: u32| self.feast.as_ref().and_then(|f| f.player(id)).is_none_or(|p| p.alive);
        Ok(view.fighters.iter().map(|fv| events::fighter(fv, clock, alive(fv.id))).collect())
    }
    pub fn client_status(&self) -> Res<KsClientStatus> {
        let Some(o) = self.online.as_deref() else {
            return fail(KsStatus::KS_ERR_WRONG_MODE, "an online call on an offline game");
        };
        Ok(KsClientStatus {
            welcomed: o.welcomed as u32,
            local_player: self.me,
            tick_hz: o.hz,
            send_hz: o.send_hz,
            newest_tick: o.newest,
            acked_command: o.ack,
            command_seq: self.seq,
            pending_commands: self.seq.saturating_sub(o.ack),
            frames: o.frames,
            broken: o.broken,
            whole: o.whole,
            predicting: (o.predict && !self.kungfu_on() && o.ghosts.get(self.me).is_some()) as u32,
            corrections: o.corrections,
            snaps: o.snaps,
            offset: o.offset.length(),
            error: KsStr::of(&o.error),
            leader: o.latest.as_ref().map_or(0, |fr| fr.leader),
            choice_random: o.latest.as_ref().is_some_and(|fr| fr.choice.random) as u32,
            choice_name: o.latest.as_ref().map_or(KsStr::EMPTY, |fr| KsStr::of(&fr.choice.name)),
            choice_title: o.latest.as_ref().map_or(KsStr::EMPTY, |fr| KsStr::of(&fr.choice.title)),
            choice_mode: o.latest.as_ref().map_or(KsStr::EMPTY, |fr| KsStr::of(&fr.choice.mode)),
            choice_theme: o.latest.as_ref().map_or(KsStr::EMPTY, |fr| KsStr::of(&fr.choice.theme)),
            tuning: KsStr::of(&o.tuning_text),
            tuning_editable: o.tuning_editable as u32,
        })
    }
}

/// A stage by the name a frame gives it (the lobby's two are not the round's).
fn stage_named(name: &str) -> Option<Stage> {
    Some(match name {
        "Warmup" => Stage::Warmup,
        "Wave" => Stage::Wave,
        "FreeForAll" => Stage::FreeForAll,
        "RoundOver" => Stage::RoundOver,
        "Build" => Stage::Build,
        "Fight" => Stage::Fight,
        _ => return None,
    })
}

/// A hotdog as the interface tells it; `seat` is the frame's (online), `shown` where this client draws its own.
fn player(p: &Player, me: u32, seat: Option<&net::Seat>, online: bool, shown: Option<Vec3>) -> KsPlayer {
    let mut flags = 0;
    let local = p.id == me;
    for (on, bit) in [
        (p.alive, KS_PLAYER_ALIVE),
        (p.bot, KS_PLAYER_BOT),
        (if online { seat.is_some_and(|s| s.human) } else { !p.bot }, KS_PLAYER_HUMAN),
        (local, KS_PLAYER_LOCAL),
        (p.consumed, KS_PLAYER_CONSUMED),
        (p.infected, KS_PLAYER_INFECTED),
        (p.aimed, KS_PLAYER_AIMED),
        (p.grounded, KS_PLAYER_GROUNDED),
        (p.launched, KS_PLAYER_LAUNCHED),
        (p.fallen, KS_PLAYER_FALLEN),
        (p.ready, KS_PLAYER_READY),
        (seat.is_some_and(|s| s.waiting), KS_PLAYER_WAITING),
        (local && p.alive && shown.is_some(), KS_PLAYER_PREDICTED),
        (p.benched, KS_PLAYER_BENCHED),
    ] {
        if on {
            flags |= bit;
        }
    }
    let at = match shown.filter(|_| local && p.alive) {
        Some(v) => v.to_array(),
        None => p.p,
    };
    let d = &p.last_damage;
    KsPlayer {
        id: p.id,
        team: p.team as u32,
        flags,
        stance: if p.prone { KS_STANCE_PRONE } else if p.crouch { KS_STANCE_CROUCH } else { KS_STANCE_STAND },
        p: at,
        v: p.v,
        yaw: p.yaw,
        pitch: p.pitch,
        hp: p.hp,
        weapon: p.weapon as u32,
        gun: p.gun as u32,
        owned: p.owned.iter().enumerate().fold(0, |bits, (w, on)| if *on { bits | 1 << w } else { bits }),
        ammo: p.ammo,
        carry: p.carry,
        carry_pose: p.pose.bits() as u32,
        score: p.score,
        kills: p.kills,
        deaths: p.deaths,
        meals: p.meals,
        swings: p.swings,
        stim: p.stim,
        stim_wait: p.stim_wait,
        respawn: p.respawn,
        shield: p.shield,
        talk: seat.map_or(0., |s| if s.human { s.talk } else { 0. }),
        name: KsStr::of(&p.name),
        color: seat.map_or(KsStr::EMPTY, |s| KsStr::of(&s.color)),
        last_damage: KsDamage { kind: d.kind as u32, attacker: d.attacker, p: d.p, d: d.d, time: d.time, amount: d.amount },
    }
}

/// One command of a person's applied to its hotdog, as the server applies it (crates/server game.rs `Game::command`):
/// in Kung Fu the whole command to `Feast::kungfu_command`; otherwise the stances, the stim, the walk at the pace the
/// hotdog has, the jump, the pitch, the gun, the aim, the pose of what it carries (or with the hands free a reload, R:
/// `button::RELOAD`), then the pickup, eating, the melee strike, picking up or putting down, the throw and the trigger.
/// The simulation refuses whatever cannot be done.
pub fn apply_command(f: &mut Feast, id: u32, c: &Command, dt: f32) {
    if f.cards.is_some() || !f.player(id).is_some_and(|p| p.alive) {
        return;
    }
    if f.kungfu.is_some() {
        return f.kungfu_command(id, c, dt);
    }
    let (prone, crouch) = (c.has(button::PRONE), c.has(button::CROUCH) && !c.has(button::PRONE));
    f.set_prone(id, prone);
    f.set_crouch(id, crouch);
    if c.has(button::STIM) {
        f.stim(id);
    }
    let stim = f.player(id).is_some_and(|p| p.stim > 0.);
    let control = if f.body(id).is_some_and(|body| body.launched) { f.bodies.config.control as f32 } else { 1. };
    let step = c.stride(f.tune.pace(prone, crouch, stim, f.burden(id)) * control, dt);
    f.walk(id, step, c.yaw());
    if c.has(button::JUMP) && !prone {
        f.jump(id, f.tune.leap(stim));
    }
    f.look(id, c.pitch());
    if (1..feast::GUNS).contains(&(c.weapon as usize)) {
        f.select(id, c.weapon as usize);
    }
    f.set_aimed(id, c.has(button::SCOPE));
    if f.player(id).is_some_and(|p| p.carry >= 0) {
        f.set_held_pose(id, c.pose());
    } else if c.reloads() {
        f.reload(id);
    }
    let direction = c.direction();
    if let Some(item) = c.pickup {
        f.pickup(id, item as usize);
    }
    if c.has(button::EAT) {
        f.eat(id);
    }
    if c.has(button::MELEE) {
        f.melee(id, direction);
    }
    if c.has(button::GRAB) {
        if f.player(id).is_some_and(|p| p.carry >= 0) {
            f.put(id, direction);
        } else {
            f.grab(id, direction);
        }
    }
    if c.has(button::THROW) {
        f.toss(id, direction);
    }
    if c.has(button::FIRE) {
        let weapon = f.player(id).map_or(0, |p| p.weapon);
        f.fire(id, direction, weapon);
    }
}

// ----- The prediction (src/arena/online.rs, without Kung Fu's foresight)

/// One command applied to this client's own body (`ctx.me` in the ghosts), as the server will: the stances, the walk at
/// the pace they and the stim and the load allow (a quarter of it while thrown), the jump, the step. The props are left
/// as the newest frame has them (`rehearse`): they are the server's, not moved here. Returns the body afterwards.
fn apply_predicted(feast: &mut Feast, ghosts: &mut actor::World, c: &Command, ctx: Ctx) -> Option<Actor> {
    let body = ghosts.get(ctx.me)?;
    let (prone, crouch) = (c.has(button::PRONE), c.has(button::CROUCH) && !c.has(button::PRONE));
    let control = if body.launched { ghosts.config.control as f32 } else { 1. };
    let step = c.stride(feast.tune.pace(prone, crouch, ctx.stim, ctx.burden) * control, ctx.dt);
    let jump = if c.has(button::JUMP) && !prone { feast.tune.leap(ctx.stim) as f64 } else { 0. };
    feast.rehearse(ctx.me, |feast| feast.predict(ghosts, ctx.me, (prone, crouch), step, jump, ctx.dt as f64, false));
    ghosts.get(ctx.me).cloned()
}
/// online.rs `Predictor::step`: a command applied and kept with the body after it. Returns where the body stands.
fn predict_step(feast: &mut Feast, o: &mut Online, c: &Command, ctx: Ctx) -> Option<Vec3> {
    let after = apply_predicted(feast, &mut o.ghosts, c, ctx)?;
    let at = after.position();
    o.pending.push_back((*c, after));
    Some(at)
}
/// Whether a predicted body is where the server says, standing or not and thrown or not alike.
fn agrees(p: &Actor, m: &Mine) -> bool {
    let far = (0..3).map(|k| (p.p[k] - m.p[k] as f64).powi(2)).sum::<f64>().sqrt();
    far < AGREE && p.grounded == m.grounded && p.launched == m.launched && (p.v[1] - m.vy as f64).abs() < 0.05 && (0..2).all(|k| (p.knock[k] - m.knock[k] as f64).abs() < 0.05)
}
/// online.rs `Predictor::reconcile`: when the prediction for `ack` disagrees with `mine`, the body is put where the
/// server says and the commands since are applied again. Returns how far that moved the body.
fn reconcile(feast: &mut Feast, o: &mut Online, ack: u32, mine: &Mine, ctx: Ctx) -> Option<Vec3> {
    while o.pending.front().is_some_and(|(c, _)| c.seq < ack) {
        o.pending.pop_front();
    }
    let body = o.ghosts.get_mut(ctx.me)?;
    let acked = o.pending.front_mut().filter(|(c, _)| c.seq == ack);
    if acked.as_ref().is_some_and(|(_, p)| agrees(p, mine)) {
        return None;
    }
    let before = body.position();
    mine.onto(body);
    if let Some((_, after)) = acked {
        *after = body.clone();
    }
    let all: Vec<Command> = o.pending.iter().map(|(c, _)| *c).collect();
    let first = all.iter().position(|c| c.seq > ack).unwrap_or(all.len());
    let ghosts = &mut o.ghosts;
    let afters: Vec<Actor> = (first..all.len()).filter_map(|k| apply_predicted(feast, ghosts, &all[k], ctx)).collect();
    for ((_, after), redone) in o.pending.iter_mut().filter(|(c, _)| c.seq > ack).zip(afters) {
        *after = redone;
    }
    o.corrections += 1;
    let now = o.ghosts.get(ctx.me)?.position();
    Some(now - before)
}
/// online.rs `settle`: a frame says the server applied this client's commands up to `ack` and its body was `mine`
/// afterwards (None while it has none: dead, between rounds).
fn settle(feast: &mut Feast, o: &mut Online, ack: u32, mine: Option<Mine>, ctx: Ctx) {
    let me = ctx.me;
    let Some(mine) = mine else {
        o.ghosts.remove(me);
        o.pending.clear();
        return;
    };
    if o.ghosts.get(me).is_none() {
        // A body to predict again: where the server has it, and nothing pending counts.
        let mut body = Actor { radius: feast::HOTDOG_RADIUS, ..Actor::hotdog(me, [0.; 3]) };
        mine.onto(&mut body);
        o.ghosts.add(body);
        o.pending.retain(|(c, _)| c.seq > ack);
        let at = Vec3::from_array(mine.p);
        (o.prev, o.cur, o.offset) = (at, at, Vec3::ZERO);
        let commands: Vec<Command> = o.pending.drain(..).map(|(c, _)| c).collect();
        for c in commands {
            if let Some(at) = predict_step(feast, o, &c, ctx) {
                (o.prev, o.cur) = (at, at);
            }
        }
        return;
    }
    let Some(moved) = reconcile(feast, o, ack, &mine, ctx) else {
        return;
    };
    o.prev += moved;
    o.cur += moved;
    if (o.offset - moved).length() > SNAP {
        o.offset = Vec3::ZERO;
        o.snaps += 1;
    } else {
        o.offset -= moved;
    }
}
/// arena.rs `mirror`: this client's copy of the bodies from the newest frame: everyone else where the server has them,
/// its own kept (it is ahead of the server).
fn mirror(feast: &Feast, o: &mut Online, me: u32) {
    let mine = o.ghosts.get(me).cloned();
    o.ghosts.actors.clear();
    o.ghosts.config = feast.bodies.config.clone();
    let at = |p: [f32; 3]| p.map(|v| v as f64);
    if let Some(mine) = mine {
        o.ghosts.add(mine);
    }
    for p in feast.players.iter().filter(|p| p.alive && p.id != me) {
        let mut body = Actor { radius: feast::HOTDOG_RADIUS, ..Actor::hotdog(p.id, at(p.p)) };
        (body.prone, body.crouch, body.grounded, body.v[1]) = (p.prone, p.crouch, p.grounded, p.v[1] as f64);
        o.ghosts.add(body);
    }
    for e in feast.enemies.iter().filter(|e| e.forming <= 0.) {
        o.ghosts.add(Actor::zombie(feast::CAT + e.id, at(e.p)));
    }
}

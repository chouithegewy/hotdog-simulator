// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Replays (kitchen_sim::replay): watching a recorded game again, and recording single player's.
//!
//! **The viewer** is the game run as a client of a recording instead of a server (`--replay <file>`, `?replay=<id, url
//! or local:name>`). The recording is the stream a client was sent, so it is played through the online client's own
//! code: its frames and texts are handed to network.rs as the socket's would be (`Network::replay`), and arena.rs
//! builds the world from them exactly as online. What differs is the clock: the online client draws what the server
//! said a little in the past at a clock that follows the frames' arrival; here the clock is the replay's own
//! (`Viewer::clock`), at the speed it is watched at (0.25x, 0.5x, 1x, 2x, paused), and drives the timeline
//! (online.rs `Timeline::driven`), the round's own clocks and the falls (slowmo.rs `SlowMo::replay`), and the sounds'
//! pitch. The frames are handed over a little ahead of the clock (LEAD), as a server's arrive ahead of what is drawn.
//!
//! A seek decodes the chunk that holds the moment from its keyframe (kitchen_sim::replay `Playhead::seek`): the world
//! is cleared as for a new round (network.rs `rewind`, online.rs `rewind`), the chunk's context (the latest card
//! table, griddles, Kung Fu, reserves and tuning) is applied, and its frames up to the moment are played at once with
//! their shots and splats passed over (`Timeline::hush`), so nothing depends on what was played before.
//!
//! **Perspectives** (`Mode`): first person from any player's eyes, with that player's own head-up display; following a
//! player from behind; or a free camera that flies anywhere, off the counter too. In first person a player whose own
//! view the replay holds (pov.rs) is seen exactly as that player saw it (RECORDED VIEW): the camera as it was drawn,
//! the gun in hand as it was, the scope, the display, the indicators, the slow motion's look. Otherwise (bots, older
//! files, players who did not share their view) the view is worked out from the recording (RECONSTRUCTED): at the
//! eyes the frames give, the gun in hand kicking at the shots the frames tell of. **Stats** come from the recording's
//! tally (kitchen_sim::replay `Tally`) at the clock. The screen: ui.rs; the keys: controls.rs, the REPLAY and REPLAY
//! CAMERA tabs.
//!
//! **Recording** single player: record.rs. **The list** of replays (the Esc menu's REPLAYS): list.rs. **Where** they
//! are kept: store.rs.
pub mod list;
pub mod pov;
pub mod record;
pub mod store;
mod ui;

use crate::arena::{Game, Weapon};
use crate::controls::{Action, Bindings};
use crate::network::{Incoming, Network};
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use kitchen_sim::net::{self, Frame};
use kitchen_sim::replay::{Mark, Marker, Playhead, Played, Record, Replay};
use std::collections::VecDeque;

/// The speeds a replay is watched at.
pub const SPEEDS: [f32; 4] = [0.25, 0.5, 1., 2.];
/// How far ahead of the clock frames are handed to the client (ms): the next frame is always there to draw towards.
pub const LEAD: u32 = 100;
/// What a seek's keys jump (ms).
pub const JUMP: u32 = 5000;
/// The pace the round goes at while paused: next to nothing, never nothing (a step's sums divide by it).
const STILL: f32 = 1e-3;
/// The eye's height over the feet standing (arena.rs EYE), crouched and prone.
const EYE: f32 = 0.135;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let options = app.world().get_resource::<crate::runtime::Options>().cloned().unwrap_or_default();
        store::configure(&options.replays_dir);
        app.init_resource::<list::List>().init_resource::<record::Recorder>().add_systems(Update, (record::sample.after(crate::arena::tick), list::update, record::dot.after(crate::arena::tick)));
        // This player's own view, kept once the frame is drawn (pov.rs).
        app.init_resource::<pov::Pov>().init_resource::<pov::Shown>().add_systems(Last, pov::sample);
        if options.replay.is_empty() {
            return;
        }
        app.insert_resource(Viewer::new(&options.replay))
            .add_systems(Update, feed.before(crate::network::receive))
            .add_systems(Update, pov::restore.after(crate::network::receive).before(crate::arena::tick))
            .add_systems(
                Update,
                (camera, ui::draw)
                    .chain()
                    .after(crate::arena::tick)
                    .after(crate::network::send)
                    .after(crate::deathcam::watch)
                    .after(crate::kungfu::view::update)
                    .before(crate::slowmo::present)
                    .before(crate::hud::update)
                    .before(crate::damage::update)
                    .before(crate::scope::optic)
                    .before(crate::reload::follow),
            )
            .add_systems(Update, sound.after(crate::slowmo::present))
            .add_systems(PostUpdate, ui::place.after(bevy::transform::TransformSystems::Propagate).before(bevy::ui::UiSystems::Content))
            // A recorded view: the camera and the gun exactly where they were drawn, after anything else has moved them
            // (the slow motion's push, the lobby), before the camera's projection and the transforms are worked out.
            .add_systems(Update, recorded.after(crate::slowmo::present))
            .add_systems(PostUpdate, recorded.after(crate::lobby::update).before(bevy::transform::TransformSystems::Propagate).before(bevy::camera::CameraUpdateSystems));
    }
}

/// How the replay is seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Through the watched player's eyes, with its head-up display.
    #[default]
    First,
    /// Behind and over the watched player.
    Follow,
    /// A camera that flies anywhere.
    Fly,
}
impl Mode {
    pub fn next(self) -> Mode {
        match self {
            Mode::First => Mode::Follow,
            Mode::Follow => Mode::Fly,
            Mode::Fly => Mode::First,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Mode::First => "FIRST PERSON",
            Mode::Follow => "FOLLOW",
            Mode::Fly => "FREE CAMERA",
        }
    }
}
/// What is on the screen besides the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Show {
    /// Everything (the viewer's own and the game's head-up display): off for clean footage.
    pub ui: bool,
    pub stats: bool,
    pub help: bool,
    pub tags: bool,
    pub numbers: bool,
    pub tracers: bool,
    pub minimap: bool,
}
impl Default for Show {
    fn default() -> Show {
        Show { ui: true, stats: false, help: false, tags: false, numbers: true, tracers: false, minimap: true }
    }
}
/// The free camera: where it is, where it looks and how fast it flies (m/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fly {
    pub at: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
}
impl Default for Fly {
    fn default() -> Fly {
        Fly { at: Vec3::new(0., 1.5, 3.), yaw: 0., pitch: -0.5, speed: 0.6 }
    }
}
/// The camera that follows: turned about the player from behind it, raised, and how far back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}
impl Default for Orbit {
    fn default() -> Orbit {
        Orbit { yaw: 0., pitch: -0.32, distance: 0.55 }
    }
}
/// A damage number floating over a hotdog, a shot's tracer.
#[derive(Clone, Debug)]
pub struct Number {
    pub ms: f64,
    pub to: u32,
    pub amount: f32,
    pub at: Vec3,
}
#[derive(Clone, Debug)]
pub struct Tracer {
    pub ms: f64,
    pub a: Vec3,
    pub b: Vec3,
    pub weapon: usize,
}
/// The replay being watched (see the module's documentation).
#[derive(Resource)]
pub struct Viewer {
    pub source: String,
    loading: Option<store::Loading>,
    /// Why it cannot be played, when it cannot.
    pub failed: Option<String>,
    pub replay: Option<Replay>,
    /// What is handed to the client (LEAD ahead of the clock), and what has been played at the clock (the stats, the
    /// overlays).
    ahead: Option<Playhead>,
    pub now: Option<Playhead>,
    /// Milliseconds into the recording.
    pub clock: f64,
    pub speed: usize,
    pub paused: bool,
    pub mode: Mode,
    pub watched: u32,
    pub fly: Fly,
    pub orbit: Orbit,
    pub show: Show,
    /// A word about the file (another build, cut short), shown at the top.
    pub warning: Option<String>,
    /// The step each frame handed over was taken at, by its time: the timeline's clock is worked out from them.
    ticks: VecDeque<(u32, u32)>,
    pub hz: f32,
    pub send_hz: f32,
    /// The map the client has built, and the reserves' text last handed over (heard again when the watched player
    /// changes).
    map: String,
    ammo: Option<String>,
    /// The newest frame played at the clock, and its markers so far.
    pub frame: Option<Frame>,
    pub numbers: VecDeque<Number>,
    pub tracers: VecDeque<Tracer>,
    /// The eye's height as drawn (eased between standing, crouched and prone).
    eye: f32,
    /// A seek asked for (ms), and when the last was done (real seconds): a drag on the timeline seeks a few times a
    /// second, not every frame.
    pub seek: Option<f64>,
    last_seek: f64,
    pub seeks: u32,
    /// The pointer's place last frame while it drags the view round.
    drag: Option<Vec2>,
    /// The end has been reached (playing again starts from the beginning).
    pub ended: bool,
    /// The players' own views (pov.rs), and the view at the clock when the replay is watched through one.
    pub views: pov::Views,
    /// The reconstructed view: the watched player's shots and swings the frames at the clock told of, to be shown.
    kicks: Vec<usize>,
    blows: Vec<usize>,
    swings: Option<(u32, u32)>,
}
impl Viewer {
    pub fn new(source: &str) -> Viewer {
        Viewer {
            source: source.into(),
            loading: None,
            failed: None,
            replay: None,
            ahead: None,
            now: None,
            clock: 0.,
            speed: 2,
            paused: false,
            mode: Mode::First,
            watched: 0,
            fly: Fly::default(),
            orbit: Orbit::default(),
            show: Show::default(),
            warning: None,
            ticks: VecDeque::new(),
            hz: net::TICK_HZ as f32,
            send_hz: net::SEND_HZ as f32,
            map: String::new(),
            ammo: None,
            frame: None,
            numbers: VecDeque::new(),
            tracers: VecDeque::new(),
            eye: EYE,
            seek: None,
            last_seek: -1.,
            seeks: 0,
            drag: None,
            ended: false,
            views: pov::Views::default(),
            kicks: vec![],
            blows: vec![],
            swings: None,
        }
    }
    /// Whether the replay is watched through a player's recorded view this frame.
    pub fn recorded(&self) -> bool {
        self.views.now.is_some()
    }
    /// How long the replay is (ms).
    pub fn duration(&self) -> f64 {
        self.replay.as_ref().map_or(0., |r| r.duration() as f64)
    }
    /// The pace the round goes at: the speed, or next to nothing paused.
    pub fn pace(&self) -> f32 {
        if self.paused { STILL } else { SPEEDS[self.speed] }
    }
    /// Every marker of the replay.
    pub fn markers(&self) -> Vec<Marker> {
        self.replay.as_ref().map_or(vec![], |r| r.markers().copied().collect())
    }
    /// The players there are to watch at the clock, in the order the frames list them (those waiting for the next
    /// round are not playing, and are left out).
    pub fn players(&self) -> Vec<u32> {
        self.frame.as_ref().map_or(vec![], |f| f.players.iter().filter(|s| !s.waiting).map(|s| s.player.id).collect())
    }
    /// The step of the server's the clock is at (worked out from the frames handed over).
    pub fn tick_at(&self, ms: f64) -> f64 {
        let at = self.ticks.iter().rev().find(|(m, _)| *m as f64 <= ms).or(self.ticks.front());
        at.map_or(0., |(m, t)| *t as f64 + (ms - *m as f64) * self.hz as f64 / 1000.)
    }
    /// The kill marker after (or before) the clock, by `way`.
    pub fn kill(&self, way: i32) -> Option<f64> {
        let kills = self.markers().into_iter().filter(|m| m.kind != Mark::RoundEnd).map(|m| m.ms as f64);
        if way > 0 { kills.filter(|ms| *ms > self.clock + 250.).reduce(f64::min) } else { kills.filter(|ms| *ms < self.clock - 1500.).reduce(f64::max) }
    }
}

/// Before the client reads what came: loads the replay, hears the viewer's keys, moves the clock on, seeks, and hands
/// the client the records up to LEAD past the clock.
pub fn feed(world: &mut World) {
    let real = world.resource::<Time<Real>>().delta_secs().min(0.1) as f64;
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let Some(mut v) = world.remove_resource::<Viewer>() else { return };
    if v.replay.is_none() && v.failed.is_none() {
        load(world, &mut v);
    }
    if v.replay.is_some() {
        let menu = world.get_resource::<crate::menu::Menu>().is_some_and(|m| m.open) || world.get_resource::<list::List>().is_some_and(|l| l.open);
        if !menu {
            keys(world, &mut v);
        }
        if !v.paused && !menu {
            v.clock += real * SPEEDS[v.speed] as f64 * 1000.;
        }
        let end = v.duration();
        if v.clock >= end {
            (v.clock, v.paused, v.ended) = (end, true, true);
        }
        if let Some(to) = v.seek
            && now - v.last_seek > 0.12
        {
            v.seek = None;
            v.last_seek = now;
            seek(world, &mut v, to);
        }
        advance(world, &mut v);
        steer(world, &mut v, real as f32);
        // Through a player's recorded view: what the presentation reads is put back before the round's frame (pov.rs).
        let (through, seeks, clock, watched) = (v.mode == Mode::First, v.seeks, v.clock, v.watched);
        let Viewer { replay, views, .. } = &mut v;
        if let Some(replay) = replay.as_ref() {
            pov::before(world, views, replay, watched, clock, through, seeks);
        }
    }
    world.insert_resource(v);
}
/// Loads the replay (natively at once, in a browser when its file has come) and plays it from the start.
fn load(world: &mut World, v: &mut Viewer) {
    let loading = v.loading.get_or_insert_with(|| store::fetch(&v.source.clone()));
    let Some(done) = store::poll(loading) else { return };
    let replay = done.and_then(|bytes| Replay::parse(bytes).map_err(|e| e.to_string()));
    let replay = match replay {
        Ok(replay) if replay.chunks.is_empty() => Err("The replay holds no part of a game: it was cut short before its first seconds.".to_string()),
        other => other,
    };
    match replay {
        Ok(replay) => {
            let h = &replay.header;
            (v.hz, v.send_hz) = (h.tick_hz.max(1) as f32, h.send_hz.max(1) as f32);
            let mut words = vec![];
            if let Some(w) = replay.warning(store::BUILD) {
                words.push(w);
            }
            if replay.truncated {
                words.push(format!("The recording was cut short: it plays up to {}.", ui::time(replay.duration() as f64)));
            }
            v.warning = (!words.is_empty()).then(|| words.join(" "));
            v.watched = h.roster.iter().find(|m| !m.bot).or(h.roster.first()).map_or(1, |m| m.id);
            v.views = pov::Views::of(&replay);
            // (A player whose own view it holds is the one watched first.)
            if let Some(id) = h.roster.iter().map(|m| m.id).find(|id| v.views.has(&replay, *id)) {
                v.watched = id;
            }
            info!("Replay: {} on {}, {} s{}", h.mode, h.map_name, replay.duration() / 1000, v.warning.as_ref().map_or(String::new(), |w| format!(" ({w})")));
            v.replay = Some(replay);
            store::hold_keys();
            // The client begins as it would with a server: welcomed, on the replay's map.
            let map = v.replay.as_ref().unwrap().header.map.clone();
            let welcome = serde_json::json!({"type": "welcome", "id": v.watched, "version": net::PROTOCOL, "map": map, "tick": v.hz, "send": v.send_hz}).to_string();
            v.map = map;
            world.resource_mut::<Network>().replay = Some(VecDeque::from([Incoming::Text(welcome)]));
            seek(world, v, 0.);
        }
        Err(error) => {
            warn!("Replay {}: {error}", v.source);
            crate::level::notice(&error);
            v.failed = Some(error);
        }
    }
}
/// Plays from `to` (ms): see the module's documentation.
pub fn seek(world: &mut World, v: &mut Viewer, to: f64) {
    let Some(replay) = v.replay.as_ref() else { return };
    let to = to.clamp(0., replay.duration() as f64);
    let (ahead, now) = match (Playhead::seek(replay, to as u32 + LEAD), Playhead::seek(replay, to as u32)) {
        (Ok(a), Ok(n)) => (a, n),
        (Err(e), _) | (_, Err(e)) => {
            v.failed = Some(e.to_string());
            return;
        }
    };
    v.clock = to;
    v.ended = false;
    v.seeks += 1;
    v.ticks.clear();
    v.numbers.clear();
    v.tracers.clear();
    world.resource_mut::<Network>().rewind();
    crate::arena::online::rewind(&mut world.resource_mut::<Game>());
    let (head, played) = ahead;
    v.ahead = Some(head);
    let mut records = vec![];
    for p in played {
        if let Some(r) = hand(v, p) {
            records.push(r);
        }
    }
    // What came before the moment is shown as it stands, not happening: its shots and splats are passed over.
    let hush = v.tick_at(to - 1000. / v.send_hz as f64).max(0.) as u32;
    world.resource_mut::<Game>().online.timeline.hush = hush;
    if let Some(stream) = world.resource_mut::<Network>().replay.as_mut() {
        stream.extend(records);
    }
    let (head, played) = now;
    v.now = Some(head);
    watch(v, played, true);
}
/// Moves the playheads on to the clock: the client is handed what is LEAD ahead of it, the overlays have what is at it.
fn advance(world: &mut World, v: &mut Viewer) {
    let (Some(replay), Some(mut ahead), Some(mut now)) = (v.replay.take(), v.ahead.take(), v.now.take()) else { return };
    let records: Vec<Incoming> = ahead.advance(&replay, v.clock as u32 + LEAD).into_iter().filter_map(|p| hand(v, p)).collect();
    if let Some(stream) = world.resource_mut::<Network>().replay.as_mut() {
        stream.extend(records);
    }
    let played = now.advance(&replay, v.clock as u32);
    (v.replay, v.ahead, v.now) = (Some(replay), Some(ahead), Some(now));
    watch(v, played, false);
}
/// A record as the client is handed it: a frame's bytes (its step noted), or a text; a new map with the watched player's
/// place in it, and none for the map already built.
fn hand(v: &mut Viewer, p: Played) -> Option<Incoming> {
    match p.record {
        Record::Frame(bytes) => {
            if let Ok((tick, _)) = net::peek_frame(&bytes) {
                v.ticks.push_back((p.ms, tick));
                while v.ticks.len() > 400 {
                    v.ticks.pop_front();
                }
            }
            Some(Incoming::Binary(bytes))
        }
        Record::Text(text) => match kitchen_sim::replay::kind_of(&text).as_deref() {
            Some("map") => {
                let map = serde_json::from_str::<serde_json::Value>(&text).ok()?["map"].as_str()?.to_string();
                if map == v.map {
                    return None;
                }
                v.map = map.clone();
                Some(Incoming::Text(serde_json::json!({"type": "map", "id": v.watched, "map": map}).to_string()))
            }
            Some("ammo") => {
                v.ammo = Some(text.clone());
                Some(Incoming::Text(text))
            }
            _ => Some(Incoming::Text(text)),
        },
    }
}
/// What was played at the clock: the newest frame, the damage numbers and the tracers.
fn watch(v: &mut Viewer, played: Vec<Played>, sought: bool) {
    let positions = |f: &Frame, id: u32| f.players.iter().find(|s| s.player.id == id).map(|s| Vec3::from_array(s.player.p));
    for p in played {
        let Some(frame) = p.frame else { continue };
        if !sought || v.clock - (p.ms as f64) < 1200. {
            for hit in &p.hits {
                if let Some(at) = positions(&frame, hit.to) {
                    v.numbers.push_back(Number { ms: p.ms as f64, to: hit.to, amount: hit.amount, at });
                }
            }
        }
        if !sought || v.clock - (p.ms as f64) < 2500. {
            for s in frame.shots.iter().filter(|s| s.projectile != 0 || s.weapon > 0) {
                v.tracers.push_back(Tracer { ms: p.ms as f64, a: Vec3::from_array(s.a), b: Vec3::from_array(s.b), weapon: s.weapon });
            }
        }
        // The reconstructed view: the watched player's shots and swings, to kick and swing the gun in hand.
        if !sought {
            v.kicks.extend(frame.shots.iter().filter(|s| s.owner == v.watched && s.weapon > 0).map(|s| s.weapon));
        }
        if let Some(seat) = frame.players.iter().find(|s| s.player.id == v.watched) {
            let swings = (seat.player.id, seat.player.swings);
            if !sought && v.swings.is_some_and(|(id, n)| id == swings.0 && swings.1 > n) {
                v.blows.push(seat.player.weapon);
            }
            v.swings = Some(swings);
        }
        v.frame = Some(frame);
    }
    let clock = v.clock;
    v.numbers.retain(|n| clock - n.ms < 1200. && n.ms <= clock + 1.);
    v.tracers.retain(|t| clock - t.ms < 2500. && t.ms <= clock + 1.);
    while v.tracers.len() > 300 {
        v.tracers.pop_front();
    }
    // The watched player must be one there is.
    let players = v.players();
    if !players.is_empty() && !players.contains(&v.watched) {
        v.watched = players[0];
    }
}
/// The viewer's keys (controls.rs, the REPLAY and REPLAY CAMERA tabs).
fn keys(world: &mut World, v: &mut Viewer) {
    let bindings = world.get_resource::<Bindings>().cloned().unwrap_or_default();
    let keys = world.resource::<ButtonInput<KeyCode>>().clone();
    let mouse = world.resource::<ButtonInput<MouseButton>>().clone();
    let tapped = |a| bindings.just(a, &keys, &mouse);
    if tapped(Action::PlayPause) {
        toggle_play(world, v);
    }
    if tapped(Action::Slower) {
        v.speed = v.speed.saturating_sub(1);
    }
    if tapped(Action::Faster) {
        v.speed = (v.speed + 1).min(SPEEDS.len() - 1);
    }
    if tapped(Action::StepFrame) {
        step_frame(v);
    }
    if tapped(Action::SeekBack) {
        v.seek = Some((v.clock - JUMP as f64).max(0.));
    }
    if tapped(Action::SeekAhead) {
        v.seek = Some(v.clock + JUMP as f64);
    }
    for (action, way) in [(Action::NextKill, 1), (Action::PreviousKill, -1)] {
        if tapped(action)
            && let Some(at) = v.kill(way)
        {
            // (A moment before it, to see it coming.)
            v.seek = Some((at - 1500.).max(0.));
        }
    }
    let show = &mut v.show;
    for (action, flag) in [(Action::Stats, &mut show.stats), (Action::HideUi, &mut show.ui), (Action::ReplayHelp, &mut show.help), (Action::NameTags, &mut show.tags), (Action::DamageNumbers, &mut show.numbers), (Action::Tracers, &mut show.tracers), (Action::Minimap, &mut show.minimap)] {
        if tapped(action) {
            *flag = !*flag;
        }
    }
    if tapped(Action::CameraMode) {
        let next = v.mode.next();
        set_mode(world, v, next);
    }
    let players = v.players();
    if !players.is_empty() {
        let at = players.iter().position(|id| *id == v.watched).unwrap_or(0);
        if tapped(Action::NextPlayer) {
            v.watched = players[(at + 1) % players.len()];
        }
        if tapped(Action::PreviousPlayer) {
            v.watched = players[(at + players.len() - 1) % players.len()];
        }
        for (i, action) in Action::WATCH.into_iter().enumerate() {
            if tapped(action)
                && let Some(id) = players.get(i)
            {
                v.watched = *id;
            }
        }
    }
}
/// Play or pause (at the end, play starts again from the beginning).
pub fn toggle_play(world: &mut World, v: &mut Viewer) {
    if v.ended {
        v.paused = false;
        seek(world, v, 0.);
    } else {
        v.paused = !v.paused;
    }
}
/// One frame on, paused.
pub fn step_frame(v: &mut Viewer) {
    v.paused = true;
    v.clock = (v.clock + 1000. / v.send_hz as f64).min(v.duration());
}
/// Another way of seeing it: the free camera starts where the camera is.
pub fn set_mode(world: &mut World, v: &mut Viewer, mode: Mode) {
    if mode == Mode::Fly
        && let Some(cam) = world.resource::<Game>().camera()
        && let Some(tr) = world.get::<Transform>(cam)
    {
        let (yaw, pitch, _) = tr.rotation.to_euler(EulerRot::YXZ);
        v.fly = Fly { at: tr.translation, yaw, pitch, ..v.fly };
    }
    if mode == Mode::Follow {
        v.orbit = Orbit::default();
    }
    v.mode = mode;
}
/// Every frame: the watched player is the client's own (its display, network.rs's frames), drawn from the timeline
/// as everyone else, in first person with its own display; the clocks go at the replay's pace; the free camera
/// flies and the view turns with a drag.
fn steer(world: &mut World, v: &mut Viewer, real: f32) {
    let pace = v.pace();
    if let Some(mut s) = world.get_resource_mut::<crate::slowmo::SlowMo>() {
        s.replay = Some(pace);
        s.lost += real * (1. - pace);
    }
    let mut virt = world.resource_mut::<Time<Virtual>>();
    if pace <= STILL {
        virt.pause();
    } else {
        virt.unpause();
        virt.set_relative_speed(pace);
    }
    let tick = v.tick_at(v.clock);
    let watched = v.watched;
    let alive = v.frame.as_ref().and_then(|f| f.players.iter().find(|s| s.player.id == watched)).is_some_and(|s| s.player.alive);
    let weapon = v.frame.as_ref().and_then(|f| f.players.iter().find(|s| s.player.id == watched)).map(|s| s.player.weapon);
    let ammo = v.ammo.clone();
    let mut g = world.resource_mut::<Game>();
    g.online.timeline.driven = Some(tick);
    let changed = g.network_id != watched;
    g.network_id = watched;
    // First person, alive: the watched player's own display (hud.rs reads `demo` as watching from nowhere). (`camera`
    // says it again once the frame's own have been applied.)
    let living = g.roster.iter().any(|p| p.id == watched && p.alive);
    g.demo = !(v.mode == Mode::First && alive && living);
    if let Some(w) = weapon {
        g.weapon = Weapon::index(w);
    }
    if changed && let Some(text) = ammo.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        crate::reload::hear(&mut g, &text);
    }
    drop(g);
    fly(world, v, real);
}
/// The free camera's moves, the wheel (its speed; the follow camera's distance) and a drag that turns the view.
fn fly(world: &mut World, v: &mut Viewer, real: f32) {
    let menu = world.get_resource::<crate::menu::Menu>().is_some_and(|m| m.open) || world.get_resource::<list::List>().is_some_and(|l| l.open);
    let bindings = world.get_resource::<Bindings>().cloned().unwrap_or_default();
    let keys = world.resource::<ButtonInput<KeyCode>>().clone();
    let mouse = world.resource::<ButtonInput<MouseButton>>().clone();
    let scroll = world.get_resource::<AccumulatedMouseScroll>().copied().unwrap_or_default();
    let over_ui = ui::pointer_over(world);
    let cursor = world.query::<&Window>().iter(world).next().and_then(|w| w.cursor_position());
    // A drag turns the view: the right button anywhere, the left off the viewer's buttons and bar.
    let dragging = !menu && (mouse.pressed(MouseButton::Right) || (mouse.pressed(MouseButton::Left) && v.drag.is_some()));
    let starting = !menu && (mouse.just_pressed(MouseButton::Right) || (mouse.just_pressed(MouseButton::Left) && !over_ui));
    let mut turn = Vec2::ZERO;
    if (dragging || starting) && cursor.is_some() {
        if let (Some(was), Some(at)) = (v.drag, cursor) {
            turn = at - was;
        }
        v.drag = cursor;
    } else {
        v.drag = None;
    }
    let sensitivity = 0.0045 * world.get_resource::<crate::settings::Settings>().map_or(1., |s| s.sensitivity.max(0.1) / 1.5);
    let wheel = if menu { 0. } else if scroll.unit == MouseScrollUnit::Pixel { scroll.delta.y / 60. } else { scroll.delta.y };
    match v.mode {
        Mode::Fly => {
            v.fly.yaw -= turn.x * sensitivity;
            v.fly.pitch = (v.fly.pitch - turn.y * sensitivity).clamp(-1.55, 1.55);
            if wheel != 0. {
                v.fly.speed = (v.fly.speed * 1.2f32.powf(wheel)).clamp(0.05, 20.);
            }
            if !menu {
                let held = |a| bindings.held(a, &keys, &mouse);
                let rotation = Quat::from_rotation_y(v.fly.yaw) * Quat::from_rotation_x(v.fly.pitch);
                let mut way = Vec3::ZERO;
                for (action, dir) in [(Action::FlyForward, Vec3::NEG_Z), (Action::FlyBack, Vec3::Z), (Action::FlyLeft, Vec3::NEG_X), (Action::FlyRight, Vec3::X)] {
                    if held(action) {
                        way += rotation * dir;
                    }
                }
                for (action, dir) in [(Action::FlyUp, Vec3::Y), (Action::FlyDown, Vec3::NEG_Y)] {
                    if held(action) {
                        way += dir;
                    }
                }
                let fast = if held(Action::FlyFast) { 4. } else { 1. };
                v.fly.at += way.normalize_or_zero() * v.fly.speed * fast * real;
            }
        }
        Mode::Follow | Mode::First => {
            v.orbit.yaw -= turn.x * sensitivity;
            v.orbit.pitch = (v.orbit.pitch - turn.y * sensitivity).clamp(-1.4, 1.2);
            if wheel != 0. {
                v.orbit.distance = (v.orbit.distance * 0.85f32.powf(wheel)).clamp(0.18, 4.);
            }
        }
    }
}
/// The sounds go at the replay's speed: pitched with it (a quarter speed, two octaves down; twice, one up), and held
/// while it is paused. At full speed they are left alone (a kill's slow motion pitches them itself: slowmo.rs).
fn sound(world: &mut World, mut was: Local<Option<(f32, bool)>>) {
    let Some(v) = world.get_resource::<Viewer>() else { return };
    let menu = world.get_resource::<crate::menu::Menu>().is_some_and(|m| m.open);
    let (speed, held) = (SPEEDS[v.speed], v.paused || menu);
    let now = (speed, held);
    if speed == 1. && !held && *was == Some(now) {
        return;
    }
    for (sink, settings) in world.query::<(&bevy::audio::AudioSink, Option<&PlaybackSettings>)>().iter(world) {
        use bevy::audio::AudioSinkPlayback;
        let want = settings.map_or(1., |p| p.speed) * speed;
        if (sink.speed() - want).abs() > 1e-3 {
            sink.set_speed(want);
        }
        if held && !sink.is_paused() {
            sink.pause();
        } else if !held && sink.is_paused() && was.is_some_and(|(_, h)| h) {
            sink.play();
        }
    }
    *was = Some(now);
}
/// After the round is drawn: the camera where the mode puts it (through a player's recorded view, where it was drawn:
/// pov.rs), the watched player's own body out of the way of its eyes, and the pointer left free (the viewer is
/// clicked, not aimed).
fn camera(world: &mut World) {
    let Some(mut v) = world.remove_resource::<Viewer>() else { return };
    if v.replay.is_none() {
        world.insert_resource(v);
        return;
    }
    for mut cursor in world.query::<&mut CursorOptions>().iter_mut(world) {
        if cursor.grab_mode != CursorGrabMode::None || !cursor.visible {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }
    let recorded = pov::after(world, &mut v.views);
    let watched = v.watched;
    if recorded {
        // Through the player's recorded view: its own body is hidden only when the view was its eyes.
        let first = v.views.now.is_some_and(|(_, s)| s.view == kitchen_sim::pov::view::FIRST);
        hide_body(world, watched, first);
        (v.kicks, v.blows) = (vec![], vec![]);
        alerts(world, &v);
        world.insert_resource(v);
        return;
    }
    let dt = world.resource::<Time<Real>>().delta_secs().min(0.1);
    let fov = world.get_resource::<crate::settings::Settings>().map_or(80f32.to_radians(), |s| s.fov_radians());
    let g = world.resource::<Game>();
    let cam = g.camera();
    let hz = g.online.hz;
    let sample = g.online.timeline.player(watched, hz);
    let seat = v.frame.as_ref().and_then(|f| f.players.iter().find(|s| s.player.id == watched)).map(|s| s.player.clone());
    let alive = seat.as_ref().is_some_and(|p| p.alive);
    let (at, yaw, pitch) = match (sample, &seat) {
        (Some(s), _) => (s.p, s.yaw, s.pitch),
        (None, Some(p)) => (Vec3::from_array(p.p), p.yaw, p.pitch),
        (None, None) => (Vec3::ZERO, 0., 0.),
    };
    let low = match &seat {
        Some(p) if p.prone => kitchen_sim::feast::LOW,
        Some(p) if p.crouch && p.grounded => kitchen_sim::feast::MID,
        _ => EYE,
    };
    v.eye += (low - v.eye) * (1. - (-dt * 10.).exp());
    let first = v.mode == Mode::First && alive && (sample.is_some() || seat.is_some());
    // The reconstruction's scope: the sniper's at 18 degrees, the rifle's optic zooming as it comes up (arena.rs).
    let (scoped, weapon, aim) = (g.scoped, g.weapon, g.aim);
    let lens = if scoped && weapon == Weapon::Sniper { 18f32.to_radians() } else { crate::scope::fov(fov, aim * aim * (3. - 2. * aim)) };
    let place = match v.mode {
        _ if first => Some((Transform::from_translation(at + Vec3::Y * v.eye).with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch)), lens)),
        Mode::Fly => Some((Transform::from_translation(v.fly.at).with_rotation(Quat::from_rotation_y(v.fly.yaw) * Quat::from_rotation_x(v.fly.pitch)), 70f32.to_radians())),
        // Following (and first person while the watched player is dead: its body is watched from behind).
        _ if sample.is_some() || seat.is_some() => {
            let target = at + Vec3::Y * if alive { 0.09 } else { 0.03 };
            let turn = Quat::from_rotation_y(yaw + v.orbit.yaw) * Quat::from_rotation_x(v.orbit.pitch);
            let eye = target + turn * Vec3::Z * v.orbit.distance;
            let eye = Vec3::new(eye.x, eye.y.max(target.y - 0.05), eye.z);
            Some((Transform::from_translation(eye).looking_at(target, Vec3::Y), 70f32.to_radians()))
        }
        _ => None,
    };
    // The reconstruction's gun in hand (the arena poses it): built once, kicked by the shots the frames tell of, swung
    // by its swings, and aimed when the frames say it aimed.
    if first {
        crate::arena::shown::ensure_viewmodels(world);
    }
    {
        let mut g = world.resource_mut::<Game>();
        // The head-up display reads the newest frame (handed over a little ahead of the clock): dead there, it is not the
        // watched player's own display any more, so it never says YOU DIED for it.
        let up = first && g.roster.iter().any(|p| p.id == watched && p.alive);
        g.demo = !up;
        if first {
            (g.position, g.yaw, g.pitch) = (at, yaw, pitch);
            g.direction = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * Vec3::NEG_Z;
            for w in std::mem::take(&mut v.kicks) {
                g.kick_shown(Weapon::index(w));
            }
            for w in std::mem::take(&mut v.blows) {
                g.swing_shown(Weapon::index(w));
            }
            let aimed = seat.as_ref().is_some_and(|p| p.aimed) && matches!(g.weapon, Weapon::Sniper | Weapon::Rifle);
            g.scoped = aimed;
        } else {
            (v.kicks, v.blows) = (vec![], vec![]);
            g.scoped = false;
        }
    }
    alerts(world, &v);
    // The crosshair only in first person.
    let crosshairs: Vec<Entity> = world.query::<(Entity, &crate::arena::Hud)>().iter(world).filter(|(_, h)| h.0 == 3).map(|(e, _)| e).collect();
    for e in crosshairs {
        if !first && world.get::<Visibility>(e) != Some(&Visibility::Hidden) {
            world.entity_mut(e).insert(Visibility::Hidden);
        }
    }
    if let (Some(cam), Some((tr, fov))) = (cam, place) {
        if world.get::<ChildOf>(cam).is_some() {
            world.entity_mut(cam).remove::<ChildOf>();
        }
        if let Some(mut t) = world.get_mut::<Transform>(cam) {
            *t = tr;
        }
        if let Some(mut projection) = world.get_mut::<Projection>(cam)
            && let Projection::Perspective(p) = projection.as_mut()
        {
            p.fov = fov;
        }
    }
    hide_body(world, watched, first);
    world.insert_resource(v);
}
/// The round's alert (its sentences) gives way to the stats and the help, which it would show through.
fn alerts(world: &mut World, v: &Viewer) {
    if v.show.stats || v.show.help {
        let alerts: Vec<Entity> = world.query_filtered::<Entity, With<crate::arena::FoodAlert>>().iter(world).collect();
        for e in alerts {
            if world.get::<Visibility>(e) != Some(&Visibility::Hidden) {
                world.entity_mut(e).insert(Visibility::Hidden);
            }
        }
    }
}
/// The watched player's own body is not seen from inside it (`inside`).
fn hide_body(world: &mut World, watched: u32, inside: bool) {
    let bodies: Vec<(Entity, u32)> = world.query_filtered::<(Entity, &crate::player_model::Gait), With<crate::player_model::PlayerBody>>().iter(world).map(|(e, g)| (e, g.id())).collect();
    for (entity, id) in bodies {
        let hidden = inside && id == watched;
        let shown = world.get::<Visibility>(entity).copied();
        if hidden && shown != Some(Visibility::Hidden) {
            world.entity_mut(entity).insert(Visibility::Hidden);
        }
    }
}
/// Through a recorded view, at the end of the frame's moves: the camera and the gun exactly where they were drawn.
fn recorded(world: &mut World) {
    let Some(s) = world.get_resource::<Viewer>().and_then(|v| v.views.now.map(|(_, s)| s)) else { return };
    pov::place(world, &s);
}

#[cfg(test)]
mod tests;

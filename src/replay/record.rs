// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Single player records its own games: each round, from its start until the next begins (or the game ends), as the
//! server records a match. The round the client steps (arena.rs `Game::feast`) is encoded into the very frames a
//! server would send (kitchen_sim::replay `snapshot`), thirty a second of the round's own time, with the texts a server
//! sends beside them when they change (the reserves, Kung Fu, the griddles, the card table), so a single-player replay
//! is watched as any other. Natively it is written as it goes into the replays folder (store.rs); in a browser it is
//! kept in memory and saved into the page's storage when the round ends and every so often while it lasts.
//!
//! The player's own view of the round (pov.rs: the camera as it was drawn, the gun in hand, what its display heard) is
//! kept in the same file, as its track (`view`), so that the replay can be watched as the player saw it.
//!
//! On by default; the replays list (list.rs) has the switch ("RECORD REPLAYS"), kept in prefs.rs as `replays.record`.
//! While a round is recorded a small red dot sits in the top left corner of the screen.
use super::store;
use crate::arena::Game;
use bevy::prelude::*;
use kitchen_sim::replay::{self, Header, Writer, say};

/// Where the switch is kept.
pub const KEY: &str = "replays.record";
/// Frames a second recorded, and seconds between a browser's saves of a round under way.
const RATE: f32 = net_rate();
const fn net_rate() -> f32 {
    kitchen_sim::net::SEND_HZ as f32
}
#[cfg(target_arch = "wasm32")]
const SAVE_EVERY: f32 = 20.;

use store::Out;

/// A recording under way.
struct Rec {
    writer: Writer<Out>,
    name: String,
    entry: store::Entry,
    #[cfg(not(target_arch = "wasm32"))]
    path: std::path::PathBuf,
    /// The round's clock it began at, and at the last frame written; the last texts written.
    start: f32,
    last: f32,
    ammo: String,
    kungfu: String,
    griddle: (Vec<u32>, f32),
    cards: Option<u64>,
    #[cfg(target_arch = "wasm32")]
    saved: f32,
    /// The player's own view: the batch being made, and whether its header is written.
    view: kitchen_sim::pov::Batch,
    viewed: bool,
}
/// Single player's recorder.
#[derive(Resource)]
pub struct Recorder {
    /// The switch (RECORD MY GAMES), as kept.
    pub on: bool,
    loaded: bool,
    current: Option<Rec>,
    /// The recordings finished since the game began (their names), for the checks.
    pub finished: Vec<String>,
}
impl Default for Recorder {
    fn default() -> Recorder {
        Recorder { on: true, loaded: false, current: None, finished: vec![] }
    }
}
impl Recorder {
    /// Whether a round is being recorded.
    pub fn recording(&self) -> bool {
        self.current.is_some()
    }
    /// Turns the switch, and keeps it.
    pub fn set(&mut self, prefs: Option<&mut crate::prefs::Prefs>, on: bool) {
        self.on = on;
        if let Some(prefs) = prefs {
            prefs.set(KEY, if on { "on" } else { "off" });
        }
        if !on {
            self.finish();
        }
    }
    /// The player's own view on this frame (pov.rs): its header (written once), the frame and what happened on it.
    /// Kept in batches of about a second. Returns the batches written and their bytes.
    pub fn view(&mut self, header: &kitchen_sim::pov::Header, sample: kitchen_sim::pov::Sample, events: Vec<kitchen_sim::pov::Event>) -> (u32, u64) {
        let Some(rec) = self.current.as_mut() else { return (0, 0) };
        if !rec.viewed {
            rec.viewed = true;
            if let Err(error) = rec.writer.pov_header(header.player, rec.start as f64 * 1000., &header.text()) {
                warn!("Replay {}: {error}", rec.name);
            }
        }
        rec.view.samples.push(sample);
        for e in events {
            rec.view.event(e);
        }
        let long = rec.view.span().is_some_and(|(a, b)| b - a >= kitchen_sim::pov::SPAN * 1000. - 1.) || rec.view.samples.len() >= kitchen_sim::pov::MOST_SAMPLES / 2;
        if long { rec.view_out() } else { (0, 0) }
    }
    /// Ends the recording under way: its summary written, and in a browser saved.
    pub fn finish(&mut self) {
        if let Some(rec) = self.current.as_mut() {
            rec.view_out();
        }
        let Some(rec) = self.current.take() else { return };
        let name = rec.name.clone();
        match rec.writer.finish(store::now()) {
            Ok((_out, summary)) => {
                #[cfg(not(target_arch = "wasm32"))]
                info!("Replay saved: {} ({} s)", rec.path.display(), summary.duration_ms / 1000);
                #[cfg(target_arch = "wasm32")]
                info!("Replay kept in this browser: {name} ({} s)", summary.duration_ms / 1000);
                #[cfg(target_arch = "wasm32")]
                store::keep(&name, &store::Entry { duration_ms: summary.duration_ms, winner: summary.winner.clone(), complete: true, ..rec.entry.clone() }, &_out);
                #[cfg(not(target_arch = "wasm32"))]
                store::prune(None);
                self.finished.push(name);
            }
            Err(error) => warn!("Replay {name} could not be finished: {error}"),
        }
        let _ = rec.entry;
    }
}
impl Rec {
    /// The view's batch so far into the file.
    fn view_out(&mut self) -> (u32, u64) {
        if self.view.samples.is_empty() {
            return (0, 0);
        }
        let bytes = std::mem::take(&mut self.view).encode();
        if let Err(error) = self.writer.pov(1, &bytes) {
            warn!("Replay {}: {error}", self.name);
        }
        (1, bytes.len() as u64)
    }
}
impl Drop for Recorder {
    /// A game that ends finishes its recording.
    fn drop(&mut self) {
        self.finish();
    }
}

/// After the round's step, in single player: a frame every 1/30 s of the round, and the texts that changed.
pub fn sample(world: &mut World) {
    let Some(mut r) = world.remove_resource::<Recorder>() else { return };
    if !r.loaded {
        r.loaded = true;
        r.on = world.get_resource::<crate::prefs::Prefs>().and_then(|p| p.get(KEY)).is_none_or(|v| v != "off");
    }
    let options = world.get_resource::<crate::runtime::Options>().cloned().unwrap_or_default();
    let watched = !options.replay.is_empty() || options.demo || !options.connect.is_empty() || !options.record;
    let paused = crate::menu::paused(world);
    if let Some(g) = world.get_resource::<Game>()
        && r.on
        && !watched
        && !g.network
    {
        // A new round (the round's clock went back) ends the last recording.
        if r.current.as_ref().is_some_and(|rec| g.feast.elapsed + 0.001 < rec.last) {
            r.finish();
        }
        if g.playing() && !paused {
            if r.current.is_none() && g.feast.elapsed > 0. {
                r.current = begin(g);
            }
            if let Some(rec) = r.current.as_mut() {
                frame(rec, g);
            }
        }
    } else if r.current.is_some() {
        r.finish();
    }
    world.insert_resource(r);
}
/// A recording of the round being played.
fn begin(g: &Game) -> Option<Rec> {
    let f = &g.feast;
    let started = store::now();
    // (The mode the round is played in, the rules' or else the map's own: a Forts map may be played as a siege.)
    let mode = f.mode().name();
    let first = replay::snapshot(f, Some(1), |_| String::new());
    let header = Header::new(store::BUILD, "offline", &f.level.map, mode, &f.rules.text(), replay::roster(&first), started);
    let name = format!("{}-solo", store::name(started, &f.level.map.name));
    let entry = store::Entry { key: String::new(), server: false, map: f.level.map.name.clone(), mode: mode.into(), started, duration_ms: 0, winner: None, players: header.roster.iter().map(|m| m.name.clone()).collect(), complete: false };
    #[cfg(not(target_arch = "wasm32"))]
    let (out, path) = {
        let dir = store::local_dir()?;
        let path = dir.join(format!("{name}.{}", replay::EXTENSION));
        let file = std::fs::create_dir_all(&dir).and_then(|_| std::fs::File::create(&path));
        match file {
            Ok(file) => (std::io::BufWriter::new(file), path),
            Err(error) => {
                warn!("Replays cannot be written to {} ({error}): this game is not recorded", dir.display());
                return None;
            }
        }
    };
    #[cfg(target_arch = "wasm32")]
    let out = Vec::new();
    let mut writer = Writer::new(out, &header).ok()?;
    let tune = f.tune.text();
    if !tune.is_empty() {
        writer.text(0, &say::tuning(&tune));
    }
    #[cfg(not(target_arch = "wasm32"))]
    store::prune(Some(&path));
    Some(Rec {
        writer,
        name,
        entry,
        #[cfg(not(target_arch = "wasm32"))]
        path,
        start: f.elapsed,
        last: f32::MIN,
        ammo: String::new(),
        kungfu: String::new(),
        griddle: (vec![], f32::MIN),
        cards: None,
        #[cfg(target_arch = "wasm32")]
        saved: f.elapsed,
        view: kitchen_sim::pov::Batch::default(),
        viewed: false,
    })
}
/// A frame of the round, and the texts that changed, when 1/30 s of it has gone by since the last.
fn frame(rec: &mut Rec, g: &Game) {
    let f = &g.feast;
    if f.elapsed - rec.last < 1. / RATE - 1e-4 {
        return;
    }
    rec.last = f.elapsed;
    let ms = ((f.elapsed - rec.start).max(0.) * 1000.).round() as u32;
    let tick = (ms as u64 * kitchen_sim::net::TICK_HZ as u64 / 1000) as u32 + 1;
    // The texts a server would have sent with it (crates/server hub.rs `tell_*`).
    if f.kungfu.is_none() && f.cards.is_none() && !f.players.is_empty() {
        let ammo = say::ammo(&f.stocks());
        if ammo != rec.ammo {
            rec.writer.text(ms, &ammo);
            rec.ammo = ammo;
        }
    }
    if let Some(view) = &g.kungfu {
        let key = serde_json::to_string(&kitchen_sim::kungfu::View { elapsed: 0., ..view.clone() }).unwrap_or_default();
        if key != rec.kungfu {
            rec.writer.text(ms, &say::kungfu(view));
            rec.kungfu = key;
        }
    }
    if !f.griddles.is_empty() {
        let flips: Vec<u32> = f.griddles.iter().map(|g| g.flips).collect();
        if flips != rec.griddle.0 || f.elapsed - rec.griddle.1 >= kitchen_sim::griddle::SYNC {
            rec.writer.text(ms, &say::griddle(&f.griddle_states()));
            rec.griddle = (flips, f.elapsed);
        }
    }
    if let Some(table) = &f.cards
        && rec.cards != Some(table.version())
    {
        rec.writer.text(ms, &say::cards(&table.view()));
        rec.cards = Some(table.version());
    }
    let snap = replay::snapshot(f, Some(1), |_| String::new());
    let state = snap.state();
    if let Err(error) = rec.writer.frame(ms, tick, &snap, &state) {
        warn!("Replay {}: {error}", rec.name);
    }
    #[cfg(target_arch = "wasm32")]
    if f.elapsed - rec.saved > SAVE_EVERY {
        rec.saved = f.elapsed;
        store::keep(&rec.name, &store::Entry { duration_ms: ms, ..rec.entry.clone() }, rec.writer.get_ref());
    }
}

#[derive(Component)]
pub struct Dot;
/// The recording's dot: a small red one in the top left corner while a round is recorded (and the display is up).
pub fn dot(world: &mut World, mut have: Local<Option<Entity>>) {
    let on = world.get_resource::<Recorder>().is_some_and(|r| r.recording()) && world.get_resource::<Game>().is_some_and(|g| g.playing());
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let entity = match *have {
        Some(e) if world.get_entity(e).is_ok() => e,
        _ => {
            let e = world
                .spawn((
                    Dot,
                    Name::new("Recording"),
                    Node { position_type: PositionType::Absolute, left: Val::Px(9.), top: Val::Px(9.), width: Val::Px(7.), height: Val::Px(7.), border_radius: BorderRadius::all(Val::Px(4.)), ..default() },
                    BackgroundColor(Color::srgba(0.95, 0.2, 0.2, 0.7)),
                    Visibility::Hidden,
                ))
                .id();
            *have = Some(e);
            e
        }
    };
    let shown = if on { Visibility::Inherited } else { Visibility::Hidden };
    if world.get::<Visibility>(entity) != Some(&shown) {
        world.entity_mut(entity).insert(shown);
    }
    if on && let Some(mut bg) = world.get_mut::<BackgroundColor>(entity) {
        bg.0 = Color::srgba(0.95, 0.2, 0.2, 0.5 + 0.25 * (now * 2.).sin());
    }
}

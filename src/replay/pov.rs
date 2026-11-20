// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! This player's own view, kept for replays (kitchen_sim::pov), and shown again when a replay is watched through it.
//!
//! **Kept** (`sample`, after everything has drawn the frame): what the screen showed (the camera exactly as it was
//! drawn, its field of view, which view it was, the first-person gun as it was drawn), what the presentation reads of
//! this player (`Sample`), and what happened on the frame (`Event`: by what changed since the last frame, what the
//! damage indicators heard, and the shot the trigger fired). At most kitchen_sim::pov `RATE` frames a second.
//!
//! * **Single player**: into the round's own recording (record.rs), as another track of its file.
//! * **Online**: on the server's clock (its steps as this client's timeline drew them), in batches of about a second.
//!   This client keeps its own copy (natively `<stamp>-<map>-view.replay` in the replays folder, the newest
//!   `store::KEEP_LOCAL` kept; in a browser in IndexedDB, the newest `store::KEEP_BROWSER`), and with the setting on
//!   (the REPLAYS page's SHARE MY VIEW, `replays.pov`, on to begin with) sends each batch to the server as a
//!   `{"type":"pov"}` text beside the game's (network.rs), which keeps it in that match's recording; a server that does
//!   not know the message passes it over. Nothing of the match depends on it.
//! * **Never** in a replay's viewer or the demo; nothing at all with `--no-pov` or `HOTDOG_NO_POV` set (anything but
//!   `0`), or `?nopov` in a browser; and online the local copy, a file, follows `--no-record` and the rest (store.rs).
//!
//! **Shown again** (the viewer, mod.rs): watching a player in first person whose view the replay holds, the camera is
//! put exactly where that player's was (between two of its frames when the replay's frames fall between them), the
//! first-person gun as it was drawn, and the state the presentation reads is put back (`Shown`) so that the game's own
//! code draws the rest as it drew it then: the scope and the rifle's optic, the crosshair and the kill cross, the
//! display, the reload, the stim; the damage indicators, the hit marker and the slow motion's look hear the events it
//! heard (`heard`), on that player's own clock (`now`), whatever the replay's speed.
use super::store;
use crate::arena::{Game, Weapon};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use kitchen_sim::pov::{self, Batch, Event, Header, Sample, flag, view};

/// Where the switch is kept.
pub const KEY: &str = "replays.pov";

/// What the last frame showed, to tell what changed.
#[derive(Clone, Default)]
struct Seen {
    weapon: u8,
    scoped: bool,
    reload: (u8, f32),
    swing: (bool, f32),
    fighter: Option<(u8, u8, u32)>,
    carrying: bool,
    pose: u8,
    slow: Option<u32>,
    smudges: u32,
    quality: u8,
    round: Option<u32>,
}
/// Online: this client's own copy of its view.
struct Local {
    name: String,
    player: u32,
    writer: kitchen_sim::replay::Writer<store::Out>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    entry: store::Entry,
    #[cfg(not(target_arch = "wasm32"))]
    path: std::path::PathBuf,
    #[cfg(target_arch = "wasm32")]
    saved: f64,
}
/// The recorder (see the module's documentation).
#[derive(Resource)]
pub struct Pov {
    /// The setting: online, this player's view is sent to the server for its recording.
    pub share: bool,
    loaded: bool,
    seen: Option<Seen>,
    /// Online: the batch being made, the header it goes with (and the hotdog it was sent for), the batches' numbers.
    batch: Batch,
    header: Option<Header>,
    told: Option<u32>,
    /// Whether the server has been sent the header (the switch may be turned on in the middle of a match).
    sent: bool,
    seq: u32,
    local: Option<Local>,
    /// The real time of the last frame kept, and what happened since it.
    kept: f64,
    pending: Vec<Event>,
    /// Online: the messages for network.rs to send.
    pub outbox: Vec<String>,
    /// For the checks and the report: batches made, their bytes, frames kept, and the span they cover (ms).
    pub batches: u32,
    pub bytes: u64,
    pub frames: u64,
    pub span: Option<(f64, f64)>,
}
impl Default for Pov {
    fn default() -> Pov {
        Pov { share: true, loaded: false, seen: None, batch: Batch::default(), header: None, told: None, sent: false, seq: 0, local: None, kept: f64::MIN, pending: vec![], outbox: vec![], batches: 0, bytes: 0, frames: 0, span: None }
    }
}
impl Pov {
    /// Turns the switch, and keeps it.
    pub fn set(&mut self, prefs: Option<&mut crate::prefs::Prefs>, on: bool) {
        self.share = on;
        if let Some(prefs) = prefs {
            prefs.set(KEY, if on { "on" } else { "off" });
        }
    }
    /// Online: the batch being made goes (into the local copy, and to the server when shared), and the copy is
    /// finished when `end` (the game left, the match over).
    fn flush(&mut self, end: bool) {
        if !self.batch.samples.is_empty() {
            let bytes = self.batch.encode();
            self.batches += 1;
            self.bytes += bytes.len() as u64;
            if let Some(local) = self.local.as_mut() {
                let _ = local.writer.pov(local.player, &bytes);
            }
            if self.share && self.told.is_some() {
                if !self.sent
                    && let Some(h) = &self.header
                {
                    self.outbox.push(serde_json::json!({"type": "pov", "header": serde_json::from_str::<serde_json::Value>(&h.text()).unwrap_or_default()}).to_string());
                    self.sent = true;
                }
                self.seq += 1;
                self.outbox.push(serde_json::json!({"type": "pov", "seq": self.seq, "data": pov::base64(&bytes)}).to_string());
            }
            self.batch = Batch::default();
        }
        if end && let Some(local) = self.local.take() {
            local.finish();
        }
    }
}
impl Local {
    fn finish(self) {
        let name = self.name.clone();
        match self.writer.finish(store::now()) {
            Ok((_out, summary)) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    info!("This player's view kept: {}", self.path.display());
                    store::prune_views(None);
                }
                #[cfg(target_arch = "wasm32")]
                store::keep(&name, &store::Entry { duration_ms: summary.duration_ms, complete: true, ..self.entry.clone() }, &_out);
                let _ = summary;
            }
            Err(error) => warn!("This player's view {name} could not be finished: {error}"),
        }
    }
}
impl Drop for Pov {
    /// A game that ends finishes its copy.
    fn drop(&mut self) {
        self.flush(true);
    }
}

// ----- What the presentation reads while a recorded view is watched.

/// A recorded view being watched (mod.rs puts it here each frame): the player's own clock at the replay's clock, and
/// the damage events its screen heard since the last frame.
#[derive(Resource, Default)]
pub struct Shown {
    pub now: Option<f32>,
    pub heard: Option<Vec<crate::damage::Event>>,
}
/// The real clock the presentation goes by: the watched player's own while a replay is watched through its recorded
/// view, otherwise this screen's (`Time<Real>`).
pub fn now(world: &World) -> f32 {
    world.get_resource::<Shown>().and_then(|s| s.now).unwrap_or_else(|| world.get_resource::<Time<Real>>().map_or(0., |t| t.elapsed_secs()))
}
/// What the damage indicators hear this frame while a recorded view is watched (None otherwise: they hear the round).
pub fn heard(world: &mut World) -> Option<Vec<crate::damage::Event>> {
    world.get_resource_mut::<Shown>().and_then(|mut s| s.heard.take())
}
/// A damage event as kept, and back.
pub fn event_of(e: &crate::damage::Event) -> Event {
    match *e {
        crate::damage::Event::Hurt(damage) => Event::Hurt { damage },
        crate::damage::Event::Dealt { victim, kill, at } => Event::Dealt { victim, kill, at: at.to_array() },
        crate::damage::Event::Kills => Event::Kills,
        crate::damage::Event::Final { victim, at } => Event::Final { victim, at: at.to_array() },
    }
}
pub fn damage_of(e: &Event) -> Option<crate::damage::Event> {
    Some(match e {
        Event::Hurt { damage } => crate::damage::Event::Hurt(*damage),
        Event::Dealt { victim, kill, at } => crate::damage::Event::Dealt { victim: *victim, kill: *kill, at: Vec3::from_array(*at) },
        Event::Kills => crate::damage::Event::Kills,
        Event::Final { victim, at } => crate::damage::Event::Final { victim: *victim, at: Vec3::from_array(*at) },
        _ => return None,
    })
}

// ----- Keeping it.

/// Whether this run keeps views at all: never in a replay's viewer or the demo, nor with `--no-pov`.
fn allowed(options: &crate::runtime::Options) -> bool {
    options.pov && options.replay.is_empty() && !options.demo
}
/// What the screen showed this frame, and what the presentation read of this player (see the module's documentation).
fn read(world: &mut World, t: f64) -> Option<Sample> {
    let captured = world.query::<&bevy::window::CursorOptions>().iter(world).any(|c| c.grab_mode == bevy::window::CursorGrabMode::Locked);
    let held = {
        let bindings = world.get_resource::<crate::controls::Bindings>().cloned().unwrap_or_default();
        let (keys, mouse) = (world.get_resource::<ButtonInput<KeyCode>>()?, world.get_resource::<ButtonInput<MouseButton>>()?);
        crate::controls::Action::ALL.iter().take(64).enumerate().filter(|(_, a)| bindings.held(**a, keys, mouse)).fold(0u64, |b, (i, _)| b | 1 << i)
    };
    let look = world.get_resource::<AccumulatedMouseMotion>().map_or(Vec2::ZERO, |m| m.delta);
    let quality = world.get_resource::<crate::quality::Detail>().map_or(0, |d| crate::quality::Level::ALL.iter().position(|l| *l == d.level).unwrap_or(0) as u8);
    let cross = world.query::<(&crate::arena::Hud, &Text)>().iter(world).any(|(h, t)| h.0 == 3 && t.0 == "x");
    let g = world.get_resource::<Game>()?;
    let cam = g.camera()?;
    let (_, turn, cam_at) = world.get::<GlobalTransform>(cam)?.to_scale_rotation_translation();
    let fov = match world.get::<Projection>(cam)? {
        Projection::Perspective(p) => p.fov,
        _ => 0.,
    };
    let time = world.resource::<Time<Real>>();
    let (now, dt) = (time.elapsed_secs() as f64, time.delta_secs());
    let me = g.roster.iter().find(|p| p.id == g.me());
    let alive = me.is_some_and(|p| p.alive);
    let table = crate::cards::at_table(g);
    let menu = world.get_resource::<crate::menu::Menu>().is_some_and(|m| m.open);
    let kungfu = g.kung_fu();
    let view = if menu {
        view::MENU
    } else if !g.playing() || g.demo {
        view::SPECTATOR
    } else if table {
        view::TABLE
    } else if !alive {
        view::DEAD
    } else if kungfu && crate::fight::third_person(world) {
        view::THIRD
    } else {
        view::FIRST
    };
    let private = g.private();
    let bits = [
        (g.scoped, flag::SCOPED),
        (alive, flag::ALIVE),
        (g.prone, flag::PRONE),
        (g.crouch, flag::CROUCH),
        (me.is_some_and(|p| !p.grounded), flag::AIRBORNE),
        (me.is_some_and(|p| p.carry >= 0), flag::CARRYING),
        (table, flag::TABLE),
        (kungfu, flag::KUNGFU),
        (captured, flag::CAPTURED),
        (menu, flag::MENU),
        (private.clap < STIM_CLAP && alive, flag::STIM),
        (g.locked(), flag::LOCKED),
        (g.playing() && !g.demo, flag::PLAYING),
        (cross, flag::CROSS),
    ];
    let flags = bits.iter().filter(|(on, _)| *on).fold(0, |f, (_, bit)| f | bit);
    // The first-person gun as drawn: the one shown, its root in the camera's frame.
    let mut gun = (0u8, [0.; 3], [0.; 3]);
    for &(weapon, root) in g.viewmodels() {
        if world.get::<Visibility>(root) == Some(&Visibility::Visible)
            && let Some(tr) = world.get::<Transform>(root)
        {
            gun = (weapon as u8, tr.translation.to_array(), pov::euler(glam_quat(tr.rotation)));
        }
    }
    let w = g.weapon as usize;
    let reload = g.mag.reload.filter(|r| r.left > 0.).map_or((0, 0.), |r| (r.gun as u8, r.progress()));
    let (ready, size) = g.burst();
    Some(Sample {
        t,
        now,
        dt,
        view,
        flags,
        cam: cam_at.to_array(),
        turn: pov::euler(glam_quat(turn)),
        fov,
        at: g.position.to_array(),
        yaw: g.yaw,
        pitch: g.pitch,
        eye: private.eye,
        gun: gun.0,
        hold: gun.1,
        tilt: gun.2,
        weapon: w as u8,
        aim: g.aim,
        hit: private.hit,
        reload: reload.0,
        progress: reload.1,
        clap: private.clap.clamp(0., 1.),
        stim: g.stim_left(),
        hp: me.map_or(0., |p| p.hp),
        ammo: std::array::from_fn(|k| g.ammo.get(k + 1).copied().unwrap_or(0)),
        reserve: std::array::from_fn(|k| g.mag.reserve.get(k + 1).copied().unwrap_or(0)),
        kills: me.map_or(g.kills, |p| p.kills),
        score: me.map_or(g.score, |p| p.score),
        burst: ready.min(255) as u8,
        bursts: size.min(255) as u8,
        buttons: held,
        look: look.to_array(),
        quality,
    })
}
/// The stim clap's length in first person (arena.rs STIM_CLAP).
pub const STIM_CLAP: f32 = 0.55;
/// Bevy's quaternion as the simulation's (they may be of two versions of glam).
pub fn glam_quat(q: Quat) -> kitchen_sim::pov::Quat {
    kitchen_sim::pov::Quat::from_xyzw(q.x, q.y, q.z, q.w)
}
pub fn bevy_quat(q: kitchen_sim::pov::Quat) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}
/// What happened on this frame, by what changed since the last (and the shot the trigger fired, what the damage
/// indicators heard).
fn events(world: &World, seen: &mut Option<Seen>, s: &Sample) -> Vec<Event> {
    let Some(g) = world.get_resource::<Game>() else { return vec![] };
    let mut out = vec![];
    let now = Seen {
        weapon: s.weapon,
        scoped: s.has(flag::SCOPED),
        reload: (s.reload, s.progress),
        swing: (g.whip.moving(), g.whip.progress()),
        fighter: g.kungfu.as_ref().and_then(|_| g.fighter(g.me())).map(|f| (f.style, f.mv, f.started.to_bits())),
        carrying: s.has(flag::CARRYING),
        pose: g.pose.bits(),
        slow: world.get_resource::<crate::slowmo::SlowMo>().and_then(|m| m.start).map(f32::to_bits),
        smudges: world.get_resource::<crate::arena::splatter::boom::Booms>().map_or(0, |b| b.smudges() as u32),
        quality: s.quality,
        round: Some(g.feast.round),
    };
    if g.pending_shot {
        out.push(Event::Shot { weapon: s.weapon, yaw: g.yaw, pitch: g.pitch });
    }
    if let Some(p) = g.pending_pickup {
        out.push(Event::Pickup { index: p as u32 });
    }
    if g.pending_throw {
        out.push(Event::Carry { what: 2, pose: now.pose });
    }
    if let Some(was) = seen.as_ref() {
        if was.weapon != now.weapon {
            out.push(Event::Weapon { to: now.weapon });
        }
        if was.scoped != now.scoped {
            out.push(Event::Scope { up: now.scoped });
        }
        match (was.reload.0, now.reload.0) {
            (0, 0) => {}
            (0, gun) => out.push(Event::Reload { gun, phase: 0 }),
            (gun, 0) => out.push(Event::Reload { gun, phase: if was.reload.1 > 0.9 { 2 } else { 1 } }),
            (a, b) if a != b || now.reload.1 + 0.05 < was.reload.1 => {
                out.push(Event::Reload { gun: a, phase: 1 });
                out.push(Event::Reload { gun: b, phase: 0 });
            }
            _ => {}
        }
        if now.swing.0 && (!was.swing.0 || now.swing.1 + 1e-4 < was.swing.1) {
            out.push(Event::Melee { weapon: s.weapon });
        }
        if let Some((style, mv, started)) = now.fighter
            && was.fighter.is_none_or(|f| f.2 != started || f.1 != mv)
        {
            out.push(Event::Move { style, mv });
        }
        if was.carrying != now.carrying && !g.pending_throw {
            out.push(Event::Carry { what: if now.carrying { 0 } else { 1 }, pose: now.pose });
        } else if now.carrying && was.pose != now.pose {
            let what = if (was.pose ^ now.pose) & 7 != 0 { 3 } else { 4 };
            out.push(Event::Carry { what, pose: now.pose });
        }
        if now.slow.is_some() && now.slow != was.slow
            && let Some(m) = world.get_resource::<crate::slowmo::SlowMo>()
        {
            out.push(Event::Slow { shown_only: m.shown_only, focus: m.focus.map(|f| f.to_array()) });
        }
        if now.smudges > was.smudges {
            out.push(Event::Lens { smudges: now.smudges });
        }
        if was.quality != now.quality {
            let name = crate::quality::Level::ALL.get(now.quality as usize).map_or("", |l| l.name());
            out.push(Event::Quality { level: now.quality, name: name.into() });
        }
    }
    // A new round: single player's card table is shuffled by a seed of this screen's own.
    if seen.as_ref().is_none_or(|w| w.round != now.round) && !g.network && g.feast.cards.is_some() {
        out.push(Event::Seed { what: "cards".into(), value: g.feast.card_seed });
    }
    if let Some(hurt) = world.get_resource::<crate::damage::Hurt>() {
        out.extend(hurt.heard.iter().map(event_of));
    }
    *seen = Some(now);
    out
}
/// The header of this player's view as the screen is now.
fn header(world: &mut World, clock: &str, source: &str) -> Header {
    let (width, height, aspect) = world.query::<&Window>().iter(world).next().map_or((0, 0, 0.), |w| (w.physical_width(), w.physical_height(), w.width() / w.height().max(1.)));
    let renderer = world.get_resource::<bevy::render::renderer::RenderAdapterInfo>().map_or(String::new(), |a| format!("{:?}: {}", a.backend, a.name));
    let detail = world.get_resource::<crate::quality::Detail>().copied();
    let settings = world.get_resource::<crate::settings::Settings>().copied().unwrap_or_default();
    let g = world.resource::<Game>();
    Header {
        format: pov::FORMAT,
        player: g.me(),
        name: g.roster.iter().find(|p| p.id == g.me()).map_or(String::new(), |p| p.name.clone()),
        build: store::BUILD.into(),
        renderer,
        quality: detail.map_or(String::new(), |d| d.level.name().into()),
        width,
        height,
        aspect,
        scale: detail.map_or(1., |d| d.scale),
        fov: settings.fov,
        scope_toggle: settings.scope_toggle,
        sensitivity: settings.sensitivity,
        rate: pov::RATE,
        clock: clock.into(),
        source: source.into(),
        started: store::now(),
    }
}

/// Every frame, once it is drawn: this player's view is kept (see the module's documentation).
pub fn sample(world: &mut World) {
    let Some(mut pov) = world.remove_resource::<Pov>() else { return };
    if !pov.loaded {
        pov.loaded = true;
        pov.share = world.get_resource::<crate::prefs::Prefs>().and_then(|p| p.get(KEY)).is_none_or(|v| v != "off");
    }
    keep(world, &mut pov);
    world.insert_resource(pov);
}
fn keep(world: &mut World, pov: &mut Pov) {
    let options = world.get_resource::<crate::runtime::Options>().cloned().unwrap_or_default();
    let Some(g) = world.get_resource::<Game>() else { return };
    let online = g.network && g.network_id != 0;
    if !allowed(&options) || !g.active() || (!online && g.network) {
        pov.seen = None;
        if pov.local.is_some() || !pov.batch.samples.is_empty() {
            pov.flush(true);
        }
        return;
    }
    // The match clock: online the server's steps as the timeline draws them (ms); single player the round's.
    let t = if online {
        let (clock, hz) = (g.online.timeline.clock, g.online.hz.max(1.) as f64);
        if clock <= 0. {
            return;
        }
        clock * 1000. / hz
    } else {
        g.feast.elapsed as f64 * 1000.
    };
    let recording = world.get_resource::<super::record::Recorder>().is_some_and(|r| r.recording());
    if !online && !recording {
        pov.seen = None;
        return;
    }
    let real = world.resource::<Time<Real>>().elapsed_secs_f64();
    let Some(sample) = read(world, t) else { return };
    let happened = events(world, &mut pov.seen, &sample);
    pov.pending.extend(happened);
    // At most RATE frames a second, on average over a faster screen's (what happened on a frame left out goes with the
    // next kept).
    let every = 1. / pov::RATE as f64;
    if real - pov.kept < every - 1e-4 {
        return;
    }
    pov.kept = (pov.kept + every).max(real - every);
    pov.frames += 1;
    pov.span = Some(pov.span.map_or((t, t), |(a, _)| (a, t)));
    let happened = std::mem::take(&mut pov.pending);
    if !online {
        let h = header(world, "round", "offline");
        if let Some(mut r) = world.get_resource_mut::<super::record::Recorder>() {
            let (batches, bytes) = r.view(&h, sample, happened);
            (pov.batches, pov.bytes) = (pov.batches + batches, pov.bytes + bytes);
        }
        return;
    }
    // Online: a header for each hotdog this client plays (a new map may give it another), to the server and the copy.
    let me = world.resource::<Game>().network_id;
    if pov.told != Some(me) {
        pov.flush(true);
        let h = header(world, "server", "online");
        if options.record {
            pov.local = begin_local(world, &h);
        }
        // (The header goes to the server with the first batch it is sent.)
        (pov.header, pov.told, pov.sent) = (Some(h), Some(me), false);
    }
    pov.batch.samples.push(sample);
    for e in happened {
        pov.batch.event(e);
    }
    let long = pov.batch.span().is_some_and(|(a, b)| b - a >= pov::SPAN * 1000. - 1.);
    if long || pov.batch.samples.len() >= pov::MOST_SAMPLES / 2 {
        pov.flush(false);
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(local) = pov.local.as_mut()
        && real - local.saved > 20.
    {
        local.saved = real;
        store::keep(&local.name, &local.entry, local.writer.get_ref());
    }
}
/// Online: this client's own copy of its view, a replay of its own holding only the view (on the server's clock).
fn begin_local(world: &mut World, h: &Header) -> Option<Local> {
    let g = world.resource::<Game>();
    let map = &g.feast.level.map;
    let started = store::now();
    // (The mode the round is played in, the rules' or else the map's own: a Forts map may be played as a siege.)
    let mode = g.feast.mode().name();
    let mut replay_header = kitchen_sim::replay::Header::new(store::BUILD, "view", map, mode, &g.feast.rules.text(), vec![], started);
    replay_header.roster = g.roster.iter().map(|p| kitchen_sim::replay::Member { id: p.id, name: p.name.clone(), team: p.team, bot: p.bot, color: String::new() }).collect();
    let name = format!("{}-view", store::name(started, &map.name));
    let entry = store::Entry { key: String::new(), server: false, map: map.name.clone(), mode: mode.into(), started, duration_ms: 0, winner: None, players: vec![h.name.clone()], complete: false };
    #[cfg(not(target_arch = "wasm32"))]
    let (out, path) = {
        let dir = store::local_dir()?;
        let path = dir.join(format!("{name}.{}", kitchen_sim::replay::EXTENSION));
        match std::fs::create_dir_all(&dir).and_then(|_| std::fs::File::create(&path)) {
            Ok(file) => (std::io::BufWriter::new(file), path),
            Err(error) => {
                warn!("This player's view cannot be kept in {} ({error})", dir.display());
                return None;
            }
        }
    };
    #[cfg(target_arch = "wasm32")]
    let out = Vec::new();
    let mut writer = kitchen_sim::replay::Writer::new(out, &replay_header).ok()?;
    // (Its clock is the server's own: the offset is 0. A viewer puts it on a server's recording by that one's start.)
    writer.pov_header(h.player, 0., &h.text()).ok()?;
    #[cfg(not(target_arch = "wasm32"))]
    store::prune_views(Some(&path));
    Some(Local {
        name,
        player: h.player,
        writer,
        entry,
        #[cfg(not(target_arch = "wasm32"))]
        path,
        #[cfg(target_arch = "wasm32")]
        saved: world.resource::<Time<Real>>().elapsed_secs_f64(),
    })
}

/// Natively, the views this client kept of matches on servers that could hold `replay` (one of a server's): those of
/// its map whose span falls within it, each as a track on the replay's clock, by player.
#[cfg(not(target_arch = "wasm32"))]
pub fn kept_for(replay: &kitchen_sim::replay::Replay) -> Vec<(u32, pov::Track)> {
    let h = &replay.header;
    if h.source != "server" || h.start_ms <= 0. {
        return vec![];
    }
    let (from, to) = (h.start_ms, h.start_ms + replay.duration() as f64);
    let Some(dir) = store::local_dir() else { return vec![] };
    let Ok(read) = std::fs::read_dir(&dir) else { return vec![] };
    let mut out = vec![];
    for path in read.flatten().map(|e| e.path()).filter(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().ends_with("-view"))) {
        let Ok(view) = kitchen_sim::replay::Replay::open(&path) else { continue };
        if view.header.map_name != h.map_name || view.header.started + 60 < h.started || view.header.started > h.started + 4 * 3600 {
            continue;
        }
        for player in view.povs.keys().copied().collect::<Vec<_>>() {
            if replay.povs.contains_key(&player) {
                continue;
            }
            let Some(mut track) = view.track(player) else { continue };
            let Some((a, b)) = track.span() else { continue };
            if b < from || a > to {
                continue;
            }
            for s in &mut track.samples {
                s.t -= from;
            }
            info!("A view this client kept of player {player} is watched with the replay: {}", path.display());
            out.push((player, track));
        }
    }
    out
}


// ----- Shown again (the viewer, mod.rs).

/// The players' own views in the viewer: each player's track once read (None: the replay has none), the views this
/// client kept itself of the match (natively), and the view at the clock this frame when the replay is watched
/// through one.
#[derive(Default)]
pub struct Views {
    tracks: std::collections::HashMap<u32, Option<pov::Track>>,
    kept: Vec<(u32, pov::Track)>,
    /// Whose view, and the view at the clock (between its two frames either side).
    pub now: Option<(u32, Sample)>,
    /// Up to when the events were heard (ms), the seeks and whose view they were heard of: a seek or another view
    /// hears the last few seconds' again, each at its own moment (`catch_up`).
    heard_to: f64,
    seeks: u32,
    of: Option<u32>,
    fresh: bool,
    /// What this frame heard: its damage events, and the shots (for their sound).
    heard: Vec<(Sample, Event)>,
}
/// Seconds of what a view heard that a seek hears again (the indicators' and the slow motion's longest).
const CATCH_UP: f64 = 3000.;
impl Views {
    /// The views of `replay`, with those this client kept of it (natively).
    pub fn of(replay: &kitchen_sim::replay::Replay) -> Views {
        #[cfg(not(target_arch = "wasm32"))]
        let kept = kept_for(replay);
        #[cfg(target_arch = "wasm32")]
        let kept = {
            let _ = replay;
            vec![]
        };
        Views { kept, ..Default::default() }
    }
    /// Whether player `id`'s own view is in the replay (or was kept here).
    pub fn has(&self, replay: &kitchen_sim::replay::Replay, id: u32) -> bool {
        replay.povs.contains_key(&id) || self.kept.iter().any(|(p, _)| *p == id)
    }
    fn track(&mut self, replay: &kitchen_sim::replay::Replay, id: u32) -> Option<&pov::Track> {
        if !self.tracks.contains_key(&id) {
            let track = replay.track(id).or_else(|| self.kept.iter().find(|(p, _)| *p == id).map(|(_, t)| t.clone()));
            if let Some(t) = &track {
                info!("Replay: player {id}'s own view, {} frames from {:.1} s to {:.1} s{}", t.samples.len(), t.span().map_or(0., |s| s.0 / 1e3), t.span().map_or(0., |s| s.1 / 1e3), t.dropped.as_ref().map_or(String::new(), |w| format!(" (the server stopped keeping it: {w})")));
            }
            self.tracks.insert(id, track);
        }
        self.tracks.get(&id)?.as_ref()
    }
    /// The view of player `id` at `ms`, if its track covers it.
    pub fn at(&mut self, replay: &kitchen_sim::replay::Replay, id: u32, ms: f64) -> Option<Sample> {
        self.track(replay, id)?.view(ms)
    }
}
/// Puts what the presentation reads back into the game from a recorded view.
fn put_back(g: &mut Game, s: &Sample) {
    (g.position, g.yaw, g.pitch) = (Vec3::from_array(s.at), s.yaw, s.pitch);
    g.direction = Quat::from_rotation_y(s.yaw) * Quat::from_rotation_x(s.pitch) * Vec3::NEG_Z;
    (g.prone, g.crouch) = (s.has(flag::PRONE), s.has(flag::CROUCH));
    g.weapon = Weapon::index(s.weapon as usize);
    (g.scoped, g.aim) = (s.has(flag::SCOPED), s.aim);
    for k in 0..pov::GUNS {
        if let Some(a) = g.ammo.get_mut(k + 1) {
            *a = s.ammo[k];
        }
        if let Some(r) = g.mag.reserve.get_mut(k + 1) {
            *r = s.reserve[k];
        }
    }
    g.mag.reload = (s.reload > 0).then(|| {
        let gun = s.reload as usize;
        let of = g.feast.tune.reload.get(gun).copied().filter(|o| *o > 0.).unwrap_or(1.);
        let left = of * (1. - s.progress.clamp(0., 1.));
        kitchen_sim::magazine::Reload { gun, at: g.elapsed - (of - left), of, left: left.max(1e-4) }
    });
    (g.kills, g.score) = (s.kills, s.score);
    let me = g.me();
    if let Some(p) = g.roster.iter_mut().find(|p| p.id == me) {
        (p.hp, p.alive, p.kills, p.score) = (s.hp, s.has(flag::ALIVE), s.kills, s.score);
    }
    g.put_back(crate::arena::shown::Private { hit: s.hit, clap: s.clap, eye: s.eye }, (s.burst as u32, s.bursts as u32));
    g.demo = false;
}
/// Before the round's frame (the viewer's `feed`): the view at the clock found, and what it heard since the last.
pub fn before(world: &mut World, views: &mut Views, replay: &kitchen_sim::replay::Replay, id: u32, clock: f64, through: bool, seeks: u32) {
    let sample = if through { views.at(replay, id, clock) } else { None };
    let fresh = sample.is_some() && (views.of != Some(id) || views.seeks != seeks || views.now.is_none());
    views.now = sample.map(|s| (id, s));
    views.heard.clear();
    let Some(s) = sample else {
        if views.of.take().is_some() {
            forget(world);
        }
        if let Some(mut shown) = world.get_resource_mut::<Shown>() {
            (shown.now, shown.heard) = (None, None);
        }
        return;
    };
    if fresh {
        views.heard_to = clock - CATCH_UP;
        (views.of, views.seeks) = (Some(id), seeks);
    }
    views.fresh = fresh;
    if let Some(track) = views.tracks.get(&id).and_then(|t| t.as_ref()) {
        views.heard = track.events_between(views.heard_to, clock).map(|(s, e)| (*s, e.clone())).collect();
    }
    views.heard_to = clock;
}
/// Just before the round's frame, once what the recording said has been heard (network.rs: its texts would put the
/// reserves back as the server last told them): what the presentation reads put back, so that the arena's frame draws
/// from it (the crosshair, the stim's forearm and its bolts, the reload's sounds, the display's +N).
pub fn restore(world: &mut World) {
    let Some(s) = world.get_resource::<super::Viewer>().and_then(|v| v.views.now.map(|(_, s)| s)) else { return };
    let dt = world.resource::<Time<Real>>().delta_secs() * crate::slowmo::sim_scale(world);
    let mut g = world.resource_mut::<Game>();
    put_back(&mut g, &s);
    // (The arena's frame moves the clap on by its frame and poses the forearm from it.)
    g.clap_before(s.clap, dt);
}
/// After the round's frame (the viewer's `camera`): the state put back again over what the arena's frame did with it,
/// the camera put where it was, the gun in hand as it was drawn, the display drawn again, and the damage indicators
/// told what the screen heard (after a seek, the last few seconds' heard again, each at its own moment). Whether the
/// replay is watched through a recorded view this frame.
pub fn after(world: &mut World, views: &mut Views) -> bool {
    let Some((_, s)) = views.now else { return false };
    world.init_resource::<Shown>();
    {
        let mut g = world.resource_mut::<Game>();
        put_back(&mut g, &s);
    }
    crate::arena::shown::ensure_viewmodels(world);
    if views.fresh {
        forget(world);
        // Heard again in order, each at its own moment (the indicators fade and the slow motion runs by their clock).
        let past: Vec<(Sample, Event)> = views.heard.iter().filter(|(e, _)| e.t < s.t - 1.).cloned().collect();
        for (at, e) in past {
            if let Some(d) = damage_of(&e) {
                *world.resource_mut::<Shown>() = Shown { now: Some(at.now as f32), heard: Some(vec![d]) };
                crate::damage::update(world);
            }
        }
        views.heard.retain(|(e, _)| e.t >= s.t - 1.);
        views.fresh = false;
    }
    let heard: Vec<crate::damage::Event> = views.heard.iter().filter_map(|(_, e)| damage_of(e)).collect();
    // The shots this player fired, heard as it heard them (the round's own frames tell the others').
    let shots: Vec<u8> = views.heard.iter().filter_map(|(_, e)| if let Event::Shot { weapon, .. } = e { Some(*weapon) } else { None }).collect();
    for weapon in shots {
        crate::arena::remote_shot(world, Vec3::ZERO, Vec3::ZERO, false, weapon as usize);
    }
    *world.resource_mut::<Shown>() = Shown { now: Some(s.now as f32), heard: Some(heard) };
    place(world, &s);
    // (The display drew the crosshair before the frame's kill was heard: as the cross it was, then the clock put back.)
    world.resource_mut::<Game>().put_back(crate::arena::shown::Private { hit: if s.has(flag::CROSS) { s.hit.max(1e-3) } else { 0. }, clap: s.clap, eye: s.eye }, (s.burst as u32, s.bursts as u32));
    crate::arena::shown::redraw_hud(world);
    world.resource_mut::<Game>().put_back(crate::arena::shown::Private { hit: s.hit, clap: s.clap, eye: s.eye }, (s.burst as u32, s.bursts as u32));
    true
}
/// What was shown is forgotten (a seek, another view, back to the reconstruction): the indicators, the marker and the
/// slow motion start again from what is heard next.
fn forget(world: &mut World) {
    if let Some(mut hurt) = world.get_resource_mut::<crate::damage::Hurt>() {
        (hurt.arcs, hurt.ring, hurt.marker) = (vec![], None, None);
        hurt.watch = crate::damage::Watch::default();
    }
    if let Some(mut s) = world.get_resource_mut::<crate::slowmo::SlowMo>() {
        s.start = None;
    }
}
/// The camera exactly where it was drawn (and the view model's camera with it), and the first-person gun as it was.
pub fn place(world: &mut World, s: &Sample) {
    let Some(cam) = world.get_resource::<Game>().and_then(|g| g.camera()) else { return };
    let tr = Transform::from_translation(Vec3::from_array(s.cam)).with_rotation(bevy_quat(pov::quat(s.turn)));
    if world.get::<ChildOf>(cam).is_some() {
        world.entity_mut(cam).remove::<ChildOf>();
    }
    if let Some(mut t) = world.get_mut::<Transform>(cam)
        && *t != tr
    {
        *t = tr;
    }
    if let Some(mut projection) = world.get_mut::<Projection>(cam)
        && let Projection::Perspective(p) = projection.as_mut()
        && p.fov != s.fov
    {
        p.fov = s.fov;
    }
    let views: Vec<Entity> = world.query_filtered::<Entity, With<crate::viewmodel::ViewCamera>>().iter(world).collect();
    for e in views {
        if let Some(mut t) = world.get_mut::<Transform>(e)
            && *t != tr
        {
            *t = tr;
        }
    }
    let roots: Vec<(Weapon, Entity)> = world.resource::<Game>().viewmodels().to_vec();
    for (weapon, root) in roots {
        let shown = s.gun != 0 && weapon as u8 == s.gun;
        let want = if shown { Visibility::Visible } else { Visibility::Hidden };
        if world.get::<Visibility>(root) != Some(&want) {
            world.entity_mut(root).insert(want);
        }
        if shown {
            let place = Transform::from_translation(Vec3::from_array(s.hold)).with_rotation(bevy_quat(pov::quat(s.tilt)));
            if let Some(mut t) = world.get_mut::<Transform>(root)
                && (t.translation, t.rotation) != (place.translation, place.rotation)
            {
                (t.translation, t.rotation) = (place.translation, place.rotation);
            }
        }
    }
}

#[cfg(test)]
#[path = "pov_tests.rs"]
mod tests;

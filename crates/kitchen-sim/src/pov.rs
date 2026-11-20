// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! A player's own view of a recorded game (a POV track): what that player's screen showed, frame by frame, kept beside
//! the replay (replay.rs) so that a viewer can show it again as it was, not as a spectator reconstructs it.
//!
//! **What the client keeps** (the game's replay/pov.rs): for every frame it drew (at most `RATE` a second), stamped on
//! the match clock (online the server's steps as its timeline drew them, in ms; single player the round's clock):
//!
//! * the game camera exactly as it was drawn (after recoil, shake, the scope, a third-person or death camera) and its
//!   field of view; which view that was (`View`);
//! * the first-person gun as it was drawn: which one, and its root's place and turn in the camera's frame (recoil, the
//!   melee swing, a reload's tilt, the rifle raised to the eye all in it);
//! * what the presentation reads of the local player: its predicted place and look, its eye, stance, health, the gun in
//!   hand, the magazine and reserve, the aim and the scope, the kill cross, the reload's progress, the stim;
//! * for diagnosis: the frame's length, the real clock, the buttons held and the mouse's move;
//! * and discrete events, each on the frame it happened (`Event`): shots, reloads, a change of gun, the scope up or
//!   down, melee, Kung Fu moves, the hit and kill markers and the damage indicators as the client heard them, slow
//!   motion, relish on the lens, pickups, carrying, the card table, the graphics level, and the seeds of anything the
//!   client randomised.
//!
//! **Encoding.** A track is a `Header` (JSON) and batches of about a second each. A batch is self-contained (so a lost
//! one loses only itself, and the server can check each alone): its samples, each a set of integer channels
//! (`CHANNELS`: quantised, see `Ch`) written as a mask of the channels whose value is not what was predicted, then
//! each of those residuals (zigzag varints); predicted from the sample before (steps) or the two before (smooth
//! motion: the camera, the clocks), the first sample of a batch against zero. Then its events. The whole batch is
//! zlib'd (miniz_oxide, as the replay's chunks). Positions are kept to 2^-16 m (2^-18 m for the gun in the camera's
//! frame), angles to 2^-20 of a turn, the field of view to 2^-16 rad: far finer than a pixel; the aim, a reload's
//! progress and the stim clap to 2^-16. Times are 1/64 ms; the real clock is kept as the very f32 the game read.
//!
//! **Validated** (`check`): the server never plays a track; it checks each batch's size, that it inflates within
//! `INFLATE`, decodes whole, its clock runs forward, and its counts are within what `RATE` allows, before keeping it.
use crate::net::{Malformed, Reader, Writer};
/// The quaternions turns are worked out with (`quat`, `euler`).
pub use glam::Quat;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

/// The format of a track this build writes and reads.
pub const FORMAT: u32 = 1;
/// Frames a second kept at most (a faster screen's frames in between are left out; a viewer interpolates them).
pub const RATE: u32 = 120;
/// Seconds a batch spans (it is closed sooner if it grows near `BATCH`).
pub const SPAN: f64 = 1.;
/// The most a batch may be, zlib'd: with its base64 and JSON it goes in one websocket message (16 KiB at most).
pub const BATCH: usize = 10 * 1024;
/// The most a batch may inflate to.
pub const INFLATE: usize = 512 * 1024;
/// The most samples and events a batch may hold.
pub const MOST_SAMPLES: usize = 4 * RATE as usize;
pub const MOST_EVENTS: usize = 2048;
/// The most a header may be (its JSON).
pub const HEADER: usize = 4096;

/// What the camera showed, by what put it there.
pub mod view {
    /// Through the player's eyes (the gun in hand, the scope).
    pub const FIRST: u8 = 0;
    /// Kung Fu over the shoulder: the whole body seen.
    pub const THIRD: u8 = 1;
    /// Dead: the death camera round the body.
    pub const DEAD: u8 = 2;
    /// At Fully Loaded's card table.
    pub const TABLE: u8 = 3;
    /// The Esc menu open over the game (single player is paused under it).
    pub const MENU: u8 = 4;
    /// Watching (the demo, between rounds, the round over).
    pub const SPECTATOR: u8 = 5;
    pub const NAMES: [&str; 6] = ["first person", "third person", "death camera", "card table", "menu", "spectator"];
}
/// The bits of `Sample::flags`.
pub mod flag {
    pub const SCOPED: u32 = 1;
    pub const ALIVE: u32 = 2;
    pub const PRONE: u32 = 4;
    pub const CROUCH: u32 = 8;
    pub const AIRBORNE: u32 = 16;
    pub const CARRYING: u32 = 32;
    pub const TABLE: u32 = 64;
    pub const KUNGFU: u32 = 128;
    /// The pointer captured (the mouse aims).
    pub const CAPTURED: u32 = 256;
    pub const MENU: u32 = 512;
    /// The stim's forearm is up in the view.
    pub const STIM: u32 = 1024;
    /// Behind a Forts forcefield: the guns locked, held lowered.
    pub const LOCKED: u32 = 2048;
    /// The world was drawn for the player (not the demo, a cutscene, the lobby).
    pub const PLAYING: u32 = 4096;
    /// The crosshair was drawn as the kill's red cross (the display drew it before the frame's kill was heard, so the
    /// kill cross's own clock, `Sample::hit`, runs a frame ahead of it).
    pub const CROSS: u32 = 8192;
}

/// What a track says of itself: who, what drew it, and what of its settings changes the picture.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Header {
    pub format: u32,
    /// The hotdog it is the view of, and its name.
    pub player: u32,
    pub name: String,
    /// The program that drew it ("hotdog-simulator 0.1.0"), the renderer ("Vulkan: NVIDIA ..."), the graphics level
    /// it began at, and the window: its size in physical pixels, its aspect and the share of it drawn (a browser's scale).
    pub build: String,
    pub renderer: String,
    pub quality: String,
    pub width: u32,
    pub height: u32,
    pub aspect: f32,
    pub scale: f32,
    /// The settings that change the picture: the field of view (degrees), the scope held or toggled, the sensitivity.
    pub fov: f32,
    pub scope_toggle: bool,
    pub sensitivity: f32,
    /// The most samples a second it keeps.
    pub rate: u32,
    /// "server": the clock is the server's steps as this client's timeline drew them (ms since the server began);
    /// "round": single player's round clock (ms). A replay puts its own start on that clock beside the track.
    pub clock: String,
    /// "online" or "offline", and Unix seconds it began.
    pub source: String,
    pub started: u64,
}
impl Header {
    pub fn text(&self) -> String {
        serde_json::to_string(&Header { format: FORMAT, ..self.clone() }).unwrap_or_default()
    }
    /// A header's JSON, checked: at most HEADER bytes, of a format this build reads.
    pub fn parse(text: &str) -> Result<Header, Malformed> {
        if text.len() > HEADER {
            return Err(Malformed("a track's header too large"));
        }
        let h = serde_json::from_str::<Header>(text).map_err(|_| Malformed("a track's header that does not read"))?;
        if h.format != FORMAT {
            return Err(Malformed("a track of another format"));
        }
        Ok(h)
    }
}

/// One frame as the player saw it (see the module's documentation).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Milliseconds on the track's clock (a replay's own once read from a file: `Track`).
    pub t: f64,
    /// The client's real clock (seconds since it started: kept exactly as the f32 the game's presentation reads, so that
    /// what goes by it, the indicators' fades, the slow motion, goes exactly as it went) and the frame's length (seconds).
    pub now: f64,
    pub dt: f32,
    /// What the camera showed (`view`), and `flag` bits.
    pub view: u8,
    pub flags: u32,
    /// The game camera as drawn: where, turned how (yaw, pitch, roll: Bevy's `EulerRot::YXZ`), its vertical field of
    /// view (radians).
    pub cam: [f32; 3],
    pub turn: [f32; 3],
    pub fov: f32,
    /// The player as its client had it: where it stood (predicted online), its look, the eye's height over its feet.
    pub at: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub eye: f32,
    /// The first-person gun as drawn: its weapon (0: none shown), its root's place and turn in the camera's frame.
    pub gun: u8,
    pub hold: [f32; 3],
    pub tilt: [f32; 3],
    /// The gun in hand, the aim (the rifle raised, 0 to 1), the kill cross (seconds left), the reload shown (its gun,
    /// 0 for none, and how far through it is), the stim clap (seconds into it, to 1) and stim (seconds left).
    pub weapon: u8,
    pub aim: f32,
    pub hit: f32,
    pub reload: u8,
    pub progress: f32,
    pub clap: f32,
    pub stim: f32,
    /// Health, every gun's magazine and reserve as the display had them (by gun, 1 to 7: index 0 is the first gun),
    /// kills and score.
    pub hp: f32,
    pub ammo: [i32; GUNS],
    pub reserve: [i32; GUNS],
    pub kills: u32,
    pub score: u32,
    /// The syrup rifle's burst as its optic shows it: rounds ready, and the burst's size.
    pub burst: u8,
    pub bursts: u8,
    /// For diagnosis: the actions held (controls.rs's order, one bit each) and the mouse's move (pixels, as read).
    pub buttons: u64,
    pub look: [f32; 2],
    /// The graphics level (quality.rs `Level` as its number).
    pub quality: u8,
}

/// How a channel is kept: its unit and how it is predicted.
#[derive(Clone, Copy, PartialEq)]
enum Way {
    /// The value itself (it is near zero: a mouse move).
    Raw,
    /// The value before.
    Step,
    /// Carried on from the two before (smooth motion).
    Smooth,
    /// A turn (2^-20 of one), carried on from the two before, the short way round.
    Angle,
    /// A turn, from the one before.
    AngleStep,
}
/// The guns a sample keeps the magazines and reserves of (1 to 7).
pub const GUNS: usize = 7;
const TURN: i64 = 1 << 20;
const M: f64 = 65536.;
const M_FINE: f64 = 262144.;
const MS: f64 = 64.;
/// The channels, in the order they are kept (those that change most often first: their mask bits are the low ones).
const CHANNELS: [(Way, f64); 43 + 2 * (GUNS - 1)] = [
    (Way::Smooth, MS),       // 0 t (ms)
    (Way::Smooth, 0.),       // 1 now (s, as the f32 the game reads it as: its bits)
    (Way::Smooth, M),        // 2-4 camera
    (Way::Smooth, M),
    (Way::Smooth, M),
    (Way::Angle, 0.),        // 5-7 its turn
    (Way::Angle, 0.),
    (Way::Angle, 0.),
    (Way::Smooth, M),        // 8-10 the player
    (Way::Smooth, M),
    (Way::Smooth, M),
    (Way::Angle, 0.),        // 11-12 its look
    (Way::Angle, 0.),
    (Way::Raw, 16.),         // 13-14 the mouse
    (Way::Raw, 16.),
    (Way::Step, M_FINE),     // 15-17 the gun's root
    (Way::Step, M_FINE),
    (Way::Step, M_FINE),
    (Way::AngleStep, 0.),    // 18-20 its turn
    (Way::AngleStep, 0.),
    (Way::AngleStep, 0.),
    (Way::Smooth, M),        // 21 fov
    (Way::Step, MS * 1e3),   // 22 dt (s)
    (Way::Step, 1.),         // 23 view
    (Way::Step, 1.),         // 24 flags
    (Way::Step, M),          // 25 eye
    (Way::Step, 1.),         // 26 gun
    (Way::Step, 1.),         // 27 weapon
    (Way::Step, 65536.),     // 28 aim
    (Way::Smooth, 1024.),    // 29 hit
    (Way::Step, 1.),         // 30 reload
    (Way::Smooth, 65536.),   // 31 progress
    (Way::Smooth, 65536.),   // 32 clap
    (Way::Smooth, 1024.),    // 33 stim
    (Way::Step, 100.),       // 34 hp
    (Way::Step, 1.),         // 35 kills
    (Way::Step, 1.),         // 38 score
    (Way::Step, 1.),         // 39 burst
    (Way::Step, 1.),         // 40 bursts
    (Way::Step, 1.),         // 39 buttons
    (Way::Step, 1.),         // 40 quality
    (Way::Step, 1.),         // 41-47 the magazines
    (Way::Step, 1.),
    (Way::Step, 1.),
    (Way::Step, 1.),
    (Way::Step, 1.),
    (Way::Step, 1.),
    (Way::Step, 1.),
    (Way::Step, 1.),         // 48-54 the reserves
    (Way::Step, 1.),
    (Way::Step, 1.),
    (Way::Step, 1.),
    (Way::Step, 1.),
    (Way::Step, 1.),
    (Way::Step, 1.),
];
const N: usize = CHANNELS.len();

fn q(v: f64, unit: f64) -> i64 {
    (v * unit).round() as i64
}
fn turn_q(radians: f32) -> i64 {
    ((radians as f64 / TAU as f64 * TURN as f64).round() as i64).rem_euclid(TURN)
}
fn turn_of(v: i64) -> f32 {
    let v = v.rem_euclid(TURN);
    let v = if v >= TURN / 2 { v - TURN } else { v };
    (v as f64 / TURN as f64 * TAU as f64) as f32
}
/// A turn's difference the short way round.
fn wrap(d: i64) -> i64 {
    (d + TURN / 2).rem_euclid(TURN) - TURN / 2
}
impl Sample {
    /// The sample as its channels' integers.
    fn channels(&self) -> [i64; N] {
        let mut c = [0i64; N];
        let mut k = 0;
        let mut put = |c: &mut [i64; N], v: i64| {
            c[k] = v;
            k += 1;
        };
        put(&mut c, q(self.t, MS));
        put(&mut c, (self.now as f32).to_bits() as i64);
        for v in self.cam {
            put(&mut c, q(v as f64, M));
        }
        for v in self.turn {
            put(&mut c, turn_q(v));
        }
        for v in self.at {
            put(&mut c, q(v as f64, M));
        }
        put(&mut c, turn_q(self.yaw));
        put(&mut c, turn_q(self.pitch));
        for v in self.look {
            put(&mut c, q(v as f64, 16.));
        }
        for v in self.hold {
            put(&mut c, q(v as f64, M_FINE));
        }
        for v in self.tilt {
            put(&mut c, turn_q(v));
        }
        put(&mut c, q(self.fov as f64, M));
        put(&mut c, q(self.dt as f64, MS * 1e3));
        put(&mut c, self.view as i64);
        put(&mut c, self.flags as i64);
        put(&mut c, q(self.eye as f64, M));
        put(&mut c, self.gun as i64);
        put(&mut c, self.weapon as i64);
        put(&mut c, q(self.aim as f64, 65536.));
        put(&mut c, q(self.hit as f64, 1024.));
        put(&mut c, self.reload as i64);
        put(&mut c, q(self.progress as f64, 65536.));
        put(&mut c, q(self.clap as f64, 65536.));
        put(&mut c, q(self.stim as f64, 1024.));
        put(&mut c, q(self.hp as f64, 100.));
        put(&mut c, self.kills as i64);
        put(&mut c, self.score as i64);
        put(&mut c, self.burst as i64);
        put(&mut c, self.bursts as i64);
        put(&mut c, self.buttons as i64);
        put(&mut c, self.quality as i64);
        for v in self.ammo.iter().chain(&self.reserve) {
            put(&mut c, *v as i64);
        }
        debug_assert_eq!(k, N);
        c
    }
    /// A sample from its channels' integers.
    fn from_channels(c: &[i64; N]) -> Sample {
        let mut k = 0;
        let mut next = || {
            k += 1;
            c[k - 1]
        };
        let f = |v: i64, unit: f64| (v as f64 / unit) as f32;
        let t = next() as f64 / MS;
        let now = f32::from_bits(next() as u32) as f64;
        let cam = [f(next(), M), f(next(), M), f(next(), M)];
        let turn = [turn_of(next()), turn_of(next()), turn_of(next())];
        let at = [f(next(), M), f(next(), M), f(next(), M)];
        let (yaw, pitch) = (turn_of(next()), turn_of(next()));
        let look = [f(next(), 16.), f(next(), 16.)];
        let hold = [f(next(), M_FINE), f(next(), M_FINE), f(next(), M_FINE)];
        let tilt = [turn_of(next()), turn_of(next()), turn_of(next())];
        let fov = f(next(), M);
        let dt = f(next(), MS * 1e3);
        let (view, flags, eye, gun, weapon) = (next() as u8, next() as u32, f(next(), M), next() as u8, next() as u8);
        let (aim, hit, reload, progress) = (f(next(), 65536.), f(next(), 1024.), next() as u8, f(next(), 65536.));
        let (clap, stim, hp) = (f(next(), 65536.), f(next(), 1024.), f(next(), 100.));
        let (kills, score) = (next() as u32, next() as u32);
        let (burst, bursts, buttons, quality) = (next() as u8, next() as u8, next() as u64, next() as u8);
        let ammo: [i32; GUNS] = std::array::from_fn(|_| next() as i32);
        let reserve: [i32; GUNS] = std::array::from_fn(|_| next() as i32);
        Sample { t, now, dt, view, flags, cam, turn, fov, at, yaw, pitch, eye, gun, hold, tilt, weapon, aim, hit, reload, progress, clap, stim, hp, ammo, reserve, kills, score, burst, bursts, buttons, look, quality }
    }
    /// The sample as kept: each channel rounded to its unit (what a reader of the track gets back).
    pub fn kept(&self) -> Sample {
        Sample::from_channels(&self.channels())
    }
    pub fn has(&self, bit: u32) -> bool {
        self.flags & bit != 0
    }
}
/// What channel `k` is predicted to be from the two values before it, `seen` samples into the batch (the first is
/// against zero, the second against the first).
fn predict(k: usize, prev: &[i64; N], prev2: &[i64; N], seen: usize) -> i64 {
    match (CHANNELS[k].0, seen) {
        (Way::Raw, _) | (_, 0) => 0,
        (Way::Step | Way::AngleStep, _) | (Way::Smooth | Way::Angle, 1) => prev[k],
        (Way::Smooth, _) => 2 * prev[k] - prev2[k],
        (Way::Angle, _) => (prev[k] + wrap(prev[k] - prev2[k])).rem_euclid(TURN),
    }
}
fn residual(k: usize, value: i64, guess: i64) -> i64 {
    match CHANNELS[k].0 {
        Way::Angle | Way::AngleStep => wrap(value - guess),
        _ => value - guess,
    }
}
fn restore(k: usize, guess: i64, residual: i64) -> i64 {
    match CHANNELS[k].0 {
        Way::Angle | Way::AngleStep => (guess + residual).rem_euclid(TURN),
        _ => guess + residual,
    }
}

/// Something that happened on the player's screen, on a sample's frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// A gun fired (the trigger as the client saw it): which, and the aim (yaw and pitch, radians).
    Shot { weapon: u8, yaw: f32, pitch: f32 },
    /// A reload of `gun` began (0), was cut short (1) or finished (2).
    Reload { gun: u8, phase: u8 },
    /// Another gun in hand (0: the hands full).
    Weapon { to: u8 },
    /// The scope or the rifle's optic up or down.
    Scope { up: bool },
    /// A melee strike with `weapon` began.
    Melee { weapon: u8 },
    /// Kung Fu: this player's fighter began a move (`kungfu::Move` as its number) in a style, as the client saw it.
    Move { style: u8, mv: u8 },
    /// The damage indicators and the hit marker heard (the game's damage.rs `Event`): this player hurt (the record).
    Hurt { damage: crate::feast::Damage },
    /// A hotdog this player hurt (and killed), and where its chest was.
    Dealt { victim: u32, kill: bool, at: [f32; 3] },
    /// This player's kills went up with no hotdog's death to show for it (a cat).
    Kills,
    /// A death ended the round.
    Final { victim: u32, at: [f32; 3] },
    /// Kill slow motion began (for the screen alone, or the round with it), towards where the victim fell.
    Slow { shown_only: bool, focus: Option<[f32; 3]> },
    /// Relish on the lens: how many smudges were on it after.
    Lens { smudges: u32 },
    /// A pickup taken (its spawn point).
    Pickup { index: u32 },
    /// Carrying: picked up (0), put down (1), thrown (2), turned (3), tipped (4); the pose's number after.
    Carry { what: u8, pose: u8 },
    /// At the card table: a flip (0), a stop (1), a hotdog picked (2, its seat).
    Card { what: u8, seat: u32 },
    /// The graphics level changed (quality.rs `Level` as its number, and its name).
    Quality { level: u8, name: String },
    /// A seed something on the screen was randomised with (`what` says what: the card shuffle).
    Seed { what: String, value: u64 },
    /// Anything else, as words.
    Note { text: String },
}
impl Event {
    fn kind(&self) -> u8 {
        match self {
            Event::Shot { .. } => 1,
            Event::Reload { .. } => 2,
            Event::Weapon { .. } => 3,
            Event::Scope { .. } => 4,
            Event::Melee { .. } => 5,
            Event::Move { .. } => 6,
            Event::Hurt { .. } => 7,
            Event::Dealt { .. } => 8,
            Event::Kills => 9,
            Event::Final { .. } => 10,
            Event::Slow { .. } => 11,
            Event::Lens { .. } => 12,
            Event::Pickup { .. } => 13,
            Event::Carry { .. } => 14,
            Event::Card { .. } => 15,
            Event::Quality { .. } => 16,
            Event::Seed { .. } => 17,
            Event::Note { .. } => 18,
        }
    }
    /// A short name, for the viewer's list and the report.
    pub fn name(&self) -> &'static str {
        ["", "shot", "reload", "weapon", "scope", "melee", "move", "hurt", "dealt", "kills", "final", "slow", "lens", "pickup", "carry", "card", "quality", "seed", "note"][self.kind() as usize]
    }
    fn write(&self, w: &mut Writer) {
        let point = |w: &mut Writer, p: &[f32; 3]| p.iter().for_each(|v| w.zigzag(q(*v as f64, M)));
        w.u8(self.kind());
        match self {
            Event::Shot { weapon, yaw, pitch } => {
                w.u8(*weapon);
                w.varint(turn_q(*yaw) as u64);
                w.varint(turn_q(*pitch) as u64);
            }
            Event::Reload { gun, phase } => {
                w.u8(*gun);
                w.u8(*phase);
            }
            Event::Weapon { to } => w.u8(*to),
            Event::Scope { up } => w.u8(*up as u8),
            Event::Melee { weapon } => w.u8(*weapon),
            Event::Move { style, mv } => {
                w.u8(*style);
                w.u8(*mv);
            }
            Event::Hurt { damage } => w.text(&serde_json::to_string(damage).unwrap_or_default()),
            Event::Dealt { victim, kill, at } => {
                w.varint(*victim as u64);
                w.u8(*kill as u8);
                point(w, at);
            }
            Event::Kills => {}
            Event::Final { victim, at } => {
                w.varint(*victim as u64);
                point(w, at);
            }
            Event::Slow { shown_only, focus } => {
                w.u8(*shown_only as u8 | (focus.is_some() as u8) << 1);
                if let Some(f) = focus {
                    point(w, f);
                }
            }
            Event::Lens { smudges } => w.varint(*smudges as u64),
            Event::Pickup { index } => w.varint(*index as u64),
            Event::Carry { what, pose } => {
                w.u8(*what);
                w.u8(*pose);
            }
            Event::Card { what, seat } => {
                w.u8(*what);
                w.varint(*seat as u64);
            }
            Event::Quality { level, name } => {
                w.u8(*level);
                w.text(name);
            }
            Event::Seed { what, value } => {
                w.text(what);
                w.varint(*value);
            }
            Event::Note { text } => w.text(text),
        }
    }
    fn read(r: &mut Reader) -> Result<Event, Malformed> {
        let point = |r: &mut Reader| -> Result<[f32; 3], Malformed> { Ok([(r.zigzag()? as f64 / M) as f32, (r.zigzag()? as f64 / M) as f32, (r.zigzag()? as f64 / M) as f32]) };
        let small = |r: &mut Reader| -> Result<u32, Malformed> { u32::try_from(r.varint()?).map_err(|_| Malformed("a number too large")) };
        Ok(match r.u8()? {
            1 => Event::Shot { weapon: r.u8()?, yaw: turn_of(r.varint()? as i64), pitch: turn_of(r.varint()? as i64) },
            2 => Event::Reload { gun: r.u8()?, phase: r.u8()? },
            3 => Event::Weapon { to: r.u8()? },
            4 => Event::Scope { up: r.u8()? != 0 },
            5 => Event::Melee { weapon: r.u8()? },
            6 => Event::Move { style: r.u8()?, mv: r.u8()? },
            7 => Event::Hurt { damage: serde_json::from_str(&r.text()?).map_err(|_| Malformed("a damage record that does not read"))? },
            8 => Event::Dealt { victim: small(r)?, kill: r.u8()? != 0, at: point(r)? },
            9 => Event::Kills,
            10 => Event::Final { victim: small(r)?, at: point(r)? },
            11 => {
                let bits = r.u8()?;
                Event::Slow { shown_only: bits & 1 != 0, focus: if bits & 2 != 0 { Some(point(r)?) } else { None } }
            }
            12 => Event::Lens { smudges: small(r)? },
            13 => Event::Pickup { index: small(r)? },
            14 => Event::Carry { what: r.u8()?, pose: r.u8()? },
            15 => Event::Card { what: r.u8()?, seat: small(r)? },
            16 => Event::Quality { level: r.u8()?, name: r.text()? },
            17 => Event::Seed { what: r.text()?, value: r.varint()? },
            18 => Event::Note { text: r.text()? },
            _ => return Err(Malformed("an event of a kind this build does not know")),
        })
    }
}
/// An event on the frame of a batch's sample `at` (its index in the batch, or in a whole track).
#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    pub at: usize,
    pub event: Event,
}

// ----- Batches.

/// A batch being made (see the module's documentation).
#[derive(Default)]
pub struct Batch {
    pub samples: Vec<Sample>,
    pub events: Vec<Timed>,
}
impl Batch {
    /// The span of its samples (ms on the track's clock), if it has any.
    pub fn span(&self) -> Option<(f64, f64)> {
        Some((self.samples.first()?.t, self.samples.last()?.t))
    }
    /// An event on the newest sample's frame (or on the next sample's, when there is none yet).
    pub fn event(&mut self, event: Event) {
        let at = self.samples.len().saturating_sub(1);
        self.events.push(Timed { at, event });
    }
    /// The batch's bytes, uncompressed.
    pub fn raw(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(self.samples.len() as u64);
        let (mut prev, mut prev2) = ([0i64; N], [0i64; N]);
        for (seen, s) in self.samples.iter().enumerate() {
            let c = s.channels();
            let mut mask = 0u64;
            let mut out = [0i64; N];
            for k in 0..N {
                out[k] = residual(k, c[k], predict(k, &prev, &prev2, seen));
                if out[k] != 0 {
                    mask |= 1 << k;
                }
            }
            w.varint(mask);
            for (k, r) in out.iter().enumerate() {
                if mask & (1 << k) != 0 {
                    w.zigzag(*r);
                }
            }
            (prev2, prev) = (prev, c);
        }
        w.varint(self.events.len() as u64);
        for e in &self.events {
            w.varint(e.at as u64);
            e.event.write(&mut w);
        }
        w.bytes
    }
    /// The batch zlib'd: what is sent and kept.
    pub fn encode(&self) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(&self.raw(), 6)
    }
    /// A batch's bytes read back (and checked: see `check`).
    pub fn decode(bytes: &[u8]) -> Result<Batch, Malformed> {
        if bytes.len() > BATCH {
            return Err(Malformed("a batch too large"));
        }
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(bytes, INFLATE).map_err(|_| Malformed("a batch that does not inflate"))?;
        Batch::read(&raw)
    }
    fn read(raw: &[u8]) -> Result<Batch, Malformed> {
        let mut r = Reader::new(raw);
        let n = r.count()?;
        if n > MOST_SAMPLES {
            return Err(Malformed("too many samples in a batch"));
        }
        let mut samples = Vec::with_capacity(n);
        let (mut prev, mut prev2) = ([0i64; N], [0i64; N]);
        for seen in 0..n {
            let mask = r.varint()?;
            if mask >> N != 0 {
                return Err(Malformed("a channel this build does not know"));
            }
            let mut c = [0i64; N];
            for k in 0..N {
                let guess = predict(k, &prev, &prev2, seen);
                let res = if mask & (1 << k) != 0 { r.zigzag()? } else { 0 };
                c[k] = restore(k, guess, res);
            }
            samples.push(Sample::from_channels(&c));
            (prev2, prev) = (prev, c);
        }
        let m = r.count()?;
        if m > MOST_EVENTS {
            return Err(Malformed("too many events in a batch"));
        }
        let mut events = Vec::with_capacity(m);
        for _ in 0..m {
            let at = r.varint()? as usize;
            if at >= n.max(1) {
                return Err(Malformed("an event on a frame the batch does not have"));
            }
            events.push(Timed { at, event: Event::read(&mut r)? });
        }
        if !r.done() {
            return Err(Malformed("bytes after a batch's end"));
        }
        Ok(Batch { samples, events })
    }
}

/// What the server checks of a batch before it keeps it (it never plays one): it reads whole (`Batch::decode`), its
/// clock runs forward and is a number, its samples are no more than `RATE` a second allows over its span (and a few),
/// and its span is between `from` and `to` (ms on the track's clock). Returns the batch's span.
pub fn check(bytes: &[u8], from: f64, to: f64) -> Result<(f64, f64), Malformed> {
    let batch = Batch::decode(bytes)?;
    let Some((a, b)) = batch.span() else {
        return Err(Malformed("a batch with no samples"));
    };
    if !a.is_finite() || !b.is_finite() || batch.samples.windows(2).any(|w| w[1].t < w[0].t) {
        return Err(Malformed("a batch whose clock runs backwards"));
    }
    if batch.samples.iter().any(|s| !s.now.is_finite() || s.now < 0.) {
        return Err(Malformed("a batch with a broken clock"));
    }
    if a < from || b > to {
        return Err(Malformed("a batch outside the match"));
    }
    let allowed = ((b - a) / 1000. * RATE as f64 * 1.25).ceil() as usize + 8;
    if batch.samples.len() > allowed {
        return Err(Malformed("a batch with more frames than the rate allows"));
    }
    Ok((a, b))
}

// ----- A whole track.

/// A player's track read back: its header, every sample in order (times on the replay's clock), every event (`at`:
/// the index of its sample).
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub header: Header,
    pub samples: Vec<Sample>,
    pub events: Vec<Timed>,
    /// Why it ended early, when the server dropped it.
    pub dropped: Option<String>,
}
impl Track {
    /// A track from its header and its batches' bytes (each zlib'd as sent), on a clock that `offset` (ms on the
    /// track's clock) is the start of. Batches out of order are put in order; one that does not read is left out.
    pub fn from_batches<'a>(header: Header, offset: f64, batches: impl Iterator<Item = &'a [u8]>) -> Track {
        let mut all: Vec<Batch> = batches.filter_map(|b| Batch::decode(b).ok()).filter(|b| !b.samples.is_empty()).collect();
        all.sort_by(|a, b| a.samples[0].t.total_cmp(&b.samples[0].t));
        let mut track = Track { header, ..Default::default() };
        // (Each list at its size at once: grown by doubling it would take up to twice that for a moment, which a
        // browser's memory keeps.)
        track.samples.reserve_exact(all.iter().map(|b| b.samples.len()).sum());
        track.events.reserve_exact(all.iter().map(|b| b.events.len()).sum());
        for b in all {
            // (A batch that overlaps the one before, a resend, is left out.)
            if track.samples.last().is_some_and(|l| b.samples[0].t <= l.t) {
                continue;
            }
            let base = track.samples.len();
            track.samples.extend(b.samples.into_iter().map(|s| Sample { t: s.t - offset, ..s }));
            track.events.extend(b.events.into_iter().map(|e| Timed { at: base + e.at, event: e.event }));
        }
        track
    }
    /// The span it covers (ms on the replay's clock).
    pub fn span(&self) -> Option<(f64, f64)> {
        Some((self.samples.first()?.t, self.samples.last()?.t))
    }
    /// The index of the last sample at or before `t` (None before the first).
    pub fn at(&self, t: f64) -> Option<usize> {
        let i = self.samples.partition_point(|s| s.t <= t);
        i.checked_sub(1)
    }
    /// The view at `t`: the sample there, or between the two either side (the camera and the gun interpolated, the
    /// rest as the earlier has it). Exact at a sample's own time (within 1/128 ms). None outside the track, or across
    /// a gap of more than `GAP` ms (a lost batch, a pause) where there is nothing to interpolate.
    pub fn view(&self, t: f64) -> Option<Sample> {
        let i = self.at(t + 1. / 128.)?;
        let a = &self.samples[i];
        if (t - a.t).abs() <= 1. / 128. {
            return Some(*a);
        }
        let b = self.samples.get(i + 1)?;
        if b.t - a.t > GAP {
            return None;
        }
        Some(between(a, b, ((t - a.t) / (b.t - a.t)) as f32))
    }
    /// The events from just after `from` to `to` (ms), oldest first, each with its sample.
    pub fn events_between(&self, from: f64, to: f64) -> impl Iterator<Item = (&Sample, &Event)> {
        self.events.iter().filter_map(move |e| {
            let s = self.samples.get(e.at)?;
            (s.t > from && s.t <= to).then_some((s, &e.event))
        })
    }
}
/// The longest stretch (ms) between two samples that a view is interpolated across.
pub const GAP: f64 = 250.;
/// Part of the way `u` from sample `a` to sample `b`: the camera, the player and the gun move between them (turns the
/// short way), the clocks and the progress too; what is on or off, counted or chosen is `a`'s.
pub fn between(a: &Sample, b: &Sample, u: f32) -> Sample {
    let lerp = |x: f32, y: f32| x + (y - x) * u;
    let lerp3 = |x: [f32; 3], y: [f32; 3]| [lerp(x[0], y[0]), lerp(x[1], y[1]), lerp(x[2], y[2])];
    let angle = |x: f32, y: f32| x + ((y - x + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI) * u;
    let mut s = *a;
    s.t = a.t + (b.t - a.t) * u as f64;
    s.now = a.now + (b.now - a.now) * u as f64;
    s.cam = lerp3(a.cam, b.cam);
    s.fov = lerp(a.fov, b.fov);
    s.at = lerp3(a.at, b.at);
    s.yaw = angle(a.yaw, b.yaw);
    s.pitch = angle(a.pitch, b.pitch);
    s.eye = lerp(a.eye, b.eye);
    if a.gun == b.gun {
        s.hold = lerp3(a.hold, b.hold);
    }
    if a.reload == b.reload {
        s.progress = lerp(a.progress, b.progress);
    }
    s.aim = lerp(a.aim, b.aim);
    s.hit = lerp(a.hit, b.hit);
    s.clap = lerp(a.clap, b.clap);
    s.stim = lerp(a.stim, b.stim);
    // (Turns as quaternions, slerped: the camera's, and the gun's while it is the same gun.)
    s.turn = slerp(a.turn, b.turn, u);
    if a.gun == b.gun {
        s.tilt = slerp(a.tilt, b.tilt, u);
    }
    s
}
/// A turn as yaw, pitch and roll (`EulerRot::YXZ`), as a quaternion.
pub fn quat(turn: [f32; 3]) -> glam::Quat {
    glam::Quat::from_euler(glam::EulerRot::YXZ, turn[0], turn[1], turn[2])
}
/// A quaternion as yaw, pitch and roll (`EulerRot::YXZ`).
pub fn euler(q: glam::Quat) -> [f32; 3] {
    let (y, x, z) = q.to_euler(glam::EulerRot::YXZ);
    [y, x, z]
}
fn slerp(a: [f32; 3], b: [f32; 3], u: f32) -> [f32; 3] {
    euler(quat(a).slerp(quat(b), u))
}

// ----- base64, for the websocket's JSON (RFC 4648, with padding).

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(ALPHABET[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
pub fn unbase64(text: &str) -> Result<Vec<u8>, Malformed> {
    let t = text.trim_end_matches('=').as_bytes();
    if t.len() % 4 == 1 || text.len() > 4 * BATCH {
        return Err(Malformed("base64 that does not read"));
    }
    let value = |b: u8| -> Result<u32, Malformed> {
        Ok(match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(Malformed("base64 that does not read")),
        } as u32)
    };
    let mut out = Vec::with_capacity(t.len() * 3 / 4);
    for c in t.chunks(4) {
        let mut n = 0u32;
        for (k, b) in c.iter().enumerate() {
            n |= value(*b)? << (18 - 6 * k);
        }
        for k in 0..c.len() - 1 {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "pov_tests.rs"]
mod tests;

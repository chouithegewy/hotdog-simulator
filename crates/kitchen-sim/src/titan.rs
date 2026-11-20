// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! **Titan** (`mode=titan`, rules.rs `Rules::titan`): one giant hotdog against everyone else. The titan is one of the
//! round's hotdogs (any seat; a person or a bot), SCALE times a hotdog's size: it walks slowly and turns slowly, cannot
//! climb anything a hotdog can stand on nor fit under the furniture, and fights with its body: a charge, a ground smash,
//! a swat (high standing, low crouched), a stomp (a kick crouched) and a heavy jump whose landing is a shockwave. Every
//! other hotdog is a small hotdog playing by the shared gun basics (rules.rs), allied to the rest, coming back from a
//! shared pool of reinforcements. The titan's health scales with the small hotdogs; the back of its head is its weak
//! spot; leg damage drops it to its knees.
//!
//! The interface the client codes against is written up in `.superpowers/sdd/adhoc/titan/interface.md`: this file is
//! the move table (which the client reads its timings from), the titan's body and its hit zones, its movement against
//! the level, the round's rules, the bots of both sides, and what the client is told (`View`, `{type:'titan'}` beside
//! the frames, which are unchanged: protocol 12).
//!
//! **Inputs** (net.rs `Command`, no new bits): the titan's command goes whole to `Feast::titan_command`: the stick walks
//! (the body turning towards the look at TURN), CROUCH crouches, FIRE swats, SCOPE smashes, MELEE stomps (kicks
//! crouched), STIM charges, JUMP jumps, the pitch looks (and nods the head). FIRE and SCOPE are levels whose rising edges
//! are found here; a press that cannot start yet is kept BUFFER seconds.
use crate::actor::{self, Actor};
use crate::feast::{Feast, Player};
use crate::feast::FLOOR;
use crate::map::{Level, Prop};
use crate::net::{Command, button};
use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

// ----- The titan's size: everything comes from SCALE.

/// The map mode (map.rs MODES) a titan arena is tagged with, the rules' and the lobby's name for the mode.
pub const MODE: &str = "titan";
/// How many hotdogs tall the titan stands: every one of its dimensions is a hotdog's (or a hotdog-sized figure's) times
/// this.
pub const SCALE: f32 = 12.;
/// A hotdog-sized figure's measure in metres, times SCALE, is the titan's.
const fn s(hotdog: f32) -> f32 {
    hotdog * SCALE
}
/// Standing, crouched and kneeling (staggered) heights.
pub const TALL: f32 = s(0.15);
pub const CROUCH_TALL: f32 = TALL * 11. / 18.;
pub const KNEEL_TALL: f32 = s(0.0775);
/// Its footprint and body radius: what keeps it off the furniture and the hotdogs off it.
pub const RADIUS: f32 = s(0.03);
/// Its mass (a hotdog's is 0.4): no hotdog shoves it.
pub const MASS: f64 = 400.;
/// Walking, crouched walking and charging speeds (m/s), how long a charge runs at most (s), and how fast it turns
/// (rad/s), and while charging.
pub const WALK: f32 = 0.9;
pub const CROUCH_WALK: f32 = 0.5;
pub const CHARGE_SPEED: f32 = 2.4;
pub const CHARGE_TIME: f32 = 2.5;
pub const TURN: f32 = 1.6;
pub const CHARGE_TURN: f32 = 0.35;
/// Its jump: the apex above the floor (m), and the take-off speed that reaches it under the game's gravity.
pub const JUMP_APEX: f32 = 0.28;
pub const JUMP_SPEED: f32 = 1.252;
/// Health: HP_BASE and HP_PER for each small hotdog. (The spec's 2500 + 900 a small hotdog, tuned by bot matches: five
/// normal small bots against a normal bot titan win about half their games, in about three minutes; see the balance
/// table in titan_tests.rs.)
pub const HP_BASE: f32 = 12500.;
pub const HP_PER: f32 = 4550.;
/// Small hotdogs: how many by default, the fewest and the most (`smalls=N`), the reinforcements each brings to the pool,
/// seconds until a dead one is back, and the round's clock (s).
pub const SMALLS: usize = 5;
pub const SMALLS_MIN: usize = 2;
pub const SMALLS_MAX: usize = 8;
pub const PER_SMALL: u32 = 3;
pub const RESPAWN: f32 = 5.;
pub const TIME: f32 = 300.;
/// Stepping on a small hotdog: the HP it takes and the seconds before the same hotdog can be stepped on again.
pub const STEP_DAMAGE: f32 = 25.;
pub const STEP_WAIT: f32 = 0.8;
/// The stagger: the leg damage (HP, after the legs' multiplier) that drops it to its knees, the seconds over which the
/// meter drains that much, and the seconds after a stagger in which leg damage fills nothing.
pub const STAGGER_LEGS: f32 = 260.;
pub const STAGGER_WINDOW: f32 = 8.;
pub const STAGGER_REST: f32 = 8.;
/// A shockwave (the smash, the landing): the share of its damage a hotdog in the air takes as the front passes, and one in
/// a den (under an overhead); the radius about the impact point in which the fists hit directly; and the highest top (m
/// above the floor) the fists land on.
pub const AIRBORNE_SHARE: f32 = 0.15;
pub const DEN_SHARE: f32 = 0.5;
pub const DIRECT: f32 = 0.3;
pub const REACH_LOW: f32 = 0.5;
/// An overhead box (map.rs `Prop::Box`) makes a den where its underside is at least this high: a small hotdog fits.
pub const DEN_CLEAR: f32 = 0.16;
/// The weak spot and the face on the head (`Zone`): how far from the nape's way (or the face's), seen from the head's
/// middle, their caps reach over the drawn head (radians). The drawn pale patch is this very cap (src/titan/body.rs).
pub const WEAK_ARC: f32 = 0.8;
pub const FACE_ARC: f32 = 0.85;
/// The weak spot shows (`View::exposed`) when the nape's way is no further down than this (its y), with a small hotdog
/// behind the titan.
pub const EXPOSED: f32 = -0.4;
/// Crouched the nape is tucked down this far (radians); kneeling the head is bowed this far forward. (Tucked or tipped
/// back, the nape hides behind the shoulders and the chest: the cap itself is always WEAK_ARC, as drawn.)
pub const CROUCH_TUCK: f32 = 0.5;
pub const KNEEL_BOW: f32 = 0.9;
/// The F strike reaches this much further on the titan (its legs stand inside its footprint, which keeps a hotdog off
/// them).
pub const STRIKE_REACH: f32 = 0.25;
/// Score: points for each weak-spot hit, beyond the HP.
pub const WEAK_BONUS: u32 = 25;
/// Events are told for EVENT_LIFE seconds; the server tells the view at least every SYNC seconds; a press is kept BUFFER
/// seconds.
pub const EVENT_LIFE: f32 = 0.75;
pub const SYNC: f32 = 1.;
pub const BUFFER: f32 = 0.2;
/// What a bot titan is called.
pub const NAME: &str = "TITAN";
/// The small hotdogs' names, by seat.
pub const SMALL_NAMES: [&str; SMALLS_MAX + 1] = ["BUNNER", "FRANKIE", "MUSTARD", "REDDOG", "SESAME", "RELISH", "PICKLE", "CHILI", "KRAUT"];
/// `View::ready`: which attack each place is.
pub const READY_CHARGE: usize = 0;
pub const READY_SMASH: usize = 1;
pub const READY_SWAT: usize = 2;
pub const READY_STOMP: usize = 3;
pub const READY_JUMP: usize = 4;
/// `View::over`.
pub const OVER_NONE: u8 = 0;
pub const OVER_TITAN: u8 = 1;
pub const OVER_SMALLS: u8 = 2;

// ----- Hit zones.

/// Where a round met the titan, and what its damage is multiplied by there (MULTIPLIER): its legs (which fill the stagger
/// meter), its body, its face, its weak spot, and its arms and hands (a limb, as the legs are, but they do not stagger it).
/// (Stable numbers: they travel in the titan's events.)
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Zone {
    Legs = 0,
    #[default]
    Body = 1,
    Face = 2,
    Weak = 3,
    Arms = 4,
}
/// How many zones there are.
pub const ZONES: usize = 5;
impl Zone {
    pub const ALL: [Zone; ZONES] = [Zone::Legs, Zone::Body, Zone::Face, Zone::Weak, Zone::Arms];
    pub fn from_u8(n: u8) -> Option<Zone> {
        Self::ALL.get(n as usize).copied()
    }
    pub fn multiplier(self) -> f32 {
        MULTIPLIER[self as usize]
    }
}
pub const MULTIPLIER: [f32; ZONES] = [0.75, 1., 1.5, 3., 0.75];

// ----- The moves.

/// How many moves there are.
pub const MOVES: usize = 13;
/// Every move, by its stable number (what travels; never renumber).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Move {
    #[default]
    Idle = 0,
    Walk = 1,
    Charge = 2,
    Smash = 3,
    SwatHigh = 4,
    SwatLow = 5,
    Stomp = 6,
    Kick = 7,
    Jump = 8,
    Stagger = 9,
    Stumble = 10,
    Down = 11,
    Roar = 12,
}
impl Move {
    pub const ALL: [Move; MOVES] = [Move::Idle, Move::Walk, Move::Charge, Move::Smash, Move::SwatHigh, Move::SwatLow, Move::Stomp, Move::Kick, Move::Jump, Move::Stagger, Move::Stumble, Move::Down, Move::Roar];
    pub fn from_u8(n: u8) -> Option<Move> {
        Self::ALL.get(n as usize).copied()
    }
    pub fn id(self) -> u8 {
        self as u8
    }
    pub fn data(self) -> &'static MoveData {
        &TABLE[self as usize]
    }
    pub fn name(self) -> &'static str {
        self.data().name
    }
    /// The place in `View::ready` of the attack it is (None for the rest).
    pub fn ready_slot(self) -> Option<usize> {
        match self {
            Move::Charge => Some(READY_CHARGE),
            Move::Smash => Some(READY_SMASH),
            Move::SwatHigh | Move::SwatLow => Some(READY_SWAT),
            Move::Stomp | Move::Kick => Some(READY_STOMP),
            Move::Jump => Some(READY_JUMP),
            _ => None,
        }
    }
}
/// What a move is, which says how the client poses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Idle and Walk: loops with no length.
    Stance,
    Charge,
    Smash,
    Swat,
    Stomp,
    Kick,
    Jump,
    /// Stagger and Stumble: on its knees, its head low.
    Reel,
    Down,
    Victory,
}
/// One row of the move table (see the interface's §3). Seconds, metres, HP, m/s, radians.
#[derive(Clone, Copy, Debug)]
pub struct MoveData {
    pub mv: Move,
    pub name: &'static str,
    pub kind: Kind,
    /// The wind-up (the tell), the window it strikes in, and the follow-through. A charge's `active` is its longest run; a
    /// jump's is its time in the air (its recovery starts when it lands).
    pub startup: f32,
    pub active: f32,
    pub recovery: f32,
    /// Seconds after it ends before it can be used again.
    pub cooldown: f32,
    /// How far it reaches: from the titan's middle (a swat, a kick), or about its effect's centre (a wave, a stomp).
    pub reach: f32,
    /// Either side of the way it faces (radians; PI all round).
    pub arc: f32,
    /// The effect's centre, this far in front of the titan's middle.
    pub ahead: f32,
    /// The heights above the titan's feet a sweep passes through.
    pub band: [f32; 2],
    /// The damage near (at the centre), and at `reach` (0: falling to nothing).
    pub damage: f32,
    pub edge: f32,
    /// How hard it throws: speed outward (or along the sweep), and upward (m/s).
    pub knock: f32,
    pub lift: f32,
    /// The speed its shockwave's front spreads at (0: no wave).
    pub wave: f32,
}
impl MoveData {
    /// Its length at the table's timing.
    pub fn total(&self) -> f32 {
        self.startup + self.active + self.recovery
    }
    /// Whether it is an attack the titan chooses (with a tell and a cooldown).
    pub fn attacks(&self) -> bool {
        matches!(self.kind, Kind::Charge | Kind::Smash | Kind::Swat | Kind::Stomp | Kind::Kick | Kind::Jump)
    }
}
const fn row(mv: Move, name: &'static str, kind: Kind, t: [f32; 3], cooldown: f32) -> MoveData {
    MoveData { mv, name, kind, startup: t[0], active: t[1], recovery: t[2], cooldown, reach: 0., arc: 0., ahead: 0., band: [0., 0.], damage: 0., edge: 0., knock: 0., lift: 0., wave: 0. }
}
const ROUND: f32 = std::f32::consts::PI;
/// The move table, by move number.
#[rustfmt::skip]
pub static TABLE: [MoveData; MOVES] = [
    row(Move::Idle, "Looming", Kind::Stance, [0., 0., 0.], 0.),
    row(Move::Walk, "Heavy Tread", Kind::Stance, [0., 0., 0.], 0.),
    MoveData { reach: RADIUS + 0.08, arc: 1.05, damage: 45., knock: 1.6, lift: 0.8, ..row(Move::Charge, "Stampede", Kind::Charge, [0.65, CHARGE_TIME, 0.7], 7.) },
    MoveData { reach: 1.7, arc: ROUND, ahead: s(0.046), damage: 90., knock: 1.2, lift: 1.0, wave: 3.4, ..row(Move::Smash, "Ground Smash", Kind::Smash, [0.95, 0.12, 1.1], 5.5) },
    MoveData { reach: 1.05, arc: 1.75, band: [0.25, 1.1], damage: 45., knock: 1.5, lift: 0.6, ..row(Move::SwatHigh, "High Swat", Kind::Swat, [0.5, 0.22, 0.55], 1.4) },
    MoveData { reach: 1.1, arc: 1.6, band: [0., 0.35], damage: 40., knock: 1.4, lift: 0.5, ..row(Move::SwatLow, "Low Swat", Kind::Swat, [0.55, 0.25, 0.6], 1.6) },
    MoveData { reach: 0.28, arc: ROUND, ahead: s(0.027), damage: 90., edge: 20., knock: 0.8, lift: 0.4, ..row(Move::Stomp, "Stomp", Kind::Stomp, [0.38, 0.08, 0.45], 1.2) },
    MoveData { reach: 0.85, arc: 0.7, band: [0., 0.3], damage: 50., knock: 2., lift: 0.9, ..row(Move::Kick, "Low Kick", Kind::Kick, [0.32, 0.14, 0.5], 1.4) },
    MoveData { reach: 1.1, arc: ROUND, damage: 55., knock: 1., lift: 0.8, wave: 3., ..row(Move::Jump, "Quake Hop", Kind::Jump, [0.4, 2. * JUMP_SPEED / actor::GRAVITY as f32, 0.55], 4.5) },
    row(Move::Stagger, "Brought to its Knees", Kind::Reel, [0.4, 2.2, 0.4], 0.),
    row(Move::Stumble, "Stumble", Kind::Reel, [0.2, 0.9, 0.3], 0.),
    row(Move::Down, "Felled", Kind::Down, [0., 2.5, 0.], 0.),
    row(Move::Roar, "Roar", Kind::Victory, [0., 2.5, 0.], 0.),
];

// ----- What the client is told.

/// Things that happen, as `Event::kind` (see the interface's §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Tell,
    Step,
    Stepped,
    ChargeStart,
    ChargeHit,
    ChargeWall,
    Smash,
    Swat,
    Stomp,
    JumpLand,
    Hit,
    StaggerStart,
    StaggerEnd,
    WeakHit,
    TitanDown,
    Reinforcement,
    Over,
}
/// One thing that happened (`View::events`): numbered in order from 1, each told for EVENT_LIFE seconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Event {
    pub id: u32,
    pub kind: Option<EventKind>,
    pub time: f32,
    pub by: u32,
    pub to: u32,
    /// The move (`Move` as its number).
    pub mv: u8,
    pub amount: f32,
    pub p: [f32; 3],
    /// A hit on the titan: the zone (`Zone` as its number).
    pub zone: u8,
}
/// One small hotdog, as the client is told it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmallView {
    pub id: u32,
    /// HP it has taken from the titan this round, and its weak-spot hits.
    pub dealt: f32,
    pub weak: u32,
    /// Seconds until it is back (0: alive, or not coming back).
    pub respawn: f32,
    /// Dead with the pool empty: out for the round.
    pub out: bool,
}
/// What every client is told of the titan (`{type:'titan', ...View}` online, `Feast::titan_view` offline).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct View {
    pub elapsed: f32,
    pub titan: u32,
    pub hp: f32,
    pub max: f32,
    pub mv: u8,
    pub started: f32,
    pub len: f32,
    pub crouch: bool,
    pub ready: [f32; 5],
    pub stagger: f32,
    pub exposed: bool,
    pub pool: u32,
    pub reserve: u32,
    pub ends: f32,
    pub over: u8,
    pub smalls: Vec<SmallView>,
    pub events: Vec<Event>,
    /// Its stride as the round has it (`Titan::stride`, `Titan::drive`): the client draws its walk from these, carrying
    /// them on between views by the same law.
    pub stride: f32,
    pub drive: f32,
}
impl View {
    pub fn mv(&self) -> Move {
        Move::from_u8(self.mv).unwrap_or_default()
    }
    /// Seconds into its move at the round's clock `elapsed`.
    pub fn t(&self, elapsed: f32) -> f32 {
        (elapsed - self.started).max(0.)
    }
    pub fn small(&self, id: u32) -> Option<&SmallView> {
        self.smalls.iter().find(|s| s.id == id)
    }
}

/// Whether the titan may start a move (or jump) at the round's clock `elapsed`.
pub fn free(view: &View, _elapsed: f32) -> bool {
    matches!(view.mv(), Move::Idle | Move::Walk)
}
/// The titan's walking pace now (m/s, before backing off: `stride_pace`): nothing in a move.
pub fn pace(view: &View) -> f32 {
    match view.mv() {
        Move::Idle | Move::Walk if view.crouch => CROUCH_WALK,
        Move::Idle | Move::Walk => WALK,
        _ => 0.,
    }
}
/// What its pace is going `way` (a unit [x, z]) while it faces `yaw`: full ahead, BACKING going backward, between
/// sideways.
pub const BACKING: f32 = 0.6;
pub fn stride_pace(pace: f32, yaw: f32, way: Vec2) -> f32 {
    let ahead = facing(yaw);
    pace * (BACKING + (1. - BACKING) * ahead.dot(way.normalize_or_zero()).max(0.))
}
/// The yaw `yaw` turned towards `want` by at most `rate` × `dt`.
pub fn turn(yaw: f32, want: f32, dt: f32, rate: f32) -> f32 {
    let delta = (want - yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    let most = rate * dt;
    yaw + delta.clamp(-most, most)
}
/// The way a body of yaw `yaw` faces, flat ([x, z]).
pub fn facing(yaw: f32) -> Vec2 {
    Vec2::new(-yaw.sin(), -yaw.cos())
}
fn flat(p: [f32; 3]) -> Vec2 {
    Vec2::new(p[0], p[2])
}

// ----- The body.

/// A capsule round the segment `a`..`b`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Capsule {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
}
impl Capsule {
    /// How far `q` is from it (0 inside), and its nearest point on the surface.
    pub fn gap(&self, q: Vec3) -> (f32, Vec3) {
        let ab = self.b - self.a;
        let t = if ab.length_squared() > 1e-12 { ((q - self.a).dot(ab) / ab.length_squared()).clamp(0., 1.) } else { 0. };
        let on = self.a + ab * t;
        let far = q.distance(on);
        if far <= self.radius { (0., q) } else { (far - self.radius, on + (q - on) / far * self.radius) }
    }
}
/// The titan's body as rounds and blasts find it, in the world: its legs (each a thigh and a shin), its torso, the line
/// of its shoulders, its arms (upper arm and forearm, left then right), and its head (the drawn head: a round body about
/// its own upright from the neck, `head_profile`), with the nape's and the face's ways (unit vectors) and how wide the
/// weak spot's cap is (radians). Every part is where the client draws it (`Frame`, titan_figures.rs).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shape {
    /// The shins (knee to ankle: what faces the small hotdogs), left and right.
    pub legs: [Capsule; 2],
    /// The thighs (hip to knee), left and right.
    pub thighs: [Capsule; 2],
    /// The body (its bottom to its waist) and the chest above it (narrower, up to the shoulders the head rises from).
    pub torso: Capsule,
    pub chest: Capsule,
    /// Across its shoulders (left to right): tipped back or hunched, the nape sinks behind them.
    pub shoulders: Capsule,
    /// The upper arms and forearms: left upper, left fore, right upper, right fore.
    pub arms: [Capsule; 4],
    /// The head's middle (where the caps are seen from), and a sphere `head_radius` round it near enough to the drawn
    /// head for a blast's reach and a bot's aim; the neck the drawn head stands on, and its turn (world).
    pub head: Vec3,
    pub head_radius: f32,
    pub neck: Vec3,
    pub turn: Quat,
    pub nape: Vec3,
    pub face: Vec3,
    pub weak_arc: f32,
    /// The middle of the weak spot's cap on the drawn head, from the head's middle (world).
    pub spot: Vec3,
}
/// The head's radius, and how far its middle stands above the neck.
pub const HEAD_RADIUS: f32 = s(0.0183);
pub const NECK: f32 = s(0.0183);
/// The parts' radii: the torso (its body, and its chest), the shoulders' line, a thigh, a shin, an upper arm, a
/// forearm; and how far up the drawn torso (above its feet, standing) its waist is, where the chest begins.
pub const TORSO_RADIUS: f32 = s(0.0242);
pub const CHEST_RADIUS: f32 = s(0.0205);
pub const WAIST: f32 = s(0.0917);
pub const SHOULDERS_RADIUS: f32 = s(0.0083);
pub const THIGH_RADIUS: f32 = s(0.0092);
pub const SHIN_RADIUS: f32 = s(0.0083);
pub const UPPER_RADIUS: f32 = s(0.0067);
pub const FORE_RADIUS: f32 = s(0.0058);
/// In the head's own frame (looking level): the nape's way (back and well down: looking level the skull overhangs it,
/// so a level shot from behind meets the back of the head; bowed it turns to face straight back) and the face's.
pub const NAPE: Vec3 = Vec3::new(0., -0.75, 0.66);
pub const FACE: Vec3 = Vec3::new(0., 0.05, -0.999);
/// The titan as it is drawn at one instant, in hotdog measures (times SCALE for metres), standing at the origin facing
/// -z: the torso's axis (its bottom, its waist, its top: `TORSO_A`, `TORSO_B`, `TORSO_C`), the shoulders, elbows, wrists,
/// hips, knees and ankles (left, right), the neck the head stands on (`NECK_AT`), and the pose's own turn of the head (a
/// quaternion [x, y, z, w]: the head turns by it and then by the look's nod, as the client turns it). And what the
/// client's pose places its limbs by, so that a blend of two figures goes as the client's blend of their poses does
/// (`lerp`): the chest's turn (`chest`, a quaternion), each hand's target and each elbow's hint (from its shoulder, in
/// the chest's frame: `HAND`, `ELBOW_HINT`), each foot's target and each knee's hint (in the figure's frame: `FOOT`,
/// `KNEE_HINT`). The client generates every figure the titan has (titan_figures.rs: the stances, each move over its
/// beats, the gaits over their cycles) from its own drawn skeleton.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub p: [[f32; 3]; 16],
    pub turn: [f32; 4],
    pub chest: [f32; 4],
    pub h: [[f32; 3]; 8],
}
pub const TORSO_A: usize = 0;
pub const TORSO_B: usize = 1;
pub const TORSO_C: usize = 2;
pub const SHOULDER: usize = 3;
pub const ELBOW: usize = 5;
pub const WRIST: usize = 7;
pub const HIP: usize = 9;
pub const KNEE: usize = 11;
pub const ANKLE: usize = 13;
pub const NECK_AT: usize = 15;
/// In `Frame::h`, left then right of each.
pub const HAND: usize = 0;
pub const ELBOW_HINT: usize = 2;
pub const FOOT: usize = 4;
pub const KNEE_HINT: usize = 6;
/// The parts that turn with the chest about its pivot (just above the hips' middle): the torso's axis, the neck, the
/// shoulders.
const CHESTED: [usize; 6] = [TORSO_A, TORSO_B, TORSO_C, NECK_AT, SHOULDER, SHOULDER + 1];
/// The client's two-bone reach (src/titan/pose.rs `reach`): the middle joint of a limb `a` and `b` long from `joint`
/// towards `target` (drawn in to within reach), bent towards `pole` (falling back to `fallback` as the pole comes in line
/// with the limb); the middle joint and the end.
fn reach(joint: Vec3, target: Vec3, a: f32, b: f32, pole: Vec3, fallback: Vec3) -> (Vec3, Vec3) {
    let to = target - joint;
    let along = to.normalize_or(Vec3::NEG_Y);
    let d = to.length().clamp((a - b).abs() + 0.02 * (a + b), (a + b) * 0.995);
    let first = pole - along * pole.dot(along);
    let second = fallback - along * fallback.dot(along);
    let k = first.length() / pole.length().max(1e-6);
    let w = ((0.45 - k) / 0.45).clamp(0., 1.);
    let out = (first.normalize_or_zero() * (1. - w) + second.normalize_or_zero() * w).normalize_or(along.any_orthonormal_vector());
    let x = (d * d + a * a - b * b) / (2. * d);
    let h = (a * a - x * x).max(0.).sqrt();
    (joint + along * x + out * h, joint + along * d)
}
impl Frame {
    /// Between this frame and `other`, `k` of the way, as the client blends their poses (and solves the blend): the hips
    /// straight across, the chest turning round its pivot with the torso, the neck and the shoulders on it, each hand's
    /// target and elbow's hint across in the chest's frame and each foot's target and knee's hint in the figure's, every
    /// limb then reaching for its target (the client's reach), and the head's own turn round.
    pub fn lerp(&self, other: &Frame, k: f32) -> Frame {
        let (a, b) = (|i: usize| Vec3::from_array(self.p[i]), |i: usize| Vec3::from_array(other.p[i]));
        let (ha, hb) = (|i: usize| Vec3::from_array(self.h[i]), |i: usize| Vec3::from_array(other.h[i]));
        let q = |x: [f32; 4]| Quat::from_array(x).normalize();
        let (ca, cb) = (q(self.chest), q(other.chest));
        let chest = ca.slerp(cb, k);
        let mut at = [Vec3::ZERO; 16];
        let mut h = [Vec3::ZERO; 8];
        for i in [HIP, HIP + 1] {
            at[i] = a(i).lerp(b(i), k);
        }
        let pivot = |f: &dyn Fn(usize) -> Vec3| (f(HIP) + f(HIP + 1)) * 0.5 + Vec3::Y * figures::PIVOT_ABOVE_HIPS;
        let (pa, pb) = (pivot(&a), pivot(&b));
        let centre = (at[HIP] + at[HIP + 1]) * 0.5 + Vec3::Y * figures::PIVOT_ABOVE_HIPS;
        for i in CHESTED {
            at[i] = centre + chest * (ca.inverse() * (a(i) - pa)).lerp(cb.inverse() * (b(i) - pb), k);
        }
        for i in 0..8 {
            h[i] = ha(i).lerp(hb(i), k);
        }
        let (upper, fore, thigh, shin) = figures::BONES;
        for side in 0..2 {
            let fallback = |out: [f32; 3]| Vec3::new(if side == 0 { -out[0] } else { out[0] }, out[1], -out[2]);
            let shoulder = at[SHOULDER + side];
            let (elbow, wrist) = reach(shoulder, shoulder + chest * h[HAND + side], upper, fore, chest * h[ELBOW_HINT + side], chest * fallback(figures::ELBOW_OUT));
            (at[ELBOW + side], at[WRIST + side]) = (elbow, wrist);
            let hip = at[HIP + side];
            let (knee, ankle) = reach(hip, h[FOOT + side], thigh, shin, h[KNEE_HINT + side], fallback(figures::KNEE_OUT));
            (at[KNEE + side], at[ANKLE + side]) = (knee, ankle);
        }
        let turn = q(self.turn).slerp(q(other.turn), k);
        Frame { p: at.map(|v| v.to_array()), turn: turn.to_array(), chest: chest.to_array(), h: h.map(|v| v.to_array()) }
    }
    fn at(&self, i: usize) -> Vec3 {
        Vec3::from_array(self.p[i]) * SCALE
    }
}
#[path = "titan_figures.rs"]
mod figures;
/// A stance's figure, as drawn: 0 standing, 1 crouched, 2 kneeling.
pub fn stance_frame(stance: usize) -> &'static Frame {
    [&figures::STANDING, &figures::CROUCHED, &figures::KNEELING][stance.min(2)]
}
/// A move's timing as the client plays it this time (src/titan/anims.rs `timing`): the table's, its active window as
/// long as the view's `len` says for a charge's run and a jump's flight, a reel stretched to its `len`, and a move that is
/// one long window (felled, a roar) a third each. [startup, active, recovery], seconds.
pub fn timing(mv: Move, len: f32) -> [f32; 3] {
    let d = mv.data();
    let (startup, active, recovery) = (d.startup, d.active, d.recovery);
    let total = startup + active + recovery;
    let len = if len > 0. { len } else { total };
    if startup <= 0. && recovery <= 0. {
        return [len * 0.34, len * 0.33, len * 0.33];
    }
    if (len - total).abs() <= 1e-3 {
        return [startup, active, recovery];
    }
    let varied = len - startup - recovery;
    if matches!(mv, Move::Charge | Move::Jump) && varied >= 0.05 {
        return [startup, varied, recovery];
    }
    let k = len / total.max(1e-3);
    [startup * k, active * k, recovery * k]
}
/// The beat (0 to 3) `t` seconds into a move of `timing`, as the client counts them (src/kungfu `Timing::beat`).
pub fn beat(timing: [f32; 3], t: f32) -> f32 {
    let [s, a, r] = timing;
    if t < s && s > 0. {
        (t / s).max(0.)
    } else if t < s + a && a > 0. {
        1. + (t - s).max(0.) / a
    } else if r > 0. {
        (2. + (t - s - a).max(0.) / r).min(3.)
    } else {
        3.
    }
}
/// The second `beat` comes at.
fn beat_time(timing: [f32; 3], beat: f32) -> f32 {
    let [s, a, r] = timing;
    if beat < 1. {
        beat * s
    } else if beat < 2. {
        s + (beat - 1.) * a
    } else {
        s + a + (beat - 2.).min(1.) * r
    }
}
/// The figure of move `mv` `t` seconds in (its view's `len`), as drawn: its drawn pose at the beat the client shows then
/// (held while the blow's hit-stop holds it, then catching up), between the samples. Idle and Walk: the stance's.
pub fn move_frame(mv: Move, t: f32, len: f32) -> Frame {
    if matches!(mv, Move::Idle | Move::Walk) {
        return figures::STANDING;
    }
    let timing = timing(mv, len);
    let total = timing[0] + timing[1] + timing[2];
    let (contact, hold) = figures::HITSTOP[mv as usize];
    let t = if hold > 0. {
        let at = beat_time(timing, contact);
        if t < at {
            t
        } else if t < at + hold {
            at
        } else {
            let rest = (total - at).max(1e-3);
            at + (t - at - hold) * rest / (rest - hold).max(1e-3)
        }
    } else {
        t
    };
    let looped = figures::LOOPS[mv as usize];
    let b = if looped > 0. { (t / looped.max(0.05)).rem_euclid(1.) * 3. } else { beat(timing, t) };
    sampled(figures::MOVES[mv as usize], b)
}
/// The figure at `at` of figures sampled at rising points (a straight blend of the two either side).
fn sampled(samples: &[(f32, Frame)], at: f32) -> Frame {
    let i = samples.partition_point(|(x, _)| *x <= at).clamp(1, samples.len() - 1);
    let ((a, from), (c, to)) = (&samples[i - 1], &samples[i]);
    from.lerp(to, ((at - a) / (c - a).max(1e-6)).clamp(0., 1.))
}
/// The figure `amount` of the way down from standing into the crouch (0 standing, 1 crouched), as drawn: the drawn titan
/// blends its poses, so its knees go out and round, not straight across.
pub fn crouch_frame(amount: f32) -> Frame {
    sampled(figures::CROUCHING, amount.clamp(0., 1.))
}
/// Metres the body goes in one whole cycle (two steps) of its gait: the walk, the crouched crawl, the charge's run
/// (src/titan/anims.rs `cycle_length`).
pub fn gait_cycle(crouched: bool, running: bool) -> f32 {
    let (walk, crawl, run) = figures::CYCLES;
    (if running { run } else if crouched { crawl } else { walk }) * SCALE
}
/// Whether the charge is in its run at `beat` (its keys either side are its rearing up and its skid).
pub fn running_at(beat: f32) -> bool {
    let (from, to) = figures::CHARGE_RUN;
    beat >= from && beat < to
}
/// The titan's figure walking or standing: `crouching` of the way into its crouch, walking `drive` (0 to 1 of its pace),
/// its stride `stride` radians into the gait's cycle, as drawn. The walk and the crawl are each a figure at its pace
/// over the cycle; walking slower blends it from the stance; between standing and crouched the walk's and the crawl's
/// own movement is blended over the crouch's figure.
pub fn stride_frame(crouching: f32, drive: f32, stride: f32) -> Frame {
    let (c, d) = (crouching.clamp(0., 1.), drive.clamp(0., 1.));
    let base = crouch_frame(c);
    if d <= 0. {
        return base;
    }
    let walk = figures::STANDING.lerp(&sampled(figures::WALK, stride.rem_euclid(TAU)), d);
    let crawl = figures::CROUCHED.lerp(&sampled(figures::CRAWL, stride.rem_euclid(TAU)), d);
    if c <= 0. {
        return walk;
    }
    if c >= 1. {
        return crawl;
    }
    let v = |f: &Frame, i: usize| Vec3::from_array(f.p[i]);
    let w = |f: &Frame, i: usize| Vec3::from_array(f.h[i]);
    let mut f = base;
    for i in 0..16 {
        let moving = (v(&walk, i) - v(&figures::STANDING, i)).lerp(v(&crawl, i) - v(&figures::CROUCHED, i), c);
        f.p[i] = (v(&base, i) + moving).to_array();
    }
    for i in 0..8 {
        let moving = (w(&walk, i) - w(&figures::STANDING, i)).lerp(w(&crawl, i) - w(&figures::CROUCHED, i), c);
        f.h[i] = (w(&base, i) + moving).to_array();
    }
    let turned = |pick: fn(&Frame) -> [f32; 4]| {
        let q = |f: &Frame| Quat::from_array(pick(f)).normalize();
        let turning = (q(&walk) * q(&figures::STANDING).inverse()).slerp(q(&crawl) * q(&figures::CROUCHED).inverse(), c);
        (turning * q(&base)).normalize().to_array()
    };
    (f.turn, f.chest) = (turned(|f| f.turn), turned(|f| f.chest));
    f
}
/// Which figure: 0 standing (or in the air), 1 crouched, 2 kneeling.
pub fn stance_of(p: &Player, mv: Move) -> usize {
    if matches!(mv.data().kind, Kind::Reel | Kind::Down) {
        2
    } else if p.crouch {
        1
    } else {
        0
    }
}
/// The titan's stature in a stance (`stance_of`).
pub fn stature(stance: usize) -> f32 {
    [TALL, CROUCH_TALL, KNEEL_TALL][stance.min(2)]
}
impl Shape {
    /// The titan `p` as it stands in move `mv`'s stance (standing, crouched, kneeling for a reel or felled), as drawn,
    /// turned to its yaw, its head nodded by its look (head.rs `nod`), tucked crouched and bowed kneeling. (The figure of
    /// a move under way, with its blends, is `Feast::titan_shape`'s.)
    pub fn of(p: &Player, mv: Move) -> Shape {
        let stance = stance_of(p, mv);
        Shape::posed(p, stance_frame(stance), stance)
    }
    /// The titan `p` in the drawn figure `f`, its look's nod that of `stance` (`stance_of`).
    pub fn posed(p: &Player, f: &Frame, stance: usize) -> Shape {
        Shape::nodding(p, f, stance, figure_nod(stance, p.pitch))
    }
    /// The titan `p` in figure `f` and `stance`, its head nodding `nod` (radians about x; less than 0 bowed).
    pub fn nodding(p: &Player, f: &Frame, stance: usize, nod: f32) -> Shape {
        let yaw = Quat::from_rotation_y(p.yaw);
        let feet = Vec3::from_array(p.p);
        let at = |i: usize| feet + yaw * f.at(i);
        let capsule = |a: usize, b: usize, radius: f32| Capsule { a: at(a), b: at(b), radius };
        let legs = [0, 1].map(|i| capsule(KNEE + i, ANKLE + i, SHIN_RADIUS));
        let thighs = [0, 1].map(|i| capsule(HIP + i, KNEE + i, THIGH_RADIUS));
        let arms = [capsule(SHOULDER, ELBOW, UPPER_RADIUS), capsule(ELBOW, WRIST, FORE_RADIUS), capsule(SHOULDER + 1, ELBOW + 1, UPPER_RADIUS), capsule(ELBOW + 1, WRIST + 1, FORE_RADIUS)];
        let torso = capsule(TORSO_A, TORSO_B, TORSO_RADIUS);
        let chest = capsule(TORSO_B, TORSO_C, CHEST_RADIUS);
        let shoulders = capsule(SHOULDER, SHOULDER + 1, SHOULDERS_RADIUS);
        let _ = stance;
        let turn = yaw * Quat::from_array(f.turn).normalize() * Quat::from_rotation_x(nod);
        let neck = at(NECK_AT);
        let head = neck + turn * Vec3::Y * NECK;
        let spot = turn * Vec3::from_array(figures::SPOT) * SCALE;
        Shape { legs, thighs, torso, chest, shoulders, arms, head, head_radius: HEAD_RADIUS, neck, turn, nape: (turn * NAPE).normalize(), face: (turn * FACE).normalize(), weak_arc: WEAK_ARC, spot }
    }
    /// Every capsule of the body but the head, with its zone.
    pub fn parts(&self) -> [(Capsule, Zone); 11] {
        let a = self.arms;
        [
            (self.chest, Zone::Body),
            (self.legs[0], Zone::Legs),
            (self.legs[1], Zone::Legs),
            (self.thighs[0], Zone::Legs),
            (self.thighs[1], Zone::Legs),
            (self.torso, Zone::Body),
            (self.shoulders, Zone::Body),
            (a[0], Zone::Arms),
            (a[1], Zone::Arms),
            (a[2], Zone::Arms),
            (a[3], Zone::Arms),
        ]
    }
    /// Where the weak spot is seen on the drawn head (the patch's point half its arc up from the cap's middle, over the
    /// shoulders): where its pick and the HUD's ring are drawn, and what a bot aims at.
    pub fn weak_point(&self) -> Vec3 {
        self.head + self.spot
    }
    /// The middle of the face on the head's surface.
    pub fn face_point(&self) -> Vec3 {
        self.head + self.face * self.head_radius
    }
    /// A knee (a bot shooting for the stagger aims here).
    pub fn knee(&self, side: usize) -> Vec3 {
        self.legs[side.min(1)].a
    }
    /// The zone a point on the head's surface is in: the weak spot's cap (within `weak_arc` of the nape's way, seen
    /// from the head's middle: the drawn pale patch), the face's (FACE_ARC of the face's way), or the body.
    pub fn zone_on_head(&self, point: Vec3) -> Zone {
        let n = (point - self.head).normalize_or_zero();
        if n.dot(self.nape) >= self.weak_arc.cos() {
            Zone::Weak
        } else if n.dot(self.face) >= FACE_ARC.cos() {
            Zone::Face
        } else {
            Zone::Body
        }
    }
    /// Where a ray from `origin` along the unit vector `dir` first meets the drawn head grown by `size` (the distance
    /// along it): the head is a round body about its own upright from the neck, `head_profile` across.
    pub fn ray_head(&self, origin: Vec3, dir: Vec3, size: f32) -> Option<f32> {
        let back = self.turn.inverse();
        let (o, d) = (back * (origin - self.neck), back * dir);
        let (low, high) = (figures::HEAD_LOW * SCALE, figures::HEAD_HIGH * SCALE);
        let mid = Vec3::Y * (low + high) * 0.5;
        let bound = ((high - low) * 0.5).hypot(figures::HEAD_WIDEST * SCALE) + size;
        let along = (mid - o).dot(d);
        let miss = (o + d * along).distance(mid);
        if miss > bound {
            return None;
        }
        let half = (bound * bound - miss * miss).max(0.).sqrt();
        let (enter, leave) = ((along - half).max(0.), along + half);
        if leave <= 0. {
            return None;
        }
        let inside = |t: f32| {
            let q = o + d * t;
            q.y >= low - size && q.y <= high + size && Vec2::new(q.x, q.z).length() <= head_profile(q.y) + size
        };
        // (How far off the head a place is at the least: off the round column it stands in.)
        let widest = figures::HEAD_WIDEST * SCALE + size;
        let off = |t: f32| {
            let q = o + d * t;
            let past = (low - size - q.y).max(q.y - high - size).max(0.);
            (Vec2::new(q.x, q.z).length() - widest).max(0.).hypot(past)
        };
        if inside(enter) {
            return Some(enter);
        }
        let mut t = enter;
        while t < leave {
            let next = (t + off(t).max(HEAD_STEP)).min(leave);
            if inside(next) {
                let (mut out, mut into) = (t, next);
                for _ in 0..14 {
                    let m = 0.5 * (out + into);
                    if inside(m) {
                        into = m;
                    } else {
                        out = m;
                    }
                }
                return Some(into);
            }
            t = next;
        }
        None
    }
    /// How far `q` is from the body (0 inside), its nearest point, and the zone that point is in (a blast's).
    pub fn gap(&self, q: Vec3) -> (f32, Vec3, Zone) {
        let mut best = {
            let far = q.distance(self.head);
            let (gap, near) = if far <= self.head_radius { (0., q) } else { (far - self.head_radius, self.head + (q - self.head) / far * self.head_radius) };
            (gap, near, self.zone_on_head(near))
        };
        for (c, zone) in self.parts() {
            let (gap, near) = c.gap(q);
            if gap < best.0 {
                best = (gap, near, zone);
            }
        }
        best
    }
}
/// Where a round met the titan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub t: f32,
    pub zone: Zone,
    pub point: Vec3,
}
/// Where a round flying from `origin` along the unit vector `dir`, `size` round, first meets the titan's `shape`.
pub fn hit(shape: &Shape, origin: Vec3, dir: Vec3, size: f32) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    let mut take = |t: Option<f32>, zone: Zone| {
        if let Some(t) = t.filter(|t| best.is_none_or(|b| *t < b.t)) {
            best = Some(Hit { t, zone, point: origin + dir * t });
        }
    };
    for (c, zone) in shape.parts() {
        take(crate::head::ray_capsule(origin, dir, c.a, c.b, c.radius + size), zone);
    }
    let parts = best;
    match shape.ray_head(origin, dir, size) {
        Some(t) if parts.is_none_or(|b| t < b.t) => {
            // (Where it met the head itself: the zone of that place on the drawn head.)
            let point = origin + dir * t;
            Some(Hit { t, zone: shape.zone_on_head(point), point })
        }
        _ => parts,
    }
}
/// How a round's way is searched for the drawn head: by how far off the column it stands in the round is, never less
/// than HEAD_STEP (metres), then halved down to a fraction of a millimetre.
const HEAD_STEP: f32 = 0.002;
/// The drawn head's radius `y` up from the neck in its own frame (metres; 0 past its ends): src/titan/body.rs
/// `head_radius`, as titan_figures.rs keeps it.
pub fn head_profile(y: f32) -> f32 {
    let (low, high) = (figures::HEAD_LOW * SCALE, figures::HEAD_HIGH * SCALE);
    if y <= low || y >= high {
        return 0.;
    }
    let n = figures::HEAD.len() - 1;
    let x = (y - low) / (high - low) * n as f32;
    let i = (x.floor() as usize).min(n - 1);
    let k = x - i as f32;
    (figures::HEAD[i] + (figures::HEAD[i + 1] - figures::HEAD[i]) * k) * SCALE
}
/// The titan's body in the actor world (and a client's mirror of it): its footprint, its stature, its mass, held still
/// (titan.rs moves it).
pub fn ghost(id: u32, p: [f32; 3], stance: usize) -> Actor {
    let mut body = Actor::hotdog(id, [p[0] as f64, p[1] as f64, p[2] as f64]);
    (body.radius, body.height, body.mass, body.idle) = (RADIUS as f64, stature(stance) as f64, MASS, true);
    body
}
/// The head's nod in a stance, looking `pitch` (radians about x; less than 0 bowed): kneeling bowed KNEEL_BOW, crouched
/// tucked CROUCH_TUCK under its look, standing its look's (head.rs `nod`).
pub fn figure_nod(stance: usize, pitch: f32) -> f32 {
    match stance {
        2 => -KNEEL_BOW,
        1 => crate::head::nod(1, pitch) + CROUCH_TUCK,
        _ => crate::head::nod(0, pitch),
    }
}
/// The titan `p`'s body as `view` tells it, at the round's clock `elapsed` (a client's, online): the figure of its move
/// then, or of its stride, as drawn (the round's own, `Feast::titan_shape`, also has the blends between them).
pub fn shape_of(p: &Player, view: &View, elapsed: f32) -> Shape {
    let mv = view.mv();
    let stance = stance_of(p, mv);
    let frame = if matches!(mv, Move::Idle | Move::Walk) { stride_frame(view.crouch as u8 as f32, view.drive, view.stride) } else { move_frame(mv, view.t(elapsed), view.len) };
    Shape::posed(p, &frame, stance)
}
/// The weak spot of the titan `p` as `view` tells it (where it is seen: `Shape::weak_point`), at the view's clock.
pub fn weak_point(p: &Player, view: &View) -> Vec3 {
    shape_of(p, view, view.elapsed).weak_point()
}

// ----- Its footing: where its footprint may be.

/// The level as the titan's footprint meets it: every block, deck, ramp and sink, and every overhead box (map.rs
/// `Prop::Box`) whose underside is below its height, each grown by RADIUS; and the counter's edge drawn in by RADIUS.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Footing {
    /// [min x, min z, max x, max z] the middle may not enter.
    pub rects: Vec<[f32; 4]>,
    /// The overheads' rects as they stand (dens), with their undersides' height above the floor.
    pub dens: Vec<([f32; 4], f32)>,
    /// [min x, min z, max x, max z] the middle must stay inside.
    pub bounds: [f32; 4],
}
/// The longest stride `slide` tests only at its end.
const SLIDE: f32 = 0.05;
impl Footing {
    pub fn of(level: &Level) -> Footing {
        let r = RADIUS;
        let grow = |rect: [f64; 4]| [rect[0] as f32 - r, rect[1] as f32 - r, rect[2] as f32 + r, rect[3] as f32 + r];
        let map = &level.map;
        let mut rects: Vec<[f32; 4]> = map.blocks.iter().map(|b| grow(b.rect)).collect();
        rects.extend(map.decks.iter().filter(|d| d.height > 0.005).map(|d| grow(d.rect)));
        rects.extend(map.ramps.iter().map(|ramp| grow(ramp.rect)));
        rects.extend(map.sinks.iter().map(|sink| grow(sink.rect)));
        let mut dens = vec![];
        for prop in &map.props {
            if let Prop::Box { rect, base, .. } = prop {
                if (*base as f32) < TALL {
                    rects.push(grow(*rect));
                }
                if (*base as f32) >= DEN_CLEAR {
                    dens.push((rect.map(|v| v as f32), *base as f32));
                }
            }
        }
        let b = map.bounds;
        Footing { rects, dens, bounds: [b[0] as f32 + r, b[1] as f32 + r, b[2] as f32 - r, b[3] as f32 - r] }
    }
    /// Whether the titan's middle may stand at (x, z).
    pub fn clear(&self, x: f32, z: f32) -> bool {
        let b = self.bounds;
        x >= b[0] && x <= b[2] && z >= b[1] && z <= b[3] && !self.rects.iter().any(|r| x > r[0] && x < r[2] && z > r[1] && z < r[3])
    }
    /// Moves the titan's middle from `from` by `step`, as far as its footing lets it, sliding along what stops it (an axis
    /// at a time). Returns where it ends and whether anything stopped it.
    pub fn slide(&self, from: Vec2, step: Vec2) -> (Vec2, bool) {
        let length = step.length();
        if length < 1e-9 {
            return (from, false);
        }
        let n = (length / SLIDE).ceil().max(1.);
        let part = step / n;
        let (mut at, mut stopped) = (from, false);
        for _ in 0..n as usize {
            let next = at + part;
            if self.clear(next.x, next.y) {
                at = next;
                continue;
            }
            stopped = true;
            let along_x = self.farthest(at, Vec2::new(part.x, 0.));
            at.x += part.x * along_x;
            let along_z = self.farthest(at, Vec2::new(0., part.y));
            at.y += part.y * along_z;
        }
        (at, stopped)
    }
    /// How much of the move from `at` by `d` can be made (0 to 1).
    fn farthest(&self, at: Vec2, d: Vec2) -> f32 {
        if d == Vec2::ZERO || self.clear(at.x + d.x, at.y + d.y) {
            return 1.;
        }
        let (mut low, mut high) = (0f32, 1f32);
        for _ in 0..10 {
            let middle = (low + high) / 2.;
            if self.clear(at.x + d.x * middle, at.y + d.y * middle) {
                low = middle;
            } else {
                high = middle;
            }
        }
        low
    }
    /// Whether [x, z] is in a den: under an overhead a small hotdog fits beneath.
    pub fn den(&self, x: f32, z: f32) -> bool {
        self.dens.iter().any(|(r, _)| x >= r[0] && x <= r[2] && z >= r[1] && z <= r[3])
    }
    /// The nearest clear spot to `at` (searched outwards on rings), or `at`.
    pub fn nearest_clear(&self, at: Vec2) -> Vec2 {
        if self.clear(at.x, at.y) {
            return at;
        }
        for ring in 1..200 {
            let r = ring as f32 * 0.05;
            let n = (ring * 8).max(8);
            for k in 0..n {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                let p = at + Vec2::new(a.cos(), a.sin()) * r;
                if self.clear(p.x, p.y) {
                    return p;
                }
            }
        }
        at
    }
}

// ----- Choosing the titan.

/// Who is to be the titan (`Rules::pick`, the lobby's choice): see the interface's §6.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pick {
    /// A bot while fewer than three people play, else a person drawn.
    #[default]
    Auto,
    Bot,
    Random,
    /// The person (offline, hotdog 1; the lobby's leader).
    Me,
    /// That hotdog.
    Seat(u32),
}
impl Pick {
    pub fn named(name: &str) -> Option<Pick> {
        match name.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Some(Pick::Auto),
            "bot" => Some(Pick::Bot),
            "random" | "player" | "human" => Some(Pick::Random),
            "me" => Some(Pick::Me),
            n => n.parse::<u32>().ok().filter(|n| *n > 0).map(Pick::Seat),
        }
    }
    pub fn name(self) -> String {
        match self {
            Pick::Auto => "auto".into(),
            Pick::Bot => "bot".into(),
            Pick::Random => "random".into(),
            Pick::Me => "me".into(),
            Pick::Seat(n) => n.to_string(),
        }
    }
}
/// The titan's hotdog id for a round: `humans` says, by seat (hotdog id - 1), who is a person; `previous` the last
/// round's titan; `seed` the dice. Bot and Random take the next seat of theirs after the previous titan (the first round:
/// drawn); Auto is Random with three people or more and Bot otherwise; Me the first person; Seat that seat. With nobody
/// of the kind asked for, the other kind.
pub fn cast(pick: Pick, humans: &[bool], previous: Option<u32>, seed: u64) -> u32 {
    let n = humans.len() as u32;
    if n == 0 {
        return 0;
    }
    let people: Vec<u32> = (1..=n).filter(|id| humans[*id as usize - 1]).collect();
    let bots: Vec<u32> = (1..=n).filter(|id| !humans[*id as usize - 1]).collect();
    let next = |pool: &[u32]| -> u32 {
        match previous {
            Some(last) => pool.iter().copied().find(|id| *id > last).unwrap_or(pool[0]),
            None => pool[(crate::spawns::Dice::new(&[seed, 0x7174_616e]).roll() % pool.len() as u64) as usize],
        }
    };
    let either = |first: &[u32], second: &[u32]| if !first.is_empty() { next(first) } else { next(second) };
    match pick {
        Pick::Seat(id) if (1..=n).contains(&id) => id,
        Pick::Me => people.first().copied().unwrap_or_else(|| either(&bots, &people)),
        Pick::Bot => either(&bots, &people),
        Pick::Random => either(&people, &bots),
        Pick::Auto | Pick::Seat(_) => {
            if people.len() >= 3 {
                either(&people, &bots)
            } else {
                either(&bots, &people)
            }
        }
    }
}

// ----- The round's state.

/// A small hotdog's record this round.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Small {
    pub id: u32,
    pub dealt: f32,
    pub weak: u32,
    pub out: bool,
    /// When the titan last stepped on it.
    pub stepped: f32,
    /// The HP it has taken from the titan lately (halving every RECENT seconds): how much of a threat it is.
    pub recent: f32,
}
/// The half-life of `Small::recent`, seconds.
pub const RECENT: f32 = 3.;
/// The round's tally for the balance runs: how the titan died (the zone of the killing blow), and the move (or a step)
/// that killed each small hotdog that died.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tally {
    pub titan_killed_by: Option<Zone>,
    pub small_deaths: [u32; MOVES],
    pub stepped_deaths: u32,
    pub other_deaths: u32,
    /// Hits on the titan by zone, and the HP they took.
    pub hits: [u32; ZONES],
    pub damage: [f32; ZONES],
    pub staggers: u32,
    /// The moves the titan began, and the blows each landed on a small hotdog.
    pub moves: [u32; MOVES],
    pub blows: [u32; MOVES],
    /// HP it took while on its knees (a stagger or a stumble), and the weak-spot hits then.
    pub reeling: f32,
    pub weak_reeling: u32,
}
/// A shockwave spreading along the floor (the smash's, the landing's).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wave {
    pub mv: Move,
    pub centre: Vec3,
    /// When it set out, and the height of the surface it spreads on.
    pub at: f32,
    pub surface: f32,
    /// The hotdogs it has passed.
    pub passed: Vec<u32>,
}
/// A bot titan's mind.
#[derive(Clone, Debug, Default)]
pub struct Brain {
    /// The small hotdog it is after (0: none), since when, and when it last hurt it.
    pub target: u32,
    pub since: f32,
    pub landed: f32,
    /// When it next chooses a target, and when it may next start an attack.
    pub choose: f32,
    pub think: f32,
    /// Its route, and when it was found.
    pub route: Vec<Vec2>,
    pub routed: f32,
    /// Hotdogs it has given up on for a while (id, until).
    pub shun: Vec<(u32, f32)>,
}
/// A small bot's mind in Titan.
#[derive(Clone, Debug, Default)]
pub struct Mind {
    pub id: u32,
    /// The titan's move it has judged (when it began), whether it means to jump it, and when.
    pub judged: f32,
    pub jump: bool,
    pub jump_at: f32,
    /// Getting out of the way until then, towards there.
    pub dodge_until: f32,
    pub dodge_to: Vec2,
    /// Which leg it shoots for, and when it last changed its mind about it.
    pub leg: usize,
}
/// The titan (`Feast::titan`): who it is, its move, its cooldowns, its stagger, the pool, the events of the last
/// EVENT_LIFE seconds, its footing, and the bots' minds.
#[derive(Clone, Debug, Default)]
pub struct Titan {
    pub id: u32,
    pub max: f32,
    pub mv: Move,
    pub started: f32,
    pub len: f32,
    pub ready: [f32; 5],
    /// Leg damage towards a stagger (HP), and when leg damage counts again.
    pub legs: f32,
    pub rest: f32,
    pub pool: u32,
    pub reserve: u32,
    pub over: u8,
    pub smalls: Vec<Small>,
    pub events: Vec<Event>,
    pub(crate) next_event: u32,
    pub footing: Footing,
    /// The last round's titan (rotation), carried over by a new round.
    pub previous: Option<u32>,
    pub tally: Tally,
    /// The move's phase (0 wind-up, 1 active, 2 recovery), what it has struck, when a charge's run ends and a jump
    /// landed (seconds into the move).
    pub phase: u8,
    pub struck: Vec<u32>,
    pub run_end: f32,
    pub landed: Option<f32>,
    /// The press waiting to start, and its time; the buttons of the person's last command.
    pub queued: Option<(Move, f32)>,
    pub buttons: u16,
    /// The yaw it turns towards (the look, or a bot's), and the walk it has (m/s, [x, z]).
    pub want: f32,
    pub vel: Vec2,
    pub last: Vec2,
    /// In the air: its upward speed and what it carries along; a stagger waiting for it to land.
    pub airborne: bool,
    pub vy: f32,
    pub carry: Vec2,
    /// Its walk as a jump was begun: the speed it takes off with (it stands still crouching down for it).
    pub run_up: Vec2,
    pub staggered: bool,
    /// Its stride (the client draws its walk from it, `View::stride`): the phase of its gait's cycle (radians: a whole
    /// cycle is two steps, the left foot falling at 0 and the right at PI), running with the ground it covers over the
    /// gait's cycle (`gait_cycle`); how much it walks (`drive`: 0 standing, 1 at its pace, eased at DRIVE_EASE); how much
    /// it is off the ground standing (`air`, eased at AIR_EASE: its legs tucked).
    pub stride: f32,
    pub drive: f32,
    pub air: f32,
    pub waves: Vec<Wave>,
    /// Who last hurt it and when (its back is protected from them).
    pub threat: (u32, f32),
    pub brain: Brain,
    pub minds: Vec<Mind>,
    pub draws: u64,
    /// The bot titan's route grid.
    pub routes: Routes,
    /// Where a cunning bot titan makes its stand, its back to a wall or into a corner: each spot and the way it faces
    /// (worked out once a round).
    pub lairs: Vec<(Vec2, f32)>,
    /// The round has been played (stepped): a new round's titan is chosen by turns from this one's.
    pub played: bool,
    /// Its figure as the client draws it now (`Frame`): its crouch eased in and out at the client's rate (`crouching`, 0
    /// to 1), each move blended in from the figure before over the client's blend (`blend`: from, when, how long), the
    /// move it was blending into and when that began (`shown`), and its head's nod eased after the look (`nod`, radians:
    /// `figure_nod`). Rounds, blasts and blows find this figure.
    pub figure: Frame,
    pub crouching: f32,
    pub(crate) blend: Option<(Frame, f32, f32)>,
    pub(crate) shown: Option<(Move, f32)>,
    pub nod: Option<f32>,
}
/// The client's blends (src/titan/play.rs): a move from the figure before over BLEND seconds (at most half its wind-up,
/// at least BLEND_MIN), standing or walking SETTLE, felled FALL_BLEND; the crouch eased at CROUCH_EASE a second.
pub const BLEND: f32 = 0.12;
pub const BLEND_MIN: f32 = 0.05;
pub const SETTLE: f32 = 0.3;
pub const FALL_BLEND: f32 = 0.25;
pub const CROUCH_EASE: f32 = 5.;
/// The walk's drive eases at DRIVE_EASE a second towards its speed's share of its pace (DRIVE_MIN to DRIVE_MAX of it while
/// it walks: faster than WALK_MOVING m/s), its tuck in the air at AIR_EASE.
pub const DRIVE_EASE: f32 = 6.;
pub const DRIVE_MIN: f32 = 0.4;
pub const DRIVE_MAX: f32 = 1.2;
pub const WALK_MOVING: f32 = 0.05;
pub const AIR_EASE: f32 = 10.;
/// The drawn head's nod follows the look at NOD_EASE a second, never more than NOD_LAG radians behind (bend.rs's ease).
pub const NOD_EASE: f32 = 9.;
pub const NOD_LAG: f32 = 0.3;
impl Titan {
    /// The view's `stagger`: 0 to 1.
    pub fn stagger(&self) -> f32 {
        (self.legs / STAGGER_LEGS).clamp(0., 1.)
    }
    pub fn small(&self, id: u32) -> Option<&Small> {
        self.smalls.iter().find(|s| s.id == id)
    }
    fn small_mut(&mut self, id: u32) -> Option<&mut Small> {
        self.smalls.iter_mut().find(|s| s.id == id)
    }
    /// Whether it may start a move: standing or walking, on the ground.
    pub fn free(&self) -> bool {
        matches!(self.mv, Move::Idle | Move::Walk) && !self.airborne
    }
    fn tell(&mut self, mut event: Event) {
        self.next_event += 1;
        event.id = self.next_event;
        self.events.push(event);
    }
    /// Seconds into its move.
    fn t(&self, now: f32) -> f32 {
        now - self.started
    }
}
/// How a bot plays at a skill (both sides): seconds to react to a tell or a new target, the chance of judging a tell
/// right (jumping a wave, getting out of a swat's way), and a titan's seconds between attacks.
#[derive(Clone, Copy, Debug)]
pub struct Wits {
    pub react: f32,
    pub judge: f32,
    pub pause: f32,
    /// The chance a bot titan takes an opening when it sees one (it hesitates otherwise, and looks again later).
    pub nerve: f32,
    /// A cunning bot titan (hard) hunts whoever hurts it most and the weakest, keeps its nape tucked (crouched) while it
    /// is flanked or its legs are near a stagger, and backs off rather than walk into one; it judges each blow where its
    /// victims will be as it lands (their walk carried on over its wind-up), strikes with the ready blow that catches the
    /// most of them, and charges only down a lane its target will still be in when it gets there.
    pub cunning: bool,
    /// How far off (seconds, either way) a small bot's jump over a shockwave is from the moment it means to jump: it
    /// judges the front's coming from the wind-up, as a person does, not to the step.
    pub timing: f32,
    /// A sharp bot titan (normal and hard) chases on its feet, crouching only as it strikes low, and strikes at whoever
    /// is in a blow's way; a plain one (easy) crouch-walks after its target and strikes at it alone.
    pub sharp: bool,
}
pub fn wits(skill: crate::aim::Skill) -> Wits {
    let mut w = wits_of(skill);
    if let Some(v) = knob("cunning") {
        w.cunning = v > 0.;
    }
    if let Some(v) = knob("judge") {
        w.judge = v;
    }
    if let Some(v) = knob("timing") {
        w.timing = v;
    }
    if let Some(v) = knob("nerve") {
        w.nerve = v;
    }
    if let Some(v) = knob("sharp") {
        w.sharp = v > 0.;
    }
    if let Some(v) = knob("pause") {
        w.pause = v;
    }
    w
}
fn wits_of(skill: crate::aim::Skill) -> Wits {
    use crate::aim::Skill;
    match skill {
        Skill::Easy => Wits { react: 0.55, judge: 0.35, pause: 0.9, nerve: 0.3, cunning: false, timing: 0.45, sharp: false },
        Skill::Normal => Wits { react: 0.32, judge: 0.55, pause: 0.45, nerve: 1., cunning: false, timing: 0.35, sharp: true },
        Skill::Hard => Wits { react: 0.2, judge: 0.55, pause: 0.15, nerve: 1., cunning: true, timing: 0.35, sharp: true },
        Skill::Perfect => Wits { react: 0.1, judge: 1., pause: 0.1, nerve: 1., cunning: true, timing: 0., sharp: true },
    }
}

/// A tuning knob the balance runs may set (TITAN_KNOBS="name=value,...": the bots' wits by name (judge, timing, nerve,
/// cunning, sharp, pause) for the skill played, and `recover`); never in a real build.
#[cfg(test)]
fn knob(name: &str) -> Option<f32> {
    std::env::var("TITAN_KNOBS").ok()?.split(',').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).and_then(|(_, v)| v.parse().ok()))
}
#[cfg(not(test))]
fn knob(_: &str) -> Option<f32> {
    None
}
/// A hotdog's height.
const SMALL_TALL: f32 = actor::HEIGHT as f32;
const SMALL_RADIUS: f32 = crate::feast::HOTDOG_RADIUS as f32;

impl Feast {
    // ----- Setting a round out.

    /// Sets a round out as Titan (called by `set_rules`): a titan and the small hotdogs (the rules' count), the guns out
    /// from the start, the clock, and the titan cast (`cast_titan`).
    pub(crate) fn open_titan(&mut self) {
        self.openings.clear();
        self.stage = crate::feast::Stage::FreeForAll;
        self.famine_at = self.elapsed - 5.;
        if self.rules.ends() {
            self.ends = self.elapsed + self.rules.time();
        }
        self.alert = self.rules.banner();
        for item in &mut self.pickups {
            item.ready = 0.;
        }
        let n = 1 + self.rules.small_count();
        self.size_roster(n);
        let footing = Footing::of(&self.level);
        let previous = self.titan_previous;
        self.titan = Some(Titan { footing, previous, ..Default::default() });
        self.cast_titan();
    }
    /// A round that is no longer Titan: the hotdogs a siege has, on one team, set out afresh, with the siege's opening
    /// to come (Free For All, a table and a dojo set their own), as a Forts round's `unfort`.
    pub(crate) fn untitan(&mut self) {
        self.players.truncate(crate::feast::NAMES.len());
        for p in &mut self.players {
            (p.team, p.crouch) = (0, false);
        }
        let ids: Vec<u32> = self.players.iter().map(|p| p.id).collect();
        for id in ids {
            self.fit(id);
        }
        (self.stage, self.famine_at, self.wave_spawned, self.wave_kills) = (crate::feast::Stage::Warmup, 0., 0, 0);
        self.warmup = self.elapsed + if self.demo { 3. } else { 12. };
        self.alert = crate::feast::SIEGE_ALERT.into();
    }
    /// The roster made `n` hotdogs long: the last let go, or more added (bots, at the map's starts, with the loadout).
    fn size_roster(&mut self, n: usize) {
        while self.players.len() > n {
            let p = self.players.pop().unwrap();
            if let Ok(prop) = usize::try_from(p.carry) {
                if prop < self.props.len() {
                    self.props.drop_prop(prop, [0.; 3], [0.; 3]);
                }
            }
        }
        while self.players.len() < n {
            let i = self.players.len();
            let (at, yaw) = self.level.start(i);
            let mut player = Player { id: i as u32 + 1, name: SMALL_NAMES[i % SMALL_NAMES.len()].into(), p: at.to_array(), yaw, hp: 100., alive: true, bot: self.demo || i > 0, camp: [at.x, at.z], grounded: true, carry: -1, ..Default::default() };
            crate::magazine::loadout(&mut player);
            self.players.push(player);
        }
        self.embody();
    }
    /// Chooses the round's titan by the rules' pick (`cast`: who is a person now, the last round's titan, the dice) and
    /// sets everybody out: the titan whole at its start with its health for the small hotdogs there are, the small
    /// hotdogs at theirs with the gun basics, the pool full. The server calls it again once it has said who is a person.
    pub fn cast_titan(&mut self) {
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let humans: Vec<bool> = self.players.iter().map(|p| !p.bot).collect();
        let seed = self.card_seed ^ (self.round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let id = cast(self.rules.pick, &humans, t.previous, seed);
        let smalls: Vec<u32> = self.players.iter().filter(|p| p.id != id && !p.benched).map(|p| p.id).collect();
        let now = self.elapsed;
        let t = self.titan.as_mut().unwrap();
        t.id = id;
        t.max = HP_BASE + HP_PER * smalls.len() as f32;
        t.pool = PER_SMALL * smalls.len() as u32;
        (t.reserve, t.over, t.tally) = (t.pool, OVER_NONE, Tally::default());
        t.smalls = smalls.iter().map(|&id| Small { id, stepped: f32::NEG_INFINITY, ..Default::default() }).collect();
        t.minds = smalls.iter().map(|&id| Mind { id, judged: f32::NEG_INFINITY, leg: id as usize % 2, ..Default::default() }).collect();
        t.brain = Brain::default();
        Self::still(t, now);
        for p in &mut self.players {
            p.team = (p.id == id) as u8;
        }
        let ids: Vec<u32> = self.players.iter().map(|p| p.id).collect();
        for id in ids {
            self.fit(id);
        }
        self.embody();
    }
    /// The titan's move and its moving about, as a round (or a life) begins: standing, nothing under way.
    fn still(t: &mut Titan, now: f32) {
        (t.mv, t.started, t.len, t.phase, t.ready) = (Move::Idle, now, 0., 0, [now; 5]);
        (t.legs, t.rest, t.queued, t.landed, t.staggered) = (0., 0., None, None, false);
        (t.airborne, t.vy, t.carry, t.vel) = (false, 0., Vec2::ZERO, Vec2::ZERO);
        (t.stride, t.drive, t.air) = (0., 0., 0.);
        t.waves.clear();
        t.struck.clear();
        t.threat = (0, f32::NEG_INFINITY);
        (t.figure, t.crouching, t.blend, t.shown, t.nod) = (figures::STANDING, 0., None, None, None);
    }
    /// A hotdog fitted out in Titan (`fit` calls it): the titan whole at its start, holding nothing; a small hotdog at a
    /// small hotdog's start.
    pub(crate) fn titan_fit(&mut self, id: u32) {
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let (titan, max) = (t.id, t.max);
        let now = self.elapsed;
        if id == titan {
            let (at, yaw) = self.titan_start();
            self.disarm_titan(id);
            if let Some(p) = self.player_mut(id) {
                (p.hp, p.p, p.yaw, p.crouch, p.prone, p.camp, p.team) = (max, [at.x, FLOOR, at.y], yaw, false, false, [at.x, at.y], 1);
                if p.bot {
                    p.name = NAME.into();
                }
            }
            let t = self.titan.as_mut().unwrap();
            Self::still(t, now);
            (t.want, t.last) = (yaw, at);
        } else {
            let rank = self.players.iter().filter(|p| p.id != titan && p.id < id).count();
            let (at, yaw) = self.small_start(rank);
            if let Some(p) = self.player_mut(id) {
                (p.p, p.yaw, p.camp, p.team) = (at.to_array(), yaw, [at.x, at.z], 0);
                if p.bot {
                    p.name = SMALL_NAMES[(id as usize).saturating_sub(1) % SMALL_NAMES.len()].into();
                }
            }
        }
        self.embody();
    }
    /// The titan holds no gun.
    pub(crate) fn disarm_titan(&mut self, id: u32) {
        if let Some(p) = self.player_mut(id) {
            (p.owned, p.ammo, p.reserve, p.reload) = ([false; crate::feast::GUNS], [0; crate::feast::GUNS], [0; crate::feast::GUNS], None);
            (p.weapon, p.gun, p.aimed, p.volley, p.stim) = (0, 0, false, 0, 0.);
        }
    }
    /// Where the titan starts: the map's first start (the nearest spot its footing allows), facing the others.
    fn titan_start(&self) -> (Vec2, f32) {
        let Some(t) = self.titan.as_ref() else {
            return (Vec2::ZERO, 0.);
        };
        let (at, yaw) = self.level.start(0);
        let at = t.footing.nearest_clear(Vec2::new(at.x, at.z));
        let n = self.level.map.starts.len();
        let others: Vec<Vec2> = (1..n).map(|i| { let p = self.level.map.starts[i].p; Vec2::new(p[0] as f32, p[1] as f32) }).collect();
        let yaw = if others.is_empty() {
            yaw
        } else {
            let middle = others.iter().copied().sum::<Vec2>() / others.len() as f32;
            let to = middle - at;
            if to.length() > 1e-3 { (-to.x).atan2(-to.y) } else { yaw }
        };
        (at, yaw)
    }
    /// The start of the small hotdog of `rank`: the map's starts after the first, in turn.
    fn small_start(&self, rank: usize) -> (Vec3, f32) {
        let n = self.level.map.starts.len();
        if n < 2 {
            let (at, yaw) = self.level.start(0);
            let a = rank as f32 * 1.1;
            return (at + Vec3::new(a.cos(), 0., a.sin()) * (RADIUS + 0.3), yaw);
        }
        self.level.start(1 + rank % (n - 1))
    }
    /// The titan's body in the bodies' world: its size, held still, where it stands (it is moved by titan.rs alone).
    pub(crate) fn titan_body(&mut self) {
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let (id, mv, airborne) = (t.id, t.mv, t.airborne);
        let Some(p) = self.players.iter().find(|p| p.id == id && p.alive) else {
            return;
        };
        let stance = stance_of(p, mv);
        let at = [p.p[0] as f64, p.p[1] as f64, p.p[2] as f64];
        let body = match self.bodies.get_mut(id) {
            Some(body) => body,
            None => self.bodies.add(ghost(id, p.p, stance)),
        };
        (body.p, body.v, body.knock, body.launched, body.grounded) = (at, [0.; 3], [0.; 2], false, !airborne);
        (body.radius, body.height, body.mass, body.idle, body.crouch, body.prone) = (RADIUS as f64, stature(stance) as f64, MASS, true, false, false);
        (body.jump, body.perch, body.pin, body.wish) = (0., None, None, [0.; 2]);
        self.echo.insert(id, p.p);
    }
    /// Where a round (or a blow) from `origin` along `dir`, `size` round, first meets hotdog `p`: how far along, the zone
    /// (the titan's; Body for a small hotdog) and where.
    pub(crate) fn hit_on(&self, p: &Player, origin: Vec3, dir: Vec3, size: f32) -> Option<(f32, Zone, Vec3)> {
        if self.is_titan(p.id) {
            let shape = self.titan_shape().unwrap_or_else(|| Shape::of(p, Move::Idle));
            return hit(&shape, origin, dir, size).map(|h| (h.t, h.zone, h.point));
        }
        crate::head::hit(p, origin, dir, size).map(|h| (h.t, Zone::Body, h.point))
    }

    // ----- Input.

    /// A person's command as the titan, the server's and single player's alike (see the module's documentation): the
    /// crouch (while free), the press, the look, the turn and the walk.
    pub fn titan_command(&mut self, id: u32, c: &Command, dt: f32) {
        let now = self.elapsed;
        if !self.is_titan(id) || !self.player(id).is_some_and(|p| p.alive) || self.stage == crate::feast::Stage::RoundOver {
            return;
        }
        let t = self.titan.as_mut().unwrap();
        let rose = |bit: u16| c.has(bit) && t.buttons & bit == 0;
        let (fire, scope) = (rose(button::FIRE), rose(button::SCOPE));
        t.buttons = c.buttons;
        t.want = c.yaw();
        let free = t.free();
        let _ = now;
        if free {
            if let Some(p) = self.player_mut(id) {
                p.crouch = c.has(button::CROUCH);
            }
        }
        let crouch = self.player(id).is_some_and(|p| p.crouch);
        let pressed = if c.has(button::STIM) {
            Some(Move::Charge)
        } else if scope {
            Some(Move::Smash)
        } else if fire {
            Some(if crouch { Move::SwatLow } else { Move::SwatHigh })
        } else if c.has(button::MELEE) {
            Some(if crouch { Move::Kick } else { Move::Stomp })
        } else if c.has(button::JUMP) {
            Some(Move::Jump)
        } else {
            None
        };
        if let Some(mv) = pressed {
            self.titan_press(mv);
        }
        self.look(id, c.pitch());
        self.titan_drive(c.stride(1., 1.), c.yaw(), dt);
        self.titan_go();
    }
    /// The titan asks for a move: kept BUFFER seconds until it can start. A charge asked for while it runs ends the run.
    pub fn titan_press(&mut self, mv: Move) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_mut() else {
            return;
        };
        if mv == Move::Charge && t.mv == Move::Charge && t.phase == 1 {
            let tt = t.t(now);
            (t.run_end, t.len) = (tt, tt + Move::Charge.data().recovery);
            return;
        }
        t.queued = Some((mv, now));
    }
    /// Starts the move asked for, if it can start now (free, on the ground, its cooldown over).
    fn titan_go(&mut self) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_mut() else {
            return;
        };
        let Some((mv, at)) = t.queued else {
            return;
        };
        if now - at > BUFFER + 1e-6 {
            t.queued = None;
            return;
        }
        if !t.free() || mv.ready_slot().is_some_and(|slot| now < t.ready[slot]) {
            return;
        }
        t.queued = None;
        self.titan_begin(mv);
    }
    /// The titan starts a move now: its cooldown counts from its end; an attack's wind-up is told (the tell), and a reel's
    /// start.
    fn titan_begin(&mut self, mv: Move) {
        let now = self.elapsed;
        let chest = self.titan_id().and_then(|id| self.player(id)).map_or(Vec3::ZERO, |p| Vec3::from_array(p.p) + Vec3::Y * TALL * 0.6);
        let Some(t) = self.titan.as_mut() else {
            return;
        };
        let d = mv.data();
        (t.mv, t.started, t.len, t.phase, t.landed) = (mv, now, d.total(), 0, None);
        t.struck.clear();
        t.run_end = d.startup + d.active;
        if let Some(slot) = mv.ready_slot() {
            t.ready[slot] = now + d.total() + d.cooldown;
        }
        t.tally.moves[mv as usize] += 1;
        if mv == Move::Jump {
            t.run_up = t.vel;
        }
        let by = t.id;
        if d.attacks() {
            t.tell(Event { kind: Some(EventKind::Tell), time: now, by, mv: mv as u8, amount: d.startup, p: chest.to_array(), ..Default::default() });
        }
        if d.kind == Kind::Reel {
            if mv == Move::Stagger {
                t.tally.staggers += 1;
            }
            t.queued = None;
            t.tell(Event { kind: Some(EventKind::StaggerStart), time: now, by, mv: mv as u8, p: chest.to_array(), ..Default::default() });
        }
    }
    /// Turns the titan towards `want` and walks it along `way` (world [x, z], at most 1 long: the share of its pace), as
    /// its move allows: free it turns at TURN and walks at its pace (`stride_pace`), winding up half as fast, in a blow
    /// not at all; reeling, in the air or down it neither turns nor walks.
    pub fn titan_drive(&mut self, way: Vec2, want: f32, dt: f32) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_mut() else {
            return;
        };
        t.want = want;
        let (id, mv, phase, free, airborne) = (t.id, t.mv, t.phase, t.free(), t.airborne);
        let kind = mv.data().kind;
        let rate = match (free, kind, phase) {
            _ if airborne => 0.,
            (true, _, _) => TURN,
            (_, Kind::Reel | Kind::Down | Kind::Victory | Kind::Charge, _) => 0.,
            (_, _, 0) => TURN * 0.5,
            (_, _, 2) => TURN * 0.3,
            _ => 0.,
        };
        let Some(i) = self.players.iter().position(|p| p.id == id && p.alive) else {
            return;
        };
        let yaw = turn(self.players[i].yaw, want, dt, rate);
        self.players[i].yaw = yaw;
        if !free || dt <= 0. {
            return;
        }
        let share = way.length().min(1.);
        if share < 0.01 {
            return;
        }
        let dir = way / way.length();
        let pace = if self.players[i].crouch { CROUCH_WALK } else { WALK };
        let step = dir * stride_pace(pace, yaw, dir) * share * dt;
        let from = flat(self.players[i].p);
        let (to, _) = self.titan.as_ref().unwrap().footing.slide(from, step);
        (self.players[i].p[0], self.players[i].p[2]) = (to.x, to.y);
        let _ = now;
        self.titan_body();
    }

    // ----- A step.

    /// One step of Titan (from `step`, before the bots choose): the move runs on (its phases, a charge's run, a jump's
    /// flight), its blows land, its waves spread, it treads on whoever is under it, and the events of more than
    /// EVENT_LIFE seconds ago are forgotten.
    pub(crate) fn titan_step(&mut self, dt: f32) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_mut() else {
            return;
        };
        t.events.retain(|e| now - e.time < EVENT_LIFE);
        t.played = true;
        let fade = 0.5f32.powf(dt / RECENT);
        for s in &mut t.smalls {
            s.recent *= fade;
        }
        let id = t.id;
        // (A small hotdog seated since the round was set out: on the record from now.)
        let missing: Vec<u32> = self.players.iter().filter(|p| p.id != id && !t.smalls.iter().any(|s| s.id == p.id)).map(|p| p.id).collect();
        for m in missing {
            t.smalls.push(Small { id: m, stepped: f32::NEG_INFINITY, ..Default::default() });
            t.minds.push(Mind { id: m, judged: f32::NEG_INFINITY, leg: m as usize % 2, ..Default::default() });
        }
        if t.over != OVER_NONE || !self.player(id).is_some_and(|p| p.alive) || self.stage == crate::feast::Stage::RoundOver {
            if let Some(t) = self.titan.as_mut() {
                t.waves.clear();
            }
            // (Its figure goes on as drawn: felled, it falls.)
            self.titan_figure(dt);
            return;
        }
        self.titan_run();
        self.titan_go();
        self.titan_air(dt);
        self.titan_charge(dt);
        self.titan_sweep();
        self.titan_waves();
        self.titan_tread(dt);
        self.titan_figure(dt);
        self.titan_body();
    }
    /// The move's phases: into its active window (what it does then), into its recovery, and its end.
    fn titan_run(&mut self) {
        let now = self.elapsed;
        loop {
            let Some(t) = self.titan.as_mut() else {
                return;
            };
            let d = t.mv.data();
            let tt = t.t(now);
            let active_end = match t.mv {
                Move::Jump => t.landed.unwrap_or(f32::INFINITY),
                Move::Charge => t.run_end,
                _ => d.startup + d.active,
            };
            let phase = if tt < d.startup - 1e-6 { 0 } else if tt < active_end - 1e-6 { 1 } else { 2 };
            if t.phase < phase {
                t.phase += 1;
                let (mv, entered) = (t.mv, t.phase);
                if entered == 1 {
                    self.titan_active(mv);
                }
                continue;
            }
            if t.len > 0. && tt >= t.len - 1e-6 && !(t.mv == Move::Jump && t.landed.is_none()) {
                let mv = t.mv;
                match mv {
                    Move::Down | Move::Roar => {
                        t.len = 0.;
                    }
                    _ => {
                        if d.kind == Kind::Reel {
                            let by = t.id;
                            t.tell(Event { kind: Some(EventKind::StaggerEnd), time: now, by, mv: mv as u8, ..Default::default() });
                            if mv == Move::Stagger {
                                (t.rest, t.legs) = (now + STAGGER_REST, 0.);
                            }
                        }
                        (t.mv, t.started, t.len, t.phase) = (Move::Idle, now, 0., 0);
                    }
                }
            }
            return;
        }
    }
    /// What a move does as its active window opens: a charge starts its run, the fists land and the wave sets out, a swat
    /// starts its sweep, a foot comes down, a jump takes off.
    fn titan_active(&mut self, mv: Move) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let id = t.id;
        let Some(p) = self.player(id).cloned() else {
            return;
        };
        let (at, ahead) = (flat(p.p), facing(p.yaw));
        let d = mv.data();
        let event = |kind: EventKind, where_: Vec3, amount: f32| Event { kind: Some(kind), time: now, by: id, mv: mv as u8, amount, p: where_.to_array(), ..Default::default() };
        match mv {
            Move::Charge => {
                let e = event(EventKind::ChargeStart, Vec3::from_array(p.p), 0.);
                self.titan.as_mut().unwrap().tell(e);
            }
            Move::Smash => {
                let reach = if p.crouch { d.ahead * 0.8 } else { d.ahead };
                let spot = at + ahead * reach;
                let surface = FLOOR + self.top_at(spot);
                let centre = Vec3::new(spot.x, surface, spot.y);
                let e = event(EventKind::Smash, centre, d.reach);
                let t = self.titan.as_mut().unwrap();
                t.tell(e);
                t.waves.push(Wave { mv, centre, at: now, surface, passed: vec![] });
            }
            Move::SwatHigh | Move::SwatLow => {
                let right = Vec2::new(-ahead.y, ahead.x);
                let hand = at + right * 0.6;
                let height = p.p[1] + (d.band[0] + d.band[1]) / 2.;
                let e = event(EventKind::Swat, Vec3::new(hand.x, height, hand.y), 0.);
                self.titan.as_mut().unwrap().tell(e);
            }
            Move::Stomp => {
                let foot = at + ahead * d.ahead;
                let e = event(EventKind::Stomp, Vec3::new(foot.x, FLOOR, foot.y), d.reach);
                self.titan.as_mut().unwrap().tell(e);
                let victims: Vec<(u32, f32, Vec2)> = self
                    .smalls_alive()
                    .into_iter()
                    .filter_map(|q| {
                        let rel = flat(q.p) - foot;
                        let far = rel.length();
                        (far <= d.reach + SMALL_RADIUS && q.p[1] < FLOOR + 0.06).then(|| (q.id, (far / d.reach).min(1.), rel.normalize_or(ahead)))
                    })
                    .collect();
                for (victim, f, out) in victims {
                    let damage = d.damage + (d.edge - d.damage) * f;
                    self.titan_blow(victim, mv, damage, Vec3::new(foot.x, FLOOR, foot.y), out * d.knock, d.lift);
                }
            }
            Move::Kick => {
                let foot = at + ahead * 0.4;
                let e = event(EventKind::Stomp, Vec3::new(foot.x, FLOOR + 0.1, foot.y), d.reach);
                self.titan.as_mut().unwrap().tell(e);
            }
            Move::Jump => {
                // (It takes off with the walk it had as it began crouching for the jump.)
                let t = self.titan.as_mut().unwrap();
                let v = if t.run_up.length() > WALK { t.run_up / t.run_up.length() * WALK } else { t.run_up };
                (t.airborne, t.vy, t.carry) = (true, JUMP_SPEED, v);
                if let Some(p) = self.player_mut(id) {
                    p.grounded = false;
                }
            }
            _ => {}
        }
    }
    /// The height above the floor the fists land on at `spot`: the top of a deck, a ramp or a block there no higher than
    /// REACH_LOW (low furniture), else the floor.
    fn top_at(&self, spot: Vec2) -> f32 {
        let (x, z) = (spot.x as f64, spot.y as f64);
        let mut top = (self.level.height(x, z) as f32).clamp(0., REACH_LOW);
        if top >= REACH_LOW {
            top = 0.;
        }
        for b in &self.level.map.blocks {
            let r = b.rect;
            if x >= r[0] && x <= r[2] && z >= r[1] && z <= r[3] && (b.height as f32) <= REACH_LOW {
                top = top.max(b.height as f32);
            }
        }
        top
    }
    /// The living small hotdogs (clones).
    fn smalls_alive(&self) -> Vec<Player> {
        let id = self.titan_id().unwrap_or(0);
        self.players.iter().filter(|p| p.alive && p.id != id).cloned().collect()
    }
    /// The jump's flight: up, along, and down; landing is a wave under it (and a stagger that waited for it).
    fn titan_air(&mut self, dt: f32) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_mut() else {
            return;
        };
        if !t.airborne {
            return;
        }
        let id = t.id;
        let g = actor::GRAVITY as f32;
        let Some(i) = self.players.iter().position(|p| p.id == id) else {
            return;
        };
        let t = self.titan.as_mut().unwrap();
        let mut y = self.players[i].p[1] + t.vy * dt - 0.5 * g * dt * dt;
        t.vy -= g * dt;
        let (to, _) = t.footing.slide(flat(self.players[i].p), t.carry * dt);
        let landed = y <= FLOOR;
        if landed {
            y = FLOOR;
        }
        self.players[i].p = [to.x, y, to.y];
        self.players[i].grounded = landed;
        if !landed {
            return;
        }
        let t = self.titan.as_mut().unwrap();
        t.airborne = false;
        let tt = t.t(now);
        (t.landed, t.len, t.vy) = (Some(tt), tt + Move::Jump.data().recovery, 0.);
        let centre = Vec3::new(to.x, FLOOR, to.y);
        t.tell(Event { kind: Some(EventKind::JumpLand), time: now, by: id, mv: Move::Jump as u8, amount: Move::Jump.data().reach, p: centre.to_array(), ..Default::default() });
        t.waves.push(Wave { mv: Move::Jump, centre, at: now, surface: FLOOR, passed: vec![] });
        let staggered = std::mem::take(&mut t.staggered);
        // Whoever it came down on is crushed.
        let under: Vec<(u32, Vec2)> = self.smalls_alive().into_iter().filter(|q| (flat(q.p) - to).length() < RADIUS + SMALL_RADIUS && q.p[1] < FLOOR + TALL).map(|q| (q.id, (flat(q.p) - to).normalize_or(Vec2::X))).collect();
        for (victim, out) in under {
            self.titan_blow(victim, Move::Jump, Move::Jump.data().damage * 1.6, centre, out * 1.2, 0.6);
            if let Some(t) = self.titan.as_mut() {
                if let Some(w) = t.waves.last_mut() {
                    w.passed.push(victim);
                }
            }
        }
        if staggered {
            self.titan_begin(Move::Stagger);
        }
    }
    /// The charge's run: ahead at CHARGE_SPEED, steering a little; whoever it meets is knocked aside; a wall stops it,
    /// and it stumbles.
    fn titan_charge(&mut self, dt: f32) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        if t.mv != Move::Charge || t.phase != 1 {
            return;
        }
        let (id, want) = (t.id, t.want);
        let Some(i) = self.players.iter().position(|p| p.id == id) else {
            return;
        };
        let yaw = turn(self.players[i].yaw, want, dt, CHARGE_TURN);
        self.players[i].yaw = yaw;
        let ahead = facing(yaw);
        let from = flat(self.players[i].p);
        let t = self.titan.as_ref().unwrap();
        let (to, stopped) = t.footing.slide(from, ahead * CHARGE_SPEED * dt);
        let made = (to - from).dot(ahead);
        (self.players[i].p[0], self.players[i].p[2]) = (to.x, to.y);
        if stopped && made < CHARGE_SPEED * dt * 0.5 {
            let contact = to + ahead * RADIUS;
            let e = Event { kind: Some(EventKind::ChargeWall), time: now, by: id, mv: Move::Charge as u8, p: [contact.x, FLOOR + TALL * 0.5, contact.y], ..Default::default() };
            self.titan.as_mut().unwrap().tell(e);
            self.titan_begin(Move::Stumble);
            return;
        }
        let d = Move::Charge.data();
        let feet = self.players[i].p[1];
        let struck = self.titan.as_ref().unwrap().struck.clone();
        let hits: Vec<(u32, Vec2)> = self
            .smalls_alive()
            .into_iter()
            .filter(|q| !struck.contains(&q.id) && q.p[1] < feet + TALL && q.p[1] + SMALL_TALL > feet)
            .filter_map(|q| {
                let rel = flat(q.p) - to;
                let far = rel.length();
                if far > d.reach + SMALL_RADIUS || (far > 1e-4 && ahead.dot(rel / far) < d.arc.cos()) {
                    return None;
                }
                let side = if ahead.perp_dot(rel) >= 0. { 1. } else { -1. };
                Some((q.id, (ahead.perp() * side + ahead * 0.6).normalize()))
            })
            .collect();
        for (victim, way) in hits {
            self.titan.as_mut().unwrap().struck.push(victim);
            let at = self.player(victim).map_or(Vec3::ZERO, |q| Vec3::from_array(q.p));
            let e = Event { kind: Some(EventKind::ChargeHit), time: now, by: id, to: victim, mv: Move::Charge as u8, amount: d.damage, p: at.to_array(), ..Default::default() };
            self.titan.as_mut().unwrap().tell(e);
            self.titan_blow(victim, Move::Charge, d.damage, Vec3::new(to.x, FLOOR, to.y), way * d.knock, d.lift);
        }
    }
    /// A swat's (or a kick's) sweep in its active window: each hotdog in its reach, its arc and its band, once.
    fn titan_sweep(&mut self) {
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let mv = t.mv;
        if !matches!(mv, Move::SwatHigh | Move::SwatLow | Move::Kick) || t.phase != 1 {
            return;
        }
        let (id, struck) = (t.id, t.struck.clone());
        let Some(p) = self.player(id).cloned() else {
            return;
        };
        let d = mv.data();
        let (at, ahead) = (flat(p.p), facing(p.yaw));
        let feet = p.p[1];
        let hits: Vec<(u32, Vec2)> = self
            .smalls_alive()
            .into_iter()
            .filter(|q| !struck.contains(&q.id))
            .filter(|q| mv != Move::SwatHigh || !self.in_den(q.id))
            .filter_map(|q| {
                let rel = flat(q.p) - at;
                let far = rel.length();
                let (low, high) = (q.p[1] - feet, q.p[1] - feet + SMALL_TALL);
                if far > d.reach + SMALL_RADIUS || high < d.band[0] || low > d.band[1] || (far > 1e-4 && ahead.dot(rel / far) < d.arc.cos()) {
                    return None;
                }
                let out = rel.normalize_or(ahead);
                // (A swat sweeps right to left: along the sweep and out; a kick, ahead.)
                let way = if mv == Move::Kick { (ahead + out * 0.3).normalize() } else { (out.perp() + out * 0.5).normalize() };
                Some((q.id, way))
            })
            .collect();
        for (victim, way) in hits {
            self.titan.as_mut().unwrap().struck.push(victim);
            self.titan_blow(victim, mv, d.damage, Vec3::new(at.x, feet + (d.band[0] + d.band[1]) / 2., at.y), way * d.knock, d.lift);
        }
    }
    /// The shockwaves spread: each hotdog the front reaches is hit once, by how far it is from the impact (heavy within
    /// DIRECT, falling to the move's `edge` at its reach), if it stands on the surface the wave spreads on: in the air it
    /// takes AIRBORNE_SHARE, in a den DEN_SHARE, behind solid cover nothing.
    fn titan_waves(&mut self) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let waves = t.waves.clone();
        let mut kept = vec![];
        for mut wave in waves {
            let d = wave.mv.data();
            let front = (now - wave.at) * d.wave;
            let hits: Vec<(u32, f32, Vec2, f32)> = self
                .smalls_alive()
                .into_iter()
                .filter(|q| !wave.passed.contains(&q.id))
                .filter_map(|q| {
                    let (gap, near) = crate::feast::body_gap(&q, wave.centre);
                    let flat_gap = (flat(q.p) - Vec2::new(wave.centre.x, wave.centre.z)).length() - SMALL_RADIUS;
                    let gap = gap.min(flat_gap.max(0.));
                    if gap > front.max(DIRECT) {
                        return None;
                    }
                    Some((q.id, gap, (flat(q.p) - Vec2::new(wave.centre.x, wave.centre.z)).normalize_or(Vec2::X), {
                        // The share: on its surface (or in the air over it), out of the cover's way.
                        let on = (q.p[1] - wave.surface).abs() <= 0.06;
                        let above = !q.grounded && q.p[1] > wave.surface - 0.01 && q.p[1] < wave.surface + 0.4;
                        let span = wave.centre.distance(near);
                        let raised = wave.centre + Vec3::Y * 0.03;
                        let covered = span > 0.05 && {
                            let way = (near - raised).normalize_or_zero();
                            self.cover(raised, way) + 0.01 < raised.distance(near)
                        };
                        let share = if covered || gap > d.reach {
                            0.
                        } else if gap <= DIRECT {
                            1.
                        } else if above {
                            AIRBORNE_SHARE
                        } else if on {
                            1.
                        } else {
                            0.
                        };
                        if share > 0. && self.in_den(q.id) && gap > DIRECT { share * DEN_SHARE } else { share }
                    }))
                })
                .collect();
            for (victim, gap, out, share) in hits {
                wave.passed.push(victim);
                if share <= 0. {
                    continue;
                }
                let f = if gap <= DIRECT { 1. } else { 1. - smooth(((gap - DIRECT) / (d.reach - DIRECT).max(1e-3)).clamp(0., 1.)) };
                let damage = (d.edge + (d.damage - d.edge) * f) * share;
                self.titan_blow(victim, wave.mv, damage, wave.centre, out * d.knock * f * share.max(0.4), d.lift * f * share.max(0.4));
            }
            if front <= d.reach + 0.2 {
                kept.push(wave);
            }
        }
        if let Some(t) = self.titan.as_mut() {
            t.waves = kept;
        }
    }
    /// The titan treads: its walk this step (its pace, its footfalls), the stance it shows (standing or walking), the
    /// stagger meter draining, and whoever it walks into shoved out from under it, stepped on if it was moving.
    fn titan_tread(&mut self, dt: f32) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let id = t.id;
        let Some(p) = self.player(id).cloned() else {
            return;
        };
        let at = flat(p.p);
        let t = self.titan.as_mut().unwrap();
        let moved = at - t.last;
        t.last = at;
        let speed = if dt > 0. { moved.length() / dt } else { 0. };
        if !t.airborne {
            t.vel = if dt > 0. { moved / dt } else { Vec2::ZERO };
        }
        // (Its walk is on its record as any hotdog's: the bodies' world never moves it, so it is said here.)
        let (vel, vy, airborne) = (t.vel, t.vy, t.airborne);
        if let Some(me) = self.players.iter_mut().find(|q| q.id == id) {
            me.v = [vel.x, if airborne { vy } else { 0. }, vel.y];
            (me.grounded, me.launched) = (!airborne, false);
        }
        let t = self.titan.as_mut().unwrap();
        if matches!(t.mv, Move::Idle | Move::Walk) {
            let want = if speed > 0.08 { Move::Walk } else { Move::Idle };
            if want != t.mv {
                (t.mv, t.started, t.len, t.phase) = (want, now, 0., 0);
            }
        }
        t.legs = (t.legs - STAGGER_LEGS / STAGGER_WINDOW * dt).max(0.);
        // Its stride, as the client draws its walk (src/titan/play.rs): the crouch eased, the drive towards its speed's
        // share of its pace, the phase on by the ground covered over the gait's cycle (backwards backing up, not
        // running), and a footfall as the phase passes each foot's (the left at 0, the right at PI).
        let ease = |rate: f32| 1. - (-dt * rate).exp();
        t.crouching += (p.crouch as u8 as f32 - t.crouching) * ease(CROUCH_EASE);
        let stance_move = matches!(t.mv, Move::Idle | Move::Walk);
        let crouched = t.crouching > 0.5;
        let pace = if crouched { CROUCH_WALK } else { WALK };
        let walking = stance_move && speed > WALK_MOVING;
        t.drive += ((walking as u8 as f32) * (speed / pace).clamp(DRIVE_MIN, DRIVE_MAX) - t.drive) * ease(DRIVE_EASE);
        t.air += (airborne as u8 as f32 - t.air) * ease(AIR_EASE);
        let running = t.mv == Move::Charge && running_at(beat(timing(Move::Charge, t.len), now - t.started));
        let ground = if moved.length() > 1. { 0. } else { moved.length() };
        let way = if moved.dot(facing(p.yaw)) < -0.002 * dt.max(1e-3) && !running { -1. } else { 1. };
        let was = t.stride;
        let to = was + way * ground / gait_cycle(crouched, running) * TAU;
        t.stride = to.rem_euclid(TAU);
        let (lo, hi) = if to >= was { (was, to) } else { (to, was) };
        let falls = ((hi / PI).floor() - (lo / PI).floor()) as i32;
        if falls > 0 && !airborne && (t.drive > 0.2 || running) {
            // (The foot that came down: the left at even multiples of PI, the right at odd.)
            let at_fall = if to >= was { (hi / PI).floor() } else { (lo / PI).floor() + 1. };
            let side = (at_fall as i32).rem_euclid(2) as usize;
            let ankle = Quat::from_rotation_y(p.yaw) * t.figure.at(ANKLE + side);
            let foot = at + Vec2::new(ankle.x, ankle.z);
            let mv = if running { Move::Charge } else { Move::Walk };
            let shake = (speed / CHARGE_SPEED).clamp(0.35, 1.);
            t.tell(Event { kind: Some(EventKind::Step), time: now, by: id, mv: mv as u8, amount: shake, p: [foot.x, FLOOR, foot.y], ..Default::default() });
        }
        // Whoever is under it: shoved out, and hurt if it was walking.
        let feet = p.p[1];
        let top = feet + stature(stance_of(&p, t.mv));
        let reach = RADIUS + SMALL_RADIUS;
        let under: Vec<u32> = self.smalls_alive().into_iter().filter(|q| (flat(q.p) - at).length() < reach - 1e-3 && q.p[1] < top && q.p[1] + SMALL_TALL > feet).map(|q| q.id).collect();
        for victim in under {
            let Some(q) = self.player(victim).cloned() else {
                continue;
            };
            let rel = flat(q.p) - at;
            let out = rel.try_normalize().unwrap_or(facing(p.yaw).perp());
            let need = reach + 0.004 - rel.length();
            for way in [out, (out + out.perp()).normalize(), (out - out.perp()).normalize(), out.perp(), -out.perp()] {
                let now_at = self.bodies.get(victim).map(|b| Vec2::new(b.p[0] as f32, b.p[2] as f32)).unwrap_or(flat(q.p));
                if (now_at - at).length() >= reach - 1e-3 {
                    break;
                }
                let push = way * (need + 0.01);
                self.bodies.walk(&self.level, victim, push.x as f64, push.y as f64);
            }
            if speed > 0.2 {
                let t = self.titan.as_mut().unwrap();
                let fresh = t.small(victim).is_some_and(|s| now - s.stepped >= STEP_WAIT);
                if fresh {
                    if let Some(s) = t.small_mut(victim) {
                        s.stepped = now;
                    }
                    t.tell(Event { kind: Some(EventKind::Stepped), time: now, by: id, to: victim, mv: Move::Walk as u8, amount: STEP_DAMAGE, p: q.p, ..Default::default() });
                    self.titan_blow(victim, Move::Walk, STEP_DAMAGE, Vec3::new(at.x, FLOOR, at.y), out * 0.6, 0.3);
                }
            }
        }
        self.report();
    }
    /// The titan's figure moves on as the client draws it (see `Titan::figure`): the crouch eased, a new move (or the same
    /// begun again) blended in from what was drawn, the stance's figure or the move's at its time.
    pub(crate) fn titan_figure(&mut self, dt: f32) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let nod = self.player(t.id).map(|p| figure_nod(stance_of(p, t.mv), p.pitch));
        let t = self.titan.as_mut().unwrap();
        t.nod = match (t.nod, nod) {
            (Some(drawn), Some(target)) => Some((drawn + (target - drawn) * (1. - (-dt * NOD_EASE).exp())).clamp(target - NOD_LAG, target + NOD_LAG)),
            (_, target) => target,
        };
        let stance_move = matches!(t.mv, Move::Idle | Move::Walk);
        let fresh = match t.shown {
            None => true,
            Some((mv, started)) => mv != t.mv || (started != t.started && !stance_move),
        };
        if fresh {
            let length = match t.mv {
                Move::Down => FALL_BLEND,
                _ if stance_move => SETTLE,
                mv => BLEND.min(timing(mv, t.len)[0] * 0.5).max(BLEND_MIN),
            };
            t.blend = Some((t.figure, now, length));
            t.shown = Some((t.mv, t.started));
        }
        let target = if stance_move {
            let walk = stride_frame(t.crouching, t.drive, t.stride);
            // (Off the ground standing, its legs tucked: a jump's.)
            if t.air > 0.01 { walk.lerp(&figures::TUCK, t.air) } else { walk }
        } else {
            let f = move_frame(t.mv, now - t.started, t.len);
            // (The charge's run, eased in and out of its keys either side.)
            let b = beat(timing(t.mv, t.len), now - t.started);
            if t.mv == Move::Charge && running_at(b) {
                let (from, to) = figures::CHARGE_RUN;
                let into = ((b - from) / 0.12).clamp(0., 1.) * ((to - b) / 0.12).clamp(0., 1.);
                f.lerp(&sampled(figures::RUN, t.stride), into)
            } else {
                f
            }
        };
        t.figure = match t.blend {
            Some((from, start, length)) if now - start < length => {
                let k = ((now - start) / length).clamp(0., 1.);
                from.lerp(&target, k * k * (3. - 2. * k))
            }
            _ => target,
        };
    }
    /// The titan's body now, as drawn (its figure, `Titan::figure`), for rounds, blasts and blows.
    pub fn titan_shape(&self) -> Option<Shape> {
        let t = self.titan.as_ref()?;
        let p = self.player(t.id)?;
        let stance = stance_of(p, t.mv);
        Some(Shape::nodding(p, &t.figure, stance, t.nod.unwrap_or_else(|| figure_nod(stance, p.pitch))))
    }
    /// A blow of the titan's move `mv` on small hotdog `victim`: `damage` HP on its record (feast::TITAN), thrown by
    /// `push` (m/s, [x, z]) and `lift`, and told.
    fn titan_blow(&mut self, victim: u32, mv: Move, damage: f32, from: Vec3, push: Vec2, lift: f32) {
        let now = self.elapsed;
        let Some(id) = self.titan_id() else {
            return;
        };
        let Some(q) = self.player(victim).filter(|q| q.alive).cloned() else {
            return;
        };
        let to = Vec3::from_array(q.p) - from;
        self.harm(victim, damage, true, crate::feast::Damage::by(crate::feast::TITAN, id, from, Vec3::new(to.x, 0., to.z)));
        let m = actor::HOTDOG_MASS;
        self.bodies.impulse(victim, [push.x as f64 * m, lift as f64 * m, push.y as f64 * m]);
        let dead = !self.player(victim).is_some_and(|q| q.alive);
        let t = self.titan.as_mut().unwrap();
        t.tally.blows[mv as usize] += 1;
        t.tell(Event { kind: Some(EventKind::Hit), time: now, by: id, to: victim, mv: mv as u8, amount: damage, p: q.p, ..Default::default() });
        if dead {
            if mv == Move::Walk {
                t.tally.stepped_deaths += 1;
            } else {
                t.tally.small_deaths[mv as usize] += 1;
            }
        }
        if t.brain.target == victim {
            t.brain.landed = now;
        }
    }

    // ----- Hits on the titan.

    /// A hit on the titan by small hotdog `by`: `base` HP times the zone's multiplier (MULTIPLIER), on the titan's record
    /// as `kind` (FACE_SHOT added on the face and the weak spot), struck from `from` along `dir` at `point`. It counts
    /// to `by`'s score; leg damage fills the stagger meter; a weak-spot hit is told; the last is the titan down.
    pub(crate) fn strike_titan(&mut self, by: u32, base: f32, zone: Zone, point: Vec3, dir: Vec3, kind: u8, from: Vec3) {
        let now = self.elapsed;
        let Some(id) = self.titan_id() else {
            return;
        };
        let Some(before) = self.player(id).filter(|p| p.alive).map(|p| p.hp) else {
            return;
        };
        let amount = base * zone.multiplier();
        let marked = if matches!(zone, Zone::Face | Zone::Weak) { crate::feast::FACE_SHOT } else { 0 };
        self.harm(id, amount, false, crate::feast::Damage::by(kind | marked, by, from, dir));
        let (hp, alive) = self.player(id).map_or((0., false), |p| (p.hp, p.alive));
        let took = (before - hp).max(0.);
        if took <= 0. {
            return;
        }
        let t = self.titan.as_mut().unwrap();
        (t.tally.hits[zone as usize], t.tally.damage[zone as usize]) = (t.tally.hits[zone as usize] + 1, t.tally.damage[zone as usize] + took);
        t.threat = (by, now);
        if t.mv.data().kind == Kind::Reel {
            t.tally.reeling += took;
            t.tally.weak_reeling += (zone == Zone::Weak) as u32;
        }
        if let Some(s) = t.small_mut(by) {
            s.recent += took;
            s.dealt += took;
            if zone == Zone::Weak {
                s.weak += 1;
            }
        }
        if zone == Zone::Weak {
            t.tell(Event { kind: Some(EventKind::WeakHit), time: now, by, to: id, amount: took, p: point.to_array(), zone: zone as u8, ..Default::default() });
        }
        let mut stagger = false;
        if zone == Zone::Legs && now >= t.rest && !matches!(t.mv.data().kind, Kind::Reel | Kind::Down) && alive {
            t.legs += took;
            if t.legs >= STAGGER_LEGS {
                t.legs = STAGGER_LEGS;
                if t.airborne { t.staggered = true } else { stagger = true }
            }
        }
        if !alive {
            t.tally.titan_killed_by = Some(zone);
        }
        self.score_small(by);
        if stagger {
            self.titan_begin(Move::Stagger);
        }
    }
    /// A small hotdog's score: a point an HP it has dealt the titan, WEAK_BONUS a weak-spot hit, 100 a kill.
    fn score_small(&mut self, id: u32) {
        let Some(s) = self.titan.as_ref().and_then(|t| t.small(id)).cloned() else {
            return;
        };
        if let Some(p) = self.player_mut(id) {
            p.score = s.dealt.floor() as u32 + s.weak * WEAK_BONUS + p.kills * 100;
        }
    }

    // ----- Deaths, reinforcements and the end.

    /// A hotdog has died in Titan (from `mourn`, its killer credited): the titan falls for good; a small hotdog is back
    /// in RESPAWN seconds if the pool has one left (taken now), and out otherwise.
    pub(crate) fn titan_mourn(&mut self, id: u32) {
        let now = self.elapsed;
        let Some(titan) = self.titan_id() else {
            return;
        };
        let killer = self.player(id).map_or(0, |p| p.last_damage.attacker);
        let kind = self.player(id).map_or(0, |p| crate::feast::plain_kind(p.last_damage.kind));
        if killer != 0 && killer != id && killer != titan {
            self.score_small(killer);
        }
        if id == titan {
            let chest = self.player(id).map_or(Vec3::ZERO, |p| Vec3::from_array(p.p) + Vec3::Y * TALL * 0.5);
            if let Some(p) = self.player_mut(id) {
                (p.deaths, p.respawn) = (p.deaths + 1, 0.);
            }
            let t = self.titan.as_mut().unwrap();
            (t.mv, t.started, t.len, t.phase, t.airborne) = (Move::Down, now, Move::Down.data().total(), 1, false);
            t.waves.clear();
            t.tell(Event { kind: Some(EventKind::TitanDown), time: now, by: killer, to: id, p: chest.to_array(), ..Default::default() });
            return;
        }
        let t = self.titan.as_mut().unwrap();
        if kind != crate::feast::TITAN {
            t.tally.other_deaths += 1;
        }
        let back = t.reserve > 0;
        if back {
            t.reserve -= 1;
        } else if let Some(s) = t.small_mut(id) {
            s.out = true;
        }
        if let Some(p) = self.player_mut(id) {
            (p.deaths, p.respawn, p.shield, p.crouch) = (p.deaths + 1, if back { RESPAWN } else { 0. }, 0., false);
        }
    }
    /// Where a small hotdog comes back: one of the map's spots furthest from the titan (drawn from the best few), free,
    /// facing it.
    pub(crate) fn titan_haven(&mut self, id: u32) -> (Vec3, f32) {
        self.survey();
        let titan = self.titan_id().and_then(|t| self.player(t)).map_or(Vec2::ZERO, |p| flat(p.p));
        let deaths = self.player(id).map_or(0, |p| p.deaths);
        let mut options: Vec<(f32, Vec3)> = self.spots.iter().copied().filter(|s| self.free(*s)).map(|s| ((Vec2::new(s.x, s.z) - titan).length(), s)).collect();
        if options.is_empty() {
            options = (1..self.level.map.starts.len().max(2)).map(|i| self.level.start(i).0).map(|s| ((Vec2::new(s.x, s.z) - titan).length(), s)).collect();
        }
        options.sort_by(|a, b| b.0.total_cmp(&a.0));
        options.truncate(4);
        let mut dice = crate::spawns::Dice::new(&[self.card_seed, self.round as u64, 0x7469_7461, id as u64, deaths as u64]);
        let at = options.get(dice.below(options.len())).map_or(self.level.start(1).0, |o| o.1);
        let to = titan - Vec2::new(at.x, at.z);
        (at, if to.length() > 1e-3 { (-to.x).atan2(-to.y) } else { 0. })
    }
    /// A small hotdog has come back from the pool: told.
    pub(crate) fn titan_back(&mut self, id: u32) {
        let now = self.elapsed;
        let at = self.player(id).map_or([0.; 3], |p| p.p);
        if let Some(t) = self.titan.as_mut() {
            let left = t.reserve as f32;
            t.tell(Event { kind: Some(EventKind::Reinforcement), time: now, to: id, amount: left, p: at, ..Default::default() });
        }
    }
    /// Titan's end (from `referee`): the small hotdogs win when the titan is down; the titan when every small hotdog is
    /// dead with the pool empty, or when the clock runs out. The line at the top says how, and who dealt the most.
    pub(crate) fn titan_whistle(&mut self) {
        let now = self.elapsed;
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let id = t.id;
        let titan_down = !self.player(id).is_some_and(|p| p.alive);
        let wiped = t.reserve == 0 && self.players.iter().filter(|p| p.id != id && !p.benched).all(|p| !p.alive && p.respawn <= 0.);
        let timed = self.ends > 0. && self.elapsed >= self.ends;
        let over = if titan_down {
            OVER_SMALLS
        } else if wiped || timed {
            OVER_TITAN
        } else {
            return;
        };
        self.stage = crate::feast::Stage::RoundOver;
        self.over_at = now;
        for q in &mut self.projectiles {
            q.hostile = false;
        }
        let t = self.titan.as_mut().unwrap();
        t.over = over;
        t.waves.clear();
        t.tell(Event { kind: Some(EventKind::Over), time: now, by: over as u32, ..Default::default() });
        if over == OVER_TITAN {
            (t.mv, t.started, t.len, t.phase, t.airborne) = (Move::Roar, now, Move::Roar.data().total(), 1, false);
        }
        let best = t.smalls.iter().max_by(|a, b| a.dealt.total_cmp(&b.dealt).then(b.id.cmp(&a.id))).map(|s| (s.id, s.dealt));
        let name = |feast: &Feast, id: u32| feast.player(id).map_or(String::new(), |p| p.name.to_uppercase());
        if over == OVER_SMALLS {
            self.winner = best.map(|b| b.0);
            let mvp = best.map_or(String::new(), |(b, dealt)| format!(" {} DEALT THE MOST: {}", name(self, b), dealt.round() as u32));
            self.alert = format!("THE TITAN FALLS.{mvp}");
        } else {
            self.winner = Some(id);
            let who = name(self, id);
            self.alert = if titan_down { String::new() } else if timed { format!("TIME. {who} STILL STANDS: THE TITAN WINS") } else { format!("NO HOTDOGS LEFT: {who} WINS") };
        }
    }
}
/// Smoothstep: 0 at 0, 1 at 1, flat at both.
fn smooth(t: f32) -> f32 {
    t * t * (3. - 2. * t)
}

// ----- What the client is told, and the small helpers.

impl Feast {
    /// The titan's hotdog id, in a round of Titan.
    pub fn titan_id(&self) -> Option<u32> {
        self.titan.as_ref().map(|t| t.id).filter(|id| *id != 0)
    }
    /// The titan the next round's is chosen by turns from: this round's if it has been played, else the one before.
    pub(crate) fn titan_last(&self) -> Option<u32> {
        match self.titan.as_ref() {
            Some(t) if t.played && t.id != 0 => Some(t.id),
            Some(t) => t.previous,
            None => self.titan_previous,
        }
    }
    /// Whether hotdog `id` is the titan.
    pub fn is_titan(&self, id: u32) -> bool {
        id != 0 && self.titan_id() == Some(id)
    }
    /// Whether the titan's weak spot is easy now: on its knees, or its nape turned up with a small hotdog behind it.
    pub fn titan_exposed(&self) -> bool {
        let Some(t) = self.titan.as_ref() else {
            return false;
        };
        let Some(p) = self.player(t.id).filter(|p| p.alive) else {
            return false;
        };
        if t.mv.data().kind == Kind::Reel {
            return true;
        }
        let Some(shape) = self.titan_shape() else {
            return false;
        };
        let (at, ahead) = (flat(p.p), facing(p.yaw));
        shape.nape.y > EXPOSED && self.players.iter().any(|q| q.alive && q.id != t.id && (flat(q.p) - at).normalize_or_zero().dot(ahead) < -0.3)
    }
    /// The titan as the client is told it (None outside Titan): see `View`.
    pub fn titan_view(&self) -> Option<View> {
        let t = self.titan.as_ref()?;
        let p = self.player(t.id);
        let now = self.elapsed;
        let smalls = t
            .smalls
            .iter()
            .map(|s| {
                let q = self.player(s.id);
                SmallView { id: s.id, dealt: s.dealt, weak: s.weak, respawn: q.filter(|q| !q.alive && !s.out).map_or(0., |q| q.respawn), out: s.out }
            })
            .collect();
        Some(View {
            elapsed: now,
            titan: t.id,
            hp: p.map_or(0., |p| p.hp),
            max: t.max,
            mv: t.mv as u8,
            started: t.started,
            len: t.len,
            crouch: p.is_some_and(|p| p.crouch),
            ready: t.ready,
            stagger: t.stagger(),
            exposed: self.titan_exposed(),
            pool: t.pool,
            reserve: t.reserve,
            ends: self.ends,
            over: t.over,
            smalls,
            events: t.events.clone(),
            stride: t.stride,
            drive: t.drive,
        })
    }
    /// Whether hotdog `id` stands in a den (under an overhead a small hotdog fits beneath): see `Footing::den`.
    pub fn in_den(&self, id: u32) -> bool {
        let (Some(t), Some(p)) = (self.titan.as_ref(), self.player(id)) else {
            return false;
        };
        p.p[1] < FLOOR + 0.06 && t.footing.den(p.p[0], p.p[2])
    }
    /// A number from 0 to 1 for the bots of Titan, from the game's seed, the round and `key`.
    fn titan_draw(&mut self, key: u64) -> f32 {
        let (seed, round) = (self.card_seed, self.round as u64);
        let Some(t) = self.titan.as_mut() else {
            return 0.5;
        };
        t.draws += 1;
        let roll = crate::spawns::Dice::new(&[seed, round, 0x7469_626f_7473, key, t.draws]).roll();
        (roll >> 40) as f32 / (1u64 << 24) as f32
    }
}

// ----- The bots.

/// The titan bot's footing as a route-finding grid (lib.rs `Navigation`), built once a round.
#[derive(Clone, Default)]
pub struct Routes(pub Option<crate::Navigation>);
impl std::fmt::Debug for Routes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "Routes(built)" } else { "Routes(none)" })
    }
}
/// The gap a small hotdog keeps from the titan (beyond its swat), and the gap the one it hunts keeps.
const RING: f32 = 1.55;
const KITE: f32 = 1.7;
/// What a small bot keeps of aim.rs's pause after a shot, aiming at the titan (see `small_bot`).
pub const RECOVER: f32 = 0.25;
/// A small bot jumps a wave this long before the front reaches it.
const JUMP_LEAD: f32 = 0.2;

impl Feast {
    /// A bot's step in Titan: the titan's, or a small hotdog's.
    pub(crate) fn titan_bot(&mut self, id: u32, dt: f32) {
        if self.is_titan(id) { self.titan_brain(dt) } else { self.small_bot(id, dt) }
    }
    /// The bot titan (see the interface): it hunts a small hotdog (the nearest it can get at, the one hurting its back
    /// first, another once one has kited it long enough), walks the titan's own routes to it, turns to it and looks at
    /// it, crouches for the floor and the dens, and mixes its moves: a charge down a clear lane, a smash into a group, a
    /// swat at the height its target is at, a stomp at its feet, a jump when it is surrounded.
    fn titan_brain(&mut self, dt: f32) {
        let now = self.elapsed;
        let w = wits(self.rules.skill);
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let id = t.id;
        let Some(p) = self.player(id).filter(|p| p.alive).cloned() else {
            return;
        };
        if t.over != OVER_NONE {
            return;
        }
        let (at, ahead) = (flat(p.p), facing(p.yaw));
        let smalls = self.smalls_alive();
        if smalls.is_empty() {
            self.titan_drive(Vec2::ZERO, p.yaw, dt);
            return;
        }
        // The target.
        let current = t.brain.target;
        let gone = !smalls.iter().any(|q| q.id == current);
        if gone || now >= t.brain.choose {
            let (threat, when) = t.threat;
            let tired = !gone && now - t.brain.since > 9. && now - t.brain.landed > 9.;
            let shun: Vec<u32> = t.brain.shun.iter().filter(|(_, until)| *until > now).map(|(id, _)| *id).chain(tired.then_some(current)).collect();
            let footing = &t.footing;
            let pick = smalls
                .iter()
                .map(|q| {
                    let rel = flat(q.p) - at;
                    let mut cost = rel.length();
                    if footing.den(q.p[0], q.p[2]) {
                        cost += 0.8;
                    }
                    if q.p[1] > FLOOR + 0.1 {
                        cost += 0.4;
                    }
                    if q.id == current && !tired {
                        cost -= 0.4;
                    }
                    if q.id == threat && now - when < 2. && rel.normalize_or_zero().dot(ahead) < -0.2 {
                        cost -= 1.2;
                    }
                    if shun.contains(&q.id) {
                        cost += 5.;
                    }
                    if w.cunning {
                        // (Whoever hurts it most, and whoever a blow or two would finish.)
                        let recent = t.smalls.iter().find(|s| s.id == q.id).map_or(0., |s| s.recent);
                        cost -= (recent / 150.).min(1.5) + (100. - q.hp).max(0.) / 100. * 0.8;
                    }
                    (cost, q.id)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
                .map(|(_, id)| id)
                .unwrap_or(0);
            let t = self.titan.as_mut().unwrap();
            if tired {
                t.brain.shun.push((current, now + 6.));
            }
            t.brain.shun.retain(|(_, until)| *until > now);
            if pick != t.brain.target {
                (t.brain.target, t.brain.since, t.brain.landed) = (pick, now, now);
                t.brain.route.clear();
            }
            t.brain.choose = now + 0.5;
        }
        let t = self.titan.as_ref().unwrap();
        let Some(q) = smalls.iter().find(|q| q.id == t.brain.target).cloned() else {
            return;
        };
        let rel = flat(q.p) - at;
        let far = rel.length();
        let lead = flat(q.p) + Vec2::new(q.v[0], q.v[2]) * 0.35;
        let to = lead - at;
        let want = if to.length() > 1e-3 { (-to.x).atan2(-to.y) } else { p.yaw };
        // It looks at its target (bowing its head to the floor).
        let shape = self.titan_shape().unwrap_or_else(|| Shape::of(&p, t.mv));
        let pitch = ((q.p[1] + 0.08) - shape.head.y).atan2(far.max(0.05)).clamp(-1.5, 1.5);
        self.look(id, pitch);
        let t = self.titan.as_ref().unwrap();
        let floor = |q: &Player| q.p[1] < FLOOR + 0.06;
        let elevated = q.p[1] > FLOOR + 0.1;
        let den = t.footing.den(q.p[0], q.p[2]);
        // Crouching: for a target on the floor close by, or in a den; standing for the rest.
        if t.free() {
            let flanked = w.cunning && smalls.iter().any(|s| {
                let rel = flat(s.p) - at;
                rel.length() < 3.5 && rel.normalize_or_zero().dot(ahead) < -0.3
            });
            let wobbly = w.cunning && t.stagger() > 0.5;
            // (Any small hotdog on the floor within a low swat's reach in front of it is a reason to crouch for one.)
            let greedy = w.sharp;
            let low_one = greedy && smalls.iter().any(|s| {
                let rel = flat(s.p) - at;
                floor(s) && rel.length() < Move::SwatLow.data().reach + SMALL_RADIUS && rel.normalize_or_zero().dot(ahead) > Move::SwatLow.data().arc.cos()
            });
            // (Chasing, it stays on its feet, which are quicker than a small hotdog's; it crouches as it swats or kicks.)
            let chase = w.sharp;
            let crouch = (den && far < 1.8) || (!chase && ((floor(&q) && far < 1.25 && !elevated) || low_one)) || flanked || wobbly;
            if let Some(me) = self.player_mut(id) {
                me.crouch = crouch;
            }
        }
        let p = self.player(id).cloned().unwrap();
        let t = self.titan.as_ref().unwrap();
        let angle = if far > 1e-4 { ahead.dot(rel / far).clamp(-1., 1.).acos() } else { 0. };
        // An attack, once it has reacted to its target and paused since the last.
        if t.free() && now >= t.brain.think && now - t.brain.since >= w.react {
            let ready = |m: Move| m.ready_slot().is_none_or(|slot| now >= t.ready[slot]);
            // (A cunning titan reads where they are going: each blow is judged where they will be as it lands, their
            // walk carried on over its wind-up.)
            let lead = |mv: Move| if w.cunning { timing(mv, 0.)[0] } else { 0. };
            let soon = |s: &Player, mv: Move| flat(s.p) + Vec2::new(s.v[0], s.v[2]) * lead(mv);
            let near_by = |c: Vec2, r: f32, ground: bool, mv: Move| smalls.iter().filter(|s| (soon(s, mv) - c).length() < r && (!ground || floor(s))).count();
            let near = |c: Vec2, r: f32, ground: bool| near_by(c, r, ground, Move::Idle);
            let smash_at = at + ahead * Move::Smash.data().ahead;
            let foot = at + ahead * Move::Stomp.data().ahead;
            // (A lane: a small hotdog in the corridor ahead, and the floor clear to it and past it.)
            let in_lane = smalls
                .iter()
                .filter(|s| {
                    let rel = flat(s.p) - at;
                    (0.8..3.6).contains(&rel.dot(ahead)) && ahead.perp_dot(rel).abs() < RADIUS + 0.15 && floor(s)
                })
                .map(|s| (flat(s.p) - at).dot(ahead))
                .fold(None, |best: Option<f32>, d| Some(best.map_or(d, |b| b.min(d))));
            let lane = {
                let far = in_lane.unwrap_or(far);
                let run = (far + 0.6).min(CHARGE_SPEED * CHARGE_TIME);
                (1..=((run / 0.2) as usize)).all(|k| {
                    let c = at + ahead * (k as f32 * 0.2);
                    t.footing.clear(c.x, c.y)
                })
            };
            let swat_low = Move::SwatLow.data();
            let swat_high = Move::SwatHigh.data();
            // (A cunning titan walking at a group jumps into it: it lands where its walk carries it.)
            let lands = at + t.vel * Move::Jump.data().active;
            let lunge = w.cunning && t.vel.dot(ahead) > 0.5 && near(lands, Move::Jump.data().reach * 0.8, true) >= 2;
            // (Whoever is in a blow's way, not only its target: a sweep, a kick or a foot for any small hotdog there.)
            let greedy = w.sharp;
            let swept_n = |mv: Move, band_low: bool| {
                let d = mv.data();
                smalls
                    .iter()
                    .filter(|s| {
                        let rel = soon(s, mv) - at;
                        let far = rel.length();
                        let low = floor(s) || t.footing.den(s.p[0], s.p[2]);
                        (s.id == q.id || greedy) && far < d.reach * 0.95 + SMALL_RADIUS && (far < 1e-4 || ahead.dot(rel / far) > (d.arc * 0.85).cos()) && if band_low { low } else { s.p[1] > FLOOR + 0.1 && !t.footing.den(s.p[0], s.p[2]) }
                    })
                    .count()
            };
            let swept = |d: &MoveData, band_low: bool| swept_n(if band_low { Move::SwatLow } else { Move::SwatHigh }, band_low) > 0 && (d.reach > 0.);
            let stomped_n = smalls.iter().filter(|s| (s.id == q.id || greedy) && floor(s) && (soon(s, Move::Stomp) - foot).length() < Move::Stomp.data().reach + SMALL_RADIUS).count();
            let stomped = stomped_n > 0;
            let kicked_n = smalls
                .iter()
                .filter(|s| {
                    let rel = soon(s, Move::Kick) - at;
                    (s.id == q.id || greedy) && floor(s) && rel.length() < 0.8 && rel.normalize_or_zero().dot(ahead) > 0.5f32.cos()
                })
                .count();
            let kicked = kicked_n > 0;
            let smash_n = near_by(smash_at, 1.1, true, Move::Smash);
            let smashed = (floor(&q) && (soon(&q, Move::Smash) - smash_at).length() < 0.9 && !den) || (greedy && near_by(smash_at, 0.9, true, Move::Smash) >= 1);
            // A cunning titan strikes with whichever ready blow catches the most of them (the order below breaking a tie).
            let best = w.cunning.then(|| {
                let can = |mv: Move, ok: bool, n: usize| (ready(mv) && ok && n > 0).then_some((n, mv));
                // (A jump into a group round it, or the lunge into one ahead.)
                let round = near_by(at, 0.95, true, Move::Jump);
                let jumped = if round >= 2 || lunge { near_by(if lunge { lands } else { at }, Move::Jump.data().reach * 0.8, true, Move::Jump).max(round) } else { 0 };
                [
                    can(Move::Jump, true, jumped),
                    can(Move::Smash, angle < 0.6, smash_n.max(smashed as usize)),
                    can(Move::Stomp, !p.crouch, stomped_n),
                    can(Move::SwatLow, true, swept_n(Move::SwatLow, true)),
                    can(Move::Kick, true, kicked_n),
                    can(Move::SwatHigh, !p.crouch, swept_n(Move::SwatHigh, false)),
                ]
                .into_iter()
                .flatten()
                .rev()
                .max_by_key(|(n, _)| *n)
                .map(|(_, mv)| mv)
            });
            let mv = if let Some(Some(mv)) = best {
                Some(mv)
            } else if ready(Move::Jump) && (near(at, 0.95, true) >= 2 || lunge) {
                Some(Move::Jump)
            } else if ready(Move::Smash) && angle < 0.6 && (near(smash_at, 1.1, true) >= 2 || smashed) {
                Some(Move::Smash)
            } else if ready(Move::Stomp) && !p.crouch && stomped {
                Some(Move::Stomp)
            } else if ready(Move::SwatLow) && (p.crouch || w.sharp) && swept(swat_low, true) {
                Some(Move::SwatLow)
            } else if ready(Move::Kick) && (p.crouch || w.sharp) && kicked {
                Some(Move::Kick)
            } else if ready(Move::SwatHigh) && !p.crouch && swept(swat_high, false) {
                Some(Move::SwatHigh)
            } else if ready(Move::Charge) && !p.crouch && lane && ((in_lane.is_some() && !den) || ((1.2..3.8).contains(&far) && angle < 0.12 && !den)) && (!w.cunning || {
                // (Not a charge wasted: its target still in the lane when it gets there, its walk carried on.)
                let arrive = timing(Move::Charge, 0.)[0] + far / CHARGE_SPEED;
                let there = flat(q.p) + Vec2::new(q.v[0], q.v[2]) * arrive - at;
                ahead.perp_dot(there).abs() < RADIUS + 0.2 && there.dot(ahead) > 0.5
            }) {
                Some(Move::Charge)
            } else {
                None
            };
            if let Some(mv) = mv {
                let pause = w.pause * (0.6 + 0.8 * self.titan_draw(id as u64));
                if self.titan_draw(id as u64 ^ 0x6e) < w.nerve {
                    // (Down into its crouch for a low blow, up out of it for a high one: it is free, so it may.)
                    let low = matches!(mv, Move::SwatLow | Move::Kick);
                    if low || matches!(mv, Move::SwatHigh | Move::Stomp | Move::Charge) {
                        if let Some(me) = self.player_mut(id) {
                            me.crouch = low;
                        }
                    }
                    self.titan_press(mv);
                }
                self.titan.as_mut().unwrap().brain.think = now + pause;
            }
        }
        let t = self.titan.as_ref().unwrap();
        // A charge that has passed its target is ended.
        if t.mv == Move::Charge && t.phase == 1 && (rel.dot(ahead) < -0.2 || far > 4.5) {
            self.titan_press(Move::Charge);
        }
        // Walking: by its own routes to where its target is going, up to striking distance. A cunning titan with nobody
        // close makes its stand in a lair instead (its back to the wall: nobody gets behind it), facing them.
        let close = if p.crouch { 0.6 } else { 0.5 };
        let mut way = Vec2::ZERO;
        let lair = if w.cunning && far > 1.4 { self.titan_lair(at, &smalls) } else { None };
        let (goal_at, stop) = match lair {
            Some((spot, _)) => (spot, 0.15),
            None => (lead, close),
        };
        let t = self.titan.as_ref().unwrap();
        let far_goal = (goal_at - at).length();
        if t.free() && far_goal > stop {
            if t.brain.route.is_empty() || now - t.brain.routed > 0.6 || t.brain.route.last().is_some_and(|end| (*end - goal_at).length() > 0.3) {
                let goal = t.footing.nearest_clear(goal_at);
                let start = t.footing.nearest_clear(at);
                let mut nav = self.titan_routes();
                let mut route = vec![];
                let found = nav.as_mut().is_some_and(|nav| nav.path(start, goal, &mut route));
                let t = self.titan.as_mut().unwrap();
                t.brain.route = if found { route } else { vec![goal] };
                t.brain.routed = now;
                t.routes = Routes(nav);
            }
            let t = self.titan.as_mut().unwrap();
            while t.brain.route.len() > 1 && (t.brain.route[0] - at).length() < 0.12 {
                t.brain.route.remove(0);
            }
            if let Some(next) = t.brain.route.first() {
                way = (*next - at).normalize_or_zero();
            }
        }
        // (Facing where it walks while far off, its target once near.)
        let mut face = if far_goal > 1.6 && way != Vec2::ZERO { (-way.x).atan2(-way.y) } else { want };
        // A cunning titan with nobody to strike yet faces whoever hurts it (weighted by what they did lately): its nape
        // away from them, and they must walk round it again to get behind it.
        let t = self.titan.as_ref().unwrap();
        if w.cunning && far > 1.3 {
            let (mut sum, mut weight) = (Vec2::ZERO, 0.);
            for q in &smalls {
                let rel = flat(q.p) - at;
                if rel.length() < 4. {
                    let k = 1. + t.smalls.iter().find(|s| s.id == q.id).map_or(0., |s| s.recent) / 50.;
                    (sum, weight) = (sum + rel.normalize_or_zero() * k, weight + k);
                }
            }
            if weight > 0. && sum.length() > 0.2 * weight {
                face = (-sum.x).atan2(-sum.y);
            }
        }
        self.titan_drive(way, face, dt);
    }
    /// A cunning titan's lair now: of the spots with its back to a wall (`lairs`), the one whose way out faces the small
    /// hotdogs best, near enough to reach (None when there is none).
    fn titan_lair(&mut self, at: Vec2, smalls: &[Player]) -> Option<(Vec2, f32)> {
        if self.titan.as_ref()?.lairs.is_empty() {
            let footing = self.titan.as_ref()?.footing.clone();
            let b = footing.bounds;
            let mut lairs = vec![];
            let step = 0.2;
            let (mut x, mut z) = (b[0], b[1]);
            while x <= b[2] {
                while z <= b[3] {
                    if footing.clear(x, z) {
                        // (Its back: the ways round it, a body's width out, that are walled.)
                        let mut walled = Vec2::ZERO;
                        let mut count = 0;
                        for k in 0..12 {
                            let a = k as f32 / 12. * std::f32::consts::TAU;
                            let d = Vec2::new(a.cos(), a.sin());
                            let probe = Vec2::new(x, z) + d * 0.3;
                            if !footing.clear(probe.x, probe.y) {
                                walled += d;
                                count += 1;
                            }
                        }
                        if (3..=6).contains(&count) {
                            let out = -walled.normalize_or_zero();
                            lairs.push((Vec2::new(x, z), (-out.x).atan2(-out.y)));
                        }
                    }
                    z += step;
                }
                (x, z) = (x + step, b[1]);
            }
            self.titan.as_mut()?.lairs = lairs;
        }
        let t = self.titan.as_ref()?;
        let middle = smalls.iter().map(|q| flat(q.p)).sum::<Vec2>() / smalls.len().max(1) as f32;
        t.lairs
            .iter()
            .map(|(spot, yaw)| {
                let toward = (middle - *spot).normalize_or_zero().dot(facing(*yaw));
                ((*spot - at).length() - toward * 2., (*spot, *yaw))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, lair)| lair)
    }
    /// The titan's route grid, built the first time it is asked for (taken out of the titan's state: put it back).
    fn titan_routes(&mut self) -> Option<crate::Navigation> {
        let t = self.titan.as_mut()?;
        if let Some(nav) = t.routes.0.take() {
            return Some(nav);
        }
        let b = t.footing.bounds;
        let obstacles: Vec<crate::Obstacle> = t.footing.rects.iter().map(|r| crate::Obstacle { min: Vec2::new(r[0], r[1]), max: Vec2::new(r[2], r[3]) }).collect();
        let (min, max) = (Vec2::new(b[0], b[1]), Vec2::new(b[2], b[3]));
        if max.x <= min.x || max.y <= min.y {
            return None;
        }
        Some(crate::Navigation::new(min, max, 0.1, 0.02, &obstacles))
    }
    /// A small bot (see the interface): it reloads and fetches rounds, reads the titan's tells (jumps the wave as its
    /// front comes, gets out of a charge's lane, a swat's reach, a stomp's foot), keeps its place on a ring round the
    /// titan with the others (behind it if it can, the one it hunts kiting in front, into a den when it closes), and
    /// shoots: the weak spot from behind or while it kneels, the legs for the stagger, else the body.
    fn small_bot(&mut self, id: u32, dt: f32) {
        let now = self.elapsed;
        let w = wits(self.rules.skill);
        let Some(t) = self.titan.as_ref() else {
            return;
        };
        let titan_id = t.id;
        let (Some(titan), Some(p)) = (self.player(titan_id).cloned(), self.player(id).cloned()) else {
            return;
        };
        if !titan.alive || !p.alive || t.over != OVER_NONE {
            return;
        }
        self.top_up(id, 0.);
        let t = self.titan.as_ref().unwrap();
        let (mv, started, phase, airborne, carry) = (t.mv, t.started, t.phase, t.airborne, t.carry);
        // (Hunted, as a person would tell: the titan faces it, near, and no other small hotdog is nearer in front of it.)
        let hunted = {
            let (tat, ahead) = (flat(titan.p), facing(titan.yaw));
            let ahead_of = |q: &Player| {
                let rel = flat(q.p) - tat;
                (rel.length() < 3.2 && rel.normalize_or_zero().dot(ahead) > 0.8).then_some(rel.length())
            };
            ahead_of(&p).is_some_and(|mine| !self.smalls_alive().iter().any(|q| q.id != id && ahead_of(q).is_some_and(|theirs| theirs < mine)))
        };
        let d = mv.data();
        let (me, tat, ahead) = (flat(p.p), flat(titan.p), facing(titan.yaw));
        let rel = me - tat;
        let far = rel.length();
        let Some(k) = t.minds.iter().position(|m| m.id == id) else {
            return;
        };
        // 1. The tell: each of the titan's moves is judged once, after the bot's reaction time.
        let tt = now - started;
        if d.attacks() && t.minds[k].judged != started && tt >= w.react {
            let right = self.titan_draw(id as u64 ^ (started.to_bits() as u64) << 8) < w.judge;
            let off = (self.titan_draw(id as u64 ^ 0x6a75) * 2. - 1.) * w.timing;
            let m = &mut self.titan.as_mut().unwrap().minds[k];
            (m.judged, m.jump, m.jump_at) = (started, right, off);
            if right {
                // Where the blow will be, and which way out of it.
                let (out, until) = match mv {
                    Move::Charge => {
                        let lateral = ahead.perp_dot(rel);
                        let along = rel.dot(ahead);
                        if along > 0. && along < 5. && lateral.abs() < RADIUS + 0.35 {
                            let side = if lateral >= 0. { 1. } else { -1. };
                            (Some(ahead.perp() * side * 0.75), started + d.startup + 1.2)
                        } else {
                            (None, 0.)
                        }
                    }
                    Move::SwatHigh | Move::SwatLow | Move::Kick => {
                        let low = p.p[1] - titan.p[1];
                        let in_band = low + SMALL_TALL >= d.band[0] && low <= d.band[1];
                        let in_arc = far < 1e-4 || ahead.dot(rel / far) >= (d.arc + 0.3).min(std::f32::consts::PI).cos();
                        if far < d.reach + 0.3 && in_band && in_arc { (Some(rel.normalize_or(-ahead) * 0.75), started + d.startup + d.active) } else { (None, 0.) }
                    }
                    Move::Stomp => {
                        let foot = tat + ahead * d.ahead;
                        if (me - foot).length() < 0.5 { (Some((me - foot).normalize_or(-ahead) * 0.6), started + d.startup + d.active) } else { (None, 0.) }
                    }
                    Move::Smash => {
                        let centre = tat + ahead * d.ahead;
                        if (me - centre).length() < DIRECT + 0.3 { (Some((me - centre).normalize_or(-ahead) * 0.8), started + d.startup + 0.1) } else { (None, 0.) }
                    }
                    _ => (None, 0.),
                };
                if let Some(out) = out {
                    let m = &mut self.titan.as_mut().unwrap().minds[k];
                    (m.dodge_to, m.dodge_until) = (me + out, until);
                }
            }
        }
        // A wave coming: jumped as its front reaches the bot.
        let m = self.titan.as_ref().unwrap().minds[k].clone();
        if m.jump && m.judged == started && p.grounded && !p.launched && matches!(mv, Move::Smash | Move::Jump) {
            let (centre, impact) = match mv {
                Move::Smash => (tat + ahead * d.ahead, started + d.startup),
                _ if airborne => {
                    // (When it comes down: y + vy t - g t²/2 = FLOOR.)
                    let (y, vy, g) = (titan.p[1] - FLOOR, self.titan.as_ref().unwrap().vy, actor::GRAVITY as f32);
                    let left = (vy + (vy * vy + 2. * g * y).max(0.).sqrt()) / g;
                    (tat + carry * left, now + left)
                }
                _ => (tat, started + d.startup + 2. * JUMP_SPEED / actor::GRAVITY as f32),
            };
            let gap = (me - centre).length() - SMALL_RADIUS;
            let front = impact + (gap - DIRECT).max(0.) / d.wave.max(0.1);
            // (It jumps when it judges the front is about to come: off by up to its skill's timing, early or late.)
            let mean = front - JUMP_LEAD + m.jump_at;
            if gap < d.reach && gap > DIRECT && now >= mean && now < mean + 0.25 {
                let stim = p.stim > 0.;
                self.jump(id, self.tune.leap(stim));
                if let Some(me) = self.player_mut(id) {
                    me.crouch = true;
                }
                self.titan.as_mut().unwrap().minds[k].jump = false;
            }
        }
        // Down again from a jump: standing.
        if p.grounded && p.crouch && !self.titan.as_ref().unwrap().minds[k].jump {
            if let Some(me) = self.player_mut(id) {
                me.crouch = false;
            }
        }
        // 2. Shooting: the mark, the gun, a clear shot.
        let shape = self.titan_shape().unwrap_or_else(|| Shape::of(&titan, mv));
        let o = crate::feast::muzzle(&p);
        let reel = mv.data().kind == Kind::Reel;
        let behind = far > 1e-4 && rel.dot(ahead) / far < -0.3;
        let weak = shape.weak_point();
        let sees_nape = (o - weak).normalize_or_zero().dot(shape.nape) > (shape.weak_arc * 0.9).cos();
        let stagger = self.titan.as_ref().unwrap().stagger();
        let legs_role = id % 3 == 0;
        let mark = if (behind || reel) && sees_nape {
            weak
        } else if (legs_role || stagger > 0.35) && !reel {
            shape.knee(m.leg)
        } else {
            (shape.torso.a + shape.torso.b) * 0.5
        };
        let span = o.distance(mark);
        let gun = self.arm(&p, span, span > 0.6);
        let mut shooting = false;
        if gun > 0 {
            self.select(id, gun);
            let mark = if gun == 4 { Vec3::new(mark.x, FLOOR + 0.02, mark.z).lerp(Vec3::new(tat.x, FLOOR + 0.02, tat.y), 0.5) } else { mark };
            let way = (mark - o).normalize_or(Vec3::NEG_Z);
            let span = o.distance(mark);
            let clear = self.cover(o, way) + 0.02 >= span && span <= self.tune.range[gun] * 0.95;
            if clear {
                let v = Vec3::new(titan.v[0], 0., titan.v[2]);
                let (dir, ready) = self.draw_bead(id, titan_id, mark, v, gun, None);
                if self.loose(id, dir, ready, gun) {
                    // (A target this size needs no bringing the aim back onto: it fires again nearly as soon as its gun
                    // allows, whatever its skill, keeping RECOVER of the pause aim.rs gives it after a shot.)
                    let (now, delay) = (self.elapsed, self.tune.delay[gun]);
                    let keep = knob("recover").unwrap_or(RECOVER);
                    if let Some(me) = self.player_mut(id) {
                        me.sight.hold = now + delay + (me.sight.hold - now - delay).max(0.) * keep;
                    }
                }
                shooting = true;
            }
        }
        // 3. Moving: out of a blow's way, to a pickup when low, else to its place on the ring.
        let m = self.titan.as_ref().unwrap().minds[k].clone();
        if now < m.dodge_until {
            self.small_go(id, m.dodge_to, shooting, dt);
            return;
        }
        let low = !crate::feast::Feast::low(&p) || far < 1.3;
        if !low && self.errand(id, 3., dt) {
            return;
        }
        if gun == 0 && self.errand(id, 6., dt) {
            return;
        }
        let living: Vec<u32> = self.smalls_alive().iter().map(|q| q.id).collect();
        let rank = living.iter().position(|q| *q == id).unwrap_or(0);
        let n = living.len().max(1);
        let perch = if rank == 0 && !hunted { self.titan_perch(tat, ahead) } else { None };
        let goal = if let Some(perch) = perch {
            perch
        } else if hunted {
            // Kiting: back off to KITE in front of it, sideways; into a den when it closes in.
            let den = self.titan.as_ref().unwrap().footing.dens.iter().map(|(r, _)| Vec2::new((r[0] + r[2]) / 2., (r[1] + r[3]) / 2.)).min_by(|a, b| (*a - me).length().total_cmp(&(*b - me).length()));
            match den.filter(|c| far < 1.25 && (*c - me).length() < 2.) {
                Some(c) => c,
                None => {
                    let side = if (now * 0.25 + id as f32).sin() > 0. { 1. } else { -1. };
                    tat + rel.normalize_or(ahead) * KITE + rel.normalize_or(ahead).perp() * side * 0.5
                }
            }
        } else {
            // Round the back: spread over 2.4 radians either side of directly behind it.
            let spread = if n > 1 { (rank as f32 / (n - 1) as f32 - 0.5) * 2.4 } else { 0. };
            let back = -ahead;
            let radius = RING + 0.1 * (rank % 3) as f32;
            let slot = |turn: f32| {
                let (sin, cos) = (spread + turn).sin_cos();
                tat + Vec2::new(back.x * cos - back.y * sin, back.x * sin + back.y * cos) * radius
            };
            // (A slot in a wall is no place: the nearest clear one round the ring, that way first.)
            let clear = |g: Vec2| self.level.clear(g.x as f64, g.y as f64);
            let side = if spread >= 0. { 1. } else { -1. };
            (0..12).flat_map(|k| [k as f32 * 0.3 * side, -(k as f32) * 0.3 * side]).map(slot).find(|g| clear(*g)).unwrap_or(slot(0.))
        };
        let goal = self.reachable(me, goal);
        if (goal - me).length() > 0.15 {
            self.small_go(id, goal, shooting, dt);
        } else if shooting {
            // (Standing still to shoot.)
        }
        let _ = (phase, dt);
    }
    /// Somewhere up off the floor for a small bot to shoot from (a deck, up its ramp), near enough the titan for its
    /// guns but out of its reach, the furthest round its back first: None where there is none.
    fn titan_perch(&self, titan: Vec2, ahead: Vec2) -> Option<Vec2> {
        self.level
            .map
            .decks
            .iter()
            .filter(|d| d.height >= 0.15)
            .map(|d| Vec2::new(((d.rect[0] + d.rect[2]) / 2.) as f32, ((d.rect[1] + d.rect[3]) / 2.) as f32))
            .filter(|c| (1.3..3.2).contains(&(*c - titan).length()))
            .max_by(|a, b| (-(*a - titan).normalize().dot(ahead)).total_cmp(&-(*b - titan).normalize().dot(ahead)))
    }
    /// A small bot walks to `goal` (by the level's routes), facing its shot if it is shooting.
    fn small_go(&mut self, id: u32, goal: Vec2, shooting: bool, dt: f32) {
        let (yaw, pitch) = self.player(id).map_or((0., 0.), |p| (p.yaw, p.pitch));
        let y = self.level.ground(goal.x as f64, goal.y as f64) as f32;
        self.advance_actor(id, Vec3::new(goal.x, y, goal.y), dt);
        if shooting {
            if let Some(p) = self.player_mut(id) {
                (p.yaw, p.pitch) = (yaw, pitch);
            }
        }
    }
    /// The nearest point to `goal` on the way from `from` that the level's grid has clear (or `goal`).
    fn reachable(&self, from: Vec2, goal: Vec2) -> Vec2 {
        let clear = |p: Vec2| self.level.clear(p.x as f64, p.y as f64);
        if clear(goal) {
            return goal;
        }
        for k in 1..=10 {
            let p = goal.lerp(from, k as f32 / 10.);
            if clear(p) {
                return p;
            }
        }
        from
    }
}

#[cfg(test)]
#[path = "titan_tests.rs"]
mod tests;

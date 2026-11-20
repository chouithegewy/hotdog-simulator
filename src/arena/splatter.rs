// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The condiments as they are seen: rounds in flight, the splash where one lands, the stains it leaves, the smears
//! on a hotdog it hits, the hot sauce thrower's flame and the relish bazooka's blast. All of it is drawn by the
//! client from what the simulation (or the server) says of rounds and splats; nothing here goes back into the
//! simulation.
//!
//! * **Colour.** Each sauce has a range of tones ([`Sauce::tones`]): dark and saturated where it lies thick,
//!   its own colour in the body, lighter where it is spread thin, and the light catches it in bright wet
//!   highlights (a clear coat on the drops, a normal map on the stains). Mustard and ketchup are the teams'
//!   colours, so they glow a little of their own and stay saturated in shadow.
//! * **Streams.** Ketchup and mayonnaise are a rope of drops stretched along the path so that they run
//!   together, darker at the head and thinner and lighter behind, that wobbles as it goes and sheds drips. The
//!   mustard pistol's round is a squirt ([`jet`]): a full, glossy head drawing out a tail that breaks up behind
//!   into beads and satellite drops, which droop and wobble and peel off as it flies.
//! * **Where a round lands.** [`classify`] works out what it hit from the level's own ground and cover
//!   queries, the loose objects and the hotdogs drawn: a stain lies flat on the ground or a wall (shaped by
//!   the way the round came in, with drips running down a wall), goes with a loose object, or smears the skin
//!   of a hotdog (until it respawns, ragdoll and all). Stains are textured quads that share their meshes
//!   and materials, so they draw in a few batches. A mark is wide ([`mark_width`]): [`WALL_SCALE`] times what it
//!   once was on a wall and [`FLOOR_SCALE`] times on the floor, more for a gun that hits harder ([`heft`]: the
//!   sniper's and the bazooka's are the biggest). On a wall it runs in up to four drips of different lengths that
//!   slow to a stop within [`RUN`] seconds, and are not touched again. It starts wet and shining and goes matte
//!   after [`WET`] seconds, and dries away after [`LIFE`] seconds, at most [`Budget::stains`] of them (and
//!   [`Budget::quads`] drawn quads among them), the oldest going first.
//! * **The flame.** Each round of hot sauce is a cluster of camera-facing cards: a white-yellow core going
//!   out to orange, red and dark smoke as it flies, flickering, with embers falling from it and a light
//!   thrown on the room nearby (not at the lower graphics levels).
//! * **The relish blast.** A massive burst wherever a relish glob bursts: a core flash, a ball of relish bursting
//!   into dozens of globs and pickle chunks, a mist, a shockwave ring marking the kill zone, large splats thrown over
//!   everything within it, relish on the lens, a shake of the camera, a pulse of light and its sound ([`boom`]).
//!
//! How much of all this there is follows the graphics level (`quality`): [`Budget`].
use super::{smooth, Game, Weapon};
use crate::quality::Level;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::NotShadowCaster,
    mesh::Indices,
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};
use kitchen_sim::feast::{self, Feast};
use kitchen_sim::map::Level as Ground;
use std::collections::{HashMap, VecDeque};
use std::f32::consts::{PI, TAU};

/// The relish bazooka's burst: its own parts, budgets and pools, and the splats, smears and stains it leaves through
/// this module's.
pub mod boom;

/// Seconds a stain lasts, and the last of them over which it dries away, in `DRY` steps. (A stain used to go after
/// 30 s: a splat that took a shot to make is worth keeping for a round's worth of looking at.)
pub const LIFE: f32 = 90.;
pub const FADE: f32 = 6.;
pub const DRY: usize = 4;
/// Seconds a stain is wet, shining, before it settles to matte.
pub const WET: f32 = 10.;
/// How many looks a stain goes through: wet, matte, then the `DRY` steps of its fading (`finish`).
pub const LOOKS: usize = 2 + DRY;
/// Seconds after landing by which the drips down a wall have run as far as they go. Until then a splat is animated
/// (its mark spreads, its drips run); after that it is left as it is, but for a change of look as it dries.
pub const RUN: f32 = 3.2;
/// How much wider a mark is drawn on a wall (or any upright surface) and on the floor than it once was.
pub const WALL_SCALE: f32 = 2.25;
pub const FLOOR_SCALE: f32 = 1.5;
/// How far a mark is lifted off the surface it lies on, and how much more each newer one lies over the last.
const LIFT: f32 = 0.0006;
const LAYER: f32 = 0.00004;
/// Smears a hotdog wears at most; the oldest goes first.
pub const SMEARS: usize = 8;
/// Hidden entities kept for reuse.
const POOL: usize = 600;
/// How hard drops and chunks fall, m/s² (the kitchen's drops have always fallen gently).
const FALL: f32 = 3.2;

/// A condiment: what a gun squirts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sauce {
    Mustard,
    Ketchup,
    Hot,
    Relish,
    Syrup,
    Mayo,
}
impl Sauce {
    pub const ALL: [Sauce; 6] = [Sauce::Mustard, Sauce::Ketchup, Sauce::Hot, Sauce::Relish, Sauce::Syrup, Sauce::Mayo];
    /// What each gun squirts; the bug spray is a mist and leaves nothing.
    pub fn of(weapon: usize) -> Option<Sauce> {
        match weapon {
            1 => Some(Sauce::Mustard),
            2 => Some(Sauce::Ketchup),
            3 => Some(Sauce::Hot),
            4 => Some(Sauce::Relish),
            6 => Some(Sauce::Syrup),
            7 => Some(Sauce::Mayo),
            _ => None,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
    /// sRGB: where it lies thick (darker, saturated), its body, and where it is spread thin (lighter, but still the
    /// sauce's own colour: a stain's rim must not wash out to cream, or it reads as a pale ghost from across the map).
    pub fn tones(self) -> [[f32; 3]; 3] {
        match self {
            Sauce::Mustard => [[0.95, 0.66, 0.0], [1.0, 0.79, 0.0], [1.0, 0.87, 0.14]],
            // Ketchup is dark: a deep tomato red that only lightens towards orange-red where it is thin. Kept
            // from going bright, which the tone mapping would wash towards pink.
            Sauce::Ketchup => [[0.38, 0.008, 0.008], [0.74, 0.03, 0.02], [0.90, 0.14, 0.06]],
            Sauce::Hot => [[0.55, 0.04, 0.0], [0.92, 0.20, 0.01], [1.0, 0.45, 0.06]],
            Sauce::Relish => [[0.12, 0.30, 0.02], [0.30, 0.60, 0.06], [0.60, 0.78, 0.18]],
            Sauce::Syrup => [[0.42, 0.16, 0.0], [0.74, 0.36, 0.03], [0.92, 0.62, 0.14]],
            Sauce::Mayo => [[0.84, 0.80, 0.64], [0.95, 0.93, 0.82], [0.99, 0.98, 0.93]],
        }
    }
    /// The share of its own colour it gives off, added after the light: mustard and ketchup are the teams'
    /// colours and must read across the room and in shadow; hot sauce is hot.
    fn glow(self) -> f32 {
        match self {
            Sauce::Mustard => 0.32,
            Sauce::Ketchup => 0.5,
            Sauce::Hot => 0.45,
            Sauce::Relish => 0.08,
            Sauce::Syrup => 0.08,
            Sauce::Mayo => 0.03,
        }
    }
    /// Width, in metres, of the mark a round leaves on the floor when it weighs what the mustard pistol's does (see
    /// `heft` for what the others weigh): how far this sauce spreads.
    fn mark(self) -> f32 {
        match self {
            Sauce::Mustard => 0.05,
            Sauce::Ketchup => 0.045,
            Sauce::Hot => 0.036,
            Sauce::Relish => 0.05,
            Sauce::Syrup => 0.04,
            Sauce::Mayo => 0.06,
        }
    }
    /// How drips run down a wall: the most there are, how long the longest is against the mark, and seconds the
    /// slowest takes to run (thin hot sauce quickly, syrup and ketchup slowly). All of them stop within `RUN`.
    fn drips(self) -> (u32, f32, f32) {
        match self {
            Sauce::Mustard => (3, 0.9, 1.3),
            Sauce::Ketchup => (4, 1.0, 1.7),
            Sauce::Hot => (2, 0.9, 0.9),
            Sauce::Relish => (3, 0.55, 1.8),
            Sauce::Syrup => (4, 1.3, 2.0),
            Sauce::Mayo => (2, 0.8, 1.6),
        }
    }
}

/// How heavy a round of `weapon` is against the mustard pistol's, as a mark: by the damage it does, to the power
/// 0.4 so that a round that hits four times as hard marks about one and three quarter times as wide. The sniper's
/// and the bazooka's are the biggest; the light guns (the rifle, the mayonnaise, the flame) leave smaller ones.
pub fn heft(weapon: usize) -> f32 {
    let damage = feast::PLAYER_DAMAGE.get(weapon).copied().unwrap_or(0.);
    (damage / feast::PLAYER_DAMAGE[PISTOL]).powf(0.4).clamp(0.75, 1.8)
}
/// The pistol, whose round everything else is weighed against.
const PISTOL: usize = 1;
const BAZOOKA: usize = 4;
/// How much wider a mark is on a surface whose normal is `normal` than it would be lying flat on the floor: the full
/// `WALL_SCALE` on an upright face, `FLOOR_SCALE` on the floor, between them on a slope.
pub fn surface_scale(normal: Vec3) -> f32 {
    let up = normal.y.abs().clamp(0., 1.);
    WALL_SCALE + (FLOOR_SCALE - WALL_SCALE) * up
}
/// The width of the mark one round of `weapon` leaves on a surface facing `normal`, in metres, before the
/// round-to-round variation. (The bazooka's glob is a splatter of its own: a hand-span on the floor, less on a wall.)
pub fn mark_width(weapon: usize, normal: Vec3) -> f32 {
    let Some(sauce) = Sauce::of(weapon) else {
        return 0.;
    };
    let base = if weapon == BAZOOKA { 0.08 + 0.03 * normal.y.abs().clamp(0., 1.) } else { sauce.mark() * heft(weapon) };
    base * surface_scale(normal)
}

/// How much of the effects a graphics level draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    /// Stains at once; the oldest goes when another is made.
    pub stains: usize,
    /// The quads (marks, drips and lumps: each one a drawn entity) all the stains together may have, which the oldest
    /// stains give up first. A wall splat with its drips costs several; this is the cap that holds the cost down.
    pub quads: usize,
    /// The most drips one splat on a wall runs.
    pub drips: u32,
    /// Drops, chunks, embers and puffs in the air at once.
    pub particles: usize,
    /// Lights the flame throws on the room (the relish burst has its own: `boom::Plan`).
    pub lights: usize,
    /// The share of a stream's drops drawn.
    pub beads: f32,
    /// The share of a splash's droplets.
    pub droplets: f32,
    /// Cards in one round of flame.
    pub flame: usize,
    /// The share of embers.
    pub sparks: f32,
}
impl Budget {
    pub fn of(level: Level) -> Self {
        match level {
            Level::High => Budget { stains: 200, quads: 600, drips: 4, particles: 320, lights: 2, beads: 1., droplets: 1., flame: 9, sparks: 1. },
            Level::Medium => Budget { stains: 160, quads: 480, drips: 3, particles: 240, lights: 1, beads: 1., droplets: 0.75, flame: 8, sparks: 0.75 },
            // (Every quad and particle is an entity the main thread checks, extracts and batches each frame: on the levels
            // for slow processors the oldest stains go sooner.)
            Level::Low => Budget { stains: 70, quads: 200, drips: 2, particles: 90, lights: 0, beads: 0.75, droplets: 0.5, flame: 6, sparks: 0.5 },
            Level::Lowest => Budget { stains: 36, quads: 100, drips: 1, particles: 45, lights: 0, beads: 0.6, droplets: 0.3, flame: 4, sparks: 0.25 },
        }
    }
}

/// Things that last a while and go oldest first: when there are more of them than a cap, when they weigh more than
/// another (a stain weighs the quads it draws), and when they are `LIFE` old.
pub struct Book<T> {
    entries: VecDeque<(f32, usize, T)>,
    weight: usize,
}
impl<T> Default for Book<T> {
    fn default() -> Self {
        Self { entries: VecDeque::new(), weight: 0 }
    }
}
impl<T> Book<T> {
    #[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// What all of it weighs.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn weight(&self) -> usize {
        self.weight
    }
    /// Keeps what was made at `born`, weighing `weight`. Returns the oldest beyond `cap` of them, or beyond `heavy` in
    /// all (the one just made is never one of them), which go at once.
    pub fn add(&mut self, born: f32, item: T, weight: usize, cap: usize, heavy: usize) -> Vec<T> {
        self.entries.push_back((born, weight, item));
        self.weight += weight;
        let mut gone = vec![];
        while self.entries.len() > 1 && (self.entries.len() > cap.max(1) || self.weight > heavy) {
            if let Some((_, weight, item)) = self.entries.pop_front() {
                self.weight -= weight;
                gone.push(item);
            }
        }
        gone
    }
    /// Takes out everything `LIFE` old at `now`.
    pub fn expire(&mut self, now: f32) -> Vec<T> {
        let mut gone = vec![];
        while self.entries.front().is_some_and(|(born, _, _)| now - born >= LIFE) {
            if let Some((_, weight, item)) = self.entries.pop_front() {
                self.weight -= weight;
                gone.push(item);
            }
        }
        gone
    }
    /// Takes out everything `keep` says no to.
    pub fn drop_where(&mut self, mut keep: impl FnMut(&T) -> bool) -> Vec<T> {
        let mut gone = vec![];
        let mut kept = VecDeque::with_capacity(self.entries.len());
        for (born, weight, item) in self.entries.drain(..) {
            if keep(&item) {
                kept.push_back((born, weight, item));
            } else {
                self.weight -= weight;
                gone.push(item);
            }
        }
        self.entries = kept;
        gone
    }
    /// Everything, oldest first.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (f32, &mut T)> {
        self.entries.iter_mut().map(|(born, _, item)| (*born, item))
    }
    /// Everything, newest first: for what only the newest need be looked at.
    pub fn iter_mut_newest(&mut self) -> impl Iterator<Item = (f32, &mut T)> {
        self.entries.iter_mut().rev().map(|(born, _, item)| (*born, item))
    }
    pub fn drain(&mut self) -> Vec<T> {
        self.weight = 0;
        self.entries.drain(..).map(|(_, _, item)| item).collect()
    }
}
/// Which look a stain of this age shows (an index into a sauce's materials): 0 wet, 1 matte from `WET` seconds on,
/// then 2 to `1 + DRY` over its last `FADE` seconds as its thin edge dries away.
pub fn finish(age: f32) -> usize {
    if age < WET {
        0
    } else if age < LIFE - FADE {
        1
    } else {
        1 + ((((age - (LIFE - FADE)) / FADE) * DRY as f32) as usize + 1).min(DRY)
    }
}

/// What a round landed on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Surface {
    /// The counter, a deck, a ramp, the floor of a sink or the top of a block: a stain lies on it.
    Ground { at: Vec3, normal: Vec3 },
    /// The side of a block or of a sink: a stain with drips running down it.
    Wall { at: Vec3, normal: Vec3 },
    /// A loose object: the stain goes with it.
    Prop { id: usize, at: Vec3, normal: Vec3 },
    /// A hotdog: a smear on its skin.
    Hotdog { id: u32 },
    /// Nothing near: the round ran out of range in the open, and only drops fall from it.
    Air,
}
/// How close a surface must be to the point a round ended at to be what it hit.
const TOUCH: f32 = 0.006;
/// What a round of `radius` that ended at `p`, flying along `dir` when that is known, landed on. Asks the
/// level (its ground under the point, its cover along each axis), the loose objects, and the hotdogs as
/// drawn (`bodies`: each one's number and the middle of its body). A surface beats a hotdog: a round stops
/// at a hotdog in the open, at the sphere round its chest, never touching a wall.
pub fn classify(feast: &Feast, p: Vec3, dir: Option<Vec3>, radius: f32, bodies: &[(u32, Vec3)]) -> Surface {
    let level = &feast.level;
    let dir = dir.and_then(|d| d.try_normalize());
    let mut best: Option<(f32, Surface)> = None;
    let mut consider = |gap: f32, surface: Surface| {
        if best.is_none_or(|(g, _)| gap < g) {
            best = Some((gap, surface));
        }
    };
    let ground = level.ground_at(p.x, p.z);
    let above = p.y - ground;
    // On the ground, or just inside a deck or ramp, which rounds fly through (its top is the surface seen).
    if above.abs() < TOUCH || (-0.03..0.).contains(&above) {
        consider(above.abs(), Surface::Ground { at: Vec3::new(p.x, ground, p.z), normal: slope(level, p.x, p.z) });
    }
    // The faces of blocks (and a sink's sides and floor): short probes along each axis from just behind the
    // point. A round can only have hit a face it was flying into.
    const BACK: f32 = 0.012;
    for axis in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z, Vec3::NEG_Y, Vec3::Y] {
        if dir.is_some_and(|d| axis.dot(d) < -0.05) {
            continue;
        }
        let t = level.cover_from(p - axis * BACK, axis);
        if t > 0. && t < BACK + TOUCH {
            let at = p + axis * (t - BACK);
            let normal = -axis;
            consider(
                (t - BACK).abs(),
                if normal.y > 0.5 { Surface::Ground { at, normal } } else { Surface::Wall { at, normal } },
            );
        }
    }
    // Loose objects: along the way it came, or else along each axis.
    let rays: Vec<Vec3> = match dir {
        Some(d) => vec![d],
        None => vec![Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z, Vec3::NEG_Y, Vec3::Y],
    };
    for d in rays {
        let back = 0.03;
        let from = p - d * back;
        if let Some(hit) = feast.props.raycast(from.as_dvec3().to_array(), d.as_dvec3().to_array(), (back + 0.02) as f64, None) {
            let at = Vec3::new(hit.point[0] as f32, hit.point[1] as f32, hit.point[2] as f32);
            let normal = Vec3::new(hit.normal[0] as f32, hit.normal[1] as f32, hit.normal[2] as f32).normalize_or(-d);
            consider(at.distance(p).min((hit.distance as f32 - back).abs()), Surface::Prop { id: hit.id, at, normal });
        }
    }
    if let Some((gap, surface)) = best.filter(|(gap, _)| *gap <= TOUCH * 2.) {
        let _ = gap;
        return surface;
    }
    // A hotdog: the nearest whose body the point is at.
    let reach = 0.055 + radius + 0.035;
    if let Some((id, _)) = bodies
        .iter()
        .map(|(id, middle)| (*id, middle.distance(p)))
        .filter(|(_, far)| *far < reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
    {
        return Surface::Hotdog { id };
    }
    best.map_or(Surface::Air, |(_, surface)| surface)
}
/// The ground's slope at a point, as its normal: up on the flat, tilted on a ramp. A step (a deck's edge)
/// is not a slope.
fn slope(level: &Ground, x: f32, z: f32) -> Vec3 {
    let e = 0.01;
    let dx = (level.ground_at(x + e, z) - level.ground_at(x - e, z)) / (2. * e);
    let dz = (level.ground_at(x, z + e) - level.ground_at(x, z - e)) / (2. * e);
    if dx.abs() > 1.5 || dz.abs() > 1.5 {
        return Vec3::Y;
    }
    Vec3::new(-dx, 1., -dz).normalize()
}

/// The ground under a point at height `y`: the level's, unless that is a deck or a ramp overhead (rounds and drops fly
/// under a deck, and the level has only its top), when it is the counter. A sink's floor, below the counter, stands.
pub fn under(level: &Ground, x: f32, y: f32, z: f32) -> f32 {
    let ground = level.ground_at(x, z);
    if ground > y + 0.02 { feast::FLOOR.min(ground) } else { ground }
}
/// A hotdog's body in its torso's frame (player_model.rs): the sausage, a capsule from `SAUSAGE.0` to
/// `SAUSAGE.1` high of radius `SAUSAGE.2`, and the bun's two halves, near enough ellipsoids.
const SAUSAGE: (f32, f32, f32) = (0.046, 0.155, 0.023);
const BUN: [(Vec3, Vec3); 2] = [
    (Vec3::new(-0.023, 0.097, 0.012), Vec3::new(0.017, 0.068, 0.026)),
    (Vec3::new(0.023, 0.097, 0.012), Vec3::new(0.017, 0.068, 0.026)),
];
/// The middle of a hotdog's body, in its torso's frame.
pub const MIDDLE: Vec3 = Vec3::new(0., 0.1, 0.);
/// Where a ray in a hotdog's torso frame first meets its body, and the body's outward normal there.
pub fn body_hit(origin: Vec3, dir: Vec3) -> Option<(Vec3, Vec3)> {
    let d = dir.try_normalize()?;
    let mut best: Option<(f32, Vec3, Vec3)> = None;
    let mut keep = |t: f32, point: Vec3, normal: Vec3| {
        if t >= 0. && best.is_none_or(|(b, _, _)| t < b) {
            best = Some((t, point, normal));
        }
    };
    let (y0, y1, r) = SAUSAGE;
    // The sausage's side.
    let (a, b, c) = (d.x * d.x + d.z * d.z, 2. * (origin.x * d.x + origin.z * d.z), origin.x * origin.x + origin.z * origin.z - r * r);
    if a > 1e-8 {
        let disc = b * b - 4. * a * c;
        if disc >= 0. {
            let t = (-b - disc.sqrt()) / (2. * a);
            let point = origin + d * t;
            if (y0..=y1).contains(&point.y) {
                keep(t, point, Vec3::new(point.x, 0., point.z).normalize_or(Vec3::NEG_Z));
            }
        }
    }
    // Its rounded ends.
    for y in [y0, y1] {
        let centre = Vec3::Y * y;
        if let Some(t) = sphere(origin - centre, d, r) {
            let point = origin + d * t;
            if (y == y0 && point.y <= y0) || (y == y1 && point.y >= y1) {
                keep(t, point, (point - centre).normalize_or(Vec3::Y));
            }
        }
    }
    // The bun.
    for (centre, semi) in BUN {
        if let Some(t) = sphere((origin - centre) / semi, d / semi, 1.) {
            let point = origin + d * t;
            keep(t, point, ((point - centre) / (semi * semi)).normalize_or(Vec3::Y));
        }
    }
    best.map(|(_, point, normal)| (point, normal))
}
/// The nearest point of a hotdog's sausage to `p` (in its torso's frame), and its outward normal: where a
/// smear goes when the way the round came is not known.
pub fn body_near(p: Vec3) -> (Vec3, Vec3) {
    let (y0, y1, r) = SAUSAGE;
    let axis = Vec3::Y * p.y.clamp(y0, y1);
    let normal = Vec3::new(p.x, 0., p.z).normalize_or(Vec3::NEG_Z);
    (axis + normal * r, normal)
}
/// The first t >= 0 at which `o + d t` is on the sphere of radius `r` about the origin (d need not be unit).
fn sphere(o: Vec3, d: Vec3, r: f32) -> Option<f32> {
    let (a, b, c) = (d.dot(d), 2. * o.dot(d), o.dot(o) - r * r);
    let disc = b * b - 4. * a * c;
    if disc < 0. || a < 1e-12 {
        return None;
    }
    let root = disc.sqrt();
    let t = (-b - root) / (2. * a);
    if t >= 0. {
        Some(t)
    } else {
        let t = (-b + root) / (2. * a);
        (t >= 0.).then_some(t)
    }
}

// ---------------------------------------------------------------------------------------------------------
// What everything is drawn with, made once.

/// The stain atlas: `COLS` by `ROWS` cells of `CELL` texels. Four splats, a drip, a smear, a spatter of
/// drops, and a chunky splat (relish).
/// (Bigger cells than they once were, 128: a mark is drawn more than twice as wide now, and each texel with it.)
const CELL: usize = 192;
const COLS: usize = 4;
const ROWS: usize = 2;
const SPLATS: [usize; 4] = [0, 1, 2, 3];
const DRIP: usize = 4;
const SMEAR: usize = 5;
const SPATTER: usize = 6;
const CHUNKY: usize = 7;
/// The card atlas: two by two cells: a soft puff, a flame's tongue, smoke, a spark.
const PUFF: usize = 0;
const TONGUE: usize = 1;
const SMOKE: usize = 2;
const SPARK: usize = 3;
/// How a stain looks at each step of its life (`finish`): how rough its surface is and how much it gives back of the
/// room, wet and shining at first and then matte; how much of its own colour it glows with; and the alpha it is cut at
/// (the thin edge goes as it dries away). It is lit like anything else, with no clear coat: a clear coat would cost the
/// stains a shader of their own, and the browser would stall compiling it at the first shot.
struct Finish {
    rough: f32,
    reflectance: f32,
    glow: f32,
    cut: f32,
}
const FINISH: [Finish; LOOKS] = [
    Finish { rough: 0.2, reflectance: 0.55, glow: 1.15, cut: 0.28 },
    Finish { rough: 0.7, reflectance: 0.35, glow: 0.8, cut: 0.28 },
    Finish { rough: 0.7, reflectance: 0.35, glow: 0.8, cut: 0.45 },
    Finish { rough: 0.7, reflectance: 0.35, glow: 0.8, cut: 0.6 },
    Finish { rough: 0.7, reflectance: 0.35, glow: 0.8, cut: 0.74 },
    Finish { rough: 0.7, reflectance: 0.35, glow: 0.8, cut: 0.88 },
];

/// Meshes and materials the effects share.
pub struct Look {
    /// Flat marks, lying in their xz plane facing +y, one per atlas cell.
    decal: Vec<Handle<Mesh>>,
    /// A smear and a splat bent round a hotdog's sausage (axis along z, facing +y).
    wrap: [Handle<Mesh>; 2],
    /// Camera-facing cards, one per card atlas cell.
    card: Vec<Handle<Mesh>>,
    ball: Handle<Mesh>,
    cube: Handle<Mesh>,
    /// By sauce: the stain through its life (`finish`), wet first.
    stain: Vec<Vec<Handle<StandardMaterial>>>,
    /// By sauce: drops and globs where it is thick, its body, where it is thin.
    tone: Vec<[Handle<StandardMaterial>; 3]>,
    /// The mustard pistol's squirt in flight and the drops it sheds (`jet`).
    jet: Handle<StandardMaterial>,
    /// The flame from the inside out (white-yellow core, orange, red), and its embers.
    fire: [Handle<StandardMaterial>; 4],
    /// The relish burst's flash: white-gold and green-gold (`boom`).
    flash: [Handle<StandardMaterial>; 2],
    smoke: Handle<StandardMaterial>,
    /// The card atlas's texture (straight alpha), for veils made elsewhere (`boom`).
    cards: Handle<Image>,
    /// Pickle chunks: dark and pale green.
    chunk: [Handle<StandardMaterial>; 2],
    /// The bug spray's mist and the mayonnaise in a relish glob.
    mist: Handle<StandardMaterial>,
    cream: Handle<StandardMaterial>,
    /// By sauce: a fast round's tracer, the streak of light it leaves behind it (`TRACE`).
    streak: Vec<Handle<StandardMaterial>>,
}
/// Seconds of a round's flight its tracer shows behind it: a fast round (five times as fast as rounds once flew) is
/// drawn with a thin streak of its sauce's colour from where it was that long ago, so that it is seen at all.
pub const TRACE: f32 = 0.03;
impl Look {
    pub fn new(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) -> Self {
        let field = stain_field();
        let normals = images.add(atlas_image(normal_atlas(&field), false, true));
        let stain = Sauce::ALL
            .iter()
            .map(|sauce| {
                let colour = images.add(atlas_image(colour_atlas(&field, *sauce), true, false));
                let glow = srgb_linear(sauce.tones()[1]) * sauce.glow();
                FINISH
                    .iter()
                    .map(|finish| {
                        mats.add(StandardMaterial {
                            base_color: Color::WHITE,
                            base_color_texture: Some(colour.clone()),
                            emissive: LinearRgba::rgb(glow.x, glow.y, glow.z) * 0.6 * finish.glow,
                            emissive_texture: Some(colour.clone()),
                            normal_map_texture: Some(normals.clone()),
                            perceptual_roughness: finish.rough,
                            reflectance: finish.reflectance,
                            alpha_mode: AlphaMode::Mask(finish.cut),
                            depth_bias: 2.,
                            ..default()
                        })
                    })
                    .collect()
            })
            .collect();
        let tone = Sauce::ALL
            .map(|sauce| {
                let tones = sauce.tones();
                [0, 1, 2].map(|k| {
                    let c = srgb_linear(tones[k]);
                    let glow = c * sauce.glow();
                    mats.add(StandardMaterial {
                        base_color: Color::srgb(tones[k][0], tones[k][1], tones[k][2]),
                        emissive: LinearRgba::rgb(glow.x, glow.y, glow.z),
                        // Rough enough underneath that `visuals` leaves it be; the wet shine is the clear coat.
                        perceptual_roughness: [0.42, 0.36, 0.34][k],
                        reflectance: 0.5,
                        clearcoat: if sauce == Sauce::Syrup { 1. } else { 0.75 },
                        clearcoat_perceptual_roughness: if sauce == Sauce::Syrup { 0.04 } else { 0.09 },
                        ..default()
                    })
                })
            })
            .to_vec();
        // The mustard squirt is wet all over: its body colour, glowing more of it than a stain so that it stays
        // mustard yellow on its shaded side, its sheen tinted mustard so that the bright room it reflects does not
        // wash it out, and a glassy clear coat for sharp wet highlights (a full coat would grey it, as it does the
        // drops: the room is brighter in it than the light on the drop).
        let jet = {
            let body = Sauce::Mustard.tones()[1];
            let glow = srgb_linear(body) * 0.3;
            let tint = Vec3::ONE.lerp(srgb_linear(body), 0.6);
            mats.add(StandardMaterial {
                base_color: Color::srgb(body[0], body[1], body[2]),
                emissive: LinearRgba::rgb(glow.x, glow.y, glow.z),
                // Rough enough underneath that `visuals` leaves it be.
                perceptual_roughness: 0.34,
                reflectance: 0.5,
                specular_tint: Color::LinearRgba(LinearRgba::rgb(tint.x, tint.y, tint.z)),
                clearcoat: 0.4,
                clearcoat_perceptual_roughness: 0.06,
                ..default()
            })
        };
        let cards = images.add(card_image(false));
        let laid = images.add(card_image(true));
        // Light given off: added to what is behind (sparks, flashes), or, for the flame, laid over it as well, so
        // that a flame in a sunlit kitchen keeps its colours instead of washing out.
        let glowing = |mats: &mut Assets<StandardMaterial>, c: [f32; 3], over: bool| {
            mats.add(StandardMaterial {
                base_color: Color::LinearRgba(LinearRgba::rgb(c[0], c[1], c[2])),
                base_color_texture: Some(if over { laid.clone() } else { cards.clone() }),
                alpha_mode: if over { AlphaMode::Premultiplied } else { AlphaMode::Add },
                unlit: true,
                fog_enabled: false,
                ..default()
            })
        };
        let fire = [([4.2, 3.4, 1.4], true), ([3.4, 1.15, 0.1], true), ([1.5, 0.2, 0.03], true), ([6., 2.0, 0.3], false)].map(|(c, over)| glowing(mats, c, over));
        let flash = [[10., 9., 4.5], [3.0, 4.6, 0.7]].map(|c| glowing(mats, c, false));
        let veil = |mats: &mut Assets<StandardMaterial>, c: [f32; 3], a: f32| {
            mats.add(StandardMaterial {
                base_color: Color::srgba(c[0], c[1], c[2], a),
                base_color_texture: Some(cards.clone()),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })
        };
        let smoke = veil(mats, [0.045, 0.04, 0.036], 0.62);
        let chunk = [[0.10, 0.26, 0.04], [0.45, 0.66, 0.16]].map(|c| {
            mats.add(StandardMaterial {
                base_color: Color::srgb(c[0], c[1], c[2]),
                perceptual_roughness: 0.4,
                clearcoat: 0.6,
                clearcoat_perceptual_roughness: 0.15,
                ..default()
            })
        });
        // A tracer's streak: the sauce's lightest colour, glowing (brighter than the screen shows, so that it blooms) and
        // laid over what is behind it, so that it shows against a bright kitchen as well as a dark one.
        let streak = Sauce::ALL
            .iter()
            .map(|sauce| {
                let c = srgb_linear(sauce.tones()[2]) * 2.6;
                mats.add(StandardMaterial { base_color: Color::LinearRgba(LinearRgba::new(c.x, c.y, c.z, 0.85)), alpha_mode: AlphaMode::Blend, unlit: true, fog_enabled: false, ..default() })
            })
            .collect();
        Look {
            streak,
            decal: (0..COLS * ROWS).map(|cell| meshes.add(decal_mesh(cell))).collect(),
            wrap: [meshes.add(wrap_mesh(SMEAR)), meshes.add(wrap_mesh(SPLATS[1]))],
            card: (0..4).map(|cell| meshes.add(card_mesh(cell))).collect(),
            ball: meshes.add(Sphere::new(1.).mesh().uv(12, 8)),
            cube: meshes.add(Cuboid::default()),
            stain,
            tone,
            jet,
            fire,
            flash,
            smoke,
            cards,
            chunk,
            mist: mats.add(StandardMaterial {
                base_color: Color::srgba(0.78, 0.93, 0.97, 0.4),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            }),
            cream: mats.add(StandardMaterial {
                base_color: Color::srgb(0.96, 0.94, 0.84),
                perceptual_roughness: 0.4,
                clearcoat: 0.7,
                clearcoat_perceptual_roughness: 0.1,
                ..default()
            }),
        }
    }
}
fn srgb_linear(c: [f32; 3]) -> Vec3 {
    let f = |s: f32| if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) };
    Vec3::new(f(c[0]), f(c[1]), f(c[2]))
}

/// A flat mark in its xz plane, facing +y, one metre square, showing one cell of the stain atlas, with
/// tangents for its normal map.
fn decal_mesh(cell: usize) -> Mesh {
    let (u0, v0) = ((cell % COLS) as f32 / COLS as f32, (cell / COLS) as f32 / ROWS as f32);
    let (du, dv) = (1. / COLS as f32, 1. / ROWS as f32);
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-0.5, 0., -0.5], [0.5, 0., -0.5], [0.5, 0., 0.5], [-0.5, 0., 0.5]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 1., 0.]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[u0, v0], [u0 + du, v0], [u0 + du, v0 + dv], [u0, v0 + dv]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1., 0., 0., 1.]; 4])
        .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}
/// The radius a wrapped mark is bent to: a hair over the sausage's.
const WRAP: f32 = 0.0245;
/// A mark 34 mm wide and 40 mm long, bent round an axis along z `WRAP` below it, so that it lies on the
/// sausage; one cell of the stain atlas.
fn wrap_mesh(cell: usize) -> Mesh {
    let (u0, v0) = ((cell % COLS) as f32 / COLS as f32, (cell / COLS) as f32 / ROWS as f32);
    let (du, dv) = (1. / COLS as f32, 1. / ROWS as f32);
    let (wide, long, n) = (0.034, 0.040, 7usize);
    let (mut positions, mut normals, mut uvs, mut tangents, mut indices) = (vec![], vec![], vec![], vec![], vec![]);
    for j in 0..=n {
        for i in 0..=n {
            let (s, t) = (i as f32 / n as f32, j as f32 / n as f32);
            let angle = (s - 0.5) * wide / WRAP;
            positions.push([WRAP * angle.sin(), WRAP * angle.cos() - WRAP, (t - 0.5) * long]);
            normals.push([angle.sin(), angle.cos(), 0.]);
            tangents.push([angle.cos(), -angle.sin(), 0., 1.]);
            uvs.push([u0 + s * du, v0 + t * dv]);
        }
    }
    for j in 0..n {
        for i in 0..n {
            let k = (j * (n + 1) + i) as u32;
            let w = (n + 1) as u32;
            indices.extend([k, k + w + 1, k + 1, k, k + w, k + w + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
        .with_inserted_indices(Indices::U32(indices))
}
/// A card in its xy plane facing +z, one metre square, showing one cell of the card atlas (its top up).
fn card_mesh(cell: usize) -> Mesh {
    let (u0, v0) = ((cell % 2) as f32 * 0.5, (cell / 2) as f32 * 0.5);
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-0.5, -0.5, 0.], [0.5, -0.5, 0.], [0.5, 0.5, 0.], [-0.5, 0.5, 0.]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[u0, v0 + 0.5], [u0 + 0.5, v0 + 0.5], [u0 + 0.5, v0], [u0, v0]])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

fn hash(n: u32) -> f32 {
    crate::player_model::noise(n)
}
/// Smooth value noise in about 0..1.
fn value(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let h = |i: f32, j: f32| hash((i as i32 as u32).wrapping_mul(73_856_093) ^ (j as i32 as u32).wrapping_mul(19_349_663) ^ seed.wrapping_mul(83_492_791));
    let s = |t: f32| t * t * (3. - 2. * t);
    let (a, b, c, d) = (h(xi, yi), h(xi + 1., yi), h(xi, yi + 1.), h(xi + 1., yi + 1.));
    let top = a + (b - a) * s(fx);
    let bottom = c + (d - c) * s(fx);
    top + (bottom - top) * s(fy)
}
fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    (value(x, y, seed) + 0.5 * value(x * 2.03, y * 2.03, seed + 1) + 0.25 * value(x * 4.1, y * 4.1, seed + 2)) / 1.75
}

/// Blobs and strokes summed into a field: where it is over the edge there is sauce, thicker the more.
#[derive(Default)]
struct Shape {
    balls: Vec<(Vec2, f32, f32)>,
    strokes: Vec<(Vec2, Vec2, f32, f32)>,
    /// Pickle chunks: where, how big, and the turn they are rotated by (as a unit vector).
    chunks: Vec<(Vec2, f32, Vec2)>,
}
impl Shape {
    /// Everything `k` times as far from the middle and as big.
    fn scaled(mut self, k: f32) -> Self {
        for (c, r, _) in &mut self.balls {
            (*c, *r) = (*c * k, *r * k);
        }
        for (a, b, ra, rb) in &mut self.strokes {
            (*a, *b, *ra, *rb) = (*a * k, *b * k, *ra * k, *rb * k);
        }
        for (c, size, _) in &mut self.chunks {
            (*c, *size) = (*c * k, *size * k);
        }
        self
    }
    fn ball(&mut self, at: Vec2, r: f32, w: f32) {
        self.balls.push((at, r, w));
    }
    fn stroke(&mut self, a: Vec2, b: Vec2, ra: f32, rb: f32) {
        self.strokes.push((a, b, ra, rb));
    }
    /// The balls and strokes that reach anywhere between rows `y0` and `y1` (a ball or a stroke adds nothing beyond
    /// three of its radii), which are all that need be asked of any point there.
    fn reaching(&self, y0: f32, y1: f32) -> (Vec<u16>, Vec<u16>) {
        let balls = self.balls.iter().enumerate().filter(|(_, (c, r, _))| c.y + 3. * r >= y0 && c.y - 3. * r <= y1).map(|(i, _)| i as u16).collect();
        let strokes = self
            .strokes
            .iter()
            .enumerate()
            .filter(|(_, (a, b, ra, rb))| {
                let reach = 3. * ra.max(*rb);
                a.y.max(b.y) + reach >= y0 && a.y.min(b.y) - reach <= y1
            })
            .map(|(i, _)| i as u16)
            .collect();
        (balls, strokes)
    }
    /// The field at `p` from just those balls and strokes (`reaching`), and how much of a pickle chunk is there.
    fn at(&self, p: Vec2, balls: &[u16], strokes: &[u16]) -> (f32, f32) {
        let mut f = 0.;
        for i in balls {
            let (c, r, w) = &self.balls[*i as usize];
            let d2 = (p - *c).length_squared() / (r * r);
            if d2 < 9. {
                f += w * (-d2).exp();
            }
        }
        for i in strokes {
            let (a, b, ra, rb) = &self.strokes[*i as usize];
            let ab = *b - *a;
            let t = ((p - *a).dot(ab) / ab.length_squared()).clamp(0., 1.);
            let r = ra + (rb - ra) * t;
            let d2 = (p - (*a + ab * t)).length_squared() / (r * r);
            if d2 < 9. {
                f += (-d2).exp();
            }
        }
        let mut chunk: f32 = 0.;
        for (c, s, turn) in &self.chunks {
            let q = turn.rotate(p - *c);
            let d = q.x.abs().max(q.y.abs()) / s;
            chunk = chunk.max(1. - smooth((d - 0.7) / 0.3));
        }
        (f, chunk)
    }
}
/// The shape in one cell of the stain atlas, over -1..1 each way (+x is the way the round was going; +y is
/// down the cell, the way a drip runs).
fn cell_shape(cell: usize) -> Shape {
    let seed = cell as u32 * 977 + 13;
    let n = |k: u32| hash(seed.wrapping_mul(131).wrapping_add(k));
    let dir = |a: f32| Vec2::new(a.cos(), a.sin());
    let mut s = Shape::default();
    match cell {
        DRIP => {
            // A run down a wall, from a blob at the top to a bead at the bottom, wavering a little.
            s.ball(Vec2::new(0., -0.76), 0.26, 1.0);
            let mut prev = Vec2::new(0., -0.76);
            for k in 1..=8 {
                let y = -0.76 + k as f32 * 0.165;
                let next = Vec2::new(0.04 * (y * 7. + 1.).sin(), y);
                s.stroke(prev, next, 0.17 - k as f32 * 0.006, 0.164 - k as f32 * 0.006);
                prev = next;
            }
            s.ball(prev + Vec2::Y * 0.06, 0.2, 1.2);
        }
        SMEAR => {
            // Wiped on: a long body with a thick glob ahead and streaks trailing behind.
            for k in 0..6 {
                let x = -0.45 + k as f32 * 0.17;
                s.ball(Vec2::new(x, 0.04 * (k as f32 * 1.7).sin()), 0.20 + 0.03 * n(k), 0.85);
            }
            s.ball(Vec2::new(0.48, 0.02), 0.25, 1.25);
            for k in 0..5 {
                let y = -0.2 + k as f32 * 0.1 + 0.03 * n(10 + k);
                s.stroke(Vec2::new(-0.82 + 0.2 * n(20 + k), y), Vec2::new(0.0, y * 0.7), 0.025 + 0.02 * n(30 + k), 0.05);
            }
            for k in 0..4 {
                let a = n(40 + k) * TAU;
                s.ball(dir(a) * (0.62 + 0.2 * n(50 + k)), 0.03 + 0.03 * n(60 + k), 0.9);
            }
        }
        SPATTER => {
            for k in 0..14 {
                let (a, d) = (n(k) * TAU, 0.75 * n(k + 30).sqrt());
                s.ball(dir(a) * d, 0.035 + 0.09 * n(k + 60) * (1. - d * 0.6), 1.0);
            }
        }
        _ => {
            // A splat: a fat body with a ring of lobes round it, so that its outline is lumpy and not round (the
            // whole pushed a little the way the round was going), spikes thrown out mostly forward with a heavy drop
            // at the end of each, and satellite drops of every size flung round it.
            let ahead = if cell == CHUNKY { 0. } else { 0.06 };
            s.ball(Vec2::new(-0.04 + ahead, 0.), 0.34, 1.15);
            let lobes = 7 + (n(1) * 4.) as u32;
            for k in 0..lobes {
                let a = k as f32 / lobes as f32 * TAU + n(2 + k) * 0.8;
                s.ball(dir(a) * (0.14 + 0.2 * n(20 + k)) + Vec2::X * ahead, 0.12 + 0.13 * n(40 + k), 0.95);
            }
            let spikes = 6 + (n(3) * 5.) as u32;
            for k in 0..spikes {
                let forward = k % 3 != 0;
                let a = if forward { (n(60 + k) - 0.5) * 2.2 } else { n(60 + k) * TAU };
                let reach = 0.46 + 0.32 * n(80 + k) * if forward { 1. } else { 0.7 };
                s.stroke(dir(a) * 0.24, dir(a) * reach, 0.09, 0.025 + 0.02 * n(100 + k));
                s.ball(dir(a) * (reach + 0.04), 0.035 + 0.045 * n(120 + k), 1.0);
            }
            // Satellites: a few big drops, then a spray of small ones, some of them well out.
            for k in 0..17 {
                let a = if k % 2 == 0 { (n(140 + k) - 0.5) * 2.4 } else { n(140 + k) * TAU };
                let big = k < 4;
                let (far, r) = if big { (0.58 + 0.2 * n(160 + k), 0.045 + 0.03 * n(180 + k)) } else { (0.6 + 0.3 * n(160 + k), 0.014 + 0.026 * n(180 + k)) };
                s.ball(dir(a) * far, r, 1.0);
            }
            if cell == CHUNKY {
                for k in 0..16 {
                    let (a, d) = (n(200 + k) * TAU, 0.5 * n(220 + k).sqrt());
                    s.chunks.push((dir(a) * d, 0.04 + 0.05 * n(240 + k), dir(n(260 + k) * TAU)));
                }
            }
            // Drawn a little smaller in its cell, which leaves the mark's own width to say how big a splat is: it
            // is `WALL_SCALE` times what it was across, and not more for the fatter shape.
            return s.scaled(FIT);
        }
    }
    s
}
/// How much of its cell a splat's shape fills, against the 1.0 of the shapes that were drawn into the first atlas
/// (the redrawn lobes and satellites would otherwise read 25% wider than the quad's growth).
const FIT: f32 = 0.84;
/// Over the whole atlas, row by row: the field (cut at 0.42 for the edge of the sauce), the chunks, and a mottle (the
/// same for every sauce) that breaks up the flatness of a stain's colour.
struct Field {
    f: Vec<f32>,
    chunk: Vec<f32>,
    mottle: Vec<f32>,
}
const WIDE: usize = CELL * COLS;
const HIGH: usize = CELL * ROWS;
/// How many of today's texels there are to one of a 128-texel cell's, so that noise made in texels keeps its size.
const GRAIN: f32 = CELL as f32 / 128.;
fn stain_field() -> Field {
    let mut f = vec![0.; WIDE * HIGH];
    let mut chunk = vec![0.; WIDE * HIGH];
    let mut mottle = vec![0.; WIDE * HIGH];
    for cell in 0..COLS * ROWS {
        let shape = cell_shape(cell);
        let (cx, cy) = ((cell % COLS) * CELL, (cell / COLS) * CELL);
        // How far each cell's outline wanders from its shape: a splat lumpily, the long shapes hardly.
        let wander = match cell {
            DRIP => 0.02,
            SMEAR => 0.035,
            SPATTER => 0.04,
            _ => 0.09,
        };
        for y in 0..CELL {
            // What reaches this row, with room for the outline to wander.
            let row = (y as f32 + 0.5) / CELL as f32 * 2. - 1.;
            let (balls, strokes) = shape.reaching(row - wander - 0.01, row + wander + 0.01);
            for x in 0..CELL {
                let p = Vec2::new((x as f32 + 0.5) / CELL as f32 * 2. - 1., row);
                let warp = Vec2::new(value(p.x * 1.7 + cell as f32 * 5.3, p.y * 1.7, 41) - 0.5, value(p.x * 1.7, p.y * 1.7 + cell as f32 * 7.7, 43) - 0.5) * 2. * wander;
                let (mut v, c) = shape.at(p + warp, &balls, &strokes);
                // (Nothing here would be seen, with or without the ragged edge: coverage starts at 0.22. Most of a
                // cell is like this, and the noise below is the cost of making the atlas.)
                if v < 0.18 {
                    continue;
                }
                // A ragged edge, and nothing at the border so that cells never bleed into each other.
                v *= 0.86 + 0.28 * fbm(p.x * 6. + cell as f32 * 3.1, p.y * 6., 7);
                let border = 1. - smooth((p.x.abs().max(p.y.abs()) - 0.9) / 0.08);
                let at = (cy + y) * WIDE + cx + x;
                f[at] = v * border;
                chunk[at] = c * (v > 0.42) as u8 as f32;
                // (Only where there is sauce: the rest is never seen.)
                if v * border > 0.2 {
                    mottle[at] = fbm((cx + x) as f32 / (9. * GRAIN), (cy + y) as f32 / (9. * GRAIN), 31);
                }
            }
        }
    }
    Field { f, chunk, mottle }
}
/// How thick the sauce lies where the field is `f`: 0 at its edge, 1 where it is deepest (the middle of a blob).
fn thickness(f: f32) -> f32 {
    ((f - 0.42) / 1.05).clamp(0., 1.)
}
fn coverage(f: f32) -> f32 {
    ((f - 0.22) / 0.7).clamp(0., 1.)
}
/// How much more of a sauce's colour than grey it keeps (above 1: richer than its tones), so that a stain reads
/// as the sauce's own colour from across the room.
const RICHNESS: f32 = 1.2;
/// One sauce's colours over the atlas (sRGB bytes): its tones from thin to thick, mottled, and the alpha the
/// stain is cut at. Relish has its pickle chunks in it.
fn colour_atlas(field: &Field, sauce: Sauce) -> Vec<u8> {
    let [thick, body, thin] = sauce.tones().map(Vec3::from_array);
    let richness = if sauce == Sauce::Mayo { 1. } else { RICHNESS };
    let bytes = |c: Vec3, a: f32| {
        let grey = c.dot(Vec3::new(0.2126, 0.7152, 0.0722));
        let c = Vec3::splat(grey).lerp(c, richness);
        [c.x, c.y, c.z, a].map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8)
    };
    // Where there is no sauce, nothing is seen, but a hair of its colour at the edge is what a filter reaches for.
    let empty = bytes(thin * 0.93, 0.);
    let mut out = Vec::with_capacity(WIDE * HIGH * 4);
    for y in 0..HIGH {
        for x in 0..WIDE {
            let at = y * WIDE + x;
            let f = field.f[at];
            if f == 0. {
                out.extend(empty);
                continue;
            }
            let t = thickness(f);
            let mut c = if t < 0.3 { thin.lerp(body, t / 0.3) } else { body.lerp(thick, (t - 0.3) / 0.7) };
            c *= 0.93 + 0.14 * field.mottle[at];
            if sauce == Sauce::Relish {
                let speck = value(x as f32 / (2.6 * GRAIN), y as f32 / (2.6 * GRAIN), 77);
                let pickle = field.chunk[at].max(if speck > 0.8 { 1. } else { 0. });
                c = c.lerp(Vec3::new(0.08, 0.2, 0.03), pickle * 0.85);
                // Streaks of the mayonnaise mixed in.
                let cream = smooth((fbm(x as f32 / (7. * GRAIN), y as f32 / (7. * GRAIN), 91) - 0.62) / 0.08) * (1. - pickle);
                c = c.lerp(Vec3::new(0.9, 0.9, 0.78), cream * 0.55);
            }
            out.extend(bytes(c, if f > 0.05 { coverage(f) } else { 0. }));
        }
    }
    out
}
/// The relief of the sauce, thickest in the middle of each blob with a glossy ripple, as a tangent-space
/// normal map (the green channel up the atlas: bevy's tangent frame, with v down the image).
fn normal_atlas(field: &Field) -> Vec<u8> {
    let height: Vec<f32> = (0..WIDE * HIGH)
        .map(|at| {
            let (x, y) = ((at % WIDE) as f32, (at / WIDE) as f32);
            let f = field.f[at];
            let t = smooth((f - 0.3) / 0.9);
            t + 0.4 * field.chunk[at] + 0.05 * t * value(x / (3.5 * GRAIN), y / (3.5 * GRAIN), 5)
        })
        .collect();
    let h = |x: i32, y: i32| height[(y.clamp(0, HIGH as i32 - 1) as usize) * WIDE + x.clamp(0, WIDE as i32 - 1) as usize];
    let mut out = Vec::with_capacity(WIDE * HIGH * 4);
    for y in 0..HIGH as i32 {
        for x in 0..WIDE as i32 {
            // (The slope per texel falls as the cells grow, so the relief is turned up with them.)
            let strength = 9. * GRAIN;
            let dx = (h(x + 1, y) - h(x - 1, y)) * 0.5 * strength;
            let dy = (h(x, y + 1) - h(x, y - 1)) * 0.5 * strength;
            let n = Vec3::new(-dx, dy, 1.).normalize();
            out.extend([n.x, n.y, n.z].map(|v| ((v * 0.5 + 0.5) * 255. + 0.5) as u8));
            out.push(255);
        }
    }
    out
}
/// The card atlas (white, its alpha the shape): a soft puff, a flame's tongue (pointing up), smoke, a spark.
/// With `premultiplied`, the colour is white times the shape (for cards laid over what is behind), otherwise
/// white throughout (for cards that are added or blended by their alpha).
fn card_image(premultiplied: bool) -> Image {
    let size = 256usize;
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let cell = (x / 128) + 2 * (y / 128);
            let p = Vec2::new(((x % 128) as f32 + 0.5) / 64. - 1., ((y % 128) as f32 + 0.5) / 64. - 1.);
            let r2 = p.length_squared();
            let edge = 1. - smooth((r2.sqrt() - 0.82) / 0.16);
            let a = match cell {
                PUFF => (-r2 * 4.2).exp() * (0.85 + 0.15 * fbm(p.x * 3., p.y * 3., 3)),
                TONGUE => {
                    // A lick of flame: a soft blob drawn up into ragged tongues, brightest low down.
                    let warp = Vec2::new(0.35 * (fbm(p.x * 2. + 3., p.y * 2.2, 11) - 0.5), 0.25 * (fbm(p.x * 2.4, p.y * 1.6 + 5., 13) - 0.5));
                    let q = p + warp;
                    let body = (-(q.x * q.x * 3.6 + (q.y + 0.12).powi(2) * 1.8)).exp();
                    let licks = fbm(q.x * 3.6, q.y * 2.4 - 1., 17);
                    ((body * (0.4 + 1.3 * licks) - 0.12) * 1.5 * (1. - 0.3 * smooth((-p.y - 0.2) / 0.7))).clamp(0., 1.)
                }
                SMOKE => ((-r2 * 3.).exp() * (0.35 + 0.9 * fbm(p.x * 2.6 + 4., p.y * 2.6, 23))).min(1.),
                _ => (-r2 * 26.).exp() + 0.3 * (-r2 * 5.).exp(),
            } * edge;
            let a = (a.clamp(0., 1.) * 255. + 0.5) as u8;
            let c = if premultiplied { a } else { 255 };
            out.extend([c, c, c, a]);
        }
    }
    let (data, levels) = mips(out, size, size, false, false);
    let mut image = Image::new_uninit(
        Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = sampler(1);
    image
}
fn sampler(anisotropy: u16) -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: anisotropy,
        ..default()
    })
}
fn atlas_image(data: Vec<u8>, srgb: bool, normal: bool) -> Image {
    let (data, levels) = mips(data, WIDE, HIGH, srgb, normal);
    let mut image = Image::new_uninit(
        Extent3d { width: WIDE as u32, height: HIGH as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = sampler(4);
    image
}
/// Appends every smaller mip of a `w` by `h` RGBA image, down to one texel, each texel the mean of the four
/// above it (in linear light for an sRGB image; renormalised for a normal map). Returns it all and the count.
fn mips(mut data: Vec<u8>, w: usize, h: usize, srgb: bool, normal: bool) -> (Vec<u8>, u32) {
    let decode = |b: u8| {
        let s = b as f32 / 255.;
        if srgb && s > 0.04045 {
            ((s + 0.055) / 1.055).powf(2.4)
        } else if srgb {
            s / 12.92
        } else {
            s
        }
    };
    let encode = |l: f32| {
        let s = if srgb { if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1. / 2.4) - 0.055 } } else { l };
        (s.clamp(0., 1.) * 255. + 0.5) as u8
    };
    let (mut start, mut w, mut h, mut levels) = (0, w, h, 1);
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::with_capacity(nw * nh * 4);
        for y in 0..nh {
            for x in 0..nw {
                let mut sum = [0f32; 4];
                let mut weight = 0.;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                    let at = start + (sy * w + sx) * 4;
                    // Colour is averaged by coverage, so the colour of empty texels never creeps in.
                    let a = data[at + 3] as f32 / 255.;
                    let k = if normal { 1. } else { a.max(0.002) };
                    for c in 0..3 {
                        sum[c] += decode(data[at + c]) * k;
                    }
                    sum[3] += a;
                    weight += k;
                }
                let mut mean = [sum[0] / weight, sum[1] / weight, sum[2] / weight, sum[3] / 4.];
                if normal {
                    let n = Vec3::new(mean[0] * 2. - 1., mean[1] * 2. - 1., mean[2] * 2. - 1.).normalize_or(Vec3::Z);
                    mean = [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, 1.];
                }
                next.extend([encode(mean[0]), encode(mean[1]), encode(mean[2]), (mean[3].clamp(0., 1.) * 255. + 0.5) as u8]);
            }
        }
        start = data.len();
        data.extend(next);
        (w, h) = (nw, nh);
        levels += 1;
    }
    (data, levels)
}

// ---------------------------------------------------------------------------------------------------------
// What is out there now.

/// A card turned to face the camera every frame, rolled by this many radians.
#[derive(Component)]
pub struct Card(pub f32);
/// A card drawn out along this way in the world (a lick of flame along its jet): its top is turned to point
/// that way on screen, before its roll.
#[derive(Component, Default)]
pub struct Along(pub Vec3);

/// One part of a stain.
struct Part {
    entity: Entity,
    kind: PartKind,
    /// Where it ends up, in its parent's frame.
    base: Transform,
}
enum PartKind {
    /// The flat mark: it spreads out in the first instant.
    Mark,
    /// A drip running down from the mark: from `top` along `down` (in its parent's frame), to `length`, starting
    /// after `delay` seconds and running for `pace` more, slowing to a stop (see `drip_place`).
    Drip { top: Vec3, down: Vec3, normal: Vec3, length: f32, width: f32, delay: f32, pace: f32 },
    /// Something standing on it (a glob, a chunk): it shrinks away as the stain dries.
    Lump,
}
struct Stain {
    sauce: usize,
    parts: Vec<Part>,
    /// The look shown (`finish`).
    step: usize,
    /// Whether it has been through its first `RUN` seconds, in which it spreads and its drips run: after that it is
    /// as it will stay, and nothing but its look is ever touched again.
    set: bool,
}
/// Something in the air.
struct Mote {
    entity: Entity,
    kind: MoteKind,
    velocity: Vec3,
    age: f32,
    life: f32,
    size: Vec3,
    spin: Vec3,
    sauce: usize,
    landed: bool,
}
#[derive(Clone, Copy, PartialEq)]
enum MoteKind {
    /// A drop of sauce: it falls and lies flat where it lands until it is gone.
    Drop,
    /// A pickle chunk: it tumbles, and where it lands it stays, as part of the stains.
    Chunk,
    Ember,
    Smoke,
}
/// A light thrown on the room by a stream of flame (its shooter).
struct Lamp {
    entity: Entity,
    owner: Option<u32>,
    left: f32,
}

/// Everything the effects have out, and the entities kept for reuse.
#[derive(Resource, Default)]
pub struct Mess {
    pool: Vec<Entity>,
    stains: Book<Stain>,
    /// Each hotdog's smears, oldest first, and whether it was alive at the last look.
    smears: HashMap<u32, VecDeque<Vec<Entity>>>,
    living: HashMap<u32, bool>,
    motes: Vec<Mote>,
    lamps: Vec<Lamp>,
    budget: Option<Budget>,
    /// Marks made so far: each lies a hair over the last.
    count: u32,
    /// When every stain was last looked over (`SWEEP`).
    swept: f32,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    report: f32,
    /// For the checks: where the last marks were made, and which way they face.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    last: VecDeque<[f32; 6]>,
}
impl Mess {
    fn budget(&self) -> Budget {
        self.budget.unwrap_or(Budget::of(Level::High))
    }
    /// An entity drawing `mesh` with `material` at `tr`: one from the pool if there is one.
    fn take(&mut self, world: &mut World, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, tr: Transform) -> Entity {
        while let Some(e) = self.pool.pop() {
            if let Ok(mut entity) = world.get_entity_mut(e) {
                entity.insert((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), tr, Visibility::Inherited));
                return e;
            }
        }
        world
            .spawn((Name::new("Condiment"), Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), tr, Visibility::Inherited, NotShadowCaster))
            .id()
    }
    /// Puts an entity back in the pool, hidden and loose.
    fn give(&mut self, world: &mut World, e: Entity) {
        let Ok(mut entity) = world.get_entity_mut(e) else {
            return;
        };
        entity.remove::<(ChildOf, Card, Along)>();
        entity.insert(Visibility::Hidden);
        if self.pool.len() < POOL {
            self.pool.push(e);
        } else {
            entity.despawn();
        }
    }
    fn drop_stain(&mut self, world: &mut World, stain: Stain) {
        for part in stain.parts {
            self.give(world, part.entity);
        }
    }
    fn mote(&mut self, world: &mut World, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Vec3, m: (MoteKind, Vec3, f32, Vec3, usize)) {
        if self.motes.len() >= self.budget().particles {
            return;
        }
        let (kind, velocity, life, size, sauce) = m;
        let card = matches!(kind, MoteKind::Ember | MoteKind::Smoke);
        let entity = self.take(world, mesh, material, Transform::from_translation(at).with_scale(size));
        if card {
            world.entity_mut(entity).insert(Card(hash(self.count.wrapping_add(entity.index_u32())) * TAU));
        }
        self.count = self.count.wrapping_add(1);
        let spin = Vec3::new(hash(self.count) - 0.5, hash(self.count + 7) - 0.5, hash(self.count + 13) - 0.5) * 18.;
        self.motes.push(Mote { entity, kind, velocity, age: 0., life, size, spin, sauce, landed: false });
    }
}

/// Builds what the effects are drawn with, the first time it is wanted.
fn look(world: &mut World) -> std::sync::Arc<Look> {
    if let Some(kit) = world.get_resource::<Kit>() {
        return kit.0.clone();
    }
    let started = bevy::platform::time::Instant::now();
    let look = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
        world.resource_scope(|world, mut mats: Mut<Assets<StandardMaterial>>| {
            let mut images = world.resource_mut::<Assets<Image>>();
            Look::new(&mut meshes, &mut mats, &mut images)
        })
    });
    info!("Condiment effects made in {:.0} ms", started.elapsed().as_secs_f32() * 1000.);
    let look = std::sync::Arc::new(look);
    world.insert_resource(Kit(look.clone()));
    world.init_resource::<Mess>();
    look
}
/// The shared `Look`, cheap to hand round.
#[derive(Resource, Clone)]
struct Kit(std::sync::Arc<Look>);
/// Makes the effects' meshes, materials and textures ahead of the first round, so firing never waits for them.
pub fn prepare(world: &mut World) {
    let look = look(world);
    boom::prepare(world, &look);
}

// ---------------------------------------------------------------------------------------------------------
// Rounds in flight.

/// The visible body of one round: a root that follows the simulation, and its parts.
pub struct Blob {
    pub root: Entity,
    parts: Vec<Entity>,
    /// A fast round's tracer (`TRACE`), if it has one.
    streak: Option<Entity>,
    pub weapon: usize,
    pub at: Vec3,
    pub seen: u32,
    /// First-person rounds leave the muzzle, then ease onto the true path from the eye.
    pub offset: Vec3,
    pub owner: u32,
    pub velocity: Vec3,
    /// How it is falling, from how its velocity changed (a droplet of mayonnaise arcs), and when last measured.
    bend: Vec3,
    last: Option<(Vec3, f32)>,
    /// Seconds to the next drip it sheds.
    shed: f32,
    seed: u32,
}
impl Blob {
    /// For the landing: the way it was going.
    pub fn heading(&self) -> Option<Vec3> {
        self.velocity.try_normalize()
    }
}
/// The roles of a round of flame's cards, as many as the budget allows.
#[derive(Clone, Copy, PartialEq)]
enum Flame {
    Core,
    Body,
    Outer,
    Smoke,
}
const FLAME: [Flame; 9] = [Flame::Core, Flame::Body, Flame::Outer, Flame::Smoke, Flame::Body, Flame::Core, Flame::Body, Flame::Outer, Flame::Smoke];
/// Drops in a stream by weapon, before the budget.
fn beads(weapon: usize) -> usize {
    match weapon {
        1 => 10,
        2 => 7,
        3 => 0,
        4 => 7,
        5 => 5,
        6 => 7,
        7 => 13,
        _ => 8,
    }
}
/// Builds the parts of one round. Mustard is a squirt of drops (`jet`), ketchup and mayonnaise a rope of them;
/// hot sauce is flame; the relish glob is a core with mayonnaise and relish chunks tumbling round it; bug spray
/// is a puff of mist; syrup is a glossy drop trailing a short strand.
pub fn spawn_round(world: &mut World, weapon: usize, owner: u32, id: u32, offset: Vec3) -> Blob {
    let look = look(world);
    let budget = world.resource::<Mess>().budget();
    let root = world.spawn((Name::new("Round"), Transform::default(), Visibility::default())).id();
    let sauce = Sauce::of(weapon);
    let tones = sauce.map(|s| look.tone[s.index()].clone());
    let mut parts = vec![];
    // The parts come from the pool, and go back to it when the round lands.
    let mut add = |world: &mut World, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, card: Option<f32>| {
        let e = world.resource_scope(|world, mut mess: Mut<Mess>| mess.take(world, mesh, material, Transform::from_scale(Vec3::ZERO)));
        if let Some(roll) = card {
            world.entity_mut(e).insert(Card(roll));
        }
        world.entity_mut(root).add_child(e);
        parts.push(e);
    };
    if weapon == 3 {
        for (k, role) in FLAME.iter().take(budget.flame).enumerate() {
            let (mesh, material) = match role {
                Flame::Core => (&look.card[PUFF], &look.fire[0]),
                Flame::Body | Flame::Outer => (&look.card[TONGUE], &look.fire[0]),
                Flame::Smoke => (&look.card[SMOKE], &look.smoke),
            };
            add(world, mesh, material, Some(hash(id * 31 + k as u32) * TAU));
        }
    } else {
        let count = ((beads(weapon) as f32 * budget.beads).round() as usize).max(if weapon == 4 { 7 } else { 3 });
        for i in 0..count {
            let thirds = (i * 3 / count).min(2);
            let (mesh, material) = match weapon {
                4 if i == 0 => (&look.ball, &look.tone[Sauce::Relish.index()][1]),
                4 if i < 3 => (&look.ball, &look.cream),
                4 => (&look.cube, &look.chunk[i % 2]),
                5 => (&look.ball, &look.mist),
                1 => (&look.ball, &look.jet),
                _ => (&look.ball, tones.as_ref().map_or(&look.mist, |t| &t[thirds])),
            };
            add(world, mesh, material, None);
        }
    }
    // A round that flies straight and fast leaves a tracer (not the flame, the mist or the mayonnaise's stream).
    let streak = matches!(weapon, 1 | 2 | 4 | 6).then(|| {
        let material = look.streak[sauce.map_or(0, |s| s.index())].clone();
        let e = world.resource_scope(|world, mut mess: Mut<Mess>| mess.take(world, &look.ball, &material, Transform::from_scale(Vec3::ZERO)));
        world.entity_mut(root).add_child(e);
        e
    });
    Blob { root, parts, streak, weapon, at: Vec3::ZERO, seen: 0, offset, owner, velocity: Vec3::ZERO, bend: Vec3::ZERO, last: None, shed: 0.03, seed: id }
}
/// A tracer `travelled` metres out of the muzzle at `speed`, in the round's frame (+z behind it): a streak from the round
/// back along its way TRACE seconds' worth (no further than the muzzle), thinning as it goes, a little thicker the faster.
pub fn streak(speed: f32, travelled: f32) -> Transform {
    let long = (speed * TRACE).min(travelled.max(0.));
    if long < 0.004 {
        return Transform::from_scale(Vec3::ZERO);
    }
    let thick = (0.0025 + 0.00012 * speed).min(0.008);
    Transform::from_translation(Vec3::Z * long * 0.5).with_scale(Vec3::new(thick, thick, long * 0.5))
}
/// Places a round and its parts for this frame. A stream is a wave fixed along the path, so it snakes rather
/// than wobbling in place; its drops are stretched along it so that they run together, swell and shrink as it
/// goes, and drips fall from it. The mustard is a squirt that breaks up behind its head (`jet`), and its drips
/// peel off the drops it has broken into.
pub fn pose_round(world: &mut World, blob: &mut Blob, velocity: Vec3, travelled: f32, now: f32, dt: f32) {
    let dir = velocity.normalize_or(Vec3::NEG_Z);
    let speed = velocity.length().max(0.05);
    // How it falls: measured from the change in its velocity.
    if let Some((before, at)) = blob.last {
        let gap = now - at;
        if gap > 0.004 {
            let seen = ((velocity - before) / gap).clamp_length_max(20.);
            blob.bend = blob.bend.lerp(seen, 0.5);
        }
    }
    blob.last = Some((velocity, now));
    blob.velocity = velocity;
    let muzzle = blob.offset * (1. - smooth(travelled / 0.3));
    let head = blob.at + muzzle;
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    if blob.weapon == 3 {
        if let Some(mut tr) = world.get_mut::<Transform>(blob.root) {
            *tr = Transform::from_translation(head);
        }
        let look = look(world);
        pose_flame(world, &look, blob, dir, travelled, now);
        // Embers fly from it.
        blob.shed -= dt;
        if blob.shed <= 0. && travelled > 0.04 {
            let k = blob.seed.wrapping_mul(31).wrapping_add((now * 1000.) as u32);
            world.resource_scope(|world, mut mess: Mut<Mess>| {
                let sparks = mess.budget().sparks;
                blob.shed = 0.07 / sparks.max(0.1) * (0.6 + 0.8 * hash(k));
                let v = dir * (0.3 + 0.3 * hash(k + 1)) + Vec3::new(hash(k + 2) - 0.5, hash(k + 3) * 0.8, hash(k + 4) - 0.5) * 0.35;
                mess.mote(world, &look.card[SPARK], &look.fire[3], head, (MoteKind::Ember, v, 0.35 + 0.5 * hash(k + 5), Vec3::splat(0.004 + 0.004 * hash(k + 6)), Sauce::Hot.index()));
            });
        }
        return;
    }
    let root = Transform::from_translation(head).looking_to(dir, up);
    if let Some(mut tr) = world.get_mut::<Transform>(blob.root) {
        *tr = root;
    }
    if let Some(e) = blob.streak {
        if let Some(mut tr) = world.get_mut::<Transform>(e) {
            *tr = streak(speed, travelled);
        }
    }
    let count = blob.parts.len() as f32;
    let fall = root.rotation.inverse() * blob.bend;
    // Swellings that run back along the rope.
    let wobble = |i: f32| 1. + 0.12 * (now * 30. - i * 0.75 + blob.seed as f32).sin();
    // The mustard pistol's squirt (`jet`): where each of its drops is this frame.
    let squirt = (blob.weapon == 1).then(|| jet(blob.parts.len(), travelled, now, blob.seed, root.rotation.inverse() * Vec3::NEG_Y, fall, speed));
    // Stream drops: spacing, radius at the head, how much thinner at the tail, squiggle width and waves per metre.
    let stream = match blob.weapon {
        2 => Some((0.016, 0.0048, 0.35, 0.0012, 70.)),
        7 => Some((0.0145, 0.0064, 0.3, 0.004, 40.)),
        w if w > 7 => Some((0.011, 0.006, 0.45, 0.008, 80.)),
        _ => None,
    };
    let place = |i: usize, along: f32, gap: f32, wide: f32, waves: f32| -> Vec3 {
        let n = i as f32;
        let grow = (along / 0.03).clamp(0., 1.);
        let lag = n * gap / speed;
        Vec3::new(wide * grow * (waves * along).sin(), wide * 0.5 * grow * (waves * 0.63 * along).cos(), n * gap) + fall * (0.5 * lag * lag)
    };
    for (i, part) in blob.parts.clone().into_iter().enumerate() {
        let n = i as f32;
        let tr = match (blob.weapon, stream) {
            (1, _) => squirt.as_ref().map_or(Transform::from_scale(Vec3::ZERO), |drops| drops[i]),
            (_, Some((gap, head_r, thin, wide, waves))) => {
                let along = travelled - n * gap;
                if along <= 0. {
                    Transform::from_scale(Vec3::ZERO)
                } else {
                    let at = place(i, along, gap, wide, waves);
                    let ahead = if i == 0 { at - Vec3::Z * gap } else { place(i - 1, along + gap, gap, wide, waves) };
                    let tangent = (at - ahead).normalize_or(Vec3::Z);
                    let r = head_r * (1. - thin * n / count) * wobble(n);
                    // Long enough to reach over the next drop and the one after: the stream is one rope.
                    let long = if i == 0 { r * 1.5 } else { (gap * 1.5).max(r) * if blob.weapon == 2 { 1.3 } else { 1. } };
                    Transform::from_translation(at).with_rotation(Quat::from_rotation_arc(Vec3::Z, tangent)).with_scale(Vec3::new(r, r * 0.92, long))
                }
            }
            (4, _) if i == 0 => {
                let w = 0.1 * (now * 18.).sin();
                Transform::from_scale(Vec3::new(1. + w, 1. - w, 1.) * 0.028)
            }
            (4, _) => {
                let angle = n * 1.047 + now * 5.;
                Transform::from_translation(Vec3::new(angle.cos(), angle.sin(), 0.3 * (angle * 2.).sin()) * 0.024)
                    .with_scale(Vec3::splat(if i < 3 { 0.011 } else { 0.008 }))
                    .with_rotation(Quat::from_rotation_y(now * 7. + n))
            }
            (5, _) => {
                let spread = |k: u32| crate::player_model::noise(i as u32 * 5 + k) - 0.5;
                Transform::from_translation(Vec3::new(spread(1), spread(2), 0.) * (0.008 + travelled * 0.08) + Vec3::Z * n * 0.02)
                    .with_scale(Vec3::splat(0.008 + travelled * 0.03))
            }
            _ => {
                // Syrup: a drop flying straight, and behind it a strand that thins, sags under its own weight
                // and wavers more the further back it is, its beads drawn out along the path so that they run
                // together.
                let (gap, wide, waves) = (0.0075, 0.0016, 45.);
                let radius = if i == 0 { 0.0048 } else { 0.0030 * (1. - 0.5 * n / count) };
                let stretch = if i == 0 { 1.4 } else { 2.6 };
                let along = travelled - n * gap;
                let grow = (along / 0.03).clamp(0., 1.);
                Transform::from_translation(Vec3::new(wide * grow * n / count * (waves * along).sin(), -0.00025 * n * n * grow, n * gap))
                    .with_scale(Vec3::new(radius, radius, radius * stretch) * if along > 0. { 1. } else { 0. })
            }
        };
        if let Some(mut t) = world.get_mut::<Transform>(part) {
            *t = tr;
        }
    }
    // Drips fall from a stream (and juice from a relish glob) as it flies.
    let shed = match blob.weapon {
        1 => 0.07,
        2 => 0.14,
        4 => 0.1,
        6 => 0.16,
        7 => 0.08,
        _ => 0.,
    };
    if shed > 0. && travelled > 0.05 {
        blob.shed -= dt;
        if blob.shed <= 0. {
            blob.shed = shed * (0.6 + 0.8 * hash(blob.seed.wrapping_mul(7).wrapping_add((now * 100.) as u32)));
            if let Some(sauce) = Sauce::of(blob.weapon) {
                let tail = ((count - 1.) * (0.4 + 0.6 * hash(blob.seed + (now * 1000.) as u32))).max(0.);
                // The mustard's drips peel off its drops, from the back half, where the jet has broken up.
                let from = match &squirt {
                    Some(drops) => {
                        let out = drops.iter().rposition(|d| d.scale.x > 0.).unwrap_or(0);
                        root.transform_point(drops[(tail.round() as usize).clamp(drops.len() / 2, drops.len() - 1).min(out)].translation)
                    }
                    None => root.transform_point(Vec3::Z * tail * 0.01),
                };
                let k = blob.seed.wrapping_add((now * 997.) as u32);
                let v = velocity * 0.35 + Vec3::new(hash(k) - 0.5, -0.2, hash(k + 1) - 0.5) * 0.15;
                let r = 0.0018 + 0.0016 * hash(k + 2);
                if squirt.is_some() {
                    // As wet as the squirt it peels off.
                    let look = look(world);
                    world.resource_scope(|world, mut mess: Mut<Mess>| {
                        mess.mote(world, &look.ball, &look.jet, from, (MoteKind::Drop, v, 0.9 + 0.6 * hash(from.x.to_bits()), Vec3::splat(r), sauce.index()));
                    });
                } else {
                    fling(world, sauce, from, v, r, 1);
                }
            }
        }
    }
}
/// The mustard pistol's round, drawn as a squirt of mustard: a jet that leaves the nozzle whole and, as it flies,
/// breaks up behind the way a real jet does. Behind a full head it tapers, then necks into beads joined by threads;
/// the threads pinch off into little satellite drops between the beads, and the drops fall back and droop under
/// their weight more the further back they are, each wobbling as a fresh drop does. The head is the round itself,
/// at the root, and goes straight where the round goes; nothing waves the body from side to side.
///
/// `count` drops, `travelled` metres out of the nozzle at `now`; `down` (the way down) and `fall` (how the round is
/// falling) are in the round's frame, whose +z is behind it, and `speed` is its speed. Gives each drop's place in the
/// round's frame, its long axis along the way it goes; a drop not yet out of the nozzle has no size.
fn jet(count: usize, travelled: f32, now: f32, seed: u32, down: Vec3, fall: Vec3, speed: f32) -> Vec<Transform> {
    /// The head's radius, and how much the tail droops: metres below the path at a metre behind the head.
    const HEAD: f32 = 0.0068;
    const SAG: f32 = 0.9;
    // Spaced so that the jet is as long whatever the graphics level's count of drops.
    let gap = 0.0084 / (count as f32 / 10.).max(0.5);
    // How far the break-up has come: none at the nozzle; 14 cm out, the back two-thirds are beads and threads
    // and the back half has come apart.
    let grown = smooth(travelled / 0.14);
    let split = 1. - 0.7 * grown;
    let last = (count.max(2) - 1) as f32;
    // Gravity across the path droops it; along the path it would only crowd the drops.
    let down = (down - Vec3::Z * down.z).normalize_or_zero();
    let jitter = |i: usize, k: u32| hash(seed.wrapping_mul(131).wrapping_add(i as u32 * 7 + k)) - 0.5;
    let mix = |a: (f32, f32), b: (f32, f32), t: f32| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    // First how far each part has broken (0 still the jet, 1 a bead or a thread, 2 a drop of its own; the head and
    // the tail it draws out behind it, the first two, never break), how far back it is, and where: on a path that
    // droops (and falls with the round, if it falls), those come apart straying a little off it.
    let mut z = 0.;
    let parts: Vec<(f32, f32, f32, Vec3, Vec3)> = (0..count)
        .map(|i| {
            let stage = if i < 2 { 0. } else { ((i as f32 / last - split) / 0.2 + 1.).clamp(0., 2.) };
            let (beaded, apart) = (smooth(stage), smooth(stage - 1.));
            if i > 0 {
                z += gap * (1. + 0.3 * beaded + 0.75 * apart);
            }
            let lag = z / speed;
            let weight = SAG * (0.3 + 0.7 * apart) * grown;
            let at = Vec3::Z * z + down * (weight * z * z) + fall * (0.5 * lag * lag) + Vec3::new(jitter(i, 1), jitter(i, 2), 0.) * (0.005 * apart);
            let tangent = (Vec3::Z + down * (2. * weight * z) + fall * (lag / speed)).normalize_or(Vec3::Z);
            (beaded, apart, z, at, tangent)
        })
        .collect();
    // Then each part's shape: its radius across and half-length along.
    let mut drops = Vec::with_capacity(count);
    for (i, &(beaded, apart, z, mut at, mut tangent)) in parts.iter().enumerate() {
        let (n, u) = (i as f32, i as f32 / last);
        let phase = n * 2.3 + (seed % 97) as f32 * 0.37;
        // The jet behind the head is much slimmer than it, and thins on back.
        let r = HEAD * (0.72 - 0.42 * u);
        // A fresh drop swings between long and squat.
        let swing = 0.15 * (now * 40. + phase * 1.7).sin();
        let (across, along) = if i == 0 {
            // The head: the blob the jet runs back from, quivering.
            let w = 0.06 * (now * 31. + phase).sin();
            (HEAD * (1. + w), HEAD * 1.15 * (1. - 1.6 * w))
        } else {
            // The jet: long drops, each reaching over two on either side (but never out of the front of the
            // head), run together into one smooth, tapering body.
            let jet = (r, (gap * 2.4).min(z + HEAD * 0.6));
            if i == 1 {
                jet
            } else if i % 2 == 0 {
                // A bead: it gathers the jet into a round drop, which goes on its own.
                let bead = r * (0.95 + 0.08 * jitter(i, 3));
                mix(mix(jet, (bead, bead * 1.1), beaded), (bead * (1. + swing), bead * (1. - 1.8 * swing)), apart)
            } else {
                // A thread from the bead before to the bead after; pinched off, it is a small drop between them.
                let before = parts[i - 1].3;
                let after = parts.get(i + 1).map_or(at + tangent * gap, |p| p.3);
                let span = after - before;
                at = at.lerp((before + after) * 0.5, beaded);
                tangent = tangent.lerp(span.normalize_or(tangent), beaded).normalize_or(Vec3::Z);
                let sat = r * (0.5 + 0.08 * jitter(i, 3));
                mix(mix(jet, (r * 0.38, span.length() * 0.5), beaded), (sat * (1. + 0.6 * swing), sat * 1.05 * (1. - swing)), apart)
            }
        };
        let out = ((travelled - z) / 0.004).clamp(0., 1.);
        drops.push(Transform::from_translation(at).with_rotation(Quat::from_rotation_arc(Vec3::Z, tangent)).with_scale(Vec3::new(across, across * 0.94, along) * out));
    }
    drops
}
/// A round of flame: cards turned to the camera, flickering. Each card is as hot as the flame is where it
/// is: white-yellow at the nozzle, then orange, red, and smoke at the end of its reach (`u`, how far through
/// its range it is). The cards are spread back along the path to the round behind, so that the rounds run
/// into one jet.
fn pose_flame(world: &mut World, look: &Look, blob: &Blob, dir: Vec3, travelled: f32, now: f32) {
    let range = feast::RANGE.get(3).copied().unwrap_or(0.65);
    let u = (travelled / range).clamp(0., 1.2);
    let frame = (now * 24.) as u32;
    for (k, part) in blob.parts.iter().enumerate() {
        let role = FLAME[k];
        let n = |j: u32| hash(blob.seed.wrapping_mul(53).wrapping_add(frame.wrapping_mul(17)).wrapping_add(k as u32 * 7 + j));
        let fixed = |j: u32| hash(blob.seed.wrapping_mul(91).wrapping_add(k as u32 * 13 + j));
        let flicker = 0.75 + 0.5 * n(0);
        let (at, size, material) = if role == Flame::Smoke {
            let back = (0.03 + 0.06 * fixed(0)).min(travelled);
            let size = (0.035 + 0.08 * u) * smooth((u - 0.45) / 0.3);
            (-dir * back + Vec3::Y * (0.07 * u * u) + Vec3::new(n(1) - 0.5, 0., n(3) - 0.5) * 0.006, size, &look.smoke)
        } else {
            // Spread back along the path, at most to where the round behind is (a tenth of a second).
            let back = (fixed(0) * 0.12).min(travelled);
            let heat = ((travelled - back) / range + 0.12 * (fixed(1) - 0.5)).max(0.);
            let size = (0.02 + 0.07 * heat.min(1.)) * flicker * (1. - smooth((heat - 0.9) / 0.25));
            let jitter = Vec3::new(n(1) - 0.5, n(2) - 0.5, n(3) - 0.5) * (0.006 + 0.03 * heat);
            let material = if heat < 0.18 { &look.fire[0] } else if heat < 0.48 { &look.fire[1] } else { &look.fire[2] };
            (-dir * back + Vec3::Y * (0.02 * heat * heat) + jitter, size, material)
        };
        let smoke = role == Flame::Smoke;
        if let Some(mut tr) = world.get_mut::<Transform>(*part) {
            tr.translation = at;
            // Fire is drawn out along its jet; smoke billows round.
            tr.scale = if smoke { Vec3::splat(size) } else { Vec3::new(size * 0.8, size * 1.7, size) };
        }
        if let Some(mut card) = world.get_mut::<Card>(*part) {
            card.0 = if smoke { fixed(2) * TAU + now * 0.8 } else { (n(4) - 0.5) * 0.35 };
        }
        if !smoke {
            if let Some(mut along) = world.get_mut::<Along>(*part) {
                along.0 = dir;
            } else {
                world.entity_mut(*part).insert(Along(dir));
            }
        }
        if world.get::<MeshMaterial3d<StandardMaterial>>(*part).is_some_and(|m| m.0 != *material) {
            world.entity_mut(*part).insert(MeshMaterial3d(material.clone()));
        }
    }
}
/// Takes a round's body away.
pub fn despawn_round(world: &mut World, blob: Blob) {
    if world.contains_resource::<Mess>() {
        world.resource_scope(|world, mut mess: Mut<Mess>| {
            for part in blob.parts.iter().chain(&blob.streak) {
                mess.give(world, *part);
            }
        });
    }
    if let Ok(e) = world.get_entity_mut(blob.root) {
        e.despawn();
    }
}

// ---------------------------------------------------------------------------------------------------------
// Landing.

/// A drop of sauce flung into the air.
fn fling(world: &mut World, sauce: Sauce, at: Vec3, velocity: Vec3, radius: f32, tone: usize) {
    let look = look(world);
    world.resource_scope(|world, mut mess: Mut<Mess>| {
        mess.mote(world, &look.ball, &look.tone[sauce.index()][tone.min(2)], at, (MoteKind::Drop, velocity, 0.9 + 0.6 * hash(at.x.to_bits()), Vec3::splat(radius), sauce.index()));
    });
}
/// Where a round of `weapon` ended (`p`), flying along `dir` when that is known: the splash and the mark it
/// leaves. `seed` tells one splat from another.
pub fn land(world: &mut World, g: &Game, weapon: usize, p: Vec3, dir: Option<Vec3>, seed: u32, now: f32) {
    let look = look(world);
    let Some(sauce) = Sauce::of(weapon) else {
        return;
    };
    let radius = feast::ROUND_RADIUS.get(weapon).copied().unwrap_or(0.012);
    let bodies = hotdogs(world);
    let middles: Vec<(u32, Vec3)> = bodies.iter().map(|b| (b.id, b.frame.transform_point(MIDDLE))).collect();
    let surface = classify(&g.feast, p, dir, radius, &middles);
    let n = |k: u32| hash(seed.wrapping_mul(97).wrapping_add(k));
    world.resource_scope(|world, mut mess: Mut<Mess>| {
        let budget = mess.budget();
        let ground = |x: f32, z: f32| under(&g.feast.level, x, p.y, z);
        // The way out of the surface, and the way the splash is thrown along it.
        let (normal, at) = match surface {
            Surface::Ground { at, normal } | Surface::Wall { at, normal } | Surface::Prop { at, normal, .. } => (normal, at),
            Surface::Hotdog { .. } => (dir.map_or(Vec3::Y, |d| -d), p),
            Surface::Air => (Vec3::Y, p),
        };
        if weapon == BAZOOKA {
            boom::burst(world, &mut mess, &look, g, p, dir, surface, seed, now);
            return;
        }
        // Hot sauce: a puff of smoke and a spit of embers; only some rounds leave a mark (there are many).
        if weapon == 3 {
            for k in 0..((4. * budget.sparks).round() as u32) {
                let v = (normal * 0.25 + Vec3::new(n(k) - 0.5, n(k + 9) * 0.6, n(k + 19) - 0.5) * 0.5) * (0.4 + 0.5 * n(k + 29));
                mess.mote(world, &look.card[SPARK], &look.fire[3], at + normal * 0.004, (MoteKind::Ember, v, 0.35 + 0.4 * n(k + 39), Vec3::splat(0.005 + 0.004 * n(k + 49)), sauce.index()));
            }
            mess.mote(world, &look.card[SMOKE], &look.smoke, at + normal * 0.01, (MoteKind::Smoke, normal * 0.03 + Vec3::Y * 0.05, 0.9, Vec3::splat(0.035), sauce.index()));
            if seed % 3 != 0 && !matches!(surface, Surface::Hotdog { .. }) || seed % 4 != 0 && matches!(surface, Surface::Hotdog { .. }) {
                return;
            }
        }
        // The splash: droplets thrown back off the surface and along it.
        let count = (match sauce {
            Sauce::Mustard => 5.,
            Sauce::Ketchup => 5.,
            Sauce::Hot => 2.,
            Sauce::Syrup => 3.,
            _ => 4.,
        } * budget.droplets)
            .round() as u32;
        let along = dir.map(|d| (d - normal * d.dot(normal)).normalize_or_zero()).unwrap_or(Vec3::ZERO);
        for k in 0..count {
            let angle = n(k * 5) * TAU;
            let (t1, t2) = basis(normal);
            let sideways = (t1 * angle.cos() + t2 * angle.sin()) * (0.3 + 0.6 * n(k * 5 + 1));
            let v = (normal * (0.25 + 0.6 * n(k * 5 + 2)) + sideways + along * 0.5) * (0.22 + 0.28 * n(k * 5 + 3)) * if sauce == Sauce::Syrup { 0.6 } else { 1. };
            let r = (0.0018 + 0.0022 * n(k * 5 + 4)) * heft(weapon).sqrt();
            mess.mote(world, &look.ball, &look.tone[sauce.index()][(k % 3) as usize], at + normal * 0.004, (MoteKind::Drop, v, 0.8 + 0.7 * n(k * 7), Vec3::splat(r), sauce.index()));
        }
        // How wide the mark is: by the gun, and wider on a wall than on the floor.
        let size = mark_width(weapon, normal) * (0.85 + 0.3 * n(90));
        match surface {
            Surface::Ground { at, normal } => {
                let marks = mark(world, &mut mess, &look, sauce, at, normal, dir, size, SPLATS[(seed % 4) as usize], None, seed, None);
                keep(world, &mut mess, sauce, marks, now);
            }
            Surface::Wall { at, normal } => {
                let at = on_the_face(&g.feast.level, at, normal, size);
                let marks = mark(world, &mut mess, &look, sauce, at, normal, dir, size, SPLATS[(seed % 4) as usize], None, seed, Some(&ground as &dyn Fn(f32, f32) -> f32));
                keep(world, &mut mess, sauce, marks, now);
            }
            Surface::Prop { id, at, normal } => {
                // On a loose object: it goes where the object goes (not a napkin, whose cloth folds).
                let host = world
                    .query::<(Entity, &crate::loose::Loose, &Transform)>()
                    .iter(world)
                    .find(|(_, loose, _)| loose.id == id)
                    .map(|(e, _, tr)| (e, *tr));
                let napkin = id < g.feast.props.len() && g.feast.props.body(id).shape == kitchen_sim::drape::NAPKIN;
                if let Some(host) = host.filter(|_| !napkin) {
                    let marks = mark(world, &mut mess, &look, sauce, at, normal, dir, size * 0.85, SPLATS[(seed % 4) as usize], Some(host), seed, None);
                    keep(world, &mut mess, sauce, marks, now);
                }
            }
            Surface::Hotdog { id } => {
                if let Some(body) = bodies.iter().find(|b| b.id == id) {
                    smear(world, &mut mess, &look, sauce, body, p, dir, seed);
                }
            }
            Surface::Air => {
                // Out of range in the open: it falls, a big drop and a few small ones.
                let d = dir.unwrap_or(Vec3::NEG_Y);
                mess.mote(world, &look.ball, &look.tone[sauce.index()][1], p, (MoteKind::Drop, d * 0.25, 1.6, Vec3::splat(radius * 0.45), sauce.index()));
            }
        }
    });
}
/// Two directions along a surface with this normal.
fn basis(normal: Vec3) -> (Vec3, Vec3) {
    let t1 = normal.any_orthonormal_vector();
    (t1, normal.cross(t1))
}
/// Where a mark of `size` wide on a block's face at `at` (facing `normal`) is centred so that it stays on the face: moved
/// in from an edge the face ends at, or the top it ends at, as far as the mark's body would hang over. (Marks are wide
/// now, and a block's face is only a hand across: a round that lands near its edge would leave half a splat in the air.)
/// The face is found with the level's own cover queries, probing along it from just outside.
fn on_the_face(level: &Ground, at: Vec3, normal: Vec3, size: f32) -> Vec3 {
    let reach = size * 0.32;
    let side = normal.cross(Vec3::Y).normalize_or_zero();
    if side == Vec3::ZERO {
        return at;
    }
    const OUT: f32 = 0.012;
    let mut shift = Vec3::ZERO;
    for dir in [side, -side, Vec3::Y] {
        // Whether the face goes on this far along `dir`: a ray back into it from just outside meets it there.
        let solid = |d: f32| {
            let t = level.cover_from(at + dir * d + normal * OUT, -normal);
            t > 0. && t < OUT + TOUCH * 2.
        };
        if solid(reach) {
            continue;
        }
        let (mut lo, mut hi) = (0., reach);
        for _ in 0..5 {
            let mid = (lo + hi) / 2.;
            if solid(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        shift -= dir * (reach - lo);
    }
    at + shift
}
/// Keeps a stain's parts as one stain, going oldest first beyond the budget: that many stains, or that many quads
/// (each part is one) among them all, whichever is reached first.
fn keep(world: &mut World, mess: &mut Mess, sauce: Sauce, parts: Vec<Part>, now: f32) {
    if parts.is_empty() {
        return;
    }
    let budget = mess.budget();
    let weight = parts.len();
    for old in mess.stains.add(now, Stain { sauce: sauce.index(), parts, step: 0, set: false }, weight, budget.stains, budget.quads) {
        mess.drop_stain(world, old);
    }
}
/// A mark of `sauce` `size` wide lying on a surface at `at` facing `normal`: drawn out the way the round was
/// going (`dir`) the more it glanced off, with drips running down a wall when `ground` says how far down
/// they can run. On a loose object (`host`, and where it stands) it is in the object's frame.
#[allow(clippy::too_many_arguments)]
fn mark(
    world: &mut World,
    mess: &mut Mess,
    look: &Look,
    sauce: Sauce,
    at: Vec3,
    normal: Vec3,
    dir: Option<Vec3>,
    size: f32,
    cell: usize,
    host: Option<(Entity, Transform)>,
    seed: u32,
    ground: Option<&dyn Fn(f32, f32) -> f32>,
) -> Vec<Part> {
    let n = |k: u32| hash(seed.wrapping_mul(193).wrapping_add(k));
    let normal = normal.normalize_or(Vec3::Y);
    // Along the surface, the way it was going; a round square on spreads evenly, so any way will do.
    let (glance, along) = match dir {
        Some(d) => {
            let flat = d - normal * d.dot(normal);
            (1. - d.dot(-normal).clamp(0., 1.), flat.try_normalize())
        }
        None => (0., None),
    };
    let along = along.unwrap_or_else(|| {
        let (t1, t2) = basis(normal);
        let a = n(1) * TAU;
        t1 * a.cos() + t2 * a.sin()
    });
    let stretch = 1. + 1.6 * glance.powf(1.5);
    let across = normal.cross(along);
    // The mark's x runs along, its y out of the surface, its z across.
    let turn = Quat::from_mat3(&Mat3::from_cols(along, normal, -across));
    mess.count = mess.count.wrapping_add(1);
    let lift = LIFT + LAYER * (mess.count % 24) as f32;
    let world_mark = Transform::from_translation(at + normal * lift).with_rotation(turn).with_scale(Vec3::new(size * stretch, 1., size));
    mess.last.push_back([at.x, at.y, at.z, normal.x, normal.y, normal.z]);
    if mess.last.len() > 8 {
        mess.last.pop_front();
    }
    let local = |tr: Transform| match host {
        Some((_, frame)) => Transform::from_matrix(frame.to_matrix().inverse() * tr.to_matrix()),
        None => tr,
    };
    let mut parts = vec![];
    let base = local(world_mark);
    let e = mess.take(world, &look.decal[cell], &look.stain[sauce.index()][0], Transform { scale: base.scale * 0.3, ..base });
    if let Some((host, _)) = host {
        world.entity_mut(host).add_child(e);
    }
    parts.push(Part { entity: e, kind: PartKind::Mark, base });
    // Drips down a wall: a few streaks, each its own length and thickness, which run for a moment and stop.
    if let Some(ground) = ground.filter(|_| normal.y.abs() < 0.5) {
        let down = (Vec3::NEG_Y - normal * -normal.y).normalize_or(Vec3::NEG_Y);
        let side = normal.cross(down);
        let (most, long, pace) = sauce.drips();
        let count = most.min(mess.budget().drips);
        // How long each runs against the longest: a short one, a long one, a middling one and the longest, which
        // goes to a different drip from one splat to the next.
        const LENGTH: [f32; 4] = [0.5, 1.0, 0.7, 1.45];
        for k in 0..count {
            // Not every one runs (but the first always does).
            if k > 0 && n(10 + k) < 0.2 {
                continue;
            }
            let across = ((k as f32 + 0.5) / count as f32 - 0.5) * 0.46 + (n(20 + k) - 0.5) * 0.1;
            let top = at + side * across * size + down * size * (0.06 + 0.08 * n(25 + k)) + normal * (lift - LAYER * 0.5);
            let room = (top.y - ground(top.x + normal.x * 0.01, top.z + normal.z * 0.01) - 0.004).max(0.);
            let length = (size * long * LENGTH[k.wrapping_add(seed) as usize % 4] * (0.8 + 0.4 * n(30 + k))).min(room);
            if length < 0.006 {
                continue;
            }
            let width = size * (0.34 + 0.22 * n(40 + k));
            let kind = PartKind::Drip { top, down, normal, length, width, delay: 0.08 + 0.35 * n(50 + k), pace: pace * (0.75 + 0.5 * n(60 + k)) };
            let base = drip_place(&kind, 0.);
            let e = mess.take(world, &look.decal[DRIP], &look.stain[sauce.index()][0], base);
            parts.push(Part { entity: e, kind, base });
        }
    }
    parts
}
/// Where a drip is drawn `t` seconds after its mark was made: running down fast and slowing to a stop at its
/// full length when `delay + pace` seconds are up (never later than `RUN`), and there for good.
fn drip_place(kind: &PartKind, t: f32) -> Transform {
    let PartKind::Drip { top, down, normal, length, width, delay, pace } = kind else {
        return Transform::IDENTITY;
    };
    let run = ((t - delay) / pace).clamp(0., 1.);
    let now = length * (1. - (1. - run).powi(3));
    if now < 0.002 {
        return Transform::from_translation(*top).with_scale(Vec3::ZERO);
    }
    // The drip cell runs down +z of the mesh: its x across, its y out of the wall.
    let across = normal.cross(*down);
    let turn = Quat::from_mat3(&Mat3::from_cols(across, *normal, *down));
    let shown = now / 0.85;
    Transform::from_translation(*top + *down * (shown * 0.5 - shown * 0.08)).with_rotation(turn).with_scale(Vec3::new(*width, 1., shown))
}

/// A hotdog as drawn: its number, its entity, its torso and the torso's frame in the world this frame.
struct Body {
    id: u32,
    torso: Entity,
    frame: Transform,
}
fn hotdogs(world: &mut World) -> Vec<Body> {
    let found: Vec<(u32, Entity, Transform, Entity)> = world
        .query_filtered::<(Entity, &crate::player_model::Gait, &crate::player_model::Rig, &Transform), With<crate::player_model::PlayerBody>>()
        .iter(world)
        .map(|(e, gait, rig, tr)| (gait.id(), e, *tr, rig.joints().torso))
        .collect();
    found
        .into_iter()
        .filter_map(|(id, _, root, torso)| {
            let local = *world.get::<Transform>(torso)?;
            Some(Body { id, torso, frame: root.mul_transform(local) })
        })
        .collect()
}
/// A smear of sauce on a hotdog where the round met its body: a mark bent round the sausage, a glob on it
/// and a drip hanging from it. It goes with the torso (alive or a ragdoll) until the hotdog is back.
#[allow(clippy::too_many_arguments)]
fn smear(world: &mut World, mess: &mut Mess, look: &Look, sauce: Sauce, body: &Body, p: Vec3, dir: Option<Vec3>, seed: u32) {
    let n = |k: u32| hash(seed.wrapping_mul(389).wrapping_add(k));
    let inverse = body.frame.to_matrix().inverse();
    let local_p = inverse.transform_point3(p);
    let hit = dir.and_then(|d| {
        let local_d = inverse.transform_vector3(d).normalize_or_zero();
        body_hit(local_p - local_d * 0.12, local_d)
    });
    let (point, normal) = hit.unwrap_or_else(|| body_near(local_p));
    let up = (Vec3::Y - normal * normal.y).normalize_or(Vec3::Z);
    let across = normal.cross(up);
    let turn = Quat::from_mat3(&Mat3::from_cols(across, normal, up));
    let scale = 1.0 + 0.3 * n(1);
    let mut parts = vec![];
    let tone = &look.tone[sauce.index()];
    let wrap = &look.wrap[(seed % 2) as usize];
    let e = mess.take(world, wrap, &look.stain[sauce.index()][0], Transform::from_translation(point + normal * 0.0007).with_rotation(turn * Quat::from_rotation_y(if n(2) < 0.5 { 0. } else { PI })).with_scale(Vec3::splat(scale)));
    parts.push(e);
    // A glob on it, and a drip hanging from it with a bead at its end.
    let glob = Transform::from_translation(point + normal * 0.0009 + up * (n(3) - 0.5) * 0.008)
        .with_rotation(turn)
        .with_scale(Vec3::new(0.0058, 0.0024, 0.0052) * scale);
    parts.push(mess.take(world, &look.ball, &tone[0], glob));
    let hang = point - up * (0.012 + 0.006 * n(4)) * scale + across * (n(5) - 0.5) * 0.01;
    parts.push(mess.take(world, &look.ball, &tone[1], Transform::from_translation(hang + normal * 0.0006).with_rotation(turn).with_scale(Vec3::new(0.0022, 0.0016, 0.0085) * scale)));
    parts.push(mess.take(world, &look.ball, &tone[0], Transform::from_translation(hang - up * 0.008 * scale + normal * 0.0011).with_scale(Vec3::splat(0.0027 * scale))));
    if sauce == Sauce::Relish {
        for k in 0..3 {
            let at = point + normal * 0.0016 + (across * (n(10 + k) - 0.5) + up * (n(20 + k) - 0.5)) * 0.016;
            parts.push(mess.take(world, &look.cube, &look.chunk[k as usize % 2], Transform::from_translation(at).with_rotation(turn * Quat::from_rotation_y(n(30 + k) * TAU)).with_scale(Vec3::new(0.005, 0.0025, 0.004))));
        }
    }
    for e in &parts {
        world.entity_mut(body.torso).add_child(*e);
    }
    let worn = mess.smears.entry(body.id).or_default();
    worn.push_back(parts);
    let mut gone = vec![];
    while worn.len() > SMEARS {
        gone.extend(worn.pop_front().unwrap_or_default());
    }
    for e in gone {
        mess.give(world, e);
    }
}
/// A light for `owner`'s flame (or, with None, a blast): its own if it has one, else a free one, else a
/// new one while the budget allows.
fn lamp(world: &mut World, mess: &mut Mess, owner: Option<u32>) -> Option<Entity> {
    // A change of theme or level takes the theme's lights away, these with them.
    mess.lamps.retain(|l| world.get_entity(l.entity).is_ok());
    if owner.is_some() {
        if let Some(l) = mess.lamps.iter().find(|l| l.owner == owner && l.left > 0.) {
            return Some(l.entity);
        }
    }
    if let Some(l) = mess.lamps.iter_mut().find(|l| l.left <= 0.) {
        l.owner = owner;
        l.left = 0.1;
        if let Ok(mut e) = world.get_entity_mut(l.entity) {
            e.insert(Visibility::Inherited);
        }
        return Some(l.entity);
    }
    if mess.lamps.len() >= mess.budget().lights {
        return None;
    }
    let entity = world
        .spawn((
            Name::new("Condiment light"),
            PointLight { color: Color::srgb(1., 0.55, 0.18), intensity: 0., range: 0.6, radius: 0.02, shadow_maps_enabled: false, ..default() },
            Transform::default(),
            Visibility::Inherited,
            // Not one of the kitchen's own lamps, which `visuals` dims to the theme and gives shadows to.
            crate::visuals::ThemeOwned,
        ))
        .id();
    mess.lamps.push(Lamp { entity, owner, left: 0.1 });
    Some(entity)
}

// ---------------------------------------------------------------------------------------------------------
// Every frame.

/// Moves what is in the air, lets stains run and dry, keeps smears until their hotdog is back, and lights
/// the flames. `now` is the real clock.
pub fn update(world: &mut World, g: &mut Game, now: f32, dt: f32) {
    let look = look(world);
    let level = world.get_resource::<crate::quality::Detail>().map_or(Level::High, |d| d.level);
    checks(world, g, now);
    world.resource_scope(|world, mut mess: Mut<Mess>| {
        let budget = Budget::of(level);
        if mess.budget != Some(budget) {
            mess.budget = Some(budget);
            // Fewer lights at a lower level: those beyond the budget go.
            while mess.lamps.len() > budget.lights {
                if let Some(Ok(e)) = mess.lamps.pop().map(|l| world.get_entity_mut(l.entity)) {
                    e.despawn();
                }
            }
        }
        // A hotdog that is back washes off its smears.
        for p in &g.roster {
            let was = mess.living.insert(p.id, p.alive);
            if p.alive && was == Some(false) {
                if let Some(worn) = mess.smears.remove(&p.id) {
                    for e in worn.into_iter().flatten() {
                        mess.give(world, e);
                    }
                }
            }
        }
        // Smears whose hotdog has gone (a new round, a new map) went with it.
        mess.smears.retain(|_, worn| {
            worn.retain(|parts| parts.iter().all(|e| world.get_entity(*e).is_ok()));
            !worn.is_empty()
        });
        motes(world, &mut mess, &look, g, now, dt);
        stains(world, &mut mess, &look, now);
        lights(world, &mut mess, g, now, dt);
        // The relish bursts under way (nothing, when there are none).
        boom::update(world, &mut mess, &look, g, now, dt);
        #[cfg(target_arch = "wasm32")]
        if now - mess.report > 0.5 {
            mess.report = now;
            let drips: Vec<[f32; 2]> = mess
                .stains
                .iter_mut()
                .flat_map(|(_, s)| s.parts.iter().filter(|p| matches!(p.kind, PartKind::Drip { .. })).map(|p| p.entity).collect::<Vec<_>>())
                .filter_map(|e| world.get::<Transform>(e).map(|t| [t.scale.x, t.scale.z]))
                .collect();
            let stats = serde_json::json!({
                "drips": drips,
                "stains": mess.stains.len(),
                "smears": mess.smears.values().map(|w| w.len()).sum::<usize>(),
                "motes": mess.motes.len(),
                "lights": mess.lamps.iter().filter(|l| l.left > 0.).count(),
                "pool": mess.pool.len(),
                "rounds": g.blobs.len(),
                "budget": format!("{:?}", budget),
                "last": mess.last,
                "bursts": world.get_resource::<boom::Booms>().map_or(0, |b| b.live()),
                "lens": world.get_resource::<boom::Booms>().map_or(0, |b| b.smudges()),
                "thrown": world.get_resource::<boom::Booms>().map_or(vec![], |b| b.last.clone()),
            });
            effects_report(&stats.to_string());
        }
    });
}
fn motes(world: &mut World, mess: &mut Mess, look: &Look, g: &Game, now: f32, dt: f32) {
    let level = &g.feast.level;
    let mut i = 0;
    let mut landed_chunks = vec![];
    while i < mess.motes.len() {
        let m = &mut mess.motes[i];
        m.age += dt;
        let t = (m.age / m.life).clamp(0., 1.);
        let mut tr = world.get::<Transform>(m.entity).copied().unwrap_or_default();
        match m.kind {
            MoteKind::Drop | MoteKind::Chunk => {
                if !m.landed {
                    m.velocity.y -= FALL * dt;
                    tr.translation += m.velocity * dt;
                    if m.kind == MoteKind::Chunk {
                        tr.rotation = Quat::from_scaled_axis(m.spin * m.age);
                    }
                    let ground = under(level, tr.translation.x, tr.translation.y, tr.translation.z);
                    if tr.translation.y < ground + 0.001 && m.velocity.y < 0. {
                        m.landed = true;
                        tr.translation.y = ground + 0.0008;
                        if m.kind == MoteKind::Chunk {
                            tr.rotation = Quat::from_rotation_y(m.spin.y);
                            tr.translation.y = ground + m.size.y * 0.35;
                            landed_chunks.push(i);
                        } else {
                            // A drop that lands is a little splat of its own for a few seconds.
                            let k = hash(m.entity.index_u32());
                            let normal = slope(level, tr.translation.x, tr.translation.z);
                            tr.rotation = Quat::from_rotation_arc(Vec3::Y, normal) * Quat::from_rotation_y(k * TAU);
                            tr.scale = Vec3::new(m.size.x * 5.5 * FLOOR_SCALE, 1., m.size.x * 5.5 * FLOOR_SCALE);
                            m.size = tr.scale;
                            m.life = m.age + 2.5 + 2. * k;
                            if let Ok(mut e) = world.get_entity_mut(m.entity) {
                                e.insert((Mesh3d(look.decal[if k < 0.5 { SPATTER } else { SPLATS[(k * 8.) as usize % 4] }].clone()), MeshMaterial3d(look.stain[m.sauce][0].clone())));
                            }
                        }
                    }
                } else if m.kind == MoteKind::Drop && m.life - m.age < 0.4 {
                    tr.scale = m.size * ((m.life - m.age) / 0.4).max(0.);
                }
            }
            MoteKind::Ember => {
                m.velocity.y -= 0.9 * dt;
                m.velocity *= (1. - dt * 1.5).max(0.);
                tr.translation += m.velocity * dt;
                tr.scale = m.size * (1. - t * 0.7) * (0.75 + 0.5 * hash((now * 30.) as u32 + m.entity.index_u32()));
            }
            MoteKind::Smoke => {
                tr.translation += m.velocity * dt;
                m.velocity *= (1. - dt * 0.8).max(0.);
                tr.scale = m.size * (1. + t * 1.8) * (1. - smooth((t - 0.6) / 0.4));
            }
        }
        if let Some(mut out) = world.get_mut::<Transform>(m.entity) {
            *out = tr;
        }
        i += 1;
    }
    // A chunk that has landed stays with the stains.
    for &k in landed_chunks.iter().rev() {
        let m = mess.motes.swap_remove(k);
        let base = world.get::<Transform>(m.entity).copied().unwrap_or_default();
        let budget = mess.budget();
        for old in mess.stains.add(now, Stain { sauce: m.sauce, parts: vec![Part { entity: m.entity, kind: PartKind::Lump, base }], step: 0, set: true }, 1, budget.stains, budget.quads) {
            mess.drop_stain(world, old);
        }
    }
    let mut k = 0;
    while k < mess.motes.len() {
        if mess.motes[k].age >= mess.motes[k].life {
            let m = mess.motes.swap_remove(k);
            mess.give(world, m.entity);
        } else {
            k += 1;
        }
    }
}
/// How often, in seconds, every stain is looked over for the look it should show and for what it stands on having gone.
const SWEEP: f32 = 0.5;
/// What every stain does each frame is almost nothing. The newest, which are spreading and running their drips, are
/// moved (and only until they have run for `RUN`: the book is in order of age, so the walk from the newest ends at the
/// first stain that is both old enough and already set); the oldest that are due to go go. Everything in between is not
/// looked at: a splat that has finished dripping costs no more per frame than a pixel of the floor. Twice a second
/// each one's look is checked, which is a few comparisons and a change of material only when it dries.
fn stains(world: &mut World, mess: &mut Mess, look: &Look, now: f32) {
    for old in mess.stains.expire(now) {
        mess.drop_stain(world, old);
    }
    for (born, stain) in mess.stains.iter_mut_newest() {
        let age = now - born;
        if age >= RUN && stain.set {
            break;
        }
        // Spreading, and the drips running (to their last place, if a stutter skipped past it).
        let age = age.min(RUN);
        for part in &stain.parts {
            let tr = match &part.kind {
                PartKind::Mark => {
                    // It lands small, spreads past its size and settles back to it.
                    let spread = 1. - (-age / 0.035).exp() * 0.7 + 0.1 * (age / 0.06) * (1. - age / 0.06).exp();
                    Transform { scale: Vec3::new(part.base.scale.x * spread, 1., part.base.scale.z * spread), ..part.base }
                }
                kind @ PartKind::Drip { .. } => drip_place(kind, age),
                PartKind::Lump => continue,
            };
            if let Some(mut t) = world.get_mut::<Transform>(part.entity) {
                *t = tr;
            }
        }
        stain.set = age >= RUN;
    }
    if now - mess.swept < SWEEP {
        return;
    }
    mess.swept = now;
    // Stains on something that has gone (a loose object of the last map) went with it.
    for old in mess.stains.drop_where(|s| s.parts.iter().all(|p| world.get_entity(p.entity).is_ok())) {
        mess.drop_stain(world, old);
    }
    for (born, stain) in mess.stains.iter_mut() {
        let step = finish(now - born);
        if step == stain.step {
            continue;
        }
        stain.step = step;
        for part in &stain.parts {
            match part.kind {
                PartKind::Lump => {
                    // (A lump stands up through the wet and the matte, and shrinks as the stain fades.)
                    if let Some(mut t) = world.get_mut::<Transform>(part.entity) {
                        t.scale = part.base.scale * (1. - step.saturating_sub(1) as f32 / (DRY + 1) as f32);
                    }
                }
                _ => {
                    if let Some(mut material) = world.get_mut::<MeshMaterial3d<StandardMaterial>>(part.entity) {
                        material.0 = look.stain[stain.sauce][step].clone();
                    }
                }
            }
        }
    }
}
/// The flame's light: one for each shooter whose flame is out, where most of it is, flickering; and the
/// blasts' flashes dying away.
fn lights(world: &mut World, mess: &mut Mess, g: &Game, now: f32, dt: f32) {
    let budget = mess.budget();
    let mut streams: HashMap<u32, (Vec3, f32)> = HashMap::new();
    if budget.lights > 0 {
        for blob in g.blobs.values().filter(|b| b.weapon == 3) {
            let entry = streams.entry(blob.owner).or_insert((Vec3::ZERO, 0.));
            entry.0 += blob.at;
            entry.1 += 1.;
        }
    }
    for lamp in &mut mess.lamps {
        lamp.left -= dt;
    }
    let mut owners: Vec<(u32, (Vec3, f32))> = streams.into_iter().collect();
    owners.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
    for (owner, (sum, count)) in owners.into_iter().take(budget.lights) {
        let Some(e) = lamp(world, mess, Some(owner)) else { break };
        let flicker = 0.75 + 0.5 * hash((now * 30.) as u32 ^ owner);
        if let Some(mut light) = world.get_mut::<PointLight>(e) {
            light.color = Color::srgb(1., 0.52, 0.16);
            light.intensity = 90. * (count / 3.).min(1.) * flicker;
            light.range = 0.55;
        }
        if let Some(mut tr) = world.get_mut::<Transform>(e) {
            tr.translation = sum / count + Vec3::Y * 0.02;
        }
        if let Some(l) = mess.lamps.iter_mut().find(|l| l.entity == e) {
            l.left = 0.08;
            l.owner = Some(owner);
        }
    }
    for lamp in &mess.lamps {
        if lamp.owner.is_none() && lamp.left > 0. {
            if let Some(mut light) = world.get_mut::<PointLight>(lamp.entity) {
                light.intensity = 140. * (lamp.left / 0.2).clamp(0., 1.).powi(2);
            }
        }
        if lamp.left <= 0. && world.get::<Visibility>(lamp.entity).is_some_and(|v| *v != Visibility::Hidden) {
            world.entity_mut(lamp.entity).insert(Visibility::Hidden);
        }
    }
}
/// A new round, a new map: everything goes.
pub fn clear(world: &mut World) {
    if !world.contains_resource::<Mess>() {
        return;
    }
    world.resource_scope(|world, mut mess: Mut<Mess>| {
        for stain in mess.stains.drain() {
            mess.drop_stain(world, stain);
        }
        let smears: Vec<Entity> = mess.smears.drain().flat_map(|(_, worn)| worn.into_iter().flatten()).collect();
        for e in smears {
            mess.give(world, e);
        }
        let motes: Vec<Entity> = mess.motes.drain(..).map(|m| m.entity).collect();
        for e in motes {
            mess.give(world, e);
        }
        mess.living.clear();
    });
    boom::clear(world);
}

/// Turns every card to face the camera, once everything has been placed and moved.
fn face(cameras: Query<&GlobalTransform, (With<crate::maya::HotdogCamera>, Without<Card>)>, mut cards: Query<(&Card, Option<&Along>, &mut Transform, &mut GlobalTransform)>) {
    let Some(eye) = cameras.iter().next() else {
        return;
    };
    let (_, turn, _) = eye.to_scale_rotation_translation();
    let back = turn.inverse();
    for (card, along, mut local, mut global) in &mut cards {
        // Drawn out along a way: its top turned to where that way points on screen.
        let lean = along.map(|a| back * a.0).filter(|d| d.x * d.x + d.y * d.y > 1e-6).map_or(0., |d| d.y.atan2(d.x) - PI / 2.);
        let rotation = turn * Quat::from_rotation_z(lean + card.0);
        // Cards hang from nothing turned, so the world's rotation is their own.
        local.rotation = rotation;
        let (scale, _, at) = global.to_scale_rotation_translation();
        *global = GlobalTransform::from(Transform { translation: at, rotation, scale });
    }
}
pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            face.after(bevy::transform::TransformSystems::Propagate).before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
    }
}

// ---------------------------------------------------------------------------------------------------------
// Checks only (`?check`): `window.hotdogGun = n` puts gun n in this player's hands with rounds to spare
// (offline); `window.hotdogSplat = [weapon, x, y, z, dx, dy, dz, ...]` lands rounds there, as if they had flown
// along (dx, dy, dz), without the simulation, and `window.hotdogSplatOn = [weapon, hotdog, angle, height, ...]`
// on a hotdog as it is drawn; `window.hotdogFollow = [hotdog, angle, distance, height, fov]` keeps the camera
// on one (through `window.hotdogView`); `window.hotdogEffects` counts what is out.

fn checks(world: &mut World, g: &mut Game, now: f32) {
    #[cfg(target_arch = "wasm32")]
    {
        let gun = effects_gun();
        if gun >= 1. && !g.network {
            arm(g, gun as usize);
        }
        let splats = effects_splats();
        for (k, s) in splats.chunks_exact(7).enumerate() {
            let dir = Vec3::new(s[4], s[5], s[6]);
            land(world, g, s[0] as usize, Vec3::new(s[1], s[2], s[3]), dir.try_normalize(), 7_000_000 + (now * 100.) as u32 + k as u32, now);
        }
        // Rounds landed on a hotdog as drawn, from the way given (radians round from its front) and this high.
        let on = effects_splats_on();
        let bodies = if on.is_empty() { vec![] } else { hotdogs(world) };
        for (k, s) in on.chunks_exact(4).enumerate() {
            if let Some(body) = bodies.iter().find(|b| b.id == s[1] as u32) {
                let from = body.frame.rotation * Vec3::new(s[2].sin(), 0., -s[2].cos());
                let p = body.frame.transform_point(MIDDLE + Vec3::Y * s[3]) + from * 0.067;
                land(world, g, s[0] as usize, p, Some(-from), 8_000_000 + (now * 100.) as u32 + k as u32, now);
            }
        }
        // How far a round fired level from this player's eye would fly, every 5 degrees of yaw from 0.
        if effects_probe() {
            if let Some(me) = g.feast.player(g.me()) {
                let eye = Vec3::from_array(me.p) + Vec3::Y * feast::EYE;
                let reach: Vec<f32> = (0..72).map(|k| (k as f32 * 5.).to_radians()).map(|yaw| g.feast.cover(eye, Vec3::new(-yaw.sin(), 0., -yaw.cos()))).collect();
                effects_reach(&reach);
            }
        }
        // The camera kept on a hotdog: [its number, the way round from its front, how far, how high, field of view].
        let follow = effects_follow();
        if follow.len() >= 5 {
            if let Some(body) = hotdogs(world).iter().find(|b| b.id == follow[0] as u32) {
                let (turn, at) = (body.frame.rotation, body.frame.translation);
                let yaw = Quat::from_rotation_y(turn.to_euler(EulerRot::YXZ).0);
                let target = at + Vec3::Y * 0.06;
                let eye = target + yaw * Vec3::new(follow[1].sin(), 0., -follow[1].cos()) * follow[2] + Vec3::Y * follow[3];
                effects_view(&[eye.x, eye.y, eye.z, target.x, target.y, target.z, follow[4]]);
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (world, g, now);
}
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn arm(g: &mut Game, gun: usize) {
    let Some(p) = g.feast.player_mut(1) else {
        return;
    };
    if gun >= p.owned.len() {
        return;
    }
    p.owned[gun] = true;
    p.ammo[gun] = p.ammo[gun].max(40);
    (p.weapon, p.gun) = (gun, gun);
    if g.weapon as usize != gun {
        g.weapon = Weapon::index(gun);
        g.gun = g.weapon;
    }
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
const checking=new URLSearchParams(location.search).has('check');
export function effects_gun(){const v=checking?Number(window.hotdogGun):NaN;return Number.isFinite(v)?v:-1}
export function effects_splats(){if(!checking)return new Float32Array(0);const v=window.hotdogSplat;window.hotdogSplat=null;const f=Array.isArray(v)?v.flat():[];return new Float32Array(f.length%7===0&&f.every(Number.isFinite)?f:[])}
export function effects_report(s){if(checking)window.hotdogEffects=JSON.parse(s)}
export function effects_splats_on(){if(!checking)return new Float32Array(0);const v=window.hotdogSplatOn;window.hotdogSplatOn=null;const f=Array.isArray(v)?v.flat():[];return new Float32Array(f.length%4===0&&f.every(Number.isFinite)?f:[])}
export function effects_follow(){const v=checking?window.hotdogFollow:null;return new Float32Array(Array.isArray(v)&&v.length>=5&&v.every(Number.isFinite)?v:[])}
export function effects_view(v){window.hotdogView=Array.from(v)}
export function effects_probe(){const v=checking&&window.hotdogProbe;window.hotdogProbe=null;return !!v}
export function effects_reach(v){window.hotdogReach=Array.from(v)}
")]
extern "C" {
    fn effects_gun() -> f32;
    fn effects_splats() -> Vec<f32>;
    fn effects_report(state: &str);
    fn effects_splats_on() -> Vec<f32>;
    fn effects_follow() -> Vec<f32>;
    fn effects_view(view: &[f32]);
    fn effects_probe() -> bool;
    fn effects_reach(reach: &[f32]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::map::Map;

    #[test]
    fn stains_go_oldest_first_beyond_the_cap() {
        let mut book = Book::default();
        let mut gone = vec![];
        for k in 0..205 {
            gone.extend(book.add(k as f32 * 0.01, k, 1, 200, usize::MAX));
        }
        assert_eq!(book.len(), 200);
        assert_eq!(gone, vec![0, 1, 2, 3, 4], "the five oldest went, in order");
        // A lower level's cap takes the oldest of what is left.
        let more = book.add(3., 999, 1, 110, usize::MAX);
        assert_eq!(more.len(), 91);
        assert_eq!(more[0], 5);
        assert_eq!(book.len(), 110);
    }

    #[test]
    fn the_cap_counts_quads_as_well_as_stains() {
        // Wall splats with their drips weigh several quads each; the cap on quads holds the cost down however
        // few stains there are.
        let budget = Budget::of(Level::High);
        let mut book = Book::default();
        let mut gone = vec![];
        for k in 0..130 {
            gone.extend(book.add(k as f32, k, 5, budget.stains, budget.quads));
        }
        assert_eq!(book.weight(), budget.quads, "130 five-quad splats are held to the quad cap: {}", book.weight());
        assert_eq!(book.len(), budget.quads / 5);
        assert!(book.len() < budget.stains, "the quad cap bites before the stain cap does");
        assert_eq!(gone, (0..130 - budget.quads / 5).collect::<Vec<_>>(), "the oldest went, in order");
        // Floor splats weigh one: the stain cap bites first.
        let mut floor = Book::default();
        for k in 0..400 {
            floor.add(k as f32, k, 1, budget.stains, budget.quads);
        }
        assert_eq!((floor.len(), floor.weight()), (budget.stains, budget.stains));
        // A splat heavier than the whole cap is still kept (the one just made is never the one to go), alone.
        let mut one = Book::default();
        assert!(one.add(0., 'a', 50, 10, 4).is_empty());
        assert_eq!((one.len(), one.weight()), (1, 50));
        assert_eq!(one.add(1., 'b', 2, 10, 4), vec!['a']);
        // Taking stains out by age or by what they stand on takes their weight with them.
        let mut book = Book::default();
        for k in 0..6 {
            book.add(k as f32 * 20., k, k as usize + 1, 100, 1000);
        }
        let old = book.expire(LIFE + 20.);
        assert_eq!(old, vec![0, 1], "those born at 0 and 20 are LIFE old by 110");
        assert_eq!(book.weight(), 3 + 4 + 5 + 6);
        assert_eq!(book.drop_where(|k| k % 2 == 0), vec![3, 5]);
        assert_eq!(book.weight(), 3 + 5);
        assert_eq!(book.drain().len(), 2);
        assert_eq!(book.weight(), 0);
        // Every level holds no more quads than the old cap's worst case (all stains with two drips) did.
        for level in Level::ALL {
            let b = Budget::of(level);
            assert!(b.quads <= b.stains * 3, "{level:?}: {} quads for {} stains", b.quads, b.stains);
            assert!(b.quads >= b.stains, "a quad for every stain at least");
        }
    }

    #[test]
    fn stains_dry_away_and_go_after_their_life() {
        let mut book = Book::default();
        book.add(0., 'a', 1, 200, usize::MAX);
        book.add(10., 'b', 1, 200, usize::MAX);
        assert!(book.expire(LIFE - 0.01).is_empty());
        assert_eq!(book.expire(LIFE), vec!['a']);
        assert_eq!(book.len(), 1);
        assert_eq!(book.expire(10. + LIFE + 1.), vec!['b']);
        // Wet at first, matte from WET on, then each fading step in turn, ending fully dried away.
        assert_eq!(finish(0.), 0);
        assert_eq!(finish(WET - 0.01), 0);
        assert_eq!(finish(WET), 1);
        assert_eq!(finish(LIFE - FADE - 0.01), 1);
        let steps: Vec<usize> = (0..=30).map(|k| finish(LIFE - FADE + FADE * k as f32 / 30.)).collect();
        assert!(steps.windows(2).all(|w| w[1] >= w[0]), "drying never goes back: {steps:?}");
        assert_eq!(steps[0], 2);
        assert_eq!(*steps.last().unwrap(), LOOKS - 1);
        assert_eq!(LOOKS, 2 + DRY);
        assert!(FINISH.windows(2).all(|w| w[1].cut >= w[0].cut), "each step cuts away more of the thin edge");
        assert_eq!(FINISH[0].cut, FINISH[1].cut, "wet and matte have the same outline");
        // Wet is smooth and shining, matte is not.
        assert!(FINISH[0].rough < 0.3 && FINISH[1].rough > 0.5, "wet, then matte");
        assert!(FINISH[0].reflectance > FINISH[1].reflectance);
        // A splat stays long enough to be seen, well over the half minute it once lasted.
        assert!(LIFE >= 60. && FADE >= 5.);
    }

    #[test]
    fn a_lower_level_draws_less() {
        let levels = Level::ALL.map(Budget::of);
        for w in levels.windows(2) {
            let (more, less) = (w[0], w[1]);
            assert!(less.stains <= more.stains && less.particles <= more.particles && less.lights <= more.lights);
            assert!(less.quads <= more.quads && less.drips <= more.drips);
            assert!(less.beads <= more.beads && less.droplets <= more.droplets && less.flame <= more.flame && less.sparks <= more.sparks);
        }
        assert_eq!(Budget::of(Level::High).stains, 200);
        assert_eq!(Budget::of(Level::Low).lights, 0, "no light from the flame at Low");
        assert_eq!(Budget::of(Level::Lowest).lights, 0);
        assert!(Budget::of(Level::High).flame <= FLAME.len());
    }

    fn siege() -> Feast {
        Feast::with_map(Map::bundled(), false)
    }

    #[test]
    fn a_round_on_the_counter_lands_on_the_ground() {
        let s = siege();
        let p = Vec3::new(0.1, feast::FLOOR, 3.0);
        match classify(&s, p, Some(Vec3::new(0.3, -1., 0.).normalize()), 0.012, &[]) {
            Surface::Ground { at, normal } => {
                assert!((at.y - feast::FLOOR).abs() < 1e-4);
                assert!(normal.y > 0.99);
            }
            other => panic!("{other:?}"),
        }
        // Without the way it came, too.
        assert!(matches!(classify(&s, p, None, 0.012, &[]), Surface::Ground { .. }));
    }

    #[test]
    fn a_round_against_a_block_lands_on_its_face_and_on_its_top() {
        let s = siege();
        // The steel block over x -0.45..-0.23, z 2.7..2.92, 0.25 high.
        let block = s.level.map.blocks.iter().find(|b| b.rect == [-0.45, 2.7, -0.23, 2.92]).expect("the bundled map's steel block");
        let top = feast::FLOOR + block.height as f32;
        let side = Vec3::new(-0.45, feast::FLOOR + 0.1, 2.8);
        match classify(&s, side, Some(Vec3::X), 0.012, &[]) {
            Surface::Wall { at, normal } => {
                assert!(normal.distance(Vec3::NEG_X) < 1e-4, "{normal}");
                assert!((at.x + 0.45).abs() < 1e-3);
            }
            other => panic!("{other:?}"),
        }
        // Its far side, flying the other way.
        match classify(&s, Vec3::new(-0.23, feast::FLOOR + 0.05, 2.8), Some(Vec3::NEG_X), 0.012, &[]) {
            Surface::Wall { normal, .. } => assert!(normal.distance(Vec3::X) < 1e-4),
            other => panic!("{other:?}"),
        }
        match classify(&s, Vec3::new(-0.34, top, 2.8), Some(Vec3::NEG_Y), 0.012, &[]) {
            Surface::Ground { at, normal } => {
                assert!(normal.y > 0.99);
                assert!((at.y - top).abs() < 1e-3);
            }
            other => panic!("{other:?}"),
        }
        // A round flying away from a face it is near did not hit it.
        assert!(!matches!(classify(&s, side - Vec3::X * 0.003, Some(Vec3::NEG_X), 0.012, &[]), Surface::Wall { .. }));
    }

    #[test]
    fn a_wide_mark_near_the_edge_of_a_block_is_kept_on_its_face() {
        let s = siege();
        // The steel block over x -0.45..-0.23, z 2.7..2.92, 0.25 high: its face towards -z.
        let top = feast::FLOOR + 0.25;
        let normal = Vec3::NEG_Z;
        let size = mark_width(PISTOL, normal);
        let reach = size * 0.32;
        let on = |x: f32, y: f32| on_the_face(&s.level, Vec3::new(x, y, 2.7), normal, size);
        // In the middle it stays where it is.
        let middle = Vec3::new(-0.34, feast::FLOOR + 0.12, 2.7);
        assert!(on(middle.x, middle.y).distance(middle) < 1e-5);
        // Near either edge it is moved in, as far as the width needs, along the face and no further.
        let (left, right) = (on(-0.44, feast::FLOOR + 0.12), on(-0.24, feast::FLOOR + 0.12));
        assert!((left.x - (-0.45 + reach)).abs() < 0.003, "{left}");
        assert!((right.x - (-0.23 - reach)).abs() < 0.003, "{right}");
        assert!(left.y == feast::FLOOR + 0.12 && left.z == 2.7, "only along the face: {left}");
        // And down from the top.
        let high = on(-0.34, top - 0.005);
        assert!((high.y - (top - reach)).abs() < 0.003, "{high}");
        assert!(high.x == -0.34);
        // Off its corner entirely (it cannot be told from where it is), nothing is asked of a face that is not there.
        let corner = on(-0.46, feast::FLOOR + 0.12);
        assert!(corner.x > -0.46, "{corner}");
        // A heavier gun's wider mark is moved in further.
        let wide = on_the_face(&s.level, Vec3::new(-0.44, feast::FLOOR + 0.12, 2.7), normal, mark_width(BAZOOKA, normal));
        assert!(wide.x > left.x);
    }

    #[test]
    fn a_round_in_the_open_hits_a_hotdog_there_or_nothing() {
        let s = siege();
        let p = Vec3::new(0.0, feast::FLOOR + 0.1, 2.5);
        assert_eq!(classify(&s, p, Some(Vec3::Z), 0.012, &[]), Surface::Air);
        let bodies = [(3, p + Vec3::new(0., 0., 0.07)), (4, p + Vec3::new(0.5, 0., 0.))];
        assert_eq!(classify(&s, p, Some(Vec3::Z), 0.012, &bodies), Surface::Hotdog { id: 3 });
        // A surface beats a hotdog that happens to stand by it.
        let floor = Vec3::new(0.0, feast::FLOOR, 2.5);
        assert!(matches!(classify(&s, floor, Some(Vec3::NEG_Y), 0.012, &[(5, floor + Vec3::Y * 0.09)]), Surface::Ground { .. }));
    }

    #[test]
    fn a_round_on_a_loose_object_lands_on_it() {
        let s = siege();
        // The bowl, from straight above.
        let id = (0..s.props.len()).find(|id| s.props.body(*id).shape != kitchen_sim::drape::NAPKIN).expect("a loose object");
        let [x, y, z] = s.props.position(id);
        let above = Vec3::new(x as f32, y as f32 + 0.5, z as f32);
        let hit = s.props.raycast(above.as_dvec3().to_array(), [0., -1., 0.], 1.0, None).expect("the ray from above meets it");
        let p = Vec3::new(hit.point[0] as f32, hit.point[1] as f32, hit.point[2] as f32);
        match classify(&s, p, Some(Vec3::NEG_Y), 0.012, &[]) {
            Surface::Prop { id: found, normal, .. } => {
                assert_eq!(found, id);
                assert!(normal.y > 0.5, "{normal}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn under_a_deck_the_ground_is_the_counter() {
        let s = Feast::with_map(Map::parse(include_str!("../../assets/maps/counter-siege-v2.json")).unwrap(), false);
        // The metal deck over x 0.8..1.3, z 2.74..3.14, 0.28 high.
        let (x, z) = (1.0, 2.9);
        let top = s.level.ground_at(x, z);
        assert!((top - (feast::FLOOR + 0.28)).abs() < 1e-3);
        assert_eq!(under(&s.level, x, top, z), top, "on the deck it is the deck");
        assert_eq!(under(&s.level, x, top + 0.1, z), top, "over it too");
        assert_eq!(under(&s.level, x, feast::FLOOR + 0.1, z), feast::FLOOR, "beneath it, the counter");
        assert_eq!(under(&s.level, 0.0, feast::FLOOR + 0.1, 2.5), feast::FLOOR);
    }

    #[test]
    fn a_round_meets_a_hotdogs_body_on_its_skin() {
        // Square on from the front (-z): the sausage's front.
        let (point, normal) = body_hit(Vec3::new(0., 0.1, -0.3), Vec3::Z).expect("hits the front");
        assert!((point.z + SAUSAGE.2).abs() < 1e-4 && (point.y - 0.1).abs() < 1e-4, "{point}");
        assert!(normal.distance(Vec3::NEG_Z) < 1e-3);
        // From the side: the bun, which stands out further than the sausage.
        let (point, normal) = body_hit(Vec3::new(0.3, 0.1, 0.012), Vec3::NEG_X).expect("hits the side");
        assert!((point.x - (BUN[1].0.x + BUN[1].1.x)).abs() < 1e-3, "{point}");
        assert!(normal.x > 0.9);
        // Over its head: the sausage's rounded top.
        let (point, normal) = body_hit(Vec3::new(0., 0.4, 0.), Vec3::NEG_Y).expect("hits the top");
        assert!((point.y - (SAUSAGE.1 + SAUSAGE.2)).abs() < 1e-3 && normal.y > 0.99);
        // Missing it altogether.
        assert!(body_hit(Vec3::new(0.2, 0.1, -0.3), Vec3::Z).is_none());
        // Where a smear goes when the way is not known: on the sausage, facing out.
        let (point, normal) = body_near(Vec3::new(0.05, 0.12, -0.05));
        assert!((Vec2::new(point.x, point.z).length() - SAUSAGE.2).abs() < 1e-5);
        assert!(normal.dot(Vec3::new(1., 0., -1.).normalize()) > 0.99);
    }

    #[test]
    fn a_mark_on_a_wall_runs_down_in_drips_and_one_on_the_floor_does_not() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<Image>>();
        let look = look(&mut world);
        let ground = |_: f32, _: f32| feast::FLOOR;
        let mut drips = 0;
        let mut lengths = vec![];
        for seed in 0..20 {
            let mut mess = Mess::default();
            let at = Vec3::new(0., feast::FLOOR + 0.13, 0.);
            let parts = mark(&mut world, &mut mess, &look, Sauce::Mustard, at, Vec3::NEG_X, Some(Vec3::X), 0.11, SPLATS[0], None, seed, Some(&ground as &dyn Fn(f32, f32) -> f32));
            let mut here = 0;
            for part in &parts {
                if let kind @ PartKind::Drip { length, down, delay, pace, .. } = &part.kind {
                    drips += 1;
                    here += 1;
                    lengths.push(*length);
                    assert!(*length > 0.006 && down.y < -0.99);
                    // It runs for a second or two at most and then stops at its full length, for good.
                    assert!(delay + pace <= RUN, "{delay} + {pace}");
                    let end = drip_place(kind, delay + pace);
                    assert!(end.scale.z * 0.85 - length < 1e-5 && end.scale.z * 0.85 - length > -1e-5, "the drip has run its length: {end:?}");
                    for t in [delay + pace + 0.01, RUN, RUN + 1., 50., LIFE] {
                        assert_eq!(drip_place(kind, t), end, "stopped at {t}");
                    }
                    // Growing, never shrinking, and slowing as it nears its end.
                    let steps: Vec<f32> = (0..=20).map(|k| drip_place(kind, delay + pace * k as f32 / 20.).scale.z).collect();
                    assert!(steps.windows(2).all(|w| w[1] >= w[0]), "{steps:?}");
                    assert!(steps[20] - steps[19] < steps[2] - steps[1] || steps[2] == 0., "slowing: {steps:?}");
                    // It hangs below where it starts, and what shows of it never goes through the counter (the drip's
                    // streak and bead end 0.41 of its quad's length below its middle).
                    assert!(end.translation.y - end.scale.z * 0.41 > feast::FLOOR - 1e-3, "{end:?}");
                }
            }
            assert!(here <= 3, "a mustard splat runs three drips at most: {here}");
            let floor = mark(&mut world, &mut mess, &look, Sauce::Mustard, Vec3::new(0., feast::FLOOR, 0.), Vec3::Y, Some(Vec3::NEG_Y), 0.075, SPLATS[0], None, seed, Some(&ground as &dyn Fn(f32, f32) -> f32));
            assert!(floor.iter().all(|p| matches!(p.kind, PartKind::Mark)));
        }
        assert!(drips >= 40, "a wall splat runs a few drips: {drips} drips from 20 marks");
        // Of different lengths (one splat's, and from one to the next).
        let (short, long) = (lengths.iter().cloned().fold(f32::MAX, f32::min), lengths.iter().cloned().fold(0., f32::max));
        assert!(long > short * 2., "streaks of varying length: {short} to {long}");
    }

    #[test]
    fn a_lower_level_runs_fewer_drips() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<Image>>();
        let look = look(&mut world);
        let ground = |_: f32, _: f32| feast::FLOOR;
        for level in Level::ALL {
            let mut mess = Mess::default();
            mess.budget = Some(Budget::of(level));
            let mut most = 0;
            for seed in 0..30 {
                let at = Vec3::new(0., feast::FLOOR + 0.2, 0.);
                let parts = mark(&mut world, &mut mess, &look, Sauce::Ketchup, at, Vec3::NEG_X, Some(Vec3::X), 0.1, SPLATS[0], None, seed, Some(&ground as &dyn Fn(f32, f32) -> f32));
                most = most.max(parts.iter().filter(|p| matches!(p.kind, PartKind::Drip { .. })).count());
            }
            assert_eq!(most as u32, Budget::of(level).drips.min(Sauce::Ketchup.drips().0), "{level:?}");
        }
    }

    #[test]
    fn heavier_guns_leave_bigger_splats() {
        let (wall, floor) = (Vec3::NEG_X, Vec3::Y);
        let width = |weapon: usize, normal: Vec3| mark_width(weapon, normal);
        // The mustard pistol is the measure: on a wall a splat is 2.25 times the width it was (5 cm), on the floor 1.5.
        assert!((width(PISTOL, wall) - 0.05 * 2.25).abs() < 1e-6, "{}", width(PISTOL, wall));
        assert!((width(PISTOL, floor) - 0.05 * 1.5).abs() < 1e-6, "{}", width(PISTOL, floor));
        assert!((2.0..=2.5).contains(&WALL_SCALE), "roughly two to two and a half times on walls");
        assert!((FLOOR_SCALE - 1.5).abs() < 1e-6 && FLOOR_SCALE < WALL_SCALE, "a little less on the floor");
        assert_eq!(heft(PISTOL), 1.);
        // The heavy guns, by what they do: the bazooka and the sniper mark wider than the pistol, on walls and floors.
        let (sniper, rifle) = (feast::SNIPER, feast::RIFLE);
        for normal in [wall, floor, Vec3::new(0.6, 0.8, 0.).normalize()] {
            assert!(width(sniper, normal) > width(PISTOL, normal) * 1.3, "the sniper's round marks wider than the pistol's: {normal}");
            assert!(width(BAZOOKA, normal) > width(PISTOL, normal) * 1.3, "so does the bazooka's: {normal}");
            assert!(width(BAZOOKA, normal) >= width(sniper, normal), "and the bazooka's the widest: {normal}");
            assert!(width(rifle, normal) < width(PISTOL, normal), "the light rifle's is smaller: {normal}");
        }
        // Heavier damage never marks smaller, gun by gun.
        let mut by_damage: Vec<usize> = Sauce::ALL.iter().map(|s| (0..feast::GUNS).find(|w| Sauce::of(*w) == Some(*s)).unwrap()).collect();
        by_damage.sort_by(|a, b| feast::PLAYER_DAMAGE[*a].total_cmp(&feast::PLAYER_DAMAGE[*b]));
        assert!(by_damage.windows(2).all(|w| heft(w[0]) <= heft(w[1])), "{by_damage:?}");
        // Every gun that squirts something leaves a mark, wider on a wall than on the floor, in a sensible range
        // (the bug spray, which is a mist, leaves none).
        for weapon in 0..feast::GUNS {
            let (on_wall, on_floor) = (width(weapon, wall), width(weapon, floor));
            if Sauce::of(weapon).is_none() {
                assert_eq!((on_wall, on_floor), (0., 0.), "gun {weapon}");
                continue;
            }
            assert!(on_wall > on_floor, "gun {weapon}: wider on the wall");
            assert!((on_wall / on_floor - WALL_SCALE / FLOOR_SCALE).abs() < 0.05 || weapon == BAZOOKA, "gun {weapon}");
            assert!((0.04..0.3).contains(&on_wall) && (0.03..0.2).contains(&on_floor), "gun {weapon}: {on_wall} {on_floor}");
        }
        // The old widths, for the weapons whose marks were not heavier: the splat is the factor bigger, not different.
        let old = [(feast::RIFLE, 0.034), (7, 0.046), (3, 0.026), (PISTOL, 0.05)];
        for (weapon, was) in old {
            assert!((width(weapon, wall) / was - WALL_SCALE).abs() < 0.2, "gun {weapon}: {}", width(weapon, wall) / was);
            assert!((width(weapon, floor) / was - FLOOR_SCALE).abs() < 0.15, "gun {weapon}: {}", width(weapon, floor) / was);
        }
        // A slope is between.
        let slope = Vec3::new(0.5, 0.5, 0.).normalize();
        assert!(width(PISTOL, slope) < width(PISTOL, wall) && width(PISTOL, slope) > width(PISTOL, floor));
        assert!(surface_scale(Vec3::Y) == FLOOR_SCALE && surface_scale(Vec3::NEG_Y) == FLOOR_SCALE && surface_scale(Vec3::Z) == WALL_SCALE);
    }

    #[test]
    fn a_splat_that_has_stopped_dripping_is_left_alone_each_frame() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<Image>>();
        let look = look(&mut world);
        let ground = |_: f32, _: f32| feast::FLOOR;
        let mut mess = Mess::default();
        let mut stained = vec![];
        // Three splats on a wall, born one a second apart.
        for k in 0..3u32 {
            let at = Vec3::new(0., feast::FLOOR + 0.2, 0.01 * k as f32);
            let parts = mark(&mut world, &mut mess, &look, Sauce::Mustard, at, Vec3::NEG_X, Some(Vec3::X), 0.11, SPLATS[0], None, k, Some(&ground as &dyn Fn(f32, f32) -> f32));
            stained.push(parts.iter().map(|p| p.entity).collect::<Vec<_>>());
            keep(&mut world, &mut mess, Sauce::Mustard, parts, k as f32);
        }
        // Early on they are moving: the newest has not yet spread to its size.
        stains(&mut world, &mut mess, &look, 2.01);
        assert!(!mess.stains.iter_mut().all(|(_, s)| s.set), "the newest is still running at 0.01 s");
        let tr = |world: &World, e: Entity| *world.get::<Transform>(e).unwrap();
        // Once run for RUN, each is set at the transforms its drips end at, whenever the stutter lands.
        stains(&mut world, &mut mess, &look, 2. + RUN + 0.2);
        assert!(mess.stains.iter_mut().all(|(_, s)| s.set), "all are set");
        let finished: Vec<Vec<Transform>> = stained.iter().map(|es| es.iter().map(|e| tr(&world, *e)).collect()).collect();
        for (k, parts) in stained.iter().enumerate() {
            let drips = mess.stains.iter_mut().nth(k).unwrap().1.parts.iter().enumerate().filter(|(_, p)| matches!(p.kind, PartKind::Drip { .. })).map(|(i, _)| i).collect::<Vec<_>>();
            assert!(!drips.is_empty());
            for i in drips {
                let kind = &mess.stains.iter_mut().nth(k).unwrap().1.parts[i].kind;
                assert_eq!(finished[k][i], drip_place(kind, RUN));
            }
            assert_eq!(parts.len(), finished[k].len());
        }
        // From then on a frame touches none of them: write a mark on each and see it survive many frames, while the
        // next stain being made is the only thing looked at.
        let marker = Vec3::new(9., 9., 9.);
        for es in &stained {
            for e in es {
                world.get_mut::<Transform>(*e).unwrap().translation = marker;
            }
        }
        for frame in 0..600 {
            stains(&mut world, &mut mess, &look, 5.5 + frame as f32 / 60.);
        }
        for es in &stained {
            for e in es {
                assert_eq!(tr(&world, *e).translation, marker, "a set splat is not moved again");
            }
        }
        // A new splat is looked at, and the old ones still are not.
        let parts = mark(&mut world, &mut mess, &look, Sauce::Mustard, Vec3::new(0., feast::FLOOR + 0.2, 0.2), Vec3::NEG_X, Some(Vec3::X), 0.11, SPLATS[1], None, 9, Some(&ground as &dyn Fn(f32, f32) -> f32));
        let newest = parts[0].entity;
        let before = tr(&world, newest);
        keep(&mut world, &mut mess, Sauce::Mustard, parts, 15.5);
        stains(&mut world, &mut mess, &look, 15.55);
        assert_ne!(tr(&world, newest), before, "the new one spreads");
        assert!(stained.iter().flatten().all(|e| tr(&world, *e).translation == marker));
        // They dried (the matte look after WET seconds) in the sweeps, not a frame, and the new one is wet.
        let wet = look.stain[Sauce::Mustard.index()][0].clone();
        let matte = look.stain[Sauce::Mustard.index()][1].clone();
        for es in &stained {
            assert!(es.iter().all(|e| world.get::<MeshMaterial3d<StandardMaterial>>(*e).is_some_and(|m| m.0 == matte)), "dry after {WET} s");
        }
        assert!(world.get::<MeshMaterial3d<StandardMaterial>>(newest).is_some_and(|m| m.0 == wet), "the new one is wet");
        // The wet material is shinier than the matte one, and they share one texture (one mesh, one material per look).
        let materials = world.resource::<Assets<StandardMaterial>>();
        let (w, m) = (materials.get(&wet).unwrap(), materials.get(&matte).unwrap());
        assert!(w.perceptual_roughness < m.perceptual_roughness && w.reflectance > m.reflectance);
        assert_eq!(w.base_color_texture, m.base_color_texture);
        assert_eq!(w.normal_map_texture, m.normal_map_texture);
        // They go after LIFE.
        stains(&mut world, &mut mess, &look, 2. + LIFE + 1.);
        assert_eq!(mess.stains.len(), 1, "the three old ones went, the new one is still here");
        stains(&mut world, &mut mess, &look, 15.5 + LIFE + 1.);
        assert_eq!(mess.stains.len(), 0);
    }

    #[test]
    fn the_textures_are_whole_and_their_cells_keep_apart() {
        let field = stain_field();
        // Every cell has sauce in its middle and none at its border.
        for cell in 0..COLS * ROWS {
            let (cx, cy) = ((cell % COLS) * CELL, (cell / COLS) * CELL);
            let at = |x: usize, y: usize| field.f[(cy + y) * WIDE + cx + x];
            assert!((0..CELL).any(|k| at(CELL / 2, k) > 0.42 || at(k, CELL / 2) > 0.42), "cell {cell} is empty");
            for k in 0..CELL {
                for v in [at(k, 0), at(k, CELL - 1), at(0, k), at(CELL - 1, k)] {
                    assert!(coverage(v) < FINISH[0].cut, "cell {cell} reaches its border");
                }
            }
        }
        let colours = colour_atlas(&field, Sauce::Ketchup);
        let (data, levels) = mips(colours, WIDE, HIGH, true, false);
        assert_eq!(levels, 10);
        let expected: usize = (0..levels).map(|l| (WIDE >> l).max(1) * (HIGH >> l).max(1) * 4).sum();
        assert_eq!(data.len(), expected);
        // Thick ketchup is darker than thin, and stays red.
        let [thick, body, thin] = Sauce::Ketchup.tones();
        assert!(thick[0] < body[0] && body[0] < thin[0]);
        assert!(Sauce::ALL.iter().all(|s| s.tones()[0].iter().sum::<f32>() < s.tones()[2].iter().sum::<f32>()));
    }

    /// The mustard squirt's drops for each graphics level's count of them.
    fn squirt_counts() -> Vec<usize> {
        Level::ALL.iter().map(|level| ((beads(1) as f32 * Budget::of(*level).beads).round() as usize).max(3)).collect()
    }
    /// Whether two drops of a squirt, one behind the other, touch: their reach along the path (each one's half-length)
    /// spans the distance between them.
    fn touching(a: &Transform, b: &Transform) -> bool {
        a.translation.distance(b.translation) < a.scale.z + b.scale.z
    }

    #[test]
    fn the_mustard_squirt_leaves_whole_and_breaks_into_beads_behind_its_head() {
        let level = Vec3::NEG_Y;
        for count in squirt_counts() {
            for seed in [1, 7, 40, 999] {
                for now in [0.5, 3.3, 17.25] {
                    // Just out of the nozzle it is one jet: what is out touches the drop before it, and what is not
                    // out yet has no size.
                    let fresh = jet(count, 0.03, now, seed, level, Vec3::ZERO, 1.5);
                    assert_eq!(fresh.len(), count);
                    assert_eq!(fresh[0].translation, Vec3::ZERO, "the head is the round");
                    let out: Vec<&Transform> = fresh.iter().filter(|d| d.scale.z > 0.).collect();
                    assert!(out.len() >= 2, "{count} drops: {fresh:?}");
                    assert!(out.windows(2).all(|w| touching(w[0], w[1])), "a whole jet at the nozzle: {fresh:?}");
                    assert!(fresh.iter().all(|d| d.scale.z > 0. || d.translation.z > 0.03 - 1e-4), "nothing shows that has not left the nozzle");
                    // Well out, the head is still the round, and the head and the tail it draws out are whole...
                    let flying = jet(count, 0.5, now, seed, level, Vec3::ZERO, 1.5);
                    assert_eq!(flying[0].translation, Vec3::ZERO, "the head is the round");
                    assert!(flying.iter().all(|d| d.scale.min_element() > 0.), "all of it is out");
                    // (At the lowest level, with six drops, the first bead behind is already coming away.)
                    assert!(touching(&flying[0], &flying[1]), "whole behind the head: {flying:?}");
                    assert!(count < 8 || touching(&flying[1], &flying[2]), "whole behind the head: {flying:?}");
                    // ...but behind, it has come apart into drops with air between them, beads and satellites in turn.
                    let apart = flying.windows(2).filter(|w| !touching(&w[0], &w[1])).count();
                    assert!(apart >= count / 2 - 1, "{count} drops, {apart} gaps: {flying:?}");
                    let tail = &flying[count / 2..];
                    let sizes: Vec<f32> = tail.iter().map(|d| d.scale.x).collect();
                    assert!(sizes.windows(3).any(|w| (w[0] > w[1] && w[2] > w[1]) || (w[0] < w[1] && w[2] < w[1])), "beads and satellites: {sizes:?}");
                    // Each lies behind the one before, the tail drooping below the path, more the further back...
                    assert!(flying.windows(2).all(|w| w[1].translation.z > w[0].translation.z));
                    let last = flying[count - 1].translation;
                    assert!(last.y < -0.005, "the tail droops: {last}");
                    assert!(last.y < flying[count / 2].translation.y && flying[count / 2].translation.y < flying[1].translation.y + 1e-4);
                    // ...and never swings side to side: what strays off the path is a few millimetres, where the
                    // rope's squiggle was 11.
                    assert!(flying.iter().all(|d| d.translation.x.abs() < 0.003), "{flying:?}");
                }
            }
        }
    }

    /// A fast round leaves a tracer behind it, as long as TRACE seconds of its flight and no longer than its way from the
    /// muzzle: the sniper's (45 m/s) a streak over a metre long, the pistol's a short one; none at the muzzle.
    #[test]
    fn a_fast_round_leaves_a_tracer_as_long_as_its_last_moments_of_flight() {
        use kitchen_sim::feast::ROUND_SPEED;
        let long = |w: usize, travelled: f32| streak(ROUND_SPEED[w], travelled).scale.z * 2.;
        assert!((long(2, 5.) - ROUND_SPEED[2] * TRACE).abs() < 1e-4 && long(2, 5.) > 1.);
        assert!(long(1, 2.) < 0.3 && long(1, 2.) > 0.2);
        assert!(long(2, 5.) > long(6, 5.) && long(6, 5.) > long(1, 5.) && long(1, 5.) > long(4, 5.));
        assert!((long(2, 0.3) - 0.3).abs() < 1e-4, "no further back than the muzzle");
        assert_eq!(streak(ROUND_SPEED[2], 0.).scale, Vec3::ZERO);
        // Behind the round (its frame's +z), and thin.
        let t = streak(ROUND_SPEED[2], 5.);
        assert!(t.translation.z > 0. && t.scale.x < 0.009 && t.scale.x > streak(ROUND_SPEED[1], 5.).scale.x, "thin, and thicker the faster");
    }
    #[test]
    fn the_mustard_squirt_wobbles_in_its_drops_not_as_a_body() {
        let level = Vec3::NEG_Y;
        for count in squirt_counts() {
            let [a, b] = [2.0, 2.037].map(|now| jet(count, 0.6, now, 5, level, Vec3::ZERO, 1.5));
            // Where each drop is does not move with the clock: the body does not swing.
            for (x, y) in a.iter().zip(&b) {
                assert_eq!(x.translation, y.translation);
                assert_eq!(x.rotation, y.rotation);
            }
            // But the drops come apart change shape, and the head quivers.
            let changed = a.iter().zip(&b).filter(|(x, y)| (x.scale - y.scale).abs().max_element() > 0.03 * x.scale.max_element()).count();
            assert!(changed >= count / 3, "{changed} of {count} drops wobble");
            assert!(a[0].scale != b[0].scale, "the head quivers");
        }
        // Aimed up or down, it still droops down across its path and never runs ahead of its head.
        let steep = Quat::from_rotation_x(1.2);
        let down = steep.inverse() * Vec3::NEG_Y;
        let drops = jet(10, 0.6, 1., 3, down, Vec3::ZERO, 1.5);
        assert!(drops.iter().skip(1).all(|d| d.translation.z > 0.));
        assert!(drops.iter().all(|d| d.translation.is_finite() && d.scale.is_finite() && d.rotation.is_finite()));
        let sag = drops[9].translation - Vec3::Z * drops[9].translation.z;
        assert!(sag.dot(down - Vec3::Z * down.z) > 0.004, "{sag}");
    }

    #[test]
    fn a_mustard_round_is_drawn_from_where_the_simulation_has_it() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<Image>>();
        let look = look(&mut world);
        let mut blob = spawn_round(&mut world, 1, 2, 31, Vec3::ZERO);
        assert_eq!(blob.parts.len(), squirt_counts()[0]);
        // Every drop of the squirt is wet mustard.
        assert!(blob.parts.iter().all(|e| world.get::<MeshMaterial3d<StandardMaterial>>(*e).is_some_and(|m| m.0 == look.jet)));
        let jet_material = world.resource::<Assets<StandardMaterial>>().get(&look.jet).unwrap();
        assert!(jet_material.clearcoat > 0. && jet_material.clearcoat_perceptual_roughness < 0.1, "glossy");
        assert!(jet_material.perceptual_roughness > 0.33, "rough enough underneath that `visuals` leaves it be");
        let dir = Vec3::new(0.6, 0., -0.8);
        let velocity = dir * feast::ROUND_SPEED[1];
        let p = Vec3::new(0.4, feast::FLOOR + 0.1, 2.2);
        let mut now = 1.;
        for step in 0..30 {
            let travelled = 0.2 + step as f32 * 0.025;
            blob.at = p + dir * travelled;
            pose_round(&mut world, &mut blob, velocity, travelled, now, 1. / 60.);
            now += 1. / 60.;
            let root = *world.get::<Transform>(blob.root).unwrap();
            assert!(root.translation.distance(blob.at) < 1e-6, "the round is where the simulation has it");
            let head = world.get::<Transform>(blob.parts[0]).unwrap();
            assert_eq!(head.translation, Vec3::ZERO, "its head is the round");
            assert!(head.scale.min_element() > 0.005);
            // The rest trails behind it and droops below it.
            for part in &blob.parts[1..] {
                let at = root.transform_point(world.get::<Transform>(*part).unwrap().translation) - blob.at;
                assert!(at.dot(dir) < 0., "behind the head");
            }
            let tail = root.transform_point(world.get::<Transform>(*blob.parts.last().unwrap()).unwrap().translation);
            assert!(tail.y < blob.at.y - 0.005, "the tail droops");
        }
        // It sheds drops as it goes, as wet as it is.
        let mess = world.resource::<Mess>();
        assert!(!mess.motes.is_empty(), "drops peel off");
        assert!(mess.motes.iter().all(|m| world.get::<MeshMaterial3d<StandardMaterial>>(m.entity).is_some_and(|mat| mat.0 == look.jet)));
        despawn_round(&mut world, blob);
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The titan's body, made in code: an original colossal hotdog, twelve hotdogs tall, built to be looked up at.
//!
//! * **Proportions.** Heavier than a hotdog: a fat sausage (its links twisted at a pinched neck) hunched on thick,
//!   short legs and flat elephant feet; long, thick arms whose enormous hands hang to its knees.
//! * **The face** is the top link's: a grin far too wide, fixed, its lips drawn back from two rows of big flat clenched
//!   teeth, and two small lidless eyes set high under a heavy ridge, staring.
//! * **The casing** is scorched: mottled dark, charred at the ends, scarred by grill marks that run across it in
//!   welted bands, and split open in long tears where the pale meat bulges out.
//! * **The bun** is armour: each half three thick plates of crust overlapping like shingles down its flanks, the top one
//!   a pauldron over the shoulder, charred at the edges, the crumb showing on the inside.
//! * **Butcher's twine** binds its wrists, elbows, knees and ankles.
//! * **The weak spot** at the back of its head is different to look at: a pale, veined patch of thin casing with a
//!   cocktail pick stuck in it, flying a little red and yellow pennant (its own mesh and material, which pulse).
//!
//! Everything is made at `REFERENCE` metres tall, in the body's frame (y up, the face towards -z, x to its right), and
//! the titan's root is scaled to the simulation's size. Each part is one mesh in one of four shared materials (the
//! casing, the crust, the enamel of the teeth and eyes, the weak spot's mark), so the whole giant is about twenty
//! entities. Each graphics level has its own resolution ([`Lod`]); the shapes stay the same.
use super::shape::{Shape, ember, fbm, hash, rgba, smooth, srgb};
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

// ----- Proportions (metres, at REFERENCE tall)

/// The height the meshes are made at: the titan's root is scaled by the simulation's height over this.
pub const REFERENCE: f32 = 1.8;
/// The torso turns (leans, twists, tilts) about this point, between the hips.
pub const PIVOT: Vec3 = Vec3::new(0., 0.66, 0.);
/// The head nods and turns about this point, in the torso's frame as it stands at rest (the pinch between the links).
pub const NECK: Vec3 = Vec3::new(0., 1.30, 0.);
/// The right hip and the right shoulder (the left ones mirror them), at rest.
pub const HIP: Vec3 = Vec3::new(0.15, 0.64, 0.);
pub const SHOULDER: Vec3 = Vec3::new(0.39, 1.15, 0.01);
/// The arms' and legs' bones.
pub const UPPER: f32 = 0.36;
pub const FORE: f32 = 0.36;
pub const THIGH: f32 = 0.285;
pub const SHIN: f32 = 0.285;
/// The ankle is this far above the sole.
pub const ANKLE: f32 = 0.10;
/// The eyes' middles (the right one; the left mirrors it), in the head's frame (from `NECK`).
pub const EYE: Vec3 = Vec3::new(0.074, 0.40, -0.178);
/// The eyes' radius.
pub const EYEBALL: f32 = 0.021;
/// The mouth: the height of the middle of the grin in the head's frame, how far round the casing it reaches either side
/// (radians: nearly ear to ear), how far its corners turn up, and how wide it is open in the middle.
pub const GRIN: (f32, f32, f32, f32) = (0.17, 1.45, 0.085, 0.1);
/// The weak spot's patch is the simulation's weak cap itself (kitchen_sim::titan `WEAK_ARC` round its `NAPE`, seen from
/// the head's middle), on the drawn head: every round on it is on the weak spot, and every round on the weak spot is
/// on it. The head's middle, in the head's frame (from `NECK`): the simulation's, `titan::NECK` up the head.
pub fn cap_centre() -> Vec3 {
    Vec3::Y * (kitchen_sim::titan::NECK / (kitchen_sim::titan::TALL / REFERENCE))
}
/// The nape's way in the head's frame (the cap's middle), as the simulation has it.
pub fn nape() -> Vec3 {
    kitchen_sim::titan::NAPE.normalize()
}
/// The point of the drawn head (`head_radius` round its upright) along the unit vector `way` from the head's middle.
pub fn on_head_along(way: Vec3) -> Vec3 {
    let centre = cap_centre();
    let inside = |q: Vec3| q.y > -0.08 && q.y < TOP - NECK.y && Vec2::new(q.x, q.z).length() <= head_radius(q.y);
    let (mut a, mut b) = (0f32, 0.6f32);
    for _ in 0..30 {
        let m = 0.5 * (a + b);
        if inside(centre + way * m) {
            a = m;
        } else {
            b = m;
        }
    }
    centre + way * a
}
/// How far into the weak spot's patch a point of the head (its frame) is: 1 on it, 0 off it, over a band `EDGE` wide
/// at its edge.
pub fn in_patch(at: Vec3) -> f32 {
    let from = at - cap_centre();
    let far = from.length().max(1e-4);
    let off = from.angle_between(nape());
    let band = EDGE / far;
    1. - smooth((off - (kitchen_sim::titan::WEAK_ARC - band * 0.5)) / band)
}
/// The width of the patch's edge on the casing (metres).
pub const EDGE: f32 = 0.006;
/// The weak spot as it is seen, aimed at and marked (the pick, the HUD's ring): the patch's point half its arc up from
/// the cap's middle (which is low, at the neck's pinch behind the shoulders), on the drawn head, in the head's frame.
pub fn spot() -> Vec3 {
    on_head_along(Quat::from_rotation_x(-kitchen_sim::titan::WEAK_ARC * 0.5) * nape())
}
/// The bottom of the sausage and the top of its head (when the head is not nodding), in the body's frame.
pub const BOTTOM: f32 = 0.50;
pub const TOP: f32 = 1.80;
/// The top of the torso link (its rounded end, under the head's).
pub const TORSO_TOP: f32 = 1.41;

/// How finely a graphics level has the titan made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lod {
    /// High and Medium.
    Full,
    /// Low: about a quarter of the triangles.
    Lean,
    /// Lowest: the least that still reads as the same giant.
    Least,
}
impl Lod {
    pub fn of(level: crate::quality::Level) -> Lod {
        use crate::quality::Level;
        match level {
            Level::High | Level::Medium => Lod::Full,
            Level::Low => Lod::Lean,
            Level::Lowest => Lod::Least,
        }
    }
    /// Rows or columns for a surface that has `full` of them at the finest.
    fn n(self, full: u32) -> u32 {
        let k = match self {
            Lod::Full => 1.,
            Lod::Lean => 0.45,
            Lod::Least => 0.22,
        };
        ((full as f32 * k).round() as u32).max(3)
    }
    /// Whether the casing has its fine relief (the welts and the meat in the splits stand out of it).
    fn relief(self) -> bool {
        self != Lod::Least
    }
}

// ----- Colours (as an artist picks them; linear once used)

fn meat() -> Vec3 {
    srgb(0.66, 0.27, 0.15)
}
fn meat_dark() -> Vec3 {
    srgb(0.5, 0.17, 0.09)
}
fn scorch() -> Vec3 {
    srgb(0.25, 0.1, 0.06)
}
fn char() -> Vec3 {
    srgb(0.07, 0.045, 0.038)
}
fn flesh() -> Vec3 {
    srgb(0.98, 0.6, 0.46)
}
fn torn() -> Vec3 {
    srgb(0.32, 0.08, 0.05)
}
fn pale() -> Vec3 {
    srgb(0.90, 0.76, 0.68)
}
fn twine() -> Vec3 {
    srgb(0.78, 0.68, 0.50)
}

// ----- The casing's surface: where it is scarred, split and scorched

/// A split in the casing: round the body at `angle` (radians from the front, to its right), from `from` to `to` high,
/// `wide` at its widest, and how much it wanders.
struct Split {
    angle: f32,
    from: f32,
    to: f32,
    wide: f32,
    wander: f32,
}
/// The torso's splits (heights in the body's frame) and the head's (in the head's frame).
const TORSO_SPLITS: [Split; 4] = [
    Split { angle: -0.55, from: 0.68, to: 1.06, wide: 0.026, wander: 0.05 },
    Split { angle: 2.45, from: 0.8, to: 1.26, wide: 0.03, wander: 0.07 },
    Split { angle: 1.25, from: 0.6, to: 0.86, wide: 0.022, wander: 0.04 },
    Split { angle: -2.0, from: 0.98, to: 1.24, wide: 0.026, wander: 0.05 },
];
const HEAD_SPLITS: [Split; 1] = [Split { angle: -1.95, from: 0.03, to: 0.28, wide: 0.02, wander: 0.04 }];

/// What the casing is at a point: how far out of (or into) the surface it stands, its colour, and how much it glows
/// with embers (0 to 1: the meat in its splits, hot cracks in the char of its grill marks). `angle` from the front to
/// the right, `y` up its axis, `radius` the casing's there; `marks` whether the grill marks run here.
fn casing_at(angle: f32, y: f32, radius: f32, splits: &[Split], marks: bool, seed: u32) -> (f32, Vec3, f32) {
    let around = angle.rem_euclid(TAU) / TAU;
    // The mottle of a sausage cooked too long, and darker scorched patches.
    let grain = fbm(around * 16., y * 9., 16, seed);
    let blotch = fbm(around * 5., y * 3.2, 5, seed + 7);
    let mut colour = meat_dark().lerp(meat(), smooth((grain - 0.2) / 0.5));
    colour = colour.lerp(scorch(), smooth((blotch - 0.5) / 0.2) * 0.85);
    // Blisters: small raised bubbles of the casing, darker at their crowns.
    let blister = smooth((fbm(around * 48., y * 26., 48, seed + 5) - 0.62) / 0.08);
    colour = colour.lerp(scorch() * 0.7, blister * 0.6);
    let mut lift = (grain - 0.5) * 0.007 + 0.005 * blister;
    let mut glow = 0f32;
    // Grill marks: welted bands of char running across the front and the back, at a slant.
    if marks {
        let face = angle.cos().abs();
        if face > 0.25 {
            // (Measured from the middle of the front, or of the back: the pattern is whole across either.)
            let a = (angle + PI).rem_euclid(TAU) - PI;
            let from = if a.cos() >= 0. { a } else { a - PI * a.signum() };
            // (A ragged edge, and only where it lay on the bars: patches of the body, not stripes all over.)
            let slant = radius * from + y * 0.55 + 0.04 * (fbm(around * 9., y * 7., 9, seed + 11) - 0.5);
            let spacing = 0.28;
            let d = (slant / spacing).rem_euclid(1.) - 0.5;
            let w = 0.17 * (0.7 + 0.6 * fbm(around * 7., y * 4., 7, seed + 3));
            let band = 1. - smooth((d.abs() - w * 0.5) / 0.03);
            let welt = (-((d.abs() - w * 0.62) / 0.03).powi(2)).exp();
            let keep = smooth((face - 0.25) / 0.2) * smooth((fbm(around * 3., y * 2.2, 3, seed + 13) - 0.32) / 0.12);
            colour = colour.lerp(char(), band * keep * 0.95);
            colour = colour.lerp(meat_dark() * 1.3, welt * keep * 0.5);
            lift += (-0.0065 * band + 0.0035 * welt) * keep;
            // Hot cracks in the char: a broken glow along the bands, never the whole band.
            let cracks = smooth((fbm(around * 34., y * 21., 34, seed + 17) - 0.42) / 0.22);
            glow = glow.max(band * keep * cracks * 0.6);
        }
    }
    // The splits: pale meat bulging out of torn, darker lips.
    for (k, s) in splits.iter().enumerate() {
        if y < s.from - 0.03 || y > s.to + 0.03 {
            continue;
        }
        let t = ((y - s.from) / (s.to - s.from)).clamp(0., 1.);
        let wobble = s.wander * (fbm(y * 6., k as f32 * 3.1, 0, seed + 20 + k as u32) - 0.5);
        let centre = s.angle + wobble / radius.max(0.05);
        let off = ((angle - centre + PI).rem_euclid(TAU) - PI) * radius;
        let half = s.wide * (t * PI).sin().max(0.).powf(0.7) * (0.8 + 0.4 * fbm(y * 14., k as f32, 0, seed + 30));
        if half <= 1e-4 {
            continue;
        }
        let inside = 1. - smooth((off.abs() - half) / 0.006);
        let lip = (-((off.abs() - half) / 0.007).powi(2)).exp();
        let fibre = 0.85 + 0.25 * fbm(off * 60., y * 22., 0, seed + 40);
        colour = colour.lerp(flesh() * fibre, inside);
        colour = colour.lerp(torn(), lip * 0.8);
        lift += 0.011 * inside * (1. - (off / half.max(1e-3)).powi(2)).max(0.) + 0.004 * lip;
        // The meat in a split glows hottest along its middle, the lips smoulder.
        glow = glow.max(inside * (0.55 + 0.45 * (1. - (off / half.max(1e-3)).powi(2)).max(0.)) * fibre.min(1.)).max(lip * 0.35);
    }
    (lift, colour, glow)
}
/// A smooth curve through `points` (x, y) at `x`: monotone in x, Catmull-Rom between them.
fn curve(points: &[(f32, f32)], x: f32) -> f32 {
    let n = points.len();
    if x <= points[0].0 {
        return points[0].1;
    }
    if x >= points[n - 1].0 {
        return points[n - 1].1;
    }
    let i = points.iter().rposition(|p| p.0 <= x).unwrap_or(0).min(n - 2);
    let (x0, y0) = points[i];
    let (x1, y1) = points[i + 1];
    let slope = |j: usize| {
        let (a, b) = (points[j.saturating_sub(1)], points[(j + 1).min(n - 1)]);
        (b.1 - a.1) / (b.0 - a.0).max(1e-5)
    };
    let h = x1 - x0;
    let t = (x - x0) / h;
    let (m0, m1) = (slope(i) * h, slope(i + 1) * h);
    let (t2, t3) = (t * t, t * t * t);
    (2. * t3 - 3. * t2 + 1.) * y0 + (t3 - 2. * t2 + t) * m0 + (-2. * t3 + 3. * t2) * y1 + (t3 - t2) * m1
}
/// A rounded end: the share of the radius left `d` from the end of a cap `length` long.
fn cap(d: f32, length: f32) -> f32 {
    if d >= length {
        1.
    } else {
        let t = 1. - d.max(0.) / length;
        (1. - t * t).max(0.).sqrt()
    }
}

/// The torso link's radius `y` up the body (its frame at rest).
pub fn torso_radius(y: f32) -> f32 {
    let r = curve(&[(0.50, 0.25), (0.62, 0.288), (0.80, 0.302), (1.00, 0.298), (1.15, 0.284), (1.27, 0.262), (1.35, 0.236), (1.41, 0.2)], y);
    r * cap(y - BOTTOM, 0.15) * cap(TORSO_TOP - y, 0.09)
}
/// The head link's radius `y` up from the neck (its own frame).
pub fn head_radius(y: f32) -> f32 {
    let r = curve(&[(-0.08, 0.2), (0.0, 0.214), (0.1, 0.24), (0.21, 0.248), (0.31, 0.24), (0.40, 0.22), (0.50, 0.19)], y);
    r * cap(y + 0.08, 0.07) * cap(TOP - NECK.y - y, 0.14)
}
/// A point on the front of the head `angle` round from the middle of the face (to the right), `y` up from the neck,
/// `out` off the casing (the casing's own relief left out), and the way out there.
pub fn on_head(angle: f32, y: f32, out: f32) -> (Vec3, Vec3) {
    let r = head_radius(y);
    let dir = Vec3::new(angle.sin(), 0., -angle.cos());
    // The way out leans up or down with the casing's curve.
    let slope = (head_radius(y + 0.005) - head_radius(y - 0.005)) / 0.01;
    let normal = (dir - Vec3::Y * slope).normalize();
    (dir * (r + out) + Vec3::Y * y, normal)
}

/// The grin's middle line and how wide it is open `angle` round from the middle of the face (in the head's frame).
pub fn grin(angle: f32) -> (f32, f32) {
    let (y0, reach, rise, open) = GRIN;
    let s = (angle / reach).clamp(-1., 1.);
    let middle = y0 + rise * s * s * (1. + 0.4 * s * s);
    let gap = open * (1. - s.powi(4)).max(0.).powf(0.55) * (1. - 0.25 * s * s);
    (middle, gap)
}

/// The torso link (in the torso's frame: origin at `PIVOT`).
pub fn torso(lod: Lod) -> Shape {
    let mut s = Shape::default();
    let (rows, columns) = (lod.n(84), lod.n(76));
    let relief = lod.relief();
    let place = move |u: f32, v: f32| -> (Vec3, Vec3, f32) {
        let y = BOTTOM + (TORSO_TOP - BOTTOM) * u;
        // The seam is at the back.
        let angle = (v - 0.5) * TAU;
        let r = torso_radius(y);
        let (lift, colour, glow) = casing_at(angle, y, r.max(0.05), &TORSO_SPLITS, true, 11);
        // The ends are charred where they lay on the grill.
        let ends = 1. - smooth((y - BOTTOM) / 0.12).min(smooth((TORSO_TOP - y) / 0.06));
        let colour = colour.lerp(char(), ends * 0.7);
        let lift = if relief { lift } else { 0. };
        // A slight hunch: the top of the body comes forward a little, and the belly sags.
        let hunch = 0.;
        let p = Vec3::new(angle.sin() * (r + lift * smooth(r / 0.1)), y, hunch - angle.cos() * (r + lift * smooth(r / 0.1)));
        (p - PIVOT, colour, glow)
    };
    // (Round the body with the face on -z: its own handedness, so its normals are turned over to face out.)
    s.surface(rows, columns, true, move |u, v| place(u, v).0, move |u, v, _| {
        let (_, c, g) = place(u, v);
        ember(c, g)
    });
    s
}

/// The head link (in the head's frame: origin at `NECK`), with its brow ridge, its eye sockets and the scar across its
/// face; its eyes, teeth and lips are other meshes (`eyes`, `mouth`).
pub fn head(lod: Lod) -> Shape {
    let mut s = Shape::default();
    let (rows, columns) = (lod.n(66), lod.n(88));
    let relief = lod.relief();
    let place = move |u: f32, v: f32| -> (Vec3, Vec3, f32) {
        let bottom = -0.08;
        let y = bottom + (TOP - NECK.y - bottom) * u;
        // The seam is at the back, under the weak spot's patch: round from the middle of the face.
        let angle = (v - 0.5) * TAU;
        let r = head_radius(y);
        let (mut lift, mut colour, mut glow) = casing_at(angle, y, r.max(0.05), &HEAD_SPLITS, false, 23);
        if !relief {
            lift = 0.;
        }
        let at = Vec3::new(angle.sin() * r, y, -angle.cos() * r);
        // One grill mark across the face, a scar from the right brow down over the left cheek.
        let scar_line = (at.x * 0.8 + (y - 0.33) * 1.0) / 1.28;
        let scar = (1. - smooth((scar_line.abs() - 0.016) / 0.012)) * smooth((angle.cos() - 0.15) / 0.2) * smooth((0.47 - y) / 0.05) * smooth((y - 0.05) / 0.05);
        let mouth = grin(angle);
        let in_mouth = (y - mouth.0).abs() < mouth.1 * 0.5 + 0.02 && angle.abs() < GRIN.1 + 0.05;
        let scar = if in_mouth { 0. } else { scar };
        colour = colour.lerp(char(), scar * 0.9);
        lift += -0.006 * scar;
        glow = glow.max(scar * 0.5 * smooth((fbm(at.x * 40., at.y * 40., 0, 91) - 0.35) / 0.25));
        // The eye sockets: sunk and bruised dark round each eye, a heavy ridge over them.
        for side in [-1f32, 1.] {
            let eye = Vec3::new(side * EYE.x, EYE.y, EYE.z);
            let d = Vec2::new(at.x - eye.x, (at.y - eye.y) * 0.85).length() * smooth(-at.z / 0.05);
            let front = smooth((-at.z - 0.05) / 0.05);
            let socket = (1. - smooth((d - 0.022) / 0.02)) * front;
            let bruise = (1. - smooth((d - 0.03) / 0.035)) * front;
            colour = colour.lerp(char(), bruise * 0.8);
            lift -= 0.014 * socket;
        }
        // A heavy brow over both eyes, overhanging them: one ridge across the face.
        {
            let front = smooth((-at.z - 0.05) / 0.05);
            let across = (at.x / 0.16).clamp(-1., 1.);
            let line = EYE.y + 0.038 + 0.012 * across * across;
            let ridge = (1. - smooth(((at.y - line).abs() - 0.008) / 0.022)) * (1. - smooth((at.x.abs() - 0.15) / 0.05));
            lift += 0.024 * ridge * front;
            colour = colour.lerp(scorch(), ridge * front * 0.5);
        }
        // The cheeks bunch up under the grin's corners.
        for side in [-1f32, 1.] {
            let corner = side * GRIN.1 * 0.82;
            let (cy, _) = grin(corner);
            let d = Vec2::new((angle - corner) * r, (y - cy - 0.05) * 1.2).length();
            lift += 0.014 * (1. - smooth(d / 0.06));
        }
        // The weak spot: a pale, thin, veined patch at the back of the head, the simulation's weak cap (drawn over by
        // its own glowing mesh, `patch`), no embers in it.
        let patch = in_patch(at);
        if patch > 0. {
            colour = colour.lerp(pale(), patch);
            glow *= 1. - patch;
        }
        // The top is charred.
        colour = colour.lerp(char(), smooth((y - 0.45) / 0.06) * 0.6);
        let r = r + lift * smooth(r / 0.08);
        (Vec3::new(angle.sin() * r, y, -angle.cos() * r), colour, glow)
    };
    s.surface(rows, columns, true, move |u, v| place(u, v).0, move |u, v, _| {
        let (_, c, g) = place(u, v);
        ember(c, g)
    });
    s
}

/// The mouth (in the head's frame): the dark inside under the grin, the lips drawn back round it, and the two rows of
/// big flat clenched teeth. Enamel.
pub fn mouth(lod: Lod) -> Shape {
    let mut s = Shape::default();
    let (_, reach, _, _) = GRIN;
    // The teeth: flat-fronted, a little uneven, yellowed, darker at the gum; fewer where they are finer.
    let count = 18;
    let tooth = |s: &mut Shape, angle: f32, upper: bool, k: u32| {
        let (middle, gap) = grin(angle);
        let r = head_radius(middle);
        let width = (2. * reach / count as f32) * r * 0.8 * (1. - 0.25 * (angle / reach).powi(2));
        // (The rows a little apart: the furnace inside shows between them.)
        let tall = (gap * 0.5 - 0.004) * (0.92 + 0.12 * hash(k * 13 + upper as u32));
        let y = middle + if upper { tall * 0.5 + 0.007 } else { -tall * 0.5 - 0.007 };
        let (at, normal) = on_head(angle, y, 0.012);
        let across = Vec3::Y.cross(normal).normalize();
        let turn = Quat::from_mat3(&Mat3::from_cols(-across, Vec3::Y, -normal)) * Quat::from_rotation_z(0.04 * (hash(k * 7 + 3) - 0.5));
        let stain = hash(k * 31 + upper as u32 * 5);
        let enamel = srgb(0.86, 0.80, 0.62).lerp(srgb(0.62, 0.55, 0.36), (stain - 0.6).max(0.) * 2.2);
        s.blob(at, Vec3::new(width * 0.5, tall * 0.5, 0.016), turn, 5., lod.n(6), lod.n(10), move |p| {
            // Darker towards the gum (the end away from the bite), and at the sides.
            let gum = if upper { p.y } else { -p.y };
            rgba(enamel.lerp(srgb(0.35, 0.22, 0.12), smooth((gum - 0.45) / 0.55) * 0.7 + smooth((p.x.abs() - 0.8) / 0.2) * 0.25))
        });
    };
    for k in 0..count {
        let angle = ((k as f32 + 0.5) / count as f32 * 2. - 1.) * reach * 0.94;
        tooth(&mut s, angle, true, k);
        tooth(&mut s, angle + reach * 0.5 / count as f32, false, k + 40);
    }
    // The lips, drawn back hard: a thick rim round the opening, darker than the casing.
    let lip = srgb(0.28, 0.07, 0.045);
    for upper in [true, false] {
        let n = 24;
        let points: Vec<(Vec3, f32)> = (0..=n)
            .map(|i| {
                let angle = (i as f32 / n as f32 * 2. - 1.) * reach * 1.02;
                let (middle, gap) = grin(angle);
                let y = middle + if upper { gap * 0.5 + 0.008 } else { -gap * 0.5 - 0.008 };
                let r = 0.0115 * (0.35 + 0.65 * (1. - (angle / reach).powi(2)).max(0.).sqrt());
                (on_head(angle, y, 0.006).0, r)
            })
            .collect();
        s.tube(&points, lod.n(8), lod.n(3), |_, _| rgba(lip));
    }
    s
}

/// The inside of the mouth (in the head's frame): a band over the opening behind the teeth, which glows like a furnace
/// (the ember material): the giant is hot inside.
pub fn maw(lod: Lod) -> Shape {
    let mut s = Shape::default();
    let (_, reach, _, _) = GRIN;
    s.surface(
        lod.n(8),
        lod.n(48),
        true,
        |u, v| {
            let angle = (v - 0.5) * 2. * reach;
            let (middle, gap) = grin(angle);
            let y = middle + (u - 0.5) * (gap + 0.012);
            on_head(angle, y, 0.004).0
        },
        |u, v, _| {
            // Brightest deep in the middle, darker towards the lips and the corners.
            let a = ((v - 0.5) * 2.).abs();
            let k = (1. - a * a) * (1. - ((u - 0.5) * 2.).powi(2) * 0.6);
            rgba(srgb(0.25, 0.03, 0.01).lerp(srgb(1., 0.42, 0.08), k.max(0.)))
        },
    );
    s
}

/// One eye (the right; the left mirrors it), in its own frame: a small lidless yellowed ball looking down its -z, a
/// pinprick of a pupil and red veins at its edges. Enamel.
pub fn eye(lod: Lod) -> Shape {
    let mut s = Shape::default();
    s.blob(Vec3::ZERO, Vec3::splat(EYEBALL), Quat::from_rotation_x(FRAC_PI_2), 2., lod.n(14), lod.n(18), |p| {
        // In the blob's frame its +y is the eye's -z (it was turned), the way it looks.
        let look = p.y;
        let pupil = 1. - smooth((1. - look - 0.012) / 0.01);
        let ring = 1. - smooth((1. - look - 0.05) / 0.02);
        let vein = smooth((0.5 - look) / 0.4) * (1. - smooth((fbm(p.x * 9. + 3., p.z * 9., 0, 5) - 0.5).abs() / 0.05));
        let white = srgb(0.86, 0.82, 0.66).lerp(srgb(0.55, 0.12, 0.08), vein * 0.7);
        rgba(white.lerp(srgb(0.36, 0.25, 0.08), ring * 0.8).lerp(Vec3::splat(0.004), pupil))
    });
    s
}

/// One plate of bun armour on the right flank (the left mirrors it), in the torso's frame: a rounded slab of crust
/// bent round the body over `angles` (from the front, to the right) and `heights`, `thick`, leaning out at its bottom
/// edge by `flare` metres.
fn plate(s: &mut Shape, lod: Lod, angles: (f32, f32), heights: (f32, f32), thick: f32, flare: f32, seed: u32) {
    let (a0, a1) = angles;
    let (y0, y1) = heights;
    let crust = srgb(0.60, 0.33, 0.12);
    let mid = (a0 + a1) * 0.5;
    let half_a = (a1 - a0) * 0.5;
    let half_y = (y1 - y0) * 0.5;
    let power = 5.;
    let e = 2. / power;
    let f = |w: f32, m: f32| w.signum() * w.abs().powf(m);
    let unit = move |u: f32, v: f32| {
        let lat = (u - 0.5) * PI;
        let lon = v * TAU;
        Vec3::new(f(lat.cos(), e) * f(lon.sin(), e), f(lat.sin(), e), f(lat.cos(), e) * f(lon.cos(), e))
    };
    let place = move |q: Vec3| -> Vec3 {
        // x round the body, y up, z out from it.
        let angle = mid + q.x * half_a;
        let y = (y0 + y1) * 0.5 + q.y * half_y;
        let base = torso_radius(y.clamp(BOTTOM + 0.15, TORSO_TOP - 0.06)).max(0.2) + 0.012;
        let out = base + thick * (0.5 + 0.5 * q.z) + flare * smooth((0.6 - q.y) / 1.6) + 0.006 * q.x * q.x;
        let hunch = 0.;
        Vec3::new(angle.sin() * out, y, hunch - angle.cos() * out) - PIVOT
    };
    s.surface(lod.n(16), lod.n(28), true, move |u, v| place(unit(u, v)), move |u, v, _| {
        let q = unit(u, v);
        let edge = q.x.abs().max(q.y.abs());
        let grain = fbm(q.x * 6. + seed as f32, q.y * 5., 0, seed);
        let mut c = crust.lerp(srgb(0.72, 0.45, 0.18), smooth((grain - 0.4) / 0.4) * 0.6);
        // Charred towards its edges and in spots; the crumb inside.
        c = c.lerp(char(), smooth((edge - 0.72) / 0.25) * 0.85);
        c = c.lerp(scorch(), smooth((fbm(q.x * 3., q.y * 3., 0, seed + 9) - 0.62) / 0.12) * 0.8);
        if q.z < -0.3 {
            c = c.lerp(srgb(0.86, 0.74, 0.52), smooth((-q.z - 0.3) / 0.4));
        }
        // Its charred edges smoulder, here and there.
        let smoulder = smooth((edge - 0.8) / 0.18) * smooth((fbm(q.x * 9. + seed as f32, q.y * 9., 0, seed + 21) - 0.45) / 0.2) * 0.45;
        ember(c, smoulder)
    });
}

/// The bun's armour (in the torso's frame): three overlapping plates down each flank, the top one a pauldron that
/// comes up over the shoulder. Crust.
pub fn bun(lod: Lod) -> Shape {
    let mut right = Shape::default();
    plate(&mut right, lod, (0.75, 2.75), (0.6, 0.88), 0.062, 0.018, 3);
    plate(&mut right, lod, (0.68, 2.7), (0.82, 1.08), 0.066, 0.026, 5);
    plate(&mut right, lod, (0.62, 2.62), (1.02, 1.24), 0.07, 0.032, 7);
    // The pauldron: a domed plate over the shoulder.
    let crust = srgb(0.58, 0.32, 0.12);
    let shoulder = SHOULDER - PIVOT + Vec3::new(-0.03, 0.06, 0.);
    right.blob(shoulder, Vec3::new(0.15, 0.09, 0.17), Quat::from_rotation_z(-0.35), 3.2, lod.n(14), lod.n(22), move |p| {
        let edge = smooth((-p.y + 0.15) / 0.5) * 0.7 + smooth((p.x.abs().max(p.z.abs()) - 0.75) / 0.25) * 0.6;
        let spot = smooth((fbm(p.x * 4. + 2., p.z * 4., 0, 13) - 0.6) / 0.15);
        rgba(crust.lerp(char(), edge.min(1.) * 0.85).lerp(scorch(), spot * 0.7))
    });
    let mut s = right.clone();
    s.append(&right.mirrored());
    s
}

/// Butcher's twine wound round a limb `radius` thick at `y` (the limb runs along y).
fn twine_ring(s: &mut Shape, lod: Lod, y: f32, radius: f32, turns: u32) {
    for k in 0..turns {
        let yy = y + (k as f32 - (turns - 1) as f32 * 0.5) * 0.013;
        let n = lod.n(20);
        let points: Vec<(Vec3, f32)> = (0..=n)
            .map(|i| {
                let a = i as f32 / n as f32 * TAU * 1.02;
                (Vec3::new(a.sin() * radius, yy + 0.002 * (a * 3.).sin(), a.cos() * radius), 0.0065)
            })
            .collect();
        s.tube(&points, lod.n(5).max(4), 1, |_, _| rgba(twine()));
    }
}
/// The colour of a limb's casing at `t` along it (0 at its joint) and `angle` round.
fn limb_casing(t: f32, angle: f32, radius: f32, seed: u32) -> (f32, Vec3, f32) {
    let (lift, mut colour, _) = casing_at(angle, t * 3. + seed as f32 * 0.37, radius, &[], false, seed);
    // One old scar across it, slanting: a single grill mark, not stripes, cracked and smouldering.
    let d = ((angle / TAU) * 0.5 + t - 0.35 - 0.1 * (seed % 3) as f32).abs();
    let scar = 1. - smooth((d - 0.035) / 0.02);
    colour = colour.lerp(char(), scar * 0.85);
    let cracks = smooth((fbm(angle * 5. + seed as f32, t * 30., 0, seed + 19) - 0.4) / 0.25);
    (lift - 0.004 * scar, colour, scar * cracks * 0.55)
}
/// A limb's bone (from its joint at the origin down its -y, `length` long), `radii` along it (from the joint), `bulge`
/// forward of its middle (a bicep, a calf), with the twine round it at `ties` (shares of its length).
fn limb(lod: Lod, length: f32, radii: &[f32], bulge: (f32, f32), ties: &[f32], seed: u32) -> Shape {
    let mut s = Shape::default();
    let n = radii.len();
    let profile: Vec<(f32, f32)> = radii.iter().enumerate().map(|(i, r)| (i as f32 / (n - 1) as f32, *r)).collect();
    let profile = profile.as_slice();
    let relief = lod.relief();
    let place = move |u: f32, v: f32| -> (Vec3, Vec3, f32) {
        // Overlapping round ends beyond the joints, so a bent elbow or knee stays whole.
        let over = 0.6;
        let t = -over * radii[0] / length + u * (1. + over * (radii[0] + radii[n - 1]) / length);
        let tt = t.clamp(0., 1.);
        let r0 = curve(profile, tt);
        let end = if t < 0. { cap(over * radii[0] + t * length, over * radii[0]) } else { cap((1. - t) * length + over * radii[n - 1], over * radii[n - 1]) };
        let angle = v * TAU;
        let b = bulge.0 * (-((tt - bulge.1) / 0.22).powi(2)).exp() * (angle.cos().max(0.)).powf(1.5);
        let r = (r0 + b) * end;
        let (lift, colour, glow) = limb_casing(tt, angle, r0, seed);
        let r = r + if relief { lift * smooth(r / 0.03) } else { 0. };
        (Vec3::new(angle.sin() * r, -t * length, -angle.cos() * r), colour, glow)
    };
    s.surface(lod.n(30), lod.n(26), false, move |u, v| place(u, v).0, move |u, v, _| {
        let (_, c, g) = place(u, v);
        ember(c, g)
    });
    for tie in ties {
        let r = curve(profile, *tie) + 0.002;
        twine_ring(&mut s, lod, -tie * length, r, 3);
    }
    s
}

/// The upper arm (the right; from the shoulder down its -y): thick, a bulge of muscle in front.
pub fn upper_arm(lod: Lod) -> Shape {
    limb(lod, UPPER, &[0.088, 0.082, 0.076, 0.072], (0.012, 0.45), &[], 31)
}
/// The forearm: heavier than the upper arm, twine at the elbow and the wrist.
pub fn forearm(lod: Lod) -> Shape {
    limb(lod, FORE, &[0.074, 0.09, 0.094, 0.082, 0.068], (0.008, 0.35), &[0.06, 0.93], 37)
}
pub fn thigh(lod: Lod) -> Shape {
    limb(lod, THIGH, &[0.124, 0.12, 0.108, 0.094], (0.01, 0.4), &[], 41)
}
pub fn shin(lod: Lod) -> Shape {
    limb(lod, SHIN, &[0.086, 0.092, 0.084, 0.074], (0.012, 0.3), &[0.08, 0.9], 43)
}

/// The shapes of a hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// Open, the fingers spread and hooked: reaching, grabbing, swatting.
    Open,
    /// A fist.
    Fist,
}
pub const GRIPS: [Grip; 2] = [Grip::Open, Grip::Fist];

/// An enormous hand (the right; the left mirrors it), from the wrist down its -y, the palm facing -z, the thumb
/// towards +x.
pub fn hand(lod: Lod, grip: Grip) -> Shape {
    let mut s = Shape::default();
    let skin = |t: f32, a: f32, seed: u32| rgba(limb_casing(t, a, 0.05, seed).1);
    let curl: [f32; 3] = match grip {
        Grip::Open => [0.35, 0.45, 0.35],
        Grip::Fist => [1.45, 1.6, 1.2],
    };
    // The palm: a broad, thick slab.
    let palm_c = match grip {
        Grip::Open => Vec3::new(0., -0.095, 0.),
        Grip::Fist => Vec3::new(0., -0.09, 0.005),
    };
    s.blob(palm_c, Vec3::new(0.105, 0.098, 0.05), Quat::IDENTITY, 2.8, lod.n(14), lod.n(20), |p| skin(p.y * 0.3 + 0.5, p.x * 3., 51));
    // Four fat fingers: from the knuckles, each three segments curling towards the palm (-z).
    let spread = match grip {
        Grip::Open => 0.16,
        Grip::Fist => 0.02,
    };
    for (i, x) in [-0.075f32, -0.026, 0.024, 0.072].into_iter().enumerate() {
        let length = [0.15, 0.175, 0.17, 0.15][i];
        let r = [0.03, 0.033, 0.033, 0.031][i];
        let fan = (x / 0.075) * spread;
        let mut at = Vec3::new(x, -0.175, -0.004);
        let mut dir = Vec3::new(fan.sin(), -fan.cos(), 0.);
        let mut points = vec![(at - dir * 0.02, r * 1.05)];
        for (k, bend) in curl.iter().enumerate() {
            let seg = length / 3. * [1.1, 1., 0.9][k];
            // Curling about the knuckle's axis (x): towards the palm.
            let axis = Vec3::new(dir.y, -dir.x, 0.).normalize_or(Vec3::X);
            dir = (Quat::from_axis_angle(axis, -bend * 0.5) * dir).normalize();
            at += dir * seg;
            points.push((at, r * (1. - 0.08 * k as f32)));
            dir = (Quat::from_axis_angle(axis, -bend * 0.5) * dir).normalize();
        }
        let seed = 60 + i as u32;
        s.tube(&points, lod.n(12), lod.n(4), move |t, a| {
            // A grimy, thick nail on the back of the tip.
            let nail = smooth((t - 0.78) / 0.1) * smooth(((a - 0.5).abs() - 0.32) / 0.08);
            let c = Vec3::from_slice(&skin(t * 0.6, a * TAU, seed)[..3]);
            rgba(c.lerp(srgb(0.55, 0.45, 0.30), nail * 0.8))
        });
    }
    // The thumb.
    let thumb: Vec<(Vec3, f32)> = match grip {
        Grip::Open => vec![(Vec3::new(0.07, -0.05, -0.02), 0.04), (Vec3::new(0.125, -0.1, -0.045), 0.036), (Vec3::new(0.15, -0.16, -0.06), 0.032), (Vec3::new(0.15, -0.205, -0.075), 0.028)],
        Grip::Fist => vec![(Vec3::new(0.07, -0.05, -0.02), 0.04), (Vec3::new(0.1, -0.1, -0.06), 0.036), (Vec3::new(0.06, -0.15, -0.085), 0.032), (Vec3::new(0.0, -0.17, -0.09), 0.029)],
    };
    s.tube(&thumb, lod.n(12), lod.n(4), |t, a| skin(t * 0.6, a * TAU, 67));
    // Enormous: half as big again as a hand for its arms.
    for p in s.positions.iter_mut() {
        *p *= HAND;
    }
    s
}
/// How much bigger than life its hands are.
pub const HAND: f32 = 1.4;

/// A foot (the right; the left mirrors it), from the ankle: a flat, heavy, elephantine slab, the toes forward (-z),
/// the sole `ANKLE` under the ankle; three stubby toes, and twine round the ankle.
pub fn foot(lod: Lod) -> Shape {
    let mut s = Shape::default();
    let skin = |p: Vec3, seed: u32| {
        let (_, c, _) = limb_casing(p.y * 0.3 + 0.5, p.x * 3. + p.z, 0.08, seed);
        // The sole is scorched black where it stands.
        rgba(c.lerp(char(), smooth((-p.y - 0.4) / 0.4) * 0.8))
    };
    s.blob(Vec3::new(0., -ANKLE * 0.48, -0.06), Vec3::new(0.11, ANKLE * 0.52, 0.19), Quat::IDENTITY, 3., lod.n(14), lod.n(22), |p| skin(p, 71));
    s.blob(Vec3::new(0., -0.012, 0.0), Vec3::new(0.085, 0.07, 0.09), Quat::IDENTITY, 2.4, lod.n(10), lod.n(16), |p| skin(p, 73));
    for (i, x) in [-0.065f32, 0., 0.065].into_iter().enumerate() {
        s.blob(Vec3::new(x, -ANKLE * 0.66, -0.235 + 0.012 * x.abs() / 0.065), Vec3::new(0.038, 0.034, 0.05), Quat::IDENTITY, 2.2, lod.n(8), lod.n(12), move |p| skin(p, 75 + i as u32));
    }
    s
}

/// The weak spot's pick (in the head's frame): a cocktail pick driven into the pale patch at the back of the head,
/// leaning out and up, flying a small pennant. The patch itself is the head's casing; this mesh is the mark's material,
/// which pulses when the spot is exposed.
pub fn pick(lod: Lod) -> Shape {
    let mut s = Shape::default();
    let spot = spot();
    let out = Vec3::new(0.08, 0.45, 1.).normalize();
    let base = spot - out * 0.05;
    let tip = spot + out * 0.27;
    let wood = srgb(0.86, 0.72, 0.48);
    s.tube(&[(base, 0.011), (spot + out * 0.12, 0.010), (tip, 0.004)], lod.n(8), lod.n(4), move |t, _| rgba(wood.lerp(srgb(0.45, 0.3, 0.18), smooth((t - 0.9) / 0.1))));
    // The pennant: a little triangle of red and yellow cloth near the top, a ripple in it.
    let top = spot + out * 0.235;
    let low = spot + out * 0.15;
    let away = Vec3::new(1., 0., -0.1).normalize();
    let pennant = |u: f32, v: f32| {
        // u along the pick, v out from it; the far end narrows to a point.
        let along = low.lerp(top, u);
        let reach = 0.13 * v * (1. - 0.5 * (u - 0.5).abs());
        let point = along.lerp((low + top) * 0.5, v * v * 0.85);
        point + away * reach + Vec3::new(0., 0., 0.012 * (v * 7. + u * 2.).sin() * v)
    };
    let tint = |u: f32, v: f32| rgba(if (u - 0.5).abs() < 0.16 && v < 0.85 { srgb(0.96, 0.78, 0.1) } else { srgb(0.8, 0.06, 0.05) });
    s.surface(lod.n(6), lod.n(10), false, pennant, move |u, v, _| tint(u, v));
    s.surface(lod.n(6), lod.n(10), true, pennant, move |u, v, _| tint(u, v));
    s
}

/// The weak spot's patch (in the head's frame): pale, thin, veined casing laid over the back of the head, a little proud
/// of it, covering exactly the simulation's weak cap (`in_patch`). It is drawn in a cold glow of its own (rig.rs), which
/// pulses when the spot is exposed, so that it can be found in the dark from behind.
pub fn patch(lod: Lod) -> Shape {
    let mut s = Shape::default();
    let nape = nape();
    let out = Vec3::new(nape.x, 0., nape.z).normalize_or(Vec3::Z);
    let (side, up) = (Vec3::X, nape.cross(Vec3::X).normalize());
    let place = move |u: f32, v: f32| -> Vec3 {
        let (off, round) = (u * kitchen_sim::titan::WEAK_ARC, v * TAU);
        let way = nape * off.cos() + (side * round.cos() + up * round.sin()) * off.sin();
        let q = on_head_along(way);
        q + Vec3::new(q.x, 0., q.z).normalize_or(out) * 0.0075
    };
    s.surface(lod.n(10).max(4), lod.n(28).max(10), false, place, move |u, v, p| {
        // Pale and veined, the veins a sickly red, the edge going pink into the casing.
        let vein = (1. - smooth((fbm(p.x * 30. + 3., p.y * 30., 0, 77) - 0.5).abs() / 0.035)) * 0.65;
        let _ = v;
        rgba(srgb(0.93, 0.95, 1.0).lerp(srgb(0.75, 0.22, 0.25), vein).lerp(srgb(0.7, 0.35, 0.38), smooth((u - 0.75) / 0.25)))
    });
    // (Whichever way the parametrisation winds, it faces out of the head.)
    let mid = s.normals.len() / 2;
    if s.normals.get(mid).is_some_and(|n| n.dot(out) < 0.) {
        for n in s.normals.iter_mut() {
            *n = -*n;
        }
        for t in s.indices.chunks_mut(3) {
            t.swap(1, 2);
        }
    }
    s
}

/// Every part's shape at one level, by name (for the tests' and the checks' sake as much as the rig's).
pub struct Shapes {
    pub torso: Shape,
    pub head: Shape,
    pub mouth: Shape,
    pub maw: Shape,
    pub eye: Shape,
    pub bun: Shape,
    pub upper: Shape,
    pub fore: Shape,
    pub hands: [Shape; 2],
    pub thigh: Shape,
    pub shin: Shape,
    pub foot: Shape,
    pub pick: Shape,
    pub patch: Shape,
}
impl Shapes {
    pub fn new(lod: Lod) -> Shapes {
        Shapes {
            torso: torso(lod),
            head: head(lod),
            mouth: mouth(lod),
            maw: maw(lod),
            eye: eye(lod),
            bun: bun(lod),
            upper: upper_arm(lod),
            fore: forearm(lod),
            hands: GRIPS.map(|g| hand(lod, g)),
            thigh: thigh(lod),
            shin: shin(lod),
            foot: foot(lod),
            pick: pick(lod),
            patch: patch(lod),
        }
    }
    /// Triangles drawn for the whole titan (both arms and legs, both eyes, one hand shape each).
    pub fn triangles(&self) -> usize {
        self.torso.triangles() + self.head.triangles() + self.mouth.triangles() + self.maw.triangles() + 2 * self.eye.triangles() + self.bun.triangles() + 2 * (self.upper.triangles() + self.fore.triangles() + self.hands[0].triangles().max(self.hands[1].triangles()) + self.thigh.triangles() + self.shin.triangles() + self.foot.triangles()) + self.pick.triangles() + self.patch.triangles()
    }
}

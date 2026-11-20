// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Building: where a carried loose object lands when it is put down (Q), and how it is locked there. A child module
//! of feast.rs; the client calls `Feast::placement` every frame to show where the object in its player's hands would
//! land, and `Feast::put` lands it there.
//!
//! How it is held (`Pose`): turned about the vertical in eighths of a turn from square to its carrier's facing, and
//! tipped a quarter turn over onto its front or onto its side (a cutting board stands up on its long edge or on end as a
//! wall panel; a cereal box lies on its back or on its side). It lands held as it is, and everything below is worked
//! out from its outline held so. The carrier turns it and tips it (the game's wheel and key; a command's
//! `button::TURN` and `button::TIP` online), and it is held as the map stood it again each time it is picked up.
//!
//! Put down standing still (the hotdog's pace under STILL, its feet on something), an object lands where it is held,
//! squared and snapped to what is there:
//!   - it lies flush on the highest thing under the middle of it (ONTO in from its edges, so that a neighbour that only
//!     an edge reaches over is beside it, not under it): the counter, a deck, a block low enough to reach up to, or a
//!     loose object. On a loose object it is turned square to it and drawn into line with it, centre to centre or edge
//!     to edge, when it is within ONTO of that;
//!   - with no loose object under it, it is turned square to a neighbour it is put beside, and failing that to the
//!     nearest quarter turn of the hotdog's facing (and then by its own turn);
//!   - a neighbour's side within BESIDE of its own (another loose object standing at the same height, a block, the
//!     forcefield), or into it, it is drawn flush against, a HAIR apart, and lined up with that neighbour along the side
//!     within ONTO. Something under one side of it only (that would tip it) stands beside it instead, and it is drawn
//!     out of that however far in it was, as out of a block. A neighbour stood on its edge or on end is as square as
//!     one lying flat;
//!   - then, if its middle has something under it, it is locked where it lands (props.rs `State::Locked`): walking
//!     hotdogs cannot shove it, nothing slides or topples it, and rounds meet it by its material as ever. It is loose
//!     again when it is picked up, thrown by a relish glob's blast, or when something it stands on is taken away or
//!     moved, and then it falls. One whose middle has nothing under it (a board laid across a gap) is let go where it
//!     is, and locks where it comes to rest.
//!
//! On the move, or in the air, it is let go where it is held with the hotdog's own motion (its pace, and in the air
//! its rising or falling speed too), and the physics says where it ends; it locks where it comes to rest. The trigger
//! still throws it (`Feast::toss`).
//!
//! Forts: nothing may be put down in a team's base, where the bacon is scored (`Placement::refused`: the put is refused
//! and the object stays in the hands). Whatever is thrown, knocked or let go into one is moved out of it when it comes
//! to rest (`Feast::clear_bases`): slid out by the nearest side that keeps it on the counter.
//!
//! (The chair that was thrown when put down: the old put lifted an object onto the highest thing under any of five
//! points of its outline, its corners included, so a chair put down beside a crate, or turned a little on top of one,
//! was set on the crate's top with most of it in the air, tipped off and fell, up to 0.8 m/s and 20 cm away.)
use super::{Feast, LIFT, OUT};
use crate::drape::{Canopy, Refusal, NAPKIN};
use crate::map::FLOOR;
use crate::props::{self, Part, State};
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_4};

/// The pace (m/s) under which a hotdog is standing still, for putting down.
pub const STILL: f32 = 0.12;
/// How far a landing spot is drawn to line up with the object under it, and to close a gap to a neighbour's side.
pub const ONTO: f64 = 0.03;
pub const BESIDE: f64 = 0.02;
/// How far a landed object is set above what it rests on, and apart from what it stands against.
pub const HAIR: f64 = 0.0005;
/// How far an object let go on the move is set above what is under it (as it always was).
const DROP: f64 = 0.004;
/// How high above its feet a hotdog reaches to put something on top of a block.
pub const REACH_UP: f64 = 0.3;
/// A neighbour turned within this (radians) of a quarter turn of the landing is square to it.
const SQUARE: f64 = 0.035;
/// How far into a base an outline may reach before it counts as in it (metres), and how far out of a base an object
/// that came to rest in one is moved.
const BASE_SLACK: f64 = 0.001;
const BASE_CLEAR: f64 = 0.005;

/// How a carried object is held (see the module's documentation): `turn` eighths of a turn about the vertical,
/// counter-clockwise seen from above, from square to its carrier's facing; and `tip`, 0 standing as the map stands it,
/// 1 tipped a quarter turn about its own x axis (its top away from the carrier: a cutting board stands on its long
/// edge, a cereal box lies on its back) and 2 a quarter turn about its own z axis (onto its side: a board stands on end,
/// a cereal box lies on its side). A napkin is never tipped (it is cloth). As a byte (`bits`, a command's buttons from
/// `button::TURN` up): the turn in bits 0 to 2, the tip in bits 3 and 4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pose {
    pub turn: u8,
    pub tip: u8,
}
impl Pose {
    /// Turns there are in a whole turn, and ways to tip.
    pub const TURNS: u8 = 8;
    pub const TIPS: u8 = 3;
    pub fn from_bits(bits: u8) -> Pose {
        let tip = (bits >> 3) & 3;
        Pose { turn: bits & 7, tip: if tip < Pose::TIPS { tip } else { 0 } }
    }
    pub fn bits(self) -> u8 {
        (self.turn & 7) | (self.tip.min(Pose::TIPS - 1) << 3)
    }
    /// Turned `by` eighths more (counter-clockwise seen from above; negative, clockwise).
    pub fn turned(self, by: i32) -> Pose {
        Pose { turn: (self.turn as i32 + by).rem_euclid(Pose::TURNS as i32) as u8, ..self }
    }
    /// Tipped the next way: standing, then over onto its front, then onto its side, then standing again.
    pub fn tipped(self) -> Pose {
        Pose { tip: (self.tip + 1) % Pose::TIPS, ..self }
    }
    /// Its turn about the vertical, in radians.
    pub fn angle(self) -> f64 {
        self.turn as f64 * FRAC_PI_4
    }
    /// Its tip, as a rotation [x, y, z, w] in the object's own frame.
    pub fn tilt(self) -> [f64; 4] {
        match self.tip {
            1 => [-FRAC_1_SQRT_2, 0., 0., FRAC_1_SQRT_2],
            2 => [0., 0., FRAC_1_SQRT_2, FRAC_1_SQRT_2],
            _ => [0., 0., 0., 1.],
        }
    }
    /// The rotation of an object held so, its carrier (or its landing's frame) turned by `yaw`.
    pub fn rotation(self, yaw: f64) -> [f64; 4] {
        let turned = props::yaw_rotation(if self.turn == 0 { yaw } else { yaw + self.angle() });
        if self.tip == 0 { turned } else { product(turned, self.tilt()) }
    }
}
/// The rotation `b` and then `a`: the quaternion product a * b, [x, y, z, w].
fn product(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let ([ax, ay, az, aw], [bx, by, bz, bw]) = (a, b);
    [aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz]
}
/// A rotation's three axes in the world (its matrix's columns): where its own x, y and z point.
fn axes(q: [f64; 4]) -> [[f64; 3]; 3] {
    let [x, y, z, w] = q;
    [
        [1. - 2. * (y * y + z * z), 2. * (x * y + w * z), 2. * (x * z - w * y)],
        [2. * (x * y - w * z), 1. - 2. * (x * x + z * z), 2. * (y * z + w * x)],
        [2. * (x * z + w * y), 2. * (y * z - w * x), 1. - 2. * (x * x + y * y)],
    ]
}
/// A point turned by a rotation.
fn turn_point(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let a = axes(q);
    [0, 1, 2].map(|i| a[0][i] * v[0] + a[1][i] * v[1] + a[2][i] * v[2])
}

/// The outline of a shape about its origin, [x0, z0, x1, z1], its height, and whether it is round (turned on a lathe:
/// the same from every side).
fn outline(shape: &props::Shape) -> ([f64; 4], f64, bool) {
    let (a, o) = (shape.aabb, shape.origin);
    ([a[0] - o[0], a[2] - o[2], a[3] - o[0], a[5] - o[2]], a[4] - a[1], shape.spec.parts.iter().any(|part| matches!(part, Part::Lathe { .. })))
}
/// A shape held tipped by `pose` (and not turned): its outline about its origin [x0, z0, x1, z1] in its own frame, its
/// height, whether it is round from above (turned on a lathe and standing upright), and how far its underside is below
/// its origin (0 standing as the map stands it; tipped, its origin is part way up a side).
pub fn posed(shape: &props::Shape, pose: Pose) -> ([f64; 4], f64, bool, f64) {
    let (outline, tall, round) = outline(shape);
    if pose.tip == 0 {
        return (outline, tall, round, 0.);
    }
    let (a, o, tilt) = (shape.aabb, shape.origin, pose.tilt());
    let mut e = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
    for k in 0..8 {
        let corner = [if k & 1 == 0 { a[0] } else { a[3] } - o[0], if k & 2 == 0 { a[1] } else { a[4] } - o[1], if k & 4 == 0 { a[2] } else { a[5] } - o[2]];
        let v = turn_point(tilt, corner);
        (e[0], e[1], e[2], e[3], e[4], e[5]) = (e[0].min(v[0]), e[1].min(v[2]), e[2].max(v[0]), e[3].max(v[2]), e[4].min(v[1]), e[5].max(v[1]));
    }
    ([e[0], e[1], e[2], e[3]], e[5] - e[4], false, e[4])
}
/// A point [x, z] in the frame turned by `yaw` (as a prop of that yaw has its own), and back.
fn into(x: f64, z: f64, yaw: f64) -> [f64; 2] {
    let (s, c) = yaw.sin_cos();
    [x * c - z * s, x * s + z * c]
}
fn out(u: f64, w: f64, yaw: f64) -> [f64; 2] {
    let (s, c) = yaw.sin_cos();
    [u * c + w * s, -u * s + w * c]
}
/// The nearest quarter turn to an angle.
fn quarter(a: f64) -> f64 {
    (a / FRAC_PI_2).round() * FRAC_PI_2
}
/// The box [u0, w0, u1, w1] round some corners.
fn bound(corners: &[[f64; 2]]) -> [f64; 4] {
    corners.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, c| [b[0].min(c[0]), b[1].min(c[1]), b[2].max(c[0]), b[3].max(c[1])])
}
/// Whether two outlines [u0, w0, u1, w1] overlap by more than `by` on both axes.
fn overlap(a: [f64; 4], b: [f64; 4], by: f64) -> bool {
    a[2].min(b[2]) - a[0].max(b[0]) > by && a[3].min(b[3]) - a[1].max(b[1]) > by
}
/// Whether a convex outline (its corners [x, z] in order round it) and a box [x0, z0, x1, z1] overlap by more than `by`:
/// no axis of either separates them (the box's two, and each side of the outline's).
pub fn crosses(corners: &[[f64; 2]], r: [f64; 4], by: f64) -> bool {
    let b = bound(corners);
    if !overlap(b, r, by) {
        return false;
    }
    let boxed = [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]];
    for i in 0..corners.len() {
        let (a, c) = (corners[i], corners[(i + 1) % corners.len()]);
        let (nx, nz) = (a[1] - c[1], c[0] - a[0]);
        let length = nx.hypot(nz);
        if length < 1e-12 {
            continue;
        }
        let along = |p: &[f64; 2]| (p[0] * nx + p[1] * nz) / length;
        let span = |ps: &[[f64; 2]]| ps.iter().map(along).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let ((a0, a1), (b0, b1)) = (span(corners), span(&boxed));
        if a1.min(b1) - a0.max(b0) <= by {
            return false;
        }
    }
    true
}
/// The shift (at most `within`) that lines `a` up with `b` along one axis (`k` 0 for u, 1 for w): centre to centre if
/// it can, or else edge to edge, the nearer.
fn line_up(a: [f64; 4], b: [f64; 4], k: usize, within: f64) -> Option<f64> {
    let centre = (b[k] + b[k + 2] - a[k] - a[k + 2]) / 2.;
    if centre.abs() <= within {
        return Some(centre);
    }
    [b[k] - a[k], b[k + 2] - a[k + 2]].into_iter().filter(|d| d.abs() <= within).min_by(|x, y| x.abs().total_cmp(&y.abs()))
}
/// A prop standing square (one of its own axes upright, as one lying flat, standing on its edge or on end stands): the
/// yaw of its frame (one of four that differ by quarter turns; of the one whose own x axis is level, when its y is
/// upright, so that a turned shape's off-centre parts are where they are), and whether its own y axis is the upright
/// one. None for one tipped over at a slant.
fn square_yaw(q: [f64; 4]) -> Option<(f64, bool)> {
    let a = axes(q);
    let up = (0..3).find(|k| a[*k][1].abs() > 0.995)?;
    let level = a[(up + 2) % 3];
    Some(((-level[2]).atan2(level[0]), up == 1))
}

/// Where the object a hotdog carries would land if it were put down now (`Feast::placement`).
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// The object (its number among the props) and its kind (map.rs LOOSE's order).
    pub prop: usize,
    pub kind: usize,
    /// Where its origin lands (the middle of its underside, as the map stands it: tipped, part way up a side), its yaw
    /// (the turn of the frame it was squared in; `q` adds its own turn and tip), and its rotation as a quaternion [x, y,
    /// z, w].
    pub p: [f64; 3],
    pub yaw: f64,
    pub q: [f64; 4],
    /// The loose object it lands on top of; the one it is drawn flush against.
    pub onto: Option<usize>,
    pub beside: Option<usize>,
    /// Whether it snapped (onto or beside something), rather than only being squared to the hotdog's facing.
    pub snapped: bool,
    /// Locked where it lands (put down standing still, its middle on something). Otherwise it is let go at `p` with
    /// `velocity`, and locks where it comes to rest.
    pub lock: bool,
    pub velocity: [f64; 3],
    /// A napkin: what it would drape over there, when it would drape (drape.rs `canopy`): the loose objects it
    /// covers (none over a block's, a deck's or the counter's edge alone), and its wall's outline and top. None when
    /// it would lie as a napkin. (A drape can still be refused once it lands, for the bacon's spot, a zombie spawn,
    /// a body that cannot be moved out or a base sealed off: `Feast::refusal` says whether it would be, which this
    /// leaves out because in Forts it builds a navigation grid.)
    pub drape: Option<Canopy>,
    /// Why it cannot be put down there at all (`Refusal::Base`: Forts, its outline would reach into a team's base), or
    /// None. A refused put leaves it in the hands.
    pub refused: Option<Refusal>,
}

/// Something near the landing spot: a loose object, or a block (a map's, a drape's wall, the forcefield: its number in
/// `Feast::standing`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Thing {
    Prop(usize),
    Block(usize),
}
/// What holds a landing up: the floor's height, the things it is the top of (none: the ground), whether they hold it on
/// both sides of its middle so that it stands, and what is under its middle.
struct Footing {
    level: f64,
    things: Vec<Thing>,
    stable: bool,
    middle: Option<Thing>,
}
/// A carried object as it is held: its outline about its origin in its own frame (tipped: `posed`), its height, how
/// far its underside is below its origin, its turn from its carrier's facing, and its outline in its carrier's frame
/// (turned by that; a round one's is its own).
struct Held {
    own: [f64; 4],
    tall: f64,
    bottom: f64,
    angle: f64,
    framed: [f64; 4],
}
impl Held {
    fn of(shape: &props::Shape, pose: Pose) -> Held {
        let (own, tall, round, bottom) = posed(shape, pose);
        let angle = pose.angle();
        let framed = if round || pose.turn == 0 { own } else { bound(&[[own[0], own[1]], [own[2], own[1]], [own[2], own[3]], [own[0], own[3]]].map(|[u, w]| out(u, w, angle))) };
        Held { own, tall, bottom, angle, framed }
    }
    /// A point of its own frame in its carrier's (or its landing's) frame.
    fn spin(&self, u: f64, w: f64) -> [f64; 2] {
        if self.angle == 0. { [u, w] } else { out(u, w, self.angle) }
    }
    /// Its corners in the world with its origin at [u, w] of the frame turned by `yaw`, in order round it.
    fn corners(&self, u: f64, w: f64, yaw: f64) -> [[f64; 2]; 4] {
        let o = self.own;
        [[o[0], o[1]], [o[2], o[1]], [o[2], o[3]], [o[0], o[3]]].map(|[a, b]| {
            let [du, dw] = self.spin(a, b);
            out(u + du, w + dw, yaw)
        })
    }
}

impl Feast {
    /// How a hotdog holds what it carries (a napkin, which is cloth, is never tipped).
    pub(crate) fn pose_of(&self, kind: usize, pose: Pose) -> Pose {
        if kind == NAPKIN { Pose { tip: 0, ..pose } } else { pose }
    }
    /// How far ahead of its carrier a loose object is held: OUT for its kind as the map stands it, and further or nearer
    /// by how much further its near side reaches towards the carrier held as it is (`pose`); nearer where a wall is in the
    /// way (its outline short of the wall), and never nearer than 3 cm. A wall is a block, a drape's wall or the
    /// forcefield standing taller than the carrier can reach up onto (REACH_UP above its feet); a lower block, or a deck,
    /// it holds the object out over, to put it down on top.
    pub(crate) fn reach(&self, prop: usize, pose: Pose, feet: [f64; 3], ahead: [f64; 3]) -> f64 {
        let kind = self.props.body(prop).shape;
        let shape = self.props.shape(kind);
        let ([_, _, _, near], _, _) = outline(shape);
        let held = Held::of(shape, self.pose_of(kind, pose));
        let [_, z0, _, z1] = held.framed;
        let (o, d) = (glam::Vec3::new(feet[0] as f32, (feet[1] + LIFT + 0.03) as f32, feet[2] as f32), glam::Vec3::new(ahead[0] as f32, 0., ahead[2] as f32).normalize_or_zero());
        let wall = self
            .standing()
            .iter()
            .filter(|b| b[4] > feet[1] + REACH_UP)
            .filter_map(|b| crate::ray_box(o, d, glam::Vec3::new(b[0] as f32, (FLOOR - 1.) as f32, b[1] as f32), glam::Vec3::new(b[2] as f32, b[4] as f32, b[3] as f32)))
            .fold(crate::map::RANGE as f32, f32::min) as f64;
        let out = if z1 == near { OUT[kind] } else { OUT[kind] - near + z1 };
        out.min(wall + z0.min(-0.045) - 0.005).max(0.03)
    }
    /// Where a carried object is held: its origin and rotation, for a carrier with its feet at `feet` turned by `yaw`,
    /// holding it as `pose` says (`Feast::haul`). Its underside is LIFT above the carrier's feet, the middle of its
    /// outline straight ahead of the carrier, `reach` ahead. (The client puts what its own player carries there online,
    /// before the server's frame says so.)
    pub fn holding(&self, prop: usize, pose: Pose, feet: [f64; 3], yaw: f64) -> ([f64; 3], [f64; 4]) {
        let kind = self.props.body(prop).shape;
        let pose = self.pose_of(kind, pose);
        let held = Held::of(self.props.shape(kind), pose);
        let reach = self.reach(prop, pose, feet, [-yaw.sin(), 0., -yaw.cos()]);
        let side = -(held.framed[0] + held.framed[2]) / 2.;
        let [dx, dz] = if side == 0. { [-yaw.sin() * reach, -yaw.cos() * reach] } else { out(side, -reach, yaw) };
        ([feet[0] + dx, feet[1] + LIFT - held.bottom, feet[2] + dz], pose.rotation(yaw))
    }
    /// The blocks a landing meets: the map's, the drapes' walls and the forcefield while it is up.
    fn standing(&self) -> Vec<[f64; 5]> {
        let field = self.fort.as_ref().filter(|fort| fort.up).map(|fort| fort.field.clone());
        self.level.map.blocks.iter().cloned().chain(self.drapes.iter().map(crate::drape::Drape::block)).chain(field).map(|b| [b.rect[0], b.rect[1], b.rect[2], b.rect[3], FLOOR + b.height]).collect()
    }
    /// Forts: whether an outline on the counter (its corners [x, z] in order round it) reaches into either team's base,
    /// where the bacon is scored and nothing may be put. Never on any other map, nor in Free For All.
    pub fn in_a_base(&self, corners: &[[f64; 2]]) -> bool {
        self.fort.is_some() && self.level.fort.as_ref().is_some_and(|fort| fort.bases.iter().any(|r| crosses(corners, *r, BASE_SLACK)))
    }
    /// Forts: every loose object that has come to rest reaching into a team's base is slid out of it, by the shortest way
    /// that keeps it on the counter, BASE_CLEAR clear of the base, and wakes to settle there. Called every step, after the
    /// props have moved and draped.
    pub(crate) fn clear_bases(&mut self) {
        let (Some(_), Some(fort)) = (&self.fort, &self.level.fort) else {
            return;
        };
        let (bases, [x0, z0, x1, z1]) = (fort.bases, [self.level.x0, self.level.z0, self.level.x1, self.level.z1]);
        for id in 0..self.props.len() {
            if !matches!(self.props.state(id), State::Asleep | State::Locked) {
                continue;
            }
            let e = self.props.extent(id);
            let Some(r) = bases.iter().find(|r| overlap([e[0], e[1], e[2], e[3]], **r, BASE_SLACK)) else {
                continue;
            };
            let ways = [
                ([r[2] + BASE_CLEAR - e[0], 0.], e[2] + r[2] + BASE_CLEAR - e[0] <= x1),
                ([r[0] - BASE_CLEAR - e[2], 0.], e[0] + r[0] - BASE_CLEAR - e[2] >= x0),
                ([0., r[3] + BASE_CLEAR - e[1]], e[3] + r[3] + BASE_CLEAR - e[1] <= z1),
                ([0., r[1] - BASE_CLEAR - e[3]], e[1] + r[1] - BASE_CLEAR - e[3] >= z0),
            ];
            let Some(([dx, dz], _)) = ways.into_iter().filter(|(_, fits)| *fits).min_by(|a, b| (a.0[0].abs() + a.0[1].abs()).total_cmp(&(b.0[0].abs() + b.0[1].abs()))) else {
                continue;
            };
            let (at, q) = (self.props.position(id), self.props.rotation(id));
            let ground = |x: f64, z: f64| self.level.ground(x, z);
            // Over the same ground as it lay on (a step up or down is the physics' to settle: it wakes).
            let lift = (ground(at[0] + dx, at[2] + dz) - ground(at[0], at[2])).max(0.);
            self.props.set_pose(id, [at[0] + dx, at[1] + lift, at[2] + dz], q);
        }
    }
    /// Where the object hotdog `id` carries would land if it were put down now, how it would be turned, what it would
    /// snap to, whether it would be locked there, (a napkin) what it would drape over, and whether it may be put there
    /// at all (see the module's documentation). None when it carries nothing. Nothing is changed: the client calls this
    /// every frame to show the landing, and `put` lands the object exactly there.
    pub fn placement(&self, id: u32) -> Option<Placement> {
        if self.cards.is_some() {
            return None;
        }
        let p = self.player(id).filter(|p| p.alive && p.carry >= 0)?;
        let prop = p.carry as usize;
        if prop >= self.props.len() || self.props.state(prop) != State::Carried {
            return None;
        }
        let kind = self.props.body(prop).shape;
        let napkin = kind == NAPKIN;
        let pose = self.pose_of(kind, p.pose);
        let (feet, facing) = (p.p.map(|v| v as f64), p.yaw as f64);
        let shape = self.props.shape(kind);
        let form = Held::of(shape, pose);
        // Where it is held: its origin, and its underside's height.
        let (origin, _) = self.holding(prop, pose, feet, facing);
        let held = [origin[0], feet[1] + LIFT, origin[2]];
        let [fx0, fz0, fx1, fz1] = form.own;
        let tall = form.tall;
        // Nothing higher than this is reached up onto: a taller block is a wall.
        let top = feet[1] + REACH_UP;
        let blocks = self.standing();
        // The loose objects near enough to matter.
        let span = (fx1 - fx0).hypot(fz1 - fz0) / 2. + ONTO + BESIDE;
        let near: Vec<usize> = (0..self.props.len())
            .filter(|other| *other != prop && matches!(self.props.state(*other), State::Awake | State::Asleep | State::Locked))
            .filter(|other| {
                let (b, r) = (self.props.body(*other), self.props.shape(self.props.body(*other).shape).radius);
                (b.px - held[0]).hypot(b.pz - held[2]) < span + r + 0.05
            })
            .collect();
        let (x0, z0, x1, z1) = (self.level.x0, self.level.z0, self.level.x1, self.level.z1);
        // The highest thing under a point that the object could be set on, leaving out `skip`, and what it is (None:
        // the ground; off the counter, nothing at all).
        let floor = |x: f64, z: f64, skip: &[Thing]| -> (f64, Option<Thing>) {
            if x < x0 || x > x1 || z < z0 || z > z1 {
                return (f64::NEG_INFINITY, None);
            }
            let mut best = (self.level.ground(x, z), None);
            for (i, b) in blocks.iter().enumerate() {
                if x >= b[0] && x <= b[2] && z >= b[1] && z <= b[3] && b[4] <= top && b[4] > best.0 && !skip.contains(&Thing::Block(i)) {
                    best = (b[4], Some(Thing::Block(i)));
                }
            }
            if top > best.0 {
                let only: Vec<usize> = near.iter().copied().filter(|q| !skip.contains(&Thing::Prop(*q))).collect();
                if let Some(hit) = self.props.raycast_only([x, top, z], [0., -1., 0.], top - best.0, &only) {
                    best = (hit.point[1], Some(Thing::Prop(hit.id)));
                }
            }
            best
        };
        // The floor under the outline (all of it for a napkin, which lies over anything; for anything else the part
        // ONTO in from its edges, or 40% of the way in for a small one), with the origin at frame point [u, w] turned
        // by `yaw` (the object turned by its own turn from that), leaving out `skip`.
        let grid = |u: f64, w: f64, yaw: f64, skip: &[Thing]| -> Footing {
            let inset = |size: f64| if napkin { 0. } else { ONTO.min(size * 0.4) };
            let (iu, iw) = (inset(fx1 - fx0), inset(fz1 - fz0));
            let (nu, nw) = ((((fx1 - fx0) / 0.08).ceil() as usize).clamp(2, 8), (((fz1 - fz0) / 0.08).ceil() as usize).clamp(2, 8));
            let mut samples = vec![];
            for i in 0..=nu {
                for j in 0..=nw {
                    let (lu, lw) = (fx0 + iu + (fx1 - fx0 - 2. * iu) * i as f64 / nu as f64, fz0 + iw + (fz1 - fz0 - 2. * iw) * j as f64 / nw as f64);
                    let [du, dw] = form.spin(lu, lw);
                    let [x, z] = out(u + du, w + dw, yaw);
                    samples.push((lu, lw, floor(x, z, skip)));
                }
            }
            let level = samples.iter().map(|s| s.2 .0).fold(f64::NEG_INFINITY, f64::max);
            let (mu, mw) = ((fx0 + fx1) / 2., (fz0 + fz1) / 2.);
            let mut things: Vec<Thing> = vec![];
            let mut held = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for (lu, lw, (f, thing)) in &samples {
                if *f > level - 0.005 {
                    held = [held[0].min(*lu), held[1].min(*lw), held[2].max(*lu), held[3].max(*lw)];
                    if let Some(t) = thing.filter(|t| !things.contains(t)) {
                        things.push(t);
                    }
                }
            }
            // It stands if what holds it up there is on both sides of its middle (or under it).
            let stable = level > f64::NEG_INFINITY && mu >= held[0] - 0.01 && mu <= held[2] + 0.01 && mw >= held[1] - 0.01 && mw <= held[3] + 0.01;
            let [du, dw] = form.spin(mu, mw);
            let [x, z] = out(u + du, w + dw, yaw);
            let middle = floor(x, z, skip);
            Footing { level, things, stable, middle: middle.1 }
        };
        let pace = p.pace.map(|v| v as f64);
        // On the move or in the air: let go where it is held, with the hotdog's motion.
        if pace[0].hypot(pace[1]) >= STILL as f64 || !p.grounded {
            let [u, w] = into(held[0], held[2], facing);
            let ground = grid(u, w, facing, &[]).level;
            let y = if ground == f64::NEG_INFINITY {
                held[1]
            } else if p.grounded {
                ground + DROP
            } else {
                held[1].max(ground + DROP)
            };
            let velocity = [pace[0], if p.grounded { 0. } else { p.v[1] as f64 }, pace[1]];
            let at = [held[0], y - form.bottom, held[2]];
            let drape = self.drapes_over(prop, kind, at, facing + form.angle);
            let refused = self.in_a_base(&form.corners(u, w, facing)).then_some(Refusal::Base);
            return Some(Placement { prop, kind, p: at, yaw: facing, q: pose.rotation(facing), onto: None, beside: None, snapped: false, lock: false, velocity, drape, refused });
        }
        // Standing still. A prop's yaw and whether it stands square (flat, on its edge or on end), whether it is round
        // (turned on a lathe, standing upright), and a thing's outline in the frame turned by `yaw` (whose origin is the
        // world's); with `square`, None for one not square to that frame (or lying over at a slant).
        let squared = |q: usize| square_yaw(self.props.rotation(q));
        let yaw_of = |q: usize| squared(q).map_or(0., |(yaw, _)| yaw);
        let upright = |q: usize| squared(q).is_some();
        let round = |q: usize| outline(self.props.shape(self.props.body(q).shape)).2 && squared(q).is_some_and(|(_, up)| up);
        let rect_of = |thing: Thing, yaw: f64, square: bool| -> Option<[f64; 4]> {
            match thing {
                Thing::Block(i) => {
                    let b = blocks[i];
                    if square && (quarter(yaw) - yaw).abs() > SQUARE {
                        return None;
                    }
                    Some(bound(&[[b[0], b[1]], [b[2], b[1]], [b[0], b[3]], [b[2], b[3]]].map(|[x, z]| into(x, z, yaw))))
                }
                Thing::Prop(q) => {
                    if !upright(q) {
                        // Lying over: the box round where its samples reach (never square).
                        let e = self.props.extent(q);
                        return (!square).then(|| bound(&[[e[0], e[1]], [e[2], e[1]], [e[0], e[3]], [e[2], e[3]]].map(|[x, z]| into(x, z, yaw))));
                    }
                    let turn = yaw_of(q) - yaw;
                    if round(q) {
                        let ([a0, b0, a1, b1], _, _) = outline(self.props.shape(self.props.body(q).shape));
                        let at = self.props.position(q);
                        let [u, w] = into(at[0], at[2], yaw);
                        let r = (a1 - a0).max(b1 - b0) / 2.;
                        let [cu, cw] = out((a0 + a1) / 2., (b0 + b1) / 2., turn);
                        return Some([u + cu - r, w + cw - r, u + cu + r, w + cw + r]);
                    }
                    if square && (quarter(turn) - turn).abs() > SQUARE {
                        return None;
                    }
                    // The corners of its box, where they are, in the frame.
                    let (b, shape) = (self.props.body(q), self.props.shape(self.props.body(q).shape));
                    let (a, rot) = (shape.aabb, self.props.rotation(q));
                    let corners: Vec<[f64; 2]> = (0..8)
                        .map(|k| {
                            let v = turn_point(rot, [if k & 1 == 0 { a[0] } else { a[3] }, if k & 2 == 0 { a[1] } else { a[4] }, if k & 4 == 0 { a[2] } else { a[5] }]);
                            into(b.px + v[0], b.pz + v[2], yaw)
                        })
                        .collect();
                    Some(bound(&corners))
                }
            }
        };
        let mine = |u: f64, w: f64| [u + form.framed[0], w + form.framed[1], u + form.framed[2], w + form.framed[3]];
        // Square to a prop: the quarter turn of its yaw nearest the hotdog's facing (a round one, or one lying over,
        // has no sides to square to: the facing's quarter turn).
        let square_to = |q: usize| if round(q) || !upright(q) { quarter(facing) } else { yaw_of(q) + quarter(facing - yaw_of(q)) };
        let mut yaw = quarter(facing);
        let [mut u, mut w] = into(held[0], held[2], yaw);
        // What holds it up: the highest floor under it that has it standing. Whatever is higher under one side only
        // stands beside it instead (it is drawn out of that below), and is left out of its floor.
        let mut skip: Vec<Thing> = vec![];
        let mut ground = grid(u, w, yaw, &skip);
        while !ground.stable && !napkin && ground.level > f64::NEG_INFINITY && !ground.things.is_empty() && skip.len() < 8 {
            skip.extend(ground.things.iter().copied());
            ground = grid(u, w, yaw, &skip);
        }
        let mut fixed = [false; 2];
        // 1. On a loose object (the one under its middle, or failing that the first that holds it): square to it and
        // lined up with it.
        let support = ground.middle.filter(|t| ground.things.contains(t)).or(ground.things.first().copied());
        let mut onto = None;
        if let Some(Thing::Prop(q)) = support {
            if upright(q) {
                yaw = square_to(q);
                [u, w] = into(held[0], held[2], yaw);
                if let Some(r) = rect_of(Thing::Prop(q), yaw, false) {
                    let me = mine(u, w);
                    for k in 0..2 {
                        if let Some(shift) = line_up(me, r, k, ONTO) {
                            if k == 0 { u += shift } else { w += shift }
                            fixed[k] = true;
                        }
                    }
                }
                onto = Some(q);
            }
        }
        let level = grid(u, w, yaw, &skip).level;
        // The things standing beside it at that height.
        let beside_it = |level: f64| -> Vec<Thing> {
            let props = near.iter().filter(|q| {
                let e = self.props.extent(**q);
                e[5] > level + 0.002 && e[4] < level + tall - 0.002
            });
            props.map(|q| Thing::Prop(*q)).chain((0..blocks.len()).filter(|i| blocks[*i][4] > level + 0.002).map(Thing::Block)).collect()
        };
        let mut around = beside_it(level);
        // 2. Nothing under it to square to: square to the nearest neighbour it is put against.
        if onto.is_none() {
            let me = mine(u, w);
            let gap = |r: [f64; 4]| (r[0] - me[2]).max(me[0] - r[2]).max(r[1] - me[3]).max(me[1] - r[3]);
            let nearest = around
                .iter()
                .filter_map(|t| match t {
                    Thing::Prop(q) if upright(*q) && !round(*q) => rect_of(*t, yaw, false).map(|r| (gap(r), *q)),
                    _ => None,
                })
                .filter(|(g, _)| *g <= BESIDE)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, q)) = nearest {
                yaw = square_to(q);
                [u, w] = into(held[0], held[2], yaw);
                around = beside_it(level);
            }
        }
        // 3. Flush against the sides within BESIDE of its own, or a little into them (out of a block, or out of what
        // was under one side of it, however far in), one axis at a time, the nearest first; then lined up along the
        // side it was first drawn to. A thing not square to it only pushes it out.
        let mut beside = None;
        let mut first: Option<(usize, Thing)> = None;
        for _ in 0..2 {
            let me = mine(u, w);
            let mut best: Option<(f64, usize, Thing)> = None;
            for t in &around {
                let (rect, square) = match rect_of(*t, yaw, true) {
                    Some(r) => (r, true),
                    None => match rect_of(*t, yaw, false) {
                        Some(r) => (r, false),
                        None => continue,
                    },
                };
                // (Anything further in than the edge of the floor it was measured over would have been under it.)
                let deep = if matches!(t, Thing::Block(_)) || skip.contains(t) { f64::INFINITY } else { ONTO + 0.01 };
                for k in 0..2 {
                    let other = 1 - k;
                    // They face each other across axis k.
                    if me[other + 2].min(rect[other + 2]) - me[other].max(rect[other]) <= 0.003 {
                        continue;
                    }
                    let before = me[k] + me[k + 2] < rect[k] + rect[k + 2];
                    let gap = if before { rect[k] - me[k + 2] } else { me[k] - rect[k + 2] };
                    if gap > if square { BESIDE } else { 0. } || gap < -deep.min(me[k + 2] - me[k]) {
                        continue;
                    }
                    // A side already lined up on what is under it, or drawn flush, stays (unless it is in something).
                    if fixed[k] && gap >= 0. {
                        continue;
                    }
                    let shift = if before { gap - HAIR } else { HAIR - gap };
                    if best.is_none_or(|(s, _, _)| shift.abs() < s.abs()) {
                        best = Some((shift, k, *t));
                    }
                }
            }
            let Some((shift, k, t)) = best else { break };
            if k == 0 { u += shift } else { w += shift }
            fixed[k] = true;
            if let Thing::Prop(q) = t {
                beside = beside.or(Some(q));
                first = first.or(Some((k, t)));
            }
        }
        if let Some((k, t)) = first {
            let other = 1 - k;
            if !fixed[other] {
                if let Some(r) = rect_of(t, yaw, true) {
                    let shift = line_up(mine(u, w), r, other, ONTO).unwrap_or(0.);
                    let mut moved = mine(u, w);
                    (moved[other], moved[other + 2]) = (moved[other] + shift, moved[other + 2] + shift);
                    // Unless that would take it into another neighbour.
                    if !around.iter().filter(|o| **o != t).filter_map(|o| rect_of(*o, yaw, false)).any(|r| overlap(moved, r, 0.003)) {
                        if other == 0 { u += shift } else { w += shift }
                    }
                }
            }
        }
        // 4. Down on whatever holds it up there.
        let ground = grid(u, w, yaw, &skip);
        let [x, z] = out(u, w, yaw);
        let landing = [x, if ground.level > f64::NEG_INFINITY { ground.level + HAIR } else { held[1] } - form.bottom, z];
        let onto = match ground.middle.filter(|t| ground.things.contains(t)).or(ground.things.first().copied()) {
            Some(Thing::Prop(q)) => Some(q),
            _ => None,
        };
        // Locked where it lands if it stands there and nothing beside it is still in its way.
        let crowded = beside_it(ground.level).iter().filter_map(|t| rect_of(*t, yaw, false)).any(|r| overlap(mine(u, w), r, 0.003));
        let lock = ground.stable && !crowded;
        let drape = self.drapes_over(prop, kind, landing, yaw + form.angle);
        let refused = self.in_a_base(&form.corners(u, w, yaw)).then_some(Refusal::Base);
        Some(Placement { prop, kind, p: landing, yaw, q: pose.rotation(yaw), onto, beside, snapped: onto.is_some() || beside.is_some(), lock, velocity: [0.; 3], drape, refused })
    }
    /// A napkin landing at `at` turned by `yaw`: what it would drape over there.
    fn drapes_over(&self, prop: usize, kind: usize, at: [f64; 3], yaw: f64) -> Option<Canopy> {
        (kind == NAPKIN).then(|| self.canopy(prop, Some((at, yaw)))).flatten()
    }
    /// Lets the carried object down where `placement` says (see the module's documentation), unless it may not be put
    /// there (`Placement::refused`). `_direction` is where the hotdog looks, which does not matter: standing still, the
    /// same spot every time.
    pub fn put(&mut self, id: u32, _direction: glam::Vec3) -> bool {
        let Some(spot) = self.placement(id).filter(|spot| spot.refused.is_none()) else {
            return false;
        };
        self.props.carry(spot.prop, spot.p, spot.q);
        self.props.drop_prop(spot.prop, spot.velocity, [0.; 3]);
        if spot.lock {
            self.props.lock(spot.prop);
        } else {
            self.props.lock_at_rest(spot.prop);
        }
        if let Some(p) = self.player_mut(id) {
            (p.carry, p.weapon) = (-1, Self::hand(p));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::Pose;
    use crate::feast::*;
    use crate::map::Map;
    use crate::props::State;
    use glam::Vec3;

    const DT: f32 = 1. / 30.;
    const F: f32 = FLOOR;
    // long-fort.json's loose objects: three chairs, four crates, five pillows, three napkins a side.
    const CHAIR: usize = 0;
    const CRATE: usize = 3;
    const PILLOW: usize = 7;
    const NAPKIN: usize = 12;
    fn read(path: &str) -> String {
        std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }
    fn fort() -> Feast {
        let mut s = Feast::with_map(Map::parse(&read("tools/network/fixtures/long-fort.json")).unwrap(), false);
        s.set_build(600.);
        for p in &mut s.players {
            p.bot = false;
        }
        s
    }
    fn run(s: &mut Feast, seconds: f32) {
        for _ in 0..(seconds * 30.).round() as u32 {
            s.tick(DT);
        }
    }
    fn at(s: &mut Feast, id: u32, x: f32, z: f32) {
        s.player_mut(id).unwrap().p = [x, F, z];
    }
    fn look(s: &mut Feast, id: u32, d: [f32; 3]) -> Vec3 {
        let d = Vec3::from_array(d).normalize();
        let p = s.player_mut(id).unwrap();
        (p.yaw, p.pitch) = ((-d.x).atan2(-d.z), d.y.asin());
        d
    }
    fn pose(s: &mut Feast, prop: usize, p: [f64; 3], yaw: f64) {
        s.props.set_pose(prop, p, crate::props::yaw_rotation(yaw));
    }

    /// Measures a Forts map's cost with all its stock out (`BUILD_MAPS` names the map files, comma separated): the
    /// first second (everything put out awake, settling), a build phase with the bots at work and the piles at rest,
    /// and the same with five pillows thrown every 20 steps.
    #[test]
    #[ignore]
    fn measure_the_stock() {
        let maps = std::env::var("BUILD_MAPS").unwrap_or_else(|_| "assets/maps/pillow-fort.json".into());
        for path in maps.split(',') {
            let text = std::fs::read_to_string(path).unwrap_or_else(|_| read(path));
            let map = Map::parse(&text).unwrap();
            let pillows = map.loose.iter().filter(|l| l.kind == "pillow").count();
            let mut s = Feast::with_map(map, false);
            s.set_build(600.);
            let time = |s: &mut Feast, dt: f32| {
                let from = std::time::Instant::now();
                s.tick(dt);
                from.elapsed().as_secs_f64() * 1e3
            };
            let first: Vec<f64> = (0..30).map(|_| time(&mut s, DT)).collect();
            let awake = |s: &Feast| (0..s.props.len()).filter(|i| s.props.state(*i) == State::Awake).count();
            let settled_after = (0..300).position(|_| {
                s.tick(DT);
                awake(&s) == 0
            });
            run(&mut s, 2.);
            let mut rest: Vec<f64> = (0..300).map(|_| time(&mut s, DT)).collect();
            let mut rest60: Vec<f64> = (0..300).map(|_| time(&mut s, 1. / 60.)).collect();
            let restless: Vec<String> = (0..s.props.len()).filter(|i| s.props.state(*i) == State::Awake).take(6).map(|i| format!("{} {} at {:?} v {:?}", i, crate::map::LOOSE[s.props.body(i).shape], s.props.position(i), s.props.velocity(i))).collect();
            println!("AWAKE {path}: {} awake: {restless:?}", awake(&s));
            // The tops of the pillow stacks.
            let piled: Vec<usize> = (0..s.props.len())
                .filter(|i| s.props.body(*i).shape == 2 && matches!(s.props.state(*i), State::Asleep | State::Locked))
                .filter(|i| {
                    let p = s.props.position(*i);
                    !(0..s.props.len()).any(|j| j != *i && s.props.body(j).shape == 2 && {
                        let q = s.props.position(j);
                        (q[0] - p[0]).abs() < 0.01 && (q[2] - p[2]).abs() < 0.01 && q[1] > p[1] + 0.01
                    })
                })
                .collect();
            let (mut moving, mut woke, mut props_only) = (vec![], vec![], vec![]);
            for k in 0..300 {
                if k % 20 == 0 {
                    for j in 0..5 {
                        let id = piled[(k * 7 + j * 31) % piled.len()];
                        s.props.set_velocity(id, [0.3, 0.6, 0.2], [0.; 3]);
                    }
                }
                moving.push(time(&mut s, DT));
                woke.push(awake(&s));
                if k % 20 == 19 {
                    let mut copy = s.props.clone();
                    let from = std::time::Instant::now();
                    copy.step(&s.level, DT as f64);
                    props_only.push(from.elapsed().as_secs_f64() * 1e3);
                }
            }
            props_only.sort_by(f64::total_cmp);
            println!("MOVING {path}: awake on average {:.1}, at most {}; the props' own step at the 20th step {:.3} ms median", woke.iter().sum::<usize>() as f64 / woke.len() as f64, woke.iter().max().unwrap(), props_only[props_only.len() / 2]);
            for v in [&mut rest, &mut rest60, &mut moving] {
                v.sort_by(f64::total_cmp);
            }
            let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
            println!(
                "MEASURE {path}: {} props, {pillows} pillows; first second mean {:.2} ms, worst {:.2} ms, all asleep after {:?} more steps; at rest median {:.3} ms (1/60 s steps {:.3} ms), p90 {:.3}; five thrown every 20 steps median {:.3} ms, p90 {:.3}, worst {:.3}",
                s.props.len(),
                mean(&first),
                first.iter().cloned().fold(0., f64::max),
                settled_after,
                rest[150],
                rest60[150],
                rest[270],
                moving[150],
                moving[270],
                moving[299]
            );
        }
    }

    /// Hotdog 1 takes `prop` in its hands (as `grab` does), and stands at `at` facing `facing` ([x, z]), still.
    fn carry(s: &mut Feast, prop: usize, at: [f32; 2], facing: [f32; 2]) {
        assert!(s.props.pick_up(prop, 1), "picked up {prop}");
        if let Some(p) = s.player_mut(1) {
            (p.carry, p.weapon) = (prop as i32, 0);
        }
        self::at(s, 1, at[0], at[1]);
        look(s, 1, [facing[0], 0., facing[1]]);
        // Stood there, not walked there: no pace.
        let p = s.player_mut(1).unwrap();
        (p.pace, p.last) = ([0.; 2], Some(p.p));
        s.tick(DT);
    }
    fn moved(a: [f64; 3], b: [f64; 3]) -> f64 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    /// The chair that was thrown: put down standing still, in the open, beside a crate, on one, on two, half over one,
    /// with a corner over one, near the forcefield (facing it and at a slant) and just after turning, it lands where the
    /// placement says, locked, upright, and does not move afterwards.
    #[test]
    fn a_chair_put_down_standing_still_stays_where_it_lands_beside_things_on_a_stack_near_a_wall_and_after_turning() {
        let cases: [(&str, fn(&mut Feast)); 6] = [
            ("in the open", |_s| {}),
            ("beside a crate", |s| pose(s, CRATE, [-1.2 + 0.105 + 0.05 + 0.045, F as f64, 2.43], 0.3)),
            ("on a crate", |s| pose(s, CRATE, [-1.2 + 0.105, F as f64, 2.4], 0.)),
            ("on two crates", |s| {
                pose(s, CRATE, [-1.2 + 0.105, F as f64, 2.4], 0.);
                pose(s, CRATE + 1, [-1.2 + 0.105, F as f64 + 0.1, 2.4], 0.);
            }),
            ("half over a crate", |s| pose(s, CRATE, [-1.2 + 0.105 + 0.06, F as f64, 2.4], 0.)),
            ("a corner over a crate", |s| pose(s, CRATE, [-1.2 + 0.105 + 0.09, F as f64, 2.49], 0.)),
        ];
        let mut checked = 0;
        for (name, setup) in cases {
            for turned in [false, true] {
                let mut s = fort();
                setup(&mut s);
                carry(&mut s, CHAIR, [-1.2, 2.4], [1., 0.]);
                run(&mut s, 0.5);
                if turned {
                    for k in 0..6 {
                        look(&mut s, 1, [1., 0., -0.2 * k as f32]);
                        s.tick(DT);
                    }
                    look(&mut s, 1, [1., 0., 0.4]);
                }
                let spot = s.placement(1).unwrap();
                assert!(s.put(1, Vec3::X));
                let from = s.props.position(CHAIR);
                assert!(moved(from, spot.p) < 1e-9, "{name}: landed at {from:?}, the placement said {:?}", spot.p);
                let mut far = 0f64;
                for _ in 0..90 {
                    s.tick(DT);
                    far = far.max(moved(s.props.position(CHAIR), from));
                }
                assert!(far < 0.005 && s.props.up(CHAIR)[1] > 0.999 && s.props.state(CHAIR) == State::Locked, "{name}, turned {turned}: moved {far} m, up {}, {:?}", s.props.up(CHAIR)[1], s.props.state(CHAIR));
                checked += 1;
            }
        }
        for dir in [[1f32, 0.], [1., 0.6]] {
            let mut s = fort();
            carry(&mut s, CHAIR, [-0.16, 3.], dir);
            run(&mut s, 0.5);
            assert!(s.put(1, Vec3::X));
            let from = s.props.position(CHAIR);
            run(&mut s, 3.);
            assert!(moved(s.props.position(CHAIR), from) < 0.005 && s.props.up(CHAIR)[1] > 0.999, "near the field, facing {dir:?}");
            assert!(s.props.extent(CHAIR)[2] < -0.04, "short of the field: {:?}", s.props.extent(CHAIR));
            checked += 1;
        }
        assert_eq!(checked, 14);
    }

    /// Put down on the move, an object keeps the hotdog's pace (and slides on, then locks where it stops); put down in
    /// the air it keeps its rising speed too, and goes on up before it falls.
    #[test]
    fn put_down_on_the_move_it_keeps_the_hotdogs_pace_and_in_the_air_its_rising_speed_too() {
        let mut s = fort();
        carry(&mut s, CRATE, [-1.6, 2.6], [1., 0.]);
        for _ in 0..20 {
            s.advance_actor(1, Vec3::new(0., F, 2.6), DT);
            s.tick(DT);
        }
        let pace = s.player(1).unwrap().pace;
        assert!(pace[0] > 0.2, "walking at {pace:?}");
        let spot = s.placement(1).unwrap();
        assert!(!spot.lock && spot.velocity == [pace[0] as f64, 0., pace[1] as f64], "{spot:?}");
        let held = s.props.position(CRATE);
        assert!(s.put(1, Vec3::X));
        assert_eq!(s.props.velocity(CRATE), [pace[0] as f64, 0., pace[1] as f64]);
        run(&mut s, 3.);
        let slid = s.props.position(CRATE)[0] - held[0];
        assert!(slid > 0.005 && slid < 0.3 && s.props.state(CRATE) == State::Locked, "slid {slid} m and locked: {:?}", s.props.state(CRATE));
        // A jump: let go on the way up, it goes on up.
        let mut s = fort();
        carry(&mut s, PILLOW, [-1.6, 2.6], [1., 0.]);
        run(&mut s, 0.3);
        s.jump(1, 1.);
        let mut rising = 0.;
        for _ in 0..4 {
            s.tick(DT);
            rising = s.player(1).unwrap().v[1];
            if !s.player(1).unwrap().grounded && rising > 0.1 {
                break;
            }
        }
        assert!(!s.player(1).unwrap().grounded && rising > 0.1, "in the air, rising at {rising}");
        let spot = s.placement(1).unwrap();
        assert!(!spot.lock && (spot.velocity[1] - rising as f64).abs() < 1e-6, "{spot:?}");
        let from = spot.p[1];
        assert!(s.put(1, Vec3::X));
        let mut top = from;
        for _ in 0..30 {
            s.tick(DT);
            top = top.max(s.props.position(PILLOW)[1]);
        }
        assert!(top > from + 0.005, "it went on up: from {from} to {top}");
        run(&mut s, 3.);
        assert!((s.props.position(PILLOW)[1] - F as f64).abs() < 0.003 && s.props.state(PILLOW) == State::Locked, "down, at rest, locked");
    }

    /// Snapping. Onto: held a little off a crate turned 0.2 rad, a pillow lands on its top, squared to it and lined up
    /// with its middle. Beside: held 1.5 cm short of a crate's side, it is drawn flush against it, squared to it. With
    /// nothing near, it is squared to the nearest quarter turn of the hotdog's facing, where it is held.
    #[test]
    fn snapping_lines_an_object_up_on_top_of_another_draws_it_flush_against_a_neighbour_and_squares_it() {
        let quarterly = |a: f64, b: f64| {
            let d = (a - b) / std::f64::consts::FRAC_PI_2;
            (d - d.round()).abs() < 1e-6
        };
        // Onto.
        let mut s = fort();
        pose(&mut s, CRATE, [-1.2 + 0.11 + 0.02, F as f64, 2.41], 0.2);
        carry(&mut s, PILLOW, [-1.2, 2.4], [1., 0.]);
        run(&mut s, 0.3);
        let spot = s.placement(1).unwrap();
        assert_eq!((spot.onto, spot.lock), (Some(CRATE), true), "{spot:?}");
        assert!(quarterly(spot.yaw, 0.2), "squared to the crate: {}", spot.yaw);
        let c = s.props.position(CRATE);
        assert!((spot.p[0] - c[0]).abs() < 1e-6 && (spot.p[2] - c[2]).abs() < 1e-6 && (spot.p[1] - (c[1] + 0.1 + super::HAIR)).abs() < 1e-6, "on its middle: {spot:?} {c:?}");
        // Beside.
        let mut s = fort();
        pose(&mut s, CRATE, [-1.2 + 0.11 + 0.05 + 0.015 + 0.05, F as f64, 2.4], 0.03);
        carry(&mut s, PILLOW, [-1.2, 2.4], [1., 0.]);
        run(&mut s, 0.3);
        let spot = s.placement(1).unwrap();
        assert_eq!((spot.onto, spot.beside, spot.lock, spot.snapped), (None, Some(CRATE), true, true), "{spot:?}");
        assert!(quarterly(spot.yaw, 0.03), "squared to the crate: {}", spot.yaw);
        assert!(s.put(1, Vec3::X));
        // Flush: its side a hair from the crate's, along the crate's own axis.
        let (c, p) = (s.props.position(CRATE), s.props.position(PILLOW));
        let (sin, cos) = 0.03f64.sin_cos();
        let along = (p[0] - c[0]) * cos - (p[2] - c[2]) * sin;
        let across = (p[0] - c[0]) * sin + (p[2] - c[2]) * cos;
        // (The pillow is turned a quarter to the crate: its 10 cm side faces the crate's.)
        assert!((along.abs() - (0.05 + 0.05 + super::HAIR)).abs() < 1e-6 && across.abs() < 0.03, "flush: {along} {across}");
        // Nothing near: square to the facing's quarter turn, where it is held.
        let mut s = fort();
        carry(&mut s, PILLOW, [-1.2, 2.4], [1., 0.4]);
        run(&mut s, 0.3);
        let spot = s.placement(1).unwrap();
        assert_eq!((spot.onto, spot.beside, spot.snapped, spot.lock), (None, None, false, true));
        assert!((spot.yaw + std::f64::consts::FRAC_PI_2).abs() < 1e-9, "a quarter turn: {}", spot.yaw);
        let held = s.props.position(PILLOW);
        assert!((spot.p[0] - held[0]).abs() < 1e-9 && (spot.p[2] - held[2]).abs() < 1e-9 && (spot.p[1] - (crate::map::FLOOR + super::HAIR)).abs() < 1e-9, "{spot:?} held at {held:?}");
    }

    /// Locked: a crate put down is not shoved by a hotdog walking into it for three seconds. A pillow put on it is
    /// locked on it, and falls once the crate is picked up from under it. A relish glob bursting beside a locked crate
    /// throws it, loose again.
    #[test]
    fn a_locked_object_is_not_shoved_by_a_walking_hotdog_and_comes_loose_when_blasted_or_when_what_it_stands_on_goes() {
        let mut s = fort();
        carry(&mut s, CRATE, [-1.2, 2.6], [1., 0.]);
        assert!(s.put(1, Vec3::X));
        let at = s.props.position(CRATE);
        assert_eq!(s.props.state(CRATE), State::Locked);
        self::at(&mut s, 3, -0.7, 2.6);
        for _ in 0..90 {
            s.advance_actor(3, Vec3::new(-1.5, F, 2.6), DT);
            s.tick(DT);
            let p = s.player(3).unwrap().p;
            assert!(!(p[0] > at[0] as f32 - 0.05 && p[0] < at[0] as f32 + 0.05 && (p[2] - 2.6).abs() < 0.05), "the walker is not inside it");
        }
        assert!(moved(s.props.position(CRATE), at) < 1e-9 && s.props.state(CRATE) == State::Locked, "not shoved: {:?}", s.props.position(CRATE));
        // A pillow on it, locked on it; the crate picked up from under it, and it falls.
        carry(&mut s, PILLOW, [-1.2, 2.6], [1., 0.]);
        let spot = s.placement(1).unwrap();
        assert_eq!((spot.onto, spot.lock), (Some(CRATE), true));
        assert!(s.put(1, Vec3::X));
        assert_eq!(s.props.state(PILLOW), State::Locked);
        assert_eq!(s.props.body(PILLOW).supports.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [CRATE]);
        run(&mut s, 1.);
        assert_eq!(s.props.state(PILLOW), State::Locked);
        self::at(&mut s, 3, at[0] as f32 + 0.15, 2.6);
        let d = look(&mut s, 3, [-1., -0.7, 0.]);
        assert!(s.grab(3, d));
        assert_eq!(s.player(3).unwrap().carry, CRATE as i32);
        s.tick(DT);
        assert_eq!(s.props.state(PILLOW), State::Awake, "loose once what it stood on went");
        run(&mut s, 2.);
        assert!((s.props.position(PILLOW)[1] - F as f64).abs() < 0.003, "and down on the counter: {:?}", s.props.position(PILLOW));
        // Blasted.
        let mut s = fort();
        carry(&mut s, CRATE, [-1.2, 2.6], [1., 0.]);
        assert!(s.put(1, Vec3::X));
        let at = s.props.position(CRATE);
        s.props.blast([at[0] + 0.12, at[1] + 0.05, at[2]], crate::actor::BLAST_RADIUS, crate::actor::BLAST_IMPULSE, crate::actor::BLAST_LIFT);
        assert_eq!(s.props.state(CRATE), State::Awake, "the blast unlocks it");
        run(&mut s, 0.5);
        assert!(moved(s.props.position(CRATE), at) > 0.02, "and throws it");
        // A round only nudges a loose object; a locked one it does not move.
        let mut s = fort();
        carry(&mut s, CRATE, [-1.2, 2.6], [1., 0.]);
        assert!(s.put(1, Vec3::X));
        assert!(!s.props.apply_impulse(CRATE, [0.05, 0., 0.], None) && s.props.state(CRATE) == State::Locked);
    }

    /// The preview is the landing: in the open, on a crate, beside one, against the forcefield and over a pile (a
    /// napkin), the object lands exactly where `placement` said, turned as it said, and is still there two seconds on;
    /// the napkin drapes over what the placement said it would.
    #[test]
    fn the_placement_preview_is_where_the_object_lands_and_stays() {
        let setups: [(&str, usize, [f32; 2], [f32; 2], fn(&mut Feast)); 5] = [
            ("in the open", PILLOW, [-1.2, 2.4], [1., 0.3], |_| {}),
            ("on a crate", CHAIR, [-1.2, 2.4], [1., 0.], |s| pose(s, CRATE, [-1.08, F as f64, 2.42], 0.1)),
            ("beside a crate", CRATE + 1, [-1.2, 2.4], [1., 0.], |s| pose(s, CRATE, [-0.98, F as f64, 2.41], -0.02)),
            ("against the field", CRATE + 1, [-0.14, 3.], [1., 0.5], |_| {}),
            ("a napkin over crates", NAPKIN, [-1.2, 2.6], [1., 0.], |s| {
                for (i, z) in [2.5, 2.6, 2.7].into_iter().enumerate() {
                    pose(s, CRATE + i, [-0.91, F as f64, z], 0.);
                }
            }),
        ];
        for (name, prop, at, facing, setup) in setups {
            let mut s = fort();
            setup(&mut s);
            run(&mut s, 1.);
            carry(&mut s, prop, at, facing);
            run(&mut s, 0.3);
            let spot = s.placement(1).unwrap();
            // (And the drape it previews would not be refused.)
            assert!(spot.drape.as_ref().is_none_or(|c| s.refusal(c, &s.bodies).is_none()), "{name}: {spot:?}");
            assert!(s.put(1, Vec3::X));
            assert!(moved(s.props.position(prop), spot.p) < 1e-9 && (0..4).all(|k| (s.props.rotation(prop)[k] - spot.q[k]).abs() < 1e-9), "{name}: {spot:?}");
            run(&mut s, 2.);
            if prop == NAPKIN {
                let covered = spot.drape.clone().expect("the preview says it drapes").props;
                assert_eq!(s.drapes.len(), 1, "{name}");
                assert_eq!(s.drapes[0].props, covered, "{name}");
            } else {
                assert!(moved(s.props.position(prop), spot.p) < 0.0005 && s.props.state(prop) == State::Locked, "{name}: moved to {:?}", s.props.position(prop));
            }
        }
    }

    /// Five pillows put down one on another, standing still, stand as a column: one over another, each locked on the
    /// one under it; and they go on standing.
    #[test]
    fn five_pillows_put_down_one_on_another_stand_as_a_column() {
        let mut s = fort();
        let pillows: Vec<usize> = (0..s.props.len()).filter(|i| s.props.body(*i).shape == 2).take(5).collect();
        for &p in &pillows {
            // Each held a little off the one before, and from a slightly different facing.
            carry(&mut s, p, [-1.2 + (p % 2) as f32 * 0.015, 2.4], [1., 0.05 * (p % 3) as f32]);
            assert!(s.put(1, Vec3::X));
        }
        run(&mut s, 5.);
        let bottom = s.props.position(pillows[0]);
        for (k, &p) in pillows.iter().enumerate() {
            let at = s.props.position(p);
            assert!((at[0] - bottom[0]).abs() < 1e-6 && (at[2] - bottom[2]).abs() < 1e-6, "pillow {k} at {at:?} over {bottom:?}");
            assert!((at[1] - (F as f64 + super::HAIR + k as f64 * (0.04 + super::HAIR))).abs() < 1e-6, "pillow {k} at height {}", at[1] - F as f64);
            assert_eq!(s.props.state(p), State::Locked);
        }
    }

    /// The kitchen's pieces: each has its material (card and paper light; china, tin, wood, glass and enamel solid), a
    /// name on the screen, and can be carried, put down and snapped flush against a crate, where it is locked.
    #[test]
    fn the_kitchens_pieces_are_made_of_what_they_are_and_can_be_carried_put_down_and_snapped() {
        use crate::props::Material::{Light, Solid};
        let made = [("cereal", Light), ("cup", Solid), ("plate", Solid), ("tin", Solid), ("board", Solid), ("dish", Solid), ("roll", Light), ("salt", Solid), ("pepper", Solid), ("breadbox", Solid)];
        for (kind, material) in made {
            assert_eq!(crate::props::material(kind), material, "{kind}");
            let k = crate::map::LOOSE.iter().position(|l| *l == kind).unwrap();
            assert!(!crate::map::LOOSE_NAMES[k].is_empty() && crate::props::ShapeSpec::named(kind).is_some());
            let map = Map::parse(&format!(
                r#"{{"format": "hotdog-map", "version": 1, "name": "Kitchen", "bounds": [0, 0, 2, 1], "starts": [{{"p": [0.2, 0.2]}}, {{"p": [1.8, 0.8]}}], "spawns": [{{"p": [1.9, 0.1]}}],
                "props": [{{"kind": "crate", "p": [1.2, 0.5], "base": 0, "yaw": 0}}, {{"kind": "{kind}", "p": [0.6, 0.8], "base": 0, "yaw": 0}}]}}"#
            ))
            .unwrap();
            let mut s = Feast::with_map(map, false);
            for p in &mut s.players {
                p.bot = false;
            }
            s.warmup = f32::INFINITY;
            for i in 1..s.players.len() {
                s.players[i].p = [1.9, F, 0.1 + 0.15 * i as f32];
            }
            s.tick(DT);
            assert_eq!(s.props.material(1), material);
            // Picked up from in front of it, looking at it.
            let near = s.props.shape(k).aabb[5] - s.props.shape(k).origin[2];
            self::at(&mut s, 1, 0.6, (0.8 - near - 0.05) as f32);
            let d = look(&mut s, 1, [0., -0.7, 1.]);
            s.tick(DT);
            assert!(s.grab(1, d), "{kind}: picked up");
            // Held so that it would land a centimetre short of the crate's side (it is held turned with the hotdog, its
            // own -z ahead).
            let half = -(s.props.shape(k).aabb[2] - s.props.shape(k).origin[2]);
            let x = 1.2 - 0.05 - half - 0.01 - OUT[k] as f32 as f64;
            self::at(&mut s, 1, x as f32, 0.5);
            look(&mut s, 1, [1., 0., 0.]);
            s.player_mut(1).unwrap().pace = [0.; 2];
            s.tick(DT);
            let spot = s.placement(1).unwrap();
            assert_eq!((spot.beside, spot.lock, spot.refused), (Some(0), true, None), "{kind}: {spot:?}");
            assert!(s.put(1, Vec3::X));
            run(&mut s, 1.);
            let e = s.props.extent(1);
            assert!((e[2] - (1.2 - 0.05 - super::HAIR)).abs() < 0.002 && s.props.state(1) == State::Locked, "{kind}: flush against the crate, {e:?}");
            // And it is cover by its material: a round along it from the crate's far side.
            let solid = s.props.raycast_solid([1.5, F as f64 + 0.005, 0.5], [-1., 0., 0.], 2.).map(|h| h.id);
            assert_eq!(solid, Some(0), "the crate is the first solid thing: {kind}");
        }
    }

    /// The kitchen's pieces at their true sizes beside a hotdog 15 cm tall: the cereal box more than twice its height, the
    /// cutting board larger than the cereal box and the heaviest, and every mass what its size and stuff make it. The
    /// heavier the piece, the slower its carrier walks and the shorter it is thrown.
    #[test]
    fn the_kitchens_pieces_are_their_true_sizes_and_the_heavier_go_slower_and_shorter() {
        use crate::props::{Shape, ShapeSpec};
        let size = |kind: &str| {
            let shape = Shape::new(&ShapeSpec::named(kind).unwrap());
            let a = shape.aabb;
            ([a[3] - a[0], a[4] - a[1], a[5] - a[2]], shape.mass)
        };
        let near = |a: f64, b: f64| (a - b).abs() < 0.004;
        let ([w, h, d], cereal) = size("cereal");
        assert!(near(w, 0.23) && near(h, 0.33) && near(d, 0.08) && h > 2. * crate::actor::HEIGHT && cereal >= 0.5, "{w} {h} {d} {cereal}");
        let ([w, h, d], board) = size("board");
        assert!(near(w, 0.45) && near(h, 0.025) && near(d, 0.30) && w * d > 0.23 * 0.33 && board > 1.5, "{w} {h} {d} {board}");
        let ([w, h, _], plate) = size("plate");
        assert!(near(w, 0.26) && h < 0.025 && plate > 0.3, "{w} {h} {plate}");
        let ([w, h, _], mug) = size("cup");
        assert!(near(h, 0.10) && w > 0.08 && w < 0.12 && mug > 0.2, "{w} {h} {mug}");
        let ([w, h, _], dish) = size("dish");
        assert!(near(w, 0.15) && h < 0.08 && dish > 0.2, "{w} {h} {dish}");
        let ([w, h, _], tin) = size("tin");
        assert!(near(w, 0.075) && near(h, 0.11) && tin > 0.3, "{w} {h} {tin}");
        let ([w, h, _], roll) = size("roll");
        assert!(near(w, 0.12) && near(h, 0.28) && roll < HEAVY, "{w} {h} {roll}");
        let ([w, h, _], salt) = size("salt");
        assert!(near(w, 0.045) && near(h, 0.09) && salt < HEAVY, "{w} {h} {salt}");
        let ([w, h, d], napkin) = size("napkin");
        assert!(near(w, 0.45) && near(d, 0.45) && h < 0.01 && napkin < 0.15, "{w} {h} {d} {napkin}");
        let ([w, h, d], bin) = size("breadbox");
        assert!(near(w, 0.40) && near(h, 0.20) && near(d, 0.25) && bin > 1., "{w} {h} {d} {bin}");
        // The heavier, the slower and the shorter.
        let order = [salt, roll, plate, cereal, bin, board];
        for pair in order.windows(2) {
            assert!(burden_of(pair[1]) <= burden_of(pair[0]) && throw_speed(pair[1]) <= throw_speed(pair[0]), "{pair:?}");
        }
        assert!(throw_speed(board) < 0.7 * THROW[1] && burden_of(board) == LADEN && throw_speed(salt) == THROW[0] && burden_of(salt) == 1.);
    }

    /// A Forts counter 3 m by 1.4 with its bases at the ends: on the mustard side a pile of two cutting boards, a cereal
    /// box, a crate and a napkin.
    fn yard() -> Feast {
        let map = Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "name": "Yard", "mode": "forts", "build": 600, "bounds": [-1.5, 0, 1.5, 1.4], "nav": {"cell": 0.04, "clearance": 0.035},
            "starts": [{"p": [-0.6, 0.15], "yaw": -1.5708, "team": "mustard"}, {"p": [1.35, 0.2], "yaw": 1.5708, "team": "ketchup"}], "spawns": [{"p": [1, 1.3]}],
            "props": [{"kind": "board", "p": [-0.6, 1.2], "base": 0, "yaw": 0}, {"kind": "cereal", "p": [-0.2, 1.25], "base": 0, "yaw": 0},
            {"kind": "crate", "p": [-1, 0.3], "base": 0, "yaw": 0}, {"kind": "napkin", "p": [-0.3, 0.25], "base": 0, "yaw": 0}, {"kind": "board", "p": [-0.6, 1.2], "base": 0.025, "yaw": 0}],
            "zones": [{"kind": "base", "team": "mustard", "rect": [-1.5, 0, -1.2, 1.4]}, {"kind": "base", "team": "ketchup", "rect": [1.2, 0, 1.5, 1.4]},
            {"kind": "field", "rect": [-0.04, 0, 0.04, 1.4]}, {"kind": "bacon", "p": [0, 0.7]}]}"#,
        )
        .unwrap();
        let mut s = Feast::with_map(map, false);
        s.set_build(600.);
        for (i, p) in s.players.iter_mut().enumerate() {
            p.bot = false;
            if i > 0 {
                p.p = [if p.team == 1 { 1.0 } else { -0.2 }, F, 0.05 + 0.03 * i as f32];
            }
        }
        run(&mut s, 0.5);
        s
    }

    /// Forts: nothing may be put down in a base, where the bacon is scored. Held over the base's edge the placement is
    /// refused (`Base`), and Q leaves it in the hands; a step out and it lands. A napkin's drape that would reach into a
    /// base is refused too. Something thrown, knocked or slid in comes to rest out of it: moved out by the base's
    /// nearest side that keeps it on the counter.
    #[test]
    fn nothing_is_put_down_in_a_base_and_what_comes_to_rest_in_one_is_moved_out() {
        use crate::drape::Refusal;
        let mut s = yard();
        // The crate, held facing the mustard base with its far side 5 cm into it: refused, standing still and walking.
        carry(&mut s, 2, [-1.2 + 0.05 + OUT[1] as f32 - 0.05, 0.5], [-1., 0.]);
        let spot = s.placement(1).unwrap();
        assert_eq!(spot.refused, Some(Refusal::Base), "{spot:?}");
        assert!(!s.put(1, Vec3::NEG_X), "Q is refused");
        assert_eq!((s.players[0].carry, s.props.state(2)), (2, State::Carried), "still in its hands");
        s.player_mut(1).unwrap().pace = [-0.5, 0.];
        assert_eq!(s.placement(1).unwrap().refused, Some(Refusal::Base), "on the move too");
        // Turned an eighth, its corner reaches in from further off; a step back and it lands outside.
        s.player_mut(1).unwrap().pace = [0.; 2];
        at(&mut s, 1, -1.2 + 0.05 + OUT[1] as f32 + 0.015, 0.5);
        assert_eq!(s.placement(1).unwrap().refused, None);
        s.set_held_pose(1, Pose { turn: 1, tip: 0 });
        assert_eq!(s.placement(1).unwrap().refused, Some(Refusal::Base), "its corner, turned, reaches in");
        s.set_held_pose(1, Pose::default());
        at(&mut s, 1, -1.2 + 0.05 + OUT[1] as f32 + 0.03, 0.5);
        let spot = s.placement(1).unwrap();
        assert_eq!(spot.refused, None);
        assert!(s.put(1, Vec3::NEG_X));
        run(&mut s, 0.5);
        assert!(s.props.extent(2)[0] > -1.2 && s.props.state(2) == State::Locked);
        // The napkin over the base's edge: refused too.
        carry(&mut s, 3, [-1.2 + OUT[3] as f32 + 0.1, 0.5], [-1., 0.]);
        let spot = s.placement(1).unwrap();
        assert_eq!(spot.refused, Some(Refusal::Base), "{spot:?}");
        assert!(!s.put(1, Vec3::NEG_X));
        // A drape whose wall would reach into the base is refused as well.
        let canopy = crate::drape::Canopy { props: vec![2], rect: [-1.25, 0.4, -1.1, 0.6], top: crate::map::FLOOR + 0.11 };
        assert_eq!(s.refusal(&canopy, &s.bodies), Some(Refusal::Base));
        // The crate shoved into the base (or thrown, or knocked): once it is at rest it is moved out by the base's
        // nearest side that keeps it on the counter (here +x), and settles there.
        pose(&mut s, 2, [-1.3, F as f64, 0.5], 0.);
        run(&mut s, 2.);
        let e = s.props.extent(2);
        assert!(e[0] >= -1.2 && e[0] < -1.18 && (s.props.position(2)[2] - 0.5).abs() < 0.01, "moved out: {e:?}");
        assert!(matches!(s.props.state(2), State::Asleep | State::Locked));
        // Thrown into the ketchup base, it ends outside it too.
        let thrown = 1;
        pose(&mut s, thrown, [1.38, F as f64 + 0.15, 0.5], 0.4);
        s.props.set_velocity(thrown, [-0.2, 0., 0.1], [0.; 3]);
        run(&mut s, 3.);
        let e = s.props.extent(thrown);
        assert!(e[2] <= 1.2 + 1e-6 && e[4] > F as f64 - 0.01, "thrown in, it came to rest out of the base, on the counter: {e:?}");
        // Off a Forts map's rules (Free For All on it), nothing is refused.
        s.fort = None;
        assert!(!s.in_a_base(&[[-1.4, 0.4], [-1.3, 0.4], [-1.3, 0.6], [-1.4, 0.6]]));
    }

    /// How it is held is how it lands: turned by eighths about the vertical, and tipped onto its edge or on end. A cutting
    /// board stood on its long edge lands upright as a wall panel 30 cm tall, locked; a second stood beside it squares to
    /// it and is drawn flush against it; one turned an eighth lands turned so; and the preview is the landing every time.
    #[test]
    fn the_pose_it_is_held_in_is_the_pose_it_lands_in_a_board_stands_on_its_edge_as_a_wall_panel() {
        let mut s = yard();
        // The top board of the pile, stood on its long edge across the hotdog's way (facing -z: the panel runs along x).
        let board = 4;
        carry(&mut s, board, [-0.9, 0.66], [0., -1.]);
        s.set_held_pose(1, Pose { turn: 0, tip: 1 });
        s.tick(DT);
        let spot = s.placement(1).unwrap();
        assert_eq!(spot.q, Pose { turn: 0, tip: 1 }.rotation(spot.yaw));
        assert!((spot.p[1] - (F as f64 + super::HAIR + 0.15)).abs() < 1e-6, "its origin half way up its height: {spot:?}");
        assert!(s.put(1, Vec3::NEG_Z));
        run(&mut s, 2.);
        let e = s.props.extent(board);
        assert!((e[5] - e[4] - 0.30).abs() < 0.004 && (e[2] - e[0] - 0.45).abs() < 0.004 && e[3] - e[1] < 0.03, "a panel 45 cm long and 30 tall: {e:?}");
        assert!(s.props.up(board)[1].abs() < 1e-3 && s.props.state(board) == State::Locked, "standing, locked");
        assert!(moved(s.props.position(board), spot.p) < 0.0005);
        // The other board stood beside it, held a little off and turned a little: squared to it and drawn flush against
        // its end, in line with it.
        let other = 0;
        carry(&mut s, other, [-0.9 + 0.46, 0.67], [0.05, -1.]);
        s.set_held_pose(1, Pose { turn: 0, tip: 1 });
        s.tick(DT);
        let spot = s.placement(1).unwrap();
        assert_eq!((spot.beside, spot.lock), (Some(board), true), "{spot:?}");
        assert!(s.put(1, Vec3::NEG_Z));
        run(&mut s, 1.);
        let (a, b) = (s.props.extent(board), s.props.extent(other));
        assert!((b[0] - a[2]).abs() < 0.002 && (b[1] - a[1]).abs() < 0.002, "flush, end to end, in line: {a:?} {b:?}");
        // The cereal box turned an eighth, standing as it stands: it lands turned so (the preview says how).
        carry(&mut s, 1, [-0.3, 0.85], [-1., 0.]);
        s.set_held_pose(1, Pose { turn: 1, tip: 0 });
        s.tick(DT);
        let spot = s.placement(1).unwrap();
        let held = s.props.rotation(1);
        assert!((0..4).all(|k| (held[k] - Pose { turn: 1, tip: 0 }.rotation(s.players[0].yaw as f64)[k]).abs() < 1e-9), "carried turned: {held:?}");
        assert!(s.put(1, Vec3::NEG_X));
        run(&mut s, 1.);
        let q = s.props.rotation(1);
        assert!((0..4).all(|k| (q[k] - spot.q[k]).abs() < 1e-6) && moved(s.props.position(1), spot.p) < 0.0005, "{q:?} {spot:?}");
        let turned = (2. * q[1].atan2(q[3]) - spot.yaw).rem_euclid(std::f64::consts::TAU);
        assert!((turned - std::f64::consts::FRAC_PI_4).abs() < 1e-6, "an eighth of a turn: {turned}");
        // A napkin is never tipped.
        carry(&mut s, 3, [-0.6, 0.45], [0., -1.]);
        s.set_held_pose(1, Pose { turn: 2, tip: 2 });
        s.tick(DT);
        let spot = s.placement(1).unwrap();
        assert_eq!(spot.q, Pose { turn: 2, tip: 0 }.rotation(spot.yaw));
        // Picked up again, everything is held as the map stands it.
        assert!(s.put(1, Vec3::NEG_Z));
        s.set_held_pose(1, Pose { turn: 3, tip: 1 });
        let d = look(&mut s, 1, [0., -0.8, -0.6]);
        assert!(s.grab(1, d));
        assert_eq!(s.players[0].pose, Pose::default());
    }
}

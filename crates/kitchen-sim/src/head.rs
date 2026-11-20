// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The head: the top of a hotdog nods with where it looks, and the rounds find it where it is.
//!
//! **The nod.** A hotdog's look pitch (`Player::pitch`, -1.5..1.5 radians, every tick and in every frame of the
//! protocol) bows its head (`nod`): looking fully down by BOW (45 degrees) forward, fully up by TIP (25 degrees) back,
//! in proportion between; lying prone, where the counter is in the way, by PRONE_BOW forward at the most. The nod is a
//! function of the stance and the pitch alone, the same on the server, in the client's prediction and replays, and in
//! the C interface (which runs this code).
//!
//! **The bend.** In a standing hotdog's frame (heights above its feet, x to its right, its face on -z) the body below
//! NECK[0] keeps still, the head above NECK[1] turns as one, and the axis between curves smoothly (`bend`): an arc
//! turning by the nod, its points carried round with it, so the head leads and nothing hinges. Bowing, the neck gives a
//! little (SQUASH of its length at a full bow): the head sinks into it. The client draws exactly this bend on the
//! sausage, the buns and the mustard (player_model.rs and its bend shader), and hangs the face on `head`.
//!
//! **What a round finds** (`hit`): the body, the sphere about the chest (feast.rs `chest`, BODY round) as far up as
//! BODY_TOP and no further, which never moves with the look; and the head, a capsule across the hotdog (left to
//! right) about HEAD_AT, carried by the bend. Looking level it is where the top of the old sphere was, as high and as
//! wide; looking down it is lower and further forward, so that a round at the head's height from behind passes over or
//! meets it lower; looking up it is further back. The body's sphere is broader than the body drawn, and a head bowed
//! into it is still the head. The face (`in_face`) is on the head and turns with it. Every stance
//! is the standing frame put where the stance has the chest (`frame`): crouched lower, tucked in the air as standing,
//! prone lying along the way the hotdog faces with its head at the front end.
use crate::feast::{CHEST, Player, chest, stance};
use crate::ray_sphere;
use glam::{Quat, Vec3};
use std::f32::consts::FRAC_PI_2;

/// The neck in a standing hotdog's frame: from NECK[0] (just under the shoulders) the axis curves, and from NECK[1]
/// (under the mouth) up the head turns as one.
pub const NECK: [f32; 2] = [0.09, 0.12];
/// How far the head bows looking fully down, and tips back looking fully up (radians); prone, how far it bows at the
/// most. LOOK is the pitch of a full nod: the most a hotdog looks either way (feast.rs `look`).
pub const BOW: f32 = std::f32::consts::FRAC_PI_4;
pub const TIP: f32 = 25. * std::f32::consts::PI / 180.;
pub const PRONE_BOW: f32 = 15. * std::f32::consts::PI / 180.;
pub const LOOK: f32 = 1.5;
/// How much of the neck's length a full bow takes up: the head sinks into it as it bows (and only bowing).
pub const SQUASH: f32 = 0.2;
/// The body a round finds: a sphere this round about the chest, cut off BODY_TOP up a standing hotdog's frame (a
/// little over the neck's top: what the bowed head comes down into, it shows through, `hit`).
pub const BODY: f32 = 0.055;
pub const BODY_TOP: f32 = 0.14;
/// The head a round finds, in a standing hotdog's frame: a capsule across it about (0, HEAD_AT, 0), HEAD_HALF either
/// side of the middle, HEAD_RADIUS round. Looking level its top is the old sphere's (CHEST[0] + BODY).
pub const HEAD_AT: f32 = 0.135;
pub const HEAD_RADIUS: f32 = 0.025;
pub const HEAD_HALF: f32 = 0.016;
/// The face on the head (`in_face`), in the head's own frame (as it is looking level): where a round enters the head,
/// the way to it from the head's middle rises FACE_RISE[0] to FACE_RISE[1] (from the chin to the brows; above is the
/// crown, below the neck and the body) and, seen from above, lies within FACE_ARC (radians) of the way the face looks;
/// and the round comes from in front of the face, its way against the face's at least FACING. Prone the face is the
/// head's front end, the top of the head: the way to it within FACE_ARC of that end, not below PRONE_RISE. Looking
/// level the face is as large a mark as the old sphere's (a sniper's round finds 97% of its area, a pistol's 109%) and
/// FACE_ARC, FACING and PRONE_RISE are the old sphere's.
pub const FACE_RISE: [f32; 2] = [-0.4, 0.9];
pub const FACE_ARC: f32 = 0.87;
pub const FACING: f32 = 0.25;
pub const PRONE_RISE: f32 = -0.25;
/// The middle of the face, the way out of the head (its own frame, looking level), and how far out along it a bot
/// aims, as a share of HEAD_RADIUS.
const FACE_MIDDLE: Vec3 = Vec3::new(0., 0.2, -0.98);
const FACE_DEPTH: f32 = 0.9;

/// How far a hotdog in `stance` (feast.rs `stance`: 0 standing or tucked, 1 crouched, 2 prone) looking `pitch` nods its
/// head: a turn about its own x axis (to its right), the way the pitch turns its look: negative bows it forward and
/// down, positive tips it back.
pub fn nod(stance: usize, pitch: f32) -> f32 {
    let pitch = if pitch.is_finite() { pitch.clamp(-LOOK, LOOK) } else { 0. };
    if pitch < 0. {
        pitch / LOOK * if stance == 2 { PRONE_BOW } else { BOW }
    } else {
        pitch / LOOK * TIP
    }
}
/// How far hotdog `p` nods, now.
pub fn nod_of(p: &Player) -> f32 {
    nod(stance(p), p.pitch)
}
/// sin(x)/x and (1 - cos x)/x, smooth through 0.
fn sinc(x: f32) -> f32 {
    if x.abs() < 1e-3 { 1. - x * x / 6. } else { x.sin() / x }
}
fn cosc(x: f32) -> f32 {
    if x.abs() < 1e-3 { x / 2. - x * x * x / 24. } else { (1. - x.cos()) / x }
}
/// The neck's length as a nod leaves it, as a share of its length straight.
pub fn squash(nod: f32) -> f32 {
    1. - SQUASH * ((-nod).max(0.) / BOW).min(1.)
}
/// Where the bend takes point `p` of a standing hotdog's frame for a nod of `nod`: below NECK[0] nowhere; in the neck
/// round the arc of its axis, which turns by the nod over its (squashed) length, each point carried with the axis at
/// its height; above NECK[1] as the head turns (`head`).
pub fn bend(nod: f32, p: Vec3) -> Vec3 {
    let [low, high] = NECK;
    if p.y <= low || nod == 0. {
        return p;
    }
    let along = p.y.min(high) - low;
    let turn = nod * along / (high - low);
    let length = squash(nod) * along;
    Vec3::new(0., low + length * sinc(turn), length * cosc(turn)) + Quat::from_rotation_x(turn) * Vec3::new(p.x, (p.y - high).max(0.), p.z)
}
/// How much the bend turns what is at height `y` of a standing hotdog's frame (normals and directions there turn by
/// it about x).
pub fn turn_at(nod: f32, y: f32) -> f32 {
    let [low, high] = NECK;
    nod * ((y - low) / (high - low)).clamp(0., 1.)
}
/// The head as the bend moves it, all that is at or above NECK[1]: a turn, then a shift (p goes to shift + turn * p).
pub fn head(nod: f32) -> (Vec3, Quat) {
    let [low, high] = NECK;
    let length = squash(nod) * (high - low);
    let turn = Quat::from_rotation_x(nod);
    (Vec3::new(0., low + length * sinc(nod), length * cosc(nod)) - turn * Vec3::Y * high, turn)
}
/// How a hotdog's frame (a standing hotdog's) lies in the world: turned to its yaw, and prone laid forward, its head the
/// way it faces and its face to the counter. A point of the frame is in the world at `place`.
pub fn frame(p: &Player) -> Quat {
    let yaw = Quat::from_rotation_y(p.yaw);
    if p.prone { yaw * Quat::from_rotation_x(-FRAC_PI_2) } else { yaw }
}
/// Point `local` of hotdog `p`'s frame in the world: its chest is where the frame's (0, CHEST[0], 0) is.
pub fn place(p: &Player, local: Vec3) -> Vec3 {
    chest(p) + frame(p) * (local - Vec3::Y * CHEST[0])
}
/// Hotdog `p`'s head as a round finds it: the ends of the capsule's middle line, and how the head is turned in the
/// world (its frame's, then the nod's).
#[derive(Clone, Copy, Debug)]
pub struct Head {
    pub ends: [Vec3; 2],
    pub turn: Quat,
}
impl Head {
    pub fn of(p: &Player) -> Head {
        let nod = nod_of(p);
        let middle = bend(nod, Vec3::Y * HEAD_AT);
        let ends = [-1., 1.].map(|side| place(p, middle + Vec3::X * side * HEAD_HALF));
        Head { ends, turn: frame(p) * Quat::from_rotation_x(nod) }
    }
    /// The middle of the capsule.
    pub fn centre(&self) -> Vec3 {
        (self.ends[0] + self.ends[1]) * 0.5
    }
    /// The nearest point of the capsule's middle line to `q`.
    pub fn spine(&self, q: Vec3) -> Vec3 {
        let [a, b] = self.ends;
        let ab = b - a;
        a + ab * ((q - a).dot(ab) / ab.length_squared()).clamp(0., 1.)
    }
}
/// The middle of hotdog `p`'s head (where the bend has it).
pub fn head_centre(p: &Player) -> Vec3 {
    Head::of(p).centre()
}
/// Which part of a hotdog a round met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Body,
    Head,
}
/// Where a round flying from `origin` along `dir` (a unit vector) first meets a hotdog, `size` being the round's
/// radius: how far along (`t`, where it strikes), which part it struck, and where on that part (`point`: on the head
/// where it met the head, which may be inside the body's sphere, `hit`).
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub part: Part,
    pub point: Vec3,
}
/// Where a ray enters and leaves the sphere about `centre` (`radius` round) cut off by the plane `cut` along `up` from
/// the centre (what is beyond it is not there), as `ray_sphere` (the entry 0 for a ray starting inside).
fn ray_cut_sphere(origin: Vec3, dir: Vec3, centre: Vec3, radius: f32, up: Vec3, cut: f32) -> Option<(f32, f32)> {
    let v = origin - centre;
    let b = v.dot(dir);
    let h = b * b - (v.length_squared() - radius * radius);
    if h < 0. {
        return None;
    }
    let (mut near, mut far) = (-b - h.sqrt(), -b + h.sqrt());
    let (height, climb) = (v.dot(up), dir.dot(up));
    if climb.abs() < 1e-9 {
        if height > cut {
            return None;
        }
    } else {
        let t = (cut - height) / climb;
        if climb > 0. { far = far.min(t) } else { near = near.max(t) }
    }
    (near <= far && far >= 0.).then_some((near.max(0.), far))
}
/// Where a ray first meets the capsule round the segment `a`..`b`, `radius` round (0 for a ray starting inside).
pub fn ray_capsule(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, radius: f32) -> Option<f32> {
    let ends = [a, b].into_iter().filter_map(|c| ray_sphere(origin, dir, c, radius));
    let axis = b - a;
    let long = axis.length();
    let mut side = None;
    if long > 1e-9 {
        let u = axis / long;
        let (v, d) = (origin - a, dir);
        let (vp, dp) = (v - u * v.dot(u), d - u * d.dot(u));
        let (aa, bb, cc) = (dp.length_squared(), vp.dot(dp), vp.length_squared() - radius * radius);
        let h = bb * bb - aa * cc;
        if aa > 1e-12 && h >= 0. {
            let (near, far) = ((-bb - h.sqrt()) / aa, (-bb + h.sqrt()) / aa);
            let t = near.max(0.);
            let along = (v + d * t).dot(u);
            if far >= 0. && (0. ..=long).contains(&along) {
                side = Some(t);
            }
        }
    }
    ends.chain(side).min_by(f32::total_cmp)
}
/// Where a round flying from `origin` along the unit vector `dir`, `size` round, first meets hotdog `p`: its body (the
/// sphere about its chest up to BODY_TOP, BODY round) or its head (`Head`), each grown by the round's size. It
/// strikes where it first meets either. The body's sphere is broader than the body drawn: a head bowed into it is still
/// the head, so a round whose line meets the head before it has passed through the body's sphere struck the head,
/// where it met it (and its face, if that is where, `in_face`), though it struck at the sphere. Looking level the head
/// is above the sphere's cut but for its chin and its back: all such a round meets there is the head's lowest part,
/// the body's damage as before; bowed, the face comes down in front of the chest, as it is drawn.
pub fn hit(p: &Player, origin: Vec3, dir: Vec3, size: f32) -> Option<Hit> {
    let up = frame(p) * Vec3::Y;
    let body = ray_cut_sphere(origin, dir, chest(p), BODY + size, up, BODY_TOP - CHEST[0]);
    let head = Head::of(p);
    let top = ray_capsule(origin, dir, head.ends[0], head.ends[1], HEAD_RADIUS + size);
    let (t, part, at) = match (body, top) {
        // (At one distance, the body.)
        (Some((enter, _)), Some(t)) if t < enter => (t, Part::Head, t),
        (Some((enter, leave)), Some(t)) if t < leave => (enter, Part::Head, t),
        (Some((enter, _)), _) => (enter, Part::Body, enter),
        (None, Some(t)) => (t, Part::Head, t),
        (None, None) => return None,
    };
    Some(Hit { t, part, point: origin + dir * at })
}
/// Whether a round flying along `dir` that entered hotdog `p`'s head at `point` struck its face: the way to that point
/// from the head's middle and the round's way, turned back into the head's own frame, as FACE_RISE, FACE_ARC and FACING say (prone:
/// FACE_ARC and PRONE_RISE). The face turns with the head: bowed, a round from straight ahead at the same height meets
/// the top of the head, and one from below the face.
pub fn in_face(p: &Player, point: Vec3, dir: Vec3) -> bool {
    let head = Head::of(p);
    let back = head.turn.inverse();
    // (The way from the head's middle to where the round entered: on the capsule's flat-sided middle the way out of its
    // surface would face the round whatever its angle; this is as the round's sphere had it.)
    let (n, d) = (back * (point - head.centre()).normalize_or_zero(), back * dir);
    if p.prone {
        // The front end: the top of the head (+y of its frame), the counter under its face (-z) and the sky over its back.
        return -d.y >= FACING && n.y >= FACE_ARC.cos() && n.z >= PRONE_RISE;
    }
    let flat = Vec3::new(n.x, 0., n.z);
    d.z >= FACING && n.y >= FACE_RISE[0] && n.y <= FACE_RISE[1] && flat.length() > 1e-4 && -flat.normalize().z >= FACE_ARC.cos()
}
/// The middle of hotdog `p`'s face, a little inside its head: what a bot going for the face aims at. Prone it is the
/// head's front end.
pub fn face_point(p: &Player) -> Vec3 {
    let head = Head::of(p);
    let out = if p.prone { Vec3::Y } else { FACE_MIDDLE.normalize() };
    head.centre() + head.turn * out * HEAD_RADIUS * FACE_DEPTH
}
/// How far `point` is from hotdog `p`'s head (0 inside it), and its nearest point on the head's surface.
pub fn head_gap(p: &Player, point: Vec3) -> (f32, Vec3) {
    let head = Head::of(p);
    let on = head.spine(point);
    let far = point.distance(on);
    if far <= HEAD_RADIUS {
        (0., point)
    } else {
        (far - HEAD_RADIUS, on + (point - on) / far * HEAD_RADIUS)
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The titan's poses, written as Kung Fu's are (kungfu/mod.rs): the whole body at one instant as an animator thinks of
//! it, in metres at `body::REFERENCE` tall, in the body's own frame: x **out** from the middle for a limb (one number
//! serves either side) or to the right for the hips, y **up**, z **forward**. Solving a pose (`solve`) turns it into the
//! place of every part of the rig, relative to the titan's root (its feet; y up, the face towards -z). Its limbs do not
//! stretch: a giant's arms are not rubber hoses. A hand or a foot put further than its limb reaches is reached for as
//! far as it goes.
use super::body::{self, FORE, HIP, NECK, PIVOT, SHIN, SHOULDER, THIGH, UPPER};
use bevy::prelude::*;

pub const L: usize = 0;
pub const R: usize = 1;
/// -1 for the left side, 1 for the right.
pub fn sign(side: usize) -> f32 {
    if side == L { -1. } else { 1. }
}
pub const fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
/// A limb's position or direction as written (out, up, forward) in the rig's frame (x right, y up, -z forward).
/// The elbows' and the knees' hints when their own come in line with the limb (out, up, forward; `reach`'s fallback).
pub const FALLBACKS: (Vec3, Vec3) = (Vec3::new(1., -0.2, -0.6), Vec3::new(0.4, 0., 1.));
pub fn limb_frame(side: usize, at: Vec3) -> Vec3 {
    Vec3::new(sign(side) * at.x, at.y, -at.z)
}
/// The hips' offset as written (right, up, forward) in the rig's frame.
pub fn body_frame(at: Vec3) -> Vec3 {
    Vec3::new(at.x, at.y, -at.z)
}

/// One arm: where the wrist is (from its shoulder, in the chest's frame: out, up, forward), which way the elbow points,
/// which way the palm faces, and how far the hand is closed (0 open and hooked, 1 a fist).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arm {
    pub hand: Vec3,
    pub elbow: Vec3,
    pub palm: Vec3,
    pub fist: f32,
}
impl Arm {
    /// Hanging, heavy, a little forward, the hand to its knee.
    pub const REST: Arm = Arm { hand: v(0.06, -0.66, 0.1), elbow: v(0.4, 0., -1.), palm: v(-1., 0., 0.2), fist: 0.15 };
}
/// One leg: where the ankle is (out from the middle of the body, up from the ground, forward), which way the knee
/// points, the toes pointed down (+) or up (-) when the foot is off the ground, and how firmly it is planted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leg {
    pub foot: Vec3,
    pub knee: Vec3,
    pub toe: f32,
    pub plant: f32,
}
impl Leg {
    pub const REST: Leg = Leg { foot: v(0.17, body::ANKLE, 0.), knee: v(0.25, 0., 1.), toe: 0., plant: 1. };
}

/// The whole body at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// The hips (and the body on them) moved from rest: right, up, forward.
    pub hips: Vec3,
    /// The body leant forward (+) about the hips, twisted to its left (+), tilted to its left (+), radians.
    pub lean: f32,
    pub twist: f32,
    pub tilt: f32,
    /// The head bowed (+) or thrown back (-) on the neck, and turned to its left (+), radians: on top of where the
    /// simulation says it looks.
    pub nod: f32,
    pub turn: f32,
    /// The chest heaving (0 to 1): a breath, a roar.
    pub heave: f32,
    /// The sausage squashed (0 to about 0.2: shorter and fatter), as it hunches down into a crouch.
    pub squash: f32,
    pub arms: [Arm; 2],
    pub legs: [Leg; 2],
    /// The whole body turned about `pivot` (a point on the ground in its frame: right, up, forward): toppled forward
    /// (+x), turned to its left (+y), rolled to its left (+z).
    pub spin: Vec3,
    pub pivot: Vec3,
    /// How much the body's steam vents now (0 to 1, on top of what its health says): a roar, a smash, its death.
    pub vent: f32,
}
impl Default for Pose {
    fn default() -> Self {
        Pose::REST
    }
}
impl Pose {
    pub const REST: Pose = Pose {
        hips: Vec3::ZERO,
        lean: 0.,
        twist: 0.,
        tilt: 0.,
        nod: 0.,
        turn: 0.,
        heave: 0.,
        squash: 0.,
        arms: [Arm::REST; 2],
        legs: [Leg::REST; 2],
        spin: Vec3::ZERO,
        pivot: Vec3::ZERO,
        vent: 0.,
    };
    pub fn hips(mut self, right: f32, up: f32, forward: f32) -> Self {
        self.hips = v(right, up, forward);
        self
    }
    pub fn lean(mut self, radians: f32) -> Self {
        self.lean = radians;
        self
    }
    pub fn twist(mut self, radians: f32) -> Self {
        self.twist = radians;
        self
    }
    pub fn tilt(mut self, radians: f32) -> Self {
        self.tilt = radians;
        self
    }
    pub fn nod(mut self, radians: f32) -> Self {
        self.nod = radians;
        self
    }
    pub fn heave(mut self, amount: f32) -> Self {
        self.heave = amount;
        self
    }
    pub fn squash(mut self, amount: f32) -> Self {
        self.squash = amount;
        self
    }
    pub fn vent(mut self, amount: f32) -> Self {
        self.vent = amount;
        self
    }
    pub fn spin(mut self, flip: f32, turn: f32, roll: f32, pivot: Vec3) -> Self {
        self.spin = v(flip, turn, roll);
        self.pivot = pivot;
        self
    }
    /// Puts the wrist on `side` at `hand` (out, up, forward from its shoulder, in the chest's frame), the hand `fist`
    /// closed.
    pub fn arm(mut self, side: usize, hand: Vec3, fist: f32) -> Self {
        self.arms[side].hand = hand;
        self.arms[side].fist = fist;
        self
    }
    /// Puts the wrist on `side` at `at` in the body's own frame (out from the middle, up from the ground, forward),
    /// wherever the lean, twist, tilt and hips (set before this) have put the shoulder.
    pub fn hand_at(mut self, side: usize, at: Vec3, fist: f32) -> Self {
        let chest = torso_rotation(self.lean, self.twist, self.tilt);
        let pivot = PIVOT + body_frame(self.hips);
        let shoulder = pivot + chest * (torso_scale(self.heave, self.squash) * (Vec3::new(sign(side) * SHOULDER.x, SHOULDER.y, SHOULDER.z) - PIVOT));
        let local = chest.inverse() * (limb_frame(side, at) - shoulder);
        self.arms[side].hand = Vec3::new(sign(side) * local.x, local.y, -local.z);
        self.arms[side].fist = fist;
        self
    }
    /// Points the elbow on `side` (out, up, forward).
    pub fn elbow(mut self, side: usize, towards: Vec3) -> Self {
        self.arms[side].elbow = towards;
        self
    }
    /// Turns the palm on `side` to face `towards` (out, up, forward).
    pub fn palm(mut self, side: usize, towards: Vec3) -> Self {
        self.arms[side].palm = towards;
        self
    }
    /// Plants the foot on `side` at `at` (out from the middle, up, forward): flat on the ground if it is at ankle height.
    pub fn foot(mut self, side: usize, at: Vec3) -> Self {
        self.legs[side].foot = at;
        self.legs[side].plant = if at.y < body::ANKLE + 0.004 { 1. } else { 0. };
        self
    }
    /// Lifts the foot on `side` to `at`, its knee towards `knee`, its toes pointed by `toe`.
    pub fn lift(mut self, side: usize, at: Vec3, knee: Vec3, toe: f32) -> Self {
        self.legs[side] = Leg { foot: at, knee, toe, plant: 0. };
        self
    }
    /// Points the knee on `side` (out, up, forward).
    pub fn knee(mut self, side: usize, towards: Vec3) -> Self {
        self.legs[side].knee = towards;
        self
    }
    /// A weighted sum of poses (weights summing to 1).
    pub fn weighted(parts: &[(&Pose, f32)]) -> Pose {
        let f = |get: &dyn Fn(&Pose) -> f32| parts.iter().map(|(p, w)| get(p) * w).sum::<f32>();
        let g = |get: &dyn Fn(&Pose) -> Vec3| parts.iter().map(|(p, w)| get(p) * *w).sum::<Vec3>();
        let arm = |i: usize| Arm { hand: g(&|p| p.arms[i].hand), elbow: g(&|p| p.arms[i].elbow), palm: g(&|p| p.arms[i].palm), fist: f(&|p| p.arms[i].fist).clamp(0., 1.) };
        let leg = |i: usize| Leg { foot: g(&|p| p.legs[i].foot), knee: g(&|p| p.legs[i].knee), toe: f(&|p| p.legs[i].toe), plant: f(&|p| p.legs[i].plant).clamp(0., 1.) };
        Pose {
            hips: g(&|p| p.hips),
            lean: f(&|p| p.lean),
            twist: f(&|p| p.twist),
            tilt: f(&|p| p.tilt),
            nod: f(&|p| p.nod),
            turn: f(&|p| p.turn),
            heave: f(&|p| p.heave).max(0.),
            squash: f(&|p| p.squash).clamp(-0.1, 0.3),
            arms: [arm(L), arm(R)],
            legs: [leg(L), leg(R)],
            spin: g(&|p| p.spin),
            pivot: g(&|p| p.pivot),
            vent: f(&|p| p.vent).clamp(0., 1.),
        }
    }
    /// Straight from this pose to `to`, `t` of the way.
    pub fn lerp(&self, to: &Pose, t: f32) -> Pose {
        Pose::weighted(&[(self, 1. - t), (to, t)])
    }
    /// How far apart two poses are, roughly in metres (for the curves through the keys).
    pub fn gap(&self, to: &Pose) -> f32 {
        let turns = (self.lean - to.lean).abs() + (self.twist - to.twist).abs() + (self.tilt - to.tilt).abs() + (self.spin - to.spin).length() + 0.3 * (self.nod - to.nod).abs();
        let limbs: f32 = (0..2).map(|i| self.arms[i].hand.distance(to.arms[i].hand) + self.legs[i].foot.distance(to.legs[i].foot)).sum();
        self.hips.distance(to.hips) + limbs + 0.8 * turns
    }
    /// Whether every number in it is finite.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn finite(&self) -> bool {
        let vs = [self.hips, self.spin, self.pivot, self.arms[0].hand, self.arms[1].hand, self.arms[0].elbow, self.arms[1].elbow, self.arms[0].palm, self.arms[1].palm, self.legs[0].foot, self.legs[1].foot, self.legs[0].knee, self.legs[1].knee];
        let fs = [self.lean, self.twist, self.tilt, self.nod, self.turn, self.heave, self.squash, self.vent, self.arms[0].fist, self.arms[1].fist, self.legs[0].toe, self.legs[1].toe, self.legs[0].plant, self.legs[1].plant];
        vs.iter().all(|v| v.is_finite()) && fs.iter().all(|f| f.is_finite())
    }
}

/// The torso's scale for a heaving chest and a squash.
pub fn torso_scale(heave: f32, squash: f32) -> Vec3 {
    let h = heave.clamp(0., 1.5);
    let q = squash.clamp(-0.1, 0.3);
    Vec3::new((1. + 0.025 * h) * (1. + 0.5 * q), (1. + 0.012 * h) * (1. - q), (1. + 0.035 * h) * (1. + 0.5 * q))
}
/// The torso's rotation for a lean, a twist and a tilt.
pub fn torso_rotation(lean: f32, twist: f32, tilt: f32) -> Quat {
    Quat::from_rotation_y(twist) * Quat::from_rotation_z(tilt) * Quat::from_rotation_x(-lean)
}
/// The rotation of a whole-body spin.
pub fn spin_rotation(spin: Vec3) -> Quat {
    Quat::from_rotation_y(spin.y) * Quat::from_rotation_z(spin.z) * Quat::from_rotation_x(-spin.x)
}

/// Two-bone reach from `joint` to `target`, bones `a` and `b` long, the middle joint pushed towards `pole` (and where the
/// limb runs along the pole, towards `fallback`). Never stretches: a target out of reach is reached for in its
/// direction. Returns the first bone's rotation (its -y down the bone, its +z the way the middle joint points) and the
/// second's, both in the frame `joint` and `target` are in, and where the end is.
pub fn reach(joint: Vec3, target: Vec3, a: f32, b: f32, pole: Vec3, fallback: Vec3) -> (Quat, Quat, Vec3) {
    let to = target - joint;
    let along = to.normalize_or(Vec3::NEG_Y);
    let d = to.length().clamp((a - b).abs() + 0.02 * (a + b), (a + b) * 0.995);
    let first = pole - along * pole.dot(along);
    let second = fallback - along * fallback.dot(along);
    let k = first.length() / pole.length().max(1e-6);
    let w = ((0.45 - k) / 0.45).clamp(0., 1.);
    let out = (first.normalize_or_zero() * (1. - w) + second.normalize_or_zero() * w).normalize_or(along.any_orthonormal_vector());
    // The middle joint: `a` from the joint and `b` from the end.
    let x = (d * d + a * a - b * b) / (2. * d);
    let h = (a * a - x * x).max(0.).sqrt();
    let middle = joint + along * x + out * h;
    let end = joint + along * d;
    let frame = |down: Vec3, towards: Vec3| {
        let y = -down.normalize_or(Vec3::NEG_Y);
        let z = (towards - y * towards.dot(y)).normalize_or(y.any_orthonormal_vector());
        Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z))
    };
    let upper = frame(middle - joint, out);
    // The second bone's +z points the same way round the hinge: away from the bend, so its -z faces where it swings.
    let lower = frame(end - middle, out - (end - middle).normalize_or(Vec3::NEG_Y) * 0.0);
    (upper, lower, end)
}

/// Where every part of the rig is, relative to the titan's root (its feet; y up, the face towards -z): each a place for
/// a part whose mesh is made in its own frame (body.rs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skeleton {
    /// The torso link (its mesh's origin is `PIVOT`).
    pub torso: Transform,
    /// The head link (its origin `NECK`).
    pub head: Transform,
    /// Each eye (looking down its -z).
    pub eyes: [Transform; 2],
    pub upper: [Transform; 2],
    pub fore: [Transform; 2],
    pub hands: [Transform; 2],
    pub fists: [f32; 2],
    pub thighs: [Transform; 2],
    pub shins: [Transform; 2],
    pub feet: [Transform; 2],
    /// Where the joints are (for the effects, the cameras and the tests).
    pub shoulder: [Vec3; 2],
    pub elbow: [Vec3; 2],
    pub wrist: [Vec3; 2],
    pub hip: [Vec3; 2],
    pub knee: [Vec3; 2],
    pub ankle: [Vec3; 2],
    pub vent: f32,
    pub heave: f32,
}
impl Skeleton {
    /// The middle of the palm of the hand on `side` (what lands a swat or a smash).
    pub fn palm(&self, side: usize) -> Vec3 {
        self.hands[side].transform_point(Vec3::new(0., -0.12 * body::HAND, 0.))
    }
    /// The middle of the sole of the foot on `side`.
    pub fn sole(&self, side: usize) -> Vec3 {
        self.feet[side].transform_point(Vec3::new(0., -body::ANKLE, -0.07))
    }
    /// A point given in the head's frame (from `NECK`, as body.rs has the face and the weak spot).
    pub fn on_head(&self, local: Vec3) -> Vec3 {
        self.head.transform_point(local)
    }
    /// The middle of the eyes.
    pub fn eye(&self) -> Vec3 {
        self.on_head(Vec3::new(0., body::EYE.y, body::EYE.z - body::EYEBALL))
    }
    /// The weak spot's middle, and the way out of the head there.
    pub fn spot(&self) -> (Vec3, Vec3) {
        let at = self.on_head(body::spot());
        (at, (self.head.rotation * body::nape()).normalize())
    }
    /// The middle of the top of the head.
    pub fn crown(&self) -> Vec3 {
        self.on_head(Vec3::new(0., body::TOP - NECK.y - 0.04, 0.))
    }
    /// A point on the axis of the torso link `y` up the body at rest.
    pub fn axis(&self, y: f32) -> Vec3 {
        self.torso.transform_point(Vec3::new(0., y - PIVOT.y, 0.))
    }
}

/// Solves `p`: the eyes stare at `stare` (in the root's frame) if they can, and the head is turned to the simulation's
/// `look` (its nod: radians about x, less than 0 bowed forward) and then bowed by the pose's own `nod` and turned by its
/// `turn`. The head keeps its own level, as a creature's does, however the chest leans (it follows a little of the
/// chest's twist and tilt); the neck it sits on goes where the chest takes it.
pub fn solve(p: &Pose, stare: Option<Vec3>, look: f32) -> Skeleton {
    let chest = torso_rotation(p.lean, p.twist, p.tilt);
    // The chest heaves: the torso swells a touch and rises.
    let heave = p.heave.clamp(0., 1.5);
    let pivot = PIVOT + body_frame(p.hips);
    let torso = Transform { translation: pivot, rotation: chest, scale: torso_scale(heave, p.squash) };
    let head_turn = Quat::from_rotation_y(p.turn + 0.35 * p.twist) * Quat::from_rotation_z(0.5 * p.tilt) * Quat::from_rotation_x(look - p.nod);
    let neck = torso.transform_point(NECK - PIVOT);
    let head = Transform { translation: neck, rotation: head_turn, scale: Vec3::ONE };
    let mut s = Skeleton {
        torso,
        head,
        eyes: [Transform::IDENTITY; 2],
        upper: [Transform::IDENTITY; 2],
        fore: [Transform::IDENTITY; 2],
        hands: [Transform::IDENTITY; 2],
        fists: [p.arms[L].fist, p.arms[R].fist],
        thighs: [Transform::IDENTITY; 2],
        shins: [Transform::IDENTITY; 2],
        feet: [Transform::IDENTITY; 2],
        shoulder: [Vec3::ZERO; 2],
        elbow: [Vec3::ZERO; 2],
        wrist: [Vec3::ZERO; 2],
        hip: [Vec3::ZERO; 2],
        knee: [Vec3::ZERO; 2],
        ankle: [Vec3::ZERO; 2],
        vent: p.vent,
        heave,
    };
    for i in [L, R] {
        // The eyes: in their sockets, staring.
        let socket = head.transform_point(Vec3::new(sign(i) * body::EYE.x, body::EYE.y, body::EYE.z));
        let ahead = head.rotation * Vec3::NEG_Z;
        let want = stare.map_or(ahead, |at| (at - socket).normalize_or(ahead));
        // (They turn at most about 35 degrees from straight ahead.)
        let look = if want.dot(ahead) > 0.82 { want } else { (ahead * 0.82 + (want - ahead * want.dot(ahead)).normalize_or_zero() * 0.57).normalize() };
        s.eyes[i] = Transform { translation: socket, rotation: Quat::from_rotation_arc(Vec3::NEG_Z, look), scale: if i == L { Vec3::ONE } else { Vec3::splat(1.06) } };
        // The arm, from the shoulder where the chest has it.
        let a = &p.arms[i];
        let shoulder = torso.transform_point(Vec3::new(sign(i) * SHOULDER.x, SHOULDER.y, SHOULDER.z) - PIVOT);
        let target = shoulder + chest * limb_frame(i, a.hand);
        let (upper, fore, wrist) = reach(shoulder, target, UPPER, FORE, chest * limb_frame(i, a.elbow), chest * limb_frame(i, FALLBACKS.0));
        let elbow = shoulder + upper * Vec3::NEG_Y * UPPER;
        s.shoulder[i] = shoulder;
        s.elbow[i] = elbow;
        s.wrist[i] = wrist;
        s.upper[i] = Transform { translation: shoulder, rotation: upper, scale: Vec3::ONE };
        s.fore[i] = Transform { translation: elbow, rotation: fore, scale: Vec3::ONE };
        // The hand: the fingers on from the forearm (its -y), the palm (its -z) as near `palm` as the forearm allows.
        let along = fore * Vec3::NEG_Y;
        let natural = fore * Vec3::NEG_Z;
        let wanted = (chest * limb_frame(i, a.palm)).normalize_or(natural);
        let across = wanted - along * wanted.dot(along);
        let palm = (across + natural * (0.35 - across.length()).max(0.)).normalize_or(natural);
        let (y, z) = (-along, -palm);
        let z = (z - y * z.dot(y)).normalize_or(fore * Vec3::Z);
        s.hands[i] = Transform { translation: wrist, rotation: Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z)), scale: Vec3::ONE };
        // The leg, from the hip (which moves with the hips, not the chest).
        let l = &p.legs[i];
        let hip = Vec3::new(sign(i) * HIP.x, HIP.y, HIP.z) + body_frame(p.hips);
        let foot = limb_frame(i, l.foot);
        let (thigh, shin, ankle) = reach(hip, foot, THIGH, SHIN, limb_frame(i, l.knee), limb_frame(i, FALLBACKS.1));
        let knee = hip + thigh * Vec3::NEG_Y * THIGH;
        s.hip[i] = hip;
        s.knee[i] = knee;
        s.ankle[i] = ankle;
        s.thighs[i] = Transform { translation: hip, rotation: thigh, scale: Vec3::ONE };
        s.shins[i] = Transform { translation: knee, rotation: shin, scale: Vec3::ONE };
        // The foot: its toes (its -z) the way the knee points, level when planted, pointed by `toe` when not.
        let ahead = knee - (hip + ankle) * 0.5;
        let level = Vec3::new(ahead.x, 0., ahead.z).normalize_or(limb_frame(i, v(0.15, 0., 1.)).normalize());
        let flat = Quat::from_rotation_arc(Vec3::NEG_Z, level);
        let pointed = flat * Quat::from_rotation_x(-l.toe);
        s.feet[i] = Transform { translation: ankle, rotation: pointed.slerp(flat, l.plant.clamp(0., 1.)), scale: Vec3::ONE };
    }
    // The whole body spun about its pivot.
    if p.spin.length_squared() > 1e-8 {
        let turn = spin_rotation(p.spin);
        let about = body_frame(p.pivot);
        let spun = |t: &mut Transform| {
            t.translation = about + turn * (t.translation - about);
            t.rotation = turn * t.rotation;
        };
        spun(&mut s.torso);
        spun(&mut s.head);
        for i in [L, R] {
            for t in [&mut s.eyes[i], &mut s.upper[i], &mut s.fore[i], &mut s.hands[i], &mut s.thighs[i], &mut s.shins[i], &mut s.feet[i]] {
                spun(t);
            }
            for p in [&mut s.shoulder[i], &mut s.elbow[i], &mut s.wrist[i], &mut s.hip[i], &mut s.knee[i], &mut s.ankle[i]] {
                *p = about + turn * (*p - about);
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reach_keeps_its_bones_and_bends_the_way_it_is_told() {
        let joint = Vec3::new(0.4, 1.2, 0.);
        for target in [Vec3::new(0.45, 0.55, -0.1), Vec3::new(0.9, 1.3, -0.4), Vec3::new(0.2, 2.5, 0.), Vec3::new(0.4, 1.19, 0.)] {
            let (upper, lower, end) = reach(joint, target, UPPER, FORE, Vec3::Z, Vec3::X);
            let elbow = joint + upper * Vec3::NEG_Y * UPPER;
            let tip = elbow + lower * Vec3::NEG_Y * FORE;
            assert!(tip.distance(end) < 1e-3, "{target}: {tip} {end}");
            assert!((elbow.distance(joint) - UPPER).abs() < 1e-4);
            if target.distance(joint) < (UPPER + FORE) * 0.99 && target.distance(joint) > 0.1 {
                assert!(end.distance(target) < 1e-3, "{target} reached");
            }
        }
        // Out of reach: as far as it goes, that way.
        let (_, _, end) = reach(joint, joint + Vec3::new(0., -5., 0.), UPPER, FORE, Vec3::Z, Vec3::X);
        assert!((end.distance(joint) - (UPPER + FORE) * 0.995).abs() < 1e-3);
    }
    #[test]
    fn at_rest_it_stands_on_its_feet_with_its_eyes_at_the_top() {
        let s = solve(&Pose::REST, None, 0.);
        for i in [L, R] {
            assert!((s.sole(i).y).abs() < 0.01, "sole {}", s.sole(i));
            assert!(s.wrist[i].y > 0.4 && s.wrist[i].y < 0.7, "the hands hang to the knees: {}", s.wrist[i]);
        }
        assert!((s.eye().y - 1.705).abs() < 0.03, "eye {}", s.eye());
        assert!(s.spot().0.z > 0.2, "the weak spot is at the back: {}", s.spot().0);
    }
    #[test]
    fn hand_at_puts_the_wrist_where_it_is_asked_however_the_body_is_turned() {
        let p = Pose::REST.lean(0.5).twist(0.3).tilt(-0.1).hips(0.05, -0.2, 0.1).hand_at(R, v(0.3, 0.3, 0.6), 1.).hand_at(L, v(0.2, 1.0, 0.4), 0.);
        let s = solve(&p, None, 0.);
        assert!(s.wrist[R].distance(limb_frame(R, v(0.3, 0.3, 0.6))) < 0.01, "{}", s.wrist[R]);
        assert!(s.wrist[L].distance(limb_frame(L, v(0.2, 1.0, 0.4))) < 0.01, "{}", s.wrist[L]);
    }
}

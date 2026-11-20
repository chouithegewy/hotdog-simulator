// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Kung Fu: how a hotdog fights with its bare hands and feet, in third person and in first.
//!
//! * **A pose** (`Pose`) is the whole body at one instant, written the way an animator thinks of it: where the hips
//!   are, how the body leans, twists, tilts and squashes, where each hand is (from its shoulder) and what shape it
//!   makes (`Hand`: fist, palm, claw, beak, finger jab, hook), where each foot is (on the counter or off it), and the
//!   face (`Face`: brows, eyes, mouth; a kiai is a wide-open shout). Positions are in metres in the body's own frame:
//!   x **out** from the middle for a limb (so one number serves either side) and to the right for the hips, y **up**,
//!   z **forward**. A hotdog is 0.155 m of sausage on legs of 0.032 m; its arms are 0.032 m long and stretch, rubber
//!   hose fashion, when a hand is put further than that (`STRAIGHT`).
//! * **A move** (`Anim`) is keyed poses on *beats*: 0 to 1 is the move's startup, 1 to 2 its active window (the blow
//!   lands on beat 1), 2 to 3 its recovery. Each beat is stretched to the simulation's own timing for the move
//!   (`Timing`, from `kitchen_sim::kungfu`), so an animation always lasts exactly as long as its move does, however the
//!   table is tuned. Between keys the pose follows a smooth curve through its neighbours (a Hermite spline: arcs, not
//!   straight lines), eased as the key says (`Ease`): a snap into contact, a settle out of it.
//! * **Solving** (`solve`) turns a pose into joint rotations for the rig in player_model.rs: two-bone reaches for the
//!   arms and legs, the wrist turned so the palm faces where it was asked to, the ankle flat on the counter or pointed.
//! * **Playing** (`Animator`, one per hotdog): which move, how far in, blended from whatever the body was doing (another
//!   move, the stance, the gun-game gait), frozen on contact for a hit-stop, feet that stay planted while the
//!   simulation slides the body, the bun jiggling after the body, and the impact flash, droplets and whoosh trails
//!   (fx.rs). In first person your own forearms, fists and feet come into view doing the same move (view.rs).
//! * **The moves themselves**: the shared fifteen (shared.rs) and each style's ten (tiger.rs, crane.rs, snake.rs,
//!   mantis.rs, monkey.rs, drunken.rs), gathered in `library`.
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

pub use kitchen_sim::kungfu as fu;

pub mod crane;
pub mod drunken;
pub mod fx;
pub mod mantis;
pub mod monkey;
pub mod rig;
pub mod shared;
pub mod sim;
pub mod snake;
pub mod tiger;
pub mod toad;
pub mod view;
#[cfg(test)]
mod tests;

pub use rig::Plugin;

// ----- The rig's measurements (player_model.rs)

/// Each limb is two bones of this length.
pub const BONE: f32 = crate::player_model::RIG.0;
/// The right hip and the right shoulder (the left ones are their mirror images), in the body's frame at rest.
pub const HIP: Vec3 = crate::player_model::RIG.1;
pub const SHOULDER: Vec3 = crate::player_model::RIG.2;
/// The torso pivots on the hips here.
pub const PIVOT: Vec3 = Vec3::new(0., 0.033, 0.);
/// A whole-body `spin` (rolls, flips, falls) turns about the middle of the body.
pub const CENTRE: Vec3 = Vec3::new(0., 0.09, 0.);
/// An ankle on the counter is this high (the foot's sole is on the counter).
pub const ANKLE: f32 = 0.001;
/// The eye, in the body's frame at rest (the game's camera is put there: arena.rs EYE).
pub const EYE: Vec3 = Vec3::new(0., 0.135, 0.);
/// The sausage: its axis runs up the middle from BOTTOM to TOP, and it is RADIUS round.
pub const BOTTOM: f32 = 0.046;
pub const TOP: f32 = 0.155;
pub const RADIUS: f32 = 0.023;
/// A limb's bones are this thick (their capsules' radius).
pub const LIMB: f32 = 0.004;
/// A limb reaching further than this share of its full length stretches instead of straightening: its elbow or
/// knee always keeps a little bend, so its hinge never flips.
pub const STRAIGHT: f32 = 0.996;
/// How far a limb may stretch at the most (a rubber-hose punch or kick), as a multiple of its length.
pub const STRETCH: f32 = 4.5;

/// Left (0) and right (1): limbs are indexed by side, the left one first, as in player_model's rig.
pub const L: usize = 0;
pub const R: usize = 1;
/// -1 for the left side, 1 for the right.
pub fn sign(side: usize) -> f32 {
    if side == L { -1. } else { 1. }
}
/// Shorthand for a position or a direction: out (or right), up, forward.
pub const fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
/// A limb's position or direction as written (out, up, forward) in the rig's frame (x right, y up, -z forward).
pub fn limb_frame(side: usize, at: Vec3) -> Vec3 {
    Vec3::new(sign(side) * at.x, at.y, -at.z)
}
/// The hips' offset as written (right, up, forward) in the rig's frame.
pub fn body_frame(at: Vec3) -> Vec3 {
    Vec3::new(at.x, at.y, -at.z)
}

// ----- Poses

/// The shape a hand makes. Each style has its own: Tiger claws and palms, Crane beaks and wings, Snake finger jabs,
/// Mantis hooks; fists for everybody.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Hand {
    /// Loose, the fingers a little curled: at rest, walking, hurt.
    #[default]
    Relaxed,
    Fist,
    /// Open and flat, the fingers together and the thumb tucked: a palm strike, a block, a bow.
    Palm,
    /// The fingers spread and curled at the tips: a tiger's claw.
    Claw,
    /// The fingertips pinched to a point: a crane's beak.
    Beak,
    /// The fingers straight and together, a spear: a snake's strike.
    Jab,
    /// The fingers curled over at the middle: a mantis's hook.
    Hook,
    /// Curled round an imaginary cup: the fingers together in a C, the thumb opposite (Drunken).
    Cup,
}
impl Hand {
    pub const ALL: [Hand; 8] = [Hand::Relaxed, Hand::Fist, Hand::Palm, Hand::Claw, Hand::Beak, Hand::Jab, Hand::Hook, Hand::Cup];
    pub fn index(self) -> usize {
        Hand::ALL.iter().position(|h| *h == self).unwrap_or(0)
    }
}

/// One arm: where the hand is, which way the elbow points, which way the palm faces, how far the wrist is flexed,
/// and the hand's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arm {
    /// Where the wrist is, from its own shoulder, in the chest's frame (out, up, forward). The arm hangs at
    /// (0, -0.031, 0.004); a full reach without stretching is 0.032; a rubber-hose punch reaches 0.06 or more.
    pub hand: Vec3,
    /// Which way the elbow points (out, up, forward); any length. Down and back by default.
    pub elbow: Vec3,
    /// Which way the palm faces with the wrist straight (out, up, forward): inwards by default, which makes a
    /// punch a vertical fist. (-0.3, -1, 0): palm down. Ignored where it runs along the forearm.
    pub palm: Vec3,
    /// The wrist bent towards the palm (+, a beak or a hook) or back (-, a palm strike with the fingers up), radians.
    pub flex: f32,
    pub shape: Hand,
}
impl Arm {
    pub const REST: Arm = Arm { hand: v(0.002, -0.031, 0.004), elbow: v(0.25, -0.2, -1.), palm: v(-1., 0., 0.), flex: 0., shape: Hand::Relaxed };
}

/// One leg: where the ankle is (out from the middle of the body, up from the counter, forward), which way the knee
/// points, how the foot is pointed, and how firmly it is planted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leg {
    /// The ankle, from the point on the counter under the middle of the body (out, up, forward). At rest
    /// (0.014, 0, 0): under its hip, on the counter. A full reach without stretching is 0.032 from the hip, which is
    /// 0.033 up.
    pub foot: Vec3,
    /// Which way the knee points (out, up, forward); forward by default.
    pub knee: Vec3,
    /// The toes pointed down (+) or pulled up (-), radians, when the foot is off the counter.
    pub toe: f32,
    /// 1: the foot is flat on the counter and stays where it was put while the body moves over it (the simulation's
    /// own movement included). 0: it is off the counter, and pointed as `toe` says.
    pub plant: f32,
}
impl Leg {
    pub const REST: Leg = Leg { foot: v(HIP.x, 0., 0.), knee: v(0., 0., 1.), toe: 0., plant: 1. };
}

/// The face: the brows, the eyes and the mouth.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Face {
    /// The brows' slant: inner ends down in anger or effort (-1) or up in pain or worry (+1).
    pub brow: f32,
    /// The brows raised (+1, surprise) or lowered (-1, a scowl).
    pub raise: f32,
    /// The mouth open: 0 shut (the smile), 1 a wide-open shout.
    pub mouth: f32,
    /// The eyes narrowed (-1, squeezed shut) or wide (+1).
    pub eyes: f32,
    /// The cheeks blown out like a croaking toad's (0 to 1); a puffed mouth stays shut.
    pub puff: f32,
}
impl Face {
    pub const CALM: Face = Face { brow: 0., raise: 0., mouth: 0., eyes: 0., puff: 0. };
    /// Ready to fight: the brows down a little.
    pub const FOCUS: Face = Face { brow: -0.45, raise: -0.25, mouth: 0., eyes: -0.15, puff: 0. };
    /// Effort: brows down hard, eyes narrowed, teeth set.
    pub const FIERCE: Face = Face { brow: -0.9, raise: -0.6, mouth: 0.15, eyes: -0.4, puff: 0. };
    /// The kiai: the shout on a heavy blow.
    pub const KIAI: Face = Face { brow: -1., raise: -0.5, mouth: 1., eyes: -0.55, puff: 0. };
    /// Hit: brows up in the middle, eyes squeezed, mouth open.
    pub const HURT: Face = Face { brow: 0.9, raise: 0.4, mouth: 0.6, eyes: -0.9, puff: 0. };
    /// Knocked silly: wide eyes, brows up, mouth slack.
    pub const DAZED: Face = Face { brow: 0.5, raise: 0.8, mouth: 0.35, eyes: 0.8, puff: 0. };
    /// Pleased with itself.
    pub const PROUD: Face = Face { brow: 0.1, raise: 0.5, mouth: 0., eyes: -0.35, puff: 0. };
}

/// The whole body at one instant (see the module's notes for the frame).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// The hips (and the body on them) moved from rest: right, up, forward. A deep stance is about 0.010 down.
    pub hips: Vec3,
    /// The body leant forward (+) or back (-) about the hips, radians.
    pub lean: f32,
    /// The chest turned to its left (+: the right shoulder comes forward) or right (-), radians.
    pub twist: f32,
    /// The body tilted over to its left (+) or right (-), radians.
    pub tilt: f32,
    /// Squashed (+, shorter and fatter: anticipation, landing) or stretched (-, taller: the reach of a blow). Up to
    /// about ±0.12.
    pub squash: f32,
    /// The body fatter round the middle without getting shorter (0 to 1): a toad's belly, an inhale.
    pub belly: f32,
    /// The whole body (legs too) turned about its middle (`CENTRE`): flipped forward (+x), spun to its left (+y),
    /// cartwheeled to its left (+z). For rolls, flips, falls and throws. Planted feet do not stay planted through it.
    pub spin: Vec3,
    pub arms: [Arm; 2],
    pub legs: [Leg; 2],
    pub face: Face,
}
impl Default for Pose {
    fn default() -> Self {
        Pose::REST
    }
}
impl Pose {
    /// Standing at rest, arms hanging.
    pub const REST: Pose = Pose { hips: Vec3::ZERO, lean: 0., twist: 0., tilt: 0., squash: 0., belly: 0., spin: Vec3::ZERO, arms: [Arm::REST; 2], legs: [Leg::REST; 2], face: Face::CALM };
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
    pub fn squash(mut self, amount: f32) -> Self {
        self.squash = amount;
        self
    }
    pub fn belly(mut self, amount: f32) -> Self {
        self.belly = amount;
        self
    }
    pub fn spin(mut self, flip: f32, turn: f32, roll: f32) -> Self {
        self.spin = v(flip, turn, roll);
        self
    }
    /// Puts the hand on `side` at `hand` (out, up, forward from its shoulder) in the shape `shape`.
    pub fn arm(mut self, side: usize, hand: Vec3, shape: Hand) -> Self {
        self.arms[side].hand = hand;
        self.arms[side].shape = shape;
        self
    }
    /// Puts the hand on `side` at `at` in the body's own frame (out from the middle, up from the counter, forward from
    /// the middle), wherever the body's lean, twist, tilt, squash and hips (set before this) have put the shoulder:
    /// the way to land a blow on a point in front of the hotdog. Two hotdogs touching are 0.09 apart, so a blow that
    /// lands is 0.07 to 0.15 ahead; chest height is about 0.10, the head 0.15.
    pub fn hand_at(mut self, side: usize, at: Vec3, shape: Hand) -> Self {
        let chest = torso_rotation(self.lean, self.twist, self.tilt);
        let scale = squash_scale(self.squash, self.belly);
        let shoulder = Vec3::new(sign(side) * SHOULDER.x, SHOULDER.y, 0.);
        let local = chest.inverse() * (limb_frame(side, at) - PIVOT - body_frame(self.hips)) + scale * PIVOT - scale * shoulder;
        self.arms[side].hand = Vec3::new(sign(side) * local.x, local.y, -local.z);
        self.arms[side].shape = shape;
        self
    }
    /// Moves the hand on `side` by `by` (out, up, forward, in the chest's frame) from where it is.
    pub fn nudge(mut self, side: usize, by: Vec3) -> Self {
        self.arms[side].hand += by;
        self
    }
    /// Points the elbow on `side` (out, up, forward).
    pub fn elbow(mut self, side: usize, towards: Vec3) -> Self {
        self.arms[side].elbow = towards;
        self
    }
    /// Turns the palm on `side` to face `towards` (out, up, forward) and bends the wrist by `flex`.
    pub fn palm(mut self, side: usize, towards: Vec3, flex: f32) -> Self {
        self.arms[side].palm = towards;
        self.arms[side].flex = flex;
        self
    }
    /// Plants the foot on `side` at `at` (out from the middle, up, forward): flat on the counter if `at.y` is 0.
    pub fn foot(mut self, side: usize, at: Vec3) -> Self {
        self.legs[side].foot = at;
        self.legs[side].plant = if at.y < 0.0015 { 1. } else { 0. };
        self
    }
    /// Lifts the foot on `side` to `at`, its toes pointed by `toe`, its knee towards `knee`.
    pub fn kick(mut self, side: usize, at: Vec3, knee: Vec3, toe: f32) -> Self {
        self.legs[side] = Leg { foot: at, knee, toe, plant: 0. };
        self
    }
    /// Points the knee on `side` (out, up, forward).
    pub fn knee(mut self, side: usize, towards: Vec3) -> Self {
        self.legs[side].knee = towards;
        self
    }
    pub fn face(mut self, face: Face) -> Self {
        self.face = face;
        self
    }
    /// Both hands the same shape.
    pub fn hands(mut self, shape: Hand) -> Self {
        self.arms[L].shape = shape;
        self.arms[R].shape = shape;
        self
    }
    /// The same pose the other way round: left for right.
    pub fn mirror(self) -> Self {
        Pose {
            hips: v(-self.hips.x, self.hips.y, self.hips.z),
            twist: -self.twist,
            tilt: -self.tilt,
            spin: v(self.spin.x, -self.spin.y, -self.spin.z),
            arms: [self.arms[R], self.arms[L]],
            legs: [self.legs[R], self.legs[L]],
            ..self
        }
    }
    /// Shifts the whole pose (hips and both feet) `by` (right, up, forward): a step.
    pub fn moved(mut self, by: Vec3) -> Self {
        self.hips += by;
        for (i, leg) in self.legs.iter_mut().enumerate() {
            leg.foot += v(sign(i) * by.x, 0., by.z);
        }
        self
    }
    /// A weighted sum of poses (weights summing to 1); the hand shapes are `shapes`'.
    fn weighted(parts: [(&Pose, f32); 4], shapes: &Pose) -> Pose {
        let f = |get: &dyn Fn(&Pose) -> f32| parts.iter().map(|(p, w)| get(p) * w).sum::<f32>();
        let g = |get: &dyn Fn(&Pose) -> Vec3| parts.iter().map(|(p, w)| get(p) * *w).sum::<Vec3>();
        let arm = |i: usize| Arm {
            hand: g(&|p| p.arms[i].hand),
            elbow: g(&|p| p.arms[i].elbow),
            palm: g(&|p| p.arms[i].palm),
            flex: f(&|p| p.arms[i].flex),
            shape: shapes.arms[i].shape,
        };
        let leg = |i: usize| Leg {
            foot: g(&|p| p.legs[i].foot),
            knee: g(&|p| p.legs[i].knee),
            toe: f(&|p| p.legs[i].toe),
            plant: f(&|p| p.legs[i].plant).clamp(0., 1.),
        };
        Pose {
            hips: g(&|p| p.hips),
            lean: f(&|p| p.lean),
            twist: f(&|p| p.twist),
            tilt: f(&|p| p.tilt),
            squash: f(&|p| p.squash),
            belly: f(&|p| p.belly).max(0.),
            spin: g(&|p| p.spin),
            arms: [arm(L), arm(R)],
            legs: [leg(L), leg(R)],
            face: Face {
                brow: f(&|p| p.face.brow).clamp(-1., 1.),
                raise: f(&|p| p.face.raise).clamp(-1., 1.),
                mouth: f(&|p| p.face.mouth).clamp(0., 1.),
                eyes: f(&|p| p.face.eyes).clamp(-1., 1.),
                puff: f(&|p| p.face.puff).clamp(0., 1.),
            },
        }
    }
    /// How far apart two poses are, roughly in metres: the hips, hands and feet, and the body's turns at its height.
    pub fn gap(&self, to: &Pose) -> f32 {
        let turns = (self.lean - to.lean).abs() + (self.twist - to.twist).abs() + (self.tilt - to.tilt).abs() + (self.spin - to.spin).length();
        let limbs: f32 = (0..2).map(|i| self.arms[i].hand.distance(to.arms[i].hand) + self.legs[i].foot.distance(to.legs[i].foot)).sum();
        self.hips.distance(to.hips) + limbs + 0.06 * turns + 0.02 * (self.squash - to.squash).abs()
    }
    /// Straight from this pose to `to`, `t` of the way (hand shapes change halfway).
    pub fn lerp(&self, to: &Pose, t: f32) -> Pose {
        let shapes = if t < 0.5 { self } else { to };
        Pose::weighted([(self, 1. - t), (to, t), (self, 0.), (self, 0.)], shapes)
    }
}

// ----- Moves

/// How a move eases into one of its keys from the key before.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Ease {
    /// Evenly.
    Linear,
    /// Speeding up into the key and slowing out of the last: the default.
    #[default]
    Smooth,
    /// Slowing down into the key: a wind-up settling into its cocked pose, a follow-through settling.
    Out,
    /// Speeding up all the way into the key: a blow snapping into contact.
    Snap,
    /// Speeding up into the key, gently.
    In,
}
impl Ease {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0., 1.);
        match self {
            Ease::Linear => t,
            Ease::Smooth => t * t * (3. - 2. * t),
            Ease::Out => 1. - (1. - t).powf(2.6),
            Ease::Snap => t.powf(2.4),
            Ease::In => t * t,
        }
    }
}

/// A key: a pose at a beat (0 to 1 startup, 1 to 2 active, 2 to 3 recovery), reached as `ease` says.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub at: f32,
    pub pose: Pose,
    pub ease: Ease,
}

/// A part of the body that lands a blow, or that leaves a whoosh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Limb {
    Hand(usize),
    Foot(usize),
    /// An elbow strike.
    Elbow(usize),
    /// The head (a butt) or the whole body (a shoulder charge, a fall).
    Head,
}

/// How a move's clock runs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Cycle {
    /// Once through its beats, timed by the simulation's table.
    #[default]
    Once,
    /// Round and round, this many seconds a turn (the idle).
    Loop(f32),
    /// Round and round with the stride (the walk): its clock is the gait's phase.
    Stride,
    /// Up to beat 1 and held there (beats 1 to 2 loop gently) while the simulation keeps it going; beats 2 to 3 play
    /// when it lets go (a block, lying knocked down).
    Hold,
}

/// One animation: its keys and how it is played.
#[derive(Clone, Debug)]
pub struct Anim {
    pub name: &'static str,
    pub keys: Vec<Key>,
    pub cycle: Cycle,
    /// The part of the body that lands the blow, for the flash, the hit-stop and the first person's camera.
    pub strike: Option<Limb>,
    /// The parts of the body that leave a whoosh while they move fast.
    pub trails: Vec<Limb>,
    /// A heavy blow: a longer hit-stop and a bigger flash.
    pub heavy: bool,
    /// What it does instead from beat 2 when it did not connect (a grab that caught nobody).
    pub miss: Option<Box<Anim>>,
    /// Seconds it stands still on contact, if not the usual (`STOP`, `HEAVY_STOP`).
    pub hitstop: Option<f32>,
    /// How hard the first person's camera is jolted when it lands (1 the usual).
    pub shake: f32,
    /// The ground shakes as the move passes these beats, this hard (1 a stomp): a ripple on the counter under the body
    /// and a jolt for every camera near it.
    pub quakes: Vec<(f32, f32)>,
}
impl Anim {
    /// An animation called `name` that starts from `first` on beat 0.
    pub fn new(name: &'static str, first: Pose) -> Anim {
        Anim { name, keys: vec![Key { at: 0., pose: first, ease: Ease::Smooth }], cycle: Cycle::Once, strike: None, trails: vec![], heavy: false, miss: None, hitstop: None, shake: 1., quakes: vec![] }
    }
    /// It stands still for `seconds` on contact instead of the usual.
    pub fn hitstop(mut self, seconds: f32) -> Anim {
        self.hitstop = Some(seconds);
        self
    }
    /// Its contact jolts the first-person camera `scale` times as hard as usual.
    pub fn shake(mut self, scale: f32) -> Anim {
        self.shake = scale;
        self
    }
    /// The ground shakes on `beat`, `strength` hard (1 a stomp).
    pub fn quake(mut self, beat: f32, strength: f32) -> Anim {
        self.quakes.push((beat, strength));
        self
    }
    /// From beat 2, when the move has not connected, play `other` instead (it should match this one up to beat 2).
    pub fn miss(mut self, other: Anim) -> Anim {
        self.miss = Some(Box::new(other));
        self
    }
    /// Then `pose` on beat `at`, reached as `ease` says. Beats must rise.
    pub fn key(mut self, at: f32, pose: Pose, ease: Ease) -> Anim {
        self.keys.push(Key { at, pose, ease });
        self
    }
    /// `limb` lands the blow (and leaves a whoosh).
    pub fn strike(mut self, limb: Limb) -> Anim {
        self.strike = Some(limb);
        if !self.trails.contains(&limb) {
            self.trails.push(limb);
        }
        self
    }
    /// `limb` leaves a whoosh too.
    pub fn trail(mut self, limb: Limb) -> Anim {
        if !self.trails.contains(&limb) {
            self.trails.push(limb);
        }
        self
    }
    pub fn heavy(mut self) -> Anim {
        self.heavy = true;
        self
    }
    pub fn cycle(mut self, cycle: Cycle) -> Anim {
        self.cycle = cycle;
        self
    }
    /// Seconds the move stands still on contact when it lands.
    pub fn stop(&self) -> f32 {
        self.hitstop.unwrap_or(if self.heavy { HEAVY_STOP } else { STOP })
    }
    /// The pose on `beat` (0 to 3).
    pub fn pose_at(&self, beat: f32) -> Pose {
        let keys = &self.keys;
        let n = keys.len();
        if n == 1 || beat <= keys[0].at {
            return keys[0].pose;
        }
        if beat >= keys[n - 1].at {
            return keys[n - 1].pose;
        }
        let i = keys.iter().rposition(|k| k.at <= beat).unwrap_or(0).min(n - 2);
        let (k1, k2) = (&keys[i], &keys[i + 1]);
        let span = (k2.at - k1.at).max(1e-4);
        let e = k2.ease.apply((beat - k1.at) / span);
        if k1.pose == k2.pose {
            return k1.pose;
        }
        // A Hermite curve through the keys: its tangent at a key runs from the key before to the key after (none at
        // the first and the last), scaled to the time either side, so the limbs swing in arcs through the keys.
        // (A key between a big move and a hold, or at a turn back, keeps its tangent small: no overshoot into a hold.)
        let (prev, next) = (i.checked_sub(1).map(|j| &keys[j]), keys.get(i + 2));
        let calm = |a: f32, b: f32| (2. * a.min(b) / a.max(b).max(1e-6)).min(1.);
        let here = k1.pose.gap(&k2.pose);
        let c1 = prev.map_or(0., |k0| TENSION * span / (k2.at - k0.at).max(1e-4) * calm(k0.pose.gap(&k1.pose), here));
        let c2 = next.map_or(0., |k3| TENSION * span / (k3.at - k1.at).max(1e-4) * calm(here, k2.pose.gap(&k3.pose)));
        let (h00, h01, h10, h11) = {
            let (t2, t3) = (e * e, e * e * e);
            (2. * t3 - 3. * t2 + 1., -2. * t3 + 3. * t2, t3 - 2. * t2 + e, t3 - t2)
        };
        let p0 = prev.map_or(&k1.pose, |k| &k.pose);
        let p3 = next.map_or(&k2.pose, |k| &k.pose);
        let shapes = if e < 0.35 { &k1.pose } else { &k2.pose };
        Pose::weighted([(p0, -h10 * c1), (&k1.pose, h00 - h11 * c2), (&k2.pose, h01 + h10 * c1), (p3, h11 * c2)], shapes)
    }
    /// The pose `t` seconds into the move timed by `timing` (for a `Loop`, `t` is the idle's clock; for a `Stride`,
    /// the gait's phase in radians).
    pub fn sample(&self, t: f32, timing: Timing) -> Pose {
        self.pose_at(self.beat(t, timing))
    }
    /// The beat `t` seconds in (see `sample`).
    pub fn beat(&self, t: f32, timing: Timing) -> f32 {
        match self.cycle {
            Cycle::Once | Cycle::Hold => timing.beat(t),
            Cycle::Loop(seconds) => (t / seconds.max(0.05)).rem_euclid(1.) * 3.,
            Cycle::Stride => (t / TAU).rem_euclid(1.) * 3.,
        }
    }
    /// The beat of the key that lands the blow: the first key in the active window.
    pub fn contact(&self) -> f32 {
        self.keys.iter().map(|k| k.at).find(|at| (1.0..=2.0).contains(at)).unwrap_or(1.)
    }
}
/// How far a key's tangent reaches towards its neighbours (1 is a Catmull-Rom curve; less overshoots less).
const TENSION: f32 = 0.8;
/// Seconds a blow stands still on contact (about three frames, five for a heavy one).
pub const STOP: f32 = 0.055;
pub const HEAVY_STOP: f32 = 0.095;

/// A move's timing in seconds, as the simulation's table has it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub startup: f32,
    pub active: f32,
    pub recovery: f32,
}
impl Default for Timing {
    fn default() -> Self {
        Timing { startup: 0.12, active: 0.06, recovery: 0.22 }
    }
}
impl Timing {
    pub fn total(&self) -> f32 {
        self.startup + self.active + self.recovery
    }
    /// The beat `t` seconds into the move.
    pub fn beat(&self, t: f32) -> f32 {
        // (A phase the table gives no time is skipped: a parry is in its active window from its first instant.)
        let (s, a, r) = (self.startup, self.active, self.recovery);
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
    /// The second at which `beat` comes.
    pub fn time(&self, beat: f32) -> f32 {
        if beat < 1. {
            beat * self.startup
        } else if beat < 2. {
            self.startup + (beat - 1.) * self.active
        } else {
            self.startup + self.active + (beat - 2.).min(1.) * self.recovery
        }
    }
}

// ----- Styles and the library

/// A style: its name, the stance its hotdog fights from, and its ten moves in the order of `Slot`.
#[derive(Clone, Debug)]
pub struct Style {
    pub name: &'static str,
    pub stance: Pose,
    /// Light 1, light 2, light 3 (a chain), heavy 1, heavy 2, kick 1, kick 2, the jump attack, the crouching sweep,
    /// the signature special.
    pub moves: Vec<Anim>,
    /// Its own idle (a `Cycle::Loop` on its stance), instead of the shared one's breathing.
    pub idle: Option<Anim>,
    /// Its own walk (a `Cycle::Stride` on its stance), instead of the shared shuffle.
    pub walk: Option<Anim>,
}
/// The fifteen moves every style shares, in this order (built on each style's stance by shared.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Common {
    Idle,
    Walk,
    BlockHigh,
    BlockLow,
    Parry,
    DodgeLeft,
    DodgeRight,
    BackHop,
    HitLight,
    HitHeavy,
    Knockdown,
    GetUp,
    Grab,
    Thrown,
    Victory,
}
pub const COMMON: usize = 15;
pub const SLOTS: usize = 10;
pub const STYLES: usize = fu::STYLES;

/// Every style's animations: its stance, the shared fifteen built on that stance, and its own ten.
pub struct Set {
    pub name: &'static str,
    pub stance: Pose,
    pub common: Vec<Anim>,
    pub moves: Vec<Anim>,
}
pub struct Library {
    pub sets: Vec<Set>,
}
/// The styles in the simulation's order (kitchen_sim::kungfu `Style`).
fn styles() -> [Style; STYLES] {
    [tiger::style(), crane::style(), snake::style(), mantis::style(), monkey::style(), drunken::style(), toad::style()]
}
/// All the animations, built once.
pub fn library() -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(|| Library {
        sets: styles()
            .into_iter()
            .enumerate()
            .map(|(i, style)| {
                let mut common = shared::anims(&style.stance, i);
                if let Some(idle) = style.idle {
                    common[Common::Idle as usize] = idle;
                }
                if let Some(walk) = style.walk {
                    common[Common::Walk as usize] = walk;
                }
                Set { name: style.name, stance: style.stance, common, moves: style.moves }
            })
            .collect(),
    })
}
impl Library {
    /// The set for `style` (the first if there is no such style).
    pub fn set(&self, style: usize) -> &Set {
        &self.sets[style.min(self.sets.len() - 1)]
    }
    /// The animation of move `mv` for a hotdog fighting in `style`: a shared move on that style's stance, or the
    /// move's own style's.
    pub fn anim(&self, style: usize, mv: fu::Move) -> Option<&Anim> {
        match (mv.style(), mv.slot()) {
            (Some(own), Some(slot)) => self.set(own.index()).moves.get(slot as usize),
            _ => self.set(style).common.get(mv.id() as usize),
        }
    }
}
impl Style {
    /// The same style with every key of every move (its idle and walk too) checked against the body: a forearm or hand
    /// that its lean or turn has put inside the sausage is moved out in front of it, an elbow out to the side
    /// (`shared::clear`).
    pub fn cleared(mut self) -> Style {
        let fix = |anim: &mut Anim| {
            for key in anim.keys.iter_mut() {
                key.pose = shared::clear(key.pose);
            }
            if let Some(miss) = anim.miss.as_mut() {
                for key in miss.keys.iter_mut() {
                    key.pose = shared::clear(key.pose);
                }
            }
        };
        self.moves.iter_mut().for_each(fix);
        self.idle.iter_mut().for_each(fix);
        self.walk.iter_mut().for_each(fix);
        self.stance = shared::clear(self.stance);
        self
    }
    /// A style nobody has animated yet: its stance is the shared one, and its moves are plain punches and kicks, so
    /// that a hotdog fighting in it still fights.
    pub fn unfinished(name: &'static str) -> Style {
        let stance = shared::stance();
        let punch = |name: &'static str, side: usize| {
            let wind = stance.arm(side, v(0.012, -0.004, -0.004), Hand::Fist).twist(-0.25 * sign(side) * -1.);
            let hit = stance.arm(side, v(-0.03, 0.006, 0.05), Hand::Fist).twist(0.4 * -sign(side)).lean(0.15);
            Anim::new(name, stance).key(0.7, wind, Ease::Out).key(1., hit, Ease::Snap).key(1.6, hit, Ease::Out).key(3., stance, Ease::Smooth).strike(Limb::Hand(side))
        };
        let kick = |name: &'static str| {
            let lift = stance.kick(R, v(0.01, 0.025, 0.02), v(0., 1., 1.), 0.4).lean(-0.1);
            let hit = stance.kick(R, v(0.0, 0.03, 0.07), v(0., 1., 0.3), 0.8).lean(-0.3);
            Anim::new(name, stance).key(0.7, lift, Ease::Out).key(1., hit, Ease::Snap).key(1.7, lift, Ease::Out).key(3., stance, Ease::Smooth).strike(Limb::Foot(R))
        };
        Style {
            name,
            stance,
            idle: None,
            walk: None,
            moves: vec![
                punch("light 1", R),
                punch("light 2", L),
                punch("light 3", R),
                punch("heavy 1", R).heavy(),
                punch("heavy 2", L).heavy(),
                kick("kick 1"),
                kick("kick 2"),
                kick("jump attack"),
                kick("sweep"),
                punch("special", R).heavy(),
            ],
        }
    }
}

// ----- Solving a pose for the rig

/// A pose solved into the rotations of player_model's rig (its `Rig` joints), plus what the rig has that the gun
/// game does not use: the hands' shapes and wrists, the feet, the leg stretch, the body's squash and spin, the face.
/// Blending happens here, joint by joint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solved {
    /// Moves the hips (and the torso pivoting on them).
    pub shift: Vec3,
    pub torso: Quat,
    pub squash: f32,
    pub belly: f32,
    /// The whole body turned about `CENTRE`.
    pub spin: Quat,
    pub shoulders: [Quat; 2],
    pub elbows: [Quat; 2],
    /// The arms' stretch (1 is their own length).
    pub reach: [f32; 2],
    /// Each hand on its forearm.
    pub wrists: [Quat; 2],
    pub hands: [Hand; 2],
    pub hips: [Quat; 2],
    pub knees: [Quat; 2],
    /// The legs' stretch.
    pub stride: [f32; 2],
    /// Each foot on its shin.
    pub ankles: [Quat; 2],
    pub face: Face,
}
/// Two-bone reach from `joint` to `target` with bones `bone` long, the middle joint pushed towards `pole`. The limb
/// stretches when the target is further than STRAIGHT of its length (up to STRETCH). Returns the joint's rotation (its
/// -y down the first bone, its +z the way the middle joint points), the bend of the hinge about its x, and the stretch.
/// Where the limb points along `pole` (which then says nothing), the middle joint turns smoothly towards `fallback`
/// instead of flipping.
pub fn reach(joint: Vec3, target: Vec3, bone: f32, pole: Vec3, fallback: Vec3) -> (Quat, f32, f32) {
    let to = target - joint;
    let along = to.normalize_or(Vec3::NEG_Y);
    let d = to.length().clamp(bone * 0.2, bone * 2. * STRAIGHT * STRETCH);
    let stretch = (d / (2. * bone * STRAIGHT)).max(1.);
    let b = bone * stretch;
    let half = d * 0.5;
    let first = pole - along * pole.dot(along);
    let second = fallback - along * fallback.dot(along);
    let k = first.length() / pole.length().max(1e-6);
    let w = ((0.45 - k) / 0.45).clamp(0., 1.);
    let out = (first.normalize_or_zero() * (1. - w) + second.normalize_or_zero() * w).normalize_or(along.any_orthonormal_vector());
    let rise = (b * b - half * half).max(0.).sqrt();
    let upper = (along * half + out * rise) / b;
    let fore = (along * half - out * rise) / b;
    let bend = upper.dot(fore).clamp(-1., 1.).acos();
    let z = (out - upper * out.dot(upper)).normalize_or(Vec3::Z);
    let y = -upper;
    (Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z)), bend, stretch)
}
/// The torso's scale for a squash and a belly.
pub fn squash_scale(squash: f32, belly: f32) -> Vec3 {
    let s = squash.clamp(-0.2, 0.2);
    let b = 0.22 * belly.clamp(0., 1.5);
    Vec3::new(1. + 0.5 * s + b, 1. - s, 1. + 0.5 * s + b)
}
/// The rotation of a whole-body spin (see `Pose::spin`).
pub fn spin_rotation(spin: Vec3) -> Quat {
    Quat::from_rotation_y(spin.y) * Quat::from_rotation_z(spin.z) * Quat::from_rotation_x(-spin.x)
}
/// The torso's rotation for a lean, a twist and a tilt.
pub fn torso_rotation(lean: f32, twist: f32, tilt: f32) -> Quat {
    Quat::from_rotation_y(twist) * Quat::from_rotation_z(tilt) * Quat::from_rotation_x(-lean)
}
/// Solves `p` for the rig.
pub fn solve(p: &Pose) -> Solved {
    let torso = torso_rotation(p.lean, p.twist, p.tilt);
    let shift = body_frame(p.hips);
    let scale = squash_scale(p.squash, p.belly);
    let mut s = Solved {
        shift,
        torso,
        squash: p.squash,
        belly: p.belly,
        spin: spin_rotation(p.spin),
        shoulders: [Quat::IDENTITY; 2],
        elbows: [Quat::IDENTITY; 2],
        reach: [1.; 2],
        wrists: [Quat::IDENTITY; 2],
        hands: [p.arms[L].shape, p.arms[R].shape],
        hips: [Quat::IDENTITY; 2],
        knees: [Quat::IDENTITY; 2],
        stride: [1.; 2],
        ankles: [Quat::IDENTITY; 2],
        face: p.face,
    };
    for i in [L, R] {
        // The arm, in the torso's frame (the shoulder where the squashed torso has it).
        let a = &p.arms[i];
        let joint = Vec3::new(sign(i) * SHOULDER.x, SHOULDER.y, 0.) * scale;
        let (q, bend, stretch) = reach(joint, joint + limb_frame(i, a.hand), BONE, limb_frame(i, a.elbow), limb_frame(i, v(1., -0.3, 0.)));
        s.shoulders[i] = q;
        s.elbows[i] = Quat::from_rotation_x(bend);
        s.reach[i] = stretch;
        // The hand: its fingers run on from the forearm (its -y), its palm (its -z) faces as near `palm` as the forearm
        // allows (where `palm` runs along the forearm, the inside of the elbow), and the wrist flexes towards the palm.
        let fore = q * s.elbows[i];
        let along = fore * Vec3::NEG_Y;
        let natural = fore * Vec3::NEG_Z;
        let want = limb_frame(i, a.palm).normalize_or(natural);
        let across = want - along * want.dot(along);
        let palm = (across + natural * (0.35 - across.length()).max(0.)).normalize_or(natural);
        let (y, z) = (-along, -palm);
        let hand = Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z)) * Quat::from_rotation_x(a.flex);
        s.wrists[i] = fore.inverse() * hand;
        // The leg, in the body's frame: the hip moves with the hips.
        let l = &p.legs[i];
        let hip = Vec3::new(sign(i) * HIP.x, HIP.y, 0.) + shift;
        let foot = limb_frame(i, l.foot) + Vec3::Y * ANKLE;
        let (q, bend, stretch) = reach(hip, foot, BONE, limb_frame(i, l.knee), limb_frame(i, v(0.6, 0., 1.)));
        s.hips[i] = q;
        s.knees[i] = Quat::from_rotation_x(bend);
        s.stride[i] = stretch;
        // The foot: its toes (its +z) the way the knee points, pointed by `toe`; or, planted, flat on the counter.
        let shin = q * s.knees[i];
        let pointed = Quat::from_rotation_x(l.toe);
        let ahead = shin * Vec3::Z;
        let level = Vec3::new(ahead.x, 0., ahead.z).normalize_or(limb_frame(i, v(0.15, 0., 1.)).normalize());
        let flat = shin.inverse() * Quat::from_mat3(&Mat3::from_cols(Vec3::Y.cross(level), Vec3::Y, level));
        s.ankles[i] = pointed.slerp(flat, l.plant.clamp(0., 1.));
    }
    s
}
/// A quaternion slerp that takes the short way round.
fn turn(a: Quat, b: Quat, t: f32) -> Quat {
    let b = if a.dot(b) < 0. { -b } else { b };
    a.slerp(b, t)
}
impl Solved {
    /// `t` of the way from this to `to`, joint by joint.
    pub fn blend(&self, to: &Solved, t: f32) -> Solved {
        let t = t.clamp(0., 1.);
        let q2 = |a: [Quat; 2], b: [Quat; 2]| [turn(a[0], b[0], t), turn(a[1], b[1], t)];
        let f2 = |a: [f32; 2], b: [f32; 2]| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let f = |a: f32, b: f32| a + (b - a) * t;
        Solved {
            shift: self.shift.lerp(to.shift, t),
            torso: turn(self.torso, to.torso, t),
            squash: f(self.squash, to.squash),
            belly: f(self.belly, to.belly),
            spin: turn(self.spin, to.spin, t),
            shoulders: q2(self.shoulders, to.shoulders),
            elbows: q2(self.elbows, to.elbows),
            reach: f2(self.reach, to.reach),
            wrists: q2(self.wrists, to.wrists),
            hands: if t < 0.5 { self.hands } else { to.hands },
            hips: q2(self.hips, to.hips),
            knees: q2(self.knees, to.knees),
            stride: f2(self.stride, to.stride),
            ankles: q2(self.ankles, to.ankles),
            face: Face {
                brow: f(self.face.brow, to.face.brow),
                raise: f(self.face.raise, to.face.raise),
                mouth: f(self.face.mouth, to.face.mouth),
                eyes: f(self.face.eyes, to.face.eyes),
                puff: f(self.face.puff, to.face.puff),
            },
        }
    }
    /// The gun game's pose (player_model's gait) in these terms: no squash or spin, loose hands, feet along the shins.
    pub fn from_gait(p: &crate::player_model::Pose) -> Solved {
        Solved {
            shift: p.shift,
            torso: p.torso,
            squash: 0.,
            belly: 0.,
            spin: Quat::IDENTITY,
            shoulders: p.shoulders,
            elbows: p.elbows,
            reach: p.reach,
            wrists: [Quat::IDENTITY; 2],
            hands: [Hand::Relaxed; 2],
            hips: p.hips,
            knees: p.knees,
            stride: [1.; 2],
            ankles: [Quat::IDENTITY; 2],
            face: Face::CALM,
        }
    }
    /// The torso entity's place: (translation, rotation, scale) relative to the hotdog's root, spin included.
    pub fn torso_place(&self) -> (Vec3, Quat, Vec3) {
        let scale = squash_scale(self.squash, self.belly);
        let at = PIVOT + self.shift - self.torso * (scale * PIVOT);
        (CENTRE + self.spin * (at - CENTRE), self.spin * self.torso, scale)
    }
    /// A hip joint entity's place (translation, rotation) relative to the root, spin included.
    pub fn hip_place(&self, i: usize) -> (Vec3, Quat) {
        let at = Vec3::new(sign(i) * HIP.x, HIP.y, 0.) + self.shift;
        (CENTRE + self.spin * (at - CENTRE), self.spin * self.hips[i])
    }
    /// Where everything is, in the hotdog's own frame (its root's; y up from the counter, -z ahead).
    pub fn skeleton(&self) -> Skeleton {
        let (at, chest, scale) = self.torso_place();
        let place = |v: Vec3| at + chest * (scale * v);
        let mut k = Skeleton { chest, pivot: place(PIVOT), eye: place(EYE), head: place(Vec3::Y * 0.178), ..Skeleton::default() };
        for i in [L, R] {
            let b = BONE * self.reach[i];
            let shoulder = place(Vec3::new(sign(i) * SHOULDER.x, SHOULDER.y, 0.));
            let upper = chest * self.shoulders[i];
            let fore = upper * self.elbows[i];
            k.shoulder[i] = shoulder;
            k.elbow[i] = shoulder + upper * Vec3::NEG_Y * b;
            k.wrist[i] = k.elbow[i] + fore * Vec3::NEG_Y * b;
            k.hand[i] = fore * self.wrists[i];
            let (hip, thigh) = self.hip_place(i);
            let b = BONE * self.stride[i];
            let shin = thigh * self.knees[i];
            k.hip[i] = hip;
            k.knee[i] = hip + thigh * Vec3::NEG_Y * b;
            k.ankle[i] = k.knee[i] + shin * Vec3::NEG_Y * b;
            k.foot[i] = shin * self.ankles[i];
        }
        k
    }
}
/// Where the joints are, in the hotdog's own frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Skeleton {
    pub chest: Quat,
    pub pivot: Vec3,
    pub eye: Vec3,
    pub head: Vec3,
    pub shoulder: [Vec3; 2],
    pub elbow: [Vec3; 2],
    pub wrist: [Vec3; 2],
    /// Each hand's turn: its fingers along its -y, its palm facing its -z.
    pub hand: [Quat; 2],
    pub hip: [Vec3; 2],
    pub knee: [Vec3; 2],
    pub ankle: [Vec3; 2],
    /// Each foot's turn: its toes along its +z, its sole under its -y.
    pub foot: [Quat; 2],
}
impl Skeleton {
    /// The point of `limb` that lands a blow: the knuckles or fingertips, the ball of the foot, the forehead.
    pub fn tip(&self, limb: Limb) -> Vec3 {
        match limb {
            Limb::Hand(i) => self.wrist[i] + self.hand[i] * Vec3::NEG_Y * 0.008,
            Limb::Foot(i) => self.ankle[i] + self.foot[i] * v(0., -0.001, 0.006),
            Limb::Elbow(i) => self.elbow[i],
            Limb::Head => self.head + self.chest * v(0., -0.015, -0.02),
        }
    }
    /// The second point a whoosh is drawn from, back along `limb` from its tip.
    pub fn heel(&self, limb: Limb) -> Vec3 {
        match limb {
            Limb::Hand(i) => self.elbow[i].lerp(self.wrist[i], 0.55),
            Limb::Foot(i) => self.knee[i].lerp(self.ankle[i], 0.55),
            Limb::Elbow(i) => self.elbow[i].lerp(self.shoulder[i], 0.5),
            Limb::Head => self.head.lerp(self.pivot, 0.25),
        }
    }
}

// ----- Playing a hotdog's moves

/// What the gait says of how a hotdog moves this frame (player_model's `Motion`): speed, direction and the stride's
/// phase, crouching, off the ground.
#[derive(Clone, Copy, Debug, Default)]
pub struct Locomotion {
    pub forward: f32,
    pub strafe: f32,
    pub phase: f32,
    pub crouch: bool,
    pub airborne: bool,
}
/// What one animator produced this frame: the pose, a blow that landed now (the limb, and how hard the first
/// person's camera is jolted: 1 a light blow), and how hard the ground shook now (0: not at all).
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub solved: Solved,
    pub landed: Option<(Limb, f32)>,
    pub quake: f32,
    /// The parts of the body that leave a whoosh now.
    pub trails: [Option<Limb>; 2],
}
/// One hotdog's animation state, from frame to frame.
#[derive(Clone, Debug, Default)]
pub struct Animator {
    /// The move it is in and its clock when last seen.
    action: Option<fu::Move>,
    t: f32,
    /// The pose last shown, and the blend from it to whatever follows: from, starting when, for how long.
    last: Option<Solved>,
    from: Option<(Solved, f32, f32)>,
    /// The animator's own clock, in seconds.
    clock: f32,
    /// The blow of this move has landed (`felt`), and the hit-stop: from which second of the move, for how long.
    felt: bool,
    stop: Option<(f32, f32)>,
    /// Smoothed: moving (0 to 1), crouching, off the ground.
    drive: f32,
    crouch: f32,
    air: f32,
    /// The style it was last in, to blend a change of stance.
    style: Option<usize>,
    /// The beat of the move last shown (for the quakes it passes).
    beat: f32,
}
/// Seconds a change of move blends over (at most half the new move's startup), and into or out of the stance.
const BLEND: f32 = 0.07;
/// Radians a second a hand may turn on its wrist at the most.
const WRIST_SPEED: f32 = 120.;
const SETTLE: f32 = 0.14;
impl Animator {
    /// The pose shown last.
    pub fn last(&self) -> Option<&Solved> {
        self.last.as_ref()
    }
    /// One frame of fighter `f`, `dt` seconds after the last: `gait` is the gun game's pose for it (blended from on
    /// the first frame), `moving` how its gait says it moves.
    pub fn update(&mut self, f: &sim::Fighter, moving: Locomotion, gait: &Solved, dt: f32) -> Frame {
        let dt = dt.clamp(0., 0.1);
        self.clock += dt;
        let lib = library();
        let set = lib.set(f.style);
        let ease = |rate: f32| 1. - (-dt * rate).exp();
        let speed = moving.forward.hypot(moving.strafe).min(1.);
        self.drive += ((speed > 0.06) as u8 as f32 * speed.max(0.5) - self.drive) * ease(10.);
        self.crouch += (moving.crouch as u8 as f32 - self.crouch) * ease(12.);
        self.air += (moving.airborne as u8 as f32 - self.air) * ease(14.);
        let anim = f.action.and_then(|m| lib.anim(f.style, m));
        // A new move, the same one again from its start, or a change of style: blend from what was shown.
        let fresh = f.action != self.action || (f.action.is_some() && f.t + 1e-3 < self.t);
        if fresh || self.style != Some(f.style) || self.last.is_none() {
            let from = self.last.unwrap_or(*gait);
            let length = if anim.is_some() { BLEND.min(f.timing.startup * 0.5).max(0.025) } else { SETTLE };
            self.from = Some((from, self.clock, length));
            (self.felt, self.stop, self.beat) = (false, None, -1.);
            // A blow taken stands still for a moment, as the blow that landed it does.
            if matches!(f.action, Some(fu::Move::HitLight | fu::Move::HitHeavy)) {
                self.stop = Some((0., STOP));
            }
        }
        self.action = f.action;
        self.t = f.t;
        self.style = Some(f.style);
        // The blow lands: stand still on contact (if it is told of in time) and say so once.
        let mut landed = None;
        if let (Some(a), true) = (anim, f.connected && !self.felt) {
            let contact = f.timing.time(a.contact());
            if f.t >= contact - 0.02 {
                self.felt = true;
                if let Some(limb) = a.strike {
                    landed = Some((limb, if a.heavy { 1.9 } else { 1. } * a.shake));
                }
                if f.t - contact < 0.1 {
                    self.stop = Some((f.t.max(contact), a.stop()));
                }
            }
        }
        let mut quake = 0f32;
        let pose = match anim {
            Some(a) => {
                let t = shown(f.t, self.stop, f.timing.total());
                let beat = a.beat(t, f.timing);
                for (at, strength) in &a.quakes {
                    if self.beat < *at && beat >= *at {
                        quake = quake.max(*strength);
                    }
                }
                self.beat = beat;
                let a = match &a.miss {
                    Some(miss) if !f.connected && beat > 2. => miss,
                    _ => a,
                };
                let pose = match a.cycle {
                    Cycle::Hold if f.held => a.pose_at(held(beat, self.clock)),
                    _ => a.pose_at(beat),
                };
                // (The keys are written from where the move began; the simulation has carried the body on since.)
                pose.moved(-f.carried)
            }
            None => self.stance(set, moving),
        };
        let target = solve(&pose);
        let mut solved = match self.from {
            Some((from, start, length)) if self.clock - start < length => from.blend(&target, smooth((self.clock - start) / length)),
            _ => target,
        };
        // A hand rolls round its forearm no faster than WRIST_SPEED: where a palm's hint runs along the forearm its roll
        // is undefined, and would otherwise flip in a frame.
        if let Some(last) = &self.last {
            for i in [L, R] {
                let (from, to) = (last.wrists[i], solved.wrists[i]);
                let to = if from.dot(to) < 0. { -to } else { to };
                let angle = from.angle_between(to);
                let most = WRIST_SPEED * dt;
                if angle > most {
                    solved.wrists[i] = from.slerp(to, most / angle);
                }
            }
        }
        self.last = Some(solved);
        let mut trails = [None; 2];
        if let Some(a) = anim.filter(|a| a.cycle == Cycle::Once) {
            let beat = f.timing.beat(f.t);
            if (0.4..2.4).contains(&beat) {
                for (slot, limb) in trails.iter_mut().zip(a.trails.iter()) {
                    *slot = Some(*limb);
                }
            }
        }
        Frame { solved, landed, quake, trails }
    }
    /// Between moves: the style's stance breathing, stepping as it walks, sunk as it crouches, tucked in the air.
    fn stance(&self, set: &Set, moving: Locomotion) -> Pose {
        let idle = set.common[Common::Idle as usize].sample(self.clock, Timing::default());
        let walk = set.common[Common::Walk as usize].sample(moving.phase, Timing::default());
        let mut pose = idle.lerp(&walk, self.drive);
        if self.crouch > 0.01 {
            pose = pose.lerp(&shared::crouched(&pose), self.crouch);
        }
        if self.air > 0.01 {
            pose = pose.lerp(&shared::airborne(&set.stance), self.air);
        }
        pose
    }
}
/// The second of a move to show `t` seconds in, with a hit-stop: frozen for its length from its start, then caught
/// up over the rest of the move, so that the animation still ends when the move does.
fn shown(t: f32, stop: Option<(f32, f32)>, total: f32) -> f32 {
    let Some((at, length)) = stop else {
        return t;
    };
    if t < at {
        t
    } else if t < at + length {
        at
    } else {
        let rest = (total - at).max(1e-3);
        at + (t - at - length) * rest / (rest - length).max(1e-3)
    }
}
/// A held move's beat: up to 1, then breathing between 1 and 1.5 for as long as it is held.
fn held(beat: f32, clock: f32) -> f32 {
    if beat < 1. { beat } else { 1. + 0.25 * (1. - (clock * 2.4).cos()) }
}
pub(crate) fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
#[allow(dead_code)]
const _ANGLES: (f32, f32) = (PI, TAU);

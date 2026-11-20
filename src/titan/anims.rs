// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Every move the titan makes, keyed on beats as Kung Fu's are (kungfu/mod.rs): 0 to 1 is the move's startup (its
//! **tell**: the wind-up a small hotdog has to read), 1 to 2 its active window (the blow lands on beat 1), 2 to 3 its
//! recovery. Each beat is stretched to the simulation's own timing (`kitchen_sim::titan::TABLE`, and the view's `len`
//! for a charge, a jump's time in the air and a stagger), so an animation always lasts exactly as long as its move.
//! Between keys the pose follows a Hermite curve through its neighbours, eased as the key says.
//!
//! The walk, the crouched knuckle-walk and the charge's run are not keyed but worked out from the stride's phase
//! (`walk`, `crawl`, `run`): a planted foot slides back under the body exactly as fast as the body goes over it, so it
//! stays where it was put, and each footfall comes down hard.
//!
//! Every tell is big, slow and different from the others, so that it can be read from across the room a second before
//! the blow: the charge rears up roaring steam, drops its head and paws the floor; the smash raises both fists high over
//! its head; a swat draws its arm right back across its body; a stomp lifts a knee to its belly; a jump sinks into a
//! deep crouch with its arms swung back.
use super::body::{self, ANKLE};
use super::pose::{L, Pose, R, v};
use crate::kungfu::{Ease, Timing};
use bevy::prelude::Vec3;
use kitchen_sim::titan::{self as sim, Move};
use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

/// A key: a pose at a beat, reached as `ease` says.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub at: f32,
    pub pose: Pose,
    pub ease: Ease,
}
/// How a move's clock runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cycle {
    /// Once through its beats, timed by the simulation.
    Once,
    /// Round and round, this many seconds a turn.
    Loop(f32),
}
/// A part of the body that lands a blow or leaves a whoosh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limb {
    Hand(usize),
    Hands,
    Foot(usize),
    Body,
}
/// The kind of blow a move lands on beat 1, for the effects and the sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blow {
    None,
    Smash,
    Swat,
    Stomp,
    Kick,
    Land,
    Charge,
    Fall,
}
/// One animation.
#[derive(Clone, Debug)]
pub struct Anim {
    #[cfg_attr(not(test), allow(dead_code))]
    pub name: &'static str,
    pub keys: Vec<Key>,
    pub cycle: Cycle,
    /// The part that lands the blow and leaves the whoosh.
    pub strike: Option<Limb>,
    pub blow: Blow,
    /// Seconds it stands still on contact (0: none).
    pub hitstop: f32,
    /// The ground shakes as the move passes these beats, this hard (1 a smash).
    pub quakes: Vec<(f32, f32)>,
    /// Between these beats the body runs (the charge): its legs and arms are the run's.
    pub running: Option<(f32, f32)>,
}
impl Anim {
    pub fn new(name: &'static str, first: Pose) -> Anim {
        Anim { name, keys: vec![Key { at: 0., pose: first, ease: Ease::Smooth }], cycle: Cycle::Once, strike: None, blow: Blow::None, hitstop: 0., quakes: vec![], running: None }
    }
    pub fn key(mut self, at: f32, pose: Pose, ease: Ease) -> Anim {
        self.keys.push(Key { at, pose, ease });
        self
    }
    pub fn strike(mut self, limb: Limb, blow: Blow) -> Anim {
        self.strike = Some(limb);
        self.blow = blow;
        self
    }
    pub fn hitstop(mut self, seconds: f32) -> Anim {
        self.hitstop = seconds;
        self
    }
    pub fn quake(mut self, beat: f32, strength: f32) -> Anim {
        self.quakes.push((beat, strength));
        self
    }
    pub fn cycle(mut self, cycle: Cycle) -> Anim {
        self.cycle = cycle;
        self
    }
    pub fn running(mut self, from: f32, to: f32) -> Anim {
        self.running = Some((from, to));
        self
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
        // A Hermite curve through the keys (as Kung Fu's): its tangent at a key runs from the key before to the key
        // after, scaled to the time either side and kept small next to a hold, so the limbs swing in arcs.
        let (prev, next) = (i.checked_sub(1).map(|j| &keys[j]), keys.get(i + 2));
        let calm = |a: f32, b: f32| (2. * a.min(b) / a.max(b).max(1e-6)).min(1.);
        let here = k1.pose.gap(&k2.pose);
        let c1 = prev.map_or(0., |k0| TENSION * span / (k2.at - k0.at).max(1e-4) * calm(k0.pose.gap(&k1.pose), here));
        let c2 = next.map_or(0., |k3| TENSION * span / (k3.at - k1.at).max(1e-4) * calm(here, k2.pose.gap(&k3.pose)));
        let (t2, t3) = (e * e, e * e * e);
        let (h00, h01, h10, h11) = (2. * t3 - 3. * t2 + 1., -2. * t3 + 3. * t2, t3 - 2. * t2 + e, t3 - t2);
        let p0 = prev.map_or(&k1.pose, |k| &k.pose);
        let p3 = next.map_or(&k2.pose, |k| &k.pose);
        Pose::weighted(&[(p0, -h10 * c1), (&k1.pose, h00 - h11 * c2), (&k2.pose, h01 + h10 * c1), (p3, h11 * c2)])
    }
    /// The beat `t` seconds in.
    pub fn beat(&self, t: f32, timing: Timing) -> f32 {
        match self.cycle {
            Cycle::Once => timing.beat(t),
            Cycle::Loop(seconds) => (t / seconds.max(0.05)).rem_euclid(1.) * 3.,
        }
    }
}
const TENSION: f32 = 0.8;

/// A move's timing this time: the table's, its active window as long as the view says when it is one whose length
/// varies (a charge's run, a jump's time in the air), or the whole move as long as `len` says for a reaction.
pub fn timing(mv: Move, len: f32) -> Timing {
    let d = mv.data();
    let (startup, active, recovery) = (d.startup, d.active, d.recovery);
    let total = startup + active + recovery;
    let len = if len > 0. { len } else { total };
    // A move that is one long window (its fall, its victory) is played over all its beats: a third each.
    if startup <= 0. && recovery <= 0. {
        return Timing { startup: len * 0.34, active: len * 0.33, recovery: len * 0.33 };
    }
    if (len - total).abs() <= 1e-3 {
        return Timing { startup, active, recovery };
    }
    // A charge's run and a jump's time in the air are what vary; anything else (or too short a run) is stretched.
    let varied = len - startup - recovery;
    if matches!(mv, Move::Charge | Move::Jump) && varied >= 0.05 {
        return Timing { startup, active: varied, recovery };
    }
    let k = len / total.max(1e-3);
    Timing { startup: startup * k, active: active * k, recovery: recovery * k }
}

// ----- Stances

/// Where the simulation's figure has the titan's neck (its head's pivot) in a stance (0 standing, 1 crouched, 2
/// kneeling), looking level, in the root's frame at the meshes' size (x right, y up, z back): titan.rs `Shape::of`'s
/// head, less the neck the head stands on (as long as its head's radius).
pub fn sim_neck(stance: usize) -> Vec3 {
    let p = kitchen_sim::feast::Player { p: [0.; 3], yaw: 0., pitch: 0., crouch: stance == 1, alive: true, ..Default::default() };
    let mv = if stance == 2 { Move::Stagger } else { Move::Idle };
    let shape = sim::Shape::of(&p, mv);
    let nod = super::play::sim_nod(stance, 0.);
    (shape.head - bevy::prelude::Quat::from_rotation_x(nod) * Vec3::Y * sim::HEAD_RADIUS) / super::rig::scale()
}
/// `pose` with its hips moved so that its neck is where the simulation's figure has it in `stance`: the head drawn is
/// the head the simulation's rounds find, however the figures are tuned.
pub fn fit(pose: Pose, stance: usize) -> Pose {
    let neck = super::pose::solve(&pose, None, 0.).head.translation;
    let d = sim_neck(stance) - neck;
    pose.hips(pose.hips.x + d.x, pose.hips.y + d.y, pose.hips.z - d.z)
}
fn stances() -> &'static [Pose; 3] {
    static STANCES: OnceLock<[Pose; 3]> = OnceLock::new();
    STANCES.get_or_init(|| [make_stance(), make_crouched(), make_kneeling()])
}
/// Standing ready: hunched, heavy, the hips back, the arms hanging to the knees, the hands half closed.
pub fn stance() -> Pose {
    stances()[0]
}
fn make_stance() -> Pose {
    fit(Pose::REST.hips(0., 0., -0.11).lean(0.18), 0)
        .hand_at(L, v(0.46, 0.5, 0.1), 0.25)
        .hand_at(R, v(0.46, 0.5, 0.1), 0.25)
        .elbow(L, v(0.5, 0., -1.))
        .elbow(R, v(0.5, 0., -1.))
        .foot(L, v(0.17, ANKLE, -0.06))
        .foot(R, v(0.17, ANKLE, -0.06))
}
/// Crouched (CROUCH_TALL): squatting deep, its hips right back and its knees up and out, leaning over them, its
/// knuckles on the floor in front of it.
pub fn crouched() -> Pose {
    stances()[1]
}
fn make_crouched() -> Pose {
    fit(Pose::REST.hips(0., -0.40, -0.2).lean(0.45).squash(0.15), 1)
        .foot(L, v(0.25, ANKLE, 0.04))
        .foot(R, v(0.25, ANKLE, 0.04))
        .knee(L, v(0.6, 0.3, 1.))
        .knee(R, v(0.6, 0.3, 1.))
        .hand_at(L, v(0.42, 0.075, 0.42), 1.)
        .hand_at(R, v(0.42, 0.075, 0.42), 1.)
        .elbow(L, v(0.7, 0.2, -1.))
        .elbow(R, v(0.7, 0.2, -1.))
        .palm(L, v(0., -1., 0.))
        .palm(R, v(0., -1., 0.))
}
/// On its knees (a stagger, a stumble: KNEEL_TALL): sat back on its heels, its knees on the floor, bent right over on
/// its hands with its head hanging (bowed as the simulation bows it), heaving.
pub fn kneeling() -> Pose {
    stances()[2]
}
fn make_kneeling() -> Pose {
    // (Fitted last: its heave raises its neck.)
    let pose = Pose::REST
        .hips(0., -0.49, -0.23)
        .lean(0.9)
        .lift(L, v(0.18, 0.1, -0.36), v(0.3, 0.1, 1.), 1.3)
        .lift(R, v(0.18, 0.1, -0.36), v(0.3, 0.1, 1.), 1.3)
        .hand_at(L, v(0.38, 0.075, 0.48), 0.1)
        .hand_at(R, v(0.38, 0.075, 0.48), 0.1)
        .elbow(L, v(0.6, 0., -1.))
        .elbow(R, v(0.6, 0., -1.))
        .palm(L, v(0., -1., 0.))
        .palm(R, v(0., -1., 0.))
        .heave(0.8)
        .vent(0.8);
    fit(pose, 2)
}

// ----- The walking gaits, from the stride's phase

/// A stride's shape: how far each foot reaches either side of its rest (metres), how high a swinging foot lifts, how
/// far the hips dip at a footfall and sway over the planted foot, how far the body tilts and twists with them, and the
/// share of the cycle a foot is on the ground.
struct Gait {
    reach: f32,
    lift: f32,
    dip: f32,
    sway: f32,
    tilt: f32,
    twist: f32,
    swing: f32,
    stance: f32,
}
/// A foot's place in its cycle (`phase` 0 to 1 from its footfall): how far forward of its rest (metres; it slides back
/// evenly while planted), how high, and whether it is on the ground.
fn foot_cycle(phase: f32, g: &Gait) -> (f32, f32, f32) {
    let s = g.stance;
    if phase < s {
        // On the ground: from reaching forward to pushing off behind, evenly (the body goes over it).
        let t = phase / s;
        (g.reach * (1. - 2. * t), 0., 1.)
    } else {
        // Swinging through: lifted high (a giant lifts its feet to step over things), forward, and slammed down.
        let t = (phase - s) / (1. - s);
        let along = -g.reach + 2. * g.reach * (0.5 - 0.5 * (t * PI).cos());
        let high = g.lift * (t * PI).sin().powf(0.8) * (1. - 0.25 * t);
        (along, high, 0.)
    }
}
/// A gait on `base`, `phase` radians into its cycle (a whole cycle is two strides, one each foot; the left foot falls
/// at 0), `amount` of it (0 standing still, 1 at its pace).
fn gait(base: &Pose, phase: f32, amount: f32, g: &Gait, arms: bool) -> Pose {
    let mut p = *base;
    let a = amount.clamp(0., 1.);
    let cycle = (phase / TAU).rem_euclid(1.);
    let mut low = 0f32;
    for (i, offset) in [(L, 0.), (R, 0.5)] {
        let ph = (cycle - offset).rem_euclid(1.);
        let (along, high, planted) = foot_cycle(ph, g);
        let leg = &mut p.legs[i];
        leg.foot.z += along * a;
        leg.foot.y += high * a;
        leg.plant = 1. - (1. - planted) * a;
        leg.toe = (-0.5 * (ph - g.stance).max(0.) / (1. - g.stance) + 0.2) * (1. - planted) * a;
        // The knee comes up as the foot does.
        leg.knee.y += high * 2.5 * a;
        // A footfall: the hips drop on impact and recover.
        let since = ph;
        low = low.max((1. - since / 0.18).max(0.) * (since < 0.18) as u8 as f32);
    }
    // The weight shifts over the foot that is down; the body tilts with it and twists with the stride.
    let over = (cycle * TAU).cos();
    p.hips.x += -g.sway * over * a;
    p.hips.y += -g.dip * (low * low) * a + 0.5 * g.dip * ((cycle * 2. * TAU).cos() * 0.5 + 0.5) * a * 0.3;
    p.tilt += g.tilt * over * a;
    p.twist += g.twist * (cycle * TAU).sin() * a;
    if arms {
        for (i, s) in [(L, 1f32), (R, -1.)] {
            // The arms swing against the legs, heavy and short.
            let swing = g.swing * s * (cycle * TAU).sin() * a;
            p.arms[i].hand.z += swing;
            p.arms[i].hand.y += swing.abs() * 0.25;
        }
    }
    p
}
const WALK: Gait = Gait { reach: 0.19, lift: 0.17, dip: 0.05, sway: 0.065, tilt: 0.075, twist: 0.08, swing: 0.13, stance: 0.62 };
const CRAWL: Gait = Gait { reach: 0.14, lift: 0.1, dip: 0.025, sway: 0.05, tilt: 0.05, twist: 0.1, swing: 0., stance: 0.6 };
const RUN: Gait = Gait { reach: 0.32, lift: 0.22, dip: 0.08, sway: 0.04, tilt: 0.05, twist: 0.16, swing: 0., stance: 0.45 };
/// Metres the body goes in one whole cycle of each gait (two strides): the stride's phase runs at the speed over this.
pub fn cycle_length(crouched: bool, running: bool) -> f32 {
    let g = if running { &RUN } else if crouched { &CRAWL } else { &WALK };
    // A planted foot slides back 2 * reach over `stance` of the cycle: the body goes that far in that time.
    2. * g.reach / g.stance
}
/// The ponderous walk.
pub fn walk(phase: f32, amount: f32) -> Pose {
    gait(&stance(), phase, amount, &WALK, true)
}
/// The crouched knuckle-walk: the hands step with the feet across from them.
pub fn crawl(phase: f32, amount: f32) -> Pose {
    let mut p = gait(&crouched(), phase, amount, &CRAWL, false);
    let cycle = (phase / TAU).rem_euclid(1.);
    for (i, offset) in [(L, 0.5), (R, 0.)] {
        let ph = (cycle - offset).rem_euclid(1.);
        let (along, high, _) = foot_cycle(ph, &CRAWL);
        // (The hands are written in the chest's frame: forward and up there are along and off the floor near enough.)
        p.arms[i].hand.z += along * amount * 0.9;
        p.arms[i].hand.y += high * amount * 0.8;
    }
    p
}
/// The charge's run: head down, pumping its fists.
pub fn run(phase: f32) -> Pose {
    let base = charging();
    let mut p = gait(&base, phase, 1., &RUN, false);
    let cycle = (phase / TAU).rem_euclid(1.);
    for (i, s) in [(L, 1f32), (R, -1.)] {
        let pump = s * (cycle * TAU).sin();
        p.arms[i].hand += v(0., 0.12 * pump.max(0.) + 0.02, 0.24 * pump);
    }
    p
}
/// Bent into the run: the head down like a bull's, the fists up.
fn charging() -> Pose {
    stance()
        .hips(0., -0.08, 0.05)
        .lean(0.52)
        .nod(0.32)
        .arm(L, v(0.12, -0.32, 0.12), 1.)
        .arm(R, v(0.12, -0.32, 0.12), 1.)
        .elbow(L, v(0.5, -0.2, -1.))
        .elbow(R, v(0.5, -0.2, -1.))
        .foot(L, v(0.19, ANKLE, 0.0))
        .foot(R, v(0.19, ANKLE, 0.0))
}

// ----- The moves

/// The idle: breathing slow and heavy, the shoulders rising, a little sway.
fn idle() -> Anim {
    let s = stance();
    let up = s.hips(s.hips.x, s.hips.y - 0.02, s.hips.z).heave(1.).nod(-0.04).tilt(0.012).arm(L, s.arms[L].hand + v(0., 0.015, 0.), 0.3).arm(R, s.arms[R].hand + v(0., 0.015, 0.), 0.3);
    let down = s.tilt(-0.012).heave(0.);
    Anim::new("idle", s.heave(0.2)).key(1.4, up, Ease::Smooth).key(3., down.heave(0.2), Ease::Smooth).cycle(Cycle::Loop(3.6))
}
/// The charge: rears up roaring steam, drops its head and paws the floor (the tell), runs, and skids to a stop.
fn charge() -> Anim {
    let s = stance();
    let rear = s
        .hips(0., -0.02, -0.04)
        .lean(-0.12)
        .nod(-0.38)
        .heave(1.3)
        .vent(1.)
        .arm(L, v(0.42, -0.3, -0.26), 1.)
        .arm(R, v(0.42, -0.3, -0.26), 1.)
        .elbow(L, v(0.3, -0.5, -1.))
        .elbow(R, v(0.3, -0.5, -1.));
    let drop = charging().vent(0.6).heave(0.8).lift(R, v(0.2, ANKLE + 0.06, -0.3), v(0.2, -0.3, 1.), 0.6).arm(L, v(0.15, -0.25, -0.2), 1.).arm(R, v(0.15, -0.25, -0.2), 1.);
    let paw = drop.foot(R, v(0.2, ANKLE, -0.12));
    let go = charging();
    let skid = s.hips(0., -0.12, -0.05).lean(-0.12).nod(-0.15).foot(L, v(0.22, ANKLE, 0.26)).foot(R, v(0.2, ANKLE, 0.12)).arm(L, v(0.35, -0.25, 0.35), 0.).arm(R, v(0.35, -0.25, 0.35), 0.).vent(0.5);
    Anim::new("charge", s)
        .key(0.3, rear, Ease::Out)
        .key(0.62, drop, Ease::Smooth)
        .key(0.8, paw, Ease::Snap)
        .key(1.0, go, Ease::Smooth)
        .key(2.0, go, Ease::Linear)
        .key(2.3, skid, Ease::Out)
        .key(3., s, Ease::Smooth)
        .running(1.0, 2.0)
        .strike(Limb::Body, Blow::Charge)
}
/// The ground smash: both fists raised high over its head, then down on the floor `ahead` of it with everything it
/// has; it holds there a moment, then heaves itself back up.
fn smash(ahead: f32) -> Anim {
    let s = stance();
    let rise = s.lean(0.0).hips(0., 0.0, 0.).hand_at(L, v(0.32, 1.28, 0.42), 0.6).hand_at(R, v(0.32, 1.28, 0.42), 0.6).heave(0.6);
    let high = s
        .lean(-0.22)
        .hips(0., 0.025, -0.04)
        .nod(-0.12)
        .heave(1.2)
        .vent(0.4)
        .hand_at(L, v(0.1, 2.02, 0.02), 1.)
        .hand_at(R, v(0.1, 2.02, 0.02), 1.)
        .elbow(L, v(1., 0.2, 0.))
        .elbow(R, v(1., 0.2, 0.))
        .foot(L, v(0.22, ANKLE, 0.06))
        .foot(R, v(0.22, ANKLE, -0.04));
    let down = s
        .lean(1.02)
        .hips(0., -0.26, 0.06)
        .nod(-0.25)
        .heave(0.)
        .vent(1.)
        .foot(L, v(0.25, ANKLE, 0.14))
        .foot(R, v(0.25, ANKLE, -0.06))
        .knee(L, v(0.5, 0., 1.))
        .knee(R, v(0.5, 0., 1.));
    let down = down.hand_at(L, v(0.1, 0.07, ahead.max(0.45)), 1.).hand_at(R, v(0.1, 0.07, ahead.max(0.45)), 1.).elbow(L, v(1., 0.3, -0.3)).elbow(R, v(1., 0.3, -0.3)).palm(L, v(0., -1., 0.)).palm(R, v(0., -1., 0.));
    let sunk = down.hips(0., -0.29, 0.06).lean(1.06);
    let up = s.lean(0.45).hips(0., -0.12, 0.03).hand_at(L, v(0.32, 0.35, 0.4), 0.6).hand_at(R, v(0.32, 0.35, 0.4), 0.6).heave(0.6);
    Anim::new("smash", s)
        .key(0.35, rise, Ease::Smooth)
        .key(0.82, high, Ease::Out)
        .key(1.0, down, Ease::Snap)
        .key(1.6, sunk, Ease::Out)
        .key(2.2, sunk, Ease::Smooth)
        .key(2.65, up, Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Hands, Blow::Smash)
        .hitstop(0.12)
        .quake(1.0, 1.)
}
/// A swat with the right arm from its right across to its left at `height` (a hand's height over the floor), `reach`
/// from its middle, on `base` (standing or crouched). The tell: the arm drawn right back across the body.
fn swat(name: &'static str, base: Pose, height: f32, reach: f32, low: bool) -> Anim {
    let s = base;
    let r = reach.min(if low { 1.0 } else { 0.98 });
    let lean = if low { base.lean + 0.12 } else { base.lean + 0.08 };
    let hips = if low { s.hips } else { s.hips + v(0.0, -0.05, 0.) };
    let at = |angle: f32, k: f32| v(angle.sin() * r * k, height, angle.cos() * r * k);
    let cocked = s
        .lean(lean - 0.06)
        .hips(hips.x + 0.05, hips.y, hips.z)
        .twist(-0.75)
        .tilt(-0.06)
        .heave(0.8)
        .hand_at(R, v(0.55, height + 0.25, -0.45), 0.)
        .elbow(R, v(0.4, 0.8, -0.6))
        .palm(R, v(-0.5, 0., 1.))
        .hand_at(L, v(0.45, height.max(0.3) + 0.15, 0.45), 0.4);
    let into = s.lean(lean).hips(hips.x, hips.y, hips.z).twist(-0.45).tilt(-0.04).hand_at(R, at(1.35, 0.95), 0.).palm(R, v(-1., 0., 0.3)).hand_at(L, v(0.45, height.max(0.3) + 0.1, 0.35), 0.4);
    let through = s.lean(lean + 0.04).hips(hips.x - 0.03, hips.y, hips.z + 0.04).twist(0.15).hand_at(R, at(0.0, 1.0), 0.).palm(R, v(-1., 0., 0.1)).hand_at(L, v(0.45, height.max(0.3), 0.1), 0.4);
    let out = s.lean(lean).hips(hips.x - 0.06, hips.y, hips.z).twist(0.8).tilt(0.05).hand_at(R, at(-1.45, 0.95), 0.).palm(R, v(-1., 0., -0.3)).hand_at(L, v(0.45, height.max(0.3), -0.2), 0.4);
    let follow = s.lean(lean - 0.04).hips(hips.x - 0.05, hips.y, hips.z).twist(0.9).tilt(0.04).hand_at(R, v(-0.6, height + 0.05, -0.1), 0.2).hand_at(L, v(0.45, height.max(0.3), -0.25), 0.4);
    Anim::new(name, s)
        .key(0.85, cocked, Ease::Out)
        .key(1.0, into, Ease::Snap)
        .key(1.5, through, Ease::Linear)
        .key(2.0, out, Ease::Linear)
        .key(2.4, follow, Ease::Out)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Hand(R), Blow::Swat)
}
/// The stomp: the right knee pulled up to its belly, then the foot slammed down `ahead`.
fn stomp(ahead: f32) -> Anim {
    let s = stance();
    let lift = s
        .hips(-0.07, -0.0, -0.02)
        .lean(0.02)
        .tilt(0.1)
        .heave(0.8)
        .lift(R, v(0.18, 0.47, 0.2), v(0.2, 0.6, 1.), -0.15)
        .hand_at(L, v(0.6, 0.85, 0.1), 0.6)
        .hand_at(R, v(0.62, 0.85, 0.05), 0.6);
    let slam = s.hips(-0.02, -0.08, 0.06).lean(0.26).tilt(0.0).foot(R, v(0.16, ANKLE, ahead.clamp(0.15, 0.4))).vent(0.4);
    Anim::new("stomp", s).key(0.85, lift, Ease::Out).key(1.0, slam, Ease::Snap).key(2.2, slam.hips(-0.02, -0.06, 0.06), Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Foot(R), Blow::Stomp).hitstop(0.06).quake(1.0, 0.55)
}
/// The crouched kick: a low drive of the right foot ahead along the floor.
fn kick(reach: f32) -> Anim {
    let s = crouched();
    let cock = s.hips(-0.04, -0.29, -0.12).lean(0.75).lift(R, v(0.17, 0.32, -0.1), v(0.3, 0.5, 1.), -0.3).vent(0.3);
    let out = s.hips(-0.03, -0.25, 0.12).lean(0.7).lift(R, v(0.12, 0.2, (reach - 0.25).clamp(0.4, 0.62)), v(0.2, 0.6, 1.), -0.9);
    Anim::new("kick", s).key(0.85, cock, Ease::Out).key(1.0, out, Ease::Snap).key(2.0, out, Ease::Smooth).key(2.5, cock.foot(R, v(0.27, ANKLE, 0.0)), Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Foot(R), Blow::Kick)
}
/// The jump: down into a deep crouch with the arms swung back (the tell), up with them swung high, tucked in the air,
/// and a crushing landing.
fn jump() -> Anim {
    let s = stance();
    let sink = s.hips(0., -0.24, -0.04).lean(0.5).nod(-0.3).arm(L, v(0.3, -0.45, -0.4), 1.).arm(R, v(0.3, -0.45, -0.4), 1.).heave(1.).foot(L, v(0.24, ANKLE, 0.03)).foot(R, v(0.24, ANKLE, 0.03)).knee(L, v(0.5, 0., 1.)).knee(R, v(0.5, 0., 1.));
    let push = s.hips(0., 0.02, 0.).lean(0.1).nod(-0.1).hand_at(L, v(0.42, 1.7, 0.3), 0.8).hand_at(R, v(0.42, 1.7, 0.3), 0.8).lift(L, v(0.2, ANKLE - 0.02, -0.05), v(0.2, 0., 1.), 0.9).lift(R, v(0.2, ANKLE - 0.02, -0.05), v(0.2, 0., 1.), 0.9);
    let tuck = s.hips(0., 0.0, 0.).lean(0.2).hand_at(L, v(0.55, 1.55, 0.2), 0.9).hand_at(R, v(0.55, 1.55, 0.2), 0.9).lift(L, v(0.22, 0.3, 0.12), v(0.3, 0.6, 1.), 0.2).lift(R, v(0.22, 0.3, 0.12), v(0.3, 0.6, 1.), 0.2);
    let land = s.hips(0., -0.27, 0.02).lean(0.62).nod(-0.2).vent(1.).foot(L, v(0.27, ANKLE, 0.05)).foot(R, v(0.27, ANKLE, 0.0)).knee(L, v(0.6, 0., 1.)).knee(R, v(0.6, 0., 1.)).hand_at(L, v(0.55, 0.3, 0.35), 1.).hand_at(R, v(0.55, 0.3, 0.35), 1.);
    Anim::new("jump", s)
        .key(0.8, sink, Ease::Out)
        .key(1.0, push, Ease::Snap)
        .key(1.45, tuck, Ease::Out)
        .key(1.85, tuck.lift(L, v(0.24, ANKLE + 0.06, 0.05), v(0.4, 0.2, 1.), 0.).lift(R, v(0.24, ANKLE + 0.06, 0.0), v(0.4, 0.2, 1.), 0.), Ease::Smooth)
        .key(2.0, land, Ease::Snap)
        .key(2.5, land.hips(0., -0.2, 0.02), Ease::Out)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Body, Blow::Land)
        .quake(2.0, 0.85)
}
/// Brought to its knees (`fast` a stumble into a wall): it buckles, falls forward onto its hands and knees with its head
/// hanging (exposing the back of its head), heaves for breath venting steam, and drags itself back up.
fn stagger(name: &'static str, fast: bool) -> Anim {
    let s = stance();
    let reel = if fast {
        s.hips(0., -0.05, -0.1).lean(-0.25).nod(-0.45).hand_at(L, v(0.35, 1.45, 0.25), 0.5).hand_at(R, v(0.35, 1.5, 0.2), 0.5).vent(1.)
    } else {
        s.hips(0.03, -0.12, 0.).lean(0.28).tilt(0.12).nod(0.25).hand_at(L, v(0.7, 0.9, 0.2), 0.).hand_at(R, v(0.6, 0.75, 0.25), 0.).lift(R, v(0.22, 0.2, 0.1), v(0.3, 0.2, 1.), 0.).vent(0.7)
    };
    let k = kneeling();
    let breathe = k.hips(k.hips.x, k.hips.y + 0.012, k.hips.z).heave(1.3).nod(0.08);
    let rise = s.hips(0., -0.2, 0.).lean(0.5).foot(L, v(0.22, ANKLE, 0.24)).lift(R, v(0.2, 0.09, -0.2), v(0.3, -0.2, 1.), 1.2).hand_at(L, v(0.45, 0.36, 0.4), 0.3).hand_at(R, v(0.4, 0.12, 0.35), 0.1);
    Anim::new(name, s)
        .key(0.35, reel, Ease::Out)
        .key(1.0, k, Ease::In)
        .key(1.3, breathe, Ease::Smooth)
        .key(1.65, k, Ease::Smooth)
        .key(2.0, breathe, Ease::Smooth)
        .key(2.45, rise, Ease::Smooth)
        .key(3., s, Ease::Smooth)
        .strike(Limb::Body, Blow::Fall)
        .quake(1.0, 0.6)
}
/// Its death: a long topple. It reels with its arms flung up and its head back, steam erupting from every split; its
/// knees go; it sags, sways, and falls forward on its face, which shakes the whole kitchen; and lies there venting.
fn down() -> Anim {
    let s = stance();
    let reel = s.hips(0., 0.0, -0.06).lean(-0.3).nod(-0.6).heave(1.4).vent(1.).hand_at(L, v(0.65, 1.6, 0.1), 0.).hand_at(R, v(0.55, 1.75, 0.2), 0.).foot(R, v(0.22, ANKLE, -0.15));
    let k = kneeling();
    let knees = k.lean(0.15).nod(-0.3).hand_at(L, v(0.5, 0.45, 0.1), 0.).hand_at(R, v(0.5, 0.4, 0.15), 0.).heave(1.).vent(1.);
    let sway = knees.lean(-0.05).tilt(0.12).nod(-0.2);
    // Falling: the whole body (knelt) turns forward over its knees until it lies on its face.
    let pivot = v(0., 0.05, 0.05);
    let tipping = knees.lean(0.25).tilt(0.05).spin(0.6, 0., 0.05, pivot).hand_at(L, v(0.7, 0.9, 0.35), 0.).hand_at(R, v(0.65, 0.7, 0.3), 0.);
    let flat = knees.lean(0.2).nod(0.1).spin(1.33, 0.0, 0.1, pivot).hand_at(L, v(0.85, 0.35, 0.55), 0.).hand_at(R, v(0.75, 0.45, 0.2), 0.).heave(0.).vent(1.);
    Anim::new("down", s)
        .key(0.3, reel, Ease::Out)
        .key(0.85, knees, Ease::In)
        .key(1.2, sway, Ease::Smooth)
        .key(1.5, tipping, Ease::In)
        .key(2.0, flat, Ease::Snap)
        .key(2.25, flat.spin(1.25, 0., 0.1, pivot), Ease::Out)
        .key(2.5, flat, Ease::Smooth)
        .key(3., flat.vent(0.7), Ease::Smooth)
        .strike(Limb::Body, Blow::Fall)
        .quake(1.0, 0.5)
        .quake(2.0, 1.6)
}
/// The titan's victory: it throws its arms wide and its head back, steam roaring out of its grin, and beats its chest.
fn roar() -> Anim {
    let s = stance();
    let wide = s.lean(-0.2).nod(-0.6).heave(1.4).vent(1.).hand_at(L, v(0.9, 1.6, 0.2), 0.).hand_at(R, v(0.9, 1.6, 0.2), 0.);
    let beat_l = s.lean(-0.05).nod(-0.3).heave(1.).vent(0.6).hand_at(L, v(0.15, 1.15, 0.35), 1.).hand_at(R, v(0.5, 1.2, 0.3), 1.);
    let beat_r = beat_l.hand_at(R, v(0.15, 1.15, 0.35), 1.).hand_at(L, v(0.5, 1.2, 0.3), 1.);
    Anim::new("roar", s).key(0.5, wide, Ease::Out).key(1.2, wide, Ease::Smooth).key(1.5, beat_l, Ease::Snap).key(1.8, beat_r, Ease::Snap).key(2.1, beat_l, Ease::Snap).key(2.4, beat_r, Ease::Snap).key(3., wide, Ease::Smooth).cycle(Cycle::Loop(4.))
}
/// The flinch when its weak spot is hit, laid over whatever it is doing: a shudder. The shoulders hunch up, the head
/// jerks a touch, the fists clench and steam bursts from its vents. Nothing goes far (the simulation does not follow it:
/// the body stays where rounds find it, within a few centimetres) and nothing comes near the weak spot.
pub fn flinched(p: &Pose, amount: f32) -> Pose {
    let mut to = p.nod(p.nod + 0.08).heave(p.heave + 0.5).twist(p.twist + 0.03).vent(1.);
    for arm in to.arms.iter_mut() {
        arm.fist = 1.;
    }
    p.lerp(&to, amount.clamp(0., 1.))
}

/// Every move's animation, by its number (`titan::Move` as u8).
pub struct Library {
    pub moves: Vec<Anim>,
}
pub fn library() -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let d = |m: Move| m.data();
        let swat_height = |m: Move| {
            let band = d(m).band;
            ((band[0] + band[1]) * 0.5) as f32 / sim::SCALE as f32 * 12. * (body::REFERENCE / sim::TALL as f32)
        };
        let reference = body::REFERENCE / sim::TALL as f32;
        let crouched = crouched();
        let moves = Move::ALL
            .iter()
            .map(|m| match m {
                Move::Idle => idle(),
                Move::Walk => Anim::new("walk", stance()),
                Move::Charge => charge(),
                Move::Smash => smash(d(*m).ahead as f32 * reference),
                Move::SwatHigh => swat("swat high", stance(), swat_height(*m).max(0.55), d(*m).reach as f32 * reference, false),
                Move::SwatLow => swat("swat low", crouched, swat_height(*m).min(0.25), d(*m).reach as f32 * reference, true),
                Move::Stomp => stomp(d(*m).ahead as f32 * reference),
                Move::Kick => kick(d(*m).reach as f32 * reference),
                Move::Jump => jump(),
                Move::Stagger => stagger("stagger", false),
                Move::Stumble => stagger("stumble", true),
                Move::Down => down(),
                Move::Roar => roar(),
            })
            .collect();
        Library { moves }
    })
}
impl Library {
    pub fn anim(&self, mv: Move) -> &Anim {
        &self.moves[(mv as usize).min(self.moves.len() - 1)]
    }
}

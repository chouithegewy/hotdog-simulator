// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Playing the titan's moves, frame by frame (as Kung Fu's `Animator` plays a hotdog's): which move it is in and how far
//! (the simulation's word), blended from whatever the body was doing, frozen on contact for a hit-stop, its feet stepping
//! as far as the body has gone (a planted foot stays where it was put), the head looking where the simulation says it
//! looks (so the weak spot is where the simulation has it), a flinch laid over everything when its weak spot is hit, and
//! what the frame did that the effects and the sounds want: footfalls, the blow landing, the ground shaking.
use super::anims::{self, Blow, Cycle, library};
use super::pose::{L, Pose, R, Skeleton, solve};
use crate::kungfu::Timing;
use bevy::prelude::*;
use kitchen_sim::titan::{self as sim, Kind, Move};

/// What the animation is told of the titan this frame.
#[derive(Clone, Copy, Debug)]
pub struct Drive {
    pub mv: Move,
    /// Seconds into the move, and the move's timing this time.
    pub t: f32,
    pub timing: Timing,
    pub crouch: bool,
    pub airborne: bool,
    /// How far the body went since the last frame (metres, flat, in its own frame: right, forward).
    pub moved: Vec2,
    /// The head's nod as the simulation has it (radians about x; less than 0 bowed: `sim_nod`).
    pub nod: f32,
    /// Where the eyes stare (in the root's frame), if at anything.
    pub stare: Option<Vec3>,
    /// The stride as the round has it (kitchen_sim::titan `View::stride`), if it is told: the walk is drawn from it.
    pub stride: Option<Stride>,
}
/// The round's stride: its phase (radians) and its drive (0 to 1 of its pace and a little more), and whether it is the
/// round's own this frame (offline) or a view's that arrived earlier (online, a replay): carried on then by the same law
/// as the round's, and any difference when a new one arrives taken up over STRIDE_PULL seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stride {
    pub phase: f32,
    pub drive: f32,
    pub exact: bool,
}
/// How quickly (a second) a new view's stride is taken up online.
const STRIDE_PULL: f32 = 12.;
/// The head's nod the simulation gives the titan's figure in a stance (titan.rs `Shape::of`).
pub fn sim_nod(stance: usize, pitch: f32) -> f32 {
    match stance {
        2 => -sim::KNEEL_BOW,
        1 => kitchen_sim::head::nod(1, pitch) + sim::CROUCH_TUCK,
        _ => kitchen_sim::head::nod(0, pitch),
    }
}
/// The stance (titan.rs `stance_of`) of a move, crouched or not.
pub fn stance_of(mv: Move, crouch: bool) -> usize {
    if matches!(mv.data().kind, Kind::Reel | Kind::Down) {
        2
    } else if crouch {
        1
    } else {
        0
    }
}

/// A footfall this frame: which foot, where (in the root's frame), how hard (1 a full walking step).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footfall {
    pub side: usize,
    pub at: Vec3,
    pub strength: f32,
}
/// What one frame produced.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    #[cfg_attr(not(test), allow(dead_code))]
    pub pose: Pose,
    pub skeleton: Skeleton,
    /// How hard the ground shook now (0: not at all).
    pub quake: f32,
    pub footfalls: [Option<Footfall>; 2],
    /// The move's beat (0 to 3; for the effects that follow a move's phases).
    pub beat: f32,
}

/// The titan's animation, from frame to frame.
#[derive(Clone, Debug, Default)]
pub struct Animator {
    mv: Option<Move>,
    t: f32,
    last: Option<Pose>,
    from: Option<(Pose, f32, f32)>,
    clock: f32,
    felt: bool,
    stop: Option<(f32, f32)>,
    /// Smoothed: walking (0 to 1), crouched, off the ground; the head's nod.
    drive: f32,
    crouch: f32,
    air: f32,
    nod: Option<f32>,
    /// The stride's phase (radians: a whole cycle is two steps).
    phase: f32,
    /// Online: the last stride told, and what is still to be taken up of the difference from it (phase, drive).
    told: Option<Stride>,
    pull: (f32, f32),
    beat: f32,
    /// A flinch began at this clock.
    flinch: Option<f32>,
    /// Each foot planted last frame.
    planted: [bool; 2],
}
/// Seconds a change of move blends over (at most half its startup), and into or out of the stance.
const BLEND: f32 = 0.12;
const SETTLE: f32 = 0.3;
/// How long a flinch lasts.
pub const FLINCH: f32 = 0.7;

impl Animator {
    /// The weak spot was hit: the flinch begins.
    pub fn flinch(&mut self) {
        self.flinch = Some(self.clock);
    }
    /// One frame, `dt` seconds after the last.
    pub fn update(&mut self, d: &Drive, dt: f32) -> Frame {
        let dt = dt.clamp(0., 0.1);
        self.clock += dt;
        let lib = library();
        let ease = |rate: f32| 1. - (-dt * rate).exp();
        let stance_move = matches!(d.mv, Move::Idle | Move::Walk);
        let anim = (!matches!(d.mv, Move::Walk)).then(|| lib.anim(d.mv));
        // Walking: how fast, as a share of its pace (smoothed); the stride's phase runs with the ground covered.
        // (The round's own law: titan.rs `titan_tread`.)
        let speed = if dt > 1e-4 { d.moved.length() / dt } else { 0. };
        self.crouch += (d.crouch as u8 as f32 - self.crouch) * ease(sim::CROUCH_EASE);
        let pace = if self.crouch > 0.5 { sim::CROUCH_WALK } else { sim::WALK };
        let walking = stance_move && speed > sim::WALK_MOVING;
        self.drive += ((walking as u8 as f32) * (speed / pace).clamp(sim::DRIVE_MIN, sim::DRIVE_MAX) - self.drive) * ease(sim::DRIVE_EASE);
        self.air += (d.airborne as u8 as f32 - self.air) * ease(sim::AIR_EASE);
        let nod = self.nod.map_or(d.nod, |n| kitchen_sim_ease(n, d.nod, dt));
        self.nod = Some(nod);
        let running = d.mv == Move::Charge && anim.and_then(|a| a.running).is_some_and(|(a, b)| (a..b).contains(&d.timing.beat(d.t)));
        // The gait's cycle: the ground covered over the length of a cycle; backwards when backing up.
        let ahead = d.moved.y;
        let cycle = sim::gait_cycle(self.crouch > 0.5, running);
        let way = if ahead < -0.002 * dt.max(1e-3) && !running { -1. } else { 1. };
        let tau = std::f32::consts::TAU;
        self.phase = (self.phase + way * d.moved.length() / cycle.max(0.05) * tau).rem_euclid(tau);
        // The round's stride: its own this frame (offline), or a view's carried on (online), its difference taken up.
        match d.stride {
            Some(s) if s.exact => (self.phase, self.drive, self.pull) = (s.phase, s.drive, (0., 0.)),
            Some(s) => {
                if self.told != Some(s) {
                    let off = (s.phase - self.phase + tau * 0.5).rem_euclid(tau) - tau * 0.5;
                    self.pull = (off, s.drive - self.drive);
                }
                let k = ease(STRIDE_PULL);
                self.phase = (self.phase + self.pull.0 * k).rem_euclid(tau);
                self.drive += self.pull.1 * k;
                self.pull = (self.pull.0 * (1. - k), self.pull.1 * (1. - k));
            }
            None => {}
        }
        self.told = d.stride;
        // A new move, or the same again from its start: blend from what was shown.
        let fresh = Some(d.mv) != self.mv || (d.t + 1e-3 < self.t && !stance_move);
        if fresh || self.last.is_none() {
            let from = self.last.unwrap_or_else(anims::stance);
            let length = if stance_move { SETTLE } else { BLEND.min(d.timing.startup * 0.5).max(0.05) };
            // (Felled, the body goes from wherever it is, slowly.)
            let length = if d.mv == Move::Down { 0.25 } else { length };
            self.from = Some((from, self.clock, length));
            (self.felt, self.stop, self.beat) = (false, None, -1.);
        }
        self.mv = Some(d.mv);
        self.t = d.t;
        let mut quake = 0f32;
        let mut beat_now = 0.;
        let pose = match anim {
            Some(a) if !stance_move => {
                let t = shown(d.t, self.stop, d.timing.total());
                let beat = a.beat(t, d.timing);
                // The blow lands on beat 1 (a jump's on beat 2: its landing): a hit-stop, once.
                let contact = if a.blow == Blow::Land { 2. } else { 1. };
                if !self.felt && a.cycle == Cycle::Once && beat >= contact && a.strike.is_some() {
                    self.felt = true;
                    if a.hitstop > 0. && d.t - d.timing.time(contact) < 0.1 {
                        self.stop = Some((d.t.max(d.timing.time(contact)), a.hitstop));
                    }
                }
                for (at, strength) in &a.quakes {
                    if self.beat < *at && beat >= *at {
                        quake = quake.max(*strength);
                    }
                }
                self.beat = beat;
                beat_now = beat;
                let mut pose = a.pose_at(beat);
                if let Some((from, to)) = a.running.filter(|_| running) {
                    // The run, eased in and out of the keys either side.
                    let into = ((beat - from) / 0.12).clamp(0., 1.) * ((to - beat) / 0.12).clamp(0., 1.);
                    pose = pose.lerp(&anims::run(self.phase), into);
                }
                if a.cycle == Cycle::Once && d.mv == Move::Jump {
                    // In the air the feet are not on the ground (the keys plan for it); landed early, they are.
                    if self.air < 0.5 && beat > 1.2 && beat < 2. {
                        pose = pose.lerp(&a.pose_at(2.), 0.5);
                    }
                }
                pose
            }
            _ => {
                let idle = lib.anim(Move::Idle).sample_loop(self.clock);
                let standing = idle.lerp(&anims::walk(self.phase, 1.), self.drive.min(1.));
                let mut pose = if self.crouch > 0.01 { standing.lerp(&anims::crawl(self.phase, self.drive.min(1.)), self.crouch) } else { standing };
                if self.air > 0.01 {
                    let tuck = lib.anim(Move::Jump).pose_at(1.5);
                    pose = pose.lerp(&tuck, self.air);
                }
                pose
            }
        };
        // Blend from what was shown.
        let mut pose = match self.from {
            Some((from, start, length)) if self.clock - start < length => from.lerp(&pose, smooth((self.clock - start) / length)),
            _ => pose,
        };
        // The flinch, over everything.
        if let Some(start) = self.flinch {
            let since = self.clock - start;
            if since > FLINCH {
                self.flinch = None;
            } else {
                let w = smooth(since / 0.07) * (1. - smooth((since - 0.2) / (FLINCH - 0.2)));
                pose = anims::flinched(&pose, w);
            }
        }
        self.last = Some(pose);
        let skeleton = solve(&pose, d.stare, nod);
        // Footfalls: a foot that has just come down (planted after not being).
        let mut footfalls = [None; 2];
        for i in [L, R] {
            let planted = pose.legs[i].plant > 0.8 && skeleton.sole(i).y < 0.03;
            if planted && !self.planted[i] && self.clock > 0.2 {
                let strength = if running { 1.2 } else if self.crouch > 0.5 { 0.45 } else { (0.5 + 0.5 * self.drive).min(1.) };
                footfalls[i] = Some(Footfall { side: i, at: skeleton.sole(i), strength });
            }
            self.planted[i] = planted;
        }
        Frame { pose, skeleton, quake, footfalls, beat: beat_now }
    }
}
/// The drawn nod one frame on (bend.rs's ease: quick, and never far behind).
fn kitchen_sim_ease(drawn: f32, target: f32, dt: f32) -> f32 {
    let eased = drawn + (target - drawn) * (1. - (-dt.max(0.) * 9.).exp());
    eased.clamp(target - 0.3, target + 0.3)
}
fn rig_scale() -> f32 {
    super::rig::scale()
}
/// The second of a move to show `t` seconds in, with a hit-stop (Kung Fu's `shown`).
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
pub(crate) fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

impl anims::Anim {
    /// A looping animation's pose at the clock `t` (seconds).
    pub fn sample_loop(&self, t: f32) -> Pose {
        self.pose_at(self.beat(t, Timing::default()))
    }
}
#[allow(dead_code)]
const _SIDES: [usize; 2] = [L, R];

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The breakfast scene's clock and paths: when each beat begins, where Maya, her husband, the tray and the hotdog's eye
//! are at every moment, and the steps their feet take. One timeline, in seconds of "clip time" (`BreakfastShift` maps
//! each stage of the scene onto it): the door opens over 0 to 5.6 s, the couple greet until 16.6 s, Maya waits (a loop
//! of 4.5 s), reaches into the fridge, carries the tray to the counter, sets it down, and from 42.3 s cuts the seal.
//! Everything here is a function of that time alone, so a frame can be drawn at any moment, in any order.
use bevy::prelude::*;
use std::f32::consts::PI;

/// Steps are planned on a grid of this many frames a second (and drawn between them).
pub const FPS: f32 = 30.;
pub const GREETING_START: f32 = 5.6;
pub const GREETING_DURATION: f32 = 11.;
pub const IDLE_START: f32 = 16.6;
pub const IDLE_DURATION: f32 = 4.5;
pub const REACH_START: f32 = 21.1;
pub const REACH_DURATION: f32 = 4.;
pub const CARRY_START: f32 = 25.1;
pub const CARRY_DURATION: f32 = 8.7;
pub const SET_START: f32 = 33.8;
pub const SET_DURATION: f32 = 4.;
pub const COMPLETE_START: f32 = 37.8;
pub const KNIFE_START: f32 = 42.3;
/// The knife and the seal (`arena::Phase::Knife`, 4.5 s, then `Peeling`, 7.5 s).
pub const END: f32 = KNIFE_START + 12.;
/// The tray on its shelf in the fridge, and on the cutting board on the back counter.
pub const START: Vec3 = Vec3::new(0., 0.9525, -0.36);
pub const COUNTER: Vec3 = Vec3::new(0.85, 0.9385, 3.25);
/// Where Maya waits, facing the fridge, and where her husband stands beside her.
pub const MAYA_HOME: Vec3 = Vec3::new(0., 0., 1.02);
pub const HUSBAND_HOME: Vec3 = Vec3::new(0.65, 0., 1.28);
/// The knife on the counter before Maya picks it up (`arena`'s chef knife).
pub const KNIFE_REST: Vec3 = Vec3::new(0.52, kitchen_sim::feast::FLOOR + 0.022, 3.04);

pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// `v` turned by `yaw` about the vertical (a person's yaw 0 faces the fridge, along -z).
pub fn rotate(v: Vec3, yaw: f32) -> Vec3 {
    Quat::from_rotation_y(yaw) * v
}

/// The beats of the timeline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Beat {
    /// Before the greeting (the door opening) and while Maya waits.
    Idle,
    Greet,
    Reach,
    Carry,
    Set,
    Done,
    Knife,
}

/// The beat at clip time `t`, and how long it has been going (the time itself for `Idle`, as it always was).
pub fn beat(t: f32) -> (Beat, f32) {
    if t >= KNIFE_START {
        (Beat::Knife, t - KNIFE_START)
    } else if (GREETING_START..IDLE_START).contains(&t) {
        (Beat::Greet, t - GREETING_START)
    } else if t < REACH_START {
        (Beat::Idle, t)
    } else if t < CARRY_START {
        (Beat::Reach, t - REACH_START)
    } else if t < SET_START {
        (Beat::Carry, t - CARRY_START)
    } else if t < COMPLETE_START {
        (Beat::Set, t - SET_START)
    } else {
        (Beat::Done, t - COMPLETE_START)
    }
}

/// Where the tray and Maya are: the tray's middle and its turn, Maya's feet (her root, on the floor) and her facing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Motion {
    pub package: Vec3,
    pub root: Vec3,
    pub yaw: f32,
    pub package_yaw: f32,
}

pub fn motion(t: f32) -> Motion {
    let (kind, elapsed) = beat(t);
    let (mut package, mut root, mut yaw) = (START, MAYA_HOME, 0.);
    match kind {
        Beat::Greet => {
            let turn = smooth(elapsed / 1.4) * (1. - smooth((elapsed - 7.5) / 2.3));
            let embrace = smooth((elapsed - 3.2) / 2.) * (1. - smooth((elapsed - 7.3) / 2.1));
            yaw = -2. * turn;
            root = root.lerp(Vec3::new(0.18, 0., 1.07), embrace);
        }
        Beat::Reach => root = root.lerp(Vec3::new(0., 0., 0.12), smooth(elapsed / 2.2)),
        Beat::Carry => {
            if elapsed < 2.2 {
                package = START.lerp(Vec3::new(0., 0.9525, 0.25), smooth(elapsed / 2.2));
                root = Vec3::new(package.x, 0., package.z + 0.48);
            } else if elapsed < 3.2 {
                package = Vec3::new(0., 0.9525, 0.25).lerp(Vec3::new(0., 1.08, 0.55), smooth((elapsed - 2.2) / 1.));
                root = Vec3::new(package.x, 0., package.z + 0.48);
            } else {
                let heading = PI + 0.85f32.atan2(1.70);
                if elapsed < 5.2 {
                    root = Vec3::new(0., 0., 1.03);
                    yaw = heading * smooth((elapsed - 3.2) / 2.);
                } else {
                    let walk = elapsed - 5.2;
                    root = Vec3::new(0., 0., 1.03).lerp(Vec3::new(0.85, 0., 2.73), smooth(walk / 3.5));
                    yaw = heading + (PI - heading) * smooth((walk - 2.7) / 0.8);
                }
                package = root + rotate(Vec3::new(0., 1.08, -0.48), yaw);
            }
        }
        Beat::Set => {
            yaw = PI;
            package = Vec3::new(0.85, 1.08, 3.21).lerp(COUNTER, smooth(elapsed / 2.4));
            root = Vec3::new(0.85, 0., 2.73).lerp(Vec3::new(0.85, 0., 2.40), smooth((elapsed - 2.7) / 1.3));
        }
        Beat::Done | Beat::Knife => (package, root, yaw) = (COUNTER, Vec3::new(0.85, 0., 2.40), PI),
        Beat::Idle => {}
    }
    if kind == Beat::Knife {
        root = Vec3::new(0.85, 0., 2.40).lerp(Vec3::new(0.85, 0., 2.72), smooth(elapsed / 1.4));
    }
    let package_yaw = if matches!(kind, Beat::Carry | Beat::Set | Beat::Done | Beat::Knife) { yaw } else { 0. };
    Motion { package, root, yaw, package_yaw }
}

/// Her husband's root and facing: he stands by, and turns to her for the greeting.
pub fn husband(t: f32) -> (Vec3, f32) {
    let (kind, elapsed) = beat(t);
    let turn = if kind == Beat::Greet { smooth(elapsed / 1.5) * (1. - smooth((elapsed - 7.5) / 2.3)) } else { 0. };
    (HUSBAND_HOME, 0.95 + 0.30 * turn)
}

/// The greeting: how far into the handshake and the hug they are (0 to 1 each), and where their right hands meet.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Greeting {
    pub shake: f32,
    pub hug: f32,
    pub centre: Vec3,
}

pub fn greeting(t: f32) -> Greeting {
    let (kind, s) = beat(t);
    if kind != Beat::Greet {
        return Greeting { shake: 0., hug: 0., centre: Vec3::new(0.33, 1.14, 1.15) };
    }
    let shake = smooth((s - 1.3) / 1.4) * (1. - smooth((s - 7.3) / 1.4));
    let hug = smooth((s - 3.2) / 2.) * (1. - smooth((s - 7.3) / 2.1));
    let centre = Vec3::new(0.325, 1.14, 1.15).lerp(Vec3::new(0.38, 1.14, 1.17), hug)
        + Vec3::Y * (((s - 2.6) * 9.).sin() * 0.014 * shake * (1. - hug));
    Greeting { shake, hug, centre }
}

/// The hotdog's eye, in the frame of the player's hotdog in the tray: its place, what it looks at, and the field of
/// view in degrees. It looks out of the fridge, but closes in on the handshake, then the embrace, then turns back on
/// the hotdog itself.
pub fn camera(t: f32) -> [f32; 7] {
    const DEFAULT: [f32; 7] = [0., 0.0325, 0., 0., 0.205, 1.35, 82.];
    const HANDSHAKE: [f32; 7] = [0., 0.0325, 0., 0.355, 0.1875, 1.51, 28.];
    const EMBRACE: [f32; 7] = [0., 0.0325, 0., 0.397, 0.4275, 1.51, 56.];
    const REACTION: [f32; 7] = [0.035, 0.18, -0.17, 0.017, 0.018, 0.04, 48.];
    let (kind, s) = beat(t);
    if kind != Beat::Greet {
        return DEFAULT;
    }
    let keys = [
        (0., DEFAULT),
        (1.5, HANDSHAKE),
        (3.8, HANDSHAKE),
        (5.4, EMBRACE),
        (6.7, EMBRACE),
        (7.8, REACTION),
        (9.1, REACTION),
        (10.7, DEFAULT),
        (11., DEFAULT),
    ];
    for pair in keys.windows(2) {
        let ((a, first), (b, second)) = (pair[0], pair[1]);
        if a <= s && s <= b {
            let k = smooth((s - a) / (b - a));
            return std::array::from_fn(|i| first[i] + (second[i] - first[i]) * k);
        }
    }
    DEFAULT
}

/// The chef knife in the cut scene, `t` seconds into it (`arena::Game::cutscene_time`): on the counter, lifted, then
/// drawn along the front of the seal. The handle is at +z in the knife's own frame, the blade at -z.
#[allow(clippy::approx_constant)] // (3.14 is the seal's front edge, in metres.)
pub fn knife(t: f32) -> Transform {
    let at = if t < 2. {
        KNIFE_REST
    } else if t < 4.5 {
        KNIFE_REST.lerp(Vec3::new(0.79, 1.09, 3.10), smooth((t - 2.) / 2.5))
    } else {
        Vec3::new(0.75 + smooth((t - 4.5) / 5.) * 0.18, 1.005, 3.14)
    };
    Transform::from_translation(at)
        .with_rotation(Quat::from_rotation_z(-0.5) * Quat::from_rotation_x(if t > 4.5 { -0.6 } else { -0.2 }))
}
/// The middle of the knife's handle, in its own frame.
pub const KNIFE_HANDLE: Vec3 = Vec3::new(0., 0., 0.065);

/// The package's clear film in the cut scene, in the package's frame: it peels up and back from 6.2 s.
pub fn film(t: f32) -> Transform {
    let peeling = smooth((t - 6.2) / 5.8);
    Transform::from_xyz(0., 0.038 + peeling * 0.09, 0.)
        .with_rotation(Quat::from_rotation_x(-peeling * 2.2))
        .with_scale(Vec3::new(1., 1., 1. - peeling * 0.7))
}

/// One foot of a step plan: the ankle's place in the world, its facing, and how far the toes are tipped up.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Foot {
    pub at: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}
/// A frame of a step plan: the left foot and the right, whether the body is on the move, and the gait (-1 to 1, which
/// foot is in the air and how high in its swing).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Stride {
    pub feet: [Foot; 2],
    pub walking: bool,
    pub gait: f32,
}

/// The feet's steps over the whole timeline, one frame each `1 / FPS` s: a body that moves or turns lifts one foot at a
/// time (left first) and sets it down where it will stand half a second later, a little ahead of it; the other foot
/// stays planted. `root` gives the body's root and yaw at a clip time; `feet` the ankles' places in the body's frame
/// at rest, left then right.
pub fn plan_steps(root: impl Fn(f32) -> (Vec3, f32), feet: [Vec3; 2]) -> Vec<Stride> {
    let frames = (END * FPS).round() as usize + 1;
    let poses: Vec<(Vec3, f32)> = (0..frames).map(|frame| root(frame as f32 / FPS)).collect();
    let (start_root, start_yaw) = poses[0];
    let mut planted = feet.map(|foot| start_root + rotate(foot, start_yaw));
    let mut angles = [start_yaw; 2];
    // The foot in the air: which, from where and when, to where and when.
    let mut active: Option<(usize, usize, usize, Vec3, f32, Vec3, f32)> = None;
    let mut previous = 1;
    let mut plan = Vec::with_capacity(frames);
    for (frame, &(here, yaw)) in poses.iter().enumerate() {
        let (ahead, ahead_yaw) = poses[(frame + 1).min(frames - 1)];
        let speed = Vec2::new(ahead.x - here.x, ahead.z - here.z).length() * FPS;
        let turning = (ahead_yaw - yaw).abs() * FPS;
        let moving = speed > 0.035 || turning > 0.12;
        if active.is_none() && moving {
            let side = 1 - previous;
            let finish = (frame + 15).min(frames - 1);
            let (future, future_yaw) = poses[finish];
            let mut destination = future + rotate(feet[side], future_yaw);
            let (dx, dz) = (future.x - here.x, future.z - here.z);
            let distance = dx.hypot(dz);
            if distance > 0.001 {
                destination += Vec3::new(dx / distance * 0.065, 0., dz / distance * 0.065);
            }
            active = Some((side, frame, finish, planted[side], angles[side], destination, future_yaw));
        }
        let mut stride = Stride {
            feet: [0, 1].map(|side| Foot { at: planted[side], yaw: angles[side], pitch: 0. }),
            walking: moving,
            gait: 0.,
        };
        if let Some((side, start, finish, origin, origin_yaw, destination, destination_yaw)) = active {
            let u = (frame - start) as f32 / (finish - start).max(1) as f32;
            let point = origin.lerp(destination, smooth(u)) + Vec3::Y * (0.045 * (PI * u).sin());
            let angle = origin_yaw + (destination_yaw - origin_yaw) * smooth(u);
            stride.feet[side] = Foot { at: point, yaw: angle, pitch: 0.10 * (2. * PI * u).sin() };
            stride.gait = (PI * u).sin() * if side == 0 { -1. } else { 1. };
            stride.walking = true;
            if frame >= finish {
                planted[side] = destination;
                angles[side] = destination_yaw;
                previous = side;
                active = None;
            }
        }
        plan.push(stride);
    }
    plan
}

/// A step plan at clip time `t`, between its frames.
pub fn stride_at(plan: &[Stride], t: f32) -> Stride {
    let index = (t * FPS).clamp(0., (plan.len() - 1) as f32);
    let (a, b) = (index.floor() as usize, (index.floor() as usize + 1).min(plan.len() - 1));
    let k = index - a as f32;
    let (p, q) = (plan[a], plan[b]);
    Stride {
        feet: [0, 1].map(|i| Foot {
            at: p.feet[i].at.lerp(q.feet[i].at, k),
            yaw: p.feet[i].yaw + (q.feet[i].yaw - p.feet[i].yaw) * k,
            pitch: p.feet[i].pitch + (q.feet[i].pitch - p.feet[i].pitch) * k,
        }),
        walking: if k < 0.5 { p.walking } else { q.walking },
        gait: p.gait + (q.gait - p.gait) * k,
    }
}

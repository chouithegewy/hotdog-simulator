// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! How a hotdog dies. The simulation only says that it has; everything here is for the eye and
//! changes no rule, no damage and nothing sent over the network.
//!
//! At the moment of death the rig that `player_model` was animating is handed to a ragdoll
//! (`ragdoll`): the torso and eight limb bones become rigid bodies joined at the hips, knees,
//! shoulders and elbows, in the pose and with the speed the hotdog had. What killed it is worked
//! out from what the client can see at that instant (`diagnose`), and decides a short authored
//! reaction: a shove, and for a fraction of a second muscles in the joints and a hand that holds
//! the body up, which let go and leave the rest to gravity. The weapon leaves the hand as a rigid
//! body of its own.
//!
//! The simulation keeps the body where the hotdog died, and others walk there to eat it, so the
//! ragdoll is on a leash: its torso is free within `REACH` of that spot, is drawn back in flight
//! if it is thrown further, and never comes to rest more than `REACH + STRAY` away. The one
//! exception is a body that goes over the edge of the counter, which falls to the kitchen floor.
use crate::player_model::{self, Gait, Joints, Motion, PlayerBody, PlayerModels, Pose, Rig};
use crate::ragdoll::{Ball, Block, Body, Joint, Limit, Ragdoll, Scene, Servo, Tuning, Wedge};
use bevy::prelude::*;
use kitchen_sim::feast::{self, Feast, Player};
use std::f32::consts::PI;

/// The pull the game's jumps and droplets already fall under (`arena::tick`), so a falling body
/// and a jumping hotdog agree. Slower than a real kitchen's, which is what makes something this
/// small read as heavy.
pub const GRAVITY: f32 = 2.8;
/// How far from where the simulation has the body its torso is free to go, and how much further
/// than that it may come to rest. Eating reaches 0.17 m from that spot.
pub const REACH: f32 = 0.07;
pub const STRAY: f32 = 0.04;
/// Seconds a dropped weapon lies on the counter before it shrinks away.
const KEEP: f32 = 15.;
/// Centre of mass of the torso in the rig's torso frame: the middle of the sausage, a little
/// towards the bun.
const CENTRE: Vec3 = Vec3::new(0., 0.098, 0.004);
const TORSO: usize = 0;
/// Bodies: the upper and lower bone of each leg, then of each arm; 0 is the left side.
const fn leg(side: usize, lower: bool) -> usize {
    1 + side * 2 + lower as usize
}
const fn arm(side: usize, lower: bool) -> usize {
    5 + side * 2 + lower as usize
}
/// Joints, in the same order: hip, knee, hip, knee, shoulder, elbow, shoulder, elbow.
const fn hip(side: usize) -> usize {
    side * 2
}
const fn shoulder(side: usize) -> usize {
    4 + side * 2
}
/// Servos: the hand under the hips, the one that keeps the torso upright, a zombie's pull.
const SUPPORT: usize = 0;
const BALANCE: usize = 1;
const DRAG: usize = 2;

/// What killed a hotdog, as far as the client can tell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cause {
    /// A mustard round, or the sniper's ketchup (`hard`), flying along `along`.
    Shot { along: Vec3, hard: bool },
    /// The relish glob bursting at `from`.
    Blast { from: Vec3 },
    /// A bite or the blow (a backstab too) of the hotdog standing at `from`.
    Bite { from: Vec3 },
    /// Hot sauce sprayed along `along`.
    Sauce { along: Vec3 },
    /// The turret's beam along `along`.
    Turret { along: Vec3 },
    /// Hunger or pests: nothing struck it.
    Slump,
    /// A zombie, standing at `from` when the client knows where the zombies are.
    Zombie { from: Option<Vec3> },
}
/// What `diagnose` is shown of the game.
pub struct View<'a> {
    pub roster: &'a [Player],
    /// Rounds in flight, splats, pests, zombies (local play only), turrets and the map.
    pub feast: &'a Feast,
    pub elapsed: f32,
    /// The zombie wave: hotdogs cannot hurt each other yet.
    pub wave: bool,
    /// Zombies and turret pulses are not in `feast` when the round is played on a server.
    pub online: bool,
}
/// What was seen on the previous look, which is what a death is compared against.
#[derive(Default)]
pub struct Memory {
    /// Rounds in flight: id and velocity.
    rounds: Vec<(u32, Vec3)>,
    /// The newest splat.
    splat: u32,
    /// Meals eaten so far by each hotdog.
    meals: Vec<(u32, u32)>,
    elapsed: f32,
}
impl Memory {
    fn look(&mut self, view: &View) {
        self.rounds.clear();
        self.rounds.extend(view.feast.projectiles.iter().map(|q| (q.id, Vec3::from_array(q.v))));
        self.splat = view.feast.splats.iter().map(|s| s.id).fold(self.splat, u32::max);
        self.meals.clear();
        self.meals.extend(view.roster.iter().map(|p| (p.id, p.meals)));
        self.elapsed = view.elapsed;
    }
}
fn flat(v: Vec3, otherwise: Vec3) -> Vec3 {
    Vec3::new(v.x, 0., v.z).try_normalize().unwrap_or(otherwise)
}
/// Works out what killed `victim`. The simulation says what last hurt each hotdog
/// (`Player::last_damage`), and the dead are past hurting, so a dead hotdog's record is the cause:
/// its kind, where it came from and the way it was travelling. A snapshot from before that record
/// existed (or a blow nobody was blamed for) leaves it blank, and then this is inference from what
/// changed since the last look: a round that ended on the victim since then, a neighbour whose meal
/// count rose, a turret beam it stands in, a zombie at its feet, a swarm over it; failing all of
/// those, hunger.
pub fn diagnose(victim: &Player, view: &View, memory: &Memory) -> Cause {
    let at = Vec3::from_array(victim.p);
    let chest = at + Vec3::Y * feast::CHEST[feast::stance(victim)];
    // (A round in the face is its gun's.)
    let record = feast::Damage { kind: feast::plain_kind(victim.last_damage.kind), ..victim.last_damage };
    let from = Vec3::from_array(record.p);
    // The record's direction is a unit vector, or zero when what hurt had none.
    let along = Some(Vec3::from_array(record.d)).filter(|d| d.length_squared() > 0.25).unwrap_or((chest - from).normalize_or(Vec3::NEG_Z));
    match record.kind {
        1 | 2 | feast::SYRUP => return Cause::Shot { along, hard: record.kind == 2 },
        3 | feast::MAYONNAISE => return Cause::Sauce { along },
        4 => return Cause::Blast { from },
        feast::MELEE | feast::BACKSTAB => return Cause::Bite { from },
        feast::TURRET => return Cause::Turret { along },
        feast::ZOMBIE => return Cause::Zombie { from: Some(from) },
        feast::PESTS | feast::HUNGER | feast::FALL | feast::BURN => return Cause::Slump,
        _ => {}
    }
    // A round that ended on the body: its splat is new and lies on the sphere the simulation
    // tests rounds against. A glob also kills with its splash.
    let mut best: Option<(f32, Cause)> = None;
    for splat in view.feast.splats.iter().filter(|s| s.id > memory.splat) {
        let (point, w) = (Vec3::from_array(splat.p), splat.weapon.min(7));
        let gap = point.distance(chest);
        let direct = gap < 0.055 + feast::ROUND_RADIUS[w] + 0.03;
        let along = memory
            .rounds
            .iter()
            .find(|(id, _)| *id == splat.projectile)
            .and_then(|(_, v)| v.try_normalize())
            .unwrap_or((chest - point).normalize_or(Vec3::NEG_Z));
        let cause = match w {
            1 | 2 | 6 if direct => Cause::Shot { along, hard: w == 2 },
            3 | 7 if direct => Cause::Sauce { along },
            4 if gap < feast::SPLASH[4] + 0.03 => Cause::Blast { from: point },
            _ => continue,
        };
        if best.is_none_or(|(nearest, _)| gap < nearest) {
            best = Some((gap, cause));
        }
    }
    if let Some((_, cause)) = best {
        return cause;
    }
    // A bite: someone within reach has just eaten.
    let ate = |p: &Player| memory.meals.iter().any(|(id, meals)| *id == p.id && p.meals > *meals);
    if let Some(biter) = view
        .roster
        .iter()
        .filter(|p| p.id != victim.id && p.alive && ate(p) && Vec3::from_array(p.p).distance(at) < 0.2)
        .min_by(|a, b| Vec3::from_array(a.p).distance(at).total_cmp(&Vec3::from_array(b.p).distance(at)))
    {
        return Cause::Bite { from: Vec3::from_array(biter.p) };
    }
    let beam = view.feast.turrets.iter().find(|(origin, direction)| {
        let range = view.feast.level.cover_from(*origin, *direction).min(1.4);
        kitchen_sim::ray_sphere(*origin, *direction, at + Vec3::Y * 0.08, 0.04).is_some_and(|d| d < range)
    });
    let pulse = view.feast.shots.iter().any(|s| s.owner == 0 && s.projectile == 0 && s.time > memory.elapsed);
    if let Some((_, along)) = beam.filter(|_| pulse) {
        return Cause::Turret { along: *along };
    }
    if view.wave {
        let nearest = view
            .feast
            .enemies
            .iter()
            .map(|e| Vec3::from_array(e.p))
            .filter(|p| p.distance(at) < 0.13)
            .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)));
        if nearest.is_some() {
            return Cause::Zombie { from: nearest };
        }
    }
    if let Some((_, along)) = beam.filter(|_| view.online) {
        return Cause::Turret { along: *along };
    }
    if view.feast.pests.iter().any(|s| feast::infested(s, victim.p)) {
        return Cause::Slump;
    }
    // During the wave nothing else can hurt a hotdog.
    if view.wave { Cause::Zombie { from: None } } else { Cause::Slump }
}

/// The map as a ragdoll meets it: the counter top with its edges, the kitchen floor below, blocks,
/// decks, ramps and props.
pub fn scenery(map: &kitchen_sim::map::Map) -> Scene {
    let floor = feast::FLOOR;
    let rect = |r: [f64; 4], low: f32, high: f32| Block {
        min: Vec3::new(r[0] as f32, low, r[1] as f32),
        max: Vec3::new(r[2] as f32, high, r[3] as f32),
    };
    // The slab `level::build` draws, thickened downwards so nothing fast can pass through it.
    let (cupboard, top) = crate::fridge::Room::fitting(map.bounds).cupboard();
    // (A sink is a hole in it, with its floor below: `kitchen::counter_top`.)
    let mut blocks: Vec<Block> = crate::kitchen::counter_top(map).into_iter().map(|r| rect(r, floor - 0.3, floor)).collect();
    for sink in &map.sinks {
        blocks.push(rect(sink.rect, floor - sink.depth as f32 - 0.3, floor - sink.depth as f32));
    }
    blocks.extend([
        // The kitchen the counter stands in (`fridge::Room`, as big as this map makes it): its floor,
        // and the cupboard under the back of the counter.
        Block { min: Vec3::new(-50., -1., -50.), max: Vec3::new(50., 0., 50.) },
        Block { min: cupboard, max: Vec3::new(top.x, floor - 0.3, top.z) },
    ]);
    // A map whose ground is the kitchen floor stands in a room of its own, its walls at the bounds (`floor`).
    if map.floor {
        let [x0, z0, x1, z1] = map.bounds;
        let (low, high) = (floor - 0.3, floor + crate::floor::HEIGHT);
        for r in [[x0 - 0.3, z0 - 0.3, x0, z1 + 0.3], [x1, z0 - 0.3, x1 + 0.3, z1 + 0.3], [x0, z0 - 0.3, x1, z0], [x0, z1, x1, z1 + 0.3]] {
            blocks.push(rect(r, low, high));
        }
        blocks.push(rect([x0, z0, x1, z1], high, high + 0.3));
    }
    // Fitted cabinets fill the space under the counter, down to the floor (`kitchen::cabinets`).
    if map.cabinets {
        let [x0, z0, x1, z1] = map.bounds;
        blocks.push(rect([x0, z0 - 0.014, x1, z1], 0., floor - 0.3));
    }
    for block in &map.blocks {
        blocks.push(rect(block.rect, floor, floor + block.height as f32));
    }
    for deck in &map.decks {
        let height = deck.height as f32;
        if deck.style == kitchen_sim::map::ROUND {
            // A round deck is a steamer standing on the counter: three boxes that together nearly fill its ellipse (a cross and
            // the square inside it).
            let [x0, z0, x1, z1] = deck.rect;
            let (cx, cz, rx, rz) = ((x0 + x1) / 2., (z0 + z1) / 2., (x1 - x0) / 2., (z1 - z0) / 2.);
            for [a, b] in [[0.92, 0.4], [0.4, 0.92], [0.71, 0.71]] {
                blocks.push(rect([cx - a * rx, cz - b * rz, cx + a * rx, cz + b * rz], floor, floor + height));
            }
            continue;
        }
        // (On the kitchen floor a deck stands solid: the box, the step stool.)
        let plate = if map.floor { height } else { height.min(0.025) };
        blocks.push(rect(deck.rect, floor + height - plate, floor + height));
    }
    for prop in &map.props {
        match prop {
            kitchen_sim::map::Prop::Box { rect: r, base, height, .. } => {
                blocks.push(rect(*r, floor + *base as f32, floor + (*base + *height) as f32));
            }
            kitchen_sim::map::Prop::Turret { p, base, .. } => {
                let (x, y, z) = (p[0] as f32, floor + *base as f32, p[1] as f32);
                blocks.push(Block { min: Vec3::new(x - 0.04, y - 0.035, z - 0.04), max: Vec3::new(x + 0.04, y + 0.035, z + 0.04) });
            }
            // A bin stands on the kitchen floor, out of the way of the counter.
            kitchen_sim::map::Prop::Bin { .. } => {}
        }
    }
    let wedges = map
        .ramps
        .iter()
        .map(|ramp| Wedge {
            min: Vec3::new(ramp.rect[0] as f32, floor + ramp.base as f32, ramp.rect[1] as f32),
            max: Vec3::new(ramp.rect[2] as f32, floor + ramp.height as f32, ramp.rect[3] as f32),
            along_z: ramp.rise.along_z(),
            rising: ramp.rise.sign() > 0.,
        })
        .collect();
    Scene { blocks, wedges }
}

/// A hotdog as a ragdoll, standing where `root` puts it in the pose `p`: the torso, then the two
/// bones of each leg and of each arm.
pub fn hotdog(root: Transform, p: &Pose) -> Ragdoll {
    let (bone, hip_at, shoulder_at, _) = player_model::RIG;
    let pivot = Vec3::Y * hip_at.y;
    let turn = root.rotation * p.torso;
    // Where the rig's torso entity is, which pivots on the hips.
    let origin = root.transform_point(pivot + p.shift - p.torso * pivot);
    let mut bodies = Vec::with_capacity(9);
    let mut balls = Vec::with_capacity(29);
    let mut joints = Vec::with_capacity(8);
    // The sausage is 0.155 m long and 0.046 m thick with half a bun on either side.
    bodies.push(Body::new(origin + turn * CENTRE, turn, 1., Vec3::new(2.3e-3, 8.3e-4, 2.5e-3)));
    for y in [0.046, 0.0735, 0.101, 0.1285, 0.155] {
        balls.push(Ball { body: TORSO, at: Vec3::new(0., y, 0.) - CENTRE, radius: 0.023 });
    }
    for side in [-1., 1.] {
        for y in [0.058, 0.097, 0.136] {
            balls.push(Ball { body: TORSO, at: Vec3::new(side * 0.021, y, 0.016) - CENTRE, radius: 0.020 });
        }
        // The shoulders stand out past the bun: lying on its side it rests on one, with room for the arm.
        balls.push(Ball { body: TORSO, at: Vec3::new(side * (shoulder_at.x - 0.002), shoulder_at.y, 0.) - CENTRE, radius: 0.0075 });
    }
    // A limb bone hangs down its own -y from the joint at its top.
    let (top, tip) = (Vec3::Y * bone * 0.5, Vec3::NEG_Y * bone * 0.5);
    let down = Quat::from_rotation_z(-PI * 0.5);
    let mut limb = |joint: Vec3, turn: Quat, mass: f32| {
        let body = bodies.len();
        bodies.push(Body::new(joint + turn * tip, turn, mass, Body::rod(mass, bone + 0.008, 0.004)));
        for y in [0.005, -0.005] {
            balls.push(Ball { body, at: Vec3::Y * y, radius: 0.0042 });
        }
    };
    for (i, side) in [-1f32, 1.].into_iter().enumerate() {
        let socket = Vec3::new(side * hip_at.x, hip_at.y, 0.);
        let (upper, at) = (root.rotation * p.hips[i], root.transform_point(socket + p.shift));
        limb(at, upper, 0.04);
        limb(at + upper * (tip * 2.), upper * p.knees[i], 0.035);
        // A leg swings within a wide cone under the body and the knee only folds backwards.
        // The torso weighs thirty times what a bone does, so it leads: limbs follow and never prop it up.
        joints.push(Joint::new(TORSO, leg(i, false), [socket - CENTRE, top], [down, down], Limit::Cone { swing: 1.35, twist: 0.8 }).led());
        joints.push(Joint::new(leg(i, false), leg(i, true), [tip, top], [Quat::IDENTITY; 2], Limit::Hinge { min: -2.5, max: 0.15 }));
    }
    for (i, side) in [-1f32, 1.].into_iter().enumerate() {
        let socket = Vec3::new(side * shoulder_at.x, shoulder_at.y, 0.);
        let (upper, at) = (turn * p.shoulders[i], origin + turn * socket);
        limb(at, upper, 0.03);
        limb(at + upper * (tip * 2.), upper * p.elbows[i], 0.025);
        // An arm swings anywhere but up and in through the head, and the elbow only folds forwards.
        let out = Quat::from_rotation_z(-PI * 0.5 + side * 0.9);
        joints.push(Joint::new(TORSO, arm(i, false), [socket - CENTRE, top], [out, down], Limit::Cone { swing: 2., twist: 2. }).led());
        joints.push(Joint::new(arm(i, false), arm(i, true), [tip, top], [Quat::IDENTITY; 2], Limit::Hinge { min: -0.15, max: 2.5 }));
    }
    Ragdoll::new(bodies, balls, joints, 3)
}
/// The rig's local transforms that show a ragdoll, for a hotdog whose root is at `root`: the torso
/// and each hip (place and turn), then the turn of each knee, shoulder and elbow.
pub struct Shown {
    pub torso: (Vec3, Quat),
    pub hips: [(Vec3, Quat); 2],
    pub knees: [Quat; 2],
    pub shoulders: [Quat; 2],
    pub elbows: [Quat; 2],
}
pub fn shown(doll: &Ragdoll, root: Transform) -> Shown {
    let inverse = root.rotation.inverse();
    let local = |p: Vec3| inverse * (p - root.translation);
    let torso = &doll.bodies[TORSO];
    let turn = |body: usize| doll.bodies[body].turn;
    // Legs hang from the torso's own hip sockets, so a joint that has drifted a hair never shows.
    let hips = [0, 1].map(|i| (local(torso.point(doll.joints[hip(i)].at[0])), inverse * turn(leg(i, false))));
    Shown {
        torso: (local(torso.at - torso.turn * CENTRE), inverse * torso.turn),
        hips,
        knees: [0, 1].map(|i| turn(leg(i, false)).inverse() * turn(leg(i, true))),
        shoulders: [0, 1].map(|i| torso.turn.inverse() * turn(arm(i, false))),
        elbows: [0, 1].map(|i| turn(arm(i, false)).inverse() * turn(arm(i, true))),
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
/// 1 until `from`, easing to 0 at `until`.
fn fade(t: f32, from: f32, until: f32) -> f32 {
    1. - smooth((t - from) / (until - from))
}
/// What a limb's two joints are asked to do for a moment: the turn of the upper bone against the
/// torso, of the lower against the upper, and how hard the muscles pull.
#[derive(Clone, Copy)]
struct Muscle {
    upper: Quat,
    lower: Quat,
    drive: f32,
}
const LIMP: Muscle = Muscle { upper: Quat::IDENTITY, lower: Quat::IDENTITY, drive: 0. };

/// One death: the ragdoll, what caused it and how far through the reaction it is.
pub struct Fall {
    pub doll: Ragdoll,
    pub cause: Cause,
    pub age: f32,
    beat: u8,
    seed: u32,
    /// The way the hotdog faced when it died.
    yaw: f32,
    /// 1 for a hotdog that died on its feet, 0 for one that was prone or in the air: nothing
    /// holds those up.
    stance: f32,
    /// It has left the counter, so the leash is off.
    gone: bool,
}
impl Fall {
    /// A hotdog at `at` facing `yaw` in the living pose `pose`, moving at `velocity`, killed by `cause`.
    pub fn new(at: Vec3, yaw: f32, pose: &Pose, prone: f32, velocity: Vec3, cause: Cause, seed: u32, scene: &Scene) -> Self {
        let root = Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw));
        let mut doll = hotdog(root, pose);
        for b in &mut doll.bodies {
            b.velocity = velocity;
        }
        let standing = scene.ground(at + Vec3::Y * 0.02).is_some_and(|ground| at.y - ground < 0.015);
        let mut fall = Self { doll, cause, age: 0., beat: 0, seed, yaw, stance: if standing { 1. - prone } else { 0. }, gone: false };
        let forward = root.rotation * Vec3::NEG_Z;
        let chest = at + Vec3::Y * 0.1;
        let side = fall.side();
        // The blow itself: the whole body takes the speed and the spin at once.
        match cause {
            Cause::Shot { along, hard } => {
                let (along, power) = (flat(along, -forward), if hard { 1. } else { 0.6 });
                fall.shove(along * 0.5 * power + Vec3::Y * 0.1 * power, Vec3::Y.cross(along) * 5.5 * power + Vec3::Y * side * 1.5 * power);
            }
            Cause::Blast { from } => {
                let away = flat(chest - from, -forward);
                let near = 1. - 0.4 * (chest.distance(from) / 0.25).min(1.);
                fall.shove(away * 0.4 * near + Vec3::Y * 0.95 * near, Vec3::Y.cross(away) * 10. * near + Vec3::Y * side * 6.);
            }
            Cause::Bite { from } => {
                let back = flat(at - from, -forward);
                fall.shove(back * 0.26, Vec3::Y.cross(back) * 1.2);
            }
            Cause::Sauce { along } => fall.shove(flat(along, -forward) * 0.14, Vec3::ZERO),
            Cause::Turret { along } => fall.shove(flat(along, -forward) * 0.18, Vec3::Y * side * 17.),
            Cause::Slump => {}
            Cause::Zombie { from } => {
                let towards = from.map_or(forward, |from| flat(from - at, forward));
                fall.shove(Vec3::ZERO, Vec3::Y.cross(-towards) * 1.5);
            }
        }
        fall
    }
    /// +1 or -1, fixed for this death: which way round it spins, which side it keels over to.
    fn side(&self) -> f32 {
        if self.seed & 1 == 0 { 1. } else { -1. }
    }
    /// Seconds the authored reaction lasts before the body is all ragdoll.
    pub fn reaction(&self) -> f32 {
        self.stance
            * match self.cause {
                Cause::Shot { .. } => 0.5,
                Cause::Blast { .. } => 0.45,
                Cause::Bite { .. } => 0.9,
                Cause::Sauce { .. } => 0.9,
                Cause::Turret { .. } => 0.7,
                Cause::Slump => 1.3,
                Cause::Zombie { .. } => 1.1,
            }
    }
    /// Adds a speed and a spin about the torso to the whole body, as if it were rigid.
    fn shove(&mut self, velocity: Vec3, spin: Vec3) {
        let centre = self.doll.bodies[TORSO].at;
        for b in &mut self.doll.bodies {
            b.velocity += velocity + spin.cross(b.at - centre);
            b.spin += spin;
        }
    }
    /// The script: sets the muscles and the outside hands for this instant of the reaction.
    fn direct(&mut self, dt: f32, home: Vec3, scene: &Scene) {
        let (_, hip_at, ..) = player_model::RIG;
        let (t, side) = (self.age, self.side());
        let facing = Quat::from_rotation_y(self.yaw);
        let (forward, right) = (facing * Vec3::NEG_Z, facing * Vec3::X);
        let wave = |rate: f32, phase: f32| (t * rate + phase).sin();
        let about = |z: f32, x: f32| Quat::from_rotation_z(z) * Quat::from_rotation_x(x);
        // How strongly the hips are held up and at what height, how strongly the torso is held
        // to a lean, and which way it faces while it is.
        let (mut support, mut height, mut balance, mut lean, mut spinning) = (0., hip_at.y, 0., Quat::IDENTITY, false);
        let (mut legs, mut arms, mut drag) = ([LIMP; 2], [LIMP; 2], Servo::Idle);
        // A blow that comes part of the way through: (when, speed, spin).
        let mut late: Option<(f32, Vec3, Vec3)> = None;
        match self.cause {
            // Jerked back along the round; the arms are left behind, the legs fold.
            Cause::Shot { along, .. } => {
                let along = flat(along, -forward);
                support = 34. * fade(t, 0.05, 0.22);
                let fling = if along.dot(forward) < 0. { 1.5 } else { -1.2 };
                for i in 0..2 {
                    let out = [-1., 1.][i];
                    arms[i] = Muscle { upper: about(out * 0.5, fling), lower: Quat::from_rotation_x(0.3), drive: 28. * fade(t, 0.08, 0.3) };
                    legs[i] = Muscle { upper: Quat::from_rotation_x(0.9), lower: Quat::from_rotation_x(-1.7), drive: 16. * fade(t, 0.15, 0.5) };
                }
            }
            // Thrown: arms and legs flung wide while it turns over in the air.
            Cause::Blast { .. } => {
                for i in 0..2 {
                    let out = [-1., 1.][i];
                    arms[i] = Muscle { upper: about(out * 2., 0.), lower: Quat::from_rotation_x(0.2), drive: 30. * fade(t, 0.15, 0.45) };
                    legs[i] = Muscle { upper: about(out * 0.7, 0.), lower: Quat::IDENTITY, drive: 20. * fade(t, 0.15, 0.45) };
                }
            }
            // Staggers back hunched over the wound with both hands on it, then pitches forward.
            Cause::Bite { .. } => {
                support = 34. * fade(t, 0.5, 0.72);
                height -= 0.004 * smooth(t / 0.4);
                balance = 20. * fade(t, 0.45, 0.6);
                lean = about(0.1 * wave(17., 0.), -0.38 * smooth(t / 0.3));
                for i in 0..2 {
                    let out = [-1., 1.][i];
                    let step = wave(15., i as f32 * PI);
                    // The arms are too short to meet: each folds in across the belly as far as it goes.
                    let across = Quat::from_rotation_arc(Vec3::NEG_Y, Vec3::new(-0.78 * out, -0.23, -0.59).normalize());
                    arms[i] = Muscle { upper: across, lower: Quat::from_rotation_x(0.35), drive: 34. * fade(t, 0.6, 0.9) };
                    legs[i] = Muscle { upper: Quat::from_rotation_x(0.35 * step), lower: Quat::from_rotation_x(-0.35 - 0.3 * step), drive: 20. * fade(t, 0.45, 0.6) };
                }
                late = Some((0.52, forward * 0.16, Vec3::Y.cross(forward) * 4.2));
            }
            // Dances on the spot beating at itself, then drops.
            Cause::Sauce { along } => {
                support = 34. * fade(t, 0.72, 0.9);
                height += 0.004 * wave(14., 0.).abs();
                balance = 20. * fade(t, 0.68, 0.82);
                lean = Quat::from_rotation_y(0.55 * wave(11., 0.)) * about(0.2 * wave(23., 0.), 0.16 * wave(19., 0.));
                for i in 0..2 {
                    let (out, turn) = ([-1., 1.][i], i as f32 * PI);
                    arms[i] = Muscle {
                        upper: about(out * (1.5 + 0.8 * wave(21., turn)), 0.9 * wave(17., i as f32 * 1.7)),
                        lower: Quat::from_rotation_x(0.7 + 0.6 * wave(29., i as f32)),
                        drive: 40. * fade(t, 0.72, 0.9),
                    };
                    legs[i] = Muscle {
                        upper: Quat::from_rotation_x(0.55 * wave(18., turn)),
                        lower: Quat::from_rotation_x(-0.5 - 0.5 * wave(18., turn)),
                        drive: 30. * fade(t, 0.7, 0.85),
                    };
                }
                late = Some((0.78, Vec3::ZERO, Vec3::Y.cross(flat(along, -forward)) * 2.2));
            }
            // Spun on its feet with the arms flung out, then down along the beam.
            Cause::Turret { along } => {
                let along = flat(along, -forward);
                support = 34. * fade(t, 0.25, 0.6);
                balance = 14. * fade(t, 0.3, 0.55);
                spinning = true;
                for i in 0..2 {
                    arms[i] = Muscle { upper: about([-1., 1.][i] * 1.5, 0.), lower: Quat::IDENTITY, drive: 14. * fade(t, 0.3, 0.7) };
                }
                late = Some((0.5, along * 0.12, Vec3::Y.cross(along) * 3.));
            }
            // Sinks to its knees, sways, and keels over.
            Cause::Slump => {
                let kneel = smooth(t / 0.85);
                support = 34. * fade(t, 1.05, 1.3);
                height -= (hip_at.y - 0.019) * kneel;
                balance = 16. * fade(t, 1., 1.25);
                lean = about(0.07 * wave(3.1, 0.) * (1. - 0.5 * kneel), -0.12 * kneel - 0.2 * smooth((t - 0.85) / 0.4));
                for i in 0..2 {
                    legs[i] = Muscle { upper: Quat::from_rotation_x(0.35 * kneel), lower: Quat::from_rotation_x(-1.9 * kneel), drive: 30. * fade(t, 1.05, 1.3) };
                    arms[i] = Muscle { upper: about([-1., 1.][i] * 0.25, 0.), lower: Quat::from_rotation_x(0.15), drive: 8. * fade(t, 0.9, 1.2) };
                }
                let over = (forward * 0.8 + right * side * 0.6).normalize();
                late = Some((1.08, Vec3::ZERO, Vec3::Y.cross(over) * 1.8));
            }
            // Its feet are pulled out from under it towards the zombie while the arms reach up.
            Cause::Zombie { from } => {
                let towards = from.map_or(forward, |from| flat(from - home, forward));
                let ground = scene.ground(home + Vec3::Y * 0.02).unwrap_or(home.y);
                let pull = 7. * smooth(t / 0.4) * fade(t, 0.8, 1.1) * self.stance;
                if pull > 0. {
                    let target = Vec3::new(home.x, ground + 0.02, home.z) + towards * 0.075;
                    drag = Servo::Hold { body: TORSO, point: Vec3::Y * hip_at.y - CENTRE, target, axes: Vec3::ONE, slack: 0., rate: pull };
                }
                for i in 0..2 {
                    arms[i] = Muscle { upper: about([-1., 1.][i] * 0.4, 2.6), lower: Quat::from_rotation_x(0.3), drive: 26. * fade(t, 0.6, 1.) };
                }
            }
        }
        // A hotdog that was not on its feet has nothing to be held up by: it just takes the blow.
        let (support, balance) = (support * self.stance, balance * self.stance);
        if let Some((when, velocity, spin)) = late {
            if t >= when && self.beat == 0 && self.stance > 0.5 {
                self.beat = 1;
                self.shove(velocity, spin);
            }
        }
        let torso = self.doll.bodies[TORSO];
        let pelvis = Vec3::Y * hip_at.y - CENTRE;
        let under = torso.point(pelvis);
        let ground = scene.ground(under + Vec3::Y * 0.02).filter(|_| support > 0.);
        self.doll.servos[SUPPORT] = match ground {
            Some(ground) => Servo::Hold { body: TORSO, point: pelvis, target: Vec3::Y * (ground + height), axes: Vec3::Y, slack: 0., rate: support },
            None => Servo::Idle,
        };
        self.doll.servos[BALANCE] = if balance <= 0. {
            Servo::Idle
        } else if spinning {
            // Kept upright like a top: free to turn about its length.
            Servo::Level { body: TORSO, axis: Vec3::Y, target: Vec3::Y, rate: balance }
        } else {
            Servo::Face { body: TORSO, target: facing * lean, rate: balance }
        };
        // Feet that are standing grip the counter: a stagger slows instead of sliding like a puck.
        if ground.is_some() {
            let grip = (-4. * dt * support / 34.).exp();
            let v = &mut self.doll.bodies[TORSO].velocity;
            (v.x, v.z) = (v.x * grip, v.z * grip);
        }
        // The leash. In the air, past `REACH` from where the simulation has the body, a soft
        // spring draws the torso back and a damper stops it going further: it bends the flight
        // rather than jerking it. A body lying on the counter is left where it fell unless that is
        // more than `STRAY` further out, and then it is slid back at once rather than crept back.
        // Off for good once the body has left the counter.
        // (In a sink the counter is the sink's floor: a body lying there has not left it.)
        let counter = scene.ground(Vec3::new(home.x, feast::FLOOR + 0.001, home.z)).filter(|g| *g > feast::FLOOR - 0.52).map_or(feast::FLOOR, |g| g.min(feast::FLOOR));
        self.gone |= torso.at.y < counter - 0.03;
        let out = Vec3::new(torso.at.x - home.x, 0., torso.at.z - home.z);
        let far = out.length();
        let lying = torso.touching() > 0;
        let (slack, spring) = if lying { (REACH + STRAY, 400.) } else { (REACH, 120.) };
        if !self.gone && far > slack {
            let out = out / far;
            let v = &mut self.doll.bodies[TORSO].velocity;
            let leaving = v.dot(out).max(0.);
            *v -= out * (spring * (far - slack) * dt + leaving * (1. - (-6. * dt).exp()));
        }
        self.doll.servos[DRAG] = drag;
        for i in 0..2 {
            for (muscle, upper) in [(legs[i], hip(i)), (arms[i], shoulder(i))] {
                let drive = muscle.drive * (0.3 + 0.7 * self.stance);
                (self.doll.joints[upper].pose, self.doll.joints[upper].drive) = (muscle.upper, drive);
                (self.doll.joints[upper + 1].pose, self.doll.joints[upper + 1].drive) = (muscle.lower, drive);
            }
        }
    }
    fn tuning(&self) -> Tuning {
        // A body that has been down for a while stops fidgeting.
        let tired = ((self.age - 4.) / 3.).clamp(0., 1.);
        Tuning {
            gravity: GRAVITY,
            friction: 0.7,
            bounce: 0.15,
            drag: 0.3 + 3. * tired,
            spin_drag: 1. + 6. * tired,
            roll_drag: 2.5 + 10. * tired,
            rest_speed: 0.008 * (1. + 3. * tired),
            rest_spin: 0.6 * (1. + 3. * tired),
            ..Tuning::default()
        }
    }
    /// Moves the death on by `dt`, with the simulation's body at `home`. Does nothing once the
    /// ragdoll is asleep.
    pub fn advance(&mut self, dt: f32, home: Vec3, scene: &Scene) {
        if self.doll.asleep() {
            return;
        }
        if self.age < self.reaction() {
            self.doll.wake();
        }
        self.direct(dt, home, scene);
        self.doll.advance(dt, scene, &self.tuning());
        self.age += dt;
    }
}

/// Collision spheres of a weapon in its own frame (-z ahead, +y up, the grip at the origin), and
/// their radius.
fn weapon_balls(weapon: usize) -> ([Vec3; 4], f32) {
    let muzzle = crate::weapons::MUZZLE[weapon.min(7)];
    match weapon {
        0 => ([Vec3::Z * 0.014, Vec3::ZERO, muzzle * 0.5, muzzle * 0.95], 0.003),
        5 => ([Vec3::Y * -0.012, Vec3::ZERO, Vec3::Y * 0.012, muzzle * 0.9], 0.009),
        // The syrup rifle lies on its grip, the jug of syrup hanging under it, its handguard and its
        // muzzle.
        6 => ([Vec3::new(0., -0.015, 0.006), Vec3::new(0., -0.021, -0.025), muzzle * 0.55, muzzle * 0.93], 0.0065),
        _ => (
            [Vec3::new(0., -0.014, 0.004), Vec3::ZERO, muzzle * 0.5, muzzle * 0.93],
            [0.003, 0.0065, 0.0055, 0.0075, 0.012, 0.009][weapon.min(5)],
        ),
    }
}
/// A weapon let go of: one rigid body, with the grip at `at` and the gun frame turned by `turn`.
pub fn weapon(weapon: usize, at: Vec3, turn: Quat) -> Ragdoll {
    let (balls, radius) = weapon_balls(weapon);
    let centre = weapon_centre(weapon);
    let length = crate::weapons::MUZZLE[weapon.min(7)].length() + 0.02;
    let rod = Body::rod(0.15, length, radius.max(0.006));
    let body = Body::new(at + turn * centre, turn, 0.15, Vec3::new(rod.x, rod.z, rod.y));
    let balls = balls.iter().map(|ball| Ball { body: 0, at: *ball - centre, radius }).collect();
    Ragdoll::new(vec![body], balls, vec![], 0)
}
fn weapon_centre(weapon: usize) -> Vec3 {
    crate::weapons::MUZZLE[weapon.min(7)] * 0.4
}
fn weapon_tuning() -> Tuning {
    Tuning { gravity: GRAVITY, friction: 0.5, bounce: 0.4, drag: 0.2, spin_drag: 0.8, roll_drag: 6., ..Tuning::default() }
}
/// Keeps a dropped weapon out from inside the body that dropped it: where one of its spheres is in
/// the torso it is slid out sideways and woken, so a hotdog that falls on its gun pushes it aside.
pub fn part(weapon: &mut Ragdoll, corpse: &Ragdoll, dt: f32) {
    let torso = &corpse.bodies[TORSO];
    let mut push = Vec3::ZERO;
    for ball in &weapon.balls {
        let at = weapon.bodies[0].point(ball.at);
        for other in corpse.balls.iter().filter(|other| other.body == TORSO) {
            let gap = at - torso.point(other.at);
            let (reach, level) = (ball.radius + other.radius, Vec2::new(gap.x, gap.z));
            if gap.length_squared() >= reach * reach {
                continue;
            }
            // A millimetre of overlap is let be, so that something lying against the body is not
            // nudged for ever.
            let clear = (reach * reach - gap.y * gap.y).max(0.).sqrt() - level.length();
            let way = level.try_normalize().unwrap_or(Vec2::X);
            let needed = way * (clear + 0.0005);
            if clear > 0.001 && needed.length_squared() > push.length_squared() {
                push = Vec3::new(needed.x, 0., needed.y);
            }
        }
    }
    if push != Vec3::ZERO {
        // Eased out over a few frames, and left moving that way.
        let step = push * (12. * dt).min(1.);
        let body = &mut weapon.bodies[0];
        body.at += step;
        body.velocity += step / dt * 0.2;
        weapon.wake();
    }
}
/// The weapon a hotdog was holding, on its way to the counter.
struct Dropped {
    holder: Entity,
    weapon: usize,
    doll: Ragdoll,
    age: f32,
}
impl Dropped {
    fn place(&self) -> (Vec3, Quat) {
        let body = &self.doll.bodies[0];
        (body.at - body.turn * weapon_centre(self.weapon), body.turn)
    }
}
struct Corpse {
    entity: Entity,
    id: u32,
    joints: Joints,
    fall: Fall,
    dropped: Option<Dropped>,
}
impl Corpse {
    fn clear(self, commands: &mut Commands) {
        if let Some(dropped) = self.dropped {
            commands.entity(dropped.holder).try_despawn();
        }
    }
}
/// Marks the root of a weapon lying where a hotdog dropped it.
#[derive(Component)]
pub struct DroppedWeapon;
/// Every hotdog that has fallen this round, and what the game looked like a frame ago.
#[derive(Resource, Default)]
pub struct Deaths {
    pub scene: Scene,
    corpses: Vec<Corpse>,
    /// Hotdogs seen so far, and whether each was alive at the last look.
    watch: Vec<(Entity, bool)>,
    memory: Memory,
}
impl Deaths {
    /// How many ragdolls there are, and how many of them are still moving.
    #[cfg(test)]
    pub fn count(&self) -> (usize, usize) {
        (self.corpses.len(), self.corpses.iter().filter(|c| !c.fall.doll.asleep()).count())
    }
    /// One frame: starts a ragdoll for every hotdog that has just died, removes the ragdoll of any
    /// that is alive again or gone, moves the rest and writes them to their rigs.
    pub fn update(
        &mut self,
        view: &View,
        dt: f32,
        hotdogs: &Query<(Entity, &Rig, &Gait), With<PlayerBody>>,
        transforms: &mut Query<&mut Transform>,
        commands: &mut Commands,
        kit: Option<&crate::weapons::Kit>,
    ) {
        if view.elapsed < self.memory.elapsed - 0.5 {
            // A new round counts its splats from one again.
            self.memory = Memory { rounds: std::mem::take(&mut self.memory.rounds), meals: std::mem::take(&mut self.memory.meals), ..default() };
        }
        // A new round or a new map despawns the hotdogs: their bodies and dropped weapons go too.
        let mut i = 0;
        while i < self.corpses.len() {
            if hotdogs.contains(self.corpses[i].entity) {
                i += 1;
            } else {
                self.corpses.swap_remove(i).clear(commands);
            }
        }
        self.watch.retain(|(entity, _)| hotdogs.contains(*entity));
        for (entity, rig, gait) in hotdogs.iter() {
            let Some(player) = view.roster.iter().find(|p| p.id == gait.id()) else {
                continue;
            };
            let was = match self.watch.iter_mut().find(|(e, _)| *e == entity) {
                Some(seen) => Some(std::mem::replace(&mut seen.1, player.alive)),
                None => {
                    self.watch.push((entity, player.alive));
                    None
                }
            };
            let corpse = self.corpses.iter().position(|c| c.entity == entity);
            if player.alive {
                // Alive again: the ragdoll goes, and `animate` has already posed the living rig.
                if let Some(i) = corpse {
                    self.corpses.swap_remove(i).clear(commands);
                }
                continue;
            }
            if corpse.is_some() || player.consumed {
                continue;
            }
            // It died since the last look, or (joining a round late) was already lying there.
            let fresh = was == Some(true);
            let cause = if fresh { diagnose(player, view, &self.memory) } else { Cause::Slump };
            let motion = gait.motion();
            let pose = player_model::pose(motion);
            let (at, facing) = (Vec3::from_array(player.p), Quat::from_rotation_y(player.yaw));
            let velocity = facing * Vec3::new(motion.strafe, 0., -motion.forward) * player_model::RIG.3;
            let seed = player.id.wrapping_mul(7919).wrapping_add((view.elapsed * 1000.) as u32);
            let mut fall = Fall::new(at, player.yaw, &pose, motion.prone, velocity, cause, seed, &self.scene);
            let mut dropped = None;
            if !fresh {
                for _ in 0..480 {
                    fall.advance(1. / 60., at, &self.scene);
                }
            } else if let Some(kit) = kit {
                dropped = Some(drop_weapon(commands, kit, player, &pose, motion, &fall, seed));
            }
            self.corpses.push(Corpse { entity, id: player.id, joints: rig.joints(), fall, dropped });
        }
        for corpse in &mut self.corpses {
            let Some(player) = view.roster.iter().find(|p| p.id == corpse.id) else {
                continue;
            };
            let home = Vec3::from_array(player.p);
            corpse.fall.advance(dt, home, &self.scene);
            // The root stands where the simulation has the body; every joint is placed against it.
            let root = Transform::from_translation(home).with_rotation(Quat::from_rotation_y(player.yaw));
            let show = shown(&corpse.fall.doll, root);
            let mut set = |entity: Entity, at: Option<Vec3>, turn: Quat| {
                if let Ok(mut tr) = transforms.get_mut(entity) {
                    tr.rotation = turn;
                    if let Some(at) = at {
                        tr.translation = at;
                    }
                }
            };
            set(corpse.entity, Some(root.translation), root.rotation);
            set(corpse.joints.torso, Some(show.torso.0), show.torso.1);
            for i in 0..2 {
                set(corpse.joints.hips[i], Some(show.hips[i].0), show.hips[i].1);
                set(corpse.joints.knees[i], None, show.knees[i]);
                set(corpse.joints.shoulders[i], None, show.shoulders[i]);
                set(corpse.joints.elbows[i], None, show.elbows[i]);
            }
            if let Some(dropped) = &mut corpse.dropped {
                dropped.age += dt;
                part(&mut dropped.doll, &corpse.fall.doll, dt);
                dropped.doll.advance(dt, &self.scene, &weapon_tuning());
                let left = 1. - smooth((dropped.age - KEEP) / 0.5);
                if left <= 0. {
                    commands.entity(dropped.holder).try_despawn();
                    corpse.dropped = None;
                } else if let Ok(mut tr) = transforms.get_mut(dropped.holder) {
                    let (at, turn) = dropped.place();
                    *tr = Transform::from_translation(at).with_rotation(turn).with_scale(Vec3::splat(left));
                }
            }
        }
        self.memory.look(view);
    }
}
/// Lets go of the weapon in the gun hand: it starts where the hand held it, moving as the hand
/// was, with a little toss so that it parts from the body.
fn drop_weapon(commands: &mut Commands, kit: &crate::weapons::Kit, player: &Player, pose: &Pose, motion: Motion, fall: &Fall, seed: u32) -> Dropped {
    let weapon = motion.weapon.min(7);
    let root = Transform::from_translation(Vec3::from_array(player.p)).with_rotation(Quat::from_rotation_y(player.yaw));
    let (doll, at, turn) = let_go(weapon, root, pose, &fall.doll, seed);
    let holder = commands
        .spawn((
            DroppedWeapon,
            Name::new("Dropped weapon"),
            Transform::from_translation(at).with_rotation(turn),
            Visibility::default(),
        ))
        .id();
    let kit = kit.clone();
    commands.queue(move |world: &mut World| {
        if world.get_entity(holder).is_ok() {
            kit.attach(world, holder, weapon, Transform::IDENTITY, Visibility::Inherited);
        }
    });
    Dropped { holder, weapon, doll, age: 0. }
}
/// The weapon as a body of its own at the instant the hand opens, and where its grip is.
pub fn let_go(weapon: usize, root: Transform, pose: &Pose, doll: &Ragdoll, seed: u32) -> (Ragdoll, Vec3, Quat) {
    let hand = root.transform_point(player_model::grip(pose, weapon).0);
    let turn = root.rotation * pose.torso * pose.shoulders[1] * pose.elbows[1] * pose.wrist * crate::weapons::in_hand().rotation;
    let mut dropped = self::weapon(weapon, hand, turn);
    let forearm = &doll.bodies[arm(1, true)];
    let noise = |k: u32| player_model::noise(seed.wrapping_mul(31).wrapping_add(k)) - 0.5;
    let out = (hand - doll.bodies[TORSO].at).normalize_or(Vec3::Y);
    let body = &mut dropped.bodies[0];
    body.velocity = forearm.velocity_at(hand) + flat(out, Vec3::X) * 0.1 + Vec3::Y * 0.12;
    body.spin = forearm.spin + Vec3::new(noise(1), noise(2), noise(3)) * 8.;
    (dropped, hand, turn)
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Deaths>().add_systems(Update, tick.after(crate::network::send));
    }
}
/// Runs after the game and the network have said who is alive and posed everyone, and before
/// transforms are propagated: for a dead hotdog, what is written here is what is drawn.
#[allow(clippy::too_many_arguments)]
fn tick(
    game: Option<Res<crate::arena::Game>>,
    time: Res<Time<Real>>,
    theme: Option<Res<crate::level::MapTheme>>,
    models: Option<Res<PlayerModels>>,
    mut deaths: ResMut<Deaths>,
    hotdogs: Query<(Entity, &Rig, &Gait), With<PlayerBody>>,
    mut transforms: Query<&mut Transform>,
    mut commands: Commands,
    slowmo: Option<Res<crate::slowmo::SlowMo>>,
) {
    let Some(game) = game else {
        return;
    };
    // A kill's slow motion (slowmo.rs) slows the fall with it: online too, where it is for this screen alone.
    // (And a replay's speed, replay/.)
    let pace = slowmo.map_or(1., |s| s.scale(time.elapsed_secs()) * s.replay.unwrap_or(1.));
    // `level::build` sets the theme again whenever a map is loaded.
    if deaths.scene.blocks.is_empty() || theme.is_some_and(|theme| theme.is_changed()) {
        deaths.scene = scenery(&game.feast.level.map);
    }
    // (The titan is not a hotdog that falls in a heap: titan/ draws its fall. Its body is left out.)
    let titan: Vec<Player>;
    let roster = match crate::titan::titan_id(&game) {
        Some(id) => {
            titan = game.roster.iter().filter(|p| p.id != id).cloned().collect();
            &titan[..]
        }
        None => &game.roster[..],
    };
    let view = View {
        roster,
        feast: &game.feast,
        elapsed: game.elapsed,
        wave: game.phase_name() == "Siege",
        online: game.network,
    };
    // (Kung Fu has no guns: nothing is dropped.)
    let kit = models.as_deref().map(|models| &models.weapons).filter(|_| !game.kung_fu());
    deaths.update(&view, time.delta_secs().min(0.05) * pace, &hotdogs, &mut transforms, &mut commands, kit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_model::Cues;
    use kitchen_sim::feast::{Splat, Swarm, Zombie};
    use kitchen_sim::map::Map;

    const FLOOR: f32 = feast::FLOOR;
    /// Open counter on the bundled map, well away from the blocks and the edge.
    const OPEN: Vec3 = Vec3::new(0.2, FLOOR, 3.0);
    fn standing() -> Pose {
        player_model::pose(Motion { clap: 1., swing: 1., weapon: 1, aim: 1., ..default() })
    }
    fn causes() -> [(&'static str, Cause); 8] {
        [
            ("mustard", Cause::Shot { along: Vec3::X, hard: false }),
            ("ketchup", Cause::Shot { along: Vec3::X, hard: true }),
            ("relish", Cause::Blast { from: OPEN + Vec3::new(-0.1, 0.05, 0.) }),
            ("bite", Cause::Bite { from: OPEN + Vec3::new(0., 0., -0.15) }),
            ("hot sauce", Cause::Sauce { along: Vec3::X }),
            ("turret", Cause::Turret { along: Vec3::X }),
            ("hunger", Cause::Slump),
            ("zombie", Cause::Zombie { from: Some(OPEN + Vec3::new(0., 0., -0.08)) }),
        ]
    }
    fn fall(cause: Cause, seed: u32) -> (Fall, Scene) {
        let scene = scenery(&Map::bundled());
        (Fall::new(OPEN, 0., &standing(), 0., Vec3::ZERO, cause, seed, &scene), scene)
    }
    /// Runs a death for `seconds` at 60 frames a second.
    fn play(fall: &mut Fall, scene: &Scene, home: Vec3, seconds: f32, mut each: impl FnMut(&Fall, f32)) {
        for frame in 0..(seconds * 60.).round() as usize {
            fall.advance(1. / 60., home, scene);
            each(fall, (frame + 1) as f32 / 60.);
        }
    }
    fn torso(fall: &Fall) -> &Body {
        &fall.doll.bodies[TORSO]
    }
    /// The way the head points.
    fn up(fall: &Fall) -> Vec3 {
        torso(fall).turn * Vec3::Y
    }
    fn astray(fall: &Fall, home: Vec3) -> f32 {
        let at = torso(fall).at;
        Vec2::new(at.x - home.x, at.z - home.z).length()
    }

    #[test]
    fn the_ragdoll_starts_exactly_as_the_hotdog_was_posed() {
        let (bone, hip_at, shoulder_at, _) = player_model::RIG;
        let root = Transform::from_translation(OPEN).with_rotation(Quat::from_rotation_y(0.9));
        for motion in [
            Motion { clap: 1., swing: 1., ..default() },
            Motion { phase: 1.2, forward: 1., aim: 1., weapon: 2, clap: 1., swing: 1., ..default() },
            Motion { phase: 4., strafe: -1., clap: 1., swing: 1., ..default() },
            Motion { prone: 1., forward: 0.4, phase: 2., clap: 1., swing: 1., ..default() },
            Motion { cheer: 1., time: 3., clap: 1., swing: 1., ..default() },
            Motion { kick: 1.4, rumble: 1., aim: 1., weapon: 4, clap: 1., swing: 1., ..default() },
        ] {
            let p = player_model::pose(motion);
            let doll = hotdog(root, &p);
            assert_eq!((doll.bodies.len(), doll.joints.len()), (9, 8));
            let show = shown(&doll, root);
            let pivot = Vec3::Y * hip_at.y;
            let close = |a: Quat, b: Quat| a.angle_between(b) < 1e-3;
            assert!(show.torso.0.distance(pivot + p.shift - p.torso * pivot) < 1e-5 && close(show.torso.1, p.torso), "{motion:?}");
            for (i, side) in [-1f32, 1.].into_iter().enumerate() {
                // The rig hangs the legs from the root; the ragdoll hangs them from the torso, a
                // hair away when the torso rolls.
                assert!(show.hips[i].0.distance(Vec3::new(side * hip_at.x, hip_at.y, 0.) + p.shift) < 0.004, "{motion:?}");
                assert!(close(show.hips[i].1, p.hips[i]) && close(show.knees[i], p.knees[i]), "{motion:?}");
                assert!(close(show.shoulders[i], p.shoulders[i]) && close(show.elbows[i], p.elbows[i]), "{motion:?}");
                // Bones are whole: each lower bone starts where the upper one ends.
                let (upper, lower) = (&doll.bodies[leg(i, false)], &doll.bodies[leg(i, true)]);
                assert!(upper.point(Vec3::NEG_Y * bone * 0.5).distance(lower.point(Vec3::Y * bone * 0.5)) < 1e-5);
                let socket = doll.bodies[TORSO].point(Vec3::new(side * shoulder_at.x, shoulder_at.y, 0.) - CENTRE);
                assert!(socket.distance(doll.bodies[arm(i, false)].point(Vec3::Y * bone * 0.5)) < 1e-5);
            }
            for j in &doll.joints {
                assert!(j.gap(&doll.bodies) < 0.004, "{motion:?}: a joint starts {} apart", j.gap(&doll.bodies));
            }
        }
        // Standing at ease every joint is inside its limits, so nothing snaps at the moment of death.
        let doll = hotdog(root, &standing());
        for j in &doll.joints {
            let angle = j.angle(&doll.bodies);
            assert!(match j.limit {
                Limit::Cone { swing, .. } => angle < swing,
                Limit::Hinge { min, max } => angle > min - 0.01 && angle < max + 0.01,
            });
        }
    }
    #[test]
    fn a_round_knocks_it_back_along_its_flight_and_the_sniper_hits_harder() {
        for along in [Vec3::X, Vec3::NEG_Z, Vec3::new(-0.6, 0.1, 0.8).normalize()] {
            let flat = Vec3::new(along.x, 0., along.z).normalize();
            let mut reach = [0f32; 2];
            for (i, hard) in [false, true].into_iter().enumerate() {
                let (mut fall, scene) = fall(Cause::Shot { along, hard }, 3);
                play(&mut fall, &scene, OPEN, 0.25, |_, _| {});
                // A quarter of a second in it is going over backwards: head and body along the shot.
                assert!(up(&fall).dot(flat) > 0.5, "{along} hard {hard}: the head points {}", up(&fall));
                assert!((torso(&fall).at - OPEN).dot(flat) > 0.03 && torso(&fall).at.y < OPEN.y + 0.085);
                reach[i] = (torso(&fall).at - OPEN).dot(flat);
                play(&mut fall, &scene, OPEN, 4., |_, _| {});
                assert!(fall.doll.asleep() && up(&fall).y.abs() < 0.2 && up(&fall).dot(flat) > 0.5, "it lies along the shot: {}", up(&fall));
            }
            assert!(reach[1] > reach[0] + 0.01, "ketchup throws further than mustard: {reach:?}");
        }
    }
    #[test]
    fn a_blast_throws_it_up_and_away_turning_over() {
        for from in [Vec3::new(-0.1, 0.05, 0.), Vec3::new(0.05, 0., 0.12), Vec3::new(0.15, 0.1, -0.1)] {
            let (mut fall, scene) = fall(Cause::Blast { from: OPEN + from }, 6);
            let away = Vec3::new(-from.x, 0., -from.z).normalize();
            let tumble = Vec3::Y.cross(away);
            let (mut highest, mut turned) = (0f32, 0f32);
            play(&mut fall, &scene, OPEN, 0.5, |fall, _| {
                highest = highest.max(torso(fall).at.y - OPEN.y);
                turned += torso(fall).spin.dot(tumble) / 60.;
                if fall.age < 0.2 {
                    assert!(torso(fall).velocity.dot(away) > 0.15, "thrown away from the burst");
                    assert!(torso(fall).spin.dot(tumble) > 3., "head over heels, away from the burst");
                }
            });
            // The middle of a standing hotdog is 0.1 m up: it leaves the counter altogether.
            assert!(highest > 0.16, "from {from}: only rose to {highest}");
            assert!(turned > 2., "from {from}: it turned {turned} rad");
            play(&mut fall, &scene, OPEN, 5., |_, _| {});
            assert!(fall.doll.asleep() && up(&fall).y.abs() < 0.3);
        }
        // Nearer the burst it is thrown harder.
        let rise = |gap: f32| {
            let (mut fall, scene) = fall(Cause::Blast { from: OPEN + Vec3::new(-gap, 0.1, 0.) }, 6);
            let mut highest = 0f32;
            play(&mut fall, &scene, OPEN, 0.6, |fall, _| highest = highest.max(torso(fall).at.y));
            highest
        };
        assert!(rise(0.03) > rise(0.2) + 0.02);
    }
    #[test]
    fn a_bite_staggers_it_back_clutching_then_it_pitches_forward() {
        for yaw in [0f32, 2.2] {
            let scene = scenery(&Map::bundled());
            let forward = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;
            let mut fall = Fall::new(OPEN, yaw, &standing(), 0., Vec3::ZERO, Cause::Bite { from: OPEN + forward * 0.15 }, 2, &scene);
            let hands = |fall: &Fall| {
                let chest = torso(fall).point(Vec3::new(0., 0.085, -0.024) - CENTRE);
                [0, 1].map(|i| fall.doll.bodies[arm(i, true)].point(Vec3::NEG_Y * 0.008).distance(chest))
            };
            let apart = hands(&fall);
            play(&mut fall, &scene, OPEN, 0.4, |_, _| {});
            // Still on its feet, driven back from the biter, hunched, both hands come in to the wound.
            assert!(torso(&fall).at.y > OPEN.y + 0.08 && up(&fall).y > 0.85, "on its feet: {} {}", torso(&fall).at, up(&fall));
            assert!((torso(&fall).at - OPEN).dot(-forward) > 0.03, "driven back");
            assert!(up(&fall).dot(forward) > 0.2, "hunched over");
            let clutching = hands(&fall);
            assert!(clutching[0] < apart[0] - 0.008 && clutching[1] < apart[1] - 0.008, "hands {apart:?} -> {clutching:?}");
            play(&mut fall, &scene, OPEN, 4., |_, _| {});
            assert!(fall.doll.asleep() && up(&fall).dot(forward) > 0.7 && up(&fall).y.abs() < 0.2, "face down, forwards: {}", up(&fall));
        }
    }
    #[test]
    fn hot_sauce_makes_it_flail_on_its_feet_before_it_drops() {
        let (mut fall, scene) = fall(Cause::Sauce { along: Vec3::X }, 4);
        let (mut swept, mut last) = ([0f32; 2], [Quat::IDENTITY; 2]);
        play(&mut fall, &scene, OPEN, 0.65, |fall, t| {
            assert!(torso(fall).at.y > OPEN.y + 0.085 && up(fall).y > 0.8, "on its feet at {t}");
            for i in 0..2 {
                let now = torso(fall).turn.inverse() * fall.doll.bodies[arm(i, false)].turn;
                if t > 0.05 {
                    swept[i] += now.angle_between(last[i]);
                }
                last[i] = now;
            }
        });
        // Each arm sweeps through several radians in that time: beating, not hanging.
        assert!(swept[0] > 4. && swept[1] > 4., "arms swept {swept:?}");
        play(&mut fall, &scene, OPEN, 4.5, |_, _| {});
        assert!(fall.doll.asleep() && torso(&fall).at.y < OPEN.y + 0.045 && up(&fall).y.abs() < 0.25);
        // A hungry hotdog's arms hang still in the same time.
        let (mut calm, scene) = self::fall(Cause::Slump, 4);
        let (mut hung, mut last) = (0f32, Quat::IDENTITY);
        play(&mut calm, &scene, OPEN, 0.65, |fall, t| {
            let now = torso(fall).turn.inverse() * fall.doll.bodies[arm(0, false)].turn;
            if t > 0.05 {
                hung += now.angle_between(last);
            }
            last = now;
        });
        assert!(hung < 1., "a slumping arm swept {hung}");
    }
    #[test]
    fn the_turret_spins_it_round_before_it_goes_down_along_the_beam() {
        let mut signs = vec![];
        for seed in [4, 5] {
            let (mut fall, scene) = fall(Cause::Turret { along: Vec3::X }, seed);
            let mut turned = 0f32;
            play(&mut fall, &scene, OPEN, 0.4, |fall, _| {
                turned += torso(fall).spin.y / 60.;
                assert!(up(fall).y > 0.9 && torso(fall).at.y > OPEN.y + 0.085, "upright while it spins");
            });
            // Most of a full turn in under half a second, one way or the other.
            assert!(turned.abs() > 4., "seed {seed}: turned {turned} rad");
            signs.push(turned.signum());
            play(&mut fall, &scene, OPEN, 4.5, |_, _| {});
            assert!(fall.doll.asleep() && up(&fall).y.abs() < 0.25 && (torso(&fall).at - OPEN).x > 0.02, "down along the beam: {}", torso(&fall).at - OPEN);
        }
        assert!(signs[0] != signs[1], "the spin goes either way");
    }
    #[test]
    fn hunger_sinks_it_to_its_knees_before_it_keels_over() {
        let (mut fall, scene) = fall(Cause::Slump, 4);
        let (_, hip_at, ..) = player_model::RIG;
        let hips = |fall: &Fall| torso(fall).point(Vec3::Y * hip_at.y - CENTRE).y - OPEN.y;
        assert!((hips(&fall) - hip_at.y).abs() < 1e-4);
        let mut fastest = 0f32;
        play(&mut fall, &scene, OPEN, 0.95, |fall, _| fastest = fastest.max(torso(fall).velocity.length()));
        // Nearly a second in it is on its knees: hips at knee height, shins folded back, still upright.
        assert!(hips(&fall) < 0.023 && hips(&fall) > 0.015, "hips at {}", hips(&fall));
        assert!(up(&fall).y > 0.9, "upright on its knees: {}", up(&fall));
        assert!(fall.doll.joints[hip(0) + 1].angle(&fall.doll.bodies) < -1.2, "the knees are bent");
        assert!(fastest < 0.12, "a slump is slow: {fastest} m/s");
        play(&mut fall, &scene, OPEN, 1.2, |_, _| {});
        assert!(up(&fall).y < 0.3, "then over: {}", up(&fall));
        play(&mut fall, &scene, OPEN, 4., |_, _| {});
        assert!(fall.doll.asleep() && up(&fall).dot(Vec3::NEG_Z) > 0.5, "forwards: {}", up(&fall));
    }
    #[test]
    fn a_zombie_drags_it_down_by_the_feet() {
        for towards in [Vec3::NEG_Z, Vec3::X] {
            let (mut fall, scene) = fall(Cause::Zombie { from: Some(OPEN + towards * 0.08) }, 4);
            let (_, hip_at, ..) = player_model::RIG;
            play(&mut fall, &scene, OPEN, 0.5, |_, _| {});
            let hips = torso(&fall).point(Vec3::Y * hip_at.y - CENTRE);
            // The hips go down and towards the zombie; the head falls away from it.
            assert!(hips.y < OPEN.y + 0.04 && (hips - OPEN).dot(towards) > 0.02, "{towards}: hips at {}", hips - OPEN);
            assert!(up(&fall).dot(-towards) > 0.6, "{towards}: the head points {}", up(&fall));
            play(&mut fall, &scene, OPEN, 5., |_, _| {});
            assert!(fall.doll.asleep());
        }
        // With no zombie to be seen (online) it is pulled the way it was facing.
        let (mut fall, scene) = fall(Cause::Zombie { from: None }, 4);
        play(&mut fall, &scene, OPEN, 0.5, |_, _| {});
        assert!(up(&fall).z > 0.6);
    }
    #[test]
    fn a_body_comes_to_rest_within_reach_of_where_the_simulation_has_it() {
        let scene = scenery(&Map::bundled());
        // Open counter, tight against a steel block, on the deck, half way up a ramp.
        let (deck, ramp) = (Vec3::new(1.5, FLOOR + 0.28, 3.4), Vec3::new(0.9, FLOOR + 0.14, 3.53));
        let mut furthest = 0f32;
        for (place, home) in [OPEN, Vec3::new(-0.2, FLOOR, 2.81), deck, ramp].into_iter().enumerate() {
            for (name, cause) in causes() {
                for seed in 0..4 {
                    let yaw = seed as f32 * 1.7;
                    // The same cause seen from this spot.
                    let moved = |p: Vec3| p - OPEN + home;
                    let cause = match cause {
                        Cause::Blast { from } => Cause::Blast { from: moved(from) },
                        Cause::Bite { from } => Cause::Bite { from: moved(from) },
                        Cause::Zombie { from } => Cause::Zombie { from: from.map(moved) },
                        Cause::Shot { hard, .. } => Cause::Shot { along: Quat::from_rotation_y(yaw * 2.) * Vec3::X, hard },
                        other => other,
                    };
                    let mut fall = Fall::new(home, yaw, &standing(), 0., Vec3::ZERO, cause, seed, &scene);
                    let mut slept = None;
                    play(&mut fall, &scene, home, 8., |fall, t| {
                        if fall.doll.asleep() && slept.is_none() {
                            slept = Some(t);
                        }
                        assert!(fall.doll.bodies.iter().all(|b| b.at.is_finite()), "{name} blew up");
                    });
                    let slept = slept.unwrap_or_else(|| panic!("{name} seed {seed} at place {place} never came to rest"));
                    assert!(slept < 7., "{name} seed {seed} at place {place} took {slept} s");
                    furthest = furthest.max(astray(&fall, home));
                    assert!(astray(&fall, home) < REACH + STRAY + 0.01, "{name} seed {seed} at place {place} lies {} m away", astray(&fall, home));
                    // Lying on something, not in it and not under the counter.
                    for ball in &fall.doll.balls {
                        let centre = fall.doll.bodies[ball.body].point(ball.at);
                        let slack = if ball.body == TORSO { 0.0005 } else { 0.003 };
                        assert!(scene.clear(centre, ball.radius, slack), "{name} seed {seed} at place {place}: a part rests inside the scenery at {centre}");
                        assert!(centre.y > FLOOR, "{name} seed {seed} at place {place}: a part is under the counter at {centre}");
                    }
                    // Down: flat on the counter or the deck, slumped against the block it died
                    // beside, or draped over the side of the narrow ramp.
                    assert!(up(&fall).y.abs() < [0.5, 0.9, 0.5, 0.75][place], "{name} seed {seed} at place {place} is not lying down: {}", up(&fall));
                }
            }
        }
        println!("furthest from the simulation's body: {furthest:.3} m");
    }
    #[test]
    fn a_body_thrown_past_the_edge_drops_off_the_counter() {
        let scene = scenery(&Map::bundled());
        // Standing at the front edge of the counter, which runs along z = 2.27, blasted from behind.
        let home = Vec3::new(0.2, FLOOR, 2.28);
        let mut fall = Fall::new(home, 0., &standing(), 0., Vec3::ZERO, Cause::Blast { from: home + Vec3::new(0., 0.05, 0.1) }, 6, &scene);
        let mut lowest = f32::MAX;
        play(&mut fall, &scene, home, 3., |fall, _| lowest = lowest.min(torso(fall).at.y));
        // Over the edge and all the way to the kitchen floor, where it lies.
        assert!(fall.gone && torso(&fall).at.y < 0.06 && lowest > 0.015, "{} (lowest {lowest})", torso(&fall).at);
        play(&mut fall, &scene, home, 4., |_, _| {});
        assert!(fall.doll.asleep());
        // The same blast from in front leaves it on the counter, inside the leash.
        let mut fall = Fall::new(home, 0., &standing(), 0., Vec3::ZERO, Cause::Blast { from: home + Vec3::new(0., 0.05, -0.1) }, 6, &scene);
        play(&mut fall, &scene, home, 6., |_, _| {});
        assert!(!fall.gone && fall.doll.asleep() && torso(&fall).at.y > FLOOR && astray(&fall, home) < REACH + STRAY + 0.01);
    }
    #[test]
    fn a_hotdog_that_was_prone_or_in_the_air_is_not_stood_up() {
        let scene = scenery(&Map::bundled());
        let prone = player_model::pose(Motion { prone: 1., clap: 1., swing: 1., ..default() });
        for (_, cause) in causes() {
            let mut fall = Fall::new(OPEN, 0., &prone, 1., Vec3::ZERO, cause, 3, &scene);
            assert_eq!(fall.reaction(), 0.);
            let mut highest = 0f32;
            play(&mut fall, &scene, OPEN, 1., |fall, _| highest = highest.max(torso(fall).at.y - OPEN.y));
            let limit = if matches!(cause, Cause::Blast { .. }) { 0.2 } else { 0.06 };
            assert!(highest < limit, "{cause:?} lifted a prone hotdog to {highest}");
        }
        // Killed at the top of a jump, it falls from there.
        let mut fall = Fall::new(OPEN + Vec3::Y * 0.1, 0., &standing(), 0., Vec3::ZERO, Cause::Slump, 3, &scene);
        let before = torso(&fall).at.y;
        play(&mut fall, &scene, OPEN, 0.25, |_, _| {});
        assert!(torso(&fall).at.y < before - 0.05, "it should be falling: {} from {before}", torso(&fall).at.y);
    }
    #[test]
    fn a_running_hotdog_carries_its_speed_into_the_fall() {
        let scene = scenery(&Map::bundled());
        let running = player_model::pose(Motion { forward: 1., phase: 1., clap: 1., swing: 1., ..default() });
        let mut fall = Fall::new(OPEN, 0., &running, 0., Vec3::NEG_Z * 0.67, Cause::Shot { along: Vec3::X, hard: false }, 3, &scene);
        play(&mut fall, &scene, OPEN, 0.3, |_, _| {});
        assert!(torso(&fall).at.z < OPEN.z - 0.04, "{}", torso(&fall).at - OPEN);
    }
    #[test]
    fn the_same_death_plays_out_the_same_way_every_time() {
        let bits = |seed: u32| {
            let (mut fall, scene) = fall(Cause::Blast { from: OPEN + Vec3::new(-0.1, 0.05, 0.03) }, seed);
            play(&mut fall, &scene, OPEN, 3., |_, _| {});
            fall.doll.bodies.iter().map(|b| (b.at.to_array().map(f32::to_bits), b.turn.to_array().map(f32::to_bits))).collect::<Vec<_>>()
        };
        assert_eq!(bits(6), bits(6));
        assert_ne!(bits(6), bits(7), "the seed chooses which way it spins");
    }
    #[test]
    fn the_weapon_leaves_the_hand_falls_bounces_and_lies_on_the_counter() {
        let scene = scenery(&Map::bundled());
        let root = Transform::from_translation(OPEN);
        for weapon in 1..7 {
            let pose = player_model::pose(Motion { clap: 1., swing: 1., weapon, aim: (weapon > 0) as u8 as f32, ..default() });
            let fall = Fall::new(OPEN, 0., &pose, 0., Vec3::NEG_Z * 0.3, Cause::Shot { along: Vec3::X, hard: true }, 3, &scene);
            let (mut dropped, at, turn) = let_go(weapon, root, &pose, &fall.doll, 3);
            // It starts in the hand, exactly as it was held, moving with the hand.
            let (hand, tip) = player_model::grip(&pose, weapon);
            assert!(at.distance(OPEN + hand) < 1e-5);
            assert!((turn * crate::weapons::MUZZLE[weapon]).distance(tip - hand) < 1e-5, "weapon {weapon} is turned as it was held");
            let forearm = &fall.doll.bodies[arm(1, true)];
            let body = dropped.bodies[0];
            assert!((body.velocity_at(at) - forearm.velocity_at(at)).length() < 0.35, "weapon {weapon} leaves at the hand's speed");
            assert!(forearm.velocity_at(at).length() > 0.3, "and the hand was moving");
            let (radius, tune) = (weapon_balls(weapon).1, weapon_tuning());
            let lowest = |doll: &Ragdoll| doll.balls.iter().map(|b| doll.bodies[0].point(b.at).y - b.radius).fold(f32::MAX, f32::min) - FLOOR;
            let highest = |doll: &Ragdoll| doll.balls.iter().map(|b| doll.bodies[0].point(b.at).y - b.radius).fold(f32::MIN, f32::max) - FLOOR;
            let (mut landed, mut rebound, mut slept) = (false, 0f32, None);
            for frame in 0..600 {
                dropped.advance(1. / 60., &scene, &tune);
                let height = lowest(&dropped);
                assert!(height > -0.0015, "weapon {weapon} sank {height} into the counter");
                // Once all of it is down, how far an end comes back up: the clatter.
                landed |= highest(&dropped) < 0.004;
                if landed {
                    rebound = rebound.max(highest(&dropped));
                }
                if dropped.asleep() && slept.is_none() {
                    slept = Some(frame as f32 / 60.);
                }
            }
            println!("weapon {weapon}: rebounds {:.1} mm, asleep after {slept:?}, {:.3} m from the hotdog", rebound * 1000., dropped.bodies[0].at.distance(OPEN));
            assert!(rebound > 0.004, "weapon {weapon} did not bounce: {rebound}");
            assert!(slept.is_some_and(|t| t < 6.), "weapon {weapon} never lay still");
            assert!(lowest(&dropped).abs() < 0.0005 && dropped.bodies[0].at.y < FLOOR + radius * 3., "weapon {weapon} lies on the counter");
            assert!(dropped.bodies[0].at.distance(OPEN) < 0.6, "weapon {weapon} went too far");
        }
    }
    #[test]
    fn a_body_that_falls_on_its_weapon_pushes_it_aside() {
        let scene = scenery(&Map::bundled());
        // How far the pistol is into the body's torso: over nothing, clear of it.
        let depth = |gun: &Ragdoll, fall: &Fall| {
            let mut deepest = f32::MIN;
            for ball in &gun.balls {
                let at = gun.bodies[0].point(ball.at);
                for other in fall.doll.balls.iter().filter(|other| other.body == TORSO) {
                    deepest = deepest.max(ball.radius + other.radius - at.distance(torso(fall).point(other.at)));
                }
            }
            deepest
        };
        // Which way a hotdog keels over, and how suddenly, is not the same on every machine: the last digit of a sine
        // differs from one maths library to the next, and a fall magnifies it. So the pistol is laid under where the
        // body of *this* run comes to rest (the weapon does not push the body, so the fall is the same with it there),
        // for falls of both kinds, and what is asked is what matters: the body came down on it, shoved it well clear,
        // and both lie still. (How deep it got before being shoved is a millimetre or two, either side of any line.)
        for seed in 0..4 {
            let rest = {
                let (mut fall, _) = fall(Cause::Slump, seed);
                for _ in 0..360 {
                    fall.advance(1. / 60., OPEN, &scene);
                }
                torso(&fall).at
            };
            let (mut fall, _) = fall(Cause::Slump, seed);
            let laid = Vec3::new(rest.x, FLOOR + 0.01, rest.z);
            let mut gun = weapon(1, laid, Quat::from_rotation_y(1.));
            let mut deepest = f32::MIN;
            for _ in 0..360 {
                fall.advance(1. / 60., OPEN, &scene);
                deepest = deepest.max(depth(&gun, &fall));
                part(&mut gun, &fall.doll, 1. / 60.);
                gun.advance(1. / 60., &scene, &weapon_tuning());
            }
            let (at, left) = (gun.bodies[0].at, depth(&gun, &fall));
            let moved = Vec2::new(at.x - laid.x, at.z - laid.z).length();
            let told = format!("fall {seed}: the body rests at {rest}, the pistol went {moved} m to {at}, was {deepest} m in at the deepest and is {left} m in now");
            assert!(deepest > 0., "the body did fall on it ({told})");
            assert!(moved > 0.02, "and shoved it aside ({told})");
            assert!(left < 0.002, "so it ends beside the body, not in it ({told})");
            assert!(gun.asleep() && fall.doll.asleep() && at.y < FLOOR + 0.03 && at.y > FLOOR, "{told}");
        }
    }
    #[test]
    fn every_weapon_ball_is_inside_the_weapon() {
        for weapon in 1..7 {
            let (lo, hi) = crate::weapons::model(weapon)
                .iter()
                .flat_map(|(_, shape)| shape.positions.iter().copied())
                .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)));
            let (balls, radius) = weapon_balls(weapon);
            for ball in balls {
                assert!(ball.cmpgt(lo - radius - 0.004).all() && ball.cmplt(hi + radius + 0.004).all(), "weapon {weapon}: {ball} is outside {lo}..{hi}");
            }
            // And they span most of its length, so it lies along the counter rather than on end.
            let span = balls.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.z), b.max(p.z)));
            assert!(span.1 - span.0 + 2. * radius > (hi.z - lo.z) * 0.6 || weapon == 5, "weapon {weapon}");
        }
    }

    /// A round on the bundled map with hotdog 2 standing at `OPEN`, and what was seen a frame ago.
    fn scene_of_the_crime() -> (Feast, Memory) {
        let mut feast = Feast::new(false);
        feast.stage = feast::Stage::FreeForAll;
        feast.elapsed = 40.;
        feast.players[1].p = OPEN.to_array();
        let mut memory = Memory::default();
        memory.look(&View { roster: &feast.players, feast: &feast, elapsed: 39.97, wave: false, online: false });
        (feast, memory)
    }
    fn verdict(feast: &Feast, memory: &Memory, wave: bool, online: bool) -> Cause {
        let mut victim = feast.players[1].clone();
        victim.alive = false;
        diagnose(&victim, &View { roster: &feast.players, feast, elapsed: feast.elapsed, wave, online }, memory)
    }
    #[test]
    fn the_cause_is_read_from_what_ended_on_the_body() {
        let chest = OPEN + Vec3::Y * 0.105;
        // Nothing happened: hunger.
        let (feast, memory) = scene_of_the_crime();
        assert_eq!(verdict(&feast, &memory, false, false), Cause::Slump);
        // A mustard round that was in flight a frame ago and has splattered on the chest.
        for (weapon, speed) in [(1, 1.5), (2, 4.5), (3, 1.1)] {
            let (mut feast, mut memory) = scene_of_the_crime();
            let along = Vec3::new(0.8, -0.1, 0.59).normalize();
            memory.rounds.push((7, along * speed));
            let hit = chest - along * (0.055 + feast::ROUND_RADIUS[weapon]);
            feast.splats.push(Splat { id: 3, projectile: 7, weapon, p: hit.to_array(), time: 40. });
            let cause = verdict(&feast, &memory, false, false);
            match (weapon, cause) {
                (1, Cause::Shot { along: seen, hard: false }) | (2, Cause::Shot { along: seen, hard: true }) | (3, Cause::Sauce { along: seen }) => {
                    assert!(seen.distance(along) < 1e-5)
                }
                _ => panic!("weapon {weapon} was read as {cause:?}"),
            }
            // Online a round may never have been seen in flight: the splat's place on the body gives the direction.
            memory.rounds.clear();
            let (Cause::Shot { along: seen, .. } | Cause::Sauce { along: seen }) = verdict(&feast, &memory, false, false) else {
                panic!("weapon {weapon} unseen")
            };
            assert!(seen.distance(along) < 1e-3);
            // The same splat a frame later is old news, and one that landed elsewhere is not this death.
            memory.splat = 3;
            assert_eq!(verdict(&feast, &memory, false, false), Cause::Slump);
            memory.splat = 0;
            feast.splats[0].p = (chest + Vec3::X * 0.3).to_array();
            assert_eq!(verdict(&feast, &memory, false, false), Cause::Slump);
        }
        // The relish glob kills with its splash as well as a direct hit, from where it burst.
        for gap in [0.085, 0.2] {
            let (mut feast, memory) = scene_of_the_crime();
            let burst = chest + Vec3::new(-gap, 0., 0.);
            feast.splats.push(Splat { id: 1, projectile: 9, weapon: 4, p: burst.to_array(), time: 40. });
            assert_eq!(verdict(&feast, &memory, false, false), Cause::Blast { from: burst });
        }
        let (mut feast, memory) = scene_of_the_crime();
        feast.splats.push(Splat { id: 1, projectile: 9, weapon: 4, p: (chest + Vec3::X * 0.4).to_array(), time: 40. });
        assert_eq!(verdict(&feast, &memory, false, false), Cause::Slump, "out of the splash");
        // Bug spray never hurts a hotdog.
        let (mut feast, memory) = scene_of_the_crime();
        feast.splats.push(Splat { id: 1, projectile: 9, weapon: 5, p: chest.to_array(), time: 40. });
        assert_eq!(verdict(&feast, &memory, false, false), Cause::Slump);
    }
    #[test]
    fn the_simulations_record_of_what_killed_it_is_believed_and_inference_is_only_for_a_blank_one() {
        use feast::Damage;
        let (mut feast, memory) = scene_of_the_crime();
        let chest = OPEN + Vec3::Y * 0.105;
        let (from, along) = (OPEN + Vec3::new(-0.4, 0.135, 0.3), Vec3::new(0.8, -0.1, -0.59).normalize());
        let record = |kind: u8| Damage { kind, attacker: 3, p: from.to_array(), d: along.to_array(), time: 40., amount: 10. };
        // Misleading evidence all round: a relish splat on the chest and a neighbour who has just eaten.
        feast.splats.push(Splat { id: 1, projectile: 9, weapon: 4, p: chest.to_array(), time: 40. });
        feast.players[3].p = (OPEN + Vec3::X * 0.1).to_array();
        feast.players[3].meals = 1;
        for (kind, cause) in [
            (1, Cause::Shot { along, hard: false }),
            (2, Cause::Shot { along, hard: true }),
            (3, Cause::Sauce { along }),
            (4, Cause::Blast { from }),
            (feast::MELEE, Cause::Bite { from }),
            (feast::BACKSTAB, Cause::Bite { from }),
            (feast::TURRET, Cause::Turret { along }),
            (feast::ZOMBIE, Cause::Zombie { from: Some(from) }),
            (feast::PESTS, Cause::Slump),
            (feast::HUNGER, Cause::Slump),
            (feast::FALL, Cause::Slump),
        ] {
            feast.players[1].last_damage = record(kind);
            assert_eq!(verdict(&feast, &memory, false, false), cause, "kind {kind}");
            assert_eq!(verdict(&feast, &memory, true, true), cause, "kind {kind}, online during the wave");
        }
        // A record with no direction is read as coming straight from its source.
        feast.players[1].last_damage = Damage { d: [0.; 3], ..record(2) };
        let Cause::Shot { along: seen, hard: true } = verdict(&feast, &memory, false, false) else { panic!("not a shot") };
        assert!(seen.distance((chest - from).normalize()) < 1e-5);
        // A blank record (a snapshot from before it existed): the evidence decides, as it always did.
        feast.players[1].last_damage = Damage::default();
        assert_eq!(verdict(&feast, &memory, false, false), Cause::Blast { from: chest });
        // A real death: shot dead with mustard, seen only a second later, when the splat is long gone and
        // inference would have said hunger. The record still says who, from where and which way.
        let mut s = Feast::new(false);
        s.stage = feast::Stage::FreeForAll;
        for (i, p) in s.players.iter_mut().enumerate() {
            (p.bot, p.p) = (false, [-1.5 + i as f32 * 0.1, FLOOR, 3.55]);
        }
        (s.players[0].p, s.players[1].p, s.players[1].hp) = ([0., FLOOR, 2.4], [0., FLOOR, 2.7], 5.);
        assert!(s.fire(1, Vec3::Z, 1));
        for _ in 0..30 {
            s.tick(1. / 30.);
        }
        assert!(!s.players[1].alive && s.splats.is_empty());
        let view = View { roster: &s.players, feast: &s, elapsed: s.elapsed, wave: false, online: false };
        assert_eq!(diagnose(&s.players[1], &view, &Memory::default()), Cause::Shot { along: Vec3::Z, hard: false });
        assert_eq!((s.players[1].last_damage.kind, s.players[1].last_damage.attacker), (1, 1));
    }
    #[test]
    fn a_bite_a_beam_a_zombie_and_a_swarm_are_each_recognised() {
        // The neighbour's meal count has just gone up.
        let (mut feast, memory) = scene_of_the_crime();
        let biter = OPEN + Vec3::new(0.1, 0., 0.08);
        feast.players[3].p = biter.to_array();
        feast.players[3].meals = 1;
        assert_eq!(verdict(&feast, &memory, false, false), Cause::Bite { from: biter });
        feast.players[3].p = (OPEN + Vec3::X * 0.5).to_array();
        assert_eq!(verdict(&feast, &memory, false, false), Cause::Slump, "a meal eaten elsewhere");
        // Standing in the turret's beam as it pulses. Online the pulse is not kept, so the beam is enough.
        let (mut feast, memory) = scene_of_the_crime();
        let (origin, along) = feast.turrets[0];
        feast.players[1].p = (origin + along * 0.6 - Vec3::Y * 0.08).to_array();
        assert_eq!(verdict(&feast, &memory, false, false), Cause::Slump, "no pulse this frame");
        assert_eq!(verdict(&feast, &memory, false, true), Cause::Turret { along });
        feast.shots.push(feast::Shot { id: 1, owner: 0, a: origin.to_array(), b: (origin + along).to_array(), time: 40., weapon: 0, projectile: 0 });
        assert_eq!(verdict(&feast, &memory, false, false), Cause::Turret { along });
        // During the wave a zombie at its feet; online, where zombies are not known here, still a zombie.
        let (mut feast, memory) = scene_of_the_crime();
        let zombie = OPEN + Vec3::new(-0.06, 0., 0.03);
        feast.enemies.push(Zombie { id: 1, p: zombie.to_array(), yaw: 0., hp: 70., born: 20., forming: 0. });
        assert_eq!(verdict(&feast, &memory, true, false), Cause::Zombie { from: Some(zombie) });
        feast.enemies.clear();
        assert_eq!(verdict(&feast, &memory, true, true), Cause::Zombie { from: None });
        // A swarm over it, in the wave or after.
        feast.pests.push(Swarm { id: 1, p: OPEN.to_array(), hp: 100., born: 30., empty: 0. });
        assert_eq!(verdict(&feast, &memory, true, true), Cause::Slump);
        assert_eq!(verdict(&feast, &memory, false, false), Cause::Slump);
    }
    #[test]
    fn the_scenery_is_the_map() {
        let scene = scenery(&Map::bundled());
        // Counter, kitchen floor and cupboard, four blocks, the deck and the turret; two ramps.
        assert_eq!((scene.blocks.len(), scene.wedges.len()), (3 + 4 + 1 + 1, 2));
        assert_eq!(scene.ground(OPEN + Vec3::Y), Some(FLOOR));
        // The counter top reaches 0.02 m past the playable area and then there is only the floor.
        assert_eq!(scene.ground(Vec3::new(0., 2., 2.26)), Some(FLOOR));
        assert_eq!(scene.ground(Vec3::new(0., 2., 2.24)), Some(0.));
        assert_eq!(scene.ground(Vec3::new(-0.34, 2., 2.81)), Some(FLOOR + 0.25), "on the steel block");
        assert_eq!(scene.ground(Vec3::new(1.4, 2., 3.3)), Some(FLOOR + 0.28), "on the deck");
        assert_eq!(scene.ground(Vec3::new(1.4, FLOOR + 0.1, 3.3)), Some(FLOOR), "under the deck");
        let half = scene.ground(Vec3::new(0.9, 2., 3.53)).unwrap();
        assert!((half - (FLOOR + 0.14)).abs() < 1e-4, "half way up the ramp: {half}");
        let level = kitchen_sim::map::Level::new(Map::bundled());
        for (x, z) in [(0.2, 3.0), (0.7, 3.5), (1.1, 3.55), (1.65, 2.8), (1.5, 3.4), (-1.7, 2.3)] {
            assert!((scene.ground(Vec3::new(x, 2., z)).unwrap() - level.ground_at(x, z)).abs() < 1e-4, "the ragdoll's ground is the simulation's at {x} {z}");
        }
    }

    /// What `arena` knows, for a test without the whole game.
    #[derive(Resource)]
    struct Stage {
        feast: Feast,
        wave: bool,
    }
    fn stage_tick(
        stage: Res<Stage>,
        models: Res<PlayerModels>,
        mut deaths: ResMut<Deaths>,
        hotdogs: Query<(Entity, &Rig, &Gait), With<PlayerBody>>,
        mut transforms: Query<&mut Transform>,
        mut commands: Commands,
    ) {
        if deaths.scene.blocks.is_empty() {
            deaths.scene = scenery(&stage.feast.level.map);
        }
        let view = View { roster: &stage.feast.players, feast: &stage.feast, elapsed: stage.feast.elapsed, wave: stage.wave, online: false };
        deaths.update(&view, 1. / 60., &hotdogs, &mut transforms, &mut commands, Some(&models.weapons));
    }
    /// A world with the real hotdog models in it and one actor per player, as `arena` keeps them.
    fn game() -> (App, Vec<Entity>) {
        let mut app = App::new();
        app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>();
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let models = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|_, mut materials: Mut<Assets<StandardMaterial>>| PlayerModels::new(&mut meshes, &mut materials, &server, false))
        });
        let mut feast = Feast::new(true);
        feast.stage = feast::Stage::FreeForAll;
        feast.elapsed = 30.;
        let actors = feast.players.iter().map(|p| models.spawn(world, Vec3::from_array(p.p), p.id)).collect();
        world.insert_resource(models);
        world.insert_resource(Stage { feast, wave: false });
        app.init_resource::<Deaths>().add_systems(Update, stage_tick);
        (app, actors)
    }
    /// One frame as the game runs it: `arena::update_players` places and animates every hotdog,
    /// then the death system has the last word.
    fn frame(app: &mut App, actors: &[Entity], time: f32) {
        let players = app.world().resource::<Stage>().feast.players.clone();
        for (player, actor) in players.iter().zip(actors) {
            let world = app.world_mut();
            let mut tr = world.get_mut::<Transform>(*actor).unwrap();
            tr.translation = Vec3::from_array(player.p) + if player.alive { Vec3::Y * (time * 4. + player.id as f32).sin() * 0.001 } else { Vec3::Y * 0.025 };
            tr.rotation = Quat::from_rotation_y(player.yaw) * if player.alive { Quat::IDENTITY } else { Quat::from_rotation_z(1.57) };
            world.entity_mut(*actor).insert(if player.consumed { Visibility::Hidden } else { Visibility::Visible });
            let cues = Cues { alive: player.alive, weapon: player.weapon, ..default() };
            player_model::animate(world, *actor, time, cues);
        }
        app.world_mut().resource_mut::<Stage>().feast.elapsed += 1. / 60.;
        app.update();
    }
    /// Every transform under a hotdog, in a fixed order.
    fn rig_of(world: &World, root: Entity) -> Vec<Transform> {
        let mut all = vec![];
        let mut queue = vec![root];
        while let Some(entity) = queue.pop() {
            all.push(*world.get::<Transform>(entity).unwrap());
            if let Some(children) = world.get::<Children>(entity) {
                queue.extend(children.iter());
            }
        }
        all
    }
    fn dropped(app: &mut App) -> Vec<Transform> {
        let world = app.world_mut();
        world.query_filtered::<&Transform, With<DroppedWeapon>>().iter(world).copied().collect()
    }
    #[test]
    fn a_death_hands_the_rig_to_the_ragdoll_and_a_reset_gives_back_the_living_rig_exactly() {
        let (mut app, actors) = game();
        // Hotdogs 2 and 3 have the mustard pistol in hand, whose grip is one of the spheres it lies on when dropped
        // (the_weapon_leaves_the_hand_falls_bounces_and_lies_on_the_counter drops every gun).
        for p in &mut app.world_mut().resource_mut::<Stage>().feast.players[1..3] {
            (p.weapon, p.gun) = (1, 1);
        }
        let mut time = 1.;
        let mut run = |app: &mut App, frames: usize| {
            for _ in 0..frames {
                time += 1. / 60.;
                frame(app, &actors, time);
            }
        };
        run(&mut app, 30);
        assert_eq!(app.world().resource::<Deaths>().count(), (0, 0), "nobody has died");
        assert!(dropped(&mut app).is_empty());
        // Hotdogs 2 and 3 stand in the same pose with the same history: 3 is the control.
        let (victim, twin) = (actors[1], actors[2]);
        let joints = app.world().get::<Rig>(victim).unwrap().joints();
        let living = *app.world().get::<Transform>(joints.torso).unwrap();
        // Hotdog 2 starves.
        app.world_mut().resource_mut::<Stage>().feast.players[1].alive = false;
        run(&mut app, 1);
        assert_eq!(app.world().resource::<Deaths>().count(), (1, 1));
        let home = Vec3::from_array(app.world().resource::<Stage>().feast.players[1].p);
        // On the frame it dies it has not moved: the root stands upright where the simulation has
        // the body, not laid on its side, and the torso is where it was a frame before.
        let root = *app.world().get::<Transform>(victim).unwrap();
        assert!(root.translation.distance(home) < 1e-6 && root.rotation.angle_between(Quat::from_rotation_y(0.9)) < 1e-4);
        let torso = *app.world().get::<Transform>(joints.torso).unwrap();
        assert!(torso.translation.distance(living.translation) < 0.002 && torso.rotation.angle_between(living.rotation) < 0.05, "{torso:?} from {living:?}");
        // Its weapon is on its way to the counter, and the one in its hand is hidden.
        assert_eq!(dropped(&mut app).len(), 1);
        let hand = dropped(&mut app)[0].translation;
        assert!(hand.y > FLOOR + 0.05 && hand.distance(home) < 0.12, "the weapon starts in the hand: {}", hand - home);
        run(&mut app, 420);
        assert_eq!(app.world().resource::<Deaths>().count(), (1, 0), "the ragdoll has come to rest and sleeps");
        let world = app.world();
        let lying = world.get::<Transform>(joints.torso).unwrap();
        let head = world.get::<Transform>(victim).unwrap().transform_point(lying.transform_point(Vec3::Y * 0.17));
        assert!(head.y < FLOOR + 0.06 && head.y > FLOOR + 0.01, "it lies on the counter: head at {head}");
        assert!(Vec2::new(head.x - home.x, head.z - home.z).length() < 0.2, "beside where it died");
        let weapon = dropped(&mut app)[0].translation;
        assert!(weapon.y < FLOOR + 0.03 && weapon.y > FLOOR, "the weapon lies on the counter: {weapon}");
        // Eaten: the game hides the body, as it always has, and the ragdoll leaves that alone.
        app.world_mut().resource_mut::<Stage>().feast.players[1].consumed = true;
        run(&mut app, 2);
        assert_eq!(*app.world().get::<Visibility>(victim).unwrap(), Visibility::Hidden);
        app.world_mut().resource_mut::<Stage>().feast.players[1].consumed = false;
        // A reset: alive again on the same entity. The very next frame the rig is the living one,
        // bit for bit the same as its twin's, and nothing of the death is left.
        app.world_mut().resource_mut::<Stage>().feast.players[1].alive = true;
        run(&mut app, 1);
        assert_eq!(app.world().resource::<Deaths>().count(), (0, 0));
        assert!(dropped(&mut app).is_empty(), "the dropped weapon is gone");
        let (a, b) = (rig_of(app.world(), victim), rig_of(app.world(), twin));
        assert_eq!(a.len(), b.len());
        let mut compared = 0;
        for (a, b) in a.iter().zip(&b).skip(1) {
            // Everything below the root: joints, bones, hand, weapons. (Sparks are scattered by
            // entity, and the roots stand in different places.)
            if (a.scale.x - 0.0016).abs() > 1e-6 {
                assert_eq!(a, b, "the rig is not restored exactly");
                compared += 1;
            }
        }
        assert!(compared > 30, "compared {compared} transforms");
        run(&mut app, 60);
        assert_eq!(rig_of(app.world(), victim).len(), a.len(), "no entity was added to or left on the hotdog");
        assert_eq!(app.world().resource::<Deaths>().count(), (0, 0));
    }
    #[test]
    fn a_new_round_takes_every_body_and_dropped_weapon_with_it() {
        let (mut app, actors) = game();
        for i in 0..20 {
            frame(&mut app, &actors, 1. + i as f32 / 60.);
        }
        for i in [0, 2, 3] {
            app.world_mut().resource_mut::<Stage>().feast.players[i].alive = false;
        }
        for i in 0..90 {
            frame(&mut app, &actors, 2. + i as f32 / 60.);
        }
        assert_eq!(app.world().resource::<Deaths>().count().0, 3);
        assert_eq!(dropped(&mut app).len(), 3);
        // `arena::clear_round` despawns the actors and spawns new ones.
        for actor in &actors {
            app.world_mut().entity_mut(*actor).despawn();
        }
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Deaths>().count(), (0, 0));
        assert!(dropped(&mut app).is_empty());
        assert!(app.world().resource::<Deaths>().watch.is_empty());
    }
    #[test]
    fn a_body_already_lying_there_when_first_seen_is_shown_at_rest() {
        let (mut app, actors) = game();
        app.world_mut().resource_mut::<Stage>().feast.players[4].alive = false;
        frame(&mut app, &actors, 1.);
        // Joining a round late: no reaction is played and nothing is dropped; the body is just there.
        assert_eq!(app.world().resource::<Deaths>().count(), (1, 0));
        assert!(dropped(&mut app).is_empty());
        let joints = app.world().get::<Rig>(actors[4]).unwrap().joints();
        let lying = app.world().get::<Transform>(joints.torso).unwrap();
        assert!((lying.rotation * Vec3::Y).y.abs() < 0.3, "lying down");
        // One that was eaten before it was ever seen gets no ragdoll at all.
        let (mut app, actors) = game();
        let ghost = &mut app.world_mut().resource_mut::<Stage>().feast.players[4];
        (ghost.alive, ghost.consumed) = (false, true);
        frame(&mut app, &actors, 1.);
        assert_eq!(app.world().resource::<Deaths>().count(), (0, 0));
    }
    #[test]
    fn the_dropped_weapon_shrinks_away_after_a_while() {
        let (mut app, actors) = game();
        frame(&mut app, &actors, 1.);
        app.world_mut().resource_mut::<Stage>().feast.players[0].alive = false;
        let frames = ((KEEP + 0.25) * 60.) as usize;
        for i in 0..frames {
            frame(&mut app, &actors, 1. + i as f32 / 60.);
        }
        let weapon = dropped(&mut app);
        assert!(weapon.len() == 1 && weapon[0].scale.x < 0.9 && weapon[0].scale.x > 0.1, "half gone: {:?}", weapon.first().map(|t| t.scale));
        for i in 0..40 {
            frame(&mut app, &actors, 20. + i as f32 / 60.);
        }
        assert!(dropped(&mut app).is_empty());
        assert_eq!(app.world().resource::<Deaths>().count(), (1, 0), "the body stays");
    }
    #[test]
    fn five_ragdolls_at_once_cost_next_to_nothing_and_allocate_nothing() {
        let scene = scenery(&Map::bundled());
        let homes = [0.2, 0.5, 0.8, -0.2, -0.6].map(|x| Vec3::new(x, FLOOR, 3.0));
        let mut falls: Vec<Fall> = causes().iter().take(5).zip(homes).map(|((_, cause), home)| {
            let cause = match *cause {
                Cause::Blast { from } => Cause::Blast { from: from - OPEN + home },
                Cause::Bite { from } => Cause::Bite { from: from - OPEN + home },
                other => other,
            };
            Fall::new(home, 0.4, &standing(), 0., Vec3::ZERO, cause, 3, &scene)
        }).collect();
        let pose = standing();
        let mut weapons: Vec<Ragdoll> = falls.iter().zip(homes).enumerate().map(|(i, (fall, home))| let_go(i, Transform::from_translation(home), &pose, &fall.doll, 3).0).collect();
        let tune = weapon_tuning();
        let step = |falls: &mut [Fall], weapons: &mut [Ragdoll]| {
            for ((fall, weapon), home) in falls.iter_mut().zip(weapons.iter_mut()).zip(homes) {
                fall.advance(1. / 60., home, &scene);
                std::hint::black_box(shown(&fall.doll, Transform::from_translation(home)));
                weapon.advance(1. / 60., &scene, &tune);
            }
        };
        // The first frame may size a buffer; after that a frame of five ragdolls and five weapons
        // in full flight touches the allocator not once.
        step(&mut falls, &mut weapons);
        let before = crate::ragdoll::allocations();
        let started = std::time::Instant::now();
        let mut frames = 0;
        while falls.iter().any(|fall| !fall.doll.asleep()) && frames < 600 {
            step(&mut falls, &mut weapons);
            frames += 1;
        }
        let spent = started.elapsed().as_secs_f64() * 1e6;
        assert_eq!(crate::ragdoll::allocations(), before, "a frame of ragdolls allocated");
        assert!(frames > 60 && frames < 600, "they all came to rest, after {frames} frames");
        println!("five ragdolls and five weapons: {:.1} microseconds a frame over {frames} frames until the last fell asleep", spent / frames as f64);
        // A frame is 16,667 microseconds. Generous, so a loaded machine does not fail it.
        assert!(spent / (frames as f64) < 1500. * kitchen_sim::timing_slack(), "{} microseconds a frame", spent / frames as f64);
        // Asleep, they cost nothing at all.
        let started = std::time::Instant::now();
        for _ in 0..1000 {
            step(&mut falls, &mut weapons);
        }
        let asleep = started.elapsed().as_secs_f64() * 1e6 / 1000.;
        println!("asleep: {asleep:.2} microseconds a frame");
        assert!(weapons.iter().all(|w| w.asleep()) && asleep < 50. * kitchen_sim::timing_slack());
    }
}

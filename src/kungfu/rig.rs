// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The kung fu parts of a hotdog (its hands' shapes, its feet, a face that can shout) and how a solved pose is put on
//! player_model's rig, frame by frame, for every hotdog in a Kung Fu round.
use super::*;
use crate::player_model::{Gait, PlayerBody};
use crate::viewmodel::Shape;
use std::collections::HashMap;

/// The meshes of the hands' shapes (`Hand::ALL`, the left hand's then the right's) and of the feet.
#[derive(Clone)]
pub struct Look {
    pub hands: [Vec<Handle<Mesh>>; 2],
    pub feet: [Handle<Mesh>; 2],
    // KF5: a cheek blown out (`Face::puff`): a ball of the sausage's own skin under each blush.
    pub pouch: Handle<Mesh>,
}
/// How much bigger a fighter's hands and feet are than life, and how much thicker its limbs: cartoon mitts and
/// sausage-link arms, so that a blow reads from across the ring.
pub const MITT: f32 = 1.35;
pub const THICK: f32 = 1.3;
/// One hand's shape, in its own frame: the wrist at the origin, the fingers along -y, the palm facing -z, the thumb
/// towards +x (a right hand; the left is its mirror image). A hotdog's forearm is 0.004 m thick (`THICK` times that in
/// Kung Fu), its hand about 0.010 across before `MITT`.
pub fn hand_shape(hand: Hand) -> Shape {
    let mut s = hand_shape_raw(hand);
    for p in s.positions.iter_mut() {
        *p *= MITT;
    }
    s
}
fn hand_shape_raw(hand: Hand) -> Shape {
    let mut s = Shape::default();
    let along = |points: &[(f32, f32, f32, f32, f32)]| points.iter().map(|&(x, y, z, w, h)| (v(x, y, z), w, h)).collect::<Vec<_>>();
    let palm = |s: &mut Shape, depth: f32| s.blob(v(0., -0.0046, 0.), v(0.0047, 0.0052, depth), Quat::IDENTITY);
    let thumb = |s: &mut Shape, points: &[(f32, f32, f32)]| {
        let n = points.len() as f32;
        let tube: Vec<(Vec3, f32, f32)> = points.iter().enumerate().map(|(i, &(x, y, z))| (v(x, y, z), 0.0016 - 0.0004 * i as f32 / n, 0.0015 - 0.0004 * i as f32 / n)).collect();
        s.tube(&tube, Vec3::Z);
    };
    match hand {
        Hand::Relaxed => {
            palm(&mut s, 0.0026);
            s.tube(&along(&[(0., -0.0080, 0., 0.0042, 0.0016), (0., -0.0118, -0.0010, 0.0040, 0.0015), (0., -0.0138, -0.0032, 0.0036, 0.0013)]), Vec3::Z);
            thumb(&mut s, &[(0.0040, -0.0030, -0.0010), (0.0052, -0.0062, -0.0022), (0.0046, -0.0082, -0.0030)]);
        }
        Hand::Fist => {
            s.blob(v(0., -0.0052, -0.0006), v(0.0051, 0.0055, 0.0043), Quat::IDENTITY);
            // The curled fingers: a roll across the front of the fist, knuckles proud.
            s.tube(&along(&[(-0.0044, -0.0088, -0.0020, 0.0024, 0.0026), (0., -0.0094, -0.0024, 0.0026, 0.0028), (0.0044, -0.0088, -0.0020, 0.0024, 0.0026)]), Vec3::Z);
            thumb(&mut s, &[(0.0048, -0.0026, -0.0016), (0.0040, -0.0058, -0.0046), (0.0008, -0.0072, -0.0054)]);
        }
        Hand::Palm => {
            palm(&mut s, 0.0024);
            for x in [-0.0033f32, -0.0011, 0.0011, 0.0033] {
                let tip = -0.0150 + 0.0012 * (x.abs() > 0.002) as u8 as f32;
                s.tube(&along(&[(x, -0.0080, 0., 0.0012, 0.0012), (x, -0.0115, 0., 0.0011, 0.0011), (x, tip, 0., 0.0010, 0.0010)]), Vec3::Z);
            }
            thumb(&mut s, &[(0.0042, -0.0024, -0.0006), (0.0058, -0.0058, -0.0010), (0.0060, -0.0082, -0.0008)]);
        }
        Hand::Claw => {
            palm(&mut s, 0.0026);
            for x in [-0.0036f32, -0.0012, 0.0012, 0.0036] {
                s.tube(&along(&[(x, -0.0080, 0., 0.0012, 0.0012), (x * 1.35, -0.0118, -0.0010, 0.0011, 0.0011), (x * 1.55, -0.0132, -0.0040, 0.0010, 0.0010), (x * 1.55, -0.0124, -0.0062, 0.0007, 0.0007)]), Vec3::Z);
            }
            thumb(&mut s, &[(0.0042, -0.0030, -0.0010), (0.0070, -0.0058, -0.0028), (0.0072, -0.0072, -0.0054)]);
        }
        Hand::Beak => {
            // All the fingertips and the thumb pinched to one point.
            s.tube(&along(&[(0., -0.0018, 0., 0.0044, 0.0028), (0., -0.0062, -0.0006, 0.0041, 0.0031), (0., -0.0108, -0.0014, 0.0026, 0.0021), (0., -0.0148, -0.0018, 0.0008, 0.0008)]), Vec3::Z);
        }
        Hand::Jab => {
            s.blob(v(0., -0.0046, 0.), v(0.0046, 0.0052, 0.0022), Quat::IDENTITY);
            // The fingers straight and together, flat, tapering to a spear point.
            s.tube(&along(&[(0., -0.0072, 0., 0.0043, 0.0014), (0., -0.0128, 0., 0.0037, 0.0012), (0., -0.0172, 0., 0.0016, 0.0009)]), Vec3::Z);
            thumb(&mut s, &[(0.0042, -0.0030, -0.0012), (0.0034, -0.0062, -0.0024), (0.0020, -0.0080, -0.0024)]);
        }
        Hand::Cup => {
            palm(&mut s, 0.0026);
            // The fingers together, curled into a C round a cup that is not there; the thumb across from them.
            s.tube(&along(&[(0., -0.0078, 0., 0.0042, 0.0015), (0., -0.0116, -0.0022, 0.0041, 0.0015), (0., -0.0122, -0.0062, 0.0039, 0.0014), (0., -0.0104, -0.0090, 0.0035, 0.0012)]), Vec3::Z);
            thumb(&mut s, &[(0.0044, -0.0030, -0.0012), (0.0046, -0.0046, -0.0050), (0.0030, -0.0052, -0.0084)]);
        }
        Hand::Hook => {
            palm(&mut s, 0.0026);
            // The fingers bunched and hooked over towards the palm at their middles.
            s.tube(&along(&[(0., -0.0078, 0., 0.0042, 0.0016), (0., -0.0112, -0.0010, 0.0041, 0.0016), (0., -0.0122, -0.0044, 0.0039, 0.0015), (0., -0.0108, -0.0068, 0.0034, 0.0013)]), Vec3::Z);
            thumb(&mut s, &[(0.0044, -0.0034, -0.0010), (0.0044, -0.0072, -0.0034), (0.0030, -0.0088, -0.0052)]);
        }
    }
    s
}
/// A foot in its own frame: the ankle at the origin, the toes along +z, the sole under -y.
pub fn foot_shape() -> Shape {
    let mut s = Shape::default();
    s.blob(v(0., 0.0012, 0.0028) * MITT, v(0.0043, 0.0027, 0.0072) * MITT, Quat::IDENTITY);
    s
}
impl Look {
    pub fn new(meshes: &mut Assets<Mesh>) -> Look {
        let right: Vec<Shape> = Hand::ALL.iter().map(|h| hand_shape(*h)).collect();
        let hands = [right.iter().cloned().map(|s| meshes.add(s.mirrored().mesh())).collect(), right.iter().map(|s| meshes.add(s.mesh())).collect()];
        let foot = foot_shape();
        Look { hands, feet: [meshes.add(foot.clone().mirrored().mesh()), meshes.add(foot.mesh())], pouch: meshes.add(Sphere::new(1.).mesh().uv(16, 10)) }
    }
}

/// The entities of one hotdog's rig that Kung Fu poses: player_model's joints and bones, and the kung fu parts hung on
/// them (hidden outside Kung Fu).
#[derive(Component, Clone)]
pub struct Parts {
    pub torso: Entity,
    pub hips: [Entity; 2],
    pub knees: [Entity; 2],
    pub shoulders: [Entity; 2],
    pub elbows: [Entity; 2],
    pub arm_bones: [[Entity; 2]; 2],
    pub leg_bones: [[Entity; 2]; 2],
    /// The gun hand (it carries the weapons), hidden in Kung Fu.
    pub gun: Entity,
    /// Each wrist, and the hand shapes hung on it (`Hand::ALL`).
    pub wrists: [Entity; 2],
    pub shapes: [[Entity; 8]; 2],
    pub feet: [Entity; 2],
    /// The face and the body's soft parts: the brows, the eyes (each: the white, the pupil, the glint), the inside of
    /// the mouth, the buns and the mustard; and how each eye part and bun sits at rest.
    pub brows: [Entity; 2],
    pub eyes: [[Entity; 3]; 2],
    pub cheeks: [Entity; 2],
    // KF5: the cheek pouches (`Look::pouch`), hidden until the cheeks puff.
    pub pouches: [Entity; 2],
    pub gape: Entity,
    pub buns: [Entity; 2],
    pub mustard: Entity,
    pub rest_eyes: [[Transform; 3]; 2],
    pub rest_buns: [Transform; 2],
}
/// The joints and parts player_model's `spawn` made, for `attach`.
pub struct Made {
    pub torso: Entity,
    pub hips: [Entity; 2],
    pub knees: [Entity; 2],
    pub shoulders: [Entity; 2],
    pub elbows: [Entity; 2],
    pub arm_bones: [[Entity; 2]; 2],
    pub leg_bones: [[Entity; 2]; 2],
    pub gun: Entity,
    pub brows: [Entity; 2],
    pub eyes: [[Entity; 3]; 2],
    pub cheeks: [Entity; 2],
    pub gape: Entity,
    pub buns: [Entity; 2],
    pub mustard: Entity,
}
/// Hangs a hand on each wrist and a foot on each shin of a hotdog player_model has just made, hidden, and gives its
/// root the `Parts`. The feet wear `colour` (the legs'). Returns the feet, which wear the livery with the legs.
pub fn attach(world: &mut World, root: Entity, look: &Look, made: Made, skin: &Handle<StandardMaterial>, colour: &Handle<StandardMaterial>) -> [Entity; 2] {
    let below = Vec3::NEG_Y * BONE;
    let mut wrists = [Entity::PLACEHOLDER; 2];
    let mut shapes = [[Entity::PLACEHOLDER; 8]; 2];
    let mut feet = [Entity::PLACEHOLDER; 2];
    for i in [L, R] {
        let wrist = world.spawn((Transform::from_translation(below), Visibility::Hidden)).id();
        world.entity_mut(made.elbows[i]).add_child(wrist);
        for (k, mesh) in look.hands[i].iter().enumerate() {
            let shape = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(skin.clone()), Transform::default(), Visibility::Hidden)).id();
            world.entity_mut(wrist).add_child(shape);
            shapes[i][k] = shape;
        }
        wrists[i] = wrist;
        let foot = world.spawn((Mesh3d(look.feet[i].clone()), MeshMaterial3d(colour.clone()), Transform::from_translation(below), Visibility::Hidden)).id();
        world.entity_mut(made.knees[i]).add_child(foot);
        feet[i] = foot;
    }
    // KF5: the cheek pouches, on the torso under the blush.
    let pouches = [L, R].map(|_| {
        let pouch = world.spawn((Mesh3d(look.pouch.clone()), MeshMaterial3d(skin.clone()), Transform::from_scale(Vec3::splat(0.001)), Visibility::Hidden, bevy::light::NotShadowCaster)).id();
        world.entity_mut(made.torso).add_child(pouch);
        pouch
    });
    let rest = |world: &World, e: Entity| world.get::<Transform>(e).copied().unwrap_or_default();
    let rest_eyes = made.eyes.map(|eye| eye.map(|e| rest(world, e)));
    let rest_buns = made.buns.map(|e| rest(world, e));
    world.entity_mut(root).insert(Parts {
        torso: made.torso,
        hips: made.hips,
        knees: made.knees,
        shoulders: made.shoulders,
        elbows: made.elbows,
        arm_bones: made.arm_bones,
        leg_bones: made.leg_bones,
        gun: made.gun,
        wrists,
        shapes,
        feet,
        brows: made.brows,
        eyes: made.eyes,
        cheeks: made.cheeks,
        pouches,
        gape: made.gape,
        buns: made.buns,
        mustard: made.mustard,
        rest_eyes,
        rest_buns,
    });
    feet
}

fn place(world: &mut World, e: Entity, at: Option<Vec3>, turn: Option<Quat>, scale: Option<Vec3>) {
    if let Some(mut tr) = world.get_mut::<Transform>(e) {
        if let Some(at) = at {
            tr.translation = at;
        }
        if let Some(turn) = turn {
            tr.rotation = turn;
        }
        if let Some(scale) = scale {
            tr.scale = scale;
        }
    }
}
fn shown(world: &mut World, e: Entity, show: bool) {
    let want = if show { Visibility::Inherited } else { Visibility::Hidden };
    if world.get::<Visibility>(e).is_some_and(|v| *v != want) {
        world.entity_mut(e).insert(want);
    }
}
/// Poses a hotdog's rig as `s` says, with its kung fu hands and feet showing, its face as the pose's and its buns and
/// mustard `jiggle`d (an offset in the torso's frame).
pub fn show(world: &mut World, parts: &Parts, s: &Solved, jiggle: Vec3) {
    let (at, turn, scale) = s.torso_place();
    place(world, parts.torso, Some(at), Some(turn), Some(scale));
    for i in [L, R] {
        let (at, turn) = s.hip_place(i);
        place(world, parts.hips[i], Some(at), Some(turn), None);
        place(world, parts.knees[i], Some(Vec3::NEG_Y * BONE * s.stride[i]), Some(s.knees[i]), None);
        place(world, parts.shoulders[i], None, Some(s.shoulders[i]), None);
        place(world, parts.elbows[i], Some(Vec3::NEG_Y * BONE * s.reach[i]), Some(s.elbows[i]), None);
        for bone in parts.arm_bones[i] {
            place(world, bone, Some(Vec3::NEG_Y * BONE * 0.5 * s.reach[i]), None, Some(v(THICK, s.reach[i], THICK)));
        }
        for bone in parts.leg_bones[i] {
            place(world, bone, Some(Vec3::NEG_Y * BONE * 0.5 * s.stride[i]), None, Some(v(THICK, s.stride[i], THICK)));
        }
        place(world, parts.wrists[i], Some(Vec3::NEG_Y * BONE * s.reach[i]), Some(s.wrists[i]), None);
        shown(world, parts.wrists[i], true);
        for (k, shape) in parts.shapes[i].iter().enumerate() {
            shown(world, *shape, Hand::ALL[k] == s.hands[i]);
        }
        place(world, parts.feet[i], Some(Vec3::NEG_Y * BONE * s.stride[i]), Some(s.ankles[i]), None);
        shown(world, parts.feet[i], true);
    }
    shown(world, parts.gun, false);
    face(world, parts, &s.face);
    // The buns and the mustard lag behind the body and wobble back.
    for (i, bun) in parts.buns.iter().enumerate() {
        let rest = parts.rest_buns[i];
        let wobble = 1. + (jiggle.length() * 9.).min(0.08);
        place(world, *bun, Some(rest.translation + jiggle * 0.55), None, Some(rest.scale * v(wobble, 1. / wobble, wobble)));
    }
    place(world, parts.mustard, Some(jiggle * 0.85), Some(Quat::from_rotation_z(-jiggle.x * 6.) * Quat::from_rotation_x(jiggle.z * 6.)), None);
}
/// The face: the brows slanted and raised, the eyes narrowed or wide, the mouth open for a shout.
fn face(world: &mut World, parts: &Parts, f: &Face) {
    for i in [L, R] {
        let middle = crate::player_model::brow_middle(sign(i));
        let turn = Quat::from_rotation_z(sign(i) * -f.brow.clamp(-1., 1.) * 0.42);
        let lift = Vec3::Y * f.raise.clamp(-1., 1.) * 0.0022 + Vec3::Y * f.brow.min(0.) * 0.0008;
        place(world, parts.brows[i], Some(middle - turn * middle + lift), Some(turn), None);
        let open = f.eyes.clamp(-1., 1.);
        let squint = if open < 0. { v(1. + 0.12 * -open, 1. + 0.72 * open, 1.) } else { Vec3::splat(1. + 0.16 * open) };
        for (k, part) in parts.eyes[i].iter().enumerate() {
            let rest = parts.rest_eyes[i][k];
            // (The pupil and its glint sink a little as the eye narrows from above.)
            let sink = Vec3::Y * open.min(0.) * 0.0018;
            place(world, *part, Some(rest.translation + sink), None, Some(rest.scale * squint));
        }
    }
    // Puffed cheeks: the blush blown out off the casing and bigger.
    let puff = f.puff.clamp(0., 1.);
    for i in [L, R] {
        let (middle, out) = crate::player_model::cheek_middle(sign(i));
        let grow = 1. + 0.7 * puff;
        // KF5: the casing swells into a ball under the blush, and the blush rides out on its front.
        let ball = 0.0068 * puff;
        let push = out * (0.0015 + 0.0052) * puff;
        place(world, parts.cheeks[i], Some(middle * (1. - grow) + push), None, Some(Vec3::splat(grow)));
        place(world, parts.pouches[i], Some(middle + out * (0.0052 * puff - ball * 0.75) - Vec3::Y * 0.0012 * puff), None, Some(v(ball * 1.15, ball, ball)));
        shown(world, parts.pouches[i], puff > 0.03);
    }
    let talking = world.get::<Visibility>(parts.gape).is_some_and(|v| *v != Visibility::Hidden);
    let talk = if talking { world.get::<Transform>(parts.gape).map_or(0., |t| t.scale.y) } else { 0. };
    let open = (f.mouth * (1. - puff)).max(talk);
    shown(world, parts.gape, open > 0.03);
    if let Some(mut tr) = world.get_mut::<Transform>(parts.gape) {
        tr.scale.y = open.max(0.02);
    }
}
/// Puts a hotdog's rig back as the gun game has it: no kung fu hands or feet, the gun hand back, legs their own length,
/// the torso unsquashed, the face and the buns at rest.
pub fn release(world: &mut World, parts: &Parts) {
    place(world, parts.torso, None, None, Some(Vec3::ONE));
    for i in [L, R] {
        shown(world, parts.wrists[i], false);
        shown(world, parts.feet[i], false);
        place(world, parts.knees[i], Some(Vec3::NEG_Y * BONE), None, None);
        for bone in parts.leg_bones[i] {
            place(world, bone, Some(Vec3::NEG_Y * BONE * 0.5), None, Some(Vec3::ONE));
        }
        for bone in parts.arm_bones[i] {
            place(world, bone, None, None, Some(Vec3::ONE));
        }
        place(world, parts.brows[i], Some(Vec3::ZERO), Some(Quat::IDENTITY), None);
        place(world, parts.cheeks[i], Some(Vec3::ZERO), None, Some(Vec3::ONE));
        shown(world, parts.pouches[i], false);
        for (k, part) in parts.eyes[i].iter().enumerate() {
            let rest = parts.rest_eyes[i][k];
            place(world, *part, Some(rest.translation), None, Some(rest.scale));
        }
        let rest = parts.rest_buns[i];
        place(world, parts.buns[i], Some(rest.translation), None, Some(rest.scale));
    }
    place(world, parts.mustard, Some(Vec3::ZERO), Some(Quat::IDENTITY), None);
    shown(world, parts.gun, true);
}

// ----- Every hotdog, every frame

/// A spring for the soft parts: how far they lag behind the body, and how fast that changes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Jiggle {
    offset: Vec3,
    speed: Vec3,
    /// Where the top of the body was, and how fast it was going.
    was: Option<(Vec3, Vec3)>,
}
impl Jiggle {
    /// Steps the spring by `dt` with the top of the body now at `top` (world) and the torso turned `turn` (world);
    /// returns the soft parts' offset in the torso's frame.
    pub fn step(&mut self, top: Vec3, turn: Quat, dt: f32) -> Vec3 {
        if dt <= 1e-4 {
            return turn.inverse() * self.offset;
        }
        let dt = dt.min(0.05);
        let (velocity, accel) = match self.was {
            Some((at, speed)) => {
                let velocity = (top - at) / dt;
                (velocity, (velocity - speed) / dt)
            }
            None => (Vec3::ZERO, Vec3::ZERO),
        };
        self.was = Some((top, velocity));
        // A soft, quick spring that is shoved the other way when the body is shoved.
        let (k, c) = (900., 18.);
        let force = -self.offset * k - self.speed * c - accel.clamp_length_max(60.) * 0.9;
        self.speed += force * dt;
        self.offset += self.speed * dt;
        self.offset = self.offset.clamp_length_max(0.0035);
        turn.inverse() * self.offset
    }
}

/// Every hotdog's animator and soft parts, by id, and what this frame showed.
#[derive(Resource, Default)]
pub struct Fighters {
    pub animators: HashMap<u32, Animator>,
    jiggles: HashMap<u32, Jiggle>,
    /// The hotdogs whose rigs are posed for Kung Fu just now (their roots).
    posed: Vec<Entity>,
    /// This player's own fighter's pose this frame, for the first-person view.
    pub mine: Option<Frame>,
    time: Option<f32>,
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fighters>()
            .init_resource::<fx::Fx>()
            .add_systems(Startup, fx::setup)
            .add_systems(Update, (animate.after(crate::network::send), view::update, fx::flecks, fx::trails).chain());
    }
}

/// Once a frame, after the gun game has posed every hotdog (`player_model::animate`, in arena.rs and network.rs): in a
/// Kung Fu round, each living fighter's move put on its rig over the top; outside one, any rig that was posed for Kung
/// Fu put back.
pub fn animate(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let roster = sim::roster(world);
    let bodies: Vec<(Entity, u32, crate::player_model::Motion, Transform, Parts)> = world
        .query_filtered::<(Entity, &Gait, &Transform, &Parts), With<PlayerBody>>()
        .iter(world)
        .map(|(e, gait, tr, parts)| (e, gait.id(), gait.motion(), *tr, parts.clone()))
        .collect();
    let mut fighters = world.remove_resource::<Fighters>().unwrap_or_default();
    let dt = fighters.time.map_or(0., |was| (now - was).clamp(0., 0.1));
    fighters.time = Some(now);
    let mut posed = vec![];
    fighters.mine = None;
    if let Some(roster) = &roster {
        for (entity, id, motion, tr, parts) in &bodies {
            let Some(f) = roster.fighters.iter().find(|f| f.id == *id && f.alive) else {
                continue;
            };
            let gait = Solved::from_gait(&crate::player_model::pose(*motion));
            let moving = Locomotion { forward: motion.forward, strafe: motion.strafe, phase: motion.phase, crouch: motion.crouch > 0.5, airborne: f.airborne };
            let frame = fighters.animators.entry(*id).or_default().update(f, moving, &gait, dt);
            let skeleton = frame.solved.skeleton();
            let (_, chest, _) = frame.solved.torso_place();
            let top = tr.transform_point(skeleton.head);
            let jiggle = fighters.jiggles.entry(*id).or_default().step(top, tr.rotation * chest, dt);
            show(world, parts, &frame.solved, jiggle);
            fx::strike(world, *id, *id == roster.me, tr, &skeleton, &frame, now);
            if frame.quake > 0. {
                let me = bodies.iter().find(|b| b.1 == roster.me).map(|b| b.3.translation).or(roster.position);
                fx::quake(world, tr.translation, frame.quake, me, now);
            }
            if *id == roster.me {
                fighters.mine = Some(frame);
            }
            posed.push(*entity);
        }
        // This player's own fighter has no body online: its animator runs all the same, for the first-person view.
        if fighters.mine.is_none() {
            if let Some(f) = roster.fighters.iter().find(|f| f.id == roster.me && f.alive) {
                let moving = roster.moving;
                let gait = solve(&library().set(f.style).stance);
                let frame = fighters.animators.entry(f.id).or_default().update(f, moving, &gait, dt);
                fighters.mine = Some(frame);
            }
        }
        fx::events(world, roster, now);
    }
    // Rigs posed last frame and not this one (out of Kung Fu, or knocked out) go back to the gun game's pose.
    for entity in std::mem::take(&mut fighters.posed) {
        if !posed.contains(&entity) {
            if let Some((_, id, _, _, parts)) = bodies.iter().find(|b| b.0 == entity) {
                release(world, parts);
                fighters.animators.remove(id);
                fighters.jiggles.remove(id);
            }
        }
    }
    if roster.is_none() {
        fighters.animators.clear();
    }
    fighters.posed = posed;
    world.insert_resource(fighters);
}

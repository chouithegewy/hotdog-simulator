// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Original, shared high-resolution food geometry; no scanned character assets.
use crate::hotdog::{food_material, surface};
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

pub mod bend;
pub mod stim;

#[derive(Resource, Clone)]
pub struct PlayerModels {
    skin: Handle<Mesh>,
    bun: Handle<Mesh>,
    mustard: Handle<Mesh>,
    eye: Handle<Mesh>,
    limb: Handle<Mesh>,
    badge: Handle<Mesh>,
    /// Whether those meshes are the detailed set now (`refine`), and whether the device may have it (not a CPU
    /// renderer).
    detailed: bool,
    device_detailed: bool,
    /// The weapons (seven guns; full hands hold none), shared by every hand and by the first-person view.
    pub weapons: crate::weapons::Kit,
    pub skin_material: Handle<StandardMaterial>,
    bread: Handle<StandardMaterial>,
    sauce: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    /// The stim's blue fire and lightning, on every hotdog and in first person (stim.rs).
    stim: stim::Look,
    colors: Vec<Handle<StandardMaterial>>,
    /// The face (see `Face`): the smile, the inside of the mouth under it, the brows and the cheeks (each left then
    /// right); the warm brown the smile and the brows are drawn in, the inside of the mouth, the cheeks' blush; and the
    /// five colours a player chooses from (`PAINTS`), as its ring wears them.
    smile: Handle<Mesh>,
    gape: Handle<Mesh>,
    brows: [Handle<Mesh>; 2],
    cheeks: [Handle<Mesh>; 2],
    brown: Handle<StandardMaterial>,
    inside: Handle<StandardMaterial>,
    blush: Handle<StandardMaterial>,
    paints: [Handle<StandardMaterial>; 5],
    /// Kung Fu's hands and feet (kungfu/rig.rs), hung on every hotdog hidden until a Kung Fu round shows them.
    pub kungfu: crate::kungfu::rig::Look,
}
/// The colours a player can choose, as the lobby names them. Its hotdog's ring wears the colour chosen (`paint`), and
/// so does its name tag (`tag_rgb`).
pub const PAINTS: [&str; 5] = ["red", "blue", "green", "yellow", "black"];
/// The five colours as a ring wears them: strong enough to read across the counter.
const PAINT_RGB: [[f32; 3]; 5] = [[0.86, 0.07, 0.06], [0.07, 0.30, 0.90], [0.12, 0.66, 0.18], [0.98, 0.80, 0.10], [0.035, 0.03, 0.035]];
/// The colour a name tag is written in for a player who chose `color`, a name from `PAINTS`: its ring's, except that
/// black, which would not show on a dark tag, is written in charcoal. None for any other name (a bot's, which is '').
pub fn tag_rgb(color: &str) -> Option<[f32; 3]> {
    let i = PAINTS.iter().position(|name| *name == color)?;
    Some(if i == 4 { [0.30, 0.30, 0.32] } else { PAINT_RGB[i] })
}
/// A hotdog's face, the same friendly face on every hotdog in every mode: big eyes under softly raised brows, rosy
/// cheeks and a smile, whose mouth opens while its player speaks (`speak`). The colour its player chose is not on the
/// face but on the ring round its waist (`paint`).
#[derive(Component)]
pub struct Face {
    /// The inside of the mouth, under the smile: `speak` opens it.
    gape: Entity,
    /// The ring round the waist, and which colour of `PlayerModels::colors` it wears unpainted (its place's).
    ring: Entity,
    own: usize,
    /// How far the mouth is open with speech, 0 to 1, eased; and when it was last eased.
    talk: f32,
    time: f32,
}
/// The smile: the height of its middle, how far its corners turn up, how far round the casing it reaches either side of
/// the middle of the face (radians), and half the thickness of its line. How far under its middle the mouth opens at
/// the most, and how much of the smile's width the opening takes.
const MOUTH: f32 = 0.1328;
const SMILE: f32 = 0.0022;
const MOUTH_REACH: f32 = 0.36;
const LINE: f32 = 0.0010;
const GAPE: (f32, f32) = (0.0052, 0.8);
/// A brow: the height of its middle, how much its middle is raised over its ends, how far round the casing it reaches
/// either side of its middle (radians), and half its thickness. It is over the middle of an eye (`EYE`).
const BROW: (f32, f32, f32, f32) = (0.1663, 0.0008, 0.21, 0.0007);
/// The middle of an eye, across the face and up (its white is 14 mm across and 18 mm high).
const EYE: (f32, f32) = (0.009, 0.153);
/// A cheek: the middle of it, across the face and up, and its half-height and half-reach round the casing (radians).
const CHEEK: (f32, f32, f32, f32) = (0.0142, 0.1388, 0.0032, 0.19);
/// The middle of the brow on `side` (-1 left, 1 right), on the casing: Kung Fu slants the brows about it.
pub(crate) fn brow_middle(side: f32) -> Vec3 {
    on_face(across(side * EYE.0, BROW.0), BROW.0, 0.)
}
/// The middle of the cheek on `side` on the casing, and the way out of the casing there: Kung Fu puffs them.
pub(crate) fn cheek_middle(side: f32) -> (Vec3, Vec3) {
    let at = on_face(across(side * CHEEK.0, CHEEK.1), CHEEK.1, 0.);
    let (middle, _) = casing(CHEEK.1);
    (at, Vec3::new(at.x - middle.x, 0., at.z - middle.z).normalize_or(Vec3::NEG_Z))
}
/// The casing at height `y`: where its middle is (it is very slightly curved) and its radius there.
fn casing(y: f32) -> (Vec3, f32) {
    let u = (y - 0.023) / 0.155;
    let t = (u * 2. - 1.).abs();
    let cap = (1. - ((t - 0.76) / 0.24).max(0.).powi(2)).max(0.0001).sqrt();
    (Vec3::new(0.002 * (u * PI).sin(), y, 0.002 * (u * TAU).sin()), 0.023 * cap)
}
/// A point on the front of the casing (the face is on -z): `angle` round from straight ahead, at
/// height `y`, `proud` of the surface.
fn on_face(angle: f32, y: f32, proud: f32) -> Vec3 {
    let (middle, radius) = casing(y);
    middle + Vec3::new(angle.sin(), 0., -angle.cos()) * (radius + proud)
}
/// The angle round the casing at height `y` of the point on its front that is `x` across the face (the middle of the
/// face, between the eyes, is 0: the casing's own middle is a little to one side of it).
fn across(x: f32, y: f32) -> f32 {
    let (middle, radius) = casing(y);
    ((x - middle.x) / radius).clamp(-1., 1.).asin()
}
/// A line drawn on the casing: a tube along `path` (for s from -1 to 1, the angle round the casing and the height),
/// `thick` either side of it at s (0 closes it), its back just off the casing; `rings` along it and `sides` round it.
fn stroke(rings: u32, sides: u32, path: impl Fn(f32) -> (f32, f32), thick: impl Fn(f32) -> f32) -> Mesh {
    surface(
        rings,
        sides,
        true,
        |u, v| {
            let s = u * 2. - 1.;
            let ((angle, y), t, ring) = (path(s), thick(s), v * TAU);
            on_face(angle, y + t * ring.sin(), 0.0002 + 0.6 * t * (1. + ring.cos()))
        },
        |_, _| [1.; 4],
    )
}
/// The smile: a slim line across the face under the eyes whose corners turn up, round at its ends.
fn smile(rings: u32, sides: u32) -> Mesh {
    let middle = across(0., MOUTH);
    stroke(rings, sides, |s| (middle + s * MOUTH_REACH, MOUTH + SMILE * s * s), |s| LINE * (1. - s.powi(8)).max(0.).sqrt())
}
/// The inside of the mouth when it is open all the way: under the smile, its top hidden by the smile's line, its bottom
/// round, with the tongue at the bottom. It is drawn about the middle of the smile (`MOUTH`), so that `speak` opens it
/// by stretching it downwards; it lies on the casing as it is at the mouth's height, which is upright there.
fn gape(rings: u32, sides: u32) -> Mesh {
    let middle = across(0., MOUTH);
    surface(
        rings,
        sides,
        false,
        |u, v| {
            let s = u * 2. - 1.;
            let line = SMILE * (s * GAPE.1).powi(2);
            let (top, bottom) = (line + LINE * 0.6, line - GAPE.0 * (1. - s * s).max(0.).powf(0.6));
            on_face(middle + s * MOUTH_REACH * GAPE.1, MOUTH, 0.0004) + Vec3::Y * (top + (bottom - top) * v - MOUTH)
        },
        |u, v| {
            // Dark red-brown, and the pink of the tongue towards the bottom of the middle (linear colours).
            let s = (u * 2. - 1.).abs();
            let tongue = smooth((v - 0.45) / 0.4) * (1. - smooth((s - 0.3) / 0.45));
            let (dark, pink) = ([0.032, 0.004, 0.003], [0.78, 0.16, 0.19]);
            let mix = |i: usize| dark[i] + (pink[i] - dark[i]) * tongue;
            [mix(0), mix(1), mix(2), 1.]
        },
    )
}
/// A brow over the eye on `side` (-1 left, 1 right): a soft arc, raised in the middle and thinner at its ends.
fn brow(side: f32, rings: u32, sides: u32) -> Mesh {
    let middle = across(side * EYE.0, BROW.0);
    stroke(rings, sides, |s| (middle + s * BROW.2, BROW.0 - BROW.1 * s * s), |s| BROW.3 * (1. - s * s).max(0.).sqrt())
}
/// The cheek on `side` (-1 left, 1 right): an oval of blush lying on the casing under the eye and further out, which
/// fades out towards its edge.
fn cheek(side: f32, rings: u32, sides: u32) -> Mesh {
    let middle = across(side * CHEEK.0, CHEEK.1);
    surface(
        rings,
        sides,
        true,
        |u, v| {
            let ring = v * TAU;
            on_face(middle + CHEEK.3 * u * ring.cos(), CHEEK.1 + CHEEK.2 * u * ring.sin(), 0.00025)
        },
        |u, _| [1., 1., 1., (1. - u * u).powf(1.5)],
    )
}
/// Gives a hotdog's ring round its waist its player's colour, a name from `PAINTS`; any other name (a bot's, which is
/// '') gives it back the colour of its place. A ring in a team's colour (Forts: forts.rs, and the lobby's figures) keeps
/// it: only a ring in its own colour or one of the paints is painted, so that when the team's colour comes off again
/// the colour chosen goes back on.
pub fn paint(world: &mut World, root: Entity, color: &str) {
    let Some(&Face { ring, own, .. }) = world.get::<Face>(root) else {
        return;
    };
    let Some(wears) = world.get::<MeshMaterial3d<StandardMaterial>>(ring).map(|m| m.0.id()) else {
        return;
    };
    let models = world.resource::<PlayerModels>();
    let want = match PAINTS.iter().position(|name| *name == color) {
        Some(i) => &models.paints[i],
        None => &models.colors[own],
    };
    let ours = wears == models.colors[own].id() || models.paints.iter().any(|paint| paint.id() == wears);
    if ours && wears != want.id() {
        let want = want.clone();
        world.entity_mut(ring).insert(MeshMaterial3d(want));
    }
}
/// Moves a hotdog's mouth for this frame: `level` is how loudly its player is speaking, 0 to 1. The smile opens
/// downwards, wider the louder, and flaps with the syllables.
pub fn speak(world: &mut World, root: Entity, level: f32, time: f32) {
    let Some(mut face) = world.get_mut::<Face>(root) else {
        return;
    };
    let dt = (time - face.time).clamp(0., 0.25);
    face.time = time;
    let level = level.clamp(0., 1.);
    face.talk += (level - face.talk) * (1. - (-dt * if level > face.talk { 30. } else { 12. }).exp());
    // Syllables: the mouth flaps while the voice lasts, wider the louder it is.
    let flap = 0.5 + 0.5 * (time * 21. + root.to_bits() as f32).sin() * (time * 5.3).cos();
    let open = face.talk * (0.35 + 0.65 * flap);
    let gape = face.gape;
    let shown = if open > 0.02 { Visibility::Inherited } else { Visibility::Hidden };
    if let Some(mut visibility) = world.get_mut::<Visibility>(gape) {
        if *visibility != shown {
            *visibility = shown;
        }
    }
    let scale = open.max(0.02);
    if let Some(mut tr) = world.get_mut::<Transform>(gape) {
        if tr.scale.y != scale {
            tr.scale.y = scale;
        }
    }
}
#[derive(Component)]
pub struct PlayerBody;
/// The parts of a hotdog that wear its own colour: its legs and its badge (the ring round its waist, which `paint`
/// gives its player's colour). Forts gives them the team's (forts.rs).
#[derive(Component, Clone, Default)]
pub struct Livery(pub Vec<Entity>);
/// Joints of one hotdog. Index 0 is its left side (-x), 1 its right; it faces -z.
#[derive(Component, Clone)]
pub struct Rig {
    torso: Entity,
    /// The head, on the torso where the nod's bend puts it (bend.rs): the face hangs from it. And the parts drawn bent by
    /// the nod: the sausage, the buns (left, right) and the mustard.
    head: Entity,
    bent: [Entity; 4],
    /// The stim's fire and lightning (stim.rs).
    aura: stim::Aura,
    hips: [Entity; 2],
    knees: [Entity; 2],
    shoulders: [Entity; 2],
    elbows: [Entity; 2],
    arms: [[Entity; 2]; 2],
    /// The gun hand at the end of the right forearm, and the weapons it can hold (0 is empty).
    hand: Entity,
    held: [Entity; 8],
}
/// Read-only views for `death`, which hands a fallen hotdog's rig to a ragdoll: the jointed
/// entities, which hotdog a rig belongs to, the motion it was last posed with, and the rig's
/// proportions (bone length, the right hip and shoulder, metres per second at a full run).
#[derive(Clone, Copy)]
pub struct Joints {
    pub torso: Entity,
    /// The head, which nods on the torso (what is worn on the head hangs from it).
    pub head: Entity,
    pub hips: [Entity; 2],
    pub knees: [Entity; 2],
    pub shoulders: [Entity; 2],
    pub elbows: [Entity; 2],
}
impl Rig {
    pub fn joints(&self) -> Joints {
        Joints { torso: self.torso, head: self.head, hips: self.hips, knees: self.knees, shoulders: self.shoulders, elbows: self.elbows }
    }
}
impl Gait {
    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn motion(&self) -> Motion {
        self.motion
    }
}
pub const RIG: (f32, Vec3, Vec3, f32) = (BONE, HIP, SHOULDER, RUN);
/// What the simulation says a hotdog is doing; `animate` turns it into motion.
#[derive(Clone, Copy, Default)]
pub struct Cues {
    pub alive: bool,
    pub cheer: bool,
    pub prone: bool,
    pub crouch: bool,
    /// Seconds of stim left (0: none). The stim's start cues the clap, and the stim's fire fades out over its end.
    pub stim: f32,
    /// The weapon in hand, 0..=7 (0: none, the hands are full).
    pub weapon: usize,
    /// Its hands hold a loose object (not the bacon, which rides on its back): both are held out in front.
    pub carry: bool,
    /// Where the hotdog aims above (+) or below (-) the horizon, in radians.
    pub pitch: f32,
    /// The simulation's count of melee attacks; a change starts the strike.
    pub swings: u32,
    /// Aiming down its sights (the simulation's `Player::aimed`): the syrup rifle or the ketchup sniper held up to
    /// the eye.
    pub aimed: bool,
    /// What the hotdog's body is doing, for poses that want it: off the ground (jumping, falling, flying; with
    /// crouch held its legs tuck up), thrown by a blast or a hit and not yet at rest, and how it actually moved
    /// over the last step in m/s, pushes and throws included (none use these two yet).
    pub airborne: bool,
    #[allow(dead_code)]
    pub launched: bool,
    #[allow(dead_code)]
    pub velocity: Vec3,
    /// How far through a reload it is (reload.rs `progress`; 0 for none).
    pub reload: f32,
    /// In a Kung Fu round: its moves own the whole body (kungfu/rig.rs), the head does not nod and the body leans with
    /// the look as it always did.
    pub kungfu: bool,
}
/// Smoothed motion behind the procedural run, strafe, prone, stim, recoil and celebration poses.
#[derive(Component)]
pub struct Gait {
    at: Vec2,
    time: f32,
    live: bool,
    stim: bool,
    motion: Motion,
    id: u32,
    swings: u32,
    /// Where the held weapon ends, in world space, as last drawn.
    muzzle: Vec3,
}
/// Each limb is two bones of this length, hinged at the knee or elbow.
const BONE: f32 = 0.016;
const HIP: Vec3 = Vec3::new(0.014, 0.033, 0.);
const SHOULDER: Vec3 = Vec3::new(0.0465, 0.095, 0.);
/// Metres per second at a full run: the stride's reach and pump are full from here. Faster still (the hotdogs walk at
/// 1.25 m/s), the legs turn over faster in proportion (`Gait::follow`), so the feet cover as much ground a cycle as
/// they do at RUN and do not slide more.
const RUN: f32 = 0.67;
/// The fastest pace the gait follows, as a share of RUN (a stimmed walk is 2.8).
const PACE: f32 = 4.;
/// The stim clap lasts this long; the stubby arms stretch this much to meet in front of the chest.
const CLAP: f32 = 0.55;
const CLAP_REACH: f32 = 2.3;
/// How a gun that is not being aimed is carried: tipped forward of the forearm.
const CARRY: f32 = 1.2;
/// Aimed down its sights, the gun is held with what the eye is put to (`weapons::sight`) RELIEF in front of the right
/// eye (in the torso's frame), and comes up to it, or goes back down, in SIGHTING seconds.
const RIGHT_EYE: Vec3 = Vec3::new(EYE.0, EYE.1, -0.0255);
const RELIEF: f32 = 0.008;
const SIGHTING: f32 = 0.15;
/// Where the left hand holds the syrup rifle, in the gun frame (ahead, up, to the right): under
/// the ribbed handguard, halfway along it.
const HANDGUARD: Vec3 = Vec3::new(0., -0.0035, -0.046);
/// Where a hotdog carrying a loose object holds it, in its body's frame (the right hand; the left is its
/// mirror image): out in front, low, a little apart, clear of its own bun (kitchen_sim's HOTDOG_RADIUS, with
/// room for the cloth). A carried napkin hangs from these two points.
pub const HANDS: Vec3 = Vec3::new(0.026, 0.076, -0.066);
/// A full crouch: how far the thighs swing forward (the shins fold back twice as far, so the feet stay
/// under the hips), and how far the body leans over them.
const SQUAT: f32 = 1.05;
/// Tucked in the air (crouch held off the ground): how far the thighs swing up (the shins fold back twice as far), the
/// body staying where it is.
const TUCK: f32 = 1.4;
const HUNCH: f32 = 0.45;

/// Tip of the held weapon in the gun hand's frame, where -y runs on from the forearm towards the
/// muzzle and -z is the top of the weapon: a gun's nozzle, or with the hands full just past the
/// empty hand.
pub fn muzzle(weapon: usize) -> Vec3 {
    crate::weapons::in_hand().rotation * crate::weapons::MUZZLE[weapon.min(7)]
}

/// Inputs of a pose. Angles are radians; `forward` and `strafe` are signed fractions of a full run.
#[derive(Clone, Copy, Debug, Default)]
pub struct Motion {
    pub phase: f32,
    pub forward: f32,
    /// Positive is the hotdog's right.
    pub strafe: f32,
    /// Arms spread wide.
    pub open: f32,
    /// Victory: arms thrown up and bouncing.
    pub cheer: f32,
    /// Weapon recoil, and the sustained shake of a continuous stream.
    pub kick: f32,
    pub rumble: f32,
    /// The gun arm held out along the aim, and how far that aim is above or below the horizon.
    pub aim: f32,
    pub pitch: f32,
    /// The head's nod as drawn (kitchen_sim head.rs `nod`, eased: bend.rs `ease`), and how much the whole body leans
    /// with the look instead (1 in Kung Fu, where the head does not nod; 0 elsewhere: the body stays planted).
    pub nod: f32,
    pub lean: f32,
    /// The gun held up to the eye, aimed down its sights (0 not at all, 1 fully; SIGHTING seconds either way).
    pub sight: f32,
    pub prone: f32,
    /// Crouching: knees bent under a body leant forward and held lower.
    pub crouch: f32,
    /// Crouch held in the air: the knees drawn up towards the body, which stays where the jump put it (kitchen_sim
    /// feast.rs `tucked`): the legs fold up (further than a crouch's, TUCK), but the hips do not sink.
    pub tuck: f32,
    /// Stim: progress of the right hand clapped on the left forearm; 0 and 1 are rest.
    pub clap: f32,
    /// The weapon in the gun hand (0: none, the hands are full), and progress of a melee strike (with
    /// full hands, a throw of what they carry); 0 and 1 are rest. The strike is on whip.rs's timeline, the first-person
    /// swing's: `struck` says the blow lands (`Some(true)` for a backstab) and it holds on contact for `held` seconds.
    pub weapon: usize,
    pub swing: f32,
    pub struck: Option<bool>,
    pub held: f32,
    pub felt: bool,
    /// Carrying a loose object: both hands held out in front, at HANDS (0 not at all, 1 fully).
    pub carry: f32,
    /// How far through a reload (0: none): the gun comes down a little and the free hand works its magazine
    /// (reload.rs `arm` and `grip`).
    pub reload: f32,
    pub time: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    /// Moves the hips, and the torso that pivots on them.
    pub shift: Vec3,
    pub torso: Quat,
    pub hips: [Quat; 2],
    pub knees: [Quat; 2],
    pub shoulders: [Quat; 2],
    pub elbows: [Quat; 2],
    /// Arm length scale.
    pub reach: [f32; 2],
    /// The gun hand on its forearm: how the held weapon is angled.
    pub wrist: Quat,
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
/// Elbow (or knee) and hand (or foot) of a limb hanging from `joint`.
#[cfg(test)]
fn limb(joint: Vec3, root: Quat, bend: Quat, bone: f32) -> (Vec3, Vec3) {
    let middle = joint + root * (Vec3::NEG_Y * bone);
    (middle, middle + root * bend * (Vec3::NEG_Y * bone))
}
/// Two-bone reach: shoulder and elbow rotations that put the hand on `target`, bending towards `pole`.
fn reach_for(shoulder: Vec3, target: Vec3, bone: f32, pole: Vec3) -> (Quat, Quat) {
    let to = target - shoulder;
    let gap = to.length().clamp(bone * 0.2, bone * 1.999);
    let along = to.normalize_or(Vec3::NEG_Y);
    let out = (pole - along * pole.dot(along)).normalize_or(Vec3::X);
    let elbow = shoulder + along * gap * 0.5 + out * (bone * bone - gap * gap * 0.25).max(0.).sqrt();
    let upper = (elbow - shoulder).normalize();
    let fore = (shoulder + along * gap - elbow).normalize();
    let bend = upper.dot(fore).clamp(-1., 1.).acos();
    // The elbow hinges about the arm's own x axis: forearm = cos(bend) * upper - sin(bend) * z.
    let z = ((upper * bend.cos() - fore) / bend.sin().max(1e-4)).normalize_or(Vec3::Z);
    let y = -upper;
    (
        Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z)),
        Quat::from_rotation_x(bend),
    )
}
/// The whole body pose for one instant. Pure, so the gaits can be checked without a renderer.
///
/// A positive rotation about x swings a hanging limb forwards (the hotdog faces -z); about z it
/// swings it towards +x. Leg `i` is half a stride out of step with the other, and each arm swings
/// against the leg on its own side.
pub fn pose(m: Motion) -> Pose {
    let run = m.forward.abs().min(1.);
    let slide = m.strafe.abs().min(1.);
    let drive = run.max(slide);
    let upright = 1. - m.prone;
    // Crouching folds the legs under the hips, which sink by what the fold takes off the legs' height.
    let squat = SQUAT * m.crouch.clamp(0., 1.) * upright;
    let sink = 2. * BONE * (1. - squat.cos());
    // Tucked in the air the legs fold further, the knees drawn up under a body that stays put.
    let knees_up = TUCK * m.tuck.clamp(0., 1.) * upright;
    // Strafing spreads the arms for balance; a winner throws them wide open.
    let open = m.open.max(0.6 * slide).max(m.cheer).min(1.);
    let mut hips = [Quat::IDENTITY; 2];
    let mut knees = [Quat::IDENTITY; 2];
    let mut shoulders = [Quat::IDENTITY; 2];
    let mut elbows = [Quat::IDENTITY; 2];
    for (i, side) in [-1f32, 1.].into_iter().enumerate() {
        let stride = m.phase + if i == 1 { PI } else { 0. };
        let swing = stride.sin();
        // 1 as the leg passes under the body on its way forward: the moment the shin tucks up.
        let tuck = (0.5 + 0.5 * stride.cos()).powi(2);
        // A crouch-walk takes shorter steps.
        let reach = run * (0.45 + 0.45 * run) * (1. - 0.45 * m.crouch.clamp(0., 1.));
        let hip = (0.15 * run + reach * swing + 0.35 * slide * tuck) * upright - 1.45 * m.prone + squat + knees_up;
        // A strafe swings the legs sideways from a wide stance instead of fore and aft.
        let step = slide * (side * 0.2 + 0.3 * swing * m.strafe.signum()) * upright;
        let knee = (0.15 * run + (0.5 + 0.9 * run) * run * tuck + 0.7 * slide * tuck) * upright
            + 0.35 * m.prone * drive * swing.max(0.)
            + 2. * squat
            + 2. * knees_up;
        hips[i] = Quat::from_rotation_z(step) * Quat::from_rotation_x(hip);
        knees[i] = Quat::from_rotation_x(-knee);
        let pump = run * (0.35 + 0.5 * run) * (1. - 0.7 * open);
        let idle = (m.time * 2.).sin() * 0.06 * (1. - drive);
        // Prone, the arms prop the chest and take turns reaching forward to crawl.
        let mut shoulder =
            (idle - pump * swing) * upright + m.prone * (1.45 + 0.35 * drive * swing);
        let elbow = (run * (0.9 - 0.35 * swing) * (1. - open) + 0.25 * open) * upright
            + m.prone * (1.5 - 0.4 * drive * swing);
        let wave = m.cheer * (0.5 + 0.18 * (m.time * 7. + side).sin());
        let mut spread = side * (0.35 + 1.2 * open * upright + wave);
        if i == 0 {
            // The free arm flails away from a recoil; the gun arm is aimed below.
            shoulder -= m.kick * 0.3;
            spread += side * m.kick * 0.5;
        }
        shoulders[i] = Quat::from_rotation_z(spread) * Quat::from_rotation_x(shoulder);
        elbows[i] = Quat::from_rotation_x(elbow);
    }
    let bounce = drive * 0.0035 * m.phase.sin().powi(2) + m.cheer * 0.006 * (m.time * 7.).sin().abs();
    // Lean into the run, away from the recoil, and shudder while a stream is flowing.
    // (A crouching body is already bent over and leans less into its walk.)
    let lean = (m.kick * 0.35 - (0.10 + 0.14 * run) * m.forward.clamp(-1., 1.) * (1. - 0.6 * m.crouch.clamp(0., 1.))
        + m.rumble * 0.05 * (m.time * 61.).sin())
        * upright
        - 1.42 * m.prone
        - HUNCH * squat / SQUAT;
    let roll = (0.04 * run * m.phase.sin() - 0.16 * m.strafe.clamp(-1., 1.)
        + m.rumble * 0.04 * (m.time * 47.).sin())
        * upright
        + 0.05 * m.prone * drive * m.phase.sin();
    let stab = strike(m.weapon, m.swing);
    let twist = 0.10 * run * m.phase.sin() * upright + stab.twist * stab.weight;
    let torso = Quat::from_rotation_y(twist)
        * Quat::from_rotation_z(roll)
        * Quat::from_rotation_x(lean + (stab.lean * stab.weight + 0.2 * m.pitch.clamp(-1., 1.) * m.lean) * upright);
    let mut reach = [1.; 2];
    // A gun is held out along the aim (straight ahead of the hotdog, whatever the body is doing)
    // with the elbow under it, and recoil swings the muzzle up from there.
    let aim = m.aim * (1. - m.cheer);
    if aim > 0. {
        // The gun follows the aim up and down, short of digging into the counter, and recoil
        // climbs from wherever it points.
        let climb = (m.kick * 0.9).min(1.4);
        // (Reloading, the gun comes down a little towards the free hand: reload.rs.)
        let pitch = m.pitch.clamp(-(0.15 + 0.4 * upright), 1.) - 0.4 * crate::reload::arm(m.reload);
        let along = torso.inverse() * (Quat::from_rotation_x((pitch + climb).min(1.5)) * Vec3::NEG_Z);
        let (shoulder, elbow) = reach_for(
            SHOULDER,
            SHOULDER + along * BONE * 1.998,
            BONE,
            torso.inverse() * Vec3::NEG_Y,
        );
        shoulders[1] = shoulders[1].slerp(shoulder, aim);
        elbows[1] = elbows[1].slerp(elbow, aim);
    }
    // Aimed down its sights (not prone, which keeps the arm out along the aim): the gun hand bends in to hold the gun
    // up to the right eye, its sight line along the aim, and its kick climbs half as far.
    let sight = smooth(m.sight) * aim * upright;
    let eyepiece = crate::weapons::sight(m.weapon).filter(|_| sight > 0.);
    let mut sighted = None;
    if let Some(eyepiece) = eyepiece {
        let climb = (m.kick * 0.45).min(0.7);
        let pitch = m.pitch.clamp(-(0.15 + 0.4 * upright), 1.);
        let along = torso.inverse() * (Quat::from_rotation_x((pitch + climb).min(1.5)) * Vec3::NEG_Z);
        let hold = held(along, torso.inverse() * Vec3::Y);
        // (To the right eye where the nod has it: bend.rs.)
        let (shift, turn) = kitchen_sim::head::head(m.nod);
        let hand = shift + turn * RIGHT_EYE + along * RELIEF - hold * (crate::weapons::in_hand().rotation * eyepiece);
        let length = (SHOULDER.distance(hand) / 1.9).max(BONE);
        let (shoulder, elbow) = reach_for(SHOULDER, hand, length, torso.inverse() * Vec3::new(0.7, -1., 0.2));
        shoulders[1] = shoulders[1].slerp(shoulder, sight);
        elbows[1] = elbows[1].slerp(elbow, sight);
        reach[1] += (length / BONE - reach[1]) * sight;
        sighted = Some(hold);
    }
    // The melee strike: the arm cocks back, lunges past its normal reach and follows through, and the
    // free hand joins in where the blow takes both.
    if stab.weight > 0. {
        let bone = BONE * stab.reach;
        let (shoulder, elbow) = reach_for(SHOULDER, stab.hand, bone, Vec3::new(0.6, -1., 0.2));
        shoulders[1] = shoulders[1].slerp(shoulder, stab.weight);
        elbows[1] = elbows[1].slerp(elbow, stab.weight);
        reach[1] = 1. + (stab.reach - 1.) * stab.weight;
        if let Some(off) = stab.off {
            let (shoulder, elbow) = reach_for(Vec3::new(-SHOULDER.x, SHOULDER.y, 0.), off, bone, Vec3::new(-0.6, -1., 0.2));
            shoulders[0] = shoulders[0].slerp(shoulder, stab.weight);
            elbows[0] = elbows[0].slerp(elbow, stab.weight);
            reach[0] = 1. + (stab.reach - 1.) * stab.weight;
        }
    }
    // An empty hand runs on from the forearm; a gun that is not being aimed is carried tipped forward
    // of it. During a strike the weapon is turned the way the blow wants it, whatever the arm is doing.
    let rest = Quat::from_rotation_x(if m.weapon == 0 { 0. } else { CARRY * (1. - aim) });
    let rest = match sighted {
        Some(hold) => rest.slerp((shoulders[1] * elbows[1]).inverse() * hold, sight),
        None => rest,
    };
    let wrist = if stab.weight > 0. {
        rest.slerp((shoulders[1] * elbows[1]).inverse() * held(stab.aim, stab.top), stab.weight)
    } else {
        rest
    };
    // The syrup rifle is shouldered in both hands: the left reaches across the chest to the
    // handguard wherever the gun is (aimed, kicking through a burst, or swung butt first), its arm
    // stretched as far as that takes and the elbow dropped forward, clear of the chest.
    if m.weapon == 6 && aim > 0. {
        let bone = Vec3::NEG_Y * BONE * reach[1];
        let forearm = shoulders[1] * elbows[1];
        let hand = SHOULDER + shoulders[1] * bone + forearm * bone;
        let target = hand + forearm * wrist * (crate::weapons::in_hand().rotation * HANDGUARD);
        let socket = Vec3::new(-SHOULDER.x, SHOULDER.y, 0.);
        let length = (socket.distance(target) / 1.9).max(BONE);
        let (shoulder, elbow) = reach_for(socket, target, length, torso.inverse() * Vec3::new(-0.2, -0.8, -1.));
        shoulders[0] = shoulders[0].slerp(shoulder, aim);
        elbows[0] = elbows[0].slerp(elbow, aim);
        reach[0] += (length / BONE - reach[0]) * aim;
    }
    // A reload: the free hand goes to the gun's magazine wherever the gun is, follows it out and down, brings the fresh
    // one up into it and works the action (reload.rs `grip`), as the rifle's left hand reaches its handguard.
    let load = crate::reload::arm(m.reload) * aim * (1. - stab.weight);
    if load > 0. {
        let bone = Vec3::NEG_Y * BONE * reach[1];
        let forearm = shoulders[1] * elbows[1];
        let hand = SHOULDER + shoulders[1] * bone + forearm * bone;
        let gun = crate::arena::Weapon::index(m.weapon);
        let target = hand + forearm * wrist * (crate::weapons::in_hand().rotation * crate::reload::grip(gun, m.reload));
        let socket = Vec3::new(-SHOULDER.x, SHOULDER.y, 0.);
        let length = (socket.distance(target) / 1.9).max(BONE);
        let (shoulder, elbow) = reach_for(socket, target, length, torso.inverse() * Vec3::new(-0.2, -0.8, -1.));
        shoulders[0] = shoulders[0].slerp(shoulder, load);
        elbows[0] = elbows[0].slerp(elbow, load);
        reach[0] += (length / BONE - reach[0]) * load;
    }
    // Carrying a loose object: both hands held out in front, low and a little apart (HANDS), the elbows out.
    let carry = m.carry.clamp(0., 1.) * upright * (1. - m.cheer);
    if carry > 0. {
        for (i, side) in [-1f32, 1.].into_iter().enumerate() {
            let socket = Vec3::new(side * SHOULDER.x, SHOULDER.y, 0.);
            // (HANDS is in the body's frame, the shoulders in the torso's, which leans about the hips.)
            let pivot = Vec3::Y * HIP.y;
            let target = torso.inverse() * (Vec3::new(side * HANDS.x, HANDS.y, HANDS.z) - pivot) + pivot;
            let length = (socket.distance(target) / 1.9).max(BONE);
            let (shoulder, elbow) = reach_for(socket, target, length, Vec3::new(side * 0.7, -1., 0.2));
            shoulders[i] = shoulders[i].slerp(shoulder, carry);
            elbows[i] = elbows[i].slerp(elbow, carry);
            reach[i] += (length / BONE - reach[i]) * carry;
        }
    }
    let clap = smooth(m.clap / 0.25) * (1. - smooth((m.clap - 0.75) / 0.25));
    if clap > 0. {
        // Stim: the left forearm comes across the chest and the right hand slaps down on it.
        let slap = smooth((m.clap - 0.30) / 0.14);
        let bone = BONE * CLAP_REACH;
        let (left, left_elbow) = reach_for(
            Vec3::new(-SHOULDER.x, SHOULDER.y, 0.),
            Vec3::new(0.004, 0.074, -0.040),
            bone,
            Vec3::new(-0.4, -1., 0.1),
        );
        let hand = Vec3::new(0.032, 0.108, -0.040).lerp(Vec3::new(-0.008, 0.080, -0.041), slap);
        let (right, right_elbow) = reach_for(SHOULDER, hand, bone, Vec3::new(0.5, -0.6, 0.3));
        shoulders = [shoulders[0].slerp(left, clap), shoulders[1].slerp(right, clap)];
        elbows = [elbows[0].slerp(left_elbow, clap), elbows[1].slerp(right_elbow, clap)];
        reach = [
            reach[0] + (CLAP_REACH - reach[0]) * clap,
            reach[1] + (CLAP_REACH - reach[1]) * clap,
        ];
    }
    Pose {
        shift: Vec3::new(0., bounce * upright - sink, m.kick * 0.006 * upright + 0.075 * m.prone),
        torso,
        hips,
        knees,
        shoulders,
        elbows,
        reach,
        wrist,
    }
}
/// The gun hand's rotation (in the torso's frame) that points the weapon's muzzle along `aim` with its
/// top towards `top`.
fn held(aim: Vec3, top: Vec3) -> Quat {
    let y = -aim.normalize_or(Vec3::NEG_Z);
    let z = -(top - y * top.dot(y)).normalize_or(Vec3::Y);
    Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z))
}
/// One beat of a melee strike, in the torso's frame: where the gun hand is, which way the weapon's muzzle
/// and its top point, where the free hand is (when the blow takes both), and how the body is wound
/// (`twist`, about the spine) and thrown (`lean`, negative forwards).
#[derive(Clone, Copy)]
struct Beat {
    hand: Vec3,
    aim: Vec3,
    top: Vec3,
    off: Vec3,
    twist: f32,
    lean: f32,
}
impl Beat {
    fn mix(self, to: Beat, t: f32) -> Beat {
        Beat {
            hand: self.hand.lerp(to.hand, t),
            aim: self.aim.lerp(to.aim, t),
            top: self.top.lerp(to.top, t),
            off: self.off.lerp(to.off, t),
            twist: self.twist + (to.twist - self.twist) * t,
            lean: self.lean + (to.lean - self.lean) * t,
        }
    }
}
/// How each weapon is swung: the wind-up, the blow and the follow-through, how far the arm stretches,
/// and whether the free hand helps.
///
/// - 0, hands full: no blow at all, but the throw of what they carry, heaved in both hands from low
///   in front up and out.
/// - 1, the mustard pistol: raised beside the head and whipped down, butt first.
/// - 2, the ketchup sniper: tipped muzzle-up and over the shoulder so the stock leads, and driven
///   forward: a rifle butt.
/// - 3, the hot sauce thrower: held crosswise in both hands and shoved out from the chest.
/// - 4, the relish bazooka: the tube swung flat across the body like a bat, in both hands.
/// - 5, the bug spray: a short straight jab with the can.
/// - 6, the syrup rifle: a horizontal butt stroke. Pulled back to the right hip, it is swung round
///   level so the muzzle goes back past the left shoulder and the butt is driven forward. The left
///   hand keeps hold of the handguard (see `pose`).
/// - 7, the mayonnaise bottle: a bonk. Raised high on the right, the bottle is brought down crosswise
///   in front, bottom first, like a rolled-up newspaper on a fly.
fn blow(weapon: usize) -> ([Beat; 3], f32, bool) {
    let v = Vec3::new;
    let beat = |hand: Vec3, aim: Vec3, top: Vec3, twist: f32, lean: f32| Beat { hand, aim, top, off: v(-hand.x, hand.y - 0.004, hand.z - 0.004), twist, lean };
    let (ahead, up) = (Vec3::NEG_Z, Vec3::Y);
    // Both hands together, mirrored, square to the body.
    let toss = |hand: Vec3, aim: Vec3, lean: f32| Beat { off: v(-hand.x, hand.y, hand.z), ..beat(hand, aim, up, 0., lean) };
    match weapon {
        1 => (
            [
                beat(v(0.062, 0.134, 0.016), v(0.1, 0.7, 0.7), v(0.5, -0.5, 0.7), -0.55, 0.16),
                beat(v(0.038, 0.084, -0.052), v(0., 0.95, -0.3), v(0., 0.3, 0.95), 0.55, -0.40),
                beat(v(0.030, 0.068, -0.048), v(0., 0.8, -0.6), v(0., 0.6, 0.8), 0.68, -0.44),
            ],
            1.6,
            false,
        ),
        2 => (
            [
                beat(v(0.062, 0.100, 0.008), v(0.1, 1., 0.), v(0., 0., 1.), -0.50, 0.06),
                beat(v(0.046, 0.088, -0.050), v(0.1, 0.7, 0.7), v(0., -0.7, 0.7), 0.45, -0.30),
                beat(v(0.040, 0.082, -0.052), v(0.1, 0.6, 0.8), v(0., -0.8, 0.6), 0.55, -0.34),
            ],
            1.6,
            false,
        ),
        3 => (
            [
                Beat { off: v(-0.032, 0.086, -0.016), ..beat(v(0.036, 0.088, -0.014), v(-1., 0., -0.25), up, 0., 0.14) },
                Beat { off: v(-0.032, 0.090, -0.062), ..beat(v(0.036, 0.092, -0.064), v(-1., 0., -0.1), up, 0.10, -0.38) },
                Beat { off: v(-0.032, 0.088, -0.058), ..beat(v(0.036, 0.090, -0.060), v(-1., 0., -0.1), up, 0.12, -0.40) },
            ],
            1.8,
            true,
        ),
        4 => (
            [
                Beat { off: v(0.044, 0.094, -0.022), ..beat(v(0.080, 0.100, 0.002), v(0.8, 0.25, 0.55), up, -0.75, 0.05) },
                Beat { off: v(-0.006, 0.090, -0.060), ..beat(v(0.024, 0.094, -0.058), v(-0.35, 0.05, -0.94), up, 0.60, -0.18) },
                Beat { off: v(-0.030, 0.088, -0.050), ..beat(v(-0.004, 0.090, -0.052), v(-0.9, 0., -0.4), up, 0.85, -0.20) },
            ],
            1.9,
            true,
        ),
        5 => (
            [
                beat(v(0.050, 0.092, 0.006), ahead, up, -0.25, 0.04),
                beat(v(0.034, 0.096, -0.060), ahead, up, 0.30, -0.20),
                beat(v(0.030, 0.094, -0.054), ahead, up, 0.36, -0.22),
            ],
            1.6,
            false,
        ),
        6 => (
            [
                beat(v(0.058, 0.080, 0.006), v(-0.35, 0.25, -0.9), up, -0.50, 0.06),
                beat(v(0.006, 0.090, -0.064), v(-0.9, 0.2, 0.4), up, 0.55, -0.30),
                beat(v(0.002, 0.088, -0.058), v(-0.92, 0.15, 0.36), up, 0.62, -0.34),
            ],
            2.,
            false,
        ),
        7 => (
            [
                beat(v(0.054, 0.136, 0.004), v(0.3, 0.75, 0.6), v(0.2, -0.6, 0.75), -0.35, 0.08),
                beat(v(0.026, 0.092, -0.058), v(0.8, -0.3, -0.5), v(0., 1., 0.), 0.35, -0.26),
                beat(v(0.020, 0.080, -0.054), v(0.85, -0.45, -0.3), v(0., 1., 0.), 0.42, -0.30),
            ],
            1.6,
            false,
        ),
        _ => (
            [
                toss(v(0.024, 0.062, -0.022), v(0., -0.5, -0.85), 0.10),
                toss(v(0.020, 0.124, -0.062), v(0., 0.55, -0.84), -0.32),
                toss(v(0.018, 0.130, -0.056), v(0., 0.8, -0.6), -0.36),
            ],
            2.2,
            true,
        ),
    }
}
/// One instant of a melee strike: the beat it has reached, how far the arms stretch, the free hand's
/// place when it helps, and how much of all that applies.
struct Strike {
    hand: Vec3,
    aim: Vec3,
    top: Vec3,
    off: Option<Vec3>,
    twist: f32,
    lean: f32,
    reach: f32,
    weight: f32,
}
/// The strike with `weapon` at `u` in 0..1, in the same beats as the first-person one and on its timeline (whip.rs
/// `between`): wind-up, a blow that accelerates into its target, follow-through, recovery.
fn strike(weapon: usize, u: f32) -> Strike {
    use crate::whip::Key;
    let ([cocked, hit, follow], stretch, both) = blow(weapon);
    let (from, to, t) = crate::whip::between(u);
    let (beat, reach, weight) = match (from, to) {
        (Key::Rest, Key::Rest) => (cocked, 1., 0.),
        (Key::Rest, _) => (cocked, 1., t),
        (Key::Cocked, _) => (cocked.mix(hit, t), 1. + (stretch - 1.) * t, 1.),
        (Key::Blow, _) => (hit.mix(follow, t), stretch, 1.),
        (Key::Follow, _) => (follow, stretch, 1. - t),
    };
    Strike {
        hand: beat.hand,
        aim: beat.aim,
        top: beat.top,
        off: both.then_some(beat.off),
        twist: beat.twist,
        lean: beat.lean,
        reach,
        weight,
    }
}
/// The gun hand and the tip of the weapon it holds (a gun's nozzle; with the hands full, just past
/// the hand), in the hotdog's own frame.
pub fn grip(p: &Pose, weapon: usize) -> (Vec3, Vec3) {
    let pivot = Vec3::Y * HIP.y;
    let bone = Vec3::NEG_Y * BONE * p.reach[1];
    let forearm = p.shoulders[1] * p.elbows[1];
    let hand = SHOULDER + p.shoulders[1] * bone + forearm * bone;
    let tip = hand + forearm * p.wrist * muzzle(weapon);
    let place = |v: Vec3| pivot + p.shift + p.torso * (v - pivot);
    (place(hand), place(tip))
}
/// Cheap repeatable noise in 0..1 for the crackle of stim lightning.
pub fn noise(n: u32) -> f32 {
    let x = n.wrapping_mul(747796405).wrapping_add(2891336453);
    let x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    ((x >> 22) ^ x) as f32 / u32::MAX as f32
}
/// A hotdog's own parts, for anything else made of one (topsy.rs): the sausage (standing, 0.023 to 0.178 up, 0.023
/// round), a half bun (one each side, `BUN` across), the mustard down its front and an eye; and their materials.
#[derive(Clone)]
pub struct Food {
    pub sausage: Handle<Mesh>,
    pub bun: Handle<Mesh>,
    pub mustard: Handle<Mesh>,
    pub eye: Handle<Mesh>,
    pub skin: Handle<StandardMaterial>,
    pub bread: Handle<StandardMaterial>,
    pub sauce: Handle<StandardMaterial>,
    pub white: Handle<StandardMaterial>,
    pub dark: Handle<StandardMaterial>,
}
/// How far either side of the sausage each half bun is, and where the eyes are (across, up).
pub const BUN: f32 = 0.023;
pub const EYES: (f32, f32) = EYE;
impl PlayerModels {
    /// The parts a hotdog is made of (see [`Food`]).
    pub fn food(&self) -> Food {
        Food {
            sausage: self.skin.clone(),
            bun: self.bun.clone(),
            mustard: self.mustard.clone(),
            eye: self.eye.clone(),
            skin: self.skin_material.clone(),
            bread: self.bread.clone(),
            sauce: self.sauce.clone(),
            white: self.white.clone(),
            dark: self.dark.clone(),
        }
    }
}
/// The meshes of a hotdog's body and face, as finely made as `detailed` says: the detailed set for a GPU at the higher
/// levels, the lean set (about a quarter of the triangles) for a CPU renderer and for the Low and Lowest levels
/// (`quality::Detail::lean`).
struct Bodies {
    skin: Mesh,
    bun: Mesh,
    mustard: Mesh,
    eye: Mesh,
    limb: Mesh,
    badge: Mesh,
    smile: Mesh,
    gape: Mesh,
    brows: [Mesh; 2],
    cheeks: [Mesh; 2],
}
fn bodies(detailed: bool) -> Bodies {
    let level = |high, low| if detailed { high } else { low };
    let skin = surface(
        level(128, 64),
        level(64, 32),
        false,
        |u, v| {
            let y = 0.023 + u * 0.155;
            let t = (u * 2. - 1.).abs();
            let cap = (1. - ((t - 0.76) / 0.24).max(0.).powi(2))
                .max(0.0001)
                .sqrt();
            let angle = v * TAU;
            let radius = 0.023 * cap * (1. + 0.025 * (angle * 11. + u * 37.).sin() * t.powi(8));
            Vec3::new(
                0.002 * (u * PI).sin() + radius * angle.cos(),
                y,
                0.002 * (u * TAU).sin() + radius * angle.sin(),
            )
        },
        |u, _| {
            let toast = (u * 2. - 1.).abs().powi(10);
            [1. - toast * 0.12, 1. - toast * 0.16, 1. - toast * 0.18, 1.]
        },
    );
    let bun = surface(
        level(80, 40),
        level(40, 20),
        false,
        |u, v| {
            let cap = (u * PI).sin().max(0.0001).sqrt();
            let angle = v * TAU;
            Vec3::new(
                0.017 * cap * angle.cos(),
                0.031 + u * 0.132,
                0.026 * cap * angle.sin() + 0.012,
            )
        },
        |_, v| {
            let toast = (v * TAU).sin().max(0.);
            [1., 1. - toast * 0.12, 1. - toast * 0.22, 1.]
        },
    );
    let mustard = surface(
        level(96, 64),
        level(12, 8),
        false,
        |u, v| {
            Vec3::new(
                (u * 8. * PI).sin() * 0.010 + (v * TAU).cos() * 0.002,
                // The squiggle stops under the chin: the face above it is for the eyes and the mouth.
                0.047 + u * 0.076,
                -0.024 + (v * TAU).sin() * 0.002,
            )
        },
        |_, _| [1.; 4],
    );
    Bodies {
        skin,
        bun,
        mustard,
        eye: Sphere::new(1.).mesh().uv(level(24, 16), level(16, 12)),
        limb: Capsule3d::new(0.004, 0.010).mesh().longitudes(level(16, 12)).latitudes(level(10, 8)).build(),
        badge: Torus::new(0.026, 0.028).mesh().major_resolution(level(64, 32) as usize).minor_resolution(level(12, 8) as usize).build(),
        smile: smile(level(36, 24), level(10, 8)),
        gape: gape(level(24, 16), level(10, 8)),
        brows: [-1., 1.].map(|side| brow(side, level(20, 14), level(8, 6))),
        cheeks: [-1., 1.].map(|side| cheek(side, level(8, 6), level(28, 20))),
    }
}
impl PlayerModels {
    /// Remakes the bodies' meshes in place, as finely as `detailed` says but never finer than the device was given (a
    /// CPU renderer always has the lean set): every hotdog shares them, so every hotdog changes at once. Returns
    /// whether anything changed.
    pub fn refine(&mut self, meshes: &mut Assets<Mesh>, detailed: bool) -> bool {
        let detailed = detailed && self.device_detailed;
        if detailed == self.detailed {
            return false;
        }
        self.detailed = detailed;
        let made = bodies(detailed);
        for (handle, mesh) in [
            (&self.skin, made.skin),
            (&self.bun, made.bun),
            (&self.mustard, made.mustard),
            (&self.eye, made.eye),
            (&self.limb, made.limb),
            (&self.badge, made.badge),
            (&self.smile, made.smile),
            (&self.gape, made.gape),
        ]
        .into_iter()
        .chain(self.brows.iter().zip(made.brows))
        .chain(self.cheeks.iter().zip(made.cheeks))
        {
            let _ = meshes.insert(handle, mesh);
        }
        true
    }
    /// Whether the bodies are the detailed set.
    pub fn detailed(&self) -> bool {
        self.detailed
    }
}
/// Keeps the hotdogs' meshes as fine as the graphics level says (`quality::Detail::lean`).
pub fn follow_detail(
    detail: Option<Res<crate::quality::Detail>>,
    models: Option<ResMut<PlayerModels>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (Some(detail), Some(mut models)) = (detail, models) else {
        return;
    };
    if models.detailed != (!detail.lean && models.device_detailed) {
        models.refine(&mut meshes, !detail.lean);
    }
}
impl PlayerModels {
    pub fn new(
        meshes: &mut Assets<Mesh>,
        mats: &mut Assets<StandardMaterial>,
        server: &AssetServer,
        detailed: bool,
    ) -> Self {
        let made = bodies(detailed);
        let colors = kitchen_sim::feast::COLORS
            .iter()
            .map(|c| {
                mats.add(StandardMaterial {
                    base_color: Color::srgb(c[0], c[1], c[2]),
                    emissive: LinearRgba::rgb(c[0] * 0.2, c[1] * 0.2, c[2] * 0.2),
                    perceptual_roughness: 0.4,
                    ..default()
                })
            })
            .collect();
        let [brow_left, brow_right] = made.brows;
        let [cheek_left, cheek_right] = made.cheeks;
        Self {
            skin: meshes.add(made.skin),
            bun: meshes.add(made.bun),
            mustard: meshes.add(made.mustard),
            eye: meshes.add(made.eye),
            limb: meshes.add(made.limb),
            badge: meshes.add(made.badge),
            detailed,
            device_detailed: detailed,
            skin_material: food_material(server, mats, "sausage", 0.48),
            bread: food_material(server, mats, "crust", 0.85),
            sauce: mats.add(StandardMaterial {
                base_color: Color::srgb(0.97, 0.67, 0.04),
                perceptual_roughness: 0.3,
                ..default()
            }),
            white: mats.add(Color::srgb(0.97, 0.94, 0.84)),
            dark: mats.add(Color::srgb(0.025, 0.012, 0.009)),
            stim: stim::Look::new(meshes, mats, detailed),
            weapons: crate::weapons::Kit::new(meshes, mats),
            kungfu: crate::kungfu::rig::Look::new(meshes),
            colors,
            smile: meshes.add(made.smile),
            gape: meshes.add(made.gape),
            brows: [meshes.add(brow_left), meshes.add(brow_right)],
            cheeks: [meshes.add(cheek_left), meshes.add(cheek_right)],
            // The smile, the brows and the cheeks are lit a little from within (as the rings are), so that a face still
            // reads in the dark of the fridge.
            brown: mats.add(StandardMaterial {
                base_color: Color::srgb(0.30, 0.10, 0.075),
                emissive: LinearRgba::rgb(0.30 * 0.2, 0.10 * 0.2, 0.075 * 0.2),
                perceptual_roughness: 0.55,
                reflectance: 0.3,
                ..default()
            }),
            // Its colour is in the mesh: dark, and the tongue.
            inside: mats.add(StandardMaterial { perceptual_roughness: 0.6, reflectance: 0.3, ..default() }),
            blush: mats.add(StandardMaterial {
                base_color: Color::srgba(1., 0.40, 0.48, 0.8),
                emissive: LinearRgba::rgb(0.2, 0.40 * 0.2, 0.48 * 0.2),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.6,
                ..default()
            }),
            // As the colours of the places are worn (`colors`), lit a little from within.
            paints: PAINT_RGB.map(|c| {
                mats.add(StandardMaterial {
                    base_color: Color::srgb(c[0], c[1], c[2]),
                    emissive: LinearRgba::rgb(c[0] * 0.2, c[1] * 0.2, c[2] * 0.2),
                    perceptual_roughness: 0.4,
                    ..default()
                })
            }),
        }
    }
    /// The colour hotdog `id`'s legs wear (its place's).
    pub(crate) fn livery(&self, id: u32) -> Handle<StandardMaterial> {
        self.colors[(id.max(1) as usize - 1) % 5].clone()
    }
    pub fn spawn(&self, world: &mut World, p: Vec3, id: u32) -> Entity {
        let color = &self.colors[(id as usize - 1) % 5];
        let root = world
            .spawn((
                PlayerBody,
                Name::new(format!("HD hotdog {} | original PBR character", id)),
                Transform::from_translation(p),
                Visibility::default(),
            ))
            .id();
        // The torso pivots on the hips so the body can lean, bob, twist and lie prone over the legs; the head nods on the
        // torso (bend.rs), and the face hangs from it.
        let torso = world.spawn((Transform::default(), Visibility::default())).id();
        world.entity_mut(root).add_child(torso);
        let head = world.spawn((Name::new("Head"), Transform::default(), Visibility::default())).id();
        world.entity_mut(torso).add_child(head);
        // The sausage, the buns and the mustard are drawn bent by the nod, in their own materials' bent copies (bend.rs),
        // with room in their bounds for the head wherever it nods.
        let bent = [&self.skin_material, &self.bread, &self.sauce].map(|straight| bend::bent(world, straight).map(|b| (b, straight.clone())));
        let mut bent_part = |mesh: &Handle<Mesh>, k: usize, straight: &Handle<StandardMaterial>, tr: Transform| {
            let e = world.spawn((Mesh3d(mesh.clone()), tr)).id();
            match &bent[k] {
                Some((b, straight)) => {
                    let room = bevy::camera::primitives::Aabb::from_min_max(Vec3::new(-0.05, 0.0, -0.085), Vec3::new(0.05, 0.19, 0.085));
                    world.entity_mut(e).insert((MeshMaterial3d(b.clone()), bend::Unbent(straight.clone()), bend::tag(0.), room, bevy::camera::visibility::NoAutoAabb));
                }
                None => {
                    world.entity_mut(e).insert(MeshMaterial3d(straight.clone()));
                }
            }
            world.entity_mut(torso).add_child(e);
            e
        };
        let sausage = bent_part(&self.skin, 0, &self.skin_material, Transform::default());
        // (Kung Fu shakes the buns and the mustard, and works the eyes, the brows and the cheeks: kungfu/rig.rs.)
        let mut buns = [Entity::PLACEHOLDER; 2];
        for (i, side) in [-1., 1.].into_iter().enumerate() {
            buns[i] = bent_part(&self.bun, 1, &self.bread, Transform::from_xyz(side * 0.023, 0., 0.));
        }
        let mustard = bent_part(&self.mustard, 2, &self.sauce, Transform::default());
        drop(bent_part);
        let mut part = |parent: Entity, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, tr: Transform| {
            let e = world
                .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), tr))
                .id();
            world.entity_mut(parent).add_child(e);
            e
        };
        let mut eyes = [[Entity::PLACEHOLDER; 3]; 2];
        for (i, side) in [-1., 1.].into_iter().enumerate() {
            eyes[i][0] = part(
                head,
                &self.eye,
                &self.white,
                Transform::from_xyz(side * EYE.0, EYE.1, -0.022)
                    .with_scale(Vec3::new(0.007, 0.009, 0.0035)),
            );
            eyes[i][1] = part(
                head,
                &self.eye,
                &self.dark,
                Transform::from_xyz(side * EYE.0, EYE.1, -0.0255)
                    .with_scale(Vec3::new(0.0035, 0.005, 0.002)),
            );
            eyes[i][2] = part(
                head,
                &self.eye,
                &self.white,
                Transform::from_xyz(side * EYE.0 - 0.001, EYE.1 + 0.002, -0.027)
                    .with_scale(Vec3::splat(0.0015)),
            );
        }
        // The rest of the face, the same for everybody: the smile, the inside of the mouth under it (shut until `speak`
        // opens it), the brows and the cheeks.
        let gape = part(head, &self.gape, &self.inside, Transform::from_xyz(0., MOUTH, 0.).with_scale(Vec3::new(1., 0.02, 1.)));
        let mut face = vec![gape, part(head, &self.smile, &self.brown, Transform::default())];
        let (mut brows, mut cheeks) = ([Entity::PLACEHOLDER; 2], [Entity::PLACEHOLDER; 2]);
        for side in 0..2 {
            brows[side] = part(head, &self.brows[side], &self.brown, Transform::default());
            cheeks[side] = part(head, &self.cheeks[side], &self.blush, Transform::default());
            face.extend([brows[side], cheeks[side]]);
        }
        let badge = part(
            torso,
            &self.badge,
            color,
            Transform::from_xyz(0., 0.072, 0.).with_scale(Vec3::new(1.6, 0.5, 1.)),
        );
        drop(part);
        for feature in face {
            world.entity_mut(feature).insert(bevy::light::NotShadowCaster);
        }
        world.entity_mut(gape).insert(Visibility::Hidden);
        // Limbs are two capsules hinged at the knee or elbow, each hanging from its joint.
        let mut joint = |parent: Entity, at: Vec3, material: &Handle<StandardMaterial>| {
            let pivot = world
                .spawn((Transform::from_translation(at), Visibility::default()))
                .id();
            let bone = world
                .spawn((
                    Mesh3d(self.limb.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(0., -BONE * 0.5, 0.),
                ))
                .id();
            world.entity_mut(pivot).add_child(bone);
            world.entity_mut(parent).add_child(pivot);
            (pivot, bone)
        };
        let below = Vec3::NEG_Y * BONE;
        let mut limbs = [[(Entity::PLACEHOLDER, Entity::PLACEHOLDER); 4]; 2];
        for (i, side) in [-1., 1.].into_iter().enumerate() {
            let hip = joint(root, Vec3::new(side * HIP.x, HIP.y, 0.), color);
            let shoulder = joint(
                torso,
                Vec3::new(side * SHOULDER.x, SHOULDER.y, 0.),
                &self.skin_material,
            );
            limbs[i] = [
                hip,
                joint(hip.0, below, color),
                shoulder,
                joint(shoulder.0, below, &self.skin_material),
            ];
        }
        drop(joint);
        // The gun hand rides the end of the right forearm and carries every weapon; `animate`
        // shows the one in use.
        let hand = world
            .spawn((Transform::from_translation(below), Visibility::default()))
            .id();
        world.entity_mut(limbs[1][3].0).add_child(hand);
        let held = [0, 1, 2, 3, 4, 5, 6, 7].map(|weapon| {
            let placing = crate::weapons::in_hand();
            self.weapons.attach(world, hand, weapon, placing, Visibility::Hidden)
        });
        // The stim: blue fire and lightning from the top of the head to the feet, hidden until it is taken (stim.rs).
        let legs = [limbs[0][0].1, limbs[0][1].1, limbs[1][0].1, limbs[1][1].1];
        let arms = [limbs[0][2].1, limbs[0][3].1, limbs[1][2].1, limbs[1][3].1];
        let aura = self.stim.spawn(world, torso, [legs[0], legs[1], legs[2], legs[3], arms[0], arms[1], arms[2], arms[3]]);
        let made = crate::kungfu::rig::Made {
            torso,
            hips: [limbs[0][0].0, limbs[1][0].0],
            knees: [limbs[0][1].0, limbs[1][1].0],
            shoulders: [limbs[0][2].0, limbs[1][2].0],
            elbows: [limbs[0][3].0, limbs[1][3].0],
            arm_bones: [[limbs[0][2].1, limbs[0][3].1], [limbs[1][2].1, limbs[1][3].1]],
            leg_bones: [[limbs[0][0].1, limbs[0][1].1], [limbs[1][0].1, limbs[1][1].1]],
            gun: hand,
            brows,
            eyes,
            cheeks,
            gape,
            buns,
            mustard,
        };
        let feet = crate::kungfu::rig::attach(world, root, &self.kungfu, made, &self.skin_material, color);
        world.entity_mut(root).insert((
            Rig {
                torso,
                head,
                bent: [sausage, buns[0], buns[1], mustard],
                aura,
                hips: [limbs[0][0].0, limbs[1][0].0],
                knees: [limbs[0][1].0, limbs[1][1].0],
                shoulders: [limbs[0][2].0, limbs[1][2].0],
                elbows: [limbs[0][3].0, limbs[1][3].0],
                arms: [
                    [limbs[0][2].1, limbs[0][3].1],
                    [limbs[1][2].1, limbs[1][3].1],
                ],
                hand,
                held,
            },
            Gait {
                at: Vec2::ZERO,
                time: 0.,
                live: false,
                stim: false,
                motion: Motion {
                    clap: 1.,
                    swing: 1.,
                    weapon: 1,
                    ..default()
                },
                id,
                swings: 0,
                muzzle: p,
            },
            Face { gape, ring: badge, own: (id as usize - 1) % 5, talk: 0., time: 0. },
            Livery(vec![badge, limbs[0][0].1, limbs[0][1].1, limbs[1][0].1, limbs[1][1].1, feet[0], feet[1]]),
        ));
        root
    }
}
/// Third-person recoil: the gun arm levels and kicks; the relish bomb and the hot sauce stream
/// rock the whole body. It is called once for every round fired.
pub fn kick(world: &mut World, root: Entity, weapon: usize) {
    let (kick, rumble) = match weapon {
        1 => (0.3, 0.),
        2 => (0.55, 0.),
        3 => (0.8, 1.),
        4 => (1.4, 0.4),
        5 => (0.15, 0.3),
        6 => (0.18, 0.),
        // Each droplet of the mayonnaise is a squeeze: a little shiver of the arm, held while it flows.
        7 => (0.12, 0.),
        _ => return,
    };
    if let Some(mut gait) = world.get_mut::<Gait>(root) {
        // The syrup rifle's three rounds come 0.05 s apart, each before the last kick has died
        // away, so its kicks add up and the muzzle climbs through the burst; it settles again
        // quickly (`Gait::follow`), in the 0.30 s pause before the next.
        gait.motion.kick = if weapon == 6 { (gait.motion.kick + kick).min(0.6) } else { gait.motion.kick.max(kick) };
        gait.motion.rumble = gait.motion.rumble.max(rumble);
        gait.motion.aim = 1.;
    }
}
impl Gait {
    /// Follows the hotdog for one frame: its speed and direction from how far the root moved, the
    /// weapon and aim it was given, and the cues that start the stim clap and the melee strike.
    fn follow(&mut self, tr: &Transform, time: f32, cues: Cues) {
        let dt = time - self.time;
        let at = Vec2::new(tr.translation.x, tr.translation.z);
        let weapon = cues.weapon.min(7);
        // The nod the simulation has (head.rs, by the stance and the look), and in Kung Fu none (its moves own the body).
        let stance = if cues.prone { 2 } else if cues.crouch && !cues.airborne { 1 } else { 0 };
        let nod = if cues.alive && !cues.kungfu { kitchen_sim::head::nod(stance, cues.pitch) } else { 0. };
        self.motion.lean = cues.kungfu as u8 as f32;
        if !self.live || !(0.0..0.25).contains(&dt) {
            self.live = true;
            self.at = at;
            self.time = time;
            self.swings = cues.swings;
            // A hotdog first seen already on its stim neither claps nor fades in: the stim began before.
            self.stim = cues.stim > 0.;
            self.motion.pitch = cues.pitch;
            self.motion.nod = nod;
            self.motion.weapon = weapon;
            self.motion.aim = (weapon > 0) as u8 as f32;
            self.motion.sight = (cues.aimed && crate::weapons::sight(weapon).is_some()) as u8 as f32;
        } else if dt > 0.004 {
            let velocity = (at - self.at) / dt;
            let (forward, right) = (tr.rotation * Vec3::NEG_Z, tr.rotation * Vec3::X);
            let ease = |rate: f32| 1. - (-dt * rate).exp();
            let m = &mut self.motion;
            let along = (velocity.dot(Vec2::new(forward.x, forward.z)) / RUN).clamp(-PACE, PACE);
            let across = (velocity.dot(Vec2::new(right.x, right.z)) / RUN).clamp(-PACE, PACE);
            m.forward += (along - m.forward) * ease(10.);
            m.strafe += (across - m.strafe) * ease(10.);
            let drive = m.forward.hypot(m.strafe).min(1.);
            // Backpedalling runs the stride in reverse.
            let back = m.forward < -0.05 && m.forward.abs() > m.strafe.abs();
            // (Past a full run the stride is as long as it gets, and turns over faster with the pace.)
            let cadence = (9. + 15. * drive) * smooth(drive / 0.12) * m.forward.hypot(m.strafe).max(1.);
            m.phase = (m.phase + dt * cadence * if back { -1. } else { 1. }).rem_euclid(TAU);
            // (The syrup rifle's tight kick settles quicker than the rest.)
            m.kick *= (-dt * if weapon == 6 { 10. } else { 6. }).exp();
            m.rumble *= (-dt * 8.).exp();
            // A gun is always held out along the aim; with the hands full the arms swing free.
            m.aim += ((weapon > 0) as u8 as f32 - m.aim) * ease(20.);
            let sighting = cues.aimed && cues.alive && !cues.carry && crate::weapons::sight(weapon).is_some();
            m.sight = (m.sight + dt / SIGHTING * if sighting { 1. } else { -1. }).clamp(0., 1.);
            m.weapon = weapon;
            m.pitch += (cues.pitch - m.pitch) * ease(14.);
            m.nod = bend::ease(m.nod, nod, dt);
            // (A blow that lands stands still on contact, as the first-person one does: whip.rs.)
            if m.held > 0. {
                m.held -= dt;
            } else {
                m.swing = (m.swing + dt / crate::whip::TOTAL).min(1.);
            }
            if let Some(backstab) = m.struck.filter(|_| !m.felt && (crate::whip::CONTACT..1.).contains(&m.swing)) {
                m.felt = true;
                if m.swing < crate::whip::CONTACT + 0.12 {
                    m.swing = crate::whip::CONTACT;
                    m.held = if backstab { crate::whip::STOP_STAB } else { crate::whip::STOP };
                }
            }
            m.cheer += (cues.cheer as u8 as f32 - m.cheer) * ease(8.);
            m.prone += (cues.prone as u8 as f32 - m.prone) * ease(9.);
            m.crouch += ((cues.crouch && !cues.prone && !cues.airborne) as u8 as f32 - m.crouch) * ease(12.);
            m.tuck += ((cues.crouch && !cues.prone && cues.airborne) as u8 as f32 - m.tuck) * ease(16.);
            m.carry += (cues.carry as u8 as f32 - m.carry) * ease(12.);
            m.reload = cues.reload;
            m.clap = (m.clap + dt / CLAP).min(1.);
            m.time = time;
            if cues.stim > 0. && !self.stim {
                self.motion.clap = 0.;
            }
            // Every melee attack the simulation counts (a strike that hits or misses, a bite) swings
            // whatever is in the gun hand, once.
            if cues.swings != self.swings {
                self.motion.swing = 0.;
                (self.motion.held, self.motion.felt, self.motion.struck) = (0., false, None);
            }
            self.swings = cues.swings;
            self.stim = cues.stim > 0.;
            self.at = at;
            self.time = time;
        }
    }
}
/// Poses one hotdog for this frame. Speed and direction come from how far its root moved since the
/// last call, measured against the way it faces, so a hotdog that moves sideways strafes.
pub fn animate(world: &mut World, root: Entity, time: f32, cues: Cues) {
    let (Some(tr), Some(rig)) = (
        world.get::<Transform>(root).copied(),
        world.get::<Rig>(root).cloned(),
    ) else {
        return;
    };
    // Whether the swing it is in the middle of landed a blow, by the simulation's word (whip.rs).
    let struck = world.get::<Gait>(root).and_then(|gait| world.get_resource::<crate::whip::Fx>().and_then(|fx| fx.struck(gait.id, cues.swings)));
    let Some(mut gait) = world.get_mut::<Gait>(root) else {
        return;
    };
    gait.motion.struck = struck.or(gait.motion.struck);
    gait.follow(&tr, time, cues);
    let weapon = cues.weapon.min(7);
    let (motion, id) = (gait.motion, gait.id);
    let posed = cues.alive.then(|| pose(motion));
    // (Written only where it changes: a body lying still keeps its whole tree out of the transform propagation.)
    let mut set = |e: Entity, at: Option<Vec3>, turn: Quat| {
        if let Some(mut tr) = world.get_mut::<Transform>(e) {
            if tr.rotation != turn {
                tr.rotation = turn;
            }
            if let Some(at) = at.filter(|at| *at != tr.translation) {
                tr.translation = at;
            }
        }
    };
    if let Some(p) = &posed {
        let pivot = Vec3::Y * HIP.y;
        set(rig.torso, Some(pivot + p.shift - p.torso * pivot), p.torso);
        for (i, side) in [-1., 1.].into_iter().enumerate() {
            set(rig.hips[i], Some(Vec3::new(side * HIP.x, HIP.y, 0.) + p.shift), p.hips[i]);
            set(rig.knees[i], None, p.knees[i]);
            set(rig.shoulders[i], None, p.shoulders[i]);
            set(rig.elbows[i], Some(Vec3::NEG_Y * BONE * p.reach[i]), p.elbows[i]);
        }
        set(rig.hand, Some(Vec3::NEG_Y * BONE * p.reach[1]), p.wrist);
    } else {
        // A fallen hotdog lies still with its limbs splayed.
        set(rig.torso, Some(Vec3::ZERO), Quat::IDENTITY);
        for (i, side) in [-1., 1.].into_iter().enumerate() {
            set(rig.hips[i], Some(Vec3::new(side * HIP.x, HIP.y, 0.)), Quat::from_rotation_z(side * 0.9));
            set(rig.shoulders[i], None, Quat::from_rotation_z(side * 0.9));
            set(rig.knees[i], None, Quat::IDENTITY);
            set(rig.elbows[i], Some(Vec3::NEG_Y * BONE), Quat::IDENTITY);
        }
    }
    drop(set);
    nod(world, &rig, motion.nod);
    let reach = posed.map_or([1.; 2], |p| p.reach);
    for (i, bones) in rig.arms.iter().enumerate() {
        for bone in bones {
            if let Some(mut tr) = world.get_mut::<Transform>(*bone) {
                if tr.scale.y != reach[i] || tr.translation.y != -BONE * 0.5 * reach[i] {
                    tr.scale.y = reach[i];
                    tr.translation.y = -BONE * 0.5 * reach[i];
                }
            }
        }
    }
    // The hand shows the weapon in use; a fallen hotdog's weapon is gone. (Written only when it changes: every write
    // has the engine work out the visibility of the whole weapon again.)
    for (i, model) in rig.held.iter().enumerate() {
        let wanted = if cues.alive && i == weapon { Visibility::Inherited } else { Visibility::Hidden };
        if let Some(mut shown) = world.get_mut::<Visibility>(*model) {
            shown.set_if_neq(wanted);
        }
    }
    if let (Some(p), Some(mut gait)) = (&posed, world.get_mut::<Gait>(root)) {
        gait.muzzle = tr.transform_point(grip(p, weapon).1);
    }
    // The stim: blue fire and lightning from head to toe, up as the clap starts it and down over its last half second.
    if world.contains_resource::<PlayerModels>() {
        let strength = stim::strength(cues.stim, motion.clap * CLAP);
        let level = world.get_resource::<crate::quality::Detail>().map_or(crate::quality::Level::High, |d| d.level);
        world.resource_scope(|world, models: Mut<PlayerModels>| {
            stim::animate(world, &models.stim, &rig.aura, id, time, strength, posed.as_ref(), level, motion.nod);
        });
    }
}
/// Puts a hotdog's head where a nod of `nod` bends it (bend.rs): the head on the torso, and the nod in the tag of each
/// bent part. Only for a hotdog that was seen last frame (the parts' own visibility: hidden in first person, off the
/// screen), and only when it has changed.
fn nod(world: &mut World, rig: &Rig, nod: f32) {
    let seen = world.get::<ViewVisibility>(rig.bent[0]).is_none_or(|v| v.get());
    let was = world.get::<bevy::mesh::MeshTag>(rig.bent[0]).map_or(0, |t| t.0);
    if !seen || was == nod.to_bits() {
        return;
    }
    let tag = bend::tag(nod);
    for part in rig.bent {
        match world.get_mut::<bevy::mesh::MeshTag>(part) {
            Some(mut t) => t.0 = tag.0,
            None => {
                world.entity_mut(part).insert(tag.clone());
            }
        }
    }
    if let Some(mut tr) = world.get_mut::<Transform>(rig.head) {
        *tr = bend::head_transform(nod);
    }
}
/// Where the weapon held by hotdog `id` ends, in world space, as last drawn. Rounds are shown
/// leaving from here rather than from the eye the simulation fires from.
pub fn muzzle_of(world: &mut World, id: u32) -> Option<Vec3> {
    world
        .query::<&Gait>()
        .iter(world)
        .find(|gait| gait.id == id && gait.live)
        .map(|gait| gait.muzzle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::whip::{CONTACT, FOLLOWED, WOUND};
    fn feet(p: &Pose) -> [Vec3; 2] {
        [-1., 1.].map(|side: f32| {
            let i = (side > 0.) as usize;
            limb(Vec3::new(side * HIP.x, HIP.y, 0.) + p.shift, p.hips[i], p.knees[i], BONE).1
        })
    }
    /// Elbows and hands in the hotdog's own frame.
    fn arms(p: &Pose) -> [(Vec3, Vec3); 2] {
        let pivot = Vec3::Y * HIP.y;
        [-1., 1.].map(|side: f32| {
            let i = (side > 0.) as usize;
            let joint = Vec3::new(side * SHOULDER.x, SHOULDER.y, 0.);
            let (elbow, hand) = limb(joint, p.shoulders[i], p.elbows[i], BONE * p.reach[i]);
            let place = |v: Vec3| pivot + p.shift + p.torso * (v - pivot);
            (place(elbow), place(hand))
        })
    }
    fn cycle() -> impl Iterator<Item = f32> {
        (0..64).map(|i| i as f32 / 64. * TAU)
    }
    #[test]
    fn running_feet_stay_on_the_ground_and_alternate() {
        for forward in [0.5, 1., -0.6] {
            let mut lowest = f32::MAX;
            for phase in cycle() {
                let p = pose(Motion { phase, forward, ..default() });
                let [left, right] = feet(&p);
                assert!(left.y > -1e-4 && right.y > -1e-4, "a foot sank at phase {phase}");
                lowest = lowest.min(left.y.min(right.y));
            }
            assert!(lowest < 0.004, "no foot ever reaches the ground: {lowest}");
        }
        let p = pose(Motion { phase: PI * 0.5, forward: 1., ..default() });
        let [left, right] = feet(&p);
        assert!(left.z < -0.01 && right.z > 0.005, "legs are half a stride apart");
        let [(_, left_hand), (_, right_hand)] = arms(&p);
        assert!(left_hand.z > right_hand.z, "each arm swings against the leg on its side");
        assert!(p.torso.mul_vec3(Vec3::Y).z < -0.1, "a runner leans forward");
    }
    #[test]
    fn a_run_lifts_the_knees_higher_than_a_walk() {
        let lift = |forward| {
            cycle()
                .map(|phase| feet(&pose(Motion { phase, forward, ..default() }))[0].y)
                .fold(0., f32::max)
        };
        assert!(lift(1.) > lift(0.4) + 0.004);
        assert!(lift(0.) < 0.002, "standing still keeps both feet planted");
    }
    #[test]
    fn strafing_left_and_right_are_distinct_mirror_images() {
        for phase in cycle() {
            let right = pose(Motion { phase, strafe: 1., ..default() });
            let left = pose(Motion { phase: phase + PI, strafe: -1., ..default() });
            let (a, b) = (feet(&right), feet(&left));
            for (x, y) in [(a[1], b[0]), (a[0], b[1])] {
                assert!((x - Vec3::new(-y.x, y.y, y.z)).length() < 1e-4);
                assert!(x.y > -1e-4);
            }
            assert!(right.torso.mul_vec3(Vec3::Y).x > 0.1, "leans into a right strafe");
            assert!(left.torso.mul_vec3(Vec3::Y).x < -0.1, "leans into a left strafe");
        }
        let sideways = cycle()
            .map(|phase| feet(&pose(Motion { phase, strafe: 1., ..default() }))[1].x)
            .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        assert!(sideways.1 - sideways.0 > 0.012, "the legs step sideways");
    }
    #[test]
    fn arms_open_wide_for_strafes_and_victory() {
        let span = |m: Motion| {
            let [(_, left), (_, right)] = arms(&pose(m));
            (right.x - left.x, left.y.min(right.y))
        };
        let rest = span(Motion::default());
        let open = span(Motion { open: 1., ..default() });
        let strafe = span(Motion { strafe: 1., ..default() });
        let cheer = span(Motion { cheer: 1., ..default() });
        assert!(open.0 > rest.0 + 0.03 && open.1 > 0.085, "open {open:?} rest {rest:?}");
        assert!(strafe.0 > rest.0 + 0.015);
        assert!(cheer.1 > SHOULDER.y, "a winner raises both hands above the shoulders");
    }
    #[test]
    fn recoil_throws_the_gun_arm_up_and_rocks_the_body_back() {
        let aim = pose(Motion { aim: 1., ..default() });
        let hand = arms(&aim)[1].1;
        assert!(hand.z < -0.025 && (hand.y - SHOULDER.y).abs() < 0.012, "the gun arm levels: {hand}");
        let bomb = pose(Motion { aim: 1., kick: 1.4, ..default() });
        assert!(arms(&bomb)[1].1.y > hand.y + 0.01);
        assert!(bomb.torso.mul_vec3(Vec3::Y).z > 0.3, "a relish bomb rocks the body back");
        let pistol = pose(Motion { aim: 1., kick: 0.3, ..default() });
        assert!(pistol.torso.mul_vec3(Vec3::Y).z < bomb.torso.mul_vec3(Vec3::Y).z * 0.4);
    }
    #[test]
    fn prone_lies_flat_and_crawls() {
        let p = pose(Motion { prone: 1., ..default() });
        let pivot = Vec3::Y * HIP.y;
        let head = pivot + p.shift + p.torso * (Vec3::Y * 0.178 - pivot);
        assert!(head.y < 0.07 && head.z < -0.05, "head {head}");
        assert!(feet(&p).iter().all(|f| f.y < 0.05 && f.y > 0. && f.z > 0.08));
        assert!(arms(&p).iter().all(|(_, hand)| hand.y > 0. && hand.y < 0.06));
        let reach = |phase| arms(&pose(Motion { prone: 1., forward: 0.4, phase, ..default() }))[0].1.z;
        assert!((reach(PI * 0.5) - reach(PI * 1.5)).abs() > 0.004, "the arms take turns reaching");
    }
    #[test]
    fn tucked_in_the_air_the_knees_come_up_under_a_body_that_stays_where_the_jump_put_it() {
        let pivot = Vec3::Y * HIP.y;
        let head = |p: &Pose| pivot + p.shift + p.torso * (Vec3::Y * 0.178 - pivot);
        let (up, tucked) = (pose(Motion::default()), pose(Motion { tuck: 1., ..default() }));
        assert!(head(&up).distance(head(&tucked)) < 1e-6 && tucked.shift.length() < 1e-6, "the head and the hips stay");
        let (low, high) = (feet(&up), feet(&tucked));
        for i in 0..2 {
            assert!(high[i].y > low[i].y + 0.02 && (high[i].z - low[i].z).abs() < 0.005, "the feet come up under the hips: {} to {}", low[i], high[i]);
        }
        // Prone, nothing tucks.
        let (prone, both) = (pose(Motion { prone: 1., ..default() }), pose(Motion { prone: 1., tuck: 1., ..default() }));
        assert!(feet(&prone)[0].distance(feet(&both)[0]) < 1e-6);
    }
    #[test]
    fn crouching_folds_the_legs_and_lowers_the_body_and_a_crouch_walk_keeps_its_feet_on_the_ground() {
        let pivot = Vec3::Y * HIP.y;
        let head = |p: &Pose| pivot + p.shift + p.torso * (Vec3::Y * 0.178 - pivot);
        let (up, down) = (pose(Motion::default()), pose(Motion { crouch: 1., ..default() }));
        // The crown comes down by about what the body's capsule loses (0.15 m to 0.10 m), the hips sink,
        // and the knees fold forward over feet that are still under the hips and on the ground.
        assert!(head(&up).y - head(&down).y > 0.028 && head(&down).y > 0.12, "crown {} to {}", head(&up).y, head(&down).y);
        assert!(down.shift.y < -0.012 && down.torso.mul_vec3(Vec3::Y).z < -0.3);
        for (i, side) in [-1f32, 1.].into_iter().enumerate() {
            let hip = Vec3::new(side * HIP.x, HIP.y, 0.) + down.shift;
            let (knee, foot) = limb(hip, down.hips[i], down.knees[i], BONE);
            assert!(knee.z < hip.z - 0.01 && knee.y < hip.y, "the knee is forward of the hip: {knee}");
            assert!(foot.y > -1e-4 && foot.y < 0.002 && (foot.z - hip.z).abs() < 1e-4, "the foot is under the hip, on the ground: {foot}");
        }
        // Halfway down is halfway between, and a prone hotdog does not crouch.
        let half = pose(Motion { crouch: 0.5, ..default() });
        assert!(head(&half).y < head(&up).y - 0.005 && head(&half).y > head(&down).y + 0.005);
        let (prone, both) = (pose(Motion { prone: 1., ..default() }), pose(Motion { prone: 1., crouch: 1., ..default() }));
        assert!(head(&prone).distance(head(&both)) < 1e-6 && feet(&prone)[0].distance(feet(&both)[0]) < 1e-6);
        // The crouch-walk: the legs still step, shorter than a run's, and no foot goes through the counter.
        let (mut lowest, mut stride, mut run) = (f32::MAX, (f32::MAX, f32::MIN), (f32::MAX, f32::MIN));
        for phase in cycle() {
            for (forward, strafe) in [(0.65, 0.), (-0.5, 0.), (0., 0.65), (0., -0.65)] {
                let p = pose(Motion { phase, forward, strafe, crouch: 1., ..default() });
                let [left, right] = feet(&p);
                assert!(left.y > -1e-4 && right.y > -1e-4, "a foot sank at phase {phase}");
                assert!(head(&p).y < head(&up).y - 0.025, "the body stays down while it walks");
                lowest = lowest.min(left.y.min(right.y));
            }
            let z = feet(&pose(Motion { phase, forward: 0.65, crouch: 1., ..default() }))[0].z;
            stride = (stride.0.min(z), stride.1.max(z));
            let z = feet(&pose(Motion { phase, forward: 0.65, ..default() }))[0].z;
            run = (run.0.min(z), run.1.max(z));
        }
        assert!(lowest < 0.004, "no foot reaches the ground: {lowest}");
        assert!(stride.1 - stride.0 > 0.006 && stride.1 - stride.0 < run.1 - run.0, "crouched stride {stride:?}, upright {run:?}");
        // The gun is still held out along the aim.
        let p = pose(Motion { crouch: 1., weapon: 1, aim: 1., ..default() });
        assert!((p.torso * p.shoulders[1] * p.elbows[1] * p.wrist * Vec3::NEG_Y).dot(Vec3::NEG_Z) > 0.985);
    }
    #[test]
    fn stim_claps_the_right_hand_on_the_left_forearm() {
        for clap in [0., 1.] {
            let (rest, p) = (pose(Motion::default()), pose(Motion { clap, ..default() }));
            assert!(rest.shoulders[1].angle_between(p.shoulders[1]) < 1e-4);
        }
        let gap = |clap| {
            let [(elbow, wrist), (_, hand)] = arms(&pose(Motion { clap, ..default() }));
            let along = ((hand - elbow).dot(wrist - elbow) / (wrist - elbow).length_squared()).clamp(0., 1.);
            (hand.distance(elbow.lerp(wrist, along)), along, elbow, wrist)
        };
        let (apart, ..) = gap(0.28);
        let (contact, along, elbow, wrist) = gap(0.5);
        assert!(apart > 0.03, "the hand is raised before the slap: {apart}");
        assert!(contact < 0.012, "the hand lands on the forearm: {contact}");
        assert!(along > 0.2, "on the forearm, not the elbow");
        assert!(elbow.x < 0. && wrist.z < -0.03, "the left forearm is in front of the chest");
        assert!((wrist.y - elbow.y).abs() < 0.02, "and roughly level");
    }
    #[test]
    fn reach_puts_the_hand_on_its_target() {
        let (shoulder, target) = (Vec3::new(0.05, 0.1, 0.), Vec3::new(0.02, 0.08, -0.03));
        let (root, bend) = reach_for(shoulder, target, 0.03, Vec3::new(0.5, -1., 0.));
        let (elbow, hand) = limb(shoulder, root, bend, 0.03);
        assert!(hand.distance(target) < 1e-4);
        assert!((elbow.distance(shoulder) - 0.03).abs() < 1e-5);
    }
    fn every_pose() -> Vec<Motion> {
        let mut all = vec![];
        for phase in cycle().step_by(8) {
            for base in [
                Motion { forward: 1., ..default() },
                Motion { forward: -0.6, ..default() },
                Motion { strafe: 1., ..default() },
                Motion { strafe: -1., ..default() },
                Motion { prone: 1., forward: 0.4, ..default() },
                Motion { crouch: 1., forward: 0.65, ..default() },
                Motion { crouch: 1., strafe: -0.65, ..default() },
                Motion { cheer: 1., ..default() },
                Motion { kick: 1.4, rumble: 1., ..default() },
                Motion { clap: 0.5, ..default() },
                Motion { swing: 0.35, ..default() },
            ] {
                all.push(Motion { phase, time: phase, ..base });
            }
        }
        all
    }
    #[test]
    fn the_weapon_stays_in_the_hand_in_every_pose() {
        for weapon in 0..8 {
            // A gun is held out along the aim; with the hands full the arms swing free.
            let aim = (weapon > 0) as u8 as f32;
            for m in every_pose() {
                let p = pose(Motion { weapon, aim, ..m });
                let (hand, tip) = grip(&p, weapon);
                let wrist = arms(&p)[1].1;
                assert!(hand.distance(wrist) < 1e-5, "the hand left the forearm: {m:?}");
                // The weapon is rigid: its tip never drifts from the hand that holds it.
                assert!((hand.distance(tip) - muzzle(weapon).length()).abs() < 1e-5);
                assert!(tip.y > 0.004, "weapon {weapon} is pushed into the counter: {m:?}");
            }
        }
    }
    #[test]
    fn a_gun_points_along_the_aim_whatever_the_body_does() {
        for weapon in [1, 2, 3, 4, 6, 7] {
            for m in every_pose() {
                if m.cheer > 0. || m.clap > 0. || m.swing > 0. || m.kick > 0. {
                    continue;
                }
                let p = pose(Motion { weapon, aim: 1., ..m });
                let (_, tip) = grip(&p, weapon);
                let barrel = p.torso * p.shoulders[1] * p.elbows[1] * p.wrist * Vec3::NEG_Y;
                assert!(barrel.dot(Vec3::NEG_Z) > 0.985, "weapon {weapon} points {barrel} in {m:?}");
                // The top of the gun stays up: the sights are not rolled over.
                let top = p.torso * p.shoulders[1] * p.elbows[1] * p.wrist * Vec3::NEG_Z;
                assert!(top.y > 0.9, "weapon {weapon} is rolled over in {m:?}");
                assert!(tip.z < -0.06, "the muzzle is out in front of the body");
            }
        }
        // Recoil swings the muzzle up from the aim and it comes back down as the kick decays.
        let climb = |kick| {
            let (hand, tip) = grip(&pose(Motion { weapon: 1, aim: 1., kick, ..default() }), 1);
            (tip - hand).normalize().y
        };
        assert!(climb(1.4) > 0.8 && climb(0.3) > 0.2 && climb(0.3) < climb(1.4) && climb(0.) < 0.2);
    }
    #[test]
    fn every_weapon_has_its_own_melee_strike_continuous_and_driven_forward() {
        // The gun hand, the way the weapon points and the free hand, in the hotdog's own frame.
        let at = |weapon: usize, swing: f32| {
            let p = pose(Motion { weapon, aim: (weapon > 0) as u8 as f32, swing, ..default() });
            let barrel = p.torso * p.shoulders[1] * p.elbows[1] * p.wrist * Vec3::NEG_Y;
            (grip(&p, weapon).0, barrel, arms(&p)[0].1, p)
        };
        let mut landed = vec![];
        for weapon in 0..8 {
            let rest = at(weapon, 0.);
            assert!(at(weapon, 1.).0.distance(rest.0) < 1e-6 && at(weapon, 1.).1.distance(rest.1) < 1e-5, "weapon {weapon} ends at rest");
            // One step is a millisecond of the 0.39 s strike: fast at the blow, but never a jump.
            let mut last = rest;
            for i in 0..=400 {
                let now = at(weapon, i as f32 / 400.);
                assert!(now.0.distance(last.0) < 0.006, "weapon {weapon}: the hand jumps at {i}");
                assert!(now.1.angle_between(last.1) < 0.25, "weapon {weapon} spins at {i}");
                assert!(now.2.distance(last.2) < 0.006, "weapon {weapon}: the free hand jumps at {i}");
                last = now;
            }
            let (cocked, hit) = (at(weapon, WOUND), at(weapon, CONTACT));
            assert!(hit.0.z < cocked.0.z - 0.04 && hit.0.z < -0.045, "weapon {weapon} is driven forward: {} from {}", hit.0, cocked.0);
            let speed = |u: f32| at(weapon, u + 0.004).0.distance(at(weapon, u).0) / 0.004;
            let fastest = (0..400).map(|i| i as f32 / 400.).map(|u| (u, speed(u))).fold((0., 0.), |a, b| if b.1 > a.1 { b } else { a });
            assert!((0.38..=CONTACT + 0.01).contains(&fastest.0), "weapon {weapon} is fastest at {}", fastest.0);
            assert!(hit.3.torso.mul_vec3(Vec3::Y).z < -0.12, "the body goes with the blow of weapon {weapon}");
            landed.push(hit);
        }
        for a in 0..8 {
            for b in a + 1..8 {
                assert!(landed[a].0.distance(landed[b].0) > 0.008 || landed[a].1.angle_between(landed[b].1) > 0.5, "{a} and {b} strike alike");
            }
        }
        // Full hands do not punch: they throw what they carry, both hands coming up together from
        // low in front and out.
        let low = at(0, WOUND);
        assert!(low.0.y < SHOULDER.y - 0.012 && low.2.y < SHOULDER.y - 0.012, "the throw starts low: {} {}", low.0, low.2);
        assert!(landed[0].0.y > low.0.y + 0.02 && landed[0].0.z < -0.05 && landed[0].2.z < -0.05, "and heaves up and out: {} {}", landed[0].0, landed[0].2);
        let (right, left) = (landed[0].0, landed[0].2);
        assert!((right.x + left.x).abs() < 0.004 && (right.y - left.y).abs() < 0.004 && (right.z - left.z).abs() < 0.004, "in both hands alike: {right} {left}");
        // The pistol is raised above the shoulder and whipped down with its muzzle up, butt first.
        assert!(at(1, WOUND).0.y > SHOULDER.y + 0.02 && landed[1].0.y < at(1, WOUND).0.y - 0.03 && landed[1].1.y > 0.5);
        // The sniper's stock leads: its muzzle points up and back over the shoulder.
        assert!(landed[2].1.z > 0.3 && landed[2].1.y > 0.3, "rifle butt points {}", landed[2].1);
        // The hot sauce thrower is crosswise in both hands, and both push.
        assert!(landed[3].1.x.abs() > 0.8 && landed[3].2.z < -0.04 && landed[3].2.z < at(3, WOUND).2.z - 0.03);
        // The bazooka's tube sweeps from the right across to the left, in both hands.
        assert!(at(4, WOUND).1.x > 0.1 && at(4, WOUND).1.z > 0.3 && at(4, FOLLOWED).1.x < -0.5, "the tube points {} then {}", at(4, WOUND).1, at(4, FOLLOWED).1);
        assert!(landed[4].2.distance(landed[4].0) < 0.05, "the free hand is {} from the grip", landed[4].2.distance(landed[4].0));
        // The spray can is jabbed straight ahead, upright; only the two-handed blows move the free arm.
        assert!(landed[5].1.z < -0.8);
        for weapon in [1, 2, 5] {
            assert!(landed[weapon].2.z > -0.03, "weapon {weapon} is swung with one hand");
        }
        // The syrup rifle is swung round level, its muzzle going back past the left shoulder, so that
        // its butt goes from behind the body to in front of it; the left hand keeps hold throughout.
        let butt = |s: &(Vec3, Vec3, Vec3, Pose)| s.0 - s.1 * 0.0375;
        let cocked = at(6, WOUND);
        assert!(landed[6].1.z > 0.25 && landed[6].1.x < -0.45 && landed[6].1.y.abs() < 0.4, "the rifle points {}", landed[6].1);
        assert!(butt(&cocked).z > 0.02 && butt(&landed[6]).z < -0.06, "the butt goes from {} to {}", butt(&cocked), butt(&landed[6]));
        assert!(landed[6].2.distance(landed[6].0) < 0.06 && cocked.2.distance(cocked.0) < 0.06, "the left hand stays on the rifle");
        // The wind-up turns the shoulders one way and the blow the other.
        let turn = |weapon, swing| pose(Motion { weapon, swing, ..default() }).torso.mul_vec3(Vec3::X).z;
        for weapon in [1, 2, 4, 5, 6] {
            assert!(turn(weapon, WOUND) * turn(weapon, CONTACT) < 0., "weapon {weapon}: the shoulders wind back, then drive through");
        }
    }
    #[test]
    fn the_gun_pitches_with_the_aim_and_recoil_climbs_on_top() {
        let barrel = |m: Motion| {
            let p = pose(m);
            p.torso * p.shoulders[1] * p.elbows[1] * p.wrist * Vec3::NEG_Y
        };
        for weapon in [1, 2, 3, 4, 6, 7] {
            for pitch in [-0.5f32, -0.2, 0., 0.4, 0.9] {
                let aim = barrel(Motion { weapon, aim: 1., pitch, ..default() });
                assert!((aim.y.asin() - pitch).abs() < 0.06, "weapon {weapon} aims {} for {pitch}", aim.y.asin());
                assert!(aim.x.abs() < 0.05, "pitching does not swing the gun sideways");
                let (_, tip) = grip(&pose(Motion { weapon, aim: 1., pitch, ..default() }), weapon);
                assert!(tip.y > 0.004, "weapon {weapon} digs into the counter at {pitch}");
            }
            // Out of range, the gun stops short: not through the counter, not over the shoulder.
            let low = barrel(Motion { weapon, aim: 1., pitch: -1.4, ..default() }).y.asin();
            let high = barrel(Motion { weapon, aim: 1., pitch: 1.4, ..default() }).y.asin();
            assert!(low > -0.6 && high < 1.1, "{low} {high}");
            let prone = Motion { weapon, aim: 1., pitch: -1.4, prone: 1., ..default() };
            assert!(grip(&pose(prone), weapon).1.y > 0.004, "a prone gun stays off the counter");
        }
        let kicked = barrel(Motion { weapon: 1, aim: 1., pitch: 0.3, kick: 0.5, ..default() }).y.asin();
        assert!(kicked > 0.3 + 0.35, "recoil climbs from the aim: {kicked}");
        // The body stays planted whatever the aim (the head nods instead: bend.rs); in Kung Fu, where the head does not
        // nod, it tips a little with the look as it always did.
        let tip = |pitch, lean| pose(Motion { pitch, lean, ..default() }).torso.mul_vec3(Vec3::Y).z;
        assert!(tip(0.8, 0.).abs() < 1e-6 && tip(-0.8, 0.).abs() < 1e-6);
        assert!(tip(0.8, 1.) > 0.1 && tip(-0.8, 1.) < -0.1 && tip(0.8, 1.) < 0.25);
    }
    #[test]
    fn the_syrup_rifle_is_shouldered_in_both_hands() {
        for m in every_pose() {
            if m.cheer > 0. || m.clap > 0. {
                continue;
            }
            for pitch in [-0.4, 0., 0.6] {
                let p = pose(Motion { weapon: 6, aim: 1., pitch, ..m });
                // The left hand is under the handguard, wherever the gun is.
                let (hand, _) = grip(&p, 6);
                let guard = hand + p.torso * p.shoulders[1] * p.elbows[1] * p.wrist * (crate::weapons::in_hand().rotation * HANDGUARD);
                let [(elbow, left), _] = arms(&p);
                assert!(left.distance(guard) < 0.001, "the left hand is {} from the handguard in {m:?}", left.distance(guard));
                assert!(p.reach[0] < 4.3, "the left arm is stretched {} times in {m:?}", p.reach[0]);
                // Aimed and standing, the arm crosses in front of the chest, its elbow dropped clear of it.
                if m.swing == 0. && m.prone == 0. && m.kick == 0. && m.crouch == 0. && pitch == 0. {
                    assert!(elbow.z < -0.035 && elbow.y < left.y, "the elbow is at {elbow}");
                }
            }
        }
        // The stock reaches back to the shoulder it is held against.
        let p = pose(Motion { weapon: 6, aim: 1., ..default() });
        let (hand, tip) = grip(&p, 6);
        let butt = hand - (tip - hand).normalize() * 0.0375;
        assert!(butt.distance(SHOULDER) < 0.012, "the butt is at {butt}");
        // Another gun leaves the left arm free.
        assert!(arms(&pose(Motion { weapon: 2, aim: 1., ..default() }))[0].1.z > -0.02);
    }
    #[test]
    fn a_syrup_burst_climbs_round_by_round_and_settles_before_the_next() {
        let mut world = World::new();
        let here = Transform::default();
        let cues = Cues { alive: true, weapon: 6, ..default() };
        let mut gait = Gait {
            at: Vec2::ZERO,
            time: 0.,
            live: false,
            stim: false,
            motion: Motion { clap: 1., swing: 1., ..default() },
            id: 1,
            swings: 0,
            muzzle: Vec3::ZERO,
        };
        gait.follow(&here, 1., cues);
        let hotdog = world.spawn(gait).id();
        let climb = |world: &World| {
            let (hand, tip) = grip(&pose(world.get::<Gait>(hotdog).unwrap().motion), 6);
            (tip - hand).normalize().y.asin()
        };
        let mut time = 1.;
        let mut wait = |world: &mut World, seconds: f32| {
            for _ in 0..(seconds / 0.01).round() as usize {
                time += 0.01;
                world.get_mut::<Gait>(hotdog).unwrap().follow(&here, time, cues);
            }
        };
        // Three rounds, 0.05 s apart: each kicks the muzzle higher above where it was aimed than the last.
        let level = climb(&world);
        let mut peaks = vec![];
        for _ in 0..3 {
            kick(&mut world, hotdog, 6);
            peaks.push(climb(&world) - level);
            wait(&mut world, 0.05);
        }
        assert!(peaks[0] > 0.12 && peaks[1] > peaks[0] + 0.05 && peaks[2] > peaks[1] + 0.03 && peaks[2] < 0.45, "{peaks:?}");
        // By the next burst, 0.30 s after the last round, the gun is back where it was aimed.
        wait(&mut world, 0.25);
        assert!(climb(&world) - level < 0.05, "still climbing {}", climb(&world) - level);
    }
    #[test]
    fn aimed_down_its_sights_a_hotdog_holds_the_rifle_or_the_sniper_up_to_its_right_eye_along_its_aim() {
        // Where the sight (`weapons::sight`) of the gun in hand is, in the torso's frame, the way its barrel points,
        // and the hand.
        let held = |p: &Pose, weapon: usize| {
            let bone = Vec3::NEG_Y * BONE * p.reach[1];
            let forearm = p.shoulders[1] * p.elbows[1];
            let hand = SHOULDER + p.shoulders[1] * bone + forearm * bone;
            let turn = forearm * p.wrist * crate::weapons::in_hand().rotation;
            (hand + turn * crate::weapons::sight(weapon).unwrap(), turn * Vec3::NEG_Z, hand)
        };
        for weapon in [6, 2] {
            for pitch in [0., 0.3, -0.12] {
                let hip = pose(Motion { weapon, aim: 1., pitch, ..default() });
                let aimed = pose(Motion { weapon, aim: 1., pitch, sight: 1., ..default() });
                let torso = aimed.torso;
                let along = torso.inverse() * (Quat::from_rotation_x(pitch) * Vec3::NEG_Z);
                let (eyepiece, barrel, _) = held(&aimed, weapon);
                assert!(eyepiece.distance(RIGHT_EYE + along * RELIEF) < 2e-3, "weapon {weapon} at {pitch}: its sight {eyepiece} by the eye {RIGHT_EYE}");
                assert!(barrel.dot(along) > 0.999, "weapon {weapon} at {pitch}: along the aim, {barrel} for {along}");
                // At the hip the sight is well away from the eye: the gun comes up to it.
                assert!(held(&hip, weapon).0.distance(RIGHT_EYE) > 0.02);
                assert!(aimed.reach[1] < 2.6, "the arm stretches no further than the stim clap's: {}", aimed.reach[1]);
            }
        }
        // Only the rifle and the sniper have sights; prone keeps the arm out along the aim.
        let pistol = pose(Motion { weapon: 1, aim: 1., sight: 1., ..default() });
        let plain = pose(Motion { weapon: 1, aim: 1., ..default() });
        assert_eq!((pistol.shoulders, pistol.elbows, pistol.wrist), (plain.shoulders, plain.elbows, plain.wrist));
        let prone = pose(Motion { weapon: 6, aim: 1., sight: 1., prone: 1., ..default() });
        let flat = pose(Motion { weapon: 6, aim: 1., prone: 1., ..default() });
        assert_eq!((prone.shoulders, prone.wrist), (flat.shoulders, flat.wrist));
        // The gait eases it up in SIGHTING and back down again when the cue goes, and not for a gun without sights.
        let here = Transform::default();
        let mut gait = Gait { at: Vec2::ZERO, time: 0., live: false, stim: false, motion: Motion { clap: 1., swing: 1., ..default() }, id: 1, swings: 0, muzzle: Vec3::ZERO };
        let rifle = |aimed| Cues { alive: true, weapon: 6, aimed, ..default() };
        gait.follow(&here, 1., rifle(false));
        let mut time = 1.;
        for _ in 0..4 {
            time += 0.02;
            gait.follow(&here, time, rifle(true));
        }
        assert!(gait.motion.sight > 0.4 && gait.motion.sight < 0.7, "{}", gait.motion.sight);
        for _ in 0..6 {
            time += 0.02;
            gait.follow(&here, time, rifle(true));
        }
        assert_eq!(gait.motion.sight, 1.);
        for _ in 0..10 {
            time += 0.02;
            gait.follow(&here, time, Cues { weapon: 1, ..rifle(true) });
        }
        assert_eq!(gait.motion.sight, 0., "the pistol has no sights");
    }
    #[test]
    fn a_change_in_the_swing_counter_starts_exactly_one_stab() {
        let cues = |swings| Cues { alive: true, swings, ..default() };
        let here = Transform::default();
        let mut gait = Gait {
            at: Vec2::ZERO,
            time: 0.,
            live: false,
            stim: false,
            motion: Motion { clap: 1., swing: 1., ..default() },
            id: 1,
            swings: 0,
            muzzle: Vec3::ZERO,
        };
        // Joining mid-round with swings already counted does not stab.
        gait.follow(&here, 1.00, cues(7));
        gait.follow(&here, 1.02, cues(7));
        assert_eq!(gait.motion.swing, 1.);
        // One more swing starts the stab, and it then runs its 0.39 s without restarting.
        gait.follow(&here, 1.04, cues(8));
        assert_eq!(gait.motion.swing, 0.);
        gait.follow(&here, 1.06, cues(8));
        gait.follow(&here, 1.08, cues(8));
        assert!(gait.motion.swing > 0.05 && gait.motion.swing < 0.15);
        for i in 0..30 {
            gait.follow(&here, 1.10 + i as f32 * 0.02, cues(8));
        }
        assert_eq!(gait.motion.swing, 1.);
        // A new round resets the counter to 0: that is not a stab either way round, but any
        // change is, so a reset while mid-count swings once at most.
        gait.follow(&here, 1.72, cues(9));
        assert_eq!(gait.motion.swing, 0.);
        // The aim eases towards where the simulation says the hotdog looks.
        let up = Cues { alive: true, swings: 9, pitch: 0.8, ..default() };
        for i in 0..40 {
            gait.follow(&here, 1.74 + i as f32 * 0.02, up);
        }
        assert!((gait.motion.pitch - 0.8).abs() < 0.01);
    }
    #[test]
    fn a_swing_that_lands_stands_still_on_contact_for_the_hit_stop_and_one_that_misses_runs_through() {
        use crate::whip::{STOP, STOP_STAB, TOTAL};
        let here = Transform::default();
        let cues = |swings| Cues { alive: true, swings, ..default() };
        // A swing of hotdog 1, 0.005 s at a time, told whether it lands (and whether that is a backstab) once it has begun:
        // how long it stood still, and how long it took in all.
        let run = |struck: Option<bool>| {
            let mut gait = Gait { at: Vec2::ZERO, time: 0., live: false, stim: false, motion: Motion { clap: 1., swing: 1., ..default() }, id: 1, swings: 0, muzzle: Vec3::ZERO };
            let mut time = 1.;
            for swings in [1, 1, 2] {
                time += 0.005;
                gait.follow(&here, time, cues(swings));
            }
            assert_eq!(gait.motion.swing, 0., "the swing has begun");
            gait.motion.struck = struck;
            let (mut held, mut steps) = (0, 0);
            while gait.motion.swing < 1. && steps < 1000 {
                let before = gait.motion.swing;
                time += 0.005;
                gait.follow(&here, time, cues(2));
                steps += 1;
                held += (gait.motion.swing == before && before > 0.) as u32;
            }
            (held as f32 * 0.005, steps as f32 * 0.005)
        };
        let (held, took) = run(None);
        assert!(held == 0. && (took - TOTAL).abs() < 0.011, "a miss: held {held}, took {took}");
        let (held, took) = run(Some(false));
        assert!((held - STOP).abs() < 0.011 && (took - TOTAL - held).abs() < 0.011, "a blow: held {held}, took {took}");
        let (held, took) = run(Some(true));
        assert!((held - STOP_STAB).abs() < 0.011 && (took - TOTAL - held).abs() < 0.011, "a backstab: held {held}, took {took}");
    }

    /// A world with the hotdog models in it (as the game has them) and hotdog `id` standing at rest at the origin.
    fn hotdog(id: u32) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>();
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let models = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|_, mut materials: Mut<Assets<StandardMaterial>>| PlayerModels::new(&mut meshes, &mut materials, &server, false))
        });
        world.insert_resource(models.clone());
        let root = models.spawn(world, Vec3::ZERO, id);
        (app, root)
    }
    /// The triangles drawn for a hotdog: every mesh hung on it that shows (a hidden part hides what hangs from it).
    fn drawn(world: &World, root: Entity) -> usize {
        let meshes = world.resource::<Assets<Mesh>>();
        let mut count = 0;
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if world.get::<Visibility>(entity).is_some_and(|v| *v == Visibility::Hidden) {
                continue;
            }
            if let Some(mesh) = world.get::<Mesh3d>(entity).and_then(|m| meshes.get(&m.0)) {
                count += mesh.indices().map_or(0, |i| i.len()) / 3;
            }
            stack.extend(world.get::<Children>(entity).into_iter().flat_map(|c| c.iter()));
        }
        count
    }
    #[test]
    fn the_low_levels_draw_a_hotdog_with_under_a_third_of_the_triangles_and_switch_in_place() {
        let mut app = App::new();
        app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>();
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        // A GPU's models: the detailed set.
        let mut models = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|_, mut materials: Mut<Assets<StandardMaterial>>| PlayerModels::new(&mut meshes, &mut materials, &server, true))
        });
        world.insert_resource(models.clone());
        let root = models.spawn(world, Vec3::ZERO, 1);
        let detailed = drawn(world, root);
        let skin = world.resource::<Assets<Mesh>>().get(&models.skin).unwrap().count_vertices();
        // Low and Lowest (`quality::Detail::lean`): the same entities, the same mesh handles, remade leaner.
        assert!(world.resource_scope(|_, mut meshes: Mut<Assets<Mesh>>| models.refine(&mut meshes, false)));
        let lean = drawn(world, root);
        println!("a hotdog: {detailed} triangles detailed, {lean} lean");
        assert!(lean * 3 <= detailed, "{lean} lean against {detailed} detailed");
        assert!(lean <= 18_000, "the lean budget: {lean} triangles a hotdog, gun and all");
        assert!(detailed >= 40_000, "High's look is the detailed set: {detailed}");
        assert!(world.resource::<Assets<Mesh>>().get(&models.skin).unwrap().count_vertices() < skin / 3);
        // And back, exactly as it was.
        assert!(world.resource_scope(|_, mut meshes: Mut<Assets<Mesh>>| models.refine(&mut meshes, true)));
        assert_eq!(drawn(world, root), detailed);
        assert!(!world.resource_scope(|_, mut meshes: Mut<Assets<Mesh>>| models.refine(&mut meshes, true)), "nothing to do");
        // A CPU renderer's models are lean whatever the level asks.
        let mut software = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|_, mut materials: Mut<Assets<StandardMaterial>>| PlayerModels::new(&mut meshes, &mut materials, &server, false))
        });
        assert!(!world.resource_scope(|_, mut meshes: Mut<Assets<Mesh>>| software.refine(&mut meshes, true)));
        assert!(!software.detailed());
    }
    /// A part hung on a hotdog's body: its mesh and material, its points in the hotdog's own frame (the body at rest),
    /// and whether it shows.
    struct Part {
        material: Handle<StandardMaterial>,
        points: Vec<Vec3>,
        shown: bool,
    }
    impl Part {
        fn min(&self) -> Vec3 {
            self.points.iter().fold(Vec3::MAX, |a, p| a.min(*p))
        }
        fn max(&self) -> Vec3 {
            self.points.iter().fold(Vec3::MIN, |a, p| a.max(*p))
        }
        fn middle(&self) -> Vec3 {
            (self.min() + self.max()) / 2.
        }
        fn size(&self) -> Vec3 {
            self.max() - self.min()
        }
        /// Every point is on the front of the casing (-z) and stands off it.
        fn on_the_front(&self) -> bool {
            self.points.iter().all(|p| {
                let (middle, radius) = casing(p.y);
                p.z < 0. && Vec2::new(p.x - middle.x, p.z - middle.z).length() > radius
            })
        }
    }
    /// The features on a hotdog's body: every part hung on it (or on its head, where it is now) but the casing, the
    /// buns, the mustard and the ring, in the torso's frame.
    fn features(world: &World, root: Entity) -> Vec<Part> {
        let models = world.resource::<PlayerModels>();
        let body = [&models.skin, &models.bun, &models.mustard, &models.badge];
        let meshes = world.resource::<Assets<Mesh>>();
        let rig = world.get::<Rig>(root).unwrap();
        let head = *world.get::<Transform>(rig.head).unwrap();
        let on = |parent: Entity, frame: Transform| world.get::<Children>(parent).unwrap().iter().map(move |e| (e, frame));
        on(rig.torso, Transform::IDENTITY)
            .chain(on(rig.head, head))
            .filter_map(|(e, frame)| {
                let mesh = &world.get::<Mesh3d>(e)?.0;
                if body.contains(&mesh) {
                    return None;
                }
                let tr = frame * *world.get::<Transform>(e)?;
                let points = match meshes.get(mesh)?.attribute(Mesh::ATTRIBUTE_POSITION)? {
                    bevy::mesh::VertexAttributeValues::Float32x3(p) => p.iter().map(|p| tr.transform_point(Vec3::from_array(*p))).collect(),
                    _ => return None,
                };
                let material = world.get::<MeshMaterial3d<StandardMaterial>>(e)?.0.clone();
                Some(Part { material, points, shown: world.get::<Visibility>(e) != Some(&Visibility::Hidden) })
            })
            .collect()
    }
    /// The whites of a hotdog's eyes, left then right.
    fn whites(world: &World, root: Entity) -> Vec<Part> {
        let white = world.resource::<PlayerModels>().white.clone();
        let mut eyes: Vec<Part> = features(world, root).into_iter().filter(|p| p.shown && p.material == white && p.size().x > 0.01).collect();
        eyes.sort_by(|a, b| a.middle().x.total_cmp(&b.middle().x));
        eyes
    }
    /// What shows of a hotdog's mouth: the features in the middle of the face, under the eyes and over the mustard.
    fn mouth(world: &World, root: Entity) -> Vec<Part> {
        let under = whites(world, root).iter().map(|eye| eye.min().y).fold(f32::MAX, f32::min);
        features(world, root).into_iter().filter(|p| p.shown && p.middle().x.abs() < 0.002 && p.min().y > 0.123 && p.max().y < under).collect()
    }
    #[test]
    fn a_painted_hotdog_has_no_mask_and_wears_its_colour_on_its_ring() {
        let (mut app, root) = hotdog(2);
        let world = app.world_mut();
        let models = world.resource::<PlayerModels>().clone();
        let ring = world.get::<Livery>(root).unwrap().0[0];
        let wears = |world: &World| world.get::<MeshMaterial3d<StandardMaterial>>(ring).unwrap().0.clone();
        // A bot's ring (and anybody's before a colour is chosen) is the colour of its place.
        let own = models.colors[1].clone();
        assert_eq!(wears(world), own);
        paint(world, root, "blue");
        assert_eq!(wears(world), models.paints[1], "the ring wears the colour chosen");
        // No mask: the colour is on the ring and nowhere on the face, and nothing on the face is as big as a mask.
        for part in features(world, root).iter().filter(|p| p.shown) {
            assert!(!models.paints.contains(&part.material), "a part at {} wears the colour", part.middle());
            assert!(part.size().x < 0.02 && part.size().y < 0.02, "a part {} across at {} covers the face", part.size(), part.middle());
        }
        // An unknown colour (a bot's is '') gives the ring back the colour of its place.
        paint(world, root, "");
        assert_eq!(wears(world), own);
        // Forts dresses the ring in its team's colour, which the colour chosen does not paint over; once the team's
        // colour comes off again (the lobby gives back what it wore before), the colour chosen goes back on.
        let team = world.resource_mut::<Assets<StandardMaterial>>().add(Color::srgb(0.95, 0.74, 0.10));
        world.entity_mut(ring).insert(MeshMaterial3d(team.clone()));
        paint(world, root, "green");
        assert_eq!(wears(world), team);
        world.entity_mut(ring).insert(MeshMaterial3d(own.clone()));
        paint(world, root, "green");
        assert_eq!(wears(world), models.paints[2]);
    }
    #[test]
    fn the_mouth_smiles() {
        let (app, root) = hotdog(1);
        let mouth = mouth(app.world(), root);
        // The line of the mouth: the widest thing that shows of it.
        let line = mouth.iter().max_by(|a, b| a.size().x.total_cmp(&b.size().x)).expect("a mouth");
        let (half, middle) = (line.size().x / 2., line.middle().x);
        // The height of its middle across the face, where it is `across` (a fraction of its half-width) from the middle.
        let height = |across: f32| {
            let near: Vec<f32> = line.points.iter().filter(|p| ((p.x - middle).abs() / half - across).abs() < 0.06).map(|p| p.y).collect();
            near.iter().sum::<f32>() / near.len() as f32
        };
        assert!(height(0.88) - height(0.) > 0.001, "the corners are {} mm over the middle", (height(0.88) - height(0.)) * 1000.);
        assert!(half > 0.006 && half < 0.0095, "the mouth is {} mm wide", half * 2000.);
    }
    #[test]
    fn the_face_has_brows_and_cheeks() {
        let (app, root) = hotdog(1);
        let world = app.world();
        let eyes = whites(world, root);
        assert_eq!(eyes.len(), 2);
        let features: Vec<Part> = features(world, root).into_iter().filter(|p| p.shown).collect();
        for (side, eye) in eyes.iter().enumerate() {
            let on_this_side = |p: &&Part| (p.middle().x > 0.) == (side == 1);
            // A brow: wholly over the eye, across its middle, no wider than the eye.
            let brows: Vec<&Part> = features.iter().filter(on_this_side).filter(|p| p.min().y > eye.max().y && p.max().y < 0.175).collect();
            assert_eq!(brows.len(), 1, "one brow over eye {side}");
            let brow = brows[0];
            assert!((brow.middle().x - eye.middle().x).abs() < 0.004 && brow.size().x < eye.size().x && brow.size().x > 0.006, "brow {side}");
            assert!(brow.min().y - eye.max().y < 0.005, "brow {side} is {} mm over its eye", (brow.min().y - eye.max().y) * 1000.);
            // A cheek: under the eye and further out, over the mustard.
            let cheeks: Vec<&Part> =
                features.iter().filter(on_this_side).filter(|p| p.middle().y < eye.min().y + 0.002 && p.middle().x.abs() > eye.middle().x.abs() + 0.002 && p.min().y > 0.123).collect();
            assert_eq!(cheeks.len(), 1, "one cheek under eye {side}");
            for part in [brow, cheeks[0]] {
                assert!(part.on_the_front(), "part at {} is not on the front of the casing", part.middle());
            }
        }
    }
    #[test]
    fn talking_still_opens_the_mouth() {
        let (mut app, root) = hotdog(3);
        let world = app.world_mut();
        let height = |world: &World| {
            let mouth = mouth(world, root);
            mouth.iter().map(|p| p.max().y).fold(f32::MIN, f32::max) - mouth.iter().map(|p| p.min().y).fold(f32::MAX, f32::min)
        };
        let mut time = 1.;
        for _ in 0..30 {
            time += 0.02;
            speak(world, root, 0., time);
        }
        let rest = height(world);
        // Talking for half a second: the mouth opens and shuts with the syllables, and at the widest is well open.
        let mut widest = rest;
        for _ in 0..25 {
            time += 0.02;
            speak(world, root, 1., time);
            widest = widest.max(height(world));
        }
        assert!(widest > rest + 0.002, "the mouth opens {} mm", (widest - rest) * 1000.);
        // Quiet again, it closes.
        for _ in 0..30 {
            time += 0.02;
            speak(world, root, 0., time);
        }
        assert!((height(world) - rest).abs() < 0.0002);
    }

    #[test]
    fn the_mouth_lies_on_the_face_under_the_eyes_over_the_mustard_and_its_paint_is_one_of_five() {
        // The face is on -z, at the height where the casing is at its full width.
        let (middle, radius) = casing(MOUTH);
        assert!((radius - 0.023).abs() < 1e-6 && middle.x.abs() < 0.002 && middle.z.abs() < 0.002);
        let front = on_face(0., MOUTH, 0.);
        assert!((front.z - (middle.z - radius)).abs() < 1e-6 && front.x == middle.x && front.y == MOUTH);
        // `across` finds the point on the front that far across the face.
        for (x, y) in [(-CHEEK.0, CHEEK.1), (0., MOUTH), (EYE.0, BROW.0)] {
            assert!((on_face(across(x, y), y, 0.).x - x).abs() < 1e-6);
        }
        let points = |mesh: &Mesh| -> Vec<Vec3> {
            match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
                bevy::mesh::VertexAttributeValues::Float32x3(p) => p.iter().map(|p| Vec3::from_array(*p)).collect(),
                _ => panic!("positions"),
            }
        };
        // The smile, and the inside of the mouth shut, half open and open all the way (`speak` stretches it down from
        // the middle of the smile).
        let line = points(&smile(36, 10));
        let open: Vec<Vec3> = [0.02, 0.5, 1.].iter().flat_map(|k| points(&gape(24, 10)).into_iter().map(move |p| Vec3::new(p.x, MOUTH + p.y * k, p.z))).collect();
        // Below the eyes (their whites reach down to 0.144), above where the squiggle now ends (0.123).
        let top = line.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        let low = open.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        assert!(top + 0.001 < EYE.1 - 0.009 && low > 0.123 + 0.001, "the mouth runs from {low} to {top}");
        // Every point of the smile and the mouth, the brows and the cheeks stands off the front of the casing, the
        // smile's line in front of the inside of the mouth.
        let features = [brow(-1., 20, 8), brow(1., 20, 8), cheek(-1., 8, 28), cheek(1., 8, 28)].iter().flat_map(points).collect::<Vec<_>>();
        for (points, proud) in [(&line, 0.00015), (&open, 0.00025), (&features, 0.00015)] {
            for p in points {
                let (middle, radius) = casing(p.y);
                assert!(Vec2::new(p.x - middle.x, p.z - middle.z).length() > radius + proud && p.z < 0., "{p:?}");
            }
        }
        let farthest = |points: &[Vec3]| points.iter().map(|p| p.z).fold(f32::MAX, f32::min);
        assert!(farthest(&line) < farthest(&open) - 0.0005);
        assert_eq!(PAINTS, ["red", "blue", "green", "yellow", "black"]);
        assert_eq!(tag_rgb("blue"), Some(PAINT_RGB[1]));
        assert_eq!((tag_rgb(""), tag_rgb("pink")), (None, None));
    }

    /// The legs turn over with the pace: past a full run (RUN) faster in proportion, so that a stride covers as much
    /// ground at the hotdogs' walk (1.25 m/s) and stimmed as at a full run, and the feet slide no more.
    #[test]
    fn the_walk_cycle_keeps_step_with_the_pace() {
        let per_metre = |speed: f32| {
            let mut gait = Gait { at: Vec2::ZERO, time: 0., live: false, stim: false, motion: Motion::default(), id: 1, swings: 0, muzzle: Vec3::ZERO };
            let cues = Cues { alive: true, weapon: 6, ..default() };
            let (mut turned, mut walked, mut last) = (0f32, 0f32, 0f32);
            for k in 0..300 {
                let t = 1. + k as f32 * 0.01;
                gait.follow(&Transform::from_xyz(0., 0., -speed * (t - 1.)), t, cues);
                if k >= 100 {
                    turned += (gait.motion.phase - last).rem_euclid(TAU);
                    walked += speed * 0.01;
                }
                last = gait.motion.phase;
            }
            turned / walked
        };
        let (run, walk, stim) = (per_metre(RUN), per_metre(kitchen_sim::actor::WALK as f32), per_metre(1.5 * kitchen_sim::actor::WALK as f32));
        assert!((walk / run - 1.).abs() < 0.05 && (stim / run - 1.).abs() < 0.05, "radians a metre: {run} at a run, {walk} walking, {stim} stimmed");
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The cameras of a titan round.
//!
//! * **The titan's player** sees from its eyes, 1.7 m over the floor, where the small hotdogs are specks: each living
//!   one has a small marker over it, a ring that stays the same size on the screen however far it is (`markers`). Its
//!   own arms and legs are drawn (its head and body are not: the camera is inside the head), so its hands are seen doing
//!   each move. Its view turns a little heavily: the camera's yaw follows the look's with a short lag (`HEAVY`), and the
//!   eye bobs with the giant's own stride. The view key (`Action::View`, V) puts the camera behind and above it, far
//!   enough back that the whole giant and the floor round it are in view, pulled in before a wall.
//! * **Every camera** (small hotdogs' first person, a death camera, a replay's) shakes when the titan's feet come down,
//!   when it smashes, lands or falls: the nearer, the harder ([`jolt`]). And none is ever left inside the giant: a camera
//!   that a step, a charge or a fall has put inside its body or limbs is moved out of it the shortest way ([`keep_out`]),
//!   before anything is drawn. Both are written after everything else has placed the camera and undone before the next
//!   frame places it again ([`restore`]), so nothing that eases a camera from its last place is disturbed.
use super::Titans;
use super::pose::{L, R, Skeleton};
use crate::arena::Game;
use crate::controls::{Action, Bindings};
use bevy::prelude::*;
use std::f32::consts::TAU;

/// The titan player's eye is a little in front of its eyes (so its own brow is behind the camera).
pub const EYE_AHEAD: f32 = 0.05;
/// Seconds the titan's view takes to catch up with a turn (the heaviness: a time constant).
pub const HEAVY: f32 = 0.07;
/// Behind and above it, in its third person: how far back, how high over its feet the camera stands, and the point it
/// looks at (over its feet).
pub const BACK: f32 = 2.4;
pub const HIGH: f32 = 2.35;
pub const LOOK_AT: f32 = 1.05;
/// The camera's near plane in the titan's views.
pub const NEAR: f32 = 0.02;
/// The most a camera is shaken (radians), and how quickly a shake dies away (time constant, s).
pub const SHAKE_MOST: f32 = 0.06;
const SHAKE_TAU: f32 = 0.16;
const SHAKE_LONG: f32 = 1.2;
/// A camera keeps this far out of the giant's surface.
pub const CLEAR: f32 = 0.025;

/// One shake under way: seconds since, how hard, its own wobble.
#[derive(Clone, Copy, Default, Debug)]
struct Kick {
    age: f32,
    amp: f32,
    seed: f32,
    /// How slow its wobble is (a footfall is a thud, a fall a long rumble).
    rumble: f32,
}
/// The cameras' state.
#[derive(Resource, Default)]
pub struct View {
    /// The titan's player wants the camera behind it.
    pub third: bool,
    /// The yaw the titan's view shows (a little behind the look).
    shown_yaw: Option<f32>,
    kicks: [Kick; 6],
    /// What `settle` found and wrote last frame, to undo.
    base: Option<(Entity, Transform, Transform)>,
    /// The markers over the small hotdogs, made the first time they are wanted.
    markers: Vec<Entity>,
    marker_kit: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
}

/// Whether this player is the titan, seen from its own eyes now.
pub fn first_person(world: &World) -> bool {
    let demo = world.get_resource::<crate::runtime::Options>().is_some_and(|o| o.demo);
    let third = world.get_resource::<View>().is_some_and(|v| v.third);
    world.get_resource::<Game>().is_some_and(|g| super::playing_titan(g, demo)) && !third
}
/// Whether this player is the titan, seen from behind.
pub fn third_person(world: &World) -> bool {
    let demo = world.get_resource::<crate::runtime::Options>().is_some_and(|o| o.demo);
    let third = world.get_resource::<View>().is_some_and(|v| v.third);
    world.get_resource::<Game>().is_some_and(|g| super::playing_titan(g, demo)) && third
}

/// Before the round's tick: the view key turns the titan's camera behind it, or back to its eyes.
pub fn toggle(world: &mut World) {
    let pressed = {
        let (Some(keys), Some(mouse)) = (world.get_resource::<ButtonInput<KeyCode>>(), world.get_resource::<ButtonInput<MouseButton>>()) else {
            return;
        };
        world.get_resource::<Bindings>().cloned().unwrap_or_default().just(Action::View, keys, mouse)
    };
    let demo = world.get_resource::<crate::runtime::Options>().is_some_and(|o| o.demo);
    if pressed && world.get_resource::<Game>().is_some_and(|g| super::playing_titan(g, demo)) {
        let mut v = world.resource_mut::<View>();
        v.third = !v.third;
    }
}

/// Where the titan's camera stands behind and above a titan standing at `feet`, its look turned `yaw`, on `level`:
/// back and up, pulled in before a wall, and the way it looks (at a point over the feet, a little ahead).
pub fn behind(level: &kitchen_sim::map::Level, feet: Vec3, yaw: f32, pitch: f32) -> Transform {
    let turn = Quat::from_rotation_y(yaw);
    let target = feet + Vec3::Y * LOOK_AT + turn * Vec3::NEG_Z * (0.6 + 0.8 * pitch.max(-0.6).min(0.4));
    let pivot = feet + Vec3::Y * LOOK_AT;
    // Looking up lowers the camera a little, looking down raises it.
    let back = turn * Vec3::new(0., HIGH - LOOK_AT - 0.6 * pitch.clamp(-0.6, 0.6), BACK);
    let (way, far) = (back.normalize_or(Vec3::Z), back.length());
    let room = level.cover_from(pivot, way).min(far + 0.1) - 0.1;
    // (And inside the room: its walls are the map's bounds, its ceiling CEILING over the floor.)
    let b = level.map.bounds.map(|v| v as f32);
    let inside = |p: Vec3| p.x > b[0] + 0.12 && p.x < b[2] - 0.12 && p.z > b[1] + 0.12 && p.z < b[3] - 0.12 && p.y < feet.y + CEILING;
    let mut reach = room.clamp(0.5, far);
    while reach > 0.5 && !inside(pivot + way * reach) {
        reach -= 0.05;
    }
    let eye = pivot + way * reach;
    Transform::from_translation(eye).looking_at(target, Vec3::Y)
}
/// The camera behind the titan stays under this much over the floor (Titan's Kitchen's ceiling is 2.7 m up).
pub const CEILING: f32 = 2.55;

/// After the round's tick (which put the camera at a hotdog's eye): the titan's player's camera at the giant's eyes, or
/// behind it; its turns a little heavy; the markers over the specks.
pub fn place(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let dt = world.resource::<Time<Real>>().delta_secs().min(0.1);
    let first = first_person(world);
    let third = third_person(world);
    if !first && !third {
        world.resource_mut::<View>().shown_yaw = None;
        markers(world, None);
        return;
    }
    let Some((_, frame, root)) = world.resource::<Titans>().drawn else {
        return;
    };
    let g = world.resource::<Game>();
    let Some(cam) = g.camera() else {
        return;
    };
    let (look_yaw, pitch, feet) = (g.yaw, g.pitch, g.position);
    let level = g.feast.level.clone();
    let mut v = world.resource_mut::<View>();
    let shown = match v.shown_yaw {
        Some(was) => {
            let delta = (look_yaw - was + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
            was + delta * (1. - (-dt / HEAVY).exp())
        }
        None => look_yaw,
    };
    v.shown_yaw = Some(shown);
    let place = if first {
        let rotation = Quat::from_rotation_y(shown) * Quat::from_rotation_x(pitch);
        let eye = root.transform_point(frame.skeleton.eye()) + rotation * Vec3::NEG_Z * EYE_AHEAD * root.scale.x;
        Transform::from_translation(eye).with_rotation(rotation)
    } else {
        behind(&level, feet, shown, pitch)
    };
    if let Some(mut tr) = world.get_mut::<Transform>(cam) {
        *tr = place;
    }
    if let Some(mut projection) = world.get_mut::<Projection>(cam) {
        if let Projection::Perspective(p) = projection.as_mut() {
            p.near = NEAR;
        }
    }
    let _ = now;
    markers(world, Some(place.translation));
}

/// The markers on the small hotdogs for the titan's player (`eye` its camera; None hides them): a ring on the floor
/// round each living small hotdog, as big on the screen near or far, the colour of a warning.
fn markers(world: &mut World, eye: Option<Vec3>) {
    let spots: Vec<Vec3> = match (eye, world.get_resource::<Game>()) {
        (Some(_), Some(g)) => g.roster.iter().filter(|p| p.alive && Some(p.id) != super::titan_id(g) && !p.consumed).map(|p| Vec3::from_array(p.p)).collect(),
        _ => vec![],
    };
    if spots.is_empty() && world.resource::<View>().markers.is_empty() {
        return;
    }
    let kit = match world.resource::<View>().marker_kit.clone() {
        Some(k) => k,
        None => {
            let mesh = world.resource_mut::<Assets<Mesh>>().add(Torus::new(0.66, 1.0).mesh().minor_resolution(6).major_resolution(24));
            let material = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.82, 0.25, 0.75),
                emissive: LinearRgba::rgb(1.4, 0.9, 0.2),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                fog_enabled: false,
                ..default()
            });
            world.resource_mut::<View>().marker_kit = Some((mesh.clone(), material.clone()));
            (mesh, material)
        }
    };
    while world.resource::<View>().markers.len() < spots.len() {
        let e = world.spawn((Name::new("Titan's marker on a small hotdog"), Mesh3d(kit.0.clone()), MeshMaterial3d(kit.1.clone()), Transform::default(), Visibility::Hidden, bevy::light::NotShadowCaster)).id();
        world.resource_mut::<View>().markers.push(e);
    }
    let all = world.resource::<View>().markers.clone();
    for (k, e) in all.into_iter().enumerate() {
        match (spots.get(k), eye) {
            (Some(p), Some(eye)) => {
                let d = eye.distance(*p);
                // A ring on the floor round it (over it the name tag is drawn), about 3 degrees of the view across whatever
                // the distance, never smaller than a hotdog's own shadow.
                let size = (d * 0.026).max(0.06);
                let at = *p + Vec3::Y * 0.012;
                if let Some(mut tr) = world.get_mut::<Transform>(e) {
                    *tr = Transform::from_translation(at).with_scale(Vec3::new(size, size * 0.15, size));
                }
                if world.get::<Visibility>(e) != Some(&Visibility::Visible) {
                    world.entity_mut(e).insert(Visibility::Visible);
                }
            }
            _ => {
                if world.get::<Visibility>(e) != Some(&Visibility::Hidden) {
                    world.entity_mut(e).insert(Visibility::Hidden);
                }
            }
        }
    }
}

// ----- The shake

/// How hard (radians) a blow of `strength` (1 a full footfall, about 3 a smash) shakes a camera `distance` from it:
/// hard right under it, falling off over a couple of metres, and still a tremor across the room.
pub fn jolt(distance: f32, strength: f32) -> f32 {
    let near = 0.55 + 0.35 * strength.min(3.);
    0.012 * strength * (0.85 / (1. + (distance / near).powi(2)) + 0.15 / (1. + distance / 2.5))
}
/// The ground shook `strength` hard at `at` (a footfall, a smash, a landing, a fall): every camera near feels it.
pub fn quake(world: &mut World, at: Vec3, strength: f32, rumble: f32) {
    let Some(eye) = eye(world) else {
        return;
    };
    let amp = jolt(eye.distance(at), strength);
    if amp < 1e-4 {
        return;
    }
    let mut v = world.resource_mut::<View>();
    let seed = (at.x * 17.3 + at.z * 3.1 + amp * 101.).fract() * 100.;
    let k = (0..v.kicks.len()).min_by(|a, b| left(&v.kicks[*a]).total_cmp(&left(&v.kicks[*b]))).unwrap_or(0);
    v.kicks[k] = Kick { age: 0., amp, seed, rumble: rumble.max(0.3) };
}
fn left(k: &Kick) -> f32 {
    if k.amp <= 0. || k.age >= SHAKE_LONG * k.rumble { 0. } else { k.amp * (-k.age / (SHAKE_TAU * k.rumble)).exp() }
}
/// The shake now, as a turn (radians) about the camera's side, its up and its forward axis.
fn shaken(v: &View) -> Vec3 {
    let mut sum = Vec3::ZERO;
    for k in &v.kicks {
        let strength = left(k) * (k.age / 0.012).min(1.);
        if strength <= 0. {
            continue;
        }
        let (t, s, r) = (k.age, k.seed, 1. / k.rumble);
        let wobble = Vec3::new(0.7 * (t * TAU * 13. * r + s).sin() + 0.3 * (t * TAU * 23. * r + 2. * s).sin(), 0.6 * (t * TAU * 9. * r + 3. * s).sin(), 0.5 * (t * TAU * 7. * r + 5. * s).sin());
        // A thud: down at first, then the wobble.
        sum += (wobble + Vec3::new(-1.0 * (-t / 0.05).exp(), 0., 0.)) * strength;
    }
    sum.clamp_length_max(SHAKE_MOST)
}
/// Where the game's camera is as last placed.
fn eye(world: &mut World) -> Option<Vec3> {
    let cam = world.get_resource::<Game>()?.camera()?;
    world.get::<Transform>(cam).map(|t| t.translation)
}

// ----- Keeping out of the giant

/// The titan's body as the camera keeps out of it: capsules (a, b, radius) in the world, from the drawn skeleton.
pub fn hulls(s: &Skeleton, root: &Transform) -> Vec<(Vec3, Vec3, f32)> {
    let w = |p: Vec3| root.transform_point(p);
    let k = root.scale.x;
    let mut v = vec![
        // The torso link, the head, each upper arm, forearm, hand, thigh, shin and foot.
        (w(s.axis(super::body::BOTTOM + 0.18)), w(s.axis(super::body::TORSO_TOP - 0.2)), 0.31 * k),
        (w(s.on_head(Vec3::new(0., 0.12, 0.))), w(s.on_head(Vec3::new(0., 0.34, 0.))), 0.25 * k),
    ];
    for i in [L, R] {
        v.push((w(s.shoulder[i]), w(s.elbow[i]), 0.09 * k));
        v.push((w(s.elbow[i]), w(s.wrist[i]), 0.1 * k));
        v.push((w(s.wrist[i]), w(s.palm(i) + (s.palm(i) - s.wrist[i]) * 0.6), 0.15 * k));
        v.push((w(s.hip[i]), w(s.knee[i]), 0.115 * k));
        v.push((w(s.knee[i]), w(s.ankle[i]), 0.095 * k));
        v.push((w(s.ankle[i] + Vec3::new(0., -0.04, 0.)), w(s.sole(i) + (s.feet[i].rotation * Vec3::NEG_Z) * 0.12 + Vec3::Y * 0.02), 0.08 * k));
    }
    v
}
/// `eye` moved, if it is inside any of `hulls` (grown by CLEAR), out of it the shortest way (a few rounds, for where
/// two meet).
pub fn keep_out(eye: Vec3, hulls: &[(Vec3, Vec3, f32)]) -> Vec3 {
    let inside = |eye: Vec3| {
        hulls.iter().any(|(a, b, r)| {
            let ab = *b - *a;
            let t = if ab.length_squared() > 1e-9 { ((eye - *a).dot(ab) / ab.length_squared()).clamp(0., 1.) } else { 0. };
            eye.distance(*a + ab * t) < r + CLEAR * 0.99
        })
    };
    let start = eye;
    let mut eye = eye;
    for _ in 0..8 {
        let mut moved = false;
        for (a, b, r) in hulls {
            let ab = *b - *a;
            let t = if ab.length_squared() > 1e-9 { ((eye - *a).dot(ab) / ab.length_squared()).clamp(0., 1.) } else { 0. };
            let on = *a + ab * t;
            let d = eye.distance(on);
            let want = r + CLEAR;
            if d < want {
                let away = if d > 1e-5 { (eye - on) / d } else { ab.any_orthonormal_vector() };
                eye = on + away * want;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    // Wedged where two parts meet: straight out from the body's middle (its first hull) until clear of all of it.
    if inside(eye) {
        let (a, b, _) = hulls.first().copied().unwrap_or((start, start, 0.));
        let ab = b - a;
        let t = if ab.length_squared() > 1e-9 { ((start - a).dot(ab) / ab.length_squared()).clamp(0., 1.) } else { 0. };
        let away = (start - (a + ab * t)).normalize_or(Vec3::X);
        eye = start;
        for _ in 0..200 {
            if !inside(eye) {
                break;
            }
            eye += away * 0.02;
        }
    }
    eye
}

/// Before the next frame places the camera: what `settle` did to it undone (if nothing has put it anywhere since).
pub fn restore(world: &mut World) {
    let Some((cam, base, written)) = world.resource_mut::<View>().base.take() else {
        return;
    };
    if let Some(mut tr) = world.get_mut::<Transform>(cam) {
        if *tr == written {
            *tr = base;
        }
    }
}
/// After everything has placed the camera, before it is drawn: the shake, and the camera kept out of the giant (but for
/// the titan's own eyes, which are in its head).
pub fn settle(world: &mut World) {
    let dt = world.resource::<Time<Real>>().delta_secs().min(0.1);
    for k in world.resource_mut::<View>().kicks.iter_mut() {
        if k.amp > 0. {
            k.age += dt;
            if k.age >= SHAKE_LONG * k.rumble {
                k.amp = 0.;
            }
        }
    }
    let Some(cam) = world.get_resource::<Game>().and_then(|g| g.camera()) else {
        return;
    };
    let Some(base) = world.get::<Transform>(cam).copied() else {
        return;
    };
    let mut tr = base;
    let shake = shaken(&world.resource::<View>());
    if shake != Vec3::ZERO {
        tr.rotation = tr.rotation * Quat::from_rotation_x(shake.x) * Quat::from_rotation_y(shake.y) * Quat::from_rotation_z(shake.z);
    }
    if !first_person(world) {
        if let Some((_, frame, root)) = world.resource::<Titans>().drawn {
            tr.translation = keep_out(tr.translation, &hulls(&frame.skeleton, &root));
        }
    }
    if tr != base {
        if let Some(mut t) = world.get_mut::<Transform>(cam) {
            *t = tr;
        }
        world.resource_mut::<View>().base = Some((cam, base, tr));
    }
}

#[cfg(test)]
pub(crate) fn kicks_live(world: &World) -> usize {
    world.resource::<View>().kicks.iter().filter(|k| left(k) > 0.).count()
}
#[allow(dead_code)]
const _SIDES: [usize; 2] = [L, R];

/// A death camera's shot (deathcam.rs) watching the titan: as the death camera wanted it, but far enough off and high
/// enough to see the whole giant (its chest in the middle of the picture), and inside the room.
pub fn framed(shot: crate::deathcam::Shot, s: &Skeleton, root: &Transform, bounds: &crate::visuals::ArenaBounds) -> crate::deathcam::Shot {
    let chest = root.transform_point(s.axis(1.0));
    let away = Vec3::new(shot.eye.x - chest.x, 0., shot.eye.z - chest.z).normalize_or(root.rotation * Vec3::Z);
    let mut eye = chest + away * 2.4 + Vec3::Y * 0.7;
    eye.x = eye.x.clamp(bounds.min.x + 0.15, bounds.max.x - 0.15);
    eye.z = eye.z.clamp(bounds.min.y + 0.15, bounds.max.y - 0.15);
    eye.y = eye.y.min(bounds.ceiling - 0.15).max(bounds.floor + 0.2);
    let turn = Transform::from_translation(eye).looking_at(chest, Vec3::Y).rotation;
    crate::deathcam::Shot { eye, turn, fov: shot.fov }
}

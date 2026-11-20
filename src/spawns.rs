// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Pickups as spawn points, on screen. A map's pickups come back where they were a while after they are
//! taken (the simulation's `respawn`, the map's or `kitchen_sim::map::RESPAWN`), so each stands on a pad
//! that stays when the pickup is gone: a dark disc ringed by 24 lights. While its pickup is there the ring
//! is lit all round, amber for a gun and green for a box of ammunition; once it is taken the ring goes dark
//! and lights again one segment at a time as the pickup's return draws near, the next segment blinking, so
//! that anyone can see how long it will be. A Forts map's pickups appear only when the forcefield drops:
//! until then their pads are dark. `arena` builds the gun models and calls [`pad`], [`ammo_box`] and
//! [`show`].
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Segments in a pad's ring, how far out they stand and how big the pad is.
const SEGMENTS: usize = 24;
const RING: f32 = 0.047;
const PAD: f32 = 0.056;
/// A pickup that has come back grows to its size over this many seconds.
const GROW: f32 = 0.3;
/// `ready` beyond this is not a time but "not yet" (a Forts pickup while the forcefield stands).
const NEVER: f32 = 1e6;

/// One pickup's pad: its ring of lights as three arcs (the lit segments, the dark ones and the one blinking
/// between them, each one mesh: see `Kit::arcs`), whether it holds a box of ammunition, what the ring showed
/// last (segments lit, and whether the next blinks), and when its pickup last came back.
pub struct Pad {
    lit: Entity,
    dark: Entity,
    blink: Entity,
    ammo: bool,
    shown: Option<(usize, bool)>,
    back: Option<f32>,
}

#[derive(Resource)]
struct Kit {
    /// The ring's first k segments as one mesh, for k from 0 to SEGMENTS (an arc of k is turned round to
    /// stand anywhere on the ring); 0 is never shown.
    arcs: Vec<Handle<Mesh>>,
    disc: Handle<Mesh>,
    cube: Handle<Mesh>,
    capsule: Handle<Mesh>,
    sphere: Handle<Mesh>,
    base: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    /// Lit, for a gun and for a box of ammunition.
    lit: [Handle<StandardMaterial>; 2],
    blink: [Handle<StandardMaterial>; 2],
    olive: Handle<StandardMaterial>,
    lid: Handle<StandardMaterial>,
    brass: Handle<StandardMaterial>,
    copper: Handle<StandardMaterial>,
}
impl FromWorld for Kit {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let arcs = (0..=SEGMENTS).map(|k| meshes.add(arc(k.max(1)))).collect();
        let (disc, cube, capsule, sphere) = (
            meshes.add(Cylinder::new(1., 1.).mesh().resolution(32)),
            meshes.add(Cuboid::default()),
            meshes.add(Capsule3d::new(1., 1.).mesh().longitudes(10).latitudes(4)),
            meshes.add(Sphere::new(1.).mesh().uv(10, 6)),
        );
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let mut glow = |c: [f32; 3], shine: f32| {
            materials.add(StandardMaterial { base_color: Color::srgb(c[0], c[1], c[2]), emissive: LinearRgba::rgb(c[0] * shine, c[1] * shine, c[2] * shine), perceptual_roughness: 0.4, ..default() })
        };
        let (amber, green) = ([1.0, 0.62, 0.16], [0.42, 0.95, 0.30]);
        let lit = [glow(amber, 2.2), glow(green, 2.0)];
        let blink = [glow(amber, 5.), glow(green, 4.5)];
        let dark = glow([0.20, 0.23, 0.25], 0.08);
        let mut plain = |c: [f32; 3], roughness: f32, metallic: f32| materials.add(StandardMaterial { base_color: Color::srgb(c[0], c[1], c[2]), perceptual_roughness: roughness, metallic, ..default() });
        let base = plain([0.06, 0.07, 0.075], 0.55, 0.4);
        let (olive, lid) = (plain([0.33, 0.37, 0.16], 0.7, 0.), plain([0.20, 0.23, 0.10], 0.6, 0.));
        let (brass, copper) = (plain([0.86, 0.66, 0.26], 0.3, 1.), plain([0.80, 0.40, 0.22], 0.35, 1.));
        Kit { arcs, disc, cube, capsule, sphere, base, dark, lit, blink, olive, lid, brass, copper }
    }
}

/// Where segment `i` of a ring stands round the pad's middle, and how it is turned: clockwise from the far
/// side, as a clock's hand goes round seen from above.
fn segment(i: usize) -> Transform {
    let angle = -(i as f32 + 0.5) / SEGMENTS as f32 * TAU;
    Transform::from_xyz(angle.sin() * RING, 0.0034, -angle.cos() * RING).with_rotation(Quat::from_rotation_y(-angle))
}
/// The ring's first `k` segments, as one mesh.
fn arc(k: usize) -> Mesh {
    let piece = || Cuboid::new(TAU * RING / SEGMENTS as f32 * 0.72, 0.0022, 0.0055).mesh().build();
    let mut mesh = piece().transformed_by(segment(0));
    for i in 1..k {
        let _ = mesh.merge(&piece().transformed_by(segment(i)));
    }
    mesh
}
/// Turns an arc that starts at segment 0 so that it starts at segment `i`.
fn from(i: usize) -> Quat {
    Quat::from_rotation_y(i as f32 / SEGMENTS as f32 * TAU)
}

fn part(world: &mut World, parent: Entity, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Vec3, scale: Vec3, turn: Quat) -> Entity {
    let e = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(at).with_rotation(turn).with_scale(scale))).id();
    world.entity_mut(parent).add_child(e);
    e
}

/// Lays a pad on the counter at `ground` (the surface under the pickup), for a gun or (`ammo`) a box of
/// ammunition. It is a `MapPart`, so the next map replaces it.
pub fn pad(world: &mut World, ground: Vec3, ammo: bool) -> Pad {
    world.init_resource::<Kit>();
    let kit = world.resource::<Kit>();
    let (disc, base, dark, all, lit, blink) = (kit.disc.clone(), kit.base.clone(), kit.dark.clone(), kit.arcs[SEGMENTS].clone(), kit.lit[ammo as usize].clone(), kit.blink[ammo as usize].clone());
    let one = kit.arcs[1].clone();
    let root = world.spawn((crate::level::MapPart, Name::new("Pickup pad"), Transform::from_translation(ground), Visibility::default())).id();
    let disc = part(world, root, &disc, &base, Vec3::Y * 0.0015, Vec3::new(PAD, 0.003, PAD), Quat::IDENTITY);
    let lit = part(world, root, &all, &lit, Vec3::ZERO, Vec3::ONE, Quat::IDENTITY);
    let dark = part(world, root, &all, &dark, Vec3::ZERO, Vec3::ONE, Quat::IDENTITY);
    let blink = part(world, root, &one, &blink, Vec3::ZERO, Vec3::ONE, Quat::IDENTITY);
    // Flat on the counter, it has no shadow worth casting. The disc never changes (`bake`); the rings do.
    for entity in [disc, lit, dark, blink] {
        world.entity_mut(entity).insert(bevy::light::NotShadowCaster);
    }
    world.entity_mut(disc).insert(crate::bake::Still);
    Pad { lit, dark, blink, ammo, shown: None, back: None }
}

/// The model of a box of ammunition, under `root`: an olive crate with a darker lid band and three brass
/// rounds standing in it, about the size a gun's pickup is.
pub fn ammo_box(world: &mut World, root: Entity) {
    world.init_resource::<Kit>();
    let kit = world.resource::<Kit>();
    let (cube, capsule, sphere, olive, lid, brass, copper) = (kit.cube.clone(), kit.capsule.clone(), kit.sphere.clone(), kit.olive.clone(), kit.lid.clone(), kit.brass.clone(), kit.copper.clone());
    part(world, root, &cube, &olive, Vec3::Y * 0.004, Vec3::new(0.056, 0.03, 0.038), Quat::IDENTITY);
    part(world, root, &cube, &lid, Vec3::Y * 0.0205, Vec3::new(0.058, 0.004, 0.040), Quat::IDENTITY);
    part(world, root, &cube, &lid, Vec3::new(0., 0.004, 0.0191), Vec3::new(0.02, 0.014, 0.002), Quat::IDENTITY);
    for k in 0..3 {
        let x = (k as f32 - 1.) * 0.014;
        part(world, root, &capsule, &brass, Vec3::new(x, 0.03, 0.), Vec3::new(0.0045, 0.0045, 0.0045), Quat::IDENTITY);
        part(world, root, &sphere, &copper, Vec3::new(x, 0.0378, 0.), Vec3::splat(0.0042), Quat::IDENTITY);
    }
}

/// How far a pickup that has been taken is through its time away, 0 just taken to 1 back; None while it
/// is there, and for one that is not coming yet (a Forts pickup behind the forcefield).
pub fn progress(ready: f32, respawn: f32, now: f32) -> Option<f32> {
    let left = ready - now;
    (left > 0. && ready < NEVER).then(|| (1. - left / respawn.max(0.1)).clamp(0., 1.))
}

/// Every frame: the pad's ring for a pickup due back at `ready` that takes `respawn` seconds to come back,
/// and the pickup's own model (`item`), shown while it is there and grown back to its size when it returns.
pub fn show(world: &mut World, pad: &mut Pad, item: Entity, ready: f32, respawn: f32, now: f32) {
    let there = ready <= now;
    let away = progress(ready, respawn, now);
    if there && pad.back.is_none() {
        pad.back = Some(now);
    } else if !there {
        pad.back = None;
    }
    let grown = pad.back.map_or(0., |t| ((now - t) / GROW).clamp(0., 1.));
    if let Some(mut vis) = world.get_mut::<Visibility>(item) {
        let want = if there { Visibility::Visible } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
    if let Some(mut tr) = world.get_mut::<Transform>(item) {
        // A little overshoot as it pops back.
        let t = grown;
        let scale = Vec3::splat(if t >= 1. { 1. } else { (t * (1. + 0.6 * (1. - t) * 2.)).max(0.05) });
        // (Written only when it changes: a write has the engine place the whole model again.)
        if tr.scale != scale {
            tr.scale = scale;
        }
    }
    let lit = match away {
        _ if there => SEGMENTS,
        Some(done) => ((done * SEGMENTS as f32).floor() as usize).min(SEGMENTS - 1),
        None => 0,
    };
    // The segment after the lit ones blinks while the pickup is on its way back.
    let blinking = away.is_some() && (now * 4.).fract() < 0.5;
    if pad.shown == Some((lit, blinking)) {
        return;
    }
    pad.shown = Some((lit, blinking));
    let arcs = world.resource::<Kit>().arcs.clone();
    let dark = SEGMENTS - lit - blinking as usize;
    for (entity, count, start) in [(pad.lit, lit, 0), (pad.blink, blinking as usize, lit), (pad.dark, dark, lit + blinking as usize)] {
        world.entity_mut(entity).insert((Mesh3d(arcs[count].clone()), if count > 0 { Visibility::Inherited } else { Visibility::Hidden }));
        if let Some(mut tr) = world.get_mut::<Transform>(entity) {
            tr.rotation = from(start);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_ring_counts_a_pickup_back_and_a_forts_pickup_behind_the_field_is_not_coming_yet() {
        assert_eq!(progress(0., 20., 5.), None, "there");
        assert_eq!(progress(25., 20., 5.), Some(0.), "just taken");
        assert_eq!(progress(15., 20., 5.), Some(0.5));
        assert_eq!(progress(kitchen_sim::forts::LATER, 20., 5.), None, "not yet: the field stands");
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        let mut pad = pad(&mut world, Vec3::ZERO, true);
        let item = world.spawn((Transform::default(), Visibility::Visible)).id();
        let lit = |world: &World, pad: &Pad| {
            let arcs = &world.resource::<Kit>().arcs;
            let shown = world.get::<Visibility>(pad.lit).is_some_and(|v| *v != Visibility::Hidden);
            let mesh = world.get::<Mesh3d>(pad.lit).map(|m| m.0.clone());
            if shown { arcs.iter().position(|a| Some(a.clone()) == mesh).unwrap_or(0) } else { 0 }
        };
        show(&mut world, &mut pad, item, 0., 12., 1.);
        assert_eq!((lit(&world, &pad), world.get::<Visibility>(item).copied()), (SEGMENTS, Some(Visibility::Visible)));
        show(&mut world, &mut pad, item, 10., 12., 4.);
        assert_eq!((lit(&world, &pad), world.get::<Visibility>(item).copied()), (12, Some(Visibility::Hidden)), "half way back, half the ring");
        show(&mut world, &mut pad, item, 10., 12., 10.05);
        assert_eq!(lit(&world, &pad), SEGMENTS);
        assert!(world.get::<Transform>(item).unwrap().scale.x < 0.5, "it grows back");
    }
}

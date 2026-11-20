// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! What a kung fu blow looks like where it lands, and on its way: a flash and sauce droplets for a hit (more for a heavy
//! one), white sparks for a block, a bright ring for a parry, a puff of dust for a knockdown, a burst for a special;
//! and a whoosh of light trailing a fast fist or foot. The flashes come from the simulation's events (`View::events`:
//! where the victim's chest was, who struck), so they are the same offline and online and for everybody's blows.
use super::*;
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    light::NotShadowCaster,
    mesh::Indices,
    render::render_resource::PrimitiveTopology,
};
use std::collections::HashMap;

/// Seconds a whoosh's light lasts behind the limb.
pub const LIFE: f32 = 0.09;

struct Look {
    sphere: Handle<Mesh>,
    ring: Handle<Mesh>,
    flash: Handle<StandardMaterial>,
    halo: Handle<StandardMaterial>,
    core: Handle<StandardMaterial>,
    spark: Handle<StandardMaterial>,
    chi: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
    bronze: Handle<StandardMaterial>,
    drops: [Handle<StandardMaterial>; 3],
    streak: Handle<StandardMaterial>,
}
/// The effects' meshes and materials, the whooshes being drawn and the events already shown.
#[derive(Resource, Default)]
pub struct Fx {
    look: Option<Look>,
    /// Where each fighter's whooshing limbs have been lately (two edges and when), by fighter and limb.
    trails: HashMap<(u32, Limb), Vec<(Vec3, Vec3, f32)>>,
    ribbons: HashMap<(u32, Limb), (Entity, Handle<Mesh>)>,
    /// The newest event shown (events are numbered from 1 each round).
    seen: u32,
    /// When this player's own blow last landed, and how hard (1 a light blow): the first-person camera's jolt.
    pub kicked: Option<(f32, f32)>,
    /// When this player was last hit, and how hard.
    pub hurt: Option<(f32, f32)>,
    /// When the ground last shook under this player, and how hard.
    pub shaken: Option<(f32, f32)>,
    /// Flecks that have had their time, hidden, to be used again. (They are never despawned: visuals.rs surveys every
    /// new mesh with a command that would fail on an entity despawned under it.)
    pool: Vec<Entity>,
}
fn unlit(materials: &mut Assets<StandardMaterial>, color: LinearRgba, add: bool) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::LinearRgba(color),
        unlit: true,
        alpha_mode: if add { AlphaMode::Add } else { AlphaMode::Opaque },
        fog_enabled: false,
        ..default()
    })
}
pub fn setup(mut fx: ResMut<Fx>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    fx.look = Some(Look {
        sphere: meshes.add(Sphere::new(1.).mesh().uv(20, 10)),
        ring: meshes.add(Torus::new(0.9, 1.).mesh().major_resolution(32).minor_resolution(6)),
        flash: unlit(&mut materials, LinearRgba::rgb(5., 4., 2.2), true),
        halo: unlit(&mut materials, LinearRgba::rgb(0.55, 0.24, 0.05), true),
        core: unlit(&mut materials, LinearRgba::rgb(9., 9., 8.), true),
        spark: unlit(&mut materials, LinearRgba::rgb(3.5, 3.6, 4.2), true),
        chi: unlit(&mut materials, LinearRgba::rgb(2.6, 1.5, 0.35), true),
        dust: unlit(&mut materials, LinearRgba::rgb(0.35, 0.3, 0.24), true),
        bronze: unlit(&mut materials, LinearRgba::rgb(0.9, 0.5, 0.15), true),
        drops: [unlit(&mut materials, LinearRgba::rgb(1.6, 1.1, 0.05), false), unlit(&mut materials, LinearRgba::rgb(1.4, 0.05, 0.03), false), unlit(&mut materials, LinearRgba::rgb(3., 2.6, 1.6), false)],
        streak: unlit(&mut materials, LinearRgba::rgb(1., 1., 1.), true),
    });
}

/// A flash, a ring or a droplet: it moves, falls, grows or shrinks, and goes.
#[derive(Component)]
pub struct Fleck {
    velocity: Vec3,
    left: f32,
    total: f32,
    size: Vec3,
    falls: bool,
    /// A ring grows as it fades rather than shrinking.
    grows: bool,
}
/// Cheap noise in 0..1.
fn hash(n: u32) -> f32 {
    crate::player_model::noise(n)
}
fn spawn_fleck(world: &mut World, mesh: Handle<Mesh>, material: Handle<StandardMaterial>, at: Transform, layer: Option<RenderLayers>, fleck: Fleck) {
    let reused = loop {
        let Some(e) = world.resource_mut::<Fx>().pool.pop() else {
            break None;
        };
        if world.get_entity(e).is_ok() {
            break Some(e);
        }
    };
    let mut e = match reused {
        Some(e) => {
            let mut e = world.entity_mut(e);
            e.insert((Mesh3d(mesh), MeshMaterial3d(material), at, Visibility::Inherited, fleck));
            e
        }
        None => world.spawn((Name::new("Kung fu fleck"), Mesh3d(mesh), MeshMaterial3d(material), at, Visibility::Inherited, NotShadowCaster, fleck)),
    };
    match layer {
        Some(layer) => {
            e.insert(layer);
        }
        None => {
            e.insert(RenderLayers::layer(0));
        }
    }
}
/// What shows where a blow lands at `at`, struck along `along`: `kind` says what the blow did.
pub fn burst(world: &mut World, at: Vec3, along: Vec3, kind: fu::EventKind, heavy: bool, seed: u32, eye: Option<Vec3>) {
    use fu::EventKind as K;
    let Some(l) = world.resource::<Fx>().look.as_ref().map(|l| (l.sphere.clone(), l.ring.clone(), l.flash.clone(), l.halo.clone(), l.core.clone(), l.spark.clone(), l.chi.clone(), l.dust.clone(), l.drops.clone())) else {
        return;
    };
    let (sphere, ring, flash, halo, core, spark, chi, dust, drops) = l;
    let along = along.normalize_or(Vec3::NEG_Z);
    let side = along.cross(Vec3::Y).normalize_or(Vec3::X);
    // (This player's own blow is drawn in the first-person pass, nearer the eye and smaller to look the same, so that
    // its own fist at the spot does not hide it.)
    // (Kung fu blows land a hand's breadth from the eye, much nearer than a gun's: the flash is kept to a small burst at
    // the knuckles rather than its full size seen from right on top of it.)
    const NEAR: f32 = 0.6;
    let (layer, place, shrink) = match eye {
        Some(eye) => (Some(RenderLayers::layer(crate::viewmodel::VIEW_LAYER)), eye.lerp(at, NEAR), NEAR * 0.4),
        None => (None, at, 1.),
    };
    let flashes: &[(f32, &Handle<StandardMaterial>, f32)] = match kind {
        K::Hit | K::Guardbroken | K::Special => &[(1.35, &halo, 0.11), (0.8, &flash, 0.09), (0.38, &core, 0.07)],
        K::Blocked => &[(0.9, &spark, 0.07), (0.35, &core, 0.05)],
        K::Parried => &[(1.1, &spark, 0.12), (0.45, &core, 0.08)],
        _ => &[],
    };
    let size = if heavy { 0.032 } else { 0.022 } * shrink;
    for (share, material, time) in flashes {
        let s = Vec3::splat(size * share);
        spawn_fleck(world, sphere.clone(), (*material).clone(), Transform::from_translation(place).with_scale(s), layer.clone(), Fleck { velocity: Vec3::ZERO, left: *time, total: *time, size: s, falls: false, grows: false });
    }
    // A ring flung out across the blow: white for a parry, gold for a heavy hit or a special.
    let rings: Option<(&Handle<StandardMaterial>, f32)> = match kind {
        K::Parried => Some((&spark, 0.05)),
        K::Hit | K::Guardbroken if heavy => Some((&chi, 0.045)),
        K::Special => Some((&chi, 0.07)),
        _ => None,
    };
    if let Some((material, reach)) = rings {
        let s = Vec3::new(reach, reach * 0.06, reach) * shrink;
        let turn = Quat::from_rotation_arc(Vec3::Y, along);
        spawn_fleck(world, ring.clone(), material.clone(), Transform::from_translation(place).with_rotation(turn).with_scale(s * 0.2), layer.clone(), Fleck { velocity: Vec3::ZERO, left: 0.16, total: 0.16, size: s, falls: false, grows: true });
    }
    // Sauce flies from a hit; sparks from a block.
    let (count, materials): (u32, Vec<Handle<StandardMaterial>>) = match kind {
        K::Hit | K::Guardbroken => (if heavy { 18 } else { 9 }, drops.to_vec()),
        K::Blocked | K::Parried => (6, vec![spark.clone()]),
        _ => (0, vec![]),
    };
    for i in 0..count {
        let r = |k: u32| hash(seed.wrapping_mul(131).wrapping_add(i * 7 + k));
        let spread = side * (r(1) - 0.5) * 1.5 + Vec3::Y * (0.15 + r(2) * 0.9);
        let velocity = (along * (0.25 + r(3) * 0.9) + spread) * if heavy { 1.15 } else { 0.85 };
        let radius = (0.0022 + r(4) * 0.003) * if heavy { 1.2 } else { 1. };
        let left = 0.3 + r(5) * 0.25;
        let s = Vec3::splat(radius);
        let material = materials[(i as usize) % materials.len()].clone();
        spawn_fleck(world, sphere.clone(), material, Transform::from_translation(at + along * 0.01).with_scale(s), None, Fleck { velocity, left, total: left, size: s, falls: true, grows: false });
    }
    if kind == K::Knockdown {
        // A puff of dust where it hits the counter.
        for i in 0..7 {
            let r = |k: u32| hash(seed.wrapping_mul(97).wrapping_add(i * 11 + k));
            let a = r(1) * TAU;
            let velocity = Vec3::new(a.cos(), 0.15 + 0.2 * r(2), a.sin()) * (0.12 + 0.12 * r(3));
            let s = Vec3::splat(0.008 + 0.006 * r(4));
            spawn_fleck(world, sphere.clone(), dust.clone(), Transform::from_translation(Vec3::new(at.x, at.y - 0.06, at.z)).with_scale(s * 0.3), None, Fleck { velocity, left: 0.35, total: 0.35, size: s, falls: false, grows: true });
        }
    }
}
/// A Toad's iron body taking a blow: a bronze glow over its body, gone in a quarter of a second.
pub fn sheen(world: &mut World, middle: Vec3) {
    let Some((sphere, bronze)) = world.resource::<Fx>().look.as_ref().map(|l| (l.sphere.clone(), l.bronze.clone())) else {
        return;
    };
    let s = v(0.034, 0.088, 0.034);
    spawn_fleck(world, sphere, bronze, Transform::from_translation(middle).with_scale(s), None, Fleck { velocity: Vec3::ZERO, left: 0.25, total: 0.25, size: s, falls: false, grows: false });
}
// KF5: the rest of a Toad's iron body taking a blow (fx `events`, `Absorbed`).
/// A bronze ripple spreading out round the Toad's bun from the height the blow struck (`at`, on its axis).
pub fn ripple(world: &mut World, at: Vec3) {
    let Some((ring, bronze)) = world.resource::<Fx>().look.as_ref().map(|l| (l.ring.clone(), l.bronze.clone())) else {
        return;
    };
    for (i, (size, life)) in [(0.05f32, 0.24f32), (0.042, 0.3)].into_iter().enumerate() {
        // (A round hoop, not a flat ring: it is seen from the side.)
        let s = v(size, size * 0.6, size);
        let at = at + Vec3::Y * 0.012 * i as f32;
        spawn_fleck(world, ring.clone(), bronze.clone(), Transform::from_translation(at).with_scale(s * 0.2), None, Fleck { velocity: Vec3::Y * 0.05 * (i as f32 * 2. - 1.), left: life, total: life, size: s, falls: false, grows: true });
    }
}
/// The striker's side: the blow bounces off with a bright ring facing back at it (`along` is the way it struck) and a
/// spray of sparks flung back at it; the striker itself reels (the simulation's `IRON_RECOIL`).
pub fn recoil(world: &mut World, at: Vec3, along: Vec3, seed: u32) {
    let Some((sphere, ring, bronze, spark, core)) = world.resource::<Fx>().look.as_ref().map(|l| (l.sphere.clone(), l.ring.clone(), l.bronze.clone(), l.spark.clone(), l.core.clone())) else {
        return;
    };
    let back = -along.normalize_or(Vec3::NEG_Z);
    let side = back.cross(Vec3::Y).normalize_or(Vec3::X);
    let s = v(0.034, 0.02, 0.034);
    spawn_fleck(world, ring, spark.clone(), Transform::from_translation(at).with_rotation(Quat::from_rotation_arc(Vec3::Y, back)).with_scale(s * 0.2), None, Fleck { velocity: back * 0.3, left: 0.2, total: 0.2, size: s, falls: false, grows: true });
    let s = Vec3::splat(0.011);
    spawn_fleck(world, sphere.clone(), core, Transform::from_translation(at).with_scale(s), None, Fleck { velocity: Vec3::ZERO, left: 0.07, total: 0.07, size: s, falls: false, grows: false });
    for i in 0..10u32 {
        let r = |k: u32| hash(seed.wrapping_mul(193).wrapping_add(i * 13 + k));
        let velocity = back * (0.35 + r(1) * 0.5) + side * (r(2) - 0.5) * 0.7 + Vec3::Y * (0.15 + r(3) * 0.45);
        let left = 0.22 + r(4) * 0.18;
        let s = Vec3::splat(0.0022 + r(5) * 0.0018);
        let material = if i % 2 == 0 { bronze.clone() } else { spark.clone() };
        spawn_fleck(world, sphere.clone(), material, Transform::from_translation(at).with_scale(s), None, Fleck { velocity, left, total: left, size: s, falls: true, grows: false });
    }
}
/// The ground shaking under a hotdog standing at `at` (on the counter), `strength` hard: a ripple across the counter,
/// and a jolt for this player's camera if it is near (`me`, where this player stands).
pub fn quake(world: &mut World, at: Vec3, strength: f32, me: Option<Vec3>, now: f32) {
    let Some((ring, dust)) = world.resource::<Fx>().look.as_ref().map(|l| (l.ring.clone(), l.dust.clone())) else {
        return;
    };
    let reach = 0.25 * strength.sqrt();
    let s = v(reach, reach * 0.02, reach);
    spawn_fleck(world, ring, dust, Transform::from_translation(at + Vec3::Y * 0.002).with_scale(s * 0.1), None, Fleck { velocity: Vec3::ZERO, left: 0.4, total: 0.4, size: s, falls: false, grows: true });
    if let Some(me) = me {
        let near = 1. - me.distance(at) / 0.35;
        if near > 0. {
            world.resource_mut::<Fx>().shaken = Some((now, strength * near));
        }
    }
}
/// Moves and fades the flecks, and puts away those whose time is up.
pub fn flecks(time: Res<Time<Real>>, mut fx: ResMut<Fx>, mut flecks: Query<(Entity, &mut Transform, &mut Fleck, &mut Visibility)>) {
    let dt = time.delta_secs();
    for (entity, mut tr, mut fleck, mut shown) in &mut flecks {
        if fleck.left <= 0. {
            continue;
        }
        fleck.left -= dt;
        if fleck.left <= 0. {
            *shown = Visibility::Hidden;
            fx.pool.push(entity);
            continue;
        }
        let life = (fleck.left / fleck.total).clamp(0., 1.);
        if fleck.falls {
            fleck.velocity.y -= 2.8 * dt;
            tr.translation += fleck.velocity * dt;
            if tr.translation.y < kitchen_sim::feast::FLOOR + 0.002 {
                tr.translation.y = kitchen_sim::feast::FLOOR + 0.002;
                fleck.velocity = Vec3::ZERO;
                fleck.falls = false;
            }
            tr.scale = fleck.size * (life * 3.).min(1.);
        } else if fleck.grows {
            tr.translation += fleck.velocity * dt;
            fleck.velocity *= (-dt * 6.).exp();
            tr.scale = fleck.size * (0.2 + 0.8 * (1. - life).sqrt()) * life.sqrt().max(0.3);
        } else {
            tr.scale = fleck.size * life.sqrt();
        }
    }
}

/// Once a frame for each fighter (rig.rs `animate`): where its whooshing limbs are now (`skeleton`, in its own frame;
/// `root`, where it stands), and for this player's own (`mine`) a blow that landed, for the camera's jolt.
pub fn strike(world: &mut World, id: u32, mine: bool, root: &Transform, skeleton: &Skeleton, frame: &Frame, now: f32) {
    let mut fx = world.resource_mut::<Fx>();
    if mine {
        if let Some((_, strength)) = frame.landed {
            fx.kicked = Some((now, strength));
        }
    }
    for limb in frame.trails.iter().flatten() {
        let (a, b) = (root.transform_point(skeleton.tip(*limb)), root.transform_point(skeleton.heel(*limb)));
        let trail = fx.trails.entry((id, *limb)).or_default();
        // (A sample every few milliseconds, however fast the frames come.)
        if trail.last().is_none_or(|(p, _, born)| now - born > 0.004 && p.distance(a) > 0.0008) {
            trail.push((a, b, now));
        }
    }
}
/// The simulation's new events, each shown once: the flashes, and the first person's jolts.
pub fn events(world: &mut World, roster: &sim::Roster, now: f32) {
    let eye = world
        .get_resource::<crate::arena::Game>()
        .and_then(|g| g.camera())
        .and_then(|camera| world.get::<GlobalTransform>(camera))
        .map(|at| at.translation());
    let places: Vec<(u32, Vec3)> = world.query::<(&crate::player_model::Gait, &Transform)>().iter(world).map(|(g, tr)| (g.id(), tr.translation)).collect();
    let seen = world.resource::<Fx>().seen;
    let newest = roster.events.iter().map(|e| e.id).max().unwrap_or(0);
    // (A new round numbers its events from 1 again.)
    let seen = if newest < seen { 0 } else { seen };
    for e in roster.events.iter().filter(|e| e.id > seen) {
        let Some(kind) = e.kind else {
            continue;
        };
        let at = Vec3::from_array(e.p);
        let from = places.iter().find(|(id, _)| *id == e.by).map(|(_, p)| *p + Vec3::Y * 0.1);
        let along = from.map_or(Vec3::NEG_Z, |from| Vec3::new(at.x - from.x, 0., at.z - from.z).normalize_or(Vec3::NEG_Z));
        let heavy = fu::Move::from_u8(e.mv).is_some_and(|m| matches!(m.data().kind, fu::Kind::Heavy | fu::Kind::Special) || m.data().knockdown) || e.amount >= 15.;
        let mine = e.by == roster.me && e.by != 0;
        // (Where the blow lands: the front of the victim, towards the striker.)
        let spot = at - along * 0.02;
        burst(world, spot, along, kind, heavy, e.id.wrapping_mul(2654435761) ^ e.by, if mine { eye } else { None });
        if kind == fu::EventKind::Absorbed {
            // A Toad's iron body: a bronze sheen over the one who shrugged it off.
            let toad = places.iter().find(|(id, _)| *id == e.to).map_or(at - Vec3::Y * 0.02, |(_, p)| *p + Vec3::Y * 0.1);
            sheen(world, toad);
            // KF5: and a ripple round its bun at the height of the blow, and the blow bouncing back at its striker.
            ripple(world, Vec3::new(toad.x, spot.y, toad.z));
            recoil(world, spot, along, e.id.wrapping_mul(2246822519) ^ e.by);
        }
        let mut fx = world.resource_mut::<Fx>();
        let strength = if heavy { 1.9 } else { 1. };
        if mine && matches!(kind, fu::EventKind::Hit | fu::EventKind::Guardbroken | fu::EventKind::Blocked) && fx.kicked.is_none_or(|(at, _)| now - at > 0.05) {
            fx.kicked = Some((now, strength));
        }
        if e.to == roster.me && matches!(kind, fu::EventKind::Hit | fu::EventKind::Guardbroken | fu::EventKind::Knockdown) {
            fx.hurt = Some((now, strength));
        }
        // KF6: the rest of what befalls this player is felt too: its blow parried, or bounced off a Toad's iron body (it
        // reels back), a grab catching it, and (lighter) a blow taken on its block.
        let me = roster.me;
        if me != 0 && ((kind == fu::EventKind::Parried && e.to == me) || (kind == fu::EventKind::Absorbed && e.by == me) || (kind == fu::EventKind::Grabbed && e.to == me)) {
            fx.hurt = Some((now, 1.));
        } else if me != 0 && kind == fu::EventKind::Blocked && e.to == me && fx.hurt.is_none_or(|(at, _)| now - at > 0.05) {
            fx.hurt = Some((now, 0.5));
        }
    }
    world.resource_mut::<Fx>().seen = seen.max(newest);
}
/// A ribbon of light through a limb's recent places (`trail`: its two edges and when), brightest where it moved
/// fastest and fading behind: positions, colours and indices.
pub fn ribbon(trail: &[(Vec3, Vec3, f32)], now: f32, tint: [f32; 3]) -> (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let n = trail.len();
    let mut positions = Vec::with_capacity(n * 2);
    let mut colors = Vec::with_capacity(n * 2);
    for (i, (a, b, born)) in trail.iter().enumerate() {
        let age = ((now - born) / LIFE).clamp(0., 1.);
        let speed = if i == 0 { 0. } else { (trail[i].0.distance(trail[i - 1].0) / (trail[i].2 - trail[i - 1].2).max(1e-3) / 1.4).clamp(0., 1.) };
        let alpha = (1. - age).powf(1.5) * (0.15 + 0.85 * speed) * 0.5;
        positions.push(a.to_array());
        positions.push(b.to_array());
        colors.push([tint[0], tint[1], tint[2], alpha]);
        colors.push([tint[0], tint[1], tint[2], 0.]);
    }
    let mut indices = vec![];
    for i in 0..n.saturating_sub(1) as u32 {
        let k = i * 2;
        indices.extend([k, k + 1, k + 2, k + 1, k + 3, k + 2, k, k + 2, k + 1, k + 1, k + 2, k + 3]);
    }
    (positions, colors, indices)
}
/// Writes a ribbon into `mesh`.
pub fn fill(meshes: &mut Assets<Mesh>, mesh: &Handle<Mesh>, (positions, colors, indices): (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>)) {
    if let Some(mut m) = meshes.get_mut(mesh) {
        let count = positions.len();
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0f32, 1., 0.]; count]);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0f32; 2]; count]);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        m.insert_indices(Indices::U32(indices));
    }
}
/// A ribbon's entity and mesh, made on first use (with `layer` for the first-person pass).
pub fn ribbon_entity(world: &mut World, parent: Option<Entity>, layer: Option<RenderLayers>) -> Option<(Entity, Handle<Mesh>)> {
    let streak = world.resource::<Fx>().look.as_ref()?.streak.clone();
    let mesh = world.resource_mut::<Assets<Mesh>>().add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
    let mut e = world.spawn((Name::new("Kung fu whoosh"), Mesh3d(mesh.clone()), MeshMaterial3d(streak), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster));
    if let Some(layer) = layer {
        e.insert(layer);
    }
    let entity = e.id();
    if let Some(parent) = parent {
        world.entity_mut(parent).add_child(entity);
    }
    Some((entity, mesh))
}
/// Draws every fighter's whooshes (in the world), dropping light that has had its time.
pub fn trails(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let keys: Vec<(u32, Limb)> = {
        let mut fx = world.resource_mut::<Fx>();
        for trail in fx.trails.values_mut() {
            trail.retain(|(_, _, born)| now - born < LIFE);
        }
        fx.trails.retain(|_, t| !t.is_empty());
        let mut keys: Vec<(u32, Limb)> = fx.trails.keys().copied().collect();
        keys.extend(fx.ribbons.keys().copied());
        keys.sort_by_key(|(id, limb)| (*id, format!("{limb:?}")));
        keys.dedup();
        keys
    };
    for key in keys {
        let trail = world.resource::<Fx>().trails.get(&key).cloned().unwrap_or_default();
        let existing = world.resource::<Fx>().ribbons.get(&key).cloned().filter(|(e, _)| world.get_entity(*e).is_ok());
        if trail.len() < 2 {
            if let Some((e, _)) = existing {
                world.entity_mut(e).insert(Visibility::Hidden);
            }
            continue;
        }
        let Some((entity, mesh)) = existing.or_else(|| ribbon_entity(world, None, None)) else {
            continue;
        };
        world.resource_mut::<Fx>().ribbons.insert(key, (entity, mesh.clone()));
        if world.get::<Visibility>(entity) != Some(&Visibility::Inherited) {
            world.entity_mut(entity).insert(Visibility::Inherited);
        }
        let shape = ribbon(&trail, now, [2., 1.8, 1.5]);
        fill(&mut world.resource_mut::<Assets<Mesh>>(), &mesh, shape);
    }
}

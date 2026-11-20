// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! What is seen of the titan besides its body: nothing here goes back into the simulation.
//!
//! * **Steam** pours off it all the time (it is hot): from the crown of its head, from between its teeth, out of the
//!   splits in its casing and from under its pauldrons, a little at full health and much more as it is hurt, in bursts
//!   when it roars, smashes, is hit on its weak spot, and when it falls.
//! * **Dust** puffs out at every footfall and rolls out in a ring along the floor.
//! * **The shockwave** of a smash (and the smaller one of a landing) runs out along the floor exactly as the
//!   simulation's does: its bright leading edge is the wave's front (`front`: the event's time and the move's `wave`
//!   speed, never past its `reach`), so where the edge has not reached, nothing has been hit. Under it the tiles crack:
//!   a decal of cracks that fades away after a while.
//! * **The charge's tell**: while it winds up, streaks along the floor mark the lane it will run down; while it runs,
//!   streaks trail behind it. A charge into a wall throws out debris and dust.
//! * **The weak spot** pulses (the pick's pennant glows) while it is exposed, and flashes white when it is hit, with a
//!   burst of steam out of the wound. The shot that kills the titan through its weak spot slows time (slowmo.rs).
//! * **The ground shakes** under every camera near a footfall, a smash, a landing, a fall (camera.rs).
//!
//! **Cost.** Everything is pooled: each kind of particle has its entities made the first time they are wanted and kept,
//! hidden, between uses, all sharing a handful of meshes and materials of kinds the game already draws; each graphics
//! level has its [`Budget`] of how many of each there are at once. A frame with nothing under way moves nothing.
use super::pose::{L, R};
use super::shape::{fbm, hash, smooth};
use super::{Titans, camera, sound};
use crate::quality::Level;
use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat};
use kitchen_sim::titan::{self as sim, EventKind, Move};
use std::f32::consts::TAU;

/// How much of each effect a graphics level draws at once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    /// Puffs of steam, puffs of dust.
    pub steam: usize,
    pub dust: usize,
    /// Rings along the floor (shockwaves, dust rings), crack decals, debris chunks, streaks.
    pub rings: usize,
    pub cracks: usize,
    pub debris: usize,
    pub streaks: usize,
    /// Puffs of steam a second at the most (hurt and venting).
    pub rate: f32,
}
impl Budget {
    pub fn of(level: Level) -> Budget {
        match level {
            Level::High => Budget { steam: 56, dust: 28, rings: 5, cracks: 4, debris: 18, streaks: 12, rate: 26. },
            Level::Medium => Budget { steam: 40, dust: 20, rings: 4, cracks: 3, debris: 12, streaks: 10, rate: 20. },
            Level::Low => Budget { steam: 20, dust: 10, rings: 3, cracks: 2, debris: 6, streaks: 6, rate: 11. },
            Level::Lowest => Budget { steam: 10, dust: 6, rings: 2, cracks: 1, debris: 4, streaks: 4, rate: 6. },
        }
    }
    /// The entities it makes at the most.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn entities(&self) -> usize {
        self.steam + self.dust + self.rings + self.cracks + self.debris + self.streaks + 1
    }
}

/// A shockwave's edge `since` seconds after it began, spreading at `speed` to `reach` (metres): where the simulation's
/// wave has got to (titan.rs `titan_waves`: a hotdog whose nearest point is within the front is hit, and from the first
/// instant anything within `DIRECT` of the impact), never past its reach.
pub fn front(since: f32, speed: f32, reach: f32) -> f32 {
    if speed <= 0. { reach } else { (since.max(0.) * speed).max(sim::DIRECT).min(reach) }
}
/// The ring card's texture has its leading edge at this share of the card's half-width.
pub const EDGE: f32 = 0.94;
/// Seconds a wave's ring holds at its reach and then has faded by.
const WAVE_HOLD: f32 = 0.25;
const WAVE_GONE: f32 = 0.6;
/// Seconds a crack decal stays before it fades, and fades over.
pub const CRACK_LIFE: f32 = 9.;
const CRACK_FADE: f32 = 3.;

/// What the effects are drawn with, made once.
#[derive(Clone)]
struct Kit {
    card: Handle<Mesh>,
    plate: Handle<Mesh>,
    chunk: Handle<Mesh>,
    /// Steam, thinning, step by step; dust likewise; the shockwave ring fading; the dust ring; cracks fading; the
    /// debris; a streak; the weak spot's flash.
    steam: [Handle<StandardMaterial>; 4],
    /// Steam near the giant, lit from below by its embers and its furnace.
    warm: [Handle<StandardMaterial>; 4],
    dust: [Handle<StandardMaterial>; 4],
    wave: [Handle<StandardMaterial>; 4],
    dust_ring: [Handle<StandardMaterial>; 4],
    crack: [Handle<StandardMaterial>; 4],
    debris: Handle<StandardMaterial>,
    streak: [Handle<StandardMaterial>; 3],
    flash: Handle<StandardMaterial>,
}
/// A puff of steam or dust.
#[derive(Clone, Copy, Debug)]
struct Puff {
    entity: Entity,
    on: bool,
    at: Vec3,
    velocity: Vec3,
    from: f32,
    to: f32,
    age: f32,
    life: f32,
    roll: f32,
    step: usize,
    /// Most opaque (0 to 1).
    alpha: f32,
}
/// A ring along the floor: a shockwave (its front the simulation's) or a dust ring.
#[derive(Clone, Copy, Debug)]
struct Ring {
    entity: Entity,
    on: bool,
    at: Vec3,
    age: f32,
    speed: f32,
    reach: f32,
    wave: bool,
    step: usize,
}
#[derive(Clone, Copy, Debug)]
struct Crack {
    entity: Entity,
    on: bool,
    age: f32,
    step: usize,
}
#[derive(Clone, Copy, Debug)]
struct Chunk {
    entity: Entity,
    on: bool,
    at: Vec3,
    velocity: Vec3,
    spin: Vec3,
    turn: Quat,
    size: f32,
    age: f32,
    floor: f32,
}
#[derive(Clone, Copy, Debug)]
struct Streak {
    entity: Entity,
    on: bool,
}

/// Every effect under way.
#[derive(Resource, Default)]
pub struct Fx {
    kit: Option<Kit>,
    puffs: Vec<Puff>,
    dusts: Vec<Puff>,
    rings: Vec<Ring>,
    cracks: Vec<Crack>,
    chunks: Vec<Chunk>,
    streaks: Vec<Streak>,
    flash: Option<(Entity, f32)>,
    /// Steam owed (puffs), carried from frame to frame.
    owed: f32,
    /// When the weak spot was last hit.
    weak_hit: Option<f32>,
    /// The mark's glow as last written.
    glow: f32,
    order: u32,
}
impl Fx {
    /// Entities the effects have made (for the budget's checks).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn entities(&self) -> usize {
        self.puffs.len() + self.dusts.len() + self.rings.len() + self.cracks.len() + self.chunks.len() + self.streaks.len() + self.flash.iter().count()
    }
    /// The drawn radius of the leading edge of each wave ring now, read back from its entity (for the checks).
    #[cfg(test)]
    pub fn drawn_edges(&self, world: &World) -> Vec<f32> {
        self.rings.iter().filter(|r| r.on && r.wave).filter_map(|r| world.get::<Transform>(r.entity).map(|t| t.scale.x * 0.5 * EDGE)).collect()
    }
    pub fn busy(&self) -> bool {
        self.puffs.iter().any(|p| p.on) || self.dusts.iter().any(|p| p.on) || self.rings.iter().any(|r| r.on) || self.cracks.iter().any(|c| c.on) || self.chunks.iter().any(|c| c.on) || self.flash.is_some_and(|(_, t)| t > 0.)
    }
}

fn level_of(world: &World) -> Level {
    world.get_resource::<crate::quality::Detail>().map_or(Level::High, |d| d.level)
}

// ----- Textures

fn image(size: usize, pixel: impl Fn(Vec2) -> [f32; 4]) -> Image {
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new((x as f32 + 0.5) / size as f32 * 2. - 1., (y as f32 + 0.5) / size as f32 * 2. - 1.);
            let c = pixel(p);
            out.extend(c.map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8));
        }
    }
    Image::new(Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 }, TextureDimension::D2, out, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}
/// A soft, wispy puff (white; its alpha the puff).
fn puff_image() -> Image {
    image(96, |p| {
        let r = p.length();
        let wisp = fbm(p.x * 3. + 5., p.y * 3., 0, 91);
        let a = (1. - smooth((r + 0.35 * (wisp - 0.5)) / 0.95)) * (0.55 + 0.45 * fbm(p.x * 6., p.y * 6., 0, 93));
        [1., 1., 1., a]
    })
}
/// The shockwave ring: a bright pale leading edge at `EDGE`, a dusty body behind it fading in towards the middle.
fn wave_image() -> Image {
    image(256, |p| {
        let u = p.length();
        let wobble = 0.01 * (fbm(p.x * 7., p.y * 7., 0, 41) - 0.5);
        let edge = (-((u + wobble - EDGE) / 0.03).powi(2)).exp();
        let body = smooth((u - 0.4) / 0.5) * (1. - smooth((u + wobble - EDGE + 0.005) / 0.02));
        let grit = 0.6 + 0.4 * fbm(p.x * 14., p.y * 14., 0, 43);
        let a = ((0.42 * body * grit + edge).min(1.)) * (1. - smooth((u - 0.985) / 0.015));
        let c = Vec3::new(1., 0.93, 0.8).lerp(Vec3::new(0.62, 0.55, 0.46), (1. - edge).min(1.));
        [c.x, c.y, c.z, a]
    })
}
/// A dust ring: soft, brownish, thin.
fn dust_ring_image() -> Image {
    image(128, |p| {
        let u = p.length();
        let a = (-((u - 0.8) / 0.13).powi(2)).exp() * (0.5 + 0.5 * fbm(p.x * 9., p.y * 9., 0, 47)) * (1. - smooth((u - 0.97) / 0.03));
        [0.72, 0.64, 0.54, a]
    })
}
/// Cracks in the floor from a heavy blow: dark branching lines out from a crushed middle, chipped edges.
fn crack_image() -> Image {
    let rays: Vec<(f32, f32, u32)> = (0..11).map(|k| (hash(k * 7 + 1) * TAU, 0.55 + 0.42 * hash(k * 7 + 2), k)).collect();
    image(256, move |p| {
        let r = p.length();
        let a = p.y.atan2(p.x);
        let mut dark = 0f32;
        for (angle, length, k) in &rays {
            // Each crack wanders, and thins towards its end; some fork.
            let wander = 0.22 * (fbm(r * 5. + *k as f32 * 3., *k as f32, 0, 51) - 0.5) / r.max(0.15);
            let off = ((a - angle - wander + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI).abs() * r;
            let width = 0.012 * (1. - r / length).max(0.) + 0.002;
            dark = dark.max((1. - smooth((off - width) / 0.006)) * (r < *length) as u8 as f32);
            if hash(*k * 13) > 0.5 {
                let fork = angle + 0.45 * (hash(*k * 17) - 0.5) * 2.;
                let off = ((a - fork - wander * 1.3 + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI).abs() * r;
                dark = dark.max((1. - smooth((off - width * 0.7) / 0.005)) * (r > 0.25 && r < length * 0.8) as u8 as f32);
            }
        }
        // The crushed middle: a darker, broken patch.
        let crush = (1. - smooth((r - 0.16 - 0.05 * fbm(p.x * 8., p.y * 8., 0, 53)) / 0.05)) * (0.45 + 0.5 * fbm(p.x * 20., p.y * 20., 0, 55));
        let a = dark.max(crush).min(1.) * (1. - smooth((r - 0.95) / 0.05));
        [0.05, 0.04, 0.035, a * 0.92]
    })
}
/// A streak: a thin line, bright in the middle, fading along and across.
fn streak_image() -> Image {
    image(64, |p| {
        let a = (1. - p.y.abs().powf(0.6)).max(0.) * (1. - smooth((p.x.abs() - 0.5) / 0.5));
        [1., 1., 1., a]
    })
}
/// A card in its xy plane facing +z, one metre square.
fn card_mesh() -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-0.5, -0.5, 0.], [0.5, -0.5, 0.], [0.5, 0.5, 0.], [-0.5, 0.5, 0.]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}
/// The same card lying flat, facing up.
fn plate_mesh() -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-0.5, 0., 0.5], [0.5, 0., 0.5], [0.5, 0., -0.5], [-0.5, 0., -0.5]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 1., 0.]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}
impl Kit {
    fn new(world: &mut World) -> Kit {
        let (card, plate, chunk) = {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            (meshes.add(card_mesh()), meshes.add(plate_mesh()), meshes.add(Cuboid::new(1., 0.6, 0.8)))
        };
        let (puff, wave, dust_ring, crack, streak) = {
            let mut images = world.resource_mut::<Assets<Image>>();
            (images.add(puff_image()), images.add(wave_image()), images.add(dust_ring_image()), images.add(crack_image()), images.add(streak_image()))
        };
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        let veil = |mats: &mut Assets<StandardMaterial>, c: Vec3, a: f32, tex: &Handle<Image>| mats.add(StandardMaterial { base_color: Color::srgba(c.x, c.y, c.z, a), base_color_texture: Some(tex.clone()), alpha_mode: AlphaMode::Blend, unlit: true, double_sided: true, cull_mode: None, ..default() });
        let fade = [1., 0.66, 0.38, 0.16];
        let steam = fade.map(|a| veil(&mut mats, Vec3::new(0.78, 0.8, 0.83), 0.42 * a, &puff));
        let warm = fade.map(|a| veil(&mut mats, Vec3::new(1.0, 0.6, 0.36), 0.45 * a, &puff));
        let dust = fade.map(|a| veil(&mut mats, Vec3::new(0.55, 0.48, 0.4), 0.55 * a, &puff));
        let wave = fade.map(|a| veil(&mut mats, Vec3::ONE, a, &wave));
        let dust_ring = fade.map(|a| veil(&mut mats, Vec3::ONE, 0.8 * a, &dust_ring));
        let crack = fade.map(|a| veil(&mut mats, Vec3::ONE, a, &crack));
        let streak = [0.5, 0.3, 0.14].map(|a| mats.add(StandardMaterial { base_color: Color::srgba(1., 0.93, 0.8, a), base_color_texture: Some(streak.clone()), alpha_mode: AlphaMode::Add, unlit: true, double_sided: true, cull_mode: None, ..default() }));
        let flash = mats.add(StandardMaterial { base_color: Color::srgba(1., 0.95, 0.85, 0.9), base_color_texture: Some(puff.clone()), alpha_mode: AlphaMode::Add, unlit: true, ..default() });
        let debris = mats.add(StandardMaterial { base_color: Color::srgb(0.62, 0.58, 0.52), perceptual_roughness: 0.9, ..default() });
        Kit { card, plate, chunk, steam, warm, dust, wave, dust_ring, crack, debris, streak, flash }
    }
}
fn kit(world: &mut World) -> Kit {
    if let Some(kit) = world.resource::<Fx>().kit.clone() {
        return kit;
    }
    let kit = Kit::new(world);
    world.resource_mut::<Fx>().kit = Some(kit.clone());
    kit
}
fn spawn(world: &mut World, name: &'static str, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>) -> Entity {
    world.spawn((Name::new(name), Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden, NotShadowCaster, crate::visuals::Surveyed)).id()
}
fn show(world: &mut World, e: Entity, on: bool) {
    let want = if on { Visibility::Visible } else { Visibility::Hidden };
    if world.get::<Visibility>(e).is_some_and(|v| *v != want) {
        world.entity_mut(e).insert(want);
    }
}
fn wear(world: &mut World, e: Entity, material: &Handle<StandardMaterial>) {
    if world.get::<MeshMaterial3d<StandardMaterial>>(e).is_some_and(|m| m.0 != *material) {
        world.entity_mut(e).insert(MeshMaterial3d(material.clone()));
    }
}

// ----- Setting things off

/// A free slot among `cap` of `pool` (made if need be), or the oldest-started.
fn slot<T>(pool: &mut Vec<T>, cap: usize, on: impl Fn(&T) -> bool, age: impl Fn(&T) -> f32, make: impl FnOnce() -> T) -> Option<usize> {
    if cap == 0 {
        return None;
    }
    if let Some(i) = pool.iter().take(cap).position(|p| !on(p)) {
        return Some(i);
    }
    if pool.len() < cap {
        pool.push(make());
        return Some(pool.len() - 1);
    }
    (0..pool.len().min(cap)).max_by(|a, b| age(&pool[*a]).total_cmp(&age(&pool[*b])))
}
/// A puff of steam (`dust` false) or dust at `at`, moving at `velocity`, growing from `from` to `to` metres across over
/// `life` seconds.
#[allow(clippy::too_many_arguments)]
fn puff(world: &mut World, dust: bool, at: Vec3, velocity: Vec3, from: f32, to: f32, life: f32, alpha: f32) {
    let k = kit(world);
    let cap = if dust { Budget::of(level_of(world)).dust } else { Budget::of(level_of(world)).steam };
    let mut fx = world.remove_resource::<Fx>().unwrap();
    fx.order = fx.order.wrapping_add(1);
    let seed = fx.order;
    let pool = if dust { &mut fx.dusts } else { &mut fx.puffs };
    let mut made = None;
    let i = slot(pool, cap, |p| p.on, |p| p.age / p.life.max(0.01), || {
        let e = spawn(world, if dust { "Titan's dust" } else { "Titan's steam" }, &k.card, if dust { &k.dust[0] } else { &k.steam[0] });
        made = Some(e);
        Puff { entity: e, on: false, at, velocity, from, to, age: 0., life, roll: 0., step: 0, alpha }
    });
    if let Some(i) = i {
        let e = pool[i].entity;
        pool[i] = Puff { entity: e, on: true, at, velocity, from, to, age: 0., life, roll: hash(seed) * TAU, step: 9, alpha };
    }
    world.insert_resource(fx);
}
/// A ring along the floor at `at`: a shockwave spreading at `speed` to `reach` (its front the simulation's), or a
/// dust ring.
pub fn ring(world: &mut World, at: Vec3, speed: f32, reach: f32, wave: bool) {
    let k = kit(world);
    let cap = Budget::of(level_of(world)).rings;
    let mut fx = world.remove_resource::<Fx>().unwrap();
    let i = slot(&mut fx.rings, cap, |r| r.on, |r| r.age, || Ring { entity: spawn(world, "Titan's shockwave", &k.plate, &k.wave[0]), on: false, at, age: 0., speed, reach, wave, step: 9 });
    if let Some(i) = i {
        let e = fx.rings[i].entity;
        fx.rings[i] = Ring { entity: e, on: true, at, age: 0., speed, reach, wave, step: 9 };
    }
    world.insert_resource(fx);
}
/// Cracks in the floor at `at`, `radius` across.
fn crack(world: &mut World, at: Vec3, radius: f32, seed: u32) {
    let k = kit(world);
    let cap = Budget::of(level_of(world)).cracks;
    let mut fx = world.remove_resource::<Fx>().unwrap();
    let i = slot(&mut fx.cracks, cap, |c| c.on, |c| c.age, || Crack { entity: spawn(world, "Cracked tiles", &k.plate, &k.crack[0]), on: false, age: 0., step: 9 });
    if let Some(i) = i {
        let e = fx.cracks[i].entity;
        fx.cracks[i] = Crack { entity: e, on: true, age: 0., step: 9 };
        world.insert_resource(fx);
        // (Just over the floor, so it never fights with it; turned at random.)
        if let Some(mut tr) = world.get_mut::<Transform>(e) {
            *tr = Transform::from_translation(at + Vec3::Y * 0.0025).with_rotation(Quat::from_rotation_y(hash(seed) * TAU)).with_scale(Vec3::new(radius * 2., 1., radius * 2.));
        }
        wear(world, e, &k.crack[0]);
        show(world, e, true);
        return;
    }
    world.insert_resource(fx);
}
/// Debris flung from `at` along `way` (a wall's chips, tiles' chips).
fn debris(world: &mut World, at: Vec3, way: Vec3, count: usize, speed: f32, floor: f32, seed: u32) {
    let k = kit(world);
    let cap = Budget::of(level_of(world)).debris;
    for n in 0..count.min(cap) as u32 {
        let r = |j: u32| hash(seed.wrapping_mul(977).wrapping_add(n * 13 + j));
        let mut fx = world.remove_resource::<Fx>().unwrap();
        let i = slot(&mut fx.chunks, cap, |c| c.on, |c| c.age, || Chunk { entity: spawn(world, "Debris", &k.chunk, &k.debris), on: false, at, velocity: Vec3::ZERO, spin: Vec3::ZERO, turn: Quat::IDENTITY, size: 0., age: 0., floor });
        if let Some(i) = i {
            let e = fx.chunks[i].entity;
            let spread = Vec3::new(r(1) - 0.5, r(2) * 0.8 + 0.3, r(3) - 0.5);
            let velocity = (way.normalize_or(Vec3::Y) + spread).normalize_or(Vec3::Y) * speed * (0.5 + 0.7 * r(4));
            fx.chunks[i] = Chunk { entity: e, on: true, at: at + spread * 0.05, velocity, spin: Vec3::new(r(5), r(6), r(7)) * 18. - 9., turn: Quat::from_rotation_y(r(8) * TAU), size: 0.012 + 0.03 * r(9), age: 0., floor };
            world.insert_resource(fx);
            show(world, e, true);
        } else {
            world.insert_resource(fx);
        }
    }
}

/// The titan's events this frame (`fresh`: those not seen before): the shockwaves and the cracks, the dust, the
/// debris, the weak spot's flash, the camera's shakes, the sounds, the slow motion on its death.
pub fn events(world: &mut World, view: &sim::View, fresh: &[sim::Event], me: u32, now: f32, elapsed: f32) {
    let floor = |p: Vec3| p.y;
    for e in fresh {
        let at = Vec3::from_array(e.p);
        let Some(kind) = e.kind else { continue };
        match kind {
            EventKind::Smash | EventKind::JumpLand => {
                let mv = if kind == EventKind::Smash { Move::Smash } else { Move::Jump };
                let d = mv.data();
                // The wave began when the event did: the ring is put where its front is now.
                let reach = if e.amount > 0. { e.amount } else { d.reach };
                ring(world, Vec3::new(at.x, floor(at), at.z), d.wave, reach, true);
                if let Some(r) = world.resource_mut::<Fx>().rings.iter_mut().rev().find(|r| r.on && r.wave && r.age == 0.) {
                    r.age = (elapsed - e.time).max(0.);
                }
                crack(world, at, if kind == EventKind::Smash { 0.75 } else { 0.5 }, e.id);
                for k in 0..8u32 {
                    let a = k as f32 / 8. * TAU + hash(e.id * 3 + k);
                    let way = Vec3::new(a.cos(), 0., a.sin());
                    puff(world, true, at + way * 0.25 + Vec3::Y * 0.05, way * 0.9 + Vec3::Y * 0.12, 0.12, 0.5, 1.4, 0.9);
                }
                debris(world, at + Vec3::Y * 0.02, Vec3::Y, 6, 1.6, at.y, e.id);
                camera::quake(world, at, if kind == EventKind::Smash { 3. } else { 2.2 }, 1.4);
                sound::play(world, if kind == EventKind::Smash { sound::Cue::Smash } else { sound::Cue::Land }, at, 1.);
            }
            EventKind::ChargeWall => {
                let titan = titan_place(world).unwrap_or(at);
                let way = Vec3::new(titan.x - at.x, 0.3, titan.z - at.z);
                debris(world, at + Vec3::Y * 0.5, way, 12, 1.4, titan.y, e.id);
                for k in 0..6u32 {
                    puff(world, true, at + Vec3::new(hash(e.id + k) - 0.5, 0.2 + 0.8 * hash(e.id * 5 + k), hash(e.id * 7 + k) - 0.5) * 0.4, way.normalize_or(Vec3::Y) * 0.5, 0.15, 0.6, 1.6, 0.8);
                }
                camera::quake(world, at, 1.8, 1.);
                sound::play(world, sound::Cue::Smash, at, 0.6);
            }
            EventKind::Stomp => {
                for k in 0..5u32 {
                    let a = k as f32 / 5. * TAU;
                    puff(world, true, at + Vec3::new(a.cos(), 0.05, a.sin()) * 0.15, Vec3::new(a.cos(), 0.1, a.sin()) * 0.6, 0.08, 0.35, 1.0, 0.8);
                }
                ring(world, at, 1.2, 0.45, false);
                camera::quake(world, at, 1.4, 0.8);
                sound::play(world, sound::Cue::Stomp, at, 1.);
            }
            EventKind::Swat => sound::play(world, sound::Cue::Swat, at, 1.),
            EventKind::Tell if Move::from_u8(e.mv) == Some(Move::Charge) => sound::play(world, sound::Cue::Roar, at, 1.),
            EventKind::Hit | EventKind::ChargeHit | EventKind::Stepped => {
                for k in 0..3u32 {
                    puff(world, true, at + Vec3::new(hash(e.id + k) - 0.5, 0.05, hash(e.id * 3 + k) - 0.5) * 0.1, Vec3::Y * 0.3, 0.05, 0.22, 0.8, 0.7);
                }
                sound::play(world, sound::Cue::Thump, at, 0.7);
            }
            EventKind::StaggerStart => sound::play(world, sound::Cue::Groan, at, 1.),
            EventKind::WeakHit => {
                world.resource_mut::<Fx>().weak_hit = Some(now);
                if let Some(spot) = spot_world(world) {
                    for k in 0..6u32 {
                        let way = (spot.1 + Vec3::new(hash(e.id + k) - 0.5, hash(e.id * 3 + k) * 0.5, hash(e.id * 5 + k) - 0.5) * 0.8).normalize_or(Vec3::Y);
                        puff(world, false, spot.0, way * 0.6, 0.06, 0.3, 1.0, 1.);
                    }
                    flash(world, spot.0);
                }
                sound::play(world, sound::Cue::Hurt, at, 1.);
            }
            EventKind::TitanDown => {
                // The shot that kills it through its weak spot slows time (as a kill does: slowmo.rs).
                let weak = view.events.iter().any(|w| w.kind == Some(EventKind::WeakHit) && (w.time - e.time).abs() < 0.05);
                let on = world.get_resource::<crate::settings::Settings>().is_none_or(|s| s.kill_slowmo);
                if weak && on {
                    let online = world.get_resource::<crate::arena::Game>().is_some_and(|g| g.network);
                    if let Some(mut s) = world.get_resource_mut::<crate::slowmo::SlowMo>() {
                        s.kill(now, online || e.by != me, Some(at));
                    }
                }
                sound::play(world, sound::Cue::Groan, at, 1.2);
            }
            _ => {}
        }
    }
}
/// The weak spot's flash: a bright card at it, fading quickly.
fn flash(world: &mut World, at: Vec3) {
    let k = kit(world);
    let e = match world.resource::<Fx>().flash {
        Some((e, _)) => e,
        None => spawn(world, "Titan's weak spot flash", &k.card, &k.flash),
    };
    world.resource_mut::<Fx>().flash = Some((e, 0.18));
    if let Some(mut tr) = world.get_mut::<Transform>(e) {
        tr.translation = at;
    }
    show(world, e, true);
}
/// Where the titan's root is.
fn titan_place(world: &World) -> Option<Vec3> {
    world.resource::<Titans>().drawn.map(|(_, _, root)| root.translation)
}
/// The weak spot's middle and its way out, in the world.
fn spot_world(world: &World) -> Option<(Vec3, Vec3)> {
    let (_, frame, root) = world.resource::<Titans>().drawn?;
    let (at, out) = frame.skeleton.spot();
    Some((root.transform_point(at), (root.rotation * out).normalize()))
}

// ----- Every frame

/// Steam from the titan's vents, the footfalls' dust and shakes, the charge's streaks, the weak spot's glow, and every
/// effect under way moved on.
pub fn update(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let pace = world.get_resource::<crate::slowmo::SlowMo>().map_or(1., |s| s.scale(now) * s.replay.unwrap_or(1.));
    let dt = world.resource::<Time<Real>>().delta_secs().min(0.1) * pace;
    let drawn = world.resource::<Titans>().drawn;
    let budget = Budget::of(level_of(world));
    if let Some((id, frame, root)) = drawn {
        let view = world.get_resource::<crate::arena::Game>().and_then(|g| g.titan.clone());
        let hurt = view.as_ref().map_or(0., |v| if v.max > 0. { 1. - (v.hp / v.max).clamp(0., 1.) } else { 0. });
        let s = &frame.skeleton;
        let w = |p: Vec3| root.transform_point(p);
        let own = camera::first_person(world) && world.get_resource::<crate::arena::Game>().is_some_and(|g| g.me() == id);
        // Steam: a breath of it always, more as it is hurt, and in bursts as it vents.
        let rate = (1.6 + 9. * hurt * hurt + 14. * s.vent).min(budget.rate);
        let mut fx = world.remove_resource::<Fx>().unwrap();
        fx.owed += rate * dt;
        let mut emits = vec![];
        while fx.owed >= 1. {
            fx.owed -= 1.;
            fx.order = fx.order.wrapping_add(1);
            emits.push(fx.order);
        }
        world.insert_resource(fx);
        for n in emits {
            let r = |j: u32| hash(n.wrapping_mul(31).wrapping_add(j));
            // Where from: the crown, the grin, a split, under a pauldron (seen from inside its own head, not the crown or
            // the grin: they would be in front of its own eyes).
            let vent = (r(1) * 5.) as u32;
            let vent = if own && vent < 2 { 2 + vent } else { vent };
            let (at, out) = match vent {
                0 => (s.crown(), Vec3::Y),
                1 => (s.on_head(Vec3::new((r(2) - 0.5) * 0.3, super::body::GRIN.0, -0.24)), s.head.rotation * Vec3::new(0., 0.2, -1.)),
                2 => (s.axis(0.95) + s.torso.rotation * Vec3::new(-0.15, 0., -0.27), s.torso.rotation * Vec3::new(-0.4, 0.3, -1.)),
                3 => (s.axis(1.05) + s.torso.rotation * Vec3::new(0.18, 0.1, 0.22), s.torso.rotation * Vec3::new(0.5, 0.4, 1.)),
                _ => {
                    let side = if r(3) < 0.5 { L } else { R };
                    (s.shoulder[side] + Vec3::Y * 0.12, Vec3::new(if side == L { -0.5 } else { 0.5 }, 1., 0.))
                }
            };
            let at = w(at);
            let out = (root.rotation * out).normalize_or(Vec3::Y);
            let up = 0.18 + 0.2 * r(4);
            let velocity = out * (0.08 + 0.12 * r(5) + 0.25 * s.vent) + Vec3::Y * up;
            let k = root.scale.x;
            puff(world, false, at, velocity, 0.07 * k, (0.28 + 0.2 * r(6)) * k, 1.5 + 1.3 * r(7), 0.75 + 0.25 * hurt);
        }
        // Footfalls: dust and a ring at the foot, the ground shaking, a boom.
        for f in frame.footfalls.iter().flatten() {
            let at = w(f.at);
            for k in 0..(2 + (f.strength * 2.) as u32) {
                let a = hash(id * 7 + k + (now * 100.) as u32) * TAU;
                puff(world, true, at + Vec3::new(a.cos(), 0.02, a.sin()) * 0.12, Vec3::new(a.cos(), 0.08, a.sin()) * 0.35 * f.strength, 0.06, 0.26, 0.9, 0.6);
            }
            ring(world, at, 0.9 * f.strength.max(0.4), 0.3 * f.strength.max(0.5), false);
            camera::quake(world, at, f.strength, 0.8);
            sound::play(world, sound::Cue::Footfall, at, f.strength);
        }
        // The fall to its knees and its death shake everything.
        if frame.quake > 0. {
            let chest = w(s.axis(0.9));
            let ground = Vec3::new(chest.x, root.translation.y, chest.z);
            camera::quake(world, ground, frame.quake * 2.2, 1. + frame.quake);
            if frame.quake > 1. {
                for k in 0..10u32 {
                    let a = k as f32 / 10. * TAU;
                    puff(world, true, ground + Vec3::new(a.cos(), 0.05, a.sin()) * 0.6, Vec3::new(a.cos(), 0.12, a.sin()) * 0.8, 0.15, 0.6, 2., 0.9);
                }
                ring(world, ground, 2., 1.4, false);
                sound::play(world, sound::Cue::Topple, ground, 1.);
            }
        }
        // The charge: its lane marked on the floor while it winds up; streaks behind it while it runs.
        let mv = view.as_ref().map(|v| v.mv()).unwrap_or_default();
        let charging = if mv == Move::Charge { Some(frame.beat) } else { None };
        streaks(world, charging, &root, s, budget.streaks, now);
        // The weak spot glows while it is exposed, and flashes when hit; the embers, the furnace and the eyes glow.
        let exposed = view.as_ref().is_some_and(|v| v.exposed);
        glow(world, exposed, now);
        let since = world.resource::<Fx>().weak_hit.map(|t| now - t);
        let heat = heat_of(mv, frame.beat, hurt, s.vent, s.heave, exposed, since, now);
        let titans = world.resource::<Titans>();
        if let (Some(kit), Some(root)) = (titans.kit.clone(), titans.roots.get(&id).copied()) {
            super::rig::heat(world, root, &kit, heat);
        }
    } else {
        streaks(world, None, &Transform::IDENTITY, &super::pose::solve(&super::pose::Pose::REST, None, 0.), budget.streaks, now);
    }
    advance(world, dt);
}
/// The charge's streaks: `beat` the charge's (None: hidden).
fn streaks(world: &mut World, beat: Option<f32>, root: &Transform, s: &super::pose::Skeleton, cap: usize, now: f32) {
    if beat.is_none() && world.resource::<Fx>().streaks.iter().all(|s| !s.on) {
        return;
    }
    let k = kit(world);
    while world.resource::<Fx>().streaks.len() < cap {
        let e = spawn(world, "Titan's charge streak", &k.plate, &k.streak[0]);
        world.resource_mut::<Fx>().streaks.push(Streak { entity: e, on: false });
    }
    let all: Vec<Entity> = world.resource::<Fx>().streaks.iter().map(|s| s.entity).collect();
    let n = all.len().max(1);
    for (i, e) in all.into_iter().enumerate() {
        let r = |j: u32| hash(i as u32 * 17 + j);
        let place = match beat {
            // The tell: lines along the floor down the lane ahead, rushing outwards.
            Some(b) if b < 1. => {
                let side = (i as f32 / (n - 1).max(1) as f32 - 0.5) * 2. * sim::RADIUS * 0.9;
                let run = (now * 2.2 + r(1)).fract();
                let along = 0.5 + 2.6 * run;
                let length = 0.45 + 0.4 * r(2);
                let at = root.transform_point(Vec3::new(side / root.scale.x, 0.004, -along / root.scale.x));
                let fade = smooth(b / 0.25) * (1. - run);
                Some((Transform::from_translation(Vec3::new(at.x, root.translation.y + 0.004, at.z)).with_rotation(root.rotation).with_scale(Vec3::new(0.03, 1., length)), fade))
            }
            // Running: streaks trailing off its body and arms.
            Some(b) if b < 2. => {
                let from = s.axis(0.6 + 0.9 * r(3)) + Vec3::new((r(4) - 0.5) * 0.9, 0., 0.);
                let at = root.transform_point(from + Vec3::new(0., 0., 0.35 + 0.5 * (now * 3. + r(5)).fract()));
                let turn = root.rotation * Quat::from_rotation_z(r(6) * TAU);
                Some((Transform::from_translation(at).with_rotation(turn).with_scale(Vec3::new(0.012, 1., 0.5 + 0.4 * r(7))), 0.7))
            }
            _ => None,
        };
        match place {
            Some((tr, fade)) => {
                if let Some(mut t) = world.get_mut::<Transform>(e) {
                    *t = tr;
                }
                let step = if fade > 0.66 { 0 } else if fade > 0.33 { 1 } else { 2 };
                wear(world, e, &k.streak[step]);
                show(world, e, fade > 0.05);
            }
            None => show(world, e, false),
        }
    }
    for s in world.resource_mut::<Fx>().streaks.iter_mut() {
        s.on = beat.is_some_and(|b| b < 2.);
    }
}
/// The weak spot's pick and pennant: a faint cold glow always (so they can be found in the dark), pulsing brighter
/// while the spot is exposed, a white flash when it is hit.
fn glow(world: &mut World, exposed: bool, now: f32) {
    let hit = world.resource::<Fx>().weak_hit.map_or(0., |t| (1. - (now - t) / 0.4).max(0.));
    let pulse = if exposed { 0.5 + 0.5 * (now * TAU * 1.4).sin() } else { 0. };
    let level = (0.35 + pulse * 1.3 + hit * 6.).min(8.);
    if (level - world.resource::<Fx>().glow).abs() < 0.02 {
        return;
    }
    world.resource_mut::<Fx>().glow = level;
    let Some(mark) = world.resource::<Titans>().kit.as_ref().map(|k| k.mark.clone()) else {
        return;
    };
    if let Some(mut m) = world.resource_mut::<Assets<StandardMaterial>>().get_mut(&mark) {
        m.emissive = LinearRgba::rgb(0.6 * level + hit * 2., 0.75 * level + hit * 2., 1.0 * level + hit * 2.);
    }
}
/// How hot the giant glows this frame (rig.rs `Heat`): its embers smoulder and breathe, hotter as it is hurt and as it
/// vents, flaring through every wind-up (a tell to read in the dark) and at a hit on its weak spot, dying to a dull red
/// as it falls; its furnace and its eyes with them; its weak spot's patch cold and pale, pulsing when exposed.
pub fn heat_of(mv: Move, beat: f32, hurt: f32, vent: f32, heave: f32, exposed: bool, since_weak_hit: Option<f32>, now: f32) -> super::rig::Heat {
    let breath = 0.85 + 0.15 * (now * 1.8).sin();
    let windup = if mv.data().attacks() {
        if beat < 1. { 0.9 * smooth(beat / 0.85) } else { 0.9 * (1. - smooth((beat - 1.) / 0.5)) }
    } else {
        0.
    };
    let flare = since_weak_hit.map_or(0., |t| 2.5 * (-t.max(0.) / 0.3).exp());
    let dull = match mv {
        Move::Down => smooth((beat - 0.8) / 2.),
        _ => 0.,
    };
    let fade = 1. - 0.7 * dull;
    let pulse = if exposed { 0.5 + 0.5 * (now * TAU * 1.4).sin() } else { 0. };
    super::rig::Heat {
        embers: (breath + 0.6 * hurt + 0.5 * vent + windup + flare) * fade,
        dull,
        maw: (0.75 + 0.3 * heave + 0.6 * vent + 0.6 * windup + 0.5 * flare) * (1. - 0.8 * dull),
        eyes: (1. + 0.6 * windup + 0.5 * flare) * (1. - 0.8 * dull),
        spot: (0.65 + if exposed { 0.35 + 0.6 * pulse } else { 0. }) * (1. + 2. * flare.min(1.)),
    }
}
/// Moves every effect under way on by `dt`, faces the cards to the camera, and puts away what has finished.
fn advance(world: &mut World, dt: f32) {
    if !world.resource::<Fx>().busy() {
        return;
    }
    let Some(k) = world.resource::<Fx>().kit.clone() else {
        return;
    };
    let facing = world.get_resource::<crate::arena::Game>().and_then(|g| g.camera()).and_then(|c| world.get::<Transform>(c)).map_or(Quat::IDENTITY, |t| t.rotation);
    let mut fx = world.remove_resource::<Fx>().unwrap();
    for dust in [false, true] {
        let pool = if dust { &mut fx.dusts } else { &mut fx.puffs };
        for p in pool.iter_mut().filter(|p| p.on) {
            p.age += dt;
            if p.age >= p.life {
                p.on = false;
                show(world, p.entity, false);
                continue;
            }
            let t = p.age / p.life;
            // Steam rises and slows; dust rolls out and settles.
            p.velocity *= (1. - dt * if dust { 2.6 } else { 0.9 }).max(0.);
            if dust {
                p.velocity.y -= 0.05 * dt;
            }
            p.at += p.velocity * dt;
            let size = p.from + (p.to - p.from) * (1. - (1. - t).powi(2));
            if let Some(mut tr) = world.get_mut::<Transform>(p.entity) {
                *tr = Transform::from_translation(p.at).with_rotation(facing * Quat::from_rotation_z(p.roll + t * 0.6)).with_scale(Vec3::splat(size));
            }
            // In quickly, out slowly; as fading steps of the shared materials.
            let a = smooth(t / 0.12) * (1. - smooth((t - 0.3) / 0.7)) * p.alpha;
            let step = if a > 0.75 { 0 } else if a > 0.45 { 1 } else if a > 0.2 { 2 } else { 3 };
            // (Steam glows warm from below while it is still close over the giant, then goes grey as it rises.)
            let warm = !dust && t < 0.4;
            let shown = step + if warm { 4 } else { 0 };
            if shown != p.step {
                p.step = shown;
                wear(world, p.entity, if dust { &k.dust[step] } else if warm { &k.warm[step] } else { &k.steam[step] });
            }
            show(world, p.entity, a > 0.04);
        }
    }
    for r in fx.rings.iter_mut().filter(|r| r.on) {
        r.age += dt;
        let reached = if r.speed > 0. { r.reach / r.speed } else { 0. };
        let (radius, fade) = if r.wave {
            let since = (r.age - reached).max(0.);
            (front(r.age, r.speed, r.reach), 1. - smooth((since - WAVE_HOLD) / (WAVE_GONE - WAVE_HOLD)))
        } else {
            let t = r.age / (reached + 0.5);
            (r.reach * (1. - (1. - t.min(1.)).powi(2)), 1. - smooth(t))
        };
        if fade <= 0.01 {
            r.on = false;
            show(world, r.entity, false);
            continue;
        }
        if let Some(mut tr) = world.get_mut::<Transform>(r.entity) {
            // The card is as wide as its leading edge's circle over EDGE: the edge is the front.
            *tr = Transform::from_translation(r.at + Vec3::Y * 0.004).with_scale(Vec3::new(2. * radius / EDGE, 1., 2. * radius / EDGE));
        }
        let step = if fade > 0.75 { 0 } else if fade > 0.45 { 1 } else if fade > 0.2 { 2 } else { 3 };
        if step != r.step {
            r.step = step;
            wear(world, r.entity, if r.wave { &k.wave[step] } else { &k.dust_ring[step] });
        }
        show(world, r.entity, radius > 0.005);
    }
    for c in fx.cracks.iter_mut().filter(|c| c.on) {
        c.age += dt;
        let fade = 1. - smooth((c.age - CRACK_LIFE) / CRACK_FADE);
        if fade <= 0.01 {
            c.on = false;
            show(world, c.entity, false);
            continue;
        }
        let step = if fade > 0.75 { 0 } else if fade > 0.45 { 1 } else if fade > 0.2 { 2 } else { 3 };
        if step != c.step {
            c.step = step;
            wear(world, c.entity, &k.crack[step]);
        }
    }
    for c in fx.chunks.iter_mut().filter(|c| c.on) {
        c.age += dt;
        if c.age > 2.5 {
            c.on = false;
            show(world, c.entity, false);
            continue;
        }
        if c.at.y > c.floor + c.size * 0.3 || c.velocity.y > 0. {
            c.velocity.y -= crate::death::GRAVITY * 1.4 * dt;
            c.at += c.velocity * dt;
            c.turn = Quat::from_scaled_axis(c.spin * dt) * c.turn;
            if c.at.y < c.floor + c.size * 0.3 {
                c.at.y = c.floor + c.size * 0.3;
                c.velocity = Vec3::new(c.velocity.x * 0.4, -c.velocity.y * 0.25, c.velocity.z * 0.4);
                c.spin *= 0.4;
                if c.velocity.y < 0.1 {
                    c.velocity = Vec3::ZERO;
                }
            }
        }
        let shrink = 1. - smooth((c.age - 1.8) / 0.7);
        if let Some(mut tr) = world.get_mut::<Transform>(c.entity) {
            *tr = Transform::from_translation(c.at).with_rotation(c.turn).with_scale(Vec3::splat(c.size * shrink));
        }
    }
    if let Some((e, left)) = fx.flash.as_mut() {
        *left -= dt;
        let a = (*left / 0.18).max(0.);
        if let Some(mut tr) = world.get_mut::<Transform>(*e) {
            tr.rotation = facing;
            tr.scale = Vec3::splat(0.12 + 0.3 * (1. - a));
        }
        show(world, *e, *left > 0.);
    }
    world.insert_resource(fx);
}

/// A new round, a new map: every effect put away.
#[allow(dead_code)]
pub fn clear(world: &mut World) {
    let Some(mut fx) = world.remove_resource::<Fx>() else {
        return;
    };
    let entities: Vec<Entity> = fx.puffs.iter().chain(fx.dusts.iter()).filter(|p| p.on).map(|p| p.entity).chain(fx.rings.iter().filter(|r| r.on).map(|r| r.entity)).chain(fx.cracks.iter().filter(|c| c.on).map(|c| c.entity)).chain(fx.chunks.iter().filter(|c| c.on).map(|c| c.entity)).collect();
    for e in entities {
        show(world, e, false);
    }
    fx.puffs.iter_mut().for_each(|p| p.on = false);
    fx.dusts.iter_mut().for_each(|p| p.on = false);
    fx.rings.iter_mut().for_each(|r| r.on = false);
    fx.cracks.iter_mut().for_each(|c| c.on = false);
    fx.chunks.iter_mut().for_each(|c| c.on = false);
    world.insert_resource(fx);
}

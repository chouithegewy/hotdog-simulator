// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The liquid of an emergence (`kitchen_sim::emergence`): the microwave dings, its door swings open,
//! ooze brims over the sill and runs down the front, flows across the counter along the simulation's
//! path to each zombie spawn point, pools there, and drains away once the zombies have formed. On a
//! map without a microwave it wells up out of the counter at the spawn points. Everything here is
//! drawn from `Feast::emergences`, which the simulation owns locally and the server sends online.
//!
//! The liquid inside and down the front of a microwave is a few dozen blobs. Everything on the counter
//! (the flows, the puddles and their bubbles) is one mesh for each emergence, reshaped every frame: a
//! map may have 64 spawn points, and a blob for every bead of 64 flows was eight thousand meshes.
use crate::arena::{Game, MicrowaveDigit, MicrowaveGlow};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use kitchen_sim::emergence::{Emergence, DOOR, DRAIN, FLOW, FORM, POOL, SPILL};
use kitchen_sim::map::{Map, Side};
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const FLOOR: f32 = 0.9025;
/// The ooze: a glossy, faintly glowing green. Zombie cats are this colour until they have formed.
pub const COLOR: Vec3 = Vec3::new(0.40, 0.84, 0.07);
pub const GLOW: Vec3 = Vec3::new(0.018, 0.10, 0.002);
// The microwave's door as level.rs builds it, in fractions of the appliance's width and height:
// centre and size across, centre and size up. The cavity floor (the sill) is SILL of the height.
const DOOR_X: f32 = -0.0625;
const DOOR_W: f32 = 0.729;
pub const SILL: f32 = 0.16;
/// How far the door swings, in radians.
const OPEN: f32 = 1.9;
/// Radius of a full puddle, metres.
const PUDDLE: f32 = 0.062;
/// A flow over the counter is a ribbon: a cross-section of ACROSS points every SPACING metres, or
/// further apart when all of an emergence's flows together would need more than SECTIONS of them.
const ACROSS: usize = 5;
const SPACING: f32 = 0.016;
const SECTIONS: usize = 5600;
/// The round shapes on the counter, as (points round, rings below the top): a puddle's wide dome, the
/// lobes round its edge, and a bubble.
const DOMES: [(usize, usize); 3] = [(24, 3), (14, 3), (8, 2)];

/// The hinge of a microwave door; the number is the microwave's block index in the map.
#[derive(Component)]
pub struct Hinge(pub usize);

#[derive(Resource, Clone)]
pub struct Kit {
    pub sphere: Handle<Mesh>,
    /// A coarser sphere for drops, which are a few millimetres across and many.
    pub drop: Handle<Mesh>,
    pub ooze: Handle<StandardMaterial>,
    ding: Option<Handle<AudioSource>>,
}
impl FromWorld for Kit {
    fn from_world(world: &mut World) -> Self {
        let sphere = world.resource_mut::<Assets<Mesh>>().add(Sphere::new(1.).mesh().uv(32, 18));
        let drop = world.resource_mut::<Assets<Mesh>>().add(Sphere::new(1.).mesh().uv(8, 5));
        let ooze = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color: Color::srgb(COLOR.x, COLOR.y, COLOR.z),
            emissive: LinearRgba::rgb(GLOW.x, GLOW.y, GLOW.z),
            perceptual_roughness: 0.2,
            reflectance: 0.9,
            ..default()
        });
        let ding = world.get_resource::<AssetServer>().map(|server| server.load("audio/ding.wav"));
        Kit { sphere, drop, ooze, ding }
    }
}

/// One emergence's liquid: a root at the origin, the blobs of what is in and on the microwaves, and
/// the one mesh of what is on the counter.
#[derive(Component)]
struct Spill {
    id: u32,
    started: f32,
    parts: Vec<(Entity, Part)>,
    counter: Counter,
}
enum Part {
    /// A heaving lump of the mass that fills the cavity behind the door: where it rests on the cavity
    /// floor and its radii, in the microwave's frame.
    Fill { at: Vec3, size: Vec3, seed: f32 },
    /// A strand hanging from the cavity's roof: its top, thickness and length.
    Strand { at: Vec3, r: f32, length: f32, seed: f32 },
    /// The bulge along the sill.
    Lip { at: Vec3, size: Vec3 },
    /// The curtain down the front below the door; `top` is the sill's height.
    Sheet { at: Vec3, wide: f32, top: f32 },
    /// One thick run down the front, and the bead at its lower end.
    Run { at: Vec3, r: f32, start: f32, seed: f32, bead: bool },
    /// The pool at the foot of the microwave where the flows begin.
    Foot { at: Vec3, size: Vec3 },
}
/// The liquid on the counter: every flow, puddle and bubble of an emergence in one mesh. What each
/// vertex belongs to is fixed when the emergence starts; `shape` moves them every frame.
struct Counter {
    mesh: Handle<Mesh>,
    flows: Vec<Flow>,
    domes: Vec<Dome>,
    /// The points of each kind of dome in DOMES on a unit hemisphere: ring by ring from the rim up,
    /// then the top.
    units: [Vec<Vec3>; 3],
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
}
/// A flow over the counter to one puddle: a ribbon of cross-sections along the simulation's path.
struct Flow {
    puddle: usize,
    first: usize,
    sections: Vec<Section>,
}
/// One cross-section of a flow: its distance along the path, where its middle lies, the unit vector
/// across the flow, how wide the flow runs here (0 to 1), and where it is in its ripple.
struct Section {
    s: f32,
    at: Vec2,
    side: Vec2,
    wide: f32,
    phase: f32,
}
/// A round shape on the counter: a lobe of a puddle or a bubble on one.
struct Dome {
    puddle: usize,
    first: usize,
    /// Which of DOMES it is drawn as.
    kind: usize,
    offset: Vec2,
    seed: f32,
    /// A lobe's radius and when it joins (0 to 1 of POOL); None for a bubble.
    lobe: Option<(f32, f32)>,
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
/// A repeatable number from 0 to 1.
pub fn noise(n: u32) -> f32 {
    let x = n.wrapping_mul(747796405).wrapping_add(2891336453);
    let x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    ((x >> 22) ^ x) as f32 / u32::MAX as f32
}
/// A microwave seen from its own front, as level.rs builds it: the frame it stands in (x across the
/// front, z = -depth / 2 at the door), and its width, depth and height.
fn oven(map: &Map, block: usize) -> Option<(Transform, f32, f32, f32)> {
    let item = map.blocks.get(block).filter(|item| item.style == "microwave")?;
    let side = item.facing?;
    let [x0, z0, x1, z1] = item.rect.map(|v| v as f32);
    let (w, d) = if side.along_z() { (x1 - x0, z1 - z0) } else { (z1 - z0, x1 - x0) };
    let turn = match side {
        Side::MinusZ => 0.,
        Side::PlusZ => PI,
        Side::MinusX => FRAC_PI_2,
        Side::PlusX => -FRAC_PI_2,
    };
    let frame = Transform::from_xyz((x0 + x1) / 2., FLOOR, (z0 + z1) / 2.).with_rotation(Quat::from_rotation_y(turn));
    Some((frame, w, d, item.height as f32))
}
/// The point `s` metres along a path of [x, z] points, and the direction of travel there.
fn along(path: &[[f32; 2]], s: f32) -> (Vec2, Vec2) {
    let mut left = s.max(0.);
    let mut heading = Vec2::NEG_Y;
    for step in path.windows(2) {
        let (a, b) = (Vec2::from_array(step[0]), Vec2::from_array(step[1]));
        let length = a.distance(b);
        if length > 1e-6 {
            heading = (b - a) / length;
            if left <= length {
                return (a + heading * left, heading);
            }
            left -= length;
        }
    }
    (path.last().map_or(Vec2::ZERO, |p| Vec2::from_array(*p)), heading)
}
/// How far a door has swung, in radians, `t` seconds after the ding of an emergence that ends at `ends`.
fn swing(t: f32, ends: f32) -> f32 {
    OPEN * smooth(t / DOOR) * (1. - smooth((t - (ends - 1.)) / 0.9))
}
/// How much of a puddle is left: each zombie that rises out of it takes its share.
fn remaining(e: &Emergence, puddle: usize, t: f32) -> f32 {
    let count = e.puddles[puddle].count.max(1) as f32;
    let taken: f32 = e.schedule.iter().filter(|(_, i)| *i == puddle).map(|(at, _)| smooth((t - at) / FORM)).sum();
    1. - 0.45 * taken / count
}

fn blob(world: &mut World, kit: &Kit, parent: Entity, part: Part, parts: &mut Vec<(Entity, Part)>) {
    let e = world
        .spawn((
            Mesh3d(kit.sphere.clone()),
            MeshMaterial3d(kit.ooze.clone()),
            Transform::from_scale(Vec3::ZERO),
            bevy::light::NotShadowCaster,
        ))
        .id();
    world.entity_mut(parent).add_child(e);
    parts.push((e, part));
}
/// Builds the liquid of one emergence. Nothing shows yet: `pose` grows it.
fn spawn(world: &mut World, kit: &Kit, map: &Map, e: &Emergence) -> Entity {
    let root = world.spawn((Name::new("Ooze"), Transform::default(), Visibility::default())).id();
    let mut parts = vec![];
    let n = |k: u32| noise(e.id.wrapping_mul(977).wrapping_add(k));
    for &block in &e.sources {
        let Some((frame, w, d, h)) = oven(map, block) else {
            continue;
        };
        let mouth = world.spawn((frame, Visibility::default())).id();
        world.entity_mut(root).add_child(mouth);
        let (front, middle, wide) = (-d / 2., DOOR_X * w, DOOR_W * w);
        // The cavity is brim-full: a bed of ooze with lumps heaving on it and strands hanging from the roof.
        let bed = Part::Fill { at: Vec3::new(middle, SILL * h, front + 0.44 * d), size: Vec3::new(0.47 * wide, 0.14 * h, 0.42 * d), seed: 0. };
        blob(world, kit, mouth, bed, &mut parts);
        for i in 0..6 {
            let x = middle + wide * ((i as f32 + 0.5 * n(i * 5 + 300)) / 5.5 - 0.5) * 0.74;
            let size = Vec3::new((0.16 + 0.08 * n(i * 5 + 301)) * wide, (0.15 + 0.13 * n(i * 5 + 302)) * h, (0.2 + 0.1 * n(i * 5 + 303)) * d);
            // Lumps stay behind the sill: only the lip, the sheet and the runs come out over it.
            let z = front + size.z + 0.012 + n(i * 5 + 304) * (0.86 * d - 2. * size.z - 0.012).max(0.);
            let lump = Part::Fill { at: Vec3::new(x, SILL * h, z), size, seed: 0.2 + 0.8 * n(i * 5 + 305) };
            blob(world, kit, mouth, lump, &mut parts);
        }
        for i in 0..8 {
            let at = Vec3::new(middle + wide * (n(i * 5 + 400) - 0.5) * 0.8, 0.875 * h, front + (0.04 + 0.5 * n(i * 5 + 401)) * d);
            let strand = Part::Strand { at, r: 0.004 + 0.006 * n(i * 5 + 402), length: (0.12 + 0.3 * n(i * 5 + 403)) * h, seed: n(i * 5 + 404) };
            blob(world, kit, mouth, strand, &mut parts);
        }
        let lip = Part::Lip { at: Vec3::new(middle, SILL * h + 0.004, front - 0.002), size: Vec3::new(0.47 * wide, 0.014, 0.016) };
        blob(world, kit, mouth, lip, &mut parts);
        blob(world, kit, mouth, Part::Sheet { at: Vec3::new(middle, 0., front - 0.001), wide: 0.45 * wide, top: SILL * h }, &mut parts);
        let runs = ((wide / 0.035).round() as u32).clamp(3, 14);
        for i in 0..runs {
            let x = middle + wide * ((i as f32 + 0.25 + 0.5 * n(i * 7 + 1)) / runs as f32 - 0.5) * 0.94;
            let r = 0.007 + 0.008 * n(i * 7 + 2);
            let (start, seed) = (0.42 + 0.5 * n(i * 7 + 3), n(i * 7 + 4));
            for bead in [false, true] {
                blob(world, kit, mouth, Part::Run { at: Vec3::new(x, SILL * h, front - r * 0.55), r, start, seed, bead }, &mut parts);
            }
        }
        let foot = Part::Foot { at: Vec3::new(middle, 0.001, front - 0.03), size: Vec3::new(0.50 * wide, 0.0065, 0.05) };
        blob(world, kit, mouth, foot, &mut parts);
    }
    let counter = Counter::new(e, world.resource_mut::<Assets<Mesh>>().as_mut());
    let liquid = world
        .spawn((
            Mesh3d(counter.mesh.clone()),
            MeshMaterial3d(kit.ooze.clone()),
            Transform::default(),
            bevy::light::NotShadowCaster,
            // Its points move every frame: it is always drawn rather than measured again each time.
            NoFrustumCulling,
        ))
        .id();
    world.entity_mut(root).add_child(liquid);
    world.entity_mut(root).insert(Spill { id: e.id, started: e.started, parts, counter });
    root
}
/// A number from 0 to 1 that varies smoothly along a flow: a random one every `knot` metres, eased
/// between them.
fn drift(seed: u32, s: f32, knot: f32) -> f32 {
    let (k, f) = ((s / knot).floor(), (s / knot).fract());
    let at = |k: f32| noise(seed.wrapping_add(k as u32 * 7));
    at(k) + (at(k + 1.) - at(k)) * smooth(f)
}
impl Counter {
    /// Lays out the mesh for an emergence: which vertices are which flow's cross-sections and which
    /// puddle's lobes and bubbles, and the triangles between them. Nothing shows until `shape`.
    fn new(e: &Emergence, meshes: &mut Assets<Mesh>) -> Counter {
        let n = |k: u32| noise(e.id.wrapping_mul(977).wrapping_add(k));
        let total: f32 = e.puddles.iter().filter(|q| q.source >= 0).map(|q| q.length).sum();
        let space = (total / SECTIONS as f32).max(SPACING);
        let (mut flows, mut domes, mut indices, mut count) = (vec![], vec![], Vec::<u16>::new(), 0usize);
        for (index, puddle) in e.puddles.iter().enumerate() {
            if puddle.source >= 0 && puddle.length > 0. {
                let steps = (puddle.length / space).ceil().max(1.) as usize;
                let seed = e.id.wrapping_mul(977).wrapping_add(index as u32 * 4099);
                let sections: Vec<Section> = (0..=steps)
                    .map(|i| {
                        let s = puddle.length * i as f32 / steps as f32;
                        // The way the flow is heading here, eased round the corners of its path.
                        let (ahead, behind) = (along(&puddle.path, s + 0.015), along(&puddle.path, (s - 0.015).max(0.)));
                        let heading = (ahead.0 - behind.0).try_normalize().unwrap_or(ahead.1);
                        let side = Vec2::new(-heading.y, heading.x);
                        let at = along(&puddle.path, s).0 + side * (drift(seed + 5, s, 0.03) - 0.5) * 0.007;
                        Section { s, at, side, wide: drift(seed + 6, s, 0.025), phase: drift(seed + 7, s, 0.04) * 2. }
                    })
                    .collect();
                for i in 0..steps {
                    for j in 0..ACROSS - 1 {
                        let (a, b) = ((count + i * ACROSS + j) as u16, (count + (i + 1) * ACROSS + j) as u16);
                        indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
                    }
                }
                flows.push(Flow { puddle: index, first: count, sections });
                count += (steps + 1) * ACROSS;
            }
            // A puddle is a wide dome with smaller lobes round its edge, so it never reads as a disc.
            let mut dome = |kind: usize, offset: Vec2, seed: f32, lobe: Option<(f32, f32)>| {
                let (round, rings) = DOMES[kind];
                let at = |ring: usize, i: usize| (count + ring * round + i % round) as u16;
                for ring in 0..rings {
                    for i in 0..round {
                        if ring + 1 < rings {
                            indices.extend_from_slice(&[at(ring, i), at(ring + 1, i), at(ring, i + 1), at(ring, i + 1), at(ring + 1, i), at(ring + 1, i + 1)]);
                        } else {
                            indices.extend_from_slice(&[at(ring, i), (count + rings * round) as u16, at(ring, i + 1)]);
                        }
                    }
                }
                domes.push(Dome { puddle: index, first: count, kind, offset, seed, lobe });
                count += rings * round + 1;
            };
            dome(0, Vec2::ZERO, 0., Some((PUDDLE, 0.)));
            for lobe in 0..6 {
                let k = index as u32 * 613 + lobe * 31 + 900;
                let angle = (lobe as f32 + n(k)) * TAU / 6.;
                let offset = Vec2::new(angle.cos(), angle.sin()) * PUDDLE * (0.5 + 0.25 * n(k + 1));
                dome(1, offset, n(k + 4), Some((PUDDLE * (0.42 + 0.3 * n(k + 2)), 0.1 + 0.45 * n(k + 3))));
            }
            for bubble in 0..5 {
                let k = index as u32 * 389 + bubble * 17 + 1700;
                let angle = n(k) * TAU;
                dome(2, Vec2::new(angle.cos(), angle.sin()) * PUDDLE * 0.62 * n(k + 1).sqrt(), n(k + 2), None);
            }
        }
        assert!(count <= u16::MAX as usize, "the liquid of one emergence has {count} points");
        let units = DOMES.map(|(round, rings)| {
            let mut points = vec![];
            for ring in 0..rings {
                let rise = ring as f32 / rings as f32 * FRAC_PI_2;
                points.extend((0..round).map(|i| {
                    let turn = i as f32 / round as f32 * TAU;
                    Vec3::new(rise.cos() * turn.cos(), rise.sin(), rise.cos() * turn.sin())
                }));
            }
            points.push(Vec3::Y);
            points
        });
        let (positions, normals) = (vec![[0f32; 3]; count], vec![[0., 1., 0.]; count]);
        // Laid out as Bevy's own primitives are (the texture coordinates say nothing), so that the mesh
        // is drawn by the pipelines the microwave's blobs already use and none is compiled for it.
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0f32; 2]; count])
            .with_inserted_indices(Indices::U16(indices));
        Counter { mesh: meshes.add(mesh), flows, domes, units, positions, normals }
    }
    /// Moves every point to where the liquid is `t` seconds after the emergence's ding; `now` drives the
    /// ripples. What has not arrived yet, or has drained away, is drawn in on itself to nothing.
    fn shape(&mut self, e: &Emergence, t: f32, now: f32) {
        let drain = 1. - smooth((t - (e.ends - DRAIN)) / (DRAIN * 0.8));
        let Counter { flows, domes, units, positions, normals, .. } = self;
        for flow in flows.iter() {
            let q = &e.puddles[flow.puddle];
            let travelled = (t - SPILL) * FLOW;
            let head = travelled.clamp(0., q.length);
            // A flow thins once its puddle has formed its last zombie; its front is swollen until it arrives.
            let fed = (1. - 0.5 * smooth((t - q.until) / 0.8)) * drain;
            let front = smooth(travelled / 0.03) * (1. - smooth((travelled - q.length) / 0.03));
            for (i, section) in flow.sections.iter().enumerate() {
                let (at, out) = (flow.first + i * ACROSS, Vec3::new(section.side.x, 0., section.side.y));
                let middle = Vec3::new(section.at.x, FLOOR, section.at.y);
                let grown = if travelled > 0. { smooth((head - section.s) / 0.035) * fed } else { 0. };
                if grown <= 0. {
                    positions[at..at + ACROSS].fill(middle.to_array());
                    continue;
                }
                let wave = 1. + 0.2 * (section.s * 85. - now * 6.5 + section.phase).sin();
                let behind = (head - section.s - 0.012) / 0.018;
                let swell = if behind.abs() < 2.5 { front * (-behind * behind).exp() } else { 0. };
                let wide = (0.014 + 0.007 * section.wide) * wave * grown * (1. + 0.4 * swell);
                let deep = (0.0045 + 0.0025 * wave) * grown * (1. + 1.3 * swell);
                for (j, (across, up)) in [(1., 0.), (0.7071, 0.7071), (0., 1.), (-0.7071, 0.7071), (-1., 0.)].into_iter().enumerate() {
                    positions[at + j] = (middle + out * (wide * across) + Vec3::Y * (deep * up)).to_array();
                    normals[at + j] = (out * (deep * across) + Vec3::Y * (wide * up)).normalize_or(Vec3::Y).to_array();
                }
            }
        }
        // How much of each puddle is left: each zombie that rises out of it takes its share.
        let left: Vec<f32> = (0..e.puddles.len()).map(|puddle| remaining(e, puddle, t)).collect();
        for dome in domes.iter() {
            let q = &e.puddles[dome.puddle];
            let (centre, size, deep) = match dome.lobe {
                Some((r, delay)) => {
                    let filled = smooth((t - q.arrive - delay * POOL) / (POOL * (1. - delay)));
                    let left = left[dome.puddle] * (1. - smooth((t - q.until - 0.2) / (DRAIN - 0.3)));
                    let size = r * filled.powf(0.6) * left * (1. + 0.05 * (now * 2.1 + dome.seed * TAU).sin());
                    let deep = (0.0045 + 0.004 * filled) * smooth(filled * 8.) * left.sqrt() * (r / PUDDLE).sqrt();
                    (Vec3::new(q.p[0] + dome.offset.x * left, FLOOR, q.p[2] + dome.offset.y * left), size, deep)
                }
                None => {
                    // Bubbles rise while the puddle is live: hardest where the liquid wells up from below.
                    let live = smooth((t - q.arrive) / 0.4) * (1. - smooth((t - q.until) / 0.5));
                    let cycle = (now * (0.7 + 0.9 * dome.seed) + dome.seed * 9.) % 1.;
                    let swell = (cycle * PI).sin().powf(0.6) * if cycle > 0.92 { 0. } else { 1. };
                    let size = (0.006 + 0.007 * dome.seed) * swell * live * if q.source < 0 { 1.5 } else { 1. };
                    (Vec3::new(q.p[0] + dome.offset.x, FLOOR + 0.004, q.p[2] + dome.offset.y), size, size * 0.8)
                }
            };
            let unit = &units[dome.kind];
            if size <= 0. || deep <= 0. {
                positions[dome.first..dome.first + unit.len()].fill(centre.to_array());
                continue;
            }
            for (i, point) in unit.iter().enumerate() {
                positions[dome.first + i] = (centre + *point * Vec3::new(size, deep, size)).to_array();
                normals[dome.first + i] = (*point * Vec3::new(deep, size, deep)).normalize_or(Vec3::Y).to_array();
            }
        }
    }
    /// Reshapes the liquid and hands the new points to the mesh.
    fn draw(&mut self, meshes: &mut Assets<Mesh>, e: &Emergence, t: f32, now: f32) {
        self.shape(e, t, now);
        let Some(mut mesh) = meshes.get_mut(&self.mesh) else {
            return;
        };
        if let Some(VertexAttributeValues::Float32x3(points)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
            points.copy_from_slice(&self.positions);
        }
        if let Some(VertexAttributeValues::Float32x3(points)) = mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL) {
            points.copy_from_slice(&self.normals);
        }
    }
}
/// Places every blob in and on the microwaves `t` seconds after the emergence's ding; `now` drives
/// the ripples.
fn pose(world: &mut World, e: &Emergence, parts: &[(Entity, Part)], t: f32, now: f32) {
    // The microwave empties while the puddles fill, and everything shrinks away at the end.
    let poured = smooth((t - SPILL) / (e.ends - DRAIN - SPILL).max(0.5));
    let drain = 1. - smooth((t - (e.ends - DRAIN)) / (DRAIN * 0.8));
    for (entity, part) in parts {
        let (at, scale, turn) = match part {
            Part::Fill { at, size, seed } => {
                let level = (1. - 0.7 * poured) * drain;
                let heave = 1. + 0.16 * seed * (now * (1.3 + 1.7 * seed) + seed * 40.).sin();
                (*at + Vec3::Y * size.y * level * heave * 0.3, *size * Vec3::new(1., level * heave, 1.) * smooth(t / 0.15) * smooth(drain * 4.), 0.)
            }
            Part::Strand { at, r, length, seed } => {
                // Strands stretch and thin, and are gone once the microwave has emptied.
                let hang = *length * (0.7 + 0.3 * (now * (0.9 + seed) + seed * 30.).sin()) * (1. - 0.8 * poured) * drain;
                let thick = *r * smooth(t / 0.15) * smooth(hang * 80.);
                (*at - Vec3::Y * hang * 0.5, Vec3::new(thick, hang * 0.5 * smooth(t / 0.15), thick), 0.)
            }
            Part::Lip { at, size } => {
                let swell = smooth((t - 0.3) / 0.4) * drain;
                (*at, *size * Vec3::new(smooth(swell * 6.), swell * (1. + 0.2 * (now * 3.1).sin()), swell), 0.)
            }
            Part::Sheet { at, wide, top } => {
                let reach = smooth((t - 0.55) / 0.8) * drain;
                (*at + Vec3::Y * *top * (1. - reach * 0.5), Vec3::new(*wide * (0.6 + 0.4 * reach) * smooth(reach * 6.), *top * reach * 0.52, 0.006 * reach), 0.)
            }
            Part::Run { at, r, start, seed, bead } => {
                // The run stretches from the sill to the counter, then keeps pulsing as more comes down.
                let reach = smooth((t - start) / (SPILL - 0.1 - start)) * drain;
                let pulse = 1. + 0.22 * (now * (4. + 3. * seed) + seed * TAU).sin();
                let length = at.y * reach;
                if *bead {
                    let swell = r * 1.45 * pulse * smooth(reach * 6.) * drain;
                    (*at - Vec3::Y * (length - swell * 0.6).max(0.), Vec3::new(swell, swell * 1.2, swell * 0.8), 0.)
                } else {
                    (*at - Vec3::Y * length * 0.5, Vec3::new(r * pulse.sqrt(), length * 0.5 + r * 0.4, r * 0.7) * smooth(reach * 8.), 0.)
                }
            }
            Part::Foot { at, size } => {
                let spread = smooth((t - (SPILL - 0.45)) / 0.8) * drain;
                (*at, *size * Vec3::new(spread.sqrt(), spread, spread.sqrt()), 0.)
            }
        };
        if let Some(mut tr) = world.get_mut::<Transform>(*entity) {
            *tr = Transform::from_translation(at).with_rotation(Quat::from_rotation_y(turn)).with_scale(scale);
        }
    }
}

/// Every frame: draws the emergences under way, swings the microwave doors, blinks the clock of a
/// microwave that has finished, and sounds the ding of a new emergence.
pub fn update(world: &mut World) {
    if !world.contains_resource::<Game>() {
        return;
    }
    world.init_resource::<Kit>();
    let kit = world.resource::<Kit>().clone();
    let now = world.resource::<Time<Real>>().elapsed_secs();
    world.resource_scope(|world, g: Mut<Game>| {
        let (feast, elapsed) = (&g.feast, g.elapsed);
        let live = |spill: &Spill| feast.emergences.iter().find(|e| e.id == spill.id && (e.started - spill.started).abs() < 0.01);
        let roots: Vec<Entity> = crate::queries::with::<Entity, With<Spill>, _>(world, |world, q| q.iter(world).collect());
        let mut drawn = vec![];
        for root in roots {
            let Some(mut spill) = world.entity_mut(root).take::<Spill>() else {
                continue;
            };
            match live(&spill) {
                Some(e) if g.playing() => {
                    pose(world, e, &spill.parts, elapsed - e.started, now);
                    spill.counter.draw(world.resource_mut::<Assets<Mesh>>().as_mut(), e, elapsed - e.started, now);
                    drawn.push(e.id);
                    world.entity_mut(root).insert(spill);
                }
                _ => world.entity_mut(root).despawn(),
            }
        }
        for e in &feast.emergences {
            if g.playing() && !drawn.contains(&e.id) {
                spawn(world, &kit, &feast.level.map, e);
                // The timer goes off. Joining a round part-way through is not a ding.
                if elapsed - e.started < 0.5 {
                    if let Some(ding) = &kit.ding {
                        world.spawn((AudioPlayer::new(ding.clone()), PlaybackSettings::DESPAWN));
                    }
                }
            }
        }
        // Doors are open on the microwaves that are pouring and shut on the rest. For each microwave:
        // how far its door is open, and the seconds since its latest ding.
        let state = |block: usize| {
            let pouring = feast.emergences.iter().filter(|e| e.sources.contains(&block));
            pouring.fold((0f32, f32::MAX), |(open, since), e| (open.max(swing(elapsed - e.started, e.ends)), since.min(elapsed - e.started)))
        };
        let mut ovens = HashMap::new();
        // (Each written only when it changes: a write makes the engine work out the door's placement and visibility again.)
        crate::queries::with::<(&Hinge, &ChildOf, &mut Transform), (), _>(world, |world, q| {
            for (hinge, parent, mut tr) in q.iter_mut(world) {
                let (open, since) = state(hinge.0);
                let rotation = Quat::from_rotation_y(open);
                if tr.rotation != rotation {
                    tr.rotation = rotation;
                }
                ovens.insert(parent.parent(), (open, since));
            }
        });
        // Nothing lurks behind an open door, and a finished microwave blinks its zeros.
        crate::queries::with::<(&ChildOf, &mut Visibility), With<MicrowaveGlow>, _>(world, |world, q| {
            for (parent, mut vis) in q.iter_mut(world) {
                if ovens.get(&parent.parent()).is_some_and(|(open, _)| *open > 0.02) {
                    vis.set_if_neq(Visibility::Hidden);
                }
            }
        });
        crate::queries::with::<(&ChildOf, &mut Visibility), With<MicrowaveDigit>, _>(world, |world, q| {
            for (parent, mut vis) in q.iter_mut(world) {
                if ovens.get(&parent.parent()).is_some_and(|(open, since)| *open > 0. && (since * 2.5) % 1. > 0.6) {
                    vis.set_if_neq(Visibility::Hidden);
                }
            }
        });
        if world.resource::<crate::runtime::Options>().check {
            view(world, elapsed);
        }
    });
}

/// Checks only (`?check` in the page's address): `window.hotdogView = [eye x, y, z, target x, y, z,
/// field of view in degrees]` pins the camera so a capture can frame the microwave or a puddle, and
/// `window.hotdogElapsed` is the round's clock every frame.
fn view(world: &mut World, elapsed: f32) {
    #[cfg(target_arch = "wasm32")]
    {
        let v = check_view(elapsed);
        if v.len() < 6 {
            return;
        }
        let cameras: Vec<Entity> = world.query_filtered::<Entity, With<crate::maya::HotdogCamera>>().iter(world).collect();
        for camera in cameras {
            if let Some(mut tr) = world.get_mut::<Transform>(camera) {
                *tr = Transform::from_xyz(v[0], v[1], v[2]).looking_at(Vec3::new(v[3], v[4], v[5]), Vec3::Y);
            }
            if let (Some(fov), Some(mut projection)) = (v.get(6), world.get_mut::<Projection>(camera)) {
                if let Projection::Perspective(ref mut p) = *projection {
                    p.fov = fov.to_radians();
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (world, elapsed);
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
const checking=new URLSearchParams(location.search).has('check');
export function check_view(elapsed){if(!checking)return new Float32Array(0);window.hotdogElapsed=elapsed;const v=window.hotdogView;return new Float32Array(Array.isArray(v)&&v.length>=6&&v.every(Number.isFinite)?v:[])}
")]
extern "C" {
    fn check_view(elapsed: f32) -> Vec<f32>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::feast::Feast;

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Kit>();
        world
    }
    /// The world-space bounds of everything that shows, as (lowest corner, highest corner): the blobs
    /// in and on the microwaves, and every triangle of the liquid on the counter that has any size.
    fn extent(world: &mut World, root: Entity) -> Option<(Vec3, Vec3)> {
        let spill = world.get::<Spill>(root).unwrap();
        let parts: Vec<Entity> = spill.parts.iter().map(|(e, _)| *e).collect();
        let mut bounds: Option<(Vec3, Vec3)> = None;
        let mut grow = |low: Vec3, high: Vec3| {
            let (a, b) = bounds.unwrap_or((low, high));
            bounds = Some((a.min(low), b.max(high)));
        };
        let mesh = world.resource::<Assets<Mesh>>().get(&spill.counter.mesh).unwrap();
        let Some(VertexAttributeValues::Float32x3(points)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("no points") };
        let Some(Indices::U16(indices)) = mesh.indices() else { panic!("no triangles") };
        for corner in indices.chunks(3) {
            let [a, b, c] = [corner[0], corner[1], corner[2]].map(|i| Vec3::from_array(points[i as usize]));
            if (b - a).cross(c - a).length() > 1e-9 {
                grow(a.min(b).min(c), a.max(b).max(c));
            }
        }
        for e in parts {
            let tr = *world.get::<Transform>(e).unwrap();
            let frame = world.get::<ChildOf>(e).and_then(|parent| world.get::<Transform>(parent.parent())).copied().unwrap_or_default();
            if tr.scale.max_element() < 1e-4 {
                continue;
            }
            let reach = (frame.rotation * (tr.rotation * tr.scale)).abs();
            let at = frame.transform_point(tr.translation);
            grow(at - reach, at + reach);
        }
        bounds
    }
    /// Places and reshapes an emergence's liquid as `update` does.
    fn show(world: &mut World, root: Entity, e: &Emergence, t: f32) -> Option<(Vec3, Vec3)> {
        let mut spill = world.entity_mut(root).take::<Spill>().unwrap();
        pose(world, e, &spill.parts, t, 0.);
        spill.counter.draw(world.resource_mut::<Assets<Mesh>>().as_mut(), e, t, 0.);
        world.entity_mut(root).insert(spill);
        extent(world, root)
    }
    #[test]
    fn the_liquid_follows_the_emergence_from_the_door_to_the_puddles_and_drains() {
        let mut world = world();
        let kit = world.resource::<Kit>().clone();
        let mut feast = Feast::new(false);
        feast.emerge(6);
        let e = feast.emergences[0].clone();
        let root = spawn(&mut world, &kit, &feast.level.map, &e);
        let mut at = |t: f32| show(&mut world, root, &e, t);
        // At the ding nothing shows. As the door opens the ooze is still inside the microwave, whose
        // front is at z = 3.1; then it comes down the front and reaches the counter.
        assert!(at(0.).is_none());
        let inside = at(0.25).unwrap();
        assert!(inside.0.z > 3.1 && inside.1.z < 3.43 && inside.0.x > -1.46 && inside.1.x < -0.96, "{inside:?}");
        let spilled = at(SPILL).unwrap();
        assert!(spilled.0.z < 3.1 && spilled.0.z > 3.02 && spilled.0.y < FLOOR + 0.002, "{spilled:?}");
        // Once the puddles are full the liquid covers every spawn point and a puddle's width round it.
        let full = at(e.puddles[2].arrive + POOL).unwrap();
        assert!(full.0.x < -1.23 - 0.05 && full.1.x > -1.16 + 0.05 && full.0.z < 3.02 - 0.05, "{full:?}");
        // It never leaves the counter top, and at the end it has drained away.
        assert!(full.0.y > FLOOR - 0.02 && full.1.y < FLOOR + 0.25);
        assert!(at(e.ends).is_none());
    }
    #[test]
    fn a_flow_runs_the_length_of_its_path_and_a_map_without_a_microwave_has_none() {
        let path = [[0., 0.], [0., -1.], [2., -1.]];
        assert_eq!(along(&path, 0.5), (Vec2::new(0., -0.5), Vec2::NEG_Y));
        assert_eq!(along(&path, 2.), (Vec2::new(1., -1.), Vec2::X));
        assert_eq!(along(&path, 9.), (Vec2::new(2., -1.), Vec2::X));
        let mut world = world();
        let kit = world.resource::<Kit>().clone();
        let count = |world: &mut World, map: Map| {
            let mut feast = Feast::with_map(map, false);
            feast.emerge(2);
            let root = spawn(world, &kit, &feast.level.map, &feast.emergences[0]);
            let spill = world.get::<Spill>(root).unwrap();
            let fills = spill.parts.iter().filter(|(_, part)| matches!(part, Part::Fill { .. })).count();
            let (flows, domes) = (&spill.counter.flows, &spill.counter.domes);
            // Every flow's cross-sections run from the door to its puddle, close enough together to read as one stream.
            for flow in flows {
                let q = &feast.emergences[0].puddles[flow.puddle];
                assert!(flow.sections[0].s == 0. && (flow.sections.last().unwrap().s - q.length).abs() < 1e-5);
                assert!(flow.sections.windows(2).all(|pair| pair[1].s > pair[0].s && pair[1].s - pair[0].s <= SPACING + 1e-5));
                assert!(flow.sections.last().unwrap().at.distance(Vec2::new(q.p[0], q.p[2])) < 0.004);
            }
            [fills, flows.len(), flows.iter().map(|flow| flow.sections.len()).sum(), domes.iter().filter(|dome| dome.lobe.is_some()).count(), domes.len()]
        };
        let fed = count(&mut world, Map::bundled());
        assert!(fed[0] == 7 && fed[1] == 2 && fed[2] > 10 && fed[3] == 14 && fed[4] == 24, "{fed:?}");
        let mut bare = Map::bundled();
        bare.blocks.retain(|block| block.style != "microwave");
        assert_eq!(count(&mut world, bare), [0, 0, 0, 14, 24]);
    }
    /// A map may have 64 spawn points. However many puddles an emergence has, what is on the counter is
    /// one mesh (a blob for every bead of every flow was eight thousand of them), small enough to index
    /// with 16 bits, and at the end of the arrival every puddle is there.
    #[test]
    fn the_liquid_of_64_puddles_is_one_mesh() {
        let mut world = world();
        let kit = world.resource::<Kit>().clone();
        let mut map = Map::bundled();
        let spare = map.spawns[0].clone();
        for i in 0..61 {
            let mut spawn = spare.clone();
            spawn.p = [-1.6 + 0.4 * (i % 9) as f64, 2.4 + 0.1 * (i / 9) as f64];
            map.spawns.push(spawn);
        }
        let mut feast = Feast::with_map(map, false);
        let wave = feast.wave();
        feast.emerge(wave);
        let e = feast.emergences[0].clone();
        assert_eq!((wave, e.puddles.len()), (128, 64));
        let root = spawn(&mut world, &kit, &feast.level.map, &e);
        let spill = world.get::<Spill>(root).unwrap();
        let meshes = spill.parts.len() + 1;
        let corners = world.resource::<Assets<Mesh>>().get(&spill.counter.mesh).unwrap().indices().unwrap().len();
        let (points, sections) = (spill.counter.positions.len(), spill.counter.flows.iter().map(|flow| flow.sections.len()).sum::<usize>());
        assert!(meshes <= 60, "{meshes} meshes");
        assert!(points <= u16::MAX as usize && sections <= SECTIONS + 64 && corners % 3 == 0, "{points} points in {sections} cross-sections");
        assert_eq!(spill.counter.domes.len(), 64 * 12);
        // Half-way along the longest flow, its ribbon has width behind its front and none ahead of it.
        let longest = (0..spill.counter.flows.len()).max_by(|a, b| e.puddles[spill.counter.flows[*a].puddle].length.total_cmp(&e.puddles[spill.counter.flows[*b].puddle].length)).unwrap();
        let length = e.puddles[spill.counter.flows[longest].puddle].length;
        assert!(length > 2.5);
        show(&mut world, root, &e, SPILL + length / FLOW * 0.5);
        let spill = world.get::<Spill>(root).unwrap();
        let flow = &spill.counter.flows[longest];
        for (i, section) in flow.sections.iter().enumerate() {
            let points = &spill.counter.positions[flow.first + i * ACROSS..flow.first + (i + 1) * ACROSS];
            let (wide, high) = (Vec3::from_array(points[0]).distance(Vec3::from_array(points[ACROSS - 1])), points[ACROSS / 2][1] - FLOOR);
            if section.s < length * 0.5 - 0.04 {
                assert!(wide > 0.02 && wide < 0.06 && high > 0.003 && high < 0.012, "{wide} wide and {high} high at {}", section.s);
            } else if section.s > length * 0.5 {
                assert!(wide == 0. && high == 0., "{wide} wide and {high} high at {}", section.s);
            }
        }
        // Every puddle is full when its first zombie rises, and the liquid reaches from one end of the counter to the other.
        let last = e.puddles.iter().map(|q| q.arrive).fold(0f32, f32::max);
        let full = show(&mut world, root, &e, last + POOL).unwrap();
        assert!(full.0.x < -1.6 && full.1.x > 1.6 && full.1.y < FLOOR + 0.25 && full.0.y > FLOOR - 0.02, "{full:?}");
        assert!(show(&mut world, root, &e, e.ends).is_none());
    }
    #[test]
    fn every_microwave_door_hangs_on_a_hinge_that_knows_its_block() {
        let mut world = world();
        let hinges = |world: &mut World, map: &Map| {
            crate::level::build(world, map);
            let mut found: Vec<(usize, Vec3, usize)> = world
                .query::<(&Hinge, &Transform, &Children)>()
                .iter(world)
                .map(|(hinge, tr, children)| (hinge.0, tr.translation, children.len()))
                .collect();
            found.sort_by_key(|hinge| hinge.0);
            found
        };
        // The bundled microwave is block 0, 0.5 m wide and 0.33 m deep: the hinge is down the door's
        // left edge, just proud of the front, and carries the glass and its handle.
        let bundled = hinges(&mut world, &Map::bundled());
        assert_eq!(bundled.len(), 1);
        assert_eq!((bundled[0].0, bundled[0].2), (0, 2));
        assert!(bundled[0].1.distance(Vec3::new(-0.427 * 0.5, 0., -0.165 - 0.008)) < 1e-5);
        assert!(oven(&Map::bundled(), 0).is_some_and(|(frame, w, d, h)| frame.translation.distance(Vec3::new(-1.21, FLOOR, 3.265)) < 1e-5 && (w - 0.5).abs() < 1e-5 && (d - 0.33).abs() < 1e-5 && h == 0.25));
        assert!(oven(&Map::bundled(), 1).is_none(), "a toaster is no microwave");
        let two = Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "bounds": [0, 0, 2, 1.2], "starts": [{"p": [1, 0.1]}], "spawns": [{"p": [1, 0.6]}],
            "blocks": [{"rect": [0.9, 0.9, 1.1, 1.1]}, {"rect": [0, 0.2, 0.2, 0.6], "style": "microwave", "facing": "+x"}, {"rect": [1.8, 0.4, 2, 0.8], "style": "microwave", "facing": "-x"}]}"#,
        )
        .unwrap();
        assert_eq!(hinges(&mut world, &two).iter().map(|hinge| hinge.0).collect::<Vec<_>>(), [1, 2]);
        // A door that faces +x opens out over the counter, towards +x.
        let (frame, ..) = oven(&two, 1).unwrap();
        assert!((frame.rotation * Vec3::NEG_Z).distance(Vec3::X) < 1e-6);
    }
    #[test]
    fn the_door_opens_after_the_ding_and_shuts_as_the_liquid_drains() {
        assert_eq!(swing(0., 10.), 0.);
        assert!((swing(DOOR, 10.) - OPEN).abs() < 1e-6 && (swing(5., 10.) - OPEN).abs() < 1e-6);
        assert!(swing(9.5, 10.) < OPEN * 0.6 && swing(10., 10.) == 0.);
        // Each zombie that rises takes its share of the puddle.
        let mut feast = Feast::new(false);
        feast.emerge(6);
        let e = &feast.emergences[0];
        assert_eq!(remaining(e, 0, e.schedule[0].0), 1.);
        assert!(remaining(e, 0, e.schedule[0].0 + FORM) < 0.8 && remaining(e, 0, e.ends) < 0.56);
    }
}

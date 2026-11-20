// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Light drawn as a multitude of fine parts: thousands of hair-thin rays from the ceiling sun, a corona
//! of radial spikes, glints on the counter and dust motes. Each kind is one mesh of four-vertex ribbons
//! that `assets/shaders/filaments.wgsl` widens, shimmers and blends additively, so five thousand rays
//! cost one draw call and no lights.
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, MeshVertexBufferLayoutRef},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, PrimitiveTopology, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct FilamentMaterial {
    /// Linear colour, and the overall intensity in alpha.
    #[uniform(0)]
    pub color: Vec4,
    /// Shimmer rate, shimmer depth, drift distance in metres, drift rate.
    #[uniform(0)]
    pub motion: Vec4,
    /// Near fade start and end in metres, taper toward the far end, narrowest width in pixels.
    #[uniform(0)]
    pub fade: Vec4,
    /// Widest a ribbon may be drawn, in pixels (0 for no limit); the rest is unused.
    #[uniform(0)]
    pub limits: Vec4,
}

impl Material for FilamentMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/filaments.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/filaments.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// How the shader lays a filament out.
#[derive(Clone, Copy)]
pub enum Shape {
    /// A ribbon that turns about its own axis to face the camera.
    Ray = 0,
    /// A round dot facing the camera.
    Sprite = 1,
    /// A ribbon lying flat on a horizontal surface.
    Flat = 2,
}

/// Collects filaments and turns them into one mesh.
#[derive(Default)]
pub struct Filaments {
    positions: Vec<[f32; 3]>,
    reach: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    traits: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Filaments {
    pub fn push(&mut self, from: Vec3, to: Vec3, width: f32, brightness: f32, seed: f32, shape: Shape) {
        let base = self.positions.len() as u32;
        for uv in [[0., 0.], [0., 1.], [1., 1.], [1., 0.]] {
            self.positions.push(from.to_array());
            self.reach.push((to - from).to_array());
            self.uvs.push(uv);
            self.traits.push([seed, width, brightness, shape as u32 as f32]);
        }
        self.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    pub fn len(&self) -> usize {
        self.positions.len() / 4
    }

    pub fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.reach)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.traits)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// A small deterministic generator, so every run and every machine draws the same rays.
pub struct Dice(pub u32);

impl Dice {
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(747796405).wrapping_add(2891336453);
        let word = ((self.0 >> ((self.0 >> 28) + 4)) ^ self.0).wrapping_mul(277803737);
        ((word >> 22) ^ word) as f32 / u32::MAX as f32
    }
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next()
    }
    /// A point in the unit disc, crowded toward the middle when `gather` is above one.
    pub fn disc(&mut self, gather: f32) -> Vec2 {
        let angle = self.range(0., std::f32::consts::TAU);
        let radius = self.next().sqrt().powf(gather);
        Vec2::from_angle(angle) * radius
    }
}

/// Rays from a disc of light on the ceiling down toward a rectangle of counter.
///
/// Most of them are gathered into `beams`: each beam is a sheaf of hundreds of nearly parallel rays
/// landing in one small pool, so from across the room it reads as a single shaft of light and from
/// close by as the filaments it is made of. The rest are strewn singly over the whole rectangle.
/// Rays start crowded around the middle of the disc and most stop in mid-air, so every sheaf is dense
/// and bright at the source and thins to a few faint threads above the play area.
pub fn sunbeam(
    count: usize,
    sun: Vec3,
    disc: f32,
    target_min: Vec3,
    target_max: Vec3,
    beams: &[(Vec3, f32)],
    seed: u32,
) -> Filaments {
    let mut dice = Dice(seed);
    let mut rays = Filaments::default();
    for i in 0..count {
        let from = sun + (dice.disc(1.4) * disc).extend(0.).xzy() - Vec3::Y * 0.004;
        let loose = beams.is_empty() || dice.next() < 0.22;
        let to = if loose {
            Vec3::new(
                dice.range(target_min.x, target_max.x),
                target_min.y,
                dice.range(target_min.z, target_max.z),
            )
        } else {
            let (pool, radius) = beams[i % beams.len()];
            pool + (dice.disc(1.6) * radius).extend(0.).xzy()
        };
        // Most rays give out before the counter; the rest land on it.
        let length = if dice.next() < 0.3 { 1. } else { dice.range(0.22, 0.9) };
        let width = dice.range(0.0009, 0.0026) * if dice.next() < 0.06 { 2.4 } else { 1. };
        let brightness = dice.range(0.35, 1.) * if dice.next() < 0.1 { 1.8 } else { 1. } * if loose { 0.6 } else { 1. };
        rays.push(from, from + (to - from) * length, width, brightness, dice.next(), Shape::Ray);
    }
    rays
}

/// Parallel shafts dropping along `direction` from scattered gaps in a canopy at `top`.
pub fn shafts(
    count: usize,
    min: Vec3,
    max: Vec3,
    top: f32,
    direction: Vec3,
    seed: u32,
) -> Filaments {
    let mut dice = Dice(seed);
    let mut rays = Filaments::default();
    // Shafts come in bundles, one per gap in the leaves.
    let gaps: Vec<(Vec3, f32)> = (0..(count / 60).max(1))
        .map(|_| {
            let foot = Vec3::new(dice.range(min.x, max.x), min.y, dice.range(min.z, max.z));
            (foot, dice.range(0.02, 0.09))
        })
        .collect();
    for i in 0..count {
        let (foot, spread) = gaps[i % gaps.len()];
        let offset = dice.disc(1.) * spread;
        let to = foot + Vec3::new(offset.x, 0., offset.y);
        let rise = (top - to.y) / -direction.y;
        let from = to - direction * rise;
        let start = dice.range(0., 0.25);
        let stop = if dice.next() < 0.35 { 1. } else { dice.range(0.45, 0.9) };
        rays.push(
            from.lerp(to, start),
            from.lerp(to, stop),
            dice.range(0.0009, 0.003),
            dice.range(0.3, 1.),
            dice.next(),
            Shape::Ray,
        );
    }
    rays
}

/// Radial spikes lying in the ceiling plane around a disc: the sun's corona.
pub fn corona(count: usize, centre: Vec3, disc: f32, reach: f32, seed: u32) -> Filaments {
    let mut dice = Dice(seed);
    let mut spikes = Filaments::default();
    for i in 0..count {
        let angle = (i as f32 + dice.next()) / count as f32 * std::f32::consts::TAU;
        let out = Vec3::new(angle.cos(), 0., angle.sin());
        // A few long spikes among many short ones, as in a drawing of the sun.
        let long = dice.next();
        let length = reach * (0.12 + 0.88 * long * long * long) * if i % 16 == 0 { 1.25 } else { 1. };
        let from = centre + out * disc * dice.range(0.82, 1.02);
        spikes.push(
            from,
            from + out * length,
            dice.range(0.0015, 0.0055),
            dice.range(0.4, 1.),
            dice.next(),
            Shape::Ray,
        );
    }
    spikes
}

/// Small glints lying on a horizontal surface, strewn along `direction` like caustic flecks.
pub fn glints(count: usize, min: Vec3, max: Vec3, direction: Vec2, seed: u32) -> Filaments {
    let mut dice = Dice(seed);
    let mut flecks = Filaments::default();
    for _ in 0..count {
        let at = Vec3::new(dice.range(min.x, max.x), min.y, dice.range(min.z, max.z));
        let turn = Vec2::from_angle(dice.range(-0.5, 0.5)).rotate(direction);
        let length = dice.range(0.004, 0.016);
        flecks.push(
            at,
            at + Vec3::new(turn.x, 0., turn.y) * length,
            dice.range(0.0012, 0.0034),
            dice.range(0.25, 1.),
            dice.next(),
            Shape::Flat,
        );
    }
    flecks
}

/// Dots scattered through a box: dust motes, pollen, fireflies.
pub fn motes(count: usize, min: Vec3, max: Vec3, size: (f32, f32), seed: u32) -> Filaments {
    let mut dice = Dice(seed);
    let mut dots = Filaments::default();
    for _ in 0..count {
        let at = Vec3::new(
            dice.range(min.x, max.x),
            dice.range(min.y, max.y),
            dice.range(min.z, max.z),
        );
        dots.push(
            at,
            at + Vec3::Y * 0.001,
            dice.range(size.0, size.1),
            dice.range(0.2, 1.),
            dice.next(),
            Shape::Sprite,
        );
    }
    dots
}

/// A star of spikes plus a soft dot around each point: the glow of many small bulbs.
pub fn sparkles(points: &[Vec3], spikes: usize, reach: f32, size: f32, seed: u32) -> Filaments {
    let mut dice = Dice(seed);
    let mut glow = Filaments::default();
    for &at in points {
        glow.push(at, at + Vec3::Y * 0.001, size, 1., dice.next(), Shape::Sprite);
        for _ in 0..spikes {
            let out = Vec3::new(dice.range(-1., 1.), dice.range(-1., 1.), dice.range(-1., 1.))
                .normalize_or_zero();
            glow.push(
                at + out * size * 0.12,
                at + out * reach * dice.range(0.3, 1.),
                dice.range(0.0008, 0.002),
                dice.range(0.3, 0.9),
                dice.next(),
                Shape::Ray,
            );
        }
    }
    glow
}

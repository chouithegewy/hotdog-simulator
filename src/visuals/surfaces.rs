// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Surfaces with detail instead of flat colour. Every texture here is computed when the game starts:
//! speckled, scuffed stone for the counter, brushed steel, plaster, floor tiles, wood grain and a
//! general layer of grit, each with albedo, normal and roughness maps and a full mip chain, plus a
//! prefiltered environment cube per theme so metal and glossy things have a room to reflect.
//!
//! Nothing in the arena is spawned here. `upgrade` looks at meshes after the game has spawned them and
//! dresses their materials according to what they are: size, placement and the material they came with.
use std::collections::HashSet;
use std::f32::consts::{PI, TAU};

use bevy::{
    asset::RenderAssetUsages,
    camera::primitives::Aabb,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::NotShadowCaster,
    math::Affine2,
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};

use super::{ArenaBounds, Quality, Theme};

// ---------------------------------------------------------------------------------------------
// Noise that repeats, so every texture tiles without a seam.

fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA77)
        ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / 16_777_216.
}

fn smooth(t: f32) -> f32 {
    t * t * (3. - 2. * t)
}

/// Smooth noise on a lattice that wraps every `px` cells across and `py` cells down.
fn value(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (smooth(x - x0), smooth(y - y0));
    let corner = |dx: i32, dy: i32| {
        unit(hash(
            (x0 as i32 + dx).rem_euclid(px),
            (y0 as i32 + dy).rem_euclid(py),
            seed,
        ))
    };
    let top = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * fx;
    let bottom = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * fx;
    top + (bottom - top) * fy
}

/// Layered noise in 0..1 over the unit square; `fu` and `fv` are the coarsest cell counts.
fn fbm(u: f32, v: f32, fu: i32, fv: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut weight, mut total) = (0., 0.5, 0.);
    for octave in 0..octaves {
        let scale = 1 << octave;
        sum += weight
            * value(
                u * (fu * scale) as f32,
                v * (fv * scale) as f32,
                fu * scale,
                fv * scale,
                seed.wrapping_add(octave * 101),
            );
        total += weight;
        weight *= 0.5;
    }
    sum / total
}

/// Scattered grains: the distance to the nearest of one random point per cell, and that grain's hash.
fn grains(u: f32, v: f32, cells: i32, seed: u32) -> (f32, u32) {
    let (x, y) = (u * cells as f32, v * cells as f32);
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let mut best = (f32::MAX, 0);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let h = hash((cx + dx).rem_euclid(cells), (cy + dy).rem_euclid(cells), seed);
            let px = (cx + dx) as f32 + unit(h);
            let py = (cy + dy) as f32 + unit(hash(h as i32, 7, seed));
            let d = (px - x).hypot(py - y);
            if d < best.0 {
                best = (d, h);
            }
        }
    }
    best
}

// ---------------------------------------------------------------------------------------------
// Baking a surface into three images.

/// One point of a surface: linear colour, relief in arbitrary units, roughness, metalness, occlusion.
struct Texel {
    color: Vec3,
    height: f32,
    rough: f32,
    metal: f32,
    shade: f32,
}

/// Albedo, normal and occlusion-roughness-metal maps for one kind of surface.
#[derive(Clone)]
pub struct Maps {
    pub albedo: Handle<Image>,
    pub normal: Handle<Image>,
    pub orm: Handle<Image>,
}

fn encode(linear: f32) -> u8 {
    let c = linear.clamp(0., 1.);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1. / 2.4) - 0.055 };
    (s * 255. + 0.5) as u8
}

fn byte(x: f32) -> u8 {
    (x.clamp(0., 1.) * 255. + 0.5) as u8
}

/// Appends every smaller mip of a square RGBA image, averaging four texels into one.
fn with_mips(mut data: Vec<u8>, size: u32, linear_light: bool, renormalize: bool) -> (Vec<u8>, u32) {
    let decode: Vec<f32> = (0..256)
        .map(|i| {
            let s = i as f32 / 255.;
            if !linear_light {
                s
            } else if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        })
        .collect();
    let (mut start, mut side, mut levels) = (0, size as usize, 1);
    while side > 1 {
        let half = side / 2;
        let mut next = Vec::with_capacity(half * half * 4);
        for y in 0..half {
            for x in 0..half {
                let mut sum = [0.; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let at = start + ((y * 2 + dy) * side + x * 2 + dx) * 4;
                    for channel in 0..4 {
                        sum[channel] += decode[data[at + channel] as usize];
                    }
                }
                let mut mean = sum.map(|s| s / 4.);
                if renormalize {
                    let n = Vec3::new(mean[0] * 2. - 1., mean[1] * 2. - 1., mean[2] * 2. - 1.)
                        .normalize_or(Vec3::Z);
                    mean = [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, 1.];
                }
                for (channel, &value) in mean.iter().enumerate() {
                    next.push(if linear_light && channel < 3 { encode(value) } else { byte(value) });
                }
            }
        }
        start = data.len();
        data.extend(next);
        side = half;
        levels += 1;
    }
    (data, levels)
}

fn tiling_image(data: Vec<u8>, size: u32, format: TextureFormat, renormalize: bool) -> Image {
    let (data, levels) = with_mips(data, size, format == TextureFormat::Rgba8UnormSrgb, renormalize);
    let mut image = Image::new_uninit(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

/// Evaluates `surface` over a square and bakes albedo, a normal map from its relief, and roughness.
fn bake(
    images: &mut Assets<Image>,
    size: u32,
    relief: f32,
    surface: impl Fn(f32, f32) -> Texel,
) -> Maps {
    let n = size as usize;
    let mut albedo = Vec::with_capacity(n * n * 4);
    let mut orm = Vec::with_capacity(n * n * 4);
    let mut heights = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let texel = surface((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32);
            albedo.extend([encode(texel.color.x), encode(texel.color.y), encode(texel.color.z), 255]);
            orm.extend([byte(texel.shade), byte(texel.rough), byte(texel.metal), 255]);
            heights.push(texel.height);
        }
    }
    let mut normal = Vec::with_capacity(n * n * 4);
    // Relief is in texels at 512: the same surface baked larger keeps the same slopes.
    let slope = relief * n as f32 / 512.;
    for y in 0..n {
        for x in 0..n {
            let at = |x: usize, y: usize| heights[(y % n) * n + x % n];
            let dx = (at(x + 1, y) - at(x + n - 1, y)) * slope;
            let dy = (at(x, y + 1) - at(x, y + n - 1)) * slope;
            let v = Vec3::new(-dx, dy, 1.).normalize();
            normal.extend([byte(v.x * 0.5 + 0.5), byte(v.y * 0.5 + 0.5), byte(v.z * 0.5 + 0.5), 255]);
        }
    }
    Maps {
        albedo: images.add(tiling_image(albedo, size, TextureFormat::Rgba8UnormSrgb, false)),
        normal: images.add(tiling_image(normal, size, TextureFormat::Rgba8Unorm, true)),
        orm: images.add(tiling_image(orm, size, TextureFormat::Rgba8Unorm, false)),
    }
}

/// Fine scratches rasterised into a mask that wraps at the edges: 0 untouched, 1 the middle of a scratch.
fn scratches(size: usize, count: usize, seed: u32, along: Option<f32>) -> Vec<f32> {
    let mut mask = vec![0f32; size * size];
    for i in 0..count as i32 {
        let h = |k: i32| unit(hash(i, k, seed));
        let (mut x, mut y) = (h(0) * size as f32, h(1) * size as f32);
        let angle = along.map_or(h(2) * TAU, |a| a + (h(2) - 0.5) * 0.5);
        let bend = (h(3) - 0.5) * 0.004;
        let depth = 0.25 + 0.75 * h(4) * h(4);
        let steps = (size as f32 * (0.03 + 0.25 * h(5) * h(5))) as i32;
        let mut heading = angle;
        for step in 0..steps {
            let fade = (step as f32 / steps as f32 * PI).sin();
            let (xi, yi) = (x as usize % size, y as usize % size);
            let cell = &mut mask[yi * size + xi];
            *cell = cell.max(depth * fade);
            heading += bend;
            x = (x + heading.cos()).rem_euclid(size as f32);
            y = (y + heading.sin()).rem_euclid(size as f32);
        }
    }
    mask
}

/// Polished speckled stone: mottled dark base, pale, black and rusty mineral grains, glittering mica,
/// and the wear of a counter that is used, as dull scuffed patches and fine scratches.
fn stone(images: &mut Assets<Image>, size: u32) -> Maps {
    let lines = scratches(size as usize, (size as usize * size as usize) / 2600, 11, None);
    bake(images, size, 2.2, |u, v| {
        let mottle = fbm(u, v, 3, 3, 5, 1);
        let veins = fbm(u + 0.3 * fbm(u, v, 4, 4, 3, 8), v, 6, 6, 4, 2);
        let mut color = Vec3::new(0.150, 0.168, 0.160) * (0.62 + 0.75 * mottle);
        color = color.lerp(Vec3::new(0.30, 0.31, 0.28), smooth(((veins - 0.56) * 7.).clamp(0., 1.)) * 0.55);
        let mut rough = 0.20 + 0.10 * mottle;
        let mut metal = 0.;
        let mut height = 0.5;
        // Coarse mineral grains.
        let (d, id) = grains(u, v, 64, 3);
        let kind = unit(id);
        let radius = 0.16 + 0.34 * unit(hash(id as i32, 1, 3));
        if d < radius {
            let edge = smooth(((radius - d) / 0.09).clamp(0., 1.));
            let grain = if kind < 0.34 {
                Vec3::new(0.46, 0.44, 0.40)
            } else if kind < 0.55 {
                Vec3::new(0.022, 0.022, 0.026)
            } else if kind < 0.66 {
                Vec3::new(0.26, 0.17, 0.11)
            } else {
                color * 1.25
            };
            color = color.lerp(grain, edge * 0.92);
            rough += edge * if kind < 0.34 { 0.08 } else { -0.04 };
            height -= edge * 0.05;
        }
        // Fine grains, and now and then a flake of mica that catches the light.
        let (d, id) = grains(u, v, 190, 4);
        let kind = unit(id);
        if d < 0.30 && kind < 0.5 {
            let edge = smooth(((0.30 - d) / 0.12).clamp(0., 1.));
            if kind < 0.07 {
                color = color.lerp(Vec3::new(0.86, 0.84, 0.78), edge);
                rough = rough + (0.07 - rough) * edge;
                metal = edge * 0.85;
            } else if kind < 0.3 {
                color = color.lerp(Vec3::new(0.50, 0.49, 0.45), edge * 0.7);
            } else {
                color *= 1. - 0.6 * edge;
            }
        }
        // Wear: patches where the polish has gone, and scratches through it.
        let scuff = smooth(((fbm(u, v, 5, 5, 4, 5) - 0.50) * 5.).clamp(0., 1.));
        rough += scuff * 0.30;
        color = color.lerp(color * 1.12 + Vec3::splat(0.012), scuff * 0.5);
        let n = size as usize;
        let line = lines[((v * n as f32) as usize % n) * n + (u * n as f32) as usize % n];
        rough += line * 0.38;
        height -= line * 0.22;
        color = color.lerp(color * 1.3 + Vec3::splat(0.02), line * 0.6);
        Texel { color, height, rough: rough.clamp(0.05, 1.), metal, shade: 1. - line * 0.25 }
    })
}

/// Brushed steel: long fine grooves running one way, smudges where hands have been, and a few scratches
/// against the grain. Colour and metalness come from the material this is laid over.
fn steel(images: &mut Assets<Image>, size: u32) -> Maps {
    let lines = scratches(size as usize, (size as usize * size as usize) / 5200, 21, None);
    bake(images, size, 1.5, |u, v| {
        let fine = fbm(u, v, 2, 180, 3, 31);
        let broad = fbm(u, v, 1, 24, 3, 32);
        let groove = fine * 0.7 + broad * 0.3;
        let smudge = smooth(((fbm(u, v, 4, 4, 4, 33) - 0.52) * 5.).clamp(0., 1.));
        let n = size as usize;
        let line = lines[((v * n as f32) as usize % n) * n + (u * n as f32) as usize % n];
        let tone = 0.80 + 0.20 * groove - 0.03 * smudge + 0.12 * line;
        Texel {
            color: Vec3::splat(tone.clamp(0., 1.)),
            height: groove * 0.6 - line * 0.5,
            rough: (0.70 + 0.26 * groove + 0.04 * smudge + 0.2 * line).clamp(0., 1.),
            metal: 1.,
            shade: 0.9 + 0.1 * groove,
        }
    })
}

/// Painted plaster: an uneven roller finish, faint stains running down, small pits.
fn plaster(images: &mut Assets<Image>, size: u32) -> Maps {
    bake(images, size, 1.3, |u, v| {
        let orange_peel = fbm(u, v, 48, 48, 3, 41);
        let trowel = fbm(u, v, 5, 5, 4, 42);
        let stain = smooth(((fbm(u, v, 7, 2, 4, 43) - 0.55) * 4.).clamp(0., 1.));
        let (d, id) = grains(u, v, 40, 44);
        let pit = if unit(id) < 0.12 { smooth(((0.22 - d) / 0.1).clamp(0., 1.)) } else { 0. };
        let tone = 0.86 + 0.10 * trowel + 0.04 * orange_peel - 0.13 * stain - 0.12 * pit;
        Texel {
            color: Vec3::new(tone, tone * (1. - 0.03 * stain), tone * (1. - 0.08 * stain)),
            height: orange_peel * 0.35 + trowel * 0.5 - pit * 0.5,
            rough: 0.86 + 0.14 * orange_peel,
            metal: 0.,
            shade: 1. - 0.3 * pit,
        }
    })
}

/// Glazed floor tiles, two by two to a repeat, each a slightly different shade, in dirty grout.
fn tiles(images: &mut Assets<Image>, size: u32) -> Maps {
    bake(images, size, 2.6, |u, v| {
        let (tu, tv) = (u * 2., v * 2.);
        let (cu, cv) = (tu.fract(), tv.fract());
        let edge = cu.min(1. - cu).min(cv.min(1. - cv));
        let grout = 1. - smooth((edge / 0.018).clamp(0., 1.));
        let bevel = 1. - smooth((edge / 0.05).clamp(0., 1.));
        let id = hash(tu as i32 % 2, tv as i32 % 2, 51);
        let shade = 0.88 + 0.12 * unit(id);
        let cloud = fbm(u, v, 6, 6, 4, 52);
        let dirt = smooth(((fbm(u, v, 9, 9, 3, 53) - 0.45) * 3.).clamp(0., 1.));
        let glaze = Vec3::new(0.95, 0.93, 0.88) * shade * (0.86 + 0.14 * cloud);
        let color = glaze.lerp(Vec3::new(0.30, 0.27, 0.23) * (0.6 + 0.4 * dirt), grout);
        Texel {
            color: color * (1. - 0.18 * bevel * dirt),
            height: (1. - grout) * 0.5 - bevel * 0.12 + cloud * 0.03,
            rough: (0.30 + 0.12 * cloud + 0.60 * grout + 0.2 * bevel * dirt).clamp(0., 1.),
            metal: 0.,
            shade: 1. - 0.45 * grout,
        }
    })
}

/// Wood: boards with grain that wanders along their length, growth rings, a knot here and there.
fn wood(images: &mut Assets<Image>, size: u32) -> Maps {
    bake(images, size, 1.6, |u, v| {
        let boards = 4.;
        let board = (v * boards).floor();
        let across = (v * boards).fract();
        let seam = 1. - smooth((across.min(1. - across) / 0.02).clamp(0., 1.));
        let shift = unit(hash(board as i32, 0, 61));
        let wander = fbm(u + shift, v, 3, 8, 3, 62);
        let rings = ((across * 7. + wander * 5. + shift * 9.) * TAU).sin() * 0.5 + 0.5;
        let fibre = fbm(u + shift, v, 6, 220, 3, 63);
        let (d, id) = grains(u, v, 5, 64);
        let knot = if unit(id) < 0.25 { smooth(((0.16 - d) / 0.14).clamp(0., 1.)) } else { 0. };
        let tone = 0.62 + 0.20 * rings + 0.16 * fibre + 0.10 * shift - 0.38 * knot;
        Texel {
            color: Vec3::new(tone, tone * 0.93, tone * 0.84) * (1. - 0.6 * seam),
            height: fibre * 0.4 + rings * 0.15 - seam * 0.8 - knot * 0.2,
            rough: (0.66 + 0.24 * fibre - 0.12 * rings + 0.2 * seam).clamp(0., 1.),
            metal: 0.,
            shade: 1. - 0.5 * seam,
        }
    })
}

/// Grit for everything else: paint and plastic with dust in it, rubbed patches and hairline scratches.
fn grit(images: &mut Assets<Image>, size: u32) -> Maps {
    let lines = scratches(size as usize, (size as usize * size as usize) / 3600, 71, None);
    bake(images, size, 1.2, |u, v| {
        let dust = fbm(u, v, 8, 8, 5, 72);
        let rub = smooth(((fbm(u, v, 3, 3, 4, 73) - 0.5) * 4.).clamp(0., 1.));
        let speck = {
            let (d, id) = grains(u, v, 90, 74);
            if unit(id) < 0.2 { smooth(((0.25 - d) / 0.12).clamp(0., 1.)) } else { 0. }
        };
        let n = size as usize;
        let line = lines[((v * n as f32) as usize % n) * n + (u * n as f32) as usize % n];
        let tone = 0.88 + 0.12 * dust - 0.10 * rub - 0.30 * speck + 0.08 * line;
        Texel {
            color: Vec3::splat(tone.clamp(0., 1.)),
            height: dust * 0.3 - line * 0.5 - speck * 0.2,
            rough: (0.70 + 0.18 * dust + 0.12 * rub + 0.2 * line).clamp(0., 1.),
            metal: 1.,
            shade: 1. - 0.2 * speck,
        }
    })
}

/// Leaf tissue: a midrib, side veins and cells between them. Laid over the foliage's vertex colours.
fn leaf(images: &mut Assets<Image>, size: u32) -> Maps {
    bake(images, size, 2.0, |u, v| {
        let across = (u - 0.5).abs() * 2.;
        let midrib = 1. - smooth((across / 0.05).clamp(0., 1.));
        let slanted = v * 9. + across * 2.2;
        let vein = 1. - smooth(((slanted.fract() - 0.5).abs() / 0.07).clamp(0., 1.));
        let side = (1. - vein) * (1. - midrib);
        let cells = fbm(u, v, 40, 40, 3, 81);
        let blotch = fbm(u, v, 4, 4, 3, 82);
        let tone = 0.74 + 0.16 * cells + 0.10 * blotch;
        let rib = (midrib + (1. - side) * 0.5).clamp(0., 1.);
        Texel {
            color: Vec3::new(tone, tone, tone * 0.9).lerp(Vec3::new(1., 1., 0.82), rib * 0.55),
            height: rib * 0.7 + cells * 0.2,
            rough: (0.50 + 0.25 * cells - 0.15 * rib).clamp(0., 1.),
            metal: 0.,
            shade: 0.85 + 0.15 * rib,
        }
    })
}

// ---------------------------------------------------------------------------------------------
// The room as a pair of prefiltered cubes: what glossy things reflect and what fills the shadows.

/// The diffuse and specular cubes Bevy's image-based lighting expects.
#[derive(Clone)]
pub struct Environment {
    pub diffuse: Handle<Image>,
    pub specular: Handle<Image>,
}

fn half(value: f32) -> [u8; 2] {
    // IEEE 754 binary16 from binary32, for the range a radiance value can take.
    let bits = value.max(0.).min(65_000.).to_bits();
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x007f_ffff;
    let packed = if exponent <= 0 {
        if exponent < -10 { 0 } else { ((mantissa | 0x0080_0000) >> (14 - exponent)) as u16 }
    } else {
        ((exponent as u16) << 10) | (mantissa >> 13) as u16
    };
    packed.to_le_bytes()
}

fn face_direction(face: usize, s: f32, t: f32) -> Vec3 {
    // s and t run -1..1 across a face; the faces are +X, -X, +Y, -Y, +Z, -Z.
    match face {
        0 => Vec3::new(1., -t, -s),
        1 => Vec3::new(-1., -t, s),
        2 => Vec3::new(s, 1., t),
        3 => Vec3::new(s, -1., -t),
        4 => Vec3::new(s, -t, 1.),
        _ => Vec3::new(-s, -t, -1.),
    }
    .normalize()
}

/// What the room looks like from the counter in direction `d`, in units where a lit white wall is 1.
fn radiance(theme: Theme, d: Vec3, sun: Vec3) -> Vec3 {
    let up = d.y;
    let facing = d.dot(sun);
    // The disc of the sun or moon and the glow around it.
    let disc = smooth(((facing - 0.972) / 0.012).clamp(0., 1.));
    let glow = ((facing - 0.3) / 0.7).clamp(0., 1.).powi(6);
    match theme {
        Theme::Kitchen => {
            let ceiling = Vec3::new(1.0, 0.94, 0.82);
            let wall = Vec3::new(0.62, 0.61, 0.58);
            let floor = Vec3::new(0.26, 0.21, 0.15);
            let mut c = if up > 0. {
                wall.lerp(ceiling, smooth((up / 0.55).clamp(0., 1.)))
            } else {
                wall.lerp(floor, smooth((-up / 0.45).clamp(0., 1.)))
            };
            // Wooden cabinets behind the counter, the dim open room on the other side.
            c = c.lerp(Vec3::new(0.30, 0.17, 0.09), smooth(((d.z - 0.55) / 0.3).clamp(0., 1.)) * (1. - up.abs()) * 0.8);
            c *= 1. - 0.45 * smooth(((-d.z - 0.5) / 0.4).clamp(0., 1.)) * (1. - up.max(0.));
            c + Vec3::new(1.0, 0.80, 0.50) * (glow * 1.6) + Vec3::new(1.0, 0.86, 0.60) * (disc * 26.)
        }
        Theme::Night => {
            let zenith = Vec3::new(0.060, 0.095, 0.210);
            let low = Vec3::new(0.022, 0.032, 0.070);
            let mut c = low.lerp(zenith, smooth((up * 0.5 + 0.5).clamp(0., 1.)));
            // The string of warm bulbs under the cabinets, a band low in the +Z direction.
            let band = smooth(((d.z - 0.35) / 0.4).clamp(0., 1.)) * (1. - ((up - 0.28) / 0.16).abs()).clamp(0., 1.);
            let beads = 0.55 + 0.45 * (d.x.atan2(d.z) * 46.).sin();
            c += Vec3::new(1.0, 0.56, 0.22) * (band * beads * 0.8);
            c + Vec3::new(0.45, 0.60, 1.0) * (glow * 0.35) + Vec3::new(0.70, 0.82, 1.0) * (disc * 7.)
        }
        Theme::Jungle => {
            // Leaves overhead with sky showing through, trunks and undergrowth around, litter below.
            let (a, b) = (d.x.atan2(d.z) / TAU + 0.5, (d.y * 0.5 + 0.5).clamp(0., 1.));
            let leaves = fbm(a, b, 8, 6, 4, 91);
            let gaps = smooth(((fbm(a, b, 12, 9, 3, 92) - 0.56) * 7.).clamp(0., 1.));
            let canopy = Vec3::new(0.16, 0.36, 0.09) * (0.4 + 1.1 * leaves);
            let under = Vec3::new(0.07, 0.11, 0.04) * (0.5 + leaves);
            let litter = Vec3::new(0.10, 0.075, 0.035);
            let mut c = if up > 0. {
                under.lerp(canopy, smooth((up / 0.5).clamp(0., 1.)))
            } else {
                under.lerp(litter, smooth((-up / 0.5).clamp(0., 1.)))
            };
            c += Vec3::new(0.95, 1.0, 0.70) * (gaps * up.max(0.) * 2.6);
            c + Vec3::new(1.0, 0.85, 0.40) * (glow * 1.8) + Vec3::new(1.0, 0.92, 0.60) * (disc * 22.)
        }
    }
}

fn cube_image(data: Vec<u8>, size: u32, levels: u32) -> Image {
    let mut image = Image::new_uninit(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// Builds the two cubes for a theme: each mip of the specular cube is the room blurred for one more
/// step of roughness, and the diffuse cube is the room gathered over a whole hemisphere.
pub fn environment(images: &mut Assets<Image>, theme: Theme, sun: Vec3, size: u32) -> Environment {
    let levels = size.trailing_zeros() + 1;
    // One buffer per face, mips in order, because that is how a layered texture is uploaded.
    let mut faces: Vec<Vec<u8>> = vec![Vec::new(); 6];
    // A coarse copy of the sharp room to gather the diffuse cube from.
    const COARSE: usize = 8;
    let mut coarse = Vec::with_capacity(6 * COARSE * COARSE);
    for (face, bytes) in faces.iter_mut().enumerate() {
        for level in 0..levels {
            let side = size >> level;
            let rough = level as f32 / (levels - 1).max(1) as f32;
            let alpha = (rough * rough).max(0.002);
            let samples = if level == 0 { 4 } else { 40 };
            for y in 0..side {
                for x in 0..side {
                    let normal = face_direction(
                        face,
                        (x as f32 + 0.5) / side as f32 * 2. - 1.,
                        (y as f32 + 0.5) / side as f32 * 2. - 1.,
                    );
                    let mut sum = Vec3::ZERO;
                    let mut weight = 0.;
                    if level == 0 {
                        for i in 0..samples {
                            let (ox, oy) = ((i % 2) as f32 * 0.5 + 0.25, (i / 2) as f32 * 0.5 + 0.25);
                            sum += radiance(
                                theme,
                                face_direction(
                                    face,
                                    (x as f32 + ox) / side as f32 * 2. - 1.,
                                    (y as f32 + oy) / side as f32 * 2. - 1.,
                                ),
                                sun,
                            );
                            weight += 1.;
                        }
                    } else {
                        let (tangent, bitangent) = normal.any_orthonormal_pair();
                        for i in 0..samples {
                            // Hammersley points through the GGX distribution, reflected about the normal.
                            let e1 = (i as f32 + 0.5) / samples as f32;
                            let e2 = (i as u32).reverse_bits() as f32 / 4_294_967_296.;
                            let cos = ((1. - e1) / (1. + (alpha * alpha - 1.) * e1)).sqrt();
                            let sin = (1. - cos * cos).max(0.).sqrt();
                            let h = tangent * (sin * (e2 * TAU).cos())
                                + bitangent * (sin * (e2 * TAU).sin())
                                + normal * cos;
                            let light = 2. * normal.dot(h) * h - normal;
                            let facing = normal.dot(light);
                            if facing > 0. {
                                // The sun is far brighter than the room; held back here it cannot sparkle.
                                sum += radiance(theme, light, sun).min(Vec3::splat(12.)) * facing;
                                weight += facing;
                            }
                        }
                    }
                    let c = sum / weight.max(0.0001);
                    for channel in [c.x, c.y, c.z, 1.] {
                        bytes.extend(half(channel));
                    }
                    if side as usize == COARSE {
                        coarse.push((normal, c));
                    }
                }
            }
        }
    }
    let specular: Vec<u8> = faces.into_iter().flatten().collect();
    const DIFFUSE: u32 = 8;
    let mut diffuse = Vec::with_capacity((6 * DIFFUSE * DIFFUSE * 8) as usize);
    for face in 0..6 {
        for y in 0..DIFFUSE {
            for x in 0..DIFFUSE {
                let normal = face_direction(
                    face,
                    (x as f32 + 0.5) / DIFFUSE as f32 * 2. - 1.,
                    (y as f32 + 0.5) / DIFFUSE as f32 * 2. - 1.,
                );
                let (mut sum, mut weight) = (Vec3::ZERO, 0.);
                for &(direction, c) in &coarse {
                    // Texels near a face's corners cover less of the sphere than those at its middle.
                    let solid = direction.abs().max_element().powi(3);
                    let facing = normal.dot(direction).max(0.) * solid;
                    sum += c * facing;
                    weight += facing;
                }
                let c = sum / weight.max(0.0001);
                for channel in [c.x, c.y, c.z, 1.] {
                    diffuse.extend(half(channel));
                }
            }
        }
    }
    Environment {
        diffuse: images.add(cube_image(diffuse, DIFFUSE, 1)),
        specular: images.add(cube_image(specular, size, levels)),
    }
}

// ---------------------------------------------------------------------------------------------
// Small generated images for the screen and the sun.

/// Film grain: black and white specks of low, varying opacity, tiled over the screen and moved every frame.
pub fn grain_image(strength: f32) -> Image {
    const SIZE: u32 = 256;
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let a = unit(hash(x, y, 201));
            let b = unit(hash(x, y, 202));
            // Mid greys rather than black and white, so the grain neither lifts the blacks nor dulls the lights.
            let tone = if a < 0.5 { 30 } else { 150 };
            data.extend([tone, tone, tone, byte(b * b * strength)]);
        }
    }
    let mut image = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

/// A vignette for renderers without the post-processing one: clear in the middle, dark in the corners.
pub fn vignette_image(strength: f32) -> Image {
    const SIZE: u32 = 128;
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = (x as f32 + 0.5) / SIZE as f32 * 2. - 1.;
            let dy = (y as f32 + 0.5) / SIZE as f32 * 2. - 1.;
            let r = (dx * dx + dy * dy).sqrt() / std::f32::consts::SQRT_2;
            let dither = (unit(hash(x as i32, y as i32, 203)) - 0.5) / 255.;
            data.extend([0, 0, 0, byte(smooth(((r - 0.45) / 0.55).clamp(0., 1.)) * strength + dither)]);
        }
    }
    Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

// ---------------------------------------------------------------------------------------------
// Dressing what the game has spawned.

/// A surface that takes a different tint in each theme.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Counter,
    Wall,
    Floor,
    Ceiling,
    Wood,
    Painted,
}

impl Kind {
    fn tint(self, theme: Theme) -> LinearRgba {
        let (r, g, b) = match (theme, self) {
            (Theme::Jungle, Kind::Counter) => (0.66, 0.80, 0.64),
            (Theme::Jungle, Kind::Wall) => (0.30, 0.44, 0.24),
            (Theme::Jungle, Kind::Floor) => (0.36, 0.34, 0.20),
            (Theme::Jungle, Kind::Ceiling) => (0.24, 0.36, 0.18),
            (Theme::Jungle, Kind::Wood) => (0.70, 0.80, 0.50),
            (Theme::Jungle, Kind::Painted) => (0.75, 0.90, 0.70),
            (Theme::Night, Kind::Wall | Kind::Ceiling) => (0.80, 0.84, 0.95),
            (Theme::Kitchen, Kind::Counter) => (0.84, 0.84, 0.84),
            _ => (1., 1., 1.),
        };
        LinearRgba::rgb(r, g, b)
    }
}

/// Marks a mesh that has been looked at, so it is dressed once.
#[derive(Component)]
pub struct Surveyed;

/// The generated maps, and the record of what has been dressed with them.
#[derive(Resource)]
pub struct Library {
    stone: Maps,
    steel: Maps,
    plaster: Maps,
    tiles: Maps,
    wood: Maps,
    grit: Maps,
    pub leaf: Maps,
    materials: HashSet<AssetId<StandardMaterial>>,
    meshes: HashSet<AssetId<Mesh>>,
    tinted: Vec<(Kind, Handle<StandardMaterial>, LinearRgba)>,
    theme: Option<Theme>,
}

impl Library {
    pub fn new(images: &mut Assets<Image>, quality: Quality) -> Self {
        let (large, small) = match quality {
            Quality::Full => (1024, 512),
            Quality::Lite => (512, 512),
            Quality::Minimal => (64, 64),
        };
        Self {
            stone: stone(images, large),
            steel: steel(images, small),
            plaster: plaster(images, small),
            tiles: tiles(images, small),
            wood: wood(images, small),
            grit: grit(images, small),
            leaf: leaf(images, small.min(256)),
            materials: HashSet::new(),
            meshes: HashSet::new(),
            tinted: Vec::new(),
            theme: None,
        }
    }
}

fn lay(material: &mut StandardMaterial, maps: &Maps, albedo: bool) {
    if albedo {
        material.base_color_texture = Some(maps.albedo.clone());
    }
    material.normal_map_texture = Some(maps.normal.clone());
    material.metallic_roughness_texture = Some(maps.orm.clone());
    material.occlusion_texture = Some(maps.orm.clone());
}

/// Normal maps and anisotropy need tangents; Bevy's primitive meshes come without them.
fn give_tangents(library: &mut Library, meshes: &mut Assets<Mesh>, mesh: &Handle<Mesh>) {
    if !library.meshes.insert(mesh.id()) {
        return;
    }
    let ready = meshes.get(mesh).is_some_and(|m| {
        !m.contains_attribute(Mesh::ATTRIBUTE_TANGENT)
            && m.contains_attribute(Mesh::ATTRIBUTE_UV_0)
            && m.contains_attribute(Mesh::ATTRIBUTE_NORMAL)
    });
    if ready {
        if let Some(mut m) = meshes.get_mut(mesh) {
            let _ = m.generate_tangents();
        }
    }
}

/// The texture transform that pins a box's largest face to world metres, `tile` metres to a repeat,
/// so two boxes that share a plane show the same texels where they overlap.
fn anchored(centre: Vec3, size: Vec3, tile: f32) -> Affine2 {
    let (scale, corner) = if size.y <= size.x && size.y <= size.z {
        (Vec2::new(size.x, size.z), Vec2::new(centre.x - size.x / 2., centre.z - size.z / 2.))
    } else if size.z <= size.x {
        (Vec2::new(size.x, size.y), Vec2::new(centre.x - size.x / 2., centre.y - size.y / 2.))
    } else {
        (Vec2::new(size.y, size.z), Vec2::new(centre.y - size.y / 2., centre.z - size.z / 2.))
    };
    Affine2::from_scale_angle_translation(scale / tile, 0., corner / tile)
}

/// Looks at meshes the game has spawned since last time and dresses them.
pub fn upgrade(
    mut commands: Commands,
    library: Option<ResMut<Library>>,
    bounds: Res<ArenaBounds>,
    theme: Res<Theme>,
    quality: Res<Quality>,
    server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    // (A hotdog's bent parts, player_model/bend.rs, are dressed by the material they wear straight, which their bent
    // material follows.)
    fresh: Query<
        (Entity, &Mesh3d, AnyOf<(&MeshMaterial3d<StandardMaterial>, &crate::player_model::bend::Unbent)>, &GlobalTransform, &Aabb),
        Without<Surveyed>,
    >,
) {
    let Some(mut library) = library else {
        return;
    };
    for (entity, mesh, (straight, unbent), transform, aabb) in &fresh {
        let Some(handle) = straight.map(|m| m.0.clone()).or_else(|| unbent.map(|u| u.0.clone())).map(MeshMaterial3d) else {
            continue;
        };
        let handle = &handle;
        commands.entity(entity).try_insert(Surveyed);
        let (scale, rotation, _) = transform.to_scale_rotation_translation();
        let size = Vec3::from(aabb.half_extents) * 2. * scale.abs();
        let centre = transform.transform_point(Vec3::from(aabb.center));
        let square = rotation.angle_between(Quat::IDENTITY) < 0.01;
        // Whatever is at or above the ceiling would shade the whole room from a sun on the ceiling.
        if centre.y - size.y / 2. >= bounds.ceiling - 0.06 {
            commands.entity(entity).try_insert(NotShadowCaster);
        }
        if *quality == Quality::Minimal {
            continue;
        }
        let Some(original) = materials.get(&handle.0) else {
            continue;
        };
        if original.unlit || original.alpha_mode != AlphaMode::Opaque {
            continue;
        }
        let plain = original.base_color_texture.is_none();
        let color = original.base_color.to_linear();
        let top = centre.y + size.y / 2.;
        let boxy = square && meshes.get(&mesh.0).is_some_and(|m| m.count_vertices() == 24);
        let kind = if !plain || !boxy || original.metallic >= 0.5 || unbent.is_some() {
            None
        } else if size.y <= 0.2 && size.x.max(size.z) >= 2.5 && top <= 0.1 {
            Some((Kind::Floor, 1.2))
        } else if size.y <= 0.2 && size.x.max(size.z) >= 2.5 && top >= bounds.ceiling {
            Some((Kind::Ceiling, 1.6))
        } else if size.y >= 2. && size.x.min(size.z) <= 0.2 {
            Some((Kind::Wall, 1.6))
        } else if size.y <= 0.1 && (top - bounds.floor).abs() <= 0.02 && size.x * size.z >= 0.8 {
            Some((Kind::Counter, 0.42))
        } else if size.max_element() >= 0.5 && color.red > color.green * 1.25 && color.green > color.blue * 1.2 {
            Some((Kind::Wood, 0.9))
        } else if size.max_element() >= 0.5 {
            Some((Kind::Painted, 0.8))
        } else {
            None
        };
        if let Some((kind, tile)) = kind {
            // A material of its own, because the texture is pinned to this box's size and place.
            let mut dressed = original.clone();
            let maps = match kind {
                Kind::Floor => &library.tiles,
                Kind::Ceiling | Kind::Wall => &library.plaster,
                Kind::Counter => &library.stone,
                Kind::Wood => &library.wood,
                Kind::Painted => &library.grit,
            };
            lay(&mut dressed, maps, true);
            dressed.uv_transform = anchored(centre, size, tile);
            let base = match kind {
                // The stone map carries its own colours; tiles and plaster want a paler base than the
                // flat paint they replace, wood a slightly richer one.
                Kind::Counter => {
                    dressed.perceptual_roughness = 1.;
                    dressed.reflectance = 0.55;
                    dressed.clearcoat = 0.35;
                    dressed.clearcoat_perceptual_roughness = 0.22;
                    LinearRgba::WHITE
                }
                Kind::Floor => {
                    dressed.perceptual_roughness = 1.;
                    LinearRgba::rgb(0.62, 0.56, 0.46)
                }
                Kind::Wall | Kind::Ceiling => LinearRgba::rgb(0.72, 0.70, 0.63),
                Kind::Wood => {
                    dressed.perceptual_roughness = 0.9;
                    dressed.clearcoat = 0.25;
                    dressed.clearcoat_perceptual_roughness = 0.35;
                    LinearRgba::rgb(color.red * 2.1, color.green * 2.0, color.blue * 1.9)
                }
                Kind::Painted => {
                    dressed.metallic = 0.;
                    LinearRgba::rgb(color.red * 1.1, color.green * 1.1, color.blue * 1.1)
                }
            };
            let tint = kind.tint(*theme);
            dressed.base_color = LinearRgba::rgb(base.red * tint.red, base.green * tint.green, base.blue * tint.blue).into();
            let dressed = materials.add(dressed);
            library.tinted.push((kind, dressed.clone(), base));
            commands.entity(entity).try_insert(MeshMaterial3d(dressed));
            if kind == Kind::Wall {
                // The sun is a light inside the room; the walls around it must not shade the room from it.
                commands.entity(entity).try_insert(NotShadowCaster);
            }
            give_tangents(&mut library, &mut meshes, &mesh.0);
            continue;
        }
        // Everything smaller shares its material with others like it: dress the material once.
        if !library.materials.insert(handle.0.id()) {
            if materials.get(&handle.0).is_some_and(|m| m.normal_map_texture.is_some()) {
                give_tangents(&mut library, &mut meshes, &mesh.0);
            }
            continue;
        }
        let casing = original
            .base_color_texture
            .as_ref()
            .and_then(|texture| server.get_path(texture.id()))
            .is_some_and(|path| path.to_string().contains("sausage"));
        let luminance = 0.2126 * color.red + 0.7152 * color.green + 0.0722 * color.blue;
        let peak = color.red.max(color.green).max(color.blue);
        let saturation = if peak > 0. { 1. - color.red.min(color.green).min(color.blue) / peak } else { 0. };
        let (metallic, rough) = (original.metallic, original.perceptual_roughness);
        let glowing = original.emissive.red + original.emissive.green + original.emissive.blue > 0.5;
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        if casing {
            // The snap of a sausage skin: a thin glossy coat over the textured meat.
            material.clearcoat = 0.7;
            material.clearcoat_perceptual_roughness = 0.24;
        } else if !plain {
        } else if metallic >= 0.5 {
            lay(&mut material, &library.steel, true);
            // Not a mirror: brushed steel scatters enough light to look grey in the sun, not black.
            material.metallic = 0.8;
            material.base_color = LinearRgba::rgb(color.red * 1.3, color.green * 1.3, color.blue * 1.3).into();
            material.perceptual_roughness = (rough * 1.55).min(0.6);
            material.reflectance = 0.6;
            // Without the feature the shader has no tangent frame for it and shades the metal as NaN. Nor in the full set:
            // its ambient occlusion brings a normal prepass, and Bevy 0.19's main pass then loads the prepass's normal
            // and skips setting the anisotropy's tangents (`LOAD_PREPASS_NORMALS`, pbr_fragment.wgsl): NaN again, the
            // metal blown out white with black streaks.
            if super::ANISOTROPY && *quality != Quality::Full {
                material.anisotropy_strength = 0.7;
            }
            give_tangents(&mut library, &mut meshes, &mesh.0);
        } else if luminance < 0.02 && rough <= 0.45 {
            // Black plastic and glass: a hard clear coat.
            material.clearcoat = 0.9;
            material.clearcoat_perceptual_roughness = 0.07;
        } else if rough <= 0.33 && saturation >= 0.5 && !glowing {
            // Sauces: wet.
            material.clearcoat = 1.;
            material.clearcoat_perceptual_roughness = 0.10;
            material.reflectance = 0.6;
        } else if size.max_element() >= 0.12 && !glowing {
            lay(&mut material, &library.grit, true);
            material.metallic = 0.;
            material.perceptual_roughness = (rough * 1.2).min(1.);
            material.base_color = LinearRgba::rgb(color.red * 1.1, color.green * 1.1, color.blue * 1.1).into();
            give_tangents(&mut library, &mut meshes, &mesh.0);
        }
    }
    // The big surfaces change tint with the theme.
    if library.theme != Some(*theme) {
        library.theme = Some(*theme);
        for (kind, handle, base) in &library.tinted {
            if let Some(mut material) = materials.get_mut(handle) {
                let tint = kind.tint(*theme);
                material.base_color =
                    LinearRgba::rgb(base.red * tint.red, base.green * tint.green, base.blue * tint.blue).into();
            }
        }
    }
}

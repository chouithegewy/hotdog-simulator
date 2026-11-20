// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The seven guns as procedural models, shared by the hotdogs' hands and the first-person view.
//! Weapon 0 is no weapon: the hands are full (a loose object, the bacon) and it has no model.
//!
//! A weapon is modelled once in the gun frame: the muzzle points towards -z, the top is +y, +x is
//! the shooter's right, and the origin is where the hand grips. Sizes are metres at the scale a
//! hotdog carries them. Every weapon is merged into one mesh per material, so a held weapon is a
//! handful of draw calls however many pieces it is made of.
use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*,
    render::render_resource::PrimitiveTopology,
};
use std::f32::consts::{PI, TAU};

/// Surface finishes. Each is one material shared by every weapon that uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    Gunmetal,
    Steel,
    Brushed,
    Brass,
    Polymer,
    Rubber,
    Wood,
    Olive,
    Glass,
    White,
    Mustard,
    Ketchup,
    HotSauce,
    Relish,
    Label,
    Red,
    Blue,
    GlowGreen,
    GlowOrange,
    Syrup,
    Pancake,
    Butter,
    /// The mayonnaise bottle's soft white plastic, and the mayonnaise.
    Mayo,
}
impl Paint {
    const ALL: [Paint; 23] = [
        Paint::Gunmetal,
        Paint::Steel,
        Paint::Brushed,
        Paint::Brass,
        Paint::Polymer,
        Paint::Rubber,
        Paint::Wood,
        Paint::Olive,
        Paint::Glass,
        Paint::White,
        Paint::Mustard,
        Paint::Ketchup,
        Paint::HotSauce,
        Paint::Relish,
        Paint::Label,
        Paint::Red,
        Paint::Blue,
        Paint::GlowGreen,
        Paint::GlowOrange,
        Paint::Syrup,
        Paint::Pancake,
        Paint::Butter,
        Paint::Mayo,
    ];
    /// Metals keep some diffuse colour so they still read in a kitchen without reflections to
    /// mirror; plastics and sauces are glossy; the glass, the syrup and the two glows are the only
    /// special cases.
    fn material(self) -> StandardMaterial {
        let (color, metallic, roughness, reflectance) = match self {
            Paint::Gunmetal => ((0.15, 0.17, 0.20), 0.65, 0.38, 0.5),
            Paint::Steel => ((0.80, 0.83, 0.86), 0.75, 0.16, 0.6),
            Paint::Brushed => ((0.52, 0.55, 0.57), 0.65, 0.48, 0.5),
            Paint::Brass => ((0.85, 0.63, 0.22), 0.75, 0.28, 0.6),
            Paint::Polymer => ((0.045, 0.05, 0.055), 0., 0.72, 0.4),
            Paint::Rubber => ((0.02, 0.02, 0.022), 0., 0.95, 0.2),
            Paint::Wood => ((0.36, 0.19, 0.09), 0., 0.55, 0.4),
            Paint::Olive => ((0.30, 0.37, 0.17), 0.15, 0.62, 0.4),
            Paint::Glass => ((0.45, 0.72, 0.98), 0., 0.04, 0.9),
            Paint::White => ((0.94, 0.93, 0.88), 0., 0.32, 0.5),
            Paint::Mustard => ((0.99, 0.77, 0.06), 0., 0.2, 0.6),
            Paint::Ketchup => ((0.82, 0.05, 0.03), 0., 0.16, 0.65),
            Paint::HotSauce => ((0.95, 0.22, 0.03), 0., 0.16, 0.65),
            Paint::Relish => ((0.30, 0.55, 0.10), 0., 0.24, 0.6),
            Paint::Label => ((0.08, 0.60, 0.24), 0., 0.3, 0.5),
            Paint::Red => ((0.86, 0.07, 0.05), 0., 0.3, 0.5),
            Paint::Blue => ((0.10, 0.32, 0.85), 0., 0.3, 0.5),
            Paint::GlowGreen => ((0.35, 1., 0.45), 0., 0.4, 0.5),
            Paint::GlowOrange => ((1., 0.55, 0.1), 0., 0.4, 0.5),
            Paint::Syrup => ((0.55, 0.23, 0.02), 0., 0.06, 0.6),
            Paint::Pancake => ((0.80, 0.52, 0.22), 0., 0.78, 0.35),
            Paint::Butter => ((0.99, 0.90, 0.52), 0., 0.38, 0.45),
            Paint::Mayo => ((0.97, 0.94, 0.82), 0., 0.30, 0.5),
        };
        let base = StandardMaterial {
            base_color: Color::srgb(color.0, color.1, color.2),
            metallic,
            perceptual_roughness: roughness,
            reflectance,
            ..default()
        };
        match self {
            Paint::Glass => StandardMaterial {
                base_color: Color::srgba(color.0, color.1, color.2, 0.6),
                emissive: LinearRgba::rgb(0.03, 0.09, 0.18),
                alpha_mode: AlphaMode::Blend,
                ..base
            },
            Paint::GlowGreen => StandardMaterial {
                emissive: LinearRgba::rgb(0.4, 5., 0.9),
                ..base
            },
            Paint::GlowOrange => StandardMaterial {
                emissive: LinearRgba::rgb(7., 1.6, 0.1),
                ..base
            },
            // Syrup is a little see-through, and warm where light comes through it.
            Paint::Syrup => StandardMaterial {
                base_color: Color::srgba(color.0, color.1, color.2, 0.82),
                emissive: LinearRgba::rgb(0.10, 0.03, 0.),
                alpha_mode: AlphaMode::Blend,
                ..base
            },
            _ => base,
        }
    }
}

/// Triangles with a normal per corner.
#[derive(Clone, Default)]
pub struct Shape {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub indices: Vec<u32>,
}
impl Shape {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
    fn merge(&mut self, other: Shape) {
        let base = self.positions.len() as u32;
        self.positions.extend(other.positions);
        self.normals.extend(other.normals);
        self.indices.extend(other.indices.into_iter().map(|i| i + base));
    }
    fn placed(mut self, turn: Quat, at: Vec3) -> Self {
        for p in &mut self.positions {
            *p = turn * *p + at;
        }
        for n in &mut self.normals {
            *n = turn * *n;
        }
        self
    }
    /// Stretched or squashed about the origin along each axis (by positive amounts), its normals
    /// turned to stay square to the new surface: a round bottle made flat.
    fn scaled(mut self, by: Vec3) -> Self {
        for p in &mut self.positions {
            *p *= by;
        }
        for n in &mut self.normals {
            *n = (*n / by).normalize_or(*n);
        }
        self
    }
    fn mesh(&self) -> Mesh {
        let list = |v: &[Vec3]| v.iter().map(|p| p.to_array()).collect::<Vec<_>>();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, list(&self.positions))
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, list(&self.normals))
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0f32; 2]; self.positions.len()])
            .with_inserted_indices(Indices::U32(self.indices.clone()))
    }
}

/// A surface of revolution about the barrel axis. `profile` lists (distance ahead, radius) from the
/// back of the part to the front. A radius of 0 at an end closes it, and a profile that doubles
/// back on itself is seen from inside, which is how a bore or a bottle mouth is hollowed.
/// `smooth` rounds the shading from band to band; otherwise each band is its own facet.
fn lathe(profile: &[(f32, f32)], sides: usize, smooth: bool) -> Shape {
    let mut s = Shape::default();
    let band: Vec<Vec2> = profile
        .windows(2)
        .map(|w| Vec2::new(w[0].1 - w[1].1, w[1].0 - w[0].0).normalize_or(Vec2::Y))
        .collect();
    let rounded = |i: usize| {
        if smooth {
            (band[i.saturating_sub(1)] + band[i.min(band.len() - 1)]).normalize_or(Vec2::Y)
        } else {
            Vec2::ZERO
        }
    };
    let ring = |s: &mut Shape, (ahead, radius): (f32, f32), n: Vec2| {
        for j in 0..=sides {
            let (sin, cos) = (j as f32 / sides as f32 * TAU).sin_cos();
            s.positions.push(Vec3::new(radius * cos, radius * sin, -ahead));
            s.normals.push(Vec3::new(n.y * cos, n.y * sin, -n.x).normalize_or(Vec3::NEG_Z));
        }
    };
    let row = sides as u32 + 1;
    for i in 0..profile.len() - 1 {
        let base = s.positions.len() as u32;
        let (first, second) = if smooth { (rounded(i), rounded(i + 1)) } else { (band[i], band[i]) };
        ring(&mut s, profile[i], first);
        ring(&mut s, profile[i + 1], second);
        for j in 0..sides as u32 {
            let (a, b, c, d) = (base + j, base + j + 1, base + row + j, base + row + j + 1);
            s.indices.extend([a, c, b, b, c, d]);
        }
    }
    s
}
/// A lathe turned to run from `from` along `along` instead of straight ahead.
fn turned(profile: &[(f32, f32)], sides: usize, smooth: bool, from: Vec3, along: Vec3) -> Shape {
    lathe(profile, sides, smooth).placed(Quat::from_rotation_arc(Vec3::NEG_Z, along.normalize()), from)
}
/// A capped rod between two points: barrels, legs, hoses, wire guards.
fn rod(from: Vec3, to: Vec3, radius: f32, sides: usize) -> Shape {
    let length = from.distance(to);
    turned(&[(0., 0.), (0., radius), (length, radius), (length, 0.)], sides, false, from, to - from)
}
/// A bent rod through several points.
fn pipe(points: &[Vec3], radius: f32, sides: usize) -> Shape {
    let mut s = Shape::default();
    for w in points.windows(2) {
        s.merge(rod(w[0], w[1], radius, sides));
        s.merge(ball(w[1], radius, sides));
    }
    s
}
fn ball(at: Vec3, radius: f32, sides: usize) -> Shape {
    let profile: Vec<(f32, f32)> = (0..=6)
        .map(|i| {
            let t = i as f32 / 6. * PI;
            (-radius * t.cos(), radius * t.sin())
        })
        .collect();
    lathe(&profile, sides, true).placed(Quat::IDENTITY, at)
}
/// A flat-shaded solid between two matching outlines: a prism, or a wedge where they pinch.
fn solid(left: &[Vec3], right: &[Vec3]) -> Shape {
    let mut s = Shape::default();
    let count = left.len();
    let centre = (left.iter().sum::<Vec3>() + right.iter().sum::<Vec3>()) / (count * 2) as f32;
    let mut face = |a: Vec3, b: Vec3, c: Vec3| {
        let n = (b - a).cross(c - a).normalize_or_zero();
        if n == Vec3::ZERO {
            return;
        }
        // Wind every face so it looks away from the middle of the solid.
        let (b, c, n) = if n.dot((a + b + c) / 3. - centre) < 0. { (c, b, -n) } else { (b, c, n) };
        let base = s.positions.len() as u32;
        s.positions.extend([a, b, c]);
        s.normals.extend([n; 3]);
        s.indices.extend([base, base + 1, base + 2]);
    };
    for cap in [left, right] {
        let middle = cap.iter().sum::<Vec3>() / count as f32;
        for i in 0..count {
            face(middle, cap[i], cap[(i + 1) % count]);
        }
    }
    for i in 0..count {
        let j = (i + 1) % count;
        face(left[i], left[j], right[j]);
        face(left[i], right[j], right[i]);
    }
    s
}
/// A side-on outline of (ahead, up) points given a width: grips, stocks, sights.
fn slab(outline: &[(f32, f32)], width: f32) -> Shape {
    let side = |x: f32| outline.iter().map(|&(a, u)| Vec3::new(x, u, -a)).collect::<Vec<_>>();
    solid(&side(-width * 0.5), &side(width * 0.5))
}
/// A bar running ahead from `back` to `front` with its four long edges bevelled.
fn beam(back: f32, front: f32, low: f32, high: f32, width: f32, bevel: f32) -> Shape {
    let (w, c) = (width * 0.5, bevel);
    let cross = [(w - c, high), (w, high - c), (w, low + c), (w - c, low), (c - w, low), (-w, low + c), (-w, high - c), (c - w, high)];
    let end = |a: f32| cross.iter().map(|&(x, u)| Vec3::new(x, u, -a)).collect::<Vec<_>>();
    solid(&end(back), &end(front))
}
const UP: Vec3 = Vec3::Y;
const DOWN: Vec3 = Vec3::NEG_Y;
/// A point `ahead` of the grip and `up` from it, `side` to the right.
const fn at(ahead: f32, up: f32, side: f32) -> Vec3 {
    Vec3::new(side, up, -ahead)
}

/// Where each weapon ends, in the gun frame: a gun's nozzle, and with the hands full (0) just ahead
/// of the empty gun hand. Rounds are drawn leaving from here.
pub const MUZZLE: [Vec3; 8] = [
    at(0.006, 0., 0.),
    at(0.053, 0.004, 0.),
    at(0.118, 0., 0.),
    at(0.070, 0., 0.),
    at(0.082, 0.014, 0.),
    at(0.018, 0.0225, 0.),
    at(0.080, 0.0035, 0.),
    at(0.0566, 0., 0.),
];
/// The syrup rifle's optic, in the gun frame: the middle of its eyepiece (the back face of the ring round its glass),
/// on the optic's axis, which runs level along the bore 12.3 mm over it. Aimed in first person the eye looks down that
/// axis (`arena`, scope.rs), and aimed in third person the eyepiece is held to the eye (`player_model`).
pub const RIFLE_SIGHT: Vec3 = at(-0.0060, 0.0158, 0.);
/// The optic's glass as the eye sees it through the eyepiece (its radius, the ring's bore), how far in it sits behind
/// the eyepiece's face, and the outer radius of the chunky ring round it.
pub const RIFLE_LENS: f32 = 0.0040;
pub const RIFLE_GLASS: f32 = 0.0008;
pub const RIFLE_RING: f32 = 0.0060;
/// The ketchup sniper's eyepiece, in the gun frame: the middle of the scope's back lens.
pub const SNIPER_EYEPIECE: Vec3 = at(0.0025, 0.0175, 0.);
/// What the eye is put to, aimed down a gun's sights, in the gun frame: the syrup rifle's optic or the ketchup
/// sniper's eyepiece. The other guns have none.
pub fn sight(weapon: usize) -> Option<Vec3> {
    match weapon {
        2 => Some(SNIPER_EYEPIECE),
        6 => Some(RIFLE_SIGHT),
        _ => None,
    }
}

/// A weapon as it is built: its body, and its magazine (what a reload takes out and puts back: src/reload.rs), each
/// one merged shape per finish. What is added while `loading` goes in the magazine.
#[derive(Default)]
struct Model {
    body: Vec<(Paint, Shape)>,
    magazine: Vec<(Paint, Shape)>,
    loading: bool,
}
fn merge_into(list: &mut Vec<(Paint, Shape)>, paint: Paint, shape: Shape) {
    match list.iter_mut().find(|(p, _)| *p == paint) {
        Some((_, merged)) => merged.merge(shape),
        None => list.push((paint, shape)),
    }
}
impl Model {
    fn add(&mut self, paint: Paint, shape: Shape) {
        merge_into(if self.loading { &mut self.magazine } else { &mut self.body }, paint, shape);
    }
    /// A pistol grip raked back under the grip point, with ridges for the fingers, a trigger and
    /// a wire trigger guard. Most of the guns share it.
    fn grip(&mut self, front: f32, depth: f32) {
        use Paint::*;
        self.add(Rubber, slab(&[(front - 0.012, -0.003), (front, -0.003), (front - 0.004, -depth), (front - 0.017, -depth + 0.002)], 0.0095));
        for i in 0..4 {
            let u = -0.007 - i as f32 * (depth - 0.009) / 4.;
            let lean = (u + 0.003) * 0.26;
            self.add(Polymer, slab(&[(front - 0.0125 + lean, u), (front + 0.0005 + lean, u), (front + 0.0002 + lean, u - 0.0014), (front - 0.0128 + lean, u - 0.0014)], 0.0103));
        }
        self.add(Steel, slab(&[(front + 0.003, -0.003), (front + 0.0045, -0.003), (front + 0.0035, -0.010), (front + 0.002, -0.0095)], 0.002));
        self.add(Gunmetal, pipe(&[at(front + 0.001, -depth * 0.62, 0.), at(front + 0.011, -depth * 0.62, 0.), at(front + 0.013, -0.003, 0.)], 0.0007, 6));
    }
}

/// The parts of one weapon, one merged shape per finish. Full hands (0) hold no gun and have none.
pub fn model(weapon: usize) -> Vec<(Paint, Shape)> {
    let (mut body, magazine) = parts(weapon);
    for (paint, shape) in magazine {
        merge_into(&mut body, paint, shape);
    }
    body
}
/// One weapon's parts as `model` has them, its body and its magazine apart: the mustard pistol's squeeze bottle on its
/// slide, the ketchup sniper's bottle, the hot sauce thrower's bottle, the relish bazooka's glob in its mouth, the syrup
/// rifle's jug. The bug spray and the mayonnaise bottle are all magazine and have none of their own (a reload swaps the
/// whole of them).
pub fn parts(weapon: usize) -> (Vec<(Paint, Shape)>, Vec<(Paint, Shape)>) {
    use Paint::*;
    let mut m = Model::default();
    match weapon {
        // Hands full: the guns are put away.
        0 => {}
        // Mustard pistol: a sidearm fed by a ribbed squeeze bottle with a flip cap.
        1 => {
            m.add(Gunmetal, beam(-0.008, 0.036, -0.003, 0.0085, 0.0115, 0.0022));
            m.add(Brushed, beam(-0.006, 0.030, 0.0085, 0.0105, 0.006, 0.0008));
            for ahead in [-0.005, -0.002, 0.001] {
                m.add(Polymer, beam(ahead, ahead + 0.0015, -0.001, 0.0088, 0.0121, 0.002));
            }
            // Barrel, then the yellow nozzle tip with a bead of mustard on it.
            m.add(Steel, turned(&[(0., 0.), (0., 0.0036), (0.008, 0.0036), (0.008, 0.0042), (0.010, 0.0042), (0.010, 0.0022), (0.007, 0.0022), (0.007, 0.)], 14, false, at(0.036, 0.004, 0.), Vec3::NEG_Z));
            m.add(Mustard, turned(&[(0., 0.0030), (0.002, 0.0030), (0.007, 0.0011), (0.007, 0.)], 12, false, at(0.045, 0.004, 0.), Vec3::NEG_Z));
            m.add(Mustard, ball(at(0.0528, 0.0036, 0.), 0.0014, 8));
            // The squeeze bottle stands on the slide: ribs, shoulder, neck, flip cap and spout.
            m.loading = true;
            m.add(Mustard, turned(&[
                (0., 0.), (0., 0.0068), (0.003, 0.0074), (0.006, 0.0066), (0.009, 0.0074), (0.012, 0.0066), (0.015, 0.0074),
                (0.018, 0.0066), (0.0205, 0.0052), (0.0225, 0.0030), (0.0235, 0.0030),
            ], 16, true, at(0.010, 0.0105, 0.), UP));
            m.add(Label, turned(&[(0.0045, 0.0076), (0.0105, 0.0076)], 16, false, at(0.010, 0.0105, 0.), UP));
            m.add(Red, turned(&[(0.0235, 0.0036), (0.0265, 0.0036), (0.0265, 0.0015), (0.0300, 0.0008), (0.0300, 0.)], 12, false, at(0.010, 0.0105, 0.), UP));
            m.add(Red, slab(&[(0.0135, 0.0355), (0.0175, 0.0372), (0.0175, 0.0362), (0.0135, 0.0345)], 0.004));
            m.loading = false;
            // Sights: a notch at the back, a glowing bead at the front.
            m.add(Polymer, beam(-0.0075, -0.0045, 0.0105, 0.0128, 0.007, 0.0006));
            m.add(GlowGreen, ball(at(0.032, 0.0116, 0.), 0.0011, 8));
            m.grip(0.004, 0.024);
        }
        // Ketchup sniper: long barrel with a shroud and bipod, ringed scope with glass, bolt,
        // stock with a cheek rest, and a ketchup bottle for a magazine.
        2 => {
            m.add(Gunmetal, beam(-0.006, 0.042, -0.0055, 0.0075, 0.0105, 0.0022));
            m.add(Steel, turned(&[(0., 0.), (0., 0.0030), (0.066, 0.0024), (0.066, 0.)], 12, false, at(0.042, 0.001, 0.), Vec3::NEG_Z));
            // Ribbed shroud over the first third of the barrel.
            m.add(Polymer, turned(&[
                (0., 0.), (0., 0.0056), (0.004, 0.0056), (0.004, 0.0048), (0.007, 0.0048), (0.007, 0.0056), (0.011, 0.0056), (0.011, 0.0048),
                (0.014, 0.0048), (0.014, 0.0056), (0.018, 0.0056), (0.018, 0.0048), (0.021, 0.0048), (0.021, 0.0056), (0.026, 0.0052), (0.026, 0.),
            ], 14, false, at(0.042, 0.001, 0.), Vec3::NEG_Z));
            // Muzzle brake and the red nozzle.
            m.add(Gunmetal, turned(&[(0., 0.), (0., 0.0042), (0.003, 0.0042), (0.003, 0.0034), (0.005, 0.0034), (0.005, 0.0042), (0.008, 0.0042), (0.008, 0.)], 12, false, at(0.103, 0.001, 0.), Vec3::NEG_Z));
            m.add(Ketchup, turned(&[(0., 0.0026), (0.004, 0.0012), (0.004, 0.)], 10, false, at(0.111, 0.001, 0.), Vec3::NEG_Z));
            m.add(Ketchup, ball(at(0.1168, 0.0006, 0.), 0.0013, 8));
            // Bipod: a collar on the barrel and two splayed legs with rubber feet.
            m.add(Gunmetal, turned(&[(0., 0.), (0., 0.0040), (0.004, 0.0040), (0.004, 0.)], 12, false, at(0.074, 0.001, 0.), Vec3::NEG_Z));
            for side in [-1., 1.] {
                let foot = at(0.086, -0.022, side * 0.011);
                m.add(Brushed, rod(at(0.076, -0.002, side * 0.002), foot, 0.0009, 6));
                m.add(Rubber, ball(foot, 0.0016, 8));
            }
            // Scope: eyepiece, tube, objective bell, two rings on bases, turrets and glass.
            m.add(Gunmetal, turned(&[
                (0., 0.0034), (0., 0.0052), (0.007, 0.0052), (0.009, 0.0037), (0.029, 0.0037), (0.033, 0.0062), (0.042, 0.0062), (0.042, 0.0050), (0.040, 0.0050),
            ], 16, false, at(0.002, 0.0175, 0.), Vec3::NEG_Z));
            for ahead in [0.013, 0.028] {
                m.add(Brushed, turned(&[(0., 0.0037), (0., 0.0047), (0.003, 0.0047), (0.003, 0.0037)], 14, false, at(ahead, 0.0175, 0.), Vec3::NEG_Z));
                m.add(Brushed, beam(ahead, ahead + 0.003, 0.0075, 0.0135, 0.004, 0.0006));
            }
            m.add(Brushed, rod(at(0.022, 0.0205, 0.), at(0.022, 0.0245, 0.), 0.0019, 10));
            m.add(Brushed, rod(at(0.022, 0.0175, 0.003), at(0.022, 0.0175, 0.007), 0.0019, 10));
            m.add(Glass, turned(&[(0., 0.0050), (0., 0.)], 16, false, at(0.0425, 0.0175, 0.), Vec3::NEG_Z));
            m.add(Glass, turned(&[(0., 0.), (0., 0.0034)], 16, false, at(0.0025, 0.0175, 0.), Vec3::NEG_Z));
            // Bolt handle out of the right side.
            m.add(Steel, rod(at(0.008, 0.003, 0.004), at(0.006, 0.001, 0.011), 0.0011, 8));
            m.add(Steel, ball(at(0.006, 0.001, 0.0115), 0.0022, 10));
            // Stock, cheek rest and butt pad.
            m.add(Wood, slab(&[(-0.006, 0.0065), (-0.006, -0.0050), (-0.020, -0.0095), (-0.040, -0.0130), (-0.040, 0.0040), (-0.026, 0.0072)], 0.0085));
            m.add(Rubber, slab(&[(-0.034, 0.0058), (-0.016, 0.0074), (-0.016, 0.0102), (-0.034, 0.0088)], 0.0066));
            m.add(Rubber, beam(-0.0425, -0.040, -0.0138, 0.0048, 0.0092, 0.0012));
            // Ketchup bottle magazine, cap down, with its label.
            m.loading = true;
            m.add(Ketchup, turned(&[(0., 0.), (0., 0.0052), (0.012, 0.0058), (0.016, 0.0040), (0.0185, 0.0026)], 14, true, at(0.022, -0.0055, 0.), DOWN));
            m.add(White, turned(&[(0.0185, 0.0032), (0.0225, 0.0032), (0.0225, 0.)], 12, false, at(0.022, -0.0055, 0.), DOWN));
            m.add(White, turned(&[(0.004, 0.0058), (0.010, 0.0060)], 14, false, at(0.022, -0.0055, 0.), DOWN));
            m.loading = false;
            m.grip(0.004, 0.022);
        }
        // Hot sauce thrower: a sauce bottle slung under a vented heat shield, a hose between them,
        // a pilot nozzle with a live flame and a pressure gauge.
        3 => {
            m.add(Gunmetal, turned(&[(0., 0.), (0., 0.0050), (0.002, 0.0064), (0.034, 0.0064), (0.034, 0.)], 16, false, at(-0.008, 0., 0.), Vec3::NEG_Z));
            // Heat shield: bright ribs over dark slots.
            m.add(Rubber, turned(&[(0., 0.0044), (0.028, 0.0044)], 14, false, at(0.026, 0., 0.), Vec3::NEG_Z));
            for i in 0..6 {
                let ahead = 0.027 + i as f32 * 0.0046;
                m.add(Brushed, turned(&[(0., 0.0044), (0., 0.0060), (0.0028, 0.0060), (0.0028, 0.0044)], 14, false, at(ahead, 0., 0.), Vec3::NEG_Z));
            }
            // Flared nozzle with a dark bore, tipped in sauce.
            m.add(Gunmetal, turned(&[(0., 0.0036), (0.010, 0.0054), (0.010, 0.0042), (0.004, 0.0026), (0.004, 0.)], 14, false, at(0.055, 0., 0.), Vec3::NEG_Z));
            m.add(HotSauce, ball(at(0.0655, 0., 0.), 0.0030, 10));
            // Pilot nozzle under the barrel and its flame.
            m.add(Brass, pipe(&[at(0.030, -0.0066, 0.), at(0.060, -0.0072, 0.), at(0.064, -0.0056, 0.)], 0.0009, 6));
            m.add(GlowOrange, turned(&[(0., 0.), (0.0012, 0.0016), (0.0050, 0.)], 8, true, at(0.064, -0.0052, 0.), Vec3::new(0., 0.35, -1.)));
            // The bottle: body, shoulder, neck and green cap pointing forward, with a label.
            m.loading = true;
            m.add(HotSauce, turned(&[(0., 0.), (0., 0.0070), (0.002, 0.0078), (0.020, 0.0078), (0.026, 0.0034), (0.030, 0.0034)], 16, true, at(-0.010, -0.0155, 0.), Vec3::NEG_Z));
            m.add(Label, turned(&[(0.030, 0.0040), (0.034, 0.0040), (0.037, 0.0014), (0.037, 0.)], 12, false, at(-0.010, -0.0155, 0.), Vec3::NEG_Z));
            m.add(White, turned(&[(0.006, 0.0080), (0.016, 0.0080)], 16, false, at(-0.010, -0.0155, 0.), Vec3::NEG_Z));
            m.loading = false;
            // Clamps holding the bottle, and the hose from its cap up into the shield.
            for ahead in [-0.006, 0.008] {
                m.add(Brushed, turned(&[(0., 0.0079), (0., 0.0086), (0.002, 0.0086), (0.002, 0.0079)], 16, false, at(ahead, -0.0155, 0.), Vec3::NEG_Z));
                m.add(Brushed, beam(ahead, ahead + 0.002, -0.0085, -0.0055, 0.004, 0.0006));
            }
            m.add(Rubber, pipe(&[at(0.027, -0.0155, 0.), at(0.033, -0.0150, 0.), at(0.037, -0.0115, 0.), at(0.038, -0.0060, 0.)], 0.0013, 8));
            // Pressure gauge: brass body, white face, a glowing needle.
            m.add(Brass, rod(at(0.006, 0.0060, 0.), at(0.006, 0.0092, 0.), 0.0040, 14));
            m.add(White, rod(at(0.006, 0.0092, 0.), at(0.006, 0.0096, 0.), 0.0033, 14));
            m.add(GlowGreen, slab(&[(0.0052, 0.0096), (0.0068, 0.0096), (0.0084, 0.0100), (0.0052, 0.0100)], 0.0007));
            m.grip(0.002, 0.022);
        }
        // Relish bazooka: a banded olive tube flared at both ends, loaded with a chunky glob of
        // relish and mayonnaise, with a shoulder rest, two grips, a ring sight and a mayo tube.
        4 => {
            let tube = at(0., 0.014, 0.);
            m.add(Olive, turned(&[
                (-0.052, 0.0172), (-0.042, 0.0130), (0.060, 0.0130), (0.078, 0.0185), (0.079, 0.0185), (0.079, 0.0165), (0.062, 0.0112), (0.058, 0.0112), (0.058, 0.),
            ], 20, false, tube, Vec3::NEG_Z));
            // The blast cone at the back, seen from behind.
            m.add(Gunmetal, turned(&[(-0.046, 0.), (-0.046, 0.0112), (-0.050, 0.0150), (-0.052, 0.0172)], 20, false, tube, Vec3::NEG_Z));
            for ahead in [-0.036, -0.012, 0.020, 0.052] {
                m.add(Brushed, turned(&[(0., 0.0130), (0., 0.0141), (0.003, 0.0141), (0.003, 0.0130)], 20, false, tube + at(ahead, 0., 0.), Vec3::NEG_Z));
            }
            m.add(Brushed, turned(&[(0.0775, 0.0185), (0.0775, 0.0196), (0.0795, 0.0196), (0.0795, 0.0185)], 20, false, tube, Vec3::NEG_Z));
            // The load: relish with darker pickle chunks and a swirl of mayonnaise.
            m.loading = true;
            m.add(Relish, ball(tube + at(0.066, 0., 0.), 0.0118, 14));
            for (i, &(ahead, up, side)) in [(0.074, 0.006, 0.004), (0.076, -0.003, -0.005), (0.073, -0.006, 0.005), (0.077, 0.002, -0.002), (0.072, 0.004, -0.007)].iter().enumerate() {
                let turn = Quat::from_euler(EulerRot::XYZ, i as f32 * 0.9, i as f32 * 1.7, 0.4);
                m.add(Label, beam(-0.0022, 0.0022, -0.0018, 0.0018, 0.0034, 0.0005).placed(turn, tube + at(ahead, up, side)));
            }
            m.add(White, pipe(&[tube + at(0.0775, 0.0045, -0.003), tube + at(0.0795, 0.0010, 0.002), tube + at(0.0790, -0.0035, -0.001), tube + at(0.0770, -0.0050, 0.003)], 0.0017, 8));
            m.loading = false;
            // Shoulder rest, rear grip with trigger, and a foregrip.
            m.add(Rubber, slab(&[(-0.040, 0.002), (-0.018, 0.002), (-0.020, -0.006), (-0.028, -0.011), (-0.038, -0.012), (-0.043, -0.006)], 0.016));
            m.grip(0.004, 0.024);
            m.add(Gunmetal, beam(-0.010, 0.012, -0.0032, 0.0015, 0.008, 0.0012));
            m.add(Polymer, rod(at(0.038, 0.0015, 0.), at(0.036, -0.016, 0.), 0.0030, 10));
            m.add(Polymer, ball(at(0.036, -0.016, 0.), 0.0034, 10));
            // Ring sight on a post, with a glowing bead.
            m.add(Gunmetal, beam(0.019, 0.022, 0.027, 0.0335, 0.0022, 0.0004));
            m.add(Gunmetal, turned(&[(0., 0.0030), (0., 0.0040), (0.0016, 0.0040), (0.0016, 0.0030), (0., 0.0030)], 14, false, at(0.0197, 0.0370, 0.), Vec3::NEG_Z));
            m.add(GlowGreen, ball(at(0.0205, 0.0370, 0.), 0.0009, 8));
            // A tube of mayonnaise strapped to the right side.
            m.add(White, turned(&[(0., 0.0008), (0.003, 0.0036), (0.024, 0.0036), (0.027, 0.0020)], 12, true, tube + at(-0.008, -0.003, 0.0160), Vec3::NEG_Z));
            m.add(Blue, turned(&[(0.027, 0.0024), (0.031, 0.0024), (0.031, 0.)], 10, false, tube + at(-0.008, -0.003, 0.0160), Vec3::NEG_Z));
            m.add(Brushed, turned(&[(0., 0.0037), (0., 0.0042), (0.002, 0.0042), (0.002, 0.0037)], 12, false, tube + at(0.004, -0.003, 0.0160), Vec3::NEG_Z));
        }
        // Pancake-syrup battle rifle: an M16 for breakfast. A flat-top upper with a rail and a short,
        // chunky optic on it over the lower receiver, a ribbed round handguard behind a delta ring, the
        // triangular front sight, a birdcage flash hider oozing syrup, a fixed stock with a short stack of
        // pancakes and a pat of butter for a cheek rest, and for a straight magazine a flat jug of syrup
        // pushed up into the well.
        6 => {
            let bore = at(0., 0.0035, 0.);
            // Receivers and the flared magazine well.
            m.add(Gunmetal, beam(-0.0085, 0.0335, 0.0010, 0.0072, 0.0086, 0.0012));
            m.add(Gunmetal, beam(-0.0085, 0.0310, -0.0045, 0.0012, 0.0092, 0.0012));
            m.add(Gunmetal, beam(0.0165, 0.0315, -0.0085, -0.0035, 0.0102, 0.0010));
            // Flat top: a rail along the upper receiver, its cross slots between raised teeth.
            m.add(Gunmetal, beam(-0.0080, 0.0320, 0.0070, 0.0080, 0.0058, 0.0005));
            for i in 0..9 {
                let a = -0.0074 + i as f32 * 0.0043;
                m.add(Gunmetal, beam(a, a + 0.0024, 0.0079, 0.0086, 0.0062, 0.0003));
            }
            // The optic: a short, chunky housing on the rail. Towards the eye a thick ring of black rubber round the glass
            // (the glass RIFLE_LENS round, set RIFLE_GLASS into the ring's bore), then a waist, and a flared bell at
            // the front round the objective with a brass band about it.
            let optic = |ahead: f32| RIFLE_SIGHT + at(ahead, 0., 0.);
            m.add(Rubber, turned(&[
                (RIFLE_GLASS, RIFLE_LENS), (0., RIFLE_LENS), (0., RIFLE_RING - 0.0005), (0.0005, RIFLE_RING), (0.0042, RIFLE_RING), (0.0050, 0.0049),
            ], 48, false, RIFLE_SIGHT, Vec3::NEG_Z));
            m.add(Gunmetal, turned(&[
                (0.0050, 0.0049), (0.0150, 0.0049), (0.0185, 0.0058), (0.0226, 0.0058), (0.0230, 0.0054), (0.0230, 0.0046), (0.0222, 0.0044),
            ], 28, false, RIFLE_SIGHT, Vec3::NEG_Z));
            m.add(Brass, turned(&[(0.0194, 0.00588), (0.0210, 0.00588)], 28, false, RIFLE_SIGHT, Vec3::NEG_Z));
            m.add(Glass, turned(&[(RIFLE_GLASS, 0.), (RIFLE_GLASS, RIFLE_LENS)], 28, false, RIFLE_SIGHT, Vec3::NEG_Z));
            m.add(Glass, turned(&[(0.0222, 0.0044), (0.0222, 0.)], 28, false, RIFLE_SIGHT, Vec3::NEG_Z));
            // Lugs on the ring at ten and two o'clock, and a broader one on top.
            for (turn, width, high) in [(1.08f32, 0.0022, 0.0009), (-1.08, 0.0022, 0.0009), (0., 0.0032, 0.0011)] {
                m.add(Rubber, beam(0.0003, 0.0040, RIFLE_RING - 0.0008, RIFLE_RING + high, width, 0.0006).placed(Quat::from_rotation_z(turn), optic(0.)));
            }
            // The mount: a block from the ring's back face to the waist, down to a clamp round the rail, and two
            // brass thumb nuts on the right (ahead, out of the eye's way under the ring when it is aimed).
            m.add(Polymer, beam(-0.0058, 0.0092, 0.0084, 0.0112, 0.0046, 0.0007));
            m.add(Gunmetal, beam(-0.0050, 0.0084, 0.0066, 0.0090, 0.0072, 0.0006));
            for ahead in [0.0012, 0.0068] {
                m.add(Steel, rod(at(ahead, 0.0078, 0.0036), at(ahead, 0.0078, 0.0050), 0.0011, 10));
                m.add(Brass, turned(&[(0., 0.), (0., 0.0017), (0.0012, 0.0017), (0.0012, 0.)], 8, false, at(ahead, 0.0078, 0.0050), Vec3::X));
            }
            // Turrets on the waist, brass-capped: elevation on top, windage on the right, and on the left the
            // knob that lights the reticle, its amber telltale lit.
            let waist = optic(0.0100);
            m.add(Gunmetal, rod(waist + UP * 0.0040, waist + UP * 0.0070, 0.0023, 14));
            m.add(Brass, turned(&[(0., 0.0023), (0.0010, 0.0023), (0.0012, 0.0015), (0.0012, 0.)], 14, false, waist + UP * 0.0070, UP));
            m.add(Gunmetal, rod(waist + Vec3::X * 0.0040, waist + Vec3::X * 0.0066, 0.0020, 14));
            m.add(Brass, turned(&[(0., 0.0020), (0.0008, 0.0020), (0.0010, 0.0013), (0.0010, 0.)], 14, false, waist + Vec3::X * 0.0066, Vec3::X));
            m.add(Gunmetal, rod(waist - Vec3::X * 0.0040, waist - Vec3::X * 0.0058, 0.0016, 12));
            m.add(GlowOrange, ball(waist - Vec3::X * 0.0059, 0.0007, 8));
            // Charging handle at the back; forward assist and ejection port cover on the right,
            // selector on the left.
            m.add(Polymer, beam(-0.0105, -0.0060, 0.0064, 0.0076, 0.0066, 0.0005));
            m.add(Steel, rod(at(-0.0035, 0.0050, 0.0040), at(-0.0055, 0.0047, 0.0062), 0.0010, 8));
            m.add(Steel, ball(at(-0.0057, 0.0047, 0.0064), 0.0013, 8));
            m.add(Polymer, beam(0.0010, 0.0140, 0.0022, 0.0060, 0.0008, 0.0002).placed(Quat::IDENTITY, Vec3::X * 0.0044));
            m.add(Steel, ball(at(0.0020, -0.0010, -0.0047), 0.0009, 8));
            // Handguard: a round sleeve with ribs along it between the delta ring and the cap.
            m.add(Gunmetal, turned(&[(0., 0.), (0., 0.0069), (0.0028, 0.0069), (0.0028, 0.)], 18, false, bore + at(0.0315, 0., 0.), Vec3::NEG_Z));
            m.add(Polymer, turned(&[(0., 0.0060), (0.0240, 0.0060)], 16, false, bore + at(0.0340, 0., 0.), Vec3::NEG_Z));
            for i in 0..10 {
                m.add(Polymer, beam(0.0345, 0.0575, 0.0055, 0.0066, 0.0016, 0.0003).placed(Quat::from_rotation_z(i as f32 / 10. * TAU), bore));
            }
            m.add(Gunmetal, turned(&[(0., 0.), (0., 0.0063), (0.0018, 0.0063), (0.0018, 0.)], 16, false, bore + at(0.0577, 0., 0.), Vec3::NEG_Z));
            // Barrel, and the front sight: the block round the barrel, the tapering tower and its post.
            m.add(Steel, turned(&[(0., 0.), (0., 0.0019), (0.0145, 0.0019), (0.0145, 0.)], 12, false, bore + at(0.0595, 0., 0.), Vec3::NEG_Z));
            m.add(Gunmetal, turned(&[(0., 0.), (0., 0.0031), (0.0068, 0.0031), (0.0068, 0.)], 14, false, bore + at(0.0597, 0., 0.), Vec3::NEG_Z));
            m.add(Gunmetal, slab(&[(0.0599, 0.0050), (0.0663, 0.0050), (0.0641, 0.0128), (0.0621, 0.0128)], 0.0028));
            m.add(Steel, rod(at(0.0631, 0.0126, 0.), at(0.0631, 0.0150, 0.), 0.00045, 6));
            // Birdcage flash hider with its mouth full of syrup, and a drip hanging from its lip.
            m.add(Gunmetal, turned(&[
                (0., 0.), (0., 0.0025), (0.0012, 0.0025), (0.0012, 0.0020), (0.0024, 0.0020), (0.0024, 0.0026), (0.0036, 0.0026),
                (0.0036, 0.0020), (0.0046, 0.0020), (0.0046, 0.0026), (0.0056, 0.0026), (0.0056, 0.0016), (0.0040, 0.0016), (0.0040, 0.),
            ], 14, false, bore + at(0.0740, 0., 0.), Vec3::NEG_Z));
            m.add(Syrup, ball(bore + at(0.0794, 0., 0.), 0.0015, 10));
            m.add(Syrup, turned(&[(0., 0.), (0., 0.0007), (0.0016, 0.0005), (0.0030, 0.0009), (0.0040, 0.0012), (0.0048, 0.0009), (0.0053, 0.)], 10, true, bore + at(0.0790, -0.0019, 0.), DOWN));
            // Fixed stock and butt plate. On top, the cheek rest: a short stack of pancakes glazed
            // with syrup that runs down the left side, and a pat of butter.
            m.add(Polymer, slab(&[(-0.0080, 0.0066), (-0.0080, -0.0030), (-0.0220, -0.0062), (-0.0360, -0.0088), (-0.0360, 0.0062)], 0.0080));
            m.add(Rubber, beam(-0.0378, -0.0358, -0.0091, 0.0065, 0.0086, 0.0010));
            let stack = at(-0.0240, 0.0062, 0.);
            for (i, (r, nudge)) in [(0.0054, 0.0003), (0.0051, -0.0002), (0.0048, 0.0002)].into_iter().enumerate() {
                let cake = [(0., 0.), (0., r - 0.0005), (0.0002, r - 0.0001), (0.0006, r), (0.0010, r - 0.0001), (0.0012, r - 0.0005), (0.0012, 0.)];
                m.add(Pancake, turned(&cake, 16, true, stack + at(nudge, i as f32 * 0.0012, -nudge), UP));
            }
            let top = stack + UP * 0.0036;
            m.add(Syrup, turned(&[(0., 0.), (0., 0.0044), (0.0003, 0.0041), (0.0005, 0.0032), (0.0005, 0.)], 16, true, top - UP * 0.0001, UP));
            m.add(Syrup, pipe(&[top + at(0.0006, 0.0002, -0.0040), top + at(0.0008, -0.0004, -0.0050), top + at(0.0008, -0.0022, -0.0053)], 0.0005, 6));
            m.add(Syrup, ball(top + at(0.0008, -0.0025, -0.0053), 0.0008, 8));
            m.add(Butter, beam(-0.0020, 0.0020, 0., 0.0014, 0.0040, 0.0004).placed(Quat::from_rotation_y(0.4), top + UP * 0.0003));
            // The magazine: a jug of syrup, flattened to a magazine's shape and raked a little
            // forward, cap down, with a cream label, a gold cap and the small loop handle syrup jugs
            // have at the neck.
            m.loading = true;
            let down = Vec3::new(0., -1., -0.08).normalize();
            let jug = |shape: Shape| shape.scaled(Vec3::new(0.68, 1., 1.)).placed(Quat::IDENTITY, at(0.0245, -0.0050, 0.));
            m.add(Syrup, jug(turned(&[(0., 0.), (0., 0.0056), (0.0020, 0.0058), (0.0150, 0.0058), (0.0176, 0.0046), (0.0196, 0.0026), (0.0206, 0.0026)], 16, true, Vec3::ZERO, down)));
            m.add(White, jug(turned(&[(0.0055, 0.0060), (0.0125, 0.0060)], 16, false, Vec3::ZERO, down)));
            m.add(Brass, jug(turned(&[(0.0204, 0.0030), (0.0236, 0.0030), (0.0240, 0.0024), (0.0240, 0.)], 12, false, Vec3::ZERO, down)));
            m.add(Syrup, jug(pipe(&[down * 0.0128 + Vec3::NEG_Z * 0.0056, down * 0.0136 + Vec3::NEG_Z * 0.0086, down * 0.0168 + Vec3::NEG_Z * 0.0082, down * 0.0182 + Vec3::NEG_Z * 0.0042], 0.0007, 6)));
            m.loading = false;
            m.grip(0.004, 0.021);
        }
        // Mayonnaise: just a soft squeeze bottle, held in the fist and squeezed, its pointed cap ahead. A
        // rounded body with a dimpled base, a shoulder, a screw cap with a grip ring, and a tall tapering
        // nozzle with a bead of mayonnaise at its tip; a plain pale label round its middle, and the dent
        // where the hand squeezes.
        7 => {
            let body = at(-0.020, 0., 0.);
            m.add(Mayo, turned(&[
                (0.0015, 0.), (0., 0.0048), (0.0004, 0.0082), (0.0022, 0.0098), (0.0060, 0.0104), (0.0180, 0.0101), (0.0215, 0.0097), (0.0250, 0.0101),
                (0.0350, 0.0104), (0.0395, 0.0098), (0.0430, 0.0082), (0.0455, 0.0062), (0.0468, 0.0058),
            ], 20, true, body, Vec3::NEG_Z));
            m.add(Butter, turned(&[(0.0072, 0.01045), (0.0168, 0.01045)], 20, false, body, Vec3::NEG_Z));
            m.add(Red, turned(&[(0.0076, 0.01050), (0.0084, 0.01050)], 20, false, body, Vec3::NEG_Z));
            m.add(White, turned(&[
                (0.0462, 0.), (0.0462, 0.0066), (0.0470, 0.0070), (0.0518, 0.0070), (0.0524, 0.0064), (0.0530, 0.0052), (0.0600, 0.0026),
                (0.0740, 0.0012), (0.0752, 0.0008), (0.0752, 0.),
            ], 16, true, body, Vec3::NEG_Z));
            for i in 0..12 {
                m.add(White, beam(0.0472 - 0.020, 0.0516 - 0.020, 0.0066, 0.0074, 0.0013, 0.0003).placed(Quat::from_rotation_z(i as f32 / 12. * TAU), Vec3::ZERO));
            }
            m.add(Mayo, ball(at(0.0558, 0., 0.), 0.0011, 8));
        }
        // Bug spray: an aerosol can with a rolled bottom rim, label band, domed top, valve and a
        // nozzle button that points forward.
        _ => {
            let can = at(0.008, -0.017, 0.);
            m.add(Brushed, turned(&[
                (0.0012, 0.), (0., 0.0070), (0., 0.0092), (0.0012, 0.0092), (0.0016, 0.0088), (0.0300, 0.0088), (0.0306, 0.0092), (0.0316, 0.0092),
                (0.0340, 0.0078), (0.0358, 0.0056), (0.0366, 0.0040), (0.0372, 0.0040), (0.0372, 0.),
            ], 20, true, can, UP));
            m.add(Label, turned(&[(0.0050, 0.0090), (0.0270, 0.0090)], 20, false, can, UP));
            m.add(Mustard, turned(&[(0.0085, 0.00915), (0.0115, 0.00915)], 20, false, can, UP));
            m.add(Red, turned(&[(0.0225, 0.00915), (0.0245, 0.00915)], 20, false, can, UP));
            // A crossed-out bug on the front of the label.
            m.add(White, turned(&[(0., 0.), (0., 0.0046), (0.0006, 0.0046), (0.0006, 0.)], 14, false, can + at(0.0088, 0.0170, 0.), Vec3::NEG_Z));
            m.add(Polymer, ball(can + at(0.0096, 0.0170, 0.), 0.0019, 8));
            m.add(Red, beam(-0.0040, 0.0040, -0.0005, 0.0005, 0.0008, 0.0002).placed(Quat::from_rotation_z(0.8) * Quat::from_rotation_y(PI * 0.5), can + at(0.0098, 0.0170, 0.)));
            // Valve stem, the white button and its spout.
            m.add(Steel, rod(can + at(0., 0.0372, 0.), can + at(0., 0.0385, 0.), 0.0012, 8));
            m.add(White, turned(&[(0.0385, 0.), (0.0385, 0.0036), (0.0420, 0.0036), (0.0428, 0.0028), (0.0428, 0.)], 14, false, can, UP));
            m.add(White, rod(can + at(0.0020, 0.0398, 0.), can + at(0.0090, 0.0398, 0.), 0.0012, 8));
            m.add(Rubber, rod(can + at(0.0088, 0.0398, 0.), can + at(0.0096, 0.0398, 0.), 0.0006, 8));
        }
    }
    (m.body, m.magazine)
}

/// The magazine of a weapon hung on a hand or the view (`Kit::attach`): a child of its root, in the gun frame, which a
/// reload moves (src/reload.rs). A weapon with no magazine of its own has none.
#[derive(Component)]
pub struct Magazine;

/// The weapons' meshes and materials, built once and shared by everyone who holds or sees one.
#[derive(Clone)]
pub struct Kit {
    materials: Vec<Handle<StandardMaterial>>,
    /// Each weapon's body, and its magazine (`parts`).
    meshes: [Vec<(Paint, Handle<Mesh>)>; 8],
    magazines: [Vec<(Paint, Handle<Mesh>)>; 8],
}
impl Kit {
    pub fn new(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>) -> Self {
        let built = [0, 1, 2, 3, 4, 5, 6, 7].map(parts);
        let mut add = |list: &Vec<(Paint, Shape)>| list.iter().map(|(paint, shape)| (*paint, meshes.add(shape.mesh()))).collect::<Vec<_>>();
        Self {
            materials: Paint::ALL.iter().map(|paint| mats.add(paint.material())).collect(),
            meshes: std::array::from_fn(|w| add(&built[w].0)),
            magazines: std::array::from_fn(|w| add(&built[w].1)),
        }
    }
    /// Hangs one weapon on `parent`: a root placed by `placing`, with one child per material, and its magazine (one
    /// child per material of its own) under a `Magazine` child, in the gun frame.
    /// The root starts with the visibility given, so a hand can carry all eight and show one.
    pub fn attach(
        &self,
        world: &mut World,
        parent: Entity,
        weapon: usize,
        placing: Transform,
        visibility: Visibility,
    ) -> Entity {
        let root = world.spawn((placing, visibility)).id();
        world.entity_mut(parent).add_child(root);
        for (paint, mesh) in &self.meshes[weapon.min(7)] {
            let piece = world
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(self.materials[*paint as usize].clone()),
                    Transform::default(),
                ))
                .id();
            world.entity_mut(root).add_child(piece);
        }
        if !self.magazines[weapon.min(7)].is_empty() {
            let magazine = world.spawn((Name::new("Magazine"), Magazine, Transform::default(), Visibility::Inherited)).id();
            world.entity_mut(root).add_child(magazine);
            for (paint, mesh) in &self.magazines[weapon.min(7)] {
                let piece = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(self.materials[*paint as usize].clone()), Transform::default())).id();
                world.entity_mut(magazine).add_child(piece);
            }
        }
        root
    }
}
/// Gun frame to the hotdog's gun hand, where -y runs on from the forearm and -z is the top.
pub fn in_hand() -> Transform {
    Transform::from_rotation(Quat::from_rotation_x(-PI * 0.5))
}
/// Gun frame to the first-person view, which already looks down -z with +y up: the same weapon,
/// a little larger. The long ones are held further out so the stocks and the blast cone are not
/// in the player's face.
pub fn in_view(weapon: usize) -> Transform {
    match weapon {
        2 => Transform::from_xyz(0., 0., -0.03),
        4 => Transform::from_xyz(0.004, -0.006, -0.035).with_scale(Vec3::splat(0.85)),
        6 => Transform::from_xyz(0., 0., -0.026),
        _ => Transform::from_scale(Vec3::splat(1.15)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Lowest and highest corner of everything a weapon is made of.
    fn bounds(weapon: usize) -> (Vec3, Vec3) {
        model(weapon)
            .iter()
            .flat_map(|(_, shape)| shape.positions.iter().copied())
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)))
    }
    #[test]
    fn every_weapon_is_a_detailed_model_in_a_few_meshes() {
        assert!(model(0).is_empty(), "full hands hold no gun: there is no knife");
        for weapon in 1..8 {
            let parts = model(weapon);
            let triangles: usize = parts.iter().map(|(_, s)| s.triangles()).sum();
            println!("weapon {weapon}: {} meshes, {triangles} triangles", parts.len());
            assert!(parts.len() >= 3 && parts.len() <= 12, "weapon {weapon}: {} meshes", parts.len());
            assert!(triangles > 200 && triangles < 9000, "weapon {weapon}: {triangles} triangles");
            for (paint, shape) in &parts {
                assert_eq!(shape.positions.len(), shape.normals.len());
                assert!(shape.indices.iter().all(|i| (*i as usize) < shape.positions.len()));
                assert!(shape.normals.iter().all(|n| (n.length() - 1.).abs() < 1e-3), "{paint:?} has a bad normal");
                assert!(shape.positions.iter().all(|p| p.is_finite()));
                assert!(parts.iter().filter(|(p, _)| p == paint).count() == 1, "{paint:?} is merged once");
            }
        }
    }
    #[test]
    fn every_gun_but_the_can_and_the_bottle_has_a_magazine_of_its_own_which_with_its_body_is_the_whole_model() {
        for weapon in 1..8 {
            let (body, magazine) = parts(weapon);
            let count = |list: &[(Paint, Shape)]| list.iter().map(|(_, s)| s.triangles()).sum::<usize>();
            assert_eq!(count(&body) + count(&magazine), count(&model(weapon)), "weapon {weapon}");
            assert_eq!(magazine.is_empty(), matches!(weapon, 5 | 7), "weapon {weapon}");
            // The magazine is a small part of the gun.
            if !magazine.is_empty() {
                assert!(count(&magazine) * 2 < count(&body), "weapon {weapon}");
            }
        }
        // The ketchup sniper's bottle hangs below the bore, the pistol's stands on its slide.
        let lowest = |w: usize| parts(w).1.iter().flat_map(|(_, s)| s.positions.iter().map(|p| p.y)).fold(f32::MAX, f32::min);
        let highest = |w: usize| parts(w).1.iter().flat_map(|(_, s)| s.positions.iter().map(|p| p.y)).fold(f32::MIN, f32::max);
        assert!(highest(2) < 0. && lowest(1) > 0.);
    }
    #[test]
    fn faces_look_outwards() {
        // A closed lathe and a prism: every triangle's winding agrees with its normals, and the
        // normals point away from the middle.
        for shape in [
            lathe(&[(0., 0.), (0., 0.01), (0.03, 0.01), (0.03, 0.)], 12, false),
            lathe(&[(0., 0.), (0., 0.01), (0.03, 0.01), (0.03, 0.)], 12, true).scaled(Vec3::new(0.68, 1., 1.)),
            beam(0., 0.03, -0.01, 0.01, 0.02, 0.002),
            ball(Vec3::ZERO + at(0.015, 0., 0.), 0.01, 10),
        ] {
            let centre = at(0.015, 0., 0.);
            for t in shape.indices.chunks(3) {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| shape.positions[i as usize]);
                let face = (b - a).cross(c - a);
                // Slivers at a pole have no area to speak of.
                if face.length() < 1e-9 {
                    continue;
                }
                assert!(face.dot(shape.normals[t[0] as usize]) > 0., "winding disagrees with the normal: {a} {b} {c} normal {}", shape.normals[t[0] as usize]);
                assert!(face.dot((a + b + c) / 3. - centre) > 0., "a face looks inwards");
            }
        }
    }
    #[test]
    fn the_muzzle_is_the_front_of_the_weapon_and_the_grip_is_at_the_hand() {
        for weapon in 1..8 {
            let (lo, hi) = bounds(weapon);
            let muzzle = MUZZLE[weapon];
            // -z is ahead: the muzzle is at the very front, inside the model's outline.
            assert!(muzzle.z < -0.015 && (muzzle.z - lo.z).abs() < 0.004, "weapon {weapon}: muzzle {muzzle}, front {}", lo.z);
            assert!(muzzle.y > lo.y && muzzle.y < hi.y && muzzle.x.abs() < 1e-6);
            // The hand is inside the weapon: it has something to hold on to.
            assert!(lo.cmplt(Vec3::ZERO).all() && hi.cmpgt(Vec3::ZERO).all(), "weapon {weapon} is not held at its grip");
        }
    }
    #[test]
    fn silhouettes_are_distinct_and_hotdog_sized() {
        let size: Vec<Vec3> = (0..8).map(|w| { let (lo, hi) = bounds(w); (hi - lo).max(Vec3::ZERO) }).collect();
        for a in 1..8 {
            // Big enough to read on a 0.18 m hotdog, small enough not to dwarf it.
            assert!(size[a].z > 0.018 && size[a].z < 0.17, "weapon {a} is {} long", size[a].z);
            for b in a + 1..8 {
                assert!((size[a] - size[b]).abs().max_element() > 0.006, "{a} and {b} have the same outline");
            }
        }
        assert!((0..8).all(|w| size[2].z >= size[w].z), "the sniper is the longest");
        assert!((0..8).all(|w| size[4].x >= size[w].x), "the bazooka is the fattest");
        assert!(size[5].y > size[5].z, "the spray can stands upright");
        assert!(size[6].z > size[1].z + 0.04 && size[6].z < size[2].z - 0.03, "the syrup rifle is {} long", size[6].z);
        // Each gun shows the condiment it squirts.
        for (weapon, sauce) in [(1, Paint::Mustard), (2, Paint::Ketchup), (3, Paint::HotSauce), (4, Paint::Relish), (6, Paint::Syrup), (7, Paint::Mayo)] {
            assert!(model(weapon).iter().any(|(paint, _)| *paint == sauce));
        }
    }
    #[test]
    fn the_syrup_rifle_is_a_breakfast_m16() {
        let parts = model(6);
        let of = |paint: Paint| {
            let shape = &parts.iter().find(|(p, _)| *p == paint).unwrap_or_else(|| panic!("no {paint:?}")).1;
            shape.positions.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
        };
        let (lo, hi) = bounds(6);
        // A stock that reaches back to the shoulder, an optic on top, a magazine hanging below ahead of the
        // grip, and a barrel twice as long ahead of the hand as the stock is behind it.
        assert!(hi.z > 0.034 && -lo.z > 2. * hi.z, "stock {} behind the hand, muzzle {} ahead", hi.z, -lo.z);
        assert!(hi.y > 0.014 && lo.y < -0.025, "{lo} to {hi}");
        let syrup = &parts.iter().find(|(p, _)| *p == Paint::Syrup).unwrap().1.positions;
        assert!(syrup.iter().any(|p| p.y < -0.022 && p.z < -0.018 && p.z > -0.035), "the magazine is a jug of syrup ahead of the grip");
        // Syrup at the nozzle, dripping, and on the pancakes on the stock, under the butter.
        assert!(syrup.iter().any(|p| p.z < -0.075 && p.y < MUZZLE[6].y - 0.004), "a drip hangs from the nozzle");
        let (cakes_lo, cakes_hi) = of(Paint::Pancake);
        let (butter_lo, _) = of(Paint::Butter);
        assert!(cakes_lo.z > 0.015 && cakes_hi.z < 0.032 && cakes_lo.y > 0.005, "pancakes {cakes_lo} to {cakes_hi}");
        assert!(butter_lo.y > cakes_hi.y - 0.0004 && butter_lo.z > cakes_lo.z && butter_lo.z < cakes_hi.z);
        // Slender beside the bazooka: no fatter than its optic's turrets and lugs.
        assert!(hi.x - lo.x < 0.016);
    }
    #[test]
    fn the_syrup_rifles_optic_looks_down_the_bore_line_through_its_glass_framed_by_its_ring() {
        // Möller-Trumbore: how far along `d` from `o` the ray meets the triangle, if it does.
        let hit = |o: Vec3, d: Vec3, [a, b, c]: [Vec3; 3]| -> Option<f32> {
            let (e1, e2) = (b - a, c - a);
            let p = d.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1e-12 {
                return None;
            }
            let t = o - a;
            let u = t.dot(p) / det;
            let q = t.cross(e1);
            let v = d.dot(q) / det;
            let s = e2.dot(q) / det;
            (u >= 0. && v >= 0. && u + v <= 1. && s > 0.).then_some(s)
        };
        let parts = model(6);
        let first = |eye: Vec3, d: Vec3| {
            parts
                .iter()
                .flat_map(|(paint, shape)| shape.indices.chunks(3).filter_map(move |t| hit(eye, d, [t[0], t[1], t[2]].map(|i| shape.positions[i as usize])).map(|s| (s, *paint))))
                .min_by(|a, b| a.0.total_cmp(&b.0))
        };
        assert_eq!(RIFLE_SIGHT.x, 0., "down the middle");
        assert!((RIFLE_SIGHT.y - MUZZLE[6].y - 0.0123).abs() < 1e-6, "12.3 mm over the bore");
        // From an eye a little behind the eyepiece on its axis, along the axis and off it inside the bore: the first
        // thing met is the glass, set into the ring.
        let eye = RIFLE_SIGHT + Vec3::Z * 0.0087;
        for off in [Vec2::ZERO, Vec2::new(0.003, 0.), Vec2::new(0., -0.003), Vec2::new(-0.0021, 0.0021)] {
            let d = (RIFLE_SIGHT + off.extend(0.) + Vec3::NEG_Z * RIFLE_GLASS - eye).normalize();
            let met = first(eye, d);
            assert!(met.is_some_and(|(s, paint)| paint == Paint::Glass && s > 0.0087), "looking {off} off the axis: {met:?}");
        }
        // Just outside the bore the ring is in the way, all the way round: it frames the glass.
        for k in 0..12 {
            let turn = (k as f32 + 0.5) / 12. * TAU;
            let off = Vec2::new(turn.cos(), turn.sin()) * (RIFLE_LENS + RIFLE_RING) * 0.5;
            let met = first(eye, (RIFLE_SIGHT + off.extend(0.) - eye).normalize());
            let face = (eye - RIFLE_SIGHT).length().hypot(off.length());
            assert!(met.is_some_and(|(s, paint)| paint == Paint::Rubber && (s - face).abs() < 1e-5), "at {turn} round: {met:?}, the face {face} away");
        }
    }
    #[test]
    fn the_mayonnaise_is_just_a_squeeze_bottle_held_in_the_fist_its_pointed_cap_ahead() {
        let (lo, hi) = bounds(7);
        // A bottle, round and about twice as long as the hand is wide, gripped about its middle, and no gun parts.
        assert!((hi.x - lo.x - (hi.y - lo.y)).abs() < 1e-3 && hi.x - lo.x > 0.018 && hi.x - lo.x < 0.024, "{lo} to {hi}");
        assert!(hi.z > 0.015 && -lo.z > 0.05, "{lo} to {hi}");
        let paints: Vec<Paint> = model(7).iter().map(|(p, _)| *p).collect();
        assert!(paints.iter().all(|p| matches!(p, Paint::Mayo | Paint::White | Paint::Butter | Paint::Red)), "{paints:?}");
        // The cap narrows to a point at the nozzle.
        let white = &model(7).into_iter().find(|(p, _)| *p == Paint::White).unwrap().1;
        let tip = white.positions.iter().filter(|p| p.z < MUZZLE[7].z + 0.004).map(|p| p.x.hypot(p.y)).fold(0f32, f32::max);
        assert!(tip < 0.0016, "the nozzle is {tip} wide at its tip");
    }
}

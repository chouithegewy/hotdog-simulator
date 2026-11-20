// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The shapes the people are made of, generated here: soft ellipsoids, lofted bodies, swept tubes with round ends,
//! pleated hats, cartoon hands and the mouth's poses. Every builder takes a [`Fine`] for how many facets it spends (the
//! detailed set, or the lean one for the Low and Lowest graphics levels).
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use std::f32::consts::{PI, TAU};

/// How finely a shape is made: rings from end to end and sides around.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fine {
    pub rings: u32,
    pub sides: u32,
}
impl Fine {
    pub const DETAILED: Fine = Fine { rings: 18, sides: 28 };
    pub const LEAN: Fine = Fine { rings: 8, sides: 12 };
    pub fn of(detailed: bool) -> Fine {
        if detailed { Fine::DETAILED } else { Fine::LEAN }
    }
    /// A share of this, for small parts (never under 4 rings and 6 sides).
    pub fn scaled(self, share: f32) -> Fine {
        Fine { rings: ((self.rings as f32 * share).round() as u32).max(4), sides: ((self.sides as f32 * share).round() as u32).max(6) }
    }
}

/// Vertices and triangles gathered from any number of parts, then made into one mesh.
#[derive(Default, Clone)]
pub struct Builder {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }
    #[cfg(test)]
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
    /// A grid of `rows + 1` by `cols + 1` samples of `sample(u, v)` (u and v from 0 to 1), its normals found from the
    /// surface's slopes and turned away from `inside(p)`, joined into triangles that face the way their normals do.
    /// With `wrap` the v = 0 and v = 1 columns are the same seam, and their normals agree.
    #[allow(clippy::too_many_arguments)]
    pub fn patch(
        &mut self,
        rows: u32,
        cols: u32,
        wrap: bool,
        sample: impl Fn(f32, f32) -> Vec3,
        inside: impl Fn(Vec3) -> Vec3,
        uv: impl Fn(f32, f32) -> Vec2,
        tint: impl Fn(f32, f32) -> [f32; 4],
    ) {
        let base = self.positions.len() as u32;
        let (du, dv) = (0.25 / rows as f32, 0.25 / cols as f32);
        for i in 0..=rows {
            let u = i as f32 / rows as f32;
            for j in 0..=cols {
                let v = j as f32 / cols as f32;
                let p = sample(u, v);
                let normal_at = |u: f32| {
                    let (u0, u1) = ((u - du).max(0.), (u + du).min(1.));
                    let (v0, v1) = if wrap { (v - dv, v + dv) } else { ((v - dv).max(0.), (v + dv).min(1.)) };
                    let wrapped = |v: f32| if wrap { v.rem_euclid(1.) } else { v };
                    (sample(u1, v) - sample(u0, v)).cross(sample(u, wrapped(v1)) - sample(u, wrapped(v0)))
                };
                let mut n = normal_at(u);
                // At a pole the ring has no length: take the normal from just beside it.
                if n.length_squared() < 1e-14 {
                    n = normal_at(u.clamp(0.03, 0.97));
                }
                let mut n = n.normalize_or(Vec3::Y);
                let out = p - inside(p);
                if out.length_squared() > 1e-14 && n.dot(out) < 0. {
                    n = -n;
                }
                self.positions.push(p);
                self.normals.push(n);
                self.uvs.push(uv(u, v));
                self.colors.push(tint(u, v));
            }
        }
        self.knit(base, rows + 1, cols + 1);
    }
    /// Joins a grid laid down row by row from `base` into triangles that face the way their normals do.
    fn knit(&mut self, base: u32, rows: u32, cols: u32) {
        for r in 0..rows - 1 {
            for c in 0..cols - 1 {
                let a = base + r * cols + c;
                let (b, cc, d) = (a + 1, a + cols, a + cols + 1);
                for [i, j, k] in [[a, cc, b], [b, cc, d]] {
                    let [p, q, s] = [i, j, k].map(|n| self.positions[n as usize]);
                    let face = (q - p).cross(s - p);
                    if face.length_squared() < 1e-20 {
                        continue;
                    }
                    let out: Vec3 = [i, j, k].iter().map(|n| self.normals[*n as usize]).sum();
                    self.indices.extend(if face.dot(out) >= 0. { [i, j, k] } else { [i, k, j] });
                }
            }
        }
    }
    /// An ellipsoid, turned by `turn`.
    pub fn ellipsoid(&mut self, centre: Vec3, radii: Vec3, turn: Quat, fine: Fine, tint: [f32; 4]) {
        self.patch(
            fine.rings,
            fine.sides,
            true,
            |u, v| {
                let (s, c) = (u * PI).sin_cos();
                let (sa, ca) = (v * TAU).sin_cos();
                centre + turn * (Vec3::new(s * ca, -c, s * sa) * radii)
            },
            |_| centre,
            |u, v| Vec2::new(v * TAU * radii.x.max(radii.z), u * PI * radii.y),
            |_, _| tint,
        );
    }
    /// A body of stacked ellipses: rows of (height, half-width, half-depth, how far forward its middle is), bottom
    /// to top, joined smoothly and closed at both ends. Texture coordinates go round (u) and up in metres (v).
    pub fn loft(&mut self, rows: &[(f32, f32, f32, f32)], fine: Fine, tint: [f32; 4]) {
        let n = rows.len();
        let at = |k: isize| rows[k.clamp(0, n as isize - 1) as usize];
        // A row between the given ones (Catmull-Rom on every value), `s` from 0 to n - 1.
        let row = move |s: f32| {
            let k = (s.floor() as isize).clamp(0, n as isize - 2);
            let t = s - k as f32;
            let (p0, p1, p2, p3) = (at(k - 1), at(k), at(k + 1), at(k + 2));
            let spline = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2. * b + (c - a) * t + (2. * a - 5. * b + 4. * c - d) * t * t + (3. * b - a - 3. * c + d) * t * t * t)
            };
            (spline(p0.0, p1.0, p2.0, p3.0), spline(p0.1, p1.1, p2.1, p3.1), spline(p0.2, p1.2, p2.2, p3.2), spline(p0.3, p1.3, p2.3, p3.3))
        };
        let rings = fine.rings * 2;
        self.patch(
            rings,
            fine.sides,
            true,
            move |u, v| {
                let (y, rx, rz, cz) = row(u * (n - 1) as f32);
                let (sa, ca) = (v * TAU).sin_cos();
                Vec3::new(rx.max(0.) * ca, y, cz + rz.max(0.) * sa)
            },
            move |p| {
                let s = (rows.iter().position(|r| r.0 > p.y).unwrap_or(n - 1)) as f32;
                let (_, _, _, cz) = row((s - 0.5).clamp(0., (n - 1) as f32));
                Vec3::new(0., p.y, cz)
            },
            move |u, v| {
                let (y, rx, rz, _) = row(u * (n - 1) as f32);
                Vec2::new(v * TAU * (rx.max(0.) + rz.max(0.)) * 0.5, y)
            },
            |_, _| tint,
        );
    }
    /// A tube swept through `points` (each with its radius) along a smooth path, rounded off at both ends. Texture
    /// coordinates go round (u) and along in metres (v).
    pub fn tube(&mut self, points: &[(Vec3, f32)], fine: Fine, tint: [f32; 4]) {
        let count = points.len() as isize;
        let at = |i: isize| points[i.clamp(0, count - 1) as usize];
        let steps = (fine.rings / 4).max(2) as usize;
        let mut spine: Vec<(Vec3, f32)> = vec![];
        for span in 0..count - 1 {
            let (p0, p1, p2, p3) = (at(span - 1), at(span), at(span + 1), at(span + 2));
            for step in 0..steps {
                let t = step as f32 / steps as f32;
                let place = 0.5
                    * (2. * p1.0
                        + (p2.0 - p0.0) * t
                        + (2. * p0.0 - 5. * p1.0 + 4. * p2.0 - p3.0) * t * t
                        + (3. * p1.0 - p0.0 - 3. * p2.0 + p3.0) * t * t * t);
                spine.push((place, p1.1 + (p2.1 - p1.1) * t));
            }
        }
        spine.push(at(count - 1));
        let last = spine.len() - 1;
        let tangent = |i: usize| (spine[(i + 1).min(last)].0 - spine[i.saturating_sub(1)].0).normalize_or(Vec3::NEG_Y);
        // Frames carried along the path without twisting.
        let mut sides = vec![];
        let mut side = tangent(0).any_orthonormal_vector();
        for i in 0..=last {
            let t = tangent(i);
            side = (side - t * side.dot(t)).normalize_or(t.any_orthonormal_vector());
            sides.push(side);
        }
        const CAP: usize = 4;
        // Each row: a place on the spine, how far round an end cap it is, and which end.
        let mut layout: Vec<(usize, f32, f32)> = (1..=CAP).rev().map(|k| (0, k as f32 / CAP as f32 * PI * 0.5, -1.)).collect();
        layout.extend((0..=last).map(|i| (i, 0., 0.)));
        layout.extend((1..=CAP).map(|k| (last, k as f32 / CAP as f32 * PI * 0.5, 1.)));
        let base = self.positions.len() as u32;
        let cols = fine.sides;
        let mut along = 0.;
        for (row, &(i, angle, end)) in layout.iter().enumerate() {
            let (place, radius) = spine[i];
            if row > 0 && angle == 0. && i > 0 {
                along += place.distance(spine[i - 1].0);
            }
            let t = tangent(i);
            let a = sides[i];
            let b = t.cross(a);
            let (bulge, shrink) = angle.sin_cos();
            let centre = place + t * end * bulge * radius;
            for c in 0..=cols {
                let (s, co) = (c as f32 / cols as f32 * TAU).sin_cos();
                let rim = a * co + b * s;
                self.positions.push(centre + rim * radius * shrink);
                self.normals.push((rim * shrink + t * end * bulge).normalize_or(rim));
                self.uvs.push(Vec2::new(c as f32 / cols as f32 * TAU * radius, along + end * bulge * radius));
                self.colors.push(tint);
            }
        }
        self.knit(base, layout.len() as u32, cols + 1);
    }
    /// A surface of revolution about +y: `shape(u, angle)` gives the radius and height at `u` (0 at the bottom, 1 at
    /// the top) and the angle round.
    pub fn revolve(&mut self, centre: Vec3, turn: Quat, fine: Fine, shape: impl Fn(f32, f32) -> (f32, f32), tint: [f32; 4]) {
        self.patch(
            fine.rings,
            fine.sides * 2,
            true,
            |u, v| {
                let angle = v * TAU;
                let (r, y) = shape(u, angle);
                centre + turn * Vec3::new(r * angle.cos(), y, r * angle.sin())
            },
            |p| {
                let local = turn.inverse() * (p - centre);
                centre + turn * Vec3::new(0., local.y, 0.)
            },
            |u, v| {
                let (r, y) = shape(u, v * TAU);
                Vec2::new(v * TAU * r, y)
            },
            |_, _| tint,
        );
    }
    /// A ring round +y (a torus), turned by `turn`.
    pub fn torus(&mut self, centre: Vec3, turn: Quat, radius: f32, thick: f32, fine: Fine, tint: [f32; 4]) {
        self.patch(
            fine.sides,
            (fine.sides / 2).max(6),
            true,
            |u, v| {
                let (sa, ca) = (u * TAU).sin_cos();
                let (sb, cb) = (v * TAU).sin_cos();
                let ring = Vec3::new(ca, 0., sa);
                centre + turn * (ring * (radius + thick * cb) + Vec3::Y * thick * sb)
            },
            |p| {
                let local = turn.inverse() * (p - centre);
                centre + turn * (Vec3::new(local.x, 0., local.z).normalize_or(Vec3::X) * radius)
            },
            Vec2::new,
            |_, _| tint,
        );
    }
    /// Appends another builder's triangles.
    pub fn append(&mut self, other: &Builder) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.uvs.extend_from_slice(&other.uvs);
        self.colors.extend_from_slice(&other.colors);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }
    /// The same shape reflected left to right (a left hand from a right).
    pub fn mirrored(mut self) -> Self {
        for v in self.positions.iter_mut().chain(self.normals.iter_mut()) {
            v.x = -v.x;
        }
        for triangle in self.indices.chunks_mut(3) {
            triangle.swap(1, 2);
        }
        self
    }
    pub fn mesh(&self) -> Mesh {
        let list = |v: &[Vec3]| v.iter().map(|p| p.to_array()).collect::<Vec<_>>();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, list(&self.positions))
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, list(&self.normals))
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.iter().map(|uv| uv.to_array()).collect::<Vec<_>>())
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors.clone())
            .with_inserted_indices(Indices::U32(self.indices.clone()))
    }
}

pub const WHITE: [f32; 4] = [1.; 4];

/// A point on an ellipsoid's surface (radii `r`, centred at `c`) seen from its middle along `dir`, and its normal there.
pub fn on_ellipsoid(c: Vec3, r: Vec3, dir: Vec3) -> (Vec3, Vec3) {
    let d = dir.normalize_or(Vec3::NEG_Z);
    let k = 1. / ((d.x / r.x).powi(2) + (d.y / r.y).powi(2) + (d.z / r.z).powi(2)).sqrt();
    let p = d * k;
    (c + p, Vec3::new(p.x / (r.x * r.x), p.y / (r.y * r.y), p.z / (r.z * r.z)).normalize())
}

// ---------------------------------------------------------------------------------------------------------------
// Hands. A right hand in its own frame: the wrist at the origin, the fingers along -y, the palm facing -x and the
// thumb towards -z. A left hand is its mirror image (palm facing +x).

/// The four fingers' places across the hand and how long each is.
const FINGERS: [(f32, f32); 4] = [(-0.026, 1.0), (-0.0085, 1.08), (0.0085, 1.02), (0.025, 0.86)];
const KNUCKLE: Vec3 = Vec3::new(-0.003, -0.078, 0.);
const PHALANGES: [f32; 3] = [0.028, 0.021, 0.017];
const FINGER: f32 = 0.0108;

/// A finger's joints with the hand closed by `curl` (0 open, 1 round a handle): from the knuckle to the tip.
fn finger_joints(z: f32, length: f32, curl: f32) -> [Vec3; 4] {
    let mut p = KNUCKLE + Vec3::Z * z;
    let mut out = [p; 4];
    let mut angle = 0.12 + curl * 0.35;
    for (k, bone) in PHALANGES.iter().enumerate() {
        let d = Vec3::new(-angle.sin(), -angle.cos(), 0.);
        p += d * bone * length;
        out[k + 1] = p;
        angle += 0.10 + curl * [1.15, 1.05, 0.9][k];
    }
    out
}

/// Where a handle is held by a hand closed by `curl`, in the hand's frame: the middle of the circle its middle finger
/// makes, on the knuckle line.
pub fn grip_point(curl: f32) -> Vec3 {
    let [a, b, c, _] = finger_joints(0., 1.08, curl);
    // The circle through the three joints.
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    if n.length_squared() < 1e-12 {
        return a + Vec3::NEG_X * 0.02;
    }
    let centre = a + (ac.length_squared() * n.cross(ab) + ab.length_squared() * ac.cross(n)) / (2. * n.length_squared());
    Vec3::new(centre.x, centre.y, 0.)
}
/// How closed the gripping hand is (the open one is `OPEN`).
pub const GRIP: f32 = 1.0;
pub const OPEN: f32 = 0.18;

/// A chubby cartoon right hand closed by `curl`.
pub fn hand(curl: f32, fine: Fine, tint: [f32; 4]) -> Builder {
    let mut b = Builder::new();
    let small = fine.scaled(0.45);
    // The palm, a soft cushion, and the heel of the hand into the wrist.
    b.ellipsoid(Vec3::new(-0.002, -0.046, 0.), Vec3::new(0.021, 0.042, 0.041), Quat::IDENTITY, fine.scaled(0.6), tint);
    b.tube(&[(Vec3::new(0., 0.012, 0.), 0.026), (Vec3::new(-0.001, -0.016, 0.), 0.028)], small, tint);
    for (z, length) in FINGERS {
        let joints = finger_joints(z, length, curl);
        let points: Vec<(Vec3, f32)> = joints.iter().enumerate().map(|(k, p)| (*p, FINGER * (1. - 0.07 * k as f32))).collect();
        b.tube(&points, small, tint);
    }
    // The thumb, from the heel of the hand, across the palm as the hand closes.
    let thumb = [
        Vec3::new(-0.010, -0.022, -0.030),
        Vec3::new(-0.024 - 0.006 * curl, -0.046 - 0.004 * curl, -0.046 + 0.010 * curl),
        Vec3::new(-0.032 - 0.010 * curl, -0.066 - 0.010 * curl, -0.050 + 0.026 * curl),
    ];
    b.tube(&[(thumb[0], 0.0135), (thumb[1], 0.0125), (thumb[2], 0.011)], small, tint);
    b
}

// ---------------------------------------------------------------------------------------------------------------
// The mouth's poses.

/// The mouth's shapes: rest (a smile), the vowels, closed lips, and a laugh.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Viseme {
    Rest,
    Ah,
    Ee,
    Oh,
    Mm,
    Laugh,
}
impl Viseme {
    pub const ALL: [Viseme; 6] = [Viseme::Rest, Viseme::Ah, Viseme::Ee, Viseme::Oh, Viseme::Mm, Viseme::Laugh];
    /// The mouth's half-width, how far it opens above and below its line, and how much its corners turn up, as shares
    /// of the head's radius.
    fn shape(self) -> (f32, f32, f32, f32) {
        match self {
            Viseme::Rest => (0.21, 0., 0., 0.075),
            Viseme::Ah => (0.18, 0.045, 0.15, 0.03),
            Viseme::Ee => (0.24, 0.025, 0.06, 0.06),
            Viseme::Oh => (0.11, 0.07, 0.09, 0.),
            Viseme::Mm => (0.17, 0., 0., 0.02),
            Viseme::Laugh => (0.23, 0.012, 0.15, 0.09),
        }
    }
}

/// The colours inside a mouth: its lips, the dark inside, the teeth and the tongue.
#[derive(Clone, Copy)]
pub struct MouthColors {
    pub lip: [f32; 4],
    pub inside: [f32; 4],
    pub teeth: [f32; 4],
    pub tongue: [f32; 4],
}

/// A mouth pose on a head (an ellipsoid with `radii` about `centre`), its middle seen from the head's middle along
/// `dir`: layers drawn just over the skin, the lips under the opening, the teeth and tongue over it.
pub fn mouth(viseme: Viseme, centre: Vec3, radii: Vec3, dir: Vec3, colors: MouthColors, fine: Fine) -> Builder {
    let r = radii.x;
    let (w, up, down, smile) = viseme.shape();
    let (w, up, down, smile) = (w * r, up * r, down * r, smile * r);
    let lip = 0.022 * r;
    let mut b = Builder::new();
    let place = |s: f32, t: f32, lift: f32| {
        let d = dir.normalize() * radii.z.max(radii.x) + Vec3::X * s + Vec3::Y * t;
        let (p, normal) = on_ellipsoid(centre, radii, d);
        p + normal * lift
    };
    // The mouth's line, with its corners turned up, and the opening above and below it.
    let line = |s: f32| smile * (s / w).powi(2) - smile * 0.35;
    let open = |s: f32| (1. - (s / w).powi(2)).max(0.).powf(0.65);
    let top = |s: f32| line(s) + up * open(s);
    let bottom = |s: f32| line(s) - down * open(s);
    let cols = (fine.sides as f32 * 0.8) as u32;
    let fill = |b: &mut Builder, from: &dyn Fn(f32) -> f32, to: &dyn Fn(f32) -> f32, half: f32, lift: f32, color: [f32; 4]| {
        b.patch(
            2,
            cols,
            false,
            |u, v| {
                let s = (v * 2. - 1.) * half;
                let (lo, hi) = (from(s), to(s));
                place(s, lo + (hi - lo) * u, lift)
            },
            |p| centre + (p - centre) * 0.5,
            |u, v| Vec2::new(v, u),
            |_, _| color,
        );
    };
    let gap = up + down;
    // The lips: a band round the opening (or, closed, the smile's line), tapering to the corners.
    let thick = if viseme == Viseme::Mm { lip * 1.7 } else { lip };
    let lips_top = |s: f32| top(s) + thick * (1. - (s / (w + lip)).powi(2)).max(0.).sqrt();
    let lips_bottom = |s: f32| bottom(s) - thick * 1.2 * (1. - (s / (w + lip)).powi(2)).max(0.).sqrt();
    fill(&mut b, &lips_bottom, &lips_top, w + lip, 0.0008 * r / 0.15, colors.lip);
    if gap > 0.001 * r {
        fill(&mut b, &bottom, &top, w, 0.0016 * r / 0.15, colors.inside);
        // Upper teeth under the top lip; the tongue in the bottom of the mouth.
        let teeth = (gap * 0.32).min(0.035 * r);
        let teeth_bottom = |s: f32| top(s) - teeth * open(s);
        fill(&mut b, &teeth_bottom, &top, w * 0.85, 0.0022 * r / 0.15, colors.teeth);
        let tongue_top = |s: f32| bottom(s) + gap * 0.38 * open(s);
        fill(&mut b, &bottom, &tongue_top, w * 0.8, 0.0022 * r / 0.15, colors.tongue);
    }
    b
}

/// A 32 by 32 tile, repeating.
pub fn tile(pixel: impl Fn(u32, u32) -> [f32; 3]) -> Image {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const SIZE: u32 = 32;
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let c = pixel(x, y);
            data.extend(c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8));
            data.push(255);
        }
    }
    let mut image = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

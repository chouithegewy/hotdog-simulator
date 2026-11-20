// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The titan's geometry is made in code, like the hotdogs' and the people's: parametric surfaces (a lathe for the
//! sausage, superellipsoids for the hands, feet and bun plates, swept tubes for the fingers, the lips and the
//! toothpick), each vertex coloured where it is (the scorch, the grill marks, the splits), merged into one mesh a part.
//! Normals come from the surfaces themselves (finite differences), so a closed tube has no seam.
use bevy::{asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology};
use std::f32::consts::{PI, TAU};

/// A mesh being built: its corners (position, normal, colour, uv) and its triangles.
#[derive(Clone, Default, Debug)]
pub struct Shape {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub colors: Vec<[f32; 4]>,
    pub uvs: Vec<Vec2>,
    pub indices: Vec<u32>,
}

/// A colour as an artist picks it (sRGB, 0 to 1), in the linear light vertex colours are in.
pub fn srgb(r: f32, g: f32, b: f32) -> Vec3 {
    let l = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    Vec3::new(l(r), l(g), l(b))
}
/// A vertex colour: no embers in it (the alpha is the embers' mask on the titan's casing and crust, titan_casing.wgsl;
/// every other material it is drawn in is opaque, and ignores it).
pub fn rgba(c: Vec3) -> [f32; 4] {
    [c.x, c.y, c.z, 0.]
}
/// A vertex colour glowing as embers do, `mask` of the way (0 to 1).
pub fn ember(c: Vec3, mask: f32) -> [f32; 4] {
    [c.x, c.y, c.z, mask.clamp(0., 1.)]
}

impl Shape {
    /// A grid of `rows` + 1 by `columns` + 1 corners over `sample(u, v)` (u, v from 0 to 1), coloured by `tint(u, v,
    /// the point)`. Its normals are the surface's own (`flip` turns them over), worked out a little way in from a
    /// degenerate edge (a pole), so a closed surface is smooth across its seam.
    pub fn surface(&mut self, rows: u32, columns: u32, flip: bool, sample: impl Fn(f32, f32) -> Vec3, tint: impl Fn(f32, f32, Vec3) -> [f32; 4]) {
        let base = self.positions.len() as u32;
        let e = 1e-3;
        for i in 0..=rows {
            let u = i as f32 / rows as f32;
            for j in 0..=columns {
                let v = j as f32 / columns as f32;
                let p = sample(u, v);
                let (uu, vv) = (u.clamp(e * 2., 1. - e * 2.), v);
                let du = sample(uu + e, vv) - sample(uu - e, vv);
                let dv = sample(uu, vv + e) - sample(uu, vv - e);
                let mut n = dv.cross(du).normalize_or_zero();
                if flip {
                    n = -n;
                }
                if n == Vec3::ZERO {
                    n = Vec3::Y;
                }
                self.positions.push(p);
                self.normals.push(n);
                self.uvs.push(Vec2::new(u, v));
                self.colors.push(tint(u, v, p));
            }
        }
        let w = columns + 1;
        for i in 0..rows {
            for j in 0..columns {
                let a = base + i * w + j;
                let (b, c, d) = (a + 1, a + w, a + w + 1);
                // Wound so the triangles face the way the normals do.
                let face = [a, c, b, b, c, d];
                // (Either of the quad's triangles may be a sliver at a pole: both say which way it faces.)
                let at = |k: u32| self.positions[k as usize];
                let n = (at(c) - at(a)).cross(at(b) - at(a)) + (at(c) - at(b)).cross(at(d) - at(b));
                let want = self.normals[a as usize] + self.normals[b as usize] + self.normals[c as usize] + self.normals[d as usize];
                if n.dot(want) >= 0. {
                    self.indices.extend_from_slice(&face);
                } else {
                    self.indices.extend_from_slice(&[a, b, c, b, d, c]);
                }
            }
        }
    }
    /// A superellipsoid: `radii` along its own x, y and z, squarer the higher `power` (2 is an ellipsoid), turned by
    /// `turn` about its middle at `at`, coloured by `tint(the point in its own frame, -1 to 1 on each axis)`.
    #[allow(clippy::too_many_arguments)]
    pub fn blob(&mut self, at: Vec3, radii: Vec3, turn: Quat, power: f32, rows: u32, columns: u32, tint: impl Fn(Vec3) -> [f32; 4]) {
        let e = 2. / power.max(0.5);
        let f = |w: f32, m: f32| w.signum() * w.abs().powf(m);
        let unit = move |u: f32, v: f32| {
            let lat = (u - 0.5) * PI;
            let lon = v * TAU;
            Vec3::new(f(lat.cos(), e) * f(lon.sin(), e), f(lat.sin(), e), f(lat.cos(), e) * f(lon.cos(), e))
        };
        self.surface(rows, columns, false, |u, v| at + turn * (unit(u, v) * radii), |u, v, _| tint(unit(u, v)));
    }
    /// A tube through `points` (each a centre and a radius), smooth between them (Catmull-Rom), closed at both ends with
    /// round caps, `sides` round and `per` rings between two points; `tint(how far along, 0 to 1; the angle round, 0
    /// to 1)`.
    pub fn tube(&mut self, points: &[(Vec3, f32)], sides: u32, per: u32, tint: impl Fn(f32, f32) -> [f32; 4]) {
        if points.len() < 2 {
            return;
        }
        let n = points.len();
        let at = |i: isize| points[i.clamp(0, n as isize - 1) as usize];
        let spline = move |s: f32| -> (Vec3, f32) {
            let s = s.clamp(0., (n - 1) as f32);
            let i = (s.floor() as isize).min(n as isize - 2);
            let t = s - i as f32;
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let (t2, t3) = (t * t, t * t * t);
            let c = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| 0.5 * ((2. * b) + (-a + c) * t + (2. * a - 5. * b + 4. * c - d) * t2 + (-a + 3. * b - 3. * c + d) * t3);
            let p = c(p0.0, p1.0, p2.0, p3.0);
            let r = p1.1 + (p2.1 - p1.1) * (t * t * (3. - 2. * t));
            (p, r)
        };
        let last = (n - 1) as f32;
        // The ends are capped: the first and last tenth of u round off over a radius.
        let cap = 0.12;
        let along = move |u: f32| -> (Vec3, f32, Vec3) {
            let s = ((u - cap) / (1. - 2. * cap)).clamp(0., 1.) * last;
            let (p, r) = spline(s);
            let d = (spline((s + 0.01).min(last)).0 - spline((s - 0.01).max(0.)).0).normalize_or(Vec3::Y);
            (p, r, d)
        };
        // A fixed reference makes the frame; for a tube that runs near it, another.
        let reference = |d: Vec3| if d.dot(Vec3::Z).abs() < 0.9 { Vec3::Z } else { Vec3::X };
        let sample = move |u: f32, v: f32| {
            let (p, r, d) = along(u);
            let side = reference(d).cross(d).normalize_or(Vec3::X);
            let up = d.cross(side);
            let a = v * TAU;
            // The caps: the radius closes and the point runs on past the end.
            let (k, push) = if u < cap {
                let t = 1. - u / cap;
                ((1. - t * t).max(0.).sqrt(), -t * r)
            } else if u > 1. - cap {
                let t = (u - (1. - cap)) / cap;
                ((1. - t * t).max(0.).sqrt(), t * r)
            } else {
                (1., 0.)
            };
            p + d * push + (side * a.cos() + up * a.sin()) * r * k
        };
        let rows = (per * (n as u32 - 1)).max(2) + 6;
        self.surface(rows, sides, true, sample, |u, v, _| tint(((u - cap) / (1. - 2. * cap)).clamp(0., 1.), v));
    }
    /// Another shape added to this one.
    pub fn append(&mut self, other: &Shape) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.colors.extend_from_slice(&other.colors);
        self.uvs.extend_from_slice(&other.uvs);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }
    /// The same shape the other way round, left for right (its triangles wound back the right way).
    pub fn mirrored(mut self) -> Shape {
        for p in self.positions.iter_mut() {
            p.x = -p.x;
        }
        for n in self.normals.iter_mut() {
            n.x = -n.x;
        }
        for t in self.indices.chunks_mut(3) {
            t.swap(1, 2);
        }
        self
    }
    /// Triangles.
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
    /// The mesh.
    pub fn mesh(&self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.iter().map(|p| p.to_array()).collect::<Vec<_>>())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.iter().map(|n| n.to_array()).collect::<Vec<_>>())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.iter().map(|u| u.to_array()).collect::<Vec<_>>())
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors.clone())
            .with_inserted_indices(Indices::U32(self.indices.clone()))
    }
}

// ----- Noise, for the scorch and the grain: the same every run.

pub fn hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}
/// Smooth value noise, about 0 to 1, that wraps every `period` along x (so it closes round a tube; 0 for none).
pub fn value(x: f32, y: f32, period: u32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let wrap = |i: i32| if period > 0 { i.rem_euclid(period as i32) } else { i };
    let h = |i: f32, j: f32| hash((wrap(i as i32) as u32).wrapping_mul(73_856_093) ^ (j as i32 as u32).wrapping_mul(19_349_663) ^ seed.wrapping_mul(83_492_791));
    let s = |t: f32| t * t * (3. - 2. * t);
    let (a, b, c, d) = (h(xi, yi), h(xi + 1., yi), h(xi, yi + 1.), h(xi + 1., yi + 1.));
    let top = a + (b - a) * s(fx);
    let bottom = c + (d - c) * s(fx);
    top + (bottom - top) * s(fy)
}
/// Three octaves of `value`, wrapping every `period` cells along x at the first.
pub fn fbm(x: f32, y: f32, period: u32, seed: u32) -> f32 {
    (value(x, y, period, seed) + 0.5 * value(x * 2., y * 2.03, period * 2, seed + 1) + 0.25 * value(x * 4., y * 4.1, period * 4, seed + 2)) / 1.75
}
pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ball_faces_out_and_its_triangles_with_it() {
        let mut s = Shape::default();
        s.blob(Vec3::ZERO, Vec3::ONE, Quat::IDENTITY, 2., 12, 16, |_| [1.; 4]);
        for (p, n) in s.positions.iter().zip(&s.normals) {
            assert!(p.normalize().dot(*n) > 0.95, "{p} {n}");
        }
        for t in s.indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|k| s.positions[t[k] as usize]);
            let face = (b - a).cross(c - a);
            if face.length() > 1e-6 {
                assert!(face.dot((a + b + c) / 3.) > 0., "a triangle faces in");
            }
        }
    }
    #[test]
    fn a_tube_faces_out_and_a_mirror_keeps_its_winding() {
        let mut s = Shape::default();
        s.tube(&[(Vec3::ZERO, 0.1), (Vec3::new(0., -0.5, 0.1), 0.12), (Vec3::new(0.1, -1., 0.), 0.08)], 12, 4, |_, _| [1.; 4]);
        let check = |s: &Shape| {
            for t in s.indices.chunks(3) {
                let [a, b, c] = [0, 1, 2].map(|k| s.positions[t[k] as usize]);
                let face = (b - a).cross(c - a);
                let n = s.normals[t[0] as usize] + s.normals[t[1] as usize] + s.normals[t[2] as usize];
                if face.length() > 1e-7 {
                    assert!(face.dot(n) > 0., "a triangle faces against its normals");
                }
            }
        };
        check(&s);
        check(&s.clone().mirrored());
    }
    #[test]
    fn noise_wraps_round_and_stays_in_range() {
        for k in 0..50 {
            let y = k as f32 * 0.37;
            assert!((value(0., y, 8, 3) - value(8., y, 8, 3)).abs() < 1e-5);
            let f = fbm(k as f32 * 0.21, y, 8, 5);
            assert!((0. ..=1.).contains(&f));
        }
    }
}

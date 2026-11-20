// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The Ultra tier's dense shapes: what the meshlet renderer draws in place of the game's own few-hundred-triangle ones.
//! Each is real geometry at a millimetre or less, with no normal map standing in for it:
//!
//! * [`counter`]: a counter top's honed stone, its surface pitted and grained a tenth of a millimetre deep, its edges rounded
//!   over a 6 mm bullnose, on a 2 mm grid (millions of triangles for a counter).
//! * [`tile`]: a glazed backsplash tile 10 cm square, standing 3 mm proud of its grout with a rounded edge and a slight
//!   pillow, on a half-millimetre grid; instanced over the back wall.
//! * [`weave`]: a bamboo steamer's woven lid: the basket weave the game paints (`dojo::weave`) as geometry, every strip
//!   crowned, every seam grooved, every strip arching over the block it crosses and diving under the next.
//! * [`cereal`]: a cereal box, a filled card box: its large faces bulging, its edges creased round, its top flaps folded,
//!   at three quarters of a millimetre.
//! * [`plate`]: a dinner plate turned from its profile at 1024 steps round, with its foot.
//!
//! Every shape has exactly a position, a normal and a texture co-ordinate per vertex and 32-bit indices: what
//! `MeshletMesh::from_mesh` takes. Positions are in metres in the frame of what it replaces.
use bevy::{
    asset::RenderAssetUsages,
    image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::f32::consts::{PI, TAU};

/// A shape being built.
#[derive(Default, Clone)]
pub struct Shape {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl Shape {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_indices(Indices::U32(self.indices))
    }

    /// A grid patch of `cols` by `rows` cells: `at(s, t)` (each 0 to 1) is a point and its texture co-ordinate. Normals
    /// come from the grid itself; `flip` turns the patch's front (and normals) over. Only the cells whose middle `keep`
    /// is drawn; vertices no cell uses are left out.
    fn patch(&mut self, cols: usize, rows: usize, at: impl Fn(f32, f32) -> (Vec3, [f32; 2]), keep: impl Fn(f32, f32) -> bool, flip: bool) {
        let (w, h) = (cols + 1, rows + 1);
        let mut points = Vec::with_capacity(w * h);
        let mut uvs = Vec::with_capacity(w * h);
        for j in 0..h {
            for i in 0..w {
                let (p, uv) = at(i as f32 / cols as f32, j as f32 / rows as f32);
                points.push(p);
                uvs.push(uv);
            }
        }
        let mut used = vec![false; w * h];
        let mut cells = Vec::new();
        for j in 0..rows {
            for i in 0..cols {
                if keep((i as f32 + 0.5) / cols as f32, (j as f32 + 0.5) / rows as f32) {
                    cells.push((i, j));
                    for k in [j * w + i, j * w + i + 1, (j + 1) * w + i, (j + 1) * w + i + 1] {
                        used[k] = true;
                    }
                }
            }
        }
        let base = self.positions.len() as u32;
        let mut index = vec![u32::MAX; w * h];
        let mut next = base;
        for j in 0..h {
            for i in 0..w {
                let k = j * w + i;
                if !used[k] {
                    continue;
                }
                index[k] = next;
                next += 1;
                // Central differences, one-sided at the patch's edges.
                let (i0, i1) = (i.saturating_sub(1), (i + 1).min(cols));
                let (j0, j1) = (j.saturating_sub(1), (j + 1).min(rows));
                let ds = points[j * w + i1] - points[j * w + i0];
                let dt = points[j1 * w + i] - points[j0 * w + i];
                let n = ds.cross(dt).normalize_or(Vec3::Y) * if flip { -1. } else { 1. };
                self.positions.push(points[k].to_array());
                self.normals.push(n.to_array());
                self.uvs.push(uvs[k]);
            }
        }
        for (i, j) in cells {
            let [a, b, c, d] = [j * w + i, j * w + i + 1, (j + 1) * w + i, (j + 1) * w + i + 1].map(|k| index[k]);
            if flip {
                self.indices.extend([a, c, b, b, c, d]);
            } else {
                self.indices.extend([a, b, c, b, d, c]);
            }
        }
    }

    /// A closed `outline` (radius, height, outward normal in that plane, texture v) turned `steps` times round the y axis.
    /// The texture's u is the radius over `reach`.
    fn lathe(&mut self, outline: &[(Vec2, Vec2, f32)], steps: usize, reach: f32) {
        let base = self.positions.len() as u32;
        for (p, n, v) in outline {
            for k in 0..=steps {
                let a = k as f32 / steps as f32 * TAU;
                let (c, s) = (a.cos(), a.sin());
                self.positions.push([p.x * c, p.y, p.x * s]);
                self.normals.push(Vec3::new(n.x * c, n.y, n.x * s).normalize_or(Vec3::Y).to_array());
                self.uvs.push([(p.x / reach).clamp(0., 1.), *v]);
            }
        }
        let w = steps as u32 + 1;
        // Wound so that the front faces the outline's normals: (along the outline) x (round the axis) points along them.
        let probe = outline.len() / 2;
        let (p0, p1, n) = (outline[probe].0, outline[(probe + 1) % outline.len()].0, outline[probe].1);
        let along = Vec3::new(p1.x - p0.x, p1.y - p0.y, 0.);
        let round = Vec3::new(0., 0., 1.);
        let forward = along.cross(round).dot(Vec3::new(n.x, n.y, 0.)) > 0.;
        for r in 0..outline.len() as u32 - 1 {
            for k in 0..steps as u32 {
                let a = base + r * w + k;
                let (b, c, d) = (a + 1, a + w, a + w + 1);
                if forward {
                    self.indices.extend([a, c, b, b, c, d]);
                } else {
                    self.indices.extend([a, b, c, b, d, c]);
                }
            }
        }
    }
}

/// Smooth noise 0 to 1 on a grid `size` metres a side: a hash per lattice point, eased between them.
fn noise(x: f32, z: f32, size: f32, seed: u32) -> f32 {
    let (u, v) = (x / size, z / size);
    let (i, j) = (u.floor(), v.floor());
    let (fu, fv) = (u - i, v - j);
    let (su, sv) = (fu * fu * (3. - 2. * fu), fv * fv * (3. - 2. * fv));
    let h = |a: f32, b: f32| hash(a as i32, b as i32, seed);
    let (a, b, c, d) = (h(i, j), h(i + 1., j), h(i, j + 1.), h(i + 1., j + 1.));
    a + (b - a) * su + (c - a) * sv + (a - b - c + d) * su * sv
}

fn hash(x: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (z as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1 << 24) as f32
}

/// How far a rounded edge of radius `r` has fallen `e` metres in from the edge.
fn bullnose(e: f32, r: f32) -> f32 {
    if e >= r { 0. } else { r - (r * r - (r - e.max(0.)).powi(2)).max(0.).sqrt() }
}

/// A counter top `w` by `d` (x, z) and `thick` deep, centred on the origin, at `step` metres; its stone texture laid
/// `tile` metres to a repeat from the world's origin (`centre` is where its middle is, x and z).
pub fn counter(w: f32, d: f32, thick: f32, step: f32, tile: f32, centre: Vec2) -> Shape {
    let mut shape = Shape::default();
    let (hx, hz, top, r) = (w / 2., d / 2., thick / 2., 0.006_f32.min(thick / 2.));
    let height = |x: f32, z: f32| {
        let e = (hx - x.abs()).min(hz - z.abs());
        // Grain at three sizes, and pits: a few cells in a hundred hold a round dimple. Faded out over the bullnose.
        let grain = (noise(x, z, 0.007, 1) - 0.5) * 0.00008 + (noise(x, z, 0.0035, 2) - 0.5) * 0.00006 + (noise(x, z, 0.022, 3) - 0.5) * 0.00004;
        let (cx, cz) = ((x / 0.009).floor(), (z / 0.009).floor());
        let pit = if hash(cx as i32, cz as i32, 4) < 0.18 {
            let (px, pz) = ((cx + 0.25 + 0.5 * hash(cx as i32, cz as i32, 5)) * 0.009, (cz + 0.25 + 0.5 * hash(cx as i32, cz as i32, 6)) * 0.009);
            let q = ((x - px).hypot(z - pz) / 0.0016).min(1.);
            -0.00025 * (1. - q * q).max(0.)
        } else {
            0.
        };
        let fade = (e / (r * 1.5)).clamp(0., 1.);
        // Lifted 0.6 mm, so that its deepest pit stays above the flat top it replaces (and the room's worktop, flush with it).
        top + (0.0006 + grain + pit) * fade - bullnose(e, r)
    };
    let (cols, rows) = ((w / step).ceil() as usize, (d / step).ceil() as usize);
    let world_uv = |x: f32, z: f32| [(x + centre.x) / tile, (z + centre.y) / tile];
    shape.patch(
        cols,
        rows,
        |s, t| {
            let (x, z) = (-hx + s * w, -hz + t * d);
            (Vec3::new(x, height(x, z), z), world_uv(x, z))
        },
        |_, _| true,
        true,
    );
    // The four sides, from the foot of the bullnose to the bottom, sampled along the edge as the top is.
    let foot = top - r;
    let side = |shape: &mut Shape, n: usize, at: &dyn Fn(f32) -> (f32, f32), flip: bool| {
        shape.patch(
            n,
            2,
            |s, t| {
                let (x, z) = at(s);
                let y = foot + (-top - foot) * t;
                (Vec3::new(x, y, z), [(x + z + centre.x + centre.y) / tile, y / tile])
            },
            |_, _| true,
            flip,
        );
    };
    side(&mut shape, cols, &|s| (-hx + s * w, -hz), false);
    side(&mut shape, cols, &|s| (-hx + s * w, hz), true);
    side(&mut shape, rows, &|s| (-hx, -hz + s * d), true);
    side(&mut shape, rows, &|s| (hx, -hz + s * d), false);
    shape.patch(1, 1, |s, t| (Vec3::new(-hx + s * w, -top, -hz + t * d), [s, t]), |_, _| true, false);
    shape
}

/// A backsplash tile `size` metres square facing +y (its base at y = 0): `proud` metres high with an edge rounded over
/// that radius and a faint pillow, at `step`.
pub fn tile(size: f32, proud: f32, step: f32) -> Shape {
    let mut shape = Shape::default();
    let h = size / 2.;
    let n = (size / step).ceil() as usize;
    shape.patch(
        n,
        n,
        |s, t| {
            let (x, z) = (-h + s * size, -h + t * size);
            let e = (h - x.abs()).min(h - z.abs());
            let edge = proud - bullnose(e, proud);
            let pillow = 0.00035 * (1. - (x / h).powi(2)) * (1. - (z / h).powi(2));
            let glaze = (noise(x, z, 0.004, 9) - 0.5) * 0.00004;
            let y = edge + (pillow + glaze) * (e / proud).clamp(0., 1.);
            (Vec3::new(x, y, z), [s, t])
        },
        |_, _| true,
        true,
    );
    shape
}

/// The woven top of a steamer, an ellipse `rx` by `rz` facing +y, at `step`: the weave `dojo::weave` paints, `weave`
/// metres to a repeat (eight squares of three strips), as relief up to a millimetre and a half; its texture co-ordinates
/// are the lid's own (x and z over `weave`), so the painted strips lie on the modelled ones.
pub fn weave(rx: f32, rz: f32, step: f32, repeat: f32) -> Shape {
    let mut shape = Shape::default();
    let height = |x: f32, z: f32| {
        let (u, v) = ((x / repeat).rem_euclid(1.) * 8., (z / repeat).rem_euclid(1.) * 8.);
        let (i, j) = (u.floor() as i32, v.floor() as i32);
        let across = (i + j) % 2 == 0;
        let (fu, fv) = (u - u.floor(), v - v.floor());
        // Across the strip (t) and along it within its square (s).
        let along3 = if across { fv * 3. } else { fu * 3. };
        let t = along3 - along3.floor();
        let s = if across { fu } else { fv };
        let crown = 0.00045 * (1. - (2. * t - 1.).powi(2));
        let seam = -0.0003 * (1. - (t.min(1. - t) / 0.12).min(1.)).powi(2);
        // Over the square it is on top of, under at the squares either side.
        let arch = 0.0005 * (PI * s).sin();
        0.0004 + crown + seam + arch
    };
    let (cols, rows) = ((2. * rx / step).ceil() as usize, (2. * rz / step).ceil() as usize);
    shape.patch(
        cols,
        rows,
        |s, t| {
            let (x, z) = (-rx + s * 2. * rx, -rz + t * 2. * rz);
            (Vec3::new(x, height(x, z), z), [x / repeat, z / repeat])
        },
        |s, t| {
            let (x, z) = (-1. + 2. * s, -1. + 2. * t);
            x * x + z * z < 1.
        },
        true,
    );
    shape
}

/// The cereal box's texture co-ordinates (the same sheet as the game's own box, `loose::art::boxed`): each face its cell
/// of a 3 by 2 sheet, chosen by the axis its normal is nearest.
fn boxed_uv(p: Vec3, n: Vec3, half: [f32; 3]) -> [f32; 2] {
    let (p, n) = (p.to_array(), n.to_array());
    let axis = (0..3).max_by(|a, b| n[*a].abs().total_cmp(&n[*b].abs())).unwrap_or(1);
    let cell = axis as u32 * 2 + if n[axis] < 0. { 1 } else { 0 };
    let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
    let (s, t) = if axis == 2 { ((p[0] / half[0] + 1.) / 2., 1. - (p[1] / half[1] + 1.) / 2.) } else { ((p[a] / half[a] + 1.) / 2., (p[b] / half[b] + 1.) / 2.) };
    [((cell % 3) as f32 + s.clamp(0.01, 0.99)) / 3., ((cell / 3) as f32 + t.clamp(0.01, 0.99)) / 2.]
}

/// A cereal box of the given half extents with edges rounded `r`, at `step`: a full box, its large faces bulging, its
/// top's flaps folded with a crease across the middle.
pub fn cereal(half: [f32; 3], r: f32, step: f32) -> Shape {
    let mut shape = Shape::default();
    let h = Vec3::from_array(half);
    let inner = h - Vec3::splat(r);
    for axis in 0..3 {
        for sign in [1., -1.] {
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let (cols, rows) = ((2. * half[a] / step).ceil() as usize, (2. * half[b] / step).ceil() as usize);
            // How far the face bulges out at its middle: a full box swells most on its big faces.
            let bulge = [0.0012, 0.0006, 0.0028][axis];
            shape.patch(
                cols,
                rows,
                |s, t| {
                    let mut p = Vec3::ZERO;
                    p[axis] = sign * half[axis];
                    p[a] = -half[a] + s * 2. * half[a];
                    p[b] = -half[b] + t * 2. * half[b];
                    let core = p.clamp(-inner, inner);
                    let n = (p - core).normalize_or(Vec3::Y);
                    let rounded = core + n * r;
                    let mut face = Vec3::ZERO;
                    face[axis] = sign;
                    let mut out = bulge * (PI * s).sin() * (PI * t).sin();
                    // The top's flaps meet in a crease across its middle, and fold down at its long edges.
                    if axis == 1 && sign > 0. {
                        let across = (p.z / 0.0012).powi(2);
                        out -= 0.0004 * (-across).exp();
                    }
                    out += (noise(p[a], p[b] + axis as f32 + sign, 0.006, 11) - 0.5) * 0.00012;
                    (rounded + face * out, boxed_uv(rounded, n, half))
                },
                |_, _| true,
                // The patch's (s, t) axes are (a, b): their cross product is +axis.
                sign < 0.,
            );
        }
    }
    shape
}

/// A Catmull-Rom curve through `points`, `samples` long.
fn spline(points: &[Vec2], samples: usize) -> Vec<Vec2> {
    let n = points.len();
    (0..=samples)
        .map(|k| {
            let f = k as f32 / samples as f32 * (n - 1) as f32;
            let i = (f.floor() as usize).min(n - 2);
            let t = f - i as f32;
            let p = |j: isize| points[(i as isize + j).clamp(0, n as isize - 1) as usize];
            let (p0, p1, p2, p3) = (p(-1), p(0), p(1), p(2));
            0.5 * ((2. * p1) + (-p0 + p2) * t + (2. * p0 - 5. * p1 + 4. * p2 - p3) * t * t + (-p0 + 3. * p1 - 3. * p2 + p3) * t * t * t)
        })
        .collect()
}

/// The outline of a turned skin round the centre line `line`, `thick` across, its far end rounded: (radius, height,
/// outward normal, texture v), from the top at the axis out to the rim and back underneath.
fn skin(line: &[Vec2], thick: f32, v: f32) -> Vec<(Vec2, Vec2, f32)> {
    let normal = |i: usize| {
        let (a, b) = (line[i.saturating_sub(1)], line[(i + 1).min(line.len() - 1)]);
        let t = (b - a).normalize_or(Vec2::X);
        Vec2::new(-t.y, t.x)
    };
    let half = thick / 2.;
    let mut out: Vec<(Vec2, Vec2, f32)> = (0..line.len()).map(|i| (line[i] + normal(i) * half, normal(i), v)).collect();
    let (end, n) = (line[line.len() - 1], normal(line.len() - 1));
    let t = Vec2::new(n.y, -n.x);
    for k in 1..24 {
        let a = k as f32 / 24. * PI;
        let dir = n * a.cos() + t * a.sin();
        out.push((end + dir * half, dir, v));
    }
    out.extend((0..line.len()).rev().map(|i| (line[i] - normal(i) * half, -normal(i), v)));
    out
}

/// A dinner plate as the game's (its profile, 5 mm thick, and its foot ring), turned `steps` times round.
pub fn plate(steps: usize) -> Shape {
    let mut shape = Shape::default();
    let profile = [Vec2::new(0.0002, 0.004), Vec2::new(0.075, 0.004), Vec2::new(0.095, 0.006), Vec2::new(0.112, 0.012), Vec2::new(0.1255, 0.0165)];
    shape.lathe(&skin(&spline(&profile, 420), 0.005, 0.25), steps, PLATE_REACH);
    // The foot: a ring 3 mm across at 68 mm out, from 1.5 to 3.5 mm up (a capsule turned round the axis).
    let foot: Vec<(Vec2, Vec2, f32)> = (0..=48)
        .map(|k| {
            let a = k as f32 / 48. * TAU;
            let (c, s) = (a.cos(), a.sin());
            let centre = Vec2::new(0.068, if s >= 0. { 0.0035 } else { 0.0015 });
            (centre + Vec2::new(c, s) * 0.0015, Vec2::new(c, s), 0.75)
        })
        .collect();
    shape.lathe(&foot, steps, PLATE_REACH);
    shape
}

/// The plate's texture spans this radius (u).
const PLATE_REACH: f32 = 0.13;

/// The plate's paint, as the game paints its own (`loose::art::paint`): white glaze with a cobalt band round the rim
/// (u along the radius, the top row), and the unglazed foot (the bottom row).
pub fn plate_paint() -> Image {
    let width = 512;
    let mut data = Vec::with_capacity(width * 2 * 4);
    for row in 0..2 {
        for x in 0..width {
            let r = (x as f32 + 0.5) / width as f32 * PLATE_REACH;
            let c: [f32; 3] = if row == 1 {
                [0.84, 0.80, 0.72]
            } else if (0.113..0.119).contains(&r) {
                [0.10, 0.22, 0.60]
            } else {
                [0.96, 0.96, 0.94]
            };
            data.extend(c.map(|v| (v * 255.).round() as u8));
            data.push(255);
        }
    }
    let mut image = Image::new(Extent3d { width: width as u32, height: 2, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle faces the way its vertices' normals do (no shape is drawn inside out).
    fn faces_out(shape: &Shape) -> f32 {
        let mut agree = 0;
        for t in shape.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(shape.positions[i as usize]));
            let face = (b - a).cross(c - a);
            if face.length() < 1e-12 {
                agree += 1;
                continue;
            }
            let n = Vec3::from(shape.normals[t[0] as usize]) + Vec3::from(shape.normals[t[1] as usize]) + Vec3::from(shape.normals[t[2] as usize]);
            if face.dot(n) > 0. {
                agree += 1;
            }
        }
        agree as f32 / shape.triangles() as f32
    }

    fn valid(shape: &Shape) {
        let n = shape.positions.len();
        assert!(n > 0 && shape.normals.len() == n && shape.uvs.len() == n);
        assert!(shape.indices.iter().all(|&i| (i as usize) < n));
        assert!(shape.positions.iter().chain(&shape.normals).flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn every_shape_is_whole_and_faces_out() {
        for (name, shape) in [
            ("counter", counter(0.3, 0.2, 0.045, 0.004, 0.42, Vec2::new(0., 3.))),
            ("tile", tile(0.1, 0.003, 0.002)),
            ("weave", weave(0.12, 0.09, 0.002, 0.24)),
            ("cereal", cereal([0.115, 0.165, 0.04], 0.003, 0.004)),
            ("plate", plate(96)),
        ] {
            valid(&shape);
            let share = faces_out(&shape);
            assert!(share > 0.98, "{name}: only {share} of its triangles face out");
        }
    }

    #[test]
    fn the_counter_keeps_its_size_and_its_top_is_rough_but_flat() {
        let shape = counter(0.6, 0.4, 0.045, 0.002, 0.42, Vec2::ZERO);
        let ys: Vec<f32> = shape.positions.iter().map(|p| p[1]).collect();
        let (lo, hi) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
        assert!((lo + 0.0225).abs() < 1e-6 && hi < 0.0225 + 0.0008 && hi > 0.0225 + 0.0006, "{lo} {hi}");
        let xs: Vec<f32> = shape.positions.iter().map(|p| p[0]).collect();
        assert!((xs.iter().cloned().fold(f32::MIN, f32::max) - 0.3).abs() < 1e-6);
        // (0.6 / 2 mm) x (0.4 / 2 mm) cells on top, two triangles each; the four sides two rows deep at the same steps; the bottom.
        assert_eq!(shape.triangles(), 2 * 300 * 200 + 2 * (2 * 2 * 300) + 2 * (2 * 2 * 200) + 2);
    }

    #[test]
    fn the_woven_lid_fills_its_ellipse_and_nothing_outside_it() {
        let shape = weave(0.2, 0.1, 0.002, 0.24);
        assert!(shape.positions.iter().all(|p| (p[0] / 0.2).powi(2) + (p[2] / 0.1).powi(2) < 1.05));
        let area = shape.triangles() as f32 / 2. * 0.002 * 0.002;
        assert!((area - PI * 0.2 * 0.1).abs() < 0.004, "{area}");
        // Relief, not a flat disc: strips stand up to a millimetre and a half.
        let top = shape.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(top > 0.001 && top < 0.002, "{top}");
    }

    #[test]
    fn the_cereal_box_is_the_games_size_and_wears_its_sheet() {
        let half = [0.115, 0.165, 0.04];
        let shape = cereal(half, 0.003, 0.004);
        for axis in 0..3 {
            let reach = shape.positions.iter().map(|p| p[axis].abs()).fold(0., f32::max);
            assert!(reach >= half[axis] - 1e-4 && reach < half[axis] + 0.0035, "{axis}: {reach}");
        }
        assert!(shape.uvs.iter().all(|uv| (0. ..=1.).contains(&uv[0]) && (0. ..=1.).contains(&uv[1])));
    }
}

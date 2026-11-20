// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The jungle that grows over the kitchen: a canopy under the ceiling that breaks the sun into dapples,
//! vines down the wall and off the cabinets, lianas, broad-leaved plants and ferns around the counter,
//! and moss, tufts and fallen leaves along its rim. Every mesh is built here from a few numbers.
//!
//! It is decoration only: nothing here has collision, and nothing stands where it could come between
//! a camera and the play area. Tall plants stay outside the arena rectangle, which no line of sight
//! between two points inside it can cross; what hangs over it stays above the spectator camera; and
//! what lies on the counter inside the rectangle is flat.
use std::f32::consts::{PI, TAU};

use bevy::{
    asset::RenderAssetUsages, light::NotShadowCaster, mesh::Indices, prelude::*,
    render::render_resource::PrimitiveTopology,
};

use super::{
    filaments::Dice,
    surfaces::{Maps, Surveyed},
    ArenaBounds, ThemeOwned,
};

/// How much of the jungle to grow.
#[derive(Clone, Copy, Debug)]
pub struct Growth {
    /// The share of the plants between the few of a CPU renderer (0) and all of them (1).
    pub plants: f32,
    /// The minimal set: leaves without light through them or a sheen, and few plants of each kind.
    pub lean: bool,
    /// Whether the sun casts shadows, for a canopy to dapple the counter with.
    pub canopy: bool,
}

/// Rocks a plant about its root, as if in a light breeze.
#[derive(Component)]
pub struct Sway {
    rest: Quat,
    axis: Vec3,
    amount: f32,
    rate: f32,
    phase: f32,
}

pub fn sway(time: Res<Time>, mut plants: Query<(&Sway, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (sway, mut transform) in &mut plants {
        let swing = (t * sway.rate + sway.phase).sin() + 0.4 * (t * sway.rate * 2.3 + sway.phase * 1.7).sin();
        transform.rotation = sway.rest * Quat::from_axis_angle(sway.axis, sway.amount * swing);
    }
}

const DEEP: Vec3 = Vec3::new(0.035, 0.150, 0.030);
const GREEN: Vec3 = Vec3::new(0.085, 0.300, 0.050);
const LIME: Vec3 = Vec3::new(0.260, 0.480, 0.070);
const GOLD: Vec3 = Vec3::new(0.520, 0.470, 0.080);
const RUST: Vec3 = Vec3::new(0.300, 0.150, 0.045);
const BARK: Vec3 = Vec3::new(0.130, 0.100, 0.045);
const MOSS: Vec3 = Vec3::new(0.110, 0.260, 0.040);

/// Leaves, stems and moss gathered into one mesh.
#[derive(Default)]
struct Greenery {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// The shape of one leaf.
#[derive(Clone, Copy)]
struct Leaf {
    length: f32,
    width: f32,
    /// How steeply it leaves the stem, in radians above the horizontal, and how far it bows over.
    lift: f32,
    droop: f32,
    /// Edges raised above the midrib, as a fraction of the half width.
    cup: f32,
    /// 0 for a whole leaf, up to 1 for one cut deeply into fingers.
    lobes: f32,
    base: Vec3,
    tip: Vec3,
}

impl Greenery {
    /// Normals and tangents are worked out as each shape is built. Leaves taper to a point at both
    /// ends, and normals averaged from the zero-area triangles there would not be numbers at all.
    fn vertex(&mut self, at: Vec3, normal: Vec3, tangent: Vec3, uv: [f32; 2], color: Vec3) {
        let normal = normal.normalize_or(Vec3::Y);
        let tangent = (tangent - normal * normal.dot(tangent)).normalize_or(normal.any_orthonormal_vector());
        self.positions.push(at.to_array());
        self.normals.push(normal.to_array());
        self.tangents.push([tangent.x, tangent.y, tangent.z, -1.]);
        self.uvs.push(uv);
        self.colors.push([color.x, color.y, color.z, 1.]);
    }

    /// A leaf growing along +Z of `frame` with its face toward +Y.
    fn leaf(&mut self, frame: Mat4, leaf: Leaf) {
        const STEPS: u32 = 7;
        let first = self.positions.len() as u32;
        let step = leaf.length / STEPS as f32;
        let (mut y, mut z) = (0., 0.);
        for i in 0..=STEPS {
            let t = i as f32 / STEPS as f32;
            let angle = leaf.lift - leaf.droop * t * t;
            if i > 0 {
                let before = t - 0.5 / STEPS as f32;
                let mid = leaf.lift - leaf.droop * before * before;
                z += mid.cos() * step;
                y += mid.sin() * step;
            }
            let outline = (PI * t.powf(0.75)).sin().max(0.).powf(0.7);
            let cut = 1. - leaf.lobes * (0.5 + 0.5 * (t * 11. * PI).cos()).powi(2) * 4. * t * (1. - t);
            let half = leaf.width * 0.5 * outline * cut;
            let normal = Vec3::new(0., angle.cos(), -angle.sin());
            let color = leaf.base.lerp(leaf.tip, t);
            for (side, u) in [(-1f32, 0.), (0., 0.5), (1., 1.)] {
                let at = Vec3::new(side * half, y, z) + normal * (leaf.cup * half * side.abs());
                self.vertex(
                    frame.transform_point3(at),
                    frame.transform_vector3(normal - Vec3::X * side * leaf.cup),
                    frame.transform_vector3(Vec3::X),
                    [u, t],
                    color * if side == 0. { 1.2 } else { 1. },
                );
            }
        }
        for i in 0..STEPS {
            let a = first + i * 3;
            self.indices.extend([a, a + 3, a + 1, a + 1, a + 3, a + 4, a + 1, a + 4, a + 2, a + 2, a + 4, a + 5]);
        }
    }

    /// A stem through `points`, `radius` thick at its start and half that at its end.
    fn stem(&mut self, points: &[Vec3], radius: f32, color: Vec3) {
        const SIDES: u32 = 5;
        let first = self.positions.len() as u32;
        let mut run = 0.;
        for (i, &at) in points.iter().enumerate() {
            let ahead = points[(i + 1).min(points.len() - 1)] - points[i.saturating_sub(1)];
            let (side, up) = ahead.normalize_or(Vec3::Y).any_orthonormal_pair();
            if i > 0 {
                run += at.distance(points[i - 1]);
            }
            let r = radius * (1. - 0.5 * i as f32 / (points.len() - 1).max(1) as f32);
            for k in 0..=SIDES {
                let turn = k as f32 / SIDES as f32 * TAU;
                let out = side * turn.cos() + up * turn.sin();
                self.vertex(at + out * r, out, up * turn.cos() - side * turn.sin(), [k as f32 / SIDES as f32, run * 6.], color);
            }
        }
        for i in 0..points.len() as u32 - 1 {
            for k in 0..SIDES {
                let a = first + i * (SIDES + 1) + k;
                let b = a + SIDES + 1;
                self.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }

    /// A low cushion of moss with a ragged outline, sitting on the plane y = 0.
    fn mound(&mut self, centre: Vec3, radius: f32, height: f32, dice: &mut Dice) {
        const RINGS: u32 = 4;
        const SPOKES: u32 = 11;
        let first = self.positions.len() as u32;
        let ragged: Vec<f32> = (0..SPOKES).map(|_| dice.range(0.6, 1.25)).collect();
        self.vertex(centre + Vec3::Y * height, Vec3::Y, Vec3::X, [0.5, 0.5], MOSS.lerp(LIME, dice.range(0., 0.5)));
        for ring in 1..=RINGS {
            let t = ring as f32 / RINGS as f32;
            for spoke in 0..SPOKES {
                let turn = spoke as f32 / SPOKES as f32 * TAU;
                let r = radius * t * ragged[spoke as usize];
                let lump = height * (1. - t * t) * dice.range(0.6, 1.2);
                let at = centre + Vec3::new(turn.cos() * r, lump, turn.sin() * r);
                let color = DEEP.lerp(MOSS, dice.next()).lerp(LIME, dice.next() * 0.35) * (0.6 + 0.4 * (1. - t));
                let out = Vec3::new(turn.cos() * t, 1.2 - t, turn.sin() * t);
                self.vertex(at, out, Vec3::X, [0.5 + turn.cos() * t * 0.5, 0.5 + turn.sin() * t * 0.5], color);
            }
        }
        for spoke in 0..SPOKES {
            let next = (spoke + 1) % SPOKES;
            self.indices.extend([first, first + 1 + next, first + 1 + spoke]);
            for ring in 0..RINGS - 1 {
                let a = first + 1 + ring * SPOKES + spoke;
                let b = first + 1 + ring * SPOKES + next;
                self.indices.extend([a, b, a + SPOKES, b, b + SPOKES, a + SPOKES]);
            }
        }
    }

    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, self.tangents)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn frame(at: Vec3, yaw: f32, roll: f32) -> Mat4 {
    Mat4::from_rotation_translation(Quat::from_rotation_y(yaw) * Quat::from_rotation_z(roll), at)
}

fn shade(dice: &mut Dice) -> (Vec3, Vec3) {
    let base = DEEP.lerp(GREEN, dice.next());
    let tip = GREEN.lerp(LIME, dice.next()).lerp(GOLD, if dice.next() < 0.12 { dice.range(0.3, 0.8) } else { 0. });
    (base, tip)
}

/// Broad leaves spreading from one point and bowing down: a bough of the canopy, seen from below.
fn bough(dice: &mut Dice) -> Mesh {
    let mut green = Greenery::default();
    let leaves = 6 + (dice.next() * 5.) as u32;
    for i in 0..leaves {
        let yaw = (i as f32 + dice.range(-0.35, 0.35)) / leaves as f32 * TAU;
        let (base, tip) = shade(dice);
        green.leaf(
            frame(Vec3::new(yaw.sin(), 0., yaw.cos()) * 0.015, yaw, dice.range(-0.3, 0.3)),
            Leaf {
                length: dice.range(0.20, 0.40),
                width: dice.range(0.10, 0.20),
                lift: dice.range(-0.1, 0.35),
                droop: dice.range(0.5, 1.3),
                cup: dice.range(0.05, 0.3),
                lobes: if dice.next() < 0.4 { dice.range(0.3, 0.75) } else { 0. },
                base,
                tip,
            },
        );
    }
    green.stem(&[Vec3::ZERO, Vec3::new(0.01, 0.07, 0.), Vec3::new(-0.01, 0.16, 0.01)], 0.008, BARK);
    green.mesh()
}

/// A dense low shrub of small leaves.
fn shrub(dice: &mut Dice, size: f32, leaves: u32) -> Mesh {
    let mut green = Greenery::default();
    for _ in 0..leaves {
        let yaw = dice.range(0., TAU);
        let out = dice.next().sqrt() * size * 0.35;
        let (base, tip) = shade(dice);
        green.leaf(
            frame(Vec3::new(yaw.sin() * out, dice.range(0., size * 0.3), yaw.cos() * out), yaw, dice.range(-0.5, 0.5)),
            Leaf {
                length: size * dice.range(0.35, 0.8),
                width: size * dice.range(0.10, 0.22),
                lift: dice.range(0.25, 1.35),
                droop: dice.range(0.2, 1.0),
                cup: dice.range(0.1, 0.35),
                lobes: 0.,
                base,
                tip,
            },
        );
    }
    green.mesh()
}

/// Thin blades standing in a clump.
fn tuft(dice: &mut Dice, height: f32, blades: u32) -> Mesh {
    let mut green = Greenery::default();
    for _ in 0..blades {
        let yaw = dice.range(0., TAU);
        let (base, tip) = shade(dice);
        green.leaf(
            frame(Vec3::new(dice.range(-0.4, 0.4), 0., dice.range(-0.4, 0.4)) * height * 0.3, yaw, 0.),
            Leaf {
                length: height * dice.range(0.6, 1.1),
                width: height * dice.range(0.07, 0.13),
                lift: dice.range(0.9, 1.5),
                droop: dice.range(0.3, 1.2),
                cup: 0.3,
                lobes: 0.,
                base,
                tip,
            },
        );
    }
    green.mesh()
}

/// Fronds arching out of a crown, each a rib with a row of leaflets down either side.
fn fern(dice: &mut Dice, size: f32, fronds: u32) -> Mesh {
    let mut green = Greenery::default();
    for i in 0..fronds {
        let yaw = (i as f32 + dice.range(-0.3, 0.3)) / fronds as f32 * TAU;
        let (length, lift, droop) = (size * dice.range(0.7, 1.1), dice.range(0.7, 1.25), dice.range(1.0, 1.9));
        let turn = Quat::from_rotation_y(yaw);
        let mut rib = vec![Vec3::ZERO];
        let (mut y, mut z) = (0f32, 0f32);
        const STEPS: u32 = 12;
        for k in 1..=STEPS {
            let t = k as f32 / STEPS as f32;
            let angle = lift - droop * t * t;
            z += angle.cos() * length / STEPS as f32;
            y += angle.sin() * length / STEPS as f32;
            let at = turn * Vec3::new(0., y, z);
            rib.push(at);
            let (base, tip) = shade(dice);
            for side in [-1f32, 1.] {
                green.leaf(
                    Mat4::from_rotation_translation(
                        turn * Quat::from_rotation_x(-angle) * Quat::from_rotation_y(side * 1.15),
                        at,
                    ),
                    Leaf {
                        length: length * 0.26 * (PI * t.powf(0.6)).sin().max(0.15),
                        width: length * 0.045,
                        lift: 0.1,
                        droop: 0.5,
                        cup: 0.2,
                        lobes: 0.,
                        base,
                        tip,
                    },
                );
            }
        }
        green.stem(&rib, size * 0.012, DEEP.lerp(BARK, 0.5));
    }
    green.mesh()
}

/// Big leaves on long stalks, like a banana plant or a monstera, standing `height` tall.
fn broadleaf(dice: &mut Dice, height: f32, leaves: u32) -> Mesh {
    let mut green = Greenery::default();
    for i in 0..leaves {
        let yaw = (i as f32 + dice.range(-0.3, 0.3)) / leaves as f32 * TAU;
        let turn = Quat::from_rotation_y(yaw);
        let reach = height * dice.range(0.18, 0.42);
        let top = height * dice.range(0.55, 1.0);
        let stalk: Vec<Vec3> = (0..=6)
            .map(|k| {
                let t = k as f32 / 6.;
                turn * Vec3::new(0., top * t, reach * t * t)
            })
            .collect();
        green.stem(&stalk, height * 0.014, DEEP.lerp(LIME, 0.3));
        let (base, tip) = shade(dice);
        green.leaf(
            Mat4::from_rotation_translation(turn * Quat::from_rotation_z(dice.range(-0.25, 0.25)), stalk[6]),
            Leaf {
                length: height * dice.range(0.35, 0.6),
                width: height * dice.range(0.16, 0.28),
                lift: dice.range(0.2, 0.8),
                droop: dice.range(0.9, 1.8),
                cup: dice.range(0.05, 0.2),
                lobes: if dice.next() < 0.5 { dice.range(0.4, 0.8) } else { 0. },
                base,
                tip,
            },
        );
    }
    green.mesh()
}

/// A vine hanging `drop` metres from its root, wandering a little, with leaves down its length.
fn vine(dice: &mut Dice, drop: f32, wander: f32, thickness: f32, leaf: f32) -> Mesh {
    let mut green = Greenery::default();
    let steps = ((drop / 0.05) as usize).clamp(4, 40);
    let (a, b) = (dice.range(0., TAU), dice.range(0., TAU));
    let line: Vec<Vec3> = (0..=steps)
        .map(|k| {
            let t = k as f32 / steps as f32;
            Vec3::new(
                wander * (t * 7. + a).sin() * t,
                -drop * t,
                wander * 0.4 * (t * 5. + b).sin() * t,
            )
        })
        .collect();
    green.stem(&line, thickness, BARK.lerp(DEEP, 0.4));
    for (k, &at) in line.iter().enumerate().skip(1) {
        if dice.next() < 0.25 {
            continue;
        }
        let (base, tip) = shade(dice);
        green.leaf(
            frame(at, k as f32 * 2.4 + dice.range(-0.5, 0.5), dice.range(-0.3, 0.3)),
            Leaf {
                length: leaf * dice.range(0.6, 1.2),
                width: leaf * dice.range(0.4, 0.7),
                lift: dice.range(-0.6, 0.2),
                droop: dice.range(0.3, 1.0),
                cup: 0.15,
                lobes: 0.,
                base,
                tip,
            },
        );
    }
    green.mesh()
}

/// Fallen leaves lying flat, and cushions of moss, strewn over a strip of counter. World coordinates.
fn litter(dice: &mut Dice, strips: &[(Vec2, Vec2)], y: f32, leaves: u32, cushions: u32) -> Mesh {
    let mut green = Greenery::default();
    for i in 0..leaves + cushions {
        let (low, high) = strips[i as usize % strips.len()];
        let at = Vec3::new(dice.range(low.x, high.x), y, dice.range(low.y, high.y));
        if i < cushions {
            let radius = dice.range(0.012, 0.04);
            green.mound(at, radius, radius * dice.range(0.12, 0.22), dice);
            continue;
        }
        let dead = dice.next();
        let (base, tip) = if dead < 0.45 {
            (RUST * dice.range(0.6, 1.2), RUST.lerp(GOLD, dice.next()))
        } else if dead < 0.7 {
            (GOLD * dice.range(0.5, 1.), GOLD.lerp(LIME, dice.next()))
        } else {
            shade(dice)
        };
        green.leaf(
            frame(at + Vec3::Y * dice.range(0.0006, 0.0018), dice.range(0., TAU), dice.range(-0.08, 0.08)),
            Leaf {
                length: dice.range(0.014, 0.038),
                width: dice.range(0.007, 0.018),
                lift: 0.03,
                droop: dice.range(-0.12, 0.05),
                cup: dice.range(0., 0.25),
                lobes: 0.,
                base,
                tip,
            },
        );
    }
    green.mesh()
}

/// Grows the jungle around the arena. `sunlight` is the direction the sun's light travels.
pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    maps: Option<&Maps>,
    bounds: &ArenaBounds,
    growth: Growth,
    sunlight: Vec3,
) {
    let mut dice = Dice(0x1EAF);
    let lean = growth.lean;
    // How many of a kind: `full` with every plant, `few` with the fewest.
    let share = if lean { 0. } else { growth.plants.clamp(0., 1.) };
    let count = |full: usize, few: usize| (few as f32 + (full as f32 - few as f32) * share).round() as usize;
    let foliage = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: maps.map(|m| m.albedo.clone()),
        normal_map_texture: maps.map(|m| m.normal.clone()),
        metallic_roughness_texture: maps.map(|m| m.orm.clone()),
        perceptual_roughness: if maps.is_some() { 0.9 } else { 0.5 },
        reflectance: 0.45,
        // Leaves let light through: lit from behind they glow.
        diffuse_transmission: if lean { 0. } else { 0.4 },
        clearcoat: if lean { 0. } else { 0.18 },
        clearcoat_perceptual_roughness: 0.4,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let (min, max, floor) = (bounds.min, bounds.max, bounds.floor);
    let plant = |commands: &mut Commands, mesh: Handle<Mesh>, at: Vec3, rest: Quat, scale: f32, amount: f32, dice: &mut Dice| {
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(foliage.clone()),
            Transform::from_translation(at).with_rotation(rest).with_scale(Vec3::splat(scale)),
            Sway {
                rest,
                axis: Vec3::new(dice.range(-1., 1.), 0., dice.range(-1., 1.)).normalize_or(Vec3::X),
                amount,
                rate: dice.range(0.5, 1.1),
                phase: dice.range(0., TAU),
            },
            ThemeOwned,
            Surveyed,
        ));
    };

    // The canopy. Boughs sit where they come between the sun and the counter, so their shadows land
    // on the play area as dapples, with gaps left for the light to come through in shafts.
    let boughs: Vec<Handle<Mesh>> = (0..count(6, 2).max(1)).map(|_| meshes.add(bough(&mut dice))).collect();
    // Without shadows there is nothing for a canopy to dapple: it gets none.
    let (columns, rows) = if lean || !growth.canopy { (0, 0) } else { (15, 5) };
    for row in 0..rows {
        for column in 0..columns {
            if dice.next() < 0.5 {
                continue;
            }
            // Fewer plants: every so many boughs left out, keeping the gaps where they were.
            if share < 1. && ((row * columns + column) as f32 * 0.618).fract() >= share.max(0.35) {
                continue;
            }
            let target = Vec3::new(
                min.x - 0.25 + (max.x - min.x + 0.5) * (column as f32 + dice.next()) / columns as f32,
                floor,
                min.y - 0.2 + (max.y - min.y + 0.4) * (row as f32 + dice.next()) / rows as f32,
            );
            let height = bounds.ceiling - dice.range(0.15, 0.48);
            let at = target - sunlight * ((height - floor) / -sunlight.y);
            let rest = Quat::from_rotation_y(dice.range(0., TAU)) * Quat::from_rotation_x(dice.range(-0.25, 0.25));
            let bough = boughs[(row * columns + column) % boughs.len()].clone();
            plant(commands, bough, at, rest, dice.range(0.5, 0.95), 0.05, &mut dice);
        }
    }
    // A second, thinner layer straight overhead, for whoever looks up.
    for _ in 0..if growth.canopy { count(16, 0) } else { 0 } {
        let at = Vec3::new(
            dice.range(min.x - 0.3, max.x + 0.3),
            bounds.ceiling - dice.range(0.08, 0.3),
            dice.range(min.y - 0.5, max.y + 0.1),
        );
        let rest = Quat::from_rotation_y(dice.range(0., TAU));
        let bough = boughs[(dice.next() * boughs.len() as f32) as usize % boughs.len()].clone();
        plant(commands, bough, at, rest, dice.range(0.8, 1.3), 0.04, &mut dice);
    }

    // Vines down the wall behind the counter and off the cabinets above it. The wall is just past the
    // back edge; the cabinets overhang the back 0.3 m of the counter from 0.7 m up, clear of every
    // line of sight to the play area.
    let overhang = floor + 0.70;
    let vines = count(14, 3);
    for i in 0..vines {
        let x = min.x + (max.x - min.x) * (i as f32 + dice.next()) / vines as f32;
        let drop = dice.range(0.25, 0.52);
        let mesh = meshes.add(vine(&mut dice, drop, 0.035, 0.004, 0.05));
        plant(commands, mesh, Vec3::new(x, overhang - 0.01, max.y + 0.016), Quat::IDENTITY, 1., 0.012, &mut dice);
    }
    for i in 0..vines {
        let x = min.x + (max.x - min.x) * (i as f32 + dice.next()) / vines as f32;
        let top = overhang + dice.range(0.25, 0.68);
        let drop = top - overhang + dice.range(-0.1, 0.1);
        let mesh = meshes.add(vine(&mut dice, drop, 0.03, 0.004, 0.06));
        plant(commands, mesh, Vec3::new(x, top, max.y - 0.30), Quat::IDENTITY, 1., 0.02, &mut dice);
    }
    // Lianas from the ceiling beyond either end of the counter.
    for side in [-1., 1.] {
        for _ in 0..count(4, 1) {
            let x = if side < 0. { min.x - dice.range(0.14, 0.5) } else { max.x + dice.range(0.14, 0.5) };
            let drop = bounds.ceiling - floor - dice.range(0.25, 0.9);
            let mesh = meshes.add(vine(&mut dice, drop, 0.06, 0.011, 0.09));
            let at = Vec3::new(x, bounds.ceiling - 0.02, dice.range(min.y - 0.1, max.y));
            plant(commands, mesh, at, Quat::IDENTITY, 1., 0.03, &mut dice);
        }
    }

    // Plants standing on the kitchen floor at either end of the counter, rising well above it.
    let giants: Vec<Handle<Mesh>> = (0..count(3, 1)).map(|_| meshes.add(broadleaf(&mut dice, 1., 7))).collect();
    let ferns: Vec<Handle<Mesh>> = (0..count(3, 1)).map(|_| meshes.add(fern(&mut dice, 1., 9))).collect();
    for side in [-1., 1.] {
        for k in 0..count(3, 1) {
            let x = if side < 0. { min.x - dice.range(0.36, 0.52) } else { max.x + dice.range(0.36, 0.52) };
            let z = min.y + (max.y - min.y) * (k as f32 + dice.range(0.2, 0.8)) / 3.;
            let rest = Quat::from_rotation_y(dice.range(0., TAU));
            let giant = giants[k % giants.len()].clone();
            plant(commands, giant, Vec3::new(x, 0., z), rest, floor + dice.range(0.35, 0.75), 0.025, &mut dice);
            // Ferns spilling over the end of the counter itself.
            let x = if side < 0. { min.x - dice.range(0.10, 0.2) } else { max.x + dice.range(0.10, 0.2) };
            let fern = ferns[k % ferns.len()].clone();
            let rest = Quat::from_rotation_y(dice.range(0., TAU));
            plant(commands, fern, Vec3::new(x, floor - dice.range(0.05, 0.2), z), rest, dice.range(0.22, 0.34), 0.04, &mut dice);
        }
    }
    // Undergrowth on the floor in front of the counter, kept below its top.
    for _ in 0..count(12, 0) {
        let at = Vec3::new(dice.range(min.x - 0.3, max.x + 0.3), 0., min.y - dice.range(0.28, 1.0));
        let rest = Quat::from_rotation_y(dice.range(0., TAU));
        let (mesh, scale) = if dice.next() < 0.5 {
            (ferns[(dice.next() * 3.) as usize % ferns.len()].clone(), (floor - 0.12) * dice.range(0.55, 0.9))
        } else {
            (giants[(dice.next() * 3.) as usize % giants.len()].clone(), (floor - 0.12) * dice.range(0.5, 0.75))
        };
        plant(commands, mesh, at, rest, scale, 0.03, &mut dice);
    }

    // The rim of the counter: shrubs and tufts on the lip just outside the play rectangle, and
    // moss and fallen leaves, which are flat, reaching a little way in.
    let shrubs: Vec<Handle<Mesh>> = (0..count(4, 1)).map(|_| meshes.add(shrub(&mut dice, 1., 26))).collect();
    let tufts: Vec<Handle<Mesh>> = (0..count(4, 1)).map(|_| meshes.add(tuft(&mut dice, 1., 14))).collect();
    let lip = 0.014;
    let along = |dice: &mut Dice| -> (Vec3, f32) {
        // A point on the lip, and how tall a plant may stand there: least along the front.
        let edge = dice.next();
        if edge < 0.42 {
            (Vec3::new(dice.range(min.x, max.x), floor, max.y + lip), 0.075)
        } else if edge < 0.62 {
            (Vec3::new(min.x - lip, floor, dice.range(min.y, max.y)), 0.06)
        } else if edge < 0.82 {
            (Vec3::new(max.x + lip, floor, dice.range(min.y, max.y)), 0.06)
        } else {
            (Vec3::new(dice.range(min.x, max.x), floor, min.y - lip), 0.028)
        }
    };
    for i in 0..count(80, 10) {
        let (at, tallest) = along(&mut dice);
        let rest = Quat::from_rotation_y(dice.range(0., TAU));
        let (mesh, scale) = if i % 3 == 0 {
            (shrubs[i % shrubs.len()].clone(), tallest * dice.range(0.7, 1.2))
        } else {
            (tufts[i % tufts.len()].clone(), tallest * dice.range(0.5, 1.0))
        };
        plant(commands, mesh, at, rest, scale, 0.06, &mut dice);
    }
    let reach = 0.13;
    let strips = [
        (Vec2::new(min.x, max.y - reach), Vec2::new(max.x, max.y + 0.02)),
        (Vec2::new(min.x, max.y - reach * 0.5), Vec2::new(max.x, max.y + 0.02)),
        (Vec2::new(min.x - 0.02, min.y), Vec2::new(min.x + reach, max.y)),
        (Vec2::new(max.x - reach, min.y), Vec2::new(max.x + 0.02, max.y)),
        (Vec2::new(min.x, min.y - 0.02), Vec2::new(max.x, min.y + reach * 0.6)),
    ];
    let scatter = meshes.add(litter(&mut dice, &strips, floor, count(900, 80) as u32, count(170, 16) as u32));
    commands.spawn((
        Mesh3d(scatter),
        MeshMaterial3d(foliage.clone()),
        Transform::IDENTITY,
        NotShadowCaster,
        ThemeOwned,
        Surveyed,
    ));
    // A few leaves that have drifted farther onto the counter.
    let strays = [(Vec2::new(min.x, min.y), Vec2::new(max.x, max.y))];
    let drift = meshes.add(litter(&mut dice, &strays, floor, count(110, 20) as u32, 0));
    commands.spawn((
        Mesh3d(drift),
        MeshMaterial3d(foliage),
        Transform::IDENTITY,
        NotShadowCaster,
        ThemeOwned,
        Surveyed,
    ));
}

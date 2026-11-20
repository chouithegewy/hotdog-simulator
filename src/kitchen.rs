// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The kitchen's own things, drawn from a map (`level::build` calls `build`): the sink and its tap, what stands
//! on the counter (the kettle, the knife block, canisters, jars, boxes of cereal, the paper towels, the dish rack,
//! the spice rack, the fruit bowl), the fitted cabinets under the counter and on the wall, and the bin on the
//! floor. Each is a map block (or sink, or prop) whose rules `kitchen_sim::map` keeps; here it only gets its looks,
//! fitted to the block's box and turned to its `facing`.
//!
//! Everything is built from a handful of shared unit meshes and one small palette, made once (`Furniture`), so a
//! counter full of things costs draw calls that batch rather than new assets, and rebuilding a map allocates none.
//! Small details (handles, labels, the wires of the dish rack, spice jars) cast no shadow.
use crate::level::MapPart;
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use kitchen_sim::map::{Block, FIRST_KITS, KITS, Map, Prop, Side};
use std::f32::consts::{FRAC_PI_2, PI};

const FLOOR: f32 = 0.9025;
/// The counter top's thickness, and how far it reaches past the playable area (as `level::build` draws it).
pub const SLAB: f32 = 0.045;
const OVER: [f64; 2] = [0.025, 0.02];

/// Whether `level` leaves a block to this module: the counter things, everything among KITS after the appliances.
pub fn drawn(style: &str) -> bool {
    // (The electric griddle is one of KITS, but griddle.rs draws it: it heats, glows and smokes. dojo.rs draws the gong: it swings.)
    // (The fridge, the oven and the pet's bowl stand on the kitchen floor: floor.rs draws them.)
    KITS[FIRST_KITS..].contains(&style) && style != kitchen_sim::griddle::STYLE && style != crate::dojo::GONG && !crate::floor::KITS.contains(&style)
}

/// The counter top as rectangles [min x, min z, max x, max z]: the counter's outline (a little past the playable
/// area) with a hole for each sink. Without a sink, the one rectangle `level` always drew (and the ragdolls met).
pub fn counter_top(map: &Map) -> Vec<[f64; 4]> {
    let [x0, z0, x1, z1] = map.bounds;
    let mut pieces = vec![[x0 - OVER[0], z0 - OVER[1], x1 + OVER[0], z1 + OVER[1]]];
    for sink in &map.sinks {
        let [a, b, c, d] = sink.rect;
        let mut cut = vec![];
        for r in pieces {
            if a >= r[2] || c <= r[0] || b >= r[3] || d <= r[1] {
                cut.push(r);
                continue;
            }
            // What is left of the piece round the hole: the strips before and after it along z, then beside it along x.
            let (lo, hi) = (b.max(r[1]), d.min(r[3]));
            for piece in [[r[0], r[1], r[2], lo], [r[0], hi, r[2], r[3]], [r[0], lo, a.max(r[0]), hi], [c.min(r[2]), lo, r[2], hi]] {
                if piece[2] - piece[0] > 1e-4 && piece[3] - piece[1] > 1e-4 {
                    cut.push(piece);
                }
            }
        }
        pieces = cut;
    }
    pieces
}

/// Shared meshes and the palette.
#[derive(Resource, Clone)]
pub struct Furniture {
    cube: Handle<Mesh>,
    /// Unit cylinders (radius 0.5, height 1, centred), smooth and coarse.
    round: Handle<Mesh>,
    coarse: Handle<Mesh>,
    ball: Handle<Mesh>,
    /// A cylinder narrowing to 0.6 of its width at the top (a kettle's shoulder).
    shoulder: Handle<Mesh>,
    /// Half a ring in the y-z plane, from (0, 0, -1) over (0, 1, 0) to (0, 0, 1), its tube 0.13 thick: a handle, a spout.
    arch: Handle<Mesh>,
    /// A bowl turned on a lathe: 1 across the rim, 1 high.
    bowl: Handle<Mesh>,
    capsule: Handle<Mesh>,
    steel: Handle<StandardMaterial>,
    chrome: Handle<StandardMaterial>,
    basin: Handle<StandardMaterial>,
    hole: Handle<StandardMaterial>,
    black: Handle<StandardMaterial>,
    ceramic: Handle<StandardMaterial>,
    paper: Handle<StandardMaterial>,
    card: Handle<StandardMaterial>,
    beech: Handle<StandardMaterial>,
    walnut: Handle<StandardMaterial>,
    glass: Handle<StandardMaterial>,
    wire: Handle<StandardMaterial>,
    /// Bright colours for labels, boxes, lids and spices.
    paint: [Handle<StandardMaterial>; 9],
    fruit: [Handle<StandardMaterial>; 4],
    stem: Handle<StandardMaterial>,
    door: Handle<StandardMaterial>,
    panel: Handle<StandardMaterial>,
    carcass: Handle<StandardMaterial>,
    plinth: Handle<StandardMaterial>,
}
const RED: usize = 0;
const YELLOW: usize = 1;
const BLUE: usize = 2;
const GREEN: usize = 3;
const ORANGE: usize = 4;
const BROWN: usize = 5;
const CREAM: usize = 6;
const TEAL: usize = 7;
const WHITE: usize = 8;

impl FromWorld for Furniture {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let cube = meshes.add(Cuboid::default());
        let round = meshes.add(Cylinder::new(0.5, 1.).mesh().resolution(28));
        let coarse = meshes.add(Cylinder::new(0.5, 1.).mesh().resolution(12));
        let ball = meshes.add(Sphere::new(0.5).mesh().uv(18, 12));
        let shoulder = meshes.add(ConicalFrustum { radius_top: 0.3, radius_bottom: 0.5, height: 1. }.mesh().resolution(28));
        let arch = meshes.add(arch());
        let bowl = meshes.add(lathe(&[[0., 0.], [0.24, 0.], [0.27, 0.06], [0.42, 0.42], [0.5, 1.], [0.47, 1.], [0.39, 0.45], [0.24, 0.1], [0., 0.1]], 28));
        let capsule = meshes.add(Capsule3d::new(0.5, 1.).mesh().longitudes(12).latitudes(8));
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        let mut make = |r: f32, g: f32, b: f32, roughness: f32, metallic: f32| {
            mats.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: roughness, metallic, ..default() })
        };
        let steel = make(0.66, 0.68, 0.70, 0.32, 0.85);
        let chrome = make(0.85, 0.87, 0.9, 0.08, 1.);
        let basin = make(0.58, 0.6, 0.62, 0.26, 0.8);
        let hole = make(0.02, 0.022, 0.024, 0.6, 0.);
        let black = make(0.03, 0.03, 0.032, 0.45, 0.);
        let ceramic = make(0.93, 0.91, 0.86, 0.22, 0.);
        let paper = make(0.97, 0.96, 0.93, 0.92, 0.);
        let card = make(0.58, 0.43, 0.27, 0.9, 0.);
        let beech = make(0.74, 0.54, 0.33, 0.55, 0.);
        let walnut = make(0.33, 0.2, 0.11, 0.45, 0.);
        let wire = make(0.9, 0.9, 0.88, 0.35, 0.);
        let paint = [
            make(0.78, 0.1, 0.07, 0.35, 0.),
            make(0.97, 0.74, 0.12, 0.4, 0.),
            make(0.13, 0.33, 0.72, 0.35, 0.),
            make(0.22, 0.55, 0.2, 0.4, 0.),
            make(0.96, 0.46, 0.08, 0.4, 0.),
            make(0.42, 0.24, 0.1, 0.6, 0.),
            make(0.93, 0.86, 0.66, 0.45, 0.),
            make(0.1, 0.5, 0.5, 0.35, 0.),
            make(0.97, 0.97, 0.95, 0.4, 0.),
        ];
        let fruit = [make(1., 0.52, 0.08, 0.5, 0.), make(0.72, 0.05, 0.05, 0.28, 0.), make(0.48, 0.72, 0.16, 0.3, 0.), make(0.96, 0.83, 0.3, 0.45, 0.)];
        let stem = make(0.3, 0.2, 0.08, 0.7, 0.);
        // The fitted cabinets in the wall cupboards' wood (`fridge::build_room`), their panels a shade lighter.
        let door = make(0.24, 0.13, 0.07, 0.6, 0.);
        let panel = make(0.3, 0.17, 0.09, 0.55, 0.);
        let carcass = make(0.16, 0.09, 0.05, 0.7, 0.);
        let plinth = make(0.05, 0.05, 0.05, 0.6, 0.);
        let glass = mats.add(StandardMaterial {
            base_color: Color::srgba(0.82, 0.92, 0.95, 0.24),
            perceptual_roughness: 0.04,
            reflectance: 0.7,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        Furniture {
            cube,
            round,
            coarse,
            ball,
            shoulder,
            arch,
            bowl,
            capsule,
            steel,
            chrome,
            basin,
            hole,
            black,
            ceramic,
            paper,
            card,
            beech,
            walnut,
            glass,
            wire,
            paint,
            fruit,
            stem,
            door,
            panel,
            carcass,
            plinth,
        }
    }
}

/// Half a ring of tube in the y-z plane (radius 1, tube 0.13), standing on its two ends at z = -1 and z = 1.
fn arch() -> Mesh {
    Torus { minor_radius: 0.13, major_radius: 1. }
        .mesh()
        .minor_resolution(10)
        .major_resolution(18)
        .angle_range(0.0..=PI)
        .build()
        .rotated_by(Quat::from_rotation_y(FRAC_PI_2) * Quat::from_rotation_x(-FRAC_PI_2))
}

/// A surface of revolution about y from a closed profile of [radius, height] points, its normals worked out from
/// the profile (the profile runs out along the outside and back along the inside of a shell).
pub(crate) fn lathe(profile: &[[f32; 2]], sides: usize) -> Mesh {
    let (mut positions, mut normals, mut uvs, mut indices) = (vec![], vec![], vec![], vec![]);
    for k in 0..profile.len() - 1 {
        let ([r0, y0], [r1, y1]) = (profile[k], profile[k + 1]);
        // Outward normal of the segment in the profile's plane: the segment turned a quarter.
        let (dr, dy) = (r1 - r0, y1 - y0);
        let length = (dr * dr + dy * dy).sqrt().max(1e-6);
        let (nr, ny) = (dy / length, -dr / length);
        let first = positions.len() as u32;
        for i in 0..=sides {
            let t = i as f32 / sides as f32 * std::f32::consts::TAU;
            let (s, c) = t.sin_cos();
            for (r, y) in [(r0, y0), (r1, y1)] {
                positions.push([c * r, y, s * r]);
                normals.push([c * nr, ny, s * nr]);
                uvs.push([i as f32 / sides as f32, y]);
            }
        }
        for i in 0..sides as u32 {
            let (a, b) = (first + 2 * i, first + 2 * i + 2);
            indices.extend([a, a + 1, b, b, a + 1, b + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// One part of a thing: a shared mesh in one of the palette's materials, placed in the world, and whether it casts a shadow.
struct Part {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
    shadow: bool,
}
/// Lays out the parts of one thing in its own frame (`frame`: no scale, so that every part's place in the world is exact).
struct Make<'p> {
    f: Furniture,
    frame: Transform,
    parts: &'p mut Vec<Part>,
}
impl Make<'_> {
    fn part(&mut self, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, transform: Transform, shadow: bool) {
        self.parts.push(Part { mesh: mesh.clone(), material: material.clone(), transform: self.frame.mul_transform(transform), shadow });
    }
    /// A box by its middle and size.
    fn cube(&mut self, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3) {
        let mesh = self.f.cube.clone();
        self.part(&mesh, material, Transform::from_translation(at).with_scale(size), true)
    }
    fn detail(&mut self, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3) {
        let mesh = self.f.cube.clone();
        self.part(&mesh, material, Transform::from_translation(at).with_scale(size), false)
    }
    /// An upright cylinder standing on `y`, its axis through (x, z).
    fn post(&mut self, material: &Handle<StandardMaterial>, x: f32, y: f32, z: f32, radius: f32, height: f32) {
        let mesh = if radius < 0.03 { self.f.coarse.clone() } else { self.f.round.clone() };
        self.part(&mesh, material, Transform::from_xyz(x, y + height / 2., z).with_scale(Vec3::new(2. * radius, height, 2. * radius)), radius >= 0.012)
    }
    /// A cylinder from a to b.
    fn rod(&mut self, material: &Handle<StandardMaterial>, a: Vec3, b: Vec3, radius: f32, shadow: bool) {
        let (way, length) = ((b - a).normalize_or(Vec3::Y), a.distance(b));
        let mesh = self.f.coarse.clone();
        self.part(
            &mesh,
            material,
            Transform::from_translation((a + b) / 2.).with_rotation(Quat::from_rotation_arc(Vec3::Y, way)).with_scale(Vec3::new(2. * radius, length, 2. * radius)),
            shadow,
        )
    }
    fn ball(&mut self, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3, shadow: bool) {
        let mesh = self.f.ball.clone();
        self.part(&mesh, material, Transform::from_translation(at).with_scale(size), shadow)
    }
}

/// A small number from a block's place, so that like things differ (a jar's contents, a box's colours).
fn seed(rect: [f32; 4]) -> usize {
    ((rect[0] * 731. + rect[1] * 377. + rect[2] * 113.).abs() as usize).wrapping_mul(2654435761) >> 7
}

/// The meshes the last map's things were baked into (`build` replaces them), and how many parts went into them.
#[derive(Resource, Default)]
pub struct Baked {
    meshes: Vec<Handle<Mesh>>,
    pub parts: usize,
    pub things: Vec<&'static str>,
}

/// Draws a map's sinks, counter things, cabinets and bins (`level::build` draws the counter top round the sinks).
/// Their parts, hundreds of them on a big kitchen, are baked into one mesh for each material, with and without a
/// shadow: a few dozen meshes for the lot, so that a counter full of things costs the frame next to nothing.
pub fn build(world: &mut World, map: &Map) {
    world.init_resource::<Furniture>();
    world.init_resource::<Baked>();
    let f = world.resource::<Furniture>().clone();
    let old = std::mem::take(&mut world.resource_mut::<Baked>().meshes);
    let mut meshes = world.resource_mut::<Assets<Mesh>>();
    for mesh in old {
        meshes.remove(&mesh);
    }
    let (mut parts, mut things) = (vec![], vec![]);
    for sink in &map.sinks {
        things.push("Sink");
        basin(&mut parts, &f, sink.rect.map(|v| v as f32), sink.depth as f32);
    }
    for block in &map.blocks {
        if let Some(side) = block.facing.filter(|_| drawn(&block.style)) {
            things.push(thing(&mut parts, &f, block, side));
        }
    }
    for prop in &map.props {
        if let Prop::Bin { p, yaw } = prop {
            things.push("Bin");
            bin(&mut parts, &f, Vec3::new(p[0] as f32, 0., p[1] as f32), *yaw as f32);
        }
    }
    if map.cabinets {
        things.push("Fitted cabinets");
        cabinets(&mut parts, &f, map);
    }
    let count = parts.len();
    let baked = bake(world, parts);
    let mut record = world.resource_mut::<Baked>();
    (record.meshes, record.parts, record.things) = (baked, count, things);
}

/// Merges the parts into one mesh for each material and whether it casts a shadow, and spawns them.
fn bake(world: &mut World, parts: Vec<Part>) -> Vec<Handle<Mesh>> {
    let mut groups: Vec<(AssetId<StandardMaterial>, bool, Handle<StandardMaterial>, Mesh)> = vec![];
    {
        let meshes = world.resource::<Assets<Mesh>>();
        for part in parts {
            let Some(unit) = meshes.get(&part.mesh) else {
                continue;
            };
            let placed = unit.clone().transformed_by(part.transform);
            match groups.iter_mut().find(|(id, shadow, _, _)| *id == part.material.id() && *shadow == part.shadow) {
                Some((_, _, _, mesh)) => {
                    let _ = mesh.merge(&placed);
                }
                None => groups.push((part.material.id(), part.shadow, part.material.clone(), placed)),
            }
        }
    }
    let mut made = vec![];
    for (_, shadow, material, mesh) in groups {
        let handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let mut e = world.spawn((MapPart, Name::new("Kitchen things"), Mesh3d(handle.clone()), MeshMaterial3d(material), Transform::IDENTITY));
        if !shadow {
            e.insert(NotShadowCaster);
        }
        made.push(handle);
    }
    made
}

/// One counter thing in its block's box, its front on the block's `facing` side. Returns what it is.
fn thing(parts: &mut Vec<Part>, f: &Furniture, block: &Block, side: Side) -> &'static str {
    let rect = block.rect.map(|v| v as f32);
    let (transform, w, d) = crate::level::facing(rect, side);
    let h = block.height as f32;
    let name = match block.style.as_str() {
        "kettle" => "Kettle",
        "knife-block" => "Knife block",
        "canister" => "Canister",
        "jar" => "Jar",
        "cereal" => "Box of cereal",
        "paper-towel" => "Paper towels",
        "dish-rack" => "Dish rack",
        "spice-rack" => "Spice rack",
        "fruit-bowl" => "Fruit bowl",
        _ => "Tap",
    };
    let mut m = Make { f: f.clone(), frame: transform, parts };
    let n = seed(rect);
    match block.style.as_str() {
        "kettle" => kettle(&mut m, w, d, h, n),
        "knife-block" => knives(&mut m, w, d, h),
        "canister" => canister(&mut m, w, d, h, n),
        "jar" => jar(&mut m, w, d, h, n),
        "cereal" => cereal(&mut m, w, d, h, n),
        "paper-towel" => towels(&mut m, w, d, h),
        "dish-rack" => rack(&mut m, w, d, h, n),
        "spice-rack" => spices(&mut m, w, d, h, n),
        "fruit-bowl" => fruit(&mut m, w, d, h, n),
        _ => tap(&mut m, w, d, h),
    }
    name
}

/// A stovetop kettle in coloured enamel: round body, a shoulder up to a steel lid, a spout out of the front and a
/// handle over the top.
fn kettle(m: &mut Make, w: f32, d: f32, h: f32, n: usize) {
    let r = (w.min(d * 0.8) * 0.5) * 0.94;
    let z = d / 2. - r - 0.004;
    let enamel = m.f.paint[[RED, TEAL][n % 2]].clone();
    let (steel, black) = (m.f.steel.clone(), m.f.black.clone());
    m.post(&steel, 0., 0., z, r * 0.94, 0.03 * h);
    m.post(&enamel, 0., 0.03 * h, z, r, 0.52 * h);
    let shoulder = m.f.shoulder.clone();
    m.part(&shoulder, &enamel, Transform::from_xyz(0., 0.55 * h + 0.07 * h, z).with_scale(Vec3::new(2. * r, 0.14 * h, 2. * r)), true);
    m.post(&steel, 0., 0.69 * h, z, r * 0.62, 0.025 * h);
    m.ball(&black, Vec3::new(0., 0.73 * h, z), Vec3::new(0.24 * r, 0.16 * r, 0.24 * r), false);
    // The spout leaves the body low on its front and rises to the box's front edge.
    let (a, b) = (Vec3::new(0., 0.22 * h, z - r * 0.8), Vec3::new(0., 0.58 * h, -d / 2. + 0.012));
    m.rod(&enamel, a, b, r * 0.16, true);
    m.rod(&enamel, b, b + (b - a).normalize() * 0.012 + Vec3::Y * 0.004, r * 0.12, false);
    // The handle: posts up from the shoulder, and an arch between them.
    let span = r * 0.55;
    let top = h * 0.98 - span * 1.13;
    for s in [-1., 1.] {
        m.rod(&black, Vec3::new(0., 0.64 * h, z + s * span), Vec3::new(0., top, z + s * span), 0.006, false);
    }
    let arch = m.f.arch.clone();
    m.part(&arch, &black, Transform::from_xyz(0., top, z).with_scale(Vec3::splat(span)), true);
}

/// A wooden knife block leaning back, with the handles of five knives out of its top.
fn knives(m: &mut Make, w: f32, d: f32, h: f32) {
    let lean = 0.32;
    let tilt = Quat::from_rotation_x(lean);
    let (beech, black, steel) = (m.f.beech.clone(), m.f.black.clone(), m.f.steel.clone());
    let (bw, bh, bd) = (w * 0.94, h * 0.78, d * 0.62);
    // The block, turned about its bottom front edge so that it leans back, on a foot along the counter.
    let pivot = Vec3::new(0., 0., -d / 2. + 0.01);
    let place = |local: Vec3| pivot + tilt * (local - pivot);
    let middle = place(Vec3::new(0., bh / 2., -d / 2. + 0.01 + bd / 2.));
    let mesh = m.f.cube.clone();
    m.part(&mesh, &beech, Transform::from_translation(middle).with_rotation(tilt).with_scale(Vec3::new(bw, bh, bd)), true);
    m.cube(&beech, Vec3::new(0., 0.006, 0.), Vec3::new(bw, 0.012, d * 0.96));
    for (i, x) in [-0.36, -0.18, 0., 0.18, 0.36].into_iter().enumerate() {
        let depth = if i % 2 == 0 { 0.3 } else { 0.62 };
        let at = place(Vec3::new(x * bw, bh + 0.03, -d / 2. + 0.01 + bd * depth));
        let handle = Transform::from_translation(at).with_rotation(tilt).with_scale(Vec3::new(0.016, 0.075, 0.026));
        m.part(&mesh, &black, handle, true);
        let collar = Transform::from_translation(place(Vec3::new(x * bw, bh + 0.004, -d / 2. + 0.01 + bd * depth))).with_rotation(tilt).with_scale(Vec3::new(0.018, 0.008, 0.028));
        m.part(&mesh, &steel, collar, false);
    }
}

/// A kitchen canister: a lidded tub with a label band, in ceramic or steel.
fn canister(m: &mut Make, w: f32, d: f32, h: f32, n: usize) {
    let r = w.min(d) * 0.48;
    let steel = n % 3 == 0;
    let body = if steel { m.f.steel.clone() } else { m.f.ceramic.clone() };
    let band = m.f.paint[[BLUE, RED, GREEN, BROWN, TEAL][n % 5]].clone();
    m.post(&body, 0., 0., 0., r, 0.86 * h);
    m.post(&band, 0., 0.32 * h, 0., r * 1.012, 0.2 * h);
    let lid = if steel { m.f.black.clone() } else { m.f.walnut.clone() };
    m.post(&lid, 0., 0.86 * h, 0., r * 1.03, 0.1 * h);
    m.ball(&m.f.steel.clone(), Vec3::new(0., 0.97 * h, 0.), Vec3::new(r * 0.36, 0.05 * h, r * 0.36), false);
}

/// A glass jar of pasta, lentils, beans or rice with a screw lid.
fn jar(m: &mut Make, w: f32, d: f32, h: f32, n: usize) {
    let r = w.min(d) * 0.48;
    let contents = [m.f.paint[YELLOW].clone(), m.f.paint[ORANGE].clone(), m.f.paint[BROWN].clone(), m.f.paint[WHITE].clone(), m.f.paint[RED].clone()][n % 5].clone();
    let lid = [m.f.paint[RED].clone(), m.f.steel.clone(), m.f.paint[YELLOW].clone(), m.f.black.clone()][n % 4].clone();
    let glass = m.f.glass.clone();
    m.post(&contents, 0., 0.006, 0., r * 0.9, (0.45 + 0.08 * (n % 4) as f32) * h);
    let mesh = m.f.round.clone();
    m.part(&mesh, &glass, Transform::from_xyz(0., 0.43 * h, 0.).with_scale(Vec3::new(2. * r, 0.86 * h, 2. * r)), false);
    m.post(&lid, 0., 0.86 * h, 0., r * 0.96, 0.13 * h);
}

/// A tall box of cereal: its colour all round, a white panel with a bowl on the front and a band at the top.
fn cereal(m: &mut Make, w: f32, d: f32, h: f32, n: usize) {
    let [main, band, bowl] = [[RED, YELLOW, BLUE], [YELLOW, RED, BLUE], [BLUE, YELLOW, RED], [GREEN, ORANGE, BLUE]][n % 4];
    let (main, band, bowl) = (m.f.paint[main].clone(), m.f.paint[band].clone(), m.f.paint[bowl].clone());
    m.cube(&main, Vec3::new(0., h / 2., 0.), Vec3::new(w * 0.97, h, d * 0.94));
    let face = -d * 0.47 - 0.0015;
    m.detail(&band, Vec3::new(0., h * 0.86, face), Vec3::new(w * 0.97, h * 0.15, 0.002));
    m.detail(&m.f.paint[WHITE].clone(), Vec3::new(0., h * 0.86, face - 0.001), Vec3::new(w * 0.6, h * 0.08, 0.002));
    m.detail(&m.f.paint[WHITE].clone(), Vec3::new(0., h * 0.42, face), Vec3::new(w * 0.78, h * 0.44, 0.002));
    // The bowl, seen from the side, and flakes heaped in it.
    let disc = Transform::from_xyz(0., h * 0.34, face - 0.002).with_rotation(Quat::from_rotation_x(FRAC_PI_2)).with_scale(Vec3::new(w * 0.56, 0.002, h * 0.16));
    let mesh = m.f.coarse.clone();
    m.part(&mesh, &bowl, disc, false);
    for (x, y) in [(-0.14, 0.43), (0., 0.46), (0.13, 0.42), (-0.06, 0.5), (0.07, 0.5)] {
        m.ball(&m.f.paint[ORANGE].clone(), Vec3::new(x * w, y * h, face - 0.003), Vec3::new(0.022, 0.016, 0.004), false);
    }
}

/// A roll of paper towels on a wooden stand, its spindle through the middle and the end of the roll hanging loose.
fn towels(m: &mut Make, w: f32, d: f32, h: f32) {
    let r = w.min(d) * 0.46;
    let (beech, paper, card, steel) = (m.f.beech.clone(), m.f.paper.clone(), m.f.card.clone(), m.f.steel.clone());
    m.post(&beech, 0., 0., 0., r * 1.08, 0.022);
    m.post(&paper, 0., 0.024, 0., r, h * 0.86);
    m.post(&card, 0., 0.024 + h * 0.86, 0., r * 0.34, 0.003);
    m.post(&steel, 0., 0.02, 0., 0.007, h * 0.97);
    m.ball(&beech, Vec3::new(0., h * 0.97, 0.), Vec3::splat(0.026), true);
    m.detail(&paper, Vec3::new(0.01, h * 0.42, -r - 0.0015), Vec3::new(r * 1.3, h * 0.6, 0.002));
}

/// A dish rack: a drip tray, a frame of coated wire, plates standing in it and two mugs upside down at the end.
fn rack(m: &mut Make, w: f32, d: f32, h: f32, n: usize) {
    let (wire, ceramic) = (m.f.wire.clone(), m.f.ceramic.clone());
    m.cube(&wire, Vec3::new(0., 0.006, 0.), Vec3::new(w, 0.012, d));
    let top = h * 0.6;
    for x in [-w / 2. + 0.006, w / 2. - 0.006] {
        for z in [-d / 2. + 0.006, d / 2. - 0.006] {
            m.detail(&wire, Vec3::new(x, top / 2., z), Vec3::new(0.005, top, 0.005));
        }
    }
    for z in [-d / 2. + 0.006, d / 2. - 0.006] {
        m.detail(&wire, Vec3::new(0., top, z), Vec3::new(w, 0.005, 0.005));
        m.detail(&wire, Vec3::new(0., top * 0.45, z), Vec3::new(w, 0.004, 0.004));
    }
    for x in [-w / 2. + 0.006, w / 2. - 0.006] {
        m.detail(&wire, Vec3::new(x, top, 0.), Vec3::new(0.005, 0.005, d));
    }
    // Plates stand across the rack, faces along x, in the first two thirds of it.
    let radius = (h * 0.62).min(d * 0.44);
    let count = ((w * 0.62) / 0.035) as usize;
    let mesh = m.f.round.clone();
    for i in 0..count {
        let x = -w / 2. + 0.03 + i as f32 * 0.035;
        let tilt = Quat::from_rotation_z(FRAC_PI_2 - 0.12);
        let plate = Transform::from_xyz(x, 0.014 + radius, 0.).with_rotation(tilt).with_scale(Vec3::new(2. * radius, 0.007, 2. * radius));
        let colour = if (i + n) % 4 == 3 { m.f.paint[TEAL].clone() } else { ceramic.clone() };
        m.part(&mesh, &colour, plate, true);
    }
    // Two mugs upside down at the other end.
    let mugs = [m.f.paint[[RED, BLUE, YELLOW][n % 3]].clone(), ceramic.clone()];
    for (k, z) in [-d * 0.22, d * 0.22].into_iter().enumerate() {
        let x = w / 2. - 0.055;
        m.post(&mugs[k], x, 0.012, z, 0.038, (h * 0.62).min(0.095));
    }
}

/// A wall-hung rack of two shelves of little spice jars.
fn spices(m: &mut Make, w: f32, d: f32, h: f32, n: usize) {
    let walnut = m.f.walnut.clone();
    m.cube(&walnut, Vec3::new(0., h / 2., d / 2. - 0.006), Vec3::new(w, h, 0.012));
    for y in [0.006, h * 0.5] {
        m.cube(&walnut, Vec3::new(0., y, 0.), Vec3::new(w, 0.012, d));
    }
    for x in [-w / 2. + 0.006, w / 2. - 0.006] {
        m.cube(&walnut, Vec3::new(x, h / 2., 0.), Vec3::new(0.012, h, d));
    }
    for y in [h * 0.16, h * 0.66] {
        m.rod(&m.f.steel.clone(), Vec3::new(-w / 2., y, -d / 2. + 0.006), Vec3::new(w / 2., y, -d / 2. + 0.006), 0.003, false);
    }
    let count = ((w - 0.02) / 0.042) as usize;
    let tall = (h * 0.5 - 0.02).min(0.075);
    for shelf in 0..2 {
        let y = if shelf == 0 { 0.012 } else { h * 0.5 + 0.006 };
        for i in 0..count {
            let x = -w / 2. + 0.012 + 0.021 + i as f32 * 0.042;
            let colour = m.f.paint[[RED, GREEN, BROWN, YELLOW, ORANGE, CREAM, RED, GREEN][(i + shelf * 3 + n) % 8]].clone();
            m.post(&colour, x, y, 0., 0.016, tall * 0.78);
            let cap = if (i + shelf) % 2 == 0 { m.f.black.clone() } else { m.f.paint[WHITE].clone() };
            m.post(&cap, x, y + tall * 0.78, 0., 0.017, tall * 0.2);
        }
    }
}

/// A ceramic fruit bowl, heaped: oranges, apples and a banana over the rim.
fn fruit(m: &mut Make, w: f32, d: f32, h: f32, n: usize) {
    let r = w.min(d) * 0.5;
    let deep = h * 0.55;
    let bowl = m.f.bowl.clone();
    let colour = [m.f.ceramic.clone(), m.f.paint[TEAL].clone(), m.f.paint[BLUE].clone()][n % 3].clone();
    m.part(&bowl, &colour, Transform::from_scale(Vec3::new(2. * r, deep, 2. * r)), true);
    let size = (r * 0.36).min(0.04);
    let spots = [(-0.3, 0.2, 0), (0.28, 0.24, 1), (0., -0.32, 2), (-0.05, 0.02, 0), (0.33, -0.18, 1), (-0.32, -0.2, 0)];
    for (k, (x, z, kind)) in spots.into_iter().enumerate() {
        let lift = if k == 3 { deep + size * 0.6 } else { deep * 0.55 + size * 0.6 };
        let at = Vec3::new(x * r, lift.min(h - size), z * r);
        let material = m.f.fruit[kind].clone();
        m.ball(&material, at, Vec3::splat(2. * size), true);
        if kind > 0 {
            m.rod(&m.f.stem.clone(), at + Vec3::Y * size * 0.8, at + Vec3::Y * size * 1.25, 0.002, false);
        }
    }
    // The banana: two lengths at an angle, lying over the top of the heap.
    let banana = m.f.fruit[3].clone();
    let capsule = m.f.capsule.clone();
    let y = (deep + size * 0.9).min(h - 0.016);
    for (x, turn) in [(-r * 0.12, 0.25), (r * 0.18, -0.25)] {
        let t = Transform::from_xyz(x, y, r * 0.05).with_rotation(Quat::from_rotation_y(0.5) * Quat::from_rotation_z(FRAC_PI_2 + turn)).with_scale(Vec3::new(0.03, 0.05, 0.03));
        m.part(&capsule, &banana, t, true);
    }
}

/// A chrome tap: a round base, a column, a gooseneck arching over the sink in front and a lever on its side.
fn tap(m: &mut Make, w: f32, d: f32, h: f32) {
    let chrome = m.f.chrome.clone();
    let _ = (w, d);
    let reach = 0.1;
    let top = h - reach - 0.013;
    m.post(&chrome, 0., 0., 0., 0.03, 0.022);
    m.post(&chrome, 0., 0.02, 0., 0.013, top - 0.02);
    let arch = m.f.arch.clone();
    m.part(&arch, &chrome, Transform::from_xyz(0., top, -reach).with_scale(Vec3::splat(reach)), true);
    m.post(&chrome, 0., top - 0.03, -2. * reach, 0.016, 0.03);
    m.post(&m.f.hole.clone(), 0., top - 0.0305, -2. * reach, 0.011, 0.001);
    // The lever: a hub on the column's side and a handle angled up and back.
    let hub = Vec3::new(0.016, 0.1, 0.);
    m.rod(&chrome, Vec3::new(0.008, 0.1, 0.), hub + Vec3::X * 0.012, 0.012, true);
    m.rod(&chrome, hub + Vec3::X * 0.012, hub + Vec3::new(0.018, 0.035, 0.06), 0.005, true);
}

/// A sink's basin, sunk into the counter over `rect`: brushed steel floor and sides, the drain, and a flange round
/// the hole in the counter top.
fn basin(parts: &mut Vec<Part>, f: &Furniture, rect: [f32; 4], depth: f32) {
    let [x0, z0, x1, z1] = rect;
    let (cx, cz, w, d) = ((x0 + x1) / 2., (z0 + z1) / 2., x1 - x0, z1 - z0);
    let mut m = Make { f: f.clone(), frame: Transform::from_xyz(cx, FLOOR, cz), parts };
    let (steel, basin) = (f.steel.clone(), f.basin.clone());
    let (t, low) = (0.012, -depth);
    m.cube(&basin, Vec3::new(0., low - t / 2., 0.), Vec3::new(w + 2. * t, t, d + 2. * t));
    // The sides stand outside the hole, their inner faces on its edges, and stop a hair under the counter top.
    let wall = -0.001 - low;
    for s in [-1., 1.] {
        m.cube(&basin, Vec3::new(s * (w / 2. + t / 2.), low + wall / 2., 0.), Vec3::new(t, wall, d + 2. * t));
        m.cube(&basin, Vec3::new(0., low + wall / 2., s * (d / 2. + t / 2.)), Vec3::new(w, wall, t));
    }
    // A rounded-looking seam where the floor meets the sides.
    for s in [-1., 1.] {
        m.detail(&steel, Vec3::new(s * (w / 2. - 0.004), low + 0.004, 0.), Vec3::new(0.008, 0.008, d));
        m.detail(&steel, Vec3::new(0., low + 0.004, s * (d / 2. - 0.004)), Vec3::new(w, 0.008, 0.008));
    }
    // The drain: a steel ring, a dark hole and a cross over it.
    let hole = f.hole.clone();
    m.post(&steel, 0., low, 0., 0.045, 0.003);
    m.post(&hole, 0., low + 0.0005, 0., 0.034, 0.003);
    for turn in [0., FRAC_PI_2] {
        let mesh = f.cube.clone();
        m.part(&mesh, &steel, Transform::from_xyz(0., low + 0.0035, 0.).with_rotation(Quat::from_rotation_y(turn)).with_scale(Vec3::new(0.066, 0.002, 0.006)), false);
    }
    // The flange on the counter round the hole.
    let (lip, rim) = (0.003, 0.028);
    m.cube(&steel, Vec3::new(0., lip / 2., -(d / 2. + rim / 2.)), Vec3::new(w + 2. * rim, lip, rim));
    m.cube(&steel, Vec3::new(0., lip / 2., d / 2. + rim / 2.), Vec3::new(w + 2. * rim, lip, rim));
    m.cube(&steel, Vec3::new(-(w / 2. + rim / 2.), lip / 2., 0.), Vec3::new(rim, lip, d));
    m.cube(&steel, Vec3::new(w / 2. + rim / 2., lip / 2., 0.), Vec3::new(rim, lip, d));
}

/// A pedal bin on the kitchen floor: an oval of brushed steel with a black base, a lid and the pedal at its front.
fn bin(parts: &mut Vec<Part>, f: &Furniture, at: Vec3, yaw: f32) {
    let mut m = Make { f: f.clone(), frame: Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)), parts };
    let (steel, black) = (f.steel.clone(), f.black.clone());
    let (w, d, h) = (0.34, 0.3, 0.6);
    let round = f.round.clone();
    let oval = |y: f32, height: f32, grow: f32| Transform::from_xyz(0., y + height / 2., 0.).with_scale(Vec3::new(w + grow, height, d + grow));
    m.part(&round, &black, oval(0., 0.03, -0.012), true);
    m.part(&round, &steel, oval(0.03, h - 0.06, 0.), true);
    m.part(&round, &black, oval(h - 0.03, 0.012, 0.006), true);
    m.part(&round, &steel, oval(h - 0.018, 0.022, 0.008), true);
    m.ball(&steel, Vec3::new(0., h + 0.004, 0.), Vec3::new(w * 0.96, 0.02, d * 0.96), false);
    m.cube(&black, Vec3::new(0., h - 0.02, d / 2. + 0.006), Vec3::new(0.12, 0.03, 0.018));
    m.cube(&black, Vec3::new(0., 0.018, -d / 2. - 0.03), Vec3::new(0.11, 0.014, 0.07));
    m.cube(&black, Vec3::new(0., 0.03, -d / 2. + 0.01), Vec3::new(0.05, 0.03, 0.04));
}

/// Fitted cabinets: under the counter, doors and drawers down its front and a panel at each end over a black plinth;
/// on the wall, the cupboards the room already has dressed as pairs of panelled doors.
fn cabinets(parts: &mut Vec<Part>, f: &Furniture, map: &Map) {
    let [x0, z0, x1, z1] = map.bounds.map(|v| v as f32);
    let mut m = Make { f: f.clone(), frame: Transform::IDENTITY, parts };
    let (door, panel, carcass, plinth, steel) = (f.door.clone(), f.panel.clone(), f.carcass.clone(), f.plinth.clone(), f.steel.clone());
    let under = FLOOR - SLAB;
    let (kick, face) = (0.1, z0 + 0.004);
    // The carcass: a front and two ends (the back is the room's own cupboard), and the recessed plinth.
    m.cube(&carcass, Vec3::new((x0 + x1) / 2., (kick + under) / 2., face + 0.01), Vec3::new(x1 - x0, under - kick, 0.02));
    m.cube(&plinth, Vec3::new((x0 + x1) / 2., kick / 2., face + 0.06), Vec3::new(x1 - x0 - 0.1, kick, 0.02));
    for (x, out) in [(x0, -1.), (x1, 1.)] {
        m.cube(&door, Vec3::new(x + out * 0.009, (kick + under) / 2., (z0 + z1) / 2.), Vec3::new(0.018, under - kick, z1 - z0 + 0.01));
        m.cube(&plinth, Vec3::new(x - out * 0.05, kick / 2., (z0 + z1) / 2. + 0.03), Vec3::new(0.02, kick, z1 - z0 - 0.06));
        // The end panel's frame: an inset panel a shade lighter.
        m.detail(&panel, Vec3::new(x + out * 0.0185, (kick + under) / 2., (z0 + z1) / 2.), Vec3::new(0.002, under - kick - 0.14, z1 - z0 - 0.14));
    }
    // The fronts, a unit about 0.6 m wide at a time: a drawer over a door, or two doors under a sink.
    let units = ((x1 - x0) / 0.6).round().max(1.) as usize;
    let pitch = (x1 - x0) / units as f32;
    let (gap, drawer) = (0.004, 0.15);
    let front = face - 0.009;
    for i in 0..units {
        let (a, b) = (x0 + pitch * i as f32, x0 + pitch * (i + 1) as f32);
        let mid = (a + b) / 2.;
        let sink = map.sinks.iter().any(|s| (s.rect[0] as f32) < b && (s.rect[2] as f32) > a);
        let split = under - drawer;
        let (top_low, top_high) = (split + gap / 2., under - gap / 2.);
        m.cube(&door, Vec3::new(mid, (top_low + top_high) / 2., front), Vec3::new(pitch - gap, top_high - top_low, 0.018));
        if !sink {
            m.detail(&steel, Vec3::new(mid, (top_low + top_high) / 2., front - 0.016), Vec3::new(0.16, 0.012, 0.012));
        }
        let (low, high) = (kick + gap / 2., split - gap / 2.);
        let leaves: Vec<(f32, f32, f32)> = if sink || pitch > 0.75 {
            vec![(a, mid, 1.), (mid, b, -1.)]
        } else {
            vec![(a, b, if i % 2 == 0 { 1. } else { -1. })]
        };
        for (l, r, handle) in leaves {
            let middle = (l + r) / 2.;
            m.cube(&door, Vec3::new(middle, (low + high) / 2., front), Vec3::new(r - l - gap, high - low, 0.018));
            // A shaker door: an inset panel a shade lighter, and a bar handle near the edge it opens from.
            m.detail(&panel, Vec3::new(middle, (low + high) / 2., front - 0.0095), Vec3::new(r - l - gap - 0.11, high - low - 0.11, 0.002));
            let edge = if handle > 0. { r - 0.045 } else { l + 0.045 };
            m.detail(&steel, Vec3::new(edge, high - 0.12, front - 0.016), Vec3::new(0.012, 0.13, 0.012));
        }
    }
    // The wall cupboards: each the room's box of wood, given two panelled doors, a gap between them and two knobs.
    let room = crate::fridge::Room::fitting(map.bounds);
    for (x, width) in crate::fridge::wall_doors(&room) {
        let z = 3.345 + (room.back - crate::fridge::Room::default().back) - 0.001;
        m.detail(&carcass, Vec3::new(x, 1.95, z), Vec3::new(0.006, 0.7, 0.002));
        for s in [-1., 1.] {
            m.detail(&panel, Vec3::new(x + s * width / 4., 1.97, z - 0.001), Vec3::new(width / 2. - 0.09, 0.52, 0.002));
            m.ball(&steel, Vec3::new(x + s * 0.03, 1.72, z - 0.012), Vec3::new(0.022, 0.022, 0.022), false);
        }
    }
    let _ = z1;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world
    }
    fn kitchen() -> Map {
        Map::parse(include_str!("../assets/maps/the-whole-kitchen.json")).unwrap()
    }

    #[test]
    fn the_counter_top_has_a_hole_for_each_sink_and_is_the_one_slab_it_always_was_without() {
        let bundled = Map::bundled();
        let [x0, z0, x1, z1] = bundled.bounds;
        assert_eq!(counter_top(&bundled), vec![[x0 - 0.025, z0 - 0.02, x1 + 0.025, z1 + 0.02]]);
        let map = kitchen();
        let pieces = counter_top(&map);
        let [sx0, sz0, sx1, sz1] = map.sinks[0].rect;
        // The pieces cover the outline but the hole, exactly once.
        let area: f64 = pieces.iter().map(|r| (r[2] - r[0]) * (r[3] - r[1])).sum();
        let [x0, z0, x1, z1] = map.bounds;
        let whole = (x1 - x0 + 0.05) * (z1 - z0 + 0.04) - (sx1 - sx0) * (sz1 - sz0);
        assert!((area - whole).abs() < 1e-3, "{area} {whole}");
        let inside = |x: f64, z: f64| pieces.iter().filter(|r| x > r[0] && x < r[2] && z > r[1] && z < r[3]).count();
        assert_eq!((inside(0., 3.4), inside(-2., 3.), inside(0., 3.9), inside(0., 3.)), (0, 1, 1, 1));
    }

    #[test]
    fn the_whole_kitchen_is_furnished_from_shared_meshes_and_materials_and_rebuilding_it_adds_none() {
        let mut world = world();
        crate::level::build(&mut world, &kitchen());
        let things = world.resource::<Baked>().things.clone();
        let named = |name: &str| things.iter().filter(|n| **n == name).count();
        assert_eq!((named("Sink"), named("Tap"), named("Bin"), named("Fitted cabinets"), named("Kettle"), named("Spice rack")), (1, 1, 1, 1, 1, 1));
        let counted = kitchen().blocks.iter().filter(|b| drawn(&b.style)).count();
        assert!(counted > 20 && things.len() == 1 + counted + 1 + 1, "every counter thing (the blocks but the appliances) and the rest: {things:?}");
        let (meshes, materials) = (world.resource::<Assets<Mesh>>().len(), world.resource::<Assets<StandardMaterial>>().len());
        let entities = world.query::<()>().iter(&world).count();
        let baked = world.query::<&Name>().iter(&world).filter(|n| n.as_str() == "Kitchen things").count();
        // Hundreds of parts, baked into a few dozen meshes.
        assert!(world.resource::<Baked>().parts > 300 && baked < 48, "{} parts in {baked} meshes", world.resource::<Baked>().parts);
        // Another map takes them away; the kitchen again bakes them anew, in place of none.
        crate::level::build(&mut world, &Map::bundled());
        assert_eq!(world.query::<&Name>().iter(&world).filter(|n| n.as_str() == "Kitchen things").count(), 0);
        crate::level::build(&mut world, &kitchen());
        assert_eq!(world.query::<()>().iter(&world).count(), entities);
        assert_eq!((world.resource::<Assets<Mesh>>().len(), world.resource::<Assets<StandardMaterial>>().len()), (meshes, materials));
    }

    #[test]
    fn a_ragdoll_meets_the_sink_floor_in_a_sink_and_the_cabinets_below_the_counter() {
        let map = kitchen();
        let scene = crate::death::scenery(&map);
        let depth = map.sinks[0].depth as f32;
        assert_eq!(scene.ground(Vec3::new(0., FLOOR + 0.2, 3.3)), Some(FLOOR - depth));
        assert_eq!(scene.ground(Vec3::new(-2., FLOOR + 0.2, 2.5)), Some(FLOOR));
        // Off the front of the counter, the fitted cabinets stand in the way down to the floor.
        assert!(!scene.clear(Vec3::new(0., 0.4, 2.12), 0.02, 0.));
        let bundled = crate::death::scenery(&Map::bundled());
        assert!(bundled.clear(Vec3::new(0., 0.4, 2.2), 0.02, 0.), "a counter without cabinets is open underneath, as it was");
    }

    #[test]
    fn the_lathe_and_the_arch_face_outwards() {
        for mesh in [lathe(&[[0., 0.], [0.5, 0.], [0.5, 1.], [0., 1.]], 16), arch()] {
            let positions: Vec<Vec3> = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap().iter().map(|p| Vec3::from_array(*p)).collect();
            let normals: Vec<Vec3> = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap().as_float3().unwrap().iter().map(|p| Vec3::from_array(*p)).collect();
            let Some(Indices::U32(indices)) = mesh.indices() else { panic!("indexed") };
            // (Triangles that meet the axis in a point have no area, and no winding.)
            let (mut agree, mut all) = (0, 0);
            for t in indices.chunks(3) {
                let [a, b, c] = [0, 1, 2].map(|i| positions[t[i] as usize]);
                let winding = (b - a).cross(c - a);
                if winding.length() > 1e-9 {
                    all += 1;
                    if winding.normalize().dot(normals[t[0] as usize]) > 0. {
                        agree += 1;
                    }
                }
            }
            assert!(agree * 10 >= all * 9, "{agree} of {all} triangles wound with their normals");
        }
    }
}

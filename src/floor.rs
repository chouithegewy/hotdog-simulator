// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The kitchen floor: a map whose ground is the kitchen floor rather than a counter top (`Map::floor`: Titan's Kitchen,
//! `assets/maps/titans-kitchen.json`) is drawn here, at true scale, as the same kitchen the counter maps stand in, seen
//! from the floor at night (`level::build` calls `build`, and leaves to it what it draws).
//!
//! * **The room**: tiles 30 cm square ([`TILE`], a texture made here with its mip levels, [`tiles`]); walls standing at the
//!   map's bounds in the kitchen's own paint (`fridge::build_room`'s), with a skirting board; a ceiling [`HEIGHT`] up, out of
//!   any giant's reach, with a flush lamp (off); and a window in the back wall (+z) over the sink ([`window`]), the night
//!   sky and the moon beyond it. **Moonlight** comes in through it ([`MOON`]: a spot light outside, casting shadows, so the
//!   window's bars and every leg fall across the floor as shadows), and the room is otherwise dark.
//! * **The cabinets**: a tall wood block (the carcass, its top the worktop: [`cabinet`]) with a wood box standing proud of
//!   it from the toe-kick up ([`front`]: doors and drawers, handles) and a stone lip ([`lip`]): drawn as the counter maps'
//!   fitted cabinets, the plinth black and recessed under the doors (the toe-kick small hotdogs hide in), the worktop
//!   stone, a white tiled splashback over it, wall cupboards above, a sink and tap under the window, a hood over the oven.
//! * **The floor's kits** ([`KITS`]): the **fridge**, its door a little ajar, light spilling from the gap (a spot light
//!   behind the door, so the door's own shadow shapes a wedge of light on the floor, and a soft glow); the **oven**, a
//!   freestanding range (oven door with its window, knobs, the hob's four rings, a clock glowing green); a **pet's bowl**.
//! * **Decks** stand on the floor as solid things: the cardboard box (`card`: flaps folded down, tape across its top) and
//!   the step stool (`white`: grey treads), any other a plain block of its style.
//!
//! Its two lights keep their own brightness ([`Lamp`]: the theme does not turn them down in a round, `visuals`). The parts
//! are merged into one mesh for each material (as `kitchen` does), so the room costs a few dozen draws.
use crate::level::MapPart;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use kitchen_sim::map::{Block, Deck, Map, Prop, Side};
use std::f32::consts::{FRAC_PI_2, PI};

const FLOOR: f32 = 0.9025;
/// How high the ceiling's underside is above the floor: out of the titan's reach (1.8 m and a hop of under 0.3 m).
pub const HEIGHT: f32 = 2.7;
/// The size of a floor tile.
pub const TILE: f32 = 0.3;
/// The walls' thickness, outside the bounds.
const WALL: f32 = 0.12;
/// The window in the back wall: its width, the sill's and the top's heights above the floor.
const WINDOW: [f32; 3] = [1.2, 1.05, 2.15];
/// The kits that stand on the kitchen floor and are drawn here.
pub const KITS: [&str; 3] = ["fridge", "oven", "pet-bowl"];
/// The moon's light: lumens of the spot outside the window, its colour, and where it comes from (a unit vector from the
/// window out to the moon: up 32 degrees, a little to the right).
const MOON: f32 = 70_000.;
const MOON_COLOR: Color = Color::srgb(0.62, 0.74, 1.0);
const MOON_WAY: Vec3 = Vec3::new(0.26, 0.53, 0.81);
/// The fridge's light: the spot behind its door, and the soft glow at its gap.
const FRIDGE: f32 = 9_000.;
const FRIDGE_GLOW: f32 = 1_500.;
/// The night's fill: moonlight thrown back off the walls and the ceiling, faint and cool, so that the room reads (four
/// wide lights under the ceiling, one over each quarter of the room, shining down and casting nothing).
const FILL: f32 = 6_000.;
const FILL_COLOR: Color = Color::srgb(0.55, 0.65, 0.95);
/// Under the wall cupboards, a strip of warm light left on over the worktop: lumens of each run's lamp (no shadows).
const UNDER: f32 = 260.;
const UNDER_COLOR: Color = Color::srgb(1.0, 0.78, 0.55);
/// The fridge's body, as deep as a fridge is: the rest of its block, in front, is where its door swings.
const FRIDGE_BODY: f32 = 0.68;
const FRIDGE_COLOR: Color = Color::srgb(0.86, 0.93, 1.0);
/// How far the fridge's door stands open.
const AJAR: f32 = 0.38;

/// A light of the kitchen floor's: the theme leaves its brightness alone (`visuals::tune_room_lights`).
#[derive(Component)]
pub struct Lamp;

/// Whether a block is a cabinet's carcass on a floor map: wood, as tall as a counter.
pub fn cabinet(map: &Map, block: &Block) -> bool {
    map.floor && block.style == "wood" && block.height >= 0.85
}
/// Whether a box is a cabinet's front on a floor map: wood, from a toe-kick's height up to under the worktop.
pub fn front(map: &Map, prop: &Prop) -> bool {
    matches!(prop, Prop::Box { base, height, style, .. } if map.floor && style == "wood" && *base >= 0.1 && *base < 0.4 && base + height >= 0.8)
}
/// Whether a box is a worktop's lip on a floor map: a thin slab of stone at counter height.
pub fn lip(map: &Map, prop: &Prop) -> bool {
    matches!(prop, Prop::Box { base, height, style, .. } if map.floor && style == "stone" && *base >= 0.8 && *height <= 0.08)
}
/// The side of a fridge (-1 or 1, along x in its own frame, as `level::facing` builds it: its front towards -z) that a
/// block is its door on: a thin steel block standing wide open against its front at one side, as far out as the fridge
/// is wide. None for any other block.
pub fn door_side(fridge: &Block, block: &Block) -> Option<f32> {
    let side = fridge.facing.filter(|_| fridge.style == "fridge" && block.style == "steel")?;
    let r = block.rect.map(|v| v as f32);
    if (r[2] - r[0]).min(r[3] - r[1]) > 0.08 {
        return None;
    }
    let (frame, w, d) = crate::level::facing(fridge.rect.map(|v| v as f32), side);
    let inverse = frame.compute_affine().inverse();
    let corners = [[r[0], r[1]], [r[2], r[1]], [r[0], r[3]], [r[2], r[3]]].map(|[x, z]| inverse.transform_point3(Vec3::new(x, FLOOR, z)));
    let (lo, hi) = corners.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
    let front = -d / 2.;
    if hi.z > front + 0.01 || lo.z < front - w - 0.1 {
        return None;
    }
    if (lo.x - w / 2.).abs() <= 0.02 {
        Some(1.)
    } else if (hi.x + w / 2.).abs() <= 0.02 {
        Some(-1.)
    } else {
        None
    }
}
/// Whether a block is a fridge's open door (`door_side`).
pub fn fridge_door(map: &Map, block: &Block) -> bool {
    map.floor && map.blocks.iter().any(|f| door_side(f, block).is_some())
}
/// Whether `level` leaves a block to this module.
pub fn drawn_block(map: &Map, block: &Block) -> bool {
    map.floor && (KITS.contains(&block.style.as_str()) || cabinet(map, block) || fridge_door(map, block))
}
/// Whether `level` leaves a prop to this module.
pub fn drawn_prop(map: &Map, prop: &Prop) -> bool {
    front(map, prop) || lip(map, prop)
}
/// Where the window is on the back wall: its middle along x (three tenths of the way across, over the sink).
pub fn window(map: &Map) -> f32 {
    let [x0, _, x1, _] = map.bounds.map(|v| v as f32);
    x0 + 0.3 * (x1 - x0)
}

/// Shared meshes and materials, made once.
#[derive(Resource, Clone)]
struct Kit {
    cube: Handle<Mesh>,
    round: Handle<Mesh>,
    ball: Handle<Mesh>,
    bowl: Handle<Mesh>,
    tiles: Handle<Image>,
    wall: Handle<StandardMaterial>,
    skirting: Handle<StandardMaterial>,
    ceiling: Handle<StandardMaterial>,
    splash: Handle<StandardMaterial>,
    frame: Handle<StandardMaterial>,
    glass: Handle<StandardMaterial>,
    sky: Handle<StandardMaterial>,
    moon: Handle<StandardMaterial>,
    star: Handle<StandardMaterial>,
    door: Handle<StandardMaterial>,
    panel: Handle<StandardMaterial>,
    carcass: Handle<StandardMaterial>,
    plinth: Handle<StandardMaterial>,
    stone: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    chrome: Handle<StandardMaterial>,
    black: Handle<StandardMaterial>,
    shell: Handle<StandardMaterial>,
    liner: Handle<StandardMaterial>,
    gasket: Handle<StandardMaterial>,
    enamel: Handle<StandardMaterial>,
    oven_glass: Handle<StandardMaterial>,
    clock: Handle<StandardMaterial>,
    kibble: Handle<StandardMaterial>,
    water: Handle<StandardMaterial>,
    card: Handle<StandardMaterial>,
    tape: Handle<StandardMaterial>,
    plastic: Handle<StandardMaterial>,
    tread: Handle<StandardMaterial>,
    lampshade: Handle<StandardMaterial>,
    strip: Handle<StandardMaterial>,
    styles: Vec<(&'static str, Handle<StandardMaterial>)>,
}
impl FromWorld for Kit {
    fn from_world(world: &mut World) -> Self {
        let tiles = world.resource_mut::<Assets<Image>>().add(tiles(256));
        let (cube, round, ball, bowl) = {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            (
                meshes.add(Cuboid::default()),
                meshes.add(Cylinder::new(0.5, 1.).mesh().resolution(28)),
                meshes.add(Sphere::new(0.5).mesh().uv(18, 12)),
                // A pet's bowl, 1 across its foot and 1 high: a wide foot narrowing up to the rim, a well inside.
                meshes.add(crate::kitchen::lathe(&[[0., 0.], [0.5, 0.], [0.46, 0.3], [0.4, 1.], [0.36, 1.], [0.3, 0.32], [0., 0.3]], 32)),
            )
        };
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        let mut plain = |r: f32, g: f32, b: f32, roughness: f32, metallic: f32| {
            mats.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: roughness, metallic, ..default() })
        };
        // The kitchen's own paint, wood and stone (`fridge::build_room`, `kitchen::Furniture`).
        let wall = plain(0.54, 0.52, 0.44, 0.94, 0.);
        let skirting = plain(0.86, 0.84, 0.78, 0.5, 0.);
        let ceiling = plain(0.6, 0.59, 0.55, 0.95, 0.);
        let splash = plain(0.88, 0.89, 0.86, 0.18, 0.);
        let frame = plain(0.9, 0.9, 0.87, 0.45, 0.);
        let door = plain(0.24, 0.13, 0.07, 0.6, 0.);
        let panel = plain(0.3, 0.17, 0.09, 0.55, 0.);
        let carcass = plain(0.16, 0.09, 0.05, 0.7, 0.);
        let plinth = plain(0.03, 0.03, 0.03, 0.7, 0.);
        let stone = plain(0.35, 0.38, 0.36, 0.38, 0.);
        let steel = plain(0.66, 0.68, 0.70, 0.32, 0.85);
        let chrome = plain(0.85, 0.87, 0.9, 0.08, 1.);
        let black = plain(0.03, 0.03, 0.032, 0.45, 0.);
        // The fridge as `fridge::build` paints it.
        let shell = plain(0.57, 0.61, 0.60, 0.31, 0.);
        let gasket = plain(0.12, 0.15, 0.14, 0.88, 0.);
        let enamel = plain(0.06, 0.065, 0.07, 0.3, 0.2);
        let kibble = plain(0.42, 0.24, 0.1, 0.8, 0.);
        let card = plain(0.6, 0.44, 0.27, 0.92, 0.);
        let tape = plain(0.72, 0.58, 0.38, 0.3, 0.);
        let plastic = plain(0.93, 0.93, 0.9, 0.4, 0.);
        let tread = plain(0.32, 0.34, 0.35, 0.9, 0.);
        let styles = vec![
            ("wood", plain(0.52, 0.33, 0.17, 0.6, 0.)),
            ("dark", plain(0.05, 0.065, 0.07, 0.45, 0.)),
            ("red", plain(0.70, 0.12, 0.07, 0.45, 0.)),
            ("green", plain(0.30, 0.52, 0.22, 0.55, 0.)),
            ("yellow", plain(0.98, 0.71, 0.12, 0.55, 0.)),
            ("metal", plain(0.30, 0.39, 0.40, 0.6, 0.)),
            ("stone", stone.clone()),
            ("card", card.clone()),
            ("white", plastic.clone()),
        ];
        let glow = |mats: &mut Assets<StandardMaterial>, color: Color, emissive: LinearRgba| {
            mats.add(StandardMaterial { base_color: color, emissive, unlit: true, ..default() })
        };
        let sky = glow(&mut mats, Color::srgb(0.05, 0.08, 0.18), LinearRgba::rgb(0.07, 0.11, 0.26));
        let moon = glow(&mut mats, Color::srgb(0.9, 0.93, 1.0), LinearRgba::rgb(6., 6.6, 8.));
        let star = glow(&mut mats, Color::srgb(0.8, 0.85, 1.0), LinearRgba::rgb(1.5, 1.6, 2.));
        let clock = glow(&mut mats, Color::srgb(0.35, 1., 0.53), LinearRgba::rgb(0.3, 3., 0.7));
        let strip = glow(&mut mats, Color::srgb(1., 0.85, 0.62), LinearRgba::rgb(4., 2.9, 1.8));
        let liner = mats.add(StandardMaterial { base_color: Color::srgb(0.77, 0.81, 0.79), emissive: LinearRgba::rgb(1.6, 1.8, 2.0), perceptual_roughness: 0.4, ..default() });
        let lampshade = mats.add(StandardMaterial { base_color: Color::srgb(0.82, 0.82, 0.8), perceptual_roughness: 0.3, ..default() });
        let glass = mats.add(StandardMaterial {
            base_color: Color::srgba(0.7, 0.8, 0.95, 0.1),
            emissive: LinearRgba::rgb(0.12, 0.18, 0.34),
            perceptual_roughness: 0.04,
            reflectance: 0.6,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let oven_glass = mats.add(StandardMaterial { base_color: Color::srgb(0.03, 0.035, 0.04), perceptual_roughness: 0.06, reflectance: 0.7, ..default() });
        let water = mats.add(StandardMaterial { base_color: Color::srgba(0.35, 0.5, 0.6, 0.55), perceptual_roughness: 0.03, alpha_mode: AlphaMode::Blend, ..default() });
        Kit {
            cube,
            round,
            ball,
            bowl,
            tiles,
            wall,
            skirting,
            ceiling,
            splash,
            frame,
            glass,
            sky,
            moon,
            star,
            door,
            panel,
            carcass,
            plinth,
            stone,
            steel,
            chrome,
            black,
            shell,
            liner,
            gasket,
            enamel,
            oven_glass,
            clock,
            kibble,
            water,
            card,
            tape,
            plastic,
            tread,
            lampshade,
            strip,
            styles,
        }
    }
}
impl Kit {
    fn style(&self, name: &str) -> Handle<StandardMaterial> {
        self.styles.iter().find(|(n, _)| *n == name).map_or(self.styles[0].1.clone(), |(_, m)| m.clone())
    }
}

/// The floor tiles, a texture `size` pixels square holding two by two tiles: cream ceramic, each a shade of its own, a
/// faint mottle, grout lines between them. With all its mip levels, so the grout does not shimmer far off.
pub fn tiles(size: u32) -> Image {
    let n = size as usize;
    let mut base = vec![[0f32; 3]; n * n];
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((x as f32 + 0.5) / n as f32 * 2., (y as f32 + 0.5) / n as f32 * 2.);
            let (i, j) = (u.floor() as u32, v.floor() as u32);
            let (fu, fv) = (u.fract(), v.fract());
            let edge = fu.min(1. - fu).min(fv.min(1. - fv));
            let grout = (1. - ((edge - 0.012) / 0.012).clamp(0., 1.)).powi(2);
            let hash = |a: u32| (a.wrapping_mul(2_654_435_761) >> 16) as f32 / 65535.;
            let shade = 0.94 + 0.08 * hash(i * 31 + j * 17 + 7);
            let mottle = 0.97 + 0.05 * hash((x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663));
            let tile = [0.64, 0.6, 0.52].map(|c| c * shade * mottle);
            let gap = [0.3, 0.28, 0.24];
            base[y * n + x] = [0, 1, 2].map(|k| tile[k] * (1. - grout) + gap[k] * grout);
        }
    }
    // The levels, each a box filter of the one before, in linear light.
    let mut data = vec![];
    let (mut level, mut side) = (base.iter().map(|c| c.map(|v| v.powf(2.2))).collect::<Vec<_>>(), n);
    let mut count = 0;
    loop {
        for c in &level {
            data.extend(c.map(|v| (v.powf(1. / 2.2).clamp(0., 1.) * 255.).round() as u8));
            data.push(255);
        }
        count += 1;
        if side == 1 {
            break;
        }
        let half = side / 2;
        level = (0..half * half)
            .map(|id| {
                let (x, y) = (id % half * 2, id / half * 2);
                let [a, b, c, d] = [level[y * side + x], level[y * side + x + 1], level[(y + 1) * side + x], level[(y + 1) * side + x + 1]];
                [0, 1, 2].map(|k| (a[k] + b[k] + c[k] + d[k]) / 4.)
            })
            .collect();
        side = half;
    }
    let mut image = Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    image.texture_descriptor.mip_level_count = count;
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

/// A flat rectangle facing up over [x0, z0, x1, z1] at height y, its texture coordinates in pairs of tiles.
fn floor_mesh(rect: [f32; 4], y: f32) -> Mesh {
    let [x0, z0, x1, z1] = rect;
    let uv = |x: f32, z: f32| [(x - x0) / (2. * TILE), (z - z0) / (2. * TILE)];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 1., 0.]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![uv(x0, z0), uv(x1, z0), uv(x1, z1), uv(x0, z1)])
        .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}

/// One part of the room, merged with the others of its material.
struct Part {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
    shadow: bool,
}
/// Builds parts in a frame (an appliance's: its front towards -z).
struct Make<'p> {
    kit: Kit,
    frame: Transform,
    parts: &'p mut Vec<Part>,
}
impl Make<'_> {
    fn part(&mut self, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, transform: Transform, shadow: bool) {
        self.parts.push(Part { mesh: mesh.clone(), material: material.clone(), transform: self.frame.mul_transform(transform), shadow });
    }
    /// A box about its middle, casting a shadow.
    fn cube(&mut self, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3) {
        let mesh = self.kit.cube.clone();
        self.part(&mesh, material, Transform::from_translation(at).with_scale(size), true)
    }
    /// A small box that casts no shadow.
    fn detail(&mut self, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3) {
        let mesh = self.kit.cube.clone();
        self.part(&mesh, material, Transform::from_translation(at).with_scale(size), false)
    }
    /// A box between two corners.
    fn span(&mut self, material: &Handle<StandardMaterial>, low: Vec3, high: Vec3, shadow: bool) {
        let mesh = self.kit.cube.clone();
        self.part(&mesh, material, Transform::from_translation((low + high) / 2.).with_scale(high - low), shadow)
    }
    /// An upright cylinder standing at (x, y, z).
    fn post(&mut self, material: &Handle<StandardMaterial>, at: Vec3, radius: f32, height: f32, shadow: bool) {
        let mesh = self.kit.round.clone();
        self.part(&mesh, material, Transform::from_translation(at + Vec3::Y * height / 2.).with_scale(Vec3::new(2. * radius, height, 2. * radius)), shadow)
    }
    /// A cylinder lying along x (a knob's stem along z when turned).
    fn rod(&mut self, material: &Handle<StandardMaterial>, a: Vec3, b: Vec3, radius: f32, shadow: bool) {
        let (way, length) = ((b - a).normalize_or(Vec3::Y), a.distance(b));
        let mesh = self.kit.round.clone();
        let at = Transform::from_translation((a + b) / 2.).with_rotation(Quat::from_rotation_arc(Vec3::Y, way)).with_scale(Vec3::new(2. * radius, length, 2. * radius));
        self.part(&mesh, material, at, shadow)
    }
}

/// What a floor map's build made: its meshes (the merged parts and the floor), freed when the map is rebuilt, and what it
/// drew, by name (for tests).
#[derive(Resource, Default)]
pub struct Built {
    meshes: Vec<Handle<Mesh>>,
    pub things: Vec<&'static str>,
}

/// Draws a floor map's room, cabinets, kits and decks, and lights it (see the module's documentation). Nothing on any
/// other map.
pub fn build(world: &mut World, map: &Map) {
    world.init_resource::<Built>();
    let old = std::mem::take(&mut world.resource_mut::<Built>().meshes);
    {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        for mesh in old {
            meshes.remove(&mesh);
        }
    }
    world.resource_mut::<Built>().things.clear();
    if !map.floor {
        return;
    }
    world.init_resource::<Kit>();
    let kit = world.resource::<Kit>().clone();
    let (mut parts, mut things) = (vec![], vec![]);
    let mut made = vec![];
    // The tiles: one rectangle over the bounds, textured, on a slab so that nothing below shows.
    let [x0, z0, x1, z1] = map.bounds.map(|v| v as f32);
    let floor = world.resource_mut::<Assets<Mesh>>().add(floor_mesh([x0, z0, x1, z1], FLOOR));
    let tiles = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color_texture: Some(kit.tiles.clone()),
        perceptual_roughness: 0.78,
        reflectance: 0.18,
        ..default()
    });
    world.spawn((MapPart, Name::new("Kitchen floor"), Mesh3d(floor.clone()), MeshMaterial3d(tiles), Transform::IDENTITY, crate::bake::Still));
    made.push(floor);
    things.push("Tiles");
    room(&mut Make { kit: kit.clone(), frame: Transform::IDENTITY, parts: &mut parts }, map);
    things.extend(["Walls", "Ceiling", "Window"]);
    let wx = window(map);
    for block in map.blocks.iter().filter(|b| cabinet(map, b)) {
        cabinets(&mut Make { kit: kit.clone(), frame: Transform::IDENTITY, parts: &mut parts }, map, block, wx);
        things.push("Cabinets");
    }
    for block in &map.blocks {
        let Some(side) = block.facing.filter(|_| KITS.contains(&block.style.as_str())) else {
            continue;
        };
        let (frame, w, d) = crate::level::facing(block.rect.map(|v| v as f32), side);
        let h = block.height as f32;
        let mut m = Make { kit: kit.clone(), frame, parts: &mut parts };
        things.push(match block.style.as_str() {
            "fridge" => {
                fridge(&mut m, w, d, h, map.blocks.iter().find_map(|b| door_side(block, b)));
                "Fridge"
            }
            "oven" => {
                oven(&mut m, w, d, h);
                "Oven"
            }
            _ => {
                pet_bowl(&mut m, w, d, h, block.rect[0] < -2.);
                "Pet bowl"
            }
        });
    }
    for deck in &map.decks {
        things.push(standing(&mut Make { kit: kit.clone(), frame: Transform::IDENTITY, parts: &mut parts }, deck));
    }
    made.extend(bake(world, parts));
    lights(world, map);
    let mut built = world.resource_mut::<Built>();
    (built.meshes, built.things) = (made, things);
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
        let mut e = world.spawn((MapPart, Name::new("Kitchen floor things"), Mesh3d(handle.clone()), MeshMaterial3d(material), Transform::IDENTITY, crate::bake::Still));
        if !shadow {
            e.insert(NotShadowCaster);
        }
        made.push(handle);
    }
    made
}

/// The walls round the bounds with the window in the back one, the skirting, the ceiling and its lamp, the sky outside.
fn room(m: &mut Make, map: &Map) {
    let k = m.kit.clone();
    let [x0, z0, x1, z1] = map.bounds.map(|v| v as f32);
    let (low, high) = (FLOOR, FLOOR + HEIGHT);
    let (wx, [ww, sill, top]) = (window(map), WINDOW);
    let (wa, wb) = (wx - ww / 2., wx + ww / 2.);
    // The slab under the tiles, and the ceiling.
    m.span(&k.plinth, Vec3::new(x0 - WALL, low - 0.3, z0 - WALL), Vec3::new(x1 + WALL, low - 0.001, z1 + WALL), true);
    m.span(&k.ceiling, Vec3::new(x0 - WALL, high, z0 - WALL), Vec3::new(x1 + WALL, high + 0.12, z1 + WALL), true);
    // The front and side walls whole; the back one round the window.
    m.span(&k.wall, Vec3::new(x0 - WALL, low, z0 - WALL), Vec3::new(x1 + WALL, high, z0), true);
    m.span(&k.wall, Vec3::new(x0 - WALL, low, z0), Vec3::new(x0, high, z1), true);
    m.span(&k.wall, Vec3::new(x1, low, z0), Vec3::new(x1 + WALL, high, z1), true);
    m.span(&k.wall, Vec3::new(x0 - WALL, low, z1), Vec3::new(wa, high, z1 + WALL), true);
    m.span(&k.wall, Vec3::new(wb, low, z1), Vec3::new(x1 + WALL, high, z1 + WALL), true);
    m.span(&k.wall, Vec3::new(wa, low, z1), Vec3::new(wb, low + sill, z1 + WALL), true);
    m.span(&k.wall, Vec3::new(wa, low + top, z1), Vec3::new(wb, high, z1 + WALL), true);
    // Skirting boards, 8 cm, along the walls (behind the cabinets they do not show).
    for (a, b) in [
        (Vec3::new(x0, low, z0), Vec3::new(x1, low + 0.08, z0 + 0.012)),
        (Vec3::new(x0, low, z0), Vec3::new(x0 + 0.012, low + 0.08, z1)),
        (Vec3::new(x1 - 0.012, low, z0), Vec3::new(x1, low + 0.08, z1)),
    ] {
        m.span(&k.skirting, a, b, false);
    }
    // The window: its frame round the opening, a bar across and one up the middle, the sill, the glass.
    let (f, y0, y1) = (0.05, low + sill, low + top);
    for (a, b) in [
        (Vec3::new(wa, y0, z1 - 0.02), Vec3::new(wb, y0 + f, z1 + 0.06)),
        (Vec3::new(wa, y1 - f, z1 - 0.02), Vec3::new(wb, y1, z1 + 0.06)),
        (Vec3::new(wa, y0, z1 - 0.02), Vec3::new(wa + f, y1, z1 + 0.06)),
        (Vec3::new(wb - f, y0, z1 - 0.02), Vec3::new(wb, y1, z1 + 0.06)),
        (Vec3::new(wx - 0.02, y0, z1 + 0.01), Vec3::new(wx + 0.02, y1, z1 + 0.05)),
        (Vec3::new(wa, (y0 + y1) / 2. + 0.12, z1 + 0.01), Vec3::new(wb, (y0 + y1) / 2. + 0.16, z1 + 0.05)),
    ] {
        m.span(&k.frame, a, b, true);
    }
    m.span(&k.frame, Vec3::new(wa - 0.05, y0 - 0.025, z1 - 0.07), Vec3::new(wb + 0.05, y0 + 0.004, z1 + 0.02), true);
    m.span(&k.glass, Vec3::new(wa, y0, z1 + 0.028), Vec3::new(wb, y1, z1 + 0.032), false);
    // Outside: the night sky on a far wall, the moon in it (seen through the window from the floor) and a few stars.
    let far = z1 + 3.4;
    m.span(&k.sky, Vec3::new(wx - 4., low - 0.5, far), Vec3::new(wx + 4., low + 6., far + 0.05), false);
    let through = Vec3::new(wx, low + 1.6, z1);
    let moon = through + MOON_WAY * ((far - z1) / MOON_WAY.z) - Vec3::Z * 0.06;
    let mesh = k.ball.clone();
    m.part(&mesh, &k.moon, Transform::from_translation(moon).with_scale(Vec3::new(0.34, 0.34, 0.02)), false);
    for i in 0..26u32 {
        let h = |a: u32| (a.wrapping_mul(2_654_435_761) >> 12) as f32 / 1_048_575.;
        let at = Vec3::new(wx - 3.5 + 7. * h(i * 3 + 1), low + 0.8 + 4.5 * h(i * 7 + 5), far - 0.03);
        if at.distance(moon) > 0.5 {
            m.part(&mesh, &k.star, Transform::from_translation(at).with_scale(Vec3::splat(0.012 + 0.012 * h(i * 11 + 2))), false);
        }
    }
    // A flush lamp in the middle of the ceiling, off.
    let centre = Vec3::new((x0 + x1) / 2., high, (z0 + z1) / 2.);
    m.post(&k.lampshade, centre - Vec3::Y * 0.07, 0.17, 0.07, false);
    m.post(&k.lampshade, centre - Vec3::Y * 0.012, 0.2, 0.012, false);
}

/// One run of base cabinets: the carcass block, the doors and drawers on the box standing proud of it over the toe-kick,
/// the plinth black and recessed, the stone worktop over all, the splashback, wall cupboards, and the sink under the window.
fn cabinets(m: &mut Make, map: &Map, block: &Block, wx: f32) {
    let k = m.kit.clone();
    let [bx0, bz0, bx1, bz1] = block.rect.map(|v| v as f32);
    let top = FLOOR + block.height as f32;
    let worktop = 0.0425;
    // Its front: the box standing against it, and the side it is on.
    let fronts: Vec<([f32; 4], f32)> = map
        .props
        .iter()
        .filter(|p| front(map, p))
        .filter_map(|p| match p {
            Prop::Box { rect, base, .. } => Some((rect.map(|v| v as f32), *base as f32)),
            _ => None,
        })
        .filter(|(r, _)| r[0] < bx1 && r[2] > bx0 && (r[3] - bz0).abs() < 1e-3)
        .collect();
    let kick = fronts.first().map_or(0.1, |(_, base)| *base);
    // The carcass to the worktop, and the plinth's black face under the doors.
    m.span(&k.carcass, Vec3::new(bx0, FLOOR, bz0), Vec3::new(bx1, top - worktop, bz1), true);
    m.span(&k.plinth, Vec3::new(bx0, FLOOR, bz0 - 0.004), Vec3::new(bx1, FLOOR + kick, bz0), false);
    // The worktop: stone over the carcass (its lip over the doors is the map's own box, drawn below with it).
    m.span(&k.stone, Vec3::new(bx0, top - worktop, bz0), Vec3::new(bx1, top, bz1), true);
    for prop in map.props.iter().filter(|p| lip(map, p)) {
        if let Prop::Box { rect, base, height, .. } = prop {
            let r = rect.map(|v| v as f32);
            if r[0] < bx1 && r[2] > bx0 && r[3] >= bz0 - 1e-3 && r[1] < bz0 {
                m.span(&k.stone, Vec3::new(r[0], FLOOR + *base as f32, r[1]), Vec3::new(r[2], FLOOR + (*base + *height) as f32, bz0), true);
            }
        }
    }
    let sink = wx > bx0 + 0.45 && wx < bx1 - 0.45;
    for (r, base) in fronts {
        let (fx0, fz0, fx1) = (r[0], r[1], r[2]);
        let (low, high) = (FLOOR + base, top - worktop);
        m.span(&k.carcass, Vec3::new(fx0, low, fz0 + 0.018), Vec3::new(fx1, high, bz0), true);
        // A unit about 0.6 m wide at a time: a drawer over a door (two doors under the sink).
        let units = ((fx1 - fx0) / 0.6).round().max(1.) as usize;
        let pitch = (fx1 - fx0) / units as f32;
        let (gap, drawer, face) = (0.004, 0.15, fz0 + 0.009);
        for i in 0..units {
            let (a, b) = (fx0 + pitch * i as f32, fx0 + pitch * (i + 1) as f32);
            let mid = (a + b) / 2.;
            let under_sink = sink && (a - 0.3..b + 0.3).contains(&wx);
            let split = high - drawer;
            m.cube(&k.door, Vec3::new(mid, (split + high) / 2., face), Vec3::new(pitch - gap, high - split - gap, 0.018));
            m.detail(&k.steel, Vec3::new(mid, (split + high) / 2., fz0 - 0.006), Vec3::new(0.16, 0.012, 0.012));
            let leaves: Vec<(f32, f32, f32)> = if under_sink { vec![(a, mid, 1.), (mid, b, -1.)] } else { vec![(a, b, if i % 2 == 0 { 1. } else { -1. })] };
            for (l, r, handle) in leaves {
                let middle = (l + r) / 2.;
                m.cube(&k.door, Vec3::new(middle, (low + split) / 2., face), Vec3::new(r - l - gap, split - low - gap, 0.018));
                m.detail(&k.panel, Vec3::new(middle, (low + split) / 2., face - 0.0095), Vec3::new(r - l - gap - 0.11, split - low - gap - 0.11, 0.002));
                let edge = if handle > 0. { r - 0.045 } else { l + 0.045 };
                m.detail(&k.steel, Vec3::new(edge, split - 0.12, fz0 - 0.006), Vec3::new(0.012, 0.13, 0.012));
            }
        }
    }
    // Against the back wall: a white tiled splashback over the worktop, and wall cupboards above it but over the window.
    let back = bz1;
    m.span(&k.splash, Vec3::new(bx0, top, back - 0.008), Vec3::new(bx1, FLOOR + 1.45, back), false);
    let [ww, _, _] = WINDOW;
    let cupboards: Vec<(f32, f32)> = if (bx0..bx1).contains(&wx) { vec![(bx0, wx - ww / 2. - 0.1), (wx + ww / 2. + 0.1, bx1)] } else { vec![(bx0, bx1)] };
    for (a, b) in cupboards.into_iter().filter(|(a, b)| b - a > 0.3) {
        let count = ((b - a) / 0.7).round().max(1.) as usize;
        let pitch = (b - a) / count as f32;
        m.span(&k.carcass, Vec3::new(a, FLOOR + 1.45, back - 0.32), Vec3::new(b, FLOOR + 2.15, back), true);
        // The strip of light under them, along their front edge.
        m.span(&k.strip, Vec3::new(a + 0.03, FLOOR + 1.443, back - 0.3), Vec3::new(b - 0.03, FLOOR + 1.449, back - 0.28), false);
        for i in 0..count {
            let mid = a + pitch * (i as f32 + 0.5);
            for s in [-1., 1.] {
                m.cube(&k.door, Vec3::new(mid + s * pitch / 4., FLOOR + 1.8, back - 0.329), Vec3::new(pitch / 2. - 0.006, 0.69, 0.018));
                m.detail(&k.panel, Vec3::new(mid + s * pitch / 4., FLOOR + 1.8, back - 0.339), Vec3::new(pitch / 2. - 0.1, 0.55, 0.002));
                m.detail(&k.steel, Vec3::new(mid + s * 0.035, FLOOR + 1.52, back - 0.345), Vec3::new(0.012, 0.09, 0.012));
            }
        }
    }
    // The sink under the window: a steel bowl let into the worktop and a tap behind it.
    if sink {
        let (sx, sz) = (wx, (bz0 + bz1) / 2.);
        m.span(&k.steel, Vec3::new(sx - 0.36, top - 0.001, sz - 0.21), Vec3::new(sx + 0.36, top + 0.002, sz + 0.21), false);
        m.span(&k.black, Vec3::new(sx - 0.32, top + 0.002, sz - 0.17), Vec3::new(sx + 0.32, top + 0.003, sz + 0.17), false);
        m.post(&k.chrome, Vec3::new(sx, top, back - 0.07), 0.022, 0.26, true);
        m.rod(&k.chrome, Vec3::new(sx, top + 0.26, back - 0.07), Vec3::new(sx, top + 0.26, back - 0.27), 0.012, true);
        m.rod(&k.chrome, Vec3::new(sx, top + 0.27, back - 0.27), Vec3::new(sx, top + 0.2, back - 0.27), 0.011, false);
    }
}

/// The fridge, built facing -z in its frame: w across, d deep, h tall. With its door a block of its own (`door_side`: the
/// side it is hinged on) it stands wide open: the lit inside, its shelves and what is on them, and the door out at a right
/// angle beside it, its bins full. Without, its body stands at the back of its box and the door, hinged on the left,
/// stands ajar in front, the light showing at the gap.
fn fridge(m: &mut Make, w: f32, d: f32, h: f32, door: Option<f32>) {
    let k = m.kit.clone();
    if let Some(s) = door {
        let (front, back) = (-d / 2., d / 2.);
        let wall = 0.04;
        // The shell round the inside: back, sides, top, and the base with its black grille.
        m.span(&k.shell, Vec3::new(-w / 2., 0., back - 0.06), Vec3::new(w / 2., h, back), true);
        for x in [-w / 2., w / 2. - wall] {
            m.span(&k.shell, Vec3::new(x, 0., front), Vec3::new(x + wall, h, back), true);
        }
        m.span(&k.shell, Vec3::new(-w / 2., h - 0.05, front), Vec3::new(w / 2., h, back), true);
        m.span(&k.shell, Vec3::new(-w / 2., 0., front + 0.02), Vec3::new(w / 2., 0.14, back), true);
        m.span(&k.black, Vec3::new(-w / 2. + wall, 0.01, front + 0.015), Vec3::new(w / 2. - wall, 0.1, front + 0.02), false);
        // The inside, lit: its back and sides, the lamp at the top, glass shelves, the crisper.
        m.span(&k.liner, Vec3::new(-w / 2. + wall, 0.14, back - 0.062), Vec3::new(w / 2. - wall, h - 0.05, back - 0.06), false);
        for x in [-w / 2. + wall, w / 2. - wall - 0.002] {
            m.span(&k.liner, Vec3::new(x, 0.14, front + 0.01), Vec3::new(x + 0.002, h - 0.05, back - 0.06), false);
        }
        m.span(&k.liner, Vec3::new(-0.08, h - 0.07, back - 0.25), Vec3::new(0.08, h - 0.05, back - 0.1), false);
        for y in [0.52, 0.9, 1.26] {
            m.span(&k.glass, Vec3::new(-w / 2. + wall, y, front + 0.03), Vec3::new(w / 2. - wall, y + 0.008, back - 0.06), false);
            m.span(&k.gasket, Vec3::new(-w / 2. + wall, y - 0.004, front + 0.03), Vec3::new(w / 2. - wall, y + 0.012, front + 0.045), false);
        }
        m.span(&k.plastic, Vec3::new(-w / 2. + wall + 0.01, 0.15, front + 0.04), Vec3::new(w / 2. - wall - 0.01, 0.42, back - 0.08), true);
        // What is in it: a carton of milk, bottles, a jar, a dish under foil.
        m.span(&k.plastic, Vec3::new(-0.3, 0.908, -0.05), Vec3::new(-0.2, 1.15, 0.05), true);
        m.span(&k.style("red"), Vec3::new(-0.3, 1.1, -0.051), Vec3::new(-0.2, 1.13, -0.05), false);
        for (x, z, r, tall, look) in [(0.12, 0.0, 0.035, 0.26, "green"), (0.22, 0.05, 0.03, 0.22, "dark"), (-0.05, 0.1, 0.05, 0.12, "yellow")] {
            m.post(&k.style(look), Vec3::new(x, 0.528, z), r, tall, true);
        }
        m.span(&k.chrome, Vec3::new(-0.15, 1.268, -0.12), Vec3::new(0.15, 1.31, 0.12), true);
        // The door, out at a right angle beside the side it hangs on: its shell outside, white inside with three bins.
        let (out, inn) = (s * (w / 2. + 0.06), s * w / 2.);
        let (x0, x1) = (out.min(inn), out.max(inn));
        m.span(&k.shell, Vec3::new(x0, 0.02, front - w), Vec3::new(x1, h, front), true);
        let face = inn - s * 0.001;
        m.span(&k.plastic, Vec3::new(face.min(inn), 0.06, front - w + 0.03), Vec3::new(face.max(inn), h - 0.04, front - 0.03), false);
        for y in [0.4, 0.85, 1.3] {
            let (a, b) = (inn, inn - s * 0.09);
            m.span(&k.plastic, Vec3::new(a.min(b), y, front - w + 0.06), Vec3::new(a.max(b), y + 0.1, front - 0.06), true);
            for i in 0..4 {
                let z = front - w + 0.14 + 0.18 * i as f32;
                // (The shelves' colours, so that they bake into the same few meshes.)
                let look = ["green", "yellow", "dark", "yellow"][i];
                m.post(&k.style(look), Vec3::new(inn - s * 0.045, y + 0.02, z), 0.028, 0.16 + 0.02 * (i % 2) as f32, true);
            }
        }
        m.cube(&k.steel, Vec3::new(out + s * 0.03, h * 0.6, front - w + 0.06), Vec3::new(0.024, h * 0.42, 0.024));
        return;
    }
    let face = d / 2. - FRIDGE_BODY.min(d - 0.05);
    m.span(&k.shell, Vec3::new(-w / 2., 0.1, face), Vec3::new(w / 2., h, d / 2.), true);
    m.span(&k.black, Vec3::new(-w / 2. + 0.02, 0.01, face + 0.02), Vec3::new(w / 2. - 0.02, 0.1, d / 2.), true);
    m.span(&k.gasket, Vec3::new(-w / 2. + 0.04, 0.02, face - 0.003), Vec3::new(w / 2. - 0.04, 0.08, face), false);
    // The lit inside, behind the door: the liner's face and the edges of the shelves.
    m.span(&k.liner, Vec3::new(-w / 2. + 0.04, 0.14, face - 0.004), Vec3::new(w / 2. - 0.04, h - 0.04, face - 0.001), false);
    for y in [0.56, 0.94, 1.28, 1.6] {
        m.span(&k.gasket, Vec3::new(-w / 2. + 0.05, y, face - 0.006), Vec3::new(w / 2. - 0.05, y + 0.012, face - 0.004), false);
    }
    // The door, on its hinge at the front left, turned out by AJAR.
    let hinge = Transform::from_xyz(-w / 2., 0., face).with_rotation(Quat::from_rotation_y(AJAR));
    let frame = m.frame;
    m.frame = frame.mul_transform(hinge);
    m.span(&k.shell, Vec3::new(0., 0.1, -0.065), Vec3::new(w, h, -0.006), true);
    m.span(&k.gasket, Vec3::new(0.01, 0.11, -0.006), Vec3::new(w - 0.01, h - 0.01, 0.), false);
    m.cube(&k.steel, Vec3::new(w - 0.06, h * 0.6, -0.09), Vec3::new(0.024, h * 0.42, 0.024));
    for y in [h * 0.39, h * 0.81] {
        m.detail(&k.steel, Vec3::new(w - 0.06, y, -0.075), Vec3::new(0.02, 0.02, 0.03));
    }
    m.frame = frame;
}

/// A freestanding range, built facing -z in its frame: the body, the oven door with its dark window and bar handle, the
/// drawer at its foot, the knobs and the clock along the front, the hob's four rings on top, a hood high over it.
fn oven(m: &mut Make, w: f32, d: f32, h: f32) {
    let k = m.kit.clone();
    let (hob, front) = (h - 0.03, -d / 2.);
    m.span(&k.enamel, Vec3::new(-w / 2., 0., front + 0.01), Vec3::new(w / 2., hob, d / 2.), true);
    m.span(&k.black, Vec3::new(-w / 2., hob, front + 0.01), Vec3::new(w / 2., hob + 0.008, d / 2.), true);
    m.span(&k.steel, Vec3::new(-w / 2., hob, d / 2. - 0.05), Vec3::new(w / 2., h, d / 2.), true);
    // The oven door, its window and the handle over it; the drawer under it.
    m.span(&k.enamel, Vec3::new(-w / 2. + 0.01, 0.13, front), Vec3::new(w / 2. - 0.01, hob - 0.17, front + 0.012), true);
    m.span(&k.oven_glass, Vec3::new(-w / 2. + 0.09, 0.25, front - 0.001), Vec3::new(w / 2. - 0.09, hob - 0.3, front + 0.002), false);
    m.rod(&k.steel, Vec3::new(-w / 2. + 0.07, hob - 0.21, front - 0.04), Vec3::new(w / 2. - 0.07, hob - 0.21, front - 0.04), 0.011, true);
    for x in [-w / 2. + 0.09, w / 2. - 0.09] {
        m.detail(&k.steel, Vec3::new(x, hob - 0.21, front - 0.02), Vec3::new(0.016, 0.016, 0.04));
    }
    m.span(&k.enamel, Vec3::new(-w / 2. + 0.01, 0.015, front), Vec3::new(w / 2. - 0.01, 0.115, front + 0.012), true);
    m.detail(&k.steel, Vec3::new(0., 0.09, front - 0.006), Vec3::new(0.2, 0.012, 0.012));
    // The control strip: five knobs, the clock glowing in the middle.
    m.span(&k.black, Vec3::new(-w / 2., hob - 0.15, front - 0.004), Vec3::new(w / 2., hob - 0.02, front + 0.01), false);
    for x in [-0.36, -0.22, 0.22, 0.36] {
        m.rod(&k.steel, Vec3::new(x * w, hob - 0.085, front - 0.004), Vec3::new(x * w, hob - 0.085, front - 0.03), 0.019, false);
    }
    m.span(&k.clock, Vec3::new(-0.045, hob - 0.1, front - 0.006), Vec3::new(0.045, hob - 0.07, front - 0.004), false);
    // The hob's rings.
    for (x, z, r) in [(-0.24, -0.17, 0.09), (0.24, -0.17, 0.07), (-0.24, 0.13, 0.07), (0.24, 0.13, 0.09)] {
        m.post(&k.steel, Vec3::new(x * w, hob + 0.008, z * d), r, 0.004, false);
        m.post(&k.black, Vec3::new(x * w, hob + 0.009, z * d), r * 0.8, 0.004, false);
    }
    // The hood high over it, against the wall.
    m.span(&k.steel, Vec3::new(-w / 2. - 0.02, 0.62 + 0.9, d / 2. - 0.5), Vec3::new(w / 2. + 0.02, 0.62 + 0.9 + 0.14, d / 2.), true);
    m.span(&k.steel, Vec3::new(-0.15, 0.62 + 1.04, d / 2. - 0.3), Vec3::new(0.15, 1.8, d / 2.), true);
}

/// A pet's steel bowl on the floor, kibble in one and water in the other.
fn pet_bowl(m: &mut Make, w: f32, d: f32, h: f32, food: bool) {
    let k = m.kit.clone();
    let r = w.min(d);
    let mesh = k.bowl.clone();
    m.part(&mesh, &k.steel, Transform::from_scale(Vec3::new(r, h, r)), true);
    if food {
        for i in 0..14 {
            let t = i as f32 * 2.4;
            let at = Vec3::new(t.cos() * r * 0.04 * (i % 5) as f32, h * 0.45 + 0.004 * (i % 3) as f32, t.sin() * r * 0.04 * (i % 5) as f32);
            let ball = k.ball.clone();
            m.part(&ball, &k.kibble, Transform::from_translation(at).with_scale(Vec3::new(0.016, 0.011, 0.016)), false);
        }
        m.post(&k.kibble, Vec3::Y * h * 0.3, r * 0.3, h * 0.15, false);
    } else {
        m.post(&k.water, Vec3::Y * h * 0.3, r * 0.37, h * 0.5, false);
    }
}

/// A deck standing on the floor, solid to it: the cardboard box, the step stool, or a block of its style. Returns its name.
fn standing(m: &mut Make, deck: &Deck) -> &'static str {
    let k = m.kit.clone();
    let [x0, z0, x1, z1] = deck.rect.map(|v| v as f32);
    let h = deck.height as f32;
    let (low, high) = (Vec3::new(x0, FLOOR, z0), Vec3::new(x1, FLOOR + h, z1));
    match deck.style.as_str() {
        "card" => {
            m.span(&k.card, low, high, true);
            // Its top flaps folded down over the sides, a strip of tape along the seam, a printed arrow.
            let fold = 0.12_f32.min(h * 0.4);
            for (a, b) in [
                (Vec3::new(x0 - 0.004, FLOOR + h - fold, z0 - 0.004), Vec3::new(x1 + 0.004, FLOOR + h, z0)),
                (Vec3::new(x0 - 0.004, FLOOR + h - fold, z1), Vec3::new(x1 + 0.004, FLOOR + h, z1 + 0.004)),
                (Vec3::new(x0 - 0.004, FLOOR + h - fold, z0), Vec3::new(x0, FLOOR + h, z1)),
                (Vec3::new(x1, FLOOR + h - fold, z0), Vec3::new(x1 + 0.004, FLOOR + h, z1)),
            ] {
                m.span(&k.card, a, b, false);
            }
            let (mx, mz) = ((x0 + x1) / 2., (z0 + z1) / 2.);
            m.span(&k.tape, Vec3::new(mx - 0.025, FLOOR + h, z0 - 0.005), Vec3::new(mx + 0.025, FLOOR + h + 0.001, z1 + 0.005), false);
            m.span(&k.tape, Vec3::new(mx - 0.025, FLOOR + h - 0.08, z0 - 0.006), Vec3::new(mx + 0.025, FLOOR + h, z0 - 0.004), false);
            m.span(&k.black, Vec3::new(x1 + 0.0045, FLOOR + h * 0.35, mz - 0.05), Vec3::new(x1 + 0.0055, FLOOR + h * 0.55, mz + 0.05), false);
            "Cardboard box"
        }
        "white" => {
            m.span(&k.plastic, low, high, true);
            m.span(&k.tread, Vec3::new(x0 + 0.03, FLOOR + h, z0 + 0.025), Vec3::new(x1 - 0.03, FLOOR + h + 0.002, z1 - 0.025), false);
            m.span(&k.tread, Vec3::new(x0 + 0.05, FLOOR, z0 - 0.002), Vec3::new(x1 - 0.05, FLOOR + 0.02, z1 + 0.002), false);
            "Step stool"
        }
        style => {
            m.span(&k.style(style), low, high, true);
            "Block"
        }
    }
}

/// The moon through the window and the fridge's glow.
fn lights(world: &mut World, map: &Map) {
    let [_, _, _, z1] = map.bounds.map(|v| v as f32);
    let through = Vec3::new(window(map), FLOOR + 1.6, z1);
    world.spawn((
        MapPart,
        Lamp,
        Name::new("Moonlight through the window"),
        SpotLight {
            intensity: MOON,
            range: 14.,
            radius: 0.2,
            color: MOON_COLOR,
            inner_angle: 0.08,
            outer_angle: 0.3,
            shadow_maps_enabled: true,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 0.4,
            ..default()
        },
        Transform::from_translation(through + MOON_WAY * 3.).looking_at(through - MOON_WAY * 3., Vec3::Y),
        // (A shaft of it in the air, where the renderer marches through fog: `visuals`, at the full feature set.)
        bevy::light::VolumetricLight,
    ));
    for block in map.blocks.iter().filter(|b| b.style == "fridge") {
        let Some(side) = block.facing else { continue };
        let (frame, w, d) = crate::level::facing(block.rect.map(|v| v as f32), side);
        let h = block.height as f32;
        let open = map.blocks.iter().any(|b| door_side(block, b).is_some());
        // Wide open: a spot from inside out over the floor in front, and a glow at the opening. Ajar: behind the door, at its
        // open edge, a spot pointing out of the gap (the door's shadow shapes its wedge), and a glow.
        let face = if open { -d / 2. + 0.2 } else { d / 2. - FRIDGE_BODY.min(d - 0.05) };
        let at = frame.transform_point(if open { Vec3::new(0., h * 0.62, face) } else { Vec3::new(w * 0.3, h * 0.55, face - 0.015) });
        let out = frame.transform_point(if open { Vec3::new(0., 0., -d / 2. - 2.) } else { Vec3::new(w * 1.1, 0., -d / 2. - 1.8) });
        world.spawn((
            MapPart,
            Lamp,
            Name::new("The fridge's light"),
            SpotLight {
                intensity: FRIDGE,
                range: 7.,
                radius: 0.04,
                color: FRIDGE_COLOR,
                inner_angle: 0.3,
                outer_angle: 1.1,
                shadow_maps_enabled: true,
                shadow_depth_bias: 0.01,
                shadow_normal_bias: 0.2,
                ..default()
            },
            Transform::from_translation(at).looking_at(out, Vec3::Y),
        ));
        let gap = frame.transform_point(if open { Vec3::new(0., h * 0.5, -d / 2. - 0.15) } else { Vec3::new(w * 0.56, h * 0.45, face - 0.12) });
        world.spawn((
            MapPart,
            Lamp,
            Name::new("The fridge's glow"),
            PointLight { intensity: FRIDGE_GLOW, range: 2.5, radius: 0.05, color: FRIDGE_COLOR, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(gap),
        ));
    }
    // Under the wall cupboards of each run of cabinets, a warm strip left on: it lights the worktop and the splashback.
    for block in map.blocks.iter().filter(|b| cabinet(map, b)) {
        let [bx0, _, bx1, bz1] = block.rect.map(|v| v as f32);
        let wx = window(map);
        let x = if (bx0..bx1).contains(&wx) { (bx0 + wx) / 2. } else { (bx0 + bx1) / 2. };
        world.spawn((
            MapPart,
            Lamp,
            Name::new("Under-cupboard light"),
            PointLight { intensity: UNDER, range: 2.4, radius: 0.3, color: UNDER_COLOR, shadow_maps_enabled: false, ..default() },
            Transform::from_xyz(x, FLOOR + 1.38, bz1 - 0.25),
        ));
    }
    // The fill, high over each quarter of the room.
    let [x0, z0, x1, z1] = map.bounds.map(|v| v as f32);
    for (u, v) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
        let at = Vec3::new(x0 + (x1 - x0) * u, FLOOR + HEIGHT - 0.05, z0 + (z1 - z0) * v);
        world.spawn((
            MapPart,
            Lamp,
            Name::new("Night fill"),
            SpotLight { intensity: FILL, range: 9., radius: 0.5, color: FILL_COLOR, inner_angle: 0.6, outer_angle: 1.25, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(at).looking_at(at - Vec3::Y, Vec3::Z),
        ));
    }
    let _ = (FRAC_PI_2, PI, Side::PlusZ);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titans() -> Map {
        Map::parse(include_str!("../assets/maps/titans-kitchen.json")).unwrap()
    }

    #[test]
    fn titans_kitchen_is_a_floor_and_leaves_its_cabinets_and_kits_to_this_module() {
        let map = titans();
        assert!(map.floor);
        let (cabinets, kits) = (map.blocks.iter().filter(|b| cabinet(&map, b)).count(), map.blocks.iter().filter(|b| KITS.contains(&b.style.as_str())).count());
        assert_eq!((cabinets, kits), (2, 4), "two runs of cabinets; the fridge, the oven and two bowls");
        // The fridge stands wide open: its door is its own block, out at a right angle from its hinge, drawn here.
        let fridge = map.blocks.iter().find(|b| b.style == "fridge").unwrap();
        let doors: Vec<&Block> = map.blocks.iter().filter(|b| fridge_door(&map, b)).collect();
        assert_eq!(doors.len(), 1);
        assert!(door_side(fridge, doors[0]).is_some() && drawn_block(&map, doors[0]));
        assert!((doors[0].rect[3] - doors[0].rect[1] - (fridge.rect[2] - fridge.rect[0])).abs() < 1e-9 && doors[0].rect[1] == fridge.rect[3], "as long as the fridge is wide, from its front");
        // Nothing else is a door: not the stool's legs, not a leg of the table.
        assert!(map.blocks.iter().filter(|b| b.style != "steel").all(|b| !fridge_door(&map, b)));
        assert_eq!(map.props.iter().filter(|p| front(&map, p)).count(), 2);
        assert_eq!(map.props.iter().filter(|p| lip(&map, p)).count(), 2);
        // The table's and the chairs' legs and tops are not: they are drawn as the map's blocks and boxes.
        assert!(map.blocks.iter().filter(|b| b.style == "wood" && b.height < 0.85).all(|b| !drawn_block(&map, b)));
        // On a counter map nothing is.
        let counter = Map::bundled();
        assert!(!counter.blocks.iter().any(|b| drawn_block(&counter, b)) && !counter.props.iter().any(|p| drawn_prop(&counter, p)));
        // The window is over the sink, in the run of cabinets left of the oven.
        let x = window(&map);
        let run = map.blocks.iter().find(|b| cabinet(&map, b) && (b.rect[0] as f32) < x && (b.rect[2] as f32) > x).unwrap();
        assert!(x - run.rect[0] as f32 > WINDOW[0] / 2. && run.rect[2] as f32 - x > WINDOW[0] / 2.);
        // The ceiling is out of the giant's reach: its height and a hop, with room to spare.
        assert!(HEIGHT > kitchen_sim::titan::TALL + 0.5);
    }

    #[test]
    fn the_tiles_have_every_mip_level_and_grout_darker_than_the_tiles() {
        let image = tiles(64);
        assert_eq!(image.texture_descriptor.mip_level_count, 7);
        let data = image.data.as_ref().unwrap();
        assert_eq!(data.len(), (64 * 64 + 32 * 32 + 16 * 16 + 8 * 8 + 4 * 4 + 2 * 2 + 1) * 4);
        let at = |x: usize, y: usize| data[(y * 64 + x) * 4] as u32;
        assert!(at(0, 0) < at(16, 16) && at(32, 10) < at(48, 10), "grout at the tiles' edges");
    }

    #[test]
    fn the_room_is_built_round_the_floor_and_lit_by_the_moon_and_the_fridge() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<Image>>();
        let map = titans();
        build(&mut world, &map);
        let things = world.resource::<Built>().things.clone();
        for thing in ["Tiles", "Walls", "Window", "Cabinets", "Fridge", "Oven", "Pet bowl", "Cardboard box", "Step stool"] {
            assert!(things.contains(&thing), "{thing} in {things:?}");
        }
        let spots: Vec<(String, f32)> = world.query::<(&Name, &SpotLight)>().iter(&world).map(|(n, s)| (n.to_string(), s.intensity)).collect();
        assert_eq!(spots.len(), 6, "the moon, the fridge's, the fill's four: {spots:?}");
        assert_eq!(world.query_filtered::<(), (With<Lamp>, With<MapPart>)>().iter(&world).count(), 9, "the moon, the fridge's two, the two under the cupboards, the fill's four");
        // A counter map draws nothing here, and takes away what a floor map left.
        build(&mut world, &Map::bundled());
        assert!(world.resource::<Built>().things.is_empty());
    }
}

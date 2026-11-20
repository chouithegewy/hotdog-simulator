// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The dojo's things, drawn from a map (`level::build` calls `build`): what the `bamboo` style and the `gong` kit look like.
//! Kung Fu is fought on `assets/maps/kung-fu-dojo.json`, but any map may have them.
//!
//! * **A round deck** (bamboo, `kitchen_sim::map::ROUND`) is a bamboo steamer: its side wrapped in bamboo strips with darker
//!   bands, its top woven (a basket weave drawn here, [`weave`], repeating every [`WEAVE`] metres so the strips are the same
//!   size on every steamer), filling the ellipse inside its rect, which is all of it a hotdog can stand on (`Level::height`).
//! * **The ring**: a round deck with three or more chopstick posts round its rim ([`CORNER`]). Its top band and a stripe
//!   just inside its edge are red lacquer, so its edge reads from anywhere on it; a string runs round the posts' tops, hung
//!   with little paper pennants, and a paper lantern hangs in the middle of each run, lit warm.
//! * **A slim bamboo block** ([`slim`]): a chopstick standing in a white holder with a blue band.
//! * **The gong** (`gong` kit, its face on the block's `facing` side): a wok hung on two strings from a bar between two long
//!   cooking chopsticks, its wooden handle out to the side, and a wooden spoon to strike it leaning on a post. It swings when
//!   it is struck ([`strike`]: Kung Fu's knockouts, fu.rs).
//!
//! Everything is a `MapPart` (a new map replaces it). The meshes and materials are made once ([`Kit`]); the woven tops,
//! one per steamer for their size, are kept and freed when the map is rebuilt ([`Built`]).
use crate::level::MapPart;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use kitchen_sim::map::{Block, Deck, Map, ROUND};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const FLOOR: f32 = 0.9025;
/// A bamboo block no wider than this is a chopstick in its holder (the editor draws it so too, editor/view.js).
pub const SLIM: f64 = 0.08;
/// The kit that is a gong.
pub const GONG: &str = "gong";
/// A slim bamboo block whose middle is no further than this beyond a round deck's rim is one of its corner posts.
pub const CORNER: f32 = 0.2;
/// The woven top repeats every this many metres (eight squares of three strips: strips of a centimetre).
pub const WEAVE: f32 = 0.24;
/// How far a ring's string sags in the middle of each run, and how far below it a lantern hangs.
const SAG: f32 = 0.022;
const DROP: f32 = 0.028;
/// A lantern's light: lumens and reach (warm, no shadows: four of them light the ring).
const LANTERN_LUMENS: f32 = 30.;
const LANTERN_RANGE: f32 = 1.1;

/// Whether a block is drawn as a chopstick standing in its holder.
pub fn slim(block: &Block) -> bool {
    block.style == ROUND && (block.rect[2] - block.rect[0]).max(block.rect[3] - block.rect[1]) <= SLIM
}
/// Whether a deck is drawn here, as a steamer.
pub fn round(deck: &Deck) -> bool {
    deck.style == ROUND
}

/// A ring: a round deck and its corner posts' tops, in order round it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    pub deck: usize,
    pub posts: Vec<Vec3>,
}
/// The rings of a map: every round deck with three or more slim bamboo posts within CORNER of its rim (inside it or out),
/// its posts' tops sorted by their angle round its middle.
pub fn rings(map: &Map) -> Vec<Ring> {
    let mut out = vec![];
    for (index, deck) in map.decks.iter().enumerate().filter(|(_, d)| round(d)) {
        let [x0, z0, x1, z1] = deck.rect.map(|v| v as f32);
        let (c, r) = (Vec2::new((x0 + x1) / 2., (z0 + z1) / 2.), Vec2::new((x1 - x0) / 2., (z1 - z0) / 2.));
        let mut posts: Vec<(f32, Vec3)> = map
            .blocks
            .iter()
            .filter(|b| slim(b))
            .filter_map(|b| {
                let p = Vec2::new((b.rect[0] + b.rect[2]) as f32 / 2., (b.rect[1] + b.rect[3]) as f32 / 2.);
                // How far beyond the rim, measured along the way out (the ellipse squeezed to its circle and back).
                let off = p - c;
                let unit = (off / r).length();
                let beyond = (unit - 1.) * off.length() / unit.max(1e-6);
                (beyond.abs() <= CORNER).then_some((off.y.atan2(off.x), Vec3::new(p.x, FLOOR + b.height as f32, p.y)))
            })
            .collect();
        if posts.len() < 3 {
            continue;
        }
        posts.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.push(Ring { deck: index, posts: posts.into_iter().map(|(_, p)| p).collect() });
    }
    out
}

/// A swinging gong: its wok's pivot turns about its frame's x by `angle`.
#[derive(Component, Default)]
pub struct Gong {
    pub angle: f32,
    pub speed: f32,
}
/// Strikes every gong (a knockout): it swings out and back and settles.
pub fn strike(world: &mut World, how: f32) {
    let mut gongs = world.query::<&mut Gong>();
    for mut gong in gongs.iter_mut(world) {
        gong.speed += 3.2 * how;
    }
}
/// Every frame: the gongs swing, damped, back to rest.
pub fn swing(time: Res<Time>, mut gongs: Query<(&mut Gong, &mut Transform)>) {
    let dt = time.delta_secs().min(0.05);
    for (mut gong, mut at) in &mut gongs {
        if gong.angle == 0. && gong.speed == 0. {
            continue;
        }
        // A pendulum about 9 cm long (2 Hz), losing most of its swing in a couple of seconds.
        let accel = -(TAU * 1.7).powi(2) * gong.angle - 2.2 * gong.speed;
        gong.speed += accel * dt;
        gong.angle = (gong.angle + gong.speed * dt).clamp(-0.6, 0.6);
        if gong.angle.abs() < 1e-4 && gong.speed.abs() < 1e-3 {
            (gong.angle, gong.speed) = (0., 0.);
        }
        at.rotation = Quat::from_rotation_x(gong.angle);
    }
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, swing);
    }
}

/// Shared meshes and materials, made once.
#[derive(Resource, Clone)]
struct Kit {
    /// Unit cylinders (radius 0.5, height 1, centred), smooth and coarse; a unit sphere and cube.
    round: Handle<Mesh>,
    coarse: Handle<Mesh>,
    ball: Handle<Mesh>,
    cube: Handle<Mesh>,
    /// A chopstick, 1 long, standing on its tip: 0.6 across there, 1 at its top (scaled by its radius).
    stick: Handle<Mesh>,
    /// A wok on its back, 1 across the rim and its bowl 0.4 deep (scaled by its width).
    wok: Handle<Mesh>,
    /// A flat ring of the unit circle, from 0.94 to 0.985 of its radius (the ring's red stripe), facing up.
    stripe: Handle<Mesh>,
    side: Handle<StandardMaterial>,
    band: Handle<StandardMaterial>,
    top: Handle<StandardMaterial>,
    lacquer: Handle<StandardMaterial>,
    ceramic: Handle<StandardMaterial>,
    glaze: Handle<StandardMaterial>,
    stick_wood: Handle<StandardMaterial>,
    tie: Handle<StandardMaterial>,
    string: Handle<StandardMaterial>,
    paper: Handle<StandardMaterial>,
    cap: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
    pennants: [Handle<StandardMaterial>; 3],
    steel: Handle<StandardMaterial>,
    boss: Handle<StandardMaterial>,
    walnut: Handle<StandardMaterial>,
    beech: Handle<StandardMaterial>,
}
impl FromWorld for Kit {
    fn from_world(world: &mut World) -> Self {
        let weave = world.get_resource_mut::<Assets<Image>>().map(|mut images| images.add(weave(256)));
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let round = meshes.add(Cylinder::new(0.5, 1.).mesh().resolution(48));
        let coarse = meshes.add(Cylinder::new(0.5, 1.).mesh().resolution(10));
        let ball = meshes.add(Sphere::new(0.5).mesh().uv(16, 10));
        let cube = meshes.add(Cuboid::default());
        let stick = meshes.add(ConicalFrustum { radius_top: 1., radius_bottom: 0.6, height: 1. }.mesh().resolution(8));
        let wok = meshes.add(wok());
        let stripe = meshes.add(annulus(0.94, 0.985, 64));
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        let mut make = |r: f32, g: f32, b: f32, roughness: f32, metallic: f32| {
            mats.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: roughness, metallic, ..default() })
        };
        let side = make(0.80, 0.64, 0.38, 0.62, 0.);
        let band = make(0.52, 0.37, 0.18, 0.55, 0.);
        let lacquer = make(0.72, 0.09, 0.05, 0.32, 0.);
        let ceramic = make(0.94, 0.93, 0.89, 0.2, 0.);
        let glaze = make(0.12, 0.27, 0.62, 0.18, 0.);
        let stick_wood = make(0.86, 0.73, 0.50, 0.5, 0.);
        let tie = make(0.18, 0.11, 0.06, 0.7, 0.);
        let string = make(0.86, 0.80, 0.66, 0.8, 0.);
        let cap = make(0.06, 0.05, 0.05, 0.5, 0.);
        // (A silk tassel, not metal: a metallic one streaks in the native renderer.)
        let gold = make(0.95, 0.70, 0.22, 0.6, 0.);
        let pennants = [make(0.80, 0.12, 0.08, 0.6, 0.), make(0.97, 0.75, 0.20, 0.6, 0.), make(0.95, 0.92, 0.84, 0.7, 0.)];
        // (Seasoned carbon steel: dark, dull, not a mirror; a mirror-like metal streaks in the native renderer.)
        let steel = make(0.09, 0.085, 0.08, 0.55, 0.35);
        let boss = make(0.42, 0.33, 0.2, 0.45, 0.4);
        let walnut = make(0.33, 0.2, 0.11, 0.45, 0.);
        let beech = make(0.74, 0.54, 0.33, 0.55, 0.);
        let top = mats.add(StandardMaterial {
            base_color: if weave.is_some() { Color::WHITE } else { Color::srgb(0.86, 0.72, 0.46) },
            base_color_texture: weave,
            perceptual_roughness: 0.66,
            ..default()
        });
        // Red paper with a light inside: it glows of itself.
        let paper = mats.add(StandardMaterial {
            base_color: Color::srgb(0.92, 0.22, 0.10),
            emissive: LinearRgba::rgb(2.6, 0.55, 0.12),
            perceptual_roughness: 0.8,
            ..default()
        });
        Kit {
            round,
            coarse,
            ball,
            cube,
            stick,
            wok,
            stripe,
            side,
            band,
            top,
            lacquer,
            ceramic,
            glaze,
            stick_wood,
            tie,
            string,
            paper,
            cap,
            gold,
            pennants,
            steel,
            boss,
            walnut,
            beech,
        }
    }
}

/// The steamers' woven tops made for the last map (`build` frees them before it makes the next map's).
#[derive(Resource, Default)]
pub struct Built {
    meshes: Vec<Handle<Mesh>>,
    /// What was drawn, for the tests: "Steamer", "Ring", "Post", "Lantern", "Gong".
    pub things: Vec<&'static str>,
}

/// A basket weave of bamboo strips, `size` texels square: eight squares a side, each three strips running across or
/// down in turn, a dark seam between strips, each strip a little rounder in the middle and a shade different.
pub fn weave(size: u32) -> Image {
    let n = size as usize;
    let mut data = Vec::with_capacity(n * n * 4);
    let cells = 8.;
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((x as f32 + 0.5) / n as f32 * cells, (y as f32 + 0.5) / n as f32 * cells);
            let (i, j) = (u.floor() as i32, v.floor() as i32);
            let across = (i + j) % 2 == 0;
            // Where in the strip (0 to 3 strips a square) and which strip.
            let along = if across { v.fract() * 3. } else { u.fract() * 3. };
            let strip = (if across { j * 3 } else { i * 3 }) + along.floor() as i32;
            let t = along.fract();
            let seam = (1. - (t.min(1. - t) / 0.12).min(1.)).powi(2);
            let round = 1. - (t - 0.5).powi(2) * 1.3;
            // A shade for each strip (and the squares' crossings), as split bamboo varies.
            let hash = ((strip * 7919 + if across { i } else { j } * 104_729) as u32).wrapping_mul(2_654_435_761) >> 24;
            let shade = 0.9 + (hash as f32 / 255.) * 0.16;
            let base = [0.88, 0.74, 0.48].map(|c| c * shade * round);
            let c = base.map(|c| c * (1. - 0.55 * seam));
            data.extend(c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8));
            data.push(255);
        }
    }
    let mut image = Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// An ellipse's top, `rx` by `rz`, facing up, its texture coordinates in WEAVEs (so the weave keeps its size).
fn lid(rx: f32, rz: f32, sides: usize) -> Mesh {
    let mut positions = vec![[0., 0., 0.]];
    let mut uvs = vec![[0., 0.]];
    for i in 0..=sides {
        let t = i as f32 / sides as f32 * TAU;
        let (x, z) = (t.cos() * rx, t.sin() * rz);
        positions.push([x, 0., z]);
        uvs.push([x / WEAVE, z / WEAVE]);
    }
    let normals = vec![[0., 1., 0.]; positions.len()];
    let mut indices = vec![];
    for i in 1..=sides as u32 {
        indices.extend([0, i + 1, i]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}
/// A flat ring of the unit circle from `inner` to `outer`, facing up.
fn annulus(inner: f32, outer: f32, sides: usize) -> Mesh {
    let (mut positions, mut uvs, mut indices) = (vec![], vec![], vec![]);
    for i in 0..=sides {
        let t = i as f32 / sides as f32 * TAU;
        for r in [inner, outer] {
            positions.push([t.cos() * r, 0., t.sin() * r]);
            uvs.push([i as f32 / sides as f32, r]);
        }
    }
    for i in 0..sides as u32 {
        let (a, b) = (2 * i, 2 * i + 2);
        indices.extend([a, b, a + 1, a + 1, b, b + 1]);
    }
    let normals = vec![[0., 1., 0.]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}
/// A wok on its back, 1 across: a shallow bowl 0.4 deep with a little lip, its steel 0.012 thick (kitchen.rs `lathe`).
fn wok() -> Mesh {
    let steps = 10;
    let mut profile: Vec<[f32; 2]> = (0..=steps).map(|k| k as f32 / steps as f32).map(|t| [0.5 * t, 0.4 * t * t]).collect();
    profile.push([0.515, 0.41]);
    profile.extend((0..=steps).rev().map(|k| k as f32 / steps as f32).map(|t| [(0.5 * t - 0.012).max(0.), 0.4 * t * t + 0.012]));
    crate::kitchen::lathe(&profile, 32)
}

/// One part, under `parent`: still (`bake`), but for the gong's swinging wok (`gong`).
fn part(world: &mut World, parent: Entity, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, transform: Transform, shadow: bool) -> Entity {
    let id = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), transform, crate::bake::Still)).id();
    if !shadow {
        world.entity_mut(id).insert(NotShadowCaster);
    }
    world.entity_mut(parent).add_child(id);
    id
}
/// A rod from `a` to `b` (a coarse cylinder) under `parent`, in its frame.
fn rod(world: &mut World, parent: Entity, kit: &Kit, material: &Handle<StandardMaterial>, a: Vec3, b: Vec3, radius: f32, shadow: bool) -> Entity {
    let (way, length) = ((b - a).normalize_or(Vec3::Y), a.distance(b));
    let at = Transform::from_translation((a + b) / 2.).with_rotation(Quat::from_rotation_arc(Vec3::Y, way)).with_scale(Vec3::new(2. * radius, length, 2. * radius));
    part(world, parent, &kit.coarse, material, at, shadow)
}
fn root(world: &mut World, name: &'static str, transform: Transform) -> Entity {
    world.spawn((MapPart, Name::new(name), transform, Visibility::default())).id()
}

/// Draws a map's steamers, rings, chopstick posts and gongs (see the module's documentation).
pub fn build(world: &mut World, map: &Map) {
    world.init_resource::<Kit>();
    world.init_resource::<Built>();
    let kit = world.resource::<Kit>().clone();
    let old = std::mem::take(&mut world.resource_mut::<Built>().meshes);
    {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        for mesh in old {
            meshes.remove(&mesh);
        }
    }
    let (mut made, mut things) = (vec![], vec![]);
    let rings = rings(map);
    for (index, deck) in map.decks.iter().enumerate().filter(|(_, d)| round(d)) {
        let ring = rings.iter().any(|r| r.deck == index);
        let top = world.resource_mut::<Assets<Mesh>>().add({
            let [x0, z0, x1, z1] = deck.rect.map(|v| v as f32);
            lid((x1 - x0) / 2., (z1 - z0) / 2., 72)
        });
        steamer(world, &kit, deck, ring, &top);
        made.push(top);
        things.push(if ring { "Ring" } else { "Steamer" });
    }
    for block in map.blocks.iter().filter(|b| slim(b)) {
        post(world, &kit, block);
        things.push("Post");
    }
    for ring in &rings {
        for _ in string(world, &kit, ring) {
            things.push("Lantern");
        }
    }
    for block in &map.blocks {
        if let (GONG, Some(side)) = (block.style.as_str(), block.facing) {
            gong(world, &kit, block, crate::level::facing(block.rect.map(|v| v as f32), side));
            things.push("Gong");
        }
    }
    let mut built = world.resource_mut::<Built>();
    (built.meshes, built.things) = (made, things);
}

/// A bamboo steamer: its side, bands round it, its woven top, and on a ring the red rim and stripe.
fn steamer(world: &mut World, kit: &Kit, deck: &Deck, ring: bool, top: &Handle<Mesh>) {
    let [x0, z0, x1, z1] = deck.rect.map(|v| v as f32);
    let (h, w, d) = (deck.height as f32, x1 - x0, z1 - z0);
    let id = root(world, if ring { "Ring: a bamboo steamer's lid" } else { "Bamboo steamer" }, Transform::from_xyz((x0 + x1) / 2., FLOOR, (z0 + z1) / 2.));
    part(world, id, &kit.round, &kit.side, Transform::from_xyz(0., h / 2., 0.).with_scale(Vec3::new(w, h, d)), true);
    // Bands of darker strips round the top and the foot (and the middle of a tall one); a ring's top band is red.
    let band = (h * 0.3).min(0.009);
    let mut bands = vec![(h - band / 2., if ring { &kit.lacquer } else { &kit.band }), (band / 2., &kit.band)];
    if h > 0.06 {
        bands.push((h / 2., &kit.band));
    }
    for (y, material) in bands {
        part(world, id, &kit.round, material, Transform::from_xyz(0., y, 0.).with_scale(Vec3::new(w + 0.004, band, d + 0.004)), false);
    }
    part(world, id, top, &kit.top, Transform::from_xyz(0., h + 0.0004, 0.), false);
    if ring {
        // The edge of the ring, in red just inside the rim.
        part(world, id, &kit.stripe, &kit.lacquer, Transform::from_xyz(0., h + 0.0008, 0.).with_scale(Vec3::new(w / 2., 1., d / 2.)), false);
    }
}

/// A chopstick standing in a white holder with a blue band, a red knot at its top where a string would tie.
fn post(world: &mut World, kit: &Kit, block: &Block) {
    let [x0, z0, x1, z1] = block.rect.map(|v| v as f32);
    let (h, r) = (block.height as f32, (x1 - x0).min(z1 - z0) / 2.);
    let cup = 0.035_f32.min(h * 0.3);
    let id = root(world, "Chopstick post", Transform::from_xyz((x0 + x1) / 2., FLOOR, (z0 + z1) / 2.));
    part(world, id, &kit.round, &kit.ceramic, Transform::from_xyz(0., cup / 2., 0.).with_scale(Vec3::new(2. * r, cup, 2. * r)), true);
    part(world, id, &kit.round, &kit.glaze, Transform::from_xyz(0., cup * 0.62, 0.).with_scale(Vec3::new(2. * r + 0.002, cup * 0.18, 2. * r + 0.002)), false);
    let thick = (r * 0.25).clamp(0.003, 0.006);
    part(world, id, &kit.stick, &kit.stick_wood, Transform::from_xyz(0., (h + 0.01) / 2., 0.).with_scale(Vec3::new(thick, h - 0.01, thick)), true);
    part(world, id, &kit.round, &kit.tie, Transform::from_xyz(0., h - 0.035, 0.).with_scale(Vec3::new(2.15 * thick, 0.008, 2.15 * thick)), false);
    part(world, id, &kit.ball, &kit.lacquer, Transform::from_xyz(0., h - 0.014, 0.).with_scale(Vec3::splat(2.6 * thick)), false);
}

/// The string round a ring's posts, sagging between them, hung with pennants, a lantern in the middle of each run.
/// Returns the lanterns.
fn string(world: &mut World, kit: &Kit, ring: &Ring) -> Vec<Entity> {
    let id = root(world, "Ring string", Transform::IDENTITY);
    let mut lanterns = vec![];
    let n = ring.posts.len();
    for k in 0..n {
        let (a, b) = (ring.posts[k] - Vec3::Y * 0.014, ring.posts[(k + 1) % n] - Vec3::Y * 0.014);
        let at = |t: f32| a.lerp(b, t) - Vec3::Y * SAG * 4. * t * (1. - t);
        let pieces = 10;
        for i in 0..pieces {
            rod(world, id, kit, &kit.string, at(i as f32 / pieces as f32), at((i + 1) as f32 / pieces as f32), 0.0011, false);
        }
        // Pennants every 6 cm or so, hanging below the string, turned along it.
        let span = a.distance(b);
        let flags = ((span / 0.06) as usize).max(2);
        let along = (b - a).with_y(0.).normalize_or(Vec3::X);
        let turn = Quat::from_rotation_y(-along.z.atan2(along.x));
        for i in 1..flags {
            let t = i as f32 / flags as f32;
            if (t - 0.5).abs() < 0.08 {
                continue;
            }
            let p = at(t) - Vec3::Y * 0.011;
            part(world, id, &kit.cube, &kit.pennants[i % 3], Transform::from_translation(p).with_rotation(turn * Quat::from_rotation_z(PI / 4.)).with_scale(Vec3::new(0.014, 0.014, 0.0008)), false);
        }
        // The lantern: red paper round a light, black caps, a gold tassel, on a cord from the string.
        let hang = at(0.5);
        let lamp = hang - Vec3::Y * DROP;
        rod(world, id, kit, &kit.string, hang, lamp + Vec3::Y * 0.02, 0.0009, false);
        let lantern = world
            .spawn((
                Name::new("Lantern"),
                Transform::from_translation(lamp),
                Visibility::default(),
                PointLight { intensity: LANTERN_LUMENS, range: LANTERN_RANGE, radius: 0.015, color: Color::srgb(1.0, 0.62, 0.30), shadow_maps_enabled: false, ..default() },
            ))
            .id();
        world.entity_mut(id).add_child(lantern);
        part(world, lantern, &kit.ball, &kit.paper, Transform::from_scale(Vec3::new(0.032, 0.036, 0.032)), false);
        for y in [0.017, -0.017] {
            part(world, lantern, &kit.round, &kit.cap, Transform::from_xyz(0., y, 0.).with_scale(Vec3::new(0.016, 0.005, 0.016)), false);
        }
        part(world, lantern, &kit.coarse, &kit.gold, Transform::from_xyz(0., -0.03, 0.).with_scale(Vec3::new(0.004, 0.018, 0.004)), false);
        lanterns.push(lantern);
    }
    lanterns
}

/// The wok gong in its frame, built facing -z in `frame` (`level::facing`: w across the front, d deep).
fn gong(world: &mut World, kit: &Kit, block: &Block, (frame, w, _d): (Transform, f32, f32)) {
    let h = block.height as f32;
    let id = root(world, "Gong: a wok on a frame of chopsticks", frame);
    let bar = h * 0.94;
    for x in [-1., 1.] {
        let x = x * w * 0.44;
        part(world, id, &kit.round, &kit.ceramic, Transform::from_xyz(x, 0.0175, 0.).with_scale(Vec3::new(0.04, 0.035, 0.04)), true);
        part(world, id, &kit.round, &kit.glaze, Transform::from_xyz(x, 0.022, 0.).with_scale(Vec3::new(0.0415, 0.006, 0.0415)), false);
        part(world, id, &kit.stick, &kit.stick_wood, Transform::from_xyz(x, (h + 0.01) / 2., 0.).with_scale(Vec3::new(0.0055, h - 0.01, 0.0055)), true);
        part(world, id, &kit.round, &kit.tie, Transform::from_xyz(x, bar, 0.).with_scale(Vec3::new(0.016, 0.014, 0.016)), false);
    }
    rod(world, id, kit, &kit.stick_wood, Vec3::new(-w * 0.49, bar, 0.), Vec3::new(w * 0.49, bar, 0.), 0.0048, true);
    // The wok swings from the bar: its pivot turns about x.
    let pivot = world.spawn((Name::new("Gong pivot"), Gong::default(), Transform::from_xyz(0., bar, 0.), Visibility::default())).id();
    world.entity_mut(id).add_child(pivot);
    let size = (w * 0.66).min(h * 0.62);
    let middle = -(size * 0.5 + 0.03);
    let mut swinging = vec![];
    for x in [-1., 1.] {
        swinging.push(rod(world, pivot, kit, &kit.string, Vec3::new(x * size * 0.22, 0., 0.), Vec3::new(x * size * 0.3, middle + size * 0.42, 0.), 0.001, false));
    }
    // On its side, its bowl's back (the face that is struck) to the front: the lathe's up turned to -z.
    let face = Transform::from_xyz(0., middle, size * 0.18).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)).with_scale(Vec3::splat(size));
    swinging.push(part(world, pivot, &kit.wok, &kit.steel, face, true));
    swinging.push(part(world, pivot, &kit.ball, &kit.boss, Transform::from_xyz(0., middle, size * 0.18 - 0.004).with_scale(Vec3::new(size * 0.24, size * 0.24, 0.01)), false));
    swinging.push(rod(world, pivot, kit, &kit.walnut, Vec3::new(size * 0.5, middle, size * 0.3), Vec3::new(size * 0.5 + 0.075, middle + 0.012, size * 0.3), 0.0075, true));
    for part in swinging {
        world.entity_mut(part).remove::<crate::bake::Still>();
    }
    // The striker: a wooden spoon leaning on the right-hand post.
    let (foot, tip) = (Vec3::new(w * 0.36, 0., -0.035), Vec3::new(w * 0.42, h * 0.42, -0.012));
    rod(world, id, kit, &kit.beech, foot, tip, 0.0035, true);
    part(world, id, &kit.ball, &kit.beech, Transform::from_translation(tip + (tip - foot).normalize() * 0.016).with_scale(Vec3::new(0.022, 0.032, 0.01)), true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<Image>>();
        world
    }
    fn dojo() -> Map {
        Map::parse(include_str!("../assets/maps/kung-fu-dojo.json")).unwrap()
    }

    #[test]
    fn the_dojo_has_one_ring_with_four_posts_in_order_round_it_and_a_stand_of_steamers_and_a_gong() {
        let map = dojo();
        let rings = rings(&map);
        assert_eq!(rings.len(), 1);
        assert_eq!((rings[0].deck, rings[0].posts.len()), (0, 4));
        // In order round the ring's middle.
        let c = Vec2::new(0., 2.78);
        let angles: Vec<f32> = rings[0].posts.iter().map(|p| (p.z - c.y).atan2(p.x - c.x)).collect();
        assert!(angles.windows(2).all(|w| w[0] < w[1]), "{angles:?}");
        let mut world = world();
        crate::level::build(&mut world, &map);
        let things = world.resource::<Built>().things.clone();
        let count = |name: &str| things.iter().filter(|t| **t == name).count();
        assert_eq!((count("Ring"), count("Steamer"), count("Post"), count("Lantern"), count("Gong")), (1, 3, 4, 4, 1), "{things:?}");
        // A lantern's light is warm, and there are four of them.
        let lights: Vec<PointLight> = world.query::<&PointLight>().iter(&world).cloned().collect();
        assert_eq!(lights.len(), 4);
        assert!(lights.iter().all(|l| !l.shadow_maps_enabled && l.color.to_srgba().red > l.color.to_srgba().blue));
        // The kitchen leaves the gong to this module (it would draw a tap), and the level draws no slab for a post or a steamer.
        assert!(!crate::kitchen::drawn(GONG));
        let slabs = world.query::<&Name>().iter(&world).filter(|n| n.as_str() == "Block" || n.as_str() == "Deck").count();
        assert_eq!(slabs, 0);
    }

    #[test]
    fn rebuilding_frees_the_last_maps_woven_tops_and_adds_no_shared_assets() {
        let mut world = world();
        let map = dojo();
        build(&mut world, &map);
        let (meshes, materials, images) = (world.resource::<Assets<Mesh>>().len(), world.resource::<Assets<StandardMaterial>>().len(), world.resource::<Assets<Image>>().len());
        build(&mut world, &map);
        assert_eq!(world.resource::<Assets<Mesh>>().len(), meshes);
        assert_eq!((world.resource::<Assets<StandardMaterial>>().len(), world.resource::<Assets<Image>>().len()), (materials, images));
        // Without a ring (a map with no bamboo), nothing.
        build(&mut world, &Map::bundled());
        assert!(world.resource::<Built>().things.is_empty());
    }

    #[test]
    fn a_slim_bamboo_block_is_a_chopstick_and_a_wide_one_is_not() {
        let block = |w: f64| Block { rect: [0., 0., w, w], height: 0.27, style: ROUND.into(), facing: None };
        assert!(slim(&block(0.036)) && slim(&block(0.08)) && !slim(&block(0.09)));
        assert!(!slim(&Block { style: "wood".into(), ..block(0.03) }));
    }

    #[test]
    fn the_weave_repeats_seamlessly_and_has_dark_seams_between_light_strips() {
        let image = weave(64);
        let data = image.data.as_ref().unwrap();
        let at = |x: usize, y: usize| data[(y * 64 + x) * 4] as f32;
        // The first column is the last's neighbour: no seam where it repeats beyond what a strip's own seam is.
        let (light, dark) = (data.chunks(4).map(|p| p[0]).max().unwrap() as f32, data.chunks(4).map(|p| p[0]).min().unwrap() as f32);
        assert!(light > 180. && dark < light * 0.6, "{light} {dark}");
        assert!((0..64).all(|y| (at(0, y) - at(63, y)).abs() < light * 0.6));
    }

    #[test]
    fn a_struck_gong_swings_and_comes_to_rest() {
        let mut app = App::new();
        app.init_resource::<Time>();
        let pivot = app.world_mut().spawn((Gong::default(), Transform::default())).id();
        strike(app.world_mut(), 1.);
        let mut most = 0f32;
        for _ in 0..240 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(16));
            app.world_mut().run_system_once(swing).unwrap();
            most = most.max(app.world().get::<Gong>(pivot).unwrap().angle.abs());
        }
        assert!(most > 0.1, "it swings: {most}");
        assert!(app.world().get::<Gong>(pivot).unwrap().angle.abs() < 0.01, "and settles");
    }
}

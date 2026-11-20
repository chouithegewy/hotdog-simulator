// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The relish bazooka's burst, as it is seen, heard and felt wherever a glob bursts: a splat of weapon 4 from the
//! simulation or the server, whoever fired it, a person, another player or a bot (`splatter::land`). No other gun's
//! round sets it off. Nothing here goes back into the simulation.
//!
//! * **The burst.** A bright core flash; a ball of chunky green relish that swells and bursts at once into dozens of
//!   globs and pickle chunks flung out on ballistic arcs, and a spray of fine drops; a mist of green that billows out
//!   and thins away; and a shockwave ring that runs out along the ground and fades, marking the kill zone. The blast is
//!   the simulation's own: it reaches the glob's splash radius as it is tuned (online, the server's) from where it burst
//!   to the nearest point of a body, its damage falling to nothing there ([`radius`]). The ring is where a hotdog
//!   standing on the ground is reached: its feet within the ring, it is hurt; outside it, not ([`kill_zone`]). The
//!   ball, the globs and the mist fly further, for show; the big splats are thrown at the surfaces within it.
//! * **Splatter everywhere.** Large splats are thrown out over the floor, the blocks and the walls within the blast,
//!   radiating from where it burst and drawn out away from it, every block face within reach taking at least one; on
//!   an upright face they run in drips as any splat does. How many goes by the blast's area ([`splat_count`]). The
//!   hotdogs and the loose objects within it are smeared and marked, and each glob leaves a smaller splat where it
//!   lands. They are stains like any other (splatter.rs): within the quads the graphics level allows, drying and going
//!   after `LIFE`.
//! * **The lens.** Within about the blast's radius of the camera, with nothing solid between, relish spatters the lens:
//!   a few green blobs, more and bigger the nearer, which sag, slide down and fade ([`lens_count`]).
//! * **Force.** The camera shakes, hard up close and slightly from across the map ([`jolt`]). The loose objects in the
//!   blast jolt because the simulation throws them (`Props::blast`); nothing is added here.
//! * **Light.** At the High level only, a short green pulse of light on the room round it.
//! * **Sound.** The boom, with a wet splat over it and, near it, a deep thump (assets/audio/relish-splat.wav and
//!   relish-thump.wav, made by tools/build_relish_sounds.py): louder and bassier the nearer ([`voices`]).
//!
//! **Cost.** Each graphics level has its [`Plan`]: how many of each part one burst draws and how many bursts are drawn
//! at once (a burst beyond that takes the oldest one's place). A burst's entities are its slot's, made the first time
//! the slot is used and kept, hidden, between bursts; their meshes and materials are shared, and are of kinds the game
//! already draws (no new shader to compile at the first shot). Setting one off spawns nothing after the first few, and
//! a frame allocates nothing. [`SPAN`] seconds after a burst nothing of it is moved or looked at again: its splats
//! stay, as stains, and the rest is hidden.
use super::*;
use bevy::audio::Volume;
use std::sync::Arc;

/// Seconds a burst is drawn for, from its flash to the last chunk shrinking away. After it nothing of the burst runs.
pub const SPAN: f32 = 2.6;
/// How hard the burst's globs, chunks and drops fall, m/s² (faster than a stray drop's `FALL`, so that the burst's arcs
/// are quick and read as thrown).
const DROP: f32 = 5.5;
/// How far from the camera, as a share of the blast's radius, relish reaches the lens.
pub const LENS_REACH: f32 = 1.25;
/// Seconds the shockwave ring takes to run out to the blast's edge, holds there, and has faded by.
const RING_OUT: f32 = 0.18;
const RING_HOLD: f32 = 0.45;
const RING_GONE: f32 = 0.75;
/// The camera's shake at its hardest (radians), and the most that bursts together shake it.
pub const SHAKE: f32 = 0.05;
const SHAKE_MOST: f32 = 0.085;
/// Seconds a shake takes to die away, as an exponential's time constant, and after which it is over.
const SHAKE_TAU: f32 = 0.14;
const SHAKE_LONG: f32 = 0.7;
/// Bursts shaking the camera at once (the faintest goes for a new one).
const KICKS: usize = 4;
/// The burst's light: its brightest (lumens), and seconds it lasts.
const LIGHT: f32 = 900.;
const LIGHT_LIFE: f32 = 0.3;

/// The blast's radius, in metres: how far the relish glob's blast reaches from where it burst to the nearest point of a
/// body, its damage falling to nothing there (the splash as the round is tuned, `tune.splash`, which online is the
/// server's; the simulation's own constant if the tuning has none).
pub fn radius(feast: &Feast) -> f32 {
    let tuned = feast.tune.splash.get(BAZOOKA).copied().unwrap_or(0.);
    if tuned > 0.01 { tuned } else { feast::SPLASH[BAZOOKA].max(0.05) }
}

/// How far from the point on the ground under a burst `height` above it a hotdog standing there is reached by a blast
/// of `radius`: the ring's radius, the kill zone. The simulation measures the blast to the nearest point of a body, and
/// a standing hotdog's is a capsule `HOTDOG_RADIUS` round from its feet to `actor::HEIGHT` (`feast::figure`); so this is
/// the blast's sphere grown by that radius, met at the height of the capsule's axis nearest the burst. None when the
/// burst is too high for anybody standing under it to be reached.
pub fn kill_zone(radius: f32, height: f32) -> Option<f32> {
    let body = feast::HOTDOG_RADIUS as f32;
    let (low, high) = (body, (kitchen_sim::actor::HEIGHT as f32 - body).max(body));
    let off = if height < low { low - height } else if height > high { height - high } else { 0. };
    let reach = radius + body;
    let capsule = (off < reach).then(|| (reach * reach - off * off).sqrt());
    // And its head (kitchen_sim head.rs `body_gap`), looking level, which reaches over the capsule: for a hotdog facing
    // the burst or turned from it, the middle of the head's line across it.
    let (off, reach) = ((height - kitchen_sim::head::HEAD_AT).abs(), radius + kitchen_sim::head::HEAD_RADIUS);
    let head = (off < reach).then(|| (reach * reach - off * off).sqrt());
    capsule.into_iter().chain(head).reduce(f32::max)
}

/// How much of a burst a graphics level draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    /// Bursts drawn at once; a burst beyond these takes the place of the oldest.
    pub bursts: usize,
    /// Core flashes (camera-facing cards, added light).
    pub flashes: usize,
    /// Puffs of mist (camera-facing cards).
    pub mist: usize,
    /// Globs flung out, `big` of them the slow, fat ones the ball breaks into.
    pub globs: usize,
    pub big: usize,
    /// Pickle chunks.
    pub chunks: usize,
    /// Fine drops.
    pub drops: usize,
    /// The big splats one burst throws round itself at most, and how many it throws for each square metre of the blast
    /// (`splat_count`).
    pub splats: usize,
    pub per_area: f32,
    /// The small splats its globs leave where they land, at most.
    pub landing: usize,
    /// Hotdogs smeared, and loose objects marked, at most.
    pub smears: usize,
    pub props: usize,
    /// Blobs on the lens at most.
    pub lens: usize,
    /// Whether the burst lights the room.
    pub light: bool,
    /// Whether the globs are drawn with the finer ball (the coarser one at Low and Lowest).
    pub fine: bool,
}
impl Plan {
    pub fn of(level: Level) -> Self {
        match level {
            Level::High => Plan { bursts: 4, flashes: 2, mist: 14, globs: 48, big: 12, chunks: 20, drops: 24, splats: 22, per_area: 140., landing: 20, smears: 4, props: 4, lens: 10, light: true, fine: true },
            Level::Medium => Plan { bursts: 3, flashes: 2, mist: 10, globs: 34, big: 9, chunks: 14, drops: 14, splats: 16, per_area: 110., landing: 14, smears: 3, props: 3, lens: 8, light: false, fine: true },
            Level::Low => Plan { bursts: 2, flashes: 1, mist: 5, globs: 14, big: 5, chunks: 7, drops: 6, splats: 10, per_area: 70., landing: 6, smears: 2, props: 2, lens: 5, light: false, fine: false },
            Level::Lowest => Plan { bursts: 2, flashes: 1, mist: 3, globs: 8, big: 3, chunks: 4, drops: 0, splats: 6, per_area: 50., landing: 3, smears: 1, props: 1, lens: 3, light: false, fine: false },
        }
    }
    /// The entities one burst draws at most: its flashes, the ring, the ball, the mist, the globs, chunks and drops.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn particles(&self) -> usize {
        self.flashes + 1 + 1 + self.mist + self.globs + self.chunks + self.drops
    }
}
/// The big splats a burst of `radius` throws round itself: by the blast's area, at least four, at most the plan's.
pub fn splat_count(plan: &Plan, radius: f32) -> usize {
    ((plan.per_area * PI * radius * radius).round() as usize).clamp(4.min(plan.splats), plan.splats)
}
/// How many blobs of relish land on the lens of a camera `distance` from a burst of `radius`: none beyond
/// `LENS_REACH` radii, one at the edge of that, the plan's most at the burst.
pub fn lens_count(plan: &Plan, distance: f32, radius: f32) -> usize {
    let reach = radius * LENS_REACH;
    if distance >= reach || plan.lens == 0 {
        return 0;
    }
    ((plan.lens as f32 * (1. - distance / reach).powf(0.45)).ceil() as usize).clamp(1, plan.lens)
}
/// How hard (radians) a burst of `radius` shakes a camera `distance` from it: `SHAKE` at the burst, about two thirds
/// of it at the blast's edge, falling off quickly beyond and then slowly, so that from across the map it is a slight
/// tremor.
pub fn jolt(distance: f32, radius: f32) -> f32 {
    let near = 1.5 * radius.max(0.05);
    SHAKE * (0.92 / (1. + (distance / near).powi(2)) + 0.08 / (1. + distance / 1.5))
}
/// How near a camera `distance` from a burst of `radius` is, 1 at the burst to 0 far off: what the sound goes by.
fn closeness(distance: f32, radius: f32) -> f32 {
    1. / (1. + (distance / (1.4 * radius.max(0.05))).powi(2))
}
/// The burst's three sounds as heard `distance` from it, each its volume and speed: the boom (slowed, deeper, near it),
/// the wet splat, and the deep thump (only near it). Louder and bassier the nearer.
pub fn voices(distance: f32, radius: f32) -> [(f32, f32); 3] {
    let c = closeness(distance, radius);
    let thump = c.powf(1.2);
    [(0.42 + 0.58 * c, 1. - 0.2 * c), (0.28 + 0.72 * c.powf(0.8), 1.04 - 0.12 * c), (if thump > 0.03 { thump } else { 0. }, 1.)]
}

/// What the burst is drawn with, besides the shared `Look`, made once.
pub struct Kit {
    /// A flat card showing a whole texture (the ring's), and a coarser ball than the shared one.
    plate: Handle<Mesh>,
    pebble: Handle<Mesh>,
    /// The ring, fading, step by step.
    ring: [Handle<StandardMaterial>; 4],
    /// The mist, pale and dark, thinning, step by step.
    mist: [[Handle<StandardMaterial>; 3]; 2],
    /// Blobs of relish for the lens.
    lens: [Handle<Image>; 3],
    /// The boom, the wet splat and the thump, when there is an asset server to load them.
    sounds: Option<[Handle<AudioSource>; 3]>,
}
impl Kit {
    fn new(world: &mut World, look: &Look) -> Self {
        let plate = world.resource_mut::<Assets<Mesh>>().add(plate_mesh());
        let pebble = world.resource_mut::<Assets<Mesh>>().add(Sphere::new(1.).mesh().uv(8, 6));
        let (ring_image, lens) = {
            let mut images = world.resource_mut::<Assets<Image>>();
            (images.add(ring_image()), [0, 1, 2].map(|k| images.add(lens_image(k))))
        };
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        // Laid over what is behind it as the flame's cards are: its bright edge adds light, its body darkens a little.
        let ring = [1., 0.7, 0.42, 0.18].map(|k| {
            mats.add(StandardMaterial {
                base_color: Color::LinearRgba(LinearRgba::new(k, k, k, k)),
                base_color_texture: Some(ring_image.clone()),
                alpha_mode: AlphaMode::Premultiplied,
                unlit: true,
                fog_enabled: false,
                ..default()
            })
        });
        // A veil, as the steam is.
        let mist = [([0.5, 0.76, 0.24], [0.88, 0.6, 0.26]), ([0.12, 0.28, 0.03], [0.9, 0.62, 0.28])].map(|(c, alphas)| {
            alphas.map(|a| mats.add(StandardMaterial { base_color: Color::srgba(c[0], c[1], c[2], a), base_color_texture: Some(look.cards.clone()), alpha_mode: AlphaMode::Blend, unlit: true, ..default() }))
        });
        let sounds = world.get_resource::<AssetServer>().map(|server| ["audio/boom.wav", "audio/relish-splat.wav", "audio/relish-thump.wav"].map(|path| server.load(path)));
        Kit { plate, pebble, ring, mist, lens, sounds }
    }
}
/// A card in its xy plane facing +z, one metre square, showing the whole of its texture (laid flat for the ring).
fn plate_mesh() -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-0.5, -0.5, 0.], [0.5, -0.5, 0.], [0.5, 0.5, 0.], [-0.5, 0.5, 0.]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}
/// The shockwave ring, premultiplied: a thin bright leading edge of pale lime that adds light, and inside it a
/// translucent green body fading in towards the middle, which is clear.
fn ring_image() -> Image {
    let size = 256usize;
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new((x as f32 + 0.5) / size as f32 * 2. - 1., (y as f32 + 0.5) / size as f32 * 2. - 1.);
            let u = p.length();
            let wobble = 0.012 * (fbm(p.x * 6., p.y * 6., 41) - 0.5);
            let edge = (-((u + wobble - 0.935) / 0.04).powi(2)).exp();
            let body = smooth((u - 0.45) / 0.45) * (1. - smooth((u + wobble - 0.94) / 0.03));
            let a = (0.55 * body + edge).min(1.) * (1. - smooth((u - 0.985) / 0.015));
            let glow = Vec3::new(0.8, 1.0, 0.42) * edge;
            let shade = Vec3::new(0.2, 0.42, 0.05) * 0.55 * body;
            let c = (glow + shade).min(Vec3::ONE) * (1. - smooth((u - 0.985) / 0.015));
            out.extend([c.x, c.y, c.z, a].map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8));
        }
    }
    let (data, levels) = mips(out, size, size, false, false);
    let mut image = Image::new_uninit(Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 }, TextureDimension::D2, TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = sampler(1);
    image
}
/// A blob of relish on the lens, one of three shapes: a lumpy translucent drop sagging into a heavier bottom, thicker and
/// darker at its rim where it has piled up, paler bits of pickle in it, a wet highlight, and specks flung round it.
/// sRGB, alpha not premultiplied (for the interface's image nodes).
fn lens_image(variant: u32) -> Image {
    let size = 128usize;
    let seed = 300 + variant * 31;
    let n = |k: u32| hash(seed.wrapping_mul(131).wrapping_add(k));
    let tones = Sauce::Relish.tones().map(Vec3::from);
    let bits: Vec<(Vec2, f32, Vec2)> = (0..7).map(|k| (Vec2::new(n(k) - 0.5, n(k + 10) - 0.55) * 0.62, 0.05 + 0.04 * n(k + 20), Vec2::from_angle(n(k + 30) * TAU))).collect();
    let specks: Vec<(Vec2, f32)> = (0..8).map(|k| (Vec2::from_angle(n(k + 40) * TAU) * (0.62 + 0.28 * n(k + 50)), 0.025 + 0.045 * n(k + 60))).collect();
    // How far it sags below its middle, and a little to one side.
    let (sag, lean) = (0.35 + 0.3 * n(72), 0.25 * (n(73) - 0.5));
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new((x as f32 + 0.5) / size as f32 * 2. - 1., (y as f32 + 0.5) / size as f32 * 2. - 1.);
            // (Below its middle it is drawn out downwards and a little aside: a drop sagging under its weight.)
            let mut q = p - Vec2::new(0., -0.12);
            if q.y > 0. {
                q = Vec2::new(q.x - lean * q.y, q.y / (1. + sag * smooth(q.y / 0.5)));
            }
            let warp = 0.2 * (fbm(q.x * 2.6 + 7., q.y * 2.6, seed) - 0.5);
            let r = (q.length() + warp) / 0.5;
            let body = 1. - smooth((r - 0.9) / 0.14);
            let speck = specks.iter().map(|(c, s)| 1. - smooth(((p - *c).length() / s - 0.8) / 0.25)).fold(0f32, f32::max);
            let cover = body.max(speck);
            if cover <= 0.004 {
                out.extend([0, 0, 0, 0]);
                continue;
            }
            // Thicker (darker) at the rim, where it has piled up; its body mottled; paler bits of pickle in it.
            let rim = smooth((r - 0.62) / 0.3) * body;
            let mut c = tones[1].lerp(tones[0], 0.6 * rim).lerp(tones[2], 0.3 * fbm(p.x * 5., p.y * 5., seed + 3) * (1. - rim));
            let bit = bits.iter().map(|(at, s, turn)| {
                let d = turn.rotate(p - *at) / s;
                1. - smooth(((d.x.powi(4) + (d.y * 1.3).powi(4)).powf(0.25) - 0.75) / 0.3)
            });
            let k = bit.fold(0f32, f32::max) * body * (1. - rim);
            c = c.lerp(Vec3::new(0.62, 0.8, 0.3), 0.45 * k);
            // A wet highlight up and to the left, and a soft one along the lower rim.
            let shine = (-((p - Vec2::new(-0.18, -0.3)).length_squared() / 0.01)).exp() * body + 0.3 * (-((r - 0.8) / 0.08).powi(2)).exp() * smooth((p.y - 0.05) / 0.3) * body;
            c = c.lerp(Vec3::splat(1.), shine.min(1.) * 0.8);
            let a = (cover * (0.72 + 0.22 * rim + 0.1 * k)).min(0.95);
            out.extend([c.x, c.y, c.z].map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8));
            out.push((a.clamp(0., 1.) * 255. + 0.5) as u8);
        }
    }
    Image::new(Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 }, TextureDimension::D2, out, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

/// One drawn part of a burst that moves: the ball, a glob, a chunk or a drop.
#[derive(Clone, Copy)]
struct Bit {
    entity: Entity,
    /// In use by the burst under way.
    on: bool,
    /// In the air; once landed a chunk lies where it fell, and a glob or a drop is gone.
    flying: bool,
    /// One of the big globs the ball bursts into.
    big: bool,
    at: Vec3,
    velocity: Vec3,
    size: Vec3,
    spin: Vec3,
}
impl Bit {
    fn new(entity: Entity) -> Self {
        Bit { entity, on: false, flying: false, big: false, at: Vec3::ZERO, velocity: Vec3::ZERO, size: Vec3::ZERO, spin: Vec3::ZERO }
    }
}
/// A puff of mist.
#[derive(Clone, Copy)]
struct Puff {
    entity: Entity,
    on: bool,
    dark: bool,
    at: Vec3,
    velocity: Vec3,
    /// Its size at first and at the end, its life and the seconds before it shows.
    from: f32,
    to: f32,
    life: f32,
    delay: f32,
    roll: f32,
}
/// A core flash: its card, how big at its brightest, and its life.
#[derive(Clone, Copy)]
struct Flash {
    entity: Entity,
    on: bool,
    peak: f32,
    life: f32,
}
/// A burst's place: its entities, kept between bursts, and the burst under way, if there is one.
struct Slot {
    live: bool,
    /// The order bursts were set off in: the oldest gives up its slot first.
    order: u64,
    age: f32,
    seed: u32,
    radius: f32,
    centre: Vec3,
    /// Where on the ground the ring runs out from, and how far (`kill_zone`).
    ring: Option<(Vec3, f32)>,
    plate: Option<Entity>,
    flashes: Vec<Flash>,
    mist: Vec<Puff>,
    core: Vec<Bit>,
    globs: Vec<Bit>,
    chunks: Vec<Bit>,
    drops: Vec<Bit>,
    /// Splats its globs may still leave.
    landing: usize,
}
impl Slot {
    fn new() -> Self {
        Slot { live: false, order: 0, age: 0., seed: 0, radius: 0., centre: Vec3::ZERO, ring: None, plate: None, flashes: vec![], mist: vec![], core: vec![], globs: vec![], chunks: vec![], drops: vec![], landing: 0 }
    }
    /// Every entity it has made.
    fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.plate.into_iter().chain(self.flashes.iter().map(|f| f.entity)).chain(self.mist.iter().map(|p| p.entity)).chain([&self.core, &self.globs, &self.chunks, &self.drops].into_iter().flat_map(|bits| bits.iter().map(|b| b.entity)))
    }
    /// Puts the burst away: everything hidden and still, the cards no longer turned to the camera.
    fn retire(&mut self, world: &mut World) {
        self.live = false;
        if let Some(e) = self.plate {
            show(world, e, false);
        }
        for f in &mut self.flashes {
            if std::mem::take(&mut f.on) {
                hide_card(world, f.entity);
            }
        }
        for p in &mut self.mist {
            if std::mem::take(&mut p.on) {
                hide_card(world, p.entity);
            }
        }
        for bits in [&mut self.core, &mut self.globs, &mut self.chunks, &mut self.drops] {
            for b in bits.iter_mut() {
                if std::mem::take(&mut b.on) {
                    show(world, b.entity, false);
                }
            }
        }
    }
}
fn show(world: &mut World, e: Entity, on: bool) {
    let want = if on { Visibility::Inherited } else { Visibility::Hidden };
    if let Some(mut v) = world.get_mut::<Visibility>(e) {
        if *v != want {
            *v = want;
        }
    }
}
fn hide_card(world: &mut World, e: Entity) {
    if let Ok(mut entity) = world.get_entity_mut(e) {
        entity.remove::<Card>();
        entity.insert(Visibility::Hidden);
    }
}
fn place(world: &mut World, e: Entity, tr: Transform) {
    if let Some(mut t) = world.get_mut::<Transform>(e) {
        *t = tr;
    }
}
/// Draws `e` with `mesh` and `material`, changing only what differs.
fn dress(world: &mut World, e: Entity, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>) {
    if world.get::<Mesh3d>(e).is_none_or(|m| m.0 != *mesh) {
        world.entity_mut(e).insert(Mesh3d(mesh.clone()));
    }
    if world.get::<MeshMaterial3d<StandardMaterial>>(e).is_none_or(|m| m.0 != *material) {
        world.entity_mut(e).insert(MeshMaterial3d(material.clone()));
    }
}
/// A hidden entity for a burst's part, drawn with `mesh` and `material`.
fn part(world: &mut World, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>) -> Entity {
    world.spawn((Name::new("Relish burst"), Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_scale(Vec3::ZERO), Visibility::Hidden, NotShadowCaster)).id()
}
/// The entity at `i` of a slot's list, made (or made again, if something took it away) when it is not there.
fn ensure(world: &mut World, e: Option<Entity>, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>) -> Entity {
    match e {
        Some(e) if world.get_entity(e).is_ok() => {
            dress(world, e, mesh, material);
            e
        }
        _ => part(world, mesh, material),
    }
}

/// A blob of relish on the lens.
#[derive(Clone, Copy)]
struct Smudge {
    entity: Entity,
    on: bool,
    age: f32,
    /// Seconds it clings before it slides, and in all.
    hold: f32,
    life: f32,
    /// How fast it starts sliding and how it speeds up (screen heights a second, and a second squared, in vh).
    speed: f32,
    fall: f32,
    /// How opaque at most, and how it sags as it slides.
    alpha: f32,
    sag: f32,
}
#[derive(Default)]
struct Lens {
    root: Option<Entity>,
    smudges: Vec<Smudge>,
}
/// One burst's shake of the camera: seconds since, how hard, and its own wobble.
#[derive(Clone, Copy, Default)]
struct Kick {
    age: f32,
    amp: f32,
    seed: f32,
}
/// A light flashed by a burst (High only).
#[derive(Clone, Copy)]
struct Lamp {
    entity: Entity,
    age: f32,
}

/// Every burst under way, their slots, the lens, the shake and the lights.
#[derive(Resource, Default)]
pub struct Booms {
    kit: Option<Arc<Kit>>,
    slots: Vec<Slot>,
    order: u64,
    /// Bursts set off so far.
    pub started: u32,
    lens: Lens,
    kicks: [Kick; KICKS],
    lamps: Vec<Lamp>,
    /// The big splats the last burst threw: where each met a surface, which way that faces, and how wide (for the
    /// checks and the tests).
    pub last: Vec<[f32; 7]>,
}
impl Booms {
    /// Bursts under way.
    #[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
    pub fn live(&self) -> usize {
        self.slots.iter().filter(|s| s.live).count()
    }
    /// Blobs on the lens now.
    #[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
    pub fn smudges(&self) -> usize {
        self.lens.smudges.iter().filter(|s| s.on).count()
    }
    /// Whether anything is under way: when not, a frame does nothing here.
    pub fn busy(&self) -> bool {
        self.slots.iter().any(|s| s.live) || self.lens.smudges.iter().any(|s| s.on) || self.kicks.iter().any(|k| k.amp > 0.) || self.lamps.iter().any(|l| l.age < LIGHT_LIFE)
    }
    /// Every entity the bursts have made (their slots', the lights' and the lens's).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn entities(&self) -> usize {
        self.slots.iter().map(|s| s.entities().count()).sum::<usize>() + self.lamps.len() + self.lens.smudges.len() + self.lens.root.iter().count()
    }
    /// The slot for a new burst: a free one among the first `cap`, or else the oldest of them.
    fn slot(&mut self, cap: usize) -> usize {
        let cap = cap.max(1);
        while self.slots.len() < cap {
            self.slots.push(Slot::new());
        }
        (0..cap).find(|i| !self.slots[*i].live).unwrap_or_else(|| (0..cap).min_by_key(|i| self.slots[*i].order).unwrap_or(0))
    }
}

/// What a burst is set off with.
struct Ignition {
    at: Vec3,
    normal: Vec3,
    radius: f32,
    ring: Option<(Vec3, f32)>,
    seed: u32,
}

/// The graphics level the game is drawn at.
fn level_of(world: &World) -> Level {
    world.get_resource::<crate::quality::Detail>().map_or(Level::High, |d| d.level)
}
/// The burst's kit, made the first time it is wanted (`prepare` makes it before the first shot).
fn kit(world: &mut World, look: &Look) -> Arc<Kit> {
    if !world.contains_resource::<Booms>() {
        world.init_resource::<Booms>();
    }
    if let Some(kit) = world.resource::<Booms>().kit.clone() {
        return kit;
    }
    let kit = Arc::new(Kit::new(world, look));
    world.resource_mut::<Booms>().kit = Some(kit.clone());
    kit
}
/// Makes what the burst is drawn with ahead of the first one.
pub(super) fn prepare(world: &mut World, look: &Look) {
    kit(world, look);
}
/// Where the game's camera is, and how it is turned: as it was last drawn (which is where a script that moves it
/// had it), or as it is set this frame before it has ever been drawn.
fn eye(world: &mut World) -> Option<(Vec3, Quat)> {
    let mut cameras = world.query_filtered::<(Option<&GlobalTransform>, &Transform), With<crate::maya::HotdogCamera>>();
    cameras.iter(world).next().map(|(global, local)| match global {
        Some(g) if *g != GlobalTransform::IDENTITY => {
            let (_, turn, at) = g.to_scale_rotation_translation();
            (at, turn)
        }
        _ => (local.translation, local.rotation),
    })
}

/// A relish glob bursting at `p` on `surface`, having flown along `dir`: the burst, its splatter, the lens, the shake,
/// the light and the sound (see the module's notes). `seed` tells one burst from another.
#[allow(clippy::too_many_arguments)]
pub(super) fn burst(world: &mut World, mess: &mut Mess, look: &Look, g: &Game, p: Vec3, dir: Option<Vec3>, surface: Surface, seed: u32, now: f32) {
    let plan = Plan::of(level_of(world));
    let r = radius(&g.feast);
    let dir = dir.and_then(|d| d.try_normalize());
    let normal = match surface {
        Surface::Ground { normal, .. } | Surface::Wall { normal, .. } | Surface::Prop { normal, .. } => normal.normalize_or(Vec3::Y),
        // In the open or on a hotdog: back the way it came, tipped up.
        _ => dir.map_or(Vec3::Y, |d| (Vec3::Y - d).normalize_or(Vec3::Y)),
    };
    let level = &g.feast.level;
    let ground = under(level, p.x, p.y + 0.01, p.z);
    let height = (p.y - ground).max(0.);
    let ring = kill_zone(r, height).map(|reach| (Vec3::new(p.x, ground, p.z), reach));
    let kit = kit(world, look);
    let ignition = Ignition { at: p, normal, radius: r, ring, seed };
    let eye = eye(world);
    world.resource_scope(|world, mut booms: Mut<Booms>| {
        booms.started = booms.started.wrapping_add(1);
        ignite(world, &mut booms, look, &kit, &plan, &ignition, eye.map(|(at, _)| at));
        if let Some((at, turn)) = eye {
            felt(world, &mut booms, &kit, &plan, &g.feast, &ignition, at, turn);
        }
    });
    let thrown = throw_splats(world, mess, look, g, &ignition, &plan, surface, dir, now);
    let mut booms = world.resource_mut::<Booms>();
    booms.last.clear();
    booms.last.extend(thrown);
}

/// Sets the burst's parts going in a slot. `eye` is where the camera is, if there is one: the flash is drawn on the
/// near side of the ball, not inside it.
fn ignite(world: &mut World, booms: &mut Booms, look: &Look, kit: &Kit, plan: &Plan, b: &Ignition, eye: Option<Vec3>) {
    let i = booms.slot(plan.bursts);
    booms.order += 1;
    let order = booms.order;
    let slot = &mut booms.slots[i];
    if slot.live {
        slot.retire(world);
    }
    let n = |k: u32| hash(b.seed.wrapping_mul(7919).wrapping_add(k));
    let r = b.radius;
    let (t1, t2) = basis(b.normal);
    let centre = b.at + b.normal * (r * 0.06);
    (slot.live, slot.order, slot.age, slot.seed, slot.radius, slot.centre, slot.ring) = (true, order, 0., b.seed, r, centre, b.ring);
    slot.landing = plan.landing;
    let relish = Sauce::Relish.index();
    // Which way out, round the normal: `rise` the sine of how far up from the surface.
    let out = |a: f32, rise: f32| (t1 * a.cos() + t2 * a.sin()) * (1. - rise * rise).max(0.).sqrt() + b.normal * rise;
    const GOLDEN: f32 = 2.399_963;
    // The ring along the ground.
    let plate = ensure(world, slot.plate, &kit.plate, &kit.ring[0]);
    slot.plate = Some(plate);
    show(world, plate, slot.ring.is_some());
    place(world, plate, Transform::from_scale(Vec3::ZERO));
    // The core flashes: a white-gold one, small and quick, and a bigger green-gold one; one alone is the green.
    let toward = eye.map_or(Vec3::ZERO, |eye| (eye - centre).normalize_or_zero() * (r * 0.55).min(eye.distance(centre) * 0.4));
    // (From across the map the green glow is a little bigger, so that it is seen at all; it is light, not the blast.)
    let afar = 1. + 0.6 * eye.map_or(0., |eye| ((eye.distance(centre) - 1.) / 2.).clamp(0., 1.));
    for k in 0..plan.flashes {
        let material = &look.flash[if plan.flashes == 1 { 1 } else { k.min(1) }];
        let e = ensure(world, slot.flashes.get(k).map(|f| f.entity), &look.card[PUFF], material);
        let (peak, life) = match (plan.flashes, k) {
            (1, _) => (r * 2.4, 0.26),
            (_, 0) => (r * 1.1, 0.11),
            _ => (r * 2.5, 0.28),
        };
        let peak = if k + 1 == plan.flashes { peak * afar } else { peak };
        let flash = Flash { entity: e, on: true, peak, life };
        if k < slot.flashes.len() {
            slot.flashes[k] = flash;
        } else {
            slot.flashes.push(flash);
        }
        world.entity_mut(e).insert((Card(n(k as u32) * TAU), Visibility::Inherited, Transform::from_translation(centre + b.normal * r * 0.2 + toward).with_scale(Vec3::ZERO)));
    }
    // The mist: puffs billowing out, mostly pale, one in four dark.
    for k in 0..plan.mist {
        let dark = k % 4 == 3;
        let e = ensure(world, slot.mist.get(k).map(|p| p.entity), &look.card[SMOKE], &kit.mist[dark as usize][0]);
        let way = out(k as f32 * GOLDEN + n(20) * TAU, 0.3 + 0.65 * n(30 + k as u32));
        let puff = Puff {
            entity: e,
            on: true,
            dark,
            at: centre + way * r * 0.2,
            velocity: way * r * (2. + 3.5 * n(40 + k as u32)),
            from: r * (0.8 + 0.4 * n(50 + k as u32)),
            to: r * (2.3 + 1.3 * n(60 + k as u32)) * if dark { 0.75 } else { 1. },
            life: if dark { 0.8 + 0.5 * n(70 + k as u32) } else { 1.1 + 0.8 * n(70 + k as u32) },
            delay: 0.008 * (k % 4) as f32,
            roll: n(80 + k as u32) * TAU,
        };
        if k < slot.mist.len() {
            slot.mist[k] = puff;
        } else {
            slot.mist.push(puff);
        }
        world.entity_mut(e).insert((Card(puff.roll), Visibility::Hidden, Transform::from_translation(puff.at).with_scale(Vec3::ZERO)));
    }
    // What flies: the ball, the globs it bursts into, chunks and drops. Their speed goes with the blast's radius, so
    // that the globs land out to two or three radii whatever it is tuned to.
    let speed = (r * DROP).sqrt();
    let ball = if plan.fine { &look.ball } else { &kit.pebble };
    let set = |world: &mut World, bits: &mut Vec<Bit>, k: usize, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, bit: Bit| {
        let e = ensure(world, bits.get(k).map(|b| b.entity), mesh, material);
        let bit = Bit { entity: e, on: true, flying: true, ..bit };
        if k < bits.len() {
            bits[k] = bit;
        } else {
            bits.push(bit);
        }
        world.entity_mut(e).insert((Visibility::Inherited, Transform::from_translation(bit.at).with_scale(Vec3::ZERO)));
    };
    set(world, &mut slot.core, 0, ball, &look.tone[relish][0], Bit { at: centre, ..Bit::new(Entity::PLACEHOLDER) });
    for k in 0..plan.globs {
        let big = k < plan.big;
        let way = out(k as f32 * GOLDEN + n(100) * TAU, if big { 0.55 + 0.4 * n(110 + k as u32) } else { 0.2 + 0.72 * n(110 + k as u32) });
        let v = way * speed * if big { 0.5 + 0.4 * n(120 + k as u32) } else { 0.9 + 1.25 * n(120 + k as u32) };
        let size = r * if big { 0.1 + 0.06 * n(130 + k as u32) } else { 0.04 + 0.045 * n(130 + k as u32) };
        let tone = if big { (k % 2) as usize } else { k % 3 };
        // The big ones are the ball: they start packed round its middle, the rest from inside it.
        let from = way * r * if big { 0.18 + 0.15 * n(140 + k as u32) } else { 0.08 * n(140 + k as u32) };
        set(world, &mut slot.globs, k, ball, &look.tone[relish][tone], Bit { at: centre + from, velocity: v, size: Vec3::splat(size), big, ..Bit::new(Entity::PLACEHOLDER) });
    }
    for k in 0..plan.chunks {
        let way = out(k as f32 * GOLDEN + n(200) * TAU, 0.25 + 0.65 * n(210 + k as u32));
        let v = way * speed * (0.75 + 1.1 * n(220 + k as u32));
        let size = Vec3::new(1., 0.55, 0.85) * r * (0.045 + 0.04 * n(230 + k as u32));
        let spin = Vec3::new(n(240 + k as u32) - 0.5, n(250 + k as u32) - 0.5, n(260 + k as u32) - 0.5) * 30.;
        set(world, &mut slot.chunks, k, &look.cube, &look.chunk[k % 2], Bit { at: centre, velocity: v, size, spin, ..Bit::new(Entity::PLACEHOLDER) });
    }
    for k in 0..plan.drops {
        let way = out(k as f32 * GOLDEN + n(300) * TAU, 0.1 + 0.5 * n(310 + k as u32));
        let v = way * speed * (1.3 + 1.1 * n(320 + k as u32));
        let size = Vec3::splat(r * (0.016 + 0.016 * n(330 + k as u32)));
        set(world, &mut slot.drops, k, &kit.pebble, &look.tone[relish][k % 3], Bit { at: centre, velocity: v, size, ..Bit::new(Entity::PLACEHOLDER) });
    }
    // Parts of a bigger plan than this one's stay hidden.
    let (flashes, mist) = (plan.flashes, plan.mist);
    for f in slot.flashes.iter_mut().skip(flashes) {
        f.on = false;
    }
    for p in slot.mist.iter_mut().skip(mist) {
        p.on = false;
    }
    for (bits, used) in [(&mut slot.globs, plan.globs), (&mut slot.chunks, plan.chunks), (&mut slot.drops, plan.drops)] {
        for b in bits.iter_mut().skip(used) {
            if std::mem::take(&mut b.on) {
                show(world, b.entity, false);
            }
        }
    }
    // The light (High only): the free lamp, or the one lit longest ago.
    if plan.light {
        booms.lamps.retain(|l| world.get_entity(l.entity).is_ok());
        let free = booms.lamps.iter().position(|l| l.age >= LIGHT_LIFE);
        let lamp = if let Some(free) = free {
            free
        } else if booms.lamps.len() < 2 {
            let e = world
                .spawn((
                    Name::new("Relish burst light"),
                    PointLight { color: Color::srgb(0.62, 1.0, 0.35), intensity: 0., range: r * 4., radius: 0.02, shadow_maps_enabled: false, ..default() },
                    Transform::default(),
                    Visibility::Hidden,
                    // Not one of the kitchen's own lamps (`visuals` dims those to the theme and takes these away with it).
                    crate::visuals::ThemeOwned,
                ))
                .id();
            booms.lamps.push(Lamp { entity: e, age: LIGHT_LIFE });
            booms.lamps.len() - 1
        } else {
            (0..booms.lamps.len()).max_by(|a, b| booms.lamps[*a].age.total_cmp(&booms.lamps[*b].age)).unwrap_or(0)
        };
        let l = &mut booms.lamps[lamp];
        l.age = 0.;
        let e = l.entity;
        if let Some(mut light) = world.get_mut::<PointLight>(e) {
            light.range = r * 4.;
            light.intensity = 0.;
        }
        place(world, e, Transform::from_translation(centre + b.normal * r * 0.3));
        show(world, e, true);
    }
}

/// What the camera at `eye` (turned `turn`) feels of a burst: the shake, the lens spattered if it is near with nothing
/// solid between, and the sound.
#[allow(clippy::too_many_arguments)]
fn felt(world: &mut World, booms: &mut Booms, kit: &Kit, plan: &Plan, feast: &Feast, b: &Ignition, eye: Vec3, turn: Quat) {
    let d = eye.distance(b.at);
    // The shake: in the place of the faintest under way.
    let amp = jolt(d, b.radius);
    let k = (0..KICKS).min_by(|x, y| left(&booms.kicks[*x]).total_cmp(&left(&booms.kicks[*y]))).unwrap_or(0);
    booms.kicks[k] = Kick { age: 0., amp, seed: hash(b.seed ^ 0x5bd1) * 100. };
    // The sound.
    if let Some(sounds) = &kit.sounds {
        for (sound, (volume, speed)) in sounds.iter().zip(voices(d, b.radius)) {
            if volume > 0. {
                world.spawn((AudioPlayer::new(sound.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)).with_speed(speed)));
            }
        }
    }
    // The lens.
    let count = lens_count(plan, d, b.radius);
    if count == 0 {
        return;
    }
    let from = b.at + b.normal * 0.02;
    let way = (eye - from).normalize_or(Vec3::Y);
    if feast.cover(from, way) < eye.distance(from) - 0.03 {
        return;
    }
    spatter(world, booms, kit, plan.lens, count, closeness(d, b.radius), turn.inverse() * (b.at - eye), b.seed);
}
/// How much of a shake is left: none once over.
fn left(k: &Kick) -> f32 {
    if k.amp <= 0. || k.age >= SHAKE_LONG { 0. } else { k.amp * (-k.age / SHAKE_TAU).exp() }
}
/// The camera's shake now, as a turn (radians) about its side (a pitch), its up (a yaw) and its forward axis (a roll).
pub fn shake(world: &World) -> Vec3 {
    let Some(booms) = world.get_resource::<Booms>() else {
        return Vec3::ZERO;
    };
    let mut sum = Vec3::ZERO;
    for k in &booms.kicks {
        let strength = left(k) * (k.age / 0.01).min(1.);
        if strength <= 0. {
            continue;
        }
        let (t, s) = (k.age, k.seed);
        let wobble = Vec3::new(
            0.7 * (t * TAU * 17. + s).sin() + 0.3 * (t * TAU * 29. + 2. * s).sin(),
            0.7 * (t * TAU * 13. + 3. * s).sin() + 0.3 * (t * TAU * 23. + s).sin(),
            0.6 * (t * TAU * 11. + 5. * s).sin(),
        );
        // A jolt down at first, then the wobble.
        sum += (wobble + Vec3::new(-0.8 * (-t / 0.04).exp(), 0., 0.)) * strength;
    }
    sum.clamp_length_max(SHAKE_MOST)
}
/// Spatters the lens with `count` blobs, bigger the `near`er, more of them on the side of the screen the burst is on
/// (`seen`: where it is from the camera, in the camera's frame). There are never more than `most` (the plan's).
#[allow(clippy::too_many_arguments)]
fn spatter(world: &mut World, booms: &mut Booms, kit: &Kit, most: usize, count: usize, near: f32, seen: Vec3, seed: u32) {
    let n = |k: u32| hash(seed.wrapping_mul(2654).wrapping_add(k));
    let lens = &mut booms.lens;
    let root = match lens.root.filter(|e| world.get_entity(*e).is_ok()) {
        Some(e) => e,
        None => {
            lens.smudges.clear();
            let e = world
                .spawn((
                    Name::new("Relish on the lens"),
                    Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(0.), width: Val::Percent(100.), height: Val::Percent(100.), ..default() },
                    Visibility::Hidden,
                    // Over the scope's mask and the name tags (they are behind the lens); under the head-up display.
                    ZIndex(0),
                ))
                .id();
            lens.root = Some(e);
            e
        }
    };
    show(world, root, true);
    // Which side of the screen the burst is on, and how far up or down, -1 to 1 (0 straight ahead or behind).
    let way = seen / seen.length().max(1e-3);
    let (side, rise) = if seen.z < 0. { (way.x, way.y) } else { (0., way.y.min(0.)) };
    let mut placed = 0;
    for k in 0..count as u32 {
        // A free blob, or the oldest.
        let i = match lens.smudges.iter().position(|s| !s.on) {
            Some(i) => i,
            None if lens.smudges.len() < most.max(count) => {
                let e = world.spawn((ImageNode::new(kit.lens[lens.smudges.len() % 3].clone()), Node { position_type: PositionType::Absolute, ..default() }, UiTransform::default(), Visibility::Hidden)).id();
                world.entity_mut(root).add_child(e);
                lens.smudges.push(Smudge { entity: e, on: false, age: 0., hold: 0., life: 0., speed: 0., fall: 0., alpha: 0., sag: 0. });
                lens.smudges.len() - 1
            }
            None => (0..lens.smudges.len()).max_by(|a, b| lens.smudges[*a].age.total_cmp(&lens.smudges[*b].age)).unwrap_or(0),
        };
        let size = (11. + 13. * near) * (0.7 + 0.6 * n(k * 7));
        // Anywhere but over the crosshair, leaning to the burst's side.
        let mut at = Vec2::new(4. + 86. * n(k * 7 + 1), 3. + 70. * n(k * 7 + 2));
        at.x = (at.x + side * 18. * n(k * 7 + 3)).clamp(2., 92.);
        at.y = (at.y - rise * 24. * n(k * 7 + 11)).clamp(2., 82.);
        let middle = Vec2::new(50., 50.) - Vec2::new(size * 0.28, size * 0.5);
        let off = at - middle;
        if off.length() < 16. {
            at = middle + off.normalize_or(Vec2::X) * 16.;
        }
        let smudge = &mut lens.smudges[i];
        *smudge = Smudge {
            entity: smudge.entity,
            on: true,
            age: -0.012 * placed as f32,
            hold: 0.25 + 0.4 * n(k * 7 + 4),
            life: 1.9 + 0.8 * n(k * 7 + 5),
            speed: 2. + 5. * n(k * 7 + 6),
            fall: 10. + 16. * n(k * 7 + 7),
            alpha: 0.85 + 0.15 * near,
            sag: 0.35 + 0.4 * n(k * 7 + 8),
        };
        placed += 1;
        let e = smudge.entity;
        if let Some(mut node) = world.get_mut::<Node>(e) {
            (node.left, node.top, node.width, node.height) = (Val::Percent(at.x), Val::Percent(at.y), Val::Vh(size), Val::Vh(size));
        }
        if let Some(mut image) = world.get_mut::<ImageNode>(e) {
            image.color = Color::srgba(1., 1., 1., 0.);
            image.flip_x = n(k * 7 + 9) < 0.5;
        }
        if let Some(mut t) = world.get_mut::<UiTransform>(e) {
            *t = UiTransform { rotation: Rot2::radians((n(k * 7 + 10) - 0.5) * 0.6), ..UiTransform::IDENTITY };
        }
        show(world, e, true);
    }
}

/// Moves every burst under way, the lens's blobs, the shake and the lights; puts away what has finished. Does nothing
/// when nothing is under way.
pub(super) fn update(world: &mut World, mess: &mut Mess, look: &Look, g: &Game, now: f32, dt: f32) {
    if !world.get_resource::<Booms>().is_some_and(|b| b.busy()) {
        return;
    }
    let light = Plan::of(level_of(world)).light;
    world.resource_scope(|world, mut booms: Mut<Booms>| {
        let Some(kit) = booms.kit.clone() else {
            return;
        };
        // (A long frame is moved in one step no longer than this, so nothing flies through a wall.)
        let step = dt.min(0.05);
        for slot in booms.slots.iter_mut().filter(|s| s.live) {
            run(world, slot, mess, look, &kit, &g.feast, dt, step, now);
        }
        for k in &mut booms.kicks {
            k.age += dt;
            if k.age >= SHAKE_LONG {
                k.amp = 0.;
            }
        }
        lens(world, &mut booms.lens, dt);
        // The lights: a quick rise and a fall over LIGHT_LIFE; gone below High.
        if !light {
            for l in booms.lamps.drain(..) {
                if let Ok(e) = world.get_entity_mut(l.entity) {
                    e.despawn();
                }
            }
        }
        for l in &mut booms.lamps {
            if l.age >= LIGHT_LIFE {
                continue;
            }
            l.age += dt;
            let t = l.age / LIGHT_LIFE;
            if let Some(mut light) = world.get_mut::<PointLight>(l.entity) {
                light.intensity = if t >= 1. { 0. } else { LIGHT * (l.age / 0.015).min(1.) * (1. - t).powi(2) };
            }
            if t >= 1. {
                show(world, l.entity, false);
            }
        }
    });
}
/// One frame of a burst: `dt` for its clock, `step` (no longer than a twentieth of a second) for what flies.
#[allow(clippy::too_many_arguments)]
fn run(world: &mut World, slot: &mut Slot, mess: &mut Mess, look: &Look, kit: &Kit, feast: &Feast, dt: f32, step: f32, now: f32) {
    slot.age += dt;
    let (age, r) = (slot.age, slot.radius);
    if age >= SPAN {
        slot.retire(world);
        return;
    }
    // The ring: out to the blast's edge, held there, fading.
    if let (Some(e), Some((at, reach))) = (slot.plate, slot.ring) {
        if age >= RING_GONE {
            show(world, e, false);
        } else {
            let out = 1. - (1. - (age / RING_OUT).min(1.)).powi(3);
            let fade = ((age - RING_HOLD) / (RING_GONE - RING_HOLD)).clamp(0., 0.999);
            dress(world, e, &kit.plate, &kit.ring[(fade * 4.) as usize]);
            let wide = 2. * reach * out.max(0.02);
            place(world, e, Transform::from_translation(at + Vec3::Y * 0.0025).with_rotation(Quat::from_rotation_x(-PI / 2.)).with_scale(Vec3::new(wide, wide, 1.)));
        }
    }
    for f in slot.flashes.iter_mut().filter(|f| f.on) {
        let t = age / f.life;
        if t >= 1. {
            f.on = false;
            hide_card(world, f.entity);
            continue;
        }
        let grow = smooth(t / 0.2) * (1. - smooth((t - 0.3) / 0.7));
        if let Some(mut tr) = world.get_mut::<Transform>(f.entity) {
            tr.scale = Vec3::splat(f.peak * grow);
        }
    }
    for p in slot.mist.iter_mut().filter(|p| p.on) {
        let t = (age - p.delay) / p.life;
        if t < 0. {
            continue;
        }
        if t >= 1. {
            p.on = false;
            hide_card(world, p.entity);
            continue;
        }
        p.at += p.velocity * step;
        p.velocity *= (-3.5 * step).exp();
        p.velocity.y += 0.04 * step;
        let size = p.from + (p.to - p.from) * (1. - (1. - t).powi(2));
        dress(world, p.entity, &look.card[SMOKE], &kit.mist[p.dark as usize][((t * 3.) as usize).min(2)]);
        if let Some(mut tr) = world.get_mut::<Transform>(p.entity) {
            (tr.translation, tr.scale) = (p.at, Vec3::splat(size));
        }
        show(world, p.entity, true);
    }
    // The ball swells for a moment and is gone into the globs.
    for b in slot.core.iter_mut().filter(|b| b.on) {
        let size = if age < 0.05 { r * (0.25 + 0.35 * smooth(age / 0.05)) } else { r * 0.6 * (1. - (age - 0.05) / 0.07) };
        if size <= 0. {
            b.on = false;
            show(world, b.entity, false);
            continue;
        }
        let wobble = Vec3::new(1. + 0.12 * (age * 90.).sin(), 1. - 0.1 * (age * 90.).sin(), 1. + 0.08 * (age * 70.).cos());
        place(world, b.entity, Transform::from_translation(b.at).with_scale(wobble * size));
    }
    let level = &feast.level;
    let ground = |x: f32, y: f32, z: f32| under(level, x, y, z);
    let inside = |at: Vec3| at.x as f64 >= level.x0 && at.x as f64 <= level.x1 && at.z as f64 >= level.z0 && at.z as f64 <= level.z1;
    // Globs: fly, drawn out along the way they go; where one lands it leaves a splat and is gone.
    let mut k = 0;
    while k < slot.globs.len() {
        let b = &mut slot.globs[k];
        k += 1;
        if !b.on || !b.flying {
            continue;
        }
        let from = b.at;
        b.velocity.y -= DROP * step;
        b.at += b.velocity * step;
        if let Some((at, surface)) = landing(feast, from, b.at, &ground, inside(b.at)) {
            b.on = false;
            b.flying = false;
            show(world, b.entity, false);
            if slot.landing > 0 {
                slot.landing -= 1;
                let seed = slot.seed.wrapping_mul(31).wrapping_add(k as u32 * 977);
                lay_landing(world, mess, look, feast, at, surface, b.velocity, b.size.x, r, seed, now);
            }
            continue;
        }
        if b.at.y < feast::FLOOR - 1. {
            b.on = false;
            show(world, b.entity, false);
            continue;
        }
        let speed = b.velocity.length();
        let stretch = (1. + speed * 0.35).min(2.);
        let turn = Quat::from_rotation_arc(Vec3::Y, b.velocity.normalize_or(Vec3::Y));
        // A big one is swollen at first (packed together they are the ball) and shrinks as it flies apart; a small one
        // comes out of the ball, small at first.
        let grow = if b.big { 1. + 0.9 * (1. - smooth(age / 0.16)) } else { smooth(age / 0.05).max(0.3) };
        place(world, b.entity, Transform::from_translation(b.at).with_rotation(turn).with_scale(Vec3::new(1. / stretch.sqrt(), stretch, 1. / stretch.sqrt()) * b.size.x * grow));
    }
    // Chunks: tumble, and lie where they land, shrinking away at the end.
    for b in slot.chunks.iter_mut().filter(|b| b.on) {
        let fade = 1. - smooth((age - (SPAN - 0.6)) / 0.6);
        if b.flying {
            let from = b.at;
            b.velocity.y -= DROP * step;
            b.at += b.velocity * step;
            if let Some((at, surface)) = landing(feast, from, b.at, &ground, inside(b.at)) {
                b.flying = false;
                let normal = match surface {
                    Surface::Ground { normal, .. } | Surface::Wall { normal, .. } => normal,
                    _ => Vec3::Y,
                };
                // A chunk that hits a wall drops to the foot of it.
                b.at = if normal.y > 0.5 { at + normal * b.size.y * 0.35 } else { let foot = at + normal * b.size.x; Vec3::new(foot.x, ground(foot.x, foot.y, foot.z) + b.size.y * 0.35, foot.z) };
                place(world, b.entity, Transform::from_translation(b.at).with_rotation(Quat::from_rotation_y(b.spin.y)).with_scale(b.size * fade));
                continue;
            }
            if b.at.y < feast::FLOOR - 1. {
                b.on = false;
                show(world, b.entity, false);
                continue;
            }
            place(world, b.entity, Transform::from_translation(b.at).with_rotation(Quat::from_scaled_axis(b.spin * age)).with_scale(b.size * smooth(age / 0.04).max(0.3)));
        } else if fade < 1. {
            if let Some(mut tr) = world.get_mut::<Transform>(b.entity) {
                tr.scale = b.size * fade;
            }
        }
    }
    // Drops: fly and are gone where they land.
    for b in slot.drops.iter_mut().filter(|b| b.on && b.flying) {
        let from = b.at;
        b.velocity.y -= DROP * step;
        b.at += b.velocity * step;
        if landing(feast, from, b.at, &ground, inside(b.at)).is_some() || b.at.y < feast::FLOOR - 1. {
            b.on = false;
            b.flying = false;
            show(world, b.entity, false);
            continue;
        }
        let speed = b.velocity.length();
        let stretch = (1. + speed * 1.2).min(4.);
        let turn = Quat::from_rotation_arc(Vec3::Y, b.velocity.normalize_or(Vec3::Y));
        place(world, b.entity, Transform::from_translation(b.at).with_rotation(turn).with_scale(Vec3::new(1. / stretch.sqrt(), stretch, 1. / stretch.sqrt()) * b.size.x));
    }
}
/// Whether something flying from `from` to `to` met the ground or a block on the way (only over the counter, `over`):
/// where, and on what.
fn landing(feast: &Feast, from: Vec3, to: Vec3, ground: &dyn Fn(f32, f32, f32) -> f32, over: bool) -> Option<(Vec3, Surface)> {
    let way = to - from;
    let long = way.length();
    if long < 1e-6 {
        return None;
    }
    let dir = way / long;
    let t = feast.level.cover_from(from, dir);
    if t >= 0. && t <= long {
        let at = from + dir * t;
        return Some((at, face_at(feast, at, dir)));
    }
    if over {
        let floor = ground(to.x, to.y, to.z);
        if to.y <= floor && way.y < 0. {
            let at = Vec3::new(to.x, floor, to.z);
            return Some((at, Surface::Ground { at, normal: slope(&feast.level, to.x, to.z) }));
        }
    }
    None
}
/// What a way along `dir` met at `at`, on a block's face: the ground on its top, a wall on a side (only a face the way
/// came into). Anything else (a sink's side or floor) is asked of `classify`.
fn face_at(feast: &Feast, at: Vec3, dir: Vec3) -> Surface {
    const E: f32 = 0.003;
    for block in &feast.level.map.blocks {
        let [x0, z0, x1, z1] = block.rect.map(|v| v as f32);
        let top = feast::FLOOR + block.height as f32;
        if at.x < x0 - E || at.x > x1 + E || at.z < z0 - E || at.z > z1 + E || at.y > top + E || at.y < feast::FLOOR - E {
            continue;
        }
        let faces = [((at.x - x0).abs(), Vec3::NEG_X), ((at.x - x1).abs(), Vec3::X), ((at.z - z0).abs(), Vec3::NEG_Z), ((at.z - z1).abs(), Vec3::Z), ((at.y - top).abs(), Vec3::Y)];
        let normal = faces.into_iter().filter(|(_, n)| n.dot(dir) < 0.).min_by(|a, b| a.0.total_cmp(&b.0)).map_or(-dir, |(_, n)| n);
        return if normal.y > 0.5 { Surface::Ground { at, normal } } else { Surface::Wall { at, normal } };
    }
    match classify(feast, at, Some(dir), 0.02, &[]) {
        s @ (Surface::Ground { .. } | Surface::Wall { .. }) => s,
        _ => Surface::Wall { at, normal: -dir },
    }
}
/// The small splat a glob of `size` leaves where it lands, flying at `velocity`.
#[allow(clippy::too_many_arguments)]
fn lay_landing(world: &mut World, mess: &mut Mess, look: &Look, feast: &Feast, at: Vec3, surface: Surface, velocity: Vec3, size: f32, radius: f32, seed: u32, now: f32) {
    let (at, normal) = match surface {
        Surface::Ground { at, normal } | Surface::Wall { at, normal } => (at, normal),
        _ => (at, Vec3::Y),
    };
    let wide = (size * 4.2 + radius * 0.06) * surface_scale(normal) / FLOOR_SCALE * (0.85 + 0.3 * hash(seed));
    let cell = if hash(seed + 1) < 0.5 { SPATTER } else { SPLATS[(seed % 4) as usize] };
    let level = &feast.level;
    let ground = |x: f32, z: f32| under(level, x, at.y, z);
    let upright = normal.y.abs() < 0.5;
    let at = if upright { on_the_face(level, Vec3::new(at.x, at.y.max(ground(at.x + normal.x * 0.01, at.z + normal.z * 0.01) + wide * 0.32), at.z), normal, wide) } else { at };
    let way = if upright { None } else { velocity.try_normalize() };
    let parts = mark(world, mess, look, Sauce::Relish, at, normal, way, wide, cell, None, seed, upright.then_some(&ground as &dyn Fn(f32, f32) -> f32));
    keep(world, mess, Sauce::Relish, parts, now);
}
/// The first surface on the way from `from` along the unit `dir` within `reach`, and how far: the ground (the
/// counter under a deck or a ramp, which the way passes under), a block's face, or one of the loose objects `near`.
fn trace(feast: &Feast, from: Vec3, dir: Vec3, reach: f32, near: &[usize]) -> Option<(f32, Surface)> {
    let level = &feast.level;
    let mut best: Option<(f32, Surface)> = None;
    let t = level.cover_from(from, dir);
    if t >= 0. && t <= reach {
        best = Some((t, face_at(feast, from + dir * t, dir)));
    }
    let limit = best.map_or(reach, |(t, _)| t);
    const STEPS: usize = 16;
    let mut last = 0.;
    for s in 1..=STEPS {
        let t = limit * s as f32 / STEPS as f32;
        let q = from + dir * t;
        if q.y < under(level, q.x, q.y, q.z) {
            let (mut lo, mut hi) = (last, t);
            for _ in 0..10 {
                let mid = (lo + hi) / 2.;
                let m = from + dir * mid;
                if m.y < under(level, m.x, m.y, m.z) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let q = from + dir * hi;
            let at = Vec3::new(q.x, under(level, q.x, q.y + 0.002, q.z), q.z);
            best = Some((hi, Surface::Ground { at, normal: slope(level, q.x, q.z) }));
            break;
        }
        last = t;
    }
    if !near.is_empty() {
        let limit = best.map_or(reach, |(t, _)| t);
        if let Some(hit) = feast.props.raycast_only(from.as_dvec3().to_array(), dir.as_dvec3().to_array(), limit as f64, near) {
            let at = Vec3::new(hit.point[0] as f32, hit.point[1] as f32, hit.point[2] as f32);
            let normal = Vec3::new(hit.normal[0] as f32, hit.normal[1] as f32, hit.normal[2] as f32).normalize_or(-dir);
            best = Some((hit.distance as f32, Surface::Prop { id: hit.id, at, normal }));
        }
    }
    best
}

/// Throws the burst's big splats round it (see the module's notes), smears the hotdogs in the blast and marks the
/// loose objects there. Returns the splats thrown (not the one where it burst): where each met a surface, which way
/// that faces, and how wide.
#[allow(clippy::too_many_arguments)]
fn throw_splats(world: &mut World, mess: &mut Mess, look: &Look, g: &Game, b: &Ignition, plan: &Plan, surface: Surface, dir: Option<Vec3>, now: f32) -> Vec<[f32; 7]> {
    let feast = &g.feast;
    let level = &feast.level;
    let (p, r) = (b.at, b.radius);
    let n = |k: u32| hash(b.seed.wrapping_mul(389).wrapping_add(k));
    let ground = |x: f32, z: f32| under(level, x, p.y, z);
    let drips = Some(&ground as &dyn Fn(f32, f32) -> f32);
    let centre = p + b.normal * 0.02;
    // The loose objects near enough to be in the way or to be hit, nearest first, and the hotdogs as drawn.
    let mut near: Vec<(f32, usize)> = (0..feast.props.len())
        .filter(|id| {
            use kitchen_sim::props::State;
            matches!(feast.props.body(*id).state, State::Awake | State::Asleep | State::Locked)
        })
        .map(|id| {
            let q = feast.props.position(id);
            (Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32).distance(p), id)
        })
        .filter(|(d, _)| *d < r + 0.2)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let ids: Vec<usize> = near.iter().map(|(_, id)| *id).collect();
    let hosts: HashMap<usize, (Entity, Transform)> = if ids.is_empty() {
        HashMap::new()
    } else {
        world.query::<(Entity, &crate::loose::Loose, &Transform)>().iter(world).filter(|(_, l, _)| ids.contains(&l.id)).map(|(e, l, tr)| (l.id, (e, *tr))).collect()
    };
    let napkin = |id: usize| feast.props.body(id).shape == kitchen_sim::drape::NAPKIN;
    let mut parts = vec![];
    let mut thrown = vec![];
    // Where it burst: a big chunky splat, or a smear on the hotdog it burst on.
    let mut direct = None;
    match surface {
        Surface::Ground { at, normal } => parts.extend(mark(world, mess, look, Sauce::Relish, at, normal, None, r * 1.3, CHUNKY, None, b.seed, None)),
        Surface::Wall { at, normal } => {
            let size = r * 1.1;
            let foot = under(level, at.x + normal.x * 0.01, at.y, at.z + normal.z * 0.01);
            let at = on_the_face(level, Vec3::new(at.x, at.y.max(foot + size * 0.25), at.z), normal, size);
            parts.extend(mark(world, mess, look, Sauce::Relish, at, normal, dir, size, CHUNKY, None, b.seed, drips));
        }
        Surface::Prop { id, at, normal } => {
            if let Some(host) = hosts.get(&id).copied().filter(|_| !napkin(id)) {
                let marks = mark(world, mess, look, Sauce::Relish, at, normal, dir, r * 0.6, CHUNKY, Some(host), b.seed, None);
                keep(world, mess, Sauce::Relish, marks, now);
            }
        }
        Surface::Hotdog { id } => direct = Some(id),
        Surface::Air => {}
    }
    let count = splat_count(plan, r);
    // One splat thrown along `way` from the centre, if it meets something within the blast.
    let throw = |world: &mut World, mess: &mut Mess, way: Vec3, reach: f32, k: u32, parts: &mut Vec<Part>, thrown: &mut Vec<[f32; 7]>| -> bool {
        let Some(way) = way.try_normalize() else { return false };
        let Some((t, hit)) = trace(feast, centre, way, reach, &ids) else { return false };
        let (at, normal, host) = match hit {
            Surface::Ground { at, normal } | Surface::Wall { at, normal } => (at, normal, None),
            Surface::Prop { id, at, normal } => match hosts.get(&id).copied().filter(|_| !napkin(id)) {
                Some(host) => (at, normal, Some(host)),
                None => return false,
            },
            _ => return false,
        };
        // (Within the blast, and on the counter: not in the air beyond its edge.)
        let over = (level.x0..=level.x1).contains(&(at.x as f64)) && (level.z0..=level.z1).contains(&(at.z as f64));
        if at.distance(p) > r * 1.02 || t <= 0. || !over {
            return false;
        }
        let d = at.distance(p) / r;
        let upright = normal.y.abs() < 0.5;
        let size = r * (0.6 + 0.35 * n(400 + k)) * (1.25 - 0.5 * d) * if upright { 1.25 } else { 1. };
        let cell = match k % 6 {
            0 if !upright => SPATTER,
            3 => CHUNKY,
            _ if upright => [CHUNKY, SPLATS[1], SPLATS[3]][(k % 3) as usize],
            _ => SPLATS[(k % 4) as usize],
        };
        // Drawn out away from the burst: its body a little beyond where the way met the surface.
        let along = (way - normal * way.dot(normal)).normalize_or_zero();
        let mut spot = at + along * size * 0.3;
        if upright {
            // (Its body above the counter at the foot of the face, not half under it.)
            spot.y = spot.y.max(under(level, spot.x + normal.x * 0.01, spot.y, spot.z + normal.z * 0.01) + size * 0.32);
            spot = on_the_face(level, spot, normal, size);
        }
        let seed = b.seed.wrapping_add(17 + k * 7);
        // (On an upright face it is not drawn out: its spikes would stand up the face like grass.)
        let marks = mark(world, mess, look, Sauce::Relish, spot, normal, if upright { None } else { Some(way) }, size, cell, host, seed, if upright && host.is_none() { drips } else { None });
        if host.is_some() {
            keep(world, mess, Sauce::Relish, marks, now);
        } else {
            parts.extend(marks);
        }
        thrown.push([at.x, at.y, at.z, normal.x, normal.y, normal.z, size]);
        true
    };
    let mut k = 0u32;
    // Every block face within the blast takes one, nearest first (but the face it burst on, which has its splat).
    let mut faces: Vec<(f32, Vec3, Vec3)> = vec![];
    for block in &level.map.blocks {
        let [x0, z0, x1, z1] = block.rect.map(|v| v as f32);
        let top = feast::FLOOR + block.height as f32;
        let q = Vec3::new(p.x.clamp(x0, x1), p.y.clamp(feast::FLOOR, top), p.z.clamp(z0, z1));
        let d = q.distance(p);
        if d >= r * 0.97 || d < 0.01 {
            continue;
        }
        let gaps = [(x0 - p.x, Vec3::NEG_X), (p.x - x1, Vec3::X), (z0 - p.z, Vec3::NEG_Z), (p.z - z1, Vec3::Z), (p.y - top, Vec3::Y)];
        let normal = gaps.into_iter().max_by(|a, b| a.0.total_cmp(&b.0)).map_or(Vec3::Y, |(_, n)| n);
        faces.push((d, q, normal));
    }
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (d, q, normal) in faces.into_iter().take(count / 2) {
        // A point on the face a little off the nearest, within the blast's circle on it.
        let (t1, t2) = basis(normal);
        let a = n(500 + k) * TAU;
        let spread = (r * r - d * d).max(0.).sqrt() * (0.15 + 0.35 * n(510 + k));
        let target = q + (t1 * a.cos() + t2 * a.sin()) * spread;
        if throw(world, mess, target - centre, r * 1.1, k, &mut parts, &mut thrown) {
            k += 1;
        }
    }
    // On the wall it burst on, round where it burst.
    if let Surface::Wall { at, normal } = surface {
        let side = normal.cross(Vec3::Y).normalize_or(Vec3::X);
        let want = count.saturating_sub(k as usize).div_ceil(2);
        for j in 0..want as u32 {
            let a = (j as f32 + n(700 + j)) / want.max(1) as f32 * TAU;
            let q = at + (side * a.cos() + Vec3::Y * a.sin()) * r * (0.3 + 0.55 * n(710 + j));
            if throw(world, mess, q - centre, r * 1.1, k, &mut parts, &mut thrown) {
                k += 1;
            }
        }
    }
    // Up and out round it, at whatever is near enough to be met: a quarter of what is left at most.
    let (t1, t2) = basis(b.normal);
    let most = k as usize + count.saturating_sub(k as usize) / 4;
    for j in 0..count as u32 {
        if k as usize >= most {
            break;
        }
        let a = j as f32 * 2.399_963 + n(800) * TAU;
        let rise = 0.35 + 0.5 * n(810 + j);
        let way = (t1 * a.cos() + t2 * a.sin()) * (1. - rise * rise).sqrt() + b.normal * rise;
        if throw(world, mess, way, r, k, &mut parts, &mut thrown) {
            k += 1;
        }
    }
    // The rest over the ground round it, out to where the blast meets the ground, evenly round (twice round at most,
    // for those that met nothing).
    if let Some((foot, reach)) = b.ring {
        let want = count.saturating_sub(k as usize);
        let turn = n(600) * TAU;
        for j in 0..(want * 2) as u32 {
            if k as usize >= count {
                break;
            }
            let a = turn + (j as f32 + 0.5 + (n(610 + j) - 0.5) * 0.7) / want.max(1) as f32 * TAU;
            // (Near and far in turn, by the golden ratio, so that a few of them spread across it as well as many.)
            let far = reach * (0.3 + 0.64 * (j as f32 * 0.618_034 + n(620)).fract());
            let target = Vec3::new(foot.x + a.cos() * far, 0., foot.z + a.sin() * far);
            let target = Vec3::new(target.x, under(level, target.x, foot.y + 0.01, target.z), target.z);
            let way = target - centre;
            if throw(world, mess, way, way.length() * 1.15 + 0.01, k, &mut parts, &mut thrown) {
                k += 1;
            }
        }
    }
    keep(world, mess, Sauce::Relish, parts, now);
    // The hotdogs in the blast, nearest first (the one it burst on has its smear: `land`), from the side facing it.
    let bodies = hotdogs(world);
    let mut struck: Vec<(f32, usize)> = bodies
        .iter()
        .enumerate()
        .filter(|(_, body)| Some(body.id) != direct)
        .map(|(i, body)| (body.frame.transform_point(MIDDLE).distance(p), i))
        .filter(|(d, _)| *d < r + 0.05)
        .collect();
    struck.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let Some(body) = direct.and_then(|id| bodies.iter().find(|b| b.id == id)) {
        smear(world, mess, look, Sauce::Relish, body, p, dir, b.seed);
    }
    for (j, (d, i)) in struck.into_iter().take(plan.smears).enumerate() {
        let body = &bodies[i];
        let middle = body.frame.transform_point(MIDDLE);
        let way = (middle - centre).normalize_or(Vec3::X);
        if feast.cover(centre, way) < d - 0.07 {
            continue;
        }
        smear(world, mess, look, Sauce::Relish, body, middle - way * 0.06, Some(way), b.seed.wrapping_add(900 + j as u32));
    }
    // The loose objects in the blast, from the side facing it (besides any the thrown splats met).
    let mut marked = 0;
    for (_, id) in near.iter().copied() {
        if marked >= plan.props {
            break;
        }
        let Some(host) = hosts.get(&id).copied().filter(|_| !napkin(id)) else { continue };
        if matches!(surface, Surface::Prop { id: on, .. } if on == id) {
            continue;
        }
        let q = feast.props.position(id);
        let way = (Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32) - centre).normalize_or(Vec3::Y);
        let Some(hit) = feast.props.raycast_only(centre.as_dvec3().to_array(), way.as_dvec3().to_array(), r as f64, &[id]) else { continue };
        let at = Vec3::new(hit.point[0] as f32, hit.point[1] as f32, hit.point[2] as f32);
        let normal = Vec3::new(hit.normal[0] as f32, hit.normal[1] as f32, hit.normal[2] as f32).normalize_or(-way);
        let size = r * (0.35 + 0.2 * n(950 + id as u32)) * (1.1 - 0.5 * hit.distance as f32 / r);
        let marks = mark(world, mess, look, Sauce::Relish, at, normal, Some(way), size, SPLATS[id % 4], Some(host), b.seed.wrapping_add(1000 + id as u32), None);
        keep(world, mess, Sauce::Relish, marks, now);
        marked += 1;
    }
    thrown
}

/// The lens's blobs: each pops on, clings a moment, sags and slides down, and fades.
fn lens(world: &mut World, lens: &mut Lens, dt: f32) {
    let mut any = false;
    for s in lens.smudges.iter_mut().filter(|s| s.on) {
        s.age += dt;
        if s.age >= s.life {
            s.on = false;
            show(world, s.entity, false);
            continue;
        }
        any = true;
        if s.age < 0. {
            continue;
        }
        let pop = 0.55 + 0.45 * smooth(s.age / 0.06) + 0.08 * (s.age / 0.05).min(1.) * (-(s.age - 0.05).max(0.) / 0.08).exp();
        let t = (s.age - s.hold).max(0.);
        let slide = s.speed * t + 0.5 * s.fall * t * t;
        let sag = 1. + s.sag * smooth(t / 1.2);
        let alpha = s.alpha * smooth(s.age / 0.03) * (1. - smooth((s.age - s.life * 0.55) / (s.life * 0.45)));
        if let Some(mut tr) = world.get_mut::<UiTransform>(s.entity) {
            tr.translation = Val2::new(Val::Px(0.), Val::Vh(slide));
            tr.scale = Vec2::new(pop / sag.sqrt(), pop * sag);
        }
        if let Some(mut image) = world.get_mut::<ImageNode>(s.entity) {
            image.color = Color::srgba(1., 1., 1., alpha);
        }
    }
    if !any {
        if let Some(root) = lens.root {
            show(world, root, false);
        }
    }
}

/// A new round, a new map: every burst put away, the lens wiped and the shake over.
pub(super) fn clear(world: &mut World) {
    if !world.contains_resource::<Booms>() {
        return;
    }
    world.resource_scope(|world, mut booms: Mut<Booms>| {
        for slot in &mut booms.slots {
            slot.retire(world);
        }
        for s in &mut booms.lens.smudges {
            s.on = false;
            show(world, s.entity, false);
        }
        if let Some(root) = booms.lens.root {
            show(world, root, false);
        }
        booms.kicks = [Kick::default(); KICKS];
        for l in &mut booms.lamps {
            l.age = LIGHT_LIFE;
            show(world, l.entity, false);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::splatter as fx;

    /// An open spot on the bundled map's counter: nothing within 0.38 m of it (the nearest block is 0.4 m off).
    const OPEN: Vec3 = Vec3::new(0., feast::FLOOR, 3.25);
    /// Before the steel block's face (x -0.45 to -0.23, z 2.7, facing -z).
    const BLOCK: Vec3 = Vec3::new(-0.34, feast::FLOOR, 2.6);

    /// The game as `main` builds it, single player in the sandbox with nobody else about, the effects made, drawn at
    /// `level`; the camera far off (at the origin, a metre under the counter and three behind it).
    fn game(level: Level) -> App {
        let mut app = crate::arena::test_app("sandbox bots=0 god");
        let world = app.world_mut();
        world.insert_resource(crate::quality::Detail::of(level, crate::visuals::Quality::Full));
        fx::prepare(world);
        app
    }
    fn down() -> Vec3 {
        Vec3::new(0.1, -0.5, 1.).normalize()
    }
    /// A round of `weapon` ending at `p`, flying along `dir`, as the arena lands one where the simulation says it ended.
    fn land(world: &mut World, weapon: usize, p: Vec3, dir: Vec3, seed: u32, now: f32) {
        world.resource_scope(|world, g: Mut<Game>| fx::land(world, &g, weapon, p, Some(dir), seed, now));
    }
    /// The effects run frame by frame for `seconds` from `now`; returns the time at the end.
    fn frames(world: &mut World, now: f32, seconds: f32, dt: f32) -> f32 {
        let mut t = now;
        for _ in 0..(seconds / dt).round() as usize {
            t += dt;
            world.resource_scope(|world, mut g: Mut<Game>| fx::update(world, &mut g, t, dt));
        }
        t
    }
    /// Puts the game's camera at `eye`, looking at `at`.
    fn camera(world: &mut World, eye: Vec3, at: Vec3) {
        let mut cameras = world.query_filtered::<&mut Transform, With<crate::maya::HotdogCamera>>();
        for mut t in cameras.iter_mut(world) {
            *t = Transform::from_translation(eye).looking_at(at, Vec3::Y);
        }
    }
    fn booms(world: &World) -> &Booms {
        world.resource::<Booms>()
    }
    /// Every entity of a burst's that is drawn now.
    fn drawn(world: &World) -> usize {
        booms(world).slots.iter().flat_map(|s| s.entities()).filter(|e| world.get::<Visibility>(*e).is_some_and(|v| *v != Visibility::Hidden)).count()
    }

    #[test]
    fn a_relish_glob_sets_the_burst_off_and_no_other_guns_round_does() {
        let mut app = game(Level::High);
        let world = app.world_mut();
        for weapon in (0..feast::GUNS).filter(|w| *w != BAZOOKA) {
            land(world, weapon, OPEN, down(), 10 + weapon as u32, 1.);
        }
        assert_eq!((booms(world).started, booms(world).live(), drawn(world)), (0, 0, 0), "no other gun's round bursts");
        assert!(!booms(world).busy());
        assert!(booms(world).kicks.iter().all(|k| k.amp == 0.) && shake(world) == Vec3::ZERO, "nor shakes the camera");
        land(world, BAZOOKA, OPEN, down(), 99, 1.);
        assert_eq!((booms(world).started, booms(world).live()), (1, 1));
        assert!(drawn(world) > 40, "a big burst: {}", drawn(world));
        // A mustard round landing just after does not set off another, nor touch the one under way.
        land(world, 1, OPEN + Vec3::X * 0.05, down(), 100, 1.01);
        assert_eq!((booms(world).started, booms(world).live()), (1, 1));
    }

    /// Through the arena, as it is played: the rounds are the simulation's, fired by this player and by a bot; the
    /// arena lands each splat the simulation reports, and a glob's alone bursts.
    #[test]
    fn the_burst_comes_from_the_simulations_splats_whoever_fired_the_glob() {
        let mut app = crate::arena::test_app("sandbox weapons=relish,mustard ammo=inf bots=2 botmode=idle god");
        app.world_mut().insert_resource(crate::quality::Detail::of(Level::Medium, crate::visuals::Quality::Full));
        app.add_systems(Update, crate::arena::tick);
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 1. / 60.);
        }
        let started = |app: &App| app.world().get_resource::<Booms>().map_or(0, |b| b.started);
        let splats = |app: &App, weapon: usize| app.world().resource::<Game>().feast.splats.iter().filter(|s| s.weapon == weapon).count();
        // Fires `weapon` from hotdog `id` at the counter just ahead of it, and runs the game until the round lands.
        let fire = |app: &mut App, id: u32, weapon: usize| {
            let mut g = app.world_mut().resource_mut::<Game>();
            let p = g.feast.player_mut(id).unwrap();
            (p.owned[weapon], p.ammo[weapon], p.cooldown, p.reload, p.bot) = (true, 5, 0., None, false);
            assert!(g.feast.fire(id, Vec3::new(0.2, -0.6, 0.75).normalize(), weapon), "hotdog {id} fires gun {weapon}");
            let before = splats(app, weapon);
            for _ in 0..60 {
                crate::arena::test_frame(app, 1. / 60.);
                if splats(app, weapon) > before {
                    // (The arena lands it in the frame the simulation reports it.)
                    return;
                }
            }
            panic!("gun {weapon}'s round never landed");
        };
        fire(&mut app, 1, 1);
        assert_eq!(started(&app), 0, "a mustard round is not a burst");
        fire(&mut app, 2, BAZOOKA);
        assert_eq!(started(&app), 1, "a bot's glob bursts");
        fire(&mut app, 1, BAZOOKA);
        assert_eq!(started(&app), 2, "and this player's");
        fire(&mut app, 3, 1);
        assert_eq!(started(&app), 2);
    }

    #[test]
    fn the_splats_and_the_ring_follow_the_simulations_blast_radius() {
        // The blast's radius is the simulation's: the glob's splash as tuned, the constant unless it is.
        let mut app = game(Level::High);
        let world = app.world_mut();
        assert_eq!(radius(&world.resource::<Game>().feast), feast::SPLASH[BAZOOKA]);
        assert_eq!(radius(&Feast::with_map(kitchen_sim::map::Map::bundled(), false)), feast::SPLASH[BAZOOKA]);
        let plan = Plan::of(Level::High);
        let mut counts = vec![];
        let mut radii = vec![0.12f32, 0.17, 0.3, feast::SPLASH[BAZOOKA]];
        radii.sort_by(|a, b| a.total_cmp(b));
        radii.dedup();
        for (k, r) in radii.into_iter().enumerate() {
            fx::clear(world);
            world.resource_mut::<Game>().feast.tune.splash[BAZOOKA] = r;
            land(world, BAZOOKA, OPEN, down(), 40 + k as u32, 1.);
            let thrown = booms(world).last.clone();
            // In the open every one it throws lands on the counter round it, as many as its area calls for.
            assert_eq!(thrown.len(), splat_count(&plan, r), "radius {r}");
            let far: Vec<f32> = thrown.iter().map(|s| Vec2::new(s[0] - OPEN.x, s[2] - OPEN.z).length()).collect();
            assert!(far.iter().all(|d| *d <= r * 1.02), "radius {r}: within the blast {far:?}");
            assert!(far.iter().cloned().fold(0., f32::max) > r * 0.75 && far.iter().cloned().fold(1., f32::min) < r * 0.5, "radius {r}: spread across it {far:?}");
            assert!(thrown.iter().all(|s| (s[1] - feast::FLOOR).abs() < 1e-3 && s[4] > 0.99), "on the counter, facing up");
            // Drawn out away from the burst (`mark`'s glance), and bigger for a bigger blast.
            assert!(thrown.iter().all(|s| s[6] > r * 0.25 && s[6] < r * 1.3), "radius {r}: sizes {:?}", thrown.iter().map(|s| s[6]).collect::<Vec<_>>());
            // The ring runs out to the kill zone's edge on the counter, and every splat thrown is within it.
            let slot = booms(world).slots.iter().find(|s| s.live).unwrap();
            let (foot, reach) = slot.ring.unwrap();
            assert!((reach - kill_zone(r, 0.).unwrap()).abs() < 1e-4 && foot.distance(OPEN) < 1e-4, "radius {r}: ring {reach}");
            assert!(reach > r && far.iter().all(|d| *d < reach), "radius {r}: nothing marked beyond the ring");
            counts.push(thrown.len());
        }
        assert!(counts.windows(2).all(|w| w[1] >= w[0]) && counts.last() > counts.first(), "more splats for a bigger blast: {counts:?}");
        // Higher up (a glob bursting on a hotdog's chest), the ring is where a hotdog standing there is reached, and
        // too high for anybody standing to be reached, there is none.
        fx::clear(world);
        let r = feast::SPLASH[BAZOOKA];
        world.resource_mut::<Game>().feast.tune.splash[BAZOOKA] = r;
        land(world, BAZOOKA, OPEN + Vec3::Y * 0.1, -down(), 50, 2.);
        let reach = booms(world).slots.iter().find(|s| s.live).unwrap().ring.unwrap().1;
        assert!((reach - kill_zone(r, 0.1).unwrap()).abs() < 2e-3, "{reach}");
        fx::clear(world);
        land(world, BAZOOKA, OPEN + Vec3::Y * (kitchen_sim::actor::HEIGHT as f32 + r + 0.02), -down(), 51, 3.);
        assert!(booms(world).slots.iter().find(|s| s.live).unwrap().ring.is_none());
    }

    /// The ring is the simulation's kill zone: a hotdog standing just inside it is hurt by the burst, just outside it
    /// is not, however high over the counter the glob burst.
    #[test]
    fn the_ring_marks_where_the_simulation_hurts_a_hotdog_standing_there() {
        let r = feast::SPLASH[BAZOOKA];
        for height in [0.004f32, 0.03, 0.08, 0.15, 0.25] {
            let reach = kill_zone(r, height).unwrap();
            assert!(reach <= r + feast::HOTDOG_RADIUS as f32 + 1e-5, "{height}: {reach}");
            for (gap, hurt) in [(reach - 0.003, true), (reach + 0.003, false)] {
                let mut s = Feast::with_map(kitchen_sim::map::Map::bundled(), false);
                let p = s.players.iter_mut().find(|p| p.id == 2).unwrap();
                // (Facing the burst, looking level.)
                (p.p, p.alive, p.hp, p.yaw, p.pitch) = ([OPEN.x + gap, feast::FLOOR, OPEN.z], true, 100., std::f32::consts::FRAC_PI_2, 0.);
                let glob = feast::Projectile { owner: 1, weapon: BAZOOKA, hostile: true, ..Default::default() };
                s.burst(&glob, OPEN + Vec3::Y * height, None, None);
                let hp = s.player(2).unwrap().hp;
                assert_eq!(hp < 100., hurt, "burst {height} m up, hotdog {gap} m off (ring {reach}): {hp} HP");
            }
        }
        assert!(kill_zone(r, kitchen_sim::head::HEAD_AT + kitchen_sim::head::HEAD_RADIUS + r + 0.01).is_none(), "too high to reach anybody standing");
    }

    #[test]
    fn every_face_within_the_blast_is_splattered_and_runs_in_drips() {
        let mut app = game(Level::High);
        let world = app.world_mut();
        // On the counter 10 cm before the block's face: the face is splattered, the splats on it drip.
        land(world, BAZOOKA, BLOCK, down(), 7, 1.);
        let r = feast::SPLASH[BAZOOKA];
        let thrown = booms(world).last.clone();
        let on_face: Vec<&[f32; 7]> = thrown.iter().filter(|s| (s[2] - 2.7).abs() < 0.003 && s[5] < -0.99).collect();
        assert!(!on_face.is_empty(), "the face is hit: {thrown:?}");
        assert!(on_face.iter().all(|s| s[1] > feast::FLOOR && s[1] < feast::FLOOR + 0.25 && s[0] > -0.46 && s[0] < -0.22), "on the face: {on_face:?}");
        assert!(thrown.iter().all(|s| Vec3::new(s[0], s[1], s[2]).distance(BLOCK) <= r * 1.02), "all within the blast");
        let mess = world.resource::<Mess>();
        let drips = mess.stains.entries.iter().flat_map(|(_, _, s)| s.parts.iter()).filter(|p| matches!(p.kind, PartKind::Drip { .. })).count();
        assert!(drips > 0, "the splats on the face drip");
        // A block beyond the blast is left clean.
        fx::clear(world);
        land(world, BAZOOKA, BLOCK - Vec3::Z * (r + 0.05), down(), 8, 2.);
        assert!(booms(world).last.iter().all(|s| s[4] > 0.99), "only the counter: {:?}", booms(world).last);
    }

    #[test]
    fn every_level_holds_to_its_budget_and_its_cap_on_bursts() {
        let levels = Level::ALL.map(Plan::of);
        for w in levels.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert!(b.particles() <= a.particles() && b.globs <= a.globs && b.splats <= a.splats && b.lens <= a.lens && b.bursts <= a.bursts, "{b:?} after {a:?}");
        }
        let (high, low, lowest) = (Plan::of(Level::High), Plan::of(Level::Low), Plan::of(Level::Lowest));
        assert!(low.globs * 3 <= high.globs && lowest.globs < low.globs, "far fewer globs low down");
        assert!(high.light && !Plan::of(Level::Medium).light && !low.light && !lowest.light, "a light at High only");
        assert!(high.globs >= 36, "dozens of globs at High");
        for level in Level::ALL {
            let plan = Plan::of(level);
            let budget = Budget::of(level);
            let mut app = game(level);
            let world = app.world_mut();
            land(world, BAZOOKA, OPEN, down(), 3, 1.);
            frames(world, 1., 0.1, 1. / 60.);
            let slot = booms(world).slots.iter().find(|s| s.live).unwrap();
            let on = |bits: &Vec<Bit>| bits.iter().filter(|b| b.on).count();
            assert_eq!((slot.flashes.len(), slot.mist.len(), slot.globs.len(), slot.chunks.len(), slot.drops.len()), (plan.flashes, plan.mist, plan.globs, plan.chunks, plan.drops), "{level:?}");
            assert!(on(&slot.globs) <= plan.globs && slot.entities().count() == plan.particles(), "{level:?}");
            assert!(drawn(world) <= plan.particles(), "{level:?}: {} drawn", drawn(world));
            let lights = booms(world).lamps.len();
            assert_eq!(lights, plan.light as usize, "{level:?}: a light at High only");
            // Many at once: no more than the cap drawn, the oldest giving its place to the newest.
            for k in 0..10u32 {
                land(world, BAZOOKA, OPEN + Vec3::new(0.02 * k as f32, 0., 0.), down(), 20 + k, 1.2 + 0.01 * k as f32);
            }
            assert_eq!(booms(world).live(), plan.bursts, "{level:?}");
            assert!(drawn(world) <= plan.bursts * plan.particles(), "{level:?}");
            assert!(booms(world).entities() <= plan.bursts * plan.particles() + 2 + plan.lens + 1, "{level:?}: {} entities", booms(world).entities());
            let newest = booms(world).slots.iter().max_by_key(|s| s.order).unwrap().seed;
            assert_eq!(newest, 29, "the last one set off is drawn");
            // Their splats keep to the stains' cap on quads.
            frames(world, 1.4, 3., 1. / 30.);
            let mess = world.resource::<Mess>();
            assert!(mess.stains.weight() <= budget.quads && mess.stains.len() <= budget.stains, "{level:?}: {} quads", mess.stains.weight());
            // A burst's own splats at most, each with its drips on a wall: the one where it burst, those it throws and
            // its globs'. A few bursts' worth fit in the cap.
            let drips = (Sauce::Relish.drips().0).min(budget.drips) as usize;
            assert!((1 + plan.splats + plan.landing) * (1 + drips) <= budget.quads / 3, "{level:?}");
        }
    }

    #[test]
    fn a_burst_is_put_away_after_its_span_and_its_entities_are_used_again() {
        let mut app = game(Level::High);
        let world = app.world_mut();
        land(world, BAZOOKA, OPEN, down(), 1, 1.);
        let made = booms(world).entities();
        assert!(booms(world).busy());
        let now = frames(world, 1., SPAN + 0.3, 1. / 60.);
        assert!(!booms(world).busy() && booms(world).live() == 0, "all over");
        assert_eq!(drawn(world), 0, "nothing of it is drawn");
        let slot = &booms(world).slots[0];
        assert!(slot.entities().all(|e| world.get::<Card>(e).is_none()), "no card is turned to the camera");
        assert!(booms(world).lamps.iter().all(|l| world.get::<Visibility>(l.entity) == Some(&Visibility::Hidden) && world.get::<PointLight>(l.entity).unwrap().intensity == 0.));
        assert_eq!(shake(world), Vec3::ZERO);
        // Nothing is moved any more: a frame leaves every part where it is.
        let marker = Vec3::new(9., 9., 9.);
        let parts: Vec<Entity> = slot.entities().collect();
        for e in &parts {
            world.get_mut::<Transform>(*e).unwrap().translation = marker;
        }
        let now = frames(world, now, 1., 1. / 60.);
        assert!(parts.iter().all(|e| world.get::<Transform>(*e).unwrap().translation == marker));
        // One after another, a burst takes the same entities again: none are made.
        let mut now = now;
        for k in 0..6 {
            land(world, BAZOOKA, OPEN + Vec3::X * 0.01 * k as f32, down(), 2 + k, now);
            now = frames(world, now, SPAN + 0.1, 1. / 30.);
        }
        assert_eq!(booms(world).entities(), made, "the pool is used again");
        assert!(!booms(world).busy());
        // A new round puts away a burst under way at once.
        land(world, BAZOOKA, OPEN, down(), 9, now);
        frames(world, now, 0.1, 1. / 60.);
        fx::clear(world);
        assert!(!booms(world).busy() && drawn(world) == 0);
    }

    #[test]
    fn the_lens_is_spattered_only_within_range_and_with_nothing_between() {
        let plan = Plan::of(Level::High);
        let r = feast::SPLASH[BAZOOKA];
        assert_eq!(lens_count(&plan, r * LENS_REACH, r), 0);
        assert_eq!(lens_count(&plan, r * 3., r), 0);
        assert_eq!(lens_count(&plan, 0., r), plan.lens);
        let counts: Vec<usize> = (0..=12).map(|k| lens_count(&plan, r * LENS_REACH * k as f32 / 12., r)).collect();
        assert!(counts.windows(2).all(|w| w[1] <= w[0]) && counts[11] >= 1, "more the nearer: {counts:?}");
        assert!(counts[2] > counts[9], "{counts:?}");
        // As the game has it: the camera far, at the edge, near, and near but behind a block.
        let mut app = game(Level::High);
        let world = app.world_mut();
        let seen = |world: &mut World, eye: Vec3, burst: Vec3, seed: u32| {
            fx::clear(world);
            camera(world, eye, burst);
            land(world, BAZOOKA, burst, down(), seed, 1.);
            booms(world).smudges()
        };
        assert_eq!(seen(world, OPEN + Vec3::new(0.3, 0.12, -0.6), OPEN, 1), 0, "from 0.68 m");
        assert_eq!(seen(world, OPEN + Vec3::new(0., 0.13, -r * LENS_REACH), OPEN, 2), 0, "just beyond the reach");
        let reach = r * LENS_REACH;
        let near = seen(world, OPEN + Vec3::new(0., 0.1, -0.03), OPEN, 3);
        let edge = seen(world, OPEN + Vec3::new(0., 0.1, -((reach * 0.92).powi(2) - 0.01).sqrt()), OPEN, 4);
        assert!(near > edge && edge >= 1 && near <= plan.lens, "near {near}, at the edge {edge}");
        // Behind the steel block (its far face at z 2.92), the burst on its other side: nothing reaches the lens.
        world.resource_mut::<Game>().feast.tune.splash[BAZOOKA] = 0.4;
        assert_eq!(seen(world, Vec3::new(-0.34, feast::FLOOR + 0.06, 2.68), Vec3::new(-0.34, feast::FLOOR, 2.95), 5), 0, "the block is in the way");
        assert!(seen(world, Vec3::new(-0.15, feast::FLOOR + 0.06, 2.68), Vec3::new(-0.15, feast::FLOOR, 2.95), 6) > 0, "beside the block it is not");
        // The blobs cling, slide down and fade, and are gone with the lens put away.
        world.resource_mut::<Game>().feast.tune.splash[BAZOOKA] = r;
        seen(world, OPEN + Vec3::new(0., 0.13, -0.05), OPEN, 7);
        let lens = booms(world).lens.smudges.iter().find(|s| s.on).copied().unwrap();
        let mut t = 1.;
        let mut slid = vec![];
        let mut alpha = vec![];
        for _ in 0..60 {
            t = frames(world, t, 0.05, 1. / 60.);
            let Val::Vh(y) = world.get::<UiTransform>(lens.entity).unwrap().translation.y else { panic!() };
            slid.push(y);
            alpha.push(world.get::<ImageNode>(lens.entity).unwrap().color.alpha());
        }
        assert!(slid.windows(2).all(|w| w[1] >= w[0]) && slid[40] > slid[10] + 5., "it slides down: {slid:?}");
        assert!(alpha[2] > 0.8 && alpha[50] < 0.05, "and fades: {alpha:?}");
        assert_eq!(booms(world).smudges(), 0);
        let root = booms(world).lens.root.unwrap();
        assert_eq!(world.get::<Visibility>(root), Some(&Visibility::Hidden));
    }

    #[test]
    fn the_camera_shakes_hard_up_close_and_slightly_across_the_map_and_the_sound_is_louder_and_bassier_near() {
        let r = feast::SPLASH[BAZOOKA];
        assert!((jolt(0., r) - SHAKE).abs() < 1e-6);
        let steps: Vec<f32> = (0..=40).map(|k| jolt(k as f32 * 0.1, r)).collect();
        assert!(steps.windows(2).all(|w| w[1] < w[0]), "less the further: {steps:?}");
        assert!(jolt(r, r) > SHAKE * 0.5, "hard within the blast");
        assert!(jolt(3., r) > SHAKE * 0.01 && jolt(3., r) < SHAKE * 0.1, "slight across the map: {}", jolt(3., r));
        let (near, far) = (voices(0.05, r), voices(3., r));
        assert!(near[0].0 > far[0].0 && near[1].0 > far[1].0, "louder near");
        assert!(near[0].1 < far[0].1, "the boom deeper near");
        assert!(near[2].0 > 0.8 && far[2].0 == 0., "the thump only near");
        assert!(far[0].0 > 0.3, "and heard across the map");
        // As the game has it: shaken as hard as its distance says, at once, and over within a second.
        let mut app = game(Level::High);
        let world = app.world_mut();
        let hardest = |world: &World| booms(world).kicks.iter().map(|k| k.amp).fold(0., f32::max);
        let near = OPEN + Vec3::new(0., 0.13, -0.3);
        camera(world, near, OPEN);
        land(world, BAZOOKA, OPEN, down(), 1, 1.);
        assert!((hardest(world) - jolt(near.distance(OPEN), r)).abs() < 1e-6);
        let t = frames(world, 1., 0.03, 1. / 60.);
        assert!(shake(world).length() > 0.002, "{}", shake(world).length());
        frames(world, t, 1., 1. / 60.);
        assert_eq!(shake(world), Vec3::ZERO);
        // From across the map, a tremor.
        fx::clear(world);
        let far = OPEN + Vec3::new(-2.5, 0.4, -0.5);
        camera(world, far, OPEN);
        land(world, BAZOOKA, OPEN, down(), 2, 3.);
        assert!((hardest(world) - jolt(far.distance(OPEN), r)).abs() < 1e-6 && hardest(world) < jolt(near.distance(OPEN), r) * 0.15);
        frames(world, 3., 0.03, 1. / 60.);
        assert!(shake(world).length() > 0.);
    }
}

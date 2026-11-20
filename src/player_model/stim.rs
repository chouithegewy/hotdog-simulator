// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The stim as it is seen. While a hotdog's stim lasts it burns with blue fire and crackles with lightning from the
//! top of its head to its feet, and the player on stim sees the same fire lick up round the edges of the view.
//!
//! * **Built once.** Every mesh and material here is made once, in [`Look::new`], and shared by every hotdog and the
//!   first-person view. A frame only shows and hides entities, moves and scales them, and changes which of the shared
//!   meshes and materials each one uses: no mesh is built and no material changed while playing.
//! * **The fire.** A coat of flame tongues all over the bun and the sausage, from under the bun to a crown off the top
//!   of the head; a sleeve of them round each bone of the arms and the legs, down to the feet; and a faint glowing skin
//!   just off the body, which shows brightest round its outline. The tongues are a flip-book: [`FRAMES`] meshes of the
//!   same tongues at successive moments of their lives, each rising, flaring and dying in its own time, shown in turn,
//!   so that the fire licks upwards without a mesh being rebuilt.
//! * **The lightning.** Short jagged arcs (a few [`SHAPES`] of them, some forked) leap between points on the body, its
//!   hands and its feet and off the top of its head, each re-forming somewhere else every few frames. How many a hotdog
//!   has depends on the graphics level ([`arcs`]).
//! * **Light.** A soft blue light on the counter round the hotdog, at High and Medium only ([`lit`]).
//! * **Fading.** All of it comes up as the stim starts and dies away over its last half second ([`strength`]). The
//!   fire's materials come in [`STEPS`] of brightness, and the fading and the flicker pick one of them.
//! * **First person.** [`build_view`] hangs the same fire and lightning from the camera, and [`view`] shows them: flames
//!   lick up from the bottom edge and the sides of the view and arcs crackle over the hands and the gun, while the
//!   middle of the view, where the crosshair is, stays clear ([`CLEAR`]).
//!
//! The fire is unlit and casts no shadow. Its colour is in its meshes, from white-hot at the root of a tongue through blue
//! to the deep blue of its tip, so it reads the same on every hotdog's colour and over Forts' team bands. The lightning
//! is added to what is behind it; the flames are laid over it as well (premultiplied), so that they stay blue in front of
//! a sunlit counter or a bright hotdog instead of washing out to white.
use super::{casing, noise, smooth, PlayerModels, Pose, BONE, HIP, SHOULDER};
use crate::quality::Level;
use bevy::{asset::RenderAssetUsages, light::NotShadowCaster, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology};
use std::f32::consts::{PI, TAU};

/// Frames of the fire's flip-book, and how many of them are shown a second.
pub const FRAMES: usize = 6;
const RATE: f32 = 15.;
/// Shapes of arc, how many times a second each arc leaps somewhere else, and how long an arc's mesh is (one longer or
/// shorter is stretched along its length, so that every arc is as thick and as jagged).
const SHAPES: usize = 8;
const LEAP: f32 = 14.;
const ARC: f32 = 0.04;
/// Steps of brightness the fire's shared materials come in, and how bright the brightest lightning and flames are (the
/// flames cover what is behind them as well, so need less to show).
pub const STEPS: usize = 8;
const GAIN: f32 = 4.;
const VEIL: f32 = 2.6;
/// Seconds the stim's look takes to come up as the stim starts, and to die away at its end.
pub const FADE_IN: f32 = 0.35;
pub const FADE_OUT: f32 = 0.5;
/// The most arcs one hotdog has (at High).
pub const ARCS: usize = 14;
/// The light at full strength: its brightness (lumens) and how far it reaches (metres).
const LIGHT: f32 = 110.;
const REACH: f32 = 0.4;
/// The middle of the body seen from above: the buns sit a little behind the sausage.
const MIDDLE: Vec2 = Vec2::new(0., 0.006);
/// First person: the flames round the edges of the view and the arcs over the hands and the gun, at most.
pub const LICKS: usize = 24;
pub const SPARKS: usize = 8;
/// First person: how far from the eye the flames are drawn, how wide one flame's mesh is for its height of one, and
/// how far out from the middle of the view (as the tangent of the angle off the line of sight) everything stays.
const DEPTH: f32 = 0.12;
const LICK_SPAN: f32 = 0.72;
pub const CLEAR: f32 = 0.3;
/// First person: where the arcs over the hands and the gun may start, in the camera's frame (the gun is held at
/// `arena::GRIP`, low on the right).
const HELD: (Vec3, Vec3) = (Vec3::new(0.034, -0.078, -0.13), Vec3::new(0.072, -0.04, -0.088));

/// The fire's colours, linear: white-hot at the root of a tongue, through cyan and blue to the deep blue of its tip.
const HOT: Vec3 = Vec3::new(0.62, 0.84, 1.0);
const CYAN: Vec3 = Vec3::new(0.25, 0.62, 1.0);
const BLUE: Vec3 = Vec3::new(0.08, 0.30, 1.0);
const DEEP: Vec3 = Vec3::new(0.05, 0.10, 0.80);
/// The colour `s` of the way along a tongue, from its root (0) to its tip (1).
fn heat(s: f32) -> Vec3 {
    if s < 0.25 {
        HOT.lerp(CYAN, s / 0.25)
    } else if s < 0.7 {
        CYAN.lerp(BLUE, (s - 0.25) / 0.45)
    } else {
        BLUE.lerp(DEEP, ((s - 0.7) / 0.3).min(1.))
    }
}

/// How strongly the stim shows, 0 to 1, with `left` seconds of it to go and `age` seconds since it started: up from
/// nothing over its first FADE_IN seconds and down to nothing over its last FADE_OUT.
pub fn strength(left: f32, age: f32) -> f32 {
    if left <= 0. {
        return 0.;
    }
    smooth(age / FADE_IN) * smooth(left / FADE_OUT)
}
/// How many arcs a stimmed hotdog shows at each graphics level.
pub fn arcs(level: Level) -> usize {
    match level {
        Level::High => ARCS,
        Level::Medium => 10,
        Level::Low => 6,
        Level::Lowest => 4,
    }
}
/// Whether a stimmed hotdog lights the counter round it: not at Low and Lowest, which have few lights of their own.
pub fn lit(level: Level) -> bool {
    matches!(level, Level::High | Level::Medium)
}
/// First person: how many arcs crackle over the hands and the gun at each graphics level.
pub fn sparks(level: Level) -> usize {
    match level {
        Level::High => SPARKS,
        Level::Medium => 6,
        Level::Low => 4,
        Level::Lowest => 3,
    }
}

/// The light a stimmed hotdog gives off. The kitchen's own lamps are dimmed to the theme (`visuals`); this one is not.
#[derive(Component)]
pub struct StimLight;

/// The meshes and materials of the stim, shared by every hotdog and the first-person view.
#[derive(Clone)]
pub struct Look {
    /// The coat of fire over the bun and the sausage, a mesh for each frame of the flip-book.
    coat: Vec<Handle<Mesh>>,
    /// A sleeve of fire round one bone of an arm or a leg, for each frame.
    sleeve: Vec<Handle<Mesh>>,
    /// The glowing skin just off the body.
    skin: Handle<Mesh>,
    /// Arcs of lightning, each from the origin ARC metres along +z and bowed towards +x.
    arc: Vec<Handle<Mesh>>,
    /// A first-person flame for each frame: tongues rising from the origin up +y, about one unit tall.
    lick: Vec<Handle<Mesh>>,
    /// The fire's materials, dimmest first, the colour in the meshes: the lightning's added to what is behind it; the
    /// flames' (`veil`) laid over it as well, so that blue fire stays blue in front of a sunlit counter instead of
    /// washing out to white.
    glow: Vec<Handle<StandardMaterial>>,
    veil: Vec<Handle<StandardMaterial>>,
}
impl Look {
    /// Builds them all. `detailed` is false for a software renderer, which gets fewer tongues.
    pub fn new(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, detailed: bool) -> Self {
        let (rows, around) = if detailed { (8, 12) } else { (6, 9) };
        Look {
            coat: (0..FRAMES).map(|f| meshes.add(coat(f, rows, around))).collect(),
            sleeve: (0..FRAMES).map(|f| meshes.add(sleeve(f))).collect(),
            skin: meshes.add(skin(if detailed { 24 } else { 14 }, if detailed { 32 } else { 18 })),
            arc: (0..SHAPES as u32).map(|k| meshes.add(arc(k))).collect(),
            lick: (0..FRAMES).map(|f| meshes.add(lick(f))).collect(),
            glow: bank(mats, AlphaMode::Add, GAIN),
            veil: bank(mats, AlphaMode::Premultiplied, VEIL),
        }
    }
    /// The lightning's material for a brightness from 0 to 1, and the flames' (`veil`): none for nothing at all.
    fn step(&self, bright: f32) -> Option<&Handle<StandardMaterial>> {
        pick(&self.glow, bright)
    }
    fn flame(&self, bright: f32) -> Option<&Handle<StandardMaterial>> {
        pick(&self.veil, bright)
    }
    /// Hangs a hotdog's stim on its body, all of it hidden: the coat, the skin, the arcs and the light from its torso,
    /// and a sleeve from each of `bones` (the thighs and shins, then the upper arms and forearms).
    pub fn spawn(&self, world: &mut World, torso: Entity, bones: [Entity; 8]) -> Aura {
        let root = world.spawn((Name::new("Stim"), Transform::default(), Visibility::Hidden)).id();
        world.entity_mut(torso).add_child(root);
        let glow = &self.glow[STEPS - 1];
        let part = |world: &mut World, parent: Entity, mesh: &Handle<Mesh>, shown: Visibility| {
            let e = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(glow.clone()), Transform::default(), shown, NotShadowCaster)).id();
            world.entity_mut(parent).add_child(e);
            e
        };
        let coat = part(world, root, &self.coat[0], Visibility::Inherited);
        let skin = part(world, root, &self.skin, Visibility::Inherited);
        let arcs = [(); ARCS].map(|_| part(world, root, &self.arc[0], Visibility::Hidden));
        let sleeves = bones.map(|bone| part(world, bone, &self.sleeve[0], Visibility::Hidden));
        let light = world
            .spawn((
                StimLight,
                PointLight { color: Color::srgb(0.35, 0.6, 1.), intensity: 0., range: REACH, radius: 0.02, shadow_maps_enabled: false, ..default() },
                Transform::from_xyz(0., 0.05, 0.),
                Visibility::Hidden,
            ))
            .id();
        world.entity_mut(root).add_child(light);
        Aura { root, coat, skin, sleeves, arcs, light }
    }
}

/// STEPS materials for the fire, dimmest first, the brightest `gain`: unlit, two-sided, untouched by fog, the colour
/// in the meshes and `blend` how they go over what is behind them. Laid over it (premultiplied), each step covers as much more of what
/// is behind as it gives off light.
fn bank(mats: &mut Assets<StandardMaterial>, blend: AlphaMode, gain: f32) -> Vec<Handle<StandardMaterial>> {
    (1..=STEPS)
        .map(|k| {
            let share = k as f32 / STEPS as f32;
            let v = gain * share;
            let cover = if blend == AlphaMode::Add { 1. } else { share };
            mats.add(StandardMaterial {
                base_color: Color::LinearRgba(LinearRgba::new(v, v, v, cover)),
                alpha_mode: blend,
                unlit: true,
                fog_enabled: false,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        })
        .collect()
}
/// The step of `bank` for a brightness from 0 to 1: none for nothing at all.
fn pick(bank: &[Handle<StandardMaterial>], bright: f32) -> Option<&Handle<StandardMaterial>> {
    let k = (bright.clamp(0., 1.) * STEPS as f32).ceil() as usize;
    (k > 0).then(|| &bank[k.min(STEPS) - 1])
}

/// One hotdog's stim: what `Look::spawn` hung on it.
#[derive(Clone, Copy)]
pub struct Aura {
    /// Hangs from the torso and carries the coat, the skin, the arcs and the light: hiding it hides them all.
    root: Entity,
    coat: Entity,
    skin: Entity,
    /// On the thighs and shins, then the upper arms and forearms, left side first.
    sleeves: [Entity; 8],
    arcs: [Entity; ARCS],
    light: Entity,
}

/// Shows a hotdog's stim this frame: as strong as `strength` (0 hides it), on the body as `pose` has it (None, fallen,
/// hides it too), with as many arcs and as much light as the graphics level allows. `id` keeps each hotdog's fire out
/// of step with everybody else's.
#[allow(clippy::too_many_arguments)]
/// The coat and the skin round the head bend with its nod (`nod`, bend.rs), and so do the arcs that crawl over it.
pub fn animate(world: &mut World, look: &Look, aura: &Aura, id: u32, time: f32, strength: f32, pose: Option<&Pose>, level: Level, nod: f32) {
    let beat = (time * RATE + noise(id) * FRAMES as f32) as u32;
    let flicker = 0.8 + 0.2 * noise(beat.wrapping_mul(7919) ^ id);
    let glow = look.flame(strength * flicker);
    let (Some(pose), Some(glow)) = (pose, glow) else {
        // Hidden already: nothing to do (the sleeves were hidden with it).
        if world.get::<Visibility>(aura.root).is_none_or(|v| *v == Visibility::Hidden) {
            return;
        }
        show(world, aura.root, false);
        for sleeve in aura.sleeves {
            show(world, sleeve, false);
        }
        return;
    };
    show(world, aura.root, true);
    let frame = beat as usize % FRAMES;
    // Every other run through the flip-book is mirrored (the body is the same either side), for twice the variety.
    let mirror = if (beat / FRAMES as u32 + id) % 2 == 1 { -1. } else { 1. };
    wear_bent(world, aura.coat, &look.coat[frame], glow, nod);
    place(world, aura.coat, Transform::from_scale(Vec3::new(mirror, 1. + 0.06 * (noise(beat ^ 0x5a5a) - 0.5), 1.)));
    if let Some(dim) = look.flame(strength * (0.6 + 0.4 * flicker)) {
        wear_bent(world, aura.skin, &look.skin, dim, nod);
    }
    let breath = 1. + 0.025 * (time * 9. + id as f32).sin();
    place(world, aura.skin, Transform::from_scale(Vec3::new(breath, 1., breath)));
    for (k, sleeve) in aura.sleeves.into_iter().enumerate() {
        show(world, sleeve, true);
        wear(world, sleeve, &look.sleeve[(frame + k * 2) % FRAMES], glow);
    }
    // The arcs, each re-forming on its own beat; the brightest material, however the fire flickers.
    let bright = look.step(strength).unwrap_or(&look.glow[0]);
    let shown = arcs(level);
    for (i, arc) in aura.arcs.into_iter().enumerate() {
        let slot = (time * LEAP + noise(i as u32 * 13 + id) * 9.) as u32;
        let seed = slot.wrapping_mul(2_654_435_761) ^ (i as u32).wrapping_mul(97).wrapping_add(id.wrapping_mul(7717));
        match (i < shown).then(|| leap(seed, pose)).flatten() {
            Some((a, b, out)) => {
                // (Over the head, where its nod has it.)
                let (a, b) = (kitchen_sim::head::bend(nod, a), kitchen_sim::head::bend(nod, b));
                let out = Quat::from_rotation_x(kitchen_sim::head::turn_at(nod, a.y)) * out;
                show(world, arc, true);
                wear(world, arc, &look.arc[seed as usize % SHAPES], bright);
                place(world, arc, span(a, b, out, 1.));
            }
            None => show(world, arc, false),
        }
    }
    let lamp = lit(level);
    show(world, aura.light, lamp);
    if lamp {
        if let Some(mut light) = world.get_mut::<PointLight>(aura.light) {
            light.intensity = LIGHT * strength * flicker;
        }
    }
}

/// Where arc `seed` leaps this time, in the torso's frame: from one point to another and which way is out from the
/// body there (an arc bows out that way). Most crawl over the bun and the sausage; some jump to a hand or a foot, or
/// off the top of the head; a few rest (None), so the lightning flickers.
fn leap(seed: u32, pose: &Pose) -> Option<(Vec3, Vec3, Vec3)> {
    let n = |k: u32| noise(seed.wrapping_add(k.wrapping_mul(0x9e37)));
    let kind = n(0);
    if kind < 0.15 {
        return None;
    }
    let angle = n(1) * TAU;
    let side = if n(2) < 0.5 { 0 } else { 1 };
    // Round the body towards that side: +x is the right (side 1), -x the left.
    let towards = |spread: f32| if side == 1 { 0. } else { PI } + (n(3) - 0.5) * spread;
    Some(if kind < 0.6 {
        let y = 0.03 + 0.14 * n(4);
        let (a, out) = hull(y, angle, 0.004);
        let (b, _) = hull((y + (n(5) - 0.5) * 0.08).clamp(0.026, 0.172), angle + (n(6) - 0.5) * 0.7, 0.004);
        (a, b, out)
    } else if kind < 0.75 {
        let (a, out) = hull(0.08 + 0.05 * n(4), towards(1.2), 0.003);
        (a, hands(pose)[side] + Vec3::new(n(5) - 0.5, n(6) - 0.5, n(7) - 0.5) * 0.006, out)
    } else if kind < 0.9 {
        let (a, out) = hull(0.026 + 0.02 * n(4), towards(1.6), 0.003);
        (a, feet(pose)[side] + Vec3::new(0., 0.004, 0.), out)
    } else {
        let (a, out) = hull(0.17, angle, 0.);
        (a, a + (Vec3::Y + out * (0.3 + 0.6 * n(5))).normalize() * (0.018 + 0.025 * n(6)), out)
    })
}
/// The hands' ends of the forearms, in the torso's frame, left then right.
fn hands(p: &Pose) -> [Vec3; 2] {
    [-1., 1.].map(|side: f32| {
        let i = (side > 0.) as usize;
        let bone = Vec3::NEG_Y * BONE * p.reach[i];
        Vec3::new(side * SHOULDER.x, SHOULDER.y, 0.) + p.shoulders[i] * (bone + p.elbows[i] * bone)
    })
}
/// The feet, in the torso's frame (the legs hang from the hips, which the torso leans and turns on), left then right.
fn feet(p: &Pose) -> [Vec3; 2] {
    let pivot = Vec3::Y * HIP.y;
    let origin = pivot + p.shift - p.torso * pivot;
    [-1., 1.].map(|side: f32| {
        let i = (side > 0.) as usize;
        let bone = Vec3::NEG_Y * BONE;
        let foot = Vec3::new(side * HIP.x, HIP.y, 0.) + p.shift + p.hips[i] * (bone + p.knees[i] * bone);
        p.torso.inverse() * (foot - origin)
    })
}
/// An arc's mesh laid from `a` to `b`, bowed out along `out`: stretched to reach, `thick` times as thick and jagged.
fn span(a: Vec3, b: Vec3, out: Vec3, thick: f32) -> Transform {
    let along = b - a;
    let length = along.length().max(1e-4);
    let z = along / length;
    let x = (out - z * out.dot(z)).try_normalize().unwrap_or_else(|| z.any_orthonormal_vector());
    let scale = Vec3::new(thick, thick, length / ARC);
    Transform { translation: a, rotation: Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z)), scale }
}

/// The outside of the bun and the sausage at height `y`, seen from the middle of the body along `angle` (0 is +x, the
/// right; a quarter turn is +z, the back), and `out` beyond it, in the torso's frame: the point there, and the way out
/// from the body there, level.
pub fn hull(y: f32, angle: f32, out: f32) -> (Vec3, Vec3) {
    let d = Vec2::new(angle.cos(), angle.sin());
    let (middle, radius) = casing(y);
    let u = (y - 0.031) / 0.132;
    let cap = if u > 0. && u < 1. { (u * PI).sin().sqrt() } else { 0. };
    let shapes = [
        (Vec2::new(middle.x, middle.z), Vec2::splat(radius)),
        (Vec2::new(-0.023, 0.012), Vec2::new(0.017, 0.026) * cap),
        (Vec2::new(0.023, 0.012), Vec2::new(0.017, 0.026) * cap),
    ];
    // The furthest any of the three reaches along that way from the middle (a ray from inside an ellipse leaves it once).
    let mut best = (0., d);
    for (centre, radii) in shapes {
        if radii.min_element() < 1e-4 {
            continue;
        }
        let (p, q) = ((MIDDLE - centre) / radii, d / radii);
        let (a, b, c) = (q.dot(q), p.dot(q), p.dot(p) - 1.);
        let disc = b * b - a * c;
        if disc < 0. {
            continue;
        }
        let t = (-b + disc.sqrt()) / a;
        if t > best.0 {
            let hit = MIDDLE + d * t;
            best = (t, ((hit - centre) / (radii * radii)).normalize_or(d));
        }
    }
    let at = MIDDLE + d * (best.0 + out);
    (Vec3::new(at.x, y, at.y), Vec3::new(best.1.x, 0., best.1.y))
}

/// A mesh being built: its points, their colours (the alpha is how much of the glow shows) and its triangles.
#[derive(Default)]
struct Build {
    at: Vec<[f32; 3]>,
    tint: Vec<[f32; 4]>,
    tri: Vec<u32>,
}
impl Build {
    /// A ribbon down `spine`, a list of its middle, the half-width either side of it and its colour there: three
    /// columns of points, so the glow is strongest down the middle and `edge` of it at the sides.
    fn ribbon(&mut self, spine: &[(Vec3, Vec3, Vec3, f32)], edge: f32) {
        let first = self.at.len() as u32;
        for (middle, half, colour, alpha) in spine {
            for (at, a) in [(*middle - *half, alpha * edge), (*middle, *alpha), (*middle + *half, alpha * edge)] {
                self.at.push(at.to_array());
                self.tint.push([colour.x, colour.y, colour.z, a]);
            }
        }
        for row in 0..spine.len().saturating_sub(1) as u32 {
            for col in 0..2 {
                let a = first + row * 3 + col;
                let b = a + 3;
                self.tri.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
    /// A tongue of flame rising from `root` along `up`, `length` long, its tip bent `bend` aside, as bright as `glow`:
    /// a ribbon for each of `ways` (the way across it, its half-width at its widest and how much of the glow it shows).
    /// Two crossed show it from any side.
    fn tongue(&mut self, root: Vec3, up: Vec3, length: f32, bend: Vec3, glow: f32, ways: &[(Vec3, f32, f32)]) {
        for &(way, wide, share) in ways {
            let spine: Vec<_> = [0., 0.2, 0.42, 0.66, 0.85, 1.]
                .iter()
                .map(|&s: &f32| {
                    let half = wide * (PI * (0.18 + 0.82 * s)).sin().max(0.).powf(0.8) * (1. - s).sqrt();
                    (root + up * length * s + bend * s * s, way * half, heat(s), glow * share * (1. - 0.6 * s))
                })
                .collect();
            self.ribbon(&spine, 0.2);
        }
    }
    /// A bolt of lightning down `points`: a white-hot core `core` wide in a blue glow `halo` wide, each as two ribbons
    /// crossed, narrowing to its ends.
    fn bolt(&mut self, points: &[Vec3], core: f32, halo: f32) {
        let last = points.len() as f32 - 1.;
        for (wide, colour, alpha) in [(halo, BLUE.lerp(CYAN, 0.5), 0.4), (core, HOT.lerp(Vec3::ONE, 0.5), 1.)] {
            for way in [Vec3::X, Vec3::Y] {
                let spine: Vec<_> =
                    points.iter().enumerate().map(|(i, p)| (*p, way * wide * (0.4 + 0.6 * (PI * i as f32 / last).sin()), colour, alpha)).collect();
                self.ribbon(&spine, 0.15);
            }
        }
    }
    /// A closed tube round the y axis: `rows` along it and `around` round it, each point and its colour from
    /// `sample(along, round)`, both 0 to 1.
    fn tube(&mut self, rows: u32, around: u32, sample: impl Fn(f32, f32) -> (Vec3, [f32; 4])) {
        let first = self.at.len() as u32;
        for i in 0..=rows {
            for j in 0..=around {
                let (at, tint) = sample(i as f32 / rows as f32, j as f32 / around as f32);
                self.at.push(at.to_array());
                self.tint.push(tint);
            }
        }
        for i in 0..rows {
            for j in 0..around {
                let a = first + i * (around + 1) + j;
                let b = a + around + 1;
                self.tri.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
    /// The mesh, its colours as they are for light added to what is behind it.
    fn mesh(self) -> Mesh {
        let normals = vec![[0., 1., 0.]; self.at.len()];
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.at)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.tint)
            .with_inserted_indices(Indices::U32(self.tri))
    }
    /// The mesh for laying over what is behind it (a premultiplied colour): it gives off the same light, and covers
    /// `cover` of what is behind wherever it shows fully.
    fn veiled(mut self, cover: f32) -> Mesh {
        for [r, g, b, a] in &mut self.tint {
            (*r, *g, *b, *a) = (*r * *a, *g * *a, *b * *a, *a * cover);
        }
        self.mesh()
    }
}
/// How far through its life a tongue is at moment `frame` of the flip-book, if it was born `phase` of the way round:
/// from 0 (just lit) to 1 (gone); and how big it is then, nothing at either end and all of it in the middle.
fn life(frame: usize, phase: f32) -> (f32, f32) {
    let t = (phase + frame as f32 / FRAMES as f32).fract();
    (t, (t * PI).sin())
}
/// The coat of fire over the bun and the sausage at moment `frame` of the flip-book: `rows` of tongues `around` the
/// body from under the bun to the top of the head, each leaning out and rising as it ages, and a crown of taller ones
/// off the top of the head.
fn coat(frame: usize, rows: usize, around: usize) -> Mesh {
    let mut b = Build::default();
    let mut site = 0u32;
    for row in 0..rows {
        let y = 0.022 + 0.144 * row as f32 / (rows - 1) as f32;
        for k in 0..around {
            site += 1;
            let n = |i: u32| noise(site * 31 + i);
            let angle = (k as f32 + 0.5 * (row % 2) as f32 + 0.6 * (n(0) - 0.5)) / around as f32 * TAU;
            let (t, big) = life(frame, n(1));
            let (root, out) = hull(y + 0.012 * t, angle, 0.0015);
            let along = Vec3::Y.cross(out).normalize_or(Vec3::X);
            let length = (0.022 + 0.018 * n(2)) * (0.45 + 0.55 * big);
            let sway = along * (TAU * (t + n(3))).sin() * 0.45 * length;
            let wide = 0.007 + 0.003 * n(4);
            // Over the body a tongue is seen flat on, a faint wisp; round the outline edge on, sticking out of it, bright.
            b.tongue(root, (Vec3::Y + out * 0.6).normalize(), length, sway, 0.6 * big.sqrt(), &[(along, wide * 0.5, 0.35), (out, wide * 0.8, 1.)]);
        }
    }
    for k in 0..8 {
        site += 1;
        let n = |i: u32| noise(site * 31 + i);
        let angle = (k as f32 + 0.4 * n(0)) / 8. * TAU;
        let (t, big) = life(frame, n(1));
        let (root, out) = hull(0.166 + 0.008 * t, angle, 0.);
        let along = Vec3::Y.cross(out).normalize_or(Vec3::X);
        let length = (0.04 + 0.025 * n(2)) * (0.5 + 0.5 * big);
        let sway = along * (TAU * (t + n(3))).sin() * 0.3 * length;
        b.tongue(root, (Vec3::Y + out * 0.2).normalize(), length, sway, 0.6 * big.sqrt(), &[(along, 0.009, 0.6), (out, 0.0075, 1.)]);
    }
    b.veiled(0.7)
}
/// A sleeve of fire round one bone of an arm or a leg (a capsule 4 mm across along y, its middle at the origin) at
/// moment `frame` of the flip-book: three rings of small tongues leaning out from it, the lowest at its end.
fn sleeve(frame: usize) -> Mesh {
    let mut b = Build::default();
    for row in 0..3 {
        for k in 0..6u32 {
            let site = 500 + row * 6 + k;
            let n = |i: u32| noise(site * 31 + i);
            let angle = (k as f32 + 0.5 * row as f32 + 0.5 * (n(0) - 0.5)) / 6. * TAU;
            let (t, big) = life(frame, n(1));
            let out = Vec3::new(angle.cos(), 0., angle.sin());
            let root = out * 0.0055 + Vec3::Y * (-0.009 + 0.006 * row as f32 + 0.004 * t);
            let along = Vec3::Y.cross(out);
            let length = (0.024 + 0.014 * n(2)) * (0.45 + 0.55 * big);
            b.tongue(root, (Vec3::Y + out * 0.8).normalize(), length, along * 0.3 * length, big.sqrt(), &[(along, 0.008, 0.6), (out, 0.007, 1.)]);
        }
    }
    b.veiled(0.75)
}
/// The glowing skin just off the body, from under the bun to over the top of the head: three thin shells, each further
/// out fainter. Over the body the eye looks through one side of each; round the outline through both, so it glows
/// brightest there.
fn skin(rows: u32, around: u32) -> Mesh {
    let mut b = Build::default();
    let colour = BLUE.lerp(CYAN, 0.3);
    for (out, alpha) in [(0.004, 0.13), (0.010, 0.08), (0.017, 0.045)] {
        b.tube(rows, around, |u, v| {
            let y = 0.012 + u * 0.176;
            let inside = y.clamp(0.026, 0.174);
            let (p, _) = hull(inside, v * TAU, out);
            // Rounded off past the ends, to close over the top of the head and under the bun.
            let beyond = (y - inside).abs() / 0.014;
            let shrink = (1. - beyond * beyond).max(0.).sqrt();
            let middle = Vec3::new(MIDDLE.x, y, MIDDLE.y);
            (middle + (Vec3::new(p.x, y, p.z) - middle) * shrink, [colour.x, colour.y, colour.z, alpha])
        });
    }
    b.veiled(0.45)
}
/// A jagged arc of lightning from the origin ARC metres along +z, bowed out towards +x; every other one forked. Its
/// white-hot core is 1.4 mm thick and its glow 5.2 mm.
fn arc(shape: u32) -> Mesh {
    let n = |i: u32| noise(shape * 101 + i + 7);
    let bends = 7;
    let bow = 0.25 + 0.2 * n(0);
    let points: Vec<Vec3> = (0..=bends)
        .map(|i| {
            let s = i as f32 / bends as f32;
            let jag = if i == 0 || i == bends { 0. } else { 1. };
            Vec3::new(bow * (s * PI).sin() + jag * 0.17 * (n(1 + 2 * i) - 0.5), jag * 0.17 * (n(2 + 2 * i) - 0.5), s + jag * 0.06 * (n(40 + i) - 0.5)) * ARC
        })
        .collect();
    let mut b = Build::default();
    b.bolt(&points, 0.0007, 0.0026);
    if shape % 2 == 1 {
        let from = points[2 + (shape as usize / 2) % 3];
        let way = Vec3::new(0.10 + 0.08 * n(60), 0.16 * (n(61) - 0.5), 0.26) * ARC;
        let fork: Vec<Vec3> =
            (0..=3).map(|i| from + way * (i as f32 / 3.) + Vec3::new(n(62 + i) - 0.5, n(66 + i) - 0.5, 0.) * if i == 0 { 0. } else { 0.06 * ARC }).collect();
        b.bolt(&fork, 0.0005, 0.0018);
    }
    b.mesh()
}
/// A first-person flame at moment `frame` of the flip-book, flat to the eye: a tall tongue rising from the origin about
/// one unit up +y, two smaller ones at its foot, and a wisp breaking off its tip and rising away.
fn lick(frame: usize) -> Mesh {
    let mut b = Build::default();
    let root = Vec3::ZERO;
    for (k, (x, tall, phase)) in [(0., 1., 0.), (-0.2, 0.6, 0.37), (0.21, 0.55, 0.71)].into_iter().enumerate() {
        let (t, _) = life(frame, phase);
        let length = tall * (0.75 + 0.25 * (t * TAU).sin());
        let sway = Vec3::X * (TAU * (t + 0.2 * k as f32)).sin() * 0.14 * length;
        b.tongue(root + Vec3::X * x, Vec3::Y, length, sway, if k == 0 { 1. } else { 0.8 }, &[(Vec3::X, 0.16 * tall + 0.04, 1.)]);
    }
    let (t, _) = life(frame, 0.5);
    let wisp = 0.22 * (1. - t);
    b.tongue(root + Vec3::new(0.06 * (t * TAU).sin(), 0.62 + 0.5 * t, 0.), Vec3::Y, wisp, Vec3::ZERO, 0.7 * (1. - t), &[(Vec3::X, wisp * 0.4, 1.)]);
    // A broad, faint glow behind them all, which runs together with its neighbours' into a band of blue light.
    b.tongue(root, Vec3::Y, 0.7, Vec3::ZERO, 0.55, &[(Vec3::X, 0.45, 1.)]);
    b.veiled(0.85)
}

/// First person: where the flames stand round the edges of a view `aspect` wide to its height, at a graphics level. Each
/// is its root (in units of the distance to the eye: x across, y up, the middle of the view at 0), how far it leans
/// (radians, anticlockwise), how wide and how tall it is. A row along the bottom, taller towards the corners, and a
/// column up each side leaning in, as long as the middle leaves them room.
pub fn licks(level: Level, aspect: f32) -> Vec<(Vec2, f32, f32, f32)> {
    let (gap, up_each_side) = match level {
        Level::High => (0.26, 4),
        Level::Medium => (0.32, 3),
        Level::Low => (0.42, 3),
        Level::Lowest => (0.52, 2),
    };
    let high = (crate::viewmodel::FOV * 0.5).tan();
    let wide = high * aspect.max(0.1);
    let across = ((2. * wide / gap).ceil() as usize).clamp(3, LICKS - 2 * up_each_side);
    let mut out = Vec::with_capacity(LICKS);
    for i in 0..across {
        let s = (i as f32 + 0.5) / across as f32 * 2. - 1.;
        let tall = 0.36 + 0.26 * s * s;
        out.push((Vec2::new(s * wide, -high - 0.03), -0.2 * s, 2. * wide / across as f32 * 1.35, tall));
    }
    // The room left at the sides between the edge and the clear middle.
    let room = ((wide - CLEAR) / 0.45).clamp(0.15, 1.);
    for side in [-1., 1.] {
        for j in 0..up_each_side {
            let s = (j as f32 + 0.5) / up_each_side as f32;
            let tall = (0.5 - 0.2 * s) * room;
            out.push((Vec2::new(side * wide, -high + 1.25 * high * s), side * (1.0 - 0.35 * s), tall * 0.9, tall));
        }
    }
    out
}
/// First person: flame `i`, standing at `place` (see `licks`), at `now`: which frame of the flip-book it shows and
/// where, flickering in height.
fn lick_pose(place: (Vec2, f32, f32, f32), i: usize, now: f32) -> (usize, Transform) {
    let (root, lean, wide, tall) = place;
    let beat = (now * RATE + i as f32 * 2.3) as u32;
    let tall = tall * (0.85 + 0.25 * noise(beat.wrapping_mul(17) + i as u32));
    let mirror = if (beat / FRAMES as u32 + i as u32) % 2 == 1 { -1. } else { 1. };
    let tr = Transform::from_xyz(root.x * DEPTH, root.y * DEPTH, -DEPTH)
        .with_rotation(Quat::from_rotation_z(lean))
        .with_scale(Vec3::new(mirror * wide / LICK_SPAN * DEPTH, tall * DEPTH, DEPTH));
    (beat as usize % FRAMES, tr)
}
/// First person: where arc `k` crackles over the hands and the gun at `now`, in the camera's frame; None while it rests.
fn spark(k: usize, now: f32) -> Option<(usize, Transform)> {
    let slot = (now * LEAP + k as f32 * 0.37) as u32;
    let seed = slot.wrapping_mul(2_654_435_761) ^ (k as u32).wrapping_mul(0x51ed);
    let n = |i: u32| noise(seed.wrapping_add(i.wrapping_mul(0x9e37)));
    if n(0) < 0.25 {
        return None;
    }
    let (lo, hi) = HELD;
    let a = lo + (hi - lo) * Vec3::new(n(1), n(2), n(3));
    let way = Vec3::new(n(4) - 0.5, n(5) - 0.5, n(6) - 0.5).normalize_or(Vec3::X);
    let b = (a + way * (0.012 + 0.018 * n(7))).clamp(lo, hi);
    // Bowed away from the middle of the view.
    let out = Vec3::new(a.x, a.y, 0.).normalize_or(Vec3::X);
    Some((seed as usize % SHAPES, span(a, b, out, 0.7)))
}

/// First person: hangs the stim's flames and arcs from the camera, hidden. `view` shows them; the first LICKS are the
/// flames, the rest the arcs.
pub fn build_view(world: &mut World, camera: Entity) -> Vec<Entity> {
    let look = world.resource::<PlayerModels>().stim.clone();
    (0..LICKS + SPARKS)
        .map(|i| {
            let mesh = if i < LICKS { &look.lick[0] } else { &look.arc[0] };
            let e = world
                .spawn((
                    Name::new(if i < LICKS { "Stim flame" } else { "Stim arc" }),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(look.glow[STEPS - 1].clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id();
            world.entity_mut(camera).add_child(e);
            e
        })
        .collect()
}
/// First person: shows the stim's flames and arcs (from `build_view`) at `strength` (0 hides them), for the camera's
/// shape and the graphics level.
pub fn view(world: &mut World, bolts: &[Entity], camera: Option<Entity>, now: f32, strength: f32) {
    if bolts.is_empty() || !world.contains_resource::<PlayerModels>() {
        return;
    }
    let level = world.get_resource::<crate::quality::Detail>().map_or(Level::High, |d| d.level);
    let aspect = camera
        .and_then(|c| match world.get::<Projection>(c) {
            Some(Projection::Perspective(p)) => Some(p.aspect_ratio),
            _ => None,
        })
        .unwrap_or(16. / 9.);
    world.resource_scope(|world, models: Mut<PlayerModels>| {
        let look = &models.stim;
        let flicker = 0.85 + 0.15 * noise((now * RATE) as u32 ^ 0x77);
        let glow = look.flame(strength * flicker);
        let bright = look.step(strength);
        let places = if glow.is_some() { licks(level, aspect) } else { vec![] };
        let shown = sparks(level);
        for (i, &e) in bolts.iter().enumerate() {
            let pose = if i < LICKS {
                places.get(i).map(|place| {
                    let (frame, tr) = lick_pose(*place, i, now);
                    (&look.lick[frame], tr, glow)
                })
            } else {
                let k = i - LICKS;
                (k < shown).then(|| spark(k, now)).flatten().map(|(shape, tr)| (&look.arc[shape], tr, bright))
            };
            match pose {
                Some((mesh, tr, Some(material))) => {
                    if world.get::<Visibility>(e).is_some_and(|v| *v != Visibility::Visible) {
                        world.entity_mut(e).insert(Visibility::Visible);
                    }
                    wear(world, e, mesh, material);
                    place(world, e, tr);
                }
                _ => show(world, e, false),
            }
        }
    });
}

/// Shows (inherited from its parent) or hides an entity, touching it only if that changes anything.
fn show(world: &mut World, e: Entity, on: bool) {
    let want = if on { Visibility::Inherited } else { Visibility::Hidden };
    if let Some(mut v) = world.get_mut::<Visibility>(e) {
        if *v != want {
            *v = want;
        }
    }
}
/// `wear` for the coat and the skin round the whole body: the material's bent copy (bend.rs), the nod in the tag, so that
/// the fire bends with the head. Where nothing is drawn bent (a test's world), the material itself.
fn wear_bent(world: &mut World, e: Entity, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, nod: f32) {
    let Some(bent) = super::bend::bent(world, material) else {
        return wear(world, e, mesh, material);
    };
    if let Some(mut m) = world.get_mut::<Mesh3d>(e) {
        if m.0 != *mesh {
            m.0 = mesh.clone();
        }
    }
    if world.get::<MeshMaterial3d<super::bend::BentMaterial>>(e).is_none_or(|m| m.0 != bent) {
        world.entity_mut(e).remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(bent));
    }
    if world.get::<bevy::mesh::MeshTag>(e).is_none_or(|t| t.0 != nod.to_bits()) {
        world.entity_mut(e).insert(super::bend::tag(nod));
    }
}
/// Gives an entity a mesh and a material of the shared ones, touching it only if that changes anything.
fn wear(world: &mut World, e: Entity, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>) {
    if let Some(mut m) = world.get_mut::<Mesh3d>(e) {
        if m.0 != *mesh {
            m.0 = mesh.clone();
        }
    }
    if let Some(mut m) = world.get_mut::<MeshMaterial3d<StandardMaterial>>(e) {
        if m.0 != *material {
            m.0 = material.clone();
        }
    }
}
/// Moves an entity, touching it only if that changes anything.
fn place(world: &mut World, e: Entity, tr: Transform) {
    if let Some(mut t) = world.get_mut::<Transform>(e) {
        if *t != tr {
            *t = tr;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_model::{animate, Cues, Rig};
    use crate::quality::Detail;
    use crate::visuals::Quality;

    fn points(mesh: &Mesh) -> Vec<Vec3> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            bevy::mesh::VertexAttributeValues::Float32x3(p) => p.iter().map(|p| Vec3::from_array(*p)).collect(),
            _ => panic!("positions"),
        }
    }
    /// A world with the hotdog models in it (as the game has them) at graphics `level`, and hotdog 2 standing at the
    /// origin.
    fn hotdog(level: Level) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>();
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let models = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|_, mut materials: Mut<Assets<StandardMaterial>>| PlayerModels::new(&mut meshes, &mut materials, &server, false))
        });
        world.insert_resource(models.clone());
        world.insert_resource(Detail::of(level, Quality::Full));
        let root = models.spawn(world, Vec3::ZERO, 2);
        (app, root)
    }
    fn aura(world: &World, root: Entity) -> Aura {
        world.get::<Rig>(root).unwrap().aura
    }
    fn shown(world: &World, e: Entity) -> bool {
        world.get::<Visibility>(e).is_some_and(|v| *v != Visibility::Hidden)
    }
    /// What shows of a hotdog's stim: whether it does at all, how many arcs, whether its light is on, and which step of
    /// brightness its coat is drawn in (1 to STEPS; 0 when nothing shows).
    fn seen(world: &World, aura: &Aura) -> (bool, usize, bool, usize) {
        if !shown(world, aura.root) {
            return (false, 0, false, 0);
        }
        let glow = &world.resource::<PlayerModels>().stim.veil;
        let coat = &world.get::<MeshMaterial3d<StandardMaterial>>(aura.coat).unwrap().0;
        let step = glow.iter().position(|g| g == coat).unwrap() + 1;
        (true, aura.arcs.iter().filter(|e| shown(world, **e)).count(), shown(world, aura.light), step)
    }
    /// Where an entity is in the world, from its own place and its parents'.
    fn global(world: &World, mut e: Entity) -> Transform {
        let mut tr = *world.get::<Transform>(e).unwrap();
        while let Some(parent) = world.get::<ChildOf>(e) {
            e = parent.parent();
            tr = world.get::<Transform>(e).unwrap().mul_transform(tr);
        }
        tr
    }

    #[test]
    fn the_stim_shows_only_while_it_lasts_and_only_on_the_living() {
        let (mut app, root) = hotdog(Level::High);
        let world = app.world_mut();
        let aura = aura(world, root);
        let mut time = 1.;
        let mut step = |world: &mut World, stim: f32, alive: bool| {
            time += 1. / 60.;
            animate(world, root, time, Cues { alive, stim, ..default() });
            seen(world, &aura)
        };
        let nothing = (false, 0, false, 0);
        for _ in 0..30 {
            assert_eq!(step(world, 0., true), nothing);
        }
        assert!(aura.sleeves.iter().all(|e| !shown(world, *e)));
        // Stimmed: the coat, the skin, every sleeve and the light, once it has come up.
        for _ in 0..60 {
            step(world, 5., true);
        }
        let (on, _, light, _) = seen(world, &aura);
        assert!(on && light && shown(world, aura.coat) && shown(world, aura.skin));
        assert!(aura.sleeves.iter().all(|e| shown(world, *e)));
        // The stim over: all of it goes, the sleeves with it.
        assert_eq!(step(world, 0., true), nothing);
        assert!(aura.sleeves.iter().all(|e| !shown(world, *e)));
        // Fallen while stimmed: it goes too.
        for _ in 0..60 {
            step(world, 3., true);
        }
        assert!(step(world, 3., true).0);
        assert_eq!(step(world, 3., false), nothing);
        assert!(aura.sleeves.iter().all(|e| !shown(world, *e)));
    }

    #[test]
    fn the_stim_fades_in_as_it_starts_and_out_over_its_last_half_second() {
        assert_eq!((strength(0., 3.), strength(-1., 3.)), (0., 0.), "no stim, nothing");
        assert_eq!(strength(6., 0.), 0., "nothing the moment it starts");
        assert_eq!((strength(3., FADE_IN), strength(FADE_OUT, 2.)), (1., 1.));
        // Up over the first FADE_IN seconds and down over the last FADE_OUT, smoothly.
        let (mut up, mut down) = (0., 1.);
        for k in 1..=20 {
            let (rise, fall) = (strength(6., FADE_IN * k as f32 / 20.), strength(FADE_OUT * (20 - k) as f32 / 20., 3.));
            assert!(rise >= up && rise - up < 0.2 && fall <= down && down - fall < 0.2);
            (up, down) = (rise, fall);
        }
        assert!(strength(0.25 * FADE_OUT, 3.) < 0.5 && strength(0.75 * FADE_OUT, 3.) > 0.5);
        // On a hotdog, through a whole stim of six seconds: dim as it starts, full in the middle, dim at its end.
        let (mut app, root) = hotdog(Level::High);
        let world = app.world_mut();
        let aura = aura(world, root);
        let mut time = 1.;
        animate(world, root, time, Cues { alive: true, ..default() });
        let mut steps = vec![];
        for k in 1..=360 {
            time += 1. / 60.;
            animate(world, root, time, Cues { alive: true, stim: 6. - k as f32 / 60., ..default() });
            steps.push(seen(world, &aura).3);
        }
        let most = |s: &[usize]| *s.iter().max().unwrap();
        let least = |s: &[usize]| *s.iter().min().unwrap();
        assert_eq!(steps[0], 0, "hidden the moment it starts");
        assert!(most(&steps[..6]) <= 2 && least(&steps[60..300]) >= STEPS - 2 && most(&steps[354..]) <= 2, "{steps:?}");
        assert!(steps[..21].windows(2).all(|w| w[1] + 2 >= w[0]), "it comes up without dropping back: {:?}", &steps[..21]);
        assert!(!seen(world, &aura).0, "gone when the stim is");
    }

    #[test]
    fn the_graphics_level_caps_the_arcs_and_lights_the_counter_only_at_high_and_medium() {
        assert_eq!(arcs(Level::High), ARCS);
        for pair in Level::ALL.windows(2) {
            assert!(arcs(pair[0]) >= arcs(pair[1]) && sparks(pair[0]) >= sparks(pair[1]));
        }
        assert!(arcs(Level::Lowest) >= 3 && arcs(Level::Low) <= ARCS / 2 && sparks(Level::High) == SPARKS);
        assert!(lit(Level::High) && lit(Level::Medium) && !lit(Level::Low) && !lit(Level::Lowest));
        for level in Level::ALL {
            let (mut app, root) = hotdog(level);
            let world = app.world_mut();
            let aura = aura(world, root);
            let mut most = 0;
            for k in 0..240 {
                animate(world, root, 1. + k as f32 / 60., Cues { alive: true, stim: 5., ..default() });
                let (on, arcs_shown, light, _) = seen(world, &aura);
                assert!(on && light == lit(level), "{level:?}");
                assert!(arcs_shown <= arcs(level), "{level:?}: {arcs_shown} arcs");
                most = most.max(arcs_shown);
            }
            assert!(most * 3 >= arcs(level) * 2, "{level:?}: at most {most} arcs at once");
        }
    }

    #[test]
    fn the_fire_covers_the_hotdog_from_the_top_of_its_head_to_its_feet() {
        let (mut app, root) = hotdog(Level::High);
        let world = app.world_mut();
        let aura = aura(world, root);
        for k in 0..60 {
            animate(world, root, 1. + k as f32 / 60., Cues { alive: true, stim: 5., ..default() });
        }
        let reach = |world: &World, e: Entity| {
            let mesh = world.resource::<Assets<Mesh>>().get(&world.get::<Mesh3d>(e).unwrap().0).unwrap();
            let tr = global(world, e);
            let ys: Vec<f32> = points(mesh).into_iter().map(|p| tr.transform_point(p).y).collect();
            (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max))
        };
        // The sausage runs from 0.023 m to the top of the head at 0.178 m; the feet stand on the ground.
        let (low, high) = reach(world, aura.coat);
        assert!(low < 0.03 && high > 0.19, "the coat runs from {low} to {high}");
        let (low, high) = reach(world, aura.skin);
        assert!(low < 0.02 && high > 0.18, "the skin runs from {low} to {high}");
        for shin in [aura.sleeves[1], aura.sleeves[3]] {
            let (low, high) = reach(world, shin);
            assert!(low < 0.006 && high > 0.012, "a shin's sleeve runs from {low} to {high}");
        }
        // Every frame of the coat, the crown included.
        for f in 0..FRAMES {
            let ys: Vec<f32> = points(&coat(f, 8, 12)).iter().map(|p| p.y).collect();
            assert!(ys.iter().any(|y| *y < 0.03) && ys.iter().any(|y| *y > 0.19), "frame {f}");
        }
        // One hotdog's stim is a handful of entities: the root, the coat, the skin, the arcs, the sleeves and the light.
        let mut count = 0;
        let mut queue = vec![aura.root];
        while let Some(e) = queue.pop() {
            count += 1;
            queue.extend(world.get::<Children>(e).map(|c| c.iter().collect::<Vec<_>>()).unwrap_or_default());
        }
        assert_eq!(count + aura.sleeves.len(), 4 + ARCS + 8);
    }

    #[test]
    fn first_person_fire_frames_the_view_and_keeps_its_middle_clear() {
        let flames: Vec<Vec<Vec3>> = (0..FRAMES).map(|f| points(&lick(f))).collect();
        let bolts: Vec<Vec<Vec3>> = (0..SHAPES as u32).map(|k| points(&arc(k))).collect();
        let high = (crate::viewmodel::FOV * 0.5).tan();
        // Where a point in the camera's frame is seen: across and up, as the tangent of the angle off the line of sight.
        let off = |p: Vec3| Vec2::new(p.x, p.y) / -p.z;
        for level in Level::ALL {
            for aspect in [0.5, 0.6, 0.75, 1., 4. / 3., 16. / 9., 2.4] {
                let places = licks(level, aspect);
                assert!(places.len() <= LICKS);
                let wide = high * aspect;
                let (mut lowest, mut left, mut right) = (f32::MAX, f32::MAX, f32::MIN);
                for k in 0..90 {
                    let now = 3. + k as f32 * 0.037;
                    for (i, place) in places.iter().enumerate() {
                        let (frame, tr) = lick_pose(*place, i, now);
                        for p in &flames[frame] {
                            let q = off(tr.transform_point(*p));
                            assert!(q.length() >= CLEAR, "{level:?} at {aspect}: flame {i} reaches {q} of the middle");
                            (lowest, left, right) = (lowest.min(q.y), left.min(q.x), right.max(q.x));
                        }
                    }
                    for k in 0..sparks(level) {
                        if let Some((shape, tr)) = spark(k, now) {
                            for p in &bolts[shape] {
                                let q = off(tr.transform_point(*p));
                                assert!(q.length() >= CLEAR && q.x > 0. && q.y < 0., "an arc reaches {q}: off the hands and the gun");
                            }
                        }
                    }
                }
                // The flames rise from below the bottom edge and come in from beyond both sides.
                assert!(lowest < -high && left < -wide && right > wide, "{level:?} at {aspect}: {lowest} {left} {right}");
            }
        }
    }
}

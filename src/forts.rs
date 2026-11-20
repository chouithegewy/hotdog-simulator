// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Forts, the team mode, as the client shows it. The rules are the simulation's (`kitchen_sim::forts`, and
//! the server plays them with the same code); nothing here decides anything.
//!
//! * [`build`] puts out what a Forts map marks: each team's base as a pad in its colour, the forcefield
//!   as a wall of blue glass across the counter, and the bacon. `level::build` calls it.
//! * [`update`] runs inside `arena::tick` once the round has been stepped (or the server's snapshot taken):
//!   the forcefield shimmers and, when the simulation drops it, sinks into the counter with a thud; the
//!   bacon turns on its spot, lies where it was dropped, or rides on its carrier's back; every hotdog
//!   wears its team's colour
//!   (its legs, its badge and a band round its head). The score, the clock and the way to the bacon are the
//!   head-up display's (hud.rs). Each hotdog's name and health float over its head in its team's colour (tags.rs).
//! * A dead player watches its own body for a few seconds and then a living team mate, the next at a click
//!   (or Space): that is the death camera's (deathcam.rs, `WAKE`), and the head-up display shows whom it watches.
use crate::arena::Game;
use crate::level::MapPart;
use crate::player_model::{Gait, Livery, PlayerBody, Rig};
use bevy::prelude::*;
use kitchen_sim::forts::{FortState, DROPPED, HELD};
use kitchen_sim::map::Map;

const FLOOR: f32 = 0.9025;
/// The teams' colours: mustard yellow and ketchup red.
pub const TEAM_RGB: [[f32; 3]; 2] = [[0.95, 0.74, 0.10], [0.86, 0.12, 0.08]];
/// How tall the forcefield is drawn (the simulation's stands as high as a block may), and the seconds it
/// takes to sink once it has dropped.
const FIELD_HEIGHT: f32 = 0.62;
const SINK: f32 = 0.9;

/// What `build` put out, and what `update` remembers from frame to frame.
#[derive(Resource, Default)]
pub struct Scene {
    field: Option<Entity>,
    bands: Vec<Entity>,
    glass: Option<Handle<StandardMaterial>>,
    bacon: Option<Entity>,
    beacon: Option<Entity>,
    /// Whether the forcefield was up on the last frame, and when it was seen to drop.
    up: bool,
    dropped: Option<f32>,
    cube: Option<Handle<Mesh>>,
    band: Option<Handle<Mesh>>,
    team: Option<[Handle<StandardMaterial>; 2]>,
    thud: Option<Handle<AudioSource>>,
    ding: Option<Handle<AudioSource>>,
}
/// A hotdog dressed for a team: which one.
#[derive(Component)]
struct Dressed(u8);
#[derive(Component)]
struct Headband;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Scene>();
    }
}

/// The meshes, materials and sounds everything here shares, made once.
fn kit(world: &mut World) {
    if world.resource::<Scene>().cube.is_some() {
        return;
    }
    let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(1., 1., 1.));
    let band = world.resource_mut::<Assets<Mesh>>().add(Cylinder::new(1., 1.));
    let team = [0, 1].map(|t| {
        let c = TEAM_RGB[t];
        world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color: Color::srgb(c[0], c[1], c[2]),
            emissive: LinearRgba::rgb(c[0] * 0.25, c[1] * 0.25, c[2] * 0.25),
            perceptual_roughness: 0.45,
            ..default()
        })
    });
    let sounds = world.get_resource::<AssetServer>().map(|server| (server.load("audio/boom.wav"), server.load("audio/ding.wav")));
    let mut scene = world.resource_mut::<Scene>();
    scene.cube = Some(cube);
    scene.band = Some(band);
    scene.team = Some(team);
    if let Some((thud, ding)) = sounds {
        (scene.thud, scene.ding) = (Some(thud), Some(ding));
    }
}
/// A flat colour that shines by itself and can be seen through.
fn glow(world: &mut World, c: [f32; 3], alpha: f32, shine: f32) -> Handle<StandardMaterial> {
    world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: Color::srgba(c[0], c[1], c[2], alpha),
        emissive: LinearRgba::rgb(c[0] * shine, c[1] * shine, c[2] * shine),
        alpha_mode: if alpha < 1. { AlphaMode::Blend } else { AlphaMode::Opaque },
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    })
}

/// A solid colour that is lit like everything else on the counter, with a little shine of its own so that it reads in shadow.
fn paint(world: &mut World, c: [f32; 3]) -> Handle<StandardMaterial> {
    world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: Color::srgb(c[0], c[1], c[2]),
        emissive: LinearRgba::rgb(c[0] * 0.16, c[1] * 0.16, c[2] * 0.16),
        perceptual_roughness: 0.5,
        ..default()
    })
}

/// Puts out a Forts map's bases, forcefield and bacon. They are `MapPart`s, so the next map replaces them.
pub fn build(world: &mut World, map: &Map) {
    world.init_resource::<Scene>();
    {
        let mut scene = world.resource_mut::<Scene>();
        (scene.field, scene.bacon, scene.beacon, scene.glass, scene.up, scene.dropped) = (None, None, None, None, false, None);
        scene.bands.clear();
    }
    let Some(fort) = map.fortress() else {
        return;
    };
    kit(world);
    let cube = world.resource::<Scene>().cube.clone().unwrap();
    let solid = |world: &mut World, parent: Option<Entity>, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3| {
        let e = world
            .spawn((Mesh3d(cube.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(at).with_scale(size), bevy::light::NotShadowCaster))
            .id();
        match parent {
            Some(parent) => {
                world.entity_mut(parent).add_child(e);
            }
            None => {
                world.entity_mut(e).insert(MapPart);
            }
        }
        e
    };
    // Each base: a pad in the team's colour, edged brighter.
    for (t, rect) in fort.bases.iter().enumerate() {
        let [x0, z0, x1, z1] = rect.map(|v| v as f32);
        let (pad, edge) = (glow(world, TEAM_RGB[t], 0.42, 0.5), glow(world, TEAM_RGB[t], 0.9, 0.9));
        let (mid, size) = (Vec3::new((x0 + x1) / 2., FLOOR + 0.003, (z0 + z1) / 2.), Vec3::new(x1 - x0, 0.003, z1 - z0));
        solid(world, None, &pad, mid, size);
        for (at, size) in [
            (Vec3::new(mid.x, FLOOR + 0.005, z0 + 0.008), Vec3::new(size.x, 0.005, 0.016)),
            (Vec3::new(mid.x, FLOOR + 0.005, z1 - 0.008), Vec3::new(size.x, 0.005, 0.016)),
            (Vec3::new(x0 + 0.008, FLOOR + 0.005, mid.z), Vec3::new(0.016, 0.005, size.z)),
            (Vec3::new(x1 - 0.008, FLOOR + 0.005, mid.z), Vec3::new(0.016, 0.005, size.z)),
        ] {
            solid(world, None, &edge, at, size);
        }
    }
    // The forcefield: blue glass with brighter bands that climb it.
    let [x0, z0, x1, z1] = fort.field.map(|v| v as f32);
    let glass = glow(world, [0.20, 0.55, 1.0], 0.30, 0.9);
    let bright = glow(world, [0.55, 0.85, 1.0], 0.45, 1.2);
    let field = world
        .spawn((
            MapPart,
            Name::new("Forcefield"),
            Transform::from_xyz((x0 + x1) / 2., FLOOR + FIELD_HEIGHT / 2., (z0 + z1) / 2.).with_scale(Vec3::new((x1 - x0).max(0.02), FIELD_HEIGHT, (z1 - z0).max(0.02))),
            Visibility::Hidden,
        ))
        .id();
    solid(world, Some(field), &glass, Vec3::ZERO, Vec3::ONE);
    let bands: Vec<Entity> = (0..5).map(|_| solid(world, Some(field), &bright, Vec3::ZERO, Vec3::new(1.06, 0.02, 1.001))).collect();
    // The bacon: two rashers, streaked with fat, and a thin column of light over it wherever it lies.
    let level = kitchen_sim::map::Level::new(map.clone());
    let spot = level.bacon_position();
    let bacon = world
        .spawn((MapPart, Name::new("Bacon"), Transform::from_xyz(spot[0] as f32, spot[1] as f32, spot[2] as f32), Visibility::default()))
        .id();
    let (meat, dark, fat) = (paint(world, [0.93, 0.17, 0.12]), paint(world, [0.66, 0.07, 0.06]), paint(world, [1.0, 0.84, 0.58]));
    // Its plate stays on the spot.
    let china = paint(world, [0.97, 0.97, 0.94]);
    let band_mesh = world.resource::<Scene>().band.clone().unwrap();
    world.spawn((MapPart, Mesh3d(band_mesh), MeshMaterial3d(china), Transform::from_xyz(spot[0] as f32, spot[1] as f32 + 0.003, spot[2] as f32).with_scale(Vec3::new(0.085, 0.006, 0.085)), bevy::light::NotShadowCaster));
    for (rasher, (lift, turn)) in [(0., -0.22_f32), (0.011, 0.25)].into_iter().enumerate() {
        let root = world.spawn((Transform::from_xyz(0., lift, 0.).with_rotation(Quat::from_rotation_y(turn)), Visibility::default())).id();
        world.entity_mut(bacon).add_child(root);
        for i in 0..7 {
            // Seven short lengths along the rasher, up and down like a fried one.
            let (x, wave) = ((i as f32 - 3.) * 0.022, if (i + rasher) % 2 == 0 { 0.004 } else { -0.003 });
            for (z, width, material) in [(-0.017, 0.016, &meat), (-0.004, 0.010, &fat), (0.007, 0.012, &dark), (0.019, 0.012, &fat)] {
                solid(world, Some(root), material, Vec3::new(x, wave, z), Vec3::new(0.0225, 0.006, width));
            }
        }
    }
    let light = glow(world, [1.0, 0.62, 0.25], 0.42, 1.2);
    let beacon = solid(world, None, &light, Vec3::new(spot[0] as f32, spot[1] as f32 + 0.45, spot[2] as f32), Vec3::new(0.016, 0.7, 0.016));
    let mut scene = world.resource_mut::<Scene>();
    (scene.field, scene.bands, scene.glass, scene.bacon, scene.beacon) = (Some(field), bands, Some(glass), Some(bacon), Some(beacon));
}

/// How fast this player walks under what it carries, as a share of its pace (the simulation's `burden`,
/// worked out from the roster so that it holds online as well).
pub fn burden(g: &Game) -> f32 {
    // The heavier what it carries, the slower it walks (kitchen_sim::feast::burden_of, as the simulation has it).
    let carried = g.roster.iter().find(|p| p.id == g.me()).map_or(-1, |p| p.carry);
    if carried >= 0 && (carried as usize) < g.feast.props.len() {
        kitchen_sim::feast::burden_of(g.feast.props.shape(g.feast.props.body(carried as usize).shape).mass)
    } else if g.me() != 0 && g.fort.as_ref().is_some_and(|fort| fort.bacon.holder == g.me()) {
        // (The bacon slows its carrier too: kitchen_sim::forts::BACON_PACE.)
        kitchen_sim::forts::BACON_PACE
    } else {
        1.
    }
}

/// Every frame of a Forts round, after the round has been stepped.
pub fn update(world: &mut World, g: &mut Game, now: f32) {
    let shown = g.fort.is_some() && g.playing();
    let Some(state) = g.fort.clone().filter(|_| shown) else {
        // Free For All on a Forts map leaves the forcefield and the bacon out (kitchen_sim::rules).
        if g.feast.rules.ffa {
            let parts: Vec<Entity> = world.get_resource::<Scene>().map(|s| [s.field, s.bacon, s.beacon].into_iter().flatten().collect()).unwrap_or_default();
            for part in parts {
                if let Ok(mut part) = world.get_entity_mut(part) {
                    part.insert(Visibility::Hidden);
                }
            }
        }
        return;
    };
    kit(world);
    world.resource_scope(|world, mut scene: Mut<Scene>| {
        field(world, &mut scene, &state, now);
        bacon(world, &scene, g, &state, now);
        dress(world, &scene, g);
    });
    publish(g, &state);
}

/// The forcefield stands while the simulation has it up, and sinks into the counter when it drops.
fn field(world: &mut World, scene: &mut Scene, state: &FortState, now: f32) {
    if scene.up && !state.field {
        scene.dropped = Some(now);
        for sound in [scene.thud.clone(), scene.ding.clone()].into_iter().flatten() {
            world.spawn((AudioPlayer::new(sound), PlaybackSettings::DESPAWN));
        }
    }
    scene.up = state.field;
    let stand = if state.field { 1. } else { scene.dropped.map_or(0., |t| 1. - ((now - t) / SINK).clamp(0., 1.)) };
    let Some(field) = scene.field else {
        return;
    };
    // (The wall, its visibility and its glass are written only when they change: a write has the engine place the wall,
    // work out its visibility, or prepare its material and the meshes drawn with it, again.)
    if let Some(mut tr) = world.get_mut::<Transform>(field) {
        let (height, y) = ((FIELD_HEIGHT * stand).max(0.001), FLOOR + FIELD_HEIGHT * stand / 2.);
        if tr.scale.y != height || tr.translation.y != y {
            tr.scale.y = height;
            tr.translation.y = y;
        }
    }
    if let Some(mut shown) = world.get_mut::<Visibility>(field) {
        shown.set_if_neq(if stand > 0. { Visibility::Visible } else { Visibility::Hidden });
    }
    for (i, band) in scene.bands.iter().enumerate() {
        if let Some(mut tr) = world.get_mut::<Transform>(*band) {
            tr.translation.y = (now * 0.22 + i as f32 / 5.).fract() - 0.5;
        }
    }
    if let Some(glass) = &scene.glass {
        // The glass's shimmer. At Low and Lowest it moves in steps of a hundredth of its opacity (too fine to see), so the
        // material is prepared again a few times a second rather than every frame.
        let mut alpha = (0.27 + 0.05 * (now * 2.6).sin()) * stand.sqrt();
        if world.get_resource::<crate::quality::Detail>().is_some_and(|d| d.lean) {
            alpha = (alpha * 100.).round() / 100.;
        }
        let colour = Color::srgba(0.20, 0.55, 1.0, alpha);
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        if materials.get(glass).is_some_and(|m| m.base_color != colour) {
            if let Some(mut material) = materials.get_mut(glass) {
                material.base_color = colour;
            }
        }
    }
}

/// The bacon: turning over its spot or where it was dropped, or upright on its carrier's back. The player
/// who carries it does not see it in first person; the head-up display says so instead.
fn bacon(world: &mut World, scene: &Scene, g: &Game, state: &FortState, now: f32) {
    let (Some(bacon), Some(beacon)) = (scene.bacon, scene.beacon) else {
        return;
    };
    let holder = g.roster.iter().find(|p| state.bacon.state == HELD && p.id == state.bacon.holder);
    let own = holder.is_some_and(|p| p.id == g.me() && p.alive && !g.demo);
    let at = Vec3::from_array(state.bacon.p);
    let pose = match holder {
        Some(p) => {
            let (at, turn) = if p.id == g.me() && !g.network { (Vec3::from_array(p.p), p.yaw) } else if p.id == g.me() { (g.position, g.yaw) } else { (Vec3::from_array(p.p), p.yaw) };
            let turn = Quat::from_rotation_y(turn);
            Transform::from_translation(at + turn * Vec3::new(0., 0.115, 0.042)).with_rotation(turn * Quat::from_rotation_x(0.12) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)).with_scale(Vec3::splat(1.45))
        }
        None => Transform::from_translation(at + Vec3::Y * (0.075 + (now * 2.2).sin() * 0.01))
            .with_rotation(Quat::from_rotation_y(now * 1.1) * Quat::from_rotation_x(0.55))
            .with_scale(Vec3::splat(if state.bacon.state == DROPPED { 1.5 } else { 1.7 })),
    };
    if let Some(mut tr) = world.get_mut::<Transform>(bacon) {
        *tr = pose;
    }
    world.entity_mut(bacon).insert(if own { Visibility::Hidden } else { Visibility::Visible });
    if let Some(mut tr) = world.get_mut::<Transform>(beacon) {
        tr.translation = Vec3::new(pose.translation.x, at.y + 0.5, pose.translation.z);
    }
    world.entity_mut(beacon).insert(if own { Visibility::Hidden } else { Visibility::Visible });
}

/// Every hotdog wears its team's colour: its legs and its badge, and a band round its head.
fn dress(world: &mut World, scene: &Scene, g: &Game) {
    let (Some(team), Some(band)) = (scene.team.clone(), scene.band.clone()) else {
        return;
    };
    let bodies: Vec<(Entity, u32, Option<u8>)> = world
        .query_filtered::<(Entity, &Gait, Option<&Dressed>), With<PlayerBody>>()
        .iter(world)
        .map(|(entity, gait, dressed)| (entity, gait.id(), dressed.map(|d| d.0)))
        .collect();
    for (entity, id, dressed) in bodies {
        let Some(side) = g.roster.iter().find(|p| p.id == id).map(|p| p.team.min(1)) else {
            continue;
        };
        if dressed == Some(side) {
            continue;
        }
        let material = team[side as usize].clone();
        let parts = world.get::<Livery>(entity).map(|livery| livery.0.clone()).unwrap_or_default();
        for part in parts {
            if let Ok(mut part) = world.get_entity_mut(part) {
                part.insert(MeshMaterial3d(material.clone()));
            }
        }
        // (Worn on the head, which nods on the torso: player_model/bend.rs.)
        let Some(torso) = world.get::<Rig>(entity).map(|rig| rig.joints().head) else {
            continue;
        };
        let old: Vec<Entity> = world.get::<Children>(torso).map(|children| children.iter().collect()).unwrap_or_default();
        for child in old {
            if world.get::<Headband>(child).is_some() {
                world.entity_mut(child).despawn();
            }
        }
        // A band round the head, and a block of the colour on a pole at its back, which is seen from any side and from far off.
        let Some(cube) = scene.cube.clone() else {
            continue;
        };
        for (mesh, at, size) in [
            // The band sits above the eyes, centred a little forward as the casing leans there, and clear all round of the
            // casing; its lower edge covers the brows (player_model.rs: BROW).
            (band.clone(), Vec3::new(0., 0.17, -0.001), Vec3::new(0.0236, 0.01, 0.0236)),
            (cube.clone(), Vec3::new(0., 0.185, 0.026), Vec3::new(0.003, 0.11, 0.003)),
            (cube, Vec3::new(0., 0.232, 0.026), Vec3::new(0.034, 0.024, 0.034)),
        ] {
            let part = world.spawn((Headband, Mesh3d(mesh), MeshMaterial3d(material.clone()), Transform::from_translation(at).with_scale(size))).id();
            world.entity_mut(torso).add_child(part);
        }
        world.entity_mut(entity).insert(Dressed(side));
    }
}

/// Checks only (`?check` in the page's address): `window.hotdogAim = [yaw, pitch, trigger]` aims this
/// player's hotdog and holds (1) or releases (0) its trigger, since a headless browser has no mouse to
/// capture; and every frame of a Forts round `window.hotdogForts` is {id, phase, position, yaw, elapsed,
/// alert, roster, forts}: this player's hotdog, where it stands, everybody, and the mode's state.
pub fn driven() -> Option<[f32; 3]> {
    #[cfg(target_arch = "wasm32")]
    {
        let v = check_aim();
        (v.len() >= 3).then(|| [v[0], v[1], v[2]])
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}
fn publish(g: &Game, state: &FortState) {
    #[cfg(target_arch = "wasm32")]
    if check_on() {
        check_forts(&serde_json::json!({"id": g.me(), "phase": g.phase_name(), "position": g.position.to_array(), "yaw": g.yaw, "elapsed": g.elapsed, "alert": g.alert, "roster": g.roster, "forts": state}).to_string());
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (g, state);
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
const checking=new URLSearchParams(location.search).has('check');
export function check_aim(){const v=checking?window.hotdogAim:null;return new Float32Array(Array.isArray(v)&&v.length>=3&&v.every(Number.isFinite)?v:[])}
export function check_on(){return checking}
export function check_forts(state){if(checking)window.hotdogForts=JSON.parse(state)}
")]
extern "C" {
    fn check_on() -> bool;
    fn check_aim() -> Vec<f32>;
    fn check_forts(state: &str);
}

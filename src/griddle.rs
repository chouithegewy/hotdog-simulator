// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The electric griddle as it is seen, heard and switched. (Its rules are kitchen_sim's: `griddle`: the plate heats and
//! cools, a hotdog standing on it burns, and anyone within reach of its dial presses use to flip it.)
//!
//! * **The appliance** (`build`, called by level.rs for a block of the style `griddle`): a 0.50 by 0.28 m body on four short
//!   feet, a grey housing, a dark non-stick plate inside a raised rim, a grease channel and a drip tray along the front, two
//!   cool handles, the thermostat dial and its indicator light on the front (the `facing` side, in the middle) and a cord with
//!   its plug out of the back. The dial turns to its setting with the switch, and the light is bright while the griddle is on.
//! * **Heat.** As the plate warms it glows: a dull red along its edges, and over it a faint warmth (`glow`); above
//!   SMOKE_AT degrees wisps of smoke rise from it, more the hotter it is; and where a hotdog stands on it hot enough to
//!   burn, grease spits from under its feet (sparks) and a sizzle plays (sizzle.wav, a loop, louder the hotter and the nearer).
//!   A click sounds whenever a griddle is flipped, by anyone.
//! * **The readout** floats over each griddle in the world: a thermometer, the temperature in whole degrees with a drawn degree
//!   mark and a C (the font has no degree sign), and a dot for whether it is on, coloured from cool blue through orange to red
//!   with the heat ([`heat_rgb`]). It is a tag of the screen's like the hotdogs' (tags.rs): it keeps its place over the griddle,
//!   faces the viewer, grows with the distance so that it can be read across the field ([`size`]), and fades behind cover and far away.
//! * **The prompt** by the crosshair (hud.rs) says that use switches the griddle, while one is within reach of the dial and faced.
//! * **Online** the server's `{type:'griddle',griddles}` sets the states (`hear`) and this client carries each temperature on
//!   between by the same law (`kitchen_sim::griddle::Griddle::advance`); a press is sent as `{type:'griddle'}` (`take_presses`,
//!   network.rs) and the client flips its own copy meanwhile, to be corrected by the server's word.
use crate::arena::Game;
use crate::icons::{Icon, Icons};
use crate::level::MapPart;
use crate::maya::HotdogCamera;
use bevy::{
    audio::{AudioSinkPlayback, Volume},
    light::NotShadowCaster,
    prelude::*,
};
use kitchen_sim::griddle::{self, Plate};
use kitchen_sim::map::{Block, Side};
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// The plate's temperature (degrees Celsius) from which smoke rises, and where it glows from.
pub const SMOKE_AT: f32 = 120.;
pub const GLOW_AT: f32 = 55.;
/// Wisps of smoke each griddle has, and sparks of spitting grease.
const WISPS: usize = 8;
const SPARKS: usize = 18;
/// The readout floats this high over the plate's top, and is readable out to FADE metres from the eye, gone by FAR.
const LIFT: f32 = 0.17;
const FADE: f32 = 6.5;
const FAR: f32 = 9.;
/// Pixels high the readout's digits are drawn at 1.2 m, and the least and the most, at a 720-pixel window.
const BASE_PX: f32 = 24.;
const BASE_AT: f32 = 1.2;
const MIN_PX: f32 = 18.;
const MAX_PX: f32 = 36.;
/// A readout this close in front of the eye, or behind it, is not drawn.
const NEAR: f32 = 0.06;
/// The foot of the head-up display's top line (its score and clock), at a 720-pixel window: a readout that would stand higher keeps
/// just under it. (As the hotdogs' tags' CEILING.)
const CEILING: f32 = 66.;
/// How fast a readout fades in or out.
const BLINK: f32 = 5.;
/// The dial's pointer: its angle off and on (radians, about the dial's axis, 0 straight up), and how fast it turns.
const OFF_ANGLE: f32 = -2.35;
const ON_ANGLE: f32 = 1.1;

// ----- What is shown: worked out from the temperature, without the screen.

/// The whole number of degrees the readout says: the temperature to the nearest degree, never below 0.
pub fn digits(temp: f32) -> String {
    format!("{}", temp.max(0.).round() as i32)
}
/// The readout as one string, with the degree sign it cannot draw: "187°C". (The screen draws the mark itself; this is what it
/// says, for the tests and the entity's name.)
#[cfg_attr(not(test), allow(dead_code))]
pub fn describe(temp: f32) -> String {
    format!("{}\u{b0}C", digits(temp))
}
/// The colour of a temperature: cool blue at room temperature, white by 60, warm yellow by 100, orange by 150 and red at the
/// griddle's hottest, 205; between them a straight line from one to the next.
pub fn heat_rgb(temp: f32) -> [f32; 3] {
    const STOPS: [(f32, [f32; 3]); 5] =
        [(21., [0.60, 0.80, 1.00]), (60., [0.92, 0.96, 1.00]), (100., [1.00, 0.86, 0.42]), (150., [1.00, 0.55, 0.14]), (205., [1.00, 0.2, 0.12])];
    if temp <= STOPS[0].0 {
        return STOPS[0].1;
    }
    for pair in STOPS.windows(2) {
        let ((t0, a), (t1, b)) = (pair[0], pair[1]);
        if temp <= t1 {
            let u = (temp - t0) / (t1 - t0);
            return [0, 1, 2].map(|i| a[i] * (1. - u) + b[i] * u);
        }
    }
    STOPS[4].1
}
/// How much of its glow a plate at `temp` has, 0 to 1: nothing under GLOW_AT, all of it at the top, eased in between.
pub fn glow(temp: f32) -> f32 {
    let t = ((temp - GLOW_AT) / (griddle::HOT - GLOW_AT)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
/// How much smoke a plate at `temp` makes, 0 to 1: none up to SMOKE_AT, then a quarter of it at once, all of it at the top.
pub fn smoke(temp: f32) -> f32 {
    if temp <= SMOKE_AT {
        return 0.;
    }
    0.25 + 0.75 * ((temp - SMOKE_AT) / (griddle::HOT - SMOKE_AT)).clamp(0., 1.)
}
/// The pixels the readout's digits are, for a griddle `distance` metres from the eye, at a 720-pixel window: larger
/// nearer and smaller further, between MIN_PX and MAX_PX, in whole even pixels.
pub fn size(distance: f32) -> f32 {
    ((BASE_PX * (BASE_AT / distance.max(0.05)).powf(0.45)).clamp(MIN_PX, MAX_PX) / 2.).round() * 2.
}
/// How much of the readout shows at `distance` metres from the eye: all of it out to FADE, none beyond FAR.
pub fn reach(distance: f32) -> f32 {
    (1. - (distance - FADE) / (FAR - FADE)).clamp(0., 1.)
}
/// The grey-white of smoke, a little warmer the hotter the plate.
fn wisp_colour(heat: f32) -> [f32; 3] {
    [0.78 + 0.1 * heat, 0.78 + 0.04 * heat, 0.78]
}

// ----- The scenery: the appliance, and what animates on it.

/// Unit meshes, and the materials that do not change.
#[derive(Resource, Clone)]
struct Kit {
    cube: Handle<Mesh>,
    sphere: Handle<Mesh>,
    round: Handle<Mesh>,
    housing: Handle<StandardMaterial>,
    tray: Handle<StandardMaterial>,
    rim: Handle<StandardMaterial>,
    black: Handle<StandardMaterial>,
    chrome: Handle<StandardMaterial>,
    cap: Handle<StandardMaterial>,
}
impl FromWorld for Kit {
    fn from_world(world: &mut World) -> Self {
        let (cube, sphere, round) = {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            (meshes.add(Cuboid::default()), meshes.add(Sphere::new(1.).mesh().uv(14, 9)), meshes.add(Cylinder::new(1., 1.).mesh().resolution(20)))
        };
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        let mut plain = |r: f32, g: f32, b: f32, roughness: f32, metallic: f32| {
            mats.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: roughness, metallic, ..default() })
        };
        Kit {
            cube,
            sphere,
            round,
            housing: plain(0.50, 0.52, 0.53, 0.42, 0.25),
            tray: plain(0.66, 0.68, 0.69, 0.34, 0.3),
            rim: plain(0.58, 0.60, 0.61, 0.36, 0.3),
            black: plain(0.025, 0.027, 0.03, 0.55, 0.),
            chrome: plain(0.78, 0.80, 0.82, 0.3, 0.35),
            cap: plain(0.08, 0.085, 0.09, 0.4, 0.1),
        }
    }
}

/// A griddle's root, with the block it is and the materials that change with its heat.
#[derive(Component)]
struct Unit {
    block: usize,
    plate: Handle<StandardMaterial>,
    glow: [Handle<StandardMaterial>; 2],
    lamp: Handle<StandardMaterial>,
    smoke: Handle<StandardMaterial>,
    sparks: Handle<StandardMaterial>,
}
/// The dial's pivot, and the angle its pointer has turned to.
#[derive(Component)]
struct Dial {
    block: usize,
    angle: f32,
}
/// A wisp of smoke (the griddle's block and its number), a child of the griddle's root; the height its plate stands at.
#[derive(Component)]
struct Wisp {
    block: usize,
    k: usize,
}
/// A spark of grease: where it is in the world is set from the hotdog it spits under.
#[derive(Component)]
struct Spark {
    block: usize,
    k: usize,
}

fn unlit(mats: &mut Assets<StandardMaterial>, color: Color, mode: AlphaMode) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial { base_color: color, unlit: true, alpha_mode: mode, fog_enabled: false, ..default() })
}

/// Builds the griddle of `block` (the map's block number `index`, with its footprint `rect` and the side `side` its front, the
/// dial's, is on).
pub fn build(world: &mut World, index: usize, block: &Block, rect: [f32; 4], side: Side) {
    world.init_resource::<Kit>();
    let kit = world.resource::<Kit>().clone();
    let (transform, w, d) = crate::level::facing(rect, side);
    // Designed for 0.06 m; another height scales it up and down.
    let k = block.height as f32 / 0.06;
    let (plate, glow_outer, glow_inner, lamp, smoke, sparks) = {
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        (
            mats.add(StandardMaterial { base_color: Color::srgb(0.03, 0.032, 0.036), perceptual_roughness: 0.72, reflectance: 0.12, metallic: 0.02, ..default() }),
            unlit(&mut mats, Color::BLACK, AlphaMode::Add),
            unlit(&mut mats, Color::BLACK, AlphaMode::Add),
            unlit(&mut mats, Color::srgb(0.2, 0.06, 0.03), AlphaMode::Opaque),
            unlit(&mut mats, Color::srgba(0.8, 0.8, 0.8, 0.), AlphaMode::Blend),
            unlit(&mut mats, Color::srgb(1., 0.85, 0.45), AlphaMode::Add),
        )
    };
    let root = world
        .spawn((
            MapPart,
            Name::new("Electric griddle"),
            transform,
            Visibility::default(),
            Unit { block: index, plate: plate.clone(), glow: [glow_outer.clone(), glow_inner.clone()], lamp: lamp.clone(), smoke: smoke.clone(), sparks: sparks.clone() },
        ))
        .id();
    // (Every part made here stands still: the plate, glow and lamp change their materials, not their meshes; `bake`. The
    // dial's pivot, the wisps and the sparks move, and are made apart.)
    let part = |world: &mut World, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Vec3, scale: Vec3, rotation: Quat, shadow: bool| {
        let e = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform { translation: at, rotation, scale }, crate::bake::Still)).id();
        if !shadow {
            world.entity_mut(e).insert(NotShadowCaster);
        }
        world.entity_mut(root).add_child(e);
        e
    };
    let cube = |world: &mut World, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3, shadow: bool| part(world, &kit.cube, material, at, size, Quat::IDENTITY, shadow);
    // A rod from `a` to `b`.
    let rod = |world: &mut World, material: &Handle<StandardMaterial>, a: Vec3, b: Vec3, r: f32| {
        let (delta, mid) = (b - a, (a + b) / 2.);
        part(world, &kit.round, material, mid, Vec3::new(r, delta.length(), r), Quat::from_rotation_arc(Vec3::Y, delta.normalize_or(Vec3::Y)), false)
    };
    let ball = |world: &mut World, material: &Handle<StandardMaterial>, at: Vec3, r: f32| part(world, &kit.sphere, material, at, Vec3::splat(r), Quat::IDENTITY, false);
    let front = -d / 2.;
    // Four short feet, the grey housing, and the plate in its tray with a raised rim along its sides and back; the front's is lower, so that
    // the grease runs down to the channel there.
    let (foot, body, base) = (0.012 * k, 0.040 * k, 0.052 * k);
    for (sx, sz) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
        part(world, &kit.round, &kit.black, Vec3::new(sx * (w / 2. - 0.04), foot / 2., sz * (d / 2. - 0.035)), Vec3::new(0.012, foot, 0.012), Quat::IDENTITY, false);
    }
    cube(world, &kit.housing, Vec3::new(0., (foot + body) / 2., 0.), Vec3::new(w - 0.008, body - foot, d - 0.008), true);
    cube(world, &plate, Vec3::new(0., (body + base) / 2., 0.), Vec3::new(w - 0.018, base - body, d - 0.02), true);
    let (rim_h, edge) = (0.008 * k, 0.009);
    for sx in [-1., 1.] {
        cube(world, &kit.rim, Vec3::new(sx * (w / 2. - 0.009 - edge / 2.), base + rim_h / 2., 0.), Vec3::new(edge, rim_h, d - 0.02), true);
    }
    cube(world, &kit.rim, Vec3::new(0., base + rim_h / 2., d / 2. - 0.01 - edge / 2.), Vec3::new(w - 0.018, rim_h, edge), true);
    cube(world, &kit.rim, Vec3::new(0., base + 0.0015 * k, front + 0.01 + edge / 2.), Vec3::new(w - 0.018, 0.003 * k, edge), false);
    // The grease channel along the front of the cooking surface.
    cube(world, &kit.black, Vec3::new(0., base + 0.0004, front + 0.024), Vec3::new(w - 0.05, 0.0008, 0.008), false);
    // The glow along the edges of the cooking surface, two frames of it (the outer the brighter), lit only as the plate heats.
    let (span_x, span_z) = (w - 0.05, d - 0.058);
    let y = base + 0.0009;
    for (m, inset, width) in [(&glow_outer, 0., 0.013), (&glow_inner, 0.013, 0.011)] {
        for sx in [-1., 1.] {
            cube(world, m, Vec3::new(sx * (span_x / 2. - inset - width / 2.), y + inset * 0.01, 0.), Vec3::new(width, 0.0006, span_z - 2. * inset), false);
        }
        for sz in [-1., 1.] {
            let (z, length) = (sz * (span_z / 2. - inset - width / 2.), span_x - 2. * inset - 2. * width);
            cube(world, m, Vec3::new(0., y + inset * 0.01, z), Vec3::new(length, 0.0006, width), false);
        }
    }
    // The drip tray at the front, left of the dial, a steel face with a bar for a handle; two cool handles, one at each end.
    cube(world, &kit.tray, Vec3::new(-0.14, 0.027 * k, front + 0.0085), Vec3::new(0.15, 0.016 * k, 0.017), false);
    rod(world, &kit.chrome, Vec3::new(-0.205, 0.027 * k, front + 0.004), Vec3::new(-0.075, 0.027 * k, front + 0.004), 0.0035);
    for sx in [-1., 1.] {
        cube(world, &kit.black, Vec3::new(sx * (w / 2. - 0.012), 0.026 * k, 0.), Vec3::new(0.024, 0.017 * k, 0.075), true);
    }
    // The control panel, a black plate on the housing's front, with the dial in the middle of the front, the indicator light by it, and marks round
    // the dial at off, in the middle and at its highest.
    let z = front + 0.004;
    cube(world, &kit.black, Vec3::new(0.034, 0.028 * k, z - 0.0012), Vec3::new(0.13, 0.026 * k, 0.0024), false);
    let (dy, dz) = (0.028 * k, z - 0.0024);
    part(world, &kit.round, &kit.chrome, Vec3::new(0., dy, dz - 0.0006), Vec3::new(0.0155, 0.0012, 0.0155), Quat::from_rotation_x(FRAC_PI_2), false);
    for angle in [OFF_ANGLE, 0., ON_ANGLE * 1.25] {
        let c = Vec3::new(angle.sin() * 0.02, dy + angle.cos() * 0.02 * k.min(1.), dz - 0.001);
        part(world, &kit.cube, &kit.chrome, c, Vec3::new(0.0025, 0.005, 0.0015), Quat::from_rotation_z(-angle), false);
    }
    let pivot = world.spawn((Dial { block: index, angle: OFF_ANGLE }, Transform::from_xyz(0., dy, dz - 0.0014), Visibility::default())).id();
    world.entity_mut(root).add_child(pivot);
    let cap = world.spawn((Mesh3d(kit.round.clone()), MeshMaterial3d(kit.cap.clone()), Transform::from_xyz(0., 0., -0.005).with_rotation(Quat::from_rotation_x(FRAC_PI_2)).with_scale(Vec3::new(0.0115, 0.0105, 0.0115)))).id();
    let pointer = world.spawn((Mesh3d(kit.cube.clone()), MeshMaterial3d(kit.chrome.clone()), Transform::from_xyz(0., 0.0058, -0.0108).with_scale(Vec3::new(0.0026, 0.0078, 0.0012)))).id();
    world.entity_mut(pivot).add_children(&[cap, pointer]);
    // The indicator light.
    part(world, &kit.round, &kit.chrome, Vec3::new(0.056, dy, dz - 0.0006), Vec3::new(0.0072, 0.0012, 0.0072), Quat::from_rotation_x(FRAC_PI_2), false);
    ball(world, &lamp, Vec3::new(0.056, dy, dz - 0.0032), 0.0052);
    // The cord, out of the back at the left, down onto the counter, a loop and its plug.
    let cord = [(-0.16, 0.030 * k, d / 2.), (-0.16, 0.022 * k, d / 2. + 0.03), (-0.165, 0.004, d / 2. + 0.07), (-0.2, 0.0035, d / 2. + 0.13), (-0.17, 0.0035, d / 2. + 0.19), (-0.19, 0.0035, d / 2. + 0.25)];
    for pair in cord.windows(2) {
        rod(world, &kit.black, Vec3::new(pair[0].0, pair[0].1, pair[0].2), Vec3::new(pair[1].0, pair[1].1, pair[1].2), 0.0028);
    }
    for c in &cord[1..cord.len() - 1] {
        ball(world, &kit.black, Vec3::new(c.0, c.1, c.2), 0.0028);
    }
    let end = cord[cord.len() - 1];
    cube(world, &kit.black, Vec3::new(end.0, 0.006, end.2 + 0.012), Vec3::new(0.022, 0.011, 0.026), false);
    for sx in [-0.005, 0.005] {
        cube(world, &kit.chrome, Vec3::new(end.0 + sx, 0.006, end.2 + 0.03), Vec3::new(0.0016, 0.005, 0.012), false);
    }
    // Wisps of smoke over the plate, and the sparks of grease (placed in the world, from the hotdogs on the plate).
    for i in 0..WISPS {
        let e = world.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(smoke.clone()), Transform::from_xyz(0., base + rim_h, 0.).with_scale(Vec3::ZERO), Visibility::Hidden, NotShadowCaster, Wisp { block: index, k: i })).id();
        world.entity_mut(root).add_child(e);
    }
    for i in 0..SPARKS {
        world.spawn((MapPart, Mesh3d(kit.sphere.clone()), MeshMaterial3d(sparks.clone()), Transform::default().with_scale(Vec3::ZERO), Visibility::Hidden, NotShadowCaster, Spark { block: index, k: i }));
    }
}

// ----- Sounds, and the presses waiting to go to the server.

/// The sounds, the loop that is playing and for how long nothing has called for it.
#[derive(Resource, Default)]
struct Fx {
    sizzle: Option<Handle<AudioSource>>,
    click: Option<Handle<AudioSource>>,
    looping: Option<Entity>,
    quiet: f32,
}
/// Presses of use at a griddle, not yet sent to the server (network.rs).
#[derive(Resource, Default)]
pub struct Presses(u32);
/// What was seen of each griddle last frame (by block): whether it was on, and the temperature the readout shows (eased).
#[derive(Resource, Default)]
struct Shown {
    on: HashMap<usize, bool>,
    temp: HashMap<usize, f32>,
}

/// This player presses use. At a griddle's dial it is flipped: single player flips it; online the client flips its own copy and
/// the press goes to the server (`take_presses`), whose word is the real one. Returns whether a griddle was flipped.
pub fn press(world: &mut World, g: &mut Game) -> bool {
    let Some(i) = g.feast.griddle_near(g.position.to_array(), g.yaw) else {
        return false;
    };
    if !g.network {
        return g.feast.use_griddle(1);
    }
    let me = g.me();
    let flipped = g.feast.flip_griddle(i, me);
    if flipped {
        world.get_resource_or_insert_with(Presses::default).0 += 1;
    }
    flipped
}
/// How many presses are waiting to go to the server; they are forgotten.
pub fn take_presses(world: &mut World) -> u32 {
    world.get_resource_mut::<Presses>().map_or(0, |mut p| std::mem::take(&mut p.0))
}
/// The server's `{type:'griddle',griddles:[...]}`: each griddle's state is set (the rest left alone).
pub fn hear(world: &mut World, message: &serde_json::Value) {
    let Ok(states) = serde_json::from_value::<Vec<griddle::State>>(message["griddles"].clone()) else {
        return;
    };
    if let Some(mut g) = world.get_resource_mut::<Game>() {
        g.feast.hear_griddles(&states);
    }
}

// ----- Frame by frame.

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fx>()
            .init_resource::<Shown>()
            .init_resource::<Presses>()
            .init_resource::<Readouts>()
            .init_resource::<Icons>()
            .add_systems(Startup, setup)
            .add_systems(Update, update.after(crate::arena::tick))
            .add_systems(PostUpdate, readouts.after(bevy::transform::TransformSystems::Propagate).before(bevy::ui::UiSystems::Content));
    }
}

/// A griddle in the round, for this frame: its block, whether it is on and how hot, and where its plate is.
struct Seen {
    block: usize,
    on: bool,
    temp: f32,
    plate: Plate,
}
/// Every frame after the round: online the temperatures are carried on; each griddle's plate, dial, light, smoke, sparks and sound follow its heat.
pub fn update(world: &mut World) {
    let Some(dt) = world.get_resource::<Time<Real>>().map(|t| t.delta_secs().min(0.1)) else {
        return;
    };
    let now = world.resource::<Time<Real>>().elapsed_secs();
    // The temperatures, carried on online (single player's simulation steps them itself).
    let seen: Vec<Seen>;
    let (occupants, listener): (Vec<(usize, Vec3)>, Vec3);
    {
        let Some(mut g) = world.get_resource_mut::<Game>() else {
            return;
        };
        if g.network {
            for griddle in g.feast.griddles.iter_mut() {
                griddle.advance(dt);
            }
        }
        seen = g
            .feast
            .griddles
            .iter()
            .filter_map(|griddle| g.feast.level.map.blocks.get(griddle.block).map(|b| Seen { block: griddle.block, on: griddle.on, temp: griddle.temp, plate: Plate::of(b) }))
            .collect();
        // Who stands on a plate that is hot enough to burn, and where: the sparks and the sizzle are theirs.
        occupants = g
            .roster
            .iter()
            .filter(|p| p.alive && !p.consumed)
            .flat_map(|p| seen.iter().filter(|s| griddle::burn_rate(s.temp) > 0. && s.plate.under(p.p, p.grounded)).map(|s| (s.block, Vec3::from_array(p.p))))
            .collect();
        listener = g.position;
    }
    if seen.is_empty() {
        return;
    }
    // The readout's temperature eases to the plate's, and a flip clicks.
    let mut clicks: Vec<Vec3> = vec![];
    {
        let mut shown = world.resource_mut::<Shown>();
        for s in &seen {
            let eased = shown.temp.entry(s.block).or_insert(s.temp);
            *eased = if (s.temp - *eased).abs() > 12. { s.temp } else { *eased + (s.temp - *eased) * (1. - (-dt * 9.).exp()) };
            if let Some(was) = shown.on.insert(s.block, s.on).filter(|was| *was != s.on) {
                let _ = was;
                clicks.push(Vec3::from_array(s.plate.dial));
            }
        }
    }
    // The materials, the dial and the light.
    let mut units: Vec<(usize, Unit)> = vec![];
    for unit in world.query::<&Unit>().iter(world) {
        units.push((
            unit.block,
            Unit { block: unit.block, plate: unit.plate.clone(), glow: unit.glow.clone(), lamp: unit.lamp.clone(), smoke: unit.smoke.clone(), sparks: unit.sparks.clone() },
        ));
    }
    {
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        // A material is touched only when its colour changes (a touched material is made ready for the GPU again, with
        // what is drawn with it): a cold griddle, or one held at its heat, costs nothing a frame.
        let mut set = |handle: &Handle<StandardMaterial>, base: Color, emissive: Option<LinearRgba>| {
            if mats.get(handle).is_some_and(|m| m.base_color != base || emissive.is_some_and(|e| m.emissive != e)) {
                if let Some(mut m) = mats.get_mut(handle) {
                    m.base_color = base;
                    if let Some(e) = emissive {
                        m.emissive = e;
                    }
                }
            }
        };
        for (block, unit) in &units {
            let Some(s) = seen.iter().find(|s| s.block == *block) else {
                continue;
            };
            let (heat, haze) = (glow(s.temp), smoke(s.temp));
            // The plate itself warms from near black towards a dull red-brown.
            set(&unit.plate, Color::srgb(0.03 + 0.09 * heat, 0.032 + 0.02 * heat, 0.036 + 0.01 * heat), Some(LinearRgba::rgb(0.12 * heat * heat, 0.01 * heat * heat, 0.)));
            for (m, strength) in unit.glow.iter().zip([1., 0.5]) {
                // A dull red, not a bright one, brightest along the very edge.
                let c = heat.powf(1.4) * strength;
                set(m, Color::linear_rgb(0.42 * c, 0.03 * c * c, 0.002 * c), None);
            }
            set(&unit.lamp, if s.on { Color::srgb(1., 0.48, 0.12) } else { Color::srgb(0.2, 0.06, 0.03) }, None);
            set(&unit.smoke, Color::srgba(wisp_colour(haze)[0], wisp_colour(haze)[1], wisp_colour(haze)[2], 0.14 + 0.22 * haze), None);
        }
    }
    for (mut dial, mut at) in world.query::<(&mut Dial, &mut Transform)>().iter_mut(world) {
        let Some(s) = seen.iter().find(|s| s.block == dial.block) else {
            continue;
        };
        let wanted = if s.on { ON_ANGLE } else { OFF_ANGLE };
        dial.angle += (wanted - dial.angle) * (1. - (-dt * 14.).exp());
        at.rotation = Quat::from_rotation_z(-dial.angle);
    }
    // The wisps rise from the plate a little each, in turn, wider as they climb and gone before the top; fewer when it is not so hot.
    for (wisp, mut at, mut vis) in world.query::<(&Wisp, &mut Transform, &mut Visibility)>().iter_mut(world) {
        let Some(s) = seen.iter().find(|s| s.block == wisp.block) else {
            continue;
        };
        let haze = smoke(s.temp);
        // Each wisp keeps to its own rhythm: its place in the turn, and a drift that bends with its height.
        let phase = (now * (0.22 + 0.06 * haze) + wisp.k as f32 / WISPS as f32).fract();
        let on = haze > 0. && (wisp.k as f32) < 1. + haze * (WISPS as f32 - 1.);
        let seed = wisp.k as f32 * 2.399;
        let (x, z) = ((seed.sin() * 0.16) + (now * 0.7 + seed).sin() * 0.02 * phase, (seed * 1.7).cos() * 0.07 + (now * 0.5 + seed).cos() * 0.015 * phase);
        let size = (0.014 + 0.04 * phase) * (PI * phase).sin().powf(0.6);
        at.translation = Vec3::new(x, 0.062 + 0.012 + phase * (0.2 + 0.12 * haze), z);
        at.scale = Vec3::new(size, size * 0.8, size);
        let wanted = if on && size > 0.002 { Visibility::Visible } else { Visibility::Hidden };
        if *vis != wanted {
            *vis = wanted;
        }
    }
    // The sparks: grease spitting from under each hotdog standing on a hot plate, each in its own short arc.
    for (spark, mut at, mut vis) in world.query::<(&Spark, &mut Transform, &mut Visibility)>().iter_mut(world) {
        let mine: Vec<&(usize, Vec3)> = occupants.iter().filter(|(block, _)| *block == spark.block).collect();
        if mine.is_empty() {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        }
        let heat = seen.iter().find(|s| s.block == spark.block).map_or(0., |s| smoke(s.temp).max(0.2));
        let who = mine[spark.k % mine.len()].1;
        let cycle = 0.55 - 0.2 * heat;
        let u = ((now + spark.k as f32 * 0.137) / cycle).fract();
        let n = ((now + spark.k as f32 * 0.137) / cycle).floor() + spark.k as f32 * 31.7;
        let (angle, reach) = (n.sin() * 43758.5 % TAU, 0.012 + 0.03 * (n * 1.3).cos().abs());
        let rise = 0.045 * (1. - (2. * u - 1.).powi(2)) * (0.6 + 0.7 * (n * 0.7).sin().abs());
        at.translation = who + Vec3::new(angle.cos() * reach * (0.3 + u), 0.004 + rise, angle.sin() * reach * (0.3 + u));
        let size = 0.0034 * (1. - u * 0.55);
        at.scale = Vec3::splat(size);
        if *vis != Visibility::Visible {
            *vis = Visibility::Visible;
        }
    }
    // The sounds: a click where a griddle was flipped, and the sizzle while anyone stands on a hot plate, louder the hotter and the nearer.
    let (sizzle, click) = {
        let fx = world.resource::<Fx>();
        (fx.sizzle.clone(), fx.click.clone())
    };
    for at in clicks {
        if let Some(click) = &click {
            let near = (1. - at.distance(listener) / 5.).clamp(0.2, 1.);
            world.spawn((AudioPlayer::new(click.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.55 * near))));
        }
    }
    let loudness = occupants
        .iter()
        .filter_map(|(block, at)| {
            let s = seen.iter().find(|s| s.block == *block)?;
            let near = (1. - at.distance(listener) / 4.5).clamp(0.12, 1.);
            Some((0.18 + 0.55 * griddle::burn_rate(s.temp) / griddle::BURN_RATE) * near)
        })
        .fold(None, |best: Option<f32>, v| Some(best.map_or(v, |b| b.max(v))));
    hum(world, sizzle, loudness, dt);
}
/// Keeps the sizzle looping at `volume` while it is Some, and stops it after a moment without.
fn hum(world: &mut World, sound: Option<Handle<AudioSource>>, volume: Option<f32>, dt: f32) {
    let playing = world.resource::<Fx>().looping.filter(|e| world.get_entity(*e).is_ok());
    match volume {
        Some(volume) => {
            world.resource_mut::<Fx>().quiet = 0.;
            match playing {
                Some(e) => {
                    // (A playing sink is set directly, so the game's volume is applied here as Bevy applies it to a sound as it starts: settings.rs.)
                    let global = world.get_resource::<GlobalVolume>().map_or(Volume::Linear(1.), |g| g.volume);
                    if let Some(mut sink) = world.get_mut::<AudioSink>(e) {
                        sink.set_volume(Volume::Linear(volume) * global);
                    }
                }
                None => {
                    if let Some(sound) = sound {
                        let e = world.spawn((AudioPlayer::new(sound), PlaybackSettings::LOOP.with_volume(Volume::Linear(volume)))).id();
                        world.resource_mut::<Fx>().looping = Some(e);
                    }
                }
            }
        }
        None => {
            let quiet = {
                let mut fx = world.resource_mut::<Fx>();
                fx.quiet += dt;
                fx.quiet
            };
            if quiet > 0.08 {
                if let Some(e) = world.resource_mut::<Fx>().looping.take() {
                    if let Ok(e) = world.get_entity_mut(e) {
                        e.despawn();
                    }
                }
            }
        }
    }
}

// ----- The readout over each griddle.

/// One griddle's readout: its nodes, what is laid out for, and how much of it shows (0 to 1).
struct Readout {
    root: Entity,
    dot: Entity,
    thermometer: Entity,
    digits: Entity,
    degree: Entity,
    unit: Entity,
    /// The pixels it was last laid out for (None: not yet), and the window scale it was drawn for.
    laid: Option<(f32, f32)>,
    alpha: f32,
}
#[derive(Resource, Default)]
struct Readouts {
    by: HashMap<usize, Readout>,
}
fn rgba(c: [f32; 3], a: f32) -> Color {
    Color::srgba(c[0], c[1], c[2], a)
}
/// A readout's nodes, hidden until it is placed.
fn spawn(commands: &mut Commands) -> Readout {
    let dot = commands.spawn((Node { width: Val::Px(8.), height: Val::Px(8.), border_radius: BorderRadius::MAX, flex_shrink: 0., ..default() }, BackgroundColor(Color::NONE))).id();
    let thermometer = commands.spawn((ImageNode::default(), Node { width: Val::Px(20.), height: Val::Px(20.), flex_shrink: 0., ..default() })).id();
    let text = |commands: &mut Commands, s: &str| commands.spawn((Text::new(s), TextFont { font_size: FontSize::Px(BASE_PX), ..default() }, TextColor(Color::NONE), TextLayout::no_wrap())).id();
    let digits = text(commands, "");
    let degree = commands.spawn((ImageNode::default(), Node { width: Val::Px(10.), height: Val::Px(10.), flex_shrink: 0., align_self: AlignSelf::FlexStart, margin: UiRect::top(Val::Px(2.)), ..default() })).id();
    let unit = text(commands, "C");
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.),
                padding: UiRect::axes(Val::Px(8.), Val::Px(3.)),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(7.)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
            // Under the scope's mask (arena.rs, ZIndex -1) and the head-up display, as the hotdogs' tags are.
            ZIndex(-2),
            Visibility::Hidden,
        ))
        .add_children(&[dot, thermometer, digits, degree, unit])
        .id();
    Readout { root, dot, thermometer, digits, degree, unit, laid: None, alpha: 0. }
}
/// Every frame: each griddle's readout over its plate on the screen, or hidden.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn readouts(
    mut commands: Commands,
    time: Res<Time<Real>>,
    game: Option<Res<Game>>,
    lobby: Option<Res<crate::lobby::Lobby>>,
    shown: Res<Shown>,
    cameras: Query<(&Camera, &GlobalTransform), With<HotdogCamera>>,
    windows: Query<&Window>,
    mut icons: ResMut<Icons>,
    mut images: ResMut<Assets<Image>>,
    mut readouts: ResMut<Readouts>,
    mut nodes: Query<(&mut Node, &ComputedNode)>,
    mut visibility: Query<&mut Visibility>,
    mut paints: Query<&mut BackgroundColor>,
    mut edges: Query<(&mut BorderColor, &mut ZIndex)>,
    mut texts: Query<(&mut Text, &mut TextFont, &mut TextColor)>,
    mut pictures: Query<&mut ImageNode>,
) {
    let dt = time.delta_secs().min(0.1);
    let game = game.filter(|g| crate::tags::up(g, lobby.as_ref().is_some_and(|lobby| lobby.holding)));
    let camera = cameras.iter().find(|(camera, _)| camera.is_active);
    let (Some(g), Some((camera, view))) = (game, camera) else {
        for r in readouts.by.values_mut() {
            r.alpha = 0.;
            if let Ok(mut v) = visibility.get_mut(r.root) {
                *v = Visibility::Hidden;
            }
        }
        return;
    };
    // A griddle that is not in the map any more (another map) takes its readout with it.
    readouts.by.retain(|block, r| {
        let kept = g.feast.griddles.iter().any(|griddle| griddle.block == *block);
        if !kept {
            commands.entity(r.root).despawn();
        }
        kept
    });
    let (eye, ahead) = (view.translation(), view.forward());
    let screen = camera.logical_viewport_size().unwrap_or(Vec2::new(1280., 720.));
    let (scale, sf) = windows.iter().next().map_or((1., 1.), |w| ((w.height() / crate::hud::SCALE).clamp(0.8, 4.), w.scale_factor()));
    let carried = g.roster.iter().find(|p| p.id == g.me()).and_then(|p| usize::try_from(p.carry).ok());
    let mut order = vec![];
    for griddle in &g.feast.griddles {
        let Some(block) = g.feast.level.map.blocks.get(griddle.block) else {
            continue;
        };
        let readout = readouts.by.entry(griddle.block).or_insert_with(|| spawn(&mut commands));
        let plate = Plate::of(block);
        let at = Vec3::new((plate.rect[0] + plate.rect[2]) / 2., plate.top + LIFT, (plate.rect[1] + plate.rect[3]) / 2.);
        let distance = at.distance(eye);
        // (A griddle over the top of the screen keeps its readout, pinned under the head-up display's top line: see CEILING.)
        let spot = camera.world_to_viewport(view, at).ok().filter(|spot| (at - eye).dot(*ahead) > NEAR && spot.x > -60. && spot.x < screen.x + 60. && spot.y > -400. && spot.y < screen.y + 60.);
        let extent = nodes.get(readout.root).map(|(_, laid)| laid.size() * laid.inverse_scale_factor).unwrap_or_default();
        let Some(spot) = spot.filter(|_| extent.x > 0.) else {
            // Off the screen, behind the camera or not laid out yet: it is simply not there (not yet laid out, it is let lay itself out, hidden).
            readout.alpha = 0.;
            if let Ok(mut v) = visibility.get_mut(readout.root) {
                *v = if extent.x > 0. { Visibility::Hidden } else { Visibility::Visible };
            }
            if extent.x <= 0. {
                if let Ok((mut node, _)) = nodes.get_mut(readout.root) {
                    (node.left, node.top) = (Val::Px(-2000.), Val::Px(-2000.));
                }
            }
            lay(readout, &mut icons, &mut images, &mut nodes, &mut texts, &mut pictures, 18., sf);
            continue;
        };
        let seen = !crate::tags::hidden(&g.feast, eye, at + Vec3::Y * 0.02, carried);
        let wanted = if seen { reach(distance) } else { 0. };
        readout.alpha += (wanted - readout.alpha).clamp(-BLINK * dt, BLINK * dt);
        let px = (size(distance) * scale).round();
        lay(readout, &mut icons, &mut images, &mut nodes, &mut texts, &mut pictures, px, sf);
        if let Ok((mut node, _)) = nodes.get_mut(readout.root) {
            let ceiling = CEILING * (screen.y / crate::hud::SCALE).clamp(0.8, 4.);
            let (left, top) = (Val::Px((spot.x - extent.x / 2.).round()), Val::Px((spot.y - extent.y).max(ceiling).round()));
            if node.left != left || node.top != top {
                (node.left, node.top) = (left, top);
            }
        }
        let temp = shown.temp.get(&griddle.block).copied().unwrap_or(griddle.temp);
        let (tint, a) = (heat_rgb(temp), readout.alpha);
        let line = digits(temp);
        if let Ok((mut text, _, mut colour)) = texts.get_mut(readout.digits) {
            if text.0 != line {
                text.0 = line;
            }
            colour.0 = rgba(tint, a);
        }
        if let Ok((_, _, mut colour)) = texts.get_mut(readout.unit) {
            colour.0 = rgba(tint, a);
        }
        for e in [readout.thermometer, readout.degree] {
            if let Ok(mut picture) = pictures.get_mut(e) {
                picture.color = rgba(tint, a);
            }
        }
        // The dot: lit green while the griddle is on, a dull grey when it is off.
        if let Ok(mut paint) = paints.get_mut(readout.dot) {
            paint.0 = if griddle.on { rgba([0.36, 1., 0.46], a) } else { rgba([0.46, 0.5, 0.52], a * 0.8) };
        }
        if let Ok(mut paint) = paints.get_mut(readout.root) {
            paint.0 = Color::srgba(0.02, 0.03, 0.04, 0.72 * a);
        }
        if let Ok((mut edge, _)) = edges.get_mut(readout.root) {
            let wanted = BorderColor::all(rgba(tint, 0.8 * a));
            if *edge != wanted {
                *edge = wanted;
            }
        }
        if let Ok(mut v) = visibility.get_mut(readout.root) {
            let wanted = if a >= 0.01 { Visibility::Visible } else { Visibility::Hidden };
            if *v != wanted {
                *v = wanted;
            }
        }
        if a >= 0.01 {
            order.push((readout.root, distance));
        }
    }
    // Nearer over further, all under the scope's mask.
    order.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (rank, (root, _)) in order.into_iter().enumerate() {
        if let Ok((_, mut z)) = edges.get_mut(root) {
            let wanted = ZIndex(-20 - rank as i32);
            if *z != wanted {
                *z = wanted;
            }
        }
    }
}
/// Lays a readout out for digits `px` pixels high at window scale `sf`: the sizes of its nodes and the icons drawn for the pixels they take.
fn lay(
    r: &mut Readout,
    icons: &mut Icons,
    images: &mut Assets<Image>,
    nodes: &mut Query<(&mut Node, &ComputedNode)>,
    texts: &mut Query<(&mut Text, &mut TextFont, &mut TextColor)>,
    pictures: &mut Query<&mut ImageNode>,
    px: f32,
    sf: f32,
) {
    if r.laid == Some((px, sf)) {
        return;
    }
    r.laid = Some((px, sf));
    let device = |logical: f32| (logical * sf).round().max(8.) as u32;
    let mut icon = |e: Entity, which: Icon, logical: f32| {
        let handle = icons.get(images, which, device(logical));
        let (w, h) = crate::icons::size(which, device(logical));
        if let Ok(mut picture) = pictures.get_mut(e) {
            picture.image = handle;
        }
        if let Ok((mut node, _)) = nodes.get_mut(e) {
            (node.width, node.height) = (Val::Px(w as f32 / sf), Val::Px(h as f32 / sf));
        }
    };
    icon(r.thermometer, Icon::Thermometer, px * 1.05);
    icon(r.degree, Icon::Degree, px * 0.5);
    let dot = (px * 0.36).round().max(5.);
    if let Ok((mut node, _)) = nodes.get_mut(r.dot) {
        (node.width, node.height) = (Val::Px(dot), Val::Px(dot));
    }
    if let Ok((mut node, _)) = nodes.get_mut(r.degree) {
        node.margin = UiRect::top(Val::Px((px * 0.1).round()));
    }
    if let Ok((mut node, _)) = nodes.get_mut(r.root) {
        node.padding = UiRect::axes(Val::Px((px * 0.36).round()), Val::Px((px * 0.14).round().max(2.)));
        node.column_gap = Val::Px((px * 0.16).round().max(2.));
        node.border_radius = BorderRadius::all(Val::Px((px * 0.3).round()));
    }
    for e in [r.digits, r.unit] {
        if let Ok((_, mut font, _)) = texts.get_mut(e) {
            font.font_size = FontSize::Px(px);
        }
    }
}

/// Loads the sounds.
fn setup(mut fx: ResMut<Fx>, server: Res<AssetServer>) {
    fx.sizzle = Some(server.load("audio/sizzle.wav"));
    fx.click = Some(server.load("audio/click.wav"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_readout_says_the_whole_number_of_degrees_and_a_drawn_degree_mark_and_c() {
        assert_eq!(digits(21.), "21");
        assert_eq!(digits(187.4), "187");
        assert_eq!(digits(187.5), "188");
        assert_eq!(digits(204.6), "205");
        assert_eq!(digits(205.), "205");
        assert_eq!(digits(-3.), "0");
        // (As one string, with the sign the font cannot draw: the screen draws the mark itself, as a ring.)
        assert_eq!(describe(187.2), "187\u{b0}C");
        assert_eq!(describe(21.), "21\u{b0}C");
        assert!(!digits(187.2).contains('\u{b0}'), "the digits are text the font can draw, and the mark is not");
        assert!(digits(187.2).is_ascii());
    }

    #[test]
    fn the_colour_runs_from_cool_blue_through_white_and_orange_to_red() {
        let at = |t: f32| heat_rgb(t);
        // The stops.
        assert_eq!(at(21.), [0.60, 0.80, 1.00]);
        assert_eq!(at(0.), at(21.), "never cooler than room temperature's");
        assert_eq!(at(205.), [1.00, 0.2, 0.12]);
        assert_eq!(at(400.), at(205.));
        // Blue below, red above: cool is blue-led, hot is red-led.
        let (cool, warm, hot) = (at(21.), at(150.), at(205.));
        assert!(cool[2] > cool[0] && cool[2] >= 0.99, "cool is blue: {cool:?}");
        assert!(warm[0] > warm[2] && warm[1] > warm[2] && warm[1] < warm[0], "orange: {warm:?}");
        assert!(hot[0] > hot[1] * 4. && hot[0] > hot[2] * 4., "red: {hot:?}");
        // White between (all three high), and warm yellow at the temperature a burn begins.
        let white = at(60.);
        assert!(white.iter().all(|v| *v > 0.9));
        let yellow = at(griddle::BURNS_AT);
        assert!(yellow[0] > 0.95 && yellow[1] > yellow[2] + 0.3, "{yellow:?}");
        // Continuous, and the blue falls and the green falls the hotter it gets, from 60 up.
        let mut last = at(21.);
        for t in 22..=205 {
            let now = at(t as f32);
            assert!((0..3).all(|i| (now[i] - last[i]).abs() < 0.05), "a jump at {t}: {last:?} {now:?}");
            if t > 60 {
                assert!(now[2] <= last[2] + 1e-6 && now[1] <= last[1] + 1e-6, "{t}");
            }
            last = now;
        }
        // Every channel is a real colour.
        for t in (0..260).step_by(5) {
            assert!(at(t as f32).iter().all(|v| (0. ..=1.).contains(v)));
        }
    }

    #[test]
    fn the_plate_glows_from_55_degrees_and_smokes_from_120() {
        assert_eq!([glow(21.), glow(55.)], [0., 0.]);
        assert_eq!(glow(205.), 1.);
        assert!(glow(100.) > 0. && glow(100.) < glow(150.) && glow(150.) < 1.);
        assert_eq!([smoke(21.), smoke(100.), smoke(120.)], [0.; 3]);
        assert!(smoke(120.5) > 0.25 && smoke(125.) < 0.35, "a few wisps as soon as it is over 120");
        assert_eq!(smoke(205.), 1.);
        assert!((smoke(162.5) - 0.625).abs() < 1e-5);
    }

    #[test]
    fn the_readout_is_larger_nearer_and_smaller_further_and_readable_across_the_field() {
        let (near, mid, far) = (size(0.5), size(1.2), size(6.));
        assert!(near > mid && mid > far, "{near} {mid} {far}");
        assert_eq!(mid, 24.);
        assert!(far >= MIN_PX && near <= MAX_PX, "{far} {near}");
        // The Forts maps are 6 m long (the whole kitchen's 8.4): from one end to the other it is still 18 pixels or more, in whole even pixels.
        for d in [0.2, 0.5, 1., 2., 4., 6., 8.] {
            let px = size(d);
            assert!((MIN_PX..=MAX_PX).contains(&px) && px % 2. == 0., "{d}: {px}");
        }
        assert_eq!([reach(0.5), reach(FADE), reach(FAR), reach(20.)], [1., 1., 0., 0.]);
        assert!(reach(8.) > 0. && reach(8.) < 1.);
    }

    /// The test round on the bundled counter with an electric griddle added to it, its dial on its +z side at z = 3.24; this player
    /// (feast hotdog 1) is put in front of the dial looking at it.
    fn at_a_griddle() -> App {
        use kitchen_sim::map::Block;
        let mut app = crate::arena::test_app("");
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.feast.level.map.blocks.push(Block { rect: [-0.15, 2.96, 0.35, 3.24], height: 0.06, style: "griddle".into(), facing: Some(Side::PlusZ) });
            let block = g.feast.level.map.blocks.len() - 1;
            g.feast.griddles.push(griddle::Griddle::new(block));
            let at = Vec3::new(0.1, kitchen_sim::feast::FLOOR, 3.4);
            (g.position, g.yaw) = (at, 0.);
            let (p, yaw) = (at.to_array(), 0.);
            let me = g.feast.player_mut(1).unwrap();
            (me.p, me.yaw) = (p, yaw);
        }
        app
    }
    fn pressed(app: &mut App) -> bool {
        app.world_mut().resource_scope(|world, mut g: Mut<Game>| press(world, &mut g))
    }

    #[test]
    fn a_press_at_the_dial_flips_the_griddle_in_single_player_and_nowhere_else() {
        let mut app = at_a_griddle();
        assert!(pressed(&mut app));
        {
            let g = app.world().resource::<Game>();
            assert_eq!((g.feast.griddles[0].on, g.feast.griddles[0].by), (true, 1));
        }
        assert_eq!(take_presses(app.world_mut()), 0, "single player tells nobody");
        // Out of reach (or turned away): nothing.
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.feast.griddles[0].lock = 0.;
            g.position = Vec3::new(0.1, kitchen_sim::feast::FLOOR, 3.9);
        }
        assert!(!pressed(&mut app));
        assert!(app.world().resource::<Game>().feast.griddles[0].on);
    }

    #[test]
    fn online_a_press_flips_the_clients_own_copy_and_is_sent_once_and_the_servers_word_sets_the_state() {
        let mut app = at_a_griddle();
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.network = true;
            g.network_id = 3;
        }
        assert!(pressed(&mut app));
        assert!(!pressed(&mut app), "it holds still a moment: the second press is not sent");
        {
            let g = app.world().resource::<Game>();
            assert_eq!((g.feast.griddles[0].on, g.feast.griddles[0].by), (true, 3));
        }
        assert_eq!((take_presses(app.world_mut()), take_presses(app.world_mut())), (1, 0));
        // The server says it is off (it was refused there), and how hot it is: that is what the client has.
        let told = serde_json::json!({"type": "griddle", "griddles": [{"block": app.world().resource::<Game>().feast.griddles[0].block, "on": false, "temp": 143.5, "by": 2}]});
        hear(app.world_mut(), &told);
        let g = app.world().resource::<Game>();
        assert_eq!((g.feast.griddles[0].on, g.feast.griddles[0].temp, g.feast.griddles[0].by), (false, 143.5, 2));
        // Junk is passed over.
        hear(app.world_mut(), &serde_json::json!({"type": "griddle", "griddles": "hot"}));
        hear(app.world_mut(), &serde_json::json!({"type": "griddle"}));
        hear(app.world_mut(), &serde_json::json!({"type": "griddle", "griddles": [{"block": 999, "on": true, "temp": 5, "by": 0}]}));
        assert_eq!(app.world().resource::<Game>().feast.griddles[0].temp, 143.5);
    }

    #[test]
    fn the_dial_has_two_settings_and_the_wisps_rise_from_a_cool_plate_not_at_all() {
        assert!(ON_ANGLE > OFF_ANGLE && ON_ANGLE - OFF_ANGLE < TAU);
        assert_eq!(wisp_colour(0.)[2], 0.78);
        assert!(wisp_colour(1.)[0] > wisp_colour(0.)[0]);
    }
}

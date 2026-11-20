// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The arena's scenery, built from a map in the editor's format, and the ways a map reaches the game:
//! `--map <file>` natively, the page's `window.hotdogMap` in a browser, the server's welcome online,
//! and otherwise the bundled map. The rules of a map live in `kitchen_sim::map`; nothing here is hit-tested.
use crate::arena::{MicrowaveDigit, MicrowaveGlow, Smoke};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use kitchen_sim::map::{Block, Map, Prop, Ramp, Side};
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};

const FLOOR: f32 = 0.9025;

/// Everything built from the map carries this on its root entity, so a new map can replace it.
#[derive(Component)]
pub struct MapPart;
/// A turret's root entity, with its place among the map's turrets: `arena` turns it to the yaw the
/// simulation has it at (`Feast::turret_yaw`).
#[derive(Component)]
pub struct Turret(pub usize);
/// The map's `theme`: "kitchen", "night" or "jungle". Set again every time a map is loaded, so a system
/// that dresses the scene can react with `resource_changed::<MapTheme>`.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct MapTheme(pub String);
/// The map the game starts on.
#[derive(Resource)]
pub struct StartMap(pub Map);

/// Shared meshes and one material per map style; made once, so rebuilding a map allocates no assets.
#[derive(Resource, Clone)]
struct Kit {
    cube: Handle<Mesh>,
    sphere: Handle<Mesh>,
    wedge: Handle<Mesh>,
    styles: HashMap<&'static str, Handle<StandardMaterial>>,
    counter: Handle<StandardMaterial>,
    microwave: Handle<StandardMaterial>,
    black: Handle<StandardMaterial>,
    digit: Handle<StandardMaterial>,
    glow: Handle<StandardMaterial>,
    smoke: Handle<StandardMaterial>,
    optic: Handle<StandardMaterial>,
}
impl FromWorld for Kit {
    fn from_world(world: &mut World) -> Self {
        let (cube, sphere, wedge) = {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            (
                meshes.add(Cuboid::default()),
                meshes.add(Sphere::new(1.).mesh().uv(12, 8)),
                meshes.add(wedge()),
            )
        };
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        let mut plain = |r: f32, g: f32, b: f32, roughness: f32| crate::mat(&mut mats, Color::srgb(r, g, b), roughness);
        // The editor's styles (editor/view.js STYLE) in the game's palette.
        let mut styles = HashMap::from([
            ("stone", plain(0.35, 0.38, 0.36, 0.42)),
            ("wood", plain(0.52, 0.33, 0.17, 0.6)),
            ("dark", plain(0.05, 0.065, 0.07, 0.45)),
            ("white", plain(0.94, 0.91, 0.75, 0.5)),
            ("red", plain(0.70, 0.12, 0.07, 0.45)),
            ("green", plain(0.30, 0.52, 0.22, 0.55)),
            ("yellow", plain(0.98, 0.71, 0.12, 0.55)),
            ("metal", plain(0.30, 0.39, 0.40, 0.6)),
            ("bamboo", plain(0.80, 0.64, 0.38, 0.62)),
        ]);
        let counter = plain(0.35, 0.38, 0.36, 0.42);
        let microwave = plain(0.35, 0.38, 0.36, 0.38);
        let black = plain(0.025, 0.030, 0.027, 0.3);
        styles.insert(
            "steel",
            mats.add(StandardMaterial {
                base_color: Color::srgb(0.42, 0.53, 0.56),
                metallic: 0.8,
                perceptual_roughness: 0.24,
                ..default()
            }),
        );
        Kit {
            cube,
            sphere,
            wedge,
            styles,
            counter,
            microwave,
            black,
            digit: mats.add(StandardMaterial {
                base_color: Color::srgb(0.35, 1., 0.53),
                emissive: LinearRgba::rgb(0.3, 4., 0.7),
                unlit: true,
                ..default()
            }),
            glow: mats.add(StandardMaterial {
                base_color: Color::srgba(0.5, 0.04, 0.005, 0.3),
                emissive: LinearRgba::rgb(2., 0.09, 0.002),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            }),
            smoke: mats.add(StandardMaterial {
                base_color: Color::srgba(0.11, 0.14, 0.12, 0.14),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            }),
            optic: mats.add(StandardMaterial {
                base_color: Color::srgb(1., 0.015, 0.03),
                emissive: LinearRgba::rgb(8., 0.02, 0.03),
                unlit: true,
                ..default()
            }),
        }
    }
}
impl Kit {
    fn style(&self, name: &str) -> Handle<StandardMaterial> {
        self.styles.get(name).unwrap_or(&self.styles["steel"]).clone()
    }
}

/// A unit ramp: one metre wide along x, climbing from y = 0 at z = -0.5 to y = 1 at z = 0.5.
fn wedge() -> Mesh {
    let (l, r, low, high) = (-0.5, 0.5, -0.5, 0.5);
    let slope = std::f32::consts::FRAC_1_SQRT_2;
    let faces: [(&[[f32; 3]], [f32; 3]); 5] = [
        (&[[l, 0., low], [l, 1., high], [r, 1., high], [r, 0., low]], [0., slope, -slope]),
        (&[[l, 0., high], [r, 0., high], [r, 1., high], [l, 1., high]], [0., 0., 1.]),
        (&[[l, 0., low], [r, 0., low], [r, 0., high], [l, 0., high]], [0., -1., 0.]),
        (&[[l, 0., low], [l, 0., high], [l, 1., high]], [-1., 0., 0.]),
        (&[[r, 0., low], [r, 1., high], [r, 0., high]], [1., 0., 0.]),
    ];
    let (mut positions, mut normals, mut uvs, mut indices) = (vec![], vec![], vec![], vec![]);
    for (corners, normal) in faces {
        let first = positions.len() as u32;
        for corner in corners {
            positions.push(*corner);
            normals.push(normal);
            uvs.push([corner[0] + 0.5, corner[2] + 0.5]);
        }
        for i in 1..corners.len() as u32 - 1 {
            indices.extend([first, first + i, first + i + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// A root entity for one object of the map.
fn root(world: &mut World, name: &'static str, transform: Transform) -> Entity {
    world.spawn((MapPart, Name::new(name), transform, Visibility::default())).id()
}
fn solid(world: &mut World, parent: Entity, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, transform: Transform) -> Entity {
    let id = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), transform)).id();
    world.entity_mut(parent).add_child(id);
    id
}
/// A part that never moves, changes or hides on its own: drawn baked at Low and Lowest (`bake`).
fn still(world: &mut World, part: Entity) -> Entity {
    world.entity_mut(part).insert(crate::bake::Still);
    part
}
fn cube(world: &mut World, kit: &Kit, parent: Entity, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3) -> Entity {
    solid(world, parent, &kit.cube, material, Transform::from_translation(at).with_scale(size))
}
/// A box standing on its own: `rect` is [min x, min z, max x, max z] and `base` its underside in world space.
fn slab(world: &mut World, kit: &Kit, name: &'static str, material: &Handle<StandardMaterial>, rect: [f32; 4], base: f32, height: f32) {
    let [x0, z0, x1, z1] = rect;
    world.spawn((
        MapPart,
        Name::new(name),
        Mesh3d(kit.cube.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_xyz((x0 + x1) / 2., base + height / 2., (z0 + z1) / 2.).with_scale(Vec3::new(x1 - x0, height, z1 - z0)),
        crate::bake::Still,
    ));
}
/// Appliances are built facing -z around the middle of their footprint and then turned to their `facing`:
/// returns the root's transform, the width across the front and the depth behind it.
pub(crate) fn facing(rect: [f32; 4], side: Side) -> (Transform, f32, f32) {
    let [x0, z0, x1, z1] = rect;
    let (across, deep) = if side.along_z() { (x1 - x0, z1 - z0) } else { (z1 - z0, x1 - x0) };
    let turn = match side {
        Side::MinusZ => 0.,
        Side::PlusZ => PI,
        Side::MinusX => FRAC_PI_2,
        Side::PlusX => -FRAC_PI_2,
    };
    (
        Transform::from_xyz((x0 + x1) / 2., FLOOR, (z0 + z1) / 2.).with_rotation(Quat::from_rotation_y(turn)),
        across,
        deep,
    )
}

/// Microwave: body, glass door, side controls, the countdown clock, and the glow, silhouettes and smoke
/// that `arena::microwave` animates. Proportions are those of the 0.48 × 0.28 × 0.32 m original.
fn microwave(world: &mut World, kit: &Kit, index: usize, block: &Block, rect: [f32; 4], side: Side) {
    let (transform, w, d) = facing(rect, side);
    let h = block.height as f32;
    let front = -d / 2.;
    let id = root(world, "Microwave", transform);
    // The body is a shell round the cooking cavity: floor, roof, left wall, the control side and the back,
    // with a dark lining. The cavity's floor is the sill the ooze brims over (`ooze::SILL`).
    let (sill, roof) = (crate::ooze::SILL * h, 0.875 * h);
    // (The shell, its lining and its buttons never move: `still`. The door, the clock, the glow and the smoke do.)
    for (at, size) in [
        (Vec3::new(0., sill / 2., 0.), Vec3::new(w, sill, d)),
        (Vec3::new(0., (roof + h) / 2., 0.), Vec3::new(w, h - roof, d)),
        (Vec3::new(-0.455 * w, (sill + roof) / 2., 0.), Vec3::new(0.09 * w, roof - sill, d)),
        (Vec3::new(0.3925 * w, (sill + roof) / 2., 0.), Vec3::new(0.215 * w, roof - sill, d)),
        (Vec3::new(-0.0625 * w, (sill + roof) / 2., 0.44 * d), Vec3::new(0.695 * w, roof - sill, 0.12 * d)),
    ] {
        let e = cube(world, kit, id, &kit.microwave, at, size);
        still(world, e);
    }
    let e = cube(world, kit, id, &kit.black, Vec3::new(-0.0625 * w, (sill + roof) / 2., 0.38 * d - 0.001), Vec3::new(0.695 * w, roof - sill, 0.002));
    still(world, e);
    let e = cube(world, kit, id, &kit.black, Vec3::new(-0.0625 * w, sill + 0.001, -0.06 * d), Vec3::new(0.695 * w, 0.002, 0.88 * d));
    still(world, e);
    let e = cube(world, kit, id, &kit.black, Vec3::new(-0.0625 * w, roof - 0.001, -0.06 * d), Vec3::new(0.695 * w, 0.002, 0.88 * d));
    still(world, e);
    for x in [-0.41 * w + 0.001, 0.285 * w - 0.001] {
        let e = cube(world, kit, id, &kit.black, Vec3::new(x, (sill + roof) / 2., -0.06 * d), Vec3::new(0.002, roof - sill, 0.88 * d));
        still(world, e);
    }
    // The glass door hangs on a hinge down its left edge; `ooze` swings it open when the timer goes off.
    let hinge = world.spawn((crate::ooze::Hinge(index), Transform::from_xyz(-0.427 * w, 0., front - 0.008), Visibility::default())).id();
    world.entity_mut(id).add_child(hinge);
    cube(world, kit, hinge, &kit.black, Vec3::new(0.3645 * w, 0.518 * h, 0.), Vec3::new(0.729 * w, 0.75 * h, 0.012));
    cube(world, kit, hinge, &kit.style("steel"), Vec3::new(0.69 * w, 0.518 * h, -0.009), Vec3::new(0.012, 0.5 * h, 0.007));
    for y in [0.625, 0.411, 0.196] {
        let e = cube(world, kit, id, &kit.black, Vec3::new(0.4125 * w, y * h, front - 0.010), Vec3::new(0.058 * w, 0.043 * h, 0.014));
        still(world, e);
    }
    // Digital clock: actual lit geometry, visible from the hotdog's view.
    let size = (w / 0.48).min(h / 0.28);
    for digit in 0..4 {
        for segment in 0..7 {
            let (x, y, sx, sy) = match segment {
                0 => (0., 0.012, 0.009, 0.002),
                1 => (0.005, 0.006, 0.002, 0.010),
                2 => (0.005, -0.006, 0.002, 0.010),
                3 => (0., -0.012, 0.009, 0.002),
                4 => (-0.005, -0.006, 0.002, 0.010),
                5 => (-0.005, 0.006, 0.002, 0.010),
                _ => (0., 0., 0.009, 0.002),
            };
            // The front faces -z, so the clock reads left to right towards -x.
            let at = Vec3::new(0.3125 * w + ((3 - digit) as f32 * 0.014 - x) * size, 0.857 * h + y * size, front - 0.017);
            let e = cube(world, kit, id, &kit.digit, at, Vec3::new(sx * size, sy * size, 0.001));
            world.entity_mut(e).insert(MicrowaveDigit(digit, segment));
        }
    }
    let e = cube(world, kit, id, &kit.glow, Vec3::new(-0.0625 * w, 0.518 * h, front - 0.016), Vec3::new(0.646 * w, 0.571 * h, 0.001));
    world.entity_mut(e).insert((MicrowaveGlow, Visibility::Hidden));
    // Flickering silhouettes behind the door suggest the threat before it emerges.
    for x in [-0.208, -0.052, 0.104] {
        let transform = Transform::from_xyz(x * w, 0.482 * h, front - 0.021).with_scale(Vec3::new(0.054 * w, 0.196 * h, 0.001));
        let e = solid(world, id, &kit.sphere, &kit.black, transform);
        world.entity_mut(e).insert((MicrowaveGlow, Visibility::Hidden));
    }
    // Smoke rises from a vent near the front of the top.
    let vent = world.spawn((Transform::from_xyz(0., h - 0.005, -0.34 * d), Visibility::default())).id();
    world.entity_mut(id).add_child(vent);
    for i in 0..10 {
        let e = solid(world, vent, &kit.sphere, &kit.smoke, Transform::default());
        world.entity_mut(e).insert((Smoke(i), Visibility::Hidden, bevy::light::NotShadowCaster));
    }
}
/// Toaster: a steel body with two slots along its depth and a lever on its front.
fn toaster(world: &mut World, kit: &Kit, block: &Block, rect: [f32; 4], side: Side) {
    let (transform, w, d) = facing(rect, side);
    let h = block.height as f32;
    let steel = kit.style("steel");
    let dark = kit.style("dark");
    let id = root(world, "Toaster", transform);
    let e = cube(world, kit, id, &steel, Vec3::new(0., h / 2., 0.), Vec3::new(w, h, d));
    still(world, e);
    for x in [-0.186, 0.186] {
        let e = cube(world, kit, id, &dark, Vec3::new(x * w, h + 0.004, 0.), Vec3::new(0.186 * w, 0.008, 0.667 * d));
        still(world, e);
    }
    let e = cube(world, kit, id, &dark, Vec3::new(0.36 * w, 0.62 * h, -d / 2. - 0.006), Vec3::new(0.06 * w, 0.05 * h, 0.012));
    still(world, e);
}
/// A ramp that names a style: a plank of that material laid up the climb, its top the walking surface, like a
/// chopping board propped in a sink or against the side of the microwave; a wooden one has a hanging hole near its top.
fn plank(world: &mut World, kit: &Kit, ramp: &Ramp, style: &str) {
    let [x0, z0, x1, z1] = ramp.rect.map(|v| v as f32);
    let (base, climb) = (ramp.base as f32, (ramp.height - ramp.base) as f32);
    let (width, run) = if ramp.rise.along_z() { (x1 - x0, z1 - z0) } else { (z1 - z0, x1 - x0) };
    let turn = match ramp.rise {
        Side::PlusZ => 0.,
        Side::MinusZ => PI,
        Side::PlusX => FRAC_PI_2,
        Side::MinusX => -FRAC_PI_2,
    };
    let id = root(world, "Plank ramp", Transform::from_xyz((x0 + x1) / 2., FLOOR + base, (z0 + z1) / 2.).with_rotation(Quat::from_rotation_y(turn)));
    // In the ramp's frame the surface climbs along +z, from height 0 at z = -run / 2 to `climb` at run / 2.
    let (length, thick) = (run.hypot(climb), 0.012);
    let tilt = Quat::from_rotation_x(-climb.atan2(run));
    let lift = Vec3::new(0., run / length, -climb / length);
    let material = kit.style(style);
    let e = solid(world, id, &kit.cube, &material, Transform::from_translation(Vec3::new(0., climb / 2., 0.) - lift * (thick / 2.)).with_rotation(tilt).with_scale(Vec3::new(width, thick, length)));
    still(world, e);
    if style == "wood" {
        let hole = Vec3::new(0., climb * 0.88, run * 0.38) + lift * 0.0004;
        let e = solid(world, id, &kit.sphere, &kit.black, Transform::from_translation(hole).with_rotation(tilt).with_scale(Vec3::new(0.026, 0.001, 0.016)));
        world.entity_mut(e).insert((bevy::light::NotShadowCaster, crate::bake::Still));
    }
}

/// A wedge with yellow edges and stripes up the climb.
fn ramp(world: &mut World, kit: &Kit, ramp: &Ramp) {
    let [x0, z0, x1, z1] = ramp.rect.map(|v| v as f32);
    let (base, climb) = (ramp.base as f32, (ramp.height - ramp.base) as f32);
    let (width, run) = if ramp.rise.along_z() { (x1 - x0, z1 - z0) } else { (z1 - z0, x1 - x0) };
    let turn = match ramp.rise {
        Side::PlusZ => 0.,
        Side::MinusZ => PI,
        Side::PlusX => FRAC_PI_2,
        Side::MinusX => -FRAC_PI_2,
    };
    let id = root(
        world,
        "Walkable ramp",
        Transform::from_xyz((x0 + x1) / 2., FLOOR + base, (z0 + z1) / 2.).with_rotation(Quat::from_rotation_y(turn)),
    );
    let e = solid(world, id, &kit.wedge, &kit.style("metal"), Transform::from_scale(Vec3::new(width, climb, run)));
    still(world, e);
    // In the ramp's frame the surface climbs along +z; `lift` raises a part off it along its normal.
    let length = run.hypot(climb);
    let tilt = Quat::from_rotation_x(-climb.atan2(run));
    let lift = Vec3::new(0., run / length, -climb / length);
    let yellow = kit.style("yellow");
    for edge in [-1., 1.] {
        let at = Vec3::new(edge * (width / 2. - 0.0045), climb / 2., 0.) + lift * 0.004;
        let e = solid(world, id, &kit.cube, &yellow, Transform::from_translation(at).with_rotation(tilt).with_scale(Vec3::new(0.009, 0.012, length)));
        still(world, e);
    }
    for i in 0..6 {
        let along = (i as f32 / 5. - 0.5) * 0.85;
        let at = Vec3::new(0., climb * (0.5 + along), run * along) + lift * 0.001;
        let e = solid(world, id, &kit.cube, &yellow, Transform::from_translation(at).with_rotation(tilt).with_scale(Vec3::new(width * 0.65, 0.002, 0.013)));
        still(world, e);
    }
}

/// Replaces the scenery with the given map's: counter, blocks, appliances, decks, ramps, props and turrets.
/// Pickups belong to the round and are spawned by `arena::load_map`, which calls this.
pub fn build(world: &mut World, map: &Map) {
    let old: Vec<Entity> = world.query_filtered::<Entity, With<MapPart>>().iter(world).collect();
    for entity in old {
        if let Ok(entity) = world.get_entity_mut(entity) {
            entity.despawn();
        }
    }
    world.init_resource::<Kit>();
    let kit = world.resource::<Kit>().clone();
    world.insert_resource(MapTheme(map.theme.clone()));
    let kit = &kit;
    // The counter top reaches a little past the playable area, as the kitchen's own does.
    let [x0, z0, x1, z1] = map.bounds.map(|v| v as f32);
    // The sun, lights, haze and foliage place themselves around the playable rectangle.
    if let Some(mut arena) = world.get_resource_mut::<crate::visuals::ArenaBounds>() {
        let (min, max) = (Vec2::new(x0, z0), Vec2::new(x1, z1));
        if arena.min != min || arena.max != max {
            arena.min = min;
            arena.max = max;
        }
    }
    // The kitchen's walls stand round it, further out for a bigger counter.
    crate::fridge::fit_room(world, map.bounds);
    // The counter top, with a hole for each sink (`kitchen` draws the sinks).
    for piece in crate::kitchen::counter_top(map) {
        slab(world, kit, "Counter", &kit.counter, piece.map(|v| v as f32), FLOOR - crate::kitchen::SLAB, crate::kitchen::SLAB);
    }
    for (index, block) in map.blocks.iter().enumerate() {
        let rect = block.rect.map(|v| v as f32);
        match (block.style.as_str(), block.facing) {
            ("microwave", Some(side)) => microwave(world, kit, index, block, rect, side),
            ("toaster", Some(side)) => toaster(world, kit, block, rect, side),
            // The electric griddle is heated, switched and read out by griddle.rs.
            ("griddle", Some(side)) => crate::griddle::build(world, index, block, rect, side),
            // The counter things are `kitchen`'s; the gong and the chopstick posts `dojo`'s.
            (style, Some(_)) if crate::kitchen::drawn(style) || style == crate::dojo::GONG => {}
            _ if crate::dojo::slim(block) => {}
            (style, _) => slab(world, kit, "Block", &kit.style(style), rect, FLOOR, block.height as f32),
        }
    }
    // A deck is a raised floor: a plate on four legs, so whatever stands beneath it stays in view.
    // (A round deck is a steamer: `dojo` draws it.)
    for deck in map.decks.iter().filter(|deck| !crate::dojo::round(deck)) {
        let [x0, z0, x1, z1] = deck.rect.map(|v| v as f32);
        let (height, material) = (deck.height as f32, kit.style(&deck.style));
        let plate = height.min(0.025);
        slab(world, kit, "Deck", &material, [x0, z0, x1, z1], FLOOR + height - plate, plate);
        if height - plate > 0.01 {
            let inset = 0.012_f32.min((x1 - x0) / 2.).min((z1 - z0) / 2.);
            for (x, z) in [(x0 + inset, z0 + inset), (x1 - inset, z0 + inset), (x0 + inset, z1 - inset), (x1 - inset, z1 - inset)] {
                slab(world, kit, "Deck leg", &material, [x - 0.006, z - 0.006, x + 0.006, z + 0.006], FLOOR, height - plate);
            }
        } else if deck.style == "wood" && x1 - x0 > 0.1 && z1 - z0 > 0.1 {
            // A wooden board lying flat is a chopping board: a juice groove runs round it near its edge.
            let (y, g, w) = (FLOOR + height + 0.0003, 0.022, 0.004);
            for rect in [[x0 + g, z0 + g, x1 - g, z0 + g + w], [x0 + g, z1 - g - w, x1 - g, z1 - g], [x0 + g, z0 + g, x0 + g + w, z1 - g], [x1 - g - w, z0 + g, x1 - g, z1 - g]] {
                let [a, b, c, d] = rect;
                let e = world.spawn((MapPart, Name::new("Groove"), Mesh3d(kit.cube.clone()), MeshMaterial3d(kit.style("dark")), Transform::from_xyz((a + c) / 2., y, (b + d) / 2.).with_scale(Vec3::new(c - a, 0.0006, d - b)), crate::bake::Still)).id();
                world.entity_mut(e).insert(bevy::light::NotShadowCaster);
            }
        }
    }
    for item in &map.ramps {
        match &item.style {
            Some(style) => plank(world, kit, item, style),
            None => ramp(world, kit, item),
        }
    }
    let mut turrets = 0;
    for prop in &map.props {
        match prop {
            Prop::Box { rect, base, height, style } => {
                slab(world, kit, "Prop", &kit.style(style), rect.map(|v| v as f32), FLOOR + *base as f32, *height as f32);
            }
            // A hitscan turret with a small red tracking optic, placed at the map's yaw; it turns on its post
            // to the yaw `Feast` has it at.
            Prop::Turret { p, base, yaw } => {
                let id = root(
                    world,
                    "Turret: tracking pulse hitscan",
                    Transform::from_xyz(p[0] as f32, FLOOR + *base as f32, p[1] as f32).with_rotation(Quat::from_rotation_y(*yaw as f32)),
                );
                world.entity_mut(id).insert(Turret(turrets));
                turrets += 1;
                let steel = kit.style("steel");
                solid(world, id, &kit.sphere, &steel, Transform::from_scale(Vec3::new(0.065, 0.035, 0.045)));
                cube(world, kit, id, &steel, Vec3::new(0., 0., -0.065), Vec3::new(0.023, 0.024, 0.10));
                solid(world, id, &kit.sphere, &kit.optic, Transform::from_xyz(0., 0.015, -0.12).with_scale(Vec3::splat(0.007)));
            }
            // A bin stands on the kitchen floor: `kitchen` draws it.
            Prop::Bin { .. } => {}
        }
    }
    // The sinks, the counter things, the bins and the fitted cabinets.
    crate::kitchen::build(world, map);
    // The steamers, the ring and its posts, the gong.
    crate::dojo::build(world, map);
    // The loose objects: the bowl and whatever else can be knocked about.
    crate::loose::build(world, map);
    // A Forts map's bases, forcefield and bacon.
    crate::forts::build(world, map);
}

/// The map to start on: `--map <file>` natively, the page's map in a browser, otherwise the bundled one.
/// An unreadable or invalid map is an error carrying the reason.
pub fn choose(options: &crate::runtime::Options) -> Result<Map, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if options.map.is_empty() {
            return Ok(Map::bundled());
        }
        let text = std::fs::read_to_string(&options.map).map_err(|error| format!("{}: {error}", options.map))?;
        Map::parse(&text).map_err(|error| format!("{}: {error}", options.map))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = options;
        let (text, name) = (page_map(), page_map_name());
        if !text.is_empty() {
            Map::parse(&text).map_err(|error| error.to_string())
        } else if !name.is_empty() {
            Err(format!("no map named \"{name}\" in assets/maps"))
        } else {
            Ok(Map::bundled())
        }
    }
}
/// Tells the player why a map was not loaded: a banner over the page in a browser (an empty text hides
/// it), the log natively.
pub fn notice(text: &str) {
    #[cfg(target_arch = "wasm32")]
    map_notice(text);
    #[cfg(not(target_arch = "wasm32"))]
    if !text.is_empty() {
        warn!("{text}");
    }
}
/// Live reload: the page counts every map the editor sends in `window.hotdogMapVersion`. A valid one
/// replaces the arena and restarts the round; an invalid one leaves the game as it is and says why.
/// Online, the server owns the map and the page's is ignored.
pub fn watch(world: &mut World, mut seen: Local<u32>) {
    #[cfg(target_arch = "wasm32")]
    {
        let version = page_map_version();
        if version == *seen {
            return;
        }
        *seen = version;
        if !world.resource::<crate::runtime::Options>().connect.is_empty() {
            return;
        }
        let current = world.resource::<crate::arena::Game>().feast.level.map.clone();
        match Map::parse(&page_map()) {
            Ok(map) => {
                notice("");
                if map != current {
                    info!("Map changed in the editor: {}", map.name);
                    crate::arena::load_map(world, map);
                }
            }
            Err(error) => notice(&format!("Map not loaded: {error}. Still playing {}.", current.name)),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (world, &mut seen);
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function page_map(){return typeof window.hotdogMap==='string'?window.hotdogMap:''}
export function page_map_name(){return new URLSearchParams(location.search).get('map')||''}
export function page_map_version(){return window.hotdogMapVersion>>>0}
export function map_notice(text){let el=document.getElementById('map-notice');if(!el){el=document.createElement('div');el.id='map-notice';el.setAttribute('role','alert');el.style.cssText='position:absolute;left:50%;bottom:112px;transform:translateX(-50%);z-index:9;width:max-content;max-width:min(680px,calc(100vw - 40px));padding:10px 16px;border-radius:6px;background:#3a1512f2;border:1px solid #f1c2ae66;color:#f1c2ae;font:13px/1.5 system-ui,sans-serif;text-align:center;pointer-events:none';document.body.appendChild(el)}el.textContent=text;el.style.display=text?'block':'none';if(text)console.warn(text)}
")]
extern "C" {
    fn page_map() -> String;
    fn page_map_name() -> String;
    fn page_map_version() -> u32;
    fn map_notice(text: &str);
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
    fn busier() -> Map {
        Map::parse(include_str!("../assets/maps/counter-siege-v2.json")).unwrap()
    }
    #[test]
    fn a_new_map_replaces_the_scenery_of_the_old_one() {
        let mut world = world();
        let parts = |world: &mut World| world.query_filtered::<(), With<MapPart>>().iter(world).count();
        build(&mut world, &Map::bundled());
        // The counter, two appliances, two blocks, a deck on four legs, two ramps, the turret and the bowl.
        assert_eq!(parts(&mut world), 1 + 2 + 2 + 5 + 2 + 1 + 1);
        let count = |world: &mut World| world.query::<()>().iter(world).count();
        let (entities, meshes) = (count(&mut world), world.resource::<Assets<Mesh>>().len());
        let materials = world.resource::<Assets<StandardMaterial>>().len();
        assert_eq!(world.query::<&MicrowaveDigit>().iter(&world).count(), 28);
        assert_eq!(world.query::<&Smoke>().iter(&world).count(), 10);
        build(&mut world, &busier());
        assert_eq!(parts(&mut world), 1 + 22 + 5 * 5 + 2 + 1);
        let mut night = Map::bundled();
        night.theme = "night".into();
        build(&mut world, &night);
        // Nothing of the busier map is left behind, and rebuilding makes no new meshes or materials.
        assert_eq!(parts(&mut world), 14);
        assert_eq!(count(&mut world), entities);
        assert_eq!(world.resource::<Assets<Mesh>>().len(), meshes);
        assert_eq!(world.resource::<Assets<StandardMaterial>>().len(), materials);
        assert_eq!(*world.resource::<MapTheme>(), MapTheme("night".into()));
    }
    #[test]
    fn scenery_stands_where_the_map_says() {
        let mut world = world();
        build(&mut world, &Map::bundled());
        let mut named = world.query::<(&Name, &Transform)>();
        let mut find = |world: &World, name: &str| named.iter(world).filter(|(n, _)| n.as_str() == name).map(|(_, t)| *t).collect::<Vec<_>>();
        // The counter top is the floor; the bundled one is the slab the game always had.
        let counter = find(&world, "Counter")[0];
        assert!(counter.translation.distance(Vec3::new(0., 0.88, 2.95)) < 1e-5 && counter.scale.distance(Vec3::new(3.65, 0.045, 1.40)) < 1e-5);
        let deck = find(&world, "Deck")[0];
        assert!(deck.translation.distance(Vec3::new(1.505, FLOOR + 0.28 - 0.0125, 3.385)) < 1e-5 && deck.scale.distance(Vec3::new(0.51, 0.025, 0.45)) < 1e-5);
        let block = find(&world, "Block")[0];
        assert!(block.translation.distance(Vec3::new(-0.34, FLOOR + 0.125, 2.81)) < 1e-5 && block.scale.distance(Vec3::new(0.22, 0.25, 0.22)) < 1e-5);
        let turret = find(&world, "Turret: tracking pulse hitscan")[0];
        assert!(turret.translation.distance(Vec3::new(1.47, 1.20, 3.22)) < 1e-5);
        assert!((turret.rotation * Vec3::NEG_Z).distance(Vec3::NEG_Z) < 1e-6);
        assert_eq!(world.query::<&Turret>().iter(&world).map(|turret| turret.0).collect::<Vec<_>>(), [0], "the turret can be turned");
        // Ramps climb towards their `rise`: +x for the first, +z for the second.
        let ramps = find(&world, "Walkable ramp");
        assert!((ramps[0].rotation * Vec3::Z).distance(Vec3::X) < 1e-6 && (ramps[1].rotation * Vec3::Z).distance(Vec3::Z) < 1e-6);
        assert!(ramps[0].translation.distance(Vec3::new(0.9, FLOOR, 3.53)) < 1e-5);
    }
    #[test]
    fn appliances_face_the_side_the_map_names() {
        for (side, front) in [(Side::MinusZ, Vec3::NEG_Z), (Side::PlusZ, Vec3::Z), (Side::MinusX, Vec3::NEG_X), (Side::PlusX, Vec3::X)] {
            let (transform, across, deep) = facing([0., 0., 0.4, 0.2], side);
            assert!((transform.rotation * Vec3::NEG_Z).distance(front) < 1e-6, "{side:?}");
            assert_eq!((across, deep), if side.along_z() { (0.4, 0.2) } else { (0.2, 0.4) });
            assert_eq!(transform.translation, Vec3::new(0.2, FLOOR, 0.1));
        }
    }
    #[test]
    fn the_ramp_mesh_is_closed_and_faces_outwards() {
        let mesh = wedge();
        let positions: Vec<Vec3> = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap().iter().map(|p| Vec3::from_array(*p)).collect();
        let normals: Vec<Vec3> = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap().as_float3().unwrap().iter().map(|p| Vec3::from_array(*p)).collect();
        let Some(Indices::U32(indices)) = mesh.indices() else { panic!("the ramp is indexed") };
        assert_eq!(indices.len(), 3 * (2 + 2 + 2 + 1 + 1));
        for triangle in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| positions[triangle[i] as usize]);
            let winding = (b - a).cross(c - a).normalize();
            assert!(winding.distance(normals[triangle[0] as usize]) < 1e-5, "a face is wound inside out");
            // Every face looks away from the middle of the wedge.
            assert!(winding.dot(a - Vec3::new(0., 0.4, 0.2)) > 0.);
        }
    }
}

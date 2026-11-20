// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
use std::f32::consts::{FRAC_PI_2, PI};

use bevy::{
    camera::Exposure,
    core_pipeline::tonemapping::Tonemapping,
    light::{NotShadowCaster, PointLightShadowMap},
    prelude::*,
};

use crate::{box_mesh, hotdog::HotdogAssets, human, mat, maya};

// Distances are metres. The camera belongs to a 17 cm frank, not to the human.
pub(crate) const PACK_FLOOR: f32 = 0.9525;
pub(crate) const PACK_Z: f32 = -0.36;
const FRANK_SCALE: f32 = 0.075;

#[derive(Component)]
pub(super) struct Door;

#[derive(Component)]
pub(super) struct FridgeLamp(f32);

/// The kitchen's own lamps over the counter (the ceiling spot, and the fill light where there is no breakfast scene):
/// in a round they are turned down to a glimmer (`visuals::tune_room_lights`), and at the Lowest level put out.
#[derive(Component)]
pub struct RoomLamp;

#[derive(Resource)]
pub struct FridgeScene {
    started: Option<f64>,
    bulb: Handle<StandardMaterial>,
    announced: bool,
}

impl FridgeScene {
    pub fn elapsed(&self, now: f64) -> Option<f32> {
        self.started.map(|start| (now - start) as f32)
    }

    pub fn restart(&mut self, now: f64) {
        self.started = Some(now);
        self.announced = false;
    }

    /// The lamp's visible bulb: the multiplayer lobby dims and lights it with the lamp.
    pub fn bulb(&self) -> &Handle<StandardMaterial> {
        &self.bulb
    }
}

fn child_box(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    parent: Entity,
    material: &Handle<StandardMaterial>,
    size: Vec3,
    at: Vec3,
) {
    let child = box_mesh(commands, meshes, material.clone(), size, at);
    commands.entity(parent).add_child(child);
}

fn cylinder(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
    radius: f32,
    height: f32,
    at: Vec3,
) -> Entity {
    commands
        .spawn((
            Mesh3d(meshes.add(Cylinder::new(radius, height))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at),
        ))
        .id()
}

pub fn build(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    actors: bool,
) {
    let liner = mat(&mut materials, Color::srgb(0.77, 0.81, 0.79), 0.38);
    let shell = mat(&mut materials, Color::srgb(0.57, 0.61, 0.60), 0.31);
    let gasket = mat(&mut materials, Color::srgb(0.12, 0.15, 0.14), 0.88);
    let steel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.61, 0.63),
        metallic: 0.82,
        perceptual_roughness: 0.27,
        ..default()
    });
    let shelf = materials.add(StandardMaterial {
        base_color: Color::srgba(0.64, 0.78, 0.76, 0.30),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.22,
        cull_mode: None,
        ..default()
    });
    let red = mat(&mut materials, Color::srgb(0.53, 0.018, 0.035), 0.37);
    let tray = mat(&mut materials, Color::srgb(0.78, 0.63, 0.45), 0.64);
    let green = mat(&mut materials, Color::srgb(0.16, 0.23, 0.045), 0.53);
    let lid = mat(&mut materials, Color::srgb(0.11, 0.23, 0.075), 0.34);
    let cream = mat(&mut materials, Color::srgb(0.87, 0.85, 0.72), 0.67);
    let blue = mat(&mut materials, Color::srgb(0.035, 0.16, 0.28), 0.51);

    // An enclosed cabinet keeps room light out while the door is shut.
    for (size, at) in [
        (Vec3::new(0.82, 1.70, 0.055), Vec3::new(0.0, 1.02, -0.69)),
        (Vec3::new(0.045, 1.70, 0.72), Vec3::new(-0.39, 1.02, -0.335)),
        (Vec3::new(0.045, 1.70, 0.72), Vec3::new(0.39, 1.02, -0.335)),
        (Vec3::new(0.82, 0.045, 0.72), Vec3::new(0.0, 1.85, -0.335)),
        (Vec3::new(0.82, 0.06, 0.72), Vec3::new(0.0, 0.20, -0.335)),
    ] {
        let part = box_mesh(&mut commands, &mut meshes, liner.clone(), size, at);
        commands.entity(part).insert(crate::bake::Still);
    }
    // Shelf edges and mounting grooves provide a readable sense of scale. (The cabinet, its shelves' edges, grooves,
    // crisper and handles never move: `bake`.)
    for y in [0.56, 0.94, 1.28, 1.60] {
        let glass = box_mesh(
            &mut commands,
            &mut meshes,
            shelf.clone(),
            Vec3::new(0.73, 0.007, 0.61),
            Vec3::new(0.0, y, -0.335),
        );
        commands.entity(glass).insert((NotShadowCaster, crate::bake::Still));
        let part = box_mesh(
            &mut commands,
            &mut meshes,
            liner.clone(),
            Vec3::new(0.73, 0.013, 0.022),
            Vec3::new(0.0, y, -0.029),
        );
        commands.entity(part).insert(crate::bake::Still);
        for x in [-0.365, 0.365] {
            let part = box_mesh(
                &mut commands,
                &mut meshes,
                gasket.clone(),
                Vec3::new(0.004, 0.007, 0.57),
                Vec3::new(x, y - 0.015, -0.34),
            );
            commands.entity(part).insert(crate::bake::Still);
        }
    }
    let part = box_mesh(
        &mut commands,
        &mut meshes,
        liner.clone(),
        Vec3::new(0.72, 0.19, 0.57),
        Vec3::new(0.0, 0.34, -0.33),
    );
    commands.entity(part).insert(crate::bake::Still);
    for x in [-0.15, 0.15] {
        let part = box_mesh(
            &mut commands,
            &mut meshes,
            shell.clone(),
            Vec3::new(0.14, 0.018, 0.025),
            Vec3::new(x, 0.38, -0.034),
        );
        commands.entity(part).insert(crate::bake::Still);
    }

    let door = commands
        .spawn((
            Name::new("Refrigerator door hinge"),
            Door,
            Transform::from_xyz(-0.405, 0.0, 0.035),
        ))
        .id();
    child_box(
        &mut commands,
        &mut meshes,
        door,
        &shell,
        Vec3::new(0.82, 1.72, 0.075),
        Vec3::new(0.405, 1.025, 0.02),
    );
    child_box(
        &mut commands,
        &mut meshes,
        door,
        &liner,
        Vec3::new(0.73, 1.60, 0.021),
        Vec3::new(0.405, 1.025, -0.026),
    );
    for x in [0.025, 0.785] {
        child_box(
            &mut commands,
            &mut meshes,
            door,
            &gasket,
            Vec3::new(0.016, 1.64, 0.012),
            Vec3::new(x, 1.025, -0.046),
        );
    }
    for y in [0.21, 1.84] {
        child_box(
            &mut commands,
            &mut meshes,
            door,
            &gasket,
            Vec3::new(0.76, 0.016, 0.012),
            Vec3::new(0.405, y, -0.046),
        );
    }
    for y in [0.56, 1.13, 1.62] {
        child_box(
            &mut commands,
            &mut meshes,
            door,
            &liner,
            Vec3::new(0.62, 0.018, 0.105),
            Vec3::new(0.405, y, -0.075),
        );
        child_box(
            &mut commands,
            &mut meshes,
            door,
            &liner,
            Vec3::new(0.62, 0.075, 0.014),
            Vec3::new(0.405, y + 0.035, -0.125),
        );
    }
    child_box(
        &mut commands,
        &mut meshes,
        door,
        &steel,
        Vec3::new(0.025, 0.42, 0.03),
        Vec3::new(0.735, 1.10, 0.10),
    );

    // Six bare franks sit together in a sealed tray; the third is the player.
    let food = HotdogAssets::new(&mut meshes, &mut materials, &server);
    let package = commands
        .spawn((
            Name::new("Six-frank package carried by Maya"),
            maya::Part::Package,
            Transform::from_xyz(0.0, PACK_FLOOR, PACK_Z),
            Visibility::default(),
        ))
        .id();
    let player = commands
        .spawn((
            Name::new("Player hotdog and fixed eyes"),
            Transform::from_xyz(-0.017, 0.0, 0.0),
            Visibility::default(),
        ))
        .id();
    commands.entity(package).add_child(player);
    let tray_base = box_mesh(
        &mut commands,
        &mut meshes,
        tray,
        Vec3::new(0.228, 0.009, 0.218),
        Vec3::new(0.0, -0.0045, 0.0),
    );
    commands.entity(package).add_child(tray_base);
    for x in [-0.113, 0.113] {
        let edge = box_mesh(
            &mut commands,
            &mut meshes,
            red.clone(),
            Vec3::new(0.009, 0.013, 0.227),
            Vec3::new(x, 0.001, 0.0),
        );
        commands.entity(package).add_child(edge);
    }
    for z in [-0.112, 0.112] {
        let edge = box_mesh(
            &mut commands,
            &mut meshes,
            red.clone(),
            Vec3::new(0.23, 0.008, 0.012),
            Vec3::new(0.0, 0.0, z),
        );
        commands.entity(package).add_child(edge);
    }
    for (index, x) in [-0.085, -0.051, -0.017, 0.017, 0.051, 0.085]
        .into_iter()
        .enumerate()
    {
        let position = if index == 2 {
            Vec3::new(0.0, -0.22 * FRANK_SCALE, 0.0)
        } else {
            Vec3::new(x, -0.22 * FRANK_SCALE, 0.0)
        };
        let frank = food.spawn_frank(
            &mut commands,
            Transform::from_translation(position)
                .with_rotation(Quat::from_rotation_y(FRAC_PI_2))
                .with_scale(Vec3::splat(FRANK_SCALE)),
            index == 2,
        );
        if index == 2 {
            commands.entity(player).add_child(frank);
        } else {
            commands.entity(package).add_child(frank);
        }
    }
    let film = materials.add(StandardMaterial {
        base_color: Color::srgba(0.89, 0.94, 0.98, 0.035),
        perceptual_roughness: 0.13,
        reflectance: 0.65,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let film_entity = box_mesh(
        &mut commands,
        &mut meshes,
        film,
        Vec3::new(0.226, 0.0003, 0.220),
        Vec3::new(0.0, 0.038, 0.0),
    );
    commands
        .entity(film_entity)
        .insert((NotShadowCaster, crate::arena::PackageFilm));
    commands.entity(package).add_child(film_entity);
    let label = materials.add(StandardMaterial {
        base_color_texture: Some(server.load("materials/package_label.png")),
        perceptual_roughness: 0.65,
        ..default()
    });
    let label_entity = commands
        .spawn((
            Name::new("Red and white beef-franks package label"),
            Mesh3d(meshes.add(Plane3d::default().mesh().size(0.224, 0.032))),
            MeshMaterial3d(label),
            Transform::from_xyz(0.0, 0.005, 0.097).with_rotation(Quat::from_rotation_y(PI)),
        ))
        .id();
    commands
        .entity(label_entity)
        .insert(crate::arena::PackageLabel);
    commands.entity(package).add_child(label_entity);

    // Pickles and a milk carton are beside the pack, not giant arena props.
    let glass_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.51, 0.65, 0.34, 0.16),
        perceptual_roughness: 0.16,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let jar = cylinder(
        &mut commands,
        &mut meshes,
        &glass_material,
        0.052,
        0.17,
        Vec3::new(-0.237, 1.032, -0.31),
    );
    commands.entity(jar).insert(NotShadowCaster);
    cylinder(
        &mut commands,
        &mut meshes,
        &lid,
        0.054,
        0.018,
        Vec3::new(-0.237, 1.125, -0.31),
    );
    for (x, z, tilt) in [
        (-0.260, -0.327, 0.14),
        (-0.216, -0.327, -0.20),
        (-0.238, -0.286, 0.08),
    ] {
        commands.spawn((
            Mesh3d(meshes.add(Capsule3d::new(0.012, 0.105))),
            MeshMaterial3d(green.clone()),
            Transform::from_xyz(x, 1.024, z).with_rotation(Quat::from_rotation_z(tilt)),
        ));
    }
    box_mesh(
        &mut commands,
        &mut meshes,
        cream.clone(),
        Vec3::new(0.085, 0.22, 0.085),
        Vec3::new(0.235, 1.057, -0.415),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        blue.clone(),
        Vec3::new(0.087, 0.065, 0.087),
        Vec3::new(0.235, 1.075, -0.415),
    );
    cylinder(
        &mut commands,
        &mut meshes,
        &blue,
        0.014,
        0.013,
        Vec3::new(0.235, 1.174, -0.415),
    );
    cylinder(
        &mut commands,
        &mut meshes,
        &cream,
        0.040,
        0.11,
        Vec3::new(-0.20, 1.339, -0.42),
    );
    cylinder(
        &mut commands,
        &mut meshes,
        &red,
        0.043,
        0.012,
        Vec3::new(-0.20, 1.40, -0.42),
    );

    // Fixed first person: camera and body share a parent and never move separately.
    commands.entity(player).with_children(|parent| {
        parent.spawn((
            Camera3d::default(),
            maya::HotdogCamera,
            Projection::Perspective(PerspectiveProjection {
                fov: 82_f32.to_radians(),
                near: 0.001,
                far: 20.0,
                ..default()
            }),
            Exposure { ev100: 5.5 },
            Tonemapping::TonyMcMapface,
            Transform::from_xyz(0.0, 0.0325, 0.0).looking_at(Vec3::new(0.0, 0.205, 1.35), Vec3::Y),
        ));
    });

    // The bundled counter's kitchen; `level::build` fits it to the map being played (`fit_room`).
    build_room(&mut commands, &mut meshes, &mut materials, &Room::default());
    commands.insert_resource(Room::default());
    // The ceiling spot over the aisle between the fridge and the counter stays put, whatever the room.
    commands.spawn((
        RoomLamp,
        SpotLight {
            intensity: 2200.0,
            range: 6.0,
            radius: 0.12,
            color: Color::srgb(1.0, 0.79, 0.57),
            inner_angle: 0.55,
            outer_angle: 1.25,
            shadow_maps_enabled: true,
            shadow_depth_bias: 0.005,
            shadow_normal_bias: 0.05,
            ..default()
        },
        Transform::from_xyz(0.6, 2.42, 0.65).looking_at(Vec3::new(0.0, 0.65, 1.1), Vec3::Y),
    ));
    if actors {
        maya::build(&mut commands, &mut meshes, &mut materials, &mut images);
    } else {
        commands.spawn((
            RoomLamp,
            PointLight {
                intensity: 210.,
                range: 5.,
                radius: 0.2,
                color: Color::srgb(1., 0.88, 0.74),
                ..default()
            },
            Transform::from_xyz(0.2, 1.9, 2.95),
        ));
    }

    // The visible lamp and its light switch on together when the seal breaks.
    let bulb = materials.add(StandardMaterial {
        base_color: Color::srgb(0.72, 0.82, 0.88),
        perceptual_roughness: 0.4,
        ..default()
    });
    box_mesh(
        &mut commands,
        &mut meshes,
        bulb.clone(),
        Vec3::new(0.012, 0.10, 0.16),
        Vec3::new(0.363, 1.194, -0.19),
    );
    for (intensity, at, shadows) in [
        (145.0, Vec3::new(0.32, 1.19, -0.17), true),
        (8.0, Vec3::new(-0.24, 1.15, -0.47), false),
    ] {
        commands.spawn((
            FridgeLamp(intensity),
            PointLight {
                intensity: 0.0,
                range: 1.5,
                radius: 0.025,
                color: Color::srgb(0.77, 0.87, 1.0),
                shadow_maps_enabled: shadows,
                shadow_depth_bias: 0.002,
                shadow_normal_bias: 0.02,
                ..default()
            },
            Transform::from_translation(at),
        ));
    }
    commands.insert_resource(PointLightShadowMap { size: 1024 });
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.77, 0.85, 1.0),
        brightness: 0.025,
        ..default()
    });
    commands.insert_resource(FridgeScene {
        started: None,
        bulb,
        announced: false,
    });
    info!("Pack faces ready: five smiling sleepers and one self-aware terrified hotdog");
    info!("Fridge POV ready: eyes fixed to a packaged 17 cm hotdog; indoor exposure");
}

/// The kitchen's walls, floor, ceiling and cupboards: what `fit_room` takes down and builds again at
/// another size.
#[derive(Component)]
pub struct RoomPart;

/// The kitchen round the counter: where its side walls (x) and back wall (z) stand, and how far along x
/// the run of cupboards and worktop under the back of the counter reaches. The bundled counter gets the
/// room the game always had. A longer or deeper counter pushes the walls out, keeping them 0.7 m past
/// its ends and just behind its back, and the run with them, up to the size `kitchen_sim::map::ROOM`
/// allows. The room grows away from the fridge: its front, where the fridge stands facing the middle of
/// the counter, never moves.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct Room {
    pub left: f32,
    pub right: f32,
    pub back: f32,
    pub run: [f32; 2],
}
impl Default for Room {
    fn default() -> Self {
        Room { left: -2.5, right: 2.5, back: 3.7, run: [-1.8, 1.8] }
    }
}
impl Room {
    /// The room round a map's `bounds` ([min x, min z, max x, max z]).
    pub fn fitting(bounds: [f64; 4]) -> Room {
        let [lowest, highest, deepest] = kitchen_sim::map::ROOM;
        let (x0, x1, z1) = (bounds[0].max(lowest), bounds[2].min(highest), bounds[3].min(deepest));
        Room {
            left: (x0 - 0.7).min(-2.5) as f32,
            right: (x1 + 0.7).max(2.5) as f32,
            back: (z1 + 0.07).max(3.7) as f32,
            run: [x0.min(-1.8) as f32, x1.max(1.8) as f32],
        }
    }
    /// The cupboard under the back of the counter, as its lowest and highest corners.
    pub fn cupboard(&self) -> (Vec3, Vec3) {
        let back = self.back - Room::default().back;
        (Vec3::new(self.run[0], 0., 3.02 + back), Vec3::new(self.run[1], 0.84, 3.62 + back))
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Finish {
    Wall,
    Tile,
    Wood,
    Stone,
    Black,
}

/// The kitchen box by box: (finish, size, middle). Every part is the bundled kitchen's own, moved and
/// stretched by however far the room reaches past that one, so that it is built exactly as it always was.
fn room_boxes(room: &Room) -> Vec<(Finish, Vec3, Vec3)> {
    let base = Room::default();
    // How far the back wall and each end of the run stand past the bundled kitchen's: all nought for it.
    let back = room.back - base.back;
    let (start, end) = (base.run[0] - room.run[0], room.run[1] - base.run[1]);
    let (wide, middle) = (room.right - room.left, (room.left + room.right) / 2.);
    let (long, centre) = (start + end, (end - start) / 2.);
    let mut boxes = vec![];
    for (size, at) in [
        (Vec3::new(wide, 0.08, 5.5 + back), Vec3::new(middle, -0.04, 1.7 + back / 2.)),
        (Vec3::new(wide, 0.1, 5.5 + back), Vec3::new(middle, 2.6, 1.7 + back / 2.)),
        (Vec3::new(wide, 2.6, 0.08), Vec3::new(middle, 1.3, 3.7 + back)),
        (Vec3::new(0.08, 2.6, 5.5 + back), Vec3::new(room.left, 1.3, 1.7 + back / 2.)),
        (Vec3::new(0.08, 2.6, 5.5 + back), Vec3::new(room.right, 1.3, 1.7 + back / 2.)),
    ] {
        boxes.push((Finish::Wall, size, at));
    }
    // Lines along the floor every 0.6 m across it, out to 0.7 m from either side wall.
    let mut lines = vec![-1.8, -1.2, -0.6, 0.0, 0.6, 1.2, 1.8];
    for side in [-1., 1.] {
        let mut x = side * 2.4;
        while x >= room.left + 0.69 && x <= room.right - 0.69 {
            lines.push(x);
            x += side * 0.6;
        }
    }
    for x in lines {
        boxes.push((Finish::Tile, Vec3::new(0.005, 0.001, 4.7 + back), Vec3::new(x, 0.002, 1.6 + back / 2.)));
    }
    boxes.push((Finish::Wood, Vec3::new(3.6 + long, 0.84, 0.60), Vec3::new(centre, 0.42, 3.32 + back)));
    boxes.push((Finish::Stone, Vec3::new(3.65 + long, 0.045, 0.65), Vec3::new(centre, 0.88, 3.30 + back)));
    for (x, width) in wall_doors(room) {
        boxes.push((Finish::Wood, Vec3::new(width, 0.70, 0.31), Vec3::new(x, 1.95, 3.5 + back)));
        boxes.push((Finish::Black, Vec3::new(0.20, 0.014, 0.022), Vec3::new(x, 1.68, 3.33 + back)));
    }
    boxes
}

/// The wall cupboards over the run, as (middle x, width): the bundled kitchen's five, or as many as fill a longer
/// run. (`kitchen::cabinets` dresses them.)
pub fn wall_doors(room: &Room) -> Vec<(f32, f32)> {
    let base = Room::default();
    let (start, end) = (base.run[0] - room.run[0], room.run[1] - base.run[1]);
    if start + end == 0. {
        [-1.45, -0.72, 0.0, 0.72, 1.45].map(|x| (x, 0.68)).to_vec()
    } else {
        let length = room.run[1] - room.run[0];
        let count = (length / 0.72).round().max(1.);
        let pitch = length / count;
        (0..count as usize).map(|i| (room.run[0] + pitch * (i as f32 + 0.5), pitch - 0.04)).collect()
    }
}

fn build_room(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    room: &Room,
) {
    let wall = mat(materials, Color::srgb(0.54, 0.52, 0.44), 0.94);
    let wood = mat(materials, Color::srgb(0.24, 0.13, 0.070), 0.64);
    let stone = mat(materials, Color::srgb(0.35, 0.38, 0.36), 0.38);
    let tile = mat(materials, Color::srgb(0.40, 0.37, 0.30), 0.78);
    let black = mat(materials, Color::srgb(0.025, 0.030, 0.027), 0.3);
    for (finish, size, at) in room_boxes(room) {
        let material = match finish {
            Finish::Wall => &wall,
            Finish::Tile => &tile,
            Finish::Wood => &wood,
            Finish::Stone => &stone,
            Finish::Black => &black,
        };
        let part = box_mesh(commands, meshes, material.clone(), size, at);
        commands.entity(part).insert((RoomPart, crate::bake::Still));
    }
    // The microwave on the far counter is a block of the map: see level.rs.
}

/// Builds the kitchen again round a map whose counter needs another room than the one standing: the
/// walls, floor, ceiling and cupboards go and come back at the new size, and nothing else of the kitchen
/// moves (the fridge, its lamps, the ceiling spot). `level::build` calls this for every map; it does
/// nothing where no kitchen was built.
pub fn fit_room(world: &mut World, bounds: [f64; 4]) {
    let room = Room::fitting(bounds);
    if world.get_resource::<Room>().is_none_or(|built| *built == room) {
        return;
    }
    let old: Vec<Entity> = world.query_filtered::<Entity, With<RoomPart>>().iter(world).collect();
    for entity in old {
        world.despawn(entity);
    }
    world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
        world.resource_scope(|world, mut materials: Mut<Assets<StandardMaterial>>| {
            build_room(&mut world.commands(), &mut meshes, &mut materials, &room);
        });
    });
    world.flush();
    world.insert_resource(room);
    info!("Kitchen walls at x = {:.2} and {:.2}, back wall at z = {:.2}", room.left, room.right, room.back);
}

#[allow(clippy::too_many_arguments)]
pub fn open_door(
    time: Res<Time<Real>>,
    game: Option<Res<crate::arena::Game>>,
    scene: Option<ResMut<FridgeScene>>,
    cast: Query<(), With<human::Human>>,
    server: Res<AssetServer>,
    mut waited: Local<f32>,
    mut doors: Query<&mut Transform, With<Door>>,
    mut lamps: Query<(&FridgeLamp, &mut PointLight)>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    let combat = game.is_some_and(|g| g.playing());
    // The breakfast scene begins once its people are there (never in multiplayer, where the lobby opens the door) and
    // the textures have arrived (15 s at the most), so that nothing is drawn plain and then changes as the door opens.
    if !combat && cast.is_empty() {
        return;
    }
    if !combat && scene.started.is_none() && *waited < 15. && !textures_arrived(&server, &materials) {
        *waited += time.delta_secs();
        return;
    }
    let started = *scene.started.get_or_insert(time.elapsed_secs_f64());
    let elapsed = if combat {
        100.0
    } else {
        (time.elapsed_secs_f64() - started) as f32
    };
    let t = ((elapsed - 1.8) / 3.8).clamp(0.0, 1.0);
    let opening = t * t * (3.0 - 2.0 * t);
    // Written only when they change: once the door is open (and through every round) they hold still, and a
    // touched transform, light or material is sent to the GPU again.
    let rotation = Quat::from_rotation_y(-opening * 108_f32.to_radians());
    for mut door in &mut doors {
        if door.rotation != rotation {
            door.rotation = rotation;
        }
    }
    let light = ((elapsed - 2.05) / 0.18).clamp(0.0, 1.0);
    for (lamp, mut point) in &mut lamps {
        if point.intensity != lamp.0 * light {
            point.intensity = lamp.0 * light;
        }
    }
    if ambient.brightness != 0.025 + light * 14.0 {
        ambient.brightness = 0.025 + light * 14.0;
    }
    let glow = LinearRgba::new(35.0 * light, 45.0 * light, 60.0 * light, 1.0);
    if materials.get(&scene.bulb).is_some_and(|bulb| bulb.emissive != glow) {
        if let Some(mut bulb) = materials.get_mut(&scene.bulb) {
            bulb.emissive = glow;
        }
    }
    if opening >= 1.0 && !scene.announced {
        scene.announced = true;
        info!("Fridge door open: Maya is ready to collect the hotdog package");
    }
}

/// Whether every texture the materials ask for has loaded (or failed to).
fn textures_arrived(server: &AssetServer, materials: &Assets<StandardMaterial>) -> bool {
    use bevy::asset::LoadState;
    materials.iter().all(|(_, m)| {
        [&m.base_color_texture, &m.normal_map_texture, &m.metallic_roughness_texture]
            .into_iter()
            .flatten()
            .all(|image| !matches!(server.get_load_state(image.id()), Some(LoadState::Loading | LoadState::NotLoaded)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::map::Map;

    fn bounds(path: &str) -> [f64; 4] {
        let text = std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        Map::parse(&text).unwrap().bounds
    }
    fn sorted(mut places: Vec<Vec3>) -> Vec<Vec3> {
        places.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).unwrap());
        places
    }
    fn standing(world: &mut World) -> Vec<Vec3> {
        sorted(world.query_filtered::<&Transform, With<RoomPart>>().iter(world).map(|t| t.translation).collect())
    }
    fn planned(room: &Room) -> Vec<Vec3> {
        sorted(room_boxes(room).into_iter().map(|(_, _, at)| at).collect())
    }

    #[test]
    fn the_bundled_counter_gets_the_kitchen_it_always_had() {
        for path in ["assets/maps/counter-siege.json", "assets/maps/counter-siege-v2.json"] {
            assert_eq!(Room::fitting(bounds(path)), Room::default(), "{path}");
        }
        // The boxes the kitchen was built of before it could grow, to the last bit.
        let mut before = vec![
            (Finish::Wall, Vec3::new(5.0, 0.08, 5.5), Vec3::new(0.0, -0.04, 1.7)),
            (Finish::Wall, Vec3::new(5.0, 0.1, 5.5), Vec3::new(0.0, 2.6, 1.7)),
            (Finish::Wall, Vec3::new(5.0, 2.6, 0.08), Vec3::new(0.0, 1.3, 3.7)),
            (Finish::Wall, Vec3::new(0.08, 2.6, 5.5), Vec3::new(-2.5, 1.3, 1.7)),
            (Finish::Wall, Vec3::new(0.08, 2.6, 5.5), Vec3::new(2.5, 1.3, 1.7)),
        ];
        for x in [-1.8, -1.2, -0.6, 0.0, 0.6, 1.2, 1.8] {
            before.push((Finish::Tile, Vec3::new(0.005, 0.001, 4.7), Vec3::new(x, 0.002, 1.6)));
        }
        before.push((Finish::Wood, Vec3::new(3.6, 0.84, 0.60), Vec3::new(0.0, 0.42, 3.32)));
        before.push((Finish::Stone, Vec3::new(3.65, 0.045, 0.65), Vec3::new(0.0, 0.88, 3.30)));
        for x in [-1.45, -0.72, 0.0, 0.72, 1.45] {
            before.push((Finish::Wood, Vec3::new(0.68, 0.70, 0.31), Vec3::new(x, 1.95, 3.5)));
            before.push((Finish::Black, Vec3::new(0.20, 0.014, 0.022), Vec3::new(x, 1.68, 3.33)));
        }
        assert_eq!(room_boxes(&Room::default()), before);
        // And the cupboard the ragdolls meet (`death::scenery`) is the one they always met.
        assert_eq!(Room::default().cupboard(), (Vec3::new(-1.8, 0., 3.02), Vec3::new(1.8, 0.84, 3.62)));
    }

    #[test]
    fn a_longer_or_deeper_counter_stands_inside_walls_moved_out_with_the_same_margins() {
        let fort = bounds("assets/maps/pillow-fort.json");
        assert_eq!(fort, [-3., 2.2, 3., 4.]);
        // The bundled Forts map, a deep siege counter, a long lopsided one, and the largest the kitchen holds.
        for map in [fort, [-1.8, 2.27, 1.8, 4.6], [-4.1, 2.27, 2.2, 3.63], [-4.5, 1., 4.5, 5.]] {
            let room = Room::fitting(map);
            let [x0, z0, x1, z1] = map.map(|v| v as f32);
            assert!(room.left <= x0 - 0.7 + 1e-5 && room.right >= x1 + 0.7 - 1e-5 && room.back >= z1 + 0.07 - 1e-5, "{map:?}: {room:?}");
            assert!(room.left <= -2.5 && room.right >= 2.5 && room.back >= 3.7, "never smaller than the bundled kitchen");
            // The counter top `level::build` draws (0.025 m past its ends, 0.02 m past its back) is clear of every
            // wall's inner face; the side walls stand a whole 0.66 m off.
            assert!(room.left + 0.04 <= x0 - 0.66 + 1e-5 && room.right - 0.04 >= x1 + 0.66 - 1e-5 && room.back - 0.04 > z1 + 0.02);
            let boxes = room_boxes(&room);
            let extent = |(_, size, at): &(Finish, Vec3, Vec3)| (*at - *size / 2., *at + *size / 2.);
            // Floor and ceiling run from wall to wall, and from behind the fridge (z = -1.05, where the room is open) to past the back wall.
            for part in &boxes[..2] {
                let (low, high) = extent(part);
                assert!((low.x - room.left).abs() < 1e-4 && (high.x - room.right).abs() < 1e-4, "{part:?}");
                assert!((low.z + 1.05).abs() < 1e-4 && high.z >= room.back + 0.75 - 1e-4, "{part:?}");
            }
            // The back wall spans the side walls; they reach from the front to past it.
            let (low, high) = extent(&boxes[2]);
            assert!((boxes[2].2.z - room.back).abs() < 1e-5 && low.x <= room.left + 1e-4 && high.x >= room.right - 1e-4);
            for (part, x) in [(&boxes[3], room.left), (&boxes[4], room.right)] {
                let (low, high) = extent(part);
                assert!(part.2.x == x && (low.z + 1.05).abs() < 1e-4 && high.z >= room.back);
            }
            // The cupboard and worktop run under the whole counter, against the back wall; the wall cupboards
            // above them fill it end to end without overlapping. Nothing comes nearer the fridge than the
            // counter does, and everything stands inside the walls.
            let (low, high) = room.cupboard();
            assert!(low.x <= x0 && high.x >= x1 && high.z > room.back - 0.1 && high.z < room.back - 0.04);
            let mut doors: Vec<(f32, f32)> =
                boxes.iter().filter(|(f, size, _)| *f == Finish::Wood && size.y == 0.70).map(|b| (extent(b).0.x, extent(b).1.x)).collect();
            doors.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!(doors[0].0 < room.run[0] + 0.05 && doors[doors.len() - 1].1 > room.run[1] - 0.05);
            assert!(doors.windows(2).all(|pair| pair[0].1 < pair[1].0));
            for part in &boxes[5..] {
                let (low, high) = extent(part);
                // (The lines on the floor run on under the back wall, as they always did.)
                assert!(low.x >= room.left && high.x <= room.right && high.z < room.back + if part.0 == Finish::Tile { 0.3 } else { 0. }, "{part:?}");
                assert!(part.0 == Finish::Tile || low.z >= z0.min(2.27) + 0.6, "{part:?} is in the way of the fridge");
            }
        }
        // A counter bigger than the largest kitchen pokes through its walls, and the map's analysis says so.
        let most = Room::fitting([-20., 0., 20., 20.]);
        assert!((most.left + 5.2).abs() < 1e-5 && (most.right - 5.2).abs() < 1e-5 && (most.back - 5.07).abs() < 1e-5);
    }

    #[test]
    fn a_new_map_moves_the_walls_and_leaves_none_of_the_old_ones_standing() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        // Without the kitchen (the `counter-map` scene) a map builds no room.
        crate::level::build(&mut world, &Map::bundled());
        assert!(standing(&mut world).is_empty());
        // The kitchen as `build` puts it up.
        world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|world, mut materials: Mut<Assets<StandardMaterial>>| {
                build_room(&mut world.commands(), &mut meshes, &mut materials, &Room::default());
            });
        });
        world.flush();
        world.insert_resource(Room::default());
        assert_eq!(standing(&mut world), planned(&Room::default()));
        // The editor lengthens and deepens the counter: `level::build` (live reload) moves the walls out.
        let mut longer = Map::bundled();
        longer.bounds = [-3., 2.2, 3., 4.];
        crate::level::build(&mut world, &longer);
        let room = *world.resource::<Room>();
        assert_eq!(room, Room::fitting(longer.bounds));
        assert_eq!(standing(&mut world), planned(&room));
        let old = planned(&Room::default());
        assert!(standing(&mut world).iter().all(|at| !old.contains(at)), "a part of the old kitchen is left standing");
        // And back: the bundled kitchen again, part for part.
        crate::level::build(&mut world, &Map::bundled());
        assert_eq!(*world.resource::<Room>(), Room::default());
        assert_eq!(standing(&mut world), planned(&Room::default()));
        // The same bounds again build nothing anew.
        let parts: Vec<Entity> = world.query_filtered::<Entity, With<RoomPart>>().iter(&world).collect();
        fit_room(&mut world, Map::bundled().bounds);
        assert_eq!(world.query_filtered::<Entity, With<RoomPart>>().iter(&world).collect::<Vec<_>>(), parts);
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
#![recursion_limit = "256"]
// A release build on Windows opens no console window beside the game's.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bevy::{asset::AssetMetaCheck, prelude::*};
mod arena;
mod bake;
mod cards;
mod cats;
mod cloth;
mod controls;
mod damage;
mod death;
mod dojo;
mod deathcam;
mod fight;
mod forts;
mod fu;
mod fridge;
mod griddle;
mod hints;
mod hotdog;
mod hud;
mod icons;
mod kitchen;
mod kungfu;
mod human;
mod level;
mod lobby;
mod loose;
mod maya;
mod menu;
mod network;
mod ooze;
mod player_model;
mod prefs;
mod preview;
mod quality;
mod queries;
mod ragdoll;
mod reload;
mod replay;
mod runtime;
mod scope;
mod settings;
mod slowmo;
mod spawns;
mod squirt;
mod tags;
mod telemetry;
mod topsy;
mod touch;
mod tune;
mod ultra;
mod viewmodel;
mod visuals;
mod weapons;
mod whip;

fn main() {
    runtime::check_cpu();
    let options = runtime::Options::read();
    // An invalid map never panics: natively the reason is printed and the game exits; in a browser the
    // bundled map is played instead and the page shows the reason.
    let map = level::choose(&options).unwrap_or_else(|reason| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            eprintln!("Cannot load the map {reason}");
            std::process::exit(1);
        }
        #[cfg(target_arch = "wasm32")]
        {
            level::notice(&format!("Map not loaded: {reason}. Playing the bundled map instead."));
            kitchen_sim::map::Map::bundled()
        }
    });
    let present_mode = if options.uncapped {
        bevy::window::PresentMode::AutoNoVsync
    } else {
        bevy::window::PresentMode::AutoVsync
    };
    App::new()
        .insert_resource(lobby::Lobby::new(&options))
        .insert_resource(options)
        .insert_resource(level::MapTheme(map.theme.clone()))
        .insert_resource(level::StartMap(map))
        .add_plugins(
            DefaultPlugins
                .build()
                // The game reads no gamepads: without gilrs nobody polls them every frame.
                .disable::<bevy::gilrs::GilrsPlugin>()
                .set(AssetPlugin {
                    meta_check: AssetMetaCheck::Never,
                    file_path: runtime::asset_directory(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "HOTDOG SIMULATOR".into(),
                        resolution: (1280, 720).into(),
                        present_mode,
                        canvas: Some("#game".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: false,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(player_model::bend::Plugin)
        .add_plugins(visuals::VisualsPlugin)
        .add_plugins(death::Plugin)
        .add_plugins((viewmodel::Plugin, deathcam::Plugin, forts::Plugin, arena::splatter::Plugin, whip::Plugin))
        .add_plugins(touch::Plugin)
        .add_plugins(bake::Plugin)
        .add_plugins(kungfu::Plugin)
        .add_plugins(tags::Plugin)
        .add_plugins(hints::Plugin)
        .add_plugins(hud::Plugin)
        .add_plugins((damage::Plugin, slowmo::Plugin, topsy::Plugin))
        .add_plugins(griddle::Plugin)
        .add_plugins(reload::Plugin)
        .add_plugins(dojo::Plugin)
        .add_plugins(fu::Plugin)
        .add_plugins(fight::Plugin)
        .add_plugins(menu::Plugin)
        .add_plugins(preview::Plugin)
        .add_plugins(scope::Plugin)
        .add_plugins(cards::CardsPlugin)
        .add_plugins(replay::Plugin)
        // `--ultra`, `--raytraced`, `--meshlets`: the native Ultra tier (README); nothing without them.
        .add_plugins(ultra::Plugin)
        .insert_resource(ClearColor(Color::srgb(0.006, 0.009, 0.014)))
        .init_resource::<telemetry::Telemetry>()
        .init_resource::<network::Network>()
        .add_systems(Startup, runtime::log_version)
        .add_systems(Startup, (build_scene, arena::setup, arena::build_map).chain())
        .add_systems(
            Update,
            (
                runtime::software_lighting,
                human::follow_detail,
                human::idle,
                fridge::open_door,
                maya::advance,
                maya::animate,
                maya::animate_camera,
                hotdog::animate_franks,
                level::watch,
                network::receive,
                arena::tick,
                tune::bridge,
                network::send,
                cats::animate,
                ooze::update,
            )
                .chain(),
        )
        // Multiplayer's lobby holds the camera, the fridge door and its lamp between rounds: after everything else has placed them.
        .add_systems(
            PostUpdate,
            lobby::update
                .before(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        )
        .add_systems(Last, telemetry::sample)
        .run();
}

fn build_scene(
    commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    server: Res<AssetServer>,
    images: ResMut<Assets<Image>>,
    options: Res<runtime::Options>,
) {
    if cfg!(feature = "counter-map") {
        build_kitchen(commands, meshes, materials, server, images);
    } else {
        fridge::build(
            commands,
            meshes,
            materials,
            server,
            images,
            !options.skip_intro,
        );
    }
}

fn mat(
    mats: &mut Assets<StandardMaterial>,
    color: Color,
    roughness: f32,
) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        ..default()
    })
}

fn box_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<StandardMaterial>,
    size: Vec3,
    at: Vec3,
) -> Entity {
    commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(material),
            Transform::from_translation(at),
        ))
        .id()
}

fn ball(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<StandardMaterial>,
    radius: f32,
    at: Vec3,
    scale: Vec3,
) -> Entity {
    commands
        .spawn((
            Mesh3d(meshes.add(Sphere::new(radius).mesh().uv(20, 12))),
            MeshMaterial3d(material),
            Transform::from_translation(at).with_scale(scale),
        ))
        .id()
}

fn build_kitchen(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    let floor = mat(&mut materials, Color::srgb(0.34, 0.29, 0.23), 0.88);
    let wall = mat(&mut materials, Color::srgb(0.67, 0.64, 0.54), 0.92);
    let tile = mat(&mut materials, Color::srgb(0.78, 0.76, 0.67), 0.72);
    let wood = mat(&mut materials, Color::srgb(0.35, 0.19, 0.10), 0.56);
    let dark_wood = mat(&mut materials, Color::srgb(0.18, 0.10, 0.07), 0.72);
    let steel = mat(&mut materials, Color::srgb(0.40, 0.46, 0.45), 0.27);
    let black = mat(&mut materials, Color::srgb(0.035, 0.045, 0.044), 0.25);
    let brass = mat(&mut materials, Color::srgb(0.72, 0.48, 0.16), 0.3);
    let eye = mat(&mut materials, Color::srgb(0.035, 0.022, 0.018), 0.18);
    let white = mat(&mut materials, Color::srgb(1.0, 0.97, 0.84), 0.22);
    let cat_fur = mat(&mut materials, Color::srgb(0.16, 0.19, 0.20), 0.83);
    let cat_light = mat(&mut materials, Color::srgb(0.54, 0.48, 0.39), 0.88);
    let green = mat(&mut materials, Color::srgb(0.42, 0.66, 0.31), 0.45);

    // A compact, fully 3D kitchen arena. The counter is the dominant high-ground objective.
    box_mesh(
        &mut commands,
        &mut meshes,
        floor.clone(),
        Vec3::new(34.0, 0.4, 32.0),
        Vec3::new(0.0, -0.2, 0.0),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        tile.clone(),
        Vec3::new(34.0, 0.03, 32.0),
        Vec3::new(0.0, 0.015, 0.0),
    );
    // Three walls leave a broad, readable approach from the living room.
    box_mesh(
        &mut commands,
        &mut meshes,
        wall.clone(),
        Vec3::new(34.0, 9.0, 0.45),
        Vec3::new(0.0, 4.5, -16.0),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        wall.clone(),
        Vec3::new(0.45, 9.0, 32.0),
        Vec3::new(-17.0, 4.5, 0.0),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        wall.clone(),
        Vec3::new(0.45, 9.0, 32.0),
        Vec3::new(17.0, 4.5, 0.0),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        wall.clone(),
        Vec3::new(34.0, 9.0, 0.45),
        Vec3::new(0.0, 4.5, 16.0),
    );
    // A partial second-storey landing frames the room and gives the upper route a purpose.
    box_mesh(
        &mut commands,
        &mut meshes,
        dark_wood.clone(),
        Vec3::new(12.0, 0.32, 5.0),
        Vec3::new(0.0, 5.1, -11.0),
    );
    for x in [-5.7, 5.7] {
        box_mesh(
            &mut commands,
            &mut meshes,
            wood.clone(),
            Vec3::new(0.18, 1.0, 0.18),
            Vec3::new(x, 5.75, -8.8),
        );
        box_mesh(
            &mut commands,
            &mut meshes,
            wood.clone(),
            Vec3::new(0.18, 1.0, 0.18),
            Vec3::new(x, 5.75, -13.2),
        );
    }
    box_mesh(
        &mut commands,
        &mut meshes,
        wood.clone(),
        Vec3::new(12.0, 0.12, 0.14),
        Vec3::new(0.0, 6.15, -8.8),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        wood.clone(),
        Vec3::new(12.0, 0.12, 0.14),
        Vec3::new(0.0, 6.15, -13.2),
    );
    // Stair runs: broad steps are readable as routes, not a decorative ramp.
    for i in 0..10 {
        let t = i as f32;
        box_mesh(
            &mut commands,
            &mut meshes,
            wood.clone(),
            Vec3::new(3.2, 0.28, 0.92),
            Vec3::new(10.8 - t * 0.82, 0.28 + t * 0.5, -8.0 - t * 0.52),
        );
    }

    // Counter base and thick stone top; defenders spawn on the top surface.
    box_mesh(
        &mut commands,
        &mut meshes,
        dark_wood.clone(),
        Vec3::new(14.0, 2.45, 5.0),
        Vec3::new(0.0, 1.28, 0.0),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        steel.clone(),
        Vec3::new(14.5, 0.30, 5.5),
        Vec3::new(0.0, 2.66, 0.0),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        wood.clone(),
        Vec3::new(14.1, 0.12, 5.1),
        Vec3::new(0.0, 2.87, 0.0),
    );
    // Two obvious climbing routes from floor height to counter height.
    for (x, direction) in [(-10.0, 1.0), (10.0, -1.0)] {
        for i in 0..8 {
            let h = i as f32;
            box_mesh(
                &mut commands,
                &mut meshes,
                wood.clone(),
                Vec3::new(3.4, 0.34, 0.8),
                Vec3::new(x + direction * h * 0.60, 0.31 + h * 0.35, 4.7 - h * 0.62),
            );
        }
        box_mesh(
            &mut commands,
            &mut meshes,
            steel.clone(),
            Vec3::new(4.4, 0.12, 1.4),
            Vec3::new(x + direction * 5.0, 3.0, -0.3),
        );
    }
    // Cover on the approach: overturned chair, stools, pantry boxes, and fridge.
    for (at, size) in [
        (Vec3::new(-7.5, 0.9, -5.8), Vec3::new(2.3, 1.8, 1.5)),
        (Vec3::new(7.1, 1.0, -7.4), Vec3::new(2.1, 2.0, 1.7)),
        (Vec3::new(-12.0, 1.25, 5.0), Vec3::new(2.8, 2.5, 2.5)),
        (Vec3::new(12.4, 1.8, -1.5), Vec3::new(2.8, 3.6, 2.6)),
    ] {
        box_mesh(&mut commands, &mut meshes, wood.clone(), size, at);
    }
    box_mesh(
        &mut commands,
        &mut meshes,
        steel.clone(),
        Vec3::new(3.4, 6.0, 3.0),
        Vec3::new(-13.8, 3.0, -11.8),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        black.clone(),
        Vec3::new(2.5, 2.4, 0.05),
        Vec3::new(-13.8, 3.8, -10.25),
    );
    box_mesh(
        &mut commands,
        &mut meshes,
        white.clone(),
        Vec3::new(0.09, 0.12, 0.08),
        Vec3::new(-12.4, 5.3, -10.2),
    );

    // Turrets are placed high on both ends of the counter, aimed down each approach lane.
    for (x, yaw) in [(-5.2, 0.5), (5.2, -0.5)] {
        box_mesh(
            &mut commands,
            &mut meshes,
            black.clone(),
            Vec3::new(1.1, 0.42, 1.0),
            Vec3::new(x, 3.18, -0.1),
        );
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(0.46, 0.30))),
            MeshMaterial3d(brass.clone()),
            Transform::from_xyz(x, 3.52, -0.1),
        ));
        let barrel = box_mesh(
            &mut commands,
            &mut meshes,
            steel.clone(),
            Vec3::new(0.22, 0.22, 1.55),
            Vec3::new(x, 3.65, -1.05),
        );
        commands
            .entity(barrel)
            .insert(Transform::from_xyz(x, 3.65, -1.05).with_rotation(Quat::from_rotation_y(yaw)));
        for side in [-0.28, 0.28] {
            commands.spawn((
                Mesh3d(meshes.add(Cylinder::new(0.07, 0.55))),
                MeshMaterial3d(brass.clone()),
                Transform::from_xyz(x + side, 3.63, -1.55)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));
        }
    }

    // Food geometry and PBR maps are shared by the five defenders.
    let hotdogs = hotdog::HotdogAssets::new(&mut meshes, &mut materials, &server);
    for (x, z, yaw) in [
        (-5.4, 1.0, -0.10),
        (-2.5, -1.1, 0.08),
        (1.0, -1.1, -0.08),
        (4.3, 1.2, 0.12),
        (0.0, 1.7, 0.0),
    ] {
        hotdogs.spawn(&mut commands, Vec3::new(x, 2.918, z), yaw);
    }
    // The host behind the counter: Maya's husband, a giant here, made in code like her (human.rs).
    let cast = human::Cast::new(&mut meshes, &mut materials, &mut images, true);
    human::spawn_host(&mut commands, &cast);
    commands.insert_resource(cast);
    // Alley-cat raiders as simple low-poly 3D meshes positioned on their ground-level lanes.
    for (x, z) in [
        (-12.0, 1.5),
        (12.0, 2.0),
        (-8.0, -6.5),
        (8.0, -10.0),
        (0.0, -13.2),
    ] {
        make_cat(
            &mut commands,
            &mut meshes,
            x,
            0.15,
            z,
            &cat_fur,
            &cat_light,
            &eye,
            &green,
        );
    }
    // A few oversized ingredient props read clearly at toy-scale without crowding the lanes.
    for (x, z) in [(-8.0, -1.6), (8.0, -2.7)] {
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(0.42, 1.3))),
            MeshMaterial3d(green.clone()),
            Transform::from_xyz(x, 0.7, z),
        ));
    }

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 4.3, 6.3).looking_at(Vec3::new(0.0, 3.0, -5.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 15000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-7.0, 12.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        PointLight {
            intensity: 4200.0,
            range: 26.0,
            color: Color::srgb(1.0, 0.72, 0.43),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-2.0, 7.7, 1.0),
    ));
    commands.spawn((
        PointLight {
            intensity: 3600.0,
            range: 24.0,
            color: Color::srgb(0.56, 0.72, 1.0),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(8.0, 7.2, -7.0),
    ));
    commands.spawn((
        PointLight {
            intensity: 1800.0,
            range: 12.0,
            color: Color::srgb(1.0, 0.91, 0.79),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(1.2, 6.6, 0.2),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.83, 1.0),
        brightness: 240.0,
        ..default()
    });
}

fn make_cat(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    x: f32,
    y: f32,
    z: f32,
    fur: &Handle<StandardMaterial>,
    light: &Handle<StandardMaterial>,
    eye: &Handle<StandardMaterial>,
    green: &Handle<StandardMaterial>,
) {
    // Rounded body, alert head, triangular ears and bright eyes; all visible as 3D forms.
    ball(
        commands,
        meshes,
        fur.clone(),
        0.68,
        Vec3::new(x, y + 0.58, z),
        Vec3::new(0.9, 0.72, 1.3),
    );
    ball(
        commands,
        meshes,
        fur.clone(),
        0.55,
        Vec3::new(x, y + 1.30, z - 0.48),
        Vec3::new(1.0, 0.95, 0.85),
    );
    for dx in [-0.34, 0.34] {
        commands.spawn((
            Mesh3d(meshes.add(Cone::new(0.34, 0.68))),
            MeshMaterial3d(fur.clone()),
            Transform::from_xyz(x + dx, y + 1.83, z - 0.5),
        ));
        ball(
            commands,
            meshes,
            light.clone(),
            0.11,
            Vec3::new(x + dx * 0.48, y + 1.38, z - 0.99),
            Vec3::new(1.0, 1.0, 0.5),
        );
        ball(
            commands,
            meshes,
            green.clone(),
            0.065,
            Vec3::new(x + dx * 0.48, y + 1.38, z - 1.045),
            Vec3::new(1.0, 1.0, 0.5),
        );
        ball(
            commands,
            meshes,
            eye.clone(),
            0.027,
            Vec3::new(x + dx * 0.48, y + 1.38, z - 1.075),
            Vec3::new(1.0, 1.0, 0.5),
        );
    }
    // Four paws and a curved-up tail silhouette.
    for dx in [-0.43, 0.43] {
        for dz in [-0.38, 0.5] {
            ball(
                commands,
                meshes,
                fur.clone(),
                0.23,
                Vec3::new(x + dx, y + 0.17, z + dz),
                Vec3::new(0.8, 0.75, 1.0),
            );
        }
    }
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(0.13, 0.9))),
        MeshMaterial3d(fur.clone()),
        Transform::from_xyz(x + 0.68, y + 0.85, z + 0.48).with_rotation(Quat::from_rotation_x(1.0)),
    ));
}

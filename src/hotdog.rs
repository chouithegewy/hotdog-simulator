// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
use bevy::{
    asset::RenderAssetUsages, image::ImageLoaderSettings, mesh::Indices, prelude::*,
    render::render_resource::PrimitiveTopology,
};
use std::f32::consts::{PI, TAU};

pub struct HotdogAssets {
    sausage: Handle<Mesh>,
    crust: Handle<Mesh>,
    crumb: Handle<Mesh>,
    mustard: Handle<Mesh>,
    smile: Handle<Mesh>,
    eye: Handle<Mesh>,
    glint: Handle<Mesh>,
    sausage_material: Handle<StandardMaterial>,
    crust_material: Handle<StandardMaterial>,
    crumb_material: Handle<StandardMaterial>,
    mustard_material: Handle<StandardMaterial>,
    eye_material: Handle<StandardMaterial>,
    glint_material: Handle<StandardMaterial>,
    smile_material: Handle<StandardMaterial>,
    sleepy_lids: [Handle<Mesh>; 2],
    sleepy_mouth: Handle<Mesh>,
    fear_mouth: Handle<Mesh>,
    fear_rim: Handle<Mesh>,
    fear_brows: [Handle<Mesh>; 2],
    face_ball: Handle<Mesh>,
    tongue_material: Handle<StandardMaterial>,
    face_dark_material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub(crate) struct FrankLife {
    rest: Transform,
    terrified: bool,
    phase: f32,
}
impl FrankLife {
    /// Where the frank lies (the multiplayer lobby lays the tray's franks out afresh, one for each seat).
    pub(crate) fn rest_mut(&mut self) -> &mut Transform {
        &mut self.rest
    }
    /// Whether this is the intro's self-aware frank, eyes open, rather than one of the sleeping ones.
    pub(crate) fn terrified(&self) -> bool {
        self.terrified
    }
}
#[derive(Component)]
pub(crate) struct FearFeature {
    rest: Transform,
    mouth: bool,
}

fn face_point(x: f32, z: f32) -> Vec3 {
    let centre = 0.41 + 0.018 * ((x / 2.30 + 0.5) * PI).sin();
    Vec3::new(
        x,
        centre + (0.19_f32.powi(2) - z * z).max(0.0).sqrt() + 0.004,
        z,
    )
}

fn face_line(path: impl Fn(f32) -> Vec2, radius: f32) -> Mesh {
    surface(
        36,
        8,
        true,
        |u, v| {
            let p = path(u);
            let d = path((u + 0.002).min(1.0)) - path((u - 0.002).max(0.0));
            let across = Vec3::new(-d.y, 0.0, d.x).normalize_or_zero();
            face_point(p.x, p.y) + (Vec3::Y * (v * TAU).cos() + across * (v * TAU).sin()) * radius
        },
        |_, _| [1.0; 4],
    )
}

fn face_hole() -> Mesh {
    let mut positions = vec![face_point(-0.58, 0.0).to_array()];
    let mut uv = vec![[0.5, 0.5]];
    let mut indices = Vec::new();
    for i in 0..=64 {
        let a = i as f32 / 64.0 * TAU;
        positions.push(face_point(-0.58 + a.cos() * 0.075, a.sin() * 0.085).to_array());
        uv.push([0.5 + a.cos() * 0.5, 0.5 + a.sin() * 0.5]);
    }
    for i in 1..=64 {
        indices.extend_from_slice(&[0, i + 1, i]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh.generate_tangents().expect("valid face UVs");
    mesh
}

/// The food textures' format: the PNGs natively; in the browser their lossy WebP copies (tools/build-web.sh writes them),
/// a tenth of the download.
const FOOD_TEXTURES: &str = if cfg!(target_arch = "wasm32") { "webp" } else { "png" };

pub(crate) fn food_material(
    server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    name: &str,
    roughness: f32,
) -> Handle<StandardMaterial> {
    let linear_image = |suffix: &str| -> Handle<Image> {
        server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| settings.is_srgb = false)
            .load(format!("materials/food/{name}_{suffix}.{FOOD_TEXTURES}"))
    };
    materials.add(StandardMaterial {
        base_color_texture: Some(server.load(format!("materials/food/{name}_albedo.{FOOD_TEXTURES}"))),
        normal_map_texture: Some(linear_image("normal")),
        metallic_roughness_texture: Some(linear_image("roughness")),
        perceptual_roughness: roughness,
        metallic: 0.0,
        clearcoat: if name == "sausage" { 0.12 } else { 0.0 },
        ..default()
    })
}

pub(crate) fn surface(
    rings: u32,
    sides: u32,
    reverse: bool,
    sample: impl Fn(f32, f32) -> Vec3,
    tint: impl Fn(f32, f32) -> [f32; 4],
) -> Mesh {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    for i in 0..=rings {
        let u = i as f32 / rings as f32;
        for j in 0..=sides {
            let v = j as f32 / sides as f32;
            positions.push(sample(u, v).to_array());
            uvs.push([u, v]);
            colors.push(tint(u, v));
        }
    }
    let mut indices = Vec::new();
    for i in 0..rings {
        for j in 0..sides {
            let a = i * (sides + 1) + j;
            let b = a + sides + 1;
            let triangles = if reverse {
                [a, a + 1, b, a + 1, b + 1, b]
            } else {
                [a, b, a + 1, a + 1, b, b + 1]
            };
            indices.extend_from_slice(&triangles);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices));
    // Angle-weighted normals discard tiny metre-scale triangles at f32::EPSILON.
    // Area weighting preserves valid normals on the detailed 17 cm player meshes.
    mesh.compute_area_weighted_normals();
    mesh.generate_tangents()
        .expect("Food surfaces have valid positions and UVs");
    mesh
}

fn bun_cap(x: f32) -> f32 {
    if x.abs() < 0.78 {
        1.0
    } else {
        (1.0 - ((x.abs() - 0.78) / 0.27).powi(2)).max(0.0001).sqrt()
    }
}

impl HotdogAssets {
    pub fn new(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        server: &AssetServer,
    ) -> Self {
        let sausage = meshes.add(surface(
            48,
            24,
            true,
            |u, v| {
                let x = (u - 0.5) * 2.30;
                let end = ((x.abs() - 0.88) / 0.27).clamp(0.0, 1.0);
                let cap = (1.0 - end * end).max(0.0003).sqrt();
                let angle = v * TAU;
                let gathered = 1.0 + 0.045 * end.powi(3) * (angle * 9.0 + u * 20.0).sin();
                let radius = 0.19 * cap * gathered * (1.0 + 0.008 * (u * 48.0 + angle * 3.0).sin());
                Vec3::new(
                    x,
                    0.41 + 0.018 * (u * PI).sin() + radius * angle.cos(),
                    0.008 * (u * TAU).sin() + radius * angle.sin(),
                )
            },
            |u, _| {
                let end = ((u - 0.5).abs() * 2.0).powi(10);
                [1.0 - 0.08 * end, 1.0 - 0.10 * end, 1.0 - 0.10 * end, 1.0]
            },
        ));
        let crust = meshes.add(surface(
            64,
            40,
            false,
            |u, v| {
                let x = (u - 0.5) * 2.10;
                let cap = bun_cap(x);
                let angle = -0.40 - v * (TAU - 0.80);
                let irregular = 1.0 + 0.012 * (u * 31.0 + angle * 3.0).sin();
                Vec3::new(
                    x,
                    0.235 + cap * 0.22 * angle.cos() * irregular,
                    cap * 0.40 * angle.sin() * irregular,
                )
            },
            |u, v| {
                let toast =
                    (v * TAU).cos().max(0.0) * 0.08 + ((u - 0.5).abs() * 2.0).powi(8) * 0.10;
                [1.0 - toast * 0.3, 1.0 - toast * 0.8, 1.0 - toast, 1.0]
            },
        ));
        let crumb = meshes.add(surface(
            64,
            28,
            false,
            |u, v| {
                let x = (u - 0.5) * 2.10;
                let cap = bun_cap(x);
                let s = 1.0 - v * 2.0;
                Vec3::new(x, 0.235 + cap * (-0.035 + 0.238 * s * s), cap * 0.156 * s)
            },
            |_, _| [1.0; 4],
        ));
        let mustard = meshes.add(surface(
            64,
            8,
            true,
            |u, v| {
                let x = (u - 0.5) * 1.65;
                let z = 0.055 * (u * 7.0 * PI).sin();
                let radius = 0.014 * (u * PI).sin().max(0.08).sqrt();
                let casing_center_y = 0.41 + 0.018 * ((x / 2.30 + 0.5) * PI).sin();
                let center = Vec3::new(
                    x,
                    casing_center_y + (0.19_f32.powi(2) - z * z).sqrt() + radius * 0.5,
                    z,
                );
                center + Vec3::new(0.0, (v * TAU).cos(), (v * TAU).sin()) * radius
            },
            |_, _| [1.0; 4],
        ));
        let smile = meshes.add(surface(
            24,
            6,
            true,
            |u, v| {
                let s = u * 2.0 - 1.0;
                let y = 0.47 + 0.025 * s * s;
                let z = (0.19_f32.powi(2) - (y - 0.428).powi(2)).sqrt() + 0.008;
                Vec3::new(
                    s * 0.13,
                    y + (v * TAU).cos() * 0.005,
                    z + (v * TAU).sin() * 0.005,
                )
            },
            |_, _| [1.0; 4],
        ));
        Self {
            sausage,
            crust,
            crumb,
            mustard,
            smile,
            eye: meshes.add(Sphere::new(0.023).mesh().uv(16, 12)),
            glint: meshes.add(Sphere::new(0.006).mesh().uv(10, 8)),
            sausage_material: food_material(server, materials, "sausage", 0.60),
            crust_material: food_material(server, materials, "crust", 0.96),
            crumb_material: food_material(server, materials, "crumb", 0.98),
            mustard_material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.86, 0.55, 0.055),
                perceptual_roughness: 0.38,
                ..default()
            }),
            eye_material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.025, 0.012, 0.008),
                perceptual_roughness: 0.18,
                ..default()
            }),
            glint_material: materials.add(Color::srgb(0.98, 0.94, 0.80)),
            smile_material: materials.add(Color::srgb(0.20, 0.055, 0.025)),
            sleepy_lids: [-0.075, 0.075].map(|z| {
                meshes.add(face_line(
                    |u| {
                        let s = u * 2.0 - 1.0;
                        Vec2::new(-0.84 + 0.022 * (1.0 - s * s), z + s * 0.036)
                    },
                    0.007,
                ))
            }),
            sleepy_mouth: meshes.add(face_line(
                |u| {
                    let s = u * 2.0 - 1.0;
                    Vec2::new(-0.58 + 0.045 * (1.0 - s * s), s * 0.095)
                },
                0.008,
            )),
            fear_mouth: meshes.add(face_hole()),
            fear_rim: meshes.add(face_line(
                |u| Vec2::new(-0.58 + (u * TAU).cos() * 0.079, (u * TAU).sin() * 0.089),
                0.007,
            )),
            fear_brows: [-0.075, 0.075].map(|z| {
                meshes.add(face_line(
                    |u| Vec2::new(-0.922 - 0.014 * (u * PI).sin(), z + (u - 0.5) * 0.08),
                    0.006,
                ))
            }),
            face_ball: meshes.add(Sphere::new(1.0).mesh().uv(20, 12)),
            tongue_material: materials.add(Color::srgb(0.53, 0.10, 0.12)),
            face_dark_material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.018, 0.008, 0.006),
                unlit: true,
                perceptual_roughness: 0.95,
                ..default()
            }),
        }
    }

    /// An uncooked frank, without the bun and toppings used in the arena.
    pub fn spawn_frank(
        &self,
        commands: &mut Commands,
        transform: Transform,
        terrified: bool,
    ) -> Entity {
        let root = commands
            .spawn((
                Name::new(if terrified {
                    "Self-aware terrified hotdog"
                } else {
                    "Sleeping smiling hotdog"
                }),
                Mesh3d(self.sausage.clone()),
                MeshMaterial3d(self.sausage_material.clone()),
                transform,
                FrankLife {
                    rest: transform,
                    terrified,
                    phase: transform.translation.x * 91.0,
                },
            ))
            .id();
        let mut mark = |mesh: Handle<Mesh>, material: Handle<StandardMaterial>, at: Transform| {
            let mouth = mesh == self.fear_mouth || mesh == self.fear_rim;
            let entity = commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    at,
                    bevy::light::NotShadowCaster,
                ))
                .id();
            commands.entity(root).add_child(entity);
            if terrified {
                commands
                    .entity(entity)
                    .insert(FearFeature { rest: at, mouth });
            }
        };
        if terrified {
            for z in [-0.075, 0.075] {
                let p = face_point(-0.84, z);
                mark(
                    self.face_ball.clone(),
                    self.glint_material.clone(),
                    Transform::from_translation(p + Vec3::Y * 0.008)
                        .with_scale(Vec3::new(0.040, 0.016, 0.035)),
                );
                mark(
                    self.face_ball.clone(),
                    self.face_dark_material.clone(),
                    Transform::from_translation(p + Vec3::new(-0.006, 0.023, 0.0))
                        .with_scale(Vec3::new(0.018, 0.007, 0.018)),
                );
                mark(
                    self.face_ball.clone(),
                    self.glint_material.clone(),
                    Transform::from_translation(p + Vec3::new(-0.010, 0.029, -0.005))
                        .with_scale(Vec3::splat(0.005)),
                );
            }
            for brow in &self.fear_brows {
                mark(
                    brow.clone(),
                    self.face_dark_material.clone(),
                    Transform::default(),
                );
            }
            mark(
                self.fear_mouth.clone(),
                self.face_dark_material.clone(),
                Transform::default(),
            );
            mark(
                self.fear_rim.clone(),
                self.smile_material.clone(),
                Transform::default(),
            );
            mark(
                self.face_ball.clone(),
                self.tongue_material.clone(),
                Transform::from_translation(face_point(-0.535, 0.0) + Vec3::Y * 0.002)
                    .with_scale(Vec3::new(0.018, 0.005, 0.040)),
            );
        } else {
            for lid in &self.sleepy_lids {
                mark(
                    lid.clone(),
                    self.face_dark_material.clone(),
                    Transform::default(),
                );
            }
            mark(
                self.sleepy_mouth.clone(),
                self.face_dark_material.clone(),
                Transform::default(),
            );
        }
        root
    }

    pub fn spawn(&self, commands: &mut Commands, at: Vec3, yaw: f32) {
        let root = commands
            .spawn((
                Name::new("Detailed happy hotdog"),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)),
            ))
            .id();
        for (name, mesh, material) in [
            (
                "Natural sausage casing",
                &self.sausage,
                &self.sausage_material,
            ),
            ("Toasted bun crust", &self.crust, &self.crust_material),
            ("Split bread crumb", &self.crumb, &self.crumb_material),
            ("Mustard ribbon", &self.mustard, &self.mustard_material),
            ("Small happy smile", &self.smile, &self.smile_material),
        ] {
            let child = commands
                .spawn((
                    Name::new(name),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                ))
                .id();
            commands.entity(root).add_child(child);
        }
        for x in [-0.12, 0.12] {
            let eye = commands
                .spawn((
                    Mesh3d(self.eye.clone()),
                    MeshMaterial3d(self.eye_material.clone()),
                    Transform::from_xyz(x, 0.548, 0.155),
                ))
                .id();
            let glint = commands
                .spawn((
                    Mesh3d(self.glint.clone()),
                    MeshMaterial3d(self.glint_material.clone()),
                    Transform::from_xyz(x - 0.006, 0.555, 0.176),
                ))
                .id();
            commands.entity(root).add_children(&[eye, glint]);
        }
    }
}

pub(crate) fn animate_franks(
    time: Res<Time>,
    game: Option<Res<crate::arena::Game>>,
    mut franks: Query<(&FrankLife, &mut Transform), Without<FearFeature>>,
    mut features: Query<(&FearFeature, &mut Transform), Without<FrankLife>>,
) {
    for (life, mut transform) in &mut franks {
        *transform = life.rest;
        if !life.terrified {
            transform.scale.y *= 1.0 + 0.008 * (time.elapsed_secs() * 1.25 + life.phase).sin();
        }
    }
    let fear = game.map(|g| g.fear()).unwrap_or(0.2);
    for (feature, mut transform) in &mut features {
        *transform = feature.rest;
        if feature.mouth {
            transform.scale.x = 0.55 + fear * 0.65;
            transform.translation.x = -0.58 * (1. - transform.scale.x);
        } else {
            transform.translation.x += (time.elapsed_secs() * 19.).sin() * 0.001 * fear;
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Virtual geometry: Bevy 0.19's meshlet renderer, drawing a few things far denser than the normal triangle budget.
//!
//! * **What is dense** (`shapes`): the counter top's stone, a tiled backsplash on the back wall (added; the kitchen has
//!   plain plaster there), the steamers' woven lids in the kung fu dojo, cereal boxes and plates. Each is a twin entity
//!   with a `MeshletMesh3d`, moved to where the game's own entity is every frame; the game's entity keeps everything else
//!   (its physics, its place, its name) and only loses its `Mesh3d`, kept in [`Swapped`]. Rigid motion is fine (a box
//!   knocked off the counter tumbles as its twin); a meshlet mesh cannot be skinned, morphed or rebuilt each frame.
//! * **Converted once, cached**: `MeshletMesh::from_mesh` takes seconds a million triangles, so each shape is made and
//!   converted on the async compute pool while the game's own mesh is still drawn, then saved under the user's cache
//!   (`$XDG_CACHE_HOME/hotdog-simulator/ultra`, else `~/.cache/...`) in Bevy's meshlet format; later runs load the file
//!   (through Bevy's in-memory `embedded://` source, so nothing outside the asset folder is opened by the asset server).
//! * **What it costs the picture**: in Bevy 0.19.1 a meshlet camera may have no prepass (the meshlet prepass binds the
//!   material at the wrong index) and no multisampling, so with meshlets the game's camera gives up temporal
//!   anti-aliasing (FXAA instead, `viewmodel::stack`), ambient occlusion and contact shadows ([`cameras`]).
//! * **Materials**: meshlets take only opaque `StandardMaterial`s here, and a texture transform would be applied to the
//!   UVs but not to their derivatives in the meshlet shader, so a twin's texture co-ordinates are baked and its material
//!   has none (the counter's stone).
use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    asset::{io::embedded::EmbeddedAssetRegistry, saver::{AssetSaver, SavedAsset}, AssetPath, LoadState},
    camera::primitives::Aabb,
    core_pipeline::prepass::{DeferredPrepass, DepthPrepass, MotionVectorPrepass, NormalPrepass},
    light::{NotShadowCaster, ShadowFilteringMethod},
    math::Affine2,
    pbr::{
        experimental::meshlet::{MeshletMesh, MeshletMesh3d, MeshletMeshSaver, MeshletPlugin, MESHLET_DEFAULT_VERTEX_POSITION_QUANTIZATION_FACTOR, MESHLET_MESH_ASSET_VERSION},
        ContactShadows, ScreenSpaceAmbientOcclusion,
    },
    platform::collections::HashMap,
    prelude::*,
    render::camera::{MipBias, TemporalJitter},
    tasks::{futures::check_ready, AsyncComputeTaskPool, Task},
    transform::TransformSystems,
};
use std::{f32::consts::FRAC_PI_2, path::PathBuf};

use super::shapes::{self, Shape};
use crate::fridge::Room;
use crate::level::MapPart;

/// Bumped whenever a shape changes, so that the cache is made again.
const SHAPES_VERSION: u32 = 3;
/// The counter's stone and the steamers' weave: grid steps (metres).
const COUNTER_STEP: f32 = 0.002;
/// At most this many grid cells (two triangles each) on a counter top.
const COUNTER_CELLS: f32 = 1.5e6;
const WEAVE_STEP: f32 = 0.0006;
/// `visuals::surfaces` lays the counter's stone this many metres to a repeat.
const STONE_TILE: f32 = 0.42;
/// The backsplash: tiles this size, with grout this wide between them, standing this proud.
const TILE: f32 = 0.1;
const GROUT: f32 = 0.003;
const PROUD: f32 = 0.003;
/// Where the backsplash runs: from the counter top to the underside of the wall cupboards (`fridge::room_boxes`).
const SPLASH_LOW: f32 = 0.9025;
const SPLASH_HIGH: f32 = 1.6;

pub fn build(app: &mut App, _gi: bool) {
    // One buffer of cluster slots serves every view in a frame (the camera and five shadow views), each slot 4 bytes of
    // VRAM. Too few shows as missing or garbled meshes: Pillow Fort's 56 million source triangles need about 2.6 million
    // slots over its six views. 2^23 is 32 MB. (Its size makes no difference to the time a view takes.)
    app.add_plugins(MeshletPlugin { cluster_buffer_slots: 1 << 23 })
        .init_resource::<Twins>()
        .add_observer(no_multisampling)
        // Last: after everything else in the frame has had its say about the cameras, just before they are drawn.
        .add_systems(Last, cameras)
        .add_systems(Last, loose_meshes)
        .add_systems(PostUpdate, (finish, find, follow, backsplash).chain().after(TransformSystems::Propagate));
}

/// A dense shape: being made, being loaded from the cache, or ready (with its triangles).
enum Made {
    Making(Task<Result<Built, String>>),
    Loading(Handle<MeshletMesh>, usize),
    Ready(Handle<MeshletMesh>, usize),
    Failed,
}

/// What a making task hands back: the converted mesh, or the cached file's bytes.
enum Built {
    Converted(MeshletMesh, usize),
    Cached(Vec<u8>, usize),
}

/// What is drawn dense, and the bookkeeping.
#[derive(Resource)]
pub struct Twins {
    shapes: HashMap<String, Made>,
    /// Each game entity drawn dense, and its twin.
    twins: HashMap<Entity, Entity>,
    /// The backsplash's entities and the room they were laid for.
    splash: Option<(Room, Vec<Entity>)>,
    /// The loose kinds' meshes (cereal box; plate and its foot) once `loose` has made them.
    loose: Option<[AssetId<Mesh>; 3]>,
    /// The counter's stone without its texture transform, per source material.
    stone: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    plate: Option<Handle<StandardMaterial>>,
    tiles: Option<(Handle<StandardMaterial>, Handle<StandardMaterial>, Handle<Mesh>)>,
    cache: PathBuf,
    /// Instances drawn and their source triangles, for the report.
    pub instances: usize,
    pub triangles: usize,
}

impl Default for Twins {
    fn default() -> Self {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        Twins {
            shapes: HashMap::default(),
            twins: HashMap::default(),
            splash: None,
            loose: None,
            stone: HashMap::default(),
            plate: None,
            tiles: None,
            cache: base.join("hotdog-simulator").join("ultra"),
            instances: 0,
            triangles: 0,
        }
    }
}

impl Twins {
    pub fn summary(&self) -> String {
        let making = self.shapes.values().filter(|m| matches!(m, Made::Making(_) | Made::Loading(..))).count();
        let ready = self.shapes.values().filter(|m| matches!(m, Made::Ready(..))).count();
        format!(
            "virtual geometry: {} meshlet instances, {:.1} M source triangles ({} dense shapes ready{})",
            self.instances,
            self.triangles as f64 / 1e6,
            ready,
            if making > 0 { format!(", {making} being made") } else { String::new() }
        )
    }

    /// The shape called `name`, made by `make` the first time (from the cache if it is there): ready, or not yet.
    fn shape(&mut self, name: String, make: impl FnOnce() -> Shape + Send + 'static) -> Option<(Handle<MeshletMesh>, usize)> {
        if let Some(made) = self.shapes.get(&name) {
            return match made {
                Made::Ready(handle, triangles) => Some((handle.clone(), *triangles)),
                _ => None,
            };
        }
        let file = self.cache.join(format!("{name}-s{SHAPES_VERSION}-m{MESHLET_MESH_ASSET_VERSION}.meshlet_mesh"));
        let label = name.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let count = file.with_extension("triangles");
            if let (Ok(bytes), Some(triangles)) = (std::fs::read(&file), std::fs::read_to_string(&count).ok().and_then(|t| t.trim().parse().ok())) {
                return Ok(Built::Cached(bytes, triangles));
            }
            let started = std::time::Instant::now();
            let shape = make();
            let triangles = shape.triangles();
            let meshlet = MeshletMesh::from_mesh(&shape.mesh(), MESHLET_DEFAULT_VERTEX_POSITION_QUANTIZATION_FACTOR).map_err(|e| format!("{label}: {e}"))?;
            let mut bytes: Vec<u8> = Vec::new();
            bevy::tasks::block_on(MeshletMeshSaver.save(&mut bytes, SavedAsset::from_asset(&meshlet), &(), AssetPath::from("ultra.meshlet_mesh"))).map_err(|e| format!("{label}: {e}"))?;
            let saved = std::fs::create_dir_all(file.parent().unwrap_or(&file)).and_then(|_| std::fs::write(&file, &bytes)).and_then(|_| std::fs::write(&count, triangles.to_string()));
            info!(
                "Ultra: made {label}: {:.2} M triangles in {:.1} s, {:.1} MB{}",
                triangles as f64 / 1e6,
                started.elapsed().as_secs_f32(),
                bytes.len() as f64 / 1e6,
                if saved.is_ok() { "" } else { " (not cached: the cache folder cannot be written)" }
            );
            Ok(Built::Converted(meshlet, triangles))
        });
        self.shapes.insert(name, Made::Making(task));
        None
    }
}

/// Takes up what the making tasks and the cache loads have finished.
fn finish(mut twins: ResMut<Twins>, mut meshlets: ResMut<Assets<MeshletMesh>>, embedded: Option<Res<EmbeddedAssetRegistry>>, server: Res<AssetServer>) {
    let cache = twins.cache.clone();
    for (name, made) in twins.shapes.iter_mut() {
        let next = match made {
            Made::Making(task) => match check_ready(task) {
                None => continue,
                Some(Ok(Built::Converted(meshlet, triangles))) => Made::Ready(meshlets.add(meshlet), triangles),
                Some(Ok(Built::Cached(bytes, triangles))) => match &embedded {
                    Some(embedded) => {
                        let path = format!("hotdog_ultra/{name}.meshlet_mesh");
                        embedded.insert_asset(cache.join(&path), std::path::Path::new(&path), bytes);
                        Made::Loading(server.load(format!("embedded://{path}")), triangles)
                    }
                    None => Made::Failed,
                },
                Some(Err(why)) => {
                    warn!("Ultra: cannot make {name}: {why}");
                    Made::Failed
                }
            },
            Made::Loading(handle, triangles) => {
                if meshlets.get(&*handle).is_some() {
                    Made::Ready(handle.clone(), *triangles)
                } else if matches!(server.load_state(&*handle), LoadState::Failed(_)) {
                    warn!("Ultra: the cached {name} would not load; delete {} to make it again", cache.display());
                    Made::Failed
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        *made = next;
    }
}

/// The cereal box's and the plate's meshes, once `loose` has made its kit (it needs the world).
fn loose_meshes(world: &mut World) {
    if world.resource::<Twins>().loose.is_some() {
        return;
    }
    let kind = |name: &str| kitchen_sim::map::LOOSE.iter().position(|k| *k == name);
    let (Some(cereal), Some(plate)) = (kind("cereal"), kind("plate")) else { return };
    let (cereal, plate) = (crate::loose::meshes(world, cereal), crate::loose::meshes(world, plate));
    if let (Some((c, _)), Some((p, _)), Some((f, _))) = (cereal.first(), plate.first(), plate.get(1)) {
        world.resource_mut::<Twins>().loose = Some([c.id(), p.id(), f.id()]);
    }
}

/// On a game entity drawn dense: its own mesh (put back if the twin goes), and its twin.
#[derive(Component)]
pub struct Swapped {
    mesh: Handle<Mesh>,
    twin: Option<Entity>,
    /// The twin is built in metres: it takes its source's place and turn, not its scale.
    unscaled: bool,
}

/// A twin: the dense meshlet stand-in for a game entity, or a piece of the backsplash.
#[derive(Component)]
struct Twin;

/// The meshlet renderer stops the program if any camera has multisampling when it is drawn, so it is turned off the moment
/// it is set (the view model's camera is made with it, part way through a frame).
fn no_multisampling(set: On<Insert, Msaa>, mut msaa: Query<(&mut Msaa, Option<&Name>)>) {
    if let Ok((mut m, name)) = msaa.get_mut(set.entity) {
        if *m != Msaa::Off {
            debug!("Ultra: multisampling off for {:?}", name.map(|n| n.as_str()));
            *m = Msaa::Off;
        }
    }
}

/// Every camera drawing meshlets goes without a prepass and without multisampling (see the module's documentation).
#[allow(clippy::type_complexity)]
fn cameras(
    mut commands: Commands,
    cameras: Query<
        (Entity, &Msaa, Has<TemporalAntiAliasing>, Has<DepthPrepass>, Has<NormalPrepass>, Has<MotionVectorPrepass>, Has<DeferredPrepass>, Has<ScreenSpaceAmbientOcclusion>, Has<ContactShadows>, Option<&ShadowFilteringMethod>),
        With<Camera3d>,
    >,
) {
    for (entity, msaa, taa, depth, normal, motion, deferred, occlusion, contact, filtering) in &cameras {
        let mut camera = commands.entity(entity);
        if *msaa != Msaa::Off {
            camera.insert(Msaa::Off);
        }
        if taa || depth || normal || motion || deferred || occlusion || contact {
            camera.remove::<(TemporalAntiAliasing, TemporalJitter, MipBias, ScreenSpaceAmbientOcclusion, ContactShadows)>();
            camera.remove::<(DepthPrepass, NormalPrepass, MotionVectorPrepass, DeferredPrepass)>();
        }
        // Temporal filtering of the shadows wants temporal anti-aliasing to average it.
        if filtering == Some(&ShadowFilteringMethod::Temporal) {
            camera.insert(ShadowFilteringMethod::Gaussian);
        }
    }
}

/// Millimetres, for a shape's name.
fn mm(v: f32) -> i32 {
    (v * 1000.).round() as i32
}

/// Finds the game entities to draw dense, and swaps each for its twin once its shape is ready.
#[allow(clippy::type_complexity)]
fn find(
    mut commands: Commands,
    mut twins: ResMut<Twins>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    candidates: Query<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>, &GlobalTransform, Option<&Name>, Option<&ChildOf>, Option<&Aabb>, Has<MapPart>, Has<NotShadowCaster>), Without<Swapped>>,
    names: Query<&Name>,
) {
    let loose = twins.loose;
    for (entity, mesh, material, at, name, parent, aabb, map_part, shadowless) in &candidates {
        let id = mesh.0.id();
        let Some(source) = materials.get(&material.0).cloned() else { continue };
        if source.alpha_mode != AlphaMode::Opaque {
            continue;
        }
        // (shape name, how to make it, the twin's material, whether it is built in metres)
        let wanted: Option<(String, Box<dyn FnOnce() -> Shape + Send>, Handle<StandardMaterial>, bool)> = if map_part && name.is_some_and(|n| n.as_str() == "Counter") {
            // The counter top: built in metres for its size, its stone laid from the world's origin as the game lays it.
            // Not before `visuals` has dressed it in its stone (which it does only to what has a `Mesh3d`).
            if source.base_color_texture.is_none() {
                continue;
            }
            let (scale, _, centre) = at.to_scale_rotation_translation();
            let (w, h, d, cx, cz) = (scale.x, scale.y, scale.z, centre.x, centre.z);
            let stone = match twins.stone.get(&material.0.id()) {
                Some(stone) => stone.clone(),
                None => {
                    // No normal map: the relief is in the geometry now, and on triangles smaller than a pixel the meshlet
                    // shader's tangents (worked out from the UVs' screen derivatives) turn it into dark blotches.
                    let stone = materials.add(StandardMaterial { uv_transform: Affine2::IDENTITY, normal_map_texture: None, ..source.clone() });
                    twins.stone.insert(material.0.id(), stone.clone());
                    stone
                }
            };
            // A long counter gets a coarser grid, so that it stays near three million triangles (and a minute to convert).
            let step = COUNTER_STEP.max((w * d / COUNTER_CELLS).sqrt());
            let name = format!("counter-{}x{}x{}-at{},{}-step{}um", mm(w), mm(d), mm(h), mm(cx), mm(cz), (step * 1e6).round());
            Some((name, Box::new(move || shapes::counter(w, d, h, step, STONE_TILE, Vec2::new(cx, cz))), stone, true))
        } else if let (Some(parent), Some(aabb), true) = (parent, aabb, source.base_color_texture.is_some()) {
            // A steamer's woven top: the flat, textured part of a steamer.
            let steamer = names.get(parent.parent()).is_ok_and(|n| n.as_str().starts_with("Ring") || n.as_str().starts_with("Bamboo steamer"));
            let half = Vec3::from(aabb.half_extents);
            if steamer && half.y < 1e-4 && half.x > 0.02 {
                let (rx, rz) = (half.x, half.z);
                Some((format!("weave-{}x{}", mm(rx), mm(rz)), Box::new(move || shapes::weave(rx, rz, WEAVE_STEP, crate::dojo::WEAVE)), material.0.clone(), false))
            } else {
                None
            }
        } else {
            None
        };
        let wanted = wanted.or_else(|| {
            let [cereal, plate, foot] = loose?;
            if id == cereal {
                Some(("cereal".into(), Box::new(|| shapes::cereal([0.115, 0.165, 0.04], 0.003, 0.00075)) as Box<dyn FnOnce() -> Shape + Send>, material.0.clone(), false))
            } else if id == plate || id == foot {
                let paint = twins.plate.get_or_insert_with(|| {
                    let image = images.add(shapes::plate_paint());
                    materials.add(StandardMaterial { base_color_texture: Some(image), perceptual_roughness: 0.16, reflectance: 0.7, ..default() })
                });
                Some(("plate".into(), Box::new(|| shapes::plate(1024)) as Box<dyn FnOnce() -> Shape + Send>, paint.clone(), false))
            } else {
                None
            }
        });
        let Some((shape, make, look, unscaled)) = wanted else { continue };
        let foot = loose.is_some_and(|[_, _, foot]| id == foot);
        let Some((dense, _)) = twins.shape(shape, make) else { continue };
        // The plate's foot is part of the dense plate: it only goes.
        let place = if unscaled {
            let (_, turn, middle) = at.to_scale_rotation_translation();
            Transform::from_translation(middle).with_rotation(turn)
        } else {
            at.compute_transform()
        };
        let twin = (!foot).then(|| {
            let mut t = commands.spawn((Twin, MeshletMesh3d(dense), MeshMaterial3d(look), place, GlobalTransform::from(place), Visibility::Inherited));
            if shadowless {
                t.insert(NotShadowCaster);
            }
            t.id()
        });
        if let Some(twin) = twin {
            twins.twins.insert(entity, twin);
        }
        commands.entity(entity).remove::<Mesh3d>().insert(Swapped { mesh: mesh.0.clone(), twin, unscaled });
    }
}

/// Moves each twin to its source, shows and hides it with it, and lets it go with it. A source that is given a mesh
/// again (the game rebuilt it) is drawn as the game draws it.
#[allow(clippy::type_complexity)]
fn follow(
    mut commands: Commands,
    mut twins: ResMut<Twins>,
    sources: Query<(Entity, &Swapped, &GlobalTransform, &InheritedVisibility, Option<&Mesh3d>)>,
    mut places: Query<(&mut GlobalTransform, &mut Visibility, &MeshletMesh3d), (With<Twin>, Without<Swapped>)>,
) {
    let mut alive = 0;
    for (entity, swapped, at, shown, again) in &sources {
        if let Some(mesh) = again {
            if mesh.0 == swapped.mesh {
                commands.entity(entity).remove::<Mesh3d>();
            } else {
                if let Some(twin) = swapped.twin {
                    commands.entity(twin).despawn();
                }
                twins.twins.remove(&entity);
                commands.entity(entity).remove::<Swapped>();
                continue;
            }
        }
        let Some(twin) = swapped.twin else { continue };
        let Ok((mut place, mut visibility, _)) = places.get_mut(twin) else { continue };
        let wanted = if swapped.unscaled {
            let (_, turn, middle) = at.to_scale_rotation_translation();
            GlobalTransform::from(Transform::from_translation(middle).with_rotation(turn))
        } else {
            *at
        };
        if *place != wanted {
            *place = wanted;
        }
        let v = if shown.get() { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != v {
            *visibility = v;
        }
        alive += 1;
    }
    // Twins whose source is gone.
    let gone: Vec<(Entity, Entity)> = twins.twins.iter().filter(|(source, _)| sources.get(**source).is_err()).map(|(a, b)| (*a, *b)).collect();
    for (source, twin) in gone {
        twins.twins.remove(&source);
        if let Ok(mut e) = commands.get_entity(twin) {
            e.despawn();
        }
    }
    let _ = alive;
    // The report: every dense instance in the world (the backsplash's tiles among them) and its source triangles.
    let count = |h: &MeshletMesh3d| {
        twins.shapes.values().find_map(|m| match m {
            Made::Ready(handle, triangles) if handle.id() == h.0.id() => Some(*triangles),
            _ => None,
        })
    };
    let (mut instances, mut triangles) = (0, 0);
    for (_, visibility, h) in &places {
        if *visibility != Visibility::Hidden {
            instances += 1;
            triangles += count(h).unwrap_or(0);
        }
    }
    twins.instances = instances;
    twins.triangles = triangles;
}

/// A tiled backsplash on the back wall between the counter and the wall cupboards, laid again for a new room: glazed
/// tiles (one dense shape, instanced) proud of a grout bed.
fn backsplash(mut commands: Commands, mut twins: ResMut<Twins>, room: Option<Res<Room>>, mut materials: ResMut<Assets<StandardMaterial>>, mut meshes: ResMut<Assets<Mesh>>) {
    let Some(room) = room else { return };
    if twins.splash.as_ref().is_some_and(|(laid, _)| *laid == *room) {
        return;
    }
    let Some((tile, _)) = twins.shape(format!("tile-{}mm", mm(TILE)), || shapes::tile(TILE, PROUD, 0.0005)) else { return };
    if let Some((_, old)) = twins.splash.take() {
        for e in old {
            if let Ok(mut e) = commands.get_entity(e) {
                e.despawn();
            }
        }
    }
    let (glaze, grout, bed) = twins
        .tiles
        .get_or_insert_with(|| {
            let glaze = materials.add(StandardMaterial { base_color: Color::srgb(0.86, 0.85, 0.79), perceptual_roughness: 0.18, reflectance: 0.55, ..default() });
            let grout = materials.add(StandardMaterial { base_color: Color::srgb(0.42, 0.40, 0.36), perceptual_roughness: 0.95, ..default() });
            (glaze, grout, meshes.add(Rectangle::new(1., 1.)))
        })
        .clone();
    // The wall's face (the back wall is 8 cm thick round `room.back`), and the run of cupboards above it.
    let wall = room.back - 0.04;
    let pitch = TILE + GROUT;
    let (x0, x1) = (room.run[0], room.run[1]);
    let (across, up) = (((x1 - x0) / pitch).floor().max(1.) as i32, ((SPLASH_HIGH - SPLASH_LOW) / pitch).floor().max(1.) as i32);
    let left = (x0 + x1) / 2. - across as f32 * pitch / 2. + pitch / 2.;
    let low = SPLASH_LOW + ((SPLASH_HIGH - SPLASH_LOW) - up as f32 * pitch) / 2. + pitch / 2.;
    let mut laid = vec![];
    // The grout bed just in front of the wall, under the tiles' feet.
    laid.push(
        commands
            .spawn((
                Twin,
                Mesh3d(bed),
                MeshMaterial3d(grout),
                Transform::from_xyz((x0 + x1) / 2., (SPLASH_LOW + SPLASH_HIGH) / 2., wall - 0.0012)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                    .with_scale(Vec3::new(across as f32 * pitch + GROUT, SPLASH_HIGH - SPLASH_LOW, 1.)),
            ))
            .id(),
    );
    // Each tile faces the room (-z): its +y turned to -z.
    let facing = Quat::from_rotation_x(-FRAC_PI_2);
    for i in 0..across {
        for j in 0..up {
            let at = Vec3::new(left + i as f32 * pitch, low + j as f32 * pitch, wall - 0.001);
            laid.push(commands.spawn((Twin, MeshletMesh3d(tile.clone()), MeshMaterial3d(glaze.clone()), Transform::from_translation(at).with_rotation(facing), Visibility::Inherited)).id());
        }
    }
    info!("Ultra: a backsplash of {} tiles on the back wall", across * up);
    twins.splash = Some((*room, laid));
}

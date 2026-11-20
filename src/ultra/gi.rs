// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Raytraced lighting: Bevy 0.19's `bevy_solari` in the game's own scene.
//!
//! Solari lights what the camera sees (its G-buffer) by tracing rays against a second, traced copy of the scene: every
//! entity with a `RaytracingMesh3d` and a `StandardMaterial`. It knows only two kinds of light, the sun (a
//! `DirectionalLight`) and emissive surfaces, and ignores point and spot lights, ambient light and environment maps. What
//! it needs, and what is done here for it:
//!
//! * **The camera** gets `SolariLighting` (with HDR, a deferred G-buffer, depth and motion-vector prepasses, which it
//!   brings) and a main texture it can write as storage; the view-model camera, which shares that texture, gets the same
//!   usage and a deferred prepass of its own (it is still lit the ordinary way). Ambient occlusion and contact shadows go:
//!   the traced light replaces them. Temporal anti-aliasing stays: with no denoiser in Bevy 0.19 for anything but NVIDIA's
//!   DLSS Ray Reconstruction, it is what averages Solari's noise away over frames.
//! * **Materials**: Solari would make every opaque material deferred; that leaves custom materials that disable their
//!   prepass (the scope's lens) drawn by no pass at all. So the default stays forward and only the opaque
//!   `StandardMaterial`s are made deferred ([`deferred_materials`]); custom materials draw forward over the traced picture,
//!   lit as before.
//! * **The traced scene** ([`stand_ins`]): each shown, opaque, shadow-casting `StandardMaterial` mesh of the world gets a
//!   stand-in: a separate entity with a `RaytracingMesh3d` and the same material, moved to where its source is every frame.
//!   Its mesh must be exactly {position, normal, uv, tangent} with 32-bit indices, so a mesh that is not gets a copy that is
//!   (kept in the render world only, made again when its source changes). What casts no shadow in the rasterised scene is
//!   left out, so the traced sun falls where the shadow-mapped one did (the ceiling, which `visuals` keeps from shading the
//!   room, among it), except what glows. Skinned and morphing meshes are left out: their traced copy would be their bind
//!   pose. So are the view model's layer, and anything hidden.
//! * **Lamps** ([`lamps`]): each shown point or spot light becomes a small glowing sphere in the traced scene only, as
//!   bright as the light's lumens make a sphere of its radius, so the kitchen's lamps, the lanterns of the kung fu dojo and
//!   the bursts of relish still light the room.
//! * **Unlit things** ([`unlit`]): a rasterised unlit material shows its colour as it is; Solari multiplies everything,
//!   unlit colour included, by the camera's exposure. Opaque unlit materials of the world are raised by the inverse so they
//!   look as they did, and their stand-ins glow with that colour.
//! * **Glows drawn additively** (the griddle's hot ring) are traced as glowing surfaces at the brightness they show.
//! * **The sun** gets an angular size (`SunDisk`), which is what makes Solari's traced shadows soft. Solari takes every
//!   directional light whatever its render layers, so the view model's own shadowless copy of the sun (`viewmodel`) is
//!   hidden from it in the render world ([`hush_view_suns`]); it would light the whole room a second time.
//! * **Exposure**: the rasterised look fills the room with ambient light and a generated reflection of it, which Solari
//!   does not use, so the game's camera is exposed [`EXPOSURE_LIFT`] stops brighter (the gun's camera is not).
//!
//! What it does not do: rays that leave the traced scene (up through the untraced ceiling, out of the open front of the
//! room) find nothing, so polished metal facing them reflects black; and there is no denoiser but temporal reuse and
//! TAA, so the picture is grainy for a few frames after anything moves.
use bevy::{
    asset::{AssetEvent, RenderAssetUsages},
    camera::{primitives::Aabb, visibility::RenderLayers, CameraMainTextureUsages, Exposure, Hdr},
    core_pipeline::prepass::{DeferredPrepass, DepthPrepass},
    light::{NotShadowCaster, SunDisk},
    mesh::{morph::MeshMorphWeights, skinning::SkinnedMesh, Indices, PrimitiveTopology, VertexAttributeValues},
    material::OpaqueRendererMethod,
    pbr::{ContactShadows, DefaultOpaqueRendererMethod, ScreenSpaceAmbientOcclusion},
    platform::collections::{HashMap, HashSet},
    prelude::*,
    render::render_resource::TextureUsages,
    solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins},
    ecs::{change_detection::Tick, system::SystemChangeTick},
    pbr::ExtractedDirectionalLight,
    render::{Render, RenderApp, RenderSystems},
    transform::TransformSystems,
};

use crate::maya::HotdogCamera;
use crate::viewmodel::ViewCamera;

/// The sun's angular size in radians: the disc on the ceiling is about 0.66 m across seen from 2 m, but the game's sun has
/// always cast crisp shadows; this is soft enough to read as a large lamp and keep the shapes.
const SUN_ANGLE: f32 = 0.09;
/// A lamp's glowing sphere is never smaller than this (metres): a smaller one is brighter for the same light, and noisier.
const LAMP_RADIUS: f32 = 0.02;
/// Glowing things smaller than this (metres, the largest side) are left out of the traced scene: the ceiling's pin beads
/// and the griddle's sparks would add many lights to sample for next to no light.
const SMALLEST_GLOW: f32 = 0.015;
/// How much brighter (in stops) the game's camera is exposed with traced light. The rasterised look fills the room with
/// an ambient term and a generated reflection of it (`visuals`), which Solari has no use for; and the ceiling is left out
/// of the traced scene (so that the sun on it still reaches the counter), so its bounce is missing too. What is traced is
/// the sun, the lamps, what glows and their bounces: about two stops darker. The view model's camera keeps its exposure:
/// the gun is still lit the rasterised way.
const EXPOSURE_LIFT: f32 = 1.5;
/// The camera moving further than this in a frame (metres) is a cut: the traced light's history is dropped.
const CUT: f32 = 0.6;
/// Meshes copied for tracing per frame at most (vertices), so a new map spreads its copies over a few frames.
const COPY_BUDGET: usize = 400_000;

pub fn build(app: &mut App) {
    app.add_plugins(SolariPlugins)
        // `SolariLightingPlugin` makes deferred the default for every opaque material; only `StandardMaterial`s are.
        .insert_resource(DefaultOpaqueRendererMethod::forward())
        .init_resource::<Traced>()
        .add_systems(Update, (deferred_materials, unlit, suns).after(crate::quality::Decide))
        .add_systems(PostUpdate, cameras.before(TransformSystems::Propagate))
        .add_systems(PostUpdate, (stand_ins, lamps).after(TransformSystems::Propagate));
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(Render, (restore_view_suns.in_set(RenderSystems::PrepareAssets), hush_view_suns.in_set(RenderSystems::PrepareResources)));
    }
}

/// In the render world: a sun that lights only the view model's layer (`viewmodel`'s shadowless copy of the sun, which
/// lights the gun) while Solari, which takes every directional light whatever its layers, builds its scene. It would
/// light the whole world a second time. Its illuminance is set to nothing after the rasterised lights are prepared
/// (`prepare_lights`, in `CreateViews`) and before Solari binds its lights (`PrepareBindGroups`), and put back before
/// the next frame's. (Lights are extracted again only when they change, which [`Hushed::at`] tells apart.)
#[derive(Component)]
struct Hushed {
    illuminance: f32,
    at: Tick,
}

fn hush_view_suns(mut commands: Commands, ticks: SystemChangeTick, mut lights: Query<(Entity, &mut ExtractedDirectionalLight, &RenderLayers), Without<Hushed>>) {
    for (entity, mut light, layers) in &mut lights {
        if !layers.intersects(&RenderLayers::layer(0)) && light.illuminance != 0. {
            debug!("Ultra: hushed a view-only sun of {} lux, layers {:?}", light.illuminance, layers);
            commands.entity(entity).insert(Hushed { illuminance: light.illuminance, at: ticks.this_run() });
            light.illuminance = 0.;
        }
    }
}

fn restore_view_suns(mut commands: Commands, mut lights: Query<(Entity, &mut ExtractedDirectionalLight, &Hushed)>) {
    for (entity, mut light, hushed) in &mut lights {
        // Untouched since it was hushed: put it back. (Extracted afresh since: it is already right.)
        if light.last_changed() == hushed.at {
            light.illuminance = hushed.illuminance;
        }
        commands.entity(entity).remove::<Hushed>();
    }
}

/// The traced scene's bookkeeping.
#[derive(Resource, Default)]
pub struct Traced {
    /// Each source entity's stand-in.
    stand_ins: HashMap<Entity, Entity>,
    /// Each light's glowing sphere, and its material.
    lamps: HashMap<Entity, (Entity, Handle<StandardMaterial>)>,
    /// Source meshes made traceable: the mesh to trace (the source itself where it already is) and its triangles, none
    /// where it cannot be.
    meshes: HashMap<AssetId<Mesh>, Option<(Handle<Mesh>, usize)>>,
    /// Source meshes changed since their copy was made.
    stale: HashSet<AssetId<Mesh>>,
    /// The stand-ins' own materials where they need one (an unlit source glows instead): source material to copy.
    glows: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    /// Unlit materials raised by the inverse of the exposure: the colour written, the colour it was raised from, the gain.
    raised: HashMap<AssetId<StandardMaterial>, (LinearRgba, LinearRgba, f32)>,
    deferred: usize,
    sphere: Option<Handle<Mesh>>,
    dome: Option<Handle<Mesh>>,
    triangles: usize,
    last_camera: Option<Vec3>,
}

impl Traced {
    pub fn summary(&self) -> String {
        format!(
            "raytraced lighting: {} stand-ins ({:.2} M triangles traced), {} lamps as glowing spheres, {} materials deferred, {} unlit raised",
            self.stand_ins.len(),
            self.triangles as f64 / 1e6,
            self.lamps.len(),
            self.deferred,
            self.raised.len()
        )
    }
}

/// Marks a stand-in in the traced scene: the entity it stands in for.
#[derive(Component)]
struct StandIn(#[allow(dead_code)] Entity);

/// The game camera's exposure before the lift (`EXPOSURE_LIFT`).
#[derive(Component)]
struct Lifted(f32);

/// Marks a lamp's glowing sphere.
#[derive(Component)]
struct LampGlow;

/// The game's camera gets Solari; the view-model camera what it needs to share the picture.
#[allow(clippy::type_complexity)]
fn cameras(
    mut commands: Commands,
    mut traced: ResMut<Traced>,
    mut games: Query<
        (
            Entity,
            Option<&mut SolariLighting>,
            Option<&CameraMainTextureUsages>,
            Has<Hdr>,
            &Msaa,
            Has<ScreenSpaceAmbientOcclusion>,
            Has<ContactShadows>,
            &GlobalTransform,
            Option<&Exposure>,
            Option<&Lifted>,
        ),
        (With<HotdogCamera>, Without<ViewCamera>),
    >,
    views: Query<(Entity, Option<&CameraMainTextureUsages>, Has<DeferredPrepass>, Has<DepthPrepass>, &Msaa, Option<&Exposure>), With<ViewCamera>>,
) {
    let mut own = None;
    let storage = |usages: Option<&CameraMainTextureUsages>| usages.is_some_and(|u| u.0.contains(TextureUsages::STORAGE_BINDING));
    let wanted = CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING);
    for (entity, solari, usages, hdr, msaa, occlusion, contact, at, exposure, lifted) in &mut games {
        let mut camera = commands.entity(entity);
        // The exposure lift, kept on top of whatever exposure the game gives the camera.
        let now = exposure.copied().unwrap_or_default().ev100;
        let base = match lifted {
            Some(l) if (now - (l.0 - EXPOSURE_LIFT)).abs() < 1e-4 => l.0,
            _ => {
                camera.insert((Lifted(now), Exposure { ev100: now - EXPOSURE_LIFT }));
                now
            }
        };
        own = Some(base);
        let here = at.translation();
        let cut = traced.last_camera.is_some_and(|last| last.distance(here) > CUT);
        traced.last_camera = Some(here);
        match solari {
            Some(mut solari) => {
                if cut {
                    debug!("Ultra: the camera cut; the traced light's history is dropped");
                    solari.reset = true;
                }
            }
            None => {
                camera.insert(SolariLighting::default());
            }
        }
        if !storage(usages) {
            camera.insert(wanted);
        }
        if !hdr {
            camera.insert(Hdr);
        }
        if *msaa != Msaa::Off {
            camera.insert(Msaa::Off);
        }
        // The traced light replaces both, and the deferred lighting they feed is skipped.
        if occlusion {
            camera.remove::<ScreenSpaceAmbientOcclusion>();
        }
        if contact {
            camera.remove::<ContactShadows>();
        }
    }
    for (entity, usages, deferred, depth, msaa, exposure) in &views {
        let mut view = commands.entity(entity);
        // (`viewmodel` copies the game camera's exposure here; the gun keeps the one it had.)
        if let Some(base) = own {
            if exposure.is_none_or(|e| (e.ev100 - base).abs() > 1e-4) {
                view.insert(Exposure { ev100: base });
            }
        }
        if !storage(usages) {
            view.insert(wanted);
        }
        // Its own StandardMaterials are deferred too: it needs a G-buffer of its own to draw them.
        if !deferred {
            view.insert(DeferredPrepass);
        }
        if !depth {
            view.insert(DepthPrepass);
        }
        if *msaa != Msaa::Off {
            view.insert(Msaa::Off);
        }
    }
}

/// Whether a material can be drawn deferred: opaque or alpha-masked, and not transmissive.
fn deferrable(material: &StandardMaterial) -> bool {
    matches!(material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_))
        && material.specular_transmission == 0.
        && material.diffuse_transmission == 0.
}

/// Makes every opaque `StandardMaterial` deferred, as it is added or changed, so that Solari's G-buffer has it.
fn deferred_materials(mut traced: ResMut<Traced>, mut events: MessageReader<AssetEvent<StandardMaterial>>, mut materials: ResMut<Assets<StandardMaterial>>, mut first: Local<bool>) {
    let ids: Vec<AssetId<StandardMaterial>> = if !*first {
        *first = true;
        materials.ids().collect()
    } else {
        events
            .read()
            .filter_map(|e| match e {
                AssetEvent::Added { id } | AssetEvent::Modified { id } => Some(*id),
                _ => None,
            })
            .collect()
    };
    for id in ids {
        let wanted = materials.get(id).is_some_and(|m| deferrable(m) && m.opaque_render_method != OpaqueRendererMethod::Deferred);
        if wanted && let Some(mut m) = materials.get_mut(id) {
            m.opaque_render_method = OpaqueRendererMethod::Deferred;
            traced.deferred += 1;
        }
    }
}

/// The exposure of the game's camera (what Solari multiplies everything by).
fn exposure(cameras: &Query<Option<&Exposure>, With<HotdogCamera>>) -> f32 {
    cameras.iter().next().flatten().copied().unwrap_or_default().exposure()
}

/// Raises the world's opaque unlit materials by the inverse of the exposure, so that they show as they did rasterised.
/// Every unlit material the view model's layer uses is left alone (its camera is lit the ordinary way).
fn unlit(
    mut traced: ResMut<Traced>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cameras: Query<Option<&Exposure>, With<HotdogCamera>>,
    used: Query<(&MeshMaterial3d<StandardMaterial>, Option<&RenderLayers>), With<Mesh3d>>,
) {
    let gain = 1. / exposure(&cameras).max(1e-6);
    let mut world = HashSet::new();
    let mut view = HashSet::new();
    for (material, layers) in &used {
        if layers.is_none_or(|l| l.intersects(&RenderLayers::default())) {
            world.insert(material.id());
        } else {
            view.insert(material.id());
        }
    }
    for id in world.difference(&view) {
        let Some(m) = materials.get(*id) else { continue };
        if !m.unlit || m.alpha_mode != AlphaMode::Opaque {
            continue;
        }
        let now = m.base_color.to_linear();
        let original = match traced.raised.get(id) {
            // Still what was written, at this exposure: nothing to do.
            Some((written, _, g)) if *written == now && *g == gain => continue,
            // Written, but the exposure has changed since: raise the original again.
            Some((written, original, _)) if *written == now => *original,
            // The game wrote a colour of its own since (a griddle warming up): raise that.
            _ => now,
        };
        let raised = LinearRgba::rgb(original.red * gain, original.green * gain, original.blue * gain).with_alpha(original.alpha);
        if let Some(mut m) = materials.get_mut(*id) {
            m.base_color = raised.into();
        }
        traced.raised.insert(*id, (raised, original, gain));
    }
}

/// Gives each sun an angular size: the traced shadows soften with it. (It changes nothing rasterised without an atmosphere.)
fn suns(mut commands: Commands, suns: Query<Entity, (With<DirectionalLight>, Without<SunDisk>)>) {
    for sun in &suns {
        commands.entity(sun).insert(SunDisk { angular_size: SUN_ANGLE, intensity: 1. });
    }
}

/// A copy of `mesh` that Solari can trace: triangles, exactly {position, normal, uv, tangent}, 32-bit indices. None for
/// what is not made of triangles. Missing normals are worked out, missing uvs are zero and missing tangents any vector
/// across the normal (they only turn normal maps, which a traced bounce barely shows).
pub fn traceable(mesh: &Mesh) -> Option<Mesh> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    // (`try_`: a mesh kept only in the render world has no data here any more.)
    let attribute = |id| mesh.try_attribute_option(id).ok().flatten();
    let Some(VertexAttributeValues::Float32x3(positions)) = attribute(Mesh::ATTRIBUTE_POSITION) else {
        return None;
    };
    let count = positions.len();
    let indices: Vec<u32> = match mesh.try_indices_option().ok()? {
        Some(indices) => indices.iter().map(|i| i as u32).collect(),
        None => (0..count as u32).collect(),
    };
    if count == 0 || indices.len() < 3 || indices.iter().any(|&i| i as usize >= count) {
        return None;
    }
    let normals = match attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(normals)) => normals.clone(),
        _ => {
            let mut sum = vec![Vec3::ZERO; count];
            for t in indices.chunks_exact(3) {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(positions[i as usize]));
                let n = (b - a).cross(c - a);
                for &i in t {
                    sum[i as usize] += n;
                }
            }
            sum.iter().map(|n| n.normalize_or(Vec3::Y).to_array()).collect()
        }
    };
    let uvs = match attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(uvs)) => uvs.clone(),
        _ => vec![[0., 0.]; count],
    };
    let tangents = match attribute(Mesh::ATTRIBUTE_TANGENT) {
        Some(VertexAttributeValues::Float32x4(tangents)) => tangents.clone(),
        _ => normals
            .iter()
            .map(|n| {
                let n = Vec3::from(*n);
                let across = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Z };
                n.cross(across).cross(n).normalize_or(Vec3::X).extend(1.).to_array()
            })
            .collect(),
    };
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
            .with_inserted_indices(Indices::U32(indices)),
    )
}

/// Whether Solari can trace `mesh` as it is.
pub fn traceable_as_is(mesh: &Mesh) -> bool {
    mesh.enable_raytracing
        && mesh.primitive_topology() == PrimitiveTopology::TriangleList
        && mesh.try_attributes().is_ok_and(|mut a| {
            a.by_ref()
                .map(|(a, _)| a.id)
                .eq([Mesh::ATTRIBUTE_POSITION.id, Mesh::ATTRIBUTE_NORMAL.id, Mesh::ATTRIBUTE_UV_0.id, Mesh::ATTRIBUTE_TANGENT.id])
        })
        && matches!(mesh.try_indices_option(), Ok(Some(Indices::U32(_))))
}

/// A mesh's triangles, where its data is still in the main world.
fn triangles(mesh: &Mesh) -> usize {
    match mesh.try_indices_option() {
        Ok(Some(i)) => i.len() / 3,
        Ok(None) => mesh.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok().flatten().map_or(0, |p| p.len() / 3),
        Err(_) => 0,
    }
}

/// Keeps the traced scene in step with the world: a stand-in for each source that should have one, where its source is.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn stand_ins(
    mut commands: Commands,
    mut traced: ResMut<Traced>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut material_events: MessageReader<AssetEvent<StandardMaterial>>,
    sources: Query<
        (
            Entity,
            &Mesh3d,
            // (A hotdog's bent parts, player_model/bend.rs, by the material they wear straight: traced as they are straight.)
            AnyOf<(&MeshMaterial3d<StandardMaterial>, &crate::player_model::bend::Unbent)>,
            &GlobalTransform,
            &InheritedVisibility,
            Option<&RenderLayers>,
            Has<NotShadowCaster>,
            Option<&Aabb>,
            Has<SkinnedMesh>,
            Has<MeshMorphWeights>,
        ),
        Without<StandIn>,
    >,
    mut stand_ins: Query<(&mut GlobalTransform, &mut RaytracingMesh3d, &mut MeshMaterial3d<StandardMaterial>), (With<StandIn>, Without<Mesh3d>)>,
    points: Query<&GlobalTransform, (Or<(With<PointLight>, With<SpotLight>)>, Without<StandIn>, Without<LampGlow>, Without<Mesh3d>)>,
    cameras: Query<Option<&Exposure>, With<HotdogCamera>>,
) {
    for event in mesh_events.read() {
        if let AssetEvent::Modified { id } | AssetEvent::Removed { id } = event {
            if traced.meshes.contains_key(id) {
                traced.stale.insert(*id);
            }
        }
    }
    // A glowing copy follows its source material (an additive one taking the exposure, as when it was made).
    let lift = 1. / exposure(&cameras).max(1e-6);
    for event in material_events.read() {
        if let AssetEvent::Modified { id } | AssetEvent::Removed { id } = event {
            if let Some(copy) = traced.glows.get(id).cloned() {
                if let Some(source) = materials.get(*id).cloned() {
                    let gain = if matches!(source.alpha_mode, AlphaMode::Add) { lift } else { 1. };
                    if let Some(mut c) = materials.get_mut(&copy) {
                        *c = glow_of(&source, gain);
                    }
                }
            }
        }
    }
    let lights: Vec<Vec3> = points.iter().map(|t| t.translation()).collect();
    let mut budget = COPY_BUDGET;
    let mut seen = HashSet::with_capacity(traced.stand_ins.len());
    let mut total = 0;
    for (entity, mesh, (straight, unbent), at, shown, layers, shadowless, aabb, skinned, morphing) in &sources {
        let Some(material) = straight.map(|m| m.0.clone()).or_else(|| unbent.map(|u| u.0.clone())).map(MeshMaterial3d) else {
            continue;
        };
        let material = &material;
        // An additive unlit glow (the griddle's hot ring) is drawn over the picture, not lit; traced, it is what it shows: a
        // glowing surface. It is not raised by `unlit` (that is for opaque ones), so its glow takes the exposure here.
        let Some((unlit, glowing, opaque, additive)) = materials.get(&material.0).map(|m| {
            let additive = m.unlit && matches!(m.alpha_mode, AlphaMode::Add);
            (m.unlit, m.unlit || m.emissive.red + m.emissive.green + m.emissive.blue > 0., m.alpha_mode == AlphaMode::Opaque && m.specular_transmission == 0., additive)
        }) else {
            continue;
        };
        let world_layer = layers.is_none_or(|l| l.intersects(&RenderLayers::default()));
        let mut eligible = shown.get() && world_layer && !skinned && !morphing && (opaque || additive);
        if eligible && (shadowless || additive) {
            // What casts no shadow is left out, unless it glows: then it is kept, if it is not tiny and holds no lamp.
            eligible = glowing
                && aabb.is_some_and(|b| {
                    let size = Vec3::from(b.half_extents) * 2. * at.compute_transform().scale.abs();
                    let (lo, hi) = (Vec3::from(b.min()), Vec3::from(b.max()));
                    let inverse = at.affine().inverse();
                    size.max_element() >= SMALLEST_GLOW
                        && !lights.iter().any(|p| {
                            let local = inverse.transform_point3(*p);
                            local.cmpge(lo).all() && local.cmple(hi).all()
                        })
                });
        }
        if !eligible {
            continue;
        }
        // The mesh to trace: the source's own if Solari can take it as it is, else a copy (made within the budget).
        let id = mesh.0.id();
        let stale = traced.stale.remove(&id);
        if stale || !traced.meshes.contains_key(&id) {
            let Some(source) = meshes.get(id) else { continue };
            let made = if traceable_as_is(source) {
                Some((mesh.0.clone(), triangles(source)))
            } else if budget > 0 {
                let copy = traceable(source);
                budget = budget.saturating_sub(copy.as_ref().map_or(1000, |c| triangles(c) * 3));
                copy.map(|copy| {
                    let n = triangles(&copy);
                    (meshes.add(copy), n)
                })
            } else {
                if stale {
                    traced.stale.insert(id);
                }
                continue;
            };
            traced.meshes.insert(id, made);
        }
        let Some(Some((trace, count))) = traced.meshes.get(&id).cloned() else { continue };
        total += count;
        // Its material: the source's own, or for an unlit one a copy that glows with its colour.
        let look = if unlit {
            let copy = traced.glows.get(&material.0.id()).cloned();
            match copy {
                Some(copy) => copy,
                None => {
                    let gain = if additive { 1. / exposure(&cameras).max(1e-6) } else { 1. };
                    let glow = glow_of(materials.get(&material.0).unwrap_or(&StandardMaterial::default()), gain);
                    let copy = materials.add(glow);
                    traced.glows.insert(material.0.id(), copy.clone());
                    copy
                }
            }
        } else {
            material.0.clone()
        };
        seen.insert(entity);
        match traced.stand_ins.get(&entity).copied().and_then(|s| stand_ins.get_mut(s).ok().map(|q| (s, q))) {
            Some((_, (mut place, mut traced_mesh, mut traced_material))) => {
                if *place != *at {
                    *place = *at;
                }
                if traced_mesh.0 != trace {
                    traced_mesh.0 = trace;
                }
                if traced_material.0 != look {
                    traced_material.0 = look;
                }
            }
            None => {
                let s = commands.spawn((StandIn(entity), RaytracingMesh3d(trace), MeshMaterial3d(look), Transform::from_matrix(at.to_matrix()), *at)).id();
                traced.stand_ins.insert(entity, s);
            }
        }
    }
    traced.triangles = total;
    // Stand-ins whose source is gone, hidden or no longer fit to trace.
    let gone: Vec<(Entity, Entity)> = traced.stand_ins.iter().filter(|(source, _)| !seen.contains(*source)).map(|(a, b)| (*a, *b)).collect();
    for (source, stand_in) in gone {
        traced.stand_ins.remove(&source);
        if let Ok(mut e) = commands.get_entity(stand_in) {
            e.despawn();
        }
    }
}

/// An unlit material as the traced scene sees it: black, glowing with its colour (already raised by `unlit`) times `gain`.
fn glow_of(source: &StandardMaterial, gain: f32) -> StandardMaterial {
    let c = source.base_color.to_linear();
    StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(c.red * gain, c.green * gain, c.blue * gain),
        emissive_texture: source.base_color_texture.clone(),
        unlit: false,
        ..default()
    }
}

/// How bright (in nits) a sphere of radius `r` glows to give off `lumens`: a sphere's emitted flux is its radiance times
/// pi times its area (4 pi r^2).
pub fn lamp_radiance(lumens: f32, r: f32) -> f32 {
    lumens / (4. * std::f32::consts::PI * std::f32::consts::PI * r * r)
}

/// Each shown point or spot light, as a small glowing sphere (half a sphere, facing its way, for a spot) in the traced scene.
#[allow(clippy::type_complexity)]
fn lamps(
    mut commands: Commands,
    mut traced: ResMut<Traced>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    points: Query<(Entity, &PointLight, &GlobalTransform, Option<&InheritedVisibility>), Without<LampGlow>>,
    spots: Query<(Entity, &SpotLight, &GlobalTransform, Option<&InheritedVisibility>), Without<LampGlow>>,
    mut glows: Query<&mut GlobalTransform, (With<LampGlow>, Without<PointLight>, Without<SpotLight>)>,
) {
    let sphere = traced.sphere.get_or_insert_with(|| meshes.add(Sphere::new(1.).mesh().uv(16, 8).with_generated_tangents().unwrap())).clone();
    let dome = traced
        .dome
        .get_or_insert_with(|| {
            // Half a sphere, open side up (+y), bulging down: turned to face a spot's way.
            let mut half = Sphere::new(1.).mesh().uv(16, 8);
            if let Some(VertexAttributeValues::Float32x3(p)) = half.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
                for v in p.iter_mut() {
                    v[1] = v[1].min(0.);
                }
            }
            meshes.add(half.with_generated_tangents().unwrap())
        })
        .clone();
    let mut wanted: Vec<(Entity, Vec3, Quat, f32, LinearRgba, bool)> = vec![];
    for (entity, light, at, shown) in &points {
        if shown.is_none_or(|s| s.get()) && light.intensity > 0. {
            let r = light.radius.max(LAMP_RADIUS);
            let c = light.color.to_linear() * lamp_radiance(light.intensity, r);
            wanted.push((entity, at.translation(), Quat::IDENTITY, r, c, false));
        }
    }
    for (entity, light, at, shown) in &spots {
        if shown.is_none_or(|s| s.get()) && light.intensity > 0. {
            let r = light.radius.max(LAMP_RADIUS);
            let c = light.color.to_linear() * lamp_radiance(light.intensity, r);
            // The dome bulges down (-y); turn -y to the spot's way (its forward).
            let turn = Quat::from_rotation_arc(Vec3::NEG_Y, at.forward().as_vec3());
            wanted.push((entity, at.translation(), turn, r, c, true));
        }
    }
    let mut seen = HashSet::new();
    for (light, at, turn, r, glow, spot) in wanted {
        seen.insert(light);
        let place = GlobalTransform::from(Transform::from_translation(at).with_rotation(turn).with_scale(Vec3::splat(r)));
        match traced.lamps.get(&light).cloned() {
            Some((sphere_entity, material)) => {
                if let Ok(mut t) = glows.get_mut(sphere_entity) {
                    if *t != place {
                        *t = place;
                    }
                }
                // Written only when the light changes: a written material is prepared for the GPU again.
                if materials.get(&material).is_some_and(|m| (m.emissive.red - glow.red).abs() > glow.red * 0.01 + 1e-3 || (m.emissive.blue - glow.blue).abs() > glow.blue * 0.01 + 1e-3) {
                    if let Some(mut m) = materials.get_mut(&material) {
                        m.emissive = glow;
                    }
                }
            }
            None => {
                let material = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: glow, ..default() });
                let e = commands
                    .spawn((LampGlow, RaytracingMesh3d(if spot { dome.clone() } else { sphere.clone() }), MeshMaterial3d(material.clone()), Transform::from_matrix(place.to_matrix()), place))
                    .id();
                traced.lamps.insert(light, (e, material));
            }
        }
    }
    let gone: Vec<Entity> = traced.lamps.keys().filter(|l| !seen.contains(*l)).copied().collect();
    for light in gone {
        if let Some((e, material)) = traced.lamps.remove(&light) {
            if let Ok(mut e) = commands.get_entity(e) {
                e.despawn();
            }
            materials.remove(&material);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_without_tangents_or_uvs_gets_a_copy_solari_can_trace() {
        let plain = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0., 0., 0.], [1., 0., 0.], [0., 0., 1.]])
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1., 0., 0., 1.]; 3])
            .with_inserted_indices(Indices::U16(vec![0, 2, 1]));
        assert!(!traceable_as_is(&plain));
        let copy = traceable(&plain).unwrap();
        assert!(traceable_as_is(&copy), "{:?}", copy.attributes().map(|(a, _)| a.name).collect::<Vec<_>>());
        // The normal is worked out from the winding (0, 2, 1 faces up).
        let Some(VertexAttributeValues::Float32x3(n)) = copy.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
        assert!(n.iter().all(|n| (Vec3::from(*n) - Vec3::Y).length() < 1e-5), "{n:?}");
        // Lines are not traced.
        let lines = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0., 0., 0.], [1., 0., 0.]]);
        assert!(traceable(&lines).is_none());
        // Bevy's own cuboid with tangents is traced as it is.
        assert!(traceable_as_is(&Cuboid::default().mesh().build().with_generated_tangents().unwrap()));
    }

    #[test]
    fn a_lamps_sphere_gives_off_the_lights_lumens() {
        // Radiance times pi (a Lambertian emitter's exitance) times the sphere's area is the flux.
        for (lumens, r) in [(600., 0.02), (2200., 0.12), (40., 0.05)] {
            let flux = lamp_radiance(lumens, r) * std::f32::consts::PI * 4. * std::f32::consts::PI * r * r;
            assert!((flux - lumens).abs() < lumens * 1e-4, "{lumens} {r}: {flux}");
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Engine frame times and main-camera submitted geometry, measured identically on both targets.
use bevy::{camera::visibility::VisibleEntities, prelude::*, render::renderer::RenderAdapterInfo};
use serde::Serialize;
use std::{any::TypeId, collections::HashMap};

#[derive(Resource, Default)]
pub struct Telemetry {
    samples: HashMap<String, Vec<f64>>,
    pub triangles: usize,
    pub instances: usize,
    pub fps: f64,
    tick: f64,
    pub done: bool,
}
#[derive(Serialize)]
struct Phase {
    frames: usize,
    fps: f64,
    median_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
}
fn percentile(v: &[f64], p: f64) -> f64 {
    v[((v.len() - 1) as f64 * p).round() as usize]
}
impl Telemetry {
    fn report(&self, adapter: String, elapsed: f64) -> String {
        let mut phases = std::collections::BTreeMap::new();
        for (name, values) in &self.samples {
            if values.is_empty() {
                continue;
            }
            let mut sorted = values.clone();
            sorted.sort_by(f64::total_cmp);
            phases.insert(
                name,
                Phase {
                    frames: sorted.len(),
                    fps: 1000. * sorted.len() as f64 / sorted.iter().sum::<f64>(),
                    median_ms: percentile(&sorted, 0.5),
                    p95_ms: percentile(&sorted, 0.95),
                    p99_ms: percentile(&sorted, 0.99),
                },
            );
        }
        serde_json::to_string_pretty(&serde_json::json!({"adapter":adapter,"elapsed_seconds":elapsed,"triangles_last_frame":self.triangles,"mesh_instances_last_frame":self.instances,"geometry_definition":"main-camera visible mesh triangles, before batching; excludes shadow redraws","frame_budget_ms":8.3333,"phases":phases})).unwrap()
    }
}

/// The lights drawing a shadow map this frame: shown, lit, and with their shadows on (each is a pass of its own, six for
/// a point light). `quality` turns them all off at Lowest; `?check` reports the count (`hotdogState.shadow_lights`).
#[derive(bevy::ecs::system::SystemParam)]
pub struct ShadowLights<'w, 's> {
    points: Query<'w, 's, (&'static PointLight, &'static InheritedVisibility)>,
    spots: Query<'w, 's, (&'static SpotLight, &'static InheritedVisibility)>,
    suns: Query<'w, 's, (&'static DirectionalLight, &'static InheritedVisibility)>,
}
impl ShadowLights<'_, '_> {
    pub fn count(&self) -> usize {
        self.points.iter().filter(|(l, v)| l.shadow_maps_enabled && l.intensity > 0. && v.get()).count()
            + self.spots.iter().filter(|(l, v)| l.shadow_maps_enabled && l.intensity > 0. && v.get()).count()
            + self.suns.iter().filter(|(l, v)| l.shadow_maps_enabled && l.illuminance > 0. && v.get()).count()
    }
}

/// What the scene is made of, for `?check&counts` (`hotdogState.counts`): entities, mesh entities (and how many are
/// shown, and what they belong to), the distinct meshes and standard materials they use, lights (shown), cameras, UI
/// nodes and texts, and what changed this frame. Counted every half second, only when asked: counting costs a frame
/// a millisecond or more.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Counts<'w, 's> {
    entities: Query<'w, 's, Entity>,
    meshes: Query<'w, 's, (&'static Mesh3d, Option<&'static MeshMaterial3d<StandardMaterial>>, &'static InheritedVisibility)>,
    points: Query<'w, 's, &'static InheritedVisibility, With<PointLight>>,
    spots: Query<'w, 's, &'static InheritedVisibility, With<SpotLight>>,
    suns: Query<'w, 's, &'static InheritedVisibility, With<DirectionalLight>>,
    cameras: Query<'w, 's, &'static Camera>,
    nodes: Query<'w, 's, &'static InheritedVisibility, With<Node>>,
    texts: Query<'w, 's, (), Or<(With<Text>, With<Text2d>)>>,
    materials: Res<'w, Assets<StandardMaterial>>,
    parents: Query<'w, 's, &'static ChildOf>,
    names: Query<'w, 's, &'static Name>,
    /// What changed since the last frame (this system runs every frame): what makes the engine redo work.
    changed: (
        Query<'w, 's, (), Changed<Visibility>>,
        Query<'w, 's, (), Changed<Transform>>,
        Query<'w, 's, (), Changed<GlobalTransform>>,
        Query<'w, 's, (), Or<(Changed<Text>, Changed<TextFont>, Changed<TextColor>)>>,
        Query<'w, 's, (), Changed<Node>>,
        Query<'w, 's, (), Or<(Changed<BackgroundColor>, Changed<BorderColor>, Changed<ImageNode>)>>,
        Query<'w, 's, (), Or<(Changed<MeshMaterial3d<StandardMaterial>>, Changed<Mesh3d>)>>,
    ),
    material_events: MessageReader<'w, 's, AssetEvent<StandardMaterial>>,
    mesh_events: MessageReader<'w, 's, AssetEvent<Mesh>>,
}
impl Counts<'_, '_> {
    /// Assets written since the last call (materials, meshes): each one is prepared for the GPU again.
    pub fn assets(&mut self) -> (usize, usize) {
        let changed = |e: &AssetEvent<StandardMaterial>| matches!(e, AssetEvent::Modified { .. });
        let materials = self.material_events.read().filter(|e| changed(e)).count();
        let meshes = self.mesh_events.read().filter(|e| matches!(e, AssetEvent::Modified { .. })).count();
        (materials, meshes)
    }
    pub fn report(&self) -> serde_json::Value {
        let mut meshes = std::collections::HashSet::new();
        let mut materials = std::collections::HashSet::new();
        let mut alike = std::collections::HashSet::new();
        let mut roots: HashMap<String, usize> = HashMap::new();
        let (mut shown, mut all) = (0, 0);
        for (entity, (mesh, material, visible)) in self.entities.iter().filter_map(|e| self.meshes.get(e).ok().map(|m| (e, m))) {
            all += 1;
            if visible.get() {
                shown += 1;
                meshes.insert(mesh.0.id());
                if let Some(m) = material {
                    materials.insert(m.0.id());
                    if let Some(found) = self.materials.get(&m.0) {
                        alike.insert(format!("{found:?}"));
                    }
                }
                // What it belongs to: the name of its topmost named ancestor (or its own).
                let (mut at, mut name) = (entity, None);
                loop {
                    if let Ok(n) = self.names.get(at) {
                        name = Some(n.as_str().split(['|', ':']).next().unwrap_or("").trim().to_string());
                    }
                    match self.parents.get(at) {
                        Ok(parent) => at = parent.parent(),
                        Err(_) => break,
                    }
                }
                *roots.entry(name.unwrap_or_else(|| "(unnamed)".into())).or_default() += 1;
            }
        }
        let mut roots: Vec<(String, usize)> = roots.into_iter().collect();
        roots.sort_by(|a, b| b.1.cmp(&a.1));
        roots.truncate(30);
        let lit = |q: &mut dyn Iterator<Item = bool>| q.filter(|v| *v).count();
        serde_json::json!({
            "entities": self.entities.iter().count(),
            "mesh_entities": all,
            "mesh_entities_shown": shown,
            "meshes_shown": meshes.len(),
            "standard_materials_shown": materials.len(),
            "standard_materials_shown_alike": alike.len(),
            "shown_by_root": roots,
            "standard_materials": self.materials.len(),
            "point_lights": lit(&mut self.points.iter().map(|v| v.get())),
            "spot_lights": lit(&mut self.spots.iter().map(|v| v.get())),
            "directional_lights": lit(&mut self.suns.iter().map(|v| v.get())),
            "cameras_active": self.cameras.iter().filter(|c| c.is_active).count(),
            "ui_nodes": self.nodes.iter().count(),
            "ui_nodes_shown": self.nodes.iter().filter(|v| v.get()).count(),
            "texts": self.texts.iter().count(),
            "changed_this_frame": {
                "visibility": self.changed.0.iter().count(),
                "transform": self.changed.1.iter().count(),
                "global_transform": self.changed.2.iter().count(),
                "text": self.changed.3.iter().count(),
                "node": self.changed.4.iter().count(),
                "ui_paint": self.changed.5.iter().count(),
                "mesh_or_material_handle": self.changed.6.iter().count(),
            },
        })
    }
}

pub fn sample(
    time: Res<Time<Real>>,
    options: Res<crate::runtime::Options>,
    game: Option<Res<crate::arena::Game>>,
    shift: Option<Res<crate::maya::BreakfastShift>>,
    assets: Res<Assets<Mesh>>,
    cameras: Query<(&Camera, &VisibleEntities)>,
    mesh_query: Query<&Mesh3d>,
    mut windows: Query<&mut Window>,
    adapter: Option<Res<RenderAdapterInfo>>,
    mut metrics: ResMut<Telemetry>,
    mut exit: MessageWriter<AppExit>,
    sheets: Option<Res<crate::loose::Sheets>>,
    net: Option<Res<crate::network::Network>>,
    shadows: ShadowLights,
    mut counts: Counts,
    work: Res<crate::quality::work::Work>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs_f64() * 1000.;
    let phase = game
        .as_ref()
        .filter(|g| g.active())
        .map(|g| g.phase_name())
        .unwrap_or_else(|| shift.as_ref().map(|s| s.phase_name()).unwrap_or("Loading"));
    if options.profile && dt > 0. && now > 3. {
        metrics.samples.entry(phase.into()).or_default().push(dt);
    }
    let modified = counts.assets();
    if now - metrics.tick < 0.5 {
        return;
    }
    metrics.tick = now;
    metrics.fps = if dt > 0. { 1000. / dt } else { 0. };
    let mut triangles = 0;
    let mut instances = 0;
    for (camera, visible) in &cameras {
        if !camera.is_active {
            continue;
        }
        for &e in visible.get(TypeId::of::<Mesh3d>()) {
            let Ok(mesh) = mesh_query.get(e) else {
                continue;
            };
            // Counted afresh each time: a mesh may be remade in place (player_model's lean and detailed sets).
            let Some(asset) = assets.get(&mesh.0) else {
                continue;
            };
            let count = if asset.primitive_topology() == bevy::render::render_resource::PrimitiveTopology::TriangleList {
                asset.indices().map(|i| i.len()).unwrap_or_else(|| asset.count_vertices()) / 3
            } else {
                0
            };
            triangles += count;
            instances += 1;
        }
    }
    metrics.triangles = triangles;
    metrics.instances = instances;
    let label = format!(
        "HOTDOG · {:.0} FPS · {:.2} ms · {} triangles · {} meshes",
        metrics.fps, dt, triangles, instances
    );
    for mut window in &mut windows {
        window.title = label.clone();
    }
    let state=game.as_ref().map(|g|serde_json::json!({"phase":g.phase_name(),"id":g.me(),"forts":g.fort,"elapsed":g.elapsed,"position":g.position.to_array(),"yaw":g.yaw,"infected":g.infected,"weapon":g.weapon as usize,"ammo":g.ammo,"scoped":g.scoped,"score":g.score,"kills":g.kills,"players":g.online_count,"network":g.network,"demo":g.demo,"roster":g.roster,"alert":g.alert,"wave_spawned":g.feast.wave_spawned,"emergences":g.feast.emergences,"enemies":g.feast.enemies,"wave_kills":g.feast.wave_kills,"friendly_fire":g.feast.friendly_fire_events,"winner":g.feast.winner,"rules":g.feast.rules.text(),"ends":g.feast.ends,"map":g.feast.level.map.name,"theme":g.feast.level.map.theme,"blocks":g.feast.level.map.blocks.len(),"pickups":g.feast.pickups.len(),"loose":crate::loose::report(&g.feast.props),"drapes":g.feast.drapes,"sheets":sheets.as_ref().map(|s|s.report()),"cloth_ms":sheets.as_ref().map(|s|s.cost()),"spawns":g.spawns(),"net":g.network.then(||crate::arena::online::report(g,net.as_deref())),"shadow_lights":shadows.count(),"work_ms":work.0,"counts":(options.check&&options.params.split(' ').any(|w|w=="counts")).then(||{let (m,n)=modified;let mut c=counts.report();c["assets_modified_this_frame"]=serde_json::json!({"materials":m,"meshes":n});c})})).unwrap_or_default();
    publish_metrics(&label, &state.to_string());
    if options.profile && now > 80. && !metrics.done {
        metrics.done = true;
        let adapter = adapter
            .map(|a| format!("{} / {:?} / {:?}", a.name, a.backend, a.device_type))
            .unwrap_or_default();
        let report = metrics.report(adapter, now);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = if options.profile_path.is_empty() {
                "reports/native-profile.json"
            } else {
                &options.profile_path
            };
            match std::fs::write(path, &report) {
                Ok(_) => info!("Profile saved: {path}"),
                Err(e) => error!("Profile write failed: {e}"),
            }
            exit.write(AppExit::Success);
        }
        #[cfg(target_arch = "wasm32")]
        {
            publish_profile(&report);
            let _ = &mut exit;
        }
    }
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function publish_metrics(label,state){window.hotdogMetrics=label;if(new URLSearchParams(location.search).has('check'))window.hotdogState=JSON.parse(state);const el=document.getElementById('fps');if(el)el.textContent=label.replace('HOTDOG · ','');}
export function publish_profile(report){window.hotdogProfile=JSON.parse(report);console.info('HOTDOG_PROFILE_READY');}
")]
extern "C" {
    fn publish_metrics(label: &str, state: &str);
    fn publish_profile(report: &str);
}
#[cfg(not(target_arch = "wasm32"))]
fn publish_metrics(_: &str, _: &str) {}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Low and Lowest (`quality::Detail::lean`): the scenery that never moves drawn as one mesh for each of its materials.
//!
//! The map and the room are built of hundreds of small parts (the walls and the floor's lines, the blocks and decks,
//! the ramps and their stripes, the microwave's shell, the ring's string and pennants, the posts and steamers), most
//! of them unit cubes and cylinders stretched into place. Each is an entity the engine checks, extracts, batches and
//! draws every frame; in a browser on a slow processor that work is most of the frame. A part that never moves,
//! never changes its look and is never hidden on its own is marked [`Still`] where it is built. While the level is
//! lean, every shown still part is merged, in world space, with the others of its material (and of its shadow and
//! its vertex layout) into one [`Baked`] mesh, and the parts themselves are hidden. Above Low the parts are shown
//! again and the baked meshes go. The picture is the same: the same triangles in the same places with the same
//! materials.
//!
//! A part is merged as it stands when it is baked: one moved, given another material or another mesh afterwards, or
//! a new or a despawned one, has everything baked again the next frame. Transparent parts are left as they are (one
//! merged mesh would be sorted as a whole), and so is any part its group could not take (another topology).
use bevy::{
    camera::visibility::RenderLayers,
    light::NotShadowCaster,
    mesh::{Indices, MeshVertexAttributeId},
    prelude::*,
};

/// A mesh that never moves, never changes its material or its mesh, and is never hidden on its own (nor is anything
/// it hangs from): at Low and Lowest it is drawn as part of a [`Baked`] mesh.
#[derive(Component, Clone, Copy, Default)]
pub struct Still;

/// One merged mesh standing in for still parts of one material, and how many.
#[derive(Component)]
pub struct Baked {
    #[cfg_attr(not(test), allow(dead_code))]
    pub parts: usize,
}

/// A still part hidden while its baked mesh stands in for it, and how it was shown before.
#[derive(Component)]
struct Away(Visibility);

/// What is baked: the meshes made, and whether the level wants them.
#[derive(Resource, Default)]
pub struct Bakery {
    meshes: Vec<Handle<Mesh>>,
    on: bool,
    /// The parts and the baked meshes they went into, for the checks and `?check`.
    pub parts: usize,
    pub baked: usize,
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bakery>()
            .add_systems(PostUpdate, follow.after(bevy::transform::TransformSystems::Propagate));
    }
}

/// What a group shares: its material, whether it casts shadows, its render layers and its vertex layout.
#[derive(PartialEq)]
struct Key {
    material: AssetId<StandardMaterial>,
    shadow: bool,
    layers: Option<RenderLayers>,
    layout: Vec<MeshVertexAttributeId>,
}

#[allow(clippy::type_complexity)]
fn follow(
    mut commands: Commands,
    detail: Option<Res<crate::quality::Detail>>,
    mut bakery: ResMut<Bakery>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    stills: Query<
        (Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>, &GlobalTransform, Option<&Visibility>, Option<&Away>, Option<&ChildOf>, Has<NotShadowCaster>, Option<&RenderLayers>),
        With<Still>,
    >,
    shown: Query<&InheritedVisibility>,
    changed: Query<(), (With<Still>, Or<(Added<Still>, Changed<GlobalTransform>, Changed<Mesh3d>, Changed<MeshMaterial3d<StandardMaterial>>)>)>,
    mut gone: RemovedComponents<Still>,
    baked: Query<Entity, With<Baked>>,
) {
    let want = detail.is_some_and(|d| d.lean);
    let moved = !changed.is_empty() || gone.read().count() > 0;
    if want == bakery.on && !(want && moved) {
        return;
    }
    // Take down what is baked, and show the parts again.
    for entity in &baked {
        commands.entity(entity).try_despawn();
    }
    for mesh in std::mem::take(&mut bakery.meshes) {
        meshes.remove(&mesh);
    }
    for (entity, .., away, _, _, _) in &stills {
        if let Some(Away(was)) = away {
            commands.entity(entity).try_insert(*was).try_remove::<Away>();
        }
    }
    (bakery.on, bakery.parts, bakery.baked) = (want, 0, 0);
    if !want {
        return;
    }
    // Every shown still part (shown as the game has it: what this hid is shown again above), by group.
    let mut groups: Vec<(Key, Handle<StandardMaterial>, Mesh, Vec<Entity>)> = vec![];
    for (entity, mesh, material, at, visibility, away, parent, unshadowed, layers) in &stills {
        let visibility = away.map_or(visibility.copied().unwrap_or_default(), |a| a.0);
        let parent_shown = parent.is_none_or(|p| shown.get(p.parent()).is_ok_and(|v| v.get()));
        if visibility == Visibility::Hidden || !parent_shown {
            continue;
        }
        if materials.get(&material.0).is_none_or(|m| m.alpha_mode != AlphaMode::Opaque) {
            continue;
        }
        // (A mesh whose data has gone to the GPU alone, without a copy here, cannot be merged: it is left as it is.)
        let Some(unit) = meshes.get(&mesh.0) else { continue };
        if unit.primitive_topology() != bevy::mesh::PrimitiveTopology::TriangleList {
            continue;
        }
        let Ok(Some(indices)) = unit.try_indices_option() else { continue };
        let indices: Vec<u32> = indices.iter().map(|i| i as u32).collect();
        let Ok(layout) = unit.try_attributes().map(|attributes| attributes.map(|(attribute, _)| attribute.id).collect::<Vec<_>>()) else { continue };
        let mut placed = unit.clone();
        if placed.try_transform_by(at.compute_transform()).is_err() {
            continue;
        }
        placed.insert_indices(Indices::U32(indices));
        let key = Key { material: material.0.id(), shadow: !unshadowed, layers: layers.cloned(), layout };
        match groups.iter_mut().find(|(k, ..)| *k == key) {
            Some((_, _, merged, parts)) => {
                if merged.merge(&placed).is_ok() {
                    parts.push(entity);
                }
            }
            None => groups.push((key, material.0.clone(), placed, vec![entity])),
        }
    }
    for (key, material, mesh, parts) in groups {
        // A part alone is drawn as it is.
        if parts.len() < 2 {
            continue;
        }
        let handle = meshes.add(mesh);
        let mut baked = commands.spawn((
            Baked { parts: parts.len() },
            crate::level::MapPart,
            Name::new("Baked scenery"),
            Mesh3d(handle.clone()),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Visibility::Visible,
            crate::visuals::Surveyed,
        ));
        if !key.shadow {
            baked.insert(NotShadowCaster);
        }
        if let Some(layers) = key.layers {
            baked.insert(layers);
        }
        bakery.meshes.push(handle);
        bakery.parts += parts.len();
        bakery.baked += 1;
        for part in parts {
            let (.., visibility, away, _, _, _) = stills.get(part).unwrap();
            let was = away.map_or(visibility.copied().unwrap_or_default(), |a| a.0);
            commands.entity(part).try_insert((Away(was), Visibility::Hidden));
        }
    }
    if bakery.baked > 0 {
        info!("Baked {} still parts into {} meshes", bakery.parts, bakery.baked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::{Detail, Level};
    use crate::visuals::Quality;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin));
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>();
        app.add_plugins(Plugin);
        app
    }

    fn scene(app: &mut App) -> (Vec<Entity>, Entity) {
        let world = app.world_mut();
        let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let (red, blue) = {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            (materials.add(Color::srgb(1., 0., 0.)), materials.add(Color::srgb(0., 0., 1.)))
        };
        // (No visibility systems here: the root is shown as they would have it.)
        let root = world.spawn((Transform::from_xyz(1., 0., 0.), Visibility::default(), InheritedVisibility::VISIBLE)).id();
        let mut parts = vec![];
        for (i, material) in [&red, &red, &red, &blue].into_iter().enumerate() {
            let part = world.spawn((Still, Mesh3d(cube.clone()), MeshMaterial3d(material.clone()), Transform::from_xyz(i as f32, 0., 0.))).id();
            world.entity_mut(root).add_child(part);
            parts.push(part);
        }
        // A moving part is never still.
        let moving = world.spawn((Mesh3d(cube), MeshMaterial3d(red), Transform::default())).id();
        (parts, moving)
    }

    #[test]
    fn at_low_and_lowest_still_parts_of_a_material_are_one_mesh_and_above_them_the_parts_are_back() {
        let mut app = app();
        let (parts, moving) = scene(&mut app);
        app.world_mut().insert_resource(Detail::of(Level::High, Quality::Full));
        app.update();
        app.update();
        assert_eq!(app.world_mut().query::<&Baked>().iter(app.world()).count(), 0);
        app.world_mut().insert_resource(Detail::of(Level::Lowest, Quality::Full));
        app.update();
        app.update();
        // Three red parts in one mesh; the blue part alone is left as it is.
        let baked: Vec<usize> = app.world_mut().query::<&Baked>().iter(app.world()).map(|b| b.parts).collect();
        assert_eq!(baked, [3]);
        let hidden = |app: &App, e: Entity| app.world().get::<Visibility>(e) == Some(&Visibility::Hidden);
        assert!(parts[..3].iter().all(|p| hidden(&app, *p)));
        assert!(!hidden(&app, parts[3]) && !hidden(&app, moving));
        // In world space: the root's move is in the vertices.
        let mesh = app.world_mut().query::<(&Mesh3d, &Baked)>().iter(app.world()).next().unwrap().0 .0.clone();
        let mesh = app.world().resource::<Assets<Mesh>>().get(&mesh).unwrap();
        let xs: Vec<f32> = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap().iter().map(|p| p[0]).collect();
        assert_eq!(xs.iter().copied().fold(f32::MAX, f32::min), 0.5);
        assert_eq!(xs.iter().copied().fold(f32::MIN, f32::max), 3.5);
        assert_eq!(mesh.count_vertices(), 3 * 24);
        // A still part moved (a map edited, say): baked again, from where it now stands.
        app.world_mut().get_mut::<Transform>(parts[0]).unwrap().translation.y = 2.;
        app.update();
        app.update();
        assert_eq!(app.world_mut().query::<&Baked>().iter(app.world()).count(), 1);
        // High again: the parts are shown as they were, and nothing is baked.
        app.world_mut().insert_resource(Detail::of(Level::High, Quality::Full));
        app.update();
        app.update();
        assert_eq!(app.world_mut().query::<&Baked>().iter(app.world()).count(), 0);
        assert!(parts.iter().all(|p| !hidden(&app, *p)));
        assert_eq!(app.world().resource::<Bakery>().meshes.len(), 0);
    }

    /// A map's scenery as `level::build` makes it, at `level`: (mesh entities shown, distinct mesh and material pairs
    /// among them (each at least a draw call), still parts baked, baked meshes).
    fn scenery(map: &str, level: Level) -> (usize, usize, usize, usize) {
        let mut app = app();
        app.init_asset::<Image>().init_asset::<AudioSource>();
        app.world_mut().insert_resource(Detail::of(level, Quality::Minimal));
        let map = kitchen_sim::map::Map::parse(map).unwrap();
        crate::level::build(app.world_mut(), &map);
        // (No visibility systems here: everything the map shows is shown as they would have it.)
        app.update();
        for entity in app.world_mut().query_filtered::<Entity, With<InheritedVisibility>>().iter(app.world()).collect::<Vec<_>>() {
            app.world_mut().entity_mut(entity).insert(InheritedVisibility::VISIBLE);
        }
        app.update();
        app.update();
        let world = app.world_mut();
        let drawn: Vec<(AssetId<Mesh>, Option<AssetId<StandardMaterial>>)> = world
            .query::<(&Mesh3d, Option<&MeshMaterial3d<StandardMaterial>>, Option<&Visibility>, Option<&ChildOf>)>()
            .iter(world)
            .filter(|(_, _, v, _)| *v != Some(&Visibility::Hidden))
            .map(|(m, mat, _, _)| (m.0.id(), mat.map(|m| m.0.id())))
            .collect();
        let pairs: std::collections::HashSet<_> = drawn.iter().copied().collect();
        let bakery = world.resource::<Bakery>();
        (drawn.len(), pairs.len(), bakery.parts, bakery.baked)
    }

    #[test]
    fn at_lowest_the_maps_scenery_keeps_to_its_budget() {
        // (mesh entities shown at most, distinct mesh and material pairs at most) at Lowest, and the most shown at High.
        // Measured when the budget was set (cpu-report.md): the scenery the main thread checks, extracts and draws
        // every frame, a few percent over what was achieved so that it cannot creep back.
        let maps = [
            ("counter-siege", include_str!("../assets/maps/counter-siege.json"), BUDGETS[0]),
            ("pillow-fort", include_str!("../assets/maps/pillow-fort.json"), BUDGETS[1]),
            ("kung-fu-dojo", include_str!("../assets/maps/kung-fu-dojo.json"), BUDGETS[2]),
        ];
        for (name, map, (entities, pairs)) in maps {
            let (high, high_pairs, _, _) = scenery(map, Level::High);
            let (shown, shown_pairs, parts, baked) = scenery(map, Level::Lowest);
            eprintln!("{name}: High {high} meshes shown in {high_pairs} pairs; Lowest {shown} in {shown_pairs} pairs ({parts} still parts baked into {baked})");
            assert!(baked > 0 && parts > baked, "{name}: something is baked");
            assert!(shown <= entities, "{name}: {shown} mesh entities shown at Lowest (budget {entities})");
            assert!(shown_pairs <= pairs, "{name}: {shown_pairs} mesh and material pairs at Lowest (budget {pairs})");
            assert!(shown < high, "{name}: Lowest shows fewer mesh entities than High ({shown} against {high})");
        }
    }
    /// Lowest's budgets for the three maps measured (counter-siege, pillow-fort, kung-fu-dojo): mesh entities shown,
    /// mesh and material pairs.
    const BUDGETS: [(usize, usize); 3] = [(76, 14), (314, 56), (44, 36)];

    #[test]
    fn a_hidden_or_see_through_part_is_not_baked() {
        let mut app = app();
        let (parts, _) = scene(&mut app);
        app.world_mut().entity_mut(parts[0]).insert(Visibility::Hidden);
        let glass = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() });
        app.world_mut().entity_mut(parts[1]).insert(MeshMaterial3d(glass));
        app.world_mut().insert_resource(Detail::of(Level::Low, Quality::Lite));
        app.update();
        app.update();
        // One red part left of three: not enough to merge.
        assert_eq!(app.world_mut().query::<&Baked>().iter(app.world()).count(), 0);
        assert_eq!(app.world().get::<Visibility>(parts[0]), Some(&Visibility::Hidden));
        assert_ne!(app.world().get::<Visibility>(parts[2]), Some(&Visibility::Hidden));
    }
}

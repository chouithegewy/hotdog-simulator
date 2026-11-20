// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The nod drawn. A hotdog's head bows and tips with its look (kitchen_sim head.rs: `nod`, `bend`, `head`), and its
//! body stays planted. What is drawn bends exactly as the simulation's head moves:
//!
//! * **The sausage, the buns and the mustard** wear a [`BentMaterial`]: their own material (each part's straight one,
//!   kept in step with it by `follow`, so whatever dresses it dresses them) with a vertex shader that puts them through
//!   the bend (`bend.wgsl`, the same numbers as head.rs), in the main pass, the prepasses and the shadows. The nod rides
//!   in each part's [`MeshTag`] (`tag`): every hotdog still shares one mesh and one material per part, so they batch as
//!   before, and nothing is skinned. A part bent wears [`Unbent`], the material it would wear straight.
//! * **The face** (the eyes, the mouth, the brows, the cheeks) and anything worn on the head (Forts' headband) hang from
//!   the rig's head (`Rig::head`), which the bend carries as one (`head_transform`).
//!
//! The drawn nod follows the simulation's eased (`ease`: EASE a second, never more than LAG behind), so a flick of the
//! mouse does not snap the neck; the simulation's head is the look's at once. A hotdog nobody sees has its nod eased but
//! not written: no tags, no transforms, nothing for the engine to carry to the screen.
use bevy::{
    mesh::MeshTag,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

/// How fast the drawn nod catches up with the look (of what is left, a second), and the most it trails it (radians).
/// At LAG the drawn head is under 4 mm from the simulation's (its middle is about 3 cm over the neck's top).
pub const EASE: f32 = 18.;
pub const LAG: f32 = 0.12;

/// The bend's vertex shaders over the standard material.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct Bend {}
impl MaterialExtension for Bend {
    fn vertex_shader() -> ShaderRef {
        "shaders/hotdog_bend.wgsl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/hotdog_bend_prepass.wgsl".into()
    }
    fn deferred_vertex_shader() -> ShaderRef {
        "shaders/hotdog_bend_prepass.wgsl".into()
    }
}
pub type BentMaterial = ExtendedMaterial<StandardMaterial, Bend>;

/// On a part drawn bent: the material it would wear straight (what the surfaces' dressing, Ultra's traced copies and the
/// rifle optic's glow go by).
#[derive(Component, Clone)]
pub struct Unbent(pub Handle<StandardMaterial>);

/// The bent copy of each straight material a hotdog's parts wear.
#[derive(Resource, Default)]
pub struct Bent {
    pairs: Vec<(Handle<StandardMaterial>, Handle<BentMaterial>)>,
}
/// The bent copy of `straight`, made the first time it is wanted; None where nothing is drawn bent (a test's world
/// without the renderer's materials).
pub fn bent(world: &mut World, straight: &Handle<StandardMaterial>) -> Option<Handle<BentMaterial>> {
    if !world.contains_resource::<Assets<BentMaterial>>() {
        return None;
    }
    let made = world.get_resource::<Bent>().and_then(|bent| bent.pairs.iter().find(|(s, _)| s.id() == straight.id()).map(|(_, b)| b.clone()));
    if made.is_some() {
        return made;
    }
    let base = world.get_resource::<Assets<StandardMaterial>>()?.get(straight)?.clone();
    let handle = world.resource_mut::<Assets<BentMaterial>>().add(ExtendedMaterial { base, extension: Bend {} });
    world.get_resource_or_insert_with(Bent::default).pairs.push((straight.clone(), handle.clone()));
    Some(handle)
}
/// Keeps each bent material as its straight one is: the surfaces' dressing (visuals/surfaces.rs) and Ultra's deferred
/// shading change the straight ones after the hotdogs are made.
fn follow(mut events: MessageReader<AssetEvent<StandardMaterial>>, bent: Option<Res<Bent>>, straight: Res<Assets<StandardMaterial>>, mut bents: ResMut<Assets<BentMaterial>>) {
    let Some(bent) = bent else {
        events.clear();
        return;
    };
    for event in events.read() {
        let (AssetEvent::Modified { id } | AssetEvent::Added { id }) = event else {
            continue;
        };
        for (s, b) in bent.pairs.iter().filter(|(s, _)| s.id() == *id) {
            if let (Some(base), Some(mut m)) = (straight.get(s), bents.get_mut(b)) {
                m.base = base.clone();
            }
        }
    }
}

/// The tag that carries a nod to the bend shader: its bits.
pub fn tag(nod: f32) -> MeshTag {
    MeshTag(nod.to_bits())
}
/// The head's place on the torso for a nod: the bend's turn and shift of all that is over the neck (head.rs `head`).
pub fn head_transform(nod: f32) -> Transform {
    let (shift, turn) = kitchen_sim::head::head(nod);
    Transform { translation: shift, rotation: turn, ..default() }
}
/// The drawn nod one frame of `dt` seconds on, after the simulation's `target`: eased, and never more than LAG behind.
pub fn ease(drawn: f32, target: f32, dt: f32) -> f32 {
    let eased = drawn + (target - drawn) * (1. - (-dt.max(0.) * EASE).exp());
    eased.clamp(target - LAG, target + LAG)
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        bevy::shader::load_shader_library!(app, "bend.wgsl");
        app.add_plugins(MaterialPlugin::<BentMaterial>::default()).add_systems(PostUpdate, follow);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Cues, Gait, PlayerModels, Rig, animate};
    use super::*;
    use kitchen_sim::feast::Player;
    use kitchen_sim::head::{self, HEAD_AT};

    /// A world with the bent materials and `n` hotdogs (the first at the origin facing `yaw`), detailed or lean.
    fn hotdogs(n: u32, yaw: f32, detailed: bool) -> (App, Vec<Entity>, PlayerModels) {
        let mut app = App::new();
        app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>().init_asset::<BentMaterial>();
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let models = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|_, mut materials: Mut<Assets<StandardMaterial>>| PlayerModels::new(&mut meshes, &mut materials, &server, detailed))
        });
        world.insert_resource(models.clone());
        let roots = (1..=n)
            .map(|id| {
                let root = models.spawn(world, Vec3::X * (id - 1) as f32, id);
                world.get_mut::<Transform>(root).unwrap().rotation = Quat::from_rotation_y(yaw);
                root
            })
            .collect();
        (app, roots, models)
    }
    /// The bent parts and the head of a hotdog's rig, and the drawn nod.
    fn rig(world: &World, root: Entity) -> Rig {
        world.get::<Rig>(root).unwrap().clone()
    }
    fn drawn_nod(world: &World, root: Entity) -> f32 {
        world.get::<Gait>(root).unwrap().motion().nod
    }
    /// Marks a hotdog seen (as the renderer would, last frame) or not.
    fn seen(world: &mut World, root: Entity, on: bool) {
        let part = rig(world, root).bent[0];
        world.entity_mut(part).insert(if on { ViewVisibility::VISIBLE } else { ViewVisibility::HIDDEN });
    }
    /// Where the drawn head's middle is in the world, and the way its face looks: the rest of the head (a standing
    /// hotdog's frame) through the head, the torso and the root as drawn.
    fn drawn_head(world: &World, root: Entity) -> (Vec3, Vec3) {
        let r = rig(world, root);
        let frame = *world.get::<Transform>(root).unwrap() * *world.get::<Transform>(r.torso).unwrap() * *world.get::<Transform>(r.head).unwrap();
        (frame.transform_point(Vec3::Y * HEAD_AT), frame.rotation * Vec3::NEG_Z)
    }
    /// The simulation's hotdog where hotdog `root` is drawn, looking `pitch`.
    fn simulated(world: &World, root: Entity, pitch: f32) -> Player {
        let tr = world.get::<Transform>(root).unwrap();
        let (yaw, _, _) = tr.rotation.to_euler(EulerRot::YXZ);
        Player { p: tr.translation.to_array(), yaw, pitch, alive: true, grounded: true, ..Player::default() }
    }
    fn look(pitch: f32) -> Cues {
        Cues { alive: true, weapon: 1, pitch, ..default() }
    }

    #[test]
    fn the_drawn_head_is_where_the_simulations_head_is_at_every_look() {
        let (mut app, roots, _) = hotdogs(1, 0.7, false);
        let (world, root) = (app.world_mut(), roots[0]);
        seen(world, root, true);
        let mut time = 1.;
        for pitch in [-1.5, -1.1, -0.6, -0.2, 0., 0.3, 0.8, 1.5] {
            // Held long enough for the ease to arrive.
            for _ in 0..40 {
                time += 1. / 60.;
                animate(world, root, time, look(pitch));
            }
            let sim = simulated(world, root, pitch);
            let (centre, face) = drawn_head(world, root);
            let h = head::Head::of(&sim);
            assert!(centre.distance(h.centre()) < 2e-4, "pitch {pitch}: drawn {centre} simulated {}", h.centre());
            assert!(face.angle_between(h.turn * Vec3::NEG_Z) < 1e-3, "pitch {pitch}: the face looks where the simulation's does");
            // The parts drawn bent carry the same nod; the head is the bend's.
            let nod = head::nod(0, pitch);
            assert!((drawn_nod(world, root) - nod).abs() < 2e-4);
            for part in rig(world, root).bent {
                assert_eq!(world.get::<bevy::mesh::MeshTag>(part).unwrap().0, drawn_nod(world, root).to_bits());
            }
            assert!(world.get::<Transform>(rig(world, root).head).unwrap().translation.distance(head_transform(nod).translation) < 2e-5);
        }
        // Crouched and prone the simulation keeps its own frame for the stance (the old sphere's place); the head turns on
        // the torso exactly as the simulation's head turns.
        for (cues, stance) in [(Cues { crouch: true, ..look(-1.2) }, 1), (Cues { prone: true, ..look(-1.2) }, 2), (Cues { prone: true, ..look(1.2) }, 2)] {
            for _ in 0..40 {
                time += 1. / 60.;
                animate(world, root, time, cues);
            }
            let want = head_transform(head::nod(stance, cues.pitch));
            let at = *world.get::<Transform>(rig(world, root).head).unwrap();
            assert!(at.translation.distance(want.translation) < 2e-5 && at.rotation.angle_between(want.rotation) < 2e-4, "stance {stance}");
        }
    }

    #[test]
    fn the_drawn_nod_eases_after_the_look_without_snapping_and_never_far_behind() {
        let (mut app, roots, _) = hotdogs(1, -0.4, true);
        let (world, root) = (app.world_mut(), roots[0]);
        seen(world, root, true);
        // A quick mouse: level, fully down in a tenth of a second, held; fully up in a twentieth, held; and a level look
        // all at once (a respawn).
        let pitch = |t: f32| match t {
            t if t < 0.2 => 0.,
            t if t < 0.3 => -1.5 * (t - 0.2) / 0.1,
            t if t < 0.8 => -1.5,
            t if t < 0.85 => -1.5 + 3. * (t - 0.8) / 0.05,
            t if t < 1.4 => 1.5,
            _ => 0.,
        };
        // The simulation's head's fastest step in a frame through all that (without the respawn's jump).
        let fastest = (1..84).map(|k| head::head_centre(&simulated(world, root, pitch(k as f32 / 60.))).distance(head::head_centre(&simulated(world, root, pitch((k - 1) as f32 / 60.))))).fold(0f32, f32::max);
        let (mut worst, mut was) = (0f32, None::<(Vec3, Vec3)>);
        for k in 0..100 {
            let t = k as f32 / 60.;
            animate(world, root, 1. + t, look(pitch(t)));
            let sim = head::head_centre(&simulated(world, root, pitch(t)));
            let (drawn, _) = drawn_head(world, root);
            let behind = drawn.distance(sim);
            worst = worst.max(behind);
            // Never more than LAG behind the look: a few millimetres at the head's middle.
            assert!((drawn_nod(world, root) - head::nod(0, pitch(t))).abs() <= LAG + 1e-5, "{t}");
            assert!(behind < 0.0045, "at {t} s the drawn head is {behind} m from the simulation's");
            // No snap: a frame never moves it further than the look moves the simulation's head in its fastest frame (but
            // where the look itself jumps).
            if let Some((d0, s0)) = was.filter(|(_, s0)| sim.distance(*s0) < 0.01) {
                assert!(drawn.distance(d0) <= fastest + 1e-4, "at {t} s: drawn {} (sim {}, fastest {fastest})", drawn.distance(d0), sim.distance(s0));
            }
            was = Some((drawn, sim));
            // Held a quarter of a second, it has arrived.
            if (0.55..0.8).contains(&t) || (1.15..1.4).contains(&t) {
                assert!(behind < 3e-4, "at {t} s still {behind} m behind");
            }
        }
        println!("the drawn head at most {:.2} mm from the simulation's through a fast flick", worst * 1e3);
        assert!(worst > 0.001, "it does ease: {worst}");
    }

    #[test]
    fn every_hotdog_shares_the_bent_materials_and_the_lean_hotdog_nods_too() {
        let (mut app, roots, mut models) = hotdogs(3, 0., true);
        let world = app.world_mut();
        // Three bent materials for every hotdog there is (the sausage's, the buns' and the mustard's), each a copy of
        // the part's straight one, which it wears as `Unbent`.
        assert_eq!(world.resource::<Assets<BentMaterial>>().len(), 3);
        let parts: Vec<Entity> = roots.iter().flat_map(|root| rig(world, *root).bent).collect();
        let bent: std::collections::HashSet<_> = parts.iter().map(|e| world.get::<MeshMaterial3d<BentMaterial>>(*e).unwrap().0.id()).collect();
        assert_eq!(bent.len(), 3);
        assert!(parts.iter().all(|e| world.get::<Unbent>(*e).is_some() && world.get::<MeshMaterial3d<StandardMaterial>>(*e).is_none()));
        // Low and Lowest: the same parts, their meshes remade lean in place; the nod as before.
        assert!(world.resource_scope(|_, mut meshes: Mut<Assets<Mesh>>| models.refine(&mut meshes, false)));
        let root = roots[1];
        seen(world, root, true);
        for k in 0..30 {
            animate(world, root, 1. + k as f32 / 60., look(-1.5));
        }
        let nod = drawn_nod(world, root);
        assert!((nod + head::BOW).abs() < 1e-3);
        for part in rig(world, root).bent {
            assert_eq!(world.get::<bevy::mesh::MeshTag>(part).unwrap().0, nod.to_bits());
        }
        let (centre, _) = drawn_head(world, root);
        assert!(centre.distance(head::head_centre(&simulated(world, root, -1.5))) < 3e-4);
    }

    #[test]
    fn a_hotdog_nobody_sees_has_its_nod_eased_but_not_written() {
        let (mut app, roots, _) = hotdogs(1, 0., true);
        let (world, root) = (app.world_mut(), roots[0]);
        seen(world, root, false);
        let r = rig(world, root);
        for k in 0..30 {
            animate(world, root, 1. + k as f32 / 60., look(-1.2));
        }
        assert!(drawn_nod(world, root) < -0.5, "eased all the same");
        assert_eq!(world.get::<bevy::mesh::MeshTag>(r.bent[0]).unwrap().0, 0f32.to_bits(), "no tag written");
        assert_eq!(*world.get::<Transform>(r.head).unwrap(), Transform::IDENTITY, "nor the head moved");
        // Seen again, it is drawn where it is at once.
        seen(world, root, true);
        animate(world, root, 1.5, look(-1.2));
        assert_eq!(world.get::<bevy::mesh::MeshTag>(r.bent[0]).unwrap().0, drawn_nod(world, root).to_bits());
    }

    #[test]
    fn in_kung_fu_the_head_does_not_nod_and_the_body_leans_as_it_did() {
        let (mut app, roots, _) = hotdogs(1, 0., true);
        let (world, root) = (app.world_mut(), roots[0]);
        seen(world, root, true);
        for k in 0..30 {
            animate(world, root, 1. + k as f32 / 60., Cues { kungfu: true, ..look(-1.) });
        }
        let r = rig(world, root);
        assert_eq!(drawn_nod(world, root), 0.);
        assert_eq!(*world.get::<Transform>(r.head).unwrap(), Transform::IDENTITY);
        assert!(r.bent.iter().all(|e| world.get::<bevy::mesh::MeshTag>(*e).is_none_or(|t| t.0 == 0)));
        let motion = world.get::<Gait>(root).unwrap().motion();
        assert_eq!(motion.lean, 1.);
        assert!(super::super::pose(motion).torso.mul_vec3(Vec3::Y).z < -0.1, "leant forward with the look, as before");
    }

    #[test]
    fn the_bend_shader_bends_by_the_simulations_numbers() {
        // The shader's constants are head.rs's.
        let wgsl = include_str!("bend.wgsl");
        let value = |name: &str| -> f32 {
            let line = wgsl.lines().find(|l| l.starts_with(&format!("const {name}: f32 = "))).unwrap_or_else(|| panic!("{name}"));
            line.split('=').nth(1).unwrap().trim().trim_end_matches(';').parse().unwrap()
        };
        assert_eq!((value("NECK_LOW"), value("NECK_HIGH")), (head::NECK[0], head::NECK[1]));
        assert!((value("BOW") - head::BOW).abs() < 1e-6 && value("SQUASH") == head::SQUASH);
        // A port of the shader's `bend`, line for line, is head.rs's.
        let sinc = |x: f32| if x.abs() < 1e-3 { 1. - x * x / 6. } else { x.sin() / x };
        let cosc = |x: f32| if x.abs() < 1e-3 { x / 2. - x * x * x / 24. } else { (1. - x.cos()) / x };
        let turn_x = |a: f32, v: Vec3| Vec3::new(v.x, v.y * a.cos() - v.z * a.sin(), v.y * a.sin() + v.z * a.cos());
        let shader = |nod: f32, p: Vec3| {
            let (low, high) = (value("NECK_LOW"), value("NECK_HIGH"));
            if p.y <= low || nod == 0. {
                return p;
            }
            let along = p.y.min(high) - low;
            let turn = nod * along / (high - low);
            let length = (1. - value("SQUASH") * ((-nod).max(0.) / value("BOW")).min(1.)) * along;
            Vec3::new(0., low + length * sinc(turn), length * cosc(turn)) + turn_x(turn, Vec3::new(p.x, (p.y - high).max(0.), p.z))
        };
        for nod in [-head::BOW, -0.3, -1e-4, 0., 2e-4, 0.2, head::TIP] {
            for p in [Vec3::new(0.01, 0.05, -0.02), Vec3::new(-0.02, 0.1, -0.026), Vec3::new(0.03, 0.115, 0.038), Vec3::new(0.009, 0.153, -0.0255), Vec3::new(0., 0.178, 0.)] {
                assert!(shader(nod, p).distance(head::bend(nod, p)) < 1e-6, "{nod} {p}");
                // (And the head's transform is the bend over the neck.)
                if p.y >= head::NECK[1] {
                    assert!(head_transform(nod).transform_point(p).distance(head::bend(nod, p)) < 1e-6);
                }
            }
        }
        // The nod rides in the tag bit for bit; no tag is no nod.
        assert_eq!(f32::from_bits(tag(-0.61).0), -0.61);
        assert_eq!(f32::from_bits(0), 0.);
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Topsy-turvy: a joke kept off every screen, in no menu, hint or note. A player who turns the look's Y over (settings.rs
//! `invert_y`) is turned over too: no longer a hotdog with a gun, a gun that shoots hotdogs.
//!
//! * **In first person** ([`View`]): where the gun was, a small hotdog (its own sausage, buns and mustard, lying along
//!   the aim) held in hands of gun metal; the gun itself is put away while it lasts.
//! * **The rounds** this player fires are drawn as tiny hotdogs tumbling end over end along the round's own path (the
//!   round's drawn condiment is hidden and the hotdog put where it is, every frame). Only the look changes: what the
//!   simulation (or the server) fires, where it goes, what it hits and the harm it does are as they always were. Nobody
//!   else's rounds change.
//! * **The body**, wherever this player sees its own (the death camera, Kung Fu over the shoulder, its figure in the
//!   fridge between rounds): a walking gun, the gun in hand stood on its butt at a hotdog's height on the hotdog's own
//!   little legs, with its eyes on, walking and falling as the hotdog would.
//! * **A squeak** the first time it is seen in a session.
//!
//! It is this screen's alone: the other players see a hotdog as ever. (Protocol 12's per-hotdog flags have bits to
//! spare, but they are the server's to set from what it is sent, and a command has no bit for it: showing it to others
//! would take the server's part too.)
use crate::arena::Game;
use crate::player_model::{Food, Gait, PlayerBody, Rig};
use bevy::audio::Volume;
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::PI;

/// The first-person hotdog: its place under the gun's root (where the hand grips), its size; a round's hotdog's size and
/// how fast it tumbles (radians a second); how tall the walking gun stands, from where its legs meet it.
const HELD_AT: Vec3 = Vec3::new(0., 0.008, 0.002);
const HELD: f32 = 0.4;
const ROUND: f32 = 0.2;
const TUMBLE: f32 = 17.;
const STANDS: (f32, f32) = (0.035, 0.155);
/// The middle of the hotdog's sausage, and its length (player_model.rs `Food`).
const MIDDLE: f32 = 0.1;

/// Whether it is on: exactly when the look's Y is turned over.
pub fn on(settings: Option<&crate::settings::Settings>) -> bool {
    settings.is_some_and(|s| s.invert_y)
}
/// Whether a round fired by `owner` is drawn as a hotdog on the screen of player `me` (on, `on`; watching, `demo`).
pub fn dressed(owner: u32, me: u32, on: bool, demo: bool) -> bool {
    on && !demo && owner == me && me != 0
}

/// What has been changed, to be undone.
#[derive(Resource, Default)]
pub struct Topsy {
    /// Each first-person gun's root: what was hidden there (and how it was), the hands' own material, and the hotdog
    /// put in them.
    views: HashMap<Entity, View>,
    /// Each of this player's rounds in the air: its hotdog.
    pub rounds: HashMap<u32, Entity>,
    /// Each of this player's bodies (by its root): the gun it is, which gun, its eyes, and what of the hotdog was hidden.
    bodies: HashMap<Entity, Body>,
    look: Option<Look>,
    /// Squeaked already this session.
    laughed: bool,
}
struct View {
    hidden: Vec<(Entity, Visibility)>,
    hands: Vec<(Entity, Handle<StandardMaterial>)>,
    hotdog: Entity,
}
struct Body {
    gun: Entity,
    weapon: usize,
    hidden: Vec<(Entity, Visibility)>,
}
/// The meshes and materials it is made of.
#[derive(Clone)]
struct Look {
    food: Food,
    metal: Handle<StandardMaterial>,
    squeak: Handle<AudioSource>,
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Topsy>().add_systems(Update, dress.after(crate::arena::tick).after(crate::deathcam::watch));
    }
}

/// A hotdog built of `food` hanging from `parent` at `place`: the sausage stood on y, the buns behind it, the mustard down
/// its front (the hotdog's own frame, scaled and turned by `place`).
fn hotdog(world: &mut World, food: &Food, parent: Option<Entity>, place: Transform) -> Entity {
    let root = world.spawn((Name::new("Hotdog"), place, Visibility::Inherited)).id();
    if let Some(parent) = parent {
        world.entity_mut(parent).add_child(root);
    }
    let parts = [
        (food.sausage.clone(), food.skin.clone(), Transform::default()),
        (food.bun.clone(), food.bread.clone(), Transform::from_xyz(-crate::player_model::BUN, 0., 0.)),
        (food.bun.clone(), food.bread.clone(), Transform::from_xyz(crate::player_model::BUN, 0., 0.)),
        (food.mustard.clone(), food.sauce.clone(), Transform::default()),
    ];
    for (mesh, material, at) in parts {
        let part = world.spawn((Mesh3d(mesh), MeshMaterial3d(material), at, bevy::light::NotShadowCaster)).id();
        world.entity_mut(root).add_child(part);
    }
    root
}
/// A short rubber squeak and honk, made here (16-bit mono WAV).
pub fn squeak() -> Vec<u8> {
    let rate = 22050u32;
    let n = (rate as f32 * 0.34) as usize;
    let mut phase = 0f32;
    let mut pcm = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / rate as f32;
        // Up, a wobble, and down: squeezed and let go.
        let pitch = if t < 0.11 { 620. + 700. * (t / 0.11) } else { 1320. - 520. * ((t - 0.11) / 0.23).min(1.) };
        phase += std::f32::consts::TAU * pitch * (1. + 0.03 * (t * 190.).sin()) / rate as f32;
        let tone = (3. * phase.sin()).tanh() * 0.7 + (2. * phase).sin() * 0.2;
        let envelope = (t / 0.012).min(1.) * (1. - (t / 0.34)).max(0.).powf(0.7) * if (0.13..0.16).contains(&t) { 0.55 } else { 1. };
        let sample = ((tone * envelope * 0.8).clamp(-1., 1.) * i16::MAX as f32) as i16;
        pcm.extend(sample.to_le_bytes());
    }
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend(b"RIFF");
    wav.extend((36 + pcm.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(rate.to_le_bytes());
    wav.extend((rate * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((pcm.len() as u32).to_le_bytes());
    wav.extend(pcm);
    wav
}

/// Every frame after the round: on, everything of this player is turned over (once, and kept so); off, put back.
pub fn dress(world: &mut World) {
    let on = on(world.get_resource::<crate::settings::Settings>());
    let Some(mut topsy) = world.remove_resource::<Topsy>() else { return };
    let (me, demo, playing) = world.get_resource::<Game>().map_or((0, true, false), |g| (g.me(), g.demo, g.playing()));
    if on && !demo && topsy.look.is_none() {
        if let Some(models) = world.get_resource::<crate::player_model::PlayerModels>() {
            let food = models.food();
            let metal = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
                base_color: Color::srgb(0.2, 0.21, 0.23),
                metallic: 0.85,
                perceptual_roughness: 0.36,
                ..default()
            });
            let squeak = world.resource_mut::<Assets<AudioSource>>().add(AudioSource { bytes: squeak().into() });
            topsy.look = Some(Look { food, metal, squeak });
        }
    }
    let look = topsy.look.clone().filter(|_| on && !demo);
    view(world, &mut topsy, look.as_ref());
    rounds(world, &mut topsy, look.as_ref(), me);
    bodies(world, &mut topsy, look.as_ref(), me);
    // A squeak the first time it shows.
    if let Some(look) = look.as_ref().filter(|_| playing && !topsy.laughed) {
        topsy.laughed = true;
        world.spawn((AudioPlayer::new(look.squeak.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.32))));
    }
    world.insert_resource(topsy);
}

/// First person: each gun's root holds a hotdog in metal hands instead of the gun.
fn view(world: &mut World, topsy: &mut Topsy, look: Option<&Look>) {
    let roots: Vec<Entity> = world.get_resource::<Game>().map_or_else(Vec::new, |g| g.viewmodels().iter().map(|(_, e)| *e).collect());
    // Put back what is no longer to be (off, or a gun's root that is gone).
    let undo: Vec<Entity> = topsy.views.keys().copied().filter(|root| look.is_none() || !roots.contains(root) || world.get_entity(*root).is_err()).collect();
    for root in undo {
        let v = topsy.views.remove(&root).unwrap();
        for (e, was) in v.hidden {
            if let Ok(mut e) = world.get_entity_mut(e) {
                e.insert(was);
            }
        }
        for (e, skin) in v.hands {
            if let Ok(mut e) = world.get_entity_mut(e) {
                e.insert(MeshMaterial3d(skin));
            }
        }
        if let Ok(e) = world.get_entity_mut(v.hotdog) {
            e.despawn();
        }
    }
    let Some(look) = look else { return };
    for root in roots {
        if topsy.views.contains_key(&root) || world.get_entity(root).is_err() {
            continue;
        }
        let children: Vec<Entity> = world.get::<Children>(root).map_or_else(Vec::new, |c| c.iter().collect());
        let (mut hidden, mut hands) = (vec![], vec![]);
        for child in children {
            if world.get::<crate::viewmodel::Hand>(child).is_some() {
                if let Some(skin) = world.get::<MeshMaterial3d<StandardMaterial>>(child).map(|m| m.0.clone()) {
                    world.entity_mut(child).insert(MeshMaterial3d(look.metal.clone()));
                    hands.push((child, skin));
                }
            } else {
                let was = world.get::<Visibility>(child).copied().unwrap_or_default();
                world.entity_mut(child).insert(Visibility::Hidden);
                hidden.push((child, was));
            }
        }
        // Lying along the aim, its tip the muzzle: the sausage's foot behind the grip, the mustard on top, the bun under.
        let place = Transform { translation: HELD_AT, rotation: Quat::from_rotation_z(PI) * Quat::from_rotation_x(-PI / 2.), scale: Vec3::splat(HELD) }
            * Transform::from_xyz(0., -0.023, 0.);
        let hotdog = hotdog(world, &look.food, Some(root), place);
        world.entity_mut(hotdog).insert(Name::new("First-person hotdog"));
        topsy.views.insert(root, View { hidden, hands, hotdog });
    }
}

/// This player's rounds: each drawn as a tiny hotdog tumbling along where the round is.
fn rounds(world: &mut World, topsy: &mut Topsy, look: Option<&Look>, me: u32) {
    let now = world.get_resource::<Time<Real>>().map_or(0., |t| t.elapsed_secs());
    let demo = world.get_resource::<Game>().is_none_or(|g| g.demo);
    let flying: Vec<(u32, Entity, Vec3)> = world
        .get_resource::<Game>()
        .map_or_else(Vec::new, |g| g.blobs().iter().filter(|(_, b)| dressed(b.owner, me, look.is_some(), demo)).map(|(id, b)| (*id, b.root, b.velocity)).collect());
    let gone: Vec<u32> = topsy.rounds.keys().copied().filter(|id| !flying.iter().any(|(f, ..)| f == id)).collect();
    for id in gone {
        if let Some(e) = topsy.rounds.remove(&id) {
            if let Ok(e) = world.get_entity_mut(e) {
                e.despawn();
            }
        }
    }
    // Off: the rounds of this player's still in the air are drawn as they were.
    let Some(look) = look else {
        let mine: Vec<Entity> = world.get_resource::<Game>().map_or_else(Vec::new, |g| g.blobs().values().filter(|b| b.owner == me).map(|b| b.root).collect());
        for root in mine {
            if world.get::<Visibility>(root).is_some_and(|v| *v == Visibility::Hidden) {
                world.entity_mut(root).insert(Visibility::Inherited);
            }
        }
        return;
    };
    for (id, root, velocity) in flying {
        let Some(at) = world.get::<Transform>(root).map(|t| t.translation) else { continue };
        if world.get::<Visibility>(root).is_some_and(|v| *v != Visibility::Hidden) {
            world.entity_mut(root).insert(Visibility::Hidden);
        }
        let e = match topsy.rounds.get(&id).copied().filter(|e| world.get_entity(*e).is_ok()) {
            Some(e) => e,
            None => {
                let e = hotdog(world, &look.food, None, Transform::default());
                topsy.rounds.insert(id, e);
                e
            }
        };
        // Along the way it flies, tumbling end over end about its side.
        let along = Quat::from_rotation_arc(Vec3::Y, velocity.normalize_or(Vec3::NEG_Z));
        let tumble = Quat::from_rotation_x(now * TUMBLE + id as f32 * 0.7);
        let turn = along * tumble;
        if let Some(mut t) = world.get_mut::<Transform>(e) {
            *t = Transform { translation: at - turn * Vec3::Y * MIDDLE * ROUND, rotation: turn, scale: Vec3::splat(ROUND) };
        }
    }
}

/// The bounds (in its own frame) of what hangs from `root`: its children's meshes.
fn bounds(world: &World, root: Entity) -> Option<(Vec3, Vec3)> {
    let meshes = world.get_resource::<Assets<Mesh>>()?;
    let mut out: Option<(Vec3, Vec3)> = None;
    for child in world.get::<Children>(root)?.iter() {
        let (Some(handle), place) = (world.get::<Mesh3d>(child), world.get::<Transform>(child).copied().unwrap_or_default()) else { continue };
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) = meshes.get(&handle.0).and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION)) else { continue };
        for p in points {
            let p = place.transform_point(Vec3::from_array(*p));
            out = Some(out.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
        }
    }
    out
}

/// This player's own bodies: each a walking gun while it is on.
fn bodies(world: &mut World, topsy: &mut Topsy, look: Option<&Look>, me: u32) {
    let weapon = world.get_resource::<Game>().map_or(1, |g| if g.kung_fu() { 1 } else { (g.weapon as usize).clamp(1, 7) });
    let mine: Vec<(Entity, Entity)> = if me == 0 || look.is_none() {
        vec![]
    } else {
        world.query_filtered::<(Entity, &Gait, &Rig), With<PlayerBody>>().iter(world).filter(|(_, gait, _)| gait.id() == me).map(|(e, _, rig)| (e, rig.joints().torso)).collect()
    };
    // Put back the bodies no longer to be guns (off, another's now, gone), and a gun that is the wrong gun.
    let undo: Vec<Entity> = topsy.bodies.iter().filter(|(root, b)| !mine.iter().any(|(e, _)| e == *root) || b.weapon != weapon || world.get_entity(**root).is_err()).map(|(root, _)| *root).collect();
    for root in undo {
        let b = topsy.bodies.remove(&root).unwrap();
        for (e, was) in b.hidden {
            if let Ok(mut e) = world.get_entity_mut(e) {
                e.insert(was);
            }
        }
        if let Ok(e) = world.get_entity_mut(b.gun) {
            e.despawn();
        }
    }
    let Some(look) = look else { return };
    let Some(kit) = world.get_resource::<crate::player_model::PlayerModels>().map(|m| m.weapons.clone()) else { return };
    for (root, torso) in mine {
        if let Some(b) = topsy.bodies.get(&root) {
            // (Kept hidden: a mouth opening to speak would show again.)
            for (e, _) in &b.hidden {
                if world.get::<Visibility>(*e).is_some_and(|v| *v != Visibility::Hidden) {
                    world.entity_mut(*e).insert(Visibility::Hidden);
                }
            }
            continue;
        }
        let children: Vec<Entity> = world.get::<Children>(torso).map_or_else(Vec::new, |c| c.iter().collect());
        let mut hidden = vec![];
        for child in children {
            let was = world.get::<Visibility>(child).copied().unwrap_or_default();
            world.entity_mut(child).insert(Visibility::Hidden);
            hidden.push((child, was));
        }
        // The gun stood on its butt, muzzle up, its side to the front: as tall as the hotdog was, on its legs. (It and its
        // eyes hang from one root, which goes when it is undone.)
        let dress = world.spawn((Name::new("Walking gun"), Transform::default(), Visibility::Inherited)).id();
        world.entity_mut(torso).add_child(dress);
        let gun = kit.attach(world, dress, weapon, Transform::default(), Visibility::Inherited);
        let turn = Quat::from_rotation_y(PI / 2.) * Quat::from_rotation_x(PI / 2.);
        let (lo, hi) = bounds(world, gun).unwrap_or((Vec3::splat(-0.02), Vec3::splat(0.02)));
        let corners: Vec<Vec3> = (0..8).map(|k| turn * Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z })).collect();
        let (clo, chi) = corners.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), c| (a.min(*c), b.max(*c)));
        let size = STANDS.1 / (chi.y - clo.y).max(1e-3);
        let middle = (clo + chi) / 2. * size;
        let place = Transform { translation: Vec3::new(-middle.x, STANDS.0 - clo.y * size, -middle.z), rotation: turn, scale: Vec3::splat(size) };
        world.entity_mut(gun).insert(place);
        // Its eyes on its front, where the hotdog's were.
        let front = -((chi.z - clo.z) / 2. * size);
        let (across, up) = crate::player_model::EYES;
        let eyes = [
            (look.food.white.clone(), Vec3::new(0., 0., 0.001), Vec3::new(0.0085, 0.011, 0.004)),
            (look.food.dark.clone(), Vec3::new(0., 0., -0.003), Vec3::new(0.0042, 0.006, 0.0022)),
            (look.food.white.clone(), Vec3::new(-0.0012, 0.0024, -0.0048), Vec3::splat(0.0018)),
        ];
        for side in [-1.2f32, 1.2] {
            for (material, offset, scale) in &eyes {
                let e = world.spawn((Mesh3d(look.food.eye.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(Vec3::new(side * across + offset.x, up + offset.y, front + offset.z)).with_scale(*scale))).id();
                // (Beside the gun, not on it: the gun is scaled.)
                world.entity_mut(dress).add_child(e);
            }
        }
        topsy.bodies.insert(root, Body { gun: dress, weapon, hidden });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn it_is_on_exactly_when_the_look_is_turned_over_and_dresses_only_this_players_rounds() {
        assert!(!on(None));
        assert!(!on(Some(&Settings::default())));
        assert!(on(Some(&Settings { invert_y: true, ..default() })));
        assert!(!on(Some(&Settings { invert_y: false, sensitivity: 2., ..default() })));
        assert!(dressed(1, 1, true, false));
        assert!(!dressed(2, 1, true, false), "another's round");
        assert!(!dressed(1, 1, false, false), "off");
        assert!(!dressed(1, 1, true, true), "watching");
        assert!(!dressed(0, 0, true, false));
        // The squeak is a WAV a sound can be made of.
        let wav = squeak();
        assert!(wav.starts_with(b"RIFF") && &wav[8..12] == b"WAVE" && wav.len() > 10_000);
    }

    /// Single player, the pointer captured (as a click would), the look's Y turned over or not, a few frames in.
    fn game(invert: bool) -> App {
        let mut app = crate::arena::test_app("");
        app.insert_resource(Settings { invert_y: invert, ..default() }).init_resource::<Topsy>().add_systems(Update, (crate::arena::tick, dress).chain());
        let window = app.world_mut().query_filtered::<Entity, With<Window>>().iter(app.world()).next().unwrap();
        app.world_mut().entity_mut(window).insert(bevy::window::CursorOptions { grab_mode: bevy::window::CursorGrabMode::Locked, ..default() });
        for _ in 0..3 {
            crate::arena::test_frame(&mut app, 0.05);
        }
        app
    }
    fn named(app: &mut App, name: &str) -> usize {
        app.world_mut().query::<&Name>().iter(app.world()).filter(|n| n.as_str() == name).count()
    }

    /// On, each first-person gun is put away for a hotdog held in gun-metal hands; off again, the gun is back.
    #[test]
    fn turned_over_the_gun_in_first_person_is_a_hotdog_in_metal_hands_and_back_again() {
        let mut app = game(false);
        assert_eq!(named(&mut app, "First-person hotdog"), 0, "not while the look is the right way up");
        app.world_mut().resource_mut::<Settings>().invert_y = true;
        crate::arena::test_frame(&mut app, 0.05);
        let roots: Vec<Entity> = app.world().resource::<Game>().viewmodels().iter().map(|(_, e)| *e).collect();
        assert!(!roots.is_empty());
        assert_eq!(named(&mut app, "First-person hotdog"), roots.len());
        let metal = app.world().resource::<Topsy>().look.as_ref().unwrap().metal.clone();
        for root in &roots {
            for child in app.world().get::<Children>(*root).unwrap().iter() {
                let w = app.world();
                if w.get::<crate::viewmodel::Hand>(child).is_some() {
                    assert_eq!(w.get::<MeshMaterial3d<StandardMaterial>>(child).unwrap().0, metal, "gun-metal hands");
                } else if w.get::<Name>(child).is_none_or(|n| n.as_str() != "First-person hotdog") {
                    assert_eq!(w.get::<Visibility>(child), Some(&Visibility::Hidden), "the gun is put away");
                }
            }
        }
        app.world_mut().resource_mut::<Settings>().invert_y = false;
        crate::arena::test_frame(&mut app, 0.05);
        assert_eq!(named(&mut app, "First-person hotdog"), 0);
        for root in &roots {
            for child in app.world().get::<Children>(*root).unwrap().iter() {
                assert_ne!(app.world().get::<Visibility>(child), Some(&Visibility::Hidden), "the gun is back");
                assert_ne!(app.world().get::<MeshMaterial3d<StandardMaterial>>(child).map(|m| m.0.clone()), Some(metal.clone()));
            }
        }
    }

    /// Only this player's rounds are drawn as hotdogs (its own condiment hidden), every one of them; and the round
    /// itself, what it hits and the harm it does are the same with it on as off.
    #[test]
    fn only_this_players_rounds_are_hotdogs_and_the_round_is_the_same_either_way() {
        let play = |invert: bool| {
            let mut app = game(invert);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            let mut mine = 0;
            let mut log = vec![];
            for _ in 0..30 {
                crate::arena::test_frame(&mut app, 0.05);
                let (g, topsy) = (app.world().resource::<Game>(), app.world().resource::<Topsy>());
                for (id, blob) in g.blobs() {
                    assert_eq!(topsy.rounds.contains_key(id), invert && blob.owner == g.me(), "round {id} of {}", blob.owner);
                    if invert && blob.owner == g.me() {
                        mine += 1;
                        assert_eq!(app.world().get::<Visibility>(blob.root), Some(&Visibility::Hidden));
                    }
                }
                let f = &g.feast;
                log.push((
                    f.elapsed,
                    f.players.iter().map(|p| (p.id, p.hp, p.alive, p.p, p.last_damage, p.ammo)).collect::<Vec<_>>(),
                    f.projectiles.iter().map(|q| (q.id, q.owner, q.p, q.v)).collect::<Vec<_>>(),
                    f.splats.len(),
                ));
            }
            (mine, log)
        };
        let (dressed, on) = play(true);
        let (_, off) = play(false);
        assert!(dressed > 0, "this player fired");
        assert!(on == off, "the round went differently with it on");
    }

    /// Nothing on any screen names it: not the menu, the hints, the head-up display or the README.
    #[test]
    fn no_word_of_it_on_any_screen() {
        let words = ["topsy", "turned over too", "shoots hotdogs", "walking gun", "easter", " egg", "hotdog launcher"];
        for (file, text) in [
            ("menu.rs", include_str!("menu.rs")),
            ("hints.rs", include_str!("hints.rs")),
            ("hud.rs", include_str!("hud.rs")),
            ("settings.rs", include_str!("settings.rs")),
            ("README.md", include_str!("../README.md")),
            ("index.html", include_str!("../index.html")),
        ] {
            let text = text.to_lowercase();
            for word in words {
                assert!(!text.contains(word), "{file} says {word:?}");
            }
        }
    }
}

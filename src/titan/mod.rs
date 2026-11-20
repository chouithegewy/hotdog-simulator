// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! **The titan on screen** (mode `titan`, kitchen_sim titan.rs): the giant hotdog's model, every move animated, its
//! player's cameras, and what everybody sees, hears and feels of it.
//!
//! * **The body** (body.rs, shape.rs): an original colossal hotdog made in code, twelve hotdogs tall, built to be looked
//!   up at: a scorched, split, grill-scarred casing, a grin far too wide full of big flat teeth, small staring eyes set
//!   high, bun plates for armour, long thick arms with enormous hands, and the weak spot at the back of its head (a pale
//!   veined patch with a cocktail pick in it) that pulses when it is exposed. A cheaper version at Low and Lowest
//!   (`body::Lod`).
//! * **The animation** (pose.rs, anims.rs, play.rs): poses and keyed moves in the manner of Kung Fu's, each stretched
//!   to the simulation's own timing (`titan::TABLE`), so every tell is on screen for exactly the startup the simulation
//!   gives the small hotdogs to react; the walk, the crouched knuckle-walk and the charge's run follow the ground the
//!   body covers. The head looks where the simulation says (the weak spot and the face are where its hit zones are).
//! * **The cameras** (camera.rs): the titan's player sees from its eyes, 1.7 m up, where the small hotdogs are specks
//!   (each marked), its own hands and feet in view doing each move, its turns a little heavy; the view key puts the
//!   camera behind and above it. The small hotdogs keep their own view; every camera near a footfall, a smash or a
//!   landing shakes (the nearer the harder), and none is ever left inside the giant.
//! * **Effects and sounds** (fx.rs, sound.rs): steam (more as it is hurt), dust at its feet, the smash's shockwave drawn
//!   to the simulation's own radius, cracked tiles that fade, debris and speed lines, the weak spot's flash, slow motion
//!   on the shot that kills it, and its synthesised sounds (tools/build_titan_sounds.py). Pooled, and budgeted by the
//!   graphics level (`fx::Budget`).
//!
//! The client's copy of the titan is `Game::titan` (offline the round's own `Feast::titan_view` every tick, online the
//! server's `{type:'titan'}` message: [`hear`]). Nothing here goes back into the simulation.
use crate::arena::Game;
use bevy::prelude::*;
use kitchen_sim::titan::{self as sim, Move, View};
use std::collections::HashMap;

pub mod anims;
pub mod body;
pub mod camera;
pub mod fx;
pub mod play;
pub mod pose;
pub mod predict;
pub mod rig;
pub mod shape;
pub mod sound;
#[cfg(test)]
mod tests;

/// Online: a `{type:'titan', ...View}` message is the titan as the server has it now (`Game::titan`). Returns whether
/// it was one.
pub fn hear(g: &mut Game, message: &serde_json::Value) -> bool {
    if message["type"] != "titan" {
        return false;
    }
    match serde_json::from_value::<View>(message.clone()) {
        Ok(view) => {
            g.titan = Some(view);
            true
        }
        Err(_) => false,
    }
}
/// The titan's hotdog this round, if there is one.
pub fn titan_id(g: &Game) -> Option<u32> {
    g.titan.as_ref().map(|v| v.titan).filter(|id| *id != 0)
}
/// Whether hotdog `id` is drawn as the titan (its own hotdog body is not drawn, nor ragdolled).
pub fn drawn_as_titan(g: &Game, id: u32) -> bool {
    titan_id(g) == Some(id)
}
/// Online, the titan's body in the client's world of bodies (online.rs): a giant's (`titan::ghost`: its footprint, its
/// stature in its stance, its mass), where the newest frame has it. None unless `id` is the titan.
pub fn ghost_of(g: &Game, id: u32) -> Option<kitchen_sim::actor::Actor> {
    if titan_id(g) != Some(id) {
        return None;
    }
    let p = g.online.latest.iter().find(|p| p.id == id).or_else(|| g.roster.iter().find(|p| p.id == id))?;
    let mv = g.titan.as_ref().map_or(Move::Idle, |v| v.mv());
    let mut body = sim::ghost(id, p.p, sim::stance_of(p, mv));
    (body.grounded, body.v[1]) = (p.grounded, p.v[1] as f64);
    Some(body)
}
/// Whether this player is the titan, alive and playing (its cameras are the titan's).
pub fn playing_titan(g: &Game, demo: bool) -> bool {
    !demo && g.playing() && titan_id(g) == Some(g.me()) && g.roster.iter().find(|p| p.id == g.me()).is_some_and(|p| p.alive) && !crate::cards::at_table(g)
}

/// Every titan drawn, and what its animation and effects keep from frame to frame.
#[derive(Resource, Default)]
pub struct Titans {
    pub kit: Option<rig::Kit>,
    pub roots: HashMap<u32, Entity>,
    animators: HashMap<u32, play::Animator>,
    /// Where each was last drawn (feet, yaw), for how far it went.
    seen: HashMap<u32, (Vec3, f32)>,
    /// The newest event seen.
    pub last_event: u32,
    /// The titan drawn this frame: its id, its frame, its root's place (for the cameras and the effects).
    pub drawn: Option<(u32, play::Frame, Transform)>,
    time: Option<f32>,
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<rig::TitanMaterial>::default())
            .init_resource::<Titans>()
            .init_resource::<camera::View>()
            .init_resource::<fx::Fx>()
            .add_systems(Update, camera::toggle.before(crate::arena::tick))
            .add_systems(
                Update,
                (animate, camera::place, fx::update, sound::update)
                    .chain()
                    .after(crate::arena::tick)
                    .after(crate::network::send)
                    .after(crate::deathcam::watch)
                    .before(crate::slowmo::present),
            )
            .add_systems(First, camera::restore)
            .add_systems(PostUpdate, camera::settle.after(crate::lobby::update).before(bevy::transform::TransformSystems::Propagate));
    }
}

/// Where the titan `p` is drawn and which way it faces: this player's own as the client has it, the others as the round
/// (or the server's timeline) has them.
fn placed(g: &Game, p: &kitchen_sim::feast::Player, demo: bool) -> (Vec3, f32) {
    if p.id != g.me() || demo || !p.alive {
        return (Vec3::from_array(p.p), p.yaw);
    }
    // Online its own giant is predicted ahead of the server (predict.rs): its turn too.
    if g.network {
        if let Some(f) = predict::Foresight::of(g) {
            let pending: Vec<kitchen_sim::net::Command> = g.online.predictor.pending.iter().map(|(c, _)| *c).collect();
            return (g.position, f.yaw_after(&pending, g.online.dt(), !p.grounded));
        }
    }
    (g.position, p.yaw)
}

/// Once a frame, after the round and the network have said where everybody is: the titan's model made (or put away),
/// placed and posed, and the frame kept for the cameras and the effects.
pub fn animate(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    // (A kill's slow motion slows the titan too: slowmo.rs.)
    let pace = world.get_resource::<crate::slowmo::SlowMo>().map_or(1., |s| s.scale(now) * s.replay.unwrap_or(1.));
    let demo = world.get_resource::<crate::runtime::Options>().is_some_and(|o| o.demo);
    let mut titans = world.remove_resource::<Titans>().unwrap_or_default();
    let dt = titans.time.map_or(0., |was| (now - was).clamp(0., 0.1)) * pace;
    titans.time = Some(now);
    titans.drawn = None;
    let wanted = world.get_resource::<Game>().and_then(|g| {
        let view = g.titan.clone()?;
        let p = g.roster.iter().find(|p| p.id == view.titan)?.clone();
        let (at, yaw) = placed(g, &p, demo);
        Some((view, p, at, yaw, g.elapsed, g.me()))
    });
    // Titans that are no longer (another round, another mode) go.
    let keep = wanted.as_ref().map(|w| w.0.titan);
    for (id, root) in titans.roots.clone() {
        if Some(id) != keep {
            if let Ok(e) = world.get_entity_mut(root) {
                e.despawn();
            }
            titans.roots.remove(&id);
            titans.animators.remove(&id);
            titans.seen.remove(&id);
        }
    }
    let Some((view, p, at, yaw, elapsed, me)) = wanted else {
        world.insert_resource(titans);
        return;
    };
    // The meshes, at the graphics level's fineness.
    let lod = body::Lod::of(world.get_resource::<crate::quality::Detail>().map_or(crate::quality::Level::High, |d| d.level));
    match titans.kit.as_mut() {
        Some(kit) => kit.remake(&mut world.resource_mut::<Assets<Mesh>>(), lod),
        None => {
            let kit = rig::Kit::make(world, lod);
            titans.kit = Some(kit);
        }
    }
    let kit = titans.kit.clone().unwrap();
    let id = view.titan;
    let root = match titans.roots.get(&id).copied().filter(|e| world.get_entity(*e).is_ok()) {
        Some(e) => e,
        None => {
            let e = rig::spawn(world, &kit, id, at, yaw);
            titans.roots.insert(id, e);
            titans.animators.insert(id, play::Animator::default());
            titans.seen.remove(&id);
            e
        }
    };
    let root_place = Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(rig::scale()));
    if let Some(mut tr) = world.get_mut::<Transform>(root) {
        *tr = root_place;
    }
    // How far it went since the last frame, in its own frame (right, forward).
    let (was, _) = titans.seen.get(&id).copied().unwrap_or((at, yaw));
    let step = Quat::from_rotation_y(-yaw) * (at - was);
    let moved = if step.length() > 1. { Vec2::ZERO } else { Vec2::new(step.x, -step.z) };
    titans.seen.insert(id, (at, yaw));
    // Online, the titan's own player keeps the walk its giant has while it stands free: a jump takes off with it.
    if me == id && dt > 1e-4 && matches!(view.mv(), Move::Idle | Move::Walk) {
        let walk = Vec2::new(at.x - was.x, at.z - was.z) / dt * pace.max(1e-3).recip().min(1.);
        let walk = walk.clamp_length_max(sim::WALK);
        if let Some(mut g) = world.get_resource_mut::<Game>() {
            if g.network {
                g.online.titan_run_up = walk;
            }
        }
    }
    let mv = if p.alive { view.mv() } else { Move::Down };
    let t = view.t(elapsed);
    let timing = anims::timing(mv, view.len);
    let stance = play::stance_of(mv, view.crouch);
    // The eyes stare at whoever is looking at it (this screen's camera), or ahead.
    let camera = world.get_resource::<Game>().and_then(|g| g.camera()).and_then(|c| world.get::<Transform>(c)).map(|t| t.translation);
    let stare = camera.filter(|_| me != id).map(|c| root_place.compute_affine().inverse().transform_point3(c));
    // The walk is drawn from the round's stride: offline the round's own each frame, online (or replayed) a view's.
    let exact = world.get_resource::<Game>().is_some_and(|g| !g.network && g.feast.titan.is_some());
    let stride = Some(play::Stride { phase: view.stride, drive: view.drive, exact });
    let drive = play::Drive { mv, t, timing, crouch: view.crouch, airborne: !p.grounded, moved, nod: play::sim_nod(stance, p.pitch), stare, stride };
    // A hit on its weak spot makes it flinch (the newest events; fx.rs shows and sounds them).
    // (A new round numbers its events from one again.)
    if view.events.iter().map(|e| e.id).max().is_some_and(|newest| newest < titans.last_event) {
        titans.last_event = 0;
    }
    let fresh: Vec<sim::Event> = view.events.iter().filter(|e| e.id > titans.last_event).cloned().collect();
    let animator = titans.animators.entry(id).or_default();
    if fresh.iter().any(|e| e.kind == Some(sim::EventKind::WeakHit)) {
        animator.flinch();
    }
    let frame = animator.update(&drive, dt);
    rig::pose(world, root, &frame.skeleton);
    rig::light(world, root, kit.lod == body::Lod::Full);
    // Its own player, in first person, sees only its arms and legs.
    let own = me == id && camera::first_person(world);
    rig::own_view(world, root, own);
    titans.drawn = Some((id, frame, root_place));
    world.insert_resource(titans);
    fx::events(world, &view, &fresh, me, now, elapsed);
    if let Some(newest) = fresh.iter().map(|e| e.id).max() {
        world.resource_mut::<Titans>().last_event = newest;
    }
}

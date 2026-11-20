// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Kung Fu in first person: this player's own forearms, hands and (when they come up) shins and feet, doing the same
//! move everybody else sees its body do. They hang from the game's camera and are drawn in the view model's pass
//! (viewmodel.rs), as the guns are, so they never sink into a wall or an opponent.
//!
//! The limbs are where the move puts them relative to the eye (the body's own skeleton, `Solved::skeleton`), turned
//! with the view as a gun is (they stay in the picture when the player looks up or down) and with half the chest's
//! twist and lean, so a cross swings across the view. The camera itself leans a quarter of the way with the chest, dips
//! with the hips, and is jolted when a blow lands, when one is taken and when the ground shakes. Nothing of the body is
//! ever drawn from inside: only the limbs, and none of them nearer the eye than `CLEAR`.
use super::*;
use crate::viewmodel::ViewModel;
use bevy::camera::visibility::RenderLayers;

/// No part of a limb comes nearer the eye than this.
pub const CLEAR: f32 = 0.014;
/// How much of the chest's turn the limbs follow, and how much the camera does.
const ARMS_TURN: f32 = 0.5;
const CAMERA_TURN: f32 = 0.22;
/// How much of the eye's own movement (a lunge, a crouching sweep) the camera follows.
const CAMERA_MOVE: f32 = 0.6;

/// The first-person limbs, made the first time they are wanted.
#[derive(Resource)]
pub struct Limbs {
    camera: Entity,
    root: Entity,
    /// Each arm's upper arm and forearm, its wrist and the shapes on it; each leg's shin and foot.
    arms: [[Entity; 2]; 2],
    wrists: [Entity; 2],
    shapes: [[Entity; 8]; 2],
    shins: [Entity; 2],
    feet: [Entity; 2],
    /// The whooshes of this player's own blows, in the camera's frame, by limb: samples and the ribbon.
    trails: Vec<(Limb, Vec<(Vec3, Vec3, f32)>)>,
    ribbon: Option<(Entity, Handle<Mesh>)>,
}
/// A segment's place: a capsule `LIMB` thick from `a` to `b` (the rig's limb mesh is 0.018 long for a 0.016 bone).
pub fn segment(a: Vec3, b: Vec3) -> Transform {
    let d = b - a;
    Transform::from_translation((a + b) * 0.5).with_rotation(Quat::from_rotation_arc(Vec3::NEG_Y, d.normalize_or(Vec3::NEG_Y))).with_scale(v(1., d.length() / BONE, 1.))
}
/// `a` to `b` moved, if need be, so that no point of it is nearer the eye (the origin) than `CLEAR`.
pub fn clear(a: Vec3, b: Vec3) -> (Vec3, Vec3) {
    let d = b - a;
    let t = (-a.dot(d) / d.length_squared().max(1e-9)).clamp(0., 1.);
    let near = a + d * t;
    let gap = near.length();
    if gap >= CLEAR {
        return (a, b);
    }
    let away = near.normalize_or(Vec3::NEG_Z) * (CLEAR - gap);
    (a + away, b + away)
}
/// Where the first-person limbs are, in the camera's frame, for `s`, and the camera's own nudge (a turn and a move in
/// the body's frame) relative to standing in `stance`.
pub struct Placed {
    pub upper: [(Vec3, Vec3); 2],
    pub fore: [(Vec3, Vec3); 2],
    pub hand: [Quat; 2],
    pub shin: [(Vec3, Vec3); 2],
    pub foot: [Quat; 2],
    /// Whether each foot is up and ahead, in view.
    pub kicking: [bool; 2],
    pub turn: Quat,
    pub shift: Vec3,
}
pub fn place(s: &Solved, stance: &Solved) -> Placed {
    let k = s.skeleton();
    let rest = stance.skeleton();
    let chest = k.chest * rest.chest.inverse();
    let frame = Quat::IDENTITY.slerp(chest, ARMS_TURN);
    let eye = k.eye;
    let into = |p: Vec3| frame.inverse() * (p - eye);
    let mut p = Placed {
        upper: [(Vec3::ZERO, Vec3::ZERO); 2],
        fore: [(Vec3::ZERO, Vec3::ZERO); 2],
        hand: [Quat::IDENTITY; 2],
        shin: [(Vec3::ZERO, Vec3::ZERO); 2],
        foot: [Quat::IDENTITY; 2],
        kicking: [false; 2],
        turn: Quat::IDENTITY.slerp(chest, CAMERA_TURN),
        shift: (k.eye - rest.eye) * CAMERA_MOVE,
    };
    for i in [L, R] {
        let (shoulder, elbow, wrist) = (into(k.shoulder[i]), into(k.elbow[i]), into(k.wrist[i]));
        let (elbow, wrist) = clear(elbow, wrist);
        p.upper[i] = clear(shoulder, elbow);
        p.fore[i] = (elbow, wrist);
        p.hand[i] = frame.inverse() * k.hand[i];
        let (knee, ankle) = clear(into(k.knee[i]), into(k.ankle[i]));
        p.shin[i] = (knee, ankle);
        p.foot[i] = frame.inverse() * k.foot[i];
        p.kicking[i] = k.ankle[i].y > 0.022 && k.ankle[i].z < -0.015;
    }
    p
}

fn build(world: &mut World, camera: Entity) -> Option<Limbs> {
    let models = world.get_resource::<crate::player_model::PlayerModels>()?.clone();
    let me = world.get_resource::<crate::arena::Game>().map_or(1, |g| g.me());
    let skin = models.skin_material.clone();
    let colour = models.livery(me);
    let capsule = world.resource_mut::<Assets<Mesh>>().add(Capsule3d::new(LIMB * rig::THICK, 0.010).mesh().longitudes(16).latitudes(10));
    let layer = RenderLayers::layer(crate::viewmodel::VIEW_LAYER);
    let root = world.spawn((Name::new("First-person kung fu limbs"), Transform::default(), Visibility::Hidden, layer.clone())).id();
    world.entity_mut(camera).add_child(root);
    let part = |world: &mut World, parent: Entity, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>| {
        let e = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Inherited, layer.clone())).id();
        world.entity_mut(parent).add_child(e);
        e
    };
    let mut arms = [[Entity::PLACEHOLDER; 2]; 2];
    let mut wrists = [Entity::PLACEHOLDER; 2];
    let mut shapes = [[Entity::PLACEHOLDER; 8]; 2];
    let mut shins = [Entity::PLACEHOLDER; 2];
    let mut feet = [Entity::PLACEHOLDER; 2];
    for i in [L, R] {
        arms[i] = [part(world, root, &capsule, &skin), part(world, root, &capsule, &skin)];
        wrists[i] = world.spawn((Transform::default(), Visibility::Inherited, layer.clone())).id();
        world.entity_mut(root).add_child(wrists[i]);
        for (k, mesh) in models.kungfu.hands[i].iter().enumerate() {
            shapes[i][k] = part(world, wrists[i], mesh, &skin);
        }
        shins[i] = part(world, root, &capsule, &colour);
        feet[i] = part(world, root, &models.kungfu.feet[i], &colour);
    }
    Some(Limbs { camera, root, arms, wrists, shapes, shins, feet, trails: vec![], ribbon: None })
}
fn set(world: &mut World, e: Entity, tr: Transform, show: bool) {
    if let Some(mut t) = world.get_mut::<Transform>(e) {
        *t = tr;
    }
    let want = if show { Visibility::Inherited } else { Visibility::Hidden };
    if world.get::<Visibility>(e).is_some_and(|v| *v != want) {
        world.entity_mut(e).insert(want);
    }
}

/// Once a frame after `rig::animate`: in Kung Fu, hides the guns, shows this player's limbs doing its move, and nudges
/// and jolts the camera; outside it, hides the limbs.
pub fn update(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let mine = world.get_resource::<rig::Fighters>().and_then(|f| f.mine);
    let (camera, playing, me, style) = match world.get_resource::<crate::arena::Game>() {
        Some(g) => {
            let alive = g.roster.iter().find(|p| p.id == g.me()).is_some_and(|p| p.alive);
            let style = sim::roster(world).and_then(|r| r.fighters.iter().find(|f| f.id == r.me).map(|f| f.style));
            // (`g.demo`: the demo's, and a replay's viewer's while it is not through a player's eyes: replay/.)
            (g.camera(), alive && g.playing() && !g.demo, g.me(), style)
        }
        None => (None, false, 0, None),
    };
    let kungfu = style.is_some();
    // No guns in Kung Fu, whatever the hotdog's weapon number says (its style).
    if kungfu {
        let models: Vec<Entity> = world.query_filtered::<Entity, With<ViewModel>>().iter(world).collect();
        for e in models {
            if world.get::<Visibility>(e).is_some_and(|v| *v != Visibility::Hidden) {
                world.entity_mut(e).insert(Visibility::Hidden);
            }
        }
    }
    // KF6: over the shoulder (fight.rs) the whole body is seen instead, and the camera is fight.rs's.
    let shown = kungfu && playing && mine.is_some() && camera.is_some() && !crate::fight::third_person(world);
    let limbs = world.remove_resource::<Limbs>();
    let limbs = match (limbs, camera) {
        (Some(l), Some(c)) if l.camera == c && world.get_entity(l.root).is_ok() => Some(l),
        (_, Some(c)) if shown => build(world, c),
        (l, _) => l.filter(|l| world.get_entity(l.root).is_ok()),
    };
    let Some(mut limbs) = limbs else {
        return;
    };
    set(world, limbs.root, Transform::default(), shown);
    if let (true, Some(frame), Some(camera), Some(style)) = (shown, mine, camera, style) {
        let stance = solve(&library().set(style).stance);
        let p = place(&frame.solved, &stance);
        for i in [L, R] {
            let (a, b) = p.upper[i];
            set(world, limbs.arms[i][0], segment(a, b), true);
            let (a, b) = p.fore[i];
            set(world, limbs.arms[i][1], segment(a, b), true);
            set(world, limbs.wrists[i], Transform::from_translation(b).with_rotation(p.hand[i]), true);
            for (k, shape) in limbs.shapes[i].iter().enumerate() {
                set(world, *shape, Transform::default(), Hand::ALL[k] == frame.solved.hands[i]);
            }
            let (a, b) = p.shin[i];
            set(world, limbs.shins[i], segment(a, b), p.kicking[i]);
            set(world, limbs.feet[i], Transform::from_translation(b).with_rotation(p.foot[i]), p.kicking[i]);
        }
        // The camera: a little of the chest's lean and twist, the eye's dip, and the jolts.
        let fx = world.resource::<fx::Fx>();
        let jolt = |hit: Option<(f32, f32)>, scale: f32| {
            hit.map_or(0., |(at, strength)| {
                let tau = now - at;
                if (0. ..0.6).contains(&tau) { strength * scale * (tau / 0.025).min(1.) * (-tau / 0.08).exp() } else { 0. }
            })
        };
        let kick = jolt(fx.kicked, 1.) + jolt(fx.hurt, 1.6) + jolt(fx.shaken, 1.2);
        let wobble = Vec2::new((now * 89.).sin(), (now * 117. + 1.).sin()) * kick * 0.012;
        if let Some(mut tr) = world.get_mut::<Transform>(camera) {
            let yaw = Quat::from_rotation_y(tr.rotation.to_euler(EulerRot::YXZ).0);
            tr.translation += yaw * p.shift;
            tr.rotation = tr.rotation * p.turn * Quat::from_rotation_x(-0.035 * kick) * Quat::from_rotation_y(wobble.x) * Quat::from_rotation_z(wobble.y);
        }
        // This player's own whooshes, in the camera's frame.
        let tips: Vec<(Limb, Vec3, Vec3)> = frame
            .trails
            .iter()
            .flatten()
            .map(|limb| match limb {
                Limb::Hand(i) => (*limb, p.fore[*i].1 + p.hand[*i] * Vec3::NEG_Y * 0.008, p.fore[*i].0.lerp(p.fore[*i].1, 0.55)),
                Limb::Foot(i) => (*limb, p.shin[*i].1 + p.foot[*i] * v(0., -0.001, 0.006), p.shin[*i].0.lerp(p.shin[*i].1, 0.55)),
                Limb::Elbow(i) => (*limb, p.fore[*i].0, p.upper[*i].0.lerp(p.fore[*i].0, 0.5)),
                Limb::Head => (*limb, Vec3::ZERO, Vec3::ZERO),
            })
            .filter(|(limb, ..)| *limb != Limb::Head)
            .collect();
        for (limb, a, b) in tips {
            let samples = match limbs.trails.iter_mut().position(|(l, _)| *l == limb) {
                Some(k) => &mut limbs.trails[k].1,
                None => {
                    limbs.trails.push((limb, vec![]));
                    &mut limbs.trails.last_mut().unwrap().1
                }
            };
            if samples.last().is_none_or(|(q, _, born)| now - born > 0.004 && q.distance(a) > 0.0008) {
                samples.push((a, b, now));
            }
        }
    } else {
        for i in [L, R] {
            set(world, limbs.shins[i], Transform::default(), false);
            set(world, limbs.feet[i], Transform::default(), false);
        }
    }
    for (_, samples) in limbs.trails.iter_mut() {
        samples.retain(|(_, _, born)| now - born < fx::LIFE);
    }
    limbs.trails.retain(|(_, s)| !s.is_empty());
    let longest = limbs.trails.iter().max_by_key(|(_, s)| s.len()).map(|(_, s)| s.clone()).unwrap_or_default();
    if limbs.ribbon.as_ref().is_none_or(|(e, _)| world.get_entity(*e).is_err()) && longest.len() >= 2 {
        limbs.ribbon = fx::ribbon_entity(world, Some(limbs.camera), Some(RenderLayers::layer(crate::viewmodel::VIEW_LAYER)));
    }
    if let Some((entity, mesh)) = limbs.ribbon.clone() {
        let show = shown && longest.len() >= 2;
        set(world, entity, Transform::default(), show);
        if show {
            let shape = fx::ribbon(&longest, now, [2., 1.8, 1.5]);
            fx::fill(&mut world.resource_mut::<Assets<Mesh>>(), &mesh, shape);
        }
    }
    let _ = me;
    world.insert_resource(limbs);
}

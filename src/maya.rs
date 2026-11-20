// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Maya's breakfast shift: a small, player-triggered scene with a real handoff. The fridge opens on Maya and her
//! husband; they greet each other while the hotdog in the tray watches; Space (or the page's button) calls Maya, who
//! reaches in, carries the tray to the counter and sets it down, then cuts the seal with a knife, and the round begins.
//! The people are the game's own (`human`), posed every frame from the scene's clock (`choreography`, `act`).
use bevy::prelude::*;

use crate::{box_mesh, fridge::FridgeScene, human, mat};

pub mod act;
pub mod choreography;
use choreography as motion;

#[derive(Component)]
pub(crate) struct HotdogCamera;

#[derive(Component)]
pub(crate) enum Part {
    Package,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    Opening,
    Greeting,
    Waiting,
    Reaching,
    Carrying,
    SettingDown,
    Complete,
}

impl Stage {
    fn id(self) -> u32 {
        match self {
            Self::Opening => 0,
            Self::Greeting => 6,
            Self::Waiting => 1,
            Self::Reaching => 2,
            Self::Carrying => 3,
            Self::SettingDown => 4,
            Self::Complete => 5,
        }
    }
    /// The stage that follows this one, `now` seconds into it (`door` seconds since the door began to open), when it is
    /// time, and with `called` when Maya has been called.
    pub(crate) fn next(self, now: f32, door: f32, called: bool) -> Option<Stage> {
        match self {
            Stage::Opening if door >= motion::GREETING_START => Some(Stage::Greeting),
            Stage::Greeting if now >= motion::GREETING_DURATION => Some(Stage::Waiting),
            Stage::Waiting if called => Some(Stage::Reaching),
            Stage::Reaching if now >= motion::REACH_DURATION => Some(Stage::Carrying),
            Stage::Carrying if now >= motion::CARRY_DURATION => Some(Stage::SettingDown),
            Stage::SettingDown if now >= motion::SET_DURATION => Some(Stage::Complete),
            _ => None,
        }
    }
    /// Where this stage is on the scene's clock, `now` seconds into it (`door` seconds since the door began to open).
    pub(crate) fn clip_time(self, now: f32, door: f32) -> f32 {
        match self {
            Stage::Opening => door.clamp(0.0, motion::GREETING_START),
            Stage::Greeting => motion::GREETING_START + now.min(motion::GREETING_DURATION),
            Stage::Waiting => motion::IDLE_START + now % motion::IDLE_DURATION,
            Stage::Reaching => motion::REACH_START + now.min(motion::REACH_DURATION),
            Stage::Carrying => motion::CARRY_START + now.min(motion::CARRY_DURATION),
            Stage::SettingDown => motion::SET_START + now.min(motion::SET_DURATION),
            Stage::Complete => motion::COMPLETE_START + now % motion::IDLE_DURATION,
        }
    }
}

#[derive(Resource)]
pub(crate) struct BreakfastShift {
    stage: Stage,
    entered: f64,
    now: f32,
    door_time: f32,
    steps: act::Steps,
}

impl BreakfastShift {
    pub(crate) fn finished(&self) -> bool {
        self.stage == Stage::Complete
    }
    pub(crate) fn phase_name(&self) -> &'static str {
        match self.stage {
            Stage::Opening => "Opening",
            Stage::Greeting => "Greeting",
            Stage::Waiting => "Waiting",
            Stage::Reaching => "Reaching",
            Stage::Carrying => "Carrying",
            Stage::SettingDown => "SettingDown",
            Stage::Complete => "Complete",
        }
    }
    fn set_stage(&mut self, stage: Stage, now: f64) {
        self.stage = stage;
        self.entered = now;
        self.now = 0.0;
        publish_stage(stage.id());
        info!("Maya breakfast shift: {stage:?}");
    }

    fn clip_time(&self) -> f32 {
        self.stage.clip_time(self.now, self.door_time)
    }
}

fn smooth(t: f32) -> f32 {
    motion::smooth(t)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function publish_stage(stage) {
  window.dispatchEvent(new CustomEvent('hotdog:stage', { detail: stage }));
}
export function take_action() {
  const action = window.hotdogAction || 0;
  window.hotdogAction = 0;
  return action;
}
")]
extern "C" {
    pub(crate) fn publish_stage(stage: u32);
    fn take_action() -> u32;
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn publish_stage(_: u32) {}
#[cfg(not(target_arch = "wasm32"))]
fn take_action() -> u32 {
    0
}

#[derive(Component)]
pub(crate) struct TaskLamp(f32);

/// Builds the scene's people (and the cast's meshes, if nobody has yet), the cutting board and plate on the back
/// counter, Maya's task lamps, and the scene's clock.
pub(crate) fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let cast = human::Cast::new(meshes, materials, images, true);
    let steps = act::Steps::plan();
    for (person, actor) in [human::Person::Maya, human::Person::Husband].into_iter().zip(act::act(0., &steps)) {
        human::spawn(commands, &cast, person, actor.place(1.));
    }
    commands.insert_resource(cast);
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(24, 16));
    let oval = |commands: &mut Commands, parent: Entity, material: &Handle<StandardMaterial>, at: Vec3, scale: Vec3| {
        let entity = commands.spawn((Mesh3d(sphere.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(at).with_scale(scale))).id();
        commands.entity(parent).add_child(entity);
    };
    let white = mat(materials, Color::srgb(0.93, 0.91, 0.82), 0.48);

    // The cutting board rests on the existing back counter, not in the aisle.
    let wood = mat(materials, Color::srgb(0.34, 0.19, 0.095), 0.85);
    box_mesh(
        commands,
        meshes,
        wood,
        Vec3::new(0.49, 0.027, 0.37),
        Vec3::new(0.85, 0.916, 3.25),
    );
    let plate = commands
        .spawn((
            Transform::from_xyz(1.17, 0.9115, 3.25),
            Visibility::default(),
        ))
        .id();
    oval(commands, plate, &white, Vec3::ZERO, Vec3::new(0.115, 0.009, 0.14));
    let bread = mat(materials, Color::srgb(0.78, 0.47, 0.17), 0.85);
    for x in [-0.024, 0.024] {
        oval(commands, plate, &bread, Vec3::new(x, 0.023, 0.0), Vec3::new(0.023, 0.024, 0.09));
    }
    // A soft task light makes her face readable from inside the refrigerator.
    commands.spawn((
        TaskLamp(100.0),
        PointLight {
            intensity: 0.0,
            range: 4.0,
            radius: 0.15,
            color: Color::srgb(1.0, 0.85, 0.69),
            ..default()
        },
        Transform::from_xyz(0.3, 1.9, 0.22),
    ));
    commands.spawn((
        TaskLamp(170.0),
        PointLight {
            intensity: 0.0,
            range: 3.0,
            radius: 0.15,
            color: Color::srgb(1.0, 0.85, 0.69),
            ..default()
        },
        Transform::from_xyz(0.7, 2.0, 3.35),
    ));
    commands.insert_resource(BreakfastShift {
        stage: Stage::Opening,
        entered: 0.0,
        now: 0.0,
        door_time: 0.0,
        steps,
    });
    publish_stage(Stage::Opening.id());
    info!("Breakfast cast ready: Maya and her husband, made in code");
}

pub(crate) fn advance(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    game: Option<Res<crate::arena::Game>>,
    scene: Option<ResMut<FridgeScene>>,
    shift: Option<ResMut<BreakfastShift>>,
) {
    if game.is_some_and(|g| g.active()) {
        return;
    }
    let (Some(mut scene), Some(mut shift)) = (scene, shift) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let Some(door_time) = scene.elapsed(now) else {
        return;
    };
    shift.door_time = door_time;
    shift.now = (now - shift.entered) as f32;
    let action = take_action();
    if keys.just_pressed(KeyCode::KeyR) || action == 2 {
        scene.restart(now);
        shift.door_time = 0.0;
        shift.set_stage(Stage::Opening, now);
        return;
    }
    let called = keys.just_pressed(KeyCode::Space) || action == 1;
    if let Some(next) = shift.stage.next(shift.now, door_time, called) {
        shift.set_stage(next, now);
    }
}

/// The scene's clock now: the stage's place on it, or during the knife and the seal (`arena`) the cut scene's.
fn clip_time(shift: &BreakfastShift, game: Option<&crate::arena::Game>) -> f32 {
    game.and_then(|g| g.cutscene_time()).map(|t| motion::KNIFE_START + t).unwrap_or_else(|| shift.clip_time())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn animate(
    shift: Option<Res<BreakfastShift>>,
    game: Option<Res<crate::arena::Game>>,
    cast: Option<Res<human::Cast>>,
    parts: Query<Entity, With<Part>>,
    rigs: Query<(Entity, &human::Rig), Without<human::Idle>>,
    mut lamps: Query<(&TaskLamp, &mut PointLight)>,
    mut transforms: Query<&mut Transform>,
    mut shown: Query<&mut Visibility>,
    mut meshes: Query<&mut Mesh3d>,
) {
    if game.as_ref().is_some_and(|g| g.playing()) {
        for (lamp, mut light) in &mut lamps {
            if light.intensity != lamp.0 {
                light.intensity = lamp.0;
            }
        }
        return;
    }
    let (Some(shift), Some(cast)) = (shift, cast) else {
        return;
    };
    for (lamp, mut light) in &mut lamps {
        light.intensity = lamp.0 * smooth((shift.door_time - 2.05) / 0.18);
    }
    let clip = clip_time(&shift, game.as_deref());
    let actors = act::act(clip, &shift.steps);
    for (root, rig) in &rigs {
        let actor = actors[if rig.person == human::Person::Maya { 0 } else { 1 }];
        if let Ok(mut t) = transforms.get_mut(root) {
            let place = actor.place(1.);
            if *t != place {
                *t = place;
            }
        }
        let solved = human::solve(rig.person, &actor.pose);
        human::apply(rig, &solved, &cast, &mut transforms, &mut shown, &mut meshes);
    }
    let package = act::package(shift.clip_time());
    for entity in &parts {
        if let Ok(mut t) = transforms.get_mut(entity) {
            t.translation = package.translation;
            t.rotation = package.rotation;
        }
    }
}

pub(crate) fn animate_camera(
    shift: Option<Res<BreakfastShift>>,
    game: Option<Res<crate::arena::Game>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<HotdogCamera>>,
) {
    if game.is_some_and(|g| g.active()) {
        return;
    }
    let Some(shift) = shift else {
        return;
    };
    let p = motion::camera(shift.clip_time());
    for (mut transform, mut projection) in &mut cameras {
        *transform =
            Transform::from_xyz(p[0], p[1], p[2]).looking_at(Vec3::new(p[3], p[4], p[5]), Vec3::Y);
        if let Projection::Perspective(ref mut lens) = *projection {
            lens.fov = p[6].to_radians();
        }
    }
}

#[cfg(test)]
mod tests;

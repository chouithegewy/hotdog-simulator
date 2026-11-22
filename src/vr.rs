// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Virtual reality (the `vr` feature): the game through OpenXR, for the Meta Quest and for any desktop OpenXR runtime.
//!
//! - **The eyes.** bevy_mod_openxr spawns one camera an eye under its tracking root. The root is kept where the
//!   game's own camera is (`follow`), at the hotdog's scale ([`SCALE`]: the player is a 13 cm frankfurter, and the
//!   kitchen is as big as that makes it), so that the head is the hotdog's eye and the game's camera work (the film
//!   being cut, the fridge between rounds, the death camera) carries the player with it. The eyes are dressed as
//!   the game's camera is (`dress`: its exposure and tone mapping).
//! - **The hands** (hands.rs): the controllers are the game's touch pad, the right hand points the gun, and the gun's
//!   first-person model is carried to it (`follow`), with a dot on the line of fire.
//! - **The screen** (screen.rs): the head-up display and the menus, on a panel in the air.
//! - Without an OpenXR runtime the plugins fall back to the flat game.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy_mod_openxr::resources::OxrViews;
use bevy_mod_xr::camera::{XrCamera, XrProjection};
use bevy_mod_xr::session::XrTrackingRoot;

use crate::maya::HotdogCamera;

pub mod hands;
pub mod screen;

/// How many of the game's units a metre of the room is: the hotdog's eye (0.135) is a standing person's (1.6 m).
pub const SCALE: f32 = kitchen_sim::feast::EYE / 1.6;
/// Where the game holds a weapon's first-person model in front of its camera (arena.rs, `GRIP`).
const GRIP: Vec3 = Vec3::new(0.045, -0.047, -0.105);
/// How far along the line of fire its dot is, in the game's units (12 m of the room: far enough that both eyes
/// agree on it, near enough that a wall is seldom in front of it).
const RETICLE: f32 = 1.;
/// The dot on the line of fire.
#[derive(Component)]
struct Reticle;
/// The eyes' near plane, in metres of the room (2 mm of the kitchen at [`SCALE`]).
const NEAR: f32 = 0.025;

/// The game's plugins, drawn through OpenXR.
pub fn plugins(plugins: bevy::app::PluginGroupBuilder) -> bevy::app::PluginGroupBuilder {
    bevy_mod_openxr::add_xr_plugins(plugins).set(bevy_mod_openxr::init::OxrInitPlugin {
        app_info: bevy_mod_openxr::types::AppInfo { name: "Hotdog Simulator".into(), ..default() },
        // (Hands are the controllers': no hand tracking is asked for.)
        exts: default(),
        ..default()
    })
}

/// Where the room stands in the kitchen.
#[derive(Resource, Default)]
pub struct Stage {
    /// The head's place in the room that stands at the game's eye (set when the session's first view is found, and
    /// again whenever the head has wandered too far from it).
    origin: Option<Vec3>,
    /// The room's turn in the kitchen (radians about Y, to the left): the right stick's, and the game camera's
    /// wherever the game moves the view itself.
    pub turn: f32,
    /// Where the gun points (yaw and pitch as the game's): the right hand, or the head (hands.rs).
    pub aim: Option<(f32, f32)>,
    /// The way the head faces in the kitchen (yaw as the game's), where it is known.
    pub faced: Option<f32>,
    /// The round took the aim this frame (arena.rs): the player has the view, not a cut scene, the fridge between
    /// rounds or the death camera.
    pub played: bool,
    /// Whether the player had the view last frame (`played`, kept for the frame after), and whether the right hand
    /// points at the screen (screen.rs): its trigger is then the mouse's button, not the gun's.
    pub playing: bool,
    pub pointing: bool,
    /// Where the game's camera was a frame ago, and whether the player had the view.
    last: Option<(Vec3, bool)>,
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stage>().add_systems(Update, unmirror.run_if(bevy_mod_openxr::openxr_session_running)).add_systems(Startup, aim_dot.run_if(bevy_mod_xr::session::session_available)).add_plugins((hands::Plugin, screen::Plugin)).add_systems(Update, (dress, shoot)).add_systems(
            PostUpdate,
            follow
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy_mod_xr::session::update_root_transform)
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
    }
}

/// Whether the flat game is still drawn in its window beside the eyes: at a desk, for whoever watches; not on a
/// headset, which has no window to see and no frames to spare (`HOTDOG_VR_MIRROR=0` or `1` says otherwise).
fn mirrored() -> bool {
    static MIRROR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *MIRROR.get_or_init(|| std::env::var("HOTDOG_VR_MIRROR").map_or(!cfg!(target_os = "android"), |v| v != "0"))
}
/// Without the mirror the game's own cameras (the world's and the view model's) draw to nothing. They stay, and
/// stay active: the game moves them, and the eyes follow.
fn unmirror(mut cameras: Query<&mut bevy::camera::RenderTarget, (With<Camera3d>, Without<XrCamera>)>) {
    if mirrored() {
        return;
    }
    for mut target in &mut cameras {
        if !matches!(*target, bevy::camera::RenderTarget::None { .. }) {
            *target = bevy::camera::RenderTarget::None { size: UVec2::new(1280, 720) };
        }
    }
}

fn aim_dot(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.spawn((
        Name::new("VR line of fire"),
        Reticle,
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(1., 0.2, 0.15), unlit: true, fog_enabled: false, ..default() })),
        bevy::light::NotShadowCaster,
        bevy::camera::visibility::RenderLayers::layer(screen::LAYER),
        Visibility::Hidden,
    ));
}

/// The head in the room: midway between the eyes, and the way it faces (yaw and pitch, radians).
pub fn head(views: &OxrViews) -> Option<(Vec3, f32, f32)> {
    let (l, r) = (views.first()?, views.get(1)?);
    let at = |v: &openxr::View| Vec3::new(v.pose.position.x, v.pose.position.y, v.pose.position.z);
    let o = l.pose.orientation;
    let forward = Quat::from_xyzw(o.x, o.y, o.z, o.w).normalize() * Vec3::NEG_Z;
    Some(((at(l) + at(r)) / 2., (-forward.x).atan2(-forward.z), forward.y.clamp(-1., 1.).asin()))
}

/// Dresses the eyes as the game's camera: a near plane for a hotdog's eye once, and its exposure and tone mapping
/// whenever they differ (the game's camera hands its tone mapping to the view model's camera, viewmodel.rs, so that
/// one is asked first). The theme's light, fog and grading come from visuals.rs, which dresses every 3D camera.
fn dress(
    mut commands: Commands,
    mut eyes: Query<(Entity, &mut Projection, &mut Exposure, &mut Tonemapping, Has<Eye>), With<XrCamera>>,
    game: Query<(&Exposure, &Tonemapping), (With<HotdogCamera>, Without<XrCamera>)>,
    view: Query<&Tonemapping, (With<crate::viewmodel::ViewCamera>, Without<XrCamera>)>,
) {
    let Some((exposure, tonemapping)) = game.iter().next() else {
        return;
    };
    let tonemapping = view.iter().next().filter(|_| *tonemapping == Tonemapping::None).unwrap_or(tonemapping);
    for (eye, mut projection, mut exposed, mut mapped, dressed) in &mut eyes {
        if !dressed {
            if let Projection::Custom(custom) = projection.as_mut()
                && let Some(lens) = custom.get_mut::<XrProjection>()
            {
                lens.near = NEAR;
            }
            commands.entity(eye).insert(Eye);
        }
        if exposed.ev100 != exposure.ev100 {
            exposed.ev100 = exposure.ev100;
        }
        if *mapped != *tonemapping {
            *mapped = *tonemapping;
        }
    }
}
/// An eye that has its near plane.
#[derive(Component)]
struct Eye;

/// Stands the room in the kitchen: the head's origin at the game camera's place, at the hotdog's scale, turned as
/// the right stick has turned it. After the transforms are propagated (the game's camera may ride its body), so the
/// root's and the eyes' are written here as well.
fn follow(
    mut stage: ResMut<Stage>,
    views: Res<OxrViews>,
    game: Query<(Entity, &GlobalTransform), (With<HotdogCamera>, Without<XrTrackingRoot>, Without<XrCamera>)>,
    hands: Query<(&hands::Hand, &Transform, &bevy_mod_xr::spaces::XrSpaceLocationFlags), (Without<XrTrackingRoot>, Without<XrCamera>, Without<Reticle>)>,
    children: Query<&Children>,
    mut held: Query<&mut GlobalTransform, (Without<HotdogCamera>, Without<XrTrackingRoot>, Without<XrCamera>, Without<Reticle>)>,
    mut reticle: Query<(&mut Transform, &mut GlobalTransform, &mut Visibility), (With<Reticle>, Without<HotdogCamera>, Without<XrTrackingRoot>, Without<XrCamera>)>,
    mut root: Query<(&mut Transform, &mut GlobalTransform), (With<XrTrackingRoot>, Without<XrCamera>)>,
    mut eyes: Query<(&Transform, &mut GlobalTransform), (With<XrCamera>, Without<XrTrackingRoot>)>,
) {
    let (Some((game, camera)), Ok((mut local, mut global))) = (game.iter().next(), root.single_mut()) else {
        return;
    };
    let Some((at, ..)) = head(&views) else {
        return;
    };
    // The head's origin is where it was found, and follows a head that has walked away from it (0.75 m).
    let origin = stage.origin.get_or_insert(at);
    if (at - *origin).with_y(0.).length() > 0.75 {
        *origin = at;
    }
    let origin = *origin;
    // The room turns with the game's camera only where the game cuts the view: at the start, when the view passes
    // from the player to the game or back (a death, the film, the fridge), and when the camera jumps. A camera
    // that swings on its own (the death camera's orbit) carries the room without turning it: the head looks.
    let played = std::mem::take(&mut stage.played);
    stage.playing = played;
    let cut = stage.last.is_none_or(|(place, was)| was != played || place.distance(camera.translation()) > 0.5);
    stage.last = Some((camera.translation(), played));
    let forward = camera.rotation() * Vec3::NEG_Z;
    if cut && !played && forward.with_y(0.).length() > 1e-3 {
        // (Facing the room's front is looking the way the camera looks.)
        stage.turn = (-forward.x).atan2(-forward.z);
    }
    let rotation = Quat::from_rotation_y(stage.turn);
    let placed = Transform { translation: camera.translation() - rotation * (origin * SCALE), rotation, scale: Vec3::splat(SCALE) };
    *local = placed;
    *global = GlobalTransform::from(placed);
    for (eye, mut global) in &mut eyes {
        *global = GlobalTransform::from(placed) * *eye;
    }
    // The dot on the line of fire: rounds leave the game's eye the way its camera looks, whichever hand holds the gun.
    if let Ok((mut dot, mut global, mut seen)) = reticle.single_mut() {
        *dot = Transform::from_translation(camera.translation() + camera.rotation() * Vec3::NEG_Z * RETICLE).with_scale(Vec3::splat(RETICLE * 0.004));
        *global = GlobalTransform::from(*dot);
        seen.set_if_neq(if played { Visibility::Visible } else { Visibility::Hidden });
    }
    // The gun is in the hand: what the game hangs from its camera (the first-person weapon and its hands,
    // viewmodel.rs, held at arena.rs's GRIP in front of the eye) is carried from there to the right hand, which
    // points the way the camera already looks (hands.rs). A hand out of sight leaves it where the game put it.
    let hand = hands.iter().find(|(hand, _, flags)| **hand == hands::Hand::Grip && flags.position_tracked).map(|(_, hand, _)| placed.transform_point(hand.translation));
    if let Some(hand) = hand.filter(|_| played) {
        let carried = hand - camera.transform_point(GRIP);
        for part in children.iter_descendants(game) {
            if let Ok(mut global) = held.get_mut(part) {
                *global = GlobalTransform::from(Transform::from_translation(carried)) * *global;
            }
        }
    }
}

/// Checks only: `HOTDOG_VR_SHOT=path.png` saves what the left eye sees twelve seconds in (`HOTDOG_VR_SHOT_AT` to
/// change that), the right eye's beside it (`path.right.png`) and the screen's picture (`path.screen.png`), and
/// logs where the room stands.
fn shoot(mut commands: Commands, time: Res<Time<Real>>, eyes: Query<&GlobalTransform, With<XrCamera>>, stage: Res<Stage>, screen: Option<Res<screen::Screen>>, mut done: Local<bool>) {
    let Ok(path) = std::env::var("HOTDOG_VR_SHOT") else {
        return;
    };
    let at = std::env::var("HOTDOG_VR_SHOT_AT").ok().and_then(|t| t.parse().ok()).unwrap_or(12.);
    if *done || time.elapsed_secs() < at || eyes.is_empty() {
        return;
    }
    *done = true;
    info!("VR: eyes at {:?}; the room turned {}, aim {:?}, the player has the view: {}", eyes.iter().map(|eye| eye.translation()).collect::<Vec<_>>(), stage.turn, stage.aim, stage.playing);
    use bevy::camera::ManualTextureViewHandle;
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let index = bevy_mod_openxr::render::XR_TEXTURE_INDEX;
    if let Some(screen) = screen {
        commands.spawn(Screenshot::image(screen.image.clone())).observe(save_to_disk(path.replace(".png", ".screen.png")));
    }
    commands.spawn(Screenshot::texture_view(ManualTextureViewHandle(index))).observe(save_to_disk(path.clone()));
    commands.spawn(Screenshot::texture_view(ManualTextureViewHandle(index + 1))).observe(save_to_disk(path.replace(".png", ".right.png")));
}

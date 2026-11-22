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
mod timing;

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

/// Checks only: switches for measuring on a headset, `name=value` pairs with `;` or a new line between them, in
/// the file `HOTDOG_VR_KNOBS` names or, on Android, in the system property `debug.hotdog.vr`
/// (`adb shell setprop debug.hotdog.vr "intro=0;timing=1"`, then start the app again). `eye=1008x1108` asks for
/// eyes of that size, `screen=0` leaves the screen in the air out, `world=0` shows the eyes nothing of the world,
/// `intro=0` skips the film (runtime.rs), `mirror=1` draws the flat picture as well, `timing=1` logs where a
/// frame's time goes (timing.rs); `bisect` has more.
pub fn knob(name: &str) -> Option<String> {
    static KNOBS: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    let knobs = KNOBS.get_or_init(|| {
        let text = std::env::var_os("HOTDOG_VR_KNOBS").and_then(|file| std::fs::read_to_string(file).ok()).unwrap_or_default();
        #[cfg(target_os = "android")]
        let text = text + ";" + &property("debug.hotdog.vr");
        let knobs: Vec<(String, String)> = text.split([';', '\n']).filter_map(|pair| pair.trim().split_once('=')).map(|(name, value)| (name.trim().to_string(), value.trim().to_string())).collect();
        if !knobs.is_empty() {
            info!("VR: checks' switches {knobs:?}");
        }
        knobs
    });
    knobs.iter().find(|(known, _)| known == name).map(|(_, value)| value.clone())
}
/// An Android system property (empty when it is not set).
#[cfg(target_os = "android")]
fn property(name: &str) -> String {
    unsafe extern "C" {
        fn __system_property_get(name: *const std::ffi::c_char, value: *mut std::ffi::c_char) -> std::ffi::c_int;
    }
    let Ok(name) = std::ffi::CString::new(name) else {
        return String::new();
    };
    // (PROP_VALUE_MAX is 92.)
    let mut value = [0 as std::ffi::c_char; 128];
    // SAFETY: `name` ends in a nul, and `value` is longer than any property's value.
    let length = unsafe { __system_property_get(name.as_ptr(), value.as_mut_ptr()) };
    let bytes: Vec<u8> = value[..length.clamp(0, 127) as usize].iter().map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

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
        let eye = knob("eye").and_then(|eye| eye.split_once('x').and_then(|(w, h)| Some(UVec2::new(w.parse().ok()?, h.parse().ok()?)))).or_else(|| headset().then_some(EYE));
        if let Some(eye) = eye {
            app.insert_resource(bevy_mod_openxr::resources::OxrSessionConfig { resolutions: Some(vec![eye]), ..default() });
        }
        // A headset is never out of focus as a window is: every frame is drawn as soon as the runtime wants it (the
        // defaults pace a phone's app, and a window nobody sees counts as unfocused).
        app.insert_resource(bevy::winit::WinitSettings::continuous());
        app.add_systems(Last, (bisect, list, plain, strip));
        app.init_resource::<Stage>().add_systems(Update, unmirror.run_if(bevy_mod_openxr::openxr_session_running)).add_systems(Startup, aim_dot.run_if(bevy_mod_xr::session::session_available)).add_plugins((hands::Plugin, screen::Plugin, timing::Plugin)).add_systems(Update, (dress, shoot)).add_systems(
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
    *MIRROR.get_or_init(|| std::env::var("HOTDOG_VR_MIRROR").ok().or_else(|| knob("mirror")).map_or(!cfg!(target_os = "android"), |v| v != "0"))
}
/// Without the mirror the game's own cameras (the world's and the view model's) draw to nothing. They stay, and
/// stay active: the game moves them, and the eyes follow.
fn unmirror(mut cameras: Query<&mut bevy::camera::RenderTarget, (With<Camera3d>, Without<XrCamera>)>) {
    if mirrored() {
        return;
    }
    for mut target in &mut cameras {
        let size = if knob("flat").as_deref() == Some("tiny") { UVec2::new(16, 9) } else { UVec2::new(1280, 720) };
        if !matches!(*target, bevy::camera::RenderTarget::None { size: now } if now == size) {
            *target = bevy::camera::RenderTarget::None { size };
        }
    }
}

/// Checks only (`knob`): `flat=0` switches the game's own cameras off, `shadows=0` every lamp's shadow map,
/// `ui=0` the screen's camera, `eyes=0` the eyes themselves.
fn bisect(
    mut hidden: Query<(Has<DirectionalLight>, &mut Visibility), Or<(With<DirectionalLight>, With<PointLight>, With<SpotLight>)>>,
    mut commands: Commands,
    eyes: Query<Entity, With<XrCamera>>,
    mut cameras: Query<(&mut Camera, Has<XrCamera>, Has<Camera3d>)>,
    mut points: Query<&mut PointLight>,
    mut spots: Query<&mut SpotLight>,
    mut suns: Query<&mut DirectionalLight>,
) {
    let off = |name: &str| knob(name).as_deref() == Some("0");
    for (mut camera, eye, world) in &mut cameras {
        let unwanted = if eye { off("eyes") } else if world { off("flat") } else { off("ui") };
        if unwanted && camera.is_active {
            camera.is_active = false;
        }
    }
    if off("lights") {
        for mut light in &mut points {
            if light.intensity != 0. {
                light.intensity = 0.;
                light.range = 0.001;
            }
        }
        for mut light in &mut spots {
            if light.intensity != 0. {
                light.intensity = 0.;
                light.range = 0.001;
            }
        }
    }
    if off("post") {
        use bevy::anti_alias::{contrast_adaptive_sharpening::ContrastAdaptiveSharpening, fxaa::Fxaa};
        for eye in &eyes {
            commands.entity(eye).remove::<(Fxaa, ContrastAdaptiveSharpening, bevy::post_process::bloom::Bloom, bevy::pbr::DistanceFog, bevy::render::view::ColorGrading)>().insert((Tonemapping::None, bevy::core_pipeline::tonemapping::DebandDither::Disabled));
        }
    }
    // `lamps=gone`, `sun=gone`: the lights are hidden outright, so no fragment sees them at all.
    for (wanted, mut seen) in &mut hidden {
        let gone = match wanted {
            true => knob("sun").as_deref() == Some("gone"),
            false => knob("lamps").as_deref() == Some("gone"),
        };
        if gone && *seen != Visibility::Hidden {
            *seen = Visibility::Hidden;
        }
    }
    if off("shadows") {
        for mut light in &mut points {
            if light.shadow_maps_enabled {
                light.shadow_maps_enabled = false;
            }
        }
        for mut light in &mut spots {
            if light.shadow_maps_enabled {
                light.shadow_maps_enabled = false;
            }
        }
        for mut light in &mut suns {
            if light.shadow_maps_enabled {
                light.shadow_maps_enabled = false;
            }
        }
    }
}

/// Checks only (`knob`, `eyes=list`): logs once, ten seconds in, what an eye carries.
fn list(world: &mut World, mut done: Local<bool>) {
    if *done || knob("eyes").as_deref() != Some("list") || world.resource::<Time<Real>>().elapsed_secs() < 10. {
        return;
    }
    let Some(eye) = world.query_filtered::<Entity, With<XrCamera>>().iter(world).next() else {
        return;
    };
    *done = true;
    if let Ok(parts) = world.inspect_entity(eye) {
        let names: Vec<String> = parts.map(|part| part.name().to_string().rsplit("::").next().unwrap_or_default().to_string()).collect();
        info!("VR eye: {}", names.join(" "));
    }
    let msaa = world.get::<Msaa>(eye).copied();
    info!("VR eye: {msaa:?}");
}

/// Whether the picture is drawn for a headset's own chip (a Quest's: a phone's GPU that draws every picture twice,
/// 72 times a second): always on Android, and at a desk with the switch `headset=1`; `lit=1` keeps the lighting.
///
/// Measured on a Quest 2 in a round, eyes of 1440 by 1584 (frame times; the headset wants 13.9 ms): as the Lowest
/// level draws it 80 ms; without the screen in the air 60; with the sun's and the lamps' light gone as well 30;
/// unlit 22; unlit and nothing done to the finished picture 15. The lit path costs the same whatever lights there
/// are, so the headset draws unlit ([`plain`]), finishes nothing ([`strip`]), and draws smaller eyes ([`EYE`]).
pub fn headset() -> bool {
    static HEADSET: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *HEADSET.get_or_init(|| knob("headset").map_or(cfg!(target_os = "android"), |v| v != "0"))
}
/// A headset's eyes, in pixels: three quarters each way of what a Quest 2 asks for (`eye=WxH` asks for another).
const EYE: UVec2 = UVec2::new(1080, 1188);

/// A headset's materials are unlit: their colour and their texture, as they are. Every frame, for the materials
/// made or changed since (the players' own, which bend, and the titan's as well).
fn plain(
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bent: Option<ResMut<Assets<crate::player_model::bend::BentMaterial>>>,
    mut titans: Option<ResMut<Assets<crate::titan::rig::TitanMaterial>>>,
) {
    if !headset() || knob("lit").as_deref() == Some("1") {
        return;
    }
    let lit: Vec<_> = materials.iter().filter(|(_, m)| !m.unlit).map(|(id, _)| id).collect();
    if !lit.is_empty() && knob("timing").is_some() {
        info!("VR plain: {} of {} materials were lit", lit.len(), materials.len());
    }
    for id in lit {
        if let Some(mut m) = materials.get_mut(id) {
            m.unlit = true;
        }
    }
    if let Some(bent) = bent.as_mut() {
        let lit: Vec<_> = bent.iter().filter(|(_, m)| !m.base.unlit).map(|(id, _)| id).collect();
        for id in lit {
            if let Some(mut m) = bent.get_mut(id) {
                m.base.unlit = true;
            }
        }
    }
    if let Some(titans) = titans.as_mut() {
        let lit: Vec<_> = titans.iter().filter(|(_, m)| !m.base.unlit).map(|(id, _)| id).collect();
        for id in lit {
            if let Some(mut m) = titans.get_mut(id) {
                m.base.unlit = true;
            }
        }
    }
}

/// A headset's eyes finish nothing: no anti-aliasing pass, no sharpening, no bloom, no grading, no fog, and no
/// lamp casts a shadow nobody is lit by. Every frame: visuals.rs dresses every camera again when the theme changes.
fn strip(
    mut commands: Commands,
    eyes: Query<(Entity, Has<bevy::anti_alias::fxaa::Fxaa>, Has<bevy::post_process::bloom::Bloom>, Has<bevy::pbr::DistanceFog>, Has<bevy::render::view::ColorGrading>), With<XrCamera>>,
    mut tones: Query<(&mut Tonemapping, &mut bevy::core_pipeline::tonemapping::DebandDither), With<XrCamera>>,
    mut points: Query<&mut PointLight>,
    mut spots: Query<&mut SpotLight>,
    mut suns: Query<&mut DirectionalLight>,
) {
    if !headset() || knob("lit").as_deref() == Some("1") {
        return;
    }
    use bevy::anti_alias::{contrast_adaptive_sharpening::ContrastAdaptiveSharpening, fxaa::Fxaa};
    for (eye, fxaa, bloom, fog, grading) in &eyes {
        if fxaa || bloom || fog || grading {
            commands.entity(eye).remove::<(Fxaa, ContrastAdaptiveSharpening, bevy::post_process::bloom::Bloom, bevy::pbr::DistanceFog, bevy::render::view::ColorGrading)>();
        }
    }
    // (Unlit colours are the picture's own: nothing to map down from a brighter range, nothing to dither.)
    for (mut mapped, mut dither) in &mut tones {
        if *mapped != Tonemapping::None {
            *mapped = Tonemapping::None;
        }
        if *dither != bevy::core_pipeline::tonemapping::DebandDither::Disabled {
            *dither = bevy::core_pipeline::tonemapping::DebandDither::Disabled;
        }
    }
    for mut light in &mut points {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
    for mut light in &mut spots {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
    for mut light in &mut suns {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
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
        let unlit = headset() && knob("lit").as_deref() != Some("1");
        if !unlit && *mapped != *tonemapping {
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
    let Some((at, facing, _)) = head(&views) else {
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
        // (The way the head faces now, wherever in the room that is, is the way the camera looks.)
        stage.turn = (-forward.x).atan2(-forward.z) - facing;
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

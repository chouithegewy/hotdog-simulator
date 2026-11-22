// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The screen in the air: the game's head-up display and menus, which are drawn for a flat screen, on a panel that
//! floats in front of the player.
//!
//! - **The picture.** A 2D camera draws all of the game's interface (it is the default interface camera) into an
//!   image the size of the window (1280 by 720 where there is none), and a panel in the room shows that image
//!   ([`WIDTH`] wide, [`AWAY`] from the head), on a render layer only the eyes see ([`LAYER`]).
//! - **Where it floats.** It follows the head, eased, so that the health and the ammunition stay in view. While the
//!   menu is open it stays where it is, to be pointed at, and comes back in front of a head that has turned away.
//! - **Pointing.** Wherever the player does not have the view (the menu, the fridge between rounds), the right hand
//!   points at the panel: a dot shows where, the trigger is the left mouse button (hands.rs), and the interface's
//!   buttons and sliders hear it (`focus`: Bevy's own hears only a window's pointer).
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui::{ComputedUiTargetCamera, FocusPolicy, IsDefaultUiCamera, OverrideClip, RelativeCursorPosition, UiGlobalTransform, UiStack};
use bevy_mod_openxr::resources::OxrViews;
use bevy_mod_xr::camera::XrCamera;
use bevy_mod_xr::session::XrTrackingRoot;
use bevy_mod_xr::spaces::XrSpaceLocationFlags;

use super::hands::Hand;

/// The render layer of what only the eyes see: the panel and its dot.
pub const LAYER: usize = 30;
/// The panel, in metres of the room: how wide, and how far from the head.
const WIDTH: f32 = 1.7;
const AWAY: f32 = 1.5;
/// The most pixels the picture is tall.
const TALL: f32 = 720.;
/// How fast the panel catches the head up (a second to the power of minus one), and how far the head turns from a
/// panel that is standing still (the menu's) before it comes back in front.
const EASE: f32 = 9.;
const AWAY_TURN: f32 = 1.0;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pointer>()
            .add_systems(Startup, build.run_if(bevy_mod_xr::session::session_available))
            .add_systems(Update, (fit, see))
            .add_systems(
                PreUpdate,
                (point, focus)
                    .chain()
                    .after(super::hands::read)
                    .after(bevy::ui::UiSystems::Focus)
                    .before(crate::controls::Shut)
                    .run_if(resource_exists::<Screen>),
            )
            .add_systems(PostUpdate, float.before(bevy::transform::TransformSystems::Propagate).run_if(resource_exists::<Screen>));
    }
}

#[derive(Resource)]
pub struct Screen {
    pub image: Handle<Image>,
    camera: Entity,
    material: Handle<StandardMaterial>,
    panel: Entity,
    dot: Entity,
    /// The picture's size, in pixels.
    size: UVec2,
    /// Where the panel floats, in the room: the way from the head (yaw and pitch), once the head is known.
    way: Option<(f32, f32)>,
}

/// Where the right hand points on the screen, in its pixels, and whether its trigger is the mouse's button.
#[derive(Resource, Default)]
pub struct Pointer {
    pub at: Option<Vec2>,
    held: Vec<Entity>,
}

fn build(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, root: Query<Entity, With<XrTrackingRoot>>) {
    let Ok(root) = root.single() else {
        return;
    };
    if super::knob("screen").as_deref() == Some("0") {
        return;
    }
    // (Not yet any window's size: `fit` makes the picture that is shown.)
    let size = UVec2::new(16, 9);
    let image = images.add(Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None));
    let camera = commands
        .spawn((
            Name::new("VR screen camera"),
            Camera2d,
            Camera { order: 10, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
            RenderTarget::Image(ImageRenderTarget { handle: image.clone(), scale_factor: 1. }),
            IsDefaultUiCamera,
            Msaa::Off,
            // (The interface's colours are the picture's own.)
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            bevy::core_pipeline::tonemapping::DebandDither::Disabled,
            // (Nothing of the world is on its layer: the interface alone.)
            RenderLayers::layer(LAYER + 1),
        ))
        .id();
    let material = materials.add(StandardMaterial { base_color_texture: Some(image.clone()), unlit: true, alpha_mode: AlphaMode::Blend, cull_mode: None, fog_enabled: false, ..default() });
    let panel = commands
        .spawn((
            Name::new("VR screen"),
            Mesh3d(meshes.add(Rectangle::new(1., 1.))),
            MeshMaterial3d(material.clone()),
            NotShadowCaster,
            RenderLayers::layer(LAYER),
            Visibility::Hidden,
            ChildOf(root),
        ))
        .id();
    let dot = commands
        .spawn((
            Name::new("VR pointer"),
            Mesh3d(meshes.add(Sphere::new(0.008))),
            MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(1., 0.85, 0.2), unlit: true, fog_enabled: false, ..default() })),
            NotShadowCaster,
            RenderLayers::layer(LAYER),
            Visibility::Hidden,
            ChildOf(root),
        ))
        .id();
    commands.insert_resource(Screen { image, camera, material, panel, dot, size, way: None });
}

/// The picture is laid out for the window's size, which is what the interface is made for (hud.rs, menu.rs), and
/// drawn no taller than [`TALL`] pixels: a taller one is drawn at a fraction of its size (the target's scale factor).
/// A picture of another size is a new image: the camera draws to it and the panel shows it.
fn fit(screen: Option<ResMut<Screen>>, windows: Query<&Window>, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<StandardMaterial>>, mut targets: Query<&mut RenderTarget>) {
    let (Some(mut screen), Some(window)) = (screen, windows.iter().next()) else {
        return;
    };
    let laid = Vec2::new(window.width(), window.height()).max(Vec2::new(320., 180.));
    let scale = (TALL / laid.y).min(1.);
    let size = (laid * scale).round().as_uvec2().max(UVec2::ONE);
    if size == screen.size {
        return;
    }
    info!("VR screen: the window is {} by {}; the picture {} by {}", laid.x, laid.y, size.x, size.y);
    let image = images.add(Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None));
    if let Ok(mut target) = targets.get_mut(screen.camera) {
        *target = RenderTarget::Image(ImageRenderTarget { handle: image.clone(), scale_factor: scale });
    }
    if let Some(mut material) = materials.get_mut(&screen.material) {
        material.base_color_texture = Some(image.clone());
    }
    (screen.image, screen.size) = (image, size);
}

/// The eyes see the world and the screen's layer, and while the player has the view the first-person weapon and
/// hands (viewmodel.rs, which draws them for the flat game with a camera of their own).
fn see(mut commands: Commands, stage: Res<super::Stage>, eyes: Query<(Entity, Option<&RenderLayers>), With<XrCamera>>) {
    let world = if super::knob("world").as_deref() == Some("0") { 29 } else { 0 };
    let wanted = if stage.playing { RenderLayers::from_layers(&[world, crate::viewmodel::VIEW_LAYER, LAYER]) } else { RenderLayers::from_layers(&[world, LAYER]) };
    for (eye, layers) in &eyes {
        if layers != Some(&wanted) {
            commands.entity(eye).insert(wanted.clone());
        }
    }
}

/// The panel's width (checks: `HOTDOG_VR_SCREEN` metres instead, for a headset with a narrow view).
fn width() -> f32 {
    static WIDE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *WIDE.get_or_init(|| std::env::var("HOTDOG_VR_SCREEN").ok().and_then(|w| w.parse().ok()).unwrap_or(WIDTH))
}
/// The panel's place in the room for a head at `head` looking at it along `way` (yaw, pitch), and its size.
fn place(head: Vec3, way: (f32, f32), size: UVec2) -> Transform {
    let rotation = Quat::from_rotation_y(way.0) * Quat::from_rotation_x(way.1);
    Transform { translation: head + rotation * Vec3::NEG_Z * AWAY, rotation, scale: Vec3::new(width(), width() * size.y as f32 / size.x.max(1) as f32, 1.) }
}

/// Floats the panel in front of the head.
fn float(mut screen: ResMut<Screen>, views: Res<OxrViews>, time: Res<Time<Real>>, menu: Option<Res<crate::menu::Menu>>, mut panels: Query<(&mut Transform, &mut Visibility)>) {
    let Some((head, yaw, pitch)) = super::head(&views) else {
        return;
    };
    let still = menu.is_some_and(|menu| menu.open);
    let turn = |from: f32, to: f32| (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    let way = match screen.way {
        None => (yaw, pitch),
        Some(way) if still => {
            if turn(way.0, yaw).abs() > AWAY_TURN || (pitch - way.1).abs() > AWAY_TURN { (yaw, pitch) } else { way }
        }
        Some(way) => {
            let k = 1. - (-EASE * time.delta_secs()).exp();
            (way.0 + turn(way.0, yaw) * k, way.1 + (pitch - way.1) * k)
        }
    };
    screen.way = Some(way);
    if let Ok((mut placed, mut seen)) = panels.get_mut(screen.panel) {
        *placed = place(head, way, screen.size);
        *seen = Visibility::Visible;
    }
}

/// Where a ray from `from` along `along` meets the panel, in the picture's pixels, and in the room.
fn meet(panel: &Transform, size: UVec2, from: Vec3, along: Vec3) -> Option<(Vec2, Vec3)> {
    let normal = panel.rotation * Vec3::Z;
    let facing = along.dot(normal);
    if facing > -1e-4 {
        return None;
    }
    let at = from + along * ((panel.translation - from).dot(normal) / facing);
    let local = panel.rotation.inverse() * (at - panel.translation);
    let (x, y) = (local.x / panel.scale.x + 0.5, 0.5 - local.y / panel.scale.y);
    ((0. ..=1.).contains(&x) && (0. ..=1.).contains(&y)).then(|| (Vec2::new(x * size.x as f32, y * size.y as f32), at))
}

/// Finds where the right hand points on the screen, wherever the player does not have the view, and shows the dot.
fn point(
    screen: Res<Screen>,
    mut pointer: ResMut<Pointer>,
    mut stage: ResMut<super::Stage>,
    menu: Option<Res<crate::menu::Menu>>,
    hands: Query<(&Hand, &Transform, &XrSpaceLocationFlags)>,
    mut things: Query<(&mut Transform, &mut Visibility), Without<Hand>>,
) {
    let wanted = menu.is_some_and(|menu| menu.open) || !stage.playing;
    let panel = things.get(screen.panel).ok().map(|(placed, _)| *placed);
    let hit = panel.filter(|_| wanted).and_then(|panel| {
        let (_, hand, _) = hands.iter().find(|(hand, _, flags)| **hand == Hand::Point && flags.rotation_tracked)?;
        meet(&panel, screen.size, hand.translation, hand.rotation * Vec3::NEG_Z)
    });
    pointer.at = hit.map(|(at, _)| at);
    stage.pointing = hit.is_some();
    if let Ok((mut placed, mut seen)) = things.get_mut(screen.dot) {
        if let Some((_, at)) = hit {
            placed.translation = at;
        }
        seen.set_if_neq(if hit.is_some() { Visibility::Visible } else { Visibility::Hidden });
    }
}

/// Bevy's `ui_focus_system` for the pointer on the screen: the topmost node under it is hovered, and pressed while
/// the trigger (the left mouse button, hands.rs) that was pressed on it is held; sliders are told where on them it
/// is. Runs after Bevy's, which has cleared every node of a camera that draws to no window.
#[allow(clippy::type_complexity)]
fn focus(
    screen: Res<Screen>,
    mut pointer: ResMut<Pointer>,
    mouse: Res<ButtonInput<MouseButton>>,
    stack: Res<UiStack>,
    mut nodes: Query<(&ComputedNode, &UiGlobalTransform, Option<&mut Interaction>, Option<&mut RelativeCursorPosition>, Option<&FocusPolicy>, Option<&InheritedVisibility>, &ComputedUiTargetCamera)>,
    clipping: Query<(&ComputedNode, &UiGlobalTransform, &Node)>,
    parents: Query<&ChildOf, Without<OverrideClip>>,
) {
    if !mouse.pressed(MouseButton::Left) {
        pointer.held.clear();
    }
    let Some(at) = pointer.at else {
        pointer.held.clear();
        return;
    };
    let clicked = mouse.just_pressed(MouseButton::Left);
    let mut blocked = false;
    for entity in stack.uinodes.iter().rev().copied() {
        let Ok((node, transform, interaction, relative, policy, visible, camera)) = nodes.get_mut(entity) else {
            continue;
        };
        if camera.get() != Some(screen.camera) || !visible.is_some_and(|v| v.get()) {
            continue;
        }
        let over = node.contains_point(*transform, at) && bevy::ui::clip_check_recursive(at, entity, &clipping, &parents);
        if let Some(mut relative) = relative {
            relative.set_if_neq(RelativeCursorPosition { cursor_over: over, normalized: node.normalize_point(*transform, at) });
        }
        let Some(mut interaction) = interaction else {
            if over && !blocked && *policy.unwrap_or(&FocusPolicy::Block) == FocusPolicy::Block {
                blocked = true;
            }
            continue;
        };
        if pointer.held.contains(&entity) {
            interaction.set_if_neq(Interaction::Pressed);
        } else if over && !blocked {
            if clicked {
                pointer.held.push(entity);
                interaction.set_if_neq(Interaction::Pressed);
            } else {
                interaction.set_if_neq(Interaction::Hovered);
            }
        }
        if over && *policy.unwrap_or(&FocusPolicy::Block) == FocusPolicy::Block {
            blocked = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_pointing_at_the_panel_finds_its_pixel() {
        let size = UVec2::new(1280, 720);
        let panel = place(Vec3::new(0., 1.6, 0.), (0., 0.), size);
        assert!((panel.translation - Vec3::new(0., 1.6, -AWAY)).length() < 1e-5);
        // Straight at the middle from the head.
        let (at, _) = meet(&panel, size, Vec3::new(0., 1.6, 0.), Vec3::NEG_Z).unwrap();
        assert!((at - Vec2::new(640., 360.)).length() < 0.01, "{at}");
        // From the hip, up and to the left: the top left quarter (y grows downwards on the screen).
        let from = Vec3::new(0.2, 1.1, -0.3);
        let to = panel.translation + Vec3::new(-WIDTH / 4., WIDTH * 720. / 1280. / 4., 0.);
        let (at, room) = meet(&panel, size, from, (to - from).normalize()).unwrap();
        assert!((at - Vec2::new(320., 180.)).length() < 0.01 && (room - to).length() < 1e-4, "{at}");
        // Past its edge, away from it, and from behind it: nothing.
        assert!(meet(&panel, size, from, (panel.translation + Vec3::X * WIDTH - from).normalize()).is_none());
        assert!(meet(&panel, size, from, Vec3::Z).is_none());
        assert!(meet(&panel, size, Vec3::new(0., 1.6, -3.), Vec3::Z).is_none());
    }

    #[test]
    fn the_panel_faces_a_head_that_looks_another_way() {
        let size = UVec2::new(1280, 720);
        // A quarter turn to the left: the panel is to the head's left (-X) and its face looks back at the head.
        let panel = place(Vec3::ZERO, (std::f32::consts::FRAC_PI_2, 0.), size);
        assert!((panel.translation - Vec3::NEG_X * AWAY).length() < 1e-5);
        assert!((panel.rotation * Vec3::Z - Vec3::X).length() < 1e-5);
        assert!(meet(&panel, size, Vec3::ZERO, Vec3::NEG_X).is_some());
    }
}

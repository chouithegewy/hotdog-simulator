// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The controllers (Meta Quest Touch, and whatever else the runtime maps the same paths to).
//!
//! The game already has a pad that is not a keyboard: the page's touch controls (touch.rs), a stick, a look and
//! fifteen buttons that press their actions' keys. The controllers are read into one of its frames ([`frame`]), so
//! every mode's buttons work as they do on a phone:
//!
//! | Control | Does |
//! |---|---|
//! | Left stick | Walk, the way the head faces |
//! | Right stick, left and right | Turn the room 30 degrees |
//! | Right stick, up / down | Swap weapon / go prone |
//! | Right trigger | Fire (throw, punch) |
//! | Left trigger | Aim (the scope; the heavy blow, the smash) |
//! | Right grip | Use |
//! | Left grip | Carry (block) |
//! | A | Jump, and the next round |
//! | B | Crouch (held) |
//! | X | Ready (reload) |
//! | Y | Melee (kick, stomp) |
//! | Left stick, pressed | Stim (dodge, charge) |
//! | Right stick, pressed | Hints |
//! | Menu | The menu (Esc) |
//!
//! The gun points where the right controller points ([`super::Stage::aim`]); a hand that is not tracked leaves the
//! aim to the head.
use std::sync::Mutex;

use bevy::prelude::*;
use bevy_mod_openxr::action_binding::{OxrSendActionBindings, OxrSuggestActionBinding};
use bevy_mod_openxr::action_set_attaching::OxrAttachActionSet;
use bevy_mod_openxr::action_set_syncing::{OxrActionSetSyncSet, OxrSyncActionSet};
use bevy_mod_openxr::resources::{OxrInstance, OxrViews};
use bevy_mod_openxr::session::OxrSession;
use bevy_mod_xr::session::{XrSessionCreated, session_available};
use bevy_mod_xr::spaces::{XrSpaceLocationFlags, XrSpaceSyncSet};

use crate::touch;

/// How far the room turns at a flick of the right stick.
const TURN: f32 = std::f32::consts::PI / 6.;
/// A stick pushed this far is a flick, and is over when it is back under [`REST`].
const FLICK: f32 = 0.65;
const REST: f32 = 0.35;
/// Under this the walking stick is at rest (a worn stick does not creep).
const DEAD: f32 = 0.15;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create.run_if(session_available))
            .add_systems(OxrSendActionBindings, suggest.run_if(resource_exists::<Actions>))
            .add_systems(XrSessionCreated, attach.run_if(resource_exists::<Actions>))
            .add_systems(PreUpdate, sync.before(OxrActionSetSyncSet).run_if(bevy_mod_openxr::openxr_session_running).run_if(resource_exists::<Actions>))
            .add_systems(
                PreUpdate,
                read.after(OxrActionSetSyncSet)
                    .after(XrSpaceSyncSet)
                    .after(bevy::input::InputSystems)
                    .before(touch::press)
                    .before(crate::controls::Shut)
                    .run_if(bevy_mod_openxr::openxr_session_running)
                    .run_if(resource_exists::<Actions>),
            );
    }
}

/// The controllers this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Raw {
    pub left: Vec2,
    pub right: Vec2,
    pub fire: bool,
    pub aim: bool,
    pub use_: bool,
    pub carry: bool,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub left_click: bool,
    pub right_click: bool,
}

/// What a flick of the right stick has done, so that it does it once.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flick {
    side: i8,
    up: i8,
    /// The buttons held last frame.
    down: u32,
}

/// One frame of the touch pad from the controllers (touch.rs, `Frame`: stick x and y, look x and y, the buttons
/// down, the buttons pressed since the last frame, cursor x and y, pinch), and how far the room turns (radians, to
/// the left). `walk` is how far the head faces to the left of the aim (radians): the stick walks the way the head
/// faces, and the game walks the way the gun points.
pub fn frame(raw: &Raw, flick: &mut Flick, walk: f32) -> ([f32; 9], f32) {
    let side = if raw.right.x.abs() > FLICK { raw.right.x.signum() as i8 } else if raw.right.x.abs() < REST { 0 } else { flick.side };
    let up = if raw.right.y.abs() > FLICK { raw.right.y.signum() as i8 } else if raw.right.y.abs() < REST { 0 } else { flick.up };
    let turn = if side != 0 && flick.side == 0 { -TURN * side as f32 } else { 0. };
    let mut down = 0;
    for (on, bit) in [
        (raw.fire, touch::FIRE),
        (raw.aim, touch::SCOPE),
        (raw.use_, touch::USE),
        (raw.carry, touch::CARRY),
        (raw.a, touch::JUMP | touch::ROUND),
        (raw.b, touch::CROUCH),
        (raw.x, touch::READY),
        (raw.y, touch::MELEE),
        (raw.left_click, touch::STIM),
        (raw.right_click, touch::HINTS),
        (up == 1, touch::SWAP),
        (up == -1, touch::PRONE),
    ] {
        if on {
            down |= bit;
        }
    }
    let taps = down & !flick.down;
    *flick = Flick { side, up, down };
    // The stick: x to the right and y back, in the head's frame; turned into the aim's.
    let stick = if raw.left.length() < DEAD { Vec2::ZERO } else { Vec2::new(raw.left.x, -raw.left.y).clamp_length_max(1.) };
    let (sin, cos) = walk.sin_cos();
    let stick = Vec2::new(stick.x * cos + stick.y * sin, -stick.x * sin + stick.y * cos);
    ([stick.x, stick.y, 0., 0., down as f32, taps as f32, 0., 0., 0.], turn)
}

static FRAME: Mutex<Option<[f32; 9]>> = Mutex::new(None);
/// This frame of the controllers for touch.rs (nothing without a running session), taken once.
pub fn take() -> Vec<f32> {
    FRAME.lock().ok().and_then(|mut frame| frame.take()).map_or_else(Vec::new, |frame| frame.to_vec())
}

#[derive(Resource)]
pub struct Actions {
    set: openxr::ActionSet,
    left: openxr::Action<openxr::Vector2f>,
    right: openxr::Action<openxr::Vector2f>,
    fire: openxr::Action<f32>,
    aim: openxr::Action<f32>,
    use_: openxr::Action<f32>,
    carry: openxr::Action<f32>,
    a: openxr::Action<bool>,
    b: openxr::Action<bool>,
    x: openxr::Action<bool>,
    y: openxr::Action<bool>,
    left_click: openxr::Action<bool>,
    right_click: openxr::Action<bool>,
    menu: openxr::Action<bool>,
    point: openxr::Action<openxr::Posef>,
    grip: openxr::Action<openxr::Posef>,
    off: openxr::Action<openxr::Posef>,
}

/// The right hand's pointing pose (the gun's), its grip (what is held), and the left hand's grip.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hand {
    Point,
    Grip,
    Off,
}

fn create(instance: Res<OxrInstance>, mut commands: Commands) {
    let made = || -> openxr::Result<Actions> {
        let set = instance.create_action_set("hotdog", "Hotdog Simulator", 0)?;
        Ok(Actions {
            left: set.create_action("walk", "Walk", &[])?,
            right: set.create_action("turn", "Turn, swap weapon, go prone", &[])?,
            fire: set.create_action("fire", "Fire", &[])?,
            aim: set.create_action("aim", "Aim", &[])?,
            use_: set.create_action("use", "Use", &[])?,
            carry: set.create_action("carry", "Carry", &[])?,
            a: set.create_action("jump", "Jump", &[])?,
            b: set.create_action("crouch", "Crouch", &[])?,
            x: set.create_action("ready", "Ready", &[])?,
            y: set.create_action("melee", "Melee", &[])?,
            left_click: set.create_action("stim", "Stim", &[])?,
            right_click: set.create_action("hints", "Hints", &[])?,
            menu: set.create_action("menu", "Menu", &[])?,
            point: set.create_action("point", "Gun hand, pointing", &[])?,
            grip: set.create_action("grip", "Gun hand", &[])?,
            off: set.create_action("off", "Off hand", &[])?,
            set,
        })
    };
    match made() {
        Ok(actions) => commands.insert_resource(actions),
        Err(error) => warn!("VR: no controller actions: {error}"),
    }
}

fn suggest(actions: Res<Actions>, mut bindings: MessageWriter<OxrSuggestActionBinding>) {
    let a = &*actions;
    let touch: Vec<(openxr::sys::Action, &str)> = vec![
        (a.left.as_raw(), "/user/hand/left/input/thumbstick"),
        (a.right.as_raw(), "/user/hand/right/input/thumbstick"),
        (a.fire.as_raw(), "/user/hand/right/input/trigger/value"),
        (a.aim.as_raw(), "/user/hand/left/input/trigger/value"),
        (a.use_.as_raw(), "/user/hand/right/input/squeeze/value"),
        (a.carry.as_raw(), "/user/hand/left/input/squeeze/value"),
        (a.a.as_raw(), "/user/hand/right/input/a/click"),
        (a.b.as_raw(), "/user/hand/right/input/b/click"),
        (a.x.as_raw(), "/user/hand/left/input/x/click"),
        (a.y.as_raw(), "/user/hand/left/input/y/click"),
        (a.left_click.as_raw(), "/user/hand/left/input/thumbstick/click"),
        (a.right_click.as_raw(), "/user/hand/right/input/thumbstick/click"),
        (a.menu.as_raw(), "/user/hand/left/input/menu/click"),
        (a.point.as_raw(), "/user/hand/right/input/aim/pose"),
        (a.grip.as_raw(), "/user/hand/right/input/grip/pose"),
        (a.off.as_raw(), "/user/hand/left/input/grip/pose"),
    ];
    for (action, path) in touch {
        bindings.write(OxrSuggestActionBinding { action, interaction_profile: "/interaction_profiles/oculus/touch_controller".into(), bindings: vec![path.into()] });
    }
    // The plainest controller any runtime has (and what a runtime maps a controller it knows nothing better for
    // to): a trigger and a menu button a hand, enough to look, fire and reach the menu.
    for (action, path) in [
        (a.fire.as_raw(), "/user/hand/right/input/select/click"),
        (a.menu.as_raw(), "/user/hand/left/input/menu/click"),
        (a.point.as_raw(), "/user/hand/right/input/aim/pose"),
        (a.grip.as_raw(), "/user/hand/right/input/grip/pose"),
        (a.off.as_raw(), "/user/hand/left/input/grip/pose"),
    ] {
        bindings.write(OxrSuggestActionBinding { action, interaction_profile: "/interaction_profiles/khr/simple_controller".into(), bindings: vec![path.into()] });
    }
}

fn attach(actions: Res<Actions>, session: Res<OxrSession>, mut attach: MessageWriter<OxrAttachActionSet>, mut commands: Commands) {
    attach.write(OxrAttachActionSet(actions.set.clone()));
    for (action, hand) in [(&actions.point, Hand::Point), (&actions.grip, Hand::Grip), (&actions.off, Hand::Off)] {
        match session.create_action_space(action, openxr::Path::NULL, Isometry3d::IDENTITY) {
            Ok(space) => {
                commands.spawn((Name::new(format!("VR hand: {hand:?}")), space, hand));
            }
            Err(error) => warn!("VR: no space for the {hand:?} hand: {error}"),
        }
    }
}

fn sync(actions: Res<Actions>, mut sync: MessageWriter<OxrSyncActionSet>) {
    sync.write(OxrSyncActionSet(actions.set.clone()));
}

/// Reads the controllers into the touch pad's frame, turns the room, points the gun and presses Esc for the menu
/// button.
#[allow(clippy::too_many_arguments)]
pub fn read(
    actions: Res<Actions>,
    session: Res<OxrSession>,
    views: Res<OxrViews>,
    mut stage: ResMut<super::Stage>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    hands: Query<(&Hand, &Transform, &XrSpaceLocationFlags)>,
    mut flick: Local<Flick>,
    mut menu: Local<bool>,
    mut click: Local<bool>,
) {
    let none = openxr::Path::NULL;
    let stick = |action: &openxr::Action<openxr::Vector2f>| action.state(&session, none).map_or(Vec2::ZERO, |s| Vec2::new(s.current_state.x, s.current_state.y));
    let pull = |action: &openxr::Action<f32>, held: bool| action.state(&session, none).is_ok_and(|s| s.current_state > if held { 0.3 } else { 0.55 });
    let button = |action: &openxr::Action<bool>| action.state(&session, none).is_ok_and(|s| s.current_state);
    let was = flick.down;
    let mut raw = Raw {
        left: stick(&actions.left),
        right: stick(&actions.right),
        fire: pull(&actions.fire, was & touch::FIRE != 0),
        aim: pull(&actions.aim, was & touch::SCOPE != 0),
        use_: pull(&actions.use_, was & touch::USE != 0),
        carry: pull(&actions.carry, was & touch::CARRY != 0),
        a: button(&actions.a),
        b: button(&actions.b),
        x: button(&actions.x),
        y: button(&actions.y),
        left_click: button(&actions.left_click),
        right_click: button(&actions.right_click),
    };
    // Pointing at the screen (screen.rs), the trigger is the left mouse button; one pressed there is let go there.
    let pointing = raw.fire && (stage.pointing || *click);
    if pointing != *click {
        if pointing { mouse.press(MouseButton::Left) } else { mouse.release(MouseButton::Left) }
        *click = pointing;
    }
    if *click || stage.pointing {
        raw.fire = false;
    }
    // The menu button is the Esc key, for as long as it is held.
    let pressed = button(&actions.menu);
    if pressed && !*menu {
        keys.press(KeyCode::Escape);
    } else if !pressed && *menu {
        keys.release(KeyCode::Escape);
    }
    *menu = pressed;
    // The gun points where the right hand points; a hand out of sight leaves it to the head.
    let room = Quat::from_rotation_y(stage.turn);
    let head = super::head(&views);
    let pointed = hands.iter().find(|(hand, _, flags)| **hand == Hand::Point && flags.rotation_tracked).map(|(_, place, _)| room * (place.rotation * Vec3::NEG_Z));
    let faced = head.map(|(_, yaw, pitch)| (stage.turn + yaw, pitch));
    let aim = pointed.map(|forward| ((-forward.x).atan2(-forward.z), forward.y.clamp(-1., 1.).asin())).or(faced);
    let walk = match (aim, faced) {
        (Some((aim, _)), Some((faced, _))) => faced - aim,
        _ => 0.,
    };
    let (frame, turn) = frame(&raw, &mut flick, walk);
    stage.turn = (stage.turn + turn).rem_euclid(std::f32::consts::TAU);
    stage.aim = aim.map(|(yaw, pitch)| (yaw + turn, pitch));
    stage.faced = faced.map(|(yaw, _)| yaw + turn);
    if let Ok(mut slot) = FRAME.lock() {
        *slot = Some(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(frame: [f32; 9]) -> (u32, u32) {
        (frame[4] as u32, frame[5] as u32)
    }

    #[test]
    fn the_buttons_press_the_touch_pads_buttons_once() {
        let mut flick = Flick::default();
        let raw = Raw { fire: true, a: true, ..default() };
        let (first, _) = frame(&raw, &mut flick, 0.);
        assert_eq!(bits(first), (touch::FIRE | touch::JUMP | touch::ROUND, touch::FIRE | touch::JUMP | touch::ROUND));
        let (held, _) = frame(&raw, &mut flick, 0.);
        assert_eq!(bits(held), (touch::FIRE | touch::JUMP | touch::ROUND, 0), "held is not pressed again");
        let (let_go, _) = frame(&Raw::default(), &mut flick, 0.);
        assert_eq!(bits(let_go), (0, 0));
    }

    #[test]
    fn a_flick_of_the_right_stick_turns_the_room_once_and_swaps_once() {
        let mut flick = Flick::default();
        let right = |x, y| Raw { right: Vec2::new(x, y), ..default() };
        assert_eq!(frame(&right(0.9, 0.), &mut flick, 0.).1, -TURN, "to the right is clockwise");
        assert_eq!(frame(&right(1., 0.), &mut flick, 0.).1, 0., "held, it has turned");
        assert_eq!(frame(&right(0.5, 0.), &mut flick, 0.).1, 0., "easing off is not letting go");
        assert_eq!(frame(&right(0., 0.), &mut flick, 0.).1, 0.);
        assert_eq!(frame(&right(-0.9, 0.), &mut flick, 0.).1, TURN);
        frame(&right(0., 0.), &mut flick, 0.);
        let (up, turn) = frame(&right(0., 0.9), &mut flick, 0.);
        assert_eq!((bits(up), turn), ((touch::SWAP, touch::SWAP), 0.));
        assert_eq!(bits(frame(&right(0., 0.9), &mut flick, 0.).0).1, 0);
        frame(&right(0., 0.), &mut flick, 0.);
        assert_eq!(bits(frame(&right(0., -0.9), &mut flick, 0.).0), (touch::PRONE, touch::PRONE));
    }

    #[test]
    fn the_stick_walks_the_way_the_head_faces() {
        let near = |a: [f32; 9], x: f32, y: f32| (a[0] - x).abs() < 1e-5 && (a[1] - y).abs() < 1e-5;
        let forward = Raw { left: Vec2::new(0., 1.), ..default() };
        // Head and gun the same way: forward is forward (the pad's y is back).
        assert!(near(frame(&forward, &mut Flick::default(), 0.).0, 0., -1.));
        // The head a quarter turn to the left of the gun: forward for the head is the gun's left.
        assert!(near(frame(&forward, &mut Flick::default(), std::f32::consts::FRAC_PI_2).0, -1., 0.));
        // A stick at rest does not creep.
        assert!(near(frame(&Raw { left: Vec2::new(0.1, 0.05), ..default() }, &mut Flick::default(), 0.).0, 0., 0.));
    }
}

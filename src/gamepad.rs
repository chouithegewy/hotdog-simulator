// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! A gamepad (an Xbox controller, by cable or Bluetooth, or any pad the system knows as one), and the choice of what
//! the game is played with.
//!
//! The pad is read as the page's touch controls are (touch.rs): a stick to walk, a look, and buttons that press
//! their actions' keys, so every mode's buttons are the ones a phone has.
//!
//! | Control | Does |
//! |---|---|
//! | Left stick | Walk |
//! | Right stick | Look |
//! | Right trigger / left trigger | Fire (throw, punch) / aim (the scope; the heavy blow, the smash) |
//! | Right bumper / left bumper | Use / carry (block) |
//! | A / B | Jump, and the next round / crouch (held) |
//! | X / Y | Ready (reload) / swap weapon |
//! | Left stick pressed / right stick pressed | Stim (dodge, charge) / melee (kick, stomp) |
//! | D-pad up / down / left / right | Hints / prone / primary / secondary |
//! | Start | The menu (Esc), which the mouse or a finger works |
//!
//! **The choice** ([`Scheme`], the settings' CONTROLLER, kept as `controls`): AUTO plays with whatever is used (the
//! pad from the moment it is touched, the keys and the mouse from the moment they are), KEYBOARD + MOUSE never
//! listens to a pad, GAMEPAD always does. A phone's touch controls are the page's own and a headset's controllers
//! its own (vr/hands.rs): both are there whenever the device is.
use std::sync::Mutex;

use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::prelude::*;

use crate::touch;

/// What the settings keep the choice under.
pub const KEY: &str = "controls";
/// A stick under this is at rest.
const DEAD: f32 = 0.16;
/// The right stick pushed all the way turns the view this fast, in the mouse counts a second that `arena::tick`
/// turns by (0.0022 radians each: about 150 degrees a second).
const LOOK: f32 = 1200.;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    #[default]
    Auto,
    Keys,
    Pad,
}
impl Scheme {
    pub fn name(self) -> &'static str {
        match self {
            Scheme::Auto => "AUTO",
            Scheme::Keys => "KEYBOARD + MOUSE",
            Scheme::Pad => "GAMEPAD",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Scheme::Auto => "auto",
            Scheme::Keys => "keys",
            Scheme::Pad => "pad",
        }
    }
    pub fn parse(text: &str) -> Self {
        match text {
            "keys" => Scheme::Keys,
            "pad" => Scheme::Pad,
            _ => Scheme::Auto,
        }
    }
    pub fn step(self, by: i32) -> Self {
        const ALL: [Scheme; 3] = [Scheme::Auto, Scheme::Keys, Scheme::Pad];
        let at = ALL.iter().position(|s| *s == self).unwrap_or(0) as i32;
        ALL[(at + by).rem_euclid(3) as usize]
    }
}

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Scheme>()
            .add_systems(Startup, load.after(crate::controls::load))
            .add_systems(PreUpdate, read.after(bevy::input::InputSystems).before(touch::press).before(crate::controls::Shut))
            .add_systems(Update, remember);
    }
}

fn load(mut scheme: ResMut<Scheme>, prefs: Res<crate::prefs::Prefs>) {
    *scheme = Scheme::parse(&prefs.get(KEY).unwrap_or_default());
}
fn remember(scheme: Res<Scheme>, mut prefs: ResMut<crate::prefs::Prefs>) {
    if scheme.is_changed() && !scheme.is_added() && prefs.get(KEY).as_deref() != Some(scheme.key()) {
        prefs.set(KEY, scheme.key());
    }
}

/// A pad this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Raw {
    pub left: Vec2,
    pub right: Vec2,
    /// The buttons held, as the touch pad's bits.
    pub down: u32,
    pub start: bool,
}
impl Raw {
    /// Whether anything of it is touched.
    pub fn busy(&self) -> bool {
        self.left.length() > DEAD || self.right.length() > DEAD || self.down != 0 || self.start
    }
}

/// A stick with its rest taken out and its push eased (fine near the middle, full at the edge).
fn eased(stick: Vec2) -> Vec2 {
    let push = stick.length();
    if push <= DEAD {
        return Vec2::ZERO;
    }
    let t = ((push - DEAD) / (1. - DEAD)).min(1.);
    stick / push * t * t.sqrt()
}

/// One frame of the touch pad from a pad (touch.rs, `Frame`: stick x and y, look x and y, the buttons down, the
/// buttons pressed since the last frame, cursor x and y, pinch): `was` are the buttons held last frame, `dt` the
/// frame's seconds.
pub fn frame(raw: &Raw, was: u32, dt: f32) -> [f32; 9] {
    let walk = eased(raw.left);
    let look = eased(raw.right) * LOOK * dt;
    // (The pad's sticks are up for forward; the touch pad's y is back, and a mouse's y is down.)
    [walk.x, -walk.y, look.x, -look.y, raw.down as f32, (raw.down & !was) as f32, 0., 0., 0.]
}

static FRAME: Mutex<Option<[f32; 9]>> = Mutex::new(None);
/// This frame of the pad for touch.rs (nothing unless the pad is what the game is played with), taken once.
pub fn take() -> Vec<f32> {
    FRAME.lock().ok().and_then(|mut frame| frame.take()).map_or_else(Vec::new, |frame| frame.to_vec())
}

/// What the pad holds.
fn raw(pad: &Gamepad) -> Raw {
    let mut down = 0;
    for (button, bit) in [
        (GamepadButton::RightTrigger2, touch::FIRE),
        (GamepadButton::LeftTrigger2, touch::SCOPE),
        (GamepadButton::RightTrigger, touch::USE),
        (GamepadButton::LeftTrigger, touch::CARRY),
        (GamepadButton::South, touch::JUMP | touch::ROUND),
        (GamepadButton::East, touch::CROUCH),
        (GamepadButton::West, touch::READY),
        (GamepadButton::North, touch::SWAP),
        (GamepadButton::LeftThumb, touch::STIM),
        (GamepadButton::RightThumb, touch::MELEE),
        (GamepadButton::DPadUp, touch::HINTS),
        (GamepadButton::DPadDown, touch::PRONE),
        (GamepadButton::DPadLeft, touch::PRIMARY),
        (GamepadButton::DPadRight, touch::SECONDARY),
    ] {
        if pad.pressed(button) || pad.get(button).is_some_and(|pull| pull > 0.5) {
            down |= bit;
        }
    }
    Raw { left: pad.left_stick(), right: pad.right_stick(), down, start: pad.pressed(GamepadButton::Start) }
}

/// Reads the first pad that is touched into the touch pad's frame, and presses Esc for Start. In AUTO the pad plays
/// from the moment it is touched until a key or a mouse button is.
fn read(
    scheme: Res<Scheme>,
    pads: Query<&Gamepad>,
    time: Res<Time<Real>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut playing: Local<bool>,
    mut was: Local<u32>,
    mut start: Local<bool>,
) {
    let raw = pads.iter().map(raw).find(Raw::busy).unwrap_or_default();
    *playing = match *scheme {
        Scheme::Keys => false,
        Scheme::Pad => !pads.is_empty(),
        // (Esc is the pad's own Start.)
        Scheme::Auto => raw.busy() || *playing && !pads.is_empty() && !mouse.get_just_pressed().next().is_some() && !keys.get_just_pressed().any(|key| *key != KeyCode::Escape),
    };
    if raw.start && !*start && *playing {
        keys.press(KeyCode::Escape);
    } else if !raw.start && *start {
        keys.release(KeyCode::Escape);
    }
    *start = raw.start;
    if !*playing {
        *was = 0;
        return;
    }
    if let Ok(mut slot) = FRAME.lock() {
        *slot = Some(frame(&raw, *was, time.delta_secs().min(0.1)));
    }
    *was = raw.down;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sticks_walk_and_look_and_a_stick_at_rest_does_neither() {
        let still = frame(&Raw { left: Vec2::new(0.1, -0.05), right: Vec2::new(0.02, 0.1), ..default() }, 0, 0.016);
        assert_eq!(&still[..4], &[0., 0., 0., 0.]);
        // Forward and to the right, all the way: the touch pad's y is back.
        let f = frame(&Raw { left: Vec2::new(0., 1.), right: Vec2::new(1., 0.), ..default() }, 0, 0.5);
        assert!(f[0].abs() < 1e-6 && (f[1] + 1.).abs() < 1e-6, "{f:?}");
        assert!((f[2] - LOOK * 0.5).abs() < 1e-3 && f[3].abs() < 1e-6, "half a second of a full turn");
        // Looking up is the mouse moving up: a negative y.
        assert!(frame(&Raw { right: Vec2::new(0., 1.), ..default() }, 0, 0.1)[3] < 0.);
        // Half a push is less than half the pace: fine aim near the middle.
        let half = frame(&Raw { right: Vec2::new(0.5, 0.), ..default() }, 0, 1.)[2];
        assert!(half > 0. && half < LOOK * 0.4, "{half}");
    }

    #[test]
    fn a_button_is_pressed_once_and_held() {
        let raw = Raw { down: touch::FIRE | touch::JUMP, ..default() };
        let first = frame(&raw, 0, 0.016);
        assert_eq!((first[4] as u32, first[5] as u32), (touch::FIRE | touch::JUMP, touch::FIRE | touch::JUMP));
        let held = frame(&raw, raw.down, 0.016);
        assert_eq!((held[4] as u32, held[5] as u32), (touch::FIRE | touch::JUMP, 0));
    }

    #[test]
    fn the_choice_goes_round_and_is_kept_by_its_key() {
        assert_eq!(Scheme::Auto.step(1), Scheme::Keys);
        assert_eq!(Scheme::Auto.step(-1), Scheme::Pad);
        for scheme in [Scheme::Auto, Scheme::Keys, Scheme::Pad] {
            assert_eq!(Scheme::parse(scheme.key()), scheme);
        }
        assert_eq!(Scheme::parse("anything else"), Scheme::Auto);
    }
}

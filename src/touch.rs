// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Touch controls. On a phone or a tablet the page lays a stick, a look pad and buttons over the game
//! (touch.js) and keeps what they are doing in `window.hotdogTouch`. Once a frame, before anything reads the
//! keyboard or the mouse, [`press`] reads it:
//!
//! * each button presses and releases the key it stands for (the scope the right mouse button) in Bevy's own
//!   `ButtonInput`s, so that everything that reads them (the round, the scope, a dead player's next view, a
//!   new round) plays by touch with no code of its own;
//! * what a key cannot carry is kept in [`Pad`] for `arena::tick`: how far the stick is pushed (it walks at
//!   that share of the pace), how far the view was dragged and whether the trigger is held (the left mouse
//!   button would capture the pointer), and for the lobby a cursor that a drag moves.
//! * at the card table (Fully Loaded) the page shows two buttons, Flip and Stop: the trigger (FIRE) flips, or
//!   picks the hotdog looked at while this player picks who gets an action card, and the scope (SCOPE, the right
//!   button) stops, or, while it picks, reads Me and picks this player itself; the drag looks round from the
//!   seat. cards.rs reads them (`play`, `sit`).
//! * dead, a drag takes the death camera round, a pinch brings it nearer or takes it further off ([`Pad::pinch`]),
//!   and the trigger (FIRE, which the page then calls Next) shows the next hotdog. deathcam.rs reads them (`hand`).
//!
//! * in Kung Fu ([`can::FIGHT`]) the page labels the buttons for the fight and each presses its Kung Fu action's key
//!   instead ([`FU_KEYS`]): Fire strikes light (held), Scope heavy, Melee kicks, Carry blocks (held), Use grabs, Stim
//!   dodges, Prone is the special (lit when the chi is full, [`can::CHI`]), and Swap takes the next style
//!   ([`Pad::next_style`]: the wheel has no key to press).
//! * as the titan ([`can::GIANT`], Titan mode) the page labels them for the giant and each presses its titan action's
//!   key ([`TITAN_KEYS`]): Fire swats (held), Scope smashes, Melee stomps, Stim charges (dim while it cools down,
//!   [`can::STIM_WAIT`]); Jump and Crouch are everyone's.
//!
//! In return the page is told what this player can do now ([`state`], a handful of bits, sent when they
//! change), so that it shows only the buttons that do something.
use crate::arena::{slots, Game, Weapon};
use crate::controls::{Action, Bindings, Input};
use bevy::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pad>().add_systems(PreUpdate, press.after(bevy::input::InputSystems));
    }
}

/// The page's buttons, as bits (touch.js has the same numbers). FIRE holds the trigger; the rest press keys.
pub const FIRE: u32 = 1;
pub const JUMP: u32 = 1 << 1;
pub const MELEE: u32 = 1 << 2;
pub const CROUCH: u32 = 1 << 3;
pub const PRONE: u32 = 1 << 4;
pub const STIM: u32 = 1 << 5;
pub const SWAP: u32 = 1 << 6;
pub const PRIMARY: u32 = 1 << 7;
pub const SECONDARY: u32 = 1 << 8;
pub const USE: u32 = 1 << 9;
pub const CARRY: u32 = 1 << 10;
pub const READY: u32 = 1 << 11;
pub const SCOPE: u32 = 1 << 12;
pub const ROUND: u32 = 1 << 13;
/// Shows or hides the hints (H), which a touch screen has no key for.
pub const HINTS: u32 = 1 << 14;
const BUTTONS: u32 = (1 << 15) - 1;

/// What each button presses: the action it stands for (controls.rs), by the key of the README's controls while that
/// action has its default keys (crouch's C, held while the page has it on), or by the action's own key or button once
/// it is rebound, so that the buttons go on working whatever the keyboard's are.
const KEYS: [(u32, Action, Input); 14] = [
    (JUMP, Action::Jump, Input::Key(KeyCode::Space)),
    (MELEE, Action::Melee, Input::Key(KeyCode::KeyF)),
    (CROUCH, Action::Crouch, Input::Key(KeyCode::KeyC)),
    (PRONE, Action::Prone, Input::Key(KeyCode::KeyT)),
    (STIM, Action::Stim, Input::Key(KeyCode::ShiftLeft)),
    (SWAP, Action::Swap, Input::Key(KeyCode::KeyY)),
    (PRIMARY, Action::Primary, Input::Key(KeyCode::Digit1)),
    (SECONDARY, Action::Secondary, Input::Key(KeyCode::Digit2)),
    (USE, Action::Use, Input::Key(KeyCode::KeyE)),
    (CARRY, Action::Carry, Input::Key(KeyCode::KeyQ)),
    (READY, Action::Ready, Input::Key(KeyCode::KeyR)),
    (SCOPE, Action::Aim, Input::Mouse(MouseButton::Right)),
    (ROUND, Action::NewRound, Input::Key(KeyCode::Enter)),
    (HINTS, Action::Hints, Input::Key(KeyCode::KeyH)),
];

/// Kung Fu: the buttons that press another action's key in a fight, and its default key (`KEYS` has the rest). Swap
/// takes the next style through [`Pad::next_style`].
const FU_KEYS: [(u32, Action, Input); 6] = [
    (MELEE, Action::Kick, Input::Key(KeyCode::KeyF)),
    (CARRY, Action::Block, Input::Key(KeyCode::KeyQ)),
    (USE, Action::Grab, Input::Key(KeyCode::KeyE)),
    (STIM, Action::Dodge, Input::Key(KeyCode::ShiftLeft)),
    (SCOPE, Action::Heavy, Input::Mouse(MouseButton::Right)),
    (PRONE, Action::Special, Input::Key(KeyCode::KeyT)),
];

/// Titan: the buttons that press the giant's action's key while this player is the titan, and its default key.
const TITAN_KEYS: [(u32, Action, Input); 3] = [
    (MELEE, Action::Stomp, Input::Key(KeyCode::KeyF)),
    (STIM, Action::Charge, Input::Key(KeyCode::ShiftLeft)),
    (SCOPE, Action::Smash, Input::Mouse(MouseButton::Right)),
];

/// What the page is told this player can do, as bits (touch.js has the same numbers).
pub mod can {
    /// In a round with a hotdog of its own to play (not the demo, a cut scene or the pause between rounds).
    pub const PLAY: u32 = 1;
    pub const ALIVE: u32 = 1 << 1;
    /// Forts: the build phase (R says ready, the guns are locked), and whether this player has said ready.
    pub const BUILD: u32 = 1 << 2;
    pub const READY: u32 = 1 << 3;
    /// A gun in hand that the right button aims (the ketchup sniper's scope, or the syrup rifle up to the eye), and
    /// whether it is aimed. (The page calls both its Scope button.)
    pub const SNIPER: u32 = 1 << 4;
    pub const SCOPED: u32 = 1 << 5;
    /// A loose object in its hands (the trigger throws it, Q puts it down), or the bacon (no weapon at all).
    pub const CARRY: u32 = 1 << 6;
    pub const BACON: u32 = 1 << 7;
    /// A loose object (or a draped wall it may pull) within reach: Q picks it up.
    pub const GRASP: u32 = 1 << 8;
    /// A weapon or ammunition pickup within reach: E collects it.
    pub const PICKUP: u32 = 1 << 9;
    /// The siege's famine (not Free For All, which has no bite and no eating), and a dead hotdog within reach: E eats it
    /// (and F bites the living).
    pub const FAMINE: u32 = 1 << 10;
    pub const EAT: u32 = 1 << 11;
    pub const PRONE: u32 = 1 << 12;
    pub const CROUCH: u32 = 1 << 13;
    /// Its guns: one in slot 1, one in slot 2, and the second slot's in hand.
    pub const PRIMARY: u32 = 1 << 14;
    pub const SECONDARY: u32 = 1 << 15;
    pub const SECOND: u32 = 1 << 16;
    /// The stim running, or cooling down.
    pub const STIM: u32 = 1 << 17;
    pub const STIM_WAIT: u32 = 1 << 18;
    /// The round is over and Enter starts the next (single player).
    pub const OVER: u32 = 1 << 19;
    /// Dead, with a living hotdog to watch (in Forts a team mate: deathcam.rs `watchable`): the trigger shows the next.
    pub const WATCH: u32 = 1 << 20;
    /// Multiplayer: in the fridge between rounds.
    pub const LOBBY: u32 = 1 << 21;
    /// At the card table (Fully Loaded, cards.rs): the trigger flips and the scope stops, at this player's go (GO), and
    /// while it picks who gets an action card it drew (PICK) the trigger picks the hotdog it looks at and the scope
    /// (Me) picks the player itself.
    pub const TABLE: u32 = 1 << 22;
    pub const GO: u32 = 1 << 23;
    pub const PICK: u32 = 1 << 24;
    /// The hints can be shown (a round this player plays: hints.rs `wanted`), and whether they are showing: the page's
    /// Hints button hides them or shows them again.
    pub const HINTS: u32 = 1 << 25;
    pub const HINTS_ON: u32 = 1 << 26;
    /// An electric griddle's dial within reach, and faced: E switches it (griddle.rs).
    pub const SWITCH: u32 = 1 << 27;
    /// Kung Fu: the buttons are the fight's (Punch, Heavy, Kick, Block, Grab, Dodge, Special, Style); and the chi is
    /// full, so the special is ready.
    pub const FIGHT: u32 = 1 << 28;
    pub const CHI: u32 = 1 << 29;
    /// A replay is watched (replay/): none of the game's buttons, the viewer's own controls instead (replay/page.rs); a
    /// drag looks round, two fingers fly the free camera (the stick) and pinch.
    pub const REPLAY: u32 = 1 << 30;
    /// Titan: this player is the giant: the buttons are its moves (Swat, Smash, Stomp, Charge, Jump, Crouch).
    pub const GIANT: u32 = 1 << 31;
}

/// This frame's touch input that the keys cannot carry, for `arena::tick` and the lobby.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub struct Pad {
    /// Whether the page's touch controls are showing.
    pub on: bool,
    /// The stick: x to the right, y back (as S is), at most 1 long; its length is the share of the pace.
    pub stick: Vec2,
    /// How far the view was dragged since the last frame, in mouse counts (the page applies the sensitivity).
    pub look: Vec2,
    /// The trigger, held; and pressed since the last frame (a tap may have let go of it again already).
    pub fire: bool,
    pub pulled: bool,
    /// Dead: how far two fingers on the view were pinched apart since the last frame, as the log of how many times
    /// further apart they are (less than nothing: pinched together). Apart brings the death camera nearer.
    pub pinch: f32,
    /// In the lobby: where a drag has moved the view, as the mouse's place would (-1 to 1 across the window).
    pub cursor: Vec2,
    /// Kung Fu: the Style button was pressed since the last frame (the next style).
    pub next_style: bool,
    /// The buttons this pad holds down in the `ButtonInput`s (and what each pressed, to let go of the same), and what
    /// the page was last told.
    held: u32,
    pressed: [Option<Input>; 14],
    told: Option<u32>,
}

/// One frame from the page: [stick x, y, look x, y, buttons down, buttons pressed since the last frame,
/// cursor x, y, pinch] (a page from before the pinch sends eight).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Frame {
    stick: Vec2,
    look: Vec2,
    down: u32,
    taps: u32,
    cursor: Vec2,
    pinch: f32,
}
fn parse(v: &[f32]) -> Option<Frame> {
    if v.len() < 8 || !v.iter().all(|x| x.is_finite()) {
        return None;
    }
    let bits = |x: f32| (x.clamp(0., BUTTONS as f32) as u32) & BUTTONS;
    Some(Frame {
        stick: Vec2::new(v[0], v[1]).clamp_length_max(1.),
        look: Vec2::new(v[2], v[3]).clamp(Vec2::splat(-5000.), Vec2::splat(5000.)),
        down: bits(v[4]),
        taps: bits(v[5]),
        cursor: Vec2::new(v[6], v[7]).clamp(Vec2::NEG_ONE, Vec2::ONE),
        pinch: v.get(8).copied().unwrap_or(0.).clamp(-3., 3.),
    })
}

/// Presses and releases the keys of the buttons with the default bindings (the tests).
#[cfg(test)]
fn apply(down: u32, taps: u32, held: u32, keys: &mut ButtonInput<KeyCode>, mouse: &mut ButtonInput<MouseButton>) -> u32 {
    apply_bound(down, taps, held, &mut [None; 14], &Bindings::default(), false, keys, mouse)
}
/// Presses and releases the keys of the buttons: `down` are held now, `taps` were pressed since the last frame
/// (perhaps let go again already, so that a quick tap is not lost), and `held` are the ones this pad pressed
/// before, with what each pressed (`pressed`). A button pressed again while it was held gives a fresh press. Each
/// presses its action's key as `bindings` have it (`KEYS`; in Kung Fu, `fu`, `FU_KEYS` where it has the button, and
/// Swap nothing). Returns what this pad holds now.
#[allow(clippy::too_many_arguments)]
fn apply_bound(down: u32, taps: u32, held: u32, pressed: &mut [Option<Input>; 14], bindings: &Bindings, fu: bool, keys: &mut ButtonInput<KeyCode>, mouse: &mut ButtonInput<MouseButton>) -> u32 {
    apply_keys(down, taps, held, pressed, bindings, if fu { &FU_KEYS } else { &[] }, fu, keys, mouse)
}
/// `apply_bound` with the buttons that press another action's key (`swaps`: Kung Fu's FU_KEYS, the titan's TITAN_KEYS),
/// and Swap pressing nothing (`fu`: the style goes round by `Pad::next_style`).
#[allow(clippy::too_many_arguments)]
fn apply_keys(down: u32, taps: u32, held: u32, pressed: &mut [Option<Input>; 14], bindings: &Bindings, swaps: &[(u32, Action, Input)], fu: bool, keys: &mut ButtonInput<KeyCode>, mouse: &mut ButtonInput<MouseButton>) -> u32 {
    let mut now = 0;
    for (i, (bit, action, default)) in KEYS.into_iter().enumerate() {
        let (action, default) = match swaps.iter().find(|(b, ..)| *b == bit) {
            Some((_, action, default)) => (*action, *default),
            None => (action, default),
        };
        let (down, taps) = if fu && bit == SWAP { (0, 0) } else { (down, taps) };
        let (on, fresh, ours) = ((down | taps) & bit != 0, taps & bit != 0, held & bit != 0);
        let bound = if bindings.is_default(action) { Some(default) } else { bindings.pressable(action) };
        let Some(input) = pressed[i].filter(|_| ours).or(bound) else {
            pressed[i] = None;
            continue;
        };
        let kept = match input {
            Input::Key(key) => hold(keys, key, on, fresh, ours),
            Input::Mouse(button) => hold(mouse, button, on, fresh, ours),
            Input::Wheel(_) => false,
        };
        pressed[i] = kept.then_some(input);
        if kept {
            now |= bit;
        }
    }
    now
}
fn hold<T: Copy + Eq + std::hash::Hash + Send + Sync + 'static>(input: &mut ButtonInput<T>, key: T, on: bool, fresh: bool, ours: bool) -> bool {
    if on {
        if fresh && ours {
            input.release(key);
        }
        input.press(key);
    } else if ours {
        input.release(key);
    }
    on
}

/// What this player can do now (see [`can`]). At the card table that is the table's two buttons and nothing of the
/// round's, and whether the table waits on this player (`seen`, the table as it is seen: cards.rs).
pub fn state(g: &Game, lobby: bool, seen: Option<&crate::cards::Seen>) -> u32 {
    use can::*;
    if lobby {
        return LOBBY;
    }
    if g.demo || !g.playing() {
        return 0;
    }
    if crate::cards::at_table(g) {
        let (go, pick) = seen.and_then(|seen| Some(crate::cards::asks(seen.view.as_ref()?, seen.me))).unwrap_or_default();
        return TABLE | if go { GO } else { 0 } | if pick { PICK } else { 0 };
    }
    let phase = g.phase_name();
    match phase {
        "RoundOver" => return if g.network { 0 } else { OVER },
        "Rest" => return 0,
        _ => {}
    }
    let me = g.roster.iter().find(|p| p.id == g.me());
    let Some(me) = me.filter(|p| p.alive) else {
        let others = me.is_some_and(|me| !crate::deathcam::watchable(&g.roster, me.id, g.fort.is_some()).is_empty());
        return PLAY | if others { WATCH } else { 0 };
    };
    let mut s = PLAY | ALIVE;
    let flag = |on: bool, bit: u32| if on { bit } else { 0 };
    // Titan: the giant's buttons, and whether its charge is cooling down.
    if let Some(view) = g.titan.as_ref().filter(|v| v.titan != 0 && v.titan == me.id) {
        let cooling = view.ready[kitchen_sim::titan::READY_CHARGE] > g.elapsed + 0.05;
        return s | GIANT | flag(cooling, STIM_WAIT) | flag(g.crouch, CROUCH);
    }
    // Kung Fu: the fight's buttons, and whether the special is ready.
    if let Some(view) = g.kungfu.as_ref() {
        let full = view.fighter(me.id).is_some_and(|f| f.chi >= kitchen_sim::kungfu::CHI_MAX - 1e-3);
        return s | FIGHT | flag(full, CHI) | flag(g.crouch, CROUCH);
    }
    let carry = me.carry >= 0;
    let bacon = g.fort.as_ref().is_some_and(|f| f.bacon.state == kitchen_sim::forts::HELD && f.bacon.holder == g.me());
    s |= flag(phase == "Build", BUILD) | flag(phase == "Build" && me.ready, READY);
    s |= flag(carry, CARRY) | flag(bacon, BACON);
    s |= flag(matches!(g.weapon, Weapon::Sniper | Weapon::Rifle), SNIPER) | flag(g.scoped, SCOPED);
    s |= flag(g.prone, PRONE) | flag(g.crouch && !g.prone, CROUCH);
    let [primary, secondary] = slots(&g.owned);
    s |= flag(primary.is_some(), PRIMARY) | flag(secondary.is_some(), SECONDARY) | flag(secondary.is_some_and(|w| w == g.weapon), SECOND);
    s |= flag(me.stim > 0., STIM) | flag(me.stim <= 0. && me.stim_wait > 0., STIM_WAIT);
    s |= flag(!carry && !bacon && within_reach(g, phase), GRASP);
    let near = g.position + Vec3::Y * 0.04;
    s |= flag(!g.infected && g.spawns().iter().any(|p| p.ready <= g.elapsed && Vec3::from_array(p.p).distance(near) < 0.15), PICKUP);
    s |= flag(g.feast.griddle_near(g.position.to_array(), g.yaw).is_some(), SWITCH);
    if phase == "FreeForAll" && !g.feast.rules.ffa {
        let at = Vec3::from_array(me.p);
        s |= FAMINE | flag(g.roster.iter().any(|p| p.id != me.id && !p.alive && !p.consumed && Vec3::from_array(p.p).distance(at) < 0.17), EAT);
    }
    s
}
/// The page's hints button ([`can::HINTS`]): shown in a round this player plays, reading whether the hints are on.
pub fn hints(g: &Game, hints: Option<&crate::hints::Hints>) -> u32 {
    if !crate::hints::wanted(g) {
        return 0;
    }
    can::HINTS | if hints.is_some_and(|h| h.on) { can::HINTS_ON } else { 0 }
}
/// Whether Q has something to pick up: a loose object about the hotdog's feet or just ahead of it (the
/// simulation's GRASP and NEXT, a little generously), or a draped wall it stands against and may pull.
fn within_reach(g: &Game, phase: &str) -> bool {
    use kitchen_sim::props::State;
    let feet = g.position.as_dvec3();
    let props = &g.feast.props;
    let loose = (0..props.len()).any(|i| {
        let at = props.position(i);
        matches!(props.state(i), State::Awake | State::Asleep | State::Locked) && (at[0] - feet.x).hypot(at[2] - feet.z) < 0.36 && (-0.25..0.4).contains(&(at[1] - feet.y))
    });
    let pull = g.fort.is_none() || phase == "Build";
    loose
        || pull
            && g.feast.drapes.iter().any(|d| {
                let (x, z) = ((d.rect[0] - feet.x).max(0.).max(feet.x - d.rect[2]), (d.rect[1] - feet.z).max(0.).max(feet.z - d.rect[3]));
                x.hypot(z) < 0.35 && feet.y < d.top
            })
}

static ON: AtomicBool = AtomicBool::new(false);
/// Whether the page's touch controls are showing (the head-up display leaves the keys out then).
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Reads the page's touch controls, presses their keys and tells the page what can be done (PreUpdate, after
/// Bevy has read the keyboard and the mouse).
pub(crate) fn press(
    mut pad: ResMut<Pad>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    game: Option<Res<Game>>,
    lobby: Option<Res<crate::lobby::Lobby>>,
    seen: Option<Res<crate::cards::Seen>>,
    hinted: Option<Res<crate::hints::Hints>>,
    bindings: Option<Res<Bindings>>,
    viewer: Option<Res<crate::replay::Viewer>>,
) {
    let frame = parse(&read());
    ON.store(frame.is_some(), Ordering::Relaxed);
    if frame.is_none() && !pad.on {
        return;
    }
    let f = frame.unwrap_or_default();
    let fu = game.as_deref().is_some_and(Game::kung_fu);
    if pad.held != 0 || f.down != 0 || f.taps != 0 {
        let bindings = bindings.map_or_else(Bindings::default, |b| b.clone());
        let (held, mut pressed) = (pad.held, pad.pressed);
        // (As the titan its moves' keys: TITAN_KEYS.)
        let giant = game.as_deref().is_some_and(|g| crate::titan::titan_id(g) == Some(g.me()));
        pad.held = if giant { apply_keys(f.down, f.taps, held, &mut pressed, &bindings, &TITAN_KEYS, false, &mut keys, &mut mouse) } else { apply_bound(f.down, f.taps, held, &mut pressed, &bindings, fu, &mut keys, &mut mouse) };
        pad.pressed = pressed;
    }
    pad.next_style = fu && f.taps & SWAP != 0;
    (pad.on, pad.stick, pad.look, pad.fire, pad.pulled, pad.cursor, pad.pinch) = (frame.is_some(), f.stick, f.look, (f.down | f.taps) & FIRE != 0, f.taps & FIRE != 0, f.cursor, f.pinch);
    if frame.is_none() {
        pad.told = None;
        return;
    }
    // (Watching a replay the watched hotdog is drawn as this player's own, but there is nothing of it to play.)
    let now = if viewer.is_some() { can::REPLAY } else { game.as_deref().map_or(0, |g| state(g, lobby.is_some_and(|l| l.holding), seen.as_deref()) | hints(g, hinted.as_deref())) };
    if pad.told != Some(now) {
        pad.told = Some(now);
        tell(now);
    }
}

/// This frame's touch input, for `arena::tick` (nothing without the touch controls).
pub fn now(world: &World) -> Pad {
    world.get_resource::<Pad>().copied().filter(|pad| pad.on).unwrap_or_default()
}
/// In the lobby, where a drag has turned the view, as the mouse's place across the window (-1 to 1); none
/// without the touch controls.
pub fn cursor(world: &World) -> Option<Vec2> {
    world.get_resource::<Pad>().filter(|pad| pad.on).map(|pad| pad.cursor)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function touch_read(){const t=window.hotdogTouch;if(!t||!t.on)return new Float32Array(0);const v=new Float32Array([t.x,t.y,t.lx,t.ly,t.down,t.taps,t.cx,t.cy,t.pz||0]);t.lx=0;t.ly=0;t.taps=0;t.pz=0;return v}
export function touch_tell(state){const t=window.hotdogTouch;if(t&&typeof t.tell==='function')t.tell(state)}
")]
extern "C" {
    fn touch_read() -> Vec<f32>;
    fn touch_tell(state: u32);
}
fn read() -> Vec<f32> {
    #[cfg(target_arch = "wasm32")]
    return touch_read();
    // Virtual reality: the controllers are the pad (vr/hands.rs).
    #[cfg(feature = "vr")]
    return crate::vr::hands::take();
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "vr")))]
    Vec::new()
}
fn tell(state: u32) {
    #[cfg(target_arch = "wasm32")]
    touch_tell(state);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = state;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(down: u32, taps: u32, held: u32, keys: &mut ButtonInput<KeyCode>, mouse: &mut ButtonInput<MouseButton>) -> u32 {
        // As a new frame does: Bevy clears the edges before it reads the devices.
        keys.clear();
        mouse.clear();
        apply(down, taps, held, keys, mouse)
    }

    #[test]
    fn a_held_button_presses_its_key_once_and_lets_go() {
        let (mut keys, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        let held = frame(JUMP, JUMP, 0, &mut keys, &mut mouse);
        assert!(keys.just_pressed(KeyCode::Space) && keys.pressed(KeyCode::Space));
        let held = frame(JUMP, 0, held, &mut keys, &mut mouse);
        assert!(keys.pressed(KeyCode::Space) && !keys.just_pressed(KeyCode::Space), "held, with no second press");
        let held = frame(0, 0, held, &mut keys, &mut mouse);
        assert!(!keys.pressed(KeyCode::Space) && keys.just_released(KeyCode::Space));
        assert_eq!(held, 0);
    }

    #[test]
    fn a_tap_inside_one_frame_is_not_lost() {
        let (mut keys, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        // Pressed and let go between two frames: down is empty, but the tap is there.
        let held = frame(0, USE, 0, &mut keys, &mut mouse);
        assert!(keys.just_pressed(KeyCode::KeyE));
        frame(0, 0, held, &mut keys, &mut mouse);
        assert!(!keys.pressed(KeyCode::KeyE));
    }

    #[test]
    fn pressing_again_while_held_is_a_fresh_press() {
        let (mut keys, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        let held = frame(SWAP, SWAP, 0, &mut keys, &mut mouse);
        let held = frame(SWAP, SWAP, held, &mut keys, &mut mouse);
        assert!(keys.just_pressed(KeyCode::KeyY), "let go and pressed again between frames");
        assert_eq!(held, SWAP);
    }

    #[test]
    fn the_scope_is_the_right_mouse_button_and_the_trigger_no_button() {
        let (mut keys, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        let held = frame(FIRE | SCOPE, FIRE | SCOPE, 0, &mut keys, &mut mouse);
        assert!(mouse.just_pressed(MouseButton::Right));
        assert!(!mouse.pressed(MouseButton::Left), "the left button would capture the pointer");
        assert_eq!(held, SCOPE);
    }

    /// Rebound, a button presses its action's new key (so it goes on working), and lets go of the one it pressed.
    #[test]
    fn a_rebound_action_s_button_presses_its_new_key() {
        let (mut keys, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        let mut bindings = Bindings::default();
        bindings.assign(Action::Jump, 0, Input::Key(KeyCode::KeyJ)).unwrap();
        bindings.assign(Action::Aim, 0, Input::Key(KeyCode::KeyZ)).unwrap();
        let mut pressed = [None; 14];
        let held = apply_bound(JUMP | SCOPE, JUMP | SCOPE, 0, &mut pressed, &bindings, false, &mut keys, &mut mouse);
        assert!(keys.just_pressed(KeyCode::KeyJ) && keys.just_pressed(KeyCode::KeyZ) && !keys.pressed(KeyCode::Space) && !mouse.pressed(MouseButton::Right));
        assert!(bindings.held(Action::Jump, &keys, &mouse));
        // Rebound again while held: the button still lets go of what it pressed.
        bindings.assign(Action::Jump, 0, Input::Key(KeyCode::KeyK)).unwrap();
        keys.clear();
        apply_bound(0, 0, held, &mut pressed, &bindings, false, &mut keys, &mut mouse);
        assert!(!keys.pressed(KeyCode::KeyJ) && !keys.pressed(KeyCode::KeyZ));
    }

    /// Kung Fu: the buttons press the fight's actions' keys (rebound, their new ones), Swap presses nothing (it is the
    /// next style, `Pad::next_style`), and the page is told to show the fight's buttons, the special lit at full chi.
    #[test]
    fn in_kung_fu_the_buttons_press_the_fights_keys_and_the_page_shows_them() {
        let (mut keys, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        let mut bindings = Bindings::default();
        bindings.assign(Action::Kick, 0, Input::Key(KeyCode::KeyG)).unwrap();
        bindings.assign(Action::Block, 0, Input::Key(KeyCode::KeyB)).unwrap();
        let mut pressed = [None; 14];
        apply_bound(MELEE | CARRY | SWAP | PRONE, MELEE | CARRY | SWAP | PRONE, 0, &mut pressed, &bindings, true, &mut keys, &mut mouse);
        assert!(keys.just_pressed(KeyCode::KeyG) && keys.pressed(KeyCode::KeyB) && keys.pressed(KeyCode::KeyT), "kick, block, special");
        assert!(!keys.pressed(KeyCode::KeyF) && !keys.pressed(KeyCode::KeyQ) && !keys.pressed(KeyCode::KeyY), "not the melee, the carry or the swap");
        assert!(bindings.just(Action::Kick, &keys, &mouse) && bindings.held(Action::Block, &keys, &mouse) && bindings.held(Action::Special, &keys, &mouse));
        // Outside Kung Fu the same buttons are what they always were.
        let (mut keys, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        apply_bound(MELEE | SWAP, MELEE | SWAP, 0, &mut [None; 14], &bindings, false, &mut keys, &mut mouse);
        assert!(keys.pressed(KeyCode::KeyF) && keys.pressed(KeyCode::KeyY));
        // The page's word.
        let mut app = crate::arena::test_app("mode=kungfu");
        app.add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut app, 0.05);
        crate::arena::test_frame(&mut app, 0.05);
        let told = |app: &App| state(app.world().resource::<Game>(), false, None);
        assert_eq!(told(&app) & (can::FIGHT | can::ALIVE | can::CHI | can::STIM | can::PICKUP | can::SNIPER), can::FIGHT | can::ALIVE);
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            let me = g.me();
            g.kungfu.as_mut().unwrap().fighters.iter_mut().find(|f| f.id == me).unwrap().chi = 100.;
        }
        assert_eq!(told(&app) & can::CHI, can::CHI);
        let mut siege = crate::arena::test_app("");
        siege.add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut siege, 0.05);
        assert_eq!(state(siege.world().resource::<Game>(), false, None) & can::FIGHT, 0, "only in Kung Fu");
    }

    #[test]
    fn the_keyboard_keeps_its_own_keys() {
        let (mut keys, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::KeyF);
        let held = frame(0, 0, 0, &mut keys, &mut mouse);
        assert!(keys.pressed(KeyCode::KeyW) && keys.pressed(KeyCode::KeyF), "a pad that holds nothing releases nothing");
        assert_eq!(held, 0);
    }

    #[test]
    fn the_table_asks_for_flip_and_stop() {
        use kitchen_sim::cards::{Card, Table};
        // Single player's game by `rules`, its round begun.
        let game = |rules: &str| {
            let mut app = crate::arena::test_app(rules);
            app.add_systems(Update, crate::arena::tick);
            crate::arena::test_frame(&mut app, 0.05);
            app
        };
        let app = game("mode=cards");
        let g = app.world().resource::<Game>();
        assert!(crate::cards::at_table(g));
        // The table as this player sees it from the first seat: another hotdog's go.
        let mut view = Table::new(5, 7).view();
        (view.turn, view.choosing, view.between, view.winner) = (Some(2), None, false, None);
        let mut seen = crate::cards::Seen::default();
        seen.me = Some(0);
        let told = |view: &kitchen_sim::cards::View| {
            let mut seen = crate::cards::Seen::default();
            (seen.view, seen.me) = (Some(view.clone()), Some(0));
            state(g, false, Some(&seen))
        };
        // At the table: the table's two buttons (none of the round's: no walking, no guns), and whether they do anything.
        assert_eq!(told(&view), can::TABLE);
        view.turn = Some(0);
        assert_eq!(told(&view), can::TABLE | can::GO, "its go: flip or stop");
        view.choosing = Some((0, Card::PileOn));
        assert_eq!(told(&view), can::TABLE | can::PICK, "picking who gets Pile It On");
        view.choosing = Some((3, Card::Fridge));
        assert_eq!(told(&view), can::TABLE, "another's pick");
        (view.choosing, view.between) = (None, true);
        assert_eq!(told(&view), can::TABLE, "between hands");
        (view.between, view.winner) = (false, Some(0));
        assert_eq!(told(&view), can::TABLE, "a winner");
        // Before the table is seen (online, until the server's comes), and with no seat yet: the table, asking nothing.
        assert_eq!(state(g, false, None), can::TABLE);
        view.winner = None;
        seen.view = Some(view);
        seen.me = None;
        assert_eq!(state(g, false, Some(&seen)), can::TABLE);
        // The fridge is the fridge's.
        assert_eq!(state(g, true, Some(&seen)), can::LOBBY);
        // A siege is the round's, as before, whatever was last seen of a table.
        let siege = game("");
        let s = state(siege.world().resource::<Game>(), false, Some(&seen));
        assert!(s & can::PLAY != 0 && s & (can::TABLE | can::GO | can::PICK) == 0, "{s:b}");
    }

    #[test]
    fn the_page_is_read_safely() {
        let f = parse(&[3., 4., 12., -7., (JUMP | FIRE) as f32, MELEE as f32, 0.5, -2.]).unwrap();
        assert!((f.stick.length() - 1.).abs() < 1e-6, "the stick is at most 1 long");
        assert_eq!((f.look, f.down, f.taps, f.cursor), (Vec2::new(12., -7.), JUMP | FIRE, MELEE, Vec2::new(0.5, -1.)));
        assert_eq!(parse(&[0.3, 0.4, 0., 0., 0., 0., 0., 0.]).unwrap().stick, Vec2::new(0.3, 0.4), "half a push walks at half the pace");
        assert!(parse(&[]).is_none(), "no touch controls");
        assert!(parse(&[f32::NAN, 0., 0., 0., 0., 0., 0., 0.]).is_none());
        assert_eq!(parse(&[0., 0., 0., 0., 1e9, -5., 0., 0.]).unwrap().down, BUTTONS, "no bits beyond the buttons");
        // The pinch, ninth: none from a page that does not send it, and never more than a few times apart.
        assert_eq!(parse(&[0., 0., 0., 0., 0., 0., 0., 0.]).unwrap().pinch, 0.);
        assert_eq!(parse(&[0., 0., 0., 0., 0., 0., 0., 0., 0.4]).unwrap().pinch, 0.4);
        assert_eq!(parse(&[0., 0., 0., 0., 0., 0., 0., 0., -90.]).unwrap().pinch, -3.);
        assert!(parse(&[0., 0., 0., 0., 0., 0., 0., 0., f32::INFINITY]).is_none());
    }

    #[test]
    fn dead_the_trigger_shows_the_next_hotdog_when_there_is_one() {
        let game = |rules: &str| {
            let mut app = crate::arena::test_app(rules);
            app.add_systems(Update, crate::arena::tick);
            crate::arena::test_frame(&mut app, 0.05);
            app
        };
        let mut app = game("");
        let told = |app: &App| state(app.world().resource::<Game>(), false, None);
        assert_eq!(told(&app) & (can::ALIVE | can::WATCH), can::ALIVE);
        // Dead in a siege: the others are there to watch.
        let kill = |g: &mut Game, id: u32| {
            for p in g.roster.iter_mut().filter(|p| id == 0 || p.id == id) {
                p.alive = false;
            }
        };
        kill(&mut app.world_mut().resource_mut::<Game>(), 1);
        assert_eq!(told(&app) & (can::PLAY | can::ALIVE | can::WATCH), can::PLAY | can::WATCH);
        // Everybody dead: nobody to watch.
        kill(&mut app.world_mut().resource_mut::<Game>(), 0);
        assert_eq!(told(&app) & (can::PLAY | can::ALIVE | can::WATCH), can::PLAY);
    }

    /// The melee button says Bite, and an Eat button comes beside a corpse, in the siege's famine alone: Free For All has
    /// the strike, as every gun mode does, and no eating.
    #[test]
    fn the_bite_and_eat_buttons_are_the_famines_and_free_for_all_has_neither() {
        let game = |rules: &str, famine: bool| {
            let mut app = crate::arena::test_app(rules);
            app.add_systems(Update, crate::arena::tick);
            crate::arena::test_frame(&mut app, 0.05);
            if famine {
                // (The wave over: the simulation's stage, which the arena follows.)
                let mut g = app.world_mut().resource_mut::<Game>();
                (g.feast.stage, g.feast.famine_at) = (kitchen_sim::feast::Stage::FreeForAll, g.feast.elapsed);
                crate::arena::test_frame(&mut app, 0.05);
            }
            app
        };
        let told = |app: &App| state(app.world().resource::<Game>(), false, None);
        let famine = game("", true);
        assert_eq!(famine.world().resource::<Game>().phase_name(), "FreeForAll");
        assert_ne!(told(&famine) & can::FAMINE, 0, "the famine bites");
        let ffa = game("mode=ffa", false);
        assert_eq!(ffa.world().resource::<Game>().phase_name(), "FreeForAll");
        assert_eq!(told(&ffa) & (can::FAMINE | can::EAT), 0, "Free For All strikes, and eats nothing");
    }

    /// The hints button: shown in a round this player plays, reading whether the hints are on, and it presses H, which
    /// hides them or shows them again (hints.rs `toggle`).
    #[test]
    fn the_hints_button_shows_in_a_round_and_presses_h() {
        // Single player's siege, its round begun: hints to show, so the button, which reads whether they are on.
        let mut app = crate::arena::test_app("");
        app.add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut app, 0.05);
        let g = app.world().resource::<Game>();
        assert!(crate::hints::wanted(g));
        // (Off to begin with: hud.rs's icons are the game, the hints are help.)
        let (mut on, off) = (crate::hints::Hints::default(), crate::hints::Hints::default());
        on.set(true);
        assert_eq!(hints(g, Some(&on)), can::HINTS | can::HINTS_ON);
        assert_eq!(hints(g, Some(&off)), can::HINTS);
        // At the card table there are no hints, and no button.
        let mut table = crate::arena::test_app("mode=cards");
        table.add_systems(Update, crate::arena::tick);
        crate::arena::test_frame(&mut table, 0.05);
        assert_eq!(hints(table.world().resource::<Game>(), Some(&on)), 0);
        let (mut keys, mut mouse) = (ButtonInput::<KeyCode>::default(), ButtonInput::<MouseButton>::default());
        apply(HINTS, HINTS, 0, &mut keys, &mut mouse);
        assert!(keys.just_pressed(KeyCode::KeyH), "it presses H");
    }

    /// As the titan the buttons press its moves' keys (rebound, too) and the page is told it is the giant; a small
    /// hotdog's buttons are the gun basics' as ever.
    #[test]
    fn as_the_titan_the_buttons_are_its_moves() {
        let mut bindings = Bindings::default();
        bindings.assign(Action::Stomp, 0, Input::Key(KeyCode::KeyG)).unwrap();
        let (mut keys, mut mouse) = (ButtonInput::default(), ButtonInput::default());
        apply_keys(MELEE | STIM | SCOPE, MELEE | STIM | SCOPE, 0, &mut [None; 14], &bindings, &TITAN_KEYS, false, &mut keys, &mut mouse);
        assert!(keys.pressed(KeyCode::KeyG) && !keys.pressed(KeyCode::KeyF), "the stomp's own key");
        assert!(keys.pressed(KeyCode::ShiftLeft) && mouse.pressed(MouseButton::Right), "the charge and the smash");
        assert!(bindings.just(Action::Stomp, &keys, &mouse) && bindings.just(Action::Charge, &keys, &mouse) && bindings.held(Action::Smash, &keys, &mouse));
        assert_eq!(can::GIANT, 1 << 31);
        let map = || kitchen_sim::map::Map::parse(include_str!("../assets/maps/titans-kitchen.json")).unwrap();
        for (pick, giant) in [("me", true), ("bot", false)] {
            let mut app = crate::arena::test_app(&format!("mode=titan&titan={pick}"));
            app.add_systems(Update, crate::arena::tick);
            crate::arena::load_map(app.world_mut(), map());
            crate::arena::test_frame(&mut app, 0.05);
            let s = state(app.world().resource::<Game>(), false, None);
            assert_eq!(s & can::GIANT != 0, giant, "{pick}");
            assert_eq!(s & can::FIGHT, 0);
        }
    }
}

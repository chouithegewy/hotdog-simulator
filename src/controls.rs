// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The player's controls: what each key, mouse button and turn of the wheel does, and the gate that keeps the game
//! deaf while the menu is open (menu.rs).
//!
//! * **Actions.** Every button the game reads in play is an [`Action`]: walking, jumping, crouching, the trigger, the
//!   aim, the melee strike, picking up, carrying, throwing, turning and tipping what is carried, the guns' slots and
//!   each gun by name, the hints, a new round. [`Bindings`] gives each one or two [`Input`]s: a key, a mouse button,
//!   or the wheel rolled up or down. The defaults ([`Bindings::default`]) are the keys the game has always had.
//! * **Reading.** `arena::tick`, the death camera, the card table, the hints, the head-up display's key caps and the
//!   touch controls (which press whatever the action's key now is) all ask the bindings ([`Bindings::held`],
//!   [`Bindings::just`], [`Bindings::wheeled`]) instead of a key. Left and right Shift, Ctrl, Alt are one key each.
//!   Esc is never bound: it is the menu's (and a browser's way out of the pointer lock). The left button still captures
//!   a free pointer, whatever it is bound to: that is the pointer's, not an action.
//! * **Conflicts.** Each action is heard in a [`Context`]: always, everywhere but Kung Fu, with a gun in hand, with a gun
//!   in hand outside Forts' build phase (the reload), only in Forts' build phase (ready), while carrying something, or
//!   only in Kung Fu. Two actions in contexts that can be live at once may not share an input
//!   ([`Bindings::conflict`]); the trigger and the throw can (both the left button), so can the wheel's guns and its
//!   turning of what is carried, the reload and the build phase's ready (both R: the guns are locked while it is
//!   heard), and Kung Fu's strikes, kicks, block, grab, dodge and styles share the keys of the
//!   trigger, the aim, the melee, picking up, using, the stim and the guns, which are not heard there. A
//!   rebind onto an input in use is refused with the action that has it, and [`Bindings::swap`] settles it by giving
//!   that action the input it replaces.
//! * **Kept** in prefs.rs, `controls` (only the actions that differ from the defaults, as JSON, so a default changed
//!   later still reaches a player who never rebound that action) and `controls.wheel` (`inverted` or `normal`).
//! * **The gate** ([`Gate`]): while the menu is open the game's systems see no key, no button, no mouse motion, no
//!   wheel and no touch. The real state is put aside after the menu (and Bevy's UI) have read it, and put back before
//!   the next frame's input is read, so nothing is lost or stuck: a key let go in the menu is let go. After the menu
//!   closes on a click, the mouse stays shut until its buttons are up, so the click on Resume does not also fire.
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::prefs::Prefs>()
            .init_resource::<Bindings>()
            .init_resource::<Gate>()
            .add_systems(Startup, load)
            .add_systems(PreUpdate, restore.before(bevy::input::InputSystems))
            .add_systems(PreUpdate, shut.in_set(Shut).after(bevy::input::InputSystems).after(bevy::ui::UiSystems::Focus))
            .add_systems(Update, remember);
    }
}
/// The gate's shutting (PreUpdate): the menu decides before it what is shut this frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shut;

/// Where the bindings and the wheel's direction are kept (prefs.rs).
pub const KEY: &str = "controls";
pub const WHEEL_KEY: &str = "controls.wheel";

// ----- Actions.

/// When an action is heard: two actions may share an input only if they are never heard at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// Whatever is in hand, in every mode.
    Always,
    /// Whatever is in hand, in every mode but Kung Fu (hands and feet only: no stim, no prone, nothing picked up).
    Kitchen,
    /// With a gun in hand (not carrying a loose object).
    Armed,
    /// With a gun in hand outside Forts' build phase, when a gun can be reloaded.
    Loading,
    /// Forts' build phase, while the guns are locked.
    Build,
    /// Carrying a loose object.
    Carrying,
    /// In Kung Fu.
    KungFu,
    /// Watching a replay (replay/): only the viewer's keys are heard, none of the game's.
    Replay,
}
impl Context {
    pub fn overlaps(self, other: Context) -> bool {
        use Context::*;
        match (self, other) {
            (Replay, Replay) => true,
            (Replay, _) | (_, Replay) => false,
            (Always, _) | (_, Always) => true,
            (KungFu, KungFu) => true,
            (KungFu, _) | (_, KungFu) => false,
            // (The guns are locked in the build phase: nothing is reloaded then.)
            (Build, Loading) | (Loading, Build) => false,
            (Kitchen, _) | (_, Kitchen) => true,
            (Build, _) | (_, Build) => true,
            (Loading, Armed) | (Armed, Loading) => true,
            _ => self == other,
        }
    }
}
/// The tabs of the menu's controls page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Movement,
    Combat,
    Weapons,
    Carrying,
    /// Kung Fu's fight: the strikes, the kick, the block, the grab, the dodge, the special, the next and last style.
    KungFu,
    /// Kung Fu's styles by name (a tab of their own: the fight's tab would not fit the window with them).
    Styles,
    /// A replay's playback (replay/): play, the speeds, seeking, the stats and the overlays.
    Replay,
    /// A replay's camera: whose view, first person, following, flying.
    Camera,
}
impl Group {
    pub const ALL: [Group; 8] = [Group::Movement, Group::Combat, Group::Weapons, Group::Carrying, Group::KungFu, Group::Styles, Group::Replay, Group::Camera];
    pub fn name(self) -> &'static str {
        match self {
            Group::Movement => "MOVEMENT",
            Group::Combat => "COMBAT",
            Group::Weapons => "WEAPONS",
            Group::Carrying => "CARRYING",
            Group::KungFu => "KUNG FU",
            Group::Styles => "STYLES",
            Group::Replay => "REPLAY",
            Group::Camera => "REPLAY CAMERA",
        }
    }
    /// Whether the wheel's inversion is shown on its tab (the wheel picks guns, turns what is carried, changes style).
    pub fn wheels(self) -> bool {
        matches!(self, Group::Weapons | Group::Carrying | Group::KungFu)
    }
}

/// Everything a button does in play.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Crouch,
    Prone,
    Stim,
    Fire,
    Aim,
    /// R: reload the gun in hand from its reserve (kitchen-sim magazine.rs).
    Reload,
    Melee,
    Use,
    Ready,
    Hints,
    NewRound,
    Primary,
    Secondary,
    Swap,
    NextWeapon,
    PreviousWeapon,
    LastWeapon,
    /// Each gun by name, `Weapon::index` 1 to 7: the mustard pistol, the ketchup sniper, the hot sauce thrower, the
    /// relish bazooka, the bug spray, the syrup rifle and the mayo squirter.
    Gun1,
    Gun2,
    Gun3,
    Gun4,
    Gun5,
    Gun6,
    Gun7,
    Carry,
    Throw,
    TurnClockwise,
    TurnAnticlockwise,
    Tip,
    /// Kung Fu (interface.md section 5): the light strike, the heavy strike, the kick, the block (held; a tap well timed
    /// parries), the grab and throw, the dodge, the special (with full chi), the next and last style, and each style by
    /// name, `kungfu::Style` 0 to 6: Tiger, Crane, Snake, Mantis, Monkey, Drunken, Toad.
    Light,
    Heavy,
    Kick,
    Block,
    Grab,
    Dodge,
    Special,
    NextStyle,
    PreviousStyle,
    Style1,
    Style2,
    Style3,
    Style4,
    Style5,
    Style6,
    Style7,
    /// Kung Fu: the camera over the shoulder, or back in first person (KF6, fight.rs).
    View,
    /// A replay (replay/): play or pause, slower and faster (0.25x, 0.5x, 1x, 2x), one frame on, five seconds back
    /// and on, the kill before and after, the stats, every display hidden, the help, and the overlays (names over
    /// everyone, damage numbers, shot tracers, the map).
    PlayPause,
    Slower,
    Faster,
    StepFrame,
    SeekBack,
    SeekAhead,
    PreviousKill,
    NextKill,
    Stats,
    HideUi,
    ReplayHelp,
    NameTags,
    DamageNumbers,
    Tracers,
    Minimap,
    /// A replay's camera: first person, following and flying in turn; the next and the previous player; each of five
    /// by its place; and the flying camera's moves, climb, descent and speed.
    CameraMode,
    NextPlayer,
    PreviousPlayer,
    Watch1,
    Watch2,
    Watch3,
    Watch4,
    Watch5,
    FlyForward,
    FlyBack,
    FlyLeft,
    FlyRight,
    FlyUp,
    FlyDown,
    FlyFast,
}
pub const ACTIONS: usize = 81;
impl Action {
    pub const ALL: [Action; ACTIONS] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Jump,
        Action::Crouch,
        Action::Prone,
        Action::Stim,
        Action::Fire,
        Action::Aim,
        Action::Reload,
        Action::Melee,
        Action::Use,
        Action::Ready,
        Action::Hints,
        Action::NewRound,
        Action::Primary,
        Action::Secondary,
        Action::Swap,
        Action::NextWeapon,
        Action::PreviousWeapon,
        Action::LastWeapon,
        Action::Gun1,
        Action::Gun2,
        Action::Gun3,
        Action::Gun4,
        Action::Gun5,
        Action::Gun6,
        Action::Gun7,
        Action::Carry,
        Action::Throw,
        Action::TurnClockwise,
        Action::TurnAnticlockwise,
        Action::Tip,
        Action::Light,
        Action::Heavy,
        Action::Kick,
        Action::Block,
        Action::Grab,
        Action::Dodge,
        Action::Special,
        Action::NextStyle,
        Action::PreviousStyle,
        Action::Style1,
        Action::Style2,
        Action::Style3,
        Action::Style4,
        Action::Style5,
        Action::Style6,
        Action::Style7,
        Action::View,
        Action::PlayPause,
        Action::Slower,
        Action::Faster,
        Action::StepFrame,
        Action::SeekBack,
        Action::SeekAhead,
        Action::PreviousKill,
        Action::NextKill,
        Action::Stats,
        Action::HideUi,
        Action::ReplayHelp,
        Action::NameTags,
        Action::DamageNumbers,
        Action::Tracers,
        Action::Minimap,
        Action::CameraMode,
        Action::NextPlayer,
        Action::PreviousPlayer,
        Action::Watch1,
        Action::Watch2,
        Action::Watch3,
        Action::Watch4,
        Action::Watch5,
        Action::FlyForward,
        Action::FlyBack,
        Action::FlyLeft,
        Action::FlyRight,
        Action::FlyUp,
        Action::FlyDown,
        Action::FlyFast,
    ];
    /// A replay's players by their places, first to fifth.
    pub const WATCH: [Action; 5] = [Action::Watch1, Action::Watch2, Action::Watch3, Action::Watch4, Action::Watch5];
    pub const GUNS: [Action; 7] = [Action::Gun1, Action::Gun2, Action::Gun3, Action::Gun4, Action::Gun5, Action::Gun6, Action::Gun7];
    /// Each Kung Fu style by name, in `kungfu::Style`'s order.
    pub const STYLES: [Action; 7] = [Action::Style1, Action::Style2, Action::Style3, Action::Style4, Action::Style5, Action::Style6, Action::Style7];
    fn at(self) -> usize {
        self as usize
    }
    /// Its name in the menu (ASCII, as the game's font is).
    pub fn name(self) -> &'static str {
        match self {
            Action::Forward => "MOVE FORWARD",
            Action::Back => "MOVE BACK",
            Action::Left => "MOVE LEFT",
            Action::Right => "MOVE RIGHT",
            Action::Jump => "JUMP / STAND UP",
            Action::Crouch => "CROUCH (HOLD)",
            Action::Prone => "PRONE",
            Action::Stim => "STIM",
            Action::Fire => "FIRE",
            Action::Aim => "AIM / SCOPE",
            Action::Reload => "RELOAD",
            Action::Melee => "MELEE / BITE",
            Action::Use => "PICK UP GUN / EAT",
            Action::Ready => "READY (FORTS BUILD)",
            Action::Hints => "SHOW HINTS",
            Action::NewRound => "NEW ROUND",
            Action::Primary => "PRIMARY WEAPON",
            Action::Secondary => "SECONDARY WEAPON",
            Action::Swap => "SWAP WEAPONS",
            Action::NextWeapon => "NEXT WEAPON",
            Action::PreviousWeapon => "PREVIOUS WEAPON",
            Action::LastWeapon => "LAST WEAPON USED",
            Action::Gun1 => "MUSTARD PISTOL",
            Action::Gun2 => "KETCHUP SNIPER",
            Action::Gun3 => "HOT SAUCE THROWER",
            Action::Gun4 => "RELISH BAZOOKA",
            Action::Gun5 => "BUG SPRAY",
            Action::Gun6 => "SYRUP RIFLE",
            Action::Gun7 => "MAYO SQUIRTER",
            Action::Carry => "PICK UP / PUT DOWN",
            Action::Throw => "THROW",
            Action::TurnClockwise => "TURN CLOCKWISE",
            Action::TurnAnticlockwise => "TURN ANTICLOCKWISE",
            Action::Tip => "TIP OVER",
            Action::Light => "LIGHT STRIKE",
            Action::Heavy => "HEAVY STRIKE",
            Action::Kick => "KICK",
            Action::Block => "BLOCK (HOLD) / PARRY (TAP)",
            Action::Grab => "GRAB AND THROW",
            Action::Dodge => "DODGE",
            Action::Special => "SPECIAL (FULL CHI)",
            Action::NextStyle => "NEXT STYLE",
            Action::PreviousStyle => "PREVIOUS STYLE",
            Action::Style1 => "TIGER",
            Action::Style2 => "CRANE",
            Action::Style3 => "SNAKE",
            Action::Style4 => "MANTIS",
            Action::Style5 => "MONKEY",
            Action::Style6 => "DRUNKEN",
            Action::Style7 => "TOAD",
            Action::View => "THIRD PERSON (KUNG FU)",
            Action::PlayPause => "PLAY / PAUSE",
            Action::Slower => "SLOWER",
            Action::Faster => "FASTER",
            Action::StepFrame => "ONE FRAME ON (PAUSED)",
            Action::SeekBack => "BACK 5 SECONDS",
            Action::SeekAhead => "ON 5 SECONDS",
            Action::PreviousKill => "PREVIOUS KILL",
            Action::NextKill => "NEXT KILL",
            Action::Stats => "STATS",
            Action::HideUi => "HIDE EVERYTHING ON SCREEN",
            Action::ReplayHelp => "REPLAY HELP",
            Action::NameTags => "NAMES OVER EVERYONE",
            Action::DamageNumbers => "DAMAGE NUMBERS",
            Action::Tracers => "SHOT TRACERS",
            Action::Minimap => "MAP",
            Action::CameraMode => "FIRST PERSON / FOLLOW / FLY",
            Action::NextPlayer => "NEXT PLAYER",
            Action::PreviousPlayer => "PREVIOUS PLAYER",
            Action::Watch1 => "PLAYER 1",
            Action::Watch2 => "PLAYER 2",
            Action::Watch3 => "PLAYER 3",
            Action::Watch4 => "PLAYER 4",
            Action::Watch5 => "PLAYER 5",
            Action::FlyForward => "FLY FORWARD",
            Action::FlyBack => "FLY BACK",
            Action::FlyLeft => "FLY LEFT",
            Action::FlyRight => "FLY RIGHT",
            Action::FlyUp => "FLY UP",
            Action::FlyDown => "FLY DOWN",
            Action::FlyFast => "FLY FAST (HOLD)",
        }
    }
    /// Its name where it is kept (prefs.rs).
    pub fn id(self) -> &'static str {
        match self {
            Action::Forward => "forward",
            Action::Back => "back",
            Action::Left => "left",
            Action::Right => "right",
            Action::Jump => "jump",
            Action::Crouch => "crouch",
            Action::Prone => "prone",
            Action::Stim => "stim",
            Action::Fire => "fire",
            Action::Aim => "aim",
            Action::Reload => "reload",
            Action::Melee => "melee",
            Action::Use => "use",
            Action::Ready => "ready",
            Action::Hints => "hints",
            Action::NewRound => "new_round",
            Action::Primary => "primary",
            Action::Secondary => "secondary",
            Action::Swap => "swap",
            Action::NextWeapon => "next_weapon",
            Action::PreviousWeapon => "previous_weapon",
            Action::LastWeapon => "last_weapon",
            Action::Gun1 => "gun_mustard_pistol",
            Action::Gun2 => "gun_ketchup_sniper",
            Action::Gun3 => "gun_hot_sauce",
            Action::Gun4 => "gun_relish_bazooka",
            Action::Gun5 => "gun_bug_spray",
            Action::Gun6 => "gun_syrup_rifle",
            Action::Gun7 => "gun_mayo_squirter",
            Action::Carry => "carry",
            Action::Throw => "throw",
            Action::TurnClockwise => "turn_clockwise",
            Action::TurnAnticlockwise => "turn_anticlockwise",
            Action::Tip => "tip",
            Action::Light => "fu_light",
            Action::Heavy => "fu_heavy",
            Action::Kick => "fu_kick",
            Action::Block => "fu_block",
            Action::Grab => "fu_grab",
            Action::Dodge => "fu_dodge",
            Action::Special => "fu_special",
            Action::NextStyle => "fu_next_style",
            Action::PreviousStyle => "fu_previous_style",
            Action::Style1 => "fu_style_tiger",
            Action::Style2 => "fu_style_crane",
            Action::Style3 => "fu_style_snake",
            Action::Style4 => "fu_style_mantis",
            Action::Style5 => "fu_style_monkey",
            Action::Style6 => "fu_style_drunken",
            Action::Style7 => "fu_style_toad",
            Action::View => "fu_third_person",
            Action::PlayPause => "replay_play",
            Action::Slower => "replay_slower",
            Action::Faster => "replay_faster",
            Action::StepFrame => "replay_step",
            Action::SeekBack => "replay_back",
            Action::SeekAhead => "replay_ahead",
            Action::PreviousKill => "replay_previous_kill",
            Action::NextKill => "replay_next_kill",
            Action::Stats => "replay_stats",
            Action::HideUi => "replay_hide_ui",
            Action::ReplayHelp => "replay_help",
            Action::NameTags => "replay_names",
            Action::DamageNumbers => "replay_damage",
            Action::Tracers => "replay_tracers",
            Action::Minimap => "replay_map",
            Action::CameraMode => "replay_camera",
            Action::NextPlayer => "replay_next_player",
            Action::PreviousPlayer => "replay_previous_player",
            Action::Watch1 => "replay_player_1",
            Action::Watch2 => "replay_player_2",
            Action::Watch3 => "replay_player_3",
            Action::Watch4 => "replay_player_4",
            Action::Watch5 => "replay_player_5",
            Action::FlyForward => "replay_fly_forward",
            Action::FlyBack => "replay_fly_back",
            Action::FlyLeft => "replay_fly_left",
            Action::FlyRight => "replay_fly_right",
            Action::FlyUp => "replay_fly_up",
            Action::FlyDown => "replay_fly_down",
            Action::FlyFast => "replay_fly_fast",
        }
    }
    pub fn context(self) -> Context {
        use Action::*;
        match self {
            Fire | Aim | Melee | Primary | Secondary | Swap | NextWeapon | PreviousWeapon | LastWeapon | Gun1 | Gun2 | Gun3 | Gun4 | Gun5 | Gun6 | Gun7 => Context::Armed,
            Throw | TurnClockwise | TurnAnticlockwise | Tip => Context::Carrying,
            Reload => Context::Loading,
            // (R, as the reload is: the guns are locked while it is heard.)
            Ready => Context::Build,
            Light | Heavy | Kick | Block | Grab | Dodge | Special | NextStyle | PreviousStyle | Style1 | Style2 | Style3 | Style4 | Style5 | Style6 | Style7 | View => Context::KungFu,
            PlayPause | Slower | Faster | StepFrame | SeekBack | SeekAhead | PreviousKill | NextKill | Stats | HideUi | ReplayHelp | NameTags | DamageNumbers | Tracers | Minimap | CameraMode | NextPlayer | PreviousPlayer | Watch1 | Watch2
            | Watch3 | Watch4 | Watch5 | FlyForward | FlyBack | FlyLeft | FlyRight | FlyUp | FlyDown | FlyFast => Context::Replay,
            // Kung Fu has no stim (Shift dodges), no prone (T is the special), nothing to pick up, use or carry, no Forts.
            Stim | Prone | Use | Carry => Context::Kitchen,
            _ => Context::Always,
        }
    }
    pub fn group(self) -> Group {
        use Action::*;
        match self {
            Forward | Back | Left | Right | Jump | Crouch | Prone | Stim | View => Group::Movement,
            Fire | Aim | Reload | Melee | Use | Ready | Hints | NewRound => Group::Combat,
            Carry | Throw | TurnClockwise | TurnAnticlockwise | Tip => Group::Carrying,
            Light | Heavy | Kick | Block | Grab | Dodge | Special | NextStyle | PreviousStyle => Group::KungFu,
            Style1 | Style2 | Style3 | Style4 | Style5 | Style6 | Style7 => Group::Styles,
            PlayPause | Slower | Faster | StepFrame | SeekBack | SeekAhead | PreviousKill | NextKill | Stats | HideUi | ReplayHelp | NameTags | DamageNumbers | Tracers | Minimap => Group::Replay,
            CameraMode | NextPlayer | PreviousPlayer | Watch1 | Watch2 | Watch3 | Watch4 | Watch5 | FlyForward | FlyBack | FlyLeft | FlyRight | FlyUp | FlyDown | FlyFast => Group::Camera,
            _ => Group::Weapons,
        }
    }
    /// The Kung Fu style a style's own action takes up (`kungfu::Style` as its number), for the others None.
    pub fn style(self) -> Option<usize> {
        Action::STYLES.iter().position(|a| *a == self)
    }
    /// The gun a gun's own action takes in hand (`Weapon::index`), for the others None.
    pub fn gun(self) -> Option<usize> {
        Action::GUNS.iter().position(|a| *a == self).map(|i| i + 1)
    }
}

// ----- Inputs.

/// A rolled notch of the mouse wheel: up is away from the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Wheel {
    Up,
    Down,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
    Wheel(Wheel),
}
/// The keys that can be bound, with their names where they are kept (the `KeyCode`'s own) and on a key cap. Esc is
/// not among them (the menu's), nor the system keys (Super, the media keys, F11's full screen in a browser...).
const KEYS: &[(KeyCode, &str, &str)] = &[
    (KeyCode::KeyA, "KeyA", "A"),
    (KeyCode::KeyB, "KeyB", "B"),
    (KeyCode::KeyC, "KeyC", "C"),
    (KeyCode::KeyD, "KeyD", "D"),
    (KeyCode::KeyE, "KeyE", "E"),
    (KeyCode::KeyF, "KeyF", "F"),
    (KeyCode::KeyG, "KeyG", "G"),
    (KeyCode::KeyH, "KeyH", "H"),
    (KeyCode::KeyI, "KeyI", "I"),
    (KeyCode::KeyJ, "KeyJ", "J"),
    (KeyCode::KeyK, "KeyK", "K"),
    (KeyCode::KeyL, "KeyL", "L"),
    (KeyCode::KeyM, "KeyM", "M"),
    (KeyCode::KeyN, "KeyN", "N"),
    (KeyCode::KeyO, "KeyO", "O"),
    (KeyCode::KeyP, "KeyP", "P"),
    (KeyCode::KeyQ, "KeyQ", "Q"),
    (KeyCode::KeyR, "KeyR", "R"),
    (KeyCode::KeyS, "KeyS", "S"),
    (KeyCode::KeyT, "KeyT", "T"),
    (KeyCode::KeyU, "KeyU", "U"),
    (KeyCode::KeyV, "KeyV", "V"),
    (KeyCode::KeyW, "KeyW", "W"),
    (KeyCode::KeyX, "KeyX", "X"),
    (KeyCode::KeyY, "KeyY", "Y"),
    (KeyCode::KeyZ, "KeyZ", "Z"),
    (KeyCode::Digit0, "Digit0", "0"),
    (KeyCode::Digit1, "Digit1", "1"),
    (KeyCode::Digit2, "Digit2", "2"),
    (KeyCode::Digit3, "Digit3", "3"),
    (KeyCode::Digit4, "Digit4", "4"),
    (KeyCode::Digit5, "Digit5", "5"),
    (KeyCode::Digit6, "Digit6", "6"),
    (KeyCode::Digit7, "Digit7", "7"),
    (KeyCode::Digit8, "Digit8", "8"),
    (KeyCode::Digit9, "Digit9", "9"),
    (KeyCode::Space, "Space", "SPACE"),
    (KeyCode::Enter, "Enter", "ENTER"),
    (KeyCode::Tab, "Tab", "TAB"),
    (KeyCode::Backspace, "Backspace", "BACKSPACE"),
    (KeyCode::CapsLock, "CapsLock", "CAPS LOCK"),
    (KeyCode::ShiftLeft, "ShiftLeft", "SHIFT"),
    (KeyCode::ControlLeft, "ControlLeft", "CTRL"),
    (KeyCode::AltLeft, "AltLeft", "ALT"),
    (KeyCode::ArrowUp, "ArrowUp", "UP"),
    (KeyCode::ArrowDown, "ArrowDown", "DOWN"),
    (KeyCode::ArrowLeft, "ArrowLeft", "LEFT"),
    (KeyCode::ArrowRight, "ArrowRight", "RIGHT"),
    (KeyCode::Insert, "Insert", "INSERT"),
    (KeyCode::Delete, "Delete", "DELETE"),
    (KeyCode::Home, "Home", "HOME"),
    (KeyCode::End, "End", "END"),
    (KeyCode::PageUp, "PageUp", "PAGE UP"),
    (KeyCode::PageDown, "PageDown", "PAGE DOWN"),
    (KeyCode::Minus, "Minus", "-"),
    (KeyCode::Equal, "Equal", "="),
    (KeyCode::BracketLeft, "BracketLeft", "["),
    (KeyCode::BracketRight, "BracketRight", "]"),
    (KeyCode::Semicolon, "Semicolon", ";"),
    (KeyCode::Quote, "Quote", "'"),
    (KeyCode::Backquote, "Backquote", "`"),
    (KeyCode::Backslash, "Backslash", "\\"),
    (KeyCode::Comma, "Comma", ","),
    (KeyCode::Period, "Period", "."),
    (KeyCode::Slash, "Slash", "/"),
    (KeyCode::F1, "F1", "F1"),
    (KeyCode::F2, "F2", "F2"),
    (KeyCode::F3, "F3", "F3"),
    (KeyCode::F4, "F4", "F4"),
    (KeyCode::F5, "F5", "F5"),
    (KeyCode::F7, "F7", "F7"),
    (KeyCode::F9, "F9", "F9"),
    (KeyCode::F10, "F10", "F10"),
    (KeyCode::F12, "F12", "F12"),
    (KeyCode::Numpad0, "Numpad0", "NUM 0"),
    (KeyCode::Numpad1, "Numpad1", "NUM 1"),
    (KeyCode::Numpad2, "Numpad2", "NUM 2"),
    (KeyCode::Numpad3, "Numpad3", "NUM 3"),
    (KeyCode::Numpad4, "Numpad4", "NUM 4"),
    (KeyCode::Numpad5, "Numpad5", "NUM 5"),
    (KeyCode::Numpad6, "Numpad6", "NUM 6"),
    (KeyCode::Numpad7, "Numpad7", "NUM 7"),
    (KeyCode::Numpad8, "Numpad8", "NUM 8"),
    (KeyCode::Numpad9, "Numpad9", "NUM 9"),
    (KeyCode::NumpadAdd, "NumpadAdd", "NUM +"),
    (KeyCode::NumpadSubtract, "NumpadSubtract", "NUM -"),
    (KeyCode::NumpadMultiply, "NumpadMultiply", "NUM *"),
    (KeyCode::NumpadDivide, "NumpadDivide", "NUM /"),
    (KeyCode::NumpadDecimal, "NumpadDecimal", "NUM ."),
    (KeyCode::NumpadEnter, "NumpadEnter", "NUM ENTER"),
];
/// A key as it is bound: right Shift, Ctrl and Alt are the left ones (each pair is one key).
pub fn plain(key: KeyCode) -> KeyCode {
    match key {
        KeyCode::ShiftRight => KeyCode::ShiftLeft,
        KeyCode::ControlRight => KeyCode::ControlLeft,
        KeyCode::AltRight => KeyCode::AltLeft,
        other => other,
    }
}
/// The keys a bound key stands for: the pair of a modifier, or the key itself.
fn twins(key: KeyCode) -> [KeyCode; 2] {
    match key {
        KeyCode::ShiftLeft => [KeyCode::ShiftLeft, KeyCode::ShiftRight],
        KeyCode::ControlLeft => [KeyCode::ControlLeft, KeyCode::ControlRight],
        KeyCode::AltLeft => [KeyCode::AltLeft, KeyCode::AltRight],
        other => [other, other],
    }
}
impl Input {
    /// Whether the input may be bound at all (Esc and keys with no name may not).
    pub fn bindable(self) -> bool {
        match self {
            Input::Key(key) => KEYS.iter().any(|(k, ..)| *k == plain(key)),
            Input::Mouse(_) | Input::Wheel(_) => true,
        }
    }
    /// As it is shown: a key cap's label, a mouse button, the wheel.
    pub fn label(self) -> &'static str {
        match self {
            Input::Key(key) => KEYS.iter().find(|(k, ..)| *k == plain(key)).map_or("?", |(_, _, label)| label),
            Input::Mouse(MouseButton::Left) => "LMB",
            Input::Mouse(MouseButton::Right) => "RMB",
            Input::Mouse(MouseButton::Middle) => "MMB",
            Input::Mouse(MouseButton::Back) => "MOUSE 4",
            Input::Mouse(MouseButton::Forward) => "MOUSE 5",
            Input::Mouse(MouseButton::Other(n)) => match n {
                5 => "MOUSE 6",
                6 => "MOUSE 7",
                7 => "MOUSE 8",
                _ => "MOUSE +",
            },
            Input::Wheel(Wheel::Up) => "WHEEL UP",
            Input::Wheel(Wheel::Down) => "WHEEL DOWN",
        }
    }
    /// Its name where it is kept.
    pub fn id(self) -> String {
        match self {
            Input::Key(key) => KEYS.iter().find(|(k, ..)| *k == plain(key)).map_or_else(String::new, |(_, id, _)| id.to_string()),
            Input::Mouse(MouseButton::Left) => "MouseLeft".into(),
            Input::Mouse(MouseButton::Right) => "MouseRight".into(),
            Input::Mouse(MouseButton::Middle) => "MouseMiddle".into(),
            Input::Mouse(MouseButton::Back) => "MouseBack".into(),
            Input::Mouse(MouseButton::Forward) => "MouseForward".into(),
            Input::Mouse(MouseButton::Other(n)) => format!("Mouse{n}"),
            Input::Wheel(Wheel::Up) => "WheelUp".into(),
            Input::Wheel(Wheel::Down) => "WheelDown".into(),
        }
    }
    pub fn parse(text: &str) -> Option<Input> {
        let mouse = match text {
            "MouseLeft" => Some(MouseButton::Left),
            "MouseRight" => Some(MouseButton::Right),
            "MouseMiddle" => Some(MouseButton::Middle),
            "MouseBack" => Some(MouseButton::Back),
            "MouseForward" => Some(MouseButton::Forward),
            _ => text.strip_prefix("Mouse").and_then(|n| n.parse::<u16>().ok()).map(MouseButton::Other),
        };
        if let Some(button) = mouse {
            return Some(Input::Mouse(button));
        }
        match text {
            "WheelUp" => Some(Input::Wheel(Wheel::Up)),
            "WheelDown" => Some(Input::Wheel(Wheel::Down)),
            _ => KEYS.iter().find(|(_, id, _)| *id == text).map(|(k, ..)| Input::Key(*k)),
        }
    }
    fn held(self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        match self {
            Input::Key(key) => keys.any_pressed(twins(key)),
            Input::Mouse(button) => mouse.pressed(button),
            Input::Wheel(_) => false,
        }
    }
    fn just(self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        match self {
            Input::Key(key) => keys.any_just_pressed(twins(key)),
            Input::Mouse(button) => mouse.just_pressed(button),
            Input::Wheel(_) => false,
        }
    }
}

// ----- Bindings.

/// What an input already does, when it is asked of another action: that action, and its slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub action: Action,
    pub slot: usize,
}

/// Each action's inputs, two at most (slot 0 the main one, slot 1 the alternative), and whether the wheel is turned
/// the other way round.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Bindings {
    slots: [[Option<Input>; 2]; ACTIONS],
    pub invert_wheel: bool,
}
impl Default for Bindings {
    /// The keys the game has always had (README's controls).
    fn default() -> Self {
        let mut slots = [[None; 2]; ACTIONS];
        for a in Action::ALL {
            slots[a.at()] = default_of(a);
        }
        Self { slots, invert_wheel: false }
    }
}
/// An action's default inputs.
pub fn default_of(action: Action) -> [Option<Input>; 2] {
    use Action::*;
    let key = |k| Some(Input::Key(k));
    let mouse = |b| Some(Input::Mouse(b));
    match action {
        Forward => [key(KeyCode::KeyW), None],
        Back => [key(KeyCode::KeyS), None],
        Left => [key(KeyCode::KeyA), None],
        Right => [key(KeyCode::KeyD), None],
        Jump => [key(KeyCode::Space), None],
        Crouch => [key(KeyCode::ControlLeft), key(KeyCode::KeyC)],
        Prone => [key(KeyCode::KeyT), None],
        Stim => [key(KeyCode::ShiftLeft), None],
        Fire => [mouse(MouseButton::Left), None],
        Aim => [mouse(MouseButton::Right), None],
        // R reloads; in Forts' build phase, when the guns are locked, it says ready instead.
        Reload => [key(KeyCode::KeyR), None],
        Melee => [key(KeyCode::KeyF), None],
        Use => [key(KeyCode::KeyE), None],
        Ready => [key(KeyCode::KeyR), None],
        Hints => [key(KeyCode::KeyH), None],
        NewRound => [key(KeyCode::Enter), None],
        Primary => [key(KeyCode::Digit1), None],
        Secondary => [key(KeyCode::Digit2), None],
        Swap => [key(KeyCode::KeyY), None],
        NextWeapon => [Some(Input::Wheel(Wheel::Down)), None],
        PreviousWeapon => [Some(Input::Wheel(Wheel::Up)), None],
        LastWeapon | Gun1 | Gun2 | Gun3 | Gun4 | Gun5 | Gun6 | Gun7 => [None, None],
        Carry => [key(KeyCode::KeyQ), None],
        Throw => [mouse(MouseButton::Left), None],
        TurnClockwise => [Some(Input::Wheel(Wheel::Down)), None],
        TurnAnticlockwise => [Some(Input::Wheel(Wheel::Up)), None],
        Tip => [key(KeyCode::KeyX), None],
        // Kung Fu, on the keys of what they stand in for (spec and interface.md): the trigger strikes light, the aim
        // heavy, the melee key kicks, the carry key blocks, the use key grabs, the stim key dodges, the prone key is the
        // special, the wheel and the number keys change style.
        Light => [mouse(MouseButton::Left), None],
        Heavy => [mouse(MouseButton::Right), None],
        Kick => [key(KeyCode::KeyF), None],
        Block => [key(KeyCode::KeyQ), None],
        Grab => [key(KeyCode::KeyE), None],
        Dodge => [key(KeyCode::ShiftLeft), None],
        Special => [key(KeyCode::KeyT), None],
        NextStyle => [Some(Input::Wheel(Wheel::Down)), None],
        PreviousStyle => [Some(Input::Wheel(Wheel::Up)), None],
        Style1 => [key(KeyCode::Digit1), None],
        Style2 => [key(KeyCode::Digit2), None],
        Style3 => [key(KeyCode::Digit3), None],
        Style4 => [key(KeyCode::Digit4), None],
        Style5 => [key(KeyCode::Digit5), None],
        Style6 => [key(KeyCode::Digit6), None],
        Style7 => [key(KeyCode::Digit7), None],
        // (V: a view key in many games, free in Kung Fu.)
        View => [key(KeyCode::KeyV), None],
        // A replay's keys (replay/), heard only there, so they may be the game's own. The flying camera moves on the walk's
        // keys, climbs on the jump's and descends on the crouch's; Shift flies fast. `?` is the help.
        PlayPause => [key(KeyCode::KeyP), key(KeyCode::KeyK)],
        Slower => [key(KeyCode::BracketLeft), key(KeyCode::Minus)],
        Faster => [key(KeyCode::BracketRight), key(KeyCode::Equal)],
        StepFrame => [key(KeyCode::Period), None],
        SeekBack => [key(KeyCode::ArrowLeft), None],
        SeekAhead => [key(KeyCode::ArrowRight), None],
        PreviousKill => [key(KeyCode::KeyB), None],
        NextKill => [key(KeyCode::KeyN), None],
        // (I as well: a browser keeps Tab for moving between the page's fields unless the viewer holds it.)
        Stats => [key(KeyCode::Tab), key(KeyCode::KeyI)],
        HideUi => [key(KeyCode::KeyU), None],
        ReplayHelp => [key(KeyCode::Slash), key(KeyCode::F1)],
        NameTags => [key(KeyCode::KeyT), None],
        DamageNumbers => [key(KeyCode::KeyY), None],
        Tracers => [key(KeyCode::KeyR), None],
        Minimap => [key(KeyCode::KeyM), None],
        CameraMode => [key(KeyCode::KeyV), None],
        NextPlayer => [key(KeyCode::KeyE), key(KeyCode::ArrowDown)],
        PreviousPlayer => [key(KeyCode::KeyQ), key(KeyCode::ArrowUp)],
        Watch1 => [key(KeyCode::Digit1), None],
        Watch2 => [key(KeyCode::Digit2), None],
        Watch3 => [key(KeyCode::Digit3), None],
        Watch4 => [key(KeyCode::Digit4), None],
        Watch5 => [key(KeyCode::Digit5), None],
        FlyForward => [key(KeyCode::KeyW), None],
        FlyBack => [key(KeyCode::KeyS), None],
        FlyLeft => [key(KeyCode::KeyA), None],
        FlyRight => [key(KeyCode::KeyD), None],
        FlyUp => [key(KeyCode::Space), None],
        FlyDown => [key(KeyCode::ControlLeft), key(KeyCode::KeyC)],
        FlyFast => [key(KeyCode::ShiftLeft), None],
    }
}
impl Bindings {
    pub fn get(&self, action: Action) -> [Option<Input>; 2] {
        self.slots[action.at()]
    }
    pub fn is_default(&self, action: Action) -> bool {
        self.get(action) == default_of(action)
    }
    /// Whether every action and the wheel are as they were to begin with.
    pub fn all_default(&self) -> bool {
        *self == Bindings::default()
    }
    pub fn has(&self, action: Action, input: Input) -> bool {
        self.get(action).contains(&Some(input))
    }
    /// The other action that `input` would clash with if `action` had it: one heard at the same time that has it.
    pub fn conflict(&self, action: Action, input: Input) -> Option<Conflict> {
        Action::ALL.into_iter().filter(|other| *other != action && other.context().overlaps(action.context())).find_map(|other| {
            self.get(other).iter().position(|slot| *slot == Some(input)).map(|slot| Conflict { action: other, slot })
        })
    }
    /// Every slot that clashes with another's (none, unless kept bindings were edited by hand).
    pub fn clashing(&self, action: Action, slot: usize) -> bool {
        self.get(action)[slot].is_some_and(|input| self.conflict(action, input).is_some())
    }
    /// Gives `action` `input` in `slot`, unless another action heard at the same time has it: then nothing changes
    /// and that action is returned. Already in the action's other slot, it moves.
    pub fn assign(&mut self, action: Action, slot: usize, input: Input) -> Result<(), Conflict> {
        if let Some(conflict) = self.conflict(action, input) {
            return Err(conflict);
        }
        self.put(action, slot, input);
        Ok(())
    }
    fn put(&mut self, action: Action, slot: usize, input: Input) {
        let slots = &mut self.slots[action.at()];
        if slots[1 - slot] == Some(input) {
            slots[1 - slot] = None;
        }
        slots[slot] = Some(input);
    }
    /// Gives `action` `input` in `slot`, and whatever action had it (heard at the same time) the input it replaces
    /// (or nothing, if the slot was empty): the two trade.
    pub fn swap(&mut self, action: Action, slot: usize, input: Input) {
        let replaced = self.get(action)[slot];
        while let Some(Conflict { action: other, slot: theirs }) = self.conflict(action, input) {
            // (What it trades for must not clash for it in turn; then it is left without.)
            let given = replaced.filter(|r| *r != input && self.conflict_but(other, *r, action).is_none());
            self.slots[other.at()][theirs] = given;
        }
        self.put(action, slot, input);
    }
    /// `conflict`, leaving `except` out (the action that is giving the input away).
    fn conflict_but(&self, action: Action, input: Input, except: Action) -> Option<Conflict> {
        Action::ALL.into_iter().filter(|other| *other != action && *other != except && other.context().overlaps(action.context())).find_map(|other| {
            self.get(other).iter().position(|slot| *slot == Some(input)).map(|slot| Conflict { action: other, slot })
        })
    }
    pub fn clear(&mut self, action: Action, slot: usize) {
        self.slots[action.at()][slot] = None;
    }
    /// The action's defaults back; an action that has one of them now trades it for what it replaces (`swap`).
    pub fn reset(&mut self, action: Action) {
        for (slot, input) in default_of(action).into_iter().enumerate() {
            match input {
                Some(input) => self.swap(action, slot, input),
                None => self.clear(action, slot),
            }
        }
    }
    /// Everything as it was to begin with, the wheel too.
    pub fn reset_all(&mut self) {
        *self = Bindings::default();
    }

    // ----- Reading.

    /// Whether `action` is held: any of its keys or buttons down.
    pub fn held(&self, action: Action, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        self.get(action).into_iter().flatten().any(|i| i.held(keys, mouse))
    }
    /// Whether `action` is held by one of its keys (not a mouse button: those need the pointer captured to count).
    pub fn held_by_key(&self, action: Action, keys: &ButtonInput<KeyCode>) -> bool {
        self.get(action).into_iter().flatten().any(|i| matches!(i, Input::Key(key) if keys.any_pressed(twins(key))))
    }
    /// Whether `action` was pressed this frame (by a key or a button; the wheel is `wheeled`).
    pub fn just(&self, action: Action, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        self.get(action).into_iter().flatten().any(|i| i.just(keys, mouse))
    }
    /// Whether this frame's step of the wheel (`step`: +1 a notch down, -1 up, 0 none, as the wheel is turned, before
    /// any inversion) is one of `action`'s.
    pub fn wheeled(&self, action: Action, step: i32) -> bool {
        self.notch(step).is_some_and(|w| self.has(action, Input::Wheel(w)))
    }
    /// The notch a step of the wheel is, inverted or not.
    pub fn notch(&self, step: i32) -> Option<Wheel> {
        let step = if self.invert_wheel { -step } else { step };
        match step.signum() {
            1 => Some(Wheel::Down),
            -1 => Some(Wheel::Up),
            _ => None,
        }
    }
    /// The key or button the touch controls press for `action` (touch.rs): its first that is not the wheel.
    pub fn pressable(&self, action: Action) -> Option<Input> {
        self.get(action).into_iter().flatten().find(|i| !matches!(i, Input::Wheel(_)))
    }
    /// A key cap's label for `action`'s main input (the head-up display's prompts), or None when it has none.
    pub fn cap(&self, action: Action) -> Option<Input> {
        self.get(action).into_iter().flatten().next()
    }

    // ----- Keeping.

    /// The actions that differ from the defaults, as JSON: `{"jump": ["KeyJ", ""]}`.
    pub fn to_text(&self) -> String {
        let mut out = serde_json::Map::new();
        for a in Action::ALL.into_iter().filter(|a| !self.is_default(*a)) {
            let slots = self.get(a).map(|s| serde_json::Value::String(s.map_or_else(String::new, Input::id)));
            out.insert(a.id().into(), serde_json::Value::Array(slots.to_vec()));
        }
        serde_json::Value::Object(out).to_string()
    }
    /// The bindings kept as `text` (`to_text`) over the defaults: unknown actions and inputs are left out, and a kept
    /// binding that would clash with one kept before it is dropped, so what is loaded never clashes.
    pub fn from_text(text: Option<&str>, invert_wheel: bool) -> Self {
        let mut out = Bindings { invert_wheel, ..default() };
        let Some(kept) = text.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()) else { return out };
        let Some(kept) = kept.as_object() else { return out };
        // First every kept action is cleared, then given its inputs, so that kept actions can trade defaults.
        let actions: Vec<(Action, [Option<Input>; 2])> = Action::ALL
            .into_iter()
            .filter_map(|a| {
                let slots = kept.get(a.id())?.as_array()?;
                let read = |i: usize| slots.get(i).and_then(|v| v.as_str()).and_then(Input::parse).filter(|i| i.bindable());
                Some((a, [read(0), read(1)]))
            })
            .collect();
        for (a, _) in &actions {
            out.slots[a.at()] = [None, None];
        }
        for (a, slots) in &actions {
            for (slot, input) in slots.iter().enumerate() {
                let Some(input) = input else { continue };
                if let Err(conflict) = out.assign(*a, slot, *input) {
                    // A default that a kept action took over goes; two kept actions that clash keep the first.
                    if actions.iter().any(|(b, _)| *b == conflict.action) {
                        continue;
                    }
                    out.clear(conflict.action, conflict.slot);
                    out.put(*a, slot, *input);
                }
            }
        }
        out
    }
}

/// Startup: the bindings kept from last time.
pub fn load(mut bindings: ResMut<Bindings>, prefs: Res<crate::prefs::Prefs>) {
    *bindings = Bindings::from_text(prefs.get(KEY).as_deref(), prefs.get(WHEEL_KEY).as_deref() == Some("inverted"));
}
/// Every frame: bindings that changed are kept for next time.
fn remember(bindings: Res<Bindings>, mut prefs: ResMut<crate::prefs::Prefs>, mut kept: Local<Option<(String, bool)>>) {
    let now = (bindings.to_text(), bindings.invert_wheel);
    match kept.as_ref() {
        None => *kept = Some(now),
        Some(was) if *was != now => {
            prefs.set(KEY, &now.0);
            prefs.set(WHEEL_KEY, if now.1 { "inverted" } else { "normal" });
            *kept = Some(now);
        }
        _ => {}
    }
}

// ----- The gate.

/// What the game is kept from hearing this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Block {
    #[default]
    Nothing,
    /// Only the mouse's buttons: the menu has closed on a click that is still held.
    Mouse,
    /// Every key, button, motion of the mouse, the wheel and the touch controls: the menu is open.
    All,
}
/// The real input put aside while the game sees none (`shut`), to be put back before the next frame's is read.
#[derive(Resource, Default)]
pub struct Gate {
    pub block: Block,
    keys: Option<ButtonInput<KeyCode>>,
    mouse: Option<ButtonInput<MouseButton>>,
}
/// PreUpdate, before Bevy reads the frame's input: the real state back, so that it reads on from it.
fn restore(mut gate: ResMut<Gate>, keys: Option<ResMut<ButtonInput<KeyCode>>>, mouse: Option<ResMut<ButtonInput<MouseButton>>>) {
    if let (Some(kept), Some(mut keys)) = (gate.keys.take(), keys) {
        *keys = kept;
    }
    if let (Some(kept), Some(mut mouse)) = (gate.mouse.take(), mouse) {
        *mouse = kept;
    }
}
/// PreUpdate, once the menu and Bevy's UI have read the input: what the gate says the game may not hear is put aside.
#[allow(clippy::type_complexity)]
pub fn shut(
    mut gate: ResMut<Gate>,
    keys: Option<ResMut<ButtonInput<KeyCode>>>,
    mouse: Option<ResMut<ButtonInput<MouseButton>>>,
    motion: Option<ResMut<AccumulatedMouseMotion>>,
    scroll: Option<ResMut<AccumulatedMouseScroll>>,
    pad: Option<ResMut<crate::touch::Pad>>,
) {
    if gate.block == Block::Nothing {
        return;
    }
    if let Some(mut mouse) = mouse {
        gate.mouse = Some(std::mem::take(&mut *mouse));
    }
    if gate.block != Block::All {
        return;
    }
    if let Some(mut keys) = keys {
        gate.keys = Some(std::mem::take(&mut *keys));
    }
    if let Some(mut motion) = motion {
        motion.delta = Vec2::ZERO;
    }
    if let Some(mut scroll) = scroll {
        scroll.delta = Vec2::ZERO;
    }
    if let Some(mut pad) = pad {
        (pad.stick, pad.look, pad.fire, pad.pulled, pad.pinch) = (Vec2::ZERO, Vec2::ZERO, false, false, 0.);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> (ButtonInput<KeyCode>, ButtonInput<MouseButton>) {
        (ButtonInput::default(), ButtonInput::default())
    }

    /// The defaults are the keys the game has always had: WASD, Space, Ctrl or C, T, Shift, the left and right buttons,
    /// R to reload (and in Forts' build phase to say ready), F, E, H, Enter, 1, 2, Y, the wheel down and up, Q, the left
    /// button again to throw, X.
    #[test]
    fn the_defaults_are_todays_keys() {
        let b = Bindings::default();
        let k = |k| Some(Input::Key(k));
        let table: [(Action, [Option<Input>; 2]); 22] = [
            (Action::Forward, [k(KeyCode::KeyW), None]),
            (Action::Back, [k(KeyCode::KeyS), None]),
            (Action::Left, [k(KeyCode::KeyA), None]),
            (Action::Right, [k(KeyCode::KeyD), None]),
            (Action::Jump, [k(KeyCode::Space), None]),
            (Action::Crouch, [k(KeyCode::ControlLeft), k(KeyCode::KeyC)]),
            (Action::Prone, [k(KeyCode::KeyT), None]),
            (Action::Stim, [k(KeyCode::ShiftLeft), None]),
            (Action::Fire, [Some(Input::Mouse(MouseButton::Left)), None]),
            (Action::Aim, [Some(Input::Mouse(MouseButton::Right)), None]),
            (Action::Reload, [k(KeyCode::KeyR), None]),
            (Action::Melee, [k(KeyCode::KeyF), None]),
            (Action::Use, [k(KeyCode::KeyE), None]),
            (Action::Ready, [k(KeyCode::KeyR), None]),
            (Action::Hints, [k(KeyCode::KeyH), None]),
            (Action::NewRound, [k(KeyCode::Enter), None]),
            (Action::Primary, [k(KeyCode::Digit1), None]),
            (Action::Secondary, [k(KeyCode::Digit2), None]),
            (Action::Swap, [k(KeyCode::KeyY), None]),
            (Action::Carry, [k(KeyCode::KeyQ), None]),
            (Action::Throw, [Some(Input::Mouse(MouseButton::Left)), None]),
            (Action::Tip, [k(KeyCode::KeyX), None]),
        ];
        for (action, slots) in table {
            assert_eq!(b.get(action), slots, "{action:?}");
        }
        assert_eq!(b.get(Action::NextWeapon), [Some(Input::Wheel(Wheel::Down)), None]);
        assert_eq!(b.get(Action::PreviousWeapon), [Some(Input::Wheel(Wheel::Up)), None]);
        assert_eq!(b.get(Action::TurnClockwise), [Some(Input::Wheel(Wheel::Down)), None], "rolled down turns it clockwise");
        assert_eq!(b.get(Action::TurnAnticlockwise), [Some(Input::Wheel(Wheel::Up)), None]);
        for a in [Action::LastWeapon].into_iter().chain(Action::GUNS) {
            assert_eq!(b.get(a), [None, None], "{a:?} is new, and has no key to begin with");
        }
        assert!(!b.invert_wheel);
        // Nothing clashes, and everything has a name and an id of its own.
        for a in Action::ALL {
            assert!(!b.clashing(a, 0) && !b.clashing(a, 1), "{a:?}");
            assert!(!a.name().is_empty() && a.name().is_ascii());
        }
        let mut ids: Vec<&str> = Action::ALL.iter().map(|a| a.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ACTIONS);
        // Read as the game read the keys: Ctrl either side or C crouches, Shift either side stims.
        let (mut keys, mouse) = input();
        for key in [KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::KeyC] {
            keys.press(key);
            assert!(b.held(Action::Crouch, &keys, &mouse), "{key:?}");
            keys.release_all();
            keys.clear();
        }
        keys.press(KeyCode::ShiftRight);
        assert!(b.just(Action::Stim, &keys, &mouse) && !b.just(Action::Jump, &keys, &mouse));
        // Every label is ASCII, and every key's name reads back.
        for (key, id, label) in KEYS {
            assert!(label.is_ascii() && !label.is_empty());
            assert_eq!(Input::parse(id), Some(Input::Key(*key)));
        }
        assert!(!Input::Key(KeyCode::Escape).bindable(), "Esc is the menu's");
    }

    /// R reloads, and in Forts' build phase (the guns locked, nothing to reload) says ready: the two share it and never
    /// clash, but either clashes with anything else heard with it. Both are on the controls page's combat tab.
    #[test]
    fn r_is_the_reload_and_in_the_build_phase_ready_and_they_share_it_without_a_clash() {
        let b = Bindings::default();
        let r = Input::Key(KeyCode::KeyR);
        assert!(b.has(Action::Reload, r) && b.has(Action::Ready, r));
        assert!(!b.clashing(Action::Reload, 0) && !b.clashing(Action::Ready, 0));
        assert_eq!(b.conflict(Action::Reload, r), None);
        assert_eq!(b.conflict(Action::Ready, r), None);
        assert!(!Action::Reload.context().overlaps(Action::Ready.context()));
        // Whatever else is heard with a gun in hand, or in the build phase, may not have R.
        assert!(b.conflict(Action::Melee, r).is_some() && b.conflict(Action::Swap, r).is_some() && b.conflict(Action::Carry, r).is_some());
        // Carrying, the reload is not heard: the carried object's keys may have R; ready, heard while building, may not.
        assert!(!Action::Reload.context().overlaps(Action::Tip.context()));
        assert_eq!(b.conflict(Action::Tip, r).map(|c| c.action), Some(Action::Ready));
        // Nor in Kung Fu.
        assert!(!Action::Reload.context().overlaps(Action::Kick.context()) && !Action::Ready.context().overlaps(Action::Kick.context()));
        // Both on the combat tab, and kept like any other.
        assert_eq!((Action::Reload.group(), Action::Ready.group(), Action::Reload.name(), Action::Reload.id()), (Group::Combat, Group::Combat, "RELOAD", "reload"));
        let mut kept = Bindings::default();
        kept.assign(Action::Reload, 0, Input::Key(KeyCode::KeyG)).unwrap();
        let read = Bindings::from_text(Some(&kept.to_text()), false);
        assert_eq!((read.get(Action::Reload), read.get(Action::Ready)), ([Some(Input::Key(KeyCode::KeyG)), None], [Some(r), None]));
    }

    /// Kung Fu's actions are on the keys of what they stand in for, which are not heard in Kung Fu, so nothing clashes:
    /// the left and right buttons, F, Q, E, Shift, T, the wheel and 1 to 7.
    #[test]
    fn kung_fus_defaults_share_the_keys_of_what_kung_fu_does_not_hear_and_clash_with_nothing() {
        let b = Bindings::default();
        let k = |k| Some(Input::Key(k));
        let mouse = |m| Some(Input::Mouse(m));
        let table: [(Action, [Option<Input>; 2]); 16] = [
            (Action::Light, [mouse(MouseButton::Left), None]),
            (Action::Heavy, [mouse(MouseButton::Right), None]),
            (Action::Kick, [k(KeyCode::KeyF), None]),
            (Action::Block, [k(KeyCode::KeyQ), None]),
            (Action::Grab, [k(KeyCode::KeyE), None]),
            (Action::Dodge, [k(KeyCode::ShiftLeft), None]),
            (Action::Special, [k(KeyCode::KeyT), None]),
            (Action::Style1, [k(KeyCode::Digit1), None]),
            (Action::Style2, [k(KeyCode::Digit2), None]),
            (Action::Style3, [k(KeyCode::Digit3), None]),
            (Action::Style4, [k(KeyCode::Digit4), None]),
            (Action::Style5, [k(KeyCode::Digit5), None]),
            (Action::Style6, [k(KeyCode::Digit6), None]),
            (Action::Style7, [k(KeyCode::Digit7), None]),
            (Action::NextStyle, [Some(Input::Wheel(Wheel::Down)), None]),
            (Action::PreviousStyle, [Some(Input::Wheel(Wheel::Up)), None]),
        ];
        for (action, slots) in table {
            assert_eq!(b.get(action), slots, "{action:?}");
            assert_eq!(action.context(), Context::KungFu);
            assert!(!b.clashing(action, 0) && !b.clashing(action, 1), "{action:?}");
        }
        assert_eq!(Action::STYLES.map(|a| a.style()), [0, 1, 2, 3, 4, 5, 6].map(Some));
        assert_eq!(Action::STYLES.map(|a| a.name()), ["TIGER", "CRANE", "SNAKE", "MANTIS", "MONKEY", "DRUNKEN", "TOAD"]);
        assert_eq!(Action::STYLES.len(), kitchen_sim::kungfu::STYLES);
        // Their own two tabs, and the wheel's inversion shown with the fight.
        assert!(Action::ALL.iter().filter(|a| a.group() == Group::KungFu).count() == 9 && Action::ALL.iter().filter(|a| a.group() == Group::Styles).count() == 7);
        assert!(Group::KungFu.wheels() && !Group::Styles.wheels() && Group::Weapons.wheels());
        // What they share is never heard in Kung Fu; what is heard there (walking, jumping, crouching, the hints) they may
        // not take.
        for (theirs, ours) in [(Action::Fire, Action::Light), (Action::Aim, Action::Heavy), (Action::Melee, Action::Kick), (Action::Carry, Action::Block), (Action::Use, Action::Grab), (Action::Stim, Action::Dodge), (Action::Prone, Action::Special), (Action::Primary, Action::Style1)] {
            assert!(!theirs.context().overlaps(ours.context()), "{theirs:?} {ours:?}");
        }
        assert_eq!(b.conflict(Action::Kick, Input::Key(KeyCode::Space)).map(|c| c.action), Some(Action::Jump));
        assert_eq!(b.conflict(Action::Block, Input::Key(KeyCode::KeyC)).map(|c| c.action), Some(Action::Crouch));
        assert_eq!(b.conflict(Action::Grab, Input::Key(KeyCode::KeyH)).map(|c| c.action), Some(Action::Hints));
        // Two of Kung Fu's own may not share; the stim and the carry key still clash with the guns' and everything else's.
        assert_eq!(b.conflict(Action::Kick, Input::Key(KeyCode::KeyQ)).map(|c| c.action), Some(Action::Block));
        assert_eq!(b.conflict(Action::Stim, Input::Mouse(MouseButton::Left)).map(|c| c.action), Some(Action::Fire));
        assert_eq!(b.conflict(Action::Carry, Input::Key(KeyCode::KeyX)).map(|c| c.action), Some(Action::Tip));
        // Kept and read back like the others.
        let mut kept = Bindings::default();
        // (KF6: G, not V, which is now the third-person view's.)
        kept.assign(Action::Special, 0, Input::Key(KeyCode::KeyG)).unwrap();
        kept.swap(Action::Block, 0, Input::Mouse(MouseButton::Middle));
        let read = Bindings::from_text(Some(&kept.to_text()), false);
        assert_eq!(read, kept);
        assert_eq!(read.get(Action::Special), [Some(Input::Key(KeyCode::KeyG)), None]);
        // The fight's tab says how the wheel turns.
        assert!(b.wheeled(Action::NextStyle, 1) && b.wheeled(Action::PreviousStyle, -1));
        // A strike held by a key counts without the pointer captured (arena.rs); one held by a mouse button does not here.
        let (mut keys, mut mouse) = input();
        mouse.press(MouseButton::Left);
        assert!(b.held(Action::Light, &keys, &mouse) && !b.held_by_key(Action::Light, &keys));
        let mut k = Bindings::default();
        k.assign(Action::Light, 1, Input::Key(KeyCode::KeyK)).unwrap();
        keys.press(KeyCode::KeyK);
        assert!(k.held_by_key(Action::Light, &keys));
    }

    #[test]
    fn a_rebind_is_kept_and_read_back_and_only_what_differs_is_kept() {
        let mut prefs = crate::prefs::Prefs::memory();
        let mut b = Bindings::default();
        assert_eq!(b.to_text(), "{}");
        b.assign(Action::Jump, 0, Input::Key(KeyCode::KeyJ)).unwrap();
        b.assign(Action::Jump, 1, Input::Mouse(MouseButton::Back)).unwrap();
        b.assign(Action::Gun6, 0, Input::Key(KeyCode::Digit6)).unwrap();
        b.clear(Action::Hints, 0);
        b.invert_wheel = true;
        prefs.set(KEY, &b.to_text());
        prefs.set(WHEEL_KEY, "inverted");
        // The next game.
        let mut app = App::new();
        app.insert_resource(prefs).init_resource::<Bindings>().add_systems(Startup, load).add_systems(Update, remember);
        app.update();
        let read = app.world().resource::<Bindings>().clone();
        assert_eq!(read, b);
        assert_eq!(read.get(Action::Jump), [Some(Input::Key(KeyCode::KeyJ)), Some(Input::Mouse(MouseButton::Back))]);
        assert_eq!(read.get(Action::Hints), [None, None]);
        let text: serde_json::Value = serde_json::from_str(&read.to_text()).unwrap();
        assert_eq!(text.as_object().unwrap().len(), 3, "only jump, the syrup rifle and the hints: {text}");
        // A change made in this game is kept for the next, and none is written when nothing changed.
        app.world_mut().resource_mut::<Bindings>().reset(Action::Jump);
        app.update();
        let prefs = app.world().resource::<crate::prefs::Prefs>();
        assert_eq!(Bindings::from_text(prefs.get(KEY).as_deref(), true).get(Action::Jump), default_of(Action::Jump));
        // Kept text that is broken, or names what does not exist, is the defaults (with what can be read).
        assert_eq!(Bindings::from_text(Some("not json"), false), Bindings::default());
        let odd = Bindings::from_text(Some(r#"{"jump":["Nope","Escape"],"warp":["KeyK"],"melee":["KeyG"]}"#), false);
        assert_eq!(odd.get(Action::Jump), [None, None]);
        assert_eq!(odd.get(Action::Melee), [Some(Input::Key(KeyCode::KeyG)), None]);
        // A kept binding that took over a default: the default's action goes without it, nothing clashes.
        let took = Bindings::from_text(Some(r#"{"stim":["KeyQ",""]}"#), false);
        assert_eq!(took.get(Action::Stim)[0], Some(Input::Key(KeyCode::KeyQ)));
        assert_eq!(took.get(Action::Carry), [None, None]);
        assert!(Action::ALL.into_iter().all(|a| !took.clashing(a, 0) && !took.clashing(a, 1)));
    }

    #[test]
    fn a_clash_is_refused_and_a_swap_trades_the_two() {
        let mut b = Bindings::default();
        // C is crouch's second key: jump may not have it.
        let c = Input::Key(KeyCode::KeyC);
        assert_eq!(b.assign(Action::Jump, 0, c), Err(Conflict { action: Action::Crouch, slot: 1 }));
        assert_eq!(b, Bindings::default(), "a refusal changes nothing");
        // Swapped: jump has C, and crouch Space (what C replaced).
        b.swap(Action::Jump, 0, c);
        assert_eq!(b.get(Action::Jump), [Some(c), None]);
        assert_eq!(b.get(Action::Crouch), [Some(Input::Key(KeyCode::ControlLeft)), Some(Input::Key(KeyCode::Space))]);
        // Into an empty slot: the other is left without.
        b.swap(Action::Jump, 1, Input::Key(KeyCode::KeyT));
        assert_eq!(b.get(Action::Prone), [None, None]);
        // The left button is both the trigger and the throw, never heard at once; the wheel both the guns and the turn.
        assert_eq!(b.conflict(Action::Throw, Input::Mouse(MouseButton::Left)), None);
        assert_eq!(b.conflict(Action::TurnClockwise, Input::Wheel(Wheel::Down)), None);
        // But a key heard always clashes with both.
        assert_eq!(b.conflict(Action::Use, Input::Mouse(MouseButton::Left)).map(|c| c.action), Some(Action::Fire));
        // A key in the action's other slot moves, it is no clash.
        let mut b = Bindings::default();
        b.assign(Action::Crouch, 0, Input::Key(KeyCode::KeyC)).unwrap();
        assert_eq!(b.get(Action::Crouch), [Some(c), None]);
        // A reset trades back.
        let mut b = Bindings::default();
        b.swap(Action::Jump, 0, Input::Key(KeyCode::KeyQ));
        assert_eq!(b.get(Action::Carry), [Some(Input::Key(KeyCode::Space)), None]);
        b.reset(Action::Jump);
        assert_eq!(b, Bindings::default());
        // A right Shift bound is the Shift key.
        let mut b = Bindings::default();
        assert_eq!(b.assign(Action::Jump, 0, Input::Key(plain(KeyCode::ShiftRight))).map_err(|c| c.action), Err(Action::Stim));
        b.reset_all();
        assert!(b.all_default());
    }

    #[test]
    fn the_wheel_inverted_turns_the_other_way() {
        let mut b = Bindings::default();
        // Rolled down (+1) is the next gun, up (-1) the one before; nothing is nothing.
        assert!(b.wheeled(Action::NextWeapon, 1) && !b.wheeled(Action::NextWeapon, -1) && !b.wheeled(Action::NextWeapon, 0));
        assert!(b.wheeled(Action::PreviousWeapon, -1) && !b.wheeled(Action::PreviousWeapon, 1));
        b.invert_wheel = true;
        assert!(b.wheeled(Action::NextWeapon, -1) && !b.wheeled(Action::NextWeapon, 1), "inverted: up is the next");
        assert!(b.wheeled(Action::PreviousWeapon, 1));
        assert!(b.wheeled(Action::TurnClockwise, -1), "and the turn of what is carried goes with it");
        // Bound to a key, the wheel does nothing for it.
        b.invert_wheel = false;
        b.assign(Action::NextWeapon, 0, Input::Key(KeyCode::KeyG)).unwrap();
        assert!(!b.wheeled(Action::NextWeapon, 1));
        let (mut keys, mouse) = input();
        keys.press(KeyCode::KeyG);
        assert!(b.just(Action::NextWeapon, &keys, &mouse));
    }

    /// While the gate is shut the game hears nothing, and the real state comes back for the next frame's input to be
    /// read on from: a key held through the menu is still held, one let go in it is let go.
    #[test]
    fn the_gate_hides_the_input_and_gives_it_back() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Gate>()
            .add_systems(PreUpdate, (restore, shut).chain());
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(5., 5.);
        app.world_mut().resource_mut::<Gate>().block = Block::All;
        app.update();
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW), "shut");
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        assert_eq!(app.world().resource::<AccumulatedMouseMotion>().delta, Vec2::ZERO);
        // Open: the real state again.
        app.world_mut().resource_mut::<Gate>().block = Block::Nothing;
        app.update();
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW), "still held");
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        // Only the mouse.
        app.world_mut().resource_mut::<Gate>().block = Block::Mouse;
        app.update();
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! kitchen-ffi: a stable C interface to kitchen-sim, Hotdog Simulator's shared deterministic simulation, so that a client
//! written in another language (the Unreal client) links the same simulation the Bevy client and the server run instead
//! of rewriting it.
//!
//! Everything here is `extern "C"` over `#[repr(C)]` plain-old-data: an opaque handle (`KsGame`) for a game, offline
//! (the simulation steps here, bots and all) or online (frames from the server decoded here, commands encoded here),
//! and snapshots the caller copies out into arrays it owns. The header (`include/kitchen_sim.h`) is generated from this
//! file by cbindgen (`tools/build-ffi.sh`); its top comment, in `cbindgen.toml`, documents the coordinate system, the
//! units, the lifetimes and the per-frame call order.
//!
//! No panic crosses the boundary: every entry point runs inside `catch_unwind` and returns a `KsStatus`; the reason for
//! the last failure on the calling thread is `ks_last_error`. A handle is checked against the set of live handles before
//! it is touched, so a null, stale or made-up pointer is refused instead of followed.
#![allow(clippy::missing_safety_doc)]
#![allow(non_camel_case_types)]

mod events;
mod session;
#[cfg(test)]
mod tests;

use kitchen_sim::feast::{self, Stage};
use kitchen_sim::kungfu::{self, Move};
use kitchen_sim::map::{self, Prop, Side, Zone};
use kitchen_sim::net;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use session::Game;

// ================================================================================================================
// Constants (cbindgen writes each as a #define)
// ================================================================================================================

/// The version of this interface: bumped whenever a struct's layout or a function's meaning changes incompatibly.
pub const KS_ABI_VERSION: u32 = 1;

/// `ks_game_create` flags. ALL_BOTS: the local player's hotdog (id 1) is played by a bot too (a bot match: commands are
/// then ignored). FOLLOW: a finished round is followed by the next by itself eight seconds later, as on the server
/// (otherwise the round stays over until `ks_game_restart`).
pub const KS_GAME_ALL_BOTS: u32 = 1;
pub const KS_GAME_FOLLOW: u32 = 2;

/// `KsCommand::buttons`, the bits of kitchen_sim::net::button. CROUCH, PRONE, FIRE, SCOPE and the pose bits are held
/// (levels: keep them set while held); JUMP, STIM, MELEE, EAT, GRAB and THROW are edges (set them in the one command
/// after the press: the library sends each edge once). In Kung Fu GRAB is the block and is a level.
/// Kung Fu reads them as: FIRE light, SCOPE heavy, MELEE kick, PRONE special, GRAB block (held), THROW or EAT grab,
/// STIM dodge (direction from the stick), JUMP jump, CROUCH crouch (an attack crouched is the sweep).
pub const KS_BUTTON_CROUCH: u32 = 1;
pub const KS_BUTTON_PRONE: u32 = 2;
pub const KS_BUTTON_FIRE: u32 = 4;
pub const KS_BUTTON_JUMP: u32 = 8;
pub const KS_BUTTON_STIM: u32 = 16;
pub const KS_BUTTON_MELEE: u32 = 32;
pub const KS_BUTTON_EAT: u32 = 64;
pub const KS_BUTTON_GRAB: u32 = 128;
pub const KS_BUTTON_THROW: u32 = 256;
pub const KS_BUTTON_SCOPE: u32 = 1024;
/// How a carried object is held: its turn in eighths of a turn (0..7) at bits 11..13, its tip (0..2) at bits 14..15.
pub const KS_BUTTON_POSE_SHIFT: u32 = 11;
pub const KS_BUTTON_TURN_MASK: u32 = 7 << 11;
pub const KS_BUTTON_TIP_MASK: u32 = 3 << 14;
/// R, reload the gun in hand (with the hands free; set it in the one command after the press): both tip bits at once,
/// a tip no pose has (kitchen_sim::net::button::RELOAD). Carrying something, set the pose instead.
pub const KS_BUTTON_RELOAD: u32 = 3 << 14;
pub const KS_BUTTON_ALL: u32 = 511 | 1024 | (7 << 11) | (3 << 14);
pub const KS_BUTTON_EDGES: u32 = 8 | 16 | 32 | 64 | 128 | 256;

/// `KsMatch::mode`.
pub const KS_MODE_SIEGE: u32 = 0;
pub const KS_MODE_FORTS: u32 = 1;
pub const KS_MODE_FFA: u32 = 2;
pub const KS_MODE_KUNGFU: u32 = 3;
pub const KS_MODE_CARDS: u32 = 4;

/// `KsMatch::stage`, as net::STAGES numbers them (Lobby and Opening only online, from the server's lobby).
pub const KS_STAGE_LOBBY: u32 = 0;
pub const KS_STAGE_OPENING: u32 = 1;
pub const KS_STAGE_WARMUP: u32 = 2;
pub const KS_STAGE_WAVE: u32 = 3;
pub const KS_STAGE_FREE_FOR_ALL: u32 = 4;
pub const KS_STAGE_ROUND_OVER: u32 = 5;
pub const KS_STAGE_BUILD: u32 = 6;
pub const KS_STAGE_FIGHT: u32 = 7;

/// `KsPlayer::flags`.
pub const KS_PLAYER_ALIVE: u32 = 1;
pub const KS_PLAYER_BOT: u32 = 2;
/// Online: a person plays this seat (the frame's `human`); offline: not a bot.
pub const KS_PLAYER_HUMAN: u32 = 4;
/// This client's own hotdog.
pub const KS_PLAYER_LOCAL: u32 = 8;
pub const KS_PLAYER_CONSUMED: u32 = 16;
pub const KS_PLAYER_INFECTED: u32 = 32;
pub const KS_PLAYER_AIMED: u32 = 64;
pub const KS_PLAYER_GROUNDED: u32 = 128;
pub const KS_PLAYER_LAUNCHED: u32 = 256;
pub const KS_PLAYER_FALLEN: u32 = 512;
pub const KS_PLAYER_READY: u32 = 1024;
/// Online: the seat's person waits in the fridge for the next round.
pub const KS_PLAYER_WAITING: u32 = 2048;
/// Online: `p` is this client's prediction of its own hotdog (ahead of the server), not the frame's.
pub const KS_PLAYER_PREDICTED: u32 = 4096;
pub const KS_PLAYER_BENCHED: u32 = 8192;

/// `KsPlayer::stance`.
pub const KS_STANCE_STAND: u32 = 0;
pub const KS_STANCE_CROUCH: u32 = 1;
pub const KS_STANCE_PRONE: u32 = 2;

/// Weapons (`KsPlayer::weapon`, `KsProjectile::weapon`): 0 melee, 1 mustard pistol, 2 ketchup sniper, 3 hot sauce
/// thrower, 4 relish bazooka, 5 bug spray, 6 syrup rifle, 7 mayonnaise. In Kung Fu `weapon` is the style + 1 (1..7).
pub const KS_WEAPONS: u32 = 8;

/// `KsDamage::kind`: 0 none, 1..5 a round of that weapon, then these (feast.rs).
pub const KS_DAMAGE_MELEE: u32 = 6;
pub const KS_DAMAGE_TURRET: u32 = 7;
pub const KS_DAMAGE_ZOMBIE: u32 = 8;
pub const KS_DAMAGE_PESTS: u32 = 9;
pub const KS_DAMAGE_HUNGER: u32 = 10;
pub const KS_DAMAGE_FALL: u32 = 11;
pub const KS_DAMAGE_SYRUP: u32 = 12;
pub const KS_DAMAGE_MAYONNAISE: u32 = 13;
pub const KS_DAMAGE_BACKSTAB: u32 = 14;
pub const KS_DAMAGE_BURN: u32 = 15;
pub const KS_DAMAGE_BLOW: u32 = 16;

/// `KsProp::state` (props.rs State).
pub const KS_PROP_AWAKE: u32 = 0;
pub const KS_PROP_ASLEEP: u32 = 1;
pub const KS_PROP_CARRIED: u32 = 2;
pub const KS_PROP_LOST: u32 = 3;
pub const KS_PROP_REMOVED: u32 = 4;
pub const KS_PROP_DRAPED: u32 = 5;
pub const KS_PROP_LOCKED: u32 = 6;

/// `KsMapProp::kind`.
pub const KS_MAP_PROP_BOX: u32 = 0;
pub const KS_MAP_PROP_TURRET: u32 = 1;
pub const KS_MAP_PROP_BIN: u32 = 2;
/// `KsZone::kind`.
pub const KS_ZONE_BASE: u32 = 0;
pub const KS_ZONE_FIELD: u32 = 1;
pub const KS_ZONE_BACON: u32 = 2;
/// A side (`KsBlock::facing`, `KsRamp::rise`): -1 none.
pub const KS_SIDE_PLUS_X: i32 = 0;
pub const KS_SIDE_MINUS_X: i32 = 1;
pub const KS_SIDE_PLUS_Z: i32 = 2;
pub const KS_SIDE_MINUS_Z: i32 = 3;

/// `KsForts::bacon_state`.
pub const KS_BACON_HOME: u32 = 0;
pub const KS_BACON_HELD: u32 = 1;
pub const KS_BACON_DROPPED: u32 = 2;

/// `KsFighter::flags`.
pub const KS_FIGHTER_CONNECTED: u32 = 1;
pub const KS_FIGHTER_STUNNED: u32 = 2;
pub const KS_FIGHTER_DOWN: u32 = 4;
pub const KS_FIGHTER_INVULNERABLE: u32 = 8;
pub const KS_FIGHTER_IRON: u32 = 16;
/// May start a move or jump now (kungfu::free).
pub const KS_FIGHTER_FREE: u32 = 32;
/// Dead: listed with move 0 and nothing else set.
pub const KS_FIGHTER_DEAD: u32 = 64;
/// `KsFighter::phase` of its move at the clock.
pub const KS_PHASE_LOOP: u32 = 0;
pub const KS_PHASE_STARTUP: u32 = 1;
pub const KS_PHASE_ACTIVE: u32 = 2;
pub const KS_PHASE_RECOVERY: u32 = 3;
pub const KS_PHASE_OVER: u32 = 4;
/// `KsFighter::block`.
pub const KS_BLOCK_NONE: u32 = 0;
pub const KS_BLOCK_HIGH: u32 = 1;
pub const KS_BLOCK_LOW: u32 = 2;

/// `KsMove::kind` (kungfu::Kind in order).
pub const KS_MOVE_STANCE: u32 = 0;
pub const KS_MOVE_BLOCK: u32 = 1;
pub const KS_MOVE_PARRY: u32 = 2;
pub const KS_MOVE_DODGE: u32 = 3;
pub const KS_MOVE_REACTION: u32 = 4;
pub const KS_MOVE_KNOCKDOWN: u32 = 5;
pub const KS_MOVE_GETUP: u32 = 6;
pub const KS_MOVE_GRAB: u32 = 7;
pub const KS_MOVE_THROWN: u32 = 8;
pub const KS_MOVE_VICTORY: u32 = 9;
pub const KS_MOVE_LIGHT: u32 = 10;
pub const KS_MOVE_HEAVY: u32 = 11;
pub const KS_MOVE_KICK: u32 = 12;
pub const KS_MOVE_JUMP: u32 = 13;
pub const KS_MOVE_SWEEP: u32 = 14;
pub const KS_MOVE_SPECIAL: u32 = 15;
/// `KsMove::height`.
pub const KS_HEIGHT_HIGH: u32 = 0;
pub const KS_HEIGHT_MID: u32 = 1;
pub const KS_HEIGHT_LOW: u32 = 2;
/// `KsMove::guard`.
pub const KS_GUARD_BLOCKABLE: u32 = 0;
pub const KS_GUARD_BREAK: u32 = 1;
pub const KS_GUARD_UNBLOCKABLE: u32 = 2;
pub const KS_GUARD_GRAB: u32 = 3;

/// `KsEvent::kind`. See `KsEvent` for what each field means for each kind.
pub const KS_EVENT_SHOT: u32 = 1;
pub const KS_EVENT_SPLAT: u32 = 2;
pub const KS_EVENT_KNOCK: u32 = 3;
pub const KS_EVENT_DAMAGE: u32 = 4;
pub const KS_EVENT_KILL: u32 = 5;
pub const KS_EVENT_SPAWN: u32 = 6;
pub const KS_EVENT_STAGE: u32 = 7;
pub const KS_EVENT_ROUND: u32 = 8;
pub const KS_EVENT_WIN: u32 = 9;
pub const KS_EVENT_GRIDDLE: u32 = 10;
pub const KS_EVENT_MAP: u32 = 11;
/// Kung Fu: a fighter's move began (`weapon` the move id).
pub const KS_EVENT_FU_MOVE: u32 = 20;
/// Kung Fu's own events (kungfu::EventKind, `id` its number in the fight).
pub const KS_EVENT_FU_HIT: u32 = 21;
pub const KS_EVENT_FU_BLOCKED: u32 = 22;
pub const KS_EVENT_FU_PARRIED: u32 = 23;
pub const KS_EVENT_FU_GUARDBROKEN: u32 = 24;
pub const KS_EVENT_FU_GRABBED: u32 = 25;
pub const KS_EVENT_FU_KNOCKDOWN: u32 = 26;
pub const KS_EVENT_FU_KO: u32 = 27;
pub const KS_EVENT_FU_DODGED: u32 = 28;
pub const KS_EVENT_FU_ABSORBED: u32 = 29;
pub const KS_EVENT_FU_SPECIAL: u32 = 30;
/// Kung Fu, this client's own fighter: its chi just filled; its style changed.
pub const KS_EVENT_FU_CHI_FULL: u32 = 31;
pub const KS_EVENT_FU_STYLE: u32 = 32;
/// `KsEvent::flags`.
pub const KS_EVENT_FLAG_RING: u32 = 1;
pub const KS_EVENT_FLAG_ON: u32 = 2;
pub const KS_EVENT_FLAG_HOSTILE: u32 = 4;
pub const KS_EVENT_FLAG_BEAM: u32 = 8;
/// DAMAGE and KILL: the round struck the victim's face (the ketchup sniper's kills from full health there); `weapon` is
/// the gun's kind as for any other hit.
pub const KS_EVENT_FLAG_FACE: u32 = 16;

/// `KsEvent::sound`: what the Bevy client plays for it (`ks_sound_file` names the file under assets/).
pub const KS_SOUND_NONE: u32 = 0;
pub const KS_SOUND_PEW: u32 = 1;
pub const KS_SOUND_SQUIRT: u32 = 2;
pub const KS_SOUND_BOOM: u32 = 3;
pub const KS_SOUND_BITE: u32 = 4;
pub const KS_SOUND_DING: u32 = 5;
pub const KS_SOUND_CLICK: u32 = 6;
pub const KS_SOUND_FU_WHOOSH: u32 = 7;
pub const KS_SOUND_FU_WHOOSH_HEAVY: u32 = 8;
pub const KS_SOUND_FU_HIT: u32 = 9;
pub const KS_SOUND_FU_HIT_HEAVY: u32 = 10;
pub const KS_SOUND_FU_BLOCK: u32 = 11;
pub const KS_SOUND_FU_PARRY: u32 = 12;
pub const KS_SOUND_FU_GUARDBREAK: u32 = 13;
pub const KS_SOUND_FU_GRAB: u32 = 14;
pub const KS_SOUND_FU_THROW: u32 = 15;
pub const KS_SOUND_FU_GONG: u32 = 16;
pub const KS_SOUND_FU_CHI: u32 = 17;
pub const KS_SOUND_FU_STYLE: u32 = 18;
pub const KS_SOUND_FU_CLANG: u32 = 19;
pub const KS_SOUND_COUNT: u32 = 20;

/// `KsMessage::kind`.
pub const KS_MESSAGE_NONE: u32 = 0;
pub const KS_MESSAGE_TEXT: u32 = 1;
pub const KS_MESSAGE_BINARY: u32 = 2;

/// `ks_cards_command` actions.
pub const KS_CARDS_FLIP: u32 = 0;
pub const KS_CARDS_STOP: u32 = 1;
pub const KS_CARDS_TARGET: u32 = 2;

// ================================================================================================================
// Types
// ================================================================================================================

/// What every function returns. Anything but KS_OK leaves a reason for `ks_last_error`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KsStatus {
    KS_OK = 0,
    /// A pointer that must not be null was null.
    KS_ERR_NULL = 1,
    /// An argument out of range or not a number, or a handle that is not a live game.
    KS_ERR_INVALID_ARGUMENT = 2,
    /// A string was not UTF-8.
    KS_ERR_UTF8 = 3,
    /// The map JSON could not be read or checked (`ks_last_error` says what is wrong with it).
    KS_ERR_MAP = 4,
    /// A network message could not be read (a frame that did not decode or check out: a whole frame is asked for).
    KS_ERR_MALFORMED = 5,
    /// The call is for the other kind of game (an offline-only call on an online game, or the reverse).
    KS_ERR_WRONG_MODE = 6,
    /// Online, before the server's welcome: there is no map yet.
    KS_ERR_NOT_READY = 7,
    /// No such thing (a player, a move, a style).
    KS_ERR_NOT_FOUND = 8,
    /// The simulation refused it (not the time for it, out of reach, not your go).
    KS_ERR_REFUSED = 9,
    /// A Rust panic was caught at the boundary. The handle may be inconsistent: destroy it.
    KS_ERR_PANIC = 10,
}

/// A UTF-8 string: a pointer and a length in bytes, never NUL-terminated unless said (`ks_last_error` is). A null
/// pointer with length 0 is the empty string. Strings the library returns are owned by it: see each function for how
/// long they stay valid.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsStr {
    pub ptr: *const u8,
    pub len: usize,
}
impl Default for KsStr {
    fn default() -> KsStr {
        KsStr::EMPTY
    }
}
impl KsStr {
    pub const EMPTY: KsStr = KsStr { ptr: std::ptr::null(), len: 0 };
    pub fn of(s: &str) -> KsStr {
        KsStr { ptr: s.as_ptr(), len: s.len() }
    }
}

/// The library's version.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsVersion {
    /// KS_ABI_VERSION of the library linked: compare it with the header's.
    pub abi: u32,
    /// The network protocol (kitchen_sim::net::PROTOCOL): the server must speak the same.
    pub protocol: u32,
    /// Steps a second the server simulates and frames a second it sends, by default (the welcome says the server's).
    pub tick_hz: u32,
    pub send_hz: u32,
    /// The crate's version ("0.1.0"), static.
    pub version: KsStr,
}

/// One step of the local player's input: the same fields as a network command (kitchen_sim::net::Command).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsCommand {
    /// The stick in the hotdog's own frame, each -1..1: x to the right, y backwards (forward is y = -1). WASD is a unit
    /// vector. Longer than 1 is cut to 1.
    pub stick_x: f32,
    pub stick_y: f32,
    /// Where it looks, radians: the yaw (it faces (-sin yaw, 0, -cos yaw)) and the pitch (up positive, -1.5..1.5).
    pub yaw: f32,
    pub pitch: f32,
    /// KS_BUTTON_* bits held now, plus the edges pressed since the last call.
    pub buttons: u32,
    /// The gun to hold, 1..7, or 0 to keep the one in hand. Kung Fu: the style + 1 (1..7), 0 to keep.
    pub weapon: u32,
    /// A pickup to collect (its index, `KsPickup::id`), or -1.
    pub pickup: i32,
}

/// What last hurt a hotdog (feast::Damage).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsDamage {
    /// 0 none, 1..5 a round of that weapon, KS_DAMAGE_* otherwise.
    pub kind: u32,
    /// The hotdog that did it, or 0.
    pub attacker: u32,
    /// Where it came from, and the way it travelled (unit vector, or zero).
    pub p: [f32; 3],
    pub d: [f32; 3],
    /// The round's clock when it landed, and the HP it took.
    pub time: f32,
    pub amount: f32,
}

/// A hotdog.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsPlayer {
    /// 1.. (its seat is id - 1).
    pub id: u32,
    /// Forts: 0 the Mustard Eaters, 1 the Ketchup Connoisseurs (0 otherwise).
    pub team: u32,
    /// KS_PLAYER_* bits.
    pub flags: u32,
    /// KS_STANCE_*.
    pub stance: u32,
    /// Its feet, metres, world space; its velocity over the last step (m/s).
    pub p: [f32; 3],
    pub v: [f32; 3],
    /// Radians: it faces (-sin yaw, 0, -cos yaw); pitch up positive.
    pub yaw: f32,
    pub pitch: f32,
    /// 0..100.
    pub hp: f32,
    /// The gun in hand (0..7; Kung Fu: style + 1) and the gun most recently in hand.
    pub weapon: u32,
    pub gun: u32,
    /// Bit w set: it owns weapon w.
    pub owned: u32,
    /// Rounds in each weapon's magazine (kitchen_sim::magazine; -1 for the strike, which needs none).
    pub ammo: [i32; 8],
    /// The loose object it carries (`KsProp::id`), or -1; and how it holds it (turn | tip << 3).
    pub carry: i32,
    pub carry_pose: u32,
    pub score: u32,
    pub kills: u32,
    pub deaths: u32,
    pub meals: u32,
    /// Melee attacks so far this round (a change means it swung).
    pub swings: u32,
    /// Seconds of stim left and until it can be used again; seconds until it is back (dead); spawn protection left.
    pub stim: f32,
    pub stim_wait: f32,
    pub respawn: f32,
    pub shield: f32,
    /// Online: how loudly its person speaks (0..1).
    pub talk: f32,
    /// Its name (UTF-8).
    pub name: KsStr,
    /// Online: the colour its person chose ("red", "blue", "green", "yellow", "black", or empty for a bot).
    pub color: KsStr,
    pub last_damage: KsDamage,
}

/// A zombie alley cat.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsEnemy {
    pub id: u32,
    pub p: [f32; 3],
    pub yaw: f32,
    pub hp: f32,
    /// The round's clock when it was born, and the seconds until it has risen out of its puddle (0: formed).
    pub born: f32,
    pub forming: f32,
}

/// A round in flight.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsProjectile {
    pub id: u32,
    pub owner: u32,
    pub weapon: u32,
    /// Launched in a free for all: it can hurt hotdogs.
    pub hostile: u32,
    pub p: [f32; 3],
    pub v: [f32; 3],
    pub travelled: f32,
    /// The round's clock when it left the muzzle, where, and its velocity then (a falling round, the mayonnaise, is on
    /// the arc these give: p(t) = from + launch * t + 0.5 * g * t^2, g = (0, -9.81, 0)).
    pub born: f32,
    pub from: [f32; 3],
    pub launch: [f32; 3],
}

/// A loose object (the bowl, a crate, a pillow...).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsProp {
    /// Its number (its place among the map's loose objects).
    pub id: u32,
    /// Its kind: an index into map::LOOSE (`ks_prop_kind_name`).
    pub kind: u32,
    /// KS_PROP_*.
    pub state: u32,
    /// The hotdog carrying it, or -1.
    pub holder: i32,
    /// Its origin (where the map stands it from), metres; its rotation as a quaternion [x, y, z, w].
    pub p: [f32; 3],
    pub q: [f32; 4],
    /// Its linear (m/s) and angular (rad/s) velocity.
    pub v: [f32; 3],
    pub w: [f32; 3],
}

/// A pickup's spawn point and what it holds now.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsPickup {
    pub id: u32,
    /// 1..7 a gun, 0 a box of ammunition.
    pub weapon: u32,
    pub ammo: i32,
    /// 1 when it can be taken now.
    pub available: u32,
    pub p: [f32; 3],
    /// The round's clock it is back from, and how long it takes to come back once taken.
    pub ready: f32,
    pub respawn: f32,
}

/// A turret: its post and the yaw it has turned to.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsTurret {
    /// The post on the counter [x, z], the height of its muzzle above the counter, and its yaw now.
    pub post: [f32; 2],
    pub base: f32,
    pub yaw: f32,
    /// Offline: its beam's origin and direction now (zero online).
    pub origin: [f32; 3],
    pub direction: [f32; 3],
}

/// A swarm of pests over a camped spot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsSwarm {
    pub id: u32,
    pub p: [f32; 3],
    pub hp: f32,
    pub born: f32,
    pub empty: f32,
}

/// One puddle of a zombie emergence (the microwave's ooze): see emergence.rs.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsPuddle {
    /// The emergence's number, and when it began (the ding) and is forgotten, on the round's clock.
    pub emergence: u32,
    pub started: f32,
    pub ends: f32,
    /// The map's spawn point it is at, where, and the microwave block feeding it (-1: it wells up).
    pub spawn: u32,
    pub p: [f32; 3],
    pub source: i32,
    /// Seconds after `started` the liquid reaches it, and its last zombie has formed; zombies it forms.
    pub arrive: f32,
    pub until: f32,
    pub count: u32,
}

/// A napkin draped over loose objects (drape.rs): a wall of the level.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsDrape {
    pub id: u32,
    pub napkin: u32,
    pub props: u32,
    pub rect: [f32; 4],
    pub top: f32,
}

/// An electric griddle: its state and where it is.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsGriddle {
    /// Its block in the map (`ks_map_blocks`).
    pub block: u32,
    pub on: u32,
    /// Degrees Celsius (21 room .. 205 hot), and the HP a second it burns a hotdog standing on it.
    pub temp: f32,
    pub burn_rate: f32,
    /// The hotdog that last switched it on, or 0.
    pub by: u32,
    /// The plate [min x, min z, max x, max z], the height feet stand at on it (world), the dial (world) and the way the
    /// dial's side faces [x, z].
    pub rect: [f32; 4],
    pub top: f32,
    pub dial: [f32; 3],
    pub front: [f32; 2],
}

/// What the round is doing.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsMatch {
    /// KS_MODE_*, and whether the dev sandbox's rules are on.
    pub mode: u32,
    pub sandbox: u32,
    /// KS_STAGE_*.
    pub stage: u32,
    pub round: u32,
    /// The round's clock, seconds (online: the newest frame's).
    pub elapsed: f32,
    /// The clock at which the warm-up ends, and the famine began.
    pub warmup: f32,
    pub famine_at: f32,
    /// Free For All and Kung Fu: the clock at time-up (0 none), and seconds left (-1 none).
    pub ends: f32,
    pub time_left: f32,
    /// Kills (Kung Fu: KOs) that win (0: none).
    pub goal: u32,
    /// The winner's id, or 0.
    pub winner: u32,
    pub wave_spawned: u32,
    pub wave_kills: u32,
    pub wave_size: u32,
    pub friendly_fire_events: u32,
    /// Online, the server's lobby: seconds left of the countdown (-1 none), seconds into the opening and its length.
    pub countdown: f32,
    pub opening: f32,
    pub opening_for: f32,
    /// This client's hotdog, and how many hotdogs there are.
    pub local_player: u32,
    pub players: u32,
    /// 1 when there is Forts, Kung Fu or a card table state to read.
    pub has_forts: u32,
    pub has_kungfu: u32,
    pub has_cards: u32,
    /// Bumped whenever the map changes (online: a welcome or a new map): read the static map again.
    pub map_generation: u32,
    /// The line at the top of the screen, and the rules (rules.rs text). Valid until the next mutating call.
    pub alert: KsStr,
    pub rules: KsStr,
}

/// Forts' state (forts.rs FortState) and its zones.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsForts {
    pub scores: [u32; 2],
    /// Rounds a match needs, and the match's number.
    pub rounds: u32,
    pub match_no: u32,
    /// Seconds a build phase lasts, and when this one ends on the round's clock.
    pub build: f32,
    pub build_ends: f32,
    /// The forcefield is up.
    pub field: u32,
    /// KS_BACON_*, who holds it (0 nobody), where it is, seconds it has lain dropped.
    pub bacon_state: u32,
    pub bacon_holder: u32,
    pub bacon_p: [f32; 3],
    pub bacon_idle: f32,
    /// Seconds until the next stalemate alarm, and alarms so far.
    pub stale: f32,
    pub alarms: u32,
    /// The last round's winning team (-1 nobody, -2 not decided), the match's champion team (-1 none).
    pub winner: i32,
    pub champion: i32,
    /// How the last round ended ("capture", "wipe" or empty). Valid until the next mutating call.
    pub reason: KsStr,
    /// The field's axis (0: it divides along x), its middle, its rectangle; the bases by team; each team's side of the
    /// field (-1 or 1); the bacon's spot [x, z].
    pub axis: u32,
    pub mid: f32,
    pub field_rect: [f32; 4],
    pub bases: [[f32; 4]; 2],
    pub sides: [i32; 2],
    pub bacon_spot: [f32; 2],
}

/// The map's static description. Rectangles are [min x, min z, max x, max z] in metres on the counter; heights are
/// above the counter top (world y = KsMapInfo::floor + height).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsMapInfo {
    pub name: KsStr,
    pub theme: KsStr,
    pub mode: KsStr,
    /// The counter top's world height (feast::FLOOR, 0.9025 m above the kitchen floor at y = 0).
    pub floor: f32,
    pub bounds: [f32; 4],
    pub cell: f32,
    pub clearance: f32,
    /// Seconds a gun's pickup and an ammunition box take to come back; a Forts build phase's length.
    pub respawn: [f32; 2],
    pub build: f32,
    pub cabinets: u32,
    pub blocks: u32,
    pub decks: u32,
    pub ramps: u32,
    pub sinks: u32,
    pub props: u32,
    pub loose: u32,
    pub starts: u32,
    pub spawns: u32,
    pub pickups: u32,
    pub zones: u32,
    pub griddles: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsBlock {
    pub rect: [f32; 4],
    pub height: f32,
    /// One of map::KITS or map::STYLES ("griddle", "microwave", "toaster", "wood", ...).
    pub style: KsStr,
    /// KS_SIDE_* an appliance's front faces, or -1.
    pub facing: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsDeck {
    pub rect: [f32; 4],
    pub height: f32,
    pub style: KsStr,
    /// 1: a round deck (style "bamboo"): its floor is the ellipse inside the rectangle.
    pub round: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsRamp {
    pub rect: [f32; 4],
    /// The height of its foot and its top.
    pub base: f32,
    pub height: f32,
    /// KS_SIDE_* it climbs towards.
    pub rise: i32,
    /// A plank of this material, or empty: the metal ramp with yellow edges.
    pub style: KsStr,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsSink {
    pub rect: [f32; 4],
    pub depth: f32,
}
/// Scenery: a box, a turret's post, a kitchen bin.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsMapProp {
    /// KS_MAP_PROP_*.
    pub kind: u32,
    /// A box's rectangle, its foot and height; a turret's or a bin's place [x, z] and yaw (a turret's base: its height).
    pub rect: [f32; 4],
    pub p: [f32; 2],
    pub base: f32,
    pub height: f32,
    pub yaw: f32,
    pub style: KsStr,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsStart {
    pub p: [f32; 2],
    pub yaw: f32,
    /// Forts: the team it is for, or -1.
    pub team: i32,
}
/// A zombie spawn point [x, z] on the counter.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsSpawn {
    pub p: [f32; 2],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsMapPickup {
    pub p: [f32; 2],
    pub weapon: u32,
    pub ammo: u32,
}
/// A loose object as the map places it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsMapLoose {
    pub kind: u32,
    pub p: [f32; 2],
    pub base: f32,
    pub yaw: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsZone {
    /// KS_ZONE_*; a base's team (-1 otherwise); a base's or the field's rectangle; the bacon's spot [x, z].
    pub kind: u32,
    pub team: i32,
    pub rect: [f32; 4],
    pub p: [f32; 2],
}

/// Kung Fu, the fight as a whole.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsKungfu {
    /// The round's clock the fighters are read at (`KsFighter::t` = clock - started).
    pub clock: f32,
    /// KOs to win, and the round's end on its clock (0 none).
    pub goal: u32,
    pub ends: f32,
    pub fighters: u32,
}
/// Kung Fu, one fighter (kungfu::FighterView, with the time into its move worked out).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsFighter {
    pub id: u32,
    /// Its style (0 Tiger, 1 Crane, 2 Snake, 3 Mantis, 4 Monkey, 5 Drunken, 6 Toad).
    pub style: u32,
    /// The move it is in (`ks_kungfu_move`), when it began on the round's clock, seconds into it now, how long it lasts
    /// this time (0 for a loop) and what its phases are scaled by.
    pub mv: u32,
    pub started: f32,
    pub t: f32,
    pub len: f32,
    pub scale: f32,
    /// KS_PHASE_* of the move at `t`.
    pub phase: u32,
    /// KS_FIGHTER_* bits.
    pub flags: u32,
    /// KS_BLOCK_*.
    pub block: u32,
    /// 0..100 each (chi 100: the special is ready).
    pub chi: f32,
    pub guard: f32,
    /// Hits in its current combo, and its grab partner (0 none).
    pub combo: u32,
    pub partner: u32,
    /// Its walking pace now, of the stance's (kungfu::pace: 0 in a move).
    pub pace: f32,
}
/// Kung Fu, one row of the move table (kungfu::TABLE).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsMove {
    pub id: u32,
    /// The move's identifier ("TigerLight1") and its display name ("Tiger Claw Swipe"). Static.
    pub key: KsStr,
    pub name: KsStr,
    /// KS_MOVE_* kind; its style (-1 for the shared moves 0..14) and slot within the style (-1 shared).
    pub kind: u32,
    pub style: i32,
    pub slot: i32,
    /// Seconds (a Drunken strike's are scaled by `KsFighter::scale`).
    pub startup: f32,
    pub active: f32,
    pub recovery: f32,
    pub total: f32,
    /// Metres centre to centre; radians either side of facing; HP; knock speed m/s; seconds of stun.
    pub reach: f32,
    pub arc: f32,
    pub damage: f32,
    pub pushback: f32,
    pub stun: f32,
    /// KS_GUARD_*, KS_HEIGHT_*.
    pub guard: u32,
    pub height: u32,
    /// Chi gained on landing (> 0) or its cost (< 0).
    pub chi: f32,
    pub knockdown: u32,
    /// Metres it carries the fighter (dodges: their distance).
    pub advance: f32,
    pub hits: u32,
    /// [from, to) seconds into it when it cannot be touched.
    pub iframes: [f32; 2],
    pub low: u32,
    pub armor: u32,
    /// The moves it chains into.
    pub chains: [u32; 4],
    pub chain_count: u32,
}
/// Kung Fu's numbers (kungfu.rs).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KsKungfuRules {
    pub styles: u32,
    pub moves: u32,
    pub kos: u32,
    pub time: f32,
    pub respawn: f32,
    pub chi_max: f32,
    pub guard_max: f32,
    pub event_life: f32,
    pub parry: f32,
    pub buffer: f32,
    pub throw_at: f32,
    pub stance_pace: f32,
    pub block_pace: f32,
    pub style_pace: [f32; 7],
}

/// One thing that happened. Fields by kind (unused ones are 0):
/// - SHOT: by the shooter, weapon, p the muzzle, q where it went, id the shot, value the round's id (0 for a turret's
///   beam: flag BEAM).
/// - SPLAT: a round ended or hit something: weapon, p, id, value the round's id.
/// - KNOCK: a loose object struck something: to the prop, value what it struck (another prop's number, -1 the ground,
///   -2 - n the map's block n), amount the closing speed m/s, p.
/// - DAMAGE: to the victim, by the attacker (0 none), weapon the damage kind (KS_DAMAGE_*), amount HP, p where it came
///   from, q its direction, flag FACE for a round in the face. Continuous harm (hunger, pests, burns) is not reported:
///   read the HP.
/// - KILL: to the victim, by the killer, weapon the damage kind, p where the victim was, flag FACE as for DAMAGE.
/// - SPAWN: to the hotdog back (or first) in the round, p where.
/// - STAGE: value the new KS_STAGE_*. ROUND: value the new round. WIN: by the winner. MAP: the map changed (online).
/// - GRIDDLE: value its block, flag ON when it was switched on, by who switched it on, amount its temperature.
/// - FU_MOVE: by the fighter, weapon the move. FU_*: id the fight's event number, by, to, weapon the move, amount (HP,
///   or guard for BLOCKED), p the struck fighter's chest, flag RING for a ring-out KO, value the KO's finishing hotdog.
/// - FU_CHI_FULL, FU_STYLE: by this client's fighter (FU_STYLE: value the new style).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KsEvent {
    pub kind: u32,
    /// KS_SOUND_* the Bevy client plays for it (0 none), and its volume before distance (`ks_sound_volume_at`).
    pub sound: u32,
    pub volume: f32,
    /// The round's clock when it happened.
    pub time: f32,
    pub id: u32,
    pub by: u32,
    pub to: u32,
    pub weapon: u32,
    pub value: i32,
    pub flags: u32,
    pub amount: f32,
    pub p: [f32; 3],
    pub q: [f32; 3],
}

/// Online: how to join the server (every string may be empty).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsJoin {
    /// The name, colour and Forts team ("mustard"/"ketchup") asked for (the server checks them).
    pub name: KsStr,
    pub color: KsStr,
    pub team: KsStr,
    /// A lobby page's token, which ties this hotdog to that page's name and colour.
    pub token: KsStr,
    /// The dev sandbox asked for, as rules text ("sandbox weapons=all bots=2"), on a --dev server; empty for none.
    pub sandbox: KsStr,
    /// 1: ask for the debug encoding (frames as JSON text). Leave 0.
    pub json: u32,
}

/// Online: a message to send to the server over the game WebSocket.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsMessage {
    /// KS_MESSAGE_NONE (nothing waiting), KS_MESSAGE_TEXT (send as a text frame) or KS_MESSAGE_BINARY.
    pub kind: u32,
    /// The bytes, valid until the next `ks_client_poll` on the handle or its destruction.
    pub data: KsStr,
}

/// Online: the connection as this client has it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KsClientStatus {
    /// 1 once the server's welcome has come (a map and a hotdog to play).
    pub welcomed: u32,
    /// The hotdog this client plays (0 before the welcome).
    pub local_player: u32,
    /// The server's steps and frames a second.
    pub tick_hz: f32,
    pub send_hz: f32,
    /// The newest frame decoded (its step) and the newest the server has applied of this client's commands.
    pub newest_tick: u32,
    pub acked_command: u32,
    /// The last command made, and how many are not yet applied by the server.
    pub command_seq: u32,
    pub pending_commands: u32,
    /// Frames decoded, ones that could not be (each made this client ask for a whole one), and whole ones.
    pub frames: u64,
    pub broken: u64,
    pub whole: u64,
    /// The prediction of this client's hotdog: on, corrections and snaps so far, and the correction still being eased
    /// away (metres).
    pub predicting: u32,
    pub corrections: u32,
    pub snaps: u32,
    pub offset: f32,
    /// The server's last error message ("Kitchen full", "Multiplayer disconnected", ...), or empty. Valid until the next
    /// mutating call.
    pub error: KsStr,
    /// The server's lobby, from the newest frame: its leader (the hotdog that chooses the map, 0 none) and the next
    /// round's map as chosen (its file name, title, mode and theme; `choice_random` 1 for Random). Valid until the next
    /// mutating call.
    pub leader: u32,
    pub choice_random: u32,
    pub choice_name: KsStr,
    pub choice_title: KsStr,
    pub choice_mode: KsStr,
    pub choice_theme: KsStr,
    /// The tuning the server plays by (tune.rs text, empty for the constants), and whether the sandbox may change it.
    pub tuning: KsStr,
    pub tuning_editable: u32,
}

/// A game: offline (`ks_game_create`) or online (`ks_client_create`). Opaque.
pub struct KsGame {
    inner: Game,
}

// ================================================================================================================
// The boundary: handles, errors, panics
// ================================================================================================================

/// Every live handle's address: a pointer is followed only when it is one of these.
static LIVE: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

thread_local! {
    /// The reason for the last failure on this thread, NUL-terminated.
    static LAST_ERROR: RefCell<Vec<u8>> = RefCell::new(vec![0]);
}

/// A failure: what to return and why.
pub(crate) struct Fail {
    pub status: KsStatus,
    pub message: String,
}
pub(crate) type Res<T = ()> = Result<T, Fail>;
pub(crate) fn fail<T>(status: KsStatus, message: impl Into<String>) -> Res<T> {
    Err(Fail { status, message: message.into() })
}
fn set_error(message: &str) {
    LAST_ERROR.with(|e| {
        let mut e = e.borrow_mut();
        e.clear();
        e.extend_from_slice(message.replace('\0', " ").as_bytes());
        e.push(0);
    });
}

/// Runs an entry point's body: its failure, or a panic, becomes a status and the thread's last error.
fn guard(name: &str, body: impl FnOnce() -> Res) -> KsStatus {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => KsStatus::KS_OK,
        Ok(Err(f)) => {
            set_error(&format!("{name}: {}", f.message));
            f.status
        }
        Err(payload) => {
            let what = payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into());
            set_error(&format!("{name}: panic: {what}"));
            KsStatus::KS_ERR_PANIC
        }
    }
}
/// The same for an entry point that returns a value rather than a status: `fallback` on failure.
fn guard_value<T>(name: &str, fallback: T, body: impl FnOnce() -> Res<T>) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(v)) => v,
        Ok(Err(f)) => {
            set_error(&format!("{name}: {}", f.message));
            fallback
        }
        Err(_) => {
            set_error(&format!("{name}: panic"));
            fallback
        }
    }
}

fn live() -> std::sync::MutexGuard<'static, BTreeSet<usize>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}
fn register(game: Game) -> *mut KsGame {
    let p = Box::into_raw(Box::new(KsGame { inner: game }));
    live().insert(p as usize);
    p
}
fn checked(p: *const KsGame) -> Res {
    if p.is_null() {
        return fail(KsStatus::KS_ERR_NULL, "the game handle is null");
    }
    if !live().contains(&(p as usize)) {
        return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, "not a live game handle (destroyed, or never made by ks_game_create/ks_client_create)");
    }
    Ok(())
}
unsafe fn game_ref<'a>(p: *const KsGame) -> Res<&'a Game> {
    checked(p)?;
    // SAFETY: the address is a live Box<KsGame> made by `register` and not yet destroyed.
    Ok(unsafe { &(*p).inner })
}
unsafe fn game_mut<'a>(p: *mut KsGame) -> Res<&'a mut Game> {
    checked(p)?;
    // SAFETY: as above; the caller promises no other call uses the handle at the same time.
    Ok(unsafe { &mut (*p).inner })
}
/// A slice from a pointer and a length: null is only allowed for nothing.
unsafe fn bytes<'a>(ptr: *const u8, len: usize, what: &str) -> Res<&'a [u8]> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return fail(KsStatus::KS_ERR_NULL, format!("{what}: null pointer with length {len}"));
    }
    if len > isize::MAX as usize {
        return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, format!("{what}: length {len} too large"));
    }
    // SAFETY: the caller promises `len` readable bytes at `ptr`.
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
}
unsafe fn text<'a>(s: KsStr, what: &str) -> Res<&'a str> {
    let b = unsafe { bytes(s.ptr, s.len, what)? };
    std::str::from_utf8(b).or_else(|e| fail(KsStatus::KS_ERR_UTF8, format!("{what}: not UTF-8 ({e})")))
}
unsafe fn out<'a, T>(p: *mut T, what: &str) -> Res<&'a mut T> {
    if p.is_null() {
        return fail(KsStatus::KS_ERR_NULL, format!("{what} is null"));
    }
    // SAFETY: the caller promises a writable T at `p`.
    Ok(unsafe { &mut *p })
}
/// Copies `items` into the caller's array: at most `capacity` of them; `*count` gets how many there are in all.
unsafe fn fill<T: Copy>(items: &[T], array: *mut T, capacity: u32, count: *mut u32) -> Res {
    let count = unsafe { out(count, "count")? };
    if capacity > 0 && array.is_null() {
        return fail(KsStatus::KS_ERR_NULL, "the array is null but its capacity is not 0");
    }
    for (i, item) in items.iter().take(capacity as usize).enumerate() {
        // SAFETY: the caller promises `capacity` writable elements at `array`.
        unsafe { array.add(i).write(*item) };
    }
    *count = items.len().min(u32::MAX as usize) as u32;
    Ok(())
}

// ================================================================================================================
// Version and errors
// ================================================================================================================

/// The library's version. Never fails.
#[unsafe(no_mangle)]
pub extern "C" fn ks_version() -> KsVersion {
    KsVersion { abi: KS_ABI_VERSION, protocol: net::PROTOCOL, tick_hz: net::TICK_HZ, send_hz: net::SEND_HZ, version: KsStr::of(env!("CARGO_PKG_VERSION")) }
}
/// The network protocol the library speaks (12).
#[unsafe(no_mangle)]
pub extern "C" fn ks_protocol() -> u32 {
    net::PROTOCOL
}
/// The reason the last call that failed on this thread failed (NUL-terminated; `len` leaves the NUL out). Valid until the
/// next failing call on this thread. Empty before any failure. Successful calls do not clear it.
#[unsafe(no_mangle)]
pub extern "C" fn ks_last_error() -> KsStr {
    LAST_ERROR.with(|e| {
        let e = e.borrow();
        KsStr { ptr: e.as_ptr(), len: e.len().saturating_sub(1) }
    })
}

// ================================================================================================================
// Lifecycle
// ================================================================================================================

/// Makes an offline game: the map (its JSON, as assets/maps/*.json or the editor writes it; empty for the bundled
/// Counter Siege), the rules ("" for the map's own mode; "mode=siege", "mode=forts", "mode=ffa", "mode=kungfu", "mode=cards", "sandbox ...": see
/// rules.rs), the seed every shuffle and spawn draw comes from (the same seed and the same commands give the same game),
/// and KS_GAME_* flags. Player 1 is this client's; the rest are bots. On success `*out` is the handle (destroy it with
/// `ks_game_destroy`); on failure `*out` is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_create(map_json: KsStr, rules: KsStr, seed: u64, flags: u32, out: *mut *mut KsGame) -> KsStatus {
    guard("ks_game_create", || {
        let out = unsafe { self::out(out, "out")? };
        *out = std::ptr::null_mut();
        let map_text = unsafe { text(map_json, "map_json")? };
        let rules = unsafe { text(rules, "rules")? };
        if flags & !(KS_GAME_ALL_BOTS | KS_GAME_FOLLOW) != 0 {
            return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, format!("unknown flags {flags:#x}"));
        }
        let map = if map_text.trim().is_empty() { map::Map::bundled() } else { map::Map::parse(map_text).or_else(|e| fail(KsStatus::KS_ERR_MAP, e.0))? };
        *out = register(Game::offline(map, rules, seed, flags));
        Ok(())
    })
}
/// Makes an online game for the server's game socket (ws://HOST:8137/ws): the join message is the first thing
/// `ks_client_poll` gives to send. Nothing can be read until the server's welcome has been fed in
/// (`ks_client_receive_text`): until then the readers return KS_ERR_NOT_READY. `join` may be null (no name, colour or
/// team asked for).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_client_create(join: *const KsJoin, out: *mut *mut KsGame) -> KsStatus {
    guard("ks_client_create", || {
        let out = unsafe { self::out(out, "out")? };
        *out = std::ptr::null_mut();
        let mut message = serde_json::json!({"type": "join", "version": net::PROTOCOL});
        if !join.is_null() {
            let j = unsafe { &*join };
            for (key, value) in [("name", j.name), ("color", j.color), ("team", j.team), ("token", j.token)] {
                let value = unsafe { text(value, key)? };
                if !value.is_empty() {
                    message[key] = serde_json::Value::String(value.into());
                }
            }
            let sandbox = unsafe { text(j.sandbox, "sandbox")? };
            if !sandbox.trim().is_empty() {
                let mut asked = serde_json::Map::new();
                asked.insert("sandbox".into(), serde_json::Value::String(String::new()));
                for word in sandbox.split([' ', '&', ';']).filter(|w| !w.is_empty()) {
                    let (key, value) = word.split_once('=').unwrap_or((word, ""));
                    asked.insert(key.into(), serde_json::Value::String(value.into()));
                }
                message["sandbox"] = serde_json::Value::Object(asked);
            }
            if j.json != 0 {
                message["json"] = serde_json::Value::Bool(true);
            }
        }
        *out = register(Game::online(message.to_string()));
        Ok(())
    })
}
/// Destroys a game. A null or already destroyed handle is passed over. Every pointer the game handed out dies with it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_destroy(game: *mut KsGame) {
    let _ = guard("ks_game_destroy", || {
        if game.is_null() || !live().remove(&(game as usize)) {
            return Ok(());
        }
        // SAFETY: it was live, made by `register`, and is now unregistered: nobody else frees it.
        drop(unsafe { Box::from_raw(game) });
        Ok(())
    });
}
/// 1 for an online game, 0 offline (and for anything that is not a live handle).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_is_online(game: *const KsGame) -> u32 {
    guard_value("ks_game_is_online", 0, || Ok(unsafe { game_ref(game)? }.online.is_some() as u32))
}

// ================================================================================================================
// Playing: commands and steps
// ================================================================================================================

/// The local player's input from now on (both modes). Held buttons, the stick, the look and the weapon stay as set until
/// the next call; edge buttons (KS_BUTTON_EDGES; in Kung Fu all but GRAB) and the pickup are latched and go once, in
/// the next step's command. Non-finite numbers, a weapon over 7 or an unknown pickup index are refused.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_set_command(game: *mut KsGame, command: *const KsCommand) -> KsStatus {
    guard("ks_game_set_command", || {
        let g = unsafe { game_mut(game)? };
        let c = unsafe { command.as_ref() }.ok_or_else(|| Fail { status: KsStatus::KS_ERR_NULL, message: "command is null".into() })?;
        g.set_command(c)
    })
}
/// Advances the game by `dt` seconds (0..10; use the frame's delta, or 1/60 for the server's step).
/// Offline: the local player's command is applied (as the server applies a command: walk, jump, look, gun, then what was
/// pressed; in Kung Fu `Feast::kungfu_command`), then the round is stepped (bots, physics, rules) in sub-steps of at most
/// 1/30 s. Online: one command is made for every server step (1/tick_hz) that `dt` completes, predicted at once on this
/// client's own hotdog and queued to send (`ks_client_poll`) with the newest frame's acknowledgement.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_step(game: *mut KsGame, dt: f32) -> KsStatus {
    guard("ks_game_step", || {
        let g = unsafe { game_mut(game)? };
        if !dt.is_finite() || !(0. ..=10.).contains(&dt) {
            return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, format!("dt {dt} is not a number of seconds in 0..10"));
        }
        g.step(dt)
    })
}
/// Offline: starts the next round on the same map now (what Enter does after a round in single player).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_restart(game: *mut KsGame) -> KsStatus {
    guard("ks_game_restart", || unsafe { game_mut(game)? }.restart())
}
/// Offline: whether a bot plays hotdog `id` (kept across rounds). The local player's hotdog (1) made a bot plays itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_set_bot(game: *mut KsGame, id: u32, bot: u32) -> KsStatus {
    guard("ks_game_set_bot", || unsafe { game_mut(game)? }.set_bot(id, bot != 0))
}
/// The local player presses use at an electric griddle: offline the one in reach flips (KS_ERR_REFUSED if none);
/// online `{type:'griddle'}` is queued (the server decides).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_use_griddle(game: *mut KsGame) -> KsStatus {
    guard("ks_game_use_griddle", || unsafe { game_mut(game)? }.use_griddle())
}
/// The local player is ready (or not): a Forts build phase offline; online also the server's lobby (`{type:'ready'}`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_ready(game: *mut KsGame, ready: u32) -> KsStatus {
    guard("ks_game_ready", || unsafe { game_mut(game)? }.ready(ready != 0))
}
/// Fully Loaded: the local player's flip, stop or (KS_CARDS_TARGET) the seat an action card is played on. Offline
/// KS_ERR_REFUSED when it is not its go; online `{type:'cards',...}` is queued.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_cards_command(game: *mut KsGame, action: u32, seat: u32) -> KsStatus {
    guard("ks_cards_command", || unsafe { game_mut(game)? }.cards_command(action, seat))
}
/// The tuning (tune.rs text, "" for the constants): offline it applies at once; online it is sent as `{type:'tune'}`
/// (a --dev server's sandbox only).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_set_tuning(game: *mut KsGame, tuning: KsStr) -> KsStatus {
    guard("ks_game_set_tuning", || {
        let g = unsafe { game_mut(game)? };
        let t = unsafe { text(tuning, "tuning")? };
        g.set_tuning(t)
    })
}

// ================================================================================================================
// Online
// ================================================================================================================

/// Online: a binary message from the server (a frame). It is decoded against the frame it names, checked, and taken in:
/// players, cats, rounds, props, the round's state, this client's prediction checked. KS_ERR_MALFORMED for one that
/// cannot be decoded or does not check out (the next batch asks for a whole frame); an old frame is passed over (KS_OK).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_client_receive_binary(game: *mut KsGame, data: *const u8, len: usize) -> KsStatus {
    guard("ks_client_receive_binary", || {
        let g = unsafe { game_mut(game)? };
        let b = unsafe { bytes(data, len, "data")? };
        g.receive_binary(b)
    })
}
/// Online: a text message from the server: welcome, map, tuning, kungfu, cards, griddle, error or (the debug encoding)
/// snapshot. Unknown types are passed over (KS_OK). KS_ERR_MALFORMED for text that is not a JSON object, or a welcome
/// whose map cannot be read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_client_receive_text(game: *mut KsGame, data: *const u8, len: usize) -> KsStatus {
    guard("ks_client_receive_text", || {
        let g = unsafe { game_mut(game)? };
        let b = unsafe { bytes(data, len, "data")? };
        let t = std::str::from_utf8(b).or_else(|e| fail(KsStatus::KS_ERR_UTF8, format!("not UTF-8 ({e})")))?;
        g.receive_text(t)
    })
}
/// Online: the next message to send, oldest first: `out->kind` is KS_MESSAGE_NONE when there is none. The bytes stay
/// valid until the next `ks_client_poll` on this handle or its destruction. Call it until NONE after every step.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_client_poll(game: *mut KsGame, message: *mut KsMessage) -> KsStatus {
    guard("ks_client_poll", || {
        let g = unsafe { game_mut(game)? };
        let m = unsafe { out(message, "message")? };
        *m = g.poll()?;
        Ok(())
    })
}
/// Online: queues a text message of the caller's own (for what this interface does not cover). It must be a JSON object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_client_send_text(game: *mut KsGame, message: KsStr) -> KsStatus {
    guard("ks_client_send_text", || {
        let g = unsafe { game_mut(game)? };
        let t = unsafe { text(message, "message")? };
        g.send_text(t)
    })
}
/// Online: the connection's state. Offline: KS_ERR_WRONG_MODE.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_client_status(game: *const KsGame, status: *mut KsClientStatus) -> KsStatus {
    guard("ks_client_status", || {
        let g = unsafe { game_ref(game)? };
        let s = unsafe { out(status, "status")? };
        *s = g.client_status()?;
        Ok(())
    })
}
/// Online: whether this client predicts its own hotdog (on by default; never in Kung Fu, whose foresight is still the
/// Bevy client's). Off, its hotdog is where the newest frame has it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_client_set_prediction(game: *mut KsGame, on: u32) -> KsStatus {
    guard("ks_client_set_prediction", || unsafe { game_mut(game)? }.set_prediction(on != 0))
}

// ================================================================================================================
// Reading the game. Every array reader copies into the caller's array: at most `capacity` entries are written and
// `*count` gets how many there are (call with capacity 0 to ask). Strings inside them point into the game and are valid
// until the next mutating call on it (set_command, step, restart, receive, poll, destroy...).
// ================================================================================================================

/// The round's state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_match(game: *const KsGame, out: *mut KsMatch) -> KsStatus {
    guard("ks_game_match", || {
        let g = unsafe { game_ref(game)? };
        let o = unsafe { self::out(out, "out")? };
        *o = g.match_state()?;
        Ok(())
    })
}
/// The hotdogs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_players(game: *const KsGame, array: *mut KsPlayer, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_players", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.players()?, array, capacity, count) }
    })
}
/// One hotdog by id (KS_ERR_NOT_FOUND if there is none).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_player(game: *const KsGame, id: u32, out: *mut KsPlayer) -> KsStatus {
    guard("ks_game_player", || {
        let g = unsafe { game_ref(game)? };
        let o = unsafe { self::out(out, "out")? };
        *o = g.players()?.into_iter().find(|p| p.id == id).ok_or_else(|| Fail { status: KsStatus::KS_ERR_NOT_FOUND, message: format!("no hotdog {id}") })?;
        Ok(())
    })
}
/// The local player's hotdog id (1 offline; the welcome's online, 0 before it).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_local_player(game: *const KsGame) -> u32 {
    guard_value("ks_game_local_player", 0, || Ok(unsafe { game_ref(game)? }.me))
}
/// The zombie cats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_enemies(game: *const KsGame, array: *mut KsEnemy, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_enemies", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.enemies()?, array, capacity, count) }
    })
}
/// The rounds in flight.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_projectiles(game: *const KsGame, array: *mut KsProjectile, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_projectiles", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.projectiles()?, array, capacity, count) }
    })
}
/// The loose objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_props(game: *const KsGame, array: *mut KsProp, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_props", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.props()?, array, capacity, count) }
    })
}
/// The pickups.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_pickups(game: *const KsGame, array: *mut KsPickup, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_pickups", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.pickups()?, array, capacity, count) }
    })
}
/// The turrets.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_turrets(game: *const KsGame, array: *mut KsTurret, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_turrets", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.turrets()?, array, capacity, count) }
    })
}
/// The swarms of pests.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_swarms(game: *const KsGame, array: *mut KsSwarm, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_swarms", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.swarms()?, array, capacity, count) }
    })
}
/// The puddles of the zombie emergences under way.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_puddles(game: *const KsGame, array: *mut KsPuddle, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_puddles", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.puddles()?, array, capacity, count) }
    })
}
/// The draped napkins.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_drapes(game: *const KsGame, array: *mut KsDrape, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_drapes", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.drapes()?, array, capacity, count) }
    })
}
/// The electric griddles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_griddles(game: *const KsGame, array: *mut KsGriddle, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_griddles", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.griddles()?, array, capacity, count) }
    })
}
/// Forts' state: KS_ERR_NOT_FOUND when the game is not Forts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_forts(game: *const KsGame, out: *mut KsForts) -> KsStatus {
    guard("ks_game_forts", || {
        let g = unsafe { game_ref(game)? };
        let o = unsafe { self::out(out, "out")? };
        *o = g.forts()?;
        Ok(())
    })
}
/// Fully Loaded: the card table as JSON (cards.rs `View`, as the server's `{type:'cards'}` carries it), valid until the
/// next mutating call. KS_ERR_NOT_FOUND without a table.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_cards_json(game: *mut KsGame, out: *mut KsStr) -> KsStatus {
    guard("ks_cards_json", || {
        let g = unsafe { game_mut(game)? };
        let o = unsafe { self::out(out, "out")? };
        *o = g.cards_json()?;
        Ok(())
    })
}
/// The height of the ground at [x, z] in world space (the highest deck or ramp there, else the counter), and whether
/// [x, z] is on the map at all (0 off it).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_ground(game: *const KsGame, x: f32, z: f32, y: *mut f32) -> KsStatus {
    guard("ks_game_ground", || {
        let g = unsafe { game_ref(game)? };
        let y = unsafe { out(y, "y")? };
        if !x.is_finite() || !z.is_finite() {
            return fail(KsStatus::KS_ERR_INVALID_ARGUMENT, "x and z must be finite");
        }
        *y = g.feast()?.level.ground(x as f64, z as f64) as f32;
        Ok(())
    })
}

// ----- Events

/// Takes the events that happened since they were last taken, oldest first: at most `capacity` are written (and removed
/// from the queue), `*count` gets how many were written. Call again while it fills the array. The queue keeps the
/// newest 4096; older ones are dropped if never taken.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_events(game: *mut KsGame, array: *mut KsEvent, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_game_events", || {
        let g = unsafe { game_mut(game)? };
        let n = unsafe { out(count, "count")? };
        if capacity > 0 && array.is_null() {
            return fail(KsStatus::KS_ERR_NULL, "the array is null but its capacity is not 0");
        }
        let take = (capacity as usize).min(g.events.len());
        for (i, e) in g.events.drain(..take).enumerate() {
            // SAFETY: the caller promises `capacity` writable events at `array`.
            unsafe { array.add(i).write(e) };
        }
        *n = take as u32;
        Ok(())
    })
}
/// How many events are waiting.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_game_event_count(game: *const KsGame) -> u32 {
    guard_value("ks_game_event_count", 0, || Ok(unsafe { game_ref(game)? }.events.len() as u32))
}
/// The file under assets/ the Bevy client plays for a KS_SOUND_* ("audio/fu-hit.wav"), static; empty for none.
#[unsafe(no_mangle)]
pub extern "C" fn ks_sound_file(sound: u32) -> KsStr {
    KsStr::of(events::sound_file(sound))
}
/// How loud a sound `distance` metres from the listener is played, of its own volume (fu.rs `near`): whole within half
/// a metre, down to a fifth at about five metres.
#[unsafe(no_mangle)]
pub extern "C" fn ks_sound_volume_at(distance: f32) -> f32 {
    if distance.is_finite() { (1. - (distance - 0.5).max(0.) / 4.4).clamp(0.2, 1.) } else { 0.2 }
}

// ----- Kung Fu

/// Kung Fu, the fight: KS_ERR_NOT_FOUND when the game is not Kung Fu (online: until the server's first
/// `{type:'kungfu'}`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_kungfu_state(game: *const KsGame, out: *mut KsKungfu) -> KsStatus {
    guard("ks_kungfu_state", || {
        let g = unsafe { game_ref(game)? };
        let o = unsafe { self::out(out, "out")? };
        *o = g.kungfu_state()?;
        Ok(())
    })
}
/// Kung Fu, the fighters (by player id order). KS_ERR_NOT_FOUND when the game is not Kung Fu.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_kungfu_fighters(game: *const KsGame, array: *mut KsFighter, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_kungfu_fighters", || {
        let g = unsafe { game_ref(game)? };
        unsafe { fill(&g.fighters()?, array, capacity, count) }
    })
}
/// Kung Fu's numbers. Never fails.
#[unsafe(no_mangle)]
pub extern "C" fn ks_kungfu_rules() -> KsKungfuRules {
    KsKungfuRules {
        styles: kungfu::STYLES as u32,
        moves: kungfu::MOVES as u32,
        kos: kungfu::KOS,
        time: kungfu::TIME,
        respawn: kungfu::RESPAWN,
        chi_max: kungfu::CHI_MAX,
        guard_max: kungfu::GUARD_MAX,
        event_life: kungfu::EVENT_LIFE,
        parry: kungfu::PARRY,
        buffer: kungfu::BUFFER,
        throw_at: kungfu::THROW_AT,
        stance_pace: kungfu::STANCE_PACE,
        block_pace: kungfu::BLOCK_PACE,
        style_pace: kungfu::STYLE_PACE,
    }
}
/// The number of moves in the table (85).
#[unsafe(no_mangle)]
pub extern "C" fn ks_kungfu_move_count() -> u32 {
    kungfu::MOVES as u32
}
/// One row of the move table. KS_ERR_NOT_FOUND for an id past the last.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_kungfu_move(id: u32, out: *mut KsMove) -> KsStatus {
    guard("ks_kungfu_move", || {
        let o = unsafe { self::out(out, "out")? };
        let mv = u8::try_from(id).ok().and_then(Move::from_u8).ok_or_else(|| Fail { status: KsStatus::KS_ERR_NOT_FOUND, message: format!("no move {id}") })?;
        *o = events::move_row(mv);
        Ok(())
    })
}
/// The number of styles (7).
#[unsafe(no_mangle)]
pub extern "C" fn ks_kungfu_style_count() -> u32 {
    kungfu::STYLES as u32
}
/// A style's name ("Tiger"), static; empty past the last.
#[unsafe(no_mangle)]
pub extern "C" fn ks_kungfu_style_name(style: u32) -> KsStr {
    kungfu::Style::NAMES.get(style as usize).map_or(KsStr::EMPTY, |s| KsStr::of(s))
}

// ----- Static names

/// A loose object's kind name (map::LOOSE: "bowl", "crate", "pillow"...), static; empty past the last.
#[unsafe(no_mangle)]
pub extern "C" fn ks_prop_kind_name(kind: u32) -> KsStr {
    map::LOOSE.get(kind as usize).map_or(KsStr::EMPTY, |s| KsStr::of(s))
}
/// The number of loose object kinds.
#[unsafe(no_mangle)]
pub extern "C" fn ks_prop_kind_count() -> u32 {
    map::LOOSE.len() as u32
}
/// A weapon's name (rules.rs WEAPONS: "melee", "mustard", "sniper"...), static; empty past the last.
#[unsafe(no_mangle)]
pub extern "C" fn ks_weapon_name(weapon: u32) -> KsStr {
    kitchen_sim::rules::WEAPONS.get(weapon as usize).map_or(KsStr::EMPTY, |s| KsStr::of(s))
}
/// A stage's name (net::STAGES: "Lobby", "Opening", "Warmup"...), static; empty past the last.
#[unsafe(no_mangle)]
pub extern "C" fn ks_stage_name(stage: u32) -> KsStr {
    net::STAGES.get(stage as usize).map_or(KsStr::EMPTY, |s| KsStr::of(s))
}

// ================================================================================================================
// The static map
// ================================================================================================================

/// The map's description and how many of each thing it has.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_info(game: *const KsGame, out: *mut KsMapInfo) -> KsStatus {
    guard("ks_map_info", || {
        let g = unsafe { game_ref(game)? };
        let o = unsafe { self::out(out, "out")? };
        let f = g.feast()?;
        let m = &f.level.map;
        let fl = |v: f64| v as f32;
        *o = KsMapInfo {
            name: KsStr::of(&m.name),
            theme: KsStr::of(&m.theme),
            mode: KsStr::of(&m.mode),
            floor: feast::FLOOR,
            bounds: m.bounds.map(fl),
            cell: fl(m.cell),
            clearance: fl(m.clearance),
            respawn: m.respawn.map(fl),
            build: fl(m.build),
            cabinets: m.cabinets as u32,
            blocks: m.blocks.len() as u32,
            decks: m.decks.len() as u32,
            ramps: m.ramps.len() as u32,
            sinks: m.sinks.len() as u32,
            props: m.props.len() as u32,
            loose: m.loose.len() as u32,
            starts: m.starts.len() as u32,
            spawns: m.spawns.len() as u32,
            pickups: m.pickups.len() as u32,
            zones: m.zones.len() as u32,
            griddles: f.griddles.len() as u32,
        };
        Ok(())
    })
}
/// The map's canonical JSON text (map.rs `to_text`), valid until the next mutating call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_json(game: *mut KsGame, out: *mut KsStr) -> KsStatus {
    guard("ks_map_json", || {
        let g = unsafe { game_mut(game)? };
        let o = unsafe { self::out(out, "out")? };
        let text = g.feast()?.level.map.to_text();
        g.cache = text;
        *o = KsStr::of(&g.cache);
        Ok(())
    })
}
fn side(s: Side) -> i32 {
    match s {
        Side::PlusX => KS_SIDE_PLUS_X,
        Side::MinusX => KS_SIDE_MINUS_X,
        Side::PlusZ => KS_SIDE_PLUS_Z,
        Side::MinusZ => KS_SIDE_MINUS_Z,
    }
}
fn rect(r: [f64; 4]) -> [f32; 4] {
    r.map(|v| v as f32)
}
/// The blocks (counter-top furniture and appliances: solid boxes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_blocks(game: *const KsGame, array: *mut KsBlock, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_blocks", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsBlock> = g.feast()?.level.map.blocks.iter().map(|b| KsBlock { rect: rect(b.rect), height: b.height as f32, style: KsStr::of(&b.style), facing: b.facing.map_or(-1, side) }).collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}
/// The decks (raised floors).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_decks(game: *const KsGame, array: *mut KsDeck, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_decks", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsDeck> = g.feast()?.level.map.decks.iter().map(|d| KsDeck { rect: rect(d.rect), height: d.height as f32, style: KsStr::of(&d.style), round: (d.style == map::ROUND) as u32 }).collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}
/// The ramps.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_ramps(game: *const KsGame, array: *mut KsRamp, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_ramps", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsRamp> =
            g.feast()?.level.map.ramps.iter().map(|r| KsRamp { rect: rect(r.rect), base: r.base as f32, height: r.height as f32, rise: side(r.rise), style: r.style.as_deref().map_or(KsStr::EMPTY, KsStr::of) }).collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}
/// The sinks (basins sunk into the counter).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_sinks(game: *const KsGame, array: *mut KsSink, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_sinks", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsSink> = g.feast()?.level.map.sinks.iter().map(|s| KsSink { rect: rect(s.rect), depth: s.depth as f32 }).collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}
/// The scenery props: boxes, turret posts, bins.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_props(game: *const KsGame, array: *mut KsMapProp, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_props", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsMapProp> = g
            .feast()?
            .level
            .map
            .props
            .iter()
            .map(|p| match p {
                Prop::Box { rect: r, base, height, style } => KsMapProp { kind: KS_MAP_PROP_BOX, rect: rect(*r), p: [((r[0] + r[2]) / 2.) as f32, ((r[1] + r[3]) / 2.) as f32], base: *base as f32, height: *height as f32, yaw: 0., style: KsStr::of(style) },
                Prop::Turret { p, base, yaw } => KsMapProp { kind: KS_MAP_PROP_TURRET, rect: [0.; 4], p: p.map(|v| v as f32), base: *base as f32, height: 0., yaw: *yaw as f32, style: KsStr::EMPTY },
                Prop::Bin { p, yaw } => KsMapProp { kind: KS_MAP_PROP_BIN, rect: [0.; 4], p: p.map(|v| v as f32), base: 0., height: 0., yaw: *yaw as f32, style: KsStr::EMPTY },
            })
            .collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}
/// The hotdogs' starts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_starts(game: *const KsGame, array: *mut KsStart, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_starts", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsStart> = g.feast()?.level.map.starts.iter().map(|s| KsStart { p: s.p.map(|v| v as f32), yaw: s.yaw as f32, team: s.team.map_or(-1, |t| t as i32) }).collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}
/// The zombies' spawn points.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_spawns(game: *const KsGame, array: *mut KsSpawn, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_spawns", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsSpawn> = g.feast()?.level.map.spawns.iter().map(|s| KsSpawn { p: s.p.map(|v| v as f32) }).collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}
/// The pickups as the map places them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_pickups(game: *const KsGame, array: *mut KsMapPickup, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_pickups", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsMapPickup> = g.feast()?.level.map.pickups.iter().map(|p| KsMapPickup { p: p.p.map(|v| v as f32), weapon: p.weapon as u32, ammo: p.ammo }).collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}
/// The loose objects as the map places them at the start of a round.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_loose(game: *const KsGame, array: *mut KsMapLoose, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_loose", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsMapLoose> = g
            .feast()?
            .level
            .map
            .loose
            .iter()
            .map(|l| KsMapLoose { kind: map::LOOSE.iter().position(|k| *k == l.kind).unwrap_or(0) as u32, p: l.p.map(|v| v as f32), base: l.base as f32, yaw: l.yaw as f32 })
            .collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}
/// A Forts map's zones.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ks_map_zones(game: *const KsGame, array: *mut KsZone, capacity: u32, count: *mut u32) -> KsStatus {
    guard("ks_map_zones", || {
        let g = unsafe { game_ref(game)? };
        let items: Vec<KsZone> = g
            .feast()?
            .level
            .map
            .zones
            .iter()
            .map(|z| match z {
                Zone::Base { team, rect: r } => KsZone { kind: KS_ZONE_BASE, team: *team as i32, rect: rect(*r), p: [((r[0] + r[2]) / 2.) as f32, ((r[1] + r[3]) / 2.) as f32] },
                Zone::Field { rect: r } => KsZone { kind: KS_ZONE_FIELD, team: -1, rect: rect(*r), p: [((r[0] + r[2]) / 2.) as f32, ((r[1] + r[3]) / 2.) as f32] },
                Zone::Bacon { p } => KsZone { kind: KS_ZONE_BACON, team: -1, rect: [0.; 4], p: p.map(|v| v as f32) },
            })
            .collect();
        unsafe { fill(&items, array, capacity, count) }
    })
}

/// The stage as net::STAGES numbers it.
pub(crate) fn stage_index(stage: Stage) -> u32 {
    match stage {
        Stage::Warmup => KS_STAGE_WARMUP,
        Stage::Wave => KS_STAGE_WAVE,
        Stage::FreeForAll => KS_STAGE_FREE_FOR_ALL,
        Stage::RoundOver => KS_STAGE_ROUND_OVER,
        Stage::Build => KS_STAGE_BUILD,
        Stage::Fight => KS_STAGE_FIGHT,
    }
}

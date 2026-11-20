// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! **Kung Fu** (`mode=kungfu`, rules.rs `Rules::kungfu`): a free for all with hotdog hands and feet only. No guns, no
//! pickups, no carrying. Seven styles of ten moves each, and fifteen moves every style shares: eighty-five moves, each one
//! an animation the client plays (src/kungfu/). (The seventh style, Toad, came last: its moves are 75 to 84, after the
//! others', so that no number changed.) This file is the move table, which the client reads its timings from,
//! the fighters' combat state machine, the bots, and what the client is told (`View`, `{type:'kungfu'}` beside the
//! frames, which are unchanged: protocol 12).
//!
//! The interface the client codes against is written up in `.superpowers/sdd/adhoc/kungfu/interface.md`.
//!
//! **Inputs** (net.rs `Command`, no new bits): FIRE light, SCOPE heavy, MELEE kick, GRAB block (sent as a level, every
//! command while the key is held; its rising edge is the parry's timing), THROW or EAT grab, STIM dodge (along the stick),
//! JUMP jump (an attack in the air is the style's jump attack), CROUCH crouch (an attack crouched is the sweep, a block
//! crouched blocks low), PRONE the signature special (or light and heavy pressed together), `weapon` 1 to 7 the style.
//! FIRE, SCOPE, PRONE and GRAB are read as levels and their rising edges found here (`Fighter::buttons` keeps the last).
//!
//! **Rules.** A strike lands on a fighter within its reach (centre to centre, flat), inside its arc (either side of the
//! way the striker faces) and at a height its stance does not avoid: a high strike passes over a crouching or low
//! fighter, a low one under a fighter in the air. A block from the front stops high and mid strikes standing and low ones
//! crouched; a block begun within PARRY of the impact is a parry, which staggers the striker. A grab beats any block; a
//! dodge's invulnerable frames beat strikes and grabs; a guard-breaking strike (every second heavy, and the tiger's heavy
//! moves) breaks a block, and so does a guard worn down by blocking. Landing hits and parries fills the chi meter; a full
//! meter pays for the style's signature special. A big hit knocks down; a fighter getting up cannot be hit for a moment.
//! Each style's own touches: the Tiger's heavies and special are armoured in their wind-up (`MoveData::armor`); the
//! Crane reaches furthest; the Snake is quickest; a Mantis parry staggers longest (MANTIS_PARRY) and its grab throws
//! hardest; the Monkey's dodges last longest (DODGE_MONKEY) and it strikes low and rolls under high strikes; the Drunken
//! style's timing varies from strike to strike (DRUNK) and it sways out of the way now and then (DRUNK_SWAY); the Toad is
//! the slowest on its feet (STYLE_PACE), hard to knock about (STYLE_KNOCK) and strongest, with its iron body.
//! The Toad's iron body: while it stands its ground (free, nearly still, on its feet, with chi) an ordinary strike from
//! in front takes only IRON_SHARE of its damage, staggers nothing and recoils on the striker, at the price of chi
//! (IRON_CHI a point of damage). Grabs, guard-breaking strikes, specials and strikes from behind (REAR_ARC) get through,
//! and it is off in the Toad's own moves.
//! A KO is 100 HP taken, credited to whoever took the most of them in the fighter's life (the last hitter if level with
//! the most), or a fighter knocked off the ring (the raised deck every start stands on, if the map has one) or off the
//! counter's edge, which is the last hitter's KO (within RING_CREDIT seconds). First to KOS KOs wins, or the most in TIME seconds.
use serde::{Deserialize, Serialize};

/// How many styles, moves, moves every style shares, and moves of each style.
pub const STYLES: usize = 7;
pub const MOVES: usize = 85;
/// The moves of the first six styles (0 to 74); the Toad's follow them (75 to 84).
pub const FIRST_MOVES: usize = 75;
pub const SHARED: usize = 15;
pub const PER_STYLE: usize = 10;

/// Kung Fu's own numbers. KOS to win, TIME seconds a round lasts, RESPAWN seconds until a KO'd fighter is back.
pub const KOS: u32 = 10;
pub const TIME: f32 = 240.;
pub const RESPAWN: f32 = 3.;
/// A fighter's HP (feast::MAX_HP), chi and guard at most.
pub const CHI_MAX: f32 = 100.;
pub const GUARD_MAX: f32 = 100.;
/// A block pressed this many seconds or less before the impact is a parry; another press counts as a parry again only
/// PARRY_REARM seconds after the last (no parrying everything by tapping).
pub const PARRY: f32 = 0.12;
pub const PARRY_REARM: f32 = 0.4;
/// Seconds a parried striker is staggered (HitHeavy), and the chi the parrier gains; MANTIS_PARRY more of each for a
/// Mantis parrier.
pub const PARRY_STUN: f32 = 0.5;
pub const PARRY_CHI: f32 = 20.;
pub const MANTIS_PARRY: [f32; 2] = [0.3, 15.];
/// A broken guard: seconds the blocker is staggered, the guard it is left with, the share of the damage that gets
/// through.
pub const BREAK_STUN: f32 = 0.6;
pub const BREAK_GUARD: f32 = 50.;
pub const BREAK_SHARE: f32 = 0.25;
/// A blocked strike costs the guard this many times its damage; the guard comes back GUARD_REGEN a second once the
/// fighter has not blocked for GUARD_WAIT seconds. A blocker is held in its block for BLOCK_STUN of the strike's stun,
/// and pushed back BLOCK_PUSH of its pushback.
pub const GUARD_COST: f32 = 2.5;
pub const GUARD_REGEN: f32 = 25.;
pub const GUARD_WAIT: f32 = 0.6;
pub const BLOCK_STUN: f32 = 0.6;
pub const BLOCK_PUSH: f32 = 0.6;
/// A blocker covers strikes from no further than this either side of the way it faces (radians).
pub const BLOCK_ARC: f32 = 1.75;
/// Chi: gained by a striker whose strike is blocked, by a fighter that is hit; what the special costs is its move's
/// `chi`.
pub const CHI_BLOCKED: f32 = 2.;
pub const CHI_HURT: f32 = 2.;
/// A combo: each hit after the first in a row on a fighter still reeling does this much less (down to COMBO_FLOOR of
/// the move's damage); COMBO_KNOCK HP taken within one combo knocks the fighter down.
pub const COMBO_DECAY: f32 = 0.1;
pub const COMBO_FLOOR: f32 = 0.5;
pub const COMBO_KNOCK: f32 = 35.;
/// A strike that lands on a fighter in the startup of its own strike: this much more damage and stun.
pub const COUNTER: f32 = 1.25;
/// Knocked down: seconds on the ground, and the knock upwards (m/s) a knockdown gives.
pub const DOWN: f32 = 0.9;
pub const LIFT: f32 = 0.3;
/// A grab that connects: seconds from the catch to the throw, and how long the thrown fighter flies before it is down.
pub const THROW_AT: f32 = 0.45;
/// A fighter's walking pace in its stance (of the game's walk), blocking, and in a move's recovery. Kung Fu is fought
/// at the walk it was balanced at (actor.rs WALK_BEFORE, 0.57 m/s), not the game's faster one: RING is that walk's share
/// of the game's, so a fighter in its stance walks 0.85 of 0.57 m/s and blocking 0.3 of it, as it always did.
pub const RING: f32 = (crate::actor::WALK_BEFORE / crate::actor::WALK) as f32;
pub const STANCE_PACE: f32 = 0.85 * RING;
pub const BLOCK_PACE: f32 = 0.3 * RING;
/// Seconds an attack pressed during a move is kept for when the move lets it go (a chain, or the next move).
pub const BUFFER: f32 = 0.2;
/// Seconds after a dodge before the next.
pub const DODGE_WAIT: f32 = 0.3;
/// A fighter that falls off the ring or the counter within this many seconds of being hit is the hitter's KO.
pub const RING_CREDIT: f32 = 5.;
/// The kind of damage a kung fu blow puts on a hotdog's record (feast::Damage::kind; after feast::BURN). A ring-out is
/// feast::FALL, with the last hitter as its attacker.
pub const BLOW: u8 = 16;

/// The six styles, numbered as the client numbers them (a command's `weapon` is the style + 1).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Style {
    /// Power: slow, heavy claws and palm strikes that break blocks, high damage.
    #[default]
    Tiger = 0,
    /// Range: long kicks, wing blocks, a one-leg stance, the best reach.
    Crane = 1,
    /// Speed: fast finger jabs from a low stance, short reach, quick recovery, poking combos.
    Snake = 2,
    /// Counters: hooks and trapping hands, the strongest grab and the best reward for a parry.
    Mantis = 3,
    /// Acrobatics: rolls, flips and tumbling attacks, the best dodge, strikes from odd heights.
    Monkey = 4,
    /// Unpredictable: a swaying stance (DRUNK_SWAY), stumbling lunges, falls that are attacks, timing that varies from
    /// strike to strike (deterministic in the simulation, and the client is told it: `FighterView::scale`).
    Drunken = 5,
    /// Iron body: a squat, wide, low stance, belly bumps, double-palm shoves, stomps and leaping body slams; immensely
    /// strong and the slowest on its feet. Standing its ground with chi it shrugs off ordinary strikes (IRON_SHARE); a
    /// grab, a guard-breaking strike, a special or a strike from behind gets through.
    Toad = 6,
}
impl Style {
    pub const ALL: [Style; STYLES] = [Style::Tiger, Style::Crane, Style::Snake, Style::Mantis, Style::Monkey, Style::Drunken, Style::Toad];
    pub const NAMES: [&'static str; STYLES] = ["Tiger", "Crane", "Snake", "Mantis", "Monkey", "Drunken", "Toad"];
    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> Option<Style> {
        Self::ALL.get(i).copied()
    }
    /// The style by name (any case), or None.
    pub fn named(name: &str) -> Option<Style> {
        Self::NAMES.iter().position(|n| n.eq_ignore_ascii_case(name.trim())).map(|i| Self::ALL[i])
    }
    /// The style's move in a slot.
    pub fn move_in(self, slot: Slot) -> Move {
        Move::ALL[SHARED + self as usize * PER_STYLE + slot as usize]
    }
}

/// The ten places of a style's moves, in the order of their numbers.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Slot {
    Light1 = 0,
    Light2 = 1,
    Light3 = 2,
    Heavy1 = 3,
    Heavy2 = 4,
    Kick1 = 5,
    Kick2 = 6,
    Jump = 7,
    Sweep = 8,
    Special = 9,
}
impl Slot {
    pub const ALL: [Slot; PER_STYLE] = [Slot::Light1, Slot::Light2, Slot::Light3, Slot::Heavy1, Slot::Heavy2, Slot::Kick1, Slot::Kick2, Slot::Jump, Slot::Sweep, Slot::Special];
}

/// Every move, by its stable number: the fifteen every style shares (0 to 14), then each style's ten in Style's order
/// and Slot's (15 + style * 10 + slot: the Toad's, style 6, are 75 to 84). These numbers are what travels; never
/// renumber them.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Move {
    // Shared.
    #[default]
    Idle = 0,
    Walk = 1,
    BlockHigh = 2,
    BlockLow = 3,
    Parry = 4,
    DodgeLeft = 5,
    DodgeRight = 6,
    BackHop = 7,
    HitLight = 8,
    HitHeavy = 9,
    KnockDown = 10,
    GetUp = 11,
    Grab = 12,
    Thrown = 13,
    Victory = 14,
    // Tiger.
    TigerLight1 = 15,
    TigerLight2 = 16,
    TigerLight3 = 17,
    TigerHeavy1 = 18,
    TigerHeavy2 = 19,
    TigerKick1 = 20,
    TigerKick2 = 21,
    TigerJump = 22,
    TigerSweep = 23,
    TigerSpecial = 24,
    // Crane.
    CraneLight1 = 25,
    CraneLight2 = 26,
    CraneLight3 = 27,
    CraneHeavy1 = 28,
    CraneHeavy2 = 29,
    CraneKick1 = 30,
    CraneKick2 = 31,
    CraneJump = 32,
    CraneSweep = 33,
    CraneSpecial = 34,
    // Snake.
    SnakeLight1 = 35,
    SnakeLight2 = 36,
    SnakeLight3 = 37,
    SnakeHeavy1 = 38,
    SnakeHeavy2 = 39,
    SnakeKick1 = 40,
    SnakeKick2 = 41,
    SnakeJump = 42,
    SnakeSweep = 43,
    SnakeSpecial = 44,
    // Mantis.
    MantisLight1 = 45,
    MantisLight2 = 46,
    MantisLight3 = 47,
    MantisHeavy1 = 48,
    MantisHeavy2 = 49,
    MantisKick1 = 50,
    MantisKick2 = 51,
    MantisJump = 52,
    MantisSweep = 53,
    MantisSpecial = 54,
    // Monkey.
    MonkeyLight1 = 55,
    MonkeyLight2 = 56,
    MonkeyLight3 = 57,
    MonkeyHeavy1 = 58,
    MonkeyHeavy2 = 59,
    MonkeyKick1 = 60,
    MonkeyKick2 = 61,
    MonkeyJump = 62,
    MonkeySweep = 63,
    MonkeySpecial = 64,
    // Drunken.
    DrunkenLight1 = 65,
    DrunkenLight2 = 66,
    DrunkenLight3 = 67,
    DrunkenHeavy1 = 68,
    DrunkenHeavy2 = 69,
    DrunkenKick1 = 70,
    DrunkenKick2 = 71,
    DrunkenJump = 72,
    DrunkenSweep = 73,
    DrunkenSpecial = 74,
    // Toad.
    ToadLight1 = 75,
    ToadLight2 = 76,
    ToadLight3 = 77,
    ToadHeavy1 = 78,
    ToadHeavy2 = 79,
    ToadKick1 = 80,
    ToadKick2 = 81,
    ToadJump = 82,
    ToadSweep = 83,
    ToadSpecial = 84,
}
impl Move {
    #[rustfmt::skip]
    pub const ALL: [Move; MOVES] = {
        use Move::*;
        [
            Idle, Walk, BlockHigh, BlockLow, Parry, DodgeLeft, DodgeRight, BackHop, HitLight, HitHeavy, KnockDown, GetUp, Grab, Thrown, Victory,
            TigerLight1, TigerLight2, TigerLight3, TigerHeavy1, TigerHeavy2, TigerKick1, TigerKick2, TigerJump, TigerSweep, TigerSpecial,
            CraneLight1, CraneLight2, CraneLight3, CraneHeavy1, CraneHeavy2, CraneKick1, CraneKick2, CraneJump, CraneSweep, CraneSpecial,
            SnakeLight1, SnakeLight2, SnakeLight3, SnakeHeavy1, SnakeHeavy2, SnakeKick1, SnakeKick2, SnakeJump, SnakeSweep, SnakeSpecial,
            MantisLight1, MantisLight2, MantisLight3, MantisHeavy1, MantisHeavy2, MantisKick1, MantisKick2, MantisJump, MantisSweep, MantisSpecial,
            MonkeyLight1, MonkeyLight2, MonkeyLight3, MonkeyHeavy1, MonkeyHeavy2, MonkeyKick1, MonkeyKick2, MonkeyJump, MonkeySweep, MonkeySpecial,
            DrunkenLight1, DrunkenLight2, DrunkenLight3, DrunkenHeavy1, DrunkenHeavy2, DrunkenKick1, DrunkenKick2, DrunkenJump, DrunkenSweep, DrunkenSpecial,
            ToadLight1, ToadLight2, ToadLight3, ToadHeavy1, ToadHeavy2, ToadKick1, ToadKick2, ToadJump, ToadSweep, ToadSpecial,
        ]
    };
    /// The move with this number, or None.
    pub fn from_u8(n: u8) -> Option<Move> {
        Self::ALL.get(n as usize).copied()
    }
    pub fn id(self) -> u8 {
        self as u8
    }
    /// Its row of the table.
    pub fn data(self) -> &'static MoveData {
        &TABLE[self as usize]
    }
    pub fn name(self) -> &'static str {
        self.data().name
    }
    /// The style it belongs to (None for the shared fifteen).
    pub fn style(self) -> Option<Style> {
        let n = self as usize;
        (n >= SHARED).then(|| Style::ALL[(n - SHARED) / PER_STYLE])
    }
    /// Its place among its style's ten (None for the shared fifteen).
    pub fn slot(self) -> Option<Slot> {
        let n = self as usize;
        (n >= SHARED).then(|| Slot::ALL[(n - SHARED) % PER_STYLE])
    }
}

/// What a move is, which says how the client poses it and the simulation treats it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Idle and Walk: loops, with no length.
    Stance,
    /// BlockHigh, BlockLow: held as long as the block is.
    Block,
    /// The parrier's deflection.
    Parry,
    /// DodgeLeft, DodgeRight, BackHop.
    Dodge,
    /// HitLight, HitHeavy: as long as the stun (`FighterView::len`).
    Reaction,
    KnockDown,
    GetUp,
    /// The grab (the attacker's), and a style's grab special (Mantis).
    Grab,
    /// The thrown fighter's.
    Thrown,
    Victory,
    Light,
    Heavy,
    Kick,
    Jump,
    Sweep,
    Special,
}
/// At what height a strike comes in: a high one passes over a crouching (or low) fighter, a low one under a fighter in
/// the air. A standing block stops high and mid; a crouching block stops low.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Height {
    High,
    Mid,
    Low,
}
/// What a block does to a strike.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Guard {
    /// Stopped by a block of its height (it wears the guard down).
    Blockable,
    /// Breaks a block of its height: the blocker is staggered and takes BREAK_SHARE of it.
    GuardBreak,
    /// No block stops it (a dodge, or not being there, does). Cannot be parried.
    Unblockable,
    /// A grab: beats any block, cannot be parried, and a dodge or a strike beats it.
    Grab,
}

/// One row of the move table: everything the simulation does with a move and the client needs to pose it. Times are
/// seconds (a Drunken move's three phases are each multiplied by its `FighterView::scale`), distances metres (reach is
/// centre to centre, flat: two hotdogs touching are 0.09 m apart), angles radians, damage HP, pushback the speed in m/s
/// the strike knocks its target back with (along the striker's facing), stun the seconds the target reels.
#[derive(Clone, Copy, Debug)]
pub struct MoveData {
    pub mv: Move,
    /// What the move is called (original names).
    pub name: &'static str,
    pub kind: Kind,
    /// Wind-up, the window in which it strikes, and the follow-through. Shared moves that loop have all three 0; a
    /// reaction's length is the stun of what hit it (the table's is the usual).
    pub startup: f32,
    pub active: f32,
    pub recovery: f32,
    pub reach: f32,
    /// How far either side of the way the striker faces it reaches.
    pub arc: f32,
    pub damage: f32,
    pub pushback: f32,
    pub stun: f32,
    pub guard: Guard,
    pub height: Height,
    /// Chi gained when it lands (more than 0), or what it costs (less than 0: the specials).
    pub chi: f32,
    /// The moves it chains into: pressing one's button from the start of this move's active window to the end of its
    /// recovery goes straight into it (as soon as the active window is over).
    pub chains: &'static [Move],
    /// It knocks down what it hits (on its last hit).
    pub knockdown: bool,
    /// Metres it carries the fighter forward (backward if less than 0) over its startup and active window; a dodge's is
    /// the distance it travels, its way given by the move.
    pub advance: f32,
    /// How many times it can strike the same fighter, its active window shared evenly between them.
    pub hits: u8,
    /// The window (seconds into the move, [from, to)) in which nothing can touch the fighter; [0, 0] for none.
    pub iframes: [f32; 2],
    /// Low to the ground for all of it: high strikes pass over.
    pub low: bool,
    /// Armoured in its startup: a strike that lands then (not a special) takes its HP but neither staggers the fighter
    /// nor knocks it down; the move goes on.
    pub armor: bool,
}
impl MoveData {
    /// Its length at the table's timing.
    pub fn total(&self) -> f32 {
        self.startup + self.active + self.recovery
    }
    /// Whether it strikes (or grabs) anybody.
    pub fn attacks(&self) -> bool {
        matches!(self.kind, Kind::Light | Kind::Heavy | Kind::Kick | Kind::Jump | Kind::Sweep | Kind::Special | Kind::Grab)
    }
}

const fn shared(mv: Move, name: &'static str, kind: Kind, t: [f32; 3]) -> MoveData {
    MoveData {
        mv,
        name,
        kind,
        startup: t[0],
        active: t[1],
        recovery: t[2],
        reach: 0.,
        arc: 0.,
        damage: 0.,
        pushback: 0.,
        stun: 0.,
        guard: Guard::Blockable,
        height: Height::Mid,
        chi: 0.,
        chains: &[],
        knockdown: false,
        advance: 0.,
        hits: 1,
        iframes: [0., 0.],
        low: false,
        armor: false,
    }
}
/// A strike: its kind, [startup, active, recovery], reach, arc, damage, pushback, stun, height, chi and chains.
#[allow(clippy::too_many_arguments)]
const fn strike(mv: Move, name: &'static str, kind: Kind, t: [f32; 3], reach: f32, arc: f32, damage: f32, pushback: f32, stun: f32, height: Height, chi: f32, chains: &'static [Move]) -> MoveData {
    MoveData { reach, arc, damage, pushback, stun, height, chi, chains, ..shared(mv, name, kind, t) }
}
impl MoveData {
    const fn down(mut self) -> Self {
        self.knockdown = true;
        self
    }
    const fn breaks(mut self) -> Self {
        self.guard = Guard::GuardBreak;
        self
    }
    const fn unblockable(mut self) -> Self {
        self.guard = Guard::Unblockable;
        self
    }
    const fn grab(mut self) -> Self {
        self.guard = Guard::Grab;
        self
    }
    const fn ahead(mut self, metres: f32) -> Self {
        self.advance = metres;
        self
    }
    const fn hits(mut self, n: u8) -> Self {
        self.hits = n;
        self
    }
    const fn untouchable(mut self, from: f32, to: f32) -> Self {
        self.iframes = [from, to];
        self
    }
    const fn low(mut self) -> Self {
        self.low = true;
        self
    }
    const fn armored(mut self) -> Self {
        self.armor = true;
        self
    }
}

/// Pi, for the arcs that reach all the way round.
const ROUND: f32 = std::f32::consts::PI;
/// The move table, by move number (see MoveData for the units).
#[rustfmt::skip]
pub static TABLE: [MoveData; MOVES] = {
    use Height::{High, Low, Mid};
    use Kind::{Heavy, Kick, Light, Special, Sweep};
    use Move::*;
    [
    // ----- Shared (0-14).
    shared(Idle, "Fighting Stance", Kind::Stance, [0., 0., 0.]),
    shared(Walk, "Stance Walk", Kind::Stance, [0., 0., 0.]),
    shared(BlockHigh, "High Block", Kind::Block, [0.05, 0., 0.]),
    MoveData { low: true, ..shared(BlockLow, "Low Block", Kind::Block, [0.05, 0., 0.]) },
    shared(Parry, "Deflecting Parry", Kind::Parry, [0., 0.12, 0.18]),
    MoveData { advance: 0.22, iframes: [0.03, 0.27], ..shared(DodgeLeft, "Side Step Left", Kind::Dodge, [0.03, 0.24, 0.17]) },
    MoveData { advance: 0.22, iframes: [0.03, 0.27], ..shared(DodgeRight, "Side Step Right", Kind::Dodge, [0.03, 0.24, 0.17]) },
    MoveData { advance: 0.2, iframes: [0.03, 0.25], ..shared(BackHop, "Back Hop", Kind::Dodge, [0.03, 0.22, 0.17]) },
    shared(HitLight, "Light Hit Reaction", Kind::Reaction, [0., 0., 0.3]),
    shared(HitHeavy, "Heavy Hit Reaction", Kind::Reaction, [0., 0., 0.55]),
    MoveData { iframes: [0., DOWN], low: true, ..shared(KnockDown, "Knockdown", Kind::KnockDown, [0.25, 0., DOWN - 0.25]) },
    MoveData { iframes: [0., 0.3], ..shared(GetUp, "Get Up", Kind::GetUp, [0., 0., 0.45]) },
    MoveData { reach: 0.14, arc: 0.7, damage: 12., pushback: 0.7, stun: 0., guard: Guard::Grab, chi: 10., knockdown: true, ..shared(Grab, "Grab and Throw", Kind::Grab, [0.14, 0.08, 0.42]) },
    shared(Thrown, "Thrown", Kind::Thrown, [0., 0., THROW_AT]),
    shared(Victory, "Victory Bow", Kind::Victory, [0., 0., 2.]),

    // ----- Tiger (15-24): power. Slower, heavier, its heavies and second kick break blocks.
    strike(TigerLight1, "Tiger Claw Swipe", Light, [0.115, 0.055, 0.161], 0.16, 0.65, 5.446, 0.14, 0.32, High, 3.6, &[TigerLight2, TigerKick1]),
    strike(TigerLight2, "Rising Claw", Light, [0.101, 0.055, 0.184], 0.16, 0.65, 6.224, 0.14, 0.32, High, 3.6, &[TigerLight3, TigerHeavy1]),
    strike(TigerLight3, "Raking Claws", Light, [0.138, 0.064, 0.304], 0.17, 0.75, 8.557, 0.35, 0.47, Mid, 4.8, &[TigerHeavy2, TigerKick2]),
    strike(TigerHeavy1, "Tiger Palm", Heavy, [0.294, 0.074, 0.405], 0.18, 0.7, 13.226, 0.525, 0.57, Mid, 6.0, &[TigerHeavy2]).breaks().armored(),
    strike(TigerHeavy2, "Crushing Paw", Heavy, [0.368, 0.083, 0.488], 0.19, 0.8, 17.893, 0.77, 0.62, Mid, 7.2, &[]).breaks().down().armored(),
    strike(TigerKick1, "Tiger Stomp Kick", Kick, [0.184, 0.064, 0.285], 0.19, 0.6, 9.335, 0.35, 0.42, Mid, 4.8, &[TigerKick2]),
    strike(TigerKick2, "Tiger Tail Kick", Kick, [0.23, 0.074, 0.34], 0.2, 0.8, 12.447, 0.525, 0.52, High, 6.0, &[]).breaks(),
    strike(TigerJump, "Pouncing Claw", Kind::Jump, [0.138, 0.092, 0.221], 0.18, 0.8, 11.67, 0.42, 0.47, Mid, 4.8, &[]),
    strike(TigerSweep, "Tiger Tail Sweep", Sweep, [0.184, 0.074, 0.368], 0.19, 1.0, 8.557, 0.245, 0.42, Low, 4.8, &[]).down().low(),
    strike(TigerSpecial, "Roaring Tiger Palm", Special, [0.322, 0.11, 0.552], 0.21, 0.9, 24.895, 1.12, 0.8, Mid, -CHI_MAX, &[]).unblockable().down().ahead(0.1).armored(),

    // ----- Crane (25-34): range. The longest reach, its kicks longest of all.
    strike(CraneLight1, "Beak Jab", Light, [0.105, 0.06, 0.17], 0.19, 0.55, 4.126, 0.105, 0.3, High, 3.6, &[CraneLight2, CraneKick1]),
    strike(CraneLight2, "Wing Slap", Light, [0.095, 0.06, 0.19], 0.19, 0.7, 5.044, 0.105, 0.3, High, 3.6, &[CraneLight3, CraneHeavy1]),
    strike(CraneLight3, "Crane Beak Strike", Light, [0.125, 0.07, 0.31], 0.2, 0.6, 6.42, 0.28, 0.45, Mid, 4.8, &[CraneHeavy2, CraneKick2]),
    strike(CraneHeavy1, "Spreading Wings", Heavy, [0.27, 0.08, 0.42], 0.21, 1.0, 11.006, 0.42, 0.55, Mid, 6.0, &[CraneHeavy2]),
    strike(CraneHeavy2, "Wing Hammer", Heavy, [0.34, 0.09, 0.5], 0.22, 0.8, 13.757, 0.63, 0.6, Mid, 7.2, &[]).breaks().down(),
    strike(CraneKick1, "One-Leg Snap Kick", Kick, [0.16, 0.066, 0.273], 0.25, 0.5, 7.337, 0.28, 0.4, Mid, 4.8, &[CraneKick2]),
    strike(CraneKick2, "Long Crane Kick", Kick, [0.21, 0.08, 0.36], 0.26, 0.5, 10.088, 0.42, 0.5, High, 6.0, &[]),
    strike(CraneJump, "Diving Crane", Kind::Jump, [0.13, 0.1, 0.23], 0.22, 0.7, 9.17, 0.35, 0.45, Mid, 4.8, &[]),
    strike(CraneSweep, "Low Wing Sweep", Sweep, [0.17, 0.08, 0.38], 0.23, 1.0, 6.42, 0.21, 0.4, Low, 4.8, &[]).down().low(),
    strike(CraneSpecial, "Crane Takes Flight", Special, [0.3, 0.15, 0.55], 0.28, 0.7, 22.01, 0.98, 0.8, Mid, -CHI_MAX, &[]).unblockable().down().ahead(0.18),

    // ----- Snake (35-44): speed. Quick out and quick back, short reach, light damage, long poking chains.
    strike(SnakeLight1, "Fang Jab", Light, [0.078, 0.052, 0.125], 0.14, 0.5, 4.998, 0.084, 0.28, High, 3.6, &[SnakeLight2, SnakeKick1]),
    strike(SnakeLight2, "Double Fang", Light, [0.073, 0.052, 0.14], 0.14, 0.5, 5.622, 0.084, 0.28, High, 3.6, &[SnakeLight3, SnakeHeavy1]),
    strike(SnakeLight3, "Coiling Strikes", Light, [0.094, 0.062, 0.229], 0.145, 0.55, 7.495, 0.21, 0.42, Mid, 4.8, &[SnakeKick1, SnakeHeavy2]),
    strike(SnakeHeavy1, "Spitting Palm", Heavy, [0.208, 0.073, 0.312], 0.155, 0.6, 12.493, 0.35, 0.52, Mid, 6.0, &[SnakeHeavy2]),
    strike(SnakeHeavy2, "Constrictor Elbow", Heavy, [0.25, 0.083, 0.374], 0.16, 0.7, 16.241, 0.56, 0.57, Mid, 7.2, &[]).breaks().down(),
    strike(SnakeKick1, "Low Viper Kick", Kick, [0.125, 0.062, 0.218], 0.17, 0.5, 8.745, 0.21, 0.38, Low, 4.8, &[SnakeKick2]),
    strike(SnakeKick2, "Rattle Kick", Kick, [0.156, 0.073, 0.27], 0.18, 0.7, 11.243, 0.35, 0.48, Mid, 6.0, &[]),
    strike(SnakeJump, "Striking Cobra", Kind::Jump, [0.094, 0.094, 0.177], 0.155, 0.6, 9.994, 0.28, 0.42, Mid, 4.8, &[]),
    strike(SnakeSweep, "Slithering Sweep", Sweep, [0.125, 0.073, 0.281], 0.16, 1.1, 7.495, 0.175, 0.38, Low, 4.8, &[]).down().low(),
    strike(SnakeSpecial, "Hundred Fangs", Special, [0.229, 0.416, 0.468], 0.17, 0.6, 7.495, 0.175, 0.5, Mid, -CHI_MAX, &[]).unblockable().hits(5).down().ahead(0.06),

    // ----- Mantis (45-54): counters. Trapping hooks that hold a fighter longer, the strongest grab (its special), and the
    // best reward for a parry (MANTIS_PARRY).
    strike(MantisLight1, "Hook Hand", Light, [0.095, 0.057, 0.152], 0.16, 0.6, 6.001, 0.105, 0.3, High, 3.6, &[MantisLight2, MantisKick1]),
    strike(MantisLight2, "Trapping Hook", Light, [0.085, 0.057, 0.171], 0.16, 0.6, 6.601, 0.07, 0.4, High, 3.6, &[MantisLight3, MantisHeavy1]),
    strike(MantisLight3, "Mantis Chop", Light, [0.114, 0.067, 0.285], 0.17, 0.7, 9.0, 0.28, 0.45, Mid, 4.8, &[MantisHeavy2, MantisKick2]),
    strike(MantisHeavy1, "Claw Pull", Heavy, [0.247, 0.076, 0.38], 0.18, 0.7, 15.002, 0.21, 0.6, Mid, 6.0, &[MantisHeavy2]),
    strike(MantisHeavy2, "Mantis Pincer", Heavy, [0.304, 0.085, 0.456], 0.19, 0.8, 19.203, 0.63, 0.6, Mid, 7.2, &[]).breaks().down(),
    strike(MantisKick1, "Mantis Stamp Kick", Kick, [0.152, 0.067, 0.266], 0.2, 0.6, 10.201, 0.28, 0.4, Low, 4.8, &[MantisKick2]),
    strike(MantisKick2, "Hook Kick", Kick, [0.19, 0.076, 0.323], 0.21, 0.8, 13.803, 0.42, 0.5, High, 6.0, &[]),
    strike(MantisJump, "Pouncing Mantis", Kind::Jump, [0.114, 0.095, 0.209], 0.18, 0.8, 12.602, 0.35, 0.45, Mid, 4.8, &[]),
    strike(MantisSweep, "Reaping Sweep", Sweep, [0.152, 0.076, 0.342], 0.19, 1.0, 9.0, 0.21, 0.4, Low, 4.8, &[]).down().low(),
    strike(MantisSpecial, "Mantis Snare", Special, [0.19, 0.095, 0.522], 0.18, 0.8, 36.003, 0.84, 0., Mid, -CHI_MAX, &[]).grab().down(),

    // ----- Monkey (55-64): acrobatics. Strikes from odd heights (a low slap, a rolling fist that passes under high
    // strikes), the best dodge (`DODGE_MONKEY`).
    strike(MonkeyLight1, "Monkey Slap", Light, [0.095, 0.06, 0.15], 0.15, 0.65, 4.29, 0.105, 0.3, High, 3.6, &[MonkeyLight2, MonkeyKick1]),
    strike(MonkeyLight2, "Monkey Swat", Light, [0.085, 0.06, 0.17], 0.15, 0.65, 4.826, 0.105, 0.3, Low, 3.6, &[MonkeyLight3, MonkeyHeavy1]),
    strike(MonkeyLight3, "Tumbling Punch", Light, [0.115, 0.07, 0.28], 0.17, 0.7, 7.509, 0.28, 0.45, Mid, 4.8, &[MonkeyHeavy2, MonkeyKick2]),
    strike(MonkeyHeavy1, "Rolling Fist", Heavy, [0.25, 0.08, 0.38], 0.18, 0.7, 12.336, 0.42, 0.55, Mid, 6.0, &[MonkeyHeavy2]).untouchable(0., 0.1).low().ahead(0.1),
    strike(MonkeyHeavy2, "Cartwheel Kick", Heavy, [0.3, 0.09, 0.45], 0.19, 0.9, 16.09, 0.63, 0.6, Mid, 7.2, &[]).breaks().down(),
    strike(MonkeyKick1, "Hop Kick", Kick, [0.168, 0.078, 0.302], 0.19, 0.6, 8.582, 0.28, 0.4, Mid, 4.8, &[MonkeyKick2]),
    strike(MonkeyKick2, "Flip Kick", Kick, [0.213, 0.09, 0.358], 0.199, 0.7, 11.799, 0.42, 0.5, Mid, 6.0, &[]).untouchable(0.05, 0.15),
    strike(MonkeyJump, "Leaping Monkey", Kind::Jump, [0.11, 0.1, 0.21], 0.18, 0.9, 10.727, 0.35, 0.45, Mid, 4.8, &[]),
    strike(MonkeySweep, "Tail Sweep", Sweep, [0.15, 0.08, 0.34], 0.19, 1.2, 7.509, 0.21, 0.4, Low, 4.8, &[]).down().low(),
    strike(MonkeySpecial, "Monkey Tumble Barrage", Special, [0.25, 0.36, 0.5], 0.2, 0.9, 8.582, 0.42, 0.5, Mid, -CHI_MAX, &[]).unblockable().hits(3).down().untouchable(0., 0.25).ahead(0.22),

    // ----- Drunken (65-74): unpredictable. Each strike's timing is scaled afresh (DRUNK), its lunges stumble forward, and
    // its second heavy and its sweep are falls.
    strike(DrunkenLight1, "Cup Jab", Light, [0.092, 0.055, 0.156], 0.165, 0.6, 4.814, 0.105, 0.3, High, 3.6, &[DrunkenLight2, DrunkenKick1]),
    strike(DrunkenLight2, "Swaying Elbow", Light, [0.092, 0.055, 0.175], 0.165, 0.7, 5.777, 0.105, 0.3, Mid, 3.6, &[DrunkenLight3, DrunkenHeavy1]),
    strike(DrunkenLight3, "Stumble Punch", Light, [0.129, 0.064, 0.294], 0.18, 0.7, 8.184, 0.315, 0.45, Mid, 4.8, &[DrunkenHeavy2, DrunkenKick2]).ahead(0.06),
    strike(DrunkenHeavy1, "Tipsy Lunge", Heavy, [0.258, 0.074, 0.386], 0.19, 0.7, 12.517, 0.42, 0.55, Mid, 6.0, &[DrunkenHeavy2]).ahead(0.1),
    strike(DrunkenHeavy2, "Falling Shoulder", Heavy, [0.313, 0.092, 0.506], 0.2, 0.8, 16.369, 0.7, 0.6, Mid, 7.2, &[]).breaks().down().ahead(0.12),
    strike(DrunkenKick1, "Staggering Kick", Kick, [0.156, 0.064, 0.267], 0.205, 0.6, 8.666, 0.28, 0.4, Mid, 4.8, &[DrunkenKick2]),
    strike(DrunkenKick2, "Toppling Kick", Kick, [0.202, 0.074, 0.331], 0.215, 0.8, 11.555, 0.42, 0.5, High, 6.0, &[]),
    strike(DrunkenJump, "Drunken Dive", Kind::Jump, [0.12, 0.092, 0.221], 0.19, 0.8, 10.591, 0.35, 0.45, Mid, 4.8, &[]),
    strike(DrunkenSweep, "Falling Sweep", Sweep, [0.166, 0.083, 0.368], 0.2, 1.1, 8.666, 0.245, 0.42, Low, 4.8, &[]).down().low(),
    strike(DrunkenSpecial, "Bottomless Gourd", Special, [0.368, 0.138, 0.552], 0.23, ROUND, 26.96, 0.98, 0.8, Mid, -CHI_MAX, &[]).unblockable().down().ahead(0.08),

    // ----- Toad (75-84): iron body. Slow to start and slow to recover, short of reach, immensely strong; stomps that come
    // in low, a leaping body slam, and its iron body while it stands its ground (IRON_SHARE).
    strike(ToadLight1, "Belly Bump", Light, [0.14, 0.07, 0.22], 0.15, 0.8, 9.438, 0.245, 0.34, Mid, 3.6, &[ToadLight2, ToadKick1]),
    strike(ToadLight2, "Double-Palm Shove", Light, [0.13, 0.07, 0.24], 0.155, 0.7, 10.484, 0.35, 0.36, Mid, 3.6, &[ToadLight3, ToadHeavy1]),
    strike(ToadLight3, "Puffed Chest Slam", Light, [0.17, 0.08, 0.36], 0.16, 0.9, 14.679, 0.49, 0.5, Mid, 4.8, &[ToadHeavy2, ToadKick2]),
    strike(ToadHeavy1, "Iron Belly Ram", Heavy, [0.36, 0.09, 0.5], 0.17, 0.8, 23.068, 0.63, 0.6, Mid, 6.0, &[ToadHeavy2]).ahead(0.06),
    strike(ToadHeavy2, "Twin Palm Thunder", Heavy, [0.44, 0.1, 0.6], 0.18, 0.9, 30.407, 0.84, 0.65, Mid, 7.2, &[]).breaks().down(),
    strike(ToadKick1, "Heavy Stomp", Kick, [0.22, 0.08, 0.34], 0.17, 0.9, 14.679, 0.28, 0.45, Low, 4.8, &[ToadKick2]),
    strike(ToadKick2, "Quaking Stomp", Kick, [0.28, 0.09, 0.42], 0.18, 2., 18.873, 0.42, 0.5, Low, 6.0, &[]).down(),
    strike(ToadJump, "Leaping Body Slam", Kind::Jump, [0.18, 0.12, 0.3], 0.18, 1.0, 20.97, 0.56, 0.5, Mid, 4.8, &[]).down(),
    strike(ToadSweep, "Squatting Sweep", Sweep, [0.22, 0.09, 0.44], 0.17, 1.2, 13.63, 0.245, 0.42, Low, 4.8, &[]).down().low(),
    strike(ToadSpecial, "Iron Toad Avalanche", Special, [0.42, 0.16, 0.65], 0.22, 1.2, 39.842, 1.05, 0.8, Mid, -CHI_MAX, &[]).unblockable().down().ahead(0.14),
    ]
};

/// Monkey's dodges: their invulnerable window and distance this many times the table's.
pub const DODGE_MONKEY: f32 = 1.25;
/// Each style's walking pace, of the stance pace: the Toad is the slowest on its feet.
pub const STYLE_PACE: [f32; STYLES] = [1., 1., 1.05, 1., 1.05, 1., 0.7];
/// How far each style is knocked back (and thrown), of the strike's pushback: the Toad is heavy.
pub const STYLE_KNOCK: [f32; STYLES] = [1., 1., 1., 1., 1., 1., 0.55];
/// The Toad's iron body (see the module's documentation): the share of a strike's damage that gets through, the chi it
/// costs a point of damage absorbed, the seconds of recoil the striker reels for, the speed (m/s) above which the Toad is
/// not standing its ground, the chi a Toad comes back with, and the chi it gathers a second standing still.
pub const IRON_SHARE: f32 = 0.1;
pub const IRON_CHI: f32 = 2.;
pub const IRON_RECOIL: f32 = 0.15;
pub const IRON_SPEED: f32 = 0.2;
pub const IRON_START: f32 = 40.;
pub const IRON_REGEN: f32 = 3.;
/// The least chi that keeps a Toad's iron body on.
pub const IRON_MIN: f32 = 5.;
/// Drunken strikes: each one's three phases are scaled by a factor drawn from this range as it starts.
pub const DRUNK: [f32; 2] = [0.75, 1.15];
/// A Drunken fighter standing or walking (free, not blocking) sways out of the way of a high or mid strike this often (a
/// "dodged" event; deterministic, from the fight's dice).
pub const DRUNK_SWAY: f32 = 0.1;

// ----- What the client is told.

/// Things that happen, as `Event::kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    /// A strike landed (`amount` the HP it took).
    Hit,
    /// A strike was blocked (`amount` the guard it cost).
    Blocked,
    /// A strike was parried: `by` the parrier, `to` the striker it staggered.
    Parried,
    /// A guard was broken (`amount` the HP that got through).
    Guardbroken,
    /// A grab caught its fighter (the throw follows THROW_AT later, as a Hit).
    Grabbed,
    /// A strike, or a throw, knocked its fighter down.
    Knockdown,
    /// A fighter was knocked out: by HP, or (`ring` true) off the ring or the counter.
    Ko,
    /// A strike passed through a dodge.
    Dodged,
    /// A strike was shrugged off by a Toad's iron body: `by` the striker (it recoils), `to` the Toad, `amount` the HP that
    /// got through.
    Absorbed,
    /// A special was used (`by` its fighter, `mv` the move).
    Special,
}
/// One thing that happened (`View::events`): numbered in order from 1, each told for EVENT_LIFE seconds (so a client
/// takes those it has not seen yet by `id`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: u32,
    pub kind: Option<EventKind>,
    /// The round's clock when it happened.
    pub time: f32,
    /// Who did it (the striker, the parrier, the grabber) and to whom (the hotdog ids; 0 for nobody).
    pub by: u32,
    pub to: u32,
    /// The move it was (`Move` as its number).
    pub mv: u8,
    pub amount: f32,
    /// Where: the struck fighter's chest.
    pub p: [f32; 3],
    /// A KO off the ring or the counter.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ring: bool,
    /// A KO: the hotdog whose blow finished it (`by` is the one credited with it, who did it the most damage; the same
    /// unless somebody else did more). 0 for a ring-out and every other event.
    #[serde(default)]
    pub last: u32,
}
pub const EVENT_LIFE: f32 = 0.5;
/// A block, as `FighterView::block`.
pub const NO_BLOCK: u8 = 0;
pub const BLOCK_HIGH: u8 = 1;
pub const BLOCK_LOW: u8 = 2;
/// One fighter as the client is told it (see `.superpowers/sdd/adhoc/kungfu/interface.md`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FighterView {
    /// The hotdog (`Player::id`).
    pub id: u32,
    /// Its style (`Style` as its number).
    pub style: u8,
    /// The move it is in (`Move` as its number): Idle or Walk when it is doing nothing else.
    pub mv: u8,
    /// The round's clock (`Feast::elapsed`, a frame's `elapsed`) when the move began: time into it is elapsed - started.
    pub started: f32,
    /// Seconds the move lasts this time (its phases × `scale`, or a reaction's stun); 0 for a loop (stance, walk, block).
    pub len: f32,
    /// What its three phases are multiplied by this time (a Drunken strike's; 1 otherwise).
    pub scale: f32,
    /// The move has landed on somebody (a hit, a block, a grab's catch).
    pub connected: bool,
    /// Blocking: NO_BLOCK, BLOCK_HIGH or BLOCK_LOW.
    pub block: u8,
    /// Reeling from a hit, a parry or a broken guard (HitLight, HitHeavy).
    pub stunned: bool,
    /// On the ground (KnockDown) or thrown.
    pub down: bool,
    /// Nothing can touch it just now (a dodge's window, getting up, spawn protection).
    pub invulnerable: bool,
    /// A Toad's iron body is on (it stands its ground with chi): ordinary strikes from in front are shrugged off.
    pub iron: bool,
    /// Chi, 0 to CHI_MAX (full: the special is ready), and guard, 0 to GUARD_MAX.
    pub chi: f32,
    pub guard: f32,
    /// Hits in a row of its latest combo while its victim still reels (0 for none).
    pub combo: u32,
    /// Its grab partner while it grabs or is thrown (the other hotdog's id), else 0.
    pub partner: u32,
}
impl FighterView {
    pub fn style(&self) -> Style {
        Style::from_index(self.style as usize).unwrap_or_default()
    }
    pub fn mv(&self) -> Move {
        Move::from_u8(self.mv).unwrap_or_default()
    }
    /// Seconds into its move at the round's clock `elapsed`.
    pub fn t(&self, elapsed: f32) -> f32 {
        (elapsed - self.started).max(0.)
    }
}
/// What every client is told of the fight (`{type:'kungfu', ...View}` online, `Feast::kungfu_view` offline).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct View {
    /// The round's clock it was taken at.
    pub elapsed: f32,
    /// KOs to win, and the round's end on its clock (0: none).
    pub goal: u32,
    pub ends: f32,
    pub fighters: Vec<FighterView>,
    pub events: Vec<Event>,
}
impl View {
    pub fn fighter(&self, id: u32) -> Option<&FighterView> {
        self.fighters.iter().find(|f| f.id == id)
    }
}

// ----- The fight.

use crate::actor::HOTDOG_MASS;
use crate::feast::{Damage, Feast, Player, Stage, FALL, backstab, chest};
use crate::net::{Command, button};
use glam::{Vec2, Vec3};

/// The map mode (map.rs MODES) that is always played as Kung Fu, and the lobby's name for the mode.
pub const MODE: &str = "kungfu";
/// How far apart in height two fighters can be and still reach each other.
const TALL: f32 = 0.12;
/// A fighter is walking (the Walk move) when it goes faster than this (m/s); slower, it stands (Idle).
const WALKING: f32 = 0.04;
/// Seconds a bot needs to see a strike coming before it can do something about it, at least.
const REACT: f32 = 0.13;

/// What a press asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Button {
    Light,
    Heavy,
    Kick,
    Special,
    Grab,
    Dodge,
}
/// The button a move of a style is pressed with (None for the shared moves, a jump attack and a sweep).
fn button_of(mv: Move) -> Option<Button> {
    match mv.slot()? {
        Slot::Light1 | Slot::Light2 | Slot::Light3 => Some(Button::Light),
        Slot::Heavy1 | Slot::Heavy2 => Some(Button::Heavy),
        Slot::Kick1 | Slot::Kick2 => Some(Button::Kick),
        Slot::Special => Some(Button::Special),
        Slot::Jump | Slot::Sweep => None,
    }
}

/// One step of a fighter's input (`Feast::kungfu_press`): what was pressed this step, and the block as it is held.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Press {
    pub light: bool,
    pub heavy: bool,
    pub kick: bool,
    pub special: bool,
    pub grab: bool,
    pub dodge: bool,
    /// The block is held.
    pub block: bool,
    /// The block was pressed this step (its rising edge: a parry's timing).
    pub tap: bool,
    /// The stick, [right, back] in the fighter's own frame (a dodge's way).
    pub stick: Vec2,
}
impl Press {
    /// A person's command, against the buttons of the one before it (`prev`): see the module's documentation.
    pub fn of(c: &Command, prev: u16) -> Press {
        let rose = |bit: u16| c.has(bit) && prev & bit == 0;
        let (light, heavy) = (rose(button::FIRE), rose(button::SCOPE));
        Press {
            light,
            heavy,
            kick: c.has(button::MELEE),
            special: rose(button::PRONE) || (light && c.has(button::SCOPE)) || (heavy && c.has(button::FIRE)),
            grab: c.has(button::THROW) || c.has(button::EAT),
            dodge: c.has(button::STIM),
            block: c.has(button::GRAB),
            tap: rose(button::GRAB),
            stick: c.stick(),
        }
    }
}

/// A bot's mind (kept with its fighter).
#[derive(Clone, Debug, Default)]
pub struct Brain {
    /// The hotdog it is fighting (0: none).
    pub target: u32,
    /// The round's clock when it may start its next attack.
    think: f32,
    /// The strike coming at it that it has made up its mind about: the striker and when the strike began.
    seen: (u32, f32),
    /// Holding a block until then (crouched for a low one), and pressing it then (a parry; NEG_INFINITY for none).
    guard_until: f32,
    guard_low: bool,
    parry_at: f32,
    /// A dodge to make, and when it decided on it.
    dodge: bool,
    /// How many more links of a chain it means to press, and the start of the move it last pressed one in.
    chain: u8,
    chained: f32,
    /// Its way round its target (+1 or -1), and when it next thinks about it.
    strafe: f32,
    strafe_at: f32,
    /// It has jumped to attack from the air.
    leap: bool,
    /// The opening it means to attack with once in reach, and until when it means to.
    plan: Option<(Opening, f32)>,
    /// When its target was last attacking or reeling.
    busy: f32,
}
/// How a bot opens an attack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opening {
    Light,
    Heavy,
    Kick,
    Sweep,
    Grab,
    Jump,
    Special,
}

/// One hotdog's state in the fight, by `Player::id`.
#[derive(Clone, Debug)]
pub struct Fighter {
    pub id: u32,
    pub style: Style,
    pub mv: Move,
    pub started: f32,
    pub len: f32,
    pub scale: f32,
    pub connected: bool,
    /// The fighters this move has struck, and with which of its hits.
    struck: Vec<(u32, u8)>,
    /// The block's held (by its player or its brain), and when it was last pressed as a parry.
    holding: bool,
    tapped: f32,
    pub guard: f32,
    blocked_at: f32,
    pub chi: f32,
    pub combo: u32,
    combo_on: u32,
    /// HP taken and hits taken in a row while reeling (a combo against it).
    reel: f32,
    reels: u32,
    pub partner: u32,
    caught_at: f32,
    queued: Option<(Button, f32)>,
    way: Vec2,
    /// The buttons of its player's last command (rising edges), and when it may dodge again.
    buttons: u16,
    dodge_ready: f32,
    /// Who hit it last and when (a ring-out's credit).
    pub hitter: u32,
    hit_at: f32,
    /// The HP each hotdog's blows have taken from it in this life (a KO's credit), and the KO a blow gave, once it has
    /// (credited, finisher).
    dealt: Vec<(u32, f32)>,
    ko: Option<(u32, u32)>,
    alive: bool,
    /// A jump attack has been used in this jump.
    air_used: bool,
    pub brain: Brain,
}
impl Fighter {
    fn new(id: u32, style: Style, now: f32) -> Fighter {
        Fighter {
            id,
            style,
            mv: Move::Idle,
            started: now,
            len: 0.,
            scale: 1.,
            connected: false,
            struck: vec![],
            holding: false,
            tapped: f32::NEG_INFINITY,
            guard: GUARD_MAX,
            blocked_at: f32::NEG_INFINITY,
            chi: if style == Style::Toad { IRON_START } else { 0. },
            combo: 0,
            combo_on: 0,
            reel: 0.,
            reels: 0,
            partner: 0,
            caught_at: 0.,
            queued: None,
            way: Vec2::ZERO,
            buttons: 0,
            dodge_ready: f32::NEG_INFINITY,
            hitter: 0,
            hit_at: f32::NEG_INFINITY,
            dealt: vec![],
            ko: None,
            alive: false,
            air_used: false,
            brain: Brain { guard_until: f32::NEG_INFINITY, parry_at: f32::NEG_INFINITY, strafe: 1., ..Default::default() },
        }
    }
    fn t(&self, now: f32) -> f32 {
        now - self.started
    }
    fn begin(&mut self, mv: Move, now: f32, len: f32, scale: f32) {
        (self.mv, self.started, self.len, self.scale, self.connected) = (mv, now, len, scale, false);
        self.struck.clear();
    }
    /// The end of the move's active window, into it.
    fn active_end(&self) -> f32 {
        let d = self.mv.data();
        (d.startup + d.active) * self.scale
    }
    /// Whether it may start a move: standing, walking, blocking (out of block stun) or out of a parry's deflection.
    fn free(&self, now: f32) -> bool {
        match self.mv {
            Move::Idle | Move::Walk => true,
            Move::BlockHigh | Move::BlockLow => self.t(now) >= self.len,
            Move::Parry => self.t(now) >= self.active_end(),
            _ => false,
        }
    }
    fn stunned(&self) -> bool {
        matches!(self.mv, Move::HitLight | Move::HitHeavy)
    }
    fn down(&self) -> bool {
        matches!(self.mv, Move::KnockDown | Move::Thrown)
    }
    /// Whether its move strikes and is still winding up.
    fn winding(&self, now: f32) -> bool {
        let d = self.mv.data();
        d.attacks() && self.t(now) < d.startup * self.scale
    }
    /// The window it cannot be touched in now (a dodge's, a roll's, the ground, getting up).
    fn untouchable(&self, now: f32) -> bool {
        let d = self.mv.data();
        let [from, mut to] = d.iframes;
        if d.kind == Kind::Dodge && self.style == Style::Monkey {
            to *= DODGE_MONKEY;
        }
        let t = self.t(now);
        to > from && t >= from * self.scale && t < to * self.scale
    }
    /// Low to the ground: high strikes pass over.
    fn low(&self, p: &Player) -> bool {
        p.crouch || self.mv.data().low
    }
}

/// The ring: the map's deck every start stands on (the dojo's round steamer lid), its top, and whether it is round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub rect: [f64; 4],
    pub top: f64,
    pub round: bool,
}
impl Ring {
    /// The deck of `level` that every start stands on (the smallest, if several do), or None.
    pub fn of(level: &crate::map::Level) -> Option<Ring> {
        let map = &level.map;
        if map.starts.is_empty() {
            return None;
        }
        map.decks
            .iter()
            .filter(|d| d.height > 0.005)
            .map(|d| Ring { rect: d.rect, top: crate::map::FLOOR + d.height, round: d.style == crate::map::ROUND })
            .filter(|r| map.starts.iter().all(|s| r.holds(s.p[0], s.p[1], 0.)))
            .min_by(|a, b| a.area().total_cmp(&b.area()))
    }
    fn area(&self) -> f64 {
        (self.rect[2] - self.rect[0]) * (self.rect[3] - self.rect[1])
    }
    /// Whether [x, z] is on it, `margin` metres of slack outside its edge.
    pub fn holds(&self, x: f64, z: f64, margin: f64) -> bool {
        let r = [self.rect[0] - margin, self.rect[1] - margin, self.rect[2] + margin, self.rect[3] + margin];
        if self.round { crate::map::round(r, x, z) } else { x >= r[0] && x <= r[2] && z >= r[1] && z <= r[3] }
    }
    pub fn centre(&self) -> Vec2 {
        Vec2::new(((self.rect[0] + self.rect[2]) / 2.) as f32, ((self.rect[1] + self.rect[3]) / 2.) as f32)
    }
    /// The distance from its middle to its nearest edge.
    pub fn radius(&self) -> f32 {
        (((self.rect[2] - self.rect[0]).min(self.rect[3] - self.rect[1])) / 2.) as f32
    }
}

/// The fight (`Feast::kungfu`): every hotdog's fighter (in the order of `Feast::players`), the events of the last
/// EVENT_LIFE seconds, the dice the bots and the Drunken style roll, and the ring.
#[derive(Clone, Debug, Default)]
pub struct Dojo {
    pub fighters: Vec<Fighter>,
    pub events: Vec<Event>,
    next_event: u32,
    dice: u64,
    pub ring: Option<Ring>,
}
impl Dojo {
    /// A number from 0 to 1 (splitmix64).
    fn roll(&mut self) -> f32 {
        self.dice = self.dice.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.dice;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    }
    fn between(&mut self, range: [f32; 2]) -> f32 {
        range[0] + (range[1] - range[0]) * self.roll()
    }
    fn tell(&mut self, mut event: Event) {
        self.next_event += 1;
        event.id = self.next_event;
        self.events.push(event);
    }
    pub fn fighter(&self, id: u32) -> Option<&Fighter> {
        self.fighters.iter().find(|f| f.id == id)
    }
}

/// Whether a fighter's iron body is on (a Toad standing its ground with chi: see the module's documentation).
fn iron(f: &Fighter, p: &Player) -> bool {
    let still = Vec2::new(p.v[0], p.v[2]).length() <= IRON_SPEED;
    f.style == Style::Toad && f.chi >= IRON_MIN && p.alive && p.grounded && !p.launched && still && matches!(f.mv, Move::Idle | Move::Walk | Move::BlockHigh | Move::BlockLow | Move::Parry)
}
/// The way a hotdog of yaw `yaw` faces, flat.
fn facing(yaw: f32) -> Vec2 {
    Vec2::new(-yaw.sin(), -yaw.cos())
}
fn flat(p: [f32; 3]) -> Vec2 {
    Vec2::new(p[0], p[2])
}

// ----- What the client may work out from a fighter as it is told it.

/// The walking pace of a fighter as it is now, of the stance's walk (`Feast::kungfu_command` walks a person at the
/// game's pace times this; the client predicts with the same): nothing in a move, BLOCK_PACE blocking, STANCE_PACE
/// free (times its style's STYLE_PACE).
pub fn pace(f: &FighterView) -> f32 {
    let style = STYLE_PACE[f.style().index()];
    match f.mv() {
        Move::Idle | Move::Walk | Move::Victory => STANCE_PACE * style,
        Move::BlockHigh | Move::BlockLow => BLOCK_PACE * style,
        _ => 0.,
    }
}
/// Whether a fighter may start a move (or jump) at the round's clock `elapsed`.
pub fn free(f: &FighterView, elapsed: f32) -> bool {
    let t = f.t(elapsed);
    match f.mv() {
        Move::Idle | Move::Walk => true,
        Move::BlockHigh | Move::BlockLow => t >= f.len,
        Move::Parry => t >= (TABLE[Move::Parry as usize].startup + TABLE[Move::Parry as usize].active) * f.scale,
        _ => false,
    }
}
/// The metres its move carries a fighter forward over the `dt` seconds to the round's clock `elapsed` (a dodge: along
/// its way, sideways or back).
pub fn lunge(f: &FighterView, elapsed: f32, dt: f32) -> f32 {
    let d = f.mv().data();
    if d.advance == 0. || f.down || f.stunned {
        return 0.;
    }
    let (w0, w1) = if d.kind == Kind::Dodge { (d.startup * f.scale, (d.startup + d.active) * f.scale) } else { (0., (d.startup + d.active) * f.scale) };
    let t = f.t(elapsed);
    let (t0, t1) = ((t - dt).max(w0), t.min(w1));
    let monkey = if d.kind == Kind::Dodge && f.style() == Style::Monkey { DODGE_MONKEY } else { 1. };
    if t1 > t0 && w1 > w0 { d.advance * monkey * (t1 - t0) / (w1 - w0) } else { 0. }
}

impl Feast {
    // ----- Setting a round out.

    /// Sets a round out as Kung Fu (called by `set_rules`): everyone against everyone at the map's starts, no guns, no
    /// pickups, the counter's edge open (a knock can carry a fighter over it), a fighter for each hotdog in a style of its
    /// own (by seat, moving on a style a round), and the clock.
    pub(crate) fn open_dojo(&mut self) {
        self.openings.clear();
        self.stage = Stage::FreeForAll;
        self.famine_at = self.elapsed - 5.;
        self.pickups.clear();
        self.projectiles.clear();
        self.bodies.config.open_edge = true;
        if self.rules.ends() {
            self.ends = self.elapsed + self.rules.time();
        }
        self.alert = self.rules.banner();
        let now = self.elapsed;
        let seed = self.card_seed ^ (self.round as u64).wrapping_mul(0xD1B5_4A32_D192_ED03) ^ 0x6B75_6E67_6675;
        let fighters = self.players.iter().enumerate().map(|(i, p)| Fighter::new(p.id, Style::ALL[(i + self.round as usize + STYLES - 1) % STYLES], now)).collect();
        self.kungfu = Some(Dojo { fighters, events: vec![], next_event: 0, dice: seed, ring: Ring::of(&self.level) });
        let ids: Vec<u32> = self.players.iter().map(|p| p.id).collect();
        for id in ids {
            self.arm_for_kungfu(id);
        }
        self.embody();
    }
    /// A new round of Kung Fu keeps what the people chose to fight in and goes on numbering the events.
    pub(crate) fn inherit_dojo(&mut self, old: Option<Dojo>) {
        let (Some(old), Some(dojo)) = (old, self.kungfu.as_mut()) else {
            return;
        };
        dojo.next_event = old.next_event;
        for (f, p) in dojo.fighters.iter_mut().zip(&self.players) {
            if let Some(was) = old.fighters.iter().find(|o| o.id == f.id).filter(|_| !p.bot) {
                f.style = was.style;
                f.chi = if f.style == Style::Toad { IRON_START } else { 0. };
            }
        }
        let ids: Vec<u32> = self.players.iter().map(|p| p.id).collect();
        for id in ids {
            self.arm_for_kungfu(id);
        }
    }
    /// A hotdog in Kung Fu holds no gun: its "weapon" is its style + 1 (the styles are what it "holds"), with no rounds,
    /// and its hands are empty.
    pub(crate) fn arm_for_kungfu(&mut self, id: u32) {
        let style = self.kungfu.as_ref().and_then(|d| d.fighter(id)).map_or(Style::Tiger, |f| f.style);
        if let Some(p) = self.player_mut(id) {
            p.owned = std::array::from_fn(|w| (1..=STYLES).contains(&w));
            p.ammo = [0; crate::feast::GUNS];
            (p.weapon, p.gun, p.aimed, p.volley) = (style as usize + 1, style as usize + 1, false, 0);
        }
    }
    /// Where a KO'd fighter comes back: the map's start furthest from the living (the first of equals), facing the
    /// middle of the ring (or as the start faces).
    pub(crate) fn dojo_spot(&mut self, id: u32) -> (Vec3, f32) {
        let living: Vec<Vec2> = self.players.iter().filter(|p| p.alive && p.id != id).map(|p| flat(p.p)).collect();
        let ring = self.kungfu.as_ref().and_then(|d| d.ring);
        let n = self.level.map.starts.len().max(1);
        let (mut best, mut far) = (0, f32::NEG_INFINITY);
        for i in 0..n {
            let (at, _) = self.level.start(i);
            let near = living.iter().map(|q| q.distance(Vec2::new(at.x, at.z))).fold(f32::INFINITY, f32::min);
            if near > far + 1e-4 {
                (best, far) = (i, near);
            }
        }
        let (at, yaw) = self.level.start(best);
        let yaw = match ring {
            Some(ring) => {
                let to = ring.centre() - Vec2::new(at.x, at.z);
                if to.length() > 1e-3 { (-to.x).atan2(-to.y) } else { yaw }
            }
            None => yaw,
        };
        (at, yaw)
    }
    /// The hotdog a fall (or a ring-out) of `id` is credited to: in Kung Fu its last hitter, if that was within
    /// RING_CREDIT seconds; 0 otherwise.
    pub(crate) fn ring_credit(&self, id: u32) -> u32 {
        let now = self.elapsed;
        self.kungfu.as_ref().and_then(|d| d.fighter(id)).filter(|f| f.hitter != id && now - f.hit_at <= RING_CREDIT).map_or(0, |f| f.hitter)
    }

    // ----- What the client is told.

    /// The fight as the client is told it (None outside Kung Fu): see `View`.
    pub fn kungfu_view(&self) -> Option<View> {
        let dojo = self.kungfu.as_ref()?;
        let now = self.elapsed;
        let fighters = dojo
            .fighters
            .iter()
            .zip(&self.players)
            .filter(|(_, p)| !p.benched)
            .map(|(f, p)| {
                if !p.alive {
                    return FighterView { id: f.id, style: f.style as u8, scale: 1., chi: f.chi, guard: f.guard, ..Default::default() };
                }
                let victory = self.stage == Stage::RoundOver && self.winner == Some(f.id);
                let (mv, started, len) = if victory { (Move::Victory, self.over_at, 0.) } else { (f.mv, f.started, f.len) };
                FighterView {
                    id: f.id,
                    style: f.style as u8,
                    mv: mv as u8,
                    started,
                    len,
                    scale: f.scale,
                    connected: f.connected,
                    block: match mv {
                        Move::BlockHigh => BLOCK_HIGH,
                        Move::BlockLow => BLOCK_LOW,
                        _ => NO_BLOCK,
                    },
                    stunned: f.stunned(),
                    down: f.down(),
                    invulnerable: f.untouchable(now) || p.shield > 0.,
                    iron: iron(f, p),
                    chi: f.chi,
                    guard: f.guard,
                    combo: f.combo,
                    partner: f.partner,
                }
            })
            .collect();
        Some(View { elapsed: now, goal: self.rules.goal(), ends: self.ends, fighters, events: dojo.events.clone() })
    }

    // ----- Input.

    /// A person's command in Kung Fu, the server's and single player's alike (see the module's documentation): the
    /// crouch, the style, the walk at the pace its fighter has, the jump, the look and what was pressed.
    pub fn kungfu_command(&mut self, id: u32, c: &Command, dt: f32) {
        let now = self.elapsed;
        let Some(i) = self.players.iter().position(|p| p.id == id && p.alive) else {
            return;
        };
        let Some(f) = self.kungfu.as_mut().and_then(|d| d.fighters.get_mut(i)) else {
            return;
        };
        let free = f.free(now);
        let restyled = (1..=STYLES).contains(&(c.weapon as usize)) && free && f.style != Style::ALL[c.weapon as usize - 1];
        if restyled {
            f.style = Style::ALL[c.weapon as usize - 1];
            f.chi = f.chi.min(CHI_MAX - 1.);
        }
        let press = Press::of(c, f.buttons);
        f.buttons = c.buttons;
        let crouch = if free || matches!(f.mv, Move::BlockHigh | Move::BlockLow) { c.has(button::CROUCH) } else { self.players[i].crouch };
        if restyled {
            self.arm_for_kungfu(id);
        }
        self.set_crouch(id, crouch);
        let k = self.kungfu_pace(id);
        let control = if self.body(id).is_some_and(|b| b.launched) { self.bodies.config.control as f32 } else { 1. };
        let step = c.stride(self.tune.pace(false, crouch, false, 1.) * k * control, dt);
        self.walk(id, step, c.yaw());
        if c.has(button::JUMP) && free && self.players[i].grounded {
            self.jump(id, self.tune.leap(false));
        }
        self.look(id, c.pitch());
        self.kungfu_press(id, &press);
    }
    /// The walking pace of a hotdog's fighter now, of the stance's (`pace`; 1 outside Kung Fu).
    pub fn kungfu_pace(&self, id: u32) -> f32 {
        let Some(dojo) = self.kungfu.as_ref() else {
            return 1.;
        };
        let Some(f) = dojo.fighter(id) else {
            return 1.;
        };
        let view = FighterView { style: f.style as u8, mv: f.mv as u8, started: f.started, len: f.len, scale: f.scale, ..Default::default() };
        if f.free(self.elapsed) { pace(&FighterView { mv: if matches!(f.mv, Move::BlockHigh | Move::BlockLow) { f.mv as u8 } else { 0 }, ..view }) } else { 0. }
    }
    /// What a fighter does this step (a person's command, or a bot's mind): the block as held, and the move pressed for
    /// (kept BUFFER seconds when it cannot be started yet).
    pub fn kungfu_press(&mut self, id: u32, press: &Press) {
        let now = self.elapsed;
        let Some(i) = self.players.iter().position(|p| p.id == id && p.alive) else {
            return;
        };
        let Some(f) = self.kungfu.as_mut().and_then(|d| d.fighters.get_mut(i)) else {
            return;
        };
        f.holding = press.block;
        if press.tap && now - f.tapped >= PARRY_REARM {
            f.tapped = now;
        }
        let pressed = if press.special && f.chi >= CHI_MAX {
            Some(Button::Special)
        } else if press.grab {
            Some(Button::Grab)
        } else if press.dodge {
            Some(Button::Dodge)
        } else if press.heavy {
            Some(Button::Heavy)
        } else if press.kick {
            Some(Button::Kick)
        } else if press.light {
            Some(Button::Light)
        } else {
            None
        };
        if let Some(b) = pressed {
            f.queued = Some((b, now));
            f.way = press.stick;
        }
        self.kungfu_go(i);
        self.kungfu_stance(i);
    }
    /// Starts what a fighter has pressed for, if it can be now: from a free stance, the move its button means where it
    /// stands (in the air the jump attack, crouched the sweep); in a strike's chain window, the chain of that button.
    fn kungfu_go(&mut self, i: usize) {
        let now = self.elapsed;
        let (grounded, crouch) = (self.players[i].grounded, self.players[i].crouch);
        let Some(dojo) = self.kungfu.as_mut() else {
            return;
        };
        let f = &mut dojo.fighters[i];
        let Some((b, at)) = f.queued else {
            return;
        };
        if now - at > BUFFER + 1e-6 {
            f.queued = None;
            return;
        }
        let d = f.mv.data();
        let chaining = d.attacks() && d.guard != Guard::Grab && d.kind != Kind::Special && f.t(now) >= f.active_end() - 1e-6;
        let next = if f.free(now) {
            match b {
                Button::Light | Button::Heavy | Button::Kick if !grounded => (!f.air_used).then(|| f.style.move_in(Slot::Jump)),
                Button::Light | Button::Heavy | Button::Kick if crouch => Some(f.style.move_in(Slot::Sweep)),
                Button::Light => Some(f.style.move_in(Slot::Light1)),
                Button::Heavy => Some(f.style.move_in(Slot::Heavy1)),
                Button::Kick => Some(f.style.move_in(Slot::Kick1)),
                Button::Special => (f.chi >= CHI_MAX).then(|| f.style.move_in(Slot::Special)),
                Button::Grab => grounded.then_some(Move::Grab),
                Button::Dodge => (grounded && now >= f.dodge_ready).then(|| {
                    let w = f.way;
                    if w.y > 0.3 || w.length() < 0.3 {
                        Move::BackHop
                    } else if w.x < 0. {
                        Move::DodgeLeft
                    } else {
                        Move::DodgeRight
                    }
                }),
            }
        } else if chaining {
            match b {
                Button::Special if f.chi >= CHI_MAX && f.connected => Some(f.style.move_in(Slot::Special)),
                _ => d.chains.iter().copied().find(|m| button_of(*m) == Some(b)),
            }
        } else {
            None
        };
        let Some(mv) = next else {
            return;
        };
        f.queued = None;
        let data = mv.data();
        let scale = if f.style == Style::Drunken && data.attacks() && mv.style() == Some(Style::Drunken) { dojo.between(DRUNK) } else { 1. };
        let f = &mut dojo.fighters[i];
        f.begin(mv, now, data.total() * scale, scale);
        f.partner = 0;
        if data.kind == Kind::Jump {
            f.air_used = true;
        }
        if data.kind == Kind::Special {
            f.chi = (f.chi + data.chi).max(0.);
            let (by, mv) = (f.id, mv as u8);
            dojo.tell(Event { kind: Some(EventKind::Special), time: now, by, mv, p: chest(&self.players[i]).to_array(), ..Default::default() });
        }
        if data.attacks() {
            self.players[i].shield = 0.;
        }
    }
    /// A free fighter's stance: its block as held (low crouched), else walking or standing.
    fn kungfu_stance(&mut self, i: usize) {
        let now = self.elapsed;
        let p = &self.players[i];
        let moving = Vec2::new(p.v[0], p.v[2]).length() > WALKING;
        let crouch = p.crouch;
        let Some(f) = self.kungfu.as_mut().map(|d| &mut d.fighters[i]) else {
            return;
        };
        if !f.free(now) || (f.mv == Move::Parry && f.t(now) < f.len) {
            return;
        }
        let want = if f.holding {
            if crouch { Move::BlockLow } else { Move::BlockHigh }
        } else if moving {
            Move::Walk
        } else {
            Move::Idle
        };
        if want != f.mv {
            f.begin(want, now, 0., 1.);
        }
        if matches!(want, Move::BlockHigh | Move::BlockLow) {
            f.blocked_at = now;
        }
    }

    // ----- A step.

    /// One step of the fight (from `step`, before the bots choose): the roster kept, KOs told, ring-outs, the guard and
    /// the Toad's chi, the moves run on (ending, chaining, lunging), the strikes that land and the grabs that catch, and
    /// the events of more than EVENT_LIFE seconds ago forgotten.
    pub(crate) fn kungfu_step(&mut self, dt: f32) {
        let now = self.elapsed;
        self.kungfu_roster();
        self.ring_outs();
        for i in 0..self.players.len() {
            if self.players[i].alive {
                self.kungfu_run(i, dt);
            }
        }
        self.strikes(dt);
        self.grabs(dt);
        self.throws();
        let players = &self.players;
        if let Some(dojo) = self.kungfu.as_mut() {
            dojo.events.retain(|e| now - e.time < EVENT_LIFE);
            // A combo is over once its victim no longer reels.
            let reeling: Vec<(u32, bool)> = dojo.fighters.iter().map(|f| (f.id, f.stunned() || f.down())).collect();
            for f in &mut dojo.fighters {
                if f.combo > 0 && !reeling.iter().any(|(id, r)| *id == f.combo_on && *r) {
                    f.combo = 0;
                }
            }
            for (f, p) in dojo.fighters.iter_mut().zip(players) {
                if !(f.stunned() || f.down()) || !p.alive {
                    (f.reel, f.reels) = (0., 0);
                }
            }
        }
    }
    /// The fighters kept in step with the hotdogs: one each, armed for Kung Fu; one that has come back is fresh, one that
    /// has just been knocked out is told of (the KO's credit is what its record says killed it).
    fn kungfu_roster(&mut self) {
        let now = self.elapsed;
        let Some(dojo) = self.kungfu.as_mut() else {
            return;
        };
        if dojo.fighters.len() != self.players.len() || dojo.fighters.iter().zip(&self.players).any(|(f, p)| f.id != p.id) {
            let old = std::mem::take(&mut dojo.fighters);
            dojo.fighters = self.players.iter().enumerate().map(|(i, p)| old.iter().find(|f| f.id == p.id).cloned().unwrap_or_else(|| Fighter::new(p.id, Style::ALL[i % STYLES], now))).collect();
        }
        let mut gone = vec![];
        for (f, p) in dojo.fighters.iter_mut().zip(self.players.iter_mut()) {
            p.carry = -1;
            (p.weapon, p.gun) = (f.style as usize + 1, f.style as usize + 1);
            if p.alive && !f.alive {
                let style = f.style;
                let brain = std::mem::take(&mut f.brain);
                *f = Fighter::new(f.id, style, now);
                f.alive = true;
                f.brain = Brain { target: 0, ..brain };
            } else if !p.alive && f.alive {
                f.alive = false;
                gone.push((f.id, f.partner, p.last_damage, chest(p), f.ko.take()));
                f.partner = 0;
                f.begin(Move::Idle, now, 0., 1.);
            }
        }
        for (id, partner, damage, at, ko) in gone {
            if partner != 0 {
                if let Some(g) = dojo.fighters.iter_mut().find(|g| g.id == partner && g.partner == id) {
                    g.partner = 0;
                    if g.mv == Move::Thrown {
                        g.begin(Move::Idle, now, 0., 1.);
                    }
                }
            }
            // (A blow's KO goes to whoever did the most damage, `blow`; a ring-out's to the last hitter, `ring_credit`.)
            let (by, last) = match ko {
                Some((by, last)) => (by, last),
                None => (if damage.attacker != id { damage.attacker } else { 0 }, 0),
            };
            dojo.tell(Event { kind: Some(EventKind::Ko), time: now, by, to: id, p: at.to_array(), ring: damage.kind == FALL, last, ..Default::default() });
        }
    }
    /// A fighter standing off the ring is rung out: KO'd, to the credit of its last hitter (see `ring_credit`).
    fn ring_outs(&mut self) {
        let Some(ring) = self.kungfu.as_ref().and_then(|d| d.ring) else {
            return;
        };
        let out: Vec<u32> = self.players.iter().filter(|p| p.alive && p.grounded && !p.launched && !ring.holds(p.p[0] as f64, p.p[2] as f64, 0.005)).map(|p| p.id).collect();
        for id in out {
            let at = self.player(id).map_or(Vec3::ZERO, |p| Vec3::from_array(p.p));
            let by = self.ring_credit(id);
            self.harm(id, f32::INFINITY, false, Damage::by(FALL, by, at, Vec3::NEG_Y));
        }
    }
    /// A fighter's move runs on: the guard comes back (and a Toad standing still gathers chi), a move that is over ends
    /// (a knockdown into getting up, the rest into the stance), what it pressed for starts if it now can, and a lunge
    /// or a dodge carries it.
    fn kungfu_run(&mut self, i: usize, dt: f32) {
        let now = self.elapsed;
        let still = Vec2::new(self.players[i].v[0], self.players[i].v[2]).length() <= WALKING;
        let grounded = self.players[i].grounded;
        let Some(dojo) = self.kungfu.as_mut() else {
            return;
        };
        let f = &mut dojo.fighters[i];
        if !matches!(f.mv, Move::BlockHigh | Move::BlockLow) && now - f.blocked_at >= GUARD_WAIT {
            f.guard = (f.guard + GUARD_REGEN * dt).min(GUARD_MAX);
        }
        if f.style == Style::Toad && still && matches!(f.mv, Move::Idle | Move::BlockHigh | Move::BlockLow) {
            f.chi = (f.chi + IRON_REGEN * dt).min(CHI_MAX);
        }
        if grounded {
            f.air_used = false;
        }
        if f.len > 0. && f.t(now) >= f.len - 1e-6 {
            match f.mv {
                Move::KnockDown => {
                    let len = Move::GetUp.data().total();
                    f.begin(Move::GetUp, now, len, 1.);
                }
                Move::BlockHigh | Move::BlockLow => f.len = 0.,
                mv => {
                    if mv.data().kind == Kind::Dodge {
                        f.dodge_ready = now + DODGE_WAIT;
                    }
                    if f.partner != 0 && mv == Move::Thrown {
                        f.partner = 0;
                    }
                    f.begin(Move::Idle, now, 0., 1.);
                }
            }
        }
        self.kungfu_go(i);
        self.kungfu_stance(i);
        // The lunge, or the dodge.
        let Some(f) = self.kungfu.as_ref().map(|d| &d.fighters[i]) else {
            return;
        };
        let view = FighterView { style: f.style as u8, mv: f.mv as u8, started: f.started, len: f.len, scale: f.scale, stunned: f.stunned(), down: f.down(), ..Default::default() };
        let metres = lunge(&view, now, dt);
        if metres > 0. {
            let yaw = self.players[i].yaw;
            let ahead = facing(yaw);
            let way = match f.mv {
                // (Its own left and right: facing (0, -1), the right is +x, as `Command::stride` has the stick.)
                Move::DodgeLeft => Vec2::new(ahead.y, -ahead.x),
                Move::DodgeRight => Vec2::new(-ahead.y, ahead.x),
                Move::BackHop => -ahead,
                _ => ahead,
            };
            let id = self.players[i].id;
            self.bodies.walk(&self.level, id, (way.x * metres) as f64, (way.y * metres) as f64);
        }
    }

    // ----- Strikes.

    /// Every strike in its active window finds the fighters within its reach, its arc and its height, each once a hit,
    /// all of them against the step as it began (so two fighters that strike each other at once both land); then each
    /// lands (`land`).
    fn strikes(&mut self, dt: f32) {
        let now = self.elapsed;
        let Some(dojo) = self.kungfu.as_ref() else {
            return;
        };
        let mut found: Vec<(usize, usize, Move, u8, bool)> = vec![];
        for (i, f) in dojo.fighters.iter().enumerate() {
            let p = &self.players[i];
            let d = f.mv.data();
            if !p.alive || !d.attacks() || d.guard == Guard::Grab || f.stunned() || f.down() {
                continue;
            }
            let (a0, a1, t) = (d.startup * f.scale, (d.startup + d.active) * f.scale, f.t(now));
            if t < a0 || t - dt >= a1 {
                continue;
            }
            let hits = d.hits.max(1);
            let slice = (((t.min(a1 - 1e-6) - a0) / ((a1 - a0) / hits as f32)).floor().max(0.) as u8).min(hits - 1);
            let ahead = facing(p.yaw);
            for (j, g) in dojo.fighters.iter().enumerate() {
                let q = &self.players[j];
                if j == i || !q.alive || f.struck.contains(&(g.id, slice)) {
                    continue;
                }
                let to = flat(q.p) - flat(p.p);
                let far = to.length();
                if far > d.reach || (q.p[1] - p.p[1]).abs() > TALL {
                    continue;
                }
                if far > 1e-4 && ahead.dot(to / far) < d.arc.min(std::f32::consts::PI).cos() - 1e-6 {
                    continue;
                }
                if (d.height == Height::High && g.low(q)) || (d.height == Height::Low && !q.grounded) {
                    continue;
                }
                found.push((i, j, f.mv, slice, slice + 1 == hits));
            }
        }
        for (i, j, mv, slice, last) in found {
            self.land(i, j, mv, slice, last);
        }
    }
    /// A strike of `mv` by fighter `i` reaches fighter `j`: dodged (its invulnerable window, or spawn protection), parried
    /// (a block pressed within PARRY), blocked (a block of its height, from in front, that the strike does not break),
    /// its guard broken, shrugged off (a Toad's iron body), or a hit: the damage (less along a combo, more on a fighter
    /// winding up), the stun or a knockdown, the knock back, the chi.
    fn land(&mut self, i: usize, j: usize, mv: Move, slice: u8, last: bool) {
        let now = self.elapsed;
        let d = mv.data();
        let (p, q) = (self.players[i].clone(), self.players[j].clone());
        if !q.alive {
            return;
        }
        let toward = (flat(q.p) - flat(p.p)).try_normalize().unwrap_or(facing(p.yaw));
        let at = chest(&q).to_array();
        let Some(dojo) = self.kungfu.as_mut() else {
            return;
        };
        let (pid, qid) = (p.id, q.id);
        dojo.fighters[i].struck.push((qid, slice));
        let g = dojo.fighters[j].clone();
        let tell = |dojo: &mut Dojo, kind: EventKind, by: u32, to: u32, amount: f32| dojo.tell(Event { id: 0, kind: Some(kind), time: now, by, to, mv: mv as u8, amount, p: at, ring: false, last: 0 });
        if g.untouchable(now) || q.shield > 0. {
            if g.mv.data().kind == Kind::Dodge || g.mv.data().iframes[1] > 0. {
                tell(dojo, EventKind::Dodged, pid, qid, 0.);
            }
            return;
        }
        // A Drunken fighter's sway.
        if g.style == Style::Drunken && matches!(g.mv, Move::Idle | Move::Walk) && d.height != Height::Low && d.guard != Guard::Grab && dojo.roll() < DRUNK_SWAY {
            tell(dojo, EventKind::Dodged, pid, qid, 0.);
            return;
        }
        let fronted = facing(q.yaw).dot(-toward) >= BLOCK_ARC.cos();
        let covers = |crouched: bool| if crouched { d.height == Height::Low } else { d.height != Height::Low };
        let blockable = matches!(d.guard, Guard::Blockable | Guard::GuardBreak);
        let ready = g.free(now) || matches!(g.mv, Move::BlockHigh | Move::BlockLow);
        let weight = |style: Style| STYLE_KNOCK[style.index()];
        let (f_knock, g_knock) = (weight(dojo.fighters[i].style), weight(g.style));
        let push = |feast: &mut Feast, id: u32, speed: f32, lift: f32| {
            let k = if id == qid { g_knock } else { f_knock };
            feast.bodies.impulse(id, [(toward.x * speed * k) as f64 * HOTDOG_MASS, (lift * k) as f64 * HOTDOG_MASS, (toward.y * speed * k) as f64 * HOTDOG_MASS]);
        };
        dojo.fighters[i].connected = true;
        // A parry: the block pressed just before the strike, of its height, from in front.
        if blockable && ready && fronted && now - g.tapped <= PARRY + 1e-4 && covers(q.crouch) {
            let mantis = g.style == Style::Mantis;
            let f = &mut dojo.fighters[i];
            f.begin(Move::HitHeavy, now, PARRY_STUN + if mantis { MANTIS_PARRY[0] } else { 0. }, 1.);
            (f.queued, f.partner) = (None, 0);
            let g = &mut dojo.fighters[j];
            g.begin(Move::Parry, now, Move::Parry.data().total(), 1.);
            g.tapped = f32::NEG_INFINITY;
            g.chi = (g.chi + PARRY_CHI + if mantis { MANTIS_PARRY[1] } else { 0. }).min(CHI_MAX);
            tell(dojo, EventKind::Parried, qid, pid, 0.);
            return;
        }
        // A block, worn down or broken.
        let blocking = matches!(g.mv, Move::BlockHigh | Move::BlockLow);
        if blocking && blockable && fronted && covers(g.mv == Move::BlockLow) {
            let cost = d.damage * GUARD_COST;
            if d.guard == Guard::GuardBreak || g.guard - cost <= 0. {
                let through = d.damage * BREAK_SHARE;
                let g = &mut dojo.fighters[j];
                g.begin(Move::HitHeavy, now, BREAK_STUN, 1.);
                (g.guard, g.queued, g.hitter, g.hit_at) = (BREAK_GUARD, None, pid, now);
                let f = &mut dojo.fighters[i];
                f.chi = (f.chi + d.chi.max(0.) * 0.5).min(CHI_MAX);
                tell(dojo, EventKind::Guardbroken, pid, qid, through);
                push(self, qid, d.pushback, 0.);
                self.blow(j, pid, through, Vec3::from_array(p.p), Vec3::new(toward.x, 0., toward.y));
            } else {
                let g = &mut dojo.fighters[j];
                (g.guard, g.blocked_at, g.hitter, g.hit_at) = (g.guard - cost, now, pid, now);
                let (block, stun) = (g.mv, d.stun * BLOCK_STUN);
                g.begin(block, now, stun, 1.);
                let f = &mut dojo.fighters[i];
                f.chi = (f.chi + CHI_BLOCKED).min(CHI_MAX);
                tell(dojo, EventKind::Blocked, pid, qid, cost);
                push(self, qid, d.pushback * BLOCK_PUSH, 0.);
            }
            return;
        }
        // The Toad's iron body shrugs off what is neither a grab, nor a breaker, nor a special, nor from behind.
        if d.guard == Guard::Blockable && iron(&g, &q) && !backstab(q.p, q.yaw, Vec3::from_array(p.p), Vec3::new(toward.x, 0., toward.y)) {
            let through = d.damage * IRON_SHARE;
            let Some(dojo) = self.kungfu.as_mut() else {
                return;
            };
            let g = &mut dojo.fighters[j];
            g.chi = (g.chi - d.damage * IRON_CHI).max(0.);
            let f = &mut dojo.fighters[i];
            f.begin(Move::HitLight, now, IRON_RECOIL, 1.);
            (f.queued, f.partner) = (None, 0);
            tell(dojo, EventKind::Absorbed, pid, qid, through);
            self.blow(j, pid, through, Vec3::from_array(p.p), Vec3::new(toward.x, 0., toward.y));
            push(self, pid, -0.3, 0.);
            return;
        }
        // Through armour: the HP, and nothing else.
        if g.mv.data().armor && g.winding(now) && d.kind != Kind::Special {
            let Some(dojo) = self.kungfu.as_mut() else {
                return;
            };
            let g = &mut dojo.fighters[j];
            (g.hitter, g.hit_at) = (pid, now);
            let f = &mut dojo.fighters[i];
            f.chi = (f.chi + d.chi.max(0.)).min(CHI_MAX);
            tell(dojo, EventKind::Hit, pid, qid, d.damage);
            self.blow(j, pid, d.damage, Vec3::from_array(p.p), Vec3::new(toward.x, 0., toward.y));
            return;
        }
        // A hit.
        let reeling = g.stunned() || g.down() || q.launched;
        let counter = if g.winding(now) { COUNTER } else { 1. };
        let reels = if reeling { g.reels + 1 } else { 0 };
        let amount = d.damage * (1. - COMBO_DECAY * reels as f32).max(COMBO_FLOOR) * counter;
        let reel = if reeling { g.reel } else { 0. } + amount;
        let knock = (d.knockdown && last) || !q.grounded || reel >= COMBO_KNOCK;
        let grabbed = g.partner;
        let g = &mut dojo.fighters[j];
        (g.reels, g.reel, g.hitter, g.hit_at, g.queued, g.partner) = (reels, reel, pid, now, None, 0);
        g.chi = (g.chi + CHI_HURT).min(CHI_MAX);
        if knock {
            g.begin(Move::KnockDown, now, DOWN, 1.);
        } else {
            let heavy = matches!(d.kind, Kind::Heavy | Kind::Special) || amount >= 12.;
            g.begin(if heavy { Move::HitHeavy } else { Move::HitLight }, now, d.stun * counter, 1.);
        }
        // Whoever it held is let go.
        if grabbed != 0 {
            if let Some(h) = dojo.fighters.iter_mut().find(|h| h.id == grabbed && h.partner == qid) {
                h.partner = 0;
                if h.mv == Move::Thrown {
                    h.begin(Move::Idle, now, 0., 1.);
                }
            }
        }
        let f = &mut dojo.fighters[i];
        f.chi = (f.chi + d.chi.max(0.)).min(CHI_MAX);
        f.combo = if reeling && f.combo_on == qid { f.combo + 1 } else { 1 };
        f.combo_on = qid;
        tell(dojo, EventKind::Hit, pid, qid, amount);
        if knock {
            tell(dojo, EventKind::Knockdown, pid, qid, 0.);
        }
        push(self, qid, d.pushback, if knock { LIFT } else { 0. });
        self.blow(j, pid, amount, Vec3::from_array(p.p), Vec3::new(toward.x, 0., toward.y));
    }
    /// A kung fu blow of `amount` HP by `by` on fighter `j`, struck from `from` along `dir`: put on its record (BLOW) and on
    /// the tally of what each hotdog's blows have taken from it in this life. A blow that knocks it out gives the KO to
    /// whoever took the most from it (the last hitter if it is level with the most): `mourn` counted it for the blow's
    /// striker, so it moves to the one credited (the record still says whose blow it was).
    fn blow(&mut self, j: usize, by: u32, amount: f32, from: Vec3, dir: Vec3) {
        let (qid, before) = (self.players[j].id, self.players[j].hp);
        self.harm(qid, amount, true, Damage::by(BLOW, by, from, dir));
        let (taken, died) = ((before - self.players[j].hp).max(0.), before > 0. && !self.players[j].alive);
        let Some(dojo) = self.kungfu.as_mut() else {
            return;
        };
        let g = &mut dojo.fighters[j];
        match g.dealt.iter_mut().find(|(id, _)| *id == by) {
            Some((_, hp)) => *hp += taken,
            None if taken > 0. => g.dealt.push((by, taken)),
            None => {}
        }
        if !died {
            return;
        }
        let most = g.dealt.iter().map(|(_, hp)| *hp).fold(0., f32::max);
        let mine = g.dealt.iter().find(|(id, _)| *id == by).map_or(0., |(_, hp)| *hp);
        let credited = if mine >= most - 1e-3 { by } else { g.dealt.iter().find(|(_, hp)| *hp >= most - 1e-3).map_or(by, |(id, _)| *id) };
        g.ko = Some((credited, by));
        if credited != by {
            if let Some(p) = self.player_mut(by) {
                (p.kills, p.score) = (p.kills.saturating_sub(1), p.score.saturating_sub(100));
            }
            if let Some(p) = self.player_mut(credited) {
                (p.kills, p.score) = (p.kills + 1, p.score + 100);
            }
        }
    }
    /// Every grab in its active window catches the nearest fighter in its reach that can be caught (on its feet, not
    /// reeling, down, untouchable or caught already): a block is no help. Two that grab each other at once break free.
    fn grabs(&mut self, dt: f32) {
        let now = self.elapsed;
        let Some(dojo) = self.kungfu.as_ref() else {
            return;
        };
        let mut caught: Vec<(usize, usize)> = vec![];
        for (i, f) in dojo.fighters.iter().enumerate() {
            let p = &self.players[i];
            let d = f.mv.data();
            if !p.alive || d.guard != Guard::Grab || f.connected {
                continue;
            }
            let (a0, a1, t) = (d.startup * f.scale, (d.startup + d.active) * f.scale, f.t(now));
            if t < a0 || t - dt >= a1 {
                continue;
            }
            let ahead = facing(p.yaw);
            let mut best: Option<(usize, f32)> = None;
            for (j, g) in dojo.fighters.iter().enumerate() {
                let q = &self.players[j];
                if j == i || !q.alive || !q.grounded || q.shield > 0. || g.untouchable(now) || g.stunned() || g.down() || g.partner != 0 {
                    continue;
                }
                let to = flat(q.p) - flat(p.p);
                let far = to.length();
                if far > d.reach || (q.p[1] - p.p[1]).abs() > TALL || (far > 1e-4 && ahead.dot(to / far) < d.arc.cos()) {
                    continue;
                }
                if best.is_none_or(|(_, b)| far < b) {
                    best = Some((j, far));
                }
            }
            if let Some((j, _)) = best {
                caught.push((i, j));
            }
        }
        let Some(dojo) = self.kungfu.as_mut() else {
            return;
        };
        for &(i, j) in &caught {
            let (pid, qid) = (self.players[i].id, self.players[j].id);
            if caught.contains(&(j, i)) {
                // Both grabbed at once: they break free of each other.
                for k in [i, j] {
                    dojo.fighters[k].begin(Move::HitLight, now, 0.25, 1.);
                    dojo.fighters[k].connected = true;
                }
                continue;
            }
            if dojo.fighters[j].partner != 0 || dojo.fighters[i].connected {
                continue;
            }
            let f = &mut dojo.fighters[i];
            (f.connected, f.partner, f.caught_at) = (true, qid, now);
            f.len = f.t(now) + THROW_AT + 0.3;
            let mv = f.mv;
            let g = &mut dojo.fighters[j];
            g.begin(Move::Thrown, now, THROW_AT + 0.05, 1.);
            (g.partner, g.queued, g.holding, g.hitter, g.hit_at) = (pid, None, false, pid, now);
            dojo.tell(Event { kind: Some(EventKind::Grabbed), time: now, by: pid, to: qid, mv: mv as u8, p: chest(&self.players[j]).to_array(), ..Default::default() });
        }
    }
    /// A grab THROW_AT after its catch throws: the damage (a Mantis grab's half as much again), a knockdown, and a
    /// throw along the grabber's facing.
    fn throws(&mut self) {
        let now = self.elapsed;
        let Some(dojo) = self.kungfu.as_ref() else {
            return;
        };
        let due: Vec<(usize, usize)> = dojo
            .fighters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.partner != 0 && f.connected && f.mv.data().guard == Guard::Grab && now >= f.caught_at + THROW_AT - 1e-6)
            .filter_map(|(i, f)| dojo.fighters.iter().position(|g| g.id == f.partner && g.partner == f.id && g.mv == Move::Thrown).map(|j| (i, j)))
            .collect();
        for (i, j) in due {
            let (p, q) = (self.players[i].clone(), self.players[j].clone());
            let Some(dojo) = self.kungfu.as_mut() else {
                return;
            };
            let f = &mut dojo.fighters[i];
            let d = f.mv.data();
            let amount = d.damage * if f.mv == Move::Grab && f.style == Style::Mantis { 1.5 } else { 1. };
            f.partner = 0;
            f.chi = (f.chi + d.chi.max(0.)).min(CHI_MAX);
            let mv = f.mv;
            let g = &mut dojo.fighters[j];
            g.partner = 0;
            g.begin(Move::KnockDown, now, DOWN, 1.);
            (g.hitter, g.hit_at) = (p.id, now);
            let at = chest(&q).to_array();
            dojo.tell(Event { id: 0, kind: Some(EventKind::Hit), time: now, by: p.id, to: q.id, mv: mv as u8, amount, p: at, ring: false, last: 0 });
            dojo.tell(Event { kind: Some(EventKind::Knockdown), time: now, by: p.id, to: q.id, mv: mv as u8, p: at, ..Default::default() });
            let (way, k) = (facing(p.yaw), STYLE_KNOCK[dojo.fighters[j].style.index()]);
            self.bodies.impulse(q.id, [(way.x * d.pushback * k) as f64 * HOTDOG_MASS, (LIFT * k) as f64 * HOTDOG_MASS, (way.y * d.pushback * k) as f64 * HOTDOG_MASS]);
            self.blow(j, p.id, amount, Vec3::from_array(p.p), Vec3::new(way.x, 0., way.y));
        }
    }
}

// ----- Bots.

/// How a bot of a style fights: how far from its target it likes to stand; its chances of answering a strike it sees
/// coming with a parry, a block, a dodge or a duck; the seconds it waits between attacks; how much it likes each opening
/// (light, heavy, kick, sweep, grab, jump attack); and its chance of pressing each next link of a chain.
struct Persona {
    spacing: f32,
    parry: f32,
    block: f32,
    dodge: f32,
    duck: f32,
    think: [f32; 2],
    moves: [f32; 6],
    chain: f32,
}
#[rustfmt::skip]
const PERSONAS: [Persona; STYLES] = [
    Persona { spacing: 0.14, parry: 0.12, block: 0.45, dodge: 0.05, duck: 0.08, think: [0.4, 0.9], moves: [0.35, 0.3, 0.2, 0.07, 0.05, 0.03], chain: 0.7 },
    Persona { spacing: 0.2, parry: 0.14, block: 0.65, dodge: 0.12, duck: 0.05, think: [0.4, 0.9], moves: [0.25, 0.1, 0.45, 0.1, 0.05, 0.05], chain: 0.65 },
    Persona { spacing: 0.12, parry: 0.03, block: 0.15, dodge: 0.06, duck: 0.1, think: [0.3, 0.75], moves: [0.55, 0.1, 0.2, 0.07, 0.05, 0.03], chain: 0.85 },
    Persona { spacing: 0.14, parry: 0.2, block: 0.22, dodge: 0.05, duck: 0.06, think: [0.4, 0.9], moves: [0.4, 0.15, 0.15, 0.07, 0.2, 0.03], chain: 0.7 },
    Persona { spacing: 0.14, parry: 0.06, block: 0.2, dodge: 0.09, duck: 0.1, think: [0.35, 0.85], moves: [0.4, 0.15, 0.2, 0.12, 0.05, 0.08], chain: 0.7 },
    Persona { spacing: 0.15, parry: 0.1, block: 0.35, dodge: 0.12, duck: 0.1, think: [0.3, 1.0], moves: [0.35, 0.2, 0.2, 0.12, 0.08, 0.05], chain: 0.7 },
    Persona { spacing: 0.13, parry: 0.05, block: 0.1, dodge: 0.03, duck: 0.05, think: [0.3, 0.7], moves: [0.4, 0.25, 0.2, 0.08, 0.04, 0.03], chain: 0.7 },
];
/// A bot walks at the bots' pace times its style's (and a Toad no faster than keeps its iron body): the bots' walk Kung Fu
/// was balanced at (actor.rs BOT_WALK before the hotdogs got FASTER), not the game's faster one.
const BOT_WALK: f64 = 0.34;
const TOAD_CREEP: f64 = 0.18;
/// The fastest a bot turns (radians a second).
const BOT_TURN: f32 = 8.;

impl Feast {
    /// The hotdog a bot fights: the nearest, counting each other bot already on it as a little further (so that they
    /// spread out rather than all piling on one), kept unless another is clearly better.
    fn kungfu_target(&self, i: usize) -> Option<usize> {
        let dojo = self.kungfu.as_ref()?;
        let me = flat(self.players[i].p);
        let cost = |j: usize| -> f32 {
            let q = &self.players[j];
            let on = dojo.fighters.iter().enumerate().filter(|(k, f)| *k != i && *k != j && self.players[*k].alive && self.players[*k].bot && f.brain.target == q.id).count();
            me.distance(flat(q.p)) + 0.12 * on as f32 + if dojo.fighters[j].down() { 0.1 } else { 0. }
        };
        let best = (0..self.players.len()).filter(|j| *j != i && self.players[*j].alive && !self.players[*j].benched).min_by(|a, b| cost(*a).total_cmp(&cost(*b)))?;
        let held = dojo.fighters[i].brain.target;
        match (0..self.players.len()).find(|j| *j != i && self.players[*j].id == held && self.players[*j].alive && !self.players[*j].benched) {
            Some(j) if cost(j) <= cost(best) + 0.08 => Some(j),
            _ => Some(best),
        }
    }
    /// A Kung Fu bot's step (see the module's documentation): it picks its target, answers a strike it sees coming
    /// (after REACT) as its style is inclined to (parry, block, dodge, duck, or for a Toad standing its ground nothing at
    /// all), attacks when its target is in reach and it has waited its while (the special when its chi is full, a grab
    /// for a fighter that blocks or stands in iron body, a sweep for one that blocks high), presses on along a chain that
    /// connects, and otherwise keeps its distance, circles, waits its turn behind two others, and stays on the ring.
    pub(crate) fn kungfu_bot(&mut self, id: u32, dt: f32) {
        let now = self.elapsed;
        let Some(i) = self.players.iter().position(|p| p.id == id && p.alive) else {
            return;
        };
        if self.stage != Stage::FreeForAll || self.kungfu.is_none() {
            return;
        }
        let target = self.kungfu_target(i);
        let Some(j) = target else {
            self.kungfu_press(id, &Press::default());
            return;
        };
        let (me, foe) = (self.players[i].clone(), self.players[j].clone());
        let to = flat(foe.p) - flat(me.p);
        let far = to.length();
        let dir = to.try_normalize().unwrap_or(facing(me.yaw));
        let ring = self.kungfu.as_ref().and_then(|d| d.ring);
        let iron_foe = {
            let dojo = self.kungfu.as_ref().unwrap();
            iron(&dojo.fighters[j], &foe)
        };
        let iron_me = {
            let dojo = self.kungfu.as_ref().unwrap();
            iron(&dojo.fighters[i], &me)
        };
        // The strike coming soonest at it, from whoever.
        let threat = {
            let dojo = self.kungfu.as_ref().unwrap();
            dojo.fighters
                .iter()
                .enumerate()
                .filter(|(k, g)| *k != i && self.players[*k].alive && g.winding(now))
                .filter(|(k, g)| {
                    let a = &self.players[*k];
                    let d = g.mv.data();
                    let off = flat(me.p) - flat(a.p);
                    let far = off.length();
                    far <= d.reach + 0.05 && (far < 1e-4 || facing(a.yaw).dot(off / far) >= (d.arc + 0.35).min(std::f32::consts::PI).cos())
                })
                .map(|(_, g)| (g.id, g.started, g.mv, g.started + g.mv.data().startup * g.scale, g.started + g.active_end(), g.t(now)))
                .min_by(|a, b| a.3.total_cmp(&b.3))
        };
        let persona = &PERSONAS[self.kungfu.as_ref().unwrap().fighters[i].style.index()];
        let dojo = self.kungfu.as_mut().unwrap();
        let reaction = REACT + 0.08 * dojo.roll();
        let mut press = Press::default();
        let mut crouch = false;
        // Answering a strike it sees coming.
        if let Some((by, started, mv, impact, ends, t)) = threat {
            let seen = dojo.fighters[i].brain.seen;
            if seen != (by, started) && t >= reaction {
                dojo.fighters[i].brain.seen = (by, started);
                let d = mv.data();
                let r = dojo.roll();
                let b = &mut dojo.fighters[i].brain;
                match d.guard {
                    Guard::Grab => b.dodge = r < persona.dodge * 2. + 0.1,
                    Guard::Unblockable => b.dodge = r < persona.dodge * 2. + 0.15,
                    _ if iron_me && d.guard == Guard::Blockable => {}
                    _ => {
                        let low = d.height == Height::Low;
                        if r < persona.parry {
                            (b.parry_at, b.guard_until, b.guard_low) = (impact - 0.05, ends + 0.05, low);
                        } else if r < persona.parry + persona.block {
                            (b.parry_at, b.guard_until, b.guard_low) = (f32::NEG_INFINITY, ends + 0.05, low);
                        } else if r < persona.parry + persona.block + persona.dodge {
                            b.dodge = true;
                        } else if r < persona.parry + persona.block + persona.dodge + persona.duck && d.height == Height::High {
                            (b.parry_at, b.guard_until, b.guard_low) = (f32::NEG_INFINITY, ends, true);
                        }
                    }
                }
            }
        }
        // Now and then it keeps its guard up while its target is in reach and it is not about to attack.
        let foe_reach = foe_reach(&dojo.fighters[j]);
        let f = &dojo.fighters[i];
        if f.free(now) && !iron_me && far <= foe_reach + 0.03 && now >= f.brain.guard_until + 0.3 && dojo.roll() < persona.block * 0.06 {
            let (low, until) = (foe.crouch, now + dojo.between([0.25, 0.5]));
            let b = &mut dojo.fighters[i].brain;
            (b.parry_at, b.guard_until, b.guard_low) = (f32::NEG_INFINITY, until, low);
        }
        let f = &dojo.fighters[i];
        let blocking = now < f.brain.guard_until && (f.brain.parry_at == f32::NEG_INFINITY || now >= f.brain.parry_at);
        press.block = blocking;
        press.tap = blocking && !f.holding;
        crouch |= blocking && f.brain.guard_low;
        // A dodge, to the side that keeps it on the ring (`right`: its right, facing its target).
        if f.brain.dodge {
            let right = Vec2::new(-dir.y, dir.x);
            let way = match ring {
                Some(ring) => {
                    let c = ring.centre();
                    if (flat(me.p) + right * 0.25).distance(c) < (flat(me.p) - right * 0.25).distance(c) { 1. } else { -1. }
                }
                None => if dojo.roll() < 0.5 { 1. } else { -1. },
            };
            (press.dodge, press.stick) = (true, Vec2::new(way, 0.));
            dojo.fighters[i].brain.dodge = false;
        }
        // Attacking: once it has waited its while it picks an opening, closes to the opening's reach and strikes. A Toad
        // standing in its iron body waits for its target to come to it (and punishes the recoil), unless that target
        // keeps away for a while or blocks.
        let toss = dojo.roll();
        // (Its target is busy with it: striking at it, or reeling from it.)
        let at_me = {
            let g = &dojo.fighters[j];
            let off = flat(me.p) - flat(foe.p);
            (g.mv.data().attacks() && off.length() <= g.mv.data().reach + 0.05 && facing(foe.yaw).dot(off.normalize_or_zero()) > 0.3) || (g.stunned() && g.hitter == id)
        };
        if at_me {
            dojo.fighters[i].brain.busy = now;
        }
        let f = &dojo.fighters[i];
        let foe_f = &dojo.fighters[j];
        let reeling = foe_f.stunned();
        let crowded = {
            let mine = far;
            dojo.fighters
                .iter()
                .enumerate()
                .filter(|(k, g)| *k != i && *k != j && self.players[*k].alive && g.brain.target == foe.id && flat(self.players[*k].p).distance(flat(foe.p)) < mine.min(0.25))
                .count()
                >= 2
        };
        let style = f.style;
        let foe_blocks = matches!(foe_f.mv, Move::BlockHigh | Move::BlockLow);
        let patient = style == Style::Toad && iron_me && !reeling && !foe_blocks && now - f.brain.busy < 0.6;
        if !blocking && !press.dodge && f.free(now) && !crowded {
            if f.brain.plan.is_none() && (now >= f.brain.think || reeling) && !patient {
                let special = style.move_in(Slot::Special).data();
                let (foe_blocks_high, foe_blocks_low, foe_down) = (foe_f.mv == Move::BlockHigh, foe_f.mv == Move::BlockLow, foe_f.down());
                let opening = if f.chi >= CHI_MAX && far <= special.reach * 1.5 && !(style == Style::Toad && toss < 0.5) {
                    Opening::Special
                } else {
                    let mut w = persona.moves;
                    if foe_blocks_high {
                        (w[3], w[4]) = (w[3] * 4. + 0.1, w[4] * 4. + 0.1);
                    }
                    if foe_blocks_low {
                        (w[0], w[1], w[2], w[3]) = (w[0] * 0.2, w[1] * 2., w[2] * 2., 0.);
                    }
                    if iron_foe {
                        (w[0], w[2], w[3], w[4]) = (w[0] * 0.4, w[2] * 0.4, w[3] * 0.4, w[4] * 1.5 + 0.1);
                        if style == Style::Tiger {
                            w[1] *= 3.;
                        }
                    }
                    if foe.crouch || foe_down {
                        w[0] *= 0.2;
                    }
                    let total: f32 = w.iter().sum();
                    let mut r = dojo.roll() * total;
                    let pick = w.iter().position(|x| {
                        r -= x;
                        r < 0.
                    });
                    [Opening::Light, Opening::Heavy, Opening::Kick, Opening::Sweep, Opening::Grab, Opening::Jump][pick.unwrap_or(0).min(5)]
                };
                dojo.fighters[i].brain.plan = Some((opening, now + 1.2));
            }
            let f = &dojo.fighters[i];
            if let Some((opening, until)) = f.brain.plan {
                let reach = match opening {
                    Opening::Light => style.move_in(Slot::Light1).data().reach,
                    Opening::Heavy => style.move_in(Slot::Heavy1).data().reach,
                    Opening::Kick => style.move_in(Slot::Kick1).data().reach,
                    Opening::Sweep => style.move_in(Slot::Sweep).data().reach,
                    Opening::Jump => style.move_in(Slot::Jump).data().reach + 0.04,
                    Opening::Special => style.move_in(Slot::Special).data().reach,
                    Opening::Grab => Move::Grab.data().reach,
                };
                if now > until {
                    let b = &mut dojo.fighters[i].brain;
                    (b.plan, b.think) = (None, now + 0.2);
                } else if far <= reach * 0.9 {
                    let think = now + dojo.between(persona.think);
                    let links = (0..2).take_while(|_| dojo.roll() < persona.chain).count() as u8;
                    let b = &mut dojo.fighters[i].brain;
                    (b.think, b.chain, b.plan) = (think, links, None);
                    match opening {
                        Opening::Light => press.light = true,
                        Opening::Heavy => press.heavy = true,
                        Opening::Kick => press.kick = true,
                        Opening::Sweep => (press.light, crouch) = (true, true),
                        Opening::Grab => press.grab = true,
                        Opening::Special => press.special = true,
                        Opening::Jump => b.leap = true,
                    }
                }
            }
        }
        // Along a chain that connects.
        let f = &dojo.fighters[i];
        let d = f.mv.data();
        if d.attacks() && f.connected && f.brain.chain > 0 && f.brain.chained != f.started && !d.chains.is_empty() {
            let started = f.started;
            let k = (dojo.roll() * d.chains.len() as f32) as usize;
            let next = d.chains[k.min(d.chains.len() - 1)];
            let next = if iron_foe { d.chains.iter().copied().find(|m| m.data().guard == Guard::GuardBreak).unwrap_or(next) } else { next };
            match button_of(next) {
                Some(Button::Light) => press.light = true,
                Some(Button::Heavy) => press.heavy = true,
                Some(Button::Kick) => press.kick = true,
                _ => {}
            }
            let b = &mut dojo.fighters[i].brain;
            (b.chain, b.chained) = (b.chain - 1, started);
        }
        // A jump attack: up, then the kick in the air.
        let f = &dojo.fighters[i];
        let jump = f.brain.leap && me.grounded && f.free(now);
        if f.brain.leap && !me.grounded {
            press.kick = true;
            dojo.fighters[i].brain.leap = false;
        }
        // Moving: closer, further, round, or waiting its turn; on the ring.
        let f = &dojo.fighters[i];
        let free = f.free(now) && !blocking && !press.dodge;
        let mut goal = None;
        if free {
            let spacing = if crowded { persona.spacing + 0.12 } else { persona.spacing };
            let at = flat(me.p);
            let mut g = if f.brain.plan.is_some() || (far > spacing + 0.02) {
                flat(foe.p)
            } else if far < spacing - 0.03 {
                at - dir * 0.1
            } else {
                at + Vec2::new(-dir.y, dir.x) * f.brain.strafe * 0.1
            };
            if let Some(ring) = ring {
                let (c, r) = (ring.centre(), ring.radius() - 0.1);
                // (Away from the edge: towards the middle, the more the nearer the edge; a Toad waiting holds the middle.)
                let out = at.distance(c) / ring.radius();
                if patient {
                    g = g.lerp(c, 0.6);
                } else if out > 0.5 {
                    g = g.lerp(c, (out - 0.5).min(0.5));
                }
                if g.distance(c) > r {
                    g = c + (g - c).normalize_or_zero() * r;
                }
            }
            goal = Some(g);
        }
        if now >= dojo.fighters[i].brain.strafe_at {
            let (way, next) = (if dojo.roll() < 0.5 { 1. } else { -1. }, now + dojo.between([1., 2.5]));
            let b = &mut dojo.fighters[i].brain;
            (b.strafe, b.strafe_at) = (way, next);
        }
        dojo.fighters[i].brain.target = foe.id;
        if let Some(g) = goal {
            let pace = if style == Style::Toad && patient { TOAD_CREEP } else { BOT_WALK * STYLE_PACE[style.index()] as f64 };
            if g.distance(flat(me.p)) > 0.01 {
                self.steer(id, [g.x as f64, me.p[1] as f64, g.y as f64], pace);
            }
        }
        if jump {
            self.jump(id, self.tune.leap(false));
        }
        // It turns to face its target, at BOT_TURN at most.
        let want = (-dir.x).atan2(-dir.y);
        if let Some(p) = self.player_mut(id) {
            let off = (want - p.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            p.yaw += off.clamp(-BOT_TURN * dt, BOT_TURN * dt);
        }
        self.set_crouch(id, crouch);
        self.kungfu_press(id, &press);
    }
}
/// How far a fighter's strikes reach, about (its first kick's, the longest of its openings).
fn foe_reach(f: &Fighter) -> f32 {
    f.style.move_in(Slot::Kick1).data().reach
}

#[cfg(test)]
mod table_tests {
    use super::*;
    /// Prints the move table as markdown (for interface.md): `cargo test -- --ignored --nocapture print_the_move_table`.
    #[test]
    #[ignore]
    fn print_the_move_table() {
        println!("| # | move | name | kind | startup | active | recovery | reach | arc | dmg | push | stun | guard | height | chi | knockdown | advance | hits | iframes | low | armor | chains |");
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        for d in TABLE.iter() {
            let chains: Vec<String> = d.chains.iter().map(|m| format!("{}", *m as u8)).collect();
            println!(
                "| {} | {:?} | {} | {:?} | {} | {} | {} | {} | {:.3} | {} | {} | {} | {:?} | {:?} | {} | {} | {} | {} | {:?} | {} | {} | {} |",
                d.mv as u8, d.mv, d.name, d.kind, d.startup, d.active, d.recovery, d.reach, d.arc, d.damage, d.pushback, d.stun, d.guard, d.height, d.chi, d.knockdown, d.advance, d.hits, d.iframes, d.low, d.armor, chains.join(",")
            );
        }
    }
}

/// The tuning harness (ignored tests, run by hand): `SEEDS=12 cargo test --release -p kitchen-sim --lib -- --ignored
/// --nocapture --exact kungfu::tuning::balance` for the duel table, `ROUNDS=70 ... balance_free_for_all` for five-bot rounds.
#[cfg(test)]
mod tuning {
    use super::*;
    use crate::map::Map;
    /// Two bots of styles `a` and `b` fight alone on the dojo (the rest benched) until one has `kos` KOs or `limit`
    /// seconds are up: the winner's seat (0 or 1), or None for a draw; and the KOs.
    pub fn duel(a: Style, b: Style, seed: u64, kos: u32, limit: f32) -> (Option<usize>, [u32; 2], u32) {
        let mut s = dojo();
        s.card_seed = seed;
        s.set_rules(crate::rules::Rules { kungfu: true, ..Default::default() });
        for p in s.players.iter_mut().skip(2) {
            (p.benched, p.alive, p.consumed, p.hp) = (true, false, true, 0.);
        }
        s.embody();
        let dojo = s.kungfu.as_mut().unwrap();
        for (k, style) in [a, b].into_iter().enumerate() {
            dojo.fighters[k].style = style;
            dojo.fighters[k].chi = if style == Style::Toad { IRON_START } else { 0. };
        }
        for id in [1, 2] {
            s.arm_for_kungfu(id);
        }
        let mut rings = 0;
        let mut seen = 0;
        for _ in 0..(limit * 60.) as usize {
            s.tick(1. / 60.);
            let v = s.kungfu_view().unwrap();
            let last = seen;
            for e in v.events.iter().filter(|e| e.id > last) {
                if e.kind == Some(EventKind::Ko) && e.ring {
                    rings += 1;
                }
                seen = seen.max(e.id);
            }
            if s.players[0].kills >= kos || s.players[1].kills >= kos {
                break;
            }
        }
        let k = [s.players[0].kills, s.players[1].kills];
        (if k[0] > k[1] { Some(0) } else if k[1] > k[0] { Some(1) } else { None }, k, rings)
    }
    /// The balance table: every pair of styles, both ways round, over many seeds.
    #[test]
    #[ignore]
    fn balance() {
        let seeds: u64 = std::env::var("SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(6);
        let mut wins = [[0u32; STYLES]; STYLES];
        let mut games = [0u32; STYLES];
        let mut won = [0u32; STYLES];
        let (mut kos, mut rings) = (0, 0);
        for a in 0..STYLES {
            for b in 0..STYLES {
                if a == b {
                    continue;
                }
                for seed in 0..seeds {
                    let (w, k, r) = duel(Style::ALL[a], Style::ALL[b], seed * 7919 + (a * 31 + b) as u64, 5, 150.);
                    kos += k[0] + k[1];
                    rings += r;
                    games[a] += 1;
                    games[b] += 1;
                    match w {
                        Some(0) => {
                            wins[a][b] += 1;
                            won[a] += 1;
                        }
                        Some(_) => {
                            wins[b][a] += 1;
                            won[b] += 1;
                        }
                        None => {}
                    }
                }
            }
        }
        println!("| style | {} | overall |", Style::NAMES.join(" | "));
        for a in 0..STYLES {
            let row: Vec<String> = (0..STYLES).map(|b| if a == b { "-".into() } else { format!("{:.0}%", 100. * wins[a][b] as f32 / (2 * seeds) as f32) }).collect();
            println!("| {} | {} | {:.1}% ({}/{}) |", Style::NAMES[a], row.join(" | "), 100. * won[a] as f32 / games[a] as f32, won[a], games[a]);
        }
        println!("KOs {kos}, ring-outs {rings} ({:.0}%)", 100. * rings as f32 / kos.max(1) as f32);
    }
    /// Five-bot rounds on the dojo (the styles move on a seat each round, so each plays every seat): how often each style
    /// wins a round it is in (a fair share is 1 in 5), and how long rounds last.
    #[test]
    #[ignore]
    fn balance_free_for_all() {
        let rounds: u64 = std::env::var("ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(35);
        let (mut played, mut won, mut length, mut draws) = ([0u32; STYLES], [0u32; STYLES], 0., 0);
        let mut kd = [(0u32, 0u32); STYLES];
        let mut dealt = [(0f32, 0u32, 0u32, 0u32); STYLES];
        for k in 0..rounds {
            let mut s = dojo();
            s.card_seed = k * 104729 + 1;
            s.round = k as u32 + 1;
            s.set_rules(crate::rules::Rules { kungfu: true, ..Default::default() });
            let mut seen = 0;
            while s.stage != Stage::RoundOver && s.elapsed < TIME + 5. {
                s.tick(1. / 60.);
                let dojo = s.kungfu.as_ref().unwrap();
                let style = |id: u32| dojo.fighter(id).map(|f| f.style.index());
                for e in dojo.events.iter().filter(|e| e.id > seen) {
                    match (e.kind, style(e.by), style(e.to)) {
                        (Some(EventKind::Hit), Some(b), _) => (dealt[b].0, dealt[b].1) = (dealt[b].0 + e.amount, dealt[b].1 + 1),
                        (Some(EventKind::Grabbed), _, Some(t)) => dealt[t].2 += 1,
                        (Some(EventKind::Absorbed), _, Some(t)) => dealt[t].3 += 1,
                        _ => {}
                    }
                }
                seen = dojo.events.iter().map(|e| e.id).max().unwrap_or(seen);
            }
            length += s.elapsed;
            let dojo = s.kungfu.as_ref().unwrap();
            for (f, p) in dojo.fighters.iter().zip(&s.players) {
                played[f.style.index()] += 1;
                kd[f.style.index()].0 += p.kills;
                kd[f.style.index()].1 += p.deaths;
            }
            match s.winner.and_then(|w| dojo.fighter(w)) {
                Some(f) => won[f.style.index()] += 1,
                None => draws += 1,
            }
        }
        println!("| style | rounds in | won | win rate | KOs a round | KO'd a round | HP dealt a round | hits a round | grabbed a round | absorbed a round |");
        println!("|---|---|---|---|---|---|---|---|---|---|");
        for i in 0..STYLES {
            let n = played[i].max(1) as f32;
            println!(
                "| {} | {} | {} | {:.0}% | {:.1} | {:.1} | {:.0} | {:.0} | {:.1} | {:.1} |",
                Style::NAMES[i], played[i], won[i], 100. * won[i] as f32 / n, kd[i].0 as f32 / n, kd[i].1 as f32 / n, dealt[i].0 / n, dealt[i].1 as f32 / n, dealt[i].2 as f32 / n, dealt[i].3 as f32 / n
            );
        }
        println!("{rounds} rounds, {draws} draws, {:.0} s a round", length / rounds as f32);
    }
    fn dojo() -> Feast {
        let text = std::fs::read_to_string(format!("{}/../../assets/maps/kung-fu-dojo.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut s = Feast::with_map(Map::parse(&text).unwrap(), true);
        s.follow = false;
        s
    }
    #[test]
    #[ignore]
    fn smoke_a_bot_match() {
        let mut s = dojo();
        let mut counts = std::collections::BTreeMap::new();
        let mut seen = 0;
        for step in 0..(240 * 60) {
            s.tick(1. / 60.);
            if let Some(v) = s.kungfu_view() {
                for f in &v.fighters {
                    *counts.entry(format!("time-{:?}", f.mv().data().kind)).or_insert(0) += 1;
                }
                let last = seen;
                for e in v.events.iter().filter(|e| e.id > last) {
                    *counts.entry(format!("{:?}{}", e.kind.unwrap(), if e.ring { "-ring" } else { "" })).or_insert(0) += 1;
                    if matches!(e.kind, Some(EventKind::Hit)) {
                        *counts.entry(format!("hit-mv-{:?}", Move::from_u8(e.mv).unwrap().slot())).or_insert(0) += 1;
                    }
                    seen = seen.max(e.id);
                }
            }
            if s.stage == Stage::RoundOver {
                println!("over at step {step}: {}", s.alert);
                break;
            }
        }
        println!("{counts:?}");
        for p in &s.players {
            let f = s.kungfu.as_ref().unwrap().fighter(p.id).unwrap();
            println!("{} {:?} kills {} deaths {} hp {}", p.name, f.style, p.kills, p.deaths, p.hp);
        }
    }
}
#[cfg(test)]
mod diag {
    use super::*;
    #[test]
    #[ignore]
    fn diag_duel() {
        let a = Style::named(&std::env::var("A").unwrap_or("Toad".into())).unwrap();
        let b = Style::named(&std::env::var("B").unwrap_or("Tiger".into())).unwrap();
        let text = std::fs::read_to_string(format!("{}/../../assets/maps/kung-fu-dojo.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut s = Feast::with_map(crate::map::Map::parse(&text).unwrap(), true);
        s.follow = false;
        for p in s.players.iter_mut().skip(2) {
            (p.benched, p.alive, p.consumed, p.hp) = (true, false, true, 0.);
        }
        s.embody();
        let dojo = s.kungfu.as_mut().unwrap();
        for (k, style) in [a, b].into_iter().enumerate() {
            dojo.fighters[k].style = style;
            dojo.fighters[k].chi = if style == Style::Toad { IRON_START } else { 0. };
        }
        let mut counts = std::collections::BTreeMap::new();
        let mut seen = 0;
        let mut iron_time = 0.;
        let mut free_time = [0.; 2];
        for _ in 0..(120 * 60) {
            s.tick(1. / 60.);
            let v = s.kungfu_view().unwrap();
            if v.fighters[0].iron {
                iron_time += 1. / 60.;
            }
            for k in 0..2 {
                if free(&v.fighters[k], v.elapsed) {
                    free_time[k] += 1. / 60.;
                }
            }
            let last = seen;
            for e in v.events.iter().filter(|e| e.id > last) {
                let who = if e.by == 1 { a.name() } else { b.name() };
                let mv = Move::from_u8(e.mv).map(|m| format!("{:?}", m.slot())).unwrap_or_default();
                *counts.entry(format!("{who} {:?} {mv}{}", e.kind.unwrap(), if e.ring { " ring" } else { "" })).or_insert(0) += 1;
                seen = seen.max(e.id);
            }
        }
        for (k, n) in &counts {
            println!("{n:5} {k}");
        }
        println!("KOs {:?}  iron {iron_time:.1}s free {free_time:?}", [s.players[0].kills, s.players[1].kills]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Map;
    use crate::rules::Rules;

    const DT: f32 = 1. / 60.;
    /// The middle of the dojo's ring.
    const C: [f32; 2] = [0., 2.78];

    fn dojo_map() -> Map {
        let text = std::fs::read_to_string(format!("{}/../../assets/maps/kung-fu-dojo.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
        Map::parse(&text).unwrap()
    }
    /// The dojo with bots fighting (a demo's: nobody to wait for).
    fn bout() -> Feast {
        let mut s = Feast::with_map(dojo_map(), true);
        s.follow = false;
        s
    }
    /// The dojo with nobody fighting on their own: hotdog 1 at the middle facing -z, hotdog 2 `gap` metres in front of it
    /// facing it, the rest benched; `a` and `b` their styles.
    fn arena(a: Style, b: Style, gap: f32) -> Feast {
        let mut s = Feast::with_map(dojo_map(), false);
        for p in &mut s.players {
            p.bot = false;
        }
        for p in s.players.iter_mut().skip(2) {
            (p.benched, p.alive, p.consumed, p.hp) = (true, false, true, 0.);
        }
        set_style(&mut s, 1, a);
        set_style(&mut s, 2, b);
        put(&mut s, 1, C[0], C[1] + 0.05, 0.);
        put(&mut s, 2, C[0], C[1] + 0.05 - gap, std::f32::consts::PI);
        run(&mut s, 0.1);
        s
    }
    fn set_style(s: &mut Feast, id: u32, style: Style) {
        let f = s.kungfu.as_mut().unwrap().fighters.iter_mut().find(|f| f.id == id).unwrap();
        f.style = style;
        f.chi = if style == Style::Toad { IRON_START } else { 0. };
        s.arm_for_kungfu(id);
    }
    fn put(s: &mut Feast, id: u32, x: f32, z: f32, yaw: f32) {
        let y = s.level.ground_at(x, z);
        let p = s.player_mut(id).unwrap();
        (p.p, p.yaw) = ([x, y, z], yaw);
        s.embody();
    }
    fn run(s: &mut Feast, seconds: f32) {
        for _ in 0..(seconds / DT).round() as usize {
            s.tick(DT);
        }
    }
    fn f(s: &Feast, id: u32) -> Fighter {
        s.kungfu.as_ref().unwrap().fighter(id).unwrap().clone()
    }
    fn hp(s: &Feast, id: u32) -> f32 {
        s.player(id).unwrap().hp
    }
    fn press(s: &mut Feast, id: u32, press: Press) {
        s.kungfu_press(id, &press);
    }
    fn light() -> Press {
        Press { light: true, ..Default::default() }
    }
    fn heavy() -> Press {
        Press { heavy: true, ..Default::default() }
    }
    fn kick() -> Press {
        Press { kick: true, ..Default::default() }
    }
    fn hold() -> Press {
        Press { block: true, ..Default::default() }
    }
    fn tap() -> Press {
        Press { block: true, tap: true, ..Default::default() }
    }
    /// Every event told since the round began, in order (`run` steps one at a time, so none are missed if read often).
    fn events(s: &Feast) -> Vec<Event> {
        s.kungfu.as_ref().unwrap().events.clone()
    }
    fn told(s: &Feast, kind: EventKind) -> Vec<Event> {
        events(s).into_iter().filter(|e| e.kind == Some(kind)).collect()
    }
    /// Runs until the move `mv` of fighter `id` has landed or `limit` seconds have gone; returns the events of that time.
    fn until_landed(s: &mut Feast, limit: f32) -> Vec<Event> {
        let first = s.kungfu.as_ref().unwrap().next_event;
        let mut all = vec![];
        for _ in 0..(limit / DT) as usize {
            s.tick(DT);
            for e in events(s) {
                if e.id > first && !all.iter().any(|a: &Event| a.id == e.id) {
                    all.push(e);
                }
            }
        }
        all
    }

    // ----- The table.

    #[test]
    fn every_move_has_sane_timings_and_data_and_its_number_is_its_place() {
        assert_eq!((MOVES, STYLES, TABLE.len(), Move::ALL.len()), (85, 7, 85, 85));
        let mut names = std::collections::HashSet::new();
        for (i, d) in TABLE.iter().enumerate() {
            assert_eq!(d.mv as usize, i, "{:?} is in place {i}", d.mv);
            assert_eq!(Move::from_u8(i as u8), Some(d.mv));
            assert!(!d.name.is_empty() && names.insert(d.name), "{:?}: a name of its own", d.mv);
            assert!(d.startup >= 0. && d.active >= 0. && d.recovery >= 0. && d.total() < 3., "{:?}", d.mv);
            assert!(d.hits >= 1 && d.iframes[0] <= d.iframes[1] && d.iframes[1] <= d.total().max(DOWN) + 1e-6, "{:?}", d.mv);
            if d.attacks() {
                assert!(d.startup >= 0.05 && d.active >= 0.04 && d.recovery >= 0.1, "{:?}: {} {} {}", d.mv, d.startup, d.active, d.recovery);
                assert!(d.active / d.hits as f32 >= 1. / 30. - 1e-6, "{:?}: each hit's window outlasts a step", d.mv);
                assert!((0.12..=0.3).contains(&d.reach) && d.arc > 0. && d.arc <= std::f32::consts::PI, "{:?}", d.mv);
                assert!(d.damage > 0. && d.damage <= 40. && d.pushback >= 0. && d.pushback < 2., "{:?}", d.mv);
                assert!(d.chi < CHI_MAX, "{:?}", d.mv);
            }
            match (d.mv.style(), d.mv.slot()) {
                (None, None) => assert!(i < SHARED),
                (Some(style), Some(slot)) => {
                    assert_eq!(style.move_in(slot), d.mv);
                    let kind = match slot {
                        Slot::Light1 | Slot::Light2 | Slot::Light3 => Kind::Light,
                        Slot::Heavy1 | Slot::Heavy2 => Kind::Heavy,
                        Slot::Kick1 | Slot::Kick2 => Kind::Kick,
                        Slot::Jump => Kind::Jump,
                        Slot::Sweep => Kind::Sweep,
                        Slot::Special => Kind::Special,
                    };
                    assert_eq!(d.kind, kind, "{:?}", d.mv);
                    if slot == Slot::Special {
                        assert!(d.chi == -CHI_MAX && matches!(d.guard, Guard::Unblockable | Guard::Grab) && d.knockdown, "{:?}", d.mv);
                    } else {
                        assert!(d.chi > 0., "{:?} gains chi", d.mv);
                    }
                    if slot == Slot::Sweep {
                        assert!(d.height == Height::Low && d.low && d.knockdown, "{:?}", d.mv);
                    }
                    if slot == Slot::Heavy2 {
                        assert_eq!(d.guard, Guard::GuardBreak, "{:?}", d.mv);
                    }
                    // A chain stays in the style, and goes to a move pressed with a button.
                    for next in d.chains {
                        assert_eq!(next.style(), Some(style), "{:?} -> {next:?}", d.mv);
                        assert!(button_of(*next).is_some() && next.data().total() > 0., "{:?} -> {next:?}", d.mv);
                    }
                }
                _ => unreachable!(),
            }
        }
        // Each style's lights chain one into the next; the Toad's are 75 to 84.
        for style in Style::ALL {
            assert!(style.move_in(Slot::Light1).data().chains.contains(&style.move_in(Slot::Light2)));
            assert!(style.move_in(Slot::Light2).data().chains.contains(&style.move_in(Slot::Light3)));
        }
        assert_eq!((Move::ToadLight1 as u8, Move::ToadSpecial as u8, Style::Toad.move_in(Slot::Jump)), (75, 84, Move::ToadJump));
        assert_eq!((Move::DrunkenSpecial as u8, Style::Drunken.move_in(Slot::Special)), (74, Move::DrunkenSpecial));
        // The shared moves the client poses have lengths that add up.
        assert!(Move::Idle.data().total() == 0. && Move::Walk.data().total() == 0.);
        for mv in [Move::Parry, Move::DodgeLeft, Move::DodgeRight, Move::BackHop, Move::KnockDown, Move::GetUp, Move::Grab, Move::Thrown, Move::Victory] {
            assert!(mv.data().total() > 0., "{mv:?}");
        }
        assert!(Move::KnockDown.data().iframes == [0., DOWN] && Move::DodgeLeft.data().iframes[1] > 0.2);
        // Toad: the strongest strikes and the slowest feet.
        let mean = |style: Style| Slot::ALL.iter().map(|s| style.move_in(*s).data().damage).sum::<f32>();
        for style in Style::ALL.iter().filter(|s| **s != Style::Toad) {
            assert!(mean(Style::Toad) > mean(*style), "{style:?}");
            assert!(STYLE_PACE[Style::Toad.index()] < STYLE_PACE[style.index()]);
        }
        assert_eq!(Style::named("toad"), Some(Style::Toad));
    }

    #[test]
    fn kung_fu_is_a_rule_and_a_map_mode_with_no_guns_pickups_or_carrying() {
        let r = Rules::parse("mode=kungfu");
        assert!(r.kungfu && !r.ffa && r.ends() && r.goal() == KOS && r.time() == TIME);
        assert_eq!(r.text(), "mode=kungfu");
        assert_eq!(Rules::parse(&r.text()), r);
        assert!(!Rules::parse("mode=ffa").kungfu && !Rules::default().kungfu);
        // The dojo (a kungfu map) is Kung Fu by itself, round after round.
        let mut s = arena(Style::Tiger, Style::Crane, 0.15);
        assert!(s.kungfu.is_some() && s.rules.kungfu && s.stage == Stage::FreeForAll && s.time_left().is_some());
        assert!(s.pickups.is_empty() && s.bodies.config.open_edge);
        let p = s.player(1).unwrap().clone();
        assert_eq!((p.weapon, p.ammo, p.owned), (1, [0; crate::feast::GUNS], [false, true, true, true, true, true, true, true]));
        assert!(!s.fire(1, Vec3::NEG_Z, 1) && !s.melee(1, Vec3::NEG_Z) && !s.stim(1) && !s.grab(1, Vec3::NEG_Z) && !s.eat(1));
        assert_eq!(s.player(2).unwrap().hp, 100.);
        s.reset();
        assert!(s.kungfu.is_some() && s.pickups.is_empty() && s.stage == Stage::FreeForAll);
        // Any other map plays it when the rules say so; the usual game has no dojo.
        let mut other = Feast::new(false);
        assert!(other.kungfu.is_none());
        other.set_rules(Rules { kungfu: true, ..Default::default() });
        assert!(other.kungfu.is_some() && other.pickups.is_empty() && other.kungfu.as_ref().unwrap().ring.is_none());
        // The dojo's ring is its round deck, which every start stands on.
        let ring = Ring::of(&s.level).unwrap();
        assert!(ring.round && ring.radius() > 0.4 && ring.centre().distance(Vec2::from(C)) < 0.01);
    }

    #[test]
    fn a_persons_command_walks_strikes_blocks_and_chooses_its_style_as_the_server_and_single_player_both_apply_it() {
        let mut s = arena(Style::Tiger, Style::Crane, 0.3);
        let cmd = |seq: u32, buttons: u16, weapon: u8, stick: Vec2| Command::new(seq, stick, 0., 0., buttons, weapon, None);
        // The weapon slot is the style: 7 is the Toad.
        s.kungfu_command(1, &cmd(1, 0, 7, Vec2::ZERO), DT);
        assert_eq!((f(&s, 1).style, s.player(1).unwrap().weapon), (Style::Toad, 7));
        s.kungfu_command(1, &cmd(2, 0, 2, Vec2::ZERO), DT);
        assert_eq!(f(&s, 1).style, Style::Crane);
        // Walking forward, at the stance's pace.
        let z = s.player(1).unwrap().p[2];
        for k in 0..30 {
            s.kungfu_command(1, &cmd(3 + k, 0, 0, Vec2::new(0., -1.)), DT);
            s.tick(DT);
        }
        let walked = z - s.player(1).unwrap().p[2];
        let pace = s.tune.walk * STANCE_PACE;
        assert!((walked - pace * 0.5).abs() < 0.03, "{walked} {pace}");
        // The trigger's rising edge is a light strike; held, nothing more.
        s.kungfu_command(1, &cmd(40, button::FIRE, 0, Vec2::ZERO), DT);
        assert_eq!(f(&s, 1).mv, Move::CraneLight1);
        assert!(s.kungfu_pace(1) == 0., "no walking in a strike");
        run(&mut s, 0.6);
        s.kungfu_command(1, &cmd(41, button::FIRE, 0, Vec2::ZERO), DT);
        assert!(matches!(f(&s, 1).mv, Move::Idle | Move::Walk), "{:?}", f(&s, 1).mv);
        // The grab key held is a block (crouched, low); let go, the stance.
        s.kungfu_command(1, &cmd(42, button::GRAB, 0, Vec2::ZERO), DT);
        assert_eq!(f(&s, 1).mv, Move::BlockHigh);
        assert!(f(&s, 1).tapped == s.elapsed, "the press is the parry's timing");
        s.kungfu_command(1, &cmd(43, button::GRAB | button::CROUCH, 0, Vec2::ZERO), DT);
        assert_eq!(f(&s, 1).mv, Move::BlockLow);
        s.kungfu_command(1, &cmd(44, 0, 0, Vec2::ZERO), DT);
        assert_eq!(f(&s, 1).mv, Move::Idle);
        // The scope's edge is a heavy, melee a kick, use a grab, stim a dodge along the stick, prone the special.
        for (buttons, stick, want) in [
            (button::SCOPE, Vec2::ZERO, Move::CraneHeavy1),
            (button::MELEE, Vec2::ZERO, Move::CraneKick1),
            (button::EAT, Vec2::ZERO, Move::Grab),
            (button::STIM, Vec2::new(-1., 0.), Move::DodgeLeft),
            (button::STIM, Vec2::new(1., 0.), Move::DodgeRight),
            (button::STIM, Vec2::ZERO, Move::BackHop),
            (button::CROUCH | button::FIRE, Vec2::ZERO, Move::CraneSweep),
        ] {
            run(&mut s, 2.);
            s.kungfu_command(1, &cmd(50, 0, 0, Vec2::ZERO), DT);
            s.kungfu_command(1, &cmd(51, buttons, 0, stick), DT);
            assert_eq!(f(&s, 1).mv, want, "{buttons:b}");
            s.set_crouch(1, false);
        }
        run(&mut s, 2.);
        s.kungfu_command(1, &cmd(60, 0, 0, Vec2::ZERO), DT);
        s.kungfu_command(1, &cmd(61, button::PRONE, 0, Vec2::ZERO), DT);
        assert!(matches!(f(&s, 1).mv, Move::Idle | Move::Walk), "no special without chi");
        s.kungfu.as_mut().unwrap().fighters[0].chi = CHI_MAX;
        s.kungfu_command(1, &cmd(62, 0, 0, Vec2::ZERO), DT);
        s.kungfu_command(1, &cmd(63, button::PRONE, 0, Vec2::ZERO), DT);
        assert_eq!((f(&s, 1).mv, f(&s, 1).chi), (Move::CraneSpecial, 0.));
        // A jump, and an attack in the air is the style's jump attack (once a jump).
        run(&mut s, 2.);
        s.kungfu_command(1, &cmd(70, button::JUMP, 0, Vec2::ZERO), DT);
        run(&mut s, 0.1);
        assert!(!s.player(1).unwrap().grounded);
        s.kungfu_command(1, &cmd(71, button::MELEE, 0, Vec2::ZERO), DT);
        assert_eq!(f(&s, 1).mv, Move::CraneJump);
    }

    #[test]
    fn lights_chain_into_the_next_in_the_window_and_a_press_after_the_chain_starts_it_again() {
        let mut s = arena(Style::Snake, Style::Tiger, 0.11);
        let t0 = s.elapsed;
        press(&mut s, 1, light());
        assert_eq!(f(&s, 1).mv, Move::SnakeLight1);
        let d = Move::SnakeLight1.data();
        run(&mut s, d.startup + 0.02);
        assert!(f(&s, 1).connected && hp(&s, 2) < 100., "the jab landed");
        // Pressed during the active window: it goes into the second as soon as the first's window is over.
        press(&mut s, 1, light());
        assert_eq!(f(&s, 1).mv, Move::SnakeLight1, "not before the window is over");
        run(&mut s, 0.04);
        let g = f(&s, 1);
        assert_eq!(g.mv, Move::SnakeLight2);
        assert!((g.started - (t0 + d.startup + d.active)).abs() <= DT + 1e-4, "{} {}", g.started, t0 + d.startup + d.active);
        run(&mut s, 0.1);
        press(&mut s, 1, light());
        run(&mut s, 0.15);
        assert_eq!(f(&s, 1).mv, Move::SnakeLight3);
        assert!(f(&s, 1).combo >= 2, "a combo: {}", f(&s, 1).combo);
        // The third chains into a kick or a heavy, not a fourth light: a light now waits, and after the chain is over
        // starts the first again.
        run(&mut s, 0.12);
        press(&mut s, 1, kick());
        run(&mut s, 0.2);
        assert_eq!(f(&s, 1).mv, Move::SnakeKick1);
        run(&mut s, 1.5);
        assert!(matches!(f(&s, 1).mv, Move::Idle | Move::Walk));
        press(&mut s, 1, light());
        assert_eq!(f(&s, 1).mv, Move::SnakeLight1);
        // A heavy from the second light.
        let mut s = arena(Style::Tiger, Style::Crane, 0.13);
        press(&mut s, 1, light());
        run(&mut s, 0.2);
        press(&mut s, 1, light());
        run(&mut s, 0.2);
        assert_eq!(f(&s, 1).mv, Move::TigerLight2);
        press(&mut s, 1, heavy());
        run(&mut s, 0.12);
        assert_eq!(f(&s, 1).mv, Move::TigerHeavy1);
    }

    #[test]
    fn a_standing_block_stops_high_and_mid_but_not_low_and_a_crouching_block_stops_low_while_high_passes_over() {
        // Standing: the Crane's high jab is blocked (no HP, guard spent), its low sweep gets under.
        let mut s = arena(Style::Crane, Style::Tiger, 0.15);
        press(&mut s, 2, hold());
        run(&mut s, 0.05);
        assert_eq!(f(&s, 2).mv, Move::BlockHigh);
        press(&mut s, 1, light());
        let e = until_landed(&mut s, 0.4);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Blocked) && e.by == 1 && e.to == 2), "{e:?}");
        assert_eq!(hp(&s, 2), 100.);
        assert!(f(&s, 2).guard < GUARD_MAX);
        run(&mut s, 0.6);
        s.set_crouch(1, true);
        press(&mut s, 1, light());
        assert_eq!(f(&s, 1).mv, Move::CraneSweep);
        s.set_crouch(1, false);
        let e = until_landed(&mut s, 0.4);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Hit) && e.to == 2), "{e:?}");
        assert!(hp(&s, 2) < 100.);
        // Crouched: the sweep is blocked, the high jab passes over, a mid kick lands.
        let mut s = arena(Style::Crane, Style::Tiger, 0.15);
        s.set_crouch(2, true);
        press(&mut s, 2, hold());
        run(&mut s, 0.05);
        assert_eq!(f(&s, 2).mv, Move::BlockLow);
        s.set_crouch(1, true);
        press(&mut s, 1, light());
        s.set_crouch(1, false);
        let e = until_landed(&mut s, 0.5);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Blocked)), "{e:?}");
        run(&mut s, 0.6);
        press(&mut s, 1, light());
        assert_eq!(f(&s, 1).mv, Move::CraneLight1);
        assert_eq!(Move::CraneLight1.data().height, Height::High);
        let e = until_landed(&mut s, 0.4);
        assert!(e.is_empty() && hp(&s, 2) == 100., "over the crouch: {e:?}");
        run(&mut s, 0.3);
        press(&mut s, 1, kick());
        assert_eq!(Move::CraneKick1.data().height, Height::Mid);
        let e = until_landed(&mut s, 0.4);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Hit)), "{e:?}");
        // A block covers the front only.
        let mut s = arena(Style::Crane, Style::Tiger, 0.15);
        s.player_mut(2).unwrap().yaw = 0.;
        press(&mut s, 2, hold());
        press(&mut s, 1, light());
        let e = until_landed(&mut s, 0.4);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Hit)), "from behind: {e:?}");
    }

    #[test]
    fn a_block_pressed_within_the_parry_window_before_impact_staggers_the_striker_and_earlier_is_only_a_block() {
        let d = Move::CraneHeavy1.data();
        for (early, parried) in [(0.04, true), (0.1, true), (0.2, false), (0.3, false)] {
            let mut s = arena(Style::Crane, Style::Mantis, 0.15);
            press(&mut s, 1, heavy());
            let impact = s.elapsed + d.startup;
            run(&mut s, d.startup - early);
            press(&mut s, 2, tap());
            let e = until_landed(&mut s, early + 0.1);
            let kinds: Vec<_> = e.iter().filter_map(|e| e.kind).collect();
            if parried {
                assert!(kinds.contains(&EventKind::Parried), "{early}: {kinds:?}");
                assert_eq!(f(&s, 1).mv, Move::HitHeavy, "the striker reels");
                assert!(f(&s, 1).len >= PARRY_STUN + MANTIS_PARRY[0] - 1e-4, "a Mantis parry staggers longer");
                assert!(f(&s, 2).chi >= PARRY_CHI + MANTIS_PARRY[1] - 1e-3 && hp(&s, 2) == 100.);
            } else {
                assert!(kinds.contains(&EventKind::Blocked) && !kinds.contains(&EventKind::Parried), "{early}: {kinds:?}");
            }
            let _ = impact;
        }
        // Tapping again and again does not keep the window open: a press within PARRY_REARM of the last is no parry.
        let mut s = arena(Style::Crane, Style::Tiger, 0.15);
        press(&mut s, 2, tap());
        run(&mut s, 0.05);
        press(&mut s, 2, Press::default());
        press(&mut s, 1, heavy());
        run(&mut s, d.startup - 0.06);
        press(&mut s, 2, tap());
        let e = until_landed(&mut s, 0.15);
        assert!(!e.iter().any(|e| e.kind == Some(EventKind::Parried)), "{e:?}");
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Blocked)), "{e:?}");
        // A parry is no help against a grab or a special.
        let mut s = arena(Style::Crane, Style::Tiger, 0.1);
        press(&mut s, 1, Press { grab: true, ..Default::default() });
        run(&mut s, Move::Grab.data().startup - 0.05);
        press(&mut s, 2, tap());
        let e = until_landed(&mut s, 0.15);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Grabbed)) && !e.iter().any(|e| e.kind == Some(EventKind::Parried)), "{e:?}");
    }

    #[test]
    fn a_guard_breaker_breaks_a_block_and_so_does_a_guard_worn_down() {
        let mut s = arena(Style::Tiger, Style::Crane, 0.15);
        press(&mut s, 2, hold());
        press(&mut s, 1, heavy());
        assert_eq!(Move::TigerHeavy1.data().guard, Guard::GuardBreak);
        let e = until_landed(&mut s, 0.5);
        let broke = e.iter().find(|e| e.kind == Some(EventKind::Guardbroken)).expect("broken");
        assert!((hp(&s, 2) - (100. - Move::TigerHeavy1.data().damage * BREAK_SHARE)).abs() < 0.01, "{}", hp(&s, 2));
        assert!((broke.amount - Move::TigerHeavy1.data().damage * BREAK_SHARE).abs() < 0.01);
        assert_eq!((f(&s, 2).mv, f(&s, 2).guard), (Move::HitHeavy, BREAK_GUARD));
        // Blocked again and again, the guard wears down and breaks.
        let mut s = arena(Style::Crane, Style::Tiger, 0.15);
        press(&mut s, 2, hold());
        let mut broken = false;
        for _ in 0..12 {
            press(&mut s, 1, kick());
            let e = until_landed(&mut s, 0.6);
            if e.iter().any(|e| e.kind == Some(EventKind::Guardbroken)) {
                broken = true;
                break;
            }
            assert!(e.iter().any(|e| e.kind == Some(EventKind::Blocked)), "{e:?}");
        }
        assert!(broken && f(&s, 2).stunned());
        // The guard comes back once the blocking stops.
        let mut s = arena(Style::Crane, Style::Tiger, 0.15);
        s.kungfu.as_mut().unwrap().fighters[1].guard = 10.;
        run(&mut s, GUARD_WAIT + 1.);
        assert!(f(&s, 2).guard >= 10. + GUARD_REGEN * 0.9);
    }

    #[test]
    fn a_grab_beats_a_block_and_throws_and_a_dodge_or_a_quicker_strike_beats_a_grab() {
        let mut s = arena(Style::Mantis, Style::Crane, 0.1);
        press(&mut s, 2, hold());
        run(&mut s, 0.1);
        press(&mut s, 1, Press { grab: true, ..Default::default() });
        let e = until_landed(&mut s, 0.3);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Grabbed) && e.by == 1 && e.to == 2), "{e:?}");
        assert_eq!((f(&s, 2).mv, f(&s, 2).partner, f(&s, 1).partner), (Move::Thrown, 1, 2));
        let e = until_landed(&mut s, THROW_AT + 0.05);
        let thrown = e.iter().find(|e| e.kind == Some(EventKind::Hit) && e.mv == Move::Grab as u8).expect("thrown");
        assert!((thrown.amount - Move::Grab.data().damage * 1.5).abs() < 0.01, "a Mantis throws hardest: {}", thrown.amount);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Knockdown)));
        assert_eq!(f(&s, 2).mv, Move::KnockDown);
        assert!(hp(&s, 2) < 100.);
        // A dodge's invulnerable frames: no catch.
        let mut s = arena(Style::Tiger, Style::Crane, 0.1);
        press(&mut s, 2, Press { dodge: true, ..Default::default() });
        press(&mut s, 1, Press { grab: true, ..Default::default() });
        let e = until_landed(&mut s, 0.4);
        assert!(!e.iter().any(|e| e.kind == Some(EventKind::Grabbed)), "{e:?}");
        // A jab that lands in the grab's wind-up stops it.
        let mut s = arena(Style::Tiger, Style::Snake, 0.1);
        press(&mut s, 1, Press { grab: true, ..Default::default() });
        press(&mut s, 2, light());
        let e = until_landed(&mut s, 0.4);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Hit) && e.by == 2) && !e.iter().any(|e| e.kind == Some(EventKind::Grabbed)), "{e:?}");
    }

    #[test]
    fn a_dodge_is_invulnerable_in_its_window_and_not_after_it() {
        let d = Move::TigerHeavy1.data();
        let dodge = Move::BackHop.data();
        // Into the dodge's window: the strike passes through.
        let mut s = arena(Style::Tiger, Style::Crane, 0.15);
        press(&mut s, 1, heavy());
        run(&mut s, d.startup - 0.08);
        press(&mut s, 2, Press { dodge: true, ..Default::default() });
        assert_eq!(f(&s, 2).mv, Move::BackHop);
        // (Held where it is, so that only the window can save it.)
        s.kungfu.as_mut().unwrap().fighters[1].mv = Move::BackHop;
        let mut through = false;
        for _ in 0..30 {
            put(&mut s, 2, C[0], C[1] + 0.05 - 0.15, std::f32::consts::PI);
            s.tick(DT);
            through |= told(&s, EventKind::Dodged).iter().any(|e| e.by == 1);
        }
        assert!(through && hp(&s, 2) == 100., "dodged");
        // Dodged too early, the window is over by the impact.
        let mut s = arena(Style::Tiger, Style::Crane, 0.15);
        press(&mut s, 2, Press { dodge: true, ..Default::default() });
        run(&mut s, dodge.total() + 0.05);
        press(&mut s, 1, heavy());
        let e = until_landed(&mut s, d.startup + 0.1);
        let _ = e;
        let mut s2 = arena(Style::Tiger, Style::Crane, 0.15);
        press(&mut s2, 2, Press { dodge: true, ..Default::default() });
        let mut hit = false;
        for _ in 0..((dodge.iframes[1] + 0.05) / DT) as usize {
            put(&mut s2, 2, C[0], C[1] + 0.05 - 0.15, std::f32::consts::PI);
            s2.tick(DT);
        }
        press(&mut s2, 1, light());
        for _ in 0..20 {
            put(&mut s2, 2, C[0], C[1] + 0.05 - 0.15, std::f32::consts::PI);
            s2.tick(DT);
            hit |= told(&s2, EventKind::Hit).iter().any(|e| e.to == 2);
        }
        assert!(hit, "after the window it is hit");
        // A Monkey's dodge is invulnerable for longer.
        let mut m = arena(Style::Monkey, Style::Tiger, 0.3);
        press(&mut m, 1, Press { dodge: true, ..Default::default() });
        run(&mut m, dodge.iframes[1] + 0.02);
        assert!(m.kungfu_view().unwrap().fighter(1).unwrap().invulnerable);
    }

    #[test]
    fn a_side_step_goes_to_the_fighters_own_left_or_right_whichever_way_it_faces_and_a_bot_steps_inward() {
        // Where hotdog 1 ends up after a dodge pressed with the stick at `x` (its own right is +1), facing `yaw`.
        let step = |yaw: f32, x: f32| -> (Move, Vec2) {
            let mut s = arena(Style::Tiger, Style::Crane, 0.4);
            put(&mut s, 1, C[0], C[1], yaw);
            run(&mut s, 0.05);
            let from = flat(s.player(1).unwrap().p);
            press(&mut s, 1, Press { dodge: true, stick: Vec2::new(x, 0.), ..Default::default() });
            let mv = f(&s, 1).mv;
            run(&mut s, Move::DodgeLeft.data().total() + 0.1);
            (mv, flat(s.player(1).unwrap().p) - from)
        };
        for yaw in [0., std::f32::consts::FRAC_PI_2, std::f32::consts::PI, -0.7] {
            // The way a walk's stick goes: Command::stride, the stick's x being the hotdog's right.
            let stride = Command::new(0, Vec2::new(1., 0.), yaw, 0., 0, 0, None).stride(1., 1.);
            let right = stride.normalize();
            let (left_mv, left) = step(yaw, -1.);
            let (right_mv, went) = step(yaw, 1.);
            assert_eq!((left_mv, right_mv), (Move::DodgeLeft, Move::DodgeRight));
            assert!(left.dot(right) < -0.15 && went.dot(right) > 0.15, "yaw {yaw}: left {left}, right {went}, its right {right}");
            assert!(left.dot(facing(yaw)).abs() < 0.03 && went.dot(facing(yaw)).abs() < 0.03, "sideways: {left} {went}");
        }
        // At yaw 0 it looks along -z, and its left is -x.
        assert!(step(0., -1.).1.x < -0.15);
        // A bot that dodges near the ring's edge steps towards the middle, on either side. (Its target out of its reach and
        // its next attack a long way off, so that it is free to.)
        for side in [1., -1.] {
            let mut s = arena(Style::Tiger, Style::Crane, 0.4);
            put(&mut s, 2, C[0] + side * 0.3, C[1], std::f32::consts::PI);
            put(&mut s, 1, C[0] + side * 0.3, C[1] + 0.3, std::f32::consts::PI);
            s.players[1].bot = true;
            s.kungfu.as_mut().unwrap().fighters[1].brain.think = f32::INFINITY;
            run(&mut s, 0.05);
            assert!(f(&s, 2).free(s.elapsed));
            let before = flat(s.player(2).unwrap().p).distance(Vec2::from(C));
            s.kungfu.as_mut().unwrap().fighters[1].brain.dodge = true;
            s.tick(DT);
            assert!(matches!(f(&s, 2).mv, Move::DodgeLeft | Move::DodgeRight), "{:?}", f(&s, 2).mv);
            run(&mut s, 0.35);
            let after = flat(s.player(2).unwrap().p).distance(Vec2::from(C));
            assert!(after < before - 0.1, "side {side}: {before} -> {after}");
        }
    }

    #[test]
    fn chi_fills_by_landing_and_parrying_and_a_full_meter_pays_for_an_unblockable_special() {
        let mut s = arena(Style::Snake, Style::Tiger, 0.11);
        let mut before = f(&s, 1).chi;
        for _ in 0..3 {
            press(&mut s, 1, light());
            run(&mut s, 0.6);
            assert!(f(&s, 1).chi > before, "{} {}", f(&s, 1).chi, before);
            before = f(&s, 1).chi;
        }
        assert!(f(&s, 2).chi >= 3. * CHI_HURT - 1e-3, "being hit gives a little");
        // Not enough: no special; full: the special, and the meter is spent.
        run(&mut s, 1.);
        press(&mut s, 1, Press { special: true, ..Default::default() });
        assert_ne!(f(&s, 1).mv, Move::SnakeSpecial);
        s.kungfu.as_mut().unwrap().fighters[0].chi = CHI_MAX;
        s.kungfu.as_mut().unwrap().fighters[1].guard = GUARD_MAX;
        press(&mut s, 2, hold());
        run(&mut s, 0.05);
        press(&mut s, 1, Press { special: true, ..Default::default() });
        assert_eq!((f(&s, 1).mv, f(&s, 1).chi), (Move::SnakeSpecial, 0.));
        assert!(told(&s, EventKind::Special).iter().any(|e| e.by == 1 && e.mv == Move::SnakeSpecial as u8));
        let hp0 = hp(&s, 2);
        let e = until_landed(&mut s, 0.8);
        let hits = e.iter().filter(|e| e.kind == Some(EventKind::Hit) && e.by == 1).count();
        assert!(hits >= 2 && hp(&s, 2) < hp0 && !e.iter().any(|e| e.kind == Some(EventKind::Blocked)), "through the block, several times: {e:?}");
    }

    #[test]
    fn a_big_hit_knocks_down_and_a_fighter_down_or_getting_up_cannot_be_hit_until_it_is_up() {
        // A move that knocks down (the sweep).
        let mut s = arena(Style::Tiger, Style::Crane, 0.15);
        s.set_crouch(1, true);
        press(&mut s, 1, light());
        s.set_crouch(1, false);
        assert!(Move::TigerSweep.data().knockdown);
        let e = until_landed(&mut s, 0.5);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Knockdown) && e.to == 2), "{e:?}");
        assert_eq!(f(&s, 2).mv, Move::KnockDown);
        let v = s.kungfu_view().unwrap();
        assert!(v.fighter(2).unwrap().down && v.fighter(2).unwrap().invulnerable);
        let hp0 = hp(&s, 2);
        run(&mut s, 0.5);
        press(&mut s, 1, kick());
        run(&mut s, 0.3);
        assert_eq!(hp(&s, 2), hp0, "not while down");
        run(&mut s, DOWN);
        // (And COMBO_KNOCK HP in one combo knocks down whatever the move.)
        let mut c = arena(Style::Tiger, Style::Crane, 0.15);
        {
            let g = &mut c.kungfu.as_mut().unwrap().fighters[1];
            g.begin(Move::HitLight, c.elapsed, 1., 1.);
            (g.reel, g.reels) = (COMBO_KNOCK - 1., 3);
        }
        press(&mut c, 1, light());
        let e = until_landed(&mut c, 0.4);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Knockdown) && e.mv == Move::TigerLight1 as u8), "{e:?}");
        assert!(matches!(f(&s, 2).mv, Move::GetUp | Move::Idle | Move::Walk), "{:?}", f(&s, 2).mv);
        run(&mut s, Move::GetUp.data().total());
        assert!(matches!(f(&s, 2).mv, Move::Idle | Move::Walk));
    }

    #[test]
    fn a_ko_scores_for_the_striker_and_the_fighter_is_back_on_the_ring_and_a_ring_out_is_the_last_hitters() {
        let mut s = arena(Style::Tiger, Style::Crane, 0.15);
        s.player_mut(2).unwrap().hp = 5.;
        press(&mut s, 1, light());
        let e = until_landed(&mut s, 0.5);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Ko) && e.by == 1 && e.to == 2 && !e.ring), "{e:?}");
        assert_eq!((s.player(1).unwrap().kills, s.player(2).unwrap().deaths, s.player(1).unwrap().score), (1, 1, 100));
        assert_eq!(s.player(2).unwrap().last_damage.kind, BLOW);
        run(&mut s, RESPAWN + 0.1);
        let p = s.player(2).unwrap();
        let ring = s.kungfu.as_ref().unwrap().ring.unwrap();
        assert!(p.alive && p.hp == 100. && p.shield > 0. && ring.holds(p.p[0] as f64, p.p[2] as f64, 0.), "back on the ring");
        assert_eq!(f(&s, 2).chi, 0.);
        // Off the ring, within RING_CREDIT of a hit: the hitter's KO.
        let mut s = arena(Style::Tiger, Style::Crane, 0.15);
        press(&mut s, 1, light());
        until_landed(&mut s, 0.4);
        put(&mut s, 2, C[0] + 0.7, C[1], 0.);
        run(&mut s, 0.2);
        let ko = told(&s, EventKind::Ko);
        assert!(ko.iter().any(|e| e.by == 1 && e.to == 2 && e.ring), "{ko:?}");
        assert_eq!((s.player(1).unwrap().kills, s.player(2).unwrap().last_damage.kind, s.player(2).unwrap().last_damage.attacker), (1, FALL, 1));
        // Knocked off for real: a special from the ring's edge.
        let mut s = arena(Style::Tiger, Style::Crane, 0.15);
        put(&mut s, 1, C[0], C[1] - 0.25, 0.);
        put(&mut s, 2, C[0], C[1] - 0.4, std::f32::consts::PI);
        s.kungfu.as_mut().unwrap().fighters[0].chi = CHI_MAX;
        press(&mut s, 1, Press { special: true, ..Default::default() });
        run(&mut s, 3.);
        assert!(told(&s, EventKind::Ko).iter().any(|e| e.by == 1 && e.to == 2 && e.ring) || s.player(1).unwrap().kills == 1, "rung out");
        // Walked off alone: nobody's KO.
        let mut s = arena(Style::Tiger, Style::Crane, 0.3);
        put(&mut s, 2, C[0] - 0.7, C[1], 0.);
        run(&mut s, 0.2);
        assert!(!s.player(2).unwrap().alive && s.player(1).unwrap().kills == 0);
        // First to KOS wins.
        let mut s = arena(Style::Tiger, Style::Crane, 0.3);
        s.player_mut(1).unwrap().kills = KOS;
        run(&mut s, 0.1);
        assert_eq!((s.stage, s.winner), (Stage::RoundOver, Some(1)));
        assert!(s.alert.contains("KOS"), "{}", s.alert);
        assert_eq!(s.kungfu_view().unwrap().fighter(1).unwrap().mv(), Move::Victory);
    }

    /// `arena`, with hotdog 3 back as well: 0.12 m to hotdog 2's side, facing it.
    fn three(a: Style, b: Style, c: Style) -> Feast {
        let mut s = arena(a, b, 0.15);
        let p = s.player_mut(3).unwrap();
        (p.benched, p.alive, p.consumed, p.hp) = (false, true, false, 100.);
        put(&mut s, 3, C[0] + 0.12, C[1] + 0.05 - 0.15, std::f32::consts::FRAC_PI_2);
        run(&mut s, DT);
        set_style(&mut s, 3, c);
        run(&mut s, 0.05);
        s
    }
    fn struck(s: &mut Feast, j: usize, by: u32, amount: f32) {
        s.blow(j, by, amount, Vec3::from_array(s.players[by as usize - 1].p), Vec3::Z);
    }

    #[test]
    fn a_ko_goes_to_whoever_did_the_most_damage_in_the_fighters_life_the_last_hitter_if_level_and_a_ring_out_to_the_last_hitter() {
        // Hotdog 1 takes 60, hotdog 3 the last 40: hotdog 1's KO (hotdog 3's blow on the record, and in the event).
        let mut s = three(Style::Tiger, Style::Crane, Style::Snake);
        struck(&mut s, 1, 1, 60.);
        struck(&mut s, 1, 3, 10.);
        struck(&mut s, 1, 3, 30.);
        assert!(!s.player(2).unwrap().alive);
        let (a, b, c) = (s.player(1).unwrap().clone(), s.player(2).unwrap().clone(), s.player(3).unwrap().clone());
        assert_eq!((a.kills, a.score, c.kills, c.score, b.deaths), (1, 100, 0, 0, 1));
        assert_eq!(b.last_damage.attacker, 3, "the record is the finishing blow's");
        run(&mut s, DT);
        let ko = told(&s, EventKind::Ko);
        assert!(ko.iter().any(|e| (e.by, e.to, e.last, e.ring) == (1, 2, 3, false)), "{ko:?}");
        // Level: the last hitter's.
        let mut s = three(Style::Tiger, Style::Crane, Style::Snake);
        struck(&mut s, 1, 1, 50.);
        struck(&mut s, 1, 3, 50.);
        assert_eq!((s.player(1).unwrap().kills, s.player(3).unwrap().kills), (0, 1));
        run(&mut s, DT);
        assert!(told(&s, EventKind::Ko).iter().any(|e| (e.by, e.last) == (3, 3)));
        // What counts is the HP a blow took, not what it would have: hotdog 3's 80 on 30 HP left is 30.
        let mut s = three(Style::Tiger, Style::Crane, Style::Snake);
        struck(&mut s, 1, 1, 70.);
        struck(&mut s, 1, 3, 80.);
        assert_eq!((s.player(1).unwrap().kills, s.player(3).unwrap().kills), (1, 0));
        // A new life, a new tally (once its spawn protection is over).
        run(&mut s, RESPAWN + crate::rules::SHIELD + 0.2);
        assert!(s.player(2).unwrap().alive);
        struck(&mut s, 1, 3, 100.);
        assert_eq!((s.player(1).unwrap().kills, s.player(3).unwrap().kills), (1, 1));
        // Real strikes: the Tiger's palm (17), then the Snake's jab finishes a fighter on 3 HP: the Tiger's KO.
        let mut s = three(Style::Tiger, Style::Crane, Style::Snake);
        press(&mut s, 1, heavy());
        let e = until_landed(&mut s, 0.6);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Hit) && e.by == 1 && e.to == 2), "{e:?}");
        run(&mut s, 1.);
        s.player_mut(2).unwrap().hp = 3.;
        put(&mut s, 3, C[0] + 0.12, C[1] + 0.05 - 0.15, std::f32::consts::FRAC_PI_2);
        put(&mut s, 2, C[0], C[1] + 0.05 - 0.15, std::f32::consts::PI);
        press(&mut s, 3, light());
        let e = until_landed(&mut s, 0.4);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Ko) && (e.by, e.to, e.last) == (1, 2, 3)), "{e:?}");
        assert_eq!((s.player(1).unwrap().kills, s.player(3).unwrap().kills), (1, 0));
        // A ring-out is still the last hitter's, whoever did the most.
        let mut s = three(Style::Tiger, Style::Crane, Style::Snake);
        struck(&mut s, 1, 1, 60.);
        press(&mut s, 3, light());
        let e = until_landed(&mut s, 0.3);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Hit) && e.by == 3), "{e:?}");
        put(&mut s, 2, C[0] + 0.7, C[1], 0.);
        run(&mut s, 0.2);
        assert!(told(&s, EventKind::Ko).iter().any(|e| (e.by, e.to, e.ring, e.last) == (3, 2, true, 0)));
        assert_eq!((s.player(1).unwrap().kills, s.player(3).unwrap().kills), (0, 1));
    }

    #[test]
    fn the_toads_iron_body_shrugs_off_ordinary_strikes_from_the_front_but_not_grabs_breakers_or_strikes_from_behind() {
        // Standing its ground with chi: a tenth of the damage, the striker recoils, chi pays for it.
        let mut s = arena(Style::Crane, Style::Toad, 0.15);
        assert!(s.kungfu_view().unwrap().fighter(2).unwrap().iron);
        let chi = f(&s, 2).chi;
        press(&mut s, 1, kick());
        for _ in 0..30 {
            s.tick(DT);
            if !told(&s, EventKind::Absorbed).is_empty() {
                break;
            }
        }
        let d = Move::CraneKick1.data();
        assert!(told(&s, EventKind::Absorbed).iter().any(|e| e.by == 1 && e.to == 2));
        assert!((hp(&s, 2) - (100. - d.damage * IRON_SHARE)).abs() < 0.01, "{}", hp(&s, 2));
        assert_eq!((f(&s, 1).mv, f(&s, 1).len, f(&s, 2).mv), (Move::HitLight, IRON_RECOIL, Move::Idle), "the striker recoils, the Toad does not");
        assert!(f(&s, 2).chi < chi);
        // A grab gets through.
        let mut s = arena(Style::Tiger, Style::Toad, 0.1);
        press(&mut s, 1, Press { grab: true, ..Default::default() });
        let e = until_landed(&mut s, 0.3);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Grabbed)), "{e:?}");
        // So does a guard breaker.
        let mut s = arena(Style::Tiger, Style::Toad, 0.15);
        press(&mut s, 1, heavy());
        let e = until_landed(&mut s, 0.5);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Hit) && e.to == 2) && !e.iter().any(|e| e.kind == Some(EventKind::Absorbed)), "{e:?}");
        // And a strike from behind.
        let mut s = arena(Style::Crane, Style::Toad, 0.15);
        s.player_mut(2).unwrap().yaw = 0.;
        press(&mut s, 1, kick());
        let e = until_landed(&mut s, 0.4);
        assert!(e.iter().any(|e| e.kind == Some(EventKind::Hit) && e.to == 2), "{e:?}");
        // Off in its own moves, and with no chi.
        let mut s = arena(Style::Crane, Style::Toad, 0.15);
        press(&mut s, 2, heavy());
        run(&mut s, 0.05);
        assert!(!s.kungfu_view().unwrap().fighter(2).unwrap().iron);
        // (A Toad standing still gathers chi again: held at none here.)
        let mut s = arena(Style::Crane, Style::Toad, 0.15);
        press(&mut s, 1, kick());
        let mut hit = false;
        for _ in 0..20 {
            s.kungfu.as_mut().unwrap().fighters[1].chi = 0.;
            s.tick(DT);
            hit |= told(&s, EventKind::Hit).iter().any(|e| e.to == 2);
        }
        assert!(hit && told(&s, EventKind::Absorbed).is_empty());
        let mut s = arena(Style::Crane, Style::Toad, 0.15);
        s.kungfu.as_mut().unwrap().fighters[1].chi = 0.;
        run(&mut s, 2.);
        assert!(f(&s, 2).chi >= IRON_REGEN * 1.8, "{}", f(&s, 2).chi);
        // Its strikes are the strongest, and it walks the slowest.
        assert!(Move::ToadHeavy2.data().damage > Move::TigerHeavy2.data().damage);
        let mut s = arena(Style::Toad, Style::Crane, 0.4);
        let (k, crane) = (s.kungfu_pace(1), s.kungfu_pace(2));
        assert!(k < crane, "{k} {crane}");
        let _ = s.tick(DT);
    }

    #[test]
    fn the_view_says_what_each_fighter_does_and_reads_back_from_json() {
        let mut s = arena(Style::Drunken, Style::Monkey, 0.15);
        press(&mut s, 1, light());
        run(&mut s, 0.1);
        let v = s.kungfu_view().unwrap();
        let a = v.fighter(1).unwrap();
        assert_eq!((a.style(), a.mv()), (Style::Drunken, Move::DrunkenLight1));
        assert!(a.scale >= DRUNK[0] && a.scale <= DRUNK[1] && (a.len - Move::DrunkenLight1.data().total() * a.scale).abs() < 1e-4);
        assert!((a.t(v.elapsed) - 0.1).abs() < 2. * DT);
        assert!(!free(a, v.elapsed) && pace(a) == 0.);
        assert_eq!((v.goal, v.fighters.len()), (KOS, 2), "the benched are not shown");
        let text = serde_json::to_string(&v).unwrap();
        let back: View = serde_json::from_str(&text).unwrap();
        assert_eq!(back, v);
        let tagged: View = serde_json::from_str(&format!("{{\"type\":\"kungfu\",{}", &text[1..])).unwrap();
        assert_eq!(tagged, v);
        // Events last EVENT_LIFE, numbered on.
        run(&mut s, 0.3);
        let ids: Vec<u32> = s.kungfu_view().unwrap().events.iter().map(|e| e.id).collect();
        assert!(!ids.is_empty() && ids.windows(2).all(|w| w[0] < w[1]));
        run(&mut s, EVENT_LIFE + 0.1);
        assert!(s.kungfu_view().unwrap().events.iter().all(|e| !ids.contains(&e.id)));
    }

    #[test]
    fn the_fight_is_deterministic() {
        let state = |s: &Feast| (s.players.iter().map(|p| (p.p, p.hp, p.kills, p.deaths)).collect::<Vec<_>>(), serde_json::to_string(&s.kungfu_view()).unwrap());
        let mut a = bout();
        let mut b = bout();
        for _ in 0..(30. / DT) as usize {
            a.tick(DT);
            b.tick(DT);
        }
        assert_eq!(state(&a), state(&b));
        assert!(a.players.iter().map(|p| p.kills).sum::<u32>() > 0, "they fought");
        // Another seed, another fight.
        let mut c = bout();
        c.card_seed = 99;
        c.reset();
        let mut d = bout();
        d.reset();
        for _ in 0..(30. / DT) as usize {
            c.tick(DT);
            d.tick(DT);
        }
        assert_ne!(state(&c).1, state(&d).1);
    }

    #[test]
    fn a_bot_match_runs_to_a_finish_with_every_bot_fighting_in_its_style_and_without_all_piling_on_one() {
        let mut s = bout();
        let styles: Vec<Style> = s.kungfu.as_ref().unwrap().fighters.iter().map(|f| f.style).collect();
        let mut distinct = styles.clone();
        distinct.dedup();
        assert_eq!(distinct.len(), styles.len(), "a style each: {styles:?}");
        let (mut piled, mut samples, mut blocks, mut parries, mut specials) = (0, 0, 0, 0, 0);
        let mut seen = 0;
        while s.stage != Stage::RoundOver && s.elapsed < TIME + 5. {
            s.tick(DT);
            let dojo = s.kungfu.as_ref().unwrap();
            for e in dojo.events.iter().filter(|e| e.id > seen) {
                match e.kind {
                    Some(EventKind::Blocked) => blocks += 1,
                    Some(EventKind::Parried) => parries += 1,
                    Some(EventKind::Special) => specials += 1,
                    _ => {}
                }
            }
            seen = dojo.events.iter().map(|e| e.id).max().unwrap_or(seen);
            let on: Vec<u32> = dojo.fighters.iter().zip(&s.players).filter(|(_, p)| p.alive).map(|(f, _)| f.brain.target).collect();
            samples += 1;
            if (1..=5).any(|id| on.iter().filter(|t| **t == id).count() >= 4) {
                piled += 1;
            }
        }
        assert_eq!(s.stage, Stage::RoundOver, "{}", s.alert);
        let kos: u32 = s.players.iter().map(|p| p.kills).sum();
        assert!(kos >= KOS, "{kos} KOs: {}", s.alert);
        assert!(blocks + parries > 0 && specials > 0, "blocks {blocks} parries {parries} specials {specials}");
        assert!((piled as f32) < samples as f32 * 0.05, "four on one {piled} of {samples}");
        // The next round follows with the styles moved on.
        s.reset();
        let next: Vec<Style> = s.kungfu.as_ref().unwrap().fighters.iter().map(|f| f.style).collect();
        assert_ne!(next, styles);
    }
}

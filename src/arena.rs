// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Countertop survival. Simulation uses simple colliders; rendering never participates in hit tests.
use crate::controls::Action;
use crate::mat;
use bevy::{
    asset::RenderAssetUsages,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::{CursorGrabMode, CursorOptions},
};
use kitchen_sim::actor::{self, Actor};
use kitchen_sim::feast::{self, build::Pose, Feast, Player, Stage};
use kitchen_sim::map::Map;
use kitchen_sim::net::button;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::f32::consts::{PI, TAU};
pub mod online;
/// What this player's screen shows of the first person, for a replay's recorded view (replay/pov.rs).
pub(crate) mod shown;
pub mod splatter;
pub use online::Online;
const FLOOR: f32 = 0.9025;
const EYE: f32 = 0.135;
const WARMUP: f32 = 30.;
/// What a person's hotdog holds before the simulation says otherwise: the syrup rifle in hand and the mustard pistol
/// beside it, each with a full magazine, as every gun mode starts it (kitchen-sim magazine.rs `loadout`).
const OPENING_AMMO: [i32; feast::GUNS] = [0, kitchen_sim::magazine::MAGAZINE[1], 0, 0, 0, 0, kitchen_sim::magazine::MAGAZINE[feast::RIFLE], 0];
const OPENING_OWNED: [bool; feast::GUNS] = [false, true, false, false, false, false, true, false];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(usize)]
pub enum Weapon {
    /// No gun in hand because the hands are full: a loose object being carried, or (Forts) the bacon. It is
    /// not a weapon: nothing selects it, and the gun last in hand comes back when the hands are free.
    Laden = 0,
    Pistol = 1,
    Sniper = 2,
    Flame = 3,
    Bazooka = 4,
    Spray = 5,
    /// The pancake-syrup battle rifle: three-round bursts.
    Rifle = 6,
    /// The mayonnaise squeeze bottle: a stream for as long as it is squeezed (see `squirt`).
    Mayo = 7,
}
impl Weapon {
    const ALL: [Weapon; 8] = [
        Self::Laden,
        Self::Pistol,
        Self::Sniper,
        Self::Flame,
        Self::Bazooka,
        Self::Spray,
        Self::Rifle,
        Self::Mayo,
    ];
    /// Every gun squirts a condiment: mustard, ketchup, hot sauce, a relish-and-mayonnaise glob, syrup.
    pub fn name(self) -> &'static str {
        match self {
            Self::Laden => "HANDS FULL",
            Self::Pistol => "MUSTARD PISTOL",
            Self::Sniper => "KETCHUP SNIPER",
            Self::Flame => "HOT SAUCE THROWER",
            Self::Bazooka => "RELISH BAZOOKA",
            Self::Spray => "BUG SPRAY",
            Self::Rifle => "SYRUP RIFLE",
            Self::Mayo => "MAYO SQUIRTER",
        }
    }
    pub(crate) fn index(i: usize) -> Self {
        Self::ALL[i.min(7)]
    }
    /// Seconds before the trigger can fire again (the simulation's DELAY): for the syrup rifle a whole burst
    /// and the pause after it.
    fn delay(self) -> f32 {
        match self {
            Self::Laden => MELEE_SWING,
            Self::Pistol => feast::DELAY[1],
            Self::Sniper => 1.05,
            Self::Flame => 0.1,
            Self::Bazooka => 1.2,
            Self::Spray => 0.12,
            Self::Rifle => feast::DELAY[6],
            Self::Mayo => feast::DELAY[feast::MAYO],
        }
    }
    /// Which slot of the two a gun goes in: the primary (1) is the bigger gun, the secondary (2) the pistol or
    /// the can (or, holding two big guns, the smaller of them). Lower ranks are more primary.
    fn rank(self) -> usize {
        match self {
            Self::Rifle => 0,
            Self::Sniper => 1,
            Self::Bazooka => 2,
            Self::Flame => 3,
            Self::Mayo => 4,
            Self::Pistol => 5,
            Self::Spray => 6,
            Self::Laden => 7,
        }
    }
    /// First-person recoil: push-back in metres, muzzle climb and roll in radians, shake, camera
    /// pitch in radians, recovery rate per second, and the camera's own shake in radians (a short one,
    /// gone twice as fast as the kick). How hard each gun kicks goes by how heavy it is (kitchen-sim recoil.rs
    /// WEIGHT, the one table the simulation's aim punch goes by too), reckoned from two guns whose kicks are as they
    /// were tuned (`RIFLE_KICK`, `SNIPER_KICK`): a gun that kicks a shot is between the syrup rifle's (weight 1) and the
    /// ketchup sniper's (2) by its weight, past the sniper's beyond it (the relish bazooka, heaviest). The mustard
    /// pistol, light as it is, is a hand cannon (`PISTOL_KICK`, as the simulation's aim punch is set for it: recoil.rs
    /// HAND_CANNON). The spray guns push and shake steadily
    /// while they are held, the hot sauce thrower's as it was tuned (`STREAM`) and the bug spray's and the mayonnaise
    /// bottle's by their weight against it; the stream is refreshed every shot, so it never settles.
    fn recoil(self) -> [f32; 7] {
        let w = kitchen_sim::recoil::WEIGHT[self as usize];
        match self {
            Self::Laden => [0., 0., 0., 0., 0., 10., 0.],
            Self::Rifle => RIFLE_KICK,
            Self::Sniper => SNIPER_KICK,
            Self::Pistol => PISTOL_KICK,
            Self::Flame | Self::Spray | Self::Mayo => {
                let k = w / kitchen_sim::recoil::WEIGHT[Self::Flame as usize];
                // (A steady push of the view too, as heavy as the gun.)
                [STREAM[0] * k, STREAM[1] * k, STREAM[2] * k, STREAM[3] * k, 0.004 * w, STREAM[5], 0.]
            }
            Self::Bazooka => {
                let mut row: [f32; 7] = std::array::from_fn(|i| RIFLE_KICK[i] + (SNIPER_KICK[i] - RIFLE_KICK[i]) * (w - 1.));
                row[5] = row[5].max(3.);
                row
            }
        }
    }
}
/// The syrup rifle's kick, the reference at weight 1: a tight kick for each round of a burst and a quick recovery: each
/// round's kick lands on what is left of the last 0.1 s on (about 1, 0.86 and 0.84 through a burst when each adds 0.75),
/// so every round of the burst is felt, and it has settled by the next, 0.20 s after the last round. Aimed down the
/// sights it is half that (`steadiness`).
const RIFLE_KICK: [f32; 7] = [0.006, 0.096, 0.018, 0.07, 0.0048, 22., 0.];
/// The mustard pistol's: a hand cannon's. The gun is thrown back and its muzzle flips up most of the way a sniper's
/// does, the view is kicked up 0.03 rad (as far again as the simulation's aim punch, recoil.rs HAND_CANNON, sends the
/// next round if it is fired at once) with a short shake, and it comes back down slowly: a third is left when the gun
/// can fire again (0.35 s), next to nothing after 0.8 s (the simulation's SETTLE). A shot fired before it is back adds
/// to what is left (`shoot`), up to PISTOL_STACK times a shot.
const PISTOL_KICK: [f32; 7] = [0.035, 0.42, 0.09, 0.2, 0.03, 3.5, 0.006];
const PISTOL_STACK: f32 = 2.;
/// The ketchup sniper's, the reference at weight 2: heavy blowback. The gun slams back towards the eye and rears up, the
/// view is thrown up three times as far as it once was (0.035 rad) and shakes for a moment, and it has all settled in
/// about 0.4 s.
const SNIPER_KICK: [f32; 7] = [0.062, 0.55, 0.12, 0.35, 0.105, 7.5, 0.02];
/// The hot sauce stream's push and shake, the spray guns' reference (deliberately enormous).
const STREAM: [f32; 7] = [0.036, 0.20, 0.10, 1., 0., 6., 0.];
/// How the kick is shown: it follows the gun's kick level through a spring a little short of critically damped, so the
/// gun comes back and dips just past where it rests before it settles; and the view's kick goes up and to a side drawn
/// afresh each shot, a third as far to the side as up.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring {
    pub shown: f32,
    rate: f32,
    pub side: f32,
    shots: u32,
}
impl Spring {
    /// Stiffness (rad/s) and damping of the spring.
    const STIFF: f32 = 45.;
    const DAMP: f32 = 0.32;
    /// The most of a frame it follows: 32 steps of 0.4 / STIFF seconds. Its steps are stable only when short (past about
    /// 2 / STIFF each overshoots further than the last), and a frame much longer than this one (the first after the tab
    /// was hidden or the window out of sight, which spans all of the time away; a long hitch) grew it without bound, to
    /// infinity and then NaN, which never left it and put NaN in the camera for good: nothing of the scene was drawn
    /// again. By the end of this much the kick has all but settled, and the frames after settle the rest.
    const LONGEST: f32 = 32. * 0.4 / Self::STIFF;
    /// One frame towards the kick level `target`.
    pub fn follow(&mut self, target: f32, dt: f32) {
        let dt = dt.clamp(0., Self::LONGEST);
        let n = ((dt * Self::STIFF / 0.4).ceil() as usize).clamp(1, 32);
        let h = dt / n as f32;
        for _ in 0..n {
            let a = Self::STIFF * Self::STIFF * (target - self.shown) - 2. * Self::DAMP * Self::STIFF * self.rate;
            self.rate += a * h;
            self.shown += self.rate * h;
        }
    }
    /// A shot: the view's kick goes to a side drawn from the shot's number, between a third and all of the way.
    pub fn shot(&mut self) {
        self.shots = self.shots.wrapping_add(1);
        let h = (self.shots.wrapping_mul(0x9E37_79B9) >> 8) as f32 / (1u32 << 24) as f32;
        self.side = if h < 0.5 { -1. } else { 1. } * (0.35 + 1.3 * (h - 0.5).abs());
    }
    /// The view's turn to the side for a kick of the view up by `pitch`.
    pub fn yaw(&self, pitch: f32) -> f32 {
        pitch * 0.33 * self.side
    }
}
/// The two slots of a hotdog's guns, by what it owns: [primary (key 1), secondary (key 2)]. Two guns go in rank
/// order (`Weapon::rank`); a gun on its own goes in the slot of its kind, the pistol and the can in the second.
pub fn slots(owned: &[bool; feast::GUNS]) -> [Option<Weapon>; 2] {
    let mut guns: Vec<Weapon> = (1..feast::GUNS).filter(|w| owned[*w]).map(Weapon::index).collect();
    guns.sort_by_key(|w| w.rank());
    match guns[..] {
        [] => [None, None],
        [one] if one.rank() >= Weapon::Pistol.rank() => [None, Some(one)],
        [one] => [Some(one), None],
        [a, b, ..] => [Some(a), Some(b)],
    }
}
/// Every gun a hotdog holds, in slot order (`Weapon::rank`): the sandbox may hand out more than two.
pub fn held_guns(owned: &[bool]) -> Vec<Weapon> {
    let mut guns: Vec<Weapon> = (1..owned.len()).filter(|w| owned[*w]).map(Weapon::index).filter(|w| *w != Weapon::Laden).collect();
    guns.sort_by_key(|w| w.rank());
    guns.dedup();
    guns
}
/// The gun a step from `weapon` goes to, among the guns held: +1 is the next in slot order (`held_guns`), -1 the one
/// before it, round and round. With two guns either step is the other one, and with one (or none) it is nowhere.
/// This is what Y does (+1) and what the mouse wheel does (down is +1, up -1).
fn next_gun(owned: &[bool; feast::GUNS], weapon: Weapon, step: i32) -> Option<Weapon> {
    let held = held_guns(owned);
    if held.len() > 2 {
        let at = held.iter().position(|w| *w == weapon);
        let to = at.map_or(if step > 0 { 0 } else { held.len() - 1 }, |i| (i as i32 + step).rem_euclid(held.len() as i32) as usize);
        held.get(to).copied()
    } else {
        slots(owned).into_iter().flatten().find(|w| *w != weapon)
    }
}
/// The wheel's travel that makes one step: a line (one notch of a wheel) counts as this many pixels, and a trackpad's
/// pixel deltas gather until they reach it.
const WHEEL_NOTCH: f32 = 50.;
/// Seconds without the wheel turning after which a part of a notch gathered is let go.
const WHEEL_REST: f32 = 0.3;
/// Seconds after a step in which the wheel is not heard: what it turns then is dropped, not saved up. A browser's one
/// notch can be 100 px or 3 lines (and a trackpad's momentum goes on turning for a second): each is one change, and a
/// fling is a change each 0.15 s.
const WHEEL_COOLDOWN: f32 = 0.15;
/// What the mouse wheel has gathered towards the next step (`wheel_step`, in the pixels of a notch), when
/// (`Time<Real>`) it last turned, and when it is next heard (after a step: `WHEEL_COOLDOWN`).
#[derive(Clone, Copy, Default)]
struct Wheel {
    gathered: f32,
    turned: f32,
    heard: f32,
}
/// A frame's turn of the mouse wheel: -1, 0 or +1 gun steps, never more than one. `delta` is the wheel's turn this
/// frame, positive for a step forward (the wheel rolled down), in `Line`s (a wheel's notches, however many a frame
/// holds) or in pixels (`unit_is_pixel`: a trackpad, or a browser's wheel, which may make a notch of 100 px or 3 lines).
/// `gathered` is what has not made a step yet, in pixels: small turns add up to a step (50 px, or a line), it is
/// forgotten when the wheel turns the other way, and a step takes all of it with it, whatever is over the notch, so
/// that one roll is one step and no more are saved up for the frames after it.
fn wheel_step(gathered: &mut f32, delta: f32, unit_is_pixel: bool) -> i32 {
    let pixels = if unit_is_pixel { delta } else { delta * WHEEL_NOTCH };
    if *gathered * pixels < 0. {
        *gathered = 0.;
    }
    *gathered += pixels;
    if gathered.abs() < WHEEL_NOTCH {
        return 0;
    }
    let step = gathered.signum();
    *gathered = 0.;
    step as i32
}
/// What the head-up display says when the trigger or F is pressed behind the forcefield.
const LOCKED: &str = "WEAPONS LOCKED UNTIL THE FIELD DROPS";
/// Seconds one melee strike takes to cool (the simulation's DELAY[0]; its look is whip.rs, a little shorter), and the
/// stim clap in first person.
const MELEE_SWING: f32 = 0.48;
const STIM_CLAP: f32 = 0.55;
/// Where the gun's grip sits in camera space.
const GRIP: Vec3 = Vec3::new(0.045, -0.047, -0.105);

/// Gun kick for a recoil level in 0..1: offset and rotation of the weapon in the hands, the
/// camera's pitch, and the camera's shake (a turn about its up and its forward axes, radians).
/// Purely visual: the aim the simulation uses never moves.
fn recoil_pose(weapon: Weapon, kick: f32, now: f32) -> (Vec3, Quat, f32, Vec2) {
    let [back, climb, roll, shake, pitch, _, quake] = weapon.recoil();
    let jitter = Vec3::new((now * 83.).sin(), (now * 67. + 1.).sin(), (now * 59. + 2.).sin())
        * shake
        * kick;
    // (The camera's shake goes with the square of the kick, so it is over in half the time.)
    let quake = Vec2::new((now * 71. + 0.5).sin(), (now * 97. + 3.).sin()) * quake * kick * kick.min(1.);
    (
        Vec3::new(jitter.x * 0.007, kick * back * 0.45 + jitter.y * 0.007, kick * back + jitter.z * 0.004),
        Quat::from_rotation_x(kick * climb + jitter.y * 0.10)
            * Quat::from_rotation_z(kick * roll + jitter.x * 0.12),
        kick * pitch + jitter.z * 0.012,
        quake,
    )
}
/// The share of a gun's kick that lands, by how the hotdog holds it: three quarters crouching, half prone, 0.4 on
/// stim, and with the syrup rifle aimed down its sights (`aim`, 0 at the hip to 1 aimed) AIMED_KICK of that.
fn steadiness(prone: bool, crouch: bool, stim: bool, aim: f32) -> f32 {
    let stance = if prone { 0.5 } else if crouch { 0.75 } else { 1. };
    stance * if stim { 0.4 } else { 1. } * (1. + (AIMED_KICK - 1.) * aim.clamp(0., 1.))
}
/// What is left of the syrup rifle's kick aimed down its sights.
const AIMED_KICK: f32 = 0.5;
/// Seconds the syrup rifle takes to come up from the hip to the eye, and as long to go back down.
const AIM_TIME: f32 = 0.15;
/// Where the syrup rifle's first-person model is held aimed (its root, as GRIP is at the hip): centred and raised so
/// that the eye looks down the optic's axis (`weapons::RIFLE_SIGHT`), its eyepiece `scope::eye_relief` in front of
/// the eye, near enough that the ring fills the middle of the view (scope.rs). Turned no more than at the hip: the
/// axis runs along the bore.
fn aim_grip() -> Vec3 {
    let view = crate::weapons::in_view(Weapon::Rifle as usize);
    Vec3::NEG_Z * crate::scope::eye_relief() - view.transform_point(crate::weapons::RIFLE_SIGHT)
}
/// The syrup rifle's first-person root, at aim `a` (eased: 0 at the hip, 1 aimed down the sights).
fn rifle_hold(a: f32) -> Vec3 {
    GRIP.lerp(aim_grip(), a.clamp(0., 1.))
}
#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Dormant,
    Knife,
    Peeling,
    Warmup,
    Siege,
    Famine,
    Over,
    /// Forts (see forts.rs): the build phase behind the forcefield, the fight over the bacon, and the pause
    /// between two rounds, which the simulation ends by itself.
    Build,
    Fight,
    Rest,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct EnemyState {
    pub id: u32,
    pub p: [f32; 3],
    pub yaw: f32,
    pub hp: f32,
    pub born: f32,
    /// Seconds until the zombie has risen out of its puddle.
    #[serde(default)]
    pub forming: f32,
}
struct Enemy {
    state: EnemyState,
    entity: Entity,
}
/// A pickup's spawn point as the simulation or the server has it: what it holds (a gun, or 0 a box of
/// ammunition), when it is there again and how long it takes to come back.
#[derive(Clone, Serialize, Deserialize)]
pub struct PickupState {
    pub id: usize,
    pub p: [f32; 3],
    pub weapon: usize,
    pub ammo: i32,
    pub ready: f32,
    #[serde(default)]
    pub respawn: f32,
}
struct Pickup {
    state: PickupState,
    entity: Entity,
    /// Its pad on the counter, with the ring that counts it back (`spawns`).
    pad: crate::spawns::Pad,
}
struct Effect {
    entity: Entity,
    left: f32,
    velocity: Vec3,
    /// Droplets fall, land and flatten; beams just fade.
    drop: bool,
}
/// The visible body of one condiment round (splatter.rs).
use splatter::Blob;
struct Swarm {
    root: Entity,
    flies: Vec<Entity>,
    grubs: Vec<Entity>,
}
#[derive(Component)]
pub struct BattleProp;
#[derive(Component)]
pub(crate) struct Hud(pub(crate) u8);
#[derive(Component)]
struct ScopeMask;
#[derive(Component)]
pub struct PackageFilm;
#[derive(Component)]
pub struct PackageLabel;
#[derive(Component)]
pub struct CutsceneKnife;
#[derive(Component)]
pub(crate) struct MicrowaveDigit(pub usize, pub usize);
#[derive(Component)]
pub(crate) struct Smoke(pub usize);
#[derive(Component)]
pub(crate) struct MicrowaveGlow;
#[derive(Resource, Clone)]
struct Visuals {
    sphere: Handle<Mesh>,
    capsule: Handle<Mesh>,
    cube: Handle<Mesh>,
    red: Handle<StandardMaterial>,
    green: Handle<StandardMaterial>,
    #[allow(dead_code)]
    orange: Handle<StandardMaterial>,
    #[allow(dead_code)]
    fur: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    /// Condiment by weapon: mustard, ketchup, hot sauce, relish, bug spray mist, pancake syrup, mayonnaise (0 is
    /// unused).
    sauces: [Handle<StandardMaterial>; feast::GUNS],
    haze: Handle<StandardMaterial>,
    pew: Handle<AudioSource>,
    /// The mustard pistol's squirt.
    squirt: Handle<AudioSource>,
    bite: Handle<AudioSource>,
    ding: Handle<AudioSource>,
}
#[derive(Resource)]
pub struct Game {
    phase: Phase,
    pub elapsed: f32,
    phase_time: f32,
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Network play: the bodies as the server last reported them, with this player's own predicted among them, and
    /// the prediction, the commands and what is drawn in the past (online.rs).
    ghosts: actor::World,
    pub online: Online,
    pub infected: bool,
    pub score: u32,
    pub kills: u32,
    pub weapon: Weapon,
    pub ammo: [i32; feast::GUNS],
    pub owned: [bool; feast::GUNS],
    /// The gun most recently in hand: Y swaps to the other one, and a third gun replaces it.
    gun: Weapon,
    /// The gun in hand before that one, for the last-weapon key (controls.rs).
    last_gun: Option<Weapon>,
    grab: Option<(Weapon, f32)>,
    cooldown: f32,
    /// The syrup burst under way on this screen: rounds still to come and seconds to the next. Each kicks the gun
    /// again before the last kick has settled, so a burst climbs (the simulation fires the rounds themselves).
    volley: u32,
    volley_wait: f32,
    /// Recoil level, the melee strike's swing (whip.rs), and seconds into the stim clap.
    kick: f32,
    pub whip: crate::whip::Swing,
    clap: f32,
    stimmed: bool,
    pub prone: bool,
    /// Crouching: held with Ctrl (or C).
    pub crouch: bool,
    eye: f32,
    pub pending_stim: bool,
    /// The right mouse button's aim, held or toggled (settings.rs): the ketchup sniper's scope, or the syrup rifle's
    /// optic raised to the eye. Never with the hands full, at the card table, or dead.
    pub scoped: bool,
    /// The aim is up because the right button is held (not toggled): it comes down when the button is let go.
    aim_held: bool,
    /// The syrup rifle coming up to the eye: 0 at the hip to 1 aimed down its sights, AIM_TIME seconds either way (and
    /// eased by `smooth` where it is used). It is 0 with any other gun in hand.
    pub aim: f32,
    /// The mouse wheel's turn gathered towards a change of gun (`wheel_step`).
    wheel: Wheel,
    hit: f32,
    message: String,
    message_time: f32,
    /// A note the player is shown whatever the hints (a mode the map refuses: [`fit_rules`]), for `notice_time` seconds,
    /// and the last such note said (each is said once).
    notice: String,
    notice_time: f32,
    refused: Option<String>,
    enemies: Vec<Enemy>,
    pickups: Vec<Pickup>,
    effects: Vec<Effect>,
    camera: Option<Entity>,
    knife: Option<Entity>,
    started: bool,
    pub network: bool,
    pub network_id: u32,
    pub online_count: usize,
    network_round: u32,
    pub pending_shot: bool,
    pub pending_pickup: Option<usize>,
    /// The first-person sway of what this player carries (sideways behind a turn, the stride's depth and its phase), and
    /// the yaw and place it was worked out from last frame.
    pub sway: Vec3,
    sway_yaw: f32,
    sway_at: Vec3,
    /// The fists that hold a napkin in first person (`viewmodel::carrying_hands`), made the first time, and
    /// how far they are moved from their place this frame.
    carry_hands: Option<Entity>,
    carry_shift: Vec3,
    pub pending_melee: bool,
    pub pending_eat: bool,
    /// Carrying (Q picks up or puts down, the trigger throws) and, in a Forts build phase, R for ready.
    pub pending_grab: bool,
    /// How this player holds what it carries (the wheel turns it, X tips it): sent in every command online.
    pub pose: Pose,
    pub pending_throw: bool,
    pub pending_ready: Option<bool>,
    /// Whether this player's hands were full on the last frame, and the local round the client has seen.
    laden: bool,
    seen_round: u32,
    /// Forts: what the simulation (or the server) says of the mode; `None` in a siege.
    pub fort: Option<kitchen_sim::forts::FortState>,
    /// Kung Fu: the client's copy of the fight (interface.md 4b): offline the round's own (`Feast::kungfu_view`) every
    /// tick, online the server's `{type:'kungfu'}` (network.rs). `None` in any other mode, which is how the client tells.
    pub kungfu: Option<kitchen_sim::kungfu::View>,
    /// Titan: the client's copy of the titan (titan/interface.md §4): offline the round's own (`Feast::titan_view`) every
    /// tick, online the server's `{type:'titan'}` (network.rs, `titan::hear`). `None` in any other mode.
    pub titan: Option<kitchen_sim::titan::View>,
    /// Kung Fu: the style this player asks for (`kungfu::Style` as its number; sent as the command's weapon, style + 1,
    /// until the simulation has changed to it), and the levels held this frame that every command carries (the heavy
    /// strike's SCOPE, the block's GRAB, the special's PRONE).
    pub fu_style: Option<u8>,
    pub fu_hold: u16,
    /// Forts: seconds the build phase lasts instead of what the map says (`?build=` or `--build`); 0 leaves it.
    build: f32,
    pub direction: Vec3,
    pub bite_time: f32,
    frame: u32,
    viewmodels: Vec<(Weapon, Entity)>,
    /// How far the first-person gun is lowered, 0 to 1: it is, behind the forcefield.
    lower: f32,
    stim_arm: Option<(Entity, Entity)>,
    bolts: Vec<Entity>,
    blobs: HashMap<u32, Blob>,
    swarms: HashMap<u32, Swarm>,
    last_splat: u32,
    pub feast: Feast,
    pub roster: Vec<Player>,
    actors: HashMap<u32, Entity>,
    last_feast_shot: u32,
    pub demo: bool,
    pub alert: String,
    /// Single player: the rules the page's address asks for (Free For All, the dev sandbox: kitchen_sim::rules), which
    /// every round is played by. Online the server's come with its frames, into `feast.rules`.
    pub rules: kitchen_sim::rules::Rules,
    /// The numbers the guns and the hotdogs go by (kitchen_sim::tune), which `feast.tune` follows: single player's
    /// own, changed by the sandbox's panel (tune.rs); online the server's, and whether it takes changes (`tuning`).
    pub tuning: kitchen_sim::tune::Tuning,
    pub tune_editable: bool,
    /// This player's reserves and the reload under way, as shown (reload.rs).
    pub mag: crate::reload::State,
    /// The kick as it is shown, through a spring, and the side the view kicks to (`Spring`).
    pub spring: Spring,
}
impl Game {
    /// The game's camera, which the first-person view hangs from.
    pub fn camera(&self) -> Option<Entity> {
        self.camera
    }
    pub fn active(&self) -> bool {
        self.phase != Phase::Dormant
    }
    pub fn playing(&self) -> bool {
        matches!(
            self.phase,
            Phase::Warmup | Phase::Siege | Phase::Famine | Phase::Over | Phase::Build | Phase::Fight | Phase::Rest
        )
    }
    /// The hotdog this player plays.
    pub fn me(&self) -> u32 {
        if self.network { self.network_id } else { 1 }
    }
    /// The number of the frame `tick` last ran.
    pub fn frame(&self) -> u32 {
        self.frame
    }
    /// Turns the crosshair into a red cross for `seconds` (a kill: damage.rs).
    pub fn mark_hit(&mut self, seconds: f32) {
        self.hit = self.hit.max(seconds);
    }
    /// Whether the crosshair is a red cross now (for the tests).
    #[allow(dead_code)]
    pub fn hit_marked(&self) -> bool {
        self.hit > 0.
    }
    /// The rounds in the air as they are drawn, by id.
    pub(crate) fn blobs(&self) -> &HashMap<u32, splatter::Blob> {
        &self.blobs
    }
    /// The first-person models of the guns, each with its root (hung from the camera).
    pub(crate) fn viewmodels(&self) -> &[(Weapon, Entity)] {
        &self.viewmodels
    }
    /// Where the loose object this player carries would land if Q put it down now (`Feast::placement`: where, turned
    /// how, what it snaps to, whether it is locked, what a napkin drapes over), or None when it carries nothing.
    /// Offline the round itself is asked. Online the client's copy of the round is asked about this player's hotdog
    /// as the client has it: where its prediction has it (online.rs), facing the way it looks, walking at `pace` (which
    /// the server keeps and does not send: preview.rs keeps it as the simulation does) and holding what the server's
    /// frames say it holds. The copy is left as it was.
    pub fn placement(&mut self, pace: [f32; 2]) -> Option<feast::Placement> {
        if !self.network {
            return self.feast.placement(1);
        }
        let me = self.network_id;
        let mut player = Player { pace, yaw: self.yaw, p: self.position.to_array(), pose: self.pose, ..self.roster.iter().find(|p| p.id == me)?.clone() };
        if let Some(body) = self.ghosts.get(me) {
            (player.p, player.v, player.grounded) = (body.p.map(|v| v as f32), body.v.map(|v| v as f32), body.grounded);
        }
        let slot = self.feast.players.iter().position(|p| p.id == me);
        let saved = match slot {
            Some(i) => Some(std::mem::replace(&mut self.feast.players[i], player)),
            None => {
                self.feast.players.push(player);
                None
            }
        };
        let spot = self.feast.placement(me);
        match (slot, saved) {
            (Some(i), Some(was)) => self.feast.players[i] = was,
            _ => {
                self.feast.players.pop();
            }
        }
        spot
    }
    /// The bodies a drape would have to move out of its wall's way (`Feast::refusal`): the round's own offline, and
    /// online the client's copy of them (its own predicted, the others where the server last had them).
    pub fn bodies(&self) -> &actor::World {
        if self.network { &self.ghosts } else { &self.feast.bodies }
    }
    /// Whether the round is Kung Fu's: hands and feet, no guns.
    pub fn kung_fu(&self) -> bool {
        self.kungfu.is_some()
    }
    /// Kung Fu: a fighter as the client last heard of it.
    pub fn fighter(&self, id: u32) -> Option<&kitchen_sim::kungfu::FighterView> {
        self.kungfu.as_ref()?.fighter(id)
    }
    /// Whether the round is played by the Forts rules: a Forts map's (not in Free For All, which leaves them out).
    pub fn forts(&self) -> bool {
        self.feast.fort.is_some()
    }
    /// Forts: while the forcefield stands nobody can fire or strike (the simulation refuses both); a carried
    /// object can still be thrown.
    pub fn locked(&self) -> bool {
        self.phase == Phase::Build
    }
    /// Whether rounds hurt hotdogs now: in Free For All (and the siege's free for all after its waves) and in a Forts
    /// fight, as the simulation's `Projectile::hostile` has it (and a Forts team's rounds still pass its own by).
    pub(crate) fn hostile(&self) -> bool {
        matches!(self.phase, Phase::Famine | Phase::Fight)
    }
    /// The syrup rifle's burst as its optic shows it (scope.rs): how many of a burst's rounds are ready to go, and the
    /// burst's size. While a burst fires, the rounds still to come; after it none until the rifle may fire again; then
    /// all of them, or as many as the magazine holds.
    pub(crate) fn burst(&self) -> (u32, u32) {
        let size = self.feast.tune.burst.max(1);
        let ready = if self.volley > 0 { self.volley } else if self.cooldown > 0. { 0 } else { size };
        (ready.min(self.ammo[Weapon::Rifle as usize].max(0) as u32), size)
    }
    /// For the checks: every pickup's spawn point as it stands (what it holds, when it is back, how long it takes).
    pub fn spawns(&self) -> Vec<&PickupState> {
        self.pickups.iter().map(|p| &p.state).collect()
    }
    /// The siege's wave: the zombie cats on the counter and those still to come out of the microwave (hud.rs).
    pub fn cats_left(&self) -> usize {
        self.enemies.len() + self.feast.pending() as usize
    }
    /// The phase a round opens with: the warm-up, or a Forts round's build phase.
    fn opening(&self) -> Phase {
        if self.forts() { Phase::Build } else { Phase::Warmup }
    }
    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            Phase::Dormant => "Cinematic",
            Phase::Knife => "Knife",
            Phase::Peeling => "Peeling",
            Phase::Warmup => "Warmup",
            Phase::Siege => "Siege",
            Phase::Famine => "FreeForAll",
            Phase::Over => "RoundOver",
            Phase::Build => "Build",
            Phase::Fight => "Fight",
            Phase::Rest => "Rest",
        }
    }
    pub fn fear(&self) -> f32 {
        match self.phase {
            Phase::Knife => 0.2 + self.phase_time / 20.,
            Phase::Peeling => 0.45 + (self.phase_time / 7.5) * 0.55,
            _ => 0.2,
        }
    }
    pub fn cutscene_time(&self) -> Option<f32> {
        match self.phase {
            Phase::Knife => Some(self.phase_time),
            Phase::Peeling => Some(4.5 + self.phase_time),
            _ => None,
        }
    }
    /// Where this player starts on the current map, and the way it faces.
    fn start(&self) -> (Vec3, f32) {
        // Forts: at the start its team gave it, which the roster knows; Free For All: at the spot the round's opening
        // gave it (kitchen_sim::spawns).
        if self.forts() || self.feast.rules.ffa {
            let from = if self.network { &self.roster } else { &self.feast.players };
            if let Some(p) = from.iter().find(|p| p.id == self.me()) {
                return (Vec3::from_array(p.p), p.yaw);
            }
        }
        let slot = if self.network { self.network_id.max(1) as usize - 1 } else { 0 };
        self.feast.level.start(slot % 5)
    }
    pub fn begin(&mut self) {
        self.feast = Feast::with_map(self.feast.level.map.clone(), self.demo);
        // Forts: the demo builds for a short while, and `?build=` (or `--build`) sets the time for anyone.
        if self.build > 0. || (self.demo && self.forts()) {
            let seconds = if self.build > 0. { self.build } else { 25. };
            self.feast.set_build(seconds);
        }
        // Single player: Free For All, the sandbox or Fully Loaded, as the address asks (online the server's frames
        // say). A card table is shuffled afresh every game (online the server shuffles).
        if !self.network {
            self.feast.card_seed = crate::cards::fresh_seed();
            // A mode this map refuses is not played on it: the map's own mode is, and the player is told, once.
            let (rules, refused) = fit_rules(&self.rules, &self.feast.level.map);
            if let Some(why) = refused.filter(|why| self.refused.as_ref() != Some(why)) {
                warn!("{why}");
                (self.notice, self.notice_time, self.refused) = (why.clone(), NOTICE, Some(why));
            }
            self.rules = rules;
            self.feast.set_rules(self.rules.clone());
        }
        self.feast.tune = self.tuning.clone();
        self.fort = self.feast.fort_state();
        // Kung Fu is known from the start of the round (online the server's word puts it there, network.rs).
        if !self.network {
            self.kungfu = self.feast.kungfu_view();
            self.titan = self.feast.titan_view();
        }
        self.fu_style = None;
        self.seen_round = self.feast.round;
        self.laden = false;
        self.roster = self.feast.players.clone();
        self.online_count = self.roster.len();
        self.last_feast_shot = 0;
        self.last_splat = 0;
        self.gun = Weapon::index(self.feast.players.first().map_or(1, |p| p.gun).max(1));
        self.grab = None;
        self.kick = 0.;
        self.whip = crate::whip::Swing::rest();
        self.clap = STIM_CLAP;
        self.stimmed = false;
        self.prone = false;
        self.crouch = false;
        self.alert = self.feast.alert.clone();
        self.phase = self.opening();
        self.phase_time = 0.;
        self.elapsed = 0.;
        (self.position, self.yaw) = self.start();
        self.pitch = 0.;
        self.started = false;
        self.ghosts.actors.clear();
        self.online.reset();
        self.infected = false;
        self.score = 0;
        self.kills = 0;
        // What the simulation hands this player: the syrup rifle in hand and the mustard pistol beside it, in
        // every mode (the sandbox's loadout aside).
        let first = self.feast.players.iter().find(|p| p.id == self.me()).or(self.feast.players.first());
        self.weapon = Weapon::index(first.map_or(feast::RIFLE, |p| p.weapon).max(1));
        self.ammo = first.map_or(OPENING_AMMO, |p| p.ammo);
        self.owned = first.map_or(OPENING_OWNED, |p| p.owned);
        self.cooldown = 0.;
        self.scoped = false;
        self.aim = 0.;
        self.hit = 0.;
        self.bite_time = 0.;
        for pickup in &mut self.pickups {
            pickup.state.ready = 0.;
        }
    }
    /// The note on the screen now, whatever the hints (see `notice`).
    pub fn notice(&self) -> Option<&str> {
        (self.notice_time > 0.).then_some(self.notice.as_str())
    }
    fn notify(&mut self, s: impl Into<String>) {
        self.message = s.into();
        self.message_time = 2.5;
    }

    pub fn set_network_state(
        &mut self,
        elapsed: f32,
        infected: bool,
        score: u32,
        kills: u32,
        ammo: [i32; feast::GUNS],
        owned: [bool; feast::GUNS],
    ) {
        self.network = true;
        self.elapsed = elapsed;
        self.infected = infected;
        self.score = score;
        self.kills = kills;
        self.ammo = ammo;
        self.owned = owned;
        self.phase = if elapsed >= 210. {
            Phase::Over
        } else if elapsed >= WARMUP {
            Phase::Siege
        } else {
            Phase::Warmup
        };
    }
}

fn child(
    commands: &mut Commands,
    parent: Entity,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    at: Vec3,
    scale: Vec3,
) -> Entity {
    let e = commands
        .spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at).with_scale(scale),
        ))
        .id();
    commands.entity(parent).add_child(e);
    e
}
fn panel_text(
    commands: &mut Commands,
    kind: u8,
    text: &str,
    left: Val,
    top: Val,
    bottom: Val,
    size: f32,
) {
    commands.spawn((
        Hud(kind),
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgb(0.78, 0.94, 0.89)),
        Node {
            position_type: PositionType::Absolute,
            left,
            top,
            bottom,
            ..default()
        },
        Visibility::Hidden,
    ));
}
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    server: Res<AssetServer>,
    options: Res<crate::runtime::Options>,
    start: Res<crate::level::StartMap>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
) {
    let steel = mats.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.53, 0.56),
        metallic: 0.8,
        perceptual_roughness: 0.24,
        ..default()
    });
    let dark = mat(&mut mats, Color::srgb(0.018, 0.028, 0.026), 0.42);
    let red = mats.add(StandardMaterial {
        base_color: Color::srgb(1., 0.015, 0.03),
        emissive: LinearRgba::rgb(8., 0.02, 0.03),
        unlit: true,
        ..default()
    });
    let green = mats.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.8, 0.14),
        emissive: LinearRgba::rgb(0.03, 0.2, 0.01),
        ..default()
    });
    let orange = mats.add(StandardMaterial {
        base_color: Color::srgb(1., 0.3, 0.015),
        emissive: LinearRgba::rgb(4., 0.7, 0.01),
        unlit: true,
        ..default()
    });
    let white = mat(&mut mats, Color::srgb(0.94, 0.91, 0.75), 0.5);
    let fur = mat(&mut mats, Color::srgb(0.18, 0.22, 0.16), 0.88);
    let sauces = [
        white.clone(),
        mat(&mut mats, Color::srgb(0.97, 0.72, 0.05), 0.3),
        mat(&mut mats, Color::srgb(0.78, 0.05, 0.03), 0.28),
        mats.add(StandardMaterial {
            base_color: Color::srgb(1., 0.28, 0.02),
            emissive: LinearRgba::rgb(1.6, 0.22, 0.01),
            perceptual_roughness: 0.25,
            ..default()
        }),
        mat(&mut mats, Color::srgb(0.33, 0.55, 0.12), 0.55),
        mats.add(StandardMaterial {
            base_color: Color::srgba(0.78, 0.93, 0.97, 0.4),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        // Pancake syrup: deep amber, glossier than any other sauce, and a little see-through, with a
        // warm glow where the light comes through it.
        mats.add(StandardMaterial {
            base_color: Color::srgba(0.54, 0.22, 0.02, 0.86),
            emissive: LinearRgba::rgb(0.10, 0.032, 0.),
            perceptual_roughness: 0.05,
            reflectance: 0.65,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        // Mayonnaise: thick, creamy and glossy.
        mat(&mut mats, Color::srgb(0.96, 0.93, 0.80), 0.28),
    ];
    // The patch of counter a swarm has claimed.
    let haze = mats.add(StandardMaterial {
        base_color: Color::srgba(0.2, 0.26, 0.04, 0.3),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let visuals = Visuals {
        sphere: meshes.add(Sphere::new(1.).mesh().uv(12, 8)),
        capsule: meshes.add(Capsule3d::new(1., 1.).mesh().longitudes(12).latitudes(6)),
        cube: meshes.add(Cuboid::default()),
        red,
        green,
        orange,
        fur,
        dark: dark.clone(),
        white,
        steel: steel.clone(),
        sauces,
        haze,
        pew: server.load("audio/pew.wav"),
        squirt: server.load("audio/squirt.wav"),
        bite: server.load("audio/bite.wav"),
        ding: server.load("audio/ding.wav"),
    };
    // Knife exists before Maya reaches for it; it is later used to split the film.
    let knife = commands
        .spawn((
            CutsceneKnife,
            Name::new("Chef knife"),
            Transform::from_translation(crate::maya::choreography::KNIFE_REST),
            Visibility::Hidden,
        ))
        .id();
    child(
        &mut commands,
        knife,
        &visuals.cube,
        &steel,
        Vec3::new(0., 0., -0.055),
        Vec3::new(0.004, 0.035, 0.16),
    );
    child(
        &mut commands,
        knife,
        &visuals.cube,
        &dark,
        Vec3::new(0., 0., 0.065),
        Vec3::new(0.012, 0.023, 0.08),
    );
    for z in [0.04, 0.085] {
        child(
            &mut commands,
            knife,
            &visuals.sphere,
            &steel,
            Vec3::new(0.007, 0., z),
            Vec3::splat(0.003),
        );
    }
    // The round's own lines are two: the cut scene's line (and, with the hints on, what just happened) and the
    // crosshair. Everything else in first person is the head-up display's, in icons (hud.rs).
    panel_text(
        &mut commands,
        2,
        "",
        Val::Percent(39.),
        Val::Percent(66.),
        Val::Auto,
        18.,
    );
    panel_text(
        &mut commands,
        3,
        "+",
        Val::Percent(49.6),
        Val::Percent(48.),
        Val::Auto,
        23.,
    );
    // Build the scope in memory so both builds are independent of an external UI image.
    let scope = images.add(scope_image());
    commands
        .spawn((
            ScopeMask,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                top: Val::Px(0.),
                width: Val::Percent(100.),
                height: Val::Percent(100.),
                justify_content: JustifyContent::Center,
                overflow: Overflow::clip(),
                ..default()
            },
            ZIndex(-1),
            Visibility::Hidden,
        ))
        .with_children(|parent| {
            parent.spawn((
                Node {
                    height: Val::Percent(100.),
                    flex_grow: 1.,
                    ..default()
                },
                BackgroundColor(Color::BLACK),
            ));
            parent.spawn((
                ImageNode::new(scope),
                Node {
                    height: Val::Percent(100.),
                    aspect_ratio: Some(1.),
                    flex_shrink: 0.,
                    ..default()
                },
            ));
            parent.spawn((
                Node {
                    height: Val::Percent(100.),
                    flex_grow: 1.,
                    ..default()
                },
                BackgroundColor(Color::BLACK),
            ));
        });
    let models = crate::player_model::PlayerModels::new(
        &mut meshes,
        &mut mats,
        &server,
        adapter.is_none_or(|a| format!("{:?}", a.device_type) != "Cpu"),
    );
    commands.insert_resource(models);
    build_survival_hud(&mut commands);
    commands.insert_resource(visuals);
    // The arena itself (counter, cover, ramps, turrets, pickups) is built from the map by `build_map`.
    let feast = Feast::with_map(start.0.clone(), options.demo);
    let (position, yaw) = feast.level.start(0);
    commands.insert_resource(Game {
        phase: Phase::Dormant,
        elapsed: 0.,
        phase_time: 0.,
        position,
        yaw,
        pitch: 0.,
        ghosts: actor::World::default(),
        online: Online::default(),
        infected: false,
        score: 0,
        kills: 0,
        weapon: Weapon::Rifle,
        ammo: OPENING_AMMO,
        owned: OPENING_OWNED,
        gun: Weapon::Rifle,
        last_gun: None,
        grab: None,
        cooldown: 0.,
        volley: 0,
        volley_wait: 0.,
        kick: 0.,
        whip: crate::whip::Swing::rest(),
        clap: STIM_CLAP,
        stimmed: false,
        prone: false,
        crouch: false,
        eye: EYE,
        pending_stim: false,
        scoped: false,
        aim_held: false,
        aim: 0.,
        wheel: Wheel::default(),
        hit: 0.,
        message: String::new(),
        message_time: 0.,
        notice: String::new(),
        notice_time: 0.,
        refused: None,
        enemies: vec![],
        pickups: vec![],
        effects: vec![],
        camera: None,
        knife: Some(knife),
        started: false,
        network: false,
        network_id: 0,
        online_count: 1,
        network_round: 0,
        pending_shot: false,
        pending_pickup: None,
        sway: Vec3::ZERO,
        sway_yaw: 0.,
        sway_at: Vec3::ZERO,
        carry_hands: None,
        carry_shift: Vec3::ZERO,
        pending_melee: false,
        pending_eat: false,
        pending_grab: false,
        pose: Pose::default(),
        pending_throw: false,
        pending_ready: None,
        laden: false,
        seen_round: 0,
        fort: None,
        kungfu: None,
        titan: None,
        fu_style: None,
        fu_hold: 0,
        build: 0.,
        direction: Vec3::NEG_Z,
        bite_time: 0.,
        frame: 0,
        viewmodels: vec![],
        lower: 0.,
        stim_arm: None,
        bolts: vec![],
        blobs: HashMap::new(),
        swarms: HashMap::new(),
        last_splat: 0,
        roster: feast.players.clone(),
        feast,
        actors: HashMap::new(),
        last_feast_shot: 0,
        demo: options.demo,
        alert: String::new(),
        rules: Default::default(),
        tuning: Default::default(),
        tune_editable: false,
        mag: Default::default(),
        spring: Default::default(),
    });
}

/// Startup: builds the arena from the map the game was started with.
pub fn build_map(world: &mut World) {
    splatter::prepare(world);
    let map = world.resource::<crate::level::StartMap>().0.clone();
    load_map(world, map);
}
/// Replaces the arena with the given map: scenery, pickups and rules. A round in progress restarts on
/// it; a hotdog being played keeps its place when that spot still exists, so an edit made in the map
/// editor can be watched from where the player stands.
pub fn load_map(world: &mut World, map: Map) {
    crate::level::build(world, &map);
    let v = world.resource::<Visuals>().clone();
    world.resource_scope(|world, mut g: Mut<Game>| {
        let stand = (g.playing() && !g.network && !g.demo).then_some((g.position, g.yaw, g.pitch));
        g.feast = Feast::with_map(map, g.demo);
        g.roster = g.feast.players.clone();
        g.fort = g.feast.fort_state();
        g.kungfu = if g.network { None } else { g.feast.kungfu_view() };
        g.titan = if g.network { None } else { g.feast.titan_view() };
        g.pickups = spawn_pickups(world, &v, &g.feast);
        if !g.playing() {
            (g.position, g.yaw) = g.start();
            return;
        }
        clear_round(world, &mut g);
        g.begin();
        let first = g.opening();
        phase(world, &mut g, first);
        if let Some((at, yaw, pitch)) = stand {
            let level = &g.feast.level;
            let ground = level.ground_at(at.x, at.z);
            if level.walkable(at.x as f64, ground as f64, at.z as f64) {
                let at = Vec3::new(at.x, ground, at.z);
                (g.position, g.yaw, g.pitch) = (at, yaw, pitch);
                if let Some(me) = g.feast.player_mut(1) {
                    me.p = at.to_array();
                    me.yaw = yaw;
                    me.camp = [at.x, at.z];
                }
            }
        }
    });
}
/// Multiplayer: the server has moved on to another map (the lobby's leader chose it) and seated this player on
/// hotdog `id` in the simulation it built for it. Nothing of the last round is kept and the next snapshot is taken
/// as a new round's; the arena is built from the map as it is at the welcome.
pub fn switch_map(world: &mut World, map: Map, id: u32) {
    world.resource_scope(|world, mut g: Mut<Game>| {
        clear_round(world, &mut g);
        g.network_id = id;
        g.network_round = 0;
    });
    load_map(world, map);
}
/// One collectable gun or box of ammunition per pickup of the map, in the map's order (the simulation and
/// the server refer to a pickup by that index), each on the pad it comes back to (`spawns`).
fn spawn_pickups(world: &mut World, v: &Visuals, feast: &Feast) -> Vec<Pickup> {
    let mut pickups = vec![];
    for item in &feast.pickups {
        let weapon = Weapon::index(item.weapon.max(1));
        let ammo = item.weapon == kitchen_sim::map::AMMO_BOX;
        let p = Vec3::from_array(item.p);
        let root = world
            .spawn((
                crate::level::MapPart,
                BattleProp,
                Name::new(if ammo { "Ammo box" } else { weapon.name() }),
                Transform::from_translation(p + Vec3::Y * 0.015),
                Visibility::Hidden,
            ))
            .id();
        let pad = crate::spawns::pad(world, p - Vec3::Y * 0.04, ammo);
        if ammo {
            crate::spawns::ammo_box(world, root);
            pickups.push(Pickup { state: PickupState { id: item.id, p: item.p, weapon: item.weapon, ammo: item.ammo, ready: 0., respawn: item.respawn }, entity: root, pad });
            continue;
        }
        piece(
            world,
            root,
            &v.cube,
            &v.steel,
            Vec3::ZERO,
            if weapon == Weapon::Spray {
                // A stubby can standing on end.
                Vec3::new(0.026, 0.05, 0.026)
            } else {
                Vec3::new(
                    0.018,
                    0.022,
                    if weapon == Weapon::Sniper {
                        0.15
                    } else {
                        0.085
                    },
                )
            },
        );
        piece(
            world,
            root,
            &v.sphere,
            &v.green,
            Vec3::new(0., 0.035, 0.),
            Vec3::splat(0.009),
        );
        // A dab of the condiment it fires tells the guns apart on the counter.
        piece(
            world,
            root,
            &v.sphere,
            &v.sauces[weapon as usize],
            Vec3::new(0., 0.004, if weapon == Weapon::Sniper { -0.08 } else { -0.046 }),
            Vec3::splat(0.011),
        );
        if weapon == Weapon::Sniper {
            piece(
                world,
                root,
                &v.cube,
                &v.dark,
                Vec3::new(0., 0.022, -0.01),
                Vec3::new(0.014, 0.015, 0.045),
            );
        }
        pickups.push(Pickup {
            state: PickupState {
                id: item.id,
                p: item.p,
                weapon: item.weapon,
                ammo: item.ammo,
                ready: 0.,
                respawn: item.respawn,
            },
            entity: root,
            pad,
        });
    }
    pickups
}

fn scope_image() -> Image {
    const SIZE: u32 = 512;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = (x as f32 + 0.5) / SIZE as f32 * 2. - 1.;
            let dy = (y as f32 + 0.5) / SIZE as f32 * 2. - 1.;
            let radius = dx.hypot(dy);
            let reticle =
                (dx.abs() < 0.003 && dy.abs() > 0.035) || (dy.abs() < 0.003 && dx.abs() > 0.035);
            let color = if radius >= 0.94 {
                [0, 0, 0, 255]
            } else if radius >= 0.925 || reticle {
                [100, 160, 140, 230]
            } else {
                [0, 0, 0, 0]
            };
            pixels.extend_from_slice(&color);
        }
    }
    Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn piece(
    world: &mut World,
    parent: Entity,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    at: Vec3,
    scale: Vec3,
) -> Entity {
    let id = world
        .spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at).with_scale(scale),
        ))
        .id();
    world.entity_mut(parent).add_child(id);
    id
}
/// The body of a zombie: an alley cat, built and animated by `cats`.
fn cat(world: &mut World, _: &Visuals, p: Vec3, id: u32) -> Entity {
    crate::cats::spawn(world, p, id)
}
pub fn remote_hotdog(world: &mut World, p: Vec3, _infected: bool, id: u32) -> Entity {
    let models = world
        .resource::<crate::player_model::PlayerModels>()
        .clone();
    models.spawn(world, p, id)
}
fn audio(world: &mut World, sound: Handle<AudioSource>) {
    world.spawn((AudioPlayer::new(sound), PlaybackSettings::DESPAWN));
}
/// A gun going off: the squirt, pitched by the gun. The syrup rifle's is a quick, high pop for each round, the
/// mustard pistol's a wet squirt of its own; the mayonnaise bottle's squelch is one long sound (`squirt`).
fn pew(world: &mut World, v: &Visuals, weapon: Weapon) {
    let (sound, speed) = match weapon {
        Weapon::Mayo => return,
        Weapon::Pistol => (&v.squirt, 1.),
        Weapon::Rifle => (&v.pew, 1.45),
        _ => (&v.pew, 1.),
    };
    world.spawn((AudioPlayer::new(sound.clone()), PlaybackSettings::DESPAWN.with_speed(speed)));
}
fn trail(world: &mut World, game: &mut Game, v: &Visuals, a: Vec3, b: Vec3) {
    let delta = b - a;
    let length = delta.length();
    if length < 0.001 {
        return;
    }
    let id = world
        .spawn((
            Mesh3d(v.cube.clone()),
            MeshMaterial3d(v.red.clone()),
            Transform::from_translation((a + b) * 0.5)
                .with_scale(Vec3::new(0.0012, 0.0012, length))
                .looking_to(delta, Vec3::Y),
            bevy::light::NotShadowCaster,
        ))
        .id();
    game.effects.push(Effect {
        entity: id,
        left: 0.065,
        velocity: Vec3::ZERO,
        drop: false,
    });
}
/// Pulls the trigger. Guns launch a round in the simulation (drawn by `update_rounds`) and kick; the syrup
/// rifle launches a burst, whose later rounds the simulation fires on its own. With the hands full there is no
/// gun to fire. Online, the server launches the round when the input arrives. In a Forts build phase no gun
/// fires: the head-up display says so.
fn shoot(world: &mut World, g: &mut Game, v: &Visuals, dir: Vec3) {
    if g.weapon == Weapon::Laden {
        return;
    }
    if g.locked() {
        g.notify(LOCKED);
        return;
    }
    if g.phase == Phase::Over {
        return;
    }
    // Reloading (reload.rs, kitchen-sim magazine.rs): the trigger does nothing; on an empty magazine it reloads, if there
    // is anything to reload it from (single player the simulation does that when the trigger is pulled).
    if g.mag.reload.is_some() {
        return;
    }
    if g.ammo[g.weapon as usize] <= 0 {
        if g.network {
            crate::reload::foresee(g);
        } else {
            g.feast.fire(1, dir, g.weapon as usize);
        }
        return;
    }
    // The mayonnaise flows while the trigger is held: the simulation lets each droplet go when it is due (its own
    // cooldown keeps the flow), and the bottle trembles in the fist and squelches for as long (`squirt`).
    if g.weapon == Weapon::Mayo {
        if g.ammo[Weapon::Mayo as usize] > 0 && (g.network || g.feast.fire(1, dir, feast::MAYO) || g.feast.player(1).is_some_and(|p| p.cooldown > 0.)) {
            g.pending_shot = true;
            g.kick = g.kick.max(0.6);
        }
        return;
    }
    if g.cooldown > 0. {
        return;
    }
    if g.network {
        if g.ammo[g.weapon as usize] <= 0 {
            return;
        }
    } else if !g.feast.fire(1, dir, g.weapon as usize) {
        return;
    }
    // (As the tuning has it, kitchen_sim::tune: a gun's time between shots once it is tuned, and its recoil.)
    let w = g.weapon as usize;
    let tune = &g.feast.tune;
    g.cooldown = if (tune.delay[w] - feast::DELAY[w]).abs() > 1e-6 { tune.delay[w] } else { g.weapon.delay() };
    g.pending_shot = true;
    // (The pistol's kick stacks on what is left of the last: a hand cannon fired again before it has come back down.)
    g.kick = if g.weapon == Weapon::Pistol { (g.kick + tune.recoil[w]).min(PISTOL_STACK * tune.recoil[w].max(1.)) } else { tune.recoil[w] };
    g.spring.shot();
    if g.weapon == Weapon::Rifle {
        (g.volley, g.volley_wait) = (g.feast.tune.burst.saturating_sub(1), g.feast.tune.gap);
    }
    pew(world, v, g.weapon);
}
/// The melee strike (F): a blow with the gun in hand (a pistol whip, a rifle butt...), which needs no
/// ammunition. The simulation lands it (`Feast::melee`: a zombie in reach is struck, and in the free for
/// all a living hotdog in reach is bitten); here the strike is shown and heard. Online the server decides,
/// and the strike is shown when the client's own cooldown allows one.
fn strike(g: &mut Game, dir: Vec3) {
    if g.phase == Phase::Over || g.weapon == Weapon::Laden {
        return;
    }
    if g.locked() {
        g.notify(LOCKED);
        return;
    }
    if g.network {
        g.pending_melee = true;
        if g.cooldown > 0. {
            return;
        }
    } else if !g.feast.melee(1, dir) {
        return;
    }
    g.cooldown = g.cooldown.max(MELEE_SWING);
    // (The swing is shown and heard by whip.rs: the whoosh now, the thwack and the hit-stop if the blow lands.)
    g.whip.start(g.weapon, g.elapsed);
}
/// Where a round ended: the splash, the mark it leaves and, for the relish glob, the blast, which is heard and felt
/// as well as seen (splatter.rs, splatter/boom.rs).
fn splash(world: &mut World, g: &Game, weapon: usize, at: Vec3, dir: Option<Vec3>, seed: u32, now: f32) {
    splatter::land(world, g, weapon, at, dir, seed, now);
}
/// The mayonnaise streams (squirt.rs): this player's own from what it squeezes this frame (`shoot` notes it), drawn
/// leaving the bottle in its first-person hand, which is squeezed while it flows; everyone else's from the droplets
/// the simulation (or the server) has in the air.
fn squirt_streams(world: &mut World, g: &mut Game, rounds: &[feast::Projectile], dt: f32) {
    let me = g.me();
    let squeezing = g.pending_shot && g.weapon == Weapon::Mayo && !g.demo;
    let mut nozzle = g.position + Vec3::Y * g.eye;
    let camera = g.camera.and_then(|c| world.get::<Transform>(c).copied());
    if let Some((_, model)) = g.viewmodels.iter().find(|(w, _)| *w == Weapon::Mayo) {
        let tip = crate::weapons::in_view(feast::MAYO).transform_point(crate::weapons::MUZZLE[feast::MAYO]);
        if let Some(mut tr) = world.get_mut::<Transform>(*model) {
            // The fist squeezes the bottle, a little in time with the flow.
            let squeeze = if squeezing { 0.7 + 0.3 * (g.elapsed * 31.).sin() } else { 0. };
            tr.scale = Vec3::new(1. - 0.05 * squeeze, 1. - 0.06 * squeeze, 1. + 0.015 * squeeze);
            if let Some(camera) = camera {
                nozzle = camera.transform_point(tr.transform_point(tip));
            }
        }
    }
    let p = g.roster.iter().find(|p| p.id == me);
    let stim = p.is_some_and(|p| p.stim > 0.);
    let from = g.position + Vec3::Y * g.eye;
    let launch = g.direction * g.feast.tune.speed[feast::MAYO] * if stim { g.feast.tune.stim_squirt } else { 1. };
    let mut targets: Vec<(Vec3, f32)> = g.feast.enemies.iter().filter(|e| e.forming <= 0.).map(|e| (Vec3::from_array(e.p) + Vec3::Y * 0.075, 0.065)).collect();
    targets.extend(g.roster.iter().filter(|p| p.id != me && p.alive).map(|p| (Vec3::from_array(p.p) + Vec3::Y * feast::CHEST[0], 0.055)));
    let mut nozzles = HashMap::new();
    for q in rounds.iter().filter(|q| q.weapon == feast::MAYO) {
        if !nozzles.contains_key(&q.owner) {
            if let Some(at) = crate::player_model::muzzle_of(world, q.owner) {
                nozzles.insert(q.owner, at);
            }
        }
    }
    let scene = crate::squirt::Scene { feast: &g.feast, targets, nozzles, me, watching: g.demo };
    crate::squirt::squeeze(world, squeezing, nozzle, from, launch, dt, &scene);
    crate::squirt::draw(world, rounds, &scene, g.position, dt);
}
/// Draws the simulation's rounds, splats and pests. Online the rounds come from the latest
/// snapshot and coast along their velocity until the next one.
fn update_rounds(world: &mut World, g: &mut Game, v: &Visuals, now: f32, dt: f32, view: Quat) {
    // (Online the rounds are already where the timeline draws them: online.rs `show`.)
    let coast = 0.;
    let me = if g.network { g.network_id } else { 1 };
    let rounds = g.feast.projectiles.clone();
    // The mayonnaise is one stream, not rounds (squirt.rs).
    squirt_streams(world, g, &rounds, dt);
    for q in rounds.iter().filter(|q| q.weapon != feast::MAYO) {
        let velocity = Vec3::from_array(q.v);
        if !g.blobs.contains_key(&q.id) {
            // My own rounds leave the gun in my hands rather than my eyes.
            let offset = if q.owner == me && !g.demo {
                view * Vec3::new(0.04, -0.036, 0.)
            } else {
                // Everyone else's rounds leave the muzzle of the gun they are seen holding.
                let start = Vec3::from_array(q.p) - velocity.normalize_or_zero() * q.travelled;
                crate::player_model::muzzle_of(world, q.owner).map_or(Vec3::ZERO, |muzzle| muzzle - start)
            };
            g.blobs.insert(q.id, splatter::spawn_round(world, q.weapon, q.owner, q.id, offset));
        }
        let blob = g.blobs.get_mut(&q.id).unwrap();
        blob.at = Vec3::from_array(q.p) + velocity * coast;
        blob.seen = g.frame;
        splatter::pose_round(world, blob, velocity, q.travelled + velocity.length() * coast, now, dt);
    }
    let frame = g.frame;
    let landed: Vec<u32> = g
        .blobs
        .iter()
        .filter(|(_, blob)| blob.seen != frame)
        .map(|(id, _)| *id)
        .collect();
    // The way each landed round was going tells its splash which way to fly and how its mark is drawn out.
    let mut heading = HashMap::new();
    for id in landed {
        if let Some(blob) = g.blobs.remove(&id) {
            heading.insert(id, blob.heading());
            splatter::despawn_round(world, blob);
        }
    }
    let splats = g.feast.splats.clone();
    for splat in splats {
        if splat.id > g.last_splat {
            g.last_splat = splat.id;
            let dir = heading.get(&splat.projectile).copied().flatten();
            splash(world, g, splat.weapon, Vec3::from_array(splat.p), dir, splat.id, now);
        }
    }
    // Flies buzz over the whole claimed area; parasites inch across the counter under them.
    let pests = g.feast.pests.clone();
    let gone: Vec<u32> = g
        .swarms
        .keys()
        .filter(|id| !pests.iter().any(|s| s.id == **id))
        .copied()
        .collect();
    for id in gone {
        if let Some(swarm) = g.swarms.remove(&id) {
            world.entity_mut(swarm.root).despawn();
        }
    }
    for pest in &pests {
        let swarm = g
            .swarms
            .entry(pest.id)
            .or_insert_with(|| spawn_swarm(world, v, Vec3::from_array(pest.p)));
        let (wide, deep) = (feast::SWARM[0], feast::SWARM[1]);
        let gather = smooth((g.elapsed - pest.born) / 0.8);
        for (i, fly) in swarm.flies.iter().enumerate() {
            let n = |k: u32| crate::player_model::noise(pest.id * 131 + i as u32 * 17 + k);
            let alive = (i as f32) < pest.hp / 100. * swarm.flies.len() as f32;
            if let Some(mut tr) = world.get_mut::<Transform>(*fly) {
                tr.translation = Vec3::new(
                    (now * (2. + n(0) * 5.) + n(1) * TAU).sin() * wide * (0.35 + 0.6 * n(2)),
                    0.03 + 0.1 * n(3) + 0.03 * (now * (5. + n(4) * 9.)).sin(),
                    (now * (2. + n(5) * 5.) + n(6) * TAU).cos() * deep * (0.35 + 0.6 * n(7)),
                );
                tr.scale = Vec3::new(0.0035, 0.0028, 0.005) * if alive { gather } else { 0. };
            }
        }
        for (i, grub) in swarm.grubs.iter().enumerate() {
            let n = |k: u32| crate::player_model::noise(pest.id * 71 + i as u32 * 29 + k);
            if let Some(mut tr) = world.get_mut::<Transform>(*grub) {
                let turn = now * (0.25 + 0.3 * n(0)) + n(1) * TAU;
                tr.translation = Vec3::new(turn.sin() * wide * 0.8 * n(2), 0.004, (turn * 1.3).cos() * deep * 0.8 * n(3));
                tr.rotation = Quat::from_rotation_y(turn * 2.) * Quat::from_rotation_x(PI * 0.5);
                // An inchworm stretch and squeeze.
                tr.scale = Vec3::new(0.003, 0.0045 + 0.0015 * (now * 6. + n(4) * TAU).sin(), 0.003) * gather;
            }
        }
    }
}
fn spawn_swarm(world: &mut World, v: &Visuals, at: Vec3) -> Swarm {
    let root = world
        .spawn((
            Name::new("Flies and parasites"),
            Transform::from_translation(at),
            Visibility::default(),
        ))
        .id();
    piece(
        world,
        root,
        &v.cube,
        &v.haze,
        Vec3::Y * 0.0015,
        Vec3::new(feast::SWARM[0] * 2., 0.001, feast::SWARM[1] * 2.),
    );
    let mut bug = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>| {
        let e = world
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_scale(Vec3::ZERO),
                bevy::light::NotShadowCaster,
            ))
            .id();
        world.entity_mut(root).add_child(e);
        e
    };
    Swarm {
        root,
        flies: (0..30).map(|_| bug(&v.sphere, &v.dark)).collect(),
        grubs: (0..8).map(|_| bug(&v.capsule, &v.white)).collect(),
    }
}
fn phase(world: &mut World, g: &mut Game, next: Phase) {
    g.phase = next;
    g.phase_time = 0.;
    crate::maya::publish_stage(match next {
        Phase::Knife => 7,
        Phase::Peeling => 8,
        Phase::Warmup => 9,
        Phase::Siege => 10,
        Phase::Famine => 13,
        Phase::Over | Phase::Rest => 12,
        Phase::Build => 9,
        Phase::Fight => 10,
        _ => 0,
    });
    info!("Kitchen combat: {:?}", next);
    let _ = world;
}

pub fn tick(world: &mut World) {
    // Single player's kill slow motion (slowmo.rs) runs the round, and this player with it, at its pace: every clock
    // below that the round goes by is stepped by the slowed frame. Online it is always the real one.
    let frame_dt = world.resource::<Time<Real>>().delta_secs() * crate::slowmo::sim_scale(world);
    let dt = frame_dt.min(0.05);
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let clock = world.resource::<Time<Real>>().elapsed_secs_f64();
    let keys = world.resource::<ButtonInput<KeyCode>>().clone();
    let mouse = world.resource::<ButtonInput<MouseButton>>().clone();
    let motion = world.resource::<AccumulatedMouseMotion>().delta;
    // The mouse wheel (a world without the input plugin has none). In winit, and so in Bevy, a positive `y` is a
    // roll away from the player (up, or content moving down) and a negative `y` a roll down: `deltaY` of a browser's
    // wheel is the other way round, and winit flips it.
    let scroll = world.get_resource::<AccumulatedMouseScroll>().copied().unwrap_or_default();
    // Touch controls (touch.rs): their buttons are already in `keys` and `mouse`; the stick, the drag and the
    // trigger are merged below.
    let touch = crate::touch::now(world);
    // What each key and button does (controls.rs: the defaults are the keys the game always had), and the player's
    // mouse and view settings (settings.rs).
    let bindings = world.get_resource::<crate::controls::Bindings>().cloned().unwrap_or_default();
    let settings = world.get_resource::<crate::settings::Settings>().copied().unwrap_or_default();
    let held = |action| bindings.held(action, &keys, &mouse);
    let tapped = |action| bindings.just(action, &keys, &mouse);
    // Single player stands still while the menu is open (menu.rs); online the round goes on, and hears nothing.
    if crate::menu::paused(world) {
        return;
    }
    let options = world.resource::<crate::runtime::Options>().clone();
    let complete = world
        .get_resource::<crate::maya::BreakfastShift>()
        .is_some_and(|s| s.finished());
    let v = world.resource::<Visuals>().clone();
    world.resource_scope(|world, gmut: Mut<Game>| {
        let mut g = gmut;
        g.frame += 1;
        g.pending_shot = false;
        g.pending_pickup = None;
        g.pending_melee = false;
        g.pending_eat = false;
        g.pending_stim = false;
        g.pending_grab = false;
        g.pending_throw = false;
        g.pending_ready = None;
        if g.camera.is_none() {
            g.camera = world
                .query_filtered::<Entity, With<crate::maya::HotdogCamera>>()
                .iter(world)
                .next();
        }
        if g.phase == Phase::Dormant {
            // Multiplayer: between rounds the players wait in the fridge and the arena sleeps (see lobby.rs).
            if world.get_resource::<crate::lobby::Lobby>().is_some_and(|lobby| lobby.holding) {
                return;
            }
            if options.skip_intro || (options.profile && now > 18.) {
                g.build = options.build;
                if options.connect.is_empty() {
                    g.rules = options.rules();
                }
                g.begin();
                let first = g.opening();
                phase(world, &mut g, first);
            } else if complete {
                phase(world, &mut g, Phase::Knife);
            } else {
                return;
            }
        }
        // Bound movement steps without stretching clocks when a frame takes longer.
        g.phase_time += frame_dt;
        g.cooldown = (g.cooldown - frame_dt).max(0.);
        // The rest of a syrup burst: each round kicks on top of the last and pops.
        if g.volley > 0 {
            g.volley_wait -= frame_dt;
            if g.volley_wait <= 0. {
                if g.weapon == Weapon::Rifle && g.ammo[Weapon::Rifle as usize] > 0 {
                    (g.volley, g.volley_wait) = (g.volley - 1, g.volley_wait + g.feast.tune.gap);
                    g.kick = (g.kick + 0.75 * g.feast.tune.recoil[feast::RIFLE]).min(2. * g.feast.tune.recoil[feast::RIFLE].max(1.));
                    pew(world, &v, Weapon::Rifle);
                } else {
                    g.volley = 0;
                }
            }
        }
        let recovery = (-frame_dt * g.weapon.recoil()[5]).exp();
        g.kick *= recovery;
        let kick = g.kick;
        g.spring.follow(kick, frame_dt);
        g.whip.advance(frame_dt);
        g.clap += frame_dt;
        g.hit = (g.hit - frame_dt).max(0.);
        g.message_time = (g.message_time - frame_dt).max(0.);
        g.notice_time = (g.notice_time - frame_dt).max(0.);
        if g.phase == Phase::Knife && g.phase_time > 4.5 {
            phase(world, &mut g, Phase::Peeling);
        }
        if g.phase == Phase::Peeling && g.phase_time > 7.5 {
            g.begin();
            let first = g.opening();
            phase(world, &mut g, first);
        }
        if let Some(t) = g.cutscene_time() {
            if let Some(id) = g.knife {
                if let Some(mut vis) = world.get_mut::<Visibility>(id) {
                    *vis = Visibility::Visible;
                }
                // Where Maya's hand holds it (maya/choreography.rs: the knife, and the film, are the scene's).
                let knife = crate::maya::choreography::knife(t);
                if let Some(mut tr) = world.get_mut::<Transform>(id) {
                    tr.translation = knife.translation;
                    tr.rotation = knife.rotation;
                }
            }
            let film = crate::maya::choreography::film(t);
            let films: Vec<Entity> = world
                .query_filtered::<Entity, With<PackageFilm>>()
                .iter(world)
                .collect();
            for e in films {
                if let Some(mut tr) = world.get_mut::<Transform>(e) {
                    tr.rotation = film.rotation;
                    tr.translation.y = film.translation.y;
                    tr.scale.z = film.scale.z;
                }
            }
            if let Some(cam) = g.camera {
                world.entity_mut(cam).remove::<ChildOf>();
                let pos = if t < 4.5 {
                    Vec3::new(0.91, 1.15, 2.87)
                } else {
                    Vec3::new(0.81, 1.12, 3.08)
                };
                let target = if t < 4.5 {
                    Vec3::new(0.57, 0.97, 3.04)
                } else {
                    Vec3::new(0.83, 0.96, 3.23)
                };
                *world.get_mut::<Transform>(cam).unwrap() =
                    Transform::from_translation(pos).looking_at(target, Vec3::Y);
                if let Projection::Perspective(ref mut p) =
                    *world.get_mut::<Projection>(cam).unwrap()
                {
                    p.fov = 60_f32.to_radians();
                }
            }
            update_hud(world, &g);
            return;
        }
        // Fully Loaded (cards.rs): at the card table the round stands down. Its clocks, its simulation and the hotdogs
        // drawn at their seats go on; this player's walking, looking, guns, pickups, camera and head-up display do not.
        let table = crate::cards::at_table(&g);
        if !g.started {
            g.started = true;
            if let Some(cam) = g.camera {
                world.entity_mut(cam).remove::<ChildOf>();
            }
            // The human actors leave the combat area; no skinning/cloth draw cost during combat.
            let humans: Vec<Entity> = world
                .query_filtered::<Entity, With<crate::human::Human>>()
                .iter(world)
                .collect();
            for e in humans {
                world.entity_mut(e).insert(Visibility::Hidden);
            }
            if let Some(id) = g.knife {
                world.entity_mut(id).insert(Visibility::Hidden);
            }
            let films: Vec<Entity> = world
                .query_filtered::<Entity, Or<(With<PackageFilm>, With<PackageLabel>)>>()
                .iter(world)
                .collect();
            for e in films {
                world.entity_mut(e).insert(Visibility::Hidden);
            }
            // (Kung Fu has no guns: hands and feet only, kungfu/view.rs.)
            if !options.demo && !table && !g.kung_fu() && g.viewmodels.is_empty() {
                if let Some(cam) = g.camera {
                    let (viewmodels, stim_arm, bolts) = build_viewmodels(world, &v, cam);
                    g.viewmodels = viewmodels;
                    g.stim_arm = Some(stim_arm);
                    g.bolts = bolts;
                }
            }
            crate::maya::publish_stage(if g.infected {
                11
            } else if g.elapsed >= WARMUP {
                10
            } else {
                9
            });
            if table {
                // (What to do at the table is the table's to say: cards.rs.)
            } else if g.forts() {
                g.notify("FORTS | Q carries what you look at | build, then steal the bacon | R when ready");
            } else {
                g.notify("ONE ZOMBIE WAVE | collect weapons and ammo | E to pick up");
            }
        }
        // Online: the other hotdogs, the cats and the rounds in the air where the timeline draws them, a little in
        // the past, and the shots it passes for network.rs to show (online.rs).
        if g.network {
            let shots = online::show(world, &mut g, clock, frame_dt);
            g.online.shots.extend(shots);
        }
        if g.phase == Phase::Over
            && !g.network
            && (tapped(Action::NewRound) || ((options.demo || g.feast.rules.ffa || g.feast.rules.cards || g.feast.rules.kungfu) && g.phase_time > 8.))
        {
            clear_round(world, &mut g);
            g.begin();
            let first = g.opening();
            phase(world, &mut g, first);
        }
        // (A new round may have begun just now.)
        let table = crate::cards::at_table(&g);
        if g.phase == Phase::Over {
            for mut cursor in world.query::<&mut CursorOptions>().iter_mut(world) {
                cursor.grab_mode = CursorGrabMode::None;
                cursor.visible = true;
            }
            g.scoped = false;
            // The round is frozen, but the last hotdog standing still celebrates.
            let shown = crate::slowmo::clock(world, now);
            update_players(world, &mut g, shown);
            update_hud(world, &g);
            return;
        }
        let mut captured = false;
        // Whether the game has the keyboard (the page's chat box open takes it, and the wheel is then not for the guns either).
        let mut focused = false;
        for (mut cursor, window) in world
            .query::<(&mut CursorOptions, &Window)>()
            .iter_mut(world)
        {
            focused |= window.focused;
            // (Esc is the menu's, menu.rs, which frees the pointer itself and keeps the key from the round while it is
            // open; this is for a world without it. A left click captures a free pointer whatever the trigger is bound to.)
            if keys.just_pressed(KeyCode::Escape) {
                cursor.grab_mode = CursorGrabMode::None;
                cursor.visible = true;
            } else if mouse.just_pressed(MouseButton::Left) && window.focused && !table {
                cursor.grab_mode = CursorGrabMode::Locked;
                cursor.visible = false;
            }
            captured = cursor.grab_mode == CursorGrabMode::Locked;
        }
        // (At the table the view is the table's, and so is a click, which captures the pointer only off its scoreboard:
        // cards.rs. Escape frees the pointer as in the round. Dead, the mouse and a drag take the death camera round,
        // not the hotdog: deathcam.rs.)
        let dead = g.roster.iter().any(|p| p.id == g.me() && !p.alive);
        // The mouse wheel's step this frame: +1 a notch rolled down, -1 up, 0 none. One roll is one step, however many
        // pixels or lines a browser makes of a notch: a trackpad's small turns gather to a step (`wheel_step`), a step
        // takes all that was turned with it, and the wheel is not heard for a moment after it (`WHEEL_COOLDOWN`; what
        // comes then is dropped). It is heard when the keys are: with the game holding the keyboard, alive, not at the
        // card table. What a step does is the bindings' (controls.rs: the guns, or turning what is carried, to begin
        // with), which may turn the wheel round.
        let mut wheel = 0;
        if focused && !dead && !table {
            if scroll.delta.y != 0. {
                if now - g.wheel.turned > WHEEL_REST {
                    g.wheel.gathered = 0.;
                }
                g.wheel.turned = now;
            }
            if now < g.wheel.heard {
                g.wheel.gathered = 0.;
            } else {
                wheel = wheel_step(&mut g.wheel.gathered, -scroll.delta.y, scroll.unit == MouseScrollUnit::Pixel);
                if wheel != 0 {
                    g.wheel.heard = now + WHEEL_COOLDOWN;
                }
            }
        } else {
            g.wheel.gathered = 0.;
        }
        // An action pressed this frame: by a key or a button, or by the wheel where it is bound to it.
        let pressed = |action| tapped(action) || bindings.wheeled(action, wheel);
        if !table && !dead && (captured || options.profile || touch.look != Vec2::ZERO) {
            // (The mouse at the player's sensitivity, and turned over for an inverted Y; a touch drag at the page's.)
            let motion = if captured || options.profile { motion * settings.look() } else { Vec2::ZERO } + touch.look;
            // (Slower through the sniper's scope, and through the syrup rifle's optic as many times slower as it
            // magnifies, scope.rs, so that the reticle crosses a target at the same pace as the crosshair at the hip.)
            let sensitivity = if g.scoped && g.weapon == Weapon::Sniper { 0.00065 } else { 0.0022 / crate::scope::zoom(smooth(g.aim)) };
            g.yaw -= motion.x * sensitivity;
            g.pitch = (g.pitch - motion.y * sensitivity).clamp(-1.48, 1.48);
        }
        // Virtual reality: the gun points where the hand points (vr/hands.rs), and the room knows that the player
        // has the view (vr.rs).
        #[cfg(feature = "vr")]
        if !table && !dead && let Some(mut stage) = world.get_resource_mut::<crate::vr::Stage>() {
            stage.played = true;
            // Given the view (a spawn, the film's end), the player faces the way the game faced the hotdog: the room
            // is turned so that the head looks along it.
            if !stage.playing && let Some(faced) = stage.faced {
                let turn = g.yaw - faced;
                stage.turn += turn;
                stage.faced = Some(g.yaw);
                stage.aim = stage.aim.map(|(yaw, pitch)| (yaw + turn, pitch));
            }
            if let Some((yaw, pitch)) = stage.aim {
                (g.yaw, g.pitch) = (yaw, pitch.clamp(-1.48, 1.48));
            }
        }
        // Checks only: a page with `?check` can aim the hotdog and hold its trigger (see forts::driven).
        let driven = crate::forts::driven();
        if let Some([yaw, pitch, _]) = driven {
            (g.yaw, g.pitch) = (yaw, pitch.clamp(-1.48, 1.48));
        }
        // The trigger: held Fire, or with something carried held Throw (both the left button to begin with), with the
        // pointer captured; or the checks', or the touch controls' Fire. (Which of the two counts is known below.)
        let squeezed = |action| !table && ((captured && held(action)) || driven.is_some_and(|aim| aim[2] > 0.5) || touch.fire);
        if options.profile {
            g.yaw += dt * 0.18;
            g.pitch = (now * 0.33).sin() * 0.10;
        }
        let rotation = Quat::from_rotation_y(g.yaw) * Quat::from_rotation_x(g.pitch);
        let dir = rotation * Vec3::NEG_Z;
        g.direction = dir;
        let mut movement = Vec2::ZERO;
        for (action, delta) in [
            (Action::Forward, Vec2::new(0., -1.)),
            (Action::Back, Vec2::Y),
            (Action::Left, Vec2::NEG_X),
            (Action::Right, Vec2::X),
        ] {
            if held(action) {
                movement += delta;
            }
        }
        // The touch stick walks at the share of the pace it is pushed (keys walk at the full pace).
        movement = (movement.normalize_or_zero() + touch.stick).clamp_length_max(1.);
        let me = g
            .roster
            .iter()
            .find(|p| p.id == if g.network { g.network_id } else { 1 })
            .cloned();
        let alive = me.as_ref().is_none_or(|p| p.alive);
        // The simulation owns the stim; its start is the cue for the clap.
        let stim = me.as_ref().is_some_and(|p| p.alive && p.stim > 0.);
        if stim && !g.stimmed {
            g.clap = 0.;
        }
        g.stimmed = stim;
        // What this player carries: a loose object (the roster says which), or in Forts the bacon.
        let carrying = alive && me.as_ref().is_some_and(|p| p.carry >= 0);
        let laden = carrying || (alive && g.fort.as_ref().is_some_and(|fort| fort.bacon.state == kitchen_sim::forts::HELD && fort.bacon.holder == g.me()));
        // Kung Fu: hands and feet (`kung_fu_input`, below). The guns, the scope, the stim, prone, picking up, carrying and
        // eating are not heard: their keys strike, kick, block, grab, dodge and change style instead.
        let fu = g.kung_fu();
        // Titan, as the titan (titan_input, below): its moves on the keys of the trigger, the aim, the melee and the stim
        // (controls.rs, the TITAN tab). It has no gun, takes no stim, lies prone, picks up, carries or eats nothing.
        let giant = !options.demo && crate::titan::titan_id(&g) == Some(g.me());
        if options.demo || !alive || g.phase == Phase::Rest || table {
            movement = Vec2::ZERO;
            g.prone = false;
            g.crouch = false;
        } else if fu || giant {
            g.prone = false;
            g.crouch = held(Action::Crouch);
        } else {
            // T drops prone or stands back up; Ctrl (or C) crouches while it is held; Shift slaps on a stim.
            if pressed(Action::Prone) {
                g.prone = !g.prone;
            }
            g.crouch = !g.prone && held(Action::Crouch);
            if pressed(Action::Stim) {
                g.pending_stim = true;
                if !g.network {
                    g.feast.stim(1);
                }
            }
        }
        if !g.network && !fu && !giant {
            let (prone, crouch) = (g.prone, g.crouch);
            g.feast.set_prone(1, prone);
            g.feast.set_crouch(1, crouch);
        }
        // Prone crawls at 40% speed and a crouch walks at 65%; stim runs at 150% and jumps higher.
        let stance = if g.prone { actor::PRONE_PACE } else if g.crouch { actor::CROUCH_PACE } else { 1. } as f32;
        // (The walk and the stim's as the tuning has them: kitchen_sim::tune, the constants unless tuned.)
        let speed = g.feast.tune.walk * stance * if stim { g.feast.tune.stim_walk } else { 1. } * crate::forts::burden(&g);
        let velocity = Quat::from_rotation_y(g.yaw)
            * Vec3::new(
                movement.x,
                0.,
                movement.y,
            )
            * speed;
        // Space jumps (out of a crouch too), or stands a prone hotdog up; the body itself refuses a jump
        // from mid-air.
        let mut jump = 0.;
        if pressed(Action::Jump) && alive && !options.demo && !table {
            if g.prone {
                g.prone = false;
            } else {
                jump = g.feast.tune.leap(stim) as f64;
            }
        }
        // The hotdog's body does the moving (kitchen_sim::actor): it walks at once, against the level and
        // the other bodies, and the step that follows works out its jump, its falls and any throw. (Online the
        // frame's input is made into the server's commands below, once everything pressed is known.)
        let stride = Vec2::new(velocity.x, velocity.z) * dt;
        if !g.network && !options.demo && alive && !table && !fu && !giant {
            let (yaw, pitch) = (g.yaw, g.pitch);
            // A thrown hotdog steers poorly until it has come to rest.
            let control = if g.feast.body(1).is_some_and(|body| body.launched) { g.feast.bodies.config.control as f32 } else { 1. };
            if let Some(at) = g.feast.walk(1, stride * control, yaw) {
                g.position = at;
            }
            if jump > 0. {
                g.feast.jump(1, jump as f32);
            }
            g.feast.look(1, pitch);
        }
        // A hotdog holds two guns, in two slots (`slots`): 1 takes the primary in hand, 2 the secondary, and Y
        // swaps between them. There are no bare hands: a hotdog always has a gun in hand unless its hands are
        // full (`Weapon::Laden`), which no key selects.
        // (At the card table no gun is taken in hand: cards.rs.)
        let [primary, secondary] = slots(&g.owned);
        let armed = alive && !laden && !table && !fu && !giant;
        for (action, slot) in [(Action::Primary, primary), (Action::Secondary, secondary)] {
            if let Some(weapon) = slot.filter(|_| armed && pressed(action)) {
                g.weapon = weapon;
                g.scoped = false;
            }
        }
        // Each gun's own key (none to begin with) takes it in hand while it is held; and the last gun in hand before
        // this one comes back (with two guns that is the other, as Y).
        for action in Action::GUNS {
            if let Some(weapon) = action.gun().map(Weapon::index).filter(|w| armed && g.owned[*w as usize] && pressed(action)) {
                g.weapon = weapon;
                g.scoped = false;
            }
        }
        if armed && pressed(Action::LastWeapon) {
            if let Some(last) = g.last_gun.filter(|w| *w != g.weapon && g.owned[*w as usize]) {
                g.weapon = last;
                g.scoped = false;
            }
        }
        // Y, and the wheel rolled down, take the next gun; the wheel rolled up the one before.
        let step = if pressed(Action::Swap) || pressed(Action::NextWeapon) {
            1
        } else if pressed(Action::PreviousWeapon) {
            -1
        } else {
            0
        };
        if armed && step != 0 {
            // (The sandbox may hand out more than two guns: Y goes round them all, in slot order.)
            if let Some(next) = next_gun(&g.owned, g.weapon, step) {
                g.weapon = next;
                g.scoped = false;
            }
        }
        // While a loose object is carried the wheel turns it instead, an eighth of a turn a step (rolled down,
        // clockwise seen from above), and X tips it over onto its front, then onto its side, then back (kitchen_sim
        // build.rs `Pose`): it is put down as it is held. Picked up, anything is held as the map stood it.
        let holding = carrying && !table && !options.demo;
        if !carrying {
            g.pose = Pose::default();
        }
        if holding {
            if pressed(Action::TurnClockwise) {
                g.pose = g.pose.turned(-1);
            }
            if pressed(Action::TurnAnticlockwise) {
                g.pose = g.pose.turned(1);
            }
            if pressed(Action::Tip) {
                g.pose = g.pose.tipped();
            }
        }
        if !g.network {
            let pose = g.pose;
            g.feast.set_held_pose(1, pose);
        }
        // A gun just collected comes to hand once the simulation confirms it is held.
        if let Some((weapon, until)) = g.grab {
            if g.owned[weapon as usize] {
                g.weapon = weapon;
                g.grab = None;
            } else if now > until {
                g.grab = None;
            }
        }
        // The gun in hand may have been swapped out for a third one.
        if g.weapon != Weapon::Laden && !g.owned[g.weapon as usize] {
            g.weapon = primary.or(secondary).unwrap_or(Weapon::Laden);
            g.scoped = false;
        }
        if g.weapon != Weapon::Laden {
            if g.weapon != g.gun {
                g.last_gun = Some(g.gun);
            }
            g.gun = g.weapon;
        }
        // Hands full: the guns are away, and the one last in hand comes back when they are free again.
        if laden {
            g.weapon = Weapon::Laden;
            g.scoped = false;
        } else if g.weapon == Weapon::Laden {
            g.weapon = if g.owned[g.gun as usize] { g.gun } else { primary.or(secondary).unwrap_or(Weapon::Laden) };
        }
        g.laden = laden;
        if !g.network {
            let weapon = g.weapon as usize;
            g.feast.select(1, weapon);
        }
        // The right button aims: the sniper's scope, or the syrup rifle's optic up to the eye. Held, as the settings have
        // it to begin with (settings.rs `scope_toggle`): up while the button is held, down when it is let go; or
        // toggled, up at one press and down at the next. The touch controls' Scope button always toggles: a thumb
        // cannot hold it down and aim as well. (At the table the right button is the table's, and no gun is in view:
        // cards.rs. Dead, nothing is aimed.)
        let toggle = settings.scope_toggle || touch.on;
        if table || !alive || fu || giant {
            (g.scoped, g.aim_held) = (false, false);
        } else if pressed(Action::Aim) && matches!(g.weapon, Weapon::Sniper | Weapon::Rifle) {
            g.scoped = !(toggle && g.scoped);
            g.aim_held = !toggle;
        } else if g.aim_held && !held(Action::Aim) {
            (g.scoped, g.aim_held) = (false, false);
        }
        // R reloads the gun in hand (reload.rs); in a Forts build phase, with the guns locked, R is ready instead.
        if armed && !options.demo && g.phase != Phase::Build && pressed(Action::Reload) {
            crate::reload::press(&mut g);
        }
        // The rifle comes up over AIM_TIME and goes back down as long (behind the forcefield it stays lowered).
        let raised = g.scoped && g.weapon == Weapon::Rifle && !g.locked();
        g.aim = if g.weapon == Weapon::Rifle { (g.aim + frame_dt / AIM_TIME * if raised { 1. } else { -1. }).clamp(0., 1.) } else { 0. };
        // Single player tells the simulation (a burst climbs half as much aimed); online it is the command's SCOPE.
        if !g.network {
            let scoped = g.scoped;
            g.feast.set_aimed(1, scoped);
        }
        let pos = g.position;
        let elapsed = g.elapsed;
        for i in 0..g.pickups.len() {
            // Nothing lies on the pads at a card table, and nothing is picked up; nor in Kung Fu (E grabs there).
            if table || fu {
                if let Some(mut vis) = world.get_mut::<Visibility>(g.pickups[i].entity).filter(|vis| **vis != Visibility::Hidden) {
                    *vis = Visibility::Hidden;
                }
                continue;
            }
            let p = Vec3::from_array(g.pickups[i].state.p);
            let (ready, respawn) = (g.pickups[i].state.ready, g.pickups[i].state.respawn);
            let available = ready <= elapsed;
            // Its pad shows it there, or counts it back.
            let item = g.pickups[i].entity;
            crate::spawns::show(world, &mut g.pickups[i].pad, item, ready, respawn, elapsed);
            if let Some(mut tr) = world.get_mut::<Transform>(item) {
                tr.rotation = Quat::from_rotation_y(now * 0.7);
                tr.translation.y = p.y + 0.01 + (now * 2. + i as f32).sin() * 0.008;
            }
            // (What E would take here, or when an empty pad's pickup is back, is shown by the crosshair: hud.rs.)
            // (The titan picks nothing up.)
            let near = p.distance(pos + Vec3::Y * 0.04) < 0.15 && !g.infected && !giant;
            let ammo = g.pickups[i].state.weapon == kitchen_sim::map::AMMO_BOX;
            if ammo && available && near {
                // A box of ammunition tops up the guns held, and is left for somebody else when they are full.
                // (It leaves the power weapons as they are: crate::hints::ammo_box.)
                // (Walked over, the simulation takes it by itself: kitchen-sim magazine.rs `scavenge`.)
                let (short, _) = crate::hints::ammo_box(&g.owned, &g.mag.reserve);
                if pressed(Action::Use) && short {
                    g.pending_pickup = Some(i);
                    if !g.network {
                        g.feast.pickup(1, i);
                    }
                    g.notify("AMMO BOX | GUNS REFILLED");
                }
                continue;
            }
            if available && near {
                let w = Weapon::index(g.pickups[i].state.weapon.max(1));
                // (A gun this player holds is not E's: walked over, its rounds go into that gun's reserve by themselves.)
                if pressed(Action::Use) && !g.owned[w as usize] {
                    g.pending_pickup = Some(i);
                    g.grab = Some((w, now + 1.));
                    if !g.network {
                        g.feast.pickup(1, i);
                    }
                    let amount = g.pickups[i].state.ammo;
                    let full = !g.owned[w as usize] && (1..feast::GUNS).filter(|i| g.owned[*i]).count() >= 2;
                    let message = if full {
                        format!("{} +{} AMMO | swapped for {}", w.name(), amount, g.gun.name())
                    } else {
                        format!("{} +{} AMMO", w.name(), amount)
                    };
                    g.notify(message);
                }
            }
        }
        // E at an electric griddle's dial switches it on or off (griddle.rs: within reach, facing it; anyone, in any phase).
        if !options.demo && alive && !table && !fu && !giant && pressed(Action::Use) {
            crate::griddle::press(world, &mut g);
        }
        // F strikes with whatever is in hand (in the free for all it bites a living hotdog in reach);
        // E eats one that is already dead. The living cannot be eaten.
        if !options.demo && alive && !carrying && !table && !fu && !giant && pressed(Action::Melee) {
            strike(&mut g, dir);
        }
        // Q picks up the loose object the hotdog looks at or stands beside, or puts down the one in its hands
        // where it aims. R, in a Forts build phase, says this player is ready for the forcefield to drop.
        if !options.demo && alive && g.phase != Phase::Rest && !table && !fu && !giant && pressed(Action::Carry) {
            g.pending_grab = true;
            // Forts: nothing is put down in a base (the ghost is grey there); Q leaves it in the hands.
            if carrying && g.placement([0.; 2]).is_some_and(|spot| spot.refused.is_some()) {
                g.notify("NOT IN A BASE");
            }
            if !g.network {
                let done = if carrying { g.feast.put(1, dir) } else { g.feast.grab(1, dir) };
                // Nothing in reach but a draped wall not looked at: how to pull it off.
                let p = g.position;
                let walled = g.feast.drapes.iter().any(|d| {
                    let (x, z) = ((d.rect[0] - p.x as f64).max(0.).max(p.x as f64 - d.rect[2]), (d.rect[1] - p.z as f64).max(0.).max(p.z as f64 - d.rect[3]));
                    x.hypot(z) < 0.35
                });
                if !done && !carrying && walled {
                    g.notify("Q AT THE WALL PULLS ITS NAPKIN OFF");
                } else if !done && !carrying {
                    g.notify("NOTHING TO PICK UP HERE");
                }
            }
        }
        if !options.demo && alive && g.phase == Phase::Build && pressed(Action::Ready) {
            let ready = !me.as_ref().is_some_and(|p| p.ready);
            g.pending_ready = Some(ready);
            if !g.network {
                g.feast.ready(1, ready);
            }
        }
        // (E eats in the siege's famine alone: Free For All, which shares its phase, has no eating.)
        if !options.demo && !fu && !giant && pressed(Action::Use) && g.phase == Phase::Famine && !g.feast.rules.ffa {
            g.pending_eat = true;
            if !g.network && g.feast.eat(1) {
                audio(world, v.bite.clone());
            }
        }
        // The eye sinks to the counter when prone and halfway when crouching; the simulation fires from
        // the same height. Crouched in the air the knees are tucked up and the head stays where the jump put it
        // (kitchen_sim feast.rs `tucked`): the eye does not dip.
        let airborne = me.as_ref().is_some_and(|p| !p.grounded);
        let low = if g.prone { feast::LOW } else if g.crouch && !airborne { feast::MID } else { EYE };
        let eye = g.eye + (low - g.eye) * (1. - (-frame_dt * 10.).exp());
        g.eye = eye;
        let trigger = squeezed(if carrying { Action::Throw } else { Action::Fire });
        if fu {
            let keys = FuKeys {
                // (A key bound to a strike strikes without the pointer captured; a mouse button needs it.)
                light: squeezed(Action::Light) || (!table && bindings.held_by_key(Action::Light, &keys)),
                heavy: (captured && held(Action::Heavy)) || bindings.held_by_key(Action::Heavy, &keys),
                kick: pressed(Action::Kick),
                block: held(Action::Block),
                grab: pressed(Action::Grab),
                dodge: pressed(Action::Dodge),
                special: held(Action::Special),
                jump: pressed(Action::Jump),
                style: Action::STYLES.iter().position(|a| pressed(*a)).map(|s| s as u8),
                step: if pressed(Action::NextStyle) || touch.next_style { 1 } else if pressed(Action::PreviousStyle) { -1 } else { 0 },
            };
            let playing = alive && !options.demo && g.phase != Phase::Rest && !table;
            kung_fu_input(&mut g, if playing { keys } else { FuKeys::default() }, movement, dt);
        } else if giant {
            let keys = TitanKeys {
                // (A key bound to the swat or the smash works without the pointer captured; a mouse button needs it.)
                swat: squeezed(Action::Swat) || (!table && bindings.held_by_key(Action::Swat, &keys)),
                smash: (captured && held(Action::Smash)) || bindings.held_by_key(Action::Smash, &keys),
                stomp: pressed(Action::Stomp),
                charge: pressed(Action::Charge),
                jump: pressed(Action::Jump),
            };
            let playing = alive && g.phase != Phase::Rest && !table;
            titan_input(&mut g, if playing { keys } else { TitanKeys::default() }, movement, dt);
        }
        if alive && !options.demo && g.phase != Phase::Rest && !table && !fu && !giant && (trigger || (options.profile && g.elapsed > 31.)) {
            if carrying {
                // The trigger throws what is carried.
                if g.cooldown <= 0. {
                    g.pending_throw = true;
                    g.cooldown = MELEE_SWING;
                    if !g.network {
                        g.feast.toss(1, dir);
                    }
                }
            } else {
                shoot(world, &mut g, &v, dir);
            }
        }
        // Online: what this frame pressed and holds becomes the server's commands, one a step, each applied at
        // once to this player's own body (online.rs).
        if g.network {
            let fire = if fu || giant {
                g.fu_hold & button::FIRE != 0
            } else {
                trigger && alive && !options.demo && g.phase != Phase::Rest && !carrying && g.weapon != Weapon::Laden && !g.locked()
            };
            let input = online::Input { stick: movement, yaw: g.yaw, pitch: g.pitch, fire, jump: jump > 0. };
            online::play(&mut g, input, frame_dt);
        }
        let origin = g.position + Vec3::Y * eye;
        // Recoil is steadier crouching, steadier still prone, far steadier on stim, and half as hard with the
        // rifle aimed down its sights. It moves the gun and the camera, never the aim: `dir` above is what
        // the simulation fires along.
        let aim = smooth(g.aim);
        let steady = steadiness(g.prone, g.crouch, stim, aim);
        let (kick_at, kick_turn, kick_pitch, quake) = recoil_pose(g.weapon, g.spring.shown * steady, now);
        // The melee strike's lean into the blow, its jolt when it lands and the shake after it (whip.rs).
        let (whip_pitch, whip_roll, whip_shake) = g.whip.camera(now);
        // A relish glob's burst shakes the view, the nearer the harder (splatter/boom.rs).
        let blast = splatter::boom::shake(world);
        // (At the table the camera is the table's: cards.rs.)
        if let Some(cam) = g.camera.filter(|_| !table) {
            let tr = Transform::from_translation(origin + rotation * (Vec3::Z * kick_at.z * 0.25))
                .with_rotation(rotation * Quat::from_rotation_x(kick_pitch + whip_pitch + blast.x) * Quat::from_rotation_y(quake.x + whip_shake.x + g.spring.yaw(kick_pitch) + blast.y) * Quat::from_rotation_z(quake.y + whip_roll + whip_shake.y + blast.z));
            *world.get_mut::<Transform>(cam).unwrap() = tr;
            if let Projection::Perspective(ref mut p) = *world.get_mut::<Projection>(cam).unwrap() {
                // The sniper's scope; the rifle's optic zooms as it comes up (scope.rs).
                p.fov = if g.scoped && g.weapon == Weapon::Sniper {
                    18_f32.to_radians()
                } else {
                    crate::scope::fov(settings.fov_radians(), aim) * (1. - g.whip.punch())
                };
                p.near = 0.004;
            }
        }
        // Behind the forcefield the gun is held lowered, muzzle down: it cannot fire until the field drops.
        let lower = g.lower + ((g.locked() as u8 as f32) - g.lower) * (1. - (-frame_dt * 8.).exp());
        g.lower = lower;
        for &(weapon, entity) in &g.viewmodels {
            if let Some(mut vis) = world.get_mut::<Visibility>(entity) {
                // (Kung Fu: hands and feet, never a gun: kungfu/view.rs draws the first person's limbs.)
                // (`g.demo` is the demo's, and a replay's viewer's while it is not through a player's eyes: replay/.)
                *vis = if alive && !g.demo && !carrying && !table && !fu && !giant && weapon == g.weapon && !(g.scoped && weapon == Weapon::Sniper) { Visibility::Visible } else { Visibility::Hidden };
            }
            // Both hands are busy during the stim clap, so the weapon dips out of the way.
            let u = g.clap / STIM_CLAP;
            let dip = Vec3::Y * -0.09 * smooth(u / 0.2) * (1. - smooth((u - 0.75) / 0.25));
            if let Some(mut tr) = world.get_mut::<Transform>(entity) {
                // The melee strike moves and turns the whole viewmodel.
                let (blow_at, blow_turn) = g.whip.pose(weapon);
                // The syrup rifle aimed is held up to the eye, its sight line along the view (`rifle_hold`).
                let hold = if weapon == Weapon::Rifle { rifle_hold(aim) } else { GRIP };
                tr.translation = hold + kick_at + dip + blow_at + Vec3::new(-0.012, -0.034, 0.022) * lower;
                tr.rotation = Quat::from_rotation_x(-0.62 * lower) * Quat::from_rotation_y(0.25 * lower) * blow_turn * kick_turn;
            }
        }
        // Stim in first person: the left forearm comes up across the view and the right hand claps
        // down on it, then blue fire and lightning burn round the edges of the view while it lasts.
        if let Some((arm, hand)) = g.stim_arm {
            let u = g.clap / STIM_CLAP;
            let shown = u < 1. && alive && !g.demo && !table;
            let rise = smooth(u / 0.25) * (1. - smooth((u - 0.75) / 0.25));
            let slap = smooth((u - 0.30) / 0.14);
            world.entity_mut(arm).insert(if shown {
                Visibility::Visible
            } else {
                Visibility::Hidden
            });
            if let Some(mut tr) = world.get_mut::<Transform>(arm) {
                // The slap knocks the forearm down a touch.
                let knock = 0.006 * (1. - ((u - 0.44) / 0.08).abs()).max(0.);
                tr.translation = Vec3::new(-0.018, -0.115 + 0.055 * rise - knock, -0.115);
                tr.rotation = Quat::from_rotation_z(1.25) * Quat::from_rotation_x(-0.25);
            }
            if let Some(mut tr) = world.get_mut::<Transform>(hand) {
                // In the forearm's frame: from raised beside it to flat on top of it.
                tr.translation = Vec3::new(0.05, 0.004, 0.012).lerp(Vec3::new(0.0135, 0.006, 0.), slap);
            }
        }
        // The stim's bolts: blue fire licks up round the bottom and the sides of the view and lightning crackles over the
        // hands and the gun, up with the clap and down over the stim's last half second (player_model/stim.rs).
        let left = me.as_ref().filter(|_| stim && !g.demo && !table).map_or(0., |p| p.stim);
        crate::player_model::stim::view(world, &g.bolts, g.camera, now, crate::player_model::stim::strength(left, g.clap));
        let mut i = 0;
        // (Over a sink the counter is the sink's floor.)
        let sinks: Vec<([f32; 4], f32)> = g.feast.level.map.sinks.iter().map(|s| (s.rect.map(|v| v as f32), s.depth as f32)).collect();
        while i < g.effects.len() {
            let e = &mut g.effects[i];
            e.left -= frame_dt;
            if e.drop {
                e.velocity.y -= 2.8 * dt;
                if let Some(mut tr) = world.get_mut::<Transform>(e.entity) {
                    tr.translation += e.velocity * dt;
                    let p = tr.translation;
                    let floor = sinks.iter().find(|(r, _)| p.x >= r[0] && p.x <= r[2] && p.z >= r[1] && p.z <= r[3]).map_or(FLOOR, |(_, depth)| FLOOR - depth);
                    // A droplet that reaches the counter stops and flattens into a small puddle.
                    if tr.translation.y < floor + 0.002 {
                        tr.translation.y = floor + 0.002;
                        tr.scale *= Vec3::new(1.7, 0.2, 1.7);
                        e.drop = false;
                    }
                }
            } else if e.left < 0.15 {
                // Puddles and beams shrink away in their last moments.
                if let Some(mut tr) = world.get_mut::<Transform>(e.entity) {
                    tr.scale *= (1. - frame_dt * 6.).max(0.);
                }
            }
            if g.effects[i].left <= 0. {
                let effect = g.effects.swap_remove(i);
                world.entity_mut(effect.entity).despawn();
            } else {
                i += 1;
            }
        }
        if !g.network {
            g.feast.tick(frame_dt);
            // Kung Fu: the fight as it stands now (hud.rs, the animation, the sounds read it).
            g.kungfu = g.feast.kungfu_view();
            g.titan = g.feast.titan_view();
            sync_local(world, &mut g, &v);
        }
        // This player's reserves and reload as shown, the +N of a pickup walked over, the reload's sounds (reload.rs).
        crate::reload::tick(world, &mut g, now);
        let online = g.network;
        crate::forts::update(world, &mut g, now);
        update_rounds(world, &mut g, &v, now, frame_dt, rotation);
        splatter::update(world, &mut g, now, frame_dt);
        // The blows that have landed (whip.rs), for the third-person swings to hold on and this player's own to feel.
        crate::whip::observe(world, &mut g, now);
        // (At the slow motion's pace: the hotdogs' legs keep step with how far they go, slowmo.rs.)
        let shown = crate::slowmo::clock(world, now);
        update_players(world, &mut g, shown);
        // The loose objects once the hotdogs stand where this frame has them: a carried napkin hangs from
        // their hands.
        let (holds, bodies, mine) = carriers(world, &mut g, frame_dt, now);
        crate::loose::update(world, &mut g.feast, online, frame_dt, &holds, &bodies, mine);
        if let Some(cam) = g.camera {
            // In first person: what is carried, at its true size where it is held, or for a napkin the two fists
            // that hold the sheet hanging from the view.
            let kind = mine.map(|id| g.feast.props.body(id).shape);
            let carried = mine.zip(kind).filter(|(_, kind)| *kind != kitchen_sim::drape::NAPKIN).map(|(id, kind)| {
                let (p, q) = (g.feast.props.position(id), g.feast.props.rotation(id));
                let turn = Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32).normalize();
                (kind, Transform::from_xyz(p[0] as f32, p[1] as f32, p[2] as f32).with_rotation(turn))
            });
            let eye = Transform::from_translation(g.position + Vec3::Y * g.eye).with_rotation(Quat::from_rotation_y(g.yaw) * Quat::from_rotation_x(g.pitch));
            crate::loose::held(world, cam, carried, eye, now, g.sway);
            let napkin = kind == Some(kitchen_sim::drape::NAPKIN);
            if napkin && g.carry_hands.is_none() {
                g.carry_hands = crate::viewmodel::carrying_hands(world, cam);
            }
            if let Some(hands) = g.carry_hands {
                let shown = if napkin { Visibility::Inherited } else { Visibility::Hidden };
                if world.get::<Visibility>(hands).is_some_and(|v| *v != shown) {
                    world.entity_mut(hands).insert(shown);
                }
                if let Some(mut tr) = world.get_mut::<Transform>(hands) {
                    tr.translation = g.carry_shift;
                }
            }
        }
        // Where what this player carries would land if Q put it down now: its ghost, and what the tooltip says of it.
        crate::preview::show(world, &mut g, frame_dt);
        if (options.demo || !alive) && !table {
            if let Some(cam) = g.camera {
                let (eye, target) = spectator(g.feast.level.map.bounds, now * 0.18);
                *world.get_mut::<Transform>(cam).unwrap() = Transform::from_translation(eye)
                    .looking_at(target, Vec3::Y);
                if let Projection::Perspective(ref mut p) =
                    *world.get_mut::<Projection>(cam).unwrap()
                {
                    p.fov = 58_f32.to_radians();
                }
            }
        }
        microwave(world, &g, now);
        aim_turrets(world, &g);
        update_hud(world, &g);
    });
}
/// Turns each turret's model on its post to the yaw the simulation (or the server) has it at.
/// Kung Fu: what this frame's keys ask of the fight (controls.rs, the KUNG FU and STYLES tabs).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FuKeys {
    /// Held: the light strike, the heavy strike, the block, the special.
    pub light: bool,
    pub heavy: bool,
    pub block: bool,
    pub special: bool,
    /// Pressed this frame: the kick, the grab, the dodge, the jump.
    pub kick: bool,
    pub grab: bool,
    pub dodge: bool,
    pub jump: bool,
    /// A style by name, or the next (+1) or the last (-1).
    pub style: Option<u8>,
    pub step: i32,
}
/// Kung Fu's buttons for `keys` (interface.md section 5): the levels every command carries (FIRE the light strike held,
/// SCOPE the heavy, GRAB the block, PRONE the special, CROUCH) and the presses one command carries (MELEE the kick,
/// THROW the grab, STIM the dodge, JUMP).
pub fn fu_buttons(keys: &FuKeys, crouch: bool) -> (u16, u16) {
    let bit = |on: bool, b: u16| if on { b } else { 0 };
    let held = bit(keys.light, button::FIRE) | bit(keys.heavy, button::SCOPE) | bit(keys.block, button::GRAB) | bit(keys.special, button::PRONE) | bit(crouch, button::CROUCH);
    let pressed = bit(keys.kick, button::MELEE) | bit(keys.grab, button::THROW) | bit(keys.dodge, button::STIM) | bit(keys.jump, button::JUMP);
    (held, pressed)
}
/// The style asked for after `keys`: by name, or the next or last from the one asked for before (or the fighter's own).
pub fn fu_style(keys: &FuKeys, asked: Option<u8>, fighting: Option<u8>) -> Option<u8> {
    let styles = kitchen_sim::kungfu::STYLES as i32;
    if let Some(style) = keys.style {
        return Some(style);
    }
    let now = asked.or(fighting)?;
    Some(if keys.step != 0 { (now as i32 + keys.step).rem_euclid(styles) as u8 } else { now })
}
/// Kung Fu, this frame: the style asked for, and the fight's buttons; offline a command to the round itself
/// (`Feast::kungfu_command`, which walks, jumps, looks and fights as the server does), online the levels and presses for
/// online.rs to put in every command (`fu_hold`, and the pending presses it latches).
fn kung_fu_input(g: &mut Game, keys: FuKeys, movement: Vec2, dt: f32) {
    let me = g.me();
    let fighting = g.fighter(me).map(|f| f.style);
    g.fu_style = fu_style(&keys, g.fu_style, fighting);
    let (held, pressed) = fu_buttons(&keys, g.crouch);
    g.fu_hold = held;
    let weapon = g.fu_style.map_or(0, |s| s + 1);
    if g.network {
        g.pending_melee |= keys.kick;
        g.pending_throw |= keys.grab;
        g.pending_stim |= keys.dodge;
        return;
    }
    let command = kitchen_sim::net::Command::new(g.frame, movement, g.yaw, g.pitch, held | pressed, weapon, None);
    g.feast.kungfu_command(1, &command, dt);
    if let Some(body) = g.feast.body(1) {
        g.position = Vec3::new(body.p[0] as f32, body.p[1] as f32, body.p[2] as f32);
    }
}

/// The titan, as this player is it: what this frame's keys ask of it (controls.rs, the TITAN tab).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TitanKeys {
    /// Held: the swat, the ground smash.
    pub swat: bool,
    pub smash: bool,
    /// Pressed this frame: the stomp (a kick crouched), the charge, the jump.
    pub stomp: bool,
    pub charge: bool,
    pub jump: bool,
}
/// The titan's buttons for `keys` (titan/interface.md section 1 and 4: no new bits): the levels every command carries
/// (FIRE the swat, SCOPE the smash, CROUCH) and the presses one command carries (MELEE the stomp, STIM the charge, JUMP).
pub fn titan_buttons(keys: &TitanKeys, crouch: bool) -> (u16, u16) {
    let bit = |on: bool, b: u16| if on { b } else { 0 };
    let held = bit(keys.swat, button::FIRE) | bit(keys.smash, button::SCOPE) | bit(crouch, button::CROUCH);
    let pressed = bit(keys.stomp, button::MELEE) | bit(keys.charge, button::STIM) | bit(keys.jump, button::JUMP);
    (held, pressed)
}
/// The titan, this frame: offline a command to the round itself (`Feast::titan_command`, which walks it, turns it,
/// crouches it and starts its moves as the server does); online the levels for online.rs to put in every command
/// (`fu_hold`, as Kung Fu's) and the presses it latches.
fn titan_input(g: &mut Game, keys: TitanKeys, movement: Vec2, dt: f32) {
    let (held, pressed) = titan_buttons(&keys, g.crouch);
    (g.fu_hold, g.fu_style) = (held, None);
    if g.network {
        g.pending_melee |= keys.stomp;
        g.pending_stim |= keys.charge;
        return;
    }
    let me = g.me();
    let command = kitchen_sim::net::Command::new(g.frame, movement, g.yaw, g.pitch, held | pressed, 0, None);
    g.feast.titan_command(me, &command, dt);
    if let Some(body) = g.feast.body(me) {
        g.position = Vec3::new(body.p[0] as f32, body.p[1] as f32, body.p[2] as f32);
    }
}

fn aim_turrets(world: &mut World, g: &Game) {
    crate::queries::with::<(&crate::level::Turret, &mut Transform), (), _>(world, |world, q| {
        for (turret, mut tr) in q.iter_mut(world) {
            if let Some(yaw) = g.feast.turret_yaw.get(turret.0) {
                let rotation = Quat::from_rotation_y(*yaw);
                if tr.rotation != rotation {
                    tr.rotation = rotation;
                }
            }
        }
    });
}
/// Seconds a note the player must see (a mode the map refuses) stays on the screen.
pub const NOTICE: f32 = 6.;
/// Offline: the rules as they are played on `map`. A mode the map refuses (`Map::refuses`: Titan on a map with no titan
/// layout, Kung Fu off the dojo, Forts with no Forts layout, anything on the dojo but Kung Fu) gives way to the map's own
/// mode, every other rule kept, and the reason comes back to be said. (Online the server refuses such a game itself.)
pub fn fit_rules(rules: &kitchen_sim::rules::Rules, map: &Map) -> (kitchen_sim::rules::Rules, Option<String>) {
    use kitchen_sim::rules::{Mode, Rules};
    let Some((asked, why)) = rules.mode().and_then(|asked| map.refuses(asked).map(|why| (asked, why))) else {
        return (rules.clone(), None);
    };
    let own = Mode::made_for(map);
    let base = Rules::playing(own, map);
    let fitted = Rules { siege: base.siege, forts: base.forts, ffa: base.ffa, finite_ammo: base.finite_ammo, cards: base.cards, kungfu: base.kungfu, titan: base.titan, ..rules.clone() };
    (fitted, Some(format!("{why}: playing {} instead of {}.", own.title(), asked.title())))
}
/// The spectator camera's eye and target at an orbit angle. Framed on the bundled counter (3.6 m by
/// 1.36 m) and scaled about the middle of the map's bounds, so any counter fills the view the same way.
fn spectator(bounds: [f64; 4], angle: f32) -> (Vec3, Vec3) {
    let [x0, z0, x1, z1] = bounds.map(|v| v as f32);
    let (x, z) = ((x0 + x1) / 2., (z0 + z1) / 2.);
    let size = ((x1 - x0) / 3.6).max((z1 - z0) / 1.36);
    (
        Vec3::new(x + angle.sin() * 0.95 * size, FLOOR + 0.5275 * size, z + (angle.cos() * 0.12 - 1.) * size),
        Vec3::new(x + 0.1 * size, FLOOR + 0.1225 * size, z + 0.07 * size),
    )
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn microwave(world: &mut World, g: &Game, now: f32) {
    const DIGITS: [u8; 10] = [
        0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
        0b1111111, 0b1101111,
    ];
    let remaining = (g.feast.warmup - g.elapsed).ceil().max(0.) as usize;
    let digits = [0, remaining / 60, remaining % 60 / 10, remaining % 10];
    // (A card table on top is no warm-up: no wave is coming, and the clock stays dark.)
    let table = crate::cards::at_table(g);
    // (Written only where they change: a write marks them changed, and the visibility of everything under them is
    // worked out again.)
    crate::queries::with::<(&MicrowaveDigit, &mut Visibility), (), _>(world, |world, q| {
        for (digit, mut vis) in q.iter_mut(world) {
            vis.set_if_neq(if !table && DIGITS[digits[digit.0].min(9)] & (1 << digit.1) != 0 {
                Visibility::Visible
            } else {
                Visibility::Hidden
            });
        }
    });
    crate::queries::with::<&mut Visibility, With<MicrowaveGlow>, _>(world, |world, q| {
        for mut vis in q.iter_mut(world) {
            vis.set_if_neq(if (now * 17.).sin() > -0.3 { Visibility::Visible } else { Visibility::Hidden });
        }
    });
    crate::queries::with::<(&Smoke, &mut Transform, &mut Visibility), (), _>(world, |world, q| {
        for (smoke, mut tr, mut vis) in q.iter_mut(world) {
            vis.set_if_neq(Visibility::Visible);
            let t = (now * 0.28 + smoke.0 as f32 * 0.1) % 1.;
            // Relative to the vent on top of the microwave the puff belongs to.
            tr.translation = Vec3::new((t * 8. + smoke.0 as f32).sin() * 0.045, t * 0.38, -t * 0.035);
            tr.scale = Vec3::splat(0.015 + t * 0.07);
        }
    });
}
fn update_hud(world: &mut World, g: &Game) {
    // Dead in a round, the player watches its body (deathcam.rs): nothing of first person is left (crosshair, scope,
    // prompts); the head-up display keeps the round's score and clock (hud.rs).
    let dead = g.playing() && !g.demo && g.roster.iter().any(|p| p.id == g.me() && !p.alive);
    // At the card table none of the round's display is up, the scope included (the table has its own: cards.rs).
    let table = crate::cards::at_table(g);
    // What just happened (a pickup, an infection, how to start) is told in words only with the hints on (hints.rs); the
    // head-up display shows it in icons and its moments.
    let hinted = world.get_resource::<crate::hints::Hints>().is_some_and(|h| h.on) && !g.demo;
    crate::queries::with::<(&Hud, &mut Text, &mut Visibility, &mut TextColor), (), _>(world, |world, q| {
        for (kind, mut text, mut vis, mut color) in q.iter_mut(world) {
            // (Each written only when it changes: a text or a colour written is shaped and laid out again.)
            vis.set_if_neq(if g.active() && !table {
                Visibility::Visible
            } else {
                Visibility::Hidden
            });
            let content = match kind.0 {
                2 => {
                    if g.phase == Phase::Knife {
                        "She reaches for the knife.".into()
                    } else if g.phase == Phase::Peeling {
                        "That sound. The seal is opening.".into()
                    } else if let Some(note) = g.notice() {
                        note.to_string()
                    } else if g.message_time > 0. && !dead && hinted && g.phase != Phase::Over {
                        g.message.clone()
                    } else {
                        "".into()
                    }
                }
                3 => {
                    if g.playing() && !dead {
                        // (Aimed down the syrup rifle's sights, the bead is the crosshair.)
                        if g.hit > 0. {
                            "x"
                        } else if g.aim > 0.5 {
                            ""
                        } else {
                            "+"
                        }
                        .into()
                    } else {
                        "".into()
                    }
                }
                _ => String::new(),
            };
            if text.0 != content {
                text.0 = content;
            }
            color.set_if_neq(TextColor(if g.infected {
                Color::srgb(0.3, 1., 0.2)
            } else if kind.0 == 3 && g.hit > 0. {
                Color::srgb(1., 0.16, 0.12)
            } else {
                Color::srgb(0.78, 0.94, 0.89)
            }));
        }
    });
    update_survival_hud(world, g);
    // The hint panel under the lines at the top, and the tooltip under the crosshair (hints.rs).
    crate::hints::show(world, g);
    crate::queries::with::<&mut Visibility, With<ScopeMask>, _>(world, |world, q| {
        for mut vis in q.iter_mut(world) {
            vis.set_if_neq(if g.scoped && g.weapon == Weapon::Sniper && g.playing() && !dead && !table {
                Visibility::Visible
            } else {
                Visibility::Hidden
            });
        }
    });
}

// Server snapshots are the only source of online damage, infection, points and ammunition.
fn clear_round(world: &mut World, g: &mut Game) {
    for enemy in g.enemies.drain(..) {
        world.entity_mut(enemy.entity).despawn();
    }
    for effect in g.effects.drain(..) {
        world.entity_mut(effect.entity).despawn();
    }
    for (_, entity) in g.viewmodels.drain(..) {
        world.entity_mut(entity).despawn();
    }
    for entity in g.bolts.drain(..).chain(g.stim_arm.take().map(|(arm, _)| arm)) {
        world.entity_mut(entity).despawn();
    }
    for (_, blob) in g.blobs.drain() {
        splatter::despawn_round(world, blob);
    }
    splatter::clear(world);
    crate::squirt::clear(world);
    for (_, swarm) in g.swarms.drain() {
        world.entity_mut(swarm.root).despawn();
    }
    g.feast.projectiles.clear();
    g.feast.splats.clear();
    g.feast.pests.clear();
    g.last_splat = 0;
    g.kick = 0.;
    g.prone = false;
    g.crouch = false;
    for (_, entity) in g.actors.drain() {
        world.entity_mut(entity).despawn();
    }
    g.scoped = false;
    g.aim = 0.;
    g.ghosts.actors.clear();
    g.online.reset();
    g.cooldown = 0.;
    g.started = false;
    g.mag.reload = None;
}

/// Network play: rebuilds the client's copy of the bodies from a frame. Everyone else stands where the server has
/// them; this player's own body is kept, since it is ahead of the server by the journey there and back (online.rs
/// checks it against the server's word).
fn mirror(g: &mut Game) {
    let me = g.network_id;
    let mine = g.ghosts.get(me).cloned();
    g.ghosts.actors.clear();
    let at = |p: [f32; 3]| [p[0] as f64, p[1] as f64, p[2] as f64];
    if let Some(mine) = mine {
        g.ghosts.add(mine);
    }
    for p in g.online.latest.iter().filter(|p| p.alive && p.id != me) {
        // (The titan is a giant: its footprint, its stature and its mass, so nobody predicts walking through it.)
        if let Some(giant) = crate::titan::ghost_of(g, p.id) {
            g.ghosts.add(giant);
            continue;
        }
        let mut body = Actor { radius: feast::HOTDOG_RADIUS, ..Actor::hotdog(p.id, at(p.p)) };
        (body.prone, body.crouch, body.grounded, body.v[1]) = (p.prone, p.crouch, p.grounded, p.v[1] as f64);
        g.ghosts.add(body);
    }
    for e in g.feast.enemies.iter().filter(|e| e.forming <= 0.) {
        g.ghosts.add(Actor::zombie(feast::CAT + e.id, at(e.p)));
    }
}
/// Network play: a frame from the server (kitchen_sim::net). What the round is doing, the HUD's numbers, the props,
/// the ooze and which cats there are come from it at once; where the other hotdogs, the cats and the rounds are goes
/// into the timeline, drawn a little in the past (online.rs `show`); and this player's own body, predicted ahead, is
/// checked against where the server had it after the last of its commands (online.rs `settle`).
pub fn apply_frame(world: &mut World, frame: &kitchen_sim::net::Frame) {
    world.resource_scope(|world, mut g: Mut<Game>| {
        let now = world.resource::<Time<Real>>().elapsed_secs_f64();
        g.online_count = frame.players.len();
        g.roster = frame.players.iter().map(|seat| seat.player.clone()).collect();
        g.online.latest = g.roster.clone();
        g.alert = frame.alert.clone();
        // Free For All and the sandbox (kitchen_sim::rules): what the server plays by, which this client's copy of
        // the round plays by too (Free For All leaves a Forts map's forcefield out), and Free For All's clock.
        if frame.rules != g.feast.rules.text() {
            g.feast.set_rules(kitchen_sim::rules::Rules::parse(&frame.rules));
        }
        g.feast.ends = frame.ends;
        if g.feast.tune != g.tuning {
            g.feast.tune = g.tuning.clone();
        }
        g.feast.wave_spawned = frame.wave_spawned;
        g.feast.wave_kills = frame.wave_kills;
        g.feast.friendly_fire_events = frame.friendly_fire_events;
        g.feast.famine_at = frame.famine_at;
        g.feast.warmup = frame.warmup;
        g.feast.winner = frame.winner;
        // Kung Fu's word comes on its own (network.rs); a round that is not Kung Fu forgets it.
        if !(g.feast.rules.kungfu || g.feast.level.map.mode == "kungfu") {
            g.kungfu = None;
        }
        // (And the titan's: titan/.)
        if !(g.feast.rules.titan || g.feast.level.map.mode == kitchen_sim::titan::MODE) {
            g.titan = None;
        }
        // Forts: the mode's state, and the forcefield, which this client's level and props keep as the server's does.
        g.fort = frame.forts.clone();
        if g.fort.as_ref().is_some_and(|fort| !fort.field) {
            g.feast.lower_field();
        }
        // Pests, the emergences (the ooze) and the zombies' forming are drawn from the server's copy, exactly like
        // local play (the zombies' places and the rounds come through the timeline).
        g.feast.pests = frame.pests.clone();
        g.feast.emergences = frame.emergences.clone();
        g.feast.enemies = frame.enemies.clone();
        // The yaw each turret has turned to; a frame without them leaves the turrets as the map has them.
        if frame.turrets.len() == g.feast.turret_yaw.len() {
            g.feast.turret_yaw = frame.turrets.clone();
        }
        crate::loose::apply(world, &mut g.feast, frame);
        let Some(me) = frame.players.iter().find(|seat| seat.player.id == g.network_id).map(|seat| seat.player.clone()) else {
            return;
        };
        let round = frame.round;
        if round != g.network_round {
            clear_round(world, &mut g);
            g.network_round = round;
            // The gun the server hands out at the start: the mustard pistol in a siege, the syrup rifle in Forts.
            let gun = Weapon::index(me.gun.max(1));
            (g.weapon, g.gun, g.volley) = (gun, gun, 0);
            // Forts: the forcefield is up again and everything is back where the map has it; the hotdog
            // looks the way its start faces.
            if g.forts() {
                g.feast.reset();
                crate::loose::apply(world, &mut g.feast, frame);
                if g.phase != Phase::Dormant {
                    g.yaw = me.yaw;
                    g.pitch = 0.;
                }
            }
        }
        // Multiplayer's lobby (see lobby.rs): while this player is held in the fridge the arena sleeps, with
        // nothing of the round left on the screen. Let out, the hotdog begins where it starts.
        if world.get_resource::<crate::lobby::Lobby>().is_some_and(|lobby| lobby.holding) {
            if g.phase != Phase::Dormant {
                clear_round(world, &mut g);
                g.phase = Phase::Dormant;
                for mut cursor in world.query::<&mut CursorOptions>().iter_mut(world) {
                    cursor.grab_mode = CursorGrabMode::None;
                    cursor.visible = true;
                }
                update_hud(world, &g);
            }
            return;
        }
        if g.phase == Phase::Dormant {
            (g.position, g.yaw) = g.start();
            g.pitch = 0.;
            g.phase_time = 0.;
        }
        let was = g.infected;
        let previous_phase = g.phase;
        g.set_network_state(frame.elapsed, me.infected, me.score, me.kills, me.ammo, me.owned);
        g.phase = match frame.stage.as_str() {
            "FreeForAll" => Phase::Famine,
            "RoundOver" if g.fort.is_some() => Phase::Rest,
            "RoundOver" => Phase::Over,
            "Wave" => Phase::Siege,
            "Warmup" => Phase::Warmup,
            "Build" => Phase::Build,
            "Fight" => Phase::Fight,
            _ => g.phase,
        };
        if g.phase != previous_phase {
            crate::maya::publish_stage(if g.phase == Phase::Famine {
                13
            } else if g.phase == Phase::Siege {
                10
            } else if g.phase == Phase::Over {
                12
            } else {
                9
            });
        }
        if g.infected && !was {
            g.scoped = false;
            g.notify("INFECTED | one bite | everyone sees you GREEN");
            crate::maya::publish_stage(11);
        }
        // Where everything else was, for the timeline; then this player's own body.
        let hz = g.online.hz;
        g.online.timeline.push(online::moment(frame), now, hz);
        mirror(&mut g);
        // (The server's clock at the last command it applied: Kung Fu's prediction foresees each command's step from it.)
        (g.online.clock, g.online.heard) = ((frame.ack, frame.elapsed), now);
        online::settle(&mut g, frame.ack, frame.mine.filter(|_| me.alive));
        if g.ghosts.get(g.network_id).is_none() {
            g.position = Vec3::from_array(me.p);
        }
        let v = world.resource::<Visuals>().clone();
        // The cats there are (where they are drawn is online.rs `show`'s).
        let ids: Vec<u32> = frame.enemies.iter().map(|e| e.id).collect();
        let mut i = 0;
        while i < g.enemies.len() {
            if !ids.contains(&g.enemies[i].state.id) {
                let e = g.enemies.swap_remove(i);
                crate::cats::kill(world, e.entity);
            } else {
                i += 1;
            }
        }
        for z in &frame.enemies {
            let state = EnemyState { id: z.id, p: z.p, yaw: z.yaw, hp: z.hp, born: z.born, forming: z.forming };
            if let Some(e) = g.enemies.iter_mut().find(|e| e.state.id == state.id) {
                e.state = state;
            } else {
                let entity = cat(world, &v, Vec3::from_array(state.p), state.id);
                g.enemies.push(Enemy { state, entity });
            }
        }
        for e in &g.enemies {
            crate::cats::drive(world, e.entity, e.state.hp, e.state.forming);
        }
        for p in &frame.pickups {
            if let Some(existing) = g.pickups.get_mut(p.id) {
                existing.state = PickupState { id: p.id, p: p.p, weapon: p.weapon, ammo: p.ammo, ready: p.ready, respawn: p.respawn };
            }
        }
    });
}

#[allow(clippy::type_complexity)]
fn build_viewmodels(
    world: &mut World,
    v: &Visuals,
    camera: Entity,
) -> (Vec<(Weapon, Entity)>, (Entity, Entity), Vec<Entity>) {
    let mut result = vec![];
    let kit = world.resource::<crate::player_model::PlayerModels>().weapons.clone();
    // A first-person model for every gun; with the hands full (`Weapon::Laden`) there is none.
    for weapon in Weapon::ALL.into_iter().filter(|w| *w != Weapon::Laden) {
        let root = world
            .spawn((
                Name::new(format!("{} first person model", weapon.name())),
                Transform::from_xyz(0.045, -0.047, -0.105),
                Visibility::Hidden,
            ))
            .id();
        world.entity_mut(camera).add_child(root);
        crate::viewmodel::hands(world, root, weapon as usize);
        // The same model the hotdogs carry, a little larger.
        kit.attach(world, root, weapon as usize, crate::weapons::in_view(weapon as usize), Visibility::Inherited);
        result.push((weapon, root));
    }
    // The stim clap: a left forearm that rises across the bottom of the view and the right hand
    // that slaps it. `tick` shows and moves them for the half second the clap lasts.
    let skin = world
        .resource::<crate::player_model::PlayerModels>()
        .skin_material
        .clone();
    let arm = world
        .spawn((
            Name::new("Stim forearm"),
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    world.entity_mut(camera).add_child(arm);
    piece(world, arm, &v.capsule, &skin, Vec3::ZERO, Vec3::new(0.009, 0.03, 0.009));
    let hand = piece(
        world,
        arm,
        &v.sphere,
        &skin,
        Vec3::new(0.05, 0.004, 0.012),
        Vec3::new(0.0055, 0.015, 0.012),
    );
    // The stim's bolts: its fire round the edges of the view and its lightning over the hands and the gun.
    let bolts = crate::player_model::stim::build_view(world, camera);
    (result, (arm, hand), bolts)
}
/// A shot seen from the server. Beams (the turret) draw a trail; guns only bark, because their
/// rounds arrive as projectiles.
pub fn remote_shot(world: &mut World, a: Vec3, b: Vec3, beam: bool, weapon: usize) {
    let visuals = world.resource::<Visuals>().clone();
    world.resource_scope(|world, mut g: Mut<Game>| {
        if beam {
            trail(world, &mut g, &visuals, a, b);
            audio(world, visuals.pew.clone());
        } else {
            pew(world, &visuals, Weapon::index(weapon));
        }
    });
}

fn sync_local(world: &mut World, g: &mut Game, v: &Visuals) {
    let next = match g.feast.stage {
        Stage::Warmup => Phase::Warmup,
        Stage::Wave => Phase::Siege,
        Stage::FreeForAll => Phase::Famine,
        Stage::RoundOver if g.forts() => Phase::Rest,
        Stage::RoundOver => Phase::Over,
        Stage::Build => Phase::Build,
        Stage::Fight => Phase::Fight,
    };
    g.fort = g.feast.fort_state();
    // Forts: the simulation has begun the next round by itself. Everything of the last one goes, and the
    // hotdog looks the way its start faces.
    if g.feast.round != g.seen_round {
        g.seen_round = g.feast.round;
        clear_round(world, g);
        // The gun the simulation hands out at the start: the syrup rifle in Forts.
        let gun = Weapon::index(g.feast.player(1).map_or(1, |p| p.gun).max(1));
        (g.weapon, g.gun, g.volley) = (gun, gun, 0);
        if let Some(p) = g.feast.player(1) {
            (g.yaw, g.pitch) = (p.yaw, 0.);
        }
    }
    if next != g.phase {
        phase(world, g, next);
        // The wave's own ding is the microwave finishing: `ooze` sounds it with the emergence.
        if next != Phase::Siege {
            audio(world, v.ding.clone());
        }
    }
    g.elapsed = g.feast.elapsed;
    g.alert = g.feast.alert.clone();
    g.roster = g.feast.players.clone();
    g.online_count = g.roster.len();
    if let Some(p) = g.feast.player(1) {
        g.score = p.score;
        g.kills = p.kills;
        g.ammo = p.ammo;
        g.owned = p.owned;
        // The simulation owns the body: it has just jumped, fallen, been pushed or thrown.
        g.position = Vec3::from_array(p.p);
    }
    let states = g.feast.enemies.clone();
    let ids: Vec<u32> = states.iter().map(|e| e.id).collect();
    let mut index = 0;
    while index < g.enemies.len() {
        if !ids.contains(&g.enemies[index].state.id) {
            let e = g.enemies.swap_remove(index);
            crate::cats::kill(world, e.entity);
        } else {
            index += 1;
        }
    }
    for e in states {
        let state = EnemyState {
            id: e.id,
            p: e.p,
            yaw: e.yaw,
            hp: e.hp,
            born: e.born,
            forming: e.forming,
        };
        if let Some(old) = g.enemies.iter_mut().find(|old| old.state.id == state.id) {
            old.state = state;
        } else {
            let entity = cat(world, v, Vec3::from_array(state.p), state.id);
            g.enemies.push(Enemy { state, entity });
        }
    }
    for e in &g.enemies {
        if let Some(mut tr) = world.get_mut::<Transform>(e.entity) {
            tr.translation = Vec3::from_array(e.state.p);
            tr.rotation = Quat::from_rotation_y(e.state.yaw);
        }
        crate::cats::drive(world, e.entity, e.state.hp, e.state.forming);
    }
    for item in &g.feast.pickups {
        if let Some(p) = g.pickups.get_mut(item.id) {
            (p.state.ready, p.state.respawn) = (item.ready, item.respawn);
        }
    }
    let shots = g.feast.shots.clone();
    for shot in shots {
        if shot.id > g.last_feast_shot {
            g.last_feast_shot = shot.id;
            // Beams (the turret) draw a trail; guns bark and kick the hotdog that fired.
            if shot.projectile == 0 {
                trail(
                    world,
                    g,
                    v,
                    Vec3::from_array(shot.a),
                    Vec3::from_array(shot.b),
                );
            }
            if shot.owner != 1 || g.demo {
                if shot.projectile == 0 {
                    audio(world, v.pew.clone());
                } else {
                    pew(world, v, Weapon::index(shot.weapon));
                }
            }
            if let Some(actor) = g.actors.get(&shot.owner) {
                crate::player_model::kick(world, *actor, shot.weapon);
            }
        }
    }
}
/// Who carries which napkin this frame, and where its two grips are: for this player in first person two
/// points low in the view (nearer when a wall is close in front), for everybody else their hotdog's hands
/// (kitchen_sim's body held out as `player_model::HANDS` has it). Also every living hotdog's body, for the
/// sheets to bump into, and the loose object this player carries in first person, which `loose` hides in
/// the world (its first-person model is drawn instead, or for a napkin the sheet hanging from the view).
/// Online, the object this player carries is put where its hands are now (the server's word is a moment old).
fn carriers(world: &mut World, g: &mut Game, dt: f32, _now: f32) -> (Vec<crate::loose::Hold>, Vec<crate::cloth::Body>, Option<usize>) {
    let me = g.me();
    // This player's eye, looking where it looks (the game's camera without its recoil, which carrying has none of;
    // worked out here rather than read from the camera, which a check may have pinned elsewhere).
    let camera = g.camera.is_some().then(|| Transform::from_translation(g.position + Vec3::Y * g.eye).with_rotation(Quat::from_rotation_y(g.yaw) * Quat::from_rotation_x(g.pitch)));
    let shown: Vec<(u32, Entity)> = world
        .query_filtered::<(Entity, &crate::player_model::Gait, &Visibility), With<crate::player_model::PlayerBody>>()
        .iter(world)
        .filter(|(_, _, v)| **v != Visibility::Hidden)
        .map(|(e, gait, _)| (gait.id(), e))
        .collect();
    // A hotdog's body frame as drawn this frame: its root, and its torso on it.
    let torso = |world: &World, id: u32| -> Option<Transform> {
        let body = g.actors.get(&id).copied().filter(|_| !g.network).or_else(|| shown.iter().find(|(other, _)| *other == id).map(|(_, e)| *e))?;
        let (root, rig) = (world.get::<Transform>(body)?, world.get::<crate::player_model::Rig>(body)?);
        Some(root.mul_transform(*world.get::<Transform>(rig.joints().torso)?))
    };
    let (mut holds, mut bodies, mut mine) = (vec![], vec![], None);
    let hands = crate::player_model::HANDS;
    for p in &g.roster {
        if !p.alive {
            continue;
        }
        let own = p.id == me && !g.demo;
        let (at, yaw) = if own { (g.position, g.yaw) } else { (Vec3::from_array(p.p), p.yaw) };
        let height = if p.prone { 0.06 } else if p.crouch { 0.10 } else { 0.15 };
        bodies.push(crate::cloth::Body { feet: at, radius: kitchen_sim::feast::HOTDOG_RADIUS as f32, height });
        if p.carry < 0 || p.carry as usize >= g.feast.props.len() {
            continue;
        }
        let prop = p.carry as usize;
        if own {
            mine = Some(prop);
        }
        if g.feast.props.body(prop).shape != kitchen_sim::drape::NAPKIN {
            continue;
        }
        let grips = match (own, camera) {
            (true, Some(view)) => {
                // The hands come back towards the eye when a wall is close in front, and sway behind a turn
                // and with the stride.
                let ahead = (view.rotation * Vec3::NEG_Z).normalize_or(Vec3::NEG_Z);
                let room = g.feast.level.cover_from(view.translation, ahead);
                let nearer = (-crate::viewmodel::CARRY_GRIPS[0].z + 0.03 - room).clamp(0., 0.1);
                g.carry_shift = Vec3::new(g.sway.x, g.sway.z.sin() * 0.004 * g.sway.y, nearer);
                crate::viewmodel::CARRY_GRIPS.map(|grip| view.transform_point(grip + g.carry_shift))
            }
            _ => {
                let frame = torso(world, p.id).unwrap_or(Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)));
                [-1f32, 1.].map(|side| frame.transform_point(Vec3::new(side * hands.x, hands.y, hands.z)))
            }
        };
        holds.push(crate::loose::Hold { prop, grips });
    }
    // Online the server's place for what this player carries is a moment old: it is put in its hands now, as the
    // simulation holds it (`Feast::haul`: nearer where a wall is in the way, turned and tipped as this player holds it).
    if let Some(id) = mine.filter(|id| g.network && g.feast.props.state(*id) == kitchen_sim::props::State::Carried) {
        let (at, turn) = g.feast.holding(id, g.pose, g.position.as_dvec3().to_array(), g.yaw as f64);
        g.feast.props.carry(id, at, turn);
    }
    // How the view sways under what is carried: behind a turn, and with the stride.
    let turn = (g.yaw - g.sway_yaw + PI).rem_euclid(TAU) - PI;
    g.sway_yaw = g.yaw;
    let step = g.position - g.sway_at;
    g.sway_at = g.position;
    let speed = if dt > 1e-4 { Vec2::new(step.x, step.z).length() / dt } else { 0. };
    let ease = 1. - (-dt * 8.).exp();
    // The stride's bob (`sway.y` how deep, `sway.z` where in its cycle): as deep at a walk as it was at the walk the
    // hotdogs once had (actor::FASTER), and quicker the faster it goes (10 a second at 0.57 m/s), so it keeps step.
    let stride = (g.sway.z + dt * 10. * speed.min(3.) / actor::WALK_BEFORE as f32).rem_euclid(TAU);
    g.sway = g.sway.lerp(Vec3::new((turn / dt.max(1e-3) * 0.004).clamp(-0.02, 0.02), (speed / actor::FASTER as f32).min(1.), 0.), ease);
    g.sway.z = stride;
    (holds, bodies, mine)
}
fn update_players(world: &mut World, g: &mut Game, time: f32) {
    if g.network {
        return;
    }
    let models = world
        .resource::<crate::player_model::PlayerModels>()
        .clone();
    let spectator = g.demo || g.roster.first().is_some_and(|p| !p.alive);
    // Kung Fu over the shoulder: this player's own body is seen (fight.rs).
    let third = world.get_resource::<crate::fight::Camera>().is_some_and(|camera| crate::fight::over_shoulder(g, camera, g.demo));
    let table = crate::cards::at_table(g);
    for player in &g.roster {
        let e = *g
            .actors
            .entry(player.id)
            .or_insert_with(|| models.spawn(world, Vec3::from_array(player.p), player.id));
        // The last hotdog standing throws its arms open.
        let cues = crate::player_model::Cues {
            alive: player.alive,
            cheer: (g.phase == Phase::Over || (g.phase == Phase::Rest && g.fort.as_ref().is_some_and(|fort| fort.winner == Some(player.team as i32)))) && player.alive,
            prone: player.prone,
            crouch: player.crouch,
            stim: player.stim,
            weapon: player.weapon,
            carry: player.carry >= 0,
            pitch: player.pitch,
            swings: player.swings,
            aimed: player.aimed,
            airborne: !player.grounded,
            launched: player.launched,
            velocity: Vec3::from_array(player.v),
            reload: crate::reload::progress(g, player.id),
            kungfu: g.kung_fu(),
        };
        // At the card table a hotdog holds no gun, turns to whoever's go it is and cheers only its own luck (cards.rs).
        let (yaw, cues) = crate::cards::seated(world, table, player.id, player.yaw, cues);
        let p = Vec3::from_array(player.p);
        if let Some(mut tr) = world.get_mut::<Transform>(e) {
            tr.translation = p + if player.alive {
                Vec3::Y * (time * 4. + player.id as f32).sin() * 0.001
            } else {
                Vec3::Y * 0.025
            };
            tr.rotation = Quat::from_rotation_y(yaw)
                * if player.alive {
                    Quat::IDENTITY
                } else {
                    Quat::from_rotation_z(1.57)
                };
        }
        world
            .entity_mut(e)
            .insert(if !player.consumed && (player.id != 1 || spectator || third) && !crate::titan::drawn_as_titan(g, player.id) {
                Visibility::Visible
            } else {
                Visibility::Hidden
            });
        crate::player_model::animate(world, e, time, cues);
    }
}

#[derive(Component)]
pub(crate) struct FoodAlert;
#[derive(Component)]
struct FoodAlertText;
/// Each hotdog's health is a tag over its head (tags.rs), and the player's own is in the head-up display (hud.rs). The
/// alert's banner is the hints' (hints.rs).
fn build_survival_hud(commands: &mut Commands) {
    commands
        .spawn((
            FoodAlert,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(12.),
                width: Val::Percent(76.),
                top: Val::Px(132.),
                padding: UiRect::all(Val::Px(14.)),
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(6.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.065, 0.025, 0.94)),
            Visibility::Hidden,
        ))
        .with_children(|panel| {
            panel.spawn((
                FoodAlertText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(17.),
                    ..default()
                },
                TextColor(Color::srgb(1., 0.74, 0.32)),
            ));
        });
}
fn update_survival_hud(world: &mut World, g: &Game) {
    // The alert is sentences (the simulation's): it is up only with the hints on, and in the demo, which is watched,
    // not played. Otherwise the head-up display says it in icons and a moment of a few words (hud.rs).
    let wordy = g.demo || world.get_resource::<crate::hints::Hints>().is_some_and(|h| h.on);
    // (Written only when they change: a text written is shaped again, and the whole display laid out again.)
    crate::queries::with::<&mut Visibility, With<FoodAlert>, _>(world, |world, q| {
        for mut vis in q.iter_mut(world) {
            // (Free For All shows its own line for the first seconds of a round, and the score after it; the sandbox none.)
            let rules = &g.feast.rules;
            let banner = !rules.active() || matches!(g.phase, Phase::Over | Phase::Rest) || (rules.ends() && g.elapsed - g.feast.famine_at < 10.);
            vis.set_if_neq(if wordy && banner && !crate::cards::at_table(g) && matches!(g.phase, Phase::Famine | Phase::Over | Phase::Build | Phase::Fight | Phase::Rest) {
                Visibility::Visible
            } else {
                Visibility::Hidden
            });
        }
    });
    crate::queries::with::<&mut Text, With<FoodAlertText>, _>(world, |world, q| {
        for mut text in q.iter_mut(world) {
            let line: &str = if g.phase == Phase::Famine && !g.feast.rules.active() {
                if g.demo && g.elapsed - g.feast.famine_at > 3. && g.elapsed - g.feast.famine_at < 7. {
                    "FRIENDLY FIRE! FRANKIE SHOT MUSTARD!\nONLY FOOD LEFT: HOTDOGS | FREE FOR ALL\nHP IS DROPPING. EAT EACH OTHER TO SURVIVE."
                } else {
                    "ONLY FOOD LEFT: HOTDOGS\nFREE FOR ALL | FRIENDLY FIRE ENABLED\nHP IS DROPPING. F TO BITE. E TO EAT."
                }
            } else {
                &g.alert
            };
            if text.0 != line {
                text.0 = line.to_string();
            }
        }
    });
}
/// For the tests: the game as `main` builds it, without a window's events or a renderer. The arena's `setup` (the
/// head-up display, the models, the game) for single player by `rules` (the page's address words) from the start, the
/// map's pickups on their pads, the game's camera and a focused window for `tick` to capture the pointer in. No
/// system of the frame is added (each test adds what it runs), and the clock has started: see `test_frame`.
/// For the tests: the map a round by `rules` starts on: the bundled counter, or for a mode played on a map of its own
/// alone (Kung Fu, Titan) that map, as offline a map that refuses the mode would play its own instead (`fit_rules`).
#[cfg(test)]
pub(crate) fn test_map(rules: &str) -> Map {
    use kitchen_sim::rules::Mode;
    let name = match (crate::runtime::Options { params: rules.into(), ..default() }).rules().mode() {
        Some(Mode::KungFu) => "kung-fu-dojo",
        Some(Mode::Titan) => "titans-kitchen",
        _ => return Map::bundled(),
    };
    Map::parse(&std::fs::read_to_string(format!("{}/assets/maps/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
}
#[cfg(test)]
pub(crate) fn test_app(rules: &str) -> App {
    let mut app = App::new();
    app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()));
    app.init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>().init_asset::<AudioSource>();
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<AccumulatedMouseScroll>()
        .insert_resource(crate::runtime::Options { skip_intro: true, params: rules.into(), ..default() })
        .insert_resource(crate::level::StartMap(test_map(rules)))
        .add_systems(Startup, setup);
    app.update();
    let world = app.world_mut();
    world.spawn((crate::maya::HotdogCamera, Transform::default(), Projection::Perspective(default()), Visibility::default()));
    world.spawn(Window { focused: true, ..default() });
    let v = world.resource::<Visuals>().clone();
    world.resource_scope(|world, mut g: Mut<Game>| {
        let pickups = spawn_pickups(world, &v, &g.feast);
        g.pickups = pickups;
    });
    world.resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::ZERO);
    app
}
/// For the tests: one frame, `dt` seconds after the last. What was pressed is no longer just pressed afterwards and
/// the mouse has stopped, as the input plugin has it.
#[cfg(test)]
pub(crate) fn test_frame(app: &mut App, dt: f32) {
    app.world_mut().resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::from_secs_f32(dt));
    app.update();
    let world = app.world_mut();
    world.resource_mut::<ButtonInput<KeyCode>>().clear();
    world.resource_mut::<ButtonInput<MouseButton>>().clear();
    world.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::ZERO;
    world.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::ZERO;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Offline, a mode the map refuses gives way to the map's own, every other rule kept, with the reason to say; a mode
    /// the map takes is played as asked. Every mode, every bundled map.
    #[test]
    fn offline_a_mode_the_map_refuses_gives_way_to_the_maps_own() {
        use kitchen_sim::rules::{Mode, Rules};
        let read = |name: &str| Map::parse(&std::fs::read_to_string(format!("{}/assets/maps/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
        let maps = [Map::bundled(), read("pillow-fort"), read("kung-fu-dojo"), read("titans-kitchen")];
        for map in &maps {
            for mode in Mode::ALL {
                let asked = Rules { sandbox: true, god: true, ..Rules::of(Some(mode)) };
                let (played, why) = fit_rules(&asked, map);
                assert!(played.sandbox && played.god, "the other rules are kept");
                match map.refuses(mode) {
                    None => assert_eq!((played, why), (asked.clone(), None), "{} as {}", map.name, mode.title()),
                    Some(reason) => {
                        let own = Mode::made_for(map);
                        assert!(map.refuses(own).is_none());
                        assert_eq!(played.mode().unwrap_or(own), own, "{} as {}: {:?}", map.name, mode.title(), played.mode());
                        let why = why.unwrap();
                        assert!(why.starts_with(&reason) && why.contains(own.title()) && why.contains(mode.title()), "{why}");
                    }
                }
            }
        }
        // Titan on the bundled counter: a siege, and why.
        let (played, why) = fit_rules(&Rules::parse("mode=titan titan=me"), &Map::bundled());
        assert!(!played.titan && played.mode().is_none_or(|m| m == Mode::Siege));
        assert_eq!(why.as_deref(), Some("Counter Siege has no Titan layout: playing Siege instead of Titan."));
    }

    /// `?play&map=counter-siege&mode=titan`: the round is a siege, the note is on the screen for a while and is said
    /// once, not again at the next round.
    #[test]
    fn offline_titan_on_a_counter_plays_the_siege_and_says_so_once() {
        let mut app = test_app("");
        let g = app.world_mut().resource_mut::<Game>().into_inner();
        // (As `tick` starts a round: the address's rules.)
        g.rules = crate::runtime::Options { params: "mode=titan".into(), ..default() }.rules();
        g.begin();
        assert!(g.titan.is_none() && g.feast.titan_id().is_none() && !g.rules.titan && g.rules.mode().is_none_or(|m| m == kitchen_sim::rules::Mode::Siege));
        let note = g.notice().map(str::to_string).expect("a note on the screen");
        assert!(note.contains("no Titan layout"), "{note}");
        g.notice_time = 0.;
        g.begin();
        assert!(g.notice().is_none(), "said once");
        // Natively, Titan with no map named plays Titan's Kitchen (and Forts Pillow Fort).
        use kitchen_sim::rules::Mode;
        assert_eq!((crate::level::own_map(Some(Mode::Titan)), crate::level::own_map(Some(Mode::Forts)), crate::level::own_map(Some(Mode::Ffa)), crate::level::own_map(None)), (Some("titans-kitchen"), Some("pillow-fort"), None, None));
        assert_eq!(crate::level::own_map(Some(Mode::KungFu)), Some("kung-fu-dojo"), "and Kung Fu its dojo");
        let dojo = crate::runtime::Options { params: "mode=kungfu".into(), ..default() };
        assert_eq!(crate::level::choose(&dojo).unwrap().name, "Kung Fu Dojo");
        let options = crate::runtime::Options { params: "mode=titan".into(), ..default() };
        assert_eq!(crate::level::choose(&options).unwrap().mode, kitchen_sim::titan::MODE);
        let named = crate::runtime::Options { params: "mode=titan".into(), map: format!("{}/assets/maps/counter-siege.json", env!("CARGO_MANIFEST_DIR")), ..default() };
        assert_eq!(crate::level::choose(&named).unwrap().name, "Counter Siege", "a map named is the one played");
    }
    /// Online the placement preview asks the client's copy of the round about this player's hotdog where its
    /// prediction has it (not where the copy last had it), holding what the server's frames say it holds, at the pace
    /// the client keeps, and gives the same answer as the round itself would; the copy is left as it was.
    #[test]
    fn online_the_placement_is_asked_about_the_predicted_hotdog_and_the_copy_is_left_as_it_was() {
        // (The Forts layout with the old stock the Forts tests play with.)
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/network/fixtures/long-fort.json")).unwrap();
        let map = Map::parse(&text).unwrap();
        let (feet, yaw) = ([-1.61, feast::FLOOR, 2.7], -std::f32::consts::FRAC_PI_2);
        // The round itself: hotdog 2 holds pillow 8 over pillow 7, standing still.
        let mut round = Feast::with_map(map.clone(), false);
        assert!(round.props.pick_up(8, 2));
        let p = round.player_mut(2).unwrap();
        (p.p, p.yaw, p.carry, p.pace, p.grounded) = (feet, yaw, 8, [0.; 2], true);
        let expected = round.placement(2).expect("it carries the pillow");
        // A client playing hotdog 2: its copy of the round has hotdog 2 where it started, empty-handed; the frames say
        // it carries the pillow; its prediction has it standing over pillow 7.
        let mut app = test_app("");
        let mut g = app.world_mut().resource_mut::<Game>();
        g.feast = Feast::with_map(map, false);
        g.begin();
        (g.network, g.network_id) = (true, 2);
        g.roster = g.feast.players.clone();
        g.roster.iter_mut().filter(|p| p.id == 2).for_each(|p| p.carry = 8);
        assert!(g.feast.props.pick_up(8, 2));
        let stale = g.feast.player(2).cloned().unwrap();
        assert_eq!(stale.carry, -1);
        let mut body = Actor { radius: feast::HOTDOG_RADIUS, ..Actor::hotdog(2, feet.map(|v| v as f64)) };
        body.grounded = true;
        g.ghosts.add(body);
        (g.position, g.yaw) = (Vec3::from_array(feet), yaw);
        let spot = g.placement([0.; 2]).expect("online too");
        assert_eq!((spot.p, spot.q, spot.onto, spot.lock), (expected.p, expected.q, Some(7), true));
        assert_eq!(g.feast.player(2).cloned().unwrap().p, stale.p, "the copy is as it was");
        assert_eq!(g.feast.player(2).unwrap().carry, -1);
        assert_eq!(g.feast.players.len(), round.players.len());
        // Held turned and tipped (the wheel and X): the client's own pose, as the round has it with the same.
        let pose = Pose { turn: 2, tip: 1 };
        round.set_held_pose(2, pose);
        let posed = round.placement(2).unwrap();
        g.pose = pose;
        let spot = g.placement([0.; 2]).unwrap();
        assert_eq!((spot.p, spot.q), (posed.p, posed.q));
        assert_eq!(spot.q, pose.rotation(spot.yaw));
        g.pose = Pose::default();
        // Walking (the pace the client keeps): let go with it, to lock where it comes to rest.
        let walking = g.placement([0.5, 0.]).unwrap();
        assert_eq!((walking.velocity, walking.lock), ([0.5, 0., 0.], false));
        // Empty-handed by the frames: nothing.
        g.roster.iter_mut().filter(|p| p.id == 2).for_each(|p| p.carry = -1);
        assert_eq!(g.placement([0.; 2]), None);
    }
    #[test]
    fn the_strike_is_over_before_the_simulation_lets_the_next_one_start() {
        // The strike lasts as long as the simulation makes the hotdog wait; how it looks is whip.rs's.
        assert_eq!(MELEE_SWING, feast::DELAY[0]);
        assert!(crate::whip::TOTAL + crate::whip::STOP <= MELEE_SWING);
        assert_eq!((Weapon::index(0), Weapon::index(6), Weapon::index(7), Weapon::index(9)), (Weapon::Laden, Weapon::Rifle, Weapon::Mayo, Weapon::Mayo));
        assert!(Weapon::ALL.iter().all(|w| !w.name().contains("KNIFE") && !w.name().contains("BARE")));
    }
    #[test]
    fn the_two_slots_are_the_primary_and_the_secondary_and_bare_hands_are_neither() {
        let owning = |guns: &[usize]| {
            let mut owned = [false; feast::GUNS];
            for w in guns {
                owned[*w] = true;
            }
            slots(&owned)
        };
        // Every hotdog starts with the syrup rifle as the primary and the pistol as the secondary.
        assert_eq!(owning(&[1, 6]), [Some(Weapon::Rifle), Some(Weapon::Pistol)]);
        // The mayo squirter is a primary: beside the pistol it is key 1, and alone it is in slot 1 too.
        assert_eq!((owning(&[1, 7]), owning(&[7]), owning(&[6, 7])), ([Some(Weapon::Mayo), Some(Weapon::Pistol)], [Some(Weapon::Mayo), None], [Some(Weapon::Rifle), Some(Weapon::Mayo)]));
        assert_eq!(owning(&[1]), [None, Some(Weapon::Pistol)]);
        assert_eq!(owning(&[2]), [Some(Weapon::Sniper), None]);
        assert_eq!(owning(&[4, 2]), [Some(Weapon::Sniper), Some(Weapon::Bazooka)]);
        assert_eq!(owning(&[5, 1]), [Some(Weapon::Pistol), Some(Weapon::Spray)]);
        // The hands-full state is not a gun: owning it gives neither slot.
        assert_eq!(owning(&[0]), [None, None]);
        assert_eq!(owning(&[0, 3]), [Some(Weapon::Flame), None]);
    }
    #[test]
    fn the_spectator_frames_the_bundled_counter_as_before_and_any_other_the_same_way() {
        for angle in [0., 0.7, 2.9] {
            let (eye, target) = spectator(Map::bundled().bounds, angle);
            assert!(eye.distance(Vec3::new(angle.sin() * 0.95, 1.43, 1.95 + angle.cos() * 0.12)) < 1e-5);
            assert!(target.distance(Vec3::new(0.1, 1.025, 3.02)) < 1e-5);
        }
        // A counter twice the size somewhere else is seen from twice as far, about its own middle.
        let (near, at) = spectator(Map::bundled().bounds, 0.4);
        let (eye, target) = spectator([10., 0., 17.2, 2.72], 0.4);
        let (middle, other) = (Vec3::new(0., FLOOR, 2.95), Vec3::new(13.6, FLOOR, 1.36));
        assert!((eye - other).distance((near - middle) * 2.) < 1e-4 && (target - other).distance((at - middle) * 2.) < 1e-4);
    }
    #[test]
    fn relish_and_hot_sauce_recoil_is_massive() {
        // (Against the syrup rifle's kick, the tight one; the mustard pistol is a hand cannon now: see below.)
        let rifle = recoil_pose(Weapon::Rifle, 1., 0.);
        let bomb = recoil_pose(Weapon::Bazooka, 1., 0.);
        assert!(rifle.0.z > 0.002 && rifle.1.angle_between(Quat::IDENTITY) > 0.03 && rifle.2 > 0.002, "every gun kicks");
        assert!(bomb.0.z > 4. * rifle.0.z && bomb.2 > 8. * rifle.2);
        assert!(bomb.1.angle_between(Quat::IDENTITY) > 3. * rifle.1.angle_between(Quat::IDENTITY));
        // The stream never settles: refreshed every 0.1 s, it keeps most of its push-back and shakes.
        let held = (-0.1 * Weapon::Flame.recoil()[5]).exp();
        assert!(held > 0.5 && Weapon::Flame.recoil()[0] * held > rifle.0.z);
        let shake = (0..40)
            .map(|i| recoil_pose(Weapon::Flame, 1., i as f32 * 0.013).1.angle_between(Quat::IDENTITY))
            .fold((f32::MAX, f32::MIN), |(lo, hi), a| (lo.min(a), hi.max(a)));
        assert!(shake.1 - shake.0 > 0.08, "the hot sauce stream shakes in the hands");
        for weapon in Weapon::ALL {
            let still = recoil_pose(weapon, 0., 1.3);
            assert!(still.0.length() < 1e-6 && still.2.abs() < 1e-6 && still.3.length() < 1e-6, "recoil recovers fully");
        }
    }
    #[test]
    fn the_syrup_rifle_kicks_tight_and_settles_quickly_and_aimed_down_its_sights_kicks_half_as_hard() {
        let [back, climb, roll, _, pitch, recovery, _] = Weapon::Rifle.recoil();
        // 60% of the kick it had (0.010 m, 0.16 rad, 0.03 rad, 0.008 rad), settling at 16 a second rather than 9.
        assert!((back - 0.006).abs() < 1e-6 && (climb - 0.096).abs() < 1e-6 && (roll - 0.018).abs() < 1e-6 && (pitch - 0.0048).abs() < 1e-6);
        assert!(recovery >= 16.);
        // A burst: a round every VOLLEY_GAP (0.1 s), each adding 0.75 to what is left (as `tick` does); 0.20 s after the
        // last round, as the next burst may come, next to nothing is left.
        let mut kick = 1.;
        for _ in 0..2 {
            kick = kick * (-feast::VOLLEY_GAP * recovery).exp() + 0.75;
        }
        assert!(kick < 1.4, "the kicks build up only a little through a burst: {kick}");
        let pause = feast::DELAY[feast::RIFLE] - 2. * feast::VOLLEY_GAP;
        assert!(kick * (-pause * recovery).exp() < 0.012, "settled by the next burst, {pause} s on");
        // The kick is a pure function of how hard it lands: aimed down the sights, half of it at the hip.
        assert_eq!((steadiness(false, false, false, 0.), steadiness(false, false, false, 1.)), (1., 0.5));
        assert_eq!((steadiness(true, false, false, 0.), steadiness(false, true, false, 0.), steadiness(false, false, true, 1.)), (0.5, 0.75, 0.2));
        for now in [0., 0.3, 1.7] {
            let hip = recoil_pose(Weapon::Rifle, steadiness(false, false, false, 0.), now);
            let aimed = recoil_pose(Weapon::Rifle, steadiness(false, false, false, 1.), now);
            assert!(aimed.0.distance(hip.0 * 0.5) < 1e-7 && (aimed.2 - hip.2 * 0.5).abs() < 1e-7, "{hip:?} {aimed:?}");
            let (turn_hip, turn_aimed) = (hip.1.angle_between(Quat::IDENTITY), aimed.1.angle_between(Quat::IDENTITY));
            assert!(turn_aimed < 0.55 * turn_hip && turn_aimed > 0.45 * turn_hip, "{turn_aimed} {turn_hip}");
            assert!(aimed.0.length() < hip.0.length() && aimed.2 < hip.2);
        }
    }
    #[test]
    fn the_ketchup_sniper_blows_back_hard_slamming_towards_the_eye_with_a_short_shake_and_settles_in_about_0_4_s() {
        let sniper = recoil_pose(Weapon::Sniper, 1., 0.);
        // As it was: 0.034 m back, a 0.035 rad kick of the view, no shake.
        let (was_back, was_pitch) = (0.034, 0.035);
        assert!(sniper.2 >= 2.9 * was_pitch, "the view is thrown up about three times as far: {}", sniper.2);
        assert!(sniper.0.z > 1.7 * was_back, "the gun slams back towards the eye: {}", sniper.0.z);
        assert!(sniper.1.angle_between(Quat::IDENTITY) > 0.5);
        assert!(sniper.2 > 4. * recoil_pose(Weapon::Rifle, 1., 0.).2 && sniper.0.z > recoil_pose(Weapon::Rifle, 1., 0.).0.z * 3.);
        // A short shake of the camera: there at the shot, gone with the square of the kick, so before the kick is.
        let shake = |kick: f32| (0..40).map(|i| recoil_pose(Weapon::Sniper, kick, i as f32 * 0.011).3.length()).fold(0f32, f32::max);
        assert!(shake(1.) > 0.012, "{}", shake(1.));
        let recovery = Weapon::Sniper.recoil()[5];
        let left = |t: f32| (-t * recovery).exp();
        assert!(shake(left(0.2)) < 0.1 * shake(1.), "the shake is over in about 0.2 s");
        assert!(left(0.4) < 0.06 && left(0.25) > 0.1, "the gun settles over about 0.4 s");
        // The other guns' cameras do not shake, but for the relish bazooka's, heavier still, and the mustard pistol's, less.
        for weapon in Weapon::ALL.into_iter().filter(|w| !matches!(w, Weapon::Sniper | Weapon::Bazooka | Weapon::Pistol)) {
            assert_eq!(weapon.recoil()[6], 0., "{weapon:?}");
        }
        assert!(Weapon::Bazooka.recoil()[6] > Weapon::Sniper.recoil()[6] && Weapon::Sniper.recoil()[6] > Weapon::Pistol.recoil()[6]);
    }
    /// Every gun kicks by its weight (kitchen-sim recoil.rs WEIGHT): of the guns that kick a shot, the heavier kicks the
    /// gun back, up and the view up further and settles slower, from the mustard pistol (lightest) through the syrup
    /// rifle and the ketchup sniper (whose kicks are as they were tuned: the references) to the relish bazooka; the
    /// spray guns push and shake by their weight, the hot sauce thrower's as it was.
    #[test]
    fn every_gun_kicks_by_its_weight_heavier_kicks_harder_and_settles_slower() {
        use kitchen_sim::recoil::WEIGHT;
        // (The mustard pistol aside: a hand cannon, its kick is set as it is: `the_mustard_pistol_kicks_like_a_hand_cannon`.)
        let shots = [Weapon::Rifle, Weapon::Sniper, Weapon::Bazooka];
        for pair in shots.windows(2) {
            let (a, b) = (pair[0].recoil(), pair[1].recoil());
            assert!(WEIGHT[pair[0] as usize] < WEIGHT[pair[1] as usize], "{pair:?}");
            assert!(a[0] < b[0] && a[1] < b[1] && a[4] < b[4] && a[5] > b[5], "{pair:?}: {a:?} {b:?}");
            let (pa, pb) = (recoil_pose(pair[0], 1., 0.), recoil_pose(pair[1], 1., 0.));
            assert!(pa.0.z < pb.0.z && pa.2 < pb.2, "{pair:?}");
        }
        assert_eq!((Weapon::Rifle.recoil(), Weapon::Sniper.recoil(), Weapon::Flame.recoil()[..4].to_vec()), (RIFLE_KICK, SNIPER_KICK, STREAM[..4].to_vec()), "the references as they were tuned");
        let sprays = [Weapon::Mayo, Weapon::Spray, Weapon::Flame];
        for pair in sprays.windows(2) {
            let (a, b) = (pair[0].recoil(), pair[1].recoil());
            assert!(a[0] < b[0] && a[3] < b[3] && a[4] < b[4] && a[4] > 0., "{pair:?}");
        }
    }
    /// The mustard pistol is a hand cannon: light, but it kicks back, flips up and throws the view up far more than the
    /// syrup rifle (more than half what the ketchup sniper does), comes back down slowly (a third left when it can fire
    /// again, next to nothing by the time the simulation's aim has settled), and a shot fired before then stacks on what
    /// is left, up to twice a shot.
    #[test]
    fn the_mustard_pistol_kicks_like_a_hand_cannon_slow_to_come_back_down_and_stacking() {
        let (pistol, rifle, sniper) = (recoil_pose(Weapon::Pistol, 1., 0.), recoil_pose(Weapon::Rifle, 1., 0.), recoil_pose(Weapon::Sniper, 1., 0.));
        let flip = |pose: &(Vec3, Quat, f32, Vec2)| pose.1.angle_between(Quat::IDENTITY);
        assert!(pistol.0.z > 5. * rifle.0.z && flip(&pistol) > 4. * flip(&rifle) && pistol.2 > 5. * rifle.2, "{pistol:?}");
        assert!(pistol.0.z > 0.5 * sniper.0.z && flip(&pistol) > 0.5 * flip(&sniper) && pistol.2 < sniper.2);
        assert!(pistol.2 >= kitchen_sim::recoil::HAND_CANNON, "the view goes up at least as far as the next round would");
        let [.., recovery, _] = Weapon::Pistol.recoil();
        let left = |t: f32| (-t * recovery).exp();
        let (again, settled) = (feast::DELAY[1], kitchen_sim::recoil::SETTLE[1]);
        assert!(left(again) > 0.25 && left(again) < 0.4 && left(settled) < 0.07, "{} {}", left(again), left(settled));
        assert_eq!(Weapon::Pistol.delay(), feast::DELAY[1], "the trigger's own pace is the simulation's");
        // Fired as fast as it goes: each shot lands on what is left of the last, up to PISTOL_STACK.
        let mut kick = 0f32;
        let mut kicks = vec![];
        for _ in 0..8 {
            kick = (kick + 1.).min(PISTOL_STACK);
            kicks.push(kick);
            kick *= left(again);
        }
        assert!(kicks[1] > 1.25 && kicks.windows(2).all(|k| k[1] >= k[0]) && kicks.iter().all(|k| *k <= PISTOL_STACK), "{kicks:?}");
    }
    /// The kick is shown through a spring: it rises with the shot, comes back past where the gun rests a little, and
    /// settles; the view kicks to a side drawn afresh each shot.
    #[test]
    fn the_kick_springs_back_and_settles_and_the_view_kicks_to_a_side() {
        let mut spring = Spring::default();
        let (mut kick, mut most, mut least, dt) = (1f32, 0f32, 0f32, 1. / 120.);
        for _ in 0..240 {
            kick *= (-dt * Weapon::Rifle.recoil()[5]).exp();
            spring.follow(kick, dt);
            (most, least) = (most.max(spring.shown), least.min(spring.shown));
        }
        assert!(most > 0.3 && least < -0.001 && least > -0.2, "it springs back past rest: {most} {least}");
        assert!(spring.shown.abs() < 1e-3, "and settles: {}", spring.shown);
        let mut sides = vec![];
        for _ in 0..20 {
            spring.shot();
            sides.push(spring.side);
        }
        assert!(sides.iter().any(|s| *s > 0.) && sides.iter().any(|s| *s < 0.) && sides.iter().all(|s| s.abs() >= 0.35 && s.abs() <= 1.01), "{sides:?}");
        assert!((spring.yaw(0.1).abs() - 0.033 * spring.side.abs()).abs() < 1e-6);
    }
    /// A frame that spans a long while moves the kick's spring towards where it is going and never throws it out: the
    /// first frame after the tab was hidden or the window out of sight (another workspace) spans all of the time away,
    /// and that frame, integrated whole in 32 steps each far too long to be stable, grew the spring without bound, to
    /// infinity and then NaN, which never left it. From a fresh kick, from half a second after one and from what is left
    /// twenty seconds later (a residue far under anything shown, which never quite comes to nothing).
    #[test]
    fn a_long_frame_never_throws_the_kick_spring_out() {
        let rate = Weapon::Rifle.recoil()[5];
        for after in [0, 30, 1200] {
            for away in [0.3, 1., 1.5, 2., 5., 12., 70., 600., 3600.] {
                let (mut spring, mut kick) = (Spring::default(), 1f32);
                spring.shot();
                for _ in 0..6 + after {
                    kick *= (-rate / 60.).exp();
                    spring.follow(kick, 1. / 60.);
                }
                kick *= (-away * rate).exp();
                spring.follow(kick, away);
                assert!(spring.shown.is_finite() && spring.rate.is_finite(), "{after} frames after the kick, {away} s away: {spring:?}");
                assert!(spring.shown.abs() <= 1., "{after} frames after the kick, {away} s away: no kick the gun gave: {spring:?}");
                for _ in 0..90 {
                    spring.follow(0., 1. / 60.);
                }
                assert!(spring.shown.abs() < 1e-3, "{after} frames after the kick, {away} s away: settles: {spring:?}");
            }
        }
    }
    /// After a shot, the frame that spans the time the game was not drawn leaves the game's camera finite, and once the
    /// kick has settled where it would be had the gun never been fired. (The spring put NaN in the camera's place and turn
    /// for good: the view's frustum then held everything, so every mesh was counted as drawn, and nothing of the scene
    /// was, only the head-up display over the clear colour.)
    #[test]
    fn a_long_frame_after_a_shot_leaves_the_camera_finite_and_settled() {
        let run = |fired: bool, away: f32| {
            let mut app = test_app("");
            app.add_systems(Update, tick);
            for _ in 0..3 {
                test_frame(&mut app, 0.05);
            }
            if fired {
                // As `shoot` kicks the view.
                let mut g = app.world_mut().resource_mut::<Game>();
                g.kick = g.feast.tune.recoil[Weapon::Rifle as usize].max(0.6);
                g.spring.shot();
            }
            for _ in 0..6 {
                test_frame(&mut app, 1. / 60.);
            }
            test_frame(&mut app, away);
            let camera = |app: &mut App| *app.world_mut().query_filtered::<&Transform, With<crate::maya::HotdogCamera>>().single(app.world()).unwrap();
            let back = camera(&mut app);
            for _ in 0..90 {
                test_frame(&mut app, 1. / 60.);
            }
            (back, camera(&mut app))
        };
        let (_, before) = run(false, 1. / 60.);
        assert!(before.is_finite());
        for away in [2., 12., 70.] {
            let (back, settled) = run(true, away);
            assert!(back.is_finite() && settled.is_finite(), "{away} s away: the camera {back:?}, then {settled:?}");
            let (_, calm) = run(false, away);
            assert!(
                settled.rotation.angle_between(calm.rotation) < 1e-3 && settled.translation.distance(calm.translation) < 1e-3,
                "{away} s away: {settled:?} where a game that never fired has {calm:?}"
            );
        }
    }
    #[test]
    fn aimed_the_syrup_rifle_puts_its_optic_on_the_view_axis_its_glass_half_the_view_high() {
        let view = crate::weapons::in_view(Weapon::Rifle as usize);
        assert_eq!(rifle_hold(0.), GRIP);
        let aimed = rifle_hold(1.);
        let eyepiece = aimed + view.transform_point(crate::weapons::RIFLE_SIGHT);
        let relief = crate::scope::eye_relief();
        assert!(eyepiece.distance(Vec3::NEG_Z * relief) < 1e-6, "the eyepiece {relief} m dead ahead of the eye: {eyepiece}");
        // Seen through the ring's bore the glass is half the view high, and the ring round it fills the middle.
        let high = |r: f32| r / relief / (crate::viewmodel::FOV * 0.5).tan();
        assert!((high(crate::weapons::RIFLE_LENS) - crate::scope::LENS_SHARE).abs() < 1e-4);
        assert!(high(crate::weapons::RIFLE_RING) > 0.7 && high(crate::weapons::RIFLE_RING) < 0.9, "{}", high(crate::weapons::RIFLE_RING));
        // On the way up it is centred and raised: from the hip to the middle of the view.
        let half = rifle_hold(0.5);
        assert!(half.x < GRIP.x && half.x > aimed.x && half.y > GRIP.y && half.y < aimed.y);
    }

    /// What this player has of the round after a second of play holding crouch and the trigger, the pointer captured
    /// and the mouse moving: whether the head-up display, the scope or the alert is up, how many first-person guns
    /// there are and whether one is in view, the camera and its field of view (the camera was parked before), whether
    /// the tags are up, whether it crouched, turned or fired, and whether any pickup was on show.
    #[derive(Debug, PartialEq)]
    struct Round {
        hud: bool,
        scope: bool,
        alert: bool,
        guns: usize,
        gun_in_view: bool,
        camera: Transform,
        fov: f32,
        tags: bool,
        crouch: bool,
        turned: bool,
        fired: bool,
        pickups: bool,
    }
    const PARKED: Transform = Transform::from_xyz(9., 9., 9.);
    fn camera(app: &mut App) -> Entity {
        app.world_mut().query_filtered::<Entity, With<crate::maya::HotdogCamera>>().single(app.world()).unwrap()
    }
    fn shown<C: Component>(app: &mut App) -> bool {
        app.world_mut().query_filtered::<&Visibility, With<C>>().iter(app.world()).any(|v| *v != Visibility::Hidden)
    }
    fn play_a_second(app: &mut App, each: impl Fn(&mut App)) -> Round {
        let cam = camera(app);
        *app.world_mut().get_mut::<Transform>(cam).unwrap() = PARKED;
        *app.world_mut().get_mut::<Projection>(cam).unwrap() = Projection::Perspective(PerspectiveProjection { fov: 0.7, ..default() });
        let yaw = app.world().resource::<Game>().yaw;
        let mut fired = false;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        for _ in 0..20 {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyC);
            app.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(30., 0.);
            each(app);
            test_frame(app, 0.05);
            fired |= app.world().resource::<Game>().pending_shot;
        }
        let g = app.world().resource::<Game>();
        let (guns, gun_in_view, tags, crouch, turned) = (g.viewmodels.len(), g.viewmodels.iter().any(|(w, _)| *w == g.weapon), crate::tags::up(g, false), g.crouch, g.yaw != yaw);
        let items: Vec<Entity> = g.pickups.iter().map(|p| p.entity).collect();
        let in_view = g.viewmodels.iter().filter(|(w, _)| *w == g.weapon).map(|(_, e)| *e).collect::<Vec<_>>();
        let gun_in_view = gun_in_view && in_view.iter().all(|e| app.world().get::<Visibility>(*e) == Some(&Visibility::Visible));
        let pickups = items.iter().any(|e| app.world().get::<Visibility>(*e) != Some(&Visibility::Hidden));
        let Projection::Perspective(lens) = app.world().get::<Projection>(cam).unwrap().clone() else { panic!("a perspective camera") };
        let mut round = Round {
            hud: false,
            scope: shown::<ScopeMask>(app),
            alert: shown::<FoodAlert>(app),
            guns,
            gun_in_view,
            camera: *app.world().get::<Transform>(cam).unwrap(),
            fov: lens.fov,
            tags,
            crouch,
            turned,
            fired,
            pickups,
        };
        let huds: Vec<bool> = app.world_mut().query::<(&Hud, &Visibility)>().iter(app.world()).map(|(_, v)| *v == Visibility::Visible).collect();
        // (Two lines are the round's own, the cut scene's and the crosshair; the rest is hud.rs's, in icons.)
        assert_eq!(huds.len(), 2);
        assert!(huds.iter().all(|up| *up == huds[0]), "the head-up display's lines are up or down together");
        round.hud = huds[0];
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release_all();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release_all();
        round
    }
    /// The round as it stands down at the card table: nothing of it on screen, the camera left to the table.
    fn stood_down(round: &Round) {
        let Round { hud, scope, alert, guns, gun_in_view, camera, fov, tags, crouch, turned, fired, pickups } = round;
        assert!(!hud && !scope && !alert, "{round:?}");
        assert_eq!((*guns, *gun_in_view), (0, false), "no first-person guns are made at the table");
        assert_eq!((*camera, *fov), (PARKED, 0.7), "the round leaves the camera alone");
        assert!(!tags && !crouch && !turned && !fired && !pickups, "{round:?}");
    }
    /// The round as it is played: all of it back.
    fn played(round: &Round, g: &Game) {
        let Round { hud, scope, alert, guns, gun_in_view, camera, fov, tags, crouch, turned, fired, pickups } = round;
        assert!(*hud && !scope && !alert, "{round:?}");
        assert!(*guns == Weapon::ALL.len() - 1 && *gun_in_view, "{round:?}");
        assert!(camera.translation.distance(g.position + Vec3::Y * g.eye) < 0.01 && (fov - 85_f32.to_radians()).abs() < 1e-5, "{round:?}");
        assert!(*tags && *crouch && *turned && *fired && *pickups, "{round:?}");
    }

    #[test]
    fn at_the_table_the_round_hud_guns_and_camera_stand_down() {
        // Single player by the page's `?mode=cards`.
        let mut app = test_app("mode=cards");
        app.add_systems(Update, tick);
        test_frame(&mut app, 0.05);
        assert!(crate::cards::at_table(app.world().resource::<Game>()));
        let round = play_a_second(&mut app, |_| {});
        stood_down(&round);
        // The hotdogs are still drawn, at their seats, and the table's game goes on.
        let g = app.world().resource::<Game>();
        assert_eq!(g.actors.len(), 5);
        let seats = feast::table_seats(&g.feast.level, 5);
        for (p, (at, _)) in g.roster.iter().zip(&seats) {
            assert!(Vec3::from_array(p.p).distance(Vec3::from_array(*at)) < 1e-3, "{} stands at its seat", p.name);
        }
        assert!(g.feast.cards.as_ref().unwrap().version() > 0, "the table is being played");
        // The game won: the winner's alert is the table's to show, not the round's.
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.feast.stage = Stage::RoundOver;
            g.feast.alert = "RELISH IS FULLY LOADED: 104 POINTS".into();
        }
        test_frame(&mut app, 0.05);
        assert_eq!(app.world().resource::<Game>().phase, Phase::Over);
        assert!(crate::cards::at_table(app.world().resource::<Game>()));
        assert!(!shown::<FoodAlert>(&mut app) && !shown::<Hud>(&mut app));
        // And a sandbox played as Fully Loaded is at the table too: no sandbox lines, no guns.
        let mut app = test_app("sandbox mode=cards weapons=all ammo=inf");
        app.add_systems(Update, tick);
        test_frame(&mut app, 0.05);
        assert!(app.world().resource::<Game>().feast.rules.sandbox);
        stood_down(&play_a_second(&mut app, |_| {}));
        // A siege for comparison: everything is there.
        let mut app = test_app("");
        app.add_systems(Update, tick);
        test_frame(&mut app, 0.05);
        let round = play_a_second(&mut app, |_| {});
        played(&round, app.world().resource::<Game>());
    }

    #[test]
    fn back_in_a_siege_they_come_back() {
        // Online: the server's frames say what is played. A game of Fully Loaded first, then (after the fridge) a siege.
        let mut app = test_app("");
        app.add_systems(Update, tick);
        app.world_mut().insert_resource(crate::lobby::Lobby::default());
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            (g.network, g.network_id) = (true, 1);
        }
        let frame = |round: u32, rules: &str, tick: u32| {
            let mut feast = Feast::with_map(Map::bundled(), false);
            feast.set_rules(kitchen_sim::rules::Rules::parse(rules));
            let players = feast.players.iter().map(|p| kitchen_sim::net::Seat { player: p.clone(), human: p.id == 1, ..Default::default() }).collect();
            kitchen_sim::net::Frame { tick, round, rules: rules.into(), stage: "Warmup".into(), players, ..Default::default() }
        };
        let tick = std::cell::Cell::new(0);
        let next = |app: &mut App, round: u32, rules: &str| {
            tick.set(tick.get() + 1);
            apply_frame(app.world_mut(), &frame(round, rules, tick.get()));
        };
        next(&mut app, 1, "mode=cards");
        test_frame(&mut app, 0.05);
        assert!(crate::cards::at_table(app.world().resource::<Game>()));
        let round = play_a_second(&mut app, |app| next(app, 1, "mode=cards"));
        stood_down(&round);
        assert!(!app.world().resource::<Game>().online.unsent.iter().any(|c| c.has(kitchen_sim::net::button::FIRE)), "nothing fired at the server");
        // The game is over and everybody waits in the fridge.
        app.world_mut().resource_mut::<crate::lobby::Lobby>().holding = true;
        next(&mut app, 1, "mode=cards");
        test_frame(&mut app, 0.05);
        assert!(!crate::cards::at_table(app.world().resource::<Game>()));
        // The next round is a siege: the head-up display, the camera, the guns, the tags and the pickups are back.
        app.world_mut().resource_mut::<crate::lobby::Lobby>().holding = false;
        next(&mut app, 2, "");
        test_frame(&mut app, 0.05);
        let g = app.world().resource::<Game>();
        assert!(!crate::cards::at_table(g) && g.feast.cards.is_none() && !g.feast.rules.cards);
        let round = play_a_second(&mut app, |app| next(app, 2, ""));
        played(&round, app.world().resource::<Game>());
    }

    /// A frame `dt` seconds after the last in which the wheel has turned by `y` in `unit` (negative is down: the
    /// way winit has it, whatever the page's `deltaY`).
    fn spin(app: &mut App, unit: MouseScrollUnit, y: f32, dt: f32) {
        *app.world_mut().resource_mut::<AccumulatedMouseScroll>() = AccumulatedMouseScroll { unit, delta: Vec2::new(0., y) };
        test_frame(app, dt);
    }
    /// A frame in which the wheel turns by `y` lines, long enough after the last for a step just taken to be over
    /// with (`WHEEL_COOLDOWN`); and `y` of 0 is such a frame without the wheel.
    fn notch(app: &mut App, y: f32) {
        spin(app, MouseScrollUnit::Line, y, 0.2);
    }
    /// A round played by `rules` that holds `guns` (as well as what it was given: in a siege the syrup rifle and the
    /// mustard pistol), synced and run for a frame.
    fn holding(rules: &str, guns: &[Weapon]) -> App {
        holding_but(rules, guns, &[])
    }
    /// The same without the guns `gone` (the syrup rifle it starts with given up for a sniper, say).
    fn holding_but(rules: &str, guns: &[Weapon], gone: &[Weapon]) -> App {
        let mut app = test_app(rules);
        app.add_systems(Update, tick);
        // (The round begins in the first frame, with what the rules give.)
        test_frame(&mut app, 0.05);
        let mut game = app.world_mut().resource_mut::<Game>();
        let p = &mut game.feast.players[0];
        for gun in guns {
            p.owned[*gun as usize] = true;
        }
        for gun in gone {
            (p.owned[*gun as usize], p.ammo[*gun as usize]) = (false, 0);
        }
        test_frame(&mut app, 0.05);
        test_frame(&mut app, 0.05);
        app
    }
    fn in_hand(app: &App) -> Weapon {
        app.world().resource::<Game>().weapon
    }
    fn keyboard(app: &mut App, has: bool) {
        let world = app.world_mut();
        for mut window in world.query::<&mut Window>().iter_mut(world) {
            window.focused = has;
        }
    }

    #[test]
    fn a_wheel_notch_steps_once_and_a_trackpad_needs_a_full_notch() {
        // A wheel's notch is a line: one step each way, with nothing left over.
        let mut rest = 0.;
        assert_eq!((wheel_step(&mut rest, 1., false), wheel_step(&mut rest, -1., false)), (1, -1));
        assert_eq!(rest, 0.);
        // A trackpad's small deltas add up: five of 10 px make one step, and not before.
        let mut rest = 0.;
        let steps: Vec<i32> = (0..5).map(|_| wheel_step(&mut rest, 10., true)).collect();
        assert_eq!(steps, [0, 0, 0, 0, 1]);
        assert_eq!(rest, 0.);
        // A step is the whole of what the wheel has made: what is over a notch goes with it. 70 px, a spin of 3 lines or
        // a browser's 100 px notch is one step and nothing is left for another.
        for (delta, pixel) in [(70., true), (3., false), (100., true), (400., true), (-100., true), (-3., false)] {
            let mut rest = 0.;
            assert_eq!((wheel_step(&mut rest, delta, pixel), rest), (delta.signum() as i32, 0.), "{delta}");
            assert_eq!((wheel_step(&mut rest, 0., pixel), wheel_step(&mut rest, 0., pixel)), (0, 0), "nothing more comes of {delta}");
        }
        // What is short of a notch is kept for the next: 49 px, and 1 px more.
        let mut rest = 0.;
        assert_eq!((wheel_step(&mut rest, 49., true), rest), (0, 49.));
        assert_eq!((wheel_step(&mut rest, 1., true), rest), (1, 0.));
        // Turning the other way forgets what was gathered: 40 px down and then 40 px up is not a step, and the 40 px
        // up (not 0) is what is left, so 10 px more up is.
        assert_eq!((wheel_step(&mut rest, 40., true), wheel_step(&mut rest, -40., true), rest), (0, 0, -40.));
        assert_eq!(wheel_step(&mut rest, -10., true), -1);
        // A wheel's fractions (a smooth wheel reports a part of a line) gather the same way.
        let mut rest = 0.;
        let steps: Vec<i32> = (0..4).map(|_| wheel_step(&mut rest, 0.25, false)).collect();
        assert_eq!(steps, [0, 0, 0, 1]);
        // Nothing turned, nothing stepped.
        assert_eq!(wheel_step(&mut rest, 0., true), 0);
    }

    /// The first-person model of `weapon`: its root's place in the camera, and whether it is shown.
    fn viewmodel(app: &App, weapon: Weapon) -> (Vec3, bool) {
        let g = app.world().resource::<Game>();
        let e = g.viewmodels.iter().find(|(w, _)| *w == weapon).unwrap().1;
        (app.world().get::<Transform>(e).unwrap().translation, app.world().get::<Visibility>(e) == Some(&Visibility::Visible))
    }
    fn fov(app: &mut App) -> f32 {
        let cam = camera(app);
        let Projection::Perspective(lens) = app.world().get::<Projection>(cam).unwrap().clone() else { panic!("a perspective camera") };
        lens.fov
    }
    fn right_click(app: &mut App) {
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        test_frame(app, 0.01);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release_all();
    }
    /// The right button pressed and kept down, or let go, and a frame.
    fn right_button(app: &mut App, down: bool) {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        if down {
            mouse.press(MouseButton::Right);
        } else {
            mouse.release(MouseButton::Right);
        }
        test_frame(app, 0.01);
    }
    #[test]
    fn the_right_button_held_raises_the_syrup_rifles_optic_to_the_eye_and_let_go_lowers_it_again() {
        let mut app = holding("", &[]);
        assert_eq!(in_hand(&app), Weapon::Rifle);
        assert!(!crate::settings::Settings::default().scope_toggle, "held, to begin with");
        let (hip, seen) = viewmodel(&app, Weapon::Rifle);
        assert!(seen && hip.distance(GRIP) < 1e-4, "at the hip: {hip}");
        right_button(&mut app, true);
        {
            let g = app.world().resource::<Game>();
            assert!(g.scoped && g.feast.player(1).unwrap().aimed, "aimed, and the simulation knows");
            assert!(g.aim > 0. && g.aim < 0.2, "on its way up: {}", g.aim);
        }
        // Up in AIM_TIME (0.15 s), the button still held: centred, the optic at the eye, still in view; the view
        // magnified ZOOM times, the sniper's black mask not up, and no crosshair (the reticle is the lens's).
        for _ in 0..4 {
            test_frame(&mut app, 0.04);
        }
        let (up, seen) = viewmodel(&app, Weapon::Rifle);
        assert_eq!(app.world().resource::<Game>().aim, 1.);
        assert!(app.world().resource::<Game>().scoped, "still held, still up");
        assert!(seen && up.distance(aim_grip()) < 1e-4, "aimed: {up} for {}", aim_grip());
        let zoomed = fov(&mut app);
        assert!(((85_f32.to_radians() * 0.5).tan() / (zoomed * 0.5).tan() - crate::scope::ZOOM).abs() < 1e-3, "{zoomed}");
        assert!(!shown::<ScopeMask>(&mut app), "the sniper's mask is not the rifle's");
        let crosshair = app.world_mut().query::<(&Hud, &Text)>().iter(app.world()).find(|(h, _)| h.0 == 3).map(|(_, t)| t.0.clone());
        assert_eq!(crosshair.as_deref(), Some(""), "the reticle is the crosshair");
        // Let go: halfway down after half the time, at the hip again after it all.
        right_button(&mut app, false);
        assert!(!app.world().resource::<Game>().scoped && !app.world().resource::<Game>().feast.player(1).unwrap().aimed);
        test_frame(&mut app, 0.055);
        let (down, _) = viewmodel(&app, Weapon::Rifle);
        assert!(down.distance(GRIP) > 0.01 && down.distance(aim_grip()) > 0.01, "on its way down: {down}");
        for _ in 0..3 {
            test_frame(&mut app, 0.05);
        }
        assert!(viewmodel(&app, Weapon::Rifle).0.distance(GRIP) < 1e-4 && (fov(&mut app) - 85_f32.to_radians()).abs() < 1e-5);
        // Toggled, the settings' other way: a click raises it, and it stays up until the next.
        app.world_mut().insert_resource(crate::settings::Settings { scope_toggle: true, ..default() });
        right_click(&mut app);
        for _ in 0..4 {
            test_frame(&mut app, 0.04);
        }
        assert!(app.world().resource::<Game>().scoped && app.world().resource::<Game>().aim == 1., "toggled up, the button let go");
        right_click(&mut app);
        assert!(!app.world().resource::<Game>().scoped);
        // Not with the pistol in hand; a swap puts the aim down; and the dead aim at nothing.
        right_click(&mut app);
        assert!(app.world().resource::<Game>().scoped);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Digit2);
        test_frame(&mut app, 0.05);
        let g = app.world().resource::<Game>();
        assert!(g.weapon == Weapon::Pistol && !g.scoped && g.aim == 0., "the pistol has no sights to raise");
        right_click(&mut app);
        assert!(!app.world().resource::<Game>().scoped);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Digit1);
        test_frame(&mut app, 0.05);
        right_click(&mut app);
        assert!(app.world().resource::<Game>().scoped);
        app.world_mut().resource_mut::<Game>().feast.players[0].alive = false;
        test_frame(&mut app, 0.05);
        test_frame(&mut app, 0.05);
        assert!(!app.world().resource::<Game>().scoped, "dead, nothing is aimed");
        right_click(&mut app);
        assert!(!app.world().resource::<Game>().scoped);
    }
    /// Through the syrup rifle's optic the mouse turns the view as many times slower as the optic magnifies, so that
    /// the reticle crosses a target at the pace the crosshair does at the hip.
    #[test]
    fn through_the_optic_the_mouse_turns_the_view_as_many_times_slower_as_it_magnifies() {
        fn turn(app: &mut App) -> f32 {
            let yaw = app.world().resource::<Game>().yaw;
            app.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(40., 0.);
            test_frame(app, 0.01);
            yaw - app.world().resource::<Game>().yaw
        }
        let mut app = holding("", &[]);
        let window = app.world_mut().query_filtered::<Entity, With<Window>>().iter(app.world()).next().unwrap();
        app.world_mut().entity_mut(window).insert(bevy::window::CursorOptions { grab_mode: bevy::window::CursorGrabMode::Locked, ..default() });
        test_frame(&mut app, 0.01);
        let hip = turn(&mut app);
        right_button(&mut app, true);
        for _ in 0..5 {
            test_frame(&mut app, 0.04);
        }
        let up = turn(&mut app);
        assert!(hip > 0. && (hip / up - crate::scope::ZOOM).abs() < 1e-3, "at the hip {hip}, through the optic {up}");
    }
    /// The ketchup sniper's scope is as it was, held up by the right button as the rifle's optic is: the black mask
    /// round its crosshair at 18 degrees, the gun put away; let go, it comes down.
    #[test]
    fn the_snipers_scope_is_held_up_by_the_right_button_as_it_always_was_drawn() {
        let mut app = holding("", &[Weapon::Sniper]);
        while in_hand(&app) != Weapon::Sniper {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyY);
            test_frame(&mut app, 0.2);
        }
        right_button(&mut app, true);
        test_frame(&mut app, 0.05);
        let g = app.world().resource::<Game>();
        assert!(g.scoped && g.feast.player(1).unwrap().aimed && g.aim == 0., "the sniper's scope is up; the rifle's optic is not");
        assert!(shown::<ScopeMask>(&mut app), "the black mask round the crosshair");
        assert!((fov(&mut app) - 18_f32.to_radians()).abs() < 1e-5);
        assert!(!viewmodel(&app, Weapon::Sniper).1, "the gun put away");
        right_button(&mut app, false);
        test_frame(&mut app, 0.05);
        assert!(!app.world().resource::<Game>().scoped && !shown::<ScopeMask>(&mut app));
        assert!((fov(&mut app) - 85_f32.to_radians()).abs() < 1e-5);
    }
    #[test]
    fn the_wheel_changes_guns_like_y() {
        // Two guns (the syrup rifle given up for a sniper): down and up both go to the other one, and the scope is put
        // away as the keys put it away.
        let mut app = holding_but("", &[Weapon::Sniper], &[Weapon::Rifle]);
        assert_eq!(held_guns(&app.world().resource::<Game>().owned), [Weapon::Sniper, Weapon::Pistol]);
        let start = in_hand(&app);
        app.world_mut().resource_mut::<Game>().scoped = true;
        notch(&mut app, -1.);
        let other = if start == Weapon::Pistol { Weapon::Sniper } else { Weapon::Pistol };
        assert_eq!(in_hand(&app), other);
        assert!(!app.world().resource::<Game>().scoped, "the scope is down");
        assert_eq!(app.world().resource::<Game>().feast.player(1).unwrap().weapon, other as usize, "the simulation knows");
        notch(&mut app, 1.);
        assert_eq!(in_hand(&app), start);
        notch(&mut app, -1.);
        assert_eq!(in_hand(&app), other);
        // A frame with no wheel changes nothing, and neither does a turn sideways.
        notch(&mut app, 0.);
        app.world_mut().resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(3., 0.);
        test_frame(&mut app, 0.05);
        assert_eq!(in_hand(&app), other);
        // The two a hotdog starts with: the rifle in hand, the pistol the other.
        let mut app = holding("", &[]);
        assert_eq!((held_guns(&app.world().resource::<Game>().owned), in_hand(&app)), (vec![Weapon::Rifle, Weapon::Pistol], Weapon::Rifle));
        notch(&mut app, -1.);
        assert_eq!(in_hand(&app), Weapon::Pistol);
        notch(&mut app, -1.);
        assert_eq!(in_hand(&app), Weapon::Rifle);
        // One gun: nothing to change to.
        let mut app = holding_but("", &[], &[Weapon::Pistol]);
        let only = in_hand(&app);
        assert_eq!(held_guns(&app.world().resource::<Game>().owned), [only]);
        notch(&mut app, -1.);
        notch(&mut app, 1.);
        assert_eq!(in_hand(&app), only);
        // The sandbox may hold more than two: down goes forward through them in slot order, up goes back, round and round.
        // (In god mode: the bots fight, and with the syrup rifles they start with they would have it dead before the
        // wheel had gone round.)
        let mut app = holding("sandbox god weapons=mustard,sniper,hotsauce", &[]);
        let held = held_guns(&app.world().resource::<Game>().owned);
        assert_eq!(held.len(), 3, "{held:?}");
        let at = held.iter().position(|w| *w == in_hand(&app)).expect("a gun in hand");
        for step in 1..=4 {
            notch(&mut app, -1.);
            assert_eq!(in_hand(&app), held[(at + step) % 3], "down, {step} times");
        }
        for step in 1..=4 {
            notch(&mut app, 1.);
            assert_eq!(in_hand(&app), held[(at + 4 + 3 * 4 - step) % 3], "and up, {step} times");
        }
        assert_eq!(in_hand(&app), held[at]);
        // Y goes the way down does.
        let down = held[(at + 1) % 3];
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyY);
        test_frame(&mut app, 0.05);
        assert_eq!(in_hand(&app), down);
    }

    #[test]
    fn the_wheel_does_nothing_at_the_card_table_dead_or_laden() {
        // Whether the gun in hand is changed by three notches down in a round where two guns are held (an odd number, so
        // two guns end up apart), with `set` done to the game before each, and `unset` (and a frame) before looking: a
        // hotdog whose hands are full has no gun in hand, and the one it had comes back when they are free.
        let turns = |rules: &str, set: &dyn Fn(&mut Game), unset: &dyn Fn(&mut Game)| {
            let mut app = holding(rules, &[]);
            let start = in_hand(&app);
            for _ in 0..3 {
                set(&mut app.world_mut().resource_mut::<Game>());
                notch(&mut app, -1.);
            }
            unset(&mut app.world_mut().resource_mut::<Game>());
            notch(&mut app, 0.);
            in_hand(&app) != start
        };
        let nothing = |_: &mut Game| {};
        assert!(turns("", &nothing, &nothing), "a siege");
        assert!(turns("sandbox weapons=all", &nothing, &nothing), "the sandbox");
        // At the card table there is no gun in hand to change: Fully Loaded, and the sandbox played as it.
        assert!(!turns("mode=cards", &nothing, &nothing), "the card table");
        assert!(!turns("sandbox mode=cards weapons=all ammo=inf", &nothing, &nothing), "the sandbox's card table");
        // Dead.
        let dead = |g: &mut Game| {
            g.feast.players[0].alive = false;
            g.roster[0].alive = false;
        };
        let alive = |g: &mut Game| {
            g.feast.players[0].alive = true;
            g.roster[0].alive = true;
        };
        assert!(!turns("", &dead, &alive), "dead");
        // Laden: carrying a loose object, the hands are full.
        let laden = |g: &mut Game| {
            g.feast.players[0].carry = 0;
            g.roster[0].carry = 0;
        };
        let free = |g: &mut Game| {
            g.feast.players[0].carry = -1;
            g.roster[0].carry = -1;
        };
        assert!(!turns("", &laden, &free), "hands full");
        // (And the keys are no different at the table.)
        let mut app = holding("mode=cards", &[]);
        let start = in_hand(&app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyY);
        test_frame(&mut app, 0.05);
        assert_eq!(in_hand(&app), start);
    }

    /// Carrying a loose object, the wheel turns it an eighth of a turn a notch (rolled down, clockwise seen from above)
    /// and X tips it over onto its front, onto its side and back; the simulation is told (offline: `set_held_pose`;
    /// online the commands carry it). Its hands empty again, it is held as the map stood it next time.
    #[test]
    fn carrying_the_wheel_turns_what_is_held_and_x_tips_it() {
        let mut app = holding("", &[]);
        let carry = |app: &mut App, prop: i32| {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.feast.players[0].carry = prop;
            g.roster[0].carry = prop;
        };
        for _ in 0..3 {
            carry(&mut app, 0);
            notch(&mut app, -1.);
        }
        let pose = |app: &App| (app.world().resource::<Game>().pose, app.world().resource::<Game>().feast.players[0].pose);
        assert_eq!(pose(&app), (Pose { turn: 5, tip: 0 }, Pose { turn: 5, tip: 0 }), "three eighths clockwise");
        carry(&mut app, 0);
        notch(&mut app, 1.);
        assert_eq!(pose(&app).0, Pose { turn: 6, tip: 0 }, "and one back");
        for tip in [1, 2, 0, 1] {
            carry(&mut app, 0);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyX);
            test_frame(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyX);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
            assert_eq!(pose(&app), (Pose { turn: 6, tip }, Pose { turn: 6, tip }));
        }
        // Put down: held as the map stands it next time, and the wheel is the guns' again.
        carry(&mut app, -1);
        notch(&mut app, 0.);
        assert_eq!(pose(&app), (Pose::default(), Pose::default()));
    }

    #[test]
    fn the_wheel_waits_for_a_full_notch_of_a_trackpad_and_does_not_race() {
        let pixels = |app: &mut App, y: f32, dt: f32| spin(app, MouseScrollUnit::Pixel, y, dt);
        let mut app = holding("", &[]);
        let start = in_hand(&app);
        // 30 px and then 30 px more make a notch down: the second frame is the one that changes the gun.
        pixels(&mut app, -30., 0.05);
        assert_eq!(in_hand(&app), start);
        pixels(&mut app, -30., 0.05);
        assert_ne!(in_hand(&app), start);
        let other = in_hand(&app);
        // The 0.15 s after a step are not heard (not saved up either): 60 px then are lost, and so is the 10 over the notch.
        pixels(&mut app, -60., 0.05);
        assert_eq!(in_hand(&app), other);
        // A notch is then 50 px again, from nothing: 30, 10 and 10 px more.
        pixels(&mut app, -30., 0.2);
        pixels(&mut app, -10., 0.05);
        assert_eq!(in_hand(&app), other);
        pixels(&mut app, -10., 0.05);
        assert_eq!(in_hand(&app), start);
        // A pause of 0.3 s and more lets a part of a notch go.
        pixels(&mut app, -30., 0.2);
        pixels(&mut app, -30., 0.4);
        assert_eq!(in_hand(&app), start, "30 px, a pause, 30 px: not a notch");
        // And a turn the other way does.
        pixels(&mut app, 30., 0.05);
        assert_eq!(in_hand(&app), start, "30 px up forgot the 30 px down");
        pixels(&mut app, 20., 0.05);
        assert_ne!(in_hand(&app), start, "50 px up is a notch up");
        // Five small deltas of a trackpad are one change.
        let mut app = holding("", &[]);
        let start = in_hand(&app);
        let mut changes = 0;
        for _ in 0..5 {
            let before = in_hand(&app);
            pixels(&mut app, -10., 0.016);
            changes += (in_hand(&app) != before) as u32;
        }
        assert_eq!((changes, in_hand(&app) != start), (1, true), "five of 10 px");
        // At most one gun a frame, and one a spin: three notches at once are one gun, and the frames after it have no
        // more to give.
        let mut app = holding("sandbox weapons=mustard,sniper,hotsauce", &[]);
        let held = held_guns(&app.world().resource::<Game>().owned);
        let at = held.iter().position(|w| *w == in_hand(&app)).unwrap();
        notch(&mut app, -3.);
        assert_eq!(in_hand(&app), held[(at + 1) % 3]);
        for _ in 0..8 {
            notch(&mut app, 0.);
            assert_eq!(in_hand(&app), held[(at + 1) % 3], "the spin is spent");
        }
    }

    #[test]
    fn one_roll_of_the_wheel_is_one_change_whatever_a_browser_makes_of_it() {
        // Chrome and Edge send about 100 px for a notch, Firefox 3 lines, a smooth wheel a part of a line, a fast flick
        // more than that. Two guns: it is the other gun, and it stays the other gun (it is not swapped back).
        for (unit, y) in [
            (MouseScrollUnit::Line, 1.),
            (MouseScrollUnit::Line, 3.),
            (MouseScrollUnit::Pixel, 100.),
            (MouseScrollUnit::Pixel, 53.),
            (MouseScrollUnit::Pixel, 400.),
        ] {
            for y in [y, -y] {
                let mut app = holding("", &[]);
                let start = in_hand(&app);
                spin(&mut app, unit, y, 0.016);
                assert_ne!(in_hand(&app), start, "{unit:?} {y}");
                let other = in_hand(&app);
                for _ in 0..40 {
                    test_frame(&mut app, 0.016);
                    assert_eq!(in_hand(&app), other, "{unit:?} {y} changes once");
                }
            }
        }
        // Three guns: one step on, the right way round.
        for (y, step) in [(-3., 1), (3., 2)] {
            let mut app = holding("sandbox weapons=mustard,sniper,hotsauce", &[]);
            let held = held_guns(&app.world().resource::<Game>().owned);
            let at = held.iter().position(|w| *w == in_hand(&app)).unwrap();
            notch(&mut app, y);
            for _ in 0..6 {
                notch(&mut app, 0.);
            }
            assert_eq!(in_hand(&app), held[(at + step) % 3], "a spin of {y} lines");
        }
    }

    #[test]
    fn the_wheel_is_heard_again_after_a_short_while_and_what_came_between_is_dropped() {
        let mut app = holding("", &[]);
        let start = in_hand(&app);
        notch(&mut app, -1.);
        let other = in_hand(&app);
        assert_ne!(other, start);
        // 0.05 s after, and 0.1 s: the wheel is not heard (0.15 s), and is not heard late either.
        spin(&mut app, MouseScrollUnit::Line, 1., 0.05);
        assert_eq!(in_hand(&app), other, "0.05 s after");
        spin(&mut app, MouseScrollUnit::Line, 1., 0.05);
        assert_eq!(in_hand(&app), other, "0.1 s after");
        for _ in 0..5 {
            test_frame(&mut app, 0.05);
            assert_eq!(in_hand(&app), other, "what was turned in the meantime is not saved up");
        }
        // A roll after that is heard.
        spin(&mut app, MouseScrollUnit::Line, 1., 0.05);
        assert_eq!(in_hand(&app), start);
    }

    #[test]
    fn a_fling_of_the_wheel_changes_the_gun_every_so_often_and_does_not_race() {
        // The wheel turning hard in every frame for half a second (a trackpad's momentum, a spun wheel): a change each
        // 0.15 s, so four at the most, and one at the least.
        for (unit, y) in [(MouseScrollUnit::Pixel, -120.), (MouseScrollUnit::Pixel, 120.), (MouseScrollUnit::Line, -3.), (MouseScrollUnit::Pixel, -60.)] {
            let mut app = holding("", &[]);
            let mut changes = 0;
            for _ in 0..31 {
                let before = in_hand(&app);
                spin(&mut app, unit, y, 0.016);
                changes += (in_hand(&app) != before) as u32;
            }
            assert!((3..=4).contains(&changes), "{unit:?} {y}: {changes} changes in 0.5 s");
        }
    }

    #[test]
    fn the_wheel_is_not_heard_while_the_window_does_not_have_the_keyboard() {
        // The page's chat box has the keyboard: the keys do not reach the game, and neither does the wheel (which
        // does reach the canvas, with the pointer over it).
        let mut app = holding("", &[]);
        let start = in_hand(&app);
        keyboard(&mut app, false);
        notch(&mut app, -1.);
        notch(&mut app, -1.);
        notch(&mut app, -1.);
        assert_eq!(in_hand(&app), start);
        // What was turned meanwhile is not saved up: the first notch after the keyboard is back is the only one.
        keyboard(&mut app, true);
        notch(&mut app, 0.);
        assert_eq!(in_hand(&app), start);
        notch(&mut app, -1.);
        assert_ne!(in_hand(&app), start);
    }

    /// Kung Fu, single player: the keys are the fight's (controls.rs), made into commands for the round itself
    /// (`Feast::kungfu_command`), and the client keeps the round's view of the fight. 7 asks for the Toad, the wheel goes
    /// round the styles, F kicks, Q held blocks; no gun is selected, the stim is not slapped on, nothing is picked up.
    #[test]
    fn kung_fu_keys_change_style_kick_and_block_and_nothing_of_the_guns_is_heard() {
        use kitchen_sim::kungfu::{Kind, Style};
        // (The sandbox's bots stand still: nobody knocks this player down while it tries its keys.)
        let mut app = test_app("sandbox mode=kungfu botmode=idle");
        app.add_systems(Update, tick);
        for _ in 0..3 {
            test_frame(&mut app, 0.05);
        }
        let mine = |app: &App| app.world().resource::<Game>().fighter(1).cloned().expect("the client has the fight");
        assert!(app.world().resource::<Game>().kung_fu());
        let press = |app: &mut App, key: KeyCode| app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        // 7: the Toad (once free, which a fighter in its stance is).
        press(&mut app, KeyCode::Digit7);
        test_frame(&mut app, 0.05);
        assert_eq!(app.world().resource::<Game>().fu_style, Some(6));
        test_frame(&mut app, 0.05);
        assert_eq!(mine(&app).style(), Style::Toad);
        // The wheel rolled down: the next style round from the Toad is the Tiger.
        app.world_mut().resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0., -1.);
        app.world_mut().resource_mut::<AccumulatedMouseScroll>().unit = bevy::input::mouse::MouseScrollUnit::Line;
        test_frame(&mut app, 0.05);
        test_frame(&mut app, 0.05);
        assert_eq!(mine(&app).style(), Style::Tiger);
        // F kicks (the style's first kick), and the guns, the stim and the melee are not heard.
        press(&mut app, KeyCode::KeyF);
        test_frame(&mut app, 0.05);
        let g = app.world().resource::<Game>();
        assert_eq!(mine(&app).mv().data().kind, Kind::Kick, "{:?}", mine(&app).mv());
        assert!(g.feast.player(1).is_some_and(|p| p.stim <= 0.));
        // Q held blocks once the kick is over.
        for _ in 0..30 {
            test_frame(&mut app, 0.05);
        }
        for _ in 0..4 {
            press(&mut app, KeyCode::KeyQ);
            test_frame(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyQ);
        }
        assert_ne!(mine(&app).block, kitchen_sim::kungfu::NO_BLOCK, "{:?}", mine(&app).mv());
        assert_eq!(app.world().resource::<Game>().fu_hold & button::GRAB, button::GRAB, "held: every command carries it");
        // Shift is the dodge, not the stim.
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release_all();
        for _ in 0..10 {
            test_frame(&mut app, 0.05);
        }
        press(&mut app, KeyCode::ShiftLeft);
        test_frame(&mut app, 0.05);
        assert_eq!(mine(&app).mv().data().kind, Kind::Dodge, "{:?}", mine(&app).mv());
        assert!(app.world().resource::<Game>().feast.player(1).is_some_and(|p| p.stim <= 0.), "no stim in Kung Fu");
    }
    /// The titan's keys (controls.rs, the TITAN tab) are the interface's bits: the swat FIRE and the smash SCOPE held,
    /// the crouch held, the stomp MELEE, the charge STIM and the jump pressed.
    #[test]
    fn the_titans_buttons_are_the_interfaces_bits() {
        let (held, pressed) = titan_buttons(&TitanKeys { swat: true, stomp: true, charge: true, ..default() }, true);
        assert_eq!((held, pressed), (button::FIRE | button::CROUCH, button::MELEE | button::STIM));
        let (held, pressed) = titan_buttons(&TitanKeys { smash: true, jump: true, ..default() }, false);
        assert_eq!((held, pressed), (button::SCOPE, button::JUMP));
        assert_eq!(titan_buttons(&TitanKeys::default(), false), (0, 0));
    }

    /// Offline, the player who is the titan drives it (`Feast::titan_command`): Shift charges, and nothing of the guns,
    /// the stim or the pickups is heard; a small hotdog's Shift is still its stim.
    #[test]
    fn the_titans_player_charges_on_shift_and_a_small_hotdog_stims() {
        let map = || Map::parse(include_str!("../assets/maps/titans-kitchen.json")).unwrap();
        for (pick, titan) in [("me", true), ("bot", false)] {
            let mut app = test_app(&format!("sandbox mode=titan titan={pick} botmode=idle"));
            app.add_systems(Update, tick);
            load_map(app.world_mut(), map());
            for _ in 0..3 {
                test_frame(&mut app, 0.05);
            }
            let g = app.world().resource::<Game>();
            assert_eq!(crate::titan::titan_id(g) == Some(g.me()), titan, "{pick}");
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ShiftLeft);
            test_frame(&mut app, 0.05);
            let g = app.world().resource::<Game>();
            let view = g.titan.as_ref().unwrap();
            let stimmed = g.feast.player(g.me()).is_some_and(|p| p.stim > 0.);
            if titan {
                assert_eq!((view.mv(), stimmed), (kitchen_sim::titan::Move::Charge, false), "the charge winds up");
                assert_eq!(g.weapon, g.weapon, "no gun changes");
            } else {
                assert!(view.mv() != kitchen_sim::titan::Move::Charge && stimmed, "the small hotdog's stim");
            }
        }
    }

    #[test]
    fn kung_fus_buttons_are_the_interfaces_bits_and_the_styles_go_round() {
        let keys = FuKeys { light: true, block: true, kick: true, dodge: true, ..default() };
        let (held, pressed) = fu_buttons(&keys, true);
        assert_eq!(held, button::FIRE | button::GRAB | button::CROUCH);
        assert_eq!(pressed, button::MELEE | button::STIM);
        let (held, pressed) = fu_buttons(&FuKeys { heavy: true, special: true, grab: true, jump: true, ..default() }, false);
        assert_eq!((held, pressed), (button::SCOPE | button::PRONE, button::THROW | button::JUMP));
        // By name; or the next and the last round from what was asked (or the fighter's own); nothing before the fight.
        assert_eq!(fu_style(&FuKeys { style: Some(3), ..default() }, Some(0), Some(1)), Some(3));
        assert_eq!(fu_style(&FuKeys { step: 1, ..default() }, Some(6), None), Some(0));
        assert_eq!(fu_style(&FuKeys { step: -1, ..default() }, None, Some(0)), Some(6));
        assert_eq!(fu_style(&FuKeys::default(), None, Some(2)), Some(2));
        assert_eq!(fu_style(&FuKeys::default(), None, None), None);
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Render-independent five-player survival rules. Positions are in metres.
use crate::actor::{self, Actor};
use crate::cards;
use crate::drape;
use crate::emergence::Emergence;
use crate::forts::{self, Fort};
#[cfg(test)]
use crate::emergence::WAVE;
use crate::map::{Level, Map, Prop, Side, LOOSE};
use crate::props::{self, PropWorld, ShapeSpec};
use crate::rules::Mode;
use crate::{ray_box, ray_sphere};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const FLOOR: f32 = 0.9025;
/// Height of the muzzle above the feet: standing, crouching and prone.
pub const EYE: f32 = 0.135;
pub const MID: f32 = 0.09;
pub const LOW: f32 = 0.05;
/// Height above the feet of the middle of what a round can hit: standing, crouching and prone. Level fire
/// from a standing hotdog's muzzle passes over a prone hotdog, and all but a relish glob passes over a
/// crouching one.
pub const CHEST: [f32; 3] = [0.105, 0.055, 0.04];
// By weapon: 0 is the melee strike (made with the gun in hand: there are no bare hands), then the condiment
// rounds: mustard, ketchup, hot sauce, relish glob, bug spray, pancake syrup, mayonnaise. Metres, seconds and HP.
// Weapon 0 in a hotdog's hand means its hands are full (a
// loose object, or the bacon): nothing selects it.
pub const DELAY: [f32; GUNS] = [0.48, 0.35, 1.05, 0.1, 1.2, 0.12, 0.40, 1. / 12.];
/// Metres a second, five times what the rounds once flew (SPEEDUP): each still ends at its RANGE, sooner. A round is
/// tested along the whole of its way each step (`flight`), so however fast it goes it meets what is in its way.
pub const ROUND_SPEED: [f32; GUNS] = [0., 7.5, 45., 5.5, 4.5, 6.5, 17.5, 11.];
/// How many times faster the rounds fly than they once did (`ROUND_SPEED`). The mayonnaise falls SPEEDUP² as fast
/// (MAYO_FALL), so that its arcs have the shapes and the reach they had, and a round is slowed to a stall (STALL)
/// by as many light objects as before.
pub const SPEEDUP: f32 = 5.;
pub const ROUND_RADIUS: [f32; GUNS] = [0., 0.012, 0.008, 0.02, 0.03, 0.05, 0.009, 0.014];
pub const RANGE: [f32; GUNS] = [0.21, 3., 6., 0.65, 4., 0.8, 4., 6.];
/// A relish glob's blast reaches SPLASH[4] metres from where it burst to the nearest point of a body (see `blast`).
pub const SPLASH: [f32; GUNS] = [0., 0., 0., 0., 0.18, 0., 0., 0.];
/// Within LETHAL metres of a body a glob's blast does all its damage (a hotdog's: MAX_HP, a kill from full health).
pub const LETHAL: f32 = 0.02;
pub const ZOMBIE_DAMAGE: [f32; GUNS] = [ZOMBIE_HP / 2., 35., 100., 12., 100., 0., 18., 5.];
pub const PLAYER_DAMAGE: [f32; GUNS] = [MAX_HP / 2., 18., 55., 4., MAX_HP, 0., 12., 1.8];
/// A round in the face (`in_face`), by gun: what its damage there is multiplied by (1: no more than anywhere else). The
/// ketchup sniper's and the mustard pistol's kill from full health there: the sniper's twice its 55 HP (two in the
/// body), the pistol's six times its 18 (108; six in the body).
pub const FACE: [f32; GUNS] = [1., 6., 2., 1., 1., 1., 1., 1.];
/// The face, on the head (head.rs: the capsule a round finds the head by, which nods with the look): where a round
/// enters the head, the way out of it rises FACE_RISE[0] to FACE_RISE[1] in the head's own frame (from the chin to the
/// eyes) and, seen from above, lies within FACE_ARC (radians) of the way the face looks; and the round comes from in
/// front of the face, its way against the face's at least FACING. Prone, the face is the head's front end: the way out
/// within FACE_ARC of it, not below PRONE_RISE. It follows the stance as the head does (crouched lower, tucked in the
/// air as standing, prone at the counter) and turns with the nod (bowed it looks at the counter).
pub use crate::head::{FACE_ARC, FACE_RISE, FACING, PRONE_RISE, face_point, in_face};
pub const SPRAY_DAMAGE: [f32; GUNS] = [0., 0., 0., 0., 0., 20., 0., 0.];
/// The most a falling round's way in a step (a chord of its arc) may sag below the arc, in metres (`flight`).
pub const ARC_SAG: f32 = 0.00005;
/// What a round meets on its way (`flight`), in every mode. Something solid stops it: a block or a wall (the
/// forcefield and a draped napkin's wall among them) or a solid loose object (props.rs MATERIALS: a chair, a crate,
/// the bowl, the tray, a napkin, the kitchen's china, tins, boards and glass). A light loose object (a pillow, a cereal
/// box, a roll of paper towels) lets it through: each one it passes takes its
/// speed down to LIGHT_SPEED of what it was and its damage (and a glob's splash) to LIGHT_DAMAGE, and a round slower
/// than STALL m/s after one splats there and then. A round is not slowed twice by one object.
pub const LIGHT_SPEED: f32 = 0.6;
pub const LIGHT_DAMAGE: f32 = 0.75;
pub const STALL: f32 = 0.3 * SPEEDUP;
/// The mustard pistol (weapon 1): a hand cannon, whose round in the face kills (FACE) and whose muzzle flips (recoil.rs).
pub const PISTOL: usize = 1;
/// The ketchup sniper's round goes through bodies: it hurts every hotdog and cat it passes (each once, at its full
/// damage, less what light objects took), and flies on until its range runs out or something solid stops it. Every
/// other round ends at the first body it meets.
pub const SNIPER: usize = 2;
/// How many weapon numbers there are: 0 (the melee strike, or the hands full) and the seven guns. Each hotdog's
/// `ammo` and `owned` have a place for each.
pub const GUNS: usize = 8;
/// The mayonnaise squeeze bottle (weapon 7) squirts a stream while the trigger is held: a droplet every DELAY[MAYO]
/// seconds (the bottle keeps the time it is owed between steps, so the flow is the same at any step), each leaving
/// the nozzle at ROUND_SPEED[MAYO] and falling under MAYO_FALL (the game's gravity times SPEEDUP², as the stream
/// flies SPEEDUP times as fast as it once did: the same arc, flown in a fifth of the time), so that it arcs and aiming up
/// throws it further: it has no reach of its own (RANGE[MAYO] only ends one that has flown that far without landing),
/// it flies until it lands. Stimmed, the bottle is squeezed harder: STIM_SQUIRT times as fast out of the nozzle
/// and STIM_FLOW times as many droplets a second. A droplet is on a hotdog's record as MAYONNAISE.
pub const MAYO: usize = 7;
pub const MAYO_FALL: f32 = actor::GRAVITY as f32 * SPEEDUP * SPEEDUP;
pub const STIM_SQUIRT: f32 = 1.4;
pub const STIM_FLOW: f32 = 1.5;
/// How fast each weapon's rounds fall, m/s² (only the mayonnaise's do).
pub const DROOP: [f32; GUNS] = [0., 0., 0., 0., 0., 0., 0., MAYO_FALL];
/// The syrup rifle fires a burst: VOLLEY rounds VOLLEY_GAP seconds apart, each leaving where the gun points as it
/// leaves (`Player::aim`) and climbing CLIMB more than the last (added to the aim's rise before it is made a unit
/// again) as the muzzle kicks, AIMED of that while its player aims down its sights (`Player::aimed`). The gap is long
/// enough that a burst fired at one spot does not land all three on a hotdog walking across (it walks 0.125 m, about
/// the width a round can hit it in, between rounds): the shooter tracks it through the burst, and leads it. Its DELAY
/// is the whole burst and the pause after it: the next burst can follow 0.20 s after the last round, so a burst
/// starts every 0.40 s as it always did. A round of it is on a hotdog's record as SYRUP.
pub const RIFLE: usize = 6;
/// Pickups are spawn points (map.rs AMMO_BOX): one taken comes back where it was `respawn` seconds later (magazine.rs
/// COMEBACK, the same in every gun mode). A gun's pickup gives its rounds (magazine.rs PICKUP): to the gun's reserve if
/// the hotdog holds it (walked over, no key), and otherwise with the gun (E), its magazine first; a box of ammunition
/// (walked over) tops the reserve of every gun the hotdog holds up to what it carries (not the power weapons), and a
/// hotdog whose reserves are all full leaves either where it is (magazine.rs).
/// FULL is by weapon, as the guns are numbered: what its magazine holds (magazine.rs MAGAZINE), and the rounds a gun's
/// own pickup gives (but the power weapons', which give fewer: magazine.rs PICKUP).
pub const FULL: [i32; GUNS] = [0, 12, 6, 90, 4, 40, 60, 150];
/// A bot waits up to WAIT seconds longer than its walk (at actor::BOT_WALK) for a pickup it has set out for to come back.
pub const WAIT: f32 = 1.;
pub const VOLLEY: u32 = 3;
pub const VOLLEY_GAP: f32 = 0.1;
pub const CLIMB: f32 = 0.004;
pub const AIMED: f32 = 0.5;
// Camping: staying within CAMP metres for LINGER seconds summons pests over an area the size of the
// turret deck (SWARM half-extents). They sting for STING HP a second and leave once the area has been
// empty for CALM seconds. Stim lasts STIM seconds, once every STIM_WAIT.
pub const CAMP: f32 = 0.25;
pub const LINGER: f32 = 10.;
pub const SWARM: [f32; 2] = [0.255, 0.225];
pub const STING: f32 = 6.;
pub const CALM: f32 = 4.;
pub const STIM: f32 = 6.;
pub const STIM_WAIT: f32 = 20.;
// Bodies are solid (see actor.rs). A hotdog's is as wide as its bun, HOTDOG_RADIUS, and a zombie's 0.03 m, so
// two hotdogs touching are 0.09 m apart, centre to centre, and a hotdog and a zombie 0.075 m. (Against the
// level every body keeps the map's own margin, as before.) A zombie stops walking STAND_OFF from the hotdog
// it is after and bites within BITE, which is also the distance at which the cats on screen attack.
pub const HOTDOG_RADIUS: f64 = 0.045;
pub const BITE: f32 = 0.125;
pub const STAND_OFF: f32 = 0.10;
/// A zombie's body in `Feast::bodies` has the id CAT + the zombie's own; a hotdog's body has the hotdog's.
pub const CAT: u32 = 100;
// Where zombies stand to bite: eight places round their hotdog at the stand-off, then eight more one rank
// out for those who must wait their turn; each is [x, z of the direction, distance]. A zombie walks straight
// in once it is within 30 degrees of its place as seen from the hotdog; until then it goes round the back of
// the ring, OUTER from the hotdog, 60 degrees at a time (TURN is the sine of that and the cosine of 30). It
// has arrived within SETTLED.
const H: f64 = 0.7071067811865476;
const C: f64 = 0.9238795325112867;
const S: f64 = 0.3826834323650898;
const OUTER: f64 = 0.165;
const SETTLED: f64 = 0.015;
const TURN: f64 = 0.8660254037844386;
/// A zombie that had to search for its route (its goal out of sight round a block) keeps to the waypoint it
/// found for up to HOLD more steps, while the waypoint is in sight and more than NEAR metres off and its goal
/// has not moved DRIFT metres: a horde far from its hotdog does not search the counter afresh, every zombie,
/// every step.
pub const HOLD: u32 = 9;
pub const DRIFT: f64 = 0.06;
pub const NEAR: f64 = 0.02;
#[rustfmt::skip]
const PLACES: [[f64; 3]; 16] = [
    [1., 0., 0.1], [H, H, 0.1], [0., 1., 0.1], [-H, H, 0.1], [-1., 0., 0.1], [-H, -H, 0.1], [0., -1., 0.1], [H, -H, 0.1],
    [C, S, OUTER], [S, C, OUTER], [-S, C, OUTER], [-C, S, OUTER], [-C, -S, OUTER], [-S, -C, OUTER], [S, -C, OUTER], [C, -S, OUTER],
];
/// Carrying. A hotdog can pick up one loose object: the one it looks at within GRASP metres of its muzzle, or
/// failing that the nearest whose origin is within NEXT metres of its feet. It is held in front of the hotdog,
/// its underside LIFT above the hotdog's feet and its origin OUT metres ahead (by kind, in map.rs's LOOSE
/// order: far enough that the two do not overlap; held turned or tipped, further or nearer by how far its near side
/// reaches then: build.rs `Pose`), nearer where a wall is in the way; it turns with the hotdog
/// and collides with nothing, though a round meets it as it meets any loose object (a solid one stops it, a cereal box
/// lets it through slowed: see LIGHT_SPEED). Its guns are put away
/// meanwhile: the trigger throws the object along the aim (`throw_speed`: THROW[0] m/s lighter than HEAVY kilograms,
/// THROW[1] up to LOAD and slower the heavier beyond that) with LOB m/s more upwards, and the hotdog cannot strike.
/// Carrying something of HEAVY kilograms or more it walks at SLOW of its pace, and slower still beyond LOAD
/// (`burden_of`), down to LADEN. A hotdog that dies lets go.
pub const GRASP: f32 = 0.3;
pub const NEXT: f64 = 0.24;
pub const LIFT: f64 = 0.05;
pub const OUT: [f64; 16] = [0.215, 0.11, 0.11, 0.275, 0.105, 0.16, 0.16, 0.1, 0.19, 0.1, 0.21, 0.135, 0.12, 0.085, 0.085, 0.19];
pub const THROW: [f64; 2] = [1.6, 1.1];
pub const LOB: f64 = 0.5;
pub const HEAVY: f64 = 0.2;
pub const SLOW: f32 = 0.8;
pub const LOAD: f64 = 0.6;
pub const LADEN: f32 = 0.55;
/// The share of its pace a hotdog keeps carrying `mass` kilograms (see GRASP): all of it under HEAVY, SLOW up to LOAD,
/// and less beyond as the square root of LOAD over the mass, never under LADEN (a cutting board, 2.2 kg).
pub fn burden_of(mass: f64) -> f32 {
    if mass < HEAVY {
        1.
    } else if mass <= LOAD {
        SLOW
    } else {
        (SLOW * ((LOAD / mass).sqrt() as f32)).max(LADEN)
    }
}
/// How fast a loose object of `mass` kilograms is thrown (see GRASP): THROW[0] under HEAVY, THROW[1] up to LOAD and
/// slower beyond as the square root of LOAD over the mass, so that the heavier goes the shorter way.
pub fn throw_speed(mass: f64) -> f64 {
    if mass < HEAVY {
        THROW[0]
    } else if mass <= LOAD {
        THROW[1]
    } else {
        THROW[1] * (LOAD / mass).sqrt()
    }
}
/// Put down (Q), the object lands where it is held, squared and snapped to what is there and locked (build.rs).
/// Standing still it lands in the same spot every time; on the move it keeps the hotdog's own pace (`pace`, its walk
/// averaged over PACE seconds, at most KEEP m/s), and in the air its rising or falling speed too, and the physics
/// says where it ends.
pub const PACE: f32 = 0.1;
pub const KEEP: f32 = 1.1;
pub const NAMES: [&str; 5] = ["BUNNER", "FRANKIE", "MUSTARD", "REDDOG", "SESAME"];
pub const COLORS: [[f32; 3]; 5] = [
    [0.22, 0.79, 0.89],
    [1.0, 0.52, 0.23],
    [0.98, 0.82, 0.22],
    [0.91, 0.27, 0.42],
    [0.63, 0.48, 0.96],
];

// Turrets turn on their posts to track the nearest formed zombie they can see within TURRET_RANGE metres of
// the muzzle, at TURRET_TURN radians a second, and pulse at it five times a second for TURRET_DAMAGE once they
// point within TURRET_AIM radians of it. They fire at nothing else: a hotdog in the beam is neither hurt nor
// pushed. With no zombie to track they sweep TURRET_SWEEP[0] radians either side of the yaw the map gave
// them (the sweep's phase runs at TURRET_SWEEP[1] radians a second).
pub const TURRET_TURN: f32 = 2.;
pub const TURRET_AIM: f32 = 0.05;
pub const TURRET_SWEEP: [f32; 2] = [0.6, 0.5];
pub const TURRET_RANGE: f32 = 1.4;
pub const TURRET_DAMAGE: f32 = 4.;
/// A turret's beam when it points along `yaw` with nothing to aim at: from its muzzle, 0.12 m ahead of the
/// post `p` ([x, z]; the muzzle is `base` above the counter), towards chest height 0.70 m further on. Returns
/// the beam's origin and direction.
fn beam(p: [f64; 2], base: f64, yaw: f64) -> (Vec3, Vec3) {
    let (x, z) = (-yaw.sin(), -yaw.cos());
    let origin = [p[0] + x * 0.12, crate::map::FLOOR + base, p[1] + z * 0.12];
    let aim = [x * 0.70, crate::map::FLOOR + 0.08 - origin[1], z * 0.70];
    let length = (aim[0] * aim[0] + aim[1] * aim[1] + aim[2] * aim[2]).sqrt();
    (
        Vec3::new(origin[0] as f32, origin[1] as f32, origin[2] as f32),
        Vec3::new((aim[0] / length) as f32, (aim[1] / length) as f32, (aim[2] / length) as f32),
    )
}
/// Every turret prop's beam as the map places it: origin and direction (see `beam`).
pub fn turrets(map: &Map) -> Vec<(Vec3, Vec3)> {
    map.props
        .iter()
        .filter_map(|prop| match prop {
            Prop::Turret { p, base, yaw } => Some(beam(*p, *base, *yaw)),
            Prop::Box { .. } | Prop::Bin { .. } => None,
        })
        .collect()
}
/// An angle brought into -pi..pi.
fn wrap(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// What last hurt a hotdog. Whatever takes HP from a hotdog writes this record, so at the moment of a death
/// it says exactly what the cause was.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Damage {
    /// 0: nothing on record. 1 to 5: a round of that weapon (4 is the relish glob, by a direct hit or by its
    /// burst). Then MELEE (a blow with the gun in hand, or a bite), TURRET, ZOMBIE, PESTS, HUNGER, FALL (carried over
    /// the counter's edge), SYRUP (a round of the syrup rifle), MAYONNAISE (a droplet of the mayonnaise bottle) and
    /// BACKSTAB (a melee blow from behind, which kills) and BURN (standing on a hot griddle: griddle.rs).
    pub kind: u8,
    /// The hotdog that did it, or 0.
    pub attacker: u32,
    /// Where it came from: the muzzle the round left, the point a glob burst at, where the biting hotdog or
    /// the zombie stood, the middle of the swarm; for hunger and a fall, where the victim was.
    pub p: [f32; 3],
    /// The way it was travelling when it landed, as a unit vector: along the round's flight, from the burst,
    /// the biter or the zombie towards the victim, down for a fall; zero for pests and hunger.
    pub d: [f32; 3],
    /// The round's clock when it landed, and the HP it took.
    pub time: f32,
    pub amount: f32,
}
pub const MELEE: u8 = 6;
/// The HP of a bite a hotdog's heal is reckoned on (the free for all: 0.8 of what a bite took, up to this).
pub const BITE_HP: f32 = 24.;
pub const TURRET: u8 = 7;
pub const ZOMBIE: u8 = 8;
pub const PESTS: u8 = 9;
pub const HUNGER: u8 = 10;
pub const FALL: u8 = 11;
/// A round of the syrup rifle (weapon 6, whose number MELEE already has).
pub const SYRUP: u8 = 12;
/// A droplet of the mayonnaise bottle (weapon 7, whose number TURRET already has).
pub const MAYONNAISE: u8 = 13;
/// A melee blow from behind (see `backstab`): on a hotdog's record in place of MELEE, with the same attacker, place and
/// direction, so that a client can mark it (it kills, unless the victim is spared). Every reader of kinds that knows
/// MELEE as a bite knows this as one.
pub const BACKSTAB: u8 = 14;
/// Standing on a hot griddle (griddle.rs): the attacker is the hotdog that last switched it on (0 for one that burned on its
/// own doing), the place is where the victim stood and the direction nothing.
pub const BURN: u8 = 15;
/// A blow of the titan's (titan.rs: a step, a charge, a smash, a swat, a stomp, a kick, a landing): the attacker is the
/// titan, and the titan's view says which move it was.
pub const TITAN: u8 = 17;
/// Added to a round's kind on a hotdog's record (`kind_of`: the ketchup sniper's 2 becomes 66) when it struck the face
/// for more than a body hit would have done (FACE): a client marks it. Every other part of the record is the round's.
/// `plain_kind` takes it off again; Kung Fu's BLOW (16) and every other kind is below it.
pub const FACE_SHOT: u8 = 0x40;
/// A hotdog's most HP, and a zombie's (emergence.rs): a melee blow from the front takes half of these, and one from
/// behind all of them.
pub const MAX_HP: f32 = 100.;
pub const ZOMBIE_HP: f32 = 70.;
/// How far either side of directly behind a hotdog (or a zombie) its rear arc reaches: a melee blow struck from
/// inside it (120 degrees in all) is a backstab.
pub const REAR_ARC: f32 = std::f32::consts::FRAC_PI_3;
/// Whether something standing at `at` is in the rear arc of a body at `victim` facing `yaw` (`Player::yaw`, which is
/// also a zombie's: it faces (-sin yaw, -cos yaw)), measured flat: the angle between directly behind the body and
/// the way `at` lies from it is no more than REAR_ARC. With `at` right on top of the victim (no way to measure) the
/// way the blow was aimed, `along`, stands for the way the attacker faces: behind a body that is looking the same way.
pub fn backstab(victim: [f32; 3], yaw: f32, at: Vec3, along: Vec3) -> bool {
    let behind = Vec3::new(yaw.sin(), 0., yaw.cos());
    let away = Vec3::new(at.x - victim[0], 0., at.z - victim[2]);
    let away = if away.length_squared() > 1e-8 { away } else { Vec3::new(-along.x, 0., -along.z) };
    away.try_normalize().is_some_and(|away| away.dot(behind) >= REAR_ARC.cos() - 1e-6)
}
/// The kind of damage a round of weapon `w` does, on a hotdog's record.
pub fn kind_of(w: usize) -> u8 {
    match w {
        RIFLE => SYRUP,
        MAYO => MAYONNAISE,
        _ => w as u8,
    }
}
impl Damage {
    pub(crate) fn by(kind: u8, attacker: u32, p: Vec3, d: Vec3) -> Damage {
        Damage { kind, attacker, p: p.to_array(), d: d.normalize_or_zero().to_array(), ..Default::default() }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Stage {
    Warmup,
    Wave,
    FreeForAll,
    RoundOver,
    /// Forts (forts.rs): the build phase, behind the forcefield, and the fight over the bacon.
    Build,
    Fight,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Player {
    pub id: u32,
    pub name: String,
    pub p: [f32; 3],
    pub yaw: f32,
    pub hp: f32,
    pub alive: bool,
    pub bot: bool,
    pub consumed: bool,
    pub score: u32,
    pub kills: u32,
    pub meals: u32,
    pub weapon: usize,
    /// The gun most recently in hand; a third gun picked up replaces it.
    pub gun: usize,
    /// The rounds in each gun's magazine (magazine.rs; -1 for the strike, which needs none).
    #[serde(deserialize_with = "per_weapon")]
    pub ammo: [i32; GUNS],
    /// The rounds each gun has in reserve, to reload from (magazine.rs CARRY). A frame does not carry them: a client
    /// hears them as `{type:'ammo'}`.
    #[serde(default, deserialize_with = "per_weapon")]
    pub reserve: [i32; GUNS],
    /// The reload under way, if any (magazine.rs): it cannot fire until it is done.
    #[serde(default)]
    pub reload: Option<crate::magazine::Reload>,
    #[serde(deserialize_with = "per_weapon")]
    pub owned: [bool; GUNS],
    pub infected: bool,
    pub prone: bool,
    /// Crouching: between standing and prone in height, pace and steadiness, and able to jump. Prone wins.
    pub crouch: bool,
    /// Aiming down its sights, the right mouse button (`net::button::SCOPE`): the syrup rifle raised to the eye, or
    /// the ketchup sniper's scope. Only a person's living hotdog with one of those two in hand, its hands free and no
    /// card table aims (`Feast::set_aimed`; each step lets go of an aim that no longer may be). A syrup burst fired aimed
    /// climbs AIMED of CLIMB a round; the other hotdogs are shown raising the gun to the eye.
    pub aimed: bool,
    /// Seconds of stim left, and seconds until it can be used again.
    pub stim: f32,
    pub stim_wait: f32,
    /// Where the hotdog is aiming above (+) or below (-) the horizon, in radians; `yaw` is the
    /// other half of the aim. Followed every tick, not only when firing.
    pub pitch: f32,
    /// Melee attacks performed so far this round: every strike (hit or miss) and every bite or meal.
    pub swings: u32,
    /// How the body moved over the last step, in m/s: pushed, thrown and falling included.
    pub v: [f32; 3],
    /// Standing on something; false while jumping, falling or flying.
    #[serde(default = "standing")]
    pub grounded: bool,
    /// Thrown by a blast or a hit and not yet at rest: its player has little control and cannot jump.
    pub launched: bool,
    /// Carried over the counter's edge to the kitchen floor (only with the open edge): it is dead.
    pub fallen: bool,
    /// What last hurt it: for a dead hotdog, what killed it.
    pub last_damage: Damage,
    /// Forts: the team it is on, 0 the Mustard Eaters and 1 the Ketchup Connoisseurs (0 in a siege).
    pub team: u8,
    /// The loose object it carries (its number among the props), or -1.
    #[serde(default = "empty_handed")]
    pub carry: i32,
    /// How it holds what it carries: turned and tipped (build.rs `Pose`). Its player's to say (a command's
    /// `button::TURN` and `button::TIP`); held as the map stood it again when it picks something up.
    #[serde(skip)]
    pub pose: build::Pose,
    /// Forts: ready for the build phase to end.
    pub ready: bool,
    /// A Forts bot's work (forts.rs).
    #[serde(skip)]
    pub job: forts::Job,
    #[serde(skip)]
    pub cooldown: f32,
    #[serde(skip)]
    pub bite_cooldown: f32,
    #[serde(skip)]
    pub camp: [f32; 2],
    #[serde(skip)]
    pub camped: f32,
    /// A bot's shopping: the pickup it is walking to, when it set out, and the pickups it has given up on.
    #[serde(skip)]
    pub errand: Option<usize>,
    #[serde(skip)]
    pub errand_at: f32,
    #[serde(skip)]
    pub shun: Vec<usize>,
    /// Its walk, averaged over PACE seconds, in m/s ([x, z]), and where it stood at the end of the last step: what
    /// it puts down keeps that pace.
    #[serde(skip)]
    pub pace: [f32; 2],
    #[serde(skip)]
    pub last: Option<[f32; 3]>,
    /// The syrup burst under way: rounds still to come, seconds to the next, and the aim of the first.
    #[serde(skip)]
    pub volley: u32,
    #[serde(skip)]
    pub volley_wait: f32,
    #[serde(skip)]
    pub volley_aim: [f32; 3],
    /// Where its gun points now, a unit vector (0 until it has aimed): a person's look (`look`, about the yaw its walk
    /// gave it), the way a bot faces to aim (`face`), and the way the trigger was pulled (`fire`). Each later round of a
    /// syrup burst leaves along it as it is when the round leaves (`volleys`), so a burst follows the aim.
    #[serde(skip)]
    pub aim: [f32; 3],
    /// How far its own shots have thrown its aim off, in radians (recoil.rs): the next shot is deflected by up to it.
    #[serde(skip)]
    pub bloom: f32,
    /// A bot's sight on its target (aim.rs).
    #[serde(skip)]
    pub sight: crate::aim::Sight,
    /// Free For All and the sandbox (rules.rs): how often it has died this round, the seconds until it is back
    /// (while dead), the seconds its spawn protection has left, and whether the sandbox has benched it (a bot
    /// beyond `bots=`: dead, eaten and never back while benched).
    pub deaths: u32,
    pub respawn: f32,
    pub shield: f32,
    #[serde(skip)]
    pub benched: bool,
}
fn standing() -> bool {
    true
}
/// A list by weapon number of whatever length into a hotdog's places (those it lacks 0, any beyond dropped): the
/// snapshot of a server that knew fewer guns still reads.
fn per_weapon<'de, D: serde::Deserializer<'de>, T: Deserialize<'de> + Default + Copy>(d: D) -> Result<[T; GUNS], D::Error> {
    let mut out = [T::default(); GUNS];
    for (slot, v) in out.iter_mut().zip(Vec::<T>::deserialize(d)?) {
        *slot = v;
    }
    Ok(out)
}
fn empty_handed() -> i32 {
    -1
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Zombie {
    pub id: u32,
    pub p: [f32; 3],
    pub yaw: f32,
    pub hp: f32,
    pub born: f32,
    /// Seconds until the zombie has risen out of its puddle. While forming it cannot be hurt and
    /// does not move or attack (see emergence.rs).
    #[serde(default)]
    pub forming: f32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Shot {
    pub id: u32,
    pub owner: u32,
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub time: f32,
    /// A shot is a fire event. Beams (the turret) have projectile 0; guns name the round they launched.
    #[serde(default)]
    pub weapon: usize,
    #[serde(default)]
    pub projectile: u32,
}
/// A condiment round in flight. `hostile` rounds were launched in the free for all and can hurt hotdogs.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Projectile {
    pub id: u32,
    pub owner: u32,
    pub weapon: usize,
    pub p: [f32; 3],
    pub v: [f32; 3],
    pub travelled: f32,
    pub hostile: bool,
    /// The round's clock when it left the muzzle, where it left it and its velocity then. A round that falls
    /// (DROOP: the mayonnaise) is wherever `arc` puts it from these, which is all a frame says of it (net.rs).
    pub born: f32,
    pub from: [f32; 3],
    pub launch: [f32; 3],
    /// Seconds it has flown.
    #[serde(skip)]
    pub age: f32,
    /// The light loose objects it has passed through (their numbers among the props), each of which slowed and
    /// weakened it (LIGHT_SPEED, LIGHT_DAMAGE), and (the sniper's) the bodies it has passed through, by body id
    /// (a hotdog's id, or CAT + a zombie's): none of them is met again.
    #[serde(skip)]
    pub soaked: Vec<usize>,
    #[serde(skip)]
    pub pierced: Vec<u32>,
}
impl Projectile {
    /// The share of its weapon's damage the round still does: LIGHT_DAMAGE for each light object it has passed.
    pub fn power(&self) -> f32 {
        LIGHT_DAMAGE.powi(self.soaked.len() as i32)
    }
}
/// Where a round falling at `fall` m/s² is `t` seconds after it left `from` at the velocity `launch`, and its velocity
/// then: exactly, whatever the steps.
pub fn arc(from: Vec3, launch: Vec3, fall: f32, t: f32) -> (Vec3, Vec3) {
    (from + launch * t - Vec3::Y * (0.5 * fall * t * t), launch - Vec3::Y * (fall * t))
}
/// How far a stream that leaves the nozzle at `speed` and falls at `fall` reaches across the level when aimed at
/// `pitch` radians above it from `height` metres above where it lands.
pub fn throw(speed: f32, fall: f32, pitch: f32, height: f32) -> f32 {
    let (vx, vy) = (speed * pitch.cos(), speed * pitch.sin());
    vx * (vy + (vy * vy + 2. * fall * height.max(0.)).sqrt()) / fall
}
/// The ways to aim a stream that leaves the nozzle at `speed` and falls at `fall` so that it passes through a point
/// `across` metres away and `rise` metres higher: the rise per metre of the low arc and of the high one, or None
/// where it does not reach.
pub fn lobs(speed: f32, fall: f32, across: f32, rise: f32) -> Option<[f32; 2]> {
    let v2 = speed * speed;
    let under = v2 * v2 - fall * (fall * across * across + 2. * rise * v2);
    if under < 0. || across <= 1e-4 {
        return None;
    }
    let root = under.sqrt();
    Some([(v2 - root) / (fall * across), (v2 + root) / (fall * across)])
}
/// What a round meets on its way in a step (`Feast::flight`): a light loose object it passes through, a zombie
/// (by its id), a hotdog or a swarm.
enum Met {
    Light(props::Hit),
    Zombie(u32),
    /// A hotdog, and where the round met its head if it did (head.rs `hit`).
    Hotdog(u32, Option<Vec3>),
    /// The titan (titan.rs), the zone the round met and where.
    Titan(u32, crate::titan::Zone, Vec3),
    Swarm(u32),
}
/// Where a round ended, or a body it hit on its way, kept briefly so renderers can splatter it.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Splat {
    pub id: u32,
    pub projectile: u32,
    pub weapon: usize,
    pub p: [f32; 3],
    pub time: f32,
}
/// Flies and parasites over a camped area; `p` is the centre of its floor.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Swarm {
    pub id: u32,
    pub p: [f32; 3],
    pub hp: f32,
    pub born: f32,
    pub empty: f32,
}
pub fn infested(swarm: &Swarm, p: [f32; 3]) -> bool {
    (p[0] - swarm.p[0]).abs() <= SWARM[0]
        && (p[2] - swarm.p[2]).abs() <= SWARM[1]
        && (p[1] - swarm.p[1]).abs() < 0.2
}
/// Aim point where a round of the given speed meets a target moving at `velocity`.
pub fn lead(origin: Vec3, target: Vec3, velocity: Vec3, speed: f32) -> Vec3 {
    let mut mark = target;
    for _ in 0..3 {
        mark = target + velocity * (mark.distance(origin) / speed);
    }
    mark
}
/// A hotdog off the ground with crouch held has its knees tucked up (actor.rs `Actor::tuck`): its body is shortened from
/// the bottom, the feet drawn up towards it, and its head stays where the jump put it. It aims and is hit as a standing
/// hotdog is (its muzzle and its chest do not dip); a glob bursting under it finds its body further off (`figure`).
pub fn tucked(p: &Player) -> bool {
    p.crouch && !p.prone && !p.grounded
}
/// A hotdog's stance as its heights go by it (CHEST, EYE, MID, LOW): 0 standing (or tucked in the air), 1 crouching on
/// the ground, 2 prone.
pub fn stance(p: &Player) -> usize {
    if p.prone {
        2
    } else if p.crouch && !tucked(p) {
        1
    } else {
        0
    }
}
pub fn chest(p: &Player) -> Vec3 {
    Vec3::from_array(p.p) + Vec3::Y * CHEST[stance(p)]
}
/// Where a hotdog's rounds and blows start: its muzzle, which is lower crouching and lower still prone.
pub fn muzzle(p: &Player) -> Vec3 {
    Vec3::from_array(p.p) + Vec3::Y * [EYE, MID, LOW][stance(p)]
}
/// A hotdog's body as a blast finds it: the bottom and the top of its capsule above its feet (HOTDOG_RADIUS round), as
/// the body is now: standing, crouched, prone, or tucked in the air (the bottom raised by what a crouch takes off). Its
/// head, which nods with the look, is the blast's too (`body_gap`).
pub fn figure(p: &Player) -> (f32, f32) {
    let (tall, low, flat) = (actor::HEIGHT as f32, actor::CROUCH_HEIGHT as f32, actor::PRONE_HEIGHT as f32);
    match stance(p) {
        2 => (0., flat),
        1 => (0., low),
        _ if tucked(p) => (tall - low, tall),
        _ => (0., tall),
    }
}
/// The way a hotdog looking `yaw` and `pitch` aims: its yaw, then its pitch, as a command's (net.rs `Command::direction`).
pub fn sightline(yaw: f32, pitch: f32) -> Vec3 {
    glam::Quat::from_rotation_y(yaw) * glam::Quat::from_rotation_x(pitch) * Vec3::NEG_Z
}
/// How far `point` is from hotdog `p`'s body as a relish glob's blast finds it (0 inside), and the body's nearest point
/// to it: the capsule `figure` gives, over its feet, HOTDOG_RADIUS round, or its head (head.rs), where the nod has it,
/// whichever is nearer.
pub fn body_gap(p: &Player, point: Vec3) -> (f32, Vec3) {
    let (low, high) = figure(p);
    let (radius, at) = (HOTDOG_RADIUS as f32, Vec3::from_array(p.p));
    let (bottom, top) = (at.y + low + radius, (at.y + high - radius).max(at.y + low + radius));
    let axis = Vec3::new(at.x, point.y.clamp(bottom, top), at.z);
    let far = point.distance(axis);
    let body = if far > radius { (far - radius, axis + (point - axis) / far * radius) } else { (0., point) };
    let head = crate::head::head_gap(p, point);
    if head.0 < body.0 { head } else { body }
}
/// The share of a relish glob's damage its blast does to a body `gap` metres from where it burst (to the nearest point of
/// the body), the blast reaching `reach`: all of it within LETHAL, falling smoothly to nothing at `reach`.
pub fn blast(gap: f32, reach: f32) -> f32 {
    if gap >= reach {
        return 0.;
    }
    let t = ((gap - LETHAL) / (reach - LETHAL).max(1e-6)).clamp(0., 1.);
    1. - t * t * (3. - 2. * t)
}
/// The kind on a hotdog's record without FACE_SHOT: the kind of what did it.
pub fn plain_kind(kind: u8) -> u8 {
    kind & !FACE_SHOT
}
/// Whether a kind on a hotdog's record was a round in the face (FACE_SHOT).
pub fn faced(kind: u8) -> bool {
    kind & FACE_SHOT != 0
}
/// A pickup's spawn point: where it is, what it holds (1 to 6 a gun with `ammo` rounds, 0 a box of ammunition), the
/// round's time it is there again from (`ready`) and how long it takes to come back once taken (`respawn`; in Forts a
/// power weapon's is drawn afresh each time it is taken, `Feast::comeback`, and is the time it was last given).
#[derive(Clone, Serialize, Deserialize)]
pub struct Pickup {
    pub id: usize,
    pub p: [f32; 3],
    pub weapon: usize,
    pub ammo: i32,
    pub ready: f32,
    #[serde(default)]
    pub respawn: f32,
}
/// A loose object's hard impact, for the clang: which prop, what it hit (another prop's number,
/// `props::GROUND`, or `props::BLOCK - n` for the map's block n), where, and how fast they closed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Knock {
    pub id: u32,
    pub prop: u32,
    pub other: i32,
    pub p: [f32; 3],
    pub speed: f32,
    pub time: f32,
}
/// Puts a map's loose objects out: each stands with its underside `base` above the counter, turned by its
/// yaw, and is numbered by its place among them; those that rest where they are put start asleep. The shapes are
/// registered in LOOSE's order. The props share the bodies' gravity and kitchen floor; with `field` (a Forts round's
/// forcefield standing) the field is their last block.
pub(crate) fn furnish(level: &Level, config: &actor::Config, field: bool) -> PropWorld {
    // (A Forts round's forcefield stands as the props' last block: see forts.rs.)
    let mut world = PropWorld::new(&forts::Fielded(level, field), props::Config { gravity: config.gravity, floor_y: config.kitchen_floor, ..Default::default() });
    for kind in LOOSE {
        world.add_shape(&ShapeSpec::named(kind).expect("every loose kind has a shape"));
    }
    for item in &level.map.loose {
        if let Some(kind) = LOOSE.iter().position(|kind| *kind == item.kind) {
            world.add(kind, [item.p[0], crate::map::FLOOR + item.base, item.p[1]], props::yaw_rotation(item.yaw));
        }
    }
    // What the map puts out at rest (on the counter, on a deck, piled up) starts asleep.
    world.rest_where_put(&forts::Fielded(level, field));
    world
}
/// A step of a whole number of 1/120 s (a single-precision 1/30, say) is taken as exactly that, as the server's is.
fn whole(dt: f64) -> f64 {
    let whole = (dt * 120.).round() / 120.;
    if whole > 0. && (whole - dt).abs() < 1e-6 { whole } else { dt }
}
/// The props' side of a body's step (`actor::Props`): the body is kept out of the props, stands on them, and
/// shoves them with the speed it really came at, which for a hotdog a person steers (walked before the step, by
/// `walk`) is known only from where it was last time.
struct Lean<'a> {
    props: &'a mut PropWorld,
    leans: &'a mut HashMap<u32, [f64; 2]>,
}
impl actor::Props for Lean<'_> {
    fn resolve(&mut self, body: &actor::Body, _from: [f64; 3], velocity: [f64; 3], dt: f64) -> actor::Touch {
        let (dt, mut v) = (whole(dt), velocity);
        if let Some(last) = self.leans.get(&body.id) {
            let (dx, dz) = (body.p[0] - last[0], body.p[2] - last[1]);
            if dx * dx + dz * dz < 9. * dt * dt {
                v = [dx / dt, velocity[1], dz / dt];
            }
        }
        let r = self.props.resolve_character(&props::Character::weighing(body.p, v, body.radius, body.height, body.mass), dt);
        self.leans.insert(body.id, [r.position[0], r.position[2]]);
        actor::Touch { p: r.position, support: r.support.map(|support| support.height), impulse: r.impulse }
    }
}
/// `Lean` for one body only: nobody else touches the props.
struct Only<'a> {
    me: u32,
    lean: Lean<'a>,
}
impl actor::Props for Only<'_> {
    fn resolve(&mut self, body: &actor::Body, from: [f64; 3], velocity: [f64; 3], dt: f64) -> actor::Touch {
        if body.id == self.me { self.lean.resolve(body, from, velocity, dt) } else { actor::Touch { p: body.p, support: None, impulse: None } }
    }
}
/// The guns a box of ammunition would top up for a hotdog holding `owned` with `reserve` rounds in reserve: those whose
/// reserve is short of what it carries (magazine.rs CARRY), but not the power weapons (magazine.rs POWER), in every gun
/// mode. The client's prompt at a box asks the same.
pub fn refills(owned: &[bool; GUNS], reserve: &[i32; GUNS]) -> Vec<usize> {
    (1..GUNS).filter(|g| owned[*g] && reserve[*g] < crate::magazine::CARRY[*g] && !crate::magazine::power(*g)).collect()
}
/// The line at the top of the screen as a siege begins.
pub const SIEGE_ALERT: &str = "ONE WAVE. FIVE HOTDOGS. STAY TOGETHER.";
/// Fully Loaded (cards.rs): where each of `seats` hotdogs stands at the card table, its feet and the yaw that faces the
/// table's middle, in seat order. The table stands on the top of the map's first microwave, or (a map without one) on
/// a top the size of Counter Siege's, 0.5 x 0.33 m, about the first start, at the ground there. A hotdog stands on a
/// block at the height the bodies give it (`actor::World`): the block's top and its lip (`map::LIP`), a body placed
/// lower being inside the block. The seats are an oval round the middle of the top at equal angles (the front seat,
/// on the side the microwave faces, first, the rest going clockwise seen from above): the oval is 0.17 m wide and
/// 0.125 m deep on Counter Siege's, less on a smaller top, so that a hotdog (0.045 m wide) stays on it.
pub fn table_seats(level: &Level, seats: usize) -> Vec<([f32; 3], f32)> {
    let (rect, front, top) = match level.map.blocks.iter().find(|b| b.style == "microwave") {
        Some(block) => (block.rect, block.facing.unwrap_or(Side::MinusZ), Some(crate::map::FLOOR + block.height + crate::map::LIP)),
        None => {
            let (start, _) = level.start(0);
            let (x, z) = (start.x as f64, start.z as f64);
            ([x - 0.25, z - 0.165, x + 0.25, z + 0.165], Side::MinusZ, None)
        }
    };
    let centre = [(rect[0] + rect[2]) / 2., (rect[1] + rect[3]) / 2.];
    let (rx, rz) = (0.17_f64.min((rect[2] - rect[0]) / 2. - 0.08).max(0.), 0.125_f64.min((rect[3] - rect[1]) / 2. - 0.04).max(0.));
    // The oval's reach towards the microwave's front, and across it (to the front's right, seen from above).
    let (ahead, across, reach, beside) = if front.along_z() { ([0., front.sign()], [-front.sign(), 0.], rz, rx) } else { ([front.sign(), 0.], [0., front.sign()], rx, rz) };
    (0..seats)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / seats as f64;
            let at = [0, 1].map(|k| centre[k] + ahead[k] * reach * a.cos() + across[k] * beside * a.sin());
            // A hotdog looks along (-sin yaw, -cos yaw).
            let yaw = (at[0] - centre[0]).atan2(at[1] - centre[1]);
            ([at[0] as f32, top.unwrap_or_else(|| level.ground(at[0], at[1])) as f32, at[1] as f32], yaw as f32)
        })
        .collect()
}
pub struct Feast {
    pub players: Vec<Player>,
    pub enemies: Vec<Zombie>,
    pub pickups: Vec<Pickup>,
    pub shots: Vec<Shot>,
    pub projectiles: Vec<Projectile>,
    pub splats: Vec<Splat>,
    pub pests: Vec<Swarm>,
    pub stage: Stage,
    pub elapsed: f32,
    pub famine_at: f32,
    pub warmup: f32,
    pub wave_spawned: u32,
    pub wave_kills: u32,
    pub winner: Option<u32>,
    pub alert: String,
    pub friendly_fire_events: u32,
    pub demo: bool,
    pub round: u32,
    pub over_at: f32,
    /// Zombie arrivals under way: the microwave's ding, the ooze, the puddles and what forms in them.
    pub emergences: Vec<Emergence>,
    pub(crate) next_emergence: u32,
    next_shot: u32,
    next_round: u32,
    next_splat: u32,
    next_pest: u32,
    demo_shot: bool,
    /// The arena: collision, cover, routes, starts, spawns and pickups all come from its map.
    pub level: Level,
    /// Origin and direction of each turret's beam, as it points now.
    pub turrets: Vec<(Vec3, Vec3)>,
    /// The yaw each turret has turned to, in the map's order: what a renderer turns the model by.
    pub turret_yaw: Vec<f32>,
    /// Each turret's post ([x, z]), the height of its muzzle above the counter, and the yaw the map gave it.
    posts: Vec<([f64; 2], f64, f32)>,
    /// The bodies of the living hotdogs and the formed zombies: they are solid to each other and to the level,
    /// they fall, and blasts throw them. `bodies.config.open_edge` lets a throw carry one off the counter.
    pub bodies: actor::World,
    /// Where each body was last reported, so a position written into a player or a zombie from outside shows.
    pub(crate) echo: HashMap<u32, [f32; 3]>,
    /// The map's loose objects (the bowl, crates, pillows...), as rigid bodies: see props.rs. They fall under
    /// the bodies' gravity, are shoved by hotdogs and zombies, stop rounds (or, a light one, slow them: see
    /// LIGHT_SPEED) and are nudged by them, are thrown
    /// by a glob's burst, and are lost over the counter's edge. A prop's number is its place in `map.loose`.
    pub props: PropWorld,
    /// The props' hard impacts of the last 0.4 s.
    pub knocks: Vec<Knock>,
    next_knock: u32,
    /// Where each body stood when the props last looked at it.
    leans: HashMap<u32, [f64; 2]>,
    /// A finished siege round is followed by the next after eight seconds, as in the demo (the server sets this;
    /// single player waits for Enter).
    pub follow: bool,
    /// The waypoint each zombie is keeping to (see HOLD), by its body's id: the waypoint, the goal it was
    /// searched for, both [x, z], and the steps it still holds.
    pub(crate) routes: HashMap<u32, ([f64; 2], [f64; 2], u32)>,
    /// Forts (forts.rs): the mode's state while the round is played as Forts, None otherwise; and the seconds a Forts
    /// round's build phase lasts and a fight may stay quiet before the microwave goes off (the rules': forts.rs `set_build`
    /// and `set_stale`; a new round, and a Forts round set out later, keep them).
    pub fort: Option<Fort>,
    pub(crate) fort_build: f32,
    pub(crate) fort_stale: f32,
    /// The draped napkins (drape.rs), the next one's number, the napkins that came to rest and did not drape
    /// (looked at again once they wake), and the map's own level without walls (a siege's: Forts keeps its own).
    pub drapes: Vec<drape::Drape>,
    pub(crate) next_drape: u32,
    pub(crate) refused: std::collections::HashSet<usize>,
    pub(crate) base: Option<Level>,
    /// What the game is played by beyond its map (rules.rs): Free For All, the dev sandbox. A new round keeps them.
    pub rules: crate::rules::Rules,
    /// Free For All: the round's clock when it ends, whoever leads (0: no limit, as in the sandbox).
    pub ends: f32,
    /// The numbers the guns and the hotdogs go by (tune.rs): the constants, unless the sandbox's panel or the
    /// server's --tune changed them. A new round keeps them.
    pub tune: crate::tune::Tuning,
    /// Fully Loaded (cards.rs): the card table, while the rules (`rules.cards`) say the hotdogs play it, and the seed
    /// its shuffles come from (with the round's number; a new round keeps it). At a table nothing else is played: no
    /// cats, no hunger, no guns, no walking. Free For All's spots and respawns are drawn from the same seed
    /// (spawns.rs).
    pub cards: Option<cards::Table>,
    pub card_seed: u64,
    /// Free For All (spawns.rs): the places a hotdog may appear on this map, surveyed once and kept by every round
    /// after; where each hotdog (by seat) began the round, where `fit` puts it back; and where each last appeared.
    pub(crate) spots: Vec<Vec3>,
    pub(crate) openings: Vec<crate::spawns::Spot>,
    pub(crate) came_back: HashMap<u32, Vec2>,
    /// The map's electric griddles (griddle.rs), in the map's order of blocks: off and at room temperature when a round
    /// begins, heated, cooled and flipped by `step` and `use_griddle`.
    pub griddles: Vec<crate::griddle::Griddle>,
    /// Kung Fu (kungfu.rs): the fighters' states, while the rules (`rules.kungfu`) say the hotdogs fight hand to hand.
    pub kungfu: Option<crate::kungfu::Dojo>,
    /// Titan (titan.rs): the titan's state, while the rules (`rules.titan`) say one giant hotdog fights the rest; and the
    /// titan of the round before (a new round's choice rotates from it).
    pub titan: Option<crate::titan::Titan>,
    pub(crate) titan_previous: Option<u32>,
}
impl Feast {
    /// A round on the map the game ships with.
    pub fn new(demo: bool) -> Self {
        Self::with_map(Map::bundled(), demo)
    }
    pub fn with_map(map: Map, demo: bool) -> Self {
        Self::on(Level::new(map), demo)
    }
    /// A round on a prepared level, in the mode the map was made for (a Forts map's round is set out as Forts: forts.rs
    /// `fortify`; `set_rules` plays it in another). The five hotdogs share the map's starts in order, each with the
    /// loadout of every gun mode (magazine.rs `loadout`), and the map's pickups give and come back as every gun mode
    /// has them (magazine.rs PICKUP and COMEBACK, whatever the map's `ammo` and `respawn` say).
    pub fn on(level: Level, demo: bool) -> Self {
        let players = (0..NAMES.len())
            .map(|i| {
                let (p, yaw) = level.start(i);
                let mut player = Player { id: i as u32 + 1, name: NAMES[i].into(), p: p.to_array(), yaw, hp: 100., alive: true, bot: demo || i > 0, camp: [p.x, p.z], grounded: true, carry: -1, ..Default::default() };
                crate::magazine::loadout(&mut player);
                player
            })
            .collect();
        let pickups = level
            .map
            .pickups
            .iter()
            .enumerate()
            .map(|(id, item)| {
                let (ammo, respawn) = crate::magazine::stock(item.weapon);
                Pickup { id, p: level.pickup(id).to_array(), weapon: item.weapon, ammo, ready: 0., respawn }
            })
            .collect();
        let turrets = turrets(&level.map);
        let posts: Vec<([f64; 2], f64, f32)> = level
            .map
            .props
            .iter()
            .filter_map(|prop| match prop {
                Prop::Turret { p, base, yaw } => Some((*p, *base, *yaw as f32)),
                Prop::Box { .. } | Prop::Bin { .. } => None,
            })
            .collect();
        let bodies = actor::World::default();
        let props = furnish(&level, &bodies.config, false);
        let griddles = crate::griddle::of(&level.map);
        let mut feast = Self {
            players,
            enemies: vec![],
            projectiles: vec![],
            splats: vec![],
            pests: vec![],
            next_round: 1,
            next_splat: 1,
            next_pest: 1,
            pickups,
            shots: vec![],
            stage: Stage::Warmup,
            elapsed: 0.,
            famine_at: 0.,
            warmup: if demo { 3. } else { 12. },
            wave_spawned: 0,
            wave_kills: 0,
            winner: None,
            alert: SIEGE_ALERT.into(),
            friendly_fire_events: 0,
            demo,
            round: 1,
            over_at: 0.,
            emergences: vec![],
            next_emergence: 1,
            next_shot: 1,
            demo_shot: false,
            level,
            turrets,
            turret_yaw: posts.iter().map(|post| post.2).collect(),
            posts,
            bodies,
            echo: HashMap::new(),
            props,
            knocks: vec![],
            next_knock: 1,
            leans: HashMap::new(),
            follow: false,
            routes: HashMap::new(),
            fort: None,
            fort_build: crate::map::BUILD as f32,
            fort_stale: forts::STALE,
            drapes: vec![],
            next_drape: 1,
            refused: Default::default(),
            base: None,
            rules: Default::default(),
            ends: 0.,
            tune: Default::default(),
            cards: None,
            card_seed: 0,
            spots: vec![],
            openings: vec![],
            came_back: HashMap::new(),
            griddles,
            kungfu: None,
            titan: None,
            titan_previous: None,
        };
        feast.base = Some(feast.level.clone());
        // A map made for Forts is played by its rules, with six hotdogs (unless the rules say otherwise: `set_rules`).
        if Mode::made_for(&feast.level.map) == Mode::Forts {
            feast.fortify();
        }
        // A Kung Fu map (the dojo) is played as Kung Fu (kungfu.rs), and a titan arena as Titan (titan.rs).
        if feast.level.map.mode == crate::kungfu::MODE {
            feast.set_rules(crate::rules::Rules { kungfu: true, ..Default::default() });
        } else if feast.level.map.mode == crate::titan::MODE {
            feast.set_rules(crate::rules::Rules { titan: true, ..Default::default() });
        }
        feast
    }
    /// Starts the next round on the same map, with the same rules for the bodies.
    pub fn reset(&mut self) {
        let (round, config, follow, seed) = (self.round + 1, self.bodies.config.clone(), self.follow, self.card_seed);
        let times = (self.fort_build, self.fort_stale);
        let fort = self.fort.take();
        let rules = std::mem::take(&mut self.rules);
        let tune = std::mem::take(&mut self.tune);
        // (Kung Fu: what the people chose to fight in, and the events' numbers, outlast the round.)
        let dojo = self.kungfu.take();
        // (Titan: who was the titan, so that the next is chosen by turns.)
        let titan = self.titan_last();
        // (The same map: Free For All's spots stand as they were surveyed.)
        let spots = std::mem::take(&mut self.spots);
        // (Without the drapes' walls: every loose object goes back where the map has it.)
        *self = Self::on(self.base.clone().unwrap_or_else(|| self.level.clone()), self.demo);
        (self.tune, self.spots) = (tune, spots);
        (self.round, self.follow, self.card_seed) = (round, follow, seed);
        // (Whatever the map's own setting out cast, the round is set out again by the rules below: a titan by turns from
        // the last round's, and on a titan arena played as another mode, the titan's roster let go.)
        self.titan_previous = titan;
        if let Some(t) = self.titan.as_mut() {
            t.previous = titan;
        }
        (self.fort_build, self.fort_stale) = times;
        self.bodies.config = config;
        self.props = furnish(&self.level, &self.bodies.config, self.fort.as_ref().is_some_and(|fort| fort.up));
        // The mode, Free For All, the sandbox and Fully Loaded set the round out their own way (rules.rs): a new deal for
        // a table, Forts' teams and forcefield where the rules ask for Forts. Then, Forts: the match goes on.
        self.set_rules(rules);
        self.inherit(fort);
        self.inherit_dojo(dojo);
    }
    // ----- Fully Loaded (cards.rs).

    /// Sets a round out as Fully Loaded: a table for the hotdogs, shuffled from the card seed and the round's number
    /// (so a new round is a new game), and the hotdogs standing at it (`table_seats`). A person has no time limit
    /// unless the server is following rounds (`follow`, online), when they have `cards::TURN` seconds for a
    /// decision; a bot's seat is a bot's at the table. The stage stays at the start and nothing is said: there is no
    /// wave to come. Called by `set_rules`.
    pub(crate) fn lay_table(&mut self) {
        let mut table = cards::Table::new(self.players.len(), self.card_seed ^ (self.round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        table.set_limit(self.follow.then_some(cards::TURN));
        for (seat, p) in self.players.iter().enumerate() {
            table.set_bot(seat, p.bot);
        }
        self.cards = Some(table);
        self.stage = Stage::Warmup;
        self.alert.clear();
        self.sit();
    }
    /// Stands every hotdog and its body at its seat, facing the table's middle (`table_seats`).
    fn sit(&mut self) {
        let seats = table_seats(&self.level, self.players.len());
        for (p, (at, yaw)) in self.players.iter_mut().zip(seats) {
            (p.p, p.yaw) = (at, yaw);
        }
        self.embody();
    }
    /// One step of a round of Fully Loaded: the hotdogs stay in their seats, the table's clock runs (a seat whose
    /// hotdog is a bot is a bot's), and once somebody has won the round is over as any other is, and the next
    /// follows in its own time.
    fn cards_step(&mut self, dt: f32) {
        self.sit();
        let Some(table) = self.cards.as_mut() else {
            return;
        };
        for (seat, p) in self.players.iter().enumerate() {
            table.set_bot(seat, p.bot);
        }
        table.tick(dt);
        let Some(seat) = table.winner() else {
            return;
        };
        let total = table.view().seats[seat].total;
        self.stage = Stage::RoundOver;
        self.over_at = self.elapsed;
        if let Some(p) = self.players.get(seat) {
            self.winner = Some(p.id);
            self.alert = format!("{} IS FULLY LOADED: {total} POINTS", p.name.to_uppercase());
        }
    }
    /// A hotdog's flip, stop or choice of a hotdog at the card table. The seat is the hotdog's place in `players`.
    /// False if there is no table, no such hotdog, or the table refuses it (not its go, or not a thing to ask just
    /// now): then nothing has changed. (Whose hotdog is whose is for the server to know.)
    pub fn cards_command(&mut self, id: u32, command: cards::Command) -> bool {
        let Some(seat) = self.players.iter().position(|p| p.id == id) else {
            return false;
        };
        self.cards.as_mut().is_some_and(|table| table.command(seat, command).is_ok())
    }
    /// The body of a hotdog (by its id) or of a zombie (by CAT + its id), while it has one.
    pub fn body(&self, id: u32) -> Option<&Actor> {
        self.bodies.get(id)
    }
    /// Brings the bodies up to date with the roster. Every living hotdog and every formed zombie has one; the
    /// dead and the forming have none, so a puddle neither blocks nor is pushed. A position written straight
    /// into a player or a zombie (a start, a spawn, a test) puts its body there.
    pub(crate) fn embody(&mut self) {
        let Feast { players, enemies, bodies, echo, .. } = self;
        let mut keep = |id: u32, p: [f32; 3], cat: bool| {
            if bodies.get(id).is_some() && echo.get(&id) == Some(&p) {
                return;
            }
            let at = [p[0] as f64, p[1] as f64, p[2] as f64];
            match bodies.get_mut(id) {
                Some(body) => body.place(Vec3::from_array(p)),
                None => {
                    bodies.add(if cat { Actor::zombie(id, at) } else { Actor { radius: HOTDOG_RADIUS, ..Actor::hotdog(id, at) } });
                }
            }
            echo.insert(id, p);
        };
        for p in players.iter().filter(|p| p.alive) {
            keep(p.id, p.p, false);
        }
        for e in enemies.iter().filter(|e| e.forming <= 0.) {
            keep(CAT + e.id, e.p, true);
        }
        bodies.actors.retain(|body| {
            if body.id < CAT {
                players.iter().any(|p| p.id == body.id && p.alive)
            } else {
                enemies.iter().any(|e| CAT + e.id == body.id && e.forming <= 0.)
            }
        });
        echo.retain(|id, _| bodies.get(*id).is_some());
        for p in players.iter() {
            if let Some(body) = bodies.get_mut(p.id) {
                body.prone = p.prone;
                body.crouch = p.crouch;
            }
        }
        // The titan's body is its size and held where titan.rs has it.
        self.titan_body();
    }
    /// Copies where the bodies are, and how they are moving, back into the roster.
    pub(crate) fn report(&mut self) {
        // (The titan is where titan.rs puts it: the bodies' world never moves it.)
        let titan = self.titan_id().unwrap_or(0);
        let Feast { players, enemies, bodies, echo, .. } = self;
        let mut lost = vec![];
        for p in players.iter_mut() {
            if p.id == titan {
                continue;
            }
            let Some(body) = bodies.get(p.id) else {
                (p.v, p.grounded, p.launched) = ([0.; 3], true, false);
                continue;
            };
            p.p = body.position().to_array();
            p.v = body.velocity().to_array();
            (p.grounded, p.launched, p.fallen) = (body.grounded, body.launched, body.fallen);
            echo.insert(p.id, p.p);
            if body.fallen {
                lost.push(p.id);
            }
        }
        let mut gone = vec![];
        for e in enemies.iter_mut() {
            if let Some(body) = bodies.get(CAT + e.id) {
                e.p = body.position().to_array();
                echo.insert(CAT + e.id, e.p);
                if body.fallen {
                    gone.push(e.id);
                }
            }
        }
        // Over the edge, which only an open counter edge allows: the fall kills a hotdog and removes a zombie.
        for id in lost {
            let at = self.player(id).map_or(Vec3::ZERO, |p| Vec3::from_array(p.p));
            // (Kung Fu: knocked off the counter is a ring-out, the last hitter's KO.)
            let by = self.ring_credit(id);
            self.harm(id, f32::INFINITY, false, Damage::by(FALL, by, at, Vec3::NEG_Y));
        }
        for id in gone {
            self.enemies.retain(|e| e.id != id);
            self.wave_kills += 1;
        }
        self.titan_body();
    }
    /// Moves every body by one step: walking where it was steered, colliding, falling, flying. Nobody goes
    /// on walking afterwards unless steered afresh.
    /// The props move first, by what was done to them since the last step; then the bodies walk, and each is
    /// put outside every prop as it goes (`Lean`), so a step ends with nobody inside a prop.
    pub(crate) fn settle(&mut self, dt: f32) {
        for event in self.props.step(&self.level, whole(dt as f64)) {
            if event.kind == props::EventKind::Impact {
                self.knocks.push(Knock { id: self.next_knock, prop: event.id as u32, other: event.other, p: event.point.map(|v| v as f32), speed: event.speed as f32, time: self.elapsed });
                self.next_knock += 1;
            }
        }
        // A napkin that has come to rest over other loose objects drapes (drape.rs), and (Forts) whatever has come to
        // rest in a team's base is moved out of it (build.rs).
        self.drape();
        self.clear_bases();
        self.bodies.step(&self.level, whole(dt as f64), &mut Lean { props: &mut self.props, leans: &mut self.leans });
        for body in &mut self.bodies.actors {
            body.wish = [0.; 2];
        }
        let bodies = &self.bodies;
        self.leans.retain(|id, _| bodies.get(*id).is_some());
        self.report();
        self.haul();
        self.stride(dt);
    }
    /// Each hotdog's pace, its walk averaged over PACE seconds and at most KEEP m/s, from where it stood at the end
    /// of the last step: whatever moved it (its own walk, a shove), but not a jump no walk could make.
    /// What it puts down keeps it.
    fn stride(&mut self, dt: f32) {
        if dt <= 0. {
            return;
        }
        let k = (dt / PACE).min(1.);
        for p in &mut self.players {
            let was = p.last.unwrap_or(p.p);
            let (mut vx, mut vz) = if p.alive { ((p.p[0] - was[0]) / dt, (p.p[2] - was[2]) / dt) } else { (0., 0.) };
            let fast = vx.hypot(vz);
            // A jump further than any walk (a start, a test putting it somewhere) is no pace at all.
            if fast > KEEP * 2.5 {
                p.last = Some(p.p);
                continue;
            }
            if fast > KEEP {
                (vx, vz) = (vx * KEEP / fast, vz * KEEP / fast);
            }
            p.pace = [p.pace[0] + (vx - p.pace[0]) * k, p.pace[1] + (vz - p.pace[1]) * k];
            p.last = Some(p.p);
        }
    }
    /// Network play, on the client, whose props are a copy of the server's and whose bodies are its own copy
    /// (`bodies`): the props move as in `settle`, then the bodies do, and the one the client steers (`me`)
    /// meets the props as it would on the server. The others are where the server last had them, and what
    /// they do to the props comes from the server.
    pub fn foresee(&mut self, bodies: &mut actor::World, me: u32, dt: f64) {
        self.props.step(&self.level, whole(dt));
        self.lean(bodies, me, dt);
    }
    /// The bodies' half of `foresee`: the props are left where they are.
    pub fn lean(&mut self, bodies: &mut actor::World, me: u32, dt: f64) {
        bodies.step(&self.level, whole(dt), &mut Only { me, lean: Lean { props: &mut self.props, leans: &mut self.leans } });
    }
    /// Network play, on the client: one step of the hotdog it steers (`me`, in its own copy of the bodies), as the
    /// server takes one of its commands (crates/server, `Game::command`): its stance, its walk by `step` ([x, z]
    /// metres), its jump (a take-off speed, or 0), then the step itself, in which it falls, flies and meets the
    /// props. `props` says whether the props move in this step too (a step forward), or stay as they are (a step
    /// taken again from the server's word, in `rehearse`).
    pub fn predict(&mut self, bodies: &mut actor::World, me: u32, stance: (bool, bool), step: Vec2, jump: f64, dt: f64, props: bool) {
        if bodies.extra != self.bodies.extra {
            bodies.extra = self.bodies.extra.clone();
        }
        let Some(body) = bodies.get_mut(me) else {
            return;
        };
        (body.prone, body.crouch) = stance;
        bodies.walk(&self.level, me, step.x as f64, step.y as f64);
        if jump > 0. {
            if let Some(body) = bodies.get_mut(me) {
                body.jump = jump;
            }
        }
        if props {
            self.foresee(bodies, me, dt);
        } else {
            self.lean(bodies, me, dt);
        }
    }
    /// Runs `f` on a copy of the props (and of where the props last saw each body), and puts them back as they were
    /// afterwards, except where they last saw `me`: the client steps its own body again from the server's word
    /// through the props as they stand, without shoving them twice.
    pub fn rehearse<T>(&mut self, me: u32, f: impl FnOnce(&mut Feast) -> T) -> T {
        let (props, mut leans) = (self.props.clone(), self.leans.clone());
        let out = f(self);
        match self.leans.get(&me) {
            Some(at) => leans.insert(me, *at),
            None => leans.remove(&me),
        };
        (self.props, self.leans) = (props, leans);
        out
    }
    /// How far a round from `origin` along the unit vector `dir` gets before something solid stops it: a block, or a
    /// solid loose object. Light loose objects (a pillow) let it through (see LIGHT_SPEED), so they are no cover: not
    /// from a round, not from a glob's splash, and a bot holds no fire for one.
    pub fn cover(&self, origin: Vec3, dir: Vec3) -> f32 {
        let wall = self.level.cover_from(origin, dir);
        self.props.raycast_solid(origin.as_dvec3().to_array(), dir.as_dvec3().to_array(), wall as f64).map_or(wall, |hit| hit.distance as f32)
    }
    /// How far a blow (or the turret's beam) from `origin` along `dir` gets: a block stops it, and so does any loose
    /// object, light or solid.
    pub fn screen(&self, origin: Vec3, dir: Vec3) -> f32 {
        let wall = self.level.cover_from(origin, dir);
        self.props.raycast(origin.as_dvec3().to_array(), dir.as_dvec3().to_array(), wall as f64, None).map_or(wall, |hit| hit.distance as f32)
    }
    /// The loose objects along a round's way from `origin` along `dir` within `reach` metres: the first solid one, if
    /// any, and the light ones before it, nearest first. Those in `skip` (the light ones it has passed already) are
    /// not met again.
    pub(crate) fn solid_along(&self, origin: Vec3, dir: Vec3, reach: f32, skip: &[usize]) -> (Option<props::Hit>, Vec<props::Hit>) {
        let (o, d) = (origin.as_dvec3().to_array(), dir.as_dvec3().to_array());
        let (mut past, mut lights) = (skip.to_vec(), vec![]);
        while let Some(hit) = self.props.raycast_past(o, d, reach as f64, &past) {
            if self.props.material(hit.id) == props::Material::Solid {
                return (Some(hit), lights);
            }
            past.push(hit.id);
            lights.push(hit);
        }
        (None, lights)
    }
    /// Points a body at `target` by the level's route, up a ramp when the target stands higher, at `pace`
    /// metres a second; returns the way it heads as [x, z]. The step that follows does the walking.
    pub(crate) fn steer(&mut self, body: u32, target: [f64; 3], pace: f64) -> Option<[f64; 2]> {
        let old = self.bodies.get(body)?.p;
        let goal = self.level.climb_goal(old, target).unwrap_or(target);
        // Without a route (its own cell is inside a block's clearance, say) it heads straight there.
        let next = self.level.path([old[0], old[2]], [goal[0], goal[2]]).first().copied().unwrap_or([goal[0], goal[2]]);
        let (dx, dz) = (next[0] - old[0], next[1] - old[2]);
        let length = (dx * dx + dz * dz).sqrt();
        let length = if length > 0. { length } else { 1. };
        self.bodies.get_mut(body)?.wish = [dx / length * pace, dz / length * pace];
        Some([dx, dz])
    }
    /// A zombie's `steer`: the same, but a route it had to search for is kept to for a few steps (see HOLD).
    fn prowl(&mut self, body: u32, target: [f64; 3], pace: f64) -> Option<[f64; 2]> {
        let old = self.bodies.get(body)?.p;
        let goal = self.level.climb_goal(old, target).unwrap_or(target);
        let (from, to) = ([old[0], old[2]], [goal[0], goal[2]]);
        let far = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]) * (p[0] - q[0]) + (p[1] - q[1]) * (p[1] - q[1]);
        let mut next = to;
        if self.level.joined(from, to) && !self.level.line(from, to) {
            let kept = self.routes.get(&body).copied().filter(|(way, goal, left)| {
                *left > 0 && far(*goal, to) < DRIFT * DRIFT && far(*way, from) > NEAR * NEAR && self.level.line(from, *way)
            });
            match kept {
                Some((way, goal, left)) => {
                    self.routes.insert(body, (way, goal, left - 1));
                    next = way;
                }
                None => {
                    next = self.level.path(from, to).first().copied().unwrap_or(to);
                    self.routes.insert(body, (next, to, HOLD));
                }
            }
        } else {
            self.routes.remove(&body);
        }
        let (dx, dz) = (next[0] - old[0], next[1] - old[2]);
        let length = (dx * dx + dz * dz).sqrt();
        let length = if length > 0. { length } else { 1. };
        self.bodies.get_mut(body)?.wish = [dx / length * pace, dz / length * pace];
        Some([dx, dz])
    }
    /// The place a zombie standing at `at` makes for: the nearest free one round the hotdog at `centre` that
    /// the map leaves clear, the inner rank first. `used` holds the places claimed so far this step for that
    /// hotdog; lower ids choose first. None when both ranks are full.
    fn place(&self, at: [f64; 3], centre: [f64; 3], used: &mut [bool; 16]) -> Option<[f64; 2]> {
        for rank in [0, 8] {
            let (mut best, mut least) = (None, f64::INFINITY);
            for (i, [x, z, r]) in PLACES.iter().enumerate().skip(rank).take(8) {
                let (px, pz) = (centre[0] + x * r, centre[2] + z * r);
                let (dx, dz) = (px - at[0], pz - at[2]);
                let d = dx * dx + dz * dz;
                if !used[i] && self.level.clear(px, pz) && d < least {
                    least = d;
                    best = Some(i);
                }
            }
            if let Some(i) = best {
                used[i] = true;
                let [x, z, r] = PLACES[i];
                return Some([centre[0] + x * r, centre[2] + z * r]);
            }
        }
        None
    }
    /// A hit shoves the body it lands on along the shot, in proportion to the damage.
    pub(crate) fn nudge(&mut self, body: u32, dir: Vec3, damage: f32) {
        // (Nothing a hotdog does shoves the titan.)
        if self.is_titan(body) {
            return;
        }
        if let Some(body) = self.bodies.get_mut(body) {
            let push = actor::HIT_IMPULSE * damage as f64;
            body.apply_impulse([dir.x as f64 * push, 0., dir.z as f64 * push]);
        }
    }
    pub fn player(&self, id: u32) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }
    pub fn player_mut(&mut self, id: u32) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.id == id)
    }
    fn shot(&mut self, owner: u32, a: Vec3, b: Vec3, weapon: usize, projectile: u32) {
        self.shots.push(Shot {
            id: self.next_shot,
            owner,
            a: a.to_array(),
            b: b.to_array(),
            time: self.elapsed,
            weapon,
            projectile,
        });
        self.next_shot += 1;
    }
    /// Keeps the simulation's idea of the gun in hand in step with the player's choice. Only a gun it holds can be
    /// chosen: there are no bare hands. Taking another gun in hand cancels a reload (magazine.rs).
    pub fn select(&mut self, id: u32, weapon: usize) {
        // With its hands full a hotdog draws no gun.
        if self.laden(id) {
            return;
        }
        if let Some(p) = self.player_mut(id) {
            if (1..GUNS).contains(&weapon) && p.owned[weapon] {
                if p.weapon != weapon {
                    p.reload = None;
                }
                (p.weapon, p.gun) = (weapon, weapon);
            }
        }
    }
    /// The gun that comes back to hand when the hands are free: the one last in hand, or else any gun it holds.
    pub(crate) fn hand(p: &Player) -> usize {
        if p.owned[p.gun] { p.gun } else { (1..GUNS).find(|w| p.owned[*w]).unwrap_or(0) }
    }
    /// Aims down the sights of the gun in hand, or stops (see `Player::aimed`): refused, and let go, where it may not.
    pub fn set_aimed(&mut self, id: u32, on: bool) {
        let may = self.may_aim(id);
        if let Some(p) = self.player_mut(id) {
            p.aimed = on && may;
        }
    }
    /// Whether a hotdog may aim down its sights: a person's, alive, with the syrup rifle or the ketchup sniper in its
    /// free hands, away from a card table.
    fn may_aim(&self, id: u32) -> bool {
        self.cards.is_none() && !self.laden(id) && self.player(id).is_some_and(|p| p.alive && !p.bot && matches!(p.weapon, 2 | RIFLE))
    }
    pub fn set_prone(&mut self, id: u32, prone: bool) {
        if let Some(p) = self.player_mut(id) {
            p.prone = prone;
        }
    }
    /// Crouches a hotdog or stands it back up. A prone hotdog stays prone.
    pub fn set_crouch(&mut self, id: u32, crouch: bool) {
        if let Some(p) = self.player_mut(id) {
            p.crouch = crouch;
        }
    }
    // ----- Carrying (see GRASP).

    /// Whether a hotdog's hands are full: a loose object, or (Forts) the bacon. Its guns are away then and it
    /// cannot strike; with the bacon its trigger does nothing at all.
    pub fn laden(&self, id: u32) -> bool {
        self.player(id).is_some_and(|p| p.carry >= 0) || self.fort.as_ref().is_some_and(|fort| id != 0 && fort.bacon.holder == id)
    }
    /// The share of its pace a hotdog keeps under what it carries (`burden_of`; the bacon, forts.rs BACON_PACE).
    pub fn burden(&self, id: u32) -> f32 {
        match self.player(id) {
            Some(p) if p.carry >= 0 && (p.carry as usize) < self.props.len() => burden_of(self.props.shape(self.props.body(p.carry as usize).shape).mass),
            _ if id != 0 && self.fort.as_ref().is_some_and(|fort| fort.bacon.holder == id) => forts::BACON_PACE,
            _ => 1.,
        }
    }
    /// How a hotdog holds what it carries (build.rs `Pose`): its player says, every command. Nothing else changes until
    /// the hotdog next moves (`haul`) or puts it down.
    pub fn set_held_pose(&mut self, id: u32, pose: build::Pose) {
        if let Some(p) = self.player_mut(id) {
            p.pose = pose;
        }
    }
    /// Picks up the loose object the hotdog looks at along `direction`, or failing that the nearest beside it.
    pub fn grab(&mut self, id: u32, direction: Vec3) -> bool {
        let Some(p) = self.player(id).filter(|_| self.cards.is_none() && self.kungfu.is_none()) else {
            return false;
        };
        if !p.alive || p.carry >= 0 || self.stage == Stage::RoundOver || self.fort.as_ref().is_some_and(|fort| fort.bacon.holder == id) {
            return false;
        }
        let bot = p.bot;
        let (mut found, tent) = self.grasp(id, direction);
        if let Some(t) = tent {
            if bot || !self.may_undrape(id) {
                return false;
            }
            found = self.drapes.iter().find(|d| d.id == t).map(|d| d.napkin);
            self.undrape(t);
        }
        let Some(prop) = found else {
            return false;
        };
        if !self.props.pick_up(prop, id as i32) {
            return false;
        }
        if let Some(p) = self.player_mut(id) {
            (p.carry, p.weapon, p.pose) = (prop as i32, 0, build::Pose::default());
        }
        self.haul();
        true
    }
    /// What `grab` along `direction` would take hold of (nothing is changed): the loose object, or else the drape (its
    /// id) whose napkin would be pulled off.
    pub(crate) fn grasp(&self, id: u32, direction: Vec3) -> (Option<usize>, Option<u32>) {
        let Some(p) = self.player(id) else {
            return (None, None);
        };
        let (o, feet, d) = (muzzle(p), p.p.map(|v| v as f64), direction.normalize_or_zero());
        let mut found = None;
        // A draped wall in reach is a napkin to pull off (drape.rs): the one looked at, or failing a loose object,
        // the one the hotdog stands against.
        let mut tent = None;
        let aimed = d.length_squared() > 0.81;
        if aimed {
            let wall = self.level.cover_from(o, d);
            found = self.props.raycast(o.as_dvec3().to_array(), d.as_dvec3().to_array(), wall.min(GRASP) as f64, None).map(|hit| hit.id);
            if found.is_none() {
                // Its wall, or the napkin itself (over a block's edge, the wall is only the strip that hangs).
                let pad = |t: &drape::Drape| {
                    let e = self.props.extent(t.napkin);
                    crate::ray_box(o, d, Vec3::new(e[0] as f32, e[4] as f32, e[1] as f32), Vec3::new(e[2] as f32, e[5] as f32, e[3] as f32))
                };
                tent = self
                    .drapes
                    .iter()
                    .find(|t| crate::ray_box(o, d, Vec3::new(t.rect[0] as f32, FLOOR - 1., t.rect[1] as f32), Vec3::new(t.rect[2] as f32, t.top as f32, t.rect[3] as f32)).or_else(|| pad(t)).is_some_and(|near| near <= wall.min(GRASP) + 1e-3))
                    .map(|t| t.id);
            }
        }
        if found.is_none() && tent.is_none() {
            let mut least = NEXT;
            for prop in 0..self.props.len() {
                if !matches!(self.props.state(prop), props::State::Awake | props::State::Asleep | props::State::Locked) {
                    continue;
                }
                let at = self.props.position(prop);
                let far = (at[0] - feet[0]).hypot(at[2] - feet[2]);
                if far < least && (at[1] - feet[1]).abs() < 0.2 {
                    (least, found) = (far, Some(prop));
                }
            }
        }
        if found.is_none() && tent.is_none() {
            let mut least = crate::map::MARGIN + 0.06;
            for t in &self.drapes {
                let (x, z) = ((t.rect[0] - feet[0]).max(0.).max(feet[0] - t.rect[2]), (t.rect[1] - feet[2]).max(0.).max(feet[2] - t.rect[3]));
                let far = x.hypot(z);
                if far < least && feet[1] < t.top {
                    (least, tent) = (far, Some(t.id));
                }
            }
        }
        (found, tent)
    }
    /// Where the carried objects are: in front of whoever holds each. Called when one is picked up and after
    /// every step's walking (and by the network client, for its copy of the props).
    pub fn haul(&mut self) {
        for i in 0..self.players.len() {
            let p = &self.players[i];
            if p.carry < 0 {
                continue;
            }
            let prop = p.carry as usize;
            if !p.alive || prop >= self.props.len() || self.props.state(prop) != props::State::Carried {
                self.players[i].carry = -1;
                continue;
            }
            let (at, yaw, pose) = (p.p.map(|v| v as f64), p.yaw as f64, p.pose);
            let (place, turn) = self.holding(prop, pose, at, yaw);
            self.props.carry(prop, place, turn);
        }
    }
    /// The hotdog as a carrier: the prop in its hands, its feet, its muzzle's height and its yaw.
    fn carrier(&self, id: u32) -> Option<(usize, [f64; 3], f64, f64)> {
        let p = self.player(id).filter(|p| p.alive && p.carry >= 0)?;
        let prop = p.carry as usize;
        (prop < self.props.len() && self.props.state(prop) == props::State::Carried).then(|| (prop, p.p.map(|v| v as f64), muzzle(p).y as f64, p.yaw as f64))
    }
    // (Putting down, `put`, and where it lands, `placement`: build.rs.)
    /// Throws the carried object along `direction`.
    pub fn toss(&mut self, id: u32, direction: Vec3) -> bool {
        if self.cards.is_some() || self.kungfu.is_some() {
            return false;
        }
        let Some((prop, _, _, yaw)) = self.carrier(id) else {
            return false;
        };
        let d = direction.normalize_or_zero().as_dvec3();
        let d = if d.length_squared() > 0.81 { [d.x, d.y, d.z] } else { [-yaw.sin(), 0., -yaw.cos()] };
        let speed = throw_speed(self.props.shape(self.props.body(prop).shape).mass);
        self.props.drop_prop(prop, [d[0] * speed, d[1] * speed + LOB, d[2] * speed], [d[2] * 2., 0., -d[0] * 2.]);
        if let Some(p) = self.players.iter_mut().find(|p| p.id == id) {
            (p.carry, p.weapon) = (-1, Self::hand(p));
            p.cooldown = self.tune.delay[0];
            p.swings += 1;
        }
        true
    }
    /// A hotdog has died: what it carried falls, and (Forts) the round takes note.
    fn fell(&mut self, id: u32) {
        if let Some(p) = self.player_mut(id).filter(|p| p.carry >= 0) {
            let prop = p.carry as usize;
            p.carry = -1;
            self.props.drop_prop(prop, [0.; 3], [0.; 3]);
        }
        if let Some(fort) = &mut self.fort {
            fort.quiet = self.elapsed;
        }
        // Free For All and the sandbox: the kill is counted and the hotdog will be back (rules.rs).
        self.mourn(id);
    }
    /// Stim: faster and springier for STIM seconds, at the price of double damage from rivals.
    pub fn stim(&mut self, id: u32) -> bool {
        if self.is_titan(id) {
            return false;
        }
        if self.stage == Stage::RoundOver || self.cards.is_some() || self.kungfu.is_some() {
            return false;
        }
        let Some(p) = self.players.iter_mut().find(|p| p.id == id) else {
            return false;
        };
        if !p.alive || p.stim_wait > 0. {
            return false;
        }
        p.stim = self.tune.stim_time;
        p.stim_wait = self.tune.stim_wait;
        true
    }
    /// Follows where a player is looking up or down; `walk` carries the yaw.
    pub fn look(&mut self, id: u32, pitch: f32) {
        if let Some(p) = self.player_mut(id).filter(|p| p.alive) {
            p.pitch = if pitch.is_finite() { pitch.clamp(-1.5, 1.5) } else { 0. };
            p.aim = sightline(p.yaw, p.pitch).to_array();
        }
    }
    /// A hotdog as a round begins, on the round under way: at its start (in Forts, its team's, by its place among
    /// its team mates), whole, with the loadout of every gun mode (magazine.rs `loadout`: the syrup rifle in hand and
    /// the mustard pistol beside it, full magazines and their spare rounds), and nothing else of the round kept. Its id,
    /// `bot` flag, `team` and `ready` mark stay. The server calls this for a hotdog taken over or handed back, and after
    /// a new round has been set out.
    pub fn fit(&mut self, id: u32) {
        let Some(i) = self.players.iter().position(|p| p.id == id) else {
            return;
        };
        let (bot, team, ready) = (self.players[i].bot, self.players[i].team, self.players[i].ready);
        let mut p = if self.fort.is_some() {
            let rank = self.players.iter().filter(|q| q.team == team && q.id < id).count();
            let (at, yaw) = self.level.team_start(team as usize, rank);
            let at = at.map(|v| v as f32);
            Player {
                name: forts::NAMES[team.min(1) as usize][rank % 3].into(),
                p: at,
                yaw: yaw as f32,
                camp: [at[0], at[2]],
                job: forts::Job { at: [at[0], at[2]], ..Default::default() },
                ..Default::default()
            }
        } else {
            // (Free For All: at the spot the round's opening gave it, spawns.rs.)
            let slot = (id as usize).saturating_sub(1) % NAMES.len();
            let (at, yaw) = match self.openings.get(slot).filter(|_| self.rules.ffa) {
                Some(spot) => (spot.p, spot.yaw),
                None => self.level.start(slot),
            };
            Player { name: NAMES[slot].into(), p: at.to_array(), yaw, camp: [at.x, at.z], ..Default::default() }
        };
        crate::magazine::loadout(&mut p);
        (p.id, p.bot, p.team, p.ready, p.hp, p.alive, p.grounded, p.carry) = (id, bot, team, ready, 100., true, true, -1);
        // What it carried is let go.
        if let Ok(prop) = usize::try_from(self.players[i].carry) {
            if prop < self.props.len() && self.props.state(prop) == props::State::Carried {
                self.props.drop_prop(prop, [0.; 3], [0.; 3]);
            }
        }
        self.players[i] = p;
        // The sandbox's loadout, and unlimited rounds (rules.rs).
        self.equip(id);
        self.embody();
        // At a card table it is back in its seat, not at its start.
        if self.cards.is_some() {
            self.sit();
        }
        // Titan: the titan as it starts, and a small hotdog at a small hotdog's start (titan.rs).
        if self.titan.is_some() {
            self.titan_fit(id);
        }
    }
    /// Walks the hotdog a person is steering by `step` ([x, z] in metres) at once, against the level and the
    /// other bodies, and returns where it now stands. Its jump, its falls and any throw happen in `tick`.
    pub fn walk(&mut self, id: u32, step: Vec2, yaw: f32) -> Option<Vec3> {
        // (At a card table the hotdogs stay in their seats.)
        if self.cards.is_some() {
            return None;
        }
        self.player(id).filter(|p| p.alive)?;
        self.embody();
        self.bodies.walk(&self.level, id, step.x as f64, step.y as f64);
        self.placed(id, yaw)
    }
    /// Holds a hotdog's body still in the steps that follow (see `actor::Actor::idle`), or lets it go again.
    pub fn hold(&mut self, id: u32, idle: bool) {
        if let Some(body) = self.bodies.get_mut(id) {
            body.idle = idle;
        }
    }
    /// Takes a step of one hotdog's body alone (`actor::World::step_only`): what the step after a walk does for it
    /// (its fall, its jump, a throw, the props), now, without the rest of the round moving.
    pub fn step_body(&mut self, id: u32, dt: f32) {
        if self.cards.is_some() {
            return;
        }
        self.embody();
        self.bodies.step_only(&self.level, whole(dt as f64), &mut Lean { props: &mut self.props, leans: &mut self.leans }, id);
        self.report();
        self.haul();
    }
    /// Asks for a jump at the next step: `speed` is the take-off speed in m/s. Only a hotdog standing on
    /// something, upright and not thrown, takes off.
    pub fn jump(&mut self, id: u32, speed: f32) {
        if self.cards.is_some() {
            return;
        }
        self.embody();
        if let Some(body) = self.bodies.get_mut(id) {
            body.jump = speed as f64;
        }
    }
    /// After a body has been moved outside a step: the roster learns where it and whoever it pushed now are.
    fn placed(&mut self, id: u32, yaw: f32) -> Option<Vec3> {
        self.report();
        let p = self.player_mut(id)?;
        p.yaw = yaw;
        Some(Vec3::from_array(p.p))
    }
    /// A hotdog takes the pickup `item` (E on its pad, or walked over: magazine.rs `scavenge`). Whoever takes it first
    /// has it: once taken it is gone from its pad until it comes back (at least a second: map.rs), so anybody else who
    /// reaches for it in the same step, later in the step's order (commands as the server takes them, then the hotdogs
    /// walking over pads by id, then the bots by id), gets nothing. False when it is not there, out of reach, or (a box
    /// of ammunition, or a gun the hotdog holds) of no use to the hotdog: its reserves are full.
    pub fn pickup(&mut self, id: u32, item: usize) -> bool {
        let Some(p) = self.player(id).filter(|_| self.cards.is_none() && self.kungfu.is_none() && !self.is_titan(id)) else {
            return false;
        };
        let Some(item) = self.pickups.get(item) else {
            return false;
        };
        if !p.alive
            || item.ready > self.elapsed
            || Vec3::from_array(item.p).distance(Vec3::from_array(p.p) + Vec3::Y * 0.04) > 0.17
            || !self.within_reach(p, item)
        {
            return false;
        }
        let (index, w, ammo) = (item.id, item.weapon, item.ammo);
        let (time, laden) = (self.elapsed, self.laden(id));
        // A box of ammunition tops up the reserves of the guns the hotdog holds (magazine.rs CARRY; not the power
        // weapons), and is left where it is when they are all full.
        if w == crate::map::AMMO_BOX {
            let Some(p) = self.player_mut(id) else {
                return false;
            };
            let short = Self::short(p);
            if short.is_empty() {
                return false;
            }
            for g in short {
                p.reserve[g] = crate::magazine::CARRY[g];
            }
            self.taken(index, time);
            return true;
        }
        if w >= GUNS {
            return false;
        }
        // The pickup of a gun it holds: its rounds go into that gun's reserve, as many as it has room for; with no room
        // it is left where it is.
        if self.player(id).is_some_and(|p| p.owned[w]) {
            let Some(p) = self.player_mut(id) else {
                return false;
            };
            let room = crate::magazine::CARRY[w] - p.reserve[w];
            if room <= 0 || ammo <= 0 {
                return false;
            }
            p.reserve[w] += ammo.min(room);
            self.taken(index, time);
            return true;
        }
        // A hotdog carries two guns; a third gun replaces the one in hand and that gun's ammunition
        // is lost.
        if let Some(p) = self.player_mut(id) {
            if !p.owned[w] && p.owned[1..].iter().filter(|held| **held).count() >= 2 {
                let drop = if p.owned[p.gun] && p.gun > 0 {
                    p.gun
                } else {
                    (1..GUNS).find(|i| p.owned[*i]).unwrap_or(1)
                };
                p.owned[drop] = false;
                (p.ammo[drop], p.reserve[drop]) = (0, 0);
            }
            // A new gun: its rounds fill its magazine, and the rest go to its reserve.
            let loaded = ammo.clamp(0, crate::magazine::MAGAZINE[w]);
            p.owned[w] = true;
            (p.ammo[w], p.reserve[w]) = (loaded, (ammo - loaded).clamp(0, crate::magazine::CARRY[w]));
            // (With its hands full the new gun waits to come to hand until they are free.)
            if !laden {
                p.weapon = w;
            }
            p.gun = w;
        }
        self.taken(index, time);
        true
    }
    /// The pickup `index` has been taken at `time`: it is back after its `respawn` seconds, except that a power
    /// weapon's time is drawn afresh (`comeback`) and kept in its `respawn`, which is what its pad's ring and the
    /// head-up display count back from.
    fn taken(&mut self, index: usize, time: f32) {
        let back = self.comeback(index, time);
        let item = &mut self.pickups[index];
        (item.respawn, item.ready) = (back, time + back);
    }
    /// How long the pickup `index`, taken at `time`, takes to come back: its `respawn`, or for a power weapon
    /// (magazine.rs POWER_BACK), in every gun mode, a time between 30 and 60 seconds drawn from the game's seed, the
    /// round, the pickup and the moment it was taken (spawns.rs `Dice`), so that the same game draws the same times and
    /// the server, which runs the game, is the one that draws them.
    pub(crate) fn comeback(&self, index: usize, time: f32) -> f32 {
        let item = &self.pickups[index];
        if !crate::magazine::power(item.weapon) {
            return item.respawn;
        }
        let mut dice = crate::spawns::Dice::new(&[self.card_seed, self.round as u64, crate::spawns::PICKUP, index as u64, (time as f64 * 1000.).round() as u64]);
        let share = (dice.roll() >> 11) as f64 / (1u64 << 53) as f64;
        let [least, most] = crate::magazine::POWER_BACK;
        least + (most - least) * share as f32
    }
    /// Whether a hotdog may take a pickup, as far as the forcefield goes: in Forts while the field stands a team
    /// takes only those on its own side of it (they are on their pads from the start of the build phase), wherever
    /// the hotdog has been put.
    pub(crate) fn within_reach(&self, p: &Player, item: &Pickup) -> bool {
        let up = self.fort.as_ref().is_some_and(|fort| fort.up);
        let side = self.level.side(item.p[0] as f64, item.p[2] as f64);
        !up || self.level.fort.as_ref().is_none_or(|fort| fort.sides[p.team as usize % 2] == side)
    }
    /// The guns a hotdog holds whose reserve has room: those a box of ammunition tops up (not the power weapons:
    /// magazine.rs POWER).
    pub(crate) fn short(p: &Player) -> Vec<usize> {
        refills(&p.owned, &p.reserve)
    }
    /// Eating consumes a hotdog that is already dead, once. The living cannot be eaten. Only in the siege's famine
    /// (`famine`), where hunger makes it the way to live: no other mode eats.
    pub fn eat(&mut self, id: u32) -> bool {
        self.chew(id, true)
    }
    /// The famine's healing bite: it wounds a living rival, as a melee blow does (`blow_on`), and heals a little. Only in
    /// the siege's famine; in every other gun mode F is the strike alone (`melee`).
    pub fn bite(&mut self, id: u32) -> bool {
        self.chew(id, false)
    }
    /// Sinks teeth into the weakest hotdog in reach that is `dead` or not, as asked.
    fn chew(&mut self, id: u32, dead: bool) -> bool {
        if !self.famine() {
            return false;
        }
        let Some(p) = self.player(id) else {
            return false;
        };
        if !p.alive || p.bite_cooldown > 0. {
            return false;
        }
        let pos = Vec3::from_array(p.p);
        let target = self
            .players
            .iter()
            .filter(|t| t.id != id && !t.consumed && Vec3::from_array(t.p).distance(pos) < 0.17)
            .filter(|t| t.alive != dead)
            .min_by(|a, b| a.hp.total_cmp(&b.hp))
            .map(|p| p.id);
        let Some(target) = target else {
            return false;
        };
        let alive = self.player(target).unwrap().alive;
        let meal = if alive {
            let old = self.player(target).unwrap().hp;
            let to = Vec3::from_array(self.player(target).unwrap().p) - pos;
            let to = Vec3::new(to.x, 0., to.z);
            // (The bite is the free for all's melee blow, F with the gun in hand, so it is a backstab from behind and
            // half of MAX_HP from in front; what it feeds the biter, as ever, is on the first 24 HP.)
            let (amount, kind) = self.blow_on(target, pos, to);
            self.harm(target, amount, true, Damage::by(kind, id, pos, to));
            old.min(BITE_HP) * 0.8
        } else {
            self.player_mut(target).unwrap().consumed = true;
            38.
        };
        if let Some(p) = self.player_mut(id) {
            p.hp = (p.hp + meal).min(100.);
            p.bite_cooldown = 0.8;
            p.meals += 1;
            p.swings += 1;
            p.score += 25;
        }
        self.friendly_fire_events += 1;
        true
    }
    /// Takes HP from a hotdog without saying what did it (tests and tools): its record of what last hurt it
    /// is left blank.
    pub fn hurt(&mut self, id: u32, amount: f32) {
        self.harm(id, amount, false, Damage::default());
    }
    /// Damage dealt by another hotdog: doubled while the victim is stimmed. Zombies, pests and hunger are
    /// not. As `hurt`, it leaves no record of the cause.
    pub fn wound(&mut self, id: u32, amount: f32) {
        self.harm(id, amount, true, Damage::default());
    }
    /// All damage to hotdogs goes through here: `rival` damage is doubled by the victim's stim, and `cause`
    /// becomes the hotdog's record of what last hurt it, with the time and the HP it took. The dead are
    /// past hurting, so a dead hotdog's record is what killed it.
    pub(crate) fn harm(&mut self, id: u32, amount: f32, rival: bool, cause: Damage) {
        let time = self.elapsed;
        // Forts: nobody can be hurt while the forcefield stands.
        if self.stage == Stage::Build {
            return;
        }
        // The sandbox's god mode and Free For All's spawn protection: the hit is on the record, the HP stay.
        if self.spared(id, amount, rival, &cause) {
            return;
        }
        let mut died = false;
        let stimmed = self.tune.stim_damage;
        if let Some(p) = self.player_mut(id) {
            if p.alive {
                let scale = if rival && p.stim > 0. { stimmed } else { 1. };
                let before = p.hp;
                p.hp = (p.hp - amount * scale).max(0.);
                p.alive = p.hp > 0.;
                p.last_damage = Damage { time, amount: before - p.hp, ..cause };
                died = !p.alive;
            }
        }
        if died {
            self.fell(id);
        }
    }
    fn maul(&mut self, owner: u32, zombie: u32, amount: f32) {
        let Some(e) = self.enemies.iter_mut().find(|e| e.id == zombie && e.forming <= 0.) else {
            return;
        };
        e.hp -= amount;
        if e.hp <= 0. {
            self.enemies.retain(|e| e.id != zombie);
            self.wave_kills += 1;
            if let Some(p) = self.player_mut(owner) {
                p.kills += 1;
                p.score += 100;
            }
        }
    }
    /// Guns launch condiment rounds that fly until they meet cover, the counter, a body or the end of their
    /// range. The trigger with a loose object in the hands throws it, and with the bacon does nothing; behind the
    /// forcefield (Forts) no gun fires. The syrup rifle fires the first round of its burst here; `volleys` fires
    /// the rest, each where the gun points as it leaves, climbing a little more. A gun being reloaded does not fire; one whose
    /// magazine is empty starts reloading instead, if it has rounds in reserve (magazine.rs). A round leaves deflected
    /// by the hotdog's bloom, and adds its gun's kick to it (recoil.rs).
    pub fn fire(&mut self, id: u32, direction: Vec3, weapon: usize) -> bool {
        if self.stage == Stage::RoundOver || weapon >= GUNS || self.cards.is_some() || self.kungfu.is_some() {
            return false;
        }
        // With a loose object in its hands the trigger throws it; with the bacon (Forts) it does nothing.
        if self.player(id).is_some_and(|p| p.carry >= 0) {
            return self.player(id).is_some_and(|p| p.alive && p.cooldown <= 0.) && self.toss(id, direction);
        }
        if self.laden(id) || self.stage == Stage::Build || weapon == 0 {
            return false;
        }
        let Some(p) = self.player(id) else {
            return false;
        };
        if !p.alive || p.cooldown > 0. || !p.owned[weapon] || p.reload.is_some() {
            return false;
        }
        if p.ammo[weapon] <= 0 {
            // An empty magazine: the trigger takes the gun in hand and reloads it, if there is anything to reload.
            let take = p.weapon != weapon && p.reserve[weapon] > 0;
            if take {
                self.select(id, weapon);
            }
            self.reload(id);
            return false;
        }
        let origin = muzzle(p);
        let dir = direction.normalize_or_zero();
        if dir.length_squared() < 0.9 {
            return false;
        }
        let aim = dir;
        let dir = crate::recoil::deflect(dir, p.bloom, id, self.next_round, weapon);
        let kicked = crate::recoil::after_shot(p, weapon, self.tune.recoil[weapon]);
        let p = self.players.iter_mut().find(|p| p.id == id).unwrap();
        p.bloom = kicked;
        // The mayonnaise keeps what it is owed from the last step (cooldown is never below a step's length), so
        // its stream flows at the same rate whatever the steps; stimmed, it flows STIM_FLOW times as fast.
        p.cooldown = if weapon == MAYO { p.cooldown.min(0.) + self.tune.delay[MAYO] / if p.stim > 0. { self.tune.stim_flow } else { 1. } } else { self.tune.delay[weapon] };
        p.weapon = weapon;
        // Whoever pulls the trigger is aiming along the shot: bots aim up and down this way.
        p.pitch = dir.y.clamp(-1., 1.).asin();
        p.aim = aim.to_array();
        p.ammo[weapon] -= 1;
        p.gun = weapon;
        if weapon == RIFLE {
            (p.volley, p.volley_wait, p.volley_aim) = (self.tune.burst - 1, self.tune.gap, dir.to_array());
        }
        let round = self.launch(id, origin, dir, weapon);
        self.shot(id, origin, origin + dir * 0.03, weapon, round);
        true
    }
    /// The rest of every syrup burst under way: a round each VOLLEY_GAP seconds while the rifle stays in hand and
    /// loaded and the hotdog may fire, each along the hotdog's aim as it is when the round leaves (`Player::aim`: a
    /// person's look that step, a bot's way it faces), climbing CLIMB more than the last (AIMED of that while it aims).
    /// So a burst at a hotdog walking across is swept with it, or it walks off the body.
    fn volleys(&mut self, dt: f32) {
        for i in 0..self.players.len() {
            if self.players[i].volley == 0 {
                continue;
            }
            self.players[i].volley_wait -= dt;
            // (Within a hundredth of a millisecond is due: the steps' sum rounds.)
            if self.players[i].volley_wait > 1e-5 {
                continue;
            }
            let id = self.players[i].id;
            let p = &self.players[i];
            if !p.alive || p.weapon != RIFLE || !p.owned[RIFLE] || p.ammo[RIFLE] <= 0 || p.reload.is_some() || self.laden(id) || matches!(self.stage, Stage::Build | Stage::RoundOver) {
                self.players[i].volley = 0;
                continue;
            }
            // Where the gun points as this round leaves (the first's aim, for one that has never aimed).
            let a = if Vec3::from_array(p.aim).length_squared() > 0.5 { p.aim } else { p.volley_aim };
            let (k, origin) = ((self.tune.burst - p.volley) as f32, muzzle(p));
            let rise = a[1] + self.tune.climb * if p.aimed { AIMED } else { 1. } * k;
            let l = (a[0] * a[0] + rise * rise + a[2] * a[2]).sqrt();
            let dir = Vec3::new(a[0] / l, rise / l, a[2] / l);
            // (The burst's rounds keep its own climb; their kicks go into the bloom, settled by the next burst.)
            let kicked = crate::recoil::after_shot(p, RIFLE, self.tune.recoil[RIFLE]);
            let p = &mut self.players[i];
            p.bloom = kicked;
            p.volley -= 1;
            p.volley_wait += self.tune.gap;
            p.ammo[RIFLE] -= 1;
            let round = self.launch(id, origin, dir, RIFLE);
            self.shot(id, origin, origin + dir * 0.03, RIFLE, round);
            // (A bot's hand wanders a little before the next: aim.rs.)
            if self.players[i].volley > 0 {
                self.wander(id);
            }
        }
    }
    /// What a melee blow struck from `from` along `dir` does to the living hotdog `target`, and the kind it goes on
    /// its record as: from inside its rear arc (`backstab`) it takes all its HP, BACKSTAB, otherwise half of MAX_HP
    /// (`tune.hotdog[0]`, MELEE). The kill is reckoned against the victim's stim, which doubles what rivals do to
    /// it, so that a backstab is a kill however the stim is tuned (short of making it immune).
    fn blow_on(&self, target: u32, from: Vec3, dir: Vec3) -> (f32, u8) {
        let Some(t) = self.player(target) else {
            return (0., MELEE);
        };
        if !backstab(t.p, t.yaw, from, dir) {
            return (self.tune.hotdog[0], MELEE);
        }
        let scale = if t.stim > 0. { self.tune.stim_damage } else { 1. };
        (if scale > 0. { t.hp / scale * 1.001 + 0.001 } else { t.hp }, BACKSTAB)
    }
    /// The same for a zombie: half of ZOMBIE_HP (`tune.zombie[0]`) from the front, and from behind ZOMBIE_HP, which
    /// is all a zombie has (a test's zombie of a million HP, to be hit and not die, is not killed by it).
    fn blow_on_zombie(&self, target: u32, from: Vec3, dir: Vec3) -> f32 {
        match self.enemies.iter().find(|e| e.id == target) {
            Some(e) if backstab(e.p, e.yaw, from, dir) => ZOMBIE_HP,
            _ => self.tune.zombie[0],
        }
    }
    /// The melee strike, F, the same in every gun mode: a blow with the gun in hand (a pistol whip, a rifle butt, a
    /// swing of the bazooka's tube, a jab of the spray can, a shove with the hot sauce thrower; there are no bare
    /// hands). It needs no ammunition and is the same blow whatever delivers it: it takes DELAY[0] seconds and reaches
    /// RANGE[0] metres along the aim. The nearest formed zombie there, or, where rounds hurt hotdogs (the siege's
    /// famine, Free For All, a Forts fight), the nearest hotdog that is no team mate, is struck: from in front of it for
    /// half its most HP (PLAYER_DAMAGE[0], ZOMBIE_DAMAGE[0]), from behind (inside the 120 degrees of its rear arc,
    /// `backstab`) for all of it, a kill, which goes on a hotdog's record as BACKSTAB. In the siege's famine alone a
    /// living hotdog in reach is bitten instead (`bite`, with its own cooldown, during which there is no striking either),
    /// by the same rule, and the bite heals.
    pub fn melee(&mut self, id: u32, direction: Vec3) -> bool {
        // With the hands full, behind the forcefield (Forts) or at a card table, there is no strike.
        if self.stage == Stage::RoundOver || self.stage == Stage::Build || self.laden(id) || self.cards.is_some() || self.kungfu.is_some() || self.is_titan(id) {
            return false;
        }
        let famine = self.famine();
        if famine && self.bite(id) {
            return true;
        }
        let Some(p) = self.player(id) else {
            return false;
        };
        if !p.alive || p.cooldown > 0. || (famine && p.bite_cooldown > 0.) {
            return false;
        }
        let origin = muzzle(p);
        let dir = direction.normalize_or_zero();
        if dir.length_squared() < 0.9 {
            return false;
        }
        let p = self.players.iter_mut().find(|p| p.id == id).unwrap();
        p.cooldown = self.tune.delay[0];
        p.pitch = dir.y.clamp(-1., 1.).asin();
        p.swings += 1;
        // (The hands leave the magazine to strike: a reload under way is cancelled.)
        p.reload = None;
        let mut nearest = self.screen(origin, dir).min(self.tune.range[0]);
        let mut zombie = None;
        for e in self.enemies.iter().filter(|e| e.forming <= 0.) {
            if let Some(t) = ray_sphere(origin, dir, Vec3::from_array(e.p) + Vec3::Y * 0.075, 0.065)
            {
                if t < nearest {
                    nearest = t;
                    zombie = Some(e.id);
                }
            }
        }
        // Where rounds hurt hotdogs, a rival in reach is struck as a zombie is, for PLAYER_DAMAGE[0]: in a free for all
        // anybody, in a Forts fight anybody of the other team.
        let mut rival = None;
        if matches!(self.stage, Stage::FreeForAll | Stage::Fight) {
            let team = self.player(id).map_or(0, |p| p.team);
            let mate = |t: &Player| (self.stage == Stage::Fight || self.titan.is_some()) && t.team == team;
            // (The titan's legs stand inside its footprint: the strike reaches a little further on it. Titan has no cats.)
            let wall = self.screen(origin, dir);
            for t in self.players.iter().filter(|t| t.alive && t.id != id && !mate(t)) {
                let reach = if self.is_titan(t.id) { nearest.max(wall.min(self.tune.range[0] + crate::titan::STRIKE_REACH)) } else { nearest };
                // (Its body or its head, where the nod has it: head.rs; the titan's, its own shape: titan.rs.)
                if let Some(far) = self.hit_on(t, origin, dir, 0.).map(|h| h.0) {
                    if far < reach {
                        nearest = far;
                        rival = Some(t.id);
                        zombie = None;
                    }
                }
            }
        }
        let from = self.player(id).map_or(origin, |p| Vec3::from_array(p.p));
        if let Some(target) = rival.filter(|t| self.is_titan(*t)) {
            // (The titan: the strike's damage times where it struck, never a kill from behind: titan.rs.)
            if let Some((_, zone, point)) = self.player(target).and_then(|t| self.hit_on(t, origin, dir, 0.)) {
                self.strike_titan(id, self.tune.hotdog[0], zone, point, dir, MELEE, from);
            }
            return true;
        }
        if let Some(target) = rival {
            let (amount, kind) = self.blow_on(target, from, dir);
            self.harm(target, amount, true, Damage::by(kind, id, from, dir));
            self.embody();
            self.nudge(target, dir, self.tune.hotdog[0]);
        }
        if let Some(target) = zombie {
            let amount = self.blow_on_zombie(target, from, dir);
            self.maul(id, target, amount);
            self.embody();
            self.nudge(CAT + target, dir, self.tune.zombie[0]);
        }
        true
    }
    /// Rounds launched during the wave never hurt hotdogs; only rounds launched in the free for
    /// all are hostile. (Forts: those launched in the fight, to the other team.)
    pub(crate) fn launch(&mut self, owner: u32, origin: Vec3, dir: Vec3, weapon: usize) -> u32 {
        let id = self.next_round;
        self.next_round += 1;
        // A stimmed hotdog squeezes the mayonnaise harder.
        let squeeze = if weapon == MAYO && self.player(owner).is_some_and(|p| p.stim > 0.) { self.tune.stim_squirt } else { 1. };
        let v = dir * self.tune.speed[weapon] * squeeze;
        self.projectiles.push(Projectile {
            id,
            owner,
            weapon,
            p: origin.to_array(),
            v: v.to_array(),
            travelled: 0.,
            hostile: matches!(self.stage, Stage::FreeForAll | Stage::Fight),
            born: self.elapsed,
            from: origin.to_array(),
            launch: v.to_array(),
            ..Default::default()
        });
        id
    }
    /// Advances every round by one step. Public so tests and tools can fly rounds without the rest
    /// of the simulation. A round flies until something solid stops it (a block, the counter, a solid loose object),
    /// its range runs out, or it meets a body or a swarm. On its way it passes through light loose objects, each
    /// slowing and weakening it (LIGHT_SPEED, LIGHT_DAMAGE; below STALL it splats there), and the sniper's round
    /// passes through bodies too (SNIPER). What it meets is taken nearest first, and every body it hits is splatted
    /// where it was hit, on its record and in its killer's score as usual.
    ///
    /// Each step a round is tested along the whole of the way it goes (a ray from where it was, as far as it gets), so
    /// a round as fast as the ketchup sniper's (45 m/s: 0.75 m a step at 60 a second) cannot pass through a 2.5 cm
    /// board or a body between one step and the next. A round that falls goes along the chord of its arc, which sags
    /// below the arc by its fall times the step squared over eight: while one is in the air the step is cut into as
    /// many as keep that under ARC_SAG (the mayonnaise's, at 60 steps a second, into seven).
    pub fn flight(&mut self, dt: f32) {
        let fall = self.projectiles.iter().map(|q| self.tune.fall.get(q.weapon).copied().unwrap_or(0.)).fold(0f32, f32::max);
        let n = if fall > 0. && dt > 0. { ((dt * dt * fall / (8. * ARC_SAG)).sqrt().ceil() as usize).clamp(1, 64) } else { 1 };
        for _ in 0..n {
            self.fly(dt / n as f32);
        }
    }
    /// One step (or part of one) of `flight`.
    fn fly(&mut self, dt: f32) {
        let flying = std::mem::take(&mut self.projectiles);
        for mut q in flying {
            let w = q.weapon;
            let origin = Vec3::from_array(q.p);
            let fall = self.tune.fall[w];
            // A round that falls goes from where it was to where its arc has it now; the rest fly straight, as fast
            // as the light objects they have passed have left them.
            let mut pace = self.tune.speed[w] * LIGHT_SPEED.powi(q.soaked.len() as i32);
            let (dir, step, arced) = if fall > 0. {
                q.age += dt;
                let (to, v) = arc(Vec3::from_array(q.from), Vec3::from_array(q.launch), fall, q.age);
                let d = to - origin;
                q.v = v.to_array();
                (d.normalize_or(Vec3::NEG_Y), d.length(), Some(to))
            } else {
                (Vec3::from_array(q.v) / pace, pace * dt, None)
            };
            // How far it gets this step (`stride`, less once a light object has slowed it), and where its range runs
            // out or something solid stops it (`stop`).
            let mut stride = step;
            let mut stop = (self.tune.range[w] - q.travelled).min(self.level.cover_from(origin, dir));
            // It lands on the counter, a deck or a ramp, or, past the counter's edge, the kitchen floor.
            if let Some(to) = arced.filter(|to| to.y < origin.y) {
                let level = &self.level;
                let inside = (to.x as f64) >= level.x0 && (to.x as f64) < level.x1 && (to.z as f64) >= level.z0 && (to.z as f64) < level.z1;
                let ground = if inside { level.ground(to.x as f64, to.z as f64) as f32 } else { self.bodies.config.kitchen_floor as f32 };
                if to.y < ground {
                    stop = stop.min(((origin.y - ground) / (origin.y - to.y)).clamp(0., 1.) * step);
                }
            } else if arced.is_none() && dir.y < 0. {
                let t = ((FLOOR - origin.y) / dir.y).max(0.);
                // Over a sink the counter top is open: the round goes on down, and the sink's sides and floor stop it
                // (`cover_from`).
                let open = self.level.sunken() && {
                    let at = origin + dir * t;
                    self.level.sunk(at.x as f64, at.z as f64)
                };
                if !open {
                    stop = stop.min(t);
                }
            }
            // A solid loose object stops a round as a block does; the light ones before it it passes through.
            let (prop, lights) = self.solid_along(origin, dir, stride.min(stop), &q.soaked);
            if let Some(hit) = &prop {
                stop = stop.min(hit.distance as f32);
            }
            let reach = stride.min(stop);
            let mut met: Vec<(f32, Met)> = lights.into_iter().map(|hit| (hit.distance as f32, Met::Light(hit))).collect();
            if self.tune.zombie[w] > 0. {
                // Rounds fly on through a zombie that is still forming.
                for e in self.enemies.iter().filter(|e| e.forming <= 0. && !q.pierced.contains(&(CAT + e.id))) {
                    let centre = Vec3::from_array(e.p) + Vec3::Y * 0.075;
                    if let Some(t) = ray_sphere(origin, dir, centre, 0.065 + self.tune.size[w]).filter(|t| *t < reach) {
                        met.push((t, Met::Zombie(e.id)));
                    }
                }
            }
            if q.hostile && self.tune.hotdog[w] > 0. {
                for p in &self.players {
                    if p.id != q.owner && p.alive && !self.allied(q.owner, p.id) && !q.pierced.contains(&p.id) {
                        // Its body, or its head where the nod has it (head.rs), grown by the round's size; the titan's
                        // own shape and its zones (titan.rs).
                        if self.is_titan(p.id) {
                            if let Some((t, zone, point)) = self.hit_on(p, origin, dir, self.tune.size[w]).filter(|h| h.0 < reach) {
                                met.push((t, Met::Titan(p.id, zone, point)));
                            }
                        } else if let Some(h) = crate::head::hit(p, origin, dir, self.tune.size[w]).filter(|h| h.t < reach) {
                            met.push((h.t, Met::Hotdog(p.id, (h.part == crate::head::Part::Head).then_some(h.point))));
                        }
                    }
                }
            }
            if self.tune.spray[w] > 0. {
                for s in &self.pests {
                    let centre = Vec3::from_array(s.p);
                    let r = self.tune.size[w];
                    if let Some(t) = ray_box(origin, dir, centre - Vec3::new(SWARM[0] + r, 0.02, SWARM[1] + r), centre + Vec3::new(SWARM[0] + r, 0.3, SWARM[1] + r)).filter(|t| *t < reach) {
                        met.push((t, Met::Swarm(s.id)));
                    }
                }
            }
            // Nearest first (and, at one distance, light objects, then zombies, hotdogs and swarms, each in their
            // order: a stable sort keeps the order they were listed in).
            met.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (splash, pierce) = (self.tune.splash[w] > 0., w == SNIPER);
            // Where it ended short of `stop`, if it did (on a body, a swarm, or stalled in a light object), the body it
            // ended on, and whether a falling round set out on a new arc from a light object it passed.
            let (mut early, mut zombie, mut opponent, mut restart) = (None, None, None, false);
            for (t, thing) in met {
                if t >= stride.min(stop) {
                    break;
                }
                let point = origin + dir * t;
                match thing {
                    Met::Light(hit) => {
                        // It goes through, nudging the object a little (by what it loses of its damage).
                        if !splash && self.tune.zombie[w] > 0. {
                            let push = actor::HIT_IMPULSE * (self.tune.zombie[w] * q.power() * (1. - LIGHT_DAMAGE)) as f64;
                            self.props.apply_impulse(hit.id, [dir.x as f64 * push, dir.y as f64 * push, dir.z as f64 * push], Some(hit.point));
                        }
                        q.soaked.push(hit.id);
                        if arced.is_some() {
                            // A falling round sets out afresh from here, slowed, on a new arc: this step it gets no
                            // further.
                            let f = if step > 1e-9 { t / step } else { 0. };
                            let (at, v) = arc(Vec3::from_array(q.from), Vec3::from_array(q.launch), fall, q.age - dt * (1. - f));
                            let launch = v * LIGHT_SPEED;
                            (q.from, q.launch, q.v, q.born, q.age) = (at.to_array(), launch.to_array(), launch.to_array(), self.elapsed, 0.);
                            (pace, stride, restart) = (launch.length(), t, true);
                        } else {
                            q.v = (Vec3::from_array(q.v) * LIGHT_SPEED).to_array();
                            (pace, stride) = (pace * LIGHT_SPEED, t + (stride - t) * LIGHT_SPEED);
                        }
                        if pace < STALL {
                            early = Some(t);
                            break;
                        }
                    }
                    Met::Zombie(id) => {
                        let damage = self.tune.zombie[w] * q.power();
                        q.pierced.push(CAT + id);
                        self.splat(&q, point);
                        self.maul(q.owner, id, damage);
                        // A direct hit nudges what it lands on; a glob throws with its burst instead.
                        if !splash {
                            self.nudge(CAT + id, dir, damage);
                        }
                        if !pierce {
                            (early, zombie) = (Some(t), Some(id));
                            break;
                        }
                    }
                    Met::Hotdog(id, head) => {
                        // In the face (FACE), on the head, a round may do more: on the record as such (FACE_SHOT).
                        let face = FACE[w] != 1. && head.is_some_and(|at| self.player(id).is_some_and(|p| in_face(p, at, dir)));
                        let damage = self.tune.hotdog[w] * q.power() * if face { FACE[w] } else { 1. };
                        q.pierced.push(id);
                        self.splat(&q, point);
                        // A round is on record as coming from the muzzle it left; a glob, from where it burst.
                        let from = if splash { point } else { origin - dir * q.travelled };
                        let kind = kind_of(w) | if face { FACE_SHOT } else { 0 };
                        self.harm(id, damage, true, Damage::by(kind, q.owner, from, dir));
                        self.friendly_fire_events += 1;
                        if !splash {
                            self.nudge(id, dir, damage);
                        }
                        if !pierce {
                            (early, opponent) = (Some(t), Some(id));
                            break;
                        }
                    }
                    Met::Titan(id, zone, at) => {
                        let damage = self.tune.hotdog[w] * q.power();
                        q.pierced.push(id);
                        self.splat(&q, point);
                        let from = if splash { point } else { origin - dir * q.travelled };
                        self.strike_titan(q.owner, damage, zone, at, dir, kind_of(w), from);
                        self.friendly_fire_events += 1;
                        if !pierce {
                            (early, opponent) = (Some(t), Some(id));
                            break;
                        }
                    }
                    Met::Swarm(id) => {
                        if let Some(s) = self.pests.iter_mut().find(|s| s.id == id) {
                            s.hp -= self.tune.spray[w];
                            if s.hp <= 0. {
                                self.pests.retain(|s| s.id != id);
                                if let Some(p) = self.player_mut(q.owner) {
                                    p.score += 50;
                                }
                            }
                        }
                        early = Some(t);
                        break;
                    }
                }
            }
            let end = early.unwrap_or(stride.min(stop));
            let point = origin + dir * end;
            if early.is_none() && stride < stop {
                q.p = if restart { q.from } else { arced.unwrap_or(point).to_array() };
                q.travelled += end;
                self.projectiles.push(q);
                continue;
            }
            // Where it ended (a body it ended on has its splat already).
            if zombie.is_none() && opponent.is_none() {
                self.splat(&q, point);
            }
            // The solid object that stopped it is nudged where it was hit, as a zombie would be.
            if let Some(hit) = prop.filter(|hit| early.is_none() && hit.distance as f32 <= stop && !splash && self.tune.zombie[w] > 0.) {
                let push = actor::HIT_IMPULSE * (self.tune.zombie[w] * q.power()) as f64;
                self.props.apply_impulse(hit.id, [dir.x as f64 * push, dir.y as f64 * push, dir.z as f64 * push], Some(hit.point));
            }
            if splash {
                self.burst(&q, point - dir * 0.004, zombie, opponent);
            }
        }
    }
    /// Where a round ended, or a body it hit, for the renderers to splatter.
    fn splat(&mut self, q: &Projectile, point: Vec3) {
        self.splats.push(Splat { id: self.next_splat, projectile: q.id, weapon: q.weapon, p: point.to_array(), time: self.elapsed });
        self.next_splat += 1;
    }
    /// A glob bursts where it lands: its damage goes by how far it burst from the nearest point of each body (a hotdog's
    /// as it is now: standing, crouched, prone, or tucked in the air, `figure`, and its head where the nod has it,
    /// `body_gap`), all of it within LETHAL (a hotdog's
    /// MAX_HP: a kill from full health) and falling smoothly to nothing at the splash radius (`blast`). Solid cover
    /// between shields; each light loose object between takes it down to LIGHT_DAMAGE. The shooter is spared (and not
    /// thrown). `direct` and `struck` already took the full hit. A glob that came through light objects bursts as weakly
    /// as it has been left (`power`).
    pub fn burst(&mut self, q: &Projectile, point: Vec3, direct: Option<u32>, struck: Option<u32>) {
        let (w, reach, power) = (q.weapon, self.tune.splash[q.weapon], q.power());
        // A body's share: by how far the burst is from the nearest point of it (a capsule over its feet `at`, from `low`
        // to `high` above them, `radius` round), `blast`; nothing through anything solid between them, and LIGHT_DAMAGE
        // of it through each light object.
        let capsule = |at: Vec3, low: f32, high: f32, radius: f32| {
            let (bottom, top) = (at.y + low + radius, (at.y + high - radius).max(at.y + low + radius));
            let axis = Vec3::new(at.x, point.y.clamp(bottom, top), at.z);
            let far = point.distance(axis);
            ((far - radius).max(0.), if far > radius { axis + (point - axis) / far * radius } else { point })
        };
        // (A hotdog's: the capsule `figure` gives, or its head where the nod has it, whichever is nearer: `body_gap`.)
        let share = |feast: &Feast, (gap, near): (f32, Vec3)| {
            let f = blast(gap, reach);
            let span = point.distance(near);
            if f <= 0. || span <= 1e-4 {
                return f;
            }
            let way = (near - point) / span;
            if feast.cover(point, way) < span {
                return 0.;
            }
            f * LIGHT_DAMAGE.powi(feast.solid_along(point, way, span, &[]).1.len() as i32)
        };
        let zombies: Vec<(u32, f32)> = self
            .enemies
            .iter()
            .filter(|e| Some(e.id) != direct && e.forming <= 0.)
            .map(|e| (e.id, share(self, capsule(Vec3::from_array(e.p), 0., actor::HEIGHT as f32, actor::RADIUS as f32))))
            .filter(|(_, f)| *f > 0.)
            .collect();
        for (id, f) in zombies {
            self.maul(q.owner, id, self.tune.zombie[w] * power * f);
        }
        if q.hostile {
            let titan = self.titan_id().unwrap_or(0);
            let rivals: Vec<(u32, f32, Vec3)> = self
                .players
                .iter()
                .filter(|p| Some(p.id) != struck && p.id != q.owner && p.alive && !self.allied(q.owner, p.id) && p.id != titan)
                .map(|p| (p.id, share(self, body_gap(p, point)), chest(p) - point))
                .filter(|(_, f, _)| *f > 0.)
                .collect();
            for (id, f, out) in rivals {
                self.harm(id, self.tune.hotdog[w] * power * f, true, Damage::by(kind_of(w), q.owner, point, out));
                self.friendly_fire_events += 1;
            }
            // The titan: by the nearest point of its shape, times the zone that point is in (titan.rs).
            if let Some(p) = self.player(titan).filter(|p| Some(p.id) != struck && p.id != q.owner && p.alive && !self.allied(q.owner, p.id)) {
                let (gap, near, zone) = self.titan_shape().unwrap_or_else(|| crate::titan::Shape::of(p, crate::titan::Move::Idle)).gap(point);
                let f = share(self, (gap, near));
                if f > 0. {
                    self.strike_titan(q.owner, self.tune.hotdog[w] * power * f, zone, near, (near - point).normalize_or_zero(), kind_of(w), point);
                    self.friendly_fire_events += 1;
                }
            }
        }
        // The blast throws whoever it could have hurt: zombies always, hotdogs once rounds are hostile, and
        // never the shooter. Cover shields from the throw as it does from the damage.
        self.embody();
        let spared: Vec<Actor> = self.bodies.actors.iter().filter(|body| body.id < CAT && (!q.hostile || body.id == q.owner || self.allied(q.owner, body.id) || self.is_titan(body.id))).cloned().collect();
        for body in &spared {
            self.bodies.remove(body.id);
        }
        self.bodies.blast(&self.level, point.as_dvec3().to_array(), actor::BLAST_RADIUS, actor::BLAST_IMPULSE, actor::BLAST_LIFT);
        for body in spared {
            self.bodies.add(body);
        }
        // Loose objects are thrown too, by the same burst.
        self.props.blast(point.as_dvec3().to_array(), actor::BLAST_RADIUS, actor::BLAST_IMPULSE, actor::BLAST_LIFT);
    }
    /// Flies and parasites gather where a hotdog camps, sting everyone inside the area, and drift
    /// off once it stays empty.
    fn pestilence(&mut self, dt: f32) {
        let mut camps = vec![];
        for p in &mut self.players {
            if !p.alive {
                p.camped = 0.;
                continue;
            }
            if Vec2::new(p.p[0] - p.camp[0], p.p[2] - p.camp[1]).length() > CAMP {
                p.camp = [p.p[0], p.p[2]];
                p.camped = 0.;
            } else {
                p.camped += dt;
            }
            if p.camped >= LINGER {
                p.camped = 0.;
                camps.push(p.p);
            }
        }
        for at in camps {
            if !self.pests.iter().any(|s| infested(s, at)) {
                self.pests.push(Swarm {
                    id: self.next_pest,
                    p: [at[0], self.level.ground_at(at[0], at[2]), at[2]],
                    hp: 100.,
                    born: self.elapsed,
                    empty: 0.,
                });
                self.next_pest += 1;
            }
        }
        let mut stung = vec![];
        for s in &mut self.pests {
            let before = stung.len();
            stung.extend(
                self.players
                    .iter()
                    .filter(|p| p.alive && infested(s, p.p))
                    .map(|p| (p.id, s.p)),
            );
            s.empty = if stung.len() > before {
                0.
            } else {
                s.empty + dt
            };
        }
        for (id, swarm) in stung {
            self.harm(id, dt * STING, false, Damage::by(PESTS, 0, Vec3::from_array(swarm), Vec3::ZERO));
        }
        self.pests.retain(|s| s.empty < CALM);
    }
    /// A bot steps out of a swarm towards the nearest reachable spot beyond its edge.
    fn flee(&mut self, id: u32, swarm: &Swarm, dt: f32) {
        let Some(p) = self.player(id) else {
            return;
        };
        let at = Vec2::new(p.p[0], p.p[2]);
        let centre = Vec2::new(swarm.p[0], swarm.p[2]);
        let away = (at.y - centre.y).atan2(at.x - centre.x);
        for turn in [0., 0.8, -0.8, 1.6, -1.6, 2.4, -2.4, 3.14] {
            let goal = centre + Vec2::new((away + turn).cos(), (away + turn).sin()) * 0.45;
            if !self.level.route(at, goal).is_empty() {
                self.advance_actor(id, Vec3::new(goal.x, FLOOR, goal.y), dt);
                return;
            }
        }
    }
    /// The turrets: each turns towards the nearest formed zombie it can see within range of its muzzle, pulses
    /// at it once it points that way, and sweeps slowly about the yaw the map gave it when there is none.
    /// Nothing but a zombie is ever fired at, hurt or pushed.
    fn guard(&mut self, dt: f32) {
        let pulse = (self.elapsed * 5.).floor() != ((self.elapsed - dt) * 5.).floor();
        for i in 0..self.posts.len() {
            let (post, base, rest) = self.posts[i];
            let pivot = Vec3::new(post[0] as f32, (crate::map::FLOOR + base) as f32, post[1] as f32);
            // Nearest first: its centre, how far it is from where the muzzle would be, and its bearing.
            let mut best: Option<(u32, Vec3, f32, f32)> = None;
            for e in self.enemies.iter().filter(|e| e.forming <= 0.) {
                let centre = Vec3::from_array(e.p) + Vec3::Y * 0.075;
                let (x, z) = (centre.x - pivot.x, centre.z - pivot.z);
                let gap = x.hypot(z);
                if gap < 0.12 {
                    continue;
                }
                let muzzle = pivot + Vec3::new(x / gap, 0., z / gap) * 0.12;
                let far = centre.distance(muzzle);
                if far > TURRET_RANGE || best.is_some_and(|(_, _, nearest, _)| nearest <= far) || self.screen(muzzle, (centre - muzzle) / far) + 1e-3 < far {
                    continue;
                }
                best = Some((e.id, centre, far, (-x).atan2(-z)));
            }
            let want = best.map_or(rest + TURRET_SWEEP[0] * (self.elapsed * TURRET_SWEEP[1]).sin(), |(.., bearing)| bearing);
            let yaw = self.turret_yaw[i] + wrap(want - self.turret_yaw[i]).clamp(-TURRET_TURN * dt, TURRET_TURN * dt);
            self.turret_yaw[i] = yaw;
            self.turrets[i] = beam(post, base, yaw as f64);
            if let Some((id, centre, ..)) = best.filter(|(.., bearing)| pulse && wrap(bearing - yaw).abs() <= TURRET_AIM) {
                let origin = self.turrets[i].0;
                self.turrets[i].1 = (centre - origin).normalize_or(self.turrets[i].1);
                self.shot(0, origin, centre, 0, 0);
                self.maul(0, id, TURRET_DAMAGE);
            }
        }
    }
    pub(crate) fn begin_famine(&mut self) {
        self.stage = Stage::FreeForAll;
        self.famine_at = self.elapsed;
        self.alert = "FOOD EMERGENCY: ONLY HOTDOGS REMAIN. FREE FOR ALL. EAT TO SURVIVE.".into();
    }
    /// A bot heads for `target`: its body is steered there and the step that follows walks it.
    pub(crate) fn advance_actor(&mut self, id: u32, target: Vec3, _dt: f32) {
        let Some(p) = self.player(id) else {
            return;
        };
        let pace = actor::BOT_WALK * if p.stim > 0. { 1.5 } else { 1. } * self.burden(id) as f64;
        // A target standing higher up is reached by a ramp: first its foot, then a point past its top.
        let heading = self.steer(id, target.as_dvec3().to_array(), pace);
        if let Some(p) = self.player_mut(id) {
            if let Some([dx, dz]) = heading {
                p.yaw = (-dx).atan2(-dz) as f32;
            }
            p.pitch = 0.;
        }
    }
    /// A bot with nothing to do yet paces a small circle (as small as it was at the walk it once had: it turns FASTER).
    fn fidget(&mut self, id: u32, dt: f32) {
        let Some(p) = self.player(id) else {
            return;
        };
        let angle = self.elapsed * 0.7 * actor::FASTER as f32 + id as f32;
        let goal = Vec3::new(p.p[0] + angle.cos() * 0.12, FLOOR, p.p[2] + angle.sin() * 0.12);
        self.advance_actor(id, goal, dt);
    }
    // ----- Bots. A bot uses whatever it holds at the range that suits it, fetches guns and ammunition when
    // nothing presses, strikes with what is in its hand when its target is in reach or its guns are empty,
    // and sprays a swarm that is on it.

    /// Which loaded gun a bot uses on a target `far` metres away: hot sauce up close (and from further off it
    /// closes in to use it), the syrup rifle within its reach, the ketchup sniper at range, a relish glob only
    /// where its burst is `clear` of hotdogs, the mayonnaise within its throw (`squirt`), the mustard pistol
    /// otherwise. The sniper's range starts at 0.8 m: it is preferred to the rifle from there (one round kills a
    /// zombie, and a bot has traded its pistol for it), in every gun mode. 0 when every gun is empty.
    pub(crate) fn arm(&self, p: &Player, far: f32, clear: bool) -> usize {
        let (mut best, mut most) = (0, 0.);
        let rifle = if far <= self.tune.range[RIFLE] * 0.95 { 3.2 } else { 2.5 };
        let sniper = if far <= 0.8 { 1. } else { 3.3 };
        let mayo = if far <= self.squirt(p) * 0.85 { 2.8 } else { 0.2 };
        let scores = [(1, 2.), (2, sniper), (3, if far < 0.6 { 4. } else if far < 1.2 { 2.5 } else { 0.1 }), (4, if clear { 3.5 } else { 0. }), (RIFLE, rifle), (MAYO, mayo)];
        // (A gun with rounds in reserve but none in its magazine will do: pulling its trigger reloads it.)
        for (w, score) in scores {
            if p.owned[w] && crate::magazine::rounds(p, w) > 0 && score > most {
                (most, best) = (score, w);
            }
        }
        best
    }
    /// Whether a glob flying from `o` to `c` would burst clear of every other living hotdog wherever along
    /// the way it lands, allowing for how far a bot can walk meanwhile. During the wave a bot fires no glob
    /// that fails this: its burst hurts no hotdog then, but it is not to land among its team.
    fn clear_of(&self, id: u32, o: Vec3, c: Vec3) -> bool {
        let room = actor::BLAST_RADIUS as f32 + 0.05 + actor::BOT_WALK as f32 * o.distance(c) / self.tune.speed[4];
        let (x, z) = (c.x - o.x, c.z - o.z);
        let l = if x * x + z * z > 0. { x * x + z * z } else { 1. };
        self.players.iter().filter(|q| q.id != id && q.alive).all(|q| {
            let t = (((q.p[0] - o.x) * x + (q.p[2] - o.z) * z) / l).clamp(0., 1.);
            (q.p[0] - o.x - x * t).hypot(q.p[2] - o.z - z * t) >= room
        })
    }
    /// Whether a pickup is worth a bot's walk: a gun it lacks that it has room for (see `swap`), more for a gun that is
    /// running low (with room in its reserve), or a box of ammunition while a gun it holds that the box tops up (not a
    /// power weapon) is below half its FULL load, magazine and reserve together. The same in every gun mode.
    pub(crate) fn wants(p: &Player, item: &Pickup) -> bool {
        use crate::magazine::{rounds, CARRY};
        let w = item.weapon;
        if w == crate::map::AMMO_BOX {
            return (1..GUNS).any(|g| p.owned[g] && (rounds(p, g) as f32) < FULL[g] as f32 / 2. && p.reserve[g] < CARRY[g] && !crate::magazine::power(g));
        }
        if w >= GUNS {
            return false;
        }
        if p.owned[w] {
            (rounds(p, w) as f32) < item.ammo as f32 / 2. && p.reserve[w] < CARRY[w]
        } else {
            Self::swap(p, w).is_some()
        }
    }
    /// What a bot gives up to take the gun `w` it lacks, the same in every gun mode: nothing (0) while it has a hand
    /// free, when any gun will do; otherwise an empty gun, though not for bug spray. Every hotdog starts with the syrup
    /// rifle and the mustard pistol, so its hands are full from the start: a bot holds its pistol as it would a free
    /// hand, and trades it for any gun it lacks (for bug spray only while its other gun is a loaded gun it can shoot
    /// with). None when it keeps what it has.
    pub(crate) fn swap(p: &Player, w: usize) -> Option<usize> {
        let held: Vec<usize> = (1..GUNS).filter(|g| p.owned[*g]).collect();
        if held.len() < 2 {
            return Some(0);
        }
        let empty = held.iter().copied().find(|g| crate::magazine::rounds(p, *g) <= 0).filter(|_| w != 5);
        let shoots = held.iter().any(|g| *g != 1 && *g != 5 && crate::magazine::rounds(p, *g) > 0);
        empty.or_else(|| (p.owned[1] && (w != 5 || shoots)).then_some(1))
    }
    /// Whether a bot's guns are low: none it can fire at a target (bug spray aside) has a quarter of its FULL load
    /// left, magazine and reserve together. A bot that is low goes further for a pickup.
    pub(crate) fn low(p: &Player) -> bool {
        [1, 2, 3, 4, RIFLE, MAYO].into_iter().all(|g| !p.owned[g] || (crate::magazine::rounds(p, g) as f32) < FULL[g] as f32 / 4.)
    }
    /// A bot's shopping, within `reach` metres (0: not now). It settles on the nearest pickup it wants that
    /// no other bot is after (bug spray counts as a metre further), walks there by the level's route and
    /// collects it, giving up an empty gun or its mustard pistol if its hands are full (see `swap`); one it
    /// cannot get to in 12 seconds it gives up on for the round. A bot knows that pickups come back: one that
    /// has been taken is worth setting out for if it will be back by the time the bot gets there, and a bot
    /// that gets there first waits on the pad. Returns whether the bot spent this step on it.
    pub(crate) fn errand(&mut self, id: u32, reach: f32, dt: f32) -> bool {
        let Some(p) = self.player(id).cloned() else {
            return false;
        };
        if reach <= 0. {
            self.player_mut(id).unwrap().errand = None;
            return false;
        }
        let away = |item: &Pickup| (item.p[0] - p.p[0]).hypot(item.p[2] - p.p[2]);
        let pace = actor::BOT_WALK as f32;
        let due = |item: &Pickup, now: f32| item.ready <= now + away(item) / pace;
        let mut item = p.errand.and_then(|index| self.pickups.get(index)).cloned();
        if let Some(chosen) = &item {
            let late = self.elapsed - p.errand_at > 12.;
            if chosen.ready > self.elapsed + away(chosen) / pace + WAIT || !Self::wants(&p, chosen) || late {
                let me = self.player_mut(id).unwrap();
                if late {
                    me.shun.push(chosen.id);
                }
                me.errand = None;
                item = None;
            }
        }
        let beat = |t: f32| (t * 4. + id as f32 / 5.).floor();
        if item.is_none() && beat(self.elapsed) != beat(self.elapsed - dt) {
            let taken: Vec<usize> = self.players.iter().filter(|q| q.id != id && q.bot && q.alive).filter_map(|q| q.errand).collect();
            let mut options: Vec<(f32, Pickup)> = self
                .pickups
                .iter()
                .filter(|i| due(i, self.elapsed) && !taken.contains(&i.id) && !p.shun.contains(&i.id) && self.within_reach(&p, i) && Self::wants(&p, i))
                .map(|i| (away(i) + if i.weapon == 5 { 1. } else { 0. }, i.clone()))
                .filter(|(cost, _)| *cost <= reach)
                .collect();
            options.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
            for (_, option) in options {
                if away(&option) < 0.15 || !self.level.path([p.p[0] as f64, p.p[2] as f64], [option.p[0] as f64, option.p[2] as f64]).is_empty() {
                    let now = self.elapsed;
                    let me = self.player_mut(id).unwrap();
                    (me.errand, me.errand_at) = (Some(option.id), now);
                    item = Some(option);
                    break;
                }
                self.player_mut(id).unwrap().shun.push(option.id);
            }
        }
        let Some(item) = item else {
            return false;
        };
        if away(&item) < 0.12 && (item.p[1] - 0.04 - p.p[1]).abs() < 0.05 {
            // There before it is back: it waits on the pad.
            if item.ready > self.elapsed {
                return true;
            }
            let me = self.player_mut(id).unwrap();
            if item.weapon != crate::map::AMMO_BOX && !me.owned[item.weapon] && (1..GUNS).filter(|g| me.owned[*g]).count() >= 2 {
                // (A third gun replaces the one in hand: it puts in its hand the gun it gives up.)
                let given = Self::swap(me, item.weapon).filter(|g| *g > 0).unwrap_or(me.gun);
                (me.weapon, me.gun) = (given, given);
            }
            me.errand = None;
            self.pickup(id, item.id);
            return true;
        }
        self.advance_actor(id, Vec3::new(item.p[0], item.p[1] - 0.04, item.p[2]), dt);
        true
    }
    /// The unit direction from a bot's muzzle to the point `c`, led by the velocity `v` for a round of the
    /// given speed (0: no lead).
    pub(crate) fn aim(&self, id: u32, c: Vec3, v: Vec3, speed: f32) -> Vec3 {
        let o = self.player(id).map_or(c, muzzle);
        let mark = if speed > 0. { lead(o, c, v, speed) } else { c };
        (mark - o).normalize_or(Vec3::NEG_Z)
    }
    /// How fast a hotdog's mayonnaise leaves the nozzle: faster stimmed.
    pub fn squeeze(&self, p: &Player) -> f32 {
        self.tune.speed[MAYO] * if p.stim > 0. { self.tune.stim_squirt } else { 1. }
    }
    /// How far a hotdog's mayonnaise reaches across the level at its best, aimed 45 degrees up.
    pub(crate) fn squirt(&self, p: &Player) -> f32 {
        throw(self.squeeze(p), self.tune.fall[MAYO], std::f32::consts::FRAC_PI_4, EYE - CHEST[0])
    }
    /// The way a bot aims the mayonnaise from its muzzle at the point `c`, which moves at `v`: along the low arc where
    /// nothing is in its way, or else lobbed over on the high one; None where neither reaches or both are blocked.
    pub(crate) fn lob(&self, id: u32, c: Vec3, v: Vec3) -> Option<Vec3> {
        let p = self.player(id)?;
        let (o, speed) = (muzzle(p), self.squeeze(p));
        for high in [0, 1] {
            // Where the target will be when the stream gets there, worked out again three times.
            let (mut mark, mut way) = (c, None);
            for _ in 0..3 {
                let flat = Vec2::new(mark.x - o.x, mark.z - o.z);
                let across = flat.length();
                way = lobs(speed, self.tune.fall[MAYO], across, mark.y - o.y).map(|rise| {
                    let d = Vec3::new(flat.x / across, rise[high], flat.y / across).normalize();
                    (d, across / (speed * Vec2::new(d.x, d.z).length()).max(1e-3))
                });
                let Some((_, t)) = way else { break };
                mark = c + v * t;
            }
            if let Some((d, _)) = way.filter(|(d, t)| self.open_arc(o, *d * speed, *t)) {
                return Some(d);
            }
        }
        None
    }
    /// Whether a stream leaving `o` at the velocity `launch` flies `t` seconds (all but the last of it, where it
    /// meets its mark) without a block, a loose object or the ground in its way.
    fn open_arc(&self, o: Vec3, launch: Vec3, t: f32) -> bool {
        let mut a = o;
        for k in 1..=10 {
            let (b, _) = arc(o, launch, self.tune.fall[MAYO], t * 0.95 * k as f32 / 10.);
            let (d, l) = ((b - a).normalize_or_zero(), a.distance(b));
            if (l > 1e-5 && self.cover(a, d) < l) || b.y < self.level.ground(b.x as f64, b.z as f64) as f32 {
                return false;
            }
            a = b;
        }
        true
    }
    /// A bot backs away from `from` without turning its back on it.
    fn retreat(&mut self, id: u32, from: Vec3, dt: f32) {
        let Some(p) = self.player(id) else {
            return;
        };
        let (at, yaw, pitch) = (Vec3::from_array(p.p), p.yaw, p.pitch);
        let away = Vec3::new(at.x - from.x, 0., at.z - from.z).normalize_or(Vec3::X);
        let goal = at + away * 0.4;
        if self.level.clear(goal.x as f64, goal.z as f64) {
            self.advance_actor(id, goal, dt);
            if let Some(p) = self.player_mut(id) {
                (p.yaw, p.pitch) = (yaw, pitch);
            }
        }
    }
    pub(crate) fn face(&mut self, id: u32, d: Vec3) {
        if let Some(p) = self.player_mut(id) {
            p.yaw = (-d.x).atan2(-d.z);
            if let Some(d) = d.try_normalize() {
                p.aim = d.to_array();
            }
        }
    }
    fn bot(&mut self, id: u32, dt: f32) {
        // The sandbox's targets stand still or walk to and fro (rules.rs).
        if self.drill(id, dt) {
            return;
        }
        // Kung Fu has bots of its own (kungfu.rs), and so has Titan (titan.rs).
        if self.kungfu.is_some() {
            return self.kungfu_bot(id, dt);
        }
        if self.titan.is_some() {
            return self.titan_bot(id, dt);
        }
        // A bot on a griddle that is on, or still warm, walks off it (griddle.rs).
        if self.scorched(id) {
            return;
        }
        // An empty magazine in its hands is reloaded at once, if there is anything to reload it from (magazine.rs); it
        // goes on about its business meanwhile, without firing.
        self.top_up(id, 0.);
        // A relish glob coming at it it may jump, knees tucked (aim.rs).
        self.dodge(id);
        // Forts has bots of its own (forts.rs).
        if self.fort.is_some() {
            return self.fort_bot(id, dt);
        }
        let Some(p) = self.player(id).cloned() else {
            return;
        };
        let origin = Vec3::from_array(p.p);
        let swarm = self.pests.iter().find(|s| infested(s, p.p)).cloned();
        let loaded = [1, 2, 3, 4, RIFLE, MAYO].into_iter().any(|w| p.owned[w] && crate::magazine::rounds(&p, w) > 0);
        // Pests on it: with the can in its hands it sprays them where it stands; without, it steps out of
        // them (below).
        if swarm.is_some() && p.owned[5] && crate::magazine::rounds(&p, 5) > 0 && self.stage != Stage::Warmup {
            self.fire(id, Vec3::new(-p.yaw.sin() * 0.6, -0.8, -p.yaw.cos() * 0.6), 5);
            return;
        }
        match self.stage {
            Stage::Wave => {
                let target = self
                    .enemies
                    .iter()
                    .filter(|e| e.forming <= 0.)
                    .min_by(|a, b| Vec3::from_array(a.p).distance_squared(origin).total_cmp(&Vec3::from_array(b.p).distance_squared(origin)))
                    .cloned();
                // Nothing has formed yet: the hotdogs reload, fetch what they want, or shift about while the ooze spreads.
                let Some(target) = target else {
                    self.top_up(id, 1.);
                    if self.errand(id, 1.6, dt) {
                        return;
                    }
                    if let Some(swarm) = &swarm {
                        self.flee(id, swarm, dt);
                    } else if !self.enemies.is_empty() || self.pending() > 0 {
                        self.fidget(id, dt);
                    }
                    return;
                };
                let (goal, far) = (Vec3::from_array(target.p), origin.distance(Vec3::from_array(target.p)));
                let centre = goal + Vec3::Y * 0.075;
                // In reach it strikes with what it holds. Out of ammunition it goes for more if there is any to
                // be had, and otherwise closes in to strike.
                if far < 0.17 {
                    let d = self.aim(id, centre, Vec3::ZERO, 0.);
                    self.face(id, d);
                    self.melee(id, d);
                    if let Some(swarm) = &swarm {
                        self.flee(id, swarm, dt);
                    }
                    return;
                }
                if self.errand(id, if !loaded { 3. } else if far > 0.9 { if Self::low(&p) { 1.2 } else { 0.5 } } else { 0. }, dt) {
                    return;
                }
                let gun = self.arm(&p, far, far > 0.6 && self.clear_of(id, muzzle(&p), centre));
                if gun > 0 {
                    self.select(id, gun);
                    let walk = Vec3::new(-target.yaw.sin(), 0., -target.yaw.cos()) * actor::ZOMBIE_WALK as f32;
                    // It fires when it has a clear shot within the gun's reach, aiming as a person does (aim.rs); the
                    // hot sauce it carries into range first, and with the sniper it keeps its distance. The mayonnaise
                    // it lobs, at the target or over whatever stops its other gun.
                    let key = crate::aim::cat(target.id);
                    let clear = gun != MAYO && self.cover(muzzle(&p), self.aim(id, centre, walk, self.tune.speed[gun])) + 0.02 >= far && far <= self.tune.range[gun] * 0.95 && !(gun == 3 && far >= 0.6);
                    let mut shot = clear.then(|| (gun, self.draw_bead(id, key, centre, walk, gun, None)));
                    if shot.is_none() && gun != 3 && p.owned[MAYO] && crate::magazine::rounds(&p, MAYO) > 0 {
                        // (A zombie that cannot walk straight at it is not led: it is going round, or nowhere.)
                        let open = self.level.line([goal.x as f64, goal.z as f64], [origin.x as f64, origin.z as f64]);
                        shot = self.draw_lob(id, key, centre, if open { walk } else { Vec3::ZERO }).map(|lobbed| (MAYO, lobbed));
                    }
                    if let Some((gun, (d, ready))) = shot {
                        self.select(id, gun);
                        self.loose(id, d, ready, gun);
                        if let Some(swarm) = &swarm {
                            self.flee(id, swarm, dt);
                        } else if gun == 2 && far < 0.7 {
                            self.retreat(id, goal, dt);
                        }
                        return;
                    }
                }
                self.advance_actor(id, goal, dt);
            }
            Stage::FreeForAll => {
                // FRANKIE and MUSTARD are busy with the demo's opening betrayal until it has been fired.
                if self.demo && !self.demo_shot && (id == 2 || id == 3) {
                    return;
                }
                if self.elapsed - self.famine_at <= 4. {
                    if let Some(swarm) = &swarm {
                        self.flee(id, swarm, dt);
                    }
                    return;
                }
                // (Only the siege's famine eats and bites: in Free For All a bot strikes and shoots, and corpses are no food.)
                let famine = self.famine();
                let prey = self
                    .players
                    .iter()
                    .filter(|t| t.id != id && !t.consumed && (t.alive || (famine && p.hp < 80.)))
                    .min_by(|a, b| Vec3::from_array(a.p).distance_squared(origin).total_cmp(&Vec3::from_array(b.p).distance_squared(origin)))
                    .cloned();
                let Some(prey) = prey else {
                    return;
                };
                let (goal, far) = (Vec3::from_array(prey.p), origin.distance(Vec3::from_array(prey.p)));
                let centre = chest(&prey);
                // A body to eat, hunger (the famine's), or nothing to shoot with: it closes in and eats, bites or
                // strikes (fetching ammunition first if some is nearer than its prey).
                let hungry = famine && p.hp < 50.;
                if !prey.alive || hungry || !loaded || far < 0.15 {
                    if far < 0.15 {
                        let d = self.aim(id, centre, Vec3::ZERO, 0.);
                        self.face(id, d);
                        if !self.eat(id) {
                            self.melee(id, d);
                        }
                        return;
                    }
                    if prey.alive && !loaded && !hungry && self.errand(id, far, dt) {
                        return;
                    }
                    if far > 0.5 {
                        self.stim(id);
                    }
                    self.advance_actor(id, goal, dt);
                    return;
                }
                if self.errand(id, if far > 1. { if Self::low(&p) { 1.2 } else { 0.5 } } else { 0. }, dt) {
                    return;
                }
                // Otherwise it shoots: it leads its rival, closes in with the hot sauce and the pistol, and
                // backs off with the sniper and the relish, whose burst it stays clear of.
                let gun = self.arm(&p, far, far > 0.6);
                // (Out of the gun's reach it reloads a magazine that is less than half full.)
                if gun > 0 && gun == p.weapon && far > self.tune.range[gun] {
                    self.top_up(id, 0.5);
                }
                // (Left with only a glob and a rival inside its burst, it makes room.)
                if gun == 0 {
                    let d = self.aim(id, centre, Vec3::ZERO, 0.);
                    self.face(id, d);
                    self.retreat(id, goal, dt);
                    return;
                }
                self.select(id, gun);
                let v = Vec3::from_array(prey.v);
                // It sees its rival where nothing solid is in the way of a round at it; then it aims as a person does
                // (aim.rs), at the chest (a relish glob at the feet).
                let o = muzzle(&p);
                let mut seen = gun != MAYO && self.cover(o, self.aim(id, centre, v, self.tune.speed[gun])) + 0.02 >= far && far <= self.tune.range[gun] * 0.95;
                let mut shot = None;
                if seen {
                    let (mark, face) = self.mark(o, &prey, gun);
                    shot = Some(self.draw_bead(id, prey.id, mark, v, gun, face));
                }
                // The mayonnaise it lobs: at its rival, or over whatever stops its other gun. With it it keeps to the
                // middle of its throw.
                let mut gun = gun;
                if !seen && gun != 3 && p.owned[MAYO] && crate::magazine::rounds(&p, MAYO) > 0 {
                    if let Some(lobbed) = self.draw_lob(id, prey.id, centre, v) {
                        (gun, shot, seen) = (MAYO, Some(lobbed), true);
                        self.select(id, MAYO);
                    }
                }
                let d = shot.map_or(Vec3::NEG_Z, |shot| shot.0);
                if let Some((d, ready)) = shot.filter(|_| !(gun == 3 && far >= 0.6)) {
                    self.loose(id, d, ready, gun);
                }
                if !seen || gun == 3 || ((gun == 1 || gun == RIFLE) && far > 0.7) || (gun == MAYO && far > self.squirt(&p) * 0.7) {
                    self.advance_actor(id, goal, dt);
                } else if (gun == 2 && far < 0.7) || (gun == 4 && far < 0.8) || (gun == MAYO && far < 0.3) {
                    self.retreat(id, goal, dt);
                }
                if seen {
                    self.face(id, d);
                }
            }
            // All five actors move during the preparation period: to the guns they want, or about where
            // they stand.
            Stage::Warmup => {
                self.top_up(id, 1.);
                if !self.errand(id, 1.6, dt) {
                    self.fidget(id, dt);
                }
            }
            Stage::RoundOver | Stage::Build | Stage::Fight => {}
        }
    }
    pub fn tick(&mut self, seconds: f32) {
        let mut remaining = seconds.clamp(0., 10.);
        while remaining > 0.00001 {
            let dt = remaining.min(1. / 30.);
            self.step(dt);
            remaining -= dt;
        }
    }
    fn step(&mut self, dt: f32) {
        self.elapsed += dt;
        self.shots.retain(|s| self.elapsed - s.time < 0.4);
        self.splats.retain(|s| self.elapsed - s.time < 0.4);
        self.knocks.retain(|k| self.elapsed - k.time < 0.4);
        for p in &mut self.players {
            // (A step's worth below nothing: what the mayonnaise is owed towards its next droplet; see `fire`.)
            p.cooldown = (p.cooldown - dt).max(-dt);
            p.bite_cooldown = (p.bite_cooldown - dt).max(0.);
            p.stim = (p.stim - dt).max(0.);
            p.stim_wait = (p.stim_wait - dt).max(0.);
            // Its aim settles from its shots' kicks (recoil.rs), at the rate of the gun in hand (or last in hand).
            p.bloom = (p.bloom - crate::recoil::settling(if p.weapon > 0 { p.weapon } else { p.gun }) * dt).max(0.);
        }
        // Reloads go on, and are done or cancelled (magazine.rs).
        self.reloads(dt);
        // An aim that may no longer be is let go: the gun put away, the hands filled, death, a bot taking the seat.
        for i in 0..self.players.len() {
            if self.players[i].aimed && !self.may_aim(self.players[i].id) {
                self.players[i].aimed = false;
            }
        }
        self.volleys(dt);
        self.embody();
        self.flight(dt);
        // The griddles: the plates warm or cool, and whoever stands on a hot one burns.
        self.heat_griddles(dt);
        // Fully Loaded: the table's own clock; the game's end is the round's, and the pause after it is as any other's.
        if self.cards.is_some() && self.stage != Stage::RoundOver {
            return self.cards_step(dt);
        }
        // Forts: the pause after a round, then the next round or the next match.
        if self.fort.is_some() && self.stage == Stage::RoundOver {
            return self.rest(dt);
        }
        if self.stage == Stage::RoundOver {
            if (self.demo || self.follow) && self.elapsed - self.over_at > 8. {
                self.reset();
            } else {
                self.settle(dt);
                // (A felled or roaring titan's figure goes on as drawn.)
                if self.titan.is_some() {
                    self.titan_figure(dt);
                }
            }
            return;
        }
        if self.stage == Stage::Warmup && self.elapsed >= self.warmup {
            self.stage = Stage::Wave;
            self.alert = "ZOMBIE WAVE 1 / 1. COOPERATE WHILE YOU CAN.".into();
        }
        // The microwave's timer goes off: the wave oozes out of it.
        if self.stage == Stage::Wave && self.wave_spawned + self.pending() < self.wave() {
            self.emerge(self.wave() - self.wave_spawned - self.pending());
        }
        self.seep(dt);
        // A zombie that has just formed gets its body, pushing aside whoever stood on its puddle.
        self.embody();
        // (The sandbox's cats, with zombies=on, go for whoever is nearest whatever the stage: rules.rs.)
        if matches!(self.stage, Stage::Wave | Stage::Fight) || self.rules.roaming() {
            let mut taken: HashMap<u32, [bool; 16]> = HashMap::new();
            let zombies = self.enemies.clone();
            for z in zombies {
                // A zombie still rising out of its puddle neither moves nor bites.
                if z.forming > 0. {
                    continue;
                }
                let target = self
                    .players
                    .iter()
                    .filter(|p| p.alive)
                    .min_by(|a, b| {
                        Vec3::from_array(a.p)
                            .distance_squared(Vec3::from_array(z.p))
                            .total_cmp(
                                &Vec3::from_array(b.p).distance_squared(Vec3::from_array(z.p)),
                            )
                    })
                    .cloned();
                if let Some(target) = target {
                    let delta = Vec3::from_array(target.p) - Vec3::from_array(z.p);
                    if delta.length() < BITE {
                        self.harm(target.id, dt * 14., false, Damage::by(ZOMBIE, 0, Vec3::from_array(z.p), Vec3::new(delta.x, 0., delta.z)));
                    }
                    let (Some(at), Some(centre)) = (self.bodies.get(CAT + z.id).map(|body| body.p), self.bodies.get(target.id).map(|body| body.p)) else {
                        continue;
                    };
                    // Each zombie has a place in the ring round its hotdog. On it, it stands facing the
                    // hotdog rather than walking into it.
                    let place = self.place(at, centre, taken.entry(target.id).or_insert([false; 16]));
                    let (ux, uz) = (at[0] - centre[0], at[2] - centre[2]);
                    let (px, pz) = place.map_or((0., 0.), |place| (place[0] - at[0], place[1] - at[2]));
                    let far = px * px + pz * pz;
                    if if place.is_some() { far < SETTLED * SETTLED } else { delta.length() < STAND_OFF } {
                        if let Some(e) = self.enemies.iter_mut().find(|e| e.id == z.id) {
                            e.yaw = ux.atan2(uz) as f32;
                        }
                        continue;
                    }
                    // Otherwise it prowls there (straight at the hotdog when the ring is full): its body does
                    // the walking, and it goes round the back of the ring until its place is before it.
                    let mut goal = place.unwrap_or([centre[0], centre[2]]);
                    if let Some(place) = place {
                        let d = (ux * ux + uz * uz).sqrt();
                        let (sx, sz) = (place[0] - centre[0], place[1] - centre[2]);
                        let r = (sx * sx + sz * sz).sqrt();
                        if d > 1e-9 && ux * sx + uz * sz < TURN * d * r {
                            let side = if ux * sz - uz * sx >= 0. { 1. } else { -1. };
                            goal = [centre[0] + (ux * 0.5 - side * uz * TURN) / d * OUTER, centre[2] + (side * ux * TURN + uz * 0.5) / d * OUTER];
                        }
                    }
                    if let Some([dx, dz]) = self.prowl(CAT + z.id, [goal[0], centre[1], goal[1]], actor::ZOMBIE_WALK) {
                        if let Some(e) = self.enemies.iter_mut().find(|e| e.id == z.id) {
                            e.yaw = (-dx).atan2(-dz) as f32;
                        }
                    }
                }
            }
            if self.stage == Stage::Wave && self.wave_spawned >= self.wave() && self.enemies.is_empty() && self.pending() == 0 {
                self.begin_famine();
            }
        }
        // (Free For All and the sandbox have no hunger, no pests and no last hotdog standing: rules.rs.)
        if self.stage == Stage::FreeForAll && !self.rules.active() {
            let ids: Vec<u32> = self
                .players
                .iter()
                .filter(|p| p.alive)
                .map(|p| p.id)
                .collect();
            for id in ids {
                let at = self.player(id).map_or(Vec3::ZERO, |p| Vec3::from_array(p.p));
                self.harm(id, dt * 2.2, false, Damage::by(HUNGER, 0, at, Vec3::ZERO));
            }
            // A visible opening betrayal demonstrates that friendly fire is now enabled: FRANKIE shoots MUSTARD,
            // walking up to it first if it has no clear shot from within 1.2 m (the two of them do nothing else
            // until it has been fired, or one of them is dead, or 12 seconds have gone). It is a real pull of the
            // trigger: of the mustard pistol if FRANKIE still has it, or else of the syrup rifle every hotdog starts
            // with (a bot trades its pistol for the first gun it collects, see `swap`), a burst of three.
            if self.demo && !self.demo_shot && self.elapsed - self.famine_at > 3. {
                if self.players[1].alive && self.players[2].alive && self.elapsed - self.famine_at < 12. {
                    let frankie = &self.players[1];
                    let gun = [1, RIFLE].into_iter().find(|g| frankie.owned[*g] && frankie.ammo[*g] > 0).unwrap_or(frankie.gun);
                    let origin = muzzle(frankie);
                    let dir = (chest(&self.players[2]) - origin).normalize_or(Vec3::NEG_Z);
                    let (at, far) = (Vec3::from_array(self.players[2].p), Vec3::from_array(self.players[1].p).distance(Vec3::from_array(self.players[2].p)));
                    // (Nor with another hotdog in the way: the burst is MUSTARD's.)
                    let target = chest(&self.players[2]);
                    let between = self.players.iter().skip(3).chain(self.players.iter().take(1)).filter(|q| q.alive).any(|q| {
                        let t = (chest(q) - origin).dot(dir).clamp(0., far);
                        (origin + dir * t).distance(chest(q)) < 0.07 && t < origin.distance(target)
                    });
                    if far <= 1.2 && self.cover(origin, dir) + 0.02 >= far && !between {
                        (self.players[1].yaw, self.players[1].pitch) = ((-dir.x).atan2(-dir.z), dir.y.clamp(-1., 1.).asin());
                        // (Not yet ready to fire: it tries again next step.)
                        self.demo_shot = self.fire(2, dir, gun);
                    } else {
                        self.advance_actor(2, at, dt);
                    }
                } else {
                    self.demo_shot = true;
                }
            }
        }
        self.guard(dt);
        if matches!(self.stage, Stage::Wave | Stage::FreeForAll) && !self.rules.active() {
            self.pestilence(dt);
        }
        // Kung Fu: the moves run on and land, before the bots choose their next (kungfu.rs).
        if self.kungfu.is_some() {
            self.kungfu_step(dt);
        }
        // Titan: the titan's move runs on, its blows land and its waves spread, before the bots choose (titan.rs).
        if self.titan.is_some() {
            self.titan_step(dt);
        }
        // The pickups that give rounds alone are taken by walking over them (magazine.rs), before the bots' errands.
        self.scavenge();
        let bots: Vec<u32> = self.players.iter().filter(|p| p.bot && p.alive).map(|p| p.id).collect();
        for id in bots {
            self.bot(id, dt);
        }
        // The round is over once fewer than two hotdogs are left: in the free for all, and in a wave that
        // is too much for them.
        if matches!(self.stage, Stage::Wave | Stage::FreeForAll) && !self.rules.active() {
            let living: Vec<u32> = self
                .players
                .iter()
                .filter(|p| p.alive)
                .map(|p| p.id)
                .collect();
            if living.len() <= 1 {
                self.stage = Stage::RoundOver;
                self.over_at = self.elapsed;
                // Rounds still in the air can no longer hurt anybody: the winner stays the winner.
                for q in &mut self.projectiles {
                    q.hostile = false;
                }
                self.winner = living.first().copied();
                self.alert = self
                    .winner
                    .and_then(|id| self.player(id))
                    .map(|p| format!("{} IS THE LAST HOTDOG STANDING", p.name))
                    .unwrap_or_else(|| "NO SURVIVORS. THE KITCHEN GOES QUIET.".into());
            }
        }
        // Free For All and the sandbox: respawns, the kill limit, the clock, unlimited rounds (rules.rs).
        self.referee(dt);
        // Forts: the build phase's clock, the bacon, the round's end, the stalemate (none of it in the sandbox).
        if !self.rules.sandbox {
            self.rule(dt);
        }
        self.settle(dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// `fit` (the server's reset of one hotdog) sets a hotdog out exactly as a new round does, in a siege and in Forts,
    /// for a person and for a bot; and on another team in Forts, at that team's start.
    #[test]
    fn a_hotdog_fitted_out_again_is_as_a_new_round_sets_it_out() {
        let forts = Map::parse(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/maps/pillow-fort.json")).unwrap()).unwrap();
        for (map, demo) in [(Map::bundled(), false), (Map::bundled(), true), (forts.clone(), false)] {
            let mut s = Feast::with_map(map, demo);
            let fresh: Vec<String> = s.players.iter().map(|p| serde_json::to_string(p).unwrap() + &format!("{:?}", p.job)).collect();
            for p in s.players.iter_mut() {
                (p.p, p.hp, p.ammo[1], p.score) = ([0., 0., 0.], 7., 99, 5);
            }
            for id in 1..=s.players.len() as u32 {
                s.fit(id);
            }
            let fitted: Vec<String> = s.players.iter().map(|p| serde_json::to_string(p).unwrap() + &format!("{:?}", p.job)).collect();
            assert_eq!(fitted, fresh);
        }
        let mut s = Feast::with_map(forts, false);
        s.players[0].team = 1;
        s.fit(1);
        let (p, level) = (&s.players[0], &s.level);
        assert_eq!(level.side(p.p[0] as f64, p.p[2] as f64), level.fort.as_ref().unwrap().sides[1]);
        assert_eq!(p.name, forts::NAMES[1][0]);
    }
    #[test]
    fn five_active_demo_players() {
        let mut s = Feast::new(true);
        let old = s.players[0].p;
        s.tick(1.);
        assert_eq!(s.players.len(), 5);
        assert!(s.players.iter().all(|p| p.bot && p.alive));
        assert_ne!(s.players[0].p, old);
    }
    #[test]
    fn friendly_fire_is_gated_by_wave_completion() {
        let mut s = Feast::new(false);
        s.players[0].p = [0., FLOOR, 2.4];
        s.players[1].p = [0., FLOOR, 2.7];
        s.players[2].p = [1.2, FLOOR, 2.3];
        s.stage = Stage::Wave;
        assert!(s.fire(1, Vec3::Z, 1));
        assert!(land(&mut s) > 0);
        assert_eq!(s.players[1].hp, 100.);
        s.players[0].cooldown = 0.;
        s.begin_famine();
        assert!(s.fire(1, Vec3::Z, 1));
        assert_eq!(s.players[1].hp, 100.);
        land(&mut s);
        assert_eq!(s.players[1].hp, 82.);
        assert_eq!(s.friendly_fire_events, 1);
    }
    /// Rounds are slow: fly them until every one has landed and report how many 30 Hz steps that took.
    fn land(s: &mut Feast) -> u32 {
        let mut steps = 0;
        while !s.projectiles.is_empty() && steps < 600 {
            s.flight(1. / 30.);
            steps += 1;
        }
        steps
    }
    /// A free-for-all duel on open counter: hotdog 1 faces hotdog 2 from 0.3 m; the rest are parked.
    fn duel() -> Feast {
        let mut s = Feast::new(false);
        s.begin_famine();
        for (i, p) in s.players.iter_mut().enumerate() {
            p.p = [-1.5 + i as f32 * 0.1, FLOOR, 3.55];
        }
        s.players[0].p = [0., FLOOR, 2.4];
        s.players[1].p = [0., FLOOR, 2.7];
        s
    }
    #[test]
    fn guns_launch_rounds_at_their_speed_that_a_rival_far_enough_off_can_still_dodge() {
        let mut s = duel();
        // (0.28 m off: a level round from the muzzle meets the rival's head, head.rs, its front 0.037 m nearer.)
        s.players[1].p[2] = 2.68;
        assert!(s.fire(1, Vec3::Z, 1));
        assert_eq!(s.projectiles.len(), 1);
        assert!((Vec3::from_array(s.projectiles[0].v).length() - ROUND_SPEED[1]).abs() < 1e-6);
        // 0.243 m to its head, mustard at 7.5 m/s (0.25 m a step) is there within a 30 Hz step.
        let steps = land(&mut s);
        assert_eq!(steps, 1, "mustard took {steps} steps to cross 0.243 m");
        assert_eq!(s.players[1].hp, 82.);
        assert_eq!(s.splats.len(), 1);
        assert_eq!(s.shots.last().unwrap().projectile, s.splats[0].projectile);
        // A metre off it takes four steps: standing still the rival is hit, and stepping aside after two it is missed.
        for dodge in [false, true] {
            let mut s = duel();
            s.players[1].p = [0., FLOOR, 3.4];
            s.fire(1, Vec3::Z, 1);
            s.flight(1. / 30.);
            s.flight(1. / 30.);
            assert_eq!(s.players[1].hp, 100.);
            if dodge {
                s.players[1].p = [0.3, FLOOR, 3.4];
            }
            let steps = 2 + land(&mut s);
            assert_eq!((s.players[1].hp, s.friendly_fire_events), if dodge { (100., 0) } else { (82., 1) }, "after {steps} steps");
        }
    }
    #[test]
    fn each_gun_has_its_own_round_speed_and_reach() {
        assert!(ROUND_SPEED[2] > ROUND_SPEED[1] && ROUND_SPEED[1] > ROUND_SPEED[5]);
        assert!(ROUND_SPEED[5] > ROUND_SPEED[3] && ROUND_SPEED[3] > ROUND_SPEED[4]);
        assert!(ROUND_SPEED[1] <= 7.5, "mustard is the slowest of the pistols and rifles");
        for w in 1..6 {
            let mut s = Feast::new(false);
            s.stage = Stage::Wave;
            s.players[0].owned[w] = true;
            s.players[0].ammo[w] = 5;
            s.players[0].p = [-1.7, FLOOR, 2.3];
            assert!(s.fire(1, Vec3::X, w));
            assert_eq!(s.projectiles[0].weapon, w);
            let steps = land(&mut s) as f32;
            let expected = RANGE[w] / ROUND_SPEED[w] * 30.;
            assert!((steps - expected).abs() <= 1.01, "weapon {w}: {steps} steps, about {expected} expected");
            assert!((s.splats[0].p[0] - (-1.7 + RANGE[w])).abs() < 1e-3);
            assert_eq!(s.players[0].ammo[w], 4);
        }
    }
    #[test]
    fn rounds_stop_at_cover_and_the_countertop() {
        let mut s = duel();
        s.players[0].p = [-0.34, FLOOR, 2.5];
        s.players[1].p = [-0.34, FLOOR, 3.0];
        s.fire(1, Vec3::Z, 1);
        land(&mut s);
        assert_eq!(s.players[1].hp, 100.);
        assert!((s.splats[0].p[2] - 2.70).abs() < 1e-4);
        let mut s = duel();
        s.fire(1, Vec3::new(0., -0.6, 0.8), 1);
        land(&mut s);
        assert!((s.splats[0].p[1] - FLOOR).abs() < 1e-5);
        assert!((s.splats[0].p[2] - 2.58).abs() < 1e-4);
        assert_eq!(s.players[1].hp, 100.);
    }
    #[test]
    fn relish_glob_splash_falls_off_and_spares_the_shooter() {
        let mut s = Feast::new(false);
        s.stage = Stage::Wave;
        s.players[0].p = [0., FLOOR, 2.4];
        s.players[0].owned[4] = true;
        s.players[0].ammo[4] = 1;
        for (i, x) in [0., 0.12, 0.5].into_iter().enumerate() {
            s.enemies.push(Zombie {
                id: i as u32 + 1,
                p: [x, FLOOR, 2.9],
                yaw: 0.,
                hp: 70.,
                born: 0.,
                forming: 0.,
            });
        }
        assert!(s.fire(1, Vec3::Z, 4));
        land(&mut s);
        assert_eq!(s.enemies.iter().map(|e| e.id).collect::<Vec<_>>(), [2, 3]);
        assert!(s.enemies[0].hp < 70. && s.enemies[0].hp > 0.);
        assert_eq!(s.enemies[1].hp, 70.);
        assert_eq!(s.players[0].hp, 100.);
        assert_eq!(s.wave_kills, 1);
        assert_eq!(s.players[0].kills, 1);
    }
    #[test]
    fn cover_shields_hotdogs_from_a_burst() {
        let mut s = Feast::new(false);
        s.begin_famine();
        s.players[1].p = [0.46, FLOOR, 2.655];
        s.players[2].p = [0.46, FLOOR, 2.225];
        let glob = |owner| Projectile {
            owner,
            weapon: 4,
            hostile: true,
            ..Default::default()
        };
        s.burst(&glob(1), Vec3::new(0.46, FLOOR + 0.105, 2.44), None, None);
        assert_eq!(s.players[1].hp, 100.);
        assert!(s.players[2].hp < 100.);
        let before = s.players[2].hp;
        s.burst(&glob(3), Vec3::new(0.46, FLOOR + 0.105, 2.3), None, None);
        assert_eq!(s.players[2].hp, before, "a glob never hurts its own shooter");
    }
    #[test]
    fn lead_meets_a_moving_target() {
        let (o, c, v) = (Vec3::new(0., 1., 0.), Vec3::new(1., 1., 0.), Vec3::Z * 0.5);
        let mark = lead(o, c, v, 1.5);
        let time = mark.distance(o) / 1.5;
        assert!(mark.z > 0.3);
        assert!((c + v * time).distance(mark) < 0.002);
    }
    #[test]
    fn a_hotdog_carries_only_two_guns() {
        let mut s = Feast::new(false);
        s.players[0].p = [-0.70, FLOOR, 2.45];
        assert!(s.pickup(1, 0));
        assert_eq!(s.players[0].weapon, 2);
        s.players[0].p = [0.06, FLOOR, 3.40];
        assert!(s.pickup(1, 1));
        assert_eq!(s.players[0].owned, [false, true, false, true, false, false, false, false]);
        assert_eq!(s.players[0].ammo[2], 0);
        s.select(1, 2);
        assert_eq!(s.players[0].weapon, 3);
        s.select(1, 1);
        s.players[0].p = [1.60, FLOOR, 2.38];
        assert!(s.pickup(1, 2));
        assert_eq!(s.players[0].owned, [false, false, false, true, true, false, false, false]);
        s.select(1, 3);
        s.players[0].p = [-0.30, FLOOR, 3.25];
        assert!(s.pickup(1, 6));
        assert_eq!(s.players[0].owned, [false, false, false, false, true, true, false, false]);
        assert_eq!(s.players[0].ammo[5], 40);
    }
    /// Pickups are spawn points (see FULL): a counter with a hot sauce thrower (the map says 70 rounds) and a box of
    /// ammunition on it, everybody but hotdog 1 parked in a corner, no bots; `respawn` is the map's setting, if any.
    fn shop(respawn: &str) -> Feast {
        let map = Map::parse(&format!(
            r#"{{"format": "hotdog-map", "version": 1, "name": "Shop", "bounds": [0, 0, 2, 1], {respawn}
            "starts": [{{"p": [0.2, 0.2], "yaw": 1.5}}, {{"p": [1.7, 0.8], "yaw": -2}}], "spawns": [{{"p": [0.1, 0.9]}}],
            "pickups": [{{"p": [1, 0.2], "weapon": 3, "ammo": 70}}, {{"p": [0.5, 0.5], "weapon": 0}}]}}"#
        ))
        .unwrap();
        let mut s = Feast::with_map(map, false);
        for p in &mut s.players {
            p.bot = false;
            if p.id != 1 {
                p.p = [1.9, FLOOR, 0.9];
            }
        }
        s.warmup = f32::INFINITY;
        s
    }
    fn pass(s: &mut Feast, seconds: f32) {
        for _ in 0..(seconds * 30.).round() as u32 {
            s.tick(1. / 30.);
        }
    }
    #[test]
    fn a_pickup_is_a_spawn_point_taken_it_comes_back_where_it_was_20_s_later_a_box_of_ammunition_12_s_whatever_the_map_says_and_not_before() {
        let mut s = shop("");
        let place: Vec<[f32; 3]> = s.pickups.iter().map(|i| i.p).collect();
        // (The rules' rounds, a magazine: the map's 70 are passed over.)
        assert_eq!(s.pickups.iter().map(|i| (i.weapon, i.ammo, i.ready, i.respawn)).collect::<Vec<_>>(), [(3, 90, 0., 20.), (crate::map::AMMO_BOX, 0, 0., 12.)]);
        s.players[0].p = [1., FLOOR, 0.2];
        assert!(s.pickup(1, 0));
        assert_eq!((s.players[0].owned[3], s.players[0].ammo[3]), (true, 90));
        // (Off the pad: holding the sniper now, it would take the next one's rounds by walking over it.)
        s.players[0].p = [0.2, FLOOR, 0.2];
        let taken = s.elapsed;
        assert_eq!(s.pickups[0].ready, taken + 20.);
        // Gone for everybody until then.
        s.players[1].p = [1., FLOOR, 0.2];
        assert!(!s.pickup(2, 0));
        pass(&mut s, 19.9);
        s.players[1].p = [1., FLOOR, 0.2];
        assert!(!s.pickup(2, 0), "not before its time");
        pass(&mut s, 0.2);
        assert!(s.pickup(2, 0), "and then where it was");
        assert_eq!(s.pickups[0].p, place[0]);
        // A box of ammunition comes back after 12 s; what a map says is passed over (the rules' times are every map's).
        s.players[0].p = [0.5, FLOOR, 0.5];
        (s.players[0].ammo[1], s.players[0].reserve[1]) = (2, 0);
        assert!(s.pickup(1, 1));
        assert_eq!(s.pickups[1].ready, s.elapsed + 12.);
        let t = shop(r#""respawn": [5, 3],"#);
        assert_eq!(t.level.map.respawn, [5., 3.], "read");
        assert_eq!(t.pickups.iter().map(|i| i.respawn).collect::<Vec<_>>(), [20., 12.], "and passed over");
    }
    #[test]
    fn a_box_of_ammunition_tops_up_the_reserves_of_the_guns_a_hotdog_holds_and_is_left_where_it_is_for_a_hotdog_whose_reserves_are_full() {
        use crate::magazine::CARRY;
        let mut s = shop("");
        assert_eq!(FULL, [0, 12, 6, 90, 4, 40, 60, 150]);
        assert_eq!(CARRY, [0, 48, 18, 180, 8, 80, 180, 300]);
        // Hotdog 1 has the guns it starts with, full magazines and their spare rounds; with the rifle down to 20 and 100
        // in reserve and the pistol's reserve gone both reserves are filled, the magazines are as they were, and nothing
        // else is given.
        let p = &s.players[0];
        assert_eq!((p.owned, p.ammo[1], p.ammo[RIFLE], p.reserve[1], p.reserve[RIFLE]), ([false, true, false, false, false, false, true, false], 12, 60, 48, 180));
        (s.players[0].ammo[RIFLE], s.players[0].reserve[RIFLE], s.players[0].reserve[1], s.players[0].p) = (20, 100, 0, [0.5, FLOOR, 0.5]);
        let hand = s.players[0].weapon;
        assert!(s.pickup(1, 1));
        let p = &s.players[0];
        assert_eq!((p.ammo[1], p.ammo[RIFLE], p.reserve[1], p.reserve[RIFLE], p.owned.iter().filter(|held| **held).count(), p.weapon), (12, 20, 48, 180, 2, hand));
        // A hotdog whose reserves are full (a full pistol's 48 and a full rifle's 180) takes nothing, however empty its
        // magazines, and the box is still there.
        pass(&mut s, 12.1);
        let q = &mut s.players[1];
        (q.owned[RIFLE], q.ammo[RIFLE], q.reserve[RIFLE], q.ammo[1], q.reserve[1], q.p) = (true, 0, 180, 0, 48, [0.5, FLOOR, 0.5]);
        let ready = s.pickups[1].ready;
        assert!(!s.pickup(2, 1));
        assert_eq!((s.players[1].reserve[1], s.players[1].reserve[RIFLE], s.pickups[1].ready), (48, 180, ready));
        // One gun short is enough: the rifle's reserve below what it carries is topped up.
        s.players[1].reserve[RIFLE] = 7;
        assert!(s.pickup(2, 1));
        assert_eq!((s.players[1].reserve[1], s.players[1].reserve[RIFLE]), (48, 180));
    }
    #[test]
    fn bots_know_pickups_come_back_a_bot_low_on_rounds_fetches_a_box_of_ammunition_waiting_on_the_pad_for_one_that_has_been_taken() {
        let mut s = shop("");
        let bot = &mut s.players[1];
        // (It holds the hot sauce thrower too, full, so that the shop's thrower is nothing to it.)
        (bot.bot, bot.p, bot.ammo[1], bot.reserve[1], bot.owned[3], bot.ammo[3]) = (true, [0.5, FLOOR, 0.9], 2, 0, true, 90);
        // The box has just been taken: due back in 12 s, too long to set out for yet.
        (s.players[0].p, s.players[0].ammo[1], s.players[0].reserve[1]) = ([0.5, FLOOR, 0.5], 0, 0);
        assert!(s.pickup(1, 1));
        s.players[0].p = [0.1, FLOOR, 0.1];
        let back = s.pickups[1].ready;
        let mut set = None;
        for _ in 0..30 * 9 {
            s.tick(1. / 30.);
            if s.players[1].errand == Some(1) && set.is_none() {
                set = Some(s.elapsed);
            }
        }
        assert_eq!(s.players[1].ammo[1], 2);
        assert!(set.is_none(), "it set out at {set:?}, 12 s before the box was back");
        // Nearer the time it sets out, waits on the pad if it is early, and takes the box once it is back.
        let mut took = None;
        for _ in 0..30 * 6 {
            s.tick(1. / 30.);
            if s.players[1].reserve[1] == crate::magazine::CARRY[1] {
                took = Some(s.elapsed);
                break;
            }
        }
        let took = took.expect("it took the box");
        assert!(took >= back && took < back + 1.5, "it took the box at {took}, back at {back}");
        assert!((s.players[1].p[0] - 0.5).hypot(s.players[1].p[2] - 0.5) < 0.13);
    }
    #[test]
    fn a_napkin_is_45_cm_square_and_90_g_light_enough_to_carry_at_full_pace_and_to_throw_far_held_clear_of_its_carrier() {
        let mut s = Feast::with_map(small(r#"{"kind": "napkin", "p": [0.4, 0.5], "base": 0, "yaw": 0}"#), false);
        let shape = s.props.shape(s.props.body(0).shape).clone();
        let [x0, _, z0, x1, _, z1] = shape.aabb;
        assert!(((x1 - x0) - 0.45).abs() < 1e-9 && ((z1 - z0) - 0.45).abs() < 1e-9, "{} by {}", x1 - x0, z1 - z0);
        assert!((shape.mass - 0.09).abs() < 1e-9 && shape.mass < HEAVY);
        assert!(OUT[3] - (z1 - z0) / 2. >= HOTDOG_RADIUS, "held clear of its carrier");
        for p in &mut s.players {
            p.bot = false;
        }
        s.players[0].p = [0.4, FLOOR, 0.75];
        s.players[0].yaw = 0.;
        s.tick(1. / 30.);
        assert!(s.grab(1, Vec3::new(0., -0.8, -0.6)));
        assert_eq!((s.players[0].carry, s.burden(1)), (0, 1.));
        s.players[0].yaw = -std::f32::consts::FRAC_PI_2;
        assert!(s.toss(1, Vec3::X));
        assert!((s.props.velocity(0)[0] - THROW[0]).abs() < 1e-6, "thrown as fast as a pillow");
    }
    #[test]
    fn camping_summons_pests_and_leaving_disperses_them() {
        let mut s = Feast::new(false);
        s.begin_famine();
        for p in &mut s.players {
            p.bot = false;
        }
        assert_eq!([SWARM[0] * 2., SWARM[1] * 2.], [0.51, 0.45]);
        s.tick(LINGER - 0.2);
        assert!(s.pests.is_empty());
        s.tick(0.4);
        let swarm = s.pests.iter().find(|x| infested(x, s.players[0].p)).cloned().unwrap();
        assert!(infested(&swarm, [swarm.p[0] + 0.25, FLOOR, swarm.p[2]]));
        assert!(!infested(&swarm, [swarm.p[0] + 0.26, FLOOR, swarm.p[2]]));
        let before = s.players[0].hp;
        s.tick(1.);
        assert!((before - s.players[0].hp - 8.2).abs() < 0.05, "pests sting on top of hunger");
        s.players[0].p = [-1.2, FLOOR, 2.6];
        let before = s.players[0].hp;
        s.tick(CALM + 0.2);
        assert!(!s.pests.iter().any(|x| x.id == swarm.id));
        assert!((before - s.players[0].hp - 2.2 * (CALM + 0.2)).abs() < 0.05);
    }
    #[test]
    fn bug_spray_clears_a_swarm_and_passes_through_hotdogs() {
        let mut s = duel();
        s.players[0].p = [0., FLOOR, 2.45];
        s.players[1].p = [0., FLOOR, 2.56];
        s.players[0].owned[5] = true;
        s.players[0].ammo[5] = 10;
        s.pests.push(Swarm {
            id: 9,
            p: [0., FLOOR, 2.9],
            hp: 100.,
            ..Default::default()
        });
        for _ in 0..4 {
            s.players[0].cooldown = 0.;
            assert!(s.fire(1, Vec3::Z, 5));
            land(&mut s);
        }
        assert_eq!(s.pests[0].hp, 20.);
        s.players[0].cooldown = 0.;
        s.fire(1, Vec3::Z, 5);
        land(&mut s);
        assert!(s.pests.is_empty());
        assert_eq!(s.players[0].score, 50);
        assert_eq!(s.players[1].hp, 100.);
        assert_eq!(s.friendly_fire_events, 0);
    }
    #[test]
    fn stim_doubles_damage_from_rivals_only_and_has_a_cooldown() {
        let mut s = duel();
        assert!(s.stim(2));
        assert_eq!(s.players[1].stim, STIM);
        assert!(!s.stim(2));
        s.fire(1, Vec3::Z, 1);
        land(&mut s);
        assert_eq!(s.players[1].hp, 64.);
        s.hurt(2, 10.);
        assert_eq!(s.players[1].hp, 54.);
        for p in &mut s.players {
            p.bot = false;
        }
        s.tick(STIM + 0.1);
        assert_eq!(s.players[1].stim, 0.);
        assert!(s.players[1].stim_wait > 0.);
        assert!(!s.stim(2));
    }
    #[test]
    fn prone_lowers_the_profile_and_the_muzzle() {
        let mut s = duel();
        s.set_prone(2, true);
        s.fire(1, Vec3::Z, 1);
        land(&mut s);
        assert_eq!(s.players[1].hp, 100.);
        s.set_prone(1, true);
        s.players[0].cooldown = 0.;
        s.fire(1, Vec3::Z, 1);
        assert!((s.projectiles[0].p[1] - (FLOOR + LOW)).abs() < 1e-6);
    }
    #[test]
    fn demo_betrayal_is_a_real_burst_of_syrup_that_lands() {
        let mut s = Feast::new(true);
        for _ in 0..1800 {
            s.tick(1. / 30.);
            if s.demo_shot {
                break;
            }
        }
        assert!(s.demo_shot);
        // FRANKIE traded its mustard pistol for a gun in the warm-up, so it fires the syrup rifle it started with.
        assert!(!s.players[1].owned[1] && s.players[1].owned[RIFLE]);
        let round = s.projectiles.last().cloned().unwrap();
        assert_eq!((round.owner, round.weapon, round.hostile), (2, RIFLE, true));
        assert_eq!(s.players[1].volley, VOLLEY - 1, "the first round of a burst");
        let before = s.players[2].hp;
        for _ in 0..120 {
            if !s.projectiles.iter().any(|q| q.id == round.id) {
                break;
            }
            s.tick(1. / 30.);
        }
        assert!(s.friendly_fire_events > 0);
        assert!(before - s.players[2].hp >= PLAYER_DAMAGE[RIFLE], "MUSTARD was hit by the syrup");
    }
    #[test]
    fn demo_produces_one_last_survivor() {
        let mut s = Feast::new(true);
        for _ in 0..3600 {
            s.tick(1. / 30.);
            if s.stage == Stage::RoundOver {
                break;
            }
        }
        assert_eq!(s.stage, Stage::RoundOver);
        assert_eq!(s.players.iter().filter(|p| p.alive).count(), 1);
        assert!(s.winner.is_some());
    }
    #[test]
    fn one_wave_then_famine() {
        let mut s = Feast::new(true);
        for _ in 0..1800 {
            s.tick(1. / 30.);
            if s.stage == Stage::FreeForAll {
                break;
            }
        }
        assert_eq!(s.wave_spawned, 6);
        assert_eq!(s.wave_kills, 6);
        assert_eq!(s.stage, Stage::FreeForAll);
        assert!(s.alert.contains("ONLY HOTDOGS"));
    }
    #[test]
    fn hunger_and_eating_transfer_health() {
        let mut s = Feast::new(false);
        s.begin_famine();
        s.players[0].hp = 40.;
        s.players[1].p = s.players[0].p;
        let before = s.players[1].hp;
        // The living cannot be eaten, only bitten; the bite is on a cooldown.
        assert!(!s.eat(1));
        assert_eq!((s.players[0].hp, s.players[1].hp), (40., before));
        assert!(s.bite(1));
        assert!(s.players[0].hp > 40.);
        assert!(s.players[1].hp < before);
        assert!(!s.bite(1));
    }
    #[test]
    fn only_the_dead_can_be_eaten_and_only_the_living_bitten() {
        let mut s = Feast::new(false);
        s.begin_famine();
        s.players[0].hp = 20.;
        s.players[1].p = s.players[0].p;
        // Alive: E does nothing, and nothing is consumed.
        assert!(!s.eat(1));
        assert!(!s.players[1].consumed && s.players[1].alive && s.players[0].meals == 0);
        // Dead: the bite has nothing to attack, and eating consumes the body once.
        s.hurt(2, 100.);
        assert!(!s.players[1].alive);
        assert!(!s.bite(1));
        assert_eq!(s.players[0].hp, 20.);
        assert!(s.eat(1));
        assert!(s.players[1].consumed);
        s.players[0].bite_cooldown = 0.;
        assert!(!s.eat(1), "a body is food only once");
        // With a body and a living rival both in reach, each key takes its own.
        s.players[2].p = s.players[0].p;
        s.players[3].p = s.players[0].p;
        s.hurt(4, 100.);
        assert!(s.bite(1));
        assert!(s.players[2].hp < 100. && !s.players[3].consumed);
        s.players[0].bite_cooldown = 0.;
        assert!(s.eat(1));
        assert!(s.players[3].consumed);
    }
    #[test]
    fn corpses_are_food_only_once() {
        let mut s = Feast::new(false);
        s.begin_famine();
        s.players[0].hp = 20.;
        s.players[1].p = s.players[0].p;
        s.hurt(2, 100.);
        assert!(s.eat(1));
        assert!(s.players[1].consumed);
        assert_eq!(s.players[0].hp, 58.);
    }
    #[test]
    fn both_ramps_reach_deck_without_jumping() {
        let level = Feast::new(false).level;
        for route in [((0.55, 3.53), (1.40, 3.53)), ((1.65, 2.46), (1.65, 3.30))] {
            let mut p = Vec3::new(route.0.0, FLOOR, route.0.1);
            for i in 1..=100 {
                let t = i as f32 / 100.;
                let q = Vec3::new(
                    route.0.0 + (route.1.0 - route.0.0) * t,
                    p.y,
                    route.0.1 + (route.1.1 - route.0.1) * t,
                );
                assert!(level.can_step_to(p, q));
                p = Vec3::new(q.x, level.ground_at(q.x, q.z), q.z);
            }
            assert!((p.y - (FLOOR + 0.28)).abs() < 0.0001);
        }
    }
    #[test]
    fn cannot_snap_up_side_of_turret() {
        assert!(!Feast::new(false).level.can_step_to(
            Vec3::new(1.2, FLOOR, 3.3),
            Vec3::new(1.26, FLOOR, 3.3)
        ));
    }
    // Solid bodies (actor.rs): hotdogs and zombies cannot overlap or pass through each other, blasts throw
    // them, and a puddle is not a body.

    /// The smallest gap between the surfaces of any two bodies, in metres; negative would be an overlap.
    fn gap(s: &Feast) -> f64 {
        let all = &s.bodies.actors;
        let mut least = f64::INFINITY;
        for a in all {
            for b in all.iter().filter(|b| a.id < b.id) {
                least = least.min((a.p[0] - b.p[0]).hypot(a.p[2] - b.p[2]) - a.radius - b.radius);
            }
        }
        least
    }
    fn cat(id: u32, x: f32, z: f32) -> Zombie {
        Zombie { id, p: [x, FLOOR, z], yaw: 0., hp: 1e6, born: 0., forming: 0. }
    }
    /// A wave that is already complete on open counter: hotdog 1 at the origin of the action, the others
    /// parked, nobody a bot, and the given zombies formed.
    fn arena(zombies: &[[f32; 2]]) -> Feast {
        let mut s = Feast::new(false);
        for (i, p) in s.players.iter_mut().enumerate() {
            (p.bot, p.hp, p.p) = (false, 1e6, [-1.5 + i as f32 * 0.1, FLOOR, 3.55]);
        }
        (s.stage, s.wave_spawned) = (Stage::Wave, WAVE);
        s.players[0].p = [0., FLOOR, 2.4];
        s.enemies = zombies.iter().enumerate().map(|(i, at)| cat(i as u32 + 1, at[0], at[1])).collect();
        s
    }
    const TICK: f32 = 1. / 30.;
    #[test]
    fn two_hotdogs_cannot_overlap_they_stop_on_contact_and_one_walking_into_another_pushes_it() {
        // (Positions that single precision holds exactly: dead-centre contact is a balance any rounding tips,
        // after which round bodies slip past each other.)
        let mut s = arena(&[]);
        s.players[0].p = [0., FLOOR, 2.375];
        s.players[1].p = [0.5, FLOOR, 2.375];
        // Steered straight at each other at 0.67 m/s, they meet in the middle and stand touching.
        for _ in 0..60 {
            s.embody();
            s.steer(1, [1., 0.9025, 2.375], 0.67);
            s.steer(2, [-1., 0.9025, 2.375], 0.67);
            s.tick(TICK);
            assert!(gap(&s) >= -1e-9, "overlap of {}", -gap(&s));
        }
        let (a, b) = (s.players[0].clone(), s.players[1].clone());
        assert!((b.p[0] - a.p[0] - 0.09).abs() < 1e-6 && (a.p[0] + b.p[0] - 0.5).abs() < 0.03 && a.p[0] < b.p[0], "{:?} {:?}", a.p, b.p);
        assert!(a.v[0].abs() < 1e-6 && a.grounded && !a.launched);
        // A person's hotdog walks (`walk`, which single player, the client's prediction and the server's commands
        // use): walking into the idle one moves it out of the way rather than through it, sharing the push by mass.
        for _ in 0..30 {
            s.walk(1, Vec2::new(0.012, 0.), 0.);
            assert!(gap(&s) >= -1e-9);
        }
        assert!((s.players[0].p[0] - (a.p[0] + 0.18)).abs() < 1e-5 && (s.players[1].p[0] - s.players[0].p[0] - 0.09).abs() < 1e-6, "{:?} {:?}", s.players[0].p, s.players[1].p);
        let before = s.players[0].p[0];
        let at = s.walk(1, Vec2::new(0.02, 0.), 0.5).unwrap();
        assert!((at.x - before - 0.01).abs() < 1e-6 && (s.players[1].p[0] - at.x - 0.09).abs() < 1e-6 && s.players[0].yaw == 0.5);
        // Five hotdogs put on one spot are parted.
        for p in &mut s.players {
            p.p = [0., FLOOR, 2.375];
        }
        s.tick(20. * TICK);
        assert!(gap(&s) >= -1e-9, "still overlapping by {}", -gap(&s));
        assert!(s.players.iter().all(|p| p.p[0].hypot(p.p[2] - 2.375) < 0.4));
    }
    #[test]
    fn zombies_cannot_walk_through_a_hotdog_or_each_other_and_a_ring_of_them_can_still_bite() {
        let mut s = arena(&[[-0.5, 2.4], [-0.6, 2.4], [-0.7, 2.4], [0.5, 2.42], [0.6, 2.42], [0.7, 2.42]]);
        let mut least = f64::INFINITY;
        for _ in 0..270 {
            s.tick(TICK);
            least = least.min(gap(&s));
        }
        assert!(least >= -1e-9, "bodies overlapped by {}", -least);
        assert!(BITE > STAND_OFF && STAND_OFF as f64 > HOTDOG_RADIUS + actor::RADIUS);
        // Each took a place in the ring round the hotdog. All six are in reach, none is inside the hotdog,
        // and it was not carried off.
        let hotdog = s.players[0].clone();
        for e in &s.enemies {
            let d = (e.p[0] - hotdog.p[0]).hypot(e.p[2] - hotdog.p[2]);
            assert!(d < BITE && d >= 0.075 - 1e-6, "zombie {} is {d} m from the hotdog", e.id);
        }
        assert!(hotdog.p[0].hypot(hotdog.p[2] - 2.4) < 0.05, "the hotdog was pushed to {:?}", hotdog.p);
        s.players[0].hp = 100.;
        s.tick(TICK);
        assert!((100. - s.players[0].hp - 6. * 14. / 30.).abs() < 1e-3, "all six bite");
        // The hotdog cannot walk through a cat either: it pushes the cat ahead of it.
        let mut s = arena(&[[0.1, 2.4]]);
        for _ in 0..20 {
            s.walk(1, Vec2::new(0.02, 0.), 0.);
            s.tick(TICK);
            assert!(gap(&s) >= -1e-9 && s.players[0].p[0] < s.enemies[0].p[0]);
        }
        assert!(s.enemies[0].p[0] > 0.3);
    }
    #[test]
    fn a_blast_throws_bodies_outward_and_they_come_to_rest_hits_nudge_and_the_shooter_is_spared() {
        let mut s = arena(&[[0.1, 2.9], [-0.16, 2.9], [0.6, 2.9]]);
        s.players[1].p = [0., FLOOR, 3.02];
        s.begin_famine();
        s.players[0].p = [0.02, FLOOR, 2.82];
        let at: Vec<[f32; 3]> = s.enemies.iter().map(|e| e.p).collect();
        let glob = |owner, hostile| Projectile { owner, weapon: 4, hostile, ..Default::default() };
        s.burst(&glob(1, true), Vec3::new(0., FLOOR + 0.03, 2.9), None, None);
        let thrown = |s: &Feast, id: u32| s.body(id).unwrap().launched;
        assert_eq!([thrown(&s, CAT + 1), thrown(&s, CAT + 2), thrown(&s, CAT + 3), thrown(&s, 2), thrown(&s, 1)], [true, true, false, true, false]);
        let (mut rested, mut flew) = (0, false);
        for i in 1..=120 {
            s.tick(TICK);
            flew |= s.enemies[0].p[1] > FLOOR + 0.005;
            assert!(gap(&s) >= -1e-9);
            if !s.bodies.actors.iter().any(|body| body.launched) {
                rested = i;
                break;
            }
        }
        assert!(flew && rested > 5 && rested < 60, "came to rest after {rested} steps");
        let moved = [s.enemies[0].p[0] - at[0][0], at[1][0] - s.enemies[1].p[0], s.players[1].p[2] - 3.02];
        assert!(moved[0] > 0.08 && moved[0] < 0.6 && moved[1] > 0.02 && moved[1] < moved[0] && moved[2] > 0.02 && moved[2] < 0.5, "thrown {moved:?}");
        assert_eq!(s.enemies[2].p, at[2]);
        assert!((s.players[0].p[0] - 0.02).hypot(s.players[0].p[2] - 2.82) < 1e-6, "the shooter stood its ground");
        let landed = &s.players[1];
        assert!(landed.grounded && !landed.launched && !landed.fallen && landed.p[1] == FLOOR);
        // A roster from a server that does not send the new fields still stands on the ground.
        let old: Player = serde_json::from_str(r#"{"id": 3, "p": [1, 2, 3], "alive": true}"#).unwrap();
        assert!(old.grounded && !old.launched && !old.fallen && old.v == [0.; 3]);
        // During the wave rounds do not hurt hotdogs, and their bursts do not throw them; zombies are thrown all the same.
        let mut w = arena(&[[0.1, 2.9]]);
        w.players[1].p = [-0.1, FLOOR, 2.9];
        w.burst(&glob(1, false), Vec3::new(0., FLOOR + 0.03, 2.9), None, None);
        assert!(!thrown(&w, 2) && thrown(&w, CAT + 1));
        // A mustard round shoves the zombie it hits along its flight, a little.
        let mut m = arena(&[[0., 2.7]]);
        m.players[0].ammo[1] = 9;
        assert!(m.fire(1, Vec3::Z, 1));
        for _ in 0..20 {
            if !m.splats.is_empty() {
                break;
            }
            m.tick(TICK);
        }
        let body = m.body(CAT + 1).unwrap();
        assert!(m.enemies[0].hp < 1e6 && body.launched && body.knock[1] > 0.3 && body.knock[1] < 0.47 && body.knock[0] == 0. && body.grounded, "knocked at {:?}", body.knock);
        m.tick(10. * TICK);
        assert!(!m.body(CAT + 1).unwrap().launched, "and it is over in a moment");
    }
    #[test]
    fn a_character_pinned_against_a_block_is_not_pushed_into_it() {
        let (mut s, wall) = (arena(&[]), -0.45 - 0.03);
        s.players[1].p = [wall - 1e-5, FLOOR, 2.8];
        s.players[0].p = [wall - 0.2, FLOOR, 2.8];
        let held = s.players[1].p;
        let stays = |s: &Feast| {
            let p = s.players[1].p;
            s.level.walkable(p[0] as f64, p[1] as f64, p[2] as f64) && gap(s) >= -1e-9 && (p[0] - held[0]).hypot(p[2] - held[2]) < 2e-5
        };
        // A hotdog walks at it, then three zombies press on it from the same side: it never enters the block's
        // margin and nobody overlaps.
        for _ in 0..30 {
            s.walk(1, Vec2::new(0.02, 0.), 0.);
            s.tick(TICK);
            assert!(stays(&s), "pushed to {:?}", s.players[1].p);
        }
        // (The walker steps round it, up to the block's margin and no further.)
        assert!(s.players[0].p[0] <= wall + 1e-6, "{:?}", s.players[0].p);
        s.players[0].p = [1., FLOOR, 2.4];
        s.enemies = vec![cat(1, wall - 0.2, 2.8), cat(2, wall - 0.3, 2.78), cat(3, wall - 0.4, 2.82)];
        for _ in 0..300 {
            s.tick(TICK);
            assert!(stays(&s), "pushed to {:?}", s.players[1].p);
        }
    }
    #[test]
    fn a_forming_zombie_does_not_block_and_is_not_pushed_and_once_formed_it_takes_its_place() {
        let mut s = arena(&[]);
        s.enemies.push(Zombie { forming: 1., hp: 70., ..cat(1, 0.2, 2.4) });
        // The hotdog walks straight across the puddle.
        for _ in 0..20 {
            s.walk(1, Vec2::new(0.02, 0.), 0.);
            s.tick(TICK);
        }
        assert!((s.players[0].p[0] - 0.4).abs() < 1e-5 && s.enemies[0].forming > 0.);
        assert!(s.enemies[0].p == [0.2, FLOOR, 2.4] && s.body(CAT + 1).is_none());
        // Standing on it when it finishes: the two are pushed apart, the lighter zombie further, and from
        // then on it is solid.
        s.players[0].p = [0.21, FLOOR, 2.4];
        s.tick(12. * TICK);
        assert!(s.enemies[0].forming == 0. && s.body(CAT + 1).is_some() && gap(&s) >= -1e-9);
        let (hotdog, zombie) = (s.players[0].p[0], s.enemies[0].p[0]);
        assert!(hotdog > 0.21 && zombie < 0.2 && 0.2 - zombie > hotdog - 0.21, "parted to {hotdog} and {zombie}");
        // A person's hotdog walks at once and jumps at the next step, at any frame rate.
        let mut s = arena(&[]);
        let (mut top, mut frames) = (0f32, 0);
        s.jump(1, actor::JUMP as f32);
        for _ in 0..120 {
            s.walk(1, Vec2::new(actor::WALK as f32 / 144., 0.), 0.);
            s.tick(1. / 144.);
            top = top.max(s.players[0].p[1] - FLOOR);
            frames += !s.players[0].grounded as u32;
        }
        assert!((top - 0.1143).abs() < 1e-3 && (frames as f32 / 144. - 0.571).abs() < 0.02, "apex {top}, {frames} frames in the air");
        assert!((s.players[0].p[0] - 120. * actor::WALK as f32 / 144.).abs() < 1e-4 && s.players[0].grounded);
        // The counter edge is one setting, and a new round keeps it.
        assert!(!s.bodies.config.open_edge);
        s.bodies.config.open_edge = true;
        s.reset();
        assert!(s.bodies.config.open_edge && s.bodies.actors.is_empty());
    }
    // Maps from the editor. The rules of a map are tested in map.rs; these cover what the simulation takes from one.
    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    fn read(path: &str) -> String {
        std::fs::read_to_string(format!("{ROOT}/{path}")).unwrap_or_else(|error| panic!("{path}: {error}"))
    }
    fn busier() -> Map {
        Map::parse(&read("assets/maps/counter-siege-v2.json")).unwrap()
    }
    /// A small map with two starts (one on a deck), one pickup, one block and the given props.
    fn small(props: &str) -> Map {
        Map::parse(&format!(
            r#"{{"format": "hotdog-map", "version": 1, "name": "Two starts", "bounds": [0, 0, 2, 1],
            "starts": [{{"p": [0.2, 0.2], "yaw": 1.5}}, {{"p": [1.7, 0.8], "yaw": -2}}], "spawns": [{{"p": [0.1, 0.9]}}],
            "pickups": [{{"p": [1, 0.2], "weapon": 5, "ammo": 7}}], "blocks": [{{"rect": [0.9, 0.4, 1.1, 0.6], "height": 0.3}}],
            "decks": [{{"rect": [1.5, 0.6, 2, 1], "height": 0.2}}], "props": [{props}]}}"#
        ))
        .unwrap()
    }
    #[test]
    fn a_map_from_the_editor_sets_the_starts_pickups_and_cover() {
        let map = busier();
        let s = Feast::with_map(map.clone(), false);
        assert_eq!(s.players.len(), 5);
        for (p, start) in s.players.iter().zip(&map.starts) {
            assert_eq!([p.p[0], p.p[1], p.p[2], p.yaw], [start.p[0] as f32, FLOOR, start.p[1] as f32, start.yaw as f32]);
        }
        assert_eq!(s.pickups.len(), map.pickups.len());
        for (item, pickup) in s.pickups.iter().zip(&map.pickups) {
            // (What it gives is the rules', whatever the map says.)
            assert_eq!((item.p[0], item.p[2], item.weapon, item.ammo), (pickup.p[0] as f32, pickup.p[1] as f32, pickup.weapon, crate::magazine::stock(pickup.weapon).0));
        }
        // A block that only the busier map has, between z = 2.66 and 2.86: it stops a hotdog walking
        // into it and a round fired at it.
        for (map, blocked) in [(Map::bundled(), false), (busier(), true)] {
            let mut s = Feast::with_map(map, false);
            s.stage = Stage::Wave;
            s.players[0].p = [-0.01, FLOOR, 2.56];
            for _ in 0..4 {
                s.walk(1, Vec2::new(0., 0.04), 0.);
            }
            assert_eq!(s.players[0].p[2] > 2.7, !blocked);
            assert_eq!(s.players[0].p[2] < 2.66 - 0.029, blocked);
            s.players[0].p = [-0.01, FLOOR, 2.4];
            assert!(s.fire(1, Vec3::Z, 1));
            land(&mut s);
            assert_eq!((s.splats[0].p[2] - 2.66).abs() < 1e-5, blocked);
        }
        // The map's one turret faces -x from the deck; the bundled map's faces -z.
        assert_eq!(s.turrets.len(), 1);
        let (origin, direction) = s.turrets[0];
        assert!(direction.x < -0.9 && direction.z.abs() < 1e-4 && (origin.x - 1.22).abs() < 1e-4);
        let bundled = Feast::new(false);
        assert_eq!(bundled.turrets[0].0, Vec3::new(1.47, 1.20, 3.10));
        assert!(bundled.turrets[0].1.distance((Vec3::new(1.47, FLOOR + 0.08, 2.40) - Vec3::new(1.47, 1.20, 3.10)).normalize()) < 1e-6);
    }
    #[test]
    fn five_hotdogs_share_a_map_with_fewer_starts_and_a_start_on_a_deck_stands_on_it() {
        let mut s = Feast::with_map(small(""), false);
        let (low, high) = ([0.2, FLOOR, 0.2], [1.7, FLOOR + 0.2, 0.8]);
        assert_eq!(s.players.iter().map(|p| p.p).collect::<Vec<_>>(), [low, high, low, high, low]);
        assert_eq!(s.players.iter().map(|p| p.yaw).collect::<Vec<_>>(), [1.5, -2., 1.5, -2., 1.5]);
        assert_eq!(s.pickups.len(), 1);
        assert_eq!((s.pickups[0].p, s.pickups[0].weapon, s.pickups[0].ammo), ([1., FLOOR + 0.04, 0.2], 5, 40), "a can of bug spray, whatever the map says");
        s.reset();
        assert_eq!((s.round, s.pickups.len(), s.level.map.name.as_str()), (2, 1, "Two starts"));
        assert_eq!(s.players[1].p, high);
    }
    #[test]
    fn every_turret_on_the_map_guards_it_and_a_map_without_one_has_no_beam() {
        let beams = |props: &str| {
            let mut s = Feast::with_map(small(props), false);
            for p in &mut s.players {
                p.bot = false;
            }
            // Nothing acts but the turrets: a zombie stands in front of each.
            s.warmup = f32::INFINITY;
            s.enemies.push(Zombie { id: 1, p: [0.5, FLOOR, 0.1], yaw: 0., hp: 1e9, born: 0., forming: 0. });
            s.enemies.push(Zombie { id: 2, p: [1.2, FLOOR, 0.3], yaw: 0., hp: 1e9, born: 0., forming: 0. });
            s.tick(0.3);
            (s.shots.iter().filter(|shot| shot.owner == 0).cloned().collect::<Vec<_>>(), s.turret_yaw.clone())
        };
        assert!(beams("").0.is_empty() && beams("").1.is_empty());
        let (two, yaws) = beams(r#"{"kind": "turret", "p": [0.5, 0.5], "base": 0.3, "yaw": 0}, {"kind": "turret", "p": [1.5, 0.3], "base": 0.1, "yaw": 1.5708}"#);
        assert_eq!(two.len(), 2);
        assert!(Vec3::from_array(two[0].a).distance(Vec3::new(0.5, FLOOR + 0.3, 0.38)) < 1e-6);
        assert!(Vec3::from_array(two[0].b).distance(Vec3::new(0.5, FLOOR + 0.075, 0.1)) < 1e-6, "the beam ends on the zombie");
        assert!((two[1].a[0] - 1.38).abs() < 1e-5 && (two[1].a[2] - 0.3).abs() < 1e-5 && two[1].b[0] < two[1].a[0] - 0.1);
        assert!(yaws[0].abs() < 1e-6 && (yaws[1] - 1.5708).abs() < 1e-5);
    }
    /// A firing range: one turret in the middle of an open counter, facing -z, with a wall 0.4 m to its right.
    /// The wave never comes, so nothing acts but the turret; the hotdogs are parked in a corner.
    fn range() -> Feast {
        let map = Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "name": "Range", "bounds": [0, 0, 3, 2], "starts": [{"p": [0.2, 1.8]}], "spawns": [{"p": [0.1, 0.1]}],
            "blocks": [{"rect": [1.9, 0.7, 2.0, 1.3], "height": 0.4}], "props": [{"kind": "turret", "p": [1.5, 1], "base": 0.1, "yaw": 0}]}"#,
        )
        .unwrap();
        let mut s = Feast::with_map(map, false);
        s.warmup = f32::INFINITY;
        for (i, p) in s.players.iter_mut().enumerate() {
            p.bot = false;
            p.p = [0.2 + 0.1 * i as f32, FLOOR, 1.8];
        }
        s
    }
    fn formed(id: u32, x: f32, z: f32) -> Zombie {
        Zombie { id, p: [x, FLOOR, z], yaw: 0., hp: 70., born: 0., forming: 0. }
    }
    /// Steps the range `ticks` times, checking that the turret never turns faster than it may.
    fn watch(s: &mut Feast, ticks: usize) {
        for _ in 0..ticks {
            let before = s.turret_yaw[0];
            s.tick(1. / 30.);
            assert!((s.turret_yaw[0] - before).abs() <= TURRET_TURN / 30. + 1e-5, "the turret snapped from {before} to {}", s.turret_yaw[0]);
        }
    }
    #[test]
    fn a_turret_turns_at_a_limited_rate_to_track_a_zombie_and_fires_once_it_points_at_it() {
        let mut s = range();
        // A zombie a quarter turn to its left, 0.6 m from the post.
        s.enemies.push(formed(1, 0.9, 1.));
        let bearing = std::f32::consts::FRAC_PI_2;
        watch(&mut s, 15);
        assert!((s.turret_yaw[0] - 1.).abs() < 1e-3, "half a second at {TURRET_TURN} rad/s: {}", s.turret_yaw[0]);
        assert_eq!((s.next_shot, s.enemies[0].hp), (1, 70.), "it does not fire until it points at its target");
        watch(&mut s, 24);
        assert!((s.turret_yaw[0] - bearing).abs() < 1e-4);
        // It pulses five times a second, from its muzzle to the zombie, 4 HP a pulse.
        let (fired, hp) = (s.next_shot - 1, s.enemies[0].hp);
        assert!(fired >= 2 && hp == 70. - 4. * fired as f32, "{fired} pulses, {hp} HP");
        let shot = s.shots.last().unwrap();
        assert_eq!((shot.owner, shot.weapon, shot.projectile), (0, 0, 0));
        assert!(Vec3::from_array(shot.a).distance(Vec3::new(1.38, FLOOR + 0.1, 1.)) < 1e-4 && Vec3::from_array(shot.b).distance(Vec3::new(0.9, FLOOR + 0.075, 1.)) < 1e-5);
        assert!(s.turrets[0].0.distance(Vec3::new(1.38, FLOOR + 0.1, 1.)) < 1e-4 && s.turrets[0].1.x < -0.99, "the beam points where the turret does");
        watch(&mut s, 60);
        assert_eq!((s.next_shot - 1 - fired, hp - s.enemies[0].hp), (10, 40.), "two seconds of fire");
        // The zombie walks round behind the turret, slower than it can turn: the turret follows it and keeps hitting.
        for i in 1..=90 {
            let angle = bearing + i as f32 * 0.02;
            s.enemies[0].p = [1.5 - angle.sin() * 0.6, FLOOR, 1. - angle.cos() * 0.6];
            s.enemies[0].hp = 70.;
            watch(&mut s, 1);
            assert!(wrap(s.turret_yaw[0] - angle).abs() < 0.03, "it lost its target at step {i}");
        }
        assert!(s.shots.len() >= 2 && s.shots.iter().all(|shot| shot.owner == 0));
        // It kills what it tracks: the kill is the wave's and nobody's score; then it goes back to its sweep.
        s.enemies[0].hp = 8.;
        watch(&mut s, 12);
        assert!(s.enemies.is_empty());
        assert_eq!((s.wave_kills, s.players.iter().map(|p| p.kills + p.score).sum::<u32>()), (1, 0));
        let shots = s.next_shot;
        watch(&mut s, 150);
        assert_eq!(s.next_shot, shots);
        assert!(wrap(s.turret_yaw[0]).abs() <= TURRET_SWEEP[0] + 1e-3, "back to within its sweep of the placed yaw: {}", s.turret_yaw[0]);
        // A new round puts it back where the map has it.
        s.reset();
        assert_eq!(s.turret_yaw, [0.]);
        assert!(s.turrets[0].0.distance(Vec3::new(1.5, FLOOR + 0.1, 0.88)) < 1e-6);
    }
    #[test]
    fn a_turret_never_hurts_or_knocks_a_hotdog_and_idles_through_the_free_for_all() {
        // Hotdogs stand, crouch and lie in the beam between the turret and the zombie it is shooting.
        let mut s = range();
        s.enemies.push(formed(1, 1.5, 0.2));
        s.players[0].p = [1.5, FLOOR, 0.7];
        s.players[1].p = [1.5, FLOOR, 0.55];
        s.players[2].p = [1.5, FLOOR, 0.4];
        (s.players[1].crouch, s.players[2].prone) = (true, true);
        let at: Vec<[f32; 3]> = s.players.iter().map(|p| p.p).collect();
        watch(&mut s, 90);
        assert!(s.enemies[0].hp <= 70. - 4. * 14., "the zombie beyond them is hit: {}", s.enemies[0].hp);
        for (p, at) in s.players.iter().zip(&at) {
            assert_eq!((p.hp, p.launched, p.p, p.v), (100., false, *at, [0.; 3]), "{} was touched", p.name);
            assert_eq!(p.last_damage, Damage::default(), "nothing is on {}'s record", p.name);
        }
        assert_eq!(s.friendly_fire_events, 0);
        // The free for all: no zombies, hotdogs all round it, one at its muzzle. It sweeps slowly and never fires.
        let mut s = range();
        s.begin_famine();
        s.players[0].p = [1.5, FLOOR, 0.8];
        s.players[1].p = [1.2, FLOOR, 0.6];
        s.players[2].p = [1.8, FLOOR, 0.6];
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for _ in 0..300 {
            let before = s.turret_yaw[0];
            s.tick(1. / 30.);
            assert!((s.turret_yaw[0] - before).abs() <= TURRET_SWEEP[0] * TURRET_SWEEP[1] / 30. + 1e-5, "the sweep is slow");
            (low, high) = (low.min(s.turret_yaw[0]), high.max(s.turret_yaw[0]));
        }
        assert!(low >= -TURRET_SWEEP[0] - 1e-4 && high <= TURRET_SWEEP[0] + 1e-4 && high - low > TURRET_SWEEP[0], "it swept {low}..{high}");
        assert_eq!(s.next_shot, 1, "it has nothing to fire at");
        for p in &s.players {
            assert!((p.hp - (100. - 2.2 * 10.)).abs() < 0.01 && !p.launched, "only hunger: {}", p.hp);
        }
    }
    #[test]
    fn a_turret_ignores_zombies_that_are_forming_hidden_or_out_of_range_and_takes_the_nearest_of_the_rest() {
        let settle = |s: &mut Feast| {
            watch(s, 75);
            s.turret_yaw[0]
        };
        // A forming zombie right in front of it is not a target; the formed one further off to the left is.
        let mut s = range();
        s.enemies.push(Zombie { forming: 1e9, ..formed(1, 1.5, 0.6) });
        s.enemies.push(formed(2, 0.7, 1.));
        assert!((settle(&mut s) - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        assert!(s.enemies[0].hp == 70. && s.enemies[1].hp < 70.);
        // Behind the wall it cannot be seen, however near: the turret goes on sweeping and holds its fire.
        let mut s = range();
        s.enemies.push(formed(1, 2.3, 1.));
        assert!(settle(&mut s).abs() <= TURRET_SWEEP[0] + 1e-4);
        assert_eq!((s.next_shot, s.enemies[0].hp), (1, 70.));
        // Its reach is 1.4 m from the muzzle: 1.39 m is tracked and hit, 1.41 m is not.
        for (far, tracked) in [(1.39f32, true), (1.41, false)] {
            let mut s = range();
            let gap = 0.12 + (far * far - 0.025 * 0.025).sqrt();
            s.enemies.push(formed(1, 1.5 - 0.8 * gap, 1. - 0.6 * gap));
            let yaw = settle(&mut s);
            assert_eq!(s.enemies[0].hp < 70., tracked, "{far} m");
            assert_eq!((yaw - 0.8f32.atan2(0.6)).abs() < 1e-3, tracked, "{far} m: yaw {yaw}");
        }
        // Of two it can see it takes the nearer, and the other once that one is gone.
        let mut s = range();
        s.enemies.push(formed(1, 0.6, 1.));
        s.enemies.push(formed(2, 1.5, 0.5));
        assert!(settle(&mut s).abs() < 1e-4);
        assert!(s.enemies[0].hp == 70. && s.enemies[1].hp < 70.);
        s.enemies.remove(1);
        assert!((settle(&mut s) - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        assert!(s.enemies[0].hp < 70.);
    }
    // Loose objects: the bowl on the bundled map, and what hotdogs, rounds, bursts and the counter's edge do to it.
    /// Nobody acts for the hotdogs and the wave never comes; the bowl has come to rest where the map puts it.
    fn still() -> Feast {
        let mut s = Feast::new(false);
        for (i, p) in s.players.iter_mut().enumerate() {
            (p.bot, p.p) = (false, [-1.5 + i as f32 * 0.1, FLOOR, 3.55]);
        }
        s.warmup = f32::INFINITY;
        for _ in 0..45 {
            s.tick(1. / 30.);
        }
        s
    }
    fn upright(s: &Feast) -> f64 {
        s.props.up(0)[1]
    }
    /// Whether a body stands clear of the bowl (prop 0), by geometry of the test's own: no point of the body's
    /// upright side is nearer the bowl's shell (its profile turned round its axis, 4 mm thick each way) than
    /// the body's radius. The rounded feet may rest on it.
    fn clear(s: &Feast, id: u32) -> bool {
        const PROFILE: [[f64; 2]; 4] = [[0., 0.], [0.065, 0.], [0.125, 0.045], [0.155, 0.105]];
        let (b, o, [x, y, z, w]) = (s.bodies.get(id).unwrap(), s.props.position(0), s.props.rotation(0));
        let mut h = b.radius;
        while h <= b.height - b.radius + 1e-9 {
            let v = [b.p[0] - o[0], b.p[1] + h - o[1], b.p[2] - o[2]];
            let (tx, ty, tz) = (2. * (z * v[1] - y * v[2]), 2. * (x * v[2] - z * v[0]), 2. * (y * v[0] - x * v[1]));
            let l = [v[0] + w * tx + (z * ty - y * tz), v[1] + w * ty + (x * tz - z * tx) - 0.004, v[2] + w * tz + (y * tx - x * ty)];
            let rho = l[0].hypot(l[2]);
            let near = PROFILE.windows(2).map(|pair| {
                let (er, ey) = (pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]);
                let t = (((rho - pair[0][0]) * er + (l[1] - pair[0][1]) * ey) / (er * er + ey * ey)).clamp(0., 1.);
                (rho - pair[0][0] - er * t).hypot(l[1] - pair[0][1] - ey * t)
            });
            if near.fold(f64::INFINITY, f64::min) - 0.004 - b.radius < -3e-4 {
                return false;
            }
            h += 0.005;
        }
        true
    }
    #[test]
    fn the_bundled_map_has_a_bowl_that_rests_on_the_counter_and_a_new_round_puts_it_back() {
        let mut s = still();
        assert_eq!(s.props.len(), 1);
        assert_eq!(s.props.shape(s.props.body(0).shape).spec.name, "bowl");
        let at = s.props.position(0);
        assert!((at[0] - 1.2).abs() < 1e-4 && (at[1] - FLOOR as f64).abs() < 1e-4 && (at[2] - 2.75).abs() < 1e-4, "the bowl rests at {at:?}");
        assert_eq!(s.props.state(0), props::State::Asleep);
        s.props.apply_impulse(0, [0.05, 0., 0.], None);
        for _ in 0..60 {
            s.tick(1. / 30.);
        }
        assert!(s.props.position(0)[0] > 1.2);
        s.reset();
        assert_eq!(s.props.position(0), [1.2, crate::map::FLOOR, 2.75]);
    }
    #[test]
    fn a_hotdog_walking_into_the_bowl_shoves_it_along_and_never_ends_a_step_inside_it() {
        // A bot's way of walking: steered, then stepped.
        let mut s = still();
        s.players[0].p = [0.85, FLOOR, 2.75];
        for i in 0..30 {
            s.embody();
            s.steer(1, [2., FLOOR as f64, 2.75], 0.67);
            s.tick(1. / 30.);
            assert!(clear(&s, 1), "inside the bowl at step {i}");
        }
        let (bowl, hotdog) = (s.props.position(0)[0], s.players[0].p[0] as f64);
        assert!(bowl > 1.35, "the bowl moved to {bowl}");
        assert!(hotdog > 1.05 && hotdog < bowl - 0.15, "the hotdog follows behind it, at {hotdog}");
        assert!(hotdog < 0.85 + 0.67 - 0.05, "and was held back by it");
        // A person's way: the hotdog is walked at once, and the step that follows puts it right.
        let mut s = still();
        s.players[0].p = [0.9, FLOOR, 2.75];
        s.tick(1. / 30.);
        for i in 0..40 {
            s.walk(1, Vec2::new(0.02, 0.), 0.);
            s.tick(1. / 30.);
            assert!(clear(&s, 1), "inside the bowl at step {i}");
        }
        assert!(s.props.position(0)[0] > 1.3, "the bowl moved to {}", s.props.position(0)[0]);
        assert!((s.players[0].p[0] as f64) < s.props.position(0)[0] - 0.15);
        // A zombie, lighter and slower, shoves it too.
        let mut s = still();
        s.enemies.push(Zombie { id: 1, p: [0.95, FLOOR, 2.75], yaw: 0., hp: 1e9, born: 0., forming: 0. });
        for _ in 0..150 {
            s.embody();
            s.steer(CAT + 1, [2., FLOOR as f64, 2.75], 0.1);
            s.tick(1. / 30.);
            assert!(clear(&s, CAT + 1));
        }
        assert!(s.props.position(0)[0] > 1.22, "the bowl moved to {}", s.props.position(0)[0]);
    }
    #[test]
    fn a_hotdog_can_stand_in_the_bowl_and_on_its_rim_which_tips_it() {
        let mut s = still();
        s.players[0].p = [1.2, FLOOR + 0.2, 2.75];
        for _ in 0..60 {
            s.tick(1. / 30.);
        }
        assert!((s.players[0].p[1] - (FLOOR + 0.008)).abs() < 0.002, "standing on the base at {}", s.players[0].p[1] - FLOOR);
        assert!(s.players[0].grounded && upright(&s) > 0.999);
        s.players[0].p = [1.2 - 0.155, FLOOR + 0.2, 2.75];
        let mut tilt = 1f64;
        for _ in 0..90 {
            s.tick(1. / 30.);
            tilt = tilt.min(upright(&s));
        }
        assert!(tilt < 0.9, "the bowl tipped to {tilt}");
        assert!(s.players[0].p[1] > FLOOR + 0.005 && s.players[0].p[1] < FLOOR + 0.1);
    }
    #[test]
    fn the_bowl_stops_a_round_which_nudges_it_and_gives_cover() {
        let wave = || {
            let mut s = still();
            (s.stage, s.wave_spawned) = (Stage::Wave, WAVE);
            // A zombie stands behind the bowl.
            s.enemies.push(Zombie { id: 1, p: [1.6, FLOOR, 2.75], yaw: 0., hp: 70., born: 0., forming: 0. });
            s.players[0].p = [0.7, FLOOR, 2.75];
            s
        };
        // Level with the wall of the bowl.
        let mut s = wave();
        let eye = Vec3::new(0.7, FLOOR + EYE, 2.75);
        let aim = (Vec3::new(1.08, FLOOR + 0.07, 2.75) - eye).normalize();
        assert!(s.cover(eye, aim) < 0.45 && s.level.cover_from(eye, aim) > 1.);
        assert!(s.fire(1, aim, 1));
        land(&mut s);
        let splat = s.splats[0].p;
        assert!(splat[0] > 1.04 && splat[0] < 1.09 && splat[1] > FLOOR + 0.05, "the round landed at {splat:?}");
        assert_eq!(s.enemies[0].hp, 70.);
        assert_eq!(s.props.state(0), props::State::Awake);
        assert!(s.props.velocity(0)[0] > 0.2, "the bowl was nudged along the shot");
        // Over the rim the same shot goes on to the zombie.
        let mut s = wave();
        assert!(s.fire(1, Vec3::X, 1));
        land(&mut s);
        assert_eq!(s.enemies[0].hp, 35.);
        assert_eq!(s.props.state(0), props::State::Asleep);
    }
    #[test]
    fn a_glob_bursting_beside_the_bowl_throws_it_and_its_landing_is_a_knock() {
        let glob = Projectile { owner: 1, weapon: 4, hostile: false, ..Default::default() };
        let mut s = still();
        s.burst(&glob, Vec3::new(1.0, FLOOR + 0.02, 2.75), None, None);
        let v = s.props.velocity(0);
        assert!(v[0] > 0.3 && v[1] > 0.05, "thrown with {v:?}");
        let (mut peak, mut knocks) = (0f64, 0);
        for _ in 0..90 {
            s.tick(1. / 30.);
            peak = peak.max(s.props.position(0)[1] - FLOOR as f64);
            knocks = knocks.max(s.knocks.len());
            assert!(s.knocks.iter().all(|k| k.prop == 0 && k.speed > 0.1));
        }
        assert!(peak > 0.01 && s.props.position(0)[0] > 1.3);
        assert!(knocks > 0, "it clanged down");
        // Out of the burst's reach nothing happens.
        let mut s = still();
        s.burst(&glob, Vec3::new(0.6, FLOOR + 0.02, 2.75), None, None);
        assert_eq!(s.props.state(0), props::State::Asleep);
    }
    #[test]
    fn a_prop_shoved_over_the_counters_edge_is_lost() {
        let mut s = still();
        s.props.apply_impulse(0, [0., 0., -0.6], None);
        let mut lost = None;
        for i in 0..150 {
            s.tick(1. / 30.);
            if s.props.state(0) == props::State::Lost {
                lost = Some(i);
                break;
            }
        }
        assert!(lost.is_some_and(|i| i > 20), "lost after {lost:?} steps");
        assert!(s.props.position(0)[2] < 2.27 && s.props.position(0)[1] < 0.3);
        // It no longer stops anything.
        let (o, d) = (Vec3::new(1.2, FLOOR + 0.05, 2.4), Vec3::NEG_Z);
        assert_eq!(s.cover(o, d), s.level.cover_from(o, d));
    }
    #[test]
    fn a_maps_loose_objects_are_put_out_in_file_order_each_kind_with_its_own_shape() {
        let map = small(r#"{"kind": "crate", "p": [0.5, 0.3], "base": 0, "yaw": 0.5}, {"kind": "turret", "p": [0.5, 0.5], "base": 0.3, "yaw": 0},
            {"kind": "pillow", "p": [1.2, 0.8], "base": 0.1, "yaw": 0}, {"kind": "bowl", "p": [0.6, 0.75], "base": 0, "yaw": 1}"#);
        let mut s = Feast::with_map(map, false);
        let names: Vec<&str> = s.props.bodies().iter().map(|b| s.props.shape(b.shape).spec.name).collect();
        assert_eq!(names, ["crate", "pillow", "bowl"]);
        let pillow = s.props.position(1);
        assert!((pillow[0] - 1.2).abs() < 1e-6 && (pillow[1] - (crate::map::FLOOR + 0.1)).abs() < 1e-6 && (pillow[2] - 0.8).abs() < 1e-6);
        assert_eq!(s.turrets.len(), 1);
        for p in &mut s.players {
            p.bot = false;
        }
        s.warmup = f32::INFINITY;
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        assert!(s.props.bodies().iter().all(|b| b.state == props::State::Asleep));
        assert!((s.props.position(1)[1] - crate::map::FLOOR).abs() < 0.001);
    }
    /// The stuck hotdog of the Forts build phase: the stock napkins lie at the bases' edges, and a hotdog walking
    /// onto one was let down to the counter at every stride and climbed it again, which held it back to under half
    /// its pace and shoved the napkin ahead of it (into a chair or a crate it then could not pass).
    #[test]
    fn a_hotdog_walks_across_a_napkin_at_its_full_pace_on_top_of_it_without_shoving_it_both_ways_and_off_centre() {
        let map = Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "name": "Rug", "bounds": [0, 0, 2, 1], "starts": [{"p": [0.2, 0.2], "yaw": 1.5}],
            "spawns": [{"p": [0.1, 0.9]}], "props": [{"kind": "napkin", "p": [1, 0.5], "base": 0, "yaw": 0}]}"#,
        )
        .unwrap();
        for (from, way, z) in [(0.4_f32, 1_f32, 0.5_f32), (1.6, -1., 0.5), (0.4, 1., 0.58), (1.6, -1., 0.42)] {
            let mut s = Feast::with_map(map.clone(), false);
            for p in &mut s.players {
                p.bot = false;
                p.alive = p.id == 1;
            }
            s.warmup = f32::INFINITY;
            for _ in 0..30 {
                s.tick(1. / 30.);
            }
            s.players[0].p = [from, FLOOR, z];
            s.embody();
            let before = s.props.position(0);
            let mut on = 0_f32;
            for _ in 0..45 {
                s.bodies.get_mut(1).unwrap().wish = [0.67 * way as f64, 0.];
                s.tick(1. / 30.);
                let p = s.players[0].p;
                if (p[0] - 1.).abs() < 0.03 {
                    on = on.max(p[1] - FLOOR);
                }
            }
            let (p, after) = (s.players[0].p, s.props.position(0));
            assert!((p[0] - from).abs() > 0.98, "from {from}: only {} m in 1.5 s", (p[0] - from).abs());
            assert!((p[2] - z).abs() < 0.002, "it drifted {} m sideways", p[2] - z);
            assert!(on > 0.006, "over the middle its feet were {on} m up: on the counter, not the napkin");
            let shoved = (after[0] - before[0]).hypot(after[2] - before[2]);
            assert!(shoved < 0.002, "the napkin was shoved {shoved} m");
        }
    }
    #[test]
    fn demo_shows_friendly_fire_and_a_meal() {
        let mut s = Feast::new(true);
        for _ in 0..3600 {
            s.tick(1. / 30.);
            if s.players.iter().any(|p| p.meals > 0) {
                break;
            }
        }
        assert!(s.friendly_fire_events > 0);
        assert!(s.players.iter().any(|p| p.meals > 0));
    }
    #[test]
    fn aim_pitch_follows_the_player_every_tick_and_bots_aim_at_what_they_shoot() {
        let mut s = Feast::new(false);
        assert_eq!(s.players[0].pitch, 0.);
        s.look(1, 0.6);
        assert_eq!(s.players[0].pitch, 0.6);
        s.look(1, -9.);
        assert_eq!(s.players[0].pitch, -1.5, "pitch is clamped short of straight down");
        // Firing aims along the shot, up or down.
        assert!(s.fire(1, Vec3::new(0., 0.6, 0.8), 1));
        assert!((s.players[0].pitch - 0.6f32.asin()).abs() < 1e-6);
        // A bot shooting a zombie on the counter aims below the horizon, and levels off to walk.
        let mut s = Feast::new(true);
        let mut shooter = None;
        for _ in 0..900 {
            s.tick(1. / 30.);
            shooter = s.shots.iter().find(|shot| shot.owner > 0).map(|shot| shot.owner);
            if shooter.is_some() {
                break;
            }
        }
        let bot = s.player(shooter.expect("a bot fires during the wave")).unwrap().clone();
        assert!(bot.pitch < -0.01 && bot.pitch > -1., "bot pitch {}", bot.pitch);
        // Its head bows with its aim (head.rs), and comes up as it levels off.
        assert!(crate::head::nod_of(&bot) < -0.005 && crate::head::head_centre(&bot).y < crate::head::place(&bot, Vec3::Y * crate::head::HEAD_AT).y);
        s.advance_actor(bot.id, Vec3::from_array(bot.p) + Vec3::X, 1. / 30.);
        assert_eq!(s.player(bot.id).unwrap().pitch, 0.);
        assert_eq!(crate::head::nod_of(s.player(bot.id).unwrap()), 0.);
    }
    #[test]
    fn the_swing_counter_counts_every_melee_attack_once() {
        let mut s = Feast::new(false);
        s.stage = Stage::Wave;
        assert_eq!(s.players[0].swings, 0);
        // A strike at thin air still swings; a strike refused by the cooldown does not.
        assert!(s.melee(1, Vec3::Z));
        assert_eq!(s.players[0].swings, 1);
        assert!(!s.melee(1, Vec3::Z));
        assert_eq!(s.players[0].swings, 1);
        // A gun is not a swing.
        s.players[0].cooldown = 0.;
        assert!(s.fire(1, Vec3::Z, 1));
        assert_eq!(s.players[0].swings, 1);
        // A strike that lands on a zombie counts once.
        for _ in 0..900 {
            if s.enemies.iter().any(|e| e.forming <= 0.) {
                break;
            }
            s.tick(1. / 30.);
        }
        let zombie = s.enemies.iter().find(|e| e.forming <= 0.).unwrap().clone();
        s.players[0].p = [zombie.p[0], zombie.p[1], zombie.p[2] - 0.12];
        s.players[0].cooldown = 0.;
        assert!(s.melee(1, Vec3::Z));
        assert_eq!(s.players[0].swings, 2);
        assert!(s.enemies.iter().all(|e| e.id != zombie.id || e.hp < zombie.hp), "the strike landed");
        // In the free for all a strike at thin air swings too, a living hotdog in reach is bitten, and a
        // bite is one swing however it is asked for.
        let mut s = duel();
        assert!(s.melee(1, Vec3::Z));
        assert_eq!((s.players[0].swings, s.players[0].meals, s.players[1].hp), (1, 0, 100.));
        s.players[1].p = s.players[0].p;
        assert!(s.melee(1, Vec3::Z), "a bite does not wait for the strike's cooldown, as F never did");
        assert_eq!((s.players[0].swings, s.players[0].meals, s.players[1].hp), (2, 1, 50.), "half of its HP: it faces the biter");
        // Until the bite has cooled there is no striking either.
        s.players[0].cooldown = 0.;
        assert!(!s.melee(1, Vec3::Z));
        s.players[0].bite_cooldown = 0.;
        assert!(s.bite(1));
        assert_eq!((s.players[0].swings, s.players[0].meals), (3, 2));
        s.reset();
        assert!(s.players.iter().all(|p| p.swings == 0 && p.pitch == 0.));
    }
    /// A wave already under way on open counter with one formed zombie 0.12 m in front of hotdog 1 (towards +z).
    fn brawl() -> Feast {
        let mut s = Feast::new(false);
        s.stage = Stage::Wave;
        s.wave_spawned = WAVE;
        for (i, p) in s.players.iter_mut().enumerate() {
            p.bot = false;
            p.p = [-1.5 + i as f32 * 0.1, FLOOR, 3.55];
        }
        s.players[0].p = [0., FLOOR, 2.4];
        s.enemies.push(Zombie { id: 1, p: [0., FLOOR, 2.52], yaw: 0., hp: 70., born: 0., forming: 0. });
        s
    }
    #[test]
    fn every_weapon_strikes_in_melee_with_the_old_knifes_numbers_even_when_empty() {
        // (Half of what a zombie has and half of what a hotdog has, from the front: melee_tests.rs has the rest.)
        assert_eq!((DELAY[0], RANGE[0], ZOMBIE_DAMAGE[0], PLAYER_DAMAGE[0]), (0.48, 0.21, 35., 50.));
        for weapon in 1..=RIFLE {
            let mut s = brawl();
            let p = &mut s.players[0];
            (p.owned, p.ammo, p.weapon, p.gun) = ([false, true, true, true, true, true, true, true], [-1, 0, 0, 0, 0, 0, 0, 0], weapon, weapon);
            // The gun is empty: the trigger does nothing, the strike lands.
            assert!(!s.fire(1, Vec3::Z, weapon));
            s.players[0].cooldown = 0.;
            let (before, swings) = (s.enemies[0].hp, s.players[0].swings);
            assert!(s.melee(1, Vec3::Z));
            assert_eq!(before - s.enemies[0].hp, 35., "weapon {weapon}");
            assert_eq!((s.players[0].swings, s.players[0].cooldown, s.players[0].weapon), (swings + 1, 0.48, weapon));
            assert_eq!(s.players[0].ammo, [-1, 0, 0, 0, 0, 0, 0, 0]);
            assert!(s.projectiles.is_empty() && s.shots.is_empty(), "a strike launches nothing");
            assert!(s.bodies.get(CAT + 1).unwrap().launched, "and shoves the zombie it lands on");
            // It reaches 0.21 m and no further, and a forming zombie is not struck.
            s.players[0].cooldown = 0.;
            s.enemies[0].p = [0., FLOOR, 2.4 + 0.21 + 0.065 + 0.01];
            let hp = s.enemies[0].hp;
            assert!(s.melee(1, Vec3::Z));
            assert_eq!(s.enemies[0].hp, hp);
            s.players[0].cooldown = 0.;
            s.enemies[0].p = [0., FLOOR, 2.52];
            s.enemies[0].forming = 1.;
            assert!(s.melee(1, Vec3::Z));
            assert_eq!(s.enemies[0].hp, hp);
        }
        // Two strikes from the front kill a zombie and the kill is the striker's.
        let mut s = brawl();
        for _ in 0..2 {
            s.players[0].cooldown = 0.;
            s.enemies[0].p = [0., FLOOR, 2.52];
            assert!(s.melee(1, Vec3::Z));
        }
        assert!(s.enemies.is_empty());
        assert_eq!((s.players[0].kills, s.wave_kills), (1, 1));
        // During the wave a strike never hurts a teammate; nor does a bite.
        let mut s = brawl();
        s.enemies.clear();
        s.players[1].p = [0., FLOOR, 2.5];
        assert!(s.melee(1, Vec3::Z));
        assert_eq!((s.players[1].hp, s.friendly_fire_events), (100., 0));
    }
    #[test]
    fn there_are_no_bare_hands_and_no_knife_a_hotdog_starts_with_the_syrup_rifle_in_hand_and_the_mustard_pistol_beside_it_always_holds_a_gun_and_biting_keeps_it_there() {
        for demo in [false, true] {
            let s = Feast::new(demo);
            for p in &s.players {
                assert_eq!((p.weapon, p.gun, p.owned), (RIFLE, RIFLE, [false, true, false, false, false, false, true, false]), "{}", p.name);
                // Person and bot alike: a full magazine of each and its spare rounds (rifle 60 and 180, pistol 12 and 48).
                assert_eq!(p.ammo, [-1, 12, 0, 0, 0, 0, 60, 0], "{}", p.name);
                assert_eq!(p.reserve, [0, 48, 0, 0, 0, 0, 180, 0], "{}", p.name);
            }
            // (A pickup is a gun or a box of ammunition, never a melee weapon: weapon 0 is the box.)
            assert!(s.pickups.iter().all(|item| (1..GUNS).contains(&item.weapon) || (item.weapon == crate::map::AMMO_BOX && item.ammo == 0)), "no pickup is a melee weapon");
        }
        // Nothing selects bare hands (weapon 0): asking for them, or for a gun it does not hold, leaves the gun in
        // hand; with no gun in hand (the hands full, which only carrying does) the trigger throws no punch.
        let mut s = duel();
        for weapon in [0, 2, 7, 99] {
            s.select(1, weapon);
            assert_eq!((s.players[0].weapon, s.players[0].gun), (RIFLE, RIFLE), "asked for {weapon}");
        }
        s.players[0].cooldown = 0.;
        assert!(!s.fire(1, Vec3::Z, 0));
        assert_eq!((s.players[0].swings, s.shots.len(), s.projectiles.len()), (0, 0, 0), "no punch");
        // The pistol is the other gun: it can be taken in hand, and the rifle again.
        s.select(1, 1);
        assert_eq!((s.players[0].weapon, s.players[0].gun), (1, 1));
        s.select(1, RIFLE);
        assert_eq!((s.players[0].weapon, s.players[0].gun), (RIFLE, RIFLE));
        // A bite or a meal leaves the gun in the hand: there is nothing to draw.
        s.players[1].p = s.players[0].p;
        assert!(s.bite(1));
        assert_eq!(s.players[0].weapon, RIFLE);
        s.players[0].bite_cooldown = 0.;
        s.hurt(2, 1000.);
        assert!(s.eat(1));
        assert_eq!(s.players[0].weapon, RIFLE);
        // A whole demo round: no bot ever has anything but a gun in hand.
        let mut s = Feast::new(true);
        for _ in 0..3600 {
            s.tick(1. / 30.);
            assert!(s.players.iter().all(|p| p.weapon >= 1 && p.owned[p.weapon] && p.owned[1..].iter().any(|held| *held)), "a bot put its gun away");
            if s.stage == Stage::RoundOver {
                break;
            }
        }
    }
    #[test]
    fn crouch_is_a_stance_between_standing_and_prone_lower_centre_and_muzzle_slower_and_able_to_jump() {
        assert!(LOW < MID && MID < EYE && CHEST[2] < CHEST[1] && CHEST[1] < CHEST[0]);
        // Level fire from a standing muzzle passes over a crouching hotdog (mustard, ketchup, hot sauce), as it
        // hits a standing one; a relish glob is too fat to miss. Aimed at the crouching hotdog, every one lands.
        for (weapon, level) in [(1, false), (2, false), (3, false), (4, true)] {
            let mut s = duel();
            s.players[0].owned[weapon] = true;
            s.players[0].ammo[weapon] = 5;
            assert!(s.fire(1, Vec3::Z, weapon));
            land(&mut s);
            assert!(s.players[1].hp < 100., "weapon {weapon} hits a standing hotdog");
            let mut s = duel();
            s.players[0].owned[weapon] = true;
            s.players[0].ammo[weapon] = 5;
            s.set_crouch(2, true);
            assert!(s.fire(1, Vec3::Z, weapon));
            land(&mut s);
            assert_eq!(s.players[1].hp < 100., level, "weapon {weapon} fired level at a crouching hotdog");
            // (A relish glob kills: the target is back for the next shot.)
            (s.players[1].hp, s.players[1].alive) = (s.players[1].hp.max(1.), true);
            s.embody();
            s.set_crouch(2, true);
            let before = s.players[1].hp;
            s.players[0].cooldown = 0.;
            let aim = (chest(&s.players[1]) - muzzle(&s.players[0])).normalize();
            assert!(aim.y < -0.15 && s.fire(1, aim, weapon));
            land(&mut s);
            assert!(s.players[1].hp < before, "weapon {weapon} aimed at a crouching hotdog");
        }
        // The muzzle drops to MID; prone wins over crouch.
        let mut s = duel();
        s.set_crouch(1, true);
        assert!(s.fire(1, Vec3::Z, 1));
        assert!((s.projectiles[0].p[1] - (FLOOR + MID)).abs() < 1e-6);
        s.set_prone(1, true);
        s.players[0].cooldown = 0.;
        assert!(s.fire(1, Vec3::Z, 1));
        assert!((s.projectiles[1].p[1] - (FLOOR + LOW)).abs() < 1e-6);
        assert_eq!(chest(&s.players[0]).y, FLOOR + CHEST[2]);
        // The body: 0.10 m tall, 65% pace (a prone one 40%), and it jumps as a standing one does.
        let stride = |crouch: bool, prone: bool| {
            let mut s = brawl();
            s.enemies.clear();
            (s.players[0].crouch, s.players[0].prone) = (crouch, prone);
            s.embody();
            s.steer(1, [1., FLOOR as f64, 2.4], 0.67);
            s.tick(1. / 30.);
            (s.players[0].p[0], s.body(1).unwrap().stature())
        };
        let (standing, crouching, prone) = (stride(false, false), stride(true, false), stride(false, true));
        assert!((crouching.0 / standing.0 - 0.65).abs() < 1e-4 && (prone.0 / standing.0 - 0.4).abs() < 1e-4, "{standing:?} {crouching:?} {prone:?}");
        assert_eq!([standing.1, crouching.1, prone.1, stride(true, true).1], [actor::HEIGHT, actor::CROUCH_HEIGHT, actor::PRONE_HEIGHT, actor::PRONE_HEIGHT]);
        let leap = |crouch: bool, prone: bool| {
            let mut s = brawl();
            s.enemies.clear();
            (s.players[0].crouch, s.players[0].prone) = (crouch, prone);
            s.jump(1, actor::JUMP as f32);
            let (mut apex, mut steps) = (0f32, 0);
            for i in 1..=60 {
                s.tick(1. / 30.);
                apex = apex.max(s.players[0].p[1] - FLOOR);
                if !s.players[0].grounded {
                    steps = i;
                }
            }
            (apex, steps)
        };
        assert_eq!(leap(true, false), leap(false, false));
        assert!(leap(true, false).0 > 0.11 && leap(false, true) == (0., 0), "a crouching hotdog jumps; a prone one does not");
        // A player record from before crouching existed reads as standing, and the stance is in the record.
        let old: Player = serde_json::from_str(r#"{"id": 3, "prone": true}"#).unwrap();
        assert!(!old.crouch && old.prone);
        let mut s = Feast::new(false);
        s.set_crouch(1, true);
        assert_eq!(serde_json::to_value(&s.players[0]).unwrap()["crouch"], true);
        s.reset();
        assert!(s.players.iter().all(|p| !p.crouch));
    }
    // ----- Bots

    /// An open yard with no turret: a sniper, a hot sauce thrower, a bazooka, a can of bug spray and some
    /// mustard to collect, and one small block.
    fn yard() -> Map {
        Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "name": "Yard", "bounds": [0, 0, 4, 2], "starts": [{"p": [0.2, 1.8]}], "spawns": [{"p": [0.1, 0.1]}],
            "pickups": [{"p": [2, 0.3], "weapon": 2, "ammo": 6}, {"p": [2.3, 0.3], "weapon": 3, "ammo": 90}, {"p": [1.2, 1.7], "weapon": 4, "ammo": 4},
            {"p": [3.5, 1.5], "weapon": 5, "ammo": 40}, {"p": [3.6, 0.3], "weapon": 1, "ammo": 12}], "blocks": [{"rect": [3, 0.9, 3.1, 1.1], "height": 0.3}]}"#,
        )
        .unwrap()
    }
    /// One bot (hotdog 1) in the middle of the yard holding the given guns ([weapon, rounds], the first in
    /// hand), in a wave that is already complete, with a formed zombie `far` metres off towards -x (none for
    /// 0). The other hotdogs are parked in a corner and are nobody's bots; the pickups are out unless `shop`.
    fn skirmish(guns: &[(usize, i32)], far: f32, shop: bool) -> Feast {
        let mut s = Feast::with_map(yard(), false);
        // (Which gun, when and where: the bot aims as bots once did, at once and true; aim.rs has its own tests.)
        s.rules.skill = crate::aim::Skill::Perfect;
        s.stage = Stage::Wave;
        s.wave_spawned = WAVE;
        for (i, p) in s.players.iter_mut().enumerate() {
            (p.bot, p.hp, p.p) = (i == 0, 1e6, [3.5 + 0.1 * i as f32, FLOOR, 1.9]);
        }
        let p = &mut s.players[0];
        (p.p, p.owned, p.ammo, p.reserve) = ([2., FLOOR, 1.], [false; GUNS], [-1, 0, 0, 0, 0, 0, 0, 0], [0; GUNS]);
        for (w, rounds) in guns {
            (p.owned[*w], p.ammo[*w]) = (true, *rounds);
        }
        (p.weapon, p.gun) = (guns[0].0, guns[0].0);
        if !shop {
            for item in &mut s.pickups {
                item.ready = f32::INFINITY;
            }
        }
        if far > 0. {
            s.enemies.push(Zombie { id: 1, p: [2. - far, FLOOR, 1.], yaw: 0., hp: 1e6, born: 0., forming: 0. });
        }
        s
    }
    /// Steps a skirmish, counting the rounds hotdog 1 launches with each weapon, and checking that it never
    /// puts its gun away.
    fn volley(s: &mut Feast, ticks: usize) -> [u32; 6] {
        let (mut fired, mut seen) = ([0; 6], s.next_shot);
        for _ in 0..ticks {
            s.tick(1. / 30.);
            for shot in s.shots.iter().filter(|shot| shot.id >= seen && shot.owner == 1) {
                fired[shot.weapon] += 1;
            }
            seen = s.next_shot;
            assert!(s.players[0].weapon > 0, "the bot put its gun away");
        }
        fired
    }
    fn reach(s: &Feast) -> f32 {
        Vec3::from_array(s.players[0].p).distance(Vec3::from_array(s.enemies[0].p))
    }
    #[test]
    fn a_bot_uses_each_gun_at_the_range_that_suits_it() {
        // The mustard pistol at any range it can see, leading the zombie's walk: the rounds land.
        let mut s = skirmish(&[(1, 60)], 1.2, false);
        assert!(volley(&mut s, 90)[1] >= 6 && s.enemies[0].hp < 1e6, "mustard lands");
        // The ketchup sniper at range, the pistol once the zombie is close: it swaps to the other gun.
        let mut s = skirmish(&[(1, 60), (2, 6)], 1.5, false);
        assert_eq!(volley(&mut s, 60), [0, 0, 2, 0, 0, 0], "the sniper at 1.5 m");
        assert_eq!(s.players[0].weapon, 2);
        s.enemies[0].p = [1.6, FLOOR, 1.];
        let fired = volley(&mut s, 45);
        assert!(fired[1] >= 3 && fired[2] == 0 && s.players[0].weapon == 1, "the pistol at 0.4 m: {fired:?}");
        // With only the sniper loaded it keeps its distance: it backs away from a zombie inside 0.7 m,
        // facing it and still shooting.
        let mut s = skirmish(&[(2, 6), (1, 0)], 0.45, false);
        assert_eq!(volley(&mut s, 3), [0, 0, 1, 0, 0, 0]);
        assert!(s.players[0].p[0] > 2.02 && reach(&s) > 0.47, "it backs off as it fires: {} m", reach(&s));
        assert!((-s.players[0].yaw.sin()) < -0.9, "and keeps facing the zombie");
        // (Its shot throws the zombie back; once that is 0.7 m off it stands and goes on shooting.)
        let fired = volley(&mut s, 90);
        assert!(fired[2] >= 2 && fired[1] == 0 && reach(&s) > 0.6, "{fired:?} at {} m", reach(&s));
        // The hot sauce thrower is carried into range first: nothing is fired until the zombie is within
        // 0.6 m, and then it is hot sauce, not mustard. From far off the pistol is used instead.
        let mut s = skirmish(&[(1, 60), (3, 90)], 1.1, false);
        let mut seen = false;
        for _ in 0..150 {
            let far = reach(&s);
            let fired = volley(&mut s, 1);
            assert!(fired[1] == 0 && (fired[3] == 0 || far < 0.6 + 0.02), "{fired:?} at {far} m");
            seen |= fired[3] > 0;
        }
        assert!(seen && s.players[0].p[0] < 1.7, "it closed in and sprayed");
        let mut s = skirmish(&[(1, 60), (3, 90)], 1.9, false);
        let fired = volley(&mut s, 30);
        assert!(fired[1] >= 2 && fired[3] == 0, "{fired:?}");
        // The relish bazooka at a lone zombie well away from it; never at one within 0.6 m.
        let mut s = skirmish(&[(1, 60), (4, 4)], 1.5, false);
        assert_eq!(volley(&mut s, 45), [0, 0, 0, 0, 2, 0], "relish at 1.5 m");
        // (Its mustard, quick to land now, may throw the zombie back past 0.6 m: only then does the relish come out.)
        let mut s = skirmish(&[(1, 60), (4, 4)], 0.5, false);
        let mut mustard = 0;
        for _ in 0..45 {
            let far = reach(&s);
            let fired = volley(&mut s, 1);
            assert!(fired[4] == 0 || far > 0.6 - 0.02, "{fired:?} at {far} m");
            mustard += fired[1];
        }
        assert!(mustard >= 3, "{mustard} mustard rounds");
    }
    #[test]
    fn a_bot_strikes_when_its_target_is_in_reach_or_its_guns_are_empty_and_swaps_to_the_gun_that_is_loaded() {
        // In reach, it strikes with the loaded gun rather than firing it.
        let mut s = skirmish(&[(1, 60)], 0.12, false);
        assert_eq!(volley(&mut s, 30), [0; 6]);
        assert!(s.players[0].swings >= 2 && s.players[0].ammo[1] == 60 && s.enemies[0].hp <= 1e6 - 60.);
        // Every gun empty and nothing to collect: it closes in and strikes with the empty gun in its hand.
        let mut s = skirmish(&[(2, 0), (1, 0)], 0.9, false);
        assert_eq!(volley(&mut s, 150), [0; 6]);
        assert!(reach(&s) < 0.17 && s.players[0].swings >= 2 && s.enemies[0].hp < 1e6, "{} m, {} swings", reach(&s), s.players[0].swings);
        assert_eq!(s.players[0].weapon, 2, "the empty sniper is the club");
        // One gun runs dry: the next shot comes from the other.
        let mut s = skirmish(&[(2, 1), (1, 60)], 1.5, false);
        let fired = volley(&mut s, 60);
        assert!(fired[2] == 1 && fired[1] >= 2 && s.players[0].weapon == 1, "{fired:?}");
        // Out of ammunition with some to be had: it fetches it instead of charging (a sniper lies 0.7 m off).
        let mut s = skirmish(&[(1, 0)], 1.8, true);
        volley(&mut s, 120);
        // (At the bots' walk it is there in time for a shot or two of the sniper's four.)
        assert!(s.players[0].owned[2] && s.players[0].ammo[2] < 4, "it collected the sniper and used it: {:?}", s.players[0].ammo);
    }
    #[test]
    fn a_bot_walks_to_the_guns_it_wants_and_leaves_the_rest() {
        let want = |guns: &[(usize, i32)]| {
            let s = skirmish(guns, 0., true);
            s.pickups.iter().map(|item| Feast::wants(&s.players[0], item) as u8).collect::<Vec<_>>()
        };
        // In the yard's order: sniper, hot sauce, bazooka, bug spray, mustard. (The same in every gun mode: Forts' bots
        // once gave up only an empty gun.)
        assert_eq!(want(&[(1, 60)]), [1, 1, 1, 1, 0], "a free hand takes any gun, and its pistol is full");
        assert_eq!(want(&[(RIFLE, 60), (1, 60)]), [1, 1, 1, 1, 0], "the pistol it starts with beside the rifle is held as a free hand would be");
        assert_eq!(want(&[(RIFLE, 60), (2, 6)]), [0, 0, 0, 0, 0], "two loaded guns and no pistol want nothing");
        assert_eq!(want(&[(1, 60), (2, 0)]), [1, 1, 1, 0, 0], "an empty gun is refilled or swapped, but not for bug spray");
        assert_eq!(want(&[(RIFLE, 0), (1, 60)]), [1, 1, 1, 0, 0], "nor is the pistol, with nothing else to shoot");
        assert_eq!(want(&[(1, 5), (3, 90)]), [1, 0, 1, 1, 1], "mustard when the pistol runs low, and the pistol traded for any other gun");
        // During the warm-up a bot with the guns it starts with walks to the nearest gun and collects it, giving
        // up its pistol; with both hands full of loaded guns it leaves the rest where they lie.
        let mut s = skirmish(&[(RIFLE, 60), (1, 60)], 0., true);
        s.stage = Stage::Warmup;
        s.warmup = f32::INFINITY;
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        let p = &s.players[0];
        assert_eq!((p.owned, p.ammo[1], p.ammo[2], p.ammo[RIFLE], p.weapon), ([false, false, true, false, false, false, true, false], 0, 4, 60, 2), "it traded its pistol for the sniper 0.7 m away (a power weapon: 4 rounds)");
        assert!(s.pickups[0].ready > s.elapsed && s.pickups[1].ready == 0.);
        for _ in 0..150 {
            s.tick(1. / 30.);
        }
        assert_eq!(s.players[0].owned, [false, false, true, false, false, false, true, false], "it collected nothing more");
        assert!(s.pickups[1..].iter().all(|item| item.ready == 0.));
        // Its sniper empty, it swaps it for the hot sauce thrower next to it; the rifle stays.
        s.players[0].ammo[2] = 0;
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        assert_eq!((s.players[0].owned, s.players[0].ammo[3], s.players[0].ammo[RIFLE]), ([false, false, false, true, false, false, true, false], 90, 60));
        // Two bots do not go for the same gun: the second takes another.
        let mut s = skirmish(&[(RIFLE, 60), (1, 60)], 0., true);
        s.stage = Stage::Warmup;
        s.warmup = f32::INFINITY;
        // (Hotdog 2 has the guns it starts with too.)
        assert_eq!(s.players[1].owned, [false, true, false, false, false, false, true, false]);
        (s.players[1].bot, s.players[1].p) = (true, [2.1, FLOOR, 1.]);
        for _ in 0..120 {
            s.tick(1. / 30.);
        }
        let held: Vec<usize> = s.players[..2].iter().map(|p| (2..6).find(|w| p.owned[*w]).unwrap_or(0)).collect();
        assert!(held.contains(&2) && held.contains(&3), "one took the sniper and the other the hot sauce: {held:?}");
        // A pickup it cannot reach is given up, not walked at for ever.
        let mut s = Feast::with_map(
            Map::parse(
                r#"{"format": "hotdog-map", "version": 1, "name": "Shelf", "bounds": [0, 0, 2, 1], "starts": [{"p": [0.3, 0.5]}], "spawns": [{"p": [0.1, 0.1]}],
                "pickups": [{"p": [1.5, 0.5], "weapon": 2, "ammo": 6}], "blocks": [{"rect": [1, 0, 1.1, 1], "height": 0.3}]}"#,
            )
            .unwrap(),
            true,
        );
        s.warmup = f32::INFINITY;
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        for p in &s.players {
            assert!(p.errand.is_none() && p.shun == [0] && p.p[0] < 0.98, "{} set out for the far side of the wall: {:?} {:?} {:?}", p.name, p.errand, p.shun, p.p);
        }
    }
    #[test]
    fn a_bot_sprays_the_swarm_that_is_on_it_and_steps_out_of_one_without_the_can() {
        let mut s = skirmish(&[(1, 60), (5, 40)], 0., false);
        s.pests.push(Swarm { id: 1, p: [2., FLOOR, 1.], hp: 100., ..Default::default() });
        let fired = volley(&mut s, 30);
        assert_eq!(fired, [0, 0, 0, 0, 0, 5], "five puffs clear it");
        assert!(s.pests.is_empty() && s.players[0].score == 50 && s.players[0].ammo[5] == 35);
        assert!(Vec3::from_array(s.players[0].p).distance(Vec3::new(2., FLOOR, 1.)) < 0.05, "it stood its ground");
        let mut s = skirmish(&[(1, 60)], 0., false);
        s.pests.push(Swarm { id: 1, p: [2., FLOOR, 1.], hp: 100., ..Default::default() });
        volley(&mut s, 45);
        assert!(!infested(&s.pests[0], s.players[0].p), "without the can it walks out");
    }
    #[test]
    fn bots_never_land_a_relish_glob_among_their_team_during_the_wave() {
        // A teammate stands beside the zombie: the glob stays in the tube and mustard is used.
        let mut s = skirmish(&[(1, 60), (4, 4)], 1.5, false);
        s.players[1].p = [0.7, FLOOR, 1.];
        let fired = volley(&mut s, 60);
        assert!(fired[4] == 0 && fired[1] >= 3, "{fired:?}");
        // Nor when the teammate is anywhere near the glob's way there.
        let mut s = skirmish(&[(1, 60), (4, 4)], 1.5, false);
        s.players[1].p = [1.3, FLOOR, 1.3];
        assert_eq!(volley(&mut s, 60)[4], 0);
        // Whole demo rounds, on both bundled maps and at three step lengths: wherever a glob lands during the
        // wave, every living hotdog (its shooter too) is outside the burst.
        let (mut globs, mut closest) = (0, f32::MAX);
        for map in [Map::bundled(), busier()] {
            for hz in [30., 45., 60.] {
                let mut s = Feast::with_map(map.clone(), true);
                let mut seen = 0;
                while s.stage != Stage::RoundOver && s.elapsed < 120. {
                    s.tick(1. / hz);
                    for splat in s.splats.iter().filter(|splat| splat.id > seen && splat.weapon == 4) {
                        if s.stage == Stage::Wave {
                            globs += 1;
                            for p in s.players.iter().filter(|p| p.alive) {
                                closest = closest.min((p.p[0] - splat.p[0]).hypot(p.p[2] - splat.p[2]));
                            }
                        }
                    }
                    seen = s.splats.iter().map(|splat| splat.id).fold(seen, u32::max);
                }
                assert_eq!(s.stage, Stage::RoundOver);
            }
        }
        assert!(closest > actor::BLAST_RADIUS as f32, "{globs} globs in the wave, one {closest} m from a hotdog");
        // In the free for all a bot still keeps out of its own burst: with a rival 0.5 m away it backs off, and
        // fires no glob until there is 0.6 m between them.
        let mut s = skirmish(&[(4, 4), (1, 0)], 0., false);
        s.begin_famine();
        s.famine_at = -10.;
        (s.players[1].p, s.players[0].hp) = ([1.5, FLOOR, 1.], 100.);
        let (mut globs, mut least) = (0, f32::MAX);
        for _ in 0..60 {
            let far = Vec3::from_array(s.players[0].p).distance(Vec3::from_array(s.players[1].p));
            if volley(&mut s, 1)[4] > 0 {
                (globs, least) = (globs + 1, least.min(far));
            }
        }
        assert!(globs >= 1 && least > 0.6 && s.players[0].p[0] > 2.08, "{globs} globs, the nearest from {least} m");
    }
    #[test]
    fn the_demo_shows_several_kinds_of_gun_collected_and_fired() {
        let mut s = Feast::new(true);
        let (mut fired, mut seen, mut held) = ([0u32; GUNS], 1, [false; GUNS]);
        while s.stage != Stage::RoundOver && s.elapsed < 120. {
            s.tick(1. / 30.);
            for shot in s.shots.iter().filter(|shot| shot.id >= seen && shot.owner > 0) {
                fired[shot.weapon] += 1;
            }
            seen = s.next_shot;
            for p in &s.players {
                held[p.weapon] |= p.alive;
            }
        }
        assert_eq!(s.stage, Stage::RoundOver);
        // Every bot starts with the syrup rifle and the mustard pistol, and trades the pistol for a gun lying about.
        // (Since the rifle's bursts came quicker, the hot sauce's bot is often down before anything is near enough to
        // spray; and since every gun mode starts the rifle with 180 rounds in reserve, a bot seldom needs another gun,
        // and the power weapons' pickups give 4 and 2: the others are seen in hand, and of the guns fired two kinds.)
        assert!(fired[RIFLE] > 0 && [2, 3, 4].iter().filter(|w| fired[**w] > 0).count() >= 1, "syrup and at least one of ketchup, hot sauce and relish are fired: {fired:?}");
        assert!(!held[0] && held[2] && held[3] && held[4] && held[RIFLE], "and seen in hand: {held:?}");
        assert!(s.players.iter().filter(|p| !p.owned[1] && (2..6).any(|w| p.owned[w])).count() >= 4, "most of the bots traded the pistol for another gun");
        assert_eq!((s.wave_spawned, s.wave_kills, s.players.iter().filter(|p| p.alive).count()), (6, 6, 1));
        assert!(s.winner.is_some() && s.players.iter().any(|p| p.meals > 0) && s.friendly_fire_events > 0);
    }
    #[test]
    fn a_demo_round_ends_with_a_winner_and_a_meal_at_nearly_every_frame_rate() {
        // The browser steps the simulation once a frame (twice below 30 frames a second), so the demo is not
        // one fixed game. Every one of them must clear the wave, show friendly fire and a meal, and end; and
        // all but the odd one must leave exactly one hotdog (now and then the last two kill each other in the
        // same step, which is the round nobody wins).
        let (mut odd, mut draws) = (vec![], 0);
        for (name, map) in [("bundled", Map::bundled()), ("v2", busier())] {
            for hz in [20., 24., 30., 37., 45., 50., 60., 72., 75., 90., 100., 120., 144., 165.] {
                let mut s = Feast::with_map(map.clone(), true);
                while s.stage != Stage::RoundOver && s.elapsed < 150. {
                    s.tick(1. / hz);
                }
                let (alive, meals) = (s.players.iter().filter(|p| p.alive).count(), s.players.iter().map(|p| p.meals).sum::<u32>());
                println!("{name} at {hz} Hz: over at {:.1} s, {alive} alive, {meals} meals, {} friendly fire, kills {}/{}", s.elapsed, s.friendly_fire_events, s.wave_kills, s.wave_spawned);
                draws += (alive == 0) as u32;
                if s.stage != Stage::RoundOver || alive > 1 || s.winner.is_some() != (alive == 1) || meals == 0 || s.friendly_fire_events == 0 || s.wave_kills != s.wave_spawned {
                    odd.push(format!("{name} at {hz} Hz: {:?} at {:.1} s, {alive} alive, {meals} meals", s.stage, s.elapsed));
                }
            }
        }
        assert!(odd.is_empty() && draws <= 2, "{draws} rounds without a winner; {odd:#?}");
    }
    // ----- What last hurt a hotdog

    #[test]
    fn whatever_hurts_a_hotdog_is_on_its_record_and_a_dead_hotdogs_record_is_what_killed_it() {
        let near = |a: [f32; 3], b: Vec3| Vec3::from_array(a).distance(b) < 1e-4;
        let s = duel();
        assert!(s.players.iter().all(|p| p.last_damage == Damage::default()));
        // Rounds: the weapon, who fired it, the muzzle it left, the way it flew, when it landed and what it took.
        for (weapon, amount) in [(1usize, 18.), (2, 55.), (3, 4.)] {
            let mut s = duel();
            (s.players[0].owned[weapon], s.players[0].ammo[weapon], s.elapsed) = (true, 5, 7.25);
            assert!(s.fire(1, Vec3::Z, weapon));
            land(&mut s);
            let hit = s.players[1].last_damage;
            assert_eq!((hit.kind as usize, hit.attacker, hit.amount, hit.time), (weapon, 1, amount, 7.25), "weapon {weapon}");
            assert!(near(hit.p, Vec3::new(0., FLOOR + EYE, 2.4)) && near(hit.d, Vec3::Z), "weapon {weapon}: from {:?} along {:?}", hit.p, hit.d);
            assert_eq!(s.players[0].last_damage, Damage::default(), "the shooter's record is its own");
        }
        // A stimmed victim's record has the doubled amount.
        let mut s = duel();
        s.stim(2);
        s.fire(1, Vec3::Z, 1);
        land(&mut s);
        assert_eq!(s.players[1].last_damage.amount, 36.);
        // A relish glob is on record as coming from where it burst: by a direct hit, along its flight...
        let mut s = duel();
        (s.players[0].owned[4], s.players[0].ammo[4]) = (true, 1);
        s.players[2].p = [0.15, FLOOR, 2.75];
        let aim = (chest(&s.players[1]) - muzzle(&s.players[0])).normalize();
        assert!(s.fire(1, aim, 4));
        land(&mut s);
        let (direct, burst) = (s.players[1].last_damage, Vec3::from_array(s.splats[0].p));
        assert_eq!((direct.kind, direct.attacker, direct.amount), (4, 1, PLAYER_DAMAGE[4]));
        assert!(near(direct.p, burst) && near(direct.d, aim) && burst.distance(chest(&s.players[1])) < 0.09);
        // ...and by its splash, outwards from the burst, with the share of the damage that reached it: by how far it
        // burst from the nearest point of the body.
        let splash = s.players[2].last_damage;
        let at = burst - aim * 0.004;
        let out = chest(&s.players[2]) - at;
        let feet = Vec3::from_array(s.players[2].p);
        let axis = Vec3::new(feet.x, at.y.clamp(feet.y + 0.045, feet.y + 0.15 - 0.045), feet.z);
        let gap = at.distance(axis) - 0.045;
        assert_eq!((splash.kind, splash.attacker), (4, 1));
        assert!(near(splash.p, at) && near(splash.d, out.normalize()), "{splash:?}");
        assert!((splash.amount - PLAYER_DAMAGE[4] * blast(gap, SPLASH[4])).abs() < 1e-3 && splash.amount > 2. && splash.amount < 60., "{} against {}", splash.amount, PLAYER_DAMAGE[4] * blast(gap, SPLASH[4]));
        // A bite: the biter and where it stood. It is half the victim's HP from the side (or the front)...
        let mut s = duel();
        s.players[1].p = [0.1, FLOOR, 2.4];
        assert!(s.melee(1, Vec3::X));
        let bite = s.players[1].last_damage;
        assert_eq!((bite.kind, bite.attacker, bite.amount), (MELEE, 1, 50.));
        assert!(near(bite.p, Vec3::new(0., FLOOR, 2.4)) && near(bite.d, Vec3::X));
        // ...and all of it from behind, which is a backstab on the record, from the same place and the same way.
        let mut s = duel();
        s.players[1].p = [0.1, FLOOR, 2.4];
        s.players[1].yaw = -std::f32::consts::FRAC_PI_2;
        assert!(s.melee(1, Vec3::X));
        let stab = s.players[1].last_damage;
        assert_eq!((stab.kind, stab.attacker, stab.amount, s.players[1].alive), (BACKSTAB, 1, 100., false));
        assert!(near(stab.p, Vec3::new(0., FLOOR, 2.4)) && near(stab.d, Vec3::X));
        // A zombie's bite: where the zombie stood and the way to its victim.
        let mut s = brawl();
        s.tick(1. / 30.);
        let bitten = s.players[0].last_damage;
        assert_eq!((bitten.kind, bitten.attacker, bitten.time), (ZOMBIE, 0, s.elapsed));
        assert!((bitten.amount - 14. / 30.).abs() < 1e-4 && near(bitten.p, Vec3::new(0., FLOOR, 2.52)) && near(bitten.d, Vec3::NEG_Z), "{bitten:?}");
        // Pests: the middle of the swarm. Hunger: where the hotdog was. Neither has a direction.
        let mut s = brawl();
        s.enemies[0].p = [1.5, FLOOR, 3.];
        s.pests.push(Swarm { id: 1, p: [0.1, FLOOR, 2.5], hp: 100., ..Default::default() });
        s.tick(1. / 30.);
        let stung = s.players[0].last_damage;
        assert_eq!((stung.kind, stung.attacker, stung.p, stung.d), (PESTS, 0, [0.1, FLOOR, 2.5], [0.; 3]));
        assert!((stung.amount - STING / 30.).abs() < 1e-4);
        let mut s = duel();
        for p in &mut s.players {
            p.bot = false;
        }
        s.tick(1. / 30.);
        let hungry = s.players[1].last_damage;
        assert_eq!((hungry.kind, hungry.attacker, hungry.p, hungry.d, hungry.time), (HUNGER, 0, s.players[1].p, [0.; 3], s.elapsed));
        assert!((hungry.amount - 2.2 / 30.).abs() < 1e-4);
        // The dead are past hurting: a hotdog killed by a shot keeps that shot on its record through the
        // hunger, the pests and the rounds that follow.
        s.players[1].hp = 10.;
        (s.players[0].owned[2], s.players[0].ammo[2], s.players[0].cooldown) = (true, 5, 0.);
        assert!(s.fire(1, Vec3::Z, 2));
        land(&mut s);
        s.pests.push(Swarm { id: 1, p: s.players[1].p, hp: 100., ..Default::default() });
        s.players[0].cooldown = 0.;
        s.fire(1, Vec3::Z, 1);
        for _ in 0..30 {
            s.tick(1. / 30.);
        }
        let fatal = s.players[1].last_damage;
        assert!(!s.players[1].alive && (fatal.kind, fatal.attacker, fatal.amount) == (2, 1, 10.), "{fatal:?}");
        assert_eq!(s.players[0].last_damage.kind, HUNGER);
        // Carried over the counter's edge (when the edge is open): the fall.
        let mut s = brawl();
        s.enemies.clear();
        s.bodies.config.open_edge = true;
        s.players[0].p = [0., FLOOR, 2.33];
        s.embody();
        s.bodies.get_mut(1).unwrap().apply_impulse([0., 0.1, -0.5]);
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        let fell = s.players[0].last_damage;
        assert!(s.players[0].fallen && !s.players[0].alive, "{:?}", s.players[0].p);
        assert_eq!((fell.kind, fell.attacker, fell.d), (FALL, 0, [0., -1., 0.]));
        assert!(fell.amount > 90. && fell.amount <= 100. && fell.amount.is_finite(), "it took all the HP that was left: {}", fell.amount);
        // A blow nobody is blamed for (`hurt`) blanks the record but for its time and amount.
        let mut s = duel();
        s.fire(1, Vec3::Z, 1);
        land(&mut s);
        s.elapsed = 3.;
        s.hurt(2, 5.);
        assert_eq!(s.players[1].last_damage, Damage { time: 3., amount: 5., ..Default::default() });
        // The record travels in the player's own record, and one from before it existed reads as blank.
        let mut s = duel();
        assert!(s.fire(1, Vec3::Z, 1));
        land(&mut s);
        let sent: Vec<Player> = serde_json::from_value(serde_json::to_value(&s.players).unwrap()).unwrap();
        assert_eq!(sent[1].last_damage, s.players[1].last_damage);
        assert_eq!(serde_json::to_value(&s.players[1]).unwrap()["last_damage"]["kind"], 1);
        let old: Player = serde_json::from_str(r#"{"id": 3, "hp": 12.5}"#).unwrap();
        assert_eq!(old.last_damage, Damage::default());
        assert_eq!([MELEE, TURRET, ZOMBIE, PESTS, HUNGER, FALL], [6, 7, 8, 9, 10, 11]);
        s.reset();
        assert!(s.players.iter().all(|p| p.last_damage == Damage::default()));
    }

    // The pancake-syrup battle rifle (weapon RIFLE): three-round bursts.
    /// Steps a round, collecting every shot hotdog `id` fires with the rifle: its time and its round's velocity.
    fn bursts(s: &mut Feast, id: u32, seconds: f32, dt: f32) -> Vec<(f32, [f32; 3])> {
        let (mut seen, mut next) = (vec![], s.next_shot);
        for _ in 0..(seconds / dt).round() as usize {
            s.tick(dt);
            for shot in s.shots.iter().filter(|shot| shot.id >= next && shot.owner == id && shot.weapon == RIFLE) {
                let v = s.projectiles.iter().find(|q| q.id == shot.projectile).map_or([0.; 3], |q| q.v);
                seen.push((shot.time, v));
            }
            next = s.next_shot;
        }
        seen
    }
    #[test]
    fn the_syrup_rifle_fires_bursts_of_three_rounds_apart_and_a_pause_between_bursts_each_round_climbing_a_little() {
        assert_eq!((VOLLEY, VOLLEY_GAP, CLIMB, DELAY[RIFLE], ROUND_SPEED[RIFLE], RANGE[RIFLE]), (3, 0.1, 0.004, 0.40, 17.5, 4.));
        assert!((DELAY[RIFLE] - (VOLLEY - 1) as f32 * VOLLEY_GAP - 0.20).abs() < 1e-6, "the next burst 0.20 s after the last round");
        assert_eq!((PLAYER_DAMAGE[RIFLE], ZOMBIE_DAMAGE[RIFLE], SPLASH[RIFLE], ROUND_RADIUS[RIFLE]), (12., 18., 0., 0.009));
        assert!(ROUND_SPEED[1] < ROUND_SPEED[RIFLE] && ROUND_SPEED[RIFLE] < ROUND_SPEED[2], "between the mustard pistol and the ketchup sniper");
        for dt in [1. / 30., 1. / 60.] {
            let mut s = Feast::new(false);
            for p in &mut s.players {
                p.bot = false;
            }
            (s.stage, s.wave_spawned) = (Stage::Wave, WAVE);
            let p = &mut s.players[0];
            (p.owned[RIFLE], p.ammo[RIFLE], p.weapon, p.gun, p.p) = (true, 7, RIFLE, RIFLE, [-1.7, FLOOR, 2.3]);
            let t0 = s.elapsed;
            assert!(s.fire(1, Vec3::X, RIFLE));
            assert_eq!(s.players[0].ammo[RIFLE], 6);
            assert!(!s.fire(1, Vec3::X, RIFLE), "not again until the burst and its pause are over");
            let mut seen = vec![(t0, s.projectiles[0].v)];
            seen.extend(bursts(&mut s, 1, 0.25, dt));
            assert_eq!(seen.len(), 3, "one burst of three at dt {dt}");
            assert_eq!(s.players[0].ammo[RIFLE], 4);
            let gaps: Vec<f32> = seen.windows(2).map(|w| w[1].0 - w[0].0).collect();
            assert!(gaps.iter().all(|g| *g > VOLLEY_GAP - dt - 1e-5 && *g < VOLLEY_GAP + dt + 1e-5) && (gaps[0] + gaps[1] - 2. * VOLLEY_GAP).abs() < dt + 1e-5, "rounds {gaps:?} s apart");
            // Each round after the first climbs CLIMB more, at the round's own speed.
            for (k, (_, v)) in seen.iter().enumerate() {
                let l = (1. + (CLIMB * k as f32).powi(2)).sqrt();
                assert!((Vec3::from_array(*v).length() - ROUND_SPEED[RIFLE]).abs() < 1e-5 && (v[1] - ROUND_SPEED[RIFLE] * CLIMB * k as f32 / l).abs() < 1e-5, "round {k} flies {v:?}");
            }
            // The next burst comes DELAY after the first: not before.
            let wait = DELAY[RIFLE] - (s.elapsed - t0);
            if wait > dt {
                assert!(!s.fire(1, Vec3::X, RIFLE));
                bursts(&mut s, 1, wait - dt / 2., dt);
                assert!(!s.fire(1, Vec3::X, RIFLE));
            }
            bursts(&mut s, 1, dt * 1.5, dt);
            assert!(s.fire(1, Vec3::X, RIFLE));
            // A burst ends early when the magazine runs dry: one round left, one round fired.
            bursts(&mut s, 1, 1., dt);
            (s.players[0].ammo[RIFLE], s.players[0].cooldown) = (1, 0.);
            assert!(s.fire(1, Vec3::X, RIFLE));
            assert!(bursts(&mut s, 1, 0.5, dt).is_empty());
            assert_eq!(s.players[0].ammo[RIFLE], 0);
            assert!(!s.fire(1, Vec3::X, RIFLE));
        }
        // Rounds fly 4 m and no further.
        let mut s = Feast::new(false);
        (s.stage, s.wave_spawned) = (Stage::Wave, WAVE);
        let p = &mut s.players[0];
        (p.owned[RIFLE], p.ammo[RIFLE], p.weapon, p.gun, p.p) = (true, 3, RIFLE, RIFLE, [-1.7, FLOOR, 2.3]);
        s.fire(1, Vec3::X, RIFLE);
        let mut first = None;
        for _ in 0..60 {
            s.tick(1. / 30.);
            first = first.or(s.splats.first().map(|splat| splat.p));
        }
        assert!((first.unwrap()[0] - (-1.7 + RANGE[RIFLE])).abs() < 1e-4, "landed at {:?}", first);
    }
    #[test]
    fn a_syrup_burst_is_three_rounds_0_1_s_apart_climbing_0_004_a_round_and_half_that_aimed_down_the_sights() {
        // At 60 steps a second the rounds leave on the steps the gaps fall on: 0, 0.1 and 0.2 s.
        let fire = |aimed: bool| {
            let mut s = Feast::new(false);
            for p in &mut s.players {
                p.bot = false;
            }
            (s.stage, s.wave_spawned) = (Stage::Wave, WAVE);
            let p = &mut s.players[0];
            (p.owned[RIFLE], p.ammo[RIFLE], p.weapon, p.gun, p.p) = (true, 60, RIFLE, RIFLE, [-1.7, FLOOR, 2.3]);
            s.set_aimed(1, aimed);
            assert_eq!(s.players[0].aimed, aimed);
            let t0 = s.elapsed;
            assert!(s.fire(1, Vec3::X, RIFLE));
            let mut seen = vec![(t0, s.projectiles[0].v)];
            seen.extend(bursts(&mut s, 1, 0.25, 1. / 60.));
            assert_eq!(s.players[0].aimed, aimed, "the aim is kept while it may be");
            seen.into_iter().map(|(t, v)| (t - t0, v[1] / ROUND_SPEED[RIFLE])).collect::<Vec<_>>()
        };
        for (aimed, climb) in [(false, 0.004), (true, 0.002)] {
            let seen = fire(aimed);
            assert_eq!(seen.len(), 3);
            for (k, (t, rise)) in seen.iter().enumerate() {
                assert!((t - 0.1 * k as f32).abs() < 1e-3, "round {k} at {t} s");
                let want = climb * k as f32 / (1. + (climb * k as f32).powi(2)).sqrt();
                assert!((rise - want).abs() < 1e-6, "round {k} rises {rise}, not {want} (aimed {aimed})");
            }
        }
        // The next burst follows 0.20 s after the last round, and not before.
        let mut s = duel();
        let p = &mut s.players[0];
        (p.owned[RIFLE], p.ammo[RIFLE], p.weapon, p.gun) = (true, 60, RIFLE, RIFLE);
        assert!(s.fire(1, Vec3::Z, RIFLE));
        bursts(&mut s, 1, 0.20 + 0.18, 1. / 60.);
        assert!(!s.fire(1, Vec3::Z, RIFLE), "0.18 s after the last round");
        bursts(&mut s, 1, 0.03, 1. / 60.);
        assert!(s.fire(1, Vec3::Z, RIFLE), "0.21 s after the last round");
        // Only a person's living hotdog with the rifle or the sniper in its free hands aims: not with the pistol, nor a
        // bot, nor the dead; and putting the rifle away lets go of the aim.
        s.set_aimed(1, true);
        assert!(s.players[0].aimed);
        s.select(1, 1);
        s.tick(1. / 60.);
        assert!(!s.players[0].aimed, "the pistol has no sights to aim down");
        s.set_aimed(1, true);
        assert!(!s.players[0].aimed);
        s.select(1, RIFLE);
        s.players[0].bot = true;
        s.set_aimed(1, true);
        assert!(!s.players[0].aimed, "a bot never aims");
        s.players[0].bot = false;
        s.set_aimed(1, true);
        s.players[0].alive = false;
        s.tick(1. / 60.);
        assert!(!s.players[0].aimed, "nor the dead");
    }
    #[test]
    fn a_syrup_burst_hurts_a_rival_12_a_round_and_a_zombie_18_is_on_the_record_as_syrup_and_another_gun_ends_it() {
        let mut s = duel();
        let p = &mut s.players[0];
        (p.owned[RIFLE], p.ammo[RIFLE], p.weapon, p.gun) = (true, 60, RIFLE, RIFLE);
        assert!(s.fire(1, Vec3::Z, RIFLE));
        let mut record = None;
        for _ in 0..20 {
            let before = s.players[1].hp;
            s.flight(1. / 30.);
            if s.players[1].hp < before {
                record = Some(s.players[1].last_damage);
            }
            s.tick(1. / 30.);
        }
        let hunger = 2.2 * (s.elapsed - s.famine_at);
        assert!((100. - hunger - s.players[1].hp - 3. * PLAYER_DAMAGE[RIFLE]).abs() < 1e-3, "{} HP from the burst", 100. - hunger - s.players[1].hp);
        let record = record.unwrap();
        assert_eq!((record.kind, record.attacker, record.amount), (SYRUP, 1, PLAYER_DAMAGE[RIFLE]));
        let mut s = brawl();
        s.enemies[0].p = [0., FLOOR, 2.9];
        let p = &mut s.players[0];
        (p.owned[RIFLE], p.ammo[RIFLE], p.weapon, p.gun) = (true, 60, RIFLE, RIFLE);
        s.embody();
        s.fire(1, Vec3::Z, RIFLE);
        for _ in 0..20 {
            s.tick(1. / 30.);
        }
        assert_eq!(s.enemies[0].hp, 70. - 3. * ZOMBIE_DAMAGE[RIFLE]);
        // The pistol drawn mid-burst: the rest of the burst is not fired.
        let mut s = duel();
        let p = &mut s.players[0];
        (p.owned[RIFLE], p.ammo[RIFLE], p.weapon, p.gun) = (true, 60, RIFLE, RIFLE);
        s.fire(1, Vec3::Z, RIFLE);
        s.select(1, 1);
        assert!(bursts(&mut s, 1, 0.5, 1. / 30.).is_empty());
        assert_eq!(s.players[0].ammo[RIFLE], 59);
    }
    #[test]
    fn a_bot_fires_the_syrup_rifle_within_its_reach_prefers_it_but_for_a_sniper_in_every_gun_mode_and_turns_to_the_pistol_when_it_is_empty() {
        let fired = |s: &mut Feast, ticks: usize| {
            let (mut n, mut next) = ([0u32; 7], s.next_shot);
            for _ in 0..ticks {
                s.tick(1. / 30.);
                for shot in s.shots.iter().filter(|shot| shot.id >= next && shot.owner == 1) {
                    n[shot.weapon] += 1;
                }
                next = s.next_shot;
            }
            n
        };
        let mut s = skirmish(&[(RIFLE, 60), (1, 12)], 1.5, false);
        let n = fired(&mut s, 60);
        assert!(n[RIFLE] >= 3 && n[1] == 0 && s.enemies[0].hp < 1e6 && n[RIFLE] % 3 == 0, "{n:?}");
        // A ketchup sniper (which a bot trades its pistol for) it prefers from 0.8 m out, where one round kills a
        // zombie; inside that, the rifle.
        let mut s = skirmish(&[(RIFLE, 60), (2, 6)], 1.5, false);
        let n = fired(&mut s, 60);
        assert!(n[2] >= 1 && n[RIFLE] == 0, "the sniper at 1.5 m: {n:?}");
        let p = s.players[0].clone();
        assert_eq!([0.5, 0.79, 0.81, 1.5, 3.7, 5.].map(|far| s.arm(&p, far, false)), [RIFLE, RIFLE, 2, 2, 2, 2]);
        // In Forts the same (it once kept to the rifle within the rifle's reach), and in Free For All.
        let map = Map::parse(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/maps/pillow-fort.json")).unwrap()).unwrap();
        let mut forts = Feast::with_map(map, false);
        assert!(forts.fort.is_some());
        assert_eq!([0.5, 0.79, 0.81, 1.5, 3.7, 5.].map(|far| forts.arm(&p, far, false)), [RIFLE, RIFLE, 2, 2, 2, 2]);
        forts.set_rules(crate::rules::Rules::parse("mode=ffa"));
        assert_eq!([0.5, 0.79, 0.81, 1.5, 3.7, 5.].map(|far| forts.arm(&p, far, false)), [RIFLE, RIFLE, 2, 2, 2, 2]);
        let mut s = skirmish(&[(RIFLE, 0), (1, 12)], 1.5, false);
        let n = fired(&mut s, 60);
        assert!(n[1] >= 3 && n[RIFLE] == 0 && s.players[0].weapon == 1, "the pistol once the rifle is empty: {n:?}");
    }

    // Draping (drape.rs). A counter with a stack of two crates in the middle, a third crate and two napkins lying
    // about, and nothing else.
    fn tent() -> Feast {
        let map = Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "name": "Two starts", "bounds": [0, 0, 2, 1],
            "starts": [{"p": [0.2, 0.2], "yaw": 1.5}, {"p": [1.7, 0.8], "yaw": -2}], "spawns": [{"p": [0.1, 0.9]}],
            "props": [{"kind": "crate", "p": [1, 0.5], "base": 0, "yaw": 0}, {"kind": "crate", "p": [1, 0.5], "base": 0.1, "yaw": 0},
            {"kind": "napkin", "p": [0.4, 0.22], "base": 0, "yaw": 0}, {"kind": "napkin", "p": [1.55, 0.25], "base": 0, "yaw": 0},
            {"kind": "crate", "p": [0.4, 0.8], "base": 0, "yaw": 0}]}"#,
        )
        .unwrap();
        let mut s = Feast::with_map(map, false);
        for (i, p) in s.players.iter_mut().enumerate() {
            p.bot = false;
            if i > 0 {
                p.p = [1.9, FLOOR, 0.06 + (i + 1) as f32 * 0.1];
            }
        }
        s.warmup = f32::INFINITY;
        for _ in 0..60 {
            s.tick(1. / 30.);
        }
        s
    }
    /// Turns hotdog 1 to the given way of looking, its yaw and pitch.
    fn gaze(s: &mut Feast, d: [f32; 3]) -> Vec3 {
        let d = Vec3::from_array(d).normalize();
        let p = &mut s.players[0];
        (p.yaw, p.pitch) = ((-d.x).atan2(-d.z), d.y.asin());
        d
    }
    /// Hotdog 1 carries the napkin `id` and puts it down over the stack, from the west, and the round goes on.
    fn cover(s: &mut Feast, id: usize) {
        let at = s.props.position(id);
        s.players[0].p = [at[0] as f32, FLOOR, at[2] as f32 + 0.2];
        let d = gaze(s, [0., -0.8, -0.6]);
        s.embody();
        assert!(s.grab(1, d));
        assert_eq!(s.players[0].carry, id as i32);
        s.players[0].p = [1. - OUT[3] as f32, FLOOR, 0.5];
        let d = gaze(s, [1., 0., 0.]);
        s.embody();
        s.tick(1. / 30.);
        assert!(s.put(1, d));
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
    }
    /// Hotdog 1 walks east for `steps`.
    fn east(s: &mut Feast, steps: usize) {
        for _ in 0..steps {
            s.bodies.get_mut(1).unwrap().wish = [0.67, 0.];
            s.tick(1. / 30.);
        }
    }
    #[test]
    fn a_napkin_put_over_a_stack_of_crates_drapes_and_their_place_is_a_wall_nobody_walks_through_shoves_shoots_through_or_blasts_that_routes_go_round_and_that_can_be_stood_on() {
        use props::State::{Asleep, Awake, Draped};
        let mut s = tent();
        cover(&mut s, 2);
        assert_eq!(s.drapes.len(), 1);
        let d = s.drapes[0].clone();
        assert_eq!((d.napkin, d.props.clone()), (2, vec![0, 1]));
        assert_eq!((0..5).map(|id| s.props.state(id)).collect::<Vec<_>>(), [Draped, Draped, Draped, Asleep, Asleep]);
        assert!(d.rect[0] < 0.951 && d.rect[2] > 1.049 && d.rect[1] < 0.451 && d.rect[3] > 0.549 && d.rect[2] - d.rect[0] < 0.14, "the wall {:?}", d.rect);
        assert!((d.top - (crate::map::FLOOR + 0.2 + 0.008)).abs() < 0.004, "its top {}", d.top);
        let sent: Vec<drape::Drape> = serde_json::from_value(serde_json::to_value(&s.drapes).unwrap()).unwrap();
        assert_eq!(sent, s.drapes);
        let crates = [s.props.position(0), s.props.position(1)];
        // Walked at, it stops a hotdog at the map's margin from its face, as any block does, and nothing in it moves.
        s.players[0].p = [0.6, FLOOR, 0.5];
        s.embody();
        east(&mut s, 60);
        assert!((s.players[0].p[0] as f64 - (d.rect[0] - crate::map::MARGIN)).abs() < 1e-3, "stopped at {}", s.players[0].p[0]);
        // Routes go round it, and a zombie on the far side comes round it to the hotdog, never through it.
        assert!(s.level.path([0.7, 0.5], [1.3, 0.5]).len() > 1);
        (s.stage, s.wave_spawned) = (Stage::Wave, WAVE);
        s.enemies = vec![Zombie { id: 1, p: [1.3, FLOOR, 0.5], yaw: 0., hp: 1e6, born: 0., forming: 0. }];
        for p in &mut s.players {
            p.hp = 1e9;
        }
        for _ in 0..150 {
            s.tick(1. / 30.);
            let [x, _, z] = s.enemies[0].p.map(|v| v as f64);
            assert!(!(x > d.rect[0] - 0.03 && x < d.rect[2] + 0.03 && z > d.rect[1] - 0.03 && z < d.rect[3] + 0.03), "the cat went through at {x}, {z}");
        }
        assert!(Vec3::from_array(s.enemies[0].p).distance(Vec3::from_array(s.players[0].p)) < 0.2, "it came round");
        s.enemies.clear();
        // A round fired at it ends at its face, and a burst beside it neither moves it nor reaches a hotdog behind it.
        s.players[0].p = [0.6, FLOOR, 0.5];
        s.embody();
        (s.players[0].cooldown, s.players[0].ammo[1]) = (0., 5);
        s.fire(1, Vec3::X, 1);
        let mut splat = None;
        for _ in 0..20 {
            s.tick(1. / 30.);
            splat = splat.or(s.splats.first().map(|x| x.p));
        }
        assert!((splat.unwrap()[0] as f64 - d.rect[0]).abs() < 1e-4, "the round ended at {:?}", splat);
        s.players[1].p = [(d.rect[2] + crate::map::MARGIN + 0.01) as f32, FLOOR, 0.5];
        s.embody();
        s.stage = Stage::Warmup;
        s.pests.clear();
        let before = s.players[1].hp;
        s.burst(&Projectile { owner: 1, weapon: 4, hostile: true, ..Default::default() }, Vec3::new(d.rect[0] as f32 - 0.02, FLOOR + 0.1, 0.5), None, None);
        for _ in 0..30 {
            s.tick(1. / 30.);
        }
        assert_eq!(s.players[1].hp, before, "the wall shields the hotdog behind it");
        assert_eq!([s.props.position(0), s.props.position(1)], crates, "and nothing in it moved");
        // Its top is a floor: a hotdog let down on it stands there.
        let top = d.top as f32;
        s.players[1].p = [1., top + 0.02, 0.5];
        s.embody();
        s.bodies.get_mut(2).unwrap().grounded = false;
        for _ in 0..20 {
            s.tick(1. / 30.);
        }
        assert!((s.players[1].p[1] - (top - 0.001)).abs() < 1e-4 && s.players[1].grounded, "it stands at {} on a top at {top}", s.players[1].p[1]);
        // A person pulls it off with Q at it: the napkin comes into its hands, the wall is gone, and the crates
        // are loose again where they were.
        s.stage = Stage::Wave;
        s.players[0].p = [0.6, FLOOR, 0.5];
        s.embody();
        east(&mut s, 20);
        let d = gaze(&mut s, [1., 0., 0.]);
        assert!(s.grab(1, d));
        assert_eq!((s.players[0].carry, s.drapes.len(), s.props.state(0), s.props.state(1)), (2, 0, Awake, Awake));
        assert_eq!(s.level.path([0.7, 0.5], [1.3, 0.5]).len(), 1, "the way is straight again");
        // Put over the stack again, it drapes again; a bot cannot pull it off.
        s.players[0].p = [1. - OUT[3] as f32, FLOOR, 0.5];
        s.embody();
        s.tick(1. / 30.);
        assert!(s.put(1, d));
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        assert_eq!(s.drapes.len(), 1);
        s.players[0].p = [0.6, FLOOR, 0.5];
        s.embody();
        east(&mut s, 20);
        s.players[0].bot = true;
        let d = gaze(&mut s, [1., 0., 0.]);
        assert!(!s.grab(1, d));
        assert_eq!(s.drapes.len(), 1);
    }
    #[test]
    fn a_napkin_on_the_bare_counter_just_lies_there_one_over_a_single_crate_drapes_a_low_wall_and_a_new_round_takes_every_drape_away() {
        let mut s = tent();
        s.players[0].p = [0.4, FLOOR, 0.45];
        let d = gaze(&mut s, [0., -0.8, -0.6]);
        s.embody();
        assert!(s.grab(1, d));
        s.players[0].p = [0.3, FLOOR, 0.25];
        let d = gaze(&mut s, [1., 0., 0.]);
        s.embody();
        s.tick(1. / 30.);
        assert!(s.put(1, d));
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        assert_eq!((s.drapes.len(), s.props.state(2)), (0, props::State::Locked));
        assert!((s.props.position(2)[1] - crate::map::FLOOR).abs() < 0.003);
        // Over the single crate at [0.4, 0.8]: a wall a little over 0.1 m high.
        s.players[0].p = [s.props.position(2)[0] as f32, FLOOR, 0.48];
        let d = gaze(&mut s, [0., -0.8, -0.6]);
        s.embody();
        for _ in 0..5 {
            s.tick(1. / 30.);
        }
        assert!(s.grab(1, d));
        s.players[0].p = [0.4, FLOOR, 0.8 - OUT[3] as f32];
        let d = gaze(&mut s, [0., 0., 1.]);
        s.embody();
        s.tick(1. / 30.);
        assert!(s.put(1, d));
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        assert_eq!(s.drapes.len(), 1);
        assert_eq!(s.drapes[0].props, [4]);
        assert!((s.drapes[0].top - (crate::map::FLOOR + 0.108)).abs() < 0.004);
        s.reset();
        assert_eq!((s.drapes.len(), s.level.path([0.2, 0.8], [0.6, 0.8]).len()), (0, 1));
        assert!((0..s.props.len()).all(|id| s.props.state(id) != props::State::Draped));
    }

    // ----- Fully Loaded (cards.rs) as a rule of the game.

    /// A Feast on the bundled map (Counter Siege) played as Fully Loaded.
    fn table(demo: bool) -> Feast {
        let mut s = Feast::with_map(Map::bundled(), demo);
        s.set_rules(crate::rules::Rules { cards: true, ..Default::default() });
        s
    }
    /// The angle from `a` to `b`, wrapped to -PI..PI.
    fn turn_between(a: f32, b: f32) -> f32 {
        (a - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
    }
    /// The yaw that faces from `from` to `to` ([x, z]): a hotdog looks along (-sin yaw, -cos yaw).
    fn facing(from: [f32; 2], to: [f32; 2]) -> f32 {
        (-(to[0] - from[0])).atan2(-(to[1] - from[1]))
    }

    #[test]
    fn a_cards_round_stands_the_hotdogs_round_the_microwave() {
        let s = table(true);
        // Counter Siege's microwave: [-1.46, 3.1, -0.96, 3.43], 0.25 m high, its front on the -z side.
        let microwave = s.level.map.blocks.iter().find(|b| b.style == "microwave").expect("the bundled map has a microwave");
        assert_eq!((microwave.rect, microwave.height), ([-1.46, 3.1, -0.96, 3.43], 0.25));
        let rect = [-1.46_f32, 3.1, -0.96, 3.43];
        let centre = [(rect[0] + rect[2]) / 2., (rect[1] + rect[3]) / 2.];
        assert_eq!((s.players.len(), s.stage, s.alert.as_str()), (5, Stage::Warmup, ""));
        assert!(s.cards.is_some());
        for p in &s.players {
            // On top of it (0.25 m above the counter, and the lip that bodies stand on), inside its edges, with its body there too.
            assert!(p.p[0] > rect[0] && p.p[0] < rect[2] && p.p[2] > rect[1] && p.p[2] < rect[3], "{} at {:?}", p.name, p.p);
            assert!((p.p[1] - (FLOOR + 0.25 + crate::map::LIP as f32)).abs() < 1e-4, "{} stands at {}", p.name, p.p[1]);
            let body = s.body(p.id).expect("a body at the table").position();
            assert!(body.distance(Vec3::from_array(p.p)) < 1e-4, "{} body {body:?}", p.name);
            // Facing the middle of the top.
            let want = facing([p.p[0], p.p[2]], centre);
            assert!(turn_between(p.yaw, want).abs() < 0.05, "{} faces {} not {want}", p.name, p.yaw);
        }
        // Spread out: no two bodies touch (a hotdog's is 0.045 m wide).
        for (a, p) in s.players.iter().enumerate() {
            for q in &s.players[a + 1..] {
                assert!(Vec3::from_array(p.p).distance(Vec3::from_array(q.p)) >= 0.07, "{} and {}", p.name, q.name);
            }
        }
        // The first seat is at the front (the -z side, in the middle); the seats go round from it, all different.
        assert!((s.players[0].p[0] - centre[0]).abs() < 1e-4 && s.players[0].p[2] < centre[1] - 0.1, "{:?}", s.players[0].p);
        // `table_seats` says the same.
        let seats = table_seats(&s.level, 5);
        assert_eq!(seats.len(), 5);
        for (p, (at, yaw)) in s.players.iter().zip(&seats) {
            assert_eq!((p.p, p.yaw), (*at, *yaw));
        }
        assert!(table_seats(&s.level, 0).is_empty());
        // With a different number of seats they are still on the top and apart (a Forts-sized six, and a lone one).
        for n in [1, 2, 6] {
            let seats = table_seats(&s.level, n);
            assert_eq!(seats.len(), n);
            for (a, (p, _)) in seats.iter().enumerate() {
                assert!(p[0] > rect[0] && p[0] < rect[2] && p[2] > rect[1] && p[2] < rect[3]);
                assert!(seats[a + 1..].iter().all(|(q, _)| Vec3::from_array(*p).distance(Vec3::from_array(*q)) >= 0.07));
            }
        }
    }

    #[test]
    fn a_table_with_no_microwave_is_laid_on_the_first_start() {
        let mut map = Map::bundled();
        map.blocks.retain(|b| b.style != "microwave");
        let level = Level::new(map);
        let (start, _) = level.start(0);
        let seats = table_seats(&level, 5);
        assert_eq!(seats.len(), 5);
        for (p, yaw) in &seats {
            // Within the 0.5 x 0.33 m top about the start, on the ground there, and facing its middle.
            assert!((p[0] - start.x).abs() < 0.25 && (p[2] - start.z).abs() < 0.165, "{p:?} about {start:?}");
            assert!((p[1] - level.ground_at(p[0], p[2])).abs() < 1e-4);
            assert!(turn_between(*yaw, facing([p[0], p[2]], [start.x, start.z])).abs() < 0.05);
        }
    }

    #[test]
    fn a_cards_round_has_no_wave_hunger_or_fight_and_ends_with_a_winner() {
        let mut s = table(true);
        let hp: Vec<f32> = s.players.iter().map(|p| p.hp).collect();
        let mut over = false;
        for _ in 0..(3600 * 30) {
            s.tick(1. / 30.);
            // No cats, no famine, no fight: the stage stays at the start until the game is won.
            assert_eq!((s.enemies.len(), s.wave_spawned), (0, 0));
            assert_eq!(s.players.iter().map(|p| p.hp).collect::<Vec<_>>(), hp);
            if s.stage == Stage::RoundOver {
                over = true;
                break;
            }
            assert_eq!(s.stage, Stage::Warmup);
        }
        assert!(over, "nobody won in an hour");
        let (seat, total) = {
            let t = s.cards.as_ref().expect("the table");
            (t.winner().expect("the table has a winner"), t.view().seats[t.winner().unwrap()].total)
        };
        assert!(total >= cards::TARGET);
        assert_eq!(s.winner, Some(s.players[seat].id));
        assert_eq!(s.alert, format!("{} IS FULLY LOADED: {total} POINTS", s.players[seat].name.to_uppercase()));
        // The winner is shown at the table (the bodies stand on the microwave through the pause).
        for _ in 0..60 {
            s.tick(1. / 30.);
            for p in &s.players {
                assert!(p.p[0] > -1.46 && p.p[0] < -0.96 && p.p[2] > 3.1 && p.p[2] < 3.43 && (p.p[1] - (FLOOR + 0.25 + crate::map::LIP as f32)).abs() < 1e-3, "{} at {:?}", p.name, p.p);
            }
        }
        // Eight seconds on, the next round: a fresh table, dealt from its first tick.
        let (round, mut steps) = (s.round, 60);
        while s.round == round {
            s.tick(1. / 30.);
            steps += 1;
            assert!(steps < 30 * 10, "the next round did not come");
        }
        // (A new round's clock starts again: count the steps from the table's win.)
        assert!((239..=242).contains(&steps), "{steps} steps of 1/30 s from the win");
        assert_eq!((s.round, s.stage, s.winner, s.alert.as_str()), (round + 1, Stage::Warmup, None, ""));
        let t = s.cards.as_ref().expect("a table for the next round");
        assert!(t.view().hand <= 1 && t.winner().is_none());
        assert!(t.view().seats.iter().all(|seat| seat.total == 0));
        // ... and it goes on from there.
        for _ in 0..30 {
            s.tick(1. / 30.);
        }
        assert_eq!(s.cards.as_ref().unwrap().view().hand, 1);
    }

    #[test]
    fn walking_and_firing_do_nothing_at_the_table() {
        // (Hotdog 1 is a person; the rest are bots.)
        let mut s = table(false);
        s.tick(0.5);
        let (feet, ammo) = (s.players.iter().map(|p| p.p).collect::<Vec<_>>(), s.players.iter().map(|p| p.ammo).collect::<Vec<_>>());
        let ahead = Vec3::new(0., 0., -1.);
        assert_eq!(s.walk(1, Vec2::new(0.05, 0.), 1.), None);
        assert_eq!(s.walk(2, Vec2::new(0., -0.05), 1.), None);
        s.jump(1, 2.);
        s.step_body(1, 0.1);
        assert!(!s.fire(1, ahead, 1) && !s.fire(2, ahead, 1) && !s.melee(1, ahead) && !s.toss(1, ahead));
        assert!(!s.grab(1, ahead) && !s.put(1, ahead) && !s.stim(1) && !s.eat(1) && !s.bite(1));
        for item in 0..s.pickups.len() {
            assert!(!s.pickup(1, item));
        }
        for _ in 0..30 {
            s.tick(1. / 30.);
        }
        assert!(s.projectiles.is_empty() && s.shots.is_empty() && s.splats.is_empty());
        assert_eq!(s.players.iter().map(|p| p.p).collect::<Vec<_>>(), feet);
        assert_eq!(s.players.iter().map(|p| p.ammo).collect::<Vec<_>>(), ammo);
        for (p, at) in s.players.iter().zip(&feet) {
            assert!(s.body(p.id).unwrap().position().distance(Vec3::from_array(*at)) < 1e-4);
        }
        // The same moves do work in a siege: the refusals above are the table's.
        let mut siege = Feast::new(false);
        assert!(siege.walk(1, Vec2::new(0.01, 0.), 1.).is_some());
        assert!(siege.fire(1, ahead, 1));
        assert!(!siege.projectiles.is_empty());
    }

    /// Plays on until it is the person's turn (hotdog 1 is seat 0, and the only person), picking a hotdog for any
    /// action card they are dealt on the way (themselves), and checks the clock at each of their decisions, on the
    /// first step of it: running when following, and not at all otherwise.
    fn up_to_their_turn(s: &mut Feast, following: bool) {
        for _ in 0..(120 * 30) {
            s.tick(1. / 30.);
            let v = s.cards.as_ref().unwrap().view();
            let picking = v.choosing.is_some_and(|(seat, _)| seat == 0);
            if picking || (v.turn == Some(0) && !v.between) {
                if following {
                    let clock = v.clock.expect("a clock on a person's decision");
                    assert!(14. < clock && clock <= cards::TURN, "{clock}");
                } else {
                    assert_eq!(v.clock, None);
                }
            }
            if picking {
                assert!(s.cards_command(1, cards::Command::Target(0)));
            } else if v.turn == Some(0) && !v.between {
                return;
            }
        }
        panic!("the person never had a turn");
    }

    #[test]
    fn a_person_at_the_table_has_no_time_limit_offline_and_fifteen_seconds_when_following() {
        // Following (online): 15 s, and then they stop.
        let mut s = Feast::with_map(Map::bundled(), false);
        s.follow = true;
        s.set_rules(crate::rules::Rules { cards: true, ..Default::default() });
        assert!(!s.players[0].bot && cards::TURN == 15.);
        up_to_their_turn(&mut s, true);
        let (asked_at, clock) = (s.elapsed, s.cards.as_ref().unwrap().view().clock.unwrap());
        assert_eq!(s.cards.as_ref().unwrap().view().seats[0].status, cards::Status::In);
        while s.cards.as_ref().unwrap().view().seats[0].status == cards::Status::In {
            s.tick(1. / 30.);
            assert!(s.elapsed - asked_at < 16., "still in after {} s", s.elapsed - asked_at);
        }
        assert!((s.elapsed - asked_at - clock).abs() < 0.1, "stopped after {} s of {clock}", s.elapsed - asked_at);
        // Offline: no clock, and the table waits as long as they like.
        let mut s = table(false);
        up_to_their_turn(&mut s, false);
        let v = s.cards.as_ref().unwrap().view();
        let version = s.cards.as_ref().unwrap().version();
        for _ in 0..(60 * 30) {
            s.tick(1. / 30.);
        }
        let t = s.cards.as_ref().unwrap();
        assert_eq!((t.view().clock, t.version(), t.view().seats[0].status), (None, version, v.seats[0].status));
        assert_eq!((t.view().turn, s.stage), (Some(0), Stage::Warmup));
        // They answer, and play goes on.
        assert!(s.cards_command(1, cards::Command::Stop));
        assert_ne!(s.cards.as_ref().unwrap().version(), version);
    }

    #[test]
    fn a_cards_command_goes_to_the_seat_of_the_player_who_sent_it() {
        // Hotdog 1 is a person (seat 0); the rest are bots, and nobody is on a clock.
        let mut s = table(false);
        let mut picked = false;
        for _ in 0..(120 * 30) {
            s.tick(1. / 30.);
            let (v, version) = (s.cards.as_ref().unwrap().view(), s.cards.as_ref().unwrap().version());
            if v.choosing.is_some_and(|(seat, _)| seat == 0) {
                // Picking a hotdog for an action card they dealt: a flip or a stop makes no sense, nor does a hotdog that
                // is not there, nor does anybody else's say; theirs does.
                assert!(!s.cards_command(1, cards::Command::Flip) && !s.cards_command(1, cards::Command::Stop));
                assert!(!s.cards_command(1, cards::Command::Target(99)) && !s.cards_command(2, cards::Command::Target(0)));
                assert_eq!(s.cards.as_ref().unwrap().version(), version);
                assert!(s.cards_command(1, cards::Command::Target(0)));
                picked = true;
            } else if v.turn == Some(0) && v.choosing.is_none() && !v.between {
                break;
            }
        }
        let v = s.cards.as_ref().unwrap().view();
        assert_eq!((v.turn, v.choosing), (Some(0), None), "never reached its go (picked: {picked})");
        // An id nobody has, another hotdog's (not its turn), and a command that makes no sense now: refused.
        let version = s.cards.as_ref().unwrap().version();
        assert!(!s.cards_command(99, cards::Command::Stop));
        assert!(!s.cards_command(0, cards::Command::Stop));
        assert!(!s.cards_command(2, cards::Command::Stop));
        assert!(!s.cards_command(1, cards::Command::Target(1)));
        assert!(!s.cards_command(1, cards::Command::Target(99)));
        assert_eq!(s.cards.as_ref().unwrap().version(), version);
        // Hotdog 1 is seat 0: its stop is taken.
        assert!(s.cards_command(1, cards::Command::Stop));
        assert_eq!(s.cards.as_ref().unwrap().view().seats[0].status, cards::Status::Stopped);
        // Not at a table, nothing is taken.
        assert!(!Feast::new(false).cards_command(1, cards::Command::Stop));
    }

    #[test]
    fn the_table_is_dealt_from_the_cards_seed_and_the_round_and_keeps_the_seed_between_rounds() {
        let hands = |seed: u64| {
            let mut s = Feast::with_map(Map::bundled(), true);
            s.card_seed = seed;
            s.set_rules(crate::rules::Rules { cards: true, ..Default::default() });
            for _ in 0..90 {
                s.tick(1. / 30.);
            }
            s.cards.as_ref().unwrap().view().seats.into_iter().map(|seat| seat.cards).collect::<Vec<_>>()
        };
        assert_eq!(hands(0), hands(0));
        assert_eq!(hands(7), hands(7));
        assert_ne!(hands(0), hands(7));
        // The next round keeps the seed, and (the round being part of the table's seed) deals another game.
        let mut s = Feast::with_map(Map::bundled(), true);
        s.card_seed = 7;
        s.set_rules(crate::rules::Rules { cards: true, ..Default::default() });
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        let first = s.cards.as_ref().unwrap().view().seats.into_iter().map(|seat| seat.cards).collect::<Vec<_>>();
        s.reset();
        assert_eq!((s.card_seed, s.round), (7, 2));
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        let second = s.cards.as_ref().unwrap().view().seats.into_iter().map(|seat| seat.cards).collect::<Vec<_>>();
        assert_ne!(first, second);
    }

    #[test]
    fn a_hotdog_a_bot_takes_over_mid_turn_plays_at_once_and_a_hotdog_fitted_out_keeps_its_seat() {
        let mut s = table(false);
        up_to_their_turn(&mut s, false);
        let (version, seats) = (s.cards.as_ref().unwrap().version(), table_seats(&s.level, 5));
        // They leave, and a bot takes the hotdog: the table moves on within a bot's think time, not never.
        s.players[0].bot = true;
        s.fit(1);
        assert_eq!((s.players[0].p, s.players[0].yaw), seats[0]);
        assert_eq!(s.body(1).unwrap().position().to_array(), seats[0].0);
        for _ in 0..((cards::THINK[1] * 30.).ceil() as usize + 2) {
            s.tick(1. / 30.);
        }
        let t = s.cards.as_ref().unwrap();
        assert!(t.view().seats[0].bot);
        assert_ne!(t.version(), version, "the bot did not play");
    }

    #[test]
    fn a_forts_map_played_as_fully_loaded_is_a_table_of_five_and_a_hotdog_keeps_to_it() {
        let map = Map::parse(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/maps/pillow-fort.json")).unwrap()).unwrap();
        let mut s = Feast::with_map(map, false);
        assert!(s.fort.is_some() && s.players.len() == 6);
        s.set_rules(crate::rules::Rules { cards: true, ..Default::default() });
        assert!(s.fort.is_none() && s.players.len() == 5);
        assert_eq!(s.cards.as_ref().map(|t| t.view().seats.len()), Some(5));
        let seats = table_seats(&s.level, 5);
        for _ in 0..90 {
            s.tick(1. / 30.);
        }
        assert_eq!(s.players.iter().map(|p| (p.p, p.yaw)).collect::<Vec<_>>(), seats);
        assert_eq!(s.stage, Stage::Warmup);
    }

    #[test]
    fn other_modes_have_no_table() {
        let mut s = Feast::new(false);
        assert!(s.cards.is_none() && s.card_seed == 0);
        s.set_rules(crate::rules::Rules { ffa: true, ..Default::default() });
        assert!(s.cards.is_none());
        s.reset();
        assert!(s.cards.is_none());
        // Back from a table to a siege: no table and its usual rules.
        let mut s = table(true);
        s.set_rules(Default::default());
        assert!(s.cards.is_none());
        s.reset();
        assert!(s.cards.is_none() && s.rules == Default::default());
    }
}
#[path = "build.rs"]
pub mod build;
pub use build::Placement;
#[cfg(test)]
#[path = "mayo_tests.rs"]
mod mayo_tests;
#[cfg(test)]
#[path = "combat_tests.rs"]
mod combat_tests;
#[cfg(test)]
#[path = "melee_tests.rs"]
mod melee_tests;
#[cfg(test)]
#[path = "aim_tests.rs"]
mod aim_tests;
#[cfg(test)]
#[path = "head_tests.rs"]
mod head_tests;

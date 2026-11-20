// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The melee strike, the pistol whip: F with whatever gun is in hand (a pistol whip, a rifle butt, a swing of the
//! bazooka's tube...). The simulation decides what the blow does (`Feast::melee`: a backstab kills, anything else
//! takes half of the most HP there is); this is how it looks, sounds and feels, in first and third person.
//!
//! * **One timeline** for both views (`between`): a wind-up in which the gun rolls over and lifts, a fast snap whose
//!   butt or grip leads, contact, a follow-through and a smooth recovery, TOTAL seconds in all. When a blow connects
//!   the swing freezes for STOP seconds on contact (STOP_STAB for a backstab), a hit-stop of two or three frames.
//! * **First person** (`Swing`, in `Game::whip`): the gun's pose (`pose`), the camera's lean into the blow, its kick
//!   when it lands and the shake that follows (`Swing::camera`), a smear of light along the arc the gun sweeps
//!   (`ribbon`), the whoosh of the swing and the thwack of the blow.
//! * **Where it lands**, for everybody to see: a flash and sauce droplets at the victim (a slash glint too for a
//!   backstab), found from what the simulation (or the server) says happened (`observe`: a hotdog's record of what
//!   last hurt it, a zombie's HP), so that it is the same offline, online, for the other players' blows and for bots'.
//! * **Third person** (player_model.rs): `Fx::struck` tells a hotdog's swing whether it connected, so that it holds on
//!   contact as the first-person one does; the arc itself is player_model's `strike`, on this timeline.
//! * **The backstab's mark**: the head-up display's dagger (icons.rs `Icon::Backstab`) beside the crosshair, for the
//!   player who struck or was struck; no words.
use crate::arena::{Game, Weapon};
use bevy::{
    asset::RenderAssetUsages,
    audio::Volume,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    light::NotShadowCaster,
    mesh::Indices,
    prelude::*,
    render::render_resource::PrimitiveTopology,
};
use kitchen_sim::feast;
use std::collections::HashMap;
use std::f32::consts::PI;

/// Seconds the swing takes on its own clock, without the hit-stop: the whole strike, wind-up to rest, is 0.445 s with one
/// (0.485 for a backstab's; the simulation makes the next blow wait 0.48 s, `feast::DELAY[0]`).
pub const TOTAL: f32 = 0.39;
/// Where in 0..1 of the swing the gun is fully wound, makes contact, and has followed through. Wind-up and recovery
/// take the rest: the snap is the 0.19 between wound and contact, which is 0.074 s.
pub const WOUND: f32 = 0.26;
pub const CONTACT: f32 = 0.45;
pub const FOLLOWED: f32 = 0.62;
/// Seconds the swing stands still on contact when it lands (a hit-stop of three frames at 60 Hz), and for a backstab.
pub const STOP: f32 = 0.055;
pub const STOP_STAB: f32 = 0.095;
/// Seconds a backstab's mark stays at the crosshair.
const MARK: f32 = 1.5;

/// A beat of the swing: at rest, wound, at the blow, followed through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Rest,
    Cocked,
    Blow,
    Follow,
}
/// The two beats a swing at `u` in 0..1 is between, and how far along it is from the first to the second, eased: the
/// wind-up decelerates into the cocked pose, the snap accelerates all the way to contact, the follow-through is thrown
/// out and settles, the recovery eases both ways. At rest (`u` outside 0..1) both are `Rest`.
pub fn between(u: f32) -> (Key, Key, f32) {
    let ease = |t: f32, power: f32| 1. - (1. - t.clamp(0., 1.)).powf(power);
    if u <= 0. || u >= 1. {
        (Key::Rest, Key::Rest, 0.)
    } else if u < WOUND {
        (Key::Rest, Key::Cocked, ease(u / WOUND, 3.))
    } else if u < CONTACT {
        (Key::Cocked, Key::Blow, ((u - WOUND) / (CONTACT - WOUND)).powf(2.3))
    } else if u < FOLLOWED {
        (Key::Blow, Key::Follow, ease((u - CONTACT) / (FOLLOWED - CONTACT), 2.5))
    } else {
        (Key::Follow, Key::Rest, smooth((u - FOLLOWED) / (1. - FOLLOWED)))
    }
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

// ----- The gun in the first-person view

type Pose = (Vec3, Quat);
const REST: Pose = (Vec3::ZERO, Quat::IDENTITY);
/// One beat of a gun's swing: where the whole viewmodel is moved to from its grip position (right, up, back) and how it
/// is turned (about the up axis, the right axis and the view axis: pitch is the muzzle up, roll tips the top to the
/// right). The hands that hold the gun hang from the same root and go with it, so the turns are kept to what an arm
/// could follow.
fn key(x: f32, y: f32, z: f32, yaw: f32, pitch: f32, roll: f32) -> Pose {
    (Vec3::new(x, y, z), Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * Quat::from_rotation_z(roll))
}
/// A gun's three beats and how its path bulges between the wound pose and the blow (a sweep, not a straight line).
struct Keys {
    cocked: Pose,
    blow: Pose,
    follow: Pose,
    arc: Vec3,
}
/// Each gun is swung its own way, with its stock or its grip leading (there are no bare hands): the pistol is rolled over
/// and raised, then hammered down and across butt first; the sniper is tipped muzzle-up over the shoulder to drive its
/// stock forward and down; the hot sauce thrower is turned crosswise and shoved; the bazooka's tube is swung across the
/// view like a bat; the spray can is jabbed straight out; the syrup rifle is swung round, its muzzle going back over the
/// left shoulder, so that its butt is driven into the middle of the view; the mayonnaise bottle is raised and brought
/// down bottom first, a bonk. (The rolls are kept to what the forearms hanging from the gun can follow without
/// sweeping across the whole view.)
fn keys(weapon: Weapon) -> Option<Keys> {
    Some(match weapon {
        Weapon::Laden => return None,
        Weapon::Pistol => Keys {
            cocked: key(0.050, 0.052, 0.032, -0.30, 0.90, -0.95),
            blow: key(-0.045, 0.018, -0.012, 0.45, 1.45, 0.25),
            follow: key(-0.090, -0.020, -0.005, 0.75, 1.70, 0.55),
            arc: Vec3::new(0.020, 0.040, -0.020),
        },
        Weapon::Sniper => Keys {
            cocked: key(0.035, 0.036, 0.035, 0.10, 0.60, -0.55),
            blow: key(-0.030, 0.012, -0.040, -0.20, 1.20, -0.10),
            follow: key(-0.050, -0.015, -0.030, -0.30, 1.35, 0.05),
            arc: Vec3::new(0.020, 0.030, 0.),
        },
        Weapon::Flame => Keys {
            cocked: key(-0.010, 0.030, 0.040, 0.95, 0.10, -0.70),
            blow: key(-0.030, 0.012, -0.060, 0.70, 0.05, 0.10),
            follow: key(-0.036, 0.006, -0.045, 0.68, 0., 0.15),
            arc: Vec3::new(-0.010, 0.020, 0.),
        },
        Weapon::Bazooka => Keys {
            cocked: key(0.060, 0.020, 0.035, -0.85, 0.20, -0.50),
            blow: key(-0.040, -0.004, -0.030, 0.55, 0., 0.15),
            follow: key(-0.080, -0.016, -0.010, 0.85, -0.10, 0.25),
            arc: Vec3::new(0., 0.015, -0.020),
        },
        Weapon::Spray => Keys {
            cocked: key(0.015, 0.030, 0.040, 0.10, 0.65, -0.60),
            blow: key(-0.025, 0.012, -0.060, 0.10, -0.15, 0.),
            follow: key(-0.028, 0.008, -0.048, 0.10, -0.20, 0.),
            arc: Vec3::new(0.010, 0.030, 0.),
        },
        Weapon::Rifle => Keys {
            cocked: key(0.050, 0.035, 0.030, 0.50, 0.50, -0.60),
            blow: key(-0.060, 0.000, -0.035, 1.80, 0.35, 0.15),
            follow: key(-0.085, -0.015, -0.025, 2.10, 0.50, 0.25),
            arc: Vec3::new(0.020, 0.030, -0.010),
        },
        Weapon::Mayo => Keys {
            cocked: key(0.020, 0.065, 0.030, 0., 0.10, -0.95),
            blow: key(-0.020, -0.015, -0.030, 0.10, 1.15, -0.10),
            follow: key(-0.026, -0.026, -0.020, 0.10, 1.30, -0.12),
            arc: Vec3::new(0.015, 0.030, 0.),
        },
    })
}
/// Where the whole viewmodel is moved to from its grip position, and how it is turned, `u` of the way through the swing
/// (0 to 1; outside it the gun is at rest). Never a jump.
pub fn pose(weapon: Weapon, u: f32) -> Pose {
    let Some(k) = keys(weapon) else {
        return REST;
    };
    let (from, to, t) = between(u);
    let at = |key: Key| match key {
        Key::Rest => REST,
        Key::Cocked => k.cocked,
        Key::Blow => k.blow,
        Key::Follow => k.follow,
    };
    let (a, b) = (at(from), at(to));
    let bulge = if (from, to) == (Key::Cocked, Key::Blow) { k.arc * (PI * t).sin() } else { Vec3::ZERO };
    (a.0.lerp(b.0, t) + bulge, a.1.slerp(b.1, t))
}

// ----- This player's swing

/// A blow that landed: who struck, which of its swings it was (`Player::swings`), where, along what, whether it came from
/// behind and killed, and when (the real clock).
#[derive(Clone, Debug, PartialEq)]
pub struct Blow {
    pub attacker: u32,
    /// Who was struck: a hotdog's id, or 0 for a zombie.
    pub victim: u32,
    pub swings: u32,
    pub at: Vec3,
    pub along: Vec3,
    pub backstab: bool,
    pub kill: bool,
    pub time: f32,
}
/// What a swing says to the rest of the game once a frame: it has begun (to be heard), or it has landed.
#[derive(Clone, Debug, Default)]
pub struct Cues {
    pub swung: bool,
    pub landed: Option<Blow>,
}
/// The first-person strike: its clock, the hit-stop, and the camera's kick. `Game::whip`.
#[derive(Clone, Debug)]
pub struct Swing {
    active: bool,
    weapon: Weapon,
    /// Seconds into the swing on its own clock: it stands still for the hit-stop.
    t: f32,
    /// Seconds of hit-stop left.
    hold: f32,
    /// The blow that connected (the simulation's word, which may come after the swing has begun), and whether the swing
    /// has felt it yet.
    hit: Option<Blow>,
    felt: bool,
    /// Seconds since contact (a very long time before one); the camera's kick and shake follow it.
    since: f32,
    heavy: bool,
    cues: Cues,
    /// The kick's seed, so that no two shakes are alike.
    seed: f32,
}
impl Swing {
    /// At rest: no swing, no kick.
    pub fn rest() -> Swing {
        Swing { active: false, weapon: Weapon::Laden, t: TOTAL, hold: 0., hit: None, felt: false, since: 1e3, heavy: false, cues: Cues::default(), seed: 0. }
    }
    /// A swing of `weapon` begins now (F).
    pub fn start(&mut self, weapon: Weapon, now: f32) {
        *self = Swing { active: weapon != Weapon::Laden, weapon, t: 0., seed: now, cues: Cues { swung: true, landed: None }, ..Swing::rest() };
    }
    /// How far through the swing it is, 0 to 1 (1 at rest).
    pub fn progress(&self) -> f32 {
        if self.active { self.t / TOTAL } else { 1. }
    }
    /// The swing is under way: the gun is off its hip.
    pub fn moving(&self) -> bool {
        self.active && self.progress() < 1.
    }
    /// Whether the swing is standing still on contact.
    pub fn held(&self) -> bool {
        self.hold > 0.
    }
    /// Whether the blow this swing felt was a backstab (for a heavier kick, and a red streak).
    pub fn heavy(&self) -> bool {
        self.heavy
    }
    /// The simulation (or the server) says a blow of this player's landed: the swing feels it on contact, or at once if it
    /// is already past.
    pub fn connect(&mut self, blow: Blow) {
        if self.active && !self.felt {
            self.hit = Some(blow);
        }
    }
    /// Steps the swing's clock by `dt` seconds. On contact, if a blow has landed, it stands still for the hit-stop.
    pub fn advance(&mut self, dt: f32) {
        self.since += dt;
        if !self.active {
            return;
        }
        if self.hold > 0. {
            self.hold -= dt;
            return;
        }
        self.t += dt;
        if let Some(blow) = self.hit.clone().filter(|_| !self.felt && self.t >= CONTACT * TOTAL) {
            self.felt = true;
            self.heavy = blow.backstab;
            // (A blow told of after the follow-through has begun is felt and shown, but nothing is held for it.)
            if self.t < (CONTACT + 0.12) * TOTAL {
                self.hold = if blow.backstab { STOP_STAB } else { STOP };
                self.t = CONTACT * TOTAL;
            }
            self.since = 0.;
            self.cues.landed = Some(blow);
        }
        if self.t >= TOTAL {
            self.active = false;
        }
    }
    /// What the swing has to say this frame, once.
    pub fn take(&mut self) -> Cues {
        std::mem::take(&mut self.cues)
    }
    /// The viewmodel's pose now: `pose` of the swing's progress, for the gun in hand.
    pub fn pose(&self, weapon: Weapon) -> Pose {
        if self.active && weapon == self.weapon { pose(weapon, self.progress()) } else { REST }
    }
    /// The camera's turn from the swing, as a pitch (up positive), a roll about the view axis and a shake of (turn, roll):
    /// it leans back into the wind-up and forward into the blow (and nods with it), and on contact it is jolted down and
    /// shaken, the more for a backstab, both fading in about a fifth of a second. Added to the gun's recoil.
    pub fn camera(&self, now: f32) -> (f32, f32, Vec2) {
        let u = self.progress().clamp(0., 1.);
        // The lean: back and to the right as the gun is wound, over to the left with the snap, then home.
        let lean = if !self.active {
            0.
        } else if u < WOUND {
            -smooth(u / WOUND)
        } else if u < CONTACT {
            -1. + 2. * smooth((u - WOUND) / (CONTACT - WOUND))
        } else {
            smooth((1. - u) / (1. - CONTACT))
        };
        let nod = if !self.active {
            0.
        } else if u < WOUND {
            smooth(u / WOUND)
        } else if u < CONTACT {
            1. - 2.4 * smooth((u - WOUND) / (CONTACT - WOUND))
        } else {
            -1.4 * smooth((1. - u) / (1. - CONTACT))
        };
        let power = if self.heavy { 1.9 } else { 1. };
        // On contact: quick to rise, then a damped ring back.
        let tau = self.since;
        let kick = if tau < 0.6 { (tau / 0.03).min(1.) * (-tau / 0.075).exp() * (1. + 0.35 * (tau * 38.).cos()) } else { 0. };
        let shake = if tau < 0.6 { (-tau / 0.09).exp() * (tau / 0.012).min(1.) } else { 0. };
        let noise = Vec2::new(((now + self.seed) * 91.).sin(), ((now + self.seed) * 113. + 2.).sin());
        (0.012 * nod - 0.050 * power * kick, 0.034 * lean * if self.heavy { 1.3 } else { 1. }, noise * 0.011 * power * shake)
    }
    /// How far the field of view is drawn in on contact (a share of itself), for a punch: 0 at rest.
    pub fn punch(&self) -> f32 {
        let tau = self.since;
        if tau < 0.3 { (if self.heavy { 0.07 } else { 0.035 }) * (-tau / 0.07).exp() * (tau / 0.012).min(1.) } else { 0. }
    }
}

// ----- What happened, from the simulation's word

/// What `observe` keeps from frame to frame, and the handles of the effects.
#[derive(Resource, Default)]
pub struct Fx {
    /// Blows seen in the last second, for the third-person swings to find (`struck`).
    blows: Vec<Blow>,
    /// The record of what last hurt each hotdog that has been acted on, and each zombie's HP and place.
    seen: HashMap<u32, (u8, u32, u32)>,
    zombies: HashMap<u32, (f32, Vec3, f32)>,
    /// Each hotdog's count of swings, and when it last rose (the real clock).
    swings: HashMap<u32, (u32, f32)>,
    /// A backstab's mark: since when, and whether it is this player's own (a kill, rather than a death).
    mark: Option<(f32, bool)>,
    /// Other hotdogs' blows to show when their swings make contact: when (the real clock), and the blow.
    pending: Vec<(f32, Blow)>,
    look: Option<Look>,
    ribbon: Option<(Entity, Handle<Mesh>)>,
    /// Where the gun's two ends have been (in the camera's frame) while it swings, newest last, with when.
    trail: Vec<(Vec3, Vec3, f32)>,
    ends: HashMap<usize, (Vec3, Vec3)>,
}
struct Look {
    sphere: Handle<Mesh>,
    cube: Handle<Mesh>,
    flash: Handle<StandardMaterial>,
    /// The flash is three spheres inside one another, a dim orange halo, the white-hot flash and a core brighter still, so
    /// that it glows rather than being a disc.
    halo: Handle<StandardMaterial>,
    core: Handle<StandardMaterial>,
    glint: Handle<StandardMaterial>,
    drops: [Handle<StandardMaterial>; 3],
    streak: Handle<StandardMaterial>,
    whoosh: Handle<AudioSource>,
    thwack: Handle<AudioSource>,
    icon: Entity,
}
impl Fx {
    /// Whether hotdog `id`'s swing number `swings` landed a blow in the last second: `Some(backstab)` if it did.
    pub fn struck(&self, id: u32, swings: u32) -> Option<bool> {
        self.blows.iter().find(|b| b.attacker == id && b.swings == swings).map(|b| b.backstab)
    }
}
/// The newest blow's age to count as fresh when first seen, in seconds of the round's clock.
const FRESH: f32 = 1.;

/// Looks at what the simulation (or the last frame from the server) says: a hotdog's record of what last hurt it that
/// says a melee blow (or a backstab) it has not been seen to say before, and a zombie that has lost HP (or been
/// killed) where a hotdog has just swung. Each is a `Blow`: the third-person swings are told of it (`Fx::struck`), this
/// player's own swing feels it, a thwack is heard, and a backstab that killed leaves its mark. Once a frame, after the
/// roster is up to date.
pub fn observe(world: &mut World, g: &mut Game, now: f32) {
    if !world.contains_resource::<Fx>() {
        return;
    }
    let me = g.me();
    let elapsed = g.elapsed;
    let mut fresh: Vec<Blow> = vec![];
    world.resource_scope(|_, mut fx: Mut<Fx>| {
        fx.blows.retain(|b| now - b.time < 1.);
        // (The swings that began this frame: a hotdog's count of them rose.)
        let mut swung: Vec<(u32, u32, Vec3)> = vec![];
        for p in &g.roster {
            let was = fx.swings.get(&p.id).map(|(n, _)| *n);
            if was != Some(p.swings) {
                if was.is_some() {
                    swung.push((p.id, p.swings, Vec3::from_array(p.p)));
                }
                fx.swings.insert(p.id, (p.swings, now));
            }
        }
        for victim in &g.roster {
            let record = victim.last_damage;
            let tag = (record.kind, record.attacker, record.time.to_bits());
            if fx.seen.get(&victim.id) == Some(&tag) {
                continue;
            }
            fx.seen.insert(victim.id, tag);
            if !matches!(record.kind, feast::MELEE | feast::BACKSTAB) || record.attacker == 0 || record.attacker == victim.id || (elapsed - record.time).abs() > FRESH {
                continue;
            }
            let swings = g.roster.iter().find(|p| p.id == record.attacker).map_or(0, |p| p.swings);
            let chest = feast::chest(victim);
            fresh.push(Blow {
                attacker: record.attacker,
                victim: victim.id,
                swings,
                at: chest,
                along: Vec3::from_array(record.d).normalize_or(Vec3::NEG_Z),
                backstab: record.kind == feast::BACKSTAB,
                kill: !victim.alive,
                time: now,
            });
        }
        // A zombie that lost HP, or is gone, beside a hotdog that has just swung.
        let before = std::mem::take(&mut fx.zombies);
        for z in g.feast.enemies.iter().filter(|z| z.forming <= 0.) {
            fx.zombies.insert(z.id, (z.hp, Vec3::from_array(z.p), z.yaw));
        }
        for (id, (hp, at, yaw)) in before {
            let now_hp = fx.zombies.get(&id).map_or(0., |(hp, _, _)| *hp);
            if now_hp >= hp - 1e-3 {
                continue;
            }
            let Some((attacker, swings, from)) = swung.iter().copied().filter(|(_, _, p)| p.distance(at) < 0.5).min_by(|a, b| a.2.distance(at).total_cmp(&b.2.distance(at))) else {
                continue;
            };
            let along = Vec3::new(at.x - from.x, 0., at.z - from.z).normalize_or(Vec3::NEG_Z);
            let killed = now_hp <= 0.;
            fresh.push(Blow {
                attacker,
                victim: 0,
                swings,
                at: at + Vec3::Y * 0.075,
                along,
                // (The simulation's own rule: from inside the zombie's rear arc.)
                backstab: feast::backstab(at.to_array(), yaw, from, along),
                kill: killed,
                time: now,
            });
        }
        fx.blows.extend(fresh.iter().cloned());
        // (The mark comes with the contact: when this player's own swing is there, or the other's would be.)
        let contact = |blow: &Blow| now + if blow.attacker == me { ((CONTACT - g.whip.progress()) * TOTAL).max(0.) } else { CONTACT * TOTAL };
        for blow in &fresh {
            if blow.backstab && blow.kill && (blow.attacker == me || blow.victim == me) {
                fx.mark = Some((contact(blow), blow.attacker == me));
            }
        }
    });
    for blow in fresh {
        if blow.attacker == me {
            g.whip.connect(blow);
        } else {
            // The others' blows are shown when their swings make contact.
            world.resource_mut::<Fx>().pending.push((now + CONTACT * TOTAL, blow));
        }
    }
    // (Those whose time has come: a thwack of their own, quieter the further off it is, and the flash and the droplets.)
    let due: Vec<Blow> = {
        let mut fx = world.resource_mut::<Fx>();
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut fx.pending).into_iter().partition(|(at, _)| *at <= now);
        fx.pending = later;
        due.into_iter().map(|(_, blow)| blow).collect()
    };
    for blow in due {
        let near = (1. - blow.at.distance(g.position) / 3.).clamp(0.15, 1.);
        thwack(world, near * 0.7, blow.backstab);
        splat(world, &blow, (now * 1000.) as u32 ^ blow.attacker, None);
    }
}
fn thwack(world: &mut World, volume: f32, heavy: bool) {
    let Some(sound) = world.get_resource::<Fx>().and_then(|fx| fx.look.as_ref().map(|l| l.thwack.clone())) else {
        return;
    };
    world.spawn((AudioPlayer::new(sound), PlaybackSettings::DESPAWN.with_speed(if heavy { 0.8 } else { 1. }).with_volume(Volume::Linear(volume))));
}

// ----- Frame by frame

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fx>().init_resource::<crate::icons::Icons>().add_systems(Startup, setup).add_systems(Update, (update.after(crate::arena::tick), flecks));
    }
}

fn unlit(materials: &mut Assets<StandardMaterial>, color: LinearRgba, add: bool) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::LinearRgba(color),
        unlit: true,
        alpha_mode: if add { AlphaMode::Add } else { AlphaMode::Opaque },
        fog_enabled: false,
        ..default()
    })
}
/// The effects' meshes, materials and sounds, and the backstab's mark on the screen.
fn setup(mut commands: Commands, mut fx: ResMut<Fx>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, server: Res<AssetServer>) {
    let icon = commands
        .spawn((
            Name::new("Backstab mark"),
            ImageNode::default(),
            Node { position_type: PositionType::Absolute, left: Val::Percent(50.), top: Val::Percent(50.8), width: Val::Px(MARK_SIZE), height: Val::Px(MARK_SIZE), margin: UiRect { left: Val::Px(MARK_SIZE * 0.55), ..default() }, ..default() },
            UiTransform::IDENTITY,
            Visibility::Hidden,
            GlobalZIndex(20),
        ))
        .id();
    fx.look = Some(Look {
        sphere: meshes.add(Sphere::new(1.).mesh().uv(24, 12)),
        cube: meshes.add(Cuboid::default()),
        // White-hot, brighter than the screen can show, so that it blooms.
        flash: unlit(&mut materials, LinearRgba::rgb(5., 4., 2.2), true),
        halo: unlit(&mut materials, LinearRgba::rgb(0.5, 0.22, 0.05), true),
        core: unlit(&mut materials, LinearRgba::rgb(9., 9., 8.), true),
        glint: unlit(&mut materials, LinearRgba::rgb(6., 2., 1.2), true),
        drops: [unlit(&mut materials, LinearRgba::rgb(1.6, 1.1, 0.05), false), unlit(&mut materials, LinearRgba::rgb(1.4, 0.05, 0.03), false), unlit(&mut materials, LinearRgba::rgb(3., 2.6, 1.6), false)],
        streak: unlit(&mut materials, LinearRgba::rgb(1., 1., 1.), true),
        whoosh: server.load("audio/whoosh.wav"),
        thwack: server.load("audio/thwack.wav"),
        icon,
    });
}
/// Seconds the light that trails the gun lasts.
const LIFE: f32 = 0.08;
/// How big the backstab's mark is: this share of the window's height, in device pixels (between the limits), and its
/// size in logical pixels before it has been drawn.
const MARK_SHARE: f32 = 0.052;
const MARK_LIMITS: (u32, u32) = (26, 72);
const MARK_SIZE: f32 = 36.;

/// A spark of what flies when a blow lands, or the flash of it: it moves, falls, and shrinks away.
#[derive(Component)]
struct Fleck {
    velocity: Vec3,
    left: f32,
    total: f32,
    size: Vec3,
    /// A droplet falls (and goes out on the counter); a flash only fades.
    falls: bool,
}
/// Cheap noise in 0..1.
fn hash(n: u32) -> f32 {
    let x = n.wrapping_mul(747796405).wrapping_add(2891336453);
    let x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    ((x >> 22) ^ x) as f32 / u32::MAX as f32
}
/// What flies from a blow at `blow.at`: a flash, droplets of sauce thrown along the blow and up, and for a backstab a
/// bigger flash, more droplets and a glint of a slash through the victim. For this player's own blow (`eye`: where it looks
/// from) the flash and the glint are drawn in the view model's pass, over the gun that is at the spot, instead of behind it.
fn splat(world: &mut World, blow: &Blow, seed: u32, eye: Option<Vec3>) {
    let Some((sphere, cube, flash, halo, core, glint, drops)) = world.resource::<Fx>().look.as_ref().map(|l| (l.sphere.clone(), l.cube.clone(), l.flash.clone(), l.halo.clone(), l.core.clone(), l.glint.clone(), l.drops.clone())) else {
        return;
    };
    let heavy = blow.backstab;
    let layers = if eye.is_some() { RenderLayers::layer(crate::viewmodel::VIEW_LAYER) } else { RenderLayers::layer(0) };
    let along = blow.along.normalize_or(Vec3::NEG_Z);
    let side = along.cross(Vec3::Y).normalize_or(Vec3::X);
    // (This player's own flash is drawn in the gun's pass, where the gun at the spot would hide it from behind: it is
    // brought along the line of sight to just short of half way from the eye, in front of the gun, and made smaller to
    // look the same size.)
    const NEAR: f32 = 0.45;
    let size = if heavy { 0.032 } else { 0.02 };
    let (size, at) = match eye {
        Some(eye) => (size * NEAR, eye.lerp(blow.at, NEAR)),
        None => (size, blow.at - along * 0.02),
    };
    for (share, material, time) in [(1.35, halo, 0.10), (0.8, flash, 0.09), (0.38, core, 0.07)] {
        world.spawn((
            Name::new("Blow flash"),
            Mesh3d(sphere.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(at).with_scale(Vec3::splat(size * share)),
            layers.clone(),
            NotShadowCaster,
            Fleck { velocity: Vec3::ZERO, left: time, total: time, size: Vec3::splat(size * share), falls: false },
        ));
    }
    if heavy {
        // The glint of a slash: a thin bar of light through the victim, across the blow and slanted.
        let slash = (side * 0.8 + Vec3::Y * 0.6).normalize();
        world.spawn((
            Name::new("Backstab glint"),
            Mesh3d(cube),
            MeshMaterial3d(glint),
            Transform::from_translation(eye.map_or(blow.at, |eye| eye.lerp(blow.at, NEAR))).looking_to(slash, Vec3::Y).with_scale(Vec3::new(0.0016, 0.0016, 0.085) * eye.map_or(1., |_| NEAR)),
            layers,
            NotShadowCaster,
            Fleck { velocity: Vec3::ZERO, left: 0.14, total: 0.14, size: Vec3::new(0.0016, 0.0016, 0.085) * eye.map_or(1., |_| NEAR), falls: false },
        ));
    }
    let count = if heavy { 22 } else { 11 };
    for i in 0..count {
        let r = |k: u32| hash(seed.wrapping_mul(131).wrapping_add(i * 7 + k));
        let spread = side * (r(1) - 0.5) * 1.5 + Vec3::Y * (0.15 + r(2) * 0.9);
        let velocity = (along * (0.25 + r(3) * 0.9) + spread) * if heavy { 1.15 } else { 0.9 };
        let radius = (0.0026 + r(4) * 0.0034) * if heavy { 1.2 } else { 1. };
        let left = 0.32 + r(5) * 0.28;
        world.spawn((
            Name::new("Blow droplet"),
            Mesh3d(sphere.clone()),
            MeshMaterial3d(drops[(i % 3) as usize].clone()),
            Transform::from_translation(blow.at + along * 0.01).with_scale(Vec3::splat(radius)),
            NotShadowCaster,
            Fleck { velocity, left, total: left, size: Vec3::splat(radius), falls: true },
        ));
    }
}

/// Once a frame, after `arena::tick`: the swing's whoosh and its landing (the thwack, the flash and the droplets), the
/// flecks that are flying, the light that trails the gun, and the backstab's mark.
fn update(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let cues = world.get_resource_mut::<Game>().map(|mut g| g.whip.take()).unwrap_or_default();
    if cues.swung {
        if let Some(whoosh) = world.resource::<Fx>().look.as_ref().map(|l| l.whoosh.clone()) {
            let speed = 0.95 + 0.1 * hash((now * 1000.) as u32);
            world.spawn((AudioPlayer::new(whoosh), PlaybackSettings::DESPAWN.with_speed(speed).with_volume(Volume::Linear(0.8))));
        }
    }
    if let Some(blow) = cues.landed {
        thwack(world, 1., blow.backstab);
        // (From where the view looks: the game's camera.)
        let eye = world.get_resource::<Game>().and_then(|g| g.camera()).and_then(|camera| world.get::<GlobalTransform>(camera)).map(|at| at.translation());
        splat(world, &blow, (now * 1000.) as u32, eye);
    }
    world.resource_scope(|world, g: Mut<Game>| ribbon(world, &g, now));
    mark(world, now);
}

/// Moves and fades the flecks, and removes those that have had their time.
fn flecks(time: Res<Time<Real>>, mut commands: Commands, mut flecks: Query<(Entity, &mut Transform, &mut Fleck)>) {
    let dt = time.delta_secs();
    for (entity, mut tr, mut fleck) in &mut flecks {
        fleck.left -= dt;
        if fleck.left <= 0. {
            commands.entity(entity).try_despawn();
            continue;
        }
        if fleck.falls {
            fleck.velocity.y -= 2.8 * dt;
            tr.translation += fleck.velocity * dt;
            if tr.translation.y < feast::FLOOR + 0.002 {
                tr.translation.y = feast::FLOOR + 0.002;
                fleck.velocity = Vec3::ZERO;
                fleck.falls = false;
            }
        }
        // Shrinks away over the end of its time.
        let life = (fleck.left / fleck.total).clamp(0., 1.);
        tr.scale = fleck.size * if fleck.falls { (life * 3.).min(1.) } else { life.sqrt() };
    }
}

/// The ends of a gun, in the view model's frame: its front and its back, from the model's extent.
fn ends(weapon: Weapon) -> (Vec3, Vec3) {
    let (mut front, mut back) = (f32::MAX, f32::MIN);
    for (_, shape) in crate::weapons::model(weapon as usize) {
        for p in &shape.positions {
            (front, back) = (front.min(p.z), back.max(p.z));
        }
    }
    let view = crate::weapons::in_view(weapon as usize);
    (view.transform_point(Vec3::new(0., 0.004, front)), view.transform_point(Vec3::new(0., 0.004, back)))
}
/// The light that trails the gun as it is swung: a ribbon along the end of it that leads (the butt or the grip) through
/// where it has been in the last LIFE seconds, bright where the gun was fastest and gone by the time the swing is over,
/// hot white for a blow and red for a backstab, additive in the view model's pass. Only the first-person gun leaves one.
fn ribbon(world: &mut World, g: &Game, now: f32) {
    let Some(camera) = g.camera() else {
        return;
    };
    let weapon = g.weapon;
    let u = g.whip.progress();
    // (Nothing swinging and nothing left of the light: nothing to do but see that the ribbon is hidden.)
    if !g.whip.moving() && world.resource::<Fx>().trail.iter().all(|(_, _, born)| now - born >= LIFE) {
        world.resource_mut::<Fx>().trail.clear();
        if let Some((entity, _)) = world.resource::<Fx>().ribbon.clone() {
            if world.get::<Visibility>(entity).is_some_and(|v| *v != Visibility::Hidden) {
                world.entity_mut(entity).insert(Visibility::Hidden);
            }
        }
        return;
    }
    // The viewmodel's place in the camera's frame this frame.
    let root = world
        .query::<(&crate::viewmodel::ViewModel, &Transform)>()
        .iter(world)
        .find(|(model, _)| model.0 == weapon as usize)
        .map(|(_, tr)| *tr);
    let streaking = g.whip.moving() && u > 0.2 && u < 0.64 && weapon != Weapon::Laden;
    {
        let mut fx = world.resource_mut::<Fx>();
        fx.trail.retain(|(_, _, born)| now - born < LIFE);
        if streaking {
            if let Some(root) = root {
                let (front, back) = *fx.ends.entry(weapon as usize).or_insert_with(|| ends(weapon));
                // (The end that leads, and a third of the way along from it: a streak at the edge, not the whole gun's wedge.)
                let (a, b) = (root.transform_point(back), root.transform_point(back.lerp(front, 0.3)));
                // (One sample every few milliseconds however fast the frames come, and none while the swing is held.)
                let new = fx.trail.last().is_none_or(|(p, q, born)| now - born > 0.004 && (p.distance(a) > 0.001 || q.distance(b) > 0.001));
                if new && !g.whip.held() {
                    fx.trail.push((a, b, now));
                }
            }
        }
    }
    let (entity, mesh) = {
        // (A ribbon whose entity is gone, with the camera's other children when the round is rebuilt, is made again.)
        let existing = world.resource::<Fx>().ribbon.clone().filter(|(entity, _)| world.get_entity(*entity).is_ok());
        match existing {
            Some(r) => r,
            None => {
                let (Some(streak),) = (world.resource::<Fx>().look.as_ref().map(|l| l.streak.clone()),) else {
                    return;
                };
                let mesh = world.resource_mut::<Assets<Mesh>>().add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
                let entity = world
                    .spawn((Name::new("Strike streak"), Mesh3d(mesh.clone()), MeshMaterial3d(streak), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster))
                    .id();
                world.entity_mut(camera).add_child(entity);
                world.resource_mut::<Fx>().ribbon = Some((entity, mesh.clone()));
                (entity, mesh)
            }
        }
    };
    let trail = world.resource::<Fx>().trail.clone();
    let shown = if trail.len() >= 2 { Visibility::Inherited } else { Visibility::Hidden };
    if world.get::<Visibility>(entity) != Some(&shown) {
        world.entity_mut(entity).insert(shown);
    }
    if trail.len() < 2 {
        return;
    }
    // Hot white, or red for a backstab; brightest where the samples are furthest apart (the fastest part).
    let tint = if g.whip.heavy() { [2.4, 0.7, 0.45] } else { [2., 1.8, 1.5] };
    let n = trail.len();
    let mut positions = Vec::with_capacity(n * 2);
    let mut colors = Vec::with_capacity(n * 2);
    for (i, (a, b, born)) in trail.iter().enumerate() {
        let age = ((now - born) / LIFE).clamp(0., 1.);
        let speed = if i == 0 { 0. } else { (trail[i].0.distance(trail[i - 1].0) / (trail[i].2 - trail[i - 1].2).max(1e-3) / 1.6).clamp(0., 1.) };
        let alpha = (1. - age).powf(1.6) * (0.2 + 0.8 * speed) * 0.55;
        positions.push(a.to_array());
        positions.push(b.to_array());
        colors.push([tint[0], tint[1], tint[2], alpha]);
        colors.push([tint[0], tint[1], tint[2], alpha * 0.1]);
    }
    let mut indices = vec![];
    for i in 0..n as u32 - 1 {
        let k = i * 2;
        indices.extend([k, k + 1, k + 2, k + 1, k + 3, k + 2, k, k + 2, k + 1, k + 1, k + 2, k + 3]);
    }
    if let Some(mut m) = world.resource_mut::<Assets<Mesh>>().get_mut(&mesh) {
        let count = positions.len();
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0f32, 0., 1.]; count]);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0f32; 2]; count]);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        m.insert_indices(Indices::U32(indices));
    }
}

/// The backstab's mark: a small dagger at the crosshair's lower right, popping in and fading out over MARK seconds.
fn mark(world: &mut World, now: f32) {
    let (Some(icon), Some((since, mine))) = (world.resource::<Fx>().look.as_ref().map(|l| l.icon), world.resource::<Fx>().mark) else {
        return;
    };
    if world.get_entity(icon).is_err() {
        return;
    }
    let age = now - since;
    if !(0. ..MARK).contains(&age) {
        if age >= MARK {
            world.resource_mut::<Fx>().mark = None;
            world.entity_mut(icon).insert(Visibility::Hidden);
        }
        return;
    }
    // A pop: big and quick to settle, then a slow fade; red when it is this player who was struck.
    let pop = 1. + 0.9 * (-age / 0.09).exp();
    let fade = (1. - ((age - 0.9) / (MARK - 0.9)).clamp(0., 1.)).powi(2);
    let tint = if mine { Color::srgba(1., 0.97, 0.9, fade) } else { Color::srgba(1., 0.4, 0.34, fade) };
    // The dagger drawn for this window's height, in device pixels, so that it is crisp.
    let (height, scale) = world.query::<&Window>().iter(world).next().map_or((720, 1.), |w| (w.physical_height(), w.scale_factor()));
    let pixels = ((height as f32 * MARK_SHARE).round() as u32).clamp(MARK_LIMITS.0, MARK_LIMITS.1);
    let image = world.resource_scope(|world, mut icons: Mut<crate::icons::Icons>| world.resource_scope(|_, mut images: Mut<Assets<Image>>| icons.get(&mut images, crate::icons::Icon::Backstab, pixels)));
    let side = pixels as f32 / scale.max(0.1);
    if let Some(mut node) = world.get_mut::<ImageNode>(icon) {
        node.color = tint;
        if node.image != image {
            node.image = image;
        }
    }
    if let Some(mut node) = world.get_mut::<Node>(icon) {
        (node.width, node.height, node.margin.left) = (Val::Px(side), Val::Px(side), Val::Px(side * 0.55));
    }
    if let Some(mut tr) = world.get_mut::<UiTransform>(icon) {
        tr.scale = Vec2::splat(pop);
        tr.rotation = Rot2::degrees(-14. * (-age / 0.12).exp());
    }
    world.entity_mut(icon).insert(Visibility::Visible);
}

#[cfg(test)]
mod tests {
    use super::*;
    const GUNS: [Weapon; 7] = [Weapon::Pistol, Weapon::Sniper, Weapon::Flame, Weapon::Bazooka, Weapon::Spray, Weapon::Rifle, Weapon::Mayo];
    #[test]
    fn every_gun_has_its_own_first_person_strike_that_starts_and_ends_at_rest() {
        for weapon in GUNS {
            for u in [0., 1., 1.7, -0.2] {
                let (at, turn) = pose(weapon, u);
                assert!(at.length() < 1e-6 && turn.angle_between(Quat::IDENTITY) < 1e-6, "{weapon:?} rests at {u}");
            }
            // Wound back and up first, then driven forward of where it is held, with a turn that rolls it over; never a
            // jump (a millisecond of the swing moves the gun no more than a few millimetres, however fast the snap).
            let (cocked, landed) = (pose(weapon, WOUND), pose(weapon, CONTACT));
            assert!(cocked.0.z > 0.02 && landed.0.z < cocked.0.z - 0.04, "{weapon:?}: {} then {}", cocked.0, landed.0);
            assert!(cocked.0.y > -0.01, "{weapon:?} is not lifted: {}", cocked.0);
            assert!(cocked.1.angle_between(Quat::IDENTITY) > 0.6, "{weapon:?} is not rolled over in the wind-up");
            let mut last = pose(weapon, 0.);
            let mut fastest = (0., 0.);
            for i in 1..=800 {
                let u = i as f32 / 800.;
                let now = pose(weapon, u);
                let step = now.0.distance(last.0);
                assert!(step < 0.006 && now.1.angle_between(last.1) < 0.13, "{weapon:?} jumps at {u}: {step}");
                if step > fastest.1 {
                    fastest = (u, step);
                }
                last = now;
            }
            // The snap is the fastest part of it, and ends at contact.
            assert!((0.38..=CONTACT + 0.01).contains(&fastest.0), "{weapon:?} is fastest at {}", fastest.0);
        }
        // No two guns are swung the same way, and full hands (no gun in them) do not strike at all.
        for (i, a) in GUNS.iter().enumerate() {
            for b in &GUNS[i + 1..] {
                let (a, b) = (pose(*a, CONTACT), pose(*b, CONTACT));
                assert!(a.0.distance(b.0) > 0.01 || a.1.angle_between(b.1) > 0.3);
            }
        }
        assert_eq!(pose(Weapon::Laden, 0.4), REST);
    }
    #[test]
    fn the_whole_strike_is_between_0_35_and_0_45_seconds_and_fits_in_the_simulations_wait() {
        assert!((0.35..=0.45).contains(&(TOTAL + STOP)) && (0.35..=0.5).contains(&(TOTAL + STOP_STAB)), "{} and {}", TOTAL + STOP, TOTAL + STOP_STAB);
        // The blow cannot be made again until the swing is over (a backstab's longer hold is a kill).
        assert!(TOTAL + STOP <= feast::DELAY[0]);
        assert!(STOP_STAB > STOP && STOP > 2. / 60. && STOP < 4. / 60. + 1e-3, "two to three frames at 60 Hz");
        // The beats are in order, and the timeline is continuous at each of them.
        assert!(0. < WOUND && WOUND < CONTACT && CONTACT < FOLLOWED && FOLLOWED < 1.);
        for u in [WOUND, CONTACT, FOLLOWED] {
            let (a, b) = (between(u - 1e-4), between(u + 1e-4));
            let (ta, tb) = (if a.1 == b.0 { 1. } else { a.2 }, b.2);
            assert!(a.1 == b.0 || (a.0 == b.0 && a.1 == b.1), "{a:?} {b:?}");
            assert!((ta - tb).abs() < 0.05 || (1. - ta + tb).abs() < 0.05, "{a:?} {b:?}");
        }
    }
    #[test]
    fn a_swing_that_lands_stands_still_on_contact_and_one_that_misses_runs_through() {
        let blow = |backstab| Blow { attacker: 1, victim: 2, swings: 1, at: Vec3::ZERO, along: Vec3::NEG_Z, backstab, kill: backstab, time: 0. };
        let run = |hit: Option<Blow>| {
            let mut s = Swing::rest();
            s.start(Weapon::Rifle, 0.);
            assert!(s.take().swung);
            if let Some(hit) = hit {
                s.connect(hit);
            }
            let (mut frames, mut held, mut landed) = (0, 0, None);
            while s.moving() && frames < 10_000 {
                s.advance(1. / 240.);
                frames += 1;
                held += s.held() as u32;
                landed = landed.or(s.take().landed);
            }
            (frames as f32 / 240., held as f32 / 240., landed)
        };
        let (miss, held, landed) = run(None);
        assert!((miss - TOTAL).abs() < 0.01 && held == 0. && landed.is_none(), "{miss} {held}");
        let (hit, held, landed) = run(Some(blow(false)));
        assert!((held - STOP).abs() < 0.01 && landed.is_some_and(|b| !b.backstab), "held {held}");
        assert!((hit - TOTAL - STOP).abs() < 0.015, "{hit}");
        let (stab, held, landed) = run(Some(blow(true)));
        assert!((held - STOP_STAB).abs() < 0.01 && landed.is_some_and(|b| b.backstab), "held {held}");
        assert!(stab > hit);
        // The pose does not move while it is held.
        let mut s = Swing::rest();
        s.start(Weapon::Pistol, 0.);
        s.connect(blow(false));
        while !s.held() {
            s.advance(1. / 240.);
        }
        let at = s.pose(Weapon::Pistol);
        for _ in 0..4 {
            s.advance(1. / 240.);
            assert_eq!(s.pose(Weapon::Pistol), at);
        }
        // A blow told of late (after the follow-through) is shown but holds nothing.
        let mut s = Swing::rest();
        s.start(Weapon::Pistol, 0.);
        while s.progress() < 0.8 {
            s.advance(1. / 240.);
        }
        s.connect(blow(false));
        s.advance(1. / 240.);
        assert!(!s.held() && s.take().landed.is_some());
    }
    #[test]
    fn the_camera_is_kicked_harder_and_shaken_longer_by_a_backstab() {
        let kick = |backstab: bool| {
            let mut s = Swing::rest();
            s.start(Weapon::Rifle, 0.);
            s.connect(Blow { attacker: 1, victim: 2, swings: 1, at: Vec3::ZERO, along: Vec3::NEG_Z, backstab, kill: backstab, time: 0. });
            let mut peak = (0f32, 0f32, 0f32);
            for i in 0..400 {
                s.advance(1. / 240.);
                let (pitch, _, shake) = s.camera(i as f32 / 240.);
                if s.since > 0.3 {
                    assert!(pitch.abs() < 0.02 && shake.length() < 0.002, "settles: {pitch} {shake}");
                }
                peak = (peak.0.max(-pitch), peak.1.max(shake.length()), peak.2.max(s.punch()));
            }
            peak
        };
        let (plain, stab) = (kick(false), kick(true));
        assert!(plain.0 > 0.02 && plain.1 > 0.003, "{plain:?}");
        assert!(stab.0 > plain.0 * 1.5 && stab.1 > plain.1 * 1.5 && stab.2 > plain.2, "{plain:?} {stab:?}");
    }
    /// What `observe` makes of the simulation's word: a blow on a hotdog's record (a fresh one only, and each once), a
    /// zombie that lost HP beside a hotdog that has just swung, whose swing it was, whether it was a backstab and a kill, and
    /// whose it is to feel and to be marked by.
    #[test]
    fn a_blow_on_the_record_is_told_to_the_swing_that_made_it_and_a_backstab_that_kills_leaves_its_mark() {
        use kitchen_sim::feast::{BACKSTAB, Damage, MELEE, Zombie, ZOMBIE_HP};
        let mut app = crate::arena::test_app("");
        app.init_resource::<Fx>();
        let world = app.world_mut();
        let now = std::cell::Cell::new(10.);
        let seen = |world: &mut World, change: &dyn Fn(&mut Game)| {
            now.set(now.get() + 0.1);
            world.resource_scope(|world, mut g: Mut<Game>| {
                change(&mut g);
                observe(world, &mut g, now.get());
            });
        };
        // The first look settles what is already on the records (nothing, and a stale blow: nobody is told of it).
        seen(world, &|g| {
            g.roster[2].last_damage = Damage { kind: BACKSTAB, attacker: 3, time: g.elapsed - 30., amount: 100., ..default() };
        });
        assert!(world.resource::<Fx>().blows.is_empty() && world.resource::<Fx>().mark.is_none());
        // This player (hotdog 1) swings and strikes hotdog 2 from the front: its swing is told, and nothing is marked.
        world.resource_scope(|_, mut g: Mut<Game>| g.whip.start(Weapon::Rifle, 0.));
        seen(world, &|g| {
            g.roster[0].swings = 1;
            g.roster[1].last_damage = Damage { kind: MELEE, attacker: 1, time: g.elapsed, amount: 50., ..default() };
        });
        let fx = world.resource::<Fx>();
        assert_eq!((fx.struck(1, 1), fx.struck(1, 2), fx.struck(3, 1), fx.mark), (Some(false), None, None, None));
        let mut felt = false;
        world.resource_scope(|_, mut g: Mut<Game>| {
            for _ in 0..200 {
                g.whip.advance(0.005);
                felt |= g.whip.held();
            }
        });
        assert!(felt, "the swing stood still on contact");
        // Seen again, it is the same blow: nothing more.
        let before = world.resource::<Fx>().pending.len() + world.resource::<Fx>().blows.len();
        seen(world, &|_| {});
        assert_eq!(world.resource::<Fx>().pending.len() + world.resource::<Fx>().blows.len(), before);
        // A backstab that kills, by this player: its mark (its own, a kill); and one that kills this player, by another's
        // hand, whose swing (another's) is shown at contact and not felt here.
        world.resource_scope(|_, mut g: Mut<Game>| g.whip.start(Weapon::Rifle, 0.));
        seen(world, &|g| {
            g.roster[0].swings = 2;
            g.roster[2].alive = false;
            g.roster[2].last_damage = Damage { kind: BACKSTAB, attacker: 1, time: g.elapsed, amount: 100., ..default() };
        });
        let fx = world.resource::<Fx>();
        assert_eq!((fx.struck(1, 2), fx.mark.map(|(_, mine)| mine)), (Some(true), Some(true)));
        seen(world, &|g| {
            g.roster[3].swings = 5;
            g.roster[0].alive = false;
            g.roster[0].last_damage = Damage { kind: BACKSTAB, attacker: 4, time: g.elapsed, amount: 100., ..default() };
        });
        let fx = world.resource::<Fx>();
        assert_eq!((fx.struck(4, 5), fx.mark.map(|(_, mine)| mine)), (Some(true), Some(false)), "the death of this player is marked as one");
        assert!(fx.pending.iter().any(|(at, blow)| blow.attacker == 4 && *at > now.get()), "shown when that swing makes contact");
        // A zombie struck beside a hotdog that has just swung: from the front it loses half and is struck; from behind, it
        // is gone, and that is a backstab by the simulation's own rule (its rear arc).
        let zombie = |yaw| Zombie { id: 7, p: [1., feast::FLOOR, 1.], yaw, hp: ZOMBIE_HP, born: 0., forming: 0. };
        seen(world, &|g| {
            g.roster[1].p = [1., feast::FLOOR, 0.88];
            g.feast.enemies = vec![zombie(0.)];
        });
        seen(world, &|g| {
            g.roster[1].swings += 1;
            g.feast.enemies[0].hp = ZOMBIE_HP / 2.;
        });
        let fx = world.resource::<Fx>();
        let front = fx.blows.iter().find(|b| b.victim == 0).cloned().expect("the zombie's blow");
        assert_eq!((front.attacker, front.backstab, front.kill), (2, false, false), "{front:?}");
        seen(world, &|g| {
            g.roster[1].swings += 1;
            g.feast.enemies[0].yaw = std::f32::consts::PI;
        });
        seen(world, &|g| {
            g.roster[1].swings += 1;
            g.feast.enemies.clear();
        });
        let back = world.resource::<Fx>().blows.iter().rev().find(|b| b.victim == 0).cloned().expect("the kill");
        assert_eq!((back.attacker, back.backstab, back.kill), (2, true, true), "{back:?}");
    }
    #[test]
    fn hash_is_in_unit_range() {
        assert!((0..1000).all(|n| (0. ..=1.).contains(&hash(n))));
    }
}

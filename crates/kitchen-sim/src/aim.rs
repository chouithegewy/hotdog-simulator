// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! How a bot aims: as a person does, not as a machine. Every bot that shoots goes through here (Free For All, the
//! siege's wave and famine, Forts); the turrets do not (they are not bots: their beam sweeps to a cat as it always did),
//! nor Kung Fu's fighters (kungfu.rs).
//!
//! **What it sees.** A bot keeps its target in a `Sight` (`Player::sight`). It aims where it thinks the target is, which
//! lags behind where it is (LAG: the bot's idea of it follows it with that time constant), and leads it by the speed it
//! has seen it move at (how fast its idea of it moves), not by the speed it really has: a person's hotdog, whose walk is
//! not reported as a velocity, is led as well as a bot's. Each shot it leads by a share of that, drawn afresh (`lead`:
//! under or over, at random).
//!
//! **The cone.** Each shot leaves off its aim by an error drawn from a normal spread whose size (radians, each way) is
//! CONE, times the skill's `cone` and the gun's GUN, made larger by the range (RANGE a metre), by how fast the target
//! is seen to move (MOVING at a person's walk, times the gun's GUN_MOVING), by how fast the bot itself is walking (SELF
//! at a bot's walk), by a hit it took within FLINCH seconds (FLINCHED), and by how new the target is: the first shot at
//! a target is WALK_IN times more off again, each shot after it KEEP of that, so that a bot that keeps on the same
//! target walks its shots in. The error is drawn once a shot (a syrup burst is one), so between shots the bot holds its
//! aim; the gun's own bloom and climb (recoil.rs, feast.rs CLIMB) come on top, as they do for everyone. A burst's rounds
//! leave a tenth of a second apart, each along the way the bot faces as it leaves (feast.rs `volleys`): it keeps
//! tracking through the burst, with its lag, and its hand wanders between rounds (each later round's error keeps STEADY_HAND
//! of the one before and draws the rest afresh: `wander`), so at a target walking across it lands one or two of a
//! burst more often than all three.
//!
//! **Pace.** A person does not hold the trigger down on a target: after a shot they bring the aim back onto it. A bot
//! fires again no sooner than its gun allows and the skill's `recover` pause after that (drawn each shot); the spray
//! guns' streams it keeps on. A patient bot (normal and hard) also waits for its muzzle to come back down (its bloom
//! under STEADY of a shot's kick: the mustard pistol's, a hand cannon's, takes longer than the gun does to fire again);
//! an easy one does not, and with the pistol it brings the aim back EAGER as long: it spams, and its shots walk up.
//!
//! **Reaction.** A target newly seen (another than the last, or the last again after FORGET seconds unseen) is not
//! fired at for a reaction time drawn between the skill's `react` bounds, and the gun's SETTLE besides (the ketchup
//! sniper's scope takes time to settle on it).
//!
//! **Guns.** The relish glob is aimed at the feet, the rest at the chest; with the ketchup sniper and the mustard pistol
//! (a round in the face kills with either) a hard bot sometimes goes for the face (feast.rs FACE) of a target that faces
//! it. The spray guns (the hot sauce, the mayonnaise) are
//! forgiving as they are: their cone is smaller.
//!
//! **Skill** (`Rules::skill`, `skill=easy|normal|hard`): normal is a fair fight for an average player; easy misses
//! more, reacts later and lags further behind; hard less, though not perfectly. `perfect` is the old aim, exactly where
//! the target is led by its reported velocity, and only for tests and trying things out.
//!
//! **Dodging.** A bot that sees a relish glob coming at it jumps it some of the time (the skill's `dodge`), tucking its
//! knees up in the air (crouch held off the ground, actor.rs `Actor::tuck`) so that a glob bursting at its feet finds
//! less of it.
//!
//! Every draw comes from the game's seed (spawns.rs `Dice`), the round, the bot and how many draws it has made, so the
//! same game aims the same way.
use crate::feast::{CAT, Feast, GUNS, HUNGER, MAYO, PESTS, PISTOL, Player, RIFLE, SNIPER, chest, face_point, lead, muzzle};
use crate::spawns::Dice;
use glam::Vec3;

/// How good the bots are (`Rules::skill`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Skill {
    Easy,
    #[default]
    Normal,
    Hard,
    /// The machine's aim the bots once had: no error, no reaction, the target's reported velocity led exactly.
    Perfect,
}
/// A skill's numbers: the cone's scale, the reaction's bounds (s) and the scale on a gun's SETTLE, the bounds of the
/// pause to bring the aim back after a shot (s), the tracking lag (s), the lead's share and its spread, the chance of
/// jumping a glob, (the sniper and the pistol, a target facing it) the chance of going for the face, and whether it
/// waits for its muzzle to settle before it fires again (`patient`; see the module's documentation).
#[derive(Clone, Copy, Debug)]
pub struct Hand {
    pub cone: f32,
    pub react: [f32; 2],
    pub settle: f32,
    pub recover: [f32; 2],
    pub lag: f32,
    pub lead: [f32; 2],
    pub dodge: f32,
    pub face: f32,
    pub patient: bool,
}
impl Skill {
    pub const ALL: [Skill; 4] = [Skill::Easy, Skill::Normal, Skill::Hard, Skill::Perfect];
    /// A skill by name (easy, normal, hard, perfect), or None.
    pub fn named(name: &str) -> Option<Skill> {
        match name.trim().to_ascii_lowercase().as_str() {
            "easy" => Some(Skill::Easy),
            "normal" | "medium" => Some(Skill::Normal),
            "hard" => Some(Skill::Hard),
            "perfect" | "aimbot" => Some(Skill::Perfect),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Skill::Easy => "easy",
            Skill::Normal => "normal",
            Skill::Hard => "hard",
            Skill::Perfect => "perfect",
        }
    }
    pub fn hand(self) -> Hand {
        match self {
            Skill::Easy => Hand { cone: 1.45, react: [0.5, 0.85], settle: 1.4, recover: [0.45, 1.], lag: 0.08, lead: [0.55, 0.45], dodge: 0.1, face: 0., patient: false },
            Skill::Normal => Hand { cone: 1., react: [0.35, 0.6], settle: 1., recover: [0.4, 0.8], lag: 0.045, lead: [0.9, 0.2], dodge: 0.35, face: 0., patient: true },
            Skill::Hard => Hand { cone: 0.8, react: [0.22, 0.4], settle: 0.6, recover: [0.15, 0.45], lag: 0.035, lead: [0.95, 0.12], dodge: 0.65, face: 0.35, patient: true },
            Skill::Perfect => Hand { cone: 0., react: [0., 0.], settle: 0., recover: [0., 0.], lag: 0., lead: [1., 0.], dodge: 0., face: 0., patient: false },
        }
    }
}

/// The cone's size (radians, each way) for a settled bot of normal skill with a gun of GUN 1, still, at a still
/// target at no range (see the module's documentation).
pub const CONE: f32 = 0.031;
/// The cone's growth: per metre of range, at a target moving at a person's walk (actor.rs WALK: as much at the faster
/// walk as it was at the old, so MOVING is less than the 0.5 it was, the bot's tracking falling further behind a target
/// that turns), at the bot's own walk, and hurt.
pub const RANGE: f32 = 0.05;
pub const MOVING: f32 = 0.3;
pub const SELF: f32 = 0.8;
pub const FLINCHED: f32 = 0.4;
/// Seconds a hit throws a bot's aim off for.
pub const FLINCH: f32 = 0.4;
/// The first shot at a target is off by WALK_IN times more again; each one after by KEEP of the one before's more.
pub const WALK_IN: f32 = 1.2;
pub const KEEP: f32 = 0.75;
/// Seconds a target can go unseen and still be the same target (no new reaction, no new walk-in).
pub const FORGET: f32 = 1.;
/// By gun: the cone's scale, how much more a moving target throws it off, and the seconds it takes to settle on a new
/// target beyond the reaction (and a hard bot goes for the face only with the sniper and the pistol).
pub const GUN: [f32; GUNS] = [0., 1., 0.6, 0.5, 1., 0., 1., 0.5];
pub const GUN_MOVING: [f32; GUNS] = [0., 1., 4.5, 0.5, 1., 0., 1., 0.5];
pub const SETTLE: [f32; GUNS] = [0., 0., 0.35, 0., 0.1, 0., 0., 0.];
/// A patient bot fires again only once its bloom is under STEADY of its gun's kick a shot (recoil.rs); an easy bot with
/// the mustard pistol brings its aim back in EAGER of its recover pause.
pub const STEADY: f32 = 0.2;
pub const EAGER: f32 = 0.5;
/// The share of a burst round's error the next round keeps (the rest drawn afresh, so that the spread stays the same).
pub const STEADY_HAND: f32 = 0.5;
/// What the dice are thrown for (spawns.rs keeps 1 to 3).
const AIM: u64 = 4;

/// A bot's sight on its target (see the module's documentation): what it aims at (a hotdog's id, or CAT and a cat's),
/// since when and when it last did, the reaction it drew, where it thinks the target is, the next shot's error (in
/// cones, each way) and lead, whether that shot goes for the face, the shots fired at this target and the time before
/// which it brings its aim back after the last, and the draws made; the last glob it judged, whether it is tucked
/// in the air over one, and whether a burst of its is still leaving.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sight {
    pub target: u32,
    pub since: f32,
    pub last: f32,
    pub react: f32,
    pub seen: [f32; 3],
    pub error: [f32; 2],
    pub lead: f32,
    pub face: bool,
    pub shots: u32,
    pub hold: f32,
    pub draws: u64,
    pub judged: u32,
    pub tucked: bool,
    /// A syrup burst it fired is still leaving: its error and its walk-in hold until the last round has gone.
    pub burst: bool,
}

impl Feast {
    /// A number from 0 up to 1, the bot's next draw.
    fn draw(&mut self, id: u32) -> f32 {
        let (seed, round) = (self.card_seed, self.round as u64);
        let Some(p) = self.player_mut(id) else {
            return 0.5;
        };
        p.sight.draws += 1;
        let roll = Dice::new(&[seed, round, AIM, id as u64, p.sight.draws]).roll();
        (roll >> 40) as f32 / (1u64 << 24) as f32
    }
    /// A draw from a normal spread of 1 each way (the sum of three even draws, which never passes 3).
    fn spread(&mut self, id: u32) -> f32 {
        (self.draw(id) + self.draw(id) + self.draw(id) - 1.5) * 2.
    }
    /// The next shot's error, lead and aim (the face or not), drawn afresh.
    fn redraw(&mut self, id: u32, hand: Hand) {
        let error = [self.spread(id), self.spread(id)];
        let lead = (hand.lead[0] + hand.lead[1] * self.spread(id)).clamp(0., 2.);
        let face = self.draw(id) < hand.face;
        if let Some(p) = self.player_mut(id) {
            (p.sight.error, p.sight.lead, p.sight.face) = (error, lead, face);
        }
    }
    /// Where a bot aims at the target `key` (a hotdog's id, or CAT and a cat's), whose middle is at `c` (a relish glob is
    /// aimed at its feet: `feet`) moving at `v` as reported, with the gun `gun`, seeing it now; `face` the middle of its
    /// face when it faces the bot. Returns the way to face and fire, and whether it has reacted yet (it may fire). See
    /// the module's documentation. A perfect bot aims as bots once did.
    pub(crate) fn draw_bead(&mut self, id: u32, key: u32, c: Vec3, v: Vec3, gun: usize, face: Option<Vec3>) -> (Vec3, bool) {
        let skill = self.rules.skill;
        if skill == Skill::Perfect {
            return (self.aim(id, c, v, self.tune.speed[gun]), true);
        }
        let hand = skill.hand();
        let mark = self.track(id, key, c, hand);
        let Some(p) = self.player(id).cloned() else {
            return (Vec3::NEG_Z, false);
        };
        let o = muzzle(&p);
        // Where it thinks the target is, how fast it has seen it go, and what it aims at: the face, for a hard bot's
        // sniper shot that goes for it (where the face is from where the target is thought to be).
        let seen = Vec3::from_array(p.sight.seen);
        let speed = if hand.lag > 0. { (c - seen) / hand.lag } else { v };
        let point = match face.filter(|_| p.sight.face && (gun == SNIPER || gun == PISTOL)) {
            Some(face) => seen + (face - c),
            None => seen,
        };
        let pace = self.tune.speed[gun];
        let at = if pace > 0. { lead(o, point, speed * p.sight.lead, pace) } else { point };
        let d = (at - o).normalize_or(Vec3::NEG_Z);
        let ready = self.elapsed - p.sight.since >= p.sight.react + SETTLE[gun] * hand.settle && self.elapsed >= p.sight.hold && self.steady(&p, gun, hand);
        (self.err(&p, d, mark, o.distance(c), speed.length(), gun, hand), ready)
    }
    /// The same for the mayonnaise, lobbed along its arc (`Feast::lob`) at where the bot thinks the target is, led by
    /// what it has seen of its pace: None where no arc reaches it.
    pub(crate) fn draw_lob(&mut self, id: u32, key: u32, c: Vec3, v: Vec3) -> Option<(Vec3, bool)> {
        let skill = self.rules.skill;
        if skill == Skill::Perfect {
            return self.lob(id, c, v).map(|d| (d, true));
        }
        let hand = skill.hand();
        let mark = self.track(id, key, c, hand);
        let p = self.player(id).cloned()?;
        let seen = Vec3::from_array(p.sight.seen);
        let speed = if hand.lag > 0. { (c - seen) / hand.lag } else { v };
        let d = self.lob(id, seen, speed * p.sight.lead)?;
        let far = muzzle(&p).distance(c);
        Some((self.err(&p, d, mark, far, speed.length(), MAYO, hand), self.elapsed - p.sight.since >= p.sight.react && self.elapsed >= p.sight.hold))
    }
    /// Whether the bot's muzzle has come back down enough for it to fire `gun` again: always for an impatient hand and a
    /// spray gun, and otherwise once its bloom is under STEADY of the gun's kick a shot.
    fn steady(&self, p: &Player, gun: usize, hand: Hand) -> bool {
        !hand.patient || crate::recoil::SPRAY.get(gun).copied().unwrap_or(true) || p.bloom <= STEADY * crate::recoil::kick(gun)
    }
    /// Keeps the bot's sight on its target for this step: a new target (or one unseen for FORGET seconds) is acquired
    /// afresh, with a reaction drawn and nothing walked in; the bot's idea of where it is follows it. Returns how far off
    /// the walk-in and a hit still throw it (a scale on the cone).
    fn track(&mut self, id: u32, key: u32, c: Vec3, hand: Hand) -> f32 {
        let now = self.elapsed;
        let Some(p) = self.player(id) else {
            return 1.;
        };
        let fresh = p.sight.target != key || now - p.sight.last > FORGET;
        // A burst whose last round has gone is a shot fired: walked in a little further, and the next drawn.
        if p.sight.burst && p.volley == 0 && !fresh {
            if let Some(p) = self.player_mut(id) {
                (p.sight.burst, p.sight.shots) = (false, p.sight.shots + 1);
            }
            self.redraw(id, hand);
        }
        if fresh {
            let react = hand.react[0] + (hand.react[1] - hand.react[0]) * self.draw(id);
            if let Some(p) = self.player_mut(id) {
                p.sight = crate::aim::Sight { target: key, since: now, last: now, react, seen: c.to_array(), shots: 0, burst: false, ..p.sight.clone() };
            }
            self.redraw(id, hand);
        }
        let p = self.player_mut(id).unwrap();
        let dt = (now - p.sight.last).clamp(0., 0.2);
        let seen = Vec3::from_array(p.sight.seen);
        let seen = if hand.lag > 0. { seen + (c - seen) * (dt / (hand.lag + dt)) } else { c };
        (p.sight.seen, p.sight.last) = (seen.to_array(), now);
        // (A hit is anything that hurt it but hunger and the pests.)
        let hurt = now - p.last_damage.time < FLINCH && p.last_damage.amount > 0. && !matches!(p.last_damage.kind, HUNGER | PESTS);
        (1. + WALK_IN * KEEP.powi(p.sight.shots as i32)) * if hurt { 1. + FLINCHED } else { 1. }
    }
    /// The way `d` thrown off by the bot's error for this shot: the cone (see the module's documentation) at a target
    /// `far` metres off seen moving at `speed`, scaled by `mark` (the walk-in and a flinch).
    #[allow(clippy::too_many_arguments)]
    fn err(&self, p: &Player, d: Vec3, mark: f32, far: f32, speed: f32, gun: usize, hand: Hand) -> Vec3 {
        let walking = Vec3::new(p.v[0], 0., p.v[2]).length() / crate::actor::BOT_WALK as f32;
        let moving = speed / crate::actor::WALK as f32;
        let cone = CONE * hand.cone * GUN[gun.min(GUNS - 1)] * mark * (1. + RANGE * far) * (1. + MOVING * GUN_MOVING[gun.min(GUNS - 1)] * moving.min(2.)) * (1. + SELF * walking.min(1.5));
        let side = d.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let up = side.cross(d).normalize_or(Vec3::Y);
        (d + side * p.sight.error[0] * cone + up * p.sight.error[1] * cone).normalize_or(d)
    }
    /// A bot's shot of `gun` has left: the next is drawn afresh and walked in a little further (a syrup burst's once its
    /// last round has gone: `track`), and (but for a spray gun, whose stream it keeps on) it takes a moment to bring its
    /// aim back onto its target before it fires again (an easy bot with the pistol, EAGER of it).
    pub(crate) fn fired(&mut self, id: u32, gun: usize) {
        let skill = self.rules.skill;
        if skill == Skill::Perfect {
            return;
        }
        let hand = skill.hand();
        let pause = if crate::recoil::SPRAY.get(gun).copied().unwrap_or(false) { 0. } else { hand.recover[0] + (hand.recover[1] - hand.recover[0]) * self.draw(id) };
        let pause = if gun == PISTOL && !hand.patient { pause * EAGER } else { pause };
        let next = self.elapsed + self.tune.delay.get(gun).copied().unwrap_or(0.) + pause;
        let burst = gun == RIFLE && self.player(id).is_some_and(|p| p.volley > 0);
        if let Some(p) = self.player_mut(id) {
            p.sight.hold = next;
            if burst {
                p.sight.burst = true;
            } else {
                p.sight.shots += 1;
            }
        }
        if burst { self.wander(id) } else { self.redraw(id, hand) }
    }
    /// A round of a bot's syrup burst has left: its hand wanders a little before the next (see STEADY_HAND).
    pub(crate) fn wander(&mut self, id: u32) {
        // (Only a burst a bot's aim fired: `fired` marks it.)
        if self.rules.skill == Skill::Perfect || !self.player(id).is_some_and(|p| p.sight.burst) {
            return;
        }
        let fresh = [self.spread(id), self.spread(id)];
        let rest = (1. - STEADY_HAND * STEADY_HAND).sqrt();
        if let Some(p) = self.player_mut(id) {
            p.sight.error = [0, 1].map(|k| STEADY_HAND * p.sight.error[k] + rest * fresh[k]);
        }
    }
    /// A bot facing the way it aims, and firing when it has reacted: what the bots do with `draw_bead`'s answer. Returns
    /// whether it fired.
    pub(crate) fn loose(&mut self, id: u32, d: Vec3, ready: bool, gun: usize) -> bool {
        self.face(id, d);
        let fired = ready && self.fire(id, d, gun);
        if fired {
            self.fired(id, gun);
        }
        fired
    }
    /// The middle of what a bot aims at on a hotdog with `gun`, and its face's middle when it faces the bot at `o`: the
    /// chest, and for a relish glob the feet (the chest, where the feet are behind something solid).
    pub(crate) fn mark(&self, o: Vec3, t: &Player, gun: usize) -> (Vec3, Option<Vec3>) {
        let c = chest(t);
        let feet = Vec3::from_array(t.p) + Vec3::Y * 0.01;
        let c = if gun == 4 && { let far = o.distance(feet); self.cover(o, (feet - o) / far.max(1e-4)) + 0.02 >= far } { feet } else { c };
        let ahead = Vec3::new(-t.yaw.sin(), 0., -t.yaw.cos());
        let toward = Vec3::new(o.x - t.p[0], 0., o.z - t.p[2]).normalize_or_zero();
        (c, (ahead.dot(toward) > 0.6).then(|| face_point(t)))
    }
    /// A bot that sees a relish glob coming at it may jump it, tucking its knees up in the air; down again, it stands
    /// out of its tuck. Each glob is judged once.
    pub(crate) fn dodge(&mut self, id: u32) {
        let skill = self.rules.skill;
        let Some(p) = self.player(id).cloned() else {
            return;
        };
        if p.sight.tucked {
            // (Up: the knees stay tucked until it lands; landed, it stands.)
            if p.grounded && p.v[1] <= 0. {
                if let Some(p) = self.player_mut(id) {
                    (p.sight.tucked, p.crouch) = (false, false);
                }
            }
            return;
        }
        let chance = skill.hand().dodge;
        if chance <= 0. || !p.grounded || p.prone || p.carry >= 0 || p.launched {
            return;
        }
        let feet = Vec3::from_array(p.p) + Vec3::Y * 0.03;
        let coming = self.projectiles.iter().filter(|q| q.weapon == 4 && q.hostile && q.owner != id && q.id > p.sight.judged && !self.allied(q.owner, id)).find_map(|q| {
            let (o, v) = (Vec3::from_array(q.p), Vec3::from_array(q.v));
            let t = (feet - o).dot(v) / v.length_squared().max(1e-6);
            (t > 0.05 && t < 0.45 && (o + v * t).distance(feet) < 0.2).then_some(q.id)
        });
        let Some(glob) = coming else {
            return;
        };
        if let Some(p) = self.player_mut(id) {
            p.sight.judged = glob;
        }
        if self.draw(id) < chance {
            let stim = p.stim > 0.;
            self.jump(id, self.tune.leap(stim));
            if let Some(p) = self.player_mut(id) {
                (p.sight.tucked, p.crouch) = (true, true);
            }
        }
    }
}
/// A bot's target key for a cat: CAT and its id.
pub fn cat(id: u32) -> u32 {
    CAT + id
}

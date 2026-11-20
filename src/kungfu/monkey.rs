// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The Monkey: acrobatics. Crouched low with its knees wide and its body slung forward, one long loose arm knuckling
//! the counter and the other scratching the top of its head. It slaps and swats with arms that stretch like rope,
//! rolls along the counter and comes up under a fighter's chin, cartwheels and backflips into kicks, spins on all fours
//! with a leg out like a tail, and its special is a barrage of slaps and tumbles that ends with both arms clapping
//! together on a fighter's head like a toy monkey's cymbals.
use super::mantis::{carried, grip, kick_at, lifted};
use super::*;
use std::f32::consts::FRAC_PI_2;

/// The knuckling hand's spot just over the counter (out, up, forward in the body's frame: high enough that the shared
/// moves, which sink and tip the body with the arm held as it is, do not push it through), and the scratching hand's
/// place on the top of the head (from its shoulder).
const KNUCKLE: Vec3 = v(0.040, 0.019, 0.052);
const SCRATCH: Vec3 = v(-0.030, 0.092, -0.002);
/// Knees wide.
const WIDE: Vec3 = v(0.8, 0.2, 1.);
/// A cheeky grin, and the grin with its mouth wide open.
const GRIN: Face = Face { brow: 0.3, raise: 0.7, mouth: 0.45, eyes: 0.35, ..Face::CALM };
const HOOT: Face = Face { brow: 0.4, raise: 0.9, mouth: 0.9, eyes: 0.5, ..Face::CALM };

/// Scratching its head with the right hand (`rub` moves it back and forth).
fn scratching(p: Pose, rub: f32) -> Pose {
    p.arm(R, SCRATCH + v(0.004 * rub, 0., 0.006 * rub), Hand::Claw).elbow(R, v(1., 0.6, -0.3)).palm(R, v(-0.3, -1., 0.), 0.4)
}
/// The left hand knuckling the counter, wherever the body is.
fn knuckling(p: Pose) -> Pose {
    grip(p, L, KNUCKLE, Hand::Fist)
}

/// The monkey stance: crouched and slung forward, knees wide, knuckling the counter and scratching its head.
pub fn stance() -> Pose {
    let p = Pose::REST
        .hips(0., -0.015, -0.006)
        .lean(0.55)
        .twist(0.12)
        .tilt(0.10)
        .foot(L, v(0.028, 0., 0.006))
        .foot(R, v(0.026, 0., -0.006))
        .knee(L, WIDE)
        .knee(R, WIDE)
        .face(GRIN);
    scratching(knuckling(p), 0.)
}

/// The idle: it bobs on its knees, looks about and scratches.
pub fn idle(s: &Pose) -> Anim {
    let at = |y: f32, twist: f32, rub: f32| scratching(knuckling(s.hips(s.hips.x, s.hips.y + y, s.hips.z).twist(twist)), rub);
    Anim::new("monkey idle", *s)
        .key(0.4, at(-0.003, 0.12, 1.), Ease::Smooth)
        .key(0.8, at(0., 0.12, -1.), Ease::Smooth)
        .key(1.2, at(-0.003, 0.40, 1.), Ease::Smooth)
        .key(1.6, at(0., 0.40, -1.), Ease::Smooth)
        .key(2.2, at(-0.002, -0.10, 0.), Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .cycle(Cycle::Loop(2.4))
}

/// The walk: a knuckle-walk, rolling from side to side, the long arm swinging forward and planting its knuckles on the
/// counter with each other step while the other hand keeps scratching. Its clock is the stride's.
pub fn walk(s: &Pose) -> Anim {
    let step = |side: usize, reach: f32| {
        let foot = s.legs[side].foot;
        let p = s.hips(s.hips.x - sign(side) * 0.003, s.hips.y + 0.004, s.hips.z).tilt(s.tilt - sign(side) * 0.10).twist(s.twist + sign(side) * 0.08);
        let p = p.kick(side, foot + v(0.002, 0.010, 0.008), v(0.8, 0.4, 1.), 0.3);
        scratching(grip(p, L, KNUCKLE + v(0., 0.004 + 0.010 * (1. - reach), 0.024 * reach), Hand::Fist), sign(side))
    };
    let down = knuckling(s.hips(s.hips.x, s.hips.y - 0.002, s.hips.z).squash(0.04));
    Anim::new("monkey walk", down)
        .key(0.75, step(L, 0.), Ease::Smooth)
        .key(1.5, down, Ease::Smooth)
        .key(2.25, step(R, 1.), Ease::Smooth)
        .key(3., down, Ease::Smooth)
        .cycle(Cycle::Stride)
}

pub fn style() -> Style {
    let s = stance();
    Style {
        name: "Monkey",
        stance: s,
        idle: Some(idle(&s)),
        walk: Some(walk(&s)),
        moves: vec![
            slap(&s),
            swat(&s),
            tumbling_punch(&s),
            rolling_fist(&s),
            cartwheel_kick(&s),
            hop_kick(&s),
            flip_kick(&s),
            leaping_monkey(&s),
            tail_sweep(&s),
            barrage(&s),
        ],
    }
}

/// How far up a pose spun as `p` is must be lifted for its body to be `gap` off the counter (negative: lowered).
fn to_floor(p: &Pose, gap: f32) -> f32 {
    let torso = torso_rotation(p.lean, p.twist, p.tilt);
    let spin = spin_rotation(p.spin);
    let scale = squash_scale(p.squash, p.belly);
    let at = PIVOT + body_frame(p.hips) - torso * (scale * PIVOT);
    let end = |y: f32| CENTRE + spin * (at + torso * (scale * Vec3::Y * y) - CENTRE);
    let low = end(BOTTOM).y.min(end(TOP).y) - RADIUS * scale.x;
    gap - low
}
/// The pose spun forward by `flip` and set down so that its body is `gap` off the counter.
fn rolled(p: Pose, flip: f32, gap: f32) -> Pose {
    let p = p.spin(flip, p.spin.y, p.spin.z);
    let up = to_floor(&p, gap);
    lifted(p, v(0., up, 0.))
}
/// The pose lifted `at_least` off the counter, or more if its body would otherwise be nearer the counter than `gap`.
fn aloft(p: Pose, gap: f32, at_least: f32) -> Pose {
    let up = to_floor(&p, gap).max(at_least);
    lifted(p, v(0., up, 0.))
}
/// Both feet tucked up under the body.
fn tucked(p: Pose) -> Pose {
    p.kick(L, v(0.016, 0.022, 0.012), v(0.3, 1., 0.5), 0.5).kick(R, v(0.016, 0.022, 0.012), v(0.3, 1., 0.5), 0.5)
}
/// Both hands balled up in front of the chest.
fn balled(p: Pose) -> Pose {
    p.arm(L, v(-0.010, 0.0, 0.030), Hand::Fist).arm(R, v(-0.010, 0.0, 0.030), Hand::Fist)
}

/// Light 1, Monkey Slap (high): the right arm drops loose behind, then whips up and across in a backhand, the
/// forearm trailing like a rope.
fn slap(s: &Pose) -> Anim {
    let wound = knuckling(s.twist(-0.40).lean(0.45)).arm(R, v(0.010, -0.042, -0.012), Hand::Palm).elbow(R, v(0.5, -1., -0.8));
    let body = s.twist(0.42).lean(0.38).hips(0., -0.009, -0.002).face(HOOT);
    let hit = grip(knuckling(body), R, v(0.004, 0.136, 0.106), Hand::Palm).elbow(R, v(0.3, -1., -0.6)).palm(R, v(1., 0., -0.3), -0.2);
    let follow = grip(knuckling(body.twist(0.58)), R, v(-0.040, 0.124, 0.090), Hand::Palm).elbow(R, v(0.3, -1., -0.6)).palm(R, v(1., 0., -0.3), -0.2);
    Anim::new("Monkey Slap", *s)
        .key(0.55, wound, Ease::Out)
        .key(1., hit, Ease::In)
        .key(1.6, follow, Ease::Out)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Hand(R))
}

/// Light 2, Monkey Swat (low): it drops lower and the knuckling hand swats across the fighter's shins along the
/// counter, still scratching its head with the other.
fn swat(s: &Pose) -> Anim {
    let low = |twist: f32, rub: f32| scratching(s.hips(0., -0.017, -0.004).lean(0.75).twist(twist).squash(0.05), rub);
    // (The swatting arm's elbow kept up, off the counter.)
    let wound = grip(low(0.35, 1.), L, v(0.064, 0.014, 0.010), Hand::Palm).elbow(L, v(0.6, 1., 0.));
    let hit = grip(low(-0.40, -1.), L, v(-0.010, 0.020, 0.086), Hand::Palm).palm(L, v(-1., 0., 0.3), 0.).elbow(L, v(0.6, 1., 0.)).face(HOOT);
    let follow = grip(low(-0.55, 1.), L, v(-0.046, 0.016, 0.062), Hand::Palm).elbow(L, v(0.6, 1., 0.));
    Anim::new("Monkey Swat", *s)
        .key(0.7, wound, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(1.6, follow, Ease::Out)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Hand(L))
}

/// Light 3, Tumbling Punch (mid): from a low tuck the right fist shoots up and out into the fighter's middle, and the
/// monkey bounces up into a tucked somersault on the spot and lands back in its crouch.
fn tumbling_punch(s: &Pose) -> Anim {
    let tuck = knuckling(s.hips(0., -0.020, -0.004).lean(0.8).twist(-0.3).squash(0.08)).arm(R, v(0., -0.022, -0.010), Hand::Fist);
    let body = s.hips(0., -0.012, 0.004).lean(0.42).twist(0.45).face(Face::FIERCE);
    let hit = grip(knuckling(body), R, v(-0.004, 0.096, 0.116), Hand::Fist);
    let follow = grip(knuckling(body.twist(0.5)), R, v(-0.008, 0.100, 0.118), Hand::Fist);
    let over = |flip: f32, up: f32| aloft(tucked(balled(s.hips(0., -0.004, 0.).lean(0.3).twist(0.).tilt(0.).face(GRIN))).spin(flip, 0., 0.), 0.012, up);
    Anim::new("Tumbling Punch", *s)
        .key(0.62, tuck, Ease::Out)
        .key(1., hit, Ease::In)
        .key(1.6, follow, Ease::Out)
        .key(2.15, over(0.9, 0.035), Ease::In)
        .key(2.4, over(PI, 0.05), Ease::Linear)
        .key(2.65, over(TAU - 1.0, 0.03), Ease::Linear)
        .key(2.85, knuckling(s.spin(TAU, 0., 0.).squash(0.08)), Ease::Out)
        .key(3., s.spin(TAU, 0., 0.), Ease::Smooth)
        .strike(Limb::Hand(R))
}

/// Heavy 1, Rolling Fist (low, carried forward): it dives over its hands into a forward roll along the counter and
/// comes up out of it crouched, both palms driving up under the fighter's chin.
fn rolling_fist(s: &Pose) -> Anim {
    let mv = fu::Move::MonkeyHeavy1;
    let go = sim::travel(fu::Style::Monkey.index(), mv);
    let carry = |beat: f32| carried(fu::Style::Monkey, mv, beat);
    // Each pose of the roll is written where the hotdog is at that beat (the simulation carries it along the counter).
    let roll = |flip: f32, beat: f32| {
        let p = tucked(s.arm(L, v(-0.010, 0.020, 0.040), Hand::Fist).arm(R, v(-0.010, 0.020, 0.040), Hand::Fist).hips(0., 0., 0.).lean(0.4).twist(0.).tilt(0.));
        rolled(p, flip, 0.009).moved(carry(beat))
    };
    let dive = grip(grip(s.hips(0., -0.018, 0.004).lean(1.0).twist(0.).tilt(0.).squash(0.06), L, v(0.018, 0.015, 0.080), Hand::Palm), R, v(0.018, 0.015, 0.080), Hand::Palm).moved(carry(0.25));
    let up = |p: Pose, high: f32, ahead: f32| {
        let p = grip(grip(p, L, v(0.006, high, ahead), Hand::Palm), R, v(0.006, high, ahead), Hand::Palm);
        p.palm(L, v(0., 1., 0.4), -0.6).palm(R, v(0., 1., 0.4), -0.6)
    };
    let body = s.hips(0., -0.012, 0.).lean(0.15).twist(0.).tilt(0.).face(Face::KIAI).spin(TAU, 0., 0.);
    let hit = up(body, 0.120, 0.090).moved(carry(1.));
    let follow = up(body.lean(-0.05).hips(0., -0.004, 0.), 0.146, 0.080).moved(carry(1.8));
    // The roll: keyed every sixteenth of a turn, so the body stays on the counter all the way over.
    let mut anim = Anim::new("Rolling Fist", *s).key(0.25, dive, Ease::Out).key(0.4, roll(1.3, 0.4), Ease::In);
    for i in 1..=8 {
        let f = i as f32 / 8.;
        let beat = 0.4 + 0.47 * f;
        anim = anim.key(beat, roll(1.3 + 3.5 * f, beat), Ease::Linear);
    }
    anim.key(1., hit, Ease::In)
        .key(1.8, follow, Ease::Out)
        .key(3., s.moved(go).spin(TAU, 0., 0.), Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
        .heavy()
}

/// Heavy 2, Cartwheel Kick (breaks a guard, knocks down): it turns side-on and cartwheels on the spot towards the
/// fighter, hands to the counter, and both legs come over the top and chop down on it like an axe.
fn cartwheel_kick(s: &Pose) -> Anim {
    // Turned a quarter to its left, its right side faces the fighter; a roll of -a then tips it over towards it.
    let base = s.hips(0., 0., 0.).lean(0.).twist(0.).tilt(0.).face(Face::FIERCE);
    let wheel = |a: f32, up: f32| lifted(base.spin(0., FRAC_PI_2, -a), v(0., up, 0.));
    let spread = |p: Pose| p.kick(L, v(0.045, 0.030, 0.), v(0., 1., 0.), 0.7).kick(R, v(0.045, 0.030, 0.), v(0., 1., 0.), 0.7);
    // The right hand goes down first, then the left beyond it, nearer the fighter, in line with the wheel.
    let planted = |a: f32, up: f32, right: f32, left: Option<f32>| {
        let p = grip(spread(wheel(a, up)), R, v(0., 0.003, right), Hand::Palm).elbow(R, v(1., 0., 0.));
        match left {
            Some(at) => grip(p, L, v(0., 0.003, at), Hand::Palm).elbow(L, v(1., 0., 0.)),
            None => p.arm(L, v(0.020, 0.060, 0.0), Hand::Palm),
        }
    };
    let side = grip(s.spin(0., FRAC_PI_2 * 0.85, 0.).hips(0., -0.012, 0.).squash(0.05), R, v(0., 0.003, 0.046), Hand::Palm).arm(L, v(0.030, 0.050, -0.010), Hand::Palm);
    let axe = |a: f32, high: f32, ahead: f32| {
        let p = wheel(a, 0.016).arm(L, v(0.020, 0.050, 0.0), Hand::Palm).arm(R, v(0.020, 0.050, 0.0), Hand::Palm);
        kick_at(kick_at(p, R, v(0.004, high, ahead), v(0., 1., 0.), 0.9), L, v(0.010, high + 0.012, ahead - 0.014), v(0., 1., 0.), 0.9)
    };
    let hit = axe(4.25, 0.112, 0.094).face(Face::KIAI);
    let chop = axe(4.9, 0.060, 0.098);
    let down = axe(5.6, 0.014, 0.050);
    // It lands side-on, steadied on the hand nearer the fighter.
    let land = grip(s.spin(0., FRAC_PI_2, -TAU).hips(0., -0.018, 0.).squash(0.08), R, v(0., 0.003, 0.056), Hand::Palm).elbow(R, v(1., 0., 0.)).arm(L, v(0.030, 0.050, -0.010), Hand::Palm);
    Anim::new("Cartwheel Kick", *s)
        .key(0.35, side, Ease::Out)
        .key(0.6, planted(1.25, 0.004, 0.036, None), Ease::In)
        .key(0.82, planted(PI, 0.014, -0.046, Some(0.050)), Ease::Linear)
        .key(1., hit, Ease::Linear)
        .key(1.7, chop, Ease::Out)
        .key(2.0, down, Ease::Smooth)
        .key(2.25, land, Ease::Out)
        .key(2.6, knuckling(s.spin(0., 0.5, -TAU).hips(0., -0.016, 0.)).arm(R, v(0.030, 0.040, 0.020), Hand::Palm), Ease::Smooth)
        .key(3., s.spin(0., 0., -TAU), Ease::Smooth)
        .strike(Limb::Foot(R))
        .trail(Limb::Foot(L))
        .heavy()
}

/// Kick 1, Hop Kick (mid): it springs up off both feet, arms flung up, and snaps the right foot out in the air.
fn hop_kick(s: &Pose) -> Anim {
    let arms = |p: Pose| p.arm(L, v(0.030, 0.078, 0.010), Hand::Palm).arm(R, v(0.030, 0.078, 0.010), Hand::Palm).elbow(L, v(1., 0.3, 0.)).elbow(R, v(1., 0.3, 0.));
    let crouch = grip(grip(s.hips(0., -0.020, -0.004).lean(0.5).squash(0.09), L, v(0.034, 0.012, 0.036), Hand::Fist), R, v(0.034, 0.012, 0.036), Hand::Fist);
    let air = |p: Pose| lifted(p, v(0., 0.022, 0.));
    let rise = air(arms(s.hips(0., 0., 0.).lean(0.1).twist(0.).squash(-0.06))
        .kick(L, v(0.014, 0.018, 0.010), v(0., 0.5, 1.), 0.5)
        .kick(R, v(0.014, 0.030, 0.024), v(0., 0.6, 1.), 0.2));
    let hit = air(arms(s.hips(0., 0., 0.).lean(-0.15).twist(0.).squash(-0.03).face(HOOT))
        .kick(L, v(0.014, 0.018, 0.006), v(0., 0.5, 1.), 0.5)
        .kick(R, v(0.006, 0.078, 0.092), v(0., 1., 0.4), 0.9));
    let land = knuckling(s.hips(0., -0.020, -0.004).squash(0.08));
    Anim::new("Hop Kick", *s)
        .key(0.55, crouch, Ease::Out)
        .key(0.85, rise, Ease::In)
        .key(1., hit, Ease::Snap)
        .key(1.7, hit, Ease::Out)
        .key(2.3, land, Ease::In)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Foot(R))
}

/// Kick 2, Flip Kick (mid): it springs into a backflip and both feet whip up under the fighter's chin on the way over;
/// it lands back in its crouch, arms up for balance.
fn flip_kick(s: &Pose) -> Anim {
    let crouch = s.arm(L, v(0.020, -0.040, -0.040), Hand::Palm).arm(R, v(0.020, -0.040, -0.040), Hand::Palm).hips(0., -0.022, -0.004).lean(0.35).twist(0.).tilt(0.).squash(0.09);
    let reach = s.arm(L, v(0.020, 0.080, -0.006), Hand::Palm).arm(R, v(0.020, 0.080, -0.006), Hand::Palm).hips(0., 0., 0.).lean(-0.2).twist(0.).tilt(0.).squash(-0.04);
    let flip = |a: f32, up: f32, p: Pose| lifted(p.spin(-a, 0., 0.), v(0., up, 0.));
    let take_off = flip(0.7, 0.026, reach.kick(L, v(0.014, 0.012, 0.020), v(0., 0.5, 1.), 0.6).kick(R, v(0.012, 0.010, 0.030), v(0., 0.5, 1.), 0.6));
    let up = flip(1.75, 0.022, reach.face(Face::KIAI));
    let hit = kick_at(kick_at(up, R, v(0.004, 0.124, 0.094), v(0., 0., 1.), 0.9), L, v(0.008, 0.116, 0.080), v(0., 0., 1.), 0.9);
    let over = flip(3.6, 0.045, tucked(reach.lean(0.3)));
    let under = flip(5.4, 0.025, tucked(reach.lean(0.3)));
    let land = s.arm(L, v(0.040, 0.050, 0.020), Hand::Palm).arm(R, v(0.040, 0.050, 0.020), Hand::Palm).hips(0., -0.019, -0.006).squash(0.07).spin(-TAU, 0., 0.);
    Anim::new("Flip Kick", *s)
        .key(0.5, crouch, Ease::Out)
        .key(0.8, take_off, Ease::In)
        .key(1., hit, Ease::Linear)
        .key(1.7, over, Ease::Linear)
        .key(2.2, under, Ease::Linear)
        .key(2.55, land, Ease::Out)
        .key(3., s.spin(-TAU, 0., 0.), Ease::Smooth)
        .strike(Limb::Foot(R))
        .trail(Limb::Foot(L))
}

/// The jump attack, Leaping Monkey: in the air it raises both fists over its head, tips forward and bonks them down on
/// the fighter's head as it starts a somersault, which carries it round to land in its crouch.
fn leaping_monkey(s: &Pose) -> Anim {
    let fists = |p: Pose, up: Vec3| p.arm(L, up, Hand::Fist).arm(R, up, Hand::Fist).elbow(L, v(1., 0., 0.)).elbow(R, v(1., 0., 0.));
    let start = tucked(fists(*s, v(0.0, 0.03, 0.04)).hips(0., 0., 0.).twist(0.).tilt(0.));
    let wound = fists(start, v(-0.040, 0.100, 0.006)).lean(-0.25).spin(-0.3, 0., 0.).face(Face::FIERCE);
    let body = start.lean(0.45).spin(0.55, 0., 0.).face(Face::KIAI);
    let hit = grip(grip(body, L, v(0.008, 0.060, 0.104), Hand::Fist), R, v(0.008, 0.060, 0.104), Hand::Fist).elbow(L, v(1., 0., 0.)).elbow(R, v(1., 0., 0.));
    let over = |a: f32| aloft(balled(tucked(start)).lean(0.4).spin(a, 0., 0.), 0.012, 0.);
    Anim::new("Leaping Monkey", start)
        .key(0.6, wound, Ease::Out)
        .key(1., hit, Ease::In)
        .key(1.35, over(1.5), Ease::Linear)
        .key(1.7, over(2.6), Ease::Linear)
        .key(2.05, over(3.8), Ease::Linear)
        .key(2.35, over(5.0), Ease::Linear)
        .key(2.6, knuckling(s.spin(TAU, 0., 0.).hips(0., -0.019, -0.004).squash(0.07)), Ease::Out)
        .key(3., s.spin(TAU, 0., 0.), Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
}

/// The sweep, Tail Sweep (low, knocks down): down on all fours, it spins a full turn on its hands with the right leg
/// stretched out along the counter like a tail.
fn tail_sweep(s: &Pose) -> Anim {
    let fours = grip(grip(s.hips(0., -0.017, 0.).lean(1.0).twist(0.).tilt(0.).squash(0.04), L, v(0.022, 0.009, 0.070), Hand::Palm), R, v(0.022, 0.009, 0.070), Hand::Palm)
        .foot(L, v(0.012, 0., 0.004))
        .kick(R, v(0.068, 0.003, -0.012), v(0.2, 1., 0.), 0.6)
        .face(HOOT);
    let turn = |a: f32| fours.spin(0., a, 0.);
    Anim::new("Tail Sweep", *s)
        .key(0.6, turn(-1.0), Ease::Out)
        .key(1., turn(FRAC_PI_2), Ease::Snap)
        .key(2., turn(PI + 0.8), Ease::Linear)
        .key(2.5, turn(TAU - 0.2), Ease::Out)
        .key(3., s.spin(0., TAU, 0.), Ease::Smooth)
        .strike(Limb::Foot(R))
}

/// The special, Monkey Tumble Barrage (three hits, carried forward): a cheeky scratch of its head and its bottom, a
/// leaping slap, a somersault into a second slap, then both long arms swing in from the sides and clap together on the
/// fighter's head like a toy monkey's cymbals. A shimmy to finish.
fn barrage(s: &Pose) -> Anim {
    let mv = fu::Move::MonkeySpecial;
    let go = sim::travel(fu::Style::Monkey.index(), mv);
    // How much of the carry the simulation has done by `beat`.
    let by = |beat: f32| carried(fu::Style::Monkey, mv, beat).z / go.z.max(1e-4);
    let cheeky = scratching(s.arm(L, v(0.004, -0.060, -0.032), Hand::Claw).twist(0.4).tilt(0.15), 1.).face(Face { eyes: -0.5, ..HOOT });
    // (A knuckle is put on the counter where the hotdog stands, then the whole pose carried along with it.)
    let dive = knuckling(s.hips(0., -0.017, -0.004).lean(0.8).squash(0.06)).moved(go * by(0.75));
    let air = |p: Pose, beat: f32, up: f32| lifted(p.moved(go * by(beat)), v(0., up, 0.));
    let slapping = |side: usize, beat: f32, spin: f32| {
        let body = s.hips(0., 0., 0.).lean(0.35).twist(-0.45 * sign(side)).tilt(0.).spin(spin, 0., 0.).face(Face::KIAI).moved(go * by(beat));
        let other = 1 - side;
        let p = grip(body, side, v(0.004, 0.124, 0.108 + go.z * by(beat)), Hand::Palm).arm(other, v(0.02, 0.02, -0.02), Hand::Palm);
        lifted(p, v(0., 0.012, 0.))
    };
    let somersault = |a: f32, beat: f32| aloft(tucked(balled(s.hips(0., 0., 0.).lean(0.3).twist(0.).tilt(0.))).spin(a, 0., 0.), 0.012, 0.03).moved(go * by(beat));
    let open = air(
        s.arm(L, v(0.060, 0.050, 0.020), Hand::Palm).arm(R, v(0.060, 0.050, 0.020), Hand::Palm).elbow(L, v(0.3, 1., 0.)).elbow(R, v(0.3, 1., 0.)).hips(0., 0., 0.).lean(0.1).twist(0.).tilt(0.).spin(TAU, 0., 0.).face(HOOT),
        1.5,
        0.03,
    );
    let clap = {
        let body = s.hips(0., 0., 0.).lean(0.4).twist(0.).tilt(0.).spin(TAU, 0., 0.).face(Face::KIAI).moved(go * by(1.67));
        let ahead = 0.100 + go.z * by(1.67);
        let p = grip(grip(body, L, v(0.010, 0.126, ahead), Hand::Palm), R, v(0.010, 0.126, ahead), Hand::Palm);
        lifted(p.palm(L, v(-1., 0., 0.), 0.).palm(R, v(-1., 0., 0.), 0.).elbow(L, v(1., 0., 0.)).elbow(R, v(1., 0., 0.)), v(0., 0.02, 0.))
    };
    let land = knuckling(s.hips(0., -0.019, -0.004).squash(0.07).spin(TAU, 0., 0.)).moved(go);
    let shimmy = |tilt: f32| scratching(s.tilt(tilt).twist(-0.2 * tilt.signum()), tilt.signum()).moved(go).spin(TAU, 0., 0.).face(HOOT);
    let mut anim = Anim::new("Monkey Tumble Barrage", *s).key(0.35, cheeky, Ease::Out).key(0.75, dive, Ease::Smooth).key(1., slapping(R, 1., 0.), Ease::Snap);
    // The somersault between the slaps, keyed every eighth of a turn so that it stays off the counter.
    for i in 1..=6 {
        let f = i as f32 / 7.;
        anim = anim.key(1. + 0.33 * f, somersault(TAU * f, 1. + 0.33 * f), Ease::Linear);
    }
    anim.key(1.33, slapping(L, 1.33, TAU), Ease::In)
        .key(1.5, open, Ease::Out)
        .key(1.67, clap, Ease::Snap)
        .key(2.2, land, Ease::In)
        .key(2.5, shimmy(0.22), Ease::Smooth)
        .key(2.75, shimmy(-0.22), Ease::Smooth)
        .key(3., s.moved(go).spin(TAU, 0., 0.), Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
        .heavy()
}

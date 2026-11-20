// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The Drunken style: unpredictable. It sways off balance over a buckling knee, the right hand held up curled round an
//! imaginary cup, the left loose. Every strike seems to happen by accident: a hiccup pops the cup hand into a face, a
//! sway swings a forearm round, a trip turns into a punch, a lurch into a head-butt, a topple into a shoulder charge or
//! a high kick, and nodding off sweeps its legs along the counter. Its special takes a long swig and whirls round with
//! its arms flung out, hitting everything near it.
//!
//! The timing looks random, but it is the simulation's: each strike's three phases are stretched by the simulation's
//! own factor (`FighterView::scale`), and every key is on a beat, so the blow lands exactly when the simulation says.
//! The look of chance is in the keys: a stall before a sudden lurch, a wobble after.
use super::mantis::{carried, grip, kick_at, lifted, settle};
use super::*;

/// The cup hand: held up in front of the face (from the right shoulder: out, up, forward), its palm turned in.
const CUP: Vec3 = v(0.004, 0.055, 0.050);
/// The free hand hanging loose and low.
const LOOSE: Vec3 = v(0.022, -0.040, 0.022);
/// Half-closed eyes, brows up in the middle, a lopsided half-open mouth: tipsy.
const TIPSY: Face = Face { brow: 0.35, raise: 0.25, mouth: 0.15, eyes: -0.55, ..Face::CALM };
/// A hiccup: eyes popping, mouth open.
const HIC: Face = Face { brow: 0.6, raise: 0.9, mouth: 0.55, eyes: 0.7, ..Face::CALM };
/// Nodding off.
const DOZE: Face = Face { brow: 0.2, raise: -0.2, mouth: 0.35, eyes: -1., ..Face::CALM };

/// The cup hand at `at` (from its shoulder).
fn cup(p: Pose, at: Vec3) -> Pose {
    p.arm(R, at, Hand::Cup).palm(R, v(-1., 0.3, 0.), 0.3).elbow(R, v(1., -0.4, -0.2))
}

/// The drunken stance: tipped over to its left above a buckling left knee, the cup held up, the free hand loose.
pub fn stance() -> Pose {
    let p = Pose::REST
        .hips(-0.004, -0.008, 0.)
        .lean(-0.06)
        .twist(-0.22)
        .tilt(0.24)
        .foot(L, v(0.022, 0., 0.012))
        .foot(R, v(0.020, 0., -0.014))
        .knee(L, v(-0.7, 0.3, 1.))
        .knee(R, v(-0.2, 0.3, 1.))
        .arm(L, LOOSE, Hand::Relaxed)
        .face(TIPSY);
    cup(p, CUP)
}

/// The idle: a slow figure of eight of a sway, the weight rolling from foot to foot, and a hiccup.
pub fn idle(s: &Pose) -> Anim {
    let sway = |tilt: f32, lean: f32, x: f32| s.tilt(tilt).lean(lean).hips(x, s.hips.y, s.hips.z);
    let hic = cup(sway(0.18, 0.04, -0.002).hips(-0.002, -0.002, 0.).squash(-0.07), CUP + v(0., 0.006, 0.004)).face(HIC);
    Anim::new("drunken idle", *s)
        .key(0.5, sway(0.34, -0.12, -0.008), Ease::Smooth)
        .key(1.0, sway(0.14, 0.02, -0.002), Ease::Smooth)
        .key(1.5, sway(0.06, -0.10, 0.004), Ease::Smooth)
        .key(1.95, sway(0.20, 0.04, -0.002), Ease::Smooth)
        .key(2.05, hic, Ease::Snap)
        .key(2.3, sway(0.26, -0.04, -0.005), Ease::Out)
        .key(3., *s, Ease::Smooth)
        .cycle(Cycle::Loop(3.4))
}

/// The walk: a weaving stagger, the body lolling far over to one side then the other, each foot swinging across in
/// front of the other, the cup held carefully level. Its clock is the stride's.
pub fn walk(s: &Pose) -> Anim {
    let step = |side: usize| {
        let foot = s.legs[side].foot;
        let p = s.hips(-sign(side) * 0.006, s.hips.y + 0.002, s.hips.z).tilt(-sign(side) * 0.30).lean(-0.02).twist(s.twist + sign(side) * 0.10);
        cup(p.kick(side, foot + v(-0.008, 0.010, 0.008), v(-0.3, 0.4, 1.), 0.3), CUP + v(0., 0.004, 0.))
    };
    let down = |tilt: f32| s.hips(s.hips.x, s.hips.y - 0.002, s.hips.z).tilt(tilt).squash(0.03);
    Anim::new("drunken walk", down(0.12))
        .key(0.75, step(L), Ease::Smooth)
        .key(1.5, down(-0.10), Ease::Smooth)
        .key(2.25, step(R), Ease::Smooth)
        .key(3., down(0.12), Ease::Smooth)
        .cycle(Cycle::Stride)
}

pub fn style() -> Style {
    let s = stance();
    Style {
        name: "Drunken",
        stance: s,
        idle: Some(idle(&s)),
        walk: Some(walk(&s)),
        moves: vec![
            cup_jab(&s),
            swaying_elbow(&s),
            stumble_punch(&s),
            tipsy_lunge(&s),
            falling_shoulder(&s),
            staggering_kick(&s),
            toppling_kick(&s),
            dive(&s),
            falling_sweep(&s),
            bottomless_gourd(&s),
        ],
    }
}

/// How far the simulation carries a Drunken fighter through `mv` (right, up, forward).
fn carry(mv: fu::Move) -> Vec3 {
    sim::travel(fu::Style::Drunken.index(), mv)
}
/// How much of a move's carry the simulation has done by `beat` (evenly over its startup and active window).
fn by(mv: fu::Move, beat: f32) -> f32 {
    carried(fu::Style::Drunken, mv, beat).length() / carry(mv).length().max(1e-4)
}
/// The pose spun as `spin` says about the counter under `foot` (right, up, forward) instead of about the middle of the
/// body: it topples over that foot, which stays put.
fn toppled(p: Pose, spin: Vec3, foot: Vec3) -> Pose {
    let p = p.spin(spin.x, spin.y, spin.z);
    let r = spin_rotation(p.spin);
    let pivot = body_frame(foot);
    let moved = (pivot - CENTRE) - r * (pivot - CENTRE);
    lifted(p, body_frame(moved))
}

/// The foot on `side` put back flat on the counter at `at` (out, up, forward), wherever a spin has turned the leg.
fn pin(p: Pose, side: usize, at: Vec3) -> Pose {
    let mut p = kick_at(p, side, at, v(0., 0.3, 1.), 0.);
    p.legs[side].plant = 1.;
    p
}

/// Light 1, Cup Jab (high): it stalls, leaning back, then hiccups, and the jolt pops the cup hand into the fighter's
/// face as if by accident. It looks at its hand.
fn cup_jab(s: &Pose) -> Anim {
    let stall = s.lean(-0.14).hips(-0.004, -0.010, -0.003).squash(0.05).tilt(0.30);
    let body = s.lean(0.20).twist(0.30).tilt(0.12).hips(-0.002, -0.004, 0.004).squash(-0.08).face(HIC);
    let hit = grip(body, R, v(-0.006, 0.142, 0.112), Hand::Cup).palm(R, v(0., 0.3, 1.), 0.2);
    let look = cup(hit.squash(0.), v(-0.030, 0.050, 0.070)).face(Face { brow: 0.7, raise: 0.8, mouth: 0.2, eyes: 0.6, ..Face::CALM });
    Anim::new("Cup Jab", *s)
        .key(0.4, stall, Ease::Out)
        .key(0.7, stall.tilt(0.33), Ease::Smooth)
        .key(1., hit, Ease::In)
        .key(1.7, look, Ease::Out)
        .key(2.4, look.tilt(0.30).lean(0.0), Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Hand(R))
}

/// Light 2, Swaying Elbow (mid): it sways far over to its right, then swings back round, lurching a step in, the left
/// arm folded so that its elbow leads into the fighter.
fn swaying_elbow(s: &Pose) -> Anim {
    let away = s.tilt(-0.35).twist(0.45).hips(0.012, -0.008, 0.).arm(L, v(0.050, 0.010, 0.010), Hand::Fist).elbow(L, v(1., 0., 0.2));
    let body = cup(s.tilt(0.18).twist(-0.62).lean(0.14).hips(-0.004, -0.008, 0.006), v(0.030, 0.040, -0.010)).face(Face::FIERCE);
    let hit = grip(body, L, v(-0.024, 0.100, 0.092), Hand::Fist).elbow(L, v(0.6, 0., 1.));
    let follow = grip(body.twist(-0.76).tilt(0.28), L, v(-0.044, 0.096, 0.080), Hand::Fist).elbow(L, v(0.6, 0., 1.));
    Anim::new("Swaying Elbow", *s)
        .key(0.55, away, Ease::Out)
        .key(1., hit, Ease::In)
        .key(1.6, follow, Ease::Out)
        .key(2.3, follow.tilt(0.36).twist(-0.3).hips(-0.004, -0.008, 0.004), Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Elbow(L))
}

/// Light 3, Stumble Punch (mid, carried forward): the right foot catches the left ankle, it pitches forward, and the
/// left fist arrives in the fighter's middle as it falls into a step.
fn stumble_punch(s: &Pose) -> Anim {
    let mv = fu::Move::DrunkenLight3;
    let go = carry(mv);
    let trip = cup(s.lean(-0.18).tilt(0.10).kick(R, v(0.004, 0.008, 0.010), v(-0.5, 0.3, 1.), 0.3), CUP + v(0., 0.02, -0.01)).arm(L, v(0.040, 0.040, 0.0), Hand::Relaxed).face(HIC);
    let pitch = trip.lean(0.45).hips(-0.002, -0.008, go.z * 0.45).foot(L, v(0.020, 0., 0.050)).kick(R, v(0.014, 0.010, -0.010), v(0., 0.3, 1.), 0.4);
    let body = cup(s.lean(0.50).twist(-0.40).tilt(0.05).hips(-0.002, -0.010, go.z * 0.8).foot(L, v(0.018, 0., 0.068)), v(0.030, 0.030, 0.0))
        .kick(R, v(0.014, 0.008, -0.016), v(0., 0.3, 1.), 0.4)
        .face(Face::FIERCE);
    let ahead = go.z * by(mv, 1.);
    let hit = grip(body, L, v(-0.006, 0.098, ahead + 0.116), Hand::Fist);
    let wobble = grip(body.lean(0.42).tilt(0.25).twist(-0.3), L, v(-0.010, 0.094, ahead + 0.108), Hand::Fist);
    Anim::new("Stumble Punch", *s)
        .key(0.5, trip, Ease::Out)
        .key(0.8, pitch, Ease::In)
        .key(1., hit, Ease::Snap)
        .key(1.7, wobble, Ease::Out)
        .key(2.4, s.moved(go).lean(0.15).tilt(0.32), Ease::Smooth)
        .key(3., s.moved(go), Ease::Smooth)
        .strike(Limb::Hand(L))
}

/// Heavy 1, Tipsy Lunge (mid, carried forward): it teeters back with its arms windmilling, then trips forward head first
/// into the fighter, the top of the hotdog the battering ram, teeters on its front foot and rights itself.
fn tipsy_lunge(s: &Pose) -> Anim {
    let go = carry(fu::Move::DrunkenHeavy1);
    let back = cup(s.lean(-0.26).tilt(0.25).hips(-0.004, -0.008, -0.006), v(0.030, 0.070, -0.010)).arm(L, v(0.040, 0.060, -0.010), Hand::Relaxed).face(HIC);
    let teeter = back.lean(-0.30).tilt(0.32);
    let hit = s
        .lean(0.95)
        .tilt(0.0)
        .twist(0.)
        .hips(0., -0.010, go.z * 0.75)
        .foot(L, v(0.018, 0., go.z * 1.1))
        .kick(R, v(0.016, 0.022, -0.020), v(0., 0.3, 1.), 0.6)
        .arm(L, v(0.020, 0.030, -0.040), Hand::Relaxed)
        .arm(R, v(0.020, 0.030, -0.040), Hand::Cup)
        .face(Face::KIAI);
    let rock = hit.lean(0.6).tilt(0.25).arm(R, v(0.020, 0.080, 0.0), Hand::Cup).arm(L, v(0.030, -0.040, 0.010), Hand::Relaxed).hips(0., -0.008, go.z);
    Anim::new("Tipsy Lunge", *s)
        .key(0.5, back, Ease::Out)
        .key(0.72, teeter, Ease::Smooth)
        .key(1., hit, Ease::In)
        .key(1.8, hit.lean(0.85), Ease::Out)
        .key(2.3, rock, Ease::Smooth)
        .key(2.6, s.moved(go).lean(0.2).tilt(-0.15), Ease::Smooth)
        .key(3., s.moved(go), Ease::Smooth)
        .strike(Limb::Head)
        .heavy()
}

/// Heavy 2, Falling Shoulder (mid, breaks a guard, knocks down, carried forward): it wobbles back, then topples forward
/// like a felled tree, shoulder first into the fighter, catches itself on its hands and bounces back up.
fn falling_shoulder(s: &Pose) -> Anim {
    let mv = fu::Move::DrunkenHeavy2;
    let go = carry(mv);
    // It topples forward over its buckling knees, the shins swinging back, wherever the simulation has carried it to.
    let at = |p: Pose, spin: Vec3, beat: f32| toppled(p, spin, v(0., 0.022, 0.006)).moved(go * by(mv, beat));
    let wobble = cup(s.lean(-0.2).tilt(0.2), v(0.020, 0.080, 0.0)).arm(L, v(0.040, 0.060, 0.0), Hand::Relaxed).face(HIC);
    let tree = s.hips(0., 0., 0.).lean(0.).tilt(0.15).twist(-0.6).foot(L, v(0.012, 0., 0.004)).foot(R, v(0.012, 0., -0.004)).arm(L, v(-0.006, 0.030, 0.030), Hand::Fist);
    let teeter = at(tree, v(0.3, 0., 0.), 0.8);
    let hit = at(tree.face(Face::KIAI), v(0.95, 0., 0.), 1.);
    let caught = toppled(tree, v(1.25, 0., 0.), v(0., 0.022, 0.006));
    let caught = grip(grip(caught, L, v(0.026, 0.014, 0.120), Hand::Palm), R, v(0.026, 0.014, 0.120), Hand::Palm).face(Face::DAZED).moved(go);
    let up = s.moved(go).squash(-0.08).lean(-0.1);
    Anim::new("Falling Shoulder", *s)
        .key(0.5, wobble, Ease::Out)
        .key(0.8, teeter, Ease::In)
        .key(1., hit, Ease::Snap)
        .key(1.8, caught, Ease::Out)
        .key(2.2, caught, Ease::Smooth)
        .key(2.55, up, Ease::Out)
        .key(3., s.moved(go), Ease::Smooth)
        .strike(Limb::Head)
        .heavy()
}

/// Kick 1, Staggering Kick (mid): it staggers back onto its rear leg as if falling over, arms windmilling, and the front
/// foot flicks up into the fighter's middle; a hop to get its balance back.
fn staggering_kick(s: &Pose) -> Anim {
    let back = cup(s.lean(-0.30).tilt(0.10).hips(-0.002, -0.010, -0.012), v(0.020, 0.080, 0.0)).arm(L, v(0.050, 0.050, -0.010), Hand::Relaxed).face(HIC);
    let back = back.kick(L, v(0.016, 0.012, 0.020), v(0., 0.5, 1.), 0.3);
    let hit = cup(back.lean(-0.42), v(0.040, 0.060, -0.020)).kick(L, v(0.010, 0.074, 0.086), v(0., 1., 0.5), 0.8).arm(L, v(0.060, 0.010, -0.030), Hand::Relaxed).face(Face::FIERCE);
    let hold = hit.kick(L, v(0.010, 0.078, 0.080), v(0., 1., 0.5), 0.8).lean(-0.46);
    let hop = lifted(s.lean(-0.1).tilt(-0.15).kick(L, v(0.020, 0.010, 0.020), v(0., 0.5, 1.), 0.2), v(0., 0.008, 0.));
    Anim::new("Staggering Kick", *s)
        .key(0.6, back, Ease::Out)
        .key(1., hit, Ease::Snap)
        .key(1.7, hold, Ease::Out)
        .key(2.3, hop, Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Foot(L))
}

/// Kick 2, Toppling Kick (high): it stumbles, turns and topples over sideways onto its left hand, and the right leg
/// swings up high over the top into the fighter's head.
fn toppling_kick(s: &Pose) -> Anim {
    let base = s.hips(0., -0.006, 0.).lean(0.).tilt(0.).twist(0.).foot(L, v(0.014, 0., 0.)).face(Face::FIERCE);
    let stumble = cup(s.tilt(0.32).hips(-0.010, -0.010, 0.), v(0.030, 0.060, 0.0)).arm(L, v(0.040, -0.030, 0.010), Hand::Palm);
    // It topples over its left hip, the left foot staying on the counter under it.
    let topple = |roll: f32, turn: f32, foot: Vec3, high: bool| {
        let p = pin(toppled(base, v(0., turn, roll), v(-0.014, 0.030, 0.)), L, v(0.014, 0., 0.));
        let p = if high { kick_at(p, R, foot, v(0., 1., 0.), 0.9) } else { p.kick(R, foot, v(0.3, 1., 0.), 0.8) };
        cup(grip(p, L, v(0.075, 0.010, 0.010), Hand::Palm), v(0.030, 0.050, 0.0))
    };
    let hit = topple(0.95, 0.9, v(0.0, 0.130, 0.100), true);
    let over = topple(1.0, 1.0, v(-0.024, 0.120, 0.090), true);
    Anim::new("Toppling Kick", *s)
        .key(0.55, stumble, Ease::Out)
        .key(0.8, topple(0.5, 0.7, v(0.030, 0.020, 0.0), false), Ease::In)
        .key(1., hit, Ease::Snap)
        .key(1.7, over, Ease::Out)
        .key(2.35, cup(s.tilt(-0.25).hips(0.008, -0.010, 0.), CUP).face(HIC), Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Foot(R))
}

/// The jump attack, Drunken Dive: in the air it tips over forward and lands on the fighter belly first, arms and legs
/// splayed, then pushes itself back upright.
fn dive(s: &Pose) -> Anim {
    let tuck = |p: Pose| p.kick(L, v(0.016, 0.016, 0.006), v(0., 0.5, 1.), 0.4).kick(R, v(0.016, 0.012, -0.006), v(0., 0.5, 1.), 0.4);
    let start = tuck(*s);
    let wind = cup(tuck(s.lean(-0.25).tilt(0.3)), v(0.030, 0.075, 0.0)).arm(L, v(0.040, 0.070, 0.0), Hand::Relaxed).spin(-0.2, 0., 0.2).face(HIC);
    let splay = |p: Pose| {
        p.arm(L, v(0.050, 0.020, 0.040), Hand::Palm)
            .arm(R, v(0.050, 0.020, 0.040), Hand::Cup)
            .kick(L, v(0.030, 0.010, -0.010), v(0., 1., 0.), 0.8)
            .kick(R, v(0.030, 0.010, -0.010), v(0., 1., 0.), 0.8)
            .lean(0.)
            .tilt(0.)
            .twist(0.)
            .hips(0., 0., 0.)
    };
    let hit = splay(*s).spin(1.35, 0., 0.).face(Face::KIAI);
    let flat = splay(*s).spin(1.45, 0., 0.).face(Face::DAZED);
    let push = grip(grip(s.spin(0.6, 0., 0.).lean(0.).tilt(0.), L, v(0.030, 0.003, 0.080), Hand::Palm), R, v(0.030, 0.003, 0.080), Hand::Palm);
    Anim::new("Drunken Dive", start)
        .key(0.55, wind, Ease::Out)
        .key(1., hit, Ease::In)
        .key(2., flat, Ease::Out)
        .key(2.5, push, Ease::Smooth)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Head)
}

/// The sweep, Falling Sweep (low, knocks down): its eyes droop, it keels over sideways fast asleep and its legs scissor
/// along the counter at the fighter's shins; it jolts awake and springs back up.
fn falling_sweep(s: &Pose) -> Anim {
    let drowsy = s.tilt(0.34).face(DOZE);
    // It keels over onto its left side and lies there, its bun on the counter.
    let base = s.hips(0., -0.004, 0.).lean(0.).tilt(0.).twist(0.).knee(L, v(0., 0.3, 1.)).knee(R, v(0., 0.3, 1.));
    // (The left arm, the one it falls onto, comes forward under its head, kept long so it does not whip as it goes.)
    let base = base.arm(L, v(0.006, 0.030, 0.048), Hand::Relaxed).elbow(L, v(-1., 0.2, 1.)).arm(R, v(0.030, 0.020, 0.020), Hand::Cup).face(DOZE);
    let keel = |roll: f32, gap: f32| settle(base.spin(0., 0., roll), gap);
    let lying = keel(1.40, 0.016);
    // (Lying on its left side its legs point off to its right: the feet are written by how far right of the hips they are.)
    let hip = solve(&lying).skeleton().hip[R];
    let legs = |p: Pose, front: usize, ahead: f32| {
        let back = 1 - front;
        let at = |side: usize, right: f32, up: f32, fwd: f32| v(sign(side) * (hip.x + right), up, fwd);
        kick_at(kick_at(p, front, at(front, 0.008, 0.012, ahead), v(0., 1., 0.3), 0.6), back, at(back, 0.024, 0.010, -0.030), v(0., 1., 0.3), 0.6)
    };
    let falling = kick_at(keel(0.6, 0.0), R, v(0.036, 0.016, 0.036), v(0., 1., 0.5), 0.5);
    let hit = legs(lying, R, 0.090);
    let scissor = legs(lying, L, 0.080);
    let awake = legs(lying, R, 0.020).face(HIC);
    let rising = settle(base.spin(0., 0., 0.7).kick(L, v(0.016, 0.016, 0.006), v(0., 1., 0.5), 0.3).kick(R, v(0.016, 0.016, -0.004), v(0., 1., 0.5), 0.3), 0.004).face(HIC);
    let spring = lifted(s.squash(-0.08).tilt(-0.1), v(0., 0.010, 0.));
    Anim::new("Falling Sweep", *s)
        .key(0.45, drowsy, Ease::Out)
        .key(0.62, drowsy.tilt(0.42), Ease::Smooth)
        .key(0.84, falling, Ease::In)
        .key(1., hit, Ease::Linear)
        .key(2., scissor, Ease::Out)
        .key(2.35, awake, Ease::Smooth)
        .key(2.52, rising, Ease::Smooth)
        .key(2.72, spring, Ease::Out)
        .key(3., *s, Ease::Smooth)
        .strike(Limb::Foot(R))
        .trail(Limb::Foot(L))
}

/// The special, Bottomless Gourd (hits all round, carried forward): head back, it takes a long swig from the cup, then
/// whirls round a full turn on wobbling legs with both arms flung out, staggers, and hiccups.
fn bottomless_gourd(s: &Pose) -> Anim {
    let mv = fu::Move::DrunkenSpecial;
    let go = carry(mv);
    let raise = cup(s.lean(-0.1), v(-0.030, 0.040, 0.030)).palm(R, v(0., 1., 0.3), 0.3);
    let swig = cup(s.lean(-0.45).tilt(0.10).squash(0.05), v(-0.026, 0.074, 0.024)).palm(R, v(0., 0.3, -1.), 0.5).face(Face { brow: 0.5, raise: 0.4, mouth: 0.3, eyes: -0.9, ..Face::CALM });
    let whirl = |turn: f32, tilt: f32, beat: f32| {
        s.arm(L, v(0.070, 0.020, 0.0), Hand::Fist)
            .arm(R, v(0.070, 0.020, 0.0), Hand::Cup)
            .elbow(L, v(0., 1., 0.))
            .elbow(R, v(0., 1., 0.))
            .lean(0.1)
            .twist(0.)
            .tilt(tilt)
            .hips(0., -0.006, 0.)
            .moved(go * by(mv, beat))
            .spin(0., turn, 0.)
            .face(Face::KIAI)
    };
    let stagger = s.tilt(-0.35).hips(0.010, -0.008, 0.).moved(go).spin(0., TAU, 0.);
    let hiccup = cup(s.squash(-0.08).hips(0., -0.002, 0.), CUP + v(0., 0.008, 0.)).moved(go).spin(0., TAU, 0.).face(HIC);
    Anim::new("Bottomless Gourd", *s)
        .key(0.3, raise, Ease::Out)
        .key(0.85, swig, Ease::Smooth)
        .key(1., whirl(1.1, 0.25, 1.), Ease::In)
        .key(1.33, whirl(2.8, -0.20, 1.33), Ease::Linear)
        .key(1.67, whirl(4.4, 0.25, 1.67), Ease::Linear)
        .key(2., whirl(5.7, -0.10, 2.), Ease::Linear)
        .key(2.3, stagger, Ease::Out)
        .key(2.6, hiccup, Ease::Snap)
        .key(3., s.moved(go).spin(0., TAU, 0.), Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
        .heavy()
}

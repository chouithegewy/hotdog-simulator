// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The fifteen moves every style shares, built on the style's own stance (`anims`): the fighting idle, the walk in
//! stance, the high and low blocks, the parry, the dodges left and right, the back hop, the light and heavy hit
//! reactions, the knockdown and getting up, the grab and being thrown, and the victory bow.
//!
//! A shared move has to read on any stance (Tiger's low crouch, Crane's one leg, Snake's coil), so each one starts and
//! ends in the stance it was given and works mostly with the body (hops, recoils, falls) and with hands placed in the
//! body's own frame (`hand_at`). Moves the simulation gives no startup (the parry, the reactions, getting up, the
//! bow) hold the same pose on beat 0 as on the first beat they really have, so the animator's blend carries them in.
use super::*;

/// The plain fighting stance, for a style that has none of its own: left foot forward, knees bent, fists up, the
/// lead hand out in front of the chin and the rear one by it.
pub fn stance() -> Pose {
    Pose::REST
        .hips(0., -0.009, 0.)
        .lean(0.08)
        .twist(-0.3)
        .foot(L, v(0.020, 0., 0.017))
        .foot(R, v(0.022, 0., -0.016))
        .knee(L, v(0.25, 0., 1.))
        .knee(R, v(0.45, 0., 1.))
        .hand_at(L, v(0.010, 0.104, 0.048), Hand::Fist)
        .hand_at(R, v(0.016, 0.100, 0.026), Hand::Fist)
        .elbow(L, v(0.35, -1., -0.2))
        .elbow(R, v(0.6, -1., -0.3))
        .face(Face::FOCUS)
}
/// Both feet on the counter: a raised foot (Crane's) put down beside the other.
pub fn grounded(pose: &Pose) -> Pose {
    let mut p = *pose;
    for i in [L, R] {
        if p.legs[i].plant < 0.5 {
            let other = p.legs[1 - i].foot;
            p.legs[i] = Leg { foot: v(other.x.max(0.016), 0., other.z + 0.022), knee: v(0.3, 0., 1.), toe: 0., plant: 1. };
        }
    }
    p
}
/// `pose` crouched: the hips down, the knees out, the body over them, a raised foot put down.
pub fn crouched(pose: &Pose) -> Pose {
    let mut p = grounded(pose);
    p.hips.y = (p.hips.y - 0.011).max(-0.026);
    p.lean = (p.lean + 0.2).min(0.7);
    for leg in p.legs.iter_mut() {
        leg.foot.x += 0.006;
        leg.knee = v(0.6, 0.1, 1.);
    }
    for arm in p.arms.iter_mut() {
        arm.hand.y -= 0.004;
    }
    p
}
/// `stance` in the air: the knees tucked, the arms out a little for balance.
pub fn airborne(stance: &Pose) -> Pose {
    stance
        .hips(stance.hips.x, stance.hips.y.max(-0.004), stance.hips.z)
        .kick(L, v(0.016, 0.016, 0.014), v(0.2, 0.5, 1.), 0.5)
        .kick(R, v(0.018, 0.012, -0.010), v(0.3, 0.3, 1.), 0.6)
        .nudge(L, v(0.006, 0.004, 0.))
        .nudge(R, v(0.006, 0.004, 0.))
        .squash(-0.04)
}
/// Lying on its back, dazed, arms and legs splayed: where a knockdown ends and getting up begins.
pub fn lying() -> Pose {
    Pose::REST
        .hips(0., -0.012, -0.012)
        .lean(-1.45)
        .kick(L, v(0.026, 0.004, 0.030), v(0.5, 1., 0.2), 0.3)
        .kick(R, v(0.030, 0.006, 0.020), v(0.6, 1., 0.1), 0.4)
        .hand_at(L, v(0.062, 0.010, -0.050), Hand::Relaxed)
        .hand_at(R, v(0.066, 0.012, -0.030), Hand::Relaxed)
        .elbow(L, v(1., 0., 0.))
        .elbow(R, v(1., 0., 0.))
        .face(Face::DAZED)
}

/// The fifteen shared moves on the stance `s` of style number `style`, in `Common`'s order.
pub fn anims(s: &Pose, style: usize) -> Vec<Anim> {
    let s = *s;
    let ground = grounded(&s);
    let travel = |mv: fu::Move| sim::travel(style, mv);
    let mut all = vec![idle(&s), walk(&s), block_high(&s), block_low(&s), parry(&s), dodge(&s, &ground, travel(fu::Move::DodgeLeft), L), dodge(&s, &ground, travel(fu::Move::DodgeRight), R), back_hop(&s, &ground, travel(fu::Move::BackHop)), hit_light(&s), hit_heavy(&s), knockdown(&s), get_up(&s), grab(&s), thrown(&s), victory(&s)];
    // (These are built on every style's stance, however it leans and turns: no hand may end up inside the body.)
    for anim in all.iter_mut() {
        for key in anim.keys.iter_mut() {
            key.pose = clear(key.pose);
        }
        if let Some(miss) = anim.miss.as_mut() {
            for key in miss.keys.iter_mut() {
                key.pose = clear(key.pose);
            }
        }
    }
    all
}
/// `p` with each forearm and hand moved out in front of the body where the stance's lean or turn has put them inside
/// it, each elbow pushed out to the side where it is in the body, and each hand lifted where it would be under the
/// counter (whole-body spins aside: those are the move's own business).
pub fn clear(mut p: Pose) -> Pose {
    let need = RADIUS + LIMB + 0.002;
    for _ in 0..12 {
        let s = solve(&p);
        let k = s.skeleton();
        let (at, chest, scale) = s.torso_place();
        let inside = |q: Vec3| {
            let l = (chest.inverse() * (q - at)) / scale;
            l.distance(Vec3::new(0., l.y.clamp(BOTTOM, TOP), 0.))
        };
        let mut moved = false;
        for i in [L, R] {
            let hand = [k.elbow[i].lerp(k.wrist[i], 0.5), k.wrist[i], k.tip(Limb::Hand(i))].map(inside).into_iter().fold(f32::MAX, f32::min);
            if hand < need {
                p.arms[i].hand += v(0.25, 0., 1.) * (need - hand + 0.001);
                moved = true;
            }
            let elbow = inside(k.elbow[i]);
            if elbow < need {
                p.arms[i].elbow = p.arms[i].elbow.normalize_or(Vec3::X) + v(0.6, 0., 0.);
                p.arms[i].hand += v(1., 0., 0.3) * (need - elbow + 0.001);
                moved = true;
            }
            let low = k.wrist[i].y.min(k.tip(Limb::Hand(i)).y);
            if low < 0.003 && p.spin.length() < 0.1 {
                p.arms[i].hand += v(0., 1., 0.) * (0.004 - low);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    p
}

/// Breathing in the stance: the belly swells, the hips settle, the fists bob. A loop.
fn idle(s: &Pose) -> Anim {
    let s = *s;
    let inhale = s.squash(s.squash - 0.025).nudge(L, v(0., 0.0015, 0.)).nudge(R, v(0., 0.002, 0.)).hips(s.hips.x + 0.0012, s.hips.y + 0.0006, s.hips.z);
    let exhale = s.squash(s.squash + 0.03).nudge(L, v(0., -0.0015, 0.001)).nudge(R, v(0., -0.001, 0.)).hips(s.hips.x - 0.0012, s.hips.y - 0.0012, s.hips.z);
    Anim::new("idle", s).key(0.9, inhale, Ease::Smooth).key(2.1, exhale, Ease::Smooth).key(3., s, Ease::Smooth).cycle(Cycle::Loop(1.9))
}
/// Shuffling in stance: each foot in turn lifts and steps, the body bobs over them, the guard stays up. Its clock is
/// the stride's.
fn walk(s: &Pose) -> Anim {
    let s = grounded(s);
    let step = |side: usize| {
        let foot = s.legs[side].foot;
        let mut p = s.hips(s.hips.x + sign(side) * -0.0015, s.hips.y + 0.0025, s.hips.z).squash(s.squash - 0.03);
        p.legs[side] = Leg { foot: foot + v(0.002, 0.009, 0.004), knee: s.legs[side].knee + v(0., 0.4, 0.), toe: 0.25, plant: 0. };
        p.nudge(L, v(0., 0.002, 0.)).nudge(R, v(0., 0.002, 0.))
    };
    let down = s.hips(s.hips.x, s.hips.y - 0.0015, s.hips.z).squash(s.squash + 0.035);
    Anim::new("walk", down).key(0.75, step(L), Ease::Smooth).key(1.5, down, Ease::Smooth).key(2.25, step(R), Ease::Smooth).key(3., down, Ease::Smooth).cycle(Cycle::Stride)
}
/// Forearms crossed up in front of the face, palms out, braced. Held.
fn block_high(s: &Pose) -> Anim {
    let s0 = *s;
    // (Squared up to the front, whatever the stance's turn: both forearms have to be across the face.)
    let braced = grounded(s).hips(s.hips.x, s.hips.y - 0.003, s.hips.z - 0.002).lean(s.lean.min(0.1) - 0.04).twist(s.twist * 0.3).squash(0.05);
    let up = braced
        .hand_at(L, v(-0.004, 0.152, 0.040), Hand::Palm)
        .hand_at(R, v(-0.002, 0.146, 0.036), Hand::Palm)
        .elbow(L, v(1., -0.3, 0.4))
        .elbow(R, v(1., -0.5, 0.3))
        .palm(L, v(0.2, 0., 1.), -0.4)
        .palm(R, v(0.2, 0., 1.), -0.4)
        .face(Face::FIERCE);
    let held = up.squash(0.065).nudge(L, v(0., -0.001, 0.)).nudge(R, v(0., -0.0012, 0.));
    Anim::new("block high", s0).key(1., up, Ease::Out).key(2., held, Ease::Smooth).key(3., s0, Ease::Smooth).cycle(Cycle::Hold)
}
/// Crouched, forearms down across the shins, palms out. Held.
fn block_low(s: &Pose) -> Anim {
    let s0 = *s;
    let down = crouched(s)
        .twist(s.twist * 0.4)
        .hand_at(L, v(0.008, 0.054, 0.056), Hand::Palm)
        .hand_at(R, v(0.018, 0.050, 0.050), Hand::Palm)
        .elbow(L, v(1., 0.2, 0.3))
        .elbow(R, v(1., 0.1, 0.2))
        .palm(L, v(0.1, -0.3, 1.), -0.3)
        .palm(R, v(0.1, -0.3, 1.), -0.3)
        .face(Face::FIERCE);
    let held = down.squash(0.04).nudge(L, v(0., 0.001, 0.));
    Anim::new("block low", s0).key(1., down, Ease::Out).key(2., held, Ease::Smooth).key(3., s0, Ease::Smooth).cycle(Cycle::Hold)
}
/// The lead palm meets the blow in the middle and sweeps it out and away in a circle; the body turns with it.
fn parry(s: &Pose) -> Anim {
    let s = *s;
    let meet = s.twist(s.twist + 0.15).hand_at(L, v(-0.004, 0.118, 0.070), Hand::Palm).palm(L, v(0., 0., 1.), -0.6).face(Face::FIERCE);
    let sweep = meet.twist(s.twist - 0.35).lean(s.lean - 0.05).hand_at(L, v(0.060, 0.130, 0.050), Hand::Palm).palm(L, v(0.6, 0., 0.8), -0.5);
    let open = sweep.hand_at(L, v(0.070, 0.112, 0.030), Hand::Palm).hand_at(R, v(0.020, 0.108, 0.066), Hand::Palm).palm(R, v(0., 0., 1.), -0.7);
    // (No startup: beat 0 is already the meeting.)
    Anim::new("parry", meet).key(1., meet, Ease::Smooth).key(1.55, sweep, Ease::Out).key(2., open, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Hand(L))
}
/// A hop to one side (`side`: L to the left), `go` the whole way the simulation carries it: a dip, a push off the far
/// foot, tucked in the air leaning into it, a soft landing.
fn dodge(s: &Pose, ground: &Pose, go: Vec3, side: usize) -> Anim {
    let s = *s;
    let way = sign(side);
    let dip = ground.hips(s.hips.x - way * 0.004, s.hips.y - 0.008, s.hips.z).squash(0.1).tilt(way * 0.15).face(Face::FIERCE);
    let air = airborne(&s).moved(go * 0.5 + v(0., 0.016, 0.)).tilt(-way * 0.42).twist(s.twist - way * 0.2).nudge(side, v(0.02, 0.01, 0.)).nudge(1 - side, v(-0.004, 0.008, 0.01));
    let land = ground.moved(go).hips(s.hips.x + go.x, s.hips.y - 0.009, s.hips.z + go.z).squash(0.09).tilt(-way * 0.12);
    let name = if side == L { "dodge left" } else { "dodge right" };
    Anim::new(name, s).key(1., dip, Ease::Out).key(1.5, air, Ease::Out).key(2., land, Ease::In).key(3., s.moved(go), Ease::Smooth)
}
/// A hop straight back, `go` the whole way: the body rocks back over it, arms up in guard.
fn back_hop(s: &Pose, ground: &Pose, go: Vec3) -> Anim {
    let s = *s;
    let dip = ground.hips(s.hips.x, s.hips.y - 0.008, s.hips.z).squash(0.1).lean(s.lean + 0.1).face(Face::FIERCE);
    let air = airborne(&s).moved(go * 0.5 + v(0., 0.018, 0.)).lean(s.lean - 0.3).nudge(L, v(0., 0.008, 0.01)).nudge(R, v(0., 0.008, 0.01));
    let land = ground.moved(go).hips(s.hips.x, s.hips.y - 0.009, s.hips.z + go.z).squash(0.09).lean(s.lean + 0.12);
    Anim::new("back hop", s).key(1., dip, Ease::Out).key(1.5, air, Ease::Out).key(2., land, Ease::In).key(3., s.moved(go), Ease::Smooth)
}
/// Hit: the head snaps back, the guard is knocked open, a wince; then the body comes back up into the stance.
fn hit_light(s: &Pose) -> Anim {
    let s = *s;
    let struck = grounded(&s)
        .lean(s.lean - 0.32)
        .twist(s.twist + 0.18)
        .squash(0.07)
        .hips(s.hips.x, s.hips.y - 0.002, s.hips.z - 0.006)
        .hand_at(L, v(0.040, 0.110, 0.030), Hand::Relaxed)
        .hand_at(R, v(0.046, 0.096, 0.004), Hand::Relaxed)
        .face(Face::HURT);
    let reel = struck.lean(s.lean - 0.38).squash(-0.03).hips(s.hips.x, s.hips.y, s.hips.z - 0.009);
    Anim::new("hit light", struck).key(2., struck, Ease::Smooth).key(2.35, reel, Ease::Out).key(3., s, Ease::Smooth)
}
/// Hit hard: thrown back off the front foot, arms flung, a stumbling step back to catch itself.
fn hit_heavy(s: &Pose) -> Anim {
    let s = *s;
    let g = grounded(&s);
    let struck = g
        .lean(-0.6)
        .twist(s.twist + 0.35)
        .squash(0.1)
        .hips(s.hips.x, s.hips.y - 0.002, s.hips.z - 0.01)
        .kick(L, g.legs[L].foot + v(0., 0.01, 0.006), v(0.2, 0.6, 1.), 0.4)
        .hand_at(L, v(0.070, 0.120, -0.010), Hand::Relaxed)
        .hand_at(R, v(0.070, 0.130, -0.020), Hand::Relaxed)
        .elbow(L, v(1., 0., 0.))
        .elbow(R, v(1., 0., 0.))
        .face(Face::HURT);
    let stagger = g.lean(-0.35).squash(-0.04).hips(s.hips.x, s.hips.y + 0.002, s.hips.z - 0.016).foot(L, v(0.02, 0., -0.004)).hand_at(L, v(0.06, 0.10, 0.0), Hand::Relaxed).hand_at(R, v(0.064, 0.09, -0.004), Hand::Relaxed).face(Face::DAZED);
    let catch = s.lean(s.lean + 0.15).squash(0.07).hips(s.hips.x, s.hips.y - 0.004, s.hips.z - 0.004).face(Face::HURT);
    Anim::new("hit heavy", struck).key(2., struck, Ease::Smooth).key(2.3, stagger, Ease::Out).key(2.65, catch, Ease::Smooth).key(3., s, Ease::Smooth)
}
/// Knocked off its feet: struck, flung back through the air, slammed flat on its back with a bounce, and lying there
/// dazed.
fn knockdown(s: &Pose) -> Anim {
    let s = *s;
    let struck = grounded(&s).lean(-0.55).squash(0.1).hips(s.hips.x, s.hips.y, s.hips.z - 0.008).hand_at(L, v(0.07, 0.12, 0.0), Hand::Relaxed).hand_at(R, v(0.07, 0.13, -0.01), Hand::Relaxed).face(Face::HURT);
    let flying = Pose::REST
        .hips(0., 0.012, -0.03)
        .lean(-1.05)
        .kick(L, v(0.02, 0.03, 0.03), v(0.3, 1., 0.5), 0.5)
        .kick(R, v(0.024, 0.038, 0.02), v(0.4, 1., 0.4), 0.6)
        .hand_at(L, v(0.07, 0.10, -0.03), Hand::Relaxed)
        .hand_at(R, v(0.07, 0.11, -0.05), Hand::Relaxed)
        .elbow(L, v(1., 0.3, 0.))
        .elbow(R, v(1., 0.3, 0.))
        .face(Face::HURT);
    let flat = lying();
    let slam = flat.squash(0.14).hips(0., -0.014, -0.012).face(Face::HURT);
    let bounce = flat.hips(0., -0.004, -0.010).lean(-1.3).squash(-0.04);
    Anim::new("knockdown", struck).key(0.5, flying, Ease::Out).key(1., slam, Ease::In).key(2., slam, Ease::Smooth).key(2.25, bounce, Ease::Out).key(2.5, flat, Ease::In).key(3., flat, Ease::Smooth)
}
/// From flat on its back: a roll up to sitting, feet under it, a spring up into the stance.
fn get_up(s: &Pose) -> Anim {
    let s = *s;
    let flat = lying();
    let sit = Pose::REST
        .hips(0., -0.022, -0.006)
        .lean(-0.55)
        .foot(L, v(0.024, 0., 0.028))
        .foot(R, v(0.026, 0., 0.018))
        .knee(L, v(0.4, 1., 0.6))
        .knee(R, v(0.5, 1., 0.5))
        .hand_at(L, v(0.05, 0.006, -0.02), Hand::Palm)
        .hand_at(R, v(0.054, 0.006, -0.026), Hand::Palm)
        .palm(L, v(0., -1., 0.), 0.)
        .palm(R, v(0., -1., 0.), 0.)
        .face(Face::DAZED);
    let rise = grounded(&s).hips(s.hips.x, s.hips.y - 0.012, s.hips.z).lean(0.45).squash(0.1).face(Face::FIERCE);
    Anim::new("get up", flat).key(2., flat, Ease::Smooth).key(2.35, sit, Ease::Out).key(2.7, rise, Ease::Smooth).key(3., s, Ease::Smooth)
}
/// Both hands shoot out and close on the other hotdog, heave it up and over the shoulder; or, when they close on
/// nothing, are pulled back empty.
fn grab(s: &Pose) -> Anim {
    let s = *s;
    let g = grounded(&s);
    let reach_back = g.lean(-0.05).squash(0.06).hand_at(L, v(0.044, 0.112, 0.020), Hand::Claw).hand_at(R, v(0.048, 0.106, 0.012), Hand::Claw).palm(L, v(-0.6, 0., 0.8), -0.3).palm(R, v(-0.6, 0., 0.8), -0.3).face(Face::FIERCE);
    let lunge = g.moved(v(0., 0., 0.016)).lean(0.35).squash(-0.06).hand_at(L, v(0.024, 0.104, 0.105), Hand::Claw).hand_at(R, v(0.026, 0.096, 0.100), Hand::Claw).palm(L, v(-1., 0., 0.), 0.).palm(R, v(-1., 0., 0.), 0.);
    let caught = lunge.hand_at(L, v(0.016, 0.104, 0.098), Hand::Fist).hand_at(R, v(0.018, 0.096, 0.094), Hand::Fist).squash(0.04);
    let heave = g.moved(v(0., 0., 0.010)).lean(-0.35).twist(s.twist + 0.4).squash(-0.08).hand_at(L, v(0.020, 0.150, 0.050), Hand::Fist).hand_at(R, v(0.030, 0.150, 0.040), Hand::Fist).face(Face::KIAI);
    let over = g.moved(v(0., 0., 0.008)).lean(0.3).twist(s.twist - 0.7).squash(0.08).hand_at(L, v(0.010, 0.100, 0.070), Hand::Relaxed).hand_at(R, v(0.052, 0.090, 0.030), Hand::Relaxed).face(Face::FIERCE);
    let hit = Anim::new("grab", s).key(0.6, reach_back, Ease::Out).key(1., lunge, Ease::Snap).key(2., caught, Ease::Out);
    let miss = hit.clone().key(2.4, lunge.hand_at(L, v(0.018, 0.100, 0.090), Hand::Fist).hand_at(R, v(0.020, 0.094, 0.086), Hand::Fist).lean(0.42).face(Face::HURT), Ease::Out).key(3., s, Ease::Smooth);
    hit.key(2.35, heave, Ease::Out).key(2.7, over, Ease::Snap).key(3., s, Ease::Smooth).strike(Limb::Hand(R)).miss(miss)
}
/// Grabbed, hoisted and flung: arms and legs flailing, the body turned over in the air, coming down on its back.
fn thrown(s: &Pose) -> Anim {
    let held = grounded(s).lean(0.25).squash(-0.06).hips(0., 0.004, 0.).hand_at(L, v(0.04, 0.12, 0.06), Hand::Relaxed).hand_at(R, v(0.044, 0.126, 0.05), Hand::Relaxed).face(Face::HURT);
    let lifted = airborne(s).hips(0., 0.02, 0.).spin(0.5, 0., 0.25).hand_at(L, v(0.07, 0.13, 0.02), Hand::Relaxed).hand_at(R, v(0.07, 0.14, 0.0), Hand::Relaxed).face(Face::DAZED);
    let over = airborne(s).hips(0., 0.024, -0.02).spin(-1.6, 0., 0.5).kick(L, v(0.03, 0.03, 0.03), v(0.5, 1., 0.), 0.6).face(Face::HURT);
    let down = Pose::REST.hips(0., 0.006, -0.02).lean(-1.0).kick(L, v(0.02, 0.03, 0.03), v(0.3, 1., 0.5), 0.5).kick(R, v(0.024, 0.036, 0.02), v(0.4, 1., 0.4), 0.6).hand_at(L, v(0.07, 0.10, -0.03), Hand::Relaxed).hand_at(R, v(0.07, 0.11, -0.05), Hand::Relaxed).face(Face::HURT);
    Anim::new("thrown", held).key(2., held, Ease::Smooth).key(2.35, lifted, Ease::Out).key(2.7, over, Ease::Smooth).key(3., down, Ease::In)
}
/// The winner's salute: feet together, right fist into the left palm before the chest, a bow, and a proud look.
fn victory(s: &Pose) -> Anim {
    let s = *s;
    let tall = Pose::REST
        .hips(0., 0.001, 0.)
        .foot(L, v(0.012, 0., 0.004))
        .foot(R, v(0.012, 0., -0.002))
        .hand_at(L, v(-0.006, 0.106, 0.040), Hand::Palm)
        .hand_at(R, v(0.002, 0.104, 0.038), Hand::Fist)
        .elbow(L, v(1., -0.4, 0.))
        .elbow(R, v(1., -0.4, 0.))
        .palm(L, v(1., 0., 0.), 0.)
        .face(Face::PROUD);
    let bow = tall.lean(0.5).hips(0., -0.002, -0.006).hand_at(L, v(-0.006, 0.090, 0.064), Hand::Palm).hand_at(R, v(0.002, 0.088, 0.062), Hand::Fist).face(Face::CALM);
    let proud = tall.lean(-0.12).squash(-0.05).face(Face::PROUD);
    // (The simulation gives it no startup: beat 0 is the stance, held to beat 2, then the bow, looped.)
    Anim::new("victory", s).key(2., s, Ease::Smooth).key(2.2, tall, Ease::Smooth).key(2.45, bow, Ease::Out).key(2.6, bow, Ease::Smooth).key(2.8, proud, Ease::Smooth).key(3., s, Ease::Smooth)
}

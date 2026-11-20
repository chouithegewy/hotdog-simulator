// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Crane: reach. Tall and light on one leg, the other drawn up with its toes pointed, both arms spread as wings with
//! the hands pinched into beaks. It strikes from furthest away: pecks with the beak, a slapping wing, a double peck
//! that dips the whole body, wings snapped open, a hammer of both wings, a snapping kick and a long high side kick
//! from the raised leg, a diving attack from the air, a low sweep with one leg stretched out, and the leaping flying
//! kick that carries it across the ring.
use super::*;
use fu::Move;

/// A beak on `side` at `at` (body frame): the fingertips pinched, pointing down from a bent wrist.
fn beak(p: Pose, side: usize, at: Vec3) -> Pose {
    p.hand_at(side, at, Hand::Beak).palm(side, v(0., -1., 0.), 1.2)
}
/// A beak pecking forward: the wrist only a little bent, the point leading.
fn peck(p: Pose, side: usize, at: Vec3) -> Pose {
    p.hand_at(side, at, Hand::Beak).palm(side, v(0., -1., 0.), 0.35)
}
/// The standing leg straight under the body, the other drawn up with the toes pointed.
fn one_leg(p: Pose) -> Pose {
    p.foot(R, v(0.012, 0., -0.004)).knee(R, v(0.2, 0., 1.)).kick(L, v(0.010, 0.027, 0.016), v(0.15, 0.5, 1.), 0.9)
}
pub fn stance() -> Pose {
    let p = one_leg(Pose::REST.hips(0., 0.002, 0.).lean(-0.04).twist(-0.2).tilt(0.04))
        .elbow(L, v(0.5, -0.3, -0.5))
        .elbow(R, v(0.4, -0.2, -1.))
        .face(Face { brow: -0.3, raise: 0.1, mouth: 0., eyes: -0.1, puff: 0. });
    let p = beak(p, L, v(0.036, 0.132, 0.056));
    beak(p, R, v(0.086, 0.126, -0.016))
}
pub fn style() -> Style {
    let s = stance();
    Style {
        name: "Crane",
        stance: s,
        idle: Some(idle(&s)),
        walk: None,
        moves: vec![beak_jab(&s), wing_slap(&s), double_peck(&s), spread(&s), hammer(&s), snap_kick(&s), long_kick(&s), dive(&s), wing_sweep(&s), flight(&s)],
    }
    .cleared()
}
/// The crane's idle: balanced on one leg, the wings rising and settling slowly, a little wobble.
fn idle(s: &Pose) -> Anim {
    let s = *s;
    let up = beak(beak(s.tilt(0.07).hips(0.001, 0.003, 0.), L, v(0.040, 0.140, 0.054)), R, v(0.090, 0.136, -0.014)).squash(-0.02);
    let down = beak(beak(s.tilt(0.01).hips(-0.001, 0.001, 0.), L, v(0.034, 0.126, 0.058)), R, v(0.084, 0.118, -0.018)).squash(0.02);
    Anim::new("crane idle", s).key(1., up, Ease::Smooth).key(2., down, Ease::Smooth).key(3., s, Ease::Smooth).cycle(Cycle::Loop(2.4))
}
/// Light 1, Beak Jab: the lead beak drawn back by the cheek, then pecked out at the face.
fn beak_jab(s: &Pose) -> Anim {
    let s = *s;
    let wind = beak(s.lean(-0.1).twist(-0.32), L, v(0.024, 0.140, 0.018)).face(Face::FOCUS);
    let hit = one_leg(s.hips(0., 0.002, 0.008)).lean(0.28).twist(-0.45);
    let hit = beak(peck(hit, L, v(0.004, 0.146, 0.132)), R, v(0.090, 0.130, -0.030)).face(Face::FIERCE);
    let follow = peck(hit.lean(0.3), L, v(0.004, 0.144, 0.136));
    Anim::new("crane beak jab", s).key(0.6, wind, Ease::Out).key(1., hit, Ease::Snap).key(1.5, follow, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(L))
}
/// Light 2, Wing Slap: the rear wing swept wide and round, open-handed, slapping across at head height.
fn wing_slap(s: &Pose) -> Anim {
    let s = *s;
    let wind = beak(s.twist(-0.6), R, v(0.090, 0.140, -0.040)).face(Face::FOCUS);
    let hit = s.twist(0.55).lean(0.16).hips(0., 0.002, 0.006);
    let hit = beak(hit.hand_at(R, v(-0.006, 0.140, 0.122), Hand::Palm).palm(R, v(-1., 0., 0.2), -0.2), L, v(0.070, 0.130, 0.010)).face(Face::FIERCE);
    let follow = hit.twist(0.8).hand_at(R, v(-0.040, 0.132, 0.096), Hand::Palm).palm(R, v(-1., 0., -0.2), -0.2);
    Anim::new("crane wing slap", s).key(0.6, wind, Ease::Out).key(1., hit, Ease::Snap).key(1.6, follow, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(R))
}
/// Light 3, Crane Beak Strike: both beaks raised high as it rears up, then the whole body dips forward and both peck
/// down at the chest, the raised leg swinging out behind.
fn double_peck(s: &Pose) -> Anim {
    let s = *s;
    let rear = beak(beak(s.lean(-0.18).squash(-0.07).hips(0., 0.004, -0.002), L, v(0.030, 0.172, 0.030)), R, v(0.036, 0.168, 0.020)).face(Face::FOCUS);
    let hit = s.lean(0.62).twist(-0.1).hips(0., 0.0, 0.014).kick(L, v(0.012, 0.030, -0.050), v(0.1, -0.6, -1.), 0.9);
    let hit = peck(peck(hit, L, v(0.010, 0.104, 0.124)), R, v(0.016, 0.098, 0.120)).face(Face::FIERCE);
    let follow = peck(peck(hit.lean(0.66), L, v(0.010, 0.094, 0.126)), R, v(0.016, 0.090, 0.122));
    Anim::new("crane beak strike", s).key(0.65, rear, Ease::Out).key(1., hit, Ease::Snap).key(1.6, follow, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(L)).trail(Limb::Hand(R))
}
/// Heavy 1, Spreading Wings: both arms folded across the chest, crouched on the standing leg, then the wings snapped
/// open forward and wide, striking with the backs of both wrists.
fn spread(s: &Pose) -> Anim {
    let s = *s;
    let fold = s.hips(0., -0.008, 0.).squash(0.1).lean(0.25).twist(0.);
    let fold = fold.hand_at(L, v(-0.016, 0.110, 0.056), Hand::Beak).hand_at(R, v(-0.014, 0.100, 0.060), Hand::Beak).palm(L, v(0., 0., -1.), 0.8).palm(R, v(0., 0., -1.), 0.8).elbow(L, v(1., -0.3, 0.8)).elbow(R, v(1., -0.5, 0.8)).face(Face { puff: 0.3, ..Face::FIERCE });
    let hit = s.hips(0., 0.004, 0.012).squash(-0.1).lean(0.12).twist(0.);
    let hit = beak(beak(hit, L, v(0.070, 0.128, 0.086)), R, v(0.072, 0.124, 0.082)).palm(L, v(0., -1., 0.2), 0.9).palm(R, v(0., -1., 0.2), 0.9).face(Face::KIAI);
    let follow = beak(beak(hit.squash(-0.08), L, v(0.088, 0.132, 0.066)), R, v(0.090, 0.128, 0.060));
    Anim::new("crane spreading wings", s).key(0.6, fold, Ease::Out).key(1., hit, Ease::Snap).key(1.7, follow, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(R)).trail(Limb::Hand(L)).heavy()
}
/// Heavy 2, Wing Hammer: up on its toes with both wings raised high, then both hammered down together as the raised
/// foot stamps forward.
fn hammer(s: &Pose) -> Anim {
    let s = *s;
    let up = s.hips(0., 0.008, -0.004).lean(-0.28).squash(-0.1).twist(0.);
    let up = beak(beak(up, L, v(0.060, 0.186, 0.010)), R, v(0.064, 0.184, 0.002)).face(Face::FIERCE);
    let hit = s.hips(0., -0.010, 0.020).lean(0.58).squash(0.12).twist(0.).foot(L, v(0.016, 0., 0.036)).knee(L, v(0.4, 0., 1.)).foot(R, v(0.016, 0., -0.010));
    let hit = hit.hand_at(L, v(0.012, 0.074, 0.120), Hand::Fist).hand_at(R, v(0.014, 0.072, 0.118), Hand::Fist).palm(L, v(1., 0., 0.), 0.).palm(R, v(1., 0., 0.), 0.).face(Face::KIAI);
    let follow = hit.squash(0.14).hand_at(L, v(0.014, 0.060, 0.114), Hand::Fist).hand_at(R, v(0.016, 0.058, 0.112), Hand::Fist);
    let lift = s.lean(0.2).hips(0., -0.004, 0.006).squash(0.05);
    Anim::new("crane wing hammer", s).key(0.6, up, Ease::Out).key(1., hit, Ease::Snap).key(1.7, follow, Ease::Out).key(2.4, lift, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Hand(R)).trail(Limb::Hand(L)).heavy().shake(1.3)
}
/// Kick 1, One-Leg Snap Kick: the raised knee lifts higher, and the foot snaps out pointed into the belly, the body
/// leaning back with the wings out.
fn snap_kick(s: &Pose) -> Anim {
    let s = *s;
    let chamber = s.lean(-0.1).kick(L, v(0.010, 0.050, 0.030), v(0.1, 0.8, 1.), 0.9);
    let chamber = beak(beak(chamber, L, v(0.050, 0.138, 0.030)), R, v(0.086, 0.130, -0.016));
    let hit = s.lean(-0.34).hips(0., 0.002, 0.016).kick(L, v(0.004, 0.074, 0.128), v(0., 0.4, 1.), 1.0);
    let hit = beak(beak(hit, L, v(0.074, 0.136, 0.010)), R, v(0.086, 0.134, -0.028)).face(Face::FIERCE);
    let follow = hit.kick(L, v(0.004, 0.078, 0.132), v(0., 0.4, 1.), 1.0);
    let back = chamber.kick(L, v(0.010, 0.040, 0.026), v(0.1, 0.7, 1.), 0.9);
    Anim::new("crane snap kick", s).key(0.6, chamber, Ease::Out).key(1., hit, Ease::Snap).key(1.45, follow, Ease::Out).key(2.2, back, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Foot(L))
}
/// Kick 2, Long Crane Kick: turned side on and tipped far over on the standing leg, the raised leg drives out high and
/// straight, edge of the foot first, the wings stretched out the other way.
fn long_kick(s: &Pose) -> Anim {
    let s = *s;
    let chamber = s.twist(-0.9).tilt(-0.25).kick(L, v(0.004, 0.060, 0.020), v(-0.2, 1., 0.4), 0.3);
    let chamber = beak(beak(chamber, L, v(0.050, 0.140, 0.040)), R, v(0.090, 0.120, -0.020)).face(Face::FOCUS);
    let hit = s.twist(-1.25).tilt(-0.62).hips(0.006, 0.002, 0.012).kick(L, v(-0.004, 0.140, 0.120), v(-0.2, 1., 0.2), -0.3);
    let hit = beak(beak(hit, L, v(0.050, 0.150, 0.060)), R, v(0.096, 0.070, -0.030)).face(Face::KIAI);
    let follow = hit.kick(L, v(-0.004, 0.146, 0.124), v(-0.2, 1., 0.2), -0.3);
    let back = chamber.kick(L, v(0.008, 0.046, 0.022), v(0., 0.8, 1.), 0.6);
    Anim::new("long crane kick", s).key(0.6, chamber, Ease::Out).key(1., hit, Ease::Snap).key(1.6, follow, Ease::Out).key(2.3, back, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Foot(L))
}
/// Jump, Diving Crane: wings spread wide in the air, then it dives, both beaks pecking down ahead of it and the legs
/// trailing behind.
fn dive(s: &Pose) -> Anim {
    let s = *s;
    let soar = shared::airborne(&s).lean(-0.15).squash(-0.06);
    let soar = beak(beak(soar, L, v(0.090, 0.150, 0.010)), R, v(0.092, 0.148, 0.0)).face(Face::FOCUS);
    let hit = s.lean(0.85).hips(0., 0.004, 0.006).kick(L, v(0.012, 0.030, -0.044), v(0.1, -0.5, -1.), 1.).kick(R, v(0.014, 0.024, -0.048), v(0.1, -0.5, -1.), 1.);
    let hit = peck(peck(hit, L, v(0.012, 0.068, 0.116)), R, v(0.018, 0.062, 0.112)).face(Face::FIERCE);
    let follow = peck(peck(hit.lean(0.9), L, v(0.012, 0.060, 0.120)), R, v(0.018, 0.056, 0.116));
    let land = shared::grounded(&s).squash(0.1).lean(0.25).hips(0., -0.010, 0.);
    Anim::new("diving crane", s).key(0.6, soar, Ease::Out).key(1., hit, Ease::Snap).key(1.7, follow, Ease::Out).key(2.4, land, Ease::In).key(3., s, Ease::Smooth).strike(Limb::Hand(L)).trail(Limb::Hand(R))
}
/// Sweep, Low Wing Sweep: sinks onto the bent standing leg with one wing down on the counter, the other leg stretched
/// out straight along the counter and swept round, wings wide.
fn wing_sweep(s: &Pose) -> Anim {
    let s = *s;
    let low = Pose::REST.hips(0., -0.022, 0.).lean(0.4).squash(0.08).foot(R, v(0.016, 0., -0.004)).knee(R, v(0.6, 0.2, 1.)).kick(L, v(0.096, 0.004, 0.010), v(0.2, 1., 0.3), 0.8).face(Face::FIERCE);
    let low = beak(low.hand_at(R, v(0.040, 0.008, 0.030), Hand::Palm).palm(R, v(0., -1., 0.), 0.), L, v(0.090, 0.090, 0.020));
    let wind = low.spin(0., 0.7, 0.);
    let hit = low.spin(0., -1.3, 0.);
    let follow = low.spin(0., -2.2, 0.);
    let rise = s.spin(0., -4.3, 0.).hips(0., -0.010, 0.).squash(0.06);
    Anim::new("crane low wing sweep", s).key(0.55, wind, Ease::Out).key(1., hit, Ease::Snap).key(1.6, follow, Ease::Out).key(2.3, rise, Ease::Smooth).key(3., s.spin(0., -TAU, 0.), Ease::Smooth).strike(Limb::Foot(L))
}
/// Special, Crane Takes Flight: crouched deep on the one leg with the wings swept back, it springs forward into the
/// air, the leading leg shot out in a flying kick and the wings wide open, and lands far across the ring.
fn flight(s: &Pose) -> Anim {
    let s = *s;
    let go = sim::travel(fu::Style::Crane.index(), Move::CraneSpecial);
    let crouch = s.hips(0., -0.018, -0.006).squash(0.16).lean(0.35).foot(R, v(0.014, 0., -0.004)).knee(R, v(0.3, 0., 1.)).kick(L, v(0.020, 0.022, 0.036), v(0.3, 0.4, 1.), 0.9);
    let crouch = beak(beak(crouch, L, v(0.080, 0.150, -0.030)), R, v(0.084, 0.146, -0.040)).face(Face { puff: 0.5, ..Face::FIERCE });
    let hit = Pose::REST.moved(go * 0.75).hips(go.x, 0.030, go.z * 0.75).lean(-0.3).squash(-0.1).twist(-0.3);
    let hit = hit.kick(L, v(0.004, 0.100, go.z * 0.75 + 0.122), v(0., 0.6, 1.), 1.).kick(R, v(0.016, 0.040, go.z * 0.75 - 0.018), v(0.2, 0.3, 1.), 0.9);
    let hit = beak(beak(hit, L, v(0.096, 0.170, go.z * 0.75 + 0.006)), R, v(0.098, 0.166, go.z * 0.75 - 0.010)).face(Face::KIAI);
    let follow = hit.moved(go * 0.2).kick(L, v(0.004, 0.096, go.z * 0.95 + 0.120), v(0., 0.6, 1.), 1.);
    let land = s.moved(go).squash(0.12).hips(0., -0.012, go.z).lean(0.25);
    let land = shared::grounded(&land);
    Anim::new("crane takes flight", s).key(0.65, crouch, Ease::Out).key(1., hit, Ease::Snap).key(1.75, follow, Ease::Out).key(2.25, land, Ease::In).key(3., s.moved(go), Ease::Smooth).strike(Limb::Foot(L)).trail(Limb::Hand(L)).trail(Limb::Hand(R)).heavy().hitstop(0.11).shake(1.5)
}

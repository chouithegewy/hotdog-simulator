// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Tiger: power. Low and wide, sunk deep between bent knees, body hunched forward over them, both hands raised as
//! claws (fingers spread and curled, palms to the front). Its blows are slow to come and hit hard: claw swipes and
//! rakes, a rising claw, a chambered palm that breaks a guard, an overhead double paw, a stamping kick, a spinning tail
//! kick, a pounce, a low spinning sweep, and the roaring palm that charges in with both hands.
use super::*;
use fu::Move;

/// A claw on `side` at `at` (body frame), palm to the front, the fingers bent back a little.
fn claw(p: Pose, side: usize, at: Vec3) -> Pose {
    p.hand_at(side, at, Hand::Claw).palm(side, v(-0.2, 0.15, 1.), -0.35)
}
/// A palm heel on `side` at `at`, fingers up, palm to the front.
fn palm(p: Pose, side: usize, at: Vec3) -> Pose {
    p.hand_at(side, at, Hand::Palm).palm(side, v(0., -1., 0.15), -1.3)
}
pub fn stance() -> Pose {
    let p = Pose::REST
        .hips(0., -0.016, -0.002)
        .lean(0.2)
        .twist(-0.25)
        .squash(0.04)
        .foot(L, v(0.030, 0., 0.021))
        .foot(R, v(0.033, 0., -0.019))
        .knee(L, v(0.7, 0., 1.))
        .knee(R, v(0.8, 0., 1.))
        .elbow(L, v(0.9, -1., -0.1))
        .elbow(R, v(1., -1., -0.2))
        .face(Face { brow: -0.8, raise: -0.5, mouth: 0.1, eyes: -0.3, puff: 0. });
    let p = claw(p, L, v(0.030, 0.104, 0.054));
    claw(p, R, v(0.040, 0.092, 0.024))
}
pub fn style() -> Style {
    let s = stance();
    Style {
        name: "Tiger",
        stance: s,
        idle: Some(idle(&s)),
        walk: None,
        moves: vec![swipe(&s), rising(&s), rake(&s), palm_strike(&s), crush(&s), stomp(&s), tail_kick(&s), pounce(&s), sweep(&s), roar(&s)],
    }
    .cleared()
}
/// The tiger's idle: it breathes heavily, shoulders rolling, claws flexing.
fn idle(s: &Pose) -> Anim {
    let s = *s;
    let a = s.squash(0.07).hips(0.0015, -0.0175, -0.002).twist(-0.3);
    let a = claw(claw(a, L, v(0.032, 0.100, 0.056)), R, v(0.042, 0.090, 0.022));
    let b = s.squash(0.01).hips(-0.0015, -0.0152, -0.001).twist(-0.2).lean(0.17);
    let b = claw(claw(b, L, v(0.028, 0.108, 0.052)), R, v(0.038, 0.096, 0.026));
    Anim::new("tiger idle", s).key(1., a, Ease::Smooth).key(2., b, Ease::Smooth).key(3., s, Ease::Smooth).cycle(Cycle::Loop(2.2))
}
/// Light 1, Tiger Claw Swipe: the right claw raised high and back, then raked down and across at head height.
fn swipe(s: &Pose) -> Anim {
    let s = *s;
    let wind = claw(s.twist(-0.55).lean(0.12).squash(0.08), R, v(0.060, 0.150, 0.004));
    let wind = claw(wind, L, v(0.024, 0.104, 0.060)).face(Face::FIERCE);
    let hit = s.moved(v(0., 0., 0.010)).twist(0.45).lean(0.32).squash(-0.05);
    let hit = claw(claw(hit, R, v(0.002, 0.136, 0.115)), L, v(0.052, 0.092, 0.024)).face(Face::FIERCE);
    let follow = s.moved(v(0., 0., 0.010)).twist(0.65).lean(0.34);
    let follow = claw(claw(follow, R, v(-0.030, 0.096, 0.090)), L, v(0.056, 0.094, 0.018));
    Anim::new("tiger claw swipe", s).key(0.65, wind, Ease::Out).key(1., hit, Ease::Snap).key(1.6, follow, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(R))
}
/// Light 2, Rising Claw: dips low, the left claw swept up from the hip under the chin.
fn rising(s: &Pose) -> Anim {
    let s = *s;
    let wind = s.hips(0., -0.022, -0.002).squash(0.1).lean(0.34).twist(-0.05);
    let wind = claw(claw(wind, L, v(0.040, 0.060, 0.030)), R, v(0.036, 0.098, 0.030)).face(Face::FIERCE);
    let hit = s.moved(v(0., 0., 0.008)).hips(0., -0.012, 0.006).twist(-0.55).lean(0.12).squash(-0.09);
    let hit = claw(hit, L, v(0.002, 0.148, 0.108)).palm(L, v(0., 0.2, 1.), -0.6);
    let hit = claw(hit, R, v(0.040, 0.096, 0.020)).face(Face::FIERCE);
    let follow = claw(hit.lean(0.02).squash(-0.1), L, v(0.004, 0.166, 0.090)).palm(L, v(0., -0.3, 1.), -0.6);
    Anim::new("tiger rising claw", s).key(0.6, wind, Ease::Out).key(1., hit, Ease::Snap).key(1.55, follow, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(L))
}
/// Light 3, Raking Claws: both claws up, then raked down one after the other as it steps in.
fn rake(s: &Pose) -> Anim {
    let s = *s;
    let wind = s.lean(0.05).squash(-0.04).hips(0., -0.013, -0.004);
    let wind = claw(claw(wind, L, v(0.036, 0.152, 0.036)), R, v(0.046, 0.150, 0.028)).face(Face::FIERCE);
    let first = s.moved(v(0., 0., 0.016)).lean(0.38).twist(0.25).squash(0.04);
    let first = claw(claw(first, R, v(0.004, 0.102, 0.118)), L, v(0.028, 0.136, 0.084)).face(Face::FIERCE);
    let second = s.moved(v(0., 0., 0.016)).lean(0.42).twist(-0.35).squash(0.07);
    let second = claw(claw(second, L, v(0.004, 0.096, 0.116)), R, v(0.034, 0.078, 0.056)).face(Face::FIERCE);
    let low = s.moved(v(0., 0., 0.012)).lean(0.46).twist(-0.2).squash(0.08);
    let low = claw(claw(low, L, v(0.014, 0.068, 0.090)), R, v(0.036, 0.076, 0.050));
    Anim::new("tiger raking claws", s).key(0.65, wind, Ease::Out).key(1., first, Ease::Snap).key(1.5, second, Ease::Snap).key(2.1, low, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(R)).trail(Limb::Hand(L))
}
/// Heavy 1, Tiger Palm: the right palm chambered at the hip with the whole body coiled, then driven out with a step,
/// fingers up, as the left claw pulls back: a shout.
fn palm_strike(s: &Pose) -> Anim {
    let s = *s;
    let coil = s.twist(-0.75).lean(0.1).squash(0.12).hips(0., -0.020, -0.008);
    let coil = claw(palm(coil, R, v(0.044, 0.080, -0.018)), L, v(0.020, 0.108, 0.070)).face(Face::FIERCE);
    let tight = claw(palm(coil.squash(0.14).hips(0., -0.021, -0.010).twist(-0.82), R, v(0.046, 0.078, -0.022)), L, v(0.018, 0.110, 0.074));
    let hit = s.moved(v(0., 0., 0.022)).twist(0.7).lean(0.36).squash(-0.09);
    let hit = claw(palm(hit, R, v(-0.004, 0.102, 0.128)), L, v(0.044, 0.084, 0.0)).face(Face::KIAI);
    let follow = palm(hit.squash(-0.07), R, v(-0.004, 0.104, 0.134));
    let back = s.moved(v(0., 0., 0.014)).twist(0.3).lean(0.28);
    let back = claw(palm(back, R, v(0.020, 0.098, 0.080)), L, v(0.048, 0.096, 0.026)).face(Face::FIERCE);
    Anim::new("tiger palm", s).key(0.45, coil, Ease::Out).key(0.8, tight, Ease::Smooth).key(1., hit, Ease::Snap).key(1.7, follow, Ease::Out).key(2.4, back, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Hand(R)).heavy()
}
/// Heavy 2, Crushing Paw: rears up with both paws high over its head, then brings them crashing down together in
/// front of it, sinking into the blow.
fn crush(s: &Pose) -> Anim {
    let s = *s;
    let rear = s.hips(0., -0.006, -0.008).lean(-0.25).twist(0.).squash(-0.1);
    let rear = claw(claw(rear, L, v(0.030, 0.180, 0.010)), R, v(0.034, 0.178, 0.002)).palm(L, v(0., 0.3, 1.), -0.6).palm(R, v(0., 0.3, 1.), -0.6).face(Face::FIERCE);
    let top = claw(claw(rear.lean(-0.3).squash(-0.12), L, v(0.030, 0.184, 0.0)), R, v(0.034, 0.182, -0.006));
    let hit = s.moved(v(0., 0., 0.018)).hips(0., -0.024, 0.018).lean(0.55).twist(0.).squash(0.14);
    let hit = claw(claw(hit, L, v(0.014, 0.078, 0.112)), R, v(0.016, 0.076, 0.110)).palm(L, v(0., -1., 0.3), 0.).palm(R, v(0., -1., 0.3), 0.).face(Face::KIAI);
    let follow = claw(claw(hit.squash(0.16).hips(0., -0.026, 0.018), L, v(0.016, 0.062, 0.106)), R, v(0.018, 0.060, 0.104));
    Anim::new("tiger crushing paw", s).key(0.55, rear, Ease::Out).key(0.85, top, Ease::Smooth).key(1., hit, Ease::Snap).key(1.7, follow, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(R)).trail(Limb::Hand(L)).heavy().shake(1.3)
}
/// Kick 1, Tiger Stomp Kick: the rear knee chambered high, the foot driven out heel first into the belly, the body
/// leaning back over the standing leg.
fn stomp(s: &Pose) -> Anim {
    let s = *s;
    let chamber = s.lean(-0.05).twist(-0.1).hips(0., -0.012, 0.004).kick(R, v(0.018, 0.040, 0.024), v(0.2, 0.7, 1.), -0.4);
    let chamber = claw(claw(chamber, L, v(0.026, 0.110, 0.050)), R, v(0.044, 0.102, 0.018)).face(Face::FIERCE);
    let hit = s.hips(0., -0.012, 0.016).lean(-0.38).twist(0.15).kick(R, v(0.006, 0.068, 0.112), v(0., 0.5, 1.), -0.5);
    let hit = claw(claw(hit, L, v(0.030, 0.118, 0.046)), R, v(0.050, 0.106, 0.004)).face(Face::FIERCE);
    let follow = hit.kick(R, v(0.006, 0.070, 0.118), v(0., 0.5, 1.), -0.5);
    let back = chamber.kick(R, v(0.020, 0.034, 0.016), v(0.2, 0.7, 1.), -0.2);
    Anim::new("tiger stomp kick", s).key(0.6, chamber, Ease::Out).key(1., hit, Ease::Snap).key(1.5, follow, Ease::Out).key(2.2, back, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Foot(R))
}
/// Kick 2, Tiger Tail Kick: turns its back, and the rear leg lashes out behind it, high, like a tail, the body tipped
/// forward; then round to face again.
fn tail_kick(s: &Pose) -> Anim {
    let s = *s;
    let turn = s.spin(0., 1.3, 0.).lean(0.25).hips(0., -0.014, 0.).kick(R, v(0.018, 0.036, -0.010), v(0.3, 0.6, 1.), 0.);
    let turn = claw(claw(turn, L, v(0.036, 0.106, 0.050)), R, v(0.046, 0.100, 0.020)).face(Face::FIERCE);
    let hit = s.spin(0., 3.0, 0.).twist(0.).lean(0.45).hips(0., -0.012, 0.004).foot(L, v(0.016, 0., 0.004)).kick(R, v(0.004, 0.140, -0.112), v(0.2, -0.4, -1.), -0.3);
    let hit = claw(claw(hit, L, v(0.050, 0.070, 0.050)), R, v(0.060, 0.074, 0.030)).face(Face::KIAI);
    let follow = hit.spin(0., 3.25, 0.).kick(R, v(0.004, 0.146, -0.116), v(0.2, -0.4, -1.), -0.3);
    let round = s.spin(0., 5.0, 0.).lean(0.3).hips(0., -0.014, 0.).kick(R, v(0.02, 0.03, 0.0), v(0.3, 0.6, 1.), 0.2);
    let round = claw(claw(round, L, v(0.034, 0.104, 0.050)), R, v(0.044, 0.096, 0.020));
    Anim::new("tiger tail kick", s).key(0.6, turn, Ease::Out).key(1., hit, Ease::Snap).key(1.6, follow, Ease::Out).key(2.4, round, Ease::Smooth).key(3., s.spin(0., TAU, 0.), Ease::Smooth).strike(Limb::Foot(R)).heavy()
}
/// Jump, Pouncing Claw: in the air, arched back with both claws high, then pouncing down on the other with both.
fn pounce(s: &Pose) -> Anim {
    let s = *s;
    let air = shared::airborne(&s).lean(-0.25).squash(-0.08);
    let air = claw(claw(air, L, v(0.032, 0.172, 0.020)), R, v(0.040, 0.168, 0.010)).face(Face::FIERCE);
    let hit = shared::airborne(&s).lean(0.7).squash(0.04).kick(L, v(0.018, 0.020, -0.020), v(0.3, 0.2, 1.), 0.6).kick(R, v(0.020, 0.026, -0.028), v(0.3, 0.2, 1.), 0.6);
    let hit = claw(claw(hit, L, v(0.014, 0.070, 0.112)), R, v(0.018, 0.066, 0.108)).palm(L, v(0., -0.6, 1.), -0.3).palm(R, v(0., -0.6, 1.), -0.3).face(Face::KIAI);
    let follow = claw(claw(hit.lean(0.75), L, v(0.016, 0.056, 0.104)), R, v(0.020, 0.054, 0.100));
    let land = s.squash(0.12).hips(0., -0.022, 0.004).lean(0.4);
    Anim::new("tiger pouncing claw", s).key(0.6, air, Ease::Out).key(1., hit, Ease::Snap).key(1.7, follow, Ease::Out).key(2.4, land, Ease::In).key(3., s, Ease::Smooth).strike(Limb::Hand(R)).trail(Limb::Hand(L))
}
/// Sweep, Tiger Tail Sweep: drops to the counter on one hand and spins, the leg swept round low like a tail.
fn sweep(s: &Pose) -> Anim {
    let s = *s;
    let low = Pose::REST
        .hips(0., -0.024, 0.)
        .lean(0.55)
        .squash(0.1)
        .foot(L, v(0.026, 0., 0.018))
        .knee(L, v(0.8, 0.2, 1.))
        .kick(R, v(0.090, 0.004, 0.0), v(0., 1., 0.4), 0.4)
        .face(Face::FIERCE);
    let low = claw(low, L, v(0.030, 0.008, 0.040)).palm(L, v(0., -1., 0.), 0.);
    let low = claw(low, R, v(0.040, 0.070, 0.040));
    let wind = low.spin(0., -0.7, 0.).kick(R, v(0.060, 0.006, -0.010), v(0., 1., 0.4), 0.2);
    let hit = low.spin(0., 1.35, 0.);
    let follow = low.spin(0., 2.5, 0.);
    let rise = s.spin(0., 4.6, 0.).hips(0., -0.02, 0.).squash(0.08).lean(0.35);
    Anim::new("tiger tail sweep", s).key(0.55, wind, Ease::Out).key(1., hit, Ease::Snap).key(1.6, follow, Ease::Out).key(2.3, rise, Ease::Smooth).key(3., s.spin(0., TAU, 0.), Ease::Smooth).strike(Limb::Foot(R))
}
/// Special, Roaring Tiger Palm: sinks and gathers both palms at the hips, puffs up, then charges in with a roar and
/// drives both palms into the other together.
fn roar(s: &Pose) -> Anim {
    let s = *s;
    let go = sim::travel(fu::Style::Tiger.index(), Move::TigerSpecial);
    let gather = s.hips(0., -0.025, -0.010).squash(0.15).lean(0.12).twist(0.);
    let gather = palm(palm(gather, L, v(0.036, 0.074, -0.014)), R, v(0.040, 0.072, -0.016)).face(Face { puff: 0.6, ..Face::FIERCE });
    let tight = palm(palm(gather.squash(0.17).hips(0., -0.026, -0.012), L, v(0.034, 0.070, -0.018)), R, v(0.038, 0.068, -0.020)).face(Face { puff: 1., ..Face::FIERCE });
    let hit = s.moved(go + v(0., 0., 0.018)).hips(go.x, -0.012, go.z + 0.016).lean(0.42).twist(0.).squash(-0.1);
    let hit = palm(palm(hit, L, v(0.014, 0.110, go.z + 0.128)), R, v(0.016, 0.094, go.z + 0.126)).face(Face::KIAI);
    let follow = palm(palm(hit.squash(-0.08), L, v(0.014, 0.112, go.z + 0.134)), R, v(0.016, 0.096, go.z + 0.132));
    let settle = s.moved(go).squash(0.08).lean(0.3);
    let settle = palm(palm(settle, L, v(0.024, 0.100, go.z + 0.080)), R, v(0.026, 0.090, go.z + 0.074)).face(Face::PROUD);
    Anim::new("roaring tiger palm", s)
        .key(0.4, gather, Ease::Out)
        .key(0.72, tight, Ease::Smooth)
        .key(1., hit, Ease::Snap)
        .key(1.8, follow, Ease::Out)
        .key(2.45, settle, Ease::Smooth)
        .key(3., s.moved(go), Ease::Smooth)
        .strike(Limb::Hand(R))
        .trail(Limb::Hand(L))
        .heavy()
        .hitstop(0.12)
        .shake(1.6)
}

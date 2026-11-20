// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Snake: speed. Low and coiled, the body bent forward and turned side on, the feet one behind the other, both hands
//! flat with the fingers together like snakes' heads, the lead one out in front and swaying. It darts: quick finger
//! jabs at the face, a double, a coiling pair at the body, both hands spat out together, an elbow that steps in and
//! wraps, a low snap at the shin, a rattling kick, a striking dive from the air, a slithering sweep along the counter,
//! and the hundred fangs, five jabs too fast to follow.
use super::*;
use fu::Move;

/// A snake's head on `side` at `at` (body frame): the fingers straight and together, palm down, the wrist dipped.
fn fang(p: Pose, side: usize, at: Vec3) -> Pose {
    p.hand_at(side, at, Hand::Jab).palm(side, v(-0.25, -1., 0.), 0.2)
}
/// A finger jab on `side` at `at`: the fingers in line with the forearm.
fn jab(p: Pose, side: usize, at: Vec3) -> Pose {
    p.hand_at(side, at, Hand::Jab).palm(side, v(-0.25, -1., 0.), 0.)
}
pub fn stance() -> Pose {
    let p = Pose::REST
        .hips(0., -0.021, 0.)
        .lean(0.44)
        .twist(-0.48)
        .tilt(0.1)
        .squash(0.04)
        .foot(L, v(0.012, 0., 0.032))
        .foot(R, v(0.020, 0., -0.024))
        .knee(L, v(0.2, 0., 1.))
        .knee(R, v(0.5, 0., 1.))
        .elbow(L, v(0.5, -1., -0.3))
        .elbow(R, v(1., -0.7, -0.3))
        .face(Face { brow: -0.5, raise: -0.3, mouth: 0., eyes: -0.55, puff: 0. });
    let p = fang(p, L, v(0.004, 0.086, 0.084)).palm(L, v(-0.25, -1., 0.), 0.45);
    fang(p, R, v(0.034, 0.074, 0.046))
}
pub fn style() -> Style {
    let s = stance();
    Style {
        name: "Snake",
        stance: s,
        idle: Some(idle(&s)),
        walk: None,
        moves: vec![fang_jab(&s), double_fang(&s), coiling(&s), spitting(&s), constrictor(&s), viper(&s), rattle(&s), cobra(&s), slither(&s), hundred(&s)],
    }
    .cleared()
}
/// The snake's idle: the body sways side to side and the lead hand weaves like a head looking for an opening.
fn idle(s: &Pose) -> Anim {
    let s = *s;
    let left = fang(fang(s.hips(-0.002, -0.021, 0.).tilt(0.16).twist(-0.42), L, v(0.016, 0.090, 0.080)), R, v(0.036, 0.076, 0.044));
    let mid = fang(s.hips(0., -0.022, 0.).squash(0.06), L, v(0.004, 0.080, 0.086));
    let right = fang(fang(s.hips(0.002, -0.021, 0.).tilt(0.04).twist(-0.54), L, v(-0.006, 0.088, 0.080)), R, v(0.032, 0.074, 0.048));
    Anim::new("snake idle", s).key(0.75, left, Ease::Smooth).key(1.5, mid, Ease::Smooth).key(2.25, right, Ease::Smooth).key(3., s, Ease::Smooth).cycle(Cycle::Loop(1.5))
}
/// Light 1, Fang Jab: the lead hand pulled back a hair, then darted out at the eyes.
fn fang_jab(s: &Pose) -> Anim {
    let s = *s;
    let wind = fang(s.squash(0.06).lean(0.28), L, v(0.012, 0.100, 0.056));
    let hit = s.lean(0.38).twist(-0.62).squash(-0.07).hips(0., -0.013, 0.006);
    let hit = jab(hit, L, v(0.002, 0.136, 0.116)).face(Face::FIERCE);
    let follow = jab(hit.squash(-0.05), L, v(0.002, 0.134, 0.112));
    Anim::new("snake fang jab", s).key(0.55, wind, Ease::Out).key(1., hit, Ease::Snap).key(1.4, follow, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(L))
}
/// Light 2, Double Fang: the rear hand jabs at the face, and as it comes back the lead one jabs after it.
fn double_fang(s: &Pose) -> Anim {
    let s = *s;
    let wind = fang(s.squash(0.05).twist(-0.56), R, v(0.040, 0.096, 0.030));
    let first = s.twist(0.12).lean(0.4).squash(-0.06).hips(0., -0.013, 0.006);
    let first = fang(jab(first, R, v(-0.002, 0.140, 0.114)), L, v(0.024, 0.096, 0.062)).face(Face::FIERCE);
    let second = s.twist(-0.6).lean(0.4).squash(-0.06).hips(0., -0.013, 0.008);
    let second = fang(jab(second, L, v(0.0, 0.132, 0.116)), R, v(0.040, 0.092, 0.038)).face(Face::FIERCE);
    Anim::new("snake double fang", s).key(0.5, wind, Ease::Out).key(1., first, Ease::Snap).key(1.55, second, Ease::Snap).key(2.1, second, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(R)).trail(Limb::Hand(L))
}
/// Light 3, Coiling Strikes: the body winds down tight and low, then uncoils, the lead hand and then the rear jabbing
/// into the body.
fn coiling(s: &Pose) -> Anim {
    let s = *s;
    let coil = s.hips(0., -0.021, -0.004).squash(0.12).twist(-0.85).lean(0.42).tilt(0.14);
    let coil = fang(fang(coil, L, v(0.024, 0.074, 0.052)), R, v(0.046, 0.070, 0.016)).face(Face::FIERCE);
    let first = s.hips(0., -0.014, 0.010).twist(-0.35).lean(0.45).squash(-0.06).tilt(-0.04);
    let first = fang(jab(first, L, v(0.002, 0.096, 0.122)), R, v(0.036, 0.082, 0.050)).face(Face::FIERCE);
    let second = s.hips(0., -0.014, 0.012).twist(0.2).lean(0.46).squash(-0.06).tilt(-0.08);
    let second = fang(jab(second, R, v(0.0, 0.092, 0.120)), L, v(0.030, 0.090, 0.064));
    Anim::new("snake coiling strikes", s).key(0.55, coil, Ease::Out).key(1., first, Ease::Snap).key(1.6, second, Ease::Snap).key(2.2, second.squash(0.02), Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(L)).trail(Limb::Hand(R))
}
/// Heavy 1, Spitting Palm: both hands drawn in together at the chest like one head rearing, then spat out together
/// with a step.
fn spitting(s: &Pose) -> Anim {
    let s = *s;
    let rear = s.hips(0., -0.019, -0.006).squash(0.12).lean(0.2).twist(-0.2);
    let rear = fang(fang(rear, L, v(0.004, 0.106, 0.050)), R, v(0.012, 0.094, 0.048)).elbow(L, v(1., -0.6, 0.)).elbow(R, v(1., -0.6, 0.)).face(Face { puff: 0.4, ..Face::FIERCE });
    let hit = s.moved(v(0., 0., 0.016)).lean(0.5).twist(-0.15).squash(-0.09);
    let hit = jab(jab(hit, L, v(0.0, 0.108, 0.126)), R, v(0.002, 0.096, 0.122)).face(Face::KIAI);
    let follow = jab(jab(hit.squash(-0.07), L, v(0.0, 0.110, 0.130)), R, v(0.002, 0.098, 0.126));
    Anim::new("snake spitting palm", s).key(0.6, rear, Ease::Out).key(1., hit, Ease::Snap).key(1.65, follow, Ease::Out).key(3., s, Ease::Smooth).strike(Limb::Hand(L)).trail(Limb::Hand(R)).heavy()
}
/// Heavy 2, Constrictor Elbow: a long step in, the body turning through to drive the right elbow into the other's
/// middle, then both arms wrap round as if to squeeze.
fn constrictor(s: &Pose) -> Anim {
    let s = *s;
    let coil = s.twist(-0.95).hips(0., -0.020, -0.006).squash(0.1).lean(0.3);
    let coil = fang(fang(coil, L, v(0.016, 0.100, 0.066)), R, v(0.054, 0.090, -0.004)).face(Face::FIERCE);
    let hit = s.moved(v(0., 0., 0.038)).twist(0.95).lean(0.5).squash(-0.05).hips(0., -0.012, 0.040);
    let hit = hit.hand_at(R, v(0.006, 0.106, 0.068), Hand::Fist).elbow(R, v(0.2, 0., 1.)).palm(R, v(0., -1., 0.), 0.);
    let hit = fang(hit, L, v(0.044, 0.110, 0.034)).face(Face::KIAI);
    let wrap = s.moved(v(0., 0., 0.038)).twist(0.3).lean(0.45).squash(0.06).hips(0., -0.014, 0.040);
    let wrap = fang(fang(wrap, L, v(-0.012, 0.100, 0.104)), R, v(-0.010, 0.092, 0.100)).elbow(L, v(1., 0., 0.6)).elbow(R, v(1., 0., 0.6)).face(Face::FIERCE);
    let back = s.moved(v(0., 0., 0.020)).squash(0.03);
    Anim::new("snake constrictor elbow", s).key(0.6, coil, Ease::Out).key(1., hit, Ease::Snap).key(1.6, wrap, Ease::Smooth).key(2.3, back, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Elbow(R)).heavy()
}
/// Kick 1, Low Viper Kick: barely lifting, the rear foot snaps out at the shin and back.
fn viper(s: &Pose) -> Anim {
    let s = *s;
    let lift = s.hips(0., -0.014, 0.004).kick(R, v(0.018, 0.010, 0.012), v(0.2, 0.4, 1.), 0.4).lean(0.36);
    let hit = s.lean(0.28).twist(-0.3).hips(0., -0.015, 0.010).kick(R, v(0.006, 0.020, 0.112), v(0., 0.4, 1.), 0.9).face(Face::FIERCE);
    let follow = hit.kick(R, v(0.006, 0.018, 0.116), v(0., 0.4, 1.), 0.9);
    let back = lift.kick(R, v(0.018, 0.008, 0.0), v(0.2, 0.4, 1.), 0.3);
    Anim::new("snake low viper kick", s).key(0.55, lift, Ease::Out).key(1., hit, Ease::Snap).key(1.4, follow, Ease::Out).key(2.1, back, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Foot(R))
}
/// Kick 2, Rattle Kick: the lead foot lifted and flicked into the body, rattling there twice before it comes back.
fn rattle(s: &Pose) -> Anim {
    let s = *s;
    let lift = s.lean(0.12).hips(0., -0.013, -0.004).kick(L, v(0.010, 0.040, 0.034), v(0.1, 0.8, 1.), 0.5);
    let hit = s.lean(-0.12).twist(-0.6).hips(0., -0.012, 0.006).foot(R, v(0.016, 0., -0.016)).kick(L, v(0.004, 0.070, 0.114), v(0., 0.5, 1.), 0.9).face(Face::FIERCE);
    let shake = hit.kick(L, v(0.006, 0.064, 0.096), v(0., 0.6, 1.), 0.7);
    let again = hit.kick(L, v(0.004, 0.074, 0.118), v(0., 0.5, 1.), 0.9);
    let back = lift.kick(L, v(0.010, 0.030, 0.034), v(0.1, 0.7, 1.), 0.4);
    Anim::new("snake rattle kick", s).key(0.55, lift, Ease::Out).key(1., hit, Ease::Snap).key(1.33, shake, Ease::Smooth).key(1.66, again, Ease::Snap).key(2.2, back, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Foot(L))
}
/// Jump, Striking Cobra: in the air the body rears back, then lunges out flat, both hands together striking ahead.
fn cobra(s: &Pose) -> Anim {
    let s = *s;
    let rear = shared::airborne(&s).lean(-0.1).squash(-0.04);
    let rear = fang(fang(rear, L, v(0.006, 0.138, 0.048)), R, v(0.030, 0.130, 0.040));
    let hit = s.lean(0.95).hips(0., 0.006, 0.010).kick(L, v(0.010, 0.026, -0.040), v(0.1, -0.5, -1.), 0.9).kick(R, v(0.014, 0.020, -0.046), v(0.1, -0.5, -1.), 0.9);
    let hit = jab(jab(hit, L, v(0.002, 0.070, 0.124)), R, v(0.004, 0.062, 0.120)).face(Face::FIERCE);
    let follow = jab(jab(hit.lean(1.0), L, v(0.002, 0.064, 0.126)), R, v(0.004, 0.056, 0.122));
    let land = s.squash(0.12).hips(0., -0.020, 0.004);
    Anim::new("snake striking cobra", s).key(0.55, rear, Ease::Out).key(1., hit, Ease::Snap).key(1.7, follow, Ease::Out).key(2.4, land, Ease::In).key(3., s, Ease::Smooth).strike(Limb::Hand(L)).trail(Limb::Hand(R))
}
/// Sweep, Slithering Sweep: flat to the counter on one hand, the lead leg slid out along it and swept through, the
/// body undulating behind.
fn slither(s: &Pose) -> Anim {
    let s = *s;
    let low = Pose::REST.hips(0., -0.025, 0.).lean(0.75).squash(0.08).tilt(0.15).foot(R, v(0.020, 0., -0.012)).knee(R, v(0.7, 0.3, 1.)).kick(L, v(0.030, 0.003, 0.100), v(0.3, 1., 0.2), 0.7).face(Face::FIERCE);
    let low = fang(low.hand_at(R, v(0.030, 0.006, 0.020), Hand::Palm).palm(R, v(0., -1., 0.), 0.), L, v(0.034, 0.040, 0.086));
    let wind = low.spin(0., -0.8, 0.).tilt(0.25);
    let hit = low.spin(0., 0.5, 0.).tilt(-0.05);
    let follow = low.spin(0., 1.1, 0.).tilt(0.12);
    let rise = s.hips(0., -0.020, 0.).squash(0.06).lean(0.45);
    Anim::new("snake slithering sweep", s).key(0.5, wind, Ease::Out).key(1., hit, Ease::Snap).key(1.6, follow, Ease::Out).key(2.3, rise, Ease::Smooth).key(3., s, Ease::Smooth).strike(Limb::Foot(L))
}
/// Special, Hundred Fangs: coiled tight with a hiss, then five jabs, left and right, too fast to follow, edging in.
fn hundred(s: &Pose) -> Anim {
    let s = *s;
    let go = sim::travel(fu::Style::Snake.index(), Move::SnakeSpecial);
    let coil = s.hips(0., -0.021, -0.004).squash(0.13).twist(-0.8).lean(0.3);
    let coil = fang(fang(coil, L, v(0.024, 0.100, 0.046)), R, v(0.044, 0.092, 0.020)).face(Face { puff: 0.7, ..Face::FIERCE });
    // (Five blows in the active window: the table's `hits`.)
    let hits = Move::SnakeSpecial.data().hits.max(1) as usize;
    let mut anim = Anim::new("snake hundred fangs", s).key(0.65, coil, Ease::Out);
    for k in 0..hits {
        let side = if k % 2 == 0 { L } else { R };
        let along = go * (0.4 + 0.6 * k as f32 / hits as f32);
        let p = s.moved(along).lean(0.42).twist(if side == L { -0.55 } else { 0.25 }).squash(-0.05);
        let p = jab(p, side, v(0.0, 0.104 + 0.012 * ((k * 7 % 3) as f32 - 1.), along.z + 0.118));
        let p = fang(p, 1 - side, v(0.032, 0.094, along.z + 0.054)).face(Face::KIAI);
        let at = 1. + k as f32 / hits as f32;
        anim = anim.key(at, p, Ease::Snap);
        // (Each hand comes back between its blows.)
        let back = fang(p, side, v(0.010, 0.100, along.z + 0.070));
        anim = anim.key(at + 0.5 / hits as f32, back, Ease::Out);
    }
    let settle = s.moved(go).squash(0.08).lean(0.36);
    anim.key(2.5, settle, Ease::Smooth).key(3., s.moved(go), Ease::Smooth).strike(Limb::Hand(L)).trail(Limb::Hand(R)).heavy()
}

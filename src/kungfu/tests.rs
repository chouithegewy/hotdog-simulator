// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Every style's animations, checked against the simulation's table and against the body: a move for every id, timed
//! as the table times it, landing ahead of the body, no joint bent past what it can, no limb through the sausage, no
//! jump from one frame to the next, and nothing of the body seen from inside in first person.
use super::*;
use fu::{Kind, Move};

fn strikes(kind: Kind) -> bool {
    matches!(kind, Kind::Light | Kind::Heavy | Kind::Kick | Kind::Jump | Kind::Sweep | Kind::Special)
}
/// Every (style, move, animation) the game can ask for.
fn all() -> Vec<(usize, Move, &'static Anim)> {
    let lib = library();
    let mut out = vec![];
    for style in 0..STYLES {
        for mv in Move::ALL {
            if mv.style().is_some_and(|s| s.index() != style) {
                continue;
            }
            out.push((style, mv, lib.anim(style, mv).unwrap_or_else(|| panic!("no animation for {mv:?} in style {style}"))));
        }
    }
    out
}
/// Beats from 0 to 3 in small steps.
fn beats(step: f32) -> impl Iterator<Item = f32> {
    (0..=(3. / step).round() as usize).map(move |i| i as f32 * step)
}

#[test]
fn every_move_of_every_style_has_an_animation() {
    let lib = library();
    assert_eq!(lib.sets.len(), fu::STYLES, "one set per style the simulation has");
    for (i, set) in lib.sets.iter().enumerate() {
        assert_eq!(set.common.len(), 15, "{}", set.name);
        assert_eq!(set.moves.len(), 10, "{}: ten moves in Slot order", set.name);
        assert_eq!(set.name, fu::Style::NAMES[i], "the styles are in the simulation's order");
    }
    for mv in Move::ALL {
        for style in 0..STYLES {
            assert!(lib.anim(style, mv).is_some(), "{mv:?} for style {style}");
        }
    }
    assert_eq!(all().len(), STYLES * 15 + STYLES * 10);
}

#[test]
fn keys_rise_from_beat_zero_to_beat_three() {
    for (style, mv, anim) in all() {
        let keys = &anim.keys;
        assert!(keys.len() >= 2, "{style} {mv:?} has one key");
        assert_eq!(keys[0].at, 0., "{style} {mv:?} starts on beat 0");
        assert!((keys.last().unwrap().at - 3.).abs() < 1e-5, "{style} {mv:?} ends on beat 3");
        for pair in keys.windows(2) {
            assert!(pair[1].at > pair[0].at, "{style} {mv:?}: keys must rise");
        }
        if let Some(miss) = &anim.miss {
            assert!((miss.keys.last().unwrap().at - 3.).abs() < 1e-5);
            // The miss matches the move up to beat 2.
            let a = solve(&anim.pose_at(2.)).skeleton();
            let b = solve(&miss.pose_at(2.)).skeleton();
            assert!(a.wrist[R].distance(b.wrist[R]) < 0.004 && a.wrist[L].distance(b.wrist[L]) < 0.004, "{style} {mv:?}: its miss starts elsewhere");
        }
    }
}

#[test]
fn durations_match_the_simulation_table() {
    for mv in Move::ALL {
        let d = mv.data();
        let t = sim::timing(mv, 1., 0.);
        assert!((t.total() - d.total()).abs() < 1e-5, "{mv:?}");
        if d.total() > 0. && d.kind != Kind::Block {
            assert!((t.beat(d.total()) - 3.).abs() < 1e-4, "{mv:?} ends on beat 3");
            assert!((t.beat(d.startup + 1e-4) - 1.).abs() < 0.01 || d.active == 0. || d.startup == 0., "{mv:?} lands on beat 1");
        }
        // A Drunken strike's phases scaled, a reaction as long as its stun.
        let scaled = sim::timing(mv, 1.2, 0.);
        assert!((scaled.total() - d.total() * 1.2).abs() < 1e-4);
        if d.kind == Kind::Reaction {
            assert!((sim::timing(mv, 1., 0.8).total() - 0.8).abs() < 1e-4);
        }
    }
    // The animator's clock runs on the table's: a move's beat 3 comes when the move ends.
    for (style, mv, anim) in all() {
        let d = mv.data();
        if d.total() <= 0. || anim.cycle != Cycle::Once || d.kind == Kind::Block {
            continue;
        }
        let t = sim::timing(mv, 1., 0.);
        assert!((anim.beat(d.total(), t) - 3.).abs() < 1e-4, "{style} {mv:?}");
        if strikes(d.kind) {
            assert!(anim.strike.is_some(), "{style} {mv:?} says what lands the blow");
            let contact = anim.contact();
            assert!((1.0..=2.0).contains(&contact), "{style} {mv:?}: contact on beat {contact}");
        }
    }
}

/// Distance from `p` (in the torso's own frame) to the sausage's axis.
fn from_axis(p: Vec3) -> f32 {
    let y = p.y.clamp(BOTTOM, TOP);
    p.distance(Vec3::new(0., y, 0.))
}

#[test]
fn poses_are_finite_and_within_the_joints_limits() {
    for (style, mv, anim) in all() {
        let timing = sim::timing(mv, 1., 0.);
        for beat in beats(0.05) {
            // (Where the hotdog stands at that beat: the simulation's carry taken off.)
            let carried = if mv.data().total() > 0. { sim::carried(sim::travel(style, mv), timing, timing.time(beat)) } else { Vec3::ZERO };
            let p = anim.pose_at(beat).moved(-carried);
            let s = solve(&p);
            let tag = format!("{} {mv:?} beat {beat}", library().set(style).name);
            let quats = [s.torso, s.spin].into_iter().chain(s.shoulders).chain(s.elbows).chain(s.wrists).chain(s.hips).chain(s.knees).chain(s.ankles);
            for q in quats {
                assert!(q.is_finite() && (q.length() - 1.).abs() < 1e-3, "{tag}: a joint is not a rotation");
            }
            // (A lunge's body may lag the simulation's carry while it winds up, by up to the whole carry.)
            assert!(s.shift.is_finite() && s.shift.length() < 0.09 + sim::advance(style, mv), "{tag}: hips {}", s.shift);
            for i in [L, R] {
                assert!((1.0..=STRETCH + 1e-3).contains(&s.reach[i]) && (1.0..=STRETCH + 1e-3).contains(&s.stride[i]), "{tag}: stretch");
                let (elbow, _) = s.elbows[i].to_axis_angle();
                let bend = s.elbows[i].to_axis_angle().1;
                assert!(bend <= 2.75 && elbow.is_finite(), "{tag}: elbow bent {bend}");
                let bend = s.knees[i].to_axis_angle().1;
                assert!(bend <= 2.85, "{tag}: knee bent {bend}");
                // The wrist: the hand turned no further than a wrist turns from its forearm.
                let wrist = s.wrists[i] * Vec3::NEG_Y;
                assert!(wrist.dot(Vec3::NEG_Y) > -0.2, "{tag}: hand bent back on itself");
                assert!(p.arms[i].hand.length() < 0.032 * STRETCH, "{tag}: hand out of reach");
            }
            assert!(p.twist.abs() <= 1.7 && p.tilt.abs() <= 1.2 && p.squash.abs() <= 0.2, "{tag}: body {p:?}");
            assert!(p.spin.length() > 0.1 || p.lean.abs() <= 1.75, "{tag}: lean {}", p.lean);
            let f = p.face;
            assert!(f.brow.abs() <= 1.01 && f.raise.abs() <= 1.01 && (0.0..=1.0).contains(&f.mouth) && f.eyes.abs() <= 1.01 && (0.0..=1.0).contains(&f.puff), "{tag}: face");
        }
    }
}

#[test]
fn no_limb_goes_through_the_body_at_a_key() {
    let mut failures = vec![];
    for (style, mv, anim) in all() {
        for key in &anim.keys {
            // KF3: each key as the Animator shows it, the simulation's carry taken off (as the joint-limit and blow
            // tests do): a key of a move that travels while its body spins is written so that this cancels exactly.
            let timing = if mv.data().total() > 0. { sim::timing(mv, 1., 0.) } else { Timing::default() };
            let carried = if mv.data().total() > 0. { sim::carried(sim::travel(style, mv), timing, timing.time(key.at)) } else { Vec3::ZERO };
            let s = solve(&key.pose.moved(-carried));
            let k = s.skeleton();
            let (at, chest, scale) = s.torso_place();
            let local = |p: Vec3| (chest.inverse() * (p - at)) / scale;
            let tag = format!("{} {mv:?} key {}", library().set(style).name, key.at);
            for i in [L, R] {
                // The forearm and the hand stay outside the sausage (a little give for the round of the capsules).
                for (name, p) in [("elbow", k.elbow[i]), ("mid-forearm", k.elbow[i].lerp(k.wrist[i], 0.5)), ("wrist", k.wrist[i]), ("fingers", k.tip(Limb::Hand(i)))] {
                    let d = from_axis(local(p));
                    if d < RADIUS + LIMB - 0.004 {
                        failures.push(format!("{tag}: {name} {i} is {d:.4} from the body's axis"));
                    }
                }
                // The knee and the foot stay clear of the body above the hips.
                for (name, p) in [("knee", k.knee[i]), ("ankle", k.ankle[i])] {
                    let q = local(p);
                    if q.y > BOTTOM + 0.01 && from_axis(q) < RADIUS + LIMB - 0.004 {
                        failures.push(format!("{tag}: {name} {i} inside the body"));
                    }
                }
                // Nothing under the counter (a little give for a sole pressed flat).
                for (name, p) in [("ankle", k.ankle[i]), ("knee", k.knee[i]), ("wrist", k.wrist[i])] {
                    if p.y < -0.003 - key.pose.spin.length().min(1.) * 0.0 {
                        failures.push(format!("{tag}: {name} {i} under the counter at {:.4}", p.y));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{} problems:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn every_blow_lands_ahead_of_the_body() {
    let mut failures = vec![];
    for (style, mv, anim) in all() {
        let d = mv.data();
        if !strikes(d.kind) {
            continue;
        }
        let Some(limb) = anim.strike else { continue };
        let t = sim::timing(mv, 1., 0.);
        let contact = anim.contact();
        // Where the blow is, in the frame of where the hotdog stands at contact (its carry taken off).
        let carried = sim::carried(sim::travel(style, mv), t, t.time(contact));
        let pose = anim.pose_at(contact).moved(-carried);
        let tip = solve(&pose).skeleton().tip(limb);
        let ahead = -tip.z;
        // Two hotdogs touching are 0.09 apart: a blow has to land at least 0.06 ahead, and not much past its reach.
        if !(0.06..=d.reach + 0.03).contains(&ahead) {
            failures.push(format!("{} {mv:?}: {limb:?} lands {ahead:.3} ahead (reach {})", library().set(style).name, d.reach));
        }
        let height = tip.y;
        let want = match d.height {
            fu::Height::High => 0.09..0.22,
            fu::Height::Mid => 0.035..0.17,
            fu::Height::Low => -0.005..0.08,
        };
        if !want.contains(&height) && d.kind != Kind::Jump {
            failures.push(format!("{} {mv:?}: lands at height {height:.3} for a {:?} blow", library().set(style).name, d.height));
        }
    }
    assert!(failures.is_empty(), "{} problems:\n{}", failures.len(), failures.join("\n"));
}

/// The joints' places for a solved pose, for comparing frames.
fn points(s: &Solved) -> Vec<Vec3> {
    let k = s.skeleton();
    let mut v = vec![k.eye, k.head, k.pivot];
    for i in [L, R] {
        v.extend([k.elbow[i], k.wrist[i], k.tip(Limb::Hand(i)), k.knee[i], k.ankle[i], k.tip(Limb::Foot(i))]);
    }
    v
}
fn biggest_step(a: &Solved, b: &Solved) -> f32 {
    points(a).iter().zip(points(b).iter()).map(|(p, q)| p.distance(*q)).fold(0., f32::max)
}

#[test]
fn every_animation_plays_without_a_jump() {
    // At 4000 frames a second no joint moves more than 4 mm a frame (16 m/s: faster than any blow), however the keys ease.
    for (style, mv, anim) in all() {
        let t = if mv.data().total() > 0. { sim::timing(mv, 1., 0.) } else { Timing::default() };
        let dt = 1. / 4000.;
        let mut was: Option<Solved> = None;
        let end = match anim.cycle {
            Cycle::Once | Cycle::Hold => t.total(),
            Cycle::Loop(seconds) => seconds,
            Cycle::Stride => TAU,
        };
        let mut time = 0.;
        while time <= end {
            // KF3: as the Animator shows it, the simulation's carry taken off (see `no_limb_goes_through_the_body_at_a_key`).
            let carried = if anim.cycle == Cycle::Once && mv.data().total() > 0. { sim::carried(sim::travel(style, mv), t, time) } else { Vec3::ZERO };
            let s = solve(&anim.sample(time, t).moved(-carried));
            if let Some(was) = was {
                // (Hand tips are left out: a hand's roll round its forearm is rate-limited by the animator.)
                let step = points(&was).iter().zip(points(&s).iter()).enumerate().filter(|(k, _)| !matches!(k, 5 | 11)).map(|(_, (p, q))| p.distance(*q)).fold(0., f32::max);
                assert!(step < 0.005, "{} {mv:?}: a joint jumps {step:.4} at {time:.4} s", library().set(style).name);
            }
            was = Some(s);
            time += if anim.cycle == Cycle::Stride { dt * 15. } else { dt };
        }
        // A loop comes round to where it began.
        if matches!(anim.cycle, Cycle::Loop(_) | Cycle::Stride) {
            let (a, b) = (solve(&anim.pose_at(0.)), solve(&anim.pose_at(3.)));
            assert!(biggest_step(&a, &b) < 1e-4, "{style} {mv:?}: the loop does not close");
        }
    }
}

#[test]
fn moves_blend_into_one_another_and_the_stance_without_a_jump() {
    // A fight in miniature, frame by frame: the stance, a light chained into the next, a heavy that lands (hit-stop),
    // hit back, knocked down, up again, a style change, walking off. No joint jumps between frames.
    let dt = 1. / 4000.;
    for style in 0..STYLES {
        let mut animator = Animator::default();
        let gait = Solved::from_gait(&crate::player_model::pose(crate::player_model::Motion::default()));
        let moves = [Move::Idle, Move::from_u8(15 + style as u8 * 10).unwrap(), Move::from_u8(16 + style as u8 * 10).unwrap(), Move::from_u8(18 + style as u8 * 10).unwrap(), Move::HitHeavy, Move::KnockDown, Move::GetUp, Move::BlockHigh, Move::Idle];
        let mut was: Option<Solved> = None;
        let mut clock = 0f32;
        for (n, mv) in moves.iter().enumerate() {
            let timing = sim::timing(*mv, 1., 0.);
            // Chains cut in once the active window is over; the rest play out (the block and the stance for a while).
            let length = match mv.data().kind {
                Kind::Stance | Kind::Block => 0.3,
                _ if n == 1 => timing.startup + timing.active + 0.02,
                _ => timing.total(),
            };
            let mut t = 0.;
            while t < length {
                let f = sim::Fighter {
                    style: if n >= 7 { (style + 1) % STYLES } else { style },
                    action: (mv.data().kind != Kind::Stance).then_some(*mv),
                    t,
                    timing,
                    connected: n == 3 && t > timing.startup,
                    held: mv.data().kind == Kind::Block,
                    carried: sim::carried(sim::travel(style, *mv), timing, t),
                    ..default()
                };
                let moving = Locomotion { forward: if n == 8 { 0.8 } else { 0. }, phase: clock * 15., ..default() };
                let frame = animator.update(&f, moving, &gait, dt);
                if let Some(was) = was {
                    let step = biggest_step(&was, &frame.solved);
                    assert!(step < 0.005, "style {style}: a jump of {step:.4} at {mv:?} t {t:.3}");
                }
                was = Some(frame.solved);
                t += dt;
                clock += dt;
            }
        }
    }
}

#[test]
fn a_blow_that_lands_stands_still_on_contact_and_still_ends_with_the_move() {
    let mv = Move::TigerHeavy1;
    let timing = sim::timing(mv, 1., 0.);
    let anim = library().anim(0, mv).unwrap();
    let contact = timing.time(anim.contact());
    let mut animator = Animator::default();
    let gait = solve(&library().set(0).stance);
    let dt = 1. / 600.;
    let mut t = 0.;
    let mut frames = vec![];
    while t < timing.total() + dt {
        let f = sim::Fighter { action: Some(mv), t, timing, connected: t >= contact, ..default() };
        frames.push((t, animator.update(&f, Locomotion::default(), &gait, dt)));
        t += dt;
    }
    let at = |when: f32| frames.iter().min_by(|a, b| (a.0 - when).abs().total_cmp(&(b.0 - when).abs())).unwrap().1.solved;
    assert_eq!(frames.iter().filter(|(_, f)| f.landed.is_some()).count(), 1, "the blow lands once");
    let stop = anim.stop();
    assert!(biggest_step(&at(contact + 0.005), &at(contact + stop - 0.005)) < 1e-4, "frozen on contact");
    assert!(biggest_step(&at(contact + stop + 0.03), &at(contact)) > 1e-3, "and moving again after");
    let end = solve(&anim.pose_at(3.));
    assert!(biggest_step(&frames.last().unwrap().1.solved, &end) < 2e-3, "it still ends with the move");
}

#[test]
fn a_block_is_held_for_as_long_as_the_simulation_holds_it() {
    let mv = Move::BlockHigh;
    let timing = sim::timing(mv, 1., 0.);
    let anim = library().anim(1, mv).unwrap();
    let up = solve(&anim.pose_at(1.)).skeleton();
    let mut animator = Animator::default();
    let gait = solve(&library().set(1).stance);
    let mut last = None;
    for i in 0..600 {
        let f = sim::Fighter { style: 1, action: Some(mv), t: i as f32 / 300., timing, held: true, ..default() };
        last = Some(animator.update(&f, Locomotion::default(), &gait, 1. / 300.).solved.skeleton());
    }
    let held = last.unwrap();
    assert!(held.wrist[L].distance(up.wrist[L]) < 0.004 && held.wrist[R].distance(up.wrist[R]) < 0.004, "the guard stays up after two seconds");
}

#[test]
fn first_person_never_shows_the_inside_of_the_body() {
    for (style, mv, anim) in all() {
        let stance = solve(&library().set(style).stance);
        for beat in beats(0.1) {
            let s = solve(&anim.pose_at(beat));
            let p = view::place(&s, &stance);
            for i in [L, R] {
                for (a, b) in [p.upper[i], p.fore[i], p.shin[i]] {
                    let d = b - a;
                    let t = (-a.dot(d) / d.length_squared().max(1e-9)).clamp(0., 1.);
                    let gap = (a + d * t).length();
                    assert!(gap >= view::CLEAR - 1e-4, "{style} {mv:?} beat {beat}: a limb {gap:.4} from the eye");
                    assert!(gap > LIMB + 0.004, "the near plane would cut a limb open");
                }
            }
        }
    }
    // Nothing of the sausage hangs from the first person's limbs: only arms, hands, shins and feet are built.
    assert!(view::CLEAR > LIMB * 2.);
}

#[test]
fn a_hand_strike_comes_into_the_first_person_view() {
    // At contact the striking hand is ahead of the eye and inside the picture (85 degrees tall, 16:9).
    let half = (85f32.to_radians() * 0.5).tan();
    for style in 0..3 {
        for slot in 0..10 {
            let mv = Move::from_u8(15 + style as u8 * 10 + slot).unwrap();
            let anim = library().anim(style, mv).unwrap();
            let Some(Limb::Hand(i)) = anim.strike else { continue };
            let stance = solve(&library().set(style).stance);
            let p = view::place(&solve(&anim.pose_at(anim.contact())), &stance);
            let tip = p.fore[i].1 + p.hand[i] * Vec3::NEG_Y * 0.008;
            assert!(tip.z < -0.02, "{mv:?}: the fist is not ahead of the eye ({tip})");
            let (x, y) = (tip.x / -tip.z, tip.y / -tip.z);
            assert!(y.abs() < half && x.abs() < half * 16. / 9., "{mv:?}: the fist is out of the picture ({x:.2}, {y:.2})");
        }
    }
}

#[test]
fn the_hands_and_feet_are_closed_solid_shapes_that_face_outwards() {
    let shapes: Vec<(String, crate::viewmodel::Shape)> = Hand::ALL.iter().map(|h| (format!("{h:?}"), rig::hand_shape(*h))).chain([("foot".to_string(), rig::foot_shape())]).collect();
    for (name, shape) in shapes {
        assert!(shape.positions.len() == shape.normals.len() && !shape.indices.is_empty(), "{name}");
        assert!(shape.positions.iter().all(|p| p.is_finite() && p.length() < 0.03), "{name}: bigger than a hand");
        for t in shape.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| shape.positions[i as usize]);
            let out: Vec3 = t.iter().map(|i| shape.normals[*i as usize]).sum();
            assert!((b - a).cross(c - a).dot(out) >= 0., "{name}: a triangle faces inwards");
        }
    }
}

#[test]
fn styles_read_from_their_silhouettes() {
    // Tiger low and wide, Crane tall on one leg with its arms out, Snake low and coiled with its hands forward.
    let lib = library();
    let stance = |style: usize| solve(&lib.set(style).stance).skeleton();
    let (tiger, crane, snake) = (stance(0), stance(1), stance(2));
    let width = |k: &Skeleton| (k.ankle[R].x - k.ankle[L].x).abs();
    let span = |k: &Skeleton| (k.wrist[R].x - k.wrist[L].x).abs();
    assert!(tiger.head.y < crane.head.y - 0.01 && width(&tiger) > width(&crane) + 0.01, "the tiger is low and wide");
    assert!(crane.ankle.iter().any(|a| a.y > 0.02), "the crane stands on one leg");
    assert!(span(&crane) > span(&tiger), "the crane's wings are out");
    assert!(snake.head.y < crane.head.y - 0.01 && snake.wrist.iter().all(|w| w.z < -0.03), "the snake is low, its hands forward");
}

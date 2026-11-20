// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The head nods with the look, and the rounds find it where it is (head.rs): the nod at every stance, the body that
//! never moves, the user's case (a hotdog looking down is harder to shoot in the head from behind), the face that turns
//! with the head, the blast, the world's collision, determinism and the protocol's precision.
use super::*;
use crate::head::{self, BOW, HEAD_AT, HEAD_RADIUS, Head, NECK, PRONE_BOW, Part, TIP};
use glam::Quat;
use std::f32::consts::{FRAC_PI_2, PI};

/// A hotdog standing at (1, FLOOR, 1) facing -z (yaw 0) in `stance` (0 standing, 1 crouched, 2 prone, 3 tucked in the
/// air 0.1 m up), looking `pitch`.
fn dog(stance: usize, pitch: f32) -> Player {
    let mut p = Player { id: 2, alive: true, hp: MAX_HP, grounded: stance != 3, pitch, ..Player::default() };
    p.p = [1., FLOOR + if stance == 3 { 0.1 } else { 0. }, 1.];
    (p.crouch, p.prone) = (stance == 1 || stance == 3, stance == 2);
    p
}
const STANCES: [usize; 4] = [0, 1, 2, 3];

/// The shape before the head nodded: one sphere about the chest, 0.055 m round, and the face on it.
fn old_hit(p: &Player, o: Vec3, d: Vec3, size: f32) -> Option<f32> {
    crate::ray_sphere(o, d, chest(p), 0.055 + size)
}
fn old_face(p: &Player, point: Vec3, dir: Vec3) -> bool {
    let n = (point - chest(p)).normalize_or_zero();
    let ahead = Vec3::new(-p.yaw.sin(), 0., -p.yaw.cos());
    if -dir.dot(ahead) < 0.25 {
        return false;
    }
    if p.prone {
        return n.y >= -0.25 && n.dot(ahead) >= 0.87f32.cos();
    }
    let flat = Vec3::new(n.x, 0., n.z);
    n.y >= 0.3 && n.y <= 0.88 && flat.length() > 1e-4 && flat.normalize().dot(ahead) >= 0.87f32.cos()
}

/// Parallel rays on a grid `step` apart across the plane through the hotdog's feet square to `along` (a unit vector),
/// from far off: for each, where it crosses (across, up) and the ray.
fn grid(p: &Player, along: Vec3, step: f32, low: f32, high: f32) -> Vec<(f32, f32, Vec3, Vec3)> {
    let feet = Vec3::from_array(p.p);
    let across = along.cross(Vec3::Y).normalize();
    let mut rays = vec![];
    let n = (0.12 / step) as i32;
    for i in -n..=n {
        let mut y = low;
        while y <= high {
            let x = i as f32 * step;
            rays.push((x, y, feet + across * x + Vec3::Y * y - along * 2., along));
            y += step;
        }
    }
    rays
}

#[test]
#[ignore]
fn measure_the_head() {
    let step = 0.001;
    let area = |count: usize| count as f32 * step * step * 1e4;
    for (name, size) in [("point", 0.), ("pistol", ROUND_RADIUS[PISTOL]), ("sniper", ROUND_RADIUS[SNIPER])] {
        println!("== {name} round ({size} m)");
        let off = |deg: f32| Quat::from_rotation_y(deg.to_radians()) * Vec3::Z;
        for (way, along) in [("from the front", Vec3::Z), ("30 deg off", off(30.)), ("45 deg off", off(45.)), ("60 deg off", off(60.)), ("from behind", Vec3::NEG_Z), ("from the side", Vec3::X)] {
            let p = dog(0, 0.);
            let rays = grid(&p, along, step, 0., 0.25);
            let old = rays.iter().filter(|r| old_hit(&p, r.2, r.3, size).is_some()).count();
            let new = rays.iter().filter(|r| head::hit(&p, r.2, r.3, size).is_some()).count();
            let old_face = rays.iter().filter(|r| old_hit(&p, r.2, r.3, size).is_some_and(|t| old_face(&p, r.2 + r.3 * t, r.3))).count();
            let new_face = rays.iter().filter(|r| head::hit(&p, r.2, r.3, size).is_some_and(|h| h.part == Part::Head && in_face(&p, h.point, r.3))).count();
            println!("{way}: silhouette old {:.1} cm², new {:.1} cm² ({:.0}%); face old {:.1}, new {:.1} ({:.0}%)", area(old), area(new), 100. * new as f32 / old as f32, area(old_face), area(new_face), 100. * new_face as f32 / old_face.max(1) as f32);
        }
        // The face from the front, from below (rising rays) and from above, by pitch.
        for (elevation, way) in [(-30., "30 deg below"), (-15., "15 deg below"), (0., "level"), (15., "15 deg above")] {
            let along = Quat::from_rotation_x((elevation as f32).to_radians()) * Vec3::Z;
            let row: Vec<String> = [0., -0.4, -0.75, -1.1, -1.5, 0.75, 1.5]
                .iter()
                .map(|pitch| {
                    let p = dog(0, *pitch);
                    let rays = grid(&p, along, step, -0.05, 0.3);
                    let face = rays.iter().filter(|r| head::hit(&p, r.2, r.3, size).is_some_and(|h| h.part == Part::Head && in_face(&p, h.point, r.3))).count();
                    format!("{pitch:+.2}: {:.1}", area(face))
                })
                .collect();
            println!("face seen from the front, {way}: {}", row.join(", "));
        }
        // From behind, by pitch: anything met from the neck's top up (head or body).
        let row: Vec<String> = [0., -0.75, -1.5].iter().map(|pitch| {
            let p = dog(0, *pitch);
            let rays = grid(&p, Vec3::NEG_Z, step, NECK[1], 0.25);
            format!("{pitch:+.2}: {:.1}", area(rays.iter().filter(|r| head::hit(&p, r.2, r.3, size).is_some()).count()))
        }).collect();
        println!("from behind, anything from the neck's top up: {}", row.join(", "));
        // From behind, by pitch: hits on the head, above the neck, and how high the silhouette reaches.
        for pitch in [0., -0.3, -0.6, -0.9, -1.2, -1.5, 0.75, 1.5] {
            let p = dog(0, pitch);
            let rays = grid(&p, Vec3::NEG_Z, step, head::BODY_TOP, 0.25);
            let head = rays.iter().filter(|r| head::hit(&p, r.2, r.3, size).is_some_and(|h| h.part == Part::Head)).count();
            let any = rays.iter().filter(|r| head::hit(&p, r.2, r.3, size).is_some()).count();
            let top = rays.iter().filter(|r| head::hit(&p, r.2, r.3, size).is_some()).map(|r| r.1).fold(0f32, f32::max);
            println!("pitch {pitch:+.2}: from behind above the neck: head {:.1} cm², anything {:.1} cm²; top {:.3} m; head centre {:?}", area(head), area(any), top, head::head_centre(&p) - Vec3::from_array(p.p));
        }
    }
}


// ----- The nod

#[test]
fn the_nod_follows_the_pitch_within_its_range_at_every_stance() {
    for stance in [0, 1] {
        assert_eq!((head::nod(stance, 0.), head::nod(stance, -1.5), head::nod(stance, 1.5)), (0., -BOW, TIP));
    }
    assert_eq!((head::nod(2, -1.5), head::nod(2, 1.5)), (-PRONE_BOW, TIP), "prone the counter is in the way");
    assert!((BOW - 45f32.to_radians()).abs() < 1e-6 && (TIP - 25f32.to_radians()).abs() < 1e-6);
    // In proportion between, the same way round as the look; no further than the most a hotdog looks; nothing for nonsense.
    let mut last = f32::NEG_INFINITY;
    for k in -20..=20 {
        let nod = head::nod(0, k as f32 * 0.075);
        assert!(nod > last && nod.signum() == (k as f32).signum() || k == 0, "{k}: {nod}");
        last = nod;
    }
    assert_eq!((head::nod(0, -3.), head::nod(0, 3.), head::nod(0, f32::NAN)), (-BOW, TIP, 0.));
    assert!((head::nod(0, -0.75) + BOW / 2.).abs() < 1e-6 && (head::nod(0, 0.75) - TIP / 2.).abs() < 1e-6);
    // Tucked in the air a hotdog nods as it stands; the nod is the stance's and the pitch's alone.
    assert_eq!(head::nod_of(&dog(3, -1.)), head::nod(0, -1.));
}

/// The way hotdog `p`'s face looks (from its head's middle to its face's middle, flattened to a unit vector).
fn face_way(p: &Player) -> Vec3 {
    (face_point(p) - head::head_centre(p)).normalize()
}

#[test]
fn the_head_bows_forward_and_down_looking_down_and_tips_back_looking_up_at_every_stance() {
    for stance in STANCES {
        let rest = dog(stance, 0.);
        let ahead = Vec3::new(-rest.yaw.sin(), 0., -rest.yaw.cos());
        // Looking level the head is where the standing frame has it, its face along the facing (prone: lying along it,
        // the face at the front end).
        let level = head::head_centre(&rest);
        assert!(level.distance(head::place(&rest, Vec3::Y * HEAD_AT)) < 1e-6, "stance {stance}");
        let along = if stance == 2 { ahead } else { (ahead * 0.98 + Vec3::Y * 0.2).normalize() };
        assert!(face_way(&rest).dot(along) > 0.999, "stance {stance}: {:?}", face_way(&rest));
        let mut was = level;
        for k in 1..=10 {
            let pitch = -0.15 * k as f32;
            let p = dog(stance, pitch);
            let at = head::head_centre(&p);
            let nod = head::nod_of(&p);
            // Further down, and (upright) further forward, every step of the look; the face turned down by the nod.
            if stance == 2 {
                assert!(at.y < was.y - 1e-5 && (at - level).dot(ahead) <= 1e-5, "prone {pitch}: {at:?}");
            } else {
                assert!(at.y < was.y - 1e-4 && (at - level).dot(ahead) > (was - level).dot(ahead) + 1e-4, "stance {stance} {pitch}: {at:?}");
            }
            let tilt = face_way(&p).angle_between(face_way(&rest));
            assert!((tilt - nod.abs()).abs() < 1e-4 && face_way(&p).y < face_way(&rest).y, "stance {stance} {pitch}: tilt {tilt} nod {nod}");
            was = at;
        }
        // Looking fully down, upright: the head's middle 1.3 cm lower and 2 cm forward; prone, a few millimetres down.
        let down = head::head_centre(&dog(stance, -1.5)) - level;
        if stance == 2 {
            assert!(down.y < -0.003 && down.y > -0.01, "prone {down:?}");
        } else {
            assert!(down.y < -0.012 && down.y > -0.016 && down.dot(ahead) > 0.018 && down.dot(ahead) < 0.024, "stance {stance}: {down:?}");
        }
        // Looking fully up: back (prone: up), the face tipped up by TIP.
        let up = dog(stance, 1.5);
        let back = head::head_centre(&up) - level;
        if stance == 2 { assert!(back.y > 0.005, "prone {back:?}") } else { assert!(back.dot(ahead) < -0.01 && back.y < 0., "stance {stance}: {back:?}") }
        assert!((face_way(&up).angle_between(face_way(&rest)) - TIP).abs() < 1e-4 && face_way(&up).y > face_way(&rest).y);
    }
    // Crouched it is all lower with the chest, tucked in the air where a standing hotdog's is.
    let (stand, crouch, tuck) = (head::head_centre(&dog(0, -1.)), head::head_centre(&dog(1, -1.)), head::head_centre(&dog(3, -1.)));
    assert!((stand.y - crouch.y - (CHEST[0] - CHEST[1])).abs() < 1e-5 && (tuck.y - stand.y - 0.1).abs() < 1e-5);
}

#[test]
fn the_bend_curves_the_neck_smoothly_carries_the_head_as_one_and_keeps_the_body_still() {
    let points = [Vec3::new(0.01, 0.05, -0.02), Vec3::new(0.0, NECK[0], 0.023), Vec3::new(-0.02, 0.105, -0.026), Vec3::new(0.03, NECK[1], 0.03), Vec3::new(0.009, 0.153, -0.0255), Vec3::new(0., 0.178, 0.)];
    for p in points {
        assert_eq!(head::bend(0., p), p, "looking level nothing moves");
    }
    for nod in [-BOW, -0.4, -0.05, 0.05, 0.2, TIP] {
        // Below the neck nothing moves; above it the head turns as one (`head`); across the neck there is no step.
        assert_eq!(head::bend(nod, Vec3::new(0.02, 0.06, -0.023)), Vec3::new(0.02, 0.06, -0.023));
        let (shift, turn) = head::head(nod);
        for p in [Vec3::new(0.009, 0.153, -0.0255), Vec3::new(-0.02, 0.17, 0.03), Vec3::new(0., NECK[1], -0.023)] {
            assert!(head::bend(nod, p).distance(shift + turn * p) < 1e-6, "{nod} {p:?}");
        }
        for y in [NECK[0], NECK[1]] {
            for z in [-0.026, 0., 0.038] {
                let (a, b) = (head::bend(nod, Vec3::new(0., y - 1e-4, z)), head::bend(nod, Vec3::new(0., y + 1e-4, z)));
                assert!(a.distance(b) < 3e-4, "{nod} at {y}: {a:?} {b:?}");
            }
        }
        // Smooth, and never folded through: up the front of the neck (the mustard's line, 2.6 cm in front of the axis) and
        // its back (the buns', 3.8 cm behind), every step up the body is a step up the bent body, shorter or longer by no
        // more than the bend asks.
        for z in [-0.026, 0.038] {
            let mut was = head::bend(nod, Vec3::new(0., NECK[0] - 0.005, z));
            for k in 1..=40 {
                let y = NECK[0] - 0.005 + k as f32 * 0.001;
                let at = head::bend(nod, Vec3::new(0., y, z));
                let step = at.distance(was);
                assert!(step > 0.1e-3 && step < 2.5e-3, "{nod} at ({y}, {z}): a step of {step}");
                was = at;
            }
        }
        // Points turn with the neck: `turn_at` is the bend's own turn there.
        assert_eq!((head::turn_at(nod, 0.), head::turn_at(nod, NECK[1]), head::turn_at(nod, 0.2)), (0., nod, nod));
    }
    // Bowing, the neck gives a little (the head sinks into it); tipping back it does not.
    assert_eq!((head::squash(0.), head::squash(TIP), head::squash(-BOW)), (1., 1., 1. - head::SQUASH));
}

// ----- What a round finds

/// Rays at points of the body below the neck (the chest, 2 and 4 cm under it), from all round, level, rising and
/// falling: those that meet the old sphere below the neck's top.
fn body_rays(p: &Player, size: f32) -> Vec<(f32, Vec3, Vec3)> {
    let mut rays = vec![];
    for drop in [0., 0.02, 0.04] {
        let target = chest(p) - Vec3::Y * drop;
        for k in 0..8 {
            let a = k as f32 * PI / 4.;
            for rise in [-0.4, 0., 0.3] {
                let way = Vec3::new(a.sin(), rise, a.cos()).normalize();
                let o = target - way * 1.5;
                let t = old_hit(p, o, way, size).unwrap();
                if (o + way * t - chest(p)).y < head::BODY_TOP - CHEST[0] - 0.003 {
                    rays.push((drop, o, way));
                }
            }
        }
    }
    rays
}

#[test]
fn a_round_at_the_body_strikes_it_where_it_always_did_whatever_the_look() {
    for stance in [0, 1, 3] {
        for size in [0., ROUND_RADIUS[PISTOL], ROUND_RADIUS[SNIPER], ROUND_RADIUS[4]] {
            let rays = body_rays(&dog(stance, 0.), size);
            assert!(rays.len() >= 40, "{}", rays.len());
            for (drop, o, d) in rays {
                // Where the old sphere had it, looking anywhere; and below the chest, the body.
                let old = old_hit(&dog(stance, 0.), o, d, size).unwrap();
                for k in -10..=10 {
                    let h = head::hit(&dog(stance, k as f32 * 0.15), o, d, size).unwrap();
                    assert!((h.t - old).abs() < 1e-5, "stance {stance} size {size} pitch {} ray {o:?} {d:?}: {h:?} (was {old})", k as f32 * 0.15);
                    if drop >= 0.04 && size < 0.02 {
                        assert_eq!(h.part, Part::Body, "stance {stance} size {size} pitch {} ray {o:?} {d:?}", k as f32 * 0.15);
                    }
                }
            }
        }
    }
    // Only where the bowed head is in front of the chest does a round through the chest's top meet it, as it is drawn:
    // the same damage, but its face, if that is where.
    let bowed = dog(0, -1.5);
    let (o, d) = (chest(&bowed) + Vec3::new(0., -0.002, -1.), Vec3::Z);
    assert_eq!(head::hit(&dog(0, 0.), o, d, 0.).map(|h| h.part), Some(Part::Body));
    let h = head::hit(&bowed, o, d, 0.).unwrap();
    assert!(h.part == Part::Head && (h.t - old_hit(&bowed, o, d, 0.).unwrap()).abs() < 1e-5, "{h:?}");
}

/// A free-for-all on a bare counter: hotdog 1 at (0.3, 1) holding `gun` (full), hotdog 2 at (1.1, 1) facing `yaw`,
/// looking `pitch`, the rest parked at the far end.
fn shooting(gun: usize, yaw: f32, pitch: f32) -> Feast {
    let map = Map::parse(r#"{"format": "hotdog-map", "version": 1, "name": "Range", "bounds": [0, 0, 7, 2], "starts": [{"p": [0.3, 1]}], "spawns": [{"p": [6.9, 0.1]}], "props": []}"#).unwrap();
    let mut s = Feast::with_map(map, false);
    s.begin_famine();
    for (i, p) in s.players.iter_mut().enumerate() {
        (p.bot, p.p) = (false, [6.7, FLOOR, 0.2 + 0.35 * i as f32]);
    }
    let p = &mut s.players[0];
    (p.p, p.yaw) = ([0.3, FLOOR, 1.], -FRAC_PI_2);
    (p.owned[gun], p.ammo[gun], p.weapon, p.gun) = (true, FULL[gun], gun, gun);
    s.players[1].p = [1.1, FLOOR, 1.];
    s.players[1].yaw = yaw;
    s.look(2, pitch);
    s.embody();
    s
}
/// Hotdog 1 fires `gun` at height `y` above hotdog 2's feet, level, along +x from 0.8 m before it: what it took, its
/// record and where the round splatted.
fn level_shot(s: &mut Feast, gun: usize, y: f32) -> (f32, Damage, Option<Vec3>) {
    let at = FLOOR + y;
    s.players[0].p[1] = at - EYE;
    let d = Vec3::X;
    assert!(s.fire(1, d, gun));
    for _ in 0..600 {
        if s.projectiles.is_empty() {
            break;
        }
        s.flight(1. / 60.);
    }
    let hp = s.players[1].hp;
    let splat = s.splats.iter().find(|q| (q.p[0] - 1.1).abs() < 0.1).map(|q| Vec3::from_array(q.p));
    (MAX_HP - hp, s.players[1].last_damage, splat)
}

#[test]
fn from_behind_a_round_at_the_heads_height_hits_a_hotdog_looking_level_and_passes_over_one_looking_down() {
    // Hotdog 2 faces away from the shooter (+x: yaw -pi/2), which fires level from behind it.
    let away = -FRAC_PI_2;
    for gun in [SNIPER, PISTOL, RIFLE] {
        // At the crown of the standing head (3 mm under its top, as the gun's round finds it): looking level it is hit in
        // the head (the body's damage: there is no face behind); looking fully down the round passes over it.
        let brows = HEAD_AT + HEAD_RADIUS + ROUND_RADIUS[gun] - 0.003;
        let (took, record, _) = level_shot(&mut shooting(gun, away, 0.), gun, brows);
        assert!(took == PLAYER_DAMAGE[gun] && !faced(record.kind), "gun {gun}, level: {took} {record:?}");
        let (took, _, _) = level_shot(&mut shooting(gun, away, -1.5), gun, brows);
        assert_eq!(took, 0., "gun {gun}, looking down: passed over");
        // At the eyes' height (between the body's top and the crown): the bowed head is still hit, but lower down it (above
        // its middle, on the crown) and 2 cm further on, where it has gone.
        let eyes = 0.15;
        let (took, _, level) = level_shot(&mut shooting(gun, away, 0.), gun, eyes);
        let (took_down, _, bowed) = level_shot(&mut shooting(gun, away, -1.5), gun, eyes);
        let (level, bowed) = (level.unwrap(), bowed.unwrap());
        assert!(took == PLAYER_DAMAGE[gun] && took_down == PLAYER_DAMAGE[gun] && bowed.x - level.x > 0.015, "gun {gun}: {level:?} {bowed:?}");
        let crown = head::head_centre(&shooting(gun, away, -1.5).players[1]);
        assert!(bowed.y > crown.y + 0.01, "gun {gun}: met above the bowed head's middle ({:?} over {:?})", bowed, crown);
    }
}

/// How many of a grid of rays (1 mm apart, parallel, from behind) at the head's region of a standing hotdog that
/// looks `pitch` meet it, for a round of `size`: from the body's top (BODY_TOP) to 6 cm above it, 24 cm across. (All
/// that is met there is the head.)
fn head_hits_from_behind(pitch: f32, size: f32) -> usize {
    let p = dog(0, pitch);
    let rays = grid(&p, Vec3::Z, 0.001, head::BODY_TOP + 0.0005, head::BODY_TOP + 0.06);
    let hits: Vec<_> = rays.iter().filter_map(|r| head::hit(&p, r.2, r.3, size)).collect();
    assert!(hits.iter().all(|h| h.part == Part::Head));
    hits.len()
}

#[test]
fn a_sweep_from_behind_lands_fewer_hits_on_the_head_the_further_down_the_hotdog_looks() {
    // (The hotdog faces -z: these rays come from behind it.)
    for (name, size) in [("point", 0.), ("pistol", ROUND_RADIUS[PISTOL]), ("sniper", ROUND_RADIUS[SNIPER]), ("rifle", ROUND_RADIUS[RIFLE])] {
        let counts: Vec<usize> = [0., -0.3, -0.6, -0.9, -1.2, -1.5].iter().map(|pitch| head_hits_from_behind(*pitch, size)).collect();
        println!("{name}: head hits from behind by pitch 0, -0.3, -0.6, -0.9, -1.2, -1.5: {counts:?} ({:.0}% at a full bow)", 100. * counts[5] as f32 / counts[0] as f32);
        assert!(counts.windows(2).all(|w| w[1] < w[0]), "{name}: {counts:?}");
        assert!((counts[5] as f32) < 0.7 * counts[0] as f32, "{name}: a full bow leaves under seven tenths: {counts:?}");
        // And the top of the head, seen from behind, is lower.
        let top = |pitch| {
            let p = dog(0, pitch);
            grid(&p, Vec3::Z, 0.0005, head::BODY_TOP, 0.25).iter().filter(|r| head::hit(&p, r.2, r.3, size).is_some()).map(|r| r.1).fold(0f32, f32::max)
        };
        assert!(top(0.) - top(-1.5) > 0.011, "{name}: {} {}", top(0.), top(-1.5));
    }
}

#[test]
fn a_round_in_the_face_of_a_level_hotdog_still_kills_and_the_face_turns_with_the_head() {
    let toward = FRAC_PI_2; // facing -x: the shooter
    for gun in [SNIPER, PISTOL] {
        let aim = |s: &mut Feast, at: Vec3, up: f32| {
            s.players[0].p[1] += up;
            let d = (at - muzzle(&s.players[0])).normalize();
            assert!(s.fire(1, d, gun));
            for _ in 0..600 {
                if s.projectiles.is_empty() {
                    break;
                }
                s.flight(1. / 60.);
            }
            (MAX_HP - s.players[1].hp, s.players[1].last_damage)
        };
        // Looking level, in the face: a kill, a face shot.
        let mut s = shooting(gun, toward, 0.);
        let face = face_point(&s.players[1]);
        let (took, record) = aim(&mut s, face, 0.);
        assert!(took == MAX_HP && faced(record.kind), "gun {gun}: {took} {record:?}");
        // Bowed fully: a round from straight ahead at the same height meets the top of the head, not the face.
        let mut s = shooting(gun, toward, -1.5);
        let (took, record) = aim(&mut s, face, face.y - FLOOR - EYE);
        assert!(!faced(record.kind) && took <= PLAYER_DAMAGE[gun], "gun {gun}, bowed, level: {took} {record:?}");
        // ... and one from below, at the face where the bow has it, is a face shot.
        let mut s = shooting(gun, toward, -1.5);
        let bowed = face_point(&s.players[1]);
        let (took, record) = aim(&mut s, bowed, -0.12);
        assert!(took == MAX_HP && faced(record.kind), "gun {gun}, bowed, from below: {took} {record:?}");
        // Looking up, the face is up and back with the head, and a round at it is a face shot.
        let mut s = shooting(gun, toward, 1.5);
        let up = face_point(&s.players[1]);
        assert!(up.y > face.y && up.x > face.x);
        let (took, record) = aim(&mut s, up, 0.);
        assert!(took == MAX_HP && faced(record.kind), "gun {gun}, looking up: {took} {record:?}");
    }
    // The face's middle is in the head, at every stance and look.
    for stance in STANCES {
        for k in -4..=4 {
            let p = dog(stance, k as f32 * 0.375);
            let head = Head::of(&p);
            let f = face_point(&p);
            assert!(f.distance(head.spine(f)) < HEAD_RADIUS, "stance {stance} pitch {}", p.pitch);
        }
    }
}

#[test]
fn a_blast_finds_the_head_where_the_nod_has_it_and_the_body_where_it_always_was() {
    let glob = Projectile { owner: 1, weapon: 4, hostile: true, ..Default::default() };
    let took = |pitch: f32, at: Vec3| {
        let mut s = shooting(1, 0., pitch);
        s.burst(&glob, Vec3::from_array(s.players[1].p) + at, None, None);
        MAX_HP - s.players[1].hp
    };
    // A glob bursting 5 cm over the top of a level hotdog's head: the head is nearer than the blast's old capsule (whose
    // top is 1 cm lower), and the bowed head, gone down and forward, further.
    let over = Vec3::Y * (HEAD_AT + HEAD_RADIUS + 0.05);
    let (level, bowed) = (body_gap(&dog(0, 0.), Vec3::from_array(dog(0, 0.).p) + over), body_gap(&dog(0, -1.5), Vec3::from_array(dog(0, 0.).p) + over));
    assert!((level.0 - 0.05).abs() < 1e-5 && bowed.0 > level.0 + 0.008, "{level:?} {bowed:?}");
    assert!(took(0., over) > took(-1.5, over) + 5., "{} {}", took(0., over), took(-1.5, over));
    // In front of the bowed face, 3 cm beyond the head where it has gone: nearer the bowed head than the body.
    let front = face_point(&dog(0, -1.5)) - Vec3::from_array(dog(0, 0.).p) + Vec3::new(0., 0.01, -0.03);
    let (gap, near) = body_gap(&dog(0, -1.5), Vec3::from_array(dog(0, 0.).p) + front);
    assert!(near.distance(Head::of(&dog(0, -1.5)).spine(near)) < HEAD_RADIUS + 1e-4 && gap < body_gap(&dog(0, 0.), Vec3::from_array(dog(0, 0.).p) + front).0, "{gap}");
    // Round the body below the neck (the feet, the sides at the chest) it is as it always was, whatever the look.
    for point in [Vec3::new(1., FLOOR + 0.004, 1.), Vec3::new(1.08, FLOOR + 0.06, 1.), Vec3::new(1., FLOOR + 0.09, 1.1), Vec3::new(0.9, FLOOR + 0.02, 0.92)] {
        let level = body_gap(&dog(0, 0.), point);
        for pitch in [-1.5, -0.7, 0.7, 1.5] {
            assert_eq!(body_gap(&dog(0, pitch), point), level, "{point:?} {pitch}");
        }
    }
}

#[test]
fn the_body_meets_the_world_the_same_whatever_the_look() {
    // Two rounds alike but for where hotdog 1 looks: it walks into the bowl and against the counter's edge, jumps and
    // crouches, and its body ends every step in the same place with the same height.
    let run = |pitch: f32| {
        let mut s = Feast::new(false);
        let mut trail = vec![];
        for step in 0..240 {
            s.look(1, pitch);
            let yaw = 0.6 + step as f32 * 0.01;
            s.walk(1, Vec2::new(0., -1.) * actor::WALK as f32 / 60., yaw);
            if step == 60 {
                s.jump(1, actor::JUMP as f32);
            }
            s.set_crouch(1, (100..140).contains(&step));
            s.tick(1. / 60.);
            let body = s.body(1).unwrap();
            trail.push((body.p, body.height, body.crouch, s.players[0].p));
        }
        trail
    };
    let level = run(0.);
    assert_eq!(run(-1.5), level);
    assert_eq!(run(1.5), level);
}

#[test]
fn the_head_is_the_same_bit_for_bit_and_the_protocols_pitch_is_fine_enough_for_it() {
    // The same look, the same head and the same hits, every time.
    let rays = grid(&dog(0, 0.), Vec3::Z, 0.004, 0., 0.2);
    for pitch in [-1.5, -0.83, 0., 0.41, 1.5] {
        let (a, b) = (dog(0, pitch), dog(0, pitch));
        assert_eq!(Head::of(&a).ends, Head::of(&b).ends);
        for r in &rays {
            assert_eq!(head::hit(&a, r.2, r.3, 0.008).map(|h| (h.t.to_bits(), h.part)), head::hit(&b, r.2, r.3, 0.008).map(|h| (h.t.to_bits(), h.part)));
        }
    }
    // A command carries the pitch to 1.5/32767 of a radian, a frame to 1e-4: the head the server has and the head a
    // client draws from either are within a few thousandths of a millimetre of each other.
    for k in -300..=300 {
        let pitch = k as f32 * 0.005 + 0.0001234;
        let command = crate::net::Command::new(0, Vec2::ZERO, 0., pitch, 0, 0, None).pitch();
        let frame = ((pitch as f64) * 1e4).round() as f32 / 1e4;
        let exact = Head::of(&dog(0, pitch));
        for q in [command, frame] {
            let near = Head::of(&dog(0, q));
            assert!(near.centre().distance(exact.centre()) < 5e-6, "{pitch} ~ {q}");
            assert!(face_point(&dog(0, q)).distance(face_point(&dog(0, pitch))) < 5e-6);
        }
    }
}

#[test]
fn a_bot_going_for_the_face_aims_where_the_bowed_face_is_and_a_bots_head_follows_its_aim() {
    let s = Feast::new(false);
    let o = Vec3::new(1., FLOOR + EYE, 1.6);
    // The target at (1, 1) faces +z, towards the bot at (1, 1.6).
    for pitch in [0., -1.5, 1.5] {
        let t = Player { yaw: PI, ..dog(0, pitch) };
        let (c, face) = s.mark(o, &t, SNIPER);
        assert_eq!((c, face), (chest(&t), Some(face_point(&t))), "the body's middle, and the face where it is");
    }
    let (level, bowed) = (face_point(&Player { yaw: PI, ..dog(0, 0.) }), face_point(&Player { yaw: PI, ..dog(0, -1.5) }));
    assert!(bowed.y < level.y - 0.01 && bowed.z > level.z + 0.01, "{level:?} {bowed:?}");
    // (A bot's head bows with its aim: feast.rs `aim_pitch_follows_the_player_every_tick_and_bots_aim_at_what_they_shoot`.)
}

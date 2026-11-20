// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! What rounds meet: solid loose objects stop them, light ones (a pillow) let them through slowed and weakened, a
//! glob's splash is shielded by solid cover only, and the ketchup sniper's round goes through bodies.
use super::*;

const DT: f32 = 1. / 30.;

/// A bare counter seven metres by two with the given loose objects on it, in the free for all (rounds hurt hotdogs).
/// Hotdog 1 stands at (0.3, 1) facing +x holding `gun` (full); hotdogs 2, 3... stand at `rivals` ([x, z]); the rest
/// stand out of the way at the far end, and nobody is a bot. Only `flight` is stepped, so the loose objects stay
/// where the map puts them, in the air if it says so.
fn range(props: &str, gun: usize, rivals: &[[f32; 2]]) -> Feast {
    let map = Map::parse(&format!(
        r#"{{"format": "hotdog-map", "version": 1, "name": "Range", "bounds": [0, 0, 7, 2], "starts": [{{"p": [0.3, 1]}}], "spawns": [{{"p": [6.9, 0.1]}}], "props": [{props}]}}"#
    ))
    .unwrap();
    let mut s = Feast::with_map(map, false);
    s.begin_famine();
    for (i, p) in s.players.iter_mut().enumerate() {
        (p.bot, p.p) = (false, [6.7, FLOOR, 0.2 + 0.35 * i as f32]);
    }
    let p = &mut s.players[0];
    (p.p, p.yaw) = ([0.3, FLOOR, 1.], -std::f32::consts::FRAC_PI_2);
    (p.owned[gun], p.ammo[gun], p.weapon, p.gun) = (true, FULL[gun], gun, gun);
    for (i, at) in rivals.iter().enumerate() {
        s.players[i + 1].p = [at[0], FLOOR, at[1]];
    }
    s.embody();
    s
}
/// A loose object of `kind` at (x, 1) with its underside `base` above the counter, turned by `yaw`.
fn put(kind: &str, x: f32, base: f32, yaw: f32) -> String {
    format!(r#"{{"kind": "{kind}", "p": [{x}, 1], "base": {base}, "yaw": {yaw}}}"#)
}
/// Flies every round until it has ended (at most 20 s).
fn fly(s: &mut Feast) {
    for _ in 0..600 {
        if s.projectiles.is_empty() {
            return;
        }
        s.flight(DT);
    }
}
fn hp(s: &Feast, id: u32) -> f32 {
    s.player(id).unwrap().hp
}
/// Where the muzzle is, and a shot from it along +x (`drop` down for each metre across).
fn shoot(s: &mut Feast, gun: usize, drop: f32) -> Vec3 {
    let d = Vec3::new(1., -drop, 0.).normalize();
    s.players[0].cooldown = 0.;
    assert!(s.fire(1, d, gun));
    muzzle(&s.players[0])
}
/// A pillow with its middle at the muzzle's height (0.135 m: its underside 0.1125 m up), at x.
fn pillow(x: f32) -> String {
    put("pillow", x, 0.1125, 0.)
}

#[test]
fn the_loose_kinds_have_a_material_each_a_pillow_light_and_the_rest_solid() {
    use props::Material::{Light, Solid};
    for (kind, m) in [("bowl", Solid), ("crate", Solid), ("pillow", Light), ("napkin", Solid), ("chair", Solid), ("tray", Solid)] {
        assert_eq!(props::material(kind), m, "{kind}");
    }
    // The kitchen's: card and paper let a round through (a cereal box, a roll of paper towels); china, tin, wood, glass
    // and enamel stop it.
    for (kind, m) in [("cereal", Light), ("roll", Light), ("cup", Solid), ("plate", Solid), ("tin", Solid), ("board", Solid), ("dish", Solid), ("salt", Solid), ("pepper", Solid), ("breadbox", Solid)] {
        assert_eq!(props::material(kind), m, "{kind}");
    }
    assert!(LOOSE.iter().all(|kind| props::MATERIALS.iter().any(|(name, _)| name == kind)), "every kind a map can place is listed");
    assert_eq!(props::material("anvil"), props::Material::Solid, "a kind not listed is solid");
    let s = range(&[put("crate", 1., 0., 0.), pillow(2.)].join(","), 1, &[]);
    assert_eq!((s.props.material(0), s.props.material(1)), (props::Material::Solid, props::Material::Light));
}

#[test]
fn a_napkin_lying_flat_and_a_chair_stop_a_round() {
    // Aimed down at the counter 0.6 m off: bare, the mustard lands on the counter; over a napkin lying flat there it
    // stops on the napkin's top, 8 mm up.
    let mut s = range("", 1, &[]);
    shoot(&mut s, 1, 0.135 / 0.6);
    fly(&mut s);
    assert!((s.splats[0].p[1] - FLOOR).abs() < 1e-4 && (s.splats[0].p[0] - 0.9).abs() < 1e-3, "{:?}", s.splats[0].p);
    let mut s = range(&put("napkin", 0.8, 0., 0.), 1, &[]);
    let o = shoot(&mut s, 1, 0.135 / 0.6);
    let d = Vec3::new(1., -0.135 / 0.6, 0.).normalize();
    assert!(s.cover(o, d) < 0.59, "a napkin is cover: {}", s.cover(o, d));
    fly(&mut s);
    let splat = s.splats[0].p;
    assert!(splat[1] > FLOOR + 0.005 && splat[1] < FLOOR + 0.012 && splat[0] < 0.89, "the round stopped on the napkin at {splat:?}");
    // A chair, its back across the shot, stops a level round, the sniper's as well, and the rival behind it is unhurt.
    for gun in [1, SNIPER] {
        let mut s = range(&put("chair", 0.8, 0., std::f32::consts::FRAC_PI_2), gun, &[[1.3, 1.]]);
        let o = shoot(&mut s, gun, 0.);
        assert!(s.cover(o, Vec3::X) < 0.56, "gun {gun}");
        fly(&mut s);
        assert_eq!((hp(&s, 2), s.splats.len()), (100., 1), "gun {gun}");
        assert!(s.splats[0].p[0] < 0.86, "gun {gun}: stopped at {:?}", s.splats[0].p);
    }
}

#[test]
fn a_pillow_lets_a_round_through_at_0_6_of_its_speed_and_0_75_of_its_damage() {
    let mut s = range(&pillow(0.8), 1, &[[1.4, 1.]]);
    let o = shoot(&mut s, 1, 0.);
    assert!(s.cover(o, Vec3::X) > 5., "a pillow is no cover");
    let speed = |s: &Feast| Vec3::from_array(s.projectiles[0].v).length();
    while s.projectiles[0].soaked.is_empty() {
        assert!((speed(&s) - ROUND_SPEED[1]).abs() < 1e-5);
        s.flight(DT);
    }
    assert!((speed(&s) - ROUND_SPEED[1] * LIGHT_SPEED).abs() < 1e-5, "slowed to {}", speed(&s));
    assert_eq!(s.projectiles[0].soaked, [0]);
    // It is not slowed again by the same pillow on its way out.
    s.flight(DT);
    assert!((speed(&s) - ROUND_SPEED[1] * LIGHT_SPEED).abs() < 1e-5);
    fly(&mut s);
    assert!((hp(&s, 2) - (100. - PLAYER_DAMAGE[1] * LIGHT_DAMAGE)).abs() < 1e-4, "hurt for {}", 100. - hp(&s, 2));
    assert_eq!(s.splats.len(), 1);
    // It got there later than it would have, at 7.5 m/s to the pillow and 4.5 m/s after it.
    let arrival = |props: &str| {
        let mut s = range(props, 1, &[[1.4, 1.]]);
        shoot(&mut s, 1, 0.);
        (0..200).find(|_| {
            s.flight(DT);
            hp(&s, 2) < 100.
        })
    };
    let (bare, slowed) = (arrival("").unwrap(), arrival(&pillow(0.8)).unwrap());
    assert!(slowed > bare, "{slowed} steps, not {bare}");
    // A zombie behind a pillow is hit for 0.75 of the damage too.
    let mut s = range(&pillow(0.8), 1, &[]);
    s.enemies.push(Zombie { id: 1, p: [1.4, FLOOR, 1.], yaw: 0., hp: 100., born: 0., forming: 0. });
    s.embody();
    shoot(&mut s, 1, 0.);
    fly(&mut s);
    assert!((s.enemies[0].hp - (100. - ZOMBIE_DAMAGE[1] * LIGHT_DAMAGE)).abs() < 1e-4);
}

#[test]
fn a_round_slowed_below_1_5_m_s_splats_in_the_light_object_it_is_in() {
    // Mustard (7.5 m/s) through pillows: 4.5, 2.7, 1.62 and on, and in the fourth 0.972, which stalls it there (STALL
    // went up with the rounds' speeds, so it is still the fourth).
    let pillows: Vec<String> = [0.6, 0.85, 1.1, 1.35].into_iter().map(pillow).collect();
    let mut s = range(&pillows.join(","), 1, &[[2., 1.]]);
    shoot(&mut s, 1, 0.);
    while s.projectiles.first().is_some_and(|q| q.soaked.len() < 3) {
        s.flight(DT);
    }
    assert!((Vec3::from_array(s.projectiles[0].v).length() - 1.62).abs() < 1e-4 && 1.62 > STALL && STALL == 1.5);
    fly(&mut s);
    let splat = s.splats[0].p;
    assert!(splat[0] > 1.35 - 0.09 && splat[0] < 1.35 + 0.09, "it splatted in the fourth pillow: {splat:?}");
    assert_eq!((hp(&s, 2), s.splats.len()), (100., 1));
    // A relish glob (4.5 m/s) stalls in the third, and bursts there.
    let mut s = range(&pillows.join(","), 4, &[[1.2, 1.]]);
    shoot(&mut s, 4, 0.);
    fly(&mut s);
    let splat = s.splats[0].p;
    assert!(splat[0] > 1.1 - 0.09 && splat[0] < 1.1 + 0.09, "it splatted in the third pillow: {splat:?}");
    assert!(hp(&s, 2) < 100., "its burst reached the rival a pillow away");
}

#[test]
fn a_relish_globs_blast_is_weakened_by_a_pillow_and_stopped_by_a_chair() {
    let glob = Projectile { owner: 1, weapon: 4, hostile: true, ..Default::default() };
    let burst = Vec3::new(1., FLOOR + CHEST[0], 1.);
    // In the open, then with a pillow (turned so that it fits between) and then a chair's back between.
    let mut hurt = vec![];
    for between in ["".to_string(), put("pillow", 1.075, 0.105 - 0.0225, std::f32::consts::FRAC_PI_2), put("chair", 1.075, 0., std::f32::consts::FRAC_PI_2)] {
        let mut s = range(&between, 1, &[[1.15, 1.]]);
        let solid = between.contains("chair");
        assert_eq!(s.cover(burst, Vec3::X) < 0.15, solid, "{between}");
        s.burst(&glob, burst, None, None);
        hurt.push(100. - hp(&s, 2));
    }
    // (The body's nearest point is its side, HOTDOG_RADIUS short of its middle.)
    let open = PLAYER_DAMAGE[4] * blast(0.15 - HOTDOG_RADIUS as f32, SPLASH[4]);
    assert!(open > 30. && open < 60., "about half its HP mid-blast: {open}");
    assert!((hurt[0] - open).abs() < 1e-3 && (hurt[1] - open * LIGHT_DAMAGE).abs() < 1e-3, "a pillow takes a quarter off: {hurt:?}");
    assert_eq!(hurt[2], 0., "a chair shields");
    // A glob that came through a pillow on its way bursts at 0.75 of its strength.
    let mut s = range("", 1, &[[1.15, 1.]]);
    s.burst(&Projectile { soaked: vec![9], ..glob }, burst, None, None);
    assert!((100. - hp(&s, 2) - open * LIGHT_DAMAGE).abs() < 1e-3);
}

#[test]
fn the_sniper_passes_through_two_hotdogs_in_a_line_hurting_each_once_and_stops_at_a_crate() {
    // Rivals at 0.8 and 1.1 m, a crate (in the air at the muzzle's height) at 1.6 and a third rival behind it at 2.
    let mut s = range(&put("crate", 1.6, 0.085, 0.), SNIPER, &[[0.8, 1.], [1.1, 1.], [2., 1.]]);
    // The first is low on HP, and the shot kills it on its way to the second.
    s.players[1].hp = 30.;
    let o = shoot(&mut s, SNIPER, 0.);
    let id = s.projectiles[0].id;
    fly(&mut s);
    assert!(!s.players[1].alive && hp(&s, 3) == 100. - PLAYER_DAMAGE[SNIPER] && hp(&s, 4) == 100.);
    // Each on its record as the sniper's, from its muzzle, along the shot: what the client's death camera reads.
    for victim in [2, 3] {
        let record = s.player(victim).unwrap().last_damage;
        assert_eq!((record.kind, record.attacker), (2, 1), "hotdog {victim}");
        assert!(Vec3::from_array(record.p).distance(o) < 1e-3 && Vec3::from_array(record.d).distance(Vec3::X) < 1e-5);
    }
    assert_eq!(s.player(2).unwrap().last_damage.amount, 30.);
    // A splat on each body, where the round entered it, and one where the crate stopped it, all of the one round.
    let at: Vec<f32> = s.splats.iter().map(|p| p.p[0]).collect();
    assert_eq!(s.splats.iter().filter(|p| p.projectile == id && p.weapon == SNIPER).count(), 3);
    assert!(at[0] < 0.8 && at[0] > 0.7 && at[1] < 1.1 && at[1] > 1.0 && (at[2] - 1.55).abs() < 0.01, "{at:?}");
    assert_eq!(s.friendly_fire_events, 2);
    // Without the crate it flies on its 6 m, through the third as well.
    let mut s = range("", SNIPER, &[[0.8, 1.], [1.1, 1.], [2., 1.]]);
    shoot(&mut s, SNIPER, 0.);
    fly(&mut s);
    assert_eq!([2, 3, 4].map(|id| hp(&s, id)), [45., 45., 45.]);
    assert!((s.splats.last().unwrap().p[0] - (0.3 + RANGE[SNIPER])).abs() < 1e-3);
    // Through a pillow first: slowed and weakened for all of them.
    let mut s = range(&pillow(0.6), SNIPER, &[[0.8, 1.], [1.1, 1.]]);
    shoot(&mut s, SNIPER, 0.);
    fly(&mut s);
    assert!([2, 3].iter().all(|id| (hp(&s, *id) - (100. - PLAYER_DAMAGE[SNIPER] * LIGHT_DAMAGE)).abs() < 1e-4));
    // Zombies too: two in a line, and the sniper's round kills both.
    let mut s = range("", SNIPER, &[]);
    s.stage = Stage::Wave;
    for (id, x) in [(1, 0.8), (2, 1.1)] {
        s.enemies.push(Zombie { id, p: [x, FLOOR, 1.], yaw: 0., hp: 70., born: 0., forming: 0. });
    }
    s.embody();
    let d = Vec3::new(1., -0.06 / 0.65, 0.).normalize();
    assert!(s.fire(1, d, SNIPER));
    fly(&mut s);
    assert_eq!((s.enemies.len(), s.players[0].kills), (0, 2));
}

#[test]
fn a_mustard_round_still_stops_at_the_first_hotdog() {
    let mut s = range("", 1, &[[0.8, 1.], [1.1, 1.]]);
    shoot(&mut s, 1, 0.);
    fly(&mut s);
    assert_eq!((hp(&s, 2), hp(&s, 3), s.splats.len()), (100. - PLAYER_DAMAGE[1], 100., 1));
    assert!(s.splats[0].p[0] < 0.8);
    // And the syrup rifle's, the hot sauce's and the relish glob's: whatever is behind the first is untouched.
    for gun in [RIFLE, 3, 4] {
        let mut s = range("", gun, &[[0.6, 1.], [0.7, 1.]]);
        shoot(&mut s, gun, 0.);
        s.players[0].volley = 0;
        fly(&mut s);
        assert!(hp(&s, 2) < 100., "gun {gun}");
        if gun != 4 {
            assert_eq!(hp(&s, 3), 100., "gun {gun}");
        }
    }
}

#[test]
fn streams_pass_light_objects_slowed_as_rounds_do() {
    // Hot sauce through a pillow to a rival behind it, at 0.75 of a droplet's damage.
    let mut s = range(&pillow(0.45), 3, &[[0.65, 1.]]);
    shoot(&mut s, 3, 0.);
    fly(&mut s);
    assert!((100. - hp(&s, 2) - PLAYER_DAMAGE[3] * LIGHT_DAMAGE).abs() < 1e-4, "{}", 100. - hp(&s, 2));
    // A droplet of mayonnaise squirted level through a pillow just in front: from there it sets out on a new arc at
    // 0.6 of the speed it had, and lands nearer than it would have.
    let landing = |props: &str| {
        let mut s = range(props, MAYO, &[]);
        shoot(&mut s, MAYO, 0.);
        let mut passed = None;
        for _ in 0..200 {
            if s.projectiles.is_empty() {
                break;
            }
            let before = Vec3::from_array(s.projectiles[0].v).length();
            s.flight(DT);
            if passed.is_none() && s.projectiles.first().is_some_and(|q| !q.soaked.is_empty()) {
                let q = &s.projectiles[0];
                passed = Some((before, Vec3::from_array(q.launch).length(), q.born, q.p, q.from));
            }
        }
        (s.splats[0].p[0], passed)
    };
    let (open, none) = landing("");
    let (through, passed) = landing(&pillow(0.5));
    assert!(none.is_none());
    let (before, after, born, p, from) = passed.unwrap();
    assert!((after / before - LIGHT_SPEED).abs() < 0.02, "{before} then {after}");
    // (A falling round's step is cut into parts, `flight`: it sets out afresh from the pillow and flies the rest of the
    // step on its new arc.)
    assert!(born == 0. && from[0] > 0.3 && from[0] < 0.6 && p[0] >= from[0], "it sets out afresh from the pillow: from {from:?}, now at {p:?}");
    assert!(through < open - 0.1, "landed at {through}, not {open}");
}

#[test]
fn a_bot_shoots_through_a_pillow_and_holds_its_fire_at_a_chair() {
    // Hotdog 1 is a bot with the mustard pistol and nothing else, well into the free for all, and a rival 0.7 m off
    // with a pillow, then a chair, between them.
    for (between, fires) in [(pillow(0.65), true), (put("chair", 0.65, 0., std::f32::consts::FRAC_PI_2), false), (String::new(), true)] {
        let mut s = range(&between, 1, &[[1., 1.]]);
        let p = &mut s.players[0];
        (p.bot, p.owned, p.ammo) = (true, [false, true, false, false, false, false, false, false], [-1, FULL[1], 0, 0, 0, 0, 0, 0]);
        s.famine_at = s.elapsed - 10.;
        // (Over a second: it reacts to its rival first, aim.rs.)
        for _ in 0..30 {
            s.bot(1, DT);
            s.elapsed += DT;
            s.players[0].cooldown -= DT;
        }
        assert_eq!(s.players[0].ammo[1] < FULL[1], fires, "{between}");
    }
}

#[test]
fn every_gun_flies_exactly_five_times_as_fast_as_it_once_did_and_reaches_as_far() {
    const BEFORE: [f32; GUNS] = [0., 1.5, 9., 1.1, 0.9, 1.3, 3.5, 2.2];
    assert_eq!(SPEEDUP, 5.);
    for w in 0..GUNS {
        assert_eq!(ROUND_SPEED[w], BEFORE[w] * 5., "gun {w}");
    }
    assert_eq!(crate::tune::Tuning::default().speed, ROUND_SPEED);
    // The reaches are as they were: the rounds get there sooner, so they are in the air for a fifth of the time.
    assert_eq!(RANGE, [0.21, 3., 6., 0.65, 4., 0.8, 4., 6.]);
    // The mayonnaise falls 25 times as fast, so its arcs are the shapes they were and reach as far.
    assert_eq!(MAYO_FALL, actor::GRAVITY as f32 * 25.);
    for pitch in [0., 0.3, 0.7, 1.] {
        let (now, then) = (throw(ROUND_SPEED[MAYO], MAYO_FALL, pitch, EYE), throw(BEFORE[MAYO], actor::GRAVITY as f32, pitch, EYE));
        assert!((now - then).abs() < 1e-4 * then, "{pitch}: {now} against {then}");
    }
    // A round stalls in as many light objects as it did.
    assert_eq!(STALL, 0.3 * 5.);
}

/// A cutting board (45 by 30 cm, 2.5 cm thick) stood on its long edge across the line of fire, its far face at `x`:
/// 2.5 cm of wood from `x - 0.025` to `x`, 45 cm tall, 30 cm across.
fn board_on_edge(s: &mut Feast, x: f32) -> usize {
    let kind = LOOSE.iter().position(|k| *k == "board").unwrap();
    let q = std::f64::consts::FRAC_1_SQRT_2;
    s.props.add(kind, [x as f64, FLOOR as f64 + 0.226, 1.], [0., 0., q, q])
}

#[test]
fn a_round_as_fast_as_the_snipers_never_passes_a_cutting_board_on_its_edge_between_one_step_and_the_next() {
    // Every gun that flies straight, and the mayonnaise squirted level, at every distance from the muzzle out to its
    // reach, a centimetre and a bit apart, with steps of 1/30 s (the longest the simulation takes: the sniper's round
    // goes 1.5 m in one) and of 1/60 s. A rival stands just behind the board.
    let mut stopped = 0;
    for gun in [1, 2, 3, 4, 5, 6, MAYO] {
        for dt in [1. / 30., 1. / 60.] {
            let mut s = range("", gun, &[[4.6, 1.]]);
            let board = board_on_edge(&mut s, 4.5);
            assert_eq!(s.props.shape(s.props.body(board).shape).spec.name, "board");
            let reach = if gun == MAYO { 0.5 } else { RANGE[gun] - 0.05 };
            let mut gap = 0.03;
            while gap < reach {
                let p = &mut s.players[0];
                (p.p, p.bloom, p.ammo[gun], p.cooldown) = ([4.475 - gap, FLOOR, 1.], 0., FULL[gun], 0.);
                s.players[1].hp = 100.;
                // (Level, or a little up for the mayonnaise, which falls.)
                let d = if gun == MAYO { Vec3::new(1., 0.2, 0.).normalize() } else { Vec3::X };
                assert!(s.fire(1, d, gun), "gun {gun}");
                let first = s.next_splat;
                for _ in 0..200 {
                    if s.projectiles.is_empty() {
                        break;
                    }
                    s.flight(dt);
                }
                let splat = s.splats.iter().find(|splat| splat.id >= first).expect("it ended somewhere");
                assert!(splat.p[0] <= 4.475 + 1e-3, "gun {gun} at {dt}, {gap} m off: it went through the board to {:?}", splat.p);
                assert_eq!(hp(&s, 2), 100., "gun {gun} at {dt}, {gap} m off: the rival behind it was hit");
                stopped += (splat.p[0] > 4.475 - 0.01) as usize;
                s.splats.clear();
                gap += 0.013;
            }
        }
    }
    assert!(stopped > 2000, "{stopped} rounds stopped at the board's face");
}

#[test]
fn a_round_as_fast_as_the_snipers_never_passes_a_hotdog_or_a_cat_between_one_step_and_the_next() {
    // A hotdog (its middle 0.055 m round, beside the round's own size) and a cat (its body 0.03 m round; it is hit
    // 0.065 m about its middle) at every distance out to the gun's reach, a centimetre apart, steps of 1/30 s: every
    // round lands, a point-sized one too (`tune.size` 0).
    for gun in [1, 2, 3, 4, 6] {
        for size in [ROUND_RADIUS[gun], 0.] {
            let mut s = range("", gun, &[[1., 1.]]);
            s.tune.size[gun] = size;
            let mut gap = 0.08;
            while gap < RANGE[gun] - 0.08 {
                let p = &mut s.players[0];
                (p.p, p.bloom, p.ammo[gun], p.cooldown) = ([0.3, FLOOR, 1.], 0., FULL[gun], 0.);
                // The rival, then a cat in its place.
                for cat in [false, true] {
                    // (A relish glob kills: the rival is back for every shot.)
                    (s.players[1].hp, s.players[1].alive) = (100., true);
                    s.players[1].p = if cat { [6.8, FLOOR, 1.9] } else { [0.3 + gap, FLOOR, 1.] };
                    s.enemies = if cat { vec![Zombie { id: 1, p: [0.3 + gap, FLOOR, 1.], yaw: 0., hp: 1e6, born: 0., forming: 0. }] } else { vec![] };
                    s.embody();
                    // (A cat is struck at its middle, 7.5 cm up; a hotdog at its chest.)
                    let at = if cat { FLOOR + 0.075 } else { FLOOR + CHEST[0] };
                    let d = Vec3::new(gap, at - (FLOOR + EYE), 0.).normalize();
                    (s.players[0].cooldown, s.players[0].bloom) = (0., 0.);
                    assert!(s.fire(1, d, gun));
                    for _ in 0..200 {
                        if s.projectiles.is_empty() {
                            break;
                        }
                        s.flight(1. / 30.);
                    }
                    let hit = if cat { s.enemies[0].hp < 1e6 } else { hp(&s, 2) < 100. };
                    assert!(hit, "gun {gun} (size {size}): the {} {gap} m off was not hit", if cat { "cat" } else { "hotdog" });
                }
                gap += 0.01;
            }
        }
    }
}

// ----- The relish bazooka: a kill where it hits, and by how near it burst round about (feast.rs `blast`).

#[test]
fn a_relish_glob_kills_outright_where_it_hits_and_round_about_by_how_near_it_burst_to_the_body() {
    // A direct hit kills from full health.
    let mut s = range("", 4, &[[0.8, 1.]]);
    let d = (chest(&s.players[1]) - muzzle(&s.players[0])).normalize();
    assert!(s.fire(1, d, 4));
    fly(&mut s);
    let record = s.players[1].last_damage;
    assert!(!s.players[1].alive && (record.kind, record.amount) == (4, MAX_HP), "{record:?}");
    // Its blast at the chest's height, `gap` from the body's side: all of it within LETHAL, falling smoothly to nothing
    // at the splash radius, and about half mid-way.
    let glob = Projectile { owner: 1, weapon: 4, hostile: true, ..Default::default() };
    let mut last = f32::INFINITY;
    for k in 0..=24 {
        let gap = k as f32 * 0.01;
        let mut s = range("", 1, &[[1.15, 1.]]);
        s.burst(&glob, Vec3::new(1.15 - HOTDOG_RADIUS as f32 - gap, FLOOR + CHEST[0], 1.), None, None);
        let took = 100. - hp(&s, 2);
        assert!((took - MAX_HP * blast(gap, SPLASH[4])).abs() < 1e-3 && took <= last + 1e-4, "{gap} m: {took}");
        assert_eq!(s.players[1].alive, gap > LETHAL, "{gap} m: {took}");
        if gap >= SPLASH[4] {
            assert_eq!(took, 0., "{gap} m");
        }
        last = took;
    }
    let half = blast(SPLASH[4] / 2., SPLASH[4]);
    assert!(half > 0.4 && half < 0.65 && blast(LETHAL, SPLASH[4]) == 1. && blast(0., SPLASH[4]) == 1., "{half}");
    // (A stimmed hotdog takes twice what reaches it, as from any rival.)
    let mut s = range("", 1, &[[1.15, 1.]]);
    (s.players[1].hp, s.players[1].stim) = (1000., 3.);
    s.burst(&glob, Vec3::new(1.15 - HOTDOG_RADIUS as f32 - 0.09, FLOOR + CHEST[0], 1.), None, None);
    assert!((1000. - hp(&s, 2) - 2. * MAX_HP * blast(0.09, SPLASH[4])).abs() < 1e-2);
}

/// What a glob bursting at the counter right under hotdog 2 (at 1.15, 1) does to it, `up` off the ground (not standing
/// on anything when above it), crouch held or not.
fn under(up: f32, crouch: bool) -> f32 {
    let glob = Projectile { owner: 1, weapon: 4, hostile: true, ..Default::default() };
    let mut s = range("", 1, &[[1.15, 1.]]);
    let p = &mut s.players[1];
    (p.p[1], p.grounded, p.crouch) = (FLOOR + up, up == 0., crouch);
    s.burst(&glob, Vec3::new(1.15, FLOOR + 0.004, 1.), None, None);
    100. - hp(&s, 2)
}

#[test]
fn a_glob_at_the_feet_kills_a_hotdog_on_the_counter_wounds_one_at_the_top_of_its_jump_and_grazes_one_with_its_knees_tucked() {
    let apex = (actor::JUMP * actor::JUMP / (2. * actor::GRAVITY)) as f32;
    let (standing, crouched, jumped, tucked) = (under(0., false), under(0., true), under(apex, false), under(apex, true));
    assert_eq!((standing, crouched), (MAX_HP, MAX_HP), "on the counter, standing or crouched");
    assert!(jumped > 25. && jumped < 70., "at the top of a jump: {jumped}");
    assert!(tucked < jumped / 3. && tucked < 15., "tucked at the top of a jump: {tucked} (untucked {jumped})");
    // Halfway up, tucking helps too.
    assert!(under(apex / 2., true) < under(apex / 2., false));
    println!("a glob at the feet: standing {standing}, at the top of a jump {jumped}, tucked {tucked}");
}

#[test]
fn a_hotdog_tucked_in_the_air_aims_and_is_hit_as_one_standing_and_its_body_is_shortened_from_below() {
    let mut s = range("", 1, &[[1.15, 1.]]);
    let p = &mut s.players[1];
    (p.p[1], p.grounded, p.crouch) = (FLOOR + 0.1, false, true);
    let p = s.players[1].clone();
    assert!(tucked(&p) && stance(&p) == 0);
    assert_eq!((chest(&p).y, muzzle(&p).y), (FLOOR + 0.1 + CHEST[0], FLOOR + 0.1 + EYE), "the head stays where the jump put it");
    let (low, high) = figure(&p);
    assert!((low - (actor::HEIGHT - actor::CROUCH_HEIGHT) as f32).abs() < 1e-6 && (high - actor::HEIGHT as f32).abs() < 1e-6);
    // On the ground a crouch is as it was: lower all over.
    let mut q = p.clone();
    q.grounded = true;
    assert!(!tucked(&q) && stance(&q) == 1 && figure(&q) == (0., actor::CROUCH_HEIGHT as f32) && chest(&q).y == FLOOR + 0.1 + CHEST[1]);
    // Prone wins in the air too.
    q.prone = true;
    q.grounded = false;
    assert!(!tucked(&q) && stance(&q) == 2);
}

// ----- The ketchup sniper in the face (feast.rs FACE).

/// A sniper round from hotdog 1 at hotdog 2, 0.8 m off along +x, which faces `yaw` in `stance` (0 standing, 1 crouched,
/// 2 prone), with `hp`, aimed at `at` of it from the muzzle `up` higher than its own: what it took and its record.
fn snipe(yaw: f32, stance: usize, hp: f32, up: f32, at: impl Fn(&Player) -> Vec3) -> (f32, Damage) {
    let mut s = range("", SNIPER, &[[1.1, 1.]]);
    let p = &mut s.players[1];
    (p.yaw, p.crouch, p.prone, p.hp) = (yaw, stance == 1, stance == 2, hp);
    s.players[0].p[1] += up;
    let d = (at(&s.players[1]) - muzzle(&s.players[0])).normalize();
    assert!(s.fire(1, d, SNIPER));
    fly(&mut s);
    (hp - s.players[1].hp, s.players[1].last_damage)
}

#[test]
fn a_sniper_round_in_the_face_kills_from_full_health_while_the_body_still_takes_two() {
    use std::f32::consts::FRAC_PI_2;
    // Facing the shooter (-x), in the face: a kill, on the record as a face shot.
    let (took, record) = snipe(FRAC_PI_2, 0, 100., 0., face_point);
    assert_eq!((took, record.kind, plain_kind(record.kind), faced(record.kind)), (100., SNIPER as u8 | FACE_SHOT, SNIPER as u8, true));
    assert!(FACE[SNIPER] * PLAYER_DAMAGE[SNIPER] >= MAX_HP && (1..GUNS).filter(|w| *w != SNIPER && *w != PISTOL).all(|w| FACE[w] == 1.));
    // The same height from behind is the back of the head: the body's 55.
    let behind = |p: &Player| face_point(&Player { yaw: FRAC_PI_2, ..p.clone() });
    let (took, record) = snipe(-FRAC_PI_2, 0, 100., 0., behind);
    assert_eq!((took, record.kind), (PLAYER_DAMAGE[SNIPER], SNIPER as u8));
    // The chest from the front is the body: 55, and a second kills.
    assert_eq!(snipe(FRAC_PI_2, 0, 100., 0., chest).0, PLAYER_DAMAGE[SNIPER]);
    // From high above, onto the top of the bun: the body.
    let (took, record) = snipe(FRAC_PI_2, 0, 100., 0.6, |p| chest(p) + Vec3::Y * 0.05);
    assert_eq!((took, record.kind), (PLAYER_DAMAGE[SNIPER], SNIPER as u8));
    // Crouched, the face is lower with the body: a round at a standing face's height passes over, one at the crouched
    // face kills.
    assert_eq!(snipe(FRAC_PI_2, 1, 100., 0., |p| face_point(&Player { crouch: false, ..p.clone() })).0, 0.);
    let (took, record) = snipe(FRAC_PI_2, 1, 100., 0., face_point);
    assert!(took == 100. && faced(record.kind) && face_point(&Player { crouch: true, grounded: true, ..Player::default() }).y < CHEST[0]);
    // Prone, the face is the front end of the body lying along the way it faces, low on the counter.
    let (took, record) = snipe(FRAC_PI_2, 2, 100., 0., face_point);
    assert!(took == 100. && faced(record.kind), "{took} {record:?}");
    assert_eq!(snipe(-FRAC_PI_2, 2, 100., 0., chest).0, PLAYER_DAMAGE[SNIPER], "prone, from behind: the body");
    // Stimmed, a rival's round does twice what it would: in the body and in the face alike.
    let stimmed = |at: fn(&Player) -> Vec3| {
        let mut s = range("", SNIPER, &[[1.1, 1.]]);
        (s.players[1].yaw, s.players[1].hp, s.players[1].stim) = (FRAC_PI_2, 1000., 3.);
        let d = (at(&s.players[1]) - muzzle(&s.players[0])).normalize();
        assert!(s.fire(1, d, SNIPER));
        fly(&mut s);
        1000. - s.players[1].hp
    };
    assert_eq!((stimmed(chest), stimmed(face_point)), (2. * PLAYER_DAMAGE[SNIPER], 2. * FACE[SNIPER] * PLAYER_DAMAGE[SNIPER]));
    // It still goes on through: two rivals in a line, both in the face.
    let mut s = range("", SNIPER, &[[0.9, 1.], [1.3, 1.]]);
    (s.players[1].yaw, s.players[2].yaw) = (FRAC_PI_2, FRAC_PI_2);
    let d = (face_point(&s.players[1]) - muzzle(&s.players[0])).normalize();
    assert!(s.fire(1, d, SNIPER));
    fly(&mut s);
    assert!(!s.players[1].alive && !s.players[2].alive && faced(s.players[2].last_damage.kind));
}

// ----- The mustard pistol in the face: a hand cannon (feast.rs FACE, PISTOL).

/// A pistol round from hotdog 1 at hotdog 2, 0.8 m off along +x, which faces `yaw` with `hp`, aimed at `at` of it: what
/// it took and its record.
fn pop(yaw: f32, hp: f32, at: impl Fn(&Player) -> Vec3) -> (f32, Damage) {
    let mut s = range("", PISTOL, &[[1.1, 1.]]);
    (s.players[1].yaw, s.players[1].hp) = (yaw, hp);
    let d = (at(&s.players[1]) - muzzle(&s.players[0])).normalize();
    assert!(s.fire(1, d, PISTOL));
    fly(&mut s);
    (hp - s.players[1].hp, s.players[1].last_damage)
}

#[test]
fn a_mustard_pistol_round_in_the_face_kills_from_full_health_the_body_takes_eighteen_and_the_back_of_the_head_is_the_body() {
    use std::f32::consts::FRAC_PI_2;
    assert!(FACE[PISTOL] * PLAYER_DAMAGE[PISTOL] >= MAX_HP && PLAYER_DAMAGE[PISTOL] == 18.);
    assert!(5. * PLAYER_DAMAGE[PISTOL] < MAX_HP && 6. * PLAYER_DAMAGE[PISTOL] >= MAX_HP, "six in the body");
    // Facing the shooter, in the face: a kill from full health, on the record as a face shot of the pistol.
    let (took, record) = pop(FRAC_PI_2, 100., face_point);
    assert_eq!((took, record.kind, plain_kind(record.kind), faced(record.kind)), (100., PISTOL as u8 | FACE_SHOT, PISTOL as u8, true));
    // The chest: 18.
    let (took, record) = pop(FRAC_PI_2, 100., chest);
    assert_eq!((took, record.kind), (PLAYER_DAMAGE[PISTOL], PISTOL as u8));
    // The same height from behind is the back of the head: the body's 18.
    let behind = |p: &Player| face_point(&Player { yaw: FRAC_PI_2, ..p.clone() });
    let (took, record) = pop(-FRAC_PI_2, 100., behind);
    assert_eq!((took, record.kind), (PLAYER_DAMAGE[PISTOL], PISTOL as u8));
    // Stimmed, twice (still a kill in the face).
    let mut s = range("", PISTOL, &[[1.1, 1.]]);
    (s.players[1].yaw, s.players[1].hp, s.players[1].stim) = (FRAC_PI_2, 1000., 3.);
    let d = (face_point(&s.players[1]) - muzzle(&s.players[0])).normalize();
    assert!(s.fire(1, d, PISTOL));
    fly(&mut s);
    assert_eq!(1000. - s.players[1].hp, 2. * FACE[PISTOL] * PLAYER_DAMAGE[PISTOL]);
}

// ----- The syrup rifle's burst at a hotdog walking across (feast.rs VOLLEY_GAP, `volleys`).

/// Hotdog 1 fires one syrup burst at hotdog 2, `far` metres off along +x, which walks across along +z at a person's walk
/// (or stands, `walking` false); each step hotdog 1 looks at the point `aim` gives from the target's chest, its velocity
/// and the burst's start (the chest and velocity then). Returns how many of the three rounds hit.
fn burst_at(far: f32, walking: bool, aim: impl Fn(Vec3, Vec3, (Vec3, Vec3)) -> Vec3) -> u32 {
    const STEP: f32 = 1. / 60.;
    let mut s = range("", RIFLE, &[[0.3 + far, 0.6]]);
    s.players[1].hp = 1000.;
    let v = Vec3::Z * if walking { actor::WALK as f32 } else { 0. };
    let start = (chest(&s.players[1]), v);
    let mut hits = 0;
    for step in 0..60 {
        let o = muzzle(&s.players[0]);
        let d = (aim(chest(&s.players[1]), v, start) - o).normalize();
        // As a person's command: the yaw with its walk, then the pitch (`look`), then the trigger.
        s.walk(1, Vec2::ZERO, (-d.x).atan2(-d.z));
        s.look(1, d.y.asin());
        if step == 0 {
            assert!(s.fire(1, d, RIFLE));
        }
        s.walk(2, Vec2::new(v.x, v.z) * STEP, 0.);
        let before = s.players[1].hp;
        s.tick(STEP);
        hits += (before - s.players[1].hp > 0.9 * PLAYER_DAMAGE[RIFLE]) as u32;
    }
    hits
}

#[test]
fn a_burst_at_one_spot_lands_one_round_on_a_hotdog_walking_across_a_burst_swept_with_it_and_led_lands_all_three() {
    let o = Vec3::new(0.3, FLOOR + EYE, 1.);
    // The burst's rounds leave 0.2 s apart end to end: a walker covers 0.25 m meanwhile, twice the width a round can hit
    // it in (its sphere and the round's size, 0.128 m), and 0.125 m between two rounds, so one spot cannot hold all three
    // and hardly two.
    let width = 2. * (0.055 + ROUND_RADIUS[RIFLE]);
    assert!(2. * VOLLEY_GAP * actor::WALK as f32 > 1.9 * width && VOLLEY_GAP * actor::WALK as f32 > 0.95 * width, "{width}");
    let led = |c: Vec3, v: Vec3| c + v * (c.distance(o) / ROUND_SPEED[RIFLE]);
    for far in [1.2, 2., 3.] {
        // Standing still: all three, at the chest.
        assert_eq!(burst_at(far, false, |c, _, _| c), 3, "standing, {far} m");
        // Walking across, the burst aimed at one spot: where the chest will be as the middle round gets there.
        let spot = |_: Vec3, _: Vec3, (c, v): (Vec3, Vec3)| led(c + v * VOLLEY_GAP, v);
        assert_eq!(burst_at(far, true, spot), 1, "one spot, {far} m");
        // Swept with it and led by the round's flight: all three.
        assert_eq!(burst_at(far, true, |c, v, _| led(c, v)), 3, "swept and led, {far} m");
    }
    // Swept with it but not led, at 2 m and more: the rounds pass behind it (a walker crosses its width in the round's flight).
    for far in [2., 3.] {
        assert_eq!(burst_at(far, true, |c, _, _| c), 0, "swept, not led, {far} m");
    }
}

#[test]
fn each_round_of_a_burst_leaves_where_the_gun_points_as_it_leaves() {
    let mut s = range("", RIFLE, &[]);
    assert!(s.fire(1, Vec3::X, RIFLE));
    let mut ways = vec![Vec3::from_array(s.projectiles[0].v).normalize()];
    for k in 0..15 {
        // The look turns steadily to the right (-z from +x) and up a little through the burst.
        let (yaw, pitch) = (-std::f32::consts::FRAC_PI_2 + 0.01 * (k + 1) as f32, 0.002 * (k + 1) as f32);
        s.walk(1, Vec2::ZERO, yaw);
        s.look(1, pitch);
        let n = s.projectiles.len();
        s.tick(1. / 60.);
        if s.projectiles.len() > n {
            let way = Vec3::from_array(s.projectiles.last().unwrap().v).normalize();
            let k = ways.len() as f32;
            let look = sightline(yaw, pitch);
            let climbed = Vec3::new(look.x, look.y + CLIMB * k, look.z).normalize();
            assert!(way.angle_between(climbed) < 1e-5, "round {k} leaves along the look as it is ({way} against {climbed})");
            ways.push(way);
        }
    }
    assert_eq!(ways.len(), 3);
    assert!(ways[1].z < ways[0].z - 0.04 && ways[2].z < ways[1].z - 0.04, "it follows the turn: {ways:?}");
}

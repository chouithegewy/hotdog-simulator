// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The mayonnaise bottle (weapon 7): its arc, its flow, stim, its rounds, its damage, the bots, and how a frame
//! carries it.
use super::*;
use crate::emergence::WAVE;
use crate::net::{self, Batch, Command, Frame, Seat};

/// A bare counter six metres by two, with `pickups`, and hotdog 1 at (0.3, 1) holding the bottle, full, facing +x,
/// with the mustard pistol beside it (the bottle takes the place of the syrup rifle it starts with). The others
/// stand out of the way at the far end, and nobody is a bot.
fn bench(pickups: &str, blocks: &str) -> Feast {
    let map = Map::parse(&format!(
        r#"{{"format": "hotdog-map", "version": 1, "name": "Bench", "bounds": [0, 0, 6, 2], "starts": [{{"p": [0.3, 1]}}], "spawns": [{{"p": [5.9, 0.1]}}], "pickups": [{pickups}], "blocks": [{blocks}]}}"#
    ))
    .unwrap();
    let mut s = Feast::with_map(map, false);
    for (i, p) in s.players.iter_mut().enumerate() {
        (p.bot, p.p) = (false, [5.6, FLOOR, 0.2 + 0.35 * i as f32]);
    }
    let p = &mut s.players[0];
    (p.p, p.yaw) = ([0.3, FLOOR, 1.], -std::f32::consts::FRAC_PI_2);
    (p.owned[MAYO], p.ammo[MAYO], p.weapon, p.gun) = (true, FULL[MAYO], MAYO, MAYO);
    (p.owned[RIFLE], p.ammo[RIFLE]) = (false, 0);
    s.embody();
    s
}
/// Aims along +x, `degrees` above the level.
fn up(degrees: f32) -> Vec3 {
    let a = degrees.to_radians();
    Vec3::new(a.cos(), a.sin(), 0.)
}
/// One droplet squirted `degrees` up: how far across the counter from the muzzle it lands, and after how long.
fn landing(degrees: f32, stim: bool) -> (f32, f32) {
    let mut s = bench("", "");
    if stim {
        assert!(s.stim(1));
    }
    let (o, t0, first) = (muzzle(&s.players[0]), s.elapsed, s.next_splat);
    assert!(s.fire(1, up(degrees), MAYO));
    for _ in 0..600 {
        s.tick(1. / 60.);
        if let Some(splat) = s.splats.iter().find(|splat| splat.id >= first) {
            assert!((splat.p[1] - FLOOR).abs() < 1e-3, "it lands on the counter: {:?}", splat.p);
            return (splat.p[0] - o.x, splat.time - t0);
        }
    }
    panic!("the droplet never landed");
}

#[test]
fn a_droplet_arcs_and_falls_under_gravity_and_aiming_up_squirts_it_further_up_to_about_45_degrees() {
    let fall = DROOP[MAYO];
    // (The game's gravity times SPEEDUP squared: the stream flies five times as fast as it once did, along the arcs it
    // always had.)
    assert_eq!(fall, actor::GRAVITY as f32 * SPEEDUP * SPEEDUP);
    assert!(DROOP.iter().enumerate().all(|(w, g)| (w == MAYO) == (*g > 0.)), "only the mayonnaise falls");
    // Followed step by step, a droplet is where the arc puts it: its height drops by gravity's square law.
    let mut s = bench("", "");
    let (o, d) = (muzzle(&s.players[0]), up(30.));
    assert!(s.fire(1, d, MAYO));
    let launch = d * ROUND_SPEED[MAYO];
    for k in 1..=9 {
        s.tick(1. / 60.);
        let q = &s.projectiles[0];
        let t = k as f32 / 60.;
        let at = o + launch * t - Vec3::Y * 0.5 * fall * t * t;
        assert!(Vec3::from_array(q.p).distance(at) < 1e-4, "at {t} s it is at {:?}, not {at}", q.p);
        assert!((q.v[1] - (launch.y - fall * t)).abs() < 1e-4, "its fall speeds up");
    }
    // How far it lands from the muzzle at each pitch, as the arc says (the muzzle is EYE above the counter).
    let mut ranges = vec![];
    for degrees in [-10., 0., 10., 20., 30., 40., 45., 50., 60., 75.] {
        let (far, time) = landing(degrees, false);
        let expected = throw(ROUND_SPEED[MAYO], fall, (degrees as f32).to_radians(), EYE);
        println!("aimed {degrees:>4} degrees up: lands {far:.3} m away after {time:.2} s (the arc says {expected:.3})");
        assert!((far - expected).abs() < 0.02, "{degrees} degrees: {far} against {expected}");
        ranges.push((degrees, far));
    }
    for w in ranges.windows(2).filter(|w| w[1].0 <= 40.) {
        assert!(w[1].1 > w[0].1, "aiming higher squirts further: {w:?}");
    }
    let best = ranges.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    assert!((40. ..=45.).contains(&best.0), "furthest about 45 degrees up: {best:?}");
    assert!(ranges.iter().find(|r| r.0 == 75.).unwrap().1 < ranges.iter().find(|r| r.0 == 45.).unwrap().1 * 0.7, "too high falls short");
    // It has no reach of its own: flat it falls to the counter in under a metre, lobbed it reaches 1.8 m.
    assert!(ranges[1].1 < 0.8 && best.1 > 1.8, "{ranges:?}");
}

#[test]
fn a_droplet_lands_on_a_block_on_the_way_and_one_squirted_past_the_edge_falls_to_the_kitchen_floor() {
    let mut s = bench("", r#"{"rect": [1.0, 0.5, 1.2, 1.5], "height": 0.3}"#);
    assert!(s.fire(1, up(20.), MAYO));
    let landed = splat(&mut s, 2.);
    assert!((landed.p[0] - 1.0).abs() < 0.01 && landed.p[1] > FLOOR, "it hit the block's face: {:?}", landed.p);
    // Off the counter's end (x < 0) it goes on down to the floor.
    let mut s = bench("", "");
    s.players[0].p = [0.05, FLOOR, 1.];
    s.embody();
    assert!(s.fire(1, Vec3::new(-0.7, 0.7, 0.), MAYO));
    let landed = splat(&mut s, 3.);
    assert!(landed.p[1].abs() < 0.01 && landed.p[0] < 0., "on the kitchen floor: {:?}", landed.p);
}
/// Steps on until the next splat, within `seconds`.
fn splat(s: &mut Feast, seconds: f32) -> Splat {
    let first = s.next_splat;
    for _ in 0..(seconds * 60.) as usize {
        s.tick(1. / 60.);
        if let Some(splat) = s.splats.iter().find(|splat| splat.id >= first) {
            return splat.clone();
        }
    }
    panic!("nothing landed");
}

/// Holds the trigger for `seconds` at steps of `dt`, firing as the server does (every step, the simulation refusing
/// what it cannot): how many droplets left the bottle.
fn hold(s: &mut Feast, d: Vec3, seconds: f32, dt: f32) -> usize {
    let before = s.next_round;
    for _ in 0..(seconds / dt).round() as usize {
        s.fire(1, d, MAYO);
        s.tick(dt);
    }
    (s.next_round - before) as usize
}

#[test]
fn held_the_trigger_squirts_a_steady_stream_at_any_step_and_stim_squirts_it_faster_and_further() {
    // Twelve droplets a second, at 60 steps a second as at 30 or 144.
    for dt in [1. / 60., 1. / 30., 1. / 144.] {
        let mut s = bench("", "");
        let n = hold(&mut s, up(30.), 3., dt);
        println!("3 s held at {:.0} steps a second: {n} droplets", 1. / dt);
        assert!((35..=37).contains(&n), "{n} droplets in 3 s at {dt}");
    }
    // Stimmed: half as many again, and faster out of the nozzle.
    let mut s = bench("", "");
    assert!(s.stim(1));
    let before = s.next_round;
    let n = hold(&mut s, up(30.), 3., 1. / 60.);
    println!("3 s held, stimmed: {n} droplets");
    assert!((53..=55).contains(&n), "{n} droplets in 3 s stimmed");
    let speeds: Vec<f32> = s.projectiles.iter().filter(|q| q.id >= before as u32).map(|q| Vec3::from_array(q.launch).length()).collect();
    assert!(speeds.iter().all(|v| (v - ROUND_SPEED[MAYO] * STIM_SQUIRT).abs() < 1e-4), "{speeds:?}");
    // Further: about twice as far at 45 degrees (the reach goes with the square of the speed).
    let ((plain, _), (stimmed, time)) = (landing(45., false), landing(45., true));
    println!("45 degrees up: {plain:.2} m, stimmed {stimmed:.2} m after {time:.2} s");
    assert!(stimmed > plain * 1.8 && stimmed < plain * 2.1, "{plain} {stimmed}");
    assert!(STIM_SQUIRT >= 1.35 && STIM_SQUIRT <= 1.5 && STIM_FLOW >= 1.4 && STIM_FLOW <= 1.5);
}

#[test]
fn the_bottle_drains_as_it_squirts_runs_dry_reloads_from_its_reserve_and_a_box_of_ammunition_or_a_bottle_on_the_counter_fills_that() {
    use crate::magazine::{CARRY, RELOAD};
    let mut s = bench(r#"{"p": [0.3, 1.3], "weapon": 0}, {"p": [0.5, 1.3], "weapon": 7, "ammo": 150}"#, "");
    let n = hold(&mut s, up(10.), 2., 1. / 60.);
    assert_eq!(s.players[0].ammo[MAYO], FULL[MAYO] - n as i32);
    // Dry, with nothing in reserve: the trigger does nothing.
    (s.players[0].ammo[MAYO], s.players[0].reserve[MAYO]) = (1, 0);
    assert_eq!(hold(&mut s, up(10.), 1., 1. / 60.), 1);
    assert_eq!(s.players[0].ammo[MAYO], 0);
    assert!(!s.fire(1, up(10.), MAYO) && !s.reloading(1));
    // Walked over, a box of ammunition fills its reserve; then the trigger reloads it.
    s.players[0].p = [0.3, FLOOR, 1.3];
    s.embody();
    s.tick(1. / 60.);
    assert_eq!((s.players[0].ammo[MAYO], s.players[0].reserve[MAYO]), (0, CARRY[MAYO]));
    assert!(!s.fire(1, up(10.), MAYO) && s.reloading(1));
    s.tick(RELOAD[MAYO] + 0.05);
    assert_eq!((s.players[0].ammo[MAYO], s.players[0].reserve[MAYO]), (FULL[MAYO], CARRY[MAYO] - FULL[MAYO]));
    // A bottle on the counter, walked over, adds its rounds to the reserve.
    s.players[0].reserve[MAYO] = 10;
    s.players[0].p = [0.5, FLOOR, 1.3];
    s.embody();
    s.tick(1. / 60.);
    assert_eq!(s.players[0].reserve[MAYO], 160);
    // It is a gun of the two a hotdog carries: picked up beside two others it takes the place of the one in hand.
    let mut s = bench(r#"{"p": [0.4, 1.0], "weapon": 7, "ammo": 150}"#, "");
    let p = &mut s.players[0];
    (p.owned, p.ammo, p.weapon, p.gun) = ([false, true, true, false, false, false, false, false], [-1, 12, 6, 0, 0, 0, 0, 0], 2, 2);
    assert!(s.pickup(1, 0));
    let p = &s.players[0];
    assert_eq!((p.owned, p.weapon, p.ammo[MAYO]), ([false, true, false, false, false, false, false, true], MAYO, 150));
}

#[test]
fn droplets_do_little_each_so_the_stream_never_outdoes_the_hot_sauce_up_close_and_are_on_the_record_as_mayonnaise() {
    // Per second, held, stimmed or not, against a zombie and against a rival.
    let rate = |stim: bool| if stim { STIM_FLOW } else { 1. } / DELAY[MAYO];
    for stim in [false, true] {
        assert!(ZOMBIE_DAMAGE[MAYO] * rate(stim) < ZOMBIE_DAMAGE[3] / DELAY[3]);
        assert!(PLAYER_DAMAGE[MAYO] * rate(stim) < PLAYER_DAMAGE[3] / DELAY[3]);
    }
    println!(
        "mayonnaise: {} a droplet to a zombie, {} to a hotdog, {:.0} droplets a second ({:.0} stimmed): {:.0} / {:.0} a second ({:.0} / {:.0} stimmed); hot sauce {:.0} / {:.0}",
        ZOMBIE_DAMAGE[MAYO], PLAYER_DAMAGE[MAYO], rate(false), rate(true),
        ZOMBIE_DAMAGE[MAYO] * rate(false), PLAYER_DAMAGE[MAYO] * rate(false), ZOMBIE_DAMAGE[MAYO] * rate(true), PLAYER_DAMAGE[MAYO] * rate(true),
        ZOMBIE_DAMAGE[3] / DELAY[3], PLAYER_DAMAGE[3] / DELAY[3]
    );
    // A zombie 0.5 m off, squirted flat: each droplet that lands on it takes ZOMBIE_DAMAGE.
    let mut s = bench("", "");
    s.stage = Stage::Wave;
    s.wave_spawned = WAVE;
    s.enemies.push(Zombie { id: 1, p: [0.8, FLOOR, 1.], yaw: 0., hp: 1e6, born: 0., forming: 0. });
    let d = (Vec3::new(0.8, FLOOR + 0.075, 1.) - muzzle(&s.players[0])).normalize();
    let n = hold(&mut s, d, 2., 1. / 60.);
    s.tick(0.5);
    let taken = 1e6 - s.enemies[0].hp;
    assert!(taken > 0. && (taken / ZOMBIE_DAMAGE[MAYO]).fract().abs() < 1e-3, "{taken}");
    assert!(taken / ZOMBIE_DAMAGE[MAYO] >= n as f32 * 0.8, "most of the stream lands: {} of {n}", taken / ZOMBIE_DAMAGE[MAYO]);
    // A rival in the free for all: PLAYER_DAMAGE a droplet (beside the hunger both of them feel), on the record as
    // mayonnaise from hotdog 1.
    let mut s = bench("", "");
    (s.stage, s.famine_at) = (Stage::FreeForAll, s.elapsed);
    s.players[1].p = [0.8, FLOOR, 1.];
    s.embody();
    let d = (chest(&s.players[1]) - muzzle(&s.players[0])).normalize();
    let n = hold(&mut s, d, 0.5, 1. / 60.);
    s.tick(0.3);
    // (Squirted straight at the chest half a metre off, the stream falls a little: most of it lands.)
    let hits = (s.players[0].hp - s.players[1].hp) / PLAYER_DAMAGE[MAYO];
    assert!((hits - hits.round()).abs() < 1e-2 && hits.round() as usize >= n / 2, "{hits} hits of {n}");
    let mut s = bench("", "");
    (s.stage, s.famine_at) = (Stage::FreeForAll, s.elapsed);
    s.players[1].p = [0.8, FLOOR, 1.];
    s.embody();
    assert!(s.fire(1, (chest(&s.players[1]) - muzzle(&s.players[0])).normalize(), MAYO));
    for _ in 0..30 {
        s.flight(1. / 60.);
    }
    let record = s.players[1].last_damage;
    assert_eq!((record.kind, record.attacker), (MAYONNAISE, 1));
    assert!((record.amount - PLAYER_DAMAGE[MAYO]).abs() < 1e-4, "{record:?}");
    assert_eq!(kind_of(MAYO), MAYONNAISE);
}

/// A bot (hotdog 1) holding only the bottle in the middle of a yard in a wave that is already complete, a formed
/// zombie `far` metres off towards -x that cannot reach it, the blocks given between them; it is stepped for
/// `seconds`. Returns how many droplets landed on the zombie, how many the bot squirted, and the furthest it was
/// from the zombie when it squirted.
fn bot_against(far: f32, blocks: &str, seconds: f32) -> (f32, usize, f32) {
    let mut s = bench("", blocks);
    (s.stage, s.wave_spawned) = (Stage::Wave, WAVE);
    let p = &mut s.players[0];
    (p.bot, p.p, p.hp) = (true, [3., FLOOR, 1.], 1e6);
    (p.owned[1], p.ammo[1]) = (false, 0);
    s.enemies.push(Zombie { id: 1, p: [3. - far, FLOOR, 1.], yaw: 0., hp: 1e6, born: 0., forming: 0. });
    s.embody();
    let (before, mut furthest) = (s.next_round, 0f32);
    for _ in 0..(seconds * 30.) as usize {
        let fired = s.next_round;
        s.tick(1. / 30.);
        if s.next_round > fired {
            furthest = furthest.max((s.players[0].p[0] - (3. - far)).abs());
        }
        // (The zombie is held where it is.)
        s.enemies[0].p = [3. - far, FLOOR, 1.];
        s.embody();
    }
    ((1e6 - s.enemies[0].hp) / ZOMBIE_DAMAGE[MAYO], (s.next_round - before) as usize, furthest)
}

#[test]
fn a_bot_squirts_the_bottle_along_its_arc_at_a_zombie_and_lobs_it_over_a_wall_it_cannot_see_past() {
    // In the open, within its throw, it hits it.
    let (hits, squirted, _) = bot_against(1.0, "", 3.);
    println!("in the open at 1 m: {hits} of {squirted} droplets landed");
    assert!(squirted > 20 && hits > squirted as f32 * 0.6, "{hits} of {squirted}");
    // Behind a wall taller than both of them, straight across the yard: over it, on the high arc.
    let wall = r#"{"rect": [2.45, 0, 2.55, 2], "height": 0.25}"#;
    let (hits, squirted, _) = bot_against(1.0, wall, 4.);
    println!("over a wall at 1 m: {hits} of {squirted} droplets landed");
    assert!(squirted > 20 && hits > squirted as f32 * 0.4, "{hits} of {squirted}");
    // Out of its throw it does not waste the bottle: it walks in until it can reach.
    let (_, squirted, furthest) = bot_against(2.6, "", 4.);
    assert!(squirted > 0 && furthest <= throw(ROUND_SPEED[MAYO], MAYO_FALL, 0.8, EYE - 0.075) + 0.02, "squirted {squirted} from {furthest} m");
}

#[test]
fn a_bot_fetches_the_bottle_and_in_the_free_for_all_squirts_its_rival_with_it() {
    let mut s = bench(r#"{"p": [1.0, 1.0], "weapon": 7, "ammo": 150}"#, "");
    let p = &mut s.players[0];
    // The guns a bot starts with: the bottle takes the pistol's place.
    (p.bot, p.owned, p.ammo, p.weapon, p.gun) = (true, [false, true, false, false, false, false, true, false], [-1, 60, 0, 0, 0, 0, 60, 0], RIFLE, RIFLE);
    for _ in 0..240 {
        s.tick(1. / 30.);
    }
    let p = &s.players[0];
    assert_eq!((p.owned, p.ammo[MAYO], p.ammo[RIFLE]), ([false, false, false, false, false, false, true, true], 150, 60), "it went and got the bottle");
    // A rival 1.2 m off: it squirts it, and it hurts.
    let mut s = bench("", "");
    (s.stage, s.famine_at) = (Stage::FreeForAll, -10.);
    s.players[0].bot = true;
    s.players[0].hp = 90.;
    s.players[1].p = [1.5, FLOOR, 1.];
    for p in s.players.iter_mut().skip(2) {
        (p.alive, p.hp) = (false, 0.);
        p.consumed = true;
    }
    s.embody();
    for _ in 0..60 {
        s.tick(1. / 30.);
    }
    assert!(s.next_round > 10, "it squirted");
    let hits = ((100. - s.players[1].hp) - (90. - s.players[0].hp)) / PLAYER_DAMAGE[MAYO];
    assert!(hits > 10., "and hit, {hits} times");
}

#[test]
fn a_frame_sends_a_falling_droplet_once_as_it_left_the_nozzle_and_the_client_puts_it_on_its_arc() {
    let mut s = bench("", "");
    assert!(s.fire(1, up(40.), MAYO));
    s.tick(0.04);
    let frame = |s: &Feast| Frame { kind: "snapshot".into(), elapsed: s.elapsed, projectiles: s.projectiles.clone(), ..Default::default() };
    let (a, at_a) = (frame(&s).state(), s.projectiles[0].p);
    s.tick(0.06);
    let (b, at_b) = (frame(&s).state(), s.projectiles[0].p);
    // The record is the same frame after frame: nothing to send after the first.
    assert_eq!(a.tables[net::ROUNDS], b.tables[net::ROUNDS]);
    let mut w = net::Writer::new();
    net::write_state(&mut w, Some(&a), &b);
    assert!(w.bytes.len() <= 1, "a delta of an unchanged round is empty: {} bytes", w.bytes.len());
    // And read at either moment it is where the server has it, to a few millimetres.
    for (state, elapsed, at) in [(&a, s.elapsed - 0.06, at_a), (&b, s.elapsed, at_b)] {
        let q = &Frame::from_state(state, elapsed).projectiles[0];
        assert!(Vec3::from_array(q.p).distance(Vec3::from_array(at)) < 0.004, "{:?} against {at:?}", q.p);
        assert_eq!((q.weapon, q.born, q.age > 0.), (MAYO, 0., true));
    }
    // Rounds that fly straight are sent as before: where they are.
    let mut s = bench("", "");
    (s.players[0].owned[1], s.players[0].ammo[1]) = (true, 12);
    assert!(s.fire(1, up(0.), 1));
    s.tick(0.1);
    let rec = &frame(&s).state().tables[net::ROUNDS][0];
    assert_eq!(rec.ints.len(), 10);
    assert_eq!(rec.ints[2], (s.projectiles[0].p[0] * 1000.).round() as i64);
}

#[test]
fn protocol_12_still_reads_both_ways_the_seventh_guns_rounds_go_at_the_end_of_a_hotdogs_record() {
    let mut s = bench("", "");
    s.players[0].ammo = [-1, 12, 6, 90, 4, 40, 60, 77];
    s.players[0].owned[2] = true;
    let seats: Vec<Seat> = s.players.iter().map(|p| Seat { player: p.clone(), ..Default::default() }).collect();
    let frame = Frame { kind: "snapshot".into(), elapsed: s.elapsed, players: seats, ..Default::default() };
    let state = frame.state();
    let rec = state.tables[net::PLAYERS][0].clone();
    // The six guns' rounds in the places protocol 12 always had them (after the position, angles, velocity, HP, the
    // flags, the gun, the last gun, carry, score, kills, meals, swings and team), then the owned mask, ...
    assert_eq!(&rec.ints[18..25], &[-1, 12, 6, 90, 4, 40, 60]);
    assert_eq!(rec.ints[25], 0b1000_0110);
    // ... and the bottle's after everything a client of the six guns looks at (the last damage, which ends at 39).
    assert_eq!(rec.ints[40], 77);
    let back = &Frame::from_state(&state, frame.elapsed).players[0].player;
    assert_eq!((back.ammo, back.owned), (s.players[0].ammo, s.players[0].owned));
    // A server of the six guns sends nothing after the last damage: the bottle is not there.
    let mut old = state.clone();
    for r in &mut old.tables[net::PLAYERS] {
        r.ints.truncate(40);
        r.ints[25] &= 0x7f;
    }
    let back = &Frame::from_state(&old, frame.elapsed).players[0].player;
    assert_eq!((&back.ammo[..7], back.ammo[MAYO], back.owned[MAYO]), (&s.players[0].ammo[..7], 0, false));
    // Its JSON (the debug encoding) has seven places; it reads as well.
    let json = r#"{"id": 3, "ammo": [-1, 12, 0, 0, 0, 0, 60], "owned": [false, true, false, false, false, false, true]}"#;
    let p: Player = serde_json::from_str(json).unwrap();
    assert_eq!((p.ammo, p.owned[6], p.owned[MAYO]), ([-1, 12, 0, 0, 0, 0, 60, 0], true, false));
    // A command names the bottle; nothing past it.
    let c = Command::new(1, Vec2::ZERO, 0., 0., 0, MAYO as u8, None);
    assert_eq!(Batch::decode(&Batch { commands: vec![c], ..Default::default() }.encode()).unwrap().commands[0].weapon, 7);
    let mut bytes = Batch { commands: vec![c], ..Default::default() }.encode();
    let at = bytes.len() - 1;
    bytes[at] = 8;
    assert!(Batch::decode(&bytes).is_err());
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The bots' aim (aim.rs): its tests, and the harness that measures it (the ignored `measure_*`, which print the
//! tables of .superpowers/sdd/adhoc/aim-report.md and speed-report.md: `cargo test --release -- --ignored --nocapture
//! measure_`, and `SKILLS=easy,hard` for other skills; `measure_pistol` the mustard pistol at a target facing the bot,
//! `measure_bursts` how many rounds of each syrup burst land).
use super::*;
use crate::aim::{SETTLE, Skill};
use crate::rules::Rules;

const DT: f32 = 1. / 60.;

/// A bare counter eight metres by four, in the free for all; nobody is a bot, and everyone but hotdogs 1 (the target)
/// and 2 (the shooter) stands out of the way at the far end.
fn range() -> Feast {
    let map = Map::parse(r#"{"format": "hotdog-map", "version": 1, "name": "Range", "bounds": [0, 0, 8, 4], "starts": [{"p": [0.5, 2]}], "spawns": [{"p": [7.9, 0.1]}]}"#).unwrap();
    let mut s = Feast::with_map(map, false);
    // (The sandbox: no hunger, no pests, no end.)
    s.set_rules(Rules::parse("sandbox"));
    for (i, p) in s.players.iter_mut().enumerate() {
        (p.bot, p.p) = (false, [7.6, FLOOR, 0.2 + 0.3 * i as f32]);
    }
    s.embody();
    s
}
fn put(s: &mut Feast, id: u32, x: f32, z: f32) {
    s.player_mut(id).unwrap().p = [x, FLOOR, z];
    s.embody();
}
/// Shooter 2 at (0.5, 2) fires `gun` at hotdog 1 `far` metres off along +x for `seconds`, the target standing or
/// strafing across at a person's walk. Returns (rounds fired, damage dealt / (rounds x a round's full damage)).
fn hit_rate(skill: Skill, gun: usize, far: f32, motion: usize, seconds: f32, seed: u32) -> (u32, f32) {
    let (strafe, known) = (motion > 0, motion > 1);
    let mut s = range();
    (s.rules.skill, s.card_seed) = (skill, seed as u64);
    let (x0, z0) = (0.5, 2.);
    put(&mut s, 2, x0, z0);
    put(&mut s, 1, x0 + far, z0);
    {
        let p = s.player_mut(2).unwrap();
        (p.owned[gun], p.weapon, p.gun) = (true, gun, gun);
    }
    let (mut damage, mut rounds, mut last_shot) = (0f32, 0u32, 0u32);
    let (mut side, mut leg, mut lcg) = (1f32, 0f32, seed.wrapping_mul(2654435761).wrapping_add(12345));
    let mut next = || {
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        (lcg >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut until = 0.5 + 0.6 * next();
    for _ in 0..(seconds / DT) as usize {
        {
            let p = s.player_mut(2).unwrap();
            (p.ammo[gun], p.reserve[gun], p.reload) = (FULL[gun].max(1), 0, None);
            s.player_mut(1).unwrap().hp = 1e4;
        }
        // The target: standing (put back if a hit shoved it), or strafing to and fro across the line of fire.
        let t = s.player(1).unwrap().p;
        if strafe {
            leg += DT;
            if leg >= until || (t[2] - z0).abs() > 0.45 && (t[2] - z0).signum() == side {
                (leg, until) = (0., 0.5 + 0.6 * next());
                side = if (t[2] - z0).abs() > 0.45 { -(t[2] - z0).signum() } else { -side };
            }
            let step = Vec2::new((x0 + far - t[0]) * 0.5, side * actor::WALK as f32 * DT);
            s.walk(1, step, 0.);
            // (A person's walk is not reported as its velocity; a bot's is.)
            if known {
                s.player_mut(1).unwrap().v = [0., 0., side * actor::WALK as f32];
            }
        } else if (t[0] - x0 - far).abs() + (t[2] - z0).abs() > 0.004 {
            put(&mut s, 1, x0 + far, z0);
        }
        shoot(&mut s, gun);
        s.tick(DT);

        damage += (1e4 - s.player(1).unwrap().hp).max(0.);
        let fresh: Vec<u32> = s.shots.iter().filter(|shot| shot.owner == 2 && shot.id > last_shot).map(|shot| shot.id).collect();
        rounds += fresh.len() as u32;
        last_shot = fresh.into_iter().fold(last_shot, u32::max);
    }
    (rounds, damage / (rounds.max(1) as f32 * s.tune.hotdog[gun]))
}
/// The bot's aim (aim.rs), and its trigger once it has reacted.
fn shoot(s: &mut Feast, gun: usize) {
    let t = s.player(1).unwrap().clone();
    let v = Vec3::from_array(t.v);
    let o = muzzle(s.player(2).unwrap());
    if gun == MAYO {
        if let Some((d, ready)) = s.draw_lob(2, 1, chest(&t), v) {
            s.loose(2, d, ready, gun);
        }
    } else {
        let (mark, face) = s.mark(o, &t, gun);
        let (d, ready) = s.draw_bead(2, 1, mark, v, gun, face);
        s.loose(2, d, ready, gun);
    }
}

/// The hit rates of every gun, standing and strafing, by skill: (rounds, share of their damage that landed).
fn rates(skill: Skill, gun: usize, far: f32, motion: usize, seeds: u32, seconds: f32) -> (u32, f32) {
    let (mut rounds, mut rate) = (0, 0.);
    for seed in 0..seeds {
        let (n, r) = hit_rate(skill, gun, far, motion, seconds, seed);
        rounds += n;
        rate += r / seeds as f32;
    }
    (rounds, rate)
}
const CASES: [(&str, usize, f32); 9] = [
    ("mustard pistol", 1, 1.2),
    ("syrup rifle", RIFLE, 1.2),
    ("syrup rifle (long)", RIFLE, 2.5),
    ("ketchup sniper", SNIPER, 1.2),
    ("ketchup sniper (long)", SNIPER, 2.5),
    ("relish bazooka", 4, 1.2),
    ("hot sauce", 3, 0.4),
    ("mayo", MAYO, 1.0),
    ("syrup rifle (close)", RIFLE, 0.5),
];

#[test]
#[ignore]
fn measure_hit_rates() {
    let skills: Vec<Skill> = match std::env::var("SKILLS") { Ok(list) => list.split(',').filter_map(Skill::named).collect(), Err(_) => Skill::ALL.to_vec() };
    for skill in skills {
        println!("{}:", skill.name());
        println!("| gun | range | standing | strafing person | strafing, velocity reported |");
        for (name, gun, far) in CASES {
            let cells: Vec<String> = [0, 1, 2].iter().map(|motion| {
                let (rounds, rate) = rates(skill, gun, far, *motion, 6, 20.);
                format!("{:.0}% ({rounds})", rate * 100.)
            }).collect();
            println!("| {name} | {far} m | {} | {} | {} |", cells[0], cells[1], cells[2]);
        }
    }
}
/// The mustard pistol at a target facing the shooter (where a round in the face kills: feast.rs FACE), standing or
/// strafing at a walk, 1.2 m off: (rounds fired, share of them in the body, share in the face, mean seconds from the
/// first shot to each kill a whole hotdog would have died of: six body rounds, or one in the face).
fn pistol_rate(skill: Skill, motion: usize, seconds: f32, seed: u32) -> (u32, u32, u32, Vec<f32>) {
    let mut s = range();
    (s.rules.skill, s.card_seed) = (skill, seed as u64);
    let (x0, z0, far) = (0.5, 2., 1.2);
    put(&mut s, 2, x0, z0);
    put(&mut s, 1, x0 + far, z0);
    let face_on = std::f32::consts::FRAC_PI_2;
    s.player_mut(1).unwrap().yaw = face_on;
    {
        let p = s.player_mut(2).unwrap();
        (p.owned[1], p.weapon, p.gun) = (true, 1, 1);
    }
    let (mut rounds, mut body, mut face, mut last_shot) = (0u32, 0u32, 0u32, 0u32);
    let (mut taken, mut since, mut kills) = (0f32, None, vec![]);
    let (mut side, mut leg, mut lcg) = (1f32, 0f32, seed.wrapping_mul(2654435761).wrapping_add(12345));
    let mut next = || {
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        (lcg >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut until = 0.5 + 0.6 * next();
    for _ in 0..(seconds / DT) as usize {
        {
            let p = s.player_mut(2).unwrap();
            (p.ammo[1], p.reserve[1], p.reload) = (FULL[1], 0, None);
            s.player_mut(1).unwrap().hp = 1e4;
        }
        let t = s.player(1).unwrap().p;
        if motion > 0 {
            leg += DT;
            if leg >= until || (t[2] - z0).abs() > 0.45 && (t[2] - z0).signum() == side {
                (leg, until) = (0., 0.5 + 0.6 * next());
                side = if (t[2] - z0).abs() > 0.45 { -(t[2] - z0).signum() } else { -side };
            }
            s.walk(1, Vec2::new((x0 + far - t[0]) * 0.5, side * actor::WALK as f32 * DT), face_on);
        } else if (t[0] - x0 - far).abs() + (t[2] - z0).abs() > 0.004 {
            put(&mut s, 1, x0 + far, z0);
            s.player_mut(1).unwrap().yaw = face_on;
        }
        shoot(&mut s, 1);
        s.tick(DT);
        let took = (1e4 - s.player(1).unwrap().hp).max(0.);
        if took > 0.5 * PLAYER_DAMAGE[1] {
            let faced = took >= MAX_HP;
            (body, face) = (body + !faced as u32, face + faced as u32);
            taken += took;
            if taken >= MAX_HP {
                kills.push(s.elapsed - since.unwrap_or(s.elapsed));
                (taken, since) = (0., None);
            }
        }
        let fresh: Vec<u32> = s.shots.iter().filter(|shot| shot.owner == 2 && shot.id > last_shot).map(|shot| shot.id).collect();
        if !fresh.is_empty() && since.is_none() {
            since = Some(s.elapsed);
        }
        rounds += fresh.len() as u32;
        last_shot = fresh.into_iter().fold(last_shot, u32::max);
    }
    (rounds, body, face, kills)
}

#[test]
#[ignore]
fn measure_pistol() {
    let skills: Vec<Skill> = match std::env::var("SKILLS") { Ok(list) => list.split(',').filter_map(Skill::named).collect(), Err(_) => Skill::ALL.to_vec() };
    println!("| skill | target (facing the bot, 1.2 m) | rounds | in the body | in the face | kills | mean seconds to a kill |");
    for skill in skills {
        for (motion, what) in [(0, "standing"), (1, "strafing")] {
            let (mut rounds, mut body, mut face, mut kills) = (0, 0, 0, vec![]);
            for seed in 0..6 {
                let (r, b, f, k) = pistol_rate(skill, motion, 20., seed);
                (rounds, body, face) = (rounds + r, body + b, face + f);
                kills.extend(k);
            }
            let mean = kills.iter().sum::<f32>() / kills.len().max(1) as f32;
            println!("| {} | {what} | {rounds} | {:.0}% | {:.0}% | {} | {mean:.2} |", skill.name(), 100. * body as f32 / rounds.max(1) as f32, 100. * face as f32 / rounds.max(1) as f32, kills.len());
        }
    }
}

/// How many rounds of each syrup burst a bot fires at a target `far` off land: standing or strafing at a walk (as in
/// `hit_rate`). Returns the bursts that landed 0, 1, 2 and 3 rounds.
fn burst_hits(skill: Skill, far: f32, motion: usize, seconds: f32, seed: u32) -> [u32; 4] {
    let mut s = range();
    (s.rules.skill, s.card_seed) = (skill, seed as u64);
    let (x0, z0) = (0.5, 2.);
    put(&mut s, 2, x0, z0);
    put(&mut s, 1, x0 + far, z0);
    {
        let p = s.player_mut(2).unwrap();
        (p.owned[RIFLE], p.weapon, p.gun) = (true, RIFLE, RIFLE);
    }
    let (mut out, mut open, mut hits) = ([0u32; 4], None::<f32>, 0usize);
    let (mut side, mut leg, mut lcg) = (1f32, 0f32, seed.wrapping_mul(2654435761).wrapping_add(12345));
    let mut next = || {
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        (lcg >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut until = 0.5 + 0.6 * next();
    for _ in 0..(seconds / DT) as usize {
        {
            let p = s.player_mut(2).unwrap();
            (p.ammo[RIFLE], p.reserve[RIFLE], p.reload) = (FULL[RIFLE], 0, None);
            s.player_mut(1).unwrap().hp = 1e4;
        }
        let t = s.player(1).unwrap().p;
        if motion > 0 {
            leg += DT;
            if leg >= until || (t[2] - z0).abs() > 0.45 && (t[2] - z0).signum() == side {
                (leg, until) = (0., 0.5 + 0.6 * next());
                side = if (t[2] - z0).abs() > 0.45 { -(t[2] - z0).signum() } else { -side };
            }
            s.walk(1, Vec2::new((x0 + far - t[0]) * 0.5, side * actor::WALK as f32 * DT), 0.);
        } else if (t[0] - x0 - far).abs() + (t[2] - z0).abs() > 0.004 {
            put(&mut s, 1, x0 + far, z0);
        }
        let before = s.player(2).unwrap().volley;
        shoot(&mut s, RIFLE);
        // (A burst begins when the trigger fires its first round: its rounds and their flight are over 0.45 s on.)
        if s.player(2).unwrap().volley > before && open.is_none() {
            open = Some(s.elapsed);
        }
        s.tick(DT);
        hits += ((1e4 - s.player(1).unwrap().hp) / (0.9 * PLAYER_DAMAGE[RIFLE])) as usize;
        if open.is_some_and(|at| s.elapsed - at > 0.45) {
            out[hits.min(3)] += 1;
            (open, hits) = (None, 0);
        }
    }
    out
}

#[test]
#[ignore]
fn measure_bursts() {
    let skills: Vec<Skill> = match std::env::var("SKILLS") { Ok(list) => list.split(',').filter_map(Skill::named).collect(), Err(_) => vec![Skill::Easy, Skill::Normal, Skill::Hard] };
    println!("| skill | target | range | bursts | none land | 1 | 2 | all 3 |");
    for skill in skills {
        for (motion, what) in [(0, "standing"), (1, "strafing")] {
            for far in [1.2, 2.5] {
                let mut all = [0u32; 4];
                for seed in 0..6 {
                    let o = burst_hits(skill, far, motion, 20., seed);
                    (0..4).for_each(|k| all[k] += o[k]);
                }
                let n: u32 = all.iter().sum();
                let pc = |k: usize| format!("{:.0}%", 100. * all[k] as f32 / n.max(1) as f32);
                println!("| {} | {what} | {far} m | {n} | {} | {} | {} | {} |", skill.name(), pc(0), pc(1), pc(2), pc(3));
            }
        }
    }
}

/// Free For All on Counter Siege with hotdog 1 a person standing idle: each life from the moment a bot whose prey it
/// is first has a clear line to it, to its death (seconds).
fn survival(seed: u64, seconds: f32, skill: Skill) -> (Vec<f32>, Vec<f32>) {
    let mut s = Feast::new(false);
    s.card_seed = seed;
    s.set_rules(Rules::parse(&format!("mode=ffa skill={}", skill.name())));
    let (mut seen, mut born, mut lives, mut whole) = (None, Some(s.elapsed), vec![], vec![]);
    for _ in 0..(seconds / DT) as usize {
        let me = s.player(1).unwrap().clone();
        if me.alive {
            born.get_or_insert(s.elapsed);
            if seen.is_none() {
                let eyes = s.players.iter().filter(|b| b.bot && b.alive).any(|b| {
                    let o = Vec3::from_array(b.p);
                    let prey = s.players.iter().filter(|t| t.id != b.id && !t.consumed && (t.alive || b.hp < 80.)).min_by(|x, y| Vec3::from_array(x.p).distance_squared(o).total_cmp(&Vec3::from_array(y.p).distance_squared(o)));
                    let m = muzzle(b);
                    let far = m.distance(chest(&me));
                    prey.is_some_and(|t| t.id == 1) && s.cover(m, (chest(&me) - m) / far) + 0.02 >= far
                });
                if eyes {
                    seen = Some(s.elapsed);
                }
            }
        }
        s.tick(DT);
        if me.alive && !s.player(1).unwrap().alive {
            if std::env::var("WHY").is_ok() {
                let d = s.player(1).unwrap().last_damage;
                let far = s.player(d.attacker).map_or(0., |k| Vec3::from_array(k.p).distance(Vec3::from_array(me.p)));
                let shooters: Vec<u32> = s.players.iter().filter(|b| b.bot && b.sight.target == 1 && s.elapsed - b.sight.last < 0.3).map(|b| b.id).collect();
                println!("died {:.1} s after seen: kind {} by {} at {far:.2} m; tracking it: {shooters:?}", seen.map_or(-1., |at| s.elapsed - at), d.kind, d.attacker);
            }
            if let Some(at) = seen.take() {
                lives.push(s.elapsed - at);
            }
            if let Some(at) = born.take() {
                whole.push(s.elapsed - at);
            }
        }
        if s.stage == Stage::RoundOver {
            break;
        }
    }
    (lives, whole)
}

#[test]
#[ignore]
fn measure_survival() {
    let skills: Vec<Skill> = match std::env::var("SKILLS") { Ok(list) => list.split(',').filter_map(Skill::named).collect(), Err(_) => vec![Skill::Normal] };
    for skill in skills {
        let (mut lives, mut whole) = (vec![], vec![]);
        for seed in 1..=12 {
            let (l, w) = survival(seed, 120., skill);
            lives.extend(l);
            whole.extend(w);
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        let mut sorted = lives.clone();
        sorted.sort_by(f32::total_cmp);
        println!("idle person in FFA with 4 bots, {}: {} deaths; from first seen to death mean {:.1} s, median {:.1} s; spawn to death mean {:.1} s", skill.name(), lives.len(), mean(&lives), sorted.get(sorted.len() / 2).copied().unwrap_or(0.), mean(&whole));
    }
}

/// A Free For All of five bots: seconds until the round ends (25 kills, or the five minutes), and the top score.
fn match_length(seed: u64, skill: Skill) -> (f32, u32) {
    let mut s = Feast::new(false);
    s.card_seed = seed;
    s.players[0].bot = true;
    s.set_rules(Rules::parse(&format!("mode=ffa skill={}", skill.name())));
    let start = s.elapsed;
    while s.stage != Stage::RoundOver && s.elapsed - start < 400. {
        s.tick(DT);
    }
    (s.elapsed - start, s.players.iter().map(|p| p.kills).max().unwrap_or(0))
}

#[test]
#[ignore]
fn measure_match_length() {
    let skills: Vec<Skill> = match std::env::var("SKILLS") { Ok(list) => list.split(',').filter_map(Skill::named).collect(), Err(_) => vec![Skill::Normal] };
    for skill in skills {
        let mut out = vec![];
        for seed in 1..=10 {
            out.push(match_length(seed, skill));
        }
        let mean = out.iter().map(|o| o.0).sum::<f32>() / out.len() as f32;
        let timed = out.iter().filter(|o| o.1 < 25).count();
        println!("5-bot FFA to 25 kills, {}: {:?}; mean {mean:.0} s, {timed} of 10 ended by the clock", skill.name(), out.iter().map(|o| o.0.round() as i32).collect::<Vec<_>>());
    }
}

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Forts with six bots: each round's fight, seconds from the field dropping to its end, and how it ended.
#[test]
#[ignore]
fn measure_forts() {
    for name in ["pillow-fort", "jungle-fort"] {
        let mut s = Feast::with_map(Map::parse(&read(&format!("assets/maps/{name}.json"))).unwrap(), true);
        let (mut rounds, mut was, mut fight_at) = (vec![], s.stage, 0.);
        while rounds.len() < 8 && s.elapsed < 3000. {
            s.tick(DT);
            if s.stage == Stage::Fight && was != Stage::Fight {
                fight_at = s.elapsed;
            }
            if s.stage == Stage::RoundOver && was != Stage::RoundOver {
                rounds.push((s.elapsed - fight_at, s.fort.as_ref().unwrap().reason.clone()));
            }
            was = s.stage;
        }
        let mean = rounds.iter().map(|r| r.0).sum::<f32>() / rounds.len().max(1) as f32;
        let captures = rounds.iter().filter(|r| r.1 == "capture").count();
        println!("Forts {name}: fights {:?}; mean {mean:.0} s, {captures} of {} by the bacon", rounds.iter().map(|r| r.0.round() as i32).collect::<Vec<_>>(), rounds.len());
    }
}

/// The idle person's survival (as `survival` measures it) in each gun mode, rounds following one another for
/// `seconds`: "ffa" Free For All on Counter Siege; "famine" the siege's free for all on Counter Siege, begun when its
/// warm-up ends (the wave is skipped: its cats would eat a person who stands still); "forts" a Forts fight on Pillow
/// Fort after a five-second build, the person on the mustard side. "First seen" is a living rival bot whose nearest
/// living rival the person is having a clear line to its chest.
fn survival_in(mode: &str, seed: u64, seconds: f32, skill: Skill) -> (Vec<f32>, Vec<f32>, u32) {
    let mut s = match mode {
        "forts" => Feast::with_map(Map::parse(&read("assets/maps/pillow-fort.json")).unwrap(), false),
        _ => Feast::new(false),
    };
    (s.card_seed, s.follow) = (seed, true);
    s.set_rules(Rules::parse(&format!("{} skill={}", if mode == "ffa" { "mode=ffa" } else { "" }, skill.name())));
    if mode == "forts" {
        s.set_build(5.);
    }
    let (mut seen, mut born, mut lives, mut whole, mut rounds) = (None, None, vec![], vec![], 0);
    for _ in 0..(seconds / DT) as usize {
        if mode == "famine" && s.stage == Stage::Warmup && s.elapsed + DT >= s.warmup {
            s.begin_famine();
        }
        let fighting = matches!(s.stage, Stage::FreeForAll | Stage::Fight);
        let me = s.player(1).unwrap().clone();
        if me.alive && fighting {
            born.get_or_insert(s.elapsed);
            if seen.is_none() {
                let eyes = s.players.iter().filter(|b| b.bot && b.alive && !s.allied(b.id, 1)).any(|b| {
                    let o = Vec3::from_array(b.p);
                    let prey = s.players.iter().filter(|t| t.id != b.id && t.alive && !s.allied(b.id, t.id)).min_by(|x, y| Vec3::from_array(x.p).distance_squared(o).total_cmp(&Vec3::from_array(y.p).distance_squared(o)));
                    let m = muzzle(b);
                    let far = m.distance(chest(&me));
                    prey.is_some_and(|t| t.id == 1) && s.cover(m, (chest(&me) - m) / far) + 0.02 >= far
                });
                if eyes {
                    seen = Some(s.elapsed);
                }
            }
        }
        let round = s.round;
        s.tick(DT);
        let died = me.alive && s.round == round && !s.player(1).is_some_and(|p| p.alive);
        if died {
            if let Some(at) = seen.take() {
                lives.push(s.elapsed - at);
            }
            if let Some(at) = born.take() {
                whole.push(s.elapsed - at);
            }
        }
        if s.round != round {
            rounds += 1;
        }
        if s.round != round || (s.stage == Stage::RoundOver && !died) {
            (seen, born) = (None, None);
        }
        if mode == "ffa" && s.stage == Stage::RoundOver {
            break;
        }
    }
    (lives, whole, rounds)
}

/// `survival_in` for every gun mode: `MODES=ffa,famine,forts` (the default) and `SKILLS` as the others take them.
#[test]
#[ignore]
fn measure_survival_by_mode() {
    let skills: Vec<Skill> = match std::env::var("SKILLS") { Ok(list) => list.split(',').filter_map(Skill::named).collect(), Err(_) => vec![Skill::Normal] };
    let modes: Vec<String> = std::env::var("MODES").map_or(vec!["ffa".into(), "famine".into(), "forts".into()], |list| list.split(',').map(str::to_string).collect());
    for skill in skills {
        for mode in &modes {
            let (mut lives, mut whole, mut rounds) = (vec![], vec![], 0);
            for seed in 1..=12 {
                let (l, w, r) = survival_in(mode, seed, if mode == "ffa" { 120. } else { 240. }, skill);
                lives.extend(l);
                whole.extend(w);
                rounds += r;
            }
            let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
            let mut sorted = lives.clone();
            sorted.sort_by(f32::total_cmp);
            println!("idle person, {mode}, {}: {rounds} rounds finished, {} deaths ({} seen first); from first seen to death mean {:.1} s, median {:.1} s; from the fight's start (or a respawn) to death mean {:.1} s", skill.name(), whole.len(), lives.len(), mean(&lives), sorted.get(sorted.len() / 2).copied().unwrap_or(0.), mean(&whole));
        }
    }
}

/// The siege, five bots (the demo): seconds to clear the wave, and the famine's free for all to its last hotdog.
#[test]
#[ignore]
fn measure_siege() {
    let mut out = vec![];
    for seed in 1..=4 {
        let mut s = Feast::new(true);
        s.card_seed = seed;
        let (mut wave_at, mut famine_at, mut over_at) = (None, None, None);
        while over_at.is_none() && s.elapsed < 600. {
            s.tick(DT);
            if s.stage == Stage::Wave && wave_at.is_none() {
                wave_at = Some(s.elapsed);
            }
            if s.stage == Stage::FreeForAll && famine_at.is_none() {
                famine_at = Some(s.elapsed);
            }
            if s.stage == Stage::RoundOver {
                over_at = Some(s.elapsed);
            }
        }
        let (w, f, o) = (wave_at.unwrap_or(0.), famine_at.unwrap_or(f32::NAN), over_at.unwrap_or(f32::NAN));
        out.push(((f - w).round(), (o - f).round(), s.players.iter().filter(|p| p.alive).count()));
    }
    println!("siege (wave cleared s, famine to the end s, alive at the end): {out:?}");
}

/// The way each round hotdog 2 fired in `seconds` of `hit_rate`'s range left its muzzle, in order.
fn shots(skill: Skill, gun: usize, motion: usize, seconds: f32, seed: u32) -> Vec<[f32; 3]> {
    let mut s = range();
    (s.rules.skill, s.card_seed) = (skill, seed as u64);
    put(&mut s, 2, 0.5, 2.);
    put(&mut s, 1, 1.7, 2.);
    {
        let p = s.player_mut(2).unwrap();
        (p.owned[gun], p.weapon, p.gun) = (true, gun, gun);
    }
    let (mut out, mut last) = (vec![], 0);
    for k in 0..(seconds / DT) as usize {
        s.player_mut(2).unwrap().ammo[gun] = FULL[gun];
        s.player_mut(1).unwrap().hp = 1e4;
        if motion > 0 {
            let side = if (k / 50) % 2 == 0 { 1. } else { -1. };
            s.walk(1, Vec2::new(0., side * actor::WALK as f32 * DT), 0.);
        }
        shoot(&mut s, gun);
        s.tick(DT);
        let since = last;
        for shot in s.shots.iter().filter(|shot| shot.owner == 2 && shot.id > since) {
            out.push((Vec3::from_array(shot.b) - Vec3::from_array(shot.a)).normalize().to_array());
            last = shot.id;
        }
    }
    out
}

#[test]
fn the_same_seed_aims_the_same_way_every_time_and_another_seed_does_not() {
    for (skill, gun) in [(Skill::Normal, RIFLE), (Skill::Easy, SNIPER), (Skill::Hard, 1), (Skill::Normal, MAYO)] {
        let a = shots(skill, gun, 1, 6., 7);
        assert!(a.len() >= 3, "{skill:?} {gun}: {}", a.len());
        assert_eq!(a, shots(skill, gun, 1, 6., 7), "{skill:?} {gun}");
        assert_ne!(a, shots(skill, gun, 1, 6., 8), "{skill:?} {gun}");
    }
    assert_eq!(hit_rate(Skill::Normal, RIFLE, 1.2, 1, 8., 3), hit_rate(Skill::Normal, RIFLE, 1.2, 1, 8., 3));
}

#[test]
fn a_normal_bot_hits_a_still_target_most_of_the_time_a_strafing_one_about_a_quarter_and_the_sniper_misses_a_mover_often() {
    // (The measured rates are 59%, 31% and 25%, against a target strafing at today's walk, 1.25 m/s: in the 55-70%, 25-35%
    // and 20-30% asked for, with room either side.)
    let (_, still) = rates(Skill::Normal, RIFLE, 1.2, 0, 6, 20.);
    let (_, strafing) = rates(Skill::Normal, RIFLE, 1.2, 1, 6, 20.);
    let (_, sniper) = rates(Skill::Normal, SNIPER, 1.2, 1, 6, 20.);
    assert!((0.48..=0.78).contains(&still), "the syrup rifle at a still target 1.2 m off: {still}");
    assert!((0.17..=0.40).contains(&strafing), "at one strafing at a walk: {strafing}");
    assert!((0.14..=0.38).contains(&sniper), "the ketchup sniper at one strafing: {sniper}");
    // The perfect aim of old hits a still target every time.
    assert!(rates(Skill::Perfect, RIFLE, 1.2, 0, 2, 10.).1 > 0.97);
}

#[test]
fn easy_is_about_six_tenths_of_normal_and_hard_about_one_and_a_half_times_short_of_perfect() {
    for (gun, motion) in [(RIFLE, 0), (RIFLE, 1), (1, 0)] {
        let [easy, normal, hard] = [Skill::Easy, Skill::Normal, Skill::Hard].map(|skill| rates(skill, gun, 1.2, motion, 6, 20.).1);
        assert!(easy < normal && normal < hard && hard < 0.95, "gun {gun} motion {motion}: {easy} {normal} {hard}");
        assert!((0.4..=0.8).contains(&(easy / normal)) && (1.1..=1.7).contains(&(hard / normal)), "gun {gun} motion {motion}: {easy} {normal} {hard}");
    }
}

/// Seconds from the first step a bot of `skill` aims at hotdog 1 to its first shot.
fn first_shot(skill: Skill, gun: usize, seed: u32) -> f32 {
    let mut s = range();
    (s.rules.skill, s.card_seed) = (skill, seed as u64);
    put(&mut s, 2, 0.5, 2.);
    put(&mut s, 1, 1.7, 2.);
    let p = s.player_mut(2).unwrap();
    (p.owned[gun], p.weapon, p.gun, p.ammo[gun]) = (true, gun, gun, FULL[gun]);
    let start = s.elapsed;
    for _ in 0..200 {
        shoot(&mut s, gun);
        if s.player(2).unwrap().ammo[gun] < FULL[gun] {
            return s.elapsed - start;
        }
        s.tick(DT);
    }
    f32::INFINITY
}

#[test]
fn a_bot_reacts_before_its_first_shot_at_a_target_it_has_just_seen_and_the_sniper_settles_longer() {
    let hand = Skill::Normal.hand();
    let times: Vec<f32> = (0..16).map(|seed| first_shot(Skill::Normal, RIFLE, seed)).collect();
    assert!(times.iter().all(|t| *t >= hand.react[0] - 1e-3 && *t <= hand.react[1] + 2. * DT), "{times:?}");
    let spread = times.iter().fold(0f32, |m, t| m.max(*t)) - times.iter().fold(9f32, |m, t| m.min(*t));
    assert!(spread > 0.1, "drawn, not fixed: {times:?}");
    let mean = |skill: Skill, gun: usize| (0..16).map(|seed| first_shot(skill, gun, seed)).sum::<f32>() / 16.;
    assert!(mean(Skill::Hard, RIFLE) < mean(Skill::Normal, RIFLE) && mean(Skill::Normal, RIFLE) < mean(Skill::Easy, RIFLE));
    assert!(mean(Skill::Normal, SNIPER) > mean(Skill::Normal, RIFLE) + SETTLE[SNIPER] * 0.9, "the scope settles");
    assert_eq!(first_shot(Skill::Perfect, RIFLE, 0), 0., "the old aim fires at once");
    // Lost from sight for longer than FORGET, or another target: it reacts afresh.
    let mut s = range();
    put(&mut s, 2, 0.5, 2.);
    put(&mut s, 1, 1.7, 2.);
    let t = s.player(1).unwrap().clone();
    s.draw_bead(2, 1, chest(&t), Vec3::ZERO, RIFLE, None);
    s.elapsed += 0.5;
    assert!(s.draw_bead(2, 1, chest(&t), Vec3::ZERO, RIFLE, None).1, "reacted");
    s.elapsed += crate::aim::FORGET + 0.1;
    assert!(!s.draw_bead(2, 1, chest(&t), Vec3::ZERO, RIFLE, None).1, "a target unseen for a while is seen afresh");
    s.elapsed += 0.7;
    assert!(s.draw_bead(2, 1, chest(&t), Vec3::ZERO, RIFLE, None).1);
    assert!(!s.draw_bead(2, 3, chest(&t), Vec3::ZERO, RIFLE, None).1, "and so is another");
}

#[test]
fn a_bot_that_keeps_on_one_target_walks_its_shots_in() {
    // The angle between each mustard round and the true line to a still target, by shot, over many seeds.
    let mut by_shot = vec![(0f32, 0u32); 12];
    for seed in 0..40 {
        let mut s = range();
        (s.rules.skill, s.card_seed) = (Skill::Normal, seed);
        put(&mut s, 2, 0.5, 2.);
        put(&mut s, 1, 1.7, 2.);
        let p = s.player_mut(2).unwrap();
        (p.owned[1], p.weapon, p.gun) = (true, 1, 1);
        let mut last = 0;
        for _ in 0..(12. / DT) as usize {
            s.player_mut(2).unwrap().ammo[1] = FULL[1];
            put(&mut s, 1, 1.7, 2.);
            shoot(&mut s, 1);
            let truth = (chest(s.player(1).unwrap()) - muzzle(s.player(2).unwrap())).normalize();
            let since = last;
            for shot in s.shots.iter().filter(|shot| shot.owner == 2 && shot.id > since) {
                let k = shot.id as usize - 1;
                let d = (Vec3::from_array(shot.b) - Vec3::from_array(shot.a)).normalize();
                if k < by_shot.len() {
                    by_shot[k] = (by_shot[k].0 + d.angle_between(truth), by_shot[k].1 + 1);
                }
                last = shot.id;
            }
            s.tick(DT);
        }
    }
    let mean = |range: std::ops::Range<usize>| by_shot[range.clone()].iter().map(|(a, _)| a).sum::<f32>() / by_shot[range].iter().map(|(_, n)| *n as f32).sum::<f32>();
    let (first, later) = (mean(0..2), mean(8..12));
    assert!(first > 1.4 * later, "the first shots are off by {first} rad, the ninth to twelfth by {later}");
}

#[test]
fn the_skill_is_a_rule_read_in_any_game_and_written_back_when_it_is_not_normal() {
    assert_eq!(Rules::default().skill, Skill::Normal);
    for (line, skill) in [("skill=easy", Skill::Easy), ("mode=ffa skill=hard", Skill::Hard), ("bots=easy", Skill::Easy), ("sandbox bots=hard", Skill::Hard), ("difficulty=perfect", Skill::Perfect), ("skill=normal", Skill::Normal), ("skill=nonsense", Skill::Normal)] {
        let r = Rules::parse(line);
        assert_eq!(r.skill, skill, "{line}");
        assert_eq!(Rules::parse(&r.text()), r, "{line}: {}", r.text());
    }
    assert_eq!(Rules::parse("mode=ffa skill=hard").text(), "mode=ffa skill=hard");
    assert_eq!(Rules::parse("skill=normal").text(), "", "normal goes unsaid");
    // The sandbox's bot count and modes are as they were.
    assert_eq!((Rules::parse("sandbox bots=2").bots, Rules::parse("sandbox bots=walk").botmode), (Some(2), crate::rules::BotMode::Walk));
    // A game keeps its skill from round to round.
    let mut s = Feast::new(false);
    s.set_rules(Rules::parse("mode=ffa skill=easy"));
    s.reset();
    assert_eq!(s.rules.skill, Skill::Easy);
}

#[test]
fn a_bot_jumps_a_relish_glob_coming_at_it_some_of_the_time_more_often_the_better_it_is_and_tucks_its_knees_up() {
    let jumps = |skill: Skill| {
        (0..60)
            .filter(|seed| {
                let mut s = range();
                (s.rules.skill, s.card_seed) = (skill, *seed);
                put(&mut s, 2, 0.5, 2.);
                put(&mut s, 1, 1.7, 2.);
                s.tick(DT);
                // Hotdog 1 fires a glob at the bot's feet.
                let p = s.player_mut(1).unwrap();
                (p.owned[4], p.ammo[4], p.weapon, p.gun) = (true, 2, 4, 4);
                let o = muzzle(s.player(1).unwrap());
                let feet = Vec3::from_array(s.player(2).unwrap().p) + Vec3::Y * 0.01;
                assert!(s.fire(1, (feet - o).normalize(), 4));
                s.dodge(2);
                // (Each glob is judged once; a perfect bot, which does not dodge, judges none.)
                assert_eq!(s.player(2).unwrap().sight.judged, if skill == Skill::Perfect { 0 } else { s.projectiles[0].id });
                s.player(2).unwrap().sight.tucked
            })
            .count()
    };
    let [easy, normal, hard] = [Skill::Easy, Skill::Normal, Skill::Hard].map(jumps);
    assert!(easy < normal && normal < hard && easy <= 15 && hard >= 27, "{easy} {normal} {hard} of 60");
    assert_eq!(jumps(Skill::Perfect), 0);
    // One that jumps is off the ground with its knees tucked, and stands again once down.
    let mut s = range();
    s.rules.skill = Skill::Hard;
    put(&mut s, 2, 0.5, 2.);
    put(&mut s, 1, 1.7, 2.);
    let p = s.player_mut(2).unwrap();
    (p.sight.tucked, p.crouch) = (true, true);
    s.jump(2, actor::JUMP as f32);
    s.tick(DT);
    let (mut tucked, mut landed) = (false, false);
    for _ in 0..60 {
        s.dodge(2);
        let p = s.player(2).unwrap();
        tucked |= crate::feast::tucked(p);
        landed |= p.grounded && !p.crouch && !p.sight.tucked;
        s.tick(DT);
    }
    assert!(tucked && landed);
}

#[test]
fn only_a_hard_bot_goes_for_the_face_with_the_sniper_and_only_some_of_the_time() {
    let faces = |skill: Skill| {
        let mut s = range();
        (s.rules.skill, s.card_seed) = (skill, 5);
        put(&mut s, 2, 0.5, 2.);
        put(&mut s, 1, 1.7, 2.);
        let t = s.player(1).unwrap().clone();
        let mut n = 0;
        for _ in 0..200 {
            s.draw_bead(2, 1, chest(&t), Vec3::ZERO, SNIPER, Some(face_point(&t)));
            n += s.player(2).unwrap().sight.face as usize;
            s.fired(2, SNIPER);
        }
        n
    };
    assert_eq!((faces(Skill::Easy), faces(Skill::Normal)), (0, 0));
    let hard = faces(Skill::Hard);
    assert!(hard > 40 && hard < 110, "{hard} of 200");
    // It sees the face only of a target facing it.
    let mut s = range();
    put(&mut s, 2, 0.5, 2.);
    put(&mut s, 1, 1.7, 2.);
    let (o, mut t) = (muzzle(s.player(2).unwrap()), s.player(1).unwrap().clone());
    t.yaw = std::f32::consts::FRAC_PI_2;
    assert!(s.mark(o, &t, SNIPER).1.is_some(), "facing the bot (-x)");
    t.yaw = -std::f32::consts::FRAC_PI_2;
    assert!(s.mark(o, &t, SNIPER).1.is_none(), "its back to the bot");
    // A relish glob is aimed at the feet.
    assert!((s.mark(o, &t, 4).0.y - t.p[1]).abs() < 0.02);
}

#[test]
fn the_guns_own_bloom_comes_on_top_of_a_bots_aim_and_is_not_taken_back() {
    let mut s = range();
    put(&mut s, 2, 0.5, 2.);
    put(&mut s, 1, 1.7, 2.);
    let p = s.player_mut(2).unwrap();
    (p.owned[1], p.weapon, p.gun, p.ammo[1]) = (true, 1, 1, FULL[1]);
    let t = s.player(1).unwrap().clone();
    s.draw_bead(2, 1, chest(&t), Vec3::ZERO, 1, None);
    s.elapsed += 1.;
    let (aim, ready) = s.draw_bead(2, 1, chest(&t), Vec3::ZERO, 1, None);
    assert!(ready);
    s.player_mut(2).unwrap().bloom = crate::recoil::MOST;
    assert!(s.loose(2, aim, ready, 1));
    let flew = Vec3::from_array(s.projectiles[0].v).normalize();
    assert!(flew.angle_between(aim) > 1e-3, "thrown off by the bloom besides: {}", flew.angle_between(aim));
}

#[test]
fn a_hard_bot_goes_for_the_face_with_the_pistol_as_with_the_sniper_and_the_rifle_is_aimed_at_the_body() {
    // With its error for the shot taken away, where a bot aims at a still target facing it: the face when it goes for it
    // (the sniper and the pistol), the chest otherwise.
    for (gun, face) in [(1, true), (SNIPER, true), (RIFLE, false)] {
        let mut s = range();
        s.rules.skill = Skill::Hard;
        put(&mut s, 2, 0.5, 2.);
        put(&mut s, 1, 1.7, 2.);
        s.player_mut(1).unwrap().yaw = std::f32::consts::FRAC_PI_2;
        let (o, t) = (muzzle(s.player(2).unwrap()), s.player(1).unwrap().clone());
        let (c, at) = s.mark(o, &t, gun);
        assert!(at.is_some(), "it faces the bot");
        s.draw_bead(2, 1, c, Vec3::ZERO, gun, at);
        for goes in [true, false] {
            let p = s.player_mut(2).unwrap();
            (p.sight.error, p.sight.face, p.sight.lead) = ([0., 0.], goes, 1.);
            let (d, _) = s.draw_bead(2, 1, c, Vec3::ZERO, gun, at);
            let want = if goes && face { face_point(&t) } else { c };
            assert!(d.angle_between((want - o).normalize()) < 1e-4, "gun {gun}, for the face {goes}");
        }
    }
}

#[test]
fn a_patient_bot_waits_for_its_pistols_muzzle_to_come_down_and_an_easy_one_fires_on_sooner() {
    let ready = |skill: Skill, bloom: f32| {
        let mut s = range();
        s.rules.skill = skill;
        put(&mut s, 2, 0.5, 2.);
        put(&mut s, 1, 1.7, 2.);
        let t = s.player(1).unwrap().clone();
        // (Seen for a second, in steps, so that it has reacted and has not forgotten it.)
        for _ in 0..4 {
            s.draw_bead(2, 1, chest(&t), Vec3::ZERO, 1, None);
            s.elapsed += 0.25;
        }
        s.player_mut(2).unwrap().bloom = bloom;
        s.draw_bead(2, 1, chest(&t), Vec3::ZERO, 1, None).1
    };
    let kick = crate::recoil::kick(1);
    for skill in [Skill::Normal, Skill::Hard] {
        assert!(!ready(skill, 0.5 * kick) && ready(skill, 0.1 * kick), "{skill:?} waits for the muzzle");
    }
    assert!(ready(Skill::Easy, 0.5 * kick), "easy does not");
    // And easy brings its aim back sooner with the pistol (it spams), though not with the rifle.
    let pause = |skill: Skill, gun: usize| {
        let mut s = range();
        s.rules.skill = skill;
        (0..100).map(|_| {
            s.fired(2, gun);
            s.player(2).unwrap().sight.hold - s.elapsed - DELAY[gun]
        }).sum::<f32>() / 100.
    };
    assert!(pause(Skill::Easy, 1) < 0.75 * pause(Skill::Normal, 1), "{} {}", pause(Skill::Easy, 1), pause(Skill::Normal, 1));
    assert!(pause(Skill::Easy, RIFLE) > pause(Skill::Normal, RIFLE));
}

#[test]
fn a_bots_burst_follows_its_target_round_by_round() {
    // The machine's aim (no error, no lag) at a hotdog walking across, its walk reported: each round of the burst leaves
    // along the aim as it is then, so all three land, and each leaves further along the walk than the last.
    let mut s = range();
    s.rules.skill = Skill::Perfect;
    put(&mut s, 2, 0.5, 2.);
    put(&mut s, 1, 1.7, 1.75);
    {
        let p = s.player_mut(2).unwrap();
        (p.owned[RIFLE], p.weapon, p.gun) = (true, RIFLE, RIFLE);
    }
    let (mut ways, mut hits, mut last) = (vec![], 0, 0);
    for _ in 0..30 {
        s.player_mut(1).unwrap().hp = 1e4;
        s.walk(1, Vec2::new(0., actor::WALK as f32 * DT), 0.);
        s.player_mut(1).unwrap().v = [0., 0., actor::WALK as f32];
        if ways.len() < 3 {
            shoot(&mut s, RIFLE);
        }
        s.tick(DT);
        hits += ((1e4 - s.player(1).unwrap().hp) / PLAYER_DAMAGE[RIFLE]).round() as u32;
        let since = last;
        for shot in s.shots.iter().filter(|shot| shot.owner == 2 && shot.id > since) {
            ways.push((Vec3::from_array(shot.b) - Vec3::from_array(shot.a)).normalize());
            last = shot.id;
        }
    }
    assert_eq!((ways.len(), hits), (3, 3), "{ways:?}");
    assert!(ways[1].z > ways[0].z + 0.05 && ways[2].z > ways[1].z + 0.05, "{ways:?}");
}

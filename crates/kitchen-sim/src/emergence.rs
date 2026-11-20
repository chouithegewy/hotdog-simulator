// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! How zombies arrive. The microwave finishes with a ding, its door swings open, a thick liquid runs
//! down its front and across the counter to the map's zombie spawn points, pools there, and each
//! puddle rises and shapes itself into zombies. A zombie that is still forming cannot be hurt and
//! does not move or attack.
//!
//! `Feast::emerge(n)` starts one such arrival of `n` zombies; the wave is one call, and any mode
//! may ask for more. The server runs these same rules (crates/server).
use crate::feast::{Feast, Zombie};
use crate::map::{Map, Side};
use glam::Vec3;
use serde::{Deserialize, Serialize};

// The timeline of one emergence, in seconds from its ding (FLOW is metres a second).
/// The door has swung fully open.
pub const DOOR: f32 = 0.7;
/// The liquid has run over the sill, down the front, and reached the counter.
pub const SPILL: f32 = 1.5;
/// Speed of the liquid across the counter.
pub const FLOW: f32 = 0.5;
/// A puddle collects for this long after the liquid reaches it before its first zombie rises.
pub const POOL: f32 = 1.5;
/// Rising out of the puddle and taking shape.
pub const FORM: f32 = 1.8;
/// The zombies of one emergence begin to form this far apart, when there are WAVE of them or fewer.
pub const STAGGER: f32 = 1.2;
/// However many zombies an emergence brings, they begin to form within this long of their puddles being
/// ready: the time the WAVE of the standard wave take (WAVE × STAGGER). More than WAVE share it evenly, so
/// a horde of 128 begins 0.056 s apart. A puddle still forms one zombie at a time.
pub const WINDOW: f32 = 7.2;
/// A puddle rests this long after one zombie has formed before it starts another.
pub const REFILL: f32 = 0.6;
/// Once the last zombie has formed the liquid drains away and the doors swing shut.
pub const DRAIN: f32 = 1.5;
/// Zombies in the wave: this many at least, and more on maps with many spawn points.
pub const WAVE: u32 = 6;
/// Zombies the wave brings for each of a map's spawn points.
pub const PER_SPAWN: u32 = 2;

/// A microwave's door: the block it belongs to, the middle of its facing side at counter level,
/// and the unit vector pointing out of it, both [x, z].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Door {
    pub block: usize,
    pub face: [f64; 2],
    pub out: [f64; 2],
}
/// Every microwave on the map, in block order.
pub fn doors(map: &Map) -> Vec<Door> {
    map.blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| block.style == "microwave")
        .map(|(index, block)| {
            let [x0, z0, x1, z1] = block.rect;
            let (face, out) = match block.facing.unwrap_or(Side::MinusZ) {
                Side::PlusX => ([x1, (z0 + z1) / 2.], [1., 0.]),
                Side::MinusX => ([x0, (z0 + z1) / 2.], [-1., 0.]),
                Side::PlusZ => ([(x0 + x1) / 2., z1], [0., 1.]),
                Side::MinusZ => ([(x0 + x1) / 2., z0], [0., -1.]),
            };
            Door { block: index, face, out }
        })
        .collect()
}

/// Where the liquid of an emergence collects: one of the map's zombie spawn points.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Puddle {
    /// Index of the spawn point in the map.
    pub spawn: usize,
    pub p: [f32; 3],
    /// Block index of the microwave that feeds it, or -1 when the liquid wells up out of the counter.
    pub source: i32,
    /// The liquid's way over the counter as [x, z] points: from the door to the puddle, around
    /// blocks. Just the puddle itself when there is no microwave.
    pub path: Vec<[f32; 2]>,
    pub length: f32,
    /// Seconds after the ding at which the liquid reaches the puddle, and at which its last
    /// zombie has formed.
    pub arrive: f32,
    pub until: f32,
    /// Zombies this puddle forms.
    pub count: u32,
}
/// One staged arrival of zombies. Times are seconds after `started`, the simulation time of the ding.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Emergence {
    pub id: u32,
    pub started: f32,
    /// Zombies asked for, and how many of them have begun to form.
    pub total: u32,
    pub begun: u32,
    /// Block indices of the microwaves whose doors are open.
    pub sources: Vec<usize>,
    pub puddles: Vec<Puddle>,
    /// For each zombie, earliest first: when it begins to form and the index of its puddle in `puddles`.
    pub schedule: Vec<(f32, usize)>,
    /// When the liquid has drained and the emergence is forgotten.
    pub ends: f32,
}

impl Feast {
    /// How many zombies the wave brings on this map: two for each spawn point, and never fewer than WAVE.
    pub fn wave(&self) -> u32 {
        WAVE.max(PER_SPAWN * self.level.map.spawns.len() as u32)
    }
    /// Starts an emergence of `zombies` zombies now: the ding. They share the map's spawn points in
    /// order, each fed by the nearest microwave. Returns the emergence's id.
    ///
    /// An emergence of fewer zombies than the map has spawn points takes points spread evenly through
    /// the map's list, and each such emergence starts one point further on than the last, so that no
    /// spawn point is left out for good.
    ///
    /// Zombie k begins to form k × STAGGER after its puddle has pooled, or k × WINDOW / zombies when
    /// there are more than WAVE of them, and never before its puddle has finished its last and rested.
    pub fn emerge(&mut self, zombies: u32) -> u32 {
        let id = self.next_emergence;
        self.next_emergence += 1;
        let doors = doors(&self.level.map);
        // Far enough out of the door that the navigation grid counts the point as open counter.
        let reach = self.level.map.clearance + self.level.map.cell * 0.75;
        let points = self.level.map.spawns.len();
        let used = (zombies as usize).min(points);
        let mut emergence = Emergence { id, started: self.elapsed, total: zombies, ..Default::default() };
        let mut arrive = vec![];
        for index in 0..used {
            let spawn = if used < points { (id as usize - 1 + index * points / used) % points } else { index };
            let [x, z] = self.level.map.spawns[spawn].p;
            let mouth = |door: &Door| [door.face[0] + door.out[0] * reach, door.face[1] + door.out[1] * reach];
            let nearest = doors
                .iter()
                .map(|door| (door, (mouth(door)[0] - x).hypot(mouth(door)[1] - z)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(door, _)| *door);
            let (source, path) = match nearest {
                Some(door) => {
                    // Level's route keeps clear of blocks; where there is none the liquid runs straight.
                    let mut path = vec![door.face, mouth(&door)];
                    let route = self.level.path(mouth(&door), [x, z]);
                    if route.is_empty() {
                        path.push([x, z]);
                    } else {
                        path.extend(route);
                    }
                    if !emergence.sources.contains(&door.block) {
                        emergence.sources.push(door.block);
                    }
                    (door.block as i32, path)
                }
                None => (-1, vec![[x, z]]),
            };
            let length: f64 = path.windows(2).map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1])).sum();
            arrive.push(if source < 0 { 0. } else { SPILL as f64 + length / FLOW as f64 });
            emergence.puddles.push(Puddle {
                spawn,
                p: self.level.spawn(spawn).to_array(),
                source,
                path: path.iter().map(|p| [p[0] as f32, p[1] as f32]).collect(),
                length: length as f32,
                arrive: arrive[index] as f32,
                until: 0.,
                count: 0,
            });
        }
        let mut busy = vec![f64::NEG_INFINITY; used];
        let mut ends = 0f64;
        let stagger = if zombies <= WAVE { STAGGER as f64 } else { WINDOW as f64 / zombies as f64 };
        for k in 0..zombies as usize {
            let puddle = k % used;
            let at = (arrive[puddle] + POOL as f64 + k as f64 * stagger).max(busy[puddle]);
            busy[puddle] = at + FORM as f64 + REFILL as f64;
            ends = ends.max(at + FORM as f64);
            emergence.puddles[puddle].until = (at + FORM as f64) as f32;
            emergence.puddles[puddle].count += 1;
            emergence.schedule.push((at as f32, puddle));
        }
        // A puddle far from its microwave is late: zombies rise in the order of their times.
        emergence.schedule.sort_by(|a, b| a.0.total_cmp(&b.0));
        emergence.ends = (ends + DRAIN as f64) as f32;
        self.emergences.push(emergence);
        id
    }
    /// Zombies that have been asked for but have not begun to form.
    pub fn pending(&self) -> u32 {
        self.emergences.iter().map(|e| e.total - e.begun).sum()
    }
    /// Advances every emergence by one step: forming zombies take shape, and those whose time has
    /// come begin to rise out of their puddles, facing the nearest hotdog.
    pub(crate) fn seep(&mut self, dt: f32) {
        let now = self.elapsed;
        for zombie in &mut self.enemies {
            zombie.forming = (zombie.forming - dt).max(0.);
        }
        let mut rising = vec![];
        for e in &mut self.emergences {
            while let Some((at, puddle)) = e.schedule.get(e.begun as usize) {
                if *at > now - e.started {
                    break;
                }
                rising.push(e.puddles[*puddle].p);
                e.begun += 1;
            }
        }
        self.emergences.retain(|e| now - e.started < e.ends);
        for p in rising {
            let at = Vec3::from_array(p);
            let yaw = self
                .players
                .iter()
                .filter(|player| player.alive)
                .min_by(|a, b| Vec3::from_array(a.p).distance_squared(at).total_cmp(&Vec3::from_array(b.p).distance_squared(at)))
                .map_or(0., |player| (at.x - player.p[0]).atan2(at.z - player.p[2]));
            self.wave_spawned += 1;
            self.enemies.push(Zombie { id: self.wave_spawned, p, yaw, hp: crate::feast::ZOMBIE_HP, born: now, forming: FORM });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feast::{Projectile, Stage, CAT, FLOOR, HOLD};
    use crate::map::Map;

    /// A counter 2 m by 1.2 m with the given spawns and blocks.
    fn counter(spawns: &str, blocks: &str) -> Map {
        Map::parse(&format!(
            r#"{{"format": "hotdog-map", "version": 1, "name": "Ooze", "bounds": [0, 0, 2, 1.2],
            "starts": [{{"p": [1, 0.1]}}], "spawns": [{spawns}], "blocks": [{blocks}]}}"#
        ))
        .unwrap()
    }
    /// A round that never reaches its wave, with nobody acting for the hotdogs: only the zombies
    /// asked for arrive.
    fn quiet(map: Map) -> Feast {
        let mut s = Feast::with_map(map, false);
        for p in &mut s.players {
            p.bot = false;
        }
        s.warmup = f32::MAX;
        s
    }
    fn run(s: &mut Feast, seconds: f32) {
        for _ in 0..(seconds * 30.).round() as u32 {
            s.tick(1. / 30.);
        }
    }
    const TICK: f32 = 1.5 / 30.;

    #[test]
    fn the_wave_begins_with_the_ding_and_nothing_can_be_hit_or_act_before_it_has_formed() {
        let mut s = Feast::new(false);
        for p in &mut s.players {
            p.bot = false;
        }
        run(&mut s, 11.9);
        assert!(s.stage == Stage::Warmup && s.emergences.is_empty() && s.enemies.is_empty());
        run(&mut s, 0.2);
        // The ding: the wave is on, one emergence of six is under way, and no zombie exists yet.
        assert_eq!(s.stage, Stage::Wave);
        assert_eq!((s.emergences.len(), s.emergences[0].total, s.pending(), s.enemies.len()), (1, WAVE, WAVE, 0));
        let e = s.emergences[0].clone();
        assert!((e.started - 12.).abs() < TICK);
        assert_eq!(e.sources, [0], "the bundled map's microwave is its first block");
        // The liquid has to spill, cross the counter and pool before anything rises.
        let first = e.schedule[0].0;
        assert!((first - (e.puddles[0].arrive + POOL)).abs() < 1e-5 && e.puddles[0].arrive > SPILL);
        run(&mut s, first - 0.3);
        assert!(s.enemies.is_empty() && s.wave_spawned == 0);
        run(&mut s, 0.4);
        assert_eq!((s.enemies.len(), s.wave_spawned, s.pending()), (1, 1, WAVE - 1));
        let at = s.enemies[0].p;
        assert_eq!(at, s.level.spawn(0).to_array());
        assert!(s.enemies[0].forming > FORM - 0.2 && s.enemies[0].hp == 70.);
        // A hotdog standing on the puddle stabs, shoots and bombs the forming zombie: nothing lands,
        // and the zombie neither bites nor moves.
        s.players[0].p = [at[0], FLOOR, at[2] - 0.05];
        s.players[0].owned[4] = true;
        s.players[0].ammo = [-1, 9, 0, 0, 9, 0, 0, 0];
        s.players[0].cooldown = 0.;
        assert!(s.melee(1, Vec3::Z));
        run(&mut s, 0.2);
        for weapon in [1, 4] {
            s.players[0].cooldown = 0.;
            assert!(s.fire(1, Vec3::Z, weapon));
            run(&mut s, 0.2);
        }
        s.burst(&Projectile { owner: 1, weapon: 4, ..Default::default() }, Vec3::from_array(at) + Vec3::Y * 0.075, None, None);
        let zombie = s.enemies[0].clone();
        assert!(zombie.forming > 0. && zombie.hp == 70. && zombie.p == at);
        assert_eq!((s.players[0].hp, s.players[0].kills, s.wave_kills), (100., 0, 0));
        // The rounds flew on through the ooze rather than stopping in it.
        assert!(s.splats.iter().all(|splat| splat.p[2] > at[2] + 0.07) || s.splats.is_empty());
        // Once formed it bites the hotdog beside it and can be killed.
        run(&mut s, FORM);
        let zombie = s.enemies.iter().find(|z| z.id == 1).unwrap().clone();
        assert_eq!(zombie.forming, 0.);
        assert!(s.players[0].hp < 100., "the formed zombie attacks");
        s.players[0].p = [zombie.p[0], FLOOR, zombie.p[2] - 0.1];
        for _ in 0..3 {
            s.players[0].cooldown = 0.;
            assert!(s.melee(1, Vec3::Z));
        }
        assert!(s.enemies.iter().all(|z| z.id != 1));
        assert_eq!((s.wave_kills, s.players[0].kills), (1, 1));
    }
    #[test]
    fn the_zombies_asked_for_form_at_the_spawn_points_in_turn() {
        for (asked, spawns) in [(5, [2, 2, 1]), (2, [1, 1, 0]), (9, [3, 3, 3])] {
            let mut s = quiet(Map::bundled());
            for p in &mut s.players {
                p.p = [1.5, FLOOR, 2.4];
            }
            let id = s.emerge(asked);
            let e = s.emergences[0].clone();
            assert_eq!((id, e.id, e.total, e.begun), (1, 1, asked, 0));
            assert_eq!(e.puddles.len(), (asked as usize).min(3));
            assert_eq!(e.puddles.iter().map(|p| p.count).collect::<Vec<_>>(), spawns[..e.puddles.len()]);
            // Six or fewer begin STAGGER apart and more share WINDOW between them, each once its puddle has
            // pooled; a puddle finishes one and rests before the next.
            let stagger = if asked <= WAVE { STAGGER } else { WINDOW / asked as f32 };
            assert!((stagger - [STAGGER, STAGGER, 0.8][[5, 2, 9].iter().position(|n| *n == asked).unwrap()]).abs() < 1e-6);
            for (k, (at, puddle)) in e.schedule.iter().enumerate() {
                assert_eq!(*puddle, k % 3);
                let due = e.puddles[*puddle].arrive + POOL + k as f32 * stagger;
                let free = if k >= 3 { e.schedule[k - 3].0 + FORM + REFILL } else { f32::MIN };
                assert!((*at - due.max(free)).abs() < 1e-4, "zombie {k} of {asked} begins at {at}");
            }
            // Park the zombies: a formed one walks off towards the hotdogs, so note where each appears.
            let mut seen = vec![];
            for _ in 0..(e.ends * 30.) as u32 + 2 {
                s.tick(1. / 30.);
                for z in &s.enemies {
                    if !seen.iter().any(|(id, _)| *id == z.id) {
                        seen.push((z.id, z.p));
                    }
                }
            }
            assert_eq!(seen.len(), asked as usize);
            for (k, (id, p)) in seen.iter().enumerate() {
                assert_eq!((*id, *p), (k as u32 + 1, s.level.spawn(k).to_array()), "zombie {k} of {asked}");
            }
            assert_eq!((s.wave_spawned, s.pending()), (asked, 0));
            assert!(s.enemies.iter().all(|z| z.forming == 0.));
            assert!(s.emergences.is_empty(), "the liquid has drained");
            assert_eq!(s.emerge(1), 2);
        }
    }
    /// tools/network/fixtures/horde.json has 64 spawn points on a 1/16 m lattice; its first `points` make smaller maps.
    fn horde_map(points: usize) -> Map {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/network/fixtures/horde.json")).unwrap();
        let mut map: serde_json::Value = serde_json::from_str(&text).unwrap();
        map["spawns"].as_array_mut().unwrap().truncate(points);
        Map::from_value(&map).unwrap()
    }
    #[test]
    fn however_many_zombies_arrive_they_begin_within_one_window_six_as_before_24_and_128_as_a_horde_at_every_spawn_point() {
        for (points, asked) in [(3usize, 6u32), (12, 24), (64, 128)] {
            let mut s = quiet(horde_map(points));
            assert_eq!((s.wave(), WAVE.max(PER_SPAWN * points as u32)), (asked, asked));
            s.emerge(asked);
            let e = s.emergences[0].clone();
            let stagger = if asked <= WAVE { STAGGER } else { WINDOW / asked as f32 };
            assert!(if asked > WAVE { stagger < STAGGER / 3. } else { stagger == STAGGER });
            // Every spawn point is served, in the map's order, and forms its share.
            for (i, puddle) in e.puddles.iter().enumerate() {
                assert_eq!((puddle.spawn, puddle.count, puddle.p), (i, 2, s.level.spawn(i).to_array()));
            }
            assert_eq!(e.puddles.len(), points);
            // The rule: zombie k is due k staggers after its puddle has pooled, and waits for its puddle to
            // finish the one before and rest. For six that is the old timetable, STAGGER apart.
            let mut plan = vec![];
            let mut busy = vec![f32::MIN; points];
            for k in 0..asked as usize {
                let i = k % points;
                let at = (e.puddles[i].arrive + POOL + k as f32 * stagger).max(busy[i]);
                busy[i] = at + FORM + REFILL;
                if asked == WAVE {
                    assert_eq!(at, e.puddles[i].arrive + POOL + k as f32 * STAGGER);
                }
                plan.push((at, i));
            }
            plan.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert_eq!(e.schedule.len(), plan.len());
            for (k, (got, want)) in e.schedule.iter().zip(&plan).enumerate() {
                assert!(got.1 == want.1 && (got.0 - want.0).abs() < 1e-4, "zombie {k} of {asked}: {got:?} against {want:?}");
            }
            // The window: each begins no earlier than its puddle has pooled and less than WINDOW after, so
            // the whole arrival has begun WINDOW after the last puddle is ready however many there are; and
            // they rise in the order of their times.
            for (at, i) in &e.schedule {
                assert!(*at >= e.puddles[*i].arrive + POOL - 1e-5 && *at < e.puddles[*i].arrive + POOL + WINDOW);
            }
            let last = e.puddles.iter().map(|q| q.arrive).fold(0f32, f32::max);
            assert!(e.schedule.windows(2).all(|pair| pair[1].0 >= pair[0].0));
            assert!(e.schedule[asked as usize - 1].0 < last + POOL + WINDOW);
            assert!((e.ends - (e.schedule[asked as usize - 1].0 + FORM + DRAIN)).abs() < 1e-4);
            // Run it. A zombie that is still forming has no body, stays on its puddle and cannot be hurt: a
            // glob is burst on each as it starts to rise and again just before it has formed. (The formed
            // are made tough, so that the count at the end is of what arrived.)
            let mut seen: Vec<(u32, [f32; 3])> = vec![];
            let mut bombed = vec![0u32; asked as usize];
            let mut most = 0;
            for _ in 0..(e.ends * 30.) as u32 + 2 {
                s.tick(1. / 30.);
                for z in s.enemies.clone() {
                    if !seen.iter().any(|(id, _)| *id == z.id) {
                        seen.push((z.id, z.p));
                    }
                    if z.forming <= 0. {
                        s.enemies.iter_mut().find(|e| e.id == z.id).unwrap().hp = 1e9;
                        continue;
                    }
                    let times = &mut bombed[z.id as usize - 1];
                    if *times == 0 || (*times == 1 && z.forming < 0.1) {
                        *times += 1;
                        s.burst(&Projectile { owner: 1, weapon: 4, ..Default::default() }, Vec3::from_array(z.p) + Vec3::Y * 0.075, None, None);
                    }
                    let now = s.enemies.iter().find(|e| e.id == z.id).expect("a forming zombie is not killed");
                    assert!(now.hp == 70. && s.body(CAT + z.id).is_none(), "zombie {} was hit or has a body while forming", z.id);
                    assert_eq!(now.p, seen.iter().find(|(id, _)| *id == z.id).unwrap().1);
                }
                most = most.max(s.enemies.iter().filter(|z| z.forming <= 0.).count());
            }
            // Each appeared on its puddle in the order of the schedule, every one was bombed twice, and all
            // of them are on their feet together at the end.
            let expected: Vec<(u32, [f32; 3])> = e.schedule.iter().enumerate().map(|(k, (_, i))| (k as u32 + 1, e.puddles[*i].p)).collect();
            assert_eq!(seen, expected);
            assert!(bombed.iter().all(|times| *times == 2));
            assert_eq!((most, s.enemies.len(), s.wave_spawned, s.pending(), s.emergences.len(), s.wave_kills), (asked as usize, asked as usize, asked, 0, 0, 0));
            assert!(s.enemies.iter().all(|z| z.forming == 0. && s.body(CAT + z.id).is_some()));
        }
    }
    #[test]
    fn an_arrival_smaller_than_the_map_has_spawn_points_is_spread_over_them_and_the_next_starts_one_point_on() {
        let mut s = quiet(horde_map(64));
        let mut points = |n: u32| {
            s.emerge(n);
            s.emergences.last().unwrap().puddles.iter().map(|q| q.spawn).collect::<Vec<_>>()
        };
        assert_eq!(points(6), [0, 10, 21, 32, 42, 53]);
        assert_eq!(points(6), [1, 11, 22, 33, 43, 54]);
        assert_eq!(points(1), [2]);
        assert_eq!(points(64), (0..64).collect::<Vec<_>>());
        assert_eq!(points(200).len(), 64);
        // Sixty-four arrivals of one visit every spawn point; on the bundled map, with as many zombies as
        // points or more, nothing changes.
        let mut t = quiet(horde_map(64));
        let mut visited = std::collections::HashSet::new();
        for _ in 0..64 {
            t.emerge(1);
            visited.insert(t.emergences.last().unwrap().puddles[0].spawn);
        }
        assert_eq!(visited.len(), 64);
        let mut u = quiet(Map::bundled());
        u.emerge(2);
        u.emerge(5);
        assert_eq!(u.emergences.iter().map(|e| e.puddles.iter().map(|q| q.spawn).collect::<Vec<_>>()).collect::<Vec<_>>(), [vec![0, 1], vec![0, 1, 2]]);
    }
    #[test]
    fn a_wave_that_leaves_fewer_than_two_hotdogs_alive_ends_the_round_the_last_one_standing_wins_or_nobody_does() {
        let wave = || {
            let mut s = quiet(horde_map(64));
            for p in &mut s.players {
                p.p = [2.9, FLOOR, 0.1 + p.id as f32 * 0.1];
            }
            s.warmup = 0.;
            s.tick(1. / 30.);
            assert_eq!((s.stage, s.pending()), (Stage::Wave, 128));
            s
        };
        let fell = |s: &mut Feast, ids: &[u32]| {
            for id in ids {
                s.hurt(*id, 1e9);
            }
        };
        // Two left: the wave goes on. One left: the round is over and it has won, with the wave still arriving.
        let mut s = wave();
        fell(&mut s, &[1, 2, 5]);
        s.tick(1. / 30.);
        assert_eq!(s.stage, Stage::Wave);
        fell(&mut s, &[4]);
        s.tick(1. / 30.);
        assert_eq!((s.stage, s.winner, s.famine_at), (Stage::RoundOver, Some(3), 0.));
        assert!(s.alert.contains("MUSTARD IS THE LAST HOTDOG STANDING"), "{}", s.alert);
        // Nothing more arrives or bites.
        run(&mut s, 6.);
        assert_eq!((s.stage, s.wave_spawned, s.enemies.len(), s.players[2].hp), (Stage::RoundOver, 0, 0, 100.));
        // Nobody left: no winner.
        let mut s = wave();
        fell(&mut s, &[1, 2, 3, 4, 5]);
        s.tick(1. / 30.);
        assert_eq!((s.stage, s.winner), (Stage::RoundOver, None));
        assert!(s.alert.contains("NO SURVIVORS"), "{}", s.alert);
        // And the horde itself does it: zombies that have formed round hotdogs nobody steers bite them to
        // death, and the last one alive is the winner.
        let mut s = wave();
        for _ in 0..30 * 120 {
            if s.stage != Stage::Wave {
                break;
            }
            s.tick(1. / 30.);
        }
        let alive: Vec<u32> = s.players.iter().filter(|p| p.alive).map(|p| p.id).collect();
        assert_eq!(s.stage, Stage::RoundOver);
        assert!(alive.len() <= 1 && s.winner == alive.first().copied() && s.famine_at == 0. && s.enemies.len() > 100, "{} alive, {} zombies", alive.len(), s.enemies.len());
    }
    #[test]
    fn the_demos_hotdogs_clear_a_wave_of_24_from_twelve_spawn_points_and_go_on_to_the_free_for_all() {
        let mut s = Feast::with_map(horde_map(12), true);
        let mut most = 0;
        for _ in 0..30 * 120 {
            if matches!(s.stage, Stage::FreeForAll | Stage::RoundOver) {
                break;
            }
            s.tick(1. / 30.);
            most = most.max(s.enemies.iter().filter(|z| z.forming <= 0.).count());
        }
        assert_eq!((s.stage, s.wave_spawned, s.wave_kills), (Stage::FreeForAll, 24, 24));
        // (Three or more: since the syrup rifle's bursts came quicker, the hotdogs cut them down sooner.)
        assert!(most >= 3, "at most {most} zombies were on their feet together");
    }
    /// A zombie whose hotdog is out of sight behind a block searches the counter for its way once, then
    /// keeps to the waypoint it found for HOLD more steps (while it can see it, has not reached it, and
    /// its goal has not moved); one with its hotdog in sight keeps nothing.
    #[test]
    fn a_zombie_going_round_a_block_searches_for_its_way_once_in_ten_steps() {
        let mut s = quiet(Map::bundled());
        (s.stage, s.wave_spawned) = (Stage::Wave, WAVE);
        for p in &mut s.players {
            p.p = [-1.6 + p.id as f32 * 0.1, FLOOR, 3.55];
            p.hp = 1e9;
        }
        // The steel block between them is 0.22 m square; the zombie is south of it and the hotdog north.
        s.players[0].p = [-0.34, FLOOR, 3.2];
        s.enemies.push(Zombie { id: 1, p: [-0.34, FLOOR, 2.45], yaw: 0., hp: 70., born: 0., forming: 0. });
        assert!(!s.level.line([-0.34, 2.45], [-0.34, 3.1]));
        // (As far as a zombie went in two seconds at the 0.10 m/s it once walked: zombies walk FASTER now.)
        let (mut searches, mut held, mut waypoints) = (0, vec![], vec![]);
        let steps = (60. / crate::actor::FASTER).round() as usize;
        for _ in 0..steps {
            s.tick(1. / 30.);
            let (way, _, left) = s.routes[&(CAT + 1)];
            searches += (left == HOLD) as u32;
            held.push(left);
            if waypoints.last() != Some(&way) {
                waypoints.push(way);
            }
        }
        assert_eq!(held[..12], [9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 9, 8]);
        assert!(searches as usize == steps.div_ceil(10) && waypoints.len() <= 2, "{searches} searches, waypoints {waypoints:?}");
        // The waypoint is a corner of the block's clearance, not the place round the hotdog.
        assert!((waypoints[0][0] + 0.34).abs() > 0.14 && waypoints[0][1] < 3., "{waypoints:?}");
        // It gets there: round the block and into reach of the hotdog, where it has nothing to keep.
        run(&mut s, 12.);
        let (z, p) = (s.enemies[0].p, s.players[0].p);
        assert!((z[0] - p[0]).hypot(z[2] - p[2]) < crate::feast::BITE && !s.routes.contains_key(&(CAT + 1)), "zombie at {z:?}");
    }
    /// A step with a whole horde on its feet, the bots fighting it, must leave nearly all of a frame for
    /// drawing: well under 2 ms in a release build. And as much when the hotdogs are walled off, so that no
    /// zombie has a route to any of them: without the grid's regions each would search the whole counter for
    /// one, every step. (A debug build is many times slower and is only held to a figure that would catch a
    /// step that had become quadratic in something large. The quickest quarter of the steps is what is
    /// judged, so that a busy machine does not fail it.)
    #[test]
    fn a_step_with_128_zombies_on_their_feet_is_well_under_2_ms_whether_or_not_they_can_reach_the_hotdogs() {
        let mut pen = horde_map(64);
        pen.starts.truncate(1);
        pen.starts[0].p = [2.8, 0.15];
        pen.spawns.retain(|spawn| !(spawn.p[0] > 2.5 && spawn.p[1] < 0.45));
        for rect in [[2.6, 0., 2.65, 0.35], [2.6, 0.3, 3., 0.35]] {
            let mut wall = pen.blocks[1].clone();
            wall.rect = rect;
            pen.blocks.push(wall);
        }
        for (map, reach) in [(horde_map(64), true), (pen, false)] {
            let mut s = Feast::with_map(map, true);
            let wave = s.wave() as usize;
            assert!(wave >= 120);
            let mut times = vec![];
            for _ in 0..30 * 60 {
                if times.len() == 240 {
                    break;
                }
                for p in &mut s.players {
                    p.hp = 1e9;
                }
                for z in &mut s.enemies {
                    z.hp = 1e9;
                }
                let full = s.enemies.len() == wave && s.enemies.iter().all(|z| z.forming <= 0.);
                let from = std::time::Instant::now();
                s.tick(1. / 30.);
                if full {
                    times.push(from.elapsed().as_secs_f64() * 1e3);
                }
            }
            assert_eq!(times.len(), 240);
            // (Some hotdog can be got to, or none: one of them pressed against a block stands where no route ends.)
            let z = s.enemies[0].p;
            assert_eq!(s.players.iter().any(|q| s.level.joined([z[0] as f64, z[2] as f64], [q.p[0] as f64, q.p[2] as f64])), reach);
            times.sort_by(f64::total_cmp);
            let (quick, median) = (times[60], times[120]);
            println!("a step with {wave} zombies{}: quickest quarter {quick:.3} ms, median {median:.3} ms", if reach { "" } else { " walled off" });
            let budget = if cfg!(debug_assertions) { 40. } else { 1.5 } * crate::timing_slack();
            assert!(quick < budget, "quickest quarter {quick:.3} ms, median {median:.3} ms");
        }
    }
    #[test]
    fn with_two_microwaves_each_spawn_point_is_fed_by_the_nearest_around_the_blocks() {
        // Microwaves at either end facing each other across the counter, and a wall between the left
        // one and the spawn point it is nearest to.
        let map = counter(
            r#"{"p": [0.6, 0.9]}, {"p": [1.7, 0.6]}, {"p": [1.0, 0.6]}"#,
            r#"{"rect": [0.3, 0.5, 0.9, 0.6], "height": 0.2},
            {"rect": [0, 0.2, 0.2, 0.6], "style": "microwave", "facing": "+x"},
            {"rect": [1.8, 0.4, 2, 0.8], "style": "microwave", "facing": "-x"}"#,
        );
        let mut s = quiet(map);
        s.emerge(4);
        let e = s.emergences[0].clone();
        assert_eq!(e.sources, [1, 2]);
        assert_eq!(e.puddles.iter().map(|p| p.source).collect::<Vec<_>>(), [1, 2, 2]);
        assert_eq!(e.puddles.iter().map(|p| p.count).collect::<Vec<_>>(), [2, 1, 1]);
        // Each flow leaves its door at the middle of the facing side and ends at the spawn point.
        assert_eq!((e.puddles[0].path[0], e.puddles[1].path[0]), ([0.2, 0.4], [1.8, 0.6]));
        for puddle in &e.puddles {
            assert_eq!(*puddle.path.last().unwrap(), [puddle.p[0], puddle.p[2]]);
            let length: f32 = puddle.path.windows(2).map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1])).sum();
            assert!((length - puddle.length).abs() < 1e-5);
            assert!((puddle.arrive - (SPILL + length / FLOW)).abs() < 1e-5);
            // Past the door's mouth the liquid never runs through a block.
            for step in puddle.path[1..].windows(2) {
                assert!(s.level.line(step[0].map(f64::from), step[1].map(f64::from)), "{:?}", puddle.path);
            }
        }
        // The first puddle is behind the wall: its flow bends round it and is longer than the crow flies.
        let direct = (0.6f32 - 0.2).hypot(0.9 - 0.4);
        assert!(e.puddles[0].path.len() > 3 && e.puddles[0].length > direct + 0.05);
        assert_eq!(e.puddles[1].path.len(), 3);
        // The further the puddle, the later its zombie rises.
        assert!(e.puddles[0].arrive > e.puddles[1].arrive + 0.5);
        // A late puddle does not hold the others up: zombies rise in the order of their times,
        // whichever spawn point is theirs.
        assert_eq!(e.schedule.iter().map(|(_, puddle)| *puddle).collect::<Vec<_>>(), [1, 0, 2, 0]);
        assert!(e.schedule.windows(2).all(|s| s[1].0 >= s[0].0));
        // Bodies are solid: the two that rose from the first puddle have pushed each other apart, half a
        // body each way.
        run(&mut s, e.ends + 0.1);
        assert_eq!(s.enemies.len(), 4);
        for (k, z) in s.enemies.iter().enumerate() {
            let at = s.level.spawn([1, 0, 2, 0][k]);
            let off = (z.p[0] - at.x).hypot(z.p[2] - at.z);
            assert!((off - if k == 1 || k == 3 { 0.03 } else { 0. }).abs() < 1e-6 && z.p[1] == at.y, "zombie {} at {:?}", z.id, z.p);
        }
        assert!(((s.enemies[1].p[0] - s.enemies[3].p[0]).hypot(s.enemies[1].p[2] - s.enemies[3].p[2]) - 0.06).abs() < 1e-6);
    }
    #[test]
    fn without_a_microwave_the_liquid_wells_up_at_the_spawn_points() {
        let mut s = quiet(counter(r#"{"p": [0.3, 1]}, {"p": [1.7, 1]}"#, r#"{"rect": [0.9, 0.5, 1.1, 0.7], "style": "toaster"}"#));
        s.emerge(3);
        let e = s.emergences[0].clone();
        assert!(e.sources.is_empty());
        for (i, puddle) in e.puddles.iter().enumerate() {
            assert_eq!((puddle.source, puddle.arrive, puddle.length), (-1, 0., 0.));
            assert_eq!(puddle.path, [[puddle.p[0], puddle.p[2]]]);
            assert_eq!(puddle.p, s.level.spawn(i).to_array());
        }
        assert_eq!(e.schedule, [(POOL, 0), (POOL + STAGGER, 1), (POOL + 2. * STAGGER, 0)]);
        run(&mut s, POOL + 2. * STAGGER + FORM + 0.2);
        assert_eq!(s.enemies.len(), 3);
        assert!(s.enemies.iter().all(|z| z.forming == 0.));
    }
    #[test]
    fn doors_are_the_middle_of_each_microwaves_facing_side() {
        let map = counter(
            r#"{"p": [1, 1]}"#,
            r#"{"rect": [0, 0, 0.4, 0.2], "style": "microwave"}, {"rect": [0.5, 0.5, 0.7, 0.9], "style": "microwave", "facing": "+z"},
            {"rect": [1, 0.2, 1.2, 0.4]}, {"rect": [1.6, 0.2, 2, 0.6], "style": "microwave", "facing": "-x"}"#,
        );
        assert_eq!(
            doors(&map),
            [
                Door { block: 0, face: [0.2, 0.], out: [0., -1.] },
                Door { block: 1, face: [0.6, 0.9], out: [0., 1.] },
                Door { block: 3, face: [1.6, 0.4], out: [-1., 0.] },
            ]
        );
        assert!(doors(&counter(r#"{"p": [1, 1]}"#, "")).is_empty());
    }
}

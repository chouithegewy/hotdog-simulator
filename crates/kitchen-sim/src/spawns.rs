// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Where the hotdogs appear in Free For All (rules.rs), and in the sandbox's `mode=ffa`.
//!
//! **The spots.** The first time a map is played as Free For All its own level is surveyed (`spots`), and the
//! Feast keeps what it finds for every round after on that map (`Feast::spots`). The survey takes the points of a
//! grid GRID apart across the counter, and the map's starts, and keeps those a hotdog can walk to from a start by
//! the navigation grid's clear cells, up no ledge taller than a step (`Level::can_step`): the counter, and the
//! decks its ramps lead up to. A point is kept only where a hotdog's whole body, its radius and CLEAR more, stands
//! clear of every block's wall on level ground (no ramp, no sink, no deck's edge under any of it), EDGE or more
//! from the counter's edge. A hotdog put on a spot faces the most open way at its eye, leaning towards the middle
//! of the map (`facing`, worked out as it is put there).
//!
//! **A round's opening.** The hotdogs are spread over the free spots (see below) by farthest-point sampling: one
//! spot drawn at random, then each next the spot farthest from those already taken, so that they stand as far apart
//! as the map allows and rounds open differently. Which hotdog gets which spot is drawn too. A hotdog fitted out for
//! the round (`Feast::fit`, as the server does after setting a round out) goes to its own.
//!
//! **A respawn.** Every spot is scored against the living rivals: its distance from the nearest of them, up to
//! ROOM; less DIED if it is within NEAR of where the hotdog died, and AGAIN if within NEAR of where it last came
//! back. A spot is safe when no rival stands within CLOSE of it and none can see a hotdog's eye standing there
//! (as a bot's shot would see it: `Feast::cover`, blocks, sinks and solid loose objects, a pillow hiding nobody), and it is free: nothing loose
//! lies in the way of a body there now, and no draped napkin walls it. The hotdog comes back at one of the best
//! CHOICE safe spots, drawn at random (spots that score alike are taken in a random order). With nowhere safe it
//! comes back by the rule it had before there were spots, furthest from the living: at the free spot furthest from
//! them.
//!
//! **The dice.** Every draw is the game's seed (`Feast::card_seed`: the server sets it for each game, and single
//! player whenever it sets a game out), the round's number and what is drawn for, mixed: the same seed gives the same
//! spawns, whatever was drawn before. Online the server is the authority: a frame says where every hotdog is, a
//! client's own included, and no client predicts a respawn.
use crate::actor;
use crate::feast::{EYE, Feast, HOTDOG_RADIUS, Player, muzzle};
use crate::map::Level;
use glam::{Vec2, Vec3};

/// The survey's grid, in metres.
pub const GRID: f64 = 0.13;
/// Room round a hotdog's body beyond its radius, to a block's wall (which keeps the map's own MARGIN besides).
pub const CLEAR: f64 = 0.02;
/// How far a spot keeps from the counter's edge.
pub const EDGE: f64 = 0.1;
/// A respawn: a rival this close (across the counter) makes a spot unsafe: as far as a bot walks in the spawn shield
/// (rules.rs SHIELD, 1 s, at actor.rs BOT_WALK) and the reach of a bite more, so that one cannot be on the hotdog before its
/// shield is down. (It was 0.6 m when the hotdogs walked at less than half the pace.)
pub const CLOSE: f32 = 0.9;
/// A respawn: the distance from the nearest rival counts up to this, and no further.
pub const ROOM: f32 = 1.5;
/// A respawn: within this of where the hotdog died, or of where it last came back, a spot is marked down.
pub const NEAR: f32 = 0.5;
/// What a spot is marked down by near where the hotdog died, and near where it last came back.
pub const DIED: f32 = 1.;
pub const AGAIN: f32 = 0.75;
/// A respawn is drawn from this many of the best safe spots.
pub const CHOICE: usize = 3;
/// How far a spot's facing looks for open counter, and how much the middle of the map counts beside it.
const LOOK: f64 = 1.5;
const MIDDLE: f64 = 0.25;
/// What the dice are thrown for.
const OPENING: u64 = 1;
const RESPAWN: u64 = 2;
/// (Forts: the time a power weapon's pickup takes to come back, `Feast::comeback`.)
pub(crate) const PICKUP: u64 = 3;

/// A place a hotdog may appear: its feet, on the ground there, and the way it faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub p: Vec3,
    pub yaw: f32,
}
impl Spot {
    fn flat(self) -> Vec2 {
        Vec2::new(self.p.x, self.p.z)
    }
}
/// Distance across the counter, heights aside.
fn across(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// Free For All's dice: splitmix64 (Steele, Lea and Flood), its state mixed with each word it is thrown for.
pub(crate) struct Dice(u64);
impl Dice {
    pub(crate) fn new(words: &[u64]) -> Dice {
        let mut state = 0x5eed_f00d_u64;
        for &word in words {
            state = Dice(state ^ word).roll();
        }
        Dice(state)
    }
    pub(crate) fn roll(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// A number from 0 up to but not including `n` (0 when `n` is 0).
    pub(crate) fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.roll() % n as u64) as usize }
    }
}

/// The level's cells a hotdog can walk to from the map's starts: clear cells of the navigation grid, joined by
/// steps to the four cells round each that climb no more than a step (`Level::can_step`, taken in pieces of 2 cm
/// at most, so that a ramp is walked up as a body walks it).
fn reachable(level: &Level) -> Vec<bool> {
    let (cols, rows) = (level.cols, level.rows);
    let mut seen = vec![false; cols * rows];
    let mut open = vec![];
    for start in &level.map.starts {
        let id = level.cell_index(start.p[0], start.p[1]);
        if level.blocked[id] == 0 && !seen[id] {
            seen[id] = true;
            open.push(id);
        }
    }
    let pieces = (level.cell / 0.02).ceil().max(1.) as usize;
    let on = |x: f64, z: f64| [x, level.ground(x, z), z];
    while let Some(id) = open.pop() {
        let (i, j) = ((id % cols) as i64, (id / cols) as i64);
        let a = level.cell_center(id);
        for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (ni, nj) = (i + di, j + dj);
            if ni < 0 || nj < 0 || ni >= cols as i64 || nj >= rows as i64 {
                continue;
            }
            let next = nj as usize * cols + ni as usize;
            if seen[next] || level.blocked[next] != 0 {
                continue;
            }
            let b = level.cell_center(next);
            let walked = (0..pieces).all(|k| {
                let (s, t) = (k as f64 / pieces as f64, (k + 1) as f64 / pieces as f64);
                let from = on(a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s);
                level.can_step(from, on(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t))
            });
            if walked {
                seen[next] = true;
                open.push(next);
            }
        }
    }
    seen
}
/// Eight points round [x, z] at `r`.
fn ring(x: f64, z: f64, r: f64) -> impl Iterator<Item = [f64; 2]> {
    (0..8).map(move |k| {
        let a = std::f64::consts::TAU * k as f64 / 8.;
        [x + r * a.cos(), z + r * a.sin()]
    })
}
/// Whether a hotdog may be put at [x, z] (see the module's documentation): its feet's height there, if so.
fn stands(level: &Level, reach: &[bool], x: f64, z: f64) -> Option<f64> {
    let inside = x >= level.x0 + EDGE && x <= level.x1 - EDGE && z >= level.z0 + EDGE && z <= level.z1 - EDGE;
    if !inside || !level.clear(x, z) || !reach[level.cell_index(x, z)] || level.sunk(x, z) {
        return None;
    }
    let r = HOTDOG_RADIUS + CLEAR;
    if level.map.ramps.iter().any(|ramp| x >= ramp.rect[0] - r && x <= ramp.rect[2] + r && z >= ramp.rect[1] - r && z <= ramp.rect[3] + r) {
        return None;
    }
    let y = level.ground(x, z);
    let level_under = |[px, pz]: [f64; 2]| level.walkable(px, y, pz) && !level.sunk(px, pz) && (level.ground(px, pz) - y).abs() < 1e-6;
    (level.walkable(x, y, z) && ring(x, z, r).all(level_under)).then_some(y)
}
/// The way a hotdog standing at `at` faces: of sixteen, the one with the most open counter ahead at its eye (blocks
/// and the counter's edge close it; LOOK is open enough), the middle of the map counting MIDDLE beside it. (A
/// hotdog looks along (-sin yaw, -cos yaw).) It is worked out for a spot when a hotdog is put there, on the level as
/// it is then, rather than for every spot of the survey.
pub fn facing(level: &Level, at: Vec3) -> f32 {
    let (x, y, z) = (at.x as f64, at.y as f64, at.z as f64);
    let middle = [(level.x0 + level.x1) / 2. - x, (level.z0 + level.z1) / 2. - z];
    let length = middle[0].hypot(middle[1]);
    let towards = if length > 1e-6 { [middle[0] / length, middle[1] / length] } else { [0., 0.] };
    let eye = [x, y + EYE as f64, z];
    let mut best = (f64::NEG_INFINITY, 0.);
    for k in 0..16 {
        let yaw = -std::f64::consts::PI + std::f64::consts::TAU * k as f64 / 16.;
        let look = [-yaw.sin(), -yaw.cos()];
        // Where it would leave the counter.
        let out = |at: f64, d: f64, low: f64, high: f64| if d > 1e-9 { (high - at) / d } else if d < -1e-9 { (low - at) / d } else { f64::INFINITY };
        let edge = out(x, look[0], level.x0, level.x1).min(out(z, look[1], level.z0, level.z1));
        let open = level.cover(eye, [look[0], 0., look[1]]).min(edge).min(LOOK);
        let score = open + MIDDLE * (look[0] * towards[0] + look[1] * towards[1]);
        if score > best.0 + 1e-9 {
            best = (score, yaw);
        }
    }
    best.1 as f32
}
/// Every place a hotdog may appear on `level` in Free For All, its feet on the ground there (see the module's
/// documentation): the map's starts that qualify first, then the grid's points row by row.
pub fn spots(level: &Level) -> Vec<Vec3> {
    let reach = reachable(level);
    let mut points: Vec<[f64; 2]> = level.map.starts.iter().map(|start| start.p).collect();
    let (x0, z0, x1, z1) = (level.x0 + EDGE, level.z0 + EDGE, level.x1 - EDGE, level.z1 - EDGE);
    if x1 >= x0 && z1 >= z0 {
        let (nx, nz) = (((x1 - x0) / GRID).floor() as usize + 1, ((z1 - z0) / GRID).floor() as usize + 1);
        // The grid is centred on the counter.
        let (ox, oz) = (x0 + (x1 - x0 - (nx - 1) as f64 * GRID) / 2., z0 + (z1 - z0 - (nz - 1) as f64 * GRID) / 2.);
        for j in 0..nz {
            for i in 0..nx {
                points.push([ox + i as f64 * GRID, oz + j as f64 * GRID]);
            }
        }
    }
    points
        .into_iter()
        .filter_map(|[x, z]| stands(level, &reach, x, z).map(|y| Vec3::new(x as f32, y as f32, z as f32)))
        .collect()
}
/// Farthest-point sampling: `n` of `spots` (all of them, then round again, when there are fewer), the first drawn
/// at random, each next the one farthest from those already taken (the first such on a tie).
pub(crate) fn spread(spots: &[Vec3], n: usize, dice: &mut Dice) -> Vec<Vec3> {
    let first = dice.below(spots.len());
    farthest(spots, first, n)
}
/// `spread` from the spot numbered `first`.
fn farthest(spots: &[Vec3], first: usize, n: usize) -> Vec<Vec3> {
    if spots.is_empty() || n == 0 {
        return vec![];
    }
    let mut taken = vec![first];
    let mut gap: Vec<f32> = spots.iter().map(|s| across(*s, spots[first])).collect();
    while taken.len() < n.min(spots.len()) {
        let next = (0..spots.len()).fold(0, |best, i| if gap[i] > gap[best] { i } else { best });
        taken.push(next);
        for (i, s) in spots.iter().enumerate() {
            gap[i] = gap[i].min(across(*s, spots[next]));
        }
    }
    (0..n).map(|k| spots[taken[k % taken.len()]]).collect()
}

impl Feast {
    /// The map's spots, surveyed on its own level (without a napkin's walls or a forcefield) unless they have been.
    pub(crate) fn survey(&mut self) {
        if self.spots.is_empty() {
            self.spots = spots(self.base.as_ref().unwrap_or(&self.level));
        }
    }
    /// Whether a body put at the spot now would be in nothing: no wall of the level as it is (a draped napkin's
    /// included) under its feet, and no loose object between its feet and above its head, at its middle or its side.
    pub(crate) fn free(&self, at: Vec3) -> bool {
        let (x, y, z) = (at.x as f64, at.y as f64, at.z as f64);
        let top = y + actor::HEIGHT + 0.02;
        std::iter::once([x, z]).chain(ring(x, z, HOTDOG_RADIUS)).all(|[px, pz]| {
            self.level.walkable(px, y, pz) && self.props.raycast([px, top, pz], [0., -1., 0.], top - y - 0.005, None).is_none()
        })
    }
    /// Whether no rival (its feet and its eye) is within CLOSE of the spot or can see a hotdog's eye standing there.
    fn unseen(&self, at: Vec3, rivals: &[(Vec3, Vec3)]) -> bool {
        let eye = at + Vec3::Y * EYE;
        rivals.iter().all(|&(feet, from)| {
            let d = eye - from;
            let far = d.length();
            across(feet, at) >= CLOSE && far > 1e-4 && self.cover(from, d / far) + 0.02 < far
        })
    }
    /// Free For All's opening (see the module's documentation): the hotdogs spread over the spots that are free,
    /// each facing its spot's way; `fit` puts a hotdog back on its own. Called as the rules set a round out.
    pub(crate) fn open(&mut self) {
        self.survey();
        let free: Vec<Vec3> = self.spots.iter().copied().filter(|s| self.free(*s)).collect();
        let pool = if free.len() >= self.players.len() { free } else { self.spots.clone() };
        let mut dice = Dice::new(&[self.card_seed, self.round as u64, OPENING]);
        let mut chosen: Vec<Spot> = spread(&pool, self.players.len(), &mut dice).into_iter().map(|p| Spot { p, yaw: facing(&self.level, p) }).collect();
        for i in (1..chosen.len()).rev() {
            chosen.swap(i, dice.below(i + 1));
        }
        self.came_back.clear();
        for (p, spot) in self.players.iter_mut().zip(&chosen) {
            stand(p, spot);
            self.came_back.insert(p.id, spot.flat());
        }
        self.openings = chosen;
        self.embody();
    }
    /// Where a hotdog comes back in Free For All (see the module's documentation).
    pub(crate) fn haven(&mut self, id: u32) -> (Vec3, f32) {
        self.survey();
        let Some(me) = self.player(id) else {
            return self.level.start(0);
        };
        let (died, deaths, last) = (Vec2::new(me.p[0], me.p[2]), me.deaths, self.came_back.get(&id).copied());
        let rivals: Vec<(Vec3, Vec3)> = self.players.iter().filter(|p| p.alive && p.id != id).map(|p| (Vec3::from_array(p.p), muzzle(p))).collect();
        let nearest = |at: Vec3| rivals.iter().map(|&(feet, _)| across(feet, at)).fold(f32::INFINITY, f32::min);
        let mut dice = Dice::new(&[self.card_seed, self.round as u64, RESPAWN, id as u64, deaths as u64]);
        let mut order: Vec<(f32, u64, usize)> = self
            .spots
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (mut score, flat) = (nearest(*s).min(ROOM), Vec2::new(s.x, s.z));
                if flat.distance(died) < NEAR {
                    score -= DIED;
                }
                if last.is_some_and(|last| flat.distance(last) < NEAR) {
                    score -= AGAIN;
                }
                (score, dice.roll(), i)
            })
            .collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut best = Vec::with_capacity(CHOICE);
        for &(_, _, i) in &order {
            let spot = self.spots[i];
            if self.unseen(spot, &rivals) && self.free(spot) {
                best.push(spot);
                if best.len() == CHOICE {
                    break;
                }
            }
        }
        if !best.is_empty() {
            let at = best[dice.below(best.len())];
            return (at, facing(&self.level, at));
        }
        // Nowhere safe: by the rule from before there were spots, the spot furthest from the living (a free one if
        // there is one; and the map's starts, on a map with no spot at all).
        let furthest = |free: bool| self.spots.iter().copied().filter(|s| !free || self.free(*s)).max_by(|a, b| nearest(*a).total_cmp(&nearest(*b)));
        if let Some(at) = furthest(true).or_else(|| furthest(false)) {
            return (at, facing(&self.level, at));
        }
        let n = self.level.map.starts.len().max(1);
        (0..n).map(|i| self.level.start(i)).max_by(|a, b| nearest(a.0).total_cmp(&nearest(b.0))).unwrap_or_else(|| self.level.start(0))
    }
}
/// A hotdog put at a spot, facing its way, keeping to it.
fn stand(p: &mut Player, spot: &Spot) {
    (p.p, p.yaw, p.camp) = (spot.p.to_array(), spot.yaw, [spot.p.x, spot.p.z]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, LIP, Map};
    use crate::rules::{FFA_RESPAWN, Rules};

    fn read(path: &str) -> String {
        std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }
    /// A round of Free For All on `map`, its dice thrown from `seed`.
    fn ffa(map: Map, seed: u64) -> Feast {
        let mut s = Feast::with_map(map, false);
        s.card_seed = seed;
        s.set_rules(Rules::parse("mode=ffa"));
        s
    }
    fn run(s: &mut Feast, seconds: f32) {
        for _ in 0..(seconds * 60.).round() as usize {
            s.tick(1. / 60.);
        }
    }
    /// Nobody plays by the bots' rules: they stand where they are put.
    fn people(s: &mut Feast) {
        for p in &mut s.players {
            p.bot = false;
        }
    }
    fn put(s: &mut Feast, id: u32, x: f32, z: f32) {
        let y = s.level.ground_at(x, z);
        s.player_mut(id).unwrap().p = [x, y, z];
        s.embody();
    }
    fn at(s: &Feast, id: u32) -> Vec3 {
        Vec3::from_array(s.player(id).unwrap().p)
    }
    /// The smallest distance across the counter between any two of `points`.
    fn closest(points: &[Vec3]) -> f32 {
        let mut least = f32::INFINITY;
        for (i, a) in points.iter().enumerate() {
            for b in &points[i + 1..] {
                least = least.min(across(*a, *b));
            }
        }
        least
    }
    /// Whether a living rival of `id` can see a hotdog's eye standing at `spot`, as a bot's shot would.
    fn seen(s: &Feast, id: u32, spot: Vec3) -> bool {
        let eye = spot + Vec3::Y * EYE;
        s.players.iter().filter(|p| p.alive && p.id != id).any(|p| {
            let from = muzzle(p);
            let far = from.distance(eye);
            s.cover(from, (eye - from) / far) + 0.02 >= far
        })
    }
    fn nearest(s: &Feast, id: u32, spot: Vec3) -> f32 {
        s.players.iter().filter(|p| p.alive && p.id != id).map(|p| across(Vec3::from_array(p.p), spot)).fold(f32::INFINITY, f32::min)
    }

    #[test]
    fn free_for_all_candidates_are_walkable_clear_and_on_the_map() {
        let mut names: Vec<String> = std::fs::read_dir(format!("{}/../../assets/maps", env!("CARGO_MANIFEST_DIR")))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".json"))
            .collect();
        names.sort();
        assert!(names.len() >= 10, "{names:?}");
        for name in names {
            let level = Level::new(Map::parse(&read(&format!("assets/maps/{name}"))).unwrap());
            let found = spots(&level);
            if name == "counter-siege.json" {
                assert!(found.len() >= 30, "{}", found.len());
            }
            assert!(found.len() >= 30, "{name}: {}", found.len());
            // From a point to a rect, across the counter.
            let off = |x: f64, z: f64, r: [f64; 4]| (r[0] - x).max(x - r[2]).max(0.).hypot((r[1] - z).max(z - r[3]).max(0.));
            for spot in &found {
                let (x, y, z) = (spot.x as f64, spot.y as f64, spot.z as f64);
                let [x0, z0, x1, z1] = level.map.bounds;
                assert!(x >= x0 + EDGE - 1e-6 && x <= x1 - EDGE + 1e-6 && z >= z0 + EDGE - 1e-6 && z <= z1 - EDGE + 1e-6, "{name}: {spot} off the counter");
                // Every block it does not stand above is a body's radius and more away (a deck may stand over one).
                for block in level.map.blocks.iter().filter(|b| FLOOR + b.height + LIP > y) {
                    assert!(off(x, z, block.rect) >= HOTDOG_RADIUS + CLEAR, "{name}: {spot} {:.3} m from a {}", off(x, z, block.rect), block.style);
                }
                for sink in &level.map.sinks {
                    assert!(!level.sunk(x, z) && off(x, z, sink.rect) >= HOTDOG_RADIUS, "{name}: {spot} over a sink");
                }
                for ramp in &level.map.ramps {
                    assert!(off(x, z, ramp.rect) >= HOTDOG_RADIUS, "{name}: {spot} on a ramp");
                }
                assert!((level.ground(x, z) - y).abs() < 1e-5 && level.walkable(x, y, z), "{name}: {spot} not on the ground");
                assert!(level.clear(x, z), "{name}: {spot} on a cell the routes keep out of");
            }
        }
    }

    #[test]
    fn a_free_for_all_round_starts_spread_out() {
        // The map's starts on Counter Siege: two of them 0.55 m apart.
        let level = Level::new(Map::bundled());
        let starts: Vec<Vec3> = (0..5).map(|i| level.start(i).0).collect();
        let old = closest(&starts);
        assert!((old - 0.551).abs() < 0.001, "{old}");
        for seed in 0..12 {
            let mut s = ffa(Map::bundled(), seed);
            let opening: Vec<Vec3> = (1..=5).map(|id| at(&s, id)).collect();
            assert!(closest(&opening) >= 0.9, "seed {seed}: {}", closest(&opening));
            // Each on a free spot of the survey, facing its spot's way, and fitting a hotdog out keeps it there.
            for (i, p) in s.players.iter().enumerate() {
                assert!(s.spots.contains(&Vec3::from_array(p.p)) && s.free(Vec3::from_array(p.p)));
                assert_eq!((p.p, p.yaw), (s.openings[i].p.to_array(), s.openings[i].yaw));
            }
            for id in 1..=5 {
                s.fit(id);
                assert_eq!(at(&s, id), opening[id as usize - 1]);
                let body = s.body(id).unwrap().p;
                assert!(Vec3::new(body[0] as f32, body[1] as f32, body[2] as f32).distance(opening[id as usize - 1]) < 1e-5, "its body with it");
            }
        }
        // Whichever spot is drawn first (the only draw farthest-point sampling makes), the closest two of the five are
        // 0.92 m apart or more: two thirds as far again as the starts.
        let s = ffa(Map::bundled(), 0);
        let pool: Vec<Vec3> = s.spots.iter().copied().filter(|p| s.free(*p)).collect();
        let least = (0..pool.len()).map(|first| closest(&farthest(&pool, first, 5))).fold(f32::INFINITY, f32::min);
        assert!(least >= 0.9 && least >= 1.6 * old, "{least}");
        // Another seed, another opening; and the next round, another again.
        let opening = |s: &Feast| s.players.iter().map(|p| p.p).collect::<Vec<_>>();
        let (one, two) = (ffa(Map::bundled(), 1), ffa(Map::bundled(), 2));
        assert_ne!(opening(&one), opening(&two));
        let mut next = ffa(Map::bundled(), 1);
        next.reset();
        assert_ne!(opening(&next), opening(&one));
        assert!(closest(&next.players.iter().map(|p| Vec3::from_array(p.p)).collect::<Vec<_>>()) >= 0.9);
    }

    #[test]
    fn a_respawn_is_out_of_sight_of_the_living_when_somewhere_is() {
        for seed in 0..8 {
            let mut s = ffa(Map::bundled(), seed);
            people(&mut s);
            // The four rivals at the left end of the counter see most of it; the microwave and the steel blocks hide
            // the rest.
            for (id, x, z) in [(2, -1.65, 2.45), (3, -1.65, 3.0), (4, -1.25, 2.6), (5, -1.0, 2.85)] {
                put(&mut s, id, x, z);
            }
            put(&mut s, 1, 0.8, 3.2);
            let hidden = s.spots.iter().filter(|spot| !seen(&s, 1, **spot) && nearest(&s, 1, **spot) >= CLOSE).count();
            assert!(hidden >= 3, "somewhere is out of sight: {hidden}");
            assert!(s.spots.iter().filter(|spot| seen(&s, 1, **spot)).count() > s.spots.len() / 2, "and most of it in sight");
            s.hurt(1, 500.);
            run(&mut s, FFA_RESPAWN + 0.05);
            let back = s.player(1).unwrap();
            assert!(back.alive && back.shield > 0.);
            let spot = Vec3::from_array(back.p);
            assert!(!seen(&s, 1, spot), "seed {seed}: {spot} is in sight");
            assert!(nearest(&s, 1, spot) >= CLOSE, "seed {seed}: {spot}");
        }
    }

    #[test]
    fn a_respawn_avoids_where_it_died_and_its_last_spot() {
        let mut s = ffa(Map::bundled(), 3);
        people(&mut s);
        // Nobody else is about: every spot is safe and as good as any other, but for those two.
        for id in 2..=5 {
            s.hurt(id, 500.);
            s.player_mut(id).unwrap().respawn = 1e6;
        }
        let (mut last, mut near_death, mut near_last) = (at(&s, 1), 0, 0);
        for k in 0..16 {
            let died = Vec3::new(-1.4 + 0.19 * k as f32, 0., if k % 2 == 0 { 2.4 } else { 3.5 });
            put(&mut s, 1, died.x, died.z);
            // (Its spawn protection would spare it.)
            s.player_mut(1).unwrap().shield = 0.;
            s.hurt(1, 500.);
            run(&mut s, FFA_RESPAWN + 0.05);
            let back = at(&s, 1);
            assert!(s.player(1).unwrap().alive);
            assert!(across(back, died) >= NEAR, "{k}: back at {back}, {} m from where it died", across(back, died));
            assert!(across(back, last) >= NEAR, "{k}: back at {back}, {} m from where it came back before", across(back, last));
            // (Unmarked, as many of the spots are near one of the two as would have been taken by chance.)
            near_death += s.spots.iter().filter(|p| across(**p, died) < NEAR).count();
            near_last += s.spots.iter().filter(|p| across(**p, last) < NEAR).count();
            last = back;
        }
        assert!(near_death > 16 * 10 && near_last > 16 * 10, "{near_death} {near_last}");
    }

    #[test]
    fn with_nowhere_safe_it_falls_back_to_furthest_from_the_living() {
        // Counter Siege bare: nothing hides anything from anyone.
        let mut map = Map::bundled();
        (map.blocks, map.decks, map.ramps, map.props, map.loose, map.order) = (vec![], vec![], vec![], vec![], vec![], vec![]);
        let mut s = ffa(map, 5);
        people(&mut s);
        for (id, x, z) in [(2, -1.2, 2.6), (3, -0.2, 3.3), (4, 0.6, 2.5), (5, 1.5, 3.4)] {
            put(&mut s, id, x, z);
        }
        put(&mut s, 1, 0., 2.9);
        assert!(s.spots.iter().all(|spot| seen(&s, 1, *spot)), "nowhere safe");
        let furthest = s.spots.iter().copied().max_by(|a, b| nearest(&s, 1, *a).total_cmp(&nearest(&s, 1, *b))).unwrap();
        s.hurt(1, 500.);
        run(&mut s, FFA_RESPAWN + 0.05);
        let back = at(&s, 1);
        assert_eq!(back, furthest, "{} m from the living, the furthest {} m", nearest(&s, 1, back), nearest(&s, 1, furthest));
        assert!(nearest(&s, 1, back) > 0.8);
        assert_eq!(s.player(1).unwrap().yaw, facing(&s.level, back));
    }

    #[test]
    fn the_same_seed_gives_the_same_spawns() {
        let play = |seed: u64| {
            let mut s = ffa(Map::bundled(), seed);
            people(&mut s);
            let mut seen = vec![s.players.iter().map(|p| (p.p, p.yaw)).collect::<Vec<_>>()];
            for (id, x, z) in [(2, -1.0, 2.6), (3, 0.9, 3.0), (4, 0.2, 2.9)] {
                put(&mut s, id, x, z);
            }
            for victim in [1, 5, 1, 3, 5] {
                s.hurt(victim, 500.);
                run(&mut s, FFA_RESPAWN + 0.05);
                seen.push(s.players.iter().map(|p| (p.p, p.yaw)).collect());
            }
            seen
        };
        assert_eq!(play(42), play(42));
        assert_ne!(play(42)[0], play(43)[0]);
        // The dice themselves: the same words, the same throws.
        let throws = |words: &[u64]| {
            let mut dice = Dice::new(words);
            (0..4).map(|_| dice.roll()).collect::<Vec<_>>()
        };
        assert_eq!(throws(&[42, 1, OPENING]), throws(&[42, 1, OPENING]));
        assert_ne!(throws(&[42, 1, OPENING]), throws(&[42, 2, OPENING]));
        assert_ne!(throws(&[0, 1, RESPAWN, 1, 1]), throws(&[0, 1, RESPAWN, 1, 2]));
    }
}

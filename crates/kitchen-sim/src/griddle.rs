// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The electric griddle: a countertop appliance (a map block of the style `griddle`) that any hotdog can switch on or
//! off, whose plate heats and cools, and whose heat burns a hotdog that stands on it.
//!
//! * **The appliance** is a 0.50 by 0.28 m block, 0.06 m high (a dark non-stick plate on short legs), with a thermostat
//!   dial and its indicator light in the middle of its `facing` side. At 0.06 m it is a little more than a step
//!   ([`crate::map::STEP`]): a hotdog climbs onto the plate with a jump (a plain jump rises 0.114 m), and a bot never does.
//! * **Switching.** A hotdog within REACH of the dial (0.25 m, measured from its chest), facing it (within FACE of
//!   straight at it) and on the dial's side of the griddle (or standing on the plate) presses use and the griddle
//!   flips; a griddle will not flip again for LOCK seconds. Anyone may, of either team, in any stage of a round. The
//!   hotdog that last switched it on is remembered (`by`).
//! * **Temperature**, in degrees Celsius. A griddle starts off, at ROOM (21). On, the plate heats towards HOT (205) at
//!   HEAT (9) degrees a second, easing in the last EASE_UP degrees; off, it cools towards ROOM at COOL (3) a second,
//!   easing in the last EASE_DOWN. Both laws ([`warm`]) are smooth and the same everywhere, so a client can carry a
//!   griddle on from the last it was told (`Griddle::advance`).
//! * **Burns.** A hotdog standing on the plate ([`Plate::under`]) takes [`burn_rate`] HP a second: nothing at or below
//!   BURNS_AT (100), rising in a straight line to BURN_RATE (25) at HOT. The record says BURN, with the hotdog that
//!   last switched the griddle on as the attacker (nobody, for a hotdog that burns on a griddle it switched on
//!   itself), so a death shows in the feed. Nobody is burned in a round that is over, at a card table, or behind a
//!   Forts forcefield (where nobody can be hurt).
//! * **Bots** step off a plate that is on or still warm (`Feast::scorched`), by the nearest side that is clear.
//! * **The wire.** Protocol 12's frames are as they were: the server sends `{type:'griddle',griddles:[State]}` as JSON text
//!   beside them, when a griddle is flipped and every SYNC seconds; a client sets the states and carries them on.
use crate::feast::{self, Damage, FLOOR, Feast, Stage};
use crate::map::{Block, LIP, Map, Side};
use glam::Vec3;
use serde::{Deserialize, Serialize};

/// The block style of a griddle (one of map::KITS).
pub const STYLE: &str = "griddle";
/// Room temperature and the plate's greatest temperature, in degrees Celsius.
pub const ROOM: f32 = 21.;
pub const HOT: f32 = 205.;
/// Degrees a second the plate heats while it is on, and cools while it is off, away from the ends of its range.
pub const HEAT: f32 = 9.;
pub const COOL: f32 = 3.;
/// The last degrees of each climb, over which the rate eases down to nothing.
pub const EASE_UP: f32 = 24.;
pub const EASE_DOWN: f32 = 16.;
/// A plate this hot or less does not burn; the burn rises in a straight line to BURN_RATE HP a second at HOT.
pub const BURNS_AT: f32 = 100.;
pub const BURN_RATE: f32 = 25.;
/// A bot leaves a plate that is on, or hotter than this.
pub const WARY: f32 = 50.;
/// How far from the dial a hotdog's chest may be to switch the griddle, how far off straight at it it may face (radians),
/// and the seconds a griddle will not flip again after it has.
pub const REACH: f32 = 0.25;
pub const FACE: f32 = 0.7;
pub const LOCK: f32 = 0.3;
/// The height of a hotdog's chest over its feet, which reach is measured from, and how far the feet's height may be off
/// the plate's top for the hotdog to be standing on it.
const CHEST: f32 = 0.07;
const STANDING: f32 = 0.012;
/// Seconds between two syncs the server sends while a griddle is not being flipped (a client carries the temperature on
/// between them).
pub const SYNC: f32 = 1.;

/// One step of the plate's temperature, `dt` seconds: on, up towards HOT at HEAT degrees a second; off, down towards ROOM at
/// COOL; each easing (the rate falls with the square root of what is left of the climb, so it ends in finite time, exactly
/// at HOT or ROOM) over the last EASE_UP or EASE_DOWN degrees.
pub fn warm(temp: f32, on: bool, dt: f32) -> f32 {
    if on {
        let gap = (HOT - temp).max(0.);
        (temp + (HEAT * (gap / EASE_UP).min(1.).sqrt() * dt).min(gap)).min(HOT)
    } else {
        let gap = (temp - ROOM).max(0.);
        (temp - (COOL * (gap / EASE_DOWN).min(1.).sqrt() * dt).min(gap)).max(ROOM.min(temp))
    }
}
/// HP a second a hotdog standing on a plate at `temp` loses: nothing at BURNS_AT or below, BURN_RATE at HOT.
pub fn burn_rate(temp: f32) -> f32 {
    BURN_RATE * ((temp - BURNS_AT) / (HOT - BURNS_AT)).clamp(0., 1.)
}

/// What the server tells a client of one griddle, and what a client sets from it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// The griddle's block in the map.
    pub block: usize,
    pub on: bool,
    /// Degrees Celsius.
    pub temp: f32,
    /// The hotdog that last switched it on, or 0.
    pub by: u32,
}

/// One griddle's state in a round: which block of the map it is, whether it is on, how hot its plate is and who last
/// switched it on.
#[derive(Clone, Debug, PartialEq)]
pub struct Griddle {
    pub block: usize,
    pub on: bool,
    pub temp: f32,
    pub by: u32,
    /// How many times it has been flipped this round (the server tells clients when it changes), and the seconds before it
    /// will flip again.
    pub flips: u32,
    pub lock: f32,
}
impl Griddle {
    pub fn new(block: usize) -> Griddle {
        Griddle { block, on: false, temp: ROOM, by: 0, flips: 0, lock: 0. }
    }
    /// Carries the plate on `dt` seconds: how a client keeps one going between what the server tells it.
    pub fn advance(&mut self, dt: f32) {
        self.temp = warm(self.temp, self.on, dt);
    }
    /// What a client is told of it: the temperature to a tenth of a degree.
    pub fn state(&self) -> State {
        State { block: self.block, on: self.on, temp: (self.temp * 10.).round() / 10., by: self.by }
    }
}
/// The griddles of a map, one for each block of the style `griddle`, in the map's order.
pub fn of(map: &Map) -> Vec<Griddle> {
    map.blocks.iter().enumerate().filter(|(_, b)| b.style == STYLE).map(|(i, _)| Griddle::new(i)).collect()
}

/// A griddle's block as the rules measure it, in world space: its rectangle [min x, min z, max x, max z], the height a hotdog
/// standing on the plate has its feet at (the block's top and LIP), the thermostat dial (in the middle of the `facing` side, at half the griddle's height) and the way
/// that side faces, [x, z].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plate {
    pub rect: [f32; 4],
    pub top: f32,
    pub dial: [f32; 3],
    pub front: [f32; 2],
}
impl Plate {
    pub fn of(block: &Block) -> Plate {
        let [x0, z0, x1, z1] = block.rect.map(|v| v as f32);
        let height = block.height as f32;
        let (front, at) = match block.facing.unwrap_or(Side::MinusZ) {
            Side::PlusX => ([1., 0.], [x1, (z0 + z1) / 2.]),
            Side::MinusX => ([-1., 0.], [x0, (z0 + z1) / 2.]),
            Side::PlusZ => ([0., 1.], [(x0 + x1) / 2., z1]),
            Side::MinusZ => ([0., -1.], [(x0 + x1) / 2., z0]),
        };
        // (A hotdog stands on a block at its height and LIP: map.rs `walls`.)
        Plate { rect: [x0, z0, x1, z1], top: FLOOR + height + LIP as f32, dial: [at[0], FLOOR + height / 2., at[1]], front }
    }
    /// Whether a hotdog with its feet at `p` is standing on the plate: over it and on its top (not jumping over it, not on
    /// something laid on it).
    pub fn under(&self, p: [f32; 3], grounded: bool) -> bool {
        let [x0, z0, x1, z1] = self.rect;
        grounded && p[0] >= x0 && p[0] <= x1 && p[2] >= z0 && p[2] <= z1 && (p[1] - self.top).abs() <= STANDING
    }
    /// Whether a hotdog with its feet at `p`, facing `yaw`, may switch the griddle: its chest within REACH of the dial, the
    /// dial within FACE of straight ahead, and it on the dial's side (or on the plate).
    pub fn within(&self, p: [f32; 3], yaw: f32) -> bool {
        let chest = Vec3::new(p[0], p[1] + CHEST, p[2]);
        if Vec3::from_array(self.dial).distance_squared(chest) > REACH * REACH {
            return false;
        }
        let flat = [self.dial[0] - p[0], self.dial[2] - p[2]];
        let length = flat[0].hypot(flat[1]);
        // (A hotdog on top of the dial has nothing to turn towards.)
        if length > 0.03 {
            // A hotdog looks along (-sin yaw, -cos yaw).
            let ahead = (flat[0] * -yaw.sin() + flat[1] * -yaw.cos()) / length;
            if ahead < FACE.cos() {
                return false;
            }
        }
        let behind = (p[0] - self.dial[0]) * self.front[0] + (p[2] - self.dial[2]) * self.front[1];
        behind >= -0.02 || self.under(p, true)
    }
    /// The way off the plate for a hotdog standing at `p` on it: the point a little past its nearest edge, trying the other
    /// three in turn if `clear` says the nearest is not (a block, the edge of the counter).
    pub fn exit(&self, p: [f32; 3], clear: impl Fn(f32, f32) -> bool) -> [f32; 2] {
        let [x0, z0, x1, z1] = self.rect;
        const PAST: f32 = 0.05;
        let mut ways = [
            (p[0] - x0, [x0 - PAST, p[2]]),
            (x1 - p[0], [x1 + PAST, p[2]]),
            (p[2] - z0, [p[0], z0 - PAST]),
            (z1 - p[2], [p[0], z1 + PAST]),
        ];
        ways.sort_by(|a, b| a.0.total_cmp(&b.0));
        ways.iter().find(|(_, at)| clear(at[0], at[1])).unwrap_or(&ways[0]).1
    }
}

impl Feast {
    /// The griddle (its place in `griddles`) the hotdog `id` could switch from where it stands, facing as it does: the nearest
    /// dial in reach. None for one that is dead, at a card table or has no griddle in reach.
    pub fn griddle_in_reach(&self, id: u32) -> Option<usize> {
        let p = self.player(id).filter(|p| p.alive && self.cards.is_none())?;
        self.griddle_near(p.p, p.yaw)
    }
    /// The same for a hotdog with its feet at `p` looking along `yaw`, whoever it is (a client's own hotdog is where it
    /// predicts it, not where the round last had it).
    pub fn griddle_near(&self, p: [f32; 3], yaw: f32) -> Option<usize> {
        self.griddles
            .iter()
            .enumerate()
            .filter(|(_, g)| self.level.map.blocks.get(g.block).is_some_and(|b| Plate::of(b).within(p, yaw)))
            .min_by(|(_, a), (_, b)| {
                let far = |g: &Griddle| Vec3::from_array(Plate::of(&self.level.map.blocks[g.block]).dial).distance_squared(Vec3::from_array(p));
                far(a).total_cmp(&far(b))
            })
            .map(|(i, _)| i)
    }
    /// The hotdog `id` presses use: the griddle in reach (`griddle_in_reach`) is flipped, on to off or off to on, and a griddle
    /// it switches on remembers it. Returns whether one flipped (none in reach, or it flipped a moment ago: no).
    pub fn use_griddle(&mut self, id: u32) -> bool {
        match self.griddle_in_reach(id) {
            Some(i) => self.flip_griddle(i, id),
            None => false,
        }
    }
    /// The hotdog `id` flips griddle `i` (its place in `griddles`), wherever it stands: what `use_griddle` does once it has found
    /// the one in reach, and what a client does of its own accord while the server's word is on its way. Returns whether it
    /// flipped (not within LOCK seconds of the last).
    pub fn flip_griddle(&mut self, i: usize, id: u32) -> bool {
        let Some(g) = self.griddles.get_mut(i).filter(|g| g.lock <= 0.) else {
            return false;
        };
        g.on = !g.on;
        g.flips += 1;
        g.lock = LOCK;
        if g.on {
            g.by = id;
        }
        true
    }
    /// A step of every griddle: the plate warms or cools, and a hotdog standing on one that is hot enough is burned.
    pub(crate) fn heat_griddles(&mut self, dt: f32) {
        if self.griddles.is_empty() {
            return;
        }
        for g in &mut self.griddles {
            g.lock = (g.lock - dt).max(0.);
            g.advance(dt);
        }
        if self.cards.is_some() || self.stage == Stage::RoundOver {
            return;
        }
        let mut burns = vec![];
        for g in self.griddles.iter().filter(|g| burn_rate(g.temp) > 0.) {
            let Some(block) = self.level.map.blocks.get(g.block) else {
                continue;
            };
            let plate = Plate::of(block);
            for p in self.players.iter().filter(|p| p.alive && plate.under(p.p, p.grounded)) {
                burns.push((p.id, g.by, burn_rate(g.temp) * dt, p.p));
            }
        }
        for (id, by, amount, at) in burns {
            // (Nobody is credited with the death of a hotdog that burned on the griddle it switched on itself.)
            self.harm(id, amount, false, Damage::by(feast::BURN, if by == id { 0 } else { by }, Vec3::from_array(at), Vec3::ZERO));
        }
    }
    /// A bot standing on a plate that is on or warm walks off it, by the nearest side that is clear, and does nothing else
    /// meanwhile. Returns whether it did.
    pub(crate) fn scorched(&mut self, id: u32) -> bool {
        if self.griddles.is_empty() {
            return false;
        }
        let Some(p) = self.player(id).filter(|p| p.alive).cloned() else {
            return false;
        };
        let level = &self.level;
        let on = self.griddles.iter().filter(|g| g.on || g.temp > WARY).filter_map(|g| level.map.blocks.get(g.block)).map(Plate::of).find(|plate| plate.under(p.p, p.grounded));
        let Some(plate) = on else {
            return false;
        };
        let [x, z] = plate.exit(p.p, |x, z| level.clear(x as f64, z as f64));
        let (dx, dz) = (x - p.p[0], z - p.p[2]);
        let length = dx.hypot(dz).max(1e-6);
        let pace = crate::actor::BOT_WALK;
        if let Some(body) = self.bodies.get_mut(id) {
            body.wish = [(dx / length) as f64 * pace, (dz / length) as f64 * pace];
        }
        if let Some(p) = self.player_mut(id) {
            p.yaw = (-dx).atan2(-dz);
            p.pitch = 0.;
        }
        true
    }
    /// A client is told the griddles' states (`State`): each one it has the block of is set, and the rest left alone.
    pub fn hear_griddles(&mut self, told: &[State]) {
        for state in told {
            if let Some(g) = self.griddles.iter_mut().find(|g| g.block == state.block) {
                if state.on != g.on {
                    g.flips += 1;
                }
                (g.on, g.temp, g.by) = (state.on, if state.temp.is_finite() { state.temp } else { ROOM }, state.by);
            }
        }
    }
    /// What a client is told of the griddles now.
    pub fn griddle_states(&self) -> Vec<State> {
        self.griddles.iter().map(Griddle::state).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Level;
    use std::f32::consts::PI;

    fn forts(name: &str) -> Map {
        let text = std::fs::read_to_string(format!("{}/../../assets/maps/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
        Map::parse(&text).unwrap()
    }
    /// A small map with a griddle: 0.50 by 0.28 m, facing +z, so its dial is in the middle of its back side, at z = 3.4.
    fn kitchen() -> Feast {
        let map = Map::parse(
            r#"{"format": "hotdog-map", "version": 2, "name": "Griddle", "bounds": [-1.5, 2.5, 1.5, 4], "nav": {"cell": 0.04, "clearance": 0.035},
            "starts": [{"p": [-1.2, 2.8], "yaw": 0}, {"p": [1.2, 2.8], "yaw": 0}, {"p": [-1.2, 3.6], "yaw": 0}, {"p": [1.2, 3.6], "yaw": 0}, {"p": [0, 2.8], "yaw": 0}],
            "spawns": [{"p": [1.2, 3.2]}],
            "blocks": [{"rect": [-0.25, 3.12, 0.25, 3.4], "height": 0.06, "style": "griddle", "facing": "+z"}]}"#,
        )
        .unwrap();
        Feast::with_map(map, false)
    }
    /// Player `id` (1 is a person's, the rest bots) set down at `at`, facing `yaw`.
    fn place(s: &mut Feast, id: u32, at: [f32; 3], yaw: f32) {
        let p = s.players.iter_mut().find(|p| p.id == id).unwrap();
        (p.p, p.yaw, p.grounded) = (at, yaw, true);
        s.embody();
    }
    /// The yaw that looks from `from` towards the dial (a hotdog looks along (-sin yaw, -cos yaw)).
    fn at_the_dial(s: &Feast, from: [f32; 3]) -> f32 {
        let dial = Plate::of(&s.level.map.blocks[0]).dial;
        (from[0] - dial[0]).atan2(from[2] - dial[2])
    }
    /// Feet standing on the plate: the block's 0.06 and the LIP a body keeps over a block's top (map.rs `walls`).
    const TOP: f32 = FLOOR + 0.0652;
    /// A hotdog 0.15 m in front of the dial, looking straight at it (towards -z).
    const FRONT: [f32; 3] = [0., FLOOR, 3.55];

    #[test]
    fn a_griddle_starts_off_at_room_temperature_one_for_each_block_of_the_style() {
        let s = kitchen();
        assert_eq!(s.griddles.len(), 1);
        assert_eq!((s.griddles[0].block, s.griddles[0].on, s.griddles[0].temp, s.griddles[0].by), (0, false, 21., 0));
        let two = Map::parse(
            r#"{"format": "hotdog-map", "version": 2, "bounds": [-1, 2, 1, 4], "starts": [{"p": [0, 2.5], "yaw": 0}], "spawns": [{"p": [0, 3]}],
            "blocks": [{"rect": [-0.9, 3, -0.5, 3.2], "style": "steel"}, {"rect": [0.1, 3, 0.4, 3.2], "height": 0.06, "style": "griddle"}, {"rect": [0.5, 3, 0.9, 3.3], "height": 0.06, "style": "griddle", "facing": "+x"}]}"#,
        )
        .unwrap();
        assert_eq!(of(&two).iter().map(|g| g.block).collect::<Vec<_>>(), [1, 2]);
        assert!(Feast::new(false).griddles.is_empty(), "a map without one has none");
    }

    #[test]
    fn the_plate_heats_about_nine_degrees_a_second_to_205_and_eases_into_it() {
        let mut t = ROOM;
        for _ in 0..30 {
            t = warm(t, true, 1. / 30.);
        }
        assert!((t - ROOM - HEAT).abs() < 0.1, "a second of heating from room temperature: {t}");
        // Never past 205, always rising, and there in finite time.
        let (mut t, mut seconds, mut last_rate) = (ROOM, 0., HEAT);
        while t < HOT {
            let next = warm(t, true, 1. / 30.);
            assert!(next > t && next <= HOT, "{t} -> {next}");
            let rate = (next - t) * 30.;
            assert!(rate <= last_rate + 1e-3, "the rate only falls: {rate} after {last_rate} at {t}");
            if t < HOT - EASE_UP - 1. {
                assert!((rate - HEAT).abs() < 0.01, "full rate below the easing at {t}: {rate}");
            }
            (t, last_rate, seconds) = (next, rate, seconds + 1. / 30.);
            assert!(seconds < 40., "never gets there");
        }
        assert_eq!(t, HOT);
        assert!((21. ..26.).contains(&seconds), "about 184 degrees at 9 a second, and the easing: {seconds} s");
        assert!(last_rate < 1., "it eases into the top: {last_rate}");
        assert_eq!(warm(HOT, true, 1.), HOT, "and stays");
    }

    #[test]
    fn the_plate_cools_about_three_degrees_a_second_to_room_temperature_and_eases_into_it() {
        let mut t = HOT;
        for _ in 0..30 {
            t = warm(t, false, 1. / 30.);
        }
        assert!((HOT - t - COOL).abs() < 0.05, "a second of cooling from the top: {t}");
        let (mut t, mut seconds, mut last_rate) = (HOT, 0., COOL);
        while t > ROOM {
            let next = warm(t, false, 1. / 30.);
            assert!(next < t && next >= ROOM, "{t} -> {next}");
            let rate = (t - next) * 30.;
            assert!(rate <= last_rate + 1e-3, "the rate only falls: {rate} after {last_rate} at {t}");
            (t, last_rate, seconds) = (next, rate, seconds + 1. / 30.);
            assert!(seconds < 120., "never gets there");
        }
        assert_eq!(t, ROOM);
        assert!((60. ..68.).contains(&seconds), "about 184 degrees at 3 a second, and the easing: {seconds} s");
        assert!(last_rate < 0.5, "it eases into room temperature: {last_rate}");
        assert_eq!(warm(ROOM, false, 1.), ROOM, "and stays");
        // Switched off half way up, it turns round where it is.
        assert!(warm(100., false, 0.1) < 100. && warm(100., true, 0.1) > 100.);
        // A big step lands on the limit, not past it.
        assert_eq!((warm(204., true, 10.), warm(22., false, 10.)), (HOT, ROOM));
        // Cooling is slower than heating at every temperature.
        for t in [30., 60., 100., 150., 200.] {
            assert!(warm(t, true, 0.1) - t > t - warm(t, false, 0.1), "{t}");
        }
    }

    #[test]
    fn a_step_of_the_round_warms_a_griddle_that_is_on_and_cools_one_that_is_off() {
        let mut s = kitchen();
        s.griddles[0].on = true;
        s.tick(2.);
        assert!((s.griddles[0].temp - (ROOM + 2. * HEAT)).abs() < 0.3, "{}", s.griddles[0].temp);
        s.griddles[0].on = false;
        s.tick(2.);
        assert!((s.griddles[0].temp - (ROOM + 2. * HEAT - 2. * COOL)).abs() < 0.5, "{}", s.griddles[0].temp);
        // A client that carries a copy on from a sync, a frame at a time, keeps the same time as the round's own steps.
        let mut s = kitchen();
        s.griddles[0].on = true;
        let mut copy = Griddle { on: true, ..Griddle::new(0) };
        for _ in 0..3 {
            s.tick(10.);
        }
        for _ in 0..(30 * 60) {
            copy.advance(1. / 60.);
        }
        assert_eq!((s.griddles[0].temp, copy.temp), (HOT, HOT));
        let (mut a, mut b) = (Griddle { on: true, ..Griddle::new(0) }, Griddle { on: true, ..Griddle::new(0) });
        for _ in 0..300 {
            a.advance(1. / 30.);
        }
        for _ in 0..1200 {
            b.advance(1. / 120.);
        }
        assert!((a.temp - b.temp).abs() < 0.2, "the step does not matter: {} {}", a.temp, b.temp);
    }

    #[test]
    fn use_switches_a_griddle_from_within_reach_of_its_dial_facing_it() {
        let mut s = kitchen();
        place(&mut s, 1, FRONT, 0.);
        assert_eq!(s.griddle_in_reach(1), Some(0));
        assert!(s.use_griddle(1) && s.griddles[0].on && s.griddles[0].by == 1 && s.griddles[0].flips == 1);
        // (It will not flip again at once, whatever is pressed.)
        assert!(!s.use_griddle(1) && s.griddles[0].on);
        s.tick(0.4);
        assert!(s.use_griddle(1) && !s.griddles[0].on && s.griddles[0].by == 1, "off again, and it remembers who switched it on");
        s.tick(0.4);
        // Anyone, of either team: another hotdog switches it on and is remembered.
        place(&mut s, 1, [1., FLOOR, 3.], 0.);
        place(&mut s, 3, [0.1, FLOOR, 3.5], 0.);
        s.players[2].bot = false;
        s.players[2].yaw = at_the_dial(&s, [0.1, FLOOR, 3.5]);
        assert!(s.use_griddle(3) && s.griddles[0].on && s.griddles[0].by == 3);
        // The same in every stage of a round.
        for stage in [Stage::Warmup, Stage::Wave, Stage::FreeForAll, Stage::Build, Stage::Fight, Stage::RoundOver] {
            s.stage = stage;
            s.tick(0.4);
            let yaw = at_the_dial(&s, [0.1, FLOOR, 3.5]);
            place(&mut s, 3, [0.1, FLOOR, 3.5], yaw);
            let before = s.griddles[0].on;
            assert!(s.use_griddle(3) && s.griddles[0].on != before, "{stage:?}");
        }
    }

    #[test]
    fn use_needs_the_dial_within_a_quarter_of_a_metre_ahead_of_the_hotdog_and_a_hotdog_that_is_alive() {
        let mut s = kitchen();
        // Reach: 0.25 m from the chest to the dial (0.03 m high, in the middle of the back side).
        for (z, near) in [(3.5, true), (3.6, true), (3.63, true), (3.66, false), (3.9, false)] {
            place(&mut s, 1, [0., FLOOR, z], 0.);
            assert_eq!(s.griddle_in_reach(1).is_some(), near, "{} m from the dial", z - 3.4);
        }
        // Along the side of it, looking at the dial.
        for (x, near) in [(0.1, true), (0.2, true), (0.24, true), (0.26, false)] {
            let from = [x, FLOOR, 3.45];
            let yaw = at_the_dial(&s, from);
            place(&mut s, 1, from, yaw);
            assert_eq!(s.griddle_in_reach(1).is_some(), near, "{x} m along");
        }
        // Facing: straight at it, a little off, and turned away.
        for (turn, faces) in [(0., true), (0.3, true), (-0.5, true), (0.9, false), (1.57, false), (3.14, false)] {
            place(&mut s, 1, FRONT, turn);
            assert_eq!(s.griddle_in_reach(1).is_some(), faces, "{turn} rad off straight at it");
            assert_eq!(s.use_griddle(1), faces);
            s.griddles[0] = Griddle::new(0);
        }
        // Round the back of the griddle, the dial is 0.4 m off, and through the body, looking at it.
        let from = [0., FLOOR, 3.0];
        let yaw = at_the_dial(&s, from);
        place(&mut s, 1, from, yaw);
        assert!(s.griddle_in_reach(1).is_none());
        // Dead, nothing.
        place(&mut s, 1, FRONT, 0.);
        s.players[0].alive = false;
        assert!(s.griddle_in_reach(1).is_none() && !s.use_griddle(1) && !s.griddles[0].on);
        // Nobody by that number.
        assert!(!s.use_griddle(99));
    }

    #[test]
    fn a_hotdog_on_the_plate_can_reach_the_dial_too() {
        let mut s = kitchen();
        // On the plate, 0.2 m from the dial along it, facing it.
        place(&mut s, 1, [0., TOP, 3.2], PI);
        assert!(Plate::of(&s.level.map.blocks[0]).under(s.players[0].p, true));
        assert!(s.use_griddle(1));
    }

    #[test]
    fn a_plate_burns_nothing_at_100_degrees_and_rises_in_a_straight_line_to_25_hp_a_second_at_205() {
        assert_eq!([burn_rate(21.), burn_rate(99.), burn_rate(100.)], [0.; 3]);
        assert!((burn_rate(152.5) - 12.5).abs() < 1e-4 && (burn_rate(205.) - 25.).abs() < 1e-5 && burn_rate(300.) == 25.);
        assert!((burn_rate(120.) - 25. * 20. / 105.).abs() < 1e-4);
        // In a round: a second on the plate at a few temperatures (the plate staying about as hot, on at the top and
        // cooling a little otherwise).
        for (temp, per_second) in [(99., 0.), (100., 0.), (120., 4.76), (150., 11.9), (205., 25.)] {
            let mut s = kitchen();
            s.stage = Stage::Fight;
            place(&mut s, 1, [0., TOP, 3.25], 0.);
            s.griddles[0].temp = temp;
            s.griddles[0].on = temp >= 205.;
            s.tick(1.);
            let lost = 100. - s.players[0].hp;
            assert!((lost - per_second).abs() < 0.6 + per_second * 0.04, "{temp} C: {lost} HP in a second, wanted {per_second}");
            if per_second > 0. {
                let record = s.players[0].last_damage;
                assert_eq!((record.kind, record.attacker), (feast::BURN, 0));
                assert!(record.amount > 0. && record.amount < 1.5 && (record.p[1] - TOP).abs() < 0.002);
            } else {
                assert_eq!(s.players[0].last_damage, Damage::default(), "nothing on its record at {temp}");
            }
        }
    }

    #[test]
    fn only_a_hotdog_standing_on_the_plate_is_burned() {
        let mut s = kitchen();
        s.stage = Stage::Fight;
        s.griddles[0].on = true;
        s.griddles[0].temp = HOT;
        for p in s.players.iter_mut() {
            p.bot = false;
        }
        // On the counter beside it, in the air over it (a jump), on the plate but dead, and on a napkin's height over it.
        place(&mut s, 1, [0.4, FLOOR, 3.25], 0.);
        place(&mut s, 2, [0., TOP, 3.25], 0.);
        s.players[1].grounded = false;
        place(&mut s, 3, [-0.1, TOP, 3.25], 0.);
        s.players[2].alive = false;
        place(&mut s, 4, [0.1, TOP + 0.05, 3.25], 0.);
        s.heat_griddles(0.5);
        assert_eq!(s.players.iter().map(|p| p.hp).collect::<Vec<_>>()[..4], [100., 100., 100., 100.], "none of them is standing on it");
        // On it.
        s.players[3].p = [0.1, TOP, 3.25];
        s.heat_griddles(0.5);
        assert!((100. - s.players[3].hp - 12.5).abs() < 1e-3, "{}", s.players[3].hp);
        assert_eq!(s.players[0].hp + s.players[1].hp + s.players[2].hp, 300.);
    }

    #[test]
    fn nobody_burns_behind_a_forcefield_at_a_card_table_or_after_the_round() {
        for stage in [Stage::Build, Stage::RoundOver] {
            let mut s = kitchen();
            s.stage = stage;
            (s.griddles[0].on, s.griddles[0].temp) = (true, HOT);
            place(&mut s, 1, [0., TOP, 3.25], 0.);
            s.heat_griddles(1.);
            assert_eq!(s.players[0].hp, 100., "{stage:?}");
        }
        let mut s = kitchen();
        s.stage = Stage::Fight;
        (s.griddles[0].on, s.griddles[0].temp) = (true, HOT);
        s.lay_table();
        place(&mut s, 1, [0., TOP, 3.25], 0.);
        s.heat_griddles(1.);
        assert_eq!(s.players[0].hp, 100., "a card table");
    }

    #[test]
    fn a_hotdog_that_burns_to_death_is_credited_to_whoever_last_switched_the_griddle_on() {
        let mut s = kitchen();
        s.stage = Stage::Fight;
        // 2 switches it on, 4 off and 3 on again: 3 it is (the bots stand where they are put).
        for (id, x) in [(2, -0.1), (4, 0.), (3, 0.1)] {
            place(&mut s, id, [x, FLOOR, 3.55], 0.);
            assert!(s.use_griddle(id), "{id}");
            s.tick(0.4);
        }
        assert_eq!((s.griddles[0].on, s.griddles[0].by), (true, 3));
        s.griddles[0].temp = HOT;
        place(&mut s, 1, [0., TOP, 3.25], 0.);
        let mut seconds = 0.;
        while s.players[0].alive && seconds < 10. {
            s.tick(1. / 30.);
            seconds += 1. / 30.;
        }
        let me = &s.players[0];
        assert!(!me.alive && me.hp == 0.);
        assert!((3.8..4.4).contains(&seconds), "100 HP at 25 a second: {seconds}");
        assert_eq!((me.last_damage.kind, me.last_damage.attacker), (feast::BURN, 3));
        assert!(me.last_damage.amount > 0.);
        // Switched off, a plate still hot burns the same and still credits them.
        let mut s = kitchen();
        s.stage = Stage::Fight;
        (s.griddles[0].on, s.griddles[0].by, s.griddles[0].temp) = (false, 2, 190.);
        place(&mut s, 1, [0., TOP, 3.25], 0.);
        s.tick(2.);
        assert!(s.players[0].hp < 60. && s.players[0].last_damage.attacker == 2);
    }

    #[test]
    fn a_hotdog_that_burns_on_a_griddle_it_switched_on_itself_is_credited_to_nobody() {
        let mut s = kitchen();
        s.stage = Stage::Fight;
        place(&mut s, 1, FRONT, 0.);
        assert!(s.use_griddle(1));
        s.griddles[0].temp = HOT;
        place(&mut s, 1, [0., TOP, 3.25], 0.);
        s.tick(5.);
        assert!(!s.players[0].alive);
        assert_eq!((s.players[0].last_damage.kind, s.players[0].last_damage.attacker), (feast::BURN, 0));
    }

    #[test]
    fn a_kill_by_burning_counts_in_free_for_all_for_whoever_switched_it_on() {
        let mut s = kitchen();
        s.set_rules(crate::rules::Rules::parse("mode=ffa"));
        // (Nobody else plays, so that the kill is the plate's.)
        s.players.iter_mut().for_each(|p| (p.shield, p.bot) = (0., false));
        s.griddles[0].on = true;
        (s.griddles[0].by, s.griddles[0].temp) = (3, HOT);
        place(&mut s, 1, [0., TOP, 3.25], 0.);
        s.tick(5.);
        assert!(s.players[0].deaths >= 1, "it died: {}", s.players[0].deaths);
        assert_eq!(s.players[2].kills, 1, "the hotdog that switched it on is credited with the kill");
        assert_eq!((s.players[0].last_damage.kind, s.players[0].last_damage.attacker), (feast::BURN, 3));
    }

    #[test]
    fn a_bot_on_a_hot_plate_walks_off_it_and_one_on_a_cold_plate_stays() {
        for (on, temp, leaves) in [(true, HOT, true), (false, 150., true), (true, ROOM, true), (false, 40., false), (false, ROOM, false)] {
            let mut s = kitchen();
            s.stage = Stage::Fight;
            (s.griddles[0].on, s.griddles[0].temp) = (on, temp);
            place(&mut s, 2, [0.05, TOP, 3.2], 0.);
            let plate = Plate::of(&s.level.map.blocks[0]);
            let mut seconds = 0.;
            while plate.under(s.players[1].p, s.players[1].grounded) && seconds < 2. {
                s.tick(1. / 30.);
                seconds += 1. / 30.;
            }
            assert_eq!(!plate.under(s.players[1].p, s.players[1].grounded), leaves, "on {on}, {temp} C: {:?} after {seconds} s", s.players[1].p);
            if leaves {
                assert!(seconds < 1., "a bot gets off in under a second: {seconds} s");
                assert!(s.players[1].hp > 85., "{} HP left", s.players[1].hp);
            }
        }
    }

    #[test]
    fn a_bot_takes_the_nearest_side_that_is_clear() {
        let plate = Plate { rect: [-0.25, 3.12, 0.25, 3.4], top: TOP, dial: [0., FLOOR + 0.03, 3.4], front: [0., 1.] };
        assert_eq!(plate.exit([0., TOP, 3.15], |_, _| true), [0., 3.12 - 0.05]);
        assert_eq!(plate.exit([0.2, TOP, 3.3], |_, _| true), [0.25 + 0.05, 3.3]);
        // With the nearest side shut, the next.
        assert_eq!(plate.exit([0., TOP, 3.15], |_, z| z > 3.1), [-0.25 - 0.05, 3.15]);
        assert_eq!(plate.exit([0.2, TOP, 3.3], |x, z| x < 0.3 || z > 3.4), [0.2, 3.4 + 0.05]);
    }

    #[test]
    fn the_server_tells_clients_the_states_and_a_client_sets_them() {
        let mut s = kitchen();
        s.griddles[0].on = true;
        (s.griddles[0].temp, s.griddles[0].by) = (187.5, 3);
        let told = s.griddle_states();
        let text = serde_json::to_string(&told).unwrap();
        assert_eq!(text, r#"[{"block":0,"on":true,"temp":187.5,"by":3}]"#);
        let mut client = kitchen();
        client.hear_griddles(&serde_json::from_str::<Vec<State>>(&text).unwrap());
        assert_eq!(client.griddle_states(), told);
        // A state for a block that is no griddle (another map's) is passed over.
        client.hear_griddles(&[State { block: 9, on: true, temp: 100., by: 1 }]);
        assert_eq!(client.griddle_states(), told);
    }

    #[test]
    fn a_new_round_puts_a_griddle_back_off_and_cold() {
        let mut s = kitchen();
        (s.griddles[0].on, s.griddles[0].temp, s.griddles[0].by) = (true, 150., 2);
        s.reset();
        assert_eq!((s.griddles[0].on, s.griddles[0].temp, s.griddles[0].by, s.griddles[0].flips), (false, ROOM, 0, 0));
    }
    /// The four Forts maps each have one griddle, off, across the middle by the front edge of the counter and clear of what
    /// the maps keep clear: the bases, the bacon, the pickups' pads, the starts, the spawns and the microwave.
    #[test]
    fn each_forts_map_has_one_griddle_in_the_middle_by_the_front_clear_of_everything() {
        for name in ["pillow-fort", "jungle-fort", "moonlit-fort", "the-whole-kitchen-forts"] {
            let map = forts(name);
            let s = Feast::with_map(map.clone(), false);
            assert_eq!(s.griddles.len(), 1, "{name}");
            assert_eq!((s.griddles[0].on, s.griddles[0].temp), (false, ROOM), "{name} starts off");
            let block = &map.blocks[s.griddles[0].block];
            let [x0, z0, x1, z1] = block.rect;
            assert_eq!(block.style, STYLE);
            assert!((x1 - x0 - 0.5).abs() < 1e-9 && (z1 - z0 - 0.28).abs() < 1e-9 && (block.height - 0.06).abs() < 1e-9, "{name}: 0.50 x 0.28 x 0.06");
            assert!((x0 + x1).abs() < 1e-9, "{name}: straddles the centre line");
            let fort = map.fortress().unwrap();
            assert!(z0 - map.bounds[1] < 0.6 && z0 > map.bounds[1] + 0.05, "{name}: near the front edge");
            // Clear of the bases, the bacon, the starts, the pickups and the spawns (with a hotdog's margin), the microwave and
            // the rest of the blocks, decks and ramps.
            let near = |p: [f64; 2], by: f64| p[0] > x0 - by && p[0] < x1 + by && p[1] > z0 - by && p[1] < z1 + by;
            let overlap = |r: [f64; 4], by: f64| r[0] < x1 + by && r[2] > x0 - by && r[1] < z1 + by && r[3] > z0 - by;
            for base in fort.bases {
                assert!(!overlap(base, 0.), "{name}: a base");
            }
            assert!(!near(fort.bacon, 0.1), "{name}: the bacon");
            assert!(map.starts.iter().all(|s| !near(s.p, 0.15)) && map.pickups.iter().all(|p| !near(p.p, 0.15)) && map.spawns.iter().all(|p| !near(p.p, 0.15)), "{name}");
            for (i, other) in map.blocks.iter().enumerate().filter(|(i, _)| *i != s.griddles[0].block) {
                assert!(!overlap(other.rect, 0.04), "{name}: block {i} ({})", other.style);
            }
            assert!(map.decks.iter().all(|d| !overlap(d.rect, 0.04)) && map.ramps.iter().all(|r| !overlap(r.rect, 0.04)), "{name}: decks and ramps");
            // Loose objects: none within a hotdog's width of it.
            for prop in &map.props {
                if let crate::map::Prop::Box { rect, .. } = prop {
                    assert!(!overlap(*rect, 0.02), "{name}");
                }
            }
            for loose in &map.loose {
                assert!(!near(loose.p, 0.12), "{name}: a {} at {:?}", loose.kind, loose.p);
            }
            // Its dial is on the side towards the middle of the counter, and the middle of the forcefield is no closer than 0.2.
            let plate = Plate::of(block);
            assert!(plate.dial[0].abs() < 1e-6, "{name}: the dial is on the centre line");
            // A hotdog can stand where it can use it, on the dial's side, between the griddle and the rest of the counter.
            let level = Level::new(map.clone());
            let ahead = [plate.dial[0] + plate.front[0] * 0.12, plate.dial[2] + plate.front[1] * 0.12];
            assert!(level.clear(ahead[0] as f64, ahead[1] as f64), "{name}: room to stand at the dial");
            let spot = [ahead[0], level.ground(ahead[0] as f64, ahead[1] as f64) as f32, ahead[1]];
            let look = plate.front[0].atan2(plate.front[1]);
            assert!(plate.within(spot, look), "{name}");
            // It stops a walker (it is a block) and a plain jump clears it.
            assert!(!level.clear(((x0 + x1) / 2.) as f64, ((z0 + z1) / 2.) as f64), "{name}: a block");
            assert!(0.06 > crate::map::STEP && 0.06 < crate::actor::JUMP * crate::actor::JUMP / (2. * crate::actor::GRAVITY), "{name}: too tall to step onto, low enough to jump onto");
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Draping: a napkin thrown or put over other loose objects becomes a wall (in single player, the client's
//! prediction and the server alike).
//!
//! A napkin (map.rs LOOSE kind NAPKIN) that comes to rest with its middle RAISED or more above the counter, over
//! other loose objects, drapes: it and the loose objects under it (whose middles lie under its outline and which do
//! not stand out above it) are frozen where they are (props.rs: `State::Draped`), and the level gains a wall in their
//! place, a block over the outline of what it covers grown by SKIRT on each side, up to the napkin's top (a hair
//! under it, so that the top can be stood on). Nobody walks, shoves, shoots or blasts through it, and routes go round
//! it. Bodies standing in the new wall's way are moved out of it, to the nearest side. A drape is refused (the napkin
//! stays a loose napkin on top) when it would reach into a team's base (Forts: nothing may be put there, see
//! build.rs), when it would cover the bacon's spot or a zombie spawn point, when a body in its way cannot be moved out,
//! and (Forts) when it would leave either team's base with no way on foot to the bacon: a sealed base must not make the
//! bacon uncapturable. A refused napkin is tried again once it has woken and come to rest.
//!
//! Q at a draped wall pulls its napkin off (into the hotdog's hands) and what was under it is loose again: anyone may,
//! at any time, in every mode (in Forts during the fight as well, and either team's wall). Bots pull one only in a
//! Forts fight, when draped walls have cut them off from where they are going (forts.rs: `breach`).

use crate::actor;
use crate::feast::{Feast, Stage, CAT};
use crate::map::{Block, Level, FLOOR, LIP, MARGIN};
use crate::props::State;
use serde::{Deserialize, Serialize};

/// The kind of loose object that drapes: a napkin (a tea towel's size), in map.rs LOOSE's order.
pub const NAPKIN: usize = 3;
/// How far above the counter a napkin's middle must rest to drape, and how far its wall reaches beyond what it covers.
pub const RAISED: f64 = 0.03;
pub const SKIRT: f64 = 0.01;
/// How far a napkin must reach past the edge of a raised surface it lies on to hang over it (and drape there): a
/// block's, a deck's or a ramp's, and the counter's own edge (a napkin hanging a little way over that is only lying
/// near the edge).
pub const HANG: f64 = 0.06;
pub const HANG_COUNTER: f64 = 0.18;

/// A draped napkin: its number among the props, the props it covers, the outline of its wall [x0, z0, x1, z1] and
/// the height of the napkin's top.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Drape {
    pub id: u32,
    #[serde(alias = "blanket")]
    pub napkin: usize,
    pub props: Vec<usize>,
    pub rect: [f64; 4],
    pub top: f64,
}
impl Drape {
    /// The drape's wall as a block of the level: its top a hair under the napkin's, so that feet on the napkin
    /// are on the block.
    pub fn block(&self) -> Block {
        Block { rect: self.rect, height: self.top - FLOOR - LIP - 0.001, style: "cloth".into(), facing: None }
    }
}

/// What a napkin would drape over (`Feast::canopy`): the loose objects it covers (none, over a block's or a deck's
/// edge), the outline of its wall [x0, z0, x1, z1] and the height of its top.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Canopy {
    pub props: Vec<usize>,
    pub rect: [f64; 4],
    pub top: f64,
}
/// Why a napkin that would drape does not (`Feast::refusal`): it stays a loose napkin where it lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Forts: it would lie in a team's base, where the bacon is scored (nothing may be put there: `Feast::placement`).
    Base,
    /// Its wall would cover the bacon's spot or a zombie spawn point.
    Spot,
    /// A body standing in its wall's way could not be moved out of it.
    Stuck,
    /// Forts: it would leave a team's base with no way on foot to the bacon.
    Sealed,
}
/// The rotation matrix of a quaternion [x, y, z, w], row by row.
fn matrix(q: [f64; 4]) -> [[f64; 3]; 3] {
    let [x, y, z, w] = q;
    [
        [1. - 2. * (y * y + z * z), 2. * (x * y - w * z), 2. * (x * z + w * y)],
        [2. * (x * y + w * z), 1. - 2. * (x * x + z * z), 2. * (y * z - w * x)],
        [2. * (x * z - w * y), 2. * (y * z + w * x), 1. - 2. * (x * x + y * y)],
    ]
}

/// The map's level with more blocks standing in it, after the map's own (draped walls, the forcefield): its `map`
/// is the map itself, without them.
pub fn walled(level: &Level, blocks: Vec<Block>) -> Level {
    let mut map = level.map.clone();
    map.blocks.extend(blocks);
    let mut walled = Level::new(map);
    (walled.map, walled.fort) = (level.map.clone(), level.fort.clone());
    walled
}

impl Feast {
    /// The level as it stands: the map's, with a wall for every drape and (Forts) the forcefield while it is up,
    /// last. Without drapes the map's own (or the fielded one) is used.
    pub fn relevel(&mut self) {
        self.bodies.extra = self.extra_walls();
        let up = self.fort.as_ref().is_some_and(|fort| fort.up);
        self.level = match (&self.fort, &self.base, self.drapes.is_empty()) {
            (Some(fort), _, true) => if up { fort.walled.clone() } else { fort.open.clone() },
            (None, Some(base), true) => base.clone(),
            (None, None, true) => return,
            (_, _, false) => {
                let field = self.fort.as_ref().filter(|fort| fort.up).map(|fort| fort.field.clone());
                walled(&self.level, self.drapes.iter().map(Drape::block).chain(field).collect())
            }
        };
    }
    /// The walls of the level's blocks beyond the map's own (the drapes', then the forcefield's while it is up), as
    /// the bodies meet them: each grown by the margin, with the height that clears it.
    pub fn extra_walls(&self) -> Vec<[f64; 5]> {
        let field = self.fort.as_ref().filter(|fort| fort.up).map(|fort| fort.field.clone());
        self.drapes
            .iter()
            .map(Drape::block)
            .chain(field)
            .map(|b| [b.rect[0] - MARGIN, b.rect[1] - MARGIN, b.rect[2] + MARGIN, b.rect[3] + MARGIN, FLOOR + (b.height + LIP)])
            .collect()
    }
    /// Network play: the drapes the server has, with their walls, in this copy of the round (the props' states come
    /// with the props).
    pub fn set_drapes(&mut self, drapes: Vec<Drape>) {
        if drapes == self.drapes {
            return;
        }
        let first = self.walls_from();
        for k in (0..self.drapes.len()).rev() {
            self.props.unwall(first + k);
        }
        self.drapes = drapes;
        for (k, d) in self.drapes.iter().enumerate() {
            self.props.wall(&self.level, d.rect, FLOOR + d.block().height, first + k);
        }
        self.relevel();
    }
    /// Where the drapes' walls start among the props' blocks: after the map's own.
    fn walls_from(&self) -> usize {
        self.level.map.blocks.len()
    }
    /// Every napkin that has come to rest is looked at once, until it wakes again.
    pub(crate) fn drape(&mut self) {
        for id in 0..self.props.len() {
            if self.props.body(id).shape != NAPKIN {
                continue;
            }
            if !matches!(self.props.state(id), State::Asleep | State::Locked) {
                self.refused.remove(&id);
                continue;
            }
            if !self.refused.contains(&id) && !self.tent(id) {
                self.refused.insert(id);
            }
        }
    }
    /// What the napkin `b` would drape over, lying as it is (`pose` None) or with its origin at `pose`'s point turned
    /// by its yaw (a napkin about to be put down: feast.rs `placement`); None where it would not drape. Outlines and
    /// tops are those of the props' sample spheres. It drapes over:
    ///   - the loose objects whose middles lie under its outline and which do not stand out above it, when its middle
    ///     is RAISED or more above the ground under it (they stand in for its wall, as they always have);
    ///   - a raised surface it lies on and hangs HANG or more over the edge of: a block, a deck, a ramp, an earlier
    ///     drape's wall, or (HANG_COUNTER) the counter itself where the napkin hangs over its edge. Its cloth hangs down the side, and the hanging part
    ///     is a strip of wall SKIRT thick against that side (on the counter's edge, the strip is the counter's last
    ///     SKIRT), up to the napkin's top: what is behind it is covered.
    /// Both at once, the wall is one block over all of it.
    pub(crate) fn canopy(&self, b: usize, pose: Option<([f64; 3], f64)>) -> Option<Canopy> {
        let shape = self.props.shape(self.props.body(b).shape);
        let (bx, o) = (shape.aabb, shape.origin);
        // Its frame: the centre of mass and the rotation (world = c + r * local).
        let (at, q) = match pose {
            Some((p, yaw)) => (p, crate::props::yaw_rotation(yaw)),
            None => (self.props.position(b), self.props.rotation(b)),
        };
        let r = matrix(q);
        let turn = |v: [f64; 3]| [0, 1, 2].map(|i| r[i][0] * v[0] + r[i][1] * v[1] + r[i][2] * v[2]);
        let lo = turn(o);
        let c = [at[0] - lo[0], at[1] - lo[1], at[2] - lo[2]];
        let local = |p: [f64; 3]| {
            let d = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
            [0, 1, 2].map(|i| r[0][i] * d[0] + r[1][i] * d[1] + r[2][i] * d[2])
        };
        // Its own outline and heights: [x0, z0, x1, z1, lowest, highest], from the corners of its box.
        let own = match pose {
            None => self.props.extent(b),
            Some(_) => {
                let mut e = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
                for k in 0..8 {
                    let v = turn([bx[if k & 1 == 0 { 0 } else { 3 }], bx[if k & 2 == 0 { 1 } else { 4 }], bx[if k & 4 == 0 { 2 } else { 5 }]]);
                    let w = [c[0] + v[0], c[1] + v[1], c[2] + v[2]];
                    (e[0], e[1], e[2], e[3], e[4], e[5]) = (e[0].min(w[0]), e[1].min(w[2]), e[2].max(w[0]), e[3].max(w[2]), e[4].min(w[1]), e[5].max(w[1]));
                }
                e
            }
        };
        // The loose objects under it.
        let reach = (bx[3] - bx[0]).hypot(bx[5] - bx[2]) / 2. + 0.2;
        let mut covered: Vec<(usize, [f64; 6])> = vec![];
        for p in 0..self.props.len() {
            if p == b || !matches!(self.props.state(p), State::Awake | State::Asleep | State::Locked) {
                continue;
            }
            let there = self.props.position(p);
            if (there[0] - at[0]).hypot(there[2] - at[2]) > reach {
                continue;
            }
            let ([lx, _, lz], e) = (local(there), self.props.extent(p));
            if lx >= bx[0] && lx <= bx[3] && lz >= bx[2] && lz <= bx[5] && e[5] <= own[5] + 0.005 && e[4] < own[4] {
                covered.push((p, e));
            }
        }
        // The raised surfaces it lies on, and the sides of them it hangs over.
        let (x0, z0, x1, z1) = (self.level.x0, self.level.z0, self.level.x1, self.level.z1);
        let walls: Vec<[f64; 5]> = self.level.map.blocks.iter().chain(self.drapes.iter().map(Drape::block).collect::<Vec<_>>().iter()).map(|k| [k.rect[0], k.rect[1], k.rect[2], k.rect[3], FLOOR + k.height]).collect();
        let surface = |x: f64, z: f64| {
            if x < x0 || x > x1 || z < z0 || z > z1 {
                return f64::NEG_INFINITY;
            }
            walls.iter().filter(|w| x >= w[0] && x <= w[2] && z >= w[1] && z <= w[3]).fold(self.level.ground(x, z), |top, w| top.max(w[4]))
        };
        // (The bottom of the napkin over a point: its underside's plane.)
        let (up, foot) = (turn([0., 1., 0.]), turn([0., bx[1], 0.]));
        let under = |x: f64, z: f64| c[1] + foot[1] - (up[0] * (x - c[0] - foot[0]) + up[2] * (z - c[2] - foot[2])) / up[1].max(1e-6);
        let mut raised: Vec<([f64; 4], f64, bool)> = walls.iter().map(|w| ([w[0], w[1], w[2], w[3]], w[4], false)).collect();
        raised.extend(self.level.map.decks.iter().map(|d| (d.rect, FLOOR + d.height, false)));
        raised.extend(self.level.map.ramps.iter().map(|d| (d.rect, f64::NAN, false)));
        raised.push(([x0, z0, x1, z1], FLOOR, true));
        let mut strips: Vec<[f64; 4]> = vec![];
        for (rect, top, counter) in raised {
            let (ox0, oz0, ox1, oz1) = (rect[0].max(own[0]), rect[1].max(own[1]), rect[2].min(own[2]), rect[3].min(own[3]));
            if ox0 >= ox1 || oz0 >= oz1 {
                continue;
            }
            let (mx, mz) = ((ox0 + ox1) / 2., (oz0 + oz1) / 2.);
            // A ramp's top is where the napkin lies on it.
            let top = if top.is_nan() { self.level.ground(mx, mz) } else { top };
            if (under(mx, mz) - top).abs() > 0.015 {
                continue;
            }
            let (thick, hang) = if counter { (-SKIRT, HANG_COUNTER) } else { (SKIRT, HANG) };
            for (beyond, side, a, b, out) in [
                (own[0] < rect[0] - hang, rect[0], oz0, oz1, [-1., 0.]),
                (own[2] > rect[2] + hang, rect[2], oz0, oz1, [1., 0.]),
                (own[1] < rect[1] - hang, rect[1], ox0, ox1, [0., -1.]),
                (own[3] > rect[3] + hang, rect[3], ox0, ox1, [0., 1.]),
            ] {
                let mid = (a + b) / 2.;
                let (px, pz) = if out[0] != 0. { (side + out[0] * 0.015, mid) } else { (mid, side + out[1] * 0.015) };
                if !beyond || surface(px, pz) > top - RAISED {
                    continue;
                }
                let (near, far) = (side, side + (if out[0] + out[1] > 0. { thick } else { -thick }));
                let (n0, n1) = (near.min(far), near.max(far));
                strips.push(if out[0] != 0. { [n0, a, n1, b] } else { [a, n0, b, n1] });
            }
        }
        let middle = at[1] - self.level.ground(at[0], at[2]) >= RAISED;
        if !(middle && !covered.is_empty()) && strips.is_empty() {
            return None;
        }
        let low = |i: usize| covered.iter().map(|(_, e)| e[i] - SKIRT).chain(strips.iter().map(|s| s[i])).fold(f64::INFINITY, f64::min);
        let high = |i: usize| covered.iter().map(|(_, e)| e[i] + SKIRT).chain(strips.iter().map(|s| s[i])).fold(f64::NEG_INFINITY, f64::max);
        let rect = [low(0).max(x0), low(1).max(z0), high(2).min(x1), high(3).min(z1)];
        Some(Canopy { props: covered.iter().map(|(p, _)| *p).collect(), rect, top: own[5] })
    }
    /// Whether a drape over `canopy` would be refused, and if not, the bodies standing in its wall's way and where each
    /// is moved to (out by its nearest side). `bodies` are those bodies: the round's own, or a client's copy of them.
    /// The refusals, in this order: (Forts) its wall would reach into a team's base (`Refusal::Base`); its wall would
    /// cover the bacon's spot or a zombie spawn point (`Refusal::Spot`); a
    /// body in its way cannot be moved out (`Refusal::Stuck`); in Forts it would leave either team's base with no way
    /// on foot to the bacon, with the field down (`Refusal::Sealed`). Nothing is changed: `tent` drapes by it, and the
    /// client asks it of the drape that a napkin about to be put down would make (`Placement::drape`), so that the
    /// preview never shows a wall that the landing then refuses. It builds the level with the new wall (a navigation
    /// grid) once for the reachability in Forts, and once more only when a body stands in the wall's way.
    pub fn clearing(&self, canopy: &Canopy, bodies: &actor::World) -> Result<Vec<(u32, [f64; 2])>, Refusal> {
        let rect = canopy.rect;
        if self.in_a_base(&[[rect[0], rect[1]], [rect[2], rect[1]], [rect[2], rect[3]], [rect[0], rect[3]]]) {
            return Err(Refusal::Base);
        }
        let block = Drape { rect, top: canopy.top, ..Drape::default() }.block();
        let inside = |x: f64, z: f64| x >= rect[0] - MARGIN && x <= rect[2] + MARGIN && z >= rect[1] - MARGIN && z <= rect[3] + MARGIN;
        let bacon = self.level.fort.as_ref().map(|fort| fort.bacon);
        if self.level.map.spawns.iter().any(|spawn| inside(spawn.p[0], spawn.p[1])) || bacon.is_some_and(|p| inside(p[0], p[1])) {
            return Err(Refusal::Spot);
        }
        let field = self.fort.as_ref().filter(|fort| fort.up).map(|fort| fort.field.clone());
        let walls: Vec<Block> = self.drapes.iter().map(Drape::block).chain([block.clone()]).collect();
        // Bodies in the wall's way go out of it by its nearest side, onto the counter there; one that cannot, and
        // the napkin stays as it is. (The level with the wall and the field is built for the first of them.)
        let mut level: Option<Level> = None;
        let mut moves = vec![];
        for a in &bodies.actors {
            if a.fallen || !inside(a.p[0], a.p[2]) || a.p[1] >= FLOOR + block.height + LIP {
                continue;
            }
            let level = level.get_or_insert_with(|| walled(&self.level, walls.iter().cloned().chain(field.clone()).collect()));
            let mut out: Vec<(f64, [f64; 2])> = [[rect[0] - MARGIN - 1e-4, a.p[2]], [rect[2] + MARGIN + 1e-4, a.p[2]], [a.p[0], rect[1] - MARGIN - 1e-4], [a.p[0], rect[3] + MARGIN + 1e-4]]
                .into_iter()
                .map(|q| ((q[0] - a.p[0]).hypot(q[1] - a.p[2]), q))
                .collect();
            out.sort_by(|u, v| u.0.total_cmp(&v.0));
            let Some((_, q)) = out.into_iter().find(|(_, q)| level.walkable(q[0], level.ground(q[0], q[1]), q[1])) else {
                return Err(Refusal::Stuck);
            };
            moves.push((a.id, q));
        }
        // Forts: both teams keep a way on foot between their base and the bacon (with the field down).
        if self.fort.is_some() && !reachable(&walled(&self.level, walls)) {
            return Err(Refusal::Sealed);
        }
        Ok(moves)
    }
    /// Why a drape over `canopy` would be refused now, with `bodies` in the way (`clearing`), or None when it would
    /// stand.
    pub fn refusal(&self, canopy: &Canopy, bodies: &actor::World) -> Option<Refusal> {
        self.clearing(canopy, bodies).err()
    }
    /// Whether the napkin `b` drapes, and if it does, the drape (`canopy`, unless `clearing` refuses it).
    fn tent(&mut self, b: usize) -> bool {
        let Some(canopy) = self.canopy(b, None) else {
            return false;
        };
        let Ok(moves) = self.clearing(&canopy, &self.bodies) else {
            return false;
        };
        let Canopy { props: covered, rect, top } = canopy;
        let drape = Drape { id: self.next_drape, napkin: b, props: covered, rect, top };
        let block = drape.block();
        self.next_drape += 1;
        self.props.freeze(b);
        for c in &drape.props {
            self.props.freeze(*c);
        }
        let at = self.walls_from() + self.drapes.len();
        self.props.wall(&self.level, rect, FLOOR + block.height, at);
        self.drapes.push(drape);
        self.relevel();
        for (id, [x, z]) in moves {
            let ground = self.level.ground(x, z);
            let Some(body) = self.bodies.get_mut(id) else {
                continue;
            };
            (body.p[0], body.p[2]) = (x, z);
            body.p[1] = body.p[1].max(ground);
            let p = body.p.map(|v| v as f32);
            if id < CAT {
                if let Some(q) = self.player_mut(id) {
                    q.p = p;
                }
            } else if let Some(e) = self.enemies.iter_mut().find(|e| CAT + e.id == id) {
                e.p = p;
            }
            self.echo.insert(id, p);
        }
        true
    }
    /// Whether a hotdog may pull a draped napkin off with Q: anybody a person plays, at any time, in every mode. (A
    /// Forts bot that has been walled off pulls one by `pull`, never by grabbing at something near it.)
    pub fn may_undrape(&self, id: u32) -> bool {
        self.player(id).is_some_and(|p| !p.bot)
    }
    /// Pulls the drape `drape` off into the hotdog's hands, as Q at it does.
    pub fn pull(&mut self, id: u32, drape: u32) -> bool {
        let Some(p) = self.player(id) else {
            return false;
        };
        if !p.alive || p.carry >= 0 || self.stage == Stage::RoundOver || self.fort.as_ref().is_some_and(|fort| fort.bacon.holder == id) {
            return false;
        }
        let Some(napkin) = self.drapes.iter().find(|d| d.id == drape).map(|d| d.napkin) else {
            return false;
        };
        self.undrape(drape);
        if !self.props.pick_up(napkin, id as i32) {
            return false;
        }
        if let Some(p) = self.player_mut(id) {
            (p.carry, p.weapon, p.pose) = (napkin as i32, 0, crate::feast::build::Pose::default());
        }
        self.haul();
        true
    }
    /// The drape comes off: its wall goes, what was under it is loose again where it lay, and the napkin is loose
    /// too (`grab` puts it in the hands of whoever pulled it).
    pub fn undrape(&mut self, drape: u32) -> bool {
        let Some(k) = self.drapes.iter().position(|d| d.id == drape) else {
            return false;
        };
        let d = self.drapes.remove(k);
        self.props.unwall(self.walls_from() + k);
        for c in &d.props {
            self.props.thaw(*c);
        }
        self.props.thaw(d.napkin);
        self.refused.remove(&d.napkin);
        self.relevel();
        true
    }
}

/// Forts: whether each team's base is joined by a way on foot to the bacon on `level`.
fn reachable(level: &Level) -> bool {
    let Some(fort) = &level.fort else {
        return true;
    };
    let bacon = fort.bacon;
    if !level.clear(bacon[0], bacon[1]) {
        return false;
    }
    fort.bases.iter().all(|&[x0, z0, x1, z1]| {
        let mut z = z0 + level.cell / 2.;
        while z < z1 {
            let mut x = x0 + level.cell / 2.;
            while x < x1 {
                if level.joined([x, z], bacon) {
                    return true;
                }
                x += level.cell;
            }
            z += level.cell;
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Map;
    use crate::props::yaw_rotation;

    /// A counter 2 m by 1 with a block 10 cm high (from x = 0.5 to 1.0), a crate standing 5 cm off its +x side, and a
    /// napkin out of the way.
    fn counter() -> Feast {
        let map = Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "name": "Edge", "bounds": [0, 0, 2, 1], "starts": [{"p": [0.1, 0.1]}, {"p": [1.9, 0.9]}], "spawns": [{"p": [1.9, 0.1]}],
            "blocks": [{"rect": [0.5, 0.3, 1, 0.7], "height": 0.1}],
            "props": [{"kind": "crate", "p": [1.1, 0.5], "base": 0, "yaw": 0}, {"kind": "napkin", "p": [1.5, 0.85], "base": 0, "yaw": 0}]}"#,
        )
        .unwrap();
        let mut s = Feast::with_map(map, false);
        for (i, p) in s.players.iter_mut().enumerate() {
            (p.bot, p.p) = (false, [0.1 + 0.08 * i as f32, crate::feast::FLOOR, 0.1]);
        }
        s.warmup = f32::INFINITY;
        s
    }
    /// The napkin laid with its middle at [x, z], and left to come to rest.
    fn lay(s: &mut Feast, x: f64, z: f64, base: f64) {
        s.props.set_pose(1, [x, FLOOR + base + 0.0005, z], yaw_rotation(0.));
        for _ in 0..60 {
            s.tick(1. / 30.);
        }
    }

    /// Laid on the block with a good part of it over the block's +x side (and nothing under that part), the napkin
    /// drapes: it is frozen where it lies, covering no loose object, and its hanging side is a strip of wall SKIRT thick
    /// against the block's face, up to the napkin's top, which a round from that side meets.
    #[test]
    fn a_napkin_over_a_blocks_edge_drapes_and_its_hanging_side_is_a_wall() {
        let mut s = counter();
        s.props.set_pose(0, [1.6, FLOOR, 0.2], yaw_rotation(0.));
        lay(&mut s, 0.85, 0.5, 0.1);
        assert_eq!(s.drapes.len(), 1, "it drapes");
        let d = &s.drapes[0];
        assert!(d.props.is_empty(), "over no loose object: {:?}", d.props);
        let want = [1., 0.3, 1. + SKIRT, 0.7];
        assert!((0..4).all(|k| (d.rect[k] - want[k]).abs() < 1e-6), "a strip against the block's face: {:?}", d.rect);
        assert!((d.top - (FLOOR + 0.1 + 0.008)).abs() < 0.002, "up to its top: {}", d.top);
        assert_eq!(s.props.state(1), State::Draped);
        // A round from beyond the hanging side stops at the strip, not at the block.
        let o = glam::Vec3::new(1.3, crate::feast::FLOOR + 0.05, 0.5);
        assert!((s.level.cover_from(o, glam::Vec3::NEG_X) - (0.3 - SKIRT as f32)).abs() < 1e-3, "{}", s.level.cover_from(o, glam::Vec3::NEG_X));
        // On the bare counter it only lies there.
        let mut s = counter();
        s.props.set_pose(0, [1.6, FLOOR, 0.2], yaw_rotation(0.));
        lay(&mut s, 1.5, 0.5, 0.);
        assert!(s.drapes.is_empty());
    }

    /// Laid over the block's edge and the crate beside it, the napkin drapes over both: one wall from the block's face
    /// past the crate, over the gap between them, up to the napkin's top; the crate is frozen under it as ever.
    #[test]
    fn a_napkin_over_a_block_and_a_crate_beside_it_drapes_over_both_as_one_wall() {
        let mut s = counter();
        lay(&mut s, 1.0, 0.5, 0.1);
        assert_eq!(s.drapes.len(), 1, "it drapes");
        let d = s.drapes[0].clone();
        assert_eq!(d.props, [0], "the crate is under it");
        assert_eq!((s.props.state(0), s.props.state(1)), (State::Draped, State::Draped));
        assert!((d.rect[0] - 1.).abs() < 1e-6 && (d.rect[2] - (1.15 + SKIRT)).abs() < 0.003, "from the block's face past the crate: {:?}", d.rect);
        assert!(d.rect[1] <= 0.3 + 1e-6 && d.rect[3] >= 0.7 - 1e-6, "{:?}", d.rect);
        // Nobody walks into the gap: the wall stands over it.
        assert!(!s.level.walkable(1.025, FLOOR, 0.5));
        // Pulled off, the crate is loose again where it stood.
        assert!(s.undrape(d.id));
        assert_eq!(s.props.state(0), State::Awake);
    }
}

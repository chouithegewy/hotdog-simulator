// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Napkins as cloth, on screen. In the simulations a napkin is a rigid pad (kitchen_sim::props), and that
//! pad stays the authority for everything that matters: where it is, what it hits, who carries it, where it
//! drapes. What is drawn is this: a sheet of COLS × ROWS particles (144), joined to their neighbours, to the
//! next but one (so that it resists bending a little) and across each square (so that it keeps its shape),
//! moved by Verlet integration in fixed steps of SUBSTEP seconds and held to its lengths by a few passes of
//! position constraints. It is driven by the pad it stands for:
//!
//! * **Carried**, two particles of the edge nearest the carrier are pinned to the carrier's hands (in first
//!   person, to two points low in the view), and the rest hangs, sways, trails and lands on the counter.
//! * **Loose** (thrown, dropped, lying about), the few particles round its middle are pulled to where the pad
//!   is and turned as it is, and in the air every particle is pulled gently towards its place on the pad; the
//!   rest falls under gravity against air that resists motion across the sheet (so that it billows and
//!   flutters as it falls), and settles over whatever it lands on: the counter, a deck, a block's edge, other
//!   loose objects.
//! * **Draped**, it is not shown: `loose::cloth` lays the drape's sheet over its wall instead.
//!
//! The sheet collides with the counter and what stands on it (the map's decks and ramps through the level's
//! ground, its blocks, the draped walls and the forcefield), with the other loose objects' own solids (the
//! same part lists the physics uses: rounded boxes, and the bowl's lathed shell) and with the hotdogs'
//! bodies, and it falls off the counter's edge to the kitchen floor. Contact is with friction, so it comes to
//! rest; once every particle has been still for SLEEP seconds and its pad is asleep, it sleeps, costing
//! nothing, until its pad moves or something comes near. It is drawn as a quilt TUCK thick, both faces
//! seen, in the red and white gingham of a kitchen napkin.
use bevy::prelude::*;
use kitchen_sim::props::Part;

/// Particles across the napkin's width and down its depth (45 cm each way): 4.1 cm apart.
pub const COLS: usize = 12;
pub const ROWS: usize = 12;
pub const COUNT: usize = COLS * ROWS;
/// Seconds a step of the cloth takes, and at most how many a frame takes (a slow frame is caught up no further).
pub const SUBSTEP: f32 = 1. / 120.;
const MOST: usize = 4;
/// Passes over the constraints in a step.
const PASSES: usize = 4;
/// How far a particle keeps from what it touches (the cloth's half thickness as it collides), and half the
/// thickness it is drawn with.
pub const RADIUS: f32 = 0.006;
const TUCK: f32 = 0.0032;
/// The cloth's gravity (twice the bodies', kitchen_sim::actor::GRAVITY: under theirs a heap of cloth would stand
/// up on its own stiffness, and swing like a slow pendulum), the share of its speed a particle loses each step,
/// the share of its speed across the sheet the air takes each step, and how much of its sliding speed a
/// particle in contact keeps.
const GRAVITY: f32 = 5.6;
const DAMP: f32 = 0.006;
const AIR: f32 = 0.05;
const GRIP: f32 = 0.25;
/// Stiffness of the constraints along the sheet, across its squares and from one particle to the next but
/// one.
const STRETCH: f32 = 1.;
const SHEAR: f32 = 0.3;
const BEND: f32 = 0.035;
/// Loose: the particles within MIDDLE of the sheet's middle are pulled this share of the way to their place on
/// the pad each step; in the air every particle is pulled SHAPE of the way.
const MIDDLE: f32 = 0.05;
const HOLD: f32 = 0.3;
const SHAPE: f32 = 0.015;
/// Static friction: a particle lying on something does not creep along it by less than STICK metres a step.
const STICK: f32 = 0.0012;
/// A particle that meets the ground from more than SIDE below its top meets it from the side: it is a wall there.
const SIDE: f32 = 0.015;
/// A hotdog walks over cloth lying lower than this above its feet; what hangs higher it pushes aside.
const STEPPED: f32 = 0.03;
/// A grip moves to its hand at most REACH metres a step (so that a napkin picked up off the counter is lifted,
/// not whipped), and no particle moves further than FASTEST metres a step (3 m/s).
const REACH: f32 = 0.012;
const FASTEST: f32 = 0.025;
/// A step in which no particle moves further than QUIET metres is a still one; SLEEP seconds of them and a
/// sleeping pad, and the sheet sleeps.
const QUIET: f32 = 8e-5;
const SLEEP: f32 = 0.6;

/// Where the sheet's particle `i` lies on the pad, in the pad's own frame (its origin is the middle of the
/// pad's underside): a grid over the pad's outline, half way up its thickness.
pub fn rest(i: usize, size: [f32; 2]) -> Vec3 {
    let (c, r) = (i % COLS, i / COLS);
    Vec3::new((c as f32 / (COLS - 1) as f32 - 0.5) * size[0], 0.004, (r as f32 / (ROWS - 1) as f32 - 0.5) * size[1])
}

/// The weave of the napkins' cloth, the drapes' and the loose ones', as a colour to multiply its gingham by (the
/// cloth's own picture, loose.rs `art::check`): a little lighter and darker here and there, as cotton lies. `u` and `v`
/// are metres along the cloth.
pub fn weave(u: f32, v: f32) -> [f32; 4] {
    let lie = 0.94 + 0.06 * ((u * 37.).sin() * (v * 29.).sin() * 0.5 + 0.5);
    [lie, lie, lie, 1.]
}

/// What the sheet is told each frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drive {
    /// Hidden: draped (the drape's own sheet is drawn), or out of the round.
    Hidden,
    /// Held by its two grips, in the world.
    Carried([Vec3; 2]),
    /// Loose, where its pad is (origin and turn); `resting` while the pad sleeps.
    Loose { at: Vec3, turn: Quat, resting: bool },
}

/// One of the other loose objects, as the sheet meets it: its parts (kitchen_sim::props::ShapeSpec's), where
/// its origin is, how it is turned, and the radius of a sphere round its origin that holds it.
pub struct Solid<'a> {
    pub parts: &'a [Part],
    pub at: Vec3,
    pub turn: Quat,
    pub reach: f32,
    /// Moving (awake, or carried): a sleeping sheet it comes near wakes.
    pub moving: bool,
}
/// A hotdog's body: an upright capsule standing on `feet`.
pub struct Body {
    pub feet: Vec3,
    pub radius: f32,
    pub height: f32,
}
/// Everything the sheet can touch this frame.
pub struct Surroundings<'a> {
    /// The counter's surface at [x, z] (the level's ground: decks and ramps included), on the counter.
    pub ground: &'a dyn Fn(f32, f32) -> f32,
    /// The counter's outline [x0, z0, x1, z1]: beyond it is the kitchen floor, `floor`.
    pub bounds: [f32; 4],
    pub floor: f32,
    /// Blocks standing on it: [x0, z0, x1, z1, top] (the map's, the drapes' walls, the forcefield).
    pub blocks: Vec<[f32; 5]>,
    pub solids: Vec<Solid<'a>>,
    pub bodies: Vec<Body>,
}

#[derive(Clone, Copy)]
struct Link {
    a: u16,
    b: u16,
    length: f32,
    stiff: f32,
}

/// One napkin's sheet.
#[derive(Clone)]
pub struct Cloth {
    /// The particles where they are, and where they were a step ago.
    pub pos: Vec<Vec3>,
    prev: Vec<Vec3>,
    /// Their places on the pad (see `rest`).
    home: Vec<Vec3>,
    links: Vec<Link>,
    /// The pinned particles while carried (the edge nearest the carrier), and those held to the pad when loose.
    grips: [usize; 2],
    middle: Vec<usize>,
    /// Seconds the sheet has been still, whether it sleeps, and the time it has not yet stepped.
    still: f32,
    pub asleep: bool,
    owed: f32,
    /// What drove it last, to tell whether its pad has moved.
    last: Option<Drive>,
    /// Steps taken, for the checks.
    pub steps: u64,
}

impl Cloth {
    /// A sheet the size of the pad (`size`, its width and depth) lying where `drive` has it.
    pub fn new(size: [f32; 2], drive: Drive) -> Cloth {
        let home: Vec<Vec3> = (0..COUNT).map(|i| rest(i, size)).collect();
        let mut links = vec![];
        let index = |c: usize, r: usize| (r * COLS + c) as u16;
        let mut join = |a: u16, b: u16, stiff: f32| links.push(Link { a, b, length: home[a as usize].distance(home[b as usize]), stiff });
        for r in 0..ROWS {
            for c in 0..COLS {
                if c + 1 < COLS {
                    join(index(c, r), index(c + 1, r), STRETCH);
                }
                if r + 1 < ROWS {
                    join(index(c, r), index(c, r + 1), STRETCH);
                }
                if c + 1 < COLS && r + 1 < ROWS {
                    join(index(c, r), index(c + 1, r + 1), SHEAR);
                    join(index(c + 1, r), index(c, r + 1), SHEAR);
                }
                if c + 2 < COLS {
                    join(index(c, r), index(c + 2, r), BEND);
                }
                if r + 2 < ROWS {
                    join(index(c, r), index(c, r + 2), BEND);
                }
            }
        }
        // Held by the edge nearest the carrier (the pad's +z edge: it is carried with its -z ahead), at two
        // points a third of the way in from each corner, so that the corners hang.
        let near = (ROWS - 1) * COLS;
        let grips = [near + COLS / 3, near + COLS - 1 - COLS / 3];
        let middle = (0..COUNT).filter(|i| Vec2::new(home[*i].x, home[*i].z).length() <= MIDDLE + 1e-4).collect();
        let mut cloth = Cloth { pos: vec![], prev: vec![], home, links, grips, middle, still: 0., asleep: false, owed: 0., last: None, steps: 0 };
        cloth.lay(drive);
        cloth
    }
    /// Puts every particle where the pad has it (flat, its places on the pad, a little gathered so that it
    /// lies in soft folds rather than as a board), at rest.
    pub fn lay(&mut self, drive: Drive) {
        let (at, turn) = match drive {
            Drive::Loose { at, turn, .. } => (at, turn),
            Drive::Carried(grips) => ((grips[0] + grips[1]) / 2. - Vec3::Y * 0.1, Quat::IDENTITY),
            Drive::Hidden => (Vec3::ZERO, Quat::IDENTITY),
        };
        // Laid in soft folds: gathered a little and lifted into a few uneven waves running different ways (the
        // length the waves take up is about what the gathering takes off, so that nothing is stretched); where
        // the pad lies decides their phases, so that no two napkins lie alike.
        let seed = (at.x * 12.9898 + at.z * 78.233).sin() * 43758.547;
        let phase = |k: f32| (seed * k).fract() * std::f32::consts::TAU;
        self.pos = self
            .home
            .iter()
            .map(|h| {
                let waves = (h.x * (27. + 6. * phase(3.).sin()) + h.z * 5. + phase(1.)).sin() + 0.55 * (h.z * 21. - h.x * 7. + phase(2.)).sin();
                at + turn * Vec3::new(h.x * 0.92, h.y + 0.011 * (waves * 0.65 + 1.), h.z * 0.95)
            })
            .collect();
        self.prev = self.pos.clone();
        (self.still, self.asleep, self.owed, self.last) = (0., false, 0., Some(drive));
    }
    /// Whether the sheet has no reason to wake: it sleeps, what drives it has not moved, and nothing moving
    /// or alive is near it.
    fn undisturbed(&self, drive: Drive, around: &Surroundings) -> bool {
        let moved = match (self.last, drive) {
            (Some(Drive::Loose { at, turn, .. }), Drive::Loose { at: now, turn: then, resting }) => !resting || at.distance(now) > 1e-4 || turn.dot(then).abs() < 0.99999,
            _ => true,
        };
        if moved {
            return false;
        }
        let (low, high) = self.bounds();
        let near = |p: Vec3, reach: f32| p.x > low.x - reach && p.x < high.x + reach && p.z > low.z - reach && p.z < high.z + reach && p.y < high.y + reach;
        // (A hotdog walking over cloth lying on the counter does not touch it: see STEPPED.)
        !around.solids.iter().any(|s| s.moving && near(s.at, s.reach + RADIUS)) && !around.bodies.iter().any(|b| near(b.feet, b.radius + RADIUS) && b.feet.y + STEPPED < high.y)
    }
    /// The box round every particle.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.pos.iter().fold((Vec3::MAX, Vec3::MIN), |(low, high), p| (low.min(*p), high.max(*p)))
    }
    /// One frame of `dt` seconds: the steps it owes, unless it sleeps undisturbed. Returns whether it moved.
    pub fn update(&mut self, drive: Drive, around: &Surroundings, dt: f32) -> bool {
        if drive == Drive::Hidden {
            self.last = Some(drive);
            return false;
        }
        // Back from hidden (pulled off a drape into somebody's hands, say), or far from its pad: laid afresh. Just put
        // down out of the hands (onto a block's top, say, with the cloth still hanging below it), it is laid afresh where
        // the pad landed unless its middle is there already, and falls over what is there from that.
        let put = matches!(self.last, Some(Drive::Carried(_))) && matches!(drive, Drive::Loose { .. });
        let lost = match drive {
            Drive::Loose { at, .. } => self.pos[self.middle[0]].distance(at) > if put { 0.05 } else { 0.6 },
            Drive::Carried(grips) => self.pos[self.grips[0]].distance(grips[0]) > 0.6,
            Drive::Hidden => false,
        };
        if self.last == Some(Drive::Hidden) || lost {
            self.lay(drive);
        }
        if self.asleep && self.undisturbed(drive, around) {
            return false;
        }
        self.asleep = false;
        self.owed = (self.owed + dt.max(0.)).min(SUBSTEP * MOST as f32);
        let mut moved = false;
        while self.owed >= SUBSTEP {
            self.owed -= SUBSTEP;
            self.step(drive, around);
            moved = true;
        }
        self.last = Some(drive);
        moved
    }
    /// One step of SUBSTEP seconds.
    pub fn step(&mut self, drive: Drive, around: &Surroundings) {
        self.steps += 1;
        let dt = SUBSTEP;
        let (carried, grips) = (matches!(drive, Drive::Carried(_)), self.grips);
        let pinned = move |i: usize| carried && (i == grips[0] || i == grips[1]);
        let normals = self.normals();
        let fall = Vec3::NEG_Y * GRAVITY * dt * dt;
        let mut fastest = 0f32;
        for i in 0..COUNT {
            let (p, q) = (self.pos[i], self.prev[i]);
            let mut v = (p - q) * (1. - DAMP);
            // The air resists motion across the sheet, not along it: a sheet falling flat billows, and one
            // waved flutters.
            let n = normals[i];
            v -= n * v.dot(n) * AIR;
            let v = v.clamp_length_max(FASTEST);
            self.prev[i] = p;
            self.pos[i] = p + v + fall;
        }
        match drive {
            Drive::Carried(hands) => {
                for (k, g) in self.grips.iter().enumerate() {
                    let at = self.prev[*g];
                    self.pos[*g] = at + (hands[k] - at).clamp_length_max(REACH);
                }
            }
            Drive::Loose { at, turn, resting } => {
                for i in &self.middle {
                    let (target, now) = (at + turn * self.home[*i], self.pos[*i]);
                    self.pos[*i] = now + (target - now) * HOLD;
                }
                // In the air it keeps something of the pad's shape; at rest it is left to lie as it falls.
                if !resting {
                    for i in 0..COUNT {
                        let (target, now) = (at + turn * self.home[i], self.pos[i]);
                        self.pos[i] = now + (target - now) * SHAPE;
                    }
                }
            }
            Drive::Hidden => {}
        }
        for _ in 0..PASSES {
            for link in &self.links {
                let (a, b) = (link.a as usize, link.b as usize);
                let d = self.pos[b] - self.pos[a];
                let length = d.length();
                if length < 1e-9 {
                    continue;
                }
                let (fa, fb) = match (pinned(a), pinned(b)) {
                    (true, true) => continue,
                    (true, false) => (0., 1.),
                    (false, true) => (1., 0.),
                    _ => (0.5, 0.5),
                };
                let shift = d * ((length - link.length) / length * link.stiff);
                self.pos[a] += shift * fa;
                self.pos[b] -= shift * fb;
            }
        }
        for i in 0..COUNT {
            if pinned(i) {
                // A grip pressed into the counter (looking down) lies on it.
                let at = self.pos[i];
                let ground = self.surface(at, around);
                self.pos[i].y = at.y.max(ground + RADIUS);
                continue;
            }
            self.collide(i, around);
            fastest = fastest.max(self.pos[i].distance(self.prev[i]));
        }
        let resting = matches!(drive, Drive::Loose { resting: true, .. });
        self.still = if fastest < QUIET && resting { self.still + dt } else { 0. };
        if self.still >= SLEEP {
            self.asleep = true;
            self.prev.clone_from(&self.pos);
        }
    }
    /// The surface under a point: the counter's (with its decks and ramps), or beyond its edge the kitchen
    /// floor.
    fn surface(&self, p: Vec3, around: &Surroundings) -> f32 {
        let [x0, z0, x1, z1] = around.bounds;
        if p.x >= x0 && p.x <= x1 && p.z >= z0 && p.z <= z1 { (around.ground)(p.x, p.z) } else { around.floor }
    }
    /// Keeps particle `i` out of everything solid, with friction where it touches.
    fn collide(&mut self, i: usize, around: &Surroundings) {
        let mut p = self.pos[i];
        let mut touched: Option<Vec3> = None;
        let push = |p: &mut Vec3, touched: &mut Option<Vec3>, by: Vec3| {
            *p += by;
            *touched = Some(touched.map_or(by, |t| t + by));
        };
        let ground = self.surface(p, around);
        if p.y < ground + RADIUS {
            let was = self.prev[i];
            if was.y < ground + RADIUS - SIDE {
                // It came at this ground from below its top, from the side: there it is a wall (the counter's
                // edge seen from the kitchen, a deck's side), and it goes back out the way it came.
                let by = Vec3::new(was.x - p.x, 0., was.z - p.z);
                push(&mut p, &mut touched, by);
                let under = self.surface(p, around);
                if p.y < under + RADIUS {
                    let by = Vec3::Y * (under + RADIUS - p.y);
                    push(&mut p, &mut touched, by);
                }
            } else {
                let by = Vec3::Y * (ground + RADIUS - p.y);
                push(&mut p, &mut touched, by);
            }
        }
        for b in &around.blocks {
            let (x0, z0, x1, z1, top) = (b[0] - RADIUS, b[1] - RADIUS, b[2] + RADIUS, b[3] + RADIUS, b[4] + RADIUS);
            if p.x <= x0 || p.x >= x1 || p.z <= z0 || p.z >= z1 || p.y >= top {
                continue;
            }
            // Out by the face it came in through (a thin wall, such as the forcefield, is not passed through), or
            // failing that by the nearest: the top, or a side.
            let was = self.prev[i];
            let ways = [(top - p.y, Vec3::Y, was.y >= top), (p.x - x0, Vec3::NEG_X, was.x <= x0), (x1 - p.x, Vec3::X, was.x >= x1), (p.z - z0, Vec3::NEG_Z, was.z <= z0), (z1 - p.z, Vec3::Z, was.z >= z1)];
            let entered = ways.iter().filter(|w| w.2).fold(None, |best: Option<(f32, Vec3)>, w| if best.is_none_or(|b| w.0 < b.0) { Some((w.0, w.1)) } else { best });
            let (depth, way) = entered.unwrap_or_else(|| ways.iter().map(|w| (w.0, w.1)).fold((f32::MAX, Vec3::Y), |best, w| if w.0 < best.0 { w } else { best }));
            push(&mut p, &mut touched, way * depth);
        }
        for s in &around.solids {
            if p.distance_squared(s.at) > (s.reach + RADIUS) * (s.reach + RADIUS) {
                continue;
            }
            let back = s.turn.inverse();
            for part in s.parts {
                let local = back * (p - s.at);
                if let Some((depth, normal)) = inside(part, local) {
                    push(&mut p, &mut touched, s.turn * normal * depth);
                }
            }
        }
        for body in &around.bodies {
            let (dx, dz) = (p.x - body.feet.x, p.z - body.feet.z);
            let reach = body.radius + RADIUS;
            let far = dx.hypot(dz);
            // Cloth lying on the counter is walked over (the feet step on it), not shoved along: only what hangs
            // higher than STEPPED is kept out of a body.
            if far >= reach || p.y <= body.feet.y + STEPPED || p.y >= body.feet.y + body.height {
                continue;
            }
            let out = if far > 1e-6 { Vec3::new(dx / far, 0., dz / far) } else { Vec3::X };
            push(&mut p, &mut touched, out * (reach - far));
        }
        if let Some(by) = touched {
            // Whatever it was pushed out of, it no longer moves into it, and it slides along it only a little;
            // resting on something, a particle that would creep along it less than STICK in a step stays put
            // (so that a napkin keeps its folds, and lies still).
            let n = by.normalize_or_zero();
            if n.y > 0.7 {
                let was = self.prev[i];
                let creep = Vec3::new(p.x - was.x, 0., p.z - was.z);
                let length = creep.length();
                if length < STICK {
                    (p.x, p.z) = (was.x, was.z);
                } else {
                    p -= creep * (STICK / length);
                }
            }
            let mut v = p - self.prev[i];
            let into = v.dot(n);
            if into < 0. {
                v -= n * into;
            }
            let along = v - n * v.dot(n);
            v -= along * (1. - GRIP);
            self.prev[i] = p - v;
        }
        self.pos[i] = p;
    }
    /// Each particle's normal: the way the sheet faces there (its +y on the pad), from its neighbours.
    pub fn normals(&self) -> Vec<Vec3> {
        (0..COUNT)
            .map(|i| {
                let (c, r) = (i % COLS, i / COLS);
                let (left, right) = (if c > 0 { i - 1 } else { i }, if c + 1 < COLS { i + 1 } else { i });
                let (back, front) = (if r > 0 { i - COLS } else { i }, if r + 1 < ROWS { i + COLS } else { i });
                (self.pos[front] - self.pos[back]).cross(self.pos[right] - self.pos[left]).normalize_or(Vec3::Y)
            })
            .collect()
    }
    /// The sheet as it is drawn: the particles' grid made twice as fine by Catmull-Rom curves through them
    /// (so that a fold is round, not a crease), as a quilt: a top and an underside TUCK either side of it, and a
    /// band round the edge between them. Positions and normals only; the mesh's other attributes are `mesh`'s.
    pub fn surfaces(&self) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
        let fine = smooth(&self.pos);
        let normals = grid_normals(&fine, FINE[0], FINE[1]);
        let mut positions = Vec::with_capacity(VERTICES);
        let mut out = Vec::with_capacity(VERTICES);
        for side in [1f32, -1.] {
            for (p, n) in fine.iter().zip(&normals) {
                positions.push((*p + *n * TUCK * side).to_array());
                out.push((*n * side).to_array());
            }
        }
        for i in rim() {
            let inward = fine[inward(i)] - fine[i];
            let outward = -(inward - normals[i] * inward.dot(normals[i])).normalize_or(normals[i]);
            for side in [1f32, -1.] {
                positions.push((fine[i] + normals[i] * TUCK * side).to_array());
                out.push(outward.to_array());
            }
        }
        (positions, out)
    }
}

/// The drawn grid: twice as fine as the particles' each way.
const FINE: [usize; 2] = [2 * COLS - 1, 2 * ROWS - 1];
const RIM: usize = 2 * (FINE[0] - 1) + 2 * (FINE[1] - 1);
const VERTICES: usize = 2 * FINE[0] * FINE[1] + 2 * RIM;
/// A grid of points (COLS × ROWS) made twice as fine each way by Catmull-Rom curves through it: the points
/// themselves, and between each two the point the curve through them and their neighbours passes half way.
fn smooth(points: &[Vec3]) -> Vec<Vec3> {
    // Half way along a curve through a, b, c, d, between b and c; at an end the missing neighbour is
    // continued straight.
    let half = |a: Option<Vec3>, b: Vec3, c: Vec3, d: Option<Vec3>| {
        let (a, d) = (a.unwrap_or(2. * b - c), d.unwrap_or(2. * c - b));
        (-a + 9. * b + 9. * c - d) / 16.
    };
    // Along the rows first, then down the columns.
    let mut rows = vec![Vec3::ZERO; FINE[0] * ROWS];
    for r in 0..ROWS {
        let at = |c: isize| (c >= 0 && c < COLS as isize).then(|| points[r * COLS + c as usize]);
        for c in 0..COLS {
            rows[r * FINE[0] + 2 * c] = points[r * COLS + c];
            if c + 1 < COLS {
                rows[r * FINE[0] + 2 * c + 1] = half(at(c as isize - 1), points[r * COLS + c], points[r * COLS + c + 1], at(c as isize + 2));
            }
        }
    }
    let mut fine = vec![Vec3::ZERO; FINE[0] * FINE[1]];
    for c in 0..FINE[0] {
        let at = |r: isize| (r >= 0 && r < ROWS as isize).then(|| rows[r as usize * FINE[0] + c]);
        for r in 0..ROWS {
            fine[2 * r * FINE[0] + c] = rows[r * FINE[0] + c];
            if r + 1 < ROWS {
                fine[(2 * r + 1) * FINE[0] + c] = half(at(r as isize - 1), rows[r * FINE[0] + c], rows[(r + 1) * FINE[0] + c], at(r as isize + 2));
            }
        }
    }
    fine
}
/// Each point's normal on a grid of `cols` × `rows` points: the way it faces there, from its neighbours.
fn grid_normals(points: &[Vec3], cols: usize, rows: usize) -> Vec<Vec3> {
    (0..cols * rows)
        .map(|i| {
            let (c, r) = (i % cols, i / cols);
            let (left, right) = (if c > 0 { i - 1 } else { i }, if c + 1 < cols { i + 1 } else { i });
            let (back, front) = (if r > 0 { i - cols } else { i }, if r + 1 < rows { i + cols } else { i });
            (points[front] - points[back]).cross(points[right] - points[left]).normalize_or(Vec3::Y)
        })
        .collect()
}
/// The drawn grid's points round its edge, in order round it.
fn rim() -> Vec<usize> {
    let [cols, rows] = FINE;
    let mut edge = Vec::with_capacity(RIM);
    edge.extend(0..cols - 1);
    edge.extend((0..rows - 1).map(|r| r * cols + cols - 1));
    edge.extend((1..cols).rev().map(|c| (rows - 1) * cols + c));
    edge.extend((1..rows).rev().map(|r| r * cols));
    edge
}
/// The drawn grid's point next to an edge point, one in towards the middle.
fn inward(i: usize) -> usize {
    let [cols, rows] = FINE;
    let (c, r) = ((i % cols).clamp(1, cols - 2), (i / cols).clamp(1, rows - 2));
    r * cols + c
}

/// How deep a point (in the frame of the part's shape) lies inside one part of a loose object, grown by the
/// sheet's RADIUS, and the way out; None when it is clear of it. A box is rounded by its radius as the
/// physics has it; a lathe is a shell of its profile turned round the y axis.
fn inside(part: &Part, p: Vec3) -> Option<(f32, Vec3)> {
    match part {
        Part::Box { center, half, radius, .. } => {
            let (c, r) = (Vec3::new(center[0] as f32, center[1] as f32, center[2] as f32), *radius as f32);
            let core = Vec3::new(half[0] as f32, half[1] as f32, half[2] as f32) - Vec3::splat(r);
            let l = p - c;
            let q = l.abs() - core;
            let outside = q.max(Vec3::ZERO);
            let (distance, normal) = if outside.length_squared() > 0. {
                (outside.length() - r, (outside * l.signum()).normalize_or(Vec3::Y))
            } else {
                // Inside the core: out by the nearest face.
                let (axis, most) = if q.x >= q.y && q.x >= q.z { (0, q.x) } else if q.y >= q.z { (1, q.y) } else { (2, q.z) };
                let mut n = Vec3::ZERO;
                n[axis] = if l[axis] < 0. { -1. } else { 1. };
                (most - r, n)
            };
            (distance < RADIUS).then(|| (RADIUS - distance, normal))
        }
        Part::Lathe { center, profile, radius, .. } => {
            let l = p - Vec3::new(center[0] as f32, center[1] as f32, center[2] as f32);
            let rho = l.x.hypot(l.z);
            let (mut best, mut way) = (f32::MAX, Vec2::Y);
            for pair in profile.windows(2) {
                let (a, b) = (Vec2::new(pair[0][0] as f32, pair[0][1] as f32), Vec2::new(pair[1][0] as f32, pair[1][1] as f32));
                let e = b - a;
                let t = ((Vec2::new(rho, l.y) - a).dot(e) / e.length_squared()).clamp(0., 1.);
                let d = Vec2::new(rho, l.y) - (a + e * t);
                if d.length() < best {
                    (best, way) = (d.length(), d.normalize_or(Vec2::Y));
                }
            }
            let distance = best - *radius as f32;
            let around = if rho > 1e-6 { Vec2::new(l.x, l.z) / rho } else { Vec2::X };
            (distance < RADIUS).then(|| (RADIUS - distance, Vec3::new(way.x * around.x, way.y, way.x * around.y)))
        }
    }
}

/// A sheet's mesh, laid out as `Cloth::surfaces` fills it: its triangles (the top, the underside wound the
/// other way, the band round the edge), its weave and hem (from where each point lies on the pad, so that the pattern
/// moves with the cloth) and its texture coordinates.
pub fn mesh(cloth: &Cloth) -> Mesh {
    use bevy::{asset::RenderAssetUsages, mesh::{Indices, PrimitiveTopology}};
    let (positions, normals) = cloth.surfaces();
    let [cols, rows] = FINE.map(|n| n as u32);
    let mut indices: Vec<u32> = vec![];
    for (side, offset) in [(1, 0u32), (-1, cols * rows)] {
        for r in 0..rows - 1 {
            for c in 0..cols - 1 {
                let (a, b, d, e) = (offset + r * cols + c, offset + r * cols + c + 1, offset + (r + 1) * cols + c + 1, offset + (r + 1) * cols + c);
                // The top faces +y on the pad: wound counter-clockwise seen from there.
                if side > 0 { indices.extend([a, e, d, a, d, b]) } else { indices.extend([a, d, e, a, b, d]) }
            }
        }
    }
    let band = 2 * cols * rows;
    for k in 0..RIM as u32 {
        let next = (k + 1) % RIM as u32;
        let (a, b, d, e) = (band + 2 * k, band + 2 * next, band + 2 * next + 1, band + 2 * k + 1);
        indices.extend([a, b, d, a, d, e]);
    }
    // Where each drawn point lies on the pad; its hem, a band round its edge, is a little darker.
    let home = smooth(&cloth.home);
    let reach = home.iter().fold(Vec2::ZERO, |m, h| m.max(Vec2::new(h.x.abs(), h.z.abs())));
    let hem = |h: Vec3| if reach.x - h.x.abs() < 0.012 || reach.y - h.z.abs() < 0.012 { 0.8 } else { 1. };
    let (mut colours, mut uvs) = (vec![], vec![]);
    for _ in 0..2 {
        for h in &home {
            colours.push(weave(h.x, h.z).map(|v| v * hem(*h)).map(|v| v.min(1.)));
            uvs.push([h.x * 8., h.z * 8.]);
        }
    }
    for i in rim() {
        let shade = weave(home[i].x, home[i].z).map(|v| v * 0.7);
        for _ in 0..2 {
            colours.push([shade[0], shade[1], shade[2], 1.]);
            uvs.push([home[i].x * 8., home[i].z * 8.]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
        .with_inserted_indices(Indices::U32(indices))
}
/// Puts the sheet's new shape into its mesh.
pub fn reshape(cloth: &Cloth, mesh: &mut Mesh) {
    let (positions, normals) = cloth.surfaces();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::props::ShapeSpec;

    const FLOOR: f32 = 0.9025;
    const SIZE: [f32; 2] = [0.45, 0.45];
    fn flat(_: f32, _: f32) -> f32 {
        FLOOR
    }
    fn counter<'a>(ground: &'a dyn Fn(f32, f32) -> f32) -> Surroundings<'a> {
        Surroundings { ground, bounds: [-2., -2., 2., 2.], floor: 0., blocks: vec![], solids: vec![], bodies: vec![] }
    }
    fn run(cloth: &mut Cloth, drive: impl Fn(u32) -> Drive, around: &Surroundings, frames: u32) {
        for f in 0..frames {
            cloth.update(drive(f), around, 1. / 60.);
            assert!(cloth.pos.iter().all(|p| p.is_finite()), "frame {f}: the sheet stays finite");
        }
    }
    fn lowest(cloth: &Cloth) -> f32 {
        cloth.bounds().0.y
    }
    #[test]
    fn a_loose_napkin_settles_on_the_counter_in_its_outline_and_sleeps() {
        let ground = flat;
        let around = counter(&ground);
        let at = Vec3::new(0., FLOOR, 0.);
        let drive = Drive::Loose { at, turn: Quat::IDENTITY, resting: true };
        let mut cloth = Cloth::new(SIZE, drive);
        run(&mut cloth, |_| drive, &around, 240);
        assert!(cloth.asleep, "it went to sleep");
        assert!(lowest(&cloth) >= FLOOR + RADIUS - 1e-4, "never under the counter: {}", lowest(&cloth));
        // Soft folds: not a board. Some of it stands up off the counter in ridges.
        let row: Vec<f32> = (0..COLS).map(|c| cloth.pos[(ROWS / 2) * COLS + c].y - FLOOR - RADIUS).collect();
        let raised = cloth.pos.iter().filter(|p| p.y > FLOOR + RADIUS + 0.003).count();
        assert!(raised > COUNT / 20, "folds: {raised} particles raised; across the middle {row:?}");
        let (low, high) = cloth.bounds();
        assert!(high.y < FLOOR + 0.03, "it lies flat-ish: {high}");
        assert!((high.x - low.x) > 0.38 && (high.z - low.z) > 0.38, "spread out, not bunched: {low} {high}");
        // Asleep, it costs nothing.
        let steps = cloth.steps;
        run(&mut cloth, |_| drive, &around, 60);
        assert_eq!(cloth.steps, steps);
        // Its pad moved: it wakes and follows.
        let moved = Drive::Loose { at: at + Vec3::X * 0.1, turn: Quat::IDENTITY, resting: false };
        run(&mut cloth, |_| moved, &around, 5);
        assert!(!cloth.asleep && cloth.steps > steps);
    }
    #[test]
    fn a_carried_napkin_keeps_its_grips_in_the_hands_and_hangs_down_onto_the_counter_without_sinking_into_it() {
        let ground = flat;
        let around = counter(&ground);
        let hands = |f: u32| {
            // Walking forward and turning.
            let t = f as f32 / 60.;
            let at = Vec3::new(t * 0.4, FLOOR + 0.08, (t * 2.).sin() * 0.05);
            Drive::Carried([at + Vec3::new(0., 0., -0.03), at + Vec3::new(0., 0., 0.03)])
        };
        let mut cloth = Cloth::new(SIZE, hands(0));
        for f in 0..300 {
            let drive = hands(f);
            cloth.update(drive, &around, 1. / 60.);
            let Drive::Carried(grips) = drive else { unreachable!() };
            // Lifted to the hands within half a second, and held there.
            assert!(f < 30 || (cloth.pos[cloth.grips[0]].distance(grips[0]) < 1e-4 && cloth.pos[cloth.grips[1]].distance(grips[1]) < 1e-4), "frame {f}: held");
            assert!(cloth.pos.iter().all(|p| p.is_finite() && p.y >= FLOOR + RADIUS - 1e-3), "frame {f}: above the counter, lowest {}", lowest(&cloth));
        }
        assert!(!cloth.asleep, "carried, it never sleeps");
        // It hangs: most of it below the hands, some of it down on the counter.
        let hands_y = FLOOR + 0.08;
        assert!(cloth.pos.iter().filter(|p| p.y < hands_y - 0.02).count() > COUNT / 2);
        assert!(cloth.pos.iter().any(|p| p.y < FLOOR + RADIUS + 0.004), "some of it lies on the counter");
        // No link is stretched to breaking.
        assert!(cloth.links.iter().all(|l| cloth.pos[l.a as usize].distance(cloth.pos[l.b as usize]) < l.length * 1.6));
    }
    #[test]
    fn a_thrown_napkin_billows_as_it_falls_and_settles_over_a_crate_hanging_down_its_sides() {
        let ground = flat;
        let crate_spec = ShapeSpec::named("crate").unwrap();
        let mut around = counter(&ground);
        around.solids.push(Solid { parts: &crate_spec.parts, at: Vec3::new(0., FLOOR, 0.), turn: Quat::IDENTITY, reach: 0.14, moving: false });
        // The pad falls from 0.3 m onto the crate's top and stays there.
        let pad = |f: u32| {
            let t = (f as f32 / 60.).min(0.6);
            let y = (FLOOR + 0.4 - 0.5 * GRAVITY * t * t).max(FLOOR + 0.1);
            Drive::Loose { at: Vec3::new(0., y, 0.), turn: Quat::IDENTITY, resting: y <= FLOOR + 0.1 && f > 40 }
        };
        let mut cloth = Cloth::new(SIZE, pad(0));
        let mut billowed = false;
        for f in 0..400 {
            cloth.update(pad(f), &around, 1. / 60.);
            assert!(cloth.pos.iter().all(|p| p.is_finite() && p.y >= FLOOR + RADIUS - 1e-3));
            if f > 5 && f < 20 {
                // Falling, its edges lag behind its middle: it billows up.
                let middle = cloth.pos[(ROWS / 2) * COLS + COLS / 2].y;
                billowed |= cloth.pos[0].y > middle + 0.005 && cloth.pos[COUNT - 1].y > middle + 0.005;
            }
        }
        assert!(billowed, "it billowed as it fell");
        // It lies over the crate: its middle on top, its sides down to the counter round it.
        let middle = cloth.pos[(ROWS / 2) * COLS + COLS / 2];
        assert!(middle.y > FLOOR + 0.1 + RADIUS - 1e-3 && middle.y < FLOOR + 0.1 + RADIUS + 0.025, "middle on the crate (in a soft fold at most): {middle}");
        let row: Vec<String> = (0..COLS).map(|c| format!("{:.3}", cloth.pos[(ROWS / 2) * COLS + c].y - FLOOR)).collect();
        let column: Vec<String> = (0..ROWS).map(|r| format!("{:.3}", cloth.pos[r * COLS + COLS / 2].y - FLOOR)).collect();
        assert!(cloth.pos.iter().filter(|p| p.y < FLOOR + 0.03).count() > COUNT / 3, "its sides hang down to the counter: across {row:?}, down {column:?}");
        // Nothing of it is inside the crate.
        assert!(cloth.pos.iter().all(|p| !(p.x.abs() < 0.046 && p.z.abs() < 0.046 && p.y < FLOOR + 0.096)));
        assert!(cloth.asleep, "and it went to sleep");
    }
    #[test]
    fn a_napkin_off_the_counters_edge_falls_to_the_kitchen_floor_and_one_against_a_block_stays_out_of_it() {
        let ground = flat;
        let mut around = counter(&ground);
        around.bounds = [-2., -2., 0.1, 2.];
        around.blocks.push([-0.6, -0.1, -0.4, 0.1, FLOOR + 0.25]);
        let drive = Drive::Loose { at: Vec3::new(0., FLOOR, 0.), turn: Quat::IDENTITY, resting: true };
        let mut cloth = Cloth::new(SIZE, drive);
        run(&mut cloth, |_| drive, &around, 300);
        // Past the edge (x > 0.1) it hangs down; the counter keeps it up where it is under it.
        let row: Vec<String> = (0..COLS).map(|c| { let p = cloth.pos[(ROWS / 2) * COLS + c]; format!("({:.3},{:.3})", p.x, p.y - FLOOR) }).collect();
        assert!(cloth.pos.iter().any(|p| p.x > 0.1 && p.y < FLOOR - 0.05), "it hangs over the edge: {row:?}");
        assert!(cloth.pos.iter().filter(|p| p.x < 0.08).all(|p| p.y >= FLOOR + RADIUS - 1e-3));
        // A napkin laid against the block is pushed out of it.
        let drive = Drive::Loose { at: Vec3::new(-0.5, FLOOR + 0.3, 0.3), turn: Quat::IDENTITY, resting: false };
        let mut cloth = Cloth::new(SIZE, drive);
        run(&mut cloth, |_| drive, &around, 120);
        assert!(cloth.pos.iter().all(|p| !(p.x > -0.6 && p.x < -0.4 && p.z > -0.1 && p.z < 0.1 && p.y < FLOOR + 0.25)), "nothing inside the block");
    }
    #[test]
    fn a_napkin_carried_up_against_the_forcefield_stays_on_its_own_side_of_it() {
        let ground = flat;
        let mut around = counter(&ground);
        // The forcefield: a thin wall, 8 cm across, as tall as a block may be.
        around.blocks.push([-0.04, -2., 0.04, 2., FLOOR + 3.]);
        let hands = |f: u32| {
            // Walking at the field and pressing against it, swinging from side to side.
            let x = (-0.4 + f as f32 * 0.004).min(-0.09);
            let at = Vec3::new(x, FLOOR + 0.08, (f as f32 * 0.07).sin() * 0.1);
            Drive::Carried([at - Vec3::Z * 0.03, at + Vec3::Z * 0.03])
        };
        let mut cloth = Cloth::new(SIZE, hands(0));
        for f in 0..360 {
            cloth.update(hands(f), &around, 1. / 60.);
            assert!(cloth.pos.iter().all(|p| p.x < -0.04 + 1e-4), "frame {f}: through the field at {:?}", cloth.pos.iter().map(|p| p.x).fold(f32::MIN, f32::max));
        }
    }
    /// Every napkin of a Forts map moving at once (Countertop Fort has four): two carried about, four falling and
    /// landing over a crate. Their steps and their meshes together take well under 2 ms a frame (in a release
    /// build; the debug build is held to a loose figure).
    #[test]
    fn six_napkins_moving_at_once_cost_well_under_two_milliseconds_a_frame() {
        let ground = flat;
        let crate_spec = ShapeSpec::named("crate").unwrap();
        let mut around = counter(&ground);
        for k in 0..4 {
            around.solids.push(Solid { parts: &crate_spec.parts, at: Vec3::new(k as f32 * 0.8 - 1.2, FLOOR, 0.6), turn: Quat::IDENTITY, reach: 0.14, moving: false });
        }
        around.bodies.push(Body { feet: Vec3::new(0., FLOOR, -0.6), radius: 0.045, height: 0.15 });
        let drive = |k: usize, f: u32| {
            let t = f as f32 / 60.;
            if k < 2 {
                let at = Vec3::new(k as f32 - 0.5 + (t * 3.).sin() * 0.2, FLOOR + 0.08, -0.6 + t * 0.3);
                Drive::Carried([at - Vec3::X * 0.03, at + Vec3::X * 0.03])
            } else {
                let y = (FLOOR + 0.5 - 0.5 * GRAVITY * t * t).max(FLOOR + 0.1);
                Drive::Loose { at: Vec3::new((k - 2) as f32 * 0.8 - 1.2, y, 0.6), turn: Quat::from_rotation_y(t), resting: false }
            }
        };
        let mut cloths: Vec<Cloth> = (0..6).map(|k| Cloth::new(SIZE, drive(k, 0))).collect();
        let mut meshes: Vec<Mesh> = cloths.iter().map(mesh).collect();
        let mut times = vec![];
        for f in 0..120 {
            let from = std::time::Instant::now();
            for (k, (cloth, mesh)) in cloths.iter_mut().zip(meshes.iter_mut()).enumerate() {
                if cloth.update(drive(k, f), &around, 1. / 60.) {
                    reshape(cloth, mesh);
                }
            }
            times.push(from.elapsed().as_secs_f64() * 1e3);
        }
        assert!(cloths.iter().all(|c| c.pos.iter().all(|p| p.is_finite())));
        times.sort_by(f64::total_cmp);
        let (median, worst) = (times[60], times[119]);
        println!("six moving napkins: median {median:.3} ms a frame, worst {worst:.3} ms");
        let from = std::time::Instant::now();
        for _ in 0..100 {
            for (cloth, mesh) in cloths.iter().zip(meshes.iter_mut()) {
                reshape(cloth, mesh);
            }
        }
        println!("reshaping six meshes: {:.3} ms", from.elapsed().as_secs_f64() * 1e3 / 100.);
        let from = std::time::Instant::now();
        for _ in 0..100 {
            for (k, cloth) in cloths.iter_mut().enumerate() {
                cloth.step(drive(k, 100), &around);
            }
        }
        println!("one step of six: {:.3} ms", from.elapsed().as_secs_f64() * 1e3 / 100.);
        let budget = if cfg!(debug_assertions) { 60. } else { 2. } * kitchen_sim::timing_slack();
        assert!(median < budget, "median {median:.3} ms, worst {worst:.3} ms");
    }
    #[test]
    fn the_quilt_mesh_has_both_faces_and_follows_the_sheet() {
        let drive = Drive::Loose { at: Vec3::new(0., FLOOR, 0.), turn: Quat::IDENTITY, resting: true };
        let cloth = Cloth::new(SIZE, drive);
        let mesh = mesh(&cloth);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        assert_eq!(positions.len(), VERTICES);
        let Some(bevy::mesh::Indices::U32(indices)) = mesh.indices() else { panic!() };
        assert!(indices.iter().all(|i| (*i as usize) < positions.len()));
        assert_eq!(indices.len(), 2 * 6 * (FINE[0] - 1) * (FINE[1] - 1) + 6 * RIM);
        // The top faces up and the underside down.
        let face = |t: &[u32]| {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from_array(positions[i as usize]));
            (b - a).cross(c - a)
        };
        assert!(face(&indices[0..3]).y > 0. && face(&indices[6 * (FINE[0] - 1) * (FINE[1] - 1)..][..3]).y < 0.);
        // The drawn grid passes through the particles.
        let fine = smooth(&cloth.pos);
        assert!((0..COUNT).all(|i| fine[(i / COLS) * 2 * FINE[0] + (i % COLS) * 2].distance(cloth.pos[i]) < 1e-6));
    }
}

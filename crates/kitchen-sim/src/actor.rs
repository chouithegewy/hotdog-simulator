// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Character physics: a kinematic controller for the living hotdogs and zombies.
//!
//! Single player, the client's prediction and the server (crates/server) all run these same rules.
//!
//! A character is an upright capsule described by the position of its feet. Every step it is given the
//! velocity it wants (from the keyboard or the AI), an optional jump and its stance, and the world answers
//! with where it ends up:
//!
//! * against the level it obeys exactly the rule the game already has (`Level::can_step`): a stride that
//!   rule allows ends where it always did, and one it forbids slides along whatever stopped it;
//! * gravity acts whenever nothing is under the feet, so jumps are parabolas and walking off a deck is a fall;
//! * characters are solid to each other: they stop on contact, push each other in proportion to their
//!   masses, and never push anyone into a wall; one that comes down on another lands on its head and
//!   slides off;
//! * impulses (explosions, hits, heavy props) throw a character; it flies, slides to rest under friction,
//!   and is harder to steer until it has stopped.
//!
//! Everything is plain f64 arithmetic and square roots, in a fixed order, as in map.rs, so the JavaScript
//! implementation reproduces every position bit for bit. Do not introduce `hypot`, trigonometry,
//! exponentials or `mul_add` here: they are not guaranteed to round identically in the two languages.
//!
//! Units: metres, seconds, kilograms; impulses are newton seconds. y is up.

use crate::map::{FLOOR, LIP, Level, MARGIN, STEP};
use glam::{Vec2, Vec3};

// ----- Sizes, masses and speeds of the game's characters

/// Body radius; the same distance the level keeps a hotdog from blocks (MARGIN).
pub const RADIUS: f64 = 0.03;
/// Height of a standing character, of one crouching, and of one lying prone.
pub const HEIGHT: f64 = 0.15;
pub const CROUCH_HEIGHT: f64 = 0.10;
pub const PRONE_HEIGHT: f64 = 0.06;
/// Masses. Only their ratio matters between characters; the props core weighs a character at 0.4 kg too.
pub const HOTDOG_MASS: f64 = 0.4;
pub const ZOMBIE_MASS: f64 = 0.3;
/// Walking speeds the game uses today: a human player, a bot, a zombie (a cat); then the stim, crouch and prone
/// multipliers. The hotdogs once walked at WALK_BEFORE: everything walks FASTER times what it did, so a bot and a cat
/// keep their pace beside a person's (about 0.6 and 0.18 of it). Kung Fu keeps the walk it was balanced at (kungfu.rs).
pub const WALK: f64 = 1.25;
pub const BOT_WALK: f64 = 0.34 * FASTER;
pub const ZOMBIE_WALK: f64 = 0.10 * FASTER;
pub const WALK_BEFORE: f64 = 0.57;
pub const FASTER: f64 = WALK / WALK_BEFORE;
pub const STIM_PACE: f64 = 1.5;
pub const CROUCH_PACE: f64 = 0.65;
pub const PRONE_PACE: f64 = 0.4;
/// Take-off speeds of a jump, plain and stimmed. The stim jump reaches 0.194 m, as it did when stim had a
/// gravity of its own: `launch_speed(0.194, GRAVITY)`.
pub const JUMP: f64 = 0.8;
pub const STIM_JUMP: f64 = 1.0423;
/// The game's gravity (it gives the 0.8 m/s jump its familiar 0.114 m apex), and the real thing for comparison.
pub const GRAVITY: f64 = 2.8;
pub const EARTH_GRAVITY: f64 = 9.81;

// ----- Knockback tuning (suggested values; the game passes its own numbers to blast() and apply_impulse())

/// Impulse a relish glob delivers at its centre, the radius it reaches, and how far it tilts the throw upwards.
pub const BLAST_IMPULSE: f64 = 0.4;
pub const BLAST_RADIUS: f64 = 0.3;
pub const BLAST_LIFT: f64 = 0.35;
/// Impulse per point of damage for bullets and stabs: a 55 HP sniper round shoves a hotdog at 0.55 m/s.
pub const HIT_IMPULSE: f64 = 0.004;

// ----- Solver constants

/// A character's head is a ledge of its own: feet no more than this far below the top of another character
/// land on it instead of colliding with its side. It is less than a plain jump clears, so only a stim jump or
/// a drop from above gets there. Nobody stays up for long: a head is slippery, and whoever is on it slides
/// off at HEAD_SLIDE m/s.
pub const HEAD_STEP: f64 = 0.03;
pub const HEAD_SLIDE: f64 = 0.3;
/// The longest stride tested only at its end, as today's movement is. Longer moves are cut into strides.
const STRIDE: f64 = 0.05;
/// Halvings used to find how far along a refused move a character can still go: a stride / 1024, under 0.05 mm.
const HALVINGS: usize = 10;
/// A neighbour that will not give way is walked round: this share of the refused push becomes a step
/// sideways, so a queue behind a blocked zombie fans out instead of standing in line.
const SIDESTEP: f64 = 0.5;
/// Characters closer than this to touching count as touching, which absorbs rounding.
const SLACK: f64 = 1e-9;
/// Passes of the overlap repair that ends each step (it only has work after spawns, teleports and landings).
const ROUNDS: usize = 8;
/// Directions in which two characters standing on exactly the same spot are parted.
const SPREAD: [[f64; 2]; 4] = [[1., 0.], [0., 1.], [-1., 0.], [0., -1.]];

/// The settings of a world; `Config::default()` is the game's.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Downward acceleration, m/s².
    pub gravity: f64,
    /// Friction coefficient of a thrown character sliding on the ground: it slows by friction * gravity.
    pub friction: f64,
    /// Share of its wanted velocity a character keeps while thrown.
    pub control: f64,
    /// The counter edge. false: the edge is a wall for everyone, as it always was. true: walking still cannot
    /// leave the counter, but a thrown character can be carried over the edge; it falls to the kitchen floor
    /// and is reported `fallen`.
    pub open_edge: bool,
    /// Height of the kitchen floor a character falls to when it leaves the counter.
    pub kitchen_floor: f64,
    /// Longest time simulated in one go; step() cuts longer intervals into equal parts.
    pub max_dt: f64,
}
impl Default for Config {
    fn default() -> Config {
        Config { gravity: GRAVITY, friction: 0.9, control: 0.25, open_edge: false, kitchen_floor: 0., max_dt: 1. / 30. }
    }
}

/// A character. `p` is the position of its feet and `v` its velocity; set `wish`, `jump` and its stance
/// (`crouch`, `prone`) before each step and read the rest afterwards.
#[derive(Clone, Debug, PartialEq)]
pub struct Actor {
    pub id: u32,
    pub p: [f64; 3],
    /// Velocity over the last step. Only v[1] is carried from step to step; the rest is there to be read.
    pub v: [f64; 3],
    pub radius: f64,
    /// Standing height; a crouching character is CROUCH_HEIGHT tall and a prone one PRONE_HEIGHT.
    pub height: f64,
    pub mass: f64,
    // Input, set by the game before each step.
    /// Wanted horizontal velocity [x, z] in m/s, before the crouch or prone slow-down.
    pub wish: [f64; 2],
    /// Take-off speed of a jump to start this step, or 0. Cleared by step().
    pub jump: f64,
    pub prone: bool,
    /// Crouching: lower and slower than standing, taller and quicker than prone (which wins if both are
    /// set), and still able to jump.
    pub crouch: bool,
    // State, read by the game after each step.
    /// Standing on something.
    pub grounded: bool,
    /// Thrown by an impulse and not yet at rest: flying or sliding, with reduced control.
    pub launched: bool,
    /// Left the counter and reached the kitchen floor. A fallen character no longer moves or collides.
    pub fallen: bool,
    /// Horizontal velocity [x, z] left over from impulses; friction wears it away.
    pub knock: [f64; 2],
    /// Set while standing on another character's head: the direction [x, z] it is sliding off in.
    pub perch: Option<[f64; 2]>,
    /// What the props core last said: the height of a prop under the feet, or None.
    pub support: Option<f64>,
    /// Set while the character leans on a prop: [x, z] of the direction the props pushed it back in, then the
    /// spot [x, z] where that left it. Until it moves from that spot, other characters cannot push it further in.
    pub pin: Option<[f64; 4]>,
    /// Held still in the steps while this is set: it does not fall, fly or meet the props, though others may still
    /// push it. The server holds a person's hotdog in a step that applied none of its player's commands, so that it
    /// takes exactly one step for each command, as the player's own prediction does (crates/server game.rs).
    pub idle: bool,
}
impl Actor {
    /// A character of the usual size and a hotdog's mass, standing at `p`.
    pub fn new(id: u32, p: [f64; 3]) -> Actor {
        Actor {
            id,
            p,
            v: [0.; 3],
            radius: RADIUS,
            height: HEIGHT,
            mass: HOTDOG_MASS,
            wish: [0.; 2],
            jump: 0.,
            prone: false,
            crouch: false,
            grounded: true,
            launched: false,
            fallen: false,
            knock: [0.; 2],
            perch: None,
            support: None,
            pin: None,
            idle: false,
        }
    }
    pub fn hotdog(id: u32, p: [f64; 3]) -> Actor {
        Actor::new(id, p)
    }
    pub fn zombie(id: u32, p: [f64; 3]) -> Actor {
        Actor { mass: ZOMBIE_MASS, ..Actor::new(id, p) }
    }
    /// Current height of the capsule's top above the feet. Crouching on the ground lowers it; crouching in the air
    /// does not (see `tuck`).
    pub fn stature(&self) -> f64 {
        if self.prone {
            PRONE_HEIGHT
        } else if self.crouch && self.grounded {
            CROUCH_HEIGHT
        } else {
            self.height
        }
    }
    /// Crouch held off the ground tucks the knees up: the capsule is shortened from the bottom by what a crouch takes
    /// off (the feet come up towards the body) and its top stays where the jump put it, instead of the head coming down
    /// as a crouch on the ground does. Returns how far the bottom is raised above the feet (`p`), 0 untucked. Only the
    /// body's shape changes: the feet's arc is the jump's, and it lands as before, crouched (no height is gained and
    /// nothing is cleared that a plain jump would not clear).
    pub fn tuck(&self) -> f64 {
        if self.crouch && !self.prone && !self.grounded { self.height - CROUCH_HEIGHT } else { 0. }
    }
    /// Throws the character: `impulse` is [x, y, z] in newton seconds. The sideways part becomes knockback
    /// that friction wears away; an upward part lifts the character off the ground.
    pub fn apply_impulse(&mut self, impulse: [f64; 3]) {
        if self.fallen {
            return;
        }
        self.knock[0] += impulse[0] / self.mass;
        self.knock[1] += impulse[2] / self.mass;
        let lift = impulse[1] / self.mass;
        if !self.grounded {
            self.v[1] += lift;
        } else if lift > 0. {
            self.v[1] = lift;
            self.grounded = false;
        }
        self.launched = true;
    }
}

/// Single-precision conveniences for the game, which draws in f32. They only convert: the physics stays in
/// f64, and the world's own positions should stay the source of truth from step to step.
impl Actor {
    pub fn position(&self) -> Vec3 {
        Vec3::new(self.p[0] as f32, self.p[1] as f32, self.p[2] as f32)
    }
    pub fn velocity(&self) -> Vec3 {
        Vec3::new(self.v[0] as f32, self.v[1] as f32, self.v[2] as f32)
    }
    /// Teleports the character (a spawn or a reset): position set, motion cleared.
    pub fn place(&mut self, p: Vec3) {
        *self = Actor { radius: self.radius, height: self.height, mass: self.mass, prone: self.prone, crouch: self.crouch, ..Actor::new(self.id, [p.x as f64, p.y as f64, p.z as f64]) };
    }
    /// Sets the wanted horizontal velocity from an [x, z] vector.
    pub fn steer(&mut self, wish: Vec2) {
        self.wish = [wish.x as f64, wish.y as f64];
    }
}

/// Take-off speed that reaches `apex` metres under `gravity`.
pub fn launch_speed(apex: f64, gravity: f64) -> f64 {
    (2. * gravity * apex).sqrt()
}

/// A character as the props core sees it: the capsule where it is trying to stand (`p` is its feet).
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub id: u32,
    pub p: [f64; 3],
    pub radius: f64,
    pub height: f64,
    pub mass: f64,
    pub grounded: bool,
}
/// What the props core answers.
#[derive(Clone, Debug, PartialEq)]
pub struct Touch {
    /// Where the feet may be instead (only x and z are used; repeat `body.p` to leave it alone).
    pub p: [f64; 3],
    /// World height of a prop surface the character can stand on at that spot.
    pub support: Option<f64>,
    /// [x, y, z] newton seconds the props give the character (a heavy pan shoving it).
    pub impulse: Option<[f64; 3]>,
}
/// The props core plugs in here. `resolve` is called once per character per step, after the character has
/// moved against the level and before characters are kept apart. `from` is where its feet were when the step
/// began, `velocity` the velocity it moved with and `dt` the length of the step.
///
/// The level has the last word: a correction that would put the character inside a block is cut short.
/// Pushing the props themselves is the hook's own business; it knows the body's mass and velocity. A
/// character the hook has just pushed back is leaning on a prop, and other characters cannot shove it
/// further in.
pub trait Props {
    fn resolve(&mut self, body: &Body, from: [f64; 3], velocity: [f64; 3], dt: f64) -> Touch;
}
/// A kitchen without props.
pub struct NoProps;
impl Props for NoProps {
    fn resolve(&mut self, body: &Body, _from: [f64; 3], _velocity: [f64; 3], _dt: f64) -> Touch {
        Touch { p: body.p, support: None, impulse: None }
    }
}

// ----- The level, as a character meets it

/// The level with the blocks' outlines as map.rs keeps them: rect grown by MARGIN, then the height feet
/// must reach to pass over.
struct Ground<'a> {
    level: &'a Level,
    walls: &'a [[f64; 5]],
    config: &'a Config,
}
impl Ground<'_> {
    /// On the counter.
    fn inside(&self, x: f64, z: f64) -> bool {
        x >= self.level.x0 && x < self.level.x1 && z >= self.level.z0 && z < self.level.z1
    }
    /// Whether a block stops feet at height y from being at the point.
    fn walled(&self, x: f64, y: f64, z: f64) -> bool {
        self.walls.iter().any(|w| y < w[4] && x >= w[0] && x <= w[2] && z >= w[1] && z <= w[3])
    }
    /// Height of what is under feet at height y: the counter with its decks and ramps, the top of a block
    /// the feet have cleared, or the kitchen floor beyond the counter's edge.
    fn surface(&self, x: f64, y: f64, z: f64) -> f64 {
        if !self.inside(x, z) {
            return self.config.kitchen_floor;
        }
        let mut top = self.level.ground(x, z);
        for w in self.walls {
            if w[4] <= y && w[4] > top && x >= w[0] && x <= w[2] && z >= w[1] && z <= w[3] {
                top = w[4];
            }
        }
        top
    }
    /// Whether feet at height y may move to (x, z). On the counter this is the game's own rule,
    /// `Level::can_step`. Off it, only a character allowed to leave may pass, and only the overhang of a
    /// block can stop it.
    fn passable(&self, y: f64, leave: bool, x: f64, z: f64) -> bool {
        if self.inside(x, z) {
            return self.level.can_step([0., y, 0.], [x, y, z]);
        }
        leave && !self.walled(x, y, z)
    }
    /// A thrown character may cross an open edge, and one already over it keeps falling.
    fn may_leave(&self, actor: &Actor) -> bool {
        self.config.open_edge && (actor.launched || !self.inside(actor.p[0], actor.p[2]))
    }
}

/// How much of the move from (x, z) by (dx, dz) can be made, as a fraction from 0 to 1, when `allowed`
/// judges each spot. The start is taken to be allowed.
fn farthest(allowed: impl Fn(f64, f64) -> bool, x: f64, z: f64, dx: f64, dz: f64) -> f64 {
    if dx == 0. && dz == 0. {
        return 1.;
    }
    if allowed(x + dx, z + dz) {
        return 1.;
    }
    let (mut low, mut high) = (0., 1.);
    for _ in 0..HALVINGS {
        let middle = (low + high) / 2.;
        if allowed(x + dx * middle, z + dz * middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    low
}

/// Keeps a grounded character's feet on the surface: up a ledge or a ramp, or down a drop no taller than a step.
/// The prop it stood on at the last step (`support`, as `settle` takes it) is surface too, so a character
/// walking across a napkin or a pillow stays on it rather than being let down to the counter beneath at every
/// stride and climbing it again (which shoved the prop along and held the character back: it could get nowhere).
fn follow(ground: &Ground, actor: &mut Actor) {
    if !actor.grounded {
        return;
    }
    let mut under = ground.surface(actor.p[0], actor.p[1], actor.p[2]);
    if let Some(support) = actor.support
        && support > under
        && support <= actor.p[1] + STEP
    {
        under = support;
    }
    if under >= actor.p[1] - STEP {
        actor.p[1] = under;
    }
}

/// Moves a character horizontally by (dx, dz) against the level alone. A stride the level allows is taken
/// whole; a refused one is taken an axis at a time, as far as each axis goes, which slides the character
/// along walls, ledges and the counter's edge. Returns which axes were stopped: (x, z).
fn travel(ground: &Ground, actor: &mut Actor, dx: f64, dz: f64) -> (bool, bool) {
    let length = (dx * dx + dz * dz).sqrt();
    if length == 0. {
        return (false, false);
    }
    let strides = (length / STRIDE).ceil().max(1.);
    let (sx, sz) = (dx / strides, dz / strides);
    let leave = ground.may_leave(actor);
    let mut stopped = (false, false);
    for _ in 0..strides as usize {
        let y = actor.p[1];
        let allowed = |x: f64, z: f64| ground.passable(y, leave, x, z);
        if allowed(actor.p[0] + sx, actor.p[2] + sz) {
            actor.p[0] += sx;
            actor.p[2] += sz;
        } else {
            let along_x = farthest(allowed, actor.p[0], actor.p[2], sx, 0.);
            actor.p[0] += sx * along_x;
            let along_z = farthest(allowed, actor.p[0], actor.p[2], 0., sz);
            actor.p[2] += sz * along_z;
            stopped.0 |= along_x < 1.;
            stopped.1 |= along_z < 1.;
        }
        follow(ground, actor);
    }
    stopped
}

// ----- Characters against each other

/// Two capsules are solid to each other where their heights overlap: one standing on a deck clears one
/// below it, and one whose feet have reached another's head is on top of it, not beside it.
fn shares_height(a: &Actor, b: &Actor) -> bool {
    a.p[1] < b.p[1] + b.stature() - HEAD_STEP && b.p[1] < a.p[1] + a.stature() - HEAD_STEP
}

/// The characters `actors[i]` could touch while moving up to `range` from where it stands, leaving out
/// `ignore`. Gathering them once keeps the many spot tests of a push cheap in a crowd.
fn neighbours(actors: &[Actor], i: usize, range: f64, ignore: Option<usize>) -> Vec<usize> {
    let actor = &actors[i];
    let mut found = vec![];
    for (j, other) in actors.iter().enumerate() {
        if j == i || Some(j) == ignore || other.fallen || !shares_height(actor, other) {
            continue;
        }
        let reach = actor.radius + other.radius + range;
        let (dx, dz) = (other.p[0] - actor.p[0], other.p[2] - actor.p[2]);
        if dx * dx + dz * dz < reach * reach {
            found.push(j);
        }
    }
    found
}

/// Whether `actors[i]` would be clear of these neighbours at (x, z). A spot is also accepted when it is no
/// closer to a neighbour than (from_x, from_z) was, so characters that already overlap can still part.
fn clear_of_others(actors: &[Actor], near: &[usize], i: usize, x: f64, z: f64, from_x: f64, from_z: f64) -> bool {
    let actor = &actors[i];
    for &j in near {
        let other = &actors[j];
        let reach = actor.radius + other.radius - SLACK;
        let (ax, az) = (x - other.p[0], z - other.p[2]);
        let gap = ax * ax + az * az;
        if gap >= reach * reach {
            continue;
        }
        let (bx, bz) = (from_x - other.p[0], from_z - other.p[2]);
        if gap < bx * bx + bz * bz {
            return false;
        }
    }
    true
}

/// Pushes `actors[i]` by (dx, dz) as far as the level and its other neighbours let it go. `pusher` is the
/// one doing the pushing, which it is moving away from.
fn shove(ground: &Ground, actors: &mut [Actor], i: usize, mut dx: f64, mut dz: f64, pusher: usize) {
    let actor = &actors[i];
    // A character the props are holding back is not pushed into them.
    if let Some(pin) = actor.pin {
        let against = dx * pin[0] + dz * pin[1];
        if against < 0. {
            dx -= pin[0] * against;
            dz -= pin[1] * against;
        }
    }
    let (x, y, z, leave) = (actor.p[0], actor.p[1], actor.p[2], ground.may_leave(actor));
    let near = neighbours(actors, i, (dx * dx + dz * dz).sqrt(), Some(pusher));
    let allowed = |tx: f64, tz: f64| ground.passable(y, leave, tx, tz) && clear_of_others(actors, &near, i, tx, tz, x, z);
    let part = farthest(allowed, x, z, dx, dz);
    let actor = &mut actors[i];
    actor.p[0] = x + dx * part;
    actor.p[2] = z + dz * part;
    follow(ground, actor);
}

/// Settles `actors[i]`, which has moved from `from` to where it now stands, against the others. It stops
/// where it first touches one. What was left of its move into that neighbour is shared by mass: the
/// neighbour is pushed its share of the way if it has room, the character follows, and the rest of the move
/// slides past. A neighbour that cannot give way (a wall behind it, or a crowd) is not pushed: the character
/// steps round it, or stops if there is no room for that either. `yielding` replaces the mass share when
/// given: 1 makes the neighbour give way entirely (see `World::move_to`).
fn contend(ground: &Ground, actors: &mut [Actor], i: usize, from: [f64; 3], yielding: Option<f64>) {
    let [from_x, from_y, from_z] = from;
    let (ex, ez) = (actors[i].p[0] - from_x, actors[i].p[2] - from_z);
    let travelled = ex * ex + ez * ez;
    if travelled == 0. {
        return;
    }
    // The first neighbour met along the way; the lowest id wins a tie.
    let (mut first, mut when) = (None, 2.);
    for (j, other) in actors.iter().enumerate() {
        if j == i || other.fallen || !shares_height(&actors[i], other) {
            continue;
        }
        let reach = actors[i].radius + other.radius;
        let (fx, fz) = (from_x - other.p[0], from_z - other.p[2]);
        let closing = fx * ex + fz * ez;
        if closing >= 0. {
            continue;
        }
        let apart = fx * fx + fz * fz - reach * reach;
        let mut t = 0.;
        if apart > 2. * reach * SLACK {
            let root = closing * closing - travelled * apart;
            if root < 0. {
                continue;
            }
            t = (-closing - root.sqrt()) / travelled;
            if t > 1. {
                continue;
            }
        }
        if t < when {
            when = t;
            first = Some(j);
        }
    }
    let Some(first) = first else {
        return;
    };

    let (meet_x, meet_z) = (from_x + ex * when, from_z + ez * when);
    let (mut nx, mut nz) = (actors[first].p[0] - meet_x, actors[first].p[2] - meet_z);
    let gap = (nx * nx + nz * nz).sqrt();
    if gap > SLACK {
        nx /= gap;
        nz /= gap;
    } else {
        let length = travelled.sqrt();
        nx = ex / length;
        nz = ez / length;
    }
    let (rest_x, rest_z) = (ex * (1. - when), ez * (1. - when));
    let towards = rest_x * nx + rest_z * nz;
    let into = if towards > 0. { towards } else { 0. };
    let (slide_x, slide_z) = (rest_x - nx * into, rest_z - nz * into);

    // A thrown character passes momentum on like a lump of dough: after the knock both move together along the normal.
    let speed = (actors[i].knock[0] - actors[first].knock[0]) * nx + (actors[i].knock[1] - actors[first].knock[1]) * nz;
    if speed > 0. {
        let total = actors[i].mass + actors[first].mass;
        let (lost, gained) = (speed * actors[first].mass / total, speed * actors[i].mass / total);
        actors[i].knock[0] -= nx * lost;
        actors[i].knock[1] -= nz * lost;
        actors[first].knock[0] += nx * gained;
        actors[first].knock[1] += nz * gained;
        actors[i].launched = true;
        actors[first].launched = true;
    }

    // The neighbour gives way by the pusher's share of the mass, and the character follows as far as it went.
    let share = yielding.unwrap_or(actors[i].mass / (actors[i].mass + actors[first].mass));
    let (before_x, before_z) = (actors[first].p[0], actors[first].p[2]);
    actors[i].p = from;
    shove(ground, actors, first, nx * into * share, nz * into * share, i);
    let gave = (actors[first].p[0] - before_x) * nx + (actors[first].p[2] - before_z) * nz;
    // Every spot tried below lies within twice the length of the move.
    let leave = ground.may_leave(&actors[i]);
    let near = neighbours(actors, i, 2. * travelled.sqrt(), None);
    let allowed = |x: f64, z: f64| ground.passable(from_y, leave, x, z) && clear_of_others(actors, &near, i, x, z, from_x, from_z);
    let (follow_x, follow_z) = (meet_x + nx * gave, meet_z + nz * gave);
    // Whatever of its share the neighbour refused becomes a step round it: the way the character is already
    // sliding, or, dead centre, to one side for odd ids and the other for even ones.
    let (refused, turning) = (into - gave / share, slide_z * nx - slide_x * nz);
    let side = if turning > SLACK {
        1.
    } else if turning < -SLACK {
        -1.
    } else if actors[i].id % 2 == 1 {
        1.
    } else {
        -1.
    };
    let round = if refused > SLACK { refused * SIDESTEP * side } else { 0. };
    let (round_x, round_z) = (follow_x + slide_x - nz * round, follow_z + slide_z + nx * round);
    let spot = if round != 0. && allowed(round_x, round_z) {
        Some((round_x, round_z))
    } else if allowed(follow_x + slide_x, follow_z + slide_z) {
        Some((follow_x + slide_x, follow_z + slide_z))
    } else if allowed(follow_x, follow_z) {
        Some((follow_x, follow_z))
    } else if allowed(meet_x, meet_z) {
        Some((meet_x, meet_z))
    } else {
        None
    };
    if let Some((x, z)) = spot {
        actors[i].p[0] = x;
        actors[i].p[2] = z;
    }
    follow(ground, &mut actors[i]);
}

/// Clears the spot `actors[i]` has been put on by the props: a prop does not give way to a crowd, so every
/// neighbour now overlapping the character is pushed off it, whatever the masses, as far as it can go.
fn make_room(ground: &Ground, actors: &mut [Actor], i: usize) {
    for j in 0..actors.len() {
        let (actor, other) = (&actors[i], &actors[j]);
        if j == i || other.fallen || !shares_height(actor, other) {
            continue;
        }
        let reach = actor.radius + other.radius;
        let limit = reach - SLACK;
        let (mut nx, mut nz) = (other.p[0] - actor.p[0], other.p[2] - actor.p[2]);
        let square = nx * nx + nz * nz;
        if square >= limit * limit {
            continue;
        }
        let gap = square.sqrt();
        if gap > SLACK {
            nx /= gap;
            nz /= gap;
        } else {
            let pin = actor.pin.unwrap_or([0.; 4]);
            nx = -pin[0];
            nz = -pin[1];
        }
        shove(ground, actors, j, nx * (reach - gap), nz * (reach - gap), i);
    }
}

/// Moves a character by (dx, dz) against the level alone, sliding along what stops it; returns how far it
/// got in the direction asked for.
fn nudge(ground: &Ground, actor: &mut Actor, dx: f64, dz: f64) -> f64 {
    let length = (dx * dx + dz * dz).sqrt();
    if length == 0. {
        return 0.;
    }
    let (x, z) = (actor.p[0], actor.p[2]);
    travel(ground, actor, dx, dz);
    ((actor.p[0] - x) * dx + (actor.p[2] - z) * dz) / length
}

/// Parts characters that overlap. Moving never creates an overlap, so this only has work after a spawn, a
/// teleport, a correction from the props, or a character standing up under another. Each pair is moved
/// apart along the line between them, the lighter one further, and whatever a wall refuses on one side is
/// given to the other. This pass looks only at the level, so with walls all round a pair can stay
/// overlapped; it never makes anything worse and it adds no velocity.
fn separate(ground: &Ground, actors: &mut [Actor]) {
    for _ in 0..ROUNDS {
        let mut parted = false;
        for i in 0..actors.len() {
            if actors[i].fallen {
                continue;
            }
            for j in i + 1..actors.len() {
                let (head, tail) = actors.split_at_mut(j);
                let (a, b) = (&mut head[i], &mut tail[0]);
                if b.fallen || !shares_height(a, b) {
                    continue;
                }
                let reach = a.radius + b.radius;
                let limit = reach - SLACK;
                let (mut nx, mut nz) = (b.p[0] - a.p[0], b.p[2] - a.p[2]);
                let square = nx * nx + nz * nz;
                if square >= limit * limit {
                    continue;
                }
                let mut gap = square.sqrt();
                if gap > SLACK {
                    nx /= gap;
                    nz /= gap;
                } else {
                    [nx, nz] = SPREAD[(i + j) % SPREAD.len()];
                    gap = 0.;
                }
                let depth = reach - gap;
                let share = b.mass / (a.mass + b.mass);
                let first = depth * share;
                let mut left = depth;
                left -= nudge(ground, a, -nx * first, -nz * first);
                left -= nudge(ground, b, nx * left, nz * left);
                if left > SLACK {
                    left -= nudge(ground, a, -nx * left, -nz * left);
                }
                let mut sideways = 0.;
                if left > SLACK {
                    // Walls on both sides (a slot too narrow for two abreast): part them sideways instead.
                    let close = reach - left;
                    let across = (reach * reach - close * close).sqrt();
                    let mut side = across;
                    side -= nudge(ground, a, nz * side * share, -nx * side * share);
                    side -= nudge(ground, b, -nz * side, nx * side);
                    if side > SLACK {
                        side -= nudge(ground, a, nz * side, -nx * side);
                    }
                    sideways = across - side;
                }
                // Another round is only worth it while something still gives.
                if depth - left > SLACK || sideways > SLACK {
                    parted = true;
                }
            }
        }
        if !parted {
            break;
        }
    }
}

// ----- One character, one step

/// The horizontal half of a step: walk or slide, meet the level, then the props, then the other characters.
fn advance(ground: &Ground, actors: &mut [Actor], i: usize, dt: f64, props: &mut dyn Props) {
    let actor = &mut actors[i];
    let from = actor.p;
    let pace = (if actor.launched { ground.config.control } else { 1. }) * (if actor.prone { PRONE_PACE } else if actor.crouch { CROUCH_PACE } else { 1. });
    let (mut vx, mut vz) = (actor.wish[0] * pace + actor.knock[0], actor.wish[1] * pace + actor.knock[1]);
    // Whoever stands on a head slides off it.
    if let Some(off) = actor.perch {
        vx += off[0] * HEAD_SLIDE;
        vz += off[1] * HEAD_SLIDE;
    }
    let stopped = travel(ground, actor, vx * dt, vz * dt);
    // Running into a wall ends that part of a throw.
    if stopped.0 {
        actor.knock[0] = 0.;
    }
    if stopped.1 {
        actor.knock[1] = 0.;
    }

    let body = Body { id: actor.id, p: actor.p, radius: actor.radius, height: actor.stature(), mass: actor.mass, grounded: actor.grounded };
    let touch = props.resolve(&body, from, [vx, actor.v[1], vz], dt);
    actor.support = touch.support;
    let (dx, dz) = (touch.p[0] - actor.p[0], touch.p[2] - actor.p[2]);
    let length = (dx * dx + dz * dz).sqrt();
    if length > 0. {
        let (x, y, z, leave) = (actor.p[0], actor.p[1], actor.p[2], ground.may_leave(actor));
        let part = farthest(|tx, tz| ground.passable(y, leave, tx, tz), x, z, dx, dz);
        actor.p[0] += dx * part;
        actor.p[2] += dz * part;
        follow(ground, actor);
        actor.pin = Some([dx / length, dz / length, actor.p[0], actor.p[2]]);
        make_room(ground, actors, i);
    } else if actor.pin.is_some_and(|pin| actor.p[0] != pin[2] || actor.p[2] != pin[3]) {
        // It has moved on without being corrected, so it no longer leans on the prop.
        actor.pin = None;
    }
    if let Some(impulse) = touch.impulse {
        actors[i].apply_impulse(impulse);
    }
    contend(ground, actors, i, from, None);
    // Being stopped or carried by a neighbour is not leaving the prop.
    let actor = &mut actors[i];
    if let Some(pin) = &mut actor.pin {
        pin[2] = actor.p[0];
        pin[3] = actor.p[2];
    }
}

/// The vertical half of a step: stay on the ground, jump, fall, land; then friction on a throw.
fn settle(ground: &Ground, actors: &mut [Actor], i: usize, dt: f64) {
    let config = ground.config;
    let p = actors[i].p;
    let mut under = ground.surface(p[0], p[1], p[2]);
    // A prop counts as ground when the feet are on it or within a step below its top.
    if let Some(support) = actors[i].support
        && support > under
        && support <= p[1] + STEP
    {
        under = support;
    }
    // So does the head of a character directly below; `off` is the way to slide off it.
    let (mut head, mut off) = (false, [0.; 2]);
    for (j, other) in actors.iter().enumerate() {
        if j == i || other.fallen {
            continue;
        }
        let top = other.p[1] + other.stature();
        let reach = actors[i].radius + other.radius - SLACK;
        let (dx, dz) = (p[0] - other.p[0], p[2] - other.p[2]);
        let square = dx * dx + dz * dz;
        if top <= under || top > p[1] + HEAD_STEP || square >= reach * reach {
            continue;
        }
        under = top;
        head = true;
        let gap = square.sqrt();
        off = if gap > SLACK { [dx / gap, dz / gap] } else { SPREAD[j % SPREAD.len()] };
    }

    let actor = &mut actors[i];
    if actor.grounded {
        if actor.jump > 0. && !actor.prone && !actor.launched {
            actor.v[1] = actor.jump;
            actor.grounded = false;
        } else if under < actor.p[1] - STEP {
            actor.v[1] = 0.;
            actor.grounded = false;
        } else {
            actor.p[1] = under;
            actor.v[1] = 0.;
        }
    }
    if !actor.grounded {
        // Exact for constant gravity, so the arc does not depend on the step length.
        let y = actor.p[1] + actor.v[1] * dt - 0.5 * config.gravity * dt * dt;
        actor.v[1] -= config.gravity * dt;
        if y <= under {
            actor.p[1] = under;
            actor.v[1] = 0.;
            actor.grounded = true;
        } else {
            actor.p[1] = y;
        }
    }
    actor.perch = if head && actor.grounded { Some(off) } else { None };
    if actor.grounded && !ground.inside(actor.p[0], actor.p[2]) {
        // Over the edge and down: the game treats this as a death.
        actor.fallen = true;
        actor.launched = false;
        actor.knock = [0.; 2];
        return;
    }
    if actor.launched && actor.grounded {
        let speed = (actor.knock[0] * actor.knock[0] + actor.knock[1] * actor.knock[1]).sqrt();
        let slower = speed - config.friction * config.gravity * dt;
        if slower > 0. {
            actor.knock[0] *= slower / speed;
            actor.knock[1] *= slower / speed;
        } else {
            actor.knock = [0.; 2];
            actor.launched = false;
        }
    }
}

/// The characters of a kitchen and the rules that move them.
#[derive(Clone, Debug, Default)]
pub struct World {
    pub config: Config,
    /// Kept in order of id, which is the order everything is resolved in.
    pub actors: Vec<Actor>,
    /// Walls of blocks the level stands that its map does not list (a draped napkin's, then the forcefield's):
    /// what map.rs keeps privately for them, which the bodies stand on as on any block. The game keeps it up to
    /// date (drape.rs `relevel`).
    pub extra: Vec<[f64; 5]>,
    walls: Vec<[f64; 5]>,
    starts: Vec<[f64; 2]>,
}
impl World {
    pub fn new(config: Config) -> World {
        World { config, ..World::default() }
    }
    /// Adds a character (replacing one with the same id) and returns it.
    pub fn add(&mut self, actor: Actor) -> &mut Actor {
        self.remove(actor.id);
        let at = self.actors.partition_point(|other| other.id < actor.id);
        self.actors.insert(at, actor);
        &mut self.actors[at]
    }
    pub fn remove(&mut self, id: u32) -> Option<Actor> {
        let at = self.actors.iter().position(|actor| actor.id == id)?;
        Some(self.actors.remove(at))
    }
    pub fn get(&self, id: u32) -> Option<&Actor> {
        self.actors.iter().find(|actor| actor.id == id)
    }
    pub fn get_mut(&mut self, id: u32) -> Option<&mut Actor> {
        self.actors.iter_mut().find(|actor| actor.id == id)
    }

    /// Advances every character by `dt` seconds on `level`. Intervals longer than `config.max_dt` are cut
    /// into equal parts. Call it with a fixed dt (the simulations use 1/30) for results that repeat exactly.
    pub fn step(&mut self, level: &Level, dt: f64, props: &mut dyn Props) {
        if dt.is_nan() || dt <= 0. {
            return;
        }
        self.survey(level);
        self.starts.clear();
        self.starts.extend(self.actors.iter().map(|actor| [actor.p[0], actor.p[2]]));
        // The slack lets a single-precision 1/30 count as one part, not two.
        let parts = (dt / self.config.max_dt - 1e-6).ceil().max(1.);
        let part = dt / parts;
        let ground = Ground { level, walls: &self.walls, config: &self.config };
        let actors = &mut self.actors[..];
        for _ in 0..parts as usize {
            for i in 0..actors.len() {
                if !actors[i].fallen && !actors[i].idle {
                    advance(&ground, actors, i, part, props);
                }
            }
            for i in 0..actors.len() {
                if !actors[i].fallen && !actors[i].idle {
                    settle(&ground, actors, i, part);
                }
            }
            separate(&ground, actors);
        }
        for (actor, start) in self.actors.iter_mut().zip(&self.starts) {
            actor.v[0] = (actor.p[0] - start[0]) / dt;
            actor.v[2] = (actor.p[2] - start[1]) / dt;
            actor.jump = 0.;
        }
    }

    /// Advances one character only by `dt`, as `step` advances everybody (its fall, its jump, a throw, the props),
    /// while the others stay where they are. The server uses it for the first of two commands of a person's that it
    /// applies in one of its own steps, so that each command takes a step.
    pub fn step_only(&mut self, level: &Level, dt: f64, props: &mut dyn Props, id: u32) {
        if dt.is_nan() || dt <= 0. {
            return;
        }
        self.survey(level);
        let Some(i) = self.actors.iter().position(|actor| actor.id == id && !actor.fallen) else {
            return;
        };
        let start = [self.actors[i].p[0], self.actors[i].p[2]];
        let parts = (dt / self.config.max_dt - 1e-6).ceil().max(1.);
        let part = dt / parts;
        let ground = Ground { level, walls: &self.walls, config: &self.config };
        for _ in 0..parts as usize {
            advance(&ground, &mut self.actors, i, part, props);
            settle(&ground, &mut self.actors, i, part);
            separate(&ground, &mut self.actors);
        }
        let actor = &mut self.actors[i];
        actor.v[0] = (actor.p[0] - start[0]) / dt;
        actor.v[2] = (actor.p[2] - start[1]) / dt;
        actor.jump = 0.;
    }

    /// Puts the characters in order of id and notes the level's blocks, ready to move anybody.
    fn survey(&mut self, level: &Level) {
        self.actors.sort_by_key(|actor| actor.id);
        self.walls.clear();
        // The same numbers map.rs keeps privately: each block grown by the margin, and the height that clears it.
        self.walls.extend(level.map.blocks.iter().map(|block| {
            let [x0, z0, x1, z1] = block.rect;
            [x0 - MARGIN, z0 - MARGIN, x1 + MARGIN, z1 + MARGIN, FLOOR + (block.height + LIP)]
        }));
        self.walls.extend_from_slice(&self.extra);
    }

    /// Walks one character by (dx, dz) right now, against the level and the other characters, exactly as a
    /// step would move it, and returns its position (None when there is no such character). The game uses
    /// this for the hotdog a person is steering, so that the view does not wait for the next step; `step`
    /// then finds it with nothing left to walk and only has its jump, its fall and any throw to work out.
    pub fn walk(&mut self, level: &Level, id: u32, dx: f64, dz: f64) -> Option<[f64; 3]> {
        self.survey(level);
        let i = self.actors.iter().position(|actor| actor.id == id && !actor.fallen)?;
        let ground = Ground { level, walls: &self.walls, config: &self.config };
        let from = self.actors[i].p;
        travel(&ground, &mut self.actors[i], dx, dz);
        contend(&ground, &mut self.actors, i, from, None);
        Some(self.actors[i].p)
    }

    /// Throws the character with this id; see `Actor::apply_impulse`.
    pub fn impulse(&mut self, id: u32, impulse: [f64; 3]) {
        if let Some(actor) = self.get_mut(id) {
            actor.apply_impulse(impulse);
        }
    }

    /// An explosion at `center`: every character within `radius` is thrown away from it. One at the centre
    /// receives `impulse` newton seconds, fading linearly to nothing at the radius; `lift` tilts the throw
    /// upwards (BLAST_LIFT is the usual value). Blocks shield, as they shield from the damage. Returns the
    /// ids of the characters thrown. The props core has a blast() with the same arguments.
    pub fn blast(&mut self, level: &Level, center: [f64; 3], radius: f64, impulse: f64, lift: f64) -> Vec<u32> {
        let mut thrown = vec![];
        for actor in &mut self.actors {
            if actor.fallen {
                continue;
            }
            let (p, tall, tuck) = (actor.p, actor.stature(), actor.tuck());
            // Distance is measured to the nearest point of the capsule's axis (raised from the bottom by a tuck); the
            // throw is aimed at its middle.
            let near_y = center[1].max(p[1] + tuck).min(p[1] + tall);
            let (nx, ny, nz) = (p[0] - center[0], near_y - center[1], p[2] - center[2]);
            let gap = (nx * nx + ny * ny + nz * nz).sqrt();
            if gap >= radius {
                continue;
            }
            if gap > 1e-4 && level.cover(center, [nx / gap, ny / gap, nz / gap]) < gap {
                continue;
            }
            let (mut dx, mut dy, mut dz) = (nx, p[1] + (tuck + tall) / 2. - center[1], nz);
            let length = (dx * dx + dy * dy + dz * dz).sqrt();
            if length > 1e-9 {
                dx /= length;
                dy /= length;
                dz /= length;
            } else {
                (dx, dy, dz) = (0., 1., 0.);
            }
            dy += lift;
            let scale = impulse * (1. - gap / radius) / (dx * dx + dy * dy + dz * dz).sqrt();
            actor.apply_impulse([dx * scale, dy * scale, dz * scale]);
            thrown.push(actor.id);
        }
        thrown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Map;
    use std::time::Instant;

    const DT: f64 = 1. / 30.;

    fn bundled() -> Level {
        Level::new(Map::bundled())
    }
    /// A small open counter for tests that want particular ledges, blocks and edges.
    fn yard(extra: &str) -> Level {
        let text = format!(r#"{{"format":"hotdog-map","version":1,"name":"Yard","bounds":[0,0,3,2],"starts":[{{"p":[0.2,0.2]}}],"spawns":[{{"p":[0.2,1.8]}}]{extra}}}"#);
        Level::new(Map::parse(&text).unwrap())
    }
    fn on(level: &Level, x: f64, z: f64) -> [f64; 3] {
        [x, level.ground(x, z), z]
    }
    fn above(level: &Level, x: f64, z: f64, lift: f64) -> [f64; 3] {
        [x, level.ground(x, z) + lift, z]
    }
    fn flat(a: &Actor, b: &Actor) -> f64 {
        ((a.p[0] - b.p[0]).powi(2) + (a.p[2] - b.p[2]).powi(2)).sqrt()
    }
    fn near(actual: f64, expected: f64, tolerance: f64, what: &str) {
        assert!((actual - expected).abs() <= tolerance, "{what}: {actual} against {expected} (tolerance {tolerance})");
    }
    fn walkable(level: &Level, p: [f64; 3]) -> bool {
        level.walkable(p[0], p[1], p[2])
    }
    fn energy(world: &World) -> f64 {
        world.actors.iter().map(|a| 0.5 * a.mass * (a.v[0] * a.v[0] + a.v[1] * a.v[1] + a.v[2] * a.v[2])).sum()
    }
    /// The smallest gap between the surfaces of any two characters side by side (not one on the other's
    /// head); negative means overlap.
    fn tightest(world: &World) -> f64 {
        let mut least = f64::INFINITY;
        for a in &world.actors {
            for b in &world.actors {
                if a.id < b.id && !a.fallen && !b.fallen && shares_height(a, b) {
                    least = least.min(flat(a, b) - a.radius - b.radius);
                }
            }
        }
        least
    }
    /// Steps a world `ticks` times at 30 Hz, calling `each(world, tick)` after every step.
    fn run(world: &mut World, level: &Level, ticks: usize, mut each: impl FnMut(&World, usize)) {
        for tick in 1..=ticks {
            world.step(level, DT, &mut NoProps);
            each(world, tick);
        }
    }
    fn wish(world: &mut World, id: u32, wish: [f64; 2]) {
        world.get_mut(id).unwrap().wish = wish;
    }
    fn actor(world: &World, id: u32) -> &Actor {
        world.get(id).unwrap()
    }
    /// Deterministic sampling keeps failures reproducible (the generator the JavaScript tests use).
    fn random(seed: u32) -> impl FnMut() -> f64 {
        let mut seed = seed;
        move || {
            seed = seed.wrapping_add(0x6D2B_79F5);
            let mut t = (seed ^ (seed >> 15)).wrapping_mul(1 | seed);
            t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
            (t ^ (t >> 14)) as f64 / 4_294_967_296.
        }
    }

    // ----- Walking against the level

    /// What feet at height y stand on: the surface, or the top of a block (with its margin) they have cleared.
    fn standing(level: &Level, x: f64, y: f64, z: f64) -> f64 {
        let mut h = level.ground(x, z);
        for block in &level.map.blocks {
            let [x0, z0, x1, z1] = block.rect;
            let top = FLOOR + (block.height + LIP);
            if top <= y && top > h && x >= x0 - MARGIN && x <= x1 + MARGIN && z >= z0 - MARGIN && z <= z1 + MARGIN {
                h = top;
            }
        }
        h
    }

    #[test]
    fn a_stride_the_game_allows_today_ends_where_it_does_today_and_a_refused_one_never_passes_through() {
        let level = bundled();
        let mut next = random(20_261_111);
        let mut rects: Vec<[f64; 4]> = level.map.blocks.iter().map(|b| [b.rect[0] - MARGIN, b.rect[1] - MARGIN, b.rect[2] + MARGIN, b.rect[3] + MARGIN]).collect();
        rects.extend(level.map.decks.iter().map(|d| d.rect));
        rects.extend(level.map.ramps.iter().map(|r| r.rect));
        rects.push(level.map.bounds);
        let (mut allowed, mut refused, mut slid, mut dropped) = (0, 0, 0, 0);
        for i in 0..9000 {
            // A third of the starts are anywhere on the counter; the rest hug the blocks, the deck, the ramps and the edge.
            let (x, z);
            if i % 3 == 0 {
                x = level.x0 + next() * (level.x1 - level.x0);
                z = level.z0 + next() * (level.z1 - level.z0);
            } else {
                let r = rects[(next() * rects.len() as f64) as usize];
                let (side, off) = ((next() * 4.) as usize, (next() - 0.5) * 0.08);
                x = if side < 2 { r[0] + next() * (r[2] - r[0]) } else { r[if side == 3 { 2 } else { 0 }] + off };
                z = if side < 2 { r[if side == 1 { 3 } else { 1 }] + off } else { r[1] + next() * (r[3] - r[1]) };
            }
            let y = level.ground(x, z);
            if !level.walkable(x, y, z) {
                continue;
            }
            let (angle, length) = (next() * std::f64::consts::TAU, 0.002 + next() * 0.043);
            let speed = length / DT;
            let wish = [angle.cos() * speed, angle.sin() * speed];
            let to = [x + wish[0] * DT, y, z + wish[1] * DT];
            let mut world = World::default();
            world.add(Actor::hotdog(1, [x, y, z])).wish = wish;
            world.step(&level, DT, &mut NoProps);
            let actor = &world.actors[0];
            let [fx, fy, fz] = actor.p;
            let context = format!("from {:?} to {to:?}, ended at {:?}", [x, y, z], actor.p);
            if level.can_step([x, y, z], to) {
                allowed += 1;
                assert!(fx == to[0] && fz == to[2], "{context}");
                let ground = standing(&level, fx, y, fz);
                // Today the server and the bots snap to the surface; a drop taller than a step is now a fall instead.
                if ground >= y - STEP {
                    assert_eq!(fy, ground, "{context}");
                } else {
                    dropped += 1;
                    assert!(!actor.grounded && fy < y && fy > ground, "{context}");
                }
            } else {
                refused += 1;
                assert!(level.walkable(fx, fy, fz), "ended inside a block's margin or off the counter: {context}");
                assert!(level.ground(fx, fz) <= y + STEP, "climbed a ledge taller than a step: {context}");
                assert!(fx >= x.min(to[0]) && fx <= x.max(to[0]) && fz >= z.min(to[2]) && fz <= z.max(to[2]), "left the stride's rectangle: {context}");
                if fx != x || fz != z {
                    slid += 1;
                }
            }
        }
        eprintln!("strides: {allowed} allowed ({dropped} of them stepped off a drop and fell), {refused} refused ({slid} of those slid part of the way)");
        assert!(allowed > 3000 && refused > 500 && slid > 400 && dropped > 20, "{allowed} allowed, {refused} refused, {slid} slid, {dropped} dropped");
    }

    #[test]
    fn wandering_at_full_stim_speed_never_ends_inside_a_block_off_the_counter_or_up_a_ledge() {
        let level = bundled();
        let (mut next, mut world) = (random(7), World::default());
        for i in 0..level.map.starts.len() {
            world.add(Actor { radius: 0.001, ..Actor::hotdog(i as u32 + 1, level.start_position(i)) });
        }
        for tick in 0..3000 {
            if tick % 20 == 0 {
                for w in &mut world.actors {
                    let a = next() * std::f64::consts::TAU;
                    w.wish = [a.cos() * WALK * STIM_PACE, a.sin() * WALK * STIM_PACE];
                }
            }
            let before: Vec<f64> = world.actors.iter().map(|w| w.p[1]).collect();
            world.step(&level, DT, &mut NoProps);
            for (w, before) in world.actors.iter().zip(before) {
                assert!(walkable(&level, w.p), "walker {} at {:?}", w.id, w.p);
                assert!(w.p[1] <= before + STEP + 1e-12, "walker {} rose {} in one step", w.id, w.p[1] - before);
                assert!(w.p[1] >= level.ground(w.p[0], w.p[2]) - 1e-12);
            }
        }
    }

    #[test]
    fn walking_diagonally_into_a_block_slides_along_its_wall() {
        // The steel block's -x face, with the margin, is at x = -0.48.
        let level = bundled();
        let (mut world, diagonal) = (World::default(), WALK * std::f64::consts::FRAC_1_SQRT_2);
        world.add(Actor::hotdog(1, on(&level, -0.56, 2.75))).wish = [diagonal, diagonal];
        run(&mut world, &level, 4, |_, _| {});
        let (a, wall) = (actor(&world, 1), -0.45 - MARGIN);
        assert!(a.p[0] < wall && a.p[0] > wall - 1e-4, "pressed against the wall, x = {}", a.p[0]);
        near(a.p[2], 2.75 + 4. * DT * diagonal, 1e-12, "kept its full speed along the wall");
        near(a.v[2], diagonal, 1e-9, "speed along the wall");
        near(a.v[0], 0., 1e-3, "speed into the wall");
        assert!(a.grounded);
        // Past the block's corner it is free again.
        run(&mut world, &level, 12, |_, _| {});
        assert!(actor(&world, 1).p[0] > -0.45, "rounded the corner: {:?}", actor(&world, 1).p);
    }

    #[test]
    fn a_low_ledge_is_stepped_onto_and_a_taller_one_is_refused_on_foot_and_reached_by_jumping() {
        // (The higher deck is long enough to land on at a walk: the jump carries 0.71 m.)
        let level = yard(r#","decks":[{"rect":[1,0,1.5,2],"height":0.04},{"rect":[2,0,3,2],"height":0.06}]"#);
        let mut world = World::default();
        world.add(Actor::hotdog(1, on(&level, 0.9, 0.5))).wish = [WALK, 0.];
        world.add(Actor::hotdog(2, on(&level, 1.9, 1.5))).wish = [WALK, 0.];
        run(&mut world, &level, 7, |_, _| {});
        let (low, high) = (actor(&world, 1), actor(&world, 2));
        assert!(low.p[1] == FLOOR + 0.04 && low.p[0] > 1.1 && low.grounded);
        assert_eq!(high.p[1], FLOOR, "stayed on the counter");
        assert!(high.p[0] < 2. && high.p[0] > 2. - 1e-4, "stopped at the foot of the ledge: {}", high.p[0]);
        // Airborne and high enough: the same ledge is cleared with a jump.
        world.get_mut(2).unwrap().jump = JUMP;
        run(&mut world, &level, 18, |_, _| {});
        let high = actor(&world, 2);
        assert!(high.p[1] == FLOOR + 0.06 && high.p[0] > 2.05 && high.grounded, "{:?}", high.p);
    }

    #[test]
    fn both_ramps_are_walked_up_onto_the_deck_feet_on_the_surface_all_the_way() {
        let level = bundled();
        for (from, to) in [([0.5f64, 3.53], [1.4f64, 3.53]), ([1.65, 2.4], [1.65, 3.3])] {
            let mut world = World::default();
            let length = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
            world.add(Actor::hotdog(1, on(&level, from[0], from[1]))).wish = [(to[0] - from[0]) / length * WALK, (to[1] - from[1]) / length * WALK];
            let mut highest = FLOOR;
            run(&mut world, &level, (length / WALK / DT).round() as usize, |world, _| {
                let a = actor(world, 1);
                assert!(a.grounded, "left the ground at {:?}", a.p);
                assert_eq!(a.p[1], level.ground(a.p[0], a.p[2]));
                assert!(a.p[1] >= highest);
                highest = a.p[1];
            });
            let a = actor(&world, 1);
            assert_eq!(a.p[1], FLOOR + 0.28);
            near(a.p[0], to[0], 0.02, "x");
            near(a.p[2], to[1], 0.02, "z");
            // And back down again without ever leaving the surface.
            let back = [-a.wish[0], -a.wish[1]];
            wish(&mut world, 1, back);
            run(&mut world, &level, (length / WALK / DT).round() as usize, |world, _| {
                let a = actor(world, 1);
                assert!(a.grounded && a.p[1] == level.ground(a.p[0], a.p[2]), "left the ground at {:?}", a.p);
            });
            assert_eq!(actor(&world, 1).p[1], FLOOR);
        }
    }

    #[test]
    fn walking_off_the_deck_is_a_fall_that_lasts_as_long_as_the_height_says() {
        let level = bundled();
        let mut world = World::default();
        world.add(Actor::hotdog(1, on(&level, 1.3, 3.3))).wish = [-WALK, 0.];
        assert_eq!(actor(&world, 1).p[1], FLOOR + 0.28);
        let (mut left, mut landed) = (0, 0);
        run(&mut world, &level, 40, |world, tick| {
            let a = actor(world, 1);
            if left == 0 && !a.grounded {
                left = tick;
            }
            if left > 0 && landed == 0 {
                let t = (tick - left + 1) as f64 * DT;
                if a.grounded {
                    landed = tick;
                } else {
                    near(a.p[1], FLOOR + 0.28 - 0.5 * GRAVITY * t * t, 1e-12, "height during the fall");
                }
            }
        });
        assert!(left > 0 && landed > left, "it fell and landed");
        let a = actor(&world, 1);
        assert!(a.p[1] == FLOOR && a.v[1] == 0.);
        let (expected, took) = ((2. * 0.28 / GRAVITY).sqrt(), (landed - left + 1) as f64 * DT);
        assert!(took >= expected && took < expected + DT, "fell for {took} s; the closed form says {expected} s");
        assert!(a.p[0] < 1.25, "it kept walking while it fell");
    }

    /// Height above the counter at the top of a jump, and the time from take-off to landing.
    fn flight(level: &Level, speed: f64, prone: bool) -> (f64, f64) {
        let mut world = World::default();
        let a = world.add(Actor::hotdog(1, on(level, 0., 3.)));
        a.prone = prone;
        a.jump = speed;
        let (mut apex, mut ticks) = (0f64, 0);
        run(&mut world, level, 60, |world, tick| {
            let a = actor(world, 1);
            apex = apex.max(a.p[1] - FLOOR);
            if !a.grounded {
                ticks = tick;
            }
        });
        assert!(actor(&world, 1).p[1] == FLOOR && actor(&world, 1).grounded);
        (apex, (ticks + 1) as f64 * DT)
    }

    #[test]
    fn a_jump_is_a_parabola_stim_jumps_higher_and_prone_does_not_jump() {
        let level = bundled();
        let (plain, stim, sampling) = (flight(&level, JUMP, false), flight(&level, STIM_JUMP, false), GRAVITY * DT * DT / 8.);
        for (got, speed) in [(plain, JUMP), (stim, STIM_JUMP)] {
            let (apex, time) = (speed * speed / (2. * GRAVITY), 2. * speed / GRAVITY);
            assert!(got.0 <= apex + 1e-12 && got.0 > apex - sampling - 1e-12, "apex {} against {apex}", got.0);
            assert!(got.1 >= time && got.1 < time + DT, "airtime {} against {time}", got.1);
        }
        near(plain.0, 0.1143, 5e-4, "the plain jump keeps its familiar height");
        assert!(stim.0 > plain.0 + 0.07 && stim.1 > plain.1);
        assert_eq!(flight(&level, JUMP, true), (0., DT), "a prone hotdog stays down");
        near(launch_speed(plain.0 + sampling, GRAVITY), JUMP, 0.01, "launch speed for that apex");
    }

    #[test]
    fn crouching_is_a_third_stance_lower_and_slower_than_standing_higher_and_quicker_than_prone_and_it_can_jump() {
        let level = bundled();
        // How far a hotdog gets in a second of walking, and how tall it is, in each stance.
        let walk = |crouch: bool, prone: bool| {
            let mut world = World::default();
            let a = world.add(Actor::hotdog(1, on(&level, 0., 3.)));
            (a.crouch, a.prone, a.wish) = (crouch, prone, [WALK, 0.]);
            run(&mut world, &level, 30, |_, _| {});
            (actor(&world, 1).p[0], actor(&world, 1).stature())
        };
        let (standing, crouching, prone) = (walk(false, false), walk(true, false), walk(false, true));
        near(standing.0, WALK, 1e-9, "a second of walking");
        near(crouching.0, WALK * CROUCH_PACE, 1e-9, "a second of crouch-walking");
        near(prone.0, WALK * PRONE_PACE, 1e-9, "a second of crawling");
        assert_eq!([standing.1, crouching.1, prone.1], [HEIGHT, CROUCH_HEIGHT, PRONE_HEIGHT]);
        const { assert!(PRONE_HEIGHT < CROUCH_HEIGHT && CROUCH_HEIGHT < HEIGHT && PRONE_PACE < CROUCH_PACE && CROUCH_PACE < 1.) };
        assert_eq!(walk(true, true), prone, "lying down wins over crouching");
        // A crouching hotdog jumps exactly as a standing one does, and is still crouching when it lands.
        let mut world = World::default();
        let a = world.add(Actor::hotdog(1, on(&level, 0., 3.)));
        (a.crouch, a.jump) = (true, JUMP);
        let (mut apex, mut ticks) = (0f64, 0);
        run(&mut world, &level, 60, |world, tick| {
            let a = actor(world, 1);
            apex = apex.max(a.p[1] - FLOOR);
            if !a.grounded {
                ticks = tick;
            }
        });
        assert_eq!((apex, (ticks + 1) as f64 * DT), flight(&level, JUMP, false));
        assert!(actor(&world, 1).crouch && actor(&world, 1).grounded);
        // A plain jump, which does not clear a standing hotdog, comes down on a crouching one's head.
        const { assert!(JUMP * JUMP / (2. * GRAVITY) > CROUCH_HEIGHT - HEAD_STEP) };
    }

    #[test]
    fn crouch_held_in_the_air_tucks_the_knees_up_without_raising_the_head_or_the_jump_and_lands_crouched() {
        let level = bundled();
        // A jump, crouch held from the first step in the air or not: where the feet and the head get to, when it lands.
        let jump = || {
            let mut world = World::default();
            world.add(Actor::hotdog(1, on(&level, 0., 3.))).jump = JUMP;
            let (mut feet, mut head, mut down, mut tucked) = (0f64, 0f64, 0, vec![]);
            run(&mut world, &level, 60, |world, tick| {
                let a = actor(world, 1);
                (feet, head) = (feet.max(a.p[1] - FLOOR), head.max(a.p[1] + a.stature() - FLOOR));
                if !a.grounded {
                    down = tick;
                    tucked.push(a.tuck());
                }
            });
            (feet, head, down, tucked, world)
        };
        let (feet, head, down, _, _) = jump();
        let mut world = World::default();
        world.add(Actor::hotdog(1, on(&level, 0., 3.))).jump = JUMP;
        world.step(&level, DT, &mut NoProps);
        world.get_mut(1).unwrap().crouch = true;
        let (mut tucked_feet, mut tucked_head, mut tucked_down, mut lift) = (0f64, 0f64, 0, 0f64);
        run(&mut world, &level, 59, |world, tick| {
            let a = actor(world, 1);
            (tucked_feet, tucked_head) = (tucked_feet.max(a.p[1] - FLOOR), tucked_head.max(a.p[1] + a.stature() - FLOOR));
            if !a.grounded {
                tucked_down = tick + 1;
                lift = lift.max(a.tuck());
                assert_eq!((a.stature(), a.tuck()), (HEIGHT, HEIGHT - CROUCH_HEIGHT), "tucked: the top where it was, the bottom up");
            }
        });
        assert_eq!((tucked_feet, tucked_head, tucked_down), (feet, head, down), "the same arc, no higher, down at the same step");
        assert!(lift > 0.049 && (head - feet - HEIGHT).abs() < 1e-9);
        let a = actor(&world, 1);
        assert!(a.grounded && a.crouch && a.stature() == CROUCH_HEIGHT && a.tuck() == 0., "it lands crouched");
        // On the ground a crouch is as it always was, and an untucked jump has nothing raised.
        assert!(jump().3.iter().all(|t| *t == 0.));
        let mut world = World::default();
        world.add(Actor::hotdog(1, on(&level, 0., 3.))).crouch = true;
        world.step(&level, DT, &mut NoProps);
        assert_eq!((actor(&world, 1).stature(), actor(&world, 1).tuck()), (CROUCH_HEIGHT, 0.));
    }

    #[test]
    fn the_arc_does_not_depend_on_the_step_length_and_long_steps_are_cut_up() {
        let level = bundled();
        let heights = |hz: usize| {
            let mut world = World::default();
            world.add(Actor::hotdog(1, on(&level, 0., 3.))).jump = JUMP;
            (0..hz / 2).map(|_| { world.step(&level, 1. / hz as f64, &mut NoProps); world.actors[0].p[1] }).collect::<Vec<f64>>()
        };
        // 10 Hz steps are cut into three parts of 1/30 s; both runs agree wherever their clocks meet.
        let (coarse, fine) = (heights(10), heights(60));
        for (i, height) in coarse.iter().enumerate() {
            near(*height, fine[i * 6 + 5], 1e-12, "height where the clocks meet");
        }
    }

    // ----- Characters against each other

    #[test]
    fn two_hotdogs_walking_into_each_other_stop_touching_and_never_overlap() {
        let level = bundled();
        let mut world = World::default();
        world.add(Actor::hotdog(1, on(&level, -0.3, 2.4))).wish = [WALK, 0.];
        world.add(Actor::hotdog(2, on(&level, 0.3, 2.4))).wish = [-WALK, 0.];
        run(&mut world, &level, 60, |world, _| assert!(tightest(world) >= -1e-9, "overlap of {}", -tightest(world)));
        let (a, b) = (actor(&world, 1), actor(&world, 2));
        near(flat(a, b), 2. * RADIUS, 1e-9, "they end up touching");
        near(a.v[0], 0., 1e-9, "a at rest");
        near(b.v[0], 0., 1e-9, "b at rest");
        near(a.p[0] + b.p[0], 0., 0.03, "they met in the middle");
    }

    #[test]
    fn nobody_tunnels_through_anybody_at_stim_speed_or_far_beyond_it() {
        let level = bundled();
        for speed in [WALK * STIM_PACE, 3., 12.] {
            for offset in [0., 0.02, 0.05] {
                let mut world = World::default();
                world.add(Actor::hotdog(1, on(&level, -0.25, 2.4))).wish = [speed, 0.];
                world.add(Actor::hotdog(2, on(&level, 0.25 + speed * 0.003, 2.4 + offset))).wish = [-speed, 0.];
                run(&mut world, &level, 20, |world, _| {
                    assert!(tightest(world) >= -1e-9, "overlap of {} at {speed} m/s", -tightest(world));
                    // Head on they stop each other; off centre they may slide round, which is not passing through.
                    if offset == 0. {
                        assert!(actor(world, 1).p[0] < actor(world, 2).p[0], "passed through each other at {speed} m/s");
                    }
                });
            }
        }
        // A thrown character is stopped by a bystander too.
        let mut world = World::default();
        world.add(Actor::hotdog(1, on(&level, -0.4, 2.4))).apply_impulse([4. * HOTDOG_MASS, 0., 0.]);
        world.add(Actor::hotdog(2, on(&level, 0., 2.4)));
        run(&mut world, &level, 60, |world, _| assert!(tightest(world) >= -1e-9 && actor(world, 1).p[0] < actor(world, 2).p[0]));
        assert!(actor(&world, 2).p[0] > 0.05, "and passes its momentum on");
    }

    #[test]
    fn a_moving_character_pushes_a_lighter_idle_one_and_is_slowed_in_proportion_to_their_masses() {
        let level = bundled();
        let pair = |pusher: f64, idle: f64| {
            let mut world = World::default();
            let a = world.add(Actor { mass: pusher, ..Actor::hotdog(1, on(&level, -0.2, 2.4)) });
            a.wish = [WALK, 0.];
            world.add(Actor { mass: idle, ..Actor::hotdog(2, on(&level, -0.2 + 2. * RADIUS, 2.4)) });
            run(&mut world, &level, 30, |world, _| assert!(tightest(world) >= -1e-9));
            world
        };
        let (world, share) = (pair(0.6, 0.2), 0.6 / (0.6 + 0.2));
        near(actor(&world, 1).v[0], WALK * share, 1e-9, "the pusher is slowed");
        near(actor(&world, 2).v[0], WALK * share, 1e-9, "the idle one is carried along");
        near(flat(actor(&world, 1), actor(&world, 2)), 2. * RADIUS, 1e-9, "still touching");
        // The other way round the heavy one barely gives.
        near(actor(&pair(0.2, 0.6), 2).v[0], WALK * 0.25, 1e-9, "a light pusher against a heavy body");
    }

    #[test]
    fn a_character_pinned_against_a_block_is_not_pushed_into_it_the_pushers_go_round_it_or_stop() {
        let level = bundled();
        let (mut world, wall) = (World::default(), -0.45 - MARGIN);
        let start = on(&level, wall - 1e-6, 2.8);
        world.add(Actor::hotdog(1, start));
        for id in 2..=4 {
            world.add(Actor { mass: 2., ..Actor::hotdog(id, on(&level, wall - 0.08 * (id - 1) as f64 - 0.02, 2.8)) }).wish = [WALK, 0.];
        }
        let mut late = 0f64;
        run(&mut world, &level, 150, |world, tick| {
            assert!(walkable(&level, actor(world, 1).p), "pushed into the block: {:?}", actor(world, 1).p);
            assert!(tightest(world) >= -1e-9);
            if tick > 120 {
                for id in 2..=4 {
                    late = late.max(actor(world, id).v[0].abs() + actor(world, id).v[2].abs());
                }
            }
        });
        // It did not move (a few micrometres along the wall, the solver's rounding at the walking pace, are nothing).
        let moved = ((actor(&world, 1).p[0] - start[0]).powi(2) + (actor(&world, 1).p[2] - start[2]).powi(2)).sqrt();
        assert!(moved < 1e-5 && actor(&world, 1).p[1] == start[1], "it moved {moved} m: {:?}", actor(&world, 1).p);
        // Unable to push it, the three stepped round it to the wall on either side, and there they stopped.
        assert!(late < 1e-3, "pushers still moving at {late} m/s");
        let pushers: Vec<[f64; 3]> = (2..=4).map(|id| actor(&world, id).p).collect();
        for p in &pushers {
            assert!(p[0] > wall - 0.1 && p[0] < wall && (p[2] - 2.8).abs() < 0.2, "a pusher ended at {p:?}");
        }
        assert!(pushers.iter().any(|p| p[2] > 2.8) && pushers.iter().any(|p| p[2] < 2.8), "odd ids pass on one side, even ids on the other");
        // The same holds at the counter's edge.
        let (mut edge, corner) = (World::default(), on(&level, level.x1 - 1e-6, 2.4));
        edge.add(Actor::hotdog(1, corner));
        edge.add(Actor { mass: 5., ..Actor::hotdog(2, on(&level, level.x1 - 0.2, 2.4)) }).wish = [WALK, 0.];
        run(&mut edge, &level, 60, |world, _| assert!(tightest(world) >= -1e-9));
        assert!(actor(&edge, 1).p == corner && actor(&edge, 2).p[0] < level.x1);
    }

    /// One hotdog in the open with ten zombies closing in from all round, each always walking straight at
    /// it. `order` is the order the characters are added in.
    fn siege(level: &Level, order: [usize; 11], ticks: usize, mut each: impl FnMut(&World, usize)) -> World {
        let ring = [[0.45, 0.], [0.36, 0.27], [0.14, 0.43], [-0.14, 0.43], [-0.36, 0.27], [-0.45, 0.], [-0.36, -0.27], [-0.14, -0.43], [0.14, -0.43], [0.36, -0.27]];
        let mut made = vec![Actor::hotdog(1, on(level, 0.2, 3.15))];
        for (i, [x, z]) in ring.into_iter().enumerate() {
            made.push(Actor::zombie(10 + i as u32, on(level, 0.2 + x, 3.15 + z)));
        }
        let mut world = World::default();
        for index in order {
            world.add(made[index].clone());
        }
        for tick in 1..=ticks {
            let target = actor(&world, 1).p;
            for z in world.actors.iter_mut().filter(|z| z.id >= 10) {
                let (dx, dz) = (target[0] - z.p[0], target[2] - z.p[2]);
                let length = (dx * dx + dz * dz).sqrt();
                z.wish = [dx / length * ZOMBIE_WALK, dz / length * ZOMBIE_WALK];
            }
            world.step(level, DT, &mut NoProps);
            each(&world, tick);
        }
        world
    }
    const IN_ORDER: [usize; 11] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

    #[test]
    fn ten_zombies_converging_on_one_hotdog_settle_into_a_ring_without_overlap_jitter_or_energy_from_nowhere() {
        let level = bundled();
        let walking = 10. * 0.5 * ZOMBIE_MASS * ZOMBIE_WALK * ZOMBIE_WALK;
        let (mut worst, mut peak, mut late, mut drift) = (f64::INFINITY, 0f64, 0f64, 0f64);
        let mut marks: Vec<[f64; 3]> = vec![];
        let world = siege(&level, IN_ORDER, 300, |world, tick| {
            worst = worst.min(tightest(world));
            peak = peak.max(energy(world));
            if tick == 240 {
                marks = world.actors.iter().map(|a| a.p).collect();
            }
            if tick > 240 {
                late = late.max(energy(world));
                for (a, mark) in world.actors.iter().zip(&marks) {
                    drift = drift.max(((a.p[0] - mark[0]).powi(2) + (a.p[2] - mark[2]).powi(2)).sqrt());
                }
            }
        });
        assert!(worst >= -1e-9, "characters overlapped by {}", -worst);
        assert!(peak <= walking * 1.0001, "kinetic energy rose to {peak}; ten walking zombies carry {walking}");
        assert!(late <= walking * 0.02, "still moving after eight seconds: {late} against {walking}");
        assert!(drift < 0.01, "the ring shifted {drift} m in its last two seconds");
        // Ten rigid bodies shoulder to shoulder make a ring of radius 0.03 / sin(18°) = 0.097 m: they jam like an arch.
        let target = actor(&world, 1);
        for z in world.actors.iter().filter(|z| z.id >= 10) {
            assert!(flat(z, target) >= 2. * RADIUS - 1e-9 && flat(z, target) < 0.105, "zombie {} is {} m away", z.id, flat(z, target));
        }
        near(target.p[0], 0.2, 0.01, "the hotdog was not carried off");
        near(target.p[2], 3.15, 0.01, "the hotdog was not carried off");
    }

    #[test]
    fn a_crowd_repeats_exactly_whatever_order_the_characters_were_added_in() {
        let level = bundled();
        let first = siege(&level, IN_ORDER, 300, |_, _| {}).actors;
        assert_eq!(siege(&level, IN_ORDER, 300, |_, _| {}).actors, first, "the same run twice");
        assert_eq!(siege(&level, [10, 3, 7, 0, 9, 1, 5, 8, 2, 6, 4], 300, |_, _| {}).actors, first, "shuffled insertion");
        assert_eq!(first.iter().map(|a| a.id).collect::<Vec<_>>(), [1, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);
    }

    #[test]
    fn hotdogs_sharing_a_start_are_parted_one_landing_on_another_slides_off_and_only_a_stim_jump_clears_a_body() {
        let level = bundled();
        let (mut world, start) = (World::default(), level.start_position(0));
        for id in 1..=5 {
            world.add(Actor::hotdog(id, start));
        }
        world.step(&level, DT, &mut NoProps);
        assert!(tightest(&world) >= -0.005, "one step leaves {} m of overlap", -tightest(&world));
        run(&mut world, &level, 6, |_, _| {});
        assert!(tightest(&world) >= -1e-9, "still overlapping by {}", -tightest(&world));
        for a in &world.actors {
            assert!(walkable(&level, a.p) && flat(a, &Actor::hotdog(0, start)) < 0.12);
        }
        // Coming down on someone: land on the head, slide off it, drop to the ground beside them.
        let mut drop = World::default();
        drop.add(Actor::hotdog(1, on(&level, 0., 3.)));
        drop.add(Actor::hotdog(2, above(&level, 0.01, 3., 0.2)));
        let mut perched = 0;
        run(&mut drop, &level, 30, |world, _| {
            assert!(tightest(world) >= -1e-9);
            if actor(world, 2).perch.is_some() {
                perched += 1;
                assert_eq!(actor(world, 2).p[1], FLOOR + HEIGHT);
            }
        });
        assert!((4..=8).contains(&perched), "stood on the head for {perched} steps");
        let (below, over) = (actor(&drop, 1), actor(&drop, 2));
        assert!(over.p[1] == FLOOR && over.grounded, "nobody stays on a head");
        assert!(flat(over, below) >= 2. * RADIUS - 1e-9 && flat(over, below) < 2. * RADIUS + 0.02 && over.p[0] > below.p[0]);
        assert_eq!(below.p, on(&level, 0., 3.), "and the one below is not moved");
        // A plain jump does not reach a head; a stim jump does, and clears it.
        for (speed, clears) in [(JUMP, false), (STIM_JUMP, true)] {
            let mut hop = World::default();
            hop.add(Actor::zombie(1, on(&level, 0., 3.)));
            let jumper = hop.add(Actor::hotdog(2, on(&level, -0.15, 3.)));
            jumper.wish = [WALK, 0.];
            jumper.jump = speed;
            run(&mut hop, &level, 40, |world, _| assert!(tightest(world) >= -1e-9));
            assert_eq!(actor(&hop, 2).p[0] > actor(&hop, 1).p[0], clears, "take-off at {speed} m/s");
            assert_eq!(actor(&hop, 2).p[1], FLOOR);
        }
        // Different storeys do not collide: one on the deck, one on the counter below it.
        let mut storeys = World::default();
        storeys.add(Actor::hotdog(1, on(&level, 1.26, 3.3)));
        storeys.add(Actor::hotdog(2, [1.24, FLOOR, 3.3]));
        run(&mut storeys, &level, 5, |_, _| {});
        assert_eq!([actor(&storeys, 1).p[0], actor(&storeys, 2).p[0]], [1.26, 1.24]);
    }

    #[test]
    fn a_steered_hotdog_walks_at_once_and_pushes_its_neighbours_but_never_through_one_or_into_a_wall() {
        let level = bundled();
        // walk() is one character's share of a step, taken now: the same push, the same share of the masses.
        let mut world = World::default();
        world.add(Actor { mass: 0.6, ..Actor::hotdog(1, on(&level, -0.2, 2.4)) });
        world.add(Actor { mass: 0.2, ..Actor::hotdog(2, on(&level, -0.2 + 2. * RADIUS, 2.4)) });
        let stride = WALK * DT;
        let at = world.walk(&level, 1, stride, 0.).unwrap();
        near(at[0], -0.2 + stride * 0.75, 1e-12, "the walker is slowed by its share");
        near(actor(&world, 2).p[0], -0.2 + 2. * RADIUS + stride * 0.75, 1e-12, "the neighbour is pushed ahead");
        assert!(world.walk(&level, 9, stride, 0.).is_none());
        // Walking into a wall slides, as in a step.
        let mut alone = World::default();
        alone.add(Actor::hotdog(1, on(&level, -0.49, 2.75)));
        let at = alone.walk(&level, 1, 0.02, 0.02).unwrap();
        assert!(at[0] < -0.48 && at[0] > -0.4801 && at[2] == 2.75 + 0.02, "{at:?}");
        // A neighbour with a wall behind it cannot give way: the walker stops at contact, or goes round.
        let (mut world, wall) = (World::default(), -0.45 - MARGIN);
        let pinned = on(&level, wall - 1e-6, 2.8);
        world.add(Actor::hotdog(1, on(&level, wall - 0.07, 2.8)));
        world.add(Actor::zombie(2, pinned));
        for _ in 0..5 {
            world.walk(&level, 1, 0.02, 0.);
            assert!(tightest(&world) >= -1e-9 && actor(&world, 2).p == pinned, "{:?}", actor(&world, 1).p);
        }
        // Nobody is passed through, however far one walk reaches.
        let mut world = World::default();
        world.add(Actor::hotdog(1, on(&level, 0., 2.4)));
        world.add(Actor::zombie(2, on(&level, 0.1, 2.4)));
        world.walk(&level, 1, 0.25, 0.);
        assert!(tightest(&world) >= -1e-9 && actor(&world, 1).p[0] < actor(&world, 2).p[0]);
    }

    // ----- Impulses

    #[test]
    fn a_blast_throws_characters_outward_less_with_distance_and_they_slide_and_come_to_rest() {
        let level = bundled();
        let (mut world, center, gaps) = (World::default(), [0., FLOOR + 0.02, 2.5], [0.06, 0.14, 0.22, 0.34]);
        for (i, gap) in gaps.iter().enumerate() {
            world.add(Actor::hotdog(i as u32 + 1, on(&level, *gap, 2.5)));
        }
        world.add(Actor::hotdog(9, on(&level, 0.46, 2.69)));
        world.add(Actor::hotdog(10, on(&level, 0.46, 2.25)));
        assert_eq!(world.blast(&level, center, BLAST_RADIUS, BLAST_IMPULSE, BLAST_LIFT), [1, 2, 3]);
        assert_eq!(world.blast(&level, [0.46, FLOOR + 0.1, 2.42], 0.2, 0.001, BLAST_LIFT), [10], "a block shields the hotdog behind it");
        world.remove(10);
        for id in 1..=3 {
            let v = actor(&world, id);
            assert!(v.launched && !v.grounded && v.knock[0] > 0. && v.v[1] > 0.);
        }
        assert!(actor(&world, 1).knock[0] > actor(&world, 2).knock[0] && actor(&world, 2).knock[0] > actor(&world, 3).knock[0], "falloff");
        assert!(!actor(&world, 4).launched && !actor(&world, 9).launched, "out of reach, and shielded");
        // Thrown characters steer poorly until they stop.
        wish(&mut world, 1, [0., WALK]);
        let (mut flew, mut rested) = (false, 0);
        for tick in 1..=120 {
            world.step(&level, DT, &mut NoProps);
            flew |= actor(&world, 1).p[1] > FLOOR + 0.01;
            assert!(tightest(&world) >= -1e-9);
            if (1..=4).all(|id| !actor(&world, id).launched) {
                rested = tick;
                break;
            }
        }
        assert!(flew, "the nearest one left the ground");
        assert!(rested > 5 && rested < 90, "came to rest after {rested} steps");
        world.step(&level, DT, &mut NoProps);
        let travelled: Vec<f64> = (0..4).map(|i| actor(&world, i as u32 + 1).p[0] - gaps[i]).collect();
        assert!(travelled[0] > travelled[1] && travelled[1] > travelled[2] && travelled[2] > 0.001 && travelled[3] == 0., "thrown {travelled:?}");
        assert!(travelled[0] > 0.15 && travelled[0] < 1., "the nearest was thrown {} m", travelled[0]);
        for id in 1..=4 {
            let v = actor(&world, id);
            assert!(v.grounded && v.knock == [0., 0.] && v.p[1] == FLOOR);
        }
        near(actor(&world, 1).v[2], WALK, 1e-9, "control is back once it has stopped");
        let steered = actor(&world, 1).p[2] - 2.5;
        assert!(steered > 0. && steered < rested as f64 * DT * WALK * 0.5, "steering while thrown is weak: {steered} m");
    }

    #[test]
    fn the_counter_edge_is_a_wall_by_default_and_with_the_open_edge_a_blast_can_carry_a_character_over_it() {
        let level = bundled();
        let launch = |open_edge: bool| {
            let mut world = World::new(Config { open_edge, ..Config::default() });
            world.add(Actor::hotdog(1, on(&level, 1.72, 2.4)));
            world.add(Actor::hotdog(2, on(&level, 1.7, 2.9))).wish = [WALK, 0.];
            world.blast(&level, [1.6, FLOOR + 0.02, 2.4], BLAST_RADIUS, 1.2, BLAST_LIFT);
            let (mut lowest, mut fell_at) = (f64::INFINITY, 0);
            run(&mut world, &level, 90, |world, tick| {
                lowest = lowest.min(actor(world, 1).p[1]);
                if actor(world, 1).fallen && fell_at == 0 {
                    fell_at = tick;
                }
            });
            (world, lowest, fell_at)
        };
        let (closed, lowest, _) = launch(false);
        let thrown = actor(&closed, 1);
        assert!(!thrown.fallen && thrown.grounded && thrown.p[1] == FLOOR && lowest == FLOOR);
        assert!(thrown.p[0] < level.x1 && thrown.p[0] > level.x1 - 1e-3, "stopped at the edge: {}", thrown.p[0]);
        let (mut opened, _, fell_at) = launch(true);
        let thrown = actor(&opened, 1);
        assert!(thrown.fallen && thrown.p[1] == 0. && thrown.p[0] > level.x1);
        let drop = (2. * FLOOR / GRAVITY).sqrt() / DT;
        assert!(fell_at as f64 > drop && (fell_at as f64) < drop + 20., "reached the kitchen floor after {fell_at} steps; the drop alone takes {drop}");
        // Walking cannot leave either way.
        for world in [&closed, &opened] {
            let walker = actor(world, 2);
            assert!(!walker.fallen && walker.p[0] < level.x1 && walker.p[0] > level.x1 - 1e-3 && walker.p[1] == FLOOR);
        }
        // A fallen character is out of the game: it stays put and nothing moves it.
        let rest = actor(&opened, 1).p;
        let gone = opened.get_mut(1).unwrap();
        gone.wish = [-1., 0.];
        gone.apply_impulse([1., 1., 1.]);
        run(&mut opened, &level, 10, |_, _| {});
        assert_eq!(actor(&opened, 1).p, rest);
    }

    #[test]
    fn hits_shove_and_an_impulse_on_an_airborne_character_adds_to_its_flight() {
        let level = bundled();
        let mut world = World::default();
        world.add(Actor::hotdog(1, on(&level, 0., 2.6))).apply_impulse([0.55 * HOTDOG_MASS, 0., 0.]);
        assert!(actor(&world, 1).grounded && actor(&world, 1).launched, "a level shove does not lift");
        let mut rested = 0;
        run(&mut world, &level, 60, |world, tick| {
            if rested == 0 && !actor(world, 1).launched {
                rested = tick;
            }
            assert_eq!(actor(world, 1).p[1], FLOOR);
        });
        let slowing = Config::default().friction * GRAVITY;
        near(actor(&world, 1).p[0], 0.55 * 0.55 / (2. * slowing), 0.55 * DT, "slid as far as friction allows");
        near(rested as f64 * DT, 0.55 / slowing, DT, "and for as long");
        world.get_mut(1).unwrap().jump = JUMP;
        world.step(&level, DT, &mut NoProps);
        let rising = actor(&world, 1).v[1];
        world.impulse(1, [0., 0.2 * HOTDOG_MASS, 0.]);
        near(actor(&world, 1).v[1], rising + 0.2, 1e-12, "lift adds to the flight");
    }

    // ----- The props hook

    /// A stub for the props core: a pan lying across x >= 0.2 that nobody passes, and a board 0.03 m thick to stand on.
    #[derive(Default)]
    struct Kitchenware {
        calls: Vec<(u32, f64, [f64; 3], bool, f64)>,
    }
    impl Props for Kitchenware {
        fn resolve(&mut self, body: &Body, _from: [f64; 3], velocity: [f64; 3], dt: f64) -> Touch {
            self.calls.push((body.id, dt, velocity, body.grounded, body.height));
            let on_board = body.p[0] >= -0.3 && body.p[0] <= -0.1 && body.p[2] >= 2.5 && body.p[2] <= 2.7;
            Touch { p: [body.p[0].min(0.2 - body.radius), body.p[1], body.p[2]], support: on_board.then_some(FLOOR + 0.03), impulse: None }
        }
    }
    /// A prop that answers with a fixed spot or a single shove.
    struct Fixed(Option<[f64; 3]>, Option<[f64; 3]>);
    impl Props for Fixed {
        fn resolve(&mut self, body: &Body, _from: [f64; 3], _velocity: [f64; 3], _dt: f64) -> Touch {
            Touch { p: self.0.unwrap_or(body.p), support: None, impulse: self.1.take() }
        }
    }

    #[test]
    fn the_props_hook_a_corrected_position_and_a_support_height_are_honoured_and_its_impulse_is_applied() {
        let level = bundled();
        let (mut world, mut props) = (World::default(), Kitchenware::default());
        world.add(Actor::hotdog(1, on(&level, 0., 2.6))).wish = [WALK, WALK * 0.5];
        for _ in 0..30 {
            world.step(&level, DT, &mut props);
            assert!(actor(&world, 1).p[0] <= 0.2 - RADIUS + 1e-12, "through the pan: {}", actor(&world, 1).p[0]);
        }
        near(actor(&world, 1).p[0], 0.2 - RADIUS, 1e-12, "held at the corrected position");
        near(actor(&world, 1).p[2], 2.6 + 30. * DT * WALK * 0.5, 1e-9, "and still free to slide along it");
        assert_eq!(props.calls[0], (1, DT, [WALK, 0., WALK * 0.5], true, HEIGHT));
        assert_eq!(props.calls.len(), 30, "called once per character per step");
        // Standing on the board: the feet follow the support height, and leave it at the board's edge.
        let a = world.get_mut(1).unwrap();
        a.p = on(&level, -0.05, 2.6);
        a.wish = [-WALK, 0.];
        let mut heights = vec![];
        for _ in 0..20 {
            world.step(&level, DT, &mut props);
            heights.push(actor(&world, 1).p[1]);
        }
        assert!(heights.contains(&(FLOOR + 0.03)), "stepped up onto the board");
        assert!(actor(&world, 1).p[1] == FLOOR && actor(&world, 1).p[0] < -0.3, "and back down past it");
        // A character leaning on a prop is not shoved into it by another character.
        let a = world.get_mut(1).unwrap();
        a.p = on(&level, 0.1, 2.4);
        a.wish = [0., 0.];
        world.add(Actor { mass: 5., ..Actor::hotdog(2, on(&level, -0.1, 2.4)) }).wish = [WALK, 0.];
        let (mut deepest, mut late) = (0f64, 0f64);
        for tick in 1..=60 {
            world.step(&level, DT, &mut props);
            assert!(tightest(&world) >= -1e-9);
            deepest = deepest.max(actor(&world, 1).p[0]);
            if tick > 30 {
                late = late.max(actor(&world, 1).v[0].abs() + actor(&world, 2).v[0].abs());
            }
        }
        near(actor(&world, 1).p[0], 0.2 - RADIUS, 1e-12, "held at the prop");
        assert!(deepest < 0.2 - RADIUS + WALK * DT, "pushed {} m into the prop", deepest - (0.2 - RADIUS));
        assert!(late < 1e-3, "and nobody is shuddering: {late} m/s");
        let gap = flat(actor(&world, 1), actor(&world, 2));
        assert!(gap < RADIUS * 2. + 0.005, "the pusher ended up beside it, against the pan too: {gap}");
        // A heavy prop shoving the character arrives as an impulse.
        let mut solo = World::default();
        solo.add(Actor::hotdog(1, on(&level, 0., 2.6)));
        solo.step(&level, DT, &mut Fixed(None, Some([0., 0.1, 0.2])));
        let victim = actor(&solo, 1);
        assert!(victim.launched && !victim.grounded);
        near(victim.knock[1], 0.2 / HOTDOG_MASS, 1e-12, "knocked along z");
        // The level has the last word over a correction.
        let (mut cornered, wall) = (World::default(), -0.45 - MARGIN);
        cornered.add(Actor::hotdog(1, on(&level, wall - 0.01, 2.8)));
        for _ in 0..5 {
            cornered.step(&level, DT, &mut Fixed(Some([-0.45, FLOOR, 2.8]), None));
            assert!(walkable(&level, actor(&cornered, 1).p));
        }
        assert!(actor(&cornered, 1).p[0] < wall && actor(&cornered, 1).p[0] > wall - 1e-4);
    }

    // ----- Cost

    /// Everyone after one hotdog, a blast every few seconds, pits beside the ramp and the deck: microseconds
    /// per step, and how many steps ended with an overlap and how deep it went. Steps are timed in runs of
    /// 100; the quickest run is the cost when nothing else is using the machine.
    fn crush(level: &Level, count: usize) -> (f64, usize, f64) {
        let (mut next, mut world) = (random(3), World::default());
        for i in 0..count {
            let p = loop {
                let p = on(level, level.x0 + 0.1 + next() * 3.4, level.z0 + 0.1 + next() * 1.1);
                if walkable(level, p) {
                    break p;
                }
            };
            world.add(if i % 6 == 0 { Actor::hotdog(i as u32 + 1, p) } else { Actor::zombie(i as u32 + 1, p) });
        }
        let (ticks, mut spent, mut quickest, mut overlapping, mut deepest) = (3300, 0., f64::INFINITY, 0, 0f64);
        for tick in 0..ticks {
            let prey = world.actors[0].p;
            for a in &mut world.actors {
                if a.id % 6 == 1 {
                    if tick % 30 == 0 {
                        let angle = next() * std::f64::consts::TAU;
                        a.wish = [angle.cos() * WALK, angle.sin() * WALK];
                    }
                } else {
                    let (dx, dz) = (prey[0] - a.p[0], prey[2] - a.p[2]);
                    let length = (dx * dx + dz * dz).sqrt().max(1e-9);
                    a.wish = [dx / length * ZOMBIE_WALK * 3., dz / length * ZOMBIE_WALK * 3.];
                }
            }
            if tick % 200 == 0 {
                world.blast(level, prey, 0.4, 0.3, BLAST_LIFT);
            }
            let began = Instant::now();
            world.step(level, DT, &mut NoProps);
            spent += began.elapsed().as_secs_f64();
            if tick % 100 == 99 {
                if tick >= 300 {
                    quickest = quickest.min(spent * 1e6 / 100.);
                }
                spent = 0.;
            }
            let gap = tightest(&world);
            if gap < -1e-6 {
                overlapping += 1;
                deepest = deepest.min(gap);
            }
            for a in &world.actors {
                assert!(walkable(level, a.p), "character {} at {:?}", a.id, a.p);
            }
        }
        (quickest, overlapping, deepest)
    }

    #[test]
    fn a_step_costs_microseconds_for_12_characters_and_stays_comfortable_for_64() {
        let level = bundled();
        let (twelve, many) = (crush(&level, 12), crush(&level, 64));
        let build = if cfg!(debug_assertions) { "unoptimised build" } else { "optimised build" };
        eprintln!("actor step: {:.2} µs for 12 characters, {:.2} µs for 64 ({build})", twelve.0, many.0);
        // Even this crush never overlaps deeply: a blast can lift one character up beside another, and characters
        // stacked in a pit narrower than a body can be squeezed a millimetre or two until the crowd thins.
        for (count, (_, overlapping, deepest)) in [(12, twelve), (64, many)] {
            eprintln!("  {count} characters: {overlapping} of 3300 steps ended with an overlap; the deepest was {:.2} mm", deepest.abs() * 1000.);
            assert!(deepest > -RADIUS, "{count} characters: {overlapping} overlapping steps, deepest {} m", -deepest);
        }
        // The budget is for an optimised build; an unoptimised one is allowed twenty times as long (and a slow or shared
        // machine as many times as HOTDOG_TIMING_SLACK says).
        let slack = if cfg!(debug_assertions) { 20. } else { 1. } * crate::timing_slack();
        assert!(twelve.0 < 50. * slack, "12 characters took {} µs a step", twelve.0);
        assert!(many.0 < 500. * slack, "64 characters took {} µs a step", many.0);
    }

    #[test]
    fn the_constants_are_the_games_own() {
        assert_eq!([RADIUS, HEIGHT, PRONE_HEIGHT, PRONE_PACE, JUMP, GRAVITY], [MARGIN, 0.15, 0.06, 0.4, 0.8, 2.8]);
        assert_eq!([CROUCH_HEIGHT, CROUCH_PACE], [0.10, 0.65]);
        near(STIM_JUMP, launch_speed(0.194, GRAVITY), 1e-4, "the stim jump keeps its old apex");
        const { assert!(ZOMBIE_MASS < HOTDOG_MASS) };
        // Zombies bite within 0.09 m, so two bodies in contact must be closer than that.
        const { assert!(2. * RADIUS < 0.09) };
        // A plain jump must not reach a standing character's head; a stim jump must.
        const { assert!(JUMP * JUMP / (2. * GRAVITY) < HEIGHT - HEAD_STEP && STIM_JUMP * STIM_JUMP / (2. * GRAVITY) > HEIGHT) };
        // The f32 conveniences only convert.
        let mut a = Actor::zombie(3, [1., 2., 3.]);
        a.steer(Vec2::new(0.5, -0.25));
        a.place(Vec3::new(4., 5., 6.));
        assert_eq!((a.position(), a.velocity(), a.wish, a.mass), (Vec3::new(4., 5., 6.), Vec3::ZERO, [0., 0.], ZOMBIE_MASS));
    }
}

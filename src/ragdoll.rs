// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! A small articulated-body solver: rigid bodies with mass and inertia, joined by ball and hinge
//! joints with limits, falling under gravity onto static boxes and ramps. Nothing here knows about
//! hotdogs; `death` builds one out of it, and anything else with bones could.
//!
//! The method is extended position-based dynamics with small substeps and one pass over the
//! constraints per substep: positions are integrated, joints and contacts move them back to where
//! they are allowed to be, and velocities are read off the result. It cannot blow up the way a force
//! solver does, which matters for bodies a few centimetres long. Every buffer is sized when a
//! `Ragdoll` is built, so stepping one allocates nothing, and a ragdoll that has come to rest
//! sleeps and costs nothing.
use bevy::math::{Quat, Vec3};
use std::f32::consts::{PI, TAU};

/// A box of scenery, axis aligned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub min: Vec3,
    pub max: Vec3,
}
/// A ramp filling the lower part of a box: its top climbs from `min.y` to `max.y` along x or z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wedge {
    pub min: Vec3,
    pub max: Vec3,
    pub along_z: bool,
    /// Climbs towards + along its axis (otherwise towards -).
    pub rising: bool,
}
/// Everything a ragdoll can land on or bump into.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub blocks: Vec<Block>,
    pub wedges: Vec<Wedge>,
}
impl Block {
    /// How a sphere overlaps the box: the way out and how far.
    fn touch(&self, c: Vec3, r: f32) -> Option<(Vec3, f32)> {
        let gap = c - c.clamp(self.min, self.max);
        let d2 = gap.length_squared();
        if d2 > r * r {
            return None;
        }
        if d2 > 1e-12 {
            let d = d2.sqrt();
            return Some((gap / d, r - d));
        }
        // The centre is inside: leave by the nearest face.
        let exits = [
            (c.x - self.min.x, Vec3::NEG_X),
            (self.max.x - c.x, Vec3::X),
            (c.y - self.min.y, Vec3::NEG_Y),
            (self.max.y - c.y, Vec3::Y),
            (c.z - self.min.z, Vec3::NEG_Z),
            (self.max.z - c.z, Vec3::Z),
        ];
        let mut best = exits[0];
        for exit in exits {
            if exit.0 < best.0 {
                best = exit;
            }
        }
        Some((best.1, best.0 + r))
    }
}
impl Wedge {
    /// Height of the slope above a point of the footprint, and the slope's unit normal.
    fn slope(&self, x: f32, z: f32) -> (f32, Vec3) {
        let (run, t) = if self.along_z {
            (self.max.z - self.min.z, (z - self.min.z) / (self.max.z - self.min.z))
        } else {
            (self.max.x - self.min.x, (x - self.min.x) / (self.max.x - self.min.x))
        };
        let climb = self.max.y - self.min.y;
        let sign = if self.rising { 1. } else { -1. };
        let t = if self.rising { t } else { 1. - t };
        let across = -climb * sign;
        let normal = if self.along_z { Vec3::new(0., run, across) } else { Vec3::new(across, run, 0.) };
        (self.min.y + t.clamp(0., 1.) * climb, normal.normalize())
    }
    fn touch(&self, c: Vec3, r: f32) -> Option<(Vec3, f32)> {
        let (x, z) = (c.x.clamp(self.min.x, self.max.x), c.z.clamp(self.min.z, self.max.z));
        let (top, normal) = self.slope(x, z);
        if x != c.x || z != c.z {
            // Beside the ramp: its nearest edge, as high as the slope stands there.
            let gap = c - Vec3::new(x, c.y.clamp(self.min.y, top), z);
            let d2 = gap.length_squared();
            if d2 > r * r || d2 < 1e-12 {
                return None;
            }
            let d = d2.sqrt();
            return Some((gap / d, r - d));
        }
        if c.y < self.min.y - r {
            return None;
        }
        let above = (c.y - top) * normal.y;
        if above > r {
            return None;
        }
        // Over or inside the ramp: out through the slope, unless a side or the tall end is nearer.
        let mut best = (r - above, normal);
        if above < 0. {
            let tall = match (self.along_z, self.rising) {
                (true, true) => (self.max.z - c.z, Vec3::Z),
                (true, false) => (c.z - self.min.z, Vec3::NEG_Z),
                (false, true) => (self.max.x - c.x, Vec3::X),
                (false, false) => (c.x - self.min.x, Vec3::NEG_X),
            };
            let sides = if self.along_z {
                [(c.x - self.min.x, Vec3::NEG_X), (self.max.x - c.x, Vec3::X)]
            } else {
                [(c.z - self.min.z, Vec3::NEG_Z), (self.max.z - c.z, Vec3::Z)]
            };
            for (depth, way) in [tall, sides[0], sides[1]] {
                if depth + r < best.0 {
                    best = (depth + r, way);
                }
            }
        }
        Some((best.1, best.0))
    }
}
impl Scene {
    /// Height of the highest surface at or below a point, or `None` over a drop.
    pub fn ground(&self, at: Vec3) -> Option<f32> {
        let (x, z) = (at.x, at.z);
        let mut top: Option<f32> = None;
        let mut raise = |y: f32| {
            if y <= at.y {
                top = Some(top.map_or(y, |t: f32| t.max(y)));
            }
        };
        for b in &self.blocks {
            if x >= b.min.x && x <= b.max.x && z >= b.min.z && z <= b.max.z {
                raise(b.max.y);
            }
        }
        for w in &self.wedges {
            if x >= w.min.x && x <= w.max.x && z >= w.min.z && z <= w.max.z {
                raise(w.slope(x, z).0);
            }
        }
        top
    }
    /// Whether a sphere is clear of everything, give or take `slack`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn clear(&self, c: Vec3, r: f32, slack: f32) -> bool {
        self.blocks.iter().all(|b| b.touch(c, r).is_none_or(|(_, depth)| depth <= slack))
            && self.wedges.iter().all(|w| w.touch(c, r).is_none_or(|(_, depth)| depth <= slack))
    }
}

/// One rigid body. `at` is its centre of mass.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub at: Vec3,
    pub turn: Quat,
    pub velocity: Vec3,
    /// Angular velocity, in world space.
    pub spin: Vec3,
    #[cfg_attr(not(test), allow(dead_code))]
    mass: f32,
    #[cfg_attr(not(test), allow(dead_code))]
    inertia: Vec3,
    inv_mass: f32,
    inv_inertia: Vec3,
    was_at: Vec3,
    was_turn: Quat,
    /// Where it was when the ragdoll last checked whether it had come to rest.
    mark_at: Vec3,
    mark_turn: Quat,
    /// How many of its spheres touched scenery in the last substep.
    touching: u8,
}
impl Body {
    /// A body of `mass` whose moments of inertia about its own x, y and z axes are `inertia`.
    pub fn new(at: Vec3, turn: Quat, mass: f32, inertia: Vec3) -> Self {
        Self {
            at,
            turn,
            velocity: Vec3::ZERO,
            spin: Vec3::ZERO,
            mass,
            inertia,
            inv_mass: 1. / mass,
            inv_inertia: inertia.recip(),
            was_at: at,
            was_turn: turn,
            mark_at: at,
            mark_turn: turn,
            touching: 0,
        }
    }
    /// Inertia of a solid cylinder or capsule lying along its own y axis. A thin rod spins about
    /// its length far more easily than a solver likes, so that moment is kept to a quarter of
    /// the others at least.
    pub fn rod(mass: f32, length: f32, radius: f32) -> Vec3 {
        let across = mass * (3. * radius * radius + length * length) / 12.;
        Vec3::new(across, (0.5 * mass * radius * radius).max(across * 0.25), across)
    }
    /// How many of its collision spheres touched scenery in the last substep.
    pub fn touching(&self) -> usize {
        self.touching as usize
    }
    /// A point of the body, given in its own frame, in the world.
    pub fn point(&self, local: Vec3) -> Vec3 {
        self.at + self.turn * local
    }
    /// How fast a point of the body, given in the world, is moving.
    pub fn velocity_at(&self, point: Vec3) -> Vec3 {
        self.velocity + self.spin.cross(point - self.at)
    }
    /// Kinetic energy plus height energy under `gravity`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn energy(&self, gravity: f32) -> f32 {
        let spin = self.turn.inverse() * self.spin;
        0.5 * self.mass * self.velocity.length_squared()
            + 0.5 * (spin * spin).dot(self.inertia)
            + self.mass * gravity * self.at.y
    }
    fn unresist(&self, v: Vec3) -> Vec3 {
        self.turn * ((self.turn.inverse() * v) * self.inv_inertia)
    }
    /// How readily the body gives way to a push along `n` at `arm` from its centre, or to a twist
    /// about `n` when there is no arm.
    fn give(&self, n: Vec3, arm: Option<Vec3>) -> f32 {
        let axis = self.turn.inverse() * arm.map_or(n, |arm| arm.cross(n));
        (axis * axis).dot(self.inv_inertia) + if arm.is_some() { self.inv_mass } else { 0. }
    }
    fn rotate(&mut self, by: Vec3) {
        // A constraint never needs to turn a body far in one substep; the cap keeps a bad one sane.
        let size = by.length();
        let by = if size > 0.5 { by * (0.5 / size) } else { by };
        let dq = Quat::from_xyzw(by.x, by.y, by.z, 0.) * self.turn;
        self.turn = Quat::from_xyzw(
            self.turn.x + 0.5 * dq.x,
            self.turn.y + 0.5 * dq.y,
            self.turn.z + 0.5 * dq.z,
            self.turn.w + 0.5 * dq.w,
        )
        .normalize();
    }
    fn push(&mut self, p: Vec3, arm: Vec3) {
        self.at += p * self.inv_mass;
        let by = self.unresist(arm.cross(p));
        self.rotate(by);
    }
}
/// A collision sphere fixed to a body, in the body's frame.
#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub body: usize,
    pub at: Vec3,
    pub radius: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Limit {
    /// Ball and socket: the child's x axis stays within `swing` radians of the parent's and the
    /// child may twist `twist` radians either way about it.
    Cone { swing: f32, twist: f32 },
    /// A hinge about the x axes, turning from `min` to `max` radians.
    Hinge { min: f32, max: f32 },
}
/// Holds a point of the child on a point of the parent and limits how the child may turn.
#[derive(Clone, Copy, Debug)]
pub struct Joint {
    pub parent: usize,
    pub child: usize,
    /// Where the joint is in the parent's frame and in the child's.
    pub at: [Vec3; 2],
    /// How the joint is turned in each: x is the cone or hinge axis, y the reference for angles.
    pub frame: [Quat; 2],
    pub limit: Limit,
    /// A muscle: the child is turned towards `pose` (its turn relative to the parent body) at
    /// `drive` per second. 0 leaves the joint limp.
    pub pose: Quat,
    pub drive: f32,
    /// The parent leads: the child follows it and never moves it. For limbs far lighter than
    /// what they hang from, whose joints would otherwise hold the heavy body up like soft springs.
    pub led: bool,
}
impl Joint {
    pub fn new(parent: usize, child: usize, at: [Vec3; 2], frame: [Quat; 2], limit: Limit) -> Self {
        Self { parent, child, at, frame, limit, pose: Quat::IDENTITY, drive: 0., led: false }
    }
    /// The same joint with the parent leading.
    pub fn led(self) -> Self {
        Self { led: true, ..self }
    }
    /// The hinge angle, or the swing away from the cone's axis.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn angle(&self, bodies: &[Body]) -> f32 {
        let (a, b) = (&bodies[self.parent], &bodies[self.child]);
        match self.limit {
            Limit::Cone { .. } => (a.turn * self.frame[0] * Vec3::X).angle_between(b.turn * self.frame[1] * Vec3::X),
            Limit::Hinge { .. } => {
                let n = a.turn * self.frame[0] * Vec3::X;
                let (ya, yb) = (a.turn * self.frame[0] * Vec3::Y, b.turn * self.frame[1] * Vec3::Y);
                ya.cross(yb).dot(n).atan2(ya.dot(yb))
            }
        }
    }
    /// Turns the two bodies until the joint is within its limits.
    fn bend(&self, a: &mut Body, b: &mut Body, h: f32) {
        let j = self;
        let axis = |body: &Body, frame: Quat, v: Vec3| body.turn * (frame * v);
        match j.limit {
            Limit::Hinge { min, max } => {
                let by = axis(a, j.frame[0], Vec3::X).cross(axis(b, j.frame[1], Vec3::X));
                pair_turn(a, b, by, j.led);
                let n = axis(a, j.frame[0], Vec3::X);
                let (ya, yb) = (axis(a, j.frame[0], Vec3::Y), axis(b, j.frame[1], Vec3::Y));
                limit_angle(a, b, n, ya, yb, min, max, PI, j.led);
            }
            Limit::Cone { swing, twist } => {
                let (xa, xb) = (axis(a, j.frame[0], Vec3::X), axis(b, j.frame[1], Vec3::X));
                let n = xa.cross(xb);
                if n.length_squared() > 1e-10 {
                    limit_angle(a, b, n.normalize(), xa, xb, -swing, swing, PI, j.led);
                }
                let (xa, xb) = (axis(a, j.frame[0], Vec3::X), axis(b, j.frame[1], Vec3::X));
                let n = (xa + xb).normalize_or_zero();
                let flat = |v: Vec3| (v - n * n.dot(v)).normalize_or_zero();
                let (ya, yb) = (flat(axis(a, j.frame[0], Vec3::Y)), flat(axis(b, j.frame[1], Vec3::Y)));
                if n != Vec3::ZERO && ya != Vec3::ZERO && yb != Vec3::ZERO {
                    // Folded right back on itself the twist is ill defined: go gently.
                    let cap = if xa.dot(xb) > -0.5 { TAU } else { h };
                    limit_angle(a, b, n, ya, yb, -twist, twist, cap, j.led);
                }
            }
        }
    }
    /// How far apart the two halves of the joint have drifted.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn gap(&self, bodies: &[Body]) -> f32 {
        bodies[self.parent].point(self.at[0]).distance(bodies[self.child].point(self.at[1]))
    }
}
/// An outside hand on a body: what an authored reaction, a leash or a puppeteer uses.
#[derive(Clone, Copy, Debug, Default)]
pub enum Servo {
    #[default]
    Idle,
    /// Pulls a point of the body (in its frame) towards a place in the world along the `axes` that
    /// are 1, at `rate` per second, once it is more than `slack` away.
    Hold { body: usize, point: Vec3, target: Vec3, axes: Vec3, slack: f32, rate: f32 },
    /// Turns the body towards an orientation in the world at `rate` per second.
    Face { body: usize, target: Quat, rate: f32 },
    /// Turns the body so that one of its own directions points along a direction of the world,
    /// leaving it free to spin about that: what keeps a top upright.
    Level { body: usize, axis: Vec3, target: Vec3, rate: f32 },
}
/// How a ragdoll moves. Lengths are metres, times seconds.
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    /// Downward acceleration.
    pub gravity: f32,
    /// Length of one substep, and the most substeps one call to `advance` will run.
    pub step: f32,
    pub most: usize,
    /// Times the joints are gone over in a substep, 1 or 2. Twice holds whipping limbs tighter.
    pub passes: usize,
    pub friction: f32,
    pub bounce: f32,
    /// Fraction of speed and of spin lost per second in the air, of spin while lying on scenery
    /// (what stops a sausage rolling for ever), and of the turning between jointed bodies.
    pub drag: f32,
    pub spin_drag: f32,
    pub roll_drag: f32,
    pub joint_drag: f32,
    /// The fastest a body is pushed back out of scenery it is sunk in, in metres per second: a
    /// limb that starts inside a wall is eased out rather than fired out.
    pub recover: f32,
    /// A ragdoll none of whose bodies has moved faster than this, in metres and radians per
    /// second, over `rest_time` falls asleep. Judged over the whole time, so limbs wedged between
    /// two constraints, which twitch within a substep and go nowhere, do not keep it awake.
    pub rest_speed: f32,
    pub rest_spin: f32,
    pub rest_time: f32,
}
impl Default for Tuning {
    fn default() -> Self {
        Self {
            gravity: 9.81,
            step: 1. / 300.,
            most: 12,
            passes: 2,
            friction: 0.6,
            bounce: 0.2,
            drag: 0.2,
            spin_drag: 0.6,
            roll_drag: 6.,
            joint_drag: 30.,
            recover: 0.6,
            rest_speed: 0.008,
            rest_spin: 0.6,
            rest_time: 0.35,
        }
    }
}
#[derive(Clone, Copy)]
struct Contact {
    body: usize,
    /// The touching point in the body's frame.
    local: Vec3,
    normal: Vec3,
    /// Speed along the normal before the substep moved anything.
    approach: f32,
}
/// A set of jointed bodies.
#[derive(Clone)]
pub struct Ragdoll {
    pub bodies: Vec<Body>,
    pub balls: Vec<Ball>,
    pub joints: Vec<Joint>,
    pub servos: Vec<Servo>,
    contacts: Vec<Contact>,
    near: Vec<u32>,
    owed: f32,
    still: f32,
    asleep: bool,
}
fn pair(bodies: &mut [Body], a: usize, b: usize) -> (&mut Body, &mut Body) {
    if a < b {
        let (low, high) = bodies.split_at_mut(b);
        (&mut low[a], &mut high[0])
    } else {
        let (low, high) = bodies.split_at_mut(a);
        (&mut high[0], &mut low[b])
    }
}
/// Turns `a` by its share of the rotation `by` and `b` the other way by its share; all of it
/// falls to `b` when `a` leads.
fn pair_turn(a: &mut Body, b: &mut Body, by: Vec3, led: bool) {
    let size = by.length();
    if size < 1e-9 {
        return;
    }
    let n = by / size;
    let give = if led { 0. } else { a.give(n, None) } + b.give(n, None);
    let p = n * (size / give);
    if !led {
        let by = a.unresist(p);
        a.rotate(by);
    }
    let by = b.unresist(-p);
    b.rotate(by);
}
/// Closes the gap `by` between a point of `a` and a point of `b`, each moving its share.
fn pair_push(a: &mut Body, b: &mut Body, by: Vec3, at_a: Vec3, at_b: Vec3, led: bool) {
    let size = by.length();
    if size < 1e-9 {
        return;
    }
    let n = by / size;
    let (arm_a, arm_b) = (at_a - a.at, at_b - b.at);
    let p = n * (size / (if led { 0. } else { a.give(n, Some(arm_a)) } + b.give(n, Some(arm_b))));
    if !led {
        a.push(p, arm_a);
    }
    b.push(-p, arm_b);
}
/// Keeps the angle from `ra` to `rb` about `n` between `min` and `max`.
#[allow(clippy::too_many_arguments)]
fn limit_angle(a: &mut Body, b: &mut Body, n: Vec3, ra: Vec3, rb: Vec3, min: f32, max: f32, cap: f32, led: bool) {
    let mut phi = ra.cross(rb).dot(n).clamp(-1., 1.).asin();
    if ra.dot(rb) < 0. {
        phi = PI - phi;
    }
    if phi > PI {
        phi -= TAU;
    }
    if phi < min || phi > max {
        let mut by = (Quat::from_axis_angle(n, phi.clamp(min, max)) * ra).cross(rb);
        let size = by.length();
        if size > cap {
            by *= cap / size;
        }
        pair_turn(a, b, by, led);
    }
}
/// The rotation `q` as an axis scaled by its angle, the short way round.
fn rotation_vector(q: Quat) -> Vec3 {
    let q = if q.w < 0. { -q } else { q };
    let sine = q.xyz().length();
    if sine < 1e-6 {
        return q.xyz() * 2.;
    }
    q.xyz() * (2. * sine.atan2(q.w) / sine)
}
impl Ragdoll {
    pub fn new(bodies: Vec<Body>, balls: Vec<Ball>, joints: Vec<Joint>, servos: usize) -> Self {
        let contacts = Vec::with_capacity(balls.len() * 3);
        Self {
            bodies,
            balls,
            joints,
            servos: vec![Servo::Idle; servos],
            contacts,
            near: Vec::new(),
            owed: 0.,
            still: 0.,
            asleep: false,
        }
    }
    pub fn asleep(&self) -> bool {
        self.asleep
    }
    pub fn wake(&mut self) {
        self.asleep = false;
        self.still = 0.;
        for b in &mut self.bodies {
            (b.mark_at, b.mark_turn) = (b.at, b.turn);
        }
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn energy(&self, gravity: f32) -> f32 {
        self.bodies.iter().map(|b| b.energy(gravity)).sum()
    }
    /// Moves the ragdoll through `dt` seconds in whole substeps, carrying the remainder over.
    /// Returns how many substeps ran: none once it is asleep.
    pub fn advance(&mut self, dt: f32, scene: &Scene, tune: &Tuning) -> usize {
        if self.asleep {
            return 0;
        }
        self.owed += dt;
        let steps = ((self.owed / tune.step) as usize).min(tune.most);
        if steps == 0 {
            return 0;
        }
        self.owed = (self.owed - steps as f32 * tune.step).min(tune.step);
        self.survey(scene, steps as f32 * tune.step);
        for _ in 0..steps {
            self.step(scene, tune);
            if self.asleep {
                break;
            }
        }
        steps
    }
    /// Lists the scenery the ragdoll could reach in `time`, so each substep only tests that.
    fn survey(&mut self, scene: &Scene, time: f32) {
        let (mut low, mut high, mut fastest) = (Vec3::MAX, Vec3::MIN, 0f32);
        for ball in &self.balls {
            let centre = self.bodies[ball.body].point(ball.at);
            low = low.min(centre - ball.radius);
            high = high.max(centre + ball.radius);
        }
        for body in &self.bodies {
            fastest = fastest.max(body.velocity.length() + body.spin.length() * 0.1);
        }
        let reach = Vec3::splat(0.02 + (fastest + 1.) * time);
        let (low, high) = (low - reach, high + reach);
        let within = |min: Vec3, max: Vec3| min.cmple(high).all() && max.cmpge(low).all();
        self.near.clear();
        // Only grows while a ragdoll meets more scenery than it ever has before.
        self.near.reserve(scene.blocks.len() + scene.wedges.len());
        for (i, b) in scene.blocks.iter().enumerate() {
            if within(b.min, b.max) {
                self.near.push(i as u32);
            }
        }
        for (i, w) in scene.wedges.iter().enumerate() {
            if within(w.min, w.max) {
                self.near.push((scene.blocks.len() + i) as u32);
            }
        }
    }
    /// One substep.
    pub fn step(&mut self, scene: &Scene, tune: &Tuning) {
        let h = tune.step;
        for b in &mut self.bodies {
            b.was_at = b.at;
            b.was_turn = b.turn;
            b.touching = 0;
            b.velocity.y -= tune.gravity * h;
            b.at += b.velocity * h;
            let by = b.spin * h;
            b.rotate(by);
        }
        for servo in &self.servos {
            match *servo {
                Servo::Idle => {}
                Servo::Hold { body, point, target, axes, slack, rate } => {
                    let b = &mut self.bodies[body];
                    let from = b.point(point);
                    let gap = (target - from) * axes;
                    let far = gap.length();
                    if far > slack && far > 1e-9 {
                        let n = gap / far;
                        let arm = from - b.at;
                        let p = n * ((far - slack) * (1. - (-rate * h).exp()) / b.give(n, Some(arm)));
                        b.push(p, arm);
                    }
                }
                Servo::Face { body, target, rate } => {
                    let b = &mut self.bodies[body];
                    let by = rotation_vector(target * b.turn.inverse()) * (1. - (-rate * h).exp());
                    b.rotate(by);
                }
                Servo::Level { body, axis, target, rate } => {
                    let b = &mut self.bodies[body];
                    let by = rotation_vector(Quat::from_rotation_arc(b.turn * axis, target)) * (1. - (-rate * h).exp());
                    b.rotate(by);
                }
            }
        }
        self.contacts.clear();
        for ball in &self.balls {
            let body = &mut self.bodies[ball.body];
            for &i in &self.near {
                let centre = body.point(ball.at);
                let i = i as usize;
                let touch = match scene.blocks.get(i) {
                    Some(block) => block.touch(centre, ball.radius),
                    None => scene.wedges[i - scene.blocks.len()].touch(centre, ball.radius),
                };
                let Some((n, depth)) = touch else {
                    continue;
                };
                // What it sank this substep is undone at once; what it was already sunk is eased out.
                let before = body.was_at + body.was_turn * ball.at;
                let sunk = match scene.blocks.get(i) {
                    Some(block) => block.touch(before, ball.radius),
                    None => scene.wedges[i - scene.blocks.len()].touch(before, ball.radius),
                };
                let depth = depth.min((depth - sunk.map_or(0., |(_, sunk)| sunk)).max(0.) + tune.recover * h);
                let arm = centre - n * ball.radius - body.at;
                let local = body.turn.inverse() * arm;
                let approach = (body.velocity + body.spin.cross(arm)).dot(n);
                body.push(n * (depth / body.give(n, Some(arm))), arm);
                body.touching = body.touching.saturating_add(1);
                // Friction: the touching point may slide no further this substep than the grip of
                // the push that held it up allows.
                let slide = body.point(local) - (body.was_at + body.was_turn * local);
                let slide = slide - n * slide.dot(n);
                let far = slide.length();
                if far > 1e-9 {
                    let t = slide / far;
                    let arm = body.point(local) - body.at;
                    let back = far.min(tune.friction * depth);
                    body.push(-t * (back / body.give(t, Some(arm))), arm);
                }
                if self.contacts.len() < self.contacts.capacity() {
                    self.contacts.push(Contact { body: ball.body, local, normal: n, approach });
                }
            }
        }
        for j in self.joints.iter().chain(&self.joints).take(self.joints.len() * tune.passes) {
            let (a, b) = pair(&mut self.bodies, j.parent, j.child);
            j.bend(a, b, h);
            if j.drive > 0. {
                let by = rotation_vector((a.turn * j.pose) * b.turn.inverse()) * (1. - (-j.drive * h).exp());
                pair_turn(a, b, -by, j.led);
            }
            let (at_a, at_b) = (a.point(j.at[0]), b.point(j.at[1]));
            pair_push(a, b, at_b - at_a, at_a, at_b, j.led);
        }
        // Closing the joints turns the bones a little; the limits have the last word.
        for j in &self.joints {
            let (a, b) = pair(&mut self.bodies, j.parent, j.child);
            j.bend(a, b, h);
        }
        for b in &mut self.bodies {
            b.velocity = (b.at - b.was_at) / h;
            b.spin = rotation_vector(b.turn * b.was_turn.inverse()) / h;
        }
        // A contact leaves a body moving away at a fraction of the speed it arrived with, or as
        // fast as it was already leaving, and no faster: being pushed out of the scenery adds no
        // speed. Slow arrivals do not bounce at all, which is what lets things lie still.
        for c in &self.contacts {
            let b = &mut self.bodies[c.body];
            let arm = b.turn * c.local;
            let leaving = (b.velocity + b.spin.cross(arm)).dot(c.normal);
            let wanted = if c.approach < -4. * tune.gravity * h { -c.approach * tune.bounce } else { c.approach.max(0.) };
            let change = wanted - leaving;
            let impulse = c.normal * (change / b.give(c.normal, Some(arm)));
            b.velocity += impulse * b.inv_mass;
            b.spin += b.unresist(arm.cross(impulse));
        }
        for j in &self.joints {
            let (a, b) = pair(&mut self.bodies, j.parent, j.child);
            let relative = b.spin - a.spin;
            let size = relative.length();
            if size > 1e-6 {
                let n = relative / size;
                let p = n * (size * (tune.joint_drag * h).min(1.) / (if j.led { 0. } else { a.give(n, None) } + b.give(n, None)));
                if !j.led {
                    a.spin += a.unresist(p);
                }
                b.spin -= b.unresist(p);
            }
        }
        let (slow, turn, roll) = ((-tune.drag * h).exp(), (-tune.spin_drag * h).exp(), (-tune.roll_drag * h).exp());
        for b in &mut self.bodies {
            b.velocity *= slow;
            // Lying on two points or more it is rolling or rocking, which dies away; balanced on
            // one it is toppling, which must not be slowed.
            b.spin *= if b.touching > 1 { turn * roll } else { turn };
        }
        self.still += h;
        if self.still >= tune.rest_time {
            let (far, round) = (tune.rest_speed * self.still, tune.rest_spin * self.still);
            self.asleep = self.bodies.iter().all(|b| b.at.distance(b.mark_at) < far && b.turn.angle_between(b.mark_turn) < round);
            self.still = 0.;
            for b in &mut self.bodies {
                (b.mark_at, b.mark_turn) = (b.at, b.turn);
                if self.asleep {
                    (b.velocity, b.spin) = (Vec3::ZERO, Vec3::ZERO);
                }
            }
        }
    }
}

/// Counts this thread's allocations, so tests can show that stepping a ragdoll makes none.
#[cfg(test)]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    thread_local!(static COUNT: Cell<usize> = const { Cell::new(0) });
    struct Counting;
    fn count() {
        let _ = COUNT.try_with(|count| count.set(count.get() + 1));
    }
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count();
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
            unsafe { System.dealloc(at, layout) }
        }
        unsafe fn realloc(&self, at: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            count();
            unsafe { System.realloc(at, layout, size) }
        }
    }
    #[global_allocator]
    static COUNTING: Counting = Counting;
    pub fn allocations() -> usize {
        COUNT.with(|count| count.get())
    }
}
#[cfg(test)]
pub use counting::allocations;

#[cfg(test)]
mod tests {
    use super::*;
    /// A slab to stand on with a tall block on it and a ramp beside that.
    fn scene() -> Scene {
        Scene {
            blocks: vec![
                Block { min: Vec3::new(-1., -0.3, -0.5), max: Vec3::new(1., 0., 0.5) },
                Block { min: Vec3::new(0.2, 0., -0.1), max: Vec3::new(0.4, 0.25, 0.1) },
            ],
            wedges: vec![Wedge {
                min: Vec3::new(-0.6, 0., -0.1),
                max: Vec3::new(-0.3, 0.15, 0.1),
                along_z: false,
                rising: false,
            }],
        }
    }
    fn tune() -> Tuning {
        Tuning { gravity: 2.8, ..Tuning::default() }
    }
    /// A heavy trunk with two two-bone limbs: the smallest thing with every kind of joint. The
    /// trunk is wider than it is deep, so it has a way to lie; a plain cylinder with a leg out to
    /// one side rolls until the leg stops it.
    fn puppet(at: Vec3, turn: Quat) -> Ragdoll {
        let bone = 0.016;
        let down = Quat::from_rotation_z(-PI * 0.5);
        let mut bodies = vec![Body::new(at, turn, 1., Body::rod(1., 0.15, 0.03))];
        let mut balls: Vec<Ball> = [-0.05, 0., 0.05].map(|y| Ball { body: 0, at: Vec3::Y * y, radius: 0.025 }).into();
        balls.extend([-0.02, 0.02].map(|x| Ball { body: 0, at: Vec3::X * x, radius: 0.02 }));
        let mut joints = vec![];
        for (i, side) in [-1f32, 1.].into_iter().enumerate() {
            let hip = Vec3::new(side * 0.03, -0.06, 0.);
            let (upper, lower) = (1 + i * 2, 2 + i * 2);
            for (n, body) in [upper, lower].into_iter().enumerate() {
                let centre = at + turn * (hip - Vec3::Y * bone * (0.5 + n as f32));
                bodies.push(Body::new(centre, turn, 0.04, Body::rod(0.04, bone, 0.004)));
                balls.push(Ball { body, at: Vec3::Y * 0.004, radius: 0.0045 });
                balls.push(Ball { body, at: Vec3::Y * -0.006, radius: 0.0045 });
            }
            let (top, tip) = (Vec3::Y * bone * 0.5, Vec3::Y * -bone * 0.5);
            joints.push(Joint::new(0, upper, [hip, top], [down, down], Limit::Cone { swing: 1.2, twist: 0.6 }));
            joints.push(Joint::new(upper, lower, [tip, top], [Quat::IDENTITY; 2], Limit::Hinge { min: -2.4, max: 0.1 }));
        }
        Ragdoll::new(bodies, balls, joints, 2)
    }
    fn thrown(seed: u32) -> Ragdoll {
        let n = |k: u32| crate::player_model::noise(seed * 31 + k) - 0.5;
        // Well clear of the block and the ramp, turned any way up, moving and spinning as one.
        let mut doll = puppet(
            Vec3::new(n(0) * 1.2, 0.42 + n(1) * 0.1, n(2) * 0.4),
            Quat::from_euler(bevy::math::EulerRot::XYZ, n(3) * 6., n(4) * 6., n(5) * 6.),
        );
        let (centre, velocity, spin) = (doll.bodies[0].at, Vec3::new(n(6), n(7), n(8)) * 1.2, Vec3::new(n(9), n(10), n(11)) * 24.);
        for b in &mut doll.bodies {
            b.velocity = velocity + spin.cross(b.at - centre);
            b.spin = spin;
        }
        doll
    }
    fn run(doll: &mut Ragdoll, seconds: f32, mut each: impl FnMut(&Ragdoll)) {
        let (scene, tune) = (scene(), tune());
        for _ in 0..(seconds * 60.) as usize {
            doll.advance(1. / 60., &scene, &tune);
            each(doll);
        }
    }
    /// The widest any joint gapes and the furthest any is bent past its limit.
    fn strain(doll: &Ragdoll) -> (f32, f32) {
        doll.joints.iter().fold((0f32, 0f32), |(gap, over), j| {
            let angle = j.angle(&doll.bodies);
            let past = match j.limit {
                Limit::Cone { swing, .. } => angle - swing,
                Limit::Hinge { min, max } => (angle - max).max(min - angle),
            };
            (gap.max(j.gap(&doll.bodies)), over.max(past))
        })
    }
    /// How deep the trunk's spheres, and the limbs', are in the scenery.
    fn sunk(doll: &Ragdoll, scene: &Scene) -> (f32, f32) {
        let (mut trunk, mut limbs) = (0f32, 0f32);
        for ball in &doll.balls {
            let centre = doll.bodies[ball.body].point(ball.at);
            let touches = scene.blocks.iter().map(|b| b.touch(centre, ball.radius)).chain(scene.wedges.iter().map(|w| w.touch(centre, ball.radius)));
            let depth = touches.flatten().map(|(_, depth)| depth).fold(0f32, f32::max);
            if ball.body == 0 { trunk = trunk.max(depth) } else { limbs = limbs.max(depth) }
        }
        (trunk, limbs)
    }
    #[test]
    fn joints_hold_their_length_and_their_limits_through_a_tumble() {
        for seed in 0..24 {
            let mut doll = thrown(seed);
            let (mut gap, mut over) = (0f32, 0f32);
            run(&mut doll, 5., |doll| {
                let now = strain(doll);
                (gap, over) = (gap.max(now.0), over.max(now.1));
                assert!(doll.bodies.iter().all(|b| b.at.is_finite() && b.turn.is_normalized()), "seed {seed} blew up");
            });
            let rest = strain(&doll);
            println!("seed {seed}: widest gap {:.2} mm, {:.2} mm at the end; furthest past a limit {over:.2} rad, {:.2} at the end", gap * 1000., rest.0 * 1000., rest.1);
            // Bones are 16 mm long. In the worst frame of a hard landing, with the trunk spinning
            // at 12 rad/s, a joint gives a quarter of that and a whipping limb overshoots its
            // limit by a quarter of a radian; lying still they are tight.
            assert!(gap < 0.0045, "seed {seed}: a joint came {gap} m apart");
            assert!(over < 0.35, "seed {seed}: a joint went {over} rad past its limit");
            if doll.asleep() {
                assert!(rest.0 < 0.0005 && rest.1 < 0.1, "seed {seed}: at rest a joint gapes {} m and is {} rad past its limit", rest.0, rest.1);
            }
        }
    }
    #[test]
    fn nothing_gains_energy_on_its_own() {
        for seed in 0..24 {
            let mut doll = thrown(seed);
            let gravity = tune().gravity;
            let start = doll.energy(gravity);
            let (mut lowest, mut worst) = (start, 0f32);
            run(&mut doll, 5., |doll| {
                let now = doll.energy(gravity);
                worst = worst.max(now - lowest);
                lowest = lowest.min(now);
            });
            // Measured against what it started with: about 1.6 for a body of mass 1.15 thrown from
            // 0.4 m. Position-based joints give a little back when a body lands on its own limb.
            println!("seed {seed}: energy {start:.4} -> {lowest:.4}, most ever regained {worst:.5} ({:.2}%)", worst / start * 100.);
            assert!(worst < 0.01 * start, "seed {seed}: gained {worst}");
        }
    }
    #[test]
    fn bodies_stay_out_of_the_counter_and_out_of_blocks() {
        let scene = scene();
        for seed in 0..24 {
            let mut doll = thrown(seed);
            let (mut trunk, mut limbs) = (0f32, 0f32);
            run(&mut doll, 5., |doll| {
                let now = sunk(doll, &scene);
                (trunk, limbs) = (trunk.max(now.0), limbs.max(now.1));
            });
            let rest = sunk(&doll, &scene);
            println!("seed {seed}: deepest the trunk sank {:.2} mm, a limb {:.2} mm; at the end {:.2} and {:.2} mm, asleep {}", trunk * 1000., limbs * 1000., rest.0 * 1000., rest.1 * 1000., doll.asleep());
            // The trunk never sinks. A limb caught under it is pressed in for a moment, by less
            // than its own thickness, and comes back out.
            assert!(trunk < 0.001, "seed {seed}: the trunk sank {trunk} m into the scenery");
            assert!(limbs < 0.008, "seed {seed}: a limb sank {limbs} m into the scenery");
            if !doll.asleep() {
                continue;
            }
            assert!(rest.0 < 0.0003 && rest.1 < 0.001, "seed {seed}: came to rest {} and {} m inside the scenery", rest.0, rest.1);
            for ball in &doll.balls {
                let centre = doll.bodies[ball.body].point(ball.at);
                if centre.y > -0.3 {
                    let ground = scene.ground(centre + Vec3::Y).expect("it is over the counter");
                    assert!(centre.y > ground + ball.radius - 0.001, "seed {seed}: under the surface at {centre}");
                }
            }
        }
    }
    #[test]
    fn a_body_past_the_edge_falls_and_one_on_the_counter_stays() {
        let tilted = Quat::from_rotation_z(0.3);
        let mut over = puppet(Vec3::new(1.08, 0.1, 0.), tilted);
        let mut on = puppet(Vec3::new(0.8, 0.1, 0.), tilted);
        run(&mut over, 1.5, |_| {});
        run(&mut on, 3., |_| {});
        assert!(over.bodies[0].at.y < -1., "it should have dropped past the counter: {}", over.bodies[0].at);
        assert!(!over.asleep(), "nothing falling goes to sleep");
        assert!(on.bodies[0].at.y > 0.02 && on.bodies[0].at.y < 0.05 && on.asleep(), "{}", on.bodies[0].at);
        // Tipped over the edge rather than dropped beyond it: pushed along the counter, it goes off the end.
        let mut slid = puppet(Vec3::new(0.93, 0.1, 0.), tilted);
        for b in &mut slid.bodies {
            b.velocity = Vec3::X * 1.2;
        }
        run(&mut slid, 3., |_| {});
        assert!(slid.bodies[0].at.x > 1. && slid.bodies[0].at.y < -0.5, "{}", slid.bodies[0].at);
    }
    #[test]
    fn a_ragdoll_comes_to_rest_and_sleeps_within_a_few_seconds() {
        for seed in 0..12 {
            let mut doll = thrown(seed);
            let mut slept = None;
            let mut frame = 0;
            run(&mut doll, 6., |doll| {
                frame += 1;
                if doll.asleep() && slept.is_none() {
                    slept = Some(frame as f32 / 60.);
                }
            });
            let Some(slept) = slept else {
                // The ones that leave the counter are still falling, which is right.
                assert!(doll.bodies[0].at.y < -1., "seed {seed} never slept, at {}", doll.bodies[0].at);
                continue;
            };
            println!("seed {seed}: asleep after {slept:.2} s");
            assert!(slept < 5., "seed {seed} took {slept} s to settle");
            let before: Vec<Vec3> = doll.bodies.iter().map(|b| b.at).collect();
            let steps = doll.advance(1., &scene(), &tune());
            assert_eq!(steps, 0, "a sleeping ragdoll does no work");
            assert!(doll.bodies.iter().zip(&before).all(|(b, at)| b.at == *at));
        }
    }
    #[test]
    fn stepping_allocates_nothing() {
        let (scene, tune) = (scene(), tune());
        let mut doll = thrown(4);
        doll.advance(1. / 60., &scene, &tune);
        let before = allocations();
        for _ in 0..200 {
            doll.advance(1. / 60., &scene, &tune);
        }
        assert_eq!(allocations(), before);
    }
    #[test]
    fn the_same_start_gives_the_same_fall() {
        let fall = |seed| {
            let mut doll = thrown(seed);
            run(&mut doll, 3., |_| {});
            doll.bodies.iter().map(|b| (b.at.to_array().map(f32::to_bits), b.turn.to_array().map(f32::to_bits))).collect::<Vec<_>>()
        };
        assert_eq!(fall(3), fall(3));
        assert_ne!(fall(3), fall(4));
    }
    #[test]
    fn a_ramp_is_a_slope_not_a_step() {
        let scene = scene();
        // The ramp climbs towards -x from x = -0.3 to 0.15 high at x = -0.6.
        assert!((scene.ground(Vec3::new(-0.45, 1., 0.)).unwrap() - 0.075).abs() < 1e-6);
        assert_eq!(scene.ground(Vec3::new(-0.45, 0.05, 0.)), Some(0.), "under the slope there is only the counter");
        assert_eq!(scene.ground(Vec3::new(-0.8, 1., 0.)), Some(0.));
        assert_eq!(scene.ground(Vec3::new(2., 1., 0.)), None);
        let ball = |x: f32| {
            let mut doll = Ragdoll::new(
                vec![Body::new(Vec3::new(x, 0.3, 0.), Quat::IDENTITY, 0.1, Vec3::splat(1e-5))],
                vec![Ball { body: 0, at: Vec3::ZERO, radius: 0.02 }],
                vec![],
                0,
            );
            let mut lowest_over_ramp = f32::MAX;
            for _ in 0..360 {
                doll.advance(1. / 60., &scene, &tune());
                let at = doll.bodies[0].at;
                if at.x > -0.6 && at.x < -0.3 {
                    lowest_over_ramp = lowest_over_ramp.min(at.y - scene.ground(at + Vec3::Y).unwrap());
                }
            }
            (doll.bodies[0].at, lowest_over_ramp)
        };
        let (end, lowest) = ball(-0.5);
        assert!(lowest > 0.015, "the ball sank into the ramp: {lowest}");
        assert!(end.x > -0.3 && (end.y - 0.02).abs() < 0.002, "it rolls down and off the low end: {end}");
    }
    #[test]
    fn a_muscle_bends_a_joint_and_a_hand_holds_a_body_up() {
        let (scene, tune) = (scene(), tune());
        let mut doll = puppet(Vec3::new(0., 0.2, 0.), Quat::IDENTITY);
        doll.servos[0] = Servo::Hold { body: 0, point: Vec3::ZERO, target: Vec3::new(0., 0.2, 0.), axes: Vec3::Y, slack: 0., rate: 40. };
        doll.servos[1] = Servo::Face { body: 0, target: Quat::IDENTITY, rate: 20. };
        doll.joints[1].pose = Quat::from_rotation_x(-1.5);
        doll.joints[1].drive = 60.;
        doll.bodies[0].spin = Vec3::Z * 3.;
        for _ in 0..90 {
            doll.advance(1. / 60., &scene, &tune);
        }
        assert!((doll.bodies[0].at.y - 0.2).abs() < 0.004, "held up: {}", doll.bodies[0].at);
        assert!(doll.asleep(), "held still, it sleeps until it is woken");
        assert!(doll.bodies[0].turn.angle_between(Quat::IDENTITY) < 0.05, "kept upright");
        // A muscle is soft: the weight of the shin holds it a little short of the pose.
        assert!((doll.joints[1].angle(&doll.bodies) + 1.5).abs() < 0.2, "the knee bends to {}", doll.joints[1].angle(&doll.bodies));
        assert!(doll.joints[3].angle(&doll.bodies).abs() < 0.3, "the other hangs");
        // Let go, and it is an ordinary falling body again.
        doll.servos = vec![Servo::Idle; 2];
        doll.joints[1].drive = 0.;
        doll.wake();
        for _ in 0..360 {
            doll.advance(1. / 60., &scene, &tune);
        }
        assert!(doll.bodies[0].at.y < 0.05 && doll.asleep(), "{} asleep {}", doll.bodies[0].at, doll.asleep());
    }
}

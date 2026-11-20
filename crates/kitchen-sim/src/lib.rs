// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Small, allocation-stable collision and navigation primitives shared by the kitchen actors.
pub mod actor;
pub mod aim;
pub mod cards;
pub mod drape;
pub mod emergence;
pub mod feast;
pub mod forts;
pub mod griddle;
pub mod head;
pub mod kungfu;
pub mod magazine;
pub mod map;
pub mod net;
pub mod props;
pub mod recoil;
pub mod pov;
pub mod replay;
pub mod rules;
pub mod spawns;
pub mod titan;
pub mod tune;
use glam::{Vec2, Vec3};
use std::{cmp::Ordering, collections::BinaryHeap};

/// What the tests that time something against a budget (a step, a plan, a query: this crate's and the game's) multiply
/// the budget by: the environment's `HOTDOG_TIMING_SLACK`, and 1 when it is not set, so that a developer's machine keeps
/// the strict budgets. A shared or slow machine (the CI runners, with 3) sets it. A value that is not a number above zero
/// stops the test with a message, so that a typo does not quietly keep the strict budget.
pub fn timing_slack() -> f64 {
    let Ok(text) = std::env::var("HOTDOG_TIMING_SLACK") else {
        return 1.;
    };
    if text.trim().is_empty() {
        return 1.;
    }
    text.trim().parse::<f64>().ok().filter(|slack| slack.is_finite() && *slack > 0.).unwrap_or_else(|| panic!("HOTDOG_TIMING_SLACK must be a number above zero, not {text:?}"))
}

#[derive(Clone, Copy, Debug)]
pub struct Capsule {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
    pub owner: u32,
    pub layer: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    pub a: usize,
    pub b: usize,
    pub normal: Vec3,
    pub depth: f32,
}
#[derive(Default)]
pub struct Contacts {
    order: Vec<usize>,
    pub hits: Vec<Contact>,
    pub candidates: usize,
}

/// Sweep-and-prune broad phase and analytic segment distances; never test render triangles.
impl Contacts {
    pub fn detect(&mut self, shapes: &[Capsule]) {
        self.hits.clear();
        self.candidates = 0;
        self.order.clear();
        self.order.extend(0..shapes.len());
        self.order.sort_unstable_by(|a, b| {
            let min = |i: usize| shapes[i].a.x.min(shapes[i].b.x) - shapes[i].radius;
            min(*a).total_cmp(&min(*b))
        });
        for (n, &i) in self.order.iter().enumerate() {
            let a = shapes[i];
            let max = a.a.max(a.b) + Vec3::splat(a.radius);
            let min = a.a.min(a.b) - Vec3::splat(a.radius);
            for &j in &self.order[n + 1..] {
                let b = shapes[j];
                let bmin = b.a.min(b.b) - Vec3::splat(b.radius);
                if bmin.x > max.x {
                    break;
                }
                if a.owner == b.owner || a.layer & b.layer == 0 {
                    continue;
                }
                let bmax = b.a.max(b.b) + Vec3::splat(b.radius);
                if bmin.y > max.y || bmax.y < min.y || bmin.z > max.z || bmax.z < min.z {
                    continue;
                }
                self.candidates += 1;
                let (p, q) = closest_segments(a.a, a.b, b.a, b.b);
                let delta = p - q;
                let d = delta.length();
                let depth = a.radius + b.radius - d;
                if depth > 0.0 {
                    let fallback = (a.a + a.b - b.a - b.b).normalize_or(Vec3::X);
                    self.hits.push(Contact {
                        a: i,
                        b: j,
                        normal: delta.try_normalize().unwrap_or(fallback),
                        depth,
                    });
                }
            }
        }
    }
}

pub fn closest_segments(p: Vec3, q: Vec3, r: Vec3, s: Vec3) -> (Vec3, Vec3) {
    let d = q - p;
    let e = s - r;
    let v = p - r;
    let a = d.length_squared();
    let b = d.dot(e);
    let c = e.length_squared();
    let f = e.dot(v);
    let g = d.dot(v);
    let (mut u, mut t);
    if a < 1e-12 && c < 1e-12 {
        return (p, r);
    }
    if a < 1e-12 {
        u = 0.;
        t = (f / c).clamp(0., 1.);
    } else if c < 1e-12 {
        t = 0.;
        u = (-g / a).clamp(0., 1.);
    } else {
        let denominator = a * c - b * b;
        u = if denominator > 1e-12 {
            ((b * f - g * c) / denominator).clamp(0., 1.)
        } else {
            0.
        };
        t = (b * u + f) / c;
        if t < 0. {
            t = 0.;
            u = (-g / a).clamp(0., 1.);
        } else if t > 1. {
            t = 1.;
            u = ((b - g) / a).clamp(0., 1.);
        }
    }
    (p + d * u, r + e * t)
}

/// Conservative advancement catches a fast hand crossing another between simulation ticks.
pub fn sweep(a: Capsule, previous: Capsule, b: Capsule, previous_b: Capsule) -> Option<f32> {
    let va = (a.a - previous.a).length().max((a.b - previous.b).length());
    let vb = (b.a - previous_b.a)
        .length()
        .max((b.b - previous_b.b).length());
    let speed = va + vb;
    let mut t = 0.;
    for _ in 0..24 {
        let (p, q) = closest_segments(
            previous.a.lerp(a.a, t),
            previous.b.lerp(a.b, t),
            previous_b.a.lerp(b.a, t),
            previous_b.b.lerp(b.b, t),
        );
        let gap = p.distance(q) - a.radius - b.radius;
        if gap <= 0.00001 {
            return Some(t);
        }
        if speed < 1e-8 {
            return None;
        }
        t += gap / speed;
        if t > 1. {
            return None;
        }
    }
    None
}

pub fn ray_sphere(origin: Vec3, direction: Vec3, centre: Vec3, radius: f32) -> Option<f32> {
    let v = origin - centre;
    let b = v.dot(direction);
    let c = v.length_squared() - radius * radius;
    let h = b * b - c;
    if h < 0. {
        None
    } else {
        let near = -b - h.sqrt();
        let far = -b + h.sqrt();
        if far < 0. { None } else { Some(near.max(0.)) }
    }
}
pub fn ray_box(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let mut near: f32 = 0.;
    let mut far = f32::INFINITY;
    for i in 0..3 {
        if direction[i].abs() < 1e-8 {
            if origin[i] < min[i] || origin[i] > max[i] {
                return None;
            }
        } else {
            let a = (min[i] - origin[i]) / direction[i];
            let b = (max[i] - origin[i]) / direction[i];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    Some(near)
}

#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub min: Vec2,
    pub max: Vec2,
}
#[derive(Clone)]
pub struct Navigation {
    pub min: Vec2,
    pub max: Vec2,
    pub cell: f32,
    width: usize,
    height: usize,
    blocked: Vec<bool>,
    costs: Vec<f32>,
    parents: Vec<usize>,
    open: BinaryHeap<Visit>,
}
#[derive(Clone, Copy, PartialEq)]
struct Visit {
    score: f32,
    cost: f32,
    id: usize,
}
impl Eq for Visit {}
impl Ord for Visit {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .score
            .total_cmp(&self.score)
            .then_with(|| other.id.cmp(&self.id))
    }
}
impl PartialOrd for Visit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Navigation {
    pub fn new(min: Vec2, max: Vec2, cell: f32, clearance: f32, obstacles: &[Obstacle]) -> Self {
        let width = ((max.x - min.x) / cell).ceil() as usize;
        let height = ((max.y - min.y) / cell).ceil() as usize;
        let mut nav = Self {
            min,
            max,
            cell,
            width,
            height,
            blocked: vec![false; width * height],
            costs: vec![f32::INFINITY; width * height],
            parents: vec![usize::MAX; width * height],
            open: BinaryHeap::new(),
        };
        for id in 0..width * height {
            let p = nav.point(id);
            nav.blocked[id] = obstacles.iter().any(|o| {
                p.cmpge(o.min - Vec2::splat(clearance)).all()
                    && p.cmple(o.max + Vec2::splat(clearance)).all()
            });
        }
        nav
    }
    fn point(&self, id: usize) -> Vec2 {
        self.min
            + Vec2::new((id % self.width) as f32, (id / self.width) as f32) * self.cell
            + Vec2::splat(self.cell * 0.5)
    }
    fn id(&self, p: Vec2) -> usize {
        let p = (p - self.min) / self.cell;
        (p.y.floor() as i32).clamp(0, self.height as i32 - 1) as usize * self.width
            + (p.x.floor() as i32).clamp(0, self.width as i32 - 1) as usize
    }
    pub fn clear(&self, p: Vec2) -> bool {
        p.cmpge(self.min).all() && p.cmplt(self.max).all() && !self.blocked[self.id(p)]
    }
    fn line(&self, a: Vec2, b: Vec2) -> bool {
        let n = (a.distance(b) / (self.cell * 0.35)).ceil().max(1.) as usize;
        (0..=n).all(|i| self.clear(a.lerp(b, i as f32 / n as f32)))
    }
    /// Returns false for unreachable goals rather than walking through an obstacle.
    pub fn path(&mut self, start: Vec2, goal: Vec2, out: &mut Vec<Vec2>) -> bool {
        out.clear();
        if !self.clear(start) || !self.clear(goal) {
            return false;
        }
        if self.line(start, goal) {
            out.push(goal);
            return true;
        }
        self.costs.fill(f32::INFINITY);
        self.parents.fill(usize::MAX);
        self.open.clear();
        let first = self.id(start);
        let last = self.id(goal);
        self.costs[first] = 0.;
        self.open.push(Visit {
            score: 0.,
            cost: 0.,
            id: first,
        });
        while let Some(v) = self.open.pop() {
            if v.cost > self.costs[v.id] {
                continue;
            }
            if v.id == last {
                let mut id = last;
                out.push(goal);
                while id != first {
                    out.push(self.point(id));
                    id = self.parents[id];
                }
                out.reverse();
                let mut at = start;
                let mut i = 0;
                while i < out.len() {
                    let mut j = i;
                    while j + 1 < out.len() && self.line(at, out[j + 1]) {
                        j += 1;
                    }
                    out.drain(i..j);
                    at = out[i];
                    i += 1;
                }
                return true;
            }
            let x = (v.id % self.width) as i32;
            let y = (v.id / self.width) as i32;
            for (dx, dy) in [
                (-1, 0),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, -1),
                (1, -1),
                (-1, 1),
                (1, 1),
            ] {
                let nx = x + dx;
                let ny = y + dy;
                if nx < 0 || ny < 0 || nx >= self.width as i32 || ny >= self.height as i32 {
                    continue;
                }
                let id = ny as usize * self.width + nx as usize;
                if self.blocked[id] {
                    continue;
                }
                if dx != 0
                    && dy != 0
                    && (self.blocked[y as usize * self.width + nx as usize]
                        || self.blocked[ny as usize * self.width + x as usize])
                {
                    continue;
                }
                let cost = v.cost + if dx != 0 && dy != 0 { 1.414214 } else { 1. };
                if cost < self.costs[id] {
                    self.costs[id] = cost;
                    self.parents[id] = v.id;
                    self.open.push(Visit {
                        id,
                        cost,
                        score: cost + self.point(id).distance(goal) / self.cell,
                    });
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cap(x: f32, owner: u32) -> Capsule {
        Capsule {
            a: Vec3::new(x, 0., 0.),
            b: Vec3::new(x, 1., 0.),
            radius: 0.1,
            owner,
            layer: 1,
        }
    }
    #[test]
    fn contacts_filter_and_separate() {
        let mut c = Contacts::default();
        c.detect(&[cap(0., 1), cap(0.15, 2), cap(0.01, 1), cap(4., 3)]);
        assert_eq!(c.hits.len(), 2);
        assert!((c.hits[0].depth - 0.05).abs() < 0.0001);
        assert!(c.candidates < 6);
    }
    #[test]
    fn fast_crossing() {
        assert!(sweep(cap(1., 1), cap(-1., 1), cap(0., 2), cap(0., 2)).is_some());
    }
    #[test]
    fn degenerate_segments() {
        let (a, b) = closest_segments(Vec3::ZERO, Vec3::ZERO, Vec3::X, Vec3::X);
        assert_eq!(a.distance(b), 1.);
    }
    #[test]
    fn path_avoids_wall() {
        let wall = Obstacle {
            min: Vec2::new(1., 0.),
            max: Vec2::new(1.5, 2.5),
        };
        let mut n = Navigation::new(Vec2::ZERO, Vec2::splat(4.), 0.1, 0.15, &[wall]);
        let mut path = vec![];
        assert!(n.path(Vec2::splat(0.5), Vec2::new(3., 0.5), &mut path));
        assert!(path.len() > 1);
        let mut prev = Vec2::splat(0.5);
        for p in path {
            assert!(n.line(prev, p));
            prev = p;
        }
    }
    #[test]
    fn unreachable_and_no_corner_cutting() {
        let wall = Obstacle {
            min: Vec2::new(0.9, 0.),
            max: Vec2::new(1.1, 4.),
        };
        let mut n = Navigation::new(Vec2::ZERO, Vec2::splat(4.), 0.1, 0.1, &[wall]);
        assert!(!n.path(Vec2::splat(0.5), Vec2::new(3., 0.5), &mut vec![]));
    }
    #[test]
    fn hitscan_head_and_cover() {
        assert_eq!(
            ray_sphere(Vec3::ZERO, Vec3::Z, Vec3::Z * 2., 0.2),
            Some(1.8000001)
        );
        assert_eq!(
            ray_box(
                Vec3::ZERO,
                Vec3::Z,
                Vec3::new(-1., -1., 1.),
                Vec3::new(1., 1., 1.1)
            ),
            Some(1.)
        );
    }
}

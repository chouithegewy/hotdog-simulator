// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The head-up display's icons: one small, coherent set, drawn here in code rather than loaded, so that the native
//! build and the browser's are the same and nothing waits on a download.
//!
//! * **The look.** Every icon is a flat silhouette filled white, with a dark outline of the same width all round
//!   (about a fifteenth of its height), so that it reads on the bright counter and in the dark alike. White takes the
//!   tint of the `ImageNode` it is shown in (the heart red, a team's pips in its colour); the outline stays dark. Line
//!   icons (the tick, the throw, infinity) are strokes of one width ([`STROKE`]).
//! * **The guns** are not drawn by hand: each is the side view of its own model (weapons.rs) flattened, so the icon is
//!   the gun the player holds, muzzle to the right. Wires and hoses thinner than a pixel are opened away.
//! * **Crisp at any size.** An icon is drawn for the size it is shown at, in device pixels ([`Icons::get`] keeps one
//!   texture for each icon and height), from a mask four times as fine whose signed distance field gives the fill and
//!   the outline; so it is never magnified or shrunk on the screen. The head-up display asks for its sizes from the
//!   window's height (hud.rs), about 24 to 32 pixels at 720p.
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::collections::HashMap;

/// Every icon. `Gun(w)` is the gun of weapon number `w` (1 to 7, as arena::Weapon numbers them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Heart,
    Bullet,
    Stim,
    Clock,
    Cat,
    Hotdog,
    Bacon,
    Arrow,
    Home,
    Skull,
    Crown,
    Check,
    Hammer,
    Shield,
    Infected,
    Bite,
    Eat,
    Strike,
    Backstab,
    /// A round in the face (the ketchup sniper's kills there): a face in a sight's crosshairs.
    FaceShot,
    Turret,
    Flies,
    Fall,
    Hand,
    Drape,
    Undrape,
    PutDown,
    Throw,
    Lock,
    Crouch,
    Prone,
    Infinity,
    Eye,
    AmmoBox,
    MouseLeft,
    MouseRight,
    Card,
    Target,
    Fridge,
    Pile,
    /// The power button's symbol: a ring open at the top with a bar down into it (the griddle's switch).
    Power,
    Thermometer,
    /// A flame: what burned a hotdog (the kill feed).
    Flame,
    /// A degree mark, which the font has no glyph for: a thick ring, small, to be set beside a number.
    Degree,
    /// Kung Fu: a style's animal (`kungfu::Style` as its number: the tiger's head, the crane on one leg, the snake raised
    /// to strike, the mantis with its hooks folded, the monkey's face, the drunkard's gourd, the squatting toad).
    Style(u8),
    /// Chi: a spiral drawn in to a point.
    Chi,
    /// A blocked strike: two forearms crossed, fists up.
    Guard,
    /// A parry: a strike's arrow turned back off a raised arm.
    Parry,
    /// A broken guard: a shield split down a crack.
    Broken,
    /// A thin ring to go round another icon (a Toad's iron body round its style).
    Halo,
    Gun(u8),
}
// (The whole set, which the tests go through.)
#[allow(dead_code)]
impl Icon {
    /// Every icon that is drawn by hand, and the seven guns.
    pub const DRAWN: [Icon; 56] = [
        Icon::Heart,
        Icon::Bullet,
        Icon::Stim,
        Icon::Clock,
        Icon::Cat,
        Icon::Hotdog,
        Icon::Bacon,
        Icon::Arrow,
        Icon::Home,
        Icon::Skull,
        Icon::Crown,
        Icon::Check,
        Icon::Hammer,
        Icon::Shield,
        Icon::Infected,
        Icon::Bite,
        Icon::Eat,
        Icon::Strike,
        Icon::Backstab,
        Icon::FaceShot,
        Icon::Turret,
        Icon::Flies,
        Icon::Fall,
        Icon::Hand,
        Icon::Drape,
        Icon::Undrape,
        Icon::PutDown,
        Icon::Throw,
        Icon::Lock,
        Icon::Crouch,
        Icon::Prone,
        Icon::Infinity,
        Icon::Eye,
        Icon::AmmoBox,
        Icon::MouseLeft,
        Icon::MouseRight,
        Icon::Card,
        Icon::Target,
        Icon::Fridge,
        Icon::Pile,
        Icon::Power,
        Icon::Thermometer,
        Icon::Flame,
        Icon::Degree,
        Icon::Style(0),
        Icon::Style(1),
        Icon::Style(2),
        Icon::Style(3),
        Icon::Style(4),
        Icon::Style(5),
        Icon::Style(6),
        Icon::Chi,
        Icon::Guard,
        Icon::Parry,
        Icon::Broken,
        Icon::Halo,
    ];
    /// The styles' animals, in `kungfu::Style`'s order.
    pub const STYLES: [Icon; 7] = [Icon::Style(0), Icon::Style(1), Icon::Style(2), Icon::Style(3), Icon::Style(4), Icon::Style(5), Icon::Style(6)];
    pub const GUNS: [Icon; 7] = [Icon::Gun(1), Icon::Gun(2), Icon::Gun(3), Icon::Gun(4), Icon::Gun(5), Icon::Gun(6), Icon::Gun(7)];
}
impl Icon {
    /// How much wider than high it is drawn: the guns lie in a long box, everything else in a square.
    pub fn aspect(self) -> f32 {
        match self {
            Icon::Gun(_) => GUN_ASPECT,
            _ => 1.,
        }
    }
}
/// A gun's box, across over up.
pub const GUN_ASPECT: f32 = 2.6;
/// The width of a line icon's strokes, in the icon's unit square.
pub const STROKE: f32 = 0.1;
/// How many samples a side each pixel is drawn from.
const FINE: usize = 4;
/// The outline's colour (the fill is white, to be tinted).
const INK: [f32; 3] = [0.035, 0.04, 0.05];
const INK_ALPHA: f32 = 0.9;

/// The icons drawn so far, by icon and height in device pixels.
#[derive(Resource, Default)]
pub struct Icons {
    made: HashMap<(Icon, u32), Handle<Image>>,
}
impl Icons {
    /// The icon drawn `height` device pixels high (and its aspect times as wide), drawn now the first time it is asked
    /// for at that height.
    pub fn get(&mut self, images: &mut Assets<Image>, icon: Icon, height: u32) -> Handle<Image> {
        let height = height.clamp(8, 256);
        self.made
            .entry((icon, height))
            .or_insert_with(|| {
                let (w, h) = size(icon, height);
                images.add(image(&raster(icon, w, h), w, h))
            })
            .clone()
    }
}
impl Icons {
    /// Which icon an image is (for the tests: what the display shows).
    #[allow(dead_code)]
    pub fn which(&self, image: &Handle<Image>) -> Option<Icon> {
        self.made.iter().find(|(_, h)| h.id() == image.id()).map(|((icon, _), _)| *icon)
    }
}
/// The width and height in pixels of `icon` drawn `height` high.
pub fn size(icon: Icon, height: u32) -> (u32, u32) {
    ((height as f32 * icon.aspect()).round() as u32, height)
}
fn image(data: &[u8], w: u32, h: u32) -> Image {
    Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data.to_vec(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

/// The outline's width in pixels for an icon `h` pixels high.
pub fn outline(h: u32) -> f32 {
    (h as f32 * 0.065).clamp(1.25, 3.5)
}

/// The icon as RGBA (sRGB, not premultiplied), `w` by `h` pixels: white where it is, dark round its edge, clear
/// elsewhere.
pub fn raster(icon: Icon, w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let (mw, mh) = (w * FINE, h * FINE);
    let line = outline(h as u32);
    // The box the shape is drawn in, inside the room the outline takes.
    let margin = line + 0.6;
    let mut mask = match icon {
        // (A gun fills its box, where a drawn icon keeps a little inside it: a pixel more of room all round.)
        Icon::Gun(weapon) => gun(weapon as usize, mw, mh, (margin + 1.) * FINE as f32),
        _ => {
            let side = (h as f32 - 2. * margin).max(1.);
            let left = (w as f32 - side) / 2.;
            let shape = shape(icon);
            let mut mask = vec![false; mw * mh];
            for y in 0..mh {
                for x in 0..mw {
                    let at = Vec2::new(((x as f32 + 0.5) / FINE as f32 - left) / side, ((y as f32 + 0.5) / FINE as f32 - margin) / side);
                    mask[y * mw + x] = shape(at);
                }
            }
            mask
        }
    };
    if matches!(icon, Icon::Gun(_)) {
        // (Anything thinner than about a pixel goes: a wire guard, a hose.)
        open(&mut mask, mw, mh, 0.5 * FINE as f32);
    }
    let signed = signed(&mask, mw, mh);
    let mut out = vec![0u8; w * h * 4];
    let per = (FINE * FINE) as f32;
    for y in 0..h {
        for x in 0..w {
            let (mut colour, mut alpha) = (Vec3::ZERO, 0.);
            for sy in 0..FINE {
                for sx in 0..FINE {
                    let d = signed[(y * FINE + sy) * mw + x * FINE + sx] / FINE as f32;
                    if d <= 0. {
                        colour += Vec3::ONE;
                        alpha += 1.;
                    } else if d <= line {
                        colour += Vec3::from_array(INK) * INK_ALPHA;
                        alpha += INK_ALPHA;
                    }
                }
            }
            let at = (y * w + x) * 4;
            if alpha > 0. {
                let c = colour / alpha;
                // (Worked out in linear light, stored as sRGB.)
                for (k, v) in [c.x, c.y, c.z].into_iter().enumerate() {
                    out[at + k] = (srgb(v) * 255.).round() as u8;
                }
                out[at + 3] = (alpha / per * 255.).round() as u8;
            }
        }
    }
    out
}
fn srgb(linear: f32) -> f32 {
    let v = linear.clamp(0., 1.);
    if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1. / 2.4) - 0.055 }
}

/// Each sample's signed distance to the shape's edge, in samples: negative inside.
fn signed(mask: &[bool], w: usize, h: usize) -> Vec<f32> {
    let to_inside = distance(mask, w, h, true);
    let to_outside = distance(mask, w, h, false);
    mask.iter().enumerate().map(|(i, inside)| if *inside { -(to_outside[i].sqrt() - 0.5) } else { to_inside[i].sqrt() - 0.5 }).collect()
}
/// The squared distance (in samples) from every sample to the nearest one whose mask is `to`.
fn distance(mask: &[bool], w: usize, h: usize, to: bool) -> Vec<f32> {
    let mut f: Vec<f64> = mask.iter().map(|m| if *m == to { 0. } else { f64::INFINITY }).collect();
    let n = w.max(h);
    let (mut line, mut out, mut v, mut z) = (vec![0.; n], vec![0.; n], vec![0usize; n], vec![0.; n + 1]);
    for x in 0..w {
        for y in 0..h {
            line[y] = f[y * w + x];
        }
        squared(&line[..h], &mut out[..h], &mut v, &mut z);
        for y in 0..h {
            f[y * w + x] = out[y];
        }
    }
    for y in 0..h {
        line[..w].copy_from_slice(&f[y * w..y * w + w]);
        squared(&line[..w], &mut out[..w], &mut v, &mut z);
        f[y * w..y * w + w].copy_from_slice(&out[..w]);
    }
    f.into_iter().map(|d| d.min(1e9) as f32).collect()
}
/// One line of the squared Euclidean distance transform (Felzenszwalb and Huttenlocher): the lower envelope of the
/// parabolas rooted at the finite samples of `f`.
fn squared(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let mut k: isize = -1;
    for q in 0..f.len() {
        if !f[q].is_finite() {
            continue;
        }
        loop {
            if k < 0 {
                k = 0;
                (v[0], z[0], z[1]) = (q, f64::NEG_INFINITY, f64::INFINITY);
                break;
            }
            let p = v[k as usize];
            let s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2. * (q as f64 - p as f64));
            if s <= z[k as usize] {
                k -= 1;
                continue;
            }
            k += 1;
            (v[k as usize], z[k as usize], z[k as usize + 1]) = (q, s, f64::INFINITY);
            break;
        }
    }
    if k < 0 {
        d.fill(f64::INFINITY);
        return;
    }
    let mut k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        *out = (q as f64 - p as f64).powi(2) + f[p];
    }
}
/// Takes away whatever is thinner than twice `by` samples (erodes and then dilates the mask by it).
fn open(mask: &mut [bool], w: usize, h: usize, by: f32) {
    let to_outside = distance(mask, w, h, false);
    let core: Vec<bool> = mask.iter().zip(&to_outside).map(|(m, d)| *m && d.sqrt() > by).collect();
    let to_core = distance(&core, w, h, true);
    for (m, d) in mask.iter_mut().zip(&to_core) {
        *m = *m && d.sqrt() <= by + 0.5;
    }
}

/// A gun's side view: every triangle of its model (weapons.rs) seen from its right, muzzle to the right, fitted into
/// the mask `w` by `h` samples less `margin` all round.
fn gun(weapon: usize, w: usize, h: usize, margin: f32) -> Vec<bool> {
    let parts = crate::weapons::model(weapon.clamp(1, 7));
    // Seen from +x: ahead (-z) is to the right, up (+y) is up.
    let flat = |p: Vec3| Vec2::new(-p.z, -p.y);
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for (_, shape) in &parts {
        for p in &shape.positions {
            (lo, hi) = (lo.min(flat(*p)), hi.max(flat(*p)));
        }
    }
    let room = Vec2::new(w as f32 - 2. * margin, h as f32 - 2. * margin);
    let scale = (room / (hi - lo).max(Vec2::splat(1e-6))).min_element();
    let offset = Vec2::new(w as f32, h as f32) / 2. - (lo + hi) / 2. * scale;
    let mut mask = vec![false; w * h];
    for (_, shape) in &parts {
        for tri in shape.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| flat(shape.positions[tri[k] as usize]) * scale + offset);
            fill_triangle(&mut mask, w, h, a, b, c);
        }
    }
    mask
}
fn fill_triangle(mask: &mut [bool], w: usize, h: usize, a: Vec2, b: Vec2, c: Vec2) {
    let area = (b - a).perp_dot(c - a);
    if area.abs() < 1e-9 {
        return;
    }
    let (lo, hi) = (a.min(b).min(c), a.max(b).max(c));
    let (x0, x1) = ((lo.x - 0.5).floor().max(0.) as usize, ((hi.x - 0.5).ceil().max(0.) as usize).min(w.saturating_sub(1)));
    let (y0, y1) = ((lo.y - 0.5).floor().max(0.) as usize, ((hi.y - 0.5).ceil().max(0.) as usize).min(h.saturating_sub(1)));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let (e0, e1, e2) = ((b - a).perp_dot(p - a), (c - b).perp_dot(p - b), (a - c).perp_dot(p - c));
            let inside = if area > 0. { e0 >= 0. && e1 >= 0. && e2 >= 0. } else { e0 <= 0. && e1 <= 0. && e2 <= 0. };
            if inside {
                mask[y * w + x] = true;
            }
        }
    }
}

// ----- The hand-drawn icons, each in a unit square (x right, y down), kept within about 0.06 to 0.94.

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}
fn circle(p: Vec2, c: Vec2, r: f32) -> bool {
    p.distance_squared(c) <= r * r
}
fn ellipse(p: Vec2, c: Vec2, r: Vec2) -> bool {
    ((p - c) / r).length_squared() <= 1.
}
fn ring(p: Vec2, c: Vec2, outer: f32, inner: f32) -> bool {
    let d = p.distance(c);
    d <= outer && d >= inner
}
/// A rectangle from `lo` to `hi` with corners rounded by `r`.
fn rect(p: Vec2, lo: Vec2, hi: Vec2, r: f32) -> bool {
    let c = (lo + hi) / 2.;
    let half = (hi - lo) / 2. - Vec2::splat(r);
    let q = (p - c).abs() - half;
    q.max(Vec2::ZERO).length() + q.max_element().min(0.) <= r
}
/// A stroke from `a` to `b`, `r` either side, with round ends.
fn seg(p: Vec2, a: Vec2, b: Vec2, r: f32) -> bool {
    let (pa, ba) = (p - a, b - a);
    let t = (pa.dot(ba) / ba.length_squared().max(1e-9)).clamp(0., 1.);
    (pa - ba * t).length_squared() <= r * r
}
/// A stroke through `points`.
fn path(p: Vec2, points: &[Vec2], r: f32) -> bool {
    points.windows(2).any(|ab| seg(p, ab[0], ab[1], r))
}
fn poly(p: Vec2, points: &[Vec2]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}
/// `p` turned by `angle` (radians, clockwise on the screen) about `c`, for drawing a shape that is turned the other
/// way about it.
fn turn(p: Vec2, c: Vec2, angle: f32) -> Vec2 {
    c + Vec2::from_angle(-angle).rotate(p - c)
}
/// Points along a quadratic curve from `a` to `c` pulled towards `b`.
fn curve(a: Vec2, b: Vec2, c: Vec2, n: usize) -> Vec<Vec2> {
    (0..=n).map(|i| i as f32 / n as f32).map(|t| a * (1. - t) * (1. - t) + b * 2. * t * (1. - t) + c * t * t).collect()
}
/// A filled arrowhead with its point at `tip`, pointing along `way`, `size` long.
fn head(p: Vec2, tip: Vec2, way: Vec2, size: f32) -> bool {
    let way = way.normalize_or(Vec2::Y);
    let back = tip - way * size;
    let side = way.perp() * size * 0.62;
    poly(p, &[tip, back + side, back - side])
}
/// An upright hotdog in its bun, from `top` to `bottom` (y), the sausage's half-width `r`.
fn hotdog(p: Vec2, x: f32, top: f32, bottom: f32, r: f32) -> bool {
    let sausage = seg(p, v(x, top + r), v(x, bottom - r), r);
    let gap = seg(p, v(x, top + r), v(x, bottom - r), r + 0.045);
    let bun = |dx: f32| seg(p, v(x + dx, top + (bottom - top) * 0.3), v(x + dx, bottom - r * 0.9), r * 0.92);
    sausage || ((bun(-r * 1.25) || bun(r * 1.25)) && !gap)
}
/// A hotdog on its side, as everyone draws one: the sausage over the ends of its bun, a squiggle of mustard along it.
fn lying(p: Vec2) -> bool {
    let sausage = seg(p, v(0.19, 0.38), v(0.81, 0.38), 0.15);
    let bun = rect(p, v(0.1, 0.46), v(0.9, 0.86), 0.18) && !seg(p, v(0.19, 0.38), v(0.81, 0.38), 0.2);
    let squiggle = path(p, &(0..=8).map(|k| v(0.22 + 0.07 * k as f32, if k % 2 == 0 { 0.33 } else { 0.42 })).collect::<Vec<_>>(), 0.028);
    (sausage && !squiggle) || bun
}
fn drape(p: Vec2, top: f32, bottom: f32) -> bool {
    // A napkin over a block: a rounded top, falling and flaring to a scalloped hem.
    let h = bottom - top;
    let cloth = poly(p, &[v(0.3, top + 0.06 * h), v(0.36, top), v(0.64, top), v(0.7, top + 0.06 * h), v(0.88, bottom), v(0.12, bottom)]);
    let hem = (0..4).any(|k| circle(p, v(0.215 + 0.19 * k as f32, bottom + 0.035), 0.075));
    cloth && !hem
}
fn bullet(p: Vec2, x: f32, top: f32, bottom: f32, half: f32) -> bool {
    let h = bottom - top;
    let neck = top + h * 0.36;
    let tip = ellipse(p, v(x, neck), v(half, h * 0.36)) && p.y <= neck;
    let slug = rect(p, v(x - half, neck - 0.001), v(x + half, neck + h * 0.08), 0.);
    let case = rect(p, v(x - half, neck + h * 0.14), v(x + half, bottom - h * 0.06), 0.01);
    let rim = rect(p, v(x - half * 1.18, bottom - h * 0.1), v(x + half * 1.18, bottom), 0.01);
    tip || slug || case || rim
}

/// The shape of a hand-drawn icon: whether a point of its unit square is inside it.
fn shape(icon: Icon) -> Box<dyn Fn(Vec2) -> bool> {
    match icon {
        Icon::Heart => Box::new(|p| {
            // Two lobes and a point, its sides running on from the lobes.
            circle(p, v(0.3, 0.37), 0.205) || circle(p, v(0.7, 0.37), 0.205) || poly(p, &[v(0.3, 0.37), v(0.7, 0.37), v(0.845, 0.515), v(0.5, 0.875), v(0.155, 0.515)])
        }),
        Icon::Bullet => Box::new(|p| bullet(p, 0.5, 0.08, 0.92, 0.15)),
        Icon::Stim => Box::new(|p| {
            // A syringe at 45 degrees, its needle up to the right.
            let q = turn(p, v(0.5, 0.5), -std::f32::consts::FRAC_PI_4);
            let barrel = rect(q, v(0.3, 0.39), v(0.7, 0.61), 0.02) && !rect(q, v(0.33, 0.43), v(0.62, 0.57), 0.01) || rect(q, v(0.3, 0.39), v(0.52, 0.61), 0.02);
            let flange = rect(q, v(0.27, 0.32), v(0.32, 0.68), 0.015);
            let rod = rect(q, v(0.1, 0.465), v(0.3, 0.535), 0.);
            let thumb = rect(q, v(0.06, 0.37), v(0.12, 0.63), 0.015);
            let nose = poly(q, &[v(0.69, 0.43), v(0.78, 0.47), v(0.78, 0.53), v(0.69, 0.57)]);
            let needle = rect(q, v(0.76, 0.48), v(0.96, 0.52), 0.);
            barrel || flange || rod || thumb || nose || needle
        }),
        Icon::Clock => Box::new(|p| {
            // A stopwatch: its face, its hands, its crown.
            let c = v(0.5, 0.56);
            ring(p, c, 0.37, 0.37 - STROKE)
                || seg(p, c, v(0.5, 0.36), STROKE / 2.)
                || seg(p, c, v(0.64, 0.62), STROKE / 2.)
                || rect(p, v(0.42, 0.06), v(0.58, 0.13), 0.02)
                || rect(p, v(0.47, 0.1), v(0.53, 0.2), 0.)
                || seg(p, v(0.8, 0.22), v(0.86, 0.28), STROKE / 2.)
        }),
        Icon::Cat => Box::new(|p| {
            // A cat's head: ears up, slit eyes.
            let head = ellipse(p, v(0.5, 0.58), v(0.38, 0.32));
            let ears = poly(p, &[v(0.13, 0.52), v(0.17, 0.1), v(0.43, 0.32)]) || poly(p, &[v(0.87, 0.52), v(0.83, 0.1), v(0.57, 0.32)]);
            let eyes = ellipse(p, v(0.36, 0.56), v(0.045, 0.085)) || ellipse(p, v(0.64, 0.56), v(0.045, 0.085));
            let nose = poly(p, &[v(0.45, 0.7), v(0.55, 0.7), v(0.5, 0.76)]);
            (head || ears) && !eyes && !nose
        }),
        Icon::Hotdog => Box::new(lying),
        Icon::Bacon => Box::new(|p| {
            // A wavy rasher across, with a streak of fat along it.
            let q = turn(p, v(0.5, 0.5), -0.5);
            let wave = |x: f32| 0.5 + 0.06 * (x * std::f32::consts::TAU * 2.).sin();
            let x = q.x;
            let along = (0.04..=0.96).contains(&x);
            let off = q.y - wave(x);
            along && off.abs() <= 0.19 && !((off - 0.06).abs() <= 0.024 && (0.16..=0.84).contains(&x))
        }),
        Icon::Arrow => Box::new(|p| poly(p, &[v(0.5, 0.06), v(0.86, 0.9), v(0.5, 0.7), v(0.14, 0.9)])),
        Icon::Home => Box::new(|p| {
            let roof = poly(p, &[v(0.5, 0.08), v(0.94, 0.5), v(0.06, 0.5)]);
            let walls = rect(p, v(0.19, 0.42), v(0.81, 0.9), 0.02);
            let door = rect(p, v(0.42, 0.62), v(0.58, 0.95), 0.02);
            (roof || walls) && !door
        }),
        Icon::Skull => Box::new(|p| {
            let cranium = ellipse(p, v(0.5, 0.42), v(0.38, 0.35));
            let jaw = rect(p, v(0.3, 0.55), v(0.7, 0.9), 0.06);
            let eyes = circle(p, v(0.36, 0.46), 0.1) || circle(p, v(0.64, 0.46), 0.1);
            let nose = poly(p, &[v(0.5, 0.58), v(0.56, 0.69), v(0.44, 0.69)]);
            let teeth = rect(p, v(0.425, 0.77), v(0.445, 0.95), 0.) || rect(p, v(0.555, 0.77), v(0.575, 0.95), 0.);
            (cranium || jaw) && !eyes && !nose && !teeth
        }),
        Icon::Crown => Box::new(|p| {
            let points = poly(p, &[v(0.12, 0.74), v(0.1, 0.3), v(0.32, 0.5), v(0.5, 0.2), v(0.68, 0.5), v(0.9, 0.3), v(0.88, 0.74)]);
            let band = rect(p, v(0.12, 0.72), v(0.88, 0.86), 0.02);
            let gems = circle(p, v(0.1, 0.27), 0.065) || circle(p, v(0.5, 0.16), 0.065) || circle(p, v(0.9, 0.27), 0.065);
            points || band || gems
        }),
        Icon::Check => Box::new(|p| path(p, &[v(0.16, 0.52), v(0.4, 0.76), v(0.86, 0.24)], STROKE * 0.75)),
        Icon::Hammer => Box::new(|p| {
            let q = v(0.5, 0.5) + (turn(p, v(0.5, 0.5), -0.75) - v(0.5, 0.5)) * 1.12;
            let handle = rect(q, v(0.45, 0.32), v(0.55, 0.98), 0.04);
            let head = rect(q, v(0.2, 0.12), v(0.8, 0.34), 0.03);
            let face = rect(q, v(0.74, 0.08), v(0.86, 0.38), 0.03);
            handle || head || face
        }),
        Icon::Shield => Box::new(|p| poly(p, &[v(0.5, 0.06), v(0.86, 0.18), v(0.84, 0.48), v(0.74, 0.7), v(0.5, 0.94), v(0.26, 0.7), v(0.16, 0.48), v(0.14, 0.18)])),
        Icon::Infected => Box::new(|p| {
            // A germ: a ball with knobbed spikes.
            let c = v(0.5, 0.5);
            let spikes = (0..8).any(|k| {
                let a = Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4 + 0.2);
                seg(p, c + a * 0.2, c + a * 0.36, 0.035) || circle(p, c + a * 0.38, 0.065)
            });
            let spots = circle(p, v(0.43, 0.44), 0.05) || circle(p, v(0.58, 0.57), 0.04);
            (circle(p, c, 0.25) && !spots) || spikes
        }),
        Icon::Bite => Box::new(|p| {
            // Jaws: a row of teeth down from above and one up from below.
            let upper = rect(p, v(0.08, 0.1), v(0.92, 0.3), 0.08) || (0..5).any(|k| poly(p, &[v(0.1 + 0.16 * k as f32, 0.28), v(0.26 + 0.16 * k as f32, 0.28), v(0.18 + 0.16 * k as f32, 0.47)]));
            let lower = rect(p, v(0.08, 0.7), v(0.92, 0.9), 0.08) || (0..4).any(|k| poly(p, &[v(0.18 + 0.16 * k as f32, 0.72), v(0.34 + 0.16 * k as f32, 0.72), v(0.26 + 0.16 * k as f32, 0.53)]));
            upper || lower
        }),
        Icon::Eat => Box::new(|p| {
            // A fork and a knife.
            let tines = (0..3).any(|k| rect(p, v(0.17 + 0.1 * k as f32, 0.06), v(0.23 + 0.1 * k as f32, 0.34), 0.02));
            let fork = tines || rect(p, v(0.17, 0.28), v(0.43, 0.44), 0.06) || seg(p, v(0.3, 0.42), v(0.3, 0.9), 0.05);
            let blade = poly(p, &[v(0.62, 0.06), v(0.7, 0.08), v(0.8, 0.22), v(0.8, 0.55), v(0.62, 0.55)]);
            let knife = blade || seg(p, v(0.71, 0.55), v(0.71, 0.9), 0.055);
            fork || knife
        }),
        Icon::Strike => Box::new(|p| {
            // An impact: a burst of eight points.
            let c = v(0.5, 0.5);
            let points: Vec<Vec2> = (0..16).map(|k| c + Vec2::from_angle(k as f32 * std::f32::consts::PI / 8. - 0.2) * if k % 2 == 0 { 0.44 } else { 0.2 }).collect();
            poly(p, &points)
        }),
        Icon::Backstab => Box::new(|p| {
            // A blade, point down, with its guard and grip.
            let q = turn(p, v(0.5, 0.5), 0.6);
            let blade = poly(q, &[v(0.44, 0.38), v(0.56, 0.38), v(0.56, 0.78), v(0.5, 0.96), v(0.44, 0.78)]);
            let guard = rect(q, v(0.3, 0.3), v(0.7, 0.38), 0.03);
            let grip = rect(q, v(0.45, 0.06), v(0.55, 0.3), 0.03);
            blade || guard || grip
        }),
        Icon::FaceShot => Box::new(|p| {
            // A face in a sight's crosshairs: a round face with its eyes and its smile cut out, and the sight's four
            // ticks round it.
            let c = v(0.5, 0.5);
            let face = circle(p, c, 0.29);
            let eyes = circle(p, v(0.4, 0.42), 0.055) || circle(p, v(0.6, 0.42), 0.055);
            let smile = ring(p, v(0.5, 0.5), 0.17, 0.17 - STROKE * 0.55) && p.y > 0.57;
            let ticks = rect(p, v(0.46, 0.03), v(0.54, 0.16), 0.) || rect(p, v(0.46, 0.84), v(0.54, 0.97), 0.) || rect(p, v(0.03, 0.46), v(0.16, 0.54), 0.) || rect(p, v(0.84, 0.46), v(0.97, 0.54), 0.);
            (face && !eyes && !smile) || ticks
        }),
        Icon::Turret => Box::new(|p| {
            // A gun on a post on a block.
            let base = rect(p, v(0.12, 0.74), v(0.72, 0.92), 0.04);
            let post = rect(p, v(0.36, 0.56), v(0.48, 0.76), 0.);
            let head = rect(p, v(0.18, 0.36), v(0.66, 0.6), 0.08);
            let barrel = rect(p, v(0.6, 0.43), v(0.94, 0.53), 0.02);
            base || post || head || barrel
        }),
        Icon::Flies => Box::new(|p| {
            let body = ellipse(p, v(0.5, 0.62), v(0.13, 0.25));
            let head = circle(p, v(0.5, 0.3), 0.1);
            let wing = |s: f32| ellipse(turn(p, v(0.5 + s * 0.2, 0.4), s * 0.5), v(0.5 + s * 0.2, 0.4), v(0.19, 0.11));
            let gap = ellipse(p, v(0.5, 0.62), v(0.18, 0.3)) || circle(p, v(0.5, 0.3), 0.145);
            body || head || ((wing(-1.) || wing(1.)) && !gap)
        }),
        Icon::Fall => Box::new(|p| {
            let ledge = rect(p, v(0.06, 0.12), v(0.52, 0.28), 0.02);
            ledge || seg(p, v(0.72, 0.12), v(0.72, 0.62), STROKE / 2.) || head(p, v(0.72, 0.92), Vec2::Y, 0.32)
        }),
        Icon::Hand => Box::new(|p| {
            // An open hand, palm on, thumb out to the left.
            let palm = rect(p, v(0.28, 0.44), v(0.74, 0.9), 0.14);
            let finger = |x: f32, top: f32| seg(p, v(x, top), v(x, 0.6), 0.055);
            let fingers = finger(0.33, 0.2) || finger(0.45, 0.1) || finger(0.57, 0.12) || finger(0.69, 0.22);
            let thumb = seg(p, v(0.34, 0.72), v(0.12, 0.48), 0.06);
            palm || fingers || thumb
        }),
        Icon::Drape => Box::new(|p| drape(p, 0.12, 0.88)),
        Icon::Undrape => Box::new(|p| drape(p, 0.44, 0.94) || seg(p, v(0.5, 0.36), v(0.5, 0.2), STROKE / 2.) || head(p, v(0.5, 0.04), Vec2::NEG_Y, 0.22)),
        Icon::PutDown => Box::new(|p| seg(p, v(0.5, 0.08), v(0.5, 0.44), STROKE / 2.) || head(p, v(0.5, 0.72), Vec2::Y, 0.32) || rect(p, v(0.12, 0.8), v(0.88, 0.9), 0.03)),
        Icon::Throw => Box::new(|p| {
            let arc = curve(v(0.1, 0.9), v(0.3, 0.12), v(0.76, 0.4), 16);
            let end = arc[arc.len() - 1];
            let way = end - arc[arc.len() - 3];
            path(p, &arc[..arc.len() - 2], STROKE / 2.) || head(p, end + way.normalize() * 0.12, way, 0.3) || circle(p, v(0.15, 0.86), 0.09)
        }),
        Icon::Lock => Box::new(|p| {
            let body = rect(p, v(0.18, 0.44), v(0.82, 0.9), 0.07);
            let shackle = ring(p, v(0.5, 0.42), 0.26, 0.26 - STROKE * 1.1) && p.y <= 0.44;
            let keyhole = circle(p, v(0.5, 0.62), 0.065) || rect(p, v(0.47, 0.62), v(0.53, 0.77), 0.);
            (body && !keyhole) || shackle || (rect(p, v(0.24, 0.4), v(0.35, 0.46), 0.) || rect(p, v(0.65, 0.4), v(0.76, 0.46), 0.))
        }),
        Icon::Crouch => Box::new(|p| hotdog(p, 0.5, 0.4, 0.94, 0.12) || path(p, &[v(0.28, 0.08), v(0.5, 0.24), v(0.72, 0.08)], STROKE / 2.)),
        Icon::Prone => Box::new(|p| {
            // The hotdog flat on the ground: lower and squatter, a line under it.
            lying(v(p.x, (p.y - 0.6) / 0.72 + 0.56)) || rect(p, v(0.06, 0.87), v(0.94, 0.94), 0.02)
        }),
        Icon::Infinity => Box::new(|p| {
            let points: Vec<Vec2> = (0..=48)
                .map(|k| {
                    let t = k as f32 / 48. * std::f32::consts::TAU;
                    let d = 1. + t.sin() * t.sin();
                    v(0.5 + 0.4 * t.cos() / d, 0.5 + 0.4 * t.sin() * t.cos() / d)
                })
                .collect();
            path(p, &points, STROKE * 0.55)
        }),
        Icon::Eye => Box::new(|p| {
            let lens = circle(p, v(0.5, 0.98), 0.66) && circle(p, v(0.5, 0.02), 0.66);
            let iris = circle(p, v(0.5, 0.5), 0.19);
            let pupil = circle(p, v(0.5, 0.5), 0.09);
            (lens && !iris) || pupil
        }),
        Icon::AmmoBox => Box::new(|p| {
            let box_ = rect(p, v(0.1, 0.36), v(0.9, 0.88), 0.04);
            let lid = rect(p, v(0.06, 0.24), v(0.94, 0.38), 0.03);
            let seam = rect(p, v(0.06, 0.37), v(0.94, 0.41), 0.);
            let mark = bullet(p, 0.5, 0.48, 0.8, 0.06);
            (box_ || lid) && !seam && !mark
        }),
        Icon::MouseLeft | Icon::MouseRight => {
            let left = icon == Icon::MouseLeft;
            Box::new(move |p| {
                // A mouse: the button pressed solid, the other one only outlined.
                let body = rect(p, v(0.22, 0.06), v(0.78, 0.94), 0.26);
                let split = rect(p, v(0.47, 0.06), v(0.53, 0.42), 0.) || rect(p, v(0.22, 0.4), v(0.78, 0.45), 0.);
                let other = if left { rect(p, v(0.56, 0.14), v(0.7, 0.37), 0.04) } else { rect(p, v(0.3, 0.14), v(0.44, 0.37), 0.04) };
                body && !split && !other
            })
        }
        Icon::Card => Box::new(|p| {
            let back = rect(turn(p, v(0.4, 0.55), -0.28), v(0.18, 0.2), v(0.6, 0.86), 0.06);
            let front = rect(turn(p, v(0.6, 0.5), 0.18), v(0.38, 0.12), v(0.82, 0.84), 0.06);
            let edge = rect(turn(p, v(0.6, 0.5), 0.18), v(0.33, 0.07), v(0.87, 0.89), 0.08);
            front || (back && !edge)
        }),
        Icon::Target => Box::new(|p| {
            let c = v(0.5, 0.5);
            ring(p, c, 0.34, 0.34 - STROKE * 0.85)
                || circle(p, c, 0.08)
                || rect(p, v(0.46, 0.04), v(0.54, 0.26), 0.)
                || rect(p, v(0.46, 0.74), v(0.54, 0.96), 0.)
                || rect(p, v(0.04, 0.46), v(0.26, 0.54), 0.)
                || rect(p, v(0.74, 0.46), v(0.96, 0.54), 0.)
        }),
        Icon::Fridge => Box::new(|p| {
            let body = rect(p, v(0.22, 0.06), v(0.78, 0.94), 0.07);
            let split = rect(p, v(0.22, 0.36), v(0.78, 0.4), 0.);
            let handles = rect(p, v(0.3, 0.16), v(0.35, 0.3), 0.02) || rect(p, v(0.3, 0.46), v(0.35, 0.66), 0.02);
            body && !split && !handles
        }),
        Icon::Pile => Box::new(|p| (0..3).any(|k| rect(p, v(0.12 + 0.06 * k as f32, 0.68 - 0.27 * k as f32), v(0.88 - 0.06 * k as f32, 0.9 - 0.27 * k as f32), 0.06))),
        Icon::Power => Box::new(|p| {
            // A ring open at the top, and a bar down from the top into the gap.
            let c = v(0.5, 0.56);
            let d = p - c;
            let angle = d.x.atan2(-d.y).abs();
            (ring(p, c, 0.38, 0.38 - STROKE * 1.25) && angle > 0.62) || seg(p, v(0.5, 0.1), v(0.5, 0.52), STROKE * 0.65)
        }),
        Icon::Thermometer => Box::new(|p| {
            // The tube and its bulb, a slit up the tube where the mercury rises, and three ticks beside it.
            let body = rect(p, v(0.36, 0.05), v(0.64, 0.7), 0.14) || circle(p, v(0.5, 0.78), 0.2);
            let slit = rect(p, v(0.465, 0.17), v(0.535, 0.62), 0.03);
            let ticks = [0.17, 0.32, 0.47].iter().any(|y| rect(p, v(0.7, *y - 0.02), v(0.86, *y + 0.02), 0.01));
            (body && !slit) || ticks
        }),
        Icon::Flame => Box::new(|p| {
            // A tongue of flame with a hollow low in it.
            let outer = poly(p, &[v(0.52, 0.04), v(0.7, 0.3), v(0.84, 0.58), v(0.74, 0.84), v(0.5, 0.95), v(0.26, 0.84), v(0.16, 0.6), v(0.28, 0.4), v(0.38, 0.5), v(0.42, 0.24)]);
            let hollow = ellipse(p, v(0.5, 0.76), v(0.13, 0.15)) || poly(p, &[v(0.5, 0.48), v(0.6, 0.65), v(0.4, 0.65)]);
            outer && !hollow
        }),
        Icon::Degree => Box::new(|p| ring(p, v(0.5, 0.5), 0.46, 0.46 - 0.22)),
        Icon::Style(style) => style_shape(style),
        Icon::Chi => Box::new(|p| {
            // A spiral wound in two and a half turns from the rim to a dot in the middle.
            let turns = 2.5;
            let points: Vec<Vec2> = (0..=60)
                .map(|k| k as f32 / 60.)
                .map(|t| {
                    let (r, a) = (0.42 * (1. - t) + 0.06 * t, -std::f32::consts::FRAC_PI_2 + t * turns * std::f32::consts::TAU);
                    v(0.5 + r * a.cos(), 0.5 + r * a.sin())
                })
                .collect();
            path(p, &points, STROKE * 0.55) || circle(p, v(0.5, 0.5), 0.07)
        }),
        Icon::Guard => Box::new(|p| {
            // Two forearms crossed at the wrists, the fists up and apart.
            let arm = |a: Vec2, b: Vec2| seg(p, a, b, 0.085);
            let fists = circle(p, v(0.2, 0.2), 0.13) || circle(p, v(0.8, 0.2), 0.13);
            let left = arm(v(0.24, 0.26), v(0.78, 0.88));
            let right = arm(v(0.76, 0.26), v(0.22, 0.88)) && !seg(p, v(0.24, 0.26), v(0.78, 0.88), 0.125);
            fists || left || right
        }),
        Icon::Parry => Box::new(|p| {
            // A raised forearm, and before it an arrow that came in from the right, turned about and went back the way it came.
            let arm = rect(p, v(0.08, 0.1), v(0.24, 0.9), 0.07);
            let turned: Vec<Vec2> = (0..=16).map(|k| std::f32::consts::FRAC_PI_2 + k as f32 / 16. * std::f32::consts::PI).map(|a| v(0.56 + 0.2 * a.cos(), 0.5 - 0.22 * a.sin())).collect();
            let arrow = path(p, &turned, STROKE * 0.6) || seg(p, v(0.56, 0.28), v(0.92, 0.28), STROKE * 0.6) || seg(p, v(0.56, 0.72), v(0.74, 0.72), STROKE * 0.6);
            arm || arrow || head(p, v(0.94, 0.72), v(1., 0.), 0.22)
        }),
        Icon::Broken => Box::new(|p| {
            // A shield split down a jagged crack, its halves pushed apart.
            let shield = |q: Vec2| poly(q, &[v(0.5, 0.06), v(0.86, 0.18), v(0.83, 0.56), v(0.5, 0.94), v(0.17, 0.56), v(0.14, 0.18)]);
            let crack = [v(0.5, 0.02), v(0.43, 0.3), v(0.57, 0.48), v(0.44, 0.7), v(0.5, 0.98)];
            let side = |q: Vec2| {
                // Which side of the crack: left of the zigzag at that height.
                let x = crack.windows(2).find(|w| q.y >= w[0].y && q.y <= w[1].y).map_or(0.5, |w| w[0].x + (w[1].x - w[0].x) * (q.y - w[0].y) / (w[1].y - w[0].y).max(1e-6));
                q.x < x
            };
            let left = { let q = p + v(0.05, 0.); shield(q) && side(q) };
            let right = { let q = p - v(0.05, 0.); shield(q) && !side(q) };
            (left || right) && !path(p, &crack, 0.03)
        }),
        Icon::Halo => Box::new(|p| ring(p, v(0.5, 0.5), 0.47, 0.47 - STROKE * 0.8)),
        Icon::Gun(_) => Box::new(|_| false),
    }
}

/// A Kung Fu style's animal (Icon::Style).
fn style_shape(style: u8) -> Box<dyn Fn(Vec2) -> bool> {
    match style {
        // The tiger: a broad head, round ears, stripes on its brow and cheeks, slanted eyes and its nose.
        0 => Box::new(|p| {
            let head = ellipse(p, v(0.5, 0.56), v(0.39, 0.33)) || circle(p, v(0.2, 0.25), 0.12) || circle(p, v(0.8, 0.25), 0.12);
            let ruff = poly(p, &[v(0.13, 0.5), v(0.03, 0.74), v(0.24, 0.8)]) || poly(p, &[v(0.87, 0.5), v(0.97, 0.74), v(0.76, 0.8)]);
            let stripes = seg(p, v(0.5, 0.26), v(0.5, 0.38), 0.035)
                || seg(p, v(0.38, 0.28), v(0.41, 0.36), 0.026)
                || seg(p, v(0.62, 0.28), v(0.59, 0.36), 0.026)
                || seg(p, v(0.1, 0.52), v(0.24, 0.55), 0.028)
                || seg(p, v(0.9, 0.52), v(0.76, 0.55), 0.028)
                || seg(p, v(0.12, 0.64), v(0.25, 0.64), 0.026)
                || seg(p, v(0.88, 0.64), v(0.75, 0.64), 0.026);
            let eyes = ellipse(turn(p, v(0.36, 0.48), 0.35), v(0.36, 0.48), v(0.07, 0.035)) || ellipse(turn(p, v(0.64, 0.48), -0.35), v(0.64, 0.48), v(0.07, 0.035));
            let nose = poly(p, &[v(0.43, 0.63), v(0.57, 0.63), v(0.5, 0.71)]);
            (head || ruff) && !stripes && !eyes && !nose
        }),
        // The crane: on one leg, the other drawn up, a wing raised, the long neck and the beak.
        1 => Box::new(|p| {
            let body = ellipse(turn(p, v(0.5, 0.44), 0.35), v(0.5, 0.44), v(0.22, 0.12));
            let wing = poly(p, &[v(0.36, 0.38), v(0.06, 0.08), v(0.16, 0.3), v(0.08, 0.34), v(0.3, 0.48)]);
            let neck = path(p, &[v(0.64, 0.4), v(0.75, 0.26), v(0.7, 0.13)], 0.04);
            let head = circle(p, v(0.72, 0.12), 0.06) || poly(p, &[v(0.74, 0.08), v(0.96, 0.15), v(0.74, 0.16)]);
            let leg = seg(p, v(0.52, 0.52), v(0.52, 0.9), 0.028) || seg(p, v(0.42, 0.91), v(0.64, 0.91), 0.025);
            let up = path(p, &[v(0.46, 0.52), v(0.36, 0.64), v(0.47, 0.7)], 0.025);
            body || wing || neck || head || leg || up
        }),
        // The snake: an S of body raised to strike, its head forward, the forked tongue out.
        2 => Box::new(|p| {
            let body = path(p, &curve(v(0.14, 0.86), v(0.62, 0.98), v(0.6, 0.66), 10), 0.075) || path(p, &curve(v(0.6, 0.66), v(0.58, 0.42), v(0.36, 0.42), 10), 0.07) || path(p, &curve(v(0.36, 0.42), v(0.2, 0.24), v(0.48, 0.22), 10), 0.065);
            let head = ellipse(turn(p, v(0.58, 0.2), -0.25), v(0.58, 0.2), v(0.13, 0.085));
            let tongue = seg(p, v(0.7, 0.17), v(0.82, 0.12), 0.018) || seg(p, v(0.82, 0.12), v(0.92, 0.07), 0.016) || seg(p, v(0.82, 0.12), v(0.94, 0.14), 0.016);
            let eye = circle(p, v(0.6, 0.17), 0.028);
            (body || head || tongue) && !eye
        }),
        // The mantis: its triangle of a head, the long thorax, the folded hooks held up, the abdomen and two legs.
        3 => Box::new(|p| {
            let head = poly(p, &[v(0.58, 0.1), v(0.9, 0.17), v(0.66, 0.32)]);
            let eye = circle(p, v(0.66, 0.17), 0.035);
            let thorax = seg(p, v(0.64, 0.28), v(0.47, 0.54), 0.06);
            let abdomen = ellipse(turn(p, v(0.3, 0.68), -0.6), v(0.3, 0.68), v(0.14, 0.27));
            // The raptorial foreleg: the thigh out and down, the shin folded back up against it, a spike on each.
            let hook = seg(p, v(0.62, 0.36), v(0.88, 0.56), 0.05) || seg(p, v(0.88, 0.56), v(0.8, 0.3), 0.04) || poly(p, &[v(0.72, 0.5), v(0.7, 0.62), v(0.78, 0.53)]) || poly(p, &[v(0.8, 0.3), v(0.92, 0.27), v(0.84, 0.36)]);
            let legs = seg(p, v(0.46, 0.56), v(0.3, 0.93), 0.025) || seg(p, v(0.5, 0.56), v(0.62, 0.93), 0.025);
            (head || thorax || abdomen || hook || legs) && !eye
        }),
        // The monkey: a round head, big ears, its face a mask set off by a gap, eyes and mouth.
        4 => Box::new(|p| {
            let face = circle(p, v(0.4, 0.5), 0.14) || circle(p, v(0.6, 0.5), 0.14) || ellipse(p, v(0.5, 0.68), v(0.19, 0.15));
            let around = circle(p, v(0.4, 0.5), 0.185) || circle(p, v(0.6, 0.5), 0.185) || ellipse(p, v(0.5, 0.68), v(0.235, 0.195));
            let head = circle(p, v(0.5, 0.52), 0.33) || circle(p, v(0.14, 0.5), 0.12) || circle(p, v(0.86, 0.5), 0.12);
            let holes = circle(p, v(0.42, 0.5), 0.045) || circle(p, v(0.58, 0.5), 0.045) || seg(p, v(0.43, 0.73), v(0.57, 0.73), 0.022) || circle(p, v(0.14, 0.5), 0.05) || circle(p, v(0.86, 0.5), 0.05);
            ((head && !around) || face) && !holes
        }),
        // Drunken: a gourd, the cord round its waist with a tassel, its stopper.
        5 => Box::new(|p| {
            let gourd = circle(p, v(0.46, 0.34), 0.17) || circle(p, v(0.46, 0.66), 0.28) || rect(p, v(0.37, 0.34), v(0.55, 0.55), 0.);
            let stopper = rect(p, v(0.39, 0.06), v(0.53, 0.19), 0.02);
            let cord = seg(p, v(0.24, 0.47), v(0.68, 0.47), 0.024);
            let tassel = path(p, &[v(0.64, 0.47), v(0.82, 0.56), v(0.88, 0.76)], 0.026);
            (gourd || stopper || tassel) && !cord
        }),
        // The toad: squat and wide, its eyes bulging on top, its mouth a wide line, its legs splayed.
        _ => Box::new(|p| {
            let body = ellipse(p, v(0.5, 0.6), v(0.37, 0.24));
            let eyes = circle(p, v(0.3, 0.36), 0.12) || circle(p, v(0.7, 0.36), 0.12);
            let pupils = circle(p, v(0.3, 0.35), 0.045) || circle(p, v(0.7, 0.35), 0.045);
            let legs = ellipse(p, v(0.22, 0.84), v(0.16, 0.07)) || ellipse(p, v(0.78, 0.84), v(0.16, 0.07)) || ellipse(p, v(0.09, 0.66), v(0.07, 0.14)) || ellipse(p, v(0.91, 0.66), v(0.07, 0.14));
            let mouth = path(p, &curve(v(0.28, 0.6), v(0.5, 0.7), v(0.72, 0.6), 8), 0.022);
            (body || eyes || legs) && !pupils && !mouth
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(data: &[u8], w: u32, x: u32, y: u32) -> u8 {
        data[((y * w + x) * 4 + 3) as usize]
    }
    fn coverage(data: &[u8]) -> (usize, usize) {
        // (Pixels mostly white and opaque, and pixels mostly dark: the fill and the outline.)
        let (mut fill, mut ink) = (0, 0);
        for px in data.chunks_exact(4) {
            if px[3] > 200 && px[0] > 220 {
                fill += 1;
            } else if px[3] > 120 && px[0] < 60 {
                ink += 1;
            }
        }
        (fill, ink)
    }

    #[test]
    fn every_icon_is_a_white_shape_with_a_dark_outline_and_clear_corners() {
        for icon in Icon::DRAWN.into_iter().chain(Icon::GUNS) {
            for height in [18, 28, 42] {
                let (w, h) = size(icon, height);
                let data = raster(icon, w, h);
                assert_eq!(data.len() as u32, w * h * 4);
                let (fill, ink) = coverage(&data);
                let area = (w * h) as usize;
                assert!(fill > area / 40 && ink > area / 40, "{icon:?} at {height}: {fill} filled, {ink} outlined of {area}");
                assert!(fill + ink < area * 9 / 10, "{icon:?} at {height} fills its box");
                for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
                    assert_eq!(alpha(&data, w, x, y), 0, "{icon:?} at {height}: corner {x},{y}");
                }
            }
        }
    }

    #[test]
    fn nothing_reaches_the_edge_so_the_outline_is_whole() {
        for icon in Icon::DRAWN.into_iter().chain(Icon::GUNS) {
            let (w, h) = size(icon, 32);
            let data = raster(icon, w, h);
            for x in 0..w {
                for y in [0, h - 1] {
                    assert!(alpha(&data, w, x, y) < 40, "{icon:?} touches the top or the bottom at {x}");
                }
            }
            for y in 0..h {
                for x in [0, w - 1] {
                    assert!(alpha(&data, w, x, y) < 40, "{icon:?} touches a side at {y}");
                }
            }
        }
    }

    /// Writes every icon as raw RGBA to $ICON_DUMP (a folder), for a person to look at: `ICON_DUMP=dir cargo test -- --ignored dump`.
    #[test]
    #[ignore]
    fn dump_the_icons() {
        let Ok(dir) = std::env::var("ICON_DUMP") else { return };
        for icon in Icon::DRAWN {
            for height in [24, 64] {
                let (w, h) = size(icon, height);
                std::fs::write(format!("{dir}/{icon:?}-{w}x{h}.rgba"), raster(icon, w, h)).unwrap();
            }
        }
    }

    #[test]
    fn the_icons_are_told_apart() {
        let all: Vec<Icon> = Icon::DRAWN.into_iter().chain(Icon::GUNS).collect();
        let masks: Vec<Vec<bool>> = all
            .iter()
            .map(|icon| {
                let (w, h) = size(*icon, 32);
                raster(*icon, w, h).chunks_exact(4).map(|px| px[3] > 200 && px[0] > 200).collect()
            })
            .collect();
        for a in 0..all.len() {
            for b in a + 1..all.len() {
                if masks[a].len() != masks[b].len() {
                    continue;
                }
                let differ = masks[a].iter().zip(&masks[b]).filter(|(x, y)| x != y).count();
                assert!(differ > masks[a].len() / 25, "{:?} and {:?} look alike ({differ} pixels apart)", all[a], all[b]);
            }
        }
    }

    #[test]
    fn an_icon_is_drawn_once_for_each_height_it_is_asked_for() {
        let mut images = Assets::<Image>::default();
        let mut icons = Icons::default();
        let a = icons.get(&mut images, Icon::Heart, 28);
        assert_eq!(icons.get(&mut images, Icon::Heart, 28), a);
        let b = icons.get(&mut images, Icon::Heart, 42);
        assert_ne!(a, b);
        let gun = icons.get(&mut images, Icon::Gun(6), 30);
        let image = images.get(&gun).unwrap();
        assert_eq!((image.width(), image.height()), (78, 30));
        assert_eq!(images.get(&b).map(|i| (i.width(), i.height())), Some((42, 42)));
    }

    #[test]
    fn the_distance_field_is_euclidean() {
        // One sample inside, in the middle of a 9 by 9 grid: the corner is sqrt(32) away.
        let mut mask = vec![false; 81];
        mask[40] = true;
        let d = distance(&mask, 9, 9, true);
        assert_eq!(d[40], 0.);
        assert_eq!(d[0], 32.);
        assert_eq!(d[4], 16.);
        // Opening takes a line one sample thick away and keeps a block.
        let mut mask = vec![false; 400];
        for x in 2..18 {
            mask[3 * 20 + x] = true;
            for y in 8..16 {
                mask[y * 20 + x] = true;
            }
        }
        open(&mut mask, 20, 20, 1.5);
        assert!(!mask[3 * 20 + 10] && mask[12 * 20 + 10]);
    }
}

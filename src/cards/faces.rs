// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The cards' faces, printed in code into small textures ([`print`]). Each kind of card shows a picture of its food,
//! or of what an action card does, in a window in the middle of the card's colour, drawn from flat shapes (discs,
//! ovals, rounded boxes, polygons and strokes) in dark outlines, as a cartoon is drawn; and its value in seven-segment
//! figures in its top-left and bottom-right corners, as on a playing card. The action cards have no value, and their
//! picture fills the card.
//!
//! The pictures are this game's own, made here from shapes: nothing is copied from anywhere, nor drawn after any other
//! card game's. They are bold and flat so that they read across the table, where a card is a couple of centimetres
//! seen from a hand's length or two away.
//!
//! Each shape is painted only within its box ([`Shape`]), and worked out pixel by pixel only near its edge
//! (`Canvas::layers`). The table prints the 22 kinds of card a few a frame as it is first laid, and deals once all are
//! printed (`Kit::print`), never in the middle of a deal.
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use kitchen_sim::cards::Card;
use std::{f32::consts::PI, rc::Rc, sync::LazyLock};

/// What a card shows: its colour, its name (`Card::name`), the value printed in its corners (a topping's value, a
/// side's points, the Foot-Long's 2), the sign before that value, and its picture. The 3D card on the table is printed
/// from it (`print`).
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub colour: Color,
    pub label: String,
    pub value: Option<u8>,
    pub glyph: Glyph,
    pub picture: Picture,
}
/// The sign before the value in a card's corners: nothing before a topping's, a plus before a side's, a times before
/// the Foot-Long's 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    None,
    Plus,
    Times,
}
/// The picture in the middle of a card: one for each kind of card.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Picture {
    /// Plain Bun: an empty bun.
    Bun,
    /// Celery Salt: a shaker.
    Shaker,
    /// Sport Peppers: a bunch of small yellow-green peppers.
    SportPeppers,
    /// Jalapeños: one green jalapeño.
    Jalapeno,
    /// Bacon: a wavy rasher.
    Bacon,
    /// Cheese: a wedge with holes.
    Cheese,
    /// Chili: a steaming bowl of it.
    Chili,
    /// Sauerkraut: a pale shredded heap on a plate.
    Sauerkraut,
    /// Pickles: round crinkle-cut slices.
    Pickles,
    /// Onions: a red onion.
    Onion,
    /// Relish: a green dollop.
    Relish,
    /// Mustard: a yellow squeeze bottle.
    Mustard,
    /// Ketchup: a red squeeze bottle.
    Ketchup,
    /// Fries: a carton of them.
    Fries,
    /// Chips: a few of them.
    Chips,
    /// Pickle Spear: a long spear.
    Spear,
    /// Soda: a cup with a straw.
    Soda,
    /// Onion Rings: three rings.
    Rings,
    /// Foot-Long: a long hotdog.
    FootLong,
    /// Back in the Fridge: a fridge.
    Fridge,
    /// Pile It On: toppings piled high.
    Pile,
    /// Napkin: a folded napkin.
    Napkin,
}
/// The toppings' pictures, by value (Plain Bun to Ketchup).
const TOPPING_PICTURES: [Picture; 13] = [
    Picture::Bun,
    Picture::Shaker,
    Picture::SportPeppers,
    Picture::Jalapeno,
    Picture::Bacon,
    Picture::Cheese,
    Picture::Chili,
    Picture::Sauerkraut,
    Picture::Pickles,
    Picture::Onion,
    Picture::Relish,
    Picture::Mustard,
    Picture::Ketchup,
];
/// The colour of each topping, by value (Plain Bun to Ketchup): what it looks like on a hotdog, no two alike.
const TOPPING_RGB: [[f32; 3]; 13] = [
    [0.88, 0.67, 0.40],
    [0.74, 0.81, 0.69],
    [0.64, 0.83, 0.20],
    [0.13, 0.46, 0.20],
    [0.87, 0.38, 0.36],
    [1.00, 0.58, 0.10],
    [0.52, 0.20, 0.10],
    [0.94, 0.89, 0.62],
    [0.46, 0.50, 0.10],
    [0.75, 0.56, 0.87],
    [0.10, 0.64, 0.48],
    [0.98, 0.84, 0.10],
    [0.86, 0.10, 0.10],
];
/// The sides' colour (the Foot-Long's too), and the action cards': Back in the Fridge's icy blue, Pile It On's violet,
/// the Napkin's cloth grey-blue.
const SIDE_RGB: [f32; 3] = [0.40, 0.67, 0.95];
const FRIDGE_RGB: [f32; 3] = [0.66, 0.89, 0.97];
const PILE_RGB: [f32; 3] = [0.42, 0.24, 0.68];
const NAPKIN_RGB: [f32; 3] = [0.60, 0.66, 0.76];
/// A card's white, the dark ink its outlines are drawn in (and its figures on a light colour), the white of its
/// figures on a dark one, and the back's red with its squiggle of mustard.
pub(super) const PAPER: [f32; 3] = [0.97, 0.95, 0.89];
const INK: [f32; 3] = [0.10, 0.08, 0.07];
const CHALK: [f32; 3] = [0.99, 0.98, 0.95];
const BACK_RGB: [f32; 3] = [0.60, 0.10, 0.08];
pub(super) const MUSTARD: [f32; 3] = [0.98, 0.80, 0.12];

pub fn card_face(card: Card) -> Face {
    let (rgb, value, glyph, picture) = match card {
        Card::Topping(value) => {
            let i = (value as usize).min(TOPPING_RGB.len() - 1);
            (TOPPING_RGB[i], Some(value), Glyph::None, TOPPING_PICTURES[i])
        }
        Card::Side(value) => {
            let picture = match value {
                4 => Picture::Chips,
                6 => Picture::Spear,
                8 => Picture::Soda,
                10 => Picture::Rings,
                _ => Picture::Fries,
            };
            (SIDE_RGB, Some(value), Glyph::Plus, picture)
        }
        Card::FootLong => (SIDE_RGB, Some(2), Glyph::Times, Picture::FootLong),
        Card::Fridge => (FRIDGE_RGB, None, Glyph::None, Picture::Fridge),
        Card::PileOn => (PILE_RGB, None, Glyph::None, Picture::Pile),
        Card::Napkin => (NAPKIN_RGB, None, Glyph::None, Picture::Napkin),
    };
    Face { colour: super::srgb(rgb), label: card.name(), value, glyph, picture }
}

// ----- Shapes, and painting them.

/// A print's size in pixels: a card's shape, across and down.
const PRINT: [usize; 2] = [128, 176];
const SIZE: Vec2 = Vec2::new(PRINT[0] as f32, PRINT[1] as f32);
/// How wide the ink is round a picture's shapes, in pixels.
const LINE: f32 = 3.;

/// A shape to paint, in a print's pixels (y down): how far a point is inside its edge (negative outside), and the box
/// it lies within, `min` to `max`, which is all of the print that painting it walks.
#[derive(Clone)]
struct Shape {
    min: Vec2,
    max: Vec2,
    inside: Rc<dyn Fn(Vec2) -> f32>,
}
impl Shape {
    fn new(min: Vec2, max: Vec2, inside: impl Fn(Vec2) -> f32 + 'static) -> Shape {
        Shape { min, max, inside: Rc::new(inside) }
    }
    fn at(&self, p: Vec2) -> f32 {
        (self.inside)(p)
    }
    /// `by` pixels fatter all round (thinner, given less than nothing).
    fn grow(&self, by: f32) -> Shape {
        let shape = self.clone();
        Shape::new(self.min - Vec2::splat(by.max(0.)), self.max + Vec2::splat(by.max(0.)), move |p| shape.at(p) + by)
    }
    /// A band `width` wide along its edge.
    fn edge(&self, width: f32) -> Shape {
        let shape = self.clone();
        Shape::new(self.min - Vec2::splat(width / 2.), self.max + Vec2::splat(width / 2.), move |p| width / 2. - shape.at(p).abs())
    }
    /// This and `other` together.
    fn or(&self, other: &Shape) -> Shape {
        let (a, b) = (self.clone(), other.clone());
        Shape::new(self.min.min(other.min), self.max.max(other.max), move |p| a.at(p).max(b.at(p)))
    }
    /// Where this and `other` overlap.
    fn and(&self, other: &Shape) -> Shape {
        let (a, b) = (self.clone(), other.clone());
        Shape::new(self.min.max(other.min), self.max.min(other.max), move |p| a.at(p).min(b.at(p)))
    }
    /// This without `other`.
    fn less(&self, other: &Shape) -> Shape {
        let (a, b) = (self.clone(), other.clone());
        Shape::new(self.min, self.max, move |p| a.at(p).min(-b.at(p)))
    }
    /// Turned `angle` radians about `about`: clockwise on the print, whose y is down.
    fn turn(&self, about: Vec2, angle: f32) -> Shape {
        let shape = self.clone();
        let (there, back) = (Vec2::from_angle(angle), Vec2::from_angle(-angle));
        let corners = [self.min, vec2(self.max.x, self.min.y), self.max, vec2(self.min.x, self.max.y)].map(|c| about + there.rotate(c - about));
        let min = corners.iter().fold(Vec2::INFINITY, |m, c| m.min(*c));
        let max = corners.iter().fold(Vec2::NEG_INFINITY, |m, c| m.max(*c));
        Shape::new(min, max, move |p| shape.at(about + back.rotate(p - about)))
    }
}
/// The whole print.
fn everywhere(inside: impl Fn(Vec2) -> f32 + 'static) -> Shape {
    Shape::new(Vec2::ZERO, SIZE, inside)
}
/// A disc.
fn disc(centre: Vec2, radius: f32) -> Shape {
    Shape::new(centre - Vec2::splat(radius), centre + Vec2::splat(radius), move |p| radius - p.distance(centre))
}
/// An oval, `radii` across and down. (Its distance is found by refining a guess at the nearest point of its edge twice
/// over, which is within a tenth of a pixel for these ovals: as near exact as a print needs.)
fn oval(centre: Vec2, radii: Vec2) -> Shape {
    let (a, b) = (radii.x, radii.y);
    let spread = a * a - b * b;
    Shape::new(centre - radii, centre + radii, move |p| {
        let q = (p - centre).abs();
        let mut t = Vec2::splat(std::f32::consts::FRAC_1_SQRT_2);
        for _ in 0..2 {
            let edge = vec2(spread * t.x.powi(3) / a, -spread * t.y.powi(3) / b);
            let (r, s) = (vec2(a * t.x, b * t.y) - edge, q - edge);
            let along = s * (r.length() / s.length().max(1e-6)) + edge;
            t = (along / radii).clamp(Vec2::ZERO, Vec2::ONE).normalize_or(Vec2::X);
        }
        let distance = q.distance(radii * t);
        if (q / radii).length_squared() < 1. { distance } else { -distance }
    })
}
/// A box about `middle`, `half` its size each way, its corners rounded by `radius`.
fn rounded(middle: Vec2, half: Vec2, radius: f32) -> Shape {
    Shape::new(middle - half, middle + half, move |p| {
        let q = (p - middle).abs() - half + Vec2::splat(radius);
        radius - q.max(Vec2::ZERO).length() - q.x.max(q.y).min(0.)
    })
}
/// A stroke from `a` to `b`, `width` thick, with round ends.
fn stroke(a: Vec2, b: Vec2, width: f32) -> Shape {
    path(&[a, b], width)
}
/// Strokes through `points` one after another, `width` thick, with round ends and joins.
fn path(points: &[Vec2], width: f32) -> Shape {
    let points = points.to_vec();
    let half = Vec2::splat(width / 2.);
    let min = points.iter().fold(Vec2::INFINITY, |m, p| m.min(*p)) - half;
    let max = points.iter().fold(Vec2::NEG_INFINITY, |m, p| m.max(*p)) + half;
    Shape::new(min, max, move |p| {
        let mut near = f32::INFINITY;
        for pair in points.windows(2) {
            let (pa, ba) = (p - pair[0], pair[1] - pair[0]);
            let along = (pa.dot(ba) / ba.length_squared().max(1e-6)).clamp(0., 1.);
            near = near.min((pa - ba * along).length_squared());
        }
        width / 2. - near.sqrt()
    })
}
/// A stroke from `a` to `b` that tapers from `ra` round at `a` to `rb` round at `b` (a pepper, a drop, a leaf).
fn taper(a: Vec2, b: Vec2, ra: f32, rb: f32) -> Shape {
    let h = a.distance(b).max(1e-3);
    let along = (b - a) / h;
    let slope = ((ra - rb) / h).clamp(-0.999, 0.999);
    let rise = (1. - slope * slope).sqrt();
    let r = Vec2::splat(ra.max(rb));
    Shape::new(a.min(b) - r, a.max(b) + r, move |p| {
        let q = p - a;
        let q = vec2(along.perp_dot(q).abs(), q.dot(along));
        let k = q.dot(vec2(-slope, rise));
        let distance = if k < 0. {
            q.length() - ra
        } else if k > rise * h {
            (q - vec2(0., h)).length() - rb
        } else {
            q.dot(vec2(rise, slope)) - ra
        };
        -distance
    })
}
/// A polygon through `points` (any simple one, either way round).
fn polygon(points: &[Vec2]) -> Shape {
    let points = points.to_vec();
    let min = points.iter().fold(Vec2::INFINITY, |m, p| m.min(*p));
    let max = points.iter().fold(Vec2::NEG_INFINITY, |m, p| m.max(*p));
    Shape::new(min, max, move |p| {
        let mut near = (p - points[0]).length_squared();
        let mut sign = 1.;
        let mut j = points.len() - 1;
        for i in 0..points.len() {
            let (e, w) = (points[j] - points[i], p - points[i]);
            let b = w - e * (w.dot(e) / e.length_squared().max(1e-6)).clamp(0., 1.);
            near = near.min(b.length_squared());
            let (up, down, left) = (p.y >= points[i].y, p.y < points[j].y, e.x * w.y > e.y * w.x);
            if (up && down && left) || (!up && !down && !left) {
                sign = -sign;
            }
            j = i;
        }
        -sign * near.sqrt()
    })
}
/// A disc with a crinkled edge: `bumps` scallops round it (a whole number of them), reaching `depth` past `radius`.
fn crinkle(centre: Vec2, radius: f32, depth: f32, bumps: f32) -> Shape {
    let step = 2. * PI / bumps;
    let small = radius * step * 0.6;
    let scallops: Vec<Vec2> = (0..bumps as usize).map(|k| Vec2::from_angle(k as f32 * step) * (radius + depth - small)).collect();
    let reach = Vec2::splat(radius + depth);
    Shape::new(centre - reach, centre + reach, move |p| {
        let q = p - centre;
        let k = (q.y.atan2(q.x) / step).floor() as isize;
        let scallop = |k: isize| small - q.distance(scallops[k.rem_euclid(scallops.len() as isize) as usize]);
        (radius - depth - q.length()).max(scallop(k)).max(scallop(k + 1))
    })
}
/// Stripes `width` wide, `every` pixels apart, across the way `way` (of length 1), one of them through `at`.
fn stripes(at: Vec2, way: Vec2, every: f32, width: f32) -> Shape {
    everywhere(move |p| {
        let t = (p - at).dot(way);
        width / 2. - (t - every * (t / every).round()).abs()
    })
}
/// A band `width` wide that waves from `a` to `b`, `high` either side of the straight way, in `halves` half waves, `off`
/// pixels to the right of it; its ends cut square.
fn wave(a: Vec2, b: Vec2, high: f32, halves: f32, off: f32, width: f32) -> Shape {
    let (along, across) = axes(a, b);
    let long = a.distance(b);
    let k = halves * PI / long;
    let reach = Vec2::splat(high + off.abs() + width / 2.);
    Shape::new(a.min(b) - reach, a.max(b) + reach, move |p| {
        let q = p - a;
        let (u, v) = (q.dot(along), q.dot(across));
        let slope = high * k * (k * u).cos();
        let apart = (v - off - high * (k * u).sin()).abs() / (1. + slope * slope).sqrt();
        (width / 2. - apart).min(u).min(long - u)
    })
}
/// The half of the print on the left of the way from `a` to `b`, as it is seen (y down).
fn side(a: Vec2, b: Vec2) -> Shape {
    let along = (b - a).normalize_or(Vec2::X);
    everywhere(move |p| -along.perp_dot(p - a))
}
/// The way from `a` to `b`, and the way a quarter turn clockwise from it (down, for a way to the right).
fn axes(a: Vec2, b: Vec2) -> (Vec2, Vec2) {
    let along = (b - a).normalize_or(Vec2::X);
    (along, along.perp())
}

/// A picture being printed, its pixels in sRGB.
struct Canvas {
    px: Vec<[f32; 3]>,
    /// Work out every pixel of the print for every shape, not only those in its box and near its edge (the tests check
    /// that the quick way paints the same).
    whole: bool,
}
/// A shape is painted a square of the print at a time, BLOCK pixels across and then each quarter of it in turn: a square
/// whose middle is further from the shape's edge than its corners are from its middle (by SLACK times, room to spare
/// for a shape whose distance is a close guess) is all inside the shape or all outside it, and is painted, or passed
/// over, whole. Only the squares the edge crosses are worked out pixel by pixel.
const BLOCK: usize = 16;
const SLACK: f32 = 1.25;
impl Canvas {
    fn new(rgb: [f32; 3]) -> Canvas {
        Canvas { px: vec![rgb; PRINT[0] * PRINT[1]], whole: false }
    }
    /// Paints `rgb` over `shape`: a pixel its edge crosses takes as much of the colour as the shape covers of it.
    fn paint(&mut self, rgb: [f32; 3], shape: &Shape) {
        self.layers(shape, &[(rgb, 0.)]);
    }
    /// Paints `rgb` over `shape` inside an outline of ink `line` wide.
    fn outlined(&mut self, rgb: [f32; 3], shape: &Shape, line: f32) {
        self.layers(shape, &[(INK, line), (rgb, 0.)]);
    }
    /// Paints `rgb` over `shape` inside an outline of ink LINE wide: the cartoon's way.
    fn inked(&mut self, rgb: [f32; 3], shape: &Shape) {
        self.outlined(rgb, shape, LINE);
    }
    /// Paints each colour of `layers` in turn over `shape` grown by so many pixels (an outline and the shape inside it,
    /// worked out at once). Only the shape's box is walked, a BLOCK at a time.
    fn layers(&mut self, shape: &Shape, layers: &[([f32; 3], f32)]) {
        let grow = layers.iter().fold(0_f32, |most, (_, by)| most.max(*by));
        let (min, max) = if self.whole { (Vec2::ZERO, SIZE) } else { ((shape.min - Vec2::splat(grow + 1.)).floor().max(Vec2::ZERO), (shape.max + Vec2::splat(grow + 1.)).ceil().min(SIZE)) };
        let ([x0, y0], [x1, y1]) = (min.to_array().map(|v| v as usize), max.to_array().map(|v| v as usize));
        if self.whole {
            self.pixel_by_pixel(shape, layers, [x0, y0, x1, y1]);
            return;
        }
        for y in (y0..y1).step_by(BLOCK) {
            for x in (x0..x1).step_by(BLOCK) {
                self.block(shape, layers, [x, y, (x + BLOCK).min(x1), (y + BLOCK).min(y1)]);
            }
        }
    }
    /// Paints `layers` over `shape` in the square from `x0, y0` to (short of) `x1, y1`: whole, not at all, or a quarter
    /// at a time.
    fn block(&mut self, shape: &Shape, layers: &[([f32; 3], f32)], [x0, y0, x1, y1]: [usize; 4]) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let inside = shape.at(vec2((x0 + x1) as f32, (y0 + y1) as f32) / 2.);
        let reach = vec2((x1 - x0 - 1) as f32, (y1 - y0 - 1) as f32).length() / 2. * SLACK + 0.5;
        let (most, least) = layers.iter().fold((f32::MIN, f32::MAX), |(most, least), (_, by)| (most.max(*by), least.min(*by)));
        if inside + most < -reach {
            return;
        }
        if inside + least > reach {
            let rgb = layers[layers.len() - 1].0;
            for y in y0..y1 {
                self.px[y * PRINT[0] + x0..y * PRINT[0] + x1].fill(rgb);
            }
            return;
        }
        if (x1 - x0) * (y1 - y0) <= 16 {
            self.pixel_by_pixel(shape, layers, [x0, y0, x1, y1]);
            return;
        }
        let (xm, ym) = ((x0 + x1).div_ceil(2), (y0 + y1).div_ceil(2));
        for quarter in [[x0, y0, xm, ym], [xm, y0, x1, ym], [x0, ym, xm, y1], [xm, ym, x1, y1]] {
            self.block(shape, layers, quarter);
        }
    }
    /// Paints `layers` over `shape` in the square from `x0, y0` to (short of) `x1, y1`, working out each pixel.
    fn pixel_by_pixel(&mut self, shape: &Shape, layers: &[([f32; 3], f32)], [x0, y0, x1, y1]: [usize; 4]) {
        for y in y0..y1 {
            for x in x0..x1 {
                let inside = shape.at(vec2(x as f32 + 0.5, y as f32 + 0.5));
                let px = &mut self.px[y * PRINT[0] + x];
                for (rgb, by) in layers {
                    let cover = (inside + by + 0.5).clamp(0., 1.);
                    if cover > 0. {
                        for c in 0..3 {
                            px[c] += (rgb[c] - px[c]) * cover;
                        }
                    }
                }
            }
        }
    }
}
/// A run of numbers from 0 up to 1 that is the same every time: where a picture's crumbs, shreds and bits go.
struct Scatter(u32);
impl Scatter {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 16_777_216.
    }
    /// A point at least `margin` inside `shape` (or, should 64 tries find none, the middle of its box).
    fn inside(&mut self, shape: &Shape, margin: f32) -> Vec2 {
        for _ in 0..64 {
            let p = shape.min + (shape.max - shape.min) * vec2(self.next(), self.next());
            if shape.at(p) >= margin {
                return p;
            }
        }
        (shape.min + shape.max) / 2.
    }
}

// ----- The corners.

/// The figures' height in a card's corners, the top-left corner of the top-left ones, and the space between two marks
/// (of the height).
const INDEX: f32 = 30.;
const CORNER: Vec2 = Vec2::new(14., 13.);
const SPACE: f32 = 0.14;
/// The seven-segment figures 0 to 9: segment `i` (a to g, from the top round clockwise and then the middle) is lit
/// where bit `i` is.
const SEGMENTS: [u8; 10] = [0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111, 0b1111111, 0b1101111];
/// What is printed in a card's corner: a sign or a figure.
#[derive(Clone, Copy)]
enum Mark {
    Plus,
    Times,
    Figure(u8),
}
impl Mark {
    /// How wide it is, of its height.
    fn width(self) -> f32 {
        if matches!(self, Mark::Figure(_)) { 0.56 } else { 0.42 }
    }
}
/// What a card's corners say: its sign, then its value's figures.
fn marks(glyph: Glyph, value: u8) -> Vec<Mark> {
    let mut marks = match glyph {
        Glyph::None => vec![],
        Glyph::Plus => vec![Mark::Plus],
        Glyph::Times => vec![Mark::Times],
    };
    if value >= 10 {
        marks.push(Mark::Figure(value / 10 % 10));
    }
    marks.push(Mark::Figure(value % 10));
    marks
}
/// How wide `marks` are, INDEX high.
fn breadth(marks: &[Mark]) -> f32 {
    (marks.iter().map(|mark| mark.width()).sum::<f32>() + SPACE * marks.len().saturating_sub(1) as f32) * INDEX
}
/// The box the top-left corner's `marks` take on the card (the bottom-right's is it turned half round the middle).
#[cfg(test)]
fn index_box(marks: &[Mark]) -> (Vec2, Vec2) {
    (CORNER, CORNER + vec2(breadth(marks), INDEX))
}
/// The strokes of `marks`, seven-segment figures INDEX high, left to right from `at` (their top-left corner).
fn figures(marks: &[Mark], at: Vec2) -> Vec<Shape> {
    let h = INDEX;
    let t = h * 0.17;
    let mut x = at.x;
    let mut strokes = vec![];
    for mark in marks {
        let w = mark.width() * h;
        let (left, right, top, middle, foot) = (x + t / 2., x + w - t / 2., at.y + t / 2., at.y + h / 2., at.y + h - t / 2.);
        let centre = vec2(x + w / 2., middle);
        let arm = w / 2. - t / 2.;
        match *mark {
            Mark::Figure(figure) => {
                let segments = [
                    (vec2(left, top), vec2(right, top)),
                    (vec2(right, top), vec2(right, middle)),
                    (vec2(right, middle), vec2(right, foot)),
                    (vec2(left, foot), vec2(right, foot)),
                    (vec2(left, middle), vec2(left, foot)),
                    (vec2(left, top), vec2(left, middle)),
                    (vec2(left, middle), vec2(right, middle)),
                ];
                for (i, (a, b)) in segments.into_iter().enumerate() {
                    if SEGMENTS[figure as usize % 10] & (1 << i) != 0 {
                        strokes.push(stroke(a, b, t));
                    }
                }
            }
            Mark::Plus => {
                strokes.push(stroke(centre - Vec2::X * arm, centre + Vec2::X * arm, t));
                strokes.push(stroke(centre - Vec2::Y * arm, centre + Vec2::Y * arm, t));
            }
            Mark::Times => {
                let arm = arm * 0.85;
                strokes.push(stroke(centre - Vec2::splat(arm), centre + Vec2::splat(arm), t));
                strokes.push(stroke(centre + vec2(-arm, arm), centre + vec2(arm, -arm), t));
            }
        }
        x += w + SPACE * h;
    }
    strokes
}

// ----- The print.

/// The window the picture sits in on a card with a value in its corners, between them, and on an action card, which
/// has none: half its size each way, about the card's middle.
const WINDOW: Vec2 = Vec2::new(52., 40.);
const TALL_WINDOW: Vec2 = Vec2::new(52., 75.);

/// A card's face as pixels: white, with a panel of its colour; on it a window of a paler shade with the card's picture
/// in it, and its value in the corners in whichever of dark ink and white stands out from the colour more. RGBA, sRGB,
/// with the smaller sizes for far away after it, and how many sizes there are.
pub(super) fn print(face: &Face) -> (Vec<u8>, u32) {
    pixels(&printed(face, false))
}
/// A card's face (`print`), as a canvas; `whole` paints every shape over the whole print (`Canvas::whole`).
fn printed(face: &Face, whole: bool) -> Canvas {
    let colour = super::rgb_of(face.colour);
    let mut canvas = Canvas::new(colour);
    canvas.whole = whole;
    let middle = SIZE / 2.;
    // (The white edge round the panel of colour: painted round it, not under it, which spares walking the panel.)
    let panel = rounded(middle, middle - Vec2::splat(6.), 10.);
    canvas.paint(PAPER, &everywhere(move |p| -panel.at(p)));
    let marks = face.value.map_or(vec![], |value| marks(face.glyph, value));
    let window = rounded(middle, if marks.is_empty() { TALL_WINDOW } else { WINDOW }, 12.);
    canvas.layers(&window, &[(colour.map(|c| c * 0.8), 2.), (super::lighter(colour, 0.72), 0.)]);
    picture(&mut canvas, face.picture);
    let light = 0.2126 * colour[0] + 0.7152 * colour[1] + 0.0722 * colour[2] > 0.55;
    let ink = if light { INK } else { CHALK };
    // (The bottom-right figures are upright too, not upside down as on a playing card: every card on the table is
    // turned upright to the one who looks at it, and a 6 upside down would read as a 9.)
    let foot = SIZE - CORNER - vec2(breadth(&marks), INDEX);
    for stroke in figures(&marks, CORNER).iter().chain(&figures(&marks, foot)) {
        canvas.paint(ink, stroke);
    }
    canvas
}
/// The back of a card (the draw pile's top): red inside a white edge, with a squiggle of mustard down it.
pub(super) fn print_back() -> (Vec<u8>, u32) {
    let mut canvas = Canvas::new(PAPER);
    let middle = SIZE / 2.;
    canvas.paint(BACK_RGB, &rounded(middle, middle - Vec2::splat(7.), 9.));
    let points: Vec<Vec2> = (0..9).map(|i| vec2(if i % 2 == 0 { 44. } else { 84. }, 22. + 16.5 * i as f32)).collect();
    canvas.paint(MUSTARD, &path(&points, 9.));
    pixels(&canvas)
}
/// An sRGB byte in linear light; and linear light, in 4096 steps, as an sRGB byte. (The smaller sizes are averaged in
/// linear light, and working out a power for every pixel of every size would cost a print more than its shapes do.)
static LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let s = i as f32 / 255.;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    })
});
static ENCODE: LazyLock<Vec<u8>> = LazyLock::new(|| {
    (0..4096)
        .map(|i| {
            let l = i as f32 / 4095.;
            let s = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1. / 2.4) - 0.055 };
            (s.clamp(0., 1.) * 255. + 0.5) as u8
        })
        .collect()
});
/// A print's bytes, each smaller size after the last (halved, averaged in linear light) while the size halves evenly.
fn pixels(canvas: &Canvas) -> (Vec<u8>, u32) {
    let (linear, encode) = (&*LINEAR, &*ENCODE);
    let bytes: Vec<u8> = canvas.px.as_flattened().iter().map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8).collect();
    let mut data = Vec::with_capacity(canvas.px.len() * 4 * 4 / 3 + 64);
    data.resize(canvas.px.len() * 4, 255);
    for (out, px) in data.chunks_exact_mut(4).zip(bytes.chunks_exact(3)) {
        (out[0], out[1], out[2]) = (px[0], px[1], px[2]);
    }
    let mut level: Vec<f32> = bytes.iter().map(|b| linear[*b as usize]).collect();
    let [mut w, mut h] = PRINT;
    let mut levels = 1;
    while w % 2 == 0 && h % 2 == 0 && levels < 5 {
        let (half_w, half_h) = (w / 2, h / 2);
        let mut next = vec![0.; half_w * half_h * 3];
        for (y, row) in next.chunks_exact_mut(half_w * 3).enumerate() {
            let (top, under) = (&level[2 * y * w * 3..(2 * y + 1) * w * 3], &level[(2 * y + 1) * w * 3..(2 * y + 2) * w * 3]);
            for (x, px) in row.chunks_exact_mut(3).enumerate() {
                for c in 0..3 {
                    let at = x * 6 + c;
                    px[c] = (top[at] + top[at + 3] + under[at] + under[at + 3]) / 4.;
                }
            }
        }
        let start = data.len();
        data.resize(start + half_w * half_h * 4, 255);
        for (out, px) in data[start..].chunks_exact_mut(4).zip(next.chunks_exact(3)) {
            for c in 0..3 {
                out[c] = encode[(px[c].clamp(0., 1.) * 4095. + 0.5) as usize];
            }
        }
        (level, w, h, levels) = (next, half_w, half_h, levels + 1);
    }
    (data, levels)
}
/// A print as a texture, sharp at a slant (the cards are seen across the table).
pub(super) fn image((data, levels): (Vec<u8>, u32)) -> Image {
    let mut image = Image::new_uninit(
        Extent3d { width: PRINT[0] as u32, height: PRINT[1] as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

// ----- The pictures, each about the card's middle (64, 88), clear of the corners' figures.

/// The colours the pictures are painted in.
mod paint {
    pub const SHINE: [f32; 3] = [1.0, 0.99, 0.94];
    pub const BUN: [f32; 3] = [0.94, 0.66, 0.32];
    pub const CRUST: [f32; 3] = [0.80, 0.46, 0.18];
    pub const CRUMB: [f32; 3] = [1.0, 0.86, 0.58];
    pub const SAUSAGE: [f32; 3] = [0.80, 0.27, 0.16];
    pub const SAUSAGE_SHINE: [f32; 3] = [0.96, 0.55, 0.42];
    pub const MUSTARD_DARK: [f32; 3] = [0.86, 0.58, 0.04];
    pub const KETCHUP: [f32; 3] = [0.86, 0.11, 0.09];
    pub const KETCHUP_DARK: [f32; 3] = [0.62, 0.05, 0.05];
    pub const GLASS: [f32; 3] = [0.86, 0.94, 0.95];
    pub const SALT: [f32; 3] = [0.97, 0.97, 0.93];
    pub const CELERY_SEED: [f32; 3] = [0.42, 0.50, 0.24];
    pub const STEEL: [f32; 3] = [0.66, 0.69, 0.74];
    pub const STEEL_LIGHT: [f32; 3] = [0.84, 0.86, 0.90];
    pub const STEEL_DARK: [f32; 3] = [0.32, 0.34, 0.38];
    pub const PEPPER: [f32; 3] = [0.78, 0.86, 0.22];
    pub const PEPPER_SHINE: [f32; 3] = [0.95, 0.98, 0.62];
    pub const STEM: [f32; 3] = [0.30, 0.52, 0.16];
    pub const JALAPENO: [f32; 3] = [0.14, 0.52, 0.18];
    pub const JALAPENO_SHINE: [f32; 3] = [0.52, 0.82, 0.44];
    pub const CALYX: [f32; 3] = [0.24, 0.40, 0.12];
    pub const MEAT: [f32; 3] = [0.78, 0.25, 0.20];
    pub const MEAT_DARK: [f32; 3] = [0.58, 0.14, 0.12];
    pub const FAT: [f32; 3] = [0.99, 0.86, 0.76];
    pub const CHEESE: [f32; 3] = [1.0, 0.80, 0.22];
    pub const CHEESE_TOP: [f32; 3] = [1.0, 0.91, 0.52];
    pub const HOLE: [f32; 3] = [0.88, 0.58, 0.10];
    pub const BOWL: [f32; 3] = [0.97, 0.95, 0.88];
    pub const BOWL_BAND: [f32; 3] = [0.24, 0.50, 0.80];
    pub const CHILI: [f32; 3] = [0.66, 0.20, 0.10];
    pub const BEAN: [f32; 3] = [0.40, 0.09, 0.07];
    pub const PLATE: [f32; 3] = [0.93, 0.95, 0.98];
    pub const PLATE_SHADE: [f32; 3] = [0.82, 0.86, 0.92];
    pub const KRAUT: [f32; 3] = [0.96, 0.91, 0.58];
    pub const KRAUT_DARK: [f32; 3] = [0.80, 0.70, 0.30];
    pub const KRAUT_LIGHT: [f32; 3] = [1.0, 0.98, 0.84];
    pub const PICKLE_SKIN: [f32; 3] = [0.24, 0.45, 0.12];
    pub const PICKLE_BUMP: [f32; 3] = [0.38, 0.62, 0.20];
    pub const PICKLE_FLESH: [f32; 3] = [0.76, 0.86, 0.40];
    pub const SEED: [f32; 3] = [0.96, 0.97, 0.76];
    pub const ONION: [f32; 3] = [0.62, 0.24, 0.56];
    pub const ONION_LAYER: [f32; 3] = [0.84, 0.56, 0.80];
    pub const ROOT: [f32; 3] = [0.88, 0.80, 0.62];
    pub const LEAF: [f32; 3] = [0.40, 0.68, 0.22];
    pub const RELISH: [f32; 3] = [0.36, 0.70, 0.18];
    pub const RELISH_DARK: [f32; 3] = [0.14, 0.40, 0.10];
    pub const RELISH_LIGHT: [f32; 3] = [0.72, 0.92, 0.40];
    pub const PIMENTO: [f32; 3] = [0.90, 0.24, 0.14];
    pub const LABEL: [f32; 3] = [0.98, 0.96, 0.88];
    pub const CAP: [f32; 3] = [0.97, 0.96, 0.93];
    pub const FRY: [f32; 3] = [0.99, 0.80, 0.30];
    pub const FRY_TIP: [f32; 3] = [0.90, 0.60, 0.16];
    pub const CARTON: [f32; 3] = [0.88, 0.15, 0.13];
    pub const CHIP: [f32; 3] = [1.0, 0.90, 0.56];
    pub const CHIP_DARK: [f32; 3] = [0.93, 0.72, 0.30];
    pub const CHIP_BROWN: [f32; 3] = [0.76, 0.46, 0.14];
    pub const CUP: [f32; 3] = [0.98, 0.98, 0.99];
    pub const LID: [f32; 3] = [0.88, 0.90, 0.94];
    pub const BATTER: [f32; 3] = [0.93, 0.66, 0.27];
    pub const BATTER_DARK: [f32; 3] = [0.68, 0.38, 0.12];
    pub const BATTER_LIGHT: [f32; 3] = [1.0, 0.84, 0.50];
    pub const ONION_FLESH: [f32; 3] = [0.99, 0.96, 0.84];
    pub const FRIDGE: [f32; 3] = [0.97, 0.98, 1.0];
    pub const FRIDGE_SHADE: [f32; 3] = [0.80, 0.88, 0.95];
    pub const FROST: [f32; 3] = [0.30, 0.62, 0.92];
    pub const CLOTH: [f32; 3] = [0.99, 0.99, 0.98];
    pub const CHECK: [f32; 3] = [0.62, 0.72, 0.94];
    pub const CHECK_DARK: [f32; 3] = [0.36, 0.48, 0.84];
    pub const FLAP: [f32; 3] = [0.86, 0.89, 0.95];
}
use paint::*;

/// Paints `picture` on a card.
fn picture(canvas: &mut Canvas, picture: Picture) {
    match picture {
        Picture::Bun => bun(canvas),
        Picture::Shaker => shaker(canvas),
        Picture::SportPeppers => sport_peppers(canvas),
        Picture::Jalapeno => jalapeno(canvas),
        Picture::Bacon => bacon(canvas),
        Picture::Cheese => cheese(canvas),
        Picture::Chili => chili(canvas),
        Picture::Sauerkraut => sauerkraut(canvas),
        Picture::Pickles => pickles(canvas),
        Picture::Onion => onion(canvas),
        Picture::Relish => relish(canvas),
        Picture::Mustard => mustard(canvas),
        Picture::Ketchup => ketchup(canvas),
        Picture::Fries => fries(canvas),
        Picture::Chips => chips(canvas),
        Picture::Spear => spear(canvas),
        Picture::Soda => soda(canvas),
        Picture::Rings => rings(canvas),
        Picture::FootLong => foot_long(canvas),
        Picture::Fridge => fridge(canvas),
        Picture::Pile => pile(canvas),
        Picture::Napkin => napkin(canvas),
    }
}

/// Plain Bun: an empty hotdog bun lying aslant, its top half over its bottom and the pale crumb between them.
fn bun(canvas: &mut Canvas) {
    let (a, b) = (vec2(26., 112.), vec2(102., 68.));
    let (along, down) = axes(a, b);
    canvas.inked(CRUST, &stroke(a + down * 8., b + down * 8., 30.));
    canvas.paint(CRUMB, &stroke(a + down * 1., b + down * 1., 14.));
    canvas.inked(BUN, &stroke(a - down * 6., b - down * 6., 28.));
    canvas.paint(CRUMB, &stroke(a - down * 13. + along * 12., a - down * 13. + along * 44., 6.));
}

/// Celery Salt: a glass shaker leaning over to shake, salt flecked with celery seed in it and a few grains falling.
fn shaker(canvas: &mut Canvas) {
    let pivot = vec2(64., 96.);
    let lean = |shape: Shape| shape.turn(pivot, 0.4);
    canvas.inked(GLASS, &lean(rounded(vec2(60., 102.), vec2(21., 26.), 9.)));
    let salt = lean(rounded(vec2(60., 110.), vec2(16., 16.), 6.));
    canvas.paint(SALT, &salt);
    let mut scatter = Scatter(7);
    for _ in 0..12 {
        let p = scatter.inside(&salt, 2.);
        canvas.paint(CELERY_SEED, &disc(p, 1.7));
    }
    canvas.paint(SHINE, &lean(stroke(vec2(46., 86.), vec2(46., 116.), 4.)));
    canvas.inked(STEEL_LIGHT, &lean(oval(vec2(60., 68.), vec2(19., 11.))));
    canvas.inked(STEEL, &lean(rounded(vec2(60., 75.), vec2(23., 7.), 3.)));
    for x in [52., 60., 68.] {
        canvas.paint(INK, &lean(disc(vec2(x, 64.), 2.3)));
    }
    for p in [vec2(98., 56.), vec2(108., 66.), vec2(101., 76.), vec2(111., 50.), vec2(106., 86.)] {
        canvas.outlined(SALT, &disc(p, 2.8), 1.5);
    }
}

/// A pepper's green cap at `top`, across the pepper going the way `way` and `half` as wide either side, three leaves of
/// it reaching down over the pepper; and its stem out of the top of it, bent `bend` to its right.
fn cap(canvas: &mut Canvas, top: Vec2, way: Vec2, half: f32, bend: f32, colour: [f32; 3]) {
    let across = way.perp();
    canvas.outlined(STEM, &path(&[top - way * 2., top - way * (half * 0.9) + across * bend * 0.3, top - way * (half * 1.3) + across * bend], half * 0.45), 2.);
    let mut leaves = oval(Vec2::ZERO, vec2(half + 2., half * 0.45)).turn(Vec2::ZERO, across.to_angle());
    for k in [-0.6, 0., 0.6] {
        leaves = leaves.or(&disc(across * half * k + way * half * 0.45, half * 0.35));
    }
    let leaves = Shape::new(leaves.min + top, leaves.max + top, move |p| leaves.at(p - top));
    canvas.outlined(colour, &leaves, 2.);
}

/// Sport Peppers: three small yellow-green peppers side by side, each with its own stem, their tips curling.
fn sport_peppers(canvas: &mut Canvas) {
    for (top, lean, curl) in [(vec2(38., 62.), 0.3_f32, 1.), (vec2(64., 58.), 0.05, -1.), (vec2(90., 62.), -0.25, 1.)] {
        let way = Vec2::from_angle(PI / 2. - lean);
        let bend = top + way * 30. + way.perp() * 3. * curl;
        let tip = top + way * 54. + way.perp() * 9. * curl;
        canvas.inked(PEPPER, &taper(top, bend, 9.5, 7.).or(&taper(bend, tip, 7., 2.)));
        canvas.paint(PEPPER_SHINE, &path(&[top + way * 9. + way.perp() * 4. * curl, bend + way.perp() * 3.5 * curl], 2.6));
        cap(canvas, top, way, 8., 5. * curl, STEM);
    }
}

/// Jalapeños: one fat green jalapeño, curved, with its cap and stem on.
fn jalapeno(canvas: &mut Canvas) {
    let (top, bend, tip) = (vec2(48., 64.), vec2(62., 98.), vec2(90., 126.));
    canvas.inked(JALAPENO, &taper(top, bend, 17., 15.).or(&taper(bend, tip, 15., 4.)));
    canvas.paint(JALAPENO_SHINE, &path(&[vec2(38., 76.), vec2(44., 96.), vec2(54., 110.)], 4.5));
    cap(canvas, top - (bend - top).normalize() * 4., (bend - top).normalize(), 14., -8., CALYX);
}

/// Bacon: a wavy rasher cut square at its ends, streaked with fat along it.
fn bacon(canvas: &mut Canvas) {
    let (a, b) = (vec2(18., 120.), vec2(110., 58.));
    canvas.inked(MEAT, &wave(a, b, 8., 3., 0., 30.));
    canvas.paint(FAT, &wave(a, b, 8., 3., -7., 5.));
    canvas.paint(FAT, &wave(a, b, 8., 3., 7., 4.));
    canvas.paint(MEAT_DARK, &wave(a, b, 8., 3., 1., 2.));
}

/// Cheese: a wedge, its cut face full of holes.
fn cheese(canvas: &mut Canvas) {
    let (tip, foot, top) = (vec2(22., 122.), vec2(112., 122.), vec2(112., 64.));
    let back = vec2(-8., -14.);
    let face = polygon(&[tip, foot, top]);
    let rind = polygon(&[tip, top, top + back, tip + back]);
    canvas.inked(CHEESE_TOP, &rind);
    canvas.inked(CHEESE, &face);
    let inner = face.grow(-1.);
    for (c, r) in [(vec2(86., 106.), 9.), (vec2(104., 86.), 6.), (vec2(60., 114.), 6.5), (vec2(101., 113.), 4.5), (vec2(40., 123.), 5.)] {
        canvas.paint(HOLE, &disc(c, r).and(&inner));
    }
    let slope = (top - tip).to_angle();
    for (c, r) in [(vec2(56., 94.), vec2(6., 3.)), (vec2(86., 76.), vec2(5., 2.6))] {
        canvas.paint(HOLE, &oval(c, r).turn(c, slope).and(&rind.grow(-1.)));
    }
}

/// Chili: a bowl of it heaped up and steaming, beans in it.
fn chili(canvas: &mut Canvas) {
    let c = vec2(64., 92.);
    for x in [54., 74.] {
        canvas.outlined(SHINE, &path(&[vec2(x, 70.), vec2(x - 4., 61.), vec2(x + 2., 52.), vec2(x - 2., 43.)], 4.), 1.5);
    }
    canvas.inked(BOWL, &rounded(vec2(64., 128.), vec2(18., 4.), 3.));
    let bowl = oval(c, vec2(50., 36.)).and(&everywhere(move |p| p.y - c.y));
    canvas.inked(BOWL, &bowl);
    canvas.paint(BOWL_BAND, &rounded(vec2(64., 109.), vec2(60., 4.), 0.).and(&bowl.grow(-1.)));
    let chili = oval(c, vec2(50., 12.)).or(&oval(c + vec2(0., -1.), vec2(40., 18.)).and(&everywhere(move |p| c.y - p.y)));
    canvas.inked(CHILI, &chili);
    for (p, angle) in [(vec2(48., 86.), 0.3), (vec2(64., 79.), -0.4), (vec2(80., 86.), 0.9), (vec2(58., 94.), 1.4), (vec2(75., 95.), -0.2), (vec2(36., 93.), 0.), (vec2(92., 93.), 0.5)] {
        canvas.outlined(BEAN, &oval(p, vec2(5., 3.3)).turn(p, angle), 1.);
        canvas.paint(SAUSAGE_SHINE, &disc(p + vec2(-1.5, -1.), 1.));
    }
}

/// Sauerkraut: a pale heap of shredded cabbage on a plate.
fn sauerkraut(canvas: &mut Canvas) {
    canvas.inked(PLATE, &oval(vec2(64., 112.), vec2(52., 13.)));
    canvas.paint(PLATE_SHADE, &oval(vec2(64., 113.), vec2(38., 8.)));
    let heap = oval(vec2(64., 102.), vec2(42., 17.)).or(&disc(vec2(56., 90.), 18.)).or(&disc(vec2(72., 80.), 14.));
    canvas.inked(KRAUT, &heap);
    let mut scatter = Scatter(11);
    for k in 0..44 {
        let p = scatter.inside(&heap, 5.);
        let way = Vec2::from_angle(scatter.next() * PI) * 6.5;
        canvas.paint(if k % 2 == 0 { KRAUT_DARK } else { KRAUT_LIGHT }, &path(&[p - way, p + way.perp() * 0.35, p + way], 2.2));
    }
    // (The shreds near the edge reach a little over it: the outline is drawn again over their ends.)
    canvas.paint(INK, &heap.grow(LINE).less(&heap));
}

/// Pickles: three crinkle-cut slices, each with its ring of seeds.
fn pickles(canvas: &mut Canvas) {
    for (c, r) in [(vec2(84., 80.), 23.), (vec2(44., 74.), 23.), (vec2(62., 109.), 23.)] {
        canvas.inked(PICKLE_SKIN, &crinkle(c, r, 1.8, 14.));
        canvas.paint(PICKLE_FLESH, &disc(c, r - 6.));
        for k in 0..6 {
            let angle = k as f32 * PI / 3. + 0.3;
            let at = c + Vec2::from_angle(angle) * r * 0.45;
            canvas.paint(SEED, &oval(at, vec2(3.4, 1.9)).turn(at, angle));
        }
    }
}

/// Onions: a red onion, its layers showing through its skin, sprouting on top and with roots below.
fn onion(canvas: &mut Canvas) {
    for end in [vec2(54., 138.), vec2(64., 141.), vec2(74., 138.)] {
        canvas.outlined(ROOT, &stroke(vec2(64., 128.), end, 3.), 1.5);
    }
    canvas.inked(LEAF, &taper(vec2(62., 56.), vec2(50., 30.), 4.5, 2.));
    canvas.inked(LEAF, &taper(vec2(66., 56.), vec2(80., 34.), 4.5, 2.));
    let bulb = taper(vec2(64., 100.), vec2(64., 52.), 31., 3.);
    canvas.inked(ONION, &bulb);
    let inner = bulb.grow(-4.);
    for side in [-1., 1.] {
        for spread in [11., 21.] {
            let layer = path(&[vec2(64. + side * 2., 58.), vec2(64. + side * spread * 0.6, 72.), vec2(64. + side * spread, 98.), vec2(64. + side * spread * 0.75, 124.)], 2.6);
            canvas.paint(ONION_LAYER, &layer.and(&inner));
        }
    }
    canvas.paint(SHINE, &path(&[vec2(44., 86.), vec2(42., 98.), vec2(46., 108.)], 4.));
}

/// Relish: a heaped spoonful of it, chunky with bits of pickle and pepper.
fn relish(canvas: &mut Canvas) {
    canvas.inked(STEEL, &stroke(vec2(24., 128.), vec2(50., 108.), 9.));
    canvas.inked(STEEL_LIGHT, &oval(vec2(70., 104.), vec2(38., 15.)).turn(vec2(70., 104.), -0.2));
    let mut heap = oval(vec2(70., 94.), vec2(32., 14.)).turn(vec2(70., 94.), -0.2);
    for (c, r) in [(vec2(52., 88.), 10.), (vec2(64., 80.), 12.), (vec2(80., 78.), 11.), (vec2(92., 86.), 9.), (vec2(72., 68.), 9.)] {
        heap = heap.or(&disc(c, r));
    }
    canvas.inked(RELISH, &heap);
    let mut scatter = Scatter(5);
    for k in 0..34 {
        let p = scatter.inside(&heap, 2.5);
        let bit = rounded(p, Vec2::splat(2.), 0.6).turn(p, scatter.next() * PI);
        canvas.paint([RELISH_DARK, RELISH_LIGHT, PIMENTO][k % 3], &bit);
    }
    canvas.paint(SHINE, &path(&[vec2(58., 74.), vec2(64., 70.)], 3.));
}

/// A squeeze bottle leaning `angle` radians: its body `body` colour, a label on it, its shoulders and the nozzle over
/// them; the pictures of mustard and of ketchup are drawn round it.
fn bottle(canvas: &mut Canvas, angle: f32, body: Shape, shoulder: [f32; 3], nozzle: [f32; 3], colour: [f32; 3]) -> impl Fn(Shape) -> Shape + use<> {
    let pivot = vec2(64., 96.);
    let lean = move |shape: Shape| shape.turn(pivot, angle);
    canvas.inked(nozzle, &lean(taper(vec2(64., 66.), vec2(64., 42.), 9., 2.5)));
    canvas.inked(shoulder, &lean(rounded(vec2(64., 69.), vec2(15., 6.), 3.)));
    canvas.inked(colour, &lean(body));
    canvas.paint(SHINE, &lean(stroke(vec2(49., 86.), vec2(49., 112.), 4.)));
    lean
}

/// Mustard: a yellow squeeze bottle with a pointed nozzle, a curl of mustard out of it and a squiggle on its label.
fn mustard(canvas: &mut Canvas) {
    let lean = bottle(canvas, 0.24, rounded(vec2(64., 98.), vec2(21., 27.), 10.), MUSTARD_DARK, MUSTARD, MUSTARD);
    canvas.outlined(LABEL, &lean(rounded(vec2(65., 100.), vec2(14., 12.), 4.)), 2.);
    let squiggle: Vec<Vec2> = (0..=12).map(|i| vec2(55. + 20. * i as f32 / 12., 100. + 3.5 * (i as f32 * PI / 3.).sin())).collect();
    canvas.paint(MUSTARD_DARK, &lean(path(&squiggle, 3.4)));
    canvas.outlined(MUSTARD, &lean(path(&[vec2(64., 40.), vec2(68., 30.), vec2(78., 26.)], 5.)), 2.);
}

/// Ketchup: a round red squeeze bottle with a white cap and a tomato on its label.
fn ketchup(canvas: &mut Canvas) {
    let lean = bottle(canvas, 0.3, rounded(vec2(64., 98.), vec2(24., 27.), 16.), CAP, CAP, KETCHUP);
    canvas.outlined(LABEL, &lean(disc(vec2(65., 100.), 12.)), 2.);
    canvas.outlined(KETCHUP, &lean(disc(vec2(65., 102.), 7.)), 1.5);
    for angle in [-PI / 2., -PI / 2. + 1.2, -PI / 2. - 1.2] {
        let way = Vec2::from_angle(angle);
        canvas.paint(LEAF, &lean(taper(vec2(65., 95.), vec2(65., 95.) + way * 6., 2., 0.8)));
    }
    canvas.paint(KETCHUP_DARK, &lean(path(&[vec2(64., 40.), vec2(62., 34.)], 4.)));
}

/// Fries: a red carton of them, standing up out of it every which way.
fn fries(canvas: &mut Canvas) {
    for (x, top, lean) in [(40., 64., -0.22), (50., 54., -0.12), (61., 50., -0.03), (72., 52., 0.07), (82., 58., 0.16), (90., 66., 0.25), (56., 64., 0.02), (77., 66., 0.1)] {
        let foot = vec2(x, top + 50.);
        let fry = rounded(vec2(x, top + 25.), vec2(5., 25.), 2.5).turn(foot, lean);
        canvas.inked(FRY, &fry);
        canvas.paint(FRY_TIP, &rounded(vec2(x, top + 2.), vec2(4., 3.), 2.).turn(foot, lean));
    }
    let carton = polygon(&[vec2(28., 90.), vec2(46., 95.), vec2(64., 97.), vec2(82., 95.), vec2(100., 90.), vec2(90., 126.), vec2(38., 126.)]);
    canvas.inked(CARTON, &carton);
    for (a, b) in [(vec2(52., 96.), vec2(54., 128.)), (vec2(76., 96.), vec2(74., 128.))] {
        canvas.paint(CUP, &stroke(a, b, 6.).and(&carton.grow(-1.)));
    }
}

/// Chips: four thin ridged chips in a little pile, pale gold and browned in spots.
fn chips(canvas: &mut Canvas) {
    let mut scatter = Scatter(3);
    for (c, angle) in [(vec2(46., 76.), 0.5), (vec2(84., 70.), -0.6), (vec2(90., 106.), 0.3), (vec2(54., 106.), -0.25)] {
        canvas.inked(CHIP, &oval(c, vec2(21., 16.)).turn(c, angle));
        let (way, across) = (Vec2::from_angle(angle), Vec2::from_angle(angle).perp());
        for k in [-1.5_f32, -0.5, 0.5, 1.5] {
            let reach = 15. * (1. - (k * 7. / 16.).powi(2)).sqrt();
            let ridge: Vec<Vec2> = (0..=6).map(|i| c + across * (k * 7. + 1.2 * (i as f32 * 1.6).sin()) + way * reach * (i as f32 / 3. - 1.)).collect();
            canvas.paint(CHIP_DARK, &path(&ridge, 1.8));
        }
        for _ in 0..2 {
            let p = scatter.inside(&oval(c, vec2(21., 16.)).turn(c, angle), 5.);
            canvas.paint(CHIP_BROWN, &disc(p, 2.));
        }
    }
}

/// Pickle Spear: a long spear of pickle, its cut face pale with a row of seeds, its skin dark and bumpy.
fn spear(canvas: &mut Canvas) {
    let (a, b) = (vec2(26., 122.), vec2(104., 56.));
    let (along, across) = axes(a, b);
    let spear = stroke(a, b, 30.);
    canvas.inked(PICKLE_SKIN, &spear);
    for k in 0..7 {
        canvas.paint(PICKLE_BUMP, &disc(a + (b - a) * (k as f32 + 0.5) / 7. + across * 10., 2.2));
    }
    let flesh = spear.grow(-4.).and(&side(a + across * 4., b + across * 4.));
    canvas.paint(PICKLE_FLESH, &flesh);
    for k in 1..7 {
        let at = a + (b - a) * k as f32 / 7. - across * 3.;
        canvas.paint(SEED, &oval(at, vec2(4., 2.2)).turn(at, along.to_angle()));
    }
}

/// Soda: a paper cup with a lid and a striped straw.
fn soda(canvas: &mut Canvas) {
    let straw = path(&[vec2(70., 66.), vec2(76., 40.), vec2(92., 30.)], 8.);
    canvas.inked(CUP, &straw);
    for k in 0..4 {
        let at = vec2(72., 56.) + vec2(1.8, -7.5) * k as f32;
        canvas.paint(KETCHUP, &stroke(at - vec2(6., 1.), at + vec2(6., 1.), 3.).and(&straw));
    }
    canvas.paint(KETCHUP, &stroke(vec2(84., 30.), vec2(86., 40.), 3.).and(&straw));
    canvas.inked(LID, &oval(vec2(64., 62.), vec2(24., 8.)));
    canvas.inked(LID, &rounded(vec2(64., 68.), vec2(32., 5.), 3.));
    let cup = polygon(&[vec2(36., 72.), vec2(92., 72.), vec2(84., 126.), vec2(44., 126.)]);
    canvas.inked(CUP, &cup);
    canvas.paint(KETCHUP, &polygon(&[vec2(30., 90.), vec2(100., 86.), vec2(100., 104.), vec2(30., 108.)]).and(&cup.grow(-0.5)));
}

/// Onion Rings: three battered rings, craggy with crumbs, the onion pale inside the batter.
fn rings(canvas: &mut Canvas) {
    let mut scatter = Scatter(9);
    for (c, outer) in [(vec2(84., 72.), 22.), (vec2(42., 80.), 22.), (vec2(66., 104.), 23.)] {
        let inner = outer - 9.;
        let ring = crinkle(c, outer, 1., 22.).less(&crinkle(c, inner, 0.8, 16.));
        canvas.inked(BATTER, &ring);
        canvas.paint(ONION_FLESH, &disc(c, inner + 2.).less(&disc(c, inner - 1.)).and(&ring));
        for k in 0..18 {
            let p = scatter.inside(&ring, 2.5);
            canvas.paint(if k % 2 == 0 { BATTER_DARK } else { BATTER_LIGHT }, &disc(p, 1.4));
        }
    }
}

/// Foot-Long: a long hotdog in its bun, the sausage out at both ends, a zigzag of mustard along it.
fn foot_long(canvas: &mut Canvas) {
    let (a, b) = (vec2(26., 120.), vec2(104., 58.));
    let (along, across) = axes(a, b);
    canvas.inked(BUN, &stroke(a - across * 6., b - across * 6., 26.));
    canvas.inked(SAUSAGE, &stroke(a - along * 10., b + along * 10., 22.));
    canvas.inked(CRUST, &stroke(a + across * 12., b + across * 12., 22.));
    let zigzag: Vec<Vec2> = (0..=14).map(|i| a + (b - a) * (0.04 + 0.92 * i as f32 / 14.) - across * if i % 2 == 0 { 8. } else { 1. }).collect();
    canvas.outlined(MUSTARD, &path(&zigzag, 3.5), 1.);
    canvas.paint(SAUSAGE_SHINE, &stroke(b + along * 3. - across * 5., b + along * 8. - across * 4., 3.));
}

/// Back in the Fridge: a fridge, frost on its freezer door and a hotdog stuck on the door below.
fn fridge(canvas: &mut Canvas) {
    for x in [44., 84.] {
        canvas.inked(STEEL_DARK, &rounded(vec2(x, 148.), vec2(6., 5.), 2.));
    }
    let body = rounded(vec2(64., 88.), vec2(36., 58.), 12.);
    canvas.inked(FRIDGE, &body);
    canvas.paint(FRIDGE_SHADE, &rounded(vec2(94., 88.), vec2(8., 60.), 0.).and(&body));
    canvas.paint(INK, &stroke(vec2(24., 68.), vec2(104., 68.), LINE).and(&body));
    canvas.inked(STEEL, &rounded(vec2(40., 52.), vec2(3., 9.), 3.));
    canvas.inked(STEEL, &rounded(vec2(40., 94.), vec2(3., 18.), 3.));
    let flake = vec2(72., 49.);
    for k in 0..3 {
        let way = Vec2::from_angle(k as f32 * PI / 3. + PI / 2.) * 11.;
        canvas.paint(FROST, &stroke(flake - way, flake + way, 3.5));
    }
    canvas.paint(FROST, &disc(flake, 3.5));
    let (a, b) = (vec2(60., 114.), vec2(86., 100.));
    let (along, across) = axes(a, b);
    canvas.inked(BUN, &stroke(a - across * 2., b - across * 2., 12.));
    canvas.inked(SAUSAGE, &stroke(a - along * 5., b + along * 5., 8.));
    canvas.inked(CRUST, &stroke(a + across * 4., b + across * 4., 9.));
}

/// Pile It On: toppings piled high on a bun, leaning as a pile does.
fn pile(canvas: &mut Canvas) {
    canvas.inked(CRUST, &rounded(vec2(64., 144.), vec2(36., 9.), 9.));
    canvas.inked(SAUSAGE, &stroke(vec2(30., 133.), vec2(98., 133.), 11.));
    let cheese = rounded(vec2(66., 122.), vec2(40., 4.), 2.).or(&taper(vec2(40., 124.), vec2(40., 134.), 4., 3.)).or(&taper(vec2(86., 124.), vec2(86., 131.), 4., 3.));
    canvas.inked(CHEESE, &cheese);
    for c in [vec2(48., 111.), vec2(80., 110.)] {
        canvas.inked(PICKLE_SKIN, &oval(c, vec2(17., 6.)));
        canvas.paint(PICKLE_FLESH, &oval(c, vec2(12., 3.5)));
    }
    canvas.inked(ONION_LAYER, &oval(vec2(60., 99.), vec2(32., 7.)).edge(5.));
    let wave = |y: f32, from: f32, to: f32, high: f32| -> Vec<Vec2> { (0..=12).map(|i| vec2(from + (to - from) * i as f32 / 12., y + high * (i as f32 * 1.4).sin())).collect() };
    canvas.inked(KETCHUP, &path(&wave(88., 28., 98., 3.), 8.));
    let relish = oval(vec2(66., 76.), vec2(30., 9.)).or(&oval(vec2(54., 72.), vec2(14., 7.)));
    canvas.inked(RELISH, &relish);
    canvas.inked(MEAT, &path(&wave(60., 32., 94., 4.), 10.));
    canvas.paint(FAT, &path(&wave(59., 32., 94., 4.), 3.));
    canvas.inked(PICKLE_SKIN, &crinkle(vec2(62., 44.), 13., 1.2, 12.));
    canvas.paint(PICKLE_FLESH, &disc(vec2(62., 44.), 9.));
    let squiggle: Vec<Vec2> = (0..=16).map(|i| vec2(34. + 58. * i as f32 / 16., 34. + 4. * (i as f32 * PI / 2.5).sin() + 0.25 * i as f32)).collect();
    canvas.outlined(MUSTARD, &path(&squiggle, 5.), 2.);
}

/// Napkin: a cloth napkin, checked, one corner folded over.
fn napkin(canvas: &mut Canvas) {
    let (c, half, fold) = (vec2(64., 90.), 42., 32.);
    let corner = c + vec2(half, -half);
    let cloth = rounded(c, Vec2::splat(half), 4.).and(&side(corner + vec2(0., fold + 1.), corner + vec2(-fold - 1., 0.)));
    let flap = polygon(&[corner + vec2(-fold, 0.), corner + vec2(0., fold), corner + vec2(-fold, fold)]);
    let turn = |shape: Shape| shape.turn(c, -0.12);
    canvas.inked(CLOTH, &turn(cloth.clone()));
    let inner = turn(cloth.grow(-0.5));
    let way = Vec2::from_angle(-0.12);
    let (down, across) = (stripes(c, way, 17., 9.), stripes(c, way.perp(), 17., 9.));
    canvas.paint(CHECK, &down.or(&across).and(&inner));
    canvas.paint(CHECK_DARK, &down.and(&across).and(&inner));
    canvas.inked(FLAP, &turn(flap));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind of card in the deck (it comes in order: the same cards together).
    fn kinds() -> Vec<Card> {
        let mut kinds = kitchen_sim::cards::deck();
        kinds.dedup();
        kinds
    }

    #[test]
    fn every_card_has_a_face() {
        let kinds = kinds();
        assert_eq!(kinds.len(), 13 + 5 + 1 + 3);
        for card in &kinds {
            let face = card_face(*card);
            assert_eq!(face.label, card.name(), "{card:?}");
            match *card {
                Card::Topping(value) => assert_eq!((face.value, face.glyph, face.picture), (Some(value), Glyph::None, TOPPING_PICTURES[value as usize])),
                Card::Side(value) => assert_eq!((face.value, face.glyph), (Some(value), Glyph::Plus)),
                Card::FootLong => assert_eq!((face.value, face.glyph, face.picture), (Some(2), Glyph::Times, Picture::FootLong)),
                Card::Fridge => assert_eq!((face.value, face.glyph, face.picture), (None, Glyph::None, Picture::Fridge)),
                Card::PileOn => assert_eq!((face.value, face.glyph, face.picture), (None, Glyph::None, Picture::Pile)),
                Card::Napkin => assert_eq!((face.value, face.glyph, face.picture), (None, Glyph::None, Picture::Napkin)),
            }
        }
        // The sides by name.
        let named: Vec<(String, Picture)> = kinds.iter().filter(|card| matches!(card, Card::Side(_))).map(|card| (card.name(), card_face(*card).picture)).collect();
        assert_eq!(
            named,
            [("Fries +2", Picture::Fries), ("Chips +4", Picture::Chips), ("Pickle Spear +6", Picture::Spear), ("Soda +8", Picture::Soda), ("Onion Rings +10", Picture::Rings)].map(|(name, picture)| (name.to_string(), picture))
        );
        // No two toppings are printed in the same colour, nor in a side's or an action card's.
        let rgb = |card: Card| card_face(card).colour.to_srgba().to_u8_array_no_alpha();
        let toppings: Vec<[u8; 3]> = (0..13).map(|v| rgb(Card::Topping(v))).collect();
        for (i, a) in toppings.iter().enumerate() {
            assert!(toppings[i + 1..].iter().all(|b| a != b), "topping {i} shares its colour");
            for other in [Card::Side(2), Card::FootLong, Card::Fridge, Card::PileOn, Card::Napkin] {
                assert_ne!(*a, rgb(other), "topping {i} looks like {other:?}");
            }
        }
        // And no two kinds of card are printed alike, nor like the back.
        let prints: Vec<Vec<u8>> = kinds.iter().map(|card| print(&card_face(*card)).0).collect();
        for (i, a) in prints.iter().enumerate() {
            assert_eq!(a.len(), prints[0].len());
            assert!(prints[i + 1..].iter().all(|b| a != b), "{:?} is printed like another", kinds[i]);
            assert!(a != &print_back().0, "{:?} is printed like the back", kinds[i]);
        }
    }

    /// The picture alone, on a plain card: what is left of `PAPER` where it is not.
    fn alone(picture: Picture) -> Canvas {
        let mut canvas = Canvas::new(PAPER);
        super::picture(&mut canvas, picture);
        canvas
    }
    /// How much two canvases differ, over all their pixels: the mean of the largest difference in each pixel's colour.
    fn apart(a: &[[f32; 3]], b: &[[f32; 3]]) -> f32 {
        a.iter().zip(b).map(|(a, b)| (0..3).map(|c| (a[c] - b[c]).abs()).fold(0., f32::max)).sum::<f32>() / a.len() as f32
    }

    #[test]
    fn every_kind_of_card_prints_its_own_picture() {
        let kinds = kinds();
        let pictures: Vec<Picture> = kinds.iter().map(|card| card_face(*card).picture).collect();
        let alone: Vec<Canvas> = pictures.iter().map(|picture| alone(*picture)).collect();
        for (i, a) in alone.iter().enumerate() {
            // A picture, bold: it covers a good part of the window it sits in.
            let covered = a.px.iter().filter(|px| (0..3).any(|c| (px[c] - PAPER[c]).abs() > 0.05)).count();
            assert!(covered as f32 > 0.3 * (WINDOW.x * WINDOW.y * 4.), "{:?} covers only {covered} pixels", kinds[i]);
            // Unlike every other kind's: each pair differs over much of the card.
            for (j, b) in alone.iter().enumerate().skip(i + 1) {
                let differs = apart(&a.px, &b.px);
                assert!(differs > 0.08, "{:?} and {:?} are drawn alike ({differs})", kinds[i], kinds[j]);
            }
            // Clear of the figures in the corners (and every shape painted within its box).
            let face = card_face(kinds[i]);
            if let Some(value) = face.value {
                let (min, max) = index_box(&marks(face.glyph, value));
                let (min, max) = (min - Vec2::splat(2.), max + Vec2::splat(2.));
                for (n, px) in a.px.iter().enumerate() {
                    let p = vec2((n % PRINT[0]) as f32 + 0.5, (n / PRINT[0]) as f32 + 0.5);
                    for p in [p, SIZE - p] {
                        if p.cmpge(min).all() && p.cmple(max).all() {
                            assert!((0..3).all(|c| (px[c] - PAPER[c]).abs() < 0.02), "{:?} is drawn over its corner at {p}", kinds[i]);
                        }
                    }
                }
            }
            let mut whole = Canvas::new(PAPER);
            whole.whole = true;
            super::picture(&mut whole, pictures[i]);
            assert!(apart(&whole.px, &a.px) < 1e-6, "a shape of {:?} is cut short by its box", kinds[i]);
            let printed = printed(&face, false);
            assert!(apart(&printed.px, &super::printed(&face, true).px) < 1e-6, "a shape of {:?}'s print is cut short by its box", kinds[i]);
        }
    }

    #[test]
    fn the_kinds_of_card_tell_apart_across_the_table() {
        // A card on the far side of the table is a couple of dozen pixels high: as the print's fourth size (16 by 22),
        // every kind still looks unlike every other.
        let kinds = kinds();
        let small: Vec<Vec<[f32; 3]>> = kinds
            .iter()
            .map(|card| {
                let (data, levels) = print(&card_face(*card));
                assert_eq!(levels, 5);
                let skip: usize = (0..3).map(|level| (PRINT[0] >> level) * (PRINT[1] >> level) * 4).sum();
                data[skip..skip + 16 * 22 * 4].chunks(4).map(|px| [px[0], px[1], px[2]].map(|b| b as f32 / 255.)).collect()
            })
            .collect();
        for (i, a) in small.iter().enumerate() {
            for (j, b) in small.iter().enumerate().skip(i + 1) {
                let differs = apart(a, b);
                assert!(differs > 0.05, "{:?} and {:?} look alike from afar ({differs})", kinds[i], kinds[j]);
            }
        }
    }
}

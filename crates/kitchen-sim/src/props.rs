// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Rigid-body physics for movable props: the bowl, crates, pillows, chairs and whatever a fort is built from.
//! Single player, the client's prediction and the server (crates/server) all run it. It was written operation for
//! operation as the JavaScript physics the Node server once ran, and tools/network/fixtures/props-golden.json keeps
//! that physics' record of scenarios, which this must still replay; the map editor draws the shapes from
//! tools/network/presets.mjs, held to the same parts by tools/network/fixtures/presets-golden.json.
//!
//! Method
//!   Every shape is a set of rounded solids (boxes, and lathed shells such as the bowl) plus a cloud of small
//!   sample spheres on its surface. All collision is "sample sphere against solid": against the level's ground
//!   height, against the level's blocks, against other props, and the same test with bigger spheres for characters.
//!   Contacts are solved with sequential impulses in fixed 1/120 s substeps: warm starting, Coulomb friction,
//!   restitution above a speed threshold, and overlaps pushed apart by a separate position pass (split impulses)
//!   that never touches the real velocities, so resting piles gain no energy. Islands of still props sleep.
//!   Three things keep piles and thin shapes honest: contacts between two things are thinned to a few
//!   well-spread ones per face; a sample that sinks into a solid is pushed back out the way it came in; and
//!   two props that overlap deeply are simply parted centre from centre.
//!
//! Arithmetic
//!   Everything is an f64, and the simulation uses only + - * / and sqrt, which IEEE 754 rounds identically
//!   everywhere. Every expression here performs the same operations in the same order as the recorded
//!   JavaScript physics did, so both produce the same bits. Keep it that way: no mul_add, sin, cos or powf inside the simulation, no reordering
//!   of sums, and no iteration over the contact cache except to delete from it.
//!
//! Use, once per game tick
//! ```text
//! 1. hits since the last tick: apply_impulse(), blast(); carrying: pick_up(), carry(), drop_prop()
//! 2. the game moves its characters
//! 3. events = world.step(&level, dt)                  props move; events are impacts, sleeps, wakes and losses
//! 4. for every character: resolve_character(&c, dt)   the character is put outside the props and pushes them
//! ```
//!   raycast() and segment() answer at any time. Props are numbered from 0 in the order they were added, and
//!   everything is processed in that order, so the same calls always give the same motion.
//!
//! Limits
//!   65536 props and blocks; 4096 sample points and 64 parts per shape. Edges are only tested at their sample
//!   points, so two edges that cross between samples can sink a centimetre or two into each other before a
//!   corner catches. Props faster than 6 m/s can pass through thin things. Ground features narrower than a
//!   centimetre may be missed. A napkin does not drape here (drape.rs drapes it).
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

pub const SUBSTEP: f64 = 1. / 120.;
pub const TICK: f64 = 1. / 60.;
/// The game's gravity, the one its characters jump and fall under (actor.rs); the props share it.
pub const GRAVITY: f64 = 2.8;
/// `other` in an impact event when the prop hit the level's surface; blocks are BLOCK - index.
pub const GROUND: i32 = -1;
pub const BLOCK: i32 = -2;

const MARGIN: f64 = 0.002; // contacts exist this far before surfaces touch...
const MARGIN_MAX: f64 = 0.05; // ...plus the distance the prop can travel in a substep, up to this
const DEEP: f64 = 0.012; // a sample this far under the surface is beside a wall, not on a floor
const TANGLED: f64 = 0.015; // two props overlapping this deeply are simply pushed apart, centre from centre
const PROBE_STEP: f64 = 0.01;
const PROBE_STEPS: usize = 5;
const PROBE_BISECT: usize = 6;
const SLOPE_EPS: f64 = 0.002;
const SLOPE_MAX: f64 = 3.;
const SOLVE_ITERATIONS: usize = 12;
const PUSH_ITERATIONS: usize = 3;
const SLOP: f64 = 0.0002; // overlap that is left alone, so resting contacts stay put
const PUSH_RATE: f64 = 0.2; // share of the remaining overlap removed per substep
const MAX_PUSH: f64 = 1.; // m/s, fastest an overlap is pushed apart
const RESTITUTION_SPEED: f64 = 0.12; // m/s, slower approaches do not bounce
const IMPACT_SPEED: f64 = 0.12; // m/s, slower approaches make no impact event
const TOUCH: f64 = 0.0005; // a gap this small counts as touching: bounces and impact events happen here
const SLEEP_LINEAR: f64 = 0.008;
const SLEEP_ANGULAR: f64 = 0.1;
const SLEEP_TIME: f64 = 0.5;
// A prop that twitches without getting anywhere also sleeps: within 1.5 mm and 1 degree of one pose for a second.
const STUCK_MOVE: f64 = 0.0015;
const STUCK_TURN: f64 = 4e-5;
const STUCK_TIME: f64 = 1.;
const MAX_LINEAR: f64 = 30.;
const MAX_ANGULAR: f64 = 60.;
const CACHE_KEEP: u64 = 2; // substeps a contact's impulses are kept after the contact was last seen
const SUPPORT_SKIN: f64 = 0.006; // a character this close above a prop stands on it
const SUPPORT_SLOPE: f64 = 0.7; // least upward normal a character can stand on
const STEP_UP: f64 = 0.045; // most a prop lifts a character that sank into its top
const CHARACTER_PASSES: usize = 4;
const PUSH: f64 = 2.85; // newtons a character can push with, per kilogram of its own mass
const BLAST_MASS: f64 = 0.35; // kg: a blast throws lighter props no faster than one of this mass
const KNOCK_SPEED: f64 = 0.3; // m/s: a prop coming at a character faster than this knocks it back
const FOOT_STEPS: usize = 64; // steps of the search for the floor under a character
const RAY_STEPS: usize = 64;
const RAY_HIT: f64 = 1e-5;
const GROUPS: usize = 16; // patches of like-facing contacts told apart per pair of things
const SAME_FACING: f64 = 0.95; // least dot product between normals of one patch
const LATHE_SLICES: usize = 8;
const NONE: usize = usize::MAX;

// cos(k * 15 degrees) for k = 0..6; rings of sample points are built from these so no implementation calls cos.
const COS15: [f64; 7] = [1., 0.9659258262890683, 0.8660254037844386, std::f64::consts::FRAC_1_SQRT_2, 0.5, 0.25881904510252074, 0.];
/// Unit vector at k * 15 degrees, k = 0..23, as [cos, sin].
pub fn direction24(k: usize) -> [f64; 2] {
    let (q, i) = (k / 6, k % 6);
    let (c, s) = (COS15[i], COS15[6 - i]);
    match q {
        0 => [c, s],
        1 => [-s, c],
        2 => [-c, -s],
        _ => [s, -c],
    }
}

/// The rotation [x, y, z, w] that turns a prop by `yaw` radians about the vertical, as map files give it.
/// Worked out with sums and products only, so that every implementation gets the same bits: sin and cos of
/// half the angle by their series, after bringing the angle within half a turn.
pub fn yaw_rotation(yaw: f64) -> [f64; 4] {
    let turn = 6.283185307179586;
    let half = (yaw - turn * (yaw / turn + 0.5).floor()) / 2.;
    let (square, mut sin, mut cos) = (half * half, 1., 1.);
    for k in (1..=13).rev() {
        let k = k as f64;
        sin = 1. - sin * square / ((2. * k) * (2. * k + 1.));
        cos = 1. - cos * square / ((2. * k - 1.) * (2. * k));
    }
    [0., half * sin, 0., cos]
}

// ---------------------------------------------------------------------------------------------------------
// Shapes

/// One piece of a shape, in the shape's own frame, in metres and kilograms.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    /// A box with rounded edges; `half` includes the rounding.
    Box { center: [f64; 3], half: [f64; 3], radius: f64, mass: f64 },
    /// A shell swept round the y axis; the profile is [distance from the axis, height] starting on the axis,
    /// and `radius` is half the wall thickness.
    Lathe { center: [f64; 3], profile: Vec<[f64; 2]>, radius: f64, mass: f64 },
}

/// A shape as the game describes it. The origin of its frame is where the game places the prop: the middle
/// of its underside for every ready-made one, so that a map's `base` is the height the prop stands at. `spacing` is the largest gap between sample points on a box.
/// `nap` is how long a prop of the shape lies still before it falls asleep (SLEEP_TIME for most; a pillow, made to
/// be piled up, drops off sooner).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeSpec {
    pub name: &'static str,
    pub friction: f64,
    pub restitution: f64,
    pub linear_damping: f64,
    pub angular_damping: f64,
    pub spacing: f64,
    pub nap: f64,
    pub parts: Vec<Part>,
}
fn part(center: [f64; 3], half: [f64; 3], radius: f64, mass: f64) -> Part {
    Part::Box { center, half, radius, mass }
}
/// A shell turned round the vertical axis, standing on its origin: `profile` from the axis outwards (and back to it
/// for a closed one, such as a tin), `radius` half its wall.
fn turned(profile: &[[f64; 2]], radius: f64, mass: f64) -> Part {
    Part::Lathe { center: [0., radius, 0.], profile: profile.to_vec(), radius, mass }
}
impl ShapeSpec {
    /// The ready-made shape of a loose object, by the kind a map gives it (`map::LOOSE`).
    pub fn named(kind: &str) -> Option<ShapeSpec> {
        Some(match kind {
            "bowl" => ShapeSpec::bowl(),
            "crate" => ShapeSpec::crate_box(0.1, 0.3),
            "pillow" => ShapeSpec::pillow(),
            "napkin" => ShapeSpec::napkin(),
            "chair" => ShapeSpec::chair(),
            "tray" => ShapeSpec::tray(),
            "cereal" => ShapeSpec::cereal(),
            "cup" => ShapeSpec::cup(),
            "plate" => ShapeSpec::plate(),
            "tin" => ShapeSpec::tin(),
            "board" => ShapeSpec::board(),
            "dish" => ShapeSpec::dish(),
            "roll" => ShapeSpec::roll(),
            "salt" => ShapeSpec::shaker("salt"),
            "pepper" => ShapeSpec::shaker("pepper"),
            "breadbox" => ShapeSpec::breadbox(),
            _ => return None,
        })
    }
    /// The big silver bowl: 32 cm across the rim, 13 cm across the base, 11 cm deep.
    pub fn bowl() -> ShapeSpec {
        let profile = vec![[0., 0.], [0.065, 0.], [0.125, 0.045], [0.155, 0.105]];
        ShapeSpec {
            name: "bowl",
            friction: 0.25,
            restitution: 0.3,
            linear_damping: 0.05,
            angular_damping: 0.4,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![Part::Lathe { center: [0., 0.004, 0.], profile, radius: 0.004, mass: 0.35 }],
        }
    }
    /// A cubic crate; the stock one is `crate_box(0.1, 0.3)`.
    pub fn crate_box(size: f64, mass: f64) -> ShapeSpec {
        ShapeSpec {
            name: "crate",
            friction: 0.5,
            restitution: 0.1,
            linear_damping: 0.05,
            angular_damping: 0.1,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![part([0., size / 2., 0.], [size / 2., size / 2., size / 2.], 0.004, mass)],
        }
    }
    /// A pillow, 14 cm by 10 cm and 4 cm thick, 40 g: a flat, nearly square-edged cushion that grips what it lies on
    /// and does not bounce, so that pillows pile up into walls and columns, and lies still sooner than most (`nap`).
    pub fn pillow() -> ShapeSpec {
        ShapeSpec {
            name: "pillow",
            friction: 1.,
            restitution: 0.,
            linear_damping: 0.6,
            angular_damping: 3.,
            spacing: 0.05,
            nap: 0.25,
            parts: vec![part([0., 0.02, 0.], [0.07, 0.02, 0.05], 0.008, 0.04)],
        }
    }
    /// A cloth napkin, or a tea towel: 45 cm square and 90 g, a thin flat pad here (the game draws it as cloth). Over
    /// other props the game drapes it (drape.rs).
    pub fn napkin() -> ShapeSpec {
        ShapeSpec {
            name: "napkin",
            friction: 0.9,
            restitution: 0.,
            linear_damping: 0.3,
            angular_damping: 1.5,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![part([0., 0.004, 0.], [0.225, 0.004, 0.225], 0.003, 0.09)],
        }
    }
    pub fn chair() -> ShapeSpec {
        ShapeSpec {
            name: "chair",
            friction: 0.5,
            restitution: 0.1,
            linear_damping: 0.05,
            angular_damping: 0.1,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![
                part([0., 0.066, 0.], [0.045, 0.006, 0.045], 0.004, 0.1),
                part([0., 0.115, -0.039], [0.045, 0.045, 0.006], 0.004, 0.06),
                part([0.039, 0.031, 0.039], [0.006, 0.031, 0.006], 0.004, 0.0225),
                part([-0.039, 0.031, 0.039], [0.006, 0.031, 0.006], 0.004, 0.0225),
                part([0.039, 0.031, -0.039], [0.006, 0.031, 0.006], 0.004, 0.0225),
                part([-0.039, 0.031, -0.039], [0.006, 0.031, 0.006], 0.004, 0.0225),
            ],
        }
    }
    /// A rectangular baking tray: a base and four low walls, open on top.
    pub fn tray() -> ShapeSpec {
        ShapeSpec {
            name: "tray",
            friction: 0.25,
            restitution: 0.25,
            linear_damping: 0.05,
            angular_damping: 0.3,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![
                part([0., 0.004, 0.], [0.15, 0.004, 0.1], 0.003, 0.3),
                part([0.146, 0.026, 0.], [0.004, 0.022, 0.1], 0.003, 0.05),
                part([-0.146, 0.026, 0.], [0.004, 0.022, 0.1], 0.003, 0.05),
                part([0., 0.026, 0.096], [0.142, 0.022, 0.004], 0.003, 0.05),
                part([0., 0.026, -0.096], [0.142, 0.022, 0.004], 0.003, 0.05),
            ],
        }
    }
    /// A family-size box of breakfast cereal, 23 cm wide, 8 cm deep and 33 cm tall (more than twice a hotdog's
    /// height), 650 g full: a panel to stand up in a wall, which a round goes through (card and flakes).
    pub fn cereal() -> ShapeSpec {
        ShapeSpec {
            name: "cereal",
            friction: 0.6,
            restitution: 0.05,
            linear_damping: 0.1,
            angular_damping: 0.4,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![part([0., 0.165, 0.], [0.115, 0.165, 0.04], 0.003, 0.65)],
        }
    }
    /// A china mug, 10 cm tall and 8.4 cm across, with its handle on its +x side, 350 g: it stands on its foot, mugs
    /// stand on one another, and it stops a round.
    pub fn cup() -> ShapeSpec {
        ShapeSpec {
            name: "cup",
            friction: 0.5,
            restitution: 0.15,
            linear_damping: 0.2,
            angular_damping: 0.6,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![
                turned(&[[0., 0.], [0.04, 0.], [0.04, 0.096], [0., 0.096]], 0.002, 0.32),
                part([0.054, 0.052, 0.], [0.013, 0.03, 0.005], 0.004, 0.03),
            ],
        }
    }
    /// A stoneware dinner plate, 26 cm across and 2 cm deep, 550 g: a flat disc to lay down, pile up or stand on its rim,
    /// which stops a round.
    pub fn plate() -> ShapeSpec {
        ShapeSpec {
            name: "plate",
            friction: 0.4,
            restitution: 0.15,
            linear_damping: 0.1,
            angular_damping: 0.4,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![turned(&[[0., 0.], [0.127, 0.], [0.127, 0.014], [0., 0.014]], 0.003, 0.55)],
        }
    }
    /// A tin of beans, 7.5 cm across and 11 cm tall, 450 g: a heavy little drum that stops a round, and tins stand on
    /// one another.
    pub fn tin() -> ShapeSpec {
        ShapeSpec {
            name: "tin",
            friction: 0.4,
            restitution: 0.15,
            linear_damping: 0.1,
            angular_damping: 0.3,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![turned(&[[0., 0.], [0.0355, 0.], [0.0355, 0.106], [0., 0.106]], 0.002, 0.45)],
        }
    }
    /// A wooden chopping board, 45 cm by 30 cm and 2.5 cm thick, 2.2 kg: the biggest and heaviest piece, a slab to lay
    /// across a gap or stand on its edge as a wall, which stops a round.
    pub fn board() -> ShapeSpec {
        ShapeSpec {
            name: "board",
            friction: 0.6,
            restitution: 0.1,
            linear_damping: 0.05,
            angular_damping: 0.2,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![part([0., 0.0125, 0.], [0.225, 0.0125, 0.15], 0.003, 2.2)],
        }
    }
    /// A china cereal bowl, 15 cm across the rim and 6.5 cm deep, 350 g: open, so things go in it, and bowls nest.
    pub fn dish() -> ShapeSpec {
        ShapeSpec {
            name: "dish",
            friction: 0.35,
            restitution: 0.15,
            linear_damping: 0.1,
            angular_damping: 0.4,
            spacing: 0.04,
            nap: SLEEP_TIME,
            parts: vec![turned(&[[0., 0.], [0.038, 0.], [0.064, 0.028], [0.0715, 0.058]], 0.0035, 0.35)],
        }
    }
    /// A roll of paper towels, 28 cm tall and 12 cm across, 180 g: it stands on end, rolls stand on one another, and a
    /// round goes through it (paper).
    pub fn roll() -> ShapeSpec {
        ShapeSpec {
            name: "roll",
            friction: 0.7,
            restitution: 0.05,
            linear_damping: 0.2,
            angular_damping: 0.6,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![turned(&[[0., 0.], [0.058, 0.], [0.058, 0.276], [0., 0.276]], 0.002, 0.18)],
        }
    }
    /// A glass salt (or pepper) shaker with a steel cap, 9 cm tall and 4.5 cm across, 120 g: small, solid and light
    /// enough to throw far.
    pub fn shaker(name: &'static str) -> ShapeSpec {
        ShapeSpec {
            name,
            friction: 0.4,
            restitution: 0.2,
            linear_damping: 0.1,
            angular_damping: 0.3,
            spacing: 0.03,
            nap: SLEEP_TIME,
            parts: vec![turned(&[[0., 0.], [0.021, 0.], [0.0215, 0.064], [0.0195, 0.07], [0.0185, 0.0876], [0., 0.0876]], 0.0012, 0.12)],
        }
    }
    /// An enamel bread bin with a roll top, 40 cm wide, 25 cm deep and 20 cm tall, 1.8 kg: a solid block to build a wall
    /// round.
    pub fn breadbox() -> ShapeSpec {
        ShapeSpec {
            name: "breadbox",
            friction: 0.55,
            restitution: 0.1,
            linear_damping: 0.05,
            angular_damping: 0.2,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![part([0., 0.1, 0.], [0.2, 0.1, 0.125], 0.012, 1.8)],
        }
    }
}
// ---------------------------------------------------------------------------------------------------------
// Materials

/// What a loose object is made of, as far as a round is concerned (the game's rounds: feast.rs `flight`). A solid
/// one stops any round that reaches it, as a block does. A light one lets a round through, slowed and weakened as it
/// passes (feast.rs LIGHT_SPEED and LIGHT_DAMAGE), and neither shields from a relish glob's splash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Solid,
    Light,
}
/// Each kind of loose object's material, by the kind a map gives it (map.rs LOOSE), one kind to a line. A kind that
/// is not listed is solid. The kitchen's cardboard, paper and cushions are light; its wood, china, glass, metal and
/// folded cloth are solid.
pub const MATERIALS: &[(&str, Material)] = &[
    ("bowl", Material::Solid),
    ("crate", Material::Solid),
    ("pillow", Material::Light),
    // Lying flat or draped (a draped one is a wall of the level, which is solid in any case).
    ("napkin", Material::Solid),
    ("chair", Material::Solid),
    ("tray", Material::Solid),
    ("cereal", Material::Light),
    ("cup", Material::Solid),
    ("plate", Material::Solid),
    ("tin", Material::Solid),
    ("board", Material::Solid),
    ("dish", Material::Solid),
    ("roll", Material::Light),
    ("salt", Material::Solid),
    ("pepper", Material::Solid),
    ("breadbox", Material::Solid),
];
/// The material of a kind of loose object (MATERIALS): solid unless it is listed as light.
pub fn material(kind: &str) -> Material {
    MATERIALS.iter().find(|(name, _)| *name == kind).map_or(Material::Solid, |(_, m)| *m)
}

/// One solid of a shape, about the shape's centre of mass. A box is its core (the box shrunk by its rounding)
/// and the rounding `r`; a lathe is its profile and half its wall thickness `r`.
#[derive(Clone, Debug)]
struct Prim {
    lathe: bool,
    cx: f64,
    cy: f64,
    cz: f64,
    hx: f64,
    hy: f64,
    hz: f64,
    r: f64,
    profile: Vec<f64>,
}

/// A shape the world can simulate: mass, inertia, solids and sample points, all about the centre of mass.
#[derive(Clone, Debug)]
pub struct Shape {
    pub spec: ShapeSpec,
    pub mass: f64,
    pub inv_mass: f64,
    /// xx, yy, zz, xy, xz, yz
    pub inertia: [f64; 6],
    pub inv_inertia: [f64; 6],
    pub friction: f64,
    pub restitution: f64,
    pub linear_damping: f64,
    pub angular_damping: f64,
    /// Sample spheres: x, y, z, radius for each.
    pub points: Vec<f64>,
    /// Pairs of neighbouring sample points; where one end is on the ground and the other over a drop, the
    /// ground's edge is found between them, so that a box overhanging the counter is carried right up to the edge.
    pub links: Vec<usize>,
    /// Bounding sphere about the centre of mass.
    pub radius: f64,
    pub aabb: [f64; 6],
    /// Where the description's origin sits relative to the centre of mass.
    pub origin: [f64; 3],
    /// What it is made of, as a round meets it (MATERIALS, by the description's name).
    pub material: Material,
    /// Seconds it lies still before it sleeps (the description's `nap`).
    pub nap: f64,
    prims: Vec<Prim>,
}

struct Piece {
    cx: f64,
    cy: f64,
    cz: f64,
    com_y: f64,
    r: f64,
    m: f64,
    ixx: f64,
    iyy: f64,
    izz: f64,
    profile: Vec<f64>,
}

impl Shape {
    pub fn new(spec: &ShapeSpec) -> Shape {
        let spacing = spec.spacing;
        let (mut mass, mut mx, mut my, mut mz) = (0., 0., 0., 0.);
        let mut pieces = Vec::with_capacity(spec.parts.len());
        for part in &spec.parts {
            let piece = match part {
                Part::Box { center, half, radius, mass: m } => {
                    let (m, [ex, ey, ez]) = (*m, *half);
                    Piece {
                        cx: center[0],
                        cy: center[1],
                        cz: center[2],
                        com_y: center[1],
                        r: *radius,
                        m,
                        ixx: m / 3. * (ey * ey + ez * ez),
                        iyy: m / 3. * (ex * ex + ez * ez),
                        izz: m / 3. * (ex * ex + ey * ey),
                        profile: Vec::new(),
                    }
                }
                Part::Lathe { center, profile: outline, radius, mass: m } => {
                    // A thin shell: slice every profile segment into rings and weigh each ring by its area.
                    let m = *m;
                    let profile: Vec<f64> = outline.iter().flat_map(|p| [p[0], p[1]]).collect();
                    let (mut area, mut moment) = (0., 0.);
                    let mut rings = Vec::new();
                    let mut i = 0;
                    while i + 3 < profile.len() {
                        let (ar, ay, er, ey) = (profile[i], profile[i + 1], profile[i + 2] - profile[i], profile[i + 3] - profile[i + 1]);
                        let slice = (er * er + ey * ey).sqrt() / LATHE_SLICES as f64;
                        for k in 0..LATHE_SLICES {
                            let t = (k as f64 + 0.5) / LATHE_SLICES as f64;
                            let (rho, y) = (ar + er * t, ay + ey * t);
                            let w = rho * slice;
                            rings.push([rho, y, w]);
                            area += w;
                            moment += w * y;
                        }
                        i += 2;
                    }
                    let centre = moment / area;
                    let (mut ixx, mut iyy) = (0., 0.);
                    for ring in &rings {
                        let (rho, y, w) = (ring[0], ring[1] - centre, m * ring[2] / area);
                        iyy += w * rho * rho;
                        ixx += w * (rho * rho / 2. + y * y);
                    }
                    Piece { cx: center[0], cy: center[1], cz: center[2], com_y: center[1] + centre, r: *radius, m, ixx, iyy, izz: ixx, profile }
                }
            };
            mass += piece.m;
            mx += piece.m * piece.cx;
            my += piece.m * piece.com_y;
            mz += piece.m * piece.cz;
            pieces.push(piece);
        }
        let (com_x, com_y, com_z) = (mx / mass, my / mass, mz / mass);

        // Inertia about the centre of mass (parallel axis), as xx, yy, zz, xy, xz, yz.
        let (mut ixx, mut iyy, mut izz, mut ixy, mut ixz, mut iyz) = (0., 0., 0., 0., 0., 0.);
        for p in &pieces {
            let (dx, dy, dz) = (p.cx - com_x, p.com_y - com_y, p.cz - com_z);
            ixx += p.ixx + p.m * (dy * dy + dz * dz);
            iyy += p.iyy + p.m * (dx * dx + dz * dz);
            izz += p.izz + p.m * (dx * dx + dy * dy);
            ixy -= p.m * dx * dy;
            ixz -= p.m * dx * dz;
            iyz -= p.m * dy * dz;
        }
        // Inverse of the symmetric matrix by cofactors.
        let (c00, c01, c02) = (iyy * izz - iyz * iyz, ixz * iyz - ixy * izz, ixy * iyz - ixz * iyy);
        let det = ixx * c00 + ixy * c01 + ixz * c02;
        let inv_inertia = [c00 / det, (ixx * izz - ixz * ixz) / det, (ixx * iyy - ixy * ixy) / det, c01 / det, c02 / det, (ixy * ixz - ixx * iyz) / det];

        let (mut prims, mut points, mut links) = (Vec::new(), Vec::<f64>::new(), Vec::<usize>::new());
        let mut radius = 0.;
        let mut aabb = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        fn grow(aabb: &mut [f64; 6], low: [f64; 3], high: [f64; 3]) {
            for k in 0..3 {
                if low[k] < aabb[k] {
                    aabb[k] = low[k];
                }
                if high[k] > aabb[k + 3] {
                    aabb[k + 3] = high[k];
                }
            }
        }
        fn point(points: &mut Vec<f64>, radius: &mut f64, x: f64, y: f64, z: f64, r: f64) {
            points.extend([x, y, z, r]);
            let reach = (x * x + y * y + z * z).sqrt() + r;
            if reach > *radius {
                *radius = reach;
            }
        }
        for (p, part) in pieces.iter().zip(&spec.parts) {
            let (cx, cy, cz, r) = (p.cx - com_x, p.cy - com_y, p.cz - com_z, p.r);
            if let Part::Box { half, .. } = part {
                // The core is the box shrunk by its rounding; the sample spheres sit on the core and carry the rounding.
                let (hx, hy, hz) = (half[0] - r, half[1] - r, half[2] - r);
                prims.push(Prim { lathe: false, cx, cy, cz, hx, hy, hz, r, profile: Vec::new() });
                grow(&mut aabb, [cx - hx - r, cy - hy - r, cz - hz - r], [cx + hx + r, cy + hy + r, cz + hz + r]);
                let count = |h: f64| if h > 1e-9 { (2. * h / spacing - 1e-9).ceil().max(1.) as usize } else { 0 };
                let (nx, ny, nz) = (count(hx), count(hy), count(hz));
                let at = |i: usize, n: usize, h: f64| if n == 0 { 0. } else { -h + 2. * h * i as f64 / n as f64 };
                let slot = |i: usize, j: usize, k: usize| (i * (ny + 1) + j) * (nz + 1) + k;
                let mut index = vec![NONE; (nx + 1) * (ny + 1) * (nz + 1)];
                for i in 0..=nx {
                    for j in 0..=ny {
                        for k in 0..=nz {
                            if i != 0 && i != nx && j != 0 && j != ny && k != 0 && k != nz {
                                continue;
                            }
                            index[slot(i, j, k)] = points.len() / 4;
                            point(&mut points, &mut radius, cx + at(i, nx, hx), cy + at(j, ny, hy), cz + at(k, nz, hz), r);
                        }
                    }
                }
                for i in 0..=nx {
                    for j in 0..=ny {
                        for k in 0..=nz {
                            let from = index[slot(i, j, k)];
                            if from == NONE {
                                continue;
                            }
                            if i < nx && index[slot(i + 1, j, k)] != NONE {
                                links.extend([from, index[slot(i + 1, j, k)]]);
                            }
                            if j < ny && index[slot(i, j + 1, k)] != NONE {
                                links.extend([from, index[slot(i, j + 1, k)]]);
                            }
                            if k < nz && index[slot(i, j, k + 1)] != NONE {
                                links.extend([from, index[slot(i, j, k + 1)]]);
                            }
                        }
                    }
                }
            } else {
                let profile = &p.profile;
                prims.push(Prim { lathe: true, cx, cy, cz, hx: 0., hy: 0., hz: 0., r, profile: profile.clone() });
                let (mut reach, mut low, mut high) = (0., f64::INFINITY, f64::NEG_INFINITY);
                for i in (0..profile.len()).step_by(2) {
                    if profile[i] > reach {
                        reach = profile[i];
                    }
                    if profile[i + 1] < low {
                        low = profile[i + 1];
                    }
                    if profile[i + 1] > high {
                        high = profile[i + 1];
                    }
                }
                grow(&mut aabb, [cx - reach - r, cy + low - r, cz - reach - r], [cx + reach + r, cy + high + r, cz + reach + r]);
                // A ring of samples at every profile corner and half way along every segment.
                let mut ring = |rho: f64, y: f64, stride: usize| {
                    if rho < 1e-9 {
                        point(&mut points, &mut radius, cx, cy + y, cz, r);
                        return;
                    }
                    let (start, count) = (points.len() / 4, 24 / stride);
                    for k in (0..24).step_by(stride) {
                        let [c, s] = direction24(k);
                        point(&mut points, &mut radius, cx + rho * c, cy + y, cz + rho * s, r);
                    }
                    for k in 0..count {
                        links.extend([start + k, start + (k + 1) % count]);
                    }
                };
                // A flat base that starts on the axis gets no sample at its very middle: the shape would stand on
                // that one point, which can hold no turn, and spin on it like a top. It stands on the rings around it.
                for i in (0..profile.len()).step_by(2) {
                    if i > 0 || profile[0] >= 1e-9 || profile.len() < 4 || profile[3] != profile[1] {
                        ring(profile[i], profile[i + 1], if profile[i] > 0.05 { 1 } else { 2 });
                    }
                    if i + 3 < profile.len() {
                        ring((profile[i] + profile[i + 2]) / 2., (profile[i + 1] + profile[i + 3]) / 2., 2);
                    }
                }
            }
        }
        assert!(points.len() / 4 <= 4096 && links.len() / 2 <= 4096 && prims.len() <= 64, "{}: a shape may have 4096 sample points, 4096 links and 64 parts", spec.name);
        Shape {
            spec: spec.clone(),
            mass,
            inv_mass: 1. / mass,
            inertia: [ixx, iyy, izz, ixy, ixz, iyz],
            inv_inertia,
            friction: spec.friction,
            restitution: spec.restitution,
            linear_damping: spec.linear_damping,
            angular_damping: spec.angular_damping,
            points,
            links,
            radius,
            aabb,
            origin: [-com_x, -com_y, -com_z],
            material: material(spec.name),
            nap: spec.nap,
            prims,
        }
    }
}

/// Distance from a point in a shape's frame to one of its solids, the direction away from it, and the face
/// that direction belongs to: [distance, x, y, z, face].
/// Props meet boxes face-on (`round` false): the direction is always a face's, the one the point is furthest
/// outside, so a box resting on a box is pushed straight up however its corners overhang, and piles stand.
/// Characters meet boxes as rounded solids (`round` true), so they slide round corners.
/// The face is for `sided`: 1 or -1 for the two faces of a lathe's shell (0 at its rim, where they meet),
/// 2 to 7 for the faces of a box met face-on, 0 for a rounded box.
#[inline]
#[allow(clippy::manual_clamp)] // written out to match the recorded physics operation for operation
fn distance(prim: &Prim, x: f64, y: f64, z: f64, round: bool) -> [f64; 5] {
    let (lx, ly, lz) = (x - prim.cx, y - prim.cy, z - prim.cz);
    if !prim.lathe {
        let (ax, ay, az) = (lx.abs() - prim.hx, ly.abs() - prim.hy, lz.abs() - prim.hz);
        if !round || (ax <= 0. && ay <= 0. && az <= 0.) {
            let mut found = if ax >= ay && ax >= az {
                [ax - prim.r, if lx < 0. { -1. } else { 1. }, 0., 0., if lx < 0. { 3. } else { 2. }]
            } else if ay >= az {
                [ay - prim.r, 0., if ly < 0. { -1. } else { 1. }, 0., if ly < 0. { 5. } else { 4. }]
            } else {
                [az - prim.r, 0., 0., if lz < 0. { -1. } else { 1. }, if lz < 0. { 7. } else { 6. }]
            };
            if round {
                found[4] = 0.;
            }
            found
        } else {
            let (ox, oy, oz) = (if ax > 0. { ax } else { 0. }, if ay > 0. { ay } else { 0. }, if az > 0. { az } else { 0. });
            let len = (ox * ox + oy * oy + oz * oz).sqrt();
            [len - prim.r, (if lx < 0. { -ox } else { ox }) / len, (if ly < 0. { -oy } else { oy }) / len, (if lz < 0. { -oz } else { oz }) / len, 0.]
        }
    } else {
        // A lathe is its profile in the (distance from axis, height) plane.
        let (rho, p) = ((lx * lx + lz * lz).sqrt(), &prim.profile);
        let (mut best, mut bu, mut bv, mut side) = (f64::INFINITY, 0., 0., 0.);
        let mut i = 0;
        while i + 3 < p.len() {
            let (ar, ay, er, ey) = (p[i], p[i + 1], p[i + 2] - p[i], p[i + 3] - p[i + 1]);
            let mut t = ((rho - ar) * er + (ly - ay) * ey) / (er * er + ey * ey);
            t = if t < 0. { 0. } else if t > 1. { 1. } else { t };
            let (u, v) = (rho - (ar + er * t), ly - (ay + ey * t));
            let d2 = u * u + v * v;
            if d2 < best {
                best = d2;
                bu = u;
                bv = v;
                side = if t >= 1. && i + 4 >= p.len() { 0. } else if er * v - ey * u >= 0. { 1. } else { -1. };
            }
            i += 2;
        }
        let dist = best.sqrt();
        let (mut nr, mut ny) = (0., 1.);
        if dist > 1e-12 {
            nr = bu / dist;
            ny = bv / dist;
        }
        if rho > 1e-12 { [dist - prim.r, nr * lx / rho, ny, nr * lz / rho, side] } else { [dist - prim.r, nr, ny, 0., side] }
    }
}

/// Solids are left the way they were entered. A sample that sinks into a box is pushed back out through the
/// face it came in by, not through whichever face has become nearest, so that a thin thing cutting into a
/// box is not clamped there from both sides; and a sample that was against one face of a lathe's shell and
/// is now found on the other has cut through the wall and still belongs to the face it came from.
/// Call straight after distance(): when the remembered face differs from the one just measured, the
/// direction in `found` and the gap `sep` are replaced. Returns the face the contact belongs to.
#[allow(clippy::too_many_arguments)]
fn sided(cache: &mut Cache, now: u64, a: usize, b: usize, key: u64, prim: &Prim, rho: f64, l: [f64; 3], found: &mut [f64; 5], sep: &mut f64) -> i32 {
    let (side, sd) = (found[4] as i32, found[0]);
    // Outside a box the face just found is the right one, and nothing needs remembering.
    if !prim.lathe && (side == 0 || sd - rho >= -SLOP) {
        return side;
    }
    let Some(kept) = cache.get_mut(&key) else {
        if side != 0 {
            cache.insert(key, Kept { jn: 0., jt1: 0., jt2: 0., hit: 0., side, seen: now, stamp: 0, a, b });
        }
        return side;
    };
    // Seen last substep but thinned away before the solver: its impulses are out of date.
    if kept.stamp < kept.seen {
        (kept.jn, kept.jt1, kept.jt2, kept.hit) = (0., 0., 0., 0.);
    }
    let fresh = kept.seen + 1 >= now;
    kept.seen = now;
    if !fresh || side == 0 || kept.side == 0 || kept.side == side || (kept.side > 1) == prim.lathe {
        kept.side = side;
        return side;
    }
    if prim.lathe {
        *sep = -(sd + 2. * prim.r) - rho;
        (found[1], found[2], found[3]) = (-found[1], -found[2], -found[3]);
        return kept.side;
    }
    // Still behind the remembered face, within its outline?
    let (axis, sign) = ((kept.side - 2) / 2, if kept.side % 2 == 0 { 1. } else { -1. });
    let (x, y, z) = (l[0] - prim.cx, l[1] - prim.cy, l[2] - prim.cz);
    let (ax, ay, az) = (x.abs() - prim.hx, y.abs() - prim.hy, z.abs() - prim.hz);
    if match axis {
        0 => ay > 0. || az > 0.,
        1 => ax > 0. || az > 0.,
        _ => ax > 0. || ay > 0.,
    } {
        kept.side = side;
        return side;
    }
    (found[1], found[2], found[3]) = (0., 0., 0.);
    match axis {
        0 => {
            *sep = sign * x - prim.hx - prim.r - rho;
            found[1] = sign;
        }
        1 => {
            *sep = sign * y - prim.hy - prim.r - rho;
            found[2] = sign;
        }
        _ => {
            *sep = sign * z - prim.hz - prim.r - rho;
            found[3] = sign;
        }
    }
    kept.side
}

// ---------------------------------------------------------------------------------------------------------
// Levels

/// What the props need to know about a level.
pub trait PropLevel {
    /// World-space height of the surface under a point inside the bounds.
    fn ground(&self, x: f64, z: f64) -> f64;
    /// [min x, min z, max x, max z]: the counter's edge. Beyond it there is no ground.
    fn bounds(&self) -> [f64; 4];
    /// Solid blocks: [min x, min z, max x, max z, top in world space].
    fn blocks(&self) -> Vec<[f64; 5]>;
}

/// A level for tests and recorded scenarios, with the same surface rules as map.rs: decks are
/// [x0, z0, x1, z1, height], ramps [x0, z0, x1, z1, base, top, along z, direction] and blocks
/// [x0, z0, x1, z1, height], all heights above `floor`.
#[derive(Clone, Debug)]
pub struct SimpleLevel {
    pub bounds: [f64; 4],
    pub floor: f64,
    pub blocks: Vec<[f64; 5]>,
    pub decks: Vec<[f64; 5]>,
    pub ramps: Vec<[f64; 8]>,
}
impl Default for SimpleLevel {
    fn default() -> SimpleLevel {
        SimpleLevel { bounds: [-2., -2., 2., 2.], floor: 0.9025, blocks: Vec::new(), decks: Vec::new(), ramps: Vec::new() }
    }
}
impl PropLevel for SimpleLevel {
    fn ground(&self, x: f64, z: f64) -> f64 {
        let mut h = 0.;
        for d in &self.decks {
            if d[4] > h && x >= d[0] && x <= d[2] && z >= d[1] && z <= d[3] {
                h = d[4];
            }
        }
        for r in &self.ramps {
            if x >= r[0] && x <= r[2] && z >= r[1] && z <= r[3] {
                let t = if r[6] != 0. { (z - r[1]) / (r[3] - r[1]) } else { (x - r[0]) / (r[2] - r[0]) };
                let v = r[4] + (if r[7] > 0. { t } else { 1. - t }) * (r[5] - r[4]);
                if v > h {
                    h = v;
                }
            }
        }
        self.floor + h
    }
    fn bounds(&self) -> [f64; 4] {
        self.bounds
    }
    fn blocks(&self) -> Vec<[f64; 5]> {
        self.blocks.iter().map(|b| [b[0], b[1], b[2], b[3], self.floor + b[4]]).collect()
    }
}

/// The game's maps. Only the map module's public surface is used.
impl PropLevel for crate::map::Level {
    fn ground(&self, x: f64, z: f64) -> f64 {
        crate::map::Level::ground(self, x, z)
    }
    fn bounds(&self) -> [f64; 4] {
        [self.x0, self.z0, self.x1, self.z1]
    }
    fn blocks(&self) -> Vec<[f64; 5]> {
        self.map.blocks.iter().map(|b| [b.rect[0], b.rect[1], b.rect[2], b.rect[3], crate::map::FLOOR + b.height]).collect()
    }
}

/// The level as props see it: its surface inside the bounds, the kitchen floor beyond the counter's edge.
#[derive(Clone, Copy)]
struct Terrain<'a> {
    level: &'a dyn PropLevel,
    x0: f64,
    z0: f64,
    x1: f64,
    z1: f64,
    floor_y: f64,
}
impl Terrain<'_> {
    #[inline]
    fn ground(&self, x: f64, z: f64) -> f64 {
        if x >= self.x0 && x < self.x1 && z >= self.z0 && z < self.z1 { self.level.ground(x, z) } else { self.floor_y }
    }
}

// ---------------------------------------------------------------------------------------------------------
// The world

/// Awake and asleep props collide; carried, lost, removed and draped (frozen under or as a draped napkin) ones do
/// not. A locked one (put down where it was built: `PropWorld::lock`) is part of the scenery until something unlocks
/// it: rounds, characters and other props meet it, but nothing moves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Awake = 0,
    Asleep = 1,
    Carried = 2,
    Lost = 3,
    Removed = 4,
    Draped = 5,
    Locked = 6,
}
impl State {
    /// A state as a number (as frames carry it, net.rs) back again; a number it does not know is Removed.
    pub fn from_number(n: u8) -> State {
        match n {
            0 => State::Awake,
            1 => State::Asleep,
            2 => State::Carried,
            3 => State::Lost,
            5 => State::Draped,
            6 => State::Locked,
            _ => State::Removed,
        }
    }
}

/// A prop. Position and velocities are those of its centre of mass; use PropWorld::position for its origin.
#[derive(Clone, Debug)]
pub struct Body {
    pub id: usize,
    pub shape: usize,
    pub state: State,
    /// Whoever carries it, or -1.
    pub holder: i32,
    pub px: f64,
    pub py: f64,
    pub pz: f64,
    pub qx: f64,
    pub qy: f64,
    pub qz: f64,
    pub qw: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub wx: f64,
    pub wy: f64,
    pub wz: f64,
    // Motion that only pushes overlaps apart: used for one substep's move, then discarded.
    sx: f64,
    sy: f64,
    sz: f64,
    tx: f64,
    ty: f64,
    tz: f64,
    // Rotation matrix and world inverse inertia.
    r00: f64,
    r01: f64,
    r02: f64,
    r10: f64,
    r11: f64,
    r12: f64,
    r20: f64,
    r21: f64,
    r22: f64,
    ixx: f64,
    iyy: f64,
    izz: f64,
    ixy: f64,
    ixz: f64,
    iyz: f64,
    inv_mass: f64,
    radius: f64,
    /// The sample points in world space: x, y, z for each.
    pub world: Vec<f64>,
    sleep_time: f64,
    parent: usize,
    quiet: f64,
    margin: f64,
    // The pose it was last seen to leave (position, then rotation), and how long it has stayed near it.
    anchor: [f64; 7],
    stuck: f64,
    // Impulses from characters: waiting for the next step and applied in the last step; and the force and
    // torque they came to when the prop fell asleep.
    load: [f64; 6],
    last: [f64; 6],
    rest: [f64; 6],
    /// Locked: the props it stands on and where their centres were when it was locked. One of them taken away or
    /// moved, and it is loose again.
    pub supports: Vec<(usize, [f64; 3])>,
    /// To be locked once it comes to rest (put down on the move).
    pub settle: bool,
}

/// A contact acts along three directions: its normal n and two friction directions 1 and 2. For each direction
/// and each body it keeps the lever (arm x direction: `ar`, `br`) and the spin an impulse along it causes
/// (inverse inertia times the lever: `aw`, `bw`), so that the solver's inner loop is a handful of dot products.
#[derive(Clone, Copy, Default)]
struct Contact {
    a: usize,
    b: usize,
    other: i32,
    key: u64,
    n: [f64; 3],
    p: [f64; 3],
    sep: f64,
    t1: [f64; 3],
    t2: [f64; 3],
    arn: [f64; 3],
    awn: [f64; 3],
    brn: [f64; 3],
    bwn: [f64; 3],
    ar1: [f64; 3],
    aw1: [f64; 3],
    br1: [f64; 3],
    bw1: [f64; 3],
    ar2: [f64; 3],
    aw2: [f64; 3],
    br2: [f64; 3],
    bw2: [f64; 3],
    mn: f64,
    mt1: f64,
    mt2: f64,
    jn: f64,
    jt1: f64,
    jt2: f64,
    jp: f64,
    friction: f64,
    restitution: f64,
    approach: f64,
    side: i32,
}

/// What is remembered of a contact from one substep to the next: its impulses, how fast it was closing
/// while still apart, the face it belongs to, and when it was last found (`seen`) and solved (`stamp`).
#[derive(Clone)]
struct Kept {
    jn: f64,
    jt1: f64,
    jt2: f64,
    hit: f64,
    side: i32,
    seen: u64,
    stamp: u64,
    a: usize,
    b: usize,
}
type Cache = HashMap<u64, Kept, BuildHasherDefault<KeyHasher>>;

// Contact keys are already well spread integers; this is far cheaper than the default hasher.
#[derive(Default)]
struct KeyHasher(u64);
impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ byte as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn write_u64(&mut self, value: u64) {
        let mixed = (value ^ (value >> 29)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        self.0 = mixed ^ (mixed >> 32);
    }
}

#[derive(Clone)]
struct Block {
    x0: f64,
    z0: f64,
    x1: f64,
    z1: f64,
    top: f64,
    points: Vec<f64>,
    prim: Prim,
}
/// A block over [x0, z0, x1, z1] from below the counter up to `top`, with sample points along its upright edges
/// and round its top so that a prop's flat face cannot swallow a block's corner.
fn block(terrain: &Terrain, bx0: f64, bz0: f64, bx1: f64, bz1: f64, top: f64) -> Block {
    let mut base = top;
    for (x, z) in [(bx0, bz0), (bx1, bz0), (bx0, bz1), (bx1, bz1)] {
        let g = terrain.ground(x, z);
        if g < base {
            base = g;
        }
    }
    let mut points = Vec::new();
    let count = |length: f64| (length / 0.04 - 1e-9).ceil().max(1.) as usize;
    let (nx, nz, ny) = (count(bx1 - bx0), count(bz1 - bz0), count(top - base));
    for k in 0..nx {
        let x = bx0 + (bx1 - bx0) * k as f64 / nx as f64;
        points.extend([x, top, bz0, bx1 - (x - bx0), top, bz1]);
    }
    for k in 0..nz {
        let z = bz0 + (bz1 - bz0) * k as f64 / nz as f64;
        points.extend([bx1, top, z, bx0, top, bz1 - (z - bz0)]);
    }
    for k in 1..ny {
        let y = top - (top - base) * k as f64 / ny as f64;
        points.extend([bx0, y, bz0, bx1, y, bz0, bx0, y, bz1, bx1, y, bz1]);
    }
    let low = (if base < terrain.floor_y { base } else { terrain.floor_y }) - 1.;
    let prim = Prim {
        lathe: false,
        cx: (bx0 + bx1) / 2.,
        cy: (low + top) / 2.,
        cz: (bz0 + bz1) / 2.,
        hx: (bx1 - bx0) / 2.,
        hy: (top - low) / 2.,
        hz: (bz1 - bz0) / 2.,
        r: 0.,
        profile: Vec::new(),
    };
    Block { x0: bx0, z0: bz0, x1: bx1, z1: bz1, top, points, prim }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// Two things knocked together: the pan clangs.
    Impact,
    Sleep,
    Wake,
    /// The prop fell off the counter and reached the kitchen floor.
    Lost,
}
/// Something the game may want to make a sound or a puff of dust for. In an impact `other` is another prop's
/// id, GROUND, or BLOCK - n for the level's block n; `speed` is the closing speed in m/s and `impulse` the
/// newton seconds exchanged. The other kinds only carry `id`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub kind: EventKind,
    pub id: usize,
    pub other: i32,
    pub point: [f64; 3],
    pub normal: [f64; 3],
    pub speed: f64,
    pub impulse: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub gravity: f64,
    /// Height of the kitchen floor: a prop that falls off the counter is lost when it gets there.
    pub floor_y: f64,
    pub ground_friction: f64,
}
impl Default for Config {
    fn default() -> Config {
        Config { gravity: GRAVITY, floor_y: 0., ground_friction: 0.5 }
    }
}

/// A hotdog or a zombie: an upright capsule that the game moves itself. `position` is its feet and `velocity`
/// what moved it this tick. A prop receives at most `push_force * dt` of sideways impulse from it.
#[derive(Clone, Copy, Debug)]
pub struct Character {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub radius: f64,
    pub height: f64,
    pub mass: f64,
    pub push_force: f64,
}
impl Character {
    /// A character of the usual build: 0.4 kg.
    pub fn new(position: [f64; 3], velocity: [f64; 3], radius: f64, height: f64) -> Character {
        Character::weighing(position, velocity, radius, height, 0.4)
    }
    /// A character of the given mass, pushing with 2.85 N for each kilogram of it.
    pub fn weighing(position: [f64; 3], velocity: [f64; 3], radius: f64, height: f64, mass: f64) -> Character {
        Character { position, velocity, radius, height, mass, push_force: mass * PUSH }
    }
}
/// The highest prop surface under a character's feet that it could stand on, however far below.
#[derive(Clone, Copy, Debug)]
pub struct Support {
    pub id: usize,
    pub height: f64,
    /// Velocity of the prop's surface there, for riding along.
    pub velocity: [f64; 3],
    pub normal: [f64; 3],
    /// The feet are on it.
    pub standing: bool,
}
/// An impulse a character handed to a prop; it acts at the next step.
#[derive(Clone, Copy, Debug)]
pub struct Push {
    pub id: usize,
    pub point: [f64; 3],
    pub impulse: [f64; 3],
}
#[derive(Clone, Debug)]
pub struct Resolved {
    /// Where the character may stand: pushed out of walls, lifted onto what it stands on.
    pub position: [f64; 3],
    /// The velocity it has left after the shove.
    pub velocity: [f64; 3],
    pub support: Option<Support>,
    /// Newton seconds a fast-moving prop knocked the character with.
    pub impulse: Option<[f64; 3]>,
    pub impulses: Vec<Push>,
}
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub id: usize,
    pub distance: f64,
    pub point: [f64; 3],
    pub normal: [f64; 3],
}
/// Everything needed to recreate a prop elsewhere: send it in snapshots, feed it to set_state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropState {
    pub id: usize,
    pub shape: usize,
    pub state: State,
    pub holder: i32,
    pub position: [f64; 3],
    pub rotation: [f64; 4],
    pub velocity: [f64; 3],
    pub angular_velocity: [f64; 3],
}

#[derive(Clone)]
pub struct PropWorld {
    pub gravity: f64,
    pub floor_y: f64,
    pub ground_friction: f64,
    shapes: Vec<Shape>,
    bodies: Vec<Body>,
    events: Vec<Event>,
    contacts: Vec<Contact>,
    cache: Cache,
    substeps: u64,
    remainder: f64,
    queue: Vec<usize>,
    group: Vec<usize>,
    keep: Vec<bool>,
    // Per sample of the prop being tested against the ground: 0 not looked at, 1 resting on a floor, 2 clear of it.
    footing: Vec<u8>,
    floor: Vec<f64>,
    bounds: [f64; 4],
    // The highest ground near each cell of a coarse grid, so that samples in the open skip the search for
    // floors and walls.
    cell: f64,
    cols: usize,
    rows: usize,
    grid_x: f64,
    grid_z: f64,
    high: Vec<f64>,
    blocks: Vec<Block>,
    // The locked props, gathered afresh each substep.
    locked: Vec<usize>,
}

/// Speed a body's surface gains at arm r per unit of impulse along d.
#[inline]
fn give(b: &Body, rx: f64, ry: f64, rz: f64, dx: f64, dy: f64, dz: f64) -> f64 {
    let (cx, cy, cz) = (ry * dz - rz * dy, rz * dx - rx * dz, rx * dy - ry * dx);
    b.inv_mass + (cx * (b.ixx * cx + b.ixy * cy + b.ixz * cz) + cy * (b.ixy * cx + b.iyy * cy + b.iyz * cz) + cz * (b.ixz * cx + b.iyz * cy + b.izz * cz))
}
/// r x d.
#[inline]
fn cross(r: [f64; 3], d: [f64; 3]) -> [f64; 3] {
    [r[1] * d[2] - r[2] * d[1], r[2] * d[0] - r[0] * d[2], r[0] * d[1] - r[1] * d[0]]
}
/// A body's world inverse inertia times a lever.
#[inline]
fn spin(b: &Body, c: [f64; 3]) -> [f64; 3] {
    [b.ixx * c[0] + b.ixy * c[1] + b.ixz * c[2], b.ixy * c[0] + b.iyy * c[1] + b.iyz * c[2], b.ixz * c[0] + b.iyz * c[1] + b.izz * c[2]]
}
/// Speed of a body along direction d at the point whose lever is `lever`.
#[inline]
fn along(b: &Body, d: &[f64; 3], lever: &[f64; 3]) -> f64 {
    d[0] * b.vx + d[1] * b.vy + d[2] * b.vz + (lever[0] * b.wx + lever[1] * b.wy + lever[2] * b.wz)
}
/// The same for the motion that only separates overlaps.
#[inline]
fn along_push(b: &Body, d: &[f64; 3], lever: &[f64; 3]) -> f64 {
    d[0] * b.sx + d[1] * b.sy + d[2] * b.sz + (lever[0] * b.tx + lever[1] * b.ty + lever[2] * b.tz)
}
#[inline]
fn near(a: &Body, b: &Body, margin: f64) -> bool {
    let (dx, dy, dz, reach) = (a.px - b.px, a.py - b.py, a.pz - b.pz, a.radius + b.radius + margin);
    dx * dx + dy * dy + dz * dz < reach * reach
}
#[inline]
fn colliding(state: State) -> bool {
    state == State::Awake || state == State::Asleep
}
/// Takes up room: awake, asleep or locked (rounds, characters and other props meet it).
#[inline]
fn present(state: State) -> bool {
    colliding(state) || state == State::Locked
}
/// A locked body is part of the scenery: no impulse moves or turns it.
fn pin(b: &mut Body) {
    b.inv_mass = 0.;
    (b.ixx, b.iyy, b.izz, b.ixy, b.ixz, b.iyz) = (0., 0., 0., 0., 0., 0.);
    (b.vx, b.vy, b.vz, b.wx, b.wy, b.wz) = (0., 0., 0., 0., 0., 0.);
}
/// And loose again: its own mass and inertia back.
fn unpin(b: &mut Body, shape: &Shape) {
    b.inv_mass = shape.inv_mass;
    sync(b, shape);
    b.supports.clear();
}

/// Recomputes what follows from a body's pose: rotation matrix, world inverse inertia, world sample points.
fn sync(b: &mut Body, shape: &Shape) {
    let (x2, y2, z2) = (b.qx + b.qx, b.qy + b.qy, b.qz + b.qz);
    let (xx, xy, xz, yy, yz, zz, wx, wy, wz) = (b.qx * x2, b.qx * y2, b.qx * z2, b.qy * y2, b.qy * z2, b.qz * z2, b.qw * x2, b.qw * y2, b.qw * z2);
    let (r00, r01, r02) = (1. - (yy + zz), xy - wz, xz + wy);
    let (r10, r11, r12) = (xy + wz, 1. - (xx + zz), yz - wx);
    let (r20, r21, r22) = (xz - wy, yz + wx, 1. - (xx + yy));
    (b.r00, b.r01, b.r02, b.r10, b.r11, b.r12, b.r20, b.r21, b.r22) = (r00, r01, r02, r10, r11, r12, r20, r21, r22);
    // World inverse inertia = R * I * R^T.
    let i = &shape.inv_inertia;
    let (m00, m01, m02) = (r00 * i[0] + r01 * i[3] + r02 * i[4], r00 * i[3] + r01 * i[1] + r02 * i[5], r00 * i[4] + r01 * i[5] + r02 * i[2]);
    let (m10, m11, m12) = (r10 * i[0] + r11 * i[3] + r12 * i[4], r10 * i[3] + r11 * i[1] + r12 * i[5], r10 * i[4] + r11 * i[5] + r12 * i[2]);
    let (m20, m21, m22) = (r20 * i[0] + r21 * i[3] + r22 * i[4], r20 * i[3] + r21 * i[1] + r22 * i[5], r20 * i[4] + r21 * i[5] + r22 * i[2]);
    b.ixx = m00 * r00 + m01 * r01 + m02 * r02;
    b.ixy = m00 * r10 + m01 * r11 + m02 * r12;
    b.ixz = m00 * r20 + m01 * r21 + m02 * r22;
    b.iyy = m10 * r10 + m11 * r11 + m12 * r12;
    b.iyz = m10 * r20 + m11 * r21 + m12 * r22;
    b.izz = m20 * r20 + m21 * r21 + m22 * r22;
    for (p, w) in shape.points.as_chunks::<4>().0.iter().zip(b.world.as_chunks_mut::<3>().0) {
        let (x, y, z) = (p[0], p[1], p[2]);
        w[0] = b.px + (r00 * x + r01 * y + r02 * z);
        w[1] = b.py + (r10 * x + r11 * y + r12 * z);
        w[2] = b.pz + (r20 * x + r21 * y + r22 * z);
    }
}

/// What PropWorld::touch found: the signed gap, the normal from prop to character, the centre of the sphere.
#[derive(Clone, Copy)]
struct Touch {
    d: f64,
    nx: f64,
    ny: f64,
    nz: f64,
    x: f64,
    y: f64,
    z: f64,
}

impl PropWorld {
    pub fn new(level: &dyn PropLevel, config: Config) -> PropWorld {
        let bounds = level.bounds();
        let [x0, z0, x1, z1] = bounds;
        let terrain = Terrain { level, x0, z0, x1, z1, floor_y: config.floor_y };
        // Each cell of the grid is measured on a lattice a fifth of its size, then widened by two cells.
        let span = if x1 - x0 > z1 - z0 { x1 - x0 } else { z1 - z0 };
        let cell = if span / 512. > 0.05 { span / 512. } else { 0.05 };
        let (cols, rows) = (((x1 - x0) / cell).ceil() as usize + 4, ((z1 - z0) / cell).ceil() as usize + 4);
        let (grid_x, grid_z) = (x0 - 2. * cell, z0 - 2. * cell);
        let (mut own, mut high) = (vec![0.; cols * rows], vec![0.; cols * rows]);
        for j in 0..rows {
            for i in 0..cols {
                let mut top = f64::NEG_INFINITY;
                for a in 0..=5 {
                    for b in 0..=5 {
                        let g = terrain.ground(grid_x + (i as f64 + a as f64 / 5.) * cell, grid_z + (j as f64 + b as f64 / 5.) * cell);
                        if g > top {
                            top = g;
                        }
                    }
                }
                own[j * cols + i] = top;
            }
        }
        for j in 0..rows {
            for i in 0..cols {
                let mut top = f64::NEG_INFINITY;
                for b in j.saturating_sub(2)..=(j + 2).min(rows - 1) {
                    for a in i.saturating_sub(2)..=(i + 2).min(cols - 1) {
                        if own[b * cols + a] > top {
                            top = own[b * cols + a];
                        }
                    }
                }
                high[j * cols + i] = top;
            }
        }
        // Blocks as solids, each with sample points along its upright edges and round its top so that
        // a prop's flat face cannot swallow a block's corner.
        let blocks = level.blocks().into_iter().map(|[bx0, bz0, bx1, bz1, top]| block(&terrain, bx0, bz0, bx1, bz1, top)).collect();
        PropWorld {
            gravity: config.gravity,
            floor_y: config.floor_y,
            ground_friction: config.ground_friction,
            shapes: Vec::new(),
            bodies: Vec::new(),
            events: Vec::new(),
            contacts: Vec::new(),
            cache: HashMap::default(),
            substeps: 0,
            remainder: 0.,
            queue: Vec::new(),
            group: Vec::new(),
            keep: Vec::new(),
            footing: Vec::new(),
            floor: Vec::new(),
            bounds,
            cell,
            cols,
            rows,
            grid_x,
            grid_z,
            high,
            blocks,
            locked: Vec::new(),
        }
    }

    // ----- setting up

    /// Registers a shape and returns its number.
    pub fn add_shape(&mut self, spec: &ShapeSpec) -> usize {
        self.shapes.push(Shape::new(spec));
        self.shapes.len() - 1
    }
    pub fn shape(&self, shape: usize) -> &Shape {
        &self.shapes[shape]
    }

    /// Adds a prop with its origin at `position` turned by the quaternion `rotation` ([x, y, z, w]); returns its id.
    pub fn add(&mut self, shape: usize, position: [f64; 3], rotation: [f64; 4]) -> usize {
        assert!(self.bodies.len() < 65536, "a world holds at most 65536 props");
        let (id, s) = (self.bodies.len(), &self.shapes[shape]);
        self.bodies.push(Body {
            id,
            shape,
            state: State::Awake,
            holder: -1,
            px: 0.,
            py: 0.,
            pz: 0.,
            qx: 0.,
            qy: 0.,
            qz: 0.,
            qw: 1.,
            vx: 0.,
            vy: 0.,
            vz: 0.,
            wx: 0.,
            wy: 0.,
            wz: 0.,
            sx: 0.,
            sy: 0.,
            sz: 0.,
            tx: 0.,
            ty: 0.,
            tz: 0.,
            r00: 1.,
            r01: 0.,
            r02: 0.,
            r10: 0.,
            r11: 1.,
            r12: 0.,
            r20: 0.,
            r21: 0.,
            r22: 1.,
            ixx: 0.,
            iyy: 0.,
            izz: 0.,
            ixy: 0.,
            ixz: 0.,
            iyz: 0.,
            inv_mass: s.inv_mass,
            radius: s.radius,
            world: vec![0.; s.points.len() / 4 * 3],
            sleep_time: 0.,
            parent: 0,
            quiet: 0.,
            margin: MARGIN,
            anchor: [0., 0., 0., 0., 0., 0., 1.],
            stuck: 0.,
            load: [0.; 6],
            last: [0.; 6],
            rest: [0.; 6],
            supports: Vec::new(),
            settle: false,
        });
        self.place(id, position, rotation);
        id
    }
    /// Takes away the level's last block (a forcefield that stood as a block and has dropped: the game builds
    /// the world from a level that has it last). Every sleeping prop wakes, in case it leant on it.
    pub fn unblock(&mut self) {
        self.blocks.pop();
        for id in 0..self.bodies.len() {
            if self.bodies[id].state == State::Asleep {
                self.wake(id);
            }
        }
    }
    /// Adds a block over [x0, z0, x1, z1] up to `top` as the level's own are, at index `at` among the blocks (a game
    /// whose forcefield is the last block puts its own before it), and returns that index. `level` is the level
    /// the world was made for. Sleeping props it touches wake. A draped napkin's wall is one.
    pub fn wall(&mut self, level: &dyn PropLevel, rect: [f64; 4], top: f64, at: usize) -> usize {
        let [x0, z0, x1, z1] = self.bounds;
        let terrain = Terrain { level, x0, z0, x1, z1, floor_y: self.floor_y };
        let at = at.min(self.blocks.len());
        self.blocks.insert(at, block(&terrain, rect[0], rect[1], rect[2], rect[3], top));
        self.stir(at);
        at
    }
    /// Takes the block at index `at` away again; sleeping props near it wake.
    pub fn unwall(&mut self, at: usize) {
        if at < self.blocks.len() {
            self.stir(at);
            // What was locked on top of it has lost its footing.
            let block = &self.blocks[at];
            let (x0, z0, x1, z1, top) = (block.x0, block.z0, block.x1, block.z1, block.top);
            for id in 0..self.bodies.len() {
                if self.bodies[id].state != State::Locked {
                    continue;
                }
                let e = self.extent(id);
                if e[4] > top - 0.006 && e[4] < top + 0.012 && e[0] < x1 && e[2] > x0 && e[1] < z1 && e[3] > z0 {
                    self.wake(id);
                }
            }
            self.blocks.remove(at);
        }
    }
    /// The number of blocks the props meet: the level's, and those added since.
    pub fn walls(&self) -> usize {
        self.blocks.len()
    }
    fn stir(&mut self, at: usize) {
        let block = &self.blocks[at];
        let (x0, z0, x1, z1, top) = (block.x0, block.z0, block.x1, block.z1, block.top);
        for id in 0..self.bodies.len() {
            let b = &self.bodies[id];
            if b.state != State::Asleep {
                continue;
            }
            let qx = if b.px < x0 { x0 - b.px } else if b.px > x1 { b.px - x1 } else { 0. };
            let qz = if b.pz < z0 { z0 - b.pz } else if b.pz > z1 { b.pz - z1 } else { 0. };
            let qy = if b.py > top { b.py - top } else { 0. };
            let reach = self.shapes[b.shape].radius + 0.01;
            if qx * qx + qy * qy + qz * qz <= reach * reach {
                self.wake(id);
            }
        }
    }
    /// Freezes a prop where it is (a napkin draped over it, and the napkin itself): it stops moving and
    /// colliding, a shot passes it (the game's wall stands for it), and whatever rested on it wakes. False unless
    /// it was awake or asleep.
    pub fn freeze(&mut self, id: usize) -> bool {
        if id >= self.bodies.len() || !present(self.bodies[id].state) {
            return false;
        }
        self.wake_near(id);
        let kind = self.bodies[id].shape;
        let (b, shape) = (&mut self.bodies[id], &self.shapes[kind]);
        if b.state == State::Locked {
            unpin(b, shape);
        }
        (b.state, b.holder, b.sleep_time, b.stuck, b.settle) = (State::Draped, -1, 0., 0., false);
        (b.vx, b.vy, b.vz, b.wx, b.wy, b.wz) = (0., 0., 0., 0., 0., 0.);
        true
    }
    /// A frozen prop is loose again, awake where it was.
    pub fn thaw(&mut self, id: usize) -> bool {
        if id >= self.bodies.len() || self.bodies[id].state != State::Draped {
            return false;
        }
        self.wake(id);
        true
    }
    /// Locks a prop where it is (State::Locked): it stops, and is part of the scenery until it is picked up, a blast
    /// throws it, or one of the props it stands on (found now, under its underside) is carried off, lost, taken away
    /// or moved. False unless it was awake, asleep or locked already.
    pub fn lock(&mut self, id: usize) -> bool {
        if id >= self.bodies.len() || !present(self.bodies[id].state) {
            return false;
        }
        let supports = self.beneath(id);
        let b = &mut self.bodies[id];
        (b.state, b.settle, b.sleep_time, b.stuck, b.supports) = (State::Locked, false, 0., 0., supports);
        pin(b);
        true
    }
    /// A locked prop is loose again (awake where it is). False unless it was locked.
    pub fn unlock(&mut self, id: usize) -> bool {
        if id >= self.bodies.len() || self.bodies[id].state != State::Locked {
            return false;
        }
        self.wake(id);
        true
    }
    /// Puts to sleep every awake prop that already rests where it is, as a map puts its loose objects out: every sample
    /// of its underside on the level's ground, a block's top or another prop's top, to within a millimetre. One put out
    /// in the air, over an edge or tilted stays awake, and falls. What stands in a pile (on another prop, or under one)
    /// is locked as well, as a pile put out on purpose is: nothing knocks it about until it is taken from, blasted, or
    /// has something taken from under it. (A round's hundreds of piled pillows cost nothing then from its first step,
    /// rather than settling for a second, and one thrown onto a pile does not set the whole pile going.) `level` is the
    /// level the world was made for.
    pub fn rest_where_put(&mut self, level: &dyn PropLevel) {
        let [x0, z0, x1, z1] = self.bounds;
        let terrain = Terrain { level, x0, z0, x1, z1, floor_y: self.floor_y };
        let mut piled = vec![false; self.bodies.len()];
        for id in 0..self.bodies.len() {
            if self.bodies[id].state != State::Awake {
                continue;
            }
            let (b, shape) = (&self.bodies[id], &self.shapes[self.bodies[id].shape]);
            let samples: Vec<[f64; 4]> = b.world.as_chunks::<3>().0.iter().zip(shape.points.as_chunks::<4>().0).map(|(w, p)| [w[0], w[1], w[2], p[3]]).collect();
            let low = samples.iter().map(|s| s[1] - s[3]).fold(f64::INFINITY, f64::min);
            let resting = samples.iter().filter(|s| s[1] - s[3] < low + 0.001).all(|&[x, y, z, rho]| {
                let foot = y - rho;
                let mut under = terrain.ground(x, z);
                for k in &self.blocks {
                    if x >= k.x0 && x <= k.x1 && z >= k.z0 && z <= k.z1 && k.top < foot + 0.001 && k.top > under {
                        under = k.top;
                    }
                }
                if let Some(hit) = self.cast([x, y, z], [0., -1., 0.], rho + 0.002, |o| o.id != id && present(o.state)) {
                    if y - hit.distance > under {
                        under = y - hit.distance;
                        if (foot - under).abs() < 0.001 {
                            piled[id] = true;
                            piled[hit.id] = true;
                        }
                    }
                }
                (foot - under).abs() < 0.001
            });
            if !resting {
                piled[id] = false;
            }
            if resting {
                let b = &mut self.bodies[id];
                (b.state, b.sleep_time, b.stuck, b.rest) = (State::Asleep, 0., 0., [0.; 6]);
                (b.vx, b.vy, b.vz, b.wx, b.wy, b.wz) = (0., 0., 0., 0., 0., 0.);
            }
        }
        for id in 0..self.bodies.len() {
            if piled[id] && self.bodies[id].state == State::Asleep {
                self.lock(id);
            }
        }
    }
    /// Whether the prop is locked.
    pub fn locked(&self, id: usize) -> bool {
        self.bodies.get(id).is_some_and(|b| b.state == State::Locked)
    }
    /// A loose prop is to be locked once it comes to rest (put down on the move: it slides on, and locks where it
    /// stops).
    pub fn lock_at_rest(&mut self, id: usize) {
        if id < self.bodies.len() && colliding(self.bodies[id].state) {
            self.bodies[id].settle = true;
        }
    }
    /// The props a prop stands on, and where their centres are: whatever is within a few millimetres under the
    /// samples of its underside.
    fn beneath(&self, id: usize) -> Vec<(usize, [f64; 3])> {
        let (b, shape) = (&self.bodies[id], &self.shapes[self.bodies[id].shape]);
        let mut low = f64::INFINITY;
        for (w, p) in b.world.as_chunks::<3>().0.iter().zip(shape.points.as_chunks::<4>().0) {
            low = low.min(w[1] - p[3]);
        }
        let mut found: Vec<(usize, [f64; 3])> = Vec::new();
        for (w, p) in b.world.as_chunks::<3>().0.iter().zip(shape.points.as_chunks::<4>().0) {
            if w[1] - p[3] > low + 0.004 {
                continue;
            }
            let hit = self.cast([w[0], w[1], w[2]], [0., -1., 0.], p[3] + 0.006, |o| o.id != id && present(o.state));
            if let Some(hit) = hit.filter(|hit| !found.iter().any(|(other, _)| *other == hit.id)) {
                let o = &self.bodies[hit.id];
                found.push((hit.id, [o.px, o.py, o.pz]));
            }
        }
        found
    }
    /// Locked props whose footing has gone (a prop they stood on carried off, lost, taken away, or moved more than
    /// 2 mm) are loose again, and fall.
    fn footing(&mut self) {
        for id in 0..self.bodies.len() {
            let b = &self.bodies[id];
            if b.state != State::Locked || b.supports.is_empty() {
                continue;
            }
            let gone = b.supports.iter().any(|(other, at)| {
                let o = &self.bodies[*other];
                let (dx, dy, dz) = (o.px - at[0], o.py - at[1], o.pz - at[2]);
                matches!(o.state, State::Carried | State::Lost | State::Removed) || dx * dx + dy * dy + dz * dz > 0.002 * 0.002
            });
            if gone {
                self.wake(id);
            }
        }
    }

    /// Takes a prop out of the world for good; whatever rested on it wakes.
    pub fn remove(&mut self, id: usize) {
        if id >= self.bodies.len() || self.bodies[id].state == State::Removed {
            return;
        }
        self.wake_near(id);
        self.bodies[id].state = State::Removed;
    }

    // ----- reading

    pub fn len(&self) -> usize {
        self.bodies.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }
    pub fn body(&self, id: usize) -> &Body {
        &self.bodies[id]
    }
    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }
    pub fn state(&self, id: usize) -> State {
        self.bodies[id].state
    }
    /// Where the prop's origin is.
    pub fn position(&self, id: usize) -> [f64; 3] {
        let b = &self.bodies[id];
        let o = &self.shapes[b.shape].origin;
        [b.px + (b.r00 * o[0] + b.r01 * o[1] + b.r02 * o[2]), b.py + (b.r10 * o[0] + b.r11 * o[1] + b.r12 * o[2]), b.pz + (b.r20 * o[0] + b.r21 * o[1] + b.r22 * o[2])]
    }
    /// Where the prop's sample spheres reach: [x0, z0, x1, z1, lowest, highest].
    pub fn extent(&self, id: usize) -> [f64; 6] {
        let (b, s) = (&self.bodies[id], &self.shapes[self.bodies[id].shape]);
        let mut e = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
        for (k, n) in (0..b.world.len()).step_by(3).zip((3..s.points.len()).step_by(4)) {
            let (w, q) = (&b.world, s.points[n]);
            for (i, v, low) in [(0, w[k] - q, true), (2, w[k] + q, false), (1, w[k + 2] - q, true), (3, w[k + 2] + q, false), (4, w[k + 1] - q, true), (5, w[k + 1] + q, false)] {
                if (low && v < e[i]) || (!low && v > e[i]) {
                    e[i] = v;
                }
            }
        }
        e
    }
    /// A point in the world in the frame of the prop's shape (about its centre of mass).
    pub fn local(&self, id: usize, point: [f64; 3]) -> [f64; 3] {
        let b = &self.bodies[id];
        let (dx, dy, dz) = (point[0] - b.px, point[1] - b.py, point[2] - b.pz);
        [b.r00 * dx + b.r10 * dy + b.r20 * dz, b.r01 * dx + b.r11 * dy + b.r21 * dz, b.r02 * dx + b.r12 * dy + b.r22 * dz]
    }
    /// The prop's rotation as a quaternion [x, y, z, w].
    pub fn rotation(&self, id: usize) -> [f64; 4] {
        let b = &self.bodies[id];
        [b.qx, b.qy, b.qz, b.qw]
    }
    /// The direction the prop's own up axis points in the world.
    pub fn up(&self, id: usize) -> [f64; 3] {
        let b = &self.bodies[id];
        [b.r01, b.r11, b.r21]
    }
    pub fn velocity(&self, id: usize) -> [f64; 3] {
        let b = &self.bodies[id];
        [b.vx, b.vy, b.vz]
    }
    pub fn angular_velocity(&self, id: usize) -> [f64; 3] {
        let b = &self.bodies[id];
        [b.wx, b.wy, b.wz]
    }
    /// Kinetic plus potential energy in joules, for tests and tuning.
    pub fn energy(&self, id: usize) -> f64 {
        let b = &self.bodies[id];
        let s = &self.shapes[b.shape];
        let i = &s.inertia;
        let (lx, ly, lz) = (b.r00 * b.wx + b.r10 * b.wy + b.r20 * b.wz, b.r01 * b.wx + b.r11 * b.wy + b.r21 * b.wz, b.r02 * b.wx + b.r12 * b.wy + b.r22 * b.wz);
        let spin = lx * (i[0] * lx + i[3] * ly + i[4] * lz) + ly * (i[3] * lx + i[1] * ly + i[5] * lz) + lz * (i[4] * lx + i[5] * ly + i[2] * lz);
        0.5 * s.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz) + 0.5 * spin + s.mass * self.gravity * b.py
    }
    pub fn get_state(&self, id: usize) -> PropState {
        let b = &self.bodies[id];
        PropState { id, shape: b.shape, state: b.state, holder: b.holder, position: self.position(id), rotation: self.rotation(id), velocity: self.velocity(id), angular_velocity: self.angular_velocity(id) }
    }
    pub fn set_state(&mut self, id: usize, s: &PropState) {
        let was = self.bodies[id].state;
        self.place(id, s.position, s.rotation);
        let kind = self.bodies[id].shape;
        let (b, shape) = (&mut self.bodies[id], &self.shapes[kind]);
        [b.vx, b.vy, b.vz] = s.velocity;
        [b.wx, b.wy, b.wz] = s.angular_velocity;
        b.state = s.state;
        b.holder = s.holder;
        b.sleep_time = 0.;
        if b.state == State::Locked {
            pin(b);
        } else if was == State::Locked {
            unpin(b, shape);
        }
    }

    // ----- moving props by hand

    /// Puts a prop somewhere else, keeping its state; things near its old and new place wake.
    pub fn set_pose(&mut self, id: usize, position: [f64; 3], rotation: [f64; 4]) {
        self.wake_near(id);
        self.place(id, position, rotation);
        self.wake_near(id);
        if matches!(self.bodies[id].state, State::Asleep | State::Locked) {
            self.wake(id);
        }
    }
    pub fn set_velocity(&mut self, id: usize, velocity: [f64; 3], angular_velocity: [f64; 3]) {
        if matches!(self.bodies[id].state, State::Asleep | State::Locked) {
            self.wake(id);
        }
        let b = &mut self.bodies[id];
        [b.vx, b.vy, b.vz] = velocity;
        [b.wx, b.wy, b.wz] = angular_velocity;
    }
    /// A holder picks the prop up: it stops colliding and goes wherever carry() puts it.
    pub fn pick_up(&mut self, id: usize, holder: i32) -> bool {
        if id >= self.bodies.len() || !present(self.bodies[id].state) {
            return false;
        }
        self.wake_near(id);
        let kind = self.bodies[id].shape;
        let (b, shape) = (&mut self.bodies[id], &self.shapes[kind]);
        if b.state == State::Locked {
            unpin(b, shape);
        }
        b.settle = false;
        b.state = State::Carried;
        b.holder = holder;
        b.sleep_time = 0.;
        (b.vx, b.vy, b.vz, b.wx, b.wy, b.wz) = (0., 0., 0., 0., 0., 0.);
        true
    }
    /// Moves a carried prop.
    pub fn carry(&mut self, id: usize, position: [f64; 3], rotation: [f64; 4]) {
        if self.bodies[id].state == State::Carried {
            self.place(id, position, rotation);
        }
    }
    /// Lets go of a carried prop; give it a velocity to throw it.
    pub fn drop_prop(&mut self, id: usize, velocity: [f64; 3], angular_velocity: [f64; 3]) -> bool {
        if id >= self.bodies.len() || self.bodies[id].state != State::Carried {
            return false;
        }
        let b = &mut self.bodies[id];
        b.state = State::Awake;
        b.holder = -1;
        b.sleep_time = 0.;
        b.stuck = 0.;
        b.settle = false;
        [b.vx, b.vy, b.vz] = velocity;
        [b.wx, b.wy, b.wz] = angular_velocity;
        true
    }

    // ----- hitting props

    /// A sudden hit: `impulse` in newton seconds at the world point `point` (the centre of mass when None).
    pub fn apply_impulse(&mut self, id: usize, impulse: [f64; 3], point: Option<[f64; 3]>) -> bool {
        if id >= self.bodies.len() || !colliding(self.bodies[id].state) {
            return false;
        }
        if self.bodies[id].state == State::Asleep {
            self.wake(id);
        }
        let b = &mut self.bodies[id];
        let [jx, jy, jz] = impulse;
        b.vx += jx * b.inv_mass;
        b.vy += jy * b.inv_mass;
        b.vz += jz * b.inv_mass;
        if let Some(point) = point {
            let (rx, ry, rz) = (point[0] - b.px, point[1] - b.py, point[2] - b.pz);
            let (tx, ty, tz) = (ry * jz - rz * jy, rz * jx - rx * jz, rx * jy - ry * jx);
            b.wx += b.ixx * tx + b.ixy * ty + b.ixz * tz;
            b.wy += b.ixy * tx + b.iyy * ty + b.iyz * tz;
            b.wz += b.ixz * tx + b.iyz * ty + b.izz * tz;
        }
        true
    }

    /// An explosion at `center`: every prop with a surface point within `radius` is thrown away from it. A prop
    /// touching the centre receives `impulse` newton seconds, fading linearly to nothing at the radius (a prop
    /// lighter than 0.35 kg receives less, in proportion, so that a pillow flies no faster than the bowl); `lift`
    /// (0.35 is a good default) tilts the throw upwards. The push lands on the prop's nearest point, so props
    /// tumble. Returns the props hit.
    pub fn blast(&mut self, center: [f64; 3], radius: f64, impulse: f64, lift: f64) -> Vec<usize> {
        let mut hit = Vec::new();
        for id in 0..self.bodies.len() {
            let b = &self.bodies[id];
            if !present(b.state) {
                continue;
            }
            let (cx, cy, cz, reach) = (b.px - center[0], b.py - center[1], b.pz - center[2], radius + b.radius);
            if cx * cx + cy * cy + cz * cz > reach * reach {
                continue;
            }
            let w = &b.world;
            let (mut best, mut at) = (f64::INFINITY, 0);
            for i in (0..w.len()).step_by(3) {
                let (dx, dy, dz) = (w[i] - center[0], w[i + 1] - center[1], w[i + 2] - center[2]);
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 < best {
                    best = d2;
                    at = i;
                }
            }
            let dist = best.sqrt();
            if dist >= radius {
                continue;
            }
            let (mut dx, mut dy, mut dz) = (cx, cy, cz);
            let len = (dx * dx + dy * dy + dz * dz).sqrt();
            if len > 1e-9 {
                dx /= len;
                dy /= len;
                dz /= len;
            } else {
                (dx, dy, dz) = (0., 1., 0.);
            }
            dy += lift;
            let mass = self.shapes[b.shape].mass;
            let share = if mass < BLAST_MASS { mass / BLAST_MASS } else { 1. };
            let scale = impulse * share * (1. - dist / radius) / (dx * dx + dy * dy + dz * dz).sqrt();
            let point = [w[at], w[at + 1], w[at + 2]];
            // A blast unlocks what it throws.
            if b.state == State::Locked {
                self.wake(id);
            }
            self.apply_impulse(id, [dx * scale, dy * scale, dz * scale], Some(point));
            hit.push(id);
        }
        hit
    }

    /// The first prop a shot from `origin` along the unit vector `direction` meets within `reach` metres.
    /// Carried props count; pass `ignore` to skip one prop.
    pub fn raycast(&self, origin: [f64; 3], direction: [f64; 3], reach: f64, ignore: Option<usize>) -> Option<Hit> {
        self.cast(origin, direction, reach, |b| (present(b.state) || b.state == State::Carried) && Some(b.id) != ignore)
    }
    /// `raycast`, skipping every prop in `skip` (a round's way through the light props it has already passed).
    pub fn raycast_past(&self, origin: [f64; 3], direction: [f64; 3], reach: f64, skip: &[usize]) -> Option<Hit> {
        self.cast(origin, direction, reach, |b| (present(b.state) || b.state == State::Carried) && !skip.contains(&b.id))
    }
    /// `raycast` against the solid props only (MATERIALS): the first that would stop a round.
    pub fn raycast_solid(&self, origin: [f64; 3], direction: [f64; 3], reach: f64) -> Option<Hit> {
        self.cast(origin, direction, reach, |b| (present(b.state) || b.state == State::Carried) && self.shapes[b.shape].material == Material::Solid)
    }
    /// What the prop `id` is made of (MATERIALS, by its shape's name).
    pub fn material(&self, id: usize) -> Material {
        self.shapes[self.bodies[id].shape].material
    }
    /// `raycast` against the given props only, whatever their state (a draped napkin's cloth is laid over the
    /// frozen props it covers this way).
    pub fn raycast_among(&self, origin: [f64; 3], direction: [f64; 3], reach: f64, only: &[usize]) -> Option<Hit> {
        self.cast(origin, direction, reach, |b| only.contains(&b.id))
    }
    /// `raycast` against the listed props only, those of them that take up room (a carried one counts): a few found
    /// near first, so that many rays cost little (feast.rs `placement`).
    pub fn raycast_only(&self, origin: [f64; 3], direction: [f64; 3], reach: f64, ids: &[usize]) -> Option<Hit> {
        self.cast_over(ids.iter().map(|id| &self.bodies[*id]).filter(|b| present(b.state) || b.state == State::Carried), origin, direction, reach)
    }
    fn cast(&self, origin: [f64; 3], direction: [f64; 3], reach: f64, keep: impl Fn(&Body) -> bool) -> Option<Hit> {
        self.cast_over(self.bodies.iter().filter(|b| keep(b)), origin, direction, reach)
    }
    fn cast_over<'a>(&'a self, bodies: impl Iterator<Item = &'a Body>, origin: [f64; 3], direction: [f64; 3], reach: f64) -> Option<Hit> {
        let ([ox, oy, oz], [dx, dy, dz]) = (origin, direction);
        let (mut near, mut found, mut fx, mut fy, mut fz) = (reach, NONE, 0., 0., 0.);
        for b in bodies {
            // The bounding sphere first.
            let (cx, cy, cz) = (b.px - ox, b.py - oy, b.pz - oz);
            let (along, radius) = (cx * dx + cy * dy + cz * dz, b.radius);
            let off = cx * cx + cy * cy + cz * cz - along * along;
            if off > radius * radius {
                continue;
            }
            let half = (radius * radius - off).sqrt();
            let (exit, mut enter) = (along + half, along - half);
            if exit < 0. || enter > near {
                continue;
            }
            if enter < 0. {
                enter = 0.;
            }
            // Into the prop's frame.
            let (lox, loy, loz) = (b.r00 * -cx + b.r10 * -cy + b.r20 * -cz, b.r01 * -cx + b.r11 * -cy + b.r21 * -cz, b.r02 * -cx + b.r12 * -cy + b.r22 * -cz);
            let (ldx, ldy, ldz) = (b.r00 * dx + b.r10 * dy + b.r20 * dz, b.r01 * dx + b.r11 * dy + b.r21 * dz, b.r02 * dx + b.r12 * dy + b.r22 * dz);
            for prim in &self.shapes[b.shape].prims {
                let (mut t, mut nx, mut ny, mut nz) = (f64::INFINITY, 0., 0., 0.);
                if !prim.lathe {
                    // Slabs of the box grown by its rounding; rounded corners are treated as square.
                    let (mut lo, mut hi, mut axis, mut sign, mut miss) = (0., f64::INFINITY, NONE, 0., false);
                    let (o, d, h) = ([lox - prim.cx, loy - prim.cy, loz - prim.cz], [ldx, ldy, ldz], [prim.hx + prim.r, prim.hy + prim.r, prim.hz + prim.r]);
                    for k in 0..3 {
                        if d[k].abs() < 1e-12 {
                            if o[k] < -h[k] || o[k] > h[k] {
                                miss = true;
                                break;
                            }
                            continue;
                        }
                        let (mut a, mut c, mut s) = ((-h[k] - o[k]) / d[k], (h[k] - o[k]) / d[k], -1.);
                        if a > c {
                            (a, c, s) = (c, a, 1.);
                        }
                        if a > lo {
                            lo = a;
                            axis = k;
                            sign = s;
                        }
                        if c < hi {
                            hi = c;
                        }
                        if lo > hi {
                            miss = true;
                            break;
                        }
                    }
                    if miss {
                        continue;
                    }
                    t = lo;
                    match axis {
                        0 => nx = sign,
                        1 => ny = sign,
                        2 => nz = sign,
                        _ => (nx, ny, nz) = (-ldx, -ldy, -ldz),
                    }
                } else {
                    // Sphere tracing: step by the distance to the shell until it is reached or left behind.
                    let mut s = enter;
                    for _ in 0..RAY_STEPS {
                        if !(s <= exit && s < near) {
                            break;
                        }
                        let [sd, snx, sny, snz, _] = distance(prim, lox + ldx * s, loy + ldy * s, loz + ldz * s, true);
                        if sd < RAY_HIT {
                            (t, nx, ny, nz) = (s, snx, sny, snz);
                            break;
                        }
                        s += sd;
                    }
                }
                if t < near {
                    near = t;
                    found = b.id;
                    fx = b.r00 * nx + b.r01 * ny + b.r02 * nz;
                    fy = b.r10 * nx + b.r11 * ny + b.r12 * nz;
                    fz = b.r20 * nx + b.r21 * ny + b.r22 * nz;
                }
            }
        }
        if found == NONE {
            return None;
        }
        Some(Hit { id: found, distance: near, point: [ox + dx * near, oy + dy * near, oz + dz * near], normal: [fx, fy, fz] })
    }
    /// raycast between two points.
    pub fn segment(&self, from: [f64; 3], to: [f64; 3], ignore: Option<usize>) -> Option<Hit> {
        let (dx, dy, dz) = (to[0] - from[0], to[1] - from[1], to[2] - from[2]);
        let len = (dx * dx + dy * dy + dz * dz).sqrt();
        if len > 1e-12 { self.raycast(from, [dx / len, dy / len, dz / len], len, ignore) } else { None }
    }

    // ----- characters

    /// Keeps a character out of the props and lets it push them. Call it once per character per tick, after the
    /// game has moved the character and after step(), so that the tick ends with the character outside every
    /// prop; `dt` is the length of the tick. Heavy props stop a character and light ones move ahead of it;
    /// landing on a prop and standing on it pass on the character's momentum and weight.
    pub fn resolve_character(&mut self, character: &Character, dt: f64) -> Resolved {
        let [mut x, mut y, mut z] = character.position;
        let [mut vx, mut vy, mut vz] = character.velocity;
        let (r, mass, cap) = (character.radius, character.mass, character.push_force * dt);
        // The body is a column of spheres a quarter of a radius apart: the dents between them are a hundredth of a radius deep.
        let span = character.height - 2. * r;
        let count = if span > 1e-9 { (4. * span / r - 1e-9).ceil() as usize + 1 } else { 1 };
        let gap = if count > 1 { span / (count - 1) as f64 } else { 0. };
        let (mut impulses, mut pushed) = (Vec::new(), Vec::<usize>::new());
        fn load(b: &mut Body, impulses: &mut Vec<Push>, j: [f64; 3], p: [f64; 3]) {
            let ([jx, jy, jz], [px, py, pz]) = (j, p);
            b.load[0] += jx;
            b.load[1] += jy;
            b.load[2] += jz;
            let (rx, ry, rz) = (px - b.px, py - b.py, pz - b.pz);
            b.load[3] += ry * jz - rz * jy;
            b.load[4] += rz * jx - rx * jz;
            b.load[5] += rx * jy - ry * jx;
            impulses.push(Push { id: b.id, point: p, impulse: j });
        }
        // How much speed the prop's surface gains per unit of impulse along n at the point.
        fn yielding(b: &Body, p: [f64; 3], n: [f64; 3]) -> f64 {
            give(b, p[0] - b.px, p[1] - b.py, p[2] - b.pz, n[0], n[1], n[2])
        }
        // Speed of the prop's surface at the point, along n.
        fn moving(b: &Body, p: [f64; 3], n: [f64; 3]) -> f64 {
            let (rx, ry, rz) = (p[0] - b.px, p[1] - b.py, p[2] - b.pz);
            (b.vx + (b.wy * rz - b.wz * ry)) * n[0] + (b.vy + (b.wz * rx - b.wx * rz)) * n[1] + (b.vz + (b.wx * ry - b.wy * rx)) * n[2]
        }

        // A fast character is taken along this tick's path in strides of half a radius, so that it cannot step
        // through a thin wall or drop through a thin floor.
        let (mut step_x, mut step_y, mut step_z, mut strides) = (vx * dt, vy * dt, vz * dt, 1usize);
        let travel = (step_x * step_x + step_y * step_y + step_z * step_z).sqrt();
        if travel > r / 2. {
            let wanted = (travel / (r / 2.)).ceil();
            strides = if wanted > 8. { 8 } else { wanted as usize };
            let n = strides as f64;
            step_x /= n;
            step_y /= n;
            step_z /= n;
            x -= step_x * (n - 1.);
            y -= step_y * (n - 1.);
            z -= step_z * (n - 1.);
        }
        let (mut support, mut height, mut s, mut sn) = (NONE, 0., [0.; 3], [0.; 3]);
        let mut knock: Option<[f64; 3]> = None;
        for stride in 0..strides {
            if stride > 0 {
                x += step_x;
                y += step_y;
                z += step_z;
            }
            for _ in 0..CHARACTER_PASSES {
                let mut moved = false;
                for id in 0..self.bodies.len() {
                    if !present(self.bodies[id].state) {
                        continue;
                    }
                    let Some(t) = self.touch(id, x, y, z, r, count, gap) else { continue };
                    let (mut nx, mut ny, mut nz, depth) = (t.nx, t.ny, t.nz, -t.d);
                    let mut p = [t.x - nx * (r - depth), t.y - ny * (r + t.d), t.z - nz * (r - depth)];
                    // The character's side is upright, like a tin: unless the prop is on its head it pushes, and
                    // is pushed, level. (A ball would wedge itself under the bowl's rim and lift it.) And it leans
                    // on the prop with its whole side, not with the one spot that touches: the push lands no
                    // higher than the prop's centre of mass, so that walking into the bowl's rim slides the bowl
                    // instead of tipping it.
                    let flat = (nx * nx + nz * nz).sqrt();
                    if ny > -SUPPORT_SLOPE {
                        if flat < 1e-6 {
                            continue;
                        }
                        nx /= flat;
                        ny = 0.;
                        nz /= flat;
                        if p[1] > self.bodies[id].py {
                            p[1] = self.bodies[id].py;
                        }
                    }
                    // A locked prop is not pushed: the character is stopped by it as by a block.
                    if !pushed.contains(&id) && self.bodies[id].state != State::Locked {
                        pushed.push(id);
                        let b = &mut self.bodies[id];
                        let (n, coming) = ([nx, ny, nz], moving(b, p, [nx, ny, nz]));
                        let (vn, k) = (vx * nx + vy * ny + vz * nz - coming, yielding(b, p, n));
                        if ny <= -SUPPORT_SLOPE {
                            // The prop is on the character's head and the character cannot give way: stop the prop.
                            let mut lift = depth * 0.2 / dt;
                            if lift > 0.5 {
                                lift = 0.5;
                            }
                            let j = (lift - vn) / k;
                            if j > 0. {
                                load(b, &mut impulses, [-nx * j, -ny * j, -nz * j], p);
                            }
                        } else if vn < 0. && coming > KNOCK_SPEED {
                            // The prop ran into the character: a collision, shared by mass. The character is knocked back.
                            let j = -vn / (1. / mass + k);
                            load(b, &mut impulses, [-nx * j, -ny * j, -nz * j], p);
                            let knock = knock.get_or_insert([0.; 3]);
                            knock[0] += nx * j;
                            knock[1] += ny * j;
                            knock[2] += nz * j;
                        } else if vn < 0. {
                            // The character walked into the prop: it pushes as hard as it can.
                            let mut j = -vn / (1. / mass + k);
                            if j > cap {
                                j = cap;
                            }
                            load(b, &mut impulses, [-nx * j, -ny * j, -nz * j], p);
                            vx += nx * j / mass;
                            vy += ny * j / mass;
                            vz += nz * j / mass;
                        }
                    }
                    if ny <= -SUPPORT_SLOPE {
                        continue;
                    }
                    // Out sideways, far enough that the sphere clears the surface.
                    let mut out = (depth + 1e-5) / flat;
                    if out > r {
                        out = r;
                    }
                    x += nx * out;
                    z += nz * out;
                    moved = true;
                }
                if !moved {
                    break;
                }
            }
            // What is under the feet: the highest surface there that is flat enough to stand on.
            support = NONE;
            for id in 0..self.bodies.len() {
                if !present(self.bodies[id].state) {
                    continue;
                }
                let Some((floor, normal)) = self.under(id, x, y, z, r) else { continue };
                if support != NONE && floor <= height {
                    continue;
                }
                (height, support, sn, s) = (floor, id, normal, [x, floor, z]);
            }
            // Standing on it (or sunk a little into it): the feet are lifted and the fall is over.
            if support != NONE && height >= y - SUPPORT_SKIN {
                if height > y {
                    y = height;
                }
                if step_y < 0. {
                    step_y = 0.;
                }
            }
        }
        let mut report = None;
        if support != NONE {
            let (b, standing) = (&mut self.bodies[support], height >= y - SUPPORT_SKIN);
            if standing && b.state == State::Locked {
                if vy < 0. {
                    vy = 0.;
                }
            } else if standing {
                let (vn, weight) = (vx * sn[0] + vy * sn[1] + vz * sn[2] - moving(b, s, sn), mass * self.gravity * dt);
                let j = if vn < 0. { -vn / (1. / mass + yielding(b, s, sn)) } else { 0. };
                // Landing passes on momentum; standing passes on weight.
                if j > weight || vy > 0.05 {
                    if j > 0. {
                        load(b, &mut impulses, [-sn[0] * j, -sn[1] * j, -sn[2] * j], s);
                    }
                } else {
                    load(b, &mut impulses, [0., -weight, 0.], s);
                }
                if vy < 0. {
                    vy = 0.;
                }
            }
            let (rx, ry, rz) = (s[0] - b.px, s[1] - b.py, s[2] - b.pz);
            let velocity = [b.vx + (b.wy * rz - b.wz * ry), b.vy + (b.wz * rx - b.wx * rz), b.vz + (b.wx * ry - b.wy * rx)];
            report = Some(Support { id: support, height, velocity, normal: sn, standing });
        }
        Resolved { position: [x, y, z], velocity: [vx, vy, vz], support: report, impulse: knock, impulses }
    }

    /// Forgets what characters have pushed since the last step; for a world that is only a copy of another.
    pub fn discard_loads(&mut self) {
        for b in &mut self.bodies {
            b.load = [0.; 6];
        }
    }

    /// The floor a prop offers under a character's feet: what the feet (a ball of radius r) rest on now, or
    /// else what a ball half that size meets when let down from a step above the feet (half, so that a wall
    /// the character leans on is not in its way). Returns the height the feet would stand at and the
    /// surface's normal; None when there is nothing there to stand on.
    fn under(&self, id: usize, x: f64, y: f64, z: f64, r: f64) -> Option<(f64, [f64; 3])> {
        let b = &self.bodies[id];
        let s = &self.shapes[b.shape];
        let (reach, dx, dz) = (s.radius + r, x - b.px, z - b.pz);
        if dx * dx + dz * dz > reach * reach {
            return None;
        }
        let local = |ry: f64| (b.r00 * dx + b.r10 * ry + b.r20 * dz, b.r01 * dx + b.r11 * ry + b.r21 * dz, b.r02 * dx + b.r12 * ry + b.r22 * dz);
        let world = |nx: f64, ny: f64, nz: f64| [b.r00 * nx + b.r01 * ny + b.r02 * nz, b.r10 * nx + b.r11 * ny + b.r12 * nz, b.r20 * nx + b.r21 * ny + b.r22 * nz];
        let mut found: Option<(f64, [f64; 3])> = None;
        let (lx, ly, lz) = local(y + r - b.py);
        for prim in &s.prims {
            let [sd, snx, sny, snz, _] = distance(prim, lx, ly, lz, true);
            let (d, ny) = (sd - r, b.r10 * snx + b.r11 * sny + b.r12 * snz);
            if ny < SUPPORT_SLOPE || d >= SUPPORT_SKIN || -d / ny > STEP_UP || found.is_some_and(|(floor, _)| y - d / ny <= floor) {
                continue;
            }
            found = Some((y - d / ny, world(snx, sny, snz)));
        }
        let probe = r / 2.;
        let (top, bottom) = (b.py + s.radius + probe, b.py - s.radius - probe);
        let mut cy = y + STEP_UP + probe;
        if cy > top {
            cy = top;
        }
        for i in 0..FOOT_STEPS {
            if cy < bottom {
                break;
            }
            let (lx, ly, lz) = local(cy - b.py);
            let (mut d, mut nx, mut ny, mut nz) = (f64::INFINITY, 0., 0., 0.);
            for prim in &s.prims {
                let [sd, snx, sny, snz, _] = distance(prim, lx, ly, lz, true);
                if sd < d {
                    (d, nx, ny, nz) = (sd, snx, sny, snz);
                }
            }
            d -= probe;
            if d < RAY_HIT {
                // Met at the very start, it is not below the feet but round them; too steep, it is no floor.
                let normal = world(nx, ny, nz);
                if (i == 0 && d < -RAY_HIT) || normal[1] < SUPPORT_SLOPE || found.is_some_and(|(floor, _)| cy - probe <= floor) {
                    break;
                }
                found = Some((cy - probe, normal));
                break;
            }
            cy -= d;
        }
        found
    }

    /// The character's spheres against one prop: the deepest contact that is not a floor under the feet.
    #[allow(clippy::too_many_arguments)]
    fn touch(&self, id: usize, x: f64, y: f64, z: f64, r: f64, count: usize, gap: f64) -> Option<Touch> {
        let b = &self.bodies[id];
        let s = &self.shapes[b.shape];
        let (reach, dx, dz) = (s.radius + r + SUPPORT_SKIN, x - b.px, z - b.pz);
        if dx * dx + dz * dz > reach * reach || y > b.py + reach || y + r * 2. + gap * ((count - 1) as f64) < b.py - reach {
            return None;
        }
        let (bx, pad) = (&s.aabb, r + SUPPORT_SKIN);
        let (mut found, mut best) = (None, 0.);
        for i in 0..count {
            let cy = y + r + gap * i as f64;
            let ry = cy - b.py;
            let (lx, ly, lz) = (b.r00 * dx + b.r10 * ry + b.r20 * dz, b.r01 * dx + b.r11 * ry + b.r21 * dz, b.r02 * dx + b.r12 * ry + b.r22 * dz);
            if lx < bx[0] - pad || ly < bx[1] - pad || lz < bx[2] - pad || lx > bx[3] + pad || ly > bx[4] + pad || lz > bx[5] + pad {
                continue;
            }
            for prim in &s.prims {
                let [sd, snx, sny, snz, _] = distance(prim, lx, ly, lz, true);
                let (d, ny) = (sd - r, b.r10 * snx + b.r11 * sny + b.r12 * snz);
                // A floor: under the feet, flat enough, and not so deep that climbing out would be a jump. A sphere
                // above the lowest that meets the top of something lower than a step (a napkin's top is above the
                // second) is over that floor too, not against a wall: pushed sideways off a top that faces up, it
                // was flung.
                if d >= 0. || (ny >= SUPPORT_SLOPE && gap * i as f64 - d / ny <= STEP_UP) {
                    continue;
                }
                if found.is_some() && d >= best {
                    continue;
                }
                best = d;
                found = Some(Touch { d, nx: b.r00 * snx + b.r01 * sny + b.r02 * snz, ny, nz: b.r20 * snx + b.r21 * sny + b.r22 * snz, x, y: cy, z });
            }
        }
        found
    }

    // ----- stepping

    /// Advances the props by `dt` seconds in whole 1/120 s substeps (a remainder carries over to the next call).
    /// `level` must be the level the world was made for. Returns what happened since the last step, hits and
    /// pick-ups in between included.
    pub fn step(&mut self, level: &dyn PropLevel, dt: f64) -> Vec<Event> {
        let [x0, z0, x1, z1] = self.bounds;
        let terrain = Terrain { level, x0, z0, x1, z1, floor_y: self.floor_y };
        self.footing();
        self.remainder += dt;
        let steps = (self.remainder / SUBSTEP + 1e-9).floor();
        self.remainder -= steps * SUBSTEP;
        if self.remainder < 1e-12 {
            self.remainder = 0.;
        }
        // A step too short for a substep leaves the characters' pushes waiting for one.
        if steps > 0. {
            self.take_loads(steps * SUBSTEP);
        }
        for _ in 0..steps as usize {
            self.substep(&terrain, SUBSTEP, 1. / steps);
        }
        if steps > 0. {
            self.rest(steps * SUBSTEP);
        }
        std::mem::take(&mut self.events)
    }

    fn place(&mut self, id: usize, position: [f64; 3], rotation: [f64; 4]) {
        let b = &mut self.bodies[id];
        let shape = &self.shapes[b.shape];
        let len = (rotation[0] * rotation[0] + rotation[1] * rotation[1] + rotation[2] * rotation[2] + rotation[3] * rotation[3]).sqrt();
        (b.qx, b.qy, b.qz, b.qw) = (rotation[0] / len, rotation[1] / len, rotation[2] / len, rotation[3] / len);
        (b.px, b.py, b.pz) = (0., 0., 0.);
        sync(b, shape);
        let o = &shape.origin;
        b.px = position[0] - (b.r00 * o[0] + b.r01 * o[1] + b.r02 * o[2]);
        b.py = position[1] - (b.r10 * o[0] + b.r11 * o[1] + b.r12 * o[2]);
        b.pz = position[2] - (b.r20 * o[0] + b.r21 * o[1] + b.r22 * o[2]);
        sync(b, shape);
        b.stuck = 0.;
        if b.state == State::Locked {
            pin(b);
        }
    }

    fn event(&mut self, kind: EventKind, id: usize) {
        self.events.push(Event { kind, id, other: 0, point: [0.; 3], normal: [0.; 3], speed: 0., impulse: 0. });
    }
    fn wake(&mut self, id: usize) {
        let kind = self.bodies[id].shape;
        let (b, shape) = (&mut self.bodies[id], &self.shapes[kind]);
        if b.state == State::Locked {
            unpin(b, shape);
        }
        b.state = State::Awake;
        b.sleep_time = 0.;
        b.stuck = 0.;
        self.event(EventKind::Wake, id);
    }
    fn wake_near(&mut self, id: usize) {
        for other in 0..self.bodies.len() {
            if other != id && self.bodies[other].state == State::Asleep && near(&self.bodies[id], &self.bodies[other], 0.01) {
                self.wake(other);
            }
        }
    }

    /// Takes up the characters' pushes for a step of `span` seconds. They act as forces, spread over the step's
    /// substeps. A sleeping prop stays asleep under the force (and torque) it fell asleep with, give or take a
    /// quarter and 0.24 N (0.012 N m), and wakes when the load changes.
    fn take_loads(&mut self, span: f64) {
        for id in 0..self.bodies.len() {
            let b = &self.bodies[id];
            if b.state == State::Asleep {
                let (l, q) = (&b.load, &b.rest);
                let (dx, dy, dz, ax, ay, az) = (l[0] / span - q[0], l[1] / span - q[1], l[2] / span - q[2], l[3] / span - q[3], l[4] / span - q[4], l[5] / span - q[5]);
                let (rest, turn) = ((q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt(), (q[3] * q[3] + q[4] * q[4] + q[5] * q[5]).sqrt());
                if (dx * dx + dy * dy + dz * dz).sqrt() > 0.25 * rest + 0.24 || (ax * ax + ay * ay + az * az).sqrt() > 0.25 * turn + 0.012 {
                    self.wake(id);
                }
            }
            let b = &mut self.bodies[id];
            b.last = b.load;
            b.load = [0.; 6];
        }
    }

    /// One substep of `h` seconds; `share` is the part of the step's character loads that falls in it.
    fn substep(&mut self, terrain: &Terrain, h: f64, share: f64) {
        self.substeps += 1;
        // A fast prop looks further ahead, so that it is caught at the surface instead of inside it.
        for b in &mut self.bodies {
            b.margin = MARGIN;
            if b.state != State::Awake {
                continue;
            }
            let mut ahead = h * ((b.vx * b.vx + b.vy * b.vy + b.vz * b.vz).sqrt() + (b.wx * b.wx + b.wy * b.wy + b.wz * b.wz).sqrt() * b.radius);
            if ahead > MARGIN_MAX {
                ahead = MARGIN_MAX;
            }
            b.margin += ahead;
        }
        self.spread();
        self.collide(terrain);

        // Closing speeds are measured before the forces act, so that weight alone never looks like an impact.
        self.prepare();
        for b in &mut self.bodies {
            if b.state != State::Awake {
                continue;
            }
            let s = &self.shapes[b.shape];
            let (m, slow, spin) = (s.inv_mass * share, 1. / (1. + h * s.linear_damping), 1. / (1. + h * s.angular_damping));
            let (ax, ay, az) = (b.last[3] * share, b.last[4] * share, b.last[5] * share);
            b.vx += b.last[0] * m;
            b.vy += b.last[1] * m - self.gravity * h;
            b.vz += b.last[2] * m;
            b.wx += b.ixx * ax + b.ixy * ay + b.ixz * az;
            b.wy += b.ixy * ax + b.iyy * ay + b.iyz * az;
            b.wz += b.ixz * ax + b.iyz * ay + b.izz * az;
            b.vx *= slow;
            b.vy *= slow;
            b.vz *= slow;
            b.wx *= spin;
            b.wy *= spin;
            b.wz *= spin;
        }
        self.warm();
        // Alternate sweeps run backwards, so a correction travels up a pile as fast as it travels down.
        for i in 0..SOLVE_ITERATIONS {
            self.solve(h, i % 2 == 1);
        }
        self.bounce();
        for i in 0..PUSH_ITERATIONS {
            self.separate(h, i % 2 == 1);
        }

        for b in &mut self.bodies {
            if b.state != State::Awake {
                continue;
            }
            let (v2, w2) = (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz, b.wx * b.wx + b.wy * b.wy + b.wz * b.wz);
            if v2 > MAX_LINEAR * MAX_LINEAR {
                let k = MAX_LINEAR / v2.sqrt();
                b.vx *= k;
                b.vy *= k;
                b.vz *= k;
            }
            if w2 > MAX_ANGULAR * MAX_ANGULAR {
                let k = MAX_ANGULAR / w2.sqrt();
                b.wx *= k;
                b.wy *= k;
                b.wz *= k;
            }
            let (wx, wy, wz) = (b.wx + b.tx, b.wy + b.ty, b.wz + b.tz);
            b.px += (b.vx + b.sx) * h;
            b.py += (b.vy + b.sy) * h;
            b.pz += (b.vz + b.sz) * h;
            (b.sx, b.sy, b.sz, b.tx, b.ty, b.tz) = (0., 0., 0., 0., 0., 0.);
            let (k, qx, qy, qz, qw) = (0.5 * h, b.qx, b.qy, b.qz, b.qw);
            let (nx, ny, nz, nw) = (qx + k * (wx * qw + wy * qz - wz * qy), qy + k * (wy * qw + wz * qx - wx * qz), qz + k * (wz * qw + wx * qy - wy * qx), qw - k * (wx * qx + wy * qy + wz * qz));
            let len = (nx * nx + ny * ny + nz * nz + nw * nw).sqrt();
            (b.qx, b.qy, b.qz, b.qw) = (nx / len, ny / len, nz / len, nw / len);
            let shape = &self.shapes[b.shape];
            sync(b, shape);
        }
        self.finish();
        // Off the counter and down to the kitchen floor: gone.
        for id in 0..self.bodies.len() {
            let b = &mut self.bodies[id];
            if b.state != State::Awake || b.py >= self.floor_y + b.radius {
                continue;
            }
            b.state = State::Lost;
            (b.vx, b.vy, b.vz, b.wx, b.wy, b.wz) = (0., 0., 0., 0., 0., 0.);
            self.event(EventKind::Lost, id);
        }
    }

    /// Wakes every sleeping prop that an awake prop touches, and whatever those touch in turn.
    fn spread(&mut self) {
        let mut queue = std::mem::take(&mut self.queue);
        queue.clear();
        queue.extend(self.bodies.iter().filter(|b| b.state == State::Awake).map(|b| b.id));
        let mut i = 0;
        while i < queue.len() {
            let a = queue[i];
            for b in 0..self.bodies.len() {
                if self.bodies[b].state != State::Asleep || !near(&self.bodies[a], &self.bodies[b], self.bodies[a].margin) {
                    continue;
                }
                if self.samples(a, b, 0, true) || self.samples(b, a, 0, true) {
                    self.wake(b);
                    queue.push(b);
                }
            }
            i += 1;
        }
        self.queue = queue;
    }

    fn collide(&mut self, terrain: &Terrain) {
        self.contacts.clear();
        // Locked props stand still, as the level's blocks do: an awake prop meets them, and their side of each
        // contact moves nothing (`pin`). They meet nothing themselves.
        let mut locked = std::mem::take(&mut self.locked);
        locked.clear();
        locked.extend(self.bodies.iter().filter(|b| b.state == State::Locked).map(|b| b.id));
        for i in 0..self.bodies.len() {
            if self.bodies[i].state != State::Awake {
                continue;
            }
            self.collide_ground(terrain, i);
            self.collide_blocks(i);
            for j in i + 1..self.bodies.len() {
                let (a, b) = (&self.bodies[i], &self.bodies[j]);
                if b.state != State::Awake || !near(a, b, a.margin + b.margin - MARGIN) {
                    continue;
                }
                self.samples(i, j, (3 * 65536 + i as u64) * 65536 + j as u64, false);
                self.samples(j, i, (4 * 65536 + i as u64) * 65536 + j as u64, false);
            }
            for &j in &locked {
                let (a, b) = (&self.bodies[i], &self.bodies[j]);
                if !near(a, b, a.margin + b.margin - MARGIN) {
                    continue;
                }
                self.samples(i, j, (5 * 65536 + i as u64) * 65536 + j as u64, false);
                self.samples(j, i, (6 * 65536 + i as u64) * 65536 + j as u64, false);
            }
        }
        self.locked = locked;
    }

    #[allow(clippy::too_many_arguments)]
    fn contact(contacts: &mut Vec<Contact>, a: usize, b: usize, other: i32, key: u64, n: [f64; 3], sep: f64, p: [f64; 3], friction: f64, restitution: f64) {
        contacts.push(Contact { a, b, other, key, n, p, sep, friction, restitution, ..Contact::default() });
    }

    /// Sample spheres of `a` against the solids of `b`. With `probe`, only reports whether anything touches.
    fn samples(&mut self, ai: usize, bi: usize, pair: u64, probe: bool) -> bool {
        let first = self.contacts.len();
        let (a, b) = (&self.bodies[ai], &self.bodies[bi]);
        let (sa, sb) = (&self.shapes[a.shape], &self.shapes[b.shape]);
        let (w, bx, margin) = (&a.world, &sb.aabb, a.margin + b.margin - MARGIN);
        let reach = sb.radius + margin;
        let (friction, restitution) = ((sa.friction * sb.friction).sqrt(), if sa.restitution > sb.restitution { sa.restitution } else { sb.restitution });
        for s in 0..w.len() / 3 {
            let k = s * 3;
            let (rho, dx, dy, dz) = (sa.points[s * 4 + 3], w[k] - b.px, w[k + 1] - b.py, w[k + 2] - b.pz);
            let far = reach + rho;
            if dx * dx + dy * dy + dz * dz > far * far {
                continue;
            }
            let (lx, ly, lz, pad) = (b.r00 * dx + b.r10 * dy + b.r20 * dz, b.r01 * dx + b.r11 * dy + b.r21 * dz, b.r02 * dx + b.r12 * dy + b.r22 * dz, rho + margin);
            if lx < bx[0] - pad || ly < bx[1] - pad || lz < bx[2] - pad || lx > bx[3] + pad || ly > bx[4] + pad || lz > bx[5] + pad {
                continue;
            }
            for (p, prim) in sb.prims.iter().enumerate() {
                let mut found = distance(prim, lx, ly, lz, false);
                let mut sep = found[0] - rho;
                if sep >= margin {
                    continue;
                }
                if probe {
                    return true;
                }
                let key = (pair * 4096 + s as u64) * 64 + p as u64;
                let side = sided(&mut self.cache, self.substeps, ai, bi, key, prim, rho, [lx, ly, lz], &mut found, &mut sep);
                let [_, snx, sny, snz, _] = found;
                let (mut nx, mut ny, mut nz) = (b.r00 * snx + b.r01 * sny + b.r02 * snz, b.r10 * snx + b.r11 * sny + b.r12 * snz, b.r20 * snx + b.r21 * sny + b.r22 * snz);
                if sep < -TANGLED {
                    // Too deep for the surface to say which way is out (the wall of the bowl may be right through
                    // a crate): part the two props along the line between their centres.
                    let (cx, cy, cz) = (a.px - b.px, a.py - b.py, a.pz - b.pz);
                    let apart = (cx * cx + cy * cy + cz * cz).sqrt();
                    (nx, ny, nz) = if apart > 1e-9 { (cx / apart, cy / apart, cz / apart) } else { (0., 1., 0.) };
                }
                let back = rho + sep / 2.;
                Self::contact(&mut self.contacts, ai, bi, bi as i32, key, [nx, ny, nz], sep, [w[k] - nx * back, w[k + 1] - ny * back, w[k + 2] - nz * back], friction, restitution);
                self.contacts[first..].last_mut().unwrap().side = side;
            }
        }
        self.reduce(first);
        false
    }

    /// Thins the contacts made since `first` (one pair of things) to a handful per patch. A patch is a set of
    /// contacts facing the same way, such as one face resting on another, and either all touching or all still
    /// apart; of each patch the four that span the widest area stay, plus the deepest. Fewer, well-spread contacts
    /// are what lets piles settle; keeping touching contacts apart from approaching ones means a corner that is
    /// already in the ground is never passed over for a neighbour that is not.
    fn reduce(&mut self, first: usize) {
        let n = self.contacts.len() - first;
        if n <= 4 {
            return;
        }
        let (mut group, mut keep) = (std::mem::take(&mut self.group), std::mem::take(&mut self.keep));
        group.clear();
        keep.clear();
        let contacts = &mut self.contacts;
        let mut normals = [0.; 3 * GROUPS];
        let mut groups = 0;
        for i in 0..n {
            let c = &contacts[first + i];
            let mut g = 0;
            while g < groups {
                if c.n[0] * normals[g * 3] + c.n[1] * normals[g * 3 + 1] + c.n[2] * normals[g * 3 + 2] > SAME_FACING {
                    break;
                }
                g += 1;
            }
            if g == groups {
                if groups == GROUPS {
                    g = GROUPS - 1;
                } else {
                    (normals[g * 3], normals[g * 3 + 1], normals[g * 3 + 2]) = (c.n[0], c.n[1], c.n[2]);
                    groups += 1;
                }
            }
            group.push(if c.sep < TOUCH { g * 2 } else { g * 2 + 1 });
            keep.push(false);
        }
        for g in 0..groups * 2 {
            let facing = g / 2;
            let (nx, ny, nz) = (normals[facing * 3], normals[facing * 3 + 1], normals[facing * 3 + 2]);
            // The first of the patch, the one furthest from it, and the ones furthest to either side of that line.
            let (mut i1, mut i2, mut i3, mut i4, mut deepest, mut size) = (NONE, NONE, NONE, NONE, NONE, 0);
            let (mut far, mut left, mut right) = (0., 0., 0.);
            for i in 0..n {
                if group[i] != g {
                    continue;
                }
                size += 1;
                if i1 == NONE {
                    i1 = i;
                }
                if deepest == NONE || contacts[first + i].sep < contacts[first + deepest].sep {
                    deepest = i;
                }
            }
            if size <= 4 {
                for i in 0..n {
                    if group[i] == g {
                        keep[i] = true;
                    }
                }
                continue;
            }
            let a = contacts[first + i1];
            for i in 0..n {
                if group[i] != g {
                    continue;
                }
                let c = &contacts[first + i];
                let (dx, dy, dz) = (c.p[0] - a.p[0], c.p[1] - a.p[1], c.p[2] - a.p[2]);
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 > far {
                    far = d2;
                    i2 = i;
                }
            }
            keep[i1] = true;
            keep[deepest] = true;
            if i2 == NONE {
                continue;
            }
            keep[i2] = true;
            let b = contacts[first + i2];
            let (ex, ey, ez) = (b.p[0] - a.p[0], b.p[1] - a.p[1], b.p[2] - a.p[2]);
            // (e x n) points to one side of the line from a to b.
            let (sx, sy, sz) = (ey * nz - ez * ny, ez * nx - ex * nz, ex * ny - ey * nx);
            for i in 0..n {
                if group[i] != g {
                    continue;
                }
                let c = &contacts[first + i];
                let side = (c.p[0] - a.p[0]) * sx + (c.p[1] - a.p[1]) * sy + (c.p[2] - a.p[2]) * sz;
                if side > left {
                    left = side;
                    i3 = i;
                }
                if side < right {
                    right = side;
                    i4 = i;
                }
            }
            if i3 != NONE {
                keep[i3] = true;
            }
            if i4 != NONE {
                keep[i4] = true;
            }
        }
        // Close the gaps, keeping the order.
        let mut at = first;
        for i in 0..n {
            if keep[i] {
                contacts[at] = contacts[first + i];
                at += 1;
            }
        }
        contacts.truncate(at);
        self.group = group;
        self.keep = keep;
    }

    fn collide_ground(&mut self, terrain: &Terrain, ai: usize) {
        let first = self.contacts.len();
        let (mut footing, mut floor) = (std::mem::take(&mut self.footing), std::mem::take(&mut self.floor));
        let a = &self.bodies[ai];
        let sa = &self.shapes[a.shape];
        let (w, radii, contacts) = (&a.world, &sa.points, &mut self.contacts);
        let (friction, restitution) = ((sa.friction * self.ground_friction).sqrt(), sa.restitution);
        let (pair, margin, cell, cols, rows) = (ai as u64 * 65536, a.margin, self.cell, self.cols, self.rows);
        let count = w.len() / 3;
        footing.clear();
        footing.resize(count, 0);
        floor.clear();
        floor.resize(count * 4, 0.);
        for s in 0..count {
            let (x, y, z, rho) = (w[s * 3], w[s * 3 + 1], w[s * 3 + 2], radii[s * 4 + 3]);
            let foot = y - rho;
            // The highest ground anywhere near: most samples are above it and need no further thought.
            let (ci, cj) = (((x - self.grid_x) / cell).floor(), ((z - self.grid_z) / cell).floor());
            if ci < 0. || cj < 0. || ci >= cols as f64 || cj >= rows as f64 {
                continue;
            }
            let high = self.high[cj as usize * cols + ci as usize];
            if foot - margin >= high {
                continue;
            }
            let (key, g) = ((pair * 4096 + s as u64) * 64, terrain.ground(x, z));
            let under = g - foot;
            footing[s] = 2;
            floor[s * 4] = g;
            if under > DEEP {
                // The centre is far below the surface: it is inside a deck, a ramp's flank or the counter's edge.
                // Look along the four axes for the nearest place where it would be in the open, and leave that way.
                let need = foot + 0.001;
                let (mut best, mut nx, mut ny, mut nz, mut sep) = (f64::INFINITY, 0., 1., 0., -under);
                for d in 0..4 {
                    let (ux, uz) = (if d == 0 { 1. } else if d == 1 { -1. } else { 0. }, if d == 2 { 1. } else if d == 3 { -1. } else { 0. });
                    for i in 1..=PROBE_STEPS {
                        let far = i as f64 * PROBE_STEP;
                        if far - PROBE_STEP >= best {
                            break;
                        }
                        if terrain.ground(x + ux * far, z + uz * far) > need {
                            continue;
                        }
                        let (mut lo, mut hi) = (far - PROBE_STEP, far);
                        for _ in 0..PROBE_BISECT {
                            let mid = (lo + hi) / 2.;
                            if terrain.ground(x + ux * mid, z + uz * mid) > need {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        if hi < best {
                            (best, nx, ny, nz, sep) = (hi, ux, 0., uz, -hi - rho);
                        }
                        break;
                    }
                }
                let back = rho + sep / 2.;
                Self::contact(contacts, ai, NONE, GROUND, key, [nx, ny, nz], sep, [x - nx * back, y - ny * back, z - nz * back], friction, restitution);
                continue;
            }
            if under > -margin {
                // On a floor: its tilt from the heights either side, ignoring any cliff next to the sample.
                let (xa, xb, za, zb) = (terrain.ground(x + SLOPE_EPS, z) - g, g - terrain.ground(x - SLOPE_EPS, z), terrain.ground(x, z + SLOPE_EPS) - g, g - terrain.ground(x, z - SLOPE_EPS));
                let limit = SLOPE_EPS * SLOPE_MAX;
                let (ok_xa, ok_xb, ok_za, ok_zb) = (xa <= limit && xa >= -limit, xb <= limit && xb >= -limit, za <= limit && za >= -limit, zb <= limit && zb >= -limit);
                let gx = if ok_xa && ok_xb { (xa + xb) / (2. * SLOPE_EPS) } else if ok_xa { xa / SLOPE_EPS } else if ok_xb { xb / SLOPE_EPS } else { 0. };
                let gz = if ok_za && ok_zb { (za + zb) / (2. * SLOPE_EPS) } else if ok_za { za / SLOPE_EPS } else if ok_zb { zb / SLOPE_EPS } else { 0. };
                let len = (gx * gx + 1. + gz * gz).sqrt();
                let (nx, ny, nz) = (-gx / len, 1. / len, -gz / len);
                let sep = (y - g) * ny - rho;
                if sep < margin {
                    let back = rho + sep / 2.;
                    Self::contact(contacts, ai, NONE, GROUND, key, [nx, ny, nz], sep, [x - nx * back, y - ny * back, z - nz * back], friction, restitution);
                    footing[s] = 1;
                    (floor[s * 4 + 1], floor[s * 4 + 2], floor[s * 4 + 3]) = (nx, ny, nz);
                }
            }
            if foot + DEEP >= high {
                continue;
            }
            // Beside something taller: a wall is where the ground steps up within the sample's reach.
            let reach = rho + margin;
            let steep = g + SLOPE_MAX * reach;
            let wall = if foot + DEEP > steep { foot + DEEP } else { steep };
            for d in 0..4u64 {
                let (ux, uz) = (if d == 0 { 1. } else if d == 1 { -1. } else { 0. }, if d == 2 { 1. } else if d == 3 { -1. } else { 0. });
                if terrain.ground(x + ux * reach, z + uz * reach) <= wall {
                    continue;
                }
                let (mut lo, mut hi) = (0., reach);
                for _ in 0..PROBE_BISECT {
                    let mid = (lo + hi) / 2.;
                    if terrain.ground(x + ux * mid, z + uz * mid) > wall {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                let sep = hi - rho;
                let back = rho + sep / 2.;
                if sep < margin {
                    Self::contact(contacts, ai, NONE, GROUND, key + 1 + d, [-ux, 0., -uz], sep, [x + ux * back, y, z + uz * back], friction, restitution);
                }
            }
        }
        // The ground's edge under a link that has one end on a floor and the other over a drop.
        for (l, link) in sa.links.as_chunks::<2>().0.iter().enumerate() {
            let (mut i, mut j) = (link[0], link[1]);
            if footing[j] == 1 && footing[i] == 2 {
                (i, j) = (j, i);
            } else if footing[i] != 1 || footing[j] != 2 {
                continue;
            }
            let level = floor[i * 4] - DEEP;
            if floor[j * 4] >= level {
                continue;
            }
            let (ax, ay, az) = (w[i * 3], w[i * 3 + 1], w[i * 3 + 2]);
            let (ex, ey, ez) = (w[j * 3] - ax, w[j * 3 + 1] - ay, w[j * 3 + 2] - az);
            let (mut lo, mut hi, mut g) = (0., 1., floor[i * 4]);
            for _ in 0..PROBE_BISECT {
                let mid = (lo + hi) / 2.;
                let here = terrain.ground(ax + ex * mid, az + ez * mid);
                if here >= level {
                    lo = mid;
                    g = here;
                } else {
                    hi = mid;
                }
            }
            let (x, y, z, rho) = (ax + ex * lo, ay + ey * lo, az + ez * lo, radii[i * 4 + 3]);
            let (nx, ny, nz) = (floor[i * 4 + 1], floor[i * 4 + 2], floor[i * 4 + 3]);
            let sep = (y - g) * ny - rho;
            let back = rho + sep / 2.;
            // Only a link lying on the edge counts: one that dives under it belongs to a prop leaning on the
            // edge, and its samples either side of the edge hold that.
            if sep < margin && sep > -DEEP {
                Self::contact(contacts, ai, NONE, GROUND, (pair * 4096 + l as u64) * 64 + 5, [nx, ny, nz], sep, [x - nx * back, y - ny * back, z - nz * back], friction, restitution);
            }
        }
        self.footing = footing;
        self.floor = floor;
        self.reduce(first);
    }

    fn collide_blocks(&mut self, ai: usize) {
        for n in 0..self.blocks.len() {
            let a = &self.bodies[ai];
            let sa = &self.shapes[a.shape];
            let (w, radii, block, contacts) = (&a.world, &sa.points, &self.blocks[n], &mut self.contacts);
            let (friction, restitution, margin) = ((sa.friction * self.ground_friction).sqrt(), sa.restitution, a.margin);
            let reach = sa.radius + margin;
            // Nearest point of the block to the prop's centre.
            let qx = if a.px < block.x0 { block.x0 - a.px } else if a.px > block.x1 { a.px - block.x1 } else { 0. };
            let qz = if a.pz < block.z0 { block.z0 - a.pz } else if a.pz > block.z1 { a.pz - block.z1 } else { 0. };
            let qy = if a.py > block.top { a.py - block.top } else { 0. };
            if qx * qx + qy * qy + qz * qz > reach * reach {
                continue;
            }
            // The prop's samples against the block.
            let mut first = contacts.len();
            let pair = (65536 + ai as u64) * 65536 + n as u64;
            for s in 0..w.len() / 3 {
                let (k, rho) = (s * 3, radii[s * 4 + 3]);
                let mut found = distance(&block.prim, w[k], w[k + 1], w[k + 2], false);
                let mut sep = found[0] - rho;
                if sep >= margin {
                    continue;
                }
                let key = (pair * 4096 + s as u64) * 64;
                let side = sided(&mut self.cache, self.substeps, ai, NONE, key, &block.prim, rho, [w[k], w[k + 1], w[k + 2]], &mut found, &mut sep);
                let ([_, snx, sny, snz, _], back) = (found, rho + sep / 2.);
                Self::contact(contacts, ai, NONE, BLOCK - n as i32, key, [snx, sny, snz], sep, [w[k] - snx * back, w[k + 1] - sny * back, w[k + 2] - snz * back], friction, restitution);
                contacts.last_mut().unwrap().side = side;
            }
            self.reduce(first);
            // The block's samples against the prop.
            let a = &self.bodies[ai];
            let sa = &self.shapes[a.shape];
            let (bx, block, contacts) = (&sa.aabb, &self.blocks[n], &mut self.contacts);
            first = contacts.len();
            let other = (2 * 65536 + ai as u64) * 65536 + n as u64;
            for (s, point) in block.points.as_chunks::<3>().0.iter().enumerate() {
                let (dx, dy, dz) = (point[0] - a.px, point[1] - a.py, point[2] - a.pz);
                if dx * dx + dy * dy + dz * dz > reach * reach {
                    continue;
                }
                let (lx, ly, lz) = (a.r00 * dx + a.r10 * dy + a.r20 * dz, a.r01 * dx + a.r11 * dy + a.r21 * dz, a.r02 * dx + a.r12 * dy + a.r22 * dz);
                if lx < bx[0] - margin || ly < bx[1] - margin || lz < bx[2] - margin || lx > bx[3] + margin || ly > bx[4] + margin || lz > bx[5] + margin {
                    continue;
                }
                for (p, prim) in sa.prims.iter().enumerate() {
                    let mut found = distance(prim, lx, ly, lz, false);
                    let mut sep = found[0];
                    if sep >= margin {
                        continue;
                    }
                    let key = (other * 4096 + s as u64) * 64 + p as u64;
                    let side = sided(&mut self.cache, self.substeps, ai, NONE, key, prim, 0., [lx, ly, lz], &mut found, &mut sep);
                    let [_, snx, sny, snz, _] = found;
                    // The solid's normal points at the block; the contact's points from the block to the prop.
                    let (nx, ny, nz, back) = (-(a.r00 * snx + a.r01 * sny + a.r02 * snz), -(a.r10 * snx + a.r11 * sny + a.r12 * snz), -(a.r20 * snx + a.r21 * sny + a.r22 * snz), sep / 2.);
                    Self::contact(contacts, ai, NONE, BLOCK - n as i32, key, [nx, ny, nz], sep, [point[0] + nx * back, point[1] + ny * back, point[2] + nz * back], friction, restitution);
                    contacts.last_mut().unwrap().side = side;
                }
            }
            self.reduce(first);
        }
    }

    /// Levers, effective masses, friction directions and closing speed for every contact.
    fn prepare(&mut self) {
        let bodies = &self.bodies;
        for c in &mut self.contacts {
            let (a, n) = (&bodies[c.a], c.n);
            let [nx, ny, nz] = n;
            // Two friction directions square to the normal.
            let t1 = if nx < 0.57735 && nx > -0.57735 {
                let k = 1. / (ny * ny + nz * nz).sqrt();
                [0., nz * k, -ny * k]
            } else {
                let k = 1. / (nx * nx + nz * nz).sqrt();
                [-nz * k, 0., nx * k]
            };
            let t2 = [ny * t1[2] - nz * t1[1], nz * t1[0] - nx * t1[2], nx * t1[1] - ny * t1[0]];
            (c.t1, c.t2) = (t1, t2);
            let r = [c.p[0] - a.px, c.p[1] - a.py, c.p[2] - a.pz];
            (c.arn, c.ar1, c.ar2) = (cross(r, n), cross(r, t1), cross(r, t2));
            (c.awn, c.aw1, c.aw2) = (spin(a, c.arn), spin(a, c.ar1), spin(a, c.ar2));
            let dot = |u: &[f64; 3], v: &[f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
            let (mut kn, mut k1, mut k2) = (a.inv_mass + dot(&c.arn, &c.awn), a.inv_mass + dot(&c.ar1, &c.aw1), a.inv_mass + dot(&c.ar2, &c.aw2));
            // Closing speed before any impulse, for restitution and impact events.
            let mut approach = along(a, &n, &c.arn);
            if c.b != NONE {
                let b = &bodies[c.b];
                let r = [c.p[0] - b.px, c.p[1] - b.py, c.p[2] - b.pz];
                (c.brn, c.br1, c.br2) = (cross(r, n), cross(r, t1), cross(r, t2));
                (c.bwn, c.bw1, c.bw2) = (spin(b, c.brn), spin(b, c.br1), spin(b, c.br2));
                kn += b.inv_mass + dot(&c.brn, &c.bwn);
                k1 += b.inv_mass + dot(&c.br1, &c.bw1);
                k2 += b.inv_mass + dot(&c.br2, &c.bw2);
                approach -= along(b, &n, &c.brn);
            }
            (c.mn, c.mt1, c.mt2) = (1. / kn, 1. / k1, 1. / k2);
            c.approach = approach;
        }
    }

    /// Warm start: begin the solve from last substep's impulses.
    fn warm(&mut self) {
        let bodies = &mut self.bodies;
        for c in &mut self.contacts {
            let Some(kept) = self.cache.get(&c.key) else {
                (c.jn, c.jt1, c.jt2) = (0., 0., 0.);
                continue;
            };
            // A contact caught while still apart remembered how fast it was closing, for the bounce on arrival.
            if kept.hit < c.approach {
                c.approach = kept.hit;
            }
            let (jn, j1, j2) = (kept.jn, kept.jt1, kept.jt2);
            (c.jn, c.jt1, c.jt2) = (jn, j1, j2);
            let (jx, jy, jz) = (c.n[0] * jn + c.t1[0] * j1 + c.t2[0] * j2, c.n[1] * jn + c.t1[1] * j1 + c.t2[1] * j2, c.n[2] * jn + c.t1[2] * j1 + c.t2[2] * j2);
            let a = &mut bodies[c.a];
            a.vx += jx * a.inv_mass;
            a.vy += jy * a.inv_mass;
            a.vz += jz * a.inv_mass;
            a.wx += c.awn[0] * jn + c.aw1[0] * j1 + c.aw2[0] * j2;
            a.wy += c.awn[1] * jn + c.aw1[1] * j1 + c.aw2[1] * j2;
            a.wz += c.awn[2] * jn + c.aw1[2] * j1 + c.aw2[2] * j2;
            if c.b != NONE {
                let b = &mut bodies[c.b];
                b.vx -= jx * b.inv_mass;
                b.vy -= jy * b.inv_mass;
                b.vz -= jz * b.inv_mass;
                b.wx -= c.bwn[0] * jn + c.bw1[0] * j1 + c.bw2[0] * j2;
                b.wy -= c.bwn[1] * jn + c.bw1[1] * j1 + c.bw2[1] * j2;
                b.wz -= c.bwn[2] * jn + c.bw1[2] * j1 + c.bw2[2] * j2;
            }
        }
    }

    /// One pass of sequential impulses over every contact: friction, then the normal.
    fn solve(&mut self, h: f64, backwards: bool) {
        let (bodies, count) = (&mut self.bodies, self.contacts.len());
        for k in 0..count {
            let c = &mut self.contacts[if backwards { count - 1 - k } else { k }];
            // Friction: cancel sliding, within the cone the normal impulse allows. Nothing to do for a contact that
            // carries no load and holds no friction.
            if c.jn != 0. || c.jt1 != 0. || c.jt2 != 0. {
                let a = &bodies[c.a];
                let (mut v1, mut v2) = (along(a, &c.t1, &c.ar1), along(a, &c.t2, &c.ar2));
                if c.b != NONE {
                    let b = &bodies[c.b];
                    v1 -= along(b, &c.t1, &c.br1);
                    v2 -= along(b, &c.t2, &c.br2);
                }
                let (mut j1, mut j2) = (c.jt1 - c.mt1 * v1, c.jt2 - c.mt2 * v2);
                let (grip, slide) = (c.friction * c.jn, j1 * j1 + j2 * j2);
                if slide > grip * grip {
                    let scale = grip / slide.sqrt();
                    j1 *= scale;
                    j2 *= scale;
                }
                let (d1, d2) = (j1 - c.jt1, j2 - c.jt2);
                (c.jt1, c.jt2) = (j1, j2);
                let (jx, jy, jz) = (c.t1[0] * d1 + c.t2[0] * d2, c.t1[1] * d1 + c.t2[1] * d2, c.t1[2] * d1 + c.t2[2] * d2);
                let a = &mut bodies[c.a];
                a.vx += jx * a.inv_mass;
                a.vy += jy * a.inv_mass;
                a.vz += jz * a.inv_mass;
                a.wx += c.aw1[0] * d1 + c.aw2[0] * d2;
                a.wy += c.aw1[1] * d1 + c.aw2[1] * d2;
                a.wz += c.aw1[2] * d1 + c.aw2[2] * d2;
                if c.b != NONE {
                    let b = &mut bodies[c.b];
                    b.vx -= jx * b.inv_mass;
                    b.vy -= jy * b.inv_mass;
                    b.vz -= jz * b.inv_mass;
                    b.wx -= c.bw1[0] * d1 + c.bw2[0] * d2;
                    b.wy -= c.bw1[1] * d1 + c.bw2[1] * d2;
                    b.wz -= c.bw1[2] * d1 + c.bw2[2] * d2;
                }
            }
            // The normal: surfaces that touch may not approach; a gap may close but not overshoot.
            let mut vn = along(&bodies[c.a], &c.n, &c.arn);
            if c.b != NONE {
                vn -= along(&bodies[c.b], &c.n, &c.brn);
            }
            let mut jn = c.jn - c.mn * (vn + if c.sep > 0. { c.sep / h } else { 0. });
            if jn < 0. {
                jn = 0.;
            }
            let dn = jn - c.jn;
            c.jn = jn;
            let a = &mut bodies[c.a];
            let la = dn * a.inv_mass;
            a.vx += c.n[0] * la;
            a.vy += c.n[1] * la;
            a.vz += c.n[2] * la;
            a.wx += c.awn[0] * dn;
            a.wy += c.awn[1] * dn;
            a.wz += c.awn[2] * dn;
            if c.b != NONE {
                let b = &mut bodies[c.b];
                let lb = dn * b.inv_mass;
                b.vx -= c.n[0] * lb;
                b.vy -= c.n[1] * lb;
                b.vz -= c.n[2] * lb;
                b.wx -= c.bwn[0] * dn;
                b.wy -= c.bwn[1] * dn;
                b.wz -= c.bwn[2] * dn;
            }
        }
    }

    /// One pass of pushing overlaps apart, on the bodies' discardable motion only.
    fn separate(&mut self, h: f64, backwards: bool) {
        let (bodies, count) = (&mut self.bodies, self.contacts.len());
        for k in 0..count {
            let c = &mut self.contacts[if backwards { count - 1 - k } else { k }];
            if c.sep >= -SLOP {
                continue;
            }
            let mut vn = along_push(&bodies[c.a], &c.n, &c.arn);
            if c.b != NONE {
                vn -= along_push(&bodies[c.b], &c.n, &c.brn);
            }
            let mut want = -PUSH_RATE * (c.sep + SLOP) / h;
            if want > MAX_PUSH {
                want = MAX_PUSH;
            }
            let mut jp = c.jp + c.mn * (want - vn);
            if jp < 0. {
                jp = 0.;
            }
            let d = jp - c.jp;
            c.jp = jp;
            let a = &mut bodies[c.a];
            let la = d * a.inv_mass;
            a.sx += c.n[0] * la;
            a.sy += c.n[1] * la;
            a.sz += c.n[2] * la;
            a.tx += c.awn[0] * d;
            a.ty += c.awn[1] * d;
            a.tz += c.awn[2] * d;
            if c.b != NONE {
                let b = &mut bodies[c.b];
                let lb = d * b.inv_mass;
                b.sx -= c.n[0] * lb;
                b.sy -= c.n[1] * lb;
                b.sz -= c.n[2] * lb;
                b.tx -= c.bwn[0] * d;
                b.ty -= c.bwn[1] * d;
                b.tz -= c.bwn[2] * d;
            }
        }
    }

    /// Restitution: contacts that closed fast enough get their approach speed back, scaled.
    fn bounce(&mut self) {
        let bodies = &mut self.bodies;
        for c in &mut self.contacts {
            if c.restitution == 0. || c.approach > -RESTITUTION_SPEED || c.jn == 0. || c.sep > TOUCH {
                continue;
            }
            let mut vn = along(&bodies[c.a], &c.n, &c.arn);
            if c.b != NONE {
                vn -= along(&bodies[c.b], &c.n, &c.brn);
            }
            let mut jn = c.jn - c.mn * (vn + c.restitution * c.approach);
            if jn < 0. {
                jn = 0.;
            }
            let dn = jn - c.jn;
            c.jn = jn;
            let a = &mut bodies[c.a];
            let la = dn * a.inv_mass;
            a.vx += c.n[0] * la;
            a.vy += c.n[1] * la;
            a.vz += c.n[2] * la;
            a.wx += c.awn[0] * dn;
            a.wy += c.awn[1] * dn;
            a.wz += c.awn[2] * dn;
            if c.b != NONE {
                let b = &mut bodies[c.b];
                let lb = dn * b.inv_mass;
                b.vx -= c.n[0] * lb;
                b.vy -= c.n[1] * lb;
                b.vz -= c.n[2] * lb;
                b.wx -= c.bwn[0] * dn;
                b.wy -= c.bwn[1] * dn;
                b.wz -= c.bwn[2] * dn;
            }
        }
    }

    /// Remembers the impulses for the next substep and reports the hard knocks.
    fn finish(&mut self) {
        let stamp = self.substeps;
        for c in &self.contacts {
            let kept = self.cache.entry(c.key).or_insert(Kept { jn: 0., jt1: 0., jt2: 0., hit: 0., side: 0, seen: 0, stamp: 0, a: c.a, b: c.b });
            (kept.jn, kept.jt1, kept.jt2, kept.stamp, kept.seen, kept.side) = (c.jn, c.jt1, c.jt2, stamp, stamp, c.side);
            kept.hit = if c.sep > TOUCH && c.jn > 0. { c.approach } else { 0. };
            if c.approach < -IMPACT_SPEED && c.jn > 0. && c.sep <= TOUCH {
                // One event per pair of things per step: the fastest approach, the summed impulse.
                let (id, other) = if c.b == NONE {
                    (c.a, c.other)
                } else if c.b < c.a {
                    (c.b, c.a as i32)
                } else {
                    (c.a, c.b as i32)
                };
                let found = self.events.iter().position(|e| e.kind == EventKind::Impact && e.id == id && e.other == other);
                let at = found.unwrap_or_else(|| {
                    self.events.push(Event { kind: EventKind::Impact, id, other, point: c.p, normal: c.n, speed: 0., impulse: 0. });
                    self.events.len() - 1
                });
                let event = &mut self.events[at];
                if -c.approach > event.speed {
                    event.speed = -c.approach;
                    event.point = c.p;
                    event.normal = c.n;
                }
                event.impulse += c.jn;
            }
        }
    }

    /// Sleep: props that touch form an island, and an island sleeps once all of it has been still for a while.
    fn rest(&mut self, dt: f64) {
        let shapes = &self.shapes;
        for b in &mut self.bodies {
            b.parent = b.id;
            if b.state != State::Awake {
                continue;
            }
            let still = b.vx * b.vx + b.vy * b.vy + b.vz * b.vz < SLEEP_LINEAR * SLEEP_LINEAR && b.wx * b.wx + b.wy * b.wy + b.wz * b.wz < SLEEP_ANGULAR * SLEEP_ANGULAR;
            b.sleep_time = if still { b.sleep_time + dt } else { 0. };
            let q = &b.anchor;
            let (dx, dy, dz, same) = (b.px - q[0], b.py - q[1], b.pz - q[2], b.qx * q[3] + b.qy * q[4] + b.qz * q[5] + b.qw * q[6]);
            if dx * dx + dy * dy + dz * dz > STUCK_MOVE * STUCK_MOVE || 1. - (if same < 0. { -same } else { same }) > STUCK_TURN {
                b.anchor = [b.px, b.py, b.pz, b.qx, b.qy, b.qz, b.qw];
                b.stuck = 0.;
            } else {
                b.stuck += dt;
            }
            // Ready to sleep at 1: still for long enough (its shape's nap), or stuck for long enough.
            let (calm, held) = (b.sleep_time / shapes[b.shape].nap, b.stuck / STUCK_TIME);
            b.quiet = if calm > held { calm } else { held };
        }
        fn root(bodies: &mut [Body], mut id: usize) -> usize {
            while bodies[id].parent != id {
                bodies[id].parent = bodies[bodies[id].parent].parent;
                id = bodies[id].parent;
            }
            id
        }
        let bodies = &mut self.bodies;
        for c in &self.contacts {
            if c.b == NONE || bodies[c.a].state != State::Awake || bodies[c.b].state != State::Awake {
                continue;
            }
            let (ra, rb) = (root(bodies, c.a), root(bodies, c.b));
            if ra == rb {
                continue;
            }
            let (low, high) = if ra < rb { (ra, rb) } else { (rb, ra) };
            bodies[high].parent = low;
            if bodies[high].quiet < bodies[low].quiet {
                bodies[low].quiet = bodies[high].quiet;
            }
        }
        let mut settled = Vec::new();
        for id in 0..bodies.len() {
            if bodies[id].state != State::Awake {
                continue;
            }
            let island = root(bodies, id);
            if bodies[island].quiet < 1. {
                continue;
            }
            let b = &mut bodies[id];
            b.state = State::Asleep;
            (b.vx, b.vy, b.vz, b.wx, b.wy, b.wz) = (0., 0., 0., 0., 0., 0.);
            b.rest = b.last.map(|impulse| impulse / dt);
            if b.settle {
                settled.push(id);
            }
            self.events.push(Event { kind: EventKind::Sleep, id, other: 0, point: [0.; 3], normal: [0.; 3], speed: 0., impulse: 0. });
        }
        // What was put down on the move locks where it has come to rest.
        for id in settled {
            self.lock(id);
        }
        // Forget the impulses of contacts that have gone, but keep those of sleeping props for when they wake.
        let (bodies, now) = (&self.bodies, self.substeps);
        self.cache.retain(|_, kept| kept.seen + CACHE_KEEP > now || bodies[kept.a].state == State::Asleep || (kept.b != NONE && bodies[kept.b].state == State::Asleep));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const FLOOR: f64 = 0.9025;
    // The props fall under the game's gravity, 2.8 m/s², as its characters do.
    const G: f64 = GRAVITY;
    const UPRIGHT: [f64; 4] = [0., 0., 0., 1.];
    const NO: [f64; 3] = [0.; 3];
    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

    fn flat() -> SimpleLevel {
        SimpleLevel::default()
    }
    fn world(level: &SimpleLevel) -> PropWorld {
        PropWorld::new(level, Config::default())
    }
    fn length(v: [f64; 3]) -> f64 {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }
    /// Steps the world, collecting events; `each` runs after every step.
    fn run_each(world: &mut PropWorld, level: &dyn PropLevel, ticks: usize, mut each: impl FnMut(&PropWorld, usize)) -> Vec<Event> {
        let mut events = Vec::new();
        for tick in 0..ticks {
            events.extend(world.step(level, TICK));
            each(world, tick);
        }
        events
    }
    fn run(world: &mut PropWorld, level: &dyn PropLevel, ticks: usize) -> Vec<Event> {
        run_each(world, level, ticks, |_, _| {})
    }
    fn settle(world: &mut PropWorld, level: &dyn PropLevel, limit: usize) -> usize {
        let mut ticks = 0;
        while world.bodies().iter().any(|b| b.state == State::Awake) && ticks < limit {
            world.step(level, TICK);
            ticks += 1;
        }
        ticks
    }
    fn count(events: &[Event], kind: EventKind) -> usize {
        events.iter().filter(|e| e.kind == kind).count()
    }
    /// A world with one prop resting on the counter, already asleep.
    fn resting(spec: &ShapeSpec, lift: f64) -> (SimpleLevel, PropWorld, usize) {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(spec);
        let id = world.add(shape, [0., FLOOR + lift, 0.], UPRIGHT);
        run(&mut world, &level, 90);
        assert_eq!(world.state(id), State::Asleep, "{} should be asleep before the test begins", spec.name);
        (level, world, id)
    }
    fn hotdog(position: [f64; 3]) -> Character {
        Character::new(position, NO, 0.03, 0.15)
    }
    /// Most tests place crates, pillows and napkins by their middles: the ready-made shape with its origin there.
    fn centred(mut spec: ShapeSpec) -> ShapeSpec {
        if let Part::Box { center, .. } = &mut spec.parts[0] {
            center[1] = 0.;
        }
        spec
    }
    fn centred_crate(size: f64, mass: f64) -> ShapeSpec {
        centred(ShapeSpec::crate_box(size, mass))
    }
    fn crate_box() -> ShapeSpec {
        centred_crate(0.1, 0.3)
    }

    // --- Independent geometry, so the tests do not judge the module by its own distance function.
    /// v rotated by the inverse of q.
    fn rotate_into(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
        let [x, y, z, w] = q;
        let (tx, ty, tz) = (2. * (z * v[1] - y * v[2]), 2. * (x * v[2] - z * v[0]), 2. * (y * v[0] - x * v[1]));
        [v[0] + w * tx + (z * ty - y * tz), v[1] + w * ty + (x * tz - z * tx), v[2] + w * tz + (y * tx - x * ty)]
    }
    /// Distance from a world point to the surface of a prop; negative inside a box.
    fn clearance(world: &PropWorld, id: usize, point: [f64; 3]) -> f64 {
        let origin = world.position(id);
        let local = rotate_into(world.rotation(id), [point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]]);
        let mut nearest = f64::INFINITY;
        for part in &world.shape(world.body(id).shape).spec.parts {
            match part {
                Part::Box { center, half, radius, .. } => {
                    let q: Vec<f64> = (0..3).map(|i| (local[i] - center[i]).abs() - (half[i] - radius)).collect();
                    let outside = (q[0].max(0.).powi(2) + q[1].max(0.).powi(2) + q[2].max(0.).powi(2)).sqrt();
                    nearest = nearest.min(outside + q[0].max(q[1]).max(q[2]).min(0.) - radius);
                }
                Part::Lathe { center, profile, radius, .. } => {
                    let p = [local[0] - center[0], local[1] - center[1], local[2] - center[2]];
                    let rho = p[0].hypot(p[2]);
                    for pair in profile.windows(2) {
                        let ([ar, ay], [br, by]) = (pair[0], pair[1]);
                        let (er, ey) = (br - ar, by - ay);
                        let t = (((rho - ar) * er + (p[1] - ay) * ey) / (er * er + ey * ey)).clamp(0., 1.);
                        nearest = nearest.min((rho - ar - er * t).hypot(p[1] - ay - ey * t) - radius);
                    }
                }
            }
        }
        nearest
    }
    /// How far a character's body is from a prop: negative when the character is inside it.
    fn character_gap(world: &PropWorld, id: usize, c: &Character) -> f64 {
        let (mut gap, mut h) = (f64::INFINITY, c.radius);
        // Everything above the feet's rounded underside, which is allowed to rest on things.
        while h <= c.height - c.radius + 1e-9 {
            gap = gap.min(clearance(world, id, [c.position[0], c.position[1] + h, c.position[2]]) - c.radius);
            h += 0.005;
        }
        gap
    }
    /// One game tick for a walking character: the game moves it, the props step, the props push it back out.
    fn walk(world: &mut PropWorld, level: &dyn PropLevel, c: &mut Character, velocity: [f64; 3]) -> Resolved {
        c.velocity = velocity;
        for i in 0..3 {
            c.position[i] += velocity[i] * TICK;
        }
        world.step(level, TICK);
        let result = world.resolve_character(c, TICK);
        c.position = result.position;
        result
    }

    // --- Shapes

    #[test]
    fn shapes_carry_sensible_mass_properties_about_their_centre_of_mass() {
        let crate_shape = Shape::new(&crate_box());
        assert_eq!(crate_shape.mass, 0.3);
        assert!((crate_shape.inertia[0] - 0.3 * 0.1 * 0.1 / 6.).abs() < 1e-12, "a cube has m s^2 / 6");
        assert_eq!(crate_shape.points.len() / 4, 26, "corners, edge middles and face centres");
        assert_eq!(crate_shape.links.len() / 2, 48);

        let bowl = Shape::new(&ShapeSpec::bowl());
        assert_eq!(bowl.mass, 0.35);
        assert!(bowl.inertia[1] > bowl.inertia[0], "a wide shell spins more easily about a diameter than about its axis");
        assert!(bowl.origin[1] < -0.03 && bowl.origin[1] > -0.07, "the centre of mass is a few centimetres above the base");
        assert_eq!(bowl.points.len() / 4, 108);

        let chair = Shape::new(&ShapeSpec::chair());
        assert!((chair.mass - 0.25).abs() < 1e-12);
        assert!(chair.inertia[5] != 0., "the chair back makes the inertia tensor lopsided");
        let ([a, _, c, d, e, f], [p, q, r, s, t, u]) = (chair.inertia, chair.inv_inertia);
        assert!((a * p + d * s + e * t - 1.).abs() < 1e-9 && (a * s + d * q + e * u).abs() < 1e-9 && (e * t + f * u + c * r - 1.).abs() < 1e-9);
        for k in 0..24 {
            let [c, s] = direction24(k);
            let angle = k as f64 * std::f64::consts::PI / 12.;
            assert!((c - angle.cos()).abs() < 1e-15 && (s - angle.sin()).abs() < 1e-15, "direction {k}");
        }
    }

    #[test]
    fn every_ready_made_shape_stands_on_its_origin() {
        for spec in crate::map::LOOSE.iter().map(|kind| ShapeSpec::named(kind).unwrap()) {
            let shape = Shape::new(&spec);
            assert!((shape.aabb[1] - shape.origin[1]).abs() < 1e-12, "{}: the underside is {} m from the origin", spec.name, shape.aabb[1] - shape.origin[1]);
            // Put on the counter at its origin, it stays there.
            let level = flat();
            let mut world = world(&level);
            let kind = world.add_shape(&spec);
            let id = world.add(kind, [0., FLOOR, 0.], yaw_rotation(0.7));
            assert!(settle(&mut world, &level, 600) < 600);
            assert!((world.position(id)[1] - FLOOR).abs() < 0.001 && world.up(id)[1] > 0.9999, "{} rests at {}", spec.name, world.position(id)[1] - FLOOR);
        }
    }

    // --- The bowl

    #[test]
    fn a_bowl_dropped_level_lands_rings_and_comes_to_rest_upright_and_asleep_without_gaining_energy() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&ShapeSpec::bowl());
        let id = world.add(shape, [0., FLOOR + 0.05, 0.], UPRIGHT);
        let (mut energy, mut rise, mut lowest) = (world.energy(id), 0f64, f64::INFINITY);
        let events = run_each(&mut world, &level, 300, |world, _| {
            let now = world.energy(id);
            rise = rise.max(now - energy);
            energy = now;
            lowest = lowest.min(world.position(id)[1]);
        });
        let impacts: Vec<&Event> = events.iter().filter(|e| e.kind == EventKind::Impact).collect();
        assert!(!impacts.is_empty() && impacts.len() <= 6, "expected a few clangs, got {}", impacts.len());
        assert_eq!(impacts[0].other, GROUND);
        assert!(impacts[0].speed > 0.42 && impacts[0].speed < 0.58, "a 5 cm drop lands at about 0.53 m/s, got {}", impacts[0].speed);
        assert!(impacts[0].impulse > 0.05, "the first impact carries a good part of the bowl's momentum");
        assert!(impacts.windows(2).all(|pair| pair[1].speed < pair[0].speed), "each bounce is weaker than the last");
        assert!(rise < 1e-9, "energy rose by {rise} J in one tick");
        assert!(lowest > FLOOR - 0.0006, "the bowl sank {} m into the counter", FLOOR - lowest);
        assert_eq!(world.state(id), State::Asleep);
        assert_eq!(count(&events, EventKind::Sleep), 1);
        let [x, y, z] = world.position(id);
        assert!((y - FLOOR).abs() < 0.0006 && x.abs() < 0.002 && z.abs() < 0.002, "rest position {x} {y} {z}");
        assert!(world.up(id)[1] > 0.99999, "upright");
        // Asleep means still: nothing changes however long we wait.
        let before = world.get_state(id);
        assert!(run(&mut world, &level, 600).is_empty());
        assert_eq!(world.get_state(id), before);
    }

    #[test]
    fn a_bowl_struck_low_slides_and_friction_stops_it() {
        let (level, mut world, id) = resting(&ShapeSpec::bowl(), 0.);
        world.apply_impulse(id, [0.2, 0., 0.], Some([-0.065, FLOOR + 0.004, 0.]));
        assert_eq!(world.state(id), State::Awake, "a hit wakes the bowl");
        let mut tilt = 1f64;
        run_each(&mut world, &level, 30, |world, _| tilt = tilt.min(world.up(id)[1]));
        assert!(world.velocity(id)[0] < 0.2, "friction is slowing it");
        settle(&mut world, &level, 1200);
        let [x, y, z] = world.position(id);
        // 0.2 N s on 0.35 kg is 0.57 m/s; friction 0.35 under the game's gravity stops that in about 16 cm.
        assert!(x > 0.13 && x < 0.2, "slid {x} m");
        assert!(z.abs() < 0.005 && (y - FLOOR).abs() < 0.001);
        assert!(tilt > 0.99 && world.up(id)[1] > 0.9999, "a low shove does not tip it");
        assert_eq!(world.state(id), State::Asleep);
    }

    #[test]
    fn a_moderate_blow_under_the_rim_rocks_the_bowl_and_a_hard_one_turns_it_over() {
        let (level, mut world, id) = resting(&ShapeSpec::bowl(), 0.);
        world.apply_impulse(id, [0., 0.1, 0.], Some([-0.155, FLOOR + 0.109, 0.]));
        let mut tilt = 1f64;
        run_each(&mut world, &level, 600, |world, _| tilt = tilt.min(world.up(id)[1]));
        assert!(tilt < 0.85 && tilt > 0.3, "it should rock well over but not past its rim: least upward component {tilt}");
        assert!(world.up(id)[1] > 0.9999, "back on its base");
        assert_eq!(world.state(id), State::Asleep);
        for blow in [0.18, 0.2] {
            let (level, mut world, id) = resting(&ShapeSpec::bowl(), 0.);
            world.apply_impulse(id, [0., blow, 0.], Some([-0.155, FLOOR + 0.109, 0.]));
            run(&mut world, &level, 600);
            assert!(world.up(id)[1] < -0.99, "after {blow} N s the bowl should lie upside down, up is {:?}", world.up(id));
            assert_eq!(world.state(id), State::Asleep);
            // Upside down it rests on its rim: the base is now 11 cm up.
            assert!((world.position(id)[1] - (FLOOR + 0.113)).abs() < 0.002);
        }
    }

    #[test]
    fn a_hotdog_walking_into_the_bowl_is_slowed_and_shoves_it_along() {
        let (level, mut world, id) = resting(&ShapeSpec::bowl(), 0.);
        let mut c = hotdog([-0.4, FLOOR, 0.]);
        let mut pushes = 0;
        for tick in 0..90 {
            pushes += walk(&mut world, &level, &mut c, [0.67, 0., 0.]).impulses.len();
            let gap = character_gap(&world, id, &c);
            assert!(gap > -3e-4, "tick {tick}: the hotdog is {} m inside the bowl", -gap);
        }
        let (free, bowl) = (-0.4 + 0.67 * 1.5, world.position(id)[0]);
        assert!(pushes > 20, "the bowl was pushed tick after tick");
        assert!(bowl > 0.3, "the bowl moved {bowl} m");
        assert!(c.position[0] < free - 0.05, "the hotdog was held back: {} against {free} unhindered", c.position[0]);
        assert!(c.position[0] > free - 0.3, "but not stopped: the bowl is light");
        assert!(world.velocity(id)[0] > 0.4 && world.velocity(id)[0] < 0.67, "the bowl runs ahead at {} m/s", world.velocity(id)[0]);
    }

    #[test]
    fn a_hotdog_standing_on_the_rim_tilts_the_bowl() {
        let (level, mut world, id) = resting(&ShapeSpec::bowl(), 0.);
        let mut c = hotdog([-0.155, FLOOR + 0.2, 0.]);
        let (mut fall, mut tilt, mut stood) = (0., 1f64, 0);
        for _ in 0..180 {
            // The game's own gravity for the character.
            fall -= G * TICK;
            c.velocity = [0., fall, 0.];
            c.position[1] = (c.position[1] + fall * TICK).max(FLOOR);
            world.step(&level, TICK);
            let result = world.resolve_character(&c, TICK);
            c.position = result.position;
            if result.support.is_some_and(|support| support.standing) || c.position[1] == FLOOR {
                fall = 0.;
            }
            if let Some(support) = result.support.filter(|support| support.standing) {
                stood += 1;
                assert_eq!(support.id, id);
            }
            tilt = tilt.min(world.up(id)[1]);
        }
        assert!(stood > 20, "the rim carried the hotdog");
        assert!(tilt < 0.9, "the bowl should tip towards the hotdog: least upward component {tilt}");
        assert!(world.up(id)[0] < -0.3, "and it tips to the side the hotdog stands on");
        assert!(c.position[1] < FLOOR + 0.06, "the rim went down under the weight");
    }

    #[test]
    fn a_hotdog_standing_in_the_middle_of_the_bowl_leaves_it_level_and_lets_it_sleep() {
        let (level, mut world, id) = resting(&ShapeSpec::bowl(), 0.);
        let mut c = hotdog([0., FLOOR + 0.008, 0.]);
        let (mut wakes, mut sleeps) = (0, 0);
        for _ in 0..240 {
            let events = world.step(&level, TICK);
            wakes += count(&events, EventKind::Wake);
            sleeps += count(&events, EventKind::Sleep);
            let result = world.resolve_character(&c, TICK);
            let support = result.support.filter(|support| support.standing).expect("the base of the bowl carries the hotdog");
            assert!((support.height - (FLOOR + 0.008)).abs() < 0.001);
            c.position = result.position;
        }
        assert_eq!(wakes, 1, "the new load wakes the bowl once");
        assert_eq!(sleeps, 1, "and it goes back to sleep under the steady load");
        assert_eq!(world.state(id), State::Asleep);
        assert!(world.up(id)[1] > 0.9999);
        // Stepping off is a change of load too.
        c.position = [1., FLOOR, 1.];
        world.step(&level, TICK);
        world.resolve_character(&c, TICK);
        assert!(world.step(&level, TICK).iter().any(|e| e.kind == EventKind::Wake && e.id == id));
    }

    // --- Characters are solid to props

    #[test]
    fn a_character_never_ends_a_tick_inside_a_prop_whatever_the_angle_and_speed_of_approach() {
        let anvil = ShapeSpec { name: "anvil", friction: 0.6, restitution: 0., linear_damping: 0.05, angular_damping: 0.1, spacing: 0.05, nap: SLEEP_TIME, parts: vec![part([0., 0., 0.], [0.08, 0.06, 0.05], 0.004, 20.)] };
        let cases = [(crate_box(), 0.05), (anvil, 0.06), (centred(ShapeSpec::pillow()), 0.02), (ShapeSpec::bowl(), 0.), (ShapeSpec::chair(), 0.)];
        let mut checks = 0;
        for (spec, lift) in &cases {
            for speed in [0.2, 0.67, 1.5, 3.] {
                for k in (0..24).step_by(3) {
                    for offset in [0., 0.045, 0.09] {
                        let level = flat();
                        let mut world = world(&level);
                        let shape = world.add_shape(spec);
                        let id = world.add(shape, [0., FLOOR + lift, 0.], [0., 0.19509032201612825, 0., 0.9807852804032304]);
                        run(&mut world, &level, 60);
                        // Start 30 cm away, aimed at (or just beside) the prop, and walk 60 cm.
                        let [dx, dz] = direction24(k);
                        let mut c = hotdog([-dx * 0.3 - dz * offset, FLOOR, -dz * 0.3 + dx * offset]);
                        for tick in 0..(0.6 / speed / TICK).ceil() as usize {
                            walk(&mut world, &level, &mut c, [dx * speed, 0., dz * speed]);
                            let gap = character_gap(&world, id, &c);
                            checks += 1;
                            assert!(gap > -3e-4, "{} at {speed} m/s, direction {k}, offset {offset}, tick {tick}: {} m inside", spec.name, -gap);
                        }
                        if spec.name == "anvil" {
                            assert!(world.position(id)[0].hypot(world.position(id)[2]) < 0.002, "a hotdog cannot shift 20 kg");
                        }
                    }
                }
            }
        }
        assert!(checks > 5000);
    }

    #[test]
    fn a_heavy_prop_stops_a_character_and_a_light_one_yields() {
        let travel = |spec: &ShapeSpec, lift: f64| {
            let level = flat();
            let mut world = world(&level);
            let shape = world.add_shape(spec);
            let id = world.add(shape, [0., FLOOR + lift, 0.], UPRIGHT);
            run(&mut world, &level, 60);
            let mut c = hotdog([-0.2, FLOOR, 0.]);
            for _ in 0..60 {
                walk(&mut world, &level, &mut c, [0.67, 0., 0.]);
            }
            (c.position[0], world.position(id)[0])
        };
        let (pillow, heavy) = (travel(&centred(ShapeSpec::pillow()), 0.02), travel(&centred_crate(0.1, 5.), 0.05));
        // Unhindered the hotdog would reach x = 0.47.
        assert!(pillow.0 > 0.35 && pillow.1 > 0.4, "a pillow hardly slows a hotdog: {pillow:?}");
        assert!(heavy.0 < -0.07 && heavy.1.abs() < 0.005, "a 5 kg crate stops it dead: {heavy:?}");
    }

    #[test]
    fn a_character_walking_along_a_wall_of_crates_slides_past_without_entering_them() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&centred_crate(0.1, 5.));
        let ids: Vec<usize> = (0..4).map(|i| world.add(shape, [i as f64 * 0.1, FLOOR + 0.05, 0.], UPRIGHT)).collect();
        run(&mut world, &level, 60);
        let mut c = hotdog([-0.02, FLOOR, 0.12]);
        for _ in 0..50 {
            // Pressing into the wall while moving along it.
            walk(&mut world, &level, &mut c, [0.6, 0., -0.3]);
            for &id in &ids {
                assert!(character_gap(&world, id, &c) > -3e-4);
            }
        }
        assert!(c.position[0] > 0.47, "it should slide the length of the wall and past its end, reached {}", c.position[0]);
        assert!(ids.iter().all(|&id| world.position(id)[2].abs() < 0.001), "the wall stands");
    }

    #[test]
    fn a_character_can_stand_on_a_box() {
        let (level, mut world, id) = resting(&crate_box(), 0.05);
        let mut c = hotdog([0.01, FLOOR + 0.25, -0.01]);
        let (mut fall, mut landed) = (0., None);
        for tick in 0..180 {
            fall -= G * TICK;
            c.velocity = [0., fall, 0.];
            c.position[1] += fall * TICK;
            world.step(&level, TICK);
            let result = world.resolve_character(&c, TICK);
            c.position = result.position;
            // Before it lands the box is already reported as what is under its feet.
            if landed.is_none() {
                assert_eq!(result.support.map(|support| support.id), Some(id));
            }
            if let Some(support) = result.support.filter(|support| support.standing) {
                landed.get_or_insert(tick);
                fall = 0.;
                assert_eq!(support.id, id);
                assert!((support.height - (FLOOR + 0.1)).abs() < 0.001, "support height {}", support.height - FLOOR);
                assert!(support.normal[1] > 0.99);
            } else {
                assert_eq!(landed, None, "the box stopped carrying the hotdog at tick {tick}");
            }
        }
        let landed = landed.expect("the hotdog landed");
        assert!(landed > 14 && landed < 24, "landed at tick {landed}");
        assert!((c.position[1] - (FLOOR + 0.1)).abs() < 0.001, "standing on the top face");
        let [x, y, z] = world.position(id);
        assert!(length([x, y - (FLOOR + 0.05), z]) < 0.002, "the box stayed put");
        assert_eq!(world.state(id), State::Asleep, "and sleeps under the standing load");
        // Walking off the edge ends the support.
        let mut supported = true;
        for _ in 0..40 {
            if !supported {
                break;
            }
            supported = walk(&mut world, &level, &mut c, [0.67, 0., 0.]).support.is_some_and(|support| support.standing);
        }
        assert!(!supported && c.position[0] > 0.05, "no support past the edge");
    }

    #[test]
    fn a_prop_that_lands_on_a_characters_head_is_held_up_not_passed_through() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let id = world.add(shape, [0., FLOOR + 0.4, 0.], UPRIGHT);
        let mut c = hotdog([0., FLOOR, 0.]);
        let mut lowest = f64::INFINITY;
        for _ in 0..40 {
            world.step(&level, TICK);
            c.position = world.resolve_character(&c, TICK).position;
            lowest = lowest.min(world.position(id)[1] - 0.05);
        }
        assert!(lowest > FLOOR + 0.13, "the crate's underside came down to {} m; the hotdog is 0.15 m tall", lowest - FLOOR);
        assert_eq!(c.position, [0., FLOOR, 0.], "the hotdog is not pushed into the counter");
    }

    // --- The level

    #[test]
    fn a_prop_pushed_off_the_counter_falls_on_a_parabola_and_is_lost_at_the_kitchen_floor() {
        let level = SimpleLevel { bounds: [-1., -1., 1., 1.], ..flat() };
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let id = world.add(shape, [0.8, FLOOR + 0.05, 0.], UPRIGHT);
        run(&mut world, &level, 60);
        world.apply_impulse(id, [0.6, 0., 0.], None);
        let (mut lost, mut airborne, mut samples) = (None, None, Vec::new());
        for tick in 0..480 {
            if world.step(&level, TICK).iter().any(|e| e.kind == EventKind::Lost && e.id == id) {
                lost = Some(tick);
                break;
            }
            let (b, v) = (world.body(id), world.velocity(id));
            // Clear of the counter's edge: nothing but gravity acts from here on.
            if airborne.is_none() && b.px > 1.12 {
                airborne = Some((tick, b.py, v[1], b.px, v[0]));
            }
            if let Some((start, ..)) = airborne {
                samples.push(((tick - start) as f64 * TICK, b.px, b.py));
            }
        }
        let (start, y0, vy, x0, vx) = airborne.expect("the crate left the counter");
        assert!(samples.len() > 15, "and fell for a while");
        for (t, x, y) in samples {
            // Semi-implicit Euler in 1/120 s steps falls g t h / 2 further than the exact parabola; damping is 0.05 /s.
            let ideal = y0 + vy * t - G * t * t / 2. - G * t / 240.;
            assert!((y - ideal).abs() < 0.004 + 0.02 * (ideal - y0).abs(), "at {t} s: y {y}, parabola {ideal}");
            assert!((x - (x0 + vx * t)).abs() < 0.01 + 0.03 * vx * t, "no sideways force in the air");
        }
        assert_eq!(world.state(id), State::Lost);
        let lost = lost.expect("a lost event was sent");
        let fall = (2. * (FLOOR - 0.05) / G).sqrt();
        assert!(((lost - start) as f64 * TICK - fall).abs() < 0.1, "the fall took {} s, free fall takes {fall} s", (lost - start) as f64 * TICK);
        // A lost prop is out of the simulation.
        let before = world.get_state(id);
        run(&mut world, &level, 30);
        assert_eq!(world.get_state(id), before);
        assert!(world.raycast([1.5, 2., 0.], [0., -1., 0.], 3., None).is_none());
    }

    #[test]
    fn a_prop_teetering_on_the_counters_edge_tips_over_it() {
        let level = SimpleLevel { bounds: [-1., -1., 1., 1.], ..flat() };
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let (safe, doomed) = (world.add(shape, [0.96, FLOOR + 0.05, -0.5], UPRIGHT), world.add(shape, [1.012, FLOOR + 0.05, 0.5], UPRIGHT));
        run(&mut world, &level, 240);
        assert_eq!(world.state(safe), State::Asleep, "four centimetres of overhang is fine");
        assert!((world.position(safe)[0] - 0.96).abs() < 0.001);
        assert_eq!(world.state(doomed), State::Lost, "with its centre past the edge it goes over");
    }

    #[test]
    fn a_prop_on_a_ramp_slides_down_it() {
        let level = SimpleLevel { ramps: vec![[0., -0.3, 0.6, 0.3, 0., 0.3, 0., 1.]], decks: vec![[0.6, -0.3, 1.2, 0.3, 0.3]], ..flat() };
        let mut world = world(&level);
        let (bowl_shape, pillow_shape) = (world.add_shape(&ShapeSpec::bowl()), world.add_shape(&centred(ShapeSpec::pillow())));
        let (bowl, pillow) = (world.add(bowl_shape, [0.4, FLOOR + 0.2 + 0.03, 0.], UPRIGHT), world.add(pillow_shape, [0.4, FLOOR + 0.2 + 0.05, 0.2], UPRIGHT));
        run(&mut world, &level, 60);
        assert!(world.velocity(bowl)[0] < -0.1 && world.velocity(bowl)[1] < -0.03, "the bowl should be sliding down the slope, velocity {:?}", world.velocity(bowl));
        assert!(world.up(bowl)[0] < -0.3, "tilted with the ramp");
        settle(&mut world, &level, 1200);
        let [x, y, _] = world.position(bowl);
        assert!(x < 0. && (y - FLOOR).abs() < 0.001, "the bowl ends on the flat below the ramp, at {x} {}", y - FLOOR);
        assert!(world.up(bowl)[1] > 0.9999);
        // Steel slides on a 27 degree ramp; a pillow's grip holds it there.
        let rest = world.position(pillow);
        assert!((rest[0] - 0.4).abs() < 0.03 && rest[1] > FLOOR + 0.15, "the pillow stays on the ramp, at {rest:?}");
        assert!((world.up(pillow)[1] - 2. / 5f64.sqrt()).abs() < 0.01, "lying on the slope");
    }

    #[test]
    fn a_prop_pushed_against_a_block_stops_at_the_block() {
        let level = SimpleLevel { blocks: vec![[0.3, -0.2, 0.6, 0.2, 0.25]], ..flat() };
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let id = world.add(shape, [0., FLOOR + 0.05, 0.02], UPRIGHT);
        run(&mut world, &level, 60);
        world.apply_impulse(id, [0.5, 0., 0.], None);
        let events = run(&mut world, &level, 300);
        let hit = events.iter().find(|e| e.kind == EventKind::Impact && e.other == BLOCK).expect("it hit the block");
        assert!(hit.speed > 0.3 && (hit.normal[0] + 1.).abs() < 1e-9, "on the block's face");
        let [x, y, z] = world.position(id);
        assert!(x > 0.235 && x < 0.2505, "the crate's face is at the block's, centre {x}");
        assert!((y - (FLOOR + 0.05)).abs() < 0.001 && (z - 0.02).abs() < 0.01);
        assert_eq!(world.state(id), State::Asleep);
        // A block's corner stops a wide flat prop whose own corners miss it.
        let level = SimpleLevel { blocks: vec![[0.3, -0.02, 0.33, 0.02, 0.25]], ..flat() };
        let mut world = self::world(&level);
        let shape = world.add_shape(&ShapeSpec::tray());
        let id = world.add(shape, [0., FLOOR, 0.], UPRIGHT);
        run(&mut world, &level, 60);
        world.apply_impulse(id, [0.4, 0., 0.], None);
        run(&mut world, &level, 300);
        assert!(world.position(id)[0] < 0.152, "the tray ran {} m into the post", world.position(id)[0] - 0.15);
    }

    #[test]
    fn a_prop_meets_the_side_of_a_deck_as_a_wall_and_drops_off_a_decks_edge() {
        let level = SimpleLevel { decks: vec![[0.6, -0.3, 1.2, 0.3, 0.3]], ..flat() };
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let (below, above) = (world.add(shape, [0.9, FLOOR + 0.05, -0.6], UPRIGHT), world.add(shape, [0.9, FLOOR + 0.35, 0.15], UPRIGHT));
        run(&mut world, &level, 60);
        assert!((world.position(above)[1] - (FLOOR + 0.35)).abs() < 0.001, "the deck carries a crate");
        world.apply_impulse(below, [0., 0., 0.5], None);
        world.apply_impulse(above, [0., 0., 0.4], None);
        settle(&mut world, &level, 1200);
        let (a, b) = (world.position(below), world.position(above));
        assert!(a[2] < -0.349 && a[2] > -0.37 && (a[1] - (FLOOR + 0.05)).abs() < 0.001, "the crate below stops at the deck's side, at {a:?}");
        assert!(b[2] > 0.35 && (b[1] - (FLOOR + 0.05)).abs() < 0.002, "the crate above falls to the counter, at {b:?}");
    }

    #[test]
    fn props_work_on_the_games_own_maps() {
        let level = crate::map::Level::new(crate::map::Map::bundled());
        let mut world = PropWorld::new(&level, Config::default());
        let (bowl, crate_shape) = (world.add_shape(&ShapeSpec::bowl()), world.add_shape(&crate_box()));
        assert_eq!(PropLevel::blocks(&level).len(), 4);
        let (on_counter, on_deck) = (world.add(bowl, [0., FLOOR, 3.2], UPRIGHT), world.add(crate_shape, [1.45, FLOOR + 0.28 + 0.05, 3.5], UPRIGHT));
        let on_ramp = world.add(bowl, [0.9, level.ground(0.9, 3.53) + 0.03, 3.53], UPRIGHT);
        let at_block = world.add(crate_shape, [-0.6, FLOOR + 0.05, 2.8], UPRIGHT);
        run(&mut world, &level, 30);
        world.apply_impulse(at_block, [0.5, 0., 0.], None);
        settle(&mut world, &level, 1200);
        assert!((world.position(on_counter)[1] - FLOOR).abs() < 0.001);
        assert!((world.position(on_deck)[1] - (FLOOR + 0.33)).abs() < 0.001, "the deck carries the crate");
        assert!(world.position(on_ramp)[0] < 0.6 && world.position(on_ramp)[1] < FLOOR + 0.03, "the bowl slid to the foot of the ramp, to {:?}", world.position(on_ramp));
        assert!(world.position(at_block)[0] < -0.499 && world.position(at_block)[0] > -0.53, "the crate stopped at the steel block, at {:?}", world.position(at_block));
    }

    // --- Piles

    #[test]
    fn five_stacked_boxes_stay_stacked_and_asleep_for_ten_seconds() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let mut ids = [0; 5];
        for height in [3, 0, 4, 1, 2] {
            ids[height] = world.add(shape, [0., FLOOR + 0.05 + height as f64 * 0.1, 0.], UPRIGHT);
        }
        let mut asleep_at = None;
        let events = run_each(&mut world, &level, 600, |world, tick| {
            if asleep_at.is_none() && ids.iter().all(|&id| world.state(id) == State::Asleep) {
                asleep_at = Some(tick);
            }
        });
        assert!(asleep_at.is_some_and(|tick| tick < 90), "the stack should be asleep within a second and a half, was at {asleep_at:?}");
        assert_eq!(count(&events, EventKind::Wake), 0, "nothing stirs afterwards");
        for (height, &id) in ids.iter().enumerate() {
            let [x, y, z] = world.position(id);
            let place = FLOOR + 0.05 + height as f64 * 0.1;
            assert!(x.abs() < 0.002 && z.abs() < 0.002, "box {height} drifted to {x} {z}");
            assert!((y - place).abs() < 0.0015, "box {height} is {} m from its place", y - place);
            assert!(world.up(id)[1] > 0.99999);
            assert_eq!(world.state(id), State::Asleep);
        }
    }

    #[test]
    fn boxes_dropped_one_onto_another_build_a_stack_that_settles() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let mut ids = Vec::new();
        for height in 0..6 {
            let h = height as f64;
            ids.push(world.add(shape, [0.003 * h, FLOOR + 0.05 + h * 0.1 + 0.03, 0.002 * h], UPRIGHT));
            run(&mut world, &level, 60);
        }
        settle(&mut world, &level, 1200);
        for (height, &id) in ids.iter().enumerate() {
            assert!((world.position(id)[1] - (FLOOR + 0.05 + height as f64 * 0.1)).abs() < 0.002, "box {height} at height {}", world.position(id)[1] - FLOOR);
            assert_eq!(world.state(id), State::Asleep);
        }
    }

    #[test]
    fn a_pile_of_ten_dropped_pillows_settles() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&centred(ShapeSpec::pillow()));
        let ids: Vec<usize> = (0..10)
            .map(|i| {
                let rotation = if i % 2 == 1 { [0., 0.3826834323650898, 0., 0.9238795325112867] } else { UPRIGHT };
                world.add(shape, [(i % 3 - 1) as f64 * 0.03, FLOOR + 0.1 + i as f64 * 0.07, (i * 7 % 5 - 2) as f64 * 0.02], rotation)
            })
            .collect();
        let ticks = settle(&mut world, &level, 900);
        assert!(ticks < 600, "the pile should be asleep within ten seconds, took {ticks} ticks");
        let mut stacked = 0;
        for &id in &ids {
            let [x, y, z] = world.position(id);
            assert_eq!(world.state(id), State::Asleep);
            assert!(y > FLOOR + 0.017 && y < FLOOR + 0.3, "pillow {id} rests at height {}", y - FLOOR);
            assert!(x.hypot(z) < 0.6, "the pile stays together");
            if y > FLOOR + 0.03 {
                stacked += 1;
            }
        }
        assert!(stacked >= 3, "some pillows should lie on others, {stacked} do");
        // No pillow is inside another: every sample point of each is clear of every other.
        for &a in &ids {
            for &b in &ids {
                if a == b {
                    continue;
                }
                for p in world.body(a).world.as_chunks::<3>().0 {
                    let gap = clearance(&world, b, [p[0], p[1], p[2]]) - 0.008;
                    assert!(gap > -0.012, "pillow {a} is {} m inside pillow {b}", -gap);
                }
            }
        }
        assert!(run(&mut world, &level, 300).is_empty(), "and it stays asleep");
    }

    /// The game's pillow is made to pile up: five let down one on another, each a centimetre off the one under it and
    /// a little turned, stand as a column (loose, not locked), asleep within a second and a half, none more than 2 cm
    /// off the bottom one, and stay so.
    #[test]
    fn five_loose_pillows_let_down_one_on_another_stand_as_a_column_and_soon_sleep() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&ShapeSpec::pillow());
        let ids: Vec<usize> = (0..5).map(|k| world.add(shape, [0.01 * (k % 2) as f64, FLOOR + 0.002 + 0.042 * k as f64, -0.008 * (k % 3) as f64], yaw_rotation(0.05 * k as f64))).collect();
        let ticks = settle(&mut world, &level, 600);
        assert!(ticks <= 90, "asleep after {ticks} ticks");
        let bottom = world.position(ids[0]);
        for (k, &id) in ids.iter().enumerate() {
            let at = world.position(id);
            assert!((at[0] - bottom[0]).hypot(at[2] - bottom[2]) < 0.02 && (at[1] - (FLOOR + 0.04 * k as f64)).abs() < 0.004 && world.up(id)[1] > 0.995, "pillow {k} at {at:?}, up {}", world.up(id)[1]);
            assert_eq!(world.state(id), State::Asleep);
        }
        assert!(run(&mut world, &level, 300).is_empty(), "and it stays asleep");
    }

    #[test]
    fn knocking_the_bottom_box_out_topples_the_stack() {
        let level = SimpleLevel { bounds: [-8., -8., 8., 8.], ..flat() };
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let ids: Vec<usize> = (0..5).map(|height| world.add(shape, [0., FLOOR + 0.05 + height as f64 * 0.1, 0.], UPRIGHT)).collect();
        settle(&mut world, &level, 1200);
        world.apply_impulse(ids[0], [1.2, 0., 0.], Some([-0.05, FLOOR + 0.03, 0.]));
        let events = run(&mut world, &level, 20);
        assert!(ids.iter().all(|&id| world.state(id) == State::Awake), "the whole stack wakes");
        assert_eq!(count(&events, EventKind::Wake), 5);
        settle(&mut world, &level, 1200);
        let heights: Vec<f64> = ids.iter().map(|&id| world.position(id)[1] - FLOOR).collect();
        assert!(heights.iter().all(|&h| h < 0.36), "the stack came down: heights {heights:?}");
        assert!(heights.iter().filter(|&&h| h < 0.06).count() >= 3, "most boxes end on the counter");
        assert!(world.position(ids[0])[0] > 0.3, "the bottom box shot out");
        assert!(ids.iter().all(|&id| world.state(id) == State::Asleep));
    }

    #[test]
    fn touching_a_sleeping_pile_wakes_all_of_it_and_only_it() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let pile: Vec<usize> = (0..3).map(|height| world.add(shape, [0., FLOOR + 0.05 + height as f64 * 0.1, 0.], UPRIGHT)).collect();
        let apart = world.add(shape, [1., FLOOR + 0.05, 1.], UPRIGHT);
        settle(&mut world, &level, 1200);
        let thrown = world.add(shape, [-0.4, FLOOR + 0.15, 0.], UPRIGHT);
        world.set_velocity(thrown, [2.5, 0.5, 0.], NO);
        let events = run(&mut world, &level, 30);
        let mut woken: Vec<usize> = events.iter().filter(|e| e.kind == EventKind::Wake).map(|e| e.id).collect();
        woken.sort();
        assert_eq!(woken, pile, "the three stacked boxes wake together");
        assert_eq!(world.state(apart), State::Asleep);
        assert!(events.iter().any(|e| e.kind == EventKind::Impact && e.id == pile[1] && e.other == thrown as i32), "with an impact between the thrown box and the one it hit");
    }

    #[test]
    fn a_chair_a_tray_and_a_napkin_come_to_rest_on_their_feet() {
        let level = flat();
        let mut world = world(&level);
        let shapes = [world.add_shape(&ShapeSpec::chair()), world.add_shape(&ShapeSpec::tray()), world.add_shape(&centred(ShapeSpec::napkin())), world.add_shape(&centred_crate(0.06, 0.1))];
        let (chair, tray, napkin) = (world.add(shapes[0], [0., FLOOR + 0.02, 0.], UPRIGHT), world.add(shapes[1], [0.5, FLOOR + 0.02, 0.], UPRIGHT), world.add(shapes[2], [-0.5, FLOOR + 0.03, 0.], UPRIGHT));
        let small = world.add(shapes[3], [0.5, FLOOR + 0.1, 0.], UPRIGHT);
        assert!(settle(&mut world, &level, 1200) < 240);
        assert!((world.position(chair)[1] - FLOOR).abs() < 0.001 && world.up(chair)[1] > 0.9999);
        assert!((world.position(tray)[1] - FLOOR).abs() < 0.001 && world.up(tray)[1] > 0.9999);
        assert!((world.position(napkin)[1] - (FLOOR + 0.004)).abs() < 0.001);
        assert!((world.position(small)[1] - (FLOOR + 0.008 + 0.03)).abs() < 0.001, "a crate dropped into the tray rests on its base");
        // The tray's walls hold the crate in when the tray is shoved.
        world.apply_impulse(tray, [0.25, 0., 0.], None);
        settle(&mut world, &level, 1200);
        assert!(world.position(tray)[0] > 0.52);
        assert!((world.position(small)[0] - world.position(tray)[0]).abs() < 0.12, "the crate rode along inside");
    }

    // --- Blasts and shots

    #[test]
    fn a_blast_throws_props_outward_harder_the_nearer_they_are() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let (near, far, out) = (world.add(shape, [0.15, FLOOR + 0.05, 0.], UPRIGHT), world.add(shape, [0., FLOOR + 0.05, -0.3], UPRIGHT), world.add(shape, [-0.6, FLOOR + 0.05, 0.], UPRIGHT));
        settle(&mut world, &level, 1200);
        assert_eq!(world.blast([0., FLOOR + 0.02, 0.], 0.4, 0.5, 0.35), vec![near, far]);
        let (a, b) = (world.velocity(near), world.velocity(far));
        assert!(a[0] > 0.5 && a[2].abs() < 0.05 && a[1] > 0., "the near crate flies away along +x and up: {a:?}");
        assert!(b[2] < -0.2 && b[0].abs() < 0.05 && b[1] > 0., "the far crate flies away along -z and up: {b:?}");
        assert!(length(a) > 1.8 * length(b), "falloff: {} m/s against {} m/s", length(a), length(b));
        // Impulse fades linearly with the distance to the nearest point of the prop: 0.1 m and 0.25 m from a 0.4 m blast.
        // A 0.3 kg crate, lighter than the 0.35 kg the blast is measured for, receives 0.3 / 0.35 of it.
        assert!((length(a) * 0.3 - 0.5 * (0.3 / 0.35) * (1. - 0.1038 / 0.4)).abs() < 0.03, "near impulse {}", length(a) * 0.3);
        assert!((length(b) * 0.3 - 0.5 * (0.3 / 0.35) * (1. - 0.2508 / 0.4)).abs() < 0.03, "far impulse {}", length(b) * 0.3);
        assert!(length(world.angular_velocity(near)) > 1., "an off-centre push sets it tumbling");
        assert_eq!(world.state(out), State::Asleep, "beyond the radius nothing happens");
        assert_eq!(world.velocity(out), NO);
        run(&mut world, &level, 120);
        assert!(world.position(near)[0] > 0.3 && world.position(far)[2] < -0.33);
    }

    #[test]
    fn a_shot_is_stopped_by_the_first_prop_in_its_way() {
        let level = flat();
        let mut world = world(&level);
        let (crate_shape, bowl) = (world.add_shape(&crate_box()), world.add_shape(&ShapeSpec::bowl()));
        let (first, second, vessel) = (world.add(crate_shape, [0.5, FLOOR + 0.05, 0.], UPRIGHT), world.add(crate_shape, [0.8, FLOOR + 0.05, 0.], UPRIGHT), world.add(bowl, [0., FLOOR, 0.6], UPRIGHT));
        settle(&mut world, &level, 1200);
        let muzzle = [0., FLOOR + 0.05, 0.];
        let hit = world.raycast(muzzle, [1., 0., 0.], 3., None).expect("the first crate is in the way");
        assert_eq!(hit.id, first);
        assert!((hit.distance - 0.45).abs() < 1e-3, "distance {}", hit.distance);
        assert!((hit.point[0] - 0.45).abs() < 1e-3 && (hit.normal[0] + 1.).abs() < 1e-9);
        assert_eq!(world.raycast(muzzle, [1., 0., 0.], 3., Some(first)).map(|h| h.id), Some(second), "ignoring the first shows the one behind");
        assert!(world.raycast(muzzle, [1., 0., 0.], 0.4, None).is_none(), "out of reach");
        assert!(world.raycast([0., FLOOR + 0.2, 0.], [1., 0., 0.], 3., None).is_none(), "over their heads");
        assert_eq!(world.segment(muzzle, [0.6, FLOOR + 0.05, 0.], None).map(|h| h.id), Some(first));
        assert!(world.segment(muzzle, [0.4, FLOOR + 0.05, 0.], None).is_none());
        let slant = world.raycast([0.9, FLOOR + 0.5, 0.02], [-std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2, 0.], 3., None).expect("a shot from above");
        assert_eq!(slant.id, first);
        assert!((slant.point[1] - (FLOOR + 0.1)).abs() < 1e-3 && (slant.normal[1] - 1.).abs() < 1e-9, "lands on the top face");
        // The bowl is hollow: a level shot at wall height hits the outside of its wall...
        let wall = world.raycast([0., FLOOR + 0.06, 0.], [0., 0., 1.], 3., None).expect("the bowl's wall");
        assert_eq!(wall.id, vessel);
        assert!(wall.distance > 0.6 - 0.16 && wall.distance < 0.6 - 0.12, "the wall is hit {} m away", wall.distance);
        assert!(wall.normal[2] < -0.3 && wall.normal[1] < 0., "on its outward, downward-leaning face");
        // ...a shot dropped down the middle reaches the base, and one skimming over the rim passes.
        let base = world.raycast([0., FLOOR + 0.5, 0.6], [0., -1., 0.], 3., None).expect("the bowl's base");
        assert!((base.point[1] - (FLOOR + 0.008)).abs() < 1e-3 && base.normal[1] > 0.999, "the base is at {}", base.point[1] - FLOOR);
        assert!(world.raycast([0., FLOOR + 0.13, 0.], [0., 0., 1.], 3., None).is_none());
        // A round that hits nudges what it hits.
        world.apply_impulse(wall.id, [0., 0., 0.02], Some(wall.point));
        assert_eq!(world.state(vessel), State::Awake);
        assert!(world.velocity(vessel)[2] > 0.03);
    }

    // --- Carrying

    #[test]
    fn a_prop_can_be_picked_up_carried_dropped_and_thrown() {
        let level = flat();
        let mut world = world(&level);
        let shape = world.add_shape(&crate_box());
        let (base, top, other) = (world.add(shape, [0., FLOOR + 0.05, 0.], UPRIGHT), world.add(shape, [0., FLOOR + 0.15, 0.], UPRIGHT), world.add(shape, [0.5, FLOOR + 0.05, 0.], UPRIGHT));
        settle(&mut world, &level, 1200);
        // Picking the bottom box up lets the top one fall.
        assert!(world.pick_up(base, 7));
        assert_eq!((world.state(base), world.body(base).holder), (State::Carried, 7));
        assert!(!world.pick_up(base, 8), "already held");
        assert_eq!(world.state(top), State::Awake, "what rested on it wakes");
        for tick in 0..60 {
            world.carry(base, [-0.5 + tick as f64 * 0.005, FLOOR + 0.2, 0.3], UPRIGHT);
            world.step(&level, TICK);
        }
        let held = world.position(base);
        assert!(length([held[0] + 0.205, held[1] - (FLOOR + 0.2), held[2] - 0.3]) < 1e-9, "a carried prop is where it was put, untouched by gravity");
        assert!((world.position(top)[1] - (FLOOR + 0.05)).abs() < 0.001, "the top box fell to the counter");
        assert!(!world.apply_impulse(base, [1., 0., 0.], None), "a carried prop takes no hits");
        // Carried through another prop without disturbing it.
        world.carry(base, [0.5, FLOOR + 0.05, 0.], UPRIGHT);
        run(&mut world, &level, 10);
        assert_eq!(world.state(other), State::Asleep);
        // Dropped, it falls and lands.
        world.carry(base, [-0.5, FLOOR + 0.3, 0.3], UPRIGHT);
        assert!(world.drop_prop(base, NO, NO));
        assert_eq!((world.state(base), world.body(base).holder), (State::Awake, -1));
        settle(&mut world, &level, 1200);
        assert!((world.position(base)[1] - (FLOOR + 0.05)).abs() < 0.001 && (world.position(base)[0] + 0.5).abs() < 0.01);
        // Thrown, it travels.
        world.pick_up(base, 7);
        world.carry(base, [-0.5, FLOOR + 0.2, 0.3], UPRIGHT);
        world.drop_prop(base, [1.5, 1., 0.], [0., 0., -3.]);
        run(&mut world, &level, 6);
        assert!(world.velocity(base)[0] > 1.4 && world.position(base)[1] > FLOOR + 0.22, "in flight");
        settle(&mut world, &level, 1200);
        assert!(world.position(base)[0] > 0., "landed at {:?}", world.position(base));
        assert!(!world.drop_prop(base, NO, NO), "not held any more");
    }

    #[test]
    fn dropping_onto_a_stack_removing_from_under_one_and_snapshots() {
        let level = flat();
        let mut world = world(&level);
        let (crate_shape, pillow) = (world.add_shape(&crate_box()), world.add_shape(&centred(ShapeSpec::pillow())));
        let ids = [world.add(crate_shape, [0., FLOOR + 0.05, 0.], UPRIGHT), world.add(crate_shape, [0., FLOOR + 0.15, 0.], UPRIGHT)];
        let held = world.add(pillow, [1., FLOOR + 0.02, 0.], UPRIGHT);
        settle(&mut world, &level, 1200);
        world.pick_up(held, 0);
        world.carry(held, [0., FLOOR + 0.25, 0.], UPRIGHT);
        world.drop_prop(held, NO, NO);
        settle(&mut world, &level, 1200);
        assert!((world.position(held)[1] - (FLOOR + 0.22)).abs() < 0.002, "the pillow lies on the stack, at {}", world.position(held)[1] - FLOOR);
        assert!(ids.iter().all(|&id| world.state(id) == State::Asleep));
        // A snapshot recreates the props elsewhere.
        let mut copy = self::world(&level);
        copy.add_shape(&crate_box());
        copy.add_shape(&centred(ShapeSpec::pillow()));
        for id in [ids[0], ids[1], held] {
            copy.add(world.body(id).shape, [0., 5., 0.], UPRIGHT);
            copy.set_state(id, &world.get_state(id));
            let (ours, theirs) = (world.get_state(id), copy.get_state(id));
            assert_eq!(ours.state, theirs.state);
            assert!((0..3).all(|i| (ours.position[i] - theirs.position[i]).abs() < 1e-12) && (0..4).all(|i| (ours.rotation[i] - theirs.rotation[i]).abs() < 1e-12));
        }
        // Removing a prop wakes what rested on it.
        world.remove(ids[0]);
        settle(&mut world, &level, 1200);
        assert!((world.position(ids[1])[1] - (FLOOR + 0.05)).abs() < 0.001);
        assert_eq!(world.raycast([-1., FLOOR + 0.05, 0.], [1., 0., 0.], 3., None).map(|h| h.id), Some(ids[1]));
    }

    #[test]
    fn any_tick_length_works_the_world_advances_in_whole_substeps_and_keeps_the_remainder() {
        // The server ticks 30 times a second, the game 60.
        let level = flat();
        let (mut run60, mut run30) = (world(&level), world(&level));
        let (shape60, shape30) = (run60.add_shape(&ShapeSpec::bowl()), run30.add_shape(&ShapeSpec::bowl()));
        let (a, b) = (run60.add(shape60, [0., FLOOR + 0.1, 0.], [0.1, 0., 0., 1.]), run30.add(shape30, [0., FLOOR + 0.1, 0.], [0.1, 0., 0., 1.]));
        for _ in 0..300 {
            run60.step(&level, 1. / 60.);
        }
        for _ in 0..150 {
            run30.step(&level, 1. / 30.);
        }
        assert_eq!((run60.substeps, run30.substeps), (600, 600));
        assert_eq!(run30.state(b), State::Asleep);
        assert!((0..3).all(|i| (run60.position(a)[i] - run30.position(b)[i]).abs() < 0.002), "the same motion at either rate");
        // Steps shorter than a substep add up.
        let mut slow = world(&level);
        for _ in 0..25 {
            slow.step(&level, 0.004);
        }
        assert_eq!(slow.substeps, 12);
        assert!(slow.remainder.abs() < 1e-9);
    }

    // --- The editor's shapes

    /// tools/network/presets.mjs is what the map editor draws the loose objects with; map.test.mjs holds it to
    /// fixtures/presets-golden.json, and this holds the game's shapes to the same record.
    #[test]
    fn the_loose_objects_shapes_are_the_ones_the_editor_draws() {
        let golden: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{ROOT}/tools/network/fixtures/presets-golden.json")).unwrap()).unwrap();
        let list = |v: &[f64]| serde_json::json!(v);
        for kind in crate::map::LOOSE {
            let spec = ShapeSpec::named(kind).unwrap();
            let parts: Vec<serde_json::Value> = spec
                .parts
                .iter()
                .map(|part| match part {
                    Part::Box { center, half, radius, mass } => serde_json::json!({"type": "box", "center": list(center), "half": list(half), "radius": radius, "mass": mass}),
                    Part::Lathe { center, profile, radius, mass } => serde_json::json!({"type": "lathe", "center": list(center), "profile": profile, "radius": radius, "mass": mass}),
                })
                .collect();
            let ours = serde_json::json!({"name": spec.name, "friction": spec.friction, "restitution": spec.restitution, "linearDamping": spec.linear_damping, "angularDamping": spec.angular_damping, "parts": parts});
            let theirs = &golden[kind];
            // (Numbers are compared as numbers: the record writes 0 where Rust has 0.0.)
            fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
                use serde_json::Value::*;
                match (a, b) {
                    (Number(x), Number(y)) => x.as_f64() == y.as_f64(),
                    (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
                    (Object(x), Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
                    _ => a == b,
                }
            }
            assert!(same(&ours, theirs), "{kind}: the game has {ours}, the editor {theirs}");
        }
    }

    // --- Determinism, and the record of the JavaScript physics this was written to match (props-golden.json)

    /// Just enough JSON to read the golden file. Numbers go through str::parse, which rounds correctly, so
    /// the scenario inputs are exactly the doubles the JavaScript wrote.
    #[derive(Clone, Debug)]
    enum Json {
        Null,
        Bool,
        Num(f64),
        Str(String),
        Arr(Vec<Json>),
        Obj(Vec<(String, Json)>),
    }
    impl Json {
        fn parse(text: &str) -> Json {
            let (bytes, mut at) = (text.as_bytes(), 0);
            let value = Json::value(bytes, &mut at);
            Json::space(bytes, &mut at);
            assert_eq!(at, bytes.len(), "trailing characters in JSON");
            value
        }
        fn space(s: &[u8], at: &mut usize) {
            while *at < s.len() && s[*at].is_ascii_whitespace() {
                *at += 1;
            }
        }
        fn value(s: &[u8], at: &mut usize) -> Json {
            Json::space(s, at);
            match s[*at] {
                b'{' => {
                    let mut fields = Vec::new();
                    *at += 1;
                    loop {
                        Json::space(s, at);
                        if s[*at] == b'}' {
                            *at += 1;
                            return Json::Obj(fields);
                        }
                        if s[*at] == b',' {
                            *at += 1;
                            continue;
                        }
                        let Json::Str(key) = Json::value(s, at) else { panic!("object keys are strings") };
                        Json::space(s, at);
                        assert_eq!(s[*at], b':');
                        *at += 1;
                        fields.push((key, Json::value(s, at)));
                    }
                }
                b'[' => {
                    let mut items = Vec::new();
                    *at += 1;
                    loop {
                        Json::space(s, at);
                        if s[*at] == b']' {
                            *at += 1;
                            return Json::Arr(items);
                        }
                        if s[*at] == b',' {
                            *at += 1;
                            continue;
                        }
                        items.push(Json::value(s, at));
                    }
                }
                b'"' => {
                    let mut out = Vec::new();
                    *at += 1;
                    while s[*at] != b'"' {
                        if s[*at] == b'\\' {
                            *at += 1;
                            out.push(match s[*at] {
                                b'n' => b'\n',
                                b't' => b'\t',
                                other => other,
                            });
                        } else {
                            out.push(s[*at]);
                        }
                        *at += 1;
                    }
                    *at += 1;
                    Json::Str(String::from_utf8(out).unwrap())
                }
                b't' => {
                    *at += 4;
                    Json::Bool
                }
                b'f' => {
                    *at += 5;
                    Json::Bool
                }
                b'n' => {
                    *at += 4;
                    Json::Null
                }
                _ => {
                    let start = *at;
                    while *at < s.len() && matches!(s[*at], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                        *at += 1;
                    }
                    Json::Num(std::str::from_utf8(&s[start..*at]).unwrap().parse().unwrap())
                }
            }
        }
        fn get(&self, key: &str) -> &Json {
            match self {
                Json::Obj(fields) => fields.iter().find(|(name, _)| name == key).map_or(&Json::Null, |(_, value)| value),
                _ => &Json::Null,
            }
        }
        fn list(&self) -> &[Json] {
            match self {
                Json::Arr(items) => items,
                _ => &[],
            }
        }
        fn num(&self) -> f64 {
            match self {
                Json::Num(n) => *n,
                other => panic!("expected a number, found {other:?}"),
            }
        }
        fn or(&self, fallback: f64) -> f64 {
            if let Json::Num(n) = self { *n } else { fallback }
        }
        fn text(&self) -> &str {
            match self {
                Json::Str(s) => s,
                other => panic!("expected a string, found {other:?}"),
            }
        }
        fn nums<const N: usize>(&self) -> [f64; N] {
            let items = self.list();
            assert_eq!(items.len(), N);
            std::array::from_fn(|i| items[i].num())
        }
        fn rows<const N: usize>(&self) -> Vec<[f64; N]> {
            self.list().iter().map(Json::nums).collect()
        }
    }

    struct Played {
        frames: Vec<Vec<f64>>,
        events: [usize; 4],
        states: Vec<usize>,
    }
    /// Plays a recorded scenario exactly as props.test.mjs does.
    fn play(scenario: &Json) -> Played {
        let described = scenario.get("level");
        let level = SimpleLevel {
            bounds: if let Json::Arr(_) = described.get("bounds") { described.get("bounds").nums() } else { [-2., -2., 2., 2.] },
            floor: FLOOR,
            blocks: described.get("blocks").rows(),
            decks: described.get("decks").rows(),
            ramps: described.get("ramps").rows(),
        };
        let mut world = world(&level);
        for shape in scenario.get("shapes").list() {
            world.add_shape(&match shape.get("preset").text() {
                "bowl" => ShapeSpec::bowl(),
                "crate" => centred_crate(shape.get("size").or(0.1), shape.get("mass").or(0.3)),
                "pillow" => centred(recorded_pillow()),
                "blanket" => centred(recorded_blanket()),
                "chair" => ShapeSpec::chair(),
                "tray" => ShapeSpec::tray(),
                other => panic!("unknown preset {other}"),
            });
        }
        let ids: Vec<usize> = scenario
            .get("props")
            .list()
            .iter()
            .map(|p| world.add(p.get("shape").num() as usize, p.get("position").nums(), if let Json::Arr(_) = p.get("rotation") { p.get("rotation").nums() } else { UPRIGHT }))
            .collect();
        let mut characters: Vec<(Character, usize, usize)> = scenario
            .get("characters")
            .list()
            .iter()
            .map(|c| (Character::new(c.get("position").nums(), c.get("velocity").nums(), c.get("radius").num(), c.get("height").num()), c.get("from").num() as usize, c.get("to").num() as usize))
            .collect();
        let mut played = Played { frames: Vec::new(), events: [0; 4], states: Vec::new() };
        for tick in 0..300 {
            for action in scenario.get("actions").list() {
                if action.get("tick").num() as usize != tick {
                    continue;
                }
                let id = action.get("id").or(0.) as usize;
                match action.get("type").text() {
                    "impulse" => {
                        let point = if let Json::Arr(_) = action.get("point") { Some(action.get("point").nums()) } else { None };
                        world.apply_impulse(id, action.get("impulse").nums(), point);
                    }
                    "blast" => {
                        world.blast(action.get("center").nums(), action.get("radius").num(), action.get("impulse").num(), 0.35);
                    }
                    "pickUp" => {
                        world.pick_up(id, action.get("holder").num() as i32);
                    }
                    "carry" => {
                        let rotation = world.rotation(id);
                        world.carry(id, action.get("position").nums(), rotation);
                    }
                    "drop" => {
                        world.drop_prop(id, action.get("velocity").nums(), NO);
                    }
                    other => panic!("unknown action {other}"),
                }
            }
            for (c, from, to) in &mut characters {
                if tick >= *from && tick < *to {
                    for i in 0..3 {
                        c.position[i] += c.velocity[i] * TICK;
                    }
                }
            }
            for event in world.step(&level, TICK) {
                played.events[event.kind as usize] += 1;
            }
            for (c, from, to) in &mut characters {
                let moving = Character { velocity: if tick >= *from && tick < *to { c.velocity } else { NO }, ..*c };
                c.position = world.resolve_character(&moving, TICK).position;
            }
            if tick % 10 == 9 {
                let mut frame = Vec::new();
                for &id in &ids {
                    frame.extend(world.position(id));
                    frame.extend(world.rotation(id));
                }
                for (c, ..) in &characters {
                    frame.extend(c.position);
                }
                played.frames.push(frame);
            }
        }
        played.states = ids.iter().map(|&id| world.state(id) as usize).collect();
        played
    }
    /// The pillow the JavaScript physics recorded its scenarios with (the game's is flatter and grips harder now).
    fn recorded_pillow() -> ShapeSpec {
        ShapeSpec {
            name: "pillow",
            friction: 0.9,
            restitution: 0.,
            linear_damping: 0.3,
            angular_damping: 1.5,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![part([0., 0.0225, 0.], [0.07, 0.0225, 0.05], 0.018, 0.04)],
        }
    }
    /// The blanket the JavaScript physics recorded its scenarios with: 64 cm by 48 cm and 140 g (the game's sheet is a
    /// napkin now, 45 cm square).
    fn recorded_blanket() -> ShapeSpec {
        ShapeSpec {
            name: "blanket",
            friction: 0.9,
            restitution: 0.,
            linear_damping: 0.3,
            angular_damping: 1.5,
            spacing: 0.05,
            nap: SLEEP_TIME,
            parts: vec![part([0., 0.008, 0.], [0.32, 0.008, 0.24], 0.007, 0.14)],
        }
    }
    fn golden() -> Json {
        let path = format!("{ROOT}/tools/network/fixtures/props-golden.json");
        Json::parse(&std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}")))
    }

    #[test]
    fn the_same_scenario_played_twice_gives_exactly_the_same_motion() {
        for scenario in golden().get("scenarios").list() {
            let (first, second) = (play(scenario), play(scenario));
            assert!(first.frames == second.frames && first.events == second.events && first.states == second.states, "{}", scenario.get("name").text());
        }
    }

    #[test]
    fn trajectories_match_the_golden_file_recorded_by_the_javascript_implementation() {
        let golden = golden();
        let tolerance = golden.get("tolerance").num();
        for row in golden.get("yaws").rows::<5>() {
            assert_eq!(yaw_rotation(row[0]), [row[1], row[2], row[3], row[4]], "the rotation for a yaw of {}", row[0]);
        }
        let scenarios = golden.get("scenarios").list();
        assert!(scenarios.len() >= 4);
        let mut worst = 0f64;
        for scenario in scenarios {
            let (name, played) = (scenario.get("name").text(), play(scenario));
            let expected = scenario.get("frames").list();
            assert_eq!(played.frames.len(), expected.len(), "{name}: frames");
            for (index, (frame, recorded)) in played.frames.iter().zip(expected).enumerate() {
                let recorded = recorded.list();
                assert_eq!(frame.len(), recorded.len(), "{name}: frame {index}");
                for (slot, (ours, theirs)) in frame.iter().zip(recorded).enumerate() {
                    let difference = (ours - theirs.num()).abs();
                    worst = worst.max(difference);
                    assert!(difference <= tolerance, "{name}: after {} ticks value {slot} is {ours}, the record has {}", (index + 1) * 10, theirs.num());
                }
            }
            let events = scenario.get("events");
            let recorded = [events.get("impact").num(), events.get("sleep").num(), events.get("wake").num(), events.get("lost").num()].map(|n| n as usize);
            assert_eq!(played.events, recorded, "{name}: impact, sleep, wake and lost events");
            let states: Vec<usize> = scenario.get("states").list().iter().map(|s| s.num() as usize).collect();
            assert_eq!(played.states, states, "{name}: final states");
        }
        println!("props parity: {} scenarios, largest difference from the record {worst:e} m (tolerance {tolerance:e})", scenarios.len());
    }

    // --- Cost

    #[test]
    fn a_tick_with_two_hundred_props_and_twelve_characters_fits_the_budget() {
        let level = SimpleLevel {
            bounds: [-1.8, 2.27, 1.8, 3.63],
            floor: FLOOR,
            blocks: vec![[-1.46, 3.1, -0.96, 3.43, 0.25], [1.29, 3.17, 1.66, 3.43, 0.25], [-0.45, 2.7, -0.23, 2.92, 0.25], [0.34, 2.45, 0.58, 2.64, 0.25]],
            decks: vec![[1.25, 3.16, 1.76, 3.61, 0.28]],
            ramps: vec![[0.55, 3.45, 1.25, 3.61, 0., 0.28, 0., 1.], [1.55, 2.46, 1.75, 3.16, 0., 0.28, 1., 1.]],
        };
        let mut world = world(&level);
        let (crate_shape, pillow, chair, bowl) = (world.add_shape(&crate_box()), world.add_shape(&centred(ShapeSpec::pillow())), world.add_shape(&ShapeSpec::chair()), world.add_shape(&ShapeSpec::bowl()));
        // Two forts of crates three high, rows of pillows and chairs, two bowls.
        for i in 0..20 {
            for k in 0..3 {
                world.add(crate_shape, [-1.5 + i as f64 * 0.11, FLOOR + 0.05 + k as f64 * 0.1, 2.5], UPRIGHT);
                world.add(crate_shape, [-1. + i as f64 * 0.11, FLOOR + 0.05 + k as f64 * 0.1, 3.], UPRIGHT);
            }
        }
        for i in 0..50 {
            world.add(pillow, [-1.6 + (i % 25) as f64 * 0.13, FLOOR + 0.03 + (i / 25) as f64 * 0.05, 2.75], UPRIGHT);
        }
        for i in 0..28 {
            world.add(chair, [-1.6 + i as f64 * 0.115, FLOOR + 0.001, 2.37], UPRIGHT);
        }
        world.add(bowl, [0., FLOOR, 3.3], UPRIGHT);
        world.add(bowl, [0.5, FLOOR, 3.3], UPRIGHT);
        assert_eq!(world.len(), 200);
        let milliseconds = |start: Instant| start.elapsed().as_secs_f64() * 1e3;
        // Everything awake at once: the worst case.
        let mut times: Vec<f64> = (0..30)
            .map(|_| {
                let start = Instant::now();
                world.step(&level, TICK);
                milliseconds(start)
            })
            .collect();
        times.sort_by(f64::total_cmp);
        assert!(settle(&mut world, &level, 600) < 600, "the scene settles");
        // Play: twelve characters run in circles on the open counter between the forts, knocking the bowls about
        // and brushing the pillows.
        let mut characters: Vec<(Character, usize)> = (0..12).map(|i| (hotdog([-0.9 + i as f64 * 0.16, FLOOR, 3.25 + (i % 3) as f64 * 0.1]), i * 2)).collect();
        let (mut playing, mut awake) = (Vec::new(), 0);
        for tick in 0..600 {
            let start = Instant::now();
            for (c, turn) in &mut characters {
                if tick % 4 == 0 {
                    *turn = (*turn + 1) % 24;
                }
                let [dx, dz] = direction24(*turn);
                c.velocity = [dx * 0.67, 0., dz * 0.67];
                for i in 0..3 {
                    c.position[i] += c.velocity[i] * TICK;
                }
            }
            world.step(&level, TICK);
            for (c, _) in &mut characters {
                c.position = world.resolve_character(c, TICK).position;
            }
            playing.push(milliseconds(start));
            awake += world.bodies().iter().filter(|b| b.state == State::Awake).count();
        }
        playing.sort_by(f64::total_cmp);
        println!(
            "props timing: in play ({} of 200 props awake on average) quartiles {:.3} / {:.3} / {:.3} ms per tick; all 200 awake {:.3} to {:.3} ms per tick",
            awake / 600,
            playing[150],
            playing[300],
            playing[450],
            times[0],
            times[15]
        );
        // The limits are generous and use the quicker ticks, because the machine running the tests may be busy,
        // and they mean nothing without optimisation. Typical figures are in the line above.
        assert!(awake / 600 < 100, "most props sleep through the commotion");
        if cfg!(debug_assertions) {
            return;
        }
        let slack = crate::timing_slack();
        assert!(playing[150] < 1. * slack, "a tick in play took {} ms", playing[150]);
        assert!(times[0] < 20. * slack, "a tick with everything awake took {} ms", times[0]);
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Zombie alley cats: the body of every zombie. A scruffy four-legged cat sculpted from primitives (a
//! torn ear, matted coats that differ from cat to cat, glowing eyes, a kinked tail, bared fangs,
//! ribs showing through one flank), about two thirds the height of a hotdog so it fits the
//! simulation's zombie. The primitives of each rigid piece are one mesh, its points coloured from a
//! palette, made once for each coat and shared by every cat that wears it, so that a horde of 128
//! is two thousand meshes drawn with a dozen materials, not twelve thousand with four hundred.
//!
//! Animation is procedural and driven by what the simulation says: a cat that is forming rises out
//! of its puddle as ooze and takes its colours last; a formed cat prowls with a four-beat walk that
//! follows the distance it covers, lunges and swipes at a hotdog in reach, flinches and spurts ooze
//! when hit, and melts back into ooze when it dies. Nothing here is hit-tested.
use crate::arena::Game;
use crate::ooze;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat};
use kitchen_sim::emergence::FORM;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const FLOOR: f32 = 0.9025;
/// The model is drawn in its own units and scaled to the zombie the simulation hit-tests: a sphere
/// 0.13 m across centred 0.075 m up. Ear tips reach 0.11 m; a hotdog stands 0.15 m.
const SIZE: f32 = 0.86;
/// Metres covered by one walk cycle, so the feet do not slide.
const STRIDE: f32 = 0.078;
/// Hip height and the two leg bones, in model units.
const HIP: f32 = 0.066;
const BONE: [f32; 2] = [0.032, 0.030];
/// Seconds a lunge takes, and a dead cat takes to melt away.
const LUNGE: f32 = 0.8;
const MELT: f32 = 1.8;
/// A hotdog this close (metres, on the counter) is being attacked; the simulation bites within 0.09.
const REACH: f32 = 0.125;
/// With more cats than this each throws a third of the drops of ooze it otherwise would, and those
/// more than FAR metres from the eye are drawn with a coarser coat (a cat is about 70 pixels tall there).
const CROWD: usize = 40;
const FAR: f32 = 0.9;

/// A coat: the fur, its darker markings and the pale parts, then which of the three eye colours.
struct Coat {
    colours: [[f32; 3]; 3],
    eyes: usize,
}
const COATS: [Coat; 5] = [
    // A dirty grey tabby, a mangy ginger, a black cat with grimy socks, a sickly calico, a mud brown.
    Coat { colours: [[0.36, 0.36, 0.35], [0.13, 0.13, 0.14], [0.62, 0.60, 0.54]], eyes: 0 },
    Coat { colours: [[0.74, 0.38, 0.11], [0.42, 0.17, 0.05], [0.86, 0.72, 0.52]], eyes: 1 },
    Coat { colours: [[0.10, 0.10, 0.125], [0.22, 0.21, 0.22], [0.58, 0.56, 0.53]], eyes: 2 },
    Coat { colours: [[0.76, 0.71, 0.58], [0.34, 0.19, 0.08], [0.90, 0.86, 0.76]], eyes: 0 },
    Coat { colours: [[0.43, 0.31, 0.20], [0.19, 0.23, 0.12], [0.68, 0.60, 0.47]], eyes: 1 },
];

/// A cat is drawn in rigid pieces, each one mesh: the trunk, the head, the lower jaw, the tail in three
/// lengths, a thigh (the front and hind ones differ) and a shin with its paw. Sixteen meshes a cat with
/// its eyes and the mound it rises from, where a primitive for every lump, tooth and claw was over ninety
/// and a horde of them stopped the game.
const PIECES: usize = 9;
const TORSO: usize = 0;
const HEAD: usize = 1;
const JAW: usize = 2;
const TAIL: usize = 3;
const THIGH: usize = 6;
const SHIN: usize = 8;
/// What a piece of the model is painted with: the coat's three colours, then bone, the dark of a pupil,
/// a wound, and the pink of a nose or an ear.
#[derive(Clone, Copy)]
enum Paint {
    Fur,
    Mark,
    Pale,
    Bone,
    Dark,
    Wound,
    Pink,
}
/// What a part is: the body's own shape, a marking lying on it, or a detail (teeth, claws, tufts).
/// Only the shape is there while the cat is still ooze; the rest come with its colours.
#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Shape,
    Marking,
    Detail,
}
/// How a cat is painted: all ooze, or in its coat, which is plain (0), still slimed lightly or heavily
/// (1, 2), or flashing from a hit, faintly or brightly (3, 4).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Look {
    Slime,
    Coat(usize),
}

/// The points and triangles of one piece while it is being put together. A point's colour is its
/// place in the palette (`Kit::palette`), which is all its texture coordinate says: the meshes have
/// the same layout as Bevy's own primitives, so they are drawn by pipelines the scene has compiled
/// already, and the first cat to rise does not stop the game while new ones are made.
#[derive(Default)]
struct Lump {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colours: Vec<[f32; 2]>,
    indices: Vec<u16>,
}
/// How many colours the palette holds: three for each coat, then the four every cat has.
const PALETTE: usize = 32;
impl Lump {
    fn point(&mut self, at: Vec3, normal: Vec3, colour: usize) {
        self.positions.push(at.to_array());
        self.normals.push(normal.normalize_or(Vec3::Y).to_array());
        self.colours.push([(colour as f32 + 0.5) / PALETTE as f32, 0.5]);
    }
    /// An ellipsoid with the radii `size`, of `grain` points round and rings from pole to pole.
    fn ball(&mut self, at: Vec3, size: Vec3, turn: Quat, colour: usize, grain: (u16, u16)) {
        let (round, rings) = grain;
        let first = self.positions.len() as u16;
        let place = |unit: Vec3| (at + turn * (unit * size), turn * (unit / size));
        let (top, up) = place(Vec3::Y);
        self.point(top, up, colour);
        for ring in 1..rings {
            let fall = ring as f32 / rings as f32 * PI;
            for i in 0..round {
                let spin = i as f32 / round as f32 * TAU;
                let (p, n) = place(Vec3::new(fall.sin() * spin.cos(), fall.cos(), fall.sin() * spin.sin()));
                self.point(p, n, colour);
            }
        }
        let (bottom, down) = place(Vec3::NEG_Y);
        self.point(bottom, down, colour);
        let at = |ring: u16, i: u16| first + 1 + (ring - 1) * round + i % round;
        let last = first + 1 + (rings - 1) * round;
        for i in 0..round {
            self.indices.extend_from_slice(&[first, at(1, i + 1), at(1, i)]);
            for ring in 1..rings - 1 {
                self.indices.extend_from_slice(&[at(ring, i), at(ring, i + 1), at(ring + 1, i), at(ring, i + 1), at(ring + 1, i + 1), at(ring + 1, i)]);
            }
            self.indices.extend_from_slice(&[last, at(rings - 1, i), at(rings - 1, i + 1)]);
        }
    }
    /// A cone standing on `base` and pointing along `turn`'s up: `size` is its radius and length. It has
    /// no underside: it always stands on something.
    fn spike(&mut self, base: Vec3, size: [f32; 2], turn: Quat, colour: usize) {
        let first = self.positions.len() as u16;
        self.point(base + turn * Vec3::Y * size[1], turn * Vec3::Y, colour);
        for i in 0..6 {
            let spin = i as f32 / 6. * TAU;
            let out = Vec3::new(spin.cos(), 0., spin.sin());
            self.point(base + turn * (out * size[0]), turn * (out * size[1] + Vec3::Y * size[0]), colour);
        }
        for i in 0..6 {
            self.indices.extend_from_slice(&[first + 1 + i, first, first + 1 + (i + 1) % 6]);
        }
    }
    /// A box with the edges `size`.
    fn slab(&mut self, at: Vec3, size: Vec3, turn: Quat, colour: usize) {
        for (normal, across, up) in [(Vec3::X, Vec3::NEG_Z, Vec3::Y), (Vec3::NEG_X, Vec3::Z, Vec3::Y), (Vec3::Y, Vec3::X, Vec3::NEG_Z), (Vec3::NEG_Y, Vec3::X, Vec3::Z), (Vec3::Z, Vec3::X, Vec3::Y), (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y)] {
            let first = self.positions.len() as u16;
            for (a, b) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
                self.point(at + turn * ((normal + across * a + up * b) * 0.5 * size), turn * normal, colour);
            }
            self.indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        }
    }
    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.colours)
            .with_inserted_indices(Indices::U16(self.indices))
    }
}

/// Where the pieces hang from each other, in model units: the head on the trunk, the jaw on the head,
/// the eyes in the head, the tail on the rump.
const NECK: Vec3 = Vec3::new(0., 0.088, -0.080);
const CHIN: Vec3 = Vec3::new(0., -0.0095, -0.014);
const GAZE: Vec3 = Vec3::new(0., 0.0055, -0.0238);
const RUMP: Vec3 = Vec3::new(0., 0.078, 0.070);
/// The tail's six bones: each hangs off the end of the last. Two make one piece of the tail, the
/// second set at an angle in the first's mesh, which is where the next piece begins.
fn bone(i: usize) -> (f32, f32) {
    (0.021 - i as f32 * 0.001, 0.0062 - i as f32 * 0.0005)
}
/// How the second bone of each piece of the tail is set in the piece: bent a little, and kinked at
/// the fourth bone where the tail was broken.
fn bend(piece: usize) -> Quat {
    match piece {
        0 => Quat::from_rotation_x(-0.12),
        1 => Quat::from_rotation_x(-0.12) * Quat::from_rotation_z(0.75),
        _ => Quat::from_rotation_x(0.5),
    }
}
/// Where the next piece of the tail hangs in this one, and how it is turned before it moves.
fn link(piece: usize) -> (Vec3, Quat) {
    (Vec3::Y * bone(piece * 2).0 + bend(piece) * Vec3::Y * bone(piece * 2 + 1).0, bend(piece))
}

/// The model of a cat with one coat (or of any cat while it is ooze), piece by piece, and its eyes.
struct Model {
    pieces: [Lump; PIECES],
    eyes: Lump,
    /// Which of COATS it wears (None while it is ooze: only the shape is made), and what tells one
    /// coat's tufts from another's.
    coat: Option<usize>,
    seed: u32,
    /// Whether it is the model seen from close to, or the coarser one a crowd wears at a distance.
    fine: bool,
    /// How many parts and how many of them details went into it.
    #[cfg_attr(not(test), allow(dead_code))]
    parts: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    details: usize,
}
impl Model {
    /// The place in the palette of a paint on this model's coat.
    fn colour(&self, paint: Paint) -> usize {
        match paint {
            Paint::Fur | Paint::Mark | Paint::Pale => self.coat.unwrap_or(0) * 3 + paint as usize,
            _ => COATS.len() * 3 + paint as usize - 3,
        }
    }
    /// The palette: each coat's fur, markings and pale parts, then bone, the dark of a pupil, a wound
    /// and pink, as sRGB.
    fn palette() -> Vec<[f32; 3]> {
        let mut colours: Vec<[f32; 3]> = COATS.iter().flat_map(|coat| coat.colours).collect();
        colours.extend([[0.86, 0.82, 0.66], [0.012, 0.010, 0.010], [0.46, 0.04, 0.03], [0.55, 0.27, 0.25]]);
        colours
    }
    fn wanted(&mut self, sort: Sort) -> bool {
        if self.coat.is_none() && sort != Sort::Shape {
            return false;
        }
        self.parts += 1;
        self.details += (sort == Sort::Detail) as usize;
        true
    }
    /// A lump of the body, finer the bigger it is: a cat is looked at from a hand's breadth away
    /// by the hotdog it is biting.
    fn ball(&mut self, piece: usize, sort: Sort, paint: Paint, at: Vec3, size: Vec3, turn: Quat) {
        if self.wanted(sort) {
            let colour = self.colour(paint);
            let grain = match (size.max_element(), self.fine) {
                (big, true) if big >= 0.012 => (20, 12),
                (middling, true) if middling >= 0.005 => (14, 8),
                (_, true) => (8, 5),
                (big, false) if big >= 0.012 => (12, 7),
                (middling, false) if middling >= 0.005 => (8, 5),
                (_, false) => (6, 4),
            };
            self.pieces[piece].ball(at, size, turn, colour, grain);
        }
    }
    fn spike(&mut self, piece: usize, sort: Sort, paint: Paint, base: Vec3, size: [f32; 2], turn: Quat) {
        if self.wanted(sort) {
            let colour = self.colour(paint);
            self.pieces[piece].spike(base, size, turn, colour);
        }
    }
    fn slab(&mut self, piece: usize, sort: Sort, paint: Paint, at: Vec3, size: Vec3, turn: Quat) {
        if self.wanted(sort) {
            let colour = self.colour(paint);
            self.pieces[piece].slab(at, size, turn, colour);
        }
    }
    /// Sculpts the cat. `coat` is which of COATS it wears; None makes the bare shape it has as ooze.
    /// `fine` rounds its lumps finely enough to be looked at from a hand's breadth away.
    fn new(coat: Option<usize>, fine: bool) -> Model {
        let mut m = Model { pieces: Default::default(), eyes: Lump::default(), coat, seed: coat.unwrap_or(0) as u32 + 1, fine, parts: 0, details: 0 };
        use Paint::*;
        use Sort::*;
        let (v, none) = (Vec3::new, Quat::IDENTITY);
        // Trunk: a deep chest, a starved waist under an arched spine, bony haunches.
        m.ball(TORSO, Shape, Fur, v(0., 0.068, -0.038), v(0.0225, 0.0245, 0.031), none);
        m.ball(TORSO, Shape, Fur, v(0., 0.065, 0.004), v(0.0168, 0.0180, 0.040), none);
        m.ball(TORSO, Shape, Fur, v(0., 0.068, 0.045), v(0.0235, 0.0265, 0.030), none);
        m.ball(TORSO, Shape, Fur, v(0., 0.080, 0.004), v(0.0135, 0.0125, 0.037), none);
        m.ball(TORSO, Shape, Pale, v(0., 0.052, 0.0), v(0.012, 0.008, 0.034), none);
        m.ball(TORSO, Shape, Fur, v(0., 0.079, -0.062), v(0.0135, 0.0155, 0.021), Quat::from_rotation_x(-0.5));
        for side in [-1., 1.] {
            m.ball(TORSO, Shape, Fur, v(side * 0.011, 0.088, -0.040), v(0.007, 0.007, 0.011), none);
        }
        // Markings, and matted spikes of fur down the spine.
        // Tabby bands round the trunk, a saddle on the rump and a blotch on one shoulder.
        for (z, y, wide, deep) in [(-0.040, 0.068, 0.0232, 0.0254), (-0.014, 0.066, 0.0197, 0.0212), (0.012, 0.0655, 0.0178, 0.0194), (0.042, 0.068, 0.0243, 0.0274)] {
            m.ball(TORSO, Marking, Mark, v(0., y, z), v(wide, deep, 0.0036), Quat::from_rotation_x(0.12));
        }
        m.ball(TORSO, Marking, Mark, v(0.008, 0.086, 0.052), v(0.012, 0.008, 0.014), none);
        m.ball(TORSO, Marking, Mark, v(-0.014, 0.080, -0.045), v(0.010, 0.008, 0.012), none);
        for i in 0..8 {
            let n = |k: u32| ooze::noise(m.seed * 53 + i * 11 + k);
            let z = -0.060 + i as f32 * 0.0175;
            let y = 0.0885 + 0.004 * (1. - ((z - 0.004) / 0.06).powi(2)).max(0.);
            let lean = Quat::from_rotation_x(0.45 + 0.5 * n(0)) * Quat::from_rotation_z((n(1) - 0.5) * 0.7);
            let (at, size) = (v((n(2) - 0.5) * 0.006, y, z), [0.0048, 0.011 + 0.009 * n(3)]);
            m.spike(TORSO, Detail, Mark, at, size, lean);
        }
        // One flank is open: ribs over a wound.
        m.ball(TORSO, Detail, Wound, v(-0.0172, 0.065, 0.007), v(0.003, 0.011, 0.015), none);
        for i in 0..3 {
            m.ball(TORSO, Detail, Bone, v(-0.0203, 0.065, -0.004 + i as f32 * 0.0105), v(0.0019, 0.0125, 0.0026), Quat::from_rotation_x(0.2));
        }
        // Head: wide skull, pale muzzle, an angry brow over glowing eyes with slit pupils.
        m.ball(HEAD, Shape, Fur, v(0., 0.003, -0.006), v(0.0215, 0.0195, 0.0215), none);
        m.ball(HEAD, Shape, Pale, v(0., -0.006, -0.026), v(0.0098, 0.0075, 0.0085), none);
        m.ball(HEAD, Detail, Pink, v(0., -0.0015, -0.0345), v(0.0036, 0.0026, 0.0024), none);
        for side in [-1., 1.] {
            m.ball(HEAD, Shape, Pale, v(side * 0.012, -0.0055, -0.017), v(0.010, 0.008, 0.010), none);
            let ruff = Quat::from_rotation_z(-side * (FRAC_PI_2 + 0.45));
            m.spike(HEAD, Detail, Fur, v(side * 0.019, -0.004, -0.011), [0.0042, 0.012], ruff);
            // One eye is swollen wide; the other squints. The eyes are a piece of their own: they glow.
            let wide = if side > 0. { 1.18 } else { 0.86 };
            m.eyes.ball(v(side * 0.0102, 0., 0.), v(0.0062, 0.0052, 0.0034) * wide, none, 0, (16, 10));
            m.slab(HEAD, Detail, Dark, v(side * 0.0102, 0.0055, -0.0238 - 0.0034 * wide), v(0.0009, 0.0044 * wide, 0.0006), none);
            m.ball(HEAD, Detail, Mark, v(side * 0.0105, 0.0122, -0.0232), v(0.0085, 0.0028, 0.0045), Quat::from_rotation_z(side * 0.5));
            // Ears: the left one is torn down to a stump with a shard standing beside it.
            let (tilt, base) = (Quat::from_rotation_z(-side * 0.3) * Quat::from_rotation_x(0.12), v(side * 0.0135, 0.0175, -0.002));
            if side > 0. {
                m.spike(HEAD, Shape, Fur, base, [0.0095, 0.026], tilt);
                m.spike(HEAD, Detail, Pink, base + v(0., 0.001, -0.0035), [0.0058, 0.018], tilt);
            } else {
                m.spike(HEAD, Shape, Fur, base, [0.0095, 0.014], tilt);
                m.spike(HEAD, Detail, Fur, base + v(-0.005, 0.004, 0.003), [0.0032, 0.015], Quat::from_rotation_z(0.75) * Quat::from_rotation_x(0.3));
            }
            // Whiskers, bent and uneven.
            for (rise, sweep) in [(0.16, 0.25), (-0.2, 0.5)] {
                let turn = Quat::from_rotation_y(-side * sweep) * Quat::from_rotation_z(side * rise);
                m.slab(HEAD, Detail, Bone, v(side * 0.0175, -0.006, -0.026) + turn * Vec3::X * side * 0.006, v(0.017, 0.0005, 0.0005), turn);
            }
            // Fangs: long ones from the upper jaw, bared even with the mouth shut.
            m.spike(HEAD, Detail, Bone, v(side * 0.0052, -0.0105, -0.0305), [0.0017, 0.0098], Quat::from_rotation_x(PI));
            m.slab(HEAD, Detail, Bone, v(side * 0.0019, -0.0122, -0.0338), v(0.0012, 0.003, 0.0012), none);
        }
        m.ball(JAW, Shape, Pale, v(0., -0.0025, -0.0105), v(0.0078, 0.0036, 0.0115), none);
        m.ball(JAW, Detail, Wound, v(0., 0.0008, -0.008), v(0.006, 0.002, 0.009), none);
        for side in [-1., 1.] {
            m.spike(JAW, Detail, Bone, v(side * 0.0042, 0., -0.0185), [0.0012, 0.0058], none);
        }
        // Tail: six bones in three pieces, ringed with markings, kinked where it was broken.
        for piece in 0..3 {
            let (first, second) = (bone(piece * 2), bone(piece * 2 + 1));
            m.ball(TAIL + piece, Shape, Fur, v(0., first.0 * 0.5, 0.), v(first.1, first.0 * 0.6, first.1), none);
            let at = Vec3::Y * first.0 + bend(piece) * Vec3::Y * second.0 * 0.5;
            m.ball(TAIL + piece, Shape, Mark, at, v(second.1, second.0 * 0.6, second.1), bend(piece));
        }
        m.ball(TAIL + 2, Shape, Mark, link(2).0 + bend(2) * Vec3::Y * 0.002, v(0.0052, 0.0075, 0.0052), bend(2));
        // Legs: thigh and shin on hinges, a pale paw with claws out.
        m.ball(THIGH, Shape, Fur, v(0., -BONE[0] * 0.5, 0.), v(0.0095, 0.0195, 0.0115), none);
        m.ball(THIGH + 1, Shape, Fur, v(0., -BONE[0] * 0.5, 0.), v(0.0115, 0.021, 0.0155), none);
        m.ball(SHIN, Shape, Fur, v(0., -BONE[1] * 0.48, 0.), v(0.0062, 0.0165, 0.0068), none);
        m.ball(SHIN, Shape, Pale, v(0., -BONE[1], -0.005), v(0.0085, 0.0052, 0.0115), none);
        for claw in [-1., 0., 1.] {
            let aim = Quat::from_rotation_x(-FRAC_PI_2 - 0.4);
            m.spike(SHIN, Detail, Bone, v(claw * 0.0042, -BONE[1] - 0.001, -0.0135), [0.0012, 0.0055], aim);
        }
        m
    }
}

#[derive(Resource, Clone)]
struct Kit {
    /// The mound a cat rises out of.
    sphere: Handle<Mesh>,
    /// The cat's pieces as bare ooze, and painted in each of COATS: finely, and coarsely for the cats
    /// of a crowd that are more than FAR from the eye.
    slime: [Handle<Mesh>; PIECES],
    coats: [[Handle<Mesh>; PIECES]; 5],
    far: [[Handle<Mesh>; PIECES]; 5],
    eyes: Handle<Mesh>,
    /// What a coat is drawn with, by `Look::Coat`'s number. The colours are in the palette, which the
    /// meshes point into, so every cat shares these: nothing is made or changed for a cat while the
    /// game runs.
    skin: [Handle<StandardMaterial>; 5],
    glow: [Handle<StandardMaterial>; 3],
}
impl FromWorld for Kit {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let sphere = meshes.add(Sphere::new(1.).mesh().uv(16, 10));
        let bare = Model::new(None, true);
        let slime = bare.pieces.map(|lump| meshes.add(lump.mesh()));
        let eyes = meshes.add(bare.eyes.mesh());
        let coats = [0, 1, 2, 3, 4].map(|coat| Model::new(Some(coat), true).pieces.map(|lump| meshes.add(lump.mesh())));
        let far = [0, 1, 2, 3, 4].map(|coat| Model::new(Some(coat), false).pieces.map(|lump| meshes.add(lump.mesh())));
        let mut texels = vec![255u8; PALETTE * 4];
        for (i, colour) in Model::palette().into_iter().enumerate() {
            for (k, part) in colour.into_iter().enumerate() {
                texels[i * 4 + k] = (part * 255.).round() as u8;
            }
        }
        let size = Extent3d { width: PALETTE as u32, height: 1, depth_or_array_layers: 1 };
        let palette = world.resource_mut::<Assets<Image>>().add(Image::new(size, TextureDimension::D2, texels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default()));
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        // Slime on a coat darkens it towards the ooze's green and glows; a hit flashes it pale.
        let skin = [(0., 0.), (0.3, 0.), (0.6, 0.), (0., 0.35), (0., 0.8)].map(|(slimed, hit): (f32, f32)| {
            let tint = Vec3::ONE.lerp(ooze::COLOR * 1.15, slimed);
            let glow = ooze::GLOW * 3. * slimed + Vec3::new(0.3, 0.26, 0.16) * hit;
            mats.add(StandardMaterial {
                base_color: Color::srgb(tint.x, tint.y, tint.z),
                base_color_texture: Some(palette.clone()),
                emissive: LinearRgba::rgb(glow.x, glow.y, glow.z),
                perceptual_roughness: mix(0.95, 0.14, slimed),
                reflectance: mix(0.5, 0.9, slimed),
                ..default()
            })
        });
        let mut eye = |r: f32, g: f32, b: f32| {
            mats.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                emissive: LinearRgba::rgb(r * 5., g * 5., b * 5.),
                unlit: true,
                ..default()
            })
        };
        Kit { sphere, slime, coats, far, eyes, skin, glow: [eye(0.45, 1., 0.2), eye(1., 0.85, 0.12), eye(1., 0.28, 0.05)] }
    }
}

#[derive(Clone, Copy)]
struct Leg {
    upper: Entity,
    lower: Entity,
    /// Hip and knee angles at rest, the leg's place in the walk cycle, and its side (-1 left, 1 right).
    rest: [f32; 2],
    beat: f32,
    side: f32,
    front: bool,
}
/// The body of one zombie. The root entity is placed and turned by the game; everything below the
/// rig is posed here.
#[derive(Component)]
pub struct Cat {
    id: u32,
    coat: usize,
    rig: Entity,
    torso: Entity,
    head: Entity,
    jaw: Entity,
    eyes: Entity,
    mound: Entity,
    legs: [Leg; 4],
    tail: [Entity; 3],
    /// Every piece that wears the coat, and which piece of the model it is.
    pieces: Vec<(Entity, usize)>,
    look: Look,
    /// Whether its coat is the fine one (it always is unless it is one of a crowd and far from the eye).
    near: bool,
    hp: f32,
    forming: f32,
    /// Seconds since it died, once it has.
    melting: Option<f32>,
    last: Vec3,
    seen: bool,
    gait: f32,
    pace: f32,
    hit: f32,
    lunge: f32,
    shaken: bool,
}
/// A drop of ooze thrown off a cat.
#[derive(Component)]
struct Droplet {
    velocity: Vec3,
    left: f32,
    landed: bool,
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn joint(world: &mut World, parent: Entity, at: Vec3) -> Entity {
    let id = world.spawn((Transform::from_translation(at), Visibility::default())).id();
    world.entity_mut(parent).add_child(id);
    id
}

/// Builds a zombie alley cat at `p`. The coat follows the zombie's id.
pub fn spawn(world: &mut World, p: Vec3, id: u32) -> Entity {
    world.init_resource::<Kit>();
    world.init_resource::<ooze::Kit>();
    let kit = world.resource::<Kit>().clone();
    let slime = world.resource::<ooze::Kit>().clone();
    let coat = id as usize % COATS.len();
    let root = world
        .spawn((Name::new(format!("Zombie alley cat {id}")), Transform::from_translation(p), Visibility::default()))
        .id();
    let rig = joint(world, root, Vec3::ZERO);
    let mound = world.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(slime.ooze.clone()), Transform::from_scale(Vec3::ZERO), bevy::light::NotShadowCaster)).id();
    world.entity_mut(root).add_child(mound);
    // Every piece starts as ooze; `pose` paints it. Only the trunk and the head are big enough to be
    // worth a shadow.
    let mut pieces = vec![];
    let mut piece = |world: &mut World, parent: Entity, which: usize, at: Vec3, turn: Quat| {
        let transform = Transform::from_translation(at).with_rotation(turn);
        let mut entity = world.spawn((Mesh3d(kit.slime[which].clone()), MeshMaterial3d(slime.ooze.clone()), transform));
        if which > HEAD {
            entity.insert(bevy::light::NotShadowCaster);
        }
        let id = entity.id();
        world.entity_mut(parent).add_child(id);
        pieces.push((id, which));
        id
    };
    let torso = piece(world, rig, TORSO, Vec3::ZERO, Quat::IDENTITY);
    let head = piece(world, torso, HEAD, NECK, Quat::IDENTITY);
    let jaw = piece(world, head, JAW, CHIN, Quat::IDENTITY);
    let eyes = world.spawn((Mesh3d(kit.eyes.clone()), MeshMaterial3d(kit.glow[COATS[coat].eyes].clone()), Transform::from_translation(GAZE).with_scale(Vec3::ZERO), bevy::light::NotShadowCaster)).id();
    world.entity_mut(head).add_child(eyes);
    let first = piece(world, torso, TAIL, RUMP, Quat::IDENTITY);
    let second = piece(world, first, TAIL + 1, link(0).0, link(0).1);
    let third = piece(world, second, TAIL + 2, link(1).0, link(1).1);
    // Legs. Order: front left, front right, hind left, hind right; the beats make a four-beat walk.
    let mut legs = vec![];
    for (front, side, beat) in [(true, -1., 0.25), (true, 1., 0.75), (false, -1., 0.), (false, 1., 0.5)] {
        let at = if front { Vec3::new(side * 0.017, HIP, -0.040) } else { Vec3::new(side * 0.019, HIP, 0.046) };
        let upper = piece(world, rig, if front { THIGH } else { THIGH + 1 }, at, Quat::IDENTITY);
        let lower = piece(world, upper, SHIN, Vec3::new(0., -BONE[0], 0.), Quat::IDENTITY);
        legs.push(Leg { upper, lower, rest: if front { [-0.15, 0.30] } else { [0.35, -0.60] }, beat, side, front });
    }
    world.entity_mut(root).insert(Cat {
        id,
        coat,
        rig,
        torso,
        head,
        jaw,
        eyes,
        mound,
        legs: [legs[0], legs[1], legs[2], legs[3]],
        tail: [first, second, third],
        pieces,
        look: Look::Slime,
        near: true,
        hp: f32::MAX,
        forming: 0.,
        melting: None,
        last: p,
        seen: false,
        gait: ooze::noise(id * 7 + 3),
        pace: 0.,
        hit: 0.,
        lunge: 0.,
        shaken: false,
    });
    root
}

/// What the simulation says about a zombie this frame: a drop in `hp` is a hit, and `forming` is the
/// seconds left before it has risen out of its puddle.
pub fn drive(world: &mut World, entity: Entity, hp: f32, forming: f32) {
    let Some(mut cat) = world.get_mut::<Cat>(entity) else {
        return;
    };
    if hp < cat.hp - 0.01 && cat.hp != f32::MAX {
        cat.hit = 1.;
    }
    cat.hp = hp;
    cat.forming = forming;
}
/// The zombie is dead: its body stays where it fell and melts back into ooze.
pub fn kill(world: &mut World, entity: Entity) {
    match world.get_mut::<Cat>(entity) {
        Some(mut cat) => {
            cat.melting.get_or_insert(0.);
        }
        None => world.entity_mut(entity).despawn(),
    }
}

/// How high a leg holds its hip off the ground at the given hip and knee angles.
fn stand(hip: f32, knee: f32) -> f32 {
    BONE[0] * hip.cos() + BONE[1] * (hip + knee).cos() + 0.0048
}
/// A leg's hip swing (1 forward, -1 back) and how far its paw is lifted at a point in the walk
/// cycle: on the ground for the first six tenths, carried forward for the rest.
fn step(u: f32) -> (f32, f32) {
    if u < 0.6 {
        (1. - 2. * u / 0.6, 0.)
    } else {
        let carry = (u - 0.6) / 0.4;
        (-1. + 2. * smooth(carry), (carry * PI).sin())
    }
}
/// The whole-body scale of a cat `progress` of the way through forming (0 to 1): a flat pool of ooze
/// that rises, overshoots a little, and settles.
fn risen(progress: f32) -> Vec3 {
    let up = smooth((progress - 0.22) / 0.6);
    let spring = 1. + 0.1 * ((progress - 0.82) / 0.18).clamp(0., 1.).mul_add(PI, 0.).sin();
    Vec3::new(mix(1.4, 1., up), mix(0.03, 1., up) * spring, mix(1.3, 1., up))
}
/// How much of the cat is still ooze-coloured while forming.
fn slimed(progress: f32) -> f32 {
    1. - smooth((progress - 0.58) / 0.4)
}

/// Throws `count` drops of ooze off a cat at `at`; a third as many (two at least) when there is a
/// `crowd` of cats, each of them throwing its own.
fn spurt(world: &mut World, slime: &ooze::Kit, at: Vec3, away: Vec3, count: u32, seed: u32, crowd: bool) {
    for i in 0..if crowd { (count / 3).max(2) } else { count } {
        let n = |k: u32| ooze::noise(seed.wrapping_mul(131).wrapping_add(i * 17 + k));
        let angle = n(0) * TAU;
        let velocity = away * (0.15 + 0.2 * n(1)) + Vec3::new(angle.cos() * 0.16 * n(2), 0.25 + 0.35 * n(3), angle.sin() * 0.16 * n(2));
        world.spawn((
            Mesh3d(slime.drop.clone()),
            MeshMaterial3d(slime.ooze.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(0.0035 + 0.004 * n(4))),
            Droplet { velocity, left: 0.9 + 0.8 * n(5), landed: false },
            bevy::light::NotShadowCaster,
        ));
    }
}
fn turn(world: &mut World, entity: Entity, rotation: Quat) {
    if let Some(mut tr) = world.get_mut::<Transform>(entity) {
        tr.rotation = rotation;
    }
}

/// Poses one cat for this frame. `prey` is where the living hotdogs stand. `crowd` is where the eye
/// is when there are so many cats that each should throw fewer drops and the far ones be drawn coarsely.
fn pose(world: &mut World, kit: &Kit, slime: &ooze::Kit, root: Entity, cat: &mut Cat, prey: &[Vec3], now: f32, dt: f32, crowd: Option<Vec3>) -> bool {
    let Some(place) = world.get::<Transform>(root).copied() else {
        return false;
    };
    let at = place.translation;
    let forward = place.rotation * Vec3::NEG_Z;
    if let Some(t) = cat.melting.as_mut() {
        *t += dt;
        if *t >= MELT {
            return false;
        }
    }
    let melt = cat.melting.unwrap_or(0.);
    let dying = cat.melting.is_some();
    let progress = if cat.forming > 0. && !dying { (1. - cat.forming / FORM).clamp(0., 1.) } else { 1. };
    let formed = progress >= 1. && !dying;
    // The walk follows the ground covered.
    let moved = if cat.seen { Vec2::new(at.x - cat.last.x, at.z - cat.last.z).length() } else { 0. };
    // A cat first seen already formed (a round joined part-way) has nothing left to shake off.
    cat.shaken |= !cat.seen && progress >= 1.;
    cat.last = at;
    cat.seen = true;
    if formed && dt > 0. {
        cat.gait = (cat.gait + moved / (STRIDE * SIZE)) % 1.;
        cat.pace += (moved / dt - cat.pace) * (1. - (-dt * 8.).exp());
    } else {
        cat.pace = 0.;
    }
    let walking = smooth(cat.pace / 0.05);
    // A hotdog in reach: rear back, lunge with jaws wide and swipe the right forepaw.
    let near = prey.iter().any(|p| Vec2::new(p.x - at.x, p.z - at.z).length() < REACH && (p.y - at.y).abs() < 0.12);
    if formed && (near || cat.lunge > 0.) {
        cat.lunge += dt;
        if cat.lunge >= LUNGE {
            cat.lunge = if near { cat.lunge - LUNGE } else { 0. };
        }
    } else {
        cat.lunge = 0.;
    }
    let a = cat.lunge / LUNGE;
    let rear = smooth(a / 0.3) * (1. - smooth((a - 0.3) / 0.12));
    let strike = smooth((a - 0.3) / 0.1) * (1. - smooth((a - 0.48) / 0.4));
    // A hit jolts it back and throws off ooze.
    if cat.hit >= 1. && !dying {
        spurt(world, slime, at + Vec3::Y * 0.07 * SIZE, -forward, 5, cat.id * 97 + (now * 60.) as u32, crowd.is_some());
    }
    let hit = if dying { 0. } else { cat.hit };
    cat.hit = (cat.hit - dt * 4.5).max(0.);
    if dying && melt <= dt {
        spurt(world, slime, at + Vec3::Y * 0.06 * SIZE, Vec3::ZERO, 9, cat.id * 61 + 5, crowd.is_some());
    }
    // Shaking off the last of the ooze as it finishes forming.
    if progress > 0.86 && !cat.shaken && !dying {
        cat.shaken = true;
        spurt(world, slime, at + Vec3::Y * 0.08 * SIZE, Vec3::ZERO, 6, cat.id * 29 + 1, crowd.is_some());
    }
    let shake = if dying { 0. } else { (now * 38.).sin() * 0.22 * smooth((progress - 0.8) / 0.08) * (1. - smooth((progress - 0.9) / 0.1)) };
    let sag = smooth(melt / 0.5);
    let pooled = smooth((melt - 0.5) / 0.5);
    // Crouch: low while taking shape, a stalking stoop afterwards, deeper before a lunge.
    let crouch = 0.45 + 1.0 * (1. - smooth((progress - 0.3) / 0.6)) + 0.35 * rear + 1.1 * sag;
    let id = cat.id;
    let n = move |k: u32| ooze::noise(id.wrapping_mul(977).wrapping_add(k));
    let mut ground = [0f32; 2];
    for leg in cat.legs {
        let u = (cat.gait + leg.beat) % 1.;
        let (swing, lift) = step(u);
        // The left hind leg drags.
        let lame = if !leg.front && leg.side < 0. { 0.55 } else { 1. };
        let bend = if leg.front { [-0.28, 0.56] } else { [0.30, -0.62] };
        let mut hip = leg.rest[0] + bend[0] * crouch + 0.40 * swing * walking * lame;
        let mut knee = leg.rest[1] + bend[1] * crouch - 0.75 * lift * walking * lame * if leg.front { -0.6 } else { 1. };
        ground[if leg.front { 0 } else { 1 }] += stand(leg.rest[0] + bend[0] * crouch, leg.rest[1] + bend[1] * crouch) * 0.5;
        if leg.front && leg.side > 0. {
            // The swipe: the paw comes up beside the head and rakes down and across.
            hip += 1.5 * rear + 0.9 * strike;
            knee += -0.5 * rear + 0.3 * strike;
        }
        let splay = leg.side * (1.25 * sag + 0.5 * (1. - smooth((progress - 0.2) / 0.5)) - 0.35 * strike * if leg.front && leg.side > 0. { 1. } else { 0. });
        turn(world, leg.upper, Quat::from_rotation_z(splay) * Quat::from_rotation_x(hip));
        turn(world, leg.lower, Quat::from_rotation_x(knee));
    }
    // The body rides on its legs: lower in a crouch, tipped when the two ends stand differently.
    let height = (ground[0] + ground[1]) * 0.5;
    let tip = ((ground[0] - ground[1]) / 0.086).atan();
    let scale = if dying {
        Vec3::new(mix(1., 1.5, sag), mix(1., 0.2, sag) * mix(1., 0.3, pooled), mix(1., 1.4, sag)) * (1. - smooth((melt - 1.0) / (MELT - 1.0)))
    } else {
        risen(progress)
    };
    if let Some(mut tr) = world.get_mut::<Transform>(cat.rig) {
        tr.translation = Vec3::new(0., (height - HIP) * scale.y * SIZE, (0.012 + 0.016 * rear - 0.024 * strike + 0.016 * hit) * SIZE);
        tr.rotation = Quat::from_rotation_x(tip + 0.2 * rear - 0.1 * strike + 0.16 * hit) * Quat::from_rotation_z(shake + 0.2 * hit * (n(1) - 0.5));
        tr.scale = scale * SIZE;
    }
    let sway = (cat.gait * TAU).sin() * walking;
    if let Some(mut tr) = world.get_mut::<Transform>(cat.torso) {
        tr.translation.y = (0.0016 * (cat.gait * 2. * TAU).sin() * walking + 0.0007 * (now * 2.4 + n(2) * TAU).sin()) * (1. - sag);
        tr.rotation = Quat::from_rotation_y(0.07 * sway) * Quat::from_rotation_z(0.03 * (cat.gait * TAU).cos() * walking);
    }
    // Head: carried low and tilted, twitching now and then; thrown up and forward to bite.
    let beat = now * 1.3 + n(3) * 7.;
    let twitch = if ooze::noise(id.wrapping_mul(31).wrapping_add(beat as u32)) > 0.62 && beat.fract() < 0.18 { (beat.fract() / 0.18 * PI).sin() * (n(4) - 0.5) * 0.9 } else { 0. };
    let nod = -0.16 - 0.5 * (1. - smooth((progress - 0.45) / 0.5)) + 0.5 * rear - 0.05 * strike + 0.3 * hit - 0.5 * sag;
    let tilt = (n(5) - 0.5) * 0.5 + twitch;
    turn(world, cat.head, Quat::from_euler(EulerRot::YXZ, -0.11 * sway + twitch * 0.4, nod, tilt));
    let gape = 0.1 + 0.75 * strike.max(rear * 0.7) + 0.55 * hit + 0.5 * sag + 0.35 * smooth((progress - 0.8) / 0.1) * (1. - smooth((progress - 0.9) / 0.1));
    turn(world, cat.jaw, Quat::from_rotation_x(-gape));
    // Tail: up in a hook, lashing harder the angrier the cat; limp while forming or dying.
    let limp = (1. - smooth((progress - 0.4) / 0.5)).max(sag);
    let lash = 0.14 + 0.3 * rear.max(strike).max(hit) + 0.1 * walking;
    for (piece, entity) in cat.tail.iter().enumerate() {
        // Up and back from the rump, then hooked over at the tip. Each piece is two of the tail's bones.
        let i = piece * 2;
        let curl = mix([1.2, -0.12, 0.25][piece], if piece == 0 { 1.75 } else { 0.02 }, limp);
        let wave = (now * (2.6 + 3. * lash) - i as f32 * 0.7 + n(6) * TAU).sin() * lash * (0.5 + i as f32 * 0.25) * (1. - limp);
        // A limp tail lies coiled beside the body rather than trailing across the counter.
        let coil = if piece == 0 { 0. } else { 2.1 * limp };
        let set = if piece == 0 { Quat::IDENTITY } else { link(piece - 1).1 };
        turn(world, *entity, set * Quat::from_rotation_x(curl) * Quat::from_rotation_z(wave + coil));
    }
    // The eyes light first, in the rising ooze.
    let lit = smooth((progress - 0.4) / 0.12) * (1. - smooth(melt / 0.6));
    let blink = if (now * 0.37 + n(7) * 5.).fract() > 0.965 { 0.12 } else { 1. };
    if let Some(mut tr) = world.get_mut::<Transform>(cat.eyes) {
        tr.scale = Vec3::new(lit, lit * blink, lit);
    }
    // The puddle heaps up into a mound that the cat grows out of.
    let heap = smooth(progress / 0.42);
    let shed = 1. - smooth((progress - 0.42) / 0.36);
    if let Some(mut tr) = world.get_mut::<Transform>(cat.mound) {
        let size = Vec3::new(mix(0.058, 0.040, heap), mix(0.004, 0.052, heap), mix(0.058, 0.055, heap)) * shed * if dying { 0. } else { 1. };
        tr.translation = Vec3::Y * size.y * 0.7;
        tr.scale = size;
    }
    // Paint: ooze while forming or melting, then the coat (with its markings, teeth, claws and tufts),
    // slimed at first, flashing pale when hit. A cat changes its meshes and materials for the shared
    // ones of its new look; none is made or altered.
    let slime_share = if dying { smooth(melt / 0.4) } else { slimed(progress) };
    let look = if slime_share > 0.75 {
        Look::Slime
    } else if slime_share > 0.42 {
        Look::Coat(2)
    } else if slime_share > 0.12 {
        Look::Coat(1)
    } else if hit > 0.55 {
        Look::Coat(4)
    } else if hit > 0.15 {
        Look::Coat(3)
    } else {
        Look::Coat(0)
    };
    // One of a crowd wears the coarse coat while it is more than FAR from the eye (and a little
    // further before it changes, so that a cat at that distance does not flicker between the two).
    let near = crowd.is_none_or(|eye| eye.distance(at) < FAR * if cat.near { 1.1 } else { 0.9 });
    if look != cat.look || near != cat.near {
        let bare = |look: Look| look == Look::Slime;
        for (entity, which) in &cat.pieces {
            let Ok(mut piece) = world.get_entity_mut(*entity) else {
                continue;
            };
            if bare(look) != bare(cat.look) || (near != cat.near && !bare(look)) {
                piece.insert(Mesh3d(if bare(look) { kit.slime[*which].clone() } else if near { kit.coats[cat.coat][*which].clone() } else { kit.far[cat.coat][*which].clone() }));
            }
            if look != cat.look {
                piece.insert(MeshMaterial3d(match look {
                    Look::Slime => slime.ooze.clone(),
                    Look::Coat(level) => kit.skin[level].clone(),
                }));
            }
        }
        (cat.look, cat.near) = (look, near);
    }
    true
}

/// Every frame: poses every cat, removes the ones that have melted away, and flies the drops of ooze.
pub fn animate(world: &mut World) {
    if !world.contains_resource::<Game>() {
        return;
    }
    world.init_resource::<Kit>();
    world.init_resource::<ooze::Kit>();
    let (kit, slime) = (world.resource::<Kit>().clone(), world.resource::<ooze::Kit>().clone());
    let (now, dt) = {
        let time = world.resource::<Time<Real>>();
        (time.elapsed_secs(), time.delta_secs().min(0.1))
    };
    let prey: Vec<Vec3> = world.resource::<Game>().roster.iter().filter(|p| p.alive).map(|p| Vec3::from_array(p.p)).collect();
    // (Over a sink the counter is the sink's floor.)
    let sinks: Vec<([f32; 4], f32)> = world.resource::<Game>().feast.level.map.sinks.iter().map(|s| (s.rect.map(|v| v as f32), s.depth as f32)).collect();
    let cats: Vec<Entity> = crate::queries::with::<Entity, With<Cat>, _>(world, |world, q| q.iter(world).collect());
    let eye = crate::queries::with::<&GlobalTransform, With<crate::maya::HotdogCamera>, _>(world, |world, q| q.iter(world).next().map(|camera| camera.translation()));
    let crowd = if cats.len() > CROWD { Some(eye.unwrap_or(Vec3::splat(f32::MAX))) } else { None };
    for entity in cats {
        let Some(mut cat) = world.entity_mut(entity).take::<Cat>() else {
            continue;
        };
        if pose(world, &kit, &slime, entity, &mut cat, &prey, now, dt, crowd) {
            world.entity_mut(entity).insert(cat);
        } else {
            world.entity_mut(entity).despawn();
        }
    }
    let mut spent = vec![];
    crate::queries::with::<(Entity, &mut Droplet, &mut Transform), (), _>(world, |world, q| {
        for (entity, mut drop, mut tr) in q.iter_mut(world) {
            drop.left -= dt;
            if drop.left <= 0. {
                spent.push(entity);
            } else if !drop.landed {
                drop.velocity.y -= 2.8 * dt;
                let fall = drop.velocity * dt;
                tr.translation += fall;
                let p = tr.translation;
                let floor = sinks.iter().find(|(r, _)| p.x >= r[0] && p.x <= r[2] && p.z >= r[1] && p.z <= r[3]).map_or(FLOOR, |(_, depth)| FLOOR - depth);
                // A drop that reaches the counter stops and flattens.
                if tr.translation.y < floor + 0.0015 {
                    tr.translation.y = floor + 0.0015;
                    tr.scale *= Vec3::new(1.9, 0.25, 1.9);
                    drop.landed = true;
                }
            } else if drop.left < 0.3 {
                tr.scale *= (1. - dt * 5.).max(0.);
            }
        }
    });
    for entity in spent {
        world.entity_mut(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world
    }
    /// Where a point of a piece ends up in the cat's own frame: the transforms from it up to the root, applied in turn.
    fn placed(world: &World, mut entity: Entity, root: Entity, mut point: Vec3) -> Vec3 {
        while entity != root {
            point = world.get::<Transform>(entity).unwrap().transform_point(point);
            entity = world.get::<ChildOf>(entity).unwrap().parent();
        }
        point
    }
    fn meshes(world: &mut World, root: Entity) -> Vec<Entity> {
        let mut found = vec![];
        let mut open = vec![root];
        while let Some(e) = open.pop() {
            if world.get::<Mesh3d>(e).is_some() {
                found.push(e);
            }
            open.extend(world.get::<Children>(e).map(|c| c.iter().collect::<Vec<_>>()).unwrap_or_default());
        }
        found
    }
    fn points(world: &World, mesh: &Handle<Mesh>) -> Vec<Vec3> {
        let Some(VertexAttributeValues::Float32x3(points)) = world.resource::<Assets<Mesh>>().get(mesh).unwrap().attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("no points") };
        points.iter().map(|p| Vec3::from_array(*p)).collect()
    }
    /// The cat's bounds in its own frame, from every point of every piece that shows.
    fn bounds(world: &mut World, root: Entity) -> (Vec3, Vec3) {
        let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for e in meshes(world, root) {
            if world.get::<Transform>(e).unwrap().scale.max_element() < 1e-5 {
                continue;
            }
            for point in points(world, &world.get::<Mesh3d>(e).unwrap().0) {
                let p = placed(world, e, root, point);
                (low, high) = (low.min(p), high.max(p));
            }
        }
        (low, high)
    }
    fn settle(world: &mut World, root: Entity, prey: &[Vec3], frames: u32, mut each: impl FnMut(&mut World, u32)) {
        let (kit, slime) = (world.resource::<Kit>().clone(), world.resource::<ooze::Kit>().clone());
        for frame in 0..frames {
            each(world, frame);
            let mut cat = world.entity_mut(root).take::<Cat>().unwrap();
            assert!(pose(world, &kit, &slime, root, &mut cat, prey, frame as f32 / 60., 1. / 60., None));
            world.entity_mut(root).insert(cat);
        }
    }
    /// The material a piece is drawn with, and whether its mesh is one of a coat's (rather than the bare shape).
    fn paint(world: &World, entity: Entity) -> (Handle<StandardMaterial>, bool) {
        let mesh = &world.get::<Mesh3d>(entity).unwrap().0;
        (world.get::<MeshMaterial3d<StandardMaterial>>(entity).unwrap().0.clone(), world.resource::<Kit>().coats.iter().flatten().any(|worn| worn == mesh))
    }
    #[test]
    fn a_cat_is_a_cat_sized_for_a_hotdog_and_stands_on_the_counter() {
        let mut world = world();
        let root = spawn(&mut world, Vec3::new(0., FLOOR, 0.), 1);
        drive(&mut world, root, 70., 0.);
        settle(&mut world, root, &[], 3, |_, _| {});
        // Four legs of two bones, a tail in three pieces (six bones), two eyes, and plenty of scruff:
        // over sixty different parts (a leg's are sculpted once and worn by all four), more than
        // thirty of them details.
        let kit = world.resource::<Kit>().clone();
        let cat = world.entity_mut(root).take::<Cat>().unwrap();
        assert_eq!((cat.legs.len(), cat.tail.len(), cat.pieces.len()), (4, 3, 14));
        let model = Model::new(Some(1), true);
        assert!(model.parts > 60 && model.details > 30, "{} parts, {} details", model.parts, model.details);
        assert_eq!(Model::new(None, true).details, 0);
        assert_eq!(points(&world, &kit.eyes).iter().filter(|p| p.x > 0.).count(), points(&world, &kit.eyes).len() / 2);
        // All of it is sixteen meshes: fourteen pieces, the eyes, and the mound it rose from.
        let (eyes, mound) = (cat.eyes, cat.mound);
        world.entity_mut(root).insert(cat);
        let drawn = meshes(&mut world, root);
        assert!(drawn.len() == 16 && drawn.contains(&eyes) && drawn.contains(&mound), "{} meshes", drawn.len());
        // It fits the zombie the simulation hit-tests (0.13 m across, 0.14 m tall) give or take a
        // nose and a tail, is lower than a hotdog (0.15 m), and its paws are on the counter.
        let (low, high) = bounds(&mut world, root);
        assert!(low.y > -0.006 && low.y < 0.004, "paws at {}", low.y);
        assert!(high.y > 0.09 && high.y < 0.135, "ears at {}", high.y);
        assert!(low.z > -0.105 && low.z < -0.07 && high.z > 0.1 && high.z < 0.19, "nose {} tail {}", low.z, high.z);
        assert!(high.x - low.x < 0.07 && high.x - low.x > 0.035);
        // Every coat is different.
        for (i, a) in COATS.iter().enumerate() {
            assert!(COATS.iter().skip(i + 1).all(|b| Vec3::from_array(a.colours[0]).distance(Vec3::from_array(b.colours[0])) > 0.05));
        }
    }
    /// A horde is drawn with what one cat is: every cat of a coat has the same meshes, every cat the
    /// same few materials, and nothing is added to either when cats are made, painted, hit or killed.
    #[test]
    fn a_horde_shares_its_meshes_and_materials() {
        let mut world = world();
        let cats: Vec<Entity> = (1..=128).map(|id| spawn(&mut world, Vec3::new(id as f32 * 0.1, FLOOR, 0.), id)).collect();
        let stock = |world: &World| (world.resource::<Assets<Mesh>>().len(), world.resource::<Assets<StandardMaterial>>().len());
        let before = stock(&world);
        // A sphere and the pieces as ooze and in five coats, fine and coarse, and the eyes, and the liquid's
        // sphere and drop; five ways to draw a coat, three colours of eye, and the ooze.
        assert_eq!(before, (1 + PIECES * 11 + 1 + 2, 5 + 3 + 1));
        for (i, cat) in cats.iter().enumerate() {
            drive(&mut world, *cat, 70., if i % 2 == 0 { 0. } else { FORM * 0.2 });
        }
        for cat in &cats {
            settle(&mut world, *cat, &[], 2, |_, _| {});
        }
        for cat in &cats[..64] {
            drive(&mut world, *cat, 30., 0.);
            kill(&mut world, *cat);
            settle(&mut world, *cat, &[], 30, |_, _| {});
        }
        assert_eq!(stock(&world), before);
        // Two of the living that wear the same coat: the same meshes, with the same shared material.
        let (a, b) = (world.get::<Cat>(cats[66]).unwrap(), world.get::<Cat>(cats[76]).unwrap());
        assert_eq!((a.coat, a.look), (b.coat, Look::Coat(0)));
        for ((first, _), (second, _)) in a.pieces.iter().zip(&b.pieces) {
            assert_eq!(world.get::<Mesh3d>(*first).unwrap().0, world.get::<Mesh3d>(*second).unwrap().0);
            assert_eq!(paint(&world, *first).0, paint(&world, *second).0);
        }
        // A whole cat is several thousand triangles, where a primitive for every part was 25,000.
        let kit = world.resource::<Kit>().clone();
        let triangles: usize = a.pieces.iter().map(|(_, which)| world.resource::<Assets<Mesh>>().get(&kit.coats[a.coat][*which]).unwrap().indices().unwrap().len() / 3).sum();
        println!("a cat is {triangles} triangles");
        assert!(triangles > 5000 && triangles < 20000, "{triangles} triangles");
    }
    /// The game steps the simulation once a frame. With a horde of 128 on its feet and the bots fighting
    /// it, a step must leave nearly all of the frame for drawing: well under 2 ms in a release build,
    /// which is how this suite is run. (The quickest quarter of the steps is judged, so that a busy
    /// machine does not fail it; a debug build is only held to a figure that would catch a step that
    /// had become quadratic in something large.)
    #[test]
    fn stepping_a_horde_of_128_leaves_the_frame_for_drawing() {
        use kitchen_sim::{feast::Feast, map::Map};
        let mut map = Map::bundled();
        let spare = map.spawns[0].clone();
        for i in 0..61 {
            let mut spawn = spare.clone();
            spawn.p = [-1.6 + 0.4 * (i % 9) as f64, 2.4 + 0.1 * (i / 9) as f64];
            map.spawns.push(spawn);
        }
        let mut s = Feast::with_map(map, true);
        assert_eq!(s.wave(), 128);
        let mut times = vec![];
        for _ in 0..30 * 60 {
            if times.len() == 240 {
                break;
            }
            for p in &mut s.players {
                p.hp = 1e9;
            }
            for z in &mut s.enemies {
                z.hp = 1e9;
            }
            let full = s.enemies.len() == 128 && s.enemies.iter().all(|z| z.forming <= 0.);
            let from = std::time::Instant::now();
            s.tick(1. / 30.);
            if full {
                times.push(from.elapsed().as_secs_f64() * 1e3);
            }
        }
        assert_eq!(times.len(), 240, "the whole horde is on its feet together");
        times.sort_by(f64::total_cmp);
        let (quick, median) = (times[60], times[120]);
        println!("a step with 128 zombies: quickest quarter {quick:.3} ms, median {median:.3} ms");
        assert!(quick < if cfg!(debug_assertions) { 40. } else { 1.5 } * kitchen_sim::timing_slack(), "quickest quarter {quick:.3} ms, median {median:.3} ms");
    }
    #[test]
    fn a_forming_cat_rises_out_of_a_mound_of_ooze_and_takes_its_colours_last() {
        // Flat at first, full height at the end, never shrinking on the way up to the spring.
        assert!(risen(0.).y < 0.05 && risen(0.).x > 1.3 && (risen(1.) - Vec3::ONE).length() < 1e-5);
        let mut last = 0.;
        for i in 0..=80 {
            let y = risen(i as f32 / 100.).y;
            assert!(y >= last - 1e-6);
            last = y;
        }
        assert_eq!((slimed(0.), slimed(0.5), slimed(1.)), (1., 1., 0.));
        let mut world = world();
        let root = spawn(&mut world, Vec3::new(0., FLOOR, 0.), 2);
        let kit = world.resource::<Kit>().clone();
        let ooze = world.resource::<ooze::Kit>().ooze.clone();
        let look = |world: &World| world.get::<Cat>(root).unwrap().look;
        let torso = |world: &World| paint(world, world.get::<Cat>(root).unwrap().torso);
        let size = |world: &World, pick: fn(&Cat) -> Entity| world.get::<Transform>(pick(world.get::<Cat>(root).unwrap())).unwrap().scale;
        // Early: a low mound of ooze, no eyes yet.
        drive(&mut world, root, 70., FORM * 0.8);
        settle(&mut world, root, &[], 1, |_, _| {});
        assert_eq!((look(&world), torso(&world)), (Look::Slime, (ooze.clone(), false)));
        assert!(size(&world, |c| c.mound).y > 0.015 && size(&world, |c| c.rig).y < 0.1 && size(&world, |c| c.eyes).x == 0.);
        // Half-way: the eyes are alight in a cat-shaped mass of ooze: every piece is the liquid's own
        // material on the bare shape, with no markings, teeth or claws.
        drive(&mut world, root, 70., FORM * 0.45);
        settle(&mut world, root, &[], 1, |_, _| {});
        assert!(look(&world) == Look::Slime && size(&world, |c| c.eyes).x > 0.9 && size(&world, |c| c.rig).y > 0.3);
        for (piece, which) in world.get::<Cat>(root).unwrap().pieces.clone() {
            assert_eq!((paint(&world, piece), world.get::<Mesh3d>(piece).unwrap().0.clone()), ((ooze.clone(), false), kit.slime[which].clone()));
        }
        // Then its coat comes through the slime, heavily slimed and then lightly.
        let mut seen = vec![];
        for tenth in [6.6, 7.4, 8.2, 8.6] {
            drive(&mut world, root, 70., FORM * (1. - tenth / 10.));
            settle(&mut world, root, &[], 1, |_, _| {});
            seen.push(look(&world));
        }
        assert_eq!(seen, [Look::Slime, Look::Coat(2), Look::Coat(1), Look::Coat(1)]);
        assert_eq!(torso(&world), (kit.skin[1].clone(), true));
        // Formed: its own coat, full size, mound gone, fangs and claws out.
        drive(&mut world, root, 70., 0.);
        settle(&mut world, root, &[], 1, |_, _| {});
        assert_eq!((look(&world), torso(&world)), (Look::Coat(0), (kit.skin[0].clone(), true)));
        assert!((size(&world, |c| c.rig) - Vec3::splat(SIZE)).length() < 1e-5 && size(&world, |c| c.mound).y == 0.);
        let cat = world.get::<Cat>(root).unwrap();
        for (piece, which) in &cat.pieces {
            let (worn, bare) = (world.get::<Mesh3d>(*piece).unwrap().0.clone(), kit.slime[*which].clone());
            assert_eq!(worn, kit.coats[2][*which]);
            // The head, the jaw and the paws have gained their teeth, whiskers and claws; the trunk its markings.
            if [TORSO, HEAD, JAW, SHIN].contains(which) {
                assert!(points(&world, &worn).len() > points(&world, &bare).len() + 12);
            }
        }
        // The colours are in the palette, which the points name: the black cat's trunk is its fur, its
        // belly pale, its wound red; and the mesh is laid out as Bevy's own primitives are.
        let mesh = world.resource::<Assets<Mesh>>().get(&kit.coats[2][TORSO]).unwrap();
        let Some(VertexAttributeValues::Float32x2(named)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!("no colours") };
        let texture = world.resource::<Assets<StandardMaterial>>().get(&kit.skin[0]).unwrap().base_color_texture.clone().unwrap();
        let palette = world.resource::<Assets<Image>>().get(&texture).unwrap().data.clone().unwrap();
        let colour = |point: usize| {
            let texel = (named[point][0] * PALETTE as f32) as usize;
            [0, 1, 2].map(|k| palette[texel * 4 + k] as f32 / 255.)
        };
        let seen: Vec<[f32; 3]> = (0..named.len()).map(colour).collect();
        let near = |a: [f32; 3], b: [f32; 3]| Vec3::from_array(a).distance(Vec3::from_array(b)) < 0.004;
        assert!(near(seen[0], COATS[2].colours[0]) && named.len() == points(&world, &kit.coats[2][TORSO]).len());
        assert!(seen.iter().any(|c| near(*c, COATS[2].colours[2])) && seen.iter().any(|c| near(*c, [0.46, 0.04, 0.03])) && seen.iter().all(|c| !near(*c, COATS[1].colours[0])));
        let ball = Sphere::new(1.).mesh().uv(8, 5);
        assert_eq!(mesh.attributes().map(|(attribute, _)| attribute.id).collect::<Vec<_>>(), ball.attributes().map(|(attribute, _)| attribute.id).collect::<Vec<_>>());
        assert!(kit.skin.iter().all(|skin| world.resource::<Assets<StandardMaterial>>().get(skin).unwrap().base_color_texture == Some(texture.clone())));
    }
    #[test]
    fn a_cat_walks_as_far_as_it_steps_lunges_at_a_hotdog_in_reach_and_flinches_when_hit() {
        let mut world = world();
        let root = spawn(&mut world, Vec3::new(0., FLOOR, 0.), 3);
        drive(&mut world, root, 70., 0.);
        let hips = |world: &World| world.get::<Cat>(root).unwrap().legs.map(|leg| world.get::<Transform>(leg.upper).unwrap().rotation);
        settle(&mut world, root, &[], 30, |_, _| {});
        let still = hips(&world);
        assert_eq!(world.get::<Cat>(root).unwrap().pace, 0.);
        // Walking at the zombies' 0.10 m/s swings every leg, and one cycle covers one stride.
        let start = world.get::<Cat>(root).unwrap().gait;
        let mut swung = [0f32; 4];
        let frames = (STRIDE * SIZE / 0.10 * 60.).round() as u32;
        settle(&mut world, root, &[], frames, |world, frame| {
            world.get_mut::<Transform>(root).unwrap().translation.z = -0.10 * (frame + 1) as f32 / 60.;
            for (i, leg) in world.get::<Cat>(root).unwrap().legs.iter().enumerate() {
                swung[i] = swung[i].max(world.get::<Transform>(leg.upper).unwrap().rotation.angle_between(still[i]));
            }
        });
        let gait = world.get::<Cat>(root).unwrap().gait;
        assert!((gait - start).abs() < 0.02 || (gait - start).abs() > 0.98, "one stride is one cycle: {start} to {gait}");
        assert!(swung.iter().all(|a| *a > 0.15), "{swung:?}");
        // The four beats are a quarter-cycle apart, and each paw is on the ground most of the time.
        let mut beats = world.get::<Cat>(root).unwrap().legs.map(|leg| leg.beat);
        beats.sort_by(f32::total_cmp);
        assert_eq!(beats, [0., 0.25, 0.5, 0.75]);
        assert!(step(0.3).1 == 0. && step(0.8).1 > 0.9 && step(0.).0 == 1. && step(0.6).0 == -1.);
        // A hotdog in reach: the jaw gapes and the body goes forward within one lunge.
        let prey = [world.get::<Transform>(root).unwrap().translation + Vec3::NEG_Z * 0.085];
        let (mut gape, mut reach, mut rest) = (0f32, f32::MAX, 0.);
        settle(&mut world, root, &prey, (LUNGE * 60.) as u32, |world, frame| {
            let cat = world.get::<Cat>(root).unwrap();
            let z = world.get::<Transform>(cat.rig).unwrap().translation.z;
            if frame == 0 {
                rest = z;
            }
            reach = reach.min(z);
            gape = gape.max(world.get::<Transform>(cat.jaw).unwrap().rotation.angle_between(Quat::IDENTITY));
        });
        assert!(gape > 0.6 && reach < rest - 0.012, "gape {gape}, lunge {}", rest - reach);
        // A drop in health is a hit: the cat is thrown back, flashes, and ooze flies.
        let drops = |world: &mut World| world.query::<&Droplet>().iter(world).count();
        assert_eq!(drops(&mut world), 0);
        drive(&mut world, root, 35., 0.);
        settle(&mut world, root, &[], 1, |_, _| {});
        assert_eq!(drops(&mut world), 5);
        let cat = world.get::<Cat>(root).unwrap();
        assert!(cat.hit > 0.8 && cat.look == Look::Coat(4));
        let kit = world.resource::<Kit>().clone();
        assert_eq!(paint(&world, cat.head), (kit.skin[4].clone(), true));
        // The flash fades, and the cat is in its plain coat again.
        settle(&mut world, root, &[], 20, |_, _| {});
        let cat = world.get::<Cat>(root).unwrap();
        assert_eq!((cat.look, paint(&world, cat.head)), (Look::Coat(0), (kit.skin[0].clone(), true)));
        // In a crowd each hit throws fewer drops.
        let (slime, mut cat) = (world.resource::<ooze::Kit>().clone(), world.entity_mut(root).take::<Cat>().unwrap());
        cat.hit = 1.;
        assert!(pose(&mut world, &kit, &slime, root, &mut cat, &[], 9., 1. / 60., Some(Vec3::ZERO)));
        world.entity_mut(root).insert(cat);
        assert_eq!(drops(&mut world), 5 + 2);
    }
    #[test]
    fn in_a_crowd_a_cat_far_from_the_eye_wears_a_coarser_coat_of_the_same_shape() {
        let mut world = world();
        let root = spawn(&mut world, Vec3::new(0., FLOOR, 0.), 6);
        drive(&mut world, root, 70., 0.);
        settle(&mut world, root, &[], 2, |_, _| {});
        let (kit, slime) = (world.resource::<Kit>().clone(), world.resource::<ooze::Kit>().clone());
        let worn = |world: &World| world.get::<Cat>(root).unwrap().pieces.iter().map(|(piece, _)| world.get::<Mesh3d>(*piece).unwrap().0.clone()).collect::<Vec<_>>();
        let triangles = |world: &World| worn(world).iter().map(|mesh| world.resource::<Assets<Mesh>>().get(mesh).unwrap().indices().unwrap().len() / 3).sum::<usize>();
        let seen = |world: &mut World, crowd: Option<Vec3>| {
            let mut cat = world.entity_mut(root).take::<Cat>().unwrap();
            assert!(pose(world, &kit, &slime, root, &mut cat, &[], 1., 1. / 60., crowd));
            let near = cat.near;
            world.entity_mut(root).insert(cat);
            (near, triangles(world), bounds(world, root))
        };
        // On its own, or close to the eye, it is the fine cat; one of a crowd two metres off is the coarse
        // one: under half the triangles in the same sixteen meshes, the same size to within a millimetre.
        let eye = |far: f32| Some(Vec3::new(0., FLOOR + 0.1, far));
        let fine = seen(&mut world, None);
        assert_eq!(seen(&mut world, eye(0.3)).1, fine.1);
        let coarse = seen(&mut world, eye(2.));
        println!("a far cat is {} triangles", coarse.1);
        assert!(fine.0 && !coarse.0 && coarse.1 * 2 < fine.1 && coarse.1 > 3000, "{} against {} triangles", coarse.1, fine.1);
        assert!((coarse.2.0 - fine.2.0).abs().max_element() < 0.0015 && (coarse.2.1 - fine.2.1).abs().max_element() < 0.0015);
        assert!(worn(&world).iter().all(|mesh| kit.far[1].contains(mesh)) && worn(&world).len() == 14);
        // It changes back a little nearer than it changed, and at once when the crowd is gone.
        assert!(!seen(&mut world, eye(FAR * 0.95)).0 && seen(&mut world, eye(FAR * 0.85)).0);
        assert!(!seen(&mut world, eye(FAR * 1.15)).0 && seen(&mut world, None).0);
        // A cat that is still ooze has only the one shape.
        drive(&mut world, root, 70., FORM * 0.6);
        assert_eq!(seen(&mut world, eye(2.)).1, seen(&mut world, eye(0.3)).1);
    }
    #[test]
    fn the_tail_is_one_tail_in_three_pieces() {
        let mut world = world();
        let root = spawn(&mut world, Vec3::new(0., FLOOR, 0.), 5);
        drive(&mut world, root, 70., 0.);
        settle(&mut world, root, &[], 2, |_, _| {});
        let cat = world.get::<Cat>(root).unwrap();
        let kit = world.resource::<Kit>();
        // Each piece begins inside the end of the one before, wherever the tail is lashing, and the
        // whole of it rises from the rump and stays behind the cat.
        for piece in 1..3 {
            let (before, here) = (cat.tail[piece - 1], cat.tail[piece]);
            let start = placed(&world, here, root, Vec3::ZERO);
            let nearest = points(&world, &kit.coats[cat.coat][TAIL + piece - 1]).into_iter().map(|p| placed(&world, before, root, p).distance(start)).fold(f32::MAX, f32::min);
            assert!(nearest < 0.006 * SIZE, "piece {piece} begins {nearest} m from the last");
        }
        let tip = points(&world, &kit.coats[cat.coat][TAIL + 2]).into_iter().map(|p| placed(&world, cat.tail[2], root, p)).fold(Vec3::splat(f32::MIN), Vec3::max);
        let base = placed(&world, cat.tail[0], root, Vec3::ZERO);
        assert!(tip.y > base.y + 0.02 && tip.z > 0.06 && base.z > 0.05, "tail from {base} to {tip}");
    }
    #[test]
    fn a_dead_cat_melts_into_ooze_and_is_gone() {
        let mut world = world();
        let root = spawn(&mut world, Vec3::new(0., FLOOR, 0.), 4);
        drive(&mut world, root, 70., 0.);
        settle(&mut world, root, &[], 2, |_, _| {});
        assert_eq!(world.get::<Cat>(root).unwrap().look, Look::Coat(0));
        kill(&mut world, root);
        settle(&mut world, root, &[], 60, |_, _| {});
        // A second after dying: flat, spread, and ooze again: the liquid's material on the bare shape.
        let cat = world.get::<Cat>(root).unwrap();
        let scale = world.get::<Transform>(cat.rig).unwrap().scale;
        assert!(scale.y < 0.1 * SIZE && scale.x > 1.2 * SIZE * 0.9, "{scale}");
        let ooze = world.resource::<ooze::Kit>().ooze.clone();
        assert_eq!((cat.look, paint(&world, cat.torso)), (Look::Slime, (ooze, false)));
        assert_eq!(world.get::<Transform>(cat.eyes).unwrap().scale.x, 0.);
        // Then it is gone, drops and all.
        let (kit, slime) = (world.resource::<Kit>().clone(), world.resource::<ooze::Kit>().clone());
        let mut cat = world.entity_mut(root).take::<Cat>().unwrap();
        let mut gone = false;
        for frame in 0..90 {
            if !pose(&mut world, &kit, &slime, root, &mut cat, &[], frame as f32 / 60., 1. / 60., None) {
                gone = true;
                break;
            }
        }
        assert!(gone, "a melted cat is removed");
    }
}

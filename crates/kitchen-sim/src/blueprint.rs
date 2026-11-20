// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The Forts bots' fortresses (a child module of forts.rs): what a team's bots build in the build phase, and how.
//!
//! A fort is built to a blueprint (`DESIGNS`: the keep, the gatehouse and the ravelin): a list of places for the
//! kitchen's pieces, each given in its team's base's own frame (u metres out from the base's front edge towards the
//! field, v metres across it), with the class of piece that goes there (`Class`: a wall panel, a block, a post, a
//! shield, a small topper or a napkin roof), which way its long side runs, whether it stands on another place's piece or
//! roofs over others, and a layer that orders the work: the foundations and walls first, then the posts and outworks,
//! then what goes on top, then the roofs. Every design rings or fronts the base outside it and leaves a gate, a clear way
//! on foot from the field into the base, for the team to bring the bacon home. One is chosen for each team at the start
//! of each build phase (`Feast::plan`), at random from the game's seed, the round and the team among those that fit the
//! map nearly as well as the best: each place is checked against the counter's edges, the map's blocks, decks and
//! ramps, the forcefield, the bases (nothing may stand in one), the pickups' pads, the zombies' spawn points and the
//! bacon's spot, and for room to stand to build it (as the bots will find it: `approach` asks the same, BODY clear of
//! what is built); places that do not fit are left out (with what stands on them), pieces lying where a place goes
//! count against it, and a design that would leave no way into the base is not used. The bots are to build what they
//! can finish, so the plan holds only what a bot can do: a kind may stand in for a place's first only where it fits
//! too (a bowl is too wide for a tin's place by the bread bin, or for a cereal wall's top beside the base); what another
//! stands on is no taller than a hotdog reaches up onto; a roof is put (moved a little, up to SHIFT, if need be) where
//! it rests steadily on what lies under its cloth, which is built first (`Place::under`), so that it locks there and
//! does not tip off; and a place whose piece would shut the bots' own way out of the fort on their level (`open_way`,
//! finer than a person needs) is left out. The other team's fort is the same drawn mirrored across the field, and a
//! design may be drawn mirrored across its own base as well.
//!
//! A bot builds it as a person would (`Feast::mason`): it walks to a free piece (the nearest of the kind the place
//! wants; when none of that kind is left, one of a kind of much the same size; any small one for a topper), picks it up,
//! turns and tips it as the place wants it (build.rs `Pose`), carries it to where it must stand for the piece to land
//! on its place, stops, faces it and puts it down (build.rs `Feast::put`), so that it snaps and locks as anybody's
//! would. It looks first at where the piece would land (`Feast::placement`): off by more than TOLERANCE (ROOFED for a
//! roof), on the wrong thing, at the wrong turn, or (a roof) not draping over what it roofs or reaching towards a
//! base, it steps over by the difference and looks again, NUDGES times, then from its other ways round, and after that
//! tries another place. A place whose piece lands wrong or is knocked off it is tried again, TRIES times (not at once
//! from where it stood: it waits WAIT seconds); one with no room to stand at, or no way there, waits WAIT seconds. A
//! piece is not put where it would shut a piece still wanted away from the base (`shuts_in`): those pieces are taken
//! away first; where it would shut the base itself in, the place has been tried. A place none of whose kinds is left
//! is given up. The bots of a team spread over the places (no two fetch for one place, nor the same piece), go only
//! for pieces they can walk to (`within_reach`), never cross the forcefield, and never put anything in a base: what
//! lies in the way of a place, where a bot must stand to build it, or on what it would land, is taken off first (from
//! the top of a pile down, `Plan::clear`), and a piece with no place to go is set down in the team's yard, clear of
//! the bases, the fort and its gate; a napkin, too big for that, on bare counter (`Plan::cloths`), never over pieces
//! (but napkins lying flat), where it would drape and shut them in. A piece buried under others (the plates under a bowl) is dug out the same
//! way. A bot never takes hold of a piece of its own fort in front of the one it is after.
//!
//! Bots find their way on a level with the pieces standing on the counter in it (`Feast::renav`, `Feast::wend`), so
//! that in the build and in the fight they go round the forts and in and out by the gates; the one that stands guard in
//! the fight stands inside its own fort, behind the wall by the gate (`Plan::posts`).
//!
//! The work is cheap: a plan is drawn once a round for each team (a few milliseconds), a bot looks for its next piece
//! only when it has none and at most four times a second, the pieces already built are looked over twice a second,
//! the bots' level is made again at most twice a second when something has moved, and `placement` (and `shuts_in`) is
//! asked once a put, and again only after a nudge.
use crate::drape::NAPKIN;
use crate::feast::build::{posed, Placement, Pose, HAIR, REACH_UP, STILL};
use crate::feast::Feast;
use crate::map::{Level, FLOOR, LOOSE, STEP};
use crate::props::State;
use glam::Vec3;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// How far a piece may land from its place's middle (metres), and how often a bot steps over to put it better.
pub const TOLERANCE: f64 = 0.035;
/// How far a roof may land from its place's middle, so long as it drapes over the place's pieces.
pub const ROOFED: f64 = 0.08;
pub const NUDGES: u8 = 3;
/// How many times a place is tried before the bots give up on it for the round.
pub const TRIES: u8 = 3;
/// Seconds a bot gives a fetch, and a carry to a place, before it gives up on them.
pub const FETCH: f32 = 12.;
pub const CARRY: f32 = 25.;
/// How far a built piece may be moved from its place (metres) before it is put back.
pub const SHIFTED: f64 = 0.06;
/// How near a piece (its outline, from above) a bot reaches out to pick it up.
pub const REACH: f64 = 0.11;
/// Seconds a bot that has come within a few centimetres of where it stands to put a piece down tries to get there
/// before it puts it down from where it is (or, further off, works out its way round again).
pub const SETTLE: f32 = 0.8;
/// Seconds a bot on its way to where it stands to put a piece down goes without getting any nearer before it tries
/// another way round.
pub const STUCK: f32 = 3.5;
/// Seconds a place waits after a bot has found no room to stand to put its piece down there, or no way to get there.
pub const WAIT: f32 = 8.;
/// How far a roof's cloth is kept from both bases (it hangs down over what it roofs, and may not reach into one).
pub const ROOF_CLEAR: f64 = 0.03;
/// How far a roof may be moved from where its design draws it (across the base's front and out from it), to rest on
/// what it roofs.
pub const SHIFT: f64 = 0.1;
/// How far inside what holds a roof up its middle may be (outside, by a centimetre, as `placement` locks a napkin), and
/// the room to land a little off its place that a roof's place is chosen for (inside what holds it up, and clear of the
/// bases), no more than LEEWAY counting.
pub const HELD: f64 = -0.01;
pub const LEEWAY: f64 = 0.03;
/// How wide (both ways) a piece is to hold a roof up alone.
pub const EDGE: f64 = 0.04;
/// How far below the most a hotdog reaches up onto (build.rs REACH_UP) the top of a piece that another stands on is.
pub const LEDGE: f64 = 0.01;
/// The spacing of the spots where a napkin may be set down out of the way (`Feast::cloths`), and how far over the
/// counter's edge it may hang there.
pub const CLOTH: f64 = 0.05;
pub const OVERHANG: f64 = 0.1;
/// The cells of the navigation-grid floods that ask whether a fort leaves a way into its base (`way_in`), and how far
/// round a piece a hotdog cannot stand (its radius and a little).
pub const CELL: f64 = 0.02;
pub const BODY: f64 = 0.034;
/// What the dice of a plan are thrown for (spawns.rs `Dice`).
const PLAN: u64 = 4;

/// What a place in a blueprint wants: a wall panel (a cutting board stood on its long edge), a block (a bread bin), a
/// post (a roll of paper towels on end), a shield (a plate stood on its rim), a small topper (a tin, a mug, a shaker, a
/// bowl) or a roof (a napkin, draped over other places' pieces).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Panel,
    Block,
    Post,
    Shield,
    Topper,
    Roof,
}
impl Class {
    /// The kinds of loose object (map.rs LOOSE) that serve, the best first and then those of much the same size, and
    /// how each is held for it (its tip, build.rs `Pose`).
    pub fn kinds(self) -> &'static [(&'static str, u8)] {
        match self {
            Class::Panel => &[("board", 1), ("cereal", 2), ("breadbox", 0), ("tray", 1), ("crate", 0), ("chair", 0)],
            Class::Block => &[("breadbox", 0), ("cereal", 2), ("board", 1), ("crate", 0), ("chair", 0)],
            Class::Post => &[("roll", 0), ("cereal", 0), ("tin", 0), ("crate", 0), ("pillow", 0)],
            Class::Shield => &[("plate", 1), ("tray", 1), ("cereal", 1), ("pillow", 0), ("crate", 0)],
            Class::Topper => &[("tin", 0), ("cup", 0), ("salt", 0), ("pepper", 0), ("dish", 0), ("pillow", 0), ("crate", 0)],
            Class::Roof => &[("napkin", 0)],
        }
    }
}
/// The kind (its number in map.rs LOOSE) of a name.
fn kind(name: &str) -> Option<usize> {
    LOOSE.iter().position(|k| *k == name)
}

/// A place in a design, in its base's frame: `u` metres out from the base's front edge (towards the field; the base is
/// below 0), `v` metres across it from its middle, or with `edge` from the base's side on v's side as a base 0.8 m
/// across has it (a wider or narrower base moves these out or in). `across`: its long side runs across the base's
/// front, rather than out from it. `on`: the place whose piece it stands on (-1: none; `u` and `v` are then from that
/// place's middle); `covers`: for a roof, the places it drapes over. `prefer`: the kind to fetch first, before its
/// class's own order.
#[derive(Clone, Copy, Debug)]
struct S {
    class: Class,
    prefer: &'static str,
    u: f64,
    v: f64,
    edge: bool,
    across: bool,
    layer: u8,
    on: i8,
    covers: [i8; 4],
}
const NONE: [i8; 4] = [-1; 4];
const fn put(class: Class, prefer: &'static str, u: f64, v: f64, edge: bool, across: bool, layer: u8) -> S {
    S { class, prefer, u, v, edge, across, layer, on: -1, covers: NONE }
}
/// A small topper on place `on`'s piece, `v` metres along it across the base's front (0: on its middle).
const fn top(prefer: &'static str, on: i8, v: f64, layer: u8) -> S {
    S { class: Class::Topper, prefer, u: 0., v, edge: false, across: true, layer, on, covers: NONE }
}
const fn roof(u: f64, v: f64, edge: bool, covers: [i8; 4]) -> S {
    S { class: Class::Roof, prefer: "napkin", u, v, edge, across: true, layer: 4, on: -1, covers }
}
use Class::{Block, Panel, Post, Shield, Topper};

/// A fort's design: its name, its places, its gate (the way in: `door` a point inside, in the base, and `gate` one
/// outside, the way between them clear), and where its guard stands in the fight.
pub struct Design {
    pub name: &'static str,
    slots: &'static [S],
    door: [f64; 2],
    gate: [f64; 2],
    posts: &'static [[f64; 2]],
}

/// The designs, drawn for a base 0.8 m across and 0.45 to 0.65 m deep against the end of the counter (a cutting board
/// stood on its long edge is a panel 45 cm long, 30 cm tall and 2.5 cm thick; a cereal box on its side one 33 cm long,
/// 23 cm tall and 8 cm thick, low enough to crown with tins and mugs; a hotdog is 15 cm tall and 6 cm across, and
/// reaches 30 cm up). Napkins are kept 23 cm or more clear of the base, which they must not reach into, and of anything
/// taller than what they roof.
pub const DESIGNS: [Design; 3] = [
    // The keep: a front wall of two boards with a gate between them, a paper towel roll either side of it with a cereal
    // bowl on each, cereal boxes standing as buttresses at its ends, a board along one side of the base and the bread bin
    // along the other with tins and a mug beside it, a napkin roofing the bin and its tins over; out in front a palisade
    // of plates on their rims with a gap before the gate, a napkin hung over each half.
    Design {
        name: "keep",
        slots: &[
            /* 0 */ put(Panel, "board", 0.06, 0.30, false, true, 0),
            /* 1 */ put(Panel, "board", 0.06, -0.30, false, true, 0),
            /* 2 */ put(Panel, "board", -0.178, 0.5125, true, false, 0),
            /* 3 */ put(Block, "breadbox", -0.153, -0.545, true, false, 0),
            /* 4 */ put(Post, "roll", 0.131, 0.133, false, true, 1),
            /* 5 */ put(Post, "roll", 0.131, -0.133, false, true, 1),
            /* 6 */ put(Post, "cereal", 0.113, 0.41, false, true, 1),
            /* 7 */ put(Post, "cereal", 0.113, -0.41, false, true, 1),
            /* 8 */ put(Topper, "tin", -0.04, -0.71, true, true, 1),
            /* 9 */ put(Topper, "cup", -0.15, -0.71, true, true, 1),
            /* 10 */ put(Topper, "tin", -0.26, -0.71, true, true, 1),
            /* 11 */ put(Shield, "plate", 0.34, 0.205, false, true, 2),
            /* 12 */ put(Shield, "plate", 0.34, -0.205, false, true, 2),
            /* 13 */ put(Shield, "plate", 0.34, 0.475, false, true, 2),
            /* 14 */ put(Shield, "plate", 0.34, -0.475, false, true, 2),
            /* 15 */ top("dish", 4, 0., 3),
            /* 16 */ top("dish", 5, 0., 3),
            /* 17 */ roof(-0.17, -0.665, true, [3, 8, 9, 10]),
            /* 18 */ roof(0.34, 0.34, false, [11, 13, -1, -1]),
            /* 19 */ roof(0.34, -0.34, false, [12, 14, -1, -1]),
        ],
        door: [-0.06, 0.],
        gate: [0.24, 0.],
        posts: &[[-0.1, 0.1], [-0.1, -0.1]],
    },
    // The gatehouse: a covered way in, between two boards stood on their long edges out from the base's front, with a
    // roll of paper towels either side of its mouth and a bowl on each; either side of it a low wall of cereal boxes on
    // their sides, crowned with tins, mugs and shakers; the bread bin as a bastion at one end with tins before it, roofed
    // with a napkin; plates on their rims out in front of the other end, with a napkin over them.
    Design {
        name: "gatehouse",
        slots: &[
            /* 0 */ put(Panel, "board", 0.23, 0.0875, false, false, 0),
            /* 1 */ put(Panel, "board", 0.23, -0.0875, false, false, 0),
            /* 2 */ put(Panel, "cereal", 0.045, 0.2655, false, true, 0),
            /* 3 */ put(Panel, "cereal", 0.045, -0.2655, false, true, 0),
            /* 4 */ put(Block, "breadbox", 0.13, -0.633, false, true, 1),
            /* 5 */ put(Panel, "cereal", 0.045, 0.597, false, true, 1),
            /* 6 */ put(Post, "roll", 0.40, 0.165, false, true, 1),
            /* 7 */ put(Post, "roll", 0.40, -0.165, false, true, 1),
            /* 8 */ top("tin", 2, 0.09, 2),
            /* 9 */ top("cup", 2, -0.07, 2),
            /* 10 */ top("tin", 3, -0.09, 2),
            /* 11 */ top("cup", 3, 0.07, 2),
            /* 12 */ top("salt", 5, -0.08, 2),
            /* 13 */ top("pepper", 5, 0.08, 2),
            /* 14 */ put(Topper, "tin", 0.31, -0.74, false, true, 2),
            /* 15 */ put(Topper, "tin", 0.31, -0.60, false, true, 2),
            /* 16 */ put(Shield, "plate", 0.36, 0.42, false, true, 2),
            /* 17 */ put(Shield, "plate", 0.36, 0.70, false, true, 2),
            /* 18 */ top("dish", 6, 0., 3),
            /* 19 */ top("dish", 7, 0., 3),
            /* 20 */ roof(0.24, -0.66, false, [4, 14, 15, -1]),
            /* 21 */ roof(0.36, 0.56, false, [16, 17, -1, -1]),
        ],
        door: [-0.06, 0.],
        gate: [0.55, 0.],
        posts: &[[-0.1, 0.12], [-0.1, -0.12]],
    },
    // The ravelin: a front wall of two boards with a gate between them, cereal boxes standing as buttresses at its ends,
    // and out before the gate a third board as a screen, so that the way in goes round it; rolls of paper towels with
    // bowls on them at the screen's ends; at the corners two bastions roofed with napkins, the bread bin with tins at one
    // and a cereal box on its side with mugs at the other; shakers and plates as low cover along the front.
    Design {
        name: "ravelin",
        slots: &[
            /* 0 */ put(Panel, "board", 0.06, 0.30, false, true, 0),
            /* 1 */ put(Panel, "board", 0.06, -0.30, false, true, 0),
            /* 2 */ put(Panel, "board", 0.27, 0., false, true, 1),
            /* 3 */ put(Post, "cereal", 0.113, 0.41, false, true, 1),
            /* 4 */ put(Post, "cereal", 0.113, -0.41, false, true, 1),
            /* 5 */ put(Post, "roll", 0.27, 0.29, false, true, 1),
            /* 6 */ put(Post, "roll", 0.27, -0.29, false, true, 1),
            /* 7 */ put(Block, "breadbox", 0.2775, -0.65, false, false, 1),
            /* 8 */ put(Topper, "tin", 0.50, -0.72, false, true, 2),
            /* 9 */ put(Topper, "tin", 0.50, -0.58, false, true, 2),
            /* 10 */ put(Panel, "cereal", 0.2475, 0.65, false, false, 1),
            /* 11 */ put(Topper, "cup", 0.48, 0.72, false, true, 2),
            /* 12 */ put(Topper, "cup", 0.48, 0.58, false, true, 2),
            /* 13 */ top("dish", 5, 0., 3),
            /* 14 */ top("dish", 6, 0., 3),
            /* 15 */ put(Shield, "plate", 0.47, 0.14, false, true, 2),
            /* 16 */ put(Shield, "plate", 0.47, -0.14, false, true, 2),
            /* 17 */ put(Topper, "salt", 0.42, 0.40, false, true, 2),
            /* 18 */ put(Topper, "pepper", 0.42, -0.40, false, true, 2),
            /* 19 */ roof(0.38, -0.65, false, [7, 8, 9, -1]),
            /* 20 */ roof(0.38, 0.65, false, [10, 11, 12, -1]),
        ],
        door: [-0.06, 0.],
        gate: [0.17, 0.],
        posts: &[[-0.1, 0.1], [-0.1, -0.1]],
    },
];

/// A place of a team's plan, fitted to its map: what it wants (its class, and the kinds that serve with how each is
/// tipped, the design's choice first), where the middle of its piece's outline goes ([x, z]), the yaw of the frame its
/// long side runs along (a piece's own x axis, build.rs `Pose`), its layer, the place it stands on and those it
/// roofs, the outline [x0, z0, x1, z1] a piece of the first kind the team has takes there, and how it stands: the
/// piece built there (once it is), whether the bots have given up on it, how many times a piece put down there has
/// landed wrong or come off it, and until when it waits.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub class: Class,
    pub kinds: Vec<(usize, u8)>,
    pub at: [f64; 2],
    pub yaw: f64,
    pub layer: u8,
    pub on: Option<usize>,
    pub covers: Vec<usize>,
    /// For a roof: the other places under its cloth, which it lies on as well, built (or given up) before it.
    pub under: Vec<usize>,
    pub rect: [f64; 4],
    pub prop: Option<usize>,
    pub failed: bool,
    pub tries: u8,
    /// Not to be tried again before then (seconds on the round's clock): a bot found no room to stand to put its piece
    /// down there, or no way to get there.
    pub wait: f32,
}
impl Place {
    /// Whether it is still to be built.
    pub fn open(&self) -> bool {
        self.prop.is_none() && !self.failed
    }
    /// How a piece of `kind` is tipped for it, if that kind serves.
    pub fn tip(&self, kind: usize) -> Option<u8> {
        self.kinds.iter().find(|(k, _)| *k == kind).map(|(_, tip)| *tip)
    }
}
/// A team's fort as it is to be built this round: the design (its number in DESIGNS, and whether it is drawn mirrored
/// across the base), its places (those that fit the map), its door and gate ([x, z], see `Design`), where its guard
/// stands, the box round the base and its walls (`hull`: in it is inside the fort), the yard where pieces are set down
/// out of the way, the pieces the bots have set down (or that came off their places), free to be fetched again, and
/// when the built places were last looked over.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub team: usize,
    pub design: usize,
    pub flipped: bool,
    pub places: Vec<Place>,
    pub door: [f64; 2],
    pub gate: [f64; 2],
    /// The box round the way in, from the door out through the gate and on towards the field, kept clear, and the point
    /// at its far end, outside the fort.
    pub approach: [f64; 4],
    pub outside: [f64; 2],
    pub posts: Vec<[f64; 2]>,
    pub hull: [f64; 4],
    pub yard: Vec<[f64; 2]>,
    /// Where a napkin may be set down out of the way (`Feast::cloths`): the yard's spots are too cramped for one.
    pub cloths: Vec<[f64; 2]>,
    pub loose: Vec<usize>,
    /// Pieces to be taken away (`Feast::choose` offers them first): lying where the bots must stand to build a place, or
    /// where one would land on them, or where a place built would shut them away from the base.
    pub clear: Vec<usize>,
    pub looked: f32,
}
impl Plan {
    /// Whether a place's piece may be built at `now`: `ready`, and not waiting.
    pub fn due(&self, k: usize, now: f32) -> bool {
        self.ready(k) && self.places[k].wait <= now
    }
    /// Whether a place's piece may be built now: open, and what it stands on (or roofs over) built.
    pub fn ready(&self, k: usize) -> bool {
        let p = &self.places[k];
        let covered = p.covers.is_empty() || (p.covers.iter().all(|c| !self.places[*c].open()) && p.covers.iter().any(|c| self.places[*c].prop.is_some()));
        p.open() && p.on.is_none_or(|on| self.places[on].prop.is_some()) && covered && p.under.iter().all(|u| !self.places[*u].open())
    }
    /// Whether [x, z] is inside the fort.
    pub fn inside(&self, p: [f64; 2]) -> bool {
        p[0] >= self.hull[0] && p[0] <= self.hull[2] && p[1] >= self.hull[1] && p[1] <= self.hull[3]
    }
    /// How many places have their piece.
    pub fn built(&self) -> usize {
        self.places.iter().filter(|p| p.prop.is_some()).count()
    }
}

/// A base's frame: the middle of its front edge, the way out of it (towards the field) and the way across it, and
/// half its width; `flip` -1 draws the design mirrored across the base.
struct Frame {
    front: [f64; 2],
    out: [f64; 2],
    across: [f64; 2],
    half: f64,
    flip: f64,
}
impl Frame {
    fn new(level: &Level, team: usize, flipped: bool) -> Option<Frame> {
        let fort = level.fort.as_ref()?;
        let (r, side, flip) = (fort.bases[team], fort.sides[team] as f64, if flipped { -1. } else { 1. });
        Some(if fort.axis == 1 {
            Frame { front: [(r[0] + r[2]) / 2., if side < 0. { r[3] } else { r[1] }], out: [0., -side], across: [1., 0.], half: (r[2] - r[0]) / 2., flip }
        } else {
            Frame { front: [if side < 0. { r[2] } else { r[0] }, (r[1] + r[3]) / 2.], out: [-side, 0.], across: [0., 1.], half: (r[3] - r[1]) / 2., flip }
        })
    }
    fn world(&self, u: f64, v: f64) -> [f64; 2] {
        let v = v * self.flip;
        [self.front[0] + self.out[0] * u + self.across[0] * v, self.front[1] + self.out[1] * u + self.across[1] * v]
    }
    fn v(&self, s: &S) -> f64 {
        if s.edge { s.v + s.v.signum() * (self.half - 0.4) } else { s.v }
    }
}
/// A point [u, w] of the frame turned by `yaw` (as a prop of that yaw has its own) in the world: build.rs `out`.
pub(crate) fn out(u: f64, w: f64, yaw: f64) -> [f64; 2] {
    let (s, c) = yaw.sin_cos();
    [u * c + w * s, -u * s + w * c]
}
/// The yaw (to the nearest eighth of a turn) of the frame whose x axis runs along [dx, dz].
fn yaw_along(d: [f64; 2]) -> f64 {
    ((-d[1]).atan2(d[0]) / FRAC_PI_4).round() * FRAC_PI_4
}
/// The box round an outline [x0, z0, x1, z1] (about its origin) with its middle at `at`, turned by `yaw`.
fn footprint(own: [f64; 4], at: [f64; 2], yaw: f64) -> [f64; 4] {
    let (hx, hz) = ((own[2] - own[0]) / 2., (own[3] - own[1]) / 2.);
    let mut r = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for [a, b] in [[-hx, -hz], [hx, -hz], [hx, hz], [-hx, hz]] {
        let [x, z] = out(a, b, yaw);
        r = [r[0].min(at[0] + x), r[1].min(at[1] + z), r[2].max(at[0] + x), r[3].max(at[1] + z)];
    }
    r
}
fn overlaps(a: [f64; 4], b: [f64; 4], by: f64) -> bool {
    a[2].min(b[2]) - a[0].max(b[0]) > by && a[3].min(b[3]) - a[1].max(b[1]) > by
}
fn grown(r: [f64; 4], by: f64) -> [f64; 4] {
    [r[0] - by, r[1] - by, r[2] + by, r[3] + by]
}
/// How far [x, z] is from a box (0 inside it).
fn apart(r: [f64; 4], p: [f64; 2]) -> f64 {
    (r[0] - p[0]).max(0.).max(p[0] - r[2]).hypot((r[1] - p[1]).max(0.).max(p[1] - r[3]))
}

/// Whether a hotdog can walk from `from` ([x, z]) into the base `base` on `level` with the boxes `rects` standing on it
/// as well: a flood over cells CELL across of the level's clear navigation cells, leaving out those within BODY of a
/// box. (The plan asks it of a design's places; the tests of what the bots built.)
pub fn way_in(level: &Level, rects: &[[f64; 4]], base: [f64; 4], from: [f64; 2]) -> bool {
    let (cols, rows) = (((level.x1 - level.x0) / CELL).ceil() as usize, ((level.z1 - level.z0) / CELL).ceil() as usize);
    let centre = |i: usize, j: usize| [level.x0 + (i as f64 + 0.5) * CELL, level.z0 + (j as f64 + 0.5) * CELL];
    let walls: Vec<[f64; 4]> = rects.iter().map(|r| grown(*r, BODY)).collect();
    let open = |i: usize, j: usize| {
        let [x, z] = centre(i, j);
        level.clear(x, z) && !walls.iter().any(|w| x > w[0] && x < w[2] && z > w[1] && z < w[3])
    };
    let cell = |p: [f64; 2]| (((p[0] - level.x0) / CELL).floor().clamp(0., cols as f64 - 1.) as usize, ((p[1] - level.z0) / CELL).floor().clamp(0., rows as f64 - 1.) as usize);
    let mut seen = vec![false; cols * rows];
    let (i, j) = cell(from);
    if !open(i, j) {
        return false;
    }
    let mut todo = vec![(i, j)];
    seen[j * cols + i] = true;
    while let Some((i, j)) = todo.pop() {
        let [x, z] = centre(i, j);
        if x >= base[0] && x <= base[2] && z >= base[1] && z <= base[3] {
            return true;
        }
        for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (ni, nj) = (i as i64 + di, j as i64 + dj);
            if ni < 0 || nj < 0 || ni >= cols as i64 || nj >= rows as i64 {
                continue;
            }
            let (ni, nj) = (ni as usize, nj as usize);
            if !seen[nj * cols + ni] && open(ni, nj) {
                seen[nj * cols + ni] = true;
                todo.push((ni, nj));
            }
        }
    }
    false
}

impl Feast {
    /// A team's plan for this round (None until its bots first look for it in the build phase, and off a Forts map).
    pub fn plan_of(&self, team: usize) -> Option<&Plan> {
        self.fort.as_ref().and_then(|fort| fort.plans.get(team)).and_then(|plan| plan.as_ref())
    }
    /// Draws a team's plan for the round (see the module's documentation): every design, plain and mirrored, is fitted
    /// to the map, and one is chosen at random, from the game's seed, the round and the team, among those that leave a
    /// way into the base and fit nearly as well as the best (places weighed by their layer: the foundations most).
    pub(crate) fn plan(&self, team: usize) -> Option<Plan> {
        let mut fits: Vec<(f64, Plan)> = vec![];
        for design in 0..DESIGNS.len() {
            for flipped in [false, true] {
                if let Some((score, plan)) = self.fit_plan(team, design, flipped) {
                    fits.push((score, plan));
                }
            }
        }
        // Each that might be chosen must leave a way into the base (`leaves_way_in`), and the bots their way out of the
        // fort (`open_way`), which can cost it places and so lower the best: the rest are looked at then (as if all had
        // been, at the cost of only a few).
        let mut looked = vec![false; fits.len()];
        let accept = |fits: &[(f64, Plan)]| {
            let best = fits.iter().map(|(score, _)| *score).fold(0., f64::max);
            move |score: f64| score > 0. && score >= best * 0.85 - 1e-9
        };
        loop {
            let ok = accept(&fits);
            let todo: Vec<usize> = (0..fits.len()).filter(|k| !looked[*k] && ok(fits[*k].0)).collect();
            if todo.is_empty() {
                break;
            }
            for k in todo {
                looked[k] = true;
                let total: f64 = DESIGNS[fits[k].1.design].slots.iter().map(|s| Self::credit(s.layer, false)).sum();
                fits[k].0 = if self.leaves_way_in(&fits[k].1) { self.open_way(&mut fits[k].1).map_or(-1., |lost| fits[k].0 - lost / total) } else { -1. };
            }
        }
        let ok = accept(&fits);
        let mut fits: Vec<Plan> = fits.into_iter().filter(|(score, _)| ok(*score)).map(|(_, plan)| plan).collect();
        if fits.is_empty() {
            return None;
        }
        let mut dice = crate::spawns::Dice::new(&[self.card_seed, self.round as u64, PLAN, team as u64]);
        let mut plan = fits.swap_remove(dice.below(fits.len()));
        self.furnish(&mut plan);
        Some(plan)
    }
    /// The chosen plan's kinds that may stand in for its places' first (`sift`), its yard and the spots for napkins set
    /// aside (`cloths`): only the plan the bots build needs them.
    pub(crate) fn furnish(&self, plan: &mut Plan) {
        self.sift(plan);
        (plan.yard, plan.cloths) = (self.yard(plan), self.cloths(plan));
    }
    /// Of the kinds a place may take (its first and those the team has that may stand in for it, `fit_plan`), only those
    /// that fit there as the first does: on the counter and the team's side, out of both bases, clear of the field and
    /// (on the counter itself) of the map's blocks, decks, ramps and sinks, the pickups' pads, the spawn points, the
    /// bacon's spot and every other place, with room to stand to put it down, clear of what is built before it. (A bowl
    /// is too wide for a tin's place by the bread bin, or for a cereal wall's top beside the base.)
    fn sift(&self, plan: &mut Plan) {
        let level = &self.level;
        let Some(fort) = level.fort.as_ref() else {
            return;
        };
        let side = fort.sides[plan.team];
        for k in 0..plan.places.len() {
            let p = &plan.places[k];
            if p.class == Class::Roof || p.kinds.len() < 2 {
                continue;
            }
            let ground = p.on.is_none();
            let wall = |o: usize, q: &Place| o != k && q.on.is_none() && q.class != Class::Roof;
            let before: Vec<[f64; 4]> = plan.places.iter().enumerate().filter(|(o, q)| wall(*o, q) && (q.layer < p.layer || (q.layer == p.layer && ground && *o < k))).map(|(_, q)| q.rect).collect();
            let others: Vec<[f64; 4]> = plan.places.iter().enumerate().filter(|(o, q)| wall(*o, q)).map(|(_, q)| q.rect).collect();
            let fits = |kind: usize, tip: u8| {
                let (own, _, _, _) = posed(self.props.shape(kind), Pose { turn: 0, tip });
                let rect = footprint(own, p.at, p.yaw);
                rect[0] >= level.x0 + 0.005 && rect[1] >= level.z0 + 0.005 && rect[2] <= level.x1 - 0.005 && rect[3] <= level.z1 - 0.005
                    && !fort.bases.iter().any(|b| overlaps(rect, *b, 0.))
                    && [[rect[0], rect[1]], [rect[2], rect[1]], [rect[2], rect[3]], [rect[0], rect[3]]].iter().all(|c| level.side(c[0], c[1]) == side)
                    && !overlaps(rect, grown(fort.field, 0.03), 0.)
                    && (!ground || {
                        let flat = (0..=4).all(|i| (0..=4).all(|j| level.height(rect[0] + (rect[2] - rect[0]) * i as f64 / 4., rect[1] + (rect[3] - rect[1]) * j as f64 / 4.) == 0.));
                        flat && !level.map.blocks.iter().any(|b| overlaps(rect, grown(b.rect, 0.01), 0.))
                            && level.map.pickups.iter().all(|item| apart(rect, item.p) >= 0.1)
                            && level.map.spawns.iter().all(|spawn| apart(rect, spawn.p) >= 0.12)
                            && apart(rect, fort.bacon) >= 0.2
                            && !others.iter().any(|r| overlaps(rect, *r, 0.004))
                    })
                    && self.standable(kind, tip, p.at, p.yaw, &before)
            };
            let keep: Vec<(usize, u8)> = p.kinds.iter().enumerate().filter(|(n, (kind, tip))| *n == 0 || fits(*kind, *tip)).map(|(_, kt)| *kt).collect();
            plan.places[k].kinds = keep;
        }
    }
    /// Where a team's bots set pieces down out of the way: spots on its side, beside the base or out in front, on the
    /// counter, clear of everything (of the plan's places and its way in, too).
    fn yard(&self, plan: &Plan) -> Vec<[f64; 2]> {
        let level = &self.level;
        let (Some(fort), Some(frame)) = (level.fort.as_ref(), Frame::new(level, plan.team, plan.flipped)) else {
            return vec![];
        };
        let (side, field, places, approach) = (fort.sides[plan.team], fort.field, &plan.places, plan.approach);
        let (x0, z0, x1, z1) = (level.x0, level.z0, level.x1, level.z1);
        let mut yard = vec![];
        for i in 0..28 {
            for j in 0..25 {
                let (u, v) = (-0.6 + 0.1 * i as f64, (j as f64 - 12.) * 0.1);
                let p = frame.world(u, v);
                let ok = p[0] >= x0 + 0.12 && p[0] <= x1 - 0.12 && p[1] >= z0 + 0.12 && p[1] <= z1 - 0.12
                    && level.side(p[0], p[1]) == side
                    && apart(field, p) >= 0.2
                    && [[0., 0.], [0.09, 0.], [-0.09, 0.], [0., 0.09], [0., -0.09]].iter().all(|o| level.height(p[0] + o[0], p[1] + o[1]) == 0. && level.clear(p[0] + o[0], p[1] + o[1]))
                    && level.map.pickups.iter().all(|item| (item.p[0] - p[0]).hypot(item.p[1] - p[1]) >= 0.2)
                    && level.map.spawns.iter().all(|spawn| (spawn.p[0] - p[0]).hypot(spawn.p[1] - p[1]) >= 0.2)
                    && (fort.bacon[0] - p[0]).hypot(fort.bacon[1] - p[1]) >= 0.25
                    && fort.bases.iter().all(|b| apart(*b, p) >= 0.15)
                    && places.iter().all(|q| apart(q.rect, p) >= 0.12)
                    && apart(approach, p) >= 0.12;
                if ok {
                    yard.push(p);
                }
            }
        }
        yard
    }
    /// Where a team's napkin may be set down out of the way: a grid of spots on its side (CLOTH apart) where a napkin
    /// lying flat on the counter keeps off the bases, the field, the map's blocks, decks and ramps, the bacon's spot and
    /// every place of the plan, and hangs over the counter's edge by no more than OVERHANG (it would drape over the edge
    /// by drape.rs HANG_COUNTER). It may lie over the way in, which it does not block. (A napkin set down on top of
    /// anything drapes over it and shuts it in: it never is, `yard_spot`.)
    fn cloths(&self, plan: &Plan) -> Vec<[f64; 2]> {
        let (level, team) = (&self.level, plan.team);
        let (Some(fort), Some(napkin)) = (level.fort.as_ref(), (0..self.props.len()).find(|i| self.props.body(*i).shape == NAPKIN)) else {
            return vec![];
        };
        let (own, _, _, _) = posed(self.props.shape(self.props.body(napkin).shape), Pose::default());
        let half = ((own[2] - own[0]).max(own[3] - own[1])) / 2. + 0.015;
        let raised: Vec<[f64; 4]> = level.map.blocks.iter().map(|b| b.rect).chain(level.map.decks.iter().map(|d| d.rect)).chain(level.map.ramps.iter().map(|r| r.rect)).collect();
        let mut spots = vec![];
        let (cols, rows) = (((level.x1 - level.x0) / CLOTH) as usize, ((level.z1 - level.z0) / CLOTH) as usize);
        for i in 0..=cols {
            for j in 0..=rows {
                let p = [level.x0 + i as f64 * CLOTH, level.z0 + j as f64 * CLOTH];
                let room = [p[0] - half, p[1] - half, p[0] + half, p[1] + half];
                let ok = room[0] >= level.x0 - OVERHANG && room[1] >= level.z0 - OVERHANG && room[2] <= level.x1 + OVERHANG && room[3] <= level.z1 + OVERHANG
                    && level.side(p[0], p[1]) == fort.sides[team]
                    && [[room[0], room[1]], [room[2], room[1]], [room[2], room[3]], [room[0], room[3]]].iter().all(|c| level.side(c[0], c[1]) == fort.sides[team])
                    && level.height(p[0], p[1]) == 0.
                    && !overlaps(room, grown(fort.field, 0.03), 0.)
                    && !fort.bases.iter().any(|b| overlaps(room, grown(*b, 0.03), 0.))
                    && !raised.iter().any(|r| overlaps(room, grown(*r, 0.02), 0.))
                    && apart(room, fort.bacon) >= 0.1
                    && !plan.places.iter().any(|q| overlaps(room, q.rect, 0.));
                if ok {
                    spots.push(p);
                }
            }
        }
        spots
    }
    /// How steadily a roof lying over `rect` with its middle at `at` rests on the places under it (among the plan's
    /// `places` so far, whose tops stand `tops` high, `tops`) as their first kinds stand: None unless the middles of
    /// those it roofs (`covers`) are under it;
    /// otherwise how far inside the highest of them (on which it lies) its middle is, so that they hold it up on both
    /// sides (HELD or more, as a napkin put down there is locked: build.rs `placement`); None too when the highest is one
    /// piece less than EDGE wide.
    fn rests(places: &[Place], tops: &[f64], covers: &[usize], rect: [f64; 4], at: [f64; 2]) -> Option<f64> {
        let inside = |p: [f64; 2]| p[0] > rect[0] + 0.005 && p[0] < rect[2] - 0.005 && p[1] > rect[1] + 0.005 && p[1] < rect[3] - 0.005;
        if !covers.iter().all(|c| inside(places[*c].at)) {
            return None;
        }
        // (What is under it, which it lies on: what it roofs, and any other place's piece under its cloth.)
        let under: Vec<(usize, f64)> = (0..places.len()).filter(|k| places[*k].class != Class::Roof && overlaps(places[*k].rect, rect, 0.)).map(|k| (k, tops[k])).collect();
        let level = under.iter().map(|(_, h)| *h).fold(f64::NEG_INFINITY, f64::max);
        let top: Vec<[f64; 4]> = under.iter().filter(|(_, h)| *h > level - 0.005).map(|(k, _)| places[*k].rect).collect();
        // (Not on one thin piece alone, a plate on its rim: on that edge it tips off whichever way it lands.)
        if let [r] = top[..] {
            if (r[2] - r[0]).min(r[3] - r[1]) < EDGE {
                return None;
            }
        }
        let held = top.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |a, r| [a[0].min(r[0].max(rect[0])), a[1].min(r[1].max(rect[1])), a[2].max(r[2].min(rect[2])), a[3].max(r[3].min(rect[3]))]);
        Some((at[0] - held[0]).min(held[2] - at[0]).min(at[1] - held[1]).min(held[3] - at[1]))
    }
    /// How far apart two boxes [x0, z0, x1, z1] are (0 where they overlap).
    fn gap(a: [f64; 4], b: [f64; 4]) -> f64 {
        (b[0] - a[2]).max(a[0] - b[2]).max(0.).hypot((b[1] - a[3]).max(a[1] - b[3]).max(0.))
    }
    /// What a place of `layer` counts for in how well a design fits a map (the foundations most), and less when pieces
    /// lie where it goes as the round starts (`lying`), which are to be cleared first.
    fn credit(layer: u8, lying: bool) -> f64 {
        let w = match layer {
            0 => 3.,
            1 => 2.,
            _ => 1.,
        };
        if lying { w * 0.6 } else { w }
    }
    /// Leaves the bots their own way out of the fort, on their level (as `renav` makes it) with every piece of the plan
    /// standing and every roof's wall: while it is shut, the place of the latest layer whose leaving out opens it is left
    /// out (with what stands on it, and a roof over it or lying on it). How much of the design's fit (`credit`) that
    /// cost, or None when no one place opens it (the design leaves the bots no way in).
    fn open_way(&self, plan: &mut Plan) -> Option<f64> {
        let places = &plan.places;
        if self.bots_way(plan.team, places, &vec![false; places.len()], plan.outside) {
            return Some(0.);
        }
        let mut order: Vec<usize> = (0..places.len()).filter(|k| places[*k].on.is_none() && places[*k].class != Class::Roof).collect();
        order.sort_by_key(|k| (std::cmp::Reverse(places[*k].layer), std::cmp::Reverse(*k)));
        let gone = order.into_iter().map(|k| Self::left_out(places, k)).find(|gone| self.bots_way(plan.team, places, gone, plan.outside))?;
        let lying: Vec<[f64; 4]> = self.extents().into_iter().flatten().map(|e| [e[0], e[1], e[2], e[3]]).collect();
        let lost = (0..places.len()).filter(|k| gone[*k]).map(|k| {
            let p = &places[k];
            Self::credit(p.layer, p.on.is_none() && p.class != Class::Roof && lying.iter().any(|e| overlaps(grown(p.rect, 0.01), *e, 0.)))
        }).sum();
        let keep: Vec<usize> = (0..places.len()).filter(|k| !gone[*k]).collect();
        let index: Vec<Option<usize>> = (0..places.len()).map(|k| keep.iter().position(|o| *o == k)).collect();
        plan.places = keep
            .iter()
            .map(|k| {
                let mut p = plan.places[*k].clone();
                (p.on, p.covers, p.under) = (p.on.and_then(|o| index[o]), p.covers.iter().filter_map(|c| index[*c]).collect(), p.under.iter().filter_map(|c| index[*c]).collect());
                p
            })
            .collect();
        let base = self.level.fort.as_ref()?.bases[plan.team];
        plan.hull = plan.places.iter().filter(|p| matches!(p.class, Class::Panel | Class::Block) && p.on.is_none()).fold(base, |h, p| [h[0].min(p.rect[0]), h[1].min(p.rect[1]), h[2].max(p.rect[2]), h[3].max(p.rect[3])]);
        Some(lost)
    }
    /// The places of a plan left out with place `k`: those standing on one left out, and a roof over one.
    fn left_out(places: &[Place], k: usize) -> Vec<bool> {
        let mut gone = vec![false; places.len()];
        gone[k] = true;
        // (A place stands on, and a roof covers, only places before it.)
        for o in 0..places.len() {
            gone[o] = gone[o] || places[o].on.is_some_and(|on| gone[on]) || places[o].covers.iter().chain(&places[o].under).any(|c| gone[*c]);
        }
        gone
    }
    /// Whether a team's bots could go from the middle of its base out of its fort's gate to `outside` on their level
    /// (`renav`, `nav_level`, with the field up) with the plan's places that are not `gone` built: every piece on the
    /// counter as it stands (its first kind), and every roof's wall over what it roofs.
    fn bots_way(&self, team: usize, places: &[Place], gone: &[bool], outside: [f64; 2]) -> bool {
        let (Some(fort), Some(level_fort)) = (self.fort.as_ref(), self.level.fort.as_ref()) else {
            return true;
        };
        let pieces: Vec<[f64; 5]> = (0..places.len())
            .filter(|k| !gone[*k] && places[*k].on.is_none())
            .map(|k| {
                let p = &places[k];
                if p.class == Class::Roof {
                    let r = p.covers.iter().filter(|c| !gone[**c]).map(|c| places[*c].rect).fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
                    let r = grown(r, crate::drape::SKIRT);
                    [r[0], r[1], r[2], r[3], FLOOR + REACH_UP]
                } else {
                    [p.rect[0], p.rect[1], p.rect[2], p.rect[3], FLOOR + self.top_of(p)]
                }
            })
            .collect();
        let nav = self.nav_level(&pieces, vec![fort.field.clone()]);
        let base = level_fort.bases[team];
        match (open_cell(&nav, [(base[0] + base[2]) / 2., (base[1] + base[3]) / 2.], 0.32), open_cell(&nav, outside, 0.32)) {
            (Some(a), Some(b)) => nav.joined(a, b),
            _ => false,
        }
    }
    /// How high the tops of the places' pieces stand (of their first kinds), with what they stand on.
    fn tops(&self, places: &[Place]) -> Vec<f64> {
        let mut tops: Vec<f64> = vec![];
        for p in places {
            let top = self.top_of(p) + p.on.map_or(0., |o| tops[o]);
            tops.push(top);
        }
        tops
    }
    /// How tall a piece of `kind` stands tipped `tip`.
    fn tall(&self, kind: usize, tip: u8) -> f64 {
        posed(self.props.shape(kind), Pose { turn: 0, tip }).1
    }
    /// How high the top of a place's piece stands (of its first kind) above what it stands on.
    fn top_of(&self, place: &Place) -> f64 {
        place.kinds.first().map_or(f64::INFINITY, |(k, tip)| self.tall(*k, *tip))
    }
    /// Whether a carrier has room to stand to put a piece of `kind` (tipped `tip`) down with its middle at `at`, turned
    /// `yaw`, one way round or another: on the counter, on open ground as flat as the place's, clear of the boxes
    /// `built` (by BODY).
    fn standable(&self, kind: usize, tip: u8, at: [f64; 2], yaw: f64, built: &[[f64; 4]]) -> bool {
        let Some(prop) = (0..self.props.len()).find(|i| self.props.body(*i).shape == kind) else {
            return false;
        };
        let ground = self.level.height(at[0], at[1]);
        (-1..=2).any(|q| {
            let facing = q as f64 * FRAC_PI_2;
            let turn = (((yaw - facing) / FRAC_PI_4).round() as i32).rem_euclid(8) as u8;
            let [x, z] = self.stand(prop, Pose { turn, tip }, at, facing);
            x > self.level.x0 + 0.04 && x < self.level.x1 - 0.04 && z > self.level.z0 + 0.04 && z < self.level.z1 - 0.04
                && self.level.clear(x, z)
                && (self.level.height(x, z) - ground).abs() < 0.01
                && built.iter().all(|r| apart(*r, [x, z]) > BODY)
        })
    }
    /// A design fitted to a team's base: how well it fits (the share of its places that do, by weight), whether it
    /// leaves a way into the base, and the plan. (`plan` asks the way in only of those it might choose.)
    #[cfg(test)]
    fn fit_design(&self, team: usize, design: usize, flipped: bool) -> Option<(f64, bool, Plan)> {
        let (score, plan) = self.fit_plan(team, design, flipped)?;
        Some((score, self.leaves_way_in(&plan), plan))
    }
    /// Whether a plan leaves a way on foot from the bacon into its base, round every piece of it on the counter and a
    /// roof over everything it covers (`way_in`).
    fn leaves_way_in(&self, plan: &Plan) -> bool {
        let (Some(fort), Some(open)) = (self.level.fort.as_ref(), self.fort.as_ref().map(|fort| &fort.open)) else {
            return false;
        };
        let places = &plan.places;
        let rects: Vec<[f64; 4]> = places
            .iter()
            .filter(|p| p.on.is_none())
            .map(|p| {
                if p.class == Class::Roof {
                    grown(p.covers.iter().map(|c| places[*c].rect).fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]), 0.01)
                } else {
                    p.rect
                }
            })
            .collect();
        way_in(open, &rects, fort.bases[plan.team], fort.bacon)
    }
    /// A design fitted to a team's base (`fit_design`), without asking whether it leaves a way in.
    fn fit_plan(&self, team: usize, design: usize, flipped: bool) -> Option<(f64, Plan)> {
        let (level, d) = (&self.level, &DESIGNS[design]);
        let fort = level.fort.as_ref()?;
        let frame = Frame::new(level, team, flipped)?;
        let side = fort.sides[team];
        // The kinds this team has on its side of the field.
        let stock: Vec<usize> = level.map.loose.iter().filter(|l| level.side(l.p[0], l.p[1]) == side).filter_map(|l| kind(&l.kind)).collect();
        let (x0, z0, x1, z1) = (level.x0, level.z0, level.x1, level.z1);
        let field = fort.field;
        let mut places: Vec<Place> = vec![];
        // A design's place number to its number in the plan (None: it does not fit).
        let mut number: Vec<Option<usize>> = vec![];
        let (mut weight, mut fitted) = (0., 0.);
        // (The pieces lying about as the round starts: a place they lie in is to be cleared first, which takes time.)
        let lying: Vec<[f64; 4]> = self.extents().into_iter().flatten().map(|e| [e[0], e[1], e[2], e[3]]).collect();
        for s in d.slots {
            weight += Self::credit(s.layer, false);
            let mut kinds: Vec<(usize, u8)> = vec![];
            for (name, tip) in std::iter::once((s.prefer, s.class.kinds().iter().find(|(k, _)| *k == s.prefer).map_or(0, |(_, t)| *t))).chain(s.class.kinds().iter().copied()) {
                if let Some(k) = kind(name).filter(|k| !kinds.iter().any(|(o, _)| o == k)) {
                    kinds.push((k, if k == NAPKIN { 0 } else { tip }));
                }
            }
            let on = (s.on >= 0).then(|| number.get(s.on as usize).copied().flatten());
            let covers: Vec<usize> = s.covers.iter().filter(|c| **c >= 0).filter_map(|c| number.get(*c as usize).copied().flatten()).collect();
            // (What it stands on, or roofs over, does not fit: nor does it.)
            if on == Some(None) || (s.class == Class::Roof && covers.is_empty()) {
                number.push(None);
                continue;
            }
            let on = on.flatten();
            let yaw = yaw_along(if s.across { [frame.across[0] * frame.flip, frame.across[1] * frame.flip] } else { frame.out });
            let at = match on {
                Some(o) => {
                    let ([x, z], [x0, z0]) = (frame.world(s.u, s.v), frame.world(0., 0.));
                    [places[o].at[0] + x - x0, places[o].at[1] + z - z0]
                }
                None => frame.world(s.u, frame.v(s)),
            };
            let roof = s.class == Class::Roof;
            let hang = if roof { -0.1 } else { 0.005 };
            let tops = if roof { self.tops(&places) } else { vec![] };
            // Somewhere to stand to put it down, clear of what is built before it.
            let before: Vec<[f64; 4]> = places.iter().filter(|q| q.on.is_none() && q.class != Class::Roof && (q.layer < s.layer || (q.layer == s.layer && on.is_none() && !roof))).map(|q| q.rect).collect();
            // Where a piece of a kind (tipped `tip`) lies here, if it fits: on the counter and its team's side, out of
            // both bases (a roof's cloth, which hangs down over what it roofs, ROOF_CLEAR clear of them), clear of the
            // field, and (on the counter itself) of the map's blocks, decks, ramps and sinks, the pickups' pads, the
            // zombies' spawn points, the bacon's spot and the places already fitted; on a place's piece no higher than
            // a hotdog reaches up onto; and with room to stand to put it down.
            let room = |kind: usize, tip: u8, at: [f64; 2]| -> Option<[f64; 4]> {
                let (own, _, _, _) = posed(self.props.shape(kind), Pose { turn: 0, tip });
                let rect = footprint(own, at, yaw);
                let ok = rect[0] >= x0 + hang && rect[1] >= z0 + hang && rect[2] <= x1 - hang && rect[3] <= z1 - hang
                    && !fort.bases.iter().any(|b| overlaps(if roof { grown(rect, ROOF_CLEAR) } else { rect }, *b, 0.))
                    && [[rect[0], rect[1]], [rect[2], rect[1]], [rect[2], rect[3]], [rect[0], rect[3]]].iter().all(|c| level.side(c[0], c[1]) == side)
                    && !overlaps(rect, grown(field, 0.03), 0.)
                    && on.is_none_or(|o| self.top_of(&places[o]) <= REACH_UP - LEDGE)
                    && (on.is_some() || roof || {
                        let flat = (0..=4).all(|i| (0..=4).all(|j| level.height(rect[0] + (rect[2] - rect[0]) * i as f64 / 4., rect[1] + (rect[3] - rect[1]) * j as f64 / 4.) == 0.));
                        flat && !level.map.blocks.iter().any(|b| overlaps(rect, grown(b.rect, 0.01), 0.))
                            && level.map.pickups.iter().all(|item| apart(rect, item.p) >= 0.1)
                            && level.map.spawns.iter().all(|spawn| apart(rect, spawn.p) >= 0.12)
                            && apart(rect, fort.bacon) >= 0.2
                            && !places.iter().any(|p| p.on.is_none() && p.class != Class::Roof && overlaps(rect, p.rect, 0.004))
                    })
                    && (!roof || Self::rests(&places, &tops, &covers, rect, at).is_some_and(|m| m >= HELD));
                ok.then_some(rect)
            };
            let fits = |kind: usize, tip: u8, at: [f64; 2]| room(kind, tip, at).filter(|_| self.standable(kind, tip, at, yaw, &before));
            // The first kind the team has decides whether the place fits (and its outline); the rest, the kinds that may
            // stand in for it, are sifted for the plan chosen (`sift`). A roof may be moved a little (up to SHIFT across
            // the base's front and out from it) to where it rests on what it roofs: where it has the most room to land a
            // little off (inside what holds it up, and clear of the bases, up to LEEWAY), the nearest of those.
            let kinds: Vec<(usize, u8)> = kinds.into_iter().filter(|(k, _)| stock.contains(k)).collect();
            let found = kinds.first().and_then(|(k, tip)| {
                if !roof {
                    return fits(*k, *tip, at).map(|rect| (at, rect));
                }
                let steps = (SHIFT / 0.02).round() as i32;
                let mut shifts: Vec<(i32, i32)> = (-steps..=steps).flat_map(|i| (-steps..=steps).map(move |j| (i, j))).collect();
                shifts.sort_by_key(|(i, j)| (i * i + j * j, *i, *j));
                let v = frame.v(s);
                let mut spots: Vec<(i64, usize, [f64; 2], [f64; 4])> = shifts
                    .into_iter()
                    .enumerate()
                    .filter_map(|(n, (i, j))| {
                        let at = frame.world(s.u + i as f64 * 0.02, v + j as f64 * 0.02);
                        let rect = room(*k, *tip, at)?;
                        let clear = fort.bases.iter().map(|b| Self::gap(rect, *b)).fold(f64::INFINITY, f64::min) - ROOF_CLEAR;
                        let margin = Self::rests(&places, &tops, &covers, rect, at)?.min(clear).min(LEEWAY);
                        Some(((-margin * 200.).round() as i64, n, at, rect))
                    })
                    .collect();
                spots.sort_by_key(|(m, n, _, _)| (*m, *n));
                spots.into_iter().find(|(_, _, at, _)| self.standable(*k, *tip, *at, yaw, &before)).map(|(_, _, at, rect)| (at, rect))
            });
            let Some((at, rect)) = found else {
                number.push(None);
                continue;
            };
            // (What a roof lies on is to stand as tall as the plan has it, whatever stands in for its first kind, so that it
            // lies level.)
            if roof {
                for c in &covers {
                    let tall = self.top_of(&places[*c]);
                    let level: Vec<(usize, u8)> = places[*c].kinds.iter().copied().filter(|(k, tip)| (self.tall(*k, *tip) - tall).abs() <= 0.005).collect();
                    places[*c].kinds = level;
                }
            }
            // (What it stands on is to be no higher than a hotdog reaches up onto, whatever stands in for its first kind.)
            if let Some(o) = on {
                let low: Vec<(usize, u8)> = places[o].kinds.iter().copied().filter(|(k, tip)| self.tall(*k, *tip) <= REACH_UP - LEDGE).collect();
                places[o].kinds = low;
            }
            let under: Vec<usize> = if roof { (0..places.len()).filter(|o| places[*o].class != Class::Roof && !covers.contains(o) && overlaps(places[*o].rect, rect, 0.)).collect() } else { vec![] };
            fitted += Self::credit(s.layer, on.is_none() && !roof && lying.iter().any(|e| overlaps(grown(rect, 0.01), *e, 0.)));
            number.push(Some(places.len()));
            places.push(Place { class: s.class, kinds, at, yaw, layer: s.layer, on, under, covers, rect, prop: None, failed: false, tries: 0, wait: 0. });
        }
        let outside = frame.world(d.gate[0] + 0.3, d.gate[1]);
        let base = fort.bases[team];
        let hull = places.iter().filter(|p| matches!(p.class, Class::Panel | Class::Block) && p.on.is_none()).fold(base, |h, p| [h[0].min(p.rect[0]), h[1].min(p.rect[1]), h[2].max(p.rect[2]), h[3].max(p.rect[3])]);
        // The way in: from the door out through the gate and on a little towards the field, to be kept clear.
        let approach = {
            let ([a, b], [c, e]) = (frame.world(d.door[0], d.door[1]), frame.world(d.gate[0] + 0.3, d.gate[1]));
            [a.min(c) - 0.1, b.min(e) - 0.1, a.max(c) + 0.1, b.max(e) + 0.1]
        };
        let plan = Plan {
            team,
            design,
            flipped,
            approach,
            outside,
            places,
            door: frame.world(d.door[0], d.door[1]),
            gate: frame.world(d.gate[0], d.gate[1]),
            posts: d.posts.iter().map(|p| frame.world(p[0], p[1])).collect(),
            hull,
            yard: vec![],
            cloths: vec![],
            loose: vec![],
            clear: vec![],
            looked: 0.,
        };
        Some((fitted / weight, plan))
    }
}

/// The open place of a navigation grid nearest [x, z] within `within` metres (the point itself, when it is open).
fn open_cell(nav: &Level, at: [f64; 2], within: f64) -> Option<[f64; 2]> {
    if nav.clear(at[0], at[1]) {
        return Some(at);
    }
    let rings = (within / nav.cell).ceil() as i64;
    let mut best: Option<(f64, [f64; 2])> = None;
    for di in -rings..=rings {
        for dj in -rings..=rings {
            let c = [at[0] + di as f64 * nav.cell, at[1] + dj as f64 * nav.cell];
            let c = nav.cell_center(nav.cell_index(c[0], c[1]));
            let far = (c[0] - at[0]).hypot(c[1] - at[1]);
            if far <= within && nav.clear(c[0], c[1]) && best.is_none_or(|(f, _)| far < f) {
                best = Some((far, c));
            }
        }
    }
    best.map(|(_, c)| c)
}
/// Whether a walker on the bots' level `nav` at the open place `start` can get within reach (REACH and a little) of a
/// piece whose extent is `e`: an open place that near it joined to `start`.
fn within_reach(nav: &Level, start: [f64; 2], e: [f64; 6]) -> bool {
    let near = REACH + 0.05;
    let steps = |lo: f64, hi: f64| ((hi - lo + 2. * near) / nav.cell).ceil() as usize;
    for i in 0..=steps(e[0], e[2]) {
        for j in 0..=steps(e[1], e[3]) {
            let c = nav.cell_center(nav.cell_index(e[0] - near + i as f64 * nav.cell, e[1] - near + j as f64 * nav.cell));
            if apart([e[0], e[1], e[2], e[3]], c) <= near && nav.clear(c[0], c[1]) && nav.joined(start, c) {
                return true;
            }
        }
    }
    false
}
/// A piece standing (or lying, or draped) where it was left: not in anybody's hands, not flying about.
fn standing(state: State) -> bool {
    matches!(state, State::Asleep | State::Locked | State::Draped)
}
/// The middle of a box [x0, z0, x1, z1, ...].
fn middle(e: &[f64]) -> [f64; 2] {
    [(e[0] + e[2]) / 2., (e[1] + e[3]) / 2.]
}

impl Feast {
    /// The build phase (forts.rs `fort_bot`): a bot builds its team's fort to the plan (see the module's documentation).
    /// A gun or a box of ammunition on its own side that it wants it fetches first, as in the fight (though it cannot
    /// fire before the field drops); at the start it wants none, both its guns being loaded.
    pub(crate) fn mason(&mut self, id: u32, dt: f32) {
        let Some((team, carrying)) = self.player(id).map(|p| (p.team as usize, p.carry >= 0)) else {
            return;
        };
        self.look_over(team);
        if self.plan_of(team).is_none() {
            return;
        }
        if carrying {
            return self.lay(id, dt);
        }
        if self.errand(id, 1.6, dt) {
            return;
        }
        self.fetch(id, dt);
    }
    /// A team's plan is drawn when its bots first look for it, and twice a second its built places are looked over: a
    /// piece taken away, knocked off its place or (a roof) not draped there is to be put back, and is free to be fetched
    /// again; a place that has failed TRIES times is given up.
    fn look_over(&mut self, team: usize) {
        let Some(fort) = &self.fort else {
            return;
        };
        if !fort.drawn[team] {
            let plan = self.plan(team);
            let fort = self.fort.as_mut().unwrap();
            (fort.plans[team], fort.drawn[team]) = (plan, true);
            return;
        }
        let now = self.elapsed;
        let Some(plan) = fort.plans[team].as_ref().filter(|plan| now - plan.looked >= 0.5) else {
            return;
        };
        let mut lost = vec![];
        for (k, place) in plan.places.iter().enumerate() {
            let Some(prop) = place.prop else {
                continue;
            };
            let state = self.props.state(prop);
            let gone = if place.class == Class::Roof {
                state != State::Draped && state != State::Awake
            } else {
                let e = self.props.extent(prop);
                let [x, z] = middle(&e);
                !(standing(state) || state == State::Awake) || (x - place.at[0]).hypot(z - place.at[1]) > SHIFTED || place.on.is_some_and(|on| plan.places[on].prop.is_none())
            };
            if gone {
                lost.push((k, prop, state));
            }
        }
        // (A piece to be taken away that has been built into the fort, or is no longer lying loose, is off the list.)
        let gone: Vec<usize> = plan.clear.iter().copied().filter(|i| plan.places.iter().any(|q| q.prop == Some(*i)) || !matches!(self.props.state(*i), State::Asleep | State::Awake | State::Locked)).collect();
        // (A place none of whose kinds the team has left to build with is given up: what stands on it or roofs over it
        // need not wait for it.)
        let left = self.left(team);
        let spent: Vec<usize> = (0..plan.places.len()).filter(|k| plan.places[*k].open() && plan.places[*k].kinds.iter().all(|(kind, _)| left[*kind] == 0)).collect();
        let plan = self.fort.as_mut().unwrap().plans[team].as_mut().unwrap();
        plan.looked = now;
        plan.clear.retain(|i| !gone.contains(i));
        for k in spent {
            plan.places[k].failed = true;
        }
        for (k, prop, state) in lost {
            let place = &mut plan.places[k];
            (place.prop, place.tries) = (None, place.tries + 1);
            place.failed = place.tries >= TRIES;
            if state != State::Carried && !plan.loose.contains(&prop) {
                plan.loose.push(prop);
            }
        }
    }
    /// The pieces holding others up (those a locked piece stands on).
    fn under(&self) -> Vec<bool> {
        let mut under = vec![false; self.props.len()];
        for i in 0..self.props.len() {
            if self.props.state(i) == State::Locked {
                for (j, _) in &self.props.body(i).supports {
                    if let Some(u) = under.get_mut(*j) {
                        *u = true;
                    }
                }
            }
        }
        under
    }
    /// Whether bot `id` may fetch the piece `i` (`under`: the pieces holding others up): one lying loose on its side of
    /// the field, or still where the map put it, or set down by the bots, that nothing stands on, that is no place's
    /// piece, that it has not given up on and that no team mate is fetching. Never a piece locked into somebody's wall,
    /// nor one in a drape.
    fn fetchable(&self, id: u32, i: usize, under: &[bool]) -> bool {
        let Some(p) = self.player(id) else {
            return false;
        };
        let Some(plan) = self.plan_of(p.team as usize) else {
            return false;
        };
        if i >= self.props.len() || under.get(i).copied().unwrap_or(true) || p.job.shun.contains(&i) || plan.places.iter().any(|q| q.prop == Some(i)) {
            return false;
        }
        let at = self.props.position(i);
        let stock = || self.level.map.loose.get(i).is_some_and(|l| (l.p[0] - at[0]).hypot(l.p[1] - at[2]) < 0.002 && (FLOOR + l.base - at[1]).abs() < 0.002);
        let loose = match self.props.state(i) {
            State::Awake | State::Asleep => true,
            State::Locked => plan.loose.contains(&i) || stock(),
            _ => false,
        };
        let side = self.level.fort.as_ref().map_or(0, |fort| fort.sides[p.team as usize]);
        loose && self.level.side(at[0], at[2]) == side && !self.players.iter().any(|q| q.id != id && q.alive && q.team == p.team && q.job.prop == i as i32)
    }
    /// How many pieces of each kind a team has left to build with: on its side of the field, lying anywhere or in
    /// somebody's hands, and no place's piece yet.
    fn left(&self, team: usize) -> Vec<usize> {
        let mut left = vec![0; LOOSE.len()];
        let (Some(plan), Some(fort)) = (self.plan_of(team), self.level.fort.as_ref()) else {
            return left;
        };
        for i in 0..self.props.len() {
            let at = self.props.position(i);
            let state = self.props.state(i);
            if (standing(state) && state != State::Draped || matches!(state, State::Awake | State::Carried)) && self.level.side(at[0], at[2]) == fort.sides[team] && !plan.places.iter().any(|q| q.prop == Some(i)) {
                left[self.props.body(i).shape] += 1;
            }
        }
        left
    }
    /// The places a team mate of `id`'s is working on.
    fn claimed(&self, id: u32) -> Vec<usize> {
        let Some(team) = self.player(id).map(|p| p.team) else {
            return vec![];
        };
        self.players.iter().filter(|q| q.id != id && q.alive && q.bot && q.team == team && q.job.slot >= 0).map(|q| q.job.slot as usize).collect()
    }
    /// What lies in the way of a place on the counter: the pieces within its outline that are not the fort's own, the
    /// topmost first (none for a roof, or a place on top of another). `ext` holds the pieces' extents (None for one not
    /// lying anywhere).
    fn in_the_way(&self, plan: &Plan, k: usize, ext: &[Option<[f64; 6]>], carried: Option<usize>) -> Vec<usize> {
        let place = &plan.places[k];
        if place.on.is_some() || place.class == Class::Roof {
            return vec![];
        }
        let rect = grown(place.rect, -0.006);
        // (Not a piece of the fort: one a little off its own place, it is put down flush against.)
        let mut found: Vec<usize> = (0..ext.len()).filter(|i| Some(*i) != carried && ext[*i].is_some_and(|e| overlaps(rect, [e[0], e[1], e[2], e[3]], 0.)) && !plan.places.iter().any(|q| q.prop == Some(*i))).collect();
        found.sort_by(|a, b| ext[*b].unwrap()[5].total_cmp(&ext[*a].unwrap()[5]).then(a.cmp(b)));
        found
    }
    /// The extents of the pieces lying anywhere (None for those carried, lost or gone).
    fn extents(&self) -> Vec<Option<[f64; 6]>> {
        (0..self.props.len()).map(|i| (standing(self.props.state(i)) || self.props.state(i) == State::Awake).then(|| self.props.extent(i))).collect()
    }
    /// A bot with empty hands: it looks (at most four times a second) for the place it can build soonest (its layer
    /// first, then the way to a piece for it and on to the place) and a piece for it, the nearest of the first kind that
    /// serves; or for a piece lying in the way of a place, to take out of it. Then it walks to the piece and picks it up,
    /// and holds it as the place wants. With nothing to do it waits at its guard post inside the fort.
    fn fetch(&mut self, id: u32, dt: f32) {
        let Some(p) = self.player(id).cloned() else {
            return;
        };
        let (team, now) = (p.team as usize, self.elapsed);
        let mut j = p.job.clone();
        let under = self.under();
        // (What it has given up on it tries again after a while: things move.)
        if !j.shun.is_empty() && now - j.shunned > FETCH {
            j.shun.clear();
        }
        if j.shun.is_empty() {
            j.shunned = now;
        }
        if j.prop >= 0 && now - j.since > FETCH {
            j.shun.push(j.prop as usize);
            (j.prop, j.slot) = (-1, -1);
        }
        if j.prop >= 0 && (!self.fetchable(id, j.prop as usize, &under) || (j.slot >= 0 && !self.plan_of(team).unwrap().ready(j.slot as usize))) {
            (j.prop, j.slot) = (-1, -1);
        }
        let beat = |t: f32| (t * 4. + id as f32 / 7.).floor();
        if j.prop < 0 && beat(now) != beat(now - dt) {
            self.player_mut(id).unwrap().job = j.clone();
            if let Some((slot, prop)) = self.choose(id, &under) {
                (j.slot, j.prop, j.since) = (slot, prop as i32, now);
            }
        }
        self.player_mut(id).unwrap().job = j.clone();
        if j.prop < 0 {
            // Nothing to do: it waits inside the fort, at its guard post.
            let plan = self.plan_of(team).unwrap();
            let post = plan.posts.get((id as usize / 2) % plan.posts.len().max(1)).copied().unwrap_or(plan.door);
            if (post[0] - p.p[0] as f64).hypot(post[1] - p.p[2] as f64) > 0.05 {
                self.march(id, [post[0] as f32, post[1] as f32], dt);
            }
            return;
        }
        let prop = j.prop as usize;
        let e = self.props.extent(prop);
        let c = middle(&e);
        // It reaches for the nearest part of it, from beside it (a board or a napkin is too big to reach the middle of from
        // its edge): of something lying flat (a napkin), its top.
        let flat = e[5] - self.level.ground(c[0], c[1]) < STEP;
        if apart([e[0], e[1], e[2], e[3]], [p.p[0] as f64, p.p[2] as f64]) < REACH {
            let inset = |lo: f64, hi: f64, at: f64| at.clamp(lo + 0.04_f64.min((hi - lo) / 2.), hi - 0.04_f64.min((hi - lo) / 2.));
            let y = if flat { e[5] - 0.002 } else { (e[4] + e[5]) / 2. };
            // Its nearest part, or failing that (something else in the way) its middle or a corner, whichever it would
            // take hold of from here.
            let toward = |x: f64, z: f64| Vec3::new(inset(e[0], e[2], x) as f32, y as f32, inset(e[1], e[3], z) as f32);
            let aims = [toward(p.p[0] as f64, p.p[2] as f64), toward(c[0], c[1]), toward(e[0], e[1]), toward(e[2], e[1]), toward(e[0], e[3]), toward(e[2], e[3])];
            let first = self.aim(id, aims[0], Vec3::ZERO, 0.);
            let reached = aims.iter().map(|a| self.aim(id, *a, Vec3::ZERO, 0.)).find(|d| self.grasp(id, *d).0 == Some(prop));
            // (Something lying flat, which it can walk onto, and which it would not take hold of from here: it goes on
            // towards its middle.)
            if reached.is_none() && flat && (c[0] - p.p[0] as f64).hypot(c[1] - p.p[2] as f64) > 0.05 {
                return self.march(id, [c[0] as f32, c[1] as f32], dt);
            }
            let d = reached.unwrap_or(first);
            self.face(id, d);
            // (Not when what it would take hold of from here is a piece of its own fort, in front of what it is after:
            // it would pull its fort down.)
            let (would, _) = self.grasp(id, d);
            let ours = would.is_some_and(|w| w != prop && self.plan_of(team).is_some_and(|plan| plan.places.iter().any(|q| q.prop == Some(w))));
            let got = !ours && self.grab(id, d);
            let held = self.player(id).map_or(-1, |p| p.carry);
            let j = &mut self.player_mut(id).unwrap().job;
            if !got {
                j.shun.push(prop);
                (j.prop, j.slot) = (-1, -1);
                return;
            }
            // (It may have picked up something else in the way: that is what it carries now, for whatever place it fits,
            // or to set down well away from what it was after.)
            if held != prop as i32 {
                (j.slot, j.missed) = (-1, prop as i32);
            }
            (j.prop, j.since, j.stood, j.tries, j.aside, j.near) = (held, now, false, 0, false, -1.);
            j.avoid.clear();
            if held >= 0 {
                self.fort.as_mut().unwrap().plans[team].as_mut().unwrap().clear.retain(|i| *i != held as usize);
            }
            return;
        }
        self.march(id, [c[0] as f32, c[1] as f32], dt);
    }
    /// The place bot `id` should build next and the piece to fetch for it, or a piece in the way of a place to fetch
    /// (-1 for its place); None when there is nothing to do.
    fn choose(&self, id: u32, under: &[bool]) -> Option<(i32, usize)> {
        let p = self.player(id)?;
        let plan = self.plan_of(p.team as usize)?;
        let (claimed, ext) = (self.claimed(id), self.extents());
        let me = [p.p[0] as f64, p.p[2] as f64];
        let far = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
        // (Only what it can get to: not a piece walled off from it, behind a wall of the fort, say.)
        let nav = self.fort.as_ref().and_then(|fort| fort.nav.as_ref());
        let start = nav.and_then(|nav| open_cell(nav, me, 0.16));
        let free: Vec<usize> = (0..self.props.len())
            .filter(|i| self.fetchable(id, *i, under))
            .filter(|i| match (nav, start) {
                (Some(nav), Some(start)) => within_reach(nav, start, self.props.extent(*i)),
                _ => true,
            })
            .collect();
        let left = self.left(p.team as usize);
        // What stands on each piece.
        let mut above: Vec<Vec<usize>> = vec![vec![]; self.props.len()];
        for i in 0..self.props.len() {
            if self.props.state(i) == State::Locked {
                for (j, _) in &self.props.body(i).supports {
                    if let Some(a) = above.get_mut(*j) {
                        a.push(i);
                    }
                }
            }
        }
        let mut best: Option<(f64, i32, usize)> = None;
        let mut offer = |cost: f64, slot: i32, prop: usize| {
            if best.is_none_or(|(c, _, _)| cost < c) {
                best = Some((cost, slot, prop));
            }
        };
        for k in 0..plan.places.len() {
            if !plan.due(k, self.elapsed) || claimed.contains(&k) {
                continue;
            }
            let place = &plan.places[k];
            let way = self.in_the_way(plan, k, &ext, None);
            if !way.is_empty() {
                // (Cleared from the top down: what lies on another first.)
                if let Some(o) = way.into_iter().find(|o| free.contains(o)) {
                    offer(place.layer as f64 * 1.5 + far(me, middle(&ext[o].unwrap())) - 0.3, -1, o);
                }
                continue;
            }
            // The first kind that serves of which any piece is left (one under others, or being fetched for another
            // place, is not to be had yet: nor is another kind instead). Any small topper will do for a topper.
            let kinds: Vec<usize> = if place.class == Class::Topper {
                place.kinds.iter().map(|(kind, _)| *kind).collect()
            } else {
                place.kinds.iter().map(|(kind, _)| *kind).find(|kind| left[*kind] > 0).into_iter().collect()
            };
            let nearest = free.iter().filter_map(|i| Some((kinds.iter().position(|kind| *kind == self.props.body(*i).shape)?, *i))).map(|(rank, i)| (far(me, middle(&ext[i].unwrap_or([0.; 6]))) + rank as f64 * 0.15, i)).min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((d, i)) = nearest {
                offer(place.layer as f64 * 1.5 + d + 0.5 * far(middle(&ext[i].unwrap_or([0.; 6])), place.at), k as i32, i);
            } else if let Some(top) = self.unpile(&kinds, &free, &above) {
                // Every piece of the kind it wants is under something (a dish on the plates, tins on the bread bin):
                // what is on top comes off first.
                offer(place.layer as f64 * 1.5 + far(me, middle(&ext[top].unwrap_or([0.; 6]))) + 0.4, -1, top);
            }
        }
        // A piece in the way of building a place (where its builder must stand, or that it would shut in) is taken
        // away, from the top of its pile down.
        for c in &plan.clear {
            let top = if free.contains(c) { Some(*c) } else { Self::pile_top(*c, &free, &above) };
            if let Some(o) = top {
                offer(far(me, middle(&ext[o].unwrap_or([0.; 6]))) - 0.3, -1, o);
            }
        }
        best.map(|(_, slot, prop)| (slot, prop))
    }
    /// The free piece on top of the pile over piece `i` (`above`: what stands on each piece).
    fn pile_top(i: usize, free: &[usize], above: &[Vec<usize>]) -> Option<usize> {
        let mut todo = above[i].clone();
        while let Some(j) = todo.pop() {
            if free.contains(&j) {
                return Some(j);
            }
            todo.extend(above[j].iter().copied());
        }
        None
    }
    /// The free piece on top of a pile with a piece of one of `kinds` under it (`above`: what stands on each piece).
    fn unpile(&self, kinds: &[usize], free: &[usize], above: &[Vec<usize>]) -> Option<usize> {
        for i in 0..self.props.len() {
            if kinds.contains(&self.props.body(i).shape) {
                if let Some(top) = Self::pile_top(i, free, above) {
                    return Some(top);
                }
            }
        }
        None
    }
    /// The place a carried piece of `kind` goes best for bot `id`: one ready and unclaimed that it serves (none of the
    /// kinds the place would rather have being left), with nothing in its way (its layer first, then how far down the
    /// place's list of kinds this one is, then the way there).
    fn place_for(&self, id: u32, kind: usize, carried: usize) -> Option<usize> {
        let p = self.player(id)?;
        let plan = self.plan_of(p.team as usize)?;
        let (claimed, ext, left) = (self.claimed(id), self.extents(), self.left(p.team as usize));
        let me = [p.p[0] as f64, p.p[2] as f64];
        (0..plan.places.len())
            .filter(|k| plan.due(*k, self.elapsed) && !claimed.contains(k) && self.in_the_way(plan, *k, &ext, Some(carried)).is_empty())
            .filter_map(|k| {
                let place = &plan.places[k];
                // (Only for a place whose better kinds have all been used up.)
                let rank = place.kinds.iter().position(|(o, _)| *o == kind)?;
                if place.class != Class::Topper && place.kinds[..rank].iter().any(|(o, _)| left[*o] > 0) {
                    return None;
                }
                Some((place.layer as f64 * 1.5 + rank as f64 * 0.4 + (place.at[0] - me[0]).hypot(place.at[1] - me[1]), k))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, k)| k)
    }
    /// Where a carrier must stand, facing `facing`, for the middle of what it holds (`prop`, held as `pose`) to be over
    /// `at` ([x, z]).
    fn stand(&self, prop: usize, pose: Pose, at: [f64; 2], facing: f64) -> [f64; 2] {
        let kind = self.props.body(prop).shape;
        let pose = self.pose_of(kind, pose);
        let (own, _, _, _) = posed(self.props.shape(kind), pose);
        let mut feet = at;
        for _ in 0..2 {
            let y = self.level.ground(feet[0], feet[1]);
            let (o, _) = self.holding(prop, pose, [feet[0], y, feet[1]], facing);
            let c = out((own[0] + own[2]) / 2., (own[1] + own[3]) / 2., facing + pose.angle());
            feet = [feet[0] + at[0] - o[0] - c[0], feet[1] + at[1] - o[2] - c[1]];
        }
        feet
    }
    /// How bot `id` puts the piece it carries on place `k`: where it stands, which way it faces (a quarter turn, so that
    /// the piece lands square) and how it holds the piece. Of the four ways round, the one with room to stand (on the
    /// counter, on its side of the field, clear of the pieces lying about) nearest the bot, outside the fort if it can;
    /// not those (`banned`, a bit each from facing -x round to facing +z) it has found it cannot get to.
    fn approach(&self, id: u32, k: usize, banned: u8) -> Result<([f64; 2], f64, Pose), Vec<usize>> {
        let Some((p, plan)) = self.player(id).and_then(|p| Some((p, self.plan_of(p.team as usize)?))) else {
            return Err(vec![]);
        };
        let (prop, place) = (p.carry as usize, &plan.places[k]);
        let kind = self.props.body(prop).shape;
        let Some(tip) = place.tip(kind) else {
            return Err(vec![]);
        };
        let side = self.level.fort.as_ref().map_or(0, |fort| fort.sides[p.team as usize]);
        let ext = self.extents();
        let ground = self.level.ground(place.at[0], place.at[1]);
        let mut best: Option<(f64, [f64; 2], f64, Pose)> = None;
        // (Failing those: the fewest loose pieces lying where it could stand otherwise, to be taken away.)
        let mut blocked: Option<Vec<usize>> = None;
        for q in -1..=2 {
            if banned & (1 << (q + 1)) != 0 {
                continue;
            }
            let facing = q as f64 * FRAC_PI_2;
            let turn = (((place.yaw - facing) / FRAC_PI_4).round() as i32).rem_euclid(8) as u8;
            let pose = Pose { turn, tip };
            let feet = self.stand(prop, pose, place.at, facing);
            let [x, z] = feet;
            let y = self.level.ground(x, z);
            let room = x > self.level.x0 + 0.04 && x < self.level.x1 - 0.04 && z > self.level.z0 + 0.04 && z < self.level.z1 - 0.04
                && self.level.clear(x, z)
                && self.level.side(x, z) == side
                && (place.on.is_some() || (y - ground).abs() < 0.01);
            if !room {
                continue;
            }
            let near: Vec<usize> = (0..ext.len()).filter(|i| *i != prop && ext[*i].is_some_and(|e| e[5] >= y + STEP && apart([e[0], e[1], e[2], e[3]], feet) <= BODY)).collect();
            if !near.is_empty() {
                if near.iter().all(|i| matches!(self.props.state(*i), State::Asleep | State::Awake | State::Locked) && !plan.places.iter().any(|q| q.prop == Some(*i))) && blocked.as_ref().is_none_or(|b| near.len() < b.len()) {
                    blocked = Some(near);
                }
                continue;
            }
            let cost = (x - p.p[0] as f64).hypot(z - p.p[2] as f64) + if plan.inside(feet) { 0.6 } else { 0. };
            if best.is_none_or(|(c, _, _, _)| cost < c) {
                best = Some((cost, feet, facing, pose));
            }
        }
        best.map(|(_, feet, facing, pose)| (feet, facing, pose)).ok_or(blocked.unwrap_or_default())
    }
    /// Whether a piece landing as `spot` says is on place `k` as it should be: its middle within TOLERANCE of the
    /// place's, turned the place's way (a round one any way), on what the place stands on (on the counter, nothing),
    /// locked there, and (a roof) draping over the place's pieces without being refused. Otherwise how far it is off
    /// ([x, z], for the bot to step over by), or None when stepping over would not help.
    fn judge(&self, plan: &Plan, k: usize, spot: &Placement, pose: Pose) -> Result<(), Option<[f64; 2]>> {
        let place = &plan.places[k];
        let pose = self.pose_of(spot.kind, pose);
        let (own, _, round, bottom) = posed(self.props.shape(spot.kind), pose);
        let turned = spot.yaw + pose.angle();
        let c = out((own[0] + own[2]) / 2., (own[1] + own[3]) / 2., turned);
        let off = [place.at[0] - spot.p[0] - c[0], place.at[1] - spot.p[2] - c[1]];
        // (Refused where it would land, a little off its place, towards a base: it steps over onto its place.)
        if spot.refused.is_some() {
            return Err((off[0].hypot(off[1]) > 0.005).then_some(off));
        }
        // (A roof only has to drape over what it roofs: it may lie a little off.)
        if off[0].hypot(off[1]) > if place.class == Class::Roof { ROOFED } else { TOLERANCE } {
            return Err(Some(off));
        }
        if !round && spot.kind != NAPKIN && ((turned - place.yaw) / PI - ((turned - place.yaw) / PI).round()).abs() * PI > 0.1 {
            return Err(None);
        }
        match place.on {
            Some(on) if spot.onto != plan.places[on].prop => return Err(Some(off)),
            None if place.class != Class::Roof && spot.p[1] + bottom > self.level.ground(place.at[0], place.at[1]) + HAIR + 0.006 => return Err(None),
            _ => {}
        }
        if place.class == Class::Roof {
            let Some(canopy) = &spot.drape else {
                return Err(Some(off));
            };
            // (Its cloth clear of the bases too, not only its wall: it hangs down over what it roofs. Well clear, ROOF_CLEAR,
            // when it is let go to settle; locked where it lands, it stays there.)
            let cloth = footprint(own, [spot.p[0] + c[0], spot.p[2] + c[1]], turned);
            let clear = if spot.lock { 0.005 } else { ROOF_CLEAR };
            if self.level.fort.as_ref().is_some_and(|fort| fort.bases.iter().any(|b| overlaps(grown(cloth, clear), *b, 0.))) {
                return Err((off[0].hypot(off[1]) > 0.005).then_some(off));
            }
            if !place.covers.iter().filter_map(|c| plan.places[*c].prop).any(|prop| canopy.props.contains(&prop)) || self.refusal(canopy, &self.bodies).is_some() {
                return Err(Some(off));
            }
        } else if !spot.lock {
            return Err(Some(off));
        }
        Ok(())
    }
    /// A bot with a piece in its hands carries it to its place and puts it down there (see the module's documentation),
    /// or, with no place for it, sets it down in the yard.
    fn lay(&mut self, id: u32, dt: f32) {
        let Some(p) = self.player(id).cloned() else {
            return;
        };
        let (team, now, prop) = (p.team as usize, self.elapsed, p.carry as usize);
        let kind = self.props.body(prop).shape;
        let mut j = p.job.clone();
        let plan = self.plan_of(team).unwrap();
        if j.slot >= 0 && !(plan.ready(j.slot as usize) && plan.places[j.slot as usize].tip(kind).is_some() && !self.claimed(id).contains(&(j.slot as usize))) {
            (j.slot, j.stood) = (-1, false);
        }
        if j.slot < 0 && !j.aside {
            match self.place_for(id, kind, prop) {
                Some(k) => (j.slot, j.stood, j.tries, j.since, j.banned) = (k as i32, false, 0, now, 0),
                None => (j.aside, j.stood, j.since) = (true, false, now),
            }
        }
        if now - j.since > CARRY {
            if j.slot >= 0 {
                self.fort.as_mut().unwrap().plans[team].as_mut().unwrap().places[j.slot as usize].wait = now + WAIT;
            }
            (j.slot, j.aside, j.stood, j.since) = (-1, true, false, now);
        }
        if !j.stood {
            let way = if j.aside { self.yard_spot(id, j.tries > 0).or_else(|| self.yard_spot(id, true)).ok_or(vec![]) } else { self.approach(id, j.slot as usize, j.banned) };
            match way {
                Ok((feet, facing, pose)) => {
                    (j.stand, j.facing, j.pose, j.stood, j.best, j.gain) = ([feet[0] as f32, feet[1] as f32], facing as f32, pose, true, f32::INFINITY, now);
                    self.set_held_pose(id, pose);
                }
                Err(blocking) if j.slot >= 0 => {
                    // No room to stand to put it there (or no way to get there): another place, another time; and what lies
                    // where it could stand is taken away meanwhile.
                    let plan = self.fort.as_mut().unwrap().plans[team].as_mut().unwrap();
                    plan.places[j.slot as usize].wait = now + WAIT;
                    for i in blocking {
                        if !plan.clear.contains(&i) {
                            plan.clear.push(i);
                        }
                    }
                    (j.slot, j.banned) = (-1, 0);
                    self.player_mut(id).unwrap().job = j;
                    return;
                }
                Err(_) => {
                    // (Nowhere in the yard at all: it tries again in a moment.)
                    self.player_mut(id).unwrap().job = j;
                    return;
                }
            }
        }
        let far = (j.stand[0] - p.p[0]).hypot(j.stand[1] - p.p[2]);
        if far > 0.05 {
            // Getting no nearer: another way round (none left: another place).
            if far < j.best - 0.02 {
                (j.best, j.gain) = (far, now);
            }
            if now - j.gain > STUCK {
                j.banned |= 1 << (((j.facing / std::f32::consts::FRAC_PI_2).round() as i32 + 1).rem_euclid(4));
                (j.stood, j.near) = (false, -1.);
                if j.aside {
                    // (Setting it down out of the way and getting nowhere: another spot, on top of whatever is there if
                    // need be.)
                    (j.tries, j.banned) = (j.tries + 1, 0);
                } else if j.banned == 15 {
                    self.fort.as_mut().unwrap().plans[team].as_mut().unwrap().places[j.slot as usize].wait = now + WAIT;
                    (j.slot, j.banned) = (-1, 0);
                }
                self.player_mut(id).unwrap().job = j;
                return;
            }
            j.near = -1.;
            self.player_mut(id).unwrap().job = j.clone();
            return self.march(id, j.stand, dt);
        }
        if j.near < 0. {
            j.near = now;
        }
        self.player_mut(id).unwrap().job = j.clone();
        // (Something in its way a centimetre or two short of there: it puts it down from where it is, and steps over
        // if it lands off its place. Further off, it works out its way round again.)
        if far > 0.004 && now - j.near < SETTLE {
            return self.creep(id, j.stand);
        }
        if far > 0.025 && !j.aside {
            let job = &mut self.player_mut(id).unwrap().job;
            (job.stood, job.tries, job.near) = (false, job.tries + 1, -1.);
            if job.tries > NUDGES + 1 {
                // (It cannot get there from this way round: another way round, and none left, another place, another
                // time.)
                job.banned |= 1 << (((j.facing / std::f32::consts::FRAC_PI_2).round() as i32 + 1).rem_euclid(4));
                job.tries = 0;
                if job.banned == 15 {
                    (job.slot, job.banned) = (-1, 0);
                    self.fort.as_mut().unwrap().plans[team].as_mut().unwrap().places[j.slot as usize].wait = now + WAIT;
                }
            }
            return;
        }
        // There: it stops, faces the way it puts from, and once it stands still looks where the piece would land.
        if p.pace[0].hypot(p.pace[1]) >= STILL * 0.6 {
            return;
        }
        if let Some(me) = self.player_mut(id) {
            (me.yaw, me.pitch, me.pose) = (j.facing, 0., j.pose);
        }
        let Some(spot) = self.placement(id) else {
            return;
        };
        if j.aside {
            // (A napkin is never set down where it would drape over what lies there: it would shut that in until the
            // fight.)
            if spot.drape.is_some() {
                let job = &mut self.player_mut(id).unwrap().job;
                job.avoid.push(job.stand);
                (job.stood, job.tries) = (false, job.tries + 1);
                return;
            }
            if self.put(id, Vec3::ZERO) {
                let plan = self.fort.as_mut().unwrap().plans[team].as_mut().unwrap();
                if !plan.loose.contains(&prop) {
                    plan.loose.push(prop);
                }
                self.player_mut(id).unwrap().job = crate::forts::Job { shun: j.shun, at: j.at, checked: j.checked, ..Default::default() };
            } else {
                let job = &mut self.player_mut(id).unwrap().job;
                job.stood = false;
                job.tries += 1;
            }
            return;
        }
        let k = j.slot as usize;
        let verdict = self.judge(self.plan_of(team).unwrap(), k, &spot, j.pose);
        // Not where it would shut a piece still wanted away from the base: that place waits until it has been taken away.
        if verdict.is_ok() {
            let place = &self.plan_of(team).unwrap().places[k];
            let pose = self.pose_of(kind, j.pose);
            let (own, tall, _, bottom) = posed(self.props.shape(kind), pose);
            let turned = spot.yaw + pose.angle();
            let c = out((own[0] + own[2]) / 2., (own[1] + own[3]) / 2., turned);
            let rect = footprint(own, [spot.p[0] + c[0], spot.p[2] + c[1]], turned);
            let (seals, shut) = if place.on.is_none() && place.class != Class::Roof && tall >= STEP { self.shuts_in(team, rect, spot.p[1] + bottom + tall) } else { (false, vec![]) };
            if seals {
                // (It would wall the base in, as things stand: that counts as a try, and the place waits.)
                let place = &mut self.fort.as_mut().unwrap().plans[team].as_mut().unwrap().places[k];
                (place.tries, place.wait) = (place.tries + 1, now + WAIT);
                place.failed = place.tries >= TRIES;
                let job = &mut self.player_mut(id).unwrap().job;
                (job.slot, job.stood, job.banned) = (-1, false, 0);
                return;
            }
            if !shut.is_empty() {
                // (They are taken away meanwhile.)
                let plan = self.fort.as_mut().unwrap().plans[team].as_mut().unwrap();
                plan.places[k].wait = now + WAIT;
                for i in shut {
                    if !plan.clear.contains(&i) {
                        plan.clear.push(i);
                    }
                }
                let job = &mut self.player_mut(id).unwrap().job;
                (job.slot, job.stood, job.banned) = (-1, false, 0);
                return;
            }
        }
        let job = &mut self.player_mut(id).unwrap().job;
        match verdict {
            Ok(()) => {
                if self.put(id, Vec3::ZERO) {
                    let plan = self.fort.as_mut().unwrap().plans[team].as_mut().unwrap();
                    plan.places[k].prop = Some(prop);
                    plan.loose.retain(|i| *i != prop);
                    self.player_mut(id).unwrap().job = crate::forts::Job { shun: j.shun, at: j.at, checked: j.checked, ..Default::default() };
                }
            }
            Err(Some(off)) if j.tries < NUDGES && off[0].hypot(off[1]) < 0.15 => {
                job.stand = [j.stand[0] + off[0] as f32, j.stand[1] + off[1] as f32];
                (job.tries, job.near) = (job.tries + 1, -1.);
            }
            Err(_) => {
                // Landing on a loose piece lying there: that is taken away first (the place waits a moment). Otherwise it
                // tries the other ways round it can put it from, and only when none is left has the place been tried.
                let plan = self.plan_of(team).unwrap();
                let support = plan.places[k].on.and_then(|o| plan.places[o].prop);
                let stray = spot.onto.filter(|o| Some(*o) != support && !plan.places.iter().any(|q| q.prop == Some(*o)) && matches!(self.props.state(*o), State::Asleep | State::Awake | State::Locked));
                let banned = j.banned | 1 << (((j.facing / std::f32::consts::FRAC_PI_2).round() as i32 + 1).rem_euclid(4));
                let other = stray.is_none() && banned != 15 && self.approach(id, k, banned).is_ok();
                let job = &mut self.player_mut(id).unwrap().job;
                if other {
                    (job.banned, job.stood, job.tries, job.near) = (banned, false, 0, -1.);
                    return;
                }
                (job.slot, job.stood, job.banned) = (-1, false, 0);
                let plan = self.fort.as_mut().unwrap().plans[team].as_mut().unwrap();
                match stray {
                    Some(o) => {
                        plan.places[k].wait = now + 2.;
                        if !plan.clear.contains(&o) {
                            plan.clear.push(o);
                        }
                    }
                    None => {
                        // (Tried, and not again at once from where it stands: it would land the same.)
                        let place = &mut plan.places[k];
                        (place.tries, place.wait) = (place.tries + 1, now + WAIT);
                        place.failed = place.tries >= TRIES;
                    }
                }
            }
        }
    }
    /// Where bot `id` sets down a piece it has no place for: the spot of its team's yard nearest it with room round it for
    /// the piece, one way round or another (or, `crowded`, on top of whatever lies there), clear of the bases, the fort's
    /// places and its way in, and with somewhere on the counter to stand to put it down; None when there is none.
    fn yard_spot(&self, id: u32, crowded: bool) -> Option<([f64; 2], f64, Pose)> {
        let p = self.player(id)?;
        let plan = self.plan_of(p.team as usize)?;
        let ext = self.extents();
        let prop = p.carry as usize;
        let napkin = self.props.body(prop).shape == NAPKIN;
        // (A napkin lies over whatever is under it, and drapes over anything there and over the edges of blocks and
        // decks: it goes only where there is nothing at all.)
        let crowded = crowded && !napkin;
        let (own, _, _, _) = posed(self.props.shape(self.props.body(prop).shape), Pose::default());
        let bases = self.level.fort.as_ref().map_or([[0.; 4]; 2], |fort| fort.bases);
        let me = [p.p[0] as f64, p.p[2] as f64];
        // (Not on a napkin, nor by what it was after when it picked this up instead; though a napkin may lie on napkins
        // lying flat on the counter, as they lie at the start, and does not drape there.)
        let flat = |i: usize| self.props.body(i).shape == NAPKIN && ext[i].is_some_and(|e| e[5] - self.level.ground((e[0] + e[2]) / 2., (e[1] + e[3]) / 2.) < 0.02);
        let keep: Vec<[f64; 4]> = (0..ext.len())
            .filter(|i| *i != prop && (self.props.body(*i).shape == NAPKIN || *i as i32 == p.job.missed) && !(napkin && flat(*i)))
            .filter_map(|i| ext[i].map(|e| grown([e[0], e[1], e[2], e[3]], 0.08)))
            .collect();
        let mut spots = if napkin { plan.cloths.clone() } else { plan.yard.clone() };
        spots.sort_by(|a, b| (a[0] - me[0]).hypot(a[1] - me[1]).total_cmp(&(b[0] - me[0]).hypot(b[1] - me[1])));
        let first = ((p.yaw as f64) / FRAC_PI_2).round() as i32;
        for spot in spots {
            for q in 0..4 {
                let facing = (first + q) as f64 * FRAC_PI_2;
                let room = grown(footprint(own, spot, facing), 0.015);
                if bases.iter().chain(plan.places.iter().map(|q| &q.rect)).chain((!napkin).then_some(&plan.approach)).chain(keep.iter()).any(|r| overlaps(room, *r, 0.)) {
                    continue;
                }
                if !crowded && !(0..ext.len()).all(|i| i == prop || (napkin && flat(i)) || ext[i].is_none_or(|e| !overlaps(room, [e[0], e[1], e[2], e[3]], 0.))) {
                    continue;
                }
                if napkin && self.level.map.blocks.iter().map(|b| b.rect).chain(self.level.map.decks.iter().map(|d| d.rect)).chain(self.level.map.ramps.iter().map(|r| r.rect)).any(|r| overlaps(room, grown(r, 0.02), 0.)) {
                    continue;
                }
                let [x, z] = self.stand(prop, Pose::default(), spot, facing);
                if p.job.avoid.iter().any(|a| (a[0] as f64 - x).hypot(a[1] as f64 - z) < 0.03) {
                    continue;
                }
                if x > self.level.x0 + 0.04 && x < self.level.x1 - 0.04 && z > self.level.z0 + 0.04 && z < self.level.z1 - 0.04 && self.level.clear(x, z) && self.level.height(x, z) == 0. {
                    return Some(([x, z], facing, Pose::default()));
                }
            }
        }
        None
    }
    /// A bot steps straight to [x, z], arriving there in this step if it is near enough.
    pub(crate) fn creep(&mut self, id: u32, to: [f32; 2]) {
        let Some(p) = self.player(id) else {
            return;
        };
        let pace = crate::actor::BOT_WALK * self.burden(id) as f64;
        let (dx, dz) = ((to[0] - p.p[0]) as f64, (to[1] - p.p[2]) as f64);
        let d = dx.hypot(dz);
        if d <= 0. {
            return;
        }
        let dt = 1. / 30.;
        let speed = (d / dt).min(pace);
        if let Some(body) = self.bodies.get_mut(id) {
            body.wish = [dx / d * speed, dz / d * speed];
        }
    }
    /// Forts: the level the bots find their way on, made again (at most twice a second) when what stands on the counter
    /// has changed: the level as it stands (the map, the drapes' walls, the forcefield while it is up) with every piece
    /// lying at rest on the counter that stands taller than a step as a block too, so that a bot's way goes round the
    /// forts and through their gates, and the decks as blocks (their sides are walls to a walker on the counter).
    pub(crate) fn renav(&mut self) {
        let Some(fort) = &self.fort else {
            return;
        };
        if fort.nav.is_some() && self.elapsed - fort.nav_at < 0.5 {
            return;
        }
        let (pieces, walls) = self.obstacles();
        let mut key: Vec<i32> = pieces.iter().flat_map(|r| r[..4].iter().map(|v| (v * 200.).round() as i32)).collect();
        key.extend(walls.iter().flat_map(|b| b.rect.iter().map(|v| (v * 200.).round() as i32)));
        let fort = self.fort.as_mut().unwrap();
        fort.nav_at = self.elapsed;
        if fort.nav.is_some() && fort.nav_key == key {
            return;
        }
        let nav = self.nav_level(&pieces, walls);
        let fort = self.fort.as_mut().unwrap();
        (fort.nav, fort.nav_key) = (Some(nav), key);
    }
    /// What stands in a bot's way on the counter (see `renav`): the pieces lying at rest taller than a step, as boxes
    /// [x0, z0, x1, z1, top], and the walls beyond the map's own (the drapes', the forcefield's while it is up).
    fn obstacles(&self) -> (Vec<[f64; 5]>, Vec<crate::map::Block>) {
        let mut pieces: Vec<[f64; 5]> = vec![];
        for i in 0..self.props.len() {
            if !matches!(self.props.state(i), State::Asleep | State::Locked) {
                continue;
            }
            let e = self.props.extent(i);
            let [x, z] = middle(&e);
            if e[5] - self.level.ground(x, z) >= STEP {
                pieces.push([e[0], e[1], e[2], e[3], e[5]]);
            }
        }
        let field = self.fort.as_ref().filter(|fort| fort.up).map(|fort| fort.field.clone());
        (pieces, self.drapes.iter().map(crate::drape::Drape::block).chain(field).collect())
    }
    /// The bots' level (`renav`) with `pieces` and `walls` standing on it.
    fn nav_level(&self, pieces: &[[f64; 5]], walls: Vec<crate::map::Block>) -> Level {
        let mut map = self.level.map.clone();
        map.blocks.extend(walls);
        map.blocks.extend(pieces.iter().map(|r| crate::map::Block { rect: [r[0], r[1], r[2], r[3]], height: r[4] - FLOOR, style: "nav".into(), facing: None }));
        // (A deck's sides are walls to a walker on the counter: its way goes round, or up its ramp.)
        let decks: Vec<crate::map::Block> = map.decks.iter().map(|d| crate::map::Block { rect: d.rect, height: d.height, style: "nav".into(), facing: None }).collect();
        map.blocks.extend(decks);
        let mut nav = Level::new(map);
        (nav.map, nav.fort) = (self.level.map.clone(), self.level.fort.clone());
        nav
    }
    /// Whether a piece put down over `rect` ([x0, z0, x1, z1], `top` high) would seal its team's base, shutting it away
    /// from outside its fort's gate (`Plan::outside`); and the pieces the team still wants for its fort (of a kind an open
    /// place takes, lying free on its side) that it would shut away from the base, where a way on foot joins them now. It
    /// costs a navigation grid, once a put.
    fn shuts_in(&self, team: usize, rect: [f64; 4], top: f64) -> (bool, Vec<usize>) {
        let (Some(plan), Some(fort)) = (self.plan_of(team), self.level.fort.as_ref()) else {
            return (false, vec![]);
        };
        let wanted: Vec<usize> = plan.places.iter().filter(|q| q.open()).flat_map(|q| q.kinds.iter().map(|(k, _)| *k)).collect();
        let base = fort.bases[team];
        let heart = [(base[0] + base[2]) / 2., (base[1] + base[3]) / 2.];
        // (The way out of the gate first, as a piece that must stay reachable.)
        let lying: Vec<(usize, [f64; 2])> = std::iter::once((usize::MAX, plan.outside))
            .chain(
                (0..self.props.len())
                    .filter(|i| matches!(self.props.state(*i), State::Asleep | State::Locked | State::Awake) && wanted.contains(&self.props.body(*i).shape) && !plan.places.iter().any(|q| q.prop == Some(*i)))
                    .map(|i| (i, middle(&self.props.extent(i))))
                    .filter(|(_, c)| self.level.side(c[0], c[1]) == fort.sides[team]),
            )
            .collect();
        let (mut pieces, walls) = self.obstacles();
        let reached = |nav: &Level, at: [f64; 2]| match (open_cell(nav, at, 0.32), open_cell(nav, heart, 0.32)) {
            (Some(a), Some(b)) => nav.joined(a, b),
            _ => false,
        };
        let Some(now) = self.fort.as_ref().and_then(|fort| fort.nav.as_ref()) else {
            return (false, vec![]);
        };
        let before: Vec<bool> = lying.iter().map(|(_, c)| reached(now, *c)).collect();
        pieces.push([rect[0], rect[1], rect[2], rect[3], top]);
        let after = self.nav_level(&pieces, walls);
        let shut: Vec<usize> = lying.iter().zip(before).filter(|((_, c), was)| *was && !reached(&after, *c)).map(|((i, _), _)| *i).collect();
        (shut.first() == Some(&usize::MAX), shut.into_iter().filter(|i| *i != usize::MAX).collect())
    }
    /// A Forts bot heads for `target` (`advance_actor`, but its way found on the level with the pieces on the counter in
    /// it, `renav`): towards the next point of its way there, up a ramp when the target stands higher. Its way starts
    /// and ends at the nearest open places when it or the target stands close against a piece (as a bot putting
    /// something down or picking something up does), and the last of it, from there, it goes straight.
    pub(crate) fn wend(&mut self, id: u32, target: Vec3, dt: f32) {
        let Some(mut nav) = self.fort.as_mut().and_then(|fort| fort.nav.take()) else {
            return self.advance_actor(id, target, dt);
        };
        let (Some(p), Some(old)) = (self.player(id), self.bodies.get(id).map(|body| body.p)) else {
            self.fort.as_mut().unwrap().nav = Some(nav);
            return;
        };
        let pace = crate::actor::BOT_WALK * if p.stim > 0. { 1.5 } else { 1. } * self.burden(id) as f64;
        let target = target.as_dvec3().to_array();
        let goal = nav.climb_goal(old, target).unwrap_or(target);
        let open = open_cell;
        let (from, to) = ([old[0], old[2]], [goal[0], goal[2]]);
        let far = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
        let next = match (open(&nav, from, 0.16), open(&nav, to, 0.32)) {
            // Near the end of its way, the rest straight.
            (_, Some(end)) if end != to && far(end, from) < 0.06 => to,
            // Standing close against something: out into the open first.
            (Some(start), Some(_)) if far(start, from) > 0.01 => start,
            (Some(start), Some(end)) => nav.path(start, end).first().copied().unwrap_or(end),
            _ => to,
        };
        self.fort.as_mut().unwrap().nav = Some(nav);
        let (dx, dz) = (next[0] - from[0], next[1] - from[1]);
        let length = dx.hypot(dz);
        if let Some(body) = self.bodies.get_mut(id) {
            body.wish = if length > 0. { [dx / length * pace, dz / length * pace] } else { [0.; 2] };
        }
        if let Some(p) = self.player_mut(id) {
            if length > 0. {
                p.yaw = (-dx).atan2(-dz) as f32;
            }
            p.pitch = 0.;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feast::Stage;
    use crate::map::Map;

    const DT: f32 = 1. / 30.;
    const MAPS: [&str; 4] = ["pillow-fort", "jungle-fort", "moonlit-fort", "the-whole-kitchen-forts"];
    fn read(path: &str) -> String {
        std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap_or_else(|error| panic!("{path}: {error}"))
    }
    /// A Forts map's round with bots on both teams and the game's seed `seed`, run to the end of its build phase.
    fn built(name: &str, seed: u64) -> Feast {
        let mut s = Feast::with_map(Map::parse(&read(&format!("assets/maps/{name}.json"))).unwrap(), true);
        s.card_seed = seed;
        while s.stage == Stage::Build && s.elapsed < 200. {
            s.tick(DT);
        }
        s
    }

    /// The boxes [x0, z0, x1, z1] of what stands in a hotdog's way on the counter: every piece lying at rest taller than a
    /// step, and every drape's wall.
    fn standing(s: &Feast) -> Vec<[f64; 4]> {
        let mut rects: Vec<[f64; 4]> = (0..s.props.len())
            .filter(|i| matches!(s.props.state(*i), State::Asleep | State::Locked | State::Awake))
            .map(|i| s.props.extent(i))
            .filter(|e| e[5] - s.level.ground((e[0] + e[2]) / 2., (e[1] + e[3]) / 2.) >= STEP)
            .map(|e| [e[0], e[1], e[2], e[3]])
            .collect();
        rects.extend(s.drapes.iter().map(|d| d.rect));
        rects
    }
    /// Whether a cutting board stands on its edge or on end (its own up axis level), at rest.
    fn on_edge(s: &Feast, i: usize) -> bool {
        LOOSE[s.props.body(i).shape] == "board" && matches!(s.props.state(i), State::Locked | State::Asleep) && s.props.up(i)[1].abs() < 0.1
    }

    /// The whole build phase of every bundled Forts map, bots on both teams, for six of the game's seeds (which choose
    /// different designs): each team builds its fort to its plan, nearly all of it (three quarters at the least, most of
    /// it on the whole), outside both bases, with walls of cutting boards stood on their edges, a napkin draped as a
    /// roof (where a plan has one), and a way on foot from the field into each base.
    #[test]
    fn on_every_forts_map_the_bots_build_detailed_forts_round_their_bases_not_heaps_in_them() {
        let (mut total, mut planned) = (0, 0);
        for name in MAPS {
            for seed in 0..6 {
                let s = built(name, seed);
                let fort = s.level.fort.clone().unwrap();
                // Nothing rests in either base.
                for i in 0..s.props.len() {
                    if !matches!(s.props.state(i), State::Asleep | State::Locked | State::Draped) {
                        continue;
                    }
                    let e = s.props.extent(i);
                    assert!(fort.bases.iter().all(|r| !overlaps([e[0], e[1], e[2], e[3]], *r, 0.002)), "{name} seed {seed}: {} {i} rests in a base: {e:?} ({:?}, drapes {:?})", LOOSE[s.props.body(i).shape], s.props.state(i), s.drapes.iter().map(|d| (d.napkin, d.rect)).collect::<Vec<_>>());
                }
                let mut drapes = vec![0; 2];
                for team in 0..2 {
                    let plan = s.plan_of(team).unwrap();
                    let design = DESIGNS[plan.design].name;
                    // Most of the plan is built: the pieces stand on their places.
                    let placed = plan.places.iter().filter(|p| p.prop.is_some_and(|i| {
                        let e = s.props.extent(i);
                        let [x, z] = middle(&e);
                        (x - p.at[0]).hypot(z - p.at[1]) <= SHIFTED && matches!(s.props.state(i), State::Locked | State::Asleep | State::Draped)
                    })).count();
                    println!("{name} seed {seed} team {team}: {design}{}, {placed} of {} places built", if plan.flipped { " (mirrored)" } else { "" }, plan.places.len());
                    assert!(plan.places.len() >= 14, "{name} seed {seed} team {team}: {design} has {} places", plan.places.len());
                    assert!(placed * 4 >= plan.places.len() * 3, "{name} seed {seed} team {team}: {design} {placed} of {} places", plan.places.len());
                    (total, planned) = (total + placed, planned + plan.places.len());
                    // Walls of boards stood on their edges, outside the base and on the team's side.
                    let side = fort.sides[team];
                    let boards = (0..s.props.len()).filter(|i| on_edge(&s, *i) && s.level.side(s.props.position(*i)[0], s.props.position(*i)[2]) == side).count();
                    assert!(boards >= 2, "{name} seed {seed} team {team}: {boards} boards stand on edge");
                    drapes[team] = s.drapes.iter().filter(|d| !d.props.is_empty() && s.level.side((d.rect[0] + d.rect[2]) / 2., (d.rect[1] + d.rect[3]) / 2.) == side).count();
                    // A way on foot from the field into the base, round everything standing on the counter.
                    assert!(way_in(&s.fort.as_ref().unwrap().open, &standing(&s), fort.bases[team], fort.bacon), "{name} seed {seed} team {team}: no way into the base");
                }
                println!("{name} seed {seed}: roofs draped {drapes:?}");
                // (A napkin draped as a roof wherever the plans have one: a plan has none only where none rests steadily
                // on that map, as a plate alone on its rim would not, and on the first and fourth seeds every map has.)
                let roofed = (0..2).any(|team| s.plan_of(team).unwrap().places.iter().any(|p| p.class == Class::Roof));
                assert!(roofed || ![0, 3].contains(&seed), "{name} seed {seed}: no roof in either plan");
                assert!(!roofed || drapes.iter().sum::<usize>() >= 1, "{name} seed {seed}: no napkin is draped over anything");
            }
        }
        println!("{total} of {planned} places built");
        assert!(total * 100 >= planned * 85, "{total} of {planned} places built");
    }

    /// The same seed builds the same forts, piece for piece; another chooses other designs.
    #[test]
    fn a_seed_builds_the_same_forts_every_time_and_the_seeds_vary_them() {
        let snapshot = |s: &Feast| (0..s.props.len()).map(|i| (s.props.state(i) as u8, s.props.position(i).map(f64::to_bits), s.props.rotation(i).map(f64::to_bits))).collect::<Vec<_>>();
        let (a, b) = (built("pillow-fort", 7), built("pillow-fort", 7));
        assert!(snapshot(&a) == snapshot(&b), "the same seed, the same forts");
        assert_eq!(a.plan_of(0), b.plan_of(0));
        assert_eq!(a.plan_of(1), b.plan_of(1));
        // Over a handful of seeds and the maps, more than one design is chosen, plain and mirrored.
        let mut chosen = std::collections::BTreeSet::new();
        for name in MAPS {
            for seed in 0..6 {
                let mut s = Feast::with_map(Map::parse(&read(&format!("assets/maps/{name}.json"))).unwrap(), true);
                s.card_seed = seed;
                s.tick(DT);
                for team in 0..2 {
                    let plan = s.plan_of(team).unwrap();
                    chosen.insert((plan.design, plan.flipped));
                }
            }
        }
        assert!(chosen.iter().map(|(d, _)| *d).collect::<std::collections::BTreeSet<_>>().len() == DESIGNS.len(), "every design is chosen somewhere: {chosen:?}");
        assert!(chosen.iter().any(|(_, f)| *f) && chosen.iter().any(|(_, f)| !*f), "plain and mirrored: {chosen:?}");
    }

    /// What the bots' building costs a step: drawing a plan (once a round a team), a step's work for six bots building
    /// (their looking for pieces, their ways round the forts, `placement` before each put), and a whole step with all of
    /// it, on the busiest bundled map.
    #[test]
    fn the_bots_building_costs_little_per_step() {
        let (plan_budget, bots_budget, step_budget) = if cfg!(debug_assertions) { (100., 20., 40.) } else { (8., 0.5, 1.5) };
        // (HOTDOG_TIMING_SLACK multiplies all three on a slow or shared machine.)
        let slack = crate::timing_slack();
        let (plan_budget, bots_budget, step_budget) = (plan_budget * slack, bots_budget * slack, step_budget * slack);
        for name in ["the-whole-kitchen-forts", "jungle-fort"] {
            let mut s = Feast::with_map(Map::parse(&read(&format!("assets/maps/{name}.json"))).unwrap(), true);
            let mut drawing: Vec<f64> = (0..10)
                .map(|_| {
                    let from = std::time::Instant::now();
                    std::hint::black_box(s.plan(0));
                    from.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            drawing.sort_by(f64::total_cmp);
            // The bots' own work, step by step: they are switched off for the step itself and do their work beforehand,
            // timed (as the step does it, in the same order).
            let ids: Vec<u32> = s.players.iter().map(|p| p.id).collect();
            for p in &mut s.players {
                p.bot = false;
            }
            let (mut bots, mut steps) = (vec![], vec![]);
            for k in 0..30 * 120 {
                let from = std::time::Instant::now();
                for id in &ids {
                    s.fort_bot(*id, DT);
                }
                let mid = std::time::Instant::now();
                s.tick(DT);
                if k % 3 == 0 {
                    bots.push((mid - from).as_secs_f64() * 1e3);
                    steps.push(from.elapsed().as_secs_f64() * 1e3);
                }
            }
            bots.sort_by(f64::total_cmp);
            steps.sort_by(f64::total_cmp);
            let (bot, step, worst) = (bots[bots.len() / 2], steps[steps.len() / 2], bots[bots.len() * 99 / 100]);
            println!("{name}: drawing a plan {:.3} ms; six bots building {bot:.3} ms a step (median), {worst:.3} ms (99th centile); a whole step {step:.3} ms", drawing[5]);
            assert!(drawing[2] < plan_budget, "{name}: a plan takes {:.3} ms", drawing[2]);
            assert!(bot < bots_budget && step < step_budget, "{name}: bots {bot:.3} ms, step {step:.3} ms");
            let plan = s.plan_of(0).unwrap();
            assert!(plan.built() * 2 >= plan.places.len(), "{name}: the bots built {} of {} places in two minutes", plan.built(), plan.places.len());
        }
    }

    /// The plans themselves: every place of every chosen plan lies on the team's side of the field, on the counter, out of
    /// both bases and clear of the map's blocks, and leaves a way into the base; each design is mirrored for the other
    /// team.
    #[test]
    fn every_place_of_a_plan_is_on_its_teams_side_out_of_the_bases_and_the_plan_leaves_a_way_in() {
        for name in MAPS {
            let s = Feast::with_map(Map::parse(&read(&format!("assets/maps/{name}.json"))).unwrap(), true);
            let fort = s.level.fort.clone().unwrap();
            for design in 0..DESIGNS.len() {
                for flipped in [false, true] {
                    let plans: Vec<Plan> = (0..2).map(|team| s.fit_design(team, design, flipped).unwrap().2).collect();
                    for (team, plan) in plans.iter().enumerate() {
                        for p in &plan.places {
                            let r = p.rect;
                            assert!(s.level.side((r[0] + r[2]) / 2., (r[1] + r[3]) / 2.) == fort.sides[team], "{name} {}: {p:?} off its side", DESIGNS[design].name);
                            assert!(fort.bases.iter().all(|b| !overlaps(r, *b, 0.)), "{name} {}: {p:?} in a base", DESIGNS[design].name);
                            if p.class != Class::Roof && p.on.is_none() {
                                assert!(!s.level.map.blocks.iter().any(|b| overlaps(r, b.rect, 0.)), "{name} {}: {p:?} in a block", DESIGNS[design].name);
                            }
                        }
                        // (A design that would leave none is never chosen: the fit says so.)
                        let rects: Vec<[f64; 4]> = plan.places.iter().filter(|p| p.on.is_none() && p.class != Class::Roof).map(|p| p.rect).collect();
                        let open = way_in(&s.fort.as_ref().unwrap().open, &rects, fort.bases[team], fort.bacon);
                        assert!(open || !s.fit_design(team, design, flipped).unwrap().1, "{name} {}: no way in", DESIGNS[design].name);
                    }
                    // The other team's is the same drawn mirrored across the field.
                    assert_eq!(plans[0].places.len(), plans[1].places.len(), "{name} {}", DESIGNS[design].name);
                    for (a, b) in plans[0].places.iter().zip(&plans[1].places) {
                        assert!((a.at[0] + b.at[0]).abs() < 1e-9 && (a.at[1] - b.at[1]).abs() < 1e-9 && a.class == b.class, "{name} {}: {a:?} and {b:?}", DESIGNS[design].name);
                    }
                }
            }
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Map format, collision and navigation for the game.
//!
//! tools/network/map.mjs implements the same rules for the server and the map editor, and
//! tools/network/fixtures/map-golden.json keeps the two in step: both test suites must reproduce its
//! fingerprints. Everything here is computed in f64, as JavaScript does, so the results match exactly.

use glam::{Vec2, Vec3};
use serde_json::{Map as Object, Value};
use std::fmt;

pub const FORMAT: &str = "hotdog-map";
/// The newest map version this build reads. A map is written as version 1 unless it uses something version 2
/// added (see `Map::version`): sinks, the fitted cabinets, the kitchen floor (`floor`), the counter things among KITS
/// after the first two (`FIRST_KITS`), the styles after the first nine (`FIRST_STYLES`), a ramp with a style or one that
/// starts below the counter, and the bin. So a map that needs none
/// of them stays byte for byte what it was, and builds that read only version 1 still read it; one that does need
/// them is refused by those builds with a reason, rather than played without its sink.
pub const VERSION: f64 = 2.;
/// Height of the counter top in world space; every map height is measured from it.
pub const FLOOR: f64 = 0.9025;
/// The materials of blocks, decks, ramps and boxes. Version 1 knew the first nine (`FIRST_STYLES`). Bamboo is also a
/// shape: a deck of bamboo is round (a bamboo steamer's lid: the ellipse inside its rect is its floor, see `round`), and
/// a slim block of it is a chopstick standing in its holder. Card, the last, is cardboard: the box on Titan's Kitchen's
/// floor that a hotdog climbs.
pub const STYLES: [&str; 11] = [
    "steel", "stone", "wood", "dark", "white", "red", "green", "yellow", "metal", "bamboo", "card",
];
pub const FIRST_STYLES: usize = 9;
/// The style whose decks are round.
pub const ROUND: &str = "bamboo";
/// Appliances and the things that stand on a kitchen counter: blocks of the map (a box each to walk round, shoot at
/// and stand on) that the game draws as what they are, with a front on their `facing` side. Version 1 knew the
/// first two (`FIRST_KITS`). The griddle is also one that does something: it is switched on and off, heats and burns
/// (griddle.rs). The gong is a wok hung in a frame of long chopsticks, Kung Fu's: it is struck at a knockout. The last
/// three stand on the kitchen floor of a `floor` map (Titan's Kitchen), at their true sizes: the fridge (its door ajar,
/// lit inside), the oven (a freestanding range: oven door, hob, a clock that glows) and a pet's bowl.
pub const KITS: [&str; 17] = [
    "microwave", "toaster", "kettle", "knife-block", "canister", "jar", "cereal", "paper-towel", "dish-rack", "spice-rack", "fruit-bowl", "faucet", "griddle", "gong", "fridge", "oven", "pet-bowl",
];
pub const FIRST_KITS: usize = 2;
/// A sink's depth below the counter when the map does not give one, and the deepest a sink (or the foot of a ramp
/// down into one) may go.
pub const SINK_DEPTH: f64 = 0.18;
pub const SINK_MAX: f64 = 0.5;
/// Loose objects: props that can be knocked about. The game simulates them as rigid bodies (props.rs). The first six
/// were the game's own (the napkin, a tea towel's size, is the sheet that drapes: drape.rs); the rest are the kitchen's
/// at their true sizes beside a 15 cm hotdog (a family-size cereal box, a mug, a dinner plate, a tin, a cutting board,
/// a cereal bowl, a roll of paper towels, salt and pepper shakers and a bread bin), for Forts' builders.
pub const LOOSE: [&str; 16] = ["bowl", "crate", "pillow", "napkin", "chair", "tray", "cereal", "cup", "plate", "tin", "board", "dish", "roll", "salt", "pepper", "breadbox"];
/// What each kind of loose object is called on the screen (the hints' "Q: pick up the ..."), in LOOSE's order.
pub const LOOSE_NAMES: [&str; 16] = [
    "bowl", "crate", "pillow", "napkin", "chair", "tray", "cereal box", "mug", "plate", "tin can", "cutting board", "cereal bowl", "paper towel roll", "salt shaker", "pepper shaker", "bread bin",
];
/// A kind of loose object that maps once called by another name: a blanket is a napkin now.
pub const LOOSE_FORMERLY: [(&str, &str); 1] = [("blanket", "napkin")];
/// How the game dresses and lights the map; it changes nothing about collision or navigation.
pub const THEMES: [&str; 3] = ["kitchen", "night", "jungle"];
/// What a map was made for: "siege" (the zombie wave, then the free for all), "forts" (two teams build a fortress
/// each side of a forcefield, then fight over the bacon), "kungfu" (hands and feet only, in a ring) or "titan" (one
/// giant hotdog against all the others: its first start is the titan's, the rest the small hotdogs'; its box props
/// with a base are the dens only small hotdogs fit under). It is the mode a game of the map is played in when nothing
/// else says (rules.rs `Mode::made_for`): the mode is the game's, and a map may be played in any mode it can hold
/// (`Map::refuses`).
pub const MODES: [&str; 4] = ["siege", "forts", "kungfu", "titan"];
/// The two teams of a Forts map, in the order the game numbers them (0 and 1), and what they are called.
pub const TEAMS: [&str; 2] = ["mustard", "ketchup"];
pub const TEAM_NAMES: [&str; 2] = ["Mustard Eaters", "Ketchup Connoisseurs"];
/// What a map marks out for Forts, in its `zones` (layout any map may carry; a map with all of them can be played as
/// Forts): each team's base (an area, with its team), the forcefield (a thin area right across the map) and the bacon's
/// spot (a point).
pub const ZONES: [&str; 3] = ["base", "field", "bacon"];
/// Seconds a Forts round's build phase lasts unless the game's rules say otherwise (the server's --build, the page's
/// ?build=): a rule, not the map's (a map made for Forts may still say `build`, which is read, written back and passed
/// over); and the hotdogs a Forts round has (three a side).
pub const BUILD: f64 = 150.;
pub const FORT_PLAYERS: usize = 6;
/// The largest counter the game builds a kitchen round, [min x, max x, max z] (tools/network/map.mjs has the same
/// `ROOM`): the room grows with a counter longer or deeper than the bundled one, keeping its walls 0.7 m past the
/// ends and just behind the back, up to walls at x = ±5.2 and z = 5.07. Towards -z the room is open, where the
/// fridge stands.
pub const ROOM: [f64; 3] = [-4.5, 4.5, 5.];
/// Movement constants, in metres: the tallest ledge a hotdog steps onto, how close its body gets to a block,
/// how far above a block's top it must be to pass over, and how far a shot travels.
pub const STEP: f64 = 0.045;
pub const MARGIN: f64 = 0.03;
pub const LIP: f64 = 0.005;
pub const RANGE: f64 = 12.;
/// Highest weapon number a pickup may hold (7, the mayonnaise bottle). A pickup's weapon 0 is a box of ammunition
/// (AMMO_BOX: there are no bare hands, so the number was free), which has no count of its own.
pub const WEAPON_MAX: f64 = 7.;
pub const AMMO_BOX: usize = 0;
/// A map's pickups are spawn points: one taken comes back where it was this many seconds later, a gun's and a box
/// of ammunition's (tools/network/map.mjs has the same RESPAWN). The game takes the times from its rules (magazine.rs
/// COMEBACK, the same numbers); a map's own `respawn` is read, written back and passed over.
pub const RESPAWN: [f64; 2] = [20., 12.];
const CLIMB: f64 = 0.12;
const FOOT: f64 = 0.03;
const HEAD: f64 = 0.15;
const EXTENT: f64 = 50.;
const HEIGHT: f64 = 3.;
const CELLS: f64 = 1_048_576.;
const UNIT: f64 = 1e4;
/// The map the game ships with.
pub const BUNDLED: &str = include_str!("../../../assets/maps/counter-siege.json");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapError(pub String);
impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for MapError {}
type Checked<T> = Result<T, MapError>;
fn fail<T>(message: String) -> Checked<T> {
    Err(MapError(message))
}

/// Direction a ramp climbs, or an appliance faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    PlusX,
    MinusX,
    PlusZ,
    MinusZ,
}
impl Side {
    const NAMES: [&'static str; 4] = ["+x", "-x", "+z", "-z"];
    /// The side's name in a map file.
    pub fn name(self) -> &'static str {
        Self::NAMES[match self {
            Side::PlusX => 0,
            Side::MinusX => 1,
            Side::PlusZ => 2,
            Side::MinusZ => 3,
        }]
    }
    fn named(name: &str) -> Side {
        match name {
            "+x" => Side::PlusX,
            "-x" => Side::MinusX,
            "+z" => Side::PlusZ,
            _ => Side::MinusZ,
        }
    }
    pub fn along_z(self) -> bool {
        matches!(self, Side::PlusZ | Side::MinusZ)
    }
    pub fn sign(self) -> f64 {
        if matches!(self, Side::PlusX | Side::PlusZ) { 1. } else { -1. }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    pub p: [f64; 2],
    pub yaw: f64,
    /// The team the start belongs to (0 mustard, 1 ketchup), when it names one: a Forts map's starts all do.
    pub team: Option<usize>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Spawn {
    pub p: [f64; 2],
}
#[derive(Clone, Debug, PartialEq)]
pub struct Pickup {
    pub p: [f64; 2],
    /// 1 to 7 a gun, 0 (AMMO_BOX) a box of ammunition.
    pub weapon: usize,
    /// The rounds the file says a gun's pickup gives (0: it says none; always 0 for a box of ammunition). Read and
    /// written back, but passed over by the game, whose pickups give what its rules say (magazine.rs PICKUP).
    pub ammo: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub rect: [f64; 4],
    pub height: f64,
    pub style: String,
    /// Set for appliances (microwave, toaster): the side their front is on.
    pub facing: Option<Side>,
}
/// A raised floor over `rect`, `height` above the counter. One of the ROUND style is round: its floor is the ellipse
/// inside the rect (`round`), and beyond it, in the rect's corners, is the counter.
#[derive(Clone, Debug, PartialEq)]
pub struct Deck {
    pub rect: [f64; 4],
    pub height: f64,
    pub style: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Ramp {
    pub rect: [f64; 4],
    /// The height of its foot. Below 0 (down to -SINK_MAX) only in a sink: a board propped in it, a way out.
    pub base: f64,
    pub height: f64,
    pub rise: Side,
    /// One of STYLES when the map gives it: the game draws the ramp as a plank of that material rather than the
    /// metal ramp with yellow edges. It changes nothing else.
    pub style: Option<String>,
}
/// A sink: a basin `depth` metres deep sunk into the counter over `rect`. Its floor is ground like any other (so
/// hotdogs, zombies, loose objects and rounds go down into it) and its sides are the counter's edge, a ledge too
/// tall to step up: a way out is a ramp whose foot stands on its floor, or a stimmed jump. Routes keep out of it
/// except by those ramps (see `Level::new`).
#[derive(Clone, Debug, PartialEq)]
pub struct Sink {
    pub rect: [f64; 4],
    pub depth: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Box { rect: [f64; 4], base: f64, height: f64, style: String },
    Turret { p: [f64; 2], base: f64, yaw: f64 },
    /// A kitchen bin standing on the kitchen floor (beside the counter, not on it), turned by `yaw`: scenery only.
    Bin { p: [f64; 2], yaw: f64 },
}
/// A loose object as the map places it: its kind (one of LOOSE), where it stands, the height it starts at and its turn.
#[derive(Clone, Debug, PartialEq)]
pub struct Loose {
    pub kind: String,
    pub p: [f64; 2],
    pub base: f64,
    pub yaw: f64,
}
/// One of a Forts map's zones.
#[derive(Clone, Debug, PartialEq)]
pub enum Zone {
    Base { team: usize, rect: [f64; 4] },
    Field { rect: [f64; 4] },
    Bacon { p: [f64; 2] },
}
/// What a Forts map says, with what it leaves out filled in, so that an unfinished map still plays (see
/// `Map::fortress`).
#[derive(Clone, Debug, PartialEq)]
pub struct Fort {
    /// 0 when the forcefield divides the map along x (it runs along z), 1 when along z.
    pub axis: usize,
    /// The field's middle on that axis.
    pub mid: f64,
    /// The forcefield's area.
    pub field: [f64; 4],
    /// Each team's base, by team number.
    pub bases: [[f64; 4]; 2],
    /// The side of the field each team is on: -1 below `mid`, 1 above.
    pub sides: [i32; 2],
    /// The bacon's spot, [x, z].
    pub bacon: [f64; 2],
    /// Each team's starts (each with `team` set to that team's number).
    pub starts: [Vec<Start>; 2],
    /// Seconds the build phase lasts.
    pub build: f64,
}
/// A checked map in canonical form: defaults filled in and coordinates on the 0.1 mm lattice.
#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    pub name: String,
    /// One of THEMES.
    pub theme: String,
    /// One of MODES.
    pub mode: String,
    /// Seconds a Forts map says its build phase lasts (BUILD for any other map): read and written back, but the game's
    /// build time is a rule (BUILD unless the server or the page say otherwise).
    pub build: f64,
    /// Seconds the map says a pickup takes to come back once taken: a gun's, a box of ammunition's (RESPAWN unless the
    /// map says otherwise). Read and written back, but the game's times are its rules' (magazine.rs COMEBACK).
    pub respawn: [f64; 2],
    /// Whether the map gives `respawn` itself (its canonical text then writes it).
    pub respawn_given: bool,
    /// Fitted cabinets: doors and drawers down the counter's front and ends, and dressed wall cupboards. Scenery only.
    pub cabinets: bool,
    /// The map's ground is the kitchen floor, not a counter top: the game draws tiles, and a room whose walls stand at
    /// the bounds, with a ceiling 2.7 m up and a window (Titan's Kitchen). Scenery only: heights still count from FLOOR.
    pub floor: bool,
    /// [min x, min z, max x, max z]
    pub bounds: [f64; 4],
    pub cell: f64,
    pub clearance: f64,
    pub starts: Vec<Start>,
    pub spawns: Vec<Spawn>,
    pub pickups: Vec<Pickup>,
    pub blocks: Vec<Block>,
    pub decks: Vec<Deck>,
    pub ramps: Vec<Ramp>,
    pub sinks: Vec<Sink>,
    pub props: Vec<Prop>,
    /// The loose objects among the file's props, in file order.
    pub loose: Vec<Loose>,
    /// The file's props in its order: true for the next of `loose`, false for the next of `props` (the canonical
    /// text writes them back in this order).
    pub order: Vec<bool>,
    /// A Forts map's bases, forcefield and bacon.
    pub zones: Vec<Zone>,
}

// Coordinates are kept on a 0.1 mm lattice so every implementation derives the same navigation grid from integers.
fn quantum(v: f64) -> f64 {
    (v * UNIT + 0.5).floor()
}
fn trim(v: f64) -> f64 {
    quantum(v) / UNIT
}
fn number(v: Option<&Value>, path: &str, min: f64, max: f64, fallback: Option<f64>) -> Checked<f64> {
    let Some(v) = v else {
        return match fallback {
            Some(value) => Ok(value),
            None => fail(format!("{path} must be a number")),
        };
    };
    let Some(n) = v.as_f64().filter(|n| v.is_number() && n.is_finite()) else {
        return fail(format!("{path} must be a number"));
    };
    if n < min || n > max {
        return fail(format!("{path} must be between {min} and {max}"));
    }
    Ok(trim(n))
}
fn whole(v: Option<&Value>, path: &str, min: f64, max: f64) -> Checked<f64> {
    match v.and_then(Value::as_f64) {
        Some(n) if n.fract() == 0. && n >= min && n <= max => Ok(n),
        _ => fail(format!("{path} must be a whole number from {min} to {max}")),
    }
}
fn point(v: Option<&Value>, path: &str) -> Checked<[f64; 2]> {
    let Some(list) = v.and_then(Value::as_array).filter(|list| list.len() == 2) else {
        return fail(format!("{path} must be [x, z]"));
    };
    let mut p = [0.; 2];
    for (i, c) in list.iter().enumerate() {
        p[i] = number(Some(c), &format!("{path}[{i}]"), -EXTENT, EXTENT, None)?;
    }
    Ok(p)
}
fn rect(v: Option<&Value>, path: &str) -> Checked<[f64; 4]> {
    let Some(list) = v.and_then(Value::as_array).filter(|list| list.len() == 4) else {
        return fail(format!("{path} must be [minX, minZ, maxX, maxZ]"));
    };
    let mut r = [0.; 4];
    for (i, c) in list.iter().enumerate() {
        r[i] = number(Some(c), &format!("{path}[{i}]"), -EXTENT, EXTENT, None)?;
    }
    if r[2] - r[0] < 0.001 || r[3] - r[1] < 0.001 {
        return fail(format!("{path} is inside out or empty; expected [minX, minZ, maxX, maxZ]"));
    }
    Ok(r)
}
fn choice(v: Option<&Value>, path: &str, allowed: &[&str], fallback: Option<&str>) -> Checked<String> {
    match (v, fallback) {
        (None, Some(value)) => Ok(value.to_string()),
        (Some(Value::String(s)), _) if allowed.contains(&s.as_str()) => Ok(s.clone()),
        _ => fail(format!("{path} must be one of {}", allowed.join(", "))),
    }
}
fn tall(v: Option<&Value>, path: &str, fallback: f64, min: f64) -> Checked<f64> {
    number(v, path, min, HEIGHT, Some(fallback))
}
fn object<'a>(v: &'a Value, path: &str) -> Checked<&'a Object<String, Value>> {
    match v.as_object() {
        Some(object) => Ok(object),
        None => fail(format!("{path} must be an object")),
    }
}
/// The entries of one kind of object, checked against how many a map may hold.
fn list<'a>(input: &'a Object<String, Value>, kind: &str, required: usize, limit: usize) -> Checked<&'a [Value]> {
    let items: &[Value] = match input.get(kind) {
        None | Some(Value::Null) => &[],
        Some(Value::Array(items)) => items,
        Some(_) => return fail(format!("{kind} must be a list")),
    };
    if items.len() < required {
        return fail(format!("A map needs at least one {}", kind.trim_end_matches('s')));
    }
    if items.len() > limit {
        return fail(format!("{kind} holds {} entries; the limit is {limit}", items.len()));
    }
    Ok(items)
}

impl Map {
    /// Reads and checks a map file. Unknown keys are ignored; anything out of range is an error.
    pub fn parse(text: &str) -> Checked<Map> {
        match serde_json::from_str::<Value>(text) {
            Ok(value) => Map::from_value(&value),
            Err(error) => fail(format!("Not valid JSON: {error}")),
        }
    }
    /// The map the game ships with.
    pub fn bundled() -> Map {
        Map::parse(BUNDLED).expect("the bundled map is valid")
    }
    pub fn from_value(value: &Value) -> Checked<Map> {
        let Some(input) = value.as_object() else {
            return fail("A map must be a JSON object".into());
        };
        let format = input.get("format").cloned().unwrap_or(Value::Null);
        if format.as_str() != Some(FORMAT) {
            return fail(format!("Unknown format {format}; expected \"{FORMAT}\""));
        }
        let version = input.get("version").cloned().unwrap_or(Value::Null);
        if !version.as_f64().is_some_and(|v| v == 1. || v == VERSION) {
            return fail(format!("Unsupported map version {version}; this build reads versions 1 and {VERSION}"));
        }
        let name = match input.get("name").and_then(Value::as_str).map(|s| s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')) {
            Some(name) if !name.is_empty() => String::from_utf16_lossy(&name.encode_utf16().take(64).collect::<Vec<_>>()),
            _ => "Untitled map".to_string(),
        };
        let theme = choice(input.get("theme"), "theme", &THEMES, Some("kitchen"))?;
        let mode = choice(input.get("mode"), "mode", &MODES, Some("siege"))?;
        let build = if mode == "forts" { number(input.get("build"), "build", 5., 900., Some(BUILD))? } else { BUILD };
        let (respawn, respawn_given) = match input.get("respawn") {
            None | Some(Value::Null) => (RESPAWN, false),
            Some(Value::Array(list)) if list.len() == 2 => ([number(Some(&list[0]), "respawn[0]", 1., 600., None)?, number(Some(&list[1]), "respawn[1]", 1., 600., None)?], true),
            Some(_) => return fail("respawn must be [weapon seconds, ammo seconds]".into()),
        };
        let flag = |key: &str| match input.get(key) {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(given)) => Ok(*given),
            Some(_) => fail(format!("{key} must be true or false")),
        };
        let (cabinets, floor) = (flag("cabinets")?, flag("floor")?);
        let bounds = rect(input.get("bounds"), "bounds")?;
        if bounds[2] - bounds[0] < 0.2 || bounds[3] - bounds[1] < 0.2 {
            return fail("bounds must be at least 0.2 m on each side".into());
        }
        let nav = input.get("nav").and_then(Value::as_object);
        let cell = number(nav.and_then(|nav| nav.get("cell")), "nav.cell", 0.01, 0.5, Some(0.04))?;
        let clearance = number(nav.and_then(|nav| nav.get("clearance")), "nav.clearance", 0., 0.2, Some(0.035))?;
        let cells = ((quantum(bounds[2]) - quantum(bounds[0])) / quantum(cell)).ceil() * ((quantum(bounds[3]) - quantum(bounds[1])) / quantum(cell)).ceil();
        if cells > CELLS {
            return fail(format!("The navigation grid would have {cells} cells; the limit is {CELLS}. Enlarge nav.cell or shrink bounds"));
        }

        let mut map = Map { name, theme, mode, build, respawn, respawn_given, cabinets, floor, bounds, cell, clearance, starts: vec![], spawns: vec![], pickups: vec![], blocks: vec![], decks: vec![], ramps: vec![], sinks: vec![], props: vec![], loose: vec![], order: vec![], zones: vec![] };
        let team = |v: Option<&Value>, path: &str| choice(v, path, &TEAMS, None).map(|name| TEAMS.iter().position(|team| *team == name).unwrap_or(0));
        for (index, item) in list(input, "starts", 1, 16)?.iter().enumerate() {
            let path = format!("starts[{index}]");
            let s = object(item, &path)?;
            let (p, yaw) = (point(s.get("p"), &format!("{path}.p"))?, number(s.get("yaw"), &format!("{path}.yaw"), -7., 7., Some(0.))?);
            // A start may name the team it belongs to: a Forts map's starts all do.
            let side = match s.get("team") {
                None | Some(Value::Null) => None,
                named => Some(team(named, &format!("{path}.team"))?),
            };
            map.starts.push(Start { p, yaw, team: side });
        }
        for (index, item) in list(input, "spawns", 1, 64)?.iter().enumerate() {
            let path = format!("spawns[{index}]");
            map.spawns.push(Spawn { p: point(object(item, &path)?.get("p"), &format!("{path}.p"))? });
        }
        for (index, item) in list(input, "pickups", 0, 256)?.iter().enumerate() {
            let path = format!("pickups[{index}]");
            let p = object(item, &path)?;
            let (at, weapon) = (point(p.get("p"), &format!("{path}.p"))?, whole(p.get("weapon"), &format!("{path}.weapon"), 0., WEAPON_MAX)? as usize);
            // A box of ammunition has no count, and a gun's is optional (the game's rules say what a pickup gives).
            let ammo = match p.get("ammo") {
                _ if weapon == AMMO_BOX => 0,
                None | Some(Value::Null) => 0,
                given => whole(given, &format!("{path}.ammo"), 1., 999.)? as u32,
            };
            map.pickups.push(Pickup { p: at, weapon, ammo });
        }
        let every: Vec<&str> = STYLES.iter().chain(KITS.iter()).copied().collect();
        for (index, item) in list(input, "blocks", 0, 4096)?.iter().enumerate() {
            let path = format!("blocks[{index}]");
            let b = object(item, &path)?;
            let rect = rect(b.get("rect"), &format!("{path}.rect"))?;
            let height = tall(b.get("height"), &format!("{path}.height"), 0.25, 0.02)?;
            let style = choice(b.get("style"), &format!("{path}.style"), &every, Some("steel"))?;
            let facing = if KITS.contains(&style.as_str()) {
                Some(Side::named(&choice(b.get("facing"), &format!("{path}.facing"), &Side::NAMES, Some("-z"))?))
            } else {
                None
            };
            map.blocks.push(Block { rect, height, style, facing });
        }
        for (index, item) in list(input, "decks", 0, 1024)?.iter().enumerate() {
            let path = format!("decks[{index}]");
            let d = object(item, &path)?;
            map.decks.push(Deck {
                rect: rect(d.get("rect"), &format!("{path}.rect"))?,
                height: tall(d.get("height"), &format!("{path}.height"), 0.28, 0.01)?,
                style: choice(d.get("style"), &format!("{path}.style"), &STYLES, Some("metal"))?,
            });
        }
        for (index, item) in list(input, "ramps", 0, 1024)?.iter().enumerate() {
            let path = format!("ramps[{index}]");
            let r = object(item, &path)?;
            let ramp = Ramp {
                rect: rect(r.get("rect"), &format!("{path}.rect"))?,
                base: number(r.get("base"), &format!("{path}.base"), -SINK_MAX, HEIGHT, Some(0.))?,
                height: tall(r.get("height"), &format!("{path}.height"), 0.28, 0.01)?,
                rise: Side::named(&choice(r.get("rise"), &format!("{path}.rise"), &Side::NAMES, None)?),
                style: match r.get("style") {
                    None | Some(Value::Null) => None,
                    given => Some(choice(given, &format!("{path}.style"), &STYLES, None)?),
                },
            };
            if ramp.height - ramp.base < 0.005 {
                return fail(format!("{path} must rise: height has to be above base"));
            }
            map.ramps.push(ramp);
        }
        for (index, item) in list(input, "sinks", 0, 16)?.iter().enumerate() {
            let path = format!("sinks[{index}]");
            let s = object(item, &path)?;
            map.sinks.push(Sink { rect: rect(s.get("rect"), &format!("{path}.rect"))?, depth: number(s.get("depth"), &format!("{path}.depth"), 0.05, SINK_MAX, Some(SINK_DEPTH))? });
        }
        for (index, item) in list(input, "props", 0, 4096)?.iter().enumerate() {
            let path = format!("props[{index}]");
            let p = object(item, &path)?;
            let base = |p: &Object<String, Value>| number(p.get("base"), &format!("{path}.base"), 0., HEIGHT, Some(0.));
            let kinds: Vec<&str> = ["box", "turret"].iter().chain(LOOSE.iter()).chain(["bin"].iter()).copied().collect();
            // (A kind by its old name is read as its new one.)
            let renamed = p.get("kind").and_then(Value::as_str).and_then(|k| LOOSE_FORMERLY.iter().find(|(old, _)| *old == k)).map(|(_, new)| Value::from(*new));
            let kind = choice(renamed.as_ref().or(p.get("kind")), &format!("{path}.kind"), &kinds, Some("box"))?;
            if LOOSE.contains(&kind.as_str()) {
                map.loose.push(Loose { kind, p: point(p.get("p"), &format!("{path}.p"))?, base: base(p)?, yaw: number(p.get("yaw"), &format!("{path}.yaw"), -7., 7., Some(0.))? });
                map.order.push(true);
                continue;
            }
            map.order.push(false);
            map.props.push(if kind == "turret" {
                Prop::Turret { p: point(p.get("p"), &format!("{path}.p"))?, base: base(p)?, yaw: number(p.get("yaw"), &format!("{path}.yaw"), -7., 7., Some(0.))? }
            } else if kind == "bin" {
                Prop::Bin { p: point(p.get("p"), &format!("{path}.p"))?, yaw: number(p.get("yaw"), &format!("{path}.yaw"), -7., 7., Some(0.))? }
            } else {
                Prop::Box {
                    rect: rect(p.get("rect"), &format!("{path}.rect"))?,
                    base: base(p)?,
                    height: tall(p.get("height"), &format!("{path}.height"), 0.1, 0.005)?,
                    style: choice(p.get("style"), &format!("{path}.style"), &STYLES, Some("wood"))?,
                }
            });
        }
        for (index, item) in list(input, "zones", 0, 16)?.iter().enumerate() {
            let path = format!("zones[{index}]");
            let z = object(item, &path)?;
            let kind = choice(z.get("kind"), &format!("{path}.kind"), &ZONES, None)?;
            map.zones.push(match kind.as_str() {
                "bacon" => Zone::Bacon { p: point(z.get("p"), &format!("{path}.p"))? },
                "field" => Zone::Field { rect: rect(z.get("rect"), &format!("{path}.rect"))? },
                _ => Zone::Base { team: team(z.get("team"), &format!("{path}.team"))?, rect: rect(z.get("rect"), &format!("{path}.rect"))? },
            });
        }
        Ok(map)
    }
    /// Whether this map was made for Forts (its `mode` tag).
    pub fn forts(&self) -> bool {
        self.mode == "forts"
    }
    /// Whether the map has the Forts layout in its zones: a base for each team, the field and the bacon's spot.
    pub fn fort_layout(&self) -> bool {
        let base = |team: usize| self.zones.iter().any(|z| matches!(z, Zone::Base { team: t, .. } if *t == team));
        base(0) && base(1) && self.zones.iter().any(|z| matches!(z, Zone::Field { .. })) && self.zones.iter().any(|z| matches!(z, Zone::Bacon { .. }))
    }
    /// The version the map is written as (see VERSION): 2 when it uses anything version 1 did not have, 1 otherwise.
    pub fn version(&self) -> f64 {
        let late = |style: &str| STYLES[FIRST_STYLES..].contains(&style);
        let newer = self.cabinets
            || self.floor
            || !self.sinks.is_empty()
            || self.blocks.iter().any(|b| KITS[FIRST_KITS..].contains(&b.style.as_str()) || late(&b.style))
            || self.decks.iter().any(|d| late(&d.style))
            || self.ramps.iter().any(|r| r.base < 0. || r.style.is_some())
            || self.props.iter().any(|p| matches!(p, Prop::Bin { .. }) || matches!(p, Prop::Box { style, .. } if late(style)));
        if newer { VERSION } else { 1. }
    }
    /// What a map made for Forts, or any map with the whole Forts layout (`fort_layout`), says for Forts, with what it
    /// leaves out filled in, so that an unfinished Forts map still plays. None for any other map (one whose zones are
    /// not the whole layout cannot be played as Forts: nothing is made up for it). Without a field, a strip across the
    /// middle of the longer side stands in; without a base, the end eighth of the map on that team's side; without the
    /// bacon's spot, the middle of the field. Starts that name no team go to the team whose side they are on, and a team
    /// with no start begins in the middle of its base, facing the field.
    pub fn fortress(&self) -> Option<Fort> {
        if !self.forts() && !self.fort_layout() {
            return None;
        }
        let [x0, z0, x1, z1] = self.bounds;
        let given = self.zones.iter().find_map(|zone| match zone {
            Zone::Field { rect } => Some(*rect),
            _ => None,
        });
        let axis = match given {
            Some(given) => if given[2] - given[0] <= given[3] - given[1] { 0 } else { 1 },
            None => if x1 - x0 >= z1 - z0 { 0 } else { 1 },
        };
        let centre = if axis == 1 { (z0 + z1) / 2. } else { (x0 + x1) / 2. };
        let field = given.unwrap_or(if axis == 1 { [x0, centre - 0.03, x1, centre + 0.03] } else { [centre - 0.03, z0, centre + 0.03, z1] });
        let mid = (field[axis] + field[axis + 2]) / 2.;
        let side = |p: [f64; 2]| if p[axis] < mid { -1 } else { 1 };
        let (low, high) = (self.bounds[axis], self.bounds[axis + 2]);
        let end = (high - low) / 8.;
        let stand = |far: bool| {
            if axis == 1 {
                [x0, if far { high - end } else { low }, x1, if far { high } else { low + end }]
            } else {
                [if far { high - end } else { low }, z0, if far { high } else { low + end }, z1]
            }
        };
        let middle = |r: [f64; 4]| [(r[0] + r[2]) / 2., (r[1] + r[3]) / 2.];
        let given: [Option<[f64; 4]>; 2] = [0, 1].map(|team| {
            self.zones.iter().find_map(|zone| match zone {
                Zone::Base { team: owner, rect } if *owner == team => Some(*rect),
                _ => None,
            })
        });
        let mut sides = given.map(|base| base.map_or(0, |r| side(middle(r))));
        // A base on neither side yet takes the side its rival has left (the low side for the first team when
        // neither has one); two on one side are an error the editor reports.
        if sides[0] == 0 {
            sides[0] = if sides[1] != 0 { -sides[1] } else { -1 };
        }
        if sides[1] == 0 {
            sides[1] = -sides[0];
        }
        let bases = [0, 1].map(|team| given[team].unwrap_or(stand(sides[team] > 0)));
        let bacon = self.zones.iter().find_map(|zone| match zone {
            Zone::Bacon { p } => Some(*p),
            _ => None,
        }).unwrap_or(middle(field));
        let named = self.starts.iter().any(|start| start.team.is_some());
        let starts = [0, 1].map(|team| {
            let own: Vec<Start> = self
                .starts
                .iter()
                .filter(|start| if named { start.team == Some(team) } else { side(start.p) == sides[team] })
                .map(|start| Start { p: start.p, yaw: start.yaw, team: Some(team) })
                .collect();
            if !own.is_empty() {
                return own;
            }
            // Facing the field: yaw 0 looks along -z, and a quarter turn left along -x.
            let yaw = match (axis, sides[team] > 0) {
                (1, true) => 0.,
                (1, false) => std::f64::consts::PI,
                (_, true) => std::f64::consts::PI / 2.,
                (_, false) => -std::f64::consts::PI / 2.,
            };
            vec![Start { p: middle(bases[team]), yaw, team: Some(team) }]
        });
        Some(Fort { axis, mid, field, bases, sides, bacon, starts, build: self.build })
    }
}

/// Whether [x, z] lies on a round deck over `rect`: inside the ellipse that fills the rect (a circle in a square).
/// tools/network/map.mjs `round` works it out in the same order, so both agree to the last bit.
pub fn round(rect: [f64; 4], x: f64, z: f64) -> bool {
    let (dx, dz) = ((x - (rect[0] + rect[2]) / 2.) / ((rect[2] - rect[0]) / 2.), (z - (rect[1] + rect[3]) / 2.) / ((rect[3] - rect[1]) / 2.));
    dx * dx + dz * dz <= 1.
}

/// Entry distance of a ray into a box, or infinity when it misses.
fn ray_box(o: [f64; 3], d: [f64; 3], min: [f64; 3], max: [f64; 3]) -> f64 {
    let (mut near, mut far) = (0f64, f64::INFINITY);
    for i in 0..3 {
        if d[i].abs() < 1e-8 {
            if o[i] < min[i] || o[i] > max[i] {
                return f64::INFINITY;
            }
        } else {
            let (a, b) = ((min[i] - o[i]) / d[i], (max[i] - o[i]) / d[i]);
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return f64::INFINITY;
            }
        }
    }
    near
}

/// Where a shot from o along the unit vector d, once it is below the counter top over the sink `s` ([rect, depth]),
/// meets the sink's side or floor: infinity when it is never below the counter top there, or leaves the basin by
/// its open top.
fn basin(o: [f64; 3], d: [f64; 3], s: &[f64; 5]) -> f64 {
    let start = if o[1] >= FLOOR {
        if d[1] >= 0. {
            return f64::INFINITY;
        }
        (FLOOR - o[1]) / d[1]
    } else {
        0.
    };
    let (x, z) = (o[0] + d[0] * start, o[2] + d[2] * start);
    if !(x >= s[0] && x <= s[2] && z >= s[1] && z <= s[3]) {
        return f64::INFINITY;
    }
    // From there it is inside the basin, which it leaves through the nearest face ahead of it.
    let (low, high) = ([s[0], FLOOR - s[4], s[1]], [s[2], FLOOR, s[3]]);
    let (mut out, mut top) = (f64::INFINITY, false);
    for i in 0..3 {
        if d[i].abs() < 1e-8 {
            continue;
        }
        let t = ((if d[i] > 0. { high[i] } else { low[i] }) - o[i]) / d[i];
        if t < out {
            out = t;
            top = i == 1 && d[1] > 0.;
        }
    }
    if top { f64::INFINITY } else { out.max(start) }
}

/// Scratch space for route searches. Stamps mark which entries belong to the current search, so nothing is
/// cleared between searches.
#[derive(Clone, Debug, Default)]
struct Search {
    cost: Vec<f64>,
    parent: Vec<i32>,
    stamp: Vec<u32>,
    generation: u32,
    score: Vec<f64>,
    id: Vec<i32>,
    size: usize,
}
impl Search {
    fn push(&mut self, score: f64, id: i32) {
        if self.size == self.score.len() {
            self.score.push(0.);
            self.id.push(0);
        }
        let mut i = self.size;
        self.size += 1;
        while i > 0 {
            let p = (i - 1) >> 1;
            if self.score[p] <= score {
                break;
            }
            self.score[i] = self.score[p];
            self.id[i] = self.id[p];
            i = p;
        }
        self.score[i] = score;
        self.id[i] = id;
    }
    fn pop(&mut self) -> i32 {
        let top = self.id[0];
        self.size -= 1;
        let (score, id, size) = (self.score[self.size], self.id[self.size], self.size);
        let mut i = 0;
        loop {
            let mut c = i * 2 + 1;
            if c >= size {
                break;
            }
            if c + 1 < size && self.score[c + 1] < self.score[c] {
                c += 1;
            }
            if score <= self.score[c] {
                break;
            }
            self.score[i] = self.score[c];
            self.id[i] = self.id[c];
            i = c;
        }
        self.score[i] = score;
        self.id[i] = id;
        top
    }
}

/// A number as JavaScript's JSON.stringify writes it: whole numbers without a fraction, the shortest digits that
/// read back as the same number, and an exponent only below 1e-6 or from 1e21 up.
pub fn js_number(v: f64) -> String {
    if !v.is_finite() {
        return "null".into();
    }
    if v == 0. {
        return "0".into();
    }
    let size = v.abs();
    if (1e-6..1e21).contains(&size) {
        // (Rust writes a whole number without a fraction too, and never an exponent.)
        format!("{v}")
    } else {
        let text = format!("{v:e}");
        match text.split_once('e') {
            Some((digits, exponent)) if !exponent.starts_with('-') => format!("{digits}e+{exponent}"),
            _ => text,
        }
    }
}
/// A string as JSON.stringify writes it.
fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}
fn js_list(values: &[f64]) -> String {
    format!("[{}]", values.iter().map(|v| js_number(*v)).collect::<Vec<_>>().join(", "))
}
/// One object of a map, written inline: `{"key": value, ...}` in the order given.
fn js_object(fields: &[(&str, String)]) -> String {
    format!("{{{}}}", fields.iter().map(|(key, value)| format!("{}: {value}", js_string(key))).collect::<Vec<_>>().join(", "))
}
impl Map {
    /// The map's canonical text, byte for byte what tools/network/map.mjs `stringify` writes for the same map: the
    /// settings one to a line, then each list with one object to a line, so that maps diff cleanly. This is the
    /// text the server sends its clients, and the text of every map in assets/maps.
    pub fn to_text(&self) -> String {
        let mut lines: Vec<(&str, String)> = vec![
            ("format", js_string(FORMAT)),
            ("version", js_number(self.version())),
            ("name", js_string(&self.name)),
            ("theme", js_string(&self.theme)),
            ("mode", js_string(&self.mode)),
        ];
        if self.forts() {
            lines.push(("build", js_number(self.build)));
        }
        if self.respawn_given {
            lines.push(("respawn", js_list(&self.respawn)));
        }
        if self.cabinets {
            lines.push(("cabinets", "true".into()));
        }
        if self.floor {
            lines.push(("floor", "true".into()));
        }
        lines.push(("bounds", js_list(&self.bounds)));
        lines.push(("nav", js_object(&[("cell", js_number(self.cell)), ("clearance", js_number(self.clearance))])));
        let point = |p: &[f64; 2]| js_list(p);
        let starts = self.starts.iter().map(|s| {
            let mut fields = vec![("p", point(&s.p)), ("yaw", js_number(s.yaw))];
            if let Some(team) = s.team {
                fields.push(("team", js_string(TEAMS[team])));
            }
            js_object(&fields)
        });
        let spawns = self.spawns.iter().map(|s| js_object(&[("p", point(&s.p))]));
        let pickups = self.pickups.iter().map(|item| {
            let mut fields = vec![("p", point(&item.p)), ("weapon", item.weapon.to_string())];
            if item.weapon != AMMO_BOX && item.ammo > 0 {
                fields.push(("ammo", item.ammo.to_string()));
            }
            js_object(&fields)
        });
        let blocks = self.blocks.iter().map(|b| {
            let mut fields = vec![("rect", js_list(&b.rect)), ("height", js_number(b.height)), ("style", js_string(&b.style))];
            if let Some(side) = b.facing {
                fields.push(("facing", js_string(side.name())));
            }
            js_object(&fields)
        });
        let decks = self.decks.iter().map(|d| js_object(&[("rect", js_list(&d.rect)), ("height", js_number(d.height)), ("style", js_string(&d.style))]));
        let ramps = self.ramps.iter().map(|r| {
            let mut fields = vec![("rect", js_list(&r.rect)), ("base", js_number(r.base)), ("height", js_number(r.height)), ("rise", js_string(r.rise.name()))];
            if let Some(style) = &r.style {
                fields.push(("style", js_string(style)));
            }
            js_object(&fields)
        });
        let sinks: Vec<String> = self.sinks.iter().map(|s| js_object(&[("rect", js_list(&s.rect)), ("depth", js_number(s.depth))])).collect();
        let (mut fixed, mut loose) = (self.props.iter(), self.loose.iter());
        // A map built in code rather than read from a file has no order: its fixed props, then its loose ones.
        let order: Vec<bool> = if self.order.len() == self.props.len() + self.loose.len() {
            self.order.clone()
        } else {
            std::iter::repeat_n(false, self.props.len()).chain(std::iter::repeat_n(true, self.loose.len())).collect()
        };
        let props: Vec<String> = order
            .iter()
            .filter_map(|&is_loose| {
                Some(if is_loose {
                    let l = loose.next()?;
                    js_object(&[("kind", js_string(&l.kind)), ("p", point(&l.p)), ("base", js_number(l.base)), ("yaw", js_number(l.yaw))])
                } else {
                    match fixed.next()? {
                        Prop::Turret { p, base, yaw } => js_object(&[("kind", js_string("turret")), ("p", point(p)), ("base", js_number(*base)), ("yaw", js_number(*yaw))]),
                        Prop::Bin { p, yaw } => js_object(&[("kind", js_string("bin")), ("p", point(p)), ("yaw", js_number(*yaw))]),
                        Prop::Box { rect, base, height, style } => js_object(&[
                            ("kind", js_string("box")),
                            ("rect", js_list(rect)),
                            ("base", js_number(*base)),
                            ("height", js_number(*height)),
                            ("style", js_string(style)),
                        ]),
                    }
                })
            })
            .collect();
        let zones = self.zones.iter().map(|z| match z {
            Zone::Bacon { p } => js_object(&[("kind", js_string("bacon")), ("p", point(p))]),
            Zone::Field { rect } => js_object(&[("kind", js_string("field")), ("rect", js_list(rect))]),
            Zone::Base { team, rect } => js_object(&[("kind", js_string("base")), ("team", js_string(TEAMS[*team])), ("rect", js_list(rect))]),
        });
        let lists: [(&str, Vec<String>); 9] = [
            ("starts", starts.collect()),
            ("spawns", spawns.collect()),
            ("pickups", pickups.collect()),
            ("blocks", blocks.collect()),
            ("decks", decks.collect()),
            ("ramps", ramps.collect()),
            ("sinks", sinks),
            ("props", props),
            ("zones", zones.collect()),
        ];
        let mut out: Vec<String> = lines.into_iter().map(|(key, value)| format!("  {}: {value}", js_string(key))).collect();
        for (key, items) in lists {
            // A map without a sink says nothing of sinks, as version 1 did not.
            if key == "sinks" && items.is_empty() {
                continue;
            }
            out.push(if items.is_empty() {
                format!("  {}: []", js_string(key))
            } else {
                format!("  {}: [\n{}\n  ]", js_string(key), items.iter().map(|item| format!("    {item}")).collect::<Vec<_>>().join(",\n"))
            });
        }
        format!("{{\n{}\n}}\n", out.join(",\n"))
    }
}

/// A map prepared for play: the blocked-cell grid plus flat lists for the collision queries.
#[derive(Clone, Debug)]
pub struct Level {
    pub map: Map,
    pub x0: f64,
    pub z0: f64,
    pub x1: f64,
    pub z1: f64,
    pub cell: f64,
    pub cols: usize,
    pub rows: usize,
    /// One byte per navigation cell, row by row: 1 when zombies and routes may not enter it.
    pub blocked: Vec<u8>,
    /// Blocks grown by MARGIN, with the height a hotdog's feet must reach to pass over.
    walls: Vec<[f64; 5]>,
    /// Blocks as shots see them: rect and world-space top.
    solids: Vec<[f64; 5]>,
    /// rect, height, 1 when the deck is round (`ROUND`)
    decks: Vec<[f64; 6]>,
    /// rect, base, top, 1 when the ramp runs along z, direction of the climb along that axis
    ramps: Vec<[f64; 8]>,
    /// rect, depth
    sinks: Vec<[f64; 5]>,
    /// The box props as shots see them: rect, then the world-space heights of their underside and their top. A box is
    /// solid to shots (and blasts) but not to bodies: a hotdog walks under one (a table top, a den: titan.rs).
    boxes: Vec<[f64; 6]>,
    search: Search,
    /// For each navigation cell, the number of its region: the group of clear cells that routes can join it
    /// to (by the moves `path` makes). 0 for a blocked cell.
    regions: Vec<u32>,
    /// A Forts map's teams, bases, forcefield and bacon (see `Map::fortress`); None for any other map.
    pub fort: Option<Fort>,
}
/// Numbers the regions of a navigation grid: clear cells joined by moves to any of the eight cells round a
/// cell, never squeezing diagonally between two blocked corners.
fn regions(blocked: &[u8], cols: usize, rows: usize) -> Vec<u32> {
    let mut out = vec![0u32; cols * rows];
    let (mut open, mut count) = (vec![], 0);
    for first in 0..out.len() {
        if blocked[first] != 0 || out[first] != 0 {
            continue;
        }
        count += 1;
        out[first] = count;
        open.push(first);
        while let Some(id) = open.pop() {
            let (x, z) = ((id % cols) as i64, (id / cols) as i64);
            for dz in -1..=1 {
                let nz = z + dz;
                if nz < 0 || nz >= rows as i64 {
                    continue;
                }
                for dx in -1..=1 {
                    let nx = x + dx;
                    if (dx == 0 && dz == 0) || nx < 0 || nx >= cols as i64 {
                        continue;
                    }
                    let next = nz as usize * cols + nx as usize;
                    if blocked[next] != 0 || out[next] != 0 {
                        continue;
                    }
                    if dx != 0 && dz != 0 && (blocked[z as usize * cols + nx as usize] != 0 || blocked[nz as usize * cols + x as usize] != 0) {
                        continue;
                    }
                    out[next] = count;
                    open.push(next);
                }
            }
        }
    }
    out
}
impl Level {
    pub fn new(map: Map) -> Level {
        let [x0, z0, x1, z1] = map.bounds;
        let (cell, cell_u, clear_u, x0_u, z0_u) = (map.cell, quantum(map.cell), quantum(map.clearance), quantum(x0), quantum(z0));
        let cols = ((quantum(x1) - x0_u) / cell_u).ceil().max(1.) as usize;
        let rows = ((quantum(z1) - z0_u) / cell_u).ceil().max(1.) as usize;
        // A cell is blocked when its centre lies within a block grown by the clearance. Doubling keeps half-cell centres integral.
        let mut blocked = vec![0u8; cols * rows];
        let span = |low: f64, high: f64, origin: f64, count: usize| {
            let first = ((2. * low - 2. * origin - cell_u) / (2. * cell_u)).ceil().max(0.);
            let last = ((2. * high - 2. * origin - cell_u) / (2. * cell_u)).floor().min(count as f64 - 1.);
            (first, last)
        };
        let (mut walls, mut solids) = (Vec::with_capacity(map.blocks.len()), Vec::with_capacity(map.blocks.len()));
        for block in &map.blocks {
            let [bx0, bz0, bx1, bz1] = block.rect;
            let (i0, i1) = span(quantum(bx0) - clear_u, quantum(bx1) + clear_u, x0_u, cols);
            let (j0, j1) = span(quantum(bz0) - clear_u, quantum(bz1) + clear_u, z0_u, rows);
            if i0 <= i1 && j0 <= j1 {
                for j in j0 as usize..=j1 as usize {
                    blocked[j * cols + i0 as usize..=j * cols + i1 as usize].fill(1);
                }
            }
            walls.push([bx0 - MARGIN, bz0 - MARGIN, bx1 + MARGIN, bz1 + MARGIN, FLOOR + (block.height + LIP)]);
            solids.push([bx0, bz0, bx1, bz1, FLOOR + block.height]);
        }
        // A sink's rim is blocked: the cells within the clearance of its edge (or within half a cell, should the cells
        // be coarser), outside it and in, so that routes neither cross a sink nor wander into it, except over the top of
        // a ramp whose foot stands below the counter (a board propped in the sink), which is the way in and out. Its
        // floor inside the rim is clear, so a route can be found down there too. Doubled again: every test is integral.
        let reach = clear_u.max(cell_u / 2.);
        let gaps: Vec<[f64; 4]> = map
            .ramps
            .iter()
            .filter(|ramp| ramp.base < 0.)
            .map(|ramp| {
                let [rx0, rz0, rx1, rz1] = ramp.rect.map(quantum);
                let mut gap = [2. * rx0, 2. * rz0, 2. * rx1, 2. * rz1];
                match ramp.rise {
                    Side::PlusX => gap[2] += 2. * reach,
                    Side::MinusX => gap[0] -= 2. * reach,
                    Side::PlusZ => gap[3] += 2. * reach,
                    Side::MinusZ => gap[1] -= 2. * reach,
                }
                gap
            })
            .collect();
        for sink in &map.sinks {
            let [sx0, sz0, sx1, sz1] = sink.rect.map(quantum);
            let (i0, i1) = span(sx0 - reach, sx1 + reach, x0_u, cols);
            let (j0, j1) = span(sz0 - reach, sz1 + reach, z0_u, rows);
            if i0 > i1 || j0 > j1 {
                continue;
            }
            for j in j0 as usize..=j1 as usize {
                let cz = 2. * z0_u + (2. * j as f64 + 1.) * cell_u;
                for i in i0 as usize..=i1 as usize {
                    let cx = 2. * x0_u + (2. * i as f64 + 1.) * cell_u;
                    let within = cx > 2. * (sx0 + reach) && cx < 2. * (sx1 - reach) && cz > 2. * (sz0 + reach) && cz < 2. * (sz1 - reach);
                    if !within && !gaps.iter().any(|g| cx >= g[0] && cx <= g[2] && cz >= g[1] && cz <= g[3]) {
                        blocked[j * cols + i] = 1;
                    }
                }
            }
        }
        let sinks = map.sinks.iter().map(|sink| [sink.rect[0], sink.rect[1], sink.rect[2], sink.rect[3], sink.depth]).collect();
        let decks = map.decks.iter().map(|deck| [deck.rect[0], deck.rect[1], deck.rect[2], deck.rect[3], deck.height, if deck.style == ROUND { 1. } else { 0. }]).collect();
        let ramps = map.ramps.iter().map(|ramp| {
            let r = ramp.rect;
            [r[0], r[1], r[2], r[3], ramp.base, ramp.height, if ramp.rise.along_z() { 1. } else { 0. }, ramp.rise.sign()]
        }).collect();
        let regions = regions(&blocked, cols, rows);
        let fort = map.fortress();
        let boxes = map
            .props
            .iter()
            .filter_map(|prop| match prop {
                Prop::Box { rect, base, height, .. } => Some([rect[0], rect[1], rect[2], rect[3], FLOOR + base, FLOOR + (base + height)]),
                _ => None,
            })
            .collect();
        Level { map, x0, z0, x1, z1, cell, cols, rows, blocked, walls, solids, decks, ramps, sinks, boxes, search: Search::default(), regions, fort }
    }
    /// A Forts map's level while its forcefield is up: the field stands as one more block, the last, of the
    /// greatest height a block may have, so that bodies, routes, shots and loose objects are all stopped by
    /// it. Its `map` is the map itself, without that block. (Any other map has no field: this is `new`.)
    pub fn fielded(map: Map) -> Level {
        let Some(fort) = map.fortress() else {
            return Level::new(map);
        };
        let mut walled = map.clone();
        walled.blocks.push(Block { rect: fort.field, height: HEIGHT, style: "steel".into(), facing: None });
        let mut level = Level::new(walled);
        level.map = map;
        level.fort = Some(fort);
        level
    }
    pub fn cell_index(&self, x: f64, z: f64) -> usize {
        let i = ((x - self.x0) / self.cell).floor().max(0.).min(self.cols as f64 - 1.) as usize;
        let j = ((z - self.z0) / self.cell).floor().max(0.).min(self.rows as f64 - 1.) as usize;
        j * self.cols + i
    }
    pub fn cell_center(&self, id: usize) -> [f64; 2] {
        [self.x0 + ((id % self.cols) as f64 + 0.5) * self.cell, self.z0 + ((id / self.cols) as f64 + 0.5) * self.cell]
    }
    /// Navigation test used by zombies and routes: on the map and not in a blocked cell.
    pub fn clear(&self, x: f64, z: f64) -> bool {
        x >= self.x0 && x < self.x1 && z >= self.z0 && z < self.z1 && self.blocked[self.cell_index(x, z)] == 0
    }
    /// Height of the walkable surface above the counter; the highest deck or ramp under the point wins (a round deck
    /// only inside its ellipse). In a sink the counter is the sink's floor, below 0.
    pub fn height(&self, x: f64, z: f64) -> f64 {
        let mut h = 0.;
        for s in &self.sinks {
            if x >= s[0] && x <= s[2] && z >= s[1] && z <= s[3] {
                h = -s[4];
            }
        }
        for d in &self.decks {
            if d[4] > h && x >= d[0] && x <= d[2] && z >= d[1] && z <= d[3] && (d[5] == 0. || round([d[0], d[1], d[2], d[3]], x, z)) {
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
        h
    }
    /// World-space height of the surface under a point.
    pub fn ground(&self, x: f64, z: f64) -> f64 {
        FLOOR + self.height(x, z)
    }
    /// Whether a hotdog whose feet are at height y may occupy the point: on the map and beside or above every block.
    pub fn walkable(&self, x: f64, y: f64, z: f64) -> bool {
        if !(x >= self.x0 && x < self.x1 && z >= self.z0 && z < self.z1) {
            return false;
        }
        !self.walls.iter().any(|w| y < w[4] && x >= w[0] && x <= w[2] && z >= w[1] && z <= w[3])
    }
    /// One movement step from a to b, both [x, y, z]: ledges above STEP need a ramp or a jump.
    pub fn can_step(&self, a: [f64; 3], b: [f64; 3]) -> bool {
        let h = self.ground(b[0], b[2]);
        h <= a[1] + STEP && self.walkable(b[0], h.max(b[1]), b[2])
    }
    /// Distance a shot from o along the unit vector d travels before a block or a box prop (its whole volume, from its
    /// underside up: a table top stops a shot from under it and one from above) stops it, or the side or floor of a
    /// sink it has gone down into. (The counter top itself is not cover: whatever fires decides where a shot meets
    /// it, as `feast` does with `sunk`.)
    pub fn cover(&self, o: [f64; 3], d: [f64; 3]) -> f64 {
        let mut near = RANGE;
        for s in &self.solids {
            let t = ray_box(o, d, [s[0], FLOOR, s[1]], [s[2], s[4], s[3]]);
            if t < near {
                near = t;
            }
        }
        for b in &self.boxes {
            let t = ray_box(o, d, [b[0], b[4], b[1]], [b[2], b[5], b[3]]);
            if t < near {
                near = t;
            }
        }
        for s in &self.sinks {
            let t = basin(o, d, s);
            if t < near {
                near = t;
            }
        }
        near
    }
    /// Whether the map has a sink (when it has none, nothing is below the counter top).
    pub fn sunken(&self) -> bool {
        !self.sinks.is_empty()
    }
    /// Whether [x, z] lies over a sink, where the counter top is open.
    pub fn sunk(&self, x: f64, z: f64) -> bool {
        self.sinks.iter().any(|s| x >= s[0] && x <= s[2] && z >= s[1] && z <= s[3])
    }
    /// Whether the straight line between two [x, z] points stays clear.
    pub fn line(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / (self.cell * 0.35)).ceil().max(1.) as usize;
        (0..=n).all(|i| {
            let t = i as f64 / n as f64;
            self.clear(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
        })
    }
    /// Whether a route can join two [x, z] points: both on clear cells of the same region of the navigation grid.
    pub fn joined(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        self.clear(a[0], a[1]) && self.clear(b[0], b[1]) && self.regions[self.cell_index(a[0], a[1])] == self.regions[self.cell_index(b[0], b[1])]
    }
    /// Shortest clear route between two [x, z] points as waypoints ending at the goal; empty when there is none.
    pub fn path(&mut self, start: [f64; 2], goal: [f64; 2]) -> Vec<[f64; 2]> {
        if !self.clear(start[0], start[1]) || !self.clear(goal[0], goal[1]) {
            return vec![];
        }
        if self.line(start, goal) {
            return vec![goal];
        }
        // No route joins two regions (a hotdog walled off from the zombies, say): say so at once, rather than
        // search every cell the walker can reach, every step, for every zombie, and still come back empty.
        if !self.joined(start, goal) {
            return vec![];
        }
        let (cols, rows) = (self.cols as i32, self.rows as i32);
        let cells = self.cols * self.rows;
        let mut s = std::mem::take(&mut self.search);
        if s.cost.len() != cells {
            s = Search { cost: vec![0.; cells], parent: vec![0; cells], stamp: vec![0; cells], ..Search::default() };
        }
        s.generation += 1;
        if s.generation == u32::MAX {
            s.stamp.fill(0);
            s.generation = 1;
        }
        s.size = 0;
        let generation = s.generation;
        let (first, last) = (self.cell_index(start[0], start[1]) as i32, self.cell_index(goal[0], goal[1]) as i32);
        let (gi, gj) = (last % cols, last / cols);
        let diagonal = std::f64::consts::SQRT_2;
        s.cost[first as usize] = 0.;
        s.stamp[first as usize] = generation;
        s.parent[first as usize] = -1;
        s.push(0., first);
        let mut route = vec![];
        while s.size > 0 {
            let id = s.pop();
            if id == last {
                route.push(goal);
                let mut at = last;
                while at != first {
                    route.push(self.cell_center(at as usize));
                    at = s.parent[at as usize];
                }
                route.reverse();
                // String pulling: drop every waypoint that the previous kept point can already see past.
                let (mut previous, mut i) = (start, 0);
                while i < route.len() {
                    let mut j = i;
                    while j + 1 < route.len() && self.line(previous, route[j + 1]) {
                        j += 1;
                    }
                    route.drain(i..j);
                    previous = route[i];
                    i += 1;
                }
                break;
            }
            let (x, z, base) = (id % cols, id / cols, s.cost[id as usize]);
            for dz in -1..=1 {
                let nz = z + dz;
                if nz < 0 || nz >= rows {
                    continue;
                }
                for dx in -1..=1 {
                    let nx = x + dx;
                    if (dx == 0 && dz == 0) || nx < 0 || nx >= cols {
                        continue;
                    }
                    let next = (nz * cols + nx) as usize;
                    if self.blocked[next] != 0 {
                        continue;
                    }
                    // A diagonal move may not squeeze between two blocked corners.
                    if dx != 0 && dz != 0 && (self.blocked[(z * cols + nx) as usize] != 0 || self.blocked[(nz * cols + x) as usize] != 0) {
                        continue;
                    }
                    let c = base + if dx != 0 && dz != 0 { diagonal } else { 1. };
                    if s.stamp[next] == generation && c >= s.cost[next] {
                        continue;
                    }
                    s.cost[next] = c;
                    s.stamp[next] = generation;
                    s.parent[next] = id;
                    let (ax, az) = ((nx - gi).abs() as f64, (nz - gj).abs() as f64);
                    s.push(c + ax.max(az) + (diagonal - 1.) * ax.min(az), next as i32);
                }
            }
        }
        self.search = s;
        route
    }
    /// Where a walker at `old` should head to reach a `target` standing higher up: the foot of the cheapest ramp,
    /// then a point just past its top. None when the target is on the same level or no ramp leads up from here.
    pub fn climb_goal(&self, old: [f64; 3], target: [f64; 3]) -> Option<[f64; 3]> {
        let (own, wanted, x, z) = (old[1] - FLOOR, target[1] - FLOOR, old[0], old[2]);
        let (mut best, mut best_cost) = (None, f64::INFINITY);
        for r in &self.ramps {
            let (base, top, along_z, up) = (r[4], r[5], r[6] == 1., r[7]);
            let middle = if along_z { (r[0] + r[2]) / 2. } else { (r[1] + r[3]) / 2. };
            let low = if along_z { if up > 0. { r[1] } else { r[3] } } else if up > 0. { r[0] } else { r[2] };
            let high = if along_z { if up > 0. { r[3] } else { r[1] } } else if up > 0. { r[2] } else { r[0] };
            let head = if along_z { [middle, high + up * HEAD] } else { [high + up * HEAD, middle] };
            // Already on a ramp that leads towards the target's level: keep climbing it rather than cutting off its side.
            if own > base + 0.01 && top > own + 0.02 && wanted >= top - 0.05 && x >= r[0] - 0.02 && x <= r[2] + 0.02 && z >= r[1] - 0.02 && z <= r[3] + 0.02 {
                return Some([head[0], self.ground(head[0], head[1]), head[1]]);
            }
            if wanted <= own + CLIMB || base > own + 0.05 || top <= own + 0.05 {
                continue;
            }
            let foot = if along_z { [middle, low + up * FOOT] } else { [low + up * FOOT, middle] };
            let reach = (foot[0] - x).hypot(foot[1] - z);
            let after = (target[0] - head[0]).hypot(target[2] - head[1]);
            // A board down into a sink is narrow and its foot is open on every side: a walker beside it, not yet in
            // front of its foot, keeps making for the foot rather than for the top up the board's side.
            let beside = base < 0. && if along_z { (x - middle).abs() > (r[2] - r[0]) / 2. - 0.01 } else { (z - middle).abs() > (r[3] - r[1]) / 2. - 0.01 };
            let at_foot = reach < 0.1 && !beside;
            let goal = if at_foot { head } else { foot };
            let cost = (if at_foot { (head[0] - x).hypot(head[1] - z) } else { reach + (high - low).abs() }) + after;
            if cost < best_cost {
                best_cost = cost;
                best = Some(goal);
            }
        }
        best.map(|b| [b[0], self.ground(b[0], b[1]), b[1]])
    }
    /// Players share the starts in order, wrapping when a map has fewer starts than players.
    pub fn start_position(&self, player: usize) -> [f64; 3] {
        let [x, z] = self.map.starts[player % self.map.starts.len()].p;
        [x, self.ground(x, z), z]
    }
    pub fn start_yaw(&self, player: usize) -> f64 {
        self.map.starts[player % self.map.starts.len()].yaw
    }
    /// Zombies walk the counter itself, so they always appear at floor height (a sink's floor, in a sink).
    pub fn spawn_position(&self, zombie: usize) -> [f64; 3] {
        let [x, z] = self.map.spawns[zombie % self.map.spawns.len()].p;
        [x, FLOOR + self.height(x, z).min(0.), z]
    }
    pub fn pickup_position(&self, index: usize) -> [f64; 3] {
        let [x, z] = self.map.pickups[index].p;
        [x, self.ground(x, z) + 0.04, z]
    }
    /// Forts: where the `n`th hotdog of a team starts and the way it faces; a team shares its starts in
    /// order. (On any other map: the map's starts, as `start_position`.)
    pub fn team_start(&self, team: usize, n: usize) -> ([f64; 3], f64) {
        let Some(own) = self.fort.as_ref().map(|fort| &fort.starts[team]).filter(|own| !own.is_empty()) else {
            return (self.start_position(n), self.start_yaw(n));
        };
        let start = &own[n % own.len()];
        ([start.p[0], self.ground(start.p[0], start.p[1]), start.p[1]], start.yaw)
    }
    /// Forts: where the bacon lies, on the surface at its spot. (On any other map: the middle of the map.)
    pub fn bacon_position(&self) -> [f64; 3] {
        let [x, z] = self.fort.as_ref().map_or([(self.x0 + self.x1) / 2., (self.z0 + self.z1) / 2.], |fort| fort.bacon);
        [x, self.ground(x, z), z]
    }
    /// Forts: whether [x, z] is inside a team's base.
    pub fn in_base(&self, team: usize, x: f64, z: f64) -> bool {
        self.fort.as_ref().is_some_and(|fort| {
            let r = fort.bases[team];
            x >= r[0] && x <= r[2] && z >= r[1] && z <= r[3]
        })
    }
    /// Forts: the side of the forcefield a point is on, -1 or 1 (a team's is `fort.sides[team]`); 0 on a map
    /// without one.
    pub fn side(&self, x: f64, z: f64) -> i32 {
        self.fort.as_ref().map_or(0, |fort| if (if fort.axis == 1 { z } else { x }) < fort.mid { -1 } else { 1 })
    }
}

/// Single-precision conveniences for the game, which keeps positions in f32. They only convert: the rules
/// above stay in f64.
impl Level {
    pub fn ground_at(&self, x: f32, z: f32) -> f32 {
        self.ground(x as f64, z as f64) as f32
    }
    pub fn can_step_to(&self, from: Vec3, to: Vec3) -> bool {
        self.can_step(from.as_dvec3().to_array(), to.as_dvec3().to_array())
    }
    pub fn cover_from(&self, origin: Vec3, direction: Vec3) -> f32 {
        self.cover(origin.as_dvec3().to_array(), direction.as_dvec3().to_array()) as f32
    }
    pub fn route(&mut self, from: Vec2, to: Vec2) -> Vec<Vec2> {
        let path = self.path(from.as_dvec2().to_array(), to.as_dvec2().to_array());
        path.into_iter().map(|p| Vec2::new(p[0] as f32, p[1] as f32)).collect()
    }
    pub fn climb(&self, old: Vec3, target: Vec3) -> Option<Vec3> {
        let goal = self.climb_goal(old.as_dvec3().to_array(), target.as_dvec3().to_array())?;
        Some(Vec3::new(goal[0] as f32, goal[1] as f32, goal[2] as f32))
    }
    /// Where a player starts, and the way it faces.
    pub fn start(&self, player: usize) -> (Vec3, f32) {
        let p = self.start_position(player);
        (Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), self.start_yaw(player) as f32)
    }
    pub fn spawn(&self, zombie: usize) -> Vec3 {
        let p = self.spawn_position(zombie);
        Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
    }
    pub fn pickup(&self, index: usize) -> Vec3 {
        let p = self.pickup_position(index);
        Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
    }
}

// FNV-1a over little-endian 32-bit words; the JavaScript tests compute the same fingerprints.
fn fnv(words: impl Iterator<Item = u32>) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for word in words {
        for byte in word.to_le_bytes() {
            h = (h ^ byte as u32).wrapping_mul(0x0100_0193);
        }
    }
    format!("{h:08x}")
}
/// Fingerprint of the navigation grid's size and blocked cells.
pub fn grid_hash(level: &Level) -> String {
    fnv([level.cols as u32, level.rows as u32].into_iter().chain(level.blocked.iter().map(|&b| b as u32)))
}
/// Fingerprint of the surface height at every cell centre, in tenths of a millimetre.
pub fn ground_hash(level: &Level) -> String {
    fnv((0..level.cols * level.rows).map(|id| {
        let [x, z] = level.cell_center(id);
        quantum(level.height(x, z)) as i64 as u32
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    fn read(path: &str) -> String {
        std::fs::read_to_string(format!("{ROOT}/{path}")).unwrap_or_else(|error| panic!("{path}: {error}"))
    }
    fn edited(change: impl FnOnce(&mut Value)) -> Checked<Map> {
        let mut value: Value = serde_json::from_str(BUNDLED).unwrap();
        change(&mut value);
        Map::from_value(&value)
    }

    #[test]
    fn the_largest_kitchen_is_the_one_the_editor_checks_maps_against() {
        // tools/network/map.mjs warns about a counter beyond its ROOM ("beyond-room"): the same numbers.
        let script = read("tools/network/map.mjs");
        let line = script.lines().find(|line| line.starts_with("export const ROOM=[")).expect("map.mjs exports ROOM");
        let numbers: Vec<f64> = line["export const ROOM=[".len()..].trim_end_matches("];").split(',').map(|n| n.parse().unwrap()).collect();
        assert_eq!(numbers, ROOM);
        for path in ["assets/maps/counter-siege.json", "assets/maps/counter-siege-v2.json", "assets/maps/pillow-fort.json", "tools/network/fixtures/long-fort.json", "assets/maps/the-whole-kitchen.json", "assets/maps/the-whole-kitchen-forts.json", "assets/maps/kung-fu-dojo.json"] {
            let [x0, _, x1, z1] = Map::parse(&read(path)).unwrap().bounds;
            assert!(x0 >= ROOM[0] && x1 <= ROOM[1] && z1 <= ROOM[2], "{path} fits in the kitchen");
        }
    }

    #[test]
    fn the_canonical_text_is_what_the_editor_writes_byte_for_byte() {
        // Every bundled map is kept in canonical form (tools/network/map.test.mjs holds them to it): read and written
        // again, each comes back exactly as it is in the file.
        let folder = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/maps");
        let mut names: Vec<_> = std::fs::read_dir(folder).unwrap().map(|entry| entry.unwrap().path()).filter(|path| path.extension().is_some_and(|e| e == "json")).collect();
        names.sort();
        assert!(names.len() >= 10);
        for path in names {
            let text = std::fs::read_to_string(&path).unwrap();
            assert_eq!(Map::parse(&text).unwrap().to_text(), text, "{}", path.display());
        }
        // Maps that are not: map.mjs's `stringify` of each, as a hash of its UTF-8 bytes in the golden file.
        let golden: Value = serde_json::from_str(&read("tools/network/fixtures/map-golden.json")).unwrap();
        let hash = |text: &str| {
            let mut h: u32 = 0x811c_9dc5;
            for byte in text.bytes() {
                h = (h ^ byte as u32).wrapping_mul(0x0100_0193);
            }
            format!("{h:08x}")
        };
        for (name, path) in [
            ("awkward.json", "tools/network/fixtures/awkward.json"),
            ("uncanonical.json", "tools/network/fixtures/uncanonical.json"),
            ("pillow-fort.json", "assets/maps/pillow-fort.json"),
            ("the-whole-kitchen-forts.json", "assets/maps/the-whole-kitchen-forts.json"),
            ("kung-fu-dojo.json", "assets/maps/kung-fu-dojo.json"),
            ("titans-kitchen.json", "assets/maps/titans-kitchen.json"),
        ] {
            let text = Map::parse(&read(path)).unwrap().to_text();
            assert_eq!(hash(&text), golden["canonical"][name].as_str().unwrap(), "{name}:\n{text}");
            // The canonical text reads back as the same map, and is its own canonical text.
            assert_eq!(Map::parse(&text).unwrap().to_text(), text);
        }
        assert_eq!([0.1, 150., -0.0001, 1e-7, 2.5e21, -0.].map(js_number), ["0.1", "150", "-0.0001", "1e-7", "2.5e+21", "0"]);
    }

    #[test]
    fn grids_match_the_golden_file_shared_with_the_javascript_tests() {
        let golden: Value = serde_json::from_str(&read("tools/network/fixtures/map-golden.json")).unwrap();
        let (fort, kitchen) = ("assets/maps/pillow-fort.json", "assets/maps/the-whole-kitchen-forts.json");
        for (name, path, fielded) in [
            ("counter-siege.json", "assets/maps/counter-siege.json", false),
            ("awkward.json", "tools/network/fixtures/awkward.json", false),
            ("pillow-fort.json", fort, false),
            ("pillow-fort.json fielded", fort, true),
            // A sink, its rim and the ramps out of it.
            ("the-whole-kitchen-forts.json", kitchen, false),
            ("the-whole-kitchen-forts.json fielded", kitchen, true),
            // Round decks: the ring and the stand of steamers.
            ("kung-fu-dojo.json", "assets/maps/kung-fu-dojo.json", false),
            // The kitchen floor: legs, the toe-kick, the box and the stool and their ramps.
            ("titans-kitchen.json", "assets/maps/titans-kitchen.json", false),
        ] {
            let map = Map::parse(&read(path)).unwrap();
            // A Forts map while its forcefield is up: the field is one more block.
            let level = if fielded { Level::fielded(map) } else { Level::new(map) };
            let expected = &golden[name];
            assert_eq!(level.cols as u64, expected["cols"].as_u64().unwrap(), "{name} cols");
            assert_eq!(level.rows as u64, expected["rows"].as_u64().unwrap(), "{name} rows");
            assert_eq!(grid_hash(&level), expected["grid"].as_str().unwrap(), "{name} grid");
            assert_eq!(ground_hash(&level), expected["ground"].as_str().unwrap(), "{name} ground");
        }
    }

    /// A box prop stops shots in its whole volume, from under it and from above, and not below its underside (the game's
    /// and the JavaScript's alike: tools/network/map.test.mjs has the same numbers).
    #[test]
    fn a_box_prop_stops_shots_from_under_it_and_from_above_but_not_below_its_underside() {
        let map = Map::parse(r#"{"format": "hotdog-map", "version": 1, "name": "Table", "bounds": [-2, 2, 2, 4], "starts": [{"p": [-1.5, 3]}], "spawns": [{"p": [1.5, 3]}], "props": [{"kind": "box", "rect": [-0.5, 2.4, 0.5, 2.8], "base": 0.3, "height": 0.05}]}"#).unwrap();
        let level = Level::new(map);
        assert!((level.cover([0., FLOOR + 0.1, 2.6], [0., 1., 0.]) - 0.2).abs() < 1e-12, "up from under it");
        assert!((level.cover([0., FLOOR + 1., 2.6], [0., -1., 0.]) - 0.65).abs() < 1e-12, "down from above");
        assert!((level.cover([-1., FLOOR + 0.32, 2.6], [1., 0., 0.]) - 0.5).abs() < 1e-12, "into its side");
        assert_eq!(level.cover([-1., FLOOR + 0.1, 2.6], [1., 0., 0.]), RANGE, "under it");
        assert!(level.walkable(0., FLOOR, 2.6), "a body walks under it");
    }

    #[test]
    fn the_bundled_map_reproduces_the_rules_the_game_used_to_hard_code() {
        let mut level = Level::new(Map::bundled());
        // (Eight guns, and the two boxes of ammunition every siege map has now that rounds run out in every gun mode.)
        assert_eq!((level.map.starts.len(), level.map.spawns.len(), level.map.pickups.len(), level.map.blocks.len()), (5, 3, 10, 4));
        assert_eq!(level.ground(1.5, 3.4), FLOOR + 0.28);
        assert!((level.ground(0.9, 3.5) - (FLOOR + 0.14)).abs() < 1e-9);
        assert_eq!(level.ground(0., 3.), FLOOR);
        assert!(!level.clear(-0.3, 2.8) && level.clear(0., 3.) && !level.clear(1.9, 3.));
        assert!(!level.walkable(-0.3, FLOOR, 2.8) && level.walkable(-0.3, FLOOR + 0.26, 2.8));
        assert!(!level.can_step([1.2, FLOOR, 3.3], [1.3, FLOOR, 3.3]), "the deck is too tall to step onto");
        assert!(level.can_step([0.6, FLOOR, 3.5], [0.62, FLOOR, 3.5]), "the ramp is not");
        assert!((level.cover([-1., FLOOR + 0.1, 2.81], [1., 0., 0.]) - 0.55).abs() < 1e-12);
        assert_eq!(level.cover([-1., FLOOR + 0.3, 2.81], [1., 0., 0.]), RANGE);
        let route = level.path([-1.23, 3.02], [0.83, 3.23]);
        assert_eq!(route.last(), Some(&[0.83, 3.23]));
        assert!(level.path([-1.23, 3.02], [-0.3, 2.8]).is_empty(), "no route into a block");
        assert_eq!(level.start_position(7), [-0.05, FLOOR, 2.52]);
    }

    #[test]
    fn a_bamboo_deck_is_round_its_floor_the_ellipse_inside_its_rect_and_a_map_with_bamboo_is_version_2() {
        assert_eq!((STYLES[9], FIRST_STYLES, ROUND, KITS[13]), ("bamboo", 9, "bamboo", "gong"));
        // The same decks as map.test.mjs: a circle 0.8 m across and an ellipse 1 m by 0.5 m.
        let map = edited(|map| {
            map["decks"] = serde_json::json!([{"rect": [-1, 2.4, -0.2, 3.2], "height": 0.03, "style": "bamboo"}, {"rect": [0.2, 2.5, 1.2, 3], "height": 0.04, "style": "bamboo"}]);
            map["ramps"] = serde_json::json!([]);
        })
        .unwrap();
        assert_eq!(map.version(), 2.);
        assert_eq!(edited(|map| map["blocks"][2]["style"] = serde_json::json!("bamboo")).unwrap().version(), 2.);
        assert_eq!(Map::bundled().version(), 1.);
        let level = Level::new(map);
        for (x, z, h) in [(-0.6, 2.8, 0.03), (-0.6 + 0.399, 2.8, 0.03), (-0.6 + 0.28, 2.8 + 0.28, 0.03), (-0.6 + 0.29, 2.8 + 0.29, 0.), (-0.99, 2.41, 0.), (1.19, 2.75, 0.04), (0.7, 2.99, 0.04), (1.15, 2.6, 0.)] {
            assert_eq!(level.height(x, z), h, "({x}, {z})");
        }
        assert!(round([0., 0., 2., 2.], 1., 0.) && round([0., 0., 2., 2.], 0., 1.) && !round([0., 0., 2., 2.], 0.29, 0.29));
        assert!(level.can_step([-0.6, FLOOR, 2.36], [-0.6, FLOOR, 2.41]), "a hotdog steps up onto it");
        // Where the deck is not, a hotdog on the counter walks under nothing: the corner of the square is the counter.
        assert!(level.can_step([-0.95, FLOOR, 2.45], [-0.97, FLOOR, 2.43]));
    }

    #[test]
    fn the_dojo_is_a_round_ring_by_the_front_with_eight_starts_on_it_facing_the_middle() {
        let map = Map::parse(&read("assets/maps/kung-fu-dojo.json")).unwrap();
        assert_eq!((map.name.as_str(), map.mode.as_str(), map.version(), map.pickups.len(), map.starts.len()), ("Kung Fu Dojo", "kungfu", 2., 0, 8));
        assert!(!map.forts() && map.fortress().is_none());
        let ring = &map.decks[0];
        let [x0, z0, x1, z1] = ring.rect;
        let (c, r) = ([(x0 + x1) / 2., (z0 + z1) / 2.], (x1 - x0) / 2.);
        assert_eq!((ring.style.as_str(), x1 - x0), ("bamboo", z1 - z0));
        assert!(z0 - map.bounds[1] <= 0.12, "a throw off the front of the ring goes off the counter");
        let level = Level::new(map.clone());
        for start in &map.starts {
            let (dx, dz) = (c[0] - start.p[0], c[1] - start.p[1]);
            assert!(dx.hypot(dz) < r - 0.1);
            // Yaw 0 looks along -z.
            assert!((-start.yaw.sin() * dx - start.yaw.cos() * dz) / dx.hypot(dz) > 0.999, "{:?} faces the middle", start.p);
            assert_eq!(level.ground(start.p[0], start.p[1]), FLOOR + ring.height);
            assert!(level.walkable(start.p[0], FLOOR + ring.height, start.p[1]));
        }
        // Off the ring's edge is the counter, and the ring is a step up from it.
        assert_eq!(level.height(c[0], z0 + 0.001), ring.height);
        assert_eq!(level.height(x0 + 0.02, z0 + 0.02), 0.);
        assert!(ring.height <= STEP);
    }

    #[test]
    fn defaults_are_filled_in_and_unknown_keys_dropped() {
        let map = edited(|map| {
            map["blocks"] = serde_json::json!([{"rect": [0, 3, 0.2, 3.2], "colour": "pink"}, {"rect": [0.5, 3, 0.7, 3.2], "style": "toaster"}]);
            map["nav"] = Value::Null;
            map["name"] = serde_json::json!("  Back counter  ");
        }).unwrap();
        assert_eq!(map.blocks[0], Block { rect: [0., 3., 0.2, 3.2], height: 0.25, style: "steel".into(), facing: None });
        assert_eq!(map.blocks[1].facing, Some(Side::MinusZ));
        assert_eq!((map.cell, map.clearance, map.name.as_str(), map.theme.as_str()), (0.04, 0.035, "Back counter", "kitchen"));
    }

    #[test]
    fn loose_objects_are_read_in_file_order_beside_the_fixed_props() {
        let map = edited(|map| {
            map["props"] = serde_json::json!([{"kind": "bowl", "p": [0, 3]}, {"kind": "turret", "p": [1.47, 3.22], "base": 0.2975}, {"kind": "chair", "p": [0.5, 3.1], "base": 0.1, "yaw": 1.5}]);
        }).unwrap();
        assert_eq!(map.props.len(), 1);
        assert_eq!(map.loose, vec![
            Loose { kind: "bowl".into(), p: [0., 3.], base: 0., yaw: 0. },
            Loose { kind: "chair".into(), p: [0.5, 3.1], base: 0.1, yaw: 1.5 },
        ]);
        assert!(edited(|map| map["props"] = serde_json::json!([{"kind": "sofa", "p": [0, 3]}])).unwrap_err().0.contains("must be one of"));
    }

    #[test]
    fn broken_maps_are_refused_with_a_reason() {
        let reason = |result: Checked<Map>| result.unwrap_err().0;
        assert!(Map::parse("{").unwrap_err().0.starts_with("Not valid JSON"));
        assert!(reason(edited(|map| map["format"] = serde_json::json!("other"))).contains("format"));
        assert!(reason(edited(|map| map["version"] = serde_json::json!(3))).contains("version"));
        assert!(reason(edited(|map| map["version"] = serde_json::json!(1.5))).contains("version"));
        assert!(reason(edited(|map| map["theme"] = serde_json::json!("beach"))).contains("theme must be one of"));
        assert!(reason(edited(|map| map["spawns"] = serde_json::json!([]))).contains("at least one spawn"));
        assert!(reason(edited(|map| map["ramps"][0]["base"] = serde_json::json!(0.5))).contains("must rise"));
        assert!(reason(edited(|map| map["blocks"][0]["rect"] = serde_json::json!([1, 3, 0.5, 3.2]))).contains("inside out"));
        assert!(reason(edited(|map| map["pickups"][0]["weapon"] = serde_json::json!(9))).contains("whole number"));
        assert!(reason(edited(|map| map["starts"][0]["p"] = serde_json::json!([99, 0]))).contains("between -50 and 50"));
        assert!(reason(edited(|map| map["decks"][0]["style"] = serde_json::json!("toaster"))).contains("one of"));
        assert!(reason(edited(|map| map["bounds"] = serde_json::json!([0, 0, 0.1, 0.1]))).contains("0.2 m"));
        assert!(reason(edited(|map| { map["bounds"] = serde_json::json!([-50, -50, 50, 50]); map["nav"] = serde_json::json!({"cell": 0.01}) })).contains("navigation grid"));
    }

    // ----- Forts. The answers pinned here are tools/network/map.mjs's (fortress, fielded, normalize).
    fn pillow() -> Map {
        Map::parse(&read("tools/network/fixtures/long-fort.json")).unwrap()
    }
    fn at(p: [f64; 2], yaw: f64, team: usize) -> Start {
        Start { p, yaw, team: Some(team) }
    }
    /// A Forts map from JSON written out in full.
    fn fort(text: &str) -> Map {
        Map::parse(text).unwrap()
    }

    #[test]
    fn a_siege_map_has_no_mode_of_its_own_no_zones_and_no_fort() {
        let map = Map::bundled();
        assert_eq!((map.mode.as_str(), map.forts(), map.build, map.zones.len()), ("siege", false, BUILD, 0));
        assert!(map.starts.iter().all(|start| start.team.is_none()));
        assert_eq!(map.fortress(), None);
        let level = Level::new(map.clone());
        assert_eq!((level.fort.is_none(), level.side(0., 3.), level.in_base(0, 0., 3.)), (true, 0, false));
        // Without a field there is nothing to put up.
        assert_eq!(grid_hash(&Level::fielded(map.clone())), grid_hash(&level));
        // A siege map's `build` is not looked at, and a team on a start or a zone on the map is kept but changes nothing.
        let odd = edited(|map| {
            map["build"] = serde_json::json!("soon");
            map["starts"][1]["team"] = serde_json::json!("ketchup");
            map["zones"] = serde_json::json!([{"kind": "bacon", "p": [0, 3]}]);
        }).unwrap();
        assert_eq!((odd.build, odd.starts[0].team, odd.starts[1].team, odd.zones.clone(), odd.fortress()), (BUILD, None, Some(1), vec![Zone::Bacon { p: [0., 3.] }], None));
        // A missing mode is a siege, as is a missing or null list of zones.
        let old = edited(|map| {
            map.as_object_mut().unwrap().remove("mode");
            map["zones"] = Value::Null;
        }).unwrap();
        assert_eq!(old, Map::bundled());
    }

    #[test]
    fn a_pickup_holds_a_gun_with_its_rounds_if_it_says_or_a_box_of_ammunition_without_a_count_and_a_map_may_say_how_long_its_pickups_take_to_come_back() {
        let reason = |result: Checked<Map>| result.unwrap_err().0;
        let map = edited(|map| map["pickups"] = serde_json::json!([{"p": [0, 3], "weapon": 0, "ammo": 50}, {"p": [0.5, 3], "weapon": 6, "ammo": 30}])).unwrap();
        assert_eq!(map.pickups, vec![Pickup { p: [0., 3.], weapon: AMMO_BOX, ammo: 0 }, Pickup { p: [0.5, 3.], weapon: 6, ammo: 30 }]);
        assert_eq!((map.respawn, RESPAWN), ([20., 12.], [20., 12.]));
        // A gun's rounds are the game's rules' (magazine.rs PICKUP): a map may leave them out, and is written back as it
        // was; what it does say must still be a count.
        let bare = edited(|map| map["pickups"] = serde_json::json!([{"p": [0, 3], "weapon": 2}])).unwrap();
        assert_eq!(bare.pickups, vec![Pickup { p: [0., 3.], weapon: 2, ammo: 0 }]);
        assert!(bare.to_text().contains(r#"{"p": [0, 3], "weapon": 2}"#), "{}", bare.to_text());
        assert_eq!(Map::parse(&bare.to_text()).unwrap(), bare);
        assert!(map.to_text().contains(r#""weapon": 6, "ammo": 30"#));
        assert!(reason(edited(|map| map["pickups"] = serde_json::json!([{"p": [0, 3], "weapon": 2, "ammo": 0}]))).contains("ammo"), "a count, if any, from 1");
        assert_eq!(edited(|map| map["respawn"] = serde_json::json!([30, 8.5])).unwrap().respawn, [30., 8.5]);
        for bad in [serde_json::json!([0, 10]), serde_json::json!([10, 601]), serde_json::json!([10]), serde_json::json!("20")] {
            assert!(reason(edited(|map| map["respawn"] = bad.clone())).contains("respawn"), "{bad}");
        }
    }

    #[test]
    fn the_bundled_forts_map_names_its_mode_teams_bases_field_and_bacon() {
        let map = pillow();
        assert_eq!((map.mode.as_str(), map.forts(), map.build, map.starts.len(), map.zones.len(), map.loose.len(), map.pickups.len()), ("forts", true, 150., 6, 4, 30, 14));
        // A side's pickups: a ketchup sniper, a hot sauce thrower, a relish bazooka, two lots of mustard, a syrup rifle and a box of ammunition.
        for side in [-1., 1.] {
            let mut own: Vec<usize> = map.pickups.iter().filter(|item| item.p[0].signum() == side).map(|item| item.weapon).collect();
            own.sort();
            assert_eq!(own, [AMMO_BOX, 1, 1, 2, 3, 4, 6]);
        }
        assert!(map.pickups.iter().all(|item| (item.weapon == AMMO_BOX) == (item.ammo == 0)) && map.respawn == RESPAWN);
        assert_eq!(map.starts.iter().map(|start| start.team).collect::<Vec<_>>(), [Some(0), Some(0), Some(0), Some(1), Some(1), Some(1)]);
        assert_eq!(map.zones, vec![
            Zone::Base { team: 0, rect: [-3., 2.7, -2.5, 3.5] },
            Zone::Base { team: 1, rect: [2.5, 2.7, 3., 3.5] },
            Zone::Field { rect: [-0.04, 2.2, 0.04, 4.] },
            Zone::Bacon { p: [0., 3.1] },
        ]);
        let expected = Fort {
            axis: 0,
            mid: 0.,
            field: [-0.04, 2.2, 0.04, 4.],
            bases: [[-3., 2.7, -2.5, 3.5], [2.5, 2.7, 3., 3.5]],
            sides: [-1, 1],
            bacon: [0., 3.1],
            starts: [
                vec![at([-2.75, 2.9], -1.5708, 0), at([-2.75, 3.1], -1.5708, 0), at([-2.75, 3.3], -1.5708, 0)],
                vec![at([2.75, 2.9], 1.5708, 1), at([2.75, 3.1], 1.5708, 1), at([2.75, 3.3], 1.5708, 1)],
            ],
            build: 150.,
        };
        assert_eq!(map.fortress(), Some(expected.clone()));
        let level = Level::new(map);
        assert_eq!(level.fort, Some(expected));
        // A team shares its starts in order; the bacon lies on the deck at its spot.
        assert_eq!(level.team_start(1, 4), ([2.75, FLOOR, 3.1], 1.5708));
        assert_eq!(level.team_start(0, 2), ([-2.75, FLOOR, 3.3], -1.5708));
        assert_eq!(level.bacon_position(), [0., FLOOR + 0.12, 3.1]);
        assert_eq!((level.in_base(0, -2.7, 3.), level.in_base(1, -2.7, 3.), level.in_base(1, 2.5, 3.5), level.in_base(1, 2.49, 3.)), (true, false, true, false));
        assert_eq!((level.side(-0.01, 3.), level.side(0., 3.), level.side(2., 2.3)), (-1, 1, 1));
    }

    #[test]
    fn a_forts_map_that_leaves_things_out_is_filled_in_as_the_server_fills_it_in() {
        // No zones and no teams: a strip across the middle of the longer side, the end eighths as bases, the bacon
        // in the middle of the field, and each start with the team whose side it is on.
        let bare = fort(r#"{"format":"hotdog-map","version":1,"mode":"forts","bounds":[0,0,4,2],"starts":[{"p":[0.5,0.5],"yaw":1},{"p":[3.5,1.5]},{"p":[3,0.4],"yaw":-2}],"spawns":[{"p":[2,1]}]}"#);
        assert_eq!(bare.fortress(), Some(Fort {
            axis: 0,
            mid: 2.,
            field: [1.97, 0., 2.03, 2.],
            bases: [[0., 0., 0.5, 2.], [3.5, 0., 4., 2.]],
            sides: [-1, 1],
            bacon: [2., 1.],
            starts: [vec![at([0.5, 0.5], 1., 0)], vec![at([3.5, 1.5], 0., 1), at([3., 0.4], -2., 1)]],
            build: 150.,
        }));
        // A map longer in z, one base only: the other team takes the side that is left, and with no start of its own
        // begins in the middle of its base, facing the field. The map sets its own build time.
        let tall = fort(r#"{"format":"hotdog-map","version":1,"mode":"forts","build":40,"bounds":[0,0,1,3],"starts":[{"p":[0.5,0.5],"team":"ketchup"}],"spawns":[{"p":[0.5,1]}],"zones":[{"kind":"base","team":"ketchup","rect":[0.2,0.1,0.8,0.6]},{"kind":"bacon","p":[0.3,1.4]}]}"#);
        assert_eq!(tall.fortress(), Some(Fort {
            axis: 1,
            mid: 1.5,
            field: [0., 1.47, 1., 1.53],
            bases: [[0., 2.625, 1., 3.], [0.2, 0.1, 0.8, 0.6]],
            sides: [1, -1],
            bacon: [0.3, 1.4],
            starts: [vec![at([0.5, 2.8125], 0., 0)], vec![at([0.5, 0.5], 0., 1)]],
            build: 40.,
        }));
        // A field that runs along x divides the map along z, whatever the map's shape.
        let wide = fort(r#"{"format":"hotdog-map","version":1,"mode":"forts","bounds":[0,0,4,2],"starts":[{"p":[0.5,0.5]}],"spawns":[{"p":[2,1]}],"zones":[{"kind":"field","rect":[0,0.9,4,1.1]},{"kind":"base","team":"mustard","rect":[1,1.5,2,1.9]}]}"#);
        assert_eq!(wide.fortress(), Some(Fort {
            axis: 1,
            mid: 1.,
            field: [0., 0.9, 4., 1.1],
            bases: [[1., 1.5, 2., 1.9], [0., 0., 4., 0.25]],
            sides: [1, -1],
            bacon: [2., 1.],
            starts: [vec![at([1.5, 1.7], 0., 0)], vec![at([0.5, 0.5], 0., 1)]],
            build: 150.,
        }));
        let level = Level::new(tall);
        assert_eq!(level.team_start(0, 5), ([0.5, FLOOR, 2.8125], 0.));
        assert_eq!((level.side(0.5, 1.49), level.side(0.5, 1.5)), (-1, 1));
    }

    #[test]
    fn the_forcefield_stands_as_a_block_that_stops_steps_routes_and_shots_until_it_is_down() {
        let (mut up, mut down) = (Level::fielded(pillow()), Level::new(pillow()));
        // The level's map is the map itself: the field is not one of its blocks.
        assert_eq!((up.map.blocks.len(), down.map.blocks.len()), (5, 5));
        assert_eq!(up.map, down.map);
        assert_eq!(up.fort, down.fort);
        let (a, b) = ([-0.08, FLOOR, 2.5], [-0.06, FLOOR, 2.5]);
        assert!(!up.can_step(a, b) && down.can_step(a, b));
        assert!(!up.walkable(0., FLOOR + 2.9, 2.5) && down.walkable(0., FLOOR, 2.5), "no jump clears it");
        assert!(up.path([-1., 2.5], [1., 2.5]).is_empty());
        assert_eq!(down.path([-1., 2.5], [1., 2.5]), vec![[1., 2.5]]);
        assert!(!up.joined([-1., 2.5], [1., 2.5]) && down.joined([-1., 2.5], [1., 2.5]));
        assert!(!up.clear(0., 3.) && down.clear(0., 3.));
        assert!((up.cover([-1., FLOOR + 0.1, 2.5], [1., 0., 0.]) - 0.96).abs() < 1e-12);
        assert!((down.cover([-1., FLOOR + 0.1, 2.5], [1., 0., 0.]) - 2.7).abs() < 1e-12);
        // Each side is still whole: a hotdog reaches its own weapons.
        assert_eq!(up.path([-2.75, 3.1], [-0.8, 2.45]).last(), Some(&[-0.8, 2.45]));
    }

    #[test]
    fn a_bad_mode_team_or_zone_is_refused_as_the_server_refuses_it() {
        let reason = |change: fn(&mut Value)| edited(change).unwrap_err().0;
        assert_eq!(reason(|map| map["mode"] = serde_json::json!("race")), "mode must be one of siege, forts, kungfu, titan");
        assert_eq!(reason(|map| { map["mode"] = serde_json::json!("forts"); map["build"] = serde_json::json!(2) }), "build must be between 5 and 900");
        assert_eq!(reason(|map| { map["mode"] = serde_json::json!("forts"); map["build"] = Value::Null }), "build must be a number");
        assert_eq!(reason(|map| map["starts"][0]["team"] = serde_json::json!("mayo")), "starts[0].team must be one of mustard, ketchup");
        assert_eq!(reason(|map| map["zones"] = serde_json::json!([{"kind": "moat", "rect": [0, 0, 1, 1]}])), "zones[0].kind must be one of base, field, bacon");
        assert_eq!(reason(|map| map["zones"] = serde_json::json!([{"rect": [0, 0, 1, 1]}])), "zones[0].kind must be one of base, field, bacon");
        assert_eq!(reason(|map| map["zones"] = serde_json::json!([{"kind": "base", "team": "relish", "rect": [0, 0, 1, 1]}])), "zones[0].team must be one of mustard, ketchup");
        assert_eq!(reason(|map| map["zones"] = serde_json::json!([{"kind": "base", "rect": [0, 0, 1, 1]}])), "zones[0].team must be one of mustard, ketchup");
        assert_eq!(reason(|map| map["zones"] = serde_json::json!([{"kind": "field", "rect": [1, 0, 0, 1]}])), "zones[0].rect is inside out or empty; expected [minX, minZ, maxX, maxZ]");
        assert_eq!(reason(|map| map["zones"] = serde_json::json!([{"kind": "bacon", "p": [1]}])), "zones[0].p must be [x, z]");
        assert_eq!(reason(|map| map["zones"] = serde_json::json!([3])), "zones[0] must be an object");
        assert_eq!(reason(|map| map["zones"] = serde_json::json!({})), "zones must be a list");
        assert_eq!(reason(|map| map["zones"] = Value::Array(vec![serde_json::json!({"kind": "bacon", "p": [1, 3]}); 17])), "zones holds 17 entries; the limit is 16");
        // A start's team may be left out or null, and the build time of a Forts map defaults.
        let map = edited(|map| { map["mode"] = serde_json::json!("forts"); map["starts"][0]["team"] = Value::Null; map["starts"][1]["team"] = serde_json::json!("mustard") }).unwrap();
        assert_eq!((map.build, map.starts[0].team, map.starts[1].team), (150., None, Some(0)));
    }

    // ----- Version 2: sinks, the counter things, styled ramps and ramps down into a sink, the bin, the cabinets.
    fn kitchen() -> Map {
        Map::parse(&read("assets/maps/the-whole-kitchen.json")).unwrap()
    }

    #[test]
    fn a_map_is_written_as_version_1_unless_it_uses_what_version_2_added() {
        // Every map that needs nothing new is still version 1, byte for byte as it was (the canonical test above).
        assert_eq!(Map::bundled().version(), 1.);
        let map = kitchen();
        assert_eq!((map.version(), map.cabinets, map.sinks.len()), (2., true, 1));
        assert!(map.to_text().starts_with("{\n  \"format\": \"hotdog-map\",\n  \"version\": 2,\n"));
        // Each of the new things alone makes a map version 2, and a version 1 file that has them is read all the same.
        let ones: [fn(&mut Value); 6] = [
            |m| m["cabinets"] = serde_json::json!(true),
            |m| m["sinks"] = serde_json::json!([{"rect": [0, 3, 0.4, 3.3]}]),
            |m| m["blocks"][2]["style"] = serde_json::json!("kettle"),
            |m| m["ramps"][0]["style"] = serde_json::json!("wood"),
            |m| m["ramps"][0]["base"] = serde_json::json!(-0.1),
            |m| m["props"] = serde_json::json!([{"kind": "bin", "p": [2.3, 3], "yaw": 1}]),
        ];
        for change in ones {
            let map = edited(change).unwrap();
            assert_eq!(map.version(), 2., "{}", map.to_text());
            assert_eq!(Map::parse(&map.to_text()).unwrap(), map);
        }
        let map = edited(|m| m["sinks"] = serde_json::json!([{"rect": [0, 3, 0.4, 3.3]}])).unwrap();
        assert_eq!(map.sinks, vec![Sink { rect: [0., 3., 0.4, 3.3], depth: SINK_DEPTH }]);
        assert!(map.to_text().contains("  \"sinks\": [\n    {\"rect\": [0, 3, 0.4, 3.3], \"depth\": 0.18}\n  ],\n  \"props\""));
        // A map with no sink, cabinets left out or off, says nothing of either.
        let off = edited(|m| m["cabinets"] = serde_json::json!(false)).unwrap();
        assert_eq!(off.to_text(), Map::bundled().to_text());
        let reason = |change: fn(&mut Value)| edited(change).unwrap_err().0;
        assert_eq!(reason(|m| m["cabinets"] = serde_json::json!("yes")), "cabinets must be true or false");
        assert_eq!(reason(|m| m["sinks"] = serde_json::json!([{"rect": [0, 3, 0.4, 3.3], "depth": 0.6}])), "sinks[0].depth must be between 0.05 and 0.5");
        assert_eq!(reason(|m| m["ramps"][0]["base"] = serde_json::json!(-0.6)), "ramps[0].base must be between -0.5 and 3");
        assert_eq!(reason(|m| m["ramps"][0]["style"] = serde_json::json!("toaster")), "ramps[0].style must be one of steel, stone, wood, dark, white, red, green, yellow, metal, bamboo, card");
        assert_eq!(reason(|m| m["blocks"][2]["style"] = serde_json::json!("blender")).split(" must").next(), Some("blocks[2].style"));
        assert_eq!(reason(|m| m["props"] = serde_json::json!([{"kind": "sofa", "p": [0, 3]}])), "props[0].kind must be one of box, turret, bowl, crate, pillow, napkin, chair, tray, cereal, cup, plate, tin, board, dish, roll, salt, pepper, breadbox, bin");
        // A blanket, by its old name, is read as the napkin it is now.
        assert_eq!(edited(|m| m["props"] = serde_json::json!([{"kind": "blanket", "p": [0, 3]}])).unwrap().loose[0].kind, "napkin");
        let kit = edited(|m| m["blocks"][2]["style"] = serde_json::json!("faucet")).unwrap();
        assert_eq!(kit.blocks[2].facing, Some(Side::MinusZ), "a counter thing faces a side, as an appliance does");
    }

    #[test]
    fn a_sink_is_a_basin_bodies_drop_into_and_climb_out_of_by_its_ramps_and_routes_keep_to_them() {
        let mut level = Level::new(kitchen());
        let [sx0, sz0, sx1, sz1] = level.map.sinks[0].rect;
        let depth = level.map.sinks[0].depth;
        let (bottom, lane) = (FLOOR - depth, 3.48);
        // Its floor is the ground, below the counter; the boards propped in it climb to its rim.
        assert!((level.ground(0., lane) - bottom).abs() < 1e-12 && level.ground(-0.5, lane) == FLOOR && level.ground(0., 3.3) - bottom < 1e-12);
        assert!(level.ground(-0.2, lane) > bottom && level.ground(-0.2, lane) < FLOOR && level.ground(sx0, lane) > FLOOR);
        // Stepping in is a drop; stepping out is a ledge too tall, except up a board.
        assert!(level.can_step([sx0 - 0.01, FLOOR, 3.3], [sx0 + 0.01, FLOOR, 3.3]));
        assert!(!level.can_step([sx0 + 0.01, bottom, 3.3], [sx0 - 0.01, bottom, 3.3]), "the side of the sink is a wall from inside");
        let up = level.ground(-0.1, lane);
        assert!(level.can_step([-0.08, bottom, lane], [-0.1, bottom, lane]) && level.can_step([-0.3, level.ground(-0.3, lane), lane], [-0.32, up, lane]));
        // Routes keep out of the rim but over the boards; the floor inside is clear, and joined to the counter by them.
        assert!(!level.clear(sx0, 3.3) && !level.clear(0., sz0) && !level.clear(0., sz1) && !level.clear(sx1, 3.65));
        assert!(level.clear(0., lane) && level.clear(sx0 - 0.02, lane) && level.clear(sx1 + 0.02, lane) && level.clear(0., 3.3));
        assert!(level.joined([0., 3.3], [-2., 2.5]) && level.joined([0., 3.3], [2., 2.5]));
        // A route across the counter goes round the sink; one to the bottom goes down a board.
        let around = level.path([-1., 3.3], [1., 3.3]);
        assert!(!around.is_empty() && around.iter().all(|p| !(p[0] > sx0 && p[0] < sx1 && p[1] > sz0 && p[1] < sz1 && (p[1] - lane).abs() > 0.06)));
        let down = level.path([-2., 2.5], [0., 3.3]);
        assert!(down.iter().any(|p| (p[1] - lane).abs() < 0.07 && p[0] < sx0 + 0.06), "{down:?}");
        // From the bottom, a walker heads for the foot of a board and then past its top.
        let foot = level.climb_goal([0.1, bottom, 3.3], [2., FLOOR, 3.0]).unwrap();
        assert!(foot[0] > 0. && foot[0] < 0.2 && (foot[2] - lane).abs() < 0.07 && (foot[1] - bottom) < 0.03, "{foot:?}");
        let top = level.climb_goal([0.1, bottom + 0.01, lane], [2., FLOOR, 3.0]).unwrap();
        assert!(top[0] > sx1 && top[1] == FLOOR, "{top:?}");
        // Beside a board, close to its foot but not in front of it, a walker still makes for the foot, not up the board's side.
        let beside = level.climb_goal([-0.16, bottom, 3.405], [-2., FLOOR, 3.0]).unwrap();
        assert!(beside[0] > -0.14 && beside[0] < -0.1 && (beside[2] - lane).abs() < 0.01, "{beside:?}");
        // Rounds: down into the sink they go on to its floor; along the bottom, its side stops them; out of the top, nothing does.
        assert!((level.cover([0., bottom + 0.05, 3.3], [1., 0., 0.]) - sx1).abs() < 1e-12);
        assert!((level.cover([0., bottom + 0.05, 3.3], [0., -1., 0.]) - 0.05).abs() < 1e-12);
        assert_eq!(level.cover([0., bottom + 0.05, 3.3], [0., 1., 0.]), RANGE);
        assert!((level.cover([0., FLOOR + 0.3, 3.], [0., -0.6, 0.8]) - (0.3 + depth) / 0.6).abs() < 1e-12);
        assert_eq!(level.cover([-2., FLOOR + 0.1, 2.3], [1., 0., 0.]), RANGE, "over the counter it is as it was");
        assert!(level.sunk(0., 3.3) && !level.sunk(0., 3.));
        assert!(!Level::new(Map::bundled()).sunken() && level.sunken());
    }

    // ----- Titan's Kitchen: the kitchen floor at night, Titan mode's arena (tools/titans-kitchen.mjs lays it out).
    fn titans() -> Map {
        Map::parse(&read("assets/maps/titans-kitchen.json")).unwrap()
    }

    #[test]
    fn the_kitchen_floor_the_card_style_and_the_floor_kits_are_version_2_and_read_back_as_written() {
        assert_eq!((STYLES[STYLES.len() - 1], &KITS[KITS.len() - 3..]), ("card", &["fridge", "oven", "pet-bowl"][..]));
        let floor = edited(|m| m["floor"] = serde_json::json!(true)).unwrap();
        assert!(floor.floor && floor.version() == 2. && floor.to_text().contains("\n  \"floor\": true,\n  \"bounds\""));
        assert_eq!(Map::parse(&floor.to_text()).unwrap(), floor);
        assert_eq!(edited(|m| m["floor"] = serde_json::json!(false)).unwrap().to_text(), Map::bundled().to_text(), "off, it says nothing");
        assert_eq!(edited(|m| m["floor"] = serde_json::json!(1)).unwrap_err().0, "floor must be true or false");
        assert_eq!(edited(|m| m["decks"][0]["style"] = serde_json::json!("card")).unwrap().version(), 2.);
        for kit in ["fridge", "oven", "pet-bowl"] {
            let map = edited(|m| m["blocks"][2]["style"] = serde_json::json!(kit)).unwrap();
            assert_eq!((map.version(), map.blocks[2].facing), (2., Some(Side::MinusZ)), "{kit}");
        }
    }

    #[test]
    fn titans_kitchen_is_made_for_titan_and_the_gun_modes_that_fit_it_play_there_too() {
        use crate::rules::Mode;
        let map = titans();
        assert_eq!((map.name.as_str(), map.mode.as_str(), map.theme.as_str(), map.floor, map.version()), ("Titan's Kitchen", crate::titan::MODE, "night", true, 2.));
        assert_eq!(Mode::made_for(&map), Mode::Titan);
        assert_eq!(map.modes(), [Mode::Siege, Mode::Ffa, Mode::Cards, Mode::Titan]);
        assert!(map.refuses(Mode::Forts).is_some() && map.refuses(Mode::KungFu).is_some());
        // No other map has the titan layout.
        let folder = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/maps");
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            let other = Map::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(other.refuses(Mode::Titan).is_none(), other == map, "{}", path.display());
        }
        // The editor's numbers for the titan's body (map.mjs TITAN) are titan.rs's.
        let script = read("tools/network/map.mjs");
        let line = script.lines().find(|line| line.starts_with("export const TITAN={")).expect("map.mjs exports TITAN");
        let number = |key: &str| line.split(&format!("{key}:")).nth(1).and_then(|rest| rest.split([',', '}']).next()).and_then(|n| n.parse::<f32>().ok()).unwrap();
        assert!((number("radius") - crate::titan::RADIUS).abs() < 1e-5 && (number("tall") - crate::titan::TALL).abs() < 1e-5, "{line}");
        // It fits in the room the game draws round a floor (its walls are the bounds), well away from the counter maps' kitchen.
        let [x0, z0, x1, z1] = map.bounds;
        assert!(x1 - x0 >= 5. && z1 - z0 >= 4. && z1 <= -5., "{:?}", map.bounds);
    }

    #[test]
    fn in_titans_kitchen_the_titan_walks_all_the_open_floor_and_none_of_the_dens() {
        use crate::titan::{Footing, RADIUS};
        let map = titans();
        let footing = Footing::of(&Level::new(map.clone()));
        // The titan's start (the first) stands in the open, well clear of everything.
        let [sx, sz] = map.starts[0].p.map(|v| v as f32);
        for (dx, dz) in [(0.15, 0.), (-0.15, 0.), (0., 0.15), (0., -0.15)] {
            assert!(footing.clear(sx + dx, sz + dz), "the titan's start is {RADIUS} + 0.15 m clear");
        }
        // Every spot its middle fits is joined to its start (four-connected on a 4 cm grid): no pocket it could be stuck in or never reach.
        let [x0, z0, x1, z1] = map.bounds.map(|v| v as f32);
        let step = 0.04;
        let (cols, rows) = (((x1 - x0) / step).round() as usize, ((z1 - z0) / step).round() as usize);
        let centre = |id: usize| (x0 + ((id % cols) as f32 + 0.5) * step, z0 + ((id / cols) as f32 + 0.5) * step);
        let clear: Vec<bool> = (0..cols * rows).map(|id| { let (x, z) = centre(id); footing.clear(x, z) }).collect();
        let first = ((sz - z0) / step) as usize * cols + ((sx - x0) / step) as usize;
        let (mut seen, mut open) = (vec![false; clear.len()], vec![first]);
        seen[first] = true;
        while let Some(id) = open.pop() {
            let i = id % cols;
            for next in [(i > 0).then(|| id - 1), (i + 1 < cols).then(|| id + 1), id.checked_sub(cols), Some(id + cols)].into_iter().flatten() {
                if next < clear.len() && clear[next] && !seen[next] {
                    seen[next] = true;
                    open.push(next);
                }
            }
        }
        let fits = clear.iter().filter(|c| **c).count();
        assert_eq!(seen.iter().filter(|s| **s).count(), fits, "every spot the titan fits is joined to its start");
        assert!(fits as f32 * step * step > 8., "{} m² for the titan's middle", fits as f32 * step * step);
        // No den holds it: under the table, the chairs, the sideboard, the stool and the cabinets' fronts over the toe-kick it never fits, while a
        // small hotdog walks in (the navigation grid is clear there, and no block stands in the way).
        let level = Level::new(map.clone());
        let dens: Vec<[f64; 4]> = map.props.iter().filter_map(|p| match p { Prop::Box { rect, base, .. } if *base >= 0.17 && *base < 0.8 => Some(*rect), _ => None }).collect();
        assert!(dens.len() >= 12 && footing.dens.len() >= dens.len());
        for den in dens {
            let mut under = 0;
            let (mut x, mut walked) = (den[0], false);
            while x <= den[2] {
                let mut z = den[1];
                while z <= den[3] {
                    assert!(!footing.clear(x as f32, z as f32), "the titan fits at ({x}, {z}) under {den:?}");
                    if level.clear(x, z) && level.walkable(x, FLOOR, z) {
                        under += 1;
                        walked = true;
                    }
                    z += 0.01;
                }
                x += 0.01;
            }
            assert!(walked && under > 0, "a small hotdog walks in under {den:?}");
        }
    }

    #[test]
    fn in_titans_kitchen_the_small_bots_reach_every_start_and_pickup() {
        let mut level = Level::new(titans());
        let from = level.map.starts[1].p;
        let items: Vec<[f64; 2]> = level.map.starts.iter().map(|s| s.p).chain(level.map.pickups.iter().map(|p| p.p)).collect();
        let mut climbs = 0;
        for p in items {
            let ground = level.ground(p[0], p[1]);
            assert!(level.walkable(p[0], ground, p[1]), "{p:?} stands clear");
            if ground > FLOOR {
                // Up on the cardboard box or the step stool: a ramp leads up from the floor.
                assert!(level.climb_goal([from[0], FLOOR, from[1]], [p[0], ground, p[1]]).is_some(), "{p:?} has a ramp up");
                climbs += 1;
                continue;
            }
            assert!(level.joined(from, p) && !level.path(from, p).is_empty(), "a route to {p:?}");
        }
        assert_eq!(climbs, 2, "the ammunition on the box and the sniper on the stool");
    }
}

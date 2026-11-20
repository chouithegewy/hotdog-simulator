// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Hints for how to play, and tooltips by the crosshair: help for a new player, **off to begin with**. The game is
//! played from the head-up display's icons (hud.rs), which assume the basic buttons are known; H brings the words back.
//!
//! * **The hint panel**, small, under the score and the objective at the top of the screen (under the alert, where
//!   that is up), shows a headline for what the round is about and one tip at a time, the next every [`ROTATE`] seconds.
//!   [`hint`] chooses them from the [`Situation`]. In Forts the build phase is about building a base out of what
//!   is loose on the counter, and the fight about the bacon, which the base is there to protect; what is happening
//!   to the bacon takes precedence over the tips. Whenever the situation changes the panel shows its first tip at
//!   once.
//! * **The tooltip**, under the crosshair and the head-up display's key prompt there, says what the keys do here and now ([`tooltip`] from [`Reach`]):
//!   Q at the loose object this player would pick up (the simulation's own choice, [`target`]), or at a draped
//!   napkin it would pull off; E on a pad; and what to do with what it carries, with what putting it down would do
//!   as the placement preview has it ([`Put`], preview.rs): "Q: put down (snaps, locks)", "Q: drape over 3 items".
//! * With the touch controls on, both name the touch buttons, as the page labels them, instead of the keys.
//! * **H** shows or hides both, with the alert's banner (arena.rs) and the messages of what just happened
//!   ([`Hints::on`]; they are **off** to begin with). The choice is remembered for next time (prefs.rs: the browser's
//!   storage, or the native settings file), whoever makes it: H, or a menu calling [`Hints::set`]. Neither is shown in
//!   the demo, while this player is dead, at the Fully Loaded card table (which has its own prompt), in the fridge, or
//!   out of a round.
//!
//! Nothing here decides anything: it reads what the client already has, offline and online alike. The text is
//! ASCII only, as the game's one font is.
use crate::arena::{FoodAlert, Game};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use kitchen_sim::feast::{Feast, EYE, GRASP, LOW, MID, NEXT};
use kitchen_sim::forts::{DROPPED, HELD};
use kitchen_sim::props::State;

/// Seconds each tip is shown before the next.
pub const ROTATE: f32 = 6.;
/// Where the panel's top is when no alert is up, at a 720-pixel window (and in proportion at any other, as the
/// head-up display is): just under the score and the objective at the top of the screen (hud.rs).
const UNDER_LINES: f32 = 88.;
/// Pixels between the alert's foot and the panel, when the alert is up.
const GAP: f32 = 6.;
/// The tooltip's distance under the middle of the screen, where the crosshair is: under the head-up display's key
/// prompt (hud.rs), at a 720-pixel window.
const UNDER_CROSSHAIR: f32 = 54.;
/// Where the choice is remembered (prefs.rs).
pub const KEY: &str = "hints";
/// The colours: the headline in mustard, a pressing line warmer, the tips in the head-up display's own colour,
/// and the note on H dim.
const HEADLINE: Color = Color::srgb(1.0, 0.84, 0.36);
const URGENT: Color = Color::srgb(1.0, 0.62, 0.30);
const TIP: Color = Color::srgb(0.78, 0.94, 0.89);
const NOTE: Color = Color::srgb(0.50, 0.60, 0.57);

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hints>()
            .init_resource::<crate::prefs::Prefs>()
            .add_systems(Startup, (spawn, load))
            .add_systems(Update, (toggle.before(crate::arena::tick), remember.after(toggle)));
    }
}

/// Whether the hints and the tooltips are on (off to begin with), what the panel is showing (its hint, and when
/// (`Time<Real>`) it came up, which the tips count their turns from), and the choice last remembered.
#[derive(Resource, Default)]
pub struct Hints {
    pub on: bool,
    shown: Option<Hint>,
    since: f32,
    saved: Option<bool>,
}
impl Hints {
    /// Turns the hints on or off. (H does, and so may a settings menu; the choice is remembered: [`remember`].)
    pub fn set(&mut self, on: bool) {
        self.on = on;
    }
    pub fn toggle(&mut self) {
        self.set(!self.on);
    }
}
/// Whether a remembered choice (prefs.rs) has the hints on: only an explicit "on" does.
pub fn remembered(choice: Option<&str>) -> bool {
    choice == Some("on")
}

/// Startup: the choice remembered from last time; off when there is none.
pub fn load(mut hints: ResMut<Hints>, prefs: Res<crate::prefs::Prefs>) {
    hints.on = remembered(prefs.get(KEY).as_deref());
    hints.saved = Some(hints.on);
}
/// H (or whatever the hints are bound to: controls.rs) shows or hides the hints and the tooltips.
pub fn toggle(keys: Res<ButtonInput<KeyCode>>, mouse: Option<Res<ButtonInput<MouseButton>>>, bindings: Option<Res<crate::controls::Bindings>>, mut hints: ResMut<Hints>) {
    let none = ButtonInput::default();
    let bindings = bindings.map_or_else(crate::controls::Bindings::default, |b| b.clone());
    if bindings.just(crate::controls::Action::Hints, &keys, mouse.as_deref().unwrap_or(&none)) {
        hints.toggle();
    }
}
/// Every frame: a choice that changed (by H, or by a menu through [`Hints::set`]) is remembered for next time.
pub fn remember(mut hints: ResMut<Hints>, mut prefs: ResMut<crate::prefs::Prefs>) {
    if hints.saved != Some(hints.on) {
        prefs.set(KEY, if hints.on { "on" } else { "off" });
        hints.saved = Some(hints.on);
    }
}

/// What the panel says: a headline, and the tips it takes turns with (at least one). `urgent` marks a line about
/// what is happening now, which is shown alone and in a warmer colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub headline: String,
    pub tips: Vec<String>,
    pub urgent: bool,
}
impl Hint {
    fn new(headline: impl Into<String>, tips: Vec<String>) -> Self {
        Self { headline: headline.into(), tips, urgent: false }
    }
    /// The tip on show `seconds` after the hint came up: each in turn for ROTATE seconds, round and round.
    pub fn tip(&self, seconds: f32) -> &str {
        if self.tips.is_empty() {
            return "";
        }
        let turn = (seconds.max(0.) / ROTATE) as usize % self.tips.len();
        &self.tips[turn]
    }
}

/// The mode, as far as the hints go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// The sandbox, the card table, the demo: no hints.
    #[default]
    None,
    Siege,
    Forts,
    FreeForAll,
}
/// What the round is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    /// A cut scene, the end of a round, the pause between two Forts rounds: nothing to hint.
    #[default]
    Other,
    Warmup,
    Wave,
    Famine,
    Build,
    Fight,
}
/// Forts: where the bacon is, from this player's side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bacon {
    #[default]
    Home,
    Mine,
    Mate,
    Rival,
    Down,
}
/// Everything the hint depends on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Situation {
    pub mode: Mode,
    pub stage: Stage,
    pub bacon: Bacon,
    /// Forts, building: a gun or a box of ammunition is on a pad on this player's side of the forcefield.
    pub armed: bool,
    /// The touch controls are on: the hints name their buttons, not the keys.
    pub touch: bool,
}
impl Situation {
    /// The situation of `g`'s round for this player.
    pub fn of(g: &Game, touch: bool) -> Self {
        let rules = &g.feast.rules;
        let mode = if rules.sandbox || rules.cards || g.demo {
            Mode::None
        } else if rules.ffa {
            Mode::FreeForAll
        } else if g.forts() {
            Mode::Forts
        } else {
            Mode::Siege
        };
        let stage = match g.phase_name() {
            "Warmup" => Stage::Warmup,
            "Siege" => Stage::Wave,
            "FreeForAll" => Stage::Famine,
            "Build" => Stage::Build,
            "Fight" => Stage::Fight,
            _ => Stage::Other,
        };
        let me = g.roster.iter().find(|p| p.id == g.me());
        let bacon = match g.fort.as_ref().map(|f| &f.bacon) {
            Some(b) if b.state == HELD && b.holder == g.me() => Bacon::Mine,
            Some(b) if b.state == HELD => match (me, g.roster.iter().find(|p| p.id == b.holder)) {
                (Some(me), Some(them)) if me.team == them.team => Bacon::Mate,
                _ => Bacon::Rival,
            },
            Some(b) if b.state == DROPPED => Bacon::Down,
            _ => Bacon::Home,
        };
        // A pickup on its pad, on this player's side of the forcefield.
        let side = |p: Vec3| g.feast.level.side(p.x as f64, p.z as f64);
        let armed = stage == Stage::Build && g.spawns().iter().any(|p| p.ready <= g.elapsed && side(Vec3::from_array(p.p)) == side(g.position));
        Self { mode, stage, bacon, armed, touch }
    }
}

/// What the keys (or, with the touch controls on, the buttons) are called in the hints.
struct Words {
    /// Picks up a loose object (Q), and puts it down again.
    carry: &'static str,
    put: &'static str,
    /// Takes a gun or ammunition (E), and eats.
    take: &'static str,
    eat: &'static str,
    /// Strikes, or in the free for all bites (F).
    bite: &'static str,
}
fn words(touch: bool) -> Words {
    if touch {
        Words { carry: "Carry", put: "Put down", take: "Pick up", eat: "Eat", bite: "Bite" }
    } else {
        Words { carry: "Q", put: "Q", take: "E", eat: "E", bite: "F" }
    }
}

/// The hint for a situation, or none (no hints in the sandbox, at the card table, between rounds).
pub fn hint(s: &Situation) -> Option<Hint> {
    let w = words(s.touch);
    let pickups = format!("{} takes a gun or ammo from a pad", w.take);
    match (s.mode, s.stage) {
        (Mode::Forts, Stage::Build) => {
            let mut tips = vec![
                format!("Anything loose on the counter is a building block: {} picks it up", w.carry),
                format!("{} again puts it down. Stand boards and cereal boxes up as walls", w.put),
                "Rounds stop at anything you build, so your walls are cover".to_string(),
                "Drape a napkin over a pile to make it a solid wall".to_string(),
            ];
            if s.armed {
                tips.push("Grab a gun from your side's pads before the forcefield drops".into());
            }
            Some(Hint::new("BUILD YOUR BASE", tips))
        }
        (Mode::Forts, Stage::Fight) => {
            let now = match s.bacon {
                Bacon::Mine => Some("BRING THE BACON HOME!"),
                Bacon::Mate => Some("Cover your bacon carrier!"),
                Bacon::Rival => Some("They have the bacon. Stop them!"),
                Bacon::Down => Some("The bacon is down. Grab it!"),
                Bacon::Home => None,
            };
            Some(match now {
                Some(line) => Hint { headline: "STEAL THE BACON".into(), tips: vec![line.into()], urgent: true },
                None => Hint::new(
                    "STEAL THE BACON",
                    vec![
                        "The bacon is in the middle: carry it back to your base to win the round".into(),
                        "Defend your base: its walls cover whoever brings the bacon home".into(),
                        "First team to three rounds wins the match".into(),
                    ],
                ),
            })
        }
        (Mode::Siege, Stage::Warmup | Stage::Wave) => Some(Hint::new(
            "SURVIVE THE CATS",
            vec![
                pickups,
                "Zombie cats pour out of the microwave: clear the wave together".into(),
                "Keep moving: a hotdog that camps draws a swarm of flies".into(),
            ],
        )),
        (Mode::Siege, Stage::Famine) => Some(Hint::new(
            "EAT OR BE EATEN",
            vec![
                format!("Your HP is draining: {} eats a fallen hotdog", w.eat),
                format!("{} bites a living hotdog in reach and heals you", w.bite),
                "The last hotdog standing wins".into(),
            ],
        )),
        (Mode::FreeForAll, Stage::Famine | Stage::Fight) => Some(Hint::new(
            format!("FREE FOR ALL: first to {} kills", kitchen_sim::rules::FFA_KILLS),
            vec![pickups, "Everyone is your rival, and you are back five seconds after you die".into()],
        )),
        _ => None,
    }
}

/// What Q would take hold of: a loose object, or a draped napkin to pull off (by the drape's number).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Loose(usize),
    Drape(u32),
}
/// The loose object a hotdog with its feet at `feet` and its muzzle at `muzzle`, looking along `direction`,
/// would pick up with Q, or the draped napkin it would pull off: `Feast::grab`'s own choice, made without
/// grabbing anything (the simulation keeps no such function apart from `grab`, which this follows step by step).
/// The one it looks at within GRASP metres of its muzzle (short of any block), or failing that a draped wall
/// it looks at within that reach, or the nearest loose object whose origin is within NEXT metres of its feet
/// (and not much above or below them), or a draped wall it stands against. Something that cannot be picked
/// up (carried by another, draped, out of the round) is nothing.
pub fn target(feast: &Feast, feet: Vec3, muzzle: Vec3, direction: Vec3) -> Option<Target> {
    let (props, d) = (&feast.props, direction.normalize_or_zero());
    let loose = |prop: usize| matches!(props.state(prop), State::Awake | State::Asleep | State::Locked);
    let mut found = None;
    let mut tent = None;
    if d.length_squared() > 0.81 {
        let wall = feast.level.cover_from(muzzle, d);
        found = props.raycast(muzzle.as_dvec3().to_array(), d.as_dvec3().to_array(), wall.min(GRASP) as f64, None).map(|hit| hit.id);
        if found.is_none() {
            tent = feast
                .drapes
                .iter()
                .find(|t| {
                    let (low, high) = (Vec3::new(t.rect[0] as f32, kitchen_sim::feast::FLOOR - 1., t.rect[1] as f32), Vec3::new(t.rect[2] as f32, t.top as f32, t.rect[3] as f32));
                    // (Or the napkin itself, as `grab` has it.)
                    let e = props.extent(t.napkin);
                    let pad = || kitchen_sim::ray_box(muzzle, d, Vec3::new(e[0] as f32, e[4] as f32, e[1] as f32), Vec3::new(e[2] as f32, e[5] as f32, e[3] as f32));
                    kitchen_sim::ray_box(muzzle, d, low, high).or_else(pad).is_some_and(|near| near <= wall.min(GRASP) + 1e-3)
                })
                .map(|t| t.id);
        }
    }
    let at = feet.as_dvec3();
    if found.is_none() && tent.is_none() {
        let mut least = NEXT;
        for prop in (0..props.len()).filter(|prop| loose(*prop)) {
            let p = props.position(prop);
            let far = (p[0] - at.x).hypot(p[2] - at.z);
            if far < least && (p[1] - at.y).abs() < 0.2 {
                (least, found) = (far, Some(prop));
            }
        }
    }
    if found.is_none() && tent.is_none() {
        let mut least = kitchen_sim::map::MARGIN + 0.06;
        for t in &feast.drapes {
            let (x, z) = ((t.rect[0] - at.x).max(0.).max(at.x - t.rect[2]), (t.rect[1] - at.z).max(0.).max(at.z - t.rect[3]));
            let far = x.hypot(z);
            if far < least && at.y < t.top {
                (least, tent) = (far, Some(t.id));
            }
        }
    }
    match (tent, found) {
        (Some(t), _) => Some(Target::Drape(t)),
        (None, Some(prop)) if loose(prop) => Some(Target::Loose(prop)),
        _ => None,
    }
}

/// What this player could take hold of with Q, as the tooltip names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grasp {
    /// A loose object, by its kind (map::LOOSE's order).
    Loose(usize),
    /// A draped napkin, to pull off.
    Drape,
}
/// The pad this player stands at, with something on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pad {
    /// A gun, by name.
    Gun(String),
    /// A box of ammunition, and whether a gun this player holds is short of its full load (else it is left).
    /// `power`: a power weapon held is short, which a box does not top up (in every gun mode).
    Ammo { short: bool, power: bool },
}
/// What Q would do with what this player carries, as the placement preview has it (preview.rs `Spot::put`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Put {
    /// Let go on the move, with the hotdog's pace: it settles where the physics takes it, and locks there.
    Moving,
    /// Put down standing still: whether it snaps onto or beside something, and whether it is locked where it lands
    /// (or settles first, and locks where it comes to rest).
    Still { snaps: bool, locks: bool },
    /// A napkin that drapes into a wall, over this many loose objects (none: over an edge alone).
    Drape(usize),
    /// A napkin that lies flat: nothing to drape over, or a drape that would be refused, and why.
    Flat(Option<kitchen_sim::drape::Refusal>),
    /// It may not be put down there at all (Forts: in a base), and why. Q leaves it in the hands.
    Refused(kitchen_sim::drape::Refusal),
}
/// Everything the tooltip depends on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reach {
    /// The kind of loose object in this player's hands (map::LOOSE's order), and what putting it down would do (None
    /// when the preview has not said).
    pub carrying: Option<usize>,
    pub put: Option<Put>,
    pub grasp: Option<Grasp>,
    pub pad: Option<Pad>,
    pub touch: bool,
}
impl Reach {
    /// What this player can reach in `g`'s round now: its hotdog alive, in a round that lets it.
    pub fn of(g: &Game, touch: bool) -> Self {
        let mut reach = Self { touch, ..default() };
        // A round that is over (or the pause between two Forts rounds) takes nothing from the keys.
        let Some(me) = g.roster.iter().find(|p| p.id == g.me() && p.alive).filter(|_| !matches!(g.phase_name(), "RoundOver" | "Rest")) else {
            return reach;
        };
        let props = &g.feast.props;
        let carried = usize::try_from(me.carry).ok().filter(|prop| *prop < props.len());
        reach.carrying = carried.map(|prop| props.body(prop).shape);
        let bacon = g.fort.as_ref().is_some_and(|f| f.bacon.state == HELD && f.bacon.holder == me.id);
        // Q picks up nothing with its hands full (the bacon included).
        if carried.is_none() && !bacon {
            let muzzle = g.position + Vec3::Y * if g.prone { LOW } else if g.crouch { MID } else { EYE };
            reach.grasp = target(&g.feast, g.position, muzzle, g.direction).map(|t| match t {
                Target::Loose(prop) => Grasp::Loose(props.body(prop).shape),
                Target::Drape(_) => Grasp::Drape,
            });
        }
        // A pad with its pickup on it, as near as E takes it (arena::tick). (The pickup of a gun this player holds is
        // taken by walking over it, or left when that gun's reserve is full: E has nothing to do with it.)
        if !g.infected {
            let near = g.position + Vec3::Y * 0.04;
            let pad = g.spawns().into_iter().filter(|p| p.ready <= g.elapsed).find(|p| Vec3::from_array(p.p).distance(near) < 0.15).map(|p| p.weapon);
            let pad = pad.filter(|weapon| *weapon == kitchen_sim::map::AMMO_BOX || !g.owned.get(*weapon).copied().unwrap_or(false));
            reach.pad = pad.map(|weapon| {
                if weapon == kitchen_sim::map::AMMO_BOX {
                    let (short, line) = ammo_box(&g.owned, &g.mag.reserve);
                    Pad::Ammo { short, power: line.contains("POWER") }
                } else {
                    Pad::Gun(crate::arena::Weapon::index(weapon.max(1)).name().to_lowercase())
                }
            });
        }
        reach
    }
}

/// What the line at a box of ammunition says, and whether it is taken: walked over, it tops up the reserves of the guns
/// held that are short of what they carry (kitchen-sim magazine.rs), but not the power weapons (the ketchup sniper and the
/// relish bazooka), which it leaves as they are in every gun mode (`kitchen_sim::feast::refills`). `reserve` is each gun's.
pub fn ammo_box(owned: &[bool; kitchen_sim::feast::GUNS], reserve: &[i32; kitchen_sim::feast::GUNS]) -> (bool, &'static str) {
    let refills = !kitchen_sim::feast::refills(owned, reserve).is_empty();
    let power = (1..kitchen_sim::feast::GUNS).any(|w| owned[w] && reserve[w] < kitchen_sim::magazine::CARRY[w] && kitchen_sim::magazine::power(w));
    match (refills, power) {
        (true, false) => (true, "AMMO BOX | WALK OVER IT TO REFILL YOUR RESERVES"),
        (true, true) => (true, "AMMO BOX | WALK OVER IT TO REFILL YOUR RESERVES (NOT THE POWER WEAPONS)"),
        (false, true) => (false, "AMMO BOX | NO REFILLS FOR POWER WEAPONS"),
        (false, false) => (false, "AMMO BOX | YOUR RESERVES ARE FULL"),
    }
}

/// What putting down does, as the tooltip words it: for the keys (after "Q: ") and for the touch buttons (after "Tap Put
/// down to ").
pub fn putting(put: Put) -> (String, String) {
    use kitchen_sim::drape::Refusal;
    let both = |key: &str, touch: &str| (key.to_string(), touch.to_string());
    match put {
        Put::Moving => both("put down (stand still to lock)", "set it down (stand still to lock)"),
        Put::Still { snaps: true, locks: true } => both("put down (snaps, locks)", "set it down (snaps, locks)"),
        Put::Still { snaps: false, locks: true } => both("put down (locks)", "set it down (locks)"),
        Put::Still { snaps: true, locks: false } => both("put down (snaps, settles)", "set it down (snaps, settles)"),
        Put::Still { snaps: false, locks: false } => both("put down (settles)", "set it down (settles)"),
        Put::Drape(0) => both("drape over the edge", "drape it over the edge"),
        Put::Drape(n) => {
            let items = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
            (format!("drape over {items}"), format!("drape it over {items}"))
        }
        Put::Flat(why) => {
            let why = match why {
                None => "no pile to drape",
                Some(Refusal::Spot) => "can't drape here",
                Some(Refusal::Stuck) => "someone is in the way",
                Some(Refusal::Sealed) => "a wall would seal a base",
                Some(Refusal::Base) => "not in a base",
            };
            (format!("put down flat ({why})"), format!("lay it flat ({why})"))
        }
        Put::Refused(_) => both("can't put down in a base", "set it down outside the base"),
    }
}

/// The tooltip for what this player can reach, one line for each thing it can do, or none.
pub fn tooltip(r: &Reach) -> Option<String> {
    let name = |kind: usize| kitchen_sim::map::LOOSE_NAMES.get(kind).copied().unwrap_or("it");
    let napkin = kitchen_sim::drape::NAPKIN;
    let mut lines = vec![];
    match (r.carrying, r.put.map(putting), r.touch) {
        (Some(_), Some((key, _)), false) => lines.push(format!("Q: {key} | Left-click: throw")),
        (Some(_), Some((_, touch)), true) => lines.push(format!("Tap Put down to {touch}, or Throw")),
        (Some(kind), None, false) if kind == napkin => lines.push("Q: put down over a pile to drape | Left-click: throw".to_string()),
        (Some(_), None, false) => lines.push("Q: put down | Left-click: throw".into()),
        (Some(kind), None, true) if kind == napkin => lines.push("Tap Put down over a pile to drape it, or Throw".into()),
        (Some(_), None, true) => lines.push("Tap Put down to set it down, or Throw".into()),
        (None, _, _) => {}
    }
    match (r.grasp, r.touch) {
        (Some(Grasp::Loose(kind)), false) => lines.push(format!("Q: pick up {}", name(kind))),
        (Some(Grasp::Drape), false) => lines.push("Q: pull the napkin off".into()),
        (Some(Grasp::Loose(kind)), true) => lines.push(format!("Tap Carry to pick up the {}", name(kind))),
        (Some(Grasp::Drape), true) => lines.push("Tap Carry to pull the napkin off".into()),
        (None, _) => {}
    }
    match (&r.pad, r.touch) {
        (Some(Pad::Gun(gun)), false) => lines.push(format!("E: take {gun}")),
        (Some(Pad::Gun(gun)), true) => lines.push(format!("Tap Pick up to take the {gun}")),
        (Some(Pad::Ammo { short: true, .. }), _) => lines.push("Ammo box: walk over it to refill".into()),
        (Some(Pad::Ammo { short: false, power: true }), _) => lines.push("Ammo box: no refills for power weapons".into()),
        (Some(Pad::Ammo { short: false, power: false }), _) => lines.push("Ammo box: your reserves are full".into()),
        (None, _) => {}
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Whether this player gets hints now: playing its own living hotdog in a round (not the demo, a cut scene, the
/// fridge, the card table or the death camera).
pub fn wanted(g: &Game) -> bool {
    let alive = g.roster.iter().any(|p| p.id == g.me() && p.alive);
    g.playing() && !g.demo && alive && !crate::cards::at_table(g)
}

/// The panel (its row across the screen, so that it is centred, and the box that is seen), its lines, and the tooltip.
#[derive(Component)]
struct Row;
/// The hint panel's box: what the name tags keep under (tags.rs).
#[derive(Component)]
pub struct Board;
#[derive(Component)]
struct Headline;
#[derive(Component)]
struct Note;
#[derive(Component)]
struct Tip;
#[derive(Component)]
struct Tooltip;
#[derive(Component)]
struct TooltipText;

fn text(line: &str, size: f32, colour: Color) -> impl Bundle {
    (
        Text::new(line),
        TextFont { font_size: FontSize::Px(size), ..default() },
        TextColor(colour),
        TextLayout::no_wrap().with_justify(Justify::Center),
    )
}
/// The panel and the tooltip, hidden until a round shows them.
fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Row,
            Name::new("Hints"),
            Node { position_type: PositionType::Absolute, left: Val::Px(0.), right: Val::Px(0.), top: Val::Px(UNDER_LINES), justify_content: JustifyContent::Center, ..default() },
            Visibility::Hidden,
        ))
        .with_children(|row| {
            row.spawn((
                Board,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(12.), Val::Px(5.)),
                    row_gap: Val::Px(3.),
                    border_radius: BorderRadius::all(Val::Px(5.)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.025, 0.045, 0.05, 0.72)),
            ))
            .with_children(|board| {
                board.spawn(Node { column_gap: Val::Px(10.), align_items: AlignItems::Center, ..default() }).with_children(|top| {
                    top.spawn((Headline, text("", 14., HEADLINE)));
                    top.spawn((Note, text("H: hide", 10., NOTE)));
                });
                board.spawn((Tip, text("", 13., TIP)));
            });
        });
    commands
        .spawn((
            Tooltip,
            Name::new("Tooltip"),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                right: Val::Px(0.),
                top: Val::Percent(50.),
                margin: UiRect::top(Val::Px(UNDER_CROSSHAIR)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|row| {
            row.spawn((
                Node { padding: UiRect::axes(Val::Px(6.), Val::Px(2.)), border_radius: BorderRadius::all(Val::Px(3.)), ..default() },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.5)),
            ))
            .with_children(|tip| {
                tip.spawn((TooltipText, text("", 13., TIP)));
            });
        });
}

fn set_text<T: Component>(world: &mut World, line: &str, colour: Option<Color>) {
    for (mut text, mut ink) in world.query_filtered::<(&mut Text, &mut TextColor), With<T>>().iter_mut(world) {
        if text.0 != line {
            text.0 = line.to_string();
        }
        if let Some(colour) = colour.filter(|c| ink.0 != *c) {
            ink.0 = colour;
        }
    }
}
fn set_shown<T: Component>(world: &mut World, on: bool) {
    let wanted = if on { Visibility::Inherited } else { Visibility::Hidden };
    for mut vis in world.query_filtered::<&mut Visibility, With<T>>().iter_mut(world) {
        if *vis != wanted {
            *vis = wanted;
        }
    }
}

/// Every frame the head-up display is (arena::update_hud): the panel and the tooltip for this player now, or
/// neither.
pub fn show(world: &mut World, g: &Game) {
    let now = world.get_resource::<Time<Real>>().map_or(0., |t| t.elapsed_secs());
    let touch = crate::touch::on();
    let on = world.get_resource::<Hints>().is_some_and(|h| h.on) && wanted(g);
    let hint = if on { hint(&Situation::of(g, touch)) } else { None };
    // (What putting down would do: the preview of this frame, preview.rs.)
    let put = world.get_resource::<crate::preview::Preview>().and_then(|p| p.spot(g.frame())).map(|s| s.put());
    let tooltip = if on { tooltip(&Reach { put, ..Reach::of(g, touch) }) } else { None };
    // The tips start again from the first whenever the hint changes.
    let since = {
        let Some(mut hints) = world.get_resource_mut::<Hints>() else {
            return;
        };
        if hints.shown != hint {
            (hints.shown, hints.since) = (hint.clone(), now);
        }
        hints.since
    };
    set_shown::<Row>(world, hint.is_some());
    set_shown::<Tooltip>(world, tooltip.is_some());
    if let Some(hint) = &hint {
        set_text::<Headline>(world, &hint.headline, None);
        set_text::<Tip>(world, hint.tip(now - since), Some(if hint.urgent { URGENT } else { TIP }));
        set_shown::<Note>(world, !touch);
        // Under the alert where it is up (as the frame before laid it out), or else under the round's lines.
        let alert = world
            .query_filtered::<(&Visibility, &ComputedNode, &UiGlobalTransform), With<FoodAlert>>()
            .iter(world)
            .find(|(vis, laid, _)| **vis != Visibility::Hidden && laid.size().y > 0.)
            .map(|(_, laid, at)| (at.translation.y + laid.size().y / 2.) * laid.inverse_scale_factor);
        let under = UNDER_LINES * scale(world);
        let top = Val::Px(alert.map_or(under, |foot| (foot + GAP).max(under)).round());
        for mut node in world.query_filtered::<&mut Node, With<Row>>().iter_mut(world) {
            if node.top != top {
                node.top = top;
            }
        }
    }
    if let Some(line) = &tooltip {
        set_text::<TooltipText>(world, line, None);
        let margin = UiRect::top(Val::Px((UNDER_CROSSHAIR * scale(world)).round()));
        for mut node in world.query_filtered::<&mut Node, With<Tooltip>>().iter_mut(world) {
            if node.margin != margin {
                node.margin = margin;
            }
        }
    }
}
/// The head-up display's scale (hud.rs): the window's height over 720 pixels.
fn scale(world: &mut World) -> f32 {
    world.query::<&Window>().iter(world).next().map_or(1., |w| (w.height() / crate::hud::SCALE).clamp(0.8, 4.))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::map::{Level, Map};

    fn forts(stage: Stage) -> Situation {
        Situation { mode: Mode::Forts, stage, ..default() }
    }

    #[test]
    fn the_build_phase_is_about_building_a_base_and_the_gun_tip_waits_for_a_gun_on_a_pad() {
        let hint = hint(&forts(Stage::Build)).unwrap();
        assert_eq!(hint.headline, "BUILD YOUR BASE");
        assert_eq!(hint.tips[0], "Anything loose on the counter is a building block: Q picks it up");
        assert!(hint.tips.iter().any(|t| t.contains("Stand boards")) && hint.tips.iter().any(|t| t.contains("Drape a napkin")));
        assert!(!hint.tips.iter().any(|t| t.contains("Grab a gun")), "no gun on a pad, no tip about one");
        let armed = hint_for(Situation { armed: true, ..forts(Stage::Build) });
        assert_eq!(armed.tips.last().unwrap(), "Grab a gun from your side's pads before the forcefield drops");
        // With the touch controls on, the buttons are named.
        let touch = hint_for(Situation { touch: true, ..forts(Stage::Build) });
        assert_eq!(touch.tips[0], "Anything loose on the counter is a building block: Carry picks it up");
        assert!(touch.tips[1].starts_with("Put down again puts it down"));
    }
    fn hint_for(s: Situation) -> Hint {
        hint(&s).unwrap()
    }

    #[test]
    fn the_fight_is_about_the_bacon_and_what_happens_to_it_comes_first() {
        let fight = hint_for(forts(Stage::Fight));
        assert_eq!(fight.headline, "STEAL THE BACON");
        assert!(!fight.urgent && fight.tips.len() == 3);
        assert_eq!(fight.tips[0], "The bacon is in the middle: carry it back to your base to win the round");
        assert!(fight.tips.iter().any(|t| t.starts_with("Defend your base")));
        assert!(fight.tips.contains(&"First team to three rounds wins the match".to_string()));
        for (bacon, line) in [
            (Bacon::Mine, "BRING THE BACON HOME!"),
            (Bacon::Mate, "Cover your bacon carrier!"),
            (Bacon::Rival, "They have the bacon. Stop them!"),
            (Bacon::Down, "The bacon is down. Grab it!"),
        ] {
            let now = hint_for(Situation { bacon, ..forts(Stage::Fight) });
            assert!(now.urgent && now.tips == vec![line.to_string()], "{bacon:?}: {now:?}");
            // Shown at once, and kept: there is no other tip to turn to.
            assert_eq!(now.tip(0.), line);
            assert_eq!(now.tip(31.), line);
        }
        // Between two rounds, nothing.
        assert_eq!(hint(&forts(Stage::Other)), None);
    }

    #[test]
    fn the_siege_and_free_for_all_have_headlines_of_their_own_and_the_sandbox_none() {
        let siege = |stage| hint(&Situation { mode: Mode::Siege, stage, ..default() });
        for stage in [Stage::Warmup, Stage::Wave] {
            let h = siege(stage).unwrap();
            assert_eq!(h.headline, "SURVIVE THE CATS");
            assert!(h.tips.iter().any(|t| t == "E takes a gun or ammo from a pad"));
        }
        let famine = siege(Stage::Famine).unwrap();
        assert!(famine.tips.iter().any(|t| t.contains("E eats a fallen hotdog")));
        assert_eq!(siege(Stage::Other), None, "the round is over");
        let ffa = hint(&Situation { mode: Mode::FreeForAll, stage: Stage::Famine, ..default() }).unwrap();
        assert_eq!(ffa.headline, format!("FREE FOR ALL: first to {} kills", kitchen_sim::rules::FFA_KILLS));
        assert_eq!(ffa.headline, "FREE FOR ALL: first to 25 kills");
        assert_eq!(hint(&Situation { mode: Mode::None, stage: Stage::Famine, ..default() }), None);
        // Every line is ASCII, which the game's font has.
        for mode in [Mode::Siege, Mode::Forts, Mode::FreeForAll] {
            for stage in [Stage::Warmup, Stage::Wave, Stage::Famine, Stage::Build, Stage::Fight] {
                for bacon in [Bacon::Home, Bacon::Mine, Bacon::Mate, Bacon::Rival, Bacon::Down] {
                    for (armed, touch) in [(false, false), (true, true)] {
                        if let Some(h) = hint(&Situation { mode, stage, bacon, armed, touch }) {
                            assert!(h.headline.is_ascii() && h.tips.iter().all(|t| t.is_ascii() && t.len() < 80), "{h:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_tip_is_shown_for_six_seconds_and_then_the_next_round_and_round() {
        let h = hint_for(forts(Stage::Fight));
        assert_eq!(h.tip(0.), h.tips[0]);
        assert_eq!(h.tip(5.9), h.tips[0]);
        assert_eq!(h.tip(6.1), h.tips[1]);
        assert_eq!(h.tip(12.1), h.tips[2]);
        assert_eq!(h.tip(18.1), h.tips[0]);
    }

    #[test]
    fn the_tooltip_says_what_q_and_e_do_here_and_now() {
        let none = Reach::default();
        assert_eq!(tooltip(&none), None);
        let pillow = Reach { grasp: Some(Grasp::Loose(2)), ..default() };
        assert_eq!(tooltip(&pillow).unwrap(), "Q: pick up pillow");
        for (kind, name) in [(0, "bowl"), (1, "crate"), (3, "napkin"), (4, "chair"), (5, "tray"), (6, "cereal box"), (10, "cutting board"), (12, "paper towel roll"), (13, "salt shaker")] {
            assert_eq!(tooltip(&Reach { grasp: Some(Grasp::Loose(kind)), ..default() }).unwrap(), format!("Q: pick up {name}"));
        }
        assert_eq!(tooltip(&Reach { grasp: Some(Grasp::Drape), ..default() }).unwrap(), "Q: pull the napkin off");
        assert_eq!(tooltip(&Reach { pad: Some(Pad::Gun("syrup rifle".into())), ..default() }).unwrap(), "E: take syrup rifle");
        assert_eq!(tooltip(&Reach { pad: Some(Pad::Ammo { short: true, power: false }), ..default() }).unwrap(), "Ammo box: walk over it to refill");
        assert_eq!(tooltip(&Reach { pad: Some(Pad::Ammo { short: false, power: false }), ..default() }).unwrap(), "Ammo box: your reserves are full");
        assert_eq!(tooltip(&Reach { pad: Some(Pad::Ammo { short: false, power: true }), ..default() }).unwrap(), "Ammo box: no refills for power weapons");
        assert_eq!(tooltip(&Reach { carrying: Some(4), ..default() }).unwrap(), "Q: put down | Left-click: throw");
        assert_eq!(tooltip(&Reach { carrying: Some(3), ..default() }).unwrap(), "Q: put down over a pile to drape | Left-click: throw");
        // Carrying on a pad: both.
        let both = Reach { carrying: Some(1), pad: Some(Pad::Gun("ketchup sniper".into())), ..default() };
        assert_eq!(tooltip(&both).unwrap(), "Q: put down | Left-click: throw\nE: take ketchup sniper");
        // By touch, the buttons as the page labels them.
        let touch = |r: Reach| tooltip(&Reach { touch: true, ..r }).unwrap();
        assert_eq!(touch(pillow.clone()), "Tap Carry to pick up the pillow");
        assert_eq!(touch(Reach { carrying: Some(4), ..default() }), "Tap Put down to set it down, or Throw");
        assert_eq!(touch(Reach { carrying: Some(3), ..default() }), "Tap Put down over a pile to drape it, or Throw");
        assert_eq!(touch(Reach { pad: Some(Pad::Gun("syrup rifle".into())), ..default() }), "Tap Pick up to take the syrup rifle");
    }

    #[test]
    fn the_tooltip_says_what_putting_down_would_do_as_the_preview_has_it() {
        use kitchen_sim::drape::Refusal;
        let line = |kind: usize, put: Put, touch: bool| tooltip(&Reach { carrying: Some(kind), put: Some(put), touch, ..default() }).unwrap();
        let still = |snaps, locks| Put::Still { snaps, locks };
        for (put, words) in [
            (Put::Moving, "put down (stand still to lock)"),
            (still(true, true), "put down (snaps, locks)"),
            (still(false, true), "put down (locks)"),
            (still(true, false), "put down (snaps, settles)"),
            (still(false, false), "put down (settles)"),
        ] {
            assert_eq!(line(4, put, false), format!("Q: {words} | Left-click: throw"));
            assert_eq!(line(4, put, true), format!("Tap Put down to {}, or Throw", words.replacen("put down", "set it down", 1)));
        }
        assert_eq!(line(3, Put::Drape(3), false), "Q: drape over 3 items | Left-click: throw");
        assert_eq!(line(3, Put::Drape(1), false), "Q: drape over 1 item | Left-click: throw");
        assert_eq!(line(3, Put::Drape(0), false), "Q: drape over the edge | Left-click: throw");
        assert_eq!(line(3, Put::Drape(12), true), "Tap Put down to drape it over 12 items, or Throw");
        assert_eq!(line(3, Put::Flat(None), false), "Q: put down flat (no pile to drape) | Left-click: throw");
        assert_eq!(line(3, Put::Flat(None), true), "Tap Put down to lay it flat (no pile to drape), or Throw");
        assert_eq!(line(3, Put::Flat(Some(Refusal::Sealed)), false), "Q: put down flat (a wall would seal a base) | Left-click: throw");
        assert_eq!(line(3, Put::Flat(Some(Refusal::Spot)), false), "Q: put down flat (can't drape here) | Left-click: throw");
        assert_eq!(line(3, Put::Flat(Some(Refusal::Stuck)), false), "Q: put down flat (someone is in the way) | Left-click: throw");
        // In a base, nothing is put down at all.
        assert_eq!(line(10, Put::Refused(Refusal::Base), false), "Q: can't put down in a base | Left-click: throw");
        assert_eq!(line(10, Put::Refused(Refusal::Base), true), "Tap Put down to set it down outside the base, or Throw");
        // Short, and ASCII only (the game's font has no middle dot or times sign).
        for put in [Put::Moving, still(true, true), still(false, false), Put::Drape(25), Put::Drape(0), Put::Flat(None), Put::Flat(Some(Refusal::Sealed)), Put::Flat(Some(Refusal::Stuck)), Put::Refused(Refusal::Base)] {
            for touch in [false, true] {
                let text = line(3, put, touch);
                assert!(text.is_ascii() && text.len() <= 64, "{text}");
            }
        }
        // Without a preview (none this frame), as before.
        assert_eq!(tooltip(&Reach { carrying: Some(4), ..default() }).unwrap(), "Q: put down | Left-click: throw");
    }

    #[test]
    fn in_the_game_the_tooltip_follows_this_frames_preview() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = crate::arena::test_app("");
        app.init_resource::<Hints>().init_asset::<crate::preview::GhostMaterial>();
        app.world_mut().run_system_once(spawn).unwrap();
        app.world_mut().resource_mut::<Hints>().set(true);
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maps/pillow-fort.json")).unwrap();
        let map = Map::parse(&text).unwrap();
        crate::loose::build(app.world_mut(), &map);
        let tip = |app: &mut App, pace: [f32; 2], napkin: bool| -> String {
            let world = app.world_mut();
            let mut g = world.remove_resource::<Game>().unwrap();
            if g.feast.level.map.name != map.name {
                g.feast = Feast::with_map(map.clone(), false);
                g.begin();
            }
            // Hotdog 1 standing by the stock, with tin 13 over tin 12 (facing -z), or napkin 7 over the pile of plates with a
            // bowl on top (facing +x).
            let (prop, at, yaw) = if napkin { (7, [-2.025, kitchen_sim::map::FLOOR as f32, 2.71], -std::f32::consts::FRAC_PI_2) } else { (13, [-0.6, kitchen_sim::map::FLOOR as f32, 2.38], 0.) };
            let carried = g.feast.players[0].carry;
            if carried >= 0 && carried as usize != prop {
                g.feast.props.drop_prop(carried as usize, [0.; 3], [0.; 3]);
            }
            if carried as usize != prop || carried < 0 {
                assert!(g.feast.props.pick_up(prop, 1));
            }
            let p = &mut g.feast.players[0];
            (p.p, p.yaw, p.pace, p.carry, p.bot) = (at, yaw, pace, prop as i32, false);
            g.roster = g.feast.players.clone();
            crate::preview::show(world, &mut g, 1. / 60.);
            show(world, &g);
            world.insert_resource(g);
            world.query_filtered::<&Text, With<TooltipText>>().single(world).map(|t| t.0.clone()).unwrap()
        };
        assert_eq!(tip(&mut app, [0.; 2], false), "Q: put down (snaps, locks) | Left-click: throw");
        assert_eq!(tip(&mut app, [0.5, 0.], false), "Q: put down (stand still to lock) | Left-click: throw");
        let drape = tip(&mut app, [0.; 2], true);
        assert!(drape.starts_with("Q: drape over ") && drape.ends_with(" items | Left-click: throw"), "{drape}");
    }

    /// The pillow-fort map's simulation, and its first cereal box (where it stands).
    fn fort() -> (Feast, usize, Vec3) {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maps/pillow-fort.json")).unwrap();
        let feast = Feast::on(Level::fielded(Map::parse(&text).unwrap()), false);
        let cereal = kitchen_sim::map::LOOSE.iter().position(|kind| *kind == "cereal").unwrap();
        let first = (0..feast.props.len()).find(|i| feast.props.body(*i).shape == cereal).expect("a cereal box");
        let p = feast.props.position(first);
        (feast, first, Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
    }
    /// Puts hotdog 1 with its feet at `feet` and asks both: what `target` names, and what `grab` then picks up.
    /// Afterwards the props are as they were.
    fn both(feast: &mut Feast, feet: Vec3, direction: Vec3) -> (Option<Target>, Option<usize>) {
        let states: Vec<_> = (0..feast.props.len()).map(|i| feast.props.get_state(i)).collect();
        feast.players[0].p = feet.to_array();
        (feast.players[0].alive, feast.players[0].carry) = (true, -1);
        let named = target(feast, feet, feet + Vec3::Y * EYE, direction);
        let taken = feast.grab(1, direction).then(|| feast.players[0].carry as usize);
        for (i, state) in states.iter().enumerate() {
            feast.props.set_state(i, state);
        }
        feast.players[0].carry = -1;
        (named, taken)
    }

    #[test]
    fn the_object_q_would_pick_up_is_the_one_the_simulation_picks_up() {
        let mut seen = [0; 3];
        // Round every loose object of the map: looked at from 0.2 m away and a little above, then beside
        // it looking up at nothing (the nearest within NEXT of the feet), then a metre away (nothing).
        let (mut feast, _, _) = fort();
        assert_eq!(feast.players[0].id, 1);
        let count = feast.props.len();
        for prop in 0..count {
            for (out, look_at) in [(0.2, true), (0.12, false), (1.0, false)] {
                for turn in [0_f32, 1.6, 3.2, 4.8] {
                    let p = feast.props.position(prop);
                    let at = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
                    let feet = at + Vec3::new(turn.cos() * out, 0., turn.sin() * out);
                    let look = if look_at { (at + Vec3::Y * 0.02 - (feet + Vec3::Y * EYE)).normalize() } else { Vec3::Y };
                    let (named, taken) = both(&mut feast, feet, look);
                    assert_eq!(named, taken.map(Target::Loose), "prop {prop} from {feet:?} looking {look:?}");
                    seen[if taken == Some(prop) { 0 } else if taken.is_some() { 1 } else { 2 }] += 1;
                }
            }
        }
        assert!(seen[0] > count && seen[2] > 0, "the prop itself, another, nothing: {seen:?}");
        // One already in somebody's hands is nothing to pick up.
        let (_, cereal, at) = fort();
        assert!(feast.props.pick_up(cereal, 2));
        let feet = at + Vec3::new(0., 0., 0.12);
        let (named, taken) = both(&mut feast, feet, Vec3::Y);
        assert_ne!(named, Some(Target::Loose(cereal)));
        assert_eq!(named, taken.map(Target::Loose));
    }

    #[test]
    fn h_shows_and_hides_the_hints_and_the_tooltips() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>().init_resource::<Hints>().add_systems(Update, toggle);
        assert!(!app.world().resource::<Hints>().on, "off to begin with");
        for wanted in [true, false, true] {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyH);
            app.update();
            assert_eq!(app.world().resource::<Hints>().on, wanted);
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::KeyH);
            keys.clear();
            app.update();
            assert_eq!(app.world().resource::<Hints>().on, wanted, "held is not pressed again");
        }
    }

    /// The game as a player meets it: the hints off, until H (or a menu) turns them on; and that choice is what the
    /// next game starts with, from wherever it is kept (prefs.rs: a browser's storage, the native settings file).
    #[test]
    fn the_hints_start_off_and_the_choice_made_is_remembered_for_the_next_game() {
        let start = |prefs: crate::prefs::Prefs| {
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<Hints>()
                .insert_resource(prefs)
                .add_systems(Startup, load)
                .add_systems(Update, (toggle, remember.after(toggle)));
            app.update();
            app
        };
        let press_h = |app: &mut App| {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyH);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::KeyH);
            keys.clear();
        };
        // Nothing remembered: off, and nothing is written until a choice is made.
        let mut app = start(crate::prefs::Prefs::memory());
        assert!(!app.world().resource::<Hints>().on);
        assert_eq!(app.world().resource::<crate::prefs::Prefs>().get(KEY), None);
        // H: on, and remembered.
        press_h(&mut app);
        assert!(app.world().resource::<Hints>().on);
        assert_eq!(app.world().resource::<crate::prefs::Prefs>().get(KEY).as_deref(), Some("on"));
        // The next game starts with them on.
        let prefs = app.world_mut().remove_resource::<crate::prefs::Prefs>().unwrap();
        let mut app = start(prefs);
        assert!(app.world().resource::<Hints>().on, "remembered on");
        // A menu turns them off through `Hints::set`: remembered as well, and the game after starts with them off.
        app.world_mut().resource_mut::<Hints>().set(false);
        app.update();
        assert_eq!(app.world().resource::<crate::prefs::Prefs>().get(KEY).as_deref(), Some("off"));
        let prefs = app.world_mut().remove_resource::<crate::prefs::Prefs>().unwrap();
        assert!(!start(prefs).world().resource::<Hints>().on);
        // Only an explicit "on" turns them on.
        for choice in [None, Some("off"), Some(""), Some("yes"), Some("ON")] {
            assert!(!remembered(choice), "{choice:?}");
        }
        assert!(remembered(Some("on")));
    }

    #[test]
    fn the_panel_and_the_tooltip_show_in_a_round_and_go_with_h_and_with_death() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = crate::arena::test_app("");
        app.init_resource::<Hints>();
        app.world_mut().run_system_once(spawn).unwrap();
        // (Off to begin with: turned on, as H would.)
        app.world_mut().resource_mut::<Hints>().set(true);
        let shown = |app: &mut App| -> (bool, bool, String) {
            let world = app.world_mut();
            let g = world.remove_resource::<Game>().unwrap();
            show(world, &g);
            world.insert_resource(g);
            let row = world.query_filtered::<&Visibility, With<Row>>().single(world).is_ok_and(|v| *v != Visibility::Hidden);
            let tip = world.query_filtered::<&Visibility, With<Tooltip>>().single(world).is_ok_and(|v| *v != Visibility::Hidden);
            let headline = world.query_filtered::<&Text, With<Headline>>().single(world).map(|t| t.0.clone()).unwrap_or_default();
            (row, tip, headline)
        };
        // Before the round: nothing.
        assert!(!shown(&mut app).0);
        // In the siege's warm-up (where a round begins), standing on a pad with its pickup on it.
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.begin();
            assert_eq!(g.phase_name(), "Warmup");
            let pad = g.spawns().first().map(|p| Vec3::from_array(p.p)).expect("the map's pickups");
            g.position = pad - Vec3::Y * 0.04;
        }
        let (row, tip, headline) = shown(&mut app);
        assert!(row && headline == "SURVIVE THE CATS", "{headline}");
        assert!(tip, "on a pad, the tooltip says E takes what is there");
        // H hides both.
        app.world_mut().resource_mut::<Hints>().on = false;
        let (row, tip, _) = shown(&mut app);
        assert!(!row && !tip);
        app.world_mut().resource_mut::<Hints>().on = true;
        assert!(shown(&mut app).0);
        // Dead: neither.
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            let me = g.me();
            g.roster.iter_mut().filter(|p| p.id == me).for_each(|p| p.alive = false);
        }
        let (row, tip, _) = shown(&mut app);
        assert!(!row && !tip);
    }

    /// At a box of ammunition: it refills the reserves that are short (walked over), but not a power weapon's, in
    /// every gun mode (the box leaves the sniper and the bazooka as they are), and says so rather than promising a refill
    /// it will not give.
    #[test]
    fn the_ammo_box_prompt_offers_no_refill_for_a_power_weapon() {
        use kitchen_sim::feast::GUNS;
        use kitchen_sim::magazine::CARRY;
        let (mut owned, mut reserve) = ([false; GUNS], [0; GUNS]);
        (owned[1], reserve[1]) = (true, CARRY[1]);
        (owned[2], reserve[2]) = (true, 0);
        // Nothing to refill but the empty sniper, which a box does not: the line says why.
        assert_eq!(ammo_box(&owned, &reserve), (false, "AMMO BOX | NO REFILLS FOR POWER WEAPONS"));
        // With the pistol short too, the pistol is refilled, the sniper not.
        reserve[1] = 3;
        assert_eq!(ammo_box(&owned, &reserve), (true, "AMMO BOX | WALK OVER IT TO REFILL YOUR RESERVES (NOT THE POWER WEAPONS)"));
        // Another gun short alone: a refill.
        (owned[2], owned[3], reserve[3]) = (false, true, 0);
        assert_eq!(ammo_box(&owned, &reserve), (true, "AMMO BOX | WALK OVER IT TO REFILL YOUR RESERVES"));
        // Full.
        (reserve[1], reserve[3]) = (CARRY[1], CARRY[3]);
        assert_eq!(ammo_box(&owned, &reserve), (false, "AMMO BOX | YOUR RESERVES ARE FULL"));
    }
}
